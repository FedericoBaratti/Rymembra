//! Google Drive per la musica: «Il mio Drive» e le cartelle condivise con
//! l'account, in sola lettura.
//!
//! # Niente elenco ricorsivo
//!
//! Drive non sa dire «tutto quel che sta sotto questa cartella»: le cartelle
//! sono genitori, non prefissi, e una domanda chiede i figli di un genitore.
//! L'elenco qui chiede i figli di **molte** cartelle insieme
//! (`'a' in parents or 'b' in parents …`, a gruppi di [`GRUPPO`]), livello per
//! livello: una libreria di duemila cartelle costa qualche decina di domande e
//! non duemila.
//!
//! # Le cartelle condivise
//!
//! Una cartella che altri condividono con l'account ha la madre nel drive di
//! chi la condivide, e Drive non dice le madri che l'account non vede: la
//! cartella arriva senza `parents`, come la cima di «Il mio Drive». Lo specchio
//! però vuole una cima sola per conto — due cime avrebbero lo stesso percorso,
//! `nuvola://gdrive-…` —, quindi qui la cima del drive resta l'unica voce senza
//! madre, col nome vuoto, e ogni altra voce senza una madre visibile va sotto
//! la cartella virtuale `~condivisi` («Shared with me», figlia della cima; vedi
//! [`rymembra_domain::nuvola::e_virtuale`]). I percorsi di «Il mio Drive»
//! restano quelli di prima.
//!
//! Vale nell'elenco e nei cambiamenti: lo specchio tiene quel che arriva dai
//! cambiamenti anche fuori dalle radici scelte, e una cartella condivisa che
//! cambia — o che è appena stata condivisa — non deve diventare una seconda
//! cima.
//!
//! Le scorciatoie («Aggiungi scorciatoia a Drive») il selettore le apre come la
//! cartella a cui portano. L'elenco di una cartella scelta invece non le segue:
//! nello specchio ogni voce ha una madre sola, e i figli del bersaglio hanno
//! per madre il bersaglio, non la scorciatoia.
//!
//! I Drive condivisi di un'organizzazione restano fuori, ed è scritto fra i
//! limiti noti.
//!
//! # Il client
//!
//! Un'app Google dell'utente — di serie la stessa del backup — con un altro
//! consenso: vedi [`crate::oauth::FORNITORE_GOOGLE_MUSICA`].

use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::ControlFlow;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use rymembra_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use rymembra_domain::nuvola::{Cambio, Servizio, VoceRemota, e_virtuale, interessa};
use rymembra_domain::tempo::istante_ms;
use rymembra_net::flusso::Sorgente;
use rymembra_net::http::{
    Cancello, Corpo, Metodo, Pezzo, Rete, Richiesta, Rimandi, Risposta, percento,
};
use serde_json::Value;

use super::{Accesso, Deposito, Identita, Permesso, errore_cursore_scaduto};

/// Le API di Drive.
pub const API: &str = "https://www.googleapis.com/drive/v3";

const CHIAVE: &str = "gdrive";

/// Il tipo delle cartelle.
const CARTELLA: &str = "application/vnd.google-apps.folder";

/// Il tipo delle scorciatoie.
const SCORCIATOIA: &str = "application/vnd.google-apps.shortcut";

/// La cartella virtuale delle cartelle condivise con l'account: vedi il `//!`.
const CONDIVISI: &str = "~condivisi";

/// Il nome della cartella virtuale. È un segmento di ogni percorso che sta lì
/// sotto, e i percorsi viaggiano con la sincronia: non si traduce, e non cambia
/// più.
const NOME_CONDIVISI: &str = "Shared with me";

/// Quante cartelle per domanda nella visita dell'elenco.
///
/// Trenta: ogni identificativo è una trentina di caratteri, e la domanda
/// codificata resta lontana dagli otto kilobyte oltre i quali un indirizzo
/// comincia a non passare da tutti i proxy.
const GRUPPO: usize = 30;

