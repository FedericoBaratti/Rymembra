//! I controlli multimediali del sistema operativo.
//!
//! # Cosa mancava
//!
//! Aether non parlava in nessun modo con Windows. Fra i comandi registrati in
//! [`crate::main`] non ce n'era uno che uscisse dalla finestra: il tasto
//! play/pausa della tastiera non faceva niente, nel riquadro del volume di
//! Windows 11 non compariva nessuna scheda, la barra delle applicazioni non
//! mostrava anteprima né comandi, e le cuffie Bluetooth non comandavano nulla.
//! Un lettore musicale sta in secondo piano quasi tutto il tempo che è acceso,
//! e per tutto quel tempo era irraggiungibile se non tornandoci sopra col
//! mouse.
//!
//! # Una registrazione sola per due cose
//!
//! Le `SystemMediaTransportControls` non sono soltanto la scheda che si vede:
//! registrandole è **Windows** a instradare i tasti multimediali
//! all'applicazione che sta suonando. Per questo qui non c'è nessuna
//! scorciatoia globale — una scorciatoia registrata su `MediaPlayPause`
//! ruberebbe quel tasto a ogni altro programma, per sempre, anche a lettore
//! fermo, ed è esattamente il difetto che rende insopportabili i lettori che
//! lo fanno.
//!
//! # Perché una dipendenza
//!
//! Il perché sta per esteso in `Cargo.toml`, accanto alla riga: la radice del
//! workspace vieta `unsafe` con `forbid`, che non si scavalca con un
//! `#[allow]`, e parlare di COM col sistema operativo è `unsafe` per
//! definizione. Il puntatore della finestra lo si **passa** e basta — passare
//! un puntatore grezzo non è `unsafe`, lo è dereferenziarlo — e a
//! dereferenziarlo ci pensa `souvlaki`.
//!
//! # Dove si aggancia
//!
//! In un punto solo: [`crate::riproduzione::manda_stato`], che è già l'unica
//! sorgente dello stato verso la finestra. Se la scheda del sistema si
//! aggiornasse da un posto suo, prima o poi mostrerebbe un brano diverso da
//! quello che suona — ed è il tipo di difetto che nessuno segnala, perché chi
//! lo vede pensa di aver letto male.

use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use aether_app::covers::CoverStore;
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};
use tauri::Manager as _;

use crate::nota;
use crate::riproduzione::{StatoLettore, StatoRiproduzione};
use crate::stato::{Stato, con_libreria};

/// Di quanto si sposta un `Seek` senza durata.
///
/// Gli stessi cinque secondi delle frecce in [`crate::riproduzione`]: il gesto
/// è lo stesso, e due passi diversi per la stessa intenzione si notano.
const PASSO_SALTO_MS: u64 = 5_000;

/// I controlli del sistema, e quel che serve per riempirli.
///
/// # Perché la copertina sta qui dentro
///
/// SMTC vuole un **file vero**, non lo schema `aether-cover` con cui la
/// finestra chiede le copertine: quel protocollo lo serve l'applicazione, e il
/// sistema operativo non lo sa parlare. Il percorso si ricava da
/// [`CoverStore::path_for`], che è deterministico, e lo `CoverStore` è
/// `Clone` — quindi se ne tiene una copia presa una volta all'avvio.
///
/// L'alternativa era prendere il lucchetto della libreria a ogni cambio di
/// stato solo per leggere una cartella che non cambia mai. Con l'ordine dei
/// lucchetti che vige qui — prima il lettore, poi la libreria — sarebbe stata
/// una presa in più dentro una sezione già critica, per un valore costante.
pub struct StatoMedia {
    /// I controlli, se il sistema li ha concessi.
    ///
    /// `Option` e non `Result`: che i controlli non ci siano non è un guasto da
    /// mostrare. Su una macchina che non li offre il lettore funziona come
    /// prima, e l'unica differenza è che la scheda nel riquadro del volume non
    /// compare.
    controlli: Mutex<Option<Controlli>>,
    /// L'ultima posizione nota, in millisecondi.
    ///
    /// Serve solo ai salti relativi, che il sistema manda senza dire da dove.
    /// In un atomico e non dentro il mutex perché chi lo legge è la chiusura
    /// degli eventi, che gira su un filo suo mentre lo scrivono [`aggiorna`] e
    /// [`segna_posizione`] — e prendere un lucchetto per leggere un intero
    /// significherebbe che un tasto premuto aspetta il disegno di uno stato.
    ///
    /// Lo scrivono in due, e non è la doppia sorgente che il modulo evita
    /// altrove: la scheda che si **vede** la compone solo [`aggiorna`]. Questo
    /// numero è un'altra cosa — dove sta il brano adesso — e lo stato completo
    /// parte a ogni cambio, non mentre la musica scorre. Con il solo
    /// [`aggiorna`] restava quindi fermo al valore dell'ultimo cambio, di
    /// solito lo zero dell'inizio del brano, e ogni «avanti di dieci secondi»
    /// dalle cuffie riportava a dieci secondi dall'inizio invece che dieci
    /// secondi più avanti.
    posizione_ms: AtomicU64,
}

