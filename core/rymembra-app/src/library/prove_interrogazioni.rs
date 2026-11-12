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

#[test]
fn trascurati_non_contiene_un_brano_appena_aggiunto() {
    // «Non li ascolti da un po'» al primo avvio si riempiva di dodici brani a
    // caso appena scansionati: mai ascoltati, quindi «non negli ultimi 180
    // giorni». Un brano entrato da meno di trenta giorni non è trascurato, è
    // nuovo (D-63).
    let connection = libreria();
    let giorno = 86_400_000_i64;
    let adesso = 1_800_000_000_000_i64;
    let percorsi: Vec<String> = (1..=5).map(|id| format!("D:/M/{id}.flac")).collect();
    for ((id, aggiunto), path) in [
        (1, adesso),               // appena entrato, mai ascoltato
        (2, adesso - 10 * giorno), // entrato dieci giorni fa
        (3, adesso - 40 * giorno), // in libreria da più di un mese, mai sentito
        (4, adesso - 400 * giorno),
        (5, adesso - 400 * giorno),
    ]
    .into_iter()
    .zip(&percorsi)
    {
        scrivi(
            &connection,
            &Riga {
                id,
                path,
                title: "Brano",
                artist: "Tizio",
                album: "Disco",
                album_key: "disco d:/m",
                disco: None,
                traccia: None,
                aggiunto,
            },
        );
    }
    // Il 4 è fermo da sette mesi, il 5 l'hai sentito la settimana scorsa.
    for (id, giorni_fa) in [(4, 210), (5, 7)] {
        connection
            .execute(
                "UPDATE tracks SET play_count = 3, last_played_at = ?2 WHERE id = ?1",
                rusqlite::params![id, adesso - giorni_fa * giorno],
            )
            .expect("ascolto");
    }
    let mut trovati = ids(&trascurati(&connection, adesso, 12).expect("trascurati"));
    trovati.sort_unstable();
    assert_eq!(trovati, vec![3, 4]);
}

#[test]
fn album_e_artisti_ordinano_allo_stesso_modo() {
    // Lo stesso nome in due posti diversi: «The Beatles» sotto T negli Album e
    // sotto B negli Artisti, «Édith Piaf» dopo «ZZ Top» negli Album e sotto E
    // negli Artisti.
    let mut connection = libreria();
    for (id, artista) in [
        (1, "ZZ Top"),
        (2, "Édith Piaf"),
        (3, "The Beatles"),
        (4, "Abba"),
        (5, "Ólafur Arnalds"),
    ] {
        let path = format!("D:/M/{id}/01.flac");
        let album_key = format!("disco d:/m/{id}");
        scrivi(
            &connection,
            &Riga {
                id,
                path: &path,
                title: "Brano",
                artist: artista,
                album: "Disco",
                album_key: &album_key,
                disco: None,
                traccia: Some(1),
                aggiunto: 1,
            },
        );
    }
    let tx = transazione_di_scrittura(&mut connection).expect("transazione");
    rebuild_aggregates(&tx).expect("aggregati");
    tx.commit().expect("commit");

    let album: Vec<String> = list_albums(&connection, 0, 10)
        .expect("album")
        .into_iter()
        .map(|a| a.artist)
        .collect();
    let artisti: Vec<String> = list_artists(&connection)
        .expect("artisti")
        .into_iter()
        .map(|a| a.name)
        .collect();
    assert_eq!(
        album,
        [
            "Abba",
            "The Beatles",
            "Édith Piaf",
            "Ólafur Arnalds",
            "ZZ Top"
        ]
    );
    assert_eq!(album, artisti);
}

#[test]
fn i_brani_ordinano_per_nome_come_artisti_e_album() {
    // `COLLATE NOCASE` piega solo l'ASCII: «È sera» e «Ça plane pour moi»
    // finivano dopo «Zombie», e «The Beatles» stava sotto T. I Brani per
    // titolo e per scaffale ordinano ora con la chiave degli Artisti e degli
    // Album (D-104), scritta nelle colonne della migrazione 042.
    let mut connection = libreria();
    for (id, titolo, artista) in [
        (1, "Zombie", "ZZ Top"),
        (2, "È sera", "Édith Piaf"),
        (3, "Alba", "The Beatles"),
        (4, "Ça plane pour moi", "Abba"),
        (5, "The Wall", "Ólafur Arnalds"),
    ] {
        let path = format!("D:/M/{id}.flac");
        scrivi(
            &connection,
            &Riga {
                id,
                path: &path,
                title: titolo,
                artist: artista,
                album: "Disco",
                album_key: "disco d:/m",
                disco: None,
                traccia: None,
                aggiunto: 1,
            },
        );
    }
    // Chi scrive i brani ricostruisce gli aggregati: è lì che le chiavi si
    // riempiono.
    let tx = transazione_di_scrittura(&mut connection).expect("transazione");
    rebuild_aggregates(&tx).expect("aggregati");
    tx.commit().expect("commit");

    let titoli: Vec<String> = list_tracks(&connection, TrackOrder::Title, 0, 10)
        .expect("per titolo")
        .into_iter()
        .map(|b| b.title)
        .collect();
    assert_eq!(
        titoli,
        ["Alba", "Ça plane pour moi", "È sera", "The Wall", "Zombie"]
    );
    let artisti: Vec<String> = list_tracks(&connection, TrackOrder::Shelf, 0, 10)
        .expect("per scaffale")
        .into_iter()
        .map(|b| b.artist)
        .collect();
    assert_eq!(
        artisti,
        [
            "Abba",
            "The Beatles",
            "Édith Piaf",
            "Ólafur Arnalds",
            "ZZ Top"
        ]
    );
}
