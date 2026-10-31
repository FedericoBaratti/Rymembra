//! Dropbox, attraverso le API v2.
//!
//! # L'identificativo è il percorso
//!
//! Dropbox dà a ogni voce un `id:…` che sopravvive agli spostamenti, ma il
//! flusso dei cambiamenti dice le **cancellazioni** solo col percorso
//! (`path_lower`): un `deleted` non porta un id. Con gli `id:` una
//! cancellazione arrivata dopo un riavvio — quando la mappa percorso → id non
//! c'è più in memoria — non si saprebbe a chi attribuirla, e un brano tolto da
//! Dropbox resterebbe in libreria fino al prossimo elenco completo.
//!
//! Quindi qui l'identificativo di una voce è il suo `path_lower`, e la cima
//! del drive è [`CIMA`]. Uno spostamento sul servizio diventa una voce tolta e
//! una nuova, e la scansione lo riconosce come spostamento dal contenuto
//! (`content_key`), con ascolti e voti al loro posto — come per un file
//! spostato sul disco. Dropbox ignora le maiuscole nei percorsi, quindi due
//! voci che differiscono solo per quelle non esistono.
//!
//! # I cambiamenti senza chiedere di continuo
//!
//! Dropbox sa **aspettare**: `list_folder/longpoll` resta aperto finché
//! qualcosa cambia (o finché scade l'attesa), e solo allora si chiede che cosa.
//! È ciò che rende un file copiato su Dropbox visibile in libreria in pochi
//! secondi senza una richiesta ogni pochi secondi.

use std::ops::ControlFlow;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::nuvola::{Cambio, Servizio, VoceRemota, interessa};
use rymembra_domain::tempo::istante_ms;
use rymembra_net::flusso::Sorgente;
use rymembra_net::http::{
    Cancello, Corpo, Metodo, Pezzo, Rete, Richiesta, Rimandi, Risposta, percento,
};
use serde_json::{Value, json};

use super::{Accesso, Deposito, Identita, Permesso, errore_cursore_scaduto};

/// Le API «RPC» di Dropbox.
pub const API: &str = "https://api.dropboxapi.com/2";

/// Il servizio dei cambiamenti in attesa: senza autenticazione, il cursore
/// basta.
pub const NOTIFICHE: &str = "https://notify.dropboxapi.com/2";

/// L'identificativo della cima del drive: Dropbox la chiama `""`.
pub const CIMA: &str = "root";

const CHIAVE: &str = "dropbox";

/// Quanto si aspetta una risposta normale.
const SCADENZA_DOMANDE: Duration = Duration::from_secs(30);

/// Quante volte si ritenta una domanda che è caduta da sé.
const TENTATIVI: u32 = 3;

/// Quante voci per pagina: il massimo che Dropbox concede.
const PER_PAGINA: u32 = 2_000;

/// L'attesa più corta e più lunga del long-poll, in secondi (i limiti di
/// Dropbox sono 30 e 480).
const ATTESA_MINIMA: u64 = 30;
const ATTESA_MASSIMA: u64 = 120;

/// Il margine oltre l'attesa: Dropbox aggiunge fino a novanta secondi a caso
/// all'attesa chiesta, per non svegliare tutti insieme.
const MARGINE_ATTESA: Duration = Duration::from_secs(100);

/// Per quanto si crede a un collegamento temporaneo: tre ore e mezza delle
/// quattro che Dropbox gli dà, così non scade a metà di un brano.
const VALIDITA_LINK: Duration = Duration::from_secs(3 * 3600 + 1800);

/// Le domande a Dropbox, condivise fra il deposito e le sue sorgenti.
#[derive(Debug, Clone)]
struct Api {
    domande: Rete,
    /// `https://api.dropboxapi.com/2`, tranne che nelle prove.
    api: String,
    accesso: Arc<Accesso>,
}

impl Api {
    /// Una chiamata RPC: `POST` con un corpo JSON, risposta JSON. Un `401`
    /// rinfresca il token una volta.
    fn rpc(&self, punto: &str, argomenti: &Value) -> Result<Value, AppError> {
        let url = format!("{}/{punto}", self.api);
        let corpo = serde_json::to_vec(argomenti).unwrap_or_default();
        self.domande.ritenta(TENTATIVI, || {
            let permesso = self.accesso.permesso()?;
            let mut risposta = self.chiedi(&url, &corpo, &permesso)?;
            if risposta.stato == 401 {
                let nuovo = self.accesso.rifiutato(&permesso)?;
                risposta = self.chiedi(&url, &corpo, &nuovo)?;
            }
            if !risposta.e_andata() {
                return Err(self.errore(&risposta, &url));
            }
            serde_json::from_slice(&risposta.corpo)
                .map_err(|err| forma(&format!("la risposta non è JSON: {err}")))
        })
    }

