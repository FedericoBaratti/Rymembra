//! Scaricare un video di YouTube come file audio.
//!
//! Porto della seconda metà di `downloadSpotifyTrack`
//! (`legacy/Aeter/electron/modules/download/spotifyEngine.ts:176-241`), senza
//! `--extract-audio` e senza ffmpeg: si prende m4a così com'è (vedi
//! [`crate::argomenti`]).
//!
//! # Il giro dei tre tentativi, e perché non è un lusso
//!
//! Un `403 Forbidden` sul primo profilo non vuol dire che il brano non c'è: vuol
//! dire che quel client di YouTube non ce lo dà adesso. Senza il giro, quel 403
//! arriverebbe all'utente come «brano non trovato su YouTube» — una frase falsa
//! che lo manda a cercare a mano una cosa che avremmo preso al secondo colpo.
//!
//! Si passa al profilo successivo **solo** se lo stderr somiglia a un 403 o a un
//! problema di firma ([`crate::argomenti::ritentabile`]). Un video privato
//! resterà privato anche col client dopo, e ritentarlo tre volte è solo tempo
//! dell'utente.
//!
//! # Perché il percorso finale lo dice yt-dlp e non lo calcoliamo noi
//!
//! Con `-P home`/`-P temp` e un modello relativo, il file nasce nella cartella
//! temporanea e ci arriva dopo. L'estensione poi la decide il formato scelto.
//! Ricostruire quel percorso vorrebbe dire riscrivere le regole di yt-dlp e
//! tenerle allineate alle sue versioni; leggerlo dalla sentinella `AETHER_D`
//! (stampata da `after_move`, cioè a file già al suo posto) è un fatto invece di
//! una previsione. Se la sentinella non arriva, lo scaricamento **non** è
//! riuscito, anche se yt-dlp è uscito con zero.

use std::path::{Path, PathBuf};

use aether_domain::{AppError, ErrorCode};

use crate::argomenti::{ARG_RETE, ARG_VELOCITA, TENTATIVI, arg_percorsi, ritentabile};
use crate::binario::Binari;
use crate::processo;
use crate::progresso::{Evento, analizza};

/// Cosa scaricare, e dove metterlo.
#[derive(Debug, Clone, Copy)]
pub struct Richiesta<'a> {
    /// L'indirizzo del video scelto.
    pub url: &'a str,
    /// La cartella sorvegliata in cui il file deve finire.
    pub cartella_download: &'a Path,
    /// Dove tenere i `.part` e i frammenti, **fuori** dalla sorvegliata.
    pub cartella_temporanea: &'a Path,
    /// Il percorso relativo senza estensione, con le barre in avanti — quello
    /// che [`aether_domain::yt_match::Destinazione::relativo`] costruisce.
    pub base_relativa: &'a str,
}

/// Scarica il video, restituendo il percorso del file prodotto.
///
/// `avanzamento` riceve una frazione da 0 a 1 ogni volta che yt-dlp ne annuncia
/// una — spesso, quindi chi chiama deve strozzarla prima di farne un evento.
/// `annullato` viene interrogato di continuo: quando dice sì, il processo figlio
/// viene ucciso.
///
/// # Errori
///
/// Il codice che descrive il guasto, con la ritentabilità presa dal catalogo
/// (vedi [`crate::errori`]). [`ErrorCode::InternalAborted`] se è stato annullato,
/// che non è un guasto e non si ritenta.
pub fn scarica(
    binari: &Binari,
    richiesta: &Richiesta<'_>,
    annullato: &dyn Fn() -> bool,
    avanzamento: &mut dyn FnMut(f32),
) -> Result<PathBuf, AppError> {
    if annullato() {
        return Err(annullamento());
    }
    let ytdlp = binari.richiedi_ytdlp()?;
    prepara_cartelle(richiesta)?;

    let mut ultimo_stderr = String::new();
    let ultimo = TENTATIVI.len().saturating_sub(1);

    for (numero, tentativo) in TENTATIVI.iter().enumerate() {
        if annullato() {
            return Err(annullamento());
        }

        let argomenti = argv(richiesta, tentativo);
        let mut prodotto: Option<String> = None;
        let esito = processo::esegui_a_righe(ytdlp, &argomenti, annullato, &mut |riga| {
            match analizza(riga) {
                // `AETHER_D` porta il percorso completo; `Destinazione` solo il
                // nome, quindi non può servire da percorso.
                Some(Evento::FileFinito { percorso }) => prodotto = Some(percorso),
                Some(Evento::Avanzamento {
                    frazione: Some(frazione),
                    ..
                })
                | Some(Evento::PercentualeVecchia { frazione }) => avanzamento(frazione),
                _ => {}
            }
        })
        .map_err(|e| errore_di_avvio(binari, &e))?;

        if esito.annullato {
            return Err(annullamento());
        }
        if let Some(percorso) = prodotto {
            return Ok(PathBuf::from(percorso));
        }
        ultimo_stderr = esito.stderr;

        if numero < ultimo && ritentabile(&ultimo_stderr) {
            continue;
        }
        break;
    }

    Err(fallimento(&ultimo_stderr))
}