/// I due pezzi che stanno insieme o non stanno affatto.
struct Controlli {
    /// La maniglia verso il sistema operativo.
    controlli: MediaControls,
    /// Dove stanno le copertine su disco.
    copertine: CoverStore,
}

impl StatoMedia {
    /// Uno stato spento, da riempire con [`avvia`].
    #[must_use]
    pub const fn nuovo() -> Self {
        Self {
            controlli: Mutex::new(None),
            posizione_ms: AtomicU64::new(0),
        }
    }
}

/// Registra i controlli e collega i tasti.
///
/// Va chiamata **dopo** che la finestra esiste: su Windows le SMTC si
/// appendono a una finestra, e senza il suo puntatore non c'è niente a cui
/// appenderle. Ogni motivo per cui potrebbe non riuscire — niente finestra,
/// niente puntatore, il sistema che rifiuta — finisce nella stessa riga di
/// registro e lascia l'applicazione intera: un lettore senza scheda nel
/// riquadro del volume è un lettore, un lettore che non si apre no.
pub fn avvia(app: &tauri::AppHandle) {
    let Some(finestra) = app.get_webview_window("main") else {
        nota!("[media] nessuna finestra: i tasti multimediali restano spenti");
        return;
    };
    let punt: *mut c_void = match finestra.hwnd() {
        Ok(hwnd) => hwnd.0.cast(),
        Err(err) => {
            nota!("[media] la finestra non ha un puntatore: {err}");
            return;
        }
    };

    let config = PlatformConfig {
        // Il nome che il sistema mostra accanto ai comandi. Non tradotto: è
        // il nome del programma, e i nomi dei programmi non si traducono.
        display_name: "Aether",
        // Serve solo a MPRIS su Linux, che qui non si compila mai. Sta qui
        // perché il tipo lo pretende, non perché faccia qualcosa.
        dbus_name: "aether",
        hwnd: Some(punt),
    };

    let mut controlli = match MediaControls::new(config) {
        Ok(controlli) => controlli,
        Err(err) => {
            nota!("[media] il sistema non concede i controlli: {err:?}");
            return;
        }
    };

    let mano = app.clone();
    if let Err(err) = controlli.attach(move |evento| su_evento(&mano, evento)) {
        nota!("[media] i tasti non si collegano: {err:?}");
        return;
    }

    // Lo `CoverStore` si prende una volta sola, qui: la cartella non cambia
    // per tutta la vita del processo.
    let stato_app = app.state::<Stato>();
    let copertine = con_libreria(&stato_app, |libreria| {
        Ok::<CoverStore, aether_domain::errors::AppError>(libreria.covers.clone())
    });
    let Ok(copertine) = copertine else {
        nota!("[media] la libreria non è aperta: niente copertine nella scheda");
        return;
    };

    let stato = app.state::<StatoMedia>();
    let mut dentro = stato
        .controlli
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    *dentro = Some(Controlli {
        controlli,
        copertine,
    });
}

/// Rispecchia lo stato del lettore nella scheda del sistema.
///
/// Chiamata da [`crate::riproduzione::manda_stato`] con lo stato **già
/// composto**: comporlo una seconda volta vorrebbe dire una seconda lettura
/// del brano dal database a ogni cambio, e soprattutto due risposte possibili
/// alla stessa domanda.
pub fn aggiorna(app: &tauri::AppHandle, stato: &StatoRiproduzione) {
    // `try_state` e non `state`: quest'ultima **va in panico** se lo stato non
    // è registrato, e chi chiama è anche il filo dell'orologio, che parte
    // insieme al lettore. L'ordine in `main` è già quello giusto, ma un ordine
    // giusto è una cosa che si può cambiare per sbaglio fra un anno; una
    // riproduzione che si interrompe perché la scheda del volume non era
    // pronta, no.
    let Some(media) = app.try_state::<StatoMedia>() else {
        return;
    };
    media
        .posizione_ms
        .store(stato.posizione_ms, Ordering::Relaxed);

    let mut dentro = media
        .controlli
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let Some(controlli) = dentro.as_mut() else {
        return;
    };

    // Il percorso della copertina va tenuto vivo per tutta la chiamata:
    // `MediaMetadata` presta delle `&str` e non possiede niente.
    let copertina = stato
        .brano
        .as_ref()
        .and_then(|brano| brano.cover_art_hash.as_ref())
        .map(|hash| controlli.copertine.path_for(hash))
        .filter(|percorso| percorso.exists())
        .and_then(|percorso| url_di_file(&percorso));

    match stato.brano.as_ref() {
        Some(brano) => {
            let _ = controlli.controlli.set_metadata(MediaMetadata {
                title: Some(&brano.title),
                album: Some(&brano.album),
                artist: Some(&brano.artist),
                cover_url: copertina.as_deref(),
                duration: Some(Duration::from_millis(stato.durata_ms)),
            });
        }
        // Coda finita: si svuota la scheda invece di lasciarci l'ultimo brano,
        // che altrimenti resterebbe lì a dire che sta suonando qualcosa.
        None => {
            let _ = controlli.controlli.set_metadata(MediaMetadata::default());
        }
    }

    let dove = Some(MediaPosition(Duration::from_millis(stato.posizione_ms)));
    let riproduzione = if stato.brano.is_none() {
        MediaPlayback::Stopped
    } else if stato.in_pausa {
        MediaPlayback::Paused { progress: dove }
    } else {
        MediaPlayback::Playing { progress: dove }
    };
    let _ = controlli.controlli.set_playback(riproduzione);
}

