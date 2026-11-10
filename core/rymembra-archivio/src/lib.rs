//! L'archivio che Spotify manda su richiesta, ridotto a un `AccountSnapshot`.
//!
//! # Perché questa strada esiste, accanto a quella con la chiave
//!
//! Perché è l'unica che dà la **cronologia vera**. La Web API, anche dopo un
//! consenso OAuth, di ascolti passati ne restituisce cinquanta e non uno di più:
//! per chi ascolta molto è l'ultima mezza giornata. L'archivio porta tutto,
//! dall'apertura dell'account.
//!
//! E perché non chiede niente a nessuno. Dal febbraio 2026 un'applicazione in
//! Development Mode richiede che il proprietario abbia un abbonamento Premium
//! attivo, e smette di funzionare il giorno in cui scade — senza avvisare.
//! Un file zip su un disco non scade.
//!
//! # Cosa NON c'è qui dentro
//!
//! `rusqlite`. Questo crate legge un file e restituisce un valore, esattamente
//! come `rymembra-meta` e `rymembra-catalogo` fanno con la rete. È la regola scritta
//! nel `Cargo.toml` di `rymembra-app`: chi parla col mondo non vede il database, e
//! così «nessun lucchetto della libreria resta preso mentre si scompatta un
//! archivio da trecento megabyte» non è un commento che qualcuno violerà per
//! comodità — è una cosa che non si può scrivere.
//!
//! # Il riconoscimento è per forma, non per nome
//!
//! Spotify ha rinominato questi file almeno tre volte: `StreamingHistory0.json`
//! è diventato `StreamingHistory_music_0.json`, la cronologia estesa è nata come
//! `endsong_0.json` e adesso si chiama `Streaming_History_Audio_2015-2016_0.json`.
//! Un lettore che pretende un nome esatto è un lettore che si rompe da solo al
//! prossimo cambio, e si rompe **in silenzio**: dice «archivio vuoto» invece di
//! «non riconosco questo file».
//!
//! Da qui due difese. I nomi si riconoscono per **prefisso sul nome base**, così
//! la numerazione e le cartelle non contano; e dentro la cronologia il formato
//! si riconosce **dalla riga**, non dal file che la contiene.
//!
//! # Niente si ferma per un file storto
//!
//! Un JSON malformato in mezzo a quaranta finisce in [`Lettura::illeggibili`]
//! con il suo nome, e gli altri trentanove entrano lo stesso. È la stessa regola
//! di `ScanReport.unreadable` per i file musicali: un archivio è grande, e
//! rifiutarlo tutto per una riga vuol dire non importare niente.

pub mod cronologia;
pub mod libreria;
pub mod playlist;

use std::io::Read as _;
use std::path::Path;

use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::spotify_account::{AccountSnapshot, Provenienza};

/// Il tetto a un singolo file dentro l'archivio, scompattato.
///
/// Duecento megabyte. La cronologia estesa di chi ascolta da dieci anni arriva a
/// qualche decina, quindi c'è margine largo; quel che il tetto impedisce è una
/// **zip bomb** — un archivio da due chilobyte che si scompatta in cinque
/// gigabyte e riempie la memoria prima che qualcuno possa fermarlo. Non è
/// paranoia astratta: questo file l'utente lo ha scaricato da un link ricevuto
/// per posta, che è esattamente il canale su cui arrivano le cose sbagliate.
const BYTE_MASSIMI_PER_FILE: u64 = 200 * 1024 * 1024;

/// Un file dell'archivio che non si è potuto leggere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Illeggibile {
    /// Come si chiamava.
    pub nome: String,
    /// Perché no.
    pub perche: String,
}