/// Crea la cartella di destinazione e quella temporanea.
fn prepara_cartelle(richiesta: &Richiesta<'_>) -> Result<(), AppError> {
    // yt-dlp le creerebbe da sé, ma un guasto di permessi va scoperto **prima**
    // di aver scaricato dieci megabyte, e con un codice che dice quale cartella.
    let mut destinazione = richiesta.cartella_download.to_path_buf();
    let mut segmenti: Vec<&str> = richiesta.base_relativa.split('/').collect();
    // L'ultimo segmento è il nome del file: crearlo come cartella impedirebbe a
    // yt-dlp di scriverci dentro.
    segmenti.pop();
    for segmento in segmenti {
        destinazione.push(segmento);
    }

    for cartella in [destinazione.as_path(), richiesta.cartella_temporanea] {
        std::fs::create_dir_all(cartella).map_err(|e| {
            AppError::new(ErrorCode::FsWriteFailed {
                path: cartella.display().to_string(),
                detail: Some(e.to_string()),
            })
        })?;
    }
    Ok(())
}

/// L'argv completo per un tentativo.
fn argv(richiesta: &Richiesta<'_>, tentativo: &crate::argomenti::Tentativo) -> Vec<String> {
    let mut argomenti: Vec<String> = vec!["--format".to_owned(), tentativo.formato.to_owned()];
    argomenti.extend(tentativo.estrattore.iter().map(|a| (*a).to_owned()));
    argomenti.extend(ARG_RETE.iter().map(|a| (*a).to_owned()));
    argomenti.extend(ARG_VELOCITA.iter().map(|a| (*a).to_owned()));
    argomenti.extend(
        [
            // Un link di YouTube può portare una playlist appesa: senza questo,
            // un brano solo diventa duecento.
            "--no-playlist",
            "--newline",
            "--no-quiet",
            "--ignore-config",
            "--no-check-certificates",
            "--progress-template",
            "download:AETHER_P:%(progress.downloaded_bytes|NA)s/%(progress.total_bytes,progress.total_bytes_estimate|NA)s",
            "--print",
            "after_move:AETHER_D:%(filepath)s",
            "--output",
        ]
        .iter()
        .map(|a| (*a).to_owned()),
    );
    // Il modello **deve** restare relativo: yt-dlp ignora `-P` con un `--output`
    // assoluto, e gli intermedi finirebbero nella cartella sorvegliata.
    argomenti.push(format!("{}.%(ext)s", richiesta.base_relativa));
    argomenti.extend(arg_percorsi(
        richiesta.cartella_download,
        richiesta.cartella_temporanea,
    ));
    argomenti.push(richiesta.url.to_owned());
    argomenti
}

/// Nessun file prodotto: che errore è.
fn fallimento(stderr: &str) -> AppError {
    if stderr.trim().is_empty() {
        // yt-dlp è uscito senza dire niente e senza produrre niente. Non si può
        // dedurre che il video non ci sia — e dedurlo vorrebbe dire segnarlo
        // introvabile per sempre. Passeggero.
        return AppError::new(ErrorCode::DownloadFailed)
            .with_message("yt-dlp non ha prodotto nessun file né spiegato perché");
    }
    crate::errori::errore(stderr)
}

/// L'annullamento, che non è un guasto.
fn annullamento() -> AppError {
    AppError::new(ErrorCode::InternalAborted {
        what: Some("scaricamento".to_owned()),
    })
}

/// Lo spawn non è riuscito.
fn errore_di_avvio(binari: &Binari, guasto: &std::io::Error) -> AppError {
    if guasto.kind() == std::io::ErrorKind::NotFound {
        return AppError::new(ErrorCode::DownloadBinaryMissing {
            name: crate::binario::NOME_YTDLP.to_owned(),
            dir: Some(binari.cartella().display().to_string()),
            url: Some(crate::binario::INDIRIZZO_YTDLP.to_owned()),
        });
    }
    AppError::new(ErrorCode::DownloadYtdlpCorrupted).with_cause(guasto.to_string())
}

#[cfg(test)]
mod prove {
    use super::*;

