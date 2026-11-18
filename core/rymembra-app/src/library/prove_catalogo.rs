//! Le prove del catalogo che questo dispositivo serve agli altri
//! (`condivisa::riassunto_catalogo`, `pagina_catalogo`, `valuta`): a secchi di
//! id, con l'impronta che cambia solo dove cambia qualcosa, ricordato per
//! generazione; e mai le righe specchio, che sono di un altro dispositivo.

use std::sync::Mutex;

use rusqlite::Connection;

use super::condivisa::{self, ID_PER_SECCHIO, Ricordate};
use crate::specchio::comuni::{ADESSO, chiave};

fn libreria() -> Connection {
    crate::db::open_in_memory().expect("database").connection
}

fn brano(c: &Connection, id: i64, path: &str) {
    c.execute(
        "INSERT INTO tracks (id, path, track_key, title, artist, album, album_key, duration_ms,
                             file_size, date_added, date_modified, codec, bitrate)
         VALUES (?1, ?2, ?3, ?4, 'A', 'Al', 'al', 180000, 1000, 1, 1, 'Flac', 900)",
        rusqlite::params![id, path, format!("k{id}"), format!("T{id}")],
    )
    .expect("brano");
}

fn locali(c: &Connection, ids: impl IntoIterator<Item = i64>) {
    for id in ids {
        brano(c, id, &format!("C:/M/{id}.flac"));
    }
}

#[test]
fn le_righe_specchio_non_si_ricondividono_in_nessuna_domanda() {
    // §4.7: un terzo dispositivo che le vedesse attraverso questo avrebbe
    // catene, doppioni e ascolti contati al proprietario sbagliato.
    let c = libreria();
    locali(&c, [1, 2]);
    brano(&c, 3, &format!("lontano://{}/77", chiave('a')));
    brano(&c, 4, "nuvola://onedrive-abcdefgh/Musica/c.flac");
    // Il resto dei campi di una riga specchio, perché sia una riga vera di libreria.
    c.execute("UPDATE tracks SET source = 'lontano' WHERE id = 3", [])
        .unwrap();
    let id = |v: &[condivisa::BranoRemoto]| v.iter().map(|b| b.id).collect::<Vec<_>>();
    assert_eq!(
        id(&condivisa::brani(&c, super::TrackOrder::Title, 0, 50).unwrap()),
        vec![1, 2]
    );
    assert_eq!(
        id(&condivisa::brani_diretti(&c, super::TrackOrder::Shelf, 0, 50).unwrap()),
        vec![1, 2]
    );
    assert!(condivisa::brano(&c, 3).unwrap().is_none());
    assert!(condivisa::file_del_brano(&c, 3).unwrap().is_none());
    assert_eq!(
        id(&condivisa::cerca(&c, "T3", 0, 50).unwrap()),
        Vec::<i64>::new()
    );
    assert_eq!(condivisa::cerca_conteggio(&c, "T3").unwrap(), 0);
    assert_eq!(id(&condivisa::cerca(&c, "T2", 0, 50).unwrap()), vec![2]);
    // Né il catalogo.
    let riassunto = condivisa::riassunto_catalogo(&c).unwrap();
    assert_eq!(riassunto.secchi.iter().map(|s| s.righe).sum::<i64>(), 2);
    let pagina = condivisa::pagina_catalogo(&c, 0, ID_PER_SECCHIO - 1).unwrap();
    assert_eq!(
        pagina.brani.iter().map(|b| b.id).collect::<Vec<_>>(),
        vec![1, 2]
    );
    // E una valutazione per una riga specchio è «non più condivisa».
    let err = condivisa::valuta(&c, 3, Some(true), None, ADESSO - 5, ADESSO).unwrap_err();
    assert_eq!(
        err.code().kind(),
        rymembra_domain::errors::ErrorCodeKind::LontanoFileCambiato
    );
}

