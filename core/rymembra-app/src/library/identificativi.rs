//! Gli identificativi di una vista intera, nell'ordine delle sue pagine.
//!
//! # PerchÃ© non le pagine con un limite di âˆ’1
//!
//! Â«Seleziona tuttoÂ» e Â«Suona da quiÂ» su una lista rada vogliono gli
//! identificativi di tutta la vista, e li prendevano chiedendo una pagina senza
//! fondo e tenendo l'id: diciassette colonne di tutte le righe, percorsi
//! compresi, per buttarle â€” su 29 183 brani 198 ms contro 17 ms chiedendo il
//! solo `t.id`, col lucchetto della libreria in mano, cioÃ¨ con cuori, voti e
//! ogni altra lettura in fila dietro (R21-6).
//!
//! Qui le stesse domande chiedono soltanto l'id, con **la stessa** clausola
//! d'ordine e la stessa origine delle pagine: [`TrackOrder::sql`],
//! [`da_preferiti`], [`da_ricerca`] e la stessa [`fts_query`]. Un secondo
//! ordine scritto qui sarebbe una selezione che non corrisponde alle righe che
//! si vedono, il giorno in cui uno dei due cambia. E senza le altre colonne la
//! lettura dei Brani sta tutta nell'indice dell'ordinamento.

use rusqlite::Connection;
use rymembra_domain::errors::AppError;

use super::db_error;
use super::interrogazioni::{TrackOrder, da_preferiti, da_ricerca, dove, fts_query};
use crate::specchio::Origine;

/// Gli identificativi di tutta la libreria, nell'ordine di [`super::list_tracks`].
///
/// # Errori
///
/// `db.*` se la lettura fallisce.
pub fn id_dei_brani(
    connection: &Connection,
    origine: &Origine,
    order: TrackOrder,
) -> Result<Vec<i64>, AppError> {
    // «Tutti» con righe nascoste (dietro una copia migliore, o in grazia): l'ordine
    // si legge dal solo indice, senza il filtro riga per riga — che costringeva a
    // leggere ogni riga, tre volte più lento —, e i nascosti, pochi, che
    // l'indice parziale di `copia_di` dà da solo, si tolgono dopo.
    if origine.e_tutti() {
        let nascosti = id_dei_nascosti(connection)?;
        if !nascosti.is_empty() {
            let sql = format!("SELECT t.id FROM tracks t ORDER BY {}", order.sql());
            let mut ids = leggi(connection, &sql, &[], "identificativi dei brani")?;
            ids.retain(|id| !nascosti.contains(id));
            return Ok(ids);
        }
    }
    let sql = format!(
        "SELECT t.id FROM tracks t WHERE {} ORDER BY {}",
        dove(connection, origine, "t.")?,
        order.sql()
    );
    leggi(connection, &sql, &[], "identificativi dei brani")
}

/// Le righe che «Tutti» nasconde: dietro una copia migliore di un altro
/// dispositivo, o in grazia (`copia_di` valorizzato). Dall'indice parziale.
fn id_dei_nascosti(connection: &Connection) -> Result<std::collections::HashSet<i64>, AppError> {
    let mut statement = connection
        .prepare_cached("SELECT id FROM tracks WHERE copia_di IS NOT NULL")
        .map_err(|err| db_error("righe nascoste", &err))?;
    statement
        .query_map([], |riga| riga.get::<_, i64>(0))
        .map_err(|err| db_error("righe nascoste", &err))?
        .collect::<Result<_, _>>()
        .map_err(|err| db_error("righe nascoste", &err))
}

/// Gli identificativi dei preferiti, nell'ordine di [`super::list_liked`].
///
/// # Errori
///
/// `db.*` se la lettura fallisce.
pub fn id_dei_preferiti(connection: &Connection, origine: &Origine) -> Result<Vec<i64>, AppError> {
    let sql = format!("SELECT t.id {}", da_preferiti(connection, origine)?);
    leggi(connection, &sql, &[], "identificativi dei preferiti")
}

/// Gli identificativi di una ricerca, nell'ordine di [`super::search`].
///
/// # Errori
///
/// `db.*` se la lettura fallisce.
pub fn id_della_ricerca(
    connection: &Connection,
    origine: &Origine,
    query: &str,
) -> Result<Vec<i64>, AppError> {
    let espressione = fts_query(query);
    if espressione.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!("SELECT t.id {}", da_ricerca(connection, origine)?);
    leggi(
        connection,
        &sql,
        &[&espressione],
        "identificativi della ricerca",
    )
}