/// Quel che si è tirato fuori da un archivio, e quel che no.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lettura {
    /// L'account, per quel che questo archivio ne conteneva.
    pub snapshot: AccountSnapshot,
    /// I file che sono stati letti, per nome.
    pub letti: Vec<String>,
    /// I file riconosciuti come «non per noi», per nome.
    ///
    /// Dichiarati e non taciuti: «ho letto 4 file e ne ho ignorati 6» permette a
    /// chi ha scaricato l'archivio sbagliato di capirlo subito. Fra i due che
    /// Spotify manda — i dati dell'account e la cronologia estesa — il silenzio
    /// li renderebbe indistinguibili.
    pub ignorati: Vec<String>,
    /// I file che erano per noi e non si sono letti.
    pub illeggibili: Vec<Illeggibile>,
    /// Quante righe erano podcast, in tutto l'archivio.
    pub podcast: usize,
    /// Quante righe erano per noi e non si sono capite.
    pub righe_illeggibili: usize,
    /// Quel che c'era in libreria e non entra: al bando, e «altro».
    pub non_musica: libreria::NonMusica,
}

impl Lettura {
    /// C'era qualcosa da leggere?
    #[must_use]
    pub fn ha_trovato_qualcosa(&self) -> bool {
        !self.letti.is_empty()
    }
}

/// Apre un archivio di Spotify e ne tira fuori quel che riguarda la musica.
///
/// Funziona su entrambi gli archivi che Spotify manda — quello dei dati
/// dell'account e quello della cronologia estesa — e su una cartella già
/// scompattata non funziona: vuole lo zip come è arrivato.
///
/// # Errori
///
/// - `spotify.archiveUnreadable` se il file non è uno zip, o è troncato; o se
///   ha più voci, o una volta aperto più byte, di quanti un archivio di
///   Spotify ne abbia mai (vedi [`Tetti`]). Un file oltre il suo tetto, o con
///   più valori JSON di quanti se ne aprono, finisce invece fra gli
///   illeggibili e gli altri entrano lo stesso.
/// - `spotify.archiveEmpty` se lo zip si apre e non contiene niente di
///   riconoscibile. Porta l'elenco di quel che c'era: senza, chi ha scaricato
///   l'archivio sbagliato non ha modo di sapere quale dei due era quello giusto.
pub fn leggi(percorso: &Path) -> Result<Lettura, AppError> {
    leggi_con(percorso, &TETTI)
}

/// I tetti con cui si legge un archivio che l'utente ha scaricato da un link.
///
/// Il tetto per file da solo non bastava, per due ragioni misurate: le voci
/// non avevano un numero massimo né un totale, quindi ogni voce pagava di nuovo
/// il suo tetto; e il tetto stava sui byte di **testo**, mentre la memoria che
/// conta è quella dell'albero `serde_json::Value` che ne nasce — `[0,0,…]` costa
/// quaranta volte il testo, cioè gigabyte da 185 KB di zip.
#[derive(Debug, Clone, Copy)]
struct Tetti {
    /// Quante voci può avere l'archivio. Spotify ne manda qualche decina.
    voci: usize,
    /// Quanti byte aperti per un file: [`BYTE_MASSIMI_PER_FILE`].
    per_file: u64,
    /// Quanti byte aperti in tutto l'archivio: un gibibyte, il triplo della
    /// cronologia estesa più lunga che si sia vista.
    totale: u64,
    /// Quanti valori JSON per file, contati prima di costruire l'albero (vedi
    /// [`valori_al_piu`]): quattro milioni sono qualche centinaio di megabyte
    /// nel caso peggiore, e una cronologia estesa da quindici megabyte ne fa
    /// mezzo milione.
    valori: usize,
}

/// I tetti dell'app. Le prove ne usano di più piccoli.
const TETTI: Tetti = Tetti {
    voci: 10_000,
    per_file: BYTE_MASSIMI_PER_FILE,
    totale: 1024 * 1024 * 1024,
    valori: 4_000_000,
};