/// Quante voci per pagina: il massimo di Drive.
const PER_PAGINA: u32 = 1_000;

/// I campi di una voce.
const CAMPI: &str = "id,name,parents,mimeType,size,modifiedTime,md5Checksum,trashed";

/// I campi di una voce del selettore: anche dove porta una scorciatoia.
const CAMPI_SELETTORE: &str = "id,name,parents,mimeType,shortcutDetails(targetId,targetMimeType)";

const SCADENZA_DOMANDE: Duration = Duration::from_secs(30);
const TENTATIVI: u32 = 3;

/// Quanto si risale al più per trovare la cima.
const PROFONDITA_MASSIMA: usize = 64;

/// Le domande a Drive, condivise fra il deposito e le sue sorgenti.
#[derive(Debug, Clone)]
struct Api {
    domande: Rete,
    api: String,
    accesso: Arc<Accesso>,
}

impl Api {
    fn json(&self, url: &str) -> Result<Value, AppError> {
        self.domande.ritenta(TENTATIVI, || {
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
        })
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

    /// Un errore di Drive, detto con un codice di Rymembra. Il `403` di Drive
    /// è due cose diverse: un limite di frequenza (si rallenta) o un divieto.
    fn errore(&self, risposta: &Risposta, url: &str) -> AppError {
        let corpo: Value = serde_json::from_slice(&risposta.corpo).unwrap_or_default();
        let ragione = corpo
            .pointer("/error/errors/0/reason")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let messaggio = corpo
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .chars()
            .take(300)
            .collect::<String>();
        let causa = format!("{ragione}: {messaggio}");
        match (risposta.stato, ragione) {
            (403, "userRateLimitExceeded" | "rateLimitExceeded") => {
                AppError::new(ErrorCode::NetRateLimited {
                    service: Some(CHIAVE.to_owned()),
                    retry_after_ms: risposta.riprova_fra_ms,
                })
                .with_cause(causa)
            }
            (401, _) => AppError::new(ErrorCode::CloudAuthExpired {
                servizio: Servizio::GoogleDrive.nome().to_owned(),
            })
            .with_cause(causa),
            (404, _) => AppError::new(ErrorCode::CloudNotFound).with_cause(causa),
            _ => self.domande.stato_a_errore(risposta, url),
        }
    }
}

/// Google Drive. Vedi il `//!`.
#[derive(Debug)]
pub struct GoogleDrive {
    api: Api,
    flusso: Rete,
    /// La cima di «Il mio Drive», col nome vuoto: si chiede la prima volta
    /// che serve, e non cambia finché il conto è questo.
    cima: OnceLock<VoceRemota>,
    /// Le cartelle di cui si sa se l'account le può leggere: `false` per
    /// quelle a cui Drive ha risposto `404`. Le riempiono la salita degli
    /// antenati e i cambiamenti; servono ai cambiamenti per mettere una
    /// cartella condivisa dove l'elenco l'aveva messa. Vedi
    /// [`GoogleDrive::madre_leggibile`].
    madri: Mutex<HashMap<String, bool>>,
}

impl GoogleDrive {
    /// Drive con questo permesso.
    #[must_use]
    pub fn nuovo(accesso: Arc<Accesso>) -> Self {
        Self {
            api: Api {
                domande: Rete::nuova(CHIAVE, SCADENZA_DOMANDE).con_rimandi(Rimandi::Nessuno),
                api: API.to_owned(),
                accesso,
            },
            // Drive può rimandare i byte a `googleusercontent.com`: il permesso
            // non segue il secondo salto (vedi `Rete::prendi`).
            flusso: Rete::per_flusso(CHIAVE).con_rimandi(Rimandi::Sorvegliati {
                quanti: 2,
                cancello: Cancello::nuovo(|bersaglio| bersaglio.starts_with("https://")),
            }),
            cima: OnceLock::new(),
            madri: Mutex::new(HashMap::new()),
        }
    }