/// Una colonna di interi, riga per riga.
fn leggi(
    connection: &Connection,
    sql: &str,
    parametri: &[&dyn rusqlite::ToSql],
    cosa: &str,
) -> Result<Vec<i64>, AppError> {
    let mut statement = connection
        .prepare_cached(sql)
        .map_err(|err| db_error(cosa, &err))?;
    let righe = statement
        .query_map(parametri, |riga| riga.get::<_, i64>(0))
        .map_err(|err| db_error(cosa, &err))?;
    righe
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error(cosa, &err))
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::library::{list_liked, list_tracks, search};

    /// Una libreria vera, con brani che si ordinano in modo diverso per ogni
    /// criterio: artisti e titoli mescolati, date, ascolti e cuori sparsi.
    fn libreria() -> Connection {
        let c = crate::db::open_in_memory().expect("database").connection;
        let brani = [
            ("Zeta", "Bravo", "Uno", 1, 3, 30, 5, Some(400), 1, Some(10)),
            ("alfa", "Bravo", "Due", 1, 1, 10, 0, None, 0, None),
            ("Mare", "Charlie", "Tre", 2, 1, 20, 9, Some(900), 1, None),
            (
                "mare",
                "alpha",
                "Quattro",
                1,
                2,
                40,
                9,
                Some(800),
                1,
                Some(30),
            ),
            ("Echo", "Alpha", "Cinque", 1, 1, 50, 2, Some(100), 0, None),
            ("Delta", "Bravo", "Sei", 1, 2, 60, 0, None, 1, Some(20)),
        ];
        for (n, (titolo, artista, album, disco, traccia, data, ascolti, ultimo, cuore, quando)) in
            brani.into_iter().enumerate()
        {
            c.execute(
                "INSERT INTO tracks (path, track_key, title, artist, album, disc_number,
                                     track_number, duration_ms, date_added, date_modified,
                                     file_size, play_count, last_played_at, liked, liked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 200000, ?8, 1, 1000000, ?9, ?10, ?11, ?12)",
                rusqlite::params![
                    format!(r"C:\m\{n}.mp3"),
                    format!("{artista}|{titolo}|{n}"),
                    titolo,
                    artista,
                    album,
                    disco,
                    traccia,
                    data,
                    ascolti,
                    ultimo,
                    cuore,
                    quando
                ],
            )
            .expect("brano");
        }
        c
    }

    #[test]
    fn gli_identificativi_seguono_l_ordine_delle_pagine() {
        let c = libreria();
        for ordine in [
            TrackOrder::Shelf,
            TrackOrder::RecentlyAdded,
            TrackOrder::MostPlayed,
            TrackOrder::RecentlyPlayed,
            TrackOrder::Title,
        ] {
            let pagine: Vec<i64> = list_tracks(&c, &Origine::Tutti, ordine, 0, -1)
                .expect("pagine")
                .into_iter()
                .map(|b| b.id)
                .collect();
            assert_eq!(
                id_dei_brani(&c, &Origine::Tutti, ordine),
                Ok(pagine),
                "{ordine:?}"
            );
        }
        let preferiti: Vec<i64> = list_liked(&c, &Origine::Tutti, 0, -1)
            .expect("preferiti")
            .into_iter()
            .map(|b| b.id)
            .collect();
        assert_eq!(preferiti.len(), 4);
        assert_eq!(id_dei_preferiti(&c, &Origine::Tutti), Ok(preferiti));
        for cercato in ["mare", "bravo", "a", "nessuno"] {
            let trovati: Vec<i64> = search(&c, &Origine::Tutti, cercato, 0, -1)
                .expect("ricerca")
                .into_iter()
                .map(|b| b.id)
                .collect();
            assert_eq!(
                id_della_ricerca(&c, &Origine::Tutti, cercato),
                Ok(trovati),
                "{cercato}"
            );
        }
        assert_eq!(id_della_ricerca(&c, &Origine::Tutti, "   "), Ok(Vec::new()));
    }

    #[test]
    fn gli_identificativi_dei_brani_si_leggono_dall_indice() {
        // Il punto della funzione: l'ordine dello scaffale Ã¨ un indice, e con
        // il solo `t.id` la lettura non tocca la tabella. Se un giorno
        // l'indice cambia (una collazione nuova), la prova va riguardata con
        // lui: Ã¨ lei che dice se Â«Suona da quiÂ» Ã¨ tornato a leggere tutto.
        // Dalla 042 l'ordine per nome sta nelle colonne `ordine_*`, con un
        // indice per lo scaffale e uno per il titolo: tutti e due coprono.
        let c = libreria();
        for ordine in [TrackOrder::Shelf, TrackOrder::Title] {
            let sql = format!(
                "EXPLAIN QUERY PLAN SELECT t.id FROM tracks t ORDER BY {}",
                ordine.sql()
            );
            let mut s = c.prepare(&sql).expect("piano");
            let piano: Vec<String> = s
                .query_map([], |r| r.get::<_, String>(3))
                .expect("piano")
                .collect::<Result<_, _>>()
                .expect("righe del piano");
            assert!(
                piano.iter().any(|riga| riga.contains("COVERING INDEX")),
                "{ordine:?}: {piano:?}"
            );
        }
    }
}