#[test]
fn i_secchi_sono_intervalli_di_500_id_stabili() {
    let c = libreria();
    // Id sparsi: 1..=3 (secchio 0), 499 (0), 500 (1), 1200 (2).
    locali(&c, [1, 2, 3, 499, 500, 1200]);
    let r = condivisa::riassunto_catalogo(&c).unwrap();
    let forma: Vec<(i64, i64, i64)> = r
        .secchi
        .iter()
        .map(|s| (s.da_id, s.a_id, s.righe))
        .collect();
    assert_eq!(forma, vec![(0, 499, 4), (500, 999, 1), (1000, 1499, 1)]);
    assert!(r.secchi.iter().all(|s| s.impronta.len() == 32));
    // Togliere un brano in mezzo non sposta gli altri secchi: le loro impronte
    // restano le stesse.
    let prima: Vec<String> = r.secchi.iter().map(|s| s.impronta.clone()).collect();
    c.execute("DELETE FROM tracks WHERE id = 2", []).unwrap();
    let dopo = condivisa::riassunto_catalogo(&c).unwrap();
    assert_eq!(dopo.secchi.len(), 3);
    assert_ne!(dopo.secchi[0].impronta, prima[0], "il suo secchio cambia");
    assert_eq!(dopo.secchi[1].impronta, prima[1]);
    assert_eq!(dopo.secchi[2].impronta, prima[2]);
    assert_eq!(dopo.secchi[0].righe, 3);
}

#[test]
fn un_brano_ascoltato_cambia_un_secchio_solo() {
    let c = libreria();
    locali(&c, [1, 2, 600, 601, 1300]);
    let prima = condivisa::riassunto_catalogo(&c).unwrap();
    c.execute(
        "UPDATE tracks SET play_count = 5, last_played_at = 1000, stats_updated_at = 1000 WHERE id = 601",
        [],
    )
    .unwrap();
    let dopo = condivisa::riassunto_catalogo(&c).unwrap();
    assert_ne!(dopo.generazione, prima.generazione);
    let diversi: Vec<i64> = prima
        .secchi
        .iter()
        .zip(&dopo.secchi)
        .filter(|(a, b)| a.impronta != b.impronta)
        .map(|(a, _)| a.da_id)
        .collect();
    assert_eq!(diversi, vec![500], "solo quello del brano 601");
    // Anche un voto, un mi piace e un titolo.
    for sql in [
        "UPDATE tracks SET liked = 1, liked_at = 5 WHERE id = 1",
        "UPDATE tracks SET rating = 3, rating_at = 5 WHERE id = 1",
        "UPDATE tracks SET title = 'Altro' WHERE id = 1",
    ] {
        let p = condivisa::riassunto_catalogo(&c).unwrap();
        c.execute(sql, []).unwrap();
        let d = condivisa::riassunto_catalogo(&c).unwrap();
        assert_ne!(p.secchi[0].impronta, d.secchi[0].impronta, "{sql}");
        assert_eq!(p.secchi[1].impronta, d.secchi[1].impronta, "{sql}");
    }
}

#[test]
fn il_riassunto_si_ricorda_per_generazione() {
    let c = libreria();
    locali(&c, [1, 2, 3]);
    let cache = Mutex::new(Ricordate::default());
    let a = condivisa::riassunto_con(&c, &cache).unwrap();
    let b = condivisa::riassunto_con(&c, &cache).unwrap();
    assert_eq!(a, b);
    assert_eq!(
        cache.lock().unwrap().costruiti(),
        1,
        "la seconda volta dalla memoria"
    );
    // Una scrittura su tracks cambia la generazione: si ricalcola.
    c.execute("UPDATE tracks SET play_count = 9 WHERE id = 1", [])
        .unwrap();
    let d = condivisa::riassunto_con(&c, &cache).unwrap();
    assert_ne!(d, a);
    assert_eq!(cache.lock().unwrap().costruiti(), 2);
    // E si tengono al più due generazioni.
    for n in 0..5 {
        c.execute("UPDATE tracks SET play_count = ?1 WHERE id = 1", [n + 20])
            .unwrap();
        condivisa::riassunto_con(&c, &cache).unwrap();
    }
    assert!(cache.lock().unwrap().catalogo_ricordati() <= 2);
}