/// [`leggi`], con dei tetti dati.
fn leggi_con(percorso: &Path, tetti: &Tetti) -> Result<Lettura, AppError> {
    let illeggibile = |detail: String| {
        AppError::new(ErrorCode::SpotifyArchiveUnreadable {
            path: percorso.display().to_string(),
            detail: Some(detail),
        })
    };
    let file = std::fs::File::open(percorso).map_err(|err| {
        AppError::new(ErrorCode::SpotifyArchiveUnreadable {
            path: percorso.display().to_string(),
            detail: Some(err.to_string()),
        })
    })?;
    let mut zip = zip::ZipArchive::new(file).map_err(|err| {
        AppError::new(ErrorCode::SpotifyArchiveUnreadable {
            path: percorso.display().to_string(),
            detail: Some(err.to_string()),
        })
    })?;

    let mut lettura = Lettura {
        snapshot: AccountSnapshot::vuoto(Provenienza::Archivio),
        letti: Vec::new(),
        ignorati: Vec::new(),
        illeggibili: Vec::new(),
        podcast: 0,
        righe_illeggibili: 0,
        non_musica: libreria::NonMusica::default(),
    };
    let mut playlist = playlist::Lette::default();
    let mut ascolti = cronologia::Letti::default();

    if zip.len() > tetti.voci {
        return Err(illeggibile(format!(
            "l'archivio ha {} voci, oltre le {} che si accettano",
            zip.len(),
            tetti.voci
        )));
    }
    let mut restanti = tetti.totale;
    for indice in 0..zip.len() {
        let (nome, corpo) = match voce(&mut zip, indice, tetti, &mut restanti) {
            Letta::Voce(nome, corpo) => (nome, corpo),
            Letta::Illeggibile(guasto) => {
                lettura.illeggibili.push(guasto);
                continue;
            }
            // Oltre il budget non si salta la voce: si rifiuta l'archivio.
            // Un archivio vero ci sta largo, e uno che ci arriva non è uno
            // da cui importare una parte a caso.
            Letta::OltreIlBudget => {
                return Err(illeggibile(format!(
                    "aperto, l'archivio supera i {} byte che si accettano in tutto",
                    tetti.totale
                )));
            }
        };

        // Una voce di cartella non è un file: passarla dal riconoscimento del
        // nome la farebbe finire fra gli illeggibili con «non è JSON valido»,
        // che è un guasto inventato.
        if nome.ends_with('/') {
            lettura.ignorati.push(nome);
            continue;
        }

        let Some(genere) = Genere::dal_nome(&nome) else {
            lettura.ignorati.push(nome);
            continue;
        };

        let corpo = match corpo {
            Some(corpo) => corpo,
            None => {
                lettura.illeggibili.push(Illeggibile {
                    nome,
                    perche: "non è JSON valido".to_owned(),
                });
                continue;
            }
        };

        match genere {
            Genere::Playlist => playlist.assorbi(playlist::leggi(&corpo)),
            Genere::Cronologia => ascolti.assorbi(cronologia::leggi(&corpo)),
            Genere::Libreria => {
                let letta = libreria::leggi(&corpo);
                lettura.snapshot.preferiti.extend(letta.preferiti);
                lettura.snapshot.album.extend(letta.album);
                lettura.snapshot.artisti.extend(letta.artisti);
                // Si accumula, come fanno `playlist` e `ascolti`: Spotify ha
                // già numerato altri file, e un secondo `YourLibrary` non deve
                // cancellare i conteggi del primo.
                lettura.non_musica.podcast = lettura
                    .non_musica
                    .podcast
                    .saturating_add(letta.non_musica.podcast);
                lettura.non_musica.al_bando = lettura
                    .non_musica
                    .al_bando
                    .saturating_add(letta.non_musica.al_bando);
                lettura.non_musica.altro = lettura
                    .non_musica
                    .altro
                    .saturating_add(letta.non_musica.altro);
            }
            Genere::Identita => {
                lettura.snapshot.profilo = lettura
                    .snapshot
                    .profilo
                    .take()
                    .or_else(|| stringa(&corpo, "displayName"))
                    .or_else(|| stringa(&corpo, "username"));
                lettura.snapshot.spotify_user_id = lettura
                    .snapshot
                    .spotify_user_id
                    .take()
                    .or_else(|| stringa(&corpo, "username"));
            }
        }
        lettura.letti.push(nome);
    }

    lettura.snapshot.playlist = playlist.playlist;
    lettura.snapshot.cronologia = ascolti.ascolti;
    lettura.podcast = playlist.podcast.saturating_add(ascolti.podcast);
    lettura.righe_illeggibili = playlist.illeggibili.saturating_add(ascolti.illeggibili);

    if !lettura.ha_trovato_qualcosa() {
        return Err(AppError::new(ErrorCode::SpotifyArchiveEmpty {
            trovati: lettura.ignorati,
        }));
    }
    Ok(lettura)
}

