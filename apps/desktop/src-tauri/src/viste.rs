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
//! Non c'è una seconda query scritta qui: le domande stanno in
//! `rymembra_app::library` accanto a quelle delle pagine (`id_dei_brani`,
//! `id_dei_preferiti`, `id_della_ricerca`), con la **stessa** clausola d'ordine
//! di `list_tracks`, `list_liked` e `search`. Un ordine scritto due volte è un
//! ordine che prima o poi diverge, e «Seleziona tutto» seguito da «Riproduci»
//! suonerebbe un ordine diverso da quello che si vede: le prove qui sotto lo
//! confrontano con le pagine.
//!
//! Si legge il solo identificativo. Prima si chiedevano le pagine senza fondo e
//! si buttava tutto il resto: diciassette colonne di ogni riga, col lucchetto
//! della libreria in mano, fra il doppio clic e il suono (R21-6).

use rymembra_app::library::{
    OrdineBraniArtista, Partecipazione, TrackOrder, id_dei_brani, id_dei_brani_artista,
    id_dei_preferiti, id_della_ricerca, id_delle_partecipazioni,
};
use rymembra_app::specchio::Origine;
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
    /// I brani di un artista: la sezione «Brani» della sua pagina.
    Artista {
        /// L'artista effettivo, come lo dà la griglia degli artisti.
        nome: String,
        /// Il nome dell'ordinamento: vedi [`ordine_artista_da`].
        ordine: String,
    },
    /// Le partecipazioni di un artista: «Compare anche in».
    Partecipazioni {
        /// L'artista effettivo, come lo dà la griglia degli artisti.
        nome: String,
        /// `esatte` o `euristiche`: le due specie non si confondono mai.
        specie: String,
    },
}

/// Il nome di un ordinamento dei brani di un artista, in un valore chiuso.
///
/// I quattro nomi di `OrdineArtista` in `ipc/libreria.ts`. Un nome sconosciuto
/// vale «album», come per `ordine_da`: l'insieme è chiuso e lo scrive solo
/// questa applicazione.
pub(crate) fn ordine_artista_da(nome: &str) -> OrdineBraniArtista {
    match nome {
        "anno" => OrdineBraniArtista::Anno,
        "titolo" => OrdineBraniArtista::Titolo,
        "ascoltati" => OrdineBraniArtista::PiuAscoltati,
        _ => OrdineBraniArtista::Album,
    }
}

