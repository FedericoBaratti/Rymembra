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

use aether_domain::errors::AppError;
use serde::Serialize;

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
    /// Il messaggio per l'utente, quando ce n'è uno.
    pub message: Option<String>,
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
            message: error.message().map(ToOwned::to_owned),
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
/// Una funzione invece di `.map_err(ErroreIpc::from)` sparso: il giorno in cui
/// gli errori andranno anche nel log, il posto dove aggiungerlo è questo.
pub fn errore(err: AppError) -> Box<ErroreIpc> {
    Box::new(ErroreIpc::from(err))
}