    fn richiesta_finta<'a>(base: &'a str, casa: &'a Path, temp: &'a Path) -> Richiesta<'a> {
        Richiesta {
            url: "https://www.youtube.com/watch?v=aaa",
            cartella_download: casa,
            cartella_temporanea: temp,
            base_relativa: base,
        }
    }

    #[test]
    fn il_modello_di_uscita_resta_relativo() {
        // La trappola documentata in `argomenti::arg_percorsi`: con un `--output`
        // assoluto yt-dlp ignora `-P`, e i `.part` finiscono nella cartella
        // sorvegliata — dove la scansione può ingoiarli.
        let casa = Path::new("C:/Musica");
        let temp = Path::new("C:/Temp/aether");
        let r = richiesta_finta("Artista/Album/01 - Titolo", casa, temp);
        let argomenti = argv(&r, &TENTATIVI[0]);

        let posizione = argomenti
            .iter()
            .position(|a| a == "--output")
            .expect("c'è un --output");
        let modello = argomenti
            .get(posizione.saturating_add(1))
            .expect("il --output ha un valore");
        assert_eq!(modello, "Artista/Album/01 - Titolo.%(ext)s");
        assert!(!modello.contains("C:/"), "il modello è diventato assoluto");
    }

    #[test]
    fn largv_chiede_le_sentinelle_che_sappiamo_leggere() {
        // Il contratto fra questo modulo e `progresso`: se qui cambiasse il
        // prefisso, l'avanzamento sparirebbe senza che niente fallisca.
        let casa = Path::new("C:/Musica");
        let temp = Path::new("C:/Temp");
        let argomenti = argv(&richiesta_finta("a/b/c", casa, temp), &TENTATIVI[0]);
        assert!(argomenti.iter().any(|a| a.contains("AETHER_P:")));
        assert!(argomenti.iter().any(|a| a.contains("after_move:AETHER_D:")));
        assert!(argomenti.iter().any(|a| a == "--no-playlist"));
    }

    #[test]
    fn largv_non_chiede_mai_ffmpeg() {
        // Senza ffmpeg impacchettato, `--extract-audio` fallirebbe ogni volta.
        let casa = Path::new("C:/Musica");
        let temp = Path::new("C:/Temp");
        for tentativo in &TENTATIVI {
            let argomenti = argv(&richiesta_finta("a/b/c", casa, temp), tentativo);
            assert!(!argomenti.iter().any(|a| a == "--extract-audio"));
            assert!(!argomenti.iter().any(|a| a == "--embed-thumbnail"));
            assert!(!argomenti.iter().any(|a| a == "--ffmpeg-location"));
        }
    }

    #[test]
    fn lurl_e_lultimo_argomento() {
        // yt-dlp tratta come URL tutto ciò che non è un'opzione: un URL in mezzo
        // agli argomenti funziona per caso finché qualcuno non aggiunge
        // un'opzione con valore subito prima.
        let casa = Path::new("C:/Musica");
        let temp = Path::new("C:/Temp");
        let argomenti = argv(&richiesta_finta("a/b/c", casa, temp), &TENTATIVI[0]);
        assert_eq!(
            argomenti.last().map(String::as_str),
            Some("https://www.youtube.com/watch?v=aaa")
        );
    }

    #[test]
    fn prepara_solo_le_cartelle_non_il_file() {
        let radice = std::env::temp_dir().join("aether-prova-scarica");
        drop(std::fs::remove_dir_all(&radice));
        let casa = radice.join("musica");
        let temp = radice.join("temp");

        prepara_cartelle(&richiesta_finta("Artista/Album/01 - Titolo", &casa, &temp))
            .expect("le cartelle si creano");

        assert!(casa.join("Artista").join("Album").is_dir());
        assert!(temp.is_dir());
        // Il nome base è un file, non una cartella: crearlo come cartella
        // impedirebbe a yt-dlp di scriverci il file.
        assert!(
            !casa
                .join("Artista")
                .join("Album")
                .join("01 - Titolo")
                .exists()
        );

        drop(std::fs::remove_dir_all(&radice));
    }

    #[test]
    fn annullare_prima_di_partire_non_avvia_niente() {
        let binari = Binari::risolvi(Path::new("C:/non/esiste"));
        let casa = Path::new("C:/Musica");
        let temp = Path::new("C:/Temp");
        let errore = scarica(
            &binari,
            &richiesta_finta("a/b/c", casa, temp),
            &|| true,
            &mut |_| {},
        )
        .expect_err("annullato è un errore");
        assert!(matches!(errore.code(), ErrorCode::InternalAborted { .. }));
        assert!(!errore.is_retryable());
    }

    #[test]
    fn uno_stderr_muto_resta_passeggero() {
        // Il caso che nel vecchio albero rischiava di diventare «non trovato»:
        // senza prove che il video sia sparito, si ritenta.
        let e = fallimento("");
        assert_eq!(*e.code(), ErrorCode::DownloadFailed);
        assert!(e.is_retryable());
    }

    #[test]
    fn uno_stderr_che_dice_sparito_e_terminale() {
        let e = fallimento("ERROR: Video unavailable");
        assert!(!e.is_retryable());
    }
}
