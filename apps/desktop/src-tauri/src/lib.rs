//! L'applicazione: su Windows una finestra, su Android un'attività.
//!
//! Una finestra sopra `rymembra-app`, e nient'altro. Tutto quel che decide sta nel
//! nucleo; qui c'è l'apertura della finestra, il registro dei comandi e il
//! protocollo che serve le copertine.
//!
//! # Perché una libreria e non solo un binario
//!
//! Perché su Android non esiste un `main`: il sistema carica una libreria
//! condivisa dentro un processo che ha già fatto partire lui, e Tauri ci entra
//! dal punto che [`run`] dichiara con `mobile_entry_point`. Il binario
//! `rymembra` di Windows è rimasto, ed è una riga sola: chiama [`run`] — vedi
//! `main.rs`.
//!
//! # Le due piattaforme
//!
//! I cfg `desktop` e `mobile` li accende `tauri-build` (in `build.rs`). Quel che
//! esiste solo su un computer — il vassoio, i pulsanti della barra, le SMTC, il
//! Cestino, l'updater, l'istanza unica — sta dietro `desktop`. Dove un modulo
//! intero non ha senso sul telefono, la sua controparte mobile sta in
//! `src/mobile/` con le **stesse** funzioni e gli stessi comandi: così il
//! registro dei comandi qui sotto è uno solo, e la pagina non deve sapere su
//! quale piattaforma gira per non ricevere un «command not found».

mod account;
#[cfg_attr(mobile, path = "mobile/aggiornamenti.rs")]
mod aggiornamenti;
mod analisi;
mod arricchimento;
mod browser;
// Android Auto: l'albero della libreria per lo schermo dell'auto. Sul computer
// non serve, ma le sue regole — gli identificativi, le pagine, le lettere, la
// ricerca a voce — si provano lì, con `cargo test`.
#[cfg(any(mobile, test))]
#[cfg_attr(
    not(mobile),
    allow(dead_code, reason = "nelle prove sul computer: lo usa solo Android")
)]
mod auto;
mod barra;
mod cancellazione;
mod cartelle;
mod comandi;
mod copertine;
mod destinazioni;
mod diario;
mod disparte;
mod errore;
mod esplora;
mod giro;
mod ia;
mod importa;
mod insieme;
mod lontano;
mod manutenzione;
mod media;
mod metadati;
mod nuvola;
mod nuvola_app;
mod nuvola_musica;
mod playlist;
mod procura;
mod profilo;
mod riproduzione;
// Le release di GitHub per gli aggiornamenti del telefono: come `auto`, le
// regole si provano anche sul computer.
#[cfg(any(mobile, test))]
#[cfg_attr(
    not(mobile),
    allow(dead_code, reason = "nelle prove sul computer: lo usa solo Android")
)]
mod rilasci;
mod scrobble;
mod sincronia;
mod skin;
mod sorveglia;
mod spegnimento;
mod stato;
mod studio;
mod telefono;
mod testi;
mod trascinati;
#[cfg_attr(mobile, path = "mobile/vassoio.rs")]
mod vassoio;
mod viste;
mod zoom;

use std::ffi::OsString;
use std::path::PathBuf;

use rymembra_app::trasloco;
use rymembra_domain::eredita;
use rymembra_domain::errors::{AppError, ErrorCode};
use tauri::Manager as _;
use tauri_plugin_dialog::{DialogExt as _, MessageDialogKind};

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
///
/// Su Android la finestra nasce visibile (`tauri.android.conf.json`): è
/// l'attività, e un'attività che non si mostra il sistema la considera bloccata.
/// `pronto` e la rete di sicurezza restano, e non fanno niente di male.
const ATTESA_PRIMO_COLORE: std::time::Duration = std::time::Duration::from_secs(2);

