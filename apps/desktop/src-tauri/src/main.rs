//! L'applicazione desktop.
//!
//! Una finestra sopra `aether-app`, e nient'altro. Tutto quel che decide sta nel
//! nucleo; qui c'è l'apertura della finestra, il registro dei comandi e il
//! protocollo che serve le copertine.

// Senza, in rilascio si aprirebbe una finestra di console dietro l'applicazione.
// In sviluppo la si vuole: è dove finiscono i messaggi di avvio.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod account;
mod aggiornamenti;
mod analisi;
mod arricchimento;
mod comandi;
mod copertine;
mod diario;
mod disparte;
mod errore;
mod ia;
mod importa;
mod media;
mod metadati;
mod nuvola;
mod playlist;
mod procura;
mod riordino;
mod riproduzione;
mod scrobble;
mod sincronia;
mod skin;
mod spegnimento;
mod stato;
mod studio;
mod testi;
mod vassoio;

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

/// # La barra del titolo la disegna la pagina
///
/// La finestra nasce senza decorazioni (`"decorations": false` in
/// `tauri.conf.json`), e i quattro gesti che il sistema operativo offriva con la
/// sua fascia grigia — trascinare, ridurre, ingrandire, chiudere — arrivano da
/// qui. Sono comandi nostri e non i permessi `core:window:*` del plugin per una
/// ragione sola: `capabilities/default.json` è un elenco chiuso, e aprirlo a
/// `core:window` darebbe alla pagina anche `set_position`, `set_size`,
/// `set_always_on_top` e `set_fullscreen` — quattro modi di far sparire una
/// finestra da sotto le dita di chi la sta guardando, in cambio di tre bottoni.
///
/// Quel che il ridimensionamento ai bordi non perde niente: lo fa `tao` da sé,
/// in `WM_NCHITTEST`, per ogni finestra senza decorazioni che sia
/// ridimensionabile e non ingrandita. Nessuna maniglia da disegnare di qua.
#[tauri::command]
fn finestra_trascina(finestra: tauri::Window) {
    let _ = finestra.start_dragging();
}

/// Riduce a icona.
#[tauri::command]
fn finestra_riduci(finestra: tauri::Window) {
    let _ = finestra.minimize();
}

/// Ingrandisce o rimette com'era, e dice com'è rimasta.
///
/// Restituisce lo stato **dopo**: senza, la pagina dovrebbe chiedere di nuovo
/// subito dopo aver chiesto di cambiare, e fra le due domande c'è un giro di
/// IPC in cui l'icona resta quella di prima.
///
/// A schermo intero il primo significato di quel bottone è «rimpicciolisci», e
/// quindi è da lì che si esce: ingrandire *sotto* lo schermo intero cambierebbe
/// una finestra che non si vede, cioè sarebbe un bottone che non fa niente
/// mentre l'icona dice il contrario.
#[tauri::command]
fn finestra_ingrandisci(finestra: tauri::Window) -> bool {
    if finestra.is_fullscreen().unwrap_or(false) {
        let _ = finestra.set_fullscreen(false);
        // `tao` rimette la finestra dov'era, ingrandita compresa: quel che
        // torna è lo stato in cui è ricaduta, non un «no» dato per scontato.
        return finestra.is_maximized().unwrap_or(false);
    }
    let _ = if finestra.is_maximized().unwrap_or(false) {
        finestra.unmaximize()
    } else {
        finestra.maximize()
    };
    finestra.is_maximized().unwrap_or(false)
}

/// È più grande della finestra adesso?
///
/// La pagina lo chiede all'apertura e a ogni ridimensionamento: ingrandire non
/// passa sempre di qua — c'è il doppio clic sulla fascia, `Win`+`↑`, e
/// l'affiancamento di Windows — e l'icona deve dire il vero anche allora.
///
/// Ingrandita **o** a schermo intero, perché è a quest'unica domanda che serve
/// rispondere: da tutte e due si torna col medesimo gesto, e l'icona che le
/// distinguesse offrirebbe di ingrandire una finestra che occupa già lo
/// schermo.
#[tauri::command]
fn finestra_ingrandita(finestra: tauri::Window) -> bool {
    finestra.is_maximized().unwrap_or(false) || finestra.is_fullscreen().unwrap_or(false)
}

