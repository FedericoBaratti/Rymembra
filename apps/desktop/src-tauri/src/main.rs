//! L'applicazione desktop.
//!
//! Una finestra sopra `aether-app`, e nient'altro. Tutto quel che decide sta nel
//! nucleo; qui c'è l'apertura della finestra, il registro dei comandi e il
//! protocollo che serve le copertine.

// Senza, in rilascio si aprirebbe una finestra di console dietro l'applicazione.
// In sviluppo la si vuole: è dove finiscono i messaggi di avvio.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod account;
mod arricchimento;
mod comandi;
mod copertine;
mod errore;
mod nuvola;
mod playlist;
mod riordino;
mod riproduzione;
mod scarica;
mod scrobble;
mod skin;
mod spotify;
mod stato;
mod studio;

use tauri::Manager as _;

/// Quanto si aspetta la finestra prima di mostrarla comunque.
///
/// La finestra nasce **nascosta** (`visible: false` in `tauri.conf.json`) e la
/// mostra il frontend con [`pronto`], dopo aver applicato skin e tema: è così
/// che chi sceglie una skin chiara smette di vedere un fotogramma scuro a ogni
/// avvio — il fondo che il sistema operativo dipinge prima che esista una
/// pagina non può venire dall'IPC, perché l'IPC non risponde ancora.
///
/// Questo però mette il primo disegno nelle mani del frontend, e un frontend
/// che non arriva mai a chiamare `pronto` — uno script che cade, una skin
/// illeggibile — lascerebbe un processo vivo e **nessuna finestra**. Due
/// secondi dopo la si mostra lo stesso: un fotogramma del colore sbagliato è un
/// difetto, un'applicazione invisibile è un guasto.
const ATTESA_PRIMO_COLORE: std::time::Duration = std::time::Duration::from_secs(2);

/// Il frontend ha applicato skin e tema: si può guardare.
///
/// Idempotente per costruzione — `show()` su una finestra già visibile non fa
/// niente — e per questo la rete di sicurezza qui sopra può chiamarlo senza
/// coordinarsi con nessuno.
#[tauri::command]
fn pronto(finestra: tauri::Window) {
    let _ = finestra.show();
}

/// Mostra la finestra fra due secondi, qualunque cosa succeda di là.
fn rete_di_sicurezza(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(ATTESA_PRIMO_COLORE);
        if let Some(finestra) = app.get_webview_window("main") {
            let _ = finestra.show();
        }
    });
}

