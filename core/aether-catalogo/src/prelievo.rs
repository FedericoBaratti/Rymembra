//! Prendere i byte e metterli dove devono stare.
//!
//! Sostituisce il processo figlio che c'era prima, e la differenza non è di
//! stile: qui il percorso di destinazione lo **decidiamo noi** e lo sappiamo
//! prima di cominciare. Il vecchio scaricamento non poteva — yt-dlp sceglieva
//! l'estensione dopo aver visto i formati disponibili — e per sapere dove fosse
//! finito il file bisognava leggerlo da una riga stampata sullo standard output,
//! con la conseguenza che un cambiamento nel formato di quella riga si
//! manifestava come «scaricamento fallito» su un file perfettamente scaricato.
//!
//! # Perché si scrive prima altrove
//!
//! La cartella temporanea sta **fuori** dalle cartelle sorvegliate, e la ragione
//! è che una scansione può partire in qualunque momento: un file a metà dentro
//! una cartella sorvegliata entrerebbe in libreria come brano troncato, e ne
//! uscirebbe solo alla scansione dopo. Si scrive fuori, si sposta quando è
//! intero, e lo spostamento è atomico sullo stesso volume.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::esterno::Licenza;
use aether_domain::scelta::Candidato;
use aether_net::Rete;

/// Dove va a finire quel che si preleva.
#[derive(Debug, Clone, Copy)]
pub struct Richiesta<'a> {
    /// La cartella in cui il file deve restare.
    pub cartella_download: &'a Path,
    /// Dove si scrive mentre arriva. Deve stare fuori dalle cartelle
    /// sorvegliate: vedi il preambolo del modulo.
    pub cartella_temporanea: &'a Path,
    /// Il percorso relativo senza estensione, con le barre in avanti — quello
    /// che [`aether_domain::destinazione::Destinazione::relativo`] produce.
    pub base_relativa: &'a str,
}

/// Un file preso, e quel che bisogna ricordarsi di lui.
#[derive(Debug, Clone)]
pub struct Prelevato {
    /// Dove si trova adesso.
    pub percorso: PathBuf,
    /// Sotto che licenza sta.
    ///
    /// Viaggia fino a chi scrive i tag e fino al database, e non per
    /// completezza: è la risposta a «questo file da dove viene e cosa ci posso
    /// fare», e se non la si conserva adesso non la si ricostruisce più.
    pub licenza: Licenza,
    /// La riga di attribuzione da mostrare e da scrivere nei tag.
    pub attribuzione: Option<String>,
    /// Quanti byte sono arrivati.
    pub byte: u64,
}

/// Il contatore che rende unico un nome temporaneo.
///
/// Tre fili prelevano insieme, e due prelievi cominciati nello stesso
/// millisecondo scriverebbero sullo stesso file.
static PROGRESSIVO: AtomicU64 = AtomicU64::new(0);

/// Prende un candidato e lo scrive in libreria.
///
/// # Errori
///
/// `download.notPermitted` se la licenza non consente di tenerne una copia — ed
/// è la prima cosa che si controlla, prima di qualunque richiesta: chiedere i
/// byte di qualcosa che non si può tenere è già di troppo. Poi `net.*` e
/// `download.*` per il trasferimento, `fs.*` per il disco.
pub fn preleva(
    rete: &Rete,
    candidato: &Candidato,
    richiesta: &Richiesta<'_>,
    annullato: &dyn Fn() -> bool,
    avanzamento: &mut dyn FnMut(f32),
) -> Result<Prelevato, AppError> {
    if !candidato.si_puo_tenere() {
        return Err(AppError::new(ErrorCode::DownloadNotPermitted {
            licenza: Some(candidato.licenza.nome()),
        })
        .with_cause(format!(
            "«{}» si può ascoltare ma non tenere",
            candidato.titolo
        )));
    }

    let estensione = candidato
        .estensione
        .clone()
        .or_else(|| estensione_da_url(&candidato.url))
        .unwrap_or_else(|| "mp3".to_owned());

    std::fs::create_dir_all(richiesta.cartella_temporanea).map_err(|err| {
        fs_errore(
            "non si riesce a creare la cartella temporanea",
            richiesta.cartella_temporanea,
            &err,
        )
    })?;

    let temporaneo = richiesta
        .cartella_temporanea
        .join(nome_temporaneo(&estensione));

    let esito = scrivi(rete, &candidato.url, &temporaneo, annullato, avanzamento);
    let byte = match esito {
        Ok(byte) => byte,
        Err(err) => {
            // Un file a metà non serve a nessuno e occupa il disco finché
            // qualcuno non ci pensa. Se anche la cancellazione fallisce non è
            // questo il guasto da riportare.
            let _ = std::fs::remove_file(&temporaneo);
            return Err(err);
        }
    };

    let definitivo = posto_libero(
        richiesta.cartella_download,
        richiesta.base_relativa,
        &estensione,
    )?;
    if let Some(genitore) = definitivo.parent() {
        std::fs::create_dir_all(genitore).map_err(|err| {
            fs_errore(
                "non si riesce a creare la cartella del brano",
                genitore,
                &err,
            )
        })?;
    }
    sposta(&temporaneo, &definitivo)?;

    Ok(Prelevato {
        percorso: definitivo,
        licenza: candidato.licenza.clone(),
        attribuzione: attribuzione(candidato),
        byte,
    })
}

