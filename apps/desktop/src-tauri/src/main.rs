//! L'applicazione desktop.
//!
//! Una finestra sopra `aether-app`, e nient'altro. Tutto quel che decide sta nel
//! nucleo; qui c'è l'apertura della finestra, il registro dei comandi e il
//! protocollo che serve le copertine.

// Senza, in rilascio si aprirebbe una finestra di console dietro l'applicazione.
// In sviluppo la si vuole: è dove finiscono i messaggi di avvio.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod comandi;
mod copertine;
mod errore;
mod stato;

use tauri::Manager as _;

fn main() {
    let esito = tauri::Builder::default()
        // Scegliere la cartella della musica è la prima cosa che fa chi apre
        // Aether: un campo di testo in cui incollare un percorso funziona, e
        // sbaglia al primo spazio o alla prima barra rovesciata.
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let stato = stato::Stato::apri(data_dir);
            // La riga di avvio va stampata **prima** di disegnare: se
            // l'apertura del database è fallita, è l'unica traccia che resta
            // quando la finestra mostra solo l'errore.
            stato::riga_di_avvio(&stato);
            app.manage(stato);
            Ok(())
        })
        // La variante asincrona: consegna un `responder` invece di pretendere
        // la risposta subito. Serve perché la lettura dal disco possa avvenire
        // fuori dal thread che riceve la richiesta — una griglia che scorre ne
        // fa decine al secondo, e farle in fila le farebbe apparire a scatti.
        .register_asynchronous_uri_scheme_protocol("aether-cover", copertine::servi)
        .invoke_handler(tauri::generate_handler![
            comandi::avvio,
            comandi::imposta_cartelle,
            comandi::scansiona,
            comandi::cerca,
            comandi::brani,
            comandi::album,
            comandi::brani_album,
            comandi::preferito,
            comandi::piano_importazione,
            comandi::importa,
        ])
        .run(tauri::generate_context!());

    // `run` restituisce un `Result` e il modo normale di trattarlo negli esempi
    // di Tauri è `.expect(…)`. Qui i panici sono vietati per un motivo che vale
    // anche all'avvio: un messaggio di panico è un messaggio per chi ha scritto
    // il codice, e chi apre l'applicazione merita di leggere cos'è andato
    // storto invece di una traccia di stack.
    if let Err(err) = esito {
        eprintln!("Aether non è riuscito ad avviarsi: {err}");
        std::process::exit(1);
    }
}