/// La specie di partecipazione di un nome della finestra: `esatte` o `euristiche`.
///
/// Un altro valore è un errore di chi chiama e non un ripiego: tradurre un
/// refuso in «esatte» direbbe fatti di quel che è una ricerca. Ci passano anche i
/// comandi `partecipazioni` e `partecipazioni_conta`.
pub(crate) fn specie_di_partecipazione(nome: &str) -> Result<Partecipazione, AppError> {
    match nome {
        "esatte" => Ok(Partecipazione::Esatta),
        "euristiche" => Ok(Partecipazione::Euristica),
        altro => Err(AppError::new(ErrorCode::IpcPayloadInvalid {
            channel: "partecipazioni".to_owned(),
            detail: Some(format!("la specie «{altro}» non esiste")),
        })),
    }
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
/// # Errori
///
/// `db.*` dalla lettura.
fn identificativi(
    connection: &rusqlite::Connection,
    origine: &Origine,
    vista: &VistaBrani,
) -> Result<Vec<i64>, AppError> {
    match vista {
        VistaBrani::Brani { ordine } => id_dei_brani(connection, origine, ordine_da(ordine)),
        VistaBrani::Preferiti => id_dei_preferiti(connection, origine),
        VistaBrani::Ricerca { query } => id_della_ricerca(connection, origine, query),
        VistaBrani::Artista { nome, ordine } => {
            id_dei_brani_artista(connection, origine, nome, ordine_artista_da(ordine))
        }
        VistaBrani::Partecipazioni { nome, specie } => {
            id_delle_partecipazioni(connection, origine, nome, specie_di_partecipazione(specie)?)
        }
    }
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
pub async fn vista_brani_id(
    app: tauri::AppHandle,
    vista: VistaBrani,
    origine: Option<Origine>,
) -> Esito<Vec<i64>> {
    let origine = origine.unwrap_or_default();
    in_disparte("identificativi di una vista", move || {
        let stato = app.try_state::<Stato>().ok_or_else(|| {
            AppError::new(ErrorCode::InternalAborted {
                what: Some("identificativi di una vista".to_owned()),
            })
            .with_cause("la libreria non è più fra gli stati gestiti")
        })?;
        con_libreria(&stato, |libreria| {
            identificativi(&libreria.connection, &origine, &vista)
        })
    })
    .await
    .map_err(errore)?
    .map_err(errore)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rymembra_app::library::{TrackSummary, brani_artista, list_liked, list_tracks, search};

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

    /// Una libreria con un artista che ha dischi, raccolte e brani senza disco.
    fn libreria_con_un_artista() -> rusqlite::Connection {
        let connection = rymembra_app::db::open_in_memory()
            .expect("database")
            .connection;
        for (i, (titolo, artista, artista_album, album_key, anno, ascolti)) in [
            ("Zeta", "Tale", Some("Tale"), Some("primo"), Some(2001), 5),
            ("Beta", "Tale", Some("Tale"), Some("primo"), Some(2001), 9),
            ("Singolo", "Tale", None, None, None, 20),
            (
                "Raccolta",
                "Tale",
                Some("Vari"),
                Some("raccolta"),
                Some(2005),
                1,
            ),
            (
                "Altrui",
                "Altro",
                Some("Vari"),
                Some("raccolta"),
                Some(2005),
                0,
            ),
            ("Mista", "Tale", None, Some("mista"), Some(2003), 2),
        ]
        .into_iter()
        .enumerate()
        {
            connection
                .execute(
                    "INSERT INTO tracks
                         (path, track_key, title, artist, album, album_artist, album_key, year,
                          play_count, duration_ms, file_size, date_added, date_modified)
                     VALUES (?1, ?2, ?3, ?4, 'Disco', ?5, ?6, ?7, ?8, 1000, 1, 1, 0)",
                    rusqlite::params![
                        format!(r"C:\M\{i}.mp3"),
                        format!("k{i}"),
                        titolo,
                        artista,
                        artista_album,
                        album_key,
                        anno,
                        ascolti
                    ],
                )
                .expect("riga di brano");
        }
        let mut connection = connection;
        let tx = connection.transaction().expect("transazione");
        rymembra_app::library::rebuild_aggregates(&tx).expect("aggregati");
        tx.commit().expect("commit");
        connection
    }

    #[test]
    fn la_vista_di_un_artista_ha_l_ordine_delle_pagine_e_solo_i_suoi_brani() {
        let connection = libreria_con_un_artista();
        for ordine in ["album", "anno", "titolo", "ascoltati"] {
            let vista = VistaBrani::Artista {
                nome: "Tale".to_owned(),
                ordine: ordine.to_owned(),
            };
            let tutti =
                identificativi(&connection, &Origine::Tutti, &vista).expect("identificativi");
            let pagine = a_pagine(|o, l| {
                brani_artista(
                    &connection,
                    &Origine::Tutti,
                    "Tale",
                    ordine_artista_da(ordine),
                    o,
                    l,
                )
            });
            // I suoi: i due del disco, quello senza disco, quello della raccolta
            // senza artista del disco... e non quelli di «Vari» né di «Altro».
            assert_eq!(tutti.len(), 4, "{ordine}");
            assert_eq!(tutti, pagine, "{ordine}: lo stesso ordine delle pagine");
        }
    }

    #[test]
    fn la_vista_di_un_artista_arriva_dalla_finestra_nella_forma_di_ipc_ts() {
        let vista: VistaBrani =
            serde_json::from_str(r#"{"tipo":"artista","nome":"Tale","ordine":"anno"}"#)
                .expect("artista");
        assert_eq!(
            vista,
            VistaBrani::Artista {
                nome: "Tale".to_owned(),
                ordine: "anno".to_owned()
            }
        );
        let compare: VistaBrani =
            serde_json::from_str(r#"{"tipo":"partecipazioni","nome":"Tale","specie":"esatte"}"#)
                .expect("partecipazioni");
        assert_eq!(
            compare,
            VistaBrani::Partecipazioni {
                nome: "Tale".to_owned(),
                specie: "esatte".to_owned()
            }
        );
        assert_eq!(
            specie_di_partecipazione("esatte").expect("esatte"),
            Partecipazione::Esatta
        );
        assert_eq!(
            specie_di_partecipazione("euristiche").expect("euristiche"),
            Partecipazione::Euristica
        );
        // Un refuso non diventa «esatte»: le due specie non si confondono.
        assert!(specie_di_partecipazione("esatta").is_err());
        // I quattro nomi di `OrdineArtista` in `ipc/libreria.ts`, e il ripiego.
        assert_eq!(ordine_artista_da("album"), OrdineBraniArtista::Album);
        assert_eq!(ordine_artista_da("anno"), OrdineBraniArtista::Anno);
        assert_eq!(ordine_artista_da("titolo"), OrdineBraniArtista::Titolo);
        assert_eq!(
            ordine_artista_da("ascoltati"),
            OrdineBraniArtista::PiuAscoltati
        );
        assert_eq!(ordine_artista_da("boh"), OrdineBraniArtista::Album);
    }

    #[test]
    fn la_vista_intera_ha_l_ordine_delle_pagine() {
        let connection = libreria(40);
        for ordine in ["scaffale", "recenti", "ascoltati", "titolo"] {
            let vista = VistaBrani::Brani {
                ordine: ordine.to_owned(),
            };
            let tutti =
                identificativi(&connection, &Origine::Tutti, &vista).expect("identificativi");
            let pagine =
                a_pagine(|o, l| list_tracks(&connection, &Origine::Tutti, ordine_da(ordine), o, l));
            assert_eq!(tutti.len(), 40, "{ordine}");
            assert_eq!(tutti, pagine, "{ordine}: lo stesso ordine delle pagine");
        }
        let preferiti = identificativi(&connection, &Origine::Tutti, &VistaBrani::Preferiti)
            .expect("preferiti");
        assert_eq!(
            preferiti,
            a_pagine(|o, l| list_liked(&connection, &Origine::Tutti, o, l))
        );
        assert_eq!(preferiti.len(), 14, "uno su tre, da zero a trentanove");
    }

    #[test]
    fn la_ricerca_intera_e_quella_delle_pagine() {
        let connection = libreria(30);
        let vista = VistaBrani::Ricerca {
            query: "artista 3".to_owned(),
        };
        let mut tutti = identificativi(&connection, &Origine::Tutti, &vista).expect("ricerca");
        let mut pagine = a_pagine(|o, l| search(&connection, &Origine::Tutti, "artista 3", o, l));
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
            identificativi(&connection, &Origine::Tutti, &vuota)
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
