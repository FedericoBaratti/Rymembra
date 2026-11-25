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
// La riga di comando: l'avvio automatico e il link `rymembra://collega/…`. Solo
// sul computer: il telefono non ha una riga di comando.
#[cfg(desktop)]
mod argomenti;
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
mod avvio_con_windows;
mod barra;
mod cancellazione;
mod cartelle;
mod chiusura;
mod comandi;
// Come compare la finestra, e come ricompare dopo la X sul vassoio.
#[cfg(desktop)]
mod comparsa;
mod copertine;
mod corsia;
mod destinazioni;
mod diario;
mod disparte;
mod errore;
mod esplora;
#[cfg(desktop)]
mod geometria;
mod giro;
mod ia;
mod importa;
mod insieme;
mod lontano;
mod maiusc;
mod manutenzione;
mod media;
mod metadati;
mod novita;
mod nucleo;
mod nuvola;
mod nuvola_app;
mod nuvola_musica;
mod pacchetto;
mod pagina;
// Gli eventi una tantum che a finestra distrutta non hanno una pagina (S4-5).
mod perso;
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

#[cfg(target_os = "android")]
use nucleo::GuardiaDellAvvio;
use nucleo::{ChiAvvia, Esito, Nucleo};
use rymembra_app::trasloco;
use rymembra_domain::eredita;
use rymembra_domain::errors::{AppError, ErrorCode};
use spegnimento::Emette as _;
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
///
/// Sul computer la comparsa passa da `comparsa::mostra`: lanciata da Windows al
/// login la finestra compare ridotta a icona, e questa è una delle tre strade
/// che la mostrano — con [`finestra_mostra`] e la rete di sicurezza.
#[tauri::command]
fn pronto(finestra: tauri::WebviewWindow) {
    #[cfg(desktop)]
    comparsa::mostra(&finestra);
    #[cfg(mobile)]
    let _ = finestra.show();
    aggiornamenti::finestra_pronta(finestra.app_handle());
}

/// Mostra la finestra e basta, senza aspettare skin e tema.
///
/// La chiama l'attesa dell'apertura (`AttesaDellaLibreria.tsx`) quando la libreria ci mette
/// più di un attimo: chi aspetta una migrazione deve vedere che Rymembra sta
/// lavorando. Non è [`pronto`], che dà anche il via al controllo degli
/// aggiornamenti: quello vuole la libreria, e parte quando la finestra vera è
/// pronta.
#[tauri::command]
fn finestra_mostra(finestra: tauri::WebviewWindow) {
    #[cfg(desktop)]
    comparsa::mostra(&finestra);
    #[cfg(mobile)]
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
///
/// La X della pagina ha già aspettato lo Studio (`studio/primaDiChiudere.ts`):
/// la sua chiusura non si rinvia una seconda volta. Vedi `chiusura.rs`.
#[tauri::command]
fn finestra_chiudi(finestra: tauri::Window) {
    chiusura::CHIUSURA.concedi();
    let _ = finestra.close();
}

/// Apre la libreria su un filo suo, e quando è aperta fa il resto dell'avvio
/// sul filo della finestra.
///
/// # Perché fuori dal `setup`
///
/// Il `setup` gira sul filo della finestra prima che il ciclo degli eventi
/// parta: finché non ritorna non si disegna niente, e la rete di sicurezza che
/// mostra la finestra parte alla sua fine. Lì dentro una migrazione lunga — la
/// copia di prima, gli schemi nuovi su una libreria da un gigabyte, un disco
/// lento — era un processo invisibile per secondi o minuti, e un secondo
/// doppio clic finiva all'istanza unica, ferma sullo stesso filo (R06b-5).
/// Adesso il `setup` ritorna subito, la finestra compare e dice «Apro la
/// libreria…» o «Aggiorno la libreria…» (`AttesaDellaLibreria.tsx`), e il lavoro lungo
/// lo fa il filo `rymembra-apertura`.
///
/// # L'ordine resta quello di prima
///
/// Il trasloco dei dati di prima e il diario restano nel `setup`, prima di
/// tutto. Poi, su questo filo: la libreria — la copia di prima delle
/// migrazioni, le migrazioni, l'ottimizzazione — e la riga di avvio. Poi, sul
/// filo della finestra come sempre, [`avvia_dopo_la_libreria`]: le cartelle
/// dal cloud, il lettore, i fili di sottofondo, il vassoio, nello stesso ordine
/// e sullo stesso filo di quando stavano nel `setup`. Solo alla fine il
/// cancello si apre e la finestra monta l'applicazione: gli stati che i comandi
/// cercano ci sono tutti, come quando il `setup` era finito.
///
/// # Se qualcosa va storto
///
/// Un panico nell'apertura è un guasto della libreria, non un filo morto con
/// la finestra ferma su «Apro la libreria…» per sempre. Un filo che non parte
/// apre la libreria qui, sul filo della finestra, com'era.
fn apri_e_avvia(app: &tauri::AppHandle, data_dir: PathBuf) {
    let mano = app.clone();
    let cartella = data_dir.clone();
    let avviato = std::thread::Builder::new()
        .name("rymembra-apertura".to_owned())
        .spawn(move || {
            let Some(stato) = mano.try_state::<stato::Stato>() else {
                return;
            };
            apri_la_libreria(&Nucleo::di(&mano), &stato, cartella);
            // Chi ha chiuso Rymembra mentre la libreria si apriva non vuole
            // il resto dell'avvio: vedi [`finisci_l_avvio`].
            if spegnimento::in_uscita() {
                return;
            }
            // Il resto sul filo della finestra, dove stava: i controlli
            // multimediali e i pulsanti della miniatura si appendono alla
            // finestra, e l'ordine fra i passi è quello di sempre.
            let dopo = mano.clone();
            if let Err(err) = mano.run_on_main_thread(move || finisci_l_avvio(&dopo)) {
                nota!("[avvio] il filo della finestra non risponde, l'avvio si ferma: {err}");
            }
        });
    if let Err(err) = avviato {
        nota!("[avvio] il filo dell'apertura non parte, la libreria si apre qui: {err}");
        if let Some(stato) = app.try_state::<stato::Stato>() {
            apri_la_libreria(&Nucleo::di(app), &stato, data_dir);
        }
        finisci_l_avvio(app);
    }
}

/// L'apertura vera: la fase da dire alla finestra, la libreria, la riga di
/// avvio. Un panico qui dentro diventa il guasto della libreria.
///
/// Prende il nucleo e non la finestra: la apre anche il servizio che parte
/// all'accensione ([`avvia_dal_servizio`]), dove una finestra non c'è, e
/// l'evento `libreria:stato` senza finestra non ha chi lo ascolti.
fn apri_la_libreria(app: &Nucleo, stato: &stato::Stato, data_dir: PathBuf) {
    let da = stato::versione_da_aggiornare(&data_dir.join(stato::NOME_DATABASE));
    if let Some(da) = da {
        stato.in_aggiornamento(da);
    }
    stato::riga_dell_apertura(da);
    app.emetti("libreria:stato", stato.fase());
    let aperta = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        stato::apri_libreria(data_dir)
    }))
    .unwrap_or_else(|panico| {
        let perche = panico
            .downcast_ref::<&str>()
            .map(|testo| (*testo).to_owned())
            .or_else(|| panico.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panico senza messaggio".to_owned());
        Err(AppError::new(ErrorCode::InternalUnexpected {
            detail: Some("apertura della libreria".to_owned()),
        })
        .with_cause(perche))
    });
    stato.completa(aperta);
    // **Prima** di disegnare, come sempre: se l'apertura è fallita è l'unica
    // traccia che resta quando la finestra mostra solo l'errore.
    stato::riga_di_avvio(stato);
}

/// Il resto dell'avvio, e poi il cancello: da qui la finestra monta
/// l'applicazione. Sul filo della finestra.
///
/// Non se si sta uscendo: chi chiude Rymembra durante una migrazione lunga
/// passa per `spegnimento::spegni` mentre questo è ancora in coda, e far
/// nascere adesso la scheda del sistema, i pulsanti della miniatura, l'icona
/// del vassoio e i fili di sottofondo vorrebbe dire toccare la finestra
/// mentre la si smonta — il modo in cui questa applicazione moriva, vedi
/// [`rete_di_sicurezza`].
fn finisci_l_avvio(app: &tauri::AppHandle) {
    if spegnimento::in_uscita() {
        return;
    }
    // Dov'era la finestra, prima che la pagina — che la mostra — monti: il
    // cancello si apre dopo. Vedi `comparsa`.
    #[cfg(desktop)]
    comparsa::ritrova_la_geometria(app);
    avvia_dopo_la_libreria(app);
    if let Some(stato) = app.try_state::<stato::Stato>() {
        let fase = stato.apri_il_cancello();
        app.emetti("libreria:stato", fase);
    }
}

/// Il resto dell'avvio: quel che vuole la libreria aperta, nell'ordine di sempre.
///
/// Tutto quel che il `setup` faceva dopo aver registrato la libreria: le
/// cartelle dal cloud, il lettore, i controlli multimediali, i fili di
/// sottofondo, il vassoio, la sorveglianza. Gira sul filo della finestra, come
/// quando stava nel `setup`: vedi [`apri_e_avvia`].
///
/// # Il nucleo e la finestra
///
/// Si divide in due: [`avvia_il_nucleo`], quel che non vuole una finestra e che
/// su Android parte anche da un servizio, prima di ogni attività; e
/// [`avvia_la_finestra`], quel che nasce con la finestra (il vassoio, il
/// backup, l'analisi, gli aggiornamenti…). Il nucleo parte **una volta per
/// processo**: se un servizio lo ha già avviato — l'attività è nata dopo — qui
/// si salta, e la finestra si aggancia a quello che c'è
/// (`Nucleo::e_gia_avviato`).
fn avvia_dopo_la_libreria(app: &tauri::AppHandle) {
    let nucleo = Nucleo::di(app);
    // Le cartelle dal cloud **subito dopo** la libreria: lo specchio si
    // carica da lì, e prima di ogni scansione — anche quella del lettore
    // che riapre la coda di ieri — chi apre un file deve già trovare il
    // cloud. I fili dei conti partono qui, e la loro prima domanda alla
    // rete non aspetta niente: è quel che rende una cartella appena
    // cambiata su OneDrive visibile all'avvio.
    nuvola_musica::avvia(app);

    // Lo stesso per i pulsanti nell'anteprima, e per una ragione in
    // più: il loro stato ricorda l'aspetto anche prima che i pulsanti
    // ci siano, e il primo stato del lettore arriva quando il dispositivo
    // si apre. Vedi `barra`.
    app.manage(barra::StatoBarra::nuovo());

    if !nucleo.e_gia_avviato() {
        avvia_il_nucleo(&nucleo);
        nucleo.avvio_finito();
        // Il nucleo qui è partito dalla finestra, anche se la libreria non si è aperta (come
        // sul computer: la pagina mostra l'errore). Sul telefono la notifica del sottofondo
        // non deve dire «si sta avviando» né «apri Rymembra» — è già aperta, e il processo non
        // rifà questo avvio fino alla prossima partenza: vedi [`avvisa_se_la_libreria_manca`].
        #[cfg(target_os = "android")]
        avvisa_se_la_libreria_manca(&nucleo);
    }
    avvia_la_finestra(app);
}

/// La libreria non c'è: lo stato non c'è, o non si è aperta.
///
/// Si legge dalla cartella dei dati, che lo stato scrive solo ad apertura riuscita.
#[cfg_attr(
    not(target_os = "android"),
    allow(
        dead_code,
        reason = "la usa solo il telefono, per dire a Kotlin che il nucleo parte senza libreria; le prove la provano"
    )
)]
fn senza_libreria(stato: Option<&stato::Stato>) -> bool {
    stato.is_none_or(|stato| stato.cartella_dati().is_none())
}

/// Un avvio dalla finestra che non ha una libreria aperta lo dice a Kotlin, **non riprovabile**:
/// la notifica del sottofondo dice «non è partita bene, chiudi l'app e riaprila» invece di
/// «si sta avviando» per sempre — la sessione multimediale, che la fa passare a «è attiva»,
/// non parte senza libreria — o di «apri Rymembra», che qui non serve a niente: il `setup`
/// gira una volta per processo, e riaprire un'attività nata dopo non riapre la libreria.
/// Chiudere l'app del tutto sì: il processo nuovo riparte da zero.
///
/// Solo se l'avvio qui è davvero della finestra: un avvio fallito del servizio, che la finestra
/// rifà, passa da qui alla prima uscita, e un secondo guasto è proprio questo caso.
#[cfg(target_os = "android")]
fn avvisa_se_la_libreria_manca(nucleo: &Nucleo) {
    if senza_libreria(nucleo.try_state::<stato::Stato>().as_ref()) {
        nota!("[avvio] il nucleo parte senza libreria: la notifica dice di chiudere l'app");
        dillo_a_kotlin(Esito::Rotto);
    }
}

