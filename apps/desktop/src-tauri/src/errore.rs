//! L'errore che attraversa l'IPC.
//!
//! [`AppError`] non è serializzabile, ed è deliberato: il dominio non dipende da
//! serde perché non deve avere un'opinione su come lo si trasmette. La forma sul
//! filo appartiene a chi la usa, ed è qui.
//!
//! # Perché non basta una stringa
//!
//! Era così nel vecchio albero, e il costo è documentato nel changelog: il
//! codice d'errore di ExoPlayer veniva scartato, e l'interfaccia mostrava «2».
//! Un errore che arriva all'interfaccia come testo non si può tradurre, non si
//! può ritentare in automatico, e non si può nemmeno raggruppare in un log.
//!
//! Quel che passa di qui è quindi il record intero: codice, dominio, gravità,
//! ritentabilità e chiave i18n. L'interfaccia decide cosa mostrare guardando
//! quelli, non facendo `indexOf` su una frase.
//!
//! # Cosa NON passa di qui
//!
//! Il **dettaglio** di un errore (`AppError::with_detail`) è per chi sviluppa e
//! resta nel diario: la finestra non lo riceve. Un testo scritto in italiano dal
//! nucleo, mostrato sotto la frase tradotta in un'interfaccia inglese, è un
//! testo tecnico nella lingua sbagliata (R10, R17). A chi guarda arrivano la
//! frase del codice, i **dati** che quella frase nomina
//! (`ErrorCode::valori_della_frase`) e, solo per i servizi remoti, quel che il
//! servizio ha scritto. Le prove in fondo lo tengono fermo.

use std::collections::BTreeMap;

use rymembra_domain::errors::AppError;
use serde::Serialize;

use crate::nota;

/// Un errore, nella forma che la finestra riceve.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErroreIpc {
    /// Il codice del catalogo, ad esempio `db.openFailed`.
    pub code: String,
    /// Il dominio: `db`, `fs`, `library`, `metadata`…
    pub domain: String,
    /// `info`, `warning`, `error`, `fatal`.
    pub severity: String,
    /// Ritentare ha senso?
    pub retryable: bool,
    /// La chiave per la traduzione.
    pub i18n_key: String,
    /// Quel che ha scritto un servizio remoto (OpenRouter, OpenAI…), mostrato
    /// com'è sotto la frase del codice. Mai testo di Rymembra.
    pub service_text: Option<String>,
    /// I dati che la frase del codice nomina (`{nome}`), i soli sicuri da
    /// mostrare: vedi `ErrorCode::valori_della_frase`. Vuoto quasi sempre.
    pub values: BTreeMap<String, String>,
    /// La causa tecnica, per il log e la diagnostica.
    pub cause: Option<String>,
}

impl From<AppError> for ErroreIpc {
    fn from(error: AppError) -> Self {
        // `code`, `domain` e compagnia stanno su `ErrorCodeKind`, che è `Copy`:
        // `ErrorCode` porta anche i dati del caso (un percorso, due versioni) e
        // non lo è. `kind()` è il passaggio dall'uno all'altro.
        let kind = error.code().kind();
        Self {
            code: kind.code().to_owned(),
            domain: format!("{:?}", kind.domain()).to_lowercase(),
            severity: format!("{:?}", error.severity()).to_lowercase(),
            retryable: error.is_retryable(),
            i18n_key: kind.i18n_key().to_owned(),
            service_text: error.service_text().map(ToOwned::to_owned),
            values: error
                .code()
                .valori_della_frase()
                .into_iter()
                .map(|(nome, valore)| (nome.to_owned(), valore))
                .collect(),
            cause: error.cause().map(ToOwned::to_owned),
        }
    }
}

/// Il risultato di un comando: o il dato, o il record d'errore.
///
/// L'errore sta dietro un `Box` perché `Result<T, E>` è grande quanto il
/// maggiore dei due: cinque stringhe di errore occupano centocinquanta byte in
/// **ogni** risposta, anche in quelle andate bene, e le risposte andate bene
/// sono tutte tranne una su mille.
pub type Esito<T> = Result<T, Box<ErroreIpc>>;