/// Com'è andata la lettura di una voce.
enum Letta {
    /// Il nome base, e il JSON se lo è.
    ///
    /// `None` quando la voce è per noi ma non si è parsata: chi chiama decide
    /// se contarla fra gli illeggibili, che dipende dal fatto che il nome sia
    /// riconosciuto o no. Buttarla qui vorrebbe dire lamentarsi anche dei PDF
    /// che Spotify mette nell'archivio per spiegare i campi.
    Voce(String, Option<serde_json::Value>),
    /// Una voce che non si legge, col perché.
    Illeggibile(Illeggibile),
    /// I byte aperti in tutto l'archivio hanno superato [`Tetti::totale`].
    OltreIlBudget,
}

/// Legge una voce dell'archivio, dentro i [`Tetti`].
///
/// `restanti` è quel che resta del budget dei byte aperti su tutto
/// l'archivio, e scende di quel che questa voce ha aperto davvero.
fn voce(
    zip: &mut zip::ZipArchive<std::fs::File>,
    indice: usize,
    tetti: &Tetti,
    restanti: &mut u64,
) -> Letta {
    let mut dentro = match zip.by_index(indice) {
        Ok(dentro) => dentro,
        Err(err) => {
            return Letta::Illeggibile(Illeggibile {
                nome: format!("voce {indice}"),
                perche: err.to_string(),
            });
        }
    };

    // Il nome **base**: gli archivi di Spotify hanno una cartella in cima che si
    // chiama in modi diversi a seconda di quale dei due è, e le voci di cartella
    // finiscono con una barra.
    let intero = dentro.name().to_owned();
    if intero.ends_with('/') {
        return Letta::Voce(intero, None);
    }
    let nome = intero
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&intero)
        .to_owned();

    if !nome.to_ascii_lowercase().ends_with(".json") {
        return Letta::Voce(nome, None);
    }

    // Il tetto si applica al **letto**, non al dichiarato: `size()` viene
    // dall'intestazione dell'archivio, cioè da chi l'ha scritto, e una zip bomb
    // dichiara quel che le pare. Un byte oltre il più piccolo dei due tetti
    // dice quale dei due si è superato, invece di troncare in silenzio.
    // Byte e non testo: un tetto che taglia a metà un carattere non deve
    // diventare «non è UTF-8», e i byte aperti si contano comunque.
    let tetto = tetti.per_file.min(*restanti);
    let mut byte = Vec::new();
    let letto = (&mut dentro)
        .take(tetto.saturating_add(1))
        .read_to_end(&mut byte);
    let aperti = u64::try_from(byte.len()).unwrap_or(u64::MAX);
    *restanti = restanti.saturating_sub(aperti);
    if aperti > tetto {
        if tetto < tetti.per_file {
            return Letta::OltreIlBudget;
        }
        return Letta::Illeggibile(Illeggibile {
            nome,
            perche: format!("aperto supera il tetto di {} byte per file", tetti.per_file),
        });
    }
    let Some(testo) = letto.ok().and_then(|_| String::from_utf8(byte).ok()) else {
        return Letta::Illeggibile(Illeggibile {
            nome,
            perche: "non si è potuto leggere, o non è testo UTF-8".to_owned(),
        });
    };

    // L'albero costa molto più del testo: i valori si contano prima di
    // costruirlo, e oltre il tetto la voce non si apre.
    let valori = valori_al_piu(testo.as_bytes());
    if valori > tetti.valori {
        return Letta::Illeggibile(Illeggibile {
            nome,
            perche: format!(
                "porterebbe fino a {valori} valori, oltre i {} che si accettano per file",
                tetti.valori
            ),
        });
    }
    Letta::Voce(nome, serde_json::from_str(&testo).ok())
}