/// Il nucleo: la libreria è già aperta, e qui parte quel che serve a Rymembra
/// per funzionare **senza una finestra** — il lettore con la coda di ieri e il
/// motore audio, i controlli del sistema (su Android la sessione per l'auto e le
/// cuffie), lo scrobbling, «Ascolta insieme» come stato inerte, il collegamento
/// con gli altri dispositivi.
///
/// Su Android lo avvia anche un servizio, all'accensione del telefono
/// (`avvia_dal_servizio`), prima che esista un'attività; sul computer parte dal
/// filo della finestra, dentro [`avvia_dopo_la_libreria`], come sempre. Quel che
/// non sta qui — il backup, la sincronia, l'arricchimento, la manutenzione,
/// l'analisi, la sorveglianza, gli aggiornamenti, il vassoio — è dell'attività:
/// accesi a ogni accensione del telefono sarebbero batteria e disco spesi senza
/// nessuno.
fn avvia_il_nucleo(nucleo: &Nucleo) {
    // Il lettore **dopo** la libreria. Il dispositivo audio non si apre qui
    // ma su un filo suo, [`riproduzione::apri_il_primo_motore`] più sotto,
    // che rimette anche la coda di ieri (R23-9).
    // Lo stato dei controlli multimediali **prima** del lettore, anche
    // se a riempirlo si farà più sotto. Il filo dell'orologio parte
    // insieme al lettore e chiama `manda_stato` da sé quando il
    // dispositivo audio sparisce: se quel momento arrivasse prima di
    // questa riga, cercherebbe uno stato non registrato. Registrarlo
    // vuoto costa un mutex e un intero.
    nucleo.manage(media::StatoMedia::nuovo());

    let (lettore, orecchio, orecchio_dispositivi) = riproduzione::StatoLettore::avvia(nucleo);
    riproduzione::riga_di_avvio_lettore(&lettore);
    nucleo.manage(lettore);
    // Il filo che apre il brano successivo, insieme agli altri del
    // lettore. L'ordine fra questi quattro non conta: il canale del
    // preparatore non ha limite, quindi una spinta che arrivasse prima
    // che il filo sia in piedi lo aspetta lì invece di perdersi.
    riproduzione::avvia_preparatore(nucleo.clone(), orecchio);
    // Lo stato del collegamento via internet prima della coda: la coda
    // di ieri può avere brani di un altro dispositivo, e le loro voci
    // si rifanno con lui. Il suo filo parte più sotto, dopo «Ascolta
    // insieme»; lo stato da solo non apre niente.
    nucleo.manage(lontano::StatoLontano::nuovo());
    riproduzione::avvia_orologio(nucleo.clone());
    // Sul telefono lo spettro non c'è (`InRiproduzione.tsx`,
    // `CON_SPETTRO`), e un filo che si sveglia quattro volte al secondo
    // per sentirsi dire che nessuno guarda è batteria spesa per niente,
    // per tutta la vita del processo — che su Android è lunga, perché
    // la musica continua anche a schermata chiusa.
    #[cfg(desktop)]
    riproduzione::avvia_spettro(nucleo.clone());
    // Il dispositivo audio, su un filo suo, e dopo il primo tentativo il
    // filo che guarda le uscite andare e venire. Ultimo dei quattro, e non
    // conta nemmeno qui: chi pungola il sorvegliante prima che sia in piedi
    // trova un canale che tiene la spinta finché non c'è nessuno a
    // raccoglierla. Vedi `riproduzione::apri_il_primo_motore`.
    riproduzione::apri_il_primo_motore(nucleo.clone(), orecchio_dispositivi);

    // I controlli veri **dopo** il lettore, e per una ragione che non è
    // l'ordine di dipendenza ma quello della finestra: su Windows le
    // SMTC si appendono a una finestra, e qui la finestra esiste già —
    // `tauri.conf.json` la crea nascosta, non assente. Serve anche la
    // libreria aperta, per sapere dove stanno le copertine. Il primo
    // stato le riempirà da sé, perché passa dallo stesso `manda_stato`
    // di tutti gli altri.
    media::avvia(nucleo);

    // Lo scrobbling **dopo** la libreria, perché la prima cosa che fa è
    // guardare se è rimasto qualcosa in coda dalla sessione precedente.
    // Il suo filo dorme finché non lo si chiama: a servizi scollegati
    // non costa niente.
    nucleo.manage(scrobble::StatoScrobble::nuovo());
    scrobble::avvia(nucleo);

    // «Ascolta insieme» **dopo** il lettore e la libreria: la sessione
    // legge le impostazioni da lì e parla col lettore attraverso il
    // ponte. Spento di serie, e da spento il suo filo dorme: nessun
    // pacchetto in rete finché qualcuno non accende l'interruttore. Lo
    // stato lo ha il nucleo, su tutte e due le piattaforme.
    nucleo.manage(insieme::StatoInsieme::nuovo());
    // Il filo della sessione: **sul computer parte da qui, prima del collegamento
    // via internet**, nell'ordine di sempre — un flusso che arriva da lontano
    // appena il collegamento è in piedi trova già chi lo legge. Sul telefono parte
    // con la finestra ([`avvia_la_finestra`]), perché una sessione accesa non deve far
    // entrare il telefono in una rete all'accensione, e il nucleo che parte da un
    // servizio non lo avvia: un flusso che arriva prima si chiude (vedi
    // `insieme::accogli_lontano`).
    #[cfg(desktop)]
    avvia_il_filo_di_insieme(nucleo);

    // Il collegamento via internet **dopo** «Ascolta insieme»: dice
    // agli altri dispositivi se la sessione è accesa. Spento di serie:
    // da spento il filo legge le impostazioni e basta.
    lontano::avvia_filo(nucleo);

    // Sul telefono la scheda del sistema riceve subito il brano della coda di
    // ieri: a nucleo che parte da solo non arriverà nessuna finestra a
    // chiederlo.
    #[cfg(mobile)]
    riproduzione::primo_stato_fuori(nucleo);
}

/// Fa partire il filo della sessione «Ascolta insieme», **una volta**: l'orecchio
/// con cui parte si prende una volta sola, e chi chiama dopo non trova niente da fare.
/// Così il computer lo avvia dal nucleo e il telefono dalla finestra, con la stessa
/// funzione.
fn avvia_il_filo_di_insieme(nucleo: &Nucleo) {
    if let Some(orecchio) = nucleo
        .try_state::<insieme::StatoInsieme>()
        .and_then(|stato| stato.prendi_l_orecchio())
    {
        insieme::avvia_filo(nucleo.clone(), orecchio);
    }
}

/// Quel che nasce con la finestra: i pulsanti nell'anteprima, i fili pesanti, gli
/// aggiornamenti, il vassoio, la sorveglianza. Sul filo della finestra.
fn avvia_la_finestra(app: &tauri::AppHandle) {
    let nucleo = Nucleo::di(app);
    // I pulsanti nell'anteprima, accanto ai controlli multimediali e per
    // la stessa ragione: si appendono alla finestra, che qui esiste già.
    // Compaiono quando la finestra si mostra.
    barra::avvia(app);

    // Il backup **per ultimo**: il suo filo aspetta mezzo minuto prima
    // della prima passata proprio per lasciar finire quel che parte
    // adesso, e non ha senso averlo in piedi prima che ci sia una
    // libreria da salvare.
    let (nuvola, orecchio) = nuvola::StatoNuvola::nuovo();
    app.manage(nuvola);
    nuvola::avvia_filo(app.clone(), orecchio);

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
    sincronia::avvia_filo(app.clone(), orecchio);

    let (arricchimento, orecchio) = arricchimento::StatoArricchimento::nuovo();
    app.manage(arricchimento);
    arricchimento::avvia_filo(app.clone(), orecchio);

    // La manutenzione del database: controllo, `optimize`, copertine
    // orfane. Il suo filo aspetta un minuto — vedi `manutenzione`.
    manutenzione::avvia(app);

    // L'analisi sonora **dopo** l'arricchimento, e il suo filo aspetta
    // due minuti. È l'unica cosa qui dentro che legge dal disco a tutta
    // velocità senza che nessuno l'abbia chiesta, e cede alla
    // riproduzione: se in quei due minuti parte una canzone, non
    // comincia affatto.
    let (analisi, orecchio) = analisi::StatoAnalisi::nuovo();
    app.manage(analisi);
    analisi::avvia_filo(app.clone(), orecchio);

    // «Ascolta insieme»: il filo della sessione. Lo stato lo ha già il nucleo; da
    // spento il filo dorme. Sul computer è già partito da [`avvia_il_nucleo`], e
    // qui non trova più niente da avviare.
    avvia_il_filo_di_insieme(&nucleo);
    // Una volta per avvio, **dopo** «Ascolta insieme» — che dice se c'è una
    // sessione, e allora non si tocca niente —: via quel che le versioni di prima
    // hanno lasciato in `copertine-lontane`. Su un filo suo. Sta con la finestra
    // perché dipende dal filo della sessione, che parte con lei.
    lontano::ripulisci_le_copertine_di_prima(&nucleo);

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
    aggiornamenti::avvia_filo(app.clone(), orecchio);

    // Il vassoio **dopo** la libreria, perché la prima cosa che fa è
    // leggere la preferenza da lì. Non compare ancora niente nell'area
    // di notifica: le due etichette del menù arrivano dalla finestra
    // qualche decina di millisecondi più tardi, e senza di quelle non
    // c'è un menù da costruire. Vedi `vassoio`.
    app.manage(vassoio::StatoVassoio::nuovo());
    vassoio::avvia(app);
    // Adesso che ci sono tutti e tre — il riquadro multimediale, i
    // pulsanti della miniatura, il vassoio — il brano della coda di
    // ieri anche a loro. Vedi `riproduzione::primo_stato_fuori`.
    #[cfg(desktop)]
    riproduzione::primo_stato_fuori(&nucleo);

    // Le cartelle sorvegliate **dopo** la libreria: legge la scelta e
    // l'elenco da lì, e il suo filo aspetta dieci secondi prima di
    // guardare il disco. Vedi `sorveglia`.
    sorveglia::avvia(app);
}

/// L'avvio del nucleo dalla finestra, da zero: le cartelle, il trasloco dei dati di
/// prima, la libreria, e il resto. È quel che il `setup` fa quando nessun servizio
/// ha avviato il nucleo — sempre, sul computer — e che la finestra rifà da sé
/// quando l'avvio di un servizio non è riuscito e non ha lasciato niente
/// ([`Esito::Fallito`], vedi [`rifai_l_avvio`]).
///
/// Per questo sa riaprire: se c'è lo stato di una libreria che non si era aperta, lo
/// riapre ([`registra_la_libreria`]) invece di farne un altro.
///
/// # Errori
///
/// Se Tauri non sa dire dove stanno i dati. Il `setup` lo propaga e l'avvio si ferma,
/// come sempre; [`rifai_l_avvio`] lo racconta e dice che il nucleo non parte.
fn avvia_da_capo(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let nucleo = Nucleo::di(app);
    let scelta = cartella_dati(app)?;
    // Se la cartella l'ha scelta una variabile, per chi tiene una
    // cache fuori dai dati: la risposta dell'unica lettura delle
    // variabili, e non una seconda lettura con una regola sua. Vedi
    // [`cartella_di_cache`]. Prima di ogni stato che apre una cache.
    let a_mano = fatta_a_mano(&scelta);
    nucleo.manage(DatiAMano(a_mano));
    // La copia del Store, la libreria che c'era prima: vedi
    // [`cartella_di_serie`].
    nucleo.manage(LibreriaDiPrima(libreria_di_prima(app, a_mano)));
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

    // Gli stati che non vogliono la libreria — l'importazione, l'esplorazione, l'account,
    // la coda dei download, i testi — prima di tutto il resto: inerti, una cella vuota e
    // un cliente HTTP che non ha aperto niente, e i comandi li cercano per tipo. La
    // richiesta di rete parte quando qualcuno incolla un link o apre una schermata, mai
    // all'avvio. Vedi [`registra_gli_stati_inerti`].
    registra_gli_stati_inerti(app);

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

    // La libreria **non** si apre qui: lo stato nasce «in apertura»,
    // si registra subito — i comandi lo cercano per tipo — e la
    // libreria si apre su un filo suo. Il resto dell'avvio segue
    // quando è aperta: vedi [`apri_e_avvia`].
    //
    // Dopo un rifiuto la libreria è l'errore che l'applicazione sa già
    // trattare — la stessa di un database che non si è aperto — e
    // non si apre niente: si creerebbero la cartella nuova, il database
    // e le copertine, proprio ciò che il rifiuto deve evitare. Lì il
    // resto dell'avvio parte subito, com'era.
    match esito {
        Ok(data_dir) => {
            // Il rilancio dopo «Installa ora» (S5-6): prima di qualunque cosa che
            // mostri la finestra — la rete di sicurezza, l'attesa di una migrazione
            // — o consegni il link degli argomenti. La libreria non serve: è un file.
            #[cfg(desktop)]
            comparsa::dopo_un_aggiornamento(&data_dir);
            registra_la_libreria(&nucleo, data_dir.clone());
            apri_e_avvia(app, data_dir);
        }
        Err(errore) => {
            let stato = rifiuta(app, &errore);
            // La riga di avvio va stampata **prima** di disegnare: è
            // l'unica traccia che resta quando la finestra mostra solo
            // l'errore.
            stato::riga_di_avvio(&stato);
            let mut da_aprire = None;
            if !nucleo.manage(stato) {
                // C'era già uno stato, di un avvio fallito prima: è quello che la
                // finestra e i comandi trovano, e va al guasto invece di restare «in
                // apertura» per sempre. Il dialogo del rifiuto chiude comunque il
                // programma. Prende il motivo di **questo** rifiuto — il guasto di prima
                // è un'altra storia — e ai fili del nucleo si apre il loro cancello; la
                // fase che vede la pagina, invece, qui sotto, dopo gli stati della finestra.
                if let Some(vecchio) = nucleo.try_state::<stato::Stato>() {
                    vecchio.completa(Err(guasto_del_rifiuto(&errore)));
                    vecchio.apri_il_cancello_dei_fili();
                    da_aprire = Some(vecchio);
                }
            }
            avvia_dopo_la_libreria(app);
            // Come `finisci_l_avvio`: la pagina arriva al guasto quando gli stati su cui
            // conta ci sono, o `finita("guasto")` la monterebbe prima di `StatoNuvola`.
            // Uno stato nato rifiutato nasce già finito, e non ha niente da aprire.
            if let Some(vecchio) = da_aprire {
                app.emetti("libreria:stato", vecchio.apri_il_cancello());
            }
        }
    }
    Ok(())
}