#[test]
fn una_pagina_porta_le_righe_del_secchio_e_nei_tetti() {
    let c = libreria();
    locali(&c, [1, 2, 499, 500]);
    c.execute(
        "UPDATE tracks SET title = ?1, album_artist = 'AA', genre = 'G', content_key = 'a|b|c',
                           mb_recording_id = '5b11f4ce-a62d-471e-81fc-a69a8278c7da',
                           liked = 1, liked_at = 77, rating = 4, rating_at = 78
         WHERE id = 2",
        [format!("é{}", "x".repeat(900))],
    )
    .unwrap();
    let p = condivisa::pagina_catalogo(&c, 0, 499).unwrap();
    assert_eq!((p.da_id, p.a_id), (0, 499));
    assert_eq!(
        p.brani.iter().map(|b| b.id).collect::<Vec<_>>(),
        vec![1, 2, 499]
    );
    let due = &p.brani[1];
    assert!(
        due.title.len() <= crate::specchio::convalida::MAX_NOME,
        "{}",
        due.title.len()
    );
    assert!(due.title.starts_with('é'));
    assert_eq!(due.album_artist.as_deref(), Some("AA"));
    assert_eq!(due.content_key.as_deref(), Some("a|b|c"));
    assert_eq!(due.track_key, "k2");
    assert_eq!(
        due.mb_recording_id.as_deref(),
        Some("5b11f4ce-a62d-471e-81fc-a69a8278c7da")
    );
    assert_eq!(
        (due.liked, due.liked_at, due.rating, due.rating_at),
        (true, Some(77), 4, Some(78))
    );
    assert_eq!(due.codec.as_deref(), Some("Flac"));
    assert_eq!(due.bitrate, Some(900));
    assert_eq!(due.estensione, "flac");
    // Un secchio più largo di quanto si serve, o al contrario, no.
    for (da, a) in [(0, 500), (10, 5), (-1, 10), (0, 100_000)] {
        let err = condivisa::pagina_catalogo(&c, da, a).unwrap_err();
        assert_eq!(
            err.code().kind(),
            rymembra_domain::errors::ErrorCodeKind::LontanoProtocollo,
            "{da}..{a}"
        );
    }
}

