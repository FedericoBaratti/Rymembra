//! Le prove della scansione e delle interrogazioni, su file audio veri e su dischi finti.

use super::interrogazioni::sql_degli_album_di_un_artista;
use super::*;
use crate::files::LocalFiles;
use lofty::config::WriteOptions;
use lofty::prelude::{Accessor, TagExt};
use lofty::tag::{Tag, TagType};
use rymembra_domain::paths::MIN_TRACK_BYTES;
use std::path::{Path, PathBuf};

/// Una libreria finta su disco, con dentro file audio veri.
struct Libreria {
    dir: tempfile::TempDir,
    store: CoverStore,
    connection: Connection,
}

/// Un WAV valido di silenzio, sopra la soglia dei [`MIN_TRACK_BYTES`].
///
/// File audio veri e non byte finti: un doppio del lettore di tag proverebbe
/// che questo modulo sa parlare col doppio. Quel che deve reggere è `lofty`
/// su un file che esiste davvero, comprese le sue idee su dove stia un tag.
fn wav(path: &Path) {
    let campioni = usize::try_from(MIN_TRACK_BYTES).unwrap_or(32_768) + 1_024;
    let dati = campioni.next_multiple_of(2);
    let mut out = Vec::with_capacity(dati + 44);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&u32::try_from(dati + 36).unwrap_or(0).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // lunghezza del blocco fmt
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&44_100u32.to_le_bytes());
    out.extend_from_slice(&88_200u32.to_le_bytes()); // byte al secondo
    out.extend_from_slice(&2u16.to_le_bytes()); // allineamento
    out.extend_from_slice(&16u16.to_le_bytes()); // bit per campione
    out.extend_from_slice(b"data");
    out.extend_from_slice(&u32::try_from(dati).unwrap_or(0).to_le_bytes());
    out.resize(out.len() + dati, 0);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("cartelle");
    }
    std::fs::write(path, &out).expect("scrittura del wav");
}

/// Scrive i tag dentro un file già esistente.
fn tagga(path: &Path, titolo: &str, artista: &str, album: &str) {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(titolo.to_owned());
    tag.set_artist(artista.to_owned());
    tag.set_album(album.to_owned());
    tag.save_to_path(path, WriteOptions::default())
        .expect("scrittura dei tag");
}

impl Libreria {
    fn nuova() -> Self {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let store = CoverStore::open(dir.path().join("copertine")).expect("store");
        let connection = crate::db::open_in_memory().expect("database").connection;
        Self {
            dir,
            store,
            connection,
        }
    }

    /// La cartella della musica: sotto la radice, così lo store delle
    /// copertine non finisce dentro quel che si scansiona.
    fn musica(&self) -> PathBuf {
        self.dir.path().join("musica")
    }

    fn root(&self) -> String {
        self.musica().to_string_lossy().into_owned()
    }

    /// Aggiunge un brano taggato in `relativo`, che è un percorso dentro la
    /// cartella della musica.
    fn brano(&self, relativo: &str, titolo: &str, artista: &str, album: &str) -> PathBuf {
        let path = self.musica().join(relativo);
        wav(&path);
        tagga(&path, titolo, artista, album);
        path
    }

    fn scansiona(&mut self) -> ScanReport {
        let roots = vec![self.root()];
        let scan = Scan {
            files: Arc::new(LocalFiles),
            covers: self.store.clone(),
            roots: &roots,
            radici_della_libreria: &roots,
            rules: PathRules::for_current_platform(),
            prudente: false,
            scadenze: Scadenze::default(),
            fermati: None,
        };
        scan.run(&mut self.connection, |_, _| ControlFlow::Continue(()))
            .expect("scansione")
    }

