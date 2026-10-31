//! OneDrive personale, attraverso Microsoft Graph v1.0.
//!
//! # Cosa si chiede, e dove
//!
//! - **Chi**: `GET /me/drive`. L'id del drive è l'id stabile dell'account (non
//!   cambia con l'email né col nome), e il nome da mostrare viene da `/me`.
//! - **Le cartelle del selettore**: i figli di una cartella, a pagine, tenendo
//!   solo quelli con la faccetta `folder`.
//! - **L'elenco**: una visita per livelli dei figli, a pagine da duecento. Il
//!   delta su una cartella esiste solo per gli account personali e andrà
//!   misurato su un account vero; i figli funzionano ovunque e bastano.
//! - **I cambiamenti**: il delta sulla cima del drive. Il primo cursore si
//!   prende con `token=latest` (il `deltaLink`, che è un indirizzo intero);
//!   poi si seguono i `nextLink` fino al `deltaLink` nuovo. Il delta
//!   racconta tutto il drive: a tenere solo quel che sta sotto le radici
//!   scelte ci pensa lo specchio, che conosce gli antenati.
//! - **I byte**: `/items/{id}/content` risponde `302` verso un indirizzo che
//!   porta il permesso dentro di sé. Si ricorda quello, e le finestre si
//!   chiedono lì senza token. Vedi [`DaOneDrive`].
//!
//! # La faccetta `audio`
//!
//! Gli account personali la danno con l'elenco: titolo, artista, album,
//! durata. È quel che fa entrare un brano in libreria col suo nome prima che
//! si legga un byte ([`TagNoti`]). La lettura vera dei tag arriva dopo e ha
//! l'ultima parola.
//!
//! # Di chi ci si fida
//!
//! Il token va solo a Graph. Ogni indirizzo che Graph restituisce e a cui si
//! manderebbe il token — un `nextLink`, un `deltaLink` salvato nel database —
//! deve avere la stessa origine di Graph prima di partire: un cursore scritto
//! da qualcun altro nel database non deve poter portare il token altrove.
//! L'indirizzo firmato del `302` invece non riceve niente di segreto: porta il
//! suo permesso, e lo sceglie Graph su una connessione già autenticata.

use std::collections::{HashSet, VecDeque};
use std::ops::ControlFlow;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::nuvola::{Cambio, Servizio, TagNoti, VoceRemota, interessa};
use rymembra_domain::tempo::istante_ms;
use rymembra_net::flusso::Sorgente;
use rymembra_net::http::{
    Cancello, Corpo, Metodo, Pezzo, Rete, Richiesta, Rimandi, Risposta, origine, percento,
};
use serde_json::Value;

use super::{Accesso, Deposito, Identita, Permesso, errore_cursore_scaduto};

/// Il punto delle API.
pub const GRAPH: &str = "https://graph.microsoft.com/v1.0";

/// Il nome di questo client nei campi di macchina degli errori.
const CHIAVE: &str = "onedrive";

/// Quanto si aspetta una risposta di Graph a una domanda.
///
/// Trenta secondi, come i cataloghi: una pagina da duecento voci arriva in
/// meno di uno, e una che non arriva in trenta non arriverà.
const SCADENZA_DOMANDE: Duration = Duration::from_secs(30);

/// Quante volte si chiede una pagina, in tutto, quando Graph rallenta.
///
/// Tre, come il backup: un `429` chiede di aspettare e la seconda volta passa;
/// uno che non passa alla terza va detto.
const TENTATIVI: u32 = 3;

/// Quante voci per pagina.
///
/// Duecento: il massimo che Graph dà senza discutere sui figli di una cartella.
/// Meno pagine sono meno andate e ritorno, che sono il costo vero di un elenco.
const PER_PAGINA: u32 = 200;

/// I campi delle voci dell'elenco.
///
/// Espliciti: la risposta intera di Graph per un file sono una trentina di
/// campi, e in un elenco da ventimila file la differenza si paga in banda.
const CAMPI_ELENCO: &str =
    "id,name,folder,file,size,lastModifiedDateTime,parentReference,eTag,cTag,audio,deleted";

/// I campi di una voce sola, per gli antenati: `root` per riconoscere la cima.
const CAMPI_VOCE: &str =
    "id,name,folder,file,size,lastModifiedDateTime,parentReference,eTag,cTag,root";

