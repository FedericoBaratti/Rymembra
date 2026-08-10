//! Chiedere a YouTube quali video potrebbero essere questo brano.
//!
//! Porto di `searchYoutubeOnce`/`searchYoutube`
//! (`legacy/Aeter/electron/modules/download/spotifyEngine.ts:66` e `113`), con
//! due differenze volute.
//!
//! # Si tiene il canale
//!
//! Il vecchio albero teneva `url`, `duration` e `title` e buttava via il resto
//! (`spotifyEngine.ts:93-99`). È il motivo per cui là una preferenza per i canali
//! ufficiali non era **scrivibile**: non c'era niente su cui esprimerla. Qui si
//! tengono anche `channel`, `uploader` e `channel_is_verified`, che sono ciò su
//! cui decide [`aether_domain::yt_match::scegli_candidato`].
//!
//! # Otto risultati e non cinque
//!
//! Con un solo criterio — la durata — cinque bastavano. Con un filtro sui canali
//! e uno sul rumore nel titolo, cinque risultati possono ridursi a zero
//! candidati accettabili per un brano che su YouTube c'è eccome. Otto costa
//! qualche decina di millisecondi in più a una ricerca che ne dura migliaia.
//!
//! # «Non trovato» e «non ho potuto cercare» sono due cose diverse
//!
//! `Ok(vec![])` vuol dire *ho cercato e non c'è*: terminale, il brano va segnato
//! introvabile e non ritentato. `Err` vuol dire *non sono riuscito a cercare*:
//! passeggero, si ritenta. Confonderle è il difetto che fa sparire in silenzio i
//! brani quando la rete singhiozza, ed è esattamente ciò che la nota a
//! `spotifyEngine.ts:54-56` racconta di aver visto succedere con una scadenza
//! troppo corta.

use std::time::Duration;

use aether_domain::yt_match::{Candidato, query_larga, query_stretta};
use aether_domain::{AppError, ErrorCode, SpotifyTrack};

use crate::argomenti::{ARG_ESTRATTORE, ARG_RETE};
use crate::binario::Binari;
use crate::processo;

/// Quanti risultati chiedere a YouTube.
pub const LIMITE_RISULTATI: u32 = 8;

/// Quanto si aspetta una ricerca prima di dichiararla scaduta.
///
/// Sessanta secondi, non trenta. Un avvio a freddo di yt-dlp — che è Python
/// dentro uno zip — più tre ricerche in parallelo è molto più lento di quanto
/// sembri da fermi, e con trenta secondi si producevano «non trovato» falsi:
/// brani segnati introvabili per sempre perché il processo non aveva fatto in
/// tempo ad aprirsi.
pub const SCADENZA: Duration = Duration::from_secs(60);

/// Cerca il brano su YouTube: prima la query stretta, poi la larga se serve.
///
/// # Errori
///
/// Un [`AppError`] **ritentabile** quando la ricerca non è riuscita a
/// rispondere. Zero risultati non è un errore: è `Ok` con un vettore vuoto, e
/// per chi chiama vuol dire terminale.
pub fn cerca(
    binari: &Binari,
    brano: &SpotifyTrack,
    annullato: &dyn Fn() -> bool,
) -> Result<Vec<Candidato>, AppError> {
    let stretta = query_stretta(brano);
    let primi = cerca_query(binari, &stretta, annullato)?;
    if !primi.is_empty() {
        return Ok(primi);
    }

    // La seconda richiesta si fa solo se dice qualcosa di diverso dalla prima:
    // `query_larga` restituisce `None` quando coinciderebbe.
    let Some(larga) = query_larga(brano) else {
        return Ok(Vec::new());
    };
    cerca_query(binari, &larga, annullato)
}