fn main() {
    let esito = tauri::Builder::default()
        // Scegliere la cartella della musica è la prima cosa che fa chi apre
        // Aether: un campo di testo in cui incollare un percorso funziona, e
        // sbaglia al primo spazio o alla prima barra rovesciata.
        .plugin(tauri_plugin_dialog::init())
        // La schermata di consenso di Google si apre nel browser di **sistema**.
        // Mai nella webview: una finestra dell'applicazione che sa disegnare
        // `accounts.google.com` è una superficie di phishing, e il CSP di
        // `tauri.conf.json` resta identico.
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = cartella_dati(app.handle())?;
            // Prima della libreria, perché non ne ha bisogno: tiene solo il
            // percorso di `spotify.json` e una cella vuota. Il lettore vero
            // nasce alla prima importazione, così chi non importa mai da
            // Spotify non paga nemmeno una stretta di mano.
            app.manage(spotify::StatoSpotify::nuovo(&data_dir));
            // Come sopra, e per la stessa ragione: due celle vuote e un bit.
            // Il consenso, la rete e lo zip arrivano solo se qualcuno apre la
            // schermata dell'account.
            app.manage(account::StatoAccount::nuovo());
            // Anche questo prima della libreria, e per la stessa ragione: tiene
            // solo il percorso dei binari e tre bit. Il binario vero si cerca
            // quando parte una coda, non adesso.
            app.manage(scarica::StatoScarico::nuovo(cartella_binari(app.handle())));

            let stato = stato::Stato::apri(data_dir);
            // La riga di avvio va stampata **prima** di disegnare: se
            // l'apertura del database è fallita, è l'unica traccia che resta
            // quando la finestra mostra solo l'errore.
            stato::riga_di_avvio(&stato);
            app.manage(stato);

            // Il lettore **dopo** la libreria: apre il dispositivo audio e
            // rilegge la coda di ieri, e per la seconda cosa il database deve
            // già essere aperto.
            let lettore = riproduzione::StatoLettore::avvia(app.handle());
            riproduzione::riga_di_avvio_lettore(&lettore);
            app.manage(lettore);
            riproduzione::riprendi_coda(app.handle());
            riproduzione::avvia_orologio(app.handle().clone());
            riproduzione::avvia_spettro(app.handle().clone());

            // Il backup **per ultimo**: il suo filo aspetta mezzo minuto prima
            // della prima passata proprio per lasciar finire quel che parte
            // adesso, e non ha senso averlo in piedi prima che ci sia una
            // libreria da salvare.
            let (nuvola, orecchio) = nuvola::StatoNuvola::nuovo();
            app.manage(nuvola);
            nuvola::avvia_filo(app.handle().clone(), orecchio);

            // L'arricchimento **dopo il backup**, ed è l'ordine giusto anche se
            // i due fili non si aspettano a vicenda: il suo aspetta un minuto e
            // mezzo prima della prima passata — il triplo della nuvola — perché
            // arricchire mentre una scansione riscrive le righe vuol dire
            // decidere su dati che stanno per cambiare.
            let (arricchimento, orecchio) = arricchimento::StatoArricchimento::nuovo();
            app.manage(arricchimento);
            arricchimento::avvia_filo(app.handle().clone(), orecchio);

            // Lo scrobbling **dopo** la libreria, perché la prima cosa che fa è
            // guardare se è rimasto qualcosa in coda dalla sessione precedente.
            // Il suo filo dorme finché non lo si chiama: a servizi scollegati
            // non costa niente.
            app.manage(scrobble::StatoScrobble::nuovo());
            scrobble::avvia(app.handle());

            // Per ultima, e dopo tutto il resto: quel che conta è che parta a
            // finestra già costruita, non prima di aprire il database.
            rete_di_sicurezza(app.handle());
            Ok(())
        })
        // La variante asincrona: consegna un `responder` invece di pretendere
        // la risposta subito. Serve perché la lettura dal disco possa avvenire
        // fuori dal thread che riceve la richiesta — una griglia che scorre ne
        // fa decine al secondo, e farle in fila le farebbe apparire a scatti.
        .register_asynchronous_uri_scheme_protocol("aether-cover", copertine::servi)
        .invoke_handler(tauri::generate_handler![
            pronto,
            comandi::avvio,
            comandi::imposta_cartelle,
            comandi::imposta_cartella_download,
            comandi::cronologia,
            comandi::cronologia_conteggio,
            comandi::scansiona,
            comandi::annulla_scansione,
            comandi::cerca,
            comandi::cerca_conteggio,
            comandi::brani,
            comandi::album,
            comandi::album_artista,
            comandi::artisti,
            comandi::brani_album,
            comandi::preferiti,
            comandi::preferito,
            comandi::valutazione,
            comandi::piano_importazione,
            comandi::importa,
            comandi::imposta_tema,
            comandi::imposta_scorciatoie,
            comandi::profilo_esporta,
            comandi::profilo_piano,
            comandi::profilo_importa,
            spotify::spotify_anteprima,
            spotify::spotify_piano,
            spotify::spotify_importa,
            spotify::spotify_diagnostica,
            account::account_stato,
            account::account_credenziali,
            account::account_collega,
            account::account_scollega,
            account::account_leggi,
            account::account_piano,
            account::account_importa,
            account::archivio_apri,
            account::cronologia_dimentica_importati,
            scarica::scarica_desiderati,
            scarica::annulla_scarico,
            scarica::scarico_stato,
            scarica::riprova_falliti,
            riordino::piano_riordino,
            riordino::esegui_riordino,
            riordino::annulla_riordino,
            playlist::playlist_elenco,
            playlist::playlist_brani,
            playlist::playlist_crea,
            playlist::playlist_rinomina,
            playlist::playlist_cancella,
            playlist::playlist_aggiungi,
            playlist::playlist_togli,
            playlist::playlist_riordina,
            playlist::playlist_crea_smart,
            playlist::playlist_regole,
            playlist::playlist_regole_scrivi,
            playlist::playlist_regole_prova,
            playlist::playlist_file_piano,
            playlist::playlist_file_importa,
            playlist::playlist_esporta,
            skin::skin,
            skin::skin_elenco,
            skin::skin_installa,
            skin::skin_installa_sorgente,
            skin::skin_scegli,
            skin::accento_dinamico,
            skin::accento_dinamico_attiva,
            skin::accento_copertina,
            studio::studio_registro,
            studio::studio_valida,
            studio::studio_documento,
            studio::studio_pacchetto,
            studio::studio_salva,
            studio::studio_esporta,
            studio::studio_istantanee,
            studio::studio_istantanea,
            studio::studio_ripristina,
            riproduzione::suona,
            riproduzione::pausa,
            riproduzione::riprendi,
            riproduzione::alterna,
            riproduzione::prossimo,
            riproduzione::precedente,
            riproduzione::vai_a,
            riproduzione::volume,
            riproduzione::equalizzatore,
            riproduzione::normalizzazione,
            riproduzione::riapri_audio,
            riproduzione::spettro,
            riproduzione::eq_preset_elenco,
            riproduzione::eq_preset_salva,
            riproduzione::eq_preset_cancella,
            riproduzione::riproduzione_stato,
            riproduzione::coda_accoda,
            riproduzione::coda_dopo,
            riproduzione::coda_vai,
            riproduzione::coda_togli,
            riproduzione::coda_riordina,
            riproduzione::coda_svuota,
            riproduzione::ripeti,
            riproduzione::mescola,
            riproduzione::brani_per_id,
            nuvola::nuvola_stato,
            nuvola::nuvola_collega,
            nuvola::nuvola_scollega,
            nuvola::nuvola_credenziali,
            nuvola::nuvola_attiva,
            nuvola::nuvola_salva,
            nuvola::nuvola_piano_ripristino,
            nuvola::nuvola_ripristina,
            arricchimento::arricchimento_stato,
            arricchimento::arricchimento_attiva,
            arricchimento::arricchimento_annulla,
            scrobble::scrobble_stato,
            scrobble::scrobble_attivo,
            scrobble::scrobble_listenbrainz_collega,
            scrobble::scrobble_listenbrainz_scollega,
            scrobble::scrobble_lastfm_credenziali,
            scrobble::scrobble_lastfm_collega,
            scrobble::scrobble_lastfm_completa,
            scrobble::scrobble_lastfm_scollega,
            scrobble::scrobble_invia,
            scrobble::scrobble_riprova,
            scrobble::scrobble_dimentica,
            scrobble::scrobble_importa_cronologia,
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

/// Dove stanno database, copertine e skin.
///
/// Normalmente la cartella dati dell'applicazione, che su Windows è
/// `%APPDATA%\dev.aether.desktop`. `AETHER_DATI` la sostituisce.
///
/// # Perché una scorciatoia del genere esiste
///
/// Perché quasi tutto ciò che questa applicazione fa di irreversibile —
/// riordinare i file sul disco, importare un account intero, ripristinare un
/// backup — si può leggere in una prova unitaria e si può giudicare **solo**
/// guardandolo succedere in una finestra vera, su una libreria che somiglia a
/// quella di qualcuno. Senza questa variabile le due cose sono la stessa
/// libreria: la sola, quella dell'utente. Provare un'importazione vorrebbe dire
/// scrivere sulla sua cronologia d'ascolto, che è — con voti e preferiti —
/// l'unica cosa in tutto il programma che una scansione non sa ricostruire.
///
/// Con questa, si copia il database in una cartella qualunque, si lancia
/// `AETHER_DATI=... aether.exe` e si rompe pure tutto: la libreria vera non sa
/// nemmeno che è successo.
///
/// La variabile si legge **solo qui**, all'avvio, e da nessun'altra parte:
/// quello che il resto del programma vede è un `PathBuf` e basta, come prima.
fn cartella_dati(app: &tauri::AppHandle) -> Result<std::path::PathBuf, tauri::Error> {
    if let Some(scelta) = std::env::var_os("AETHER_DATI").filter(|v| !v.is_empty()) {
        return Ok(std::path::PathBuf::from(scelta));
    }
    app.path().app_data_dir()
}

/// Dove stanno i binari esterni — oggi solo yt-dlp.
///
/// In rilascio è `bin/` dentro le risorse impacchettate, dichiarata in
/// `tauri.conf.json` sotto `bundle.resources`. In sviluppo è la cartella del
/// repo, che non è impacchettata: senza questo secondo ramo, provare uno
/// scaricamento richiederebbe una build di rilascio da sei minuti.
///
/// Modellato su `binDir()` di `legacy/Aeter/electron/modules/binaries.ts`, che
/// faceva esattamente la stessa distinzione.
fn cartella_binari(app: &tauri::AppHandle) -> std::path::PathBuf {
    if let Ok(risorse) = app
        .path()
        .resolve("bin", tauri::path::BaseDirectory::Resource)
        && risorse.is_dir()
    {
        return risorse;
    }
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/bin")
}
