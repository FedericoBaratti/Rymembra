//! Dallo stderr di yt-dlp a un codice del catalogo.
//!
//! Porto di `legacy/Aeter/electron/modules/download/errors.ts`, con **una
//! sottrazione voluta**: là c'erano due funzioni,
//! `friendlyYtError` (il codice da mostrare) e `classifyDownloadFailure` (se
//! ritentare). Due funzioni per due proprietà dello stesso guasto, e quindi due
//! elenchi di espressioni regolari da tenere allineati — che non lo erano: il
//! caso ignoto valeva `permanent` sul desktop e `transient` sul telefono, cioè
//! lo stesso guasto si ritentava su una piattaforma e no sull'altra. È il difetto
//! raccontato in testa a [`aether_domain::errors`].
//!
//! Qui la funzione è **una** e restituisce un [`ErrorCode`]. La ritentabilità
//! non si decide: si legge da [`AppError::is_retryable`], che la prende dal
//! catalogo. Due piattaforme non possono più rispondere in modo diverso perché
//! non c'è più una seconda risposta da dare.
//!
//! # L'ordine dei confronti è la parte difficile
//!
//! Uno stderr di yt-dlp contiene spesso più righe e più cause. `Video
//! unavailable. This video has been removed by the uploader` contiene sia
//! «unavailable» sia «removed», e va bene perché portano allo stesso codice; ma
//! un 403 dentro un messaggio che nomina anche la rete deve restare un 403,
//! perché i due codici hanno ritentabilità diverse. Si va quindi **dal più
//! specifico al più generico**, e i terminali prima dei passeggeri: sbagliare
//! verso il terminale fa perdere un brano che si poteva prendere, sbagliare verso
//! il passeggero fa ritentare per sempre qualcosa che non ci sarà mai.

use aether_domain::{AppError, ErrorCode};

/// Quante righe di stderr si tengono come causa.
///
/// yt-dlp può scrivere pagine di avvertimenti; la causa utile sta nelle ultime
/// righe. Tenerle tutte vorrebbe dire scriversi in database un log intero per
/// brano fallito.
const CODA_MASSIMA: usize = 400;

/// Traduce lo stderr di yt-dlp nel codice che lo descrive.
///
/// Il codice ignoto è [`ErrorCode::DownloadYtError`], che il catalogo dichiara
/// **ritentabile**: un guasto che non sappiamo leggere è più spesso la rete che
/// un video sparito, e non ritentarlo vuol dire perdere brani senza dirlo.
#[must_use]
pub fn da_stderr(stderr: &str) -> ErrorCode {
    let piegato = stderr.to_lowercase();
    let contiene = |aghi: &[&str]| aghi.iter().any(|ago| piegato.contains(ago));

    // ── terminali, dal più specifico ────────────────────────────────────────
    if contiene(&[
        "sign in to confirm your age",
        "age-restricted",
        "age restricted",
    ]) {
        return ErrorCode::DownloadAgeRestricted;
    }
    if contiene(&["private video", "this video is private"]) {
        return ErrorCode::DownloadPrivate;
    }
    if contiene(&[
        "video unavailable",
        "has been removed",
        "no longer available",
        "account associated with this video has been terminated",
    ]) {
        return ErrorCode::DownloadUnavailable;
    }
    if contiene(&["is not a valid url", "unsupported url"]) {
        return ErrorCode::DownloadInvalidUrl;
    }

    // ── passeggeri ──────────────────────────────────────────────────────────
    if contiene(&[
        "http error 429",
        "rate limit",
        "rate-limit",
        "too many requests",
    ]) {
        return ErrorCode::DownloadRateLimited;
    }
    // Il 403 **dopo** il 429: entrambi sono rifiuti, ma «rallenta» è
    // un'istruzione precisa e «vietato» è una constatazione, e uno stderr che
    // contiene tutti e due sta raccontando il primo.
    if contiene(&["http error 403", "403: forbidden"]) {
        return ErrorCode::DownloadForbidden;
    }
    // Il pacchetto rotto prima della rete: un traceback di `zipimport` nomina
    // spesso anche una connessione, ed è un guasto d'ambiente, non di rete.
    if contiene(&["zipimport", "bad magic number", "zipimporterror"]) {
        return ErrorCode::DownloadYtdlpCorrupted;
    }
    if contiene(&[
        "unable to download webpage",
        "temporary failure",
        "getaddrinfo",
        "connection reset",
        "connection refused",
        "connection aborted",
        "network is unreachable",
        "timed out",
        "timeout",
        "ssl",
        "incomplete read",
        "http error 5",
        "service unavailable",
    ]) {
        return ErrorCode::DownloadNetwork;
    }

    ErrorCode::DownloadYtError {
        detail: dettaglio(stderr),
    }
}

