//! Portare dentro una playlist da un file, e riportarla fuori.
//!
//! # Perché l'abbinamento comincia dal percorso e non dai tag
//!
//! Perché un file di playlist punta a dei **file**, e un file è un'identità più
//! forte di «artista e titolo si somigliano». Se il percorso c'è ed è in
//! libreria, la domanda è chiusa: nessuna scala di somiglianza può fare meglio
//! di un percorso che coincide, e provarci significa solo introdurre modi di
//! sbagliare.
//!
//! La scala di [`aether_domain::abbinamento`] resta, ed è il secondo gradino:
//! serve quando il percorso non porta a niente — la playlist viene da un altro
//! computer, da un altro disco, da una libreria riordinata — e allora quel che
//! resta sono il titolo e l'interprete scritti nel file. È la **stessa** scala
//! dell'importazione da Spotify, non una sua copia: un brano che l'importatore
//! di Spotify sa riconoscere lo deve riconoscere anche questo.
//!
//! # Cosa non fa
//!
//! Non scarica niente. Un brano di un M3U che non è in libreria non finisce in
//! `desiderati`: quel file esisteva sul computer di chi ha scritto la
//! playlist, e cercarlo su YouTube sarebbe una risposta a una domanda che
//! nessuno ha fatto. I mancanti si **dicono**, con il percorso che avevano, che
//! è l'unica cosa utile per andarseli a prendere dove sono davvero.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use aether_domain::BranoEsterno;
use aether_domain::abbinamento::Indice;
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::playlist_file::{FormatoPlaylist, PlaylistLetta, VocePlaylist};
use rusqlite::Connection;

use crate::library::db_error;

/// Cosa ha prodotto — o produrrebbe — l'importazione di un file di playlist.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistFileReport {
    /// Il nome che la playlist ha (o avrebbe) in libreria.
    pub name: String,
    /// Quante voci c'erano nel file.
    pub entries: usize,
    /// Quante sono state ritrovate per **percorso**.
    pub matched_by_path: usize,
    /// Quante sono state ritrovate per titolo e interprete.
    pub matched_by_tags: usize,
    /// I brani che la libreria non ha, con il percorso che avevano nel file.
    pub missing: Vec<VoceMancante>,
    /// Righe del file che non si sono capite.
    pub unreadable: usize,
    /// La playlist creata, quando l'importazione è stata eseguita davvero.
    pub playlist_id: Option<i64>,
    /// La playlist c'era già ed è stata sostituita.
    pub replaced: bool,
}

impl PlaylistFileReport {
    /// Quante voci sono finite in playlist.
    #[must_use]
    pub const fn matched(&self) -> usize {
        self.matched_by_path.saturating_add(self.matched_by_tags)
    }
}

/// Una voce del file che in libreria non c'è.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoceMancante {
    /// Come si chiamava, se il file lo diceva.
    pub title: Option<String>,
    /// Chi la suonava, se il file lo diceva.
    pub artist: Option<String>,
    /// Il percorso scritto nel file: l'unica cosa utile per andarsela a
    /// prendere dov'è davvero.
    pub path: String,
}

/// Prepara l'importazione senza scrivere niente.
///
/// # Errori
///
/// `db.queryFailed` se la libreria non risponde.
pub fn plan(
    connection: &mut Connection,
    letta: &PlaylistLetta,
    nome: &str,
    accanto_a: Option<&Path>,
) -> Result<PlaylistFileReport, AppError> {
    esegui(connection, letta, nome, accanto_a, false)
}

/// Importa davvero.
///
/// # Errori
///
/// `library.playlistNameInvalid` se il nome non identifica niente;
/// `db.queryFailed` se la scrittura fallisce.
pub fn import(
    connection: &mut Connection,
    letta: &PlaylistLetta,
    nome: &str,
    accanto_a: Option<&Path>,
) -> Result<PlaylistFileReport, AppError> {
    esegui(connection, letta, nome, accanto_a, true)
}