    #[cfg(test)]
    fn per_prova(rete: Rete, base: &str, accesso: Arc<Accesso>) -> Self {
        Self {
            api: Api {
                domande: rete.clone(),
                api: format!("{base}/drive/v3"),
                accesso,
            },
            flusso: rete,
            cima: OnceLock::new(),
            madri: Mutex::new(HashMap::new()),
        }
    }

    /// Una domanda `files.list`, con tutte le sue pagine.
    fn elenco(
        &self,
        domanda: &str,
        campi: &str,
        pagina: &mut dyn FnMut(&[Value]) -> ControlFlow<()>,
    ) -> Result<bool, AppError> {
        let mut segnalibro: Option<String> = None;
        loop {
            let mut url = format!(
                "{}/files?q={}&fields=nextPageToken,files({campi})&pageSize={PER_PAGINA}&spaces=drive&corpora=user",
                self.api.api,
                percento(domanda)
            );
            if let Some(segno) = &segnalibro {
                url.push_str("&pageToken=");
                url.push_str(&percento(segno));
            }
            let corpo = self.api.json(&url)?;
            let file = corpo
                .get("files")
                .and_then(Value::as_array)
                .ok_or_else(|| forma("una pagina senza «files»"))?;
            if pagina(file).is_break() {
                return Ok(false);
            }
            match corpo.get("nextPageToken").and_then(Value::as_str) {
                Some(prossimo) => segnalibro = Some(prossimo.to_owned()),
                None => return Ok(true),
            }
        }
    }

    /// Una voce per identificativo.
    fn voce_per_id(&self, id: &str) -> Result<Value, AppError> {
        self.api.json(&format!(
            "{}/files/{}?fields={CAMPI}",
            self.api.api,
            percento(id)
        ))
    }

    /// La cima di «Il mio Drive», col nome vuoto: nel percorso non entra.
    fn cima_del_drive(&self) -> Result<&VoceRemota, AppError> {
        if let Some(cima) = self.cima.get() {
            return Ok(cima);
        }
        let item = self.voce_per_id("root")?;
        let mut cima = voce(&item, None).ok_or_else(|| forma("la cima senza identificativo"))?;
        cima.genitore = None;
        cima.nome.clear();
        Ok(self.cima.get_or_init(|| cima))
    }

    /// Mette al suo posto una voce senza madre visibile: la cima del drive
    /// resta senza madre e col nome vuoto, ogni altra va sotto la cartella
    /// virtuale. Vedi il `//!`.
    fn al_suo_posto(&self, voce: &mut VoceRemota) -> Result<(), AppError> {
        if voce.genitore.is_some() {
            return Ok(());
        }
        if voce.id == self.cima_del_drive()?.id {
            voce.nome.clear();
        } else {
            voce.genitore = Some(CONDIVISI.to_owned());
        }
        Ok(())
    }

    /// Si sa, o si chiede, se l'account può leggere la cartella `id`.
    ///
    /// # Perché i cambiamenti lo chiedono
    ///
    /// La salita degli antenati si ferma a una madre che risponde `404`, e la
    /// cartella sotto finisce nella cartella virtuale (vedi
    /// [`GoogleDrive::antenati`]). Ma quando quella cartella cambia — il
    /// proprietario la rinomina, la ricondivide — Drive la consegna con la sua
    /// madre vera, `parents: [P]`, che lo specchio non conosce: senza questa
    /// domanda la cartella perdeva il percorso, con tutto quel che le sta
    /// sotto. Una domanda per madre, la prima volta che serve.
    ///
    /// # Errori
    ///
    /// Quelli della rete, tranne il `404`, che è la risposta «no».
    fn madre_leggibile(&self, id: &str) -> Result<bool, AppError> {
        if let Some(sa) = self
            .madri
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
        {
            return Ok(*sa);
        }
        let leggibile = match self.voce_per_id(id) {
            Ok(_) => true,
            Err(err) if err.code().kind() == ErrorCodeKind::CloudNotFound => false,
            Err(err) => return Err(err),
        };
        self.segna_madre(id, leggibile);
        Ok(leggibile)
    }