    fn chiedi(&self, url: &str, corpo: &[u8], permesso: &Permesso) -> Result<Risposta, AppError> {
        let autorizzazione = permesso.intestazione();
        self.domande.esegui(Richiesta {
            metodo: Metodo::Post,
            url,
            intestazioni: &[("authorization", &autorizzazione)],
            corpo: Corpo::Byte {
                tipo: "application/json",
                dati: corpo,
            },
        })
    }

    /// Un errore di Dropbox, detto con un codice di Rymembra.
    ///
    /// Gli errori «di percorso» arrivano come `409` con un `error_summary`
    /// del tipo `path/not_found/..`; un cursore che non vale più è `reset`.
    fn errore(&self, risposta: &Risposta, url: &str) -> AppError {
        let corpo: Value = serde_json::from_slice(&risposta.corpo).unwrap_or_default();
        let sommario = corpo
            .get("error_summary")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .chars()
            .take(300)
            .collect::<String>();
        if risposta.stato == 409 && sommario.contains("reset") {
            return errore_cursore_scaduto(&sommario);
        }
        match risposta.stato {
            401 => AppError::new(ErrorCode::CloudAuthExpired {
                servizio: Servizio::Dropbox.nome().to_owned(),
            })
            .with_cause(sommario),
            409 if sommario.contains("not_found") => {
                AppError::new(ErrorCode::CloudNotFound).with_cause(sommario)
            }
            _ => self.domande.stato_a_errore(risposta, url),
        }
    }
}

/// Dropbox. Vedi il `//!`.
#[derive(Debug)]
pub struct Dropbox {
    api: Api,
    /// L'attesa dei cambiamenti: una scadenza che regge il long-poll.
    notifiche: Rete,
    notifiche_base: String,
    /// I pezzi dei brani, dal collegamento temporaneo.
    flusso: Rete,
}

impl Dropbox {
    /// Dropbox con questo permesso.
    #[must_use]
    pub fn nuovo(accesso: Arc<Accesso>) -> Self {
        Self {
            api: Api {
                domande: Rete::nuova(CHIAVE, SCADENZA_DOMANDE).con_rimandi(Rimandi::Nessuno),
                api: API.to_owned(),
                accesso,
            },
            notifiche: Rete::nuova(
                CHIAVE,
                Duration::from_secs(ATTESA_MASSIMA).saturating_add(MARGINE_ATTESA),
            )
            .con_rimandi(Rimandi::Nessuno),
            notifiche_base: NOTIFICHE.to_owned(),
            flusso: Rete::per_flusso(CHIAVE).con_rimandi(Rimandi::Sorvegliati {
                quanti: 2,
                cancello: Cancello::nuovo(|bersaglio| bersaglio.starts_with("https://")),
            }),
        }
    }

    /// Dropbox verso un servitore finto, con una rete sola per tutto.
    #[cfg(test)]
    fn per_prova(rete: Rete, base: &str, accesso: Arc<Accesso>) -> Self {
        Self {
            api: Api {
                domande: rete.clone(),
                api: format!("{base}/2"),
                accesso,
            },
            notifiche: rete.clone(),
            notifiche_base: format!("{base}/notify/2"),
            flusso: rete,
        }
    }

    /// Tutte le pagine di un elenco, dalla prima risposta in poi.
    fn pagine(
        &self,
        prima: Value,
        pagina: &mut dyn FnMut(&Value) -> ControlFlow<()>,
    ) -> Result<(bool, String), AppError> {
        let mut corpo = prima;
        loop {
            if pagina(&corpo).is_break() {
                return Ok((false, String::new()));
            }
            let cursore = corpo
                .get("cursor")
                .and_then(Value::as_str)
                .ok_or_else(|| forma("una pagina senza cursore"))?
                .to_owned();
            if !corpo
                .get("has_more")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return Ok((true, cursore));
            }
            corpo = self
                .api
                .rpc("files/list_folder/continue", &json!({ "cursor": cursore }))?;
        }
    }
}