/// Il piano **è** l'esecuzione, annullata: la stessa funzione, con la
/// transazione abbandonata invece che chiusa. È la regola di tutto il repo, ed
/// è l'unico modo perché quel che l'utente vede nell'anteprima sia esattamente
/// quel che succederà — comprese le collisioni di nome, che una simulazione
/// scritta a parte non troverebbe.
fn esegui(
    connection: &mut Connection,
    letta: &PlaylistLetta,
    nome: &str,
    accanto_a: Option<&Path>,
    commit: bool,
) -> Result<PlaylistFileReport, AppError> {
    let per_percorso = indice_percorsi(connection)?;
    let libreria = crate::import_esterno::leggi_libreria(connection)?;
    let indice = Indice::nuovo(&libreria);

    let tx = connection
        .transaction()
        .map_err(|err| db_error("apertura dell'importazione di una playlist", &err))?;

    let mut rapporto = PlaylistFileReport {
        name: nome.trim().to_owned(),
        entries: letta.voci.len(),
        matched_by_path: 0,
        matched_by_tags: 0,
        missing: Vec::new(),
        unreadable: letta.illeggibili,
        playlist_id: None,
        replaced: false,
    };

    let mut ordine: Vec<i64> = Vec::with_capacity(letta.voci.len());
    for voce in &letta.voci {
        // Due tentativi sul percorso, in quest'ordine: quello intero e, se non
        // porta a niente, il solo nome del file. Il secondo è il ripiego per le
        // playlist che vengono da un albero diverso — «D:\Backup\pezzo.mp3»
        // contro «C:\Musica\pezzo.mp3» — e non è ambiguo quanto sembra: in
        // `indice_percorsi` un nome che si ripete in libreria non entra due
        // volte, quindi chi è ambiguo semplicemente non risponde e la voce
        // scende al gradino dei tag.
        let intero = chiave_percorso(&voce.percorso, accanto_a);
        let trovato = per_percorso
            .get(&intero)
            .or_else(|| nome_file(&voce.percorso).and_then(|n| per_percorso.get(&n)));
        if let Some(id) = trovato {
            rapporto.matched_by_path += 1;
            ordine.push(*id);
            continue;
        }
        // Il secondo gradino: la stessa scala dell'importazione da Spotify. Un
        // `BranoEsterno` senza identificativi — non viene da Spotify — quindi
        // ISRC e chiave completa cadono da sole e restano artista+titolo, che
        // è esattamente quel che un file di playlist porta.
        let brano = BranoEsterno {
            title: voce.titolo.clone().unwrap_or_default(),
            artist: voce.artista.clone(),
            ..BranoEsterno::default()
        };
        match (!brano.title.is_empty())
            .then(|| indice.abbina(&brano))
            .flatten()
        {
            Some((id, _gradino)) => {
                rapporto.matched_by_tags += 1;
                ordine.push(id);
            }
            None => rapporto.missing.push(VoceMancante {
                title: voce.titolo.clone(),
                artist: voce.artista.clone(),
                path: voce.percorso.clone(),
            }),
        }
    }

    let (id, _creata, sostituita) = crate::import_esterno::prepara_playlist(&tx, nome, None, None)?;
    rapporto.playlist_id = Some(id);
    rapporto.replaced = sostituita;
    crate::playlists::riscrivi_ordine(&tx, id, &ordine)?;

    if commit {
        tx.commit()
            .map_err(|err| db_error("chiusura dell'importazione di una playlist", &err))?;
    } else {
        // Abbandonata di proposito: `Transaction` senza `commit` fa `rollback`
        // quando cade, ed è il motivo per cui `plan` non può mentire.
        rapporto.playlist_id = None;
    }
    Ok(rapporto)
}