#[test]
fn una_valutazione_vince_per_campo_con_il_suo_orologio_ed_e_innocua_due_volte() {
    let c = libreria();
    locali(&c, [1]);
    // Il proprietario ha votato 5 stelle all'istante 100, e nessun mi piace.
    c.execute(
        "UPDATE tracks SET rating = 5, rating_at = 100, stats_updated_at = 100 WHERE id = 1",
        [],
    )
    .unwrap();
    // Un mi piace arrivato da fuori a 50 e le stelle a 60: il mi piace entra
    // (nessuno l'aveva deciso), le stelle no (là sono più nuove).
    assert!(condivisa::valuta(&c, 1, Some(true), Some(2), 60, ADESSO).unwrap());
    let stato = |c: &Connection| -> (i64, Option<i64>, i64, Option<i64>, i64) {
        c.query_row(
            "SELECT liked, liked_at, rating, rating_at, stats_updated_at FROM tracks WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap()
    };
    assert_eq!(stato(&c), (1, Some(60), 5, Some(100), 100));
    // Innocuo se arriva due volte: stesso orologio, non è più recente.
    assert!(!condivisa::valuta(&c, 1, Some(false), Some(0), 60, ADESSO).unwrap());
    assert_eq!(stato(&c), (1, Some(60), 5, Some(100), 100));
    // Più nuovo, vince: e `stats_updated_at` non torna mai indietro.
    assert!(condivisa::valuta(&c, 1, None, Some(1), 150, ADESSO).unwrap());
    assert_eq!(stato(&c), (1, Some(60), 1, Some(150), 150));
    assert!(condivisa::valuta(&c, 1, Some(false), None, 120, ADESSO).unwrap());
    assert_eq!(stato(&c), (0, Some(120), 1, Some(150), 150));
    // Un orologio avanti non fissa un voto nel futuro.
    assert!(condivisa::valuta(&c, 1, None, Some(4), ADESSO + 10_000_000_000, ADESSO).unwrap());
    let (_, _, voto, quando, _) = stato(&c);
    assert_eq!((voto, quando), (4, Some(ADESSO)));
}

#[test]
fn una_valutazione_storta_o_per_un_brano_che_non_c_e_si_rifiuta() {
    let c = libreria();
    locali(&c, [1]);
    let codice = |e: rymembra_domain::errors::AppError| e.code().kind();
    use rymembra_domain::errors::ErrorCodeKind as K;
    assert_eq!(
        codice(condivisa::valuta(&c, 1, None, Some(6), 10, ADESSO).unwrap_err()),
        K::LontanoProtocollo
    );
    assert_eq!(
        codice(condivisa::valuta(&c, 1, None, Some(-1), 10, ADESSO).unwrap_err()),
        K::LontanoProtocollo
    );
    assert_eq!(
        codice(condivisa::valuta(&c, 1, Some(true), None, 0, ADESSO).unwrap_err()),
        K::LontanoProtocollo
    );
    assert_eq!(
        codice(condivisa::valuta(&c, 99, Some(true), None, 10, ADESSO).unwrap_err()),
        K::LontanoFileCambiato
    );
    // Senza niente da dire non cambia niente.
    assert!(!condivisa::valuta(&c, 1, None, None, 10, ADESSO).unwrap());
}

#[test]
fn la_scansione_dello_stesso_originale_prende_il_posto_della_copertina_nata_da_fuori() {
    // Advisor, punto 4: la riga di `cover_art` nata per una riga specchio
    // (`source = 'lontano'`, le misure della media) non deve tenere il posto
    // della copertina vera di questa macchina, che `INSERT OR IGNORE` lasciava
    // com'era per sempre. Una riga di altra origine invece non si tocca.
    let c = libreria();
    let da_fuori = "ef".repeat(32);
    let locale = "ab".repeat(32);
    for (hash, sorgente, larghezza) in [(&da_fuori, "lontano", 320), (&locale, "tag", 700)] {
        c.execute(
            "INSERT INTO cover_art (hash, mime_type, width, height, byte_size, source, created_at)
             VALUES (?1, 'image/jpeg', ?2, ?2, 20000, ?3, 1)",
            rusqlite::params![hash, larghezza, sorgente],
        )
        .unwrap();
    }
    for (n, hash) in [&da_fuori, &locale].into_iter().enumerate() {
        let file = super::DiscoveredFile {
            path: format!("C:/M/{n}.flac"),
            size_bytes: 5_000_000,
            modified_ms: 1,
        };
        let mut riga = super::righe::riga_provvisoria(&file, None);
        riga.cover = Some(crate::covers::StoredCover {
            hash: hash.clone(),
            width: 640,
            height: 640,
            byte_size: 90_000,
            mime_type: "image/jpeg",
            already_present: false,
        });
        let tx = c.unchecked_transaction().unwrap();
        super::insert_track(&tx, &riga, 5).unwrap();
        tx.commit().unwrap();
    }
    let cosi = |hash: &str| -> (String, i64) {
        c.query_row(
            "SELECT source, width FROM cover_art WHERE hash = ?1",
            [hash],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    };
    assert_eq!(
        cosi(&da_fuori),
        ("tag".to_owned(), 640),
        "la vera prende il posto"
    );
    assert_eq!(
        cosi(&locale),
        ("tag".to_owned(), 700),
        "una già di qui non si tocca"
    );
}