/// Il percorso Dropbox di un identificativo: la cima è `""`.
fn percorso_di(id: &str) -> &str {
    if id == CIMA { "" } else { id }
}

/// L'identificativo della cartella madre di un `path_lower`.
fn madre_di(path_lower: &str) -> String {
    match path_lower.rsplit_once('/') {
        Some(("", _)) | None => CIMA.to_owned(),
        Some((madre, _)) => madre.to_owned(),
    }
}

/// Una voce di Dropbox, se è un file o una cartella.
fn voce(entry: &Value) -> Option<VoceRemota> {
    let tipo = entry.get(".tag").and_then(Value::as_str)?;
    let cartella = match tipo {
        "folder" => true,
        "file" => false,
        _ => return None,
    };
    let percorso = entry.get("path_lower").and_then(Value::as_str)?;
    let nome = entry.get("name").and_then(Value::as_str)?;
    if percorso.is_empty() {
        return None;
    }
    let testo = |campo: &str| entry.get(campo).and_then(Value::as_str);
    Some(VoceRemota {
        id: percorso.to_owned(),
        genitore: Some(madre_di(percorso)),
        nome: nome.to_owned(),
        cartella,
        dimensione: entry.get("size").and_then(Value::as_u64).unwrap_or(0),
        modificato_ms: testo("server_modified").and_then(istante_ms).unwrap_or(0),
        versione: testo("content_hash")
            .or_else(|| testo("rev"))
            .map(ToOwned::to_owned),
        tag: None,
    })
}

/// La cima del drive, come voce.
fn cima() -> VoceRemota {
    VoceRemota {
        id: CIMA.to_owned(),
        genitore: None,
        nome: String::new(),
        cartella: true,
        dimensione: 0,
        modificato_ms: 0,
        versione: None,
        tag: None,
    }
}

/// La catena dalla cima del drive a una cartella, dai suoi due percorsi.
///
/// `path_lower` e `path_display` hanno gli stessi segmenti, uno in minuscolo e
/// l'altro come l'utente li ha scritti: si leggono insieme, senza una domanda
/// per antenato.
fn catena(path_lower: &str, path_display: &str) -> Vec<VoceRemota> {
    let mut voci = vec![cima()];
    let mut finora = String::new();
    for (minuscolo, mostrato) in path_lower
        .split('/')
        .filter(|s| !s.is_empty())
        .zip(path_display.split('/').filter(|s| !s.is_empty()))
    {
        let madre = if finora.is_empty() {
            CIMA.to_owned()
        } else {
            finora.clone()
        };
        finora = format!("{finora}/{minuscolo}");
        voci.push(VoceRemota {
            id: finora.clone(),
            genitore: Some(madre),
            nome: mostrato.to_owned(),
            cartella: true,
            dimensione: 0,
            modificato_ms: 0,
            versione: None,
            tag: None,
        });
    }
    voci
}

impl Deposito for Dropbox {
    fn servizio(&self) -> Servizio {
        Servizio::Dropbox
    }

    fn spegni(&self) {
        self.api.accesso.spegni();
    }

    fn chi(&self) -> Result<Identita, AppError> {
        let conto = self.api.rpc("users/get_current_account", &Value::Null)?;
        let id_stabile = conto
            .get("account_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| forma("l'account non ha un identificativo"))?
            .to_owned();
        let nome = conto
            .get("email")
            .and_then(Value::as_str)
            .or_else(|| conto.pointer("/name/display_name").and_then(Value::as_str))
            .filter(|n| !n.trim().is_empty())
            .unwrap_or("Dropbox")
            .to_owned();
        Ok(Identita { id_stabile, nome })
    }

    fn cartelle(&self, dentro: Option<&str>) -> Result<Vec<VoceRemota>, AppError> {
        let percorso = percorso_di(dentro.unwrap_or(CIMA));
        let prima = self.api.rpc(
            "files/list_folder",
            &json!({ "path": percorso, "recursive": false, "limit": PER_PAGINA }),
        )?;
        let mut cartelle = Vec::new();
        self.pagine(prima, &mut |corpo| {
            if let Some(entries) = corpo.get("entries").and_then(Value::as_array) {
                cartelle.extend(entries.iter().filter_map(voce).filter(|v| v.cartella));
            }
            ControlFlow::Continue(())
        })?;
        Ok(cartelle)
    }

    fn cursore_adesso(&self) -> Result<String, AppError> {
        let corpo = self.api.rpc(
            "files/list_folder/get_latest_cursor",
            &json!({ "path": "", "recursive": true }),
        )?;
        corpo
            .get("cursor")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| forma("niente cursore"))
    }