/// Lo stato della libreria che sta per aprirsi in `data_dir`, registrato nel nucleo.
///
/// Se un avvio di prima — quello di un servizio che non è riuscito ad aprirla — ha
/// lasciato il suo stato in guasto, **si riapre quello** ([`stato::Stato::riapri`]):
/// `Nucleo::manage` e quello di Tauri tengono il primo valore per tipo, e uno stato
/// nuovo sarebbe una libreria che nessun comando vede. È sicuro perché l'avvio fallito
/// non aveva aperto la libreria né fatto nascere un filo (vedi [`Esito::Fallito`]).
fn registra_la_libreria(nucleo: &Nucleo, data_dir: PathBuf) -> stato::Stato {
    if let Some(stato) = nucleo.try_state::<stato::Stato>() {
        stato.riapri();
        return stato;
    }
    let stato = stato::Stato::in_apertura(data_dir);
    nucleo.manage(stato.clone());
    stato
}

/// La finestra nasce a nucleo già partito (o che sta partendo): non lo avvia, si
/// aggancia.
///
/// Succede su Android quando un servizio ha avviato il nucleo — all'accensione del
/// telefono, o dopo un `kill` che il sistema ha riparato — prima che l'attività
/// esistesse: la libreria è già in mano al nucleo, il lettore suona già, il
/// collegamento con i PC è già in piedi. Qui si registrano gli stati che servono
/// solo alla finestra e che non vogliono i dati, e quando il nucleo ha chiuso il suo
/// avvio — subito, se l'ha già chiuso — si fa quel che l'esito dice
/// ([`segui_l_esito`]): se è riuscito, il resto sul filo della finestra, gli stessi
/// passi di [`finisci_l_avvio`], che salta il nucleo perché c'è.
///
/// La finestra compare e dice «Apro la libreria…» finché non ha finito il suo avvio
/// (`Stato::apri_il_cancello`): il cancello che il servizio ha già aperto è quello dei
/// fili del nucleo, non questo.
///
/// # Quando il `setup` ritorna
///
/// Subito, se il nucleo sta ancora partendo: il lavoro si accoda e lo fa il filo che
/// chiude l'avvio. **Non** subito se il nucleo ha già chiuso il suo: [`Nucleo::dopo_l_avvio`]
/// fa il lavoro sul posto, e `run_on_main_thread` chiamata dal filo principale non
/// mette niente in coda — `send_user_message` esegue il messaggio direttamente
/// (`tauri-runtime-wry` 2.12.0, `src/lib.rs:263-273`) —, quindi [`segui_l_esito`] gira
/// **dentro** il `setup`, e con essa [`finisci_l_aggancio`], fino a [`finisci_l_avvio`]
/// se la libreria è libera. Se è occupata si rimette in coda (con un filo) e il `setup`
/// ritorna. In ogni caso [`rete_di_sicurezza`] si arma solo dopo.
fn aggancia_al_nucleo(
    app: &tauri::AppHandle,
    nucleo: &Nucleo,
) -> Result<(), Box<dyn std::error::Error>> {
    registra_gli_stati_inerti(app);
    aspetta_il_nucleo(app.clone(), nucleo);
    rete_di_sicurezza(app);
    Ok(())
}

/// Gli stati della finestra che non vogliono i dati: inerti — una cella vuota, un
/// cliente HTTP che non ha aperto niente — e i comandi li cercano per tipo. Il
/// `setup` e l'aggancio li registrano allo stesso modo; chi li trova già non fa niente.
fn registra_gli_stati_inerti(app: &tauri::AppHandle) {
    app.manage(importa::StatoImport::nuovo());
    app.manage(esplora::StatoEsplora::nuovo());
    app.manage(account::StatoAccount::nuovo());
    app.manage(procura::StatoProcura::nuovo());
    app.manage(testi::StatoTesti::nuovo());
}

/// Quando l'avvio del nucleo si chiude, la finestra fa quel che l'esito dice, sul
/// suo filo: [`segui_l_esito`].
fn aspetta_il_nucleo(app: tauri::AppHandle, nucleo: &Nucleo) {
    nucleo.dopo_l_avvio(move |esito| {
        let sul_filo = app.clone();
        if let Err(err) = app.run_on_main_thread(move || segui_l_esito(&sul_filo, esito)) {
            nota!("[avvio] il filo della finestra non risponde, l'aggancio si ferma: {err}");
        }
    });
}

/// Cosa fa la finestra quando il nucleo che aspettava ha chiuso il suo avvio.
fn segui_l_esito(app: &tauri::AppHandle, esito: Esito) {
    match esito {
        Esito::Riuscito => finisci_l_aggancio(app),
        Esito::Fallito => rifai_l_avvio(app),
        Esito::Rotto => finisci_senza_nucleo(app),
    }
}

/// L'avvio di un servizio non è riuscito e non ha lasciato niente — la libreria non
/// si apriva —: la finestra lo rifà da sé, da zero, come se fosse la prima ad
/// arrivare ([`avvia_da_capo`]). Se il guasto c'è ancora (un disco pieno), la pagina
/// mostra l'errore della libreria come in un avvio normale.
///
/// L'avvio lo chiede di nuovo a chi lo dà ([`Nucleo::reclama_l_avvio`]): se nel
/// frattempo un secondo tentativo del servizio l'ha già preso, si aspetta il suo esito
/// invece di avviare due volte la libreria.
fn rifai_l_avvio(app: &tauri::AppHandle) {
    let nucleo = Nucleo::di(app);
    if nucleo.reclama_l_avvio() != ChiAvvia::Tu {
        aspetta_il_nucleo(app.clone(), &nucleo);
        return;
    }
    nota!("[avvio] il nucleo del servizio non è partito: la finestra lo avvia da capo");
    if let Err(err) = avvia_da_capo(app) {
        nota!("[avvio] il nucleo non parte, i dati non si trovano: {err}");
        // Niente è nato: per Rust un secondo tentativo, da dove sia, può ancora riuscire.
        nucleo.avvio_chiuso(Esito::Fallito);
        // Ma la notifica del sottofondo — che dopo l'avvio fallito del servizio dice «a
        // riposo, apri Rymembra» — lo dice a Kotlin **non riprovabile**, come
        // [`avvisa_se_la_libreria_manca`]: la finestra c'è già, e riaprirla non rifà
        // niente. Nessuno può chiedere un altro avvio: il `setup` gira una volta per
        // processo, e `Ponte.avviaIlNucleo` non rientra dopo la prima attività
        // (`attivitaGiaNata`). Chiudere l'app del tutto, invece, sì. Dopo
        // `avvio_chiuso`, perché Kotlin può rientrare subito.
        #[cfg(target_os = "android")]
        dillo_a_kotlin(Esito::Rotto);
        finisci_senza_nucleo(app);
    }
}

/// Il nucleo non c'è, o c'è a metà: la pagina **non monta**, e dice che l'avvio non è
/// andato (`FaseLibreria::NonParte`). Montare l'applicazione su un nucleo senza il
/// lettore, o senza lo scrobbling, vorrebbe dire comandi che rispondono
/// `internal.aborted` uno dopo l'altro senza una frase che spieghi perché.
fn finisci_senza_nucleo(app: &tauri::AppHandle) {
    let nucleo = Nucleo::di(app);
    let stato = nucleo.try_state::<stato::Stato>().unwrap_or_else(|| {
        // Nessuno ha provato nemmeno ad aprire la libreria: serve comunque uno stato
        // che dia la fase alla pagina (`libreria_fase`).
        let stato = stato::Stato::rifiutato(
            AppError::new(ErrorCode::InternalUnexpected {
                detail: Some("il nucleo non è partito".to_owned()),
            })
            .with_cause("l'avvio del nucleo non è andato a buon fine"),
        );
        nucleo.manage(stato.clone());
        stato
    });
    nota!("[avvio] il nucleo non è partito intero: la pagina non monta");
    nucleo.emetti("libreria:stato", stato.non_parte());
}

/// Quante volte, e ogni quanto, l'aggancio aspetta che la libreria si liberi prima di
/// leggerla dal filo della finestra: dieci secondi in tutto. Vedi [`finisci_l_aggancio`].
const ATTESE_DELLA_LIBRERIA: u32 = 100;

/// Ogni quanto l'aggancio riguarda se la libreria è libera.
const PASSO_DELLA_LIBRERIA: std::time::Duration = std::time::Duration::from_millis(100);

/// Conviene riprovare fra poco, invece di leggere la libreria adesso dal filo della
/// finestra? Sì se è occupata e il tetto delle attese non è finito.
const fn da_riprovare_piu_tardi(libreria_libera: bool, tentativo: u32) -> bool {
    !libreria_libera && tentativo < ATTESE_DELLA_LIBRERIA
}

/// Il resto dell'aggancio, sul filo della finestra, a nucleo partito: l'indice delle
/// cartelle, che vuole la cartella dei dati, e poi [`finisci_l_avvio`].
///
/// # La libreria occupata
///
/// Il nucleo è già vivo, coi suoi fili che leggono e scrivono la libreria, e
/// [`finisci_l_avvio`] comincia con `nuvola_musica::avvia`, che la legge dal filo
/// della finestra: lì l'attesa ha un tetto di quattro secondi, e allo scadere i conti
/// del cloud resterebbero vuoti per tutta la vita del processo. Prima si guarda se la
/// libreria è libera, **senza aspettarla** — il filo della finestra non si ferma —, e
/// se non lo è si riprova fra poco, con una nuova richiesta al filo della finestra
/// (la pagina intanto dice «Apro la libreria…»). Dopo dieci secondi si va avanti
/// lo stesso, com'era.
fn finisci_l_aggancio(app: &tauri::AppHandle) {
    finisci_l_aggancio_al_tentativo(app, 0);
}

fn finisci_l_aggancio_al_tentativo(app: &tauri::AppHandle, tentativo: u32) {
    // Chi chiude Rymembra mentre si aspetta la libreria non vuole altri giri: ognuno è un
    // filo breve e una richiesta al filo della finestra, cioè toccare l'`AppHandle` mentre
    // lo si smonta (vedi [`finisci_l_avvio`], che per questo guarda lo stesso).
    if spegnimento::in_uscita() {
        return;
    }
    let nucleo = Nucleo::di(app);
    // La libreria c'è sempre: il nucleo è partito intero, e la libreria è il primo dei suoi passi.
    let Some(stato) = nucleo.try_state::<stato::Stato>() else {
        finisci_senza_nucleo(app);
        return;
    };
    if da_riprovare_piu_tardi(stato.e_libera(), tentativo) {
        let mano = app.clone();
        let avviato = std::thread::Builder::new()
            .name("rymembra-aggancio".to_owned())
            .spawn(move || {
                std::thread::sleep(PASSO_DELLA_LIBRERIA);
                // L'uscita può cominciare durante l'attesa: niente altra richiesta al filo
                // della finestra (come `rete_di_sicurezza`, dopo il suo sonno).
                if spegnimento::in_uscita() {
                    return;
                }
                let sul_filo = mano.clone();
                if let Err(err) = mano.run_on_main_thread(move || {
                    finisci_l_aggancio_al_tentativo(&sul_filo, tentativo + 1);
                }) {
                    nota!(
                        "[avvio] il filo della finestra non risponde, l'aggancio si ferma: {err}"
                    );
                }
            });
        if avviato.is_ok() {
            return;
        }
        nota!("[avvio] il filo dell'attesa non parte, si va avanti con la libreria occupata");
    }
    // La cartella c'è solo se la libreria si è aperta, e col nucleo partito lo è.
    if let Some(dati) = stato.cartella_dati() {
        app.manage(std::sync::Arc::new(cartelle::IndiceCartelle::nuovo(dati)));
    }
    finisci_l_avvio(app);
}