    fn segna_madre(&self, id: &str, leggibile: bool) {
        self.madri
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id.to_owned(), leggibile);
    }

    /// `cima` e i suoi antenati, dalla cima del drive in giù.
    ///
    /// La salita si ferma dove la madre non si vede — Drive risponde `404` per
    /// una voce che l'account non può leggere —: è il caso di una cartella
    /// condivisa con l'account, e allora la catena passa dalla cartella
    /// virtuale. Un `404` sulla cartella scelta invece è un errore: non c'è più.
    fn antenati(&self, cima: &str) -> Result<Vec<VoceRemota>, AppError> {
        // La cartella virtuale non ha un elenco suo, e il suo nome non si
        // manda a Google.
        if e_virtuale(cima) {
            return Err(AppError::new(ErrorCode::CloudNotFound)
                .with_cause("una cartella virtuale non si elenca"));
        }
        let mut catena: Vec<VoceRemota> = Vec::new();
        let mut prossimo = Some(cima.to_owned());
        for _ in 0..PROFONDITA_MASSIMA {
            let Some(id) = prossimo.take() else {
                break;
            };
            let item = match self.voce_per_id(&id) {
                Ok(item) => item,
                Err(err) if err.code().kind() == ErrorCodeKind::CloudNotFound => {
                    let Some(sotto) = catena.last_mut() else {
                        return Err(err);
                    };
                    sotto.genitore = None;
                    self.segna_madre(&id, false);
                    break;
                }
                Err(err) => return Err(err),
            };
            self.segna_madre(&id, true);
            let voce = voce(&item, None).ok_or_else(|| forma("una voce senza identificativo"))?;
            prossimo = voce.genitore.clone();
            catena.push(voce);
        }
        let condivisa = match catena.last_mut() {
            Some(in_testa) if in_testa.genitore.is_none() => {
                self.al_suo_posto(in_testa)?;
                in_testa.genitore.is_some()
            }
            _ => false,
        };
        if condivisa {
            let cima_del_drive = self.cima_del_drive()?.clone();
            catena.push(condivisi(&cima_del_drive.id));
            catena.push(cima_del_drive);
        }
        catena.reverse();
        Ok(catena)
    }
}

impl GoogleDrive {
    /// Una cartella dei cambiamenti la cui madre l'account non legge va sotto
    /// la cartella virtuale, come l'aveva messa l'elenco. Vedi
    /// [`GoogleDrive::madre_leggibile`].
    ///
    /// Solo le cartelle: un file sta dentro una cartella che si legge — la
    /// condivisa stessa, o una sotto di lei.
    fn sotto_i_condivisi_se_orfana(&self, voce: &mut VoceRemota) -> Result<(), AppError> {
        if !voce.cartella {
            return Ok(());
        }
        let Some(madre) = voce.genitore.as_deref() else {
            return Ok(());
        };
        if madre == CONDIVISI || madre == self.cima_del_drive()?.id {
            return Ok(());
        }
        if !self.madre_leggibile(madre)? {
            voce.genitore = Some(CONDIVISI.to_owned());
        }
        Ok(())
    }
}

/// La cartella virtuale delle cartelle condivise, figlia di `cima`.
fn condivisi(cima: &str) -> VoceRemota {
    VoceRemota {
        id: CONDIVISI.to_owned(),
        genitore: Some(cima.to_owned()),
        nome: NOME_CONDIVISI.to_owned(),
        cartella: true,
        dimensione: 0,
        modificato_ms: 0,
        versione: None,
        tag: None,
    }
}

