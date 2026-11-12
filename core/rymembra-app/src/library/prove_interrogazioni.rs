//! Le prove degli ordinamenti di `interrogazioni`, sui dati e non sul piano.
//!
//! Il piano di SQLite lo guarda `tests.rs` (`gli_elenchi_non_ordinano_…`):
//! qui si guarda **che cosa** esce, in che ordine, su righe scritte a mano.

use rusqlite::Connection;

use super::*;

/// Una libreria vuota, con lo schema aggiornato.
fn libreria() -> Connection {
    crate::db::open_in_memory().expect("database").connection
}

/// Un brano con quel che serve a ordinarlo.
struct Riga<'a> {
    id: i64,
    path: &'a str,
    title: &'a str,
    artist: &'a str,
    album: &'a str,
    album_key: &'a str,
    disco: Option<i64>,
    traccia: Option<i64>,
    aggiunto: i64,
}

fn scrivi(connection: &Connection, riga: &Riga<'_>) {
    connection
        .execute(
            "INSERT INTO tracks
               (id, path, track_key, title, artist, album, album_key,
                disc_number, track_number, duration_ms, file_size,
                date_added, date_modified)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 200000, 1, ?10, 1)",
            rusqlite::params![
                riga.id,
                riga.path,
                format!("k{}", riga.id),
                riga.title,
                riga.artist,
                riga.album,
                riga.album_key,
                riga.disco,
                riga.traccia,
                riga.aggiunto,
            ],
        )
        .expect("brano");
}

fn ids(brani: &[TrackSummary]) -> Vec<i64> {
    brani.iter().map(|b| b.id).collect()
}

#[test]
fn recenti_a_parita_di_data_tiene_l_ordine_del_disco() {
    // Una scansione scrive un lotto intero con lo stesso istante: a parità di
    // `date_added` decideva `id DESC`, cioè l'ordine di lettura rovesciato, e
    // «Ascolta» del primo avvio partiva dall'ultima traccia dell'ultimo disco.
    let connection = libreria();
    let stesso = 1_700_000_000_000;
    let riga = |id, path, album_key, disco, traccia, aggiunto| Riga {
        id,
        path,
        title: "Parte",
        artist: "Tizio",
        album: "Disco",
        album_key,
        disco,
        traccia,
        aggiunto,
    };
    // Inseriti nell'ordine di lettura della scansione: cartella, disco, traccia.
    for r in [
        riga(1, "D:/M/A/01.flac", "a d:/m/a", Some(1), Some(1), stesso),
        riga(2, "D:/M/A/02.flac", "a d:/m/a", Some(1), Some(2), stesso),
        riga(3, "D:/M/A/03.flac", "a d:/m/a", Some(1), Some(3), stesso),
        riga(
            4,
            "D:/M/B/CD2/01.flac",
            "b d:/m/b",
            Some(2),
            Some(1),
            stesso,
        ),
        riga(
            5,
            "D:/M/B/CD1/01.flac",
            "b d:/m/b",
            Some(1),
            Some(1),
            stesso,
        ),
        // Entrato dopo: sta in cima, come deve.
        riga(
            6,
            "D:/M/C/01.flac",
            "c d:/m/c",
            None,
            Some(1),
            stesso + 60_000,
        ),
    ] {
        scrivi(&connection, &r);
    }
    let brani = list_tracks(&connection, TrackOrder::RecentlyAdded, 0, 10).expect("elenco");
    assert_eq!(ids(&brani), vec![6, 1, 2, 3, 5, 4]);
}