/// Il ponte da un errore di dominio a quello che la finestra riceve.
///
/// Una funzione invece di `.map_err(ErroreIpc::from)` sparso, e il giorno in
/// cui gli errori sarebbero andati anche nel log è arrivato: **è questa riga**.
///
/// Ogni guasto che l'utente vede passa di qui — è la definizione di questo
/// tipo — quindi il diario contiene esattamente ciò di cui qualcuno potrebbe
/// lamentarsi, e nell'ordine in cui è successo. È la differenza fra leggere una
/// segnalazione che dice «non funziona» e vedere quale comando ha risposto
/// male, con che codice, tre secondi prima.
///
/// Nel diario va il **codice**, non la frase: la frase è tradotta e scritta per
/// chi ascolta, mentre `db.openFailed` è la stessa parola in ogni lingua. La
/// causa tecnica invece sì, perché è l'unica metà che dice *quale* apertura è
/// fallita, e con lei il **dettaglio** di chi ha scritto il punto: è per questo
/// che non serve alla finestra. I `valori` no: un account è un indirizzo di
/// posta, e il diario si allega alle segnalazioni.
pub fn errore(err: AppError) -> Box<ErroreIpc> {
    let dettaglio = err.detail().map(ToOwned::to_owned);
    let ipc = ErroreIpc::from(err);
    nota!("{}", riga_del_diario(&ipc, dettaglio.as_deref()));
    Box::new(ipc)
}

/// La riga che [`errore`] scrive nel diario.
///
/// Pura, perché è la riga che decide cosa chi legge il diario prende per un
/// guasto. Un codice di gravità `info` è, per il catalogo, un «esito atteso,
/// non un guasto» — «nessun risultato», «annullato», e l'uscita audio ancora in
/// apertura, che a ogni avvio con la coda ripristinata riempiva il diario di
/// `[errore] playback.engineOpening`: la riga resta, con tutti i suoi campi, ma
/// come `[esito]`. Vale per ogni codice `info` del catalogo, non per uno solo.
fn riga_del_diario(ipc: &ErroreIpc, dettaglio: Option<&str>) -> String {
    let etichetta = if ipc.severity == "info" {
        "[esito]"
    } else {
        "[errore]"
    };
    format!(
        "{etichetta} {} gravita={} ritentabile={} causa={} dettaglio={}",
        ipc.code,
        ipc.severity,
        ipc.retryable,
        ipc.cause.as_deref().unwrap_or("—"),
        dettaglio.unwrap_or("—")
    )
}

#[cfg(test)]
mod prove {
    use super::*;
    use rymembra_domain::errors::ErrorCode;

    /// Un esito atteso non è un `[errore]` nel diario.
    ///
    /// `playback.engineOpening` ha gravità `info` («esito atteso, non un
    /// guasto»): l'uscita audio si apre in disparte, dopo la libreria, e la
    /// domanda di stato che arriva prima ha quella risposta. La finestra la
    /// tratta da stato passeggero (`apreAncora`), ma il diario la scriveva come
    /// `[errore] playback.engineOpening gravita=info ritentabile=true` a ogni
    /// avvio con la coda ripristinata (collaudo in Windows Sandbox). Il diario
    /// «contiene esattamente ciò di cui qualcuno potrebbe lamentarsi»: un
    /// `[errore]` che non è un errore fa cercare un guasto dove non c'è.
    #[test]
    fn un_esito_atteso_non_e_scritto_come_errore() {
        let atteso = ErroreIpc::from(AppError::new(ErrorCode::PlaybackEngineOpening));
        assert_eq!(atteso.severity, "info", "il catalogo lo dice un esito");
        let riga = riga_del_diario(&atteso, None);
        assert!(
            !riga.starts_with("[errore]"),
            "un esito di gravità info non è un errore: {riga}"
        );
        assert!(
            riga.starts_with("[esito] playback.engineOpening gravita=info ritentabile=true"),
            "la riga resta nel diario, col suo codice: {riga}"
        );

        // Un guasto vero resta com'era: stessa etichetta, stessi campi.
        let guasto = ErroreIpc::from(
            AppError::new(ErrorCode::PlaybackDeviceLost).with_cause("cuffie staccate".to_owned()),
        );
        assert_eq!(guasto.severity, "error");
        assert_eq!(
            riga_del_diario(&guasto, Some("dopo cinque secondi")),
            "[errore] playback.deviceLost gravita=error ritentabile=true causa=cuffie staccate dettaglio=dopo cinque secondi"
        );
    }

    /// I file `.rs` sotto una cartella della radice del repository, col loro
    /// percorso relativo alla radice.
    fn sorgenti(cartella: &str) -> Vec<(String, String)> {
        fn visita(
            cartella: &std::path::Path,
            radice: &std::path::Path,
            tutti: &mut Vec<(String, String)>,
        ) {
            let Ok(voci) = std::fs::read_dir(cartella) else {
                return;
            };
            for voce in voci.flatten() {
                let percorso = voce.path();
                if percorso.is_dir() {
                    if !matches!(voce.file_name().to_str(), Some("target" | "node_modules")) {
                        visita(&percorso, radice, tutti);
                    }
                } else if percorso.extension().is_some_and(|e| e == "rs")
                    && let Ok(testo) = std::fs::read_to_string(&percorso)
                {
                    let relativo = percorso
                        .strip_prefix(radice)
                        .unwrap_or(&percorso)
                        .to_string_lossy()
                        .replace('\\', "/");
                    tutti.push((relativo, testo));
                }
            }
        }
        let radice = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let mut tutti = Vec::new();
        visita(&radice.join(cartella), &radice, &mut tutti);
        tutti
    }

