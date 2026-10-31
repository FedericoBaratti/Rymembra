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

use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::esterno::Licenza;
use rymembra_domain::indirizzo::estensione_da_url;
use rymembra_domain::scelta::Candidato;
use rymembra_net::Rete;

/// Dove va a finire quel che si preleva.
#[derive(Debug, Clone, Copy)]
pub struct Richiesta<'a> {
    /// La cartella in cui il file deve restare.
    pub cartella_download: &'a Path,
    /// Dove si scrive mentre arriva. Deve stare fuori dalle cartelle
    /// sorvegliate: vedi il preambolo del modulo.
    pub cartella_temporanea: &'a Path,
    /// Il percorso relativo senza estensione, con le barre in avanti — quello
    /// che [`rymembra_domain::destinazione::Destinazione::relativo`] produce.
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

    // La cartella dei download deve esserci **prima** di chiedere un byte. Su
    // un NAS spento o un disco esterno staccato il prelievo arriverebbe intero
    // e fallirebbe solo al deposito; e siccome `fs.writeFailed` si ritenta, la
    // coda lo riscaricherebbe intero altre due volte. Chiederlo qui costa una
    // domanda al disco e risparmia tutta la banda.
    //
    // La radice e non la cartella del brano: quella si crea al deposito, a file
    // arrivato. Crearla qui lascerebbe una cartella d'album vuota, dentro la
    // cartella sorvegliata, a ogni brano che poi non arriva — e un brano sparito
    // dal catalogo non arriva mai.
    std::fs::create_dir_all(richiesta.cartella_download).map_err(|err| {
        fs_errore(
            "non si raggiunge la cartella dei download",
            richiesta.cartella_download,
            &err,
        )
    })?;

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
            // questo il guasto da riportare. Lo stesso vale per un file
            // arrivato intero che non si riesce a depositare: ci pensa
            // [`deposita`]. Quel che resta dopo una chiusura forzata lo spazza
            // [`spazza_parziali`].
            let _ = std::fs::remove_file(&temporaneo);
            return Err(err);
        }
    };

    let definitivo = deposita(&temporaneo, richiesta, &estensione)?;

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

/// Porta il file arrivato al suo posto definitivo, o lo cancella.
///
/// # Perché cancella su ogni errore
///
/// Perché un deposito fallito si ritenta: `fs.writeFailed` è ritentabile, e la
/// coda rimette il brano in attesa e lo riscarica da capo. Un temporaneo
/// lasciato qui a ogni giro sarebbe un brano intero in più nella cartella dati
/// per ogni tentativo, e nessuno lo rileggerebbe mai: il giro dopo ne scrive
/// uno nuovo, con un altro nome.
///
/// # Perché il nome si sceglie qui e non prima del prelievo
///
/// Perché fra la scelta e lo spostamento passa tutto il trasferimento, e su
/// Windows `rename` e `copy` sovrascrivono un file già presente: un brano con
/// lo stesso nome arrivato nel frattempo andrebbe perso in silenzio, che è
/// proprio quel che [`posto_libero`] esiste per evitare.
fn deposita(
    temporaneo: &Path,
    richiesta: &Richiesta<'_>,
    estensione: &str,
) -> Result<PathBuf, AppError> {
    let esito = posto_libero(
        richiesta.cartella_download,
        richiesta.base_relativa,
        estensione,
    )
    .and_then(|definitivo| {
        if let Some(genitore) = definitivo.parent() {
            std::fs::create_dir_all(genitore).map_err(|err| {
                fs_errore(
                    "non si riesce a creare la cartella del brano",
                    genitore,
                    &err,
                )
            })?;
        }
        sposta(temporaneo, &definitivo)?;
        Ok(definitivo)
    });
    if esito.is_err() {
        // Come per un trasferimento caduto: se anche la cancellazione fallisce
        // non è questo il guasto da riportare.
        let _ = std::fs::remove_file(temporaneo);
    }
    esito
}