/// Quanti valori JSON al più stanno in questo testo, senza costruire niente.
///
/// Ogni elemento di un vettore o di un oggetto, tranne il primo, è preceduto
/// da una virgola; i primi sono al più uno per contenitore; più la radice.
/// Quindi virgole + contenitori + 1, contati **fuori** dalle stringhe. Un conto
/// per eccesso, in un passaggio e senza allocare. È lo stesso conto di
/// `rymembra_sync::documento`: due crate che non si vedono, e una funzione di
/// quindici righe che non vale una dipendenza nuova fra i due.
fn valori_al_piu(testo: &[u8]) -> usize {
    let mut valori: usize = 1;
    let mut in_stringa = false;
    let mut dopo_barra = false;
    for &byte in testo {
        if in_stringa {
            if dopo_barra {
                dopo_barra = false;
            } else if byte == b'\\' {
                dopo_barra = true;
            } else if byte == b'"' {
                in_stringa = false;
            }
            continue;
        }
        match byte {
            b'"' => in_stringa = true,
            b',' | b'[' | b'{' => valori = valori.saturating_add(1),
            _ => {}
        }
    }
    valori
}

/// Che cosa Rymembra sa fare di un file dell'archivio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Genere {
    /// `Playlist1.json`, `Playlist2.json`, …
    Playlist,
    /// `YourLibrary.json`.
    Libreria,
    /// Una delle due cronologie.
    Cronologia,
    /// `Identity.json` o `Userdata.json`: solo per sapere di chi è l'account.
    Identita,
}

impl Genere {
    /// Riconosce un file dal suo nome base.
    ///
    /// Per **prefisso** e non per uguaglianza, così la numerazione e gli
    /// intervalli di anni non contano: `Playlist1.json` e `Playlist17.json` sono
    /// la stessa cosa, e `Streaming_History_Audio_2015-2016_0.json` pure.
    fn dal_nome(nome: &str) -> Option<Self> {
        let n = nome.to_ascii_lowercase();
        // L'ordine conta: `streaming_history_audio` comincia anche per
        // `streaming_history`, e i video vanno esclusi **prima** di qualunque
        // regola più larga.
        if n.starts_with("streaming_history_video") {
            return None;
        }
        if n.starts_with("streaming_history_audio")
            || n.starts_with("streaminghistory")
            || n.starts_with("endsong")
        {
            return Some(Self::Cronologia);
        }
        if n.starts_with("playlist") {
            return Some(Self::Playlist);
        }
        if n.starts_with("yourlibrary") {
            return Some(Self::Libreria);
        }
        if n.starts_with("identity") || n.starts_with("userdata") {
            return Some(Self::Identita);
        }
        None
    }
}