/// Aggiorna dove sta il brano, senza toccare la scheda.
///
/// La chiama l'orologio di [`crate::riproduzione`], lo stesso giro che manda
/// `riproduzione:tempo` alla finestra: quattro volte al secondo, e senza
/// ricomporre niente. Serve ai salti relativi — vedi
/// [`StatoMedia::posizione_ms`] — e non ridisegna la scheda del sistema, che
/// interpola per conto suo dall'ultimo [`MediaPlayback`] ricevuto.
pub fn segna_posizione(app: &tauri::AppHandle, ms: u64) {
    // `try_state` per la stessa ragione di [`aggiorna`]: l'orologio parte
    // insieme al lettore, e questo modulo può non essere ancora registrato.
    if let Some(media) = app.try_state::<StatoMedia>() {
        media.posizione_ms.store(ms, Ordering::Relaxed);
    }
}

/// Un `file://` che Windows sappia aprire.
///
/// `Url::from_file_path` sarebbe la strada, ma vorrebbe dire dipendere da
/// `url` qui per una riga. I percorsi di Windows hanno una sola stranezza che
/// conta — le barre rovesce — e il resto lo si lascia stare: se un percorso
/// contiene caratteri che vorrebbero la codifica percentuale, la scheda
/// resterà senza immagine, che è esattamente quel che succedeva prima di
/// questo modulo.
fn url_di_file(percorso: &std::path::Path) -> Option<String> {
    let testo = percorso.to_str()?;
    Some(format!("file:///{}", testo.replace('\\', "/")))
}

/// Un tasto premuto, o un comando arrivato dalla scheda del sistema.
///
/// Ogni ramo passa dai comandi che esistono già invece di toccare il lettore:
/// così un tasto della tastiera e il pulsante nella finestra fanno
/// **letteralmente** la stessa cosa, salvataggio della coda e invio dello
/// stato compresi.
fn su_evento(app: &tauri::AppHandle, evento: MediaControlEvent) {
    let lettore = app.state::<StatoLettore>();
    let esito = match evento {
        MediaControlEvent::Play => crate::riproduzione::riprendi(app.clone(), lettore),
        MediaControlEvent::Pause => crate::riproduzione::pausa(app.clone(), lettore),
        MediaControlEvent::Toggle => crate::riproduzione::alterna(app.clone(), lettore),
        MediaControlEvent::Next => crate::riproduzione::prossimo(app.clone(), lettore),
        MediaControlEvent::Previous => crate::riproduzione::precedente(app.clone(), lettore),
        // `Stop` come pausa, di proposito: fermare davvero butterebbe la
        // posizione, e il tasto stop di una tastiera lo si preme per
        // interrompere, non per perdere il segno.
        MediaControlEvent::Stop => crate::riproduzione::pausa(app.clone(), lettore),
        MediaControlEvent::SetPosition(MediaPosition(dove)) => {
            crate::riproduzione::vai_a(app.clone(), lettore, ms_di(dove))
        }
        MediaControlEvent::Seek(verso) => {
            let dove = salto(app, verso, PASSO_SALTO_MS);
            crate::riproduzione::vai_a(app.clone(), lettore, dove)
        }
        MediaControlEvent::SeekBy(verso, quanto) => {
            let dove = salto(app, verso, ms_di(quanto));
            crate::riproduzione::vai_a(app.clone(), lettore, dove)
        }
        // Il volume del sistema non è il volume del lettore, e `OpenUri`
        // chiederebbe di suonare qualcosa che non è in libreria. `Raise` e
        // `Quit` sono gesti della finestra che la scheda non deve avere: da
        // lì si comanda la musica, non il programma.
        MediaControlEvent::SetVolume(_)
        | MediaControlEvent::OpenUri(_)
        | MediaControlEvent::Raise
        | MediaControlEvent::Quit => Ok(()),
    };
    if let Err(err) = esito {
        nota!(
            "[media] {} causa={}",
            err.code,
            err.cause.unwrap_or_default()
        );
    }
}

/// Dove finisce un salto relativo, senza uscire dal brano.
fn salto(app: &tauri::AppHandle, verso: SeekDirection, quanto_ms: u64) -> u64 {
    let adesso = app
        .state::<StatoMedia>()
        .posizione_ms
        .load(Ordering::Relaxed);
    match verso {
        SeekDirection::Forward => adesso.saturating_add(quanto_ms),
        SeekDirection::Backward => adesso.saturating_sub(quanto_ms),
    }
}

/// I millisecondi di una durata, senza traboccare.
fn ms_di(durata: Duration) -> u64 {
    u64::try_from(durata.as_millis()).unwrap_or(u64::MAX)
}