/// La riga da scrivere nei tag del brano.
///
/// Non è cortesia: per certi cataloghi è un obbligo dei termini d'uso, e
/// costruirla qui — accanto a chi conosce la fonte — è ciò che impedisce che si
/// dimentichi in uno dei posti in cui va messa.
///
/// # Perché questa non segue la lingua dell'interfaccia
///
/// Perché non si mostra: si **scrive**, dentro il file, e ci resta. Comporla
/// nella lingua attiva al momento dello scarico vorrebbe dire una libreria con
/// l'attribuzione in tre lingue a seconda di che mese era, e il testo di un tag
/// non si riscrive quando qualcuno cambia lingua nelle impostazioni. Come per
/// i due segnaposti «sconosciuto» di `album.rs`: quel che finisce sul disco è
/// un valore, non un'etichetta. La riga che invece si **mostra**, nell'anteprima
/// di un link, la compone il frontend dal catalogo delle lingue.
#[must_use]
pub fn attribuzione(candidato: &Candidato) -> Option<String> {
    let fonte = candidato.fonte.etichetta();
    let licenza = match &candidato.licenza {
        Licenza::CreativeCommons(codice) => format!(", CC {}", codice.to_uppercase()),
        Licenza::PubblicoDominio => ", pubblico dominio".to_owned(),
        Licenza::LiberaNonCommerciale => ", distribuzione non commerciale".to_owned(),
        Licenza::OpenMusicLicense => ", Open Music License".to_owned(),
        Licenza::TutteRiservate | Licenza::Sconosciuta => String::new(),
    };
    let pagina = candidato
        .pagina
        .as_deref()
        .map(|p| format!(" — {p}"))
        .unwrap_or_default();
    Some(format!("via {fonte}{licenza}{pagina}"))
}

/// Scrive il corpo di una richiesta in un file, riportando l'avanzamento.
fn scrivi(
    rete: &Rete,
    url: &str,
    dove: &Path,
    annullato: &dyn Fn() -> bool,
    avanzamento: &mut dyn FnMut(f32),
) -> Result<u64, AppError> {
    let file = std::fs::File::create(dove)
        .map_err(|err| fs_errore("non si riesce a scrivere il file", dove, &err))?;
    let mut buffer = std::io::BufWriter::new(file);

    let byte = rete.preleva(url, &[], &mut buffer, annullato, &mut |fatti, totale| {
        // Senza un totale dichiarato non c'è una frazione da mostrare, e
        // inventarne una — «più byte, più vicino alla fine» — disegnerebbe
        // una barra che avanza sempre e non arriva mai.
        if let Some(totale) = totale
            && totale > 0
        {
            #[expect(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                reason = "una frazione a schermo non ha bisogno di più di sette cifre"
            )]
            avanzamento((fatti as f64 / totale as f64) as f32);
        }
    })?;

    buffer
        .flush()
        .map_err(|err| fs_errore("il file non si è chiuso bene", dove, &err))?;
    drop(buffer);

    if byte == 0 {
        return Err(AppError::new(ErrorCode::DownloadInvalidFiles)
            .with_cause("il catalogo ha risposto con un file vuoto".to_owned()));
    }
    Ok(byte)
}