/// I campi delle cartelle del selettore.
const CAMPI_CARTELLE: &str = "id,name,folder,size,lastModifiedDateTime,parentReference,eTag,cTag";

/// Quanti antenati si risalgono, al più.
///
/// Un tetto contro un giro chiuso di genitori, che Graph non dovrebbe mai dare
/// e che senza tetto sarebbe un ciclo infinito di richieste.
const PROFONDITA_MASSIMA: usize = 64;

/// Quanti rimandi segue un pezzo, oltre all'indirizzo firmato.
///
/// Due, sorvegliati: l'indirizzo firmato di solito risponde sul posto, e se un
/// giorno rimandasse ancora il permesso non viaggia comunque oltre il primo
/// salto (vedi `rymembra_net::http::Rete::prendi`).
const RIMANDI_PEZZO: u32 = 2;

/// Le domande a Graph, condivise fra il deposito e le sue sorgenti.
#[derive(Debug, Clone)]
struct Graph {
    /// Le domande: una scadenza per richiesta, nessun rimando (portano il
    /// token).
    domande: Rete,
    /// `https://graph.microsoft.com/v1.0`, tranne che nelle prove.
    api: String,
    /// Il permesso del conto.
    accesso: Arc<Accesso>,
}

impl Graph {
    /// L'indirizzo è di Graph: stessa origine (schema, ospite, porta).
    fn di_casa(&self, url: &str) -> bool {
        origine(url).is_some_and(|o| Some(o) == origine(&self.api))
    }

    /// Un `GET` a Graph, come JSON.
    ///
    /// # Errori
    ///
    /// Vedi [`Self::errore`]; `net.badSchema` per un indirizzo che non è di
    /// Graph (il token non parte) e per una risposta che non è JSON.
    fn json(&self, url: &str) -> Result<Value, AppError> {
        if !self.di_casa(url) {
            return Err(forma(
                "Graph ha dato un indirizzo che non è suo, e il permesso non lo segue",
            ));
        }
        self.domande.ritenta(TENTATIVI, || self.json_una_volta(url))
    }

    /// Un tentativo di [`Self::json`]: un `401` rinfresca il token una volta.
    fn json_una_volta(&self, url: &str) -> Result<Value, AppError> {
        let permesso = self.accesso.permesso()?;
        let mut risposta = self.chiedi(url, &permesso)?;
        if risposta.stato == 401 {
            let nuovo = self.accesso.rifiutato(&permesso)?;
            risposta = self.chiedi(url, &nuovo)?;
        }
        if !risposta.e_andata() {
            return Err(self.errore(&risposta, url));
        }
        serde_json::from_slice(&risposta.corpo)
            .map_err(|err| forma(&format!("la risposta non è JSON: {err}")))
    }

    fn chiedi(&self, url: &str, permesso: &Permesso) -> Result<Risposta, AppError> {
        let autorizzazione = permesso.intestazione();
        self.domande.esegui(Richiesta {
            metodo: Metodo::Get,
            url,
            intestazioni: &[
                ("authorization", &autorizzazione),
                ("accept", "application/json"),
            ],
            corpo: Corpo::Niente,
        })
    }

    /// Dove rimanda `/content`: l'indirizzo firmato, o `None` se Graph serve
    /// sul posto. Un `401` rinfresca il token una volta.
    ///
    /// # Errori
    ///
    /// `cloud.notFound` per un file che non c'è più, `cloud.authExpired`,
    /// `net.*`.
    fn rimando(&self, url: &str) -> Result<Option<String>, AppError> {
        self.domande.ritenta(TENTATIVI, || {
            let permesso = self.accesso.permesso()?;
            let esito = self
                .domande
                .dove_rimanda(url, &[("authorization", &permesso.intestazione())]);
            let esito = match esito {
                Err(err) if stato_http(&err) == Some(401) => {
                    let nuovo = self.accesso.rifiutato(&permesso)?;
                    self.domande
                        .dove_rimanda(url, &[("authorization", &nuovo.intestazione())])
                }
                altro => altro,
            };
            esito.map_err(traduci)
        })
    }

