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
        .prepare("SELECT id, path FROM tracks")
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
    let senza_schema = scritto
        .strip_prefix("file:///")
        .or_else(|| scritto.strip_prefix("file://"))
        .unwrap_or(scritto);
    if !senza_schema.contains('%') {
        return senza_schema.to_owned();
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

const fn esadecimale(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Barre in un verso solo, tutto minuscolo, niente barra finale.
fn normalizza(percorso: &str) -> String {
    percorso
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase()
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
    let voci: Vec<VocePlaylist> = brani
        .into_iter()
        .map(|b| VocePlaylist {
            percorso: b.path,
            titolo: Some(b.title),
            artista: Some(b.artist),
            durata_ms: u64::try_from(b.duration_ms).ok(),
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
}