/// Il frontend ha applicato skin e tema: si può guardare.
///
/// Idempotente per costruzione — `show()` su una finestra già visibile non fa
/// niente — e per questo la rete di sicurezza qui sopra può chiamarlo senza
/// coordinarsi con nessuno.
///
/// È anche il via del controllo aggiornamenti d'avvio: vedi
/// `aggiornamenti::finestra_pronta`.
#[tauri::command]
fn pronto(finestra: tauri::Window) {
    let _ = finestra.show();
    aggiornamenti::finestra_pronta(finestra.app_handle());
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
#[cfg(desktop)]
#[tauri::command]
fn finestra_trascina(finestra: tauri::Window) {
    let _ = finestra.start_dragging();
}

/// Riduce a icona.
#[cfg(desktop)]
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
#[cfg(desktop)]
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
#[cfg(desktop)]
#[tauri::command]
fn finestra_schermo_intero(finestra: tauri::Window) -> bool {
    let intero = finestra.is_fullscreen().unwrap_or(false);
    let _ = finestra.set_fullscreen(!intero);
    finestra.is_fullscreen().unwrap_or(false)
}

/// Su Android la finestra è lo schermo: non si trascina, non si riduce e non
/// si ingrandisce, e la barra del titolo che li chiede lì non si disegna. I
/// comandi esistono lo stesso, perché il registro dei comandi è uno solo, e
/// rispondono il vero: la finestra non è cambiata.
#[cfg(mobile)]
#[tauri::command]
fn finestra_trascina() {}

/// Vedi [`finestra_trascina`]: su Android non c'è niente da ridurre.
#[cfg(mobile)]
#[tauri::command]
fn finestra_riduci() {}

/// Vedi [`finestra_trascina`]: su Android la finestra occupa già lo schermo.
#[cfg(mobile)]
#[tauri::command]
fn finestra_ingrandisci() -> bool {
    false
}

/// Vedi [`finestra_trascina`]: lo schermo intero di Android (senza barre di
/// sistema) è un'altra cosa, e la decide l'attività, non questo comando.
#[cfg(mobile)]
#[tauri::command]
fn finestra_schermo_intero() -> bool {
    false
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

/// Costruisce l'applicazione e la fa girare fino all'uscita.
///
/// Su Windows la chiama `main.rs`; su Android la chiama il sistema, attraverso
/// il punto d'ingresso che `mobile_entry_point` genera.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Per prima cosa, prima ancora di costruire la finestra: da qui in poi un
    // panico lascia scritto cos'era invece di far sparire il processo in
    // silenzio. Finché `setup` non apre il diario le righe del gancio vanno nel
    // file di riserva (`diario::RISERVA`, in `%LOCALAPPDATA%`): è il caso della
    // WebView2 che non si crea, che in Tauri 2 succede **prima** del nostro
    // `setup` e diventa un panico «Failed to setup app»; per quel panico il
    // gancio mostra anche un dialogo. Il gancio installato tardi non
    // coprirebbe proprio i panici dell'avvio, che sono quelli che nessuno
    // riesce a raccontare.
    diario::installa_gancio_dei_panici();
    // Il portachiavi prima di tutto il resto, come il gancio: su Android è il
    // Keystore, e `DiSistema` lo deve trovare già al suo posto quando il primo
    // filo — il backup, lo scrobbling — chiede un segreto. Altrove non fa niente.
    telefono::installa_il_portachiavi();
    // E l'uscita audio, per la stessa ragione: il motore si apre nel `setup`.
    telefono::installa_l_uscita_audio();

    let costruttore = tauri::Builder::default();
    // L'istanza unica per prima, come chiede il plugin: il secondo processo
    // deve morire prima che qualunque altro plugin faccia qualcosa. Su Android
    // non serve — il sistema non apre mai due volte la stessa attività — e il
    // plugin lì non esiste.
    #[cfg(desktop)]
    let costruttore = una_sola_istanza(costruttore);
    let costruttore = costruttore
        // Scegliere la cartella della musica è la prima cosa che fa chi apre
        // Rymembra: un campo di testo in cui incollare un percorso funziona, e
        // sbaglia al primo spazio o alla prima barra rovesciata.
        .plugin(tauri_plugin_dialog::init())
        // La schermata di consenso di Google si apre nel browser di **sistema**.
        // Mai nella webview: una finestra dell'applicazione che sa disegnare
        // `accounts.google.com` è una superficie di phishing, e il CSP di
        // `tauri.conf.json` resta identico.
        //
        // Senza l'aggancio dei clic: di serie il plugin inietta nella pagina uno
        // script che apre nel browser ogni `<a target="_blank">` cliccato — un
        // modo di uscire dalla finestra che non passa da nessun comando nostro,
        // e che una skin o un testo di un catalogo potrebbero usare. I link che
        // Rymembra apre li apre `comandi::apri_documento`, che sa quali sono.
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        );
    // Il controllo degli aggiornamenti. Il plugin qui non fa niente da sé:
    // non parte nessuna richiesta al `init`, e la finestra non ha il
    // permesso di chiamarlo — `capabilities/default.json` non elenca
    // `updater:*`, ed è deliberato. Tutto passa da `crate::aggiornamenti`,
    // che è l'unico posto in cui sta scritto ogni quanto si controlla e a
    // quali condizioni. Su Android l'updater di Tauri non esiste: lì
    // `aggiornamenti` sa solo dire che c'è una versione nuova.
    #[cfg(desktop)]
    let costruttore = costruttore.plugin(tauri_plugin_updater::Builder::new().build());
    let costruita = costruttore
        .setup(|app| {
            // Per prima cosa: il `setup` gira sul filo della finestra, ed è il
            // solo punto in cui lo si può riconoscere. Vedi
            // `stato::con_libreria`.
            stato::segna_filo_della_finestra();
            let scelta = cartella_dati(app.handle())?;
            // Se la cartella l'ha scelta una variabile, per chi tiene una
            // cache fuori dai dati: la risposta dell'unica lettura delle
            // variabili, e non una seconda lettura con una regola sua. Vedi
            // [`cartella_di_cache`]. Prima di ogni stato che apre una cache.
            let a_mano = fatta_a_mano(&scelta);
            app.manage(DatiAMano(a_mano));
            // In una build di sviluppo, una cartella dati di prova ha anche un
            // portachiavi di prova: il servizio vero è fisso, e senza questo la
            // copia di prova userebbe gli account veri (e l'identità del
            // collegamento via internet dell'app installata). Prima di ogni
            // filo che arrivi ai segreti. Nelle build di rilascio non esiste.
            #[cfg(all(desktop, debug_assertions))]
            if a_mano {
                rymembra_oauth::portachiavi::usa_il_servizio_delle_prove();
            }
            // I dati di chi aveva la versione di prima, **prima di tutto il
            // resto**: se la cartella nuova deve arrivare dalla vecchia, deve
            // arrivare prima che qualcosa ci scriva dentro — il diario qui
            // sotto, il database, le copertine — perché lo spostamento di una
            // cartella intera vale solo se quella nuova non c'è ancora. Il
            // diario si apre subito dopo, comunque, ed è lì che il trasloco
            // racconta cos'ha fatto. Una cartella che la variabile indica
            // relativa non arriva nemmeno fin qui: è già un rifiuto.
            let esito = scelta.and_then(|scelta| prepara_i_dati(&scelta).map(|()| scelta.cartella));
            // Un rifiuto non ferma il `setup`: gli stati vanno registrati
            // comunque, perché i comandi li estraggono per tipo e uno stato
            // mancante farebbe fallire ognuno con «state not managed», cioè
            // con un messaggio per chi scrive il codice invece di quello vero.
            // Ma i passi che aprono o scrivono nella cartella nuova non
            // partono — vedi [`rifiuta`].

            // Prima della libreria, perché non ne ha bisogno: una cella vuota
            // e un cliente HTTP che non ha ancora aperto niente. La prima
            // richiesta parte quando qualcuno incolla un link, così chi non
            // importa mai da un catalogo non paga nemmeno una stretta di mano.
            app.manage(importa::StatoImport::nuovo());
            app.manage(esplora::StatoEsplora::nuovo());
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

            // L'albero delle cartelle: un percorso e tre parole di stato, e
            // nient'altro. Non apre il database e non legge niente — lo fa alla
            // prima volta che qualcuno apre il pannello, e lo lascia cadere
            // cinque minuti dopo l'ultima domanda. Chi il pannello non lo apre
            // mai non paga né la lettura né la memoria.
            //
            // Dentro un `Arc` perché il filo che sfratta l'albero se lo tiene
            // per conto suo, senza dover passare da Tauri per ritrovarlo.
            //
            // Solo se i dati sono stati presi: l'indice apre il database alla
            // prima domanda, e dopo un rifiuto del trasloco nella cartella
            // nuova non si deve aprire niente, nemmeno in sola lettura. Senza
            // indice le sue domande rispondono `internal.aborted`: lo cercano
            // per `try_state` (vedi `cartelle::indice`), non con un'estrazione
            // per tipo.
            if let Ok(data_dir) = &esito {
                app.manage(std::sync::Arc::new(cartelle::IndiceCartelle::nuovo(
                    data_dir,
                )));
            }

            // Dopo un rifiuto la libreria è l'errore che l'applicazione sa già
            // trattare — la stessa di un database che non si è aperto — e
            // `Stato::apri` non si chiama: creerebbe la cartella nuova, il
            // database e le copertine, proprio ciò che il rifiuto deve evitare.
            let stato = match esito {
                Ok(data_dir) => stato::Stato::apri(data_dir),
                Err(errore) => rifiuta(app.handle(), &errore),
            };
            // La riga di avvio va stampata **prima** di disegnare: se
            // l'apertura del database è fallita, è l'unica traccia che resta
            // quando la finestra mostra solo l'errore.
            stato::riga_di_avvio(&stato);
            app.manage(stato);
            // Le cartelle dal cloud **subito dopo** la libreria: lo specchio si
            // carica da lì, e prima di ogni scansione — anche quella del lettore
            // che riapre la coda di ieri — chi apre un file deve già trovare il
            // cloud. I fili dei conti partono qui, e la loro prima domanda alla
            // rete non aspetta niente: è quel che rende una cartella appena
            // cambiata su OneDrive visibile all'avvio.
            nuvola_musica::avvia(app.handle());

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
            // Lo stesso per i pulsanti nell'anteprima, e per una ragione in
            // più: il loro stato ricorda l'aspetto anche prima che i pulsanti
            // ci siano, e il primo stato del lettore arriva da `riprendi_coda`
            // qui sotto. Vedi `barra`.
            app.manage(barra::StatoBarra::nuovo());

            let (lettore, orecchio, orecchio_dispositivi) =
                riproduzione::StatoLettore::avvia(app.handle());
            riproduzione::riga_di_avvio_lettore(&lettore);
            app.manage(lettore);
            // Il filo che apre il brano successivo, insieme agli altri del
            // lettore. L'ordine fra questi quattro non conta: il canale del
            // preparatore non ha limite, quindi una spinta che arrivasse prima
            // che il filo sia in piedi lo aspetta lì invece di perdersi.
            riproduzione::avvia_preparatore(app.handle().clone(), orecchio);
            // Lo stato del collegamento via internet prima della coda: la coda
            // di ieri può avere brani di un altro dispositivo, e le loro voci
            // si rifanno con lui. Il suo filo parte più sotto, dopo «Ascolta
            // insieme»; lo stato da solo non apre niente.
            app.manage(lontano::StatoLontano::nuovo());
            riproduzione::riprendi_coda(app.handle());
            riproduzione::avvia_orologio(app.handle().clone());
            // Sul telefono lo spettro non c'è (`InRiproduzione.tsx`,
            // `CON_SPETTRO`), e un filo che si sveglia quattro volte al secondo
            // per sentirsi dire che nessuno guarda è batteria spesa per niente,
            // per tutta la vita del processo — che su Android è lunga, perché
            // la musica continua anche a schermata chiusa.
            #[cfg(desktop)]
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
            // I pulsanti nell'anteprima, accanto ai controlli multimediali e per
            // la stessa ragione: si appendono alla finestra, che qui esiste già.
            // Compaiono quando la finestra si mostra.
            barra::avvia(app.handle());

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

            // La manutenzione del database: controllo, `optimize`, copertine
            // orfane. Il suo filo aspetta un minuto — vedi `manutenzione`.
            manutenzione::avvia(app.handle());

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

            // «Ascolta insieme» **dopo** il lettore e la libreria: la sessione
            // legge le impostazioni da lì e parla col lettore attraverso il
            // ponte. Spento di serie, e da spento il suo filo dorme: nessun
            // pacchetto in rete finché qualcuno non accende l'interruttore.
            let (insieme, orecchio) = insieme::StatoInsieme::nuovo();
            app.manage(insieme);
            insieme::avvia_filo(app.handle().clone(), orecchio);

            // Il collegamento via internet **dopo** «Ascolta insieme»: dice
            // agli altri dispositivi se la sessione è accesa. Spento di serie:
            // da spento il filo legge le impostazioni e basta.
            lontano::avvia_filo(app.handle());

            // I modelli di linguaggio: un lucchetto vuoto e un contatore, e
            // nessun filo. Qui non parte niente da solo — non c'e` niente da
            // riprendere e nessuna coda da svuotare — e finche' nessuno apre la
            // chat dello Studio questo stato non tocca ne' rete ne' disco.
            app.manage(ia::StatoIa::nuovo());

            // Gli aggiornamenti **dopo tutti gli altri**: il suo filo aspetta
            // che la finestra dica `pronto`, e poi dieci secondi. È l'unica
            // cosa che parla con la rete senza che nessuno gliel'abbia
            // chiesto, e lo fa una volta per avvio: vedi `aggiornamenti`.
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
            // Adesso che ci sono tutti e tre — il riquadro multimediale, i
            // pulsanti della miniatura, il vassoio — il brano della coda di
            // ieri anche a loro. Vedi `riproduzione::primo_stato_fuori`.
            riproduzione::primo_stato_fuori(app.handle());

            // Le cartelle sorvegliate **dopo** la libreria: legge la scelta e
            // l'elenco da lì, e il suo filo aspetta dieci secondi prima di
            // guardare il disco. Vedi `sorveglia`.
            sorveglia::avvia(app.handle());

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
        .register_asynchronous_uri_scheme_protocol("rymembra-cover", copertine::servi)
        // I caratteri e le immagini delle skin. Stessa forma, stesse ragioni:
        // un `@font-face` li chiede al motore come qualunque risorsa.
        .register_asynchronous_uri_scheme_protocol("rymembra-skin", skin::servi_risorsa)
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
            comandi::cartelle_trasloca,
            trascinati::smista_trascinati,
            comandi::brano_mostra_nella_cartella,
            comandi::brano_apri_pagina,
            comandi::cartelle_candidate,
            sorveglia::sorveglianza_stato,
            sorveglia::imposta_sorveglianza,
            comandi::imposta_cartella_download,
            comandi::cronologia,
            comandi::cronologia_conteggio,
            comandi::scansiona,
            comandi::annulla_scansione,
            comandi::scansione_avanzamento,
            comandi::rimozioni_conferma,
            comandi::rimozioni_scarta,
            cartelle::cartelle_figlie,
            cartelle::cartelle_brani,
            cartelle::cartelle_file,
            cartelle::cartelle_filtra,
            cartelle::cartella_mostra,
            cartelle::cartelle_radici_vive,
            comandi::cartelle_ui,
            comandi::imposta_cartelle_ui,
            comandi::cerca,
            comandi::cerca_conteggio,
            comandi::brani,
            viste::vista_brani_id,
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
            // Le due voci in fondo al menù del tasto destro: la riga se ne va, e
            // con la seconda anche il file. Vedi `crate::cancellazione`.
            cancellazione::brani_togli,
            cancellazione::brani_elimina,
            cancellazione::brani_su_rete,
            comandi::piano_importazione,
            comandi::importa,
            comandi::imposta_tema,
            comandi::movimento_ridotto,
            comandi::imposta_movimento_ridotto,
            comandi::imposta_lingua,
            comandi::imposta_scorciatoie,
            // Lo zoom della finestra. Tre gesti e nessun numero: la scala dei
            // gradini vive in `rymembra_app::preferenze::SCALA_ZOOM` e di elenchi
            // non ne esistono due — vedi il preambolo di `zoom.rs`.
            zoom::zoom_avvio,
            zoom::zoom_passo,
            zoom::zoom_normale,
            // Il giro guidato: qui c'è soltanto la memoria fra due avvii, il
            // giro vero è in `Giro.tsx`.
            giro::giro_da_fare,
            giro::giro_fatto,
            profilo::profilo_esporta,
            profilo::profilo_piano,
            profilo::profilo_importa,
            profilo::profilo_annulla,
            esplora::esplora_cerca,
            esplora::esplora_annulla,
            esplora::esplora_aggiungi,
            esplora::esplora_aggiungi_tutti,
            esplora::esplora_togli,
            esplora::esplora_identificativo,
            esplora::esplora_apri_pagina,
            esplora::esplora_nella_lista,
            esplora::esplora_tieni,
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
            metadati::metadati_conteggio,
            metadati::metadati_incerti,
            metadati::metadati_correggi,
            metadati::metadati_conferma,
            metadati::metadati_rileggi_tutti,
            playlist::playlist_elenco,
            playlist::playlist_brani,
            playlist::playlist_crea,
            playlist::playlist_rinomina,
            playlist::playlist_cancella,
            playlist::playlist_ripristina,
            playlist::playlist_aggiungi,
            playlist::playlist_crea_e_aggiungi,
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
            studio::studio_risorsa_aggiungi,
            studio::studio_risorsa_togli,
            studio::studio_migra,
            studio::studio_compila_app,
            studio::studio_crea,
            studio::studio_occupati,
            studio::studio_chat_leggi,
            studio::studio_chat_scrivi,
            studio::studio_tavolozza_da,
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
            riproduzione::normalizzazione_modo,
            riproduzione::normalizzazione_modo_scegli,
            riproduzione::normalizzazione_avanzamento,
            riproduzione::spegnimento,
            riproduzione::autoplay,
            riproduzione::dissolvenza,
            riproduzione::latenza,
            riproduzione::riapri_audio,
            insieme::insieme_stato,
            insieme::insieme_attiva,
            insieme::insieme_nome,
            insieme::insieme_firewall_visto,
            insieme::insieme_ospita,
            insieme::insieme_termina,
            insieme::insieme_esci,
            insieme::insieme_cerca,
            insieme::insieme_associa,
            insieme::insieme_codice,
            insieme::insieme_invita,
            insieme::insieme_rispondi,
            insieme::insieme_chiedi,
            insieme::insieme_ammetti,
            insieme::insieme_allontana,
            insieme::insieme_concedi,
            insieme::insieme_pari_rinomina,
            insieme::insieme_pari_dimentica,
            insieme::insieme_pari_blocca,
            insieme::insieme_indirizzi,
            insieme::insieme_calibra,
            lontano::lontano_stato,
            lontano::lontano_attiva,
            lontano::lontano_relay,
            lontano::lontano_dati_mobili,
            lontano::lontano_qr_crea,
            lontano::lontano_qr_annulla,
            lontano::lontano_collega,
            lontano::lontano_rispondi,
            lontano::lontano_rinomina,
            lontano::lontano_dimentica,
            lontano::sfoglia::lontano_album,
            lontano::sfoglia::lontano_album_artista,
            lontano::sfoglia::lontano_artisti,
            lontano::sfoglia::lontano_brani,
            lontano::sfoglia::lontano_brani_album,
            lontano::sfoglia::lontano_cerca,
            lontano::sfoglia::lontano_cerca_conteggio,
            lontano::sfoglia::lontano_suona,
            lontano::sfoglia::lontano_accoda,
            lontano::sfoglia::lontano_dopo,
            lontano::scarico::lontano_scarica,
            lontano::scarico::lontano_scarico_annulla,
            lontano::scarico::lontano_scarico_stato,
            lontano::lontano_link_in_attesa,
            lontano::lontano_link_esamina,
            lontano::lontano_collega_annulla,
            lontano::lontano_cerca_ora,
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
            riproduzione::formato_visibile,
            riproduzione::formato_visibile_scegli,
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
            riproduzione::coda_ripristina,
            riproduzione::coda_prepara,
            riproduzione::ripeti,
            riproduzione::mescola,
            riproduzione::brani_per_id,
            nuvola::nuvola_stato,
            nuvola::nuvola_collega,
            nuvola::nuvola_annulla,
            nuvola::nuvola_scollega,
            nuvola::nuvola_backup_app,
            nuvola::nuvola_attiva,
            nuvola::nuvola_salva,
            nuvola::nuvola_piano_ripristino,
            nuvola::nuvola_ripristina,
            nuvola_musica::nuvola_servizi,
            nuvola_musica::nuvola_conti,
            nuvola_musica::nuvola_conto_collega,
            nuvola_musica::nuvola_conto_annulla,
            nuvola_musica::nuvola_conto_scollega,
            nuvola_musica::nuvola_cartelle,
            nuvola_musica::nuvola_aggiungi,
            nuvola_musica::nuvola_aggiorna,
            nuvola_musica::nuvola_cache_stato,
            nuvola_musica::nuvola_cache_imposta,
            nuvola_musica::nuvola_cache_svuota,
            nuvola_musica::brano_apri_sul_web,
            nuvola_app::nuvola_app,
            nuvola_app::nuvola_app_salva,
            nuvola_app::nuvola_app_togli,
            sincronia::sincronia_stato,
            sincronia::sincronia_attiva,
            sincronia::sincronia_adesso,
            sincronia::sincronia_magazzino,
            sincronia::sincronia_dispositivi,
            sincronia::sincronia_accoppia,
            sincronia::sincronia_rinomina,
            sincronia::sincronia_dimentica,
            testi::testo_brano,
            testi::testo_scarto,
            testi::testo_cerca,
            testi::testo_cerca_di_nuovo,
            testi::testo_candidati,
            testi::testo_scegli,
            testi::testo_rifiuta,
            testi::testi_pannello,
            testi::testi_stato,
            testi::testi_rete,
            testi::testi_accanto,
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
            aggiornamenti::aggiornamenti_annulla,
            stato::fili_salute,
            diario::diario_apri,
            diario::diario_annota,
            manutenzione::manutenzione_stato,
            manutenzione::manutenzione_salute,
            manutenzione::manutenzione_pulisci_copertine,
            comandi::apri_documento,
            comandi::apri_guida,
            // Solo sul telefono: altrove rispondono `internal.aborted`. Vedi
            // `telefono`.
            telefono::telefono_permessi,
            telefono::telefono_chiedi_permessi,
            telefono::telefono_chiedi_tutti_i_file,
            telefono::telefono_cartella_musica,
            telefono::telefono_scegli_cartella,
            telefono::telefono_apri_documento,
            telefono::telefono_salva_documento,
            telefono::telefono_file_temporaneo,
            telefono::telefono_secondo_piano,
            telefono::telefono_riquadri,
            telefono::telefono_vibra,
            telefono::telefono_barre,
            telefono::telefono_scansiona_qr,
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
    // Il `Result` di `build` copre solo i guasti di prima del `setup` — che in
    // Tauri 2 gira dentro `App::run`, all'evento `Ready`, dopo aver creato le
    // finestre. Un `setup` che fallisce, compresa la WebView2 che non si crea,
    // non arriva qui: diventa un panico «Failed to setup app», e lo scrive il
    // gancio. In tutti e due i casi chi ha aperto Rymembra vede un dialogo
    // (`diario::mostra_l_avvio_fallito`), e la riga resta su disco. Il dialogo
    // è quello nativo di `rfd`, non quello di `tauri-plugin-dialog`: il ciclo
    // degli eventi, che quello vuole, a quel punto non c'è o sta cadendo.
    match costruita {
        Ok(app) => app.run(|mano, evento| {
            // Sul telefono, a ogni giro: se l'attività è morta e rinata, le si
            // ridà la finestra. Vedi `telefono::FINESTRA_PERSA`.
            #[cfg(mobile)]
            if matches!(evento, tauri::RunEvent::MainEventsCleared) {
                telefono::riapri_la_finestra(mano);
                return;
            }

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

            // Su Android la finestra che si chiude è l'attività — tolta dalle
            // recenti, o chiusa dal sistema per memoria — e non l'applicazione:
            // la musica sta nel servizio in primo piano, e deve continuare con lo
            // schermo spento e senza attività. Un'uscita chiesta da qui
            // spegnerebbe il lettore e farebbe cadere il processo, e il sistema
            // riavvierebbe il servizio in un processo vuoto. Quando chiudere
            // davvero lo decide Android, uccidendo il processo; un'uscita chiesta
            // dal codice (con un codice d'uscita) resta un'uscita. Quando
            // l'attività torna, la finestra rinasce: vedi `telefono`.
            #[cfg(mobile)]
            {
                if chiusura {
                    return;
                }
                if let tauri::RunEvent::ExitRequested {
                    code: None, api, ..
                } = &evento
                {
                    api.prevent_exit();
                    telefono::finestra_persa();
                    nota!("[uscita] l'attività si è chiusa: il lettore resta acceso");
                    return;
                }
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

            // La sequenza vera sta in `spegnimento::spegni`, perché la usa
            // anche l'aggiornamento, che esce da un'altra strada: vedi lì.
            spegnimento::spegni(mano);
        }),
        Err(err) => {
            // Su disco e non solo su `stderr`: questo è un ramo in cui la
            // finestra non è mai comparsa, e chi guarda non ha nient'altro da
            // leggere. Il `setup` qui non è mai partito, quindi il diario è
            // chiuso: la riga va nel file di riserva (`diario::RISERVA`). Poi
            // il dialogo, che dice dove sta la riga e aspetta che lo si
            // chiuda prima dell'uscita.
            let scritta = diario::nota_di_emergenza(format_args!(
                "[avvio] Rymembra non è riuscito ad avviarsi: {err}"
            ));
            diario::mostra_l_avvio_fallito(&diario::testo_dell_avvio_fallito(
                &err.to_string(),
                scritta.as_deref(),
            ));
            std::process::exit(1);
        }
    }
}

/// Registra il guardiano dell'istanza unica, se in questo avvio ha senso.
///
/// # Che problema risolve, e non è quello che sembra
///
/// Non è il doppio controllo degli aggiornamenti, che costerebbe trecento byte e
/// nessun danno. Sono due cose che fanno male davvero:
///
/// 1. **Due processi sullo stesso database.** SQLite in WAL regge due scrittori,
///    ma Rymembra non è scritta per questo: `Stato` tiene la libreria dietro un
///    mutex *di processo*, e due processi hanno due mutex che non si vedono. Due
///    scansioni in parallelo sulle stesse righe, due `rebuild_aggregates`, due
///    code di ieri riprese insieme — e la cronologia d'ascolto è l'unica cosa in
///    tutto il programma che una riscansione non sa ricostruire.
/// 2. **Due icone nel vassoio**, di cui una riapre una finestra che non sta
///    suonando. Chi chiude con la X e poi riapre Rymembra dal collegamento
///    ottiene, oggi, un secondo processo muto accanto a quello che suona — e il
///    gesto che voleva fare era «rimettimi davanti la finestra».
///
/// Il secondo è esattamente quel che fa il callback: [`vassoio::mostra`], che è
/// già la risposta giusta e fa tutti e tre i gesti — mostra, de-riduce, dà il
/// fuoco. Nessun controllo aggiornamenti in più e nessun altro effetto: il filo
/// del primo processo batte anche a finestra nascosta, quindi non c'è niente da
/// risvegliare.
///
/// # Perché una cartella scelta a mano e non `debug_assertions`
///
/// Perché la ragione di questo guardiano è **una cartella dati condivisa**, e
/// una variabile d'ambiente è l'unico modo che esiste in questo programma per
/// averne due diverse (vedi [`cartella_dati`]). Le variabili sono due:
/// `RYMEMBRA_DATI`, e [`eredita::VARIABILE_DATI`], quella della versione
/// precedente, che il programma rispetta ancora e che fa lo stesso mestiere. Chi
/// la teneva impostata per separare un'istanza di prova da quella vera non deve
/// ritrovarsi, da un aggiornamento all'altro, con un guardiano che gli impedisce
/// di aprirle insieme.
///
/// Il criterio è uno solo, e sta in [`dati_a_mano`]: lo stesso con cui
/// [`cartella_dati`] decide se la cartella è di serie. Due Rymembra su due
/// cartelle dati non sono due processi sullo stesso database: sono la prova che
/// la scorciatoia di sviluppo esiste per fare, cioè la libreria vera da una
/// parte e quella di prova dall'altra, aperte insieme per confrontarle.
///
/// `#[cfg(not(debug_assertions))]` sarebbe stato più corto e avrebbe sbagliato
/// due volte: avrebbe **bloccato** le due cartelle dati in rilascio, che è il
/// caso legittimo, e avrebbe reso il guardiano invisibile in sviluppo — cioè
/// esattamente dove si prova l'updater con `npm run dev`, e dove un secondo
/// `npm run dev` deve poter partire. Una funzione che non gira mai in sviluppo è
/// una funzione che si scopre rotta dall'utente.
///
/// Il prezzo è dichiarato: chi tiene una delle due variabili impostata di suo —
/// in sviluppo, o perché i dati stanno su un altro disco — non ha il guardiano, e
/// due avvii gli danno due processi come oggi. È il baratto giusto fra i due
/// possibili, perché chi ha impostato quella variabile sa di averla impostata.
///
/// Il plugin, comunque, non distingue le cartelle da sé: la sua chiave è
/// `config().identifier`, identico nei due processi. Senza questa riga sarebbe
/// il comportamento più invadente dei due.
///
/// # Dove muore il secondo processo, che è la parte che conta
///
/// Dentro il `setup` del plugin, cioè **prima** di [`cartella_dati`] e di tutto
/// quel che il `setup` qui sotto apre: il secondo processo non arriva mai a
/// toccare il database, quindi non può lasciare un `-wal` da riassorbire né
/// mettere mano alla cronologia d'ascolto. Né a spostare la cartella della
/// versione precedente: [`prepara_i_dati`] viene dopo, e due processi che
/// traslocano insieme gli stessi file sono il guasto che il trasloco, con la
/// sua sonda, esiste per non avere. E non si vede niente lampeggiare, perché la
/// finestra nasce nascosta (`visible: false` in `tauri.conf.json`) e a mostrarla
/// è il frontend, che in quel processo non parte.
#[cfg(desktop)]
fn una_sola_istanza(costruttore: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    let (nuova, vecchia) = variabili_dei_dati();
    if dati_a_mano(nuova, vecchia) {
        return costruttore;
    }
    costruttore.plugin(tauri_plugin_single_instance::init(
        |app, _argomenti, _da| {
            // Gli argomenti e la cartella del secondo processo non servono: Rymembra
            // non si apre con un file sulla riga di comando, e non c'è niente da
            // consegnare. Quel che c'era da fare è riportare davanti la finestra.
            vassoio::mostra(app);
        },
    ))
}

/// Il nome della variabile d'ambiente con cui si sceglie a mano la cartella dati:
/// quello di [`trasloco::VARIABILE_DATI`], che ne nomina il rifiuto.
const VARIABILE_DATI: &str = trasloco::VARIABILE_DATI;

/// Le due variabili che possono dettare la cartella dati, nell'ordine in cui
/// valgono: quella di adesso e quella della versione precedente.
///
/// È l'unico punto che le legge dall'ambiente del processo. Chi decide —
/// [`cartella_dati`], [`una_sola_istanza`] — le riceve come valori, ed è per
/// questo che la decisione si prova senza toccare l'ambiente.
fn variabili_dei_dati() -> (Option<OsString>, Option<OsString>) {
    (
        std::env::var_os(VARIABILE_DATI),
        std::env::var_os(eredita::VARIABILE_DATI),
    )
}

/// Una delle due variabili detta la cartella dati?
///
/// La risposta è quella di [`trasloco::scegli_cartella`] — una variabile vuota è
/// come se non ci fosse — e non una sua copia: il guardiano dell'istanza unica e
/// la scelta della cartella devono andare d'accordo, e due modi di leggere
/// «impostata» sono due modi di non andarci.
fn dati_a_mano(nuova: Option<OsString>, vecchia: Option<OsString>) -> bool {
    fatta_a_mano(&trasloco::scegli_cartella(nuova, vecchia, PathBuf::new()))
}

/// Se la scelta della cartella dati l'ha fatta una variabile.
///
/// Anche quando la variabile la indica relativa e la scelta è un rifiuto: chi
/// l'ha impostata stava provando un'istanza a parte, e quel che non deve
/// toccare — il guardiano dell'istanza unica della libreria vera, la sua cache —
/// resta da non toccare.
fn fatta_a_mano(scelta: &Result<trasloco::Scelta, trasloco::Errore>) -> bool {
    scelta.as_ref().map_or(true, |scelta| scelta.personalizzata)
}

/// La scelta della cartella dati, chiedendo quella di serie solo se serve.
///
/// Con una variabile impostata la cartella di serie non si calcola nemmeno,
/// com'è sempre stato: `di_serie` può fallire, e un guasto lì non deve fermare
/// chi la cartella l'ha già indicata a mano.
///
/// Il risultato di fuori è il guasto di `di_serie`, che ferma l'avvio; quello di
/// dentro è la scelta, o il suo rifiuto ([`trasloco::Errore::CartellaRelativa`]),
/// che l'avvio dice con un dialogo ([`rifiuta`]).
fn scegli_la_cartella<E>(
    nuova: Option<OsString>,
    vecchia: Option<OsString>,
    di_serie: impl FnOnce() -> Result<PathBuf, E>,
) -> Result<Result<trasloco::Scelta, trasloco::Errore>, E> {
    let predefinita = if dati_a_mano(nuova.clone(), vecchia.clone()) {
        PathBuf::new()
    } else {
        di_serie()?
    };
    Ok(trasloco::scegli_cartella(nuova, vecchia, predefinita))
}

/// Se la cartella dati di questo avvio l'ha scelta una variabile d'ambiente
/// (o se la variabile c'era, e la cartella è stata rifiutata): la risposta di
/// [`fatta_a_mano`], registrata nel `setup` per chi deve saperlo dopo.
///
/// # Perché uno stato e non un'altra lettura dell'ambiente
///
/// Perché la cache dei brani del cloud e quella delle copertine lontane la
/// leggevano da sé, con `var_os("RYMEMBRA_DATI").is_some()`: una regola che
/// non vedeva la variabile della versione precedente (e la cache finiva
/// condivisa con l'istanza vera, che può girare insieme) e contava come
/// impostata una variabile vuota (e la cache finiva nel profilo roaming). La
/// lettura è una, [`variabili_dei_dati`], e chi ne ha bisogno ne riceve la
/// risposta.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DatiAMano(pub(crate) bool);

/// Dove sta una cache che non deve viaggiare col profilo roaming: la cartella
/// `nome` in `%LOCALAPPDATA%\<identificatore>`, oppure dentro la cartella dei
/// dati se l'ha scelta una variabile ([`DatiAMano`]), così due istanze non si
/// dividono la stessa cache.
///
/// `None` se la cartella l'ha scelta una variabile e la libreria non si è
/// aperta (il rifiuto di una cartella relativa): la cartella locale è quella
/// dell'istanza vera, che può girare insieme, e una cache aperta da qui
/// poterebbe la sua. Senza nessuna delle due cartelle, quella temporanea.
pub(crate) fn cartella_di_cache(app: &tauri::AppHandle, nome: &str) -> Option<PathBuf> {
    let a_mano = app.try_state::<DatiAMano>().is_some_and(|dati| dati.0);
    let dati = app
        .try_state::<stato::Stato>()
        .and_then(|stato| stato.cartella_dati().map(std::path::Path::to_path_buf));
    let locale = app.path().app_local_data_dir().ok();
    dove_sta_la_cache(a_mano, dati.as_deref(), locale.as_deref(), nome)
}

/// La regola di [`cartella_di_cache`], senza Tauri.
fn dove_sta_la_cache(
    a_mano: bool,
    dati: Option<&std::path::Path>,
    locale: Option<&std::path::Path>,
    nome: &str,
) -> Option<PathBuf> {
    match (a_mano, dati, locale) {
        (true, Some(dati), _) | (false, Some(dati), None) => Some(dati.join(nome)),
        (false, _, Some(locale)) => Some(locale.join(nome)),
        (true, None, _) => None,
        (false, None, None) => Some(std::env::temp_dir().join(format!("rymembra-{nome}"))),
    }
}

/// Dove stanno database, copertine e skin, e come ci si è arrivati.
///
/// Normalmente la cartella dati dell'applicazione, che su Windows è
/// `%APPDATA%\io.github.federicobaratti.rymembra`. `RYMEMBRA_DATI` la sostituisce.
/// La sostituisce anche la variabile della versione precedente,
/// [`eredita::VARIABILE_DATI`], se la nuova non c'è o è vuota: chi la teneva
/// impostata continua a vederla rispettata. La precedenza e il resto sono di
/// [`trasloco::scegli_cartella`]; qui si leggono solo le variabili.
///
/// Torna la [`trasloco::Scelta`] intera, e non il solo percorso, perché da come
/// ci si è arrivati dipende il trasloco dei dati di prima: lo si cerca accanto
/// alla cartella di serie, e mai accanto a una scelta a mano.
///
/// # Perché una scorciatoia del genere esiste
///
/// Perché quasi tutto ciò che questa applicazione fa di irreversibile —
/// importare un account intero, ripristinare un backup, riscrivere i tag di
/// migliaia di file — si può leggere in una prova unitaria e si può giudicare **solo**
/// guardandolo succedere in una finestra vera, su una libreria che somiglia a
/// quella di qualcuno. Senza questa variabile le due cose sono la stessa
/// libreria: la sola, quella dell'utente. Provare un'importazione vorrebbe dire
/// scrivere sulla sua cronologia d'ascolto, che è — con voti e preferiti —
/// l'unica cosa in tutto il programma che una scansione non sa ricostruire.
///
/// Con questa, si copia il database in una cartella qualunque, si lancia
/// `RYMEMBRA_DATI=... rymembra.exe` e si rompe pure tutto: la libreria vera non sa
/// nemmeno che è successo.
///
/// Le variabili si leggono **solo qui**, all'avvio, e da nessun'altra parte:
/// quello che il resto del programma vede è un `PathBuf` e basta, come prima, e
/// [`DatiAMano`] per chi deve sapere se la cartella è quella di serie.
///
/// Una variabile che indica una cartella relativa dà il rifiuto di dentro
/// ([`trasloco::Errore::CartellaRelativa`]): l'avvio lo dice e si chiude, senza
/// ripiegare sulla cartella di serie, che è la libreria vera di chi stava
/// facendo una prova.
fn cartella_dati(
    app: &tauri::AppHandle,
) -> Result<Result<trasloco::Scelta, trasloco::Errore>, tauri::Error> {
    let (nuova, vecchia) = variabili_dei_dati();
    scegli_la_cartella(nuova, vecchia, || app.path().app_data_dir())
}

/// Porta nella cartella dati quella della versione precedente, poi apre il diario.
///
/// # L'ordine non si cambia
///
/// Il trasloco sta **prima** del diario, della libreria e di tutto ciò che crea
/// un file nella cartella nuova: se la cartella di prima deve arrivare intera,
/// lo può fare solo finché quella nuova non esiste (o è vuota), e il diario, il
/// database e le copertine bastano a farla esistere. Il diario viene subito
/// dopo, e racconta lì cos'è successo: una riga per ogni cosa spostata o
/// rinominata, e per ogni conflitto lasciato com'era.
///
/// # Quando dice di no
///
/// `Err` vuol dire che il trasloco non è andato avanti — la versione di prima è
/// ancora aperta, o una cartella non si lascia spostare — e che la cartella nuova
/// **non** va toccata: niente diario, niente database, niente copertine. A
/// dirlo ci pensa [`rifiuta`]: il motivo nel file di riserva, perché il diario è
/// chiuso e non si apre, e un dialogo per chi guarda.
fn prepara_i_dati(scelta: &trasloco::Scelta) -> Result<(), trasloco::Errore> {
    let rapporto = trasloco::trasloca(&trasloco::percorsi_per(scelta))?;
    diario::apri(&scelta.cartella);
    if scelta.da_variabile_precedente {
        nota!(
            "[avvio] cartella dati dettata da {}, la variabile della versione precedente: {}",
            eredita::VARIABILE_DATI,
            scelta.cartella.display()
        );
    }
    for riga in trasloco::righe_di_diario(&rapporto) {
        nota!("{riga}");
    }
    Ok(())
}

/// Il motivo di un rifiuto, dove resta: il diario a quel punto è chiuso — si
/// apre solo dopo un trasloco riuscito — e una riga di [`nota!`] non andava da
/// nessuna parte. Il file di riserva esiste per questo: quel che succede prima
/// che il diario si apra (`diario::nota_di_emergenza`). Di un «non parte»
/// restava solo il testo del dialogo, se qualcuno lo ricopiava.
fn annota_il_rifiuto(errore: &trasloco::Errore) {
    diario::nota_di_emergenza(format_args!(
        "[avvio] cartella dati rifiutata, Rymembra si chiude: {errore}"
    ));
}

/// Il trasloco ha detto di no, o la variabile dei dati indica una cartella
/// relativa: lo si dice all'utente, e lo stato non apre niente.
///
/// # Che cosa ottiene chi guarda
///
/// Un dialogo di errore con il testo bilingue di
/// [`trasloco::messaggio_di_rifiuto`], e l'uscita quando lo si chiude. Il testo
/// nomina la versione di prima perché l'utente capisca che cosa chiudere, e dice
/// che nessun dato è stato cancellato: non che non sia stato toccato niente, perché
/// un rifiuto può arrivare a metà di una fusione, con qualche voce già passata.
///
/// # Perché il dialogo non blocca
///
/// Perché il `setup` gira sul filo della finestra, e `blocking_show` aspetta che
/// quel filo recapiti il dialogo: si aspetterebbero a vicenda. `show` lo
/// consegna al ciclo degli eventi e ritorna subito; il dialogo compare quando
/// il `setup` ha finito, e la callback — che gira su un filo suo — esce dal
/// processo. `exit` e non `app.exit`: a quel punto non c'è niente da spegnere in
/// ordine, e la sequenza di [`spegnimento`] presuppone una libreria.
///
/// # Lo stato che torna
///
/// [`stato::Stato::rifiutato`]: la libreria «non aperta» che l'applicazione sa già
/// trattare, con `internal.unexpected` perché il rifiuto non ha una frase sua
/// nel catalogo — la causa porta il motivo vero per il diario e per chi legge
/// l'errore a schermo, e il dialogo ha già detto il resto. Fino all'uscita ogni
/// comando che chiede la libreria riceve quell'errore, e nessuno apre la cartella
/// nuova.
///
/// # Il portachiavi
///
/// Non solo i file: anche le voci del Credential Manager della versione di prima
/// non si toccano. Il `setup` continua dopo il rifiuto, e il filo del backup
/// legge l'account di Google dal portachiavi mezzo minuto dopo l'avvio; con il
/// travaso acceso lo porterebbe dal servizio di prima a quello nuovo togliendolo
/// alla versione di prima, che è aperta e in quel momento lo usa. Per questo la
/// prima cosa che si fa, prima ancora del dialogo, è sospendere il travaso: da lì
/// `DiSistema` lavora solo sul servizio nuovo.
fn rifiuta(app: &tauri::AppHandle, errore: &trasloco::Errore) -> stato::Stato {
    // La versione di prima è aperta, o comunque i suoi file sono in uso, e le sue
    // voci del portachiavi sono ancora le sue: il rifiuto serve a lasciarle
    // intatte, e il dialogo qui sotto promette che nessun dato è stato
    // cancellato. Per prima cosa, perché il `setup` prosegue e avvia i fili che
    // arrivano al portachiavi.
    rymembra_oauth::portachiavi::sospendi_il_travaso();
    // Il motivo su disco **prima** del dialogo, che si chiude con l'uscita.
    annota_il_rifiuto(errore);
    app.dialog()
        .message(trasloco::messaggio_di_rifiuto(errore))
        .kind(MessageDialogKind::Error)
        .title("Rymembra")
        .show(|_| std::process::exit(1));
    stato::Stato::rifiutato(
        AppError::new(ErrorCode::InternalUnexpected {
            detail: Some("cartella dati rifiutata all'avvio".to_owned()),
        })
        .with_cause(errore.to_string()),
    )
}

#[cfg(test)]
mod prove {
    use super::*;
    use std::cell::Cell;

    fn os(testo: &str) -> Option<OsString> {
        Some(OsString::from(testo))
    }

    /// Una cartella assoluta su ogni sistema: quella temporanea, più un nome.
    /// Non si crea niente.
    fn assoluta(nome: &str) -> PathBuf {
        std::env::temp_dir().join(nome)
    }

    fn os_assoluta(nome: &str) -> Option<OsString> {
        Some(assoluta(nome).into_os_string())
    }

    #[test]
    fn senza_variabili_la_cartella_e_quella_di_serie_e_il_guardiano_resta() {
        assert!(!dati_a_mano(None, None));
        let scelta = scegli_la_cartella(None, None, || Ok::<_, ()>(PathBuf::from("di-serie")))
            .expect("la cartella di serie c'è")
            .expect("una scelta");
        assert_eq!(scelta.cartella, PathBuf::from("di-serie"));
        assert!(!scelta.personalizzata);
        assert!(!scelta.da_variabile_precedente);
    }

    #[test]
    fn una_delle_due_variabili_basta_a_togliere_il_guardiano() {
        // La nuova, quella di prima, tutte e due: in tutti e tre i casi la
        // cartella è scelta a mano, e Rymembra non registra l'istanza unica.
        assert!(dati_a_mano(os("nuova"), None));
        assert!(dati_a_mano(None, os("vecchia")));
        assert!(dati_a_mano(os("nuova"), os("vecchia")));
    }

    #[test]
    fn una_variabile_vuota_e_come_se_non_ci_fosse() {
        assert!(!dati_a_mano(os(""), None));
        assert!(!dati_a_mano(None, os("")));
        assert!(!dati_a_mano(os(""), os("")));
        // Una vuota non cancella l'altra.
        assert!(dati_a_mano(os(""), os("vecchia")));
        assert!(dati_a_mano(os("nuova"), os("")));
    }

    #[test]
    fn la_cartella_di_serie_si_chiede_solo_se_nessuna_variabile_la_sostituisce() {
        let chiamate = Cell::new(0_u32);
        let di_serie = || {
            chiamate.set(chiamate.get() + 1);
            Ok::<_, ()>(PathBuf::from("di-serie"))
        };

        let scelta = scegli_la_cartella(os_assoluta("a-mano"), None, di_serie)
            .expect("scelta")
            .expect("valida");
        assert_eq!(scelta.cartella, assoluta("a-mano"));
        assert!(scelta.personalizzata);
        let scelta = scegli_la_cartella(None, os_assoluta("di-prima"), di_serie)
            .expect("scelta")
            .expect("valida");
        assert_eq!(scelta.cartella, assoluta("di-prima"));
        assert!(scelta.da_variabile_precedente);
        // Nemmeno per rifiutare una cartella relativa.
        assert!(matches!(
            scegli_la_cartella(os("relativa"), None, di_serie),
            Ok(Err(trasloco::Errore::CartellaRelativa { .. }))
        ));
        assert_eq!(chiamate.get(), 0, "la cartella di serie non serviva");

        let scelta = scegli_la_cartella(os(""), os(""), di_serie)
            .expect("scelta")
            .expect("valida");
        assert_eq!(scelta.cartella, PathBuf::from("di-serie"));
        assert_eq!(chiamate.get(), 1, "serviva, e una volta sola");
    }

    #[test]
    fn un_guasto_della_cartella_di_serie_ferma_solo_chi_non_ha_scelto() {
        let guasto = || Err::<PathBuf, _>("la cartella dati non si risolve");
        // Nessuna variabile: la cartella di serie serve, e il guasto arriva.
        assert_eq!(
            scegli_la_cartella(None, None, guasto).expect_err("niente cartella"),
            "la cartella dati non si risolve"
        );
        // Con una variabile il guasto non viene nemmeno incontrato.
        let scelta = scegli_la_cartella(os_assoluta("a-mano"), None, guasto)
            .expect("scelta")
            .expect("valida");
        assert_eq!(scelta.cartella, assoluta("a-mano"));
    }

    #[test]
    fn la_variabile_di_adesso_vince_su_quella_di_prima() {
        let scelta = scegli_la_cartella(os_assoluta("nuova"), os_assoluta("vecchia"), || {
            Ok::<_, ()>(PathBuf::new())
        })
        .expect("scelta")
        .expect("valida");
        assert_eq!(scelta.cartella, assoluta("nuova"));
        assert!(!scelta.da_variabile_precedente);
    }

    /// Una variabile che indica una cartella relativa resta una scelta a mano:
    /// il guardiano dell'istanza unica non c'è (il rifiuto si deve vedere, non
    /// finire nella finestra dell'istanza vera), e la cache non va in quella
    /// dell'istanza di serie.
    #[test]
    fn una_cartella_relativa_resta_una_scelta_a_mano() {
        assert!(dati_a_mano(os("relativa"), None));
        assert!(dati_a_mano(None, os("relativa")));
    }

    /// Dove sta una cache: dentro i dati se la cartella l'ha scelta una
    /// variabile, nella cartella locale se no; e **da nessuna parte** se la
    /// cartella era a mano e la libreria non si è aperta, perché la cartella
    /// locale è quella dell'istanza vera, che può girare insieme.
    #[test]
    fn la_cache_non_si_divide_con_l_istanza_vera() {
        let dati = assoluta("dati");
        let locale = assoluta("locale");
        let dove = |a_mano, dati, locale| dove_sta_la_cache(a_mano, dati, locale, "cache");
        assert_eq!(
            dove(true, Some(dati.as_path()), Some(locale.as_path())),
            Some(dati.join("cache"))
        );
        assert_eq!(
            dove(false, Some(dati.as_path()), Some(locale.as_path())),
            Some(locale.join("cache"))
        );
        assert_eq!(dove(true, None, Some(locale.as_path())), None);
        assert_eq!(dove(true, None, None), None);
        assert_eq!(
            dove(false, Some(dati.as_path()), None),
            Some(dati.join("cache"))
        );
        assert_eq!(
            dove(false, None, Some(locale.as_path())),
            Some(locale.join("cache"))
        );
        assert_eq!(
            dove(false, None, None),
            Some(std::env::temp_dir().join("rymembra-cache"))
        );
    }

    /// Il rifiuto arriva a diario chiuso — il diario si apre solo dopo un
    /// trasloco riuscito — e il dialogo si chiude con l'uscita. Senza una riga
    /// nel file di riserva, di un «non parte» restava solo il testo del
    /// dialogo, se qualcuno lo ricopiava.
    #[test]
    fn il_rifiuto_del_trasloco_resta_nella_riserva() {
        let errore = trasloco::Errore::InUso {
            percorso: PathBuf::from("dati-finti").join("aether.db"),
        };
        let riserva = diario::a_diario_chiuso(|| annota_il_rifiuto(&errore));
        assert!(
            riserva.contains("rifiutat") && riserva.contains(&errore.to_string()),
            "il motivo del rifiuto non è da nessuna parte: {riserva:?}"
        );
    }

    /// E il motivo lo scrive [`rifiuta`], che è la strada di ogni rifiuto
    /// all'avvio — il trasloco e la cartella relativa — **prima** del dialogo,
    /// che si chiude con l'uscita.
    #[test]
    fn ogni_rifiuto_all_avvio_lascia_il_motivo_prima_del_dialogo() {
        let corpo = include_str!("lib.rs")
            .split("fn rifiuta(")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        let riga = corpo.find("annota_il_rifiuto(");
        let dialogo = corpo.find(".dialog()");
        assert!(
            riga.is_some() && riga < dialogo,
            "il rifiuto si mostra senza lasciare il motivo su disco"
        );
    }

    /// Le due variabili dei dati si leggono dall'ambiente in un posto solo,
    /// [`variabili_dei_dati`]: chi altro deve sapere se la cartella è stata
    /// scelta a mano lo riceve da lì. Due letture con due regole avevano dato
    /// una cache condivisa fra due istanze (con la variabile di prima
    /// impostata) e una cache di centinaia di megabyte nel profilo roaming (con
    /// quella di adesso impostata ma vuota).
    #[test]
    fn la_variabile_dei_dati_si_legge_in_un_posto_solo() {
        fn sorgenti(cartella: &std::path::Path, tutti: &mut Vec<(PathBuf, String)>) {
            let Ok(voci) = std::fs::read_dir(cartella) else {
                return;
            };
            for voce in voci.flatten() {
                let percorso = voce.path();
                if percorso.is_dir() {
                    sorgenti(&percorso, tutti);
                } else if percorso.extension().is_some_and(|e| e == "rs")
                    && let Ok(testo) = std::fs::read_to_string(&percorso)
                {
                    tutti.push((percorso, testo));
                }
            }
        }
        let mut tutti = Vec::new();
        sorgenti(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut tutti,
        );
        assert!(tutti.len() > 50, "i sorgenti non si sono trovati");
        let mut letture = Vec::new();
        for (percorso, testo) in &tutti {
            for (dove, _) in testo.match_indices("env::var") {
                let accanto: String = testo
                    .get(dove..)
                    .unwrap_or_default()
                    .chars()
                    .take(48)
                    .collect();
                if accanto.contains("_DATI") {
                    letture.push(format!("{}: {accanto}", percorso.display()));
                }
            }
        }
        let corpo = include_str!("lib.rs")
            .split("fn variabili_dei_dati()")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        assert_eq!(
            letture.len(),
            2,
            "le variabili dei dati si leggono anche fuori da `variabili_dei_dati`: {letture:#?}"
        );
        assert!(
            letture.iter().all(|l| l.contains("lib.rs"))
                && corpo.matches("env::var_os(").count() == 2,
            "{letture:#?}"
        );
    }

    #[test]
    fn il_nome_della_variabile_di_adesso_non_cambia() {
        // Il nome della variabile è un'interfaccia per chi lancia il programma:
        // le prove di sviluppo e la documentazione lo usano per esteso.
        assert_eq!(VARIABILE_DATI, "RYMEMBRA_DATI");
        assert_ne!(VARIABILE_DATI, eredita::VARIABILE_DATI);
    }
}