    /// Uno stato di errore di Graph, detto con un codice di Rymembra.
    ///
    /// - `401` dopo il rinfresco: il consenso non vale più → `cloud.authExpired`;
    /// - `404` → `cloud.notFound`;
    /// - `410`, o un codice `resync…` di Graph: il cursore dei cambiamenti non
    ///   vale più → il segnale di [`super::cursore_scaduto`];
    /// - il resto con la traduzione normale (un `429` resta `net.rateLimited`
    ///   col tempo chiesto, e si ritenta).
    fn errore(&self, risposta: &Risposta, url: &str) -> AppError {
        let corpo: Value = serde_json::from_slice(&risposta.corpo).unwrap_or_default();
        let codice = corpo
            .pointer("/error/code")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let messaggio = corpo
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let causa = format!(
            "{codice}: {}",
            messaggio.chars().take(300).collect::<String>()
        );
        if codice.starts_with("resync") || risposta.stato == 410 {
            return errore_cursore_scaduto(&causa);
        }
        match risposta.stato {
            401 => AppError::new(ErrorCode::CloudAuthExpired {
                servizio: Servizio::OneDrive.nome().to_owned(),
            })
            .with_cause(causa),
            404 => AppError::new(ErrorCode::CloudNotFound).with_cause(causa),
            _ => self.domande.stato_a_errore(risposta, url),
        }
    }
}

/// OneDrive personale. Vedi il `//!`.
#[derive(Debug)]
pub struct OneDrive {
    graph: Graph,
    /// I pezzi dei brani: nessuna scadenza complessiva (vedi
    /// `Rete::per_flusso`), rimandi sorvegliati.
    flusso: Rete,
}

impl OneDrive {
    /// OneDrive con questo permesso.
    #[must_use]
    pub fn nuovo(accesso: Arc<Accesso>) -> Self {
        Self {
            graph: Graph {
                domande: Rete::nuova(CHIAVE, SCADENZA_DOMANDE).con_rimandi(Rimandi::Nessuno),
                api: GRAPH.to_owned(),
                accesso,
            },
            flusso: Rete::per_flusso(CHIAVE).con_rimandi(Rimandi::Sorvegliati {
                quanti: RIMANDI_PEZZO,
                cancello: Cancello::nuovo(|bersaglio| bersaglio.starts_with("https://")),
            }),
        }
    }

    /// OneDrive verso un Graph finto, con una rete sola per tutto.
    #[cfg(test)]
    fn per_prova(rete: Rete, api: &str, accesso: Arc<Accesso>) -> Self {
        Self {
            graph: Graph {
                domande: rete.clone(),
                api: api.to_owned(),
                accesso,
            },
            flusso: rete,
        }
    }

    /// I figli di una cartella (la cima se `None`), coi campi dati.
    fn figli(&self, id: Option<&str>, campi: &str) -> String {
        let cartella = id.map_or_else(|| "root".to_owned(), |id| format!("items/{}", percento(id)));
        format!(
            "{}/me/drive/{cartella}/children?$select={campi}&$top={PER_PAGINA}",
            self.graph.api
        )
    }

    /// `cima` e i suoi antenati, dalla cima del drive in giù.
    fn antenati(&self, cima: &str) -> Result<Vec<VoceRemota>, AppError> {
        let mut catena = Vec::new();
        let mut prossimo = Some(cima.to_owned());
        for _ in 0..PROFONDITA_MASSIMA {
            let Some(id) = prossimo.take() else {
                break;
            };
            let item = self.graph.json(&format!(
                "{}/me/drive/items/{}?$select={CAMPI_VOCE}",
                self.graph.api,
                percento(&id)
            ))?;
            let voce = voce(&item).ok_or_else(|| forma("una voce senza identificativo"))?;
            prossimo = voce.genitore.clone();
            catena.push(voce);
        }
        catena.reverse();
        Ok(catena)
    }
}

impl Deposito for OneDrive {
    fn servizio(&self) -> Servizio {
        Servizio::OneDrive
    }

    fn spegni(&self) {
        self.graph.accesso.spegni();
    }