/// Una voce del selettore: una cartella, o una scorciatoia a una cartella. La
/// scorciatoia si apre e si sceglie come la cartella a cui porta, col nome che
/// ha la scorciatoia; quelle a un file non sono cartelle.
fn cartella_del_selettore(item: &Value) -> Option<VoceRemota> {
    if item.get("mimeType").and_then(Value::as_str) != Some(SCORCIATOIA) {
        return voce(item, None).filter(|v| v.cartella);
    }
    let bersaglio = item.get("shortcutDetails")?;
    if bersaglio.get("targetMimeType").and_then(Value::as_str) != Some(CARTELLA) {
        return None;
    }
    Some(VoceRemota {
        id: bersaglio
            .get("targetId")
            .and_then(Value::as_str)?
            .to_owned(),
        genitore: item
            .pointer("/parents/0")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        nome: item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        cartella: true,
        dimensione: 0,
        modificato_ms: 0,
        versione: None,
        tag: None,
    })
}

/// Una voce di Drive. `sotto`, se c'è, è l'insieme delle cartelle visitate:
/// di un file con più genitori si tiene quello che sta dentro la visita.
fn voce(item: &Value, sotto: Option<&HashSet<String>>) -> Option<VoceRemota> {
    let id = item.get("id").and_then(Value::as_str)?;
    let tipo = item
        .get("mimeType")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let cartella = tipo == CARTELLA;
    // I documenti di Google (e le scorciatoie) non sono file da suonare.
    if !cartella && tipo.starts_with("application/vnd.google-apps.") {
        return None;
    }
    let genitori: Vec<&str> = item
        .get("parents")
        .and_then(Value::as_array)
        .map(|g| g.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let genitore = match sotto {
        Some(visitate) => genitori
            .iter()
            .find(|g| visitate.contains(**g))
            .or_else(|| genitori.first())
            .map(|g| (*g).to_owned()),
        None => genitori.first().map(|g| (*g).to_owned()),
    };
    let testo = |campo: &str| item.get(campo).and_then(Value::as_str);
    Some(VoceRemota {
        id: id.to_owned(),
        genitore,
        nome: testo("name").unwrap_or_default().to_owned(),
        cartella,
        dimensione: testo("size").and_then(|s| s.parse().ok()).unwrap_or(0),
        modificato_ms: testo("modifiedTime").and_then(istante_ms).unwrap_or(0),
        versione: testo("md5Checksum").map(ToOwned::to_owned),
        tag: None,
    })
}

impl Deposito for GoogleDrive {
    fn servizio(&self) -> Servizio {
        Servizio::GoogleDrive
    }

    fn spegni(&self) {
        self.api.accesso.spegni();
    }

    fn chi(&self) -> Result<Identita, AppError> {
        let corpo = self.api.json(&format!(
            "{}/about?fields=user(permissionId,emailAddress,displayName)",
            self.api.api
        ))?;
        let id_stabile = corpo
            .pointer("/user/permissionId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| forma("l'utente non ha un identificativo"))?
            .to_owned();
        let nome = corpo
            .pointer("/user/emailAddress")
            .or_else(|| corpo.pointer("/user/displayName"))
            .and_then(Value::as_str)
            .filter(|n| !n.trim().is_empty())
            .unwrap_or("Google Drive")
            .to_owned();
        Ok(Identita { id_stabile, nome })
    }

    /// In cima, accanto alle cartelle di «Il mio Drive», c'è la cartella
    /// virtuale; dentro di lei, le cartelle condivise con l'account.
    fn cartelle(&self, dentro: Option<&str>) -> Result<Vec<VoceRemota>, AppError> {
        let domanda = match dentro {
            Some(CONDIVISI) => {
                format!("sharedWithMe = true and mimeType = '{CARTELLA}' and trashed = false")
            }
            _ => format!(
                "'{}' in parents and (mimeType = '{CARTELLA}' or mimeType = '{SCORCIATOIA}') and trashed = false",
                virgolette(dentro.unwrap_or("root"))
            ),
        };
        let mut cartelle = Vec::new();
        if dentro.is_none() {
            // La madre per soprannome: del selettore contano l'id e il nome.
            cartelle.push(condivisi("root"));
        }
        self.elenco(&domanda, CAMPI_SELETTORE, &mut |file| {
            cartelle.extend(file.iter().filter_map(cartella_del_selettore));
            ControlFlow::Continue(())
        })?;
        // Una scorciatoia può portare a una cartella che è già lì accanto, o
        // due a una stessa: nel selettore una voce per cartella, la prima.
        let mut viste = HashSet::new();
        cartelle.retain(|v| viste.insert(v.id.clone()));
        Ok(cartelle)
    }

    fn cursore_adesso(&self) -> Result<String, AppError> {
        let corpo = self
            .api
            .json(&format!("{}/changes/startPageToken", self.api.api))?;
        corpo
            .get("startPageToken")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| forma("niente startPageToken"))
    }

    fn elenca(
        &self,
        cima: &str,
        pagina: &mut dyn FnMut(Vec<VoceRemota>) -> ControlFlow<()>,
    ) -> Result<bool, AppError> {
        let mut in_testa = self.antenati(cima)?;
        // `root` è un soprannome: la visita parte dall'identificativo vero.
        let cima_vera = in_testa
            .last()
            .map_or_else(|| cima.to_owned(), |v| v.id.clone());
        let mut visitate: HashSet<String> = HashSet::from([cima_vera.clone()]);
        let mut livello: VecDeque<String> = VecDeque::from([cima_vera]);
        while !livello.is_empty() {
            let mut prossimo = VecDeque::new();
            while !livello.is_empty() {
                let gruppo: Vec<String> = livello.drain(..GRUPPO.min(livello.len())).collect();
                let genitori = gruppo
                    .iter()
                    .map(|g| format!("'{}' in parents", virgolette(g)))
                    .collect::<Vec<_>>()
                    .join(" or ");
                let domanda = format!("({genitori}) and trashed = false");
                let mut fermato = false;
                let finito = self.elenco(&domanda, CAMPI, &mut |file| {
                    let mut voci = std::mem::take(&mut in_testa);
                    for item in file {
                        let Some(v) = voce(item, Some(&visitate)) else {
                            continue;
                        };
                        if v.cartella {
                            if !visitate.contains(&v.id) {
                                prossimo.push_back(v.id.clone());
                            }
                            voci.push(v);
                        } else if interessa(&v.nome) {
                            voci.push(v);
                        }
                    }
                    if pagina(voci).is_break() {
                        fermato = true;
                        return ControlFlow::Break(());
                    }
                    ControlFlow::Continue(())
                });
                finito?;
                if fermato {
                    return Ok(false);
                }
            }
            for id in &prossimo {
                visitate.insert(id.clone());
            }
            livello = prossimo;
        }
        // Una cartella vuota consegna comunque la sua catena.
        if !in_testa.is_empty() && pagina(in_testa).is_break() {
            return Ok(false);
        }
        Ok(true)
    }

    /// Drive non sa aspettare: si risponde subito, il ritmo lo tiene chi
    /// chiama.
    fn cambiamenti(
        &self,
        cursore: &str,
        attesa: Duration,
    ) -> Result<(Vec<Cambio>, String), AppError> {
        let _ = attesa;
        let mut cambi = Vec::new();
        let mut segno = cursore.to_owned();
        loop {
            let url = format!(
                "{}/changes?pageToken={}&pageSize={PER_PAGINA}&spaces=drive&includeRemoved=true&fields=nextPageToken,newStartPageToken,changes(fileId,removed,file({CAMPI}))",
                self.api.api,
                percento(&segno)
            );
            let corpo = match self.api.json(&url) {
                Ok(corpo) => corpo,
                // Un segno che Drive non conosce più: si rielenca.
                Err(err)
                    if err.code().kind()
                        == rymembra_domain::errors::ErrorCodeKind::CloudNotFound =>
                {
                    return Err(errore_cursore_scaduto(
                        "Drive non riconosce il segno dei cambiamenti",
                    ));
                }
                Err(err) => return Err(err),
            };
            for cambio in corpo
                .get("changes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let id = cambio
                    .get("fileId")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let tolto = cambio
                    .get("removed")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    || cambio
                        .pointer("/file/trashed")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                if tolto {
                    if !id.is_empty() {
                        cambi.push(Cambio::Tolta { id: id.to_owned() });
                    }
                } else if let Some(mut v) = cambio.get("file").and_then(|f| voce(f, None)) {
                    self.al_suo_posto(&mut v)?;
                    // Quel che Drive consegna si legge.
                    if v.cartella {
                        self.segna_madre(&v.id, true);
                    }
                    self.sotto_i_condivisi_se_orfana(&mut v)?;
                    cambi.push(Cambio::Voce(v));
                }
            }
            if let Some(prossimo) = corpo.get("nextPageToken").and_then(Value::as_str) {
                segno = prossimo.to_owned();
                continue;
            }
            let nuovo = corpo
                .get("newStartPageToken")
                .and_then(Value::as_str)
                .ok_or_else(|| forma("né nextPageToken né newStartPageToken"))?;
            return Ok((cambi, nuovo.to_owned()));
        }
    }

    fn sorgente(&self, voce: &VoceRemota) -> Result<Box<dyn Sorgente>, AppError> {
        Ok(Box::new(DaDrive {
            api: self.api.clone(),
            flusso: self.flusso.clone(),
            url: format!("{}/files/{}?alt=media", self.api.api, percento(&voce.id)),
        }))
    }

    fn indirizzo_web(&self, voce: &VoceRemota) -> Option<String> {
        Some(if voce.id == CONDIVISI {
            "https://drive.google.com/drive/shared-with-me".to_owned()
        } else if voce.cartella {
            format!(
                "https://drive.google.com/drive/folders/{}",
                percento(&voce.id)
            )
        } else {
            format!(
                "https://drive.google.com/file/d/{}/view",
                percento(&voce.id)
            )
        })
    }
}

/// I byte di un file di Drive: `alt=media` col token e `Range`. Un `401`
/// rinfresca il token, una volta.
#[derive(Debug)]
pub struct DaDrive {
    api: Api,
    flusso: Rete,
    url: String,
}

impl Sorgente for DaDrive {
    fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
        self.pezzo_finche(da, quanti, &|| true)
    }

    fn pezzo_finche(
        &self,
        da: u64,
        quanti: u64,
        avanti: &dyn Fn() -> bool,
    ) -> Result<Pezzo, AppError> {
        let permesso = self.api.accesso.permesso()?;
        let esito = self.flusso.intervallo_con(
            &self.url,
            da,
            quanti,
            avanti,
            &[("authorization", &permesso.intestazione())],
        );
        let esito = match esito {
            Err(err) if matches!(err.code(), ErrorCode::NetHttp { status: 401, .. }) => {
                let nuovo = self.api.accesso.rifiutato(&permesso)?;
                self.flusso.intervallo_con(
                    &self.url,
                    da,
                    quanti,
                    avanti,
                    &[("authorization", &nuovo.intestazione())],
                )
            }
            altro => altro,
        };
        esito.map(|pezzo| Pezzo {
            url_finale: None,
            ..pezzo
        })
    }
}

/// Un identificativo dentro le virgolette di una domanda di Drive.
fn virgolette(testo: &str) -> String {
    testo.replace('\\', "\\\\").replace('\'', "\\'")
}

/// Una risposta di Drive che non ha la forma attesa.
fn forma(dettaglio: &str) -> AppError {
    AppError::new(ErrorCode::NetBadSchema {
        service: Some(CHIAVE.to_owned()),
        detail: Some(dettaglio.to_owned()),
    })
}

#[cfg(test)]
mod prove;
