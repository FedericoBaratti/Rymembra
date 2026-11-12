//! Le prove della regola con cui si costruiscono gli aggregati, quando cambia.

use rusqlite::Connection;

use super::*;

/// Una libreria vuota, con lo schema aggiornato.
fn libreria() -> Connection {
    crate::db::open_in_memory().expect("database").connection
}

/// Un brano su file, con la chiave d'album che gli si dà.
fn su_file(connection: &Connection, id: i64, path: &str, album: &str, album_key: &str) {
    connection
        .execute(
            "INSERT INTO tracks
               (id, path, track_key, title, artist, album, album_key,
                duration_ms, file_size, date_added, date_modified)
             VALUES (?1, ?2, ?3, 'Brano', 'Pink Floyd', ?4, ?5, 200000, 1, 1, 1)",
            rusqlite::params![id, path, format!("k{id}"), album, album_key],
        )
        .expect("brano");
}

fn versione(connection: &Connection) -> i64 {
    connection
        .query_row(
            "SELECT aggregati FROM libreria_stato WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .expect("versione")
}

fn chiavi(connection: &Connection) -> Vec<(i64, String)> {
    let mut statement = connection
        .prepare("SELECT id, album_key FROM tracks ORDER BY id")
        .expect("chiavi");
    statement
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .expect("chiavi")
        .collect::<Result<Vec<_>, _>>()
        .expect("chiavi")
}

#[test]
fn una_libreria_della_regola_di_prima_si_rimette_in_riga() {
    // Una libreria costruita con la regola di prima: i due dischi di «The
    // Wall» in due cartelle «(CD1)» e «(CD2)», ognuno con la sua chiave, e un
    // brano di catalogo che il percorso non ce l'ha. Alla prima ricostruzione
    // intera le chiavi si rifanno dalla regola di adesso, e il disco è uno.
    let mut connection = libreria();
    let percorsi = [
        (1, "C:/M/Pink Floyd/The Wall (CD1)/01.flac"),
        (2, "C:/M/Pink Floyd/The Wall (CD1)/02.flac"),
        (3, "C:/M/Pink Floyd/The Wall (CD2)/01.flac"),
        (4, "C:/M/Pink Floyd/The Wall (CD2)/02.flac"),
    ];
    for (id, path) in percorsi {
        let cartella = if id <= 2 { "(cd1)" } else { "(cd2)" };
        su_file(
            &connection,
            id,
            path,
            "The Wall",
            &format!("the wall c:/m/pink floyd/the wall {cartella}"),
        );
    }
    let indirizzo = "https://archive.org/download/concerto/01.mp3";
    connection
        .execute(
            "INSERT INTO tracks
               (id, path, track_key, title, artist, album, album_key, duration_ms,
                date_added, source, source_service, fonte_url, licenza, disponibilita)
             VALUES (5, NULL, 'k5', 'Dal vivo', 'Qualcuno', 'Concerto', 'vecchia', 1000,
                     1, 'catalogo', 'archive', ?1, 'pubblicoDominio', 'soloAscolto')",
            [indirizzo],
        )
        .expect("brano di catalogo");
    connection
        .execute("UPDATE libreria_stato SET aggregati = 2 WHERE id = 1", [])
        .expect("versione di prima");

    let tx = transazione_di_scrittura(&mut connection).expect("transazione");
    rebuild_aggregates(&tx).expect("ricostruzione");
    tx.commit().expect("commit");

    let disco = album_group_key("The Wall", "C:/M/Pink Floyd/The Wall/01.flac");
    assert_eq!(
        chiavi(&connection),
        vec![
            (1, disco.clone()),
            (2, disco.clone()),
            (3, disco.clone()),
            (4, disco.clone()),
            (5, album_group_key("Concerto", indirizzo)),
        ]
    );
    let album: i64 = connection
        .query_row(
            "SELECT total_tracks FROM albums WHERE album_key = ?1",
            [&disco],
            |r| r.get(0),
        )
        .expect("un album solo");
    assert_eq!(album, 4);
    assert_eq!(versione(&connection), VERSIONE_AGGREGATI);
}

#[test]
fn una_libreria_appena_nata_non_rifa_le_chiavi() {
    // Lo zero è «non si sa»: un database nuovo, o «Rileggi tutti i tag». Le
    // chiavi le scrive chi scrive i brani, e la ricostruzione le prende come
    // sono.
    let mut connection = libreria();
    su_file(
        &connection,
        1,
        "C:/M/A/01.flac",
        "Disco",
        "una chiave qualunque",
    );
    assert_eq!(versione(&connection), 0);
    let tx = transazione_di_scrittura(&mut connection).expect("transazione");
    rebuild_aggregates(&tx).expect("ricostruzione");
    tx.commit().expect("commit");
    assert_eq!(
        chiavi(&connection),
        vec![(1, "una chiave qualunque".to_owned())]
    );
}