    fn chi(&self) -> Result<Identita, AppError> {
        let drive = self.graph.json(&format!(
            "{}/me/drive?$select=id,driveType,owner",
            self.graph.api
        ))?;
        let id_stabile = drive
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| forma("il drive non ha un identificativo"))?
            .to_owned();
        // Il nome è un ornamento delle Impostazioni: se `/me` non risponde si
        // ripiega sul proprietario del drive, e il collegamento non fallisce.
        let me = self
            .graph
            .json(&format!(
                "{}/me?$select=displayName,mail,userPrincipalName",
                self.graph.api
            ))
            .ok();
        let testo = |valore: Option<&Value>, campo: &str| {
            valore
                .and_then(|v| v.get(campo))
                .and_then(Value::as_str)
                .filter(|t| !t.trim().is_empty())
                .map(ToOwned::to_owned)
        };
        let nome = testo(me.as_ref(), "mail")
            .or_else(|| testo(me.as_ref(), "userPrincipalName"))
            .or_else(|| testo(me.as_ref(), "displayName"))
            .or_else(|| testo(drive.pointer("/owner/user"), "displayName"))
            .unwrap_or_else(|| Servizio::OneDrive.nome().to_owned());
        Ok(Identita { id_stabile, nome })
    }

    fn cartelle(&self, dentro: Option<&str>) -> Result<Vec<VoceRemota>, AppError> {
        let mut cartelle = Vec::new();
        let mut url = Some(self.figli(dentro, CAMPI_CARTELLE));
        while let Some(pagina) = url.take() {
            let corpo = self.graph.json(&pagina)?;
            cartelle.extend(
                valori(&corpo)?
                    .iter()
                    .filter_map(voce)
                    .filter(|v| v.cartella),
            );
            url = dopo(&corpo);
        }
        Ok(cartelle)
    }

    fn cursore_adesso(&self) -> Result<String, AppError> {
        let corpo = self.graph.json(&format!(
            "{}/me/drive/root/delta?token=latest",
            self.graph.api
        ))?;
        corpo
            .get("@odata.deltaLink")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| forma("il delta non ha dato un deltaLink"))
    }

    fn elenca(
        &self,
        cima: &str,
        pagina: &mut dyn FnMut(Vec<VoceRemota>) -> ControlFlow<()>,
    ) -> Result<bool, AppError> {
        // La catena va nella prima pagina, prima di qualunque figlio: lo
        // specchio compone i percorsi risalendo i genitori.
        let mut in_testa = self.antenati(cima)?;
        let mut coda = VecDeque::from([cima.to_owned()]);
        // Contro un giro di cartelle (non dovrebbe esistere): una cartella si
        // visita una volta sola.
        let mut viste: HashSet<String> = HashSet::from([cima.to_owned()]);
        while let Some(cartella) = coda.pop_front() {
            let mut url = Some(self.figli(Some(&cartella), CAMPI_ELENCO));
            while let Some(indirizzo) = url.take() {
                let corpo = self.graph.json(&indirizzo)?;
                let mut voci = std::mem::take(&mut in_testa);
                for item in valori(&corpo)? {
                    let Some(voce) = voce(item) else {
                        continue;
                    };
                    if voce.cartella {
                        if viste.insert(voce.id.clone()) {
                            coda.push_back(voce.id.clone());
                        }
                        voci.push(voce);
                    } else if interessa(&voce.nome) {
                        voci.push(voce);
                    }
                }
                if pagina(voci).is_break() {
                    return Ok(false);
                }
                url = dopo(&corpo);
            }
        }
        Ok(true)
    }

    /// Graph non sa aspettare i cambiamenti: `attesa` non serve, e si risponde
    /// subito. Il ritmo lo tiene il filo del conto.
    fn cambiamenti(
        &self,
        cursore: &str,
        attesa: Duration,
    ) -> Result<(Vec<Cambio>, String), AppError> {
        let _ = attesa;
        // Il cursore viene dal database: se non è di Graph non ci si manda il
        // token. Si tratta come scaduto, così chi chiama rifà l'elenco e se ne
        // procura uno buono.
        if !self.graph.di_casa(cursore) {
            return Err(errore_cursore_scaduto("non è un indirizzo di Graph"));
        }
        let mut cambi = Vec::new();
        let mut url = cursore.to_owned();
        loop {
            let corpo = self.graph.json(&url)?;
            for item in valori(&corpo)? {
                if item.get("deleted").is_some() {
                    if let Some(id) = item.get("id").and_then(Value::as_str) {
                        cambi.push(Cambio::Tolta { id: id.to_owned() });
                    }
                } else if let Some(voce) = voce(item) {
                    cambi.push(Cambio::Voce(voce));
                }
            }
            if let Some(prossima) = dopo(&corpo) {
                url = prossima;
                continue;
            }
            let nuovo = corpo
                .get("@odata.deltaLink")
                .and_then(Value::as_str)
                .ok_or_else(|| forma("il delta non ha dato né un nextLink né un deltaLink"))?;
            return Ok((cambi, nuovo.to_owned()));
        }
    }

    fn sorgente(&self, voce: &VoceRemota) -> Result<Box<dyn Sorgente>, AppError> {
        Ok(Box::new(DaOneDrive {
            contenuto: format!(
                "{}/me/drive/items/{}/content",
                self.graph.api,
                percento(&voce.id)
            ),
            graph: self.graph.clone(),
            flusso: self.flusso.clone(),
            bersaglio: Mutex::new(None),
        }))
    }

    /// La cartella che contiene la voce (o la voce stessa, se è una cartella)
    /// sul sito di OneDrive.
    ///
    /// Gli id degli account personali sono `<cid>!<numero>`, e il `cid` è
    /// quel che il sito vuole accanto all'id. Un id di un'altra forma non si
    /// sa comporre senza chiedere `webUrl` a Graph, e allora `None`: meglio
    /// nessun tasto che uno che apre la pagina sbagliata.
    fn indirizzo_web(&self, voce: &VoceRemota) -> Option<String> {
        let id = if voce.cartella {
            voce.id.as_str()
        } else {
            voce.genitore.as_deref().unwrap_or(&voce.id)
        };
        let (cid, _) = id.split_once('!')?;
        if cid.is_empty() {
            return None;
        }
        Some(format!(
            "https://onedrive.live.com/?cid={}&id={}",
            percento(&cid.to_ascii_lowercase()),
            percento(id)
        ))
    }
}

