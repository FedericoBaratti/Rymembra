//! Gli identificativi di una vista intera.
//!
//! # Perché un comando a parte
//!
//! La finestra tiene in mano solo le pagine che si guardano (`rada.ts`): su
//! cinquantamila brani, qualche centinaio. «Seleziona tutto» prendeva le righe
//! in mano, cioè diceva «200 brani» sopra una vista che ne aveva cinquantamila,
//! e un clic su un brano faceva partire la coda delle sole righe arrivate.
//! Chiedere tutte le pagine per avere tutti i numeri vorrebbe dire
//! duecentocinquanta richieste di righe intere per buttarne via tutto tranne
//! l'identificativo.
//!
//! Qui la vista si descrive — quale elenco, con quale ordinamento o quale
//! ricerca — e tornano gli identificativi, tutti e nello stesso ordine delle
//! pagine.
//!
//! # Lo stesso ordine delle pagine, per costruzione
//!
//! Non c'è una seconda query scritta qui: si chiamano le stesse funzioni di
//! `rymembra_app::library` che servono le pagine (`list_tracks`, `list_liked`,
//! `search`), senza limite. Un ordine scritto due volte è un ordine che prima o
//! poi diverge, e «Seleziona tutto» seguito da «Riproduci» suonerebbe un ordine
//! diverso da quello che si vede.
//!
//! Il prezzo è che si leggono le righe intere per tenerne il numero: su
//! cinquantamila brani qualche decina di millisecondi, fuori dal filo della
//! finestra. È il gesto di un clic, non di uno scorrimento.

use rymembra_app::library::{TrackOrder, TrackSummary, list_liked, list_tracks, search};
use rymembra_domain::errors::{AppError, ErrorCode};
use serde::Deserialize;
use tauri::Manager as _;

use crate::disparte::in_disparte;
use crate::errore::{Esito, errore};
use crate::stato::{Stato, con_libreria};

/// Quale elenco di brani si sta guardando: `VistaBrani` in `ipc/libreria.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "tipo", rename_all = "camelCase")]
pub enum VistaBrani {
    /// La vista Brani, con il suo ordinamento.
    Brani {
        /// Il nome dell'ordinamento, come lo manda la finestra.
        ordine: String,
    },
    /// I preferiti.
    Preferiti,
    /// Una ricerca.
    Ricerca {
        /// Quel che si cerca, come lo scrive chi cerca.
        query: String,
    },
}

/// Il nome di un ordinamento, in un valore chiuso.
///
/// Gli stessi nomi di `comandi::brani`, e lo stesso ripiego: un nome
/// sconosciuto vale «scaffale». Qui non lascia traccia nel diario perché la
/// traccia l'ha già lasciata la pagina, chiesta prima con lo stesso nome.
fn ordine_da(nome: &str) -> TrackOrder {
    match nome {
        "recenti" => TrackOrder::RecentlyAdded,
        "ascoltati" => TrackOrder::MostPlayed,
        "titolo" => TrackOrder::Title,
        _ => TrackOrder::Shelf,
    }
}

/// Gli identificativi della vista, nell'ordine delle sue pagine.
///
/// `-1` come limite è il «nessun limite» di SQLite: le funzioni sono quelle
/// delle pagine, e una pagina senza fondo è la vista intera.
///
/// # Errori
///
/// `db.*` dalla lettura.
fn identificativi(
    connection: &rusqlite::Connection,
    vista: &VistaBrani,
) -> Result<Vec<i64>, AppError> {
    let righe: Vec<TrackSummary> = match vista {
        VistaBrani::Brani { ordine } => list_tracks(connection, ordine_da(ordine), 0, -1)?,
        VistaBrani::Preferiti => list_liked(connection, 0, -1)?,
        VistaBrani::Ricerca { query } => search(connection, query, 0, -1)?,
    };
    Ok(righe.into_iter().map(|brano| brano.id).collect())
}