/// Sposta il file, anche fra volumi diversi.
///
/// `rename` fallisce quando la cartella temporanea e quella dei download stanno
/// su due dischi — che è il caso normale di chi tiene la musica su un disco
/// esterno — e il messaggio che ne esce parla di un errore di sistema invece che
/// di due dischi. Il ripiego copia e cancella.
fn sposta(da: &Path, a: &Path) -> Result<(), AppError> {
    if std::fs::rename(da, a).is_ok() {
        return Ok(());
    }
    std::fs::copy(da, a).map_err(|err| fs_errore("non si riesce a spostare il brano", a, &err))?;
    // La copia è andata: il brano c'è. Se il temporaneo non si cancella è un
    // fastidio, non un guasto da riportare a chi sta ascoltando.
    let _ = std::fs::remove_file(da);
    Ok(())
}

/// Il primo nome libero per questa destinazione.
///
/// Due brani con lo stesso titolo nello stesso album esistono — le tracce
/// nascoste, i due take — e sovrascrivere il primo col secondo sarebbe una
/// perdita silenziosa.
fn posto_libero(
    cartella: &Path,
    base_relativa: &str,
    estensione: &str,
) -> Result<PathBuf, AppError> {
    // L'estensione si ATTACCA, mai con `with_extension`: quella sostituisce
    // tutto ciò che segue l'ultimo punto del nome, e i punti nei titoli sono
    // comuni — «02 - Mr. Brightside» diventerebbe «02 - Mr.mp3».
    let radice = cartella.join(base_relativa.replace('/', std::path::MAIN_SEPARATOR_STR));
    let candidato = PathBuf::from(format!("{}.{estensione}", radice.display()));
    if !candidato.exists() {
        return Ok(candidato);
    }
    for n in 2_u32..=99 {
        let alternativo = PathBuf::from(format!("{} ({n}).{estensione}", radice.display()));
        if !alternativo.exists() {
            return Ok(alternativo);
        }
    }
    Err(
        AppError::new(ErrorCode::DownloadInvalidFiles).with_cause(format!(
            "esistono già cento file con il nome «{base_relativa}»"
        )),
    )
}

/// Un nome che nessun altro filo può avere scelto.
fn nome_temporaneo(estensione: &str) -> String {
    let n = PROGRESSIVO.fetch_add(1, Ordering::Relaxed);
    let quando = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("aether-{quando:x}-{n:x}.{estensione}.parziale")
}

/// L'estensione ricavata dall'indirizzo, quando il catalogo non la dichiara.
///
/// Si guarda **solo** dentro il percorso, mai nell'host: `archive.org` finisce
/// con un punto e tre lettere esattamente come `t01.mp3`, e senza questa
/// distinzione un indirizzo senza percorso produrrebbe l'estensione `org`.
fn estensione_da_url(url: &str) -> Option<String> {
    let senza_query = url.split(['?', '#']).next().unwrap_or(url);
    let senza_schema = senza_query
        .strip_prefix("https://")
        .or_else(|| senza_query.strip_prefix("http://"))
        .unwrap_or(senza_query);
    let (_, percorso) = senza_schema.split_once('/')?;
    let ultimo = percorso.rsplit('/').next()?;
    let (_, coda) = ultimo.rsplit_once('.')?;
    let pulita = coda.to_ascii_lowercase();
    (!pulita.is_empty() && pulita.len() <= 5 && pulita.chars().all(|c| c.is_ascii_alphanumeric()))
        .then_some(pulita)
}

/// Un guasto del disco, con dentro il percorso che lo ha prodotto.
fn fs_errore(cosa: &str, dove: &Path, err: &std::io::Error) -> AppError {
    AppError::new(ErrorCode::FsWriteFailed {
        path: dove.display().to_string(),
        detail: Some(err.to_string()),
    })
    .with_cause(cosa.to_owned())
}

#[cfg(test)]
mod prove {
    use super::*;
    use aether_domain::esterno::{Disponibilita, Fonte};

    fn candidato(disponibilita: Disponibilita) -> Candidato {
        Candidato {
            url: "https://archive.org/download/x/t01.flac".to_owned(),
            titolo: "Sugaree".to_owned(),
            fonte: Fonte::InternetArchive,
            licenza: Licenza::LiberaNonCommerciale,
            disponibilita,
            estensione: Some("flac".to_owned()),
            pagina: Some("https://archive.org/details/x".to_owned()),
            ..Candidato::default()
        }
    }