/// Una ricerca sola, con la stringa data.
///
/// # Errori
///
/// Come [`cerca`].
pub fn cerca_query(
    binari: &Binari,
    query: &str,
    annullato: &dyn Fn() -> bool,
) -> Result<Vec<Candidato>, AppError> {
    if annullato() {
        return Err(annullamento());
    }
    let ytdlp = binari.richiedi_ytdlp()?;

    let mut argomenti: Vec<String> = vec![
        "--dump-single-json".to_owned(),
        "--flat-playlist".to_owned(),
        "--no-warnings".to_owned(),
        "--no-check-certificates".to_owned(),
    ];
    argomenti.extend(ARG_ESTRATTORE.iter().map(|a| (*a).to_owned()));
    argomenti.extend(ARG_RETE.iter().map(|a| (*a).to_owned()));
    argomenti.push(format!("ytsearch{LIMITE_RISULTATI}:{query}"));

    let esito = processo::esegui(ytdlp, &argomenti, SCADENZA, annullato)
        .map_err(|e| errore_di_avvio(binari, &e))?;

    if esito.annullato {
        return Err(annullamento());
    }
    if esito.scaduto {
        return Err(AppError::new(ErrorCode::DownloadYtdlpTimeout)
            .with_message(format!("ricerca scaduta dopo {}s", SCADENZA.as_secs())));
    }
    if !esito.riuscito() {
        return Err(crate::errori::errore(&esito.stderr));
    }

    analizza(&esito.stdout).ok_or_else(|| {
        AppError::new(ErrorCode::DownloadYtdlpBadResponse)
            .with_message("l'uscita di --dump-single-json non è leggibile")
    })
}

/// Legge l'uscita di `--dump-single-json` in candidati.
///
/// `None` solo quando il JSON non si legge affatto: un elenco di voci vuoto è un
/// risultato legittimo, e restituire `None` lo trasformerebbe in un guasto
/// passeggero da ritentare all'infinito.
#[must_use]
pub fn analizza(grezzo: &str) -> Option<Vec<Candidato>> {
    let radice: serde_json::Value = serde_json::from_str(grezzo).ok()?;
    let voci = match radice.get("entries") {
        Some(serde_json::Value::Array(voci)) => voci.as_slice(),
        // Nessun campo `entries`: è un JSON valido che non è una ricerca. Zero
        // candidati, non un errore.
        _ => return Some(Vec::new()),
    };
    Some(voci.iter().filter_map(voce).collect())
}

/// Una voce del JSON, se ha almeno un indirizzo.
fn voce(voce: &serde_json::Value) -> Option<Candidato> {
    let url = voce
        .get("url")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            voce.get("id")
                .and_then(serde_json::Value::as_str)
                .map(|id| format!("https://www.youtube.com/watch?v={id}"))
        })?;
    if url.is_empty() {
        return None;
    }

    // `channel` è il nome del canale, `uploader` quello di chi ha caricato: per i
    // canali generati dalla distribuzione sono la stessa cosa, ma non sempre
    // entrambi sono presenti, e il secondo è il ripiego del primo.
    let canale = testo(voce, "channel")
        .or_else(|| testo(voce, "uploader"))
        .or_else(|| testo(voce, "channel_id"));

    Some(Candidato {
        url,
        titolo: testo(voce, "title").unwrap_or_default(),
        canale,
        canale_verificato: voce
            .get("channel_is_verified")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        durata_sec: voce
            .get("duration")
            .and_then(serde_json::Value::as_f64)
            .filter(|d| d.is_finite() && *d >= 0.0)
            .map(arrotonda),
    })
}

/// Un campo di testo non vuoto.
fn testo(voce: &serde_json::Value, campo: &str) -> Option<String> {
    voce.get(campo)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// La durata in secondi interi.
///
/// yt-dlp la dà come numero in virgola mobile e a volte con i decimali. Si
/// arrotonda, e si limita: una durata assurda arrivata da un JSON storto non
/// deve diventare un numero negativo passando per `as`.
fn arrotonda(secondi: f64) -> u32 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "il valore è già limitato fra 0 e u32::MAX dalla riga sopra"
    )]
    let intero = secondi.round().clamp(0.0, f64::from(u32::MAX)) as u32;
    intero
}

/// L'annullamento, che non è un guasto.
fn annullamento() -> AppError {
    AppError::new(ErrorCode::InternalAborted {
        what: Some("ricerca su YouTube".to_owned()),
    })
}