/// Sposta il file, anche fra volumi diversi.
///
/// `rename` fallisce quando la cartella temporanea e quella dei download stanno
/// su due dischi — che è il caso normale di chi tiene la musica su un disco
/// esterno — e il messaggio che ne esce parla di un errore di sistema invece che
/// di due dischi. Il ripiego copia e cancella.
///
/// # La copia non va sul nome finale
///
/// Va su un `….parziale` accanto, e poi si rinomina: quella sì è sullo stesso
/// volume, quindi atomica. Copiare direttamente sul nome finale vorrebbe dire
/// che una copia interrotta — disco pieno, chiavetta staccata — lascia dentro
/// la cartella sorvegliata proprio il brano a metà che il preambolo del modulo
/// vuole tenere fuori. Un `….parziale` la scansione non lo legge (non è
/// un'estensione audio), e se la copia fallisce si cancella.
fn sposta(da: &Path, a: &Path) -> Result<(), AppError> {
    if std::fs::rename(da, a).is_ok() {
        return Ok(());
    }
    let mut accanto = a.as_os_str().to_owned();
    accanto.push(".parziale");
    let accanto = PathBuf::from(accanto);
    if let Err(err) = std::fs::copy(da, &accanto).and_then(|_| std::fs::rename(&accanto, a)) {
        let _ = std::fs::remove_file(&accanto);
        return Err(fs_errore("non si riesce a spostare il brano", a, &err));
    }
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

/// Cancella i file temporanei rimasti in `cartella_temporanea`, e dice quanti.
///
/// # Perché serve, se ogni errore cancella già il suo
///
/// Perché c'è un caso in cui nessuno arriva a cancellare: la chiusura forzata a
/// metà prelievo — il processo terminato, il computer spento. Il file a metà
/// resta nella cartella dati con un nome che nessun prelievo sceglierà più, e
/// senza questa pulizia ci resta per sempre.
///
/// # Quando si chiama
///
/// All'inizio di una passata della coda, e **solo** quando nessun prelievo è in
/// corso: tre fili prelevano insieme, e chiamarla in mezzo cancellerebbe il
/// file che un altro sta scrivendo. Per questo non la chiama [`preleva`].
///
/// Tocca solo i file con il nome di [`nome_temporaneo`] — `rymembra-….parziale`
/// — e non entra nelle sottocartelle: la cartella è di Rymembra, ma quel che
/// non ha scritto lui non è suo da cancellare. Una cartella che non c'è non è un
/// guasto: è la prima passata.
pub fn spazza_parziali(cartella_temporanea: &Path) -> usize {
    let Ok(voci) = std::fs::read_dir(cartella_temporanea) else {
        return 0;
    };
    let mut tolti = 0_usize;
    for voce in voci.filter_map(Result::ok) {
        let nome = voce.file_name();
        let nome = nome.to_string_lossy();
        let e_nostro = nome.starts_with("rymembra-") && nome.ends_with(".parziale");
        let e_un_file = voce.file_type().is_ok_and(|tipo| tipo.is_file());
        if e_nostro && e_un_file && std::fs::remove_file(voce.path()).is_ok() {
            tolti = tolti.saturating_add(1);
        }
    }
    tolti
}

/// Un nome che nessun altro filo può avere scelto.
fn nome_temporaneo(estensione: &str) -> String {
    let n = PROGRESSIVO.fetch_add(1, Ordering::Relaxed);
    let quando = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("rymembra-{quando:x}-{n:x}.{estensione}.parziale")
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
    use rymembra_domain::esterno::{Disponibilita, Fonte};

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
        let temporanea = std::env::temp_dir().join("rymembra-prova-che-non-esiste");
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
        let radice = std::env::temp_dir().join(format!("rymembra-posto-{}", nome_temporaneo("x")));
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
        let radice = std::env::temp_dir().join(format!("rymembra-punto-{}", nome_temporaneo("x")));
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

    /// I file temporanei di Rymembra rimasti in una cartella.
    fn parziali_in(cartella: &Path) -> Vec<String> {
        std::fs::read_dir(cartella)
            .map(|voci| {
                voci.filter_map(Result::ok)
                    .map(|voce| voce.file_name().to_string_lossy().into_owned())
                    .filter(|nome| nome.ends_with(".parziale"))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Un servitore HTTP su questa macchina che risponde sempre con lo stesso
    /// file, e conta quante richieste ha ricevuto.
    ///
    /// La testa della richiesta si legge **tutta** prima di rispondere: su
    /// Windows un socket chiuso con byte ancora da leggere manda un RST, e il
    /// client potrebbe perdere la risposta.
    fn servitore(corpo: &'static [u8]) -> (String, std::sync::Arc<AtomicU64>) {
        use std::io::Read as _;
        let ascolto =
            std::net::TcpListener::bind("127.0.0.1:0").expect("una porta su questa macchina");
        let porta = ascolto.local_addr().expect("la porta scelta").port();
        let contate = std::sync::Arc::new(AtomicU64::new(0));
        let conto = std::sync::Arc::clone(&contate);
        std::thread::spawn(move || {
            for connessione in ascolto.incoming() {
                let Ok(mut connessione) = connessione else {
                    continue;
                };
                let mut testa = Vec::new();
                let mut byte = [0_u8; 1];
                while !testa.ends_with(b"\r\n\r\n") {
                    match connessione.read(&mut byte) {
                        Ok(1) => testa.push(byte[0]),
                        _ => break,
                    }
                }
                conto.fetch_add(1, Ordering::SeqCst);
                let _ = connessione.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        corpo.len()
                    )
                    .as_bytes(),
                );
                let _ = connessione.write_all(corpo);
            }
        });
        (format!("http://127.0.0.1:{porta}"), contate)
    }

    /// La cartella dei download irraggiungibile — un NAS spento, un disco
    /// esterno staccato — si scopre **prima** di chiedere un byte, e nella
    /// cartella temporanea non resta niente. Qui la cartella sta sotto un file,
    /// che è il modo di renderla impossibile da creare su qualunque sistema.
    #[test]
    fn una_cartella_dei_download_irraggiungibile_non_costa_un_prelievo() {
        let radice =
            std::env::temp_dir().join(format!("rymembra-irraggiungibile-{}", nome_temporaneo("x")));
        let temporanea = radice.join("procura");
        std::fs::create_dir_all(&radice).expect("la cartella di prova si crea");
        let un_file = radice.join("un-file");
        std::fs::write(&un_file, b"x").expect("il file di prova si scrive");
        let download = un_file.join("Download");

        let (base, contate) = servitore(b"un brano intero");
        let rete = Rete::verso("prova", std::time::Duration::from_secs(5), &base)
            .expect("un indirizzo di casa costruisce");
        let esito = preleva(
            &rete,
            &Candidato {
                url: format!("{base}/t01.flac"),
                ..candidato(Disponibilita::Scaricabile)
            },
            &Richiesta {
                cartella_download: &download,
                cartella_temporanea: &temporanea,
                base_relativa: "A/B/01 - C",
            },
            &|| false,
            &mut |_| {},
        );

        let err = esito.expect_err("la cartella dei download non si crea");
        assert!(
            matches!(err.code(), ErrorCode::FsWriteFailed { .. }),
            "codice sbagliato: {:?}",
            err.code()
        );
        let rimasti = parziali_in(&temporanea);
        let chieste = contate.load(Ordering::SeqCst);
        let _ = std::fs::remove_dir_all(&radice);
        assert!(rimasti.is_empty(), "temporanei rimasti: {rimasti:?}");
        assert_eq!(chieste, 0, "il brano è stato chiesto lo stesso");
    }

    /// Un deposito che fallisce — nomi esauriti, cartella che non si crea,
    /// disco di destinazione pieno — cancella il file arrivato: la passata lo
    /// rimette in coda, e ogni giro ne lascerebbe un altro.
    #[test]
    fn un_deposito_fallito_non_lascia_il_temporaneo() {
        let radice =
            std::env::temp_dir().join(format!("rymembra-deposito-{}", nome_temporaneo("x")));
        let temporanea = radice.join("procura");
        std::fs::create_dir_all(&temporanea).expect("la cartella di prova si crea");
        let temporaneo = temporanea.join(nome_temporaneo("flac"));
        std::fs::write(&temporaneo, b"un brano intero").expect("il temporaneo si scrive");
        let un_file = radice.join("un-file");
        std::fs::write(&un_file, b"x").expect("il file di prova si scrive");

        let esito = deposita(
            &temporaneo,
            &Richiesta {
                cartella_download: &un_file,
                cartella_temporanea: &temporanea,
                base_relativa: "A/B/01 - C",
            },
            "flac",
        );

        let rimasto = temporaneo.exists();
        let _ = std::fs::remove_dir_all(&radice);
        assert!(esito.is_err(), "sotto un file non si deposita niente");
        assert!(
            !rimasto,
            "il temporaneo è rimasto: {}",
            temporaneo.display()
        );
    }

    /// Uno spostamento che fallisce a metà non lascia niente accanto alla
    /// destinazione. Qui fallisce l'ultimo passo: al posto del brano c'è una
    /// cartella, quindi né il primo `rename` né quello dopo la copia passano.
    #[test]
    fn uno_spostamento_fallito_non_lascia_un_parziale_accanto() {
        let radice = std::env::temp_dir().join(format!("rymembra-sposta-{}", nome_temporaneo("x")));
        std::fs::create_dir_all(&radice).expect("la cartella di prova si crea");
        let da = radice.join(nome_temporaneo("flac"));
        std::fs::write(&da, b"un brano intero").expect("il temporaneo si scrive");
        let a = radice.join("Album").join("01 - C.flac");
        std::fs::create_dir_all(&a).expect("al posto del brano c'è una cartella");

        let esito = sposta(&da, &a);
        let accanto = parziali_in(&radice.join("Album"));
        let _ = std::fs::remove_dir_all(&radice);
        assert!(esito.is_err());
        assert!(accanto.is_empty(), "rimasti accanto: {accanto:?}");
    }

    /// La pulizia all'inizio di una passata: via i temporanei di Rymembra, e
    /// nient'altro.
    #[test]
    fn la_coda_spazza_i_parziali_rimasti() {
        let radice = std::env::temp_dir().join(format!("rymembra-spazza-{}", nome_temporaneo("x")));
        std::fs::create_dir_all(radice.join("rymembra-cartella.parziale"))
            .expect("la cartella di prova si crea");
        let rimasto = radice.join(nome_temporaneo("flac"));
        std::fs::write(&rimasto, b"mezzo brano").expect("il temporaneo si scrive");
        let altrui = radice.join("appunti.txt.parziale");
        std::fs::write(&altrui, b"x").expect("il file di prova si scrive");
        let brano = radice.join("rymembra-intero.flac");
        std::fs::write(&brano, b"x").expect("il file di prova si scrive");

        let tolti = spazza_parziali(&radice);
        let esiste = (rimasto.exists(), altrui.exists(), brano.exists());
        let cartella = radice.join("rymembra-cartella.parziale").is_dir();
        let _ = std::fs::remove_dir_all(&radice);
        assert_eq!(tolti, 1);
        assert_eq!(esiste, (false, true, true), "solo il temporaneo va via");
        assert!(cartella, "una cartella non è un temporaneo");
        // Una cartella che non c'è non è un guasto: è la prima passata.
        assert_eq!(spazza_parziali(&radice.join("non-esiste")), 0);
    }
}