/// Il nucleo che parte da un servizio, senza attività: la funzione che
/// `Ponte.avviaNucleo` chiama da un filo suo (vedi `rymembra_android::jvm`).
///
/// Quel che fa il `setup` fino alla libreria, senza Tauri: il gancio dei panici,
/// il portachiavi e l'uscita audio ([`inizia_il_processo`]); le cartelle, dalla
/// stessa funzione della finestra ([`cartella_dati_da`], col posto di serie che
/// Kotlin dà); il diario e il trasloco dei dati di prima ([`prepara_i_dati`]); la
/// libreria; e poi [`avvia_il_nucleo`]. Alla fine dice come è andata
/// ([`GuardiaDellAvvio`]) e l'attività che nasce dopo ne segue l'esito
/// ([`aggancia_al_nucleo`]).
///
/// Il nucleo si avvia una volta per processo: se il `setup` di Tauri ha già
/// reclamato l'avvio — l'attività è nata mentre il servizio entrava in Rust — o c'è
/// già un nucleo, l'avvio non è suo, e qui non c'è niente da fare. Se un avvio di
/// prima è fallito senza lasciare niente ([`Esito::Fallito`]) invece **si rifà**: è
/// quel che succede quando Kotlin lo richiama (`Nucleo.sveglia`) e la libreria,
/// stavolta, si apre.
#[cfg(target_os = "android")]
fn avvia_dal_servizio(dati: PathBuf) {
    inizia_il_processo();
    let nucleo = Nucleo::del_processo();
    if nucleo.reclama_l_avvio() != ChiAvvia::Tu {
        nota!(
            "[avvio] il nucleo non lo avvia questo servizio: o lo avvia già l'attività, o c'è già"
        );
        return;
    }
    // **Subito** dopo aver reclamato, prima di ogni passo che può cadere: comunque
    // finisca questa funzione — anche con un panico — l'avvio si chiude con il suo
    // esito, e la finestra che nasce dopo e aspetta `dopo_l_avvio` non aspetta per
    // sempre.
    let mut fine = GuardiaDellAvvio::nuova(nucleo.clone(), dillo_a_kotlin);
    let scelta = match cartella_dati_da(|| Ok::<_, std::convert::Infallible>(dati)) {
        Ok(scelta) => scelta,
        Err(mai) => match mai {},
    };
    let a_mano = fatta_a_mano(&scelta);
    nucleo.manage(DatiAMano(a_mano));
    // Il Microsoft Store è del computer: sul telefono non c'è la libreria di prima.
    nucleo.manage(LibreriaDiPrima(None));
    match scelta.and_then(|scelta| prepara_i_dati(&scelta).map(|()| scelta.cartella)) {
        Ok(data_dir) => {
            let stato = registra_la_libreria(&nucleo, data_dir.clone());
            apri_la_libreria(&nucleo, &stato, data_dir);
            // Il cancello dei fili: i fili del nucleo lo aspettano per sapere che la
            // libreria è aperta (`Stato::aspetta_l_apertura`, da `con_libreria`), e qui
            // non c'è nessuna finestra che lo apra dopo: senza questa riga restano in
            // attesa per sempre. **Solo** quello: la fase che vede la pagina la apre la
            // finestra, quando ha anche i suoi stati (`Stato::apri_il_cancello`).
            stato.apri_il_cancello_dei_fili();
            if stato.cartella_dati().is_some() {
                // Da qui il nucleo comincia a far nascere fili e a registrare stati: un
                // panico non lascia più un avvio da rifare, ma un nucleo a metà.
                fine.la_libreria_c_e();
                avvia_il_nucleo(&nucleo);
                fine.riuscito();
            } else {
                // Senza libreria il nucleo non ha niente da servire. Non è nato nessun
                // filo: l'avvio è `Fallito`, e l'attività che nasce dopo lo rifà.
                nota!("[avvio] la libreria non si è aperta: il nucleo senza attività non parte");
            }
        }
        Err(errore) => annota_il_rifiuto(&errore),
    }
}

/// Dice a Kotlin che il nucleo avviato dal servizio non è partito, e se ha senso
/// riprovare: la notifica del sottofondo non dice «si sta avviando» per sempre, e
/// Kotlin sa se rilanciare l'avvio quando qualcuno lo sveglia.
#[cfg(target_os = "android")]
fn dillo_a_kotlin(esito: Esito) {
    if let Err(err) = rymembra_android::sistema::nucleo_non_parte(esito == Esito::Fallito) {
        nota!("[avvio] Kotlin non sa che il nucleo non è partito: {err}");
    }
}

// La porta da cui Kotlin entra in Rust per avviare il nucleo da un servizio: una
// macro di `rymembra-android`, perché la funzione JNI e il suo `unsafe` stanno
// in quel crate (`src/jvm.rs`), che questo non può nominare al contrario.
#[cfg(target_os = "android")]
rymembra_android::esporta_il_nucleo!(crate::avvia_dal_servizio);

/// Mostra la finestra fra due secondi, qualunque cosa succeda di là.
///
/// Salvo che di là si sia deciso di chiudere: due secondi sono lunghi
/// abbastanza perché qualcuno apra e richiuda subito, e far riapparire una
/// finestra che sta morendo vuol dire toccare l'`AppHandle` durante la
/// demolizione del ciclo degli eventi — che è esattamente il modo in cui questa
/// applicazione moriva. Vedi [`spegnimento`].
///
/// Una finestra ricreata dal vassoio la riprende: `comparsa::finestra_nata`.
pub(crate) fn rete_di_sicurezza(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(ATTESA_PRIMO_COLORE);
        if spegnimento::in_uscita() {
            return;
        }
        if let Some(finestra) = app.get_webview_window("main") {
            #[cfg(desktop)]
            comparsa::mostra(&finestra);
            #[cfg(mobile)]
            let _ = finestra.show();
        }
    });
}