/// Dove stanno i byte di un brano, una volta chiesto a Graph.
#[derive(Clone)]
enum Bersaglio {
    /// L'indirizzo firmato del `302`: si legge senza token.
    Firmato(String),
    /// Graph ha risposto sul posto: si legge `/content` col token.
    Graph,
}

/// Scritto a mano: l'indirizzo firmato è un permesso, e non va in un diario.
impl std::fmt::Debug for Bersaglio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Firmato(_) => f.write_str("Firmato(…)"),
            Self::Graph => f.write_str("Graph"),
        }
    }
}

/// I byte di un file di OneDrive, come [`Sorgente`].
///
/// # Le due scadenze, e una volta ciascuna
///
/// - L'**indirizzo firmato** vale per un po' (un'ora, di solito) e poi
///   risponde `401`, `403`, `404` o `410`. Allora lo si chiede di nuovo a Graph,
///   **una** volta, e si rifà la richiesta. Fino alla 3.1.0 un flusso non
///   sapeva farlo: il guasto diventava un errore di rete, il motore si
///   fermava, e un brano lungo non arrivava in fondo.
/// - Il **token** di Graph scade ogni ora: un `401` di Graph lo rinfresca,
///   una volta, attraverso [`Accesso`] — che lo fa una volta sola anche se a
///   chiederlo sono otto fili insieme.
///
/// L'indirizzo firmato non esce mai da qui: gli errori nominano
/// `/items/{id}/content`, e il [`Pezzo`] non lo riporta.
#[derive(Debug)]
pub struct DaOneDrive {
    graph: Graph,
    flusso: Rete,
    /// `…/items/{id}/content`.
    contenuto: String,
    /// Dove si è letto l'ultima volta.
    bersaglio: Mutex<Option<Bersaglio>>,
}

impl DaOneDrive {
    /// Chiede a Graph dove stanno i byte, e se lo ricorda.
    fn risolvi(&self) -> Result<Bersaglio, AppError> {
        let bersaglio = match self.graph.rimando(&self.contenuto)? {
            Some(firmato) => Bersaglio::Firmato(firmato),
            None => Bersaglio::Graph,
        };
        *self
            .bersaglio
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(bersaglio.clone());
        Ok(bersaglio)
    }