    /// Come `scansiona`, ma si ferma dopo `quanti` file letti.
    fn scansiona_e_ferma(&mut self, quanti: usize) -> ScanReport {
        let roots = vec![self.root()];
        let scan = Scan {
            files: Arc::new(LocalFiles),
            covers: self.store.clone(),
            roots: &roots,
            radici_della_libreria: &roots,
            rules: PathRules::for_current_platform(),
            prudente: false,
            scadenze: Scadenze::default(),
            fermati: None,
        };
        scan.run(&mut self.connection, |fatti, _| {
            if fatti >= quanti {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .expect("scansione")
    }

    fn conta(&self, tabella: &str) -> i64 {
        self.connection
            .query_row(&format!("SELECT COUNT(*) FROM {tabella}"), [], |r| r.get(0))
            .expect("conteggio")
    }

    /// L'identificativo della riga di un brano, che la scansione decide da sé.
    ///
    /// Si cerca per titolo e non per percorso: la camminata scrive il
    /// percorso coi separatori del sistema, mentre quello che la prova ha
    /// composto tiene le barre con cui è stato scritto qui sopra.
    fn id_del_titolo(&self, titolo: &str) -> i64 {
        self.connection
            .query_row("SELECT id FROM tracks WHERE title = ?1", [titolo], |r| {
                r.get(0)
            })
            .expect("identificativo")
    }
}

#[test]
fn una_scansione_riempie_la_libreria_e_la_rende_cercabile() {
    let mut lib = Libreria::nuova();
    lib.brano("Björk/Homogenic/01.wav", "Jóga", "Björk", "Homogenic");
    lib.brano(
        "Björk/Homogenic/02.wav",
        "Bachelorette",
        "Björk",
        "Homogenic",
    );
    lib.brano("Altro/Raccolta/01.wav", "Pezzo", "Tale", "Raccolta");

    let esito = lib.scansiona();

    assert_eq!(esito.inserted, 3);
    assert_eq!(esito.updated, 0);
    assert_eq!(esito.removed, 0);
    assert!(esito.unreadable.is_empty(), "{:?}", esito.unreadable);
    assert_eq!(lib.conta("tracks"), 3);
    // Due cartelle diverse, due album; gli artisti seguono.
    assert_eq!(esito.aggregates.albums, 2);
    assert_eq!(esito.aggregates.artists, 2);
    assert_eq!(lib.conta("albums"), 2);

    // Interrogabile davvero: la piegatura del tokenizzatore attraversa tutto
    // il giro, dal tag sul disco alla riga trovata.
    let trovati = search(&lib.connection, "bjork", 0, 10).expect("ricerca");
    assert_eq!(trovati.len(), 2);
    let per_titolo = search(&lib.connection, "bachelor", 0, 10).expect("ricerca");
    assert_eq!(
        per_titolo.first().map(|t| t.title.as_str()),
        Some("Bachelorette"),
        "una ricerca si fa mentre si digita"
    );
}

#[test]
fn i_percorsi_trascinati_ritrovano_i_loro_brani() {
    let mut lib = Libreria::nuova();
    lib.brano("A/01.wav", "Uno", "Tale", "A");
    lib.brano("A/02.wav", "Due", "Tale", "A");
    lib.scansiona();

    // Con i separatori del sistema, come li consegna Esplora risorse e come
    // li scrive la camminata: `join("A/01.wav")` su Windows lascerebbe una
    // barra dritta in mezzo, e quel percorso in tabella non c'è.
    let nativo = |cartella: &str, file: &str| {
        lib.musica()
            .join(cartella)
            .join(file)
            .to_string_lossy()
            .into_owned()
    };
    let primo = nativo("A", "01.wav");
    let secondo = nativo("A", "02.wav");
    let fuori = nativo("B", "03.wav");
    let insensibile = PathRules {
        case_insensitive: true,
    };

    let trovati = ids_by_path(
        &lib.connection,
        &[secondo.clone(), fuori.clone(), primo.clone()],
        insensibile,
    )
    .expect("ricerca per percorso");
    assert_eq!(trovati.len(), 3, "uno per percorso, nello stesso ordine");
    assert!(trovati.first().is_some_and(Option::is_some));
    assert_eq!(trovati.get(1), Some(&None), "quel che non è in libreria");
    assert!(trovati.get(2).is_some_and(Option::is_some));
    assert_ne!(trovati.first(), trovati.get(2));

    // Le maiuscole contano solo dove il filesystem le distingue.
    let gridato = vec![primo.to_uppercase()];
    let piegato = ids_by_path(&lib.connection, &gridato, insensibile).expect("piegato");
    assert_eq!(piegato.first(), trovati.get(2));
    let sensibile = PathRules {
        case_insensitive: false,
    };
    let esatto = ids_by_path(&lib.connection, &gridato, sensibile).expect("esatto");
    assert_eq!(esatto, vec![None]);
}

#[test]
fn la_correzione_sopravvive_a_una_riscansione() {
    // Il difetto che questa prova impedisce, ed è stato vero fino alla
    // 2.3.1: `track_overrides` si riempiva e **nessuno** la rileggeva.
    // Bastava che cambiasse la data di modifica di un file — ritaggarlo,
    // copiarlo, toccarlo — perché la correzione dell'utente sparisse da
    // `tracks` restando orfana nella sua tabella, senza che niente lo
    // dicesse.
    let mut lib = Libreria::nuova();
    lib.brano("Senza/Nome/01.wav", "Traccia 01", "Unknown Artist", "Al");
    assert_eq!(lib.scansiona().inserted, 1);
    let id: i64 = lib
        .connection
        .query_row("SELECT id FROM tracks", [], |r| r.get(0))
        .expect("identificativo");

    crate::incerti::correggi(
        &mut lib.connection,
        id,
        &crate::provenienza::Correzioni {
            titolo: Some("Hey You".to_owned()),
            artista: Some("Pink Floyd".to_owned()),
            ..crate::provenienza::Correzioni::default()
        },
    )
    .expect("correzione");

    // Il file non si tocca: i suoi tag restano quelli sbagliati, ed è il
    // punto. Quel che cambia è la sola cosa che serve a far rileggere la
    // riga, cioè il confronto `existing.modified_ms == file.modified_ms` di
    // `plan_scan`. Riscrivere i tag funzionerebbe anche, ma farebbe
    // dipendere la prova dalla risoluzione dell'orologio del filesystem.
    lib.connection
        .execute("UPDATE tracks SET date_modified = 0 WHERE id = ?1", [id])
        .expect("data di modifica");

    let esito = lib.scansiona();
    assert_eq!(
        esito.updated, 1,
        "la riga deve essere stata riletta, altrimenti la prova non prova niente"
    );

    let (titolo, artista, chiave): (String, String, String) = lib
        .connection
        .query_row(
            "SELECT title, artist, track_key FROM tracks WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("riga");
    assert_eq!(titolo, "Hey You", "il titolo corretto resta");
    assert_eq!(artista, "Pink Floyd");
    // E con esso l'identità: una chiave ricalcolata dai tag del file
    // rilegherebbe in silenzio ascolti e posizioni di un altro brano.
    assert!(chiave.contains("pink floyd"), "chiave: {chiave}");
}

#[test]
fn il_testo_di_un_brano_corretto_resta_suo_dopo_una_riscansione() {
    // Due copie dello stesso brano con gli stessi tag sbagliati — il FLAC e
    // l'mp3 — e una sola corretta, con un testo sincronizzato a mano. La
    // riscansione della copia corretta scriveva prima la chiave dei tag, e
    // il trigger della 022 le portava dietro il testo; poi `riapplica`
    // rimetteva la chiave corretta, ma il ritorno era bloccato dall'altra
    // copia, che quella chiave la porta ancora. Il testo finiva sulla copia
    // che nessuno aveva sincronizzato.
    let mut lib = Libreria::nuova();
    lib.brano("Uno/01.wav", "Traccia 01", "Unknown Artist", "Al");
    lib.brano("Due/01.wav", "Traccia 01", "Unknown Artist", "Al");
    assert_eq!(lib.scansiona().inserted, 2);
    let (id, chiave_dei_tag): (i64, String) = lib
        .connection
        .query_row(
            "SELECT id, track_key FROM tracks WHERE path LIKE '%Uno%'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("la prima copia");

    crate::incerti::correggi(
        &mut lib.connection,
        id,
        &crate::provenienza::Correzioni {
            titolo: Some("Hey You".to_owned()),
            artista: Some("Pink Floyd".to_owned()),
            ..crate::provenienza::Correzioni::default()
        },
    )
    .expect("correzione");
    let chiave_corretta: String = lib
        .connection
        .query_row("SELECT track_key FROM tracks WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .expect("chiave corretta");
    assert_ne!(chiave_corretta, chiave_dei_tag);
    lib.connection
        .execute(
            "INSERT INTO lyrics (track_key, source, synced, checked_at, updated_at)
                 VALUES (?1, 'mano', '[00:01.00]Is there anybody', 1, 1)",
            [&chiave_corretta],
        )
        .expect("testo a mano");

    lib.connection
        .execute("UPDATE tracks SET date_modified = 0 WHERE id = ?1", [id])
        .expect("data di modifica");
    assert_eq!(
        lib.scansiona().updated,
        1,
        "la riga deve essere stata riletta"
    );

    let dove: Vec<String> = lib
        .connection
        .prepare("SELECT track_key FROM lyrics")
        .and_then(|mut s| {
            s.query_map([], |r| r.get::<_, String>(0))
                .and_then(Iterator::collect)
        })
        .expect("testi");
    assert_eq!(
        dove,
        vec![chiave_corretta],
        "il testo resta alla copia corretta"
    );
}

#[test]
fn gli_elenchi_non_ordinano_la_tabella_a_ogni_pagina() {
    // Il piano di SQLite, e non un cronometro: «USE TEMP B-TREE FOR ORDER
    // BY» vuol dire che ogni pagina da duecento righe ordinava la libreria
    // intera. È la frase che la migrazione 024 esiste per togliere, e questa
    // prova fallisce se un giorno l'ordinamento e l'indice smettono di
    // avere la stessa forma.
    let lib = Libreria::nuova();
    let piano = |sql: &str| -> String {
        let mut statement = lib
            .connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .expect("piano");
        let righe = statement
            .query_map([], |r| r.get::<_, String>(3))
            .expect("righe del piano");
        righe
            .collect::<Result<Vec<_>, _>>()
            .expect("piano letto")
            .join(" | ")
    };
    for ordine in [TrackOrder::Shelf, TrackOrder::Title, TrackOrder::MostPlayed] {
        let letto = piano(&format!(
            "SELECT t.id FROM tracks t ORDER BY {} LIMIT 200 OFFSET 0",
            ordine.sql()
        ));
        assert!(
            !letto.contains("TEMP B-TREE"),
            "{ordine:?} ordina la tabella intera: {letto}"
        );
    }
    let album = piano(
        "SELECT a.title FROM albums a
             ORDER BY a.artist COLLATE NOCASE, a.year, a.title COLLATE NOCASE
             LIMIT 200 OFFSET 0",
    );
    assert!(!album.contains("TEMP B-TREE"), "gli album: {album}");
}

#[test]
fn la_correzione_torna_con_il_file() {
    // Il file sparisce — la cartella tolta, il disco scollegato — e la
    // scansione cancella la riga. Il file torna, e la riga nuova deve
    // ritrovare quel che l'utente aveva corretto a mano.
    let mut lib = Libreria::nuova();
    let file = lib.brano("Senza/Nome/01.wav", "Traccia 01", "Unknown Artist", "Al");
    assert_eq!(lib.scansiona().inserted, 1);
    let id: i64 = lib
        .connection
        .query_row("SELECT id FROM tracks", [], |r| r.get(0))
        .expect("identificativo");
    crate::incerti::correggi(
        &mut lib.connection,
        id,
        &crate::provenienza::Correzioni {
            titolo: Some("Hey You".to_owned()),
            artista: Some("Pink Floyd".to_owned()),
            ..crate::provenienza::Correzioni::default()
        },
    )
    .expect("correzione");

    std::fs::remove_file(&file).expect("il file sparisce");
    assert_eq!(lib.scansiona().removed, 1);
    assert_eq!(lib.conta("track_overrides"), 0, "la riga se n'è andata");
    assert_eq!(lib.conta("correzioni_orfane"), 1, "la correzione no");

    lib.brano("Senza/Nome/01.wav", "Traccia 01", "Unknown Artist", "Al");
    assert_eq!(lib.scansiona().inserted, 1);
    let (titolo, artista): (String, String) = lib
        .connection
        .query_row("SELECT title, artist FROM tracks", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .expect("riga");
    assert_eq!(titolo, "Hey You");
    assert_eq!(artista, "Pink Floyd");
    assert_eq!(lib.conta("correzioni_orfane"), 0, "e non resta orfana");
}

#[test]
fn ascolti_voto_e_playlist_tornano_con_il_file() {
    // La migrazione 028 vista da una scansione vera: il file sparisce, la
    // riga se ne va — e con lei, in cascata, cronologia e playlist — e poi
    // il file torna. La riga nuova deve ritrovare tutto quel che l'utente
    // ci aveva costruito sopra.
    let mut lib = Libreria::nuova();
    let file = lib.brano("Disco/01.wav", "Uno", "Artista", "Disco");
    lib.brano("Disco/02.wav", "Due", "Artista", "Disco");
    assert_eq!(lib.scansiona().inserted, 2);
    let uno = lib.id_del_titolo("Uno");
    let due = lib.id_del_titolo("Due");
    lib.connection
        .execute(
            "UPDATE tracks SET play_count = 5, last_played_at = 1000, rating = 4,
                        rating_at = 900, liked = 1, liked_at = 800, stats_updated_at = 900
                  WHERE id = ?1",
            [uno],
        )
        .expect("statistiche");
    lib.connection
        .execute_batch(&format!(
            "INSERT INTO play_history (track_id, played_at, ms_played) VALUES ({uno}, 100, 1000);
                 INSERT INTO play_history (track_id, played_at, ms_played, source)
                 VALUES ({uno}, 200, 2000, 'spotify');"
        ))
        .expect("cronologia");
    let lista = crate::playlists::create(&lib.connection, "Lista")
        .expect("playlist")
        .id;
    crate::playlists::add_tracks(&mut lib.connection, lista, &[due, uno, due]).expect("aggiunti");
    let altra = crate::playlists::create(&lib.connection, "Altra")
        .expect("playlist")
        .id;
    crate::playlists::add_tracks(&mut lib.connection, altra, &[uno]).expect("aggiunto");

    std::fs::remove_file(&file).expect("il file sparisce");
    assert_eq!(lib.scansiona().removed, 1);
    assert_eq!(
        lib.conta("play_history"),
        0,
        "la cascata ha portato via gli ascolti"
    );
    assert_eq!(lib.conta("brani_orfani"), 1, "ma sono stati messi da parte");
    assert_eq!(lib.conta("ascolti_orfani"), 2);
    assert_eq!(lib.conta("playlist_orfane"), 2);

    // Una playlist cancellata nel frattempo si porta via il posto che teneva.
    crate::playlists::delete(&mut lib.connection, altra).expect("cancellata");
    assert_eq!(lib.conta("playlist_orfane"), 1);

    lib.brano("Disco/01.wav", "Uno", "Artista", "Disco");
    assert_eq!(lib.scansiona().inserted, 1);
    let tornato = lib.id_del_titolo("Uno");
    assert_ne!(tornato, uno, "è una riga nuova");
    let statistiche: (i64, i64, i64, i64, i64, i64, i64) = lib
        .connection
        .query_row(
            "SELECT play_count, last_played_at, rating, rating_at, liked, liked_at,
                        stats_updated_at
                   FROM tracks WHERE id = ?1",
            [tornato],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .expect("statistiche");
    assert_eq!(statistiche, (5, 1000, 4, 900, 1, 800, 900));
    let ascolti: i64 = lib
        .connection
        .query_row(
            "SELECT COUNT(*) FROM play_history WHERE track_id = ?1",
            [tornato],
            |r| r.get(0),
        )
        .expect("cronologia");
    assert_eq!(ascolti, 2, "la cronologia è tornata");
    let importato: String = lib
        .connection
        .query_row(
            "SELECT source FROM play_history WHERE track_id = ?1 AND played_at = 200",
            [tornato],
            |r| r.get(0),
        )
        .expect("origine");
    assert_eq!(
        importato, "spotify",
        "un ascolto importato torna importato, e si lascia ancora disfare"
    );
    // Nel buco che aveva lasciato, fra i due «Due».
    let ordine: Vec<String> = crate::playlists::tracks(&lib.connection, lista)
        .expect("brani")
        .into_iter()
        .map(|b| b.title)
        .collect();
    assert_eq!(ordine, ["Due", "Uno", "Due"]);
    assert_eq!(lib.conta("brani_orfani"), 0, "e non resta orfano");
    assert_eq!(lib.conta("ascolti_orfani"), 0);
    assert_eq!(lib.conta("playlist_orfane"), 0);
}

#[cfg(windows)]
#[test]
fn un_file_rinominato_solo_nelle_maiuscole_tiene_la_riga_e_prende_il_nome() {
    // Windows non cambia la data di un file rinominato, e la chiave del
    // percorso piega il caso: senza guardare la grafia la riga terrebbe il
    // nome vecchio, che su una share Samba sensibile alle maiuscole non si
    // apre più. Stessa riga, stessa storia, nome nuovo.
    let mut lib = Libreria::nuova();
    let file = lib.brano("Disco/Song.wav", "Canzone", "Artista", "Disco");
    assert_eq!(lib.scansiona().inserted, 1);
    let id = lib.id_del_titolo("Canzone");
    lib.connection
        .execute("UPDATE tracks SET play_count = 3 WHERE id = ?1", [id])
        .expect("ascolti");

    let rinominato = file.with_file_name("song.wav");
    std::fs::rename(&file, &rinominato).expect("rinomina");
    let esito = lib.scansiona();
    assert_eq!((esito.inserted, esito.removed, esito.updated), (0, 0, 1));
    let (percorso, ascolti): (String, i64) = lib
        .connection
        .query_row(
            "SELECT path, play_count FROM tracks WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("riga");
    assert!(percorso.ends_with("song.wav"), "{percorso}");
    assert_eq!(ascolti, 3);

    // E la passata dopo non ha più niente da fare.
    assert_eq!(lib.scansiona().plan.unchanged, 1);
}

#[test]
fn una_correzione_di_prima_della_019_si_salva_anche_lei() {
    // Una riga corretta prima della 019: la migrazione le ha lasciato
    // `content_key` a NULL, e il file non è mai cambiato da allora.
    let mut lib = Libreria::nuova();
    let file = lib.brano("Senza/Nome/01.wav", "Traccia 01", "Unknown Artist", "Al");
    assert_eq!(lib.scansiona().inserted, 1);
    let id: i64 = lib
        .connection
        .query_row("SELECT id FROM tracks", [], |r| r.get(0))
        .expect("identificativo");
    crate::incerti::correggi(
        &mut lib.connection,
        id,
        &crate::provenienza::Correzioni {
            titolo: Some("Hey You".to_owned()),
            ..crate::provenienza::Correzioni::default()
        },
    )
    .expect("correzione");
    lib.connection
        .execute("UPDATE tracks SET content_key = NULL WHERE id = ?1", [id])
        .expect("com'era dopo la 019");

    // La riscansione di tutti i giorni la rilegge, una volta.
    assert_eq!(
        lib.scansiona().updated,
        1,
        "la riga senza chiave si rilegge"
    );
    assert_eq!(lib.scansiona().updated, 0, "e poi basta");
    let titolo: String = lib
        .connection
        .query_row("SELECT title FROM tracks WHERE id = ?1", [id], |r| r.get(0))
        .expect("titolo");
    assert_eq!(titolo, "Hey You", "la correzione resta");

    // E il file che sparisce lascia la correzione da parte.
    std::fs::remove_file(&file).expect("il file sparisce");
    assert_eq!(lib.scansiona().removed, 1);
    assert_eq!(
        lib.conta("correzioni_orfane"),
        1,
        "la correzione resta orfana"
    );
}

#[test]
fn la_chiave_di_contenuto_ignora_la_correzione() {
    // Le due chiavi fanno due mestieri diversi, e questa prova lo fissa:
    // `track_key` segue quel che l'utente ha deciso — deve, o gli altri
    // dispositivi cercano il brano sotto il nome sbagliato — mentre
    // `content_key` resta quella dei tag del file, che nessuno ha toccato.
    let mut lib = Libreria::nuova();
    lib.brano("Senza/Nome/01.wav", "Traccia 01", "Unknown Artist", "Al");
    assert_eq!(lib.scansiona().inserted, 1);

    let (id, contenuto_prima, brano_prima): (i64, String, String) = lib
        .connection
        .query_row("SELECT id, content_key, track_key FROM tracks", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .expect("riga");
    assert_eq!(
        contenuto_prima, brano_prima,
        "senza correzioni le due chiavi coincidono: la differenza nasce dopo"
    );

    crate::incerti::correggi(
        &mut lib.connection,
        id,
        &crate::provenienza::Correzioni {
            artista: Some("Pink Floyd".to_owned()),
            ..crate::provenienza::Correzioni::default()
        },
    )
    .expect("correzione");

    let (contenuto, brano): (String, String) = lib
        .connection
        .query_row(
            "SELECT content_key, track_key FROM tracks WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("riga");
    assert!(
        brano.contains("pink floyd"),
        "la chiave di scambio segue la correzione: {brano}"
    );
    assert_eq!(
        contenuto, contenuto_prima,
        "la chiave di contenuto non si muove: la scrivono solo insert_track e update_track"
    );
    assert!(
        contenuto.contains("unknown artist"),
        "e resta quella dei tag del file: {contenuto}"
    );

    // E resiste anche a una riscansione, che ripassa da `update_track` e
    // poi da `riapplica`: il primo la riscrive identica, il secondo non la
    // guarda affatto.
    lib.connection
        .execute("UPDATE tracks SET date_modified = 0 WHERE id = ?1", [id])
        .expect("data di modifica");
    assert_eq!(lib.scansiona().updated, 1);
    let dopo: String = lib
        .connection
        .query_row("SELECT content_key FROM tracks WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .expect("riga");
    assert_eq!(dopo, contenuto_prima);
}

#[test]
fn un_file_spostato_si_ritrova_anche_se_lartista_e_stato_corretto() {
    // **La** prova di questo pacchetto, e il difetto che chiude è perdita di
    // dati: correggere l'artista di un brano gli cambiava l'identità, e da
    // quel momento spostare il suo file non lo faceva più riconoscere. La
    // riga veniva cancellata e reinserita — via ascolti, voto, preferiti,
    // playlist — e con `ON DELETE CASCADE` se ne andava anche la
    // sovrascrittura in `track_overrides`, cioè la correzione stessa.
    //
    // Con `track_key` al posto di `content_key` in `content_keys_of` e nelle
    // chiavi passate a `match_moved_tracks`, qui sotto `moved` è 0 e
    // `inserted` è 1: la prova fallisce, ed è il modo di verificare che
    // stia provando qualcosa.
    let mut lib = Libreria::nuova();
    lib.brano("sfusi/traccia.wav", "Hey You", "unknown", "The Wall");
    lib.scansiona();

    let id: i64 = lib
        .connection
        .query_row("SELECT id FROM tracks", [], |r| r.get(0))
        .expect("id");
    lib.connection
        .execute(
            "UPDATE tracks SET play_count = 42, rating = 5 WHERE id = ?1",
            [id],
        )
        .expect("storia d'ascolto");

    // La correzione: da qui in poi `tracks.track_key` dice «pink floyd» e i
    // tag del file dicono ancora «unknown».
    crate::incerti::correggi(
        &mut lib.connection,
        id,
        &crate::provenienza::Correzioni {
            artista: Some("Pink Floyd".to_owned()),
            ..crate::provenienza::Correzioni::default()
        },
    )
    .expect("correzione");

    // E lo spostamento, col file intatto: è il caso di ogni riordino, di
    // ogni cartella trascinata altrove, di ogni disco rimontato con una
    // lettera diversa.
    let da = lib.musica().join("sfusi/traccia.wav");
    let a = lib.musica().join("Pink Floyd/The Wall/01 Hey You.wav");
    std::fs::create_dir_all(a.parent().expect("cartella")).expect("cartelle");
    std::fs::rename(&da, &a).expect("spostamento");

    let esito = lib.scansiona();

    assert_eq!(esito.moved, 1, "il file si è spostato, non è nuovo");
    assert_eq!(esito.inserted, 0, "un inserimento qui è un brano duplicato");
    assert_eq!(esito.removed, 0);
    assert_eq!(lib.conta("tracks"), 1);

    let (stesso_id, ascolti, voto, artista, percorso): (i64, i64, i64, String, String) = lib
        .connection
        .query_row(
            "SELECT id, play_count, rating, artist, path FROM tracks",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .expect("riga");
    assert_eq!(stesso_id, id, "deve essere la stessa riga, non una nuova");
    assert_eq!(ascolti, 42);
    assert_eq!(voto, 5);
    assert!(percorso.ends_with("01 Hey You.wav"), "percorso: {percorso}");
    // E la correzione è ancora lì: `riapplica` gira dopo `update_track`
    // anche sul ramo `Sposta`, e la riga di `track_overrides` non è stata
    // portata via da nessuna cancellazione.
    assert_eq!(artista, "Pink Floyd");
    assert_eq!(lib.conta("track_overrides"), 1);
}

#[test]
fn senza_tag_la_chiave_e_durata_e_dimensione() {
    // Attraverso la scansione vera, e non chiamando `compute` a mano:
    // l'unica cosa che questa prova può dire e le prove di dominio no è
    // **quali valori** `read_track` passa alla chiave. Se ci passasse i
    // campi già ripiegati — la radice del nome, «Artista sconosciuto» —
    // qui uscirebbero tre segmenti invece di due, e il ramo del ripiego
    // resterebbe quel che era fino a un momento fa: codice che non gira.
    let mut lib = Libreria::nuova();
    let path = lib.musica().join("A/Al/07 Senza tag.wav");
    wav(&path);
    assert_eq!(lib.scansiona().inserted, 1);

    let (contenuto, brano, durata, dimensione): (String, String, i64, i64) = lib
        .connection
        .query_row(
            "SELECT content_key, track_key, duration_ms, file_size FROM tracks",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .expect("riga");

    assert_eq!(
        contenuto,
        format!("{durata}|{dimensione}"),
        "un file senza tag si identifica da quanto dura e quanto pesa"
    );
    // E la dimensione è quella vera sul disco, non un numero qualunque che
    // combacia con sé stesso.
    assert_eq!(
        dimensione,
        i64::try_from(std::fs::metadata(&path).expect("stat").len()).expect("dimensione")
    );

    // `track_key` invece i ripieghi li usa, e deve: è la chiave con cui la
    // sincronizzazione nomina il brano, e `||` renderebbe ogni file senza
    // tag lo stesso brano su ogni dispositivo. Le due chiavi qui divergono
    // apposta.
    assert!(brano.contains("07 senza tag"), "chiave di brano: {brano}");
    assert_ne!(contenuto, brano);
}

#[test]
fn un_file_senza_tag_rinominato_non_si_duplica() {
    // Il caso d'uso vero, ed è quello che il ripiego esiste per servire.
    //
    // Un file senza tag si distingue solo dal nome, quindi il nome è la
    // prima cosa che si sistema. Con l'identità di contenuto derivata dai
    // ripieghi — cioè dalla radice del nome — rinominarlo lo rendeva un
    // brano nuovo: riga cancellata, riga inserita, via ascolti, voto e
    // playlist. Durata e dimensione un rinomino non le tocca.
    //
    // Con i campi ripiegati al posto dei tag grezzi in `read_track`, qui
    // sotto `moved` è 0 e `inserted` è 1: la prova fallisce.
    let mut lib = Libreria::nuova();
    let da = lib.musica().join("sfusi/traccia 001.wav");
    wav(&da);
    lib.scansiona();

    let id: i64 = lib
        .connection
        .query_row("SELECT id FROM tracks", [], |r| r.get(0))
        .expect("id");
    lib.connection
        .execute_batch(&format!(
            "UPDATE tracks SET play_count = 17, rating = 4 WHERE id = {id};
                 INSERT INTO playlists (id, playlist_key, name, created_at, updated_at)
                 VALUES (1, 'p', 'P', 1, 1);
                 INSERT INTO playlist_tracks (playlist_id, track_id, position)
                 VALUES (1, {id}, 0);"
        ))
        .expect("storia d'ascolto");

    // Il rinomino, fuori da Rymembra: stessa cartella, nome che dice
    // finalmente qualcosa. Il contenuto del file non cambia di un byte.
    let a = lib.musica().join("sfusi/Nick Drake - Pink Moon.wav");
    std::fs::rename(&da, &a).expect("rinomino");

    let esito = lib.scansiona();

    assert_eq!(esito.moved, 1, "è lo stesso file con un nome nuovo");
    assert_eq!(esito.inserted, 0, "un inserimento qui è un brano duplicato");
    assert_eq!(esito.removed, 0);
    assert_eq!(lib.conta("tracks"), 1);

    let (stesso_id, ascolti, voto, titolo): (i64, i64, i64, String) = lib
        .connection
        .query_row(
            "SELECT id, play_count, rating, title FROM tracks",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .expect("riga");
    assert_eq!(stesso_id, id, "deve essere la stessa riga, non una nuova");
    assert_eq!(ascolti, 17);
    assert_eq!(voto, 4);
    assert_eq!(lib.conta("playlist_tracks"), 1, "la playlist regge");
    // E il titolo mostrato segue il nome nuovo, che è quel che l'utente si
    // aspetta: cambia l'etichetta, non l'identità.
    assert_eq!(titolo, "Nick Drake - Pink Moon");
}

#[test]
fn la_migrazione_019_sblocca_i_testi_che_uno_scarto_aveva_congelato() {
    // La riparazione dei dati che viaggia con la 019, provata dove sta il
    // pacchetto che la porta.
    //
    // Il difetto: `imposta_scarto` scriveva `source = 'mano'` anche quando
    // un testo non c'era ancora, e da lì gli `UPSERT` di `ricorda` e
    // `ricorda_esito` — che hanno `WHERE lyrics.source <> 'mano'` per non
    // calpestare mai un testo dell'utente — non scrivevano più niente per
    // quel brano. Nessun testo, mai più.
    let connection = rusqlite::Connection::open_in_memory().expect("database");
    for migrazione in crate::db::MIGRATIONS.iter().filter(|m| m.version < 19) {
        connection
            .execute_batch(migrazione.sql)
            .unwrap_or_else(|err| panic!("migrazione {}: {err}", migrazione.version));
    }

    connection
        .execute_batch(
            "INSERT INTO lyrics (track_key, source, offset_ms, updated_at)
                   VALUES ('bloccato', 'mano', 250, 1);
                 INSERT INTO lyrics (track_key, synced, source, offset_ms, updated_at)
                   VALUES ('a mano', '[00:01.00]parole', 'mano', 0, 1);
                 INSERT INTO lyrics (track_key, source, instrumental, offset_ms, updated_at)
                   VALUES ('strumentale', 'mano', 1, 0, 1);
                 INSERT INTO lyrics (track_key, plain, source, offset_ms, updated_at)
                   VALUES ('piatto', 'parole', 'mano', 0, 1);",
        )
        .expect("righe di partenza");

    let diciannove = crate::db::MIGRATIONS
        .iter()
        .find(|m| m.version == 19)
        .expect("la 019 dev'essere nella catena");
    connection
        .execute_batch(diciannove.sql)
        .expect("la 019 si applica");

    let fonte = |chiave: &str| -> String {
        connection
            .query_row(
                "SELECT source FROM lyrics WHERE track_key = ?1",
                [chiave],
                |r| r.get(0),
            )
            .expect("riga")
    };
    assert_eq!(
        fonte("bloccato"),
        "",
        "la riga nata da uno scarto torna al valore neutro"
    );
    // E lo scarto, che è la sola cosa che quella riga portava, resta.
    let scarto: i64 = connection
        .query_row(
            "SELECT offset_ms FROM lyrics WHERE track_key = 'bloccato'",
            [],
            |r| r.get(0),
        )
        .expect("scarto");
    assert_eq!(scarto, 250);

    // I tre casi che non vanno toccati, e sono la ragione per cui la
    // condizione ha tre clausole invece di una.
    assert_eq!(fonte("a mano"), "mano", "un testo sincronizzato a mano");
    assert_eq!(fonte("strumentale"), "mano", "una risposta «non ha parole»");
    assert_eq!(fonte("piatto"), "mano", "un testo piatto scritto a mano");
}

#[test]
fn una_scansione_fermata_lascia_una_libreria_giusta_e_incompleta() {
    // Il patto dell'annullamento: quel che è stato letto è scritto, il resto
    // lo finisce la passata dopo. Con il lotto da 500 e sei file, tutto sta
    // in un lotto solo — quindi il conteggio dice quanti ne sono stati letti
    // prima dello stop, e il taglio avviene alla chiusura del lotto.
    let mut lib = Libreria::nuova();
    for n in 1..=6 {
        lib.brano(
            &format!("Tale/Al/{n:02}.wav"),
            &format!("Pezzo {n}"),
            "Tale",
            "Al",
        );
    }

    let esito = lib.scansiona_e_ferma(3);
    assert!(esito.cancelled, "l'esito deve dire che è stata fermata");
    // I brani letti prima dello stop ci sono, e sono righe vere: la
    // transazione del lotto è stata chiusa comunque.
    assert!(lib.conta("tracks") > 0, "quel che è stato letto resta");

    // E la passata dopo finisce, senza rileggere quel che c'era già.
    let seconda = lib.scansiona();
    assert!(!seconda.cancelled);
    assert_eq!(lib.conta("tracks"), 6, "la seconda passata completa");
}

#[test]
fn fermare_una_scansione_non_cancella_i_brani_spostati() {
    // La parte che rende l'annullamento sicuro invece che distruttivo.
    //
    // Un brano il cui file non è più dove era resta **in sospeso**: potrebbe
    // essersi spostato, e uno dei lotti successivi potrebbe ritrovarlo. Se
    // ci si ferma prima, quei lotti non ci sono stati — e cancellare le
    // righe in sospeso vorrebbe dire che premere Annulla dopo aver
    // riorganizzato una cartella azzera conteggi, preferiti e voti di tutti
    // i brani che ci stavano dentro.
    let mut lib = Libreria::nuova();
    lib.brano("Tale/Al/01.wav", "Pezzo", "Tale", "Al");
    lib.scansiona();
    assert_eq!(lib.conta("tracks"), 1);

    // Gli si dà un voto: è la cosa che una cancellazione distruggerebbe e
    // che nessuna riscansione potrebbe ricostruire.
    lib.connection
        .execute("UPDATE tracks SET rating = 5, liked = 1", [])
        .expect("voto");

    // Il file si sposta, e insieme arriva altra musica da leggere.
    let da = lib.musica().join("Tale/Al/01.wav");
    let a = lib.musica().join("Tale/Altrove/01.wav");
    std::fs::create_dir_all(a.parent().expect("cartella")).expect("cartella");
    std::fs::rename(&da, &a).expect("spostamento");
    for n in 2..=5 {
        lib.brano(
            &format!("Nuovi/Al/{n:02}.wav"),
            &format!("Nuovo {n}"),
            "Altri",
            "Al",
        );
    }

    // Ci si ferma al primo file letto: la riga sparita è ancora in sospeso.
    let esito = lib.scansiona_e_ferma(1);
    assert!(esito.cancelled);
    assert_eq!(
        esito.removed, 0,
        "una scansione fermata non toglie righe in sospeso"
    );

    let voto: i64 = lib
        .connection
        .query_row("SELECT COALESCE(MAX(rating), -1) FROM tracks", [], |r| {
            r.get(0)
        })
        .expect("voto");
    assert_eq!(voto, 5, "il voto del brano spostato è sopravvissuto");
}

#[test]
fn gli_artisti_si_ordinano_senza_articolo() {
    // La regola che la vista Artisti promette: «The Cure» sotto C. Vive nel
    // dominio (`sort_name`) e non in SQL, e questo test è il posto in cui si
    // vede che il giro completo — tag sul disco, riga in libreria, elenco
    // ordinato — la rispetta davvero.
    let mut lib = Libreria::nuova();
    lib.brano(
        "Cure/Disintegration/01.wav",
        "Plainsong",
        "The Cure",
        "Disintegration",
    );
    lib.brano(
        "Cure/Disintegration/02.wav",
        "Lovesong",
        "The Cure",
        "Disintegration",
    );
    lib.brano("Cure/Wish/01.wav", "Open", "The Cure", "Wish");
    lib.brano(
        "Air/Moon Safari/01.wav",
        "La femme d'argent",
        "Air",
        "Moon Safari",
    );
    lib.brano(
        "Doors/Strange Days/01.wav",
        "Strange Days",
        "The Doors",
        "Strange Days",
    );
    lib.scansiona();

    let artisti = list_artists(&lib.connection).expect("elenco");
    let nomi: Vec<&str> = artisti.iter().map(|a| a.name.as_str()).collect();
    // Air, poi Cure, poi Doors: l'articolo non conta, e il nome mostrato
    // resta quello vero.
    assert_eq!(nomi, ["Air", "The Cure", "The Doors"]);

    let cure = artisti
        .iter()
        .find(|a| a.name == "The Cure")
        .expect("i Cure");
    assert_eq!(cure.sort_name, "cure");
    assert_eq!(cure.albums, 2, "due album");
    assert_eq!(cure.tracks, 3, "tre brani");
}

/// Una PNG diversa per ogni numero, per avere impronte diverse.
fn png(seme: u8) -> Vec<u8> {
    let mut buffer = image::RgbImage::new(64, 64);
    for (x, y, pixel) in buffer.enumerate_pixels_mut() {
        *pixel = image::Rgb([
            u8::try_from(x % 256).unwrap_or(0),
            u8::try_from(y % 256).unwrap_or(0),
            seme,
        ]);
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(buffer)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("codifica");
    out
}

#[test]
fn il_mosaico_di_un_artista_si_ferma_a_quattro() {
    // Il ritratto è due per due: chiedere tutte le copertine di chi ha
    // trenta dischi vorrebbe dire spedirne ventisei che nessuno disegna.
    let mut lib = Libreria::nuova();
    for n in 1..=6u8 {
        let path = lib.musica().join(format!("Tale/Album {n}")).join("01.wav");
        wav(&path);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title("Pezzo".to_owned());
        tag.set_artist("Tale".to_owned());
        tag.set_album(format!("Album {n}"));
        tag.push_picture(
            lofty::picture::Picture::unchecked(png(n * 40))
                .pic_type(lofty::picture::PictureType::CoverFront)
                .mime_type(lofty::picture::MimeType::Png)
                .build(),
        );
        tag.save_to_path(&path, WriteOptions::default())
            .expect("tag con copertina");
    }
    lib.scansiona();

    let artisti = list_artists(&lib.connection).expect("elenco");
    let tale = artisti.first().expect("un artista");
    assert_eq!(tale.albums, 6);
    assert_eq!(tale.covers.len(), 4, "il mosaico ne mostra quattro");
    // E sono quattro **diverse**: quattro volte la stessa immagine sarebbe
    // un mosaico che sembra un errore di disegno.
    let distinte: std::collections::HashSet<&String> = tale.covers.iter().collect();
    assert_eq!(distinte.len(), 4);
}

#[test]
fn riscansionare_non_rilegge_niente() {
    // È il numero che rende una riscansione sopportabile: se questo cade,
    // aprire l'app rilegge centomila file.
    let mut lib = Libreria::nuova();
    lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
    lib.brano("A/Al/02.wav", "Due", "Art", "Al");
    lib.scansiona();

    let secondo = lib.scansiona();
    assert_eq!(secondo.inserted, 0);
    assert_eq!(secondo.updated, 0);
    assert_eq!(secondo.removed, 0);
    assert_eq!(secondo.plan.unchanged, 2);
    assert_eq!(lib.conta("tracks"), 2);
}

#[test]
fn un_riordino_non_azzera_ascolti_ne_playlist() {
    // Il caso vero, e il motivo per cui gli spostamenti si riconoscono: dopo
    // un riordino OGNI file ha un percorso nuovo. Senza appaiamento la
    // libreria si svuota e si riempie di righe nuove, e con le righe vecchie
    // se ne vanno conteggi, preferiti e appartenenza alle playlist.
    let mut lib = Libreria::nuova();
    lib.brano("sfusi/traccia.wav", "Uno", "Art", "Al");
    lib.scansiona();

    let id: i64 = lib
        .connection
        .query_row("SELECT id FROM tracks", [], |r| r.get(0))
        .expect("id");
    lib.connection
        .execute_batch(&format!(
            "UPDATE tracks SET play_count = 42, rating = 5, liked = 1 WHERE id = {id};
                 INSERT INTO playlists (id, playlist_key, name, created_at, updated_at)
                 VALUES (1, 'p', 'P', 1, 1);
                 INSERT INTO playlist_tracks (playlist_id, track_id, position)
                 VALUES (1, {id}, 0);"
        ))
        .expect("storia d'ascolto");

    // Il riordino: stesso file, cartella nuova.
    let da = lib.musica().join("sfusi/traccia.wav");
    let a = lib.musica().join("Art/Al/01 Uno.wav");
    std::fs::create_dir_all(a.parent().expect("cartella")).expect("cartelle");
    std::fs::rename(&da, &a).expect("spostamento");

    let esito = lib.scansiona();

    assert_eq!(esito.moved, 1, "il file si è spostato, non è nuovo");
    assert_eq!(esito.inserted, 0);
    assert_eq!(esito.removed, 0);
    assert_eq!(lib.conta("tracks"), 1);

    let (stesso_id, ascolti, voto, percorso): (i64, i64, i64, String) = lib
        .connection
        .query_row("SELECT id, play_count, rating, path FROM tracks", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .expect("riga");
    assert_eq!(stesso_id, id, "deve essere la stessa riga, non una nuova");
    assert_eq!(ascolti, 42);
    assert_eq!(voto, 5);
    assert!(percorso.ends_with("01 Uno.wav"), "percorso: {percorso}");
    assert_eq!(lib.conta("playlist_tracks"), 1, "la playlist regge");
}

#[test]
fn un_brano_ritaggato_conserva_le_statistiche() {
    let mut lib = Libreria::nuova();
    let path = lib.brano("A/Al/01.wav", "Titolo vecchio", "Art", "Al");
    lib.scansiona();
    lib.connection
        .execute("UPDATE tracks SET play_count = 7", [])
        .expect("ascolti");

    // Ritaggato sul posto: cambia il contenuto, non il percorso.
    tagga(&path, "Titolo nuovo", "Art", "Al");
    // La data di modifica deve risultare diversa: su un filesystem che la
    // riporta al millisecondo due scritture di fila possono cadere nello
    // stesso istante, e il piano direbbe «invariato».
    filetime::set_file_mtime(&path, filetime::FileTime::from_unix_time(1_700_000_000, 0))
        .expect("data di modifica");

    let esito = lib.scansiona();
    assert_eq!(esito.updated, 1);
    assert_eq!(esito.inserted, 0);

    let (titolo, ascolti): (String, i64) = lib
        .connection
        .query_row("SELECT title, play_count FROM tracks", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .expect("riga");
    assert_eq!(titolo, "Titolo nuovo");
    assert_eq!(ascolti, 7, "riscrivere i tag non azzera l'ascolto");
}

// ── la rilettura dei tag e lo strato dell'arricchimento ─────────────────

/// Scrive i tag come li lascerebbe un programma qualunque: quelli dati e
/// nient'altro. Senza album se `album` è `None`; con un ReplayGain se
/// `replaygain` c'è, che è quel che ci aggiunge un calcolatore del volume.
fn tagga_con(
    path: &Path,
    titolo: &str,
    artista: &str,
    album: Option<&str>,
    replaygain: Option<&str>,
) {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(titolo.to_owned());
    tag.set_artist(artista.to_owned());
    if let Some(album) = album {
        tag.set_album(album.to_owned());
    }
    if let Some(guadagno) = replaygain {
        tag.insert_text(
            lofty::prelude::ItemKey::ReplayGainTrackGain,
            guadagno.to_owned(),
        );
    }
    tag.save_to_path(path, WriteOptions::default())
        .expect("scrittura dei tag");
}

/// Mette sul file una data di modifica precisa, diversa da quelle vicine.
fn data(path: &Path, secondi: i64) {
    filetime::set_file_mtime(path, filetime::FileTime::from_unix_time(secondi, 0))
        .expect("data di modifica");
}

/// Un brano arricchito come lo lascia una passata vera: `enrich::registra`,
/// con l'annotazione e una copertina arrivata dal catalogo.
fn arricchisci(lib: &mut Libreria, id: i64, campi: rymembra_domain::enrich::Fields) {
    let path: String = lib
        .connection
        .query_row("SELECT path FROM tracks WHERE id = ?1", [id], |r| r.get(0))
        .expect("percorso");
    let tx = lib.connection.transaction().expect("transazione");
    let scrittura = crate::enrich::Scrittura {
        track_id: id,
        path,
        campi,
        copertina: None,
        fonte: "mb-release",
        confidenza: 0.95,
        originali: crate::enrich::TagOriginali::default(),
    };
    let esito = crate::enrich::EsitoFile {
        track_id: id,
        copertina: Some(StoredCover {
            hash: "c0ffee".to_owned(),
            width: 1,
            height: 1,
            byte_size: 1,
            mime_type: "image/jpeg",
            already_present: false,
        }),
        copertina_da: Some("provider"),
        applicato: true,
    };
    crate::enrich::registra(
        &tx,
        &crate::enrich::Decisione::default(),
        &[scrittura],
        &[esito],
        1,
    )
    .expect("registrazione");
    tx.commit().expect("chiusura");
}

/// Titolo, album, anno, copertina e stato dell'arricchimento di una riga.
fn come_sta(
    lib: &Libreria,
    id: i64,
) -> (String, String, Option<i64>, Option<String>, Option<String>) {
    lib.connection
        .query_row(
            "SELECT title, album, year, cover_art_hash, enrich_status
                   FROM tracks WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .expect("riga")
}

/// L'annotazione dell'arricchimento, così com'è in tabella.
fn annotata(lib: &Libreria, id: i64) -> Option<crate::enrich::CampiArricchiti> {
    crate::enrich::annotazione(&lib.connection, id).expect("lettura dell'annotazione")
}

#[test]
fn un_programma_che_tocca_il_file_non_si_porta_via_l_arricchimento() {
    // Il difetto: un calcolatore di ReplayGain riscrive il file, la data
    // cambia, la scansione lo rilegge — e tornavano «Album sconosciuto» e
    // la copertina grigia, per sempre, perché per l'arricchimento il brano
    // era già fatto.
    let mut lib = Libreria::nuova();
    let path = lib.musica().join("Bjork/01.wav");
    wav(&path);
    tagga_con(&path, "Joga", "Björk", None, None);
    data(&path, 1_600_000_000);
    lib.scansiona();
    let id = lib.id_del_titolo("Joga");
    arricchisci(
        &mut lib,
        id,
        rymembra_domain::enrich::Fields {
            album: Some("Homogenic".to_owned()),
            year: Some(1997),
            ..rymembra_domain::enrich::Fields::default()
        },
    );

    // ── il calcolatore del volume: i tag di prima, più un ReplayGain ──
    tagga_con(&path, "Joga", "Björk", None, Some("-6.50 dB"));
    data(&path, 1_600_000_100);
    assert_eq!(lib.scansiona().updated, 1, "il file è cambiato davvero");

    let (titolo, album, anno, copertina, stato) = come_sta(&lib, id);
    assert_eq!(titolo, "Joga");
    assert_eq!(album, "Homogenic", "l'album trovato resta");
    assert_eq!(anno, Some(1997));
    assert_eq!(copertina.as_deref(), Some("c0ffee"), "e la copertina pure");
    assert_eq!(stato.as_deref(), Some("ok"), "non c'è niente da rifare");
    let annotazione = annotata(&lib, id).expect("l'annotazione resta");
    assert_eq!(annotazione.album.as_deref(), Some("Homogenic"));
    assert_eq!(annotazione.copertina.as_deref(), Some("c0ffee"));
    // L'album raggruppa: il brano sta nell'album trovato, non in quello
    // sconosciuto.
    let (chiave, percorso): (String, String) = lib
        .connection
        .query_row(
            "SELECT album_key, path FROM tracks WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("chiave");
    assert_eq!(chiave, album_group_key("Homogenic", &percorso));

    // ── e poi qualcuno lo ritagga davvero: l'album adesso lo dice il file ──
    tagga_con(&path, "Joga", "Björk", Some("Vespertine"), Some("-6.50 dB"));
    data(&path, 1_600_000_200);
    assert_eq!(lib.scansiona().updated, 1);

    let (_, album, anno, copertina, stato) = come_sta(&lib, id);
    assert_eq!(album, "Vespertine", "la parola più recente è del file");
    assert_eq!(anno, Some(1997), "l'anno il file non l'ha toccato");
    assert_eq!(copertina.as_deref(), Some("c0ffee"));
    assert_eq!(stato, None, "e il brano torna in coda all'arricchimento");
    let annotazione = annotata(&lib, id).expect("l'annotazione resta");
    assert_eq!(
        annotazione.album, None,
        "l'album non viene più dal catalogo"
    );
    assert_eq!(annotazione.year, Some(1997));
}

#[test]
fn un_brano_arricchito_spostato_tiene_quel_che_si_era_trovato() {
    // Un file senza tag, arricchito, poi spostato e rinominato. Il titolo
    // «del file» era il ripiego dal nome, e il nome è cambiato: ma dentro
    // il file non è cambiato niente, e il titolo trovato deve restare.
    let mut lib = Libreria::nuova();
    let da = lib.musica().join("sfusi/traccia 01.wav");
    wav(&da);
    lib.scansiona();
    let id = lib.id_del_titolo("traccia 01");
    arricchisci(
        &mut lib,
        id,
        rymembra_domain::enrich::Fields {
            title: Some("Hunter".to_owned()),
            album: Some("Homogenic".to_owned()),
            ..rymembra_domain::enrich::Fields::default()
        },
    );

    let a = lib.musica().join("Björk/Homogenic/01 Hunter.wav");
    std::fs::create_dir_all(a.parent().expect("cartella")).expect("cartelle");
    std::fs::rename(&da, &a).expect("spostamento");
    let esito = lib.scansiona();
    assert_eq!(esito.moved, 1);

    let (titolo, album, _, copertina, stato) = come_sta(&lib, id);
    assert_eq!(titolo, "Hunter");
    assert_eq!(album, "Homogenic");
    assert_eq!(copertina.as_deref(), Some("c0ffee"));
    assert_eq!(stato.as_deref(), Some("ok"));
}

#[test]
fn un_annotazione_della_2_4_si_rimette_dove_il_file_tace() {
    // Le annotazioni scritte prima della 3.0 non sanno cosa ci fosse sotto,
    // né quale fosse la copertina. Dove il file non dice niente si rimette
    // — nessuno l'ha ritaggato —, dove dice altro vince il file e il brano
    // torna in coda. La copertina si riconosce da `cover_art.source`.
    let mut lib = Libreria::nuova();
    let path = lib.musica().join("Bjork/02.wav");
    wav(&path);
    tagga_con(&path, "Joga", "Björk", None, None);
    data(&path, 1_600_000_000);
    lib.scansiona();
    let id = lib.id_del_titolo("Joga");
    lib.connection
            .execute_batch(&format!(
                "INSERT INTO cover_art (hash, mime_type, width, height, byte_size, source, created_at)
                 VALUES ('c0ffee', 'image/jpeg', 1, 1, 1, 'provider', 1);
                 UPDATE tracks SET title = 'Jóga', album = 'Homogenic',
                        cover_art_hash = 'c0ffee', enrich_status = 'ok'
                  WHERE id = {id};
                 INSERT INTO track_meta_arricchita (track_id, campi, fonte, confidenza, set_at)
                 VALUES ({id}, '{{\"title\":\"Jóga\",\"album\":\"Homogenic\"}}', 'mb-release', 0.9, 1);"
            ))
            .expect("un arricchimento della 2.4");

    data(&path, 1_600_000_100);
    assert_eq!(lib.scansiona().updated, 1);

    let (titolo, album, _, copertina, stato) = come_sta(&lib, id);
    assert_eq!(
        album, "Homogenic",
        "il file l'album non ce l'ha: si rimette"
    );
    assert_eq!(titolo, "Joga", "il titolo il file ce l'ha: vince lui");
    assert_eq!(copertina.as_deref(), Some("c0ffee"));
    assert_eq!(stato, None, "un campo ritaggato rimette in coda");
    let annotazione = annotata(&lib, id).expect("l'annotazione resta");
    assert_eq!(annotazione.title, None);
    assert_eq!(annotazione.album.as_deref(), Some("Homogenic"));
    assert_eq!(
        annotazione.copertina.as_deref(),
        Some("c0ffee"),
        "e adesso la copertina la ricorda"
    );
}

#[test]
fn un_file_ritaggato_con_la_data_ferma_si_rilegge() {
    // Mp3tag con «conserva la data di modifica»: stessa data, un titolo
    // più lungo, un file più grande. Il piano guardava solo la data.
    let mut lib = Libreria::nuova();
    let path = lib.brano("A/Al/01.wav", "Corto", "Art", "Al");
    data(&path, 1_600_000_000);
    lib.scansiona();

    tagga(&path, "Un titolo molto più lungo di prima", "Art", "Al");
    data(&path, 1_600_000_000);
    let esito = lib.scansiona();
    assert_eq!(esito.updated, 1, "la dimensione è cambiata");
    let titolo: String = lib
        .connection
        .query_row("SELECT title FROM tracks", [], |r| r.get(0))
        .expect("titolo");
    assert_eq!(titolo, "Un titolo molto più lungo di prima");
}

#[test]
fn rileggere_tutti_i_tag_rilegge_ogni_file_una_volta() {
    let mut lib = Libreria::nuova();
    lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
    lib.brano("A/Al/02.wav", "Due", "Art", "Al");
    lib.scansiona();
    assert_eq!(lib.scansiona().updated, 0, "niente è cambiato");

    assert_eq!(rileggi_tutti_i_tag(&lib.connection).expect("richiesta"), 2);
    assert_eq!(lib.scansiona().updated, 2, "tutti riletti");
    // E la sentinella non resta: la rilettura ha riscritto le date vere.
    assert_eq!(lib.scansiona().updated, 0);
}

#[test]
fn un_file_sparito_se_ne_va_e_si_porta_dietro_l_album() {
    let mut lib = Libreria::nuova();
    lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
    let secondo = lib.brano("B/Bl/01.wav", "Due", "Art2", "Bl");
    lib.scansiona();
    assert_eq!(lib.conta("albums"), 2);

    std::fs::remove_file(&secondo).expect("cancellazione");
    let esito = lib.scansiona();

    assert_eq!(esito.removed, 1);
    assert_eq!(esito.moved, 0, "non c'è nessun file nuovo a reclamarlo");
    assert_eq!(lib.conta("tracks"), 1);
    // Gli aggregati seguono: un album senza brani non deve restare in giro.
    assert_eq!(lib.conta("albums"), 1);
    assert_eq!(lib.conta("artists"), 1);
}

#[test]
fn una_scansione_non_scrive_lapidi() {
    // Staccare un disco esterno non è «cancella questi brani»: una lapide
    // propagherebbe al telefono una cancellazione che nessuno ha chiesto.
    let mut lib = Libreria::nuova();
    let path = lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
    lib.scansiona();
    std::fs::remove_file(&path).expect("cancellazione");
    lib.scansiona();
    assert_eq!(lib.conta("tracks"), 0);
    assert_eq!(lib.conta("sync_tombstones"), 0);
}

/// Le regole dei percorsi delle prove: Windows e macOS ignorano le maiuscole.
const REGOLE: PathRules = PathRules {
    case_insensitive: true,
};

#[test]
fn cancellare_un_brano_se_ne_porta_dietro_l_album_e_l_artista() {
    let mut lib = Libreria::nuova();
    lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
    let secondo = lib.brano("B/Bl/01.wav", "Due", "Art2", "Bl");
    lib.scansiona();
    assert_eq!(lib.conta("albums"), 2);
    let id = lib.id_del_titolo("Due");
    let radici = vec![lib.root()];

    let esito = cancella_brani(&mut lib.connection, &[id], &radici, REGOLE).expect("cancellazione");

    assert_eq!(esito.tolti, 1);
    assert_eq!(lib.conta("tracks"), 1);
    // Gli aggregati nella stessa transazione: senza, resterebbe un album
    // senza brani e un artista senza album.
    assert_eq!(lib.conta("albums"), 1);
    assert_eq!(lib.conta("artists"), 1);
    // Il file non si tocca: questa funzione non sa nemmeno che esiste un
    // disco.
    assert!(secondo.exists(), "il file deve essere ancora al suo posto");
}

#[test]
fn cancellare_a_mano_non_scrive_lapidi() {
    // Per la stessa ragione della scansione, detta dall'altro lato:
    // `lapidi.brani` non la consulta nessuno, e una riga che non protegge
    // niente è stato morto.
    let mut lib = Libreria::nuova();
    lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
    lib.scansiona();
    let id = lib.id_del_titolo("Uno");
    let radici = vec![lib.root()];

    cancella_brani(&mut lib.connection, &[id], &radici, REGOLE).expect("cancellazione");

    assert_eq!(lib.conta("tracks"), 0);
    assert_eq!(lib.conta("sync_tombstones"), 0);
}

#[test]
fn cancellare_dice_quanti_torneranno_alla_prossima_scansione() {
    let mut lib = Libreria::nuova();
    lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
    lib.scansiona();
    let id = lib.id_del_titolo("Uno");

    // Sorvegliata: il file è ancora lì, e la scansione dopo lo rimette.
    let radici = vec![lib.root()];
    let esito = cancella_brani(&mut lib.connection, &[id], &radici, REGOLE).expect("cancellazione");
    assert_eq!(esito.tolti, 1);
    assert_eq!(esito.torneranno, 1);
    lib.scansiona();
    assert_eq!(
        lib.conta("tracks"),
        1,
        "torna davvero, non per modo di dire"
    );

    // Fuori da ogni cartella sorvegliata — una radice tolta, un disco
    // staccato — non torna nessuno, e non lo si promette.
    let id = lib.id_del_titolo("Uno");
    let esito = cancella_brani(&mut lib.connection, &[id], &[], REGOLE).expect("cancellazione");
    assert_eq!(esito.tolti, 1);
    assert_eq!(esito.torneranno, 0);
}

#[test]
fn cancellare_un_brano_lo_toglie_dalle_playlist() {
    let mut lib = Libreria::nuova();
    lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
    lib.scansiona();
    let id = lib.id_del_titolo("Uno");
    lib.connection
        .execute_batch(
            "INSERT INTO playlists (id, playlist_key, name, created_at, updated_at)
                 VALUES (1, 'p', 'P', 1, 1);",
        )
        .expect("playlist");
    lib.connection
        .execute(
            "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (1, ?1, 0)",
            [id],
        )
        .expect("riga di playlist");

    cancella_brani(&mut lib.connection, &[id], &[], REGOLE).expect("cancellazione");

    // La cascata, che le chiavi esterne accese rendono possibile.
    assert_eq!(lib.conta("playlist_tracks"), 0);
    // La playlist resta: si è cancellato un brano, non un elenco.
    assert_eq!(lib.conta("playlists"), 1);
}

#[test]
fn cancellare_quel_che_non_c_e_non_e_un_guasto() {
    // Il clic su un elenco cambiato sotto: non si tocca niente, e non si
    // rifanno gli aggregati per niente.
    let mut lib = Libreria::nuova();
    lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
    lib.scansiona();

    let esito = cancella_brani(&mut lib.connection, &[4_242], &[], REGOLE).expect("cancellazione");

    assert_eq!(esito.tolti, 0);
    assert_eq!(esito.torneranno, 0);
    assert_eq!(lib.conta("tracks"), 1);
    assert_eq!(lib.conta("albums"), 1);
}

/// Più identificativi di quanti parametri SQLite accetti in una query
/// sola (32 766): quelli veri stanno **in fondo**, dopo il limite, dove
/// un'unica `IN (…)` non arrivava nemmeno a essere preparata.
fn oltre_il_limite_dei_parametri(veri: &[i64]) -> Vec<i64> {
    let mut ids: Vec<i64> = (1_000_000..1_040_000).collect();
    ids.extend_from_slice(veri);
    ids
}

#[test]
fn una_coda_di_quarantamila_brani_si_legge_lo_stesso() {
    let mut lib = Libreria::nuova();
    lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
    lib.brano("B/Bl/01.wav", "Due", "Art2", "Bl");
    lib.scansiona();
    let uno = lib.id_del_titolo("Uno");
    let due = lib.id_del_titolo("Due");

    let ids = oltre_il_limite_dei_parametri(&[due, uno, due]);
    let righe = summaries_by_id(&lib.connection, &ids).expect("lettura a pezzi");

    // L'ordine chiesto, doppioni compresi, e niente per chi non esiste.
    let titoli: Vec<&str> = righe.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(titoli, ["Due", "Uno", "Due"]);
}

#[test]
fn cancellare_quarantamila_brani_non_supera_il_limite_dei_parametri() {
    let mut lib = Libreria::nuova();
    lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
    lib.brano("B/Bl/01.wav", "Due", "Art2", "Bl");
    lib.scansiona();
    let ids = oltre_il_limite_dei_parametri(&[lib.id_del_titolo("Uno")]);

    let esito = cancella_brani(&mut lib.connection, &ids, &[], REGOLE).expect("cancellazione");

    assert_eq!(esito.tolti, 1);
    assert_eq!(lib.conta("tracks"), 1);
    assert_eq!(lib.conta("albums"), 1);
}

#[test]
fn l_arricchimento_degli_artisti_sopravvive_a_una_riscansione() {
    // `albums` si ricostruisce da zero perché non contiene niente d'altro;
    // `artists` no, perché la biografia non sta nei file.
    let mut lib = Libreria::nuova();
    lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
    lib.scansiona();
    lib.connection
        .execute("UPDATE artists SET bio = 'una biografia'", [])
        .expect("arricchimento");

    lib.brano("A/Al/02.wav", "Due", "Art", "Al");
    lib.scansiona();

    let bio: Option<String> = lib
        .connection
        .query_row("SELECT bio FROM artists WHERE name = 'Art'", [], |r| {
            r.get(0)
        })
        .expect("artista");
    assert_eq!(bio.as_deref(), Some("una biografia"));
}

#[test]
fn quel_che_non_e_audio_non_ferma_la_scansione() {
    let mut lib = Libreria::nuova();
    lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
    // Un file con l'estensione giusta e dentro spazzatura: passa il filtro
    // del piano (estensione e dimensione) e cade sulla lettura dei tag.
    let finto = lib.musica().join("A/Al/rotto.mp3");
    std::fs::write(&finto, vec![0u8; 40_000]).expect("scrittura");

    let esito = lib.scansiona();
    assert_eq!(esito.inserted, 1, "il brano buono entra lo stesso");
    assert_eq!(esito.unreadable.len(), 1);
    assert!(
        esito
            .unreadable
            .first()
            .is_some_and(|u| u.path.ends_with("rotto.mp3"))
    );
    assert_eq!(lib.conta("tracks"), 1);
}

#[test]
fn un_file_senza_tag_prende_il_nome_dal_disco() {
    let mut lib = Libreria::nuova();
    wav(&lib.musica().join("A/Al/07 Senza tag.wav"));
    let esito = lib.scansiona();

    assert_eq!(esito.inserted, 1);
    let (titolo, chiave): (String, String) = lib
        .connection
        .query_row("SELECT title, track_key FROM tracks", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .expect("riga");
    assert_eq!(titolo, "07 Senza tag");
    // La chiave si deriva dai ripieghi, non dai tag vuoti: fosse `||`, ogni
    // file senza tag sarebbe lo stesso brano per la sincronizzazione.
    assert_ne!(chiave, "||");
    assert!(chiave.contains("07 senza tag"), "chiave: {chiave}");
}

#[test]
fn i_dischi_multipli_restano_una_pubblicazione_sola() {
    let mut lib = Libreria::nuova();
    lib.brano("Art/Opera/CD1/01.wav", "Uno", "Art", "Opera");
    lib.brano("Art/Opera/CD2/01.wav", "Due", "Art", "Opera");
    let esito = lib.scansiona();

    assert_eq!(esito.aggregates.albums, 1);
    let (titolo, brani): (String, i64) = lib
        .connection
        .query_row("SELECT title, total_tracks FROM albums", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .expect("album");
    assert_eq!(titolo, "Opera");
    assert_eq!(brani, 2);
}

#[test]
fn la_ricerca_non_si_rompe_sui_caratteri_da_operatore() {
    // `AC/DC`, un trattino, una virgoletta: senza virgolettatura FTS5 li
    // legge come sintassi e la ricerca fallisce invece di non trovare nulla.
    let mut lib = Libreria::nuova();
    lib.brano("AC-DC/Back/01.wav", "T.N.T.", "AC/DC", "Back in Black");
    lib.scansiona();

    for query in ["AC/DC", "back - black", "\"virgolette", "NEAR", "OR", "*"] {
        let esito = search(&lib.connection, query, 0, 10);
        assert!(
            esito.is_ok(),
            "{query} ha fatto fallire la ricerca: {esito:?}"
        );
    }
    assert_eq!(
        search(&lib.connection, "AC/DC", 0, 10)
            .expect("ricerca")
            .len(),
        1
    );
    assert!(
        search(&lib.connection, "", 0, 10)
            .expect("ricerca")
            .is_empty(),
        "una ricerca vuota non è un errore, è nessun risultato"
    );
}

#[test]
fn la_ricerca_si_impagina_e_sa_quanti_ne_ha() {
    // Il difetto che chiude: l'intestazione scriveva «N risultati» dove N
    // era la lunghezza della prima pagina, cioè il limite travestito da
    // conteggio. E oltre quel limite non c'era modo di arrivare.
    let mut lib = Libreria::nuova();
    for n in 1..=7 {
        lib.brano(
            &format!("Tale/Al/{n:02}.wav"),
            &format!("Pezzo {n}"),
            "Tale",
            "Al",
        );
    }
    lib.scansiona();

    assert_eq!(
        search_count(&lib.connection, "pezzo").expect("conteggio"),
        7,
        "il conteggio non si ferma alla pagina"
    );
    let prima = search(&lib.connection, "pezzo", 0, 3).expect("ricerca");
    let seconda = search(&lib.connection, "pezzo", 3, 3).expect("ricerca");
    let terza = search(&lib.connection, "pezzo", 6, 3).expect("ricerca");
    assert_eq!((prima.len(), seconda.len(), terza.len()), (3, 3, 1));

    // Le tre pagine non si sovrappongono e coprono tutto: senza un ordine
    // stabile due fette consecutive potrebbero ripetere una riga e saltarne
    // un'altra, che è il difetto che l'impaginazione porta con sé.
    let mut visti: Vec<i64> = prima
        .iter()
        .chain(&seconda)
        .chain(&terza)
        .map(|b| b.id)
        .collect();
    visti.sort_unstable();
    visti.dedup();
    assert_eq!(visti.len(), 7, "sette righe distinte in tre pagine");

    assert_eq!(
        search_count(&lib.connection, "").expect("conteggio"),
        0,
        "una ricerca vuota conta zero, non tutto"
    );
}

#[test]
fn i_preferiti_sono_una_query_e_non_un_filtro() {
    // Prima si chiedevano duemila brani e si tenevano quelli col cuore:
    // tutta la libreria letta a ogni visita, e chi ne ha più di duemila non
    // vedeva i preferiti oltre — senza che niente lo dicesse.
    let mut lib = Libreria::nuova();
    for n in 1..=5 {
        lib.brano(
            &format!("Tale/Al/{n:02}.wav"),
            &format!("Pezzo {n}"),
            "Tale",
            "Al",
        );
    }
    lib.scansiona();

    // Tre segnati, in tre momenti diversi: l'ordine atteso è quello della
    // decisione, dal più recente.
    lib.connection
        .execute(
            "UPDATE tracks SET liked = 1, liked_at = 300 WHERE title = 'Pezzo 1'",
            [],
        )
        .expect("cuore");
    lib.connection
        .execute(
            "UPDATE tracks SET liked = 1, liked_at = 100 WHERE title = 'Pezzo 3'",
            [],
        )
        .expect("cuore");
    // Segnato prima che `liked_at` esistesse: va in fondo, non dove capita.
    lib.connection
        .execute(
            "UPDATE tracks SET liked = 1, liked_at = NULL WHERE title = 'Pezzo 5'",
            [],
        )
        .expect("cuore");

    let tutti = list_liked(&lib.connection, 0, 100).expect("preferiti");
    assert_eq!(
        tutti.iter().map(|b| b.title.as_str()).collect::<Vec<_>>(),
        ["Pezzo 1", "Pezzo 3", "Pezzo 5"]
    );
    assert!(
        tutti.iter().all(|b| b.liked),
        "solo brani col cuore, e la query non lo deve dimenticare"
    );

    // E si impagina come gli altri elenchi.
    let pagina = list_liked(&lib.connection, 1, 1).expect("preferiti");
    assert_eq!(pagina.len(), 1);
    assert_eq!(pagina.first().map(|b| b.title.as_str()), Some("Pezzo 3"));
}

#[test]
fn la_cronologia_si_legge_dal_piu_recente_e_porta_il_brano_con_se() {
    // La tabella si scriveva dal primo giorno e non la leggeva nessuno.
    // Questa prova tiene fermi i due punti in cui è facile sbagliare: il
    // verso dell'ordine — chi apre la cronologia guarda l'ultima cosa che ha
    // ascoltato, non la prima in assoluto — e gli indici delle colonne dopo
    // quelle del brano, che si spostano se `COLONNE_BRANO` ne guadagna una.
    let mut lib = Libreria::nuova();
    lib.brano("Tale/Al/01.wav", "Primo", "Tale", "Al");
    lib.brano("Tale/Al/02.wav", "Secondo", "Tale", "Al");
    lib.scansiona();

    let ids: Vec<i64> = {
        let mut s = lib
            .connection
            .prepare("SELECT id FROM tracks ORDER BY title")
            .expect("brani");
        let righe = s.query_map([], |r| r.get(0)).expect("query");
        righe.collect::<Result<_, _>>().expect("ids")
    };
    let (primo, secondo) = (ids.first().copied(), ids.get(1).copied());

    for (id, quando, ms, sorgente) in [
        (primo, 1_000_i64, 200_000_i64, "local"),
        (secondo, 3_000, 150_000, "spotify"),
        (primo, 2_000, 210_000, "local"),
    ] {
        lib.connection
            .execute(
                "INSERT INTO play_history (track_id, played_at, ms_played, source)
                     VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, quando, ms, sorgente],
            )
            .expect("ascolto");
    }

    let pagina = list_history(&lib.connection, 0, 10).expect("cronologia");
    assert_eq!(count_history(&lib.connection), Ok(3));
    assert_eq!(
        pagina.iter().map(|v| v.quando_ms).collect::<Vec<_>>(),
        [3_000, 2_000, 1_000],
        "il più recente in cima"
    );
    let capo = pagina.first().expect("una riga");
    assert_eq!(capo.brano.title, "Secondo");
    assert_eq!(capo.ms_ascoltati, 150_000);
    assert_eq!(capo.sorgente, "spotify");

    // E si impagina come tutti gli altri elenchi.
    let seconda = list_history(&lib.connection, 2, 10).expect("cronologia");
    assert_eq!(seconda.len(), 1);
    assert_eq!(seconda.first().map(|v| v.quando_ms), Some(1_000));
}

#[test]
fn un_brano_tolto_porta_via_i_suoi_ascolti() {
    // `ON DELETE CASCADE` più la `JOIN`: due difese sullo stesso caso, e la
    // seconda è quella che regge se un giorno la prima venisse tolta da una
    // migrazione. Una riga di cronologia senza titolo né interprete, in
    // mezzo a un elenco di canzoni, non è un dato mancante — è una riga
    // vuota che l'utente legge come un guasto.
    let mut lib = Libreria::nuova();
    lib.brano("Tale/Al/01.wav", "Solo", "Tale", "Al");
    lib.scansiona();
    lib.connection
        .execute(
            "INSERT INTO play_history (track_id, played_at, ms_played)
                 SELECT id, 500, 120000 FROM tracks",
            [],
        )
        .expect("ascolto");
    assert_eq!(list_history(&lib.connection, 0, 10).map(|v| v.len()), Ok(1));

    std::fs::remove_file(lib.musica().join("Tale/Al/01.wav")).expect("cancellato");
    lib.scansiona();
    assert_eq!(list_history(&lib.connection, 0, 10), Ok(vec![]));
}

#[test]
fn gli_album_di_un_artista_si_chiedono_al_database() {
    // Prima si filtrava nella finestra la pagina di album già scaricata:
    // un artista i cui dischi stavano oltre l'ultimo album chiesto dava una
    // pagina vuota sotto un titolo che diceva «due album».
    let mut lib = Libreria::nuova();
    lib.brano("Tale/Primo/01.wav", "A", "Tale", "Primo");
    lib.brano("Tale/Secondo/01.wav", "B", "Tale", "Secondo");
    lib.brano("Altro/Terzo/01.wav", "C", "Altro", "Terzo");
    lib.scansiona();

    let suoi = albums_by_artist(&lib.connection, "Tale", 0, 100).expect("album");
    assert_eq!(
        suoi.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(),
        ["Primo", "Secondo"]
    );
    assert!(
        suoi.iter().all(|a| a.artist == "Tale"),
        "nessun disco in cui «Tale» non suona"
    );

    // Confronto esatto sull'artista effettivo dei brani — il risultato di
    // `TRIM` non porta collazione, quindi `BINARY` — e deve restare così:
    // la normalizzazione serve a **ordinare** gli artisti, non a fonderli,
    // e il filtro usa la stessa chiave con cui `list_artists` raggruppa.
    // Se un giorno le due dovessero fondersi, va deciso in un posto solo e
    // questa prova cade.
    assert!(
        albums_by_artist(&lib.connection, "tale", 0, 100)
            .expect("album")
            .is_empty(),
        "il raggruppamento e il filtro devono usare la stessa chiave"
    );

    let pagina = albums_by_artist(&lib.connection, "Tale", 1, 1).expect("album");
    assert_eq!(pagina.first().map(|a| a.title.as_str()), Some("Secondo"));
}

#[test]
fn una_scansione_caduta_dopo_un_lotto_non_perde_gli_album() {
    // Il lotto dei brani nuovi si conferma, poi cade la rimozione dei
    // brani spariti (qui un trigger; nella vita un `db.locked`, un disco
    // pieno). Le chiavi toccate dalla passata vivevano solo in memoria:
    // senza riparo, alla passata dopo il brano nuovo è invariato e il suo
    // album non nasce mai, con la libreria segnata «in regola».
    let mut lib = Libreria::nuova();
    lib.brano("A/Al/01.wav", "Uno", "Art1", "Al");
    lib.scansiona();
    lib.brano("B/Bl/01.wav", "Due", "Art2", "Bl");
    std::fs::remove_file(lib.musica().join("A/Al/01.wav")).expect("cancellato");
    lib.connection
        .execute_batch(
            "CREATE TEMP TRIGGER no_delete BEFORE DELETE ON main.tracks
                 BEGIN SELECT RAISE(ABORT, 'prova'); END;",
        )
        .expect("trigger");
    let roots = vec![lib.root()];
    let scan = Scan {
        files: Arc::new(LocalFiles),
        covers: lib.store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules::for_current_platform(),
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };
    assert!(
        scan.run(&mut lib.connection, |_, _| ControlFlow::Continue(()))
            .is_err(),
        "la rimozione cade e la scansione lo dice"
    );
    lib.connection
        .execute_batch("DROP TRIGGER temp.no_delete")
        .expect("via il trigger");

    lib.scansiona();
    let album = |titolo: &str| -> i64 {
        lib.connection
            .query_row(
                "SELECT COUNT(*) FROM albums WHERE title = ?1",
                [titolo],
                |r| r.get(0),
            )
            .expect("conteggio")
    };
    assert_eq!(album("Bl"), 1, "l'album del lotto confermato c'è");
    assert_eq!(album("Al"), 0, "quello del brano sparito no");
}

#[test]
fn gli_album_di_un_artista_sono_quelli_che_list_artists_conta() {
    // Una raccolta senza `album_artist`: tre interpreti, un album solo, il
    // cui artista canonico è il più frequente («B»). «A» compare fra gli
    // artisti con un album, e la sua pagina deve mostrarlo: prima cercava
    // `albums.artist = 'A'` e la griglia restava vuota sotto un titolo che
    // diceva «1 album».
    let mut lib = Libreria::nuova();
    lib.brano("Raccolta/Hits/01.wav", "Uno", "A", "Hits");
    lib.brano("Raccolta/Hits/02.wav", "Due", "B", "Hits");
    lib.brano("Raccolta/Hits/03.wav", "Tre", "B", "Hits");
    lib.brano("B/Suo/01.wav", "Quattro", "B", "Suo");
    lib.scansiona();

    let artisti = list_artists(&lib.connection).expect("artisti");
    let a = artisti.iter().find(|x| x.name == "A").expect("A c'è");
    assert_eq!(a.albums, 1);
    let suoi = albums_by_artist(&lib.connection, "A", 0, 100).expect("album");
    assert_eq!(
        suoi.iter().map(|x| x.title.as_str()).collect::<Vec<_>>(),
        ["Hits"]
    );
    // L'artista del disco resta quello canonico: la raccolta compare nella
    // pagina di ogni interprete, ma è sempre la stessa riga di `albums`.
    assert_eq!(suoi.first().map(|x| x.artist.as_str()), Some("B"));
    // In generale: per ogni artista dell'elenco, la griglia ha tanti album
    // quanti l'intestazione ne promette.
    for artista in &artisti {
        let griglia = albums_by_artist(&lib.connection, &artista.name, 0, 1_000).expect("album");
        assert_eq!(
            i64::try_from(griglia.len()).ok(),
            Some(artista.albums),
            "{}",
            artista.name
        );
    }

    // E passa dall'indice dell'artista effettivo, non da una lettura intera.
    let piano: Vec<String> = {
        let sql = format!("EXPLAIN QUERY PLAN {}", sql_degli_album_di_un_artista());
        let mut s = lib.connection.prepare(&sql).expect("piano");
        let righe = s
            .query_map(rusqlite::params!["A", 100, 0], |r| r.get::<_, String>(3))
            .expect("piano");
        righe.collect::<Result<_, _>>().expect("righe del piano")
    };
    assert!(
        piano
            .iter()
            .any(|r| r.contains("idx_tracks_artista_effettivo")),
        "{piano:?}"
    );
    assert!(!piano.iter().any(|r| r.starts_with("SCAN")), "{piano:?}");
}

#[test]
fn una_copertina_condivisa_si_salva_una_volta_sola() {
    let mut lib = Libreria::nuova();
    let copertina = {
        let mut buffer = image::RgbImage::new(300, 300);
        for (x, y, pixel) in buffer.enumerate_pixels_mut() {
            *pixel = image::Rgb([
                u8::try_from(x % 256).unwrap_or(0),
                u8::try_from(y % 256).unwrap_or(0),
                90,
            ]);
        }
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(buffer)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .expect("codifica");
        out
    };

    for (nome, titolo) in [("01.wav", "Uno"), ("02.wav", "Due")] {
        let path = lib.musica().join("Art/Al").join(nome);
        wav(&path);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title(titolo.to_owned());
        tag.set_artist("Art".to_owned());
        tag.set_album("Al".to_owned());
        tag.push_picture(
            lofty::picture::Picture::unchecked(copertina.clone())
                .pic_type(lofty::picture::PictureType::CoverFront)
                .mime_type(lofty::picture::MimeType::Png)
                .build(),
        );
        tag.save_to_path(&path, WriteOptions::default())
            .expect("tag con copertina");
    }

    let esito = lib.scansiona();
    assert_eq!(esito.covers_stored, 1, "ricodificata una volta");
    assert_eq!(esito.covers_reused, 1, "la seconda era già lì");
    assert_eq!(lib.conta("cover_art"), 1);
    // E l'album la eredita dai suoi brani.
    let hash: Option<String> = lib
        .connection
        .query_row("SELECT cover_art_hash FROM albums", [], |r| r.get(0))
        .expect("album");
    assert!(hash.is_some(), "l'album deve avere la copertina dei brani");
}

#[test]
fn i_dischi_aggiunti_di_recente_sono_uno_per_album_col_piu_nuovo_in_cima() {
    // Il difetto che questa query esiste per togliere: chiedendo i brani per
    // data d'ingresso, le prime dodici righe sono dodici tracce dello stesso
    // disco — perché la musica non entra un brano alla volta, entra una
    // cartella alla volta.
    let mut lib = Libreria::nuova();
    for n in 1..=3 {
        lib.brano(
            &format!("Tale/Vecchio/{n:02}.wav"),
            &format!("Pezzo {n}"),
            "Tale",
            "Vecchio",
        );
    }
    for n in 1..=3 {
        lib.brano(
            &format!("Tale/Nuovo/{n:02}.wav"),
            &format!("Brano {n}"),
            "Tale",
            "Nuovo",
        );
    }
    lib.scansiona();

    // Le date si scrivono a mano perché una scansione sola le mette tutte
    // nello stesso istante: è esattamente il caso in cui l'ordinamento per
    // brano non aveva niente da dire, e quello per disco deve averlo.
    lib.connection
        .execute(
            "UPDATE tracks SET date_added = 100 WHERE album = 'Vecchio'",
            [],
        )
        .expect("date del disco vecchio");
    lib.connection
        .execute(
            "UPDATE tracks SET date_added = 200 WHERE album = 'Nuovo'",
            [],
        )
        .expect("date del disco nuovo");

    let dischi = recently_added_albums(&lib.connection, 12).expect("dischi recenti");

    assert_eq!(dischi.len(), 2, "due dischi, non sei brani");
    assert_eq!(
        dischi.first().map(|a| a.title.as_str()),
        Some("Nuovo"),
        "l'ultimo entrato sta in cima"
    );
    assert_eq!(dischi.get(1).map(|a| a.title.as_str()), Some("Vecchio"));

    // E il limite conta dischi, non tracce: è la misura del ripiano.
    let uno = recently_added_albums(&lib.connection, 1).expect("dischi recenti");
    assert_eq!(uno.len(), 1);
    assert_eq!(uno.first().map(|a| a.title.as_str()), Some("Nuovo"));
}

#[test]
fn a_pari_data_conta_l_ordine_di_scrittura_e_non_il_titolo() {
    // Il caso normale al primo avvio: una cartella sola, una scansione
    // sola, e `date_added` con un valore solo per tutta la libreria. Lì lo
    // spareggio **è** l'ordinamento, e in ordine alfabetico «aggiunti di
    // recente» sarebbe la lettera A.
    let mut lib = Libreria::nuova();
    lib.brano("Tale/Zulu/01.wav", "Uno", "Tale", "Zulu");
    lib.brano("Tale/Alfa/01.wav", "Uno", "Tale", "Alfa");
    lib.scansiona();
    lib.connection
        .execute("UPDATE tracks SET date_added = 500", [])
        .expect("stessa data per tutti");
    // La riga scritta per ultima, qualunque sia il titolo del suo disco.
    let ultimo: String = lib
        .connection
        .query_row(
            "SELECT album FROM tracks ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .expect("l'ultima riga");

    let titoli =
        |dischi: Vec<AlbumSummary>| dischi.into_iter().map(|a| a.title).collect::<Vec<_>>();
    let primo = titoli(recently_added_albums(&lib.connection, 12).expect("dischi"));
    let secondo = titoli(recently_added_albums(&lib.connection, 12).expect("dischi"));

    assert_eq!(
        primo.first().map(String::as_str),
        Some(ultimo.as_str()),
        "davanti va l'ultimo scritto, non il primo in alfabeto"
    );
    assert_eq!(primo.len(), 2);
    // E due volte di fila la stessa risposta: un ripiano che si rimescola da
    // solo fra una visita e l'altra sembra rotto anche quando non lo è.
    assert_eq!(primo, secondo);
}

// ── le radici che non rispondono ────────────────────────────────────────
//
// Il guasto che queste prove tengono chiuso è il più caro di tutti: con la
// radice su una condivisione spenta, `walk` non falliva — restituiva zero
// file — e il piano dichiarava sparito ogni brano che ci stava sotto. La
// scansione dopo li cancellava, e con loro voti, preferiti, conteggi e
// cronologia: le sole cose in libreria che una riscansione non ricostruisce.
//
// Qui non serve un disco vero: serve un disco che **mente** nel modo
// preciso in cui mente una share morta, cioè camminando vuoto.

/// Un disco finto che può dichiararsi irraggiungibile.
struct FintoDisco {
    /// Cosa si trova sotto ogni radice viva.
    trovati: std::collections::HashMap<String, Vec<DiscoveredFile>>,
    /// Le radici che la sonda dichiara morte da subito.
    morte: std::collections::HashSet<String>,
    /// Le radici che rispondono alla prima sonda e non alla seconda: è la
    /// rete che se ne va **fra** la sonda e la camminata, cioè la finestra
    /// che la risonda esiste per chiudere.
    cade_in_corsa: std::collections::HashSet<String>,
    /// Quante sonde sono state fatte finora.
    ///
    /// Un atomico e non un contatore normale perché `MusicFiles` è `Sync` e
    /// la sonda si chiama da dietro un `&self`: una cella mutabile qui non
    /// compilerebbe nemmeno.
    sonde: std::sync::atomic::AtomicUsize,
    /// Radici i cui file non si aprono perché la rete se n'è andata.
    rete_giu: std::collections::HashSet<String>,
    /// Quante volte è stato chiesto di aprire un file.
    ///
    /// È il numero che dice se la scansione ha smesso di insistere: su una
    /// share vera ogni tentativo in più costa una scadenza intera, e le
    /// scadenze non si sommano su un filo solo — si sommano in fili
    /// abbandonati, uno per file.
    aperture: std::sync::atomic::AtomicUsize,
    /// Radici che rispondono ma di cui si vede solo un pezzo.
    ///
    /// La share che muore **durante** la camminata: `walkdir` perde i rami
    /// che non rispondono più e prosegue in silenzio, quindi torna un elenco
    /// che sembra buono e non lo è.
    monche: std::collections::HashSet<String>,
    /// Radici la cui camminata consegna quel che ha e poi **non finisce**.
    ///
    /// Il guasto che nessun `Result` sa dire: la share non fallisce, resta
    /// lì. Un `walk` intero non tornerebbe mai; a rate, il silenzio si vede.
    arenate: std::collections::HashSet<String>,
    /// Radici i cui file si aprono, ma con comodo.
    lente: std::collections::HashSet<String>,
    /// Quanto dorme il disco dove ha promesso di essere lento o arenato.
    ///
    /// Uno solo per tutto il doppio: le prove che lo usano ne hanno bisogno
    /// di un valore solo, e due manopole per la stessa attesa sarebbero due
    /// numeri da tenere allineati a mano.
    attesa: std::time::Duration,
    /// La bandiera che il deposito alza mentre presta la connessione.
    ///
    /// Quando c'è, il disco la guarda a ogni camminata e a ogni apertura:
    /// se la trova alzata, qualcuno sta tenendo il lucchetto mentre si fa
    /// I/O — cioè esattamente il difetto che tutto questo lavoro toglie.
    prestata: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Il disco ha trovato la connessione prestata mentre lavorava.
    ///
    /// Una bandiera e non un `assert!`, perché la camminata gira su un filo
    /// suo: un panico là dentro chiuderebbe il canale, la scansione lo
    /// leggerebbe come «radice arenata» e la prova passerebbe verde senza
    /// aver provato niente.
    sorpresa: std::sync::atomic::AtomicBool,
    /// I rami che la camminata di una radice perde, per nome: la
    /// sottocartella a permessi negati sul disco di casa.
    rami_persi: std::collections::HashMap<String, Vec<String>>,
    /// I file che fanno cadere il lettore dei tag: il file malformato su
    /// cui `lofty` panica.
    malformati: std::collections::HashSet<String>,
}

impl FintoDisco {
    fn nuovo() -> Self {
        Self {
            rami_persi: std::collections::HashMap::new(),
            malformati: std::collections::HashSet::new(),
            trovati: std::collections::HashMap::new(),
            morte: std::collections::HashSet::new(),
            cade_in_corsa: std::collections::HashSet::new(),
            monche: std::collections::HashSet::new(),
            rete_giu: std::collections::HashSet::new(),
            arenate: std::collections::HashSet::new(),
            lente: std::collections::HashSet::new(),
            attesa: std::time::Duration::ZERO,
            prestata: None,
            sorpresa: std::sync::atomic::AtomicBool::new(false),
            aperture: std::sync::atomic::AtomicUsize::new(0),
            sonde: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn con_file(mut self, root: &str, file: Vec<DiscoveredFile>) -> Self {
        self.trovati.insert(root.to_owned(), file);
        self
    }

    fn morta(mut self, root: &str) -> Self {
        self.morte.insert(root.to_owned());
        self
    }

    fn cade_dopo_la_sonda(mut self, root: &str) -> Self {
        self.cade_in_corsa.insert(root.to_owned());
        self
    }

    /// La camminata su questa radice torna quel che le è stato dato, ma
    /// dichiarandosi parziale.
    fn cammina_a_meta(mut self, root: &str) -> Self {
        self.monche.insert(root.to_owned());
        self
    }

    /// La camminata su questa radice perde `ramo`, e lo dice: parziale,
    /// con l'indirizzo del pezzo mancante.
    fn perde_il_ramo(mut self, root: &str, ramo: &str) -> Self {
        self.monche.insert(root.to_owned());
        self.rami_persi
            .entry(root.to_owned())
            .or_default()
            .push(ramo.to_owned());
        self
    }

    /// Aprire questo file fa cadere il lettore.
    fn malformato(mut self, path: &str) -> Self {
        self.malformati.insert(path.to_owned());
        self
    }

    /// I file di questa radice non si aprono: la rete non c'è più.
    fn senza_rete(mut self, root: &str) -> Self {
        self.rete_giu.insert(root.to_owned());
        self
    }

    /// La camminata su questa radice consegna quel che ha e poi si pianta.
    fn si_arena(mut self, root: &str, attesa: std::time::Duration) -> Self {
        self.arenate.insert(root.to_owned());
        self.attesa = attesa;
        self
    }

    /// I file di questa radice si aprono, ma dopo `attesa`.
    fn apre_lentamente(mut self, root: &str, attesa: std::time::Duration) -> Self {
        self.lente.insert(root.to_owned());
        self.attesa = attesa;
        self
    }

    /// Tiene d'occhio la bandiera del deposito mentre lavora.
    fn sorveglia(mut self, prestata: &std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        self.prestata = Some(std::sync::Arc::clone(prestata));
        self
    }

    fn aperture(&self) -> usize {
        self.aperture.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Il disco ha lavorato mentre la connessione era prestata?
    fn sorpresa(&self) -> bool {
        self.sorpresa.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Da chiamare in ogni punto in cui il disco fa I/O.
    fn guarda_il_lucchetto(&self) {
        if let Some(prestata) = self.prestata.as_ref()
            && prestata.load(std::sync::atomic::Ordering::SeqCst)
        {
            self.sorpresa
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Dorme, se la radice di questo percorso ha promesso di essere lenta.
    fn forse_dormi(&self, path: &str) {
        if self
            .lente
            .iter()
            .any(|root| path.to_lowercase().starts_with(&root.to_lowercase()))
        {
            std::thread::sleep(self.attesa);
        }
    }
}

impl MusicFiles for FintoDisco {
    fn walk(&self, root: &str) -> Result<crate::files::Camminata, AppError> {
        self.guarda_il_lucchetto();
        Ok(crate::files::Camminata {
            file: self.trovati.get(root).cloned().unwrap_or_default(),
            completa: !self.monche.contains(root),
        })
    }

    /// Consegna i file uno per uno e, sulle radici arenate, non finisce.
    ///
    /// L'ordine conta: prima si consegna, poi ci si pianta. È il caso vero —
    /// la share risponde per i primi file e poi smette — ed è anche l'unico
    /// che distingua «arenata» da «morta», perché una morta non consegna
    /// niente e la sonda basterebbe a scoprirla.
    fn walk_a_rate(
        &self,
        root: &str,
        su_voce: &mut dyn FnMut(crate::files::Voce) -> ControlFlow<()>,
    ) -> Result<bool, AppError> {
        self.guarda_il_lucchetto();
        for file in self.trovati.get(root).cloned().unwrap_or_default() {
            if su_voce(crate::files::Voce::File(file)).is_break() {
                return Ok(false);
            }
        }
        for ramo in self.rami_persi.get(root).cloned().unwrap_or_default() {
            if su_voce(crate::files::Voce::RamoPerso(ramo)).is_break() {
                return Ok(false);
            }
        }
        if self.arenate.contains(root) {
            std::thread::sleep(self.attesa);
        }
        Ok(!self.monche.contains(root))
    }

    fn open(&self, path: &str) -> Result<Box<dyn crate::files::ReadSeek + Send + Sync>, AppError> {
        self.aperture
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.guarda_il_lucchetto();
        self.forse_dormi(path);
        assert!(
            !self.malformati.contains(path),
            "intestazione malformata: {path}"
        );
        if self
            .rete_giu
            .iter()
            .any(|root| path.to_lowercase().starts_with(&root.to_lowercase()))
        {
            return Err(AppError::new(ErrorCode::FsNetworkUnavailable {
                path: Some(path.to_owned()),
            }));
        }
        Err(AppError::new(ErrorCode::FsNotFound {
            path: path.to_owned(),
        }))
    }

    fn radice_raggiungibile(&self, root: &str) -> bool {
        let prima = self
            .sonde
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if self.morte.contains(root) {
            return false;
        }
        !(self.cade_in_corsa.contains(root) && prima > 0)
    }
}

/// Un file trovato, abbastanza grande da non essere scartato come avanzo.
fn trovato(path: &str, modified_ms: i64) -> DiscoveredFile {
    DiscoveredFile {
        path: path.to_owned(),
        size_bytes: MIN_TRACK_BYTES.saturating_mul(2),
        modified_ms,
    }
}

/// Una riga in `tracks` scritta a mano: qui interessa il percorso, non i tag.
/// Una riga come la scrive una scansione di oggi: con la `content_key`, senza
/// la quale il piano la rileggerebbe (vedi `known_tracks`).
///
/// La dimensione è quella di [`trovato`]: da quando il piano confronta anche
/// quella, una dimensione diversa farebbe rileggere un file «invariato».
fn riga(connection: &Connection, id: i64, path: &str, modified_ms: i64) {
    connection
        .execute(
            "INSERT INTO tracks
                     (id, path, track_key, content_key, title, artist, album, duration_ms,
                      file_size, date_added, date_modified)
                 VALUES (?1, ?2, ?3, ?4, 'T', 'A', 'Al', 1000, 65536, 0, ?5)",
            rusqlite::params![
                id,
                path,
                format!("a|t{id}|al"),
                format!("c{id}"),
                modified_ms
            ],
        )
        .expect("riga di brano");
}

/// Un banco senza disco vero: database in memoria e store delle copertine.
fn banco() -> (tempfile::TempDir, CoverStore, Connection) {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let store = CoverStore::open(dir.path().join("copertine")).expect("store");
    let connection = crate::db::open_in_memory().expect("database").connection;
    (dir, store, connection)
}

#[test]
fn una_radice_che_non_risponde_non_fa_sparire_i_suoi_brani() {
    let (_dir, store, connection) = banco();
    // Due brani sul NAS, uno sul disco interno.
    riga(&connection, 1, r"\\nas\musica\a.mp3", 10);
    riga(&connection, 2, r"\\nas\musica\b.mp3", 10);
    riga(&connection, 3, r"C:\musica\c.mp3", 10);

    let disco = Arc::new(
        FintoDisco::nuovo()
            .con_file(r"C:\musica", vec![trovato(r"C:\musica\c.mp3", 10)])
            .morta(r"\\nas\musica"),
    );
    let roots = vec![r"\\nas\musica".to_owned(), r"C:\musica".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };

    let esplorazione = plan(&scan, &connection).expect("piano");

    assert_eq!(
        esplorazione.radici_saltate,
        vec![r"\\nas\musica".to_owned()],
        "la radice morta va dichiarata, non taciuta"
    );
    assert!(
        esplorazione.piano.to_remove.is_empty(),
        "nessun brano del NAS deve risultare sparito: {:?}",
        esplorazione.piano.to_remove
    );
    assert_eq!(
        esplorazione.piano.untouched, 2,
        "i due del NAS sono fuori competenza, non spariti"
    );
    // E l'altra radice si pianifica normalmente: una share giù non deve
    // fermare la scansione del disco interno.
    assert_eq!(esplorazione.piano.unchanged, 1);
}

#[test]
fn una_radice_che_cade_fra_la_sonda_e_la_camminata_si_salta_lo_stesso() {
    // La finestra stretta: la sonda risponde, poi la rete se ne va, e la
    // camminata torna vuota senza dire perché. Senza la risonda quel vuoto
    // varrebbe «la cartella è stata svuotata».
    let (_dir, store, connection) = banco();
    riga(&connection, 1, r"\\nas\musica\a.mp3", 10);

    let disco = Arc::new(FintoDisco::nuovo().cade_dopo_la_sonda(r"\\nas\musica"));
    let roots = vec![r"\\nas\musica".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };

    let esplorazione = plan(&scan, &connection).expect("piano");
    assert_eq!(
        esplorazione.radici_saltate,
        vec![r"\\nas\musica".to_owned()]
    );
    assert!(esplorazione.piano.to_remove.is_empty());
    assert_eq!(esplorazione.piano.untouched, 1);
}

#[test]
fn una_camminata_a_meta_non_fa_sparire_quel_che_non_ha_visto() {
    // Il caso peggiore dei tre, perché ha l'aria di essere andato bene: la
    // sonda risponde, la camminata **trova roba**, e intanto la share è
    // morta a metà strada. Senza `Camminata::completa` quel che non si è
    // visto diventa «sparito», cioè da cancellare — su una libreria vera
    // sono decine di migliaia di righe, con dentro voti e cronologia.
    let (_dir, store, connection) = banco();
    riga(&connection, 1, r"\\nas\musica\visto.mp3", 10);
    riga(&connection, 2, r"\\nas\musica\non-visto.mp3", 10);
    riga(&connection, 3, r"\\nas\musica\nemmeno-questo.mp3", 10);

    let disco = Arc::new(
        FintoDisco::nuovo()
            // Di tre file ne torna uno: gli altri due stanno nei rami che
            // `walkdir` ha perso quando la rete se n'è andata.
            .con_file(
                r"\\nas\musica",
                vec![trovato(r"\\nas\musica\visto.mp3", 10)],
            )
            .cammina_a_meta(r"\\nas\musica")
            .cade_dopo_la_sonda(r"\\nas\musica"),
    );
    let roots = vec![r"\\nas\musica".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        // Non presidiata **no**: questa è la scansione che l'utente ha
        // chiesto guardando la finestra, quella senza guardia anti-strage.
        // Se la protezione non stesse qui, sotto non ci sarebbe niente.
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };

    let esplorazione = plan(&scan, &connection).expect("piano");
    assert_eq!(
        esplorazione.radici_saltate,
        vec![r"\\nas\musica".to_owned()],
        "una radice vista a metà è una radice non vista"
    );
    assert!(
        esplorazione.piano.to_remove.is_empty(),
        "nessuna rimozione da un elenco parziale: {:?}",
        esplorazione.piano.to_remove
    );
    assert_eq!(
        esplorazione.piano.untouched, 3,
        "tutti e tre fuori competenza, compreso quello che si era visto"
    );
}

#[test]
fn una_camminata_a_meta_su_disco_sano_non_ferma_niente() {
    // Il contrappeso, e serve quanto il test qui sopra: una sottocartella a
    // permessi negati sul disco di casa abbassa `completa` esattamente come
    // una share morta. Lì la sonda risponde, e la scansione deve andare
    // avanti — altrimenti la protezione diventa «non si cancella più
    // niente», che è un altro modo di avere una libreria sbagliata.
    //
    // Avanti **sul resto della radice**: il ramo perso è detto per nome, e
    // solo quel che ci sta sotto resta non verificato.
    let (_dir, store, connection) = banco();
    riga(&connection, 1, r"C:\musica\c-e.mp3", 10);
    riga(&connection, 2, r"C:\musica\non-c-e-piu.mp3", 10);
    riga(&connection, 3, r"C:\musica\Negata\dietro-la-porta.mp3", 10);

    let disco = Arc::new(
        FintoDisco::nuovo()
            .con_file(r"C:\musica", vec![trovato(r"C:\musica\c-e.mp3", 10)])
            .perde_il_ramo(r"C:\musica", r"C:\musica\Negata"),
    );
    let roots = vec![r"C:\musica".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };

    let esplorazione = plan(&scan, &connection).expect("piano");
    assert!(esplorazione.radici_saltate.is_empty());
    let tolte: Vec<i64> = esplorazione
        .piano
        .to_remove
        .iter()
        .map(|r| r.track.id)
        .collect();
    assert_eq!(
        tolte,
        vec![2],
        "la radice risponde: il brano che non c'è più va tolto, quello dietro la porta no"
    );
    assert_eq!(
        esplorazione.rami_persi,
        vec![r"C:\musica\Negata".to_owned()]
    );
    assert_eq!(esplorazione.non_verificati, 1);
    assert_eq!(esplorazione.piano.untouched, 1, "risparmiato, e contato");
}

#[test]
fn una_camminata_monca_che_non_dice_dove_non_toglie_niente_dalla_radice() {
    // Chi si dichiara parziale senza dire dove ha rotto il patto di
    // `walk_a_rate`: lo si prende in parola sulla sola cosa che ha detto, e
    // il ramo perso è la radice intera. La sonda risponde, quindi la radice
    // non è saltata — si inserisce e si aggiorna — ma non si toglie niente.
    let (_dir, store, connection) = banco();
    riga(&connection, 1, r"C:\musica\c-e.mp3", 10);
    riga(&connection, 2, r"C:\musica\forse-c-e.mp3", 10);

    let disco = Arc::new(
        FintoDisco::nuovo()
            .con_file(
                r"C:\musica",
                vec![
                    trovato(r"C:\musica\c-e.mp3", 10),
                    trovato(r"C:\musica\nuovo.mp3", 10),
                ],
            )
            .cammina_a_meta(r"C:\musica"),
    );
    let roots = vec![r"C:\musica".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };

    let esplorazione = plan(&scan, &connection).expect("piano");
    assert!(esplorazione.radici_saltate.is_empty());
    assert!(esplorazione.piano.to_remove.is_empty());
    assert_eq!(
        esplorazione.piano.to_insert.len(),
        1,
        "il nuovo entra lo stesso"
    );
    assert_eq!(esplorazione.rami_persi, vec![r"C:\musica".to_owned()]);
}

#[test]
fn una_sottocartella_illeggibile_non_si_porta_via_brani_e_cronologia() {
    // Il guasto verificato a mano: la radice risponde, una sottocartella
    // no, e una scansione a mano — `prudente: false` — toglieva ogni brano
    // là sotto. La cascata si portava via cronologia, playlist, voti e
    // preferiti. Qui la scansione corre per intero, fino alle rimozioni.
    let (_dir, store, mut connection) = banco();
    riga(&connection, 1, r"C:\musica\c-e.mp3", 10);
    riga(&connection, 2, r"C:\musica\Negata\uno.mp3", 10);
    riga(&connection, 3, r"C:\musica\Negata\Sotto\due.mp3", 10);
    riga(&connection, 4, r"C:\musica\non-c-e-piu.mp3", 10);
    connection
        .execute_batch(
            "UPDATE tracks SET play_count = 7, rating = 4, liked = 1 WHERE id = 2;
                 INSERT INTO play_history (track_id, played_at, ms_played) VALUES (2, 1000, 5000);
                 INSERT INTO play_history (track_id, played_at, ms_played) VALUES (3, 2000, 5000);",
        )
        .expect("storia");

    let disco = Arc::new(
        FintoDisco::nuovo()
            .con_file(r"C:\musica", vec![trovato(r"C:\musica\c-e.mp3", 10)])
            .perde_il_ramo(r"C:\musica", r"C:\musica\Negata"),
    );
    let roots = vec![r"C:\musica".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };

    let report = scan
        .run(&mut connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione");

    assert_eq!(report.removed, 1, "solo quello che è sparito davvero");
    assert_eq!(report.non_verificati, 2);
    assert_eq!(report.rami_persi, vec![r"C:\musica\Negata".to_owned()]);
    let rimasti: Vec<i64> = {
        let mut statement = connection
            .prepare("SELECT id FROM tracks ORDER BY id")
            .expect("query");
        statement
            .query_map([], |r| r.get(0))
            .expect("righe")
            .collect::<Result<_, _>>()
            .expect("id")
    };
    assert_eq!(rimasti, vec![1, 2, 3]);
    let (ascolti, voto, cuore): (i64, i64, i64) = connection
        .query_row(
            "SELECT play_count, rating, liked FROM tracks WHERE id = 2",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("statistiche");
    assert_eq!((ascolti, voto, cuore), (7, 4, 1), "né tolte né riscritte");
    let cronologia: i64 = connection
        .query_row("SELECT COUNT(*) FROM play_history", [], |r| r.get(0))
        .expect("cronologia");
    assert_eq!(cronologia, 2, "la cronologia dietro la porta è ancora lì");
}

#[test]
fn una_ricostruzione_caduta_chiede_quella_intera() {
    // I brani di una passata si scrivono prima degli aggregati, in
    // transazioni già chiuse: se la ricostruzione cade, la libreria non
    // può restare segnata in regola con gli album di prima. È il caso delle
    // chiavi d'album doppie della 3.0 — un guasto che si ripeteva identico
    // a ogni passata — ma anche di un `db.locked` qualunque.
    let mut lib = Libreria::nuova();
    lib.brano("Artista/Disco/uno.wav", "Uno", "Artista", "Disco");
    lib.scansiona();
    assert!(aggregati_in_regola(&lib.connection).expect("versione"));

    // Da qui ogni album scritto fallisce, come falliva la riga doppia.
    lib.connection
        .execute_batch(
            "CREATE TEMP TRIGGER album_rifiutati BEFORE INSERT ON main.albums
                 BEGIN SELECT RAISE(ABORT, 'album rifiutato dalla prova'); END;",
        )
        .expect("trigger");
    lib.brano("Artista/Disco/due.wav", "Due", "Artista", "Disco");
    let roots = vec![lib.root()];
    let caduta = Scan {
        files: Arc::new(LocalFiles),
        covers: lib.store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules::for_current_platform(),
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    }
    .run(&mut lib.connection, |_, _| ControlFlow::Continue(()));
    assert!(caduta.is_err(), "la ricostruzione doveva cadere");
    let brani: i64 = lib
        .connection
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .expect("conteggio");
    assert_eq!(brani, 2, "i brani della passata restano scritti");
    assert!(
        !aggregati_in_regola(&lib.connection).expect("versione"),
        "la prossima ricostruzione deve essere intera"
    );

    // Tolto il guasto, la passata dopo rimette tutto in regola: la
    // ricostruzione intera trova il brano nuovo che quella per chiavi
    // aveva perso insieme alla sua transazione.
    lib.connection
        .execute_batch("DROP TRIGGER temp.album_rifiutati;")
        .expect("via il trigger");
    lib.scansiona();
    assert!(aggregati_in_regola(&lib.connection).expect("versione"));
    let (album, nel_disco): (i64, i64) = lib
        .connection
        .query_row("SELECT COUNT(*), MAX(total_tracks) FROM albums", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .expect("album");
    assert_eq!(
        (album, nel_disco),
        (1, 2),
        "un disco, con tutti e due i brani"
    );
}
#[test]
fn tre_file_malformati_di_fila_non_fanno_abbandonare_la_radice() {
    // Il lettore dei tag che panica su un file malformato tornava lo
    // stesso `None` di una lettura scaduta, e contava fra le scadenze di
    // fila: tre file rotti in una cartella del disco di casa, e la radice
    // veniva abbandonata come «rete non disponibile».
    let (_dir, store, mut connection) = banco();
    let disco = Arc::new(
        FintoDisco::nuovo()
            .con_file(
                r"C:\musica",
                vec![
                    trovato(r"C:\musica\rotto-1.mp3", 10),
                    trovato(r"C:\musica\rotto-2.mp3", 10),
                    trovato(r"C:\musica\rotto-3.mp3", 10),
                    trovato(r"C:\musica\rotto-4.mp3", 10),
                    trovato(r"C:\musica\dopo.mp3", 10),
                ],
            )
            .malformato(r"C:\musica\rotto-1.mp3")
            .malformato(r"C:\musica\rotto-2.mp3")
            .malformato(r"C:\musica\rotto-3.mp3")
            .malformato(r"C:\musica\rotto-4.mp3"),
    );
    let roots = vec![r"C:\musica".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };

    let prima = Instant::now();
    let report = scan
        .run(&mut connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione");

    assert!(
        report.radici_saltate.is_empty(),
        "una radice di file rotti non è una radice morta"
    );
    assert_eq!(
        disco.aperture(),
        5,
        "si prova ogni file, anche dopo i rotti"
    );
    assert_eq!(report.unreadable.len(), 5);
    let cadute = report
        .unreadable
        .iter()
        .filter(|u| u.error.code().kind() == ErrorCodeKind::MetadataTagReadFailed)
        .count();
    assert_eq!(
        cadute, 4,
        "i quattro rotti si dicono file illeggibili, non rete"
    );
    // E nessuno ha aspettato la scadenza di lettura: una caduta si
    // riconosce subito.
    assert!(
        prima.elapsed() < LETTURA_BRANO,
        "una caduta ha aspettato la scadenza: {:?}",
        prima.elapsed()
    );
}

#[test]
fn una_radice_che_muore_a_meta_lettura_si_abbandona_invece_di_insistere() {
    // La rete se ne va **dopo** che la scansione è cominciata: la sonda
    // iniziale aveva detto sì, la camminata ha trovato i file, e il guasto
    // arriva alla prima apertura. Due cose devono succedere, e nessuna delle
    // due succedeva: non si insiste sugli altri file — su una share vera
    // ognuno costerebbe la scadenza di lettura per intero, e un filo
    // abbandonato a testa — e i brani di quella radice non si cancellano,
    // perché non è stato dimostrato che non ci siano, solo che non si
    // riesce a guardarli.
    let (_dir, store, mut connection) = banco();
    riga(&connection, 1, r"\\nas\musica\sparito-1.mp3", 10);
    riga(&connection, 2, r"\\nas\musica\sparito-2.mp3", 10);

    let disco = Arc::new(
        FintoDisco::nuovo()
            .con_file(
                r"\\nas\musica",
                vec![
                    trovato(r"\\nas\musica\nuovo-1.mp3", 10),
                    trovato(r"\\nas\musica\nuovo-2.mp3", 10),
                    trovato(r"\\nas\musica\nuovo-3.mp3", 10),
                ],
            )
            .senza_rete(r"\\nas\musica")
            // La sonda risponde la prima volta — quella della camminata — e
            // non la seconda, che è quella chiesta dal guasto in lettura.
            .cade_dopo_la_sonda(r"\\nas\musica"),
    );
    let roots = vec![r"\\nas\musica".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };

    let report = scan
        .run(&mut connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione");

    assert_eq!(
        disco.aperture(),
        1,
        "il primo file paga l'attesa e la scopre per tutti: gli altri due              non vanno nemmeno tentati"
    );
    assert_eq!(
        report.removed, 0,
        "i brani di una radice che non risponde non sono brani spariti"
    );
    assert_eq!(
        report.radici_saltate,
        vec![r"\\nas\musica".to_owned()],
        "e la passata deve dire che non ha guardato lì"
    );
    assert!(
        report.unreadable.is_empty(),
        "un guasto di rete non è un elenco di file illeggibili: {:?}",
        report.unreadable
    );
}

#[test]
fn un_singhiozzo_di_rete_non_fa_abbandonare_la_radice() {
    // Il contrappeso: la rete dà un errore su un file, ma la radice alla
    // sonda risponde ancora. Non è una share morta, è un incidente — e la
    // scansione deve proseguire sugli altri file, elencando quello perso
    // per nome invece di dichiarare morta l'intera cartella.
    let (_dir, store, mut connection) = banco();

    let disco = Arc::new(
        FintoDisco::nuovo()
            .con_file(
                r"\\nas\musica",
                vec![
                    trovato(r"\\nas\musica\a.mp3", 10),
                    trovato(r"\\nas\musica\b.mp3", 10),
                ],
            )
            .senza_rete(r"\\nas\musica"),
    );
    let roots = vec![r"\\nas\musica".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };

    let report = scan
        .run(&mut connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione");

    assert_eq!(disco.aperture(), 2, "si prova ogni file, non solo il primo");
    assert_eq!(report.unreadable.len(), 2);
    assert!(report.radici_saltate.is_empty());
}

#[test]
fn una_radice_davvero_vuota_resta_una_radice_vuota() {
    // Il contrario del caso sopra, ed è quel che impedisce alla protezione
    // di diventare «non si cancella più niente»: la cartella risponde, ci si
    // è guardato dentro, non c'è nulla. Quei brani vanno tolti.
    let (_dir, store, connection) = banco();
    riga(&connection, 1, r"C:\musica\a.mp3", 10);

    let disco = Arc::new(FintoDisco::nuovo());
    let roots = vec![r"C:\musica".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };

    let esplorazione = plan(&scan, &connection).expect("piano");
    assert!(esplorazione.radici_saltate.is_empty());
    assert_eq!(esplorazione.piano.to_remove.len(), 1);
}

#[test]
fn una_scansione_non_presidiata_non_toglie_mezza_libreria_da_sola() {
    // La seconda difesa, quella che vale anche per i casi che la sonda non
    // vede: un punto di mount che risponde ed è vuoto, un disco che non si è
    // montato all'avvio. Nessuno sta guardando, quindi nel dubbio si rimanda.
    let (_dir, store, mut connection) = banco();
    for id in 1..=60 {
        riga(&connection, id, &format!(r"C:\musica\{id}.mp3"), 10);
    }
    // Uno solo è rimasto al suo posto: la radice non è vuota, quindi non è
    // la guardia per radice a fermare tutto — vedi la prova dopo — ma
    // quella contro le stragi.
    riga(&connection, 61, r"C:\musica\resta.mp3", 10);

    let disco = Arc::new(
        FintoDisco::nuovo().con_file(r"C:\musica", vec![trovato(r"C:\musica\resta.mp3", 10)]),
    );
    let roots = vec![r"C:\musica".to_owned()];
    let prudente = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: true,
        scadenze: Scadenze::default(),
        fermati: None,
    };

    let esito = prudente
        .run(&mut connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione");
    assert_eq!(esito.removed, 0, "non doveva togliere niente");
    assert_eq!(esito.rimozioni_rinviate, 60);
    assert!(
        esito.rimozioni_in_attesa.is_empty(),
        "nessuno può confermare: l'elenco non serve a nessuno"
    );
    let rimasti: i64 = connection
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .expect("conteggio");
    assert_eq!(rimasti, 61, "le righe sono ancora tutte lì");
}

#[test]
fn una_radice_vuota_che_aveva_cinquanta_brani_vale_saltata_se_nessuno_guarda() {
    // Il punto di mount che risponde vuoto: la sonda dice sì, la camminata
    // dice niente. Una scansione partita da sola — al risveglio, al ritorno
    // della rete — non deve nemmeno arrivare a contare le rimozioni: la
    // radice vale saltata, e i suoi brani restano fuori competenza.
    let (_dir, store, mut connection) = banco();
    // Dieci più della soglia: abbastanza anche per la guardia contro le
    // stragi, che scatta **sopra** le cinquanta.
    let quante = RADICE_SOSPETTA + 10;
    for id in 1..=quante {
        let id = i64::try_from(id).expect("pochi");
        riga(&connection, id, &format!(r"\\nas\musica\{id}.mp3"), 10);
    }
    let disco = Arc::new(FintoDisco::nuovo());
    let roots = vec![r"\\nas\musica".to_owned()];
    let scansione = |prudente: bool| Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente,
        scadenze: Scadenze::default(),
        fermati: None,
    };

    let esito = scansione(true)
        .run(&mut connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione prudente");
    assert_eq!(esito.radici_saltate, vec![r"\\nas\musica".to_owned()]);
    // Vuota e non muta: chi riprova deve poter distinguere le due.
    assert_eq!(esito.radici_vuote, vec![r"\\nas\musica".to_owned()]);
    assert_eq!(esito.removed, 0);
    assert_eq!(
        esito.rimozioni_rinviate, 0,
        "niente da rinviare: non si è giudicato"
    );
    assert_eq!(esito.plan.untouched, quante);

    // A mano la guardia per radice non c'è: chi guarda può dire di sì, e
    // la guardia contro le stragi gli chiede conferma.
    let esito = scansione(false)
        .run(&mut connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione a mano");
    assert!(esito.radici_saltate.is_empty());
    assert!(esito.radici_vuote.is_empty());
    assert_eq!(esito.rimozioni_in_attesa.len(), quante);
    assert_eq!(esito.removed, 0);

    // La cartella sorvegliata scritta in un'altra grafia nelle impostazioni
    // è la stessa: la guardia vale per chiave di percorso.
    let libreria = vec![r"\\NAS\Musica\".to_owned()];
    let altra_grafia = Scan {
        radici_della_libreria: &libreria,
        ..scansione(true)
    };
    let esito = altra_grafia
        .run(&mut connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione prudente");
    assert_eq!(esito.radici_vuote, vec![r"\\nas\musica".to_owned()]);
    assert_eq!(esito.removed, 0);
}

#[test]
fn una_radice_vuota_con_meno_di_cinquanta_brani_si_svuota_davvero() {
    // Sotto la soglia una cartella vuota è plausibile — una chiavetta
    // svuotata di proposito — e la guardia non scatta nemmeno in una
    // scansione automatica.
    let (_dir, store, mut connection) = banco();
    for id in 1..RADICE_SOSPETTA {
        let id = i64::try_from(id).expect("pochi");
        riga(&connection, id, &format!(r"E:\chiavetta\{id}.mp3"), 10);
    }
    let disco = Arc::new(FintoDisco::nuovo());
    let roots = vec![r"E:\chiavetta".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: true,
        scadenze: Scadenze::default(),
        fermati: None,
    };
    let esito = scan
        .run(&mut connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione");
    assert!(esito.radici_saltate.is_empty());
    assert_eq!(esito.removed, RADICE_SOSPETTA - 1);
}

#[test]
fn una_scansione_a_mano_chiede_prima_di_togliere_mezza_libreria() {
    // La stessa strage, chiesta a mano. Fino alla 2.4 toglieva senza
    // chiedere: chi guardava l'esito leggeva «60 tolti» a cose fatte. Ora
    // non toglie niente, finisce il resto del lavoro, e consegna l'elenco
    // a chi può dire sì.
    let (_dir, store, mut connection) = banco();
    for id in 1..=60 {
        riga(&connection, id, &format!(r"C:\musica\{id}.mp3"), 10);
    }
    riga(&connection, 61, r"D:\altro\resta.mp3", 10);

    let disco = Arc::new(FintoDisco::nuovo().con_file(
        r"D:\altro",
        vec![
            trovato(r"D:\altro\resta.mp3", 10),
            trovato(r"D:\altro\nuovo.mp3", 10),
        ],
    ));
    let roots = vec![r"C:\musica".to_owned(), r"D:\altro".to_owned()];
    let a_mano = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };

    let esito = a_mano
        .run(&mut connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione");
    assert_eq!(esito.removed, 0, "senza un sì non si toglie niente");
    assert_eq!(esito.rimozioni_rinviate, 60);
    assert_eq!(esito.rimozioni_in_attesa.len(), 60);
    // Il resto della passata è andato avanti: la radice sana si legge.
    // (Il finto disco non apre file: il nuovo è tentato e resta illeggibile.)
    assert_eq!(
        disco.aperture(),
        1,
        "il file nuovo dell'altra radice è stato tentato"
    );

    // Quel che la finestra mostra: una voce per radice, con gli esempi.
    let riassunto = riassumi_rimozioni(
        &esito.rimozioni_in_attesa,
        &roots,
        PathRules {
            case_insensitive: true,
        },
        3,
    );
    assert_eq!(riassunto.len(), 1);
    let voce = riassunto.first().expect("una voce");
    assert_eq!(voce.radice, r"C:\musica");
    assert_eq!(voce.quante, 60);
    assert_eq!(voce.esempi.len(), 3);

    // Il sì: si tolgono esattamente quelle, senza ricamminare il disco.
    let camminate_prima = disco.sonde.load(std::sync::atomic::Ordering::Relaxed);
    let tolte = applica_rimozioni(&mut connection, &esito.rimozioni_in_attesa).expect("conferma");
    assert_eq!(tolte, 60);
    assert_eq!(
        disco.sonde.load(std::sync::atomic::Ordering::Relaxed),
        camminate_prima,
        "confermare non guarda il disco"
    );
    let rimasti: i64 = connection
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .expect("conteggio");
    assert_eq!(rimasti, 1, "resta solo quello dell'altra radice");
}

#[test]
fn una_conferma_arrivata_tardi_non_tocca_una_riga_cambiata() {
    // Fra la domanda e il sì passa il tempo di leggere una finestrella, e
    // in quel tempo la riga 1 può essere diventata un'altra: si toglie per
    // identificativo **e** percorso, e quella non si riconosce più.
    let (_dir, _store, mut connection) = banco();
    riga(&connection, 1, r"C:\musica\a.mp3", 10);
    riga(&connection, 2, r"C:\musica\b.mp3", 10);
    let elenco = vec![
        RimozioneInAttesa {
            id: 1,
            path: r"C:\musica\a.mp3".to_owned(),
        },
        RimozioneInAttesa {
            id: 2,
            path: r"C:\musica\b.mp3".to_owned(),
        },
    ];
    connection
        .execute(
            "UPDATE tracks SET path = ?1 WHERE id = 1",
            [r"C:\musica\rinominato.mp3"],
        )
        .expect("rinomina");
    let tolte = applica_rimozioni(&mut connection, &elenco).expect("conferma");
    assert_eq!(tolte, 1);
    let rimasto: String = connection
        .query_row("SELECT path FROM tracks", [], |r| r.get(0))
        .expect("rimasto");
    assert_eq!(rimasto, r"C:\musica\rinominato.mp3");
}

// ── la migrazione 028, trigger per trigger ──────────────────────────────

/// Una riga di brano con la `content_key` scelta da chi prova.
fn riga_con_chiave(connection: &Connection, id: i64, path: &str, chiave: &str) {
    connection
        .execute(
            "INSERT INTO tracks
                     (id, path, track_key, content_key, title, artist, album, duration_ms,
                      file_size, date_added, date_modified)
                 VALUES (?1, ?2, 'a|t|al', ?3, 'T', 'A', 'Al', 1000, 65536, 0, 10)",
            rusqlite::params![id, path, chiave],
        )
        .expect("riga di brano");
}

fn conta(connection: &Connection, tabella: &str) -> i64 {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {tabella}"), [], |r| r.get(0))
        .expect("conteggio")
}

#[test]
fn un_brano_senza_storia_non_lascia_orfani() {
    // Solo chi ha qualcosa da perdere: una cartella tolta con diecimila
    // brani mai ascoltati non deve scrivere diecimila righe vuote.
    let (_dir, _store, connection) = banco();
    riga_con_chiave(&connection, 1, r"C:\m\a.mp3", "k1");
    connection
        .execute("DELETE FROM tracks WHERE id = 1", [])
        .expect("tolto");
    assert_eq!(conta(&connection, "brani_orfani"), 0);
}

#[test]
fn il_brano_che_torna_va_in_coda_se_il_suo_posto_e_stato_preso() {
    let (_dir, _store, mut connection) = banco();
    riga_con_chiave(&connection, 1, r"C:\m\a.mp3", "ka");
    riga_con_chiave(&connection, 2, r"C:\m\b.mp3", "kb");
    riga_con_chiave(&connection, 3, r"C:\m\c.mp3", "kc");
    let lista = crate::playlists::create(&connection, "Lista")
        .expect("playlist")
        .id;
    crate::playlists::add_tracks(&mut connection, lista, &[1, 2, 3]).expect("aggiunti");
    connection
        .execute("DELETE FROM tracks WHERE id = 1", [])
        .expect("tolto");
    // Un riordino riscrive le posizioni da zero: il buco dove stava «a» è
    // stato preso da «b».
    crate::playlists::reorder(&mut connection, lista, 1, 0).expect("riordinata");

    riga_con_chiave(&connection, 4, r"C:\m\a.mp3", "ka");
    let ordine: Vec<i64> = {
        let mut statement = connection
            .prepare(
                "SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position",
            )
            .expect("query");
        statement
            .query_map([lista], |r| r.get(0))
            .expect("righe")
            .collect::<Result<_, _>>()
            .expect("ordine")
    };
    assert_eq!(
        ordine,
        vec![3, 2, 4],
        "in fondo, non sopra un altro e non fuori"
    );
}

#[test]
fn due_copie_dello_stesso_file_si_fondono() {
    // Lo stesso album in due cartelle, tolte tutte e due: una voce sola,
    // col conteggio più alto, il voto più recente e tutti gli ascolti.
    let (_dir, _store, connection) = banco();
    riga_con_chiave(&connection, 1, r"C:\m\a.mp3", "k");
    riga_con_chiave(&connection, 2, r"D:\copia\a.mp3", "k");
    connection
        .execute_batch(
            "UPDATE tracks SET play_count = 3, rating = 2, stats_updated_at = 100 WHERE id = 1;
                 UPDATE tracks SET play_count = 9, rating = 5, stats_updated_at = 50 WHERE id = 2;
                 INSERT INTO play_history (track_id, played_at, ms_played) VALUES (1, 10, 1);
                 INSERT INTO play_history (track_id, played_at, ms_played) VALUES (2, 20, 1);
                 DELETE FROM tracks;",
        )
        .expect("tolte");
    let (quante, voto): (i64, i64) = connection
        .query_row(
            "SELECT play_count, rating FROM brani_orfani WHERE content_key = 'k'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("orfano");
    assert_eq!(quante, 9, "il conteggio più alto");
    assert_eq!(voto, 2, "il voto con l'orologio più recente");
    assert_eq!(conta(&connection, "ascolti_orfani"), 2);
}

#[test]
fn un_orfano_scaduto_non_torna_e_se_ne_va() {
    let (_dir, _store, connection) = banco();
    riga_con_chiave(&connection, 1, r"C:\m\a.mp3", "vecchio");
    riga_con_chiave(&connection, 2, r"C:\m\b.mp3", "recente");
    connection
        .execute_batch(
            "UPDATE tracks SET play_count = 4;
                 DELETE FROM tracks WHERE id = 1;",
        )
        .expect("tolto");
    // Novantun giorni fa.
    connection
        .execute(
            "UPDATE brani_orfani
                    SET orfano_at = (CAST(strftime('%s', 'now') AS INTEGER) - 91 * 86400) * 1000",
            [],
        )
        .expect("invecchiato");

    // Torna dopo la scadenza: niente statistiche, e la voce se ne va.
    riga_con_chiave(&connection, 3, r"C:\m\a.mp3", "vecchio");
    let quante: i64 = connection
        .query_row("SELECT play_count FROM tracks WHERE id = 3", [], |r| {
            r.get(0)
        })
        .expect("riga");
    assert_eq!(quante, 0, "scaduto non torna");
    assert_eq!(conta(&connection, "brani_orfani"), 0);

    // E la pulizia passa anche da chi se ne va: una voce scaduta sparisce
    // alla prima rimozione successiva, qualunque sia.
    connection
        .execute("DELETE FROM tracks WHERE id = 3", [])
        .expect("tolto");
    connection
        .execute(
            "INSERT INTO brani_orfani (content_key, play_count, orfano_at)
                 VALUES ('dimenticato', 1, 0)",
            [],
        )
        .expect("orfano antico");
    connection
        .execute("DELETE FROM tracks WHERE id = 2", [])
        .expect("tolto");
    let chiavi: Vec<String> = {
        let mut statement = connection
            .prepare("SELECT content_key FROM brani_orfani ORDER BY content_key")
            .expect("query");
        statement
            .query_map([], |r| r.get(0))
            .expect("righe")
            .collect::<Result<_, _>>()
            .expect("chiavi")
    };
    assert_eq!(chiavi, vec!["recente".to_owned()]);
}

#[test]
fn un_brano_di_catalogo_non_lascia_orfani() {
    // Nessuna scansione lo farà tornare: tenerne la storia novanta giorni
    // vorrebbe dire tenerla per nessuno.
    let (_dir, _store, connection) = banco();
    connection
        .execute(
            "INSERT INTO tracks
                     (id, path, track_key, content_key, title, artist, album, duration_ms,
                      date_added, play_count, source, source_service, fonte_url,
                      licenza, disponibilita)
                 VALUES (1, NULL, 'a|t|al', 'k', 'T', 'A', 'Al', 1000, 0, 3,
                         'catalogo', 'audius', '/v1/tracks/1/stream',
                         'cc-by', 'soloAscolto')",
            [],
        )
        .expect("brano di catalogo");
    connection
        .execute("DELETE FROM tracks WHERE id = 1", [])
        .expect("tolto");
    assert_eq!(conta(&connection, "brani_orfani"), 0);
}

#[test]
fn la_guardia_non_scatta_sulle_rimozioni_normali() {
    // Cancellare un album non è una strage: sotto la soglia la scansione
    // automatica toglie come ha sempre fatto, altrimenti la protezione
    // diventerebbe una libreria che non si aggiorna più da sola.
    let (_dir, store, mut connection) = banco();
    for id in 1..=100 {
        riga(&connection, id, &format!(r"C:\musica\{id}.mp3"), 10);
    }
    // Novanta restano al loro posto, dieci sono spariti.
    let presenti: Vec<DiscoveredFile> = (1..=90)
        .map(|id| trovato(&format!(r"C:\musica\{id}.mp3"), 10))
        .collect();
    let disco = Arc::new(FintoDisco::nuovo().con_file(r"C:\musica", presenti));
    let roots = vec![r"C:\musica".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: true,
        scadenze: Scadenze::default(),
        fermati: None,
    };

    let esito = scan
        .run(&mut connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione");
    assert_eq!(esito.removed, 10);
    assert_eq!(esito.rimozioni_rinviate, 0);
}
// ── il tempo, e il lucchetto ────────────────────────────────────────────
//
// Le prove qui sopra tengono chiuso il guasto in cui la rete **fallisce**.
// Queste tengono chiuso quello in cui la rete **aspetta**, che è più
// insidioso perché non produce nessun errore: la finestra si ferma, Windows
// dichiara «non risponde», e l'utente termina il processo.

/// Un deposito che conta le prese e dice quando è in mano a qualcuno.
///
/// Le due cose insieme, perché la domanda è una sola: la scansione chiede la
/// connessione **abbastanza spesso** da lavorare, e **mai** mentre guarda il
/// disco.
struct DepositoContato {
    connection: Connection,
    /// Alzata per tutta la durata di una presa.
    ///
    /// La legge il finto disco: se la trova alzata mentre cammina o mentre
    /// apre un file, qualcuno sta tenendo il lucchetto durante l'I/O — cioè
    /// esattamente il difetto per cui esiste [`Deposito`].
    prestata: Arc<std::sync::atomic::AtomicBool>,
    /// Quante volte la connessione è stata chiesta.
    prestiti: usize,
}

impl DepositoContato {
    fn nuovo(connection: Connection) -> Self {
        Self {
            connection,
            prestata: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            prestiti: 0,
        }
    }
}

impl Deposito for DepositoContato {
    fn con_connessione<T, F>(&mut self, azione: F) -> Result<T, AppError>
    where
        F: FnOnce(&mut Connection) -> Result<T, AppError>,
    {
        self.prestiti += 1;
        self.prestata
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let esito = azione(&mut self.connection);
        self.prestata
            .store(false, std::sync::atomic::Ordering::SeqCst);
        esito
    }
}

/// Un deposito che cambia le carte in tavola fra due prese.
///
/// È il mondo vero, non un dispetto: lasciare la connessione fra una fase e
/// l'altra significa che un'altra scrittura può passare in mezzo, e questa
/// prova mette quella scrittura esattamente dove fa più danno.
struct DepositoDispettoso {
    connection: Connection,
    prestiti: usize,
    /// Dopo la presa numero questa, la riga 1 cambia percorso.
    quando: usize,
}

impl Deposito for DepositoDispettoso {
    fn con_connessione<T, F>(&mut self, azione: F) -> Result<T, AppError>
    where
        F: FnOnce(&mut Connection) -> Result<T, AppError>,
    {
        self.prestiti += 1;
        let esito = azione(&mut self.connection);
        if self.prestiti == self.quando {
            self.connection
                .execute(
                    "UPDATE tracks SET path = ?1 WHERE id = 1",
                    [r"C:\musica\rinominato.mp3"],
                )
                .expect("rinomina");
        }
        esito
    }
}

/// Scadenze cortissime: una prova non può aspettare quindici secondi.
fn svelte() -> Scadenze {
    Scadenze {
        camminata: Duration::from_millis(100),
        brano: Duration::from_millis(100),
    }
}

#[test]
fn una_camminata_che_si_arena_non_fa_sparire_i_suoi_brani() {
    // Il guasto nuovo, e il più caro: la share non è spenta — risponde alla
    // sonda — ma la camminata dentro non finisce. Con un `walk` intero la
    // scansione starebbe lì finché Windows non rinuncia da solo, tenendo
    // ferma la finestra; e se qualcuno la interrompesse, i brani non
    // enumerati diventerebbero «spariti».
    let (_dir, store, mut connection) = banco();
    riga(&connection, 1, r"\\nas\musica\a.mp3", 10);
    riga(&connection, 2, r"\\nas\musica\b.mp3", 10);
    riga(&connection, 3, r"\\nas\musica\c.mp3", 10);

    // Nessun file consegnato, e poi il silenzio. La sonda dice sempre di sì:
    // è il punto: la radice **risponde**, quindi risondarla non scoprirebbe
    // niente.
    let disco = Arc::new(FintoDisco::nuovo().si_arena(r"\\nas\musica", Duration::from_secs(5)));
    let roots = vec![r"\\nas\musica".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: false,
        scadenze: svelte(),
        fermati: None,
    };

    let prima = std::time::Instant::now();
    let report = scan
        .run(&mut connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione");
    let passato = prima.elapsed();

    assert!(
        passato < Duration::from_secs(1),
        "ha aspettato la fine della camminata: {passato:?}"
    );
    assert_eq!(
        report.radici_saltate,
        vec![r"\\nas\musica".to_owned()],
        "una radice che non finisce di camminare è una radice non vista"
    );
    assert_eq!(
        report.removed, 0,
        "i suoi brani non sono spariti: non si è potuto guardare"
    );
    assert_eq!(
        report.plan.untouched, 3,
        "tutti e tre fuori competenza, non da cancellare"
    );
    let rimasti: i64 = connection
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .expect("conteggio");
    assert_eq!(rimasti, 3, "le righe sono ancora tutte lì");
}

#[test]
fn un_brano_che_non_si_apre_in_tempo_vale_una_radice_che_non_risponde() {
    // La share che smette **dopo** la camminata: i file sono stati elencati,
    // e poi le aperture non tornano più. Un'attesa che non finisce non è un
    // errore che qualcuno riporti, quindi va trasformata in uno — e da lì in
    // poi vale quel che vale un guasto di rete: si chiede alla radice come
    // sta, e se non risponde si smette di insistere.
    let (_dir, store, mut connection) = banco();

    let disco = Arc::new(
        FintoDisco::nuovo()
            .con_file(
                r"\\nas\musica",
                vec![
                    trovato(r"\\nas\musica\uno.mp3", 10),
                    trovato(r"\\nas\musica\due.mp3", 10),
                    trovato(r"\\nas\musica\tre.mp3", 10),
                ],
            )
            .apre_lentamente(r"\\nas\musica", Duration::from_secs(5))
            // La sonda risponde alla camminata e non alla domanda che nasce
            // dalla lettura scaduta: la rete se n'è andata in mezzo.
            .cade_dopo_la_sonda(r"\\nas\musica"),
    );
    let roots = vec![r"\\nas\musica".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: false,
        scadenze: svelte(),
        fermati: None,
    };

    let report = scan
        .run(&mut connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione");

    assert_eq!(
        disco.aperture(),
        1,
        "il primo file paga l'attesa e la scopre per tutti"
    );
    assert!(
        report.unreadable.is_empty(),
        "una share morta non è un elenco di file illeggibili: {:?}",
        report.unreadable
    );
    assert_eq!(
        report.radici_saltate,
        vec![r"\\nas\musica".to_owned()],
        "e la passata deve dire che non ha guardato lì"
    );
}

#[test]
fn un_brano_lento_su_una_radice_viva_resta_un_file_illeggibile() {
    // Il contrappeso, e serve quanto la prova qui sopra: un file che non si
    // apre in tempo su una cartella che risponde è un incidente di quel
    // file, non una diagnosi sulla cartella. Se la scadenza abbandonasse la
    // radice da sé, un disco lento — una chiavetta USB, un file enorme —
    // farebbe sparire dalla scansione tutto quel che ci sta sopra.
    let (_dir, store, mut connection) = banco();

    let disco = Arc::new(
        FintoDisco::nuovo()
            .con_file(
                r"\\nas\musica",
                vec![
                    trovato(r"\\nas\musica\a.mp3", 10),
                    trovato(r"\\nas\musica\b.mp3", 10),
                ],
            )
            // Lento, ma vivo: la sonda dice sempre di sì.
            .apre_lentamente(r"\\nas\musica", Duration::from_secs(5)),
    );
    let roots = vec![r"\\nas\musica".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: false,
        scadenze: svelte(),
        fermati: None,
    };

    let report = scan
        .run(&mut connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione");

    assert_eq!(disco.aperture(), 2, "si prova ogni file, non solo il primo");
    assert_eq!(
        report.unreadable.len(),
        2,
        "due file che non sono arrivati in tempo, detti per nome"
    );
    assert!(
        report.radici_saltate.is_empty(),
        "la cartella risponde: non si abbandona"
    );
}

#[test]
fn tre_scadenze_di_fila_valgono_una_radice_che_non_risponde() {
    // Dove finisce la pazienza della prova qui sopra. Un file lento è un
    // incidente, due sono una coincidenza; tre di fila sulla stessa radice
    // sono una condivisione che non ce la fa, e la sonda non se ne accorge —
    // risponde, e continuerà a rispondere. Senza questa regola il quarto
    // file, e ognuno dei diecimila dopo di lui, costerebbe la scadenza
    // intera più la sonda, e lascerebbe dietro di sé un filo appeso.
    let (_dir, store, mut connection) = banco();

    let disco = Arc::new(
        FintoDisco::nuovo()
            .con_file(
                r"\\nas\musica",
                vec![
                    trovato(r"\\nas\musica\a.mp3", 10),
                    trovato(r"\\nas\musica\b.mp3", 10),
                    trovato(r"\\nas\musica\c.mp3", 10),
                    trovato(r"\\nas\musica\d.mp3", 10),
                ],
            )
            // Viva alla sonda, inutile alle letture: il caso che il
            // contatore esiste per riconoscere.
            .apre_lentamente(r"\\nas\musica", Duration::from_secs(5)),
    );
    let roots = vec![r"\\nas\musica".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: false,
        scadenze: svelte(),
        fermati: None,
    };

    let report = scan
        .run(&mut connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione");

    assert_eq!(
        disco.aperture(),
        3,
        "il quarto file non si tenta nemmeno: la radice è già stata dichiarata inutile"
    );
    assert_eq!(
        report.unreadable.len(),
        3,
        "i tre che hanno pagato la scadenza restano detti per nome: {:?}",
        report.unreadable
    );
    assert_eq!(
        report.radici_saltate,
        vec![r"\\nas\musica".to_owned()],
        "e la passata deve dire che di lì in poi non ha guardato"
    );
    assert_eq!(
        report.removed, 0,
        "una radice abbandonata non fa sparire i suoi brani"
    );
}

#[test]
fn la_scansione_non_tiene_la_connessione_mentre_guarda_il_disco() {
    // La prova che riassume tutto il lavoro. Non conta quanto duri una
    // scansione: conta che mentre dura, il lucchetto della libreria sia di
    // chi lo vuole — la finestra che si ridisegna, l'audio, la chiusura
    // dell'applicazione.
    let (_dir, store, connection) = banco();
    // Una riga che sul disco non c'è più: serve a far accadere anche la fase
    // delle rimozioni, così le prese da contare sono tutte.
    riga(&connection, 1, r"C:\musica\sparito.mp3", 10);

    let mut deposito = DepositoContato::nuovo(connection);
    let disco = Arc::new(
        FintoDisco::nuovo()
            .con_file(
                r"C:\musica",
                vec![
                    trovato(r"C:\musica\a.mp3", 10),
                    trovato(r"C:\musica\b.mp3", 10),
                ],
            )
            .sorveglia(&deposito.prestata),
    );
    let roots = vec![r"C:\musica".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };

    let report = scan
        .run_su(&mut deposito, |_, _| ControlFlow::Continue(()))
        .expect("scansione");

    assert!(
        !disco.sorpresa(),
        "il disco ha lavorato mentre la connessione era prestata"
    );
    // E il contrario della stessa medaglia: la connessione va chiesta e
    // restituita più volte, non presa una volta sola e tenuta. Le prese sono
    // il piano, le doppie e le sparizioni, il lotto, le rimozioni e gli
    // aggregati.
    assert!(
        deposito.prestiti >= 4,
        "la scansione ha preso la connessione {} volte: se fossero una o \
             due, la starebbe tenendo",
        deposito.prestiti
    );
    assert_eq!(report.removed, 1, "e il lavoro l'ha fatto lo stesso");
}

#[test]
fn annullare_durante_la_camminata_non_scrive_niente() {
    // Premere Annulla mentre si enumera un NAS: prima non succedeva niente
    // fino al primo file letto, che su una share lenta poteva non arrivare
    // mai. Adesso la camminata guarda la bandiera a ogni battito — e chi si
    // ferma lì non ha deciso niente, quindi non scrive niente.
    let (_dir, store, connection) = banco();
    riga(&connection, 1, r"C:\musica\a.mp3", 10);

    let mut deposito = DepositoContato::nuovo(connection);
    let disco = Arc::new(FintoDisco::nuovo());
    let roots = vec![r"C:\musica".to_owned()];
    let basta: &dyn Fn() -> bool = &|| true;
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: false,
        scadenze: svelte(),
        fermati: Some(basta),
    };

    let report = scan
        .run_su(&mut deposito, |_, _| ControlFlow::Continue(()))
        .expect("scansione");

    assert!(report.cancelled, "l'esito deve dire che è stata fermata");
    assert_eq!(
        deposito.prestiti, 0,
        "non si è deciso niente: la connessione non serviva nemmeno"
    );
    assert!(
        report.plan.to_remove.is_empty(),
        "il piano di una camminata interrotta è vuoto, non parziale"
    );
    let rimasti: i64 = deposito
        .connection
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .expect("conteggio");
    assert_eq!(rimasti, 1, "la riga è ancora lì");
}

#[test]
fn la_cancellazione_guarda_anche_il_percorso() {
    // Il prezzo di lasciare la connessione fra una fase e l'altra: fra il
    // momento in cui il piano dice «togli la riga 1» e il momento in cui la
    // si toglie, quella riga può essere diventata un'altra cosa. SQLite
    // riusa gli identificativi liberi, quindi il numero da solo non è una
    // prova d'identità: il percorso sì.
    let (_dir, store, connection) = banco();
    riga(&connection, 1, r"C:\musica\a.mp3", 10);

    // Il piano si calcola alla prima presa e le sparizioni alla seconda;
    // subito dopo la seconda, qualcun altro rinomina la riga.
    let mut deposito = DepositoDispettoso {
        connection,
        prestiti: 0,
        quando: 2,
    };
    let disco = Arc::new(FintoDisco::nuovo());
    let roots = vec![r"C:\musica".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };

    let report = scan
        .run_su(&mut deposito, |_, _| ControlFlow::Continue(()))
        .expect("scansione");

    assert_eq!(
        report.removed, 0,
        "la riga da togliere non è più quella che si era vista"
    );
    let rimasti: i64 = deposito
        .connection
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .expect("conteggio");
    assert_eq!(rimasti, 1, "e quella che c'è adesso non si tocca");
}

// ── i dati tecnici del file ─────────────────────────────────────────────

/// Una riga di `tracks` con i quattro campi tecnici messi a mano.
///
/// A mano e non per scansione: la scansione sa scrivere solo quel che
/// `lofty` legge da un WAV generato dalle prove, cioè un caso, mentre qui
/// servono i casi che in libreria ci sono davvero — un «Mpeg», un nome che
/// questa versione non conosce, e la riga con tutto a `NULL`.
fn riga_tecnica(
    connection: &Connection,
    id: i64,
    codec: Option<&str>,
    sample_rate: Option<i64>,
    channels: Option<i64>,
    bitrate: Option<i64>,
) {
    connection
        .execute(
            "INSERT INTO tracks
                   (id, path, track_key, title, artist, album, file_size,
                    date_added, date_modified, codec, sample_rate, channels, bitrate)
                 VALUES (?1, ?2, 'k', 'T', 'A', 'D', 1000, 1, 1, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                id,
                format!("C:/M/{id}.x"),
                codec,
                sample_rate,
                channels,
                bitrate
            ],
        )
        .expect("riga di prova");
}

#[test]
fn il_formato_legge_i_quattro_campi() {
    let db = crate::db::open_in_memory().expect("database");
    riga_tecnica(
        &db.connection,
        1,
        Some("Flac"),
        Some(44_100),
        Some(2),
        Some(1058),
    );

    let formato = read_formato(&db.connection, 1)
        .expect("lettura")
        .expect("la riga c'è");
    assert_eq!(formato.codec.as_deref(), Some("FLAC"));
    assert_eq!(formato.sample_rate, Some(44_100));
    assert_eq!(formato.channels, Some(2));
    assert_eq!(formato.bitrate, Some(1058));
}

#[test]
fn il_formato_degrada_sui_nulli() {
    // Il caso della libreria scansionata quando quelle colonne non si
    // scrivevano: la riga c'è, i dati no. `Some` con tutto vuoto e non
    // `None`, perché le due cose vogliono dire cose diverse — «non so
    // com'è fatto» contro «questo brano non c'è più».
    let db = crate::db::open_in_memory().expect("database");
    riga_tecnica(&db.connection, 7, None, None, None, None);

    let formato = read_formato(&db.connection, 7)
        .expect("lettura")
        .expect("la riga c'è comunque");
    assert_eq!(formato.codec, None);
    assert_eq!(formato.sample_rate, None);
    assert_eq!(formato.channels, None);
    assert_eq!(formato.bitrate, None);
}

#[test]
fn il_formato_di_un_brano_che_non_ce_piu_e_none() {
    // La coda tiene gli identificativi, non le righe: un brano tolto dalla
    // libreria mentre suonava arriva qui come un `id` che non esiste, e
    // `None` è la risposta giusta — non un errore da mostrare.
    let db = crate::db::open_in_memory().expect("database");
    assert_eq!(read_formato(&db.connection, 404).expect("lettura"), None);
}

fn suonato(
    codec: Option<&'static str>,
    kbps: Option<u32>,
    remoto: bool,
) -> rymembra_play::FormatoSuonato {
    rymembra_play::FormatoSuonato {
        codec,
        frequenza: 44_100,
        bit: Some(24),
        canali: 2,
        frequenza_uscita: 48_000,
        kbps,
        remoto,
    }
}

fn dal_database(codec: &str, bitrate: Option<i64>) -> FormatoFile {
    FormatoFile {
        codec: Some(codec.to_owned()),
        sample_rate: Some(48_000),
        channels: Some(1),
        bitrate,
        bits_per_sample: None,
        output_rate: None,
    }
}

#[test]
fn il_formato_suonato_corregge_codec_frequenza_e_canali() {
    // Un M4A che dentro è ALAC: la colonna dice il contenitore, il
    // decodificatore il codec. Frequenza e canali sono quelli che escono.
    let unito =
        dal_database("M4A", Some(900)).completato_da(&suonato(Some("ALAC"), Some(950), false));
    assert_eq!(unito.codec.as_deref(), Some("ALAC"));
    assert_eq!(unito.sample_rate, Some(44_100));
    assert_eq!(unito.channels, Some(2));
    assert_eq!(unito.bits_per_sample, Some(24));
    assert_eq!(unito.output_rate, Some(48_000));
    // Il file è locale: vince la misura di `lofty`, senza i tag.
    assert_eq!(unito.bitrate, Some(900));
}

#[test]
fn su_un_flusso_il_bitrate_e_quello_che_arriva() {
    // Un FLAC dell'Internet Archive ascoltato dal derivato MP3: la colonna
    // descrive il file del catalogo, la rete porta l'altro.
    let unito =
        dal_database("FLAC", Some(1000)).completato_da(&suonato(Some("MP3"), Some(192), true));
    assert_eq!(unito.codec.as_deref(), Some("MP3"));
    assert_eq!(unito.bitrate, Some(192));
}

#[test]
fn un_codec_che_il_motore_non_nomina_lascia_quello_del_database() {
    let unito = dal_database("Musepack", None).completato_da(&suonato(None, Some(180), false));
    assert_eq!(unito.codec.as_deref(), Some("Musepack"));
    // Il database tace: meglio il conto del motore che niente.
    assert_eq!(unito.bitrate, Some(180));
}

#[test]
fn etichetta_codec_traduce_mpeg_in_mp3() {
    // I tre nomi che la mappa esiste per correggere: sono i soli in cui il
    // `Debug` di `lofty::FileType` non somiglia a come il formato si
    // chiama. «Mp4» diventa il contenitore e non il codec, perché dentro
    // un M4A ci può stare ALAC.
    assert_eq!(etichetta_codec("Mpeg"), "MP3");
    assert_eq!(etichetta_codec("Mp4"), "M4A");
    assert_eq!(etichetta_codec("Vorbis"), "Ogg Vorbis");
    assert_eq!(etichetta_codec("Flac"), "FLAC");
}

#[test]
fn un_codec_sconosciuto_resta_scritto_come_sta() {
    // `FileType` è `#[non_exhaustive]`: questa prova è il contratto con le
    // versioni future di `lofty`. Un contenitore nuovo si mostra col nome
    // della variante invece di sparire, e chi guarda legge qualcosa di
    // brutto ma vero.
    assert_eq!(etichetta_codec("Dsf"), "Dsf");
    assert_eq!(etichetta_codec("Custom(\"tta\")"), "Custom(\"tta\")");
}

/// Una scansione limitata a una cartella, come la fa la sorveglianza: le
/// cartelle guardate sono le date, quella sorvegliata resta la radice.
fn scansiona_soltanto(lib: &mut Libreria, cartelle: &[String]) -> ScanReport {
    let libreria = vec![lib.root()];
    let scan = Scan {
        files: Arc::new(LocalFiles),
        covers: lib.store.clone(),
        roots: cartelle,
        radici_della_libreria: &libreria,
        rules: PathRules::for_current_platform(),
        prudente: true,
        scadenze: Scadenze::default(),
        fermati: None,
    };
    scan.run(&mut lib.connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione di un sottoalbero")
}

#[test]
fn un_sottoalbero_non_fa_sparire_il_resto_della_radice() {
    // La garanzia di cui la sorveglianza vive: si riscansiona l'album che
    // è cambiato, e i brani del resto della radice — anche quelli il cui
    // file nel frattempo è sparito davvero — non sono affar suo.
    let mut lib = Libreria::nuova();
    let a = lib.brano("Uno/A/01.wav", "A1", "Uno", "A");
    lib.brano("Uno/A/02.wav", "A2", "Uno", "A");
    lib.brano("Due/B/01.wav", "B1", "Due", "B");
    assert_eq!(lib.scansiona().inserted, 3);

    // Sparisce un file di A, e arriva un file nuovo in B.
    std::fs::remove_file(&a).expect("via un file");
    lib.brano("Due/B/02.wav", "B2", "Due", "B");
    let b = lib.musica().join("Due").join("B");
    let esito = scansiona_soltanto(&mut lib, &[b.to_string_lossy().into_owned()]);
    assert_eq!(esito.inserted, 1, "il brano nuovo di B entra");
    assert_eq!(
        esito.removed, 0,
        "il file sparito di A non è di questa passata"
    );
    assert_eq!(lib.conta("tracks"), 4);

    // Riguardata A, se ne va.
    let a = lib.musica().join("Uno").join("A");
    let esito = scansiona_soltanto(&mut lib, &[a.to_string_lossy().into_owned()]);
    assert_eq!(esito.removed, 1);
    assert_eq!(lib.conta("tracks"), 3);
    assert_eq!(esito.aggregates.albums, 2);
}

#[test]
fn un_sottoalbero_sparito_non_toglie_niente() {
    // La cartella da riguardare non c'è più: la scansione la salta come
    // una radice che non risponde. È il motivo per cui
    // `cartelle_da_riguardare` risale fino a una cartella che esiste.
    let mut lib = Libreria::nuova();
    lib.brano("Uno/A/01.wav", "A1", "Uno", "A");
    assert_eq!(lib.scansiona().inserted, 1);
    let a = lib.musica().join("Uno").join("A");
    std::fs::remove_dir_all(&a).expect("via la cartella");
    let esito = scansiona_soltanto(&mut lib, &[a.to_string_lossy().into_owned()]);
    assert_eq!(esito.removed, 0);
    assert_eq!(esito.radici_saltate.len(), 1);
    // Dalla cartella che la conteneva, invece, la sparizione si vede.
    let uno = lib.musica().join("Uno");
    let cartelle = cartelle_da_riguardare(
        &[Cambiamento {
            percorso: a.join("01.wav").to_string_lossy().into_owned(),
            nuovo: false,
        }],
        &[lib.root()],
        PathRules::for_current_platform(),
        &natura_sul_disco,
    );
    assert_eq!(cartelle, vec![uno.to_string_lossy().into_owned()]);
    let esito = scansiona_soltanto(&mut lib, &cartelle);
    assert_eq!(esito.removed, 1);
}

/// Che cos'è un percorso sul disco vero.
fn natura_sul_disco(percorso: &str) -> Natura {
    let percorso = std::path::Path::new(percorso);
    if percorso.is_dir() {
        Natura::Cartella
    } else if percorso.exists() {
        Natura::File
    } else {
        Natura::Assente
    }
}

/// Un avviso di modifica o cancellazione: non nuovo.
fn cambiato(percorso: &str) -> Cambiamento {
    Cambiamento {
        percorso: percorso.to_owned(),
        nuovo: false,
    }
}

#[test]
fn un_avviso_dal_cestino_non_fa_riguardare_niente() {
    // Un'unità intera sorvegliata: un brano eliminato da Rymembra va nel
    // Cestino, cioè in `D:\$RECYCLE.BIN\<SID>\$R….flac`, e l'avviso del
    // file nuovo non deve far partire una scansione con radice il Cestino,
    // che la camminata attraverserebbe com'è.
    let win = PathRules {
        case_insensitive: true,
    };
    let radici = vec![r"D:\".to_owned()];
    let natura = |c: &str| {
        if c.ends_with(".flac") {
            Natura::File
        } else {
            Natura::Cartella
        }
    };
    let cambiati = [
        cambiato(r"D:\$RECYCLE.BIN\S-1-5-21-1-1001\$R4K2J9Q.flac"),
        cambiato(r"D:\$Recycle.Bin\S-1-5-21-1-1001\$IAB12CD.flac"),
        cambiato(r"D:\System Volume Information\x.flac"),
        Cambiamento {
            percorso: r"D:\$RECYCLE.BIN\S-1-5-21-1-1001".to_owned(),
            nuovo: true,
        },
    ];
    assert_eq!(
        cartelle_da_riguardare(&cambiati, &radici, win, &natura),
        Vec::<String>::new()
    );
    // Una cartella di musica qualunque accanto resta da riguardare.
    assert_eq!(
        cartelle_da_riguardare(&[cambiato(r"D:\Album\01.flac")], &radici, win, &natura),
        vec![r"D:\Album".to_owned()]
    );
}

#[test]
fn le_cartelle_da_riguardare_sono_le_piu_esterne_che_esistono() {
    let win = PathRules {
        case_insensitive: true,
    };
    let radici = vec![r"C:\Musica".to_owned(), r"D:\Altro".to_owned()];
    // Un disco finto: quel che finisce con un'estensione è un file, quel
    // che si chiama «Sparito» non c'è, il resto è una cartella.
    let natura = |c: &str| {
        if c.contains("Sparito") {
            Natura::Assente
        } else if c.ends_with(".flac") {
            Natura::File
        } else {
            Natura::Cartella
        }
    };
    let cambiati = [
        // Due file dello stesso album: un album solo.
        r"C:\Musica\Uno\A\01.flac",
        r"C:\Musica\Uno\A\02.flac",
        // Una cartella cancellata con dentro un album: si risale a «Due».
        r"C:\Musica\Due\Sparito\X\01.flac",
        // Fuori da tutte le radici: niente.
        r"E:\Scaricati\x.flac",
        // La radice stessa.
        r"D:\Altro",
        // Dentro una cartella già presa per intero.
        r"D:\Altro\Qualcosa\y.flac",
    ]
    .map(cambiato);
    assert_eq!(
        cartelle_da_riguardare(&cambiati, &radici, win, &natura),
        vec![
            r"C:\Musica\Due".to_owned(),
            r"C:\Musica\Uno\A".to_owned(),
            r"D:\Altro".to_owned(),
        ]
    );
    // Un file subito sotto la radice: la radice.
    assert_eq!(
        cartelle_da_riguardare(&[cambiato(r"c:\musica\x.flac")], &radici, win, &natura),
        vec![r"C:\Musica".to_owned()]
    );
    // Una cartella nuova si guarda da sé, non dal genitore: un album
    // copiato sotto l'artista non costa l'elenco di tutti gli altri suoi.
    let arrivato = Cambiamento {
        percorso: r"C:\Musica\Guccini\Radici".to_owned(),
        nuovo: true,
    };
    assert_eq!(
        cartelle_da_riguardare(std::slice::from_ref(&arrivato), &radici, win, &natura),
        vec![r"C:\Musica\Guccini\Radici".to_owned()]
    );
    // La stessa cartella, ma avvisata come modifica: dal genitore, come
    // tutto quel che non è nuovo.
    assert_eq!(
        cartelle_da_riguardare(
            &[cambiato(r"C:\Musica\Guccini\Radici")],
            &radici,
            win,
            &natura
        ),
        vec![r"C:\Musica\Guccini".to_owned()]
    );
    // Un file nuovo non è una cartella: dal genitore.
    assert_eq!(
        cartelle_da_riguardare(
            &[Cambiamento {
                percorso: r"C:\Musica\Guccini\Radici\01.flac".to_owned(),
                nuovo: true,
            }],
            &radici,
            win,
            &natura
        ),
        vec![r"C:\Musica\Guccini\Radici".to_owned()]
    );
    // Una cartella nuova che nel frattempo è sparita — creata e rinominata
    // subito — si guarda dal primo antenato che c'è.
    assert_eq!(
        cartelle_da_riguardare(
            &[Cambiamento {
                percorso: r"C:\Musica\Tre\Sparito".to_owned(),
                nuovo: true,
            }],
            &radici,
            win,
            &natura
        ),
        vec![r"C:\Musica\Tre".to_owned()]
    );
    // Un antenato che è diventato un file non si cammina: si risale ancora.
    let file_al_posto = |c: &str| {
        if c.ends_with("Quattro") || c.ends_with(".flac") {
            Natura::File
        } else {
            Natura::Cartella
        }
    };
    assert_eq!(
        cartelle_da_riguardare(
            &[cambiato(r"C:\Musica\Cinque\Quattro\01.flac")],
            &radici,
            win,
            &file_al_posto
        ),
        vec![r"C:\Musica\Cinque".to_owned()]
    );
    // E l'arrivo dentro un altro arrivo si riduce al più esterno.
    assert_eq!(
        cartelle_da_riguardare(
            &[
                arrivato,
                Cambiamento {
                    percorso: r"C:\Musica\Guccini\Radici\CD2".to_owned(),
                    nuovo: true,
                },
            ],
            &radici,
            win,
            &natura
        ),
        vec![r"C:\Musica\Guccini\Radici".to_owned()]
    );
    // Una radice di unità: il genitore di `X:\a.flac` è `X:\`.
    assert_eq!(cartella_che_contiene(r"X:\a.flac").as_deref(), Some(r"X:\"));
    assert_eq!(cartella_che_contiene("nudo"), None);
}

#[test]
fn i_download_si_scansionano_solo_dentro_le_radici() {
    let win = PathRules {
        case_insensitive: true,
    };
    let radici = vec![r"C:\Musica".to_owned(), r"D:\Scarichi\Buoni".to_owned()];
    assert_eq!(
        dentro_le_radici(r"C:\Musica\Download", &radici, win),
        vec![r"C:\Musica\Download".to_owned()]
    );
    assert_eq!(
        dentro_le_radici(r"D:\Scarichi", &radici, win),
        vec![r"D:\Scarichi\Buoni".to_owned()]
    );
    assert!(dentro_le_radici(r"E:\Altrove", &radici, win).is_empty());
}

// ── le immagini CD, la memoria dei rifiuti, le letture in parallelo ─────
//
// Su file veri in una cartella temporanea, attraverso un disco che conta le
// aperture: la domanda che conta per un'immagine CD non è solo «i brani
// escono giusti» ma «alla passata dopo non si riapre niente», e la risposta
// si vede contando.

/// Il disco vero, contando le aperture; volendo con i `.cue` che non si
/// leggono e qualche file che si apre con comodo.
struct DiscoContato {
    aperture: std::sync::atomic::AtomicUsize,
    cue_rotti: std::sync::atomic::AtomicBool,
    /// I file il cui percorso contiene uno di questi pezzi dormono
    /// `attesa` prima di aprirsi.
    lenti: Vec<String>,
    attesa: Duration,
}

impl DiscoContato {
    fn nuovo() -> Arc<Self> {
        Arc::new(Self {
            aperture: std::sync::atomic::AtomicUsize::new(0),
            cue_rotti: std::sync::atomic::AtomicBool::new(false),
            lenti: Vec::new(),
            attesa: Duration::ZERO,
        })
    }

    fn aperture(&self) -> usize {
        self.aperture.swap(0, std::sync::atomic::Ordering::SeqCst)
    }

    fn rompi_i_fogli(&self, rotti: bool) {
        self.cue_rotti
            .store(rotti, std::sync::atomic::Ordering::SeqCst);
    }
}

impl MusicFiles for DiscoContato {
    fn walk(&self, root: &str) -> Result<crate::files::Camminata, AppError> {
        LocalFiles.walk(root)
    }

    fn walk_a_rate(
        &self,
        root: &str,
        su_voce: &mut dyn FnMut(crate::files::Voce) -> ControlFlow<()>,
    ) -> Result<bool, AppError> {
        LocalFiles.walk_a_rate(root, su_voce)
    }

    fn radice_raggiungibile(&self, root: &str) -> bool {
        LocalFiles.radice_raggiungibile(root)
    }

    fn open(&self, path: &str) -> Result<Box<dyn crate::files::ReadSeek + Send + Sync>, AppError> {
        self.aperture
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.cue_rotti.load(std::sync::atomic::Ordering::SeqCst)
            && path.to_lowercase().ends_with(".cue")
        {
            return Err(AppError::new(ErrorCode::FsNetworkUnavailable {
                path: Some(path.to_owned()),
            }));
        }
        if self.lenti.iter().any(|pezzo| path.contains(pezzo.as_str())) {
            std::thread::sleep(self.attesa);
        }
        LocalFiles.open(path)
    }
}

/// Una scansione a mano della libreria, attraverso il disco dato.
fn scansiona_con(lib: &mut Libreria, disco: &Arc<DiscoContato>) -> ScanReport {
    let roots = vec![lib.root()];
    let scan = Scan {
        files: Arc::clone(disco) as Arc<dyn MusicFiles>,
        covers: lib.store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules::for_current_platform(),
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };
    scan.run(&mut lib.connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione")
}

/// Un WAV di silenzio lungo `secondi`: 8 kHz mono a 16 bit, che si taglia
/// al fotogramma CD senza resti.
fn wav_lungo(path: &Path, secondi: u32) {
    let dati = usize::try_from(16_000 * secondi).expect("piccolo");
    let mut out = Vec::with_capacity(dati + 44);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&u32::try_from(dati + 36).expect("piccolo").to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&8_000u32.to_le_bytes());
    out.extend_from_slice(&16_000u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&u32::try_from(dati).expect("piccolo").to_le_bytes());
    out.resize(out.len() + dati, 0);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("cartelle");
    }
    std::fs::write(path, &out).expect("scrittura del wav");
}

/// Un foglio come lo scrive EAC, per l'immagine `file`, con i brani
/// `(titolo, INDEX 01)`.
fn scrivi_foglio(path: &Path, file: &str, brani: &[(&str, &str)]) {
    use std::fmt::Write as _;
    let mut testo = String::from(
        "REM GENRE Rock\r\nREM DATE 1999\r\nPERFORMER \"Artista\"\r\nTITLE \"Disco\"\r\n",
    );
    let _ = write!(testo, "FILE \"{file}\" WAVE\r\n");
    for (posto, (titolo, inizio)) in brani.iter().enumerate() {
        let _ = write!(
            testo,
            "  TRACK {:02} AUDIO\r\n    TITLE \"{titolo}\"\r\n    INDEX 01 {inizio}\r\n",
            posto + 1
        );
    }
    std::fs::write(path, testo).expect("scrittura del foglio");
}

/// Tre brani su un'immagine di dieci secondi: 0–3 s, 3–6,49 s, il resto.
const TRE_BRANI: [(&str, &str); 3] = [
    ("Primo", "00:00:00"),
    ("Secondo", "00:03:00"),
    ("Terzo", "00:06:37"),
];

/// Un disco in `cartella` (dentro la musica): `Disco.wav` più `Disco.cue`,
/// con date fisse — il foglio più recente dell'immagine.
fn un_disco(lib: &Libreria, cartella: &[&str]) -> (PathBuf, PathBuf) {
    let mut dir = lib.musica();
    for pezzo in cartella {
        dir = dir.join(pezzo);
    }
    let immagine = dir.join("Disco.wav");
    let cue = dir.join("Disco.cue");
    wav_lungo(&immagine, 10);
    scrivi_foglio(&cue, "Disco.wav", &TRE_BRANI);
    data(&immagine, 1_000_000);
    data(&cue, 1_000_100);
    (cue, immagine)
}

/// Un brano d'immagine come sta in tabella.
#[derive(Debug, PartialEq, Eq)]
struct BranoVisto {
    id: i64,
    path: String,
    numero: Option<i64>,
    titolo: String,
    durata: i64,
    inizio: Option<i64>,
    fine: Option<i64>,
    immagine: Option<String>,
    data: Option<i64>,
    dimensione: Option<i64>,
}

/// I brani d'immagine in tabella, in ordine di percorso.
fn brani_visti(lib: &Libreria) -> Vec<BranoVisto> {
    let mut statement = lib
        .connection
        .prepare(
            "SELECT id, path, track_number, title, duration_ms, inizio_cd, fine_cd,
                        immagine, date_modified, file_size
                   FROM tracks WHERE immagine IS NOT NULL ORDER BY path",
        )
        .expect("brani d'immagine");
    statement
        .query_map([], |r| {
            Ok(BranoVisto {
                id: r.get(0)?,
                path: r.get(1)?,
                numero: r.get(2)?,
                titolo: r.get(3)?,
                durata: r.get(4)?,
                inizio: r.get(5)?,
                fine: r.get(6)?,
                immagine: r.get(7)?,
                data: r.get(8)?,
                dimensione: r.get(9)?,
            })
        })
        .expect("lettura")
        .collect::<Result<Vec<_>, _>>()
        .expect("righe")
}

/// Lo stesso percorso, per chi confronta come la scansione.
fn stesso(a: &str, b: &Path) -> bool {
    let rules = PathRules::for_current_platform();
    path_key(a, rules) == path_key(&b.to_string_lossy(), rules)
}

#[test]
fn un_immagine_col_suo_foglio_diventa_i_suoi_brani() {
    let mut lib = Libreria::nuova();
    let (cue, immagine) = un_disco(&lib, &["Artista", "Disco"]);

    let primo = lib.scansiona();
    assert_eq!(primo.inserted, 3);
    assert_eq!(primo.brani_d_immagine, 3);
    assert!(primo.unreadable.is_empty(), "{:?}", primo.unreadable);

    let brani = brani_visti(&lib);
    let titoli: Vec<&str> = brani.iter().map(|b| b.titolo.as_str()).collect();
    assert_eq!(titoli, ["Primo", "Secondo", "Terzo"]);
    assert_eq!(
        brani.iter().map(|b| b.numero).collect::<Vec<_>>(),
        [Some(1), Some(2), Some(3)]
    );
    // I tempi esatti del foglio, in fotogrammi; la fine dell'ultimo è
    // quella del file.
    assert_eq!(
        brani.iter().map(|b| (b.inizio, b.fine)).collect::<Vec<_>>(),
        [
            (Some(0), Some(225)),
            (Some(225), Some(487)),
            (Some(487), None)
        ]
    );
    assert_eq!(
        brani.iter().map(|b| b.durata).collect::<Vec<_>>(),
        [3000, 3493, 3507]
    );
    let dimensione =
        i64::try_from(std::fs::metadata(&immagine).expect("immagine").len()).expect("piccola");
    for brano in &brani {
        assert!(stesso(&brano.path, &cue) || brano.path.contains('|'));
        assert!(
            brano
                .immagine
                .as_deref()
                .is_some_and(|p| stesso(p, &immagine)),
            "{brano:?}"
        );
        // Il contratto delle date: esattamente la voce virtuale.
        assert_eq!(
            brano.data,
            Some(1_000_100_000),
            "la data del foglio, la più recente"
        );
        assert_eq!(
            brano.dimensione,
            Some(dimensione),
            "la dimensione dell'immagine"
        );
    }
    // Dal foglio: album, artista, anno, genere. E un album solo.
    let (album, artista, anno, genere): (String, String, Option<i64>, Option<String>) = lib
        .connection
        .query_row(
            "SELECT album, artist, year, genre FROM tracks WHERE track_number = 2",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .expect("il secondo brano");
    assert_eq!(
        (album.as_str(), artista.as_str(), anno, genere.as_deref()),
        ("Disco", "Artista", Some(1999), Some("Rock"))
    );
    assert_eq!(lib.conta("albums"), 1);
    // L'immagine non c'è anche come brano intero.
    assert_eq!(lib.conta("tracks"), 3);

    // La passata dopo non apre niente: né i brani né il foglio.
    let disco = DiscoContato::nuovo();
    let secondo = scansiona_con(&mut lib, &disco);
    assert_eq!(
        disco.aperture(),
        0,
        "una riscansione invariata non apre nessun file"
    );
    assert_eq!(secondo.letti, 0);
    assert_eq!(
        (secondo.inserted, secondo.updated, secondo.removed),
        (0, 0, 0)
    );
    assert_eq!(secondo.plan.unchanged, 3);
    assert_eq!(secondo.brani_d_immagine, 3);
}

#[test]
fn un_foglio_cambiato_riscrive_i_suoi_brani() {
    let mut lib = Libreria::nuova();
    let (cue, _) = un_disco(&lib, &["Artista", "Disco"]);
    lib.scansiona();
    let prima: Vec<i64> = brani_visti(&lib).iter().map(|b| b.id).collect();

    scrivi_foglio(
        &cue,
        "Disco.wav",
        &[
            ("Uno", "00:00:00"),
            ("Due", "00:03:00"),
            ("Tre", "00:06:37"),
            ("Quattro", "00:08:00"),
        ],
    );
    data(&cue, 1_000_200);
    let esito = lib.scansiona();
    assert_eq!((esito.updated, esito.inserted, esito.removed), (3, 1, 0));
    let dopo = brani_visti(&lib);
    assert_eq!(
        dopo.iter().map(|b| b.titolo.as_str()).collect::<Vec<_>>(),
        ["Uno", "Due", "Tre", "Quattro"]
    );
    assert_eq!(
        dopo.iter().take(3).map(|b| b.id).collect::<Vec<_>>(),
        prima,
        "le righe che c'erano restano le stesse"
    );
    assert!(dopo.iter().all(|b| b.data == Some(1_000_200_000)));
    // Il terzo adesso finisce dove comincia il quarto.
    assert_eq!(dopo.get(2).map(|b| b.fine), Some(Some(600)));
}

#[test]
fn un_brano_solo_da_rileggere_riapre_l_immagine_e_rifa_quella_riga() {
    // La sentinella `-1` su un brano solo — «dimentica l'arricchimento»,
    // o una riga rimasta indietro —: si riapre l'immagine, si rifà quella
    // riga dal foglio, e le sorelle non si toccano.
    let mut lib = Libreria::nuova();
    un_disco(&lib, &["Artista", "Disco"]);
    lib.scansiona();
    let prima = brani_visti(&lib);
    lib.connection
        .execute(
            "UPDATE tracks SET date_modified = -1, title = 'X' WHERE track_number = 2",
            [],
        )
        .expect("sentinella");

    let disco = DiscoContato::nuovo();
    let esito = scansiona_con(&mut lib, &disco);
    assert_eq!((esito.letti, esito.updated), (1, 1));
    assert_eq!(
        disco.aperture(),
        1,
        "l'immagine sola: il foglio viene dalla cache"
    );
    assert_eq!(
        brani_visti(&lib),
        prima,
        "la riga è tornata quella del foglio"
    );
}

#[test]
fn un_foglio_tolto_restituisce_l_immagine_intera() {
    let mut lib = Libreria::nuova();
    let (cue, _) = un_disco(&lib, &["Artista", "Disco"]);
    lib.scansiona();
    assert_eq!(lib.conta("fogli_cue"), 1);

    std::fs::remove_file(&cue).expect("via il foglio");
    let esito = lib.scansiona();
    assert_eq!(esito.removed, 3, "i brani se ne vanno");
    assert_eq!(esito.inserted, 1, "e l'immagine torna un file intero");
    assert!(brani_visti(&lib).is_empty());
    assert_eq!(lib.conta("tracks"), 1);
    assert_eq!(
        lib.conta("fogli_cue"),
        0,
        "la cache dimentica il foglio che non c'è più"
    );
}

#[test]
fn un_disco_intero_non_diventa_uno_dei_suoi_brani() {
    // Il caso della libreria vera: un'immagine FLAC già in libreria come
    // brano intero, con nei tag il titolo del secondo brano — cioè la
    // stessa chiave di contenuto di quel brano. Arriva il `.cue`: il disco
    // intero se ne va, e i suoi ascolti non finiscono su un pezzo solo.
    let mut lib = Libreria::nuova();
    let (cue, _) = un_disco(&lib, &["Artista", "Disco"]);
    lib.scansiona();
    let chiave_del_secondo: String = lib
        .connection
        .query_row(
            "SELECT content_key FROM tracks WHERE track_number = 2",
            [],
            |r| r.get(0),
        )
        .expect("la chiave del secondo brano");

    // Senza foglio, l'immagine torna un brano intero — nemmeno questo è
    // uno spostamento.
    std::fs::remove_file(&cue).expect("via il foglio");
    let senza = lib.scansiona();
    assert_eq!((senza.moved, senza.removed, senza.inserted), (0, 3, 1));
    lib.connection
        .execute(
            "UPDATE tracks SET content_key = ?1, play_count = 9 WHERE immagine IS NULL",
            [&chiave_del_secondo],
        )
        .expect("il disco intero con la chiave del secondo brano");

    scrivi_foglio(&cue, "Disco.wav", &TRE_BRANI);
    data(&cue, 1_000_300);
    let con = lib.scansiona();
    assert_eq!(
        (con.moved, con.removed, con.inserted),
        (0, 1, 3),
        "il disco intero si toglie, i brani entrano nuovi"
    );
    let ascoltati: i64 = lib
        .connection
        .query_row(
            "SELECT COUNT(*) FROM tracks WHERE play_count > 0",
            [],
            |r| r.get(0),
        )
        .expect("conteggio");
    assert_eq!(ascoltati, 0, "nessun brano ha ereditato il disco intero");
    assert_eq!(brani_visti(&lib).len(), 3);
    assert_eq!(lib.conta("tracks"), 3);
}

#[test]
fn un_immagine_spostata_col_suo_foglio_tiene_i_suoi_brani() {
    let mut lib = Libreria::nuova();
    un_disco(&lib, &["Artista", "Disco"]);
    lib.scansiona();
    let prima = brani_visti(&lib);
    lib.connection
        .execute(
            "UPDATE tracks SET play_count = 7 WHERE track_number = 2",
            [],
        )
        .expect("ascolti");

    let vecchia = lib.musica().join("Artista").join("Disco");
    let nuova = lib.musica().join("Artista").join("Altro");
    std::fs::rename(&vecchia, &nuova).expect("spostamento");
    let esito = lib.scansiona();
    assert_eq!((esito.moved, esito.inserted, esito.removed), (3, 0, 0));
    let dopo = brani_visti(&lib);
    assert_eq!(
        dopo.iter().map(|b| b.id).collect::<Vec<_>>(),
        prima.iter().map(|b| b.id).collect::<Vec<_>>(),
        "gli identificativi restano"
    );
    assert!(
        dopo.iter().all(|b| b
            .immagine
            .as_deref()
            .is_some_and(|p| stesso(p, &nuova.join("Disco.wav")))),
        "e l'immagine è quella nuova: {dopo:?}"
    );
    let ascolti: i64 = lib
        .connection
        .query_row(
            "SELECT play_count FROM tracks WHERE track_number = 2",
            [],
            |r| r.get(0),
        )
        .expect("ascolti");
    assert_eq!(ascolti, 7);
}

#[test]
fn un_foglio_che_non_si_legge_non_fa_danni() {
    let mut lib = Libreria::nuova();
    let (cue, _) = un_disco(&lib, &["Artista", "Disco"]);
    let disco = DiscoContato::nuovo();

    // Mai letto e non si legge: la musica accanto non si giudica — una di
    // quelle è l'immagine — e la cartella è un ramo perso.
    disco.rompi_i_fogli(true);
    let esito = scansiona_con(&mut lib, &disco);
    assert_eq!(esito.inserted, 0, "l'immagine non entra come brano intero");
    assert_eq!(esito.rami_persi.len(), 1);
    assert_eq!(lib.conta("tracks"), 0);

    disco.rompi_i_fogli(false);
    assert_eq!(scansiona_con(&mut lib, &disco).inserted, 3);
    let prima = brani_visti(&lib);

    // Cambiato e non si legge: vale quel che la cache ricorda, con la data
    // di allora — niente si rilegge, niente si toglie.
    scrivi_foglio(&cue, "Disco.wav", &[("A", "00:00:00"), ("B", "00:05:00")]);
    data(&cue, 1_000_300);
    disco.rompi_i_fogli(true);
    let esito = scansiona_con(&mut lib, &disco);
    assert_eq!((esito.letti, esito.updated, esito.removed), (0, 0, 0));
    assert_eq!(esito.rami_persi.len(), 1);
    assert_eq!(brani_visti(&lib), prima);

    // Appena si legge, il foglio nuovo vale per davvero.
    disco.rompi_i_fogli(false);
    let esito = scansiona_con(&mut lib, &disco);
    assert_eq!((esito.updated, esito.removed), (2, 1));
    assert_eq!(
        brani_visti(&lib)
            .iter()
            .map(|b| b.titolo.as_str())
            .collect::<Vec<_>>(),
        ["A", "B"]
    );
}

#[test]
fn un_immagine_rinominata_senza_toccare_il_foglio_si_riscrive() {
    // Il foglio nomina `Rip.wav`; l'immagine si chiama poi come il foglio.
    // Il foglio la ritrova lo stesso — col suo nome — e la voce virtuale
    // resta identica: stesso percorso, stessa dimensione, stessa data. Ma
    // la colonna `immagine` deve seguire il file, o i brani non suonano.
    let mut lib = Libreria::nuova();
    let dir = lib.musica().join("Artista").join("Disco");
    let rip = dir.join("Rip.wav");
    let cue = dir.join("Disco.cue");
    wav_lungo(&rip, 10);
    scrivi_foglio(&cue, "Rip.wav", &TRE_BRANI);
    data(&rip, 1_000_000);
    data(&cue, 1_000_100);
    assert_eq!(lib.scansiona().inserted, 3);
    let prima = brani_visti(&lib);
    assert!(
        prima
            .iter()
            .all(|b| b.immagine.as_deref().is_some_and(|p| stesso(p, &rip)))
    );
    let ids = |brani: &[BranoVisto]| brani.iter().map(|b| b.id).collect::<Vec<_>>();

    let nuova = dir.join("Disco.wav");
    std::fs::rename(&rip, &nuova).expect("rinomina");
    let esito = lib.scansiona();
    assert_eq!(
        (esito.updated, esito.inserted, esito.removed, esito.moved),
        (3, 0, 0, 0)
    );
    let dopo = brani_visti(&lib);
    assert_eq!(ids(&dopo), ids(&prima), "le stesse righe");
    assert!(
        dopo.iter()
            .all(|b| b.immagine.as_deref().is_some_and(|p| stesso(p, &nuova))),
        "{dopo:?}"
    );

    // Le sole maiuscole: su una share sensibile `Disco.wav` e `disco.wav`
    // sono due nomi, e il vecchio non si apre più.
    let minuscola = dir.join("disco.wav");
    std::fs::rename(&nuova, &minuscola).expect("rinomina");
    let esito = lib.scansiona();
    assert_eq!((esito.updated, esito.inserted, esito.removed), (3, 0, 0));
    let dopo = brani_visti(&lib);
    assert_eq!(ids(&dopo), ids(&prima));
    assert!(
        dopo.iter().all(|b| b
            .immagine
            .as_deref()
            .is_some_and(|p| p.ends_with("disco.wav"))),
        "{dopo:?}"
    );

    // E poi più niente: la colonna non rimbalza.
    let disco = DiscoContato::nuovo();
    let esito = scansiona_con(&mut lib, &disco);
    assert_eq!(
        disco.aperture(),
        0,
        "una riscansione invariata non apre niente"
    );
    assert_eq!((esito.updated, esito.letti), (0, 0));
    assert_eq!(esito.plan.unchanged, 3);
}

#[test]
fn un_foglio_nuovo_accanto_non_ruba_il_disco() {
    // Un secondo foglio per la stessa immagine, che si ordina prima del
    // primo e porta altri titoli. Se vincesse lui, i percorsi virtuali
    // cambierebbero tutti, gli spostamenti non si riconoscerebbero — i
    // titoli sono diversi — e i brani se ne andrebbero con i loro ascolti.
    let mut lib = Libreria::nuova();
    let (cue, _) = un_disco(&lib, &["Artista", "Disco"]);
    lib.scansiona();
    let prima = brani_visti(&lib);

    scrivi_foglio(
        &cue.with_file_name("Aaa.cue"),
        "Disco.wav",
        &[
            ("Uno", "00:00:00"),
            ("Due", "00:03:00"),
            ("Tre", "00:06:37"),
        ],
    );
    let esito = lib.scansiona();
    assert_eq!(
        (esito.inserted, esito.removed, esito.updated, esito.moved),
        (0, 0, 0, 0)
    );
    assert_eq!(brani_visti(&lib), prima);
}

#[test]
fn una_sottocartella_svuotata_dalla_sorveglianza_e_uno_spostamento() {
    // La sorveglianza guarda le sottocartelle: un album spostato per intero
    // arriva come «la cartella di prima e quella di adesso». La prima ha
    // più di cinquanta brani noti e nessun file, ma non è una cartella
    // sorvegliata: è davvero vuota, e i suoi brani sono quelli comparsi
    // nell'altra. Saltarla vorrebbe dire ogni brano due volte.
    let mut lib = Libreria::nuova();
    let quanti = RADICE_SOSPETTA + 2;
    for n in 1..=quanti {
        lib.brano(
            &format!("Sorgente/{n:02}.wav"),
            &format!("Pezzo {n}"),
            "Tale",
            "Al",
        );
    }
    assert_eq!(lib.scansiona().inserted, quanti);
    let ids_prima: Vec<i64> = (1..=quanti)
        .map(|n| lib.id_del_titolo(&format!("Pezzo {n}")))
        .collect();

    let sorgente = lib.musica().join("Sorgente");
    let destinazione = lib.musica().join("Destinazione");
    std::fs::create_dir_all(&destinazione).expect("cartella");
    for n in 1..=quanti {
        let nome = format!("{n:02}.wav");
        std::fs::rename(sorgente.join(&nome), destinazione.join(&nome)).expect("spostamento");
    }
    let cartelle = [
        sorgente.to_string_lossy().into_owned(),
        destinazione.to_string_lossy().into_owned(),
    ];
    let esito = scansiona_soltanto(&mut lib, &cartelle);
    assert!(
        esito.radici_saltate.is_empty(),
        "{:?}",
        esito.radici_saltate
    );
    assert!(esito.radici_vuote.is_empty());
    assert_eq!((esito.moved, esito.inserted, esito.removed), (quanti, 0, 0));
    assert_eq!(lib.conta("tracks"), i64::try_from(quanti).expect("pochi"));
    let ids_dopo: Vec<i64> = (1..=quanti)
        .map(|n| lib.id_del_titolo(&format!("Pezzo {n}")))
        .collect();
    assert_eq!(ids_dopo, ids_prima, "le stesse righe, spostate");
}

/// Un file che il lettore rifiuta: un `.mp3` che dentro non ha niente.
fn spazzatura(path: &Path, quanti: usize) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("cartelle");
    }
    std::fs::write(path, vec![b'x'; quanti]).expect("scrittura");
}

#[test]
fn un_file_rifiutato_non_si_riapre_finche_non_cambia() {
    let mut lib = Libreria::nuova();
    lib.brano("A/buono.wav", "Buono", "A", "B");
    let rotto = lib.musica().join("A").join("rotto.mp3");
    let grande = usize::try_from(MIN_TRACK_BYTES * 2).expect("piccolo");
    spazzatura(&rotto, grande);
    data(&rotto, 2_000_000);
    let disco = DiscoContato::nuovo();

    let primo = scansiona_con(&mut lib, &disco);
    assert_eq!(primo.unreadable.len(), 1);
    assert_eq!(lib.conta("file_illeggibili"), 1, "il no si ricorda");

    // Uguale a prima: non si riapre, e si conta fra i già noti.
    disco.aperture();
    let secondo = scansiona_con(&mut lib, &disco);
    assert_eq!(disco.aperture(), 0);
    assert_eq!((secondo.letti, secondo.illeggibili_noti), (0, 1));
    assert!(secondo.unreadable.is_empty());

    // Cambiato: si riprova.
    spazzatura(&rotto, grande + 1);
    data(&rotto, 2_000_100);
    let terzo = scansiona_con(&mut lib, &disco);
    assert_eq!((terzo.letti, terzo.illeggibili_noti), (1, 0));
    assert_eq!(lib.conta("file_illeggibili"), 1);

    // «Rileggi tutti i tag» dimentica tutti i no.
    rileggi_tutti_i_tag(&lib.connection).expect("rilettura");
    assert_eq!(lib.conta("file_illeggibili"), 0);
    let quarto = scansiona_con(&mut lib, &disco);
    assert_eq!(quarto.illeggibili_noti, 0);
    assert!(
        quarto
            .unreadable
            .iter()
            .any(|u| u.path.ends_with("rotto.mp3"))
    );

    // Guarito — al suo posto un file vero —: la memoria lo dimentica.
    std::fs::remove_file(&rotto).expect("via");
    let sano = lib.musica().join("A").join("rotto.wav");
    wav(&sano);
    // E un file rifiutato che sparisce si pota: qui il `.mp3` non c'è più.
    let quinto = scansiona_con(&mut lib, &disco);
    assert_eq!(quinto.inserted, 1);
    assert_eq!(lib.conta("file_illeggibili"), 0);
}

#[test]
fn la_rete_e_le_cadute_non_si_ricordano() {
    // Un singhiozzo di rete su una radice viva e un lettore che si
    // schianta: nessuno dei due dice niente del file, e la passata dopo
    // deve riprovare tutti e due.
    let (_dir, store, mut connection) = banco();
    let disco = Arc::new(
        FintoDisco::nuovo()
            .con_file(r"\\nas\musica", vec![trovato(r"\\nas\musica\a.mp3", 10)])
            .senza_rete(r"\\nas\musica")
            .con_file(r"C:\musica", vec![trovato(r"C:\musica\rotto.mp3", 10)])
            .malformato(r"C:\musica\rotto.mp3"),
    );
    let roots = vec![r"\\nas\musica".to_owned(), r"C:\musica".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };
    let primo = scan
        .run(&mut connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione");
    assert_eq!(primo.unreadable.len(), 2);
    let ricordati: i64 = connection
        .query_row("SELECT COUNT(*) FROM file_illeggibili", [], |r| r.get(0))
        .expect("conteggio");
    assert_eq!(ricordati, 0);
    let secondo = scan
        .run(&mut connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione");
    assert_eq!((secondo.letti, secondo.illeggibili_noti), (2, 0));
}

#[test]
fn le_letture_in_parallelo_si_gestiscono_nell_ordine_del_piano() {
    // I primi due file arrivano tardi, i tre dopo subito: senza l'ordine,
    // fermarsi al quarto scriverebbe l'1, il 3, il 4 e il 5.
    let mut lib = Libreria::nuova();
    for n in 1..=8 {
        lib.brano(
            &format!("Tale/Al/{n:02}.wav"),
            &format!("Pezzo {n}"),
            "Tale",
            "Al",
        );
    }
    let disco = Arc::new(DiscoContato {
        aperture: std::sync::atomic::AtomicUsize::new(0),
        cue_rotti: std::sync::atomic::AtomicBool::new(false),
        lenti: vec!["01.wav".to_owned(), "02.wav".to_owned()],
        attesa: Duration::from_millis(300),
    });
    let roots = vec![lib.root()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: lib.store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules::for_current_platform(),
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };
    let esito = scan
        .run(&mut lib.connection, |fatti, _| {
            if fatti >= 4 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .expect("scansione");
    assert!(esito.cancelled);
    let mut statement = lib
        .connection
        .prepare("SELECT title FROM tracks ORDER BY title")
        .expect("titoli");
    let titoli: Vec<String> = statement
        .query_map([], |r| r.get(0))
        .expect("lettura")
        .collect::<Result<_, _>>()
        .expect("righe");
    assert_eq!(titoli, ["Pezzo 1", "Pezzo 2", "Pezzo 3", "Pezzo 4"]);
}

#[test]
fn una_radice_che_muore_con_letture_in_volo_non_insiste() {
    // Il primo file arriva — rotto, ma arriva — e la radice si guadagna le
    // letture in parallelo; i quattro dopo partono insieme, e la rete se
    // ne va. Il primo dei quattro scopre la radice morta con la sonda; gli
    // altri tre, già in volo, valgono saltati: niente sonda, niente elenco.
    // I file dopo non si tentano.
    let (_dir, store, mut connection) = banco();
    riga(&connection, 1, r"\\nas\musica\vecchio.mp3", 10);
    let mut file = vec![trovato(r"\\nas\musica\0-rotto.mp3", 10)];
    for lettera in ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'] {
        file.push(trovato(&format!(r"\\nas\musica\{lettera}.mp3"), 10));
    }
    let disco = Arc::new(
        FintoDisco::nuovo()
            .con_file(r"\\nas\musica", file)
            .malformato(r"\\nas\musica\0-rotto.mp3")
            .senza_rete(r"\\nas\musica")
            .cade_dopo_la_sonda(r"\\nas\musica"),
    );
    let roots = vec![r"\\nas\musica".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };
    let esito = scan
        .run(&mut connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione");
    // Il rotto, più al più le letture che erano in volo quando la rete se
    // n'è andata: dei nove file, i quattro dopo non si tentano.
    assert_eq!(disco.aperture(), 1 + LETTURE_IN_VOLO);
    assert_eq!(
        esito
            .unreadable
            .iter()
            .map(|u| u.path.as_str())
            .collect::<Vec<_>>(),
        [r"\\nas\musica\0-rotto.mp3"],
        "i file in volo su una radice morta non sono illeggibili"
    );
    assert_eq!(esito.radici_saltate, vec![r"\\nas\musica".to_owned()]);
    assert_eq!(esito.removed, 0);
}

#[test]
fn annullare_non_aspetta_le_letture_in_volo() {
    // Un file che ci mette tre secondi ad aprirsi, e un Annulla dopo un
    // terzo di secondo: si smette subito, senza aspettare che arrivi.
    let (_dir, store, mut connection) = banco();
    let disco = Arc::new(
        FintoDisco::nuovo()
            .con_file(
                r"\\nas\musica",
                vec![
                    trovato(r"\\nas\musica\a.mp3", 10),
                    trovato(r"\\nas\musica\b.mp3", 10),
                ],
            )
            .apre_lentamente(r"\\nas\musica", Duration::from_secs(3)),
    );
    let basta = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let alza = Arc::clone(&basta);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        alza.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    let chiedi = move || basta.load(std::sync::atomic::Ordering::SeqCst);
    let roots = vec![r"\\nas\musica".to_owned()];
    let scan = Scan {
        files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules {
            case_insensitive: true,
        },
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: Some(&chiedi),
    };
    let prima = Instant::now();
    let esito = scan
        .run(&mut connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione");
    assert!(esito.cancelled);
    assert!(
        prima.elapsed() < Duration::from_secs(2),
        "ha aspettato la lettura in volo: {:?}",
        prima.elapsed()
    );
}

// ── le radici del cloud: righe prima, tag dopo ─────────────────────────
//
// Un servizio finto in memoria, con i limiti di una sorgente remota: più
// letture insieme, nessun «un file alla volta», le righe prima dei tag, e
// a richiesta un tetto di letture per giro. I file sono WAV veri con i tag
// veri, come nelle prove del disco: deve reggere `lofty`, non il finto.

/// La radice del cloud di queste prove.
const NUVOLA: &str = "nuvola://onedrive-abcdefgh/Musica";

/// Quel che il servizio finto racconta a chi guarda.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Evento {
    /// Una presa del deposito finita, con quante righe c'erano dopo.
    Presa(i64),
    /// Un file aperto.
    Apertura(String),
}

/// Un lettore che risponde come il lettore dei tag del cloud oltre il suo
/// tetto: `FileTooLarge` alla prima lettura.
struct OltreIlTetto;

impl std::io::Read for OltreIlTetto {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::FileTooLarge,
            "tetto di lettura superato",
        ))
    }
}

impl std::io::Seek for OltreIlTetto {
    fn seek(&mut self, _pos: std::io::SeekFrom) -> std::io::Result<u64> {
        Ok(0)
    }
}

/// Un lettore che consegna la testa del file e poi, più avanti, fallisce
/// come il lettore dei tag del cloud quando la rete gli dice di no: un
/// errore di I/O con dentro l'`AppError` vero. Dopo la testa, perché
/// l'errore deve attraversare `lofty` — il riconoscimento del formato è
/// già passato — e arrivare a valle dentro la sua catena.
struct GuastoNelLettore {
    byte: std::io::Cursor<Vec<u8>>,
    codice: ErrorCode,
    tipo: std::io::ErrorKind,
}

/// Fin dove il lettore guasto consegna: oltre il riconoscimento del
/// formato, prima dei tag.
const TESTA_SANA: u64 = 64;

impl std::io::Read for GuastoNelLettore {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let restano = TESTA_SANA.saturating_sub(self.byte.position());
        if restano == 0 {
            return Err(std::io::Error::new(
                self.tipo,
                AppError::new(self.codice.clone()),
            ));
        }
        let quanti = buf.len().min(usize::try_from(restano).expect("piccolo"));
        let pezzo = buf.get_mut(..quanti).expect("dentro");
        std::io::Read::read(&mut self.byte, pezzo)
    }
}

impl std::io::Seek for GuastoNelLettore {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        std::io::Seek::seek(&mut self.byte, pos)
    }
}

/// Un servizio cloud finto.
struct FintoCloud {
    /// I file, per percorso: i byte e la data.
    file: std::collections::BTreeMap<String, (Vec<u8>, i64)>,
    /// Quel che il servizio sa dei tag senza leggere il file.
    noti: std::collections::HashMap<String, TagNoti>,
    /// Come si legge.
    limiti: Limiti,
    /// Gli errori da dare alle prossime aperture di un file, in ordine.
    errori:
        std::sync::Mutex<std::collections::HashMap<String, std::collections::VecDeque<ErrorCode>>>,
    /// I file la cui prossima apertura tocca il tetto del lettore.
    tetto: std::sync::Mutex<std::collections::HashSet<String>>,
    /// I file la cui prossima apertura riesce, e la cui prima lettura
    /// fallisce con un `AppError` dentro l'errore di I/O.
    nel_lettore:
        std::sync::Mutex<std::collections::HashMap<String, (ErrorCode, std::io::ErrorKind)>>,
    /// I file che si aprono con comodo, e quanto.
    lenti: std::collections::HashSet<String>,
    attesa: Duration,
    /// Quante aperture.
    aperture: std::sync::atomic::AtomicUsize,
    /// Il diario condiviso col deposito, se c'è.
    diario: Option<Arc<std::sync::Mutex<Vec<Evento>>>>,
}

impl FintoCloud {
    fn nuovo(limiti: Limiti) -> Self {
        Self {
            file: std::collections::BTreeMap::new(),
            noti: std::collections::HashMap::new(),
            limiti,
            errori: std::sync::Mutex::new(std::collections::HashMap::new()),
            tetto: std::sync::Mutex::new(std::collections::HashSet::new()),
            nel_lettore: std::sync::Mutex::new(std::collections::HashMap::new()),
            lenti: std::collections::HashSet::new(),
            attesa: Duration::ZERO,
            aperture: std::sync::atomic::AtomicUsize::new(0),
            diario: None,
        }
    }

    /// Un brano taggato sotto la radice, scritto davvero e poi tenuto in
    /// memoria.
    fn con_brano(mut self, relativo: &str, titolo: &str, artista: &str, album: &str) -> Self {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let percorso = dir.path().join("brano.wav");
        wav(&percorso);
        tagga(&percorso, titolo, artista, album);
        let byte = std::fs::read(&percorso).expect("i byte del brano");
        self.file
            .insert(format!("{NUVOLA}/{relativo}"), (byte, 1_000));
        self
    }

    /// Un file che il lettore rifiuta: spazzatura con un nome da `.mp3`.
    fn con_spazzatura(mut self, relativo: &str) -> Self {
        let quanti = usize::try_from(MIN_TRACK_BYTES * 2).expect("piccolo");
        self.file
            .insert(format!("{NUVOLA}/{relativo}"), (vec![b'x'; quanti], 1_000));
        self
    }

    fn con_noti(mut self, relativo: &str, noti: TagNoti) -> Self {
        self.noti.insert(format!("{NUVOLA}/{relativo}"), noti);
        self
    }

    fn sbaglia(&self, relativo: &str, errore: ErrorCode) {
        self.errori
            .lock()
            .expect("lucchetto")
            .entry(format!("{NUVOLA}/{relativo}"))
            .or_default()
            .push_back(errore);
    }

    fn tocca_il_tetto(&self, relativo: &str) {
        self.tetto
            .lock()
            .expect("lucchetto")
            .insert(format!("{NUVOLA}/{relativo}"));
    }

    /// Sposta i file da una cartella a un'altra, come un rinomina sul
    /// servizio.
    fn rinomina(&mut self, da: &str, a: &str) {
        let vecchi: Vec<String> = self
            .file
            .keys()
            .filter(|p| p.starts_with(&format!("{NUVOLA}/{da}/")))
            .cloned()
            .collect();
        for vecchio in vecchi {
            if let Some(dati) = self.file.remove(&vecchio) {
                let nuovo = vecchio.replacen(&format!("/{da}/"), &format!("/{a}/"), 1);
                self.file.insert(nuovo, dati);
            }
        }
    }

    fn aperture(&self) -> usize {
        self.aperture.swap(0, std::sync::atomic::Ordering::SeqCst)
    }
}

impl MusicFiles for FintoCloud {
    fn walk(&self, root: &str) -> Result<crate::files::Camminata, AppError> {
        let regole = PathRules::for_current_platform();
        Ok(crate::files::Camminata {
            file: self
                .file
                .iter()
                .filter(|(percorso, _)| is_under(percorso, root, regole))
                .map(|(percorso, (byte, data))| DiscoveredFile {
                    path: percorso.clone(),
                    size_bytes: u64::try_from(byte.len()).expect("piccolo"),
                    modified_ms: *data,
                })
                .collect(),
            completa: true,
        })
    }

    fn open(&self, path: &str) -> Result<Box<dyn crate::files::ReadSeek + Send + Sync>, AppError> {
        self.aperture
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Some(diario) = self.diario.as_ref() {
            diario
                .lock()
                .expect("diario")
                .push(Evento::Apertura(path.to_owned()));
        }
        if self.lenti.contains(path) {
            std::thread::sleep(self.attesa);
        }
        let errore = self
            .errori
            .lock()
            .expect("lucchetto")
            .get_mut(path)
            .and_then(std::collections::VecDeque::pop_front);
        if let Some(errore) = errore {
            return Err(AppError::new(errore));
        }
        if self.tetto.lock().expect("lucchetto").remove(path) {
            return Ok(Box::new(OltreIlTetto));
        }
        if let Some((codice, tipo)) = self.nel_lettore.lock().expect("lucchetto").remove(path)
            && let Some((byte, _)) = self.file.get(path)
        {
            return Ok(Box::new(GuastoNelLettore {
                byte: std::io::Cursor::new(byte.clone()),
                codice,
                tipo,
            }));
        }
        let (byte, _) = self
            .file
            .get(path)
            .ok_or_else(|| AppError::new(ErrorCode::CloudNotFound).with_cause(path.to_owned()))?;
        Ok(Box::new(std::io::Cursor::new(byte.clone())))
    }

    fn limiti(&self, _root: &str) -> Limiti {
        self.limiti
    }

    fn tag_noti(&self, path: &str) -> Option<TagNoti> {
        self.noti.get(path).cloned()
    }
}

/// I limiti di una sorgente remota, per le prove: due righe per lotto,
/// perché i lotti si vedano.
fn limiti_del_cloud(letture_per_giro: Option<usize>) -> Limiti {
    Limiti {
        in_volo: 8,
        per_file: Duration::from_secs(5),
        scadenze_di_fila: 3,
        lotto: 2,
        prima_uno_alla_volta: false,
        prima_le_righe: true,
        letture_per_giro,
    }
}

/// Una scansione a mano della radice del cloud.
fn scansiona_nuvola(
    connection: &mut Connection,
    store: &CoverStore,
    cloud: &Arc<FintoCloud>,
) -> ScanReport {
    let roots = vec![NUVOLA.to_owned()];
    let scan = Scan {
        files: Arc::clone(cloud) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules::for_current_platform(),
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };
    scan.run(connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione")
}

/// Una riga, vista da una prova: titolo, chiave di contenuto, durata.
fn riga_del(connection: &Connection, percorso: &str) -> Option<(i64, String, Option<String>, i64)> {
    connection
        .query_row(
            "SELECT id, title, content_key, duration_ms FROM tracks WHERE path = ?1",
            [percorso],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok()
}

/// Un deposito che scrive nel diario quante righe ci sono dopo ogni presa.
struct DepositoSpia {
    connection: Connection,
    diario: Arc<std::sync::Mutex<Vec<Evento>>>,
}

impl Deposito for DepositoSpia {
    fn con_connessione<T, F>(&mut self, azione: F) -> Result<T, AppError>
    where
        F: FnOnce(&mut Connection) -> Result<T, AppError>,
    {
        let esito = azione(&mut self.connection);
        let righe: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
            .expect("conteggio");
        self.diario
            .lock()
            .expect("diario")
            .push(Evento::Presa(righe));
        esito
    }
}

#[test]
fn le_righe_del_cloud_entrano_prima_delle_letture() {
    let (_dir, store, connection) = banco();
    let diario = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut cloud = FintoCloud::nuovo(limiti_del_cloud(None))
        .con_brano("A/01.wav", "Uno", "Tale", "Disco")
        .con_brano("A/02.wav", "Due", "Tale", "Disco")
        .con_brano("B/03.wav", "Tre", "Altro", "Raccolta");
    cloud.diario = Some(Arc::clone(&diario));
    let cloud = Arc::new(cloud);
    let mut deposito = DepositoSpia {
        connection,
        diario: Arc::clone(&diario),
    };
    let roots = vec![NUVOLA.to_owned()];
    let scan = Scan {
        files: Arc::clone(&cloud) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules::for_current_platform(),
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };
    let esito = scan
        .run_su(&mut deposito, |_, _| ControlFlow::Continue(()))
        .expect("scansione");

    // Il diario: tutte e tre le righe c'erano già quando si è aperto il
    // primo file.
    let eventi = diario.lock().expect("diario").clone();
    let prima_apertura = eventi
        .iter()
        .position(|e| matches!(e, Evento::Apertura(_)))
        .expect("un'apertura");
    assert!(
        eventi
            .iter()
            .take(prima_apertura)
            .any(|e| *e == Evento::Presa(3)),
        "{eventi:?}"
    );
    // E poi la lettura le ha completate, sul posto.
    assert_eq!((esito.provvisorie, esito.inserted), (3, 3));
    assert_eq!((esito.letti, esito.updated, esito.rimasti), (3, 3, 0));
    assert!(esito.unreadable.is_empty(), "{:?}", esito.unreadable);
    for (relativo, titolo) in [
        ("A/01.wav", "Uno"),
        ("A/02.wav", "Due"),
        ("B/03.wav", "Tre"),
    ] {
        let (_, letto, chiave, _) =
            riga_del(&deposito.connection, &format!("{NUVOLA}/{relativo}")).expect("la riga");
        assert_eq!(letto, titolo);
        assert!(chiave.is_some(), "{relativo}: letta, quindi con la chiave");
    }
    // Gli aggregati ci sono, come per ogni scansione.
    assert_eq!(esito.aggregates.albums, 2);
}

#[test]
fn i_tag_noti_fanno_la_riga_provvisoria_e_il_resto_i_ripieghi() {
    let (_dir, store, mut connection) = banco();
    // Nessuna lettura in questo giro: si guarda la riga come entra.
    let cloud = Arc::new(
        FintoCloud::nuovo(limiti_del_cloud(Some(0)))
            .con_brano("A/01.wav", "Dal file", "Tale", "Disco")
            .con_noti(
                "A/01.wav",
                TagNoti {
                    titolo: Some("Dal servizio".to_owned()),
                    artista: Some("Interprete".to_owned()),
                    album: Some("Album del servizio".to_owned()),
                    artista_album: Some("Interpreti vari".to_owned()),
                    genere: Some("Jazz".to_owned()),
                    anno: Some(1959),
                    traccia: Some(4),
                    disco: Some(2),
                    durata_ms: Some(321_000),
                },
            )
            .con_brano("A/Live %7C 1977.wav", "Dal file", "Tale", "Disco"),
    );
    let esito = scansiona_nuvola(&mut connection, &store, &cloud);
    assert_eq!(cloud.aperture(), 0, "niente si è letto");
    assert_eq!((esito.provvisorie, esito.letti, esito.rimasti), (2, 0, 2));

    let (titolo, artista, album, album_artist, genere, anno, traccia, disco, durata, chiave) =
        connection
            .query_row(
                "SELECT title, artist, album, album_artist, genre, year, track_number,
                            disc_number, duration_ms, content_key
                       FROM tracks WHERE path = ?1",
                [format!("{NUVOLA}/A/01.wav")],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, Option<i64>>(5)?,
                        r.get::<_, Option<i64>>(6)?,
                        r.get::<_, Option<i64>>(7)?,
                        r.get::<_, i64>(8)?,
                        r.get::<_, Option<String>>(9)?,
                    ))
                },
            )
            .expect("la riga col servizio");
    assert_eq!(
        (titolo.as_str(), artista.as_str(), album.as_str()),
        ("Dal servizio", "Interprete", "Album del servizio")
    );
    assert_eq!(album_artist.as_deref(), Some("Interpreti vari"));
    assert_eq!(genere.as_deref(), Some("Jazz"));
    assert_eq!((anno, traccia, disco), (Some(1959), Some(4), Some(2)));
    assert_eq!(durata, 321_000);
    assert_eq!(chiave, None, "provvisoria: la chiave la dirà il file");

    // Senza tag noti: i ripieghi di un file senza tag, col nome decodificato.
    let (_, titolo, chiave, durata) =
        riga_del(&connection, &format!("{NUVOLA}/A/Live %7C 1977.wav")).expect("la riga");
    assert_eq!(titolo, "Live | 1977");
    assert_eq!((chiave, durata), (None, 0));
    let artista: String = connection
        .query_row(
            "SELECT artist FROM tracks WHERE title = 'Live | 1977'",
            [],
            |r| r.get(0),
        )
        .expect("artista");
    assert_eq!(artista, rymembra_domain::album::UNKNOWN_ARTIST);

    // Dalla stessa via di ogni brano: si cerca, e l'album c'è.
    let trovati = search(&connection, "servizio", 0, 10).expect("ricerca");
    assert_eq!(trovati.len(), 1);
    let album: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM albums WHERE title = 'Album del servizio'",
            [],
            |r| r.get(0),
        )
        .expect("album");
    assert_eq!(album, 1);

    // Il giro dopo, senza tetto, le completa: stessa riga, i tag del file.
    let id_prima = riga_del(&connection, &format!("{NUVOLA}/A/01.wav")).map(|r| r.0);
    let cloud = Arc::new(
        FintoCloud::nuovo(limiti_del_cloud(None))
            .con_brano("A/01.wav", "Dal file", "Tale", "Disco")
            .con_brano("A/Live %7C 1977.wav", "Dal file 2", "Tale", "Disco"),
    );
    // Stessi byte e stessa data di prima: il piano le rilegge solo perché
    // sono provvisorie.
    let esito = scansiona_nuvola(&mut connection, &store, &cloud);
    assert_eq!((esito.letti, esito.updated, esito.inserted), (2, 2, 0));
    let (id, titolo, chiave, _) =
        riga_del(&connection, &format!("{NUVOLA}/A/01.wav")).expect("la riga");
    assert_eq!(Some(id), id_prima, "la stessa riga, completata");
    assert_eq!(titolo, "Dal file");
    assert!(chiave.is_some());
}

#[test]
fn il_tetto_per_giro_rimanda_le_letture_e_le_ritrova() {
    let (_dir, store, mut connection) = banco();
    let mut cloud = FintoCloud::nuovo(limiti_del_cloud(Some(2)));
    for n in 1..=5 {
        cloud = cloud.con_brano(
            &format!("A/{n:02}.wav"),
            &format!("Pezzo {n}"),
            "Tale",
            "Disco",
        );
    }
    let cloud = Arc::new(cloud);
    let completate = |connection: &Connection| -> i64 {
        connection
            .query_row(
                "SELECT COUNT(*) FROM tracks WHERE content_key IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .expect("conteggio")
    };

    let primo = scansiona_nuvola(&mut connection, &store, &cloud);
    assert_eq!((primo.provvisorie, primo.letti, primo.rimasti), (5, 2, 3));
    assert_eq!(cloud.aperture(), 2);
    assert_eq!(completate(&connection), 2);
    let righe: i64 = connection
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .expect("righe");
    assert_eq!(righe, 5, "tutte in libreria, lette o no");

    let secondo = scansiona_nuvola(&mut connection, &store, &cloud);
    assert_eq!(
        (secondo.provvisorie, secondo.letti, secondo.rimasti),
        (0, 2, 1)
    );
    assert_eq!(completate(&connection), 4);

    let terzo = scansiona_nuvola(&mut connection, &store, &cloud);
    assert_eq!((terzo.letti, terzo.rimasti), (1, 0));
    assert_eq!(completate(&connection), 5);
    assert_eq!(terzo.removed, 0);

    let quarto = scansiona_nuvola(&mut connection, &store, &cloud);
    assert_eq!(
        (quarto.letti, quarto.rimasti),
        (0, 0),
        "niente più da leggere"
    );
}

#[test]
fn una_provvisoria_sopravvive_a_un_guasto_passeggero() {
    let (_dir, store, mut connection) = banco();
    let cloud = Arc::new(
        FintoCloud::nuovo(limiti_del_cloud(None)).con_brano("A/01.wav", "Uno", "Tale", "Disco"),
    );
    cloud.sbaglia(
        "A/01.wav",
        ErrorCode::FsReadFailed {
            path: format!("{NUVOLA}/A/01.wav"),
            detail: None,
        },
    );
    let primo = scansiona_nuvola(&mut connection, &store, &cloud);
    assert_eq!(primo.unreadable.len(), 1);
    let (id, titolo, chiave, _) =
        riga_del(&connection, &format!("{NUVOLA}/A/01.wav")).expect("la riga resta");
    assert_eq!((titolo.as_str(), chiave), ("01", None));
    let ricordati: i64 = connection
        .query_row("SELECT COUNT(*) FROM file_illeggibili", [], |r| r.get(0))
        .expect("conteggio");
    assert_eq!(ricordati, 0, "un guasto del momento non si ricorda");

    let secondo = scansiona_nuvola(&mut connection, &store, &cloud);
    assert!(secondo.unreadable.is_empty());
    let (id_dopo, titolo, chiave, _) =
        riga_del(&connection, &format!("{NUVOLA}/A/01.wav")).expect("la riga");
    assert_eq!(id_dopo, id);
    assert_eq!(titolo, "Uno");
    assert!(chiave.is_some());
}

#[test]
fn un_rifiuto_toglie_la_provvisoria_se_nessuno_ci_ha_messo_le_mani() {
    let (_dir, store, mut connection) = banco();
    // Prima le righe, senza leggere.
    let cloud = Arc::new(
        FintoCloud::nuovo(limiti_del_cloud(Some(0)))
            .con_spazzatura("A/rotto.mp3")
            .con_spazzatura("A/ascoltato.mp3"),
    );
    let primo = scansiona_nuvola(&mut connection, &store, &cloud);
    assert_eq!(primo.provvisorie, 2);
    // Una delle due la si è già ascoltata.
    connection
        .execute(
            "UPDATE tracks SET play_count = 1 WHERE path = ?1",
            [format!("{NUVOLA}/A/ascoltato.mp3")],
        )
        .expect("un ascolto");

    let cloud = Arc::new(
        FintoCloud::nuovo(limiti_del_cloud(None))
            .con_spazzatura("A/rotto.mp3")
            .con_spazzatura("A/ascoltato.mp3"),
    );
    let secondo = scansiona_nuvola(&mut connection, &store, &cloud);
    assert_eq!(secondo.unreadable.len(), 2);
    assert_eq!(secondo.removed, 1);
    assert!(
        riga_del(&connection, &format!("{NUVOLA}/A/rotto.mp3")).is_none(),
        "come un file che non era mai entrato"
    );
    assert!(
        riga_del(&connection, &format!("{NUVOLA}/A/ascoltato.mp3")).is_some(),
        "un ascolto non si porta via per un rifiuto"
    );
    let ricordati: i64 = connection
        .query_row("SELECT COUNT(*) FROM file_illeggibili", [], |r| r.get(0))
        .expect("conteggio");
    assert_eq!(ricordati, 2, "il no si ricorda per tutti e due");

    // E la volta dopo il rifiutato non rientra: né riga, né lettura.
    let terzo = scansiona_nuvola(&mut connection, &store, &cloud);
    assert_eq!(
        (terzo.provvisorie, terzo.letti, terzo.illeggibili_noti),
        (0, 0, 2)
    );
    assert!(riga_del(&connection, &format!("{NUVOLA}/A/rotto.mp3")).is_none());
}

#[test]
fn oltre_il_tetto_si_rilegge_senza_proprieta() {
    let (_dir, store, mut connection) = banco();
    let cloud = Arc::new(
        FintoCloud::nuovo(limiti_del_cloud(None))
            .con_brano("A/01.wav", "Uno", "Tale", "Disco")
            .con_noti(
                "A/01.wav",
                TagNoti {
                    durata_ms: Some(200_000),
                    ..TagNoti::default()
                },
            )
            .con_brano("A/02.wav", "Due", "Tale", "Disco"),
    );
    cloud.tocca_il_tetto("A/01.wav");
    cloud.tocca_il_tetto("A/02.wav");
    let esito = scansiona_nuvola(&mut connection, &store, &cloud);
    assert!(esito.unreadable.is_empty(), "{:?}", esito.unreadable);
    // Due aperture per file: il tetto, e la rilettura con un lettore nuovo.
    assert_eq!(cloud.aperture(), 4);
    let (_, titolo, chiave, durata) =
        riga_del(&connection, &format!("{NUVOLA}/A/01.wav")).expect("la riga");
    assert_eq!(titolo, "Uno", "i tag si sono letti");
    assert!(chiave.is_some());
    assert_eq!(durata, 200_000, "la durata del servizio");
    let (_, titolo, _, durata) =
        riga_del(&connection, &format!("{NUVOLA}/A/02.wav")).expect("la riga");
    assert_eq!(
        (titolo.as_str(), durata),
        ("Due", 0),
        "la dirà il primo ascolto"
    );
    let ricordati: i64 = connection
        .query_row("SELECT COUNT(*) FROM file_illeggibili", [], |r| r.get(0))
        .expect("conteggio");
    assert_eq!(ricordati, 0);
}

#[test]
fn un_servizio_che_chiede_di_rallentare_rimanda_e_non_abbandona() {
    let (_dir, store, mut connection) = banco();
    // Una lettura alla volta, e una scadenza sola basterebbe ad
    // abbandonare: un `429` non deve contare come tale.
    let mut limiti = limiti_del_cloud(None);
    limiti.in_volo = 1;
    limiti.scadenze_di_fila = 1;
    let mut cloud = FintoCloud::nuovo(limiti);
    for n in 1..=4 {
        cloud = cloud.con_brano(
            &format!("A/{n:02}.wav"),
            &format!("Pezzo {n}"),
            "Tale",
            "Disco",
        );
    }
    let cloud = Arc::new(cloud);
    cloud.sbaglia(
        "A/02.wav",
        ErrorCode::NetRateLimited {
            service: Some("OneDrive".to_owned()),
            retry_after_ms: Some(30_000),
        },
    );
    cloud.sbaglia(
        "A/03.wav",
        ErrorCode::CloudQuotaExceeded {
            servizio: "OneDrive".to_owned(),
            riprova_fra_ms: None,
        },
    );

    let primo = scansiona_nuvola(&mut connection, &store, &cloud);
    assert!(primo.unreadable.is_empty(), "{:?}", primo.unreadable);
    assert!(primo.radici_saltate.is_empty());
    assert_eq!(
        (primo.letti, primo.rimasti),
        (2, 3),
        "il 2 e quel che segue"
    );
    assert_eq!(cloud.aperture(), 2, "dopo il 429 non si insiste");

    let secondo = scansiona_nuvola(&mut connection, &store, &cloud);
    assert!(secondo.unreadable.is_empty());
    assert!(secondo.radici_saltate.is_empty());
    assert_eq!((secondo.letti, secondo.rimasti), (2, 2), "la quota sul 3");

    let terzo = scansiona_nuvola(&mut connection, &store, &cloud);
    assert_eq!((terzo.letti, terzo.rimasti), (2, 0));
    let lette: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM tracks WHERE content_key IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .expect("conteggio");
    assert_eq!(lette, 4);
    let ricordati: i64 = connection
        .query_row("SELECT COUNT(*) FROM file_illeggibili", [], |r| r.get(0))
        .expect("conteggio");
    assert_eq!(ricordati, 0);
}

#[test]
fn un_guasto_del_servizio_dentro_il_lettore_non_e_un_rifiuto_del_file() {
    // Il lettore dei tag del cloud si apre, e a metà lettura la rete gli
    // risponde `429` o cade: l'errore arriva dentro un `io::Error`, e
    // attraverso `lofty`. Letto dal solo `ErrorKind` — `InvalidData` è un
    // errore «della struttura» — diventerebbe `metadata.tagReadFailed`, un
    // rifiuto del file, e la riga provvisoria se ne andrebbe.
    let (_dir, store, mut connection) = banco();
    let cloud = Arc::new(
        FintoCloud::nuovo(limiti_del_cloud(None))
            .con_brano("A/01.wav", "Uno", "Tale", "Disco")
            .con_brano("A/02.wav", "Due", "Tale", "Disco"),
    );
    cloud.nel_lettore.lock().expect("lucchetto").insert(
        format!("{NUVOLA}/A/01.wav"),
        (
            ErrorCode::NetRateLimited {
                service: Some("OneDrive".to_owned()),
                retry_after_ms: None,
            },
            std::io::ErrorKind::InvalidData,
        ),
    );
    cloud.nel_lettore.lock().expect("lucchetto").insert(
        format!("{NUVOLA}/A/02.wav"),
        (
            ErrorCode::FsNetworkUnavailable {
                path: Some(format!("{NUVOLA}/A/02.wav")),
            },
            std::io::ErrorKind::Other,
        ),
    );
    let primo = scansiona_nuvola(&mut connection, &store, &cloud);
    // Il `429` rimanda; la rete caduta è un illeggibile del momento.
    assert!(primo.rimasti >= 1, "{primo:?}");
    assert!(
        primo
            .unreadable
            .iter()
            .all(|u| u.error.code().kind() == ErrorCodeKind::FsNetworkUnavailable),
        "{:?}",
        primo.unreadable
    );
    assert_eq!(primo.removed, 0);
    for brano in ["01", "02"] {
        let (_, _, chiave, _) =
            riga_del(&connection, &format!("{NUVOLA}/A/{brano}.wav")).expect("la riga resta");
        assert_eq!(chiave, None, "{brano}: ancora provvisoria");
    }
    let ricordati: i64 = connection
        .query_row("SELECT COUNT(*) FROM file_illeggibili", [], |r| r.get(0))
        .expect("conteggio");
    assert_eq!(
        ricordati, 0,
        "niente rifiuti ricordati per un guasto del servizio"
    );

    // Il giro dopo la rete c'è, e le due righe si completano.
    let secondo = scansiona_nuvola(&mut connection, &store, &cloud);
    assert!(secondo.unreadable.is_empty(), "{:?}", secondo.unreadable);
    assert_eq!(secondo.rimasti, 0);
    for (brano, titolo) in [("01", "Uno"), ("02", "Due")] {
        let (_, letto, chiave, _) =
            riga_del(&connection, &format!("{NUVOLA}/A/{brano}.wav")).expect("la riga");
        assert_eq!(letto, titolo);
        assert!(chiave.is_some());
    }
}

#[test]
fn la_scadenza_di_un_file_del_cloud_e_quella_del_cloud() {
    // `Scadenze::default` aspetterebbe venticinque secondi; il servizio ne
    // dichiara una frazione, e vale la sua.
    let (_dir, store, mut connection) = banco();
    let mut limiti = limiti_del_cloud(None);
    limiti.per_file = Duration::from_millis(200);
    let mut cloud = FintoCloud::nuovo(limiti).con_brano("A/01.wav", "Uno", "Tale", "Disco");
    cloud.lenti.insert(format!("{NUVOLA}/A/01.wav"));
    cloud.attesa = Duration::from_secs(2);
    let cloud = Arc::new(cloud);
    let prima = Instant::now();
    let esito = scansiona_nuvola(&mut connection, &store, &cloud);
    assert!(
        prima.elapsed() < Duration::from_millis(1_500),
        "{:?}",
        prima.elapsed()
    );
    assert_eq!(
        esito
            .unreadable
            .iter()
            .map(|u| u.error.code().kind())
            .collect::<Vec<_>>(),
        [ErrorCodeKind::FsNetworkUnavailable]
    );
    // La riga provvisoria resta: una scadenza non dice niente del file.
    assert!(riga_del(&connection, &format!("{NUVOLA}/A/01.wav")).is_some());
}

/// Le statistiche di una riga del cloud, per percorso.
fn ascolti_di(connection: &Connection, percorso: &str) -> Option<(i64, i64)> {
    connection
        .query_row(
            "SELECT id, play_count FROM tracks WHERE path = ?1",
            [percorso],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok()
}

#[test]
fn una_cartella_rinominata_nel_cloud_tiene_gli_ascolti() {
    // Dove il piano ha delle sparizioni non si fanno righe provvisorie: i
    // file nuovi si leggono, e quelli che sono i brani di prima spostati
    // si riconoscono dalla chiave di contenuto.
    let (_dir, store, mut connection) = banco();
    let mut cloud = FintoCloud::nuovo(limiti_del_cloud(None))
        .con_brano("Vecchia/01.wav", "Uno", "Tale", "Disco")
        .con_brano("Vecchia/02.wav", "Due", "Tale", "Disco");
    let primo = scansiona_nuvola(
        &mut connection,
        &store,
        &Arc::new(FintoCloud {
            file: cloud.file.clone(),
            ..FintoCloud::nuovo(limiti_del_cloud(None))
        }),
    );
    assert_eq!(primo.inserted, 2);
    connection
        .execute(
            "UPDATE tracks SET play_count = 7 WHERE path = ?1",
            [format!("{NUVOLA}/Vecchia/01.wav")],
        )
        .expect("ascolti");
    let (id, _) = ascolti_di(&connection, &format!("{NUVOLA}/Vecchia/01.wav")).expect("riga");

    cloud.rinomina("Vecchia", "Nuova");
    let esito = scansiona_nuvola(&mut connection, &store, &Arc::new(cloud));
    assert_eq!(
        esito.provvisorie, 0,
        "con delle sparizioni si legge e basta"
    );
    assert_eq!((esito.moved, esito.inserted, esito.removed), (2, 0, 0));
    assert_eq!(
        ascolti_di(&connection, &format!("{NUVOLA}/Nuova/01.wav")),
        Some((id, 7))
    );
}

#[test]
fn una_sottocartella_rinominata_riguardata_dai_cambiamenti_tiene_gli_ascolti() {
    // I cambiamenti del servizio non fanno riguardare la radice ma le due
    // cartelle del rinomina, la vecchia e la nuova, in una passata della
    // sorveglianza (prudente). Fino alla 3.2.0 di prova queste passate non
    // partivano mai: una sottocartella del cloud non si raggiungeva.
    let (_dir, store, mut connection) = banco();
    let mut cloud = FintoCloud::nuovo(limiti_del_cloud(None))
        .con_brano("Rock/Vecchia/01.wav", "Uno", "Tale", "Disco")
        .con_brano("Rock/Vecchia/02.wav", "Due", "Tale", "Disco")
        .con_brano("Rock/Resta/03.wav", "Tre", "Tale", "Altro");
    scansiona_nuvola(
        &mut connection,
        &store,
        &Arc::new(FintoCloud {
            file: cloud.file.clone(),
            ..FintoCloud::nuovo(limiti_del_cloud(None))
        }),
    );
    connection
        .execute("UPDATE tracks SET play_count = 5", [])
        .expect("ascolti");

    cloud.rinomina("Rock/Vecchia", "Rock/Nuova");
    let cartelle = vec![
        format!("{NUVOLA}/Rock/Vecchia"),
        format!("{NUVOLA}/Rock/Nuova"),
    ];
    let radici = vec![NUVOLA.to_owned()];
    let scan = Scan {
        files: Arc::new(cloud) as Arc<dyn MusicFiles>,
        covers: store.clone(),
        roots: &cartelle,
        radici_della_libreria: &radici,
        rules: PathRules::for_current_platform(),
        prudente: true,
        scadenze: Scadenze::default(),
        fermati: None,
    };
    let esito = scan
        .run(&mut connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione");
    assert_eq!((esito.moved, esito.inserted, esito.removed), (2, 0, 0));
    for brano in ["01", "02"] {
        assert_eq!(
            ascolti_di(&connection, &format!("{NUVOLA}/Rock/Nuova/{brano}.wav")).map(|r| r.1),
            Some(5),
            "{brano}"
        );
    }
    let righe: i64 = connection
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .expect("righe");
    assert_eq!(righe, 3, "nessun doppione, e quel che non si è mosso resta");
}

#[test]
fn un_tetto_per_giro_non_toglie_le_sparizioni_che_non_hanno_avuto_la_loro_occasione() {
    let (_dir, store, mut connection) = banco();
    let mut cloud = FintoCloud::nuovo(limiti_del_cloud(None))
        .con_brano("Vecchia/01.wav", "Uno", "Tale", "Disco")
        .con_brano("Vecchia/02.wav", "Due", "Tale", "Disco");
    let tutto = |file: &std::collections::BTreeMap<String, (Vec<u8>, i64)>, giro| {
        Arc::new(FintoCloud {
            file: file.clone(),
            ..FintoCloud::nuovo(limiti_del_cloud(giro))
        })
    };
    scansiona_nuvola(&mut connection, &store, &tutto(&cloud.file, None));
    connection
        .execute("UPDATE tracks SET play_count = 3", [])
        .expect("ascolti");

    // Rinominata, e una lettura sola per giro: il primo brano si ritrova,
    // il secondo aspetta — e la sua riga vecchia non se ne va.
    cloud.rinomina("Vecchia", "Nuova");
    let primo = scansiona_nuvola(&mut connection, &store, &tutto(&cloud.file, Some(1)));
    assert_eq!((primo.moved, primo.removed, primo.rimasti), (1, 0, 1));
    assert!(ascolti_di(&connection, &format!("{NUVOLA}/Vecchia/02.wav")).is_some());

    let secondo = scansiona_nuvola(&mut connection, &store, &tutto(&cloud.file, Some(1)));
    assert_eq!((secondo.moved, secondo.removed, secondo.rimasti), (1, 0, 0));
    for brano in ["01", "02"] {
        assert_eq!(
            ascolti_di(&connection, &format!("{NUVOLA}/Nuova/{brano}.wav")).map(|r| r.1),
            Some(3),
            "{brano}"
        );
    }
    let righe: i64 = connection
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .expect("righe");
    assert_eq!(righe, 2);
}

#[test]
fn le_radici_del_disco_accanto_al_cloud_leggono_come_prima() {
    // Le stesse sorgenti che il desktop userà: il disco e il cloud dietro
    // `Archivi`, che sceglie percorso per percorso. Sul disco niente righe
    // provvisorie e niente tetto; nel cloud sì.
    let mut lib = Libreria::nuova();
    lib.brano("Tale/Al/01.wav", "Locale 1", "Tale", "Al");
    lib.brano("Tale/Al/02.wav", "Locale 2", "Tale", "Al");
    let cloud = Arc::new(
        FintoCloud::nuovo(limiti_del_cloud(Some(0)))
            .con_brano("A/01.wav", "Remoto", "Tale", "Disco"),
    );
    let archivi = crate::files::Archivi {
        locali: LocalFiles,
        nuvola: Some(Arc::clone(&cloud) as Arc<dyn MusicFiles>),
    };
    let roots = vec![lib.root(), NUVOLA.to_owned()];
    let scan = Scan {
        files: Arc::new(archivi),
        covers: lib.store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules::for_current_platform(),
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };
    let esito = scan
        .run(&mut lib.connection, |_, _| ControlFlow::Continue(()))
        .expect("scansione");
    assert_eq!(esito.inserted, 3);
    assert_eq!((esito.provvisorie, esito.letti, esito.rimasti), (1, 2, 1));
    let locali_lette: i64 = lib
        .connection
        .query_row(
            "SELECT COUNT(*) FROM tracks
                  WHERE content_key IS NOT NULL AND title LIKE 'Locale %'",
            [],
            |r| r.get(0),
        )
        .expect("conteggio");
    assert_eq!(locali_lette, 2);
    assert_eq!(cloud.aperture(), 0);
}

#[test]
fn la_camminata_si_racconta_e_i_tempi_si_contano() {
    let mut lib = Libreria::nuova();
    lib.brano("Uno/A/01.wav", "A1", "Uno", "A");
    lib.brano("Uno/B/01.wav", "B1", "Uno", "B");
    lib.brano("Due/C/01.wav", "C1", "Due", "C");
    let roots = vec![lib.root()];
    let scan = Scan {
        files: Arc::new(LocalFiles),
        covers: lib.store.clone(),
        roots: &roots,
        radici_della_libreria: &roots,
        rules: PathRules::for_current_platform(),
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: None,
    };
    let mut visti = Vec::new();
    let esito = scan
        .run_con(&mut lib.connection, |avanzamento| {
            visti.push(avanzamento);
            ControlFlow::Continue(())
        })
        .expect("scansione");
    assert!(
        visti.contains(&Avanzamento::Camminata {
            cartelle: esito.cartelle_viste,
            file: 3
        }),
        "{visti:?}"
    );
    assert!(
        esito.cartelle_viste >= 5,
        "la radice, due artisti, tre album"
    );
    assert_eq!(esito.file_visti, 3);
    assert_eq!(esito.letti, 3);
    assert_eq!(
        visti.last(),
        Some(&Avanzamento::Lettura {
            fatti: 3,
            totale: 3
        })
    );
    // La camminata viene prima della lettura, sempre.
    let prima_lettura = visti
        .iter()
        .position(|a| matches!(a, Avanzamento::Lettura { .. }))
        .expect("una lettura");
    assert!(
        visti
            .iter()
            .skip(prima_lettura)
            .all(|a| matches!(a, Avanzamento::Lettura { .. }))
    );
}