    fn elenca(
        &self,
        cima_scelta: &str,
        pagina: &mut dyn FnMut(Vec<VoceRemota>) -> ControlFlow<()>,
    ) -> Result<bool, AppError> {
        let percorso = percorso_di(cima_scelta);
        // La catena dalla cima del drive alla cartella scelta va nella prima
        // pagina: lo specchio compone i percorsi risalendo i genitori.
        let mut in_testa = if percorso.is_empty() {
            vec![cima()]
        } else {
            let meta = self
                .api
                .rpc("files/get_metadata", &json!({ "path": percorso }))?;
            let minuscolo = meta
                .get("path_lower")
                .and_then(Value::as_str)
                .unwrap_or(percorso);
            let mostrato = meta
                .get("path_display")
                .and_then(Value::as_str)
                .unwrap_or(minuscolo);
            catena(minuscolo, mostrato)
        };
        let prima = self.api.rpc(
            "files/list_folder",
            &json!({ "path": percorso, "recursive": true, "limit": PER_PAGINA }),
        )?;
        let (completo, _) = self.pagine(prima, &mut |corpo| {
            let mut voci = std::mem::take(&mut in_testa);
            if let Some(entries) = corpo.get("entries").and_then(Value::as_array) {
                voci.extend(
                    entries
                        .iter()
                        .filter_map(voce)
                        // La cartella scelta è già nella catena.
                        .filter(|v| v.id != percorso)
                        .filter(|v| v.cartella || interessa(&v.nome)),
                );
            }
            pagina(voci)
        })?;
        Ok(completo)
    }

    /// Prima si aspetta (`longpoll`, senza token), poi si chiede cosa è
    /// cambiato. Un `backoff` di Dropbox si rispetta qui, sul filo del conto.
    fn cambiamenti(
        &self,
        cursore: &str,
        attesa: Duration,
    ) -> Result<(Vec<Cambio>, String), AppError> {
        let secondi = attesa.as_secs().clamp(ATTESA_MINIMA, ATTESA_MASSIMA);
        let url = format!("{}/files/list_folder/longpoll", self.notifiche_base);
        let argomenti = serde_json::to_vec(&json!({ "cursor": cursore, "timeout": secondi }))
            .unwrap_or_default();
        let risposta = self.notifiche.esegui(Richiesta {
            metodo: Metodo::Post,
            url: &url,
            intestazioni: &[],
            corpo: Corpo::Byte {
                tipo: "application/json",
                dati: &argomenti,
            },
        })?;
        if !risposta.e_andata() {
            return Err(self.api.errore(&risposta, &url));
        }
        let attesa: Value = serde_json::from_slice(&risposta.corpo)
            .map_err(|err| forma(&format!("la risposta non è JSON: {err}")))?;
        if let Some(pausa) = attesa.get("backoff").and_then(Value::as_u64) {
            std::thread::sleep(Duration::from_secs(pausa.min(ATTESA_MASSIMA)));
        }
        if !attesa
            .get("changes")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Ok((Vec::new(), cursore.to_owned()));
        }
        let mut cambi = Vec::new();
        let prima = self
            .api
            .rpc("files/list_folder/continue", &json!({ "cursor": cursore }))?;
        let (_, nuovo) = self.pagine(prima, &mut |corpo| {
            for entry in corpo
                .get("entries")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if entry.get(".tag").and_then(Value::as_str) == Some("deleted") {
                    if let Some(percorso) = entry.get("path_lower").and_then(Value::as_str) {
                        cambi.push(Cambio::Tolta {
                            id: percorso.to_owned(),
                        });
                    }
                } else if let Some(voce) = voce(entry) {
                    cambi.push(Cambio::Voce(voce));
                }
            }
            ControlFlow::Continue(())
        })?;
        Ok((cambi, nuovo))
    }

    fn sorgente(&self, voce: &VoceRemota) -> Result<Box<dyn Sorgente>, AppError> {
        Ok(Box::new(DaDropbox {
            api: self.api.clone(),
            flusso: self.flusso.clone(),
            percorso: voce.id.clone(),
            link: Mutex::new(None),
        }))
    }

    /// La cartella che contiene la voce, sul sito di Dropbox.
    fn indirizzo_web(&self, voce: &VoceRemota) -> Option<String> {
        let cartella = if voce.cartella {
            voce.id.clone()
        } else {
            madre_di(&voce.id)
        };
        let percorso = percorso_di(&cartella);
        let codificato: Vec<String> = percorso
            .split('/')
            .filter(|s| !s.is_empty())
            .map(percento)
            .collect();
        Some(format!(
            "https://www.dropbox.com/home/{}",
            codificato.join("/")
        ))
    }
}