/// L'indice dei percorsi della libreria, normalizzati.
///
/// # Perché normalizzati e non confrontati com'è
///
/// Perché lo stesso file si scrive in almeno quattro modi: barre in un verso o
/// nell'altro, maiuscole diverse (su Windows il filesystem non le distingue, e
/// chi ha scritto l'M3U può averle cambiate), e la coda dopo `file:///`. Un
/// confronto letterale fallirebbe su tutti e quattro, e il sintomo sarebbe
/// «nessun brano ritrovato» su una playlist i cui file sono tutti lì.
fn indice_percorsi(connection: &Connection) -> Result<HashMap<String, i64>, AppError> {
    let mut statement = connection
        .prepare("SELECT id, path FROM tracks WHERE path IS NOT NULL")
        .map_err(|err| db_error("indice dei percorsi", &err))?;
    let righe = statement
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|err| db_error("indice dei percorsi", &err))?;

    let mut indice = HashMap::new();
    // I nomi di file a parte, e con il conto: un nome che si ripete in libreria
    // — «01.mp3» ce l'hanno cento album — non deve rispondere a nessuno.
    // Rispondere con il primo trovato vorrebbe dire mettere in playlist una
    // canzone a caso fra cento, in silenzio, e chi guarda vedrebbe una playlist
    // completa con dentro dei brani sbagliati: il difetto peggiore di tutti,
    // perché non somiglia a un difetto.
    let mut per_nome: HashMap<String, Option<i64>> = HashMap::new();
    for riga in righe {
        let (id, percorso) = riga.map_err(|err| db_error("indice dei percorsi", &err))?;
        indice.insert(normalizza(&percorso), id);
        if let Some(nome) = nome_file(&percorso) {
            per_nome
                .entry(nome)
                .and_modify(|posto| *posto = None)
                .or_insert(Some(id));
        }
    }
    for (nome, unico) in per_nome {
        if let Some(id) = unico {
            // `or_insert`: un nome di file che coincide con un percorso intero
            // è un caso che non si dà, ma se si desse il percorso vince.
            indice.entry(nome).or_insert(id);
        }
    }
    Ok(indice)
}

/// La chiave con cui una voce del file si cerca in libreria.
fn chiave_percorso(scritto: &str, accanto_a: Option<&Path>) -> String {
    let ripulito = da_url(scritto);
    let assoluto = Path::new(&ripulito);
    // Un percorso relativo si risolve rispetto alla **cartella del file di
    // playlist**, non a quella di lavoro: è la regola di tutti e tre i formati,
    // e l'unica che rende utile un M3U che sta dentro la cartella dell'album.
    let candidato = match (assoluto.is_absolute(), accanto_a) {
        (false, Some(base)) => base.join(&ripulito),
        _ => PathBuf::from(&ripulito),
    };
    let chiave = normalizza(&candidato.to_string_lossy());
    // Se il percorso composto non è in libreria ci penserà chi chiama, con il
    // ripiego sul nome del file: qui si restituisce comunque la chiave piena,
    // perché è quella giusta quando c'è.
    chiave
}

/// Toglie `file://` e scioglie le sequenze `%NN`.
fn da_url(scritto: &str) -> String {
    let senza_schema = senza_schema(scritto);
    if !senza_schema.contains('%') {
        return senza_schema;
    }
    let byte = senza_schema.as_bytes();
    let mut fuori: Vec<u8> = Vec::with_capacity(byte.len());
    let mut i = 0usize;
    while i < byte.len() {
        let c = byte.get(i).copied().unwrap_or(b'?');
        if c == b'%'
            && let (Some(alto), Some(basso)) = (byte.get(i + 1), byte.get(i + 2))
            && let (Some(a), Some(b)) = (esadecimale(*alto), esadecimale(*basso))
        {
            fuori.push(a.saturating_mul(16).saturating_add(b));
            i = i.saturating_add(3);
            continue;
        }
        fuori.push(c);
        i = i.saturating_add(1);
    }
    String::from_utf8_lossy(&fuori).into_owned()
}

