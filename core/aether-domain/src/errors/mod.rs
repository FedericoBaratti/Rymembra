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
pub mod rete;

pub use catalog::{Domain, ErrorCode, ErrorCodeKind, RetryRule, Severity};

/// Quanto può essere lunga una causa.
///
/// Cinquecento caratteri sono più del doppio della causa più lunga che si
/// incontri in un guasto vero — un errore di sistema, la frase di un servizio,
/// la riga di un parser — e molto meno di una pagina.
const CAUSA_MASSIMA: usize = 500;

/// La causa, ridotta a una riga leggibile.
///
/// # Il difetto che chiude
///
/// Una causa è quel che il mondo di fuori ha detto, e il mondo di fuori a
/// volte risponde con un documento. Una ricerca respinta dal guardiano di un
/// catalogo torna con la **pagina HTML intera** del blocco; un motore di
/// ricerca che rifiuta una frase troppo lunga torna con il suo errore
/// completo, annidato e indentato. Il testo arrivava qui così com'era, e da
/// qui finiva nel diario e sulla console: righe su righe di `<title>`, `<div>`
/// e `<script>` per dire una cosa sola, cioè che quella ricerca non è passata.
///
/// Il diario è quel che si allega quando si racconta un guasto, e si legge in
/// diagonale: un guasto per riga. Una causa che porta dentro i suoi a capo
/// rompe quella forma, e una che porta dentro venti chilobyte la rende
/// inservibile.
///
/// # Perché tagliare e non buttare
///
/// Perché l'inizio di quelle risposte è la parte che dice qualcosa: il titolo
/// della pagina di blocco, il tipo dell'errore del motore di ricerca. Il resto
/// è contorno. E quanto se n'è tagliato si dice, così chi legge sa che c'era
/// dell'altro invece di credere che il servizio si sia fermato a metà frase.
fn una_riga_sola(causa: &str) -> String {
    let piatta = causa.split_whitespace().collect::<Vec<_>>().join(" ");
    let quanti = piatta.chars().count();
    if quanti <= CAUSA_MASSIMA {
        return piatta;
    }
    // Per carattere e non per byte: tagliare a metà di una lettera accentata
    // darebbe una stringa che non è più UTF-8 valido.
    let taglio: String = piatta.chars().take(CAUSA_MASSIMA).collect();
    format!("{taglio}… [{quanti} caratteri in tutto]")
}

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
    ///
    /// Il testo viene messo **su una riga sola e accorciato**: vedi
    /// [`una_riga_sola`] per il perché.
    #[must_use]
    pub fn with_cause(mut self, cause: impl Into<String>) -> Self {
        self.cause = Some(una_riga_sola(&cause.into()));
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

    #[test]
    fn una_causa_lunga_una_pagina_diventa_una_riga() {
        // La pagina di blocco di un guardiano, in miniatura: a capo, rientri, e
        // la sola cosa che conta nel titolo.
        let pagina = format!(
            "<html>
  <head>
    <title>Attention Required! | Cloudflare</title>
  </head>
  <body>{}</body>
</html>",
            "x".repeat(20_000)
        );
        let e = AppError::new(ErrorCode::InternalAborted { what: None }).with_cause(pagina);
        let causa = e.cause().expect("la causa c'è");

        // Una riga sola: il diario si legge in diagonale.
        assert!(!causa.contains('\n'));
        // Accorciata, e con scritto di quanto.
        assert!(causa.chars().count() < CAUSA_MASSIMA + 40);
        assert!(causa.ends_with("caratteri in tutto]"));
        // E l'inizio, che è la parte che dice qualcosa, è ancora lì.
        assert!(causa.contains("Attention Required"));
    }

    #[test]
    fn una_causa_corta_resta_quella_che_era() {
        let e = AppError::new(ErrorCode::InternalAborted { what: None }).with_cause("ENOENT");
        assert_eq!(e.cause(), Some("ENOENT"));
    }

    #[test]
    fn accorciare_non_taglia_a_meta_di_una_lettera() {
        // Cinquecento caratteri accentati sono mille byte: un taglio contato in
        // byte cadrebbe in mezzo all'ultima lettera.
        let e = AppError::new(ErrorCode::InternalAborted { what: None })
            .with_cause("è".repeat(CAUSA_MASSIMA + 10));
        let causa = e.cause().expect("la causa c'è");
        assert!(causa.starts_with(&"è".repeat(CAUSA_MASSIMA)));
    }
}