    fn leggi(
        &self,
        bersaglio: &Bersaglio,
        da: u64,
        quanti: u64,
        avanti: &dyn Fn() -> bool,
    ) -> Result<Pezzo, AppError> {
        match bersaglio {
            Bersaglio::Firmato(url) => self.flusso.intervallo_con(url, da, quanti, avanti, &[]),
            Bersaglio::Graph => {
                let permesso = self.graph.accesso.permesso()?;
                let esito = self.flusso.intervallo_con(
                    &self.contenuto,
                    da,
                    quanti,
                    avanti,
                    &[("authorization", &permesso.intestazione())],
                );
                match esito {
                    Err(err) if stato_http(&err) == Some(401) => {
                        let nuovo = self.graph.accesso.rifiutato(&permesso)?;
                        self.flusso.intervallo_con(
                            &self.contenuto,
                            da,
                            quanti,
                            avanti,
                            &[("authorization", &nuovo.intestazione())],
                        )
                    }
                    altro => altro,
                }
            }
        }
    }

    /// Un pezzo, con al più una seconda richiesta a Graph se l'indirizzo
    /// firmato non vale più. `firmati` raccoglie gli indirizzi usati, perché
    /// [`Self::senza_firma`] li tolga da un eventuale errore.
    fn tenta(
        &self,
        da: u64,
        quanti: u64,
        avanti: &dyn Fn() -> bool,
        firmati: &mut Vec<String>,
    ) -> Result<Pezzo, AppError> {
        let ricordato = self
            .bersaglio
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let bersaglio = match ricordato {
            Some(bersaglio) => bersaglio,
            None => self.risolvi()?,
        };
        if let Bersaglio::Firmato(url) = &bersaglio {
            firmati.push(url.clone());
        }
        match self.leggi(&bersaglio, da, quanti, avanti) {
            Err(err) if matches!(bersaglio, Bersaglio::Firmato(_)) && firma_scaduta(&err) => {
                let nuovo = self.risolvi()?;
                if let Bersaglio::Firmato(url) = &nuovo {
                    firmati.push(url.clone());
                }
                self.leggi(&nuovo, da, quanti, avanti)
            }
            altro => altro,
        }
    }

    /// Un errore che nomina `/content` invece dell'indirizzo firmato.
    fn senza_firma(&self, err: AppError, firmati: &[String]) -> AppError {
        let codice = match err.code() {
            ErrorCode::NetHttp {
                status,
                url: Some(_),
            } => ErrorCode::NetHttp {
                status: *status,
                url: Some(self.contenuto.clone()),
            },
            ErrorCode::NetTimeout {
                url: Some(_),
                timeout_ms,
            } => ErrorCode::NetTimeout {
                url: Some(self.contenuto.clone()),
                timeout_ms: *timeout_ms,
            },
            ErrorCode::NetOffline { url: Some(_) } => ErrorCode::NetOffline {
                url: Some(self.contenuto.clone()),
            },
            altro => altro.clone(),
        };
        let mut pulito = AppError::new(codice);
        if let Some(causa) = err.cause() {
            let mut causa = causa.to_owned();
            for firmato in firmati {
                causa = causa.replace(firmato.as_str(), &self.contenuto);
            }
            pulito = pulito.with_cause(causa);
        }
        if let Some(messaggio) = err.message() {
            pulito = pulito.with_message(messaggio.to_owned());
        }
        pulito
    }
}

impl Sorgente for DaOneDrive {
    fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
        self.pezzo_finche(da, quanti, &|| true)
    }

    fn pezzo_finche(
        &self,
        da: u64,
        quanti: u64,
        avanti: &dyn Fn() -> bool,
    ) -> Result<Pezzo, AppError> {
        let mut firmati = Vec::new();
        match self.tenta(da, quanti, avanti, &mut firmati) {
            // L'indirizzo a cui si è arrivati resta qui: è firmato, e a chi
            // legge non serve.
            Ok(pezzo) => Ok(Pezzo {
                url_finale: None,
                ..pezzo
            }),
            Err(err) => Err(self.senza_firma(err, &firmati)),
        }
    }
}

/// Lo stato di un `net.http`, se lo è.
fn stato_http(err: &AppError) -> Option<u16> {
    match err.code() {
        ErrorCode::NetHttp { status, .. } => Some(*status),
        _ => None,
    }
}