/// Da `file://…` al percorso che il sistema sa aprire.
///
/// # Le quattro forme, e perché sono quattro
///
/// `file://` ha un host fra le due barre e il percorso: `file://HOST/percorso`.
/// Quando l'host è vuoto — `file:///…` — il percorso comincia subito, e le tre
/// barre di fila sono la ragione per cui questa funzione esiste. I programmi che
/// scrivono M3U però non concordano, e le forme che arrivano davvero sono:
///
/// 1. `file:///C:/musica/a.mp3` → `C:/musica/a.mp3`. La barra in testa fa parte
///    della grammatica dell'URL, non del percorso, e lasciarla renderebbe il
///    percorso relativo — per poi incollarlo alla cartella della playlist, cioè
///    a un posto sbagliato.
/// 2. `file://C:/musica/a.mp3` → invariato. Fuori standard ma diffuso: è già un
///    percorso Windows, e anteporgli qualcosa lo trasformerebbe in un nome di
///    server.
/// 3. `file:///home/x` → `/home/x`, e `file:////server/share/x` →
///    `//server/share/x`. Comincia con una barra: è già assoluto, non si tocca.
/// 4. `file://server/share/a.mp3` → `//server/share/a.mp3`. Questa è la forma
///    dello standard con l'host valorizzato, ed è **il caso di rete**: `server`
///    è una macchina, e il percorso sul disco è `\\server\share\a.mp3`. Prima
///    restava `server/share/a.mp3` — un percorso relativo — che
///    [`chiave_percorso`] incollava alla cartella della playlist: la voce non
///    trovava niente e ripiegava sul nome del file, agganciandosi in silenzio a
///    un brano che poteva essere un altro.
///
/// `localhost` come host è il modo lungo di dire «questa macchina»: si toglie e
/// si riapplicano le regole di sopra.
fn senza_schema(scritto: &str) -> String {
    let Some(resto) = scritto.strip_prefix("file://") else {
        return scritto.to_owned();
    };
    let resto = match resto.strip_prefix("localhost") {
        // Solo se `localhost` è l'host **intero**: una macchina che si chiama
        // `localhostdue` è una macchina come le altre.
        Some(dopo) if dopo.is_empty() || dopo.starts_with('/') => dopo,
        _ => resto,
    };
    let byte = resto.as_bytes();
    match (byte.first(), byte.get(1), byte.get(2)) {
        (Some(b'/'), Some(lettera), Some(b':')) if lettera.is_ascii_alphabetic() => {
            resto.get(1..).unwrap_or(resto).to_owned()
        }
        (Some(lettera), Some(b':'), _) if lettera.is_ascii_alphabetic() => resto.to_owned(),
        (Some(b'/'), _, _) => resto.to_owned(),
        (None, _, _) => String::new(),
        _ => format!("//{resto}"),
    }
}

