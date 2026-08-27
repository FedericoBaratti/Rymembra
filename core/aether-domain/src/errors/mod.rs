//! Gli errori: un record serializzabile, con un catalogo solo.
//!
//! # Il difetto che questo modulo esiste per impedire
//!
//! Nel vecchio albero la stessa informazione viveva in tre posti, e i tre erano
//! già divergiti:
//!
//! 1. i punti che producevano il codice (`download/errors.ts`, `binaries.ts`, …);
//! 2. le tabelle `SIMPLE_CODES`/`PARAM_CODES` in `src/lib/ipcError.ts`;
//! 3. le chiavi `errors.*` nei file di traduzione.
//!
//! Risultato misurato: 37 chiavi `errors.*` nei bundle desktop contro ~24
//! mappate, con **otto chiavi orfane** rimaste indietro perché l'i18n era stato
//! allineato dal lato mobile e la tabella dei codici no. E
//! `classifyDownloadFailure` che decideva `permanent` sul desktop e `transient`
//! sul mobile **per lo stesso identico guasto**: un download che sul telefono si
//! ritentava e sul computer no, senza che nessuna delle due parti fosse
//! «sbagliata» in sé.
//!
//! Qui dominio, gravità, ritentabilità e chiave i18n sono dichiarati una volta
//! sola, accanto al codice. Le quattro cose si leggono su una riga, e non
//! esistono due posti da tenere allineati.
//!
//! # Perché è un record e non una gerarchia di eccezioni
//!
//! Il vecchio albero aveva dieci sottoclassi di `Error`, e morivano tutte al
//! primo salto: attraversando l'IPC o la rete restava una stringa. Un errore qui
//! è **dati** — codice, parametri tipizzati, causa — quindi attraversa FFI,
//! processi e rete senza perdere né il dominio né la ritentabilità, che sono le
//! due cose per cui il catalogo esiste.

mod catalog;

pub use catalog::{Domain, ErrorCode, ErrorCodeKind, RetryRule, Severity};

/// Un errore di Aether: cosa è andato storto, e tutto ciò che serve per
/// deciderne il seguito.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppError {
    code: ErrorCode,
    message: Option<String>,
    cause: Option<String>,
}

impl AppError {
    /// Costruisce un errore dal suo codice.
    #[must_use]
    pub const fn new(code: ErrorCode) -> Self {
        Self {
            code,
            message: None,
            cause: None,
        }
    }

    /// Aggiunge un messaggio **per chi sviluppa**, non per chi ascolta musica.
    ///
    /// Il testo mostrato all'utente si ottiene traducendo [`ErrorCode::i18n_key`]
    /// con i parametri del codice: un messaggio scritto qui in italiano
    /// resterebbe in italiano anche con l'app in inglese, ed è precisamente il
    /// modo in cui i messaggi d'errore smettono di essere tradotti.
    #[must_use]
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Allega la descrizione dell'errore sottostante.
    #[must_use]
    pub fn with_cause(mut self, cause: impl Into<String>) -> Self {
        self.cause = Some(cause.into());
        self
    }

    /// Il codice.
    #[must_use]
    pub const fn code(&self) -> &ErrorCode {
        &self.code
    }

    /// Il messaggio per chi sviluppa, se c'è.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// La causa sottostante, se c'è.
    #[must_use]
    pub fn cause(&self) -> Option<&str> {
        self.cause.as_deref()
    }

    /// Ritentare ha senso?
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        self.code.is_retryable()
    }

    /// La gravità.
    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.code.kind().severity()
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}]", self.code.kind().code())?;
        if let Some(message) = &self.message {
            write!(f, " {message}")?;
        }
        if let Some(cause) = &self.cause {
            write!(f, " (causa: {cause})")?;
        }
        Ok(())
    }
}

impl std::error::Error for AppError {}