/// Quel che il processo vuole prima di ogni altra cosa, **una volta sola**: il
/// gancio dei panici, il portachiavi e l'uscita audio.
///
/// La chiamano [`run`], quando parte l'applicazione, e — su Android — il servizio
/// che avvia il nucleo all'accensione ([`avvia_dal_servizio`]), che può arrivare
/// prima di ogni attività e dunque prima di `run`: la seconda chiamata non fa
/// niente. Il gancio, in particolare, non si installa due volte: ogni giro si
/// ricorderebbe di quello di prima e ogni panico finirebbe nel diario due volte.
fn inizia_il_processo() {
    static FATTO: std::sync::Once = std::sync::Once::new();
    FATTO.call_once(|| {
        diario::installa_gancio_dei_panici();
        // Il portachiavi prima di tutto il resto, come il gancio: su Android è il
        // Keystore, e `DiSistema` lo deve trovare già al suo posto quando il primo
        // filo — il backup, lo scrobbling — chiede un segreto. Altrove non fa niente.
        telefono::installa_il_portachiavi();
        // E l'uscita audio, per la stessa ragione: il motore si apre dopo.
        telefono::installa_l_uscita_audio();
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
    inizia_il_processo();
    // Come si è partiti, subito: la riga di comando — l'avvio automatico di
    // Windows, un link `rymembra://collega/…` cliccato — e, per la copia del
    // Store, che non passa argomenti, l'attivazione dell'attività di avvio. Quest'ultima
    // risponde solo la prima volta (vedi `rymembra_barra::avviata_dal_login_store`):
    // da chiamare qui, e una volta.
    #[cfg(desktop)]
    comparsa::registra_l_avvio(
        argomenti::Argomenti::del_processo(),
        dallo_store() && rymembra_barra::avviata_dal_login_store(),
    );

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
    // La pagina della finestra: che cosa può caricare (solo l'app, decisione
    // D-44), che cosa si spegne a ogni pagina nuova, e quando disegna. Su tutte
    // e due le piattaforme: vedi `pagina`.
    let costruttore = costruttore.plugin(pagina::plugin());
    let costruita = costruttore
        .setup(|app| {
            // Per prima cosa: il `setup` gira sul filo della finestra, ed è il
            // solo punto in cui lo si può riconoscere. Vedi
            // `stato::con_libreria`.
            stato::segna_filo_della_finestra();
            riproduzione::segna_filo_della_finestra();
            // Il nucleo del processo, agganciato alla finestra **prima** di ogni
            // altro stato: da qui gli stati che il nucleo possiede diventano anche
            // quelli di Tauri, e gli eventi partono verso la pagina. Vedi `nucleo`.
            let nucleo = Nucleo::di(app.handle());
            nucleo.aggancia(app.handle());
            // Un Alt da solo non deve mandare la finestra in un menu di sistema
            // invisibile (reperto 1 di FALT): la guardia si monta ora, e non con i
            // pulsanti della miniatura, che aspettano la libreria.
            barra::ferma_alt(app.handle());
            // Maiusc tenuto mentre ci si apre (D-48): l'ascolto parte subito,
            // perché la pressione che conta può cominciare prima della finestra.
            maiusc::avvia(app.handle());
            // Sul telefono la finestra la crea Rymembra, col gestore che nega i
            // permessi alla pagina: vedi `telefono::crea_la_finestra`. Subito,
            // dove prima la creava Tauri, perché quel che segue la trovi.
            #[cfg(target_os = "android")]
            telefono::crea_la_finestra(app.handle())?;
            // Il nucleo può essere già partito: su Android un servizio lo avvia
            // all'accensione del telefono, prima di ogni attività, e questa
            // nasce dopo. Un nucleo solo per processo: lo avvia chi lo reclama
            // per primo, e chi arriva dopo si aggancia a quello che c'è.
            if nucleo.reclama_l_avvio() != ChiAvvia::Tu {
                return aggancia_al_nucleo(app.handle(), &nucleo);
            }
            avvia_da_capo(app.handle())?;

            // Il link `rymembra://collega/…` che ha fatto partire il programma,
            // se c'è: resta in attesa finché la pagina non lo prende — non c'è
            // ancora — e passa dalla stessa conferma di quello arrivato col
            // programma già aperto. Vedi `argomenti`.
            #[cfg(desktop)]
            if let Some(link) = comparsa::avvio().link.as_deref()
                && !comparsa::e_un_rilancio()
            {
                lontano::link_arrivato(app.handle(), link);
            }

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
            // Dov'è la finestra, prima di tutto il resto — anche se si sta uscendo
            // —: è l'ultimo momento in cui c'è qualcosa da leggere, e la X col
            // vassoio la distrugge. Vedi `comparsa`.
            #[cfg(desktop)]
            comparsa::alla_chiusura(finestra.app_handle());
            // Chi sta uscendo davvero passa di qui: «Esci» dal vassoio alza la
            // bandiera prima di chiamare `exit`, e impedirgli la chiusura
            // vorrebbe dire un programma che non si spegne più.
            if spegnimento::in_uscita() {
                return;
            }
            let mano = finestra.app_handle();
            // La X col vassoio chiede al sistema se l'icona c'è **adesso**: un'icona
            // che la shell ha rifiutato non è una via di ritorno (S4-1). Sul telefono
            // il vassoio non c'è.
            #[cfg(desktop)]
            let nasconde = vassoio::nasconde_alla_x(mano);
            #[cfg(mobile)]
            let nasconde = vassoio::nasconde(mano);
            match chiusura::alla_richiesta(mano, nasconde) {
                chiusura::Decisione::Passa => {}
                // Alt+F4 o la barra delle applicazioni con lo Studio aperto:
                // la pagina scrive prima, e la chiusura la rifà `chiusura.rs`
                // quando ha scritto — o dopo il suo tetto (R14-7).
                chiusura::Decisione::Rinvia(_) | chiusura::Decisione::Aspetta => {
                    api.prevent_close();
                }
                // Il vassoio c'è: la finestra **si chiude davvero**, con la sua
                // WebView2 — non si impedisce e non si nasconde — e il programma
                // resta: vedi `RunEvent::ExitRequested` più sotto, che alla
                // fine dell'ultima finestra non esce. «Mostra» la ricrea
                // (`comparsa`).
                chiusura::Decisione::Nascondi => {
                    // Il QR del collegamento non resta valido nel vassoio, dove
                    // nessuno lo vede (decisione D-30).
                    lontano::finestra_nascosta(mano);
                    // Una proposta che non ha avuto risposta vale «Più tardi» (D5).
                    avvio_con_windows::proposta_ignorata();
                }
            }
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
            finestra_mostra,
            maiusc::maiusc_all_avvio,
            stato::libreria_fase,
            finestra_trascina,
            finestra_riduci,
            finestra_ingrandisci,
            finestra_ingrandita,
            finestra_schermo_intero,
            finestra_chiudi,
            chiusura::chiusura_da_scrivere,
            chiusura::chiusura_scritto,
            vassoio::secondo_piano,
            vassoio::secondo_piano_attiva,
            vassoio::vassoio_lingua,
            perso::riproduzione_errore_perso,
            avvio_con_windows::avvio_con_windows_stato,
            avvio_con_windows::avvio_con_windows_imposta,
            avvio_con_windows::proposta_sottofondo_da_fare,
            avvio_con_windows::proposta_sottofondo_rispondi,
            avvio_con_windows::proposta_sottofondo_rimanda,
            novita::novita_da_mostrare,
            novita::novita_viste,
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
            comandi::brani_artista,
            comandi::partecipazioni,
            comandi::partecipazioni_conta,
            comandi::artista,
            comandi::artista_righe,
            comandi::righe_scelte,
            comandi::artisti,
            comandi::conteggi,
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
            procura::riprova_introvabili,
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
            studio::studio_lettere,
            studio::studio_lettere_imposta,
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
            lontano::lontano_sottofondo,
            lontano::lontano_parti_con_auto,
            lontano::lontano_qr_crea,
            lontano::lontano_qr_annulla,
            lontano::lontano_collega,
            lontano::lontano_rispondi,
            lontano::lontano_rinomina,
            lontano::lontano_dimentica,
            lontano::lontano_specchio_lasciati,
            lontano::lontano_specchio_lascia,
            lontano::specchio::lontano_dimentica_anteprima,
            lontano::specchio::lontano_specchio_conferma,
            lontano::specchio::lontano_specchio_ora,
            lontano::specchio::lontano_specchio_stato,
            lontano::scarico::lontano_scarica_anteprima,
            lontano::scarico::lontano_scarica_righe,
            lontano::scarico::lontano_scarico_annulla,
            lontano::scarico::lontano_scarico_stato,
            lontano::lontano_link_in_attesa,
            lontano::lontano_link_dialogo,
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
            riproduzione::riproduzione_coda,
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
            testi::testo_copia,
            testi::testo_ripristina,
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
            arricchimento::arricchimento_fonte,
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
            manutenzione::manutenzione_apri_cartella_dati,
            manutenzione::manutenzione_copia_di_prima,
            manutenzione::manutenzione_torna_alla_copia,
            manutenzione::store_libreria_di_prima,
            manutenzione::store_copia_libreria_di_prima,
            manutenzione::store_rifiuta_libreria_di_prima,
            comandi::apri_documento,
            comandi::documento_locale,
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
            telefono::telefono_senza_risparmio_batteria,
            telefono::telefono_apri_risparmio_batteria,
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

            // La finestra si è spostata o ha cambiato misura: si segna e basta, e si
            // rilegge a eventi finiti (`MainEventsCleared`, qui sotto). Leggere
            // adesso vedrebbe una finestra ingrandita ancora «non ingrandita»:
            // vedi `comparsa::finestra_mossa`.
            #[cfg(desktop)]
            if let tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_),
                ..
            } = &evento
                && label == "main"
            {
                comparsa::finestra_mossa();
                return;
            }
            #[cfg(desktop)]
            if matches!(evento, tauri::RunEvent::MainEventsCleared) {
                comparsa::rileggi_se_mossa(mano);
                return;
            }

            // La finestra è morta — la X sul vassoio, e il motore resta — e con
            // lei la pagina: quel che la pagina aveva acceso (lo spettro, i
            // pannelli dei testi) non lo spegne più nessuno, e va spento da qui.
            // Non uscendo: chi esce spegne tutto con `spegnimento::spegni`.
            #[cfg(desktop)]
            if let tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::Destroyed,
                ..
            } = &evento
                && label == "main"
                && !spegnimento::in_uscita()
            {
                pagina::pagina_finita(mano);
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

            // La fine dell'ultima finestra non è la fine del programma, se il
            // vassoio c'è: con la X sul vassoio la finestra **si chiude**
            // (`on_window_event`), e a quel punto Tauri chiede di uscire
            // (`ExitRequested` senza codice). Si impedisce: restano il motore,
            // l'icona e le scorciatoie multimediali, e «Mostra» ricrea la
            // finestra (`comparsa`). `spegnimento::spegni` parte solo da
            // «Esci» — che alza la bandiera ed esce con un codice — e da un
            // vassoio che non c'è, dove la finestra era l'unica via di ritorno.
            #[cfg(desktop)]
            if let tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } = &evento
                && !spegnimento::in_uscita()
                && vassoio::nasconde(mano)
            {
                api.prevent_exit();
                nota!("[uscita] la finestra si è chiusa: il motore resta, nel vassoio");
                return;
            }

            // Con il secondo piano acceso, però, una chiusura non è un'uscita:
            // la finestra si chiude e il programma resta. Alzare qui la
            // bandiera darebbe il guasto peggiore dei
            // due possibili — musica che continua mentre si ferma tutto il
            // resto, perché `spegnimento` non torna mai indietro: orologio,
            // spettro, sorveglianza dei dispositivi, posizione di ripresa,
            // scrobbling e scheda del sistema, spenti per sempre dentro un
            // programma vivo.
            //
            // Lo stesso per una chiusura rinviata ad aspettare lo Studio: la
            // finestra resta aperta finché la pagina non ha scritto, e
            // spegnere adesso fermerebbe tutto sotto di lei (`chiusura.rs`).
            if chiusura
                && !spegnimento::in_uscita()
                && (vassoio::nasconde(mano) || crate::chiusura::CHIUSURA.rinviata())
            {
                return;
            }

            // Una chiusura di una persona — la X senza vassoio, Alt+F4 — si segna
            // **prima** di spegnere: l'aggiornamento che aspetta la chiusura parte
            // solo se è passata di qui, o da «Esci» (`chiusura::esci_adesso`). Quando
            // Windows spegne il computer arriva `Exit` da solo, senza nessuna delle
            // due, e un installer lanciato in quel momento verrebbe ucciso a metà.
            #[cfg(desktop)]
            if chiusura {
                aggiornamenti::uscita_voluta();
            }

            // La sequenza vera sta in `spegnimento::spegni`, perché la usa
            // anche l'aggiornamento, che esce da un'altra strada: vedi lì.
            spegnimento::spegni(mano);

            // E solo dopo aver spento tutto, e solo sull'uscita vera: il pacchetto
            // verificato che aspettava si installa, senza riaprire Rymembra.
            #[cfg(desktop)]
            if matches!(evento, tauri::RunEvent::Exit) {
                aggiornamenti::alla_chiusura(mano);
            }
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
/// fuoco — e, se la finestra non c'è perché la X sul vassoio l'ha chiusa, la
/// ricrea. Nessun controllo aggiornamenti in più e nessun altro effetto: i fili
/// del primo processo battono anche senza finestra, quindi non c'è niente da
/// risvegliare. Fanno eccezione due righe di comando — l'avvio automatico di
/// Windows, che non alza niente, e un link `rymembra://collega/…`, che si
/// consegna prima: vedi `argomenti`.
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
    costruttore.plugin(tauri_plugin_single_instance::init(|app, argomenti, _da| {
        // Della riga di comando del secondo processo contano due cose sole:
        // `--avvio-automatico` — l'avvio di Windows su un programma che c'è
        // già, che **non** alza nessuna finestra — e un link
        // `rymembra://collega/…`, che si consegna e poi alza la finestra.
        // Tutto il resto è riportare davanti la finestra, ricreandola se il
        // vassoio l'aveva chiusa. La cartella del secondo processo non serve.
        comparsa::secondo_avvio(app, &argomenti);
    }))
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
pub(crate) fn cartella_di_cache(app: &impl nucleo::ComeNucleo, nome: &str) -> Option<PathBuf> {
    let nucleo = app.come_nucleo();
    let a_mano = nucleo.try_state::<DatiAMano>().is_some_and(|dati| dati.0);
    let dati = nucleo
        .try_state::<stato::Stato>()
        .and_then(|stato| stato.cartella_dati().map(std::path::Path::to_path_buf));
    // Della copia del Microsoft Store, per la copia del Store: vedi
    // [`cartella_di_serie`].
    //
    // Senza la finestra — il nucleo che parte da un servizio, su Android — non c'è
    // chi risolva il percorso locale, e non serve: lì la cartella locale e quella
    // dei dati sono la stessa (`Context.getDataDir()`, per tutte e due:
    // `tauri-2.12.0/mobile/android/.../PathPlugin.kt`), e `dove_sta_la_cache`
    // risponde `dati/nome` come risponderebbe `locale/nome`.
    let locale = nucleo
        .finestra()
        .and_then(|app| app.path().app_local_data_dir().ok())
        .map(|base| cartella_di_serie(base, dallo_store()));
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
    cartella_dati_da(|| app.path().app_data_dir())
}

/// [`cartella_dati`] dato il posto di serie, da chi lo sa: **l'unica** funzione
/// che calcola le cartelle, per tutti e due i modi in cui il nucleo parte — la
/// finestra, che lo chiede a Tauri, e il servizio di Android, che non ha Tauri e
/// lo riceve da Kotlin (`Context.getDataDir()`, lo stesso posto che Tauri dà su
/// Android: `PathPlugin.getDataDir`). Le variabili, la copia del Store e il
/// rifiuto sono quelli di sempre.
fn cartella_dati_da<E>(
    base: impl FnOnce() -> Result<PathBuf, E>,
) -> Result<Result<trasloco::Scelta, trasloco::Errore>, E> {
    let (nuova, vecchia) = variabili_dei_dati();
    let store = dallo_store();
    scegli_la_cartella(nuova, vecchia, || {
        base().map(|base| cartella_di_serie(base, store))
    })
}

/// Questa copia è quella del Microsoft Store?
///
/// Lo dice il marcatore accanto all'eseguibile (`aggiornamenti::gestita_dallo_store`).
/// Sul telefono mai: il Store è del computer, e lì `aggiornamenti` è il modulo
/// di `mobile/`, che non ha marcatori da cercare.
fn dallo_store() -> bool {
    #[cfg(desktop)]
    let store = aggiornamenti::gestita_dallo_store();
    #[cfg(not(desktop))]
    let store = false;
    store
}

/// Il suffisso della cartella dati, e della cache locale, della copia del
/// Microsoft Store.
const SUFFISSO_STORE: &str = ".store";

/// La cartella di serie di questa copia: quella che Tauri dà, o, per la copia
/// del Microsoft Store, la sua sorella col suffisso [`SUFFISSO_STORE`].
///
/// # Perché due librerie
///
/// Dentro il pacchetto MSIX Windows manda nella cartella privata del pacchetto
/// i file che la copia **crea** in `%APPDATA%`, e lascia dove sono quelli che
/// esistono già. Sullo stesso nome della copia installata dal sito, quindi, la
/// copia del Store apriva il `rymembra.db` vero e ne creava `-wal` e `-shm`
/// nella cartella privata: un database diviso fra due posti. Un processo
/// chiuso a metà lasciava le transazioni in un giornale che l'altra copia non
/// vede; riaperta, la copia del Store applicava il suo giornale vecchio a un
/// database cambiato nel frattempo; e la casella «cancella i dati» del
/// disinstallatore toglieva il database lasciando il giornale orfano. Con un
/// nome che fuori dal pacchetto non esiste ogni file è nuovo, e resta privato
/// per intero, database e giornale insieme.
///
/// La libreria che c'era — della copia del sito, o della stessa copia del
/// Store prima di questa versione: da dentro il pacchetto si vedono nello
/// stesso posto — si offre al primo avvio (`manutenzione::store_libreria_di_prima`).
fn cartella_di_serie(base: PathBuf, dallo_store: bool) -> PathBuf {
    if !dallo_store {
        return base;
    }
    let mut nome = base.file_name().map(OsString::from).unwrap_or_default();
    nome.push(SUFFISSO_STORE);
    base.with_file_name(nome)
}

/// La libreria che la copia del Microsoft Store può proporre di prendere:
/// quella nella cartella di serie senza suffisso, se c'è un `rymembra.db`.
///
/// `None` fuori dal Store, e quando la cartella l'ha scelta una variabile:
/// chi fa una prova non riceve proposte sulla libreria vera.
#[derive(Debug, Clone)]
pub(crate) struct LibreriaDiPrima(pub(crate) Option<PathBuf>);

/// Dove sta la [`LibreriaDiPrima`], se c'è.
fn libreria_di_prima(app: &tauri::AppHandle, a_mano: bool) -> Option<PathBuf> {
    if a_mano || !dallo_store() {
        return None;
    }
    app.path()
        .app_data_dir()
        .ok()
        .filter(|cartella| cartella.join(stato::NOME_DATABASE).is_file())
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
    let mut percorsi = trasloco::percorsi_per(scelta);
    // La copia del Store non sposta la cartella di Aether: la sua sorella
    // sarebbe quella vera, fuori dal pacchetto, e il trasloco la porterebbe
    // nella cartella privata, lontano dalla copia del sito. Quel che c'è lo
    // propone la [`LibreriaDiPrima`], copiandolo e lasciandolo dov'è.
    if dallo_store() {
        percorsi.precedente = None;
    }
    let rapporto = trasloco::trasloca(&percorsi)?;
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

/// L'errore che la libreria porta dopo un rifiuto: lo stato nato rifiutato
/// ([`rifiuta`]) e, se il nucleo ne aveva già uno di un avvio fallito prima, quello
/// ([`avvia_da_capo`]) — la finestra mostra il motivo vero, non quello di prima.
fn guasto_del_rifiuto(errore: &trasloco::Errore) -> AppError {
    AppError::new(ErrorCode::InternalUnexpected {
        detail: Some("cartella dati rifiutata all'avvio".to_owned()),
    })
    .with_cause(errore.to_string())
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
    stato::Stato::rifiutato(guasto_del_rifiuto(errore))
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

    #[test]
    fn la_copia_dello_store_non_apre_la_libreria_della_copia_nsis() {
        // R20-3: la stessa cartella per le due copie voleva dire, dentro il
        // pacchetto MSIX, il `rymembra.db` della copia del sito aperto sul
        // posto e il suo giornale nella cartella privata del pacchetto. La
        // copia del Store ne ha una sua, sorella e con un nome che fuori dal
        // pacchetto non esiste; quella del sito resta dov'è.
        let base = assoluta("io.github.federicobaratti.rymembra");
        assert_eq!(cartella_di_serie(base.clone(), false), base);
        let dello_store = cartella_di_serie(base.clone(), true);
        assert_ne!(dello_store, base);
        assert_eq!(dello_store.parent(), base.parent(), "sorella, accanto");
        assert_eq!(
            dello_store.file_name().and_then(|nome| nome.to_str()),
            Some("io.github.federicobaratti.rymembra.store")
        );
        // E la scelta di una variabile vince anche sulla copia del Store: la
        // cartella di serie non si calcola nemmeno.
        let a_mano = scegli_la_cartella(os_assoluta("prova"), None, || -> Result<PathBuf, ()> {
            Ok(cartella_di_serie(base.clone(), true))
        })
        .expect("di serie")
        .expect("scelta");
        assert_eq!(a_mano.cartella, assoluta("prova"));
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

    /// La pagina della finestra ha il suo guardiano, registrato con gli altri
    /// plugin: la navigazione (D-44), le pulizie di una pagina nuova (R13-4) e
    /// il disegno a finestra ridotta (R21-3). Un plugin vale per ogni webview,
    /// anche quella che Android ricostruisce con la stessa etichetta, e per
    /// quella nata da `tauri.conf.json`, che non ha un costruttore a cui
    /// appendere un gestore.
    #[test]
    fn la_pagina_della_finestra_ha_il_suo_guardiano() {
        let costruzione = include_str!("lib.rs")
            .split("pub fn run()")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        assert!(
            costruzione.contains(".plugin(pagina::plugin())"),
            "nessun guardiano della pagina fra i plugin"
        );
    }

    /// Il corpo del `setup`, fino alla sua chiusura.
    fn corpo_del_setup() -> &'static str {
        include_str!("lib.rs")
            .split(".setup(|app| {")
            .nth(1)
            .and_then(|resto| {
                resto
                    .split(
                        "
        })
",
                    )
                    .next()
            })
            .unwrap_or_default()
    }

    /// R06b-5: il `setup` non apre la libreria. Gira sul filo della finestra
    /// prima che il ciclo degli eventi parta, e una migrazione lunga lì dentro
    /// era un processo senza finestra per tutto il tempo della migrazione.
    #[test]
    fn il_setup_non_apre_la_libreria() {
        let setup = corpo_del_setup();
        assert!(!setup.is_empty(), "il setup non si trova");
        // L'avvio da zero è una funzione sua, che il `setup` chiama e che la finestra
        // rifà se l'avvio di un servizio è fallito: la libreria non si apre lì dentro.
        assert!(setup.contains("avvia_da_capo(app.handle())"));
        let da_capo = corpo_di("fn avvia_da_capo(");
        assert!(!da_capo.is_empty(), "l'avvio da zero non si trova");
        assert!(
            !da_capo.contains("Stato::apri("),
            "l'avvio apre la libreria sul filo della finestra"
        );
        assert!(da_capo.contains("registra_la_libreria("));
        assert!(da_capo.contains("apri_e_avvia("));
        assert!(
            corpo_di("fn registra_la_libreria(").contains("Stato::in_apertura("),
            "lo stato della libreria non nasce «in apertura»"
        );
    }

    /// Un Alt da solo non deve mandare la finestra in un menu di sistema
    /// invisibile (reperto 1 di FALT), e la guardia va montata nel `setup`: i
    /// pulsanti della miniatura si agganciano solo dopo l'apertura della
    /// libreria — `avvia_dopo_la_libreria` — e la finestra può comparire prima
    /// (`finestra_mostra` durante una migrazione, la rete di sicurezza dopo due
    /// secondi). Con la guardia là, un Alt dato in quel tempo non avrebbe niente a
    /// fermarlo.
    #[test]
    fn la_guardia_dell_alt_si_monta_nel_setup_e_non_dopo_la_libreria() {
        let setup = corpo_del_setup();
        assert!(
            setup.contains("barra::ferma_alt(app.handle())"),
            "il setup non monta la guardia dell'Alt"
        );
        let dopo = include_str!("lib.rs")
            .split("fn avvia_dopo_la_libreria(")
            .nth(1)
            .and_then(|resto| {
                resto
                    .split(
                        "
}
",
                    )
                    .next()
            })
            .unwrap_or_default();
        assert!(
            !dopo.contains("ferma_alt("),
            "la guardia dell'Alt aspetta la libreria"
        );
    }

    /// Il resto dell'avvio non riprende la coda da sé e non apre il
    /// dispositivo: lo fa `apri_il_primo_motore`, su un filo suo, per la
    /// strada del motore mai aperto (R23-9).
    #[test]
    fn il_resto_dell_avvio_apre_l_audio_in_disparte() {
        let resto = include_str!("lib.rs")
            .split("fn avvia_il_nucleo(")
            .nth(1)
            .and_then(|resto| {
                resto
                    .split(
                        "
}
",
                    )
                    .next()
            })
            .unwrap_or_default();
        assert!(!resto.is_empty(), "il resto dell'avvio non si trova");
        assert!(
            !resto.contains("riprendi_coda("),
            "la coda si riprende due volte"
        );
        assert!(resto.contains("riproduzione::apri_il_primo_motore("));
        assert!(
            !resto.contains("riproduzione::avvia_sorveglianza("),
            "il sorvegliante parte dopo il primo tentativo, non insieme"
        );
    }

    /// Chi chiude Rymembra mentre la libreria si apre non deve vedersi
    /// partire il resto dell'avvio sotto una finestra che si smonta.
    #[test]
    fn il_resto_dell_avvio_non_parte_uscendo() {
        let corpo = include_str!("lib.rs")
            .split("fn finisci_l_avvio(")
            .nth(1)
            .and_then(|resto| {
                resto
                    .split(
                        "
}
",
                    )
                    .next()
            })
            .unwrap_or_default();
        let guardia = corpo.find("spegnimento::in_uscita()");
        let resto = corpo.find("avvia_dopo_la_libreria(");
        assert!(
            guardia.is_some() && guardia < resto,
            "il resto dell'avvio parte anche uscendo"
        );
    }

    /// Il ciclo degli eventi di `run`, dalla chiusura della callback in poi.
    fn ciclo_degli_eventi() -> &'static str {
        include_str!("lib.rs")
            .split("Ok(app) => app.run(|mano, evento| {")
            .nth(1)
            .and_then(|resto| resto.split("\n        }),\n").next())
            .unwrap_or_default()
    }

    /// §8.5: la X col vassoio non spegne il nucleo. La fine dell'ultima finestra
    /// (`ExitRequested` senza codice) si impedisce se il vassoio c'è e non si sta
    /// uscendo, **prima** che il ciclo arrivi a `spegnimento::spegni`; e un'uscita
    /// vera — «Esci», con un codice — passa.
    #[test]
    fn la_x_col_vassoio_non_spegne_il_nucleo() {
        let ciclo = ciclo_degli_eventi();
        assert!(!ciclo.is_empty(), "il ciclo degli eventi non si trova");
        let impedisce = ciclo
            .find("tauri::RunEvent::ExitRequested {\n                code: None, api, ..\n            } = &evento")
            .expect("manca il ramo di ExitRequested senza codice");
        let blocco = ciclo.get(impedisce..).unwrap_or_default();
        let fine = blocco.find("return;").expect("il ramo non torna");
        let ramo = blocco.get(..fine).unwrap_or_default();
        assert!(
            ramo.contains("api.prevent_exit()"),
            "non impedisce l'uscita"
        );
        assert!(
            ramo.contains("!spegnimento::in_uscita()"),
            "impedirebbe anche un'uscita voluta"
        );
        assert!(
            ramo.contains("vassoio::nasconde(mano)"),
            "impedirebbe l'uscita anche senza un'icona a cui tornare"
        );
        // Solo `code: None`: un'uscita con un codice è «Esci» o l'aggiornatore.
        assert!(!ramo.contains("code: Some"));
        // E sta **prima** della sequenza di spegnimento.
        let spegni = ciclo
            .find("spegnimento::spegni(mano)")
            .expect("la sequenza di spegnimento non si trova");
        assert!(
            impedisce < spegni,
            "l'uscita si impedisce dopo aver spento tutto"
        );
        assert!(ciclo.contains("crate::chiusura::CHIUSURA.rinviata()"));
    }

    /// Alla X col vassoio la finestra si chiude davvero: non si impedisce e non
    /// si nasconde, o la pagina — con la sua WebView2 — resterebbe viva.
    #[test]
    fn la_x_col_vassoio_chiude_la_finestra_invece_di_nasconderla() {
        let lib = include_str!("lib.rs");
        let gestore = lib
            .split(".on_window_event(|finestra, evento| {")
            .nth(1)
            .and_then(|resto| resto.split("\n        })\n").next())
            .unwrap_or_default();
        assert!(
            !gestore.is_empty(),
            "il gestore della chiusura non si trova"
        );
        let nascondi = gestore
            .split("chiusura::Decisione::Nascondi => {")
            .nth(1)
            .and_then(|resto| resto.split("\n                }\n").next())
            .unwrap_or_default();
        assert!(!nascondi.is_empty(), "il ramo Nascondi non si trova");
        assert!(
            !nascondi.contains("prevent_close") && !nascondi.contains(".hide()"),
            "il ramo Nascondi impedisce la chiusura o nasconde la finestra"
        );
        assert!(nascondi.contains("lontano::finestra_nascosta(mano)"));
    }

    /// §11.2.4: il pacchetto di un aggiornamento in attesa si installa
    /// all'uscita vera e solo lì. La chiusura di una persona — la X senza
    /// vassoio, o «Esci» — si segna **prima** della sequenza di spegnimento; il
    /// pacchetto parte **dopo**, e solo sull'`Exit`. Quando Windows spegne il
    /// computer tao manda `Exit` da solo, senza nessuna delle due.
    #[test]
    fn l_aggiornamento_in_attesa_si_installa_solo_all_uscita_voluta() {
        let ciclo = ciclo_degli_eventi();
        let segna = ciclo
            .find("aggiornamenti::uscita_voluta()")
            .expect("la chiusura voluta non si segna");
        let spegni = ciclo
            .find("spegnimento::spegni(mano)")
            .expect("la sequenza di spegnimento non si trova");
        let installa = ciclo
            .find("aggiornamenti::alla_chiusura(mano)")
            .expect("il pacchetto in attesa non si installa mai");
        assert!(
            segna < spegni,
            "la chiusura voluta si segna dopo lo spegnimento"
        );
        assert!(spegni < installa, "si installa prima di aver spento tutto");
        let davanti = ciclo
            .get(installa.saturating_sub(120)..installa)
            .unwrap_or_default();
        assert!(
            davanti.contains("tauri::RunEvent::Exit"),
            "si installa anche su un evento che non è l'uscita"
        );
        let prima = ciclo
            .get(segna.saturating_sub(120)..segna)
            .unwrap_or_default();
        assert!(
            prima.contains("if chiusura"),
            "si segna la chiusura voluta anche per un'uscita che non viene dalla finestra"
        );
    }

    /// Morta la finestra, quel che la pagina aveva acceso — lo spettro, i
    /// pannelli dei testi — non lo spegne più nessuno: lo spegne il nucleo, alla
    /// `Destroyed`, e non se si sta uscendo.
    #[test]
    fn alla_fine_della_finestra_si_spegne_quel_che_la_pagina_aveva_acceso() {
        let ciclo = ciclo_degli_eventi();
        let da = ciclo
            .find("tauri::WindowEvent::Destroyed")
            .expect("il ciclo non guarda la fine della finestra");
        let ramo = ciclo.get(da..).unwrap_or_default();
        let fine = ramo.find("return;").expect("il ramo non torna");
        let ramo = ramo.get(..fine).unwrap_or_default();
        assert!(ramo.contains("pagina::pagina_finita(mano)"));
        assert!(ramo.contains("!spegnimento::in_uscita()"));
    }

    /// Il corpo di una funzione di questo file, dal suo nome fino alla graffa che la
    /// chiude in colonna zero.
    fn corpo_di(firma: &str) -> &'static str {
        include_str!("lib.rs")
            .split(firma)
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default()
    }

    /// Un nucleo solo per processo: il `setup` reclama l'avvio **prima** di
    /// toccare i dati, e chi lo trova già reclamato — un servizio che l'ha avviato
    /// prima dell'attività — si aggancia invece di rifare la libreria e il lettore.
    #[test]
    fn il_setup_reclama_il_nucleo_prima_di_toccare_i_dati_e_si_aggancia_se_c_e() {
        let setup = corpo_del_setup();
        let reclama = setup
            .find("nucleo.reclama_l_avvio() != ChiAvvia::Tu")
            .expect("il setup non reclama l'avvio");
        let aggancia = setup
            .find("return aggancia_al_nucleo(app.handle(), &nucleo)")
            .expect("il setup non si aggancia a un nucleo già partito");
        let dati = setup
            .find("avvia_da_capo(app.handle())")
            .expect("il setup non avvia il nucleo");
        assert!(
            reclama < aggancia && aggancia < dati,
            "il setup tocca i dati prima di sapere chi avvia il nucleo"
        );
        assert!(
            corpo_di("fn avvia_da_capo(").contains("cartella_dati(app)"),
            "l'avvio da zero non calcola le cartelle"
        );
        let aggancio = corpo_di("fn aggancia_al_nucleo(");
        assert!(
            aggancio.contains("aspetta_il_nucleo(") && !aggancio.contains("Stato::in_apertura("),
            "l'aggancio rifà la libreria invece di aspettare il nucleo"
        );
        assert!(
            corpo_di("fn aspetta_il_nucleo(").contains("nucleo.dopo_l_avvio("),
            "l'aggancio non aspetta la fine dell'avvio"
        );
        assert!(
            corpo_di("fn finisci_l_aggancio_al_tentativo(").contains("finisci_l_avvio(app)"),
            "l'aggancio non fa il resto dell'avvio"
        );
    }

    /// Il calcolo delle cartelle è uno solo: la finestra e il servizio di Android
    /// passano dalla stessa funzione, e `scegli_la_cartella` si chiama da lì e da
    /// nessun altro posto.
    #[test]
    fn la_finestra_e_il_servizio_calcolano_le_cartelle_con_la_stessa_funzione() {
        assert!(
            corpo_di("fn cartella_dati(").contains("cartella_dati_da("),
            "la finestra calcola le cartelle per conto suo"
        );
        let servizio = corpo_di("fn avvia_dal_servizio(");
        assert!(
            servizio.contains("cartella_dati_da("),
            "il servizio calcola le cartelle per conto suo"
        );
        let sorgente = include_str!("lib.rs");
        let prove = sorgente
            .find("#[cfg(test)]\nmod prove")
            .unwrap_or(sorgente.len());
        let codice = sorgente.get(..prove).unwrap_or_default();
        assert_eq!(
            codice.matches("scegli_la_cartella(").count(),
            1,
            "`scegli_la_cartella` si chiama da `cartella_dati_da` e da nessun altro"
        );
    }

    /// Il nucleo senza attività avvia solo quel che serve al traguardo: i fili
    /// pesanti — backup, sincronia, arricchimento, manutenzione, analisi,
    /// sorveglianza, aggiornamenti, vassoio — restano della finestra. A ogni
    /// accensione del telefono sarebbero batteria e disco spesi senza nessuno.
    #[test]
    fn il_nucleo_non_avvia_i_fili_pesanti_e_la_finestra_si() {
        let nucleo = corpo_di("fn avvia_il_nucleo(");
        assert!(!nucleo.is_empty(), "il nucleo non si trova");
        for pesante in [
            "nuvola::",
            "sincronia::",
            "arricchimento::",
            "manutenzione::",
            "analisi::",
            "sorveglia::",
            "aggiornamenti::",
            "vassoio::",
            "ia::StatoIa",
            "AppHandle",
            "app.manage(",
        ] {
            assert!(
                !nucleo.contains(pesante),
                "il nucleo avvia {pesante}, che è della finestra"
            );
        }
        for serve in [
            "riproduzione::apri_il_primo_motore(",
            "media::avvia(",
            "scrobble::avvia(",
            "lontano::avvia_filo(",
            "insieme::StatoInsieme::nuovo()",
        ] {
            assert!(nucleo.contains(serve), "il nucleo non avvia {serve}");
        }
        assert!(
            !nucleo.contains("insieme::avvia_filo("),
            "il filo di «Ascolta insieme» sta in una funzione sua, che il nucleo chiama solo sul computer"
        );
        let finestra = corpo_di("fn avvia_la_finestra(");
        for pesante in [
            "nuvola::avvia_filo(",
            "sincronia::avvia_filo(",
            "arricchimento::avvia_filo(",
            "manutenzione::avvia(",
            "analisi::avvia_filo(",
            "sorveglia::avvia(",
            "aggiornamenti::avvia_filo(",
            "vassoio::avvia(",
            "avvia_il_filo_di_insieme(",
        ] {
            assert!(
                finestra.contains(pesante),
                "la finestra non avvia {pesante}"
            );
        }
        // E il resto dell'avvio avvia il nucleo solo se nessuno l'ha già fatto.
        let dopo = corpo_di("fn avvia_dopo_la_libreria(");
        assert!(
            dopo.contains("if !nucleo.e_gia_avviato()")
                && dopo.contains("avvia_il_nucleo(&nucleo)")
                && dopo.contains("avvia_la_finestra(app)"),
            "il resto dell'avvio non salta il nucleo già partito"
        );
    }

    /// R5: sul computer il filo di «Ascolta insieme» parte **prima** del collegamento
    /// via internet, com'è sempre stato — un flusso che arriva da lontano appena il
    /// collegamento è in piedi trova già chi lo legge —, e sta dentro `#[cfg(desktop)]`:
    /// il nucleo che parte da un servizio non lo avvia, e sul telefono parte con la
    /// finestra. La finestra chiama la stessa funzione, che si avvia una volta sola.
    #[test]
    fn sul_computer_il_filo_di_insieme_parte_prima_del_collegamento_via_internet() {
        let nucleo = corpo_di("fn avvia_il_nucleo(");
        let insieme = nucleo
            .find("avvia_il_filo_di_insieme(nucleo)")
            .expect("il nucleo non avvia il filo di «Ascolta insieme» sul computer");
        let lontano = nucleo
            .find("lontano::avvia_filo(nucleo)")
            .expect("il nucleo non avvia il collegamento via internet");
        assert!(
            insieme < lontano,
            "il collegamento via internet parte prima del filo di «Ascolta insieme»"
        );
        let davanti = nucleo
            .get(insieme.saturating_sub(30)..insieme)
            .unwrap_or_default();
        assert!(
            davanti.contains("#[cfg(desktop)]"),
            "il nucleo avvia il filo di «Ascolta insieme» anche sul telefono, e dal servizio non deve"
        );
        let filo = corpo_di("fn avvia_il_filo_di_insieme(");
        assert!(
            filo.contains("prendi_l_orecchio()") && filo.contains("insieme::avvia_filo("),
            "il filo di «Ascolta insieme» non si avvia con l'orecchio, una volta sola"
        );
    }

    /// R2: quel che la pagina chiede appena monta — `nuvola_stato`, `sincronia_stato`,
    /// `arricchimento_stato`, `finestra_pronta` — trova il suo stato. Li registra l'avvio
    /// della finestra (`avvia_la_finestra`, e l'indice delle cartelle l'aggancio), e la
    /// fase «pronta» si apre solo dopo, in `finisci_l_avvio`: se la pagina potesse montare
    /// prima, quei comandi riceverebbero `internal.aborted` e le fasce d'errore
    /// resterebbero finché un filo non emette.
    #[test]
    fn gli_stati_su_cui_la_pagina_conta_ci_sono_prima_che_la_fase_si_apra() {
        let finestra = corpo_di("fn avvia_la_finestra(");
        for stato in [
            "app.manage(nuvola)",
            "app.manage(sincronia)",
            "app.manage(arricchimento)",
            "app.manage(analisi)",
            "app.manage(aggiornamenti)",
            "app.manage(ia::StatoIa::nuovo())",
            "app.manage(vassoio::StatoVassoio::nuovo())",
        ] {
            assert!(
                finestra.contains(stato),
                "l'avvio della finestra non registra {stato}"
            );
        }
        let dopo = corpo_di("fn avvia_dopo_la_libreria(");
        let finestra_avviata = dopo
            .find("avvia_la_finestra(app)")
            .expect("il resto dell'avvio non avvia la finestra");
        assert!(
            dopo.find("app.manage(barra::StatoBarra::nuovo())")
                .is_some_and(|barra| barra < finestra_avviata),
            "lo stato dei pulsanti nell'anteprima non c'è prima del resto"
        );
        // L'indice delle cartelle, nell'aggancio, prima del resto dell'avvio.
        let aggancio = corpo_di("fn finisci_l_aggancio_al_tentativo(");
        let indice = aggancio
            .find("cartelle::IndiceCartelle::nuovo(")
            .expect("l'aggancio non registra l'indice delle cartelle");
        let resto = aggancio
            .find("finisci_l_avvio(app)")
            .expect("l'aggancio non fa il resto dell'avvio");
        assert!(indice < resto);
        // E il comando che fa la domanda sta nell'elenco: se manca lo stato, risponde errore,
        // non panica (uno `State<T>` mancante dà un `InvokeError`).
        assert!(include_str!("nuvola.rs").contains("fn nuvola_stato("));
    }

    /// Il servizio di Android parte da una funzione che prepara il processo prima di
    /// tutto, reclama l'avvio e non apre il nucleo se c'è già; e la porta JNI è
    /// esportata da `rymembra-android`, non scritta qui.
    #[test]
    fn il_servizio_avvia_il_nucleo_una_volta_e_dopo_aver_preparato_il_processo() {
        let servizio = corpo_di("fn avvia_dal_servizio(");
        let processo = servizio
            .find("inizia_il_processo()")
            .expect("il servizio non prepara il processo");
        let reclama = servizio
            .find("nucleo.reclama_l_avvio() != ChiAvvia::Tu")
            .expect("il servizio non reclama l'avvio");
        let nucleo = servizio
            .find("avvia_il_nucleo(&nucleo)")
            .expect("il servizio non avvia il nucleo");
        // L'avvio si chiude comunque finisca la funzione, anche con un panico: una
        // guardia che lo chiude nel `Drop`, creata **subito** dopo il reclamo e prima
        // di ogni passo che può cadere. Senza, la finestra che nasce dopo aspetta
        // `dopo_l_avvio` per sempre e resta su «Apro la libreria…».
        let guardia = servizio
            .find("GuardiaDellAvvio::nuova(")
            .expect("il servizio non ha la guardia che chiude l'avvio");
        let primo_passo = servizio
            .find("cartella_dati_da(")
            .expect("il servizio non calcola le cartelle");
        assert!(
            processo < reclama
                && reclama < guardia
                && guardia < primo_passo
                && primo_passo < nucleo,
            "l'ordine del servizio non è processo, reclamo, guardia, passi, nucleo"
        );
        assert!(
            !servizio.contains("nucleo.avvio_finito()") && !servizio.contains("avvio_chiuso("),
            "l'avvio si chiude a mano: un panico lo salterebbe"
        );
        // L'esito lo decide la guardia: dopo la libreria un avvio che cade è «rotto» (ci
        // sono dei fili), prima è «fallito» (niente da raddoppiare). Il punto di non
        // ritorno sta **dopo** il controllo che la libreria si è aperta e **prima** di
        // avviare il nucleo; «riuscito» solo dopo.
        let controllo = servizio
            .find("stato.cartella_dati().is_some()")
            .expect("il servizio non controlla che la libreria si sia aperta");
        let senza_ritorno = servizio
            .find("fine.la_libreria_c_e()")
            .expect("il servizio non segna il punto di non ritorno");
        let riuscito = servizio
            .find("fine.riuscito()")
            .expect("il servizio non dice che l'avvio è riuscito");
        assert!(
            controllo < senza_ritorno && senza_ritorno < nucleo && nucleo < riuscito,
            "il punto di non ritorno non sta fra il controllo della libreria e l'avvio del nucleo"
        );
        // E dice a Kotlin se ha senso riprovare: solo un avvio fallito si riprova.
        assert!(
            corpo_di("fn dillo_a_kotlin(").contains("nucleo_non_parte(esito == Esito::Fallito)"),
            "Kotlin non sa se riprovare"
        );
        // I fili del nucleo aspettano il cancello della libreria (`con_libreria` →
        // `aspetta_l_apertura`): senza una finestra che lo apra dopo, lo apre il
        // servizio, subito dopo aver aperto la libreria e prima di avviare il nucleo. Il
        // cancello dei fili, **non** quello della pagina: se il servizio aprisse anche la
        // fase che vede la pagina, un'attività che nasce mentre il nucleo si avvia
        // monterebbe prima che esistano gli stati della finestra (R2).
        let libreria = servizio
            .find("apri_la_libreria(")
            .expect("il servizio non apre la libreria");
        let cancello = servizio.find("stato.apri_il_cancello_dei_fili()").expect(
            "il servizio non apre il cancello dei fili: i fili del nucleo aspetterebbero per sempre",
        );
        assert!(
            libreria < cancello && cancello < nucleo,
            "il cancello si apre dopo la libreria e prima del nucleo"
        );
        assert!(
            !servizio.contains(".apri_il_cancello()"),
            "il servizio apre la fase della pagina: l'attività monterebbe prima che gli stati della finestra ci siano"
        );
        // La fase della pagina la apre la finestra, a fine avvio, **dopo** `avvia_dopo_la_libreria`.
        let finisci = corpo_di("fn finisci_l_avvio(");
        let resto = finisci
            .find("avvia_dopo_la_libreria(")
            .expect("la finestra non fa il resto dell'avvio");
        let pagina = finisci
            .find("stato.apri_il_cancello()")
            .expect("la finestra non apre la fase della pagina");
        assert!(
            resto < pagina,
            "la fase della pagina si apre prima degli stati della finestra"
        );
        assert!(
            include_str!("lib.rs")
                .contains("rymembra_android::esporta_il_nucleo!(crate::avvia_dal_servizio)"),
            "la porta JNI non è esportata"
        );
        let processo = corpo_di("fn inizia_il_processo(");
        assert!(
            processo.contains("Once") && processo.contains("installa_gancio_dei_panici()"),
            "il gancio dei panici si può installare due volte"
        );
    }

    /// R3: l'aggancio non legge la libreria dal filo della finestra mentre i fili del
    /// nucleo, già vivi, la tengono: aspetta che sia libera — senza fermare il filo —
    /// e dopo dieci secondi va avanti come prima. Il tetto di quattro secondi del filo
    /// della finestra lascerebbe i conti del cloud vuoti per tutta la vita del processo.
    #[test]
    fn l_aggancio_aspetta_che_la_libreria_si_liberi_ma_non_per_sempre() {
        assert!(!da_riprovare_piu_tardi(true, 0), "libera: si va avanti");
        assert!(da_riprovare_piu_tardi(false, 0), "occupata: si riprova");
        assert!(da_riprovare_piu_tardi(false, ATTESE_DELLA_LIBRERIA - 1));
        assert!(
            !da_riprovare_piu_tardi(false, ATTESE_DELLA_LIBRERIA),
            "dopo il tetto delle attese si va avanti, come prima"
        );
        assert_eq!(
            PASSO_DELLA_LIBRERIA * ATTESE_DELLA_LIBRERIA,
            std::time::Duration::from_secs(10)
        );
        // Si guarda **prima** di `finisci_l_avvio`, che è dove `nuvola_musica::avvia` legge.
        let aggancio = corpo_di("fn finisci_l_aggancio_al_tentativo(");
        let guarda = aggancio
            .find("stato.e_libera()")
            .expect("l'aggancio non guarda se la libreria è libera");
        let avvia = aggancio
            .find("finisci_l_avvio(app)")
            .expect("l'aggancio non fa il resto dell'avvio");
        assert!(guarda < avvia, "si guarda dopo aver già letto");
        assert!(
            aggancio.contains("run_on_main_thread("),
            "si riprova bloccando il filo della finestra invece di rimettersi in coda"
        );
    }

    /// FC5, C3: nel ramo del rifiuto di `avvia_da_capo`, lo stato di un avvio fallito prima
    /// (quello che il nucleo ha già, in guasto) **prende il motivo del rifiuto** — la pagina
    /// mostra il guasto vero, non `db.openFailed` di prima —, e la fase che vede la pagina si
    /// apre **dopo** `avvia_dopo_la_libreria`, come in [`finisci_l_avvio`]: un cancello aperto
    /// prima farebbe montare la pagina (`finita("guasto")`) prima di `StatoNuvola` e degli
    /// altri stati della finestra. Ai fili del nucleo si apre subito il cancello loro.
    #[test]
    fn il_rifiuto_dopo_un_avvio_fallito_apre_la_pagina_dopo_gli_stati_e_dice_il_motivo_vero() {
        let da_capo = corpo_di("fn avvia_da_capo(");
        let ramo = da_capo
            .split("Err(errore) => {")
            .nth(1)
            .expect("l'avvio da zero non ha il ramo del rifiuto");
        let stati = ramo
            .find("avvia_dopo_la_libreria(app)")
            .expect("il rifiuto non fa il resto dell'avvio");
        let fili = ramo
            .find("apri_il_cancello_dei_fili()")
            .expect("i fili del nucleo non hanno il loro cancello");
        let pagina = ramo
            .find(".apri_il_cancello()")
            .expect("la fase della pagina non si apre");
        assert!(
            fili < stati && stati < pagina,
            "la fase della pagina si apre prima degli stati della finestra"
        );
        let motivo = ramo
            .find("completa(Err(guasto_del_rifiuto(&errore)))")
            .expect("lo stato di prima non prende il motivo del rifiuto");
        assert!(
            motivo < stati,
            "il motivo arriva dopo che la pagina può leggerlo"
        );
        assert!(
            ramo.contains("emetti(\"libreria:stato\""),
            "la pagina, ferma su «Apro la libreria…», non sa che la fase è cambiata"
        );
    }

    /// FC5, C4: se la finestra non riesce a rifare l'avvio nemmeno da zero (Tauri non sa
    /// dire dove stanno i dati), la notifica del sottofondo lo sa, **non riprovabile**: la
    /// finestra c'è già, e «apri Rymembra» non servirebbe a niente. Come
    /// [`avvisa_se_la_libreria_manca`], e dopo aver chiuso l'avvio (C2).
    #[test]
    fn se_la_finestra_non_rifa_l_avvio_la_notifica_dice_di_chiudere_l_app() {
        let rifai = corpo_di("fn rifai_l_avvio(");
        let ramo = rifai
            .split("if let Err(err) = avvia_da_capo(app) {")
            .nth(1)
            .expect("rifare l'avvio non ha il ramo dell'errore");
        let chiuso = ramo
            .find("nucleo.avvio_chiuso(Esito::Fallito)")
            .expect("il ramo dell'errore non chiude l'avvio");
        let avvisa = ramo
            .find("dillo_a_kotlin(Esito::Rotto)")
            .expect("il ramo dell'errore non lo dice a Kotlin, o lo dice «riprovabile»");
        assert!(
            chiuso < avvisa,
            "Kotlin si avvisa prima di aver chiuso l'avvio"
        );
        assert!(
            ramo[..avvisa].contains("#[cfg(target_os = \"android\")]"),
            "l'avviso a Kotlin non è solo di Android: `dillo_a_kotlin` non esiste altrove"
        );
    }

    /// FC5, C5: chi chiude Rymembra mentre l'aggancio aspetta la libreria non vuole altri
    /// giri: ogni riaccodamento è un filo breve e una richiesta al filo della finestra,
    /// cioè toccare l'`AppHandle` mentre lo si smonta (vedi [`rete_di_sicurezza`] e
    /// [`apri_e_avvia`], che fanno la stessa domanda). Si guarda in testa, e di nuovo dopo
    /// l'attesa — il momento in cui l'uscita può essere cominciata — prima di rimettersi
    /// in coda.
    #[test]
    fn l_aggancio_non_si_rimette_in_coda_mentre_si_esce() {
        let aggancio = corpo_di("fn finisci_l_aggancio_al_tentativo(");
        let in_testa = aggancio
            .find("spegnimento::in_uscita()")
            .expect("l'aggancio non guarda se si sta uscendo");
        let riprova = aggancio
            .find("da_riprovare_piu_tardi(")
            .expect("l'aggancio non decide se riprovare");
        assert!(
            in_testa < riprova,
            "si decide di riprovare prima di guardare l'uscita"
        );
        let dorme = aggancio
            .find("std::thread::sleep(PASSO_DELLA_LIBRERIA)")
            .expect("l'aggancio non aspetta fra un tentativo e l'altro");
        let rimette = aggancio
            .find("mano.run_on_main_thread(")
            .expect("l'aggancio non si rimette in coda");
        let dopo_l_attesa = aggancio[dorme..]
            .find("spegnimento::in_uscita()")
            .map(|posto| posto + dorme)
            .expect("dopo l'attesa non si guarda se si sta uscendo");
        assert!(
            dorme < dopo_l_attesa && dopo_l_attesa < rimette,
            "dopo l'attesa si chiede al filo della finestra un altro giro senza guardare l'uscita"
        );
    }

    /// R1/R2: quel che la finestra fa quando il nucleo che aspettava chiude l'avvio.
    /// Riuscito: si aggancia. Fallito (la libreria non si apriva, niente di vivo): lo
    /// rifà da zero. Rotto (caduto a metà): la pagina non monta, e dice che non è partito.
    #[test]
    fn la_finestra_segue_l_esito_del_nucleo_che_aspettava() {
        let segui = corpo_di("fn segui_l_esito(");
        for ramo in [
            "Esito::Riuscito => finisci_l_aggancio(app)",
            "Esito::Fallito => rifai_l_avvio(app)",
            "Esito::Rotto => finisci_senza_nucleo(app)",
        ] {
            assert!(segui.contains(ramo), "manca il ramo {ramo}");
        }
        // Rifare l'avvio vuol dire reclamarlo di nuovo, e farlo da zero come la
        // finestra che nasce per prima.
        let rifai = corpo_di("fn rifai_l_avvio(");
        let reclama = rifai
            .find("nucleo.reclama_l_avvio() != ChiAvvia::Tu")
            .expect("rifare l'avvio non lo reclama: due tentativi lo farebbero due volte");
        let da_capo = rifai
            .find("avvia_da_capo(app)")
            .expect("rifare l'avvio non lo fa da zero");
        assert!(reclama < da_capo);
        // Chi non è il primo aspetta l'esito di chi lo è, invece di cadere nel vuoto.
        assert!(rifai.contains("aspetta_il_nucleo("));
        // Un nucleo che non c'è, o è a metà, non porta la pagina a «pronta»: dice
        // «non è partito» e non fa il resto dell'avvio.
        let senza = corpo_di("fn finisci_senza_nucleo(");
        assert!(senza.contains(".non_parte()"));
        assert!(
            !senza.contains("apri_il_cancello()")
                && !senza.contains("finisci_l_avvio(")
                && !senza.contains("avvia_dopo_la_libreria("),
            "un nucleo a metà porta la pagina a «pronta»"
        );
        // E l'aggancio riuscito non rifà la libreria: la trova.
        assert!(!corpo_di("fn finisci_l_aggancio_al_tentativo(").contains("riapri("));
    }

    /// R1: un nucleo che parte dalla finestra senza libreria aperta — il guasto è ancora lì, dopo
    /// che la finestra ha rifatto l'avvio del servizio — non lascia la notifica del sottofondo
    /// su «si sta avviando» né su «apri Rymembra»: lo dice a Kotlin, non riprovabile.
    #[test]
    fn senza_libreria_la_notifica_dice_di_chiudere_l_app() {
        let dir = tempfile::tempdir().expect("cartella");
        // Nessuno stato: niente libreria.
        assert!(senza_libreria(None));
        // In apertura, o aperta male: la libreria non c'è.
        let stato = stato::Stato::in_apertura(dir.path().to_path_buf());
        assert!(senza_libreria(Some(&stato)));
        stato.completa(Err(AppError::new(ErrorCode::DbOpenFailed { path: None })));
        stato.apri_il_cancello();
        assert!(senza_libreria(Some(&stato)));
        // Aperta: c'è.
        let aperta = stato::Stato::apri(dir.path().to_path_buf());
        assert!(!senza_libreria(Some(&aperta)));
        // Il cablaggio: dopo aver chiuso l'avvio, e non riprovabile.
        let dopo = corpo_di("fn avvia_dopo_la_libreria(");
        let chiuso = dopo
            .find("nucleo.avvio_finito()")
            .expect("il resto dell'avvio non chiude l'avvio");
        let avvisa = dopo
            .find("avvisa_se_la_libreria_manca(&nucleo)")
            .expect("il nucleo senza libreria non lo dice a Kotlin");
        assert!(chiuso < avvisa);
        let corpo = corpo_di("fn avvisa_se_la_libreria_manca(");
        assert!(
            corpo.contains("senza_libreria(") && corpo.contains("dillo_a_kotlin(Esito::Rotto)"),
            "l'avviso a Kotlin non è «non riprovabile»"
        );
    }

    /// Il lettore non segna più il filo della finestra: lo avvia anche un servizio,
    /// su un filo qualunque. Lo segna il `setup`.
    #[test]
    fn il_filo_della_finestra_lo_segna_il_setup_e_non_il_lettore() {
        let setup = corpo_del_setup();
        assert!(setup.contains("riproduzione::segna_filo_della_finestra()"));
        let riproduzione = include_str!("riproduzione/mod.rs");
        let avvia = riproduzione
            .split("pub fn avvia(\n        app: &Nucleo,")
            .nth(1)
            .and_then(|resto| resto.split("\n    }\n").next())
            .unwrap_or_default();
        assert!(!avvia.is_empty(), "StatoLettore::avvia non si trova");
        assert!(
            !avvia.contains("FILO_DELLA_FINESTRA"),
            "StatoLettore::avvia segna ancora il filo: dal servizio segnerebbe quello sbagliato"
        );
    }
}