const fn esadecimale(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Barre in un verso solo, niente barra finale, e le maiuscole piegate **solo
/// dove il filesystem le ignora**.
///
/// È la stessa regola — la stessa funzione — con cui la scansione confronta i
/// percorsi: piegare sempre, come si faceva qui, su un filesystem
/// case-sensitive faceva collidere `Song.flac` e `song.flac` nell'indice, e
/// una voce dell'M3U finiva abbinata al brano sbagliato.
fn normalizza(percorso: &str) -> String {
    aether_domain::paths::path_key(
        percorso,
        aether_domain::paths::PathRules::for_current_platform(),
    )
}

/// Il solo nome del file, normalizzato.
fn nome_file(percorso: &str) -> Option<String> {
    let normalizzato = normalizza(percorso);
    let nome = normalizzato.rsplit('/').next()?;
    (!nome.is_empty()).then(|| nome.to_owned())
}

// ── l'esportazione ──────────────────────────────────────────────────────────

/// Trasforma una playlist in un file, senza scriverlo.
///
/// # Percorsi assoluti, sempre
///
/// La tentazione è scriverli relativi alla cartella del file, che è più
/// portabile. La ragione per non farlo è che la libreria di Aether sta in più
/// cartelle sorvegliate, spesso su dischi diversi: un percorso relativo fra due
/// dischi non esiste, e un file di playlist per metà relativo e per metà
/// assoluto è la cosa che i lettori altrui interpretano peggio.
///
/// # Errori
///
/// `library.playlistNotFound` se la playlist non c'è; `db.queryFailed` se la
/// lettura fallisce.
pub fn esporta(
    connection: &Connection,
    playlist_id: i64,
    formato: FormatoPlaylist,
) -> Result<String, AppError> {
    let riepilogo = crate::playlists::list(connection)?
        .into_iter()
        .find(|p| p.id == playlist_id)
        .ok_or_else(|| {
            AppError::new(ErrorCode::LibraryPlaylistNotFound {
                playlist_id: Some(playlist_id),
            })
        })?;
    let brani = crate::playlists::tracks(connection, playlist_id)?;
    // `filter_map` e non `map`: un file di playlist è un elenco di **file**, e
    // un brano di catalogo un file non ce l'ha. Scriverci dentro l'indirizzo
    // del flusso sembrerebbe generoso e sarebbe un errore doppio: altri lettori
    // aprirebbero un indirizzo di Audius senza il nodo davanti — cioè niente —
    // e l'elenco uscirebbe di casa portandosi dietro un indirizzo che i termini
    // di certi cataloghi non vogliono veder ridistribuito.
    let voci: Vec<VocePlaylist> = brani
        .into_iter()
        .filter_map(|b| {
            Some(VocePlaylist {
                percorso: b.path?,
                titolo: Some(b.title),
                artista: Some(b.artist),
                durata_ms: u64::try_from(b.duration_ms).ok(),
            })
        })
        .collect();
    Ok(aether_domain::playlist_file::scrivi(
        &riepilogo.name,
        &voci,
        formato,
    ))
}

#[cfg(test)]
mod prove {
    use super::*;
    use aether_domain::playlist_file::VocePlaylist;

    fn libreria() -> Connection {
        let connection = crate::db::open_in_memory().expect("database").connection;
        connection
            .execute_batch(
                "INSERT INTO tracks
                     (id, path, track_key, title, artist, album, duration_ms,
                      file_size, date_added, date_modified)
                 VALUES (1, 'C:\\Musica\\Bjork\\01 Joga.flac', 'k1', 'Jóga', 'Björk', 'Homogenic', 300000, 1, 1, 1),
                        (2, 'C:\\Musica\\Bjork\\02 Bachelorette.flac', 'k2', 'Bachelorette', 'Björk', 'Homogenic', 300000, 1, 1, 1),
                        (3, 'C:\\Altro\\pezzo.mp3', 'k3', 'Pezzo', 'Tale', 'Raccolta', 200000, 1, 1, 1);",
            )
            .expect("brani");
        connection
    }

    fn voce(percorso: &str, artista: Option<&str>, titolo: Option<&str>) -> VocePlaylist {
        VocePlaylist {
            percorso: percorso.to_owned(),
            titolo: titolo.map(ToOwned::to_owned),
            artista: artista.map(ToOwned::to_owned),
            durata_ms: None,
        }
    }

    #[test]
    fn il_percorso_vince_sui_tag() {
        // Il percorso è un'identità più forte di «si somigliano»: se c'è e
        // porta a un brano in libreria, la domanda è chiusa — anche quando i
        // tag scritti nel file sono di un altro brano, come capita a un M3U
        // esportato prima di un arricchimento.
        let mut c = libreria();
        let letta = PlaylistLetta {
            nome: None,
            voci: vec![voce(
                "C:\\Musica\\Bjork\\01 Joga.flac",
                Some("Sbagliato"),
                Some("Titolo che non esiste"),
            )],
            illeggibili: 0,
        };
        let r = import(&mut c, &letta, "Prova", None).expect("importata");
        assert_eq!(r.matched_by_path, 1);
        assert_eq!(r.matched_by_tags, 0);
        assert!(r.missing.is_empty());
    }

    #[test]
    fn le_barre_e_le_maiuscole_non_contano() {
        let mut c = libreria();
        let letta = PlaylistLetta {
            nome: None,
            voci: vec![voce("c:/MUSICA/bjork/01 joga.flac", None, None)],
            illeggibili: 0,
        };
        let r = plan(&mut c, &letta, "Prova", None).expect("piano");
        assert_eq!(
            r.matched_by_path, 1,
            "lo stesso file scritto in un altro modo"
        );
    }

    #[test]
    fn un_percorso_relativo_si_risolve_accanto_al_file() {
        let mut c = libreria();
        let letta = PlaylistLetta {
            nome: None,
            voci: vec![voce("Bjork/01 Joga.flac", None, None)],
            illeggibili: 0,
        };
        let r = plan(&mut c, &letta, "Prova", Some(Path::new("C:\\Musica"))).expect("piano");
        assert_eq!(r.matched_by_path, 1);
    }

    #[test]
    fn un_file_url_con_le_percentuali_si_scioglie() {
        let mut c = libreria();
        let letta = PlaylistLetta {
            nome: None,
            voci: vec![voce("file:///C:/Musica/Bjork/01%20Joga.flac", None, None)],
            illeggibili: 0,
        };
        let r = plan(&mut c, &letta, "Prova", None).expect("piano");
        assert_eq!(r.matched_by_path, 1);
    }

    #[test]
    fn quando_il_percorso_non_porta_a_niente_valgono_i_tag() {
        // La playlist viene da un altro computer: nessun percorso esiste, e
        // quel che resta sono artista e titolo. È la stessa scala
        // dell'importazione da Spotify.
        let mut c = libreria();
        let letta = PlaylistLetta {
            nome: None,
            voci: vec![
                voce("/home/tizio/musica/x.flac", Some("Björk"), Some("Jóga")),
                voce("/home/tizio/musica/y.flac", Some("Nessuno"), Some("Niente")),
            ],
            illeggibili: 0,
        };
        let r = plan(&mut c, &letta, "Prova", None).expect("piano");
        assert_eq!(r.matched_by_path, 0);
        assert_eq!(r.matched_by_tags, 1);
        assert_eq!(r.missing.len(), 1);
        // Il percorso del mancante si dice: è l'unica cosa utile per andarselo
        // a prendere dov'è davvero.
        assert_eq!(
            r.missing.first().map(|m| m.path.as_str()),
            Some("/home/tizio/musica/y.flac")
        );
    }

    #[test]
    fn il_solo_nome_del_file_e_l_ultimo_ripiego() {
        let mut c = libreria();
        let letta = PlaylistLetta {
            nome: None,
            voci: vec![voce("D:\\Backup\\Vecchio\\pezzo.mp3", None, None)],
            illeggibili: 0,
        };
        let r = plan(&mut c, &letta, "Prova", None).expect("piano");
        assert_eq!(
            r.matched_by_path, 1,
            "stesso nome di file, cartella diversa"
        );
    }

    #[test]
    fn un_nome_di_file_che_si_ripete_non_risponde_a_nessuno() {
        // «01.mp3» ce l'hanno cento album. Rispondere con il primo trovato
        // metterebbe in playlist una canzone a caso fra cento, in silenzio: chi
        // guarda vedrebbe una playlist completa con dentro i brani sbagliati.
        let mut c = libreria();
        c.execute_batch(
            "INSERT INTO tracks (id, path, track_key, title, artist, album,
                                 duration_ms, file_size, date_added, date_modified)
             VALUES (10, 'C:\\Musica\\A\\01.mp3', 'ka', 'A', 'Uno', 'AA', 1000, 1, 1, 1),
                    (11, 'C:\\Musica\\B\\01.mp3', 'kb', 'B', 'Due', 'BB', 1000, 1, 1, 1);",
        )
        .expect("brani");
        let letta = PlaylistLetta {
            nome: None,
            voci: vec![voce("D:\\Altrove\\01.mp3", None, None)],
            illeggibili: 0,
        };
        let r = plan(&mut c, &letta, "Prova", None).expect("piano");
        assert_eq!(r.matched_by_path, 0, "ambiguo: non deve rispondere");
        assert_eq!(r.missing.len(), 1);
    }

    #[test]
    fn il_piano_non_scrive_niente() {
        let mut c = libreria();
        let letta = PlaylistLetta {
            nome: None,
            voci: vec![voce("C:\\Musica\\Bjork\\01 Joga.flac", None, None)],
            illeggibili: 0,
        };
        let r = plan(&mut c, &letta, "Prova", None).expect("piano");
        assert_eq!(r.matched(), 1);
        assert_eq!(r.playlist_id, None, "il piano non crea niente");
        assert!(
            crate::playlists::list(&c).expect("elenco").is_empty(),
            "il database dopo un piano deve essere intatto"
        );
    }

    #[test]
    fn reimportare_sostituisce_invece_di_duplicare() {
        let mut c = libreria();
        let letta = PlaylistLetta {
            nome: None,
            voci: vec![voce("C:\\Musica\\Bjork\\01 Joga.flac", None, None)],
            illeggibili: 0,
        };
        let prima = import(&mut c, &letta, "Serata", None).expect("prima");
        assert!(!prima.replaced);
        let dopo = import(&mut c, &letta, "Serata", None).expect("seconda");
        assert!(dopo.replaced, "la seconda deve sostituire, non affiancare");
        assert_eq!(crate::playlists::list(&c).expect("elenco").len(), 1);
    }

    #[test]
    fn l_esportazione_rilegge_quel_che_ha_scritto() {
        let mut c = libreria();
        let letta = PlaylistLetta {
            nome: None,
            voci: vec![
                voce("C:\\Musica\\Bjork\\01 Joga.flac", None, None),
                voce("C:\\Altro\\pezzo.mp3", None, None),
            ],
            illeggibili: 0,
        };
        let r = import(&mut c, &letta, "Andata", None).expect("importata");
        let id = r.playlist_id.expect("creata");

        for formato in [
            FormatoPlaylist::M3u,
            FormatoPlaylist::Pls,
            FormatoPlaylist::Xspf,
        ] {
            let testo = esporta(&c, id, formato).expect("esportata");
            let riletta = aether_domain::playlist_file::leggi_testo(&testo, formato);
            assert_eq!(riletta.voci.len(), 2, "{}", formato.nome());
            let mut c2 = libreria();
            let ritorno = plan(&mut c2, &riletta, "Ritorno", None).expect("piano");
            assert_eq!(
                ritorno.matched_by_path,
                2,
                "{}: il giro completo deve ritrovare tutti i brani",
                formato.nome()
            );
        }
    }

    // ── i `file://` e le loro quattro forme ─────────────────────────────────

    #[test]
    fn un_file_url_con_host_e_un_percorso_di_rete() {
        // La forma dello standard con l'host valorizzato. Prima diventava
        // `server/share/a.mp3` — un percorso **relativo** — che veniva incollato
        // alla cartella della playlist: la voce non trovava niente e ripiegava
        // sul nome del file, agganciandosi in silenzio a un brano qualunque che
        // si chiamasse così.
        assert_eq!(
            da_url("file://server/share/a.mp3"),
            "//server/share/a.mp3",
            "l'host è una macchina, non una cartella"
        );
        // La scrittura con l'host vuoto e il percorso già UNC arriva alla stessa
        // forma: due grafie, un percorso solo.
        assert_eq!(
            da_url("file:////server/share/a.mp3"),
            "//server/share/a.mp3"
        );
    }

    #[test]
    fn localhost_e_il_modo_lungo_di_dire_questa_macchina() {
        assert_eq!(da_url("file://localhost/C:/m/a.mp3"), "C:/m/a.mp3");
        assert_eq!(da_url("file://localhost/home/x/a.mp3"), "/home/x/a.mp3");
        // …ma solo se è l'host intero: `localhostdue` è una macchina come le
        // altre, e trattarla come questa manderebbe a cercare sul disco locale.
        assert_eq!(
            da_url("file://localhostdue/share/a.mp3"),
            "//localhostdue/share/a.mp3"
        );
    }

    #[test]
    fn le_forme_di_prima_restano_come_erano() {
        // Le regressioni: qui non si è aggiustato un caso rompendone tre.
        assert_eq!(da_url("file:///C:/musica/a.mp3"), "C:/musica/a.mp3");
        assert_eq!(da_url("file:///home/x/a.mp3"), "/home/x/a.mp3");
        // Fuori standard ma diffuso: è già un percorso Windows.
        assert_eq!(da_url("file://C:/musica/a.mp3"), "C:/musica/a.mp3");
        // Senza schema non si tocca niente.
        assert_eq!(da_url(r"D:\musica\a.mp3"), r"D:\musica\a.mp3");
        assert_eq!(da_url("file://"), "");
    }

    #[test]
    #[cfg(windows)]
    fn una_riga_m3u_di_rete_si_aggancia_per_percorso_pieno() {
        // Il giro completo, che è quel che conta: un brano che in libreria sta
        // su una share, e una riga M3U che lo nomina nella forma RFC. Deve
        // agganciarsi **per percorso**, non per nome del file — l'aggancio per
        // nome è l'ultimo ripiego, e sbaglia in silenzio.
        let mut c = crate::db::open_in_memory().expect("database").connection;
        c.execute_batch(
            "INSERT INTO tracks (id, path, track_key, title, artist, album,
                                 duration_ms, file_size, date_added, date_modified)
             VALUES (1, '\\\\server\\share\\a.mp3', 'k1', 'A', 'Uno', 'AA', 1000, 1, 1, 1);",
        )
        .expect("brano");
        let letta = PlaylistLetta {
            nome: None,
            voci: vec![voce("file://server/share/a.mp3", None, None)],
            illeggibili: 0,
        };
        let r = plan(&mut c, &letta, "Di rete", Some(Path::new(r"C:\altrove"))).expect("piano");
        assert_eq!(
            r.matched_by_path, 1,
            "la riga di rete deve trovare il suo brano"
        );
        assert!(r.missing.is_empty());
    }
}