    #[test]
    fn quel_che_non_si_puo_tenere_non_si_chiede_nemmeno() {
        // Il controllo sta prima di ogni richiesta: chiedere i byte di qualcosa
        // che non si può tenere è già di troppo. La prova lo verifica proprio
        // così — con una cartella che non esiste e un indirizzo irraggiungibile:
        // se il rifiuto arrivasse dopo, questo test fallirebbe con un errore di
        // rete o di disco invece che con quello giusto.
        let temporanea = std::env::temp_dir().join("aether-prova-che-non-esiste");
        let esito = preleva(
            &Rete::nuova("prova", std::time::Duration::from_millis(1)),
            &candidato(Disponibilita::SoloAscolto),
            &Richiesta {
                cartella_download: &temporanea,
                cartella_temporanea: &temporanea,
                base_relativa: "A/B/01 - C",
            },
            &|| false,
            &mut |_| {},
        );
        let err = esito.expect_err("un brano di solo ascolto non si preleva");
        assert!(
            matches!(err.code(), ErrorCode::DownloadNotPermitted { .. }),
            "codice sbagliato: {:?}",
            err.code()
        );
        assert!(
            !temporanea.exists(),
            "non doveva nemmeno creare la cartella"
        );
    }

    #[test]
    fn lestensione_si_ricava_dallindirizzo_quando_manca() {
        assert_eq!(
            estensione_da_url("https://archive.org/download/x/t01.flac"),
            Some("flac".to_owned())
        );
        assert_eq!(
            estensione_da_url("https://a/b/t01.mp3?token=1"),
            Some("mp3".to_owned())
        );
        assert_eq!(estensione_da_url("https://a/b/senza-estensione"), None);
        // Un dominio non è un'estensione: `archive.org` finisce con un punto e
        // tre lettere esattamente come `t01.mp3`.
        assert_eq!(estensione_da_url("https://archive.org"), None);
        assert_eq!(estensione_da_url("https://archive.org/"), None);
    }

    #[test]
    fn due_nomi_temporanei_di_fila_non_coincidono() {
        assert_ne!(nome_temporaneo("flac"), nome_temporaneo("flac"));
        assert!(nome_temporaneo("mp3").ends_with(".mp3.parziale"));
    }

    #[test]
    fn lattribuzione_dice_fonte_licenza_e_pagina() {
        let a = attribuzione(&candidato(Disponibilita::Scaricabile)).unwrap_or_default();
        assert!(a.contains("Internet Archive"));
        assert!(a.contains("non commerciale"));
        assert!(a.contains("archive.org/details/x"));

        let cc = Candidato {
            licenza: Licenza::CreativeCommons("by-sa".to_owned()),
            ..candidato(Disponibilita::Scaricabile)
        };
        assert!(attribuzione(&cc).unwrap_or_default().contains("CC BY-SA"));
    }

    #[test]
    fn un_nome_gia_preso_non_si_sovrascrive() {
        let radice = std::env::temp_dir().join(format!("aether-posto-{}", nome_temporaneo("x")));
        std::fs::create_dir_all(radice.join("A").join("B")).expect("la cartella di prova si crea");
        let primo = posto_libero(&radice, "A/B/01 - C", "mp3").expect("un posto libero c'è");
        std::fs::write(&primo, b"x").expect("il file di prova si scrive");
        let secondo = posto_libero(&radice, "A/B/01 - C", "mp3").expect("e un secondo pure");
        assert_ne!(primo, secondo);
        assert!(secondo.display().to_string().contains("(2)"));
        let _ = std::fs::remove_dir_all(&radice);
    }

    #[test]
    fn un_punto_nel_titolo_non_mangia_il_nome() {
        // `with_extension` avrebbe fatto di «02 - Mr. Brightside» un
        // «02 - Mr.mp3», e del ramo anti-collisione un ciclo che produce cento
        // volte lo stesso percorso.
        let radice = std::env::temp_dir().join(format!("aether-punto-{}", nome_temporaneo("x")));
        std::fs::create_dir_all(radice.join("A").join("B")).expect("la cartella di prova si crea");
        let primo =
            posto_libero(&radice, "A/B/02 - Mr. Brightside", "mp3").expect("un posto libero c'è");
        assert!(
            primo
                .display()
                .to_string()
                .ends_with("02 - Mr. Brightside.mp3"),
            "nome sbagliato: {}",
            primo.display()
        );
        std::fs::write(&primo, b"x").expect("il file di prova si scrive");
        let secondo =
            posto_libero(&radice, "A/B/02 - Mr. Brightside", "mp3").expect("e un secondo pure");
        assert!(
            secondo
                .display()
                .to_string()
                .ends_with("02 - Mr. Brightside (2).mp3"),
            "alternativo sbagliato: {}",
            secondo.display()
        );
        let _ = std::fs::remove_dir_all(&radice);
    }
}