    /// Chi può scrivere un testo di un servizio remoto nell'errore.
    ///
    /// `rymembra-ia` è il solo che ne produce: il testo che OpenRouter o un
    /// server compatibile scrive nel corpo della risposta, mostrato com'è
    /// perché contiene la correzione («use this slug instead»). Gli altri due
    /// file sono la definizione e questa prova.
    const PUNTI_DEI_SERVIZI: [&str; 3] = [
        "core/rymembra-domain/src/errors/mod.rs",
        "core/rymembra-ia/src/cliente.rs",
        "apps/desktop/src-tauri/src/errore.rs",
    ];

    /// Nessun testo scritto dal nucleo viaggia verso la finestra (R10, R17).
    ///
    /// `with_message` non esiste più: il testo di un errore era un secondo
    /// canale di frasi italiane sotto quelle del catalogo, e con l'app in
    /// inglese si leggeva una riga italiana sotto la frase tradotta. Resta
    /// `with_detail` (diario) e `with_service_text` (le parole di un servizio),
    /// quest'ultimo solo nei punti dei servizi.
    #[test]
    fn il_testo_di_un_errore_non_e_un_secondo_canale_di_frasi() {
        let vecchio = concat!("with_", "message(");
        let servizio = concat!("with_service_", "text(");
        let mut tutti = sorgenti("core");
        tutti.extend(sorgenti("apps"));
        assert!(tutti.len() > 200, "i sorgenti non si sono trovati");

        let mut vecchi = Vec::new();
        let mut fuori_posto = Vec::new();
        for (file, testo) in &tutti {
            // `cpal::Error::with_message` è di un'altra libreria (le prove del motore audio).
            if testo.contains(vecchio) && !file.starts_with("core/rymembra-play/") {
                vecchi.push(file.as_str());
            }
            if testo.contains(servizio) && !PUNTI_DEI_SERVIZI.contains(&file.as_str()) {
                fuori_posto.push(file.as_str());
            }
        }
        assert!(
            vecchi.is_empty(),
            "`with_message` non esiste più: un dettaglio per chi sviluppa è `with_detail` \
             (solo nel diario), un dato per la frase è un campo del codice in \
             `valori_della_frase`: {vecchi:?}"
        );
        assert!(
            fuori_posto.is_empty(),
            "`with_service_text` è solo per le parole di un servizio remoto: {fuori_posto:?}"
        );
    }

    /// Quel che il nucleo scrive per sé non arriva alla finestra, e quel che
    /// un servizio ha scritto sì.
    #[test]
    fn la_finestra_riceve_il_testo_del_servizio_e_non_il_dettaglio() {
        let ipc = ErroreIpc::from(
            AppError::new(ErrorCode::NetHttp {
                status: 400,
                url: None,
            })
            .with_detail("il profilo «openrouter» non risponde")
            .with_service_text("use this slug instead: minimax/minimax-m3")
            .with_cause("HTTP 400"),
        );
        let json = serde_json::to_string(&ipc).expect("si serializza");
        assert!(
            !json.contains("openrouter"),
            "il dettaglio è partito: {json}"
        );
        assert!(
            !json.contains("detail") && !json.contains("\"message\""),
            "{json}"
        );
        assert!(
            json.contains(r#""serviceText":"use this slug instead: minimax/minimax-m3""#),
            "{json}"
        );
        assert!(ipc.values.is_empty(), "nessun dato per questo codice");
    }

    /// I soli dati che partono sono quelli che il codice dichiara.
    #[test]
    fn i_valori_sono_quelli_che_il_codice_dichiara() {
        let ipc = ErroreIpc::from(AppError::new(ErrorCode::CloudOtherAccount {
            servizio: "OneDrive".to_owned(),
            account: "casa@esempio.it".to_owned(),
        }));
        assert_eq!(
            ipc.values.get("ottenuto").map(String::as_str),
            Some("casa@esempio.it")
        );
        // Un percorso, un indirizzo: parametri del codice, non dati della frase.
        let altro = ErroreIpc::from(AppError::new(ErrorCode::FsNotFound {
            path: r"C:\Musica\a.flac".to_owned(),
        }));
        assert!(altro.values.is_empty(), "{:?}", altro.values);
    }
}
