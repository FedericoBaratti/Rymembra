//! I comandi delle playlist.
//!
//! Involucri, come quelli di [`crate::comandi`]: ogni funzione traduce gli
//! argomenti, chiama `aether_app::playlists` e traduce l'errore. Nessuna
//! decisione — né su cosa sia un nome valido, né su cosa succeda a una playlist
//! automatica, né su come si riscrivano le posizioni. Sono tutte cose che
//! Android dovrà fare identiche, e una regola scritta qui sarebbe una regola da
//! riscrivere di là.
//!
//! Sta in un file suo e non in `comandi.rs` perché quello è già il più lungo, e
//! perché queste otto funzioni hanno un solo argomento in comune: la playlist.

use aether_app::library::TrackSummary;
use aether_app::playlists::{self, PlaylistSummary};

use crate::errore::{Esito, errore};
use crate::stato::{Stato, con_libreria};
use tauri::State;

/// Tutte le playlist, con i loro numeri.
#[tauri::command]
pub fn playlist_elenco(stato: State<'_, Stato>) -> Esito<Vec<PlaylistSummary>> {
    con_libreria(&stato, |libreria| playlists::list(&libreria.connection)).map_err(errore)
}

/// I brani di una playlist, nell'ordine in cui stanno.
#[tauri::command]
pub fn playlist_brani(stato: State<'_, Stato>, id: i64) -> Esito<Vec<TrackSummary>> {
    con_libreria(&stato, |libreria| {
        playlists::tracks(&libreria.connection, id)
    })
    .map_err(errore)
}

/// Crea una playlist vuota.
#[tauri::command]
pub fn playlist_crea(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    nome: String,
) -> Esito<PlaylistSummary> {
    let esito = con_libreria(&stato, |libreria| {
        playlists::create(&libreria.connection, &nome)
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Rinomina una playlist.
#[tauri::command]
pub fn playlist_rinomina(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    id: i64,
    nome: String,
) -> Esito<PlaylistSummary> {
    let esito = con_libreria(&stato, |libreria| {
        playlists::rename(&libreria.connection, id, &nome)
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Cancella una playlist.
#[tauri::command]
pub fn playlist_cancella(app: tauri::AppHandle, stato: State<'_, Stato>, id: i64) -> Esito<()> {
    let esito = con_libreria(&stato, |libreria| {
        playlists::delete(&mut libreria.connection, id)
    })
    .map_err(errore);
    // La lapide che `delete` lascia deve arrivare su Drive: senza, l'altro
    // dispositivo vedrebbe solo «a me manca una playlist» e la rimanderebbe
    // indietro alla prima passata.
    crate::nuvola::se_riuscito(&app, esito)
}

/// Aggiunge brani in fondo a una playlist.
#[tauri::command]
pub fn playlist_aggiungi(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    id: i64,
    brani: Vec<i64>,
) -> Esito<PlaylistSummary> {
    let esito = con_libreria(&stato, |libreria| {
        playlists::add_tracks(&mut libreria.connection, id, &brani)
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Toglie il brano che sta in una posizione.
#[tauri::command]
pub fn playlist_togli(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    id: i64,
    posizione: i64,
) -> Esito<PlaylistSummary> {
    let esito = con_libreria(&stato, |libreria| {
        playlists::remove_at(&mut libreria.connection, id, posizione)
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Sposta un brano dentro una playlist.
#[tauri::command]
pub fn playlist_riordina(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    id: i64,
    da: i64,
    a: i64,
) -> Esito<PlaylistSummary> {
    let esito = con_libreria(&stato, |libreria| {
        playlists::reorder(&mut libreria.connection, id, da, a)
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}