/// Lo spawn non è riuscito: il binario c'è ma non parte.
fn errore_di_avvio(binari: &Binari, guasto: &std::io::Error) -> AppError {
    if guasto.kind() == std::io::ErrorKind::NotFound {
        // Il file c'era quando `Binari::risolvi` ha guardato e non c'è più:
        // qualcuno lo ha tolto mentre l'applicazione girava.
        return AppError::new(ErrorCode::DownloadBinaryMissing {
            name: crate::binario::NOME_YTDLP.to_owned(),
            dir: Some(binari.cartella().display().to_string()),
            url: Some(crate::binario::INDIRIZZO_YTDLP.to_owned()),
        });
    }
    // Un eseguibile che c'è e non parte è quasi sempre un pacchetto scompattato
    // a metà o bloccato da un antivirus: guasto d'ambiente, ritentabile.
    AppError::new(ErrorCode::DownloadYtdlpCorrupted).with_cause(guasto.to_string())
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn legge_i_candidati_col_canale() {
        let grezzo = r#"{
            "entries": [
                {
                    "id": "aaa",
                    "url": "https://www.youtube.com/watch?v=aaa",
                    "title": "Karma Police",
                    "duration": 264.0,
                    "channel": "Radiohead - Topic",
                    "channel_is_verified": false
                }
            ]
        }"#;
        let candidati = analizza(grezzo).expect("il JSON è valido");
        let primo = candidati.first().expect("c'è una voce");
        assert_eq!(primo.canale.as_deref(), Some("Radiohead - Topic"));
        assert_eq!(primo.durata_sec, Some(264));
        assert_eq!(primo.titolo, "Karma Police");
    }

    #[test]
    fn lindirizzo_si_ricostruisce_dallidentificativo() {
        // Con `--flat-playlist` alcune versioni di yt-dlp danno solo `id`.
        let grezzo = r#"{"entries":[{"id":"xyz","title":"T"}]}"#;
        let candidati = analizza(grezzo).expect("il JSON è valido");
        assert_eq!(
            candidati.first().map(|c| c.url.as_str()),
            Some("https://www.youtube.com/watch?v=xyz")
        );
    }

    #[test]
    fn una_voce_senza_indirizzo_si_butta() {
        // Un candidato che non si può scaricare non è un candidato: tenerlo
        // vorrebbe dire poterlo scegliere e poi fallire.
        let grezzo = r#"{"entries":[{"title":"senza indirizzo"},{"id":"ok"}]}"#;
        let candidati = analizza(grezzo).expect("il JSON è valido");
        assert_eq!(candidati.len(), 1);
    }

    #[test]
    fn uploader_fa_da_ripiego_al_canale() {
        let grezzo = r#"{"entries":[{"id":"a","uploader":"BjörkVEVO"}]}"#;
        let candidati = analizza(grezzo).expect("il JSON è valido");
        assert_eq!(
            candidati.first().and_then(|c| c.canale.as_deref()),
            Some("BjörkVEVO")
        );
    }

    #[test]
    fn zero_risultati_non_e_un_errore_di_lettura() {
        // La distinzione su cui si regge tutto: `Some(vec![])` è terminale,
        // `None` sarebbe passeggero.
        assert_eq!(analizza(r#"{"entries":[]}"#), Some(Vec::new()));
        assert_eq!(analizza(r#"{"_type":"video","id":"a"}"#), Some(Vec::new()));
    }

    #[test]
    fn un_json_rotto_e_un_errore_di_lettura() {
        assert_eq!(analizza("non sono json"), None);
        assert_eq!(analizza(""), None);
    }

    #[test]
    fn una_durata_assente_o_assurda_diventa_nessuna_durata() {
        let grezzo = r#"{"entries":[
            {"id":"a","duration":null},
            {"id":"b"},
            {"id":"c","duration":-5},
            {"id":"d","duration":123.6}
        ]}"#;
        let candidati = analizza(grezzo).expect("il JSON è valido");
        assert_eq!(candidati.first().and_then(|c| c.durata_sec), None);
        assert_eq!(candidati.get(1).and_then(|c| c.durata_sec), None);
        assert_eq!(candidati.get(2).and_then(|c| c.durata_sec), None);
        // Arrotondata, non troncata: 123.6 secondi sono 124, non 123.
        assert_eq!(candidati.get(3).and_then(|c| c.durata_sec), Some(124));
    }

    #[test]
    fn senza_binario_la_ricerca_fallisce_dicendo_quale() {
        let binari = Binari::risolvi(std::path::Path::new("C:/non/esiste/proprio"));
        let errore = cerca_query(&binari, "qualcosa", &|| false)
            .expect_err("senza yt-dlp non si può cercare");
        assert!(matches!(
            errore.code(),
            ErrorCode::DownloadBinaryMissing { .. }
        ));
    }

    #[test]
    fn annullare_prima_di_partire_non_avvia_niente() {
        let binari = Binari::risolvi(std::path::Path::new("C:/non/esiste/proprio"));
        let errore = cerca_query(&binari, "qualcosa", &|| true).expect_err("annullato è un errore");
        // E non è `DownloadBinaryMissing`: l'annullamento viene prima, o
        // l'utente vedrebbe un guasto che non c'è per aver premuto «Annulla».
        assert!(matches!(errore.code(), ErrorCode::InternalAborted { .. }));
        assert!(!errore.is_retryable());
    }
}