/// Un campo di testo non vuoto.
fn stringa(corpo: &serde_json::Value, nome: &str) -> Option<String> {
    corpo
        .get(nome)
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod prove {
    use super::*;
    use std::io::Write as _;

    /// Scrive uno zip con i file dati, e restituisce il percorso.
    ///
    /// Uno zip vero e non un finto: qui si sta provando anche che
    /// `zip::ZipArchive` faccia quel che ci si aspetta con una cartella in cima
    /// e con file che non sono JSON, e un finto proverebbe solo il finto.
    fn archivio(dir: &Path, nome: &str, voci: &[(&str, &str)]) -> std::path::PathBuf {
        let percorso = dir.join(nome);
        let file = std::fs::File::create(&percorso).expect("creazione");
        let mut zip = zip::ZipWriter::new(file);
        let opzioni: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (nome, contenuto) in voci {
            zip.start_file(*nome, opzioni).expect("voce");
            zip.write_all(contenuto.as_bytes()).expect("scrittura");
        }
        zip.finish().expect("chiusura");
        percorso
    }

    #[test]
    fn l_archivio_dei_dati_dell_account_si_legge_tutto() {
        let dir = tempfile::tempdir().expect("cartella");
        let percorso = archivio(
            dir.path(),
            "my_spotify_data.zip",
            &[
                // La cartella in cima: gli archivi veri ce l'hanno, e il nome
                // cambia fra i due.
                (
                    "Spotify Account Data/Playlist1.json",
                    r#"{"playlists":[{"name":"Corsa","items":[
                        {"track":{"trackName":"Song 2","artistName":"Blur","albumName":"Blur"}}]}]}"#,
                ),
                (
                    "Spotify Account Data/YourLibrary.json",
                    r#"{"tracks":[{"artist":"Blur","album":"Blur","track":"Song 2"}],
                        "artists":[{"name":"Blur"}],"shows":[{"name":"P"}]}"#,
                ),
                (
                    "Spotify Account Data/StreamingHistory_music_0.json",
                    r#"[{"endTime":"2023-01-01 12:00","artistName":"Blur",
                         "trackName":"Song 2","msPlayed":197000}]"#,
                ),
                (
                    "Spotify Account Data/Identity.json",
                    r#"{"displayName":"Tizio"}"#,
                ),
                // Quel che non ci riguarda.
                ("Spotify Account Data/Marquee.json", r#"[]"#),
                ("Spotify Account Data/ReadMeFirst.pdf", "non un json"),
            ],
        );

        let lettura = leggi(&percorso).expect("si legge");
        assert_eq!(lettura.snapshot.provenienza, Provenienza::Archivio);
        assert_eq!(lettura.snapshot.profilo.as_deref(), Some("Tizio"));
        assert_eq!(lettura.snapshot.playlist.len(), 1);
        assert_eq!(lettura.snapshot.preferiti.len(), 1);
        assert_eq!(lettura.snapshot.artisti.len(), 1);
        assert_eq!(lettura.snapshot.cronologia.len(), 1);
        assert_eq!(lettura.non_musica.podcast, 1);
        assert_eq!(lettura.letti.len(), 4);
        // Dichiarati, non taciuti: è così che chi ha scaricato l'archivio
        // sbagliato se ne accorge.
        assert!(lettura.ignorati.contains(&"Marquee.json".to_owned()));
        assert!(lettura.ignorati.contains(&"ReadMeFirst.pdf".to_owned()));
        assert!(lettura.illeggibili.is_empty(), "{:?}", lettura.illeggibili);
    }

    #[test]
    fn l_archivio_della_cronologia_estesa_si_legge_da_solo() {
        // Arriva separato e settimane dopo. Importarne uno solo deve funzionare.
        let dir = tempfile::tempdir().expect("cartella");
        let percorso = archivio(
            dir.path(),
            "esteso.zip",
            &[
                (
                    "Spotify Extended Streaming History/Streaming_History_Audio_2015-2016_0.json",
                    r#"[{"ts":"2016-05-01T12:00:00Z","ms_played":197000,
                         "master_metadata_track_name":"Song 2",
                         "master_metadata_album_artist_name":"Blur",
                         "master_metadata_album_album_name":"Blur"}]"#,
                ),
                // I video sono podcast filmati: esclusi prima di ogni altra regola.
                (
                    "Spotify Extended Streaming History/Streaming_History_Video_2020.json",
                    r#"[{"ts":"2020-01-01T12:00:00Z","ms_played":1}]"#,
                ),
            ],
        );

        let lettura = leggi(&percorso).expect("si legge");
        assert_eq!(lettura.snapshot.cronologia.len(), 1);
        assert!(
            lettura.snapshot.playlist.is_empty(),
            "stanno nell'altro zip"
        );
        assert!(
            lettura
                .ignorati
                .iter()
                .any(|n| n.starts_with("Streaming_History_Video")),
            "{:?}",
            lettura.ignorati
        );
    }

    #[test]
    fn un_json_rotto_non_ferma_gli_altri() {
        // La stessa regola di `ScanReport.unreadable`: un archivio è grande, e
        // rifiutarlo tutto per un file vuol dire non importare niente.
        let dir = tempfile::tempdir().expect("cartella");
        let percorso = archivio(
            dir.path(),
            "misto.zip",
            &[
                ("Playlist1.json", "{ questo non è json"),
                (
                    "Playlist2.json",
                    r#"{"playlists":[{"name":"Buona","items":[]}]}"#,
                ),
            ],
        );

        let lettura = leggi(&percorso).expect("si legge lo stesso");
        assert_eq!(lettura.snapshot.playlist.len(), 1);
        assert_eq!(lettura.illeggibili.len(), 1);
        assert_eq!(
            lettura.illeggibili.first().map(|i| i.nome.as_str()),
            Some("Playlist1.json"),
            "e lo dice con il nome"
        );
    }

    #[test]
    fn uno_zip_senza_niente_per_noi_dice_cosa_c_era() {
        // È il caso di chi scarica l'archivio sbagliato. Senza l'elenco,
        // «archivio non riconosciuto» non dice quale dei due era quello giusto.
        let dir = tempfile::tempdir().expect("cartella");
        let percorso = archivio(
            dir.path(),
            "altro.zip",
            &[("Payments.json", "[]"), ("Inferences.json", "[]")],
        );

        let err = leggi(&percorso).expect_err("niente da importare");
        assert_eq!(err.code().kind().code(), "spotify.archiveEmpty");
        assert_eq!(
            err.code(),
            &ErrorCode::SpotifyArchiveEmpty {
                trovati: vec!["Payments.json".to_owned(), "Inferences.json".to_owned()]
            }
        );
    }

    #[test]
    fn quel_che_non_e_uno_zip_lo_dice_subito() {
        let dir = tempfile::tempdir().expect("cartella");
        let percorso = dir.path().join("non_uno_zip.zip");
        std::fs::write(&percorso, b"PK ma poi no").expect("scrittura");
        let err = leggi(&percorso).expect_err("non è uno zip");
        assert_eq!(err.code().kind().code(), "spotify.archiveUnreadable");

        let err = leggi(&dir.path().join("non_esiste.zip")).expect_err("non c'è");
        assert_eq!(err.code().kind().code(), "spotify.archiveUnreadable");
    }

    #[test]
    fn un_json_enorme_che_non_e_del_formato_non_si_materializza() {
        // `[0,0,…]` da 10 MB, una decina di KB compressi: come
        // `serde_json::Value` pesa ~400 MB (misurato: 40 volte il testo), e al
        // tetto dei 200 MB per file si arriva a gigabyte.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let enorme = format!("[{}0]", "0,".repeat(5_000_000));
        let percorso = archivio(
            dir.path(),
            "bomba.zip",
            &[
                ("Playlist1.json", enorme.as_str()),
                ("Identity.json", r#"{"username":"u"}"#),
            ],
        );
        drop(enorme);
        let lettura = leggi(&percorso).expect("lettura");
        assert!(
            lettura
                .illeggibili
                .iter()
                .any(|i| i.nome == "Playlist1.json" && i.perche.contains("valori")),
            "letti: {:?}, illeggibili: {:?}",
            lettura.letti,
            lettura.illeggibili
        );
        assert_eq!(lettura.letti, vec!["Identity.json".to_owned()]);
    }

    #[test]
    fn un_archivio_con_troppe_voci_si_rifiuta() {
        // Spotify ne manda qualche decina. Diecimila voci vuote non sono un
        // archivio di Spotify, e ognuna costa una lettura.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let nomi: Vec<String> = (0..=TETTI.voci).map(|n| format!("vuoto{n}.txt")).collect();
        let voci: Vec<(&str, &str)> = nomi.iter().map(|n| (n.as_str(), "")).collect();
        let percorso = archivio(dir.path(), "tante.zip", &voci);
        let err = leggi(&percorso).expect_err("troppe voci");
        assert_eq!(err.code().kind().code(), "spotify.archiveUnreadable");
    }

    #[test]
    fn un_file_oltre_il_tetto_si_dice_invece_di_troncarlo() {
        // Prima il testo si troncava al tetto in silenzio, e il guasto che
        // arrivava era «non è JSON valido»: vero, ma non il perché.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let percorso = archivio(
            dir.path(),
            "lungo.zip",
            &[
                (
                    "Playlist1.json",
                    r#"{"playlists":[{"name":"Lunghissima","items":[]}]}"#,
                ),
                ("Identity.json", r#"{"username":"u"}"#),
            ],
        );
        let tetti = Tetti {
            per_file: 20,
            ..TETTI
        };
        let lettura = leggi_con(&percorso, &tetti).expect("lettura");
        assert!(
            lettura
                .illeggibili
                .iter()
                .any(|i| i.nome == "Playlist1.json" && i.perche.contains("tetto")),
            "{:?}",
            lettura.illeggibili
        );
        assert_eq!(lettura.letti, vec!["Identity.json".to_owned()]);
    }

    #[test]
    fn i_byte_aperti_hanno_un_budget_su_tutto_l_archivio() {
        // Ogni file sotto il suo tetto, e tanti: il conto dei byte **aperti**
        // davvero, su tutto l'archivio, è il solo posto in cui la somma si vede.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let percorso = archivio(
            dir.path(),
            "somma.zip",
            &[
                ("Playlist1.json", r#"{"playlists":[]}"#),
                ("Playlist2.json", r#"{"playlists":[]}"#),
                ("Playlist3.json", r#"{"playlists":[]}"#),
            ],
        );
        let tetti = Tetti {
            totale: 40,
            ..TETTI
        };
        let err = leggi_con(&percorso, &tetti).expect_err("oltre il budget");
        assert_eq!(err.code().kind().code(), "spotify.archiveUnreadable");
        // Con il budget vero si legge tutto.
        assert_eq!(leggi(&percorso).expect("lettura").letti.len(), 3);
    }

    #[test]
    fn i_nomi_si_riconoscono_per_prefisso_e_senza_maiuscole() {
        // Spotify ha rinominato questi file almeno tre volte. Un lettore che
        // pretende un nome esatto si rompe in silenzio al prossimo cambio.
        assert_eq!(Genere::dal_nome("Playlist1.json"), Some(Genere::Playlist));
        assert_eq!(Genere::dal_nome("Playlist17.json"), Some(Genere::Playlist));
        assert_eq!(Genere::dal_nome("YourLibrary.json"), Some(Genere::Libreria));
        assert_eq!(
            Genere::dal_nome("StreamingHistory0.json"),
            Some(Genere::Cronologia),
            "la forma vecchia"
        );
        assert_eq!(
            Genere::dal_nome("StreamingHistory_music_3.json"),
            Some(Genere::Cronologia),
            "la forma di mezzo"
        );
        assert_eq!(
            Genere::dal_nome("Streaming_History_Audio_2015-2016_0.json"),
            Some(Genere::Cronologia),
            "la forma di adesso"
        );
        assert_eq!(
            Genere::dal_nome("endsong_0.json"),
            Some(Genere::Cronologia),
            "come si chiamava la cronologia estesa quando è nata"
        );
        assert_eq!(
            Genere::dal_nome("streaming_history_video_2020.json"),
            None,
            "i video sono esclusi PRIMA della regola più larga"
        );
        assert_eq!(Genere::dal_nome("Marquee.json"), None);
        assert_eq!(Genere::dal_nome("Payments.json"), None);
    }
}