/// Entra o esce dallo schermo intero, e dice com'è rimasta.
///
/// È il quinto gesto, e l'unico che la fascia grigia di Windows non dava: `F11`
/// arriva dalla tabella delle scorciatoie, come ogni altro tasto della finestra
/// (`tastiera.ts`). Vale però la regola dei quattro qui sopra — un comando
/// nostro invece di aprire `capabilities/default.json` a `core:window`, che
/// darebbe alla pagina anche `set_position`, `set_size` e `set_always_on_top`.
/// Quel che si rifiuta è il mazzo intero, non lo schermo intero: qui c'è una
/// funzione sola, chiamata da un tasto che chi guarda ha premuto apposta.
///
/// Restituisce lo stato **dopo**, per la stessa ragione di
/// `finestra_ingrandisci`. Chiedere a `set_fullscreen` invece che fidarsi del
/// contrario di prima non è pignoleria: se il sistema operativo lo nega — un
/// monitor staccato mentre si preme — la pagina deve saperlo, non crederci.
#[tauri::command]
fn finestra_schermo_intero(finestra: tauri::Window) -> bool {
    let intero = finestra.is_fullscreen().unwrap_or(false);
    let _ = finestra.set_fullscreen(!intero);
    finestra.is_fullscreen().unwrap_or(false)
}

/// Chiude la finestra, cioè l'applicazione — o la nasconde.
///
/// `close()` e non `exit()`: fa la stessa strada del tasto di sistema — l'evento
/// di chiusura, e chi lo ascolta — invece di scavalcarla.
///
/// Quella scelta, fatta quando l'unico ascoltatore spegneva i fili, è ciò che
/// permette adesso a `on_window_event` di **impedire** questa chiusura quando il
/// secondo piano è acceso. La X di `BarraTitolo.tsx` non sa niente della
/// preferenza, e non deve: chiede di chiudere, e chi ascolta decide.
#[tauri::command]
fn finestra_chiudi(finestra: tauri::Window) {
    let _ = finestra.close();
}

/// Mostra la finestra fra due secondi, qualunque cosa succeda di là.
///
/// Salvo che di là si sia deciso di chiudere: due secondi sono lunghi
/// abbastanza perché qualcuno apra e richiuda subito, e far riapparire una
/// finestra che sta morendo vuol dire toccare l'`AppHandle` durante la
/// demolizione del ciclo degli eventi — che è esattamente il modo in cui questa
/// applicazione moriva. Vedi [`spegnimento`].
fn rete_di_sicurezza(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(ATTESA_PRIMO_COLORE);
        if spegnimento::in_uscita() {
            return;
        }
        if let Some(finestra) = app.get_webview_window("main") {
            let _ = finestra.show();
        }
    });
}