/// Un collegamento temporaneo, e quando lo si è avuto.
#[derive(Clone)]
struct Link {
    url: String,
    da: Instant,
}

/// Scritto a mano: un collegamento temporaneo è un permesso, e non va in un
/// diario.
impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Link(…)")
    }
}

/// I byte di un file di Dropbox, come [`Sorgente`].
///
/// Il collegamento temporaneo (`files/get_temporary_link`) vale quattro ore e
/// si legge con `Range` senza token: lo si chiede una volta, e di nuovo — **una**
/// volta — se smette di valere a metà di un brano.
#[derive(Debug)]
pub struct DaDropbox {
    api: Api,
    flusso: Rete,
    percorso: String,
    link: Mutex<Option<Link>>,
}

impl DaDropbox {
    fn nuovo_link(&self) -> Result<String, AppError> {
        let corpo = self.api.rpc(
            "files/get_temporary_link",
            &json!({ "path": self.percorso }),
        )?;
        let url = corpo
            .get("link")
            .and_then(Value::as_str)
            .filter(|u| collegamento_ammesso(u))
            .ok_or_else(|| forma("niente collegamento temporaneo"))?
            .to_owned();
        *self.link.lock().unwrap_or_else(PoisonError::into_inner) = Some(Link {
            url: url.clone(),
            da: Instant::now(),
        });
        Ok(url)
    }

    fn link(&self) -> Result<String, AppError> {
        let ricordato = self
            .link
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        match ricordato {
            Some(link) if link.da.elapsed() < VALIDITA_LINK => Ok(link.url),
            _ => self.nuovo_link(),
        }
    }
}

impl Sorgente for DaDropbox {
    fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
        self.pezzo_finche(da, quanti, &|| true)
    }

    fn pezzo_finche(
        &self,
        da: u64,
        quanti: u64,
        avanti: &dyn Fn() -> bool,
    ) -> Result<Pezzo, AppError> {
        let url = self.link()?;
        let esito = match self.flusso.intervallo_con(&url, da, quanti, avanti, &[]) {
            Err(err) if link_scaduto(&err) => {
                let nuovo = self.nuovo_link()?;
                self.flusso.intervallo_con(&nuovo, da, quanti, avanti, &[])
            }
            altro => altro,
        };
        // Il collegamento non esce da qui: è un permesso.
        esito
            .map(|pezzo| Pezzo {
                url_finale: None,
                ..pezzo
            })
            .map_err(|err| {
                let mut pulito = AppError::new(match err.code() {
                    ErrorCode::NetHttp { status, .. } => ErrorCode::NetHttp {
                        status: *status,
                        url: None,
                    },
                    altro => altro.clone(),
                });
                if let Some(causa) = err.cause() {
                    pulito = pulito.with_cause(
                        causa
                            .split("https://")
                            .next()
                            .unwrap_or_default()
                            .to_owned(),
                    );
                }
                pulito
            })
    }
}

/// Un collegamento temporaneo si segue solo in `https`: porta il brano, e su
/// `http` chiunque fra qui e Dropbox lo leggerebbe. Le prove parlano con un
/// servitore su 127.0.0.1, che di certificati non ne ha.
fn collegamento_ammesso(url: &str) -> bool {
    url.starts_with("https://") || (cfg!(test) && url.starts_with("http://127.0.0.1"))
}

/// Un collegamento temporaneo che non vale più.
fn link_scaduto(err: &AppError) -> bool {
    matches!(
        err.code(),
        ErrorCode::NetHttp {
            status: 401 | 403 | 404 | 410,
            ..
        }
    )
}

/// Una risposta di Dropbox che non ha la forma attesa.
fn forma(dettaglio: &str) -> AppError {
    AppError::new(ErrorCode::NetBadSchema {
        service: Some(CHIAVE.to_owned()),
        detail: Some(dettaglio.to_owned()),
    })
}

#[cfg(test)]
mod prove;