/// L'errore, con lo stderr allegato come causa.
#[must_use]
pub fn errore(stderr: &str) -> AppError {
    let codice = da_stderr(stderr);
    let errore = AppError::new(codice);
    let coda = dettaglio(stderr);
    if coda.is_empty() {
        errore
    } else {
        errore.with_cause(coda)
    }
}

/// La riga di `ERROR:` se c'è, altrimenti la coda dello stderr.
fn dettaglio(stderr: &str) -> String {
    let riga = stderr
        .lines()
        .rev()
        .find(|riga| riga.contains("ERROR"))
        .map(str::trim);
    let scelta = riga.unwrap_or_else(|| stderr.trim());
    let senza_prefisso = scelta
        .trim_start_matches("ERROR:")
        .trim_start_matches("ERROR")
        .trim();
    // Il taglio è per **caratteri**, non per byte: un taglio a metà di un
    // carattere accentato darebbe una stringa non valida da scrivere in SQLite.
    let tagliato: String = senza_prefisso.chars().take(CODA_MASSIMA).collect();
    tagliato
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn i_guasti_terminali_non_si_ritentano() {
        // È il punto di tutto il modulo: la ritentabilità viene dal catalogo,
        // quindi provare il codice è provare anche il comportamento della coda.
        for stderr in [
            "ERROR: Sign in to confirm your age",
            "ERROR: Private video. Sign in if you've been granted access",
            "ERROR: Video unavailable. This video has been removed by the uploader",
            "ERROR: 'pippo' is not a valid URL",
        ] {
            let codice = da_stderr(stderr);
            assert!(
                !AppError::new(codice.clone()).is_retryable(),
                "{stderr} non dovrebbe ritentarsi ({codice:?})"
            );
        }
    }

    #[test]
    fn i_guasti_passeggeri_si_ritentano() {
        for stderr in [
            "ERROR: unable to download webpage: <urlopen error timed out>",
            "ERROR: HTTP Error 403: Forbidden",
            "ERROR: HTTP Error 429: Too Many Requests",
            "ERROR: HTTP Error 503: Service Unavailable",
        ] {
            let codice = da_stderr(stderr);
            assert!(
                AppError::new(codice.clone()).is_retryable(),
                "{stderr} dovrebbe ritentarsi ({codice:?})"
            );
        }
    }

    #[test]
    fn un_guasto_ignoto_si_ritenta() {
        // La scelta che nel vecchio albero divergeva fra le due piattaforme.
        let codice = da_stderr("ERROR: qualcosa che non abbiamo mai visto");
        assert!(matches!(codice, ErrorCode::DownloadYtError { .. }));
        assert!(AppError::new(codice).is_retryable());
    }

    #[test]
    fn rallenta_ha_la_precedenza_su_vietato() {
        // Uno stderr che contiene entrambi sta chiedendo di aspettare, non
        // dicendo che il video è chiuso: il codice sbagliato farebbe ritentare
        // subito e prendersi un altro rifiuto.
        assert_eq!(
            da_stderr("HTTP Error 429: Too Many Requests (403 Forbidden earlier)"),
            ErrorCode::DownloadRateLimited
        );
    }

    #[test]
    fn il_pacchetto_rotto_non_e_un_guasto_di_rete() {
        // Ritentabili entrambi, ma il messaggio all'utente è opposto: uno dice
        // «riprova», l'altro «reinstalla yt-dlp».
        assert_eq!(
            da_stderr("zipimport.ZipImportError: bad magic number in 'yt_dlp'"),
            ErrorCode::DownloadYtdlpCorrupted
        );
    }

    #[test]
    fn il_dettaglio_e_lultima_riga_di_errore_senza_prefisso() {
        let stderr = "WARNING: roba\nERROR: prima causa\nqualcosa\nERROR: causa vera";
        assert_eq!(dettaglio(stderr), "causa vera");
    }

    #[test]
    fn senza_righe_di_errore_si_tiene_la_coda() {
        assert_eq!(dettaglio("  solo questo  "), "solo questo");
        assert_eq!(dettaglio(""), "");
    }

    #[test]
    fn il_dettaglio_si_taglia_per_caratteri() {
        // Con un taglio per byte, un accento a cavallo del limite produrrebbe
        // una stringa non valida — e questa finisce in `download_error`.
        let lungo = "à".repeat(1000);
        let tagliato = dettaglio(&lungo);
        assert_eq!(tagliato.chars().count(), CODA_MASSIMA);
    }

    #[test]
    fn lerrore_porta_lo_stderr_come_causa() {
        let e = errore("ERROR: Video unavailable");
        assert_eq!(*e.code(), ErrorCode::DownloadUnavailable);
        assert_eq!(e.cause(), Some("Video unavailable"));
    }
}