/// Un indirizzo firmato che non vale più: si chiede di nuovo a Graph.
fn firma_scaduta(err: &AppError) -> bool {
    matches!(stato_http(err), Some(401 | 403 | 404 | 410))
}

/// Gli stati di `/content` detti con i codici del cloud.
fn traduci(err: AppError) -> AppError {
    let codice = match stato_http(&err) {
        Some(401) => ErrorCode::CloudAuthExpired {
            servizio: Servizio::OneDrive.nome().to_owned(),
        },
        Some(404) => ErrorCode::CloudNotFound,
        _ => return err,
    };
    let mut tradotto = AppError::new(codice);
    if let Some(causa) = err.cause() {
        tradotto = tradotto.with_cause(causa.to_owned());
    }
    tradotto
}

/// Una risposta di Graph che non ha la forma attesa.
fn forma(dettaglio: &str) -> AppError {
    AppError::new(ErrorCode::NetBadSchema {
        service: Some(CHIAVE.to_owned()),
        detail: Some(dettaglio.to_owned()),
    })
}

/// Le voci di una pagina.
///
/// Una pagina senza `value` è un guasto, non una pagina vuota: un elenco che
/// la prendesse per vuota arriverebbe in fondo «completo», e lo specchio
/// toglierebbe quel che quella pagina non ha detto.
fn valori(corpo: &Value) -> Result<&Vec<Value>, AppError> {
    corpo
        .get("value")
        .and_then(Value::as_array)
        .ok_or_else(|| forma("una pagina senza «value»"))
}

/// La pagina dopo, se c'è.
fn dopo(corpo: &Value) -> Option<String> {
    corpo
        .get("@odata.nextLink")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

/// Una voce di Graph, come la vuole il dominio.
///
/// La cima del drive (la faccetta `root`) ha nome vuoto e nessun genitore:
/// vedi le convenzioni in `super`. La versione è il `cTag`, che cambia solo
/// col contenuto; l'`eTag` cambia anche con un rinomina, e si usa solo quando
/// il `cTag` manca (le cartelle).
fn voce(item: &Value) -> Option<VoceRemota> {
    let id = item.get("id").and_then(Value::as_str)?.to_owned();
    let cima = item.get("root").is_some();
    let testo = |campo: &str| item.get(campo).and_then(Value::as_str);
    let cartella = cima || item.get("folder").is_some();
    Some(VoceRemota {
        id,
        genitore: if cima {
            None
        } else {
            item.pointer("/parentReference/id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        },
        nome: if cima {
            String::new()
        } else {
            testo("name").unwrap_or_default().to_owned()
        },
        cartella,
        dimensione: if cartella {
            0
        } else {
            item.get("size").and_then(Value::as_u64).unwrap_or(0)
        },
        modificato_ms: testo("lastModifiedDateTime")
            .and_then(istante_ms)
            .unwrap_or(0),
        versione: testo("cTag")
            .or_else(|| testo("eTag"))
            .map(ToOwned::to_owned),
        tag: if cartella {
            None
        } else {
            item.get("audio").and_then(tag_da)
        },
    })
}

/// La faccetta `audio`, come [`TagNoti`]. `None` se non dice niente.
fn tag_da(audio: &Value) -> Option<TagNoti> {
    let testo = |campo: &str| {
        audio
            .get(campo)
            .and_then(Value::as_str)
            .filter(|t| !t.trim().is_empty())
            .map(ToOwned::to_owned)
    };
    let numero = |campo: &str| audio.get(campo).and_then(Value::as_u64).filter(|n| *n > 0);
    let tag = TagNoti {
        titolo: testo("title"),
        artista: testo("artist"),
        album: testo("album"),
        artista_album: testo("albumArtist"),
        genere: testo("genre"),
        anno: numero("year").and_then(|n| i32::try_from(n).ok()),
        traccia: numero("track").and_then(|n| u32::try_from(n).ok()),
        disco: numero("disc").and_then(|n| u32::try_from(n).ok()),
        // Graph la dà già in millisecondi.
        durata_ms: numero("duration"),
    };
    (tag != TagNoti::default()).then_some(tag)
}

#[cfg(test)]
mod prove;