fn main() {
    // Per prima cosa, prima ancora di costruire la finestra: da qui in poi un
    // panico lascia scritto cos'era invece di far sparire il processo in
    // silenzio. Finché `setup` non apre il diario le righe vanno su `stderr` —
    // cioè da nessuna parte, in rilascio — ma il gancio installato tardi non
    // coprirebbe proprio i panici dell'avvio, che sono quelli che nessuno
    // riesce a raccontare.
    diario::installa_gancio_dei_panici();

    let costruita = tauri::Builder::default()
        // Scegliere la cartella della musica è la prima cosa che fa chi apre
        // Aether: un campo di testo in cui incollare un percorso funziona, e
        // sbaglia al primo spazio o alla prima barra rovesciata.
        .plugin(tauri_plugin_dialog::init())
        // La schermata di consenso di Google si apre nel browser di **sistema**.
        // Mai nella webview: una finestra dell'applicazione che sa disegnare
        // `accounts.google.com` è una superficie di phishing, e il CSP di
        // `tauri.conf.json` resta identico.
        .plugin(tauri_plugin_opener::init())
        // Il controllo degli aggiornamenti. Il plugin qui non fa niente da sé:
        // non parte nessuna richiesta al `init`, e la finestra non ha il
        // permesso di chiamarlo — `capabilities/default.json` non elenca
        // `updater:*`, ed è deliberato. Tutto passa da `crate::aggiornamenti`,
        // che è l'unico posto in cui sta scritto ogni quanto si controlla e a
        // quali condizioni.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let data_dir = cartella_dati(app.handle())?;
            // Il diario **prima di tutto il resto**, libreria compresa: quel
            // che si vuole leggere quando Aether non si apre è precisamente
            // ciò che succede nelle righe qui sotto.
            diario::apri(&data_dir);
            // Prima della libreria, perché non ne ha bisogno: una cella vuota
            // e un cliente HTTP che non ha ancora aperto niente. La prima
            // richiesta parte quando qualcuno incolla un link, così chi non
            // importa mai da un catalogo non paga nemmeno una stretta di mano.
            app.manage(importa::StatoImport::nuovo());
            // Come sopra, e per la stessa ragione: una cella vuota e un bit.
            // Lo zip arriva solo se qualcuno apre la schermata dell'account.
            app.manage(account::StatoAccount::nuovo());
            // E anche questo: tre bit e i cataloghi, che sono a loro volta un
            // cliente HTTP inerte. Non c'è più nessun binario da cercare sul
            // disco — la coda parla solo con la rete, ed è il motivo per cui
            // l'installer non impacchetta più niente di eseguibile.
            app.manage(procura::StatoProcura::nuovo());

            // I testi: due bit, due contatori, e i fornitori che nascono alla
            // prima richiesta. Prima di allora non c'è niente da aprire, e
            // aprire il deposito all'avvio vorrebbe dire una connessione al
            // database in più per chi il pannello del testo non lo apre mai.
            app.manage(testi::StatoTesti::nuovo());

            let stato = stato::Stato::apri(data_dir);
            // La riga di avvio va stampata **prima** di disegnare: se
            // l'apertura del database è fallita, è l'unica traccia che resta
            // quando la finestra mostra solo l'errore.
            stato::riga_di_avvio(&stato);
            app.manage(stato);

            // Il lettore **dopo** la libreria: apre il dispositivo audio e
            // rilegge la coda di ieri, e per la seconda cosa il database deve
            // già essere aperto.
            // Lo stato dei controlli multimediali **prima** del lettore, anche
            // se a riempirlo si farà più sotto. Il filo dell'orologio parte
            // insieme al lettore e chiama `manda_stato` da sé quando il
            // dispositivo audio sparisce: se quel momento arrivasse prima di
            // questa riga, cercherebbe uno stato non registrato. Registrarlo
            // vuoto costa un mutex e un intero.
            app.manage(media::StatoMedia::nuovo());

            let (lettore, orecchio, orecchio_dispositivi) =
                riproduzione::StatoLettore::avvia(app.handle());
            riproduzione::riga_di_avvio_lettore(&lettore);
            app.manage(lettore);
            // Il filo che apre il brano successivo, insieme agli altri del
            // lettore. L'ordine fra questi quattro non conta: il canale del
            // preparatore non ha limite, quindi una spinta che arrivasse prima
            // che il filo sia in piedi lo aspetta lì invece di perdersi.
            riproduzione::avvia_preparatore(app.handle().clone(), orecchio);
            riproduzione::riprendi_coda(app.handle());
            riproduzione::avvia_orologio(app.handle().clone());
            riproduzione::avvia_spettro(app.handle().clone());
            // Il filo che guarda le uscite audio andare e venire. Ultimo dei
            // quattro, e non conta nemmeno qui: la prima passata la fa da sé
            // appena parte, e chi la pungola prima che sia in piedi trova un
            // canale che tiene la spinta finché non c'è nessuno a raccoglierla.
            riproduzione::avvia_sorveglianza(app.handle().clone(), orecchio_dispositivi);

            // I controlli veri **dopo** il lettore, e per una ragione che non è
            // l'ordine di dipendenza ma quello della finestra: su Windows le
            // SMTC si appendono a una finestra, e qui la finestra esiste già —
            // `tauri.conf.json` la crea nascosta, non assente. Serve anche la
            // libreria aperta, per sapere dove stanno le copertine. Il primo
            // stato le riempirà da sé, perché passa dallo stesso `manda_stato`
            // di tutti gli altri.
            media::avvia(app.handle());

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
            // La sincronia **subito dopo il backup**: condivide con lui la cache
            // dell'access token di Drive, e il suo filo aspetta venti secondi —
            // meno della nuvola, perché una passata a vuoto su una cartella è una
            // lettura di directory e nient'altro.
            let (sincronia, orecchio) = sincronia::StatoSincronia::nuovo();
            app.manage(sincronia);
            sincronia::avvia_filo(app.handle().clone(), orecchio);

            let (arricchimento, orecchio) = arricchimento::StatoArricchimento::nuovo();
            app.manage(arricchimento);
            arricchimento::avvia_filo(app.handle().clone(), orecchio);

            // L'analisi sonora **dopo** l'arricchimento, e il suo filo aspetta
            // due minuti. È l'unica cosa qui dentro che legge dal disco a tutta
            // velocità senza che nessuno l'abbia chiesta, e cede alla
            // riproduzione: se in quei due minuti parte una canzone, non
            // comincia affatto.
            let (analisi, orecchio) = analisi::StatoAnalisi::nuovo();
            app.manage(analisi);
            analisi::avvia_filo(app.handle().clone(), orecchio);

            // Lo scrobbling **dopo** la libreria, perché la prima cosa che fa è
            // guardare se è rimasto qualcosa in coda dalla sessione precedente.
            // Il suo filo dorme finché non lo si chiama: a servizi scollegati
            // non costa niente.
            app.manage(scrobble::StatoScrobble::nuovo());
            scrobble::avvia(app.handle());

            // I modelli di linguaggio: un lucchetto vuoto e un contatore, e
            // nessun filo. Qui non parte niente da solo — non c'e` niente da
            // riprendere e nessuna coda da svuotare — e finche' nessuno apre la
            // chat dello Studio questo stato non tocca ne' rete ne' disco.
            app.manage(ia::StatoIa::nuovo());

            // Gli aggiornamenti **dopo tutti gli altri**, e il suo filo aspetta
            // due minuti: è la cosa meno urgente che l'applicazione possa fare
            // all'apertura, ed è anche l'unica che parla con la rete senza che
            // nessuno gliel'abbia chiesto. Due minuti sono il tempo perché la
            // scansione e la coda di ieri abbiano finito di contendersi il
            // disco.
            let (aggiornamenti, orecchio) = aggiornamenti::StatoAggiornamenti::nuovo();
            app.manage(aggiornamenti);
            aggiornamenti::avvia_filo(app.handle().clone(), orecchio);

            // Il vassoio **dopo** la libreria, perché la prima cosa che fa è
            // leggere la preferenza da lì. Non compare ancora niente nell'area
            // di notifica: le due etichette del menù arrivano dalla finestra
            // qualche decina di millisecondi più tardi, e senza di quelle non
            // c'è un menù da costruire. Vedi `vassoio`.
            app.manage(vassoio::StatoVassoio::nuovo());
            vassoio::avvia(app.handle());

            // Per ultima, e dopo tutto il resto: quel che conta è che parta a
            // finestra già costruita, non prima di aprire il database.
            rete_di_sicurezza(app.handle());
            Ok(())
        })
        // La chiusura, prima che diventi un'uscita.
        //
        // `on_window_event` e non il `RunEvent` in fondo a questo file: là
        // `CloseRequested` arriva senza la maniglia che permette di impedirlo,
        // e impedirlo è tutto il punto. I due gestori guardano lo stesso
        // evento da due posti diversi e fanno due cose diverse — questo decide
        // se la chiusura succede, quello decide cosa spegnere quando succede.
        .on_window_event(|finestra, evento| {
            let tauri::WindowEvent::CloseRequested { api, .. } = evento else {
                return;
            };
            // Chi sta uscendo davvero passa di qui: «Esci» dal vassoio alza la
            // bandiera prima di chiamare `exit`, e impedirgli la chiusura
            // vorrebbe dire un programma che non si spegne più.
            if spegnimento::in_uscita() {
                return;
            }
            if !vassoio::nasconde(finestra.app_handle()) {
                return;
            }
            api.prevent_close();
            let _ = finestra.hide();
        })
        // La variante asincrona: consegna un `responder` invece di pretendere
        // la risposta subito. Serve perché la lettura dal disco possa avvenire
        // fuori dal thread che riceve la richiesta — una griglia che scorre ne
        // fa decine al secondo, e farle in fila le farebbe apparire a scatti.
        .register_asynchronous_uri_scheme_protocol("aether-cover", copertine::servi)
        .invoke_handler(tauri::generate_handler![
            pronto,
            finestra_trascina,
            finestra_riduci,
            finestra_ingrandisci,
            finestra_ingrandita,
            finestra_schermo_intero,
            finestra_chiudi,
            vassoio::secondo_piano,
            vassoio::secondo_piano_attiva,
            vassoio::vassoio_lingua,
            comandi::avvio,
            comandi::imposta_cartelle,
            comandi::cartelle_candidate,
            comandi::imposta_cartella_download,
            comandi::cronologia,
            comandi::cronologia_conteggio,
            comandi::scansiona,
            comandi::annulla_scansione,
            comandi::cerca,
            comandi::cerca_conteggio,
            comandi::brani,
            comandi::casa,
            comandi::settimana,
            comandi::settimana_apri,
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
            comandi::imposta_lingua,
            comandi::imposta_scorciatoie,
            comandi::profilo_esporta,
            comandi::profilo_piano,
            comandi::profilo_importa,
            importa::import_anteprima,
            importa::import_piano,
            importa::import_esegui,
            importa::import_rapporto,
            importa::import_rapporti,
            importa::import_diagnostica,
            account::account_stato,
            account::account_piano,
            account::account_importa,
            account::archivio_apri,
            account::archivio_dimentica,
            account::cronologia_dimentica_importati,
            procura::scarica_desiderati,
            procura::annulla_scarico,
            procura::scarico_stato,
            procura::riprova_falliti,
            procura::da_comprare,
            procura::cerca_dove_comprare,
            procura::alternative_ammettile,
            riordino::piano_riordino,
            riordino::esegui_riordino,
            riordino::annulla_riordino,
            metadati::metadati_conteggio,
            metadati::metadati_incerti,
            metadati::metadati_correggi,
            metadati::metadati_conferma,
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
            skin::skin_disinstalla,
            skin::skin_scegli,
            skin::accento_dinamico,
            skin::accento_dinamico_attiva,
            skin::accento_copertina,
            studio::studio_registro,
            studio::studio_valida,
            studio::studio_documento,
            studio::studio_pacchetto,
            studio::studio_salva,
            studio::studio_scarta,
            studio::studio_esporta,
            studio::studio_istantanee,
            studio::studio_istantanea,
            studio::studio_ripristina,
            ia::ia_profili,
            ia::ia_salva_profilo,
            ia::ia_elimina_profilo,
            ia::ia_scegli_profilo,
            ia::ia_modelli,
            ia::ia_conversa,
            ia::ia_operazioni,
            ia::ia_ferma,
            riproduzione::suona,
            riproduzione::radio,
            riproduzione::pausa,
            riproduzione::riprendi,
            riproduzione::alterna,
            riproduzione::prossimo,
            riproduzione::precedente,
            riproduzione::vai_a,
            riproduzione::volume,
            riproduzione::equalizzatore,
            riproduzione::normalizzazione,
            riproduzione::spegnimento,
            riproduzione::autoplay,
            riproduzione::dissolvenza,
            riproduzione::riapri_audio,
            riproduzione::dispositivi_audio,
            riproduzione::scegli_dispositivo_audio,
            riproduzione::riprova_corrente,
            riproduzione::spettro,
            riproduzione::spettro_bande,
            riproduzione::spettro_bande_scegli,
            riproduzione::spettro_visibile,
            riproduzione::spettro_visibile_scegli,
            riproduzione::spettro_qualita,
            riproduzione::spettro_qualita_scegli,
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
            sincronia::sincronia_stato,
            sincronia::sincronia_attiva,
            sincronia::sincronia_adesso,
            sincronia::sincronia_magazzino,
            sincronia::sincronia_dispositivi,
            sincronia::sincronia_accoppia,
            sincronia::sincronia_dimentica,
            testi::testo_brano,
            testi::testo_scarto,
            testi::testo_cerca,
            testi::testi_stato,
            testi::testi_rete,
            testi::testi_riempi,
            testi::testi_ferma,
            testi::testo_aggancia,
            testi::testo_salva,
            testi::testo_pubblica,
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
            aggiornamenti::aggiornamenti_stato,
            aggiornamenti::aggiornamenti_attivo,
            aggiornamenti::aggiornamenti_adesso,
            aggiornamenti::aggiornamenti_salta,
            aggiornamenti::aggiornamenti_installa,
            diario::diario_apri,
            diario::diario_annota,
            comandi::apri_documento,
        ])
        .build(tauri::generate_context!());

    // `build` e non `run`, per una riga sola: quella che segue.
    //
    // `run(context)` avvia il ciclo degli eventi e non lascia nessun posto in
    // cui accorgersi che sta finendo. `build` restituisce l'applicazione, e
    // `App::run` prende una callback che riceve ogni `RunEvent` — compresi i
    // tre che dicono «si chiude». È l'unico punto del programma in cui la
    // chiusura è un fatto osservabile invece che una cosa che succede.
    //
    // Il `Result` si tratta come prima, e per lo stesso motivo: un messaggio di
    // panico è un messaggio per chi ha scritto il codice, e chi apre
    // l'applicazione merita di leggere cos'è andato storto invece di una
    // traccia di stack.
    match costruita {
        Ok(app) => app.run(|mano, evento| {
            // `CloseRequested` è la presa più precoce che Tauri offra: arriva
            // mentre la finestra c'è ancora, prima che `tao` dichiari il ciclo
            // distrutto. Gli altri due sono la cintura oltre le bretelle — se
            // un giorno si uscisse per una strada che non passa dalla finestra
            // (l'aggiornatore che riavvia, un `exit()` da qualche parte), la
            // bandiera si alza lo stesso.
            //
            let chiusura = matches!(
                evento,
                tauri::RunEvent::WindowEvent {
                    event: tauri::WindowEvent::CloseRequested { .. },
                    ..
                }
            );
            if !chiusura
                && !matches!(
                    evento,
                    tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
                )
            {
                return;
            }

            // Con il secondo piano acceso, però, una chiusura non è un'uscita:
            // `on_window_event` l'ha appena impedita, e la finestra si è solo
            // nascosta. Alzare qui la bandiera darebbe il guasto peggiore dei
            // due possibili — musica che continua mentre si ferma tutto il
            // resto, perché `spegnimento` non torna mai indietro: orologio,
            // spettro, sorveglianza dei dispositivi, posizione di ripresa,
            // scrobbling e scheda del sistema, spenti per sempre dentro un
            // programma vivo.
            if chiusura && !spegnimento::in_uscita() && vassoio::nasconde(mano) {
                return;
            }

            spegnimento::chiedi();
            // E poi si dice alla scansione di smettere, se ce n'è una. Non è un
            // di più rispetto alla riga qui sopra: `spegnimento` ferma i fili
            // che guardano la sua bandiera, e la scansione guarda la propria —
            // quella che alza anche «Annulla». Senza questa riga una scansione
            // che sta leggendo una condivisione di rete continuerebbe a leggerla
            // mentre il resto del programma se ne va, e le due righe che
            // seguono aspetterebbero il suo lucchetto.
            //
            // `try_state` e non `state`: `state` panica se lo stato non c'è, e
            // qui si sta smontando tutto. Nessuno stato vuol dire nessuna
            // scansione da fermare.
            if let Some(stato) = mano.try_state::<stato::Stato>() {
                stato.ferma_scansione();
            }
            // L'ultima posizione di un cursore mosso un attimo prima di
            // chiudere: il filo dell'orologio, che di solito la scrive, è
            // appena uscito per via della riga qui sopra.
            riproduzione::salva_uscendo(mano);
            // Poi la scheda del sistema. Il distacco parla all'`HWND` della
            // finestra, e questo è l'ultimo momento in cui quell'`HWND` esiste
            // ancora.
            media::stacca(mano);
            // E l'icona dell'area di notifica, per la stessa ragione: anche lei
            // se ne va parlando col sistema operativo, e lasciarla cadere da
            // sola vorrebbe dire farlo a ciclo degli eventi già smontato.
            vassoio::stacca(mano);
        }),
        Err(err) => {
            // Nel diario e non solo su `stderr`: questo è il ramo in cui la
            // finestra non è mai comparsa, cioè l'unico caso in cui chi guarda
            // non ha nient'altro da leggere. Se `setup` era arrivato ad aprire
            // il diario, la riga resta sul disco; se non ci era arrivato, resta
            // almeno in sviluppo.
            nota!("[avvio] Aether non è riuscito ad avviarsi: {err}");
            std::process::exit(1);
        }
    }
}

/// Dove stanno database, copertine e skin.
///
/// Normalmente la cartella dati dell'applicazione, che su Windows è
/// `%APPDATA%\io.github.federicobaratti.aether`. `AETHER_DATI` la sostituisce.
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