impl From<ErrorCode> for AppError {
    fn from(code: ErrorCode) -> Self {
        Self::new(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ogni_codice_ha_una_chiave_i18n_distinta() {
        let mut chiavi: Vec<&str> = ErrorCodeKind::ALL.iter().map(|k| k.i18n_key()).collect();
        let totale = chiavi.len();
        chiavi.sort_unstable();
        chiavi.dedup();
        assert_eq!(
            chiavi.len(),
            totale,
            "due codici condividono una chiave i18n"
        );
        assert!(
            totale >= 80,
            "il catalogo si è svuotato? solo {totale} codici"
        );
    }

    #[test]
    fn ogni_chiave_i18n_deriva_dal_codice() {
        // Derivata, non scritta a mano: è il meccanismo per cui non possono
        // esistere chiavi orfane come le otto trovate nel vecchio albero.
        for kind in ErrorCodeKind::ALL {
            assert_eq!(kind.i18n_key(), format!("errors.{}", kind.code()));
        }
    }

    #[test]
    fn i_codici_legacy_sono_univoci_e_ritrovabili() {
        // Le righe `downloads` già salvate in SQLite contengono questi codici:
        // senza la mappa inversa diventerebbero errori sconosciuti dopo la
        // migrazione, cioè guasti veri mostrati come «errore sconosciuto».
        let mut visti: Vec<&str> = Vec::new();
        for kind in ErrorCodeKind::ALL {
            if let Some(legacy) = kind.legacy_code() {
                assert!(
                    !visti.contains(&legacy),
                    "codice legacy duplicato: {legacy}"
                );
                visti.push(legacy);
                assert_eq!(ErrorCodeKind::from_legacy_code(legacy), Some(*kind));
            }
        }
        assert!(
            visti.len() >= 25,
            "solo {} codici legacy mappati",
            visti.len()
        );
    }

    #[test]
    fn i_codici_legacy_a_parametro_si_riconoscono_dal_prefisso() {
        // Viaggiavano come `PREFISSO:payload`, es. `DL_YT_ERROR:...`.
        assert_eq!(
            ErrorCodeKind::from_legacy_code("EXT_SEARCH_FAILED:archive.org"),
            Some(ErrorCodeKind::DownloadExternalSearchFailed)
        );
        assert_eq!(ErrorCodeKind::from_legacy_code("SCONOSCIUTO"), None);
    }

    #[test]
    fn la_ritentabilita_di_http_dipende_dallo_stato() {
        // È il caso che nel vecchio albero era una funzione a sé, e l'unico in
        // cui la decisione non si legge dal solo codice.
        let retry = |status| AppError::new(ErrorCode::NetHttp { status, url: None }).is_retryable();
        assert!(retry(500), "5xx: il server può riprendersi");
        assert!(retry(503));
        assert!(retry(429), "troppe richieste: si riprova più tardi");
        assert!(retry(408), "timeout di richiesta");
        assert!(!retry(404), "non esiste: riprovare non lo farà esistere");
        assert!(!retry(401));
        assert!(!retry(400));
    }

    #[test]
    fn download_ha_una_sola_ritentabilita_per_codice() {
        // Il difetto originale: default opposti sulle due piattaforme. Qui la
        // decisione sta nel catalogo, quindi è la stessa ovunque per costruzione.
        assert!(!ErrorCode::DownloadUnavailable.is_retryable());
        assert!(!ErrorCode::DownloadPrivate.is_retryable());
        assert!(ErrorCode::DownloadNetwork.is_retryable());
        // Un `403` non è una proprietà del file: arriva a ondate, legato
        // all'indirizzo IP e al ritmo delle richieste. Con `Never` il brano
        // resterebbe perduto per sempre.
        assert!(ErrorCode::DownloadForbidden.is_retryable());
        // Una licenza che non permette la copia non cambia riprovando, ed è la
        // differenza fra «riprovo» e «dico dove si compra».
        assert!(!ErrorCode::DownloadNotPermitted { licenza: None }.is_retryable());
    }

    #[test]
    fn un_annullamento_non_e_un_guasto() {
        // Nel vecchio albero arrivava come `new Error('Aborted')` e finiva nei
        // log accanto ai guasti veri.
        let annullato = ErrorCode::InternalAborted { what: None };
        assert_eq!(annullato.kind().severity(), Severity::Info);
        assert!(
            !annullato.is_retryable(),
            "ritentare ciò che è stato annullato è il contrario di ciò che è stato chiesto"
        );
    }

    #[test]
    fn il_messaggio_mostra_il_codice_e_la_causa() {
        let e = AppError::new(ErrorCode::FsNotFound {
            path: "C:/Music/a.mp3".into(),
        })
        .with_message("scansione interrotta")
        .with_cause("ENOENT");
        assert_eq!(
            e.to_string(),
            "[fs.notFound] scansione interrotta (causa: ENOENT)"
        );
    }
}