/// Gli identificativi di tutta la vista, per «Seleziona tutto» e per suonare
/// da una lista rada.
///
/// Fuori dal filo della finestra: su una libreria grande è una lettura di tutte
/// le righe, e il filo della finestra è quello che la ridisegna.
///
/// # Errori
///
/// `internal.aborted` se la libreria non c'è più; `db.*` dalla lettura.
#[tauri::command]
pub async fn vista_brani_id(app: tauri::AppHandle, vista: VistaBrani) -> Esito<Vec<i64>> {
    in_disparte("identificativi di una vista", move || {
        let stato = app.try_state::<Stato>().ok_or_else(|| {
            AppError::new(ErrorCode::InternalAborted {
                what: Some("identificativi di una vista".to_owned()),
            })
            .with_cause("la libreria non è più fra gli stati gestiti")
        })?;
        con_libreria(&stato, |libreria| {
            identificativi(&libreria.connection, &vista)
        })
    })
    .await
    .map_err(errore)?
    .map_err(errore)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Una libreria in memoria con `n` brani, uno su tre fra i preferiti.
    fn libreria(n: i64) -> rusqlite::Connection {
        let connection = rymembra_app::db::open_in_memory()
            .expect("database")
            .connection;
        for i in 0..n {
            // Titoli e date mescolati, perché i quattro ordinamenti diano
            // quattro ordini diversi: con un ordine solo il confronto con le
            // pagine non proverebbe niente.
            let titolo = format!("Brano {:03}", (i * 37) % n);
            connection
                .execute(
                    "INSERT INTO tracks
                         (path, track_key, title, artist, album, duration_ms,
                          file_size, date_added, date_modified, liked, liked_at,
                          play_count)
                     VALUES (?1, ?2, ?3, ?4, 'Disco', 1000, 99999, ?5, 0, ?6, ?7, ?8)",
                    rusqlite::params![
                        format!(r"C:\M\{i}.mp3"),
                        format!("k{i}"),
                        titolo,
                        format!("Artista {}", i % 7),
                        (i * 13) % n,
                        i64::from(i % 3 == 0),
                        if i % 3 == 0 { Some(i * 11 % n) } else { None },
                        (i * 5) % 9,
                    ],
                )
                .expect("riga di brano");
        }
        connection
    }

    /// Gli identificativi chiesti pagina per pagina, come fa la finestra.
    fn a_pagine(pagina: impl Fn(i64, i64) -> Result<Vec<TrackSummary>, AppError>) -> Vec<i64> {
        let mut tutti = Vec::new();
        let mut offset = 0;
        loop {
            let righe = pagina(offset, 7).expect("pagina");
            if righe.is_empty() {
                return tutti;
            }
            offset += 7;
            tutti.extend(righe.into_iter().map(|b| b.id));
        }
    }

    #[test]
    fn la_vista_intera_ha_l_ordine_delle_pagine() {
        let connection = libreria(40);
        for ordine in ["scaffale", "recenti", "ascoltati", "titolo"] {
            let vista = VistaBrani::Brani {
                ordine: ordine.to_owned(),
            };
            let tutti = identificativi(&connection, &vista).expect("identificativi");
            let pagine = a_pagine(|o, l| list_tracks(&connection, ordine_da(ordine), o, l));
            assert_eq!(tutti.len(), 40, "{ordine}");
            assert_eq!(tutti, pagine, "{ordine}: lo stesso ordine delle pagine");
        }
        let preferiti = identificativi(&connection, &VistaBrani::Preferiti).expect("preferiti");
        assert_eq!(preferiti, a_pagine(|o, l| list_liked(&connection, o, l)));
        assert_eq!(preferiti.len(), 14, "uno su tre, da zero a trentanove");
    }

    #[test]
    fn la_ricerca_intera_e_quella_delle_pagine() {
        let connection = libreria(30);
        let vista = VistaBrani::Ricerca {
            query: "artista 3".to_owned(),
        };
        let mut tutti = identificativi(&connection, &vista).expect("ricerca");
        let mut pagine = a_pagine(|o, l| search(&connection, "artista 3", o, l));
        assert!(!tutti.is_empty());
        // Come insiemi: `bm25` dà lo stesso punteggio a righe che si somigliano,
        // e fra due pari SQLite non promette un ordine. Chi suona dalla vista
        // intera ritrova il brano premuto per identificativo, non per posto.
        tutti.sort_unstable();
        pagine.sort_unstable();
        assert_eq!(tutti, pagine);
        // Una ricerca vuota non trova niente, come la sua pagina.
        let vuota = VistaBrani::Ricerca {
            query: "   ".to_owned(),
        };
        assert!(
            identificativi(&connection, &vuota)
                .expect("vuota")
                .is_empty()
        );
    }

    #[test]
    fn la_vista_arriva_dalla_finestra_nella_forma_di_ipc_ts() {
        let brani: VistaBrani =
            serde_json::from_str(r#"{"tipo":"brani","ordine":"titolo"}"#).expect("brani");
        assert_eq!(
            brani,
            VistaBrani::Brani {
                ordine: "titolo".to_owned()
            }
        );
        let preferiti: VistaBrani =
            serde_json::from_str(r#"{"tipo":"preferiti"}"#).expect("preferiti");
        assert_eq!(preferiti, VistaBrani::Preferiti);
        let ricerca: VistaBrani =
            serde_json::from_str(r#"{"tipo":"ricerca","query":"x"}"#).expect("ricerca");
        assert_eq!(
            ricerca,
            VistaBrani::Ricerca {
                query: "x".to_owned()
            }
        );
        // Un ordinamento sconosciuto vale «scaffale», come per le pagine.
        assert_eq!(ordine_da("boh"), TrackOrder::Shelf);
    }
}
