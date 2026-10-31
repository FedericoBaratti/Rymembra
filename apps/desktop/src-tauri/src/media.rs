//! I controlli multimediali del sistema operativo.
//!
//! # Cosa mancava
//!
//! Rymembra non parlava in nessun modo con Windows. Fra i comandi registrati in
//! [`crate::main`] non ce n'era uno che uscisse dalla finestra: il tasto
//! play/pausa della tastiera non faceva niente, nel riquadro del volume di
//! Windows 11 non compariva nessuna scheda, la barra delle applicazioni non
//! mostrava anteprima né comandi, e le cuffie Bluetooth non comandavano nulla.
//! Un lettore musicale sta in secondo piano quasi tutto il tempo che è acceso,
//! e per tutto quel tempo era irraggiungibile se non tornandoci sopra col
//! mouse.
//!
//! Questo modulo chiude la scheda nel riquadro del volume e i tasti della
//! tastiera e delle cuffie. I tre pulsanti sotto l'anteprima nella barra delle
//! applicazioni sono un'altra cosa — le SMTC non li danno — e li mette
//! `crate::barra`, con gli stessi gesti di qui: vedi [`premi`].
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
//!
//! # Tre difetti che la 3.0 ha chiuso
//!
//! **La scheda restava vuota per ogni brano con la copertina.** Il percorso
//! della copertina partiva come `file:///C:/…`; `souvlaki` toglie soltanto
//! `file://` e passa a Windows `/C:/…`, che `StorageFile::GetFileFromPathAsync`
//! rifiuta. L'errore usciva col `?` **prima** di `DisplayUpdater::Update`, cioè
//! prima che titolo, artista, album e durata venissero consegnati, e qui lo si
//! scartava in silenzio. Adesso l'indirizzo è quello che `souvlaki` sa
//! trasformare in un percorso vero: vedi [`url_di_file`].
//!
//! **A coda finita restava l'ultimo brano.** `souvlaki` scrive solo i campi che
//! riceve: `MediaMetadata::default()` non azzerava niente. Adesso si mandano
//! stringhe vuote.
//!
//! **La copertina si ricaricava a ogni pausa.** Ogni stato passava la
//! copertina, e ogni copertina è un file aperto e un'attesa del sistema: a ogni
//! pausa, salto o ritocco della coda. Adesso la scheda si riscrive solo quando
//! cambia, e la copertina solo quando cambia lei.
//!
//! E i tasti non fanno più aspettare il sistema: arrivano su un filo di
//! Windows, e un «prossimo» su un brano che arriva dalla rete può durare
//! quindici secondi. Vedi [`premi`].

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;

use rymembra_app::covers::CoverStore;
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};
use tauri::Manager as _;

use crate::nota;
use crate::riproduzione::{StatoLettore, StatoRiproduzione};
use crate::spegnimento::Emette as _;
use crate::stato::Stato;

/// Di quanto si sposta un `Seek` senza durata.
///
/// Gli stessi cinque secondi delle frecce in [`crate::riproduzione`]: il gesto
/// è lo stesso, e due passi diversi per la stessa intenzione si notano.
const PASSO_SALTO_MS: u64 = 5_000;

/// I controlli del sistema, e quel che serve per riempirli.
///
/// # Perché la copertina sta qui dentro
///
/// SMTC vuole un **file vero**, non lo schema `rymembra-cover` con cui la
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
    /// Quanto dura il brano corrente, in millisecondi.
    ///
    /// Accanto alla posizione, e per la stessa ragione: la legge la chiusura
    /// degli eventi, sul filo del sistema operativo, e serve a non far uscire
    /// dal brano un gesto che voleva restarci. «Avanti di cinque secondi» a
    /// tre secondi dalla fine chiedeva un salto oltre l'ultimo campione, e un
    /// salto oltre la fine — o **sulla** fine — è la fine: il gesto diventava
    /// «brano successivo» senza che nessuno l'avesse chiesto. Per questo il
    /// tetto dei salti relativi sta un margine prima: vedi [`avanti_entro`].
    ///
    /// `0` vuol dire **non si sa** — niente è ancora partito — e non «lungo
    /// zero»: con un tetto a zero ogni salto in avanti tornerebbe all'inizio,
    /// che è un difetto peggiore di quello che il tetto viene a togliere.
    durata_ms: AtomicU64,
    /// Il canale verso il filo che esegue i tasti. Vedi [`premi`].
    tasti: OnceLock<Sender<Gesto>>,
}

/// I due pezzi che stanno insieme o non stanno affatto, e quel che si è già
/// mostrato.
struct Controlli {
    /// La maniglia verso il sistema operativo.
    controlli: MediaControls,
    /// Dove stanno le copertine su disco.
    copertine: CoverStore,
    /// La scheda consegnata a Windows l'ultima volta che la consegna è
    /// riuscita. Vedi [`Scheda`].
    mostrata: Option<Scheda>,
    /// L'ultima scheda che Windows ha rifiutato: serve a scriverlo nel diario
    /// una volta sola invece che a ogni stato.
    rifiutata: Option<Scheda>,
}

impl StatoMedia {
    /// Uno stato spento, da riempire con [`avvia`].
    #[must_use]
    pub const fn nuovo() -> Self {
        Self {
            controlli: Mutex::new(None),
            posizione_ms: AtomicU64::new(0),
            durata_ms: AtomicU64::new(0),
            tasti: OnceLock::new(),
        }
    }
}

/// Quel che la scheda di Windows mostra, per sapere se è cambiato.
///
/// # Perché si confronta
///
/// Perché consegnare la scheda costa: `souvlaki` riscrive i quattro testi, la
/// durata, e — se riceve una copertina — **apre il file** e aspetta il sistema
/// finché non l'ha letto. Lo stato del lettore invece parte a ogni pausa, a
/// ogni salto, a ogni ritocco della coda, e prima di questo confronto ognuno
/// di loro riapriva la stessa copertina. Adesso la scheda si consegna solo se
/// qualcosa di quel che mostra è cambiato, e la copertina solo se è cambiata
/// lei: a `souvlaki` arriva `None`, e il sistema tiene quella che ha.
#[derive(Clone, PartialEq, Eq)]
struct Scheda {
    track_id: Option<i64>,
    titolo: String,
    artista: String,
    album: String,
    /// L'indirizzo della copertina, già nella forma di [`url_di_file`].
    copertina: Option<String>,
    durata_ms: u64,
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
        display_name: "Rymembra",
        // Serve solo a MPRIS su Linux, che qui non si compila mai. Sta qui
        // perché il tipo lo pretende, non perché faccia qualcosa.
        dbus_name: "rymembra",
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
    // per tutta la vita del processo. E si prende **accanto** al mutex, non
    // dentro: questa funzione gira mentre la finestra si costruisce, e mettersi
    // in coda dietro il database per copiare un percorso vorrebbe dire ritardare
    // la comparsa della finestra di quanto dura la prima passata di qualcun
    // altro.
    let stato_app = app.state::<Stato>();
    let Ok(copertine) = stato_app.copertine() else {
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
        mostrata: None,
        rifiutata: None,
    });
}

/// Stacca la scheda del sistema, mentre la finestra c'è ancora.
///
/// # Perché non basta lasciarli cadere da soli
///
/// Perché i controlli stanno in uno stato **gestito**, e uno stato gestito
/// viene lasciato cadere alla fine di tutto — dopo che il ciclo degli eventi si
/// è dichiarato distrutto e `DestroyWindow` ha smontato la finestra. Ma questi
/// controlli non sono un oggetto qualunque: su Windows sono agganciati
/// all'`HWND` della finestra, che `souvlaki` si è fatto dare all'avvio
/// (`GetForWindow`), e il loro `Drop` chiama `detach` su quella finestra. Cioè
/// parla a una finestra che non c'è più.
///
/// Chiamata dal `RunEvent` d'uscita in `main`, il distacco succede quando la
/// finestra è ancora al suo posto e la domanda ha ancora una risposta.
/// Lasciarli cadere una seconda volta dopo non fa niente: il posto è già vuoto.
pub fn stacca(app: &tauri::AppHandle) {
    let Some(stato) = app.try_state::<StatoMedia>() else {
        return;
    };
    let mut dentro = stato
        .controlli
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    // `take` e non `as_mut().detach()`: il `Drop` di `MediaControls` fa già il
    // distacco, e farlo due volte vorrebbe dire fidarsi di come `souvlaki`
    // tratta il secondo. Portarli via è l'unico modo che non dipende da quello.
    drop(dentro.take());
}

/// Rispecchia lo stato del lettore nella scheda del sistema.
///
/// Chiamata da [`crate::riproduzione::manda_stato`] con lo stato **già
/// composto**: comporlo una seconda volta vorrebbe dire una seconda lettura
/// del brano dal database a ogni cambio, e soprattutto due risposte possibili
/// alla stessa domanda.
///
/// Gira sul filo del seguito del lettore, non dentro il suo lucchetto: la
/// consegna della copertina aspetta il sistema, e prima si aspettava con il
/// lettore chiuso. Vedi `crate::riproduzione::in_ordine`.
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
    // Da qui e non dalla scheda del sistema: la scheda la si riempie qualche
    // riga più giù e non la si può rileggere — `souvlaki` prende e non
    // racconta. Questo è lo stesso numero, tenuto dove serve ai salti.
    media.durata_ms.store(stato.durata_ms, Ordering::Relaxed);

    let mut dentro = media
        .controlli
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let Some(controlli) = dentro.as_mut() else {
        return;
    };

    let scheda = scheda_di(stato, &controlli.copertine);
    if controlli.mostrata.as_ref() != Some(&scheda) {
        consegna(controlli, scheda);
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

/// Quel che la scheda deve mostrare per questo stato.
///
/// A coda finita la scheda è **vuota**, non assente: quattro stringhe vuote e
/// la copertina di ripiego. `souvlaki` scrive soltanto i campi che riceve, e
/// `MediaMetadata::default()` — che era quel che si mandava — non ne conteneva
/// nessuno: la scheda restava lì a dire il titolo dell'ultimo brano.
fn scheda_di(stato: &StatoRiproduzione, copertine: &CoverStore) -> Scheda {
    let Some(brano) = stato.brano.as_ref() else {
        return Scheda {
            track_id: None,
            titolo: String::new(),
            artista: String::new(),
            album: String::new(),
            copertina: copertina_di_ripiego(),
            durata_ms: 0,
        };
    };
    let copertina = brano
        .cover_art_hash
        .as_ref()
        .map(|hash| copertine.path_for(hash))
        .filter(|percorso| percorso.exists())
        .and_then(|percorso| url_di_file(&percorso))
        // Un brano senza copertina mostra quella di ripiego, e non è un
        // ornamento: senza, la scheda terrebbe la copertina del brano di prima
        // — `souvlaki` non ha un modo di toglierla.
        .or_else(copertina_di_ripiego);
    Scheda {
        track_id: Some(brano.id),
        titolo: brano.title.clone(),
        artista: brano.artist.clone(),
        album: brano.album.clone(),
        copertina,
        durata_ms: stato.durata_ms,
    }
}

/// Consegna la scheda a Windows, e ricorda com'è andata.
///
/// La copertina si passa solo se è cambiata: con `None` `souvlaki` lascia
/// quella che c'è, ed è l'unico modo di non riaprire lo stesso file a ogni
/// pausa.
///
/// # Quando Windows rifiuta
///
/// `souvlaki` consegna la copertina **prima** del resto e, se il sistema la
/// rifiuta, esce senza consegnare niente: un file che non si apre costava
/// titolo, artista e durata. Qui si riprova subito senza copertina, così la
/// scheda almeno dice cosa suona; e la si dà per consegnata, perché riaprire lo
/// stesso file a ogni stato non lo farebbe aprire meglio.
///
/// Ogni rifiuto si scrive nel diario una volta per scheda: prima veniva
/// scartato in silenzio, ed è così che la scheda vuota è rimasta nascosta per
/// tre versioni.
fn consegna(controlli: &mut Controlli, scheda: Scheda) {
    let copertina_nuova = controlli
        .mostrata
        .as_ref()
        .is_none_or(|prima| prima.copertina != scheda.copertina);
    let copertina = if copertina_nuova {
        scheda.copertina.as_deref()
    } else {
        None
    };
    let metti = |controlli: &mut MediaControls, copertina: Option<&str>| {
        controlli.set_metadata(MediaMetadata {
            title: Some(&scheda.titolo),
            album: Some(&scheda.album),
            artist: Some(&scheda.artista),
            cover_url: copertina,
            duration: Some(Duration::from_millis(scheda.durata_ms)),
        })
    };
    let mut esito = metti(&mut controlli.controlli, copertina);
    if let Err(err) = &esito
        && copertina.is_some()
    {
        if controlli.rifiutata.as_ref() != Some(&scheda) {
            nota!(
                "[media] la scheda di Windows non accetta la copertina del brano {:?}: {err:?}",
                scheda.track_id
            );
        }
        esito = metti(&mut controlli.controlli, None);
    }
    match esito {
        Ok(()) => {
            controlli.mostrata = Some(scheda);
            controlli.rifiutata = None;
        }
        Err(err) => {
            if controlli.rifiutata.as_ref() != Some(&scheda) {
                nota!(
                    "[media] la scheda di Windows non accetta il brano {:?}: {err:?}",
                    scheda.track_id
                );
            }
            // Quel che è stato consegnato a metà non si sa: la prossima volta
            // si riparte da zero, copertina compresa.
            controlli.mostrata = None;
            controlli.rifiutata = Some(scheda);
        }
    }
}

/// La copertina per quando non ce n'è una: l'icona di Rymembra.
///
/// # Perché un file nella cartella temporanea
///
/// Perché la scheda di Windows vuole un file vero — vedi [`StatoMedia`] — e
/// l'icona sta dentro l'eseguibile, non accanto. Si scrive una volta per
/// processo, e si riscrive se nel frattempo qualcuno ha pulito la cartella.
/// Nella cartella temporanea e non accanto al database, perché non è un dato:
/// è un'immagine che l'applicazione sa rifare quando vuole.
///
/// `None` se non si riesce a scriverla: la scheda resta con la copertina che
/// aveva, che è quel che succedeva prima.
fn copertina_di_ripiego() -> Option<String> {
    const ICONA: &[u8] = include_bytes!("../icons/128x128.png");
    static PERCORSO: OnceLock<PathBuf> = OnceLock::new();
    let percorso =
        PERCORSO.get_or_init(|| std::env::temp_dir().join("rymembra-scheda-senza-copertina.png"));
    if !percorso.exists() && rymembra_app::scrittura::scrivi_atomico(percorso, ICONA).is_err() {
        return None;
    }
    url_di_file(percorso)
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

/// L'indirizzo di una copertina, nella forma che `souvlaki` sa aprire.
///
/// # Non è un URL, ed è voluto
///
/// `souvlaki` 0.8.3, su Windows, fa una cosa sola con un indirizzo che
/// comincia per `file://`: **toglie il prefisso** (`trim_start_matches`) e
/// passa quel che resta a `StorageFile::GetFileFromPathAsync`, che vuole un
/// percorso di Windows vero — lettera di unità e barre rovesce. Qui si
/// costruiva un URL da manuale, `file:///C:/…`: tolto il prefisso restava
/// `/C:/…`, che il sistema rifiuta, e con quel rifiuto saltava la scheda
/// intera — vedi il preambolo del modulo.
///
/// La forma giusta è quindi il prefisso seguito dal percorso **così com'è**:
/// `file://C:\Users\…`, che tolto il prefisso torna a essere
/// `C:\Users\…`. Niente codifica percentuale, per la stessa ragione: spazi e
/// lettere accentate devono arrivare al sistema tali e quali.
///
/// Il prefisso `\\?\` dei percorsi lunghi si toglie: `GetFileFromPathAsync`
/// non lo accetta, e per una copertina — che sta in una cartella dell'utente —
/// non serve.
fn url_di_file(percorso: &std::path::Path) -> Option<String> {
    let testo = percorso.to_str()?;
    let testo = if let Some(resto) = testo.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{resto}")
    } else {
        testo.strip_prefix(r"\\?\").unwrap_or(testo).to_owned()
    };
    Some(format!("file://{testo}"))
}

/// Un gesto sulla musica, da eseguire lontano da chi l'ha ricevuto.
///
/// Li mandano i tasti della scheda di Windows, il menù dell'icona nel vassoio
/// e i pulsanti nell'anteprima della barra: vedi [`premi`].
#[derive(Debug, Clone, Copy)]
pub(crate) enum Gesto {
    /// Riprendi.
    Riprendi,
    /// Pausa.
    Pausa,
    /// Pausa se suona, riprendi se è fermo.
    Alterna,
    /// Il brano dopo.
    Prossimo,
    /// Il brano prima, o da capo.
    Precedente,
    /// A questo millisecondo del brano.
    VaiA(u64),
}

/// Esegue un gesto sul filo «rymembra-tasti», dietro a quelli arrivati prima.
///
/// # Perché un filo
///
/// Perché chi riceve il gesto non può aspettare. I tasti della scheda di
/// Windows arrivano su un filo del sistema operativo, dentro il gestore che
/// `souvlaki` gli ha registrato; il menù del vassoio arriva sul filo della
/// finestra. E i comandi che eseguono il gesto possono aspettare parecchio:
/// «prossimo» apre un file, e un brano che arriva dalla rete ha quindici
/// secondi di scadenza. Per tutto quel tempo il sistema restava con un tasto
/// in mano, o la finestra ferma.
///
/// Un filo solo, e non uno per gesto: due tasti premuti di fila — pausa e
/// subito play — devono valere nell'ordine in cui sono stati premuti.
///
/// Parte al primo gesto, non all'avvio: chi non tocca mai un tasto non paga un
/// filo.
pub(crate) fn premi(app: &tauri::AppHandle, gesto: Gesto) {
    let Some(media) = app.try_state::<StatoMedia>() else {
        return;
    };
    let tasti = media.tasti.get_or_init(|| {
        let (manda, ricevi) = std::sync::mpsc::channel::<Gesto>();
        let mano = app.clone();
        let avviato = std::thread::Builder::new()
            .name("rymembra-tasti".to_owned())
            .spawn(move || {
                for gesto in ricevi {
                    esegui(&mano, gesto);
                }
            });
        if avviato.is_err() {
            nota!("[media] il filo dei tasti non è partito: i gesti si eseguono sul posto");
        }
        manda
    });
    // Canale chiuso: il filo non è partito. Si esegue qui, lenti come prima ma
    // non muti.
    if let Err(respinto) = tasti.send(gesto) {
        esegui(app, respinto.0);
    }
}

/// Il gesto, fatto con i comandi che esistono già.
///
/// Ogni ramo passa dai comandi della finestra invece di toccare il lettore:
/// così un tasto della tastiera, una voce del vassoio e il pulsante nella
/// finestra fanno **letteralmente** la stessa cosa, salvataggio della coda e
/// invio dello stato compresi.
fn esegui(app: &tauri::AppHandle, gesto: Gesto) {
    let Some(lettore) = app.try_state::<StatoLettore>() else {
        return;
    };
    let esito = match gesto {
        Gesto::Riprendi => crate::riproduzione::riprendi_ora(app.clone(), lettore),
        Gesto::Pausa => crate::riproduzione::pausa(app.clone(), lettore),
        Gesto::Alterna => crate::riproduzione::alterna_ora(app.clone(), lettore),
        Gesto::Prossimo => crate::riproduzione::prossimo_ora(app.clone(), lettore),
        Gesto::Precedente => crate::riproduzione::precedente_ora(app.clone(), lettore),
        Gesto::VaiA(ms) => crate::riproduzione::vai_a(app.clone(), lettore, ms),
    };
    if let Err(err) = esito {
        nota!(
            "[media] {} causa={}",
            err.code,
            err.cause.as_deref().unwrap_or_default()
        );
        // Anche alla finestra, come quando il gesto parte da lì: un «prossimo»
        // dal vassoio su un file sparito fermava tutto, e il perché finiva
        // solo nel diario. Chi ha premuto un tasto multimediale, una voce del
        // vassoio o un pulsante della miniatura non ha un comando a cui
        // l'errore risalga: la fascia è l'unico posto in cui leggerlo.
        app.emetti("riproduzione:errore", err);
    }
}

/// Un tasto premuto, o un comando arrivato dalla scheda del sistema.
///
/// Gira sul filo del sistema operativo che ha ricevuto il tasto, e ci resta il
/// meno possibile: traduce il tasto in un [`Gesto`] e lo consegna a [`premi`].
/// I conti dei salti relativi si fanno qui, perché leggono due atomici e basta.
fn su_evento(app: &tauri::AppHandle, evento: MediaControlEvent) {
    let gesto = match evento {
        MediaControlEvent::Play => Gesto::Riprendi,
        MediaControlEvent::Pause => Gesto::Pausa,
        MediaControlEvent::Toggle => Gesto::Alterna,
        MediaControlEvent::Next => Gesto::Prossimo,
        MediaControlEvent::Previous => Gesto::Precedente,
        // `Stop` come pausa, di proposito: fermare davvero butterebbe la
        // posizione, e il tasto stop di una tastiera lo si preme per
        // interrompere, non per perdere il segno.
        MediaControlEvent::Stop => Gesto::Pausa,
        // Anche questo passa dal tetto, che pure arriva già come posizione
        // assoluta: il numero lo sceglie il sistema operativo — una barra
        // trascinata nel riquadro del volume, un'auto collegata in Bluetooth —
        // e la durata che quel sistema crede di avere è quella dell'ultimo
        // [`aggiorna`], che può essere del brano di prima.
        MediaControlEvent::SetPosition(MediaPosition(dove)) => {
            Gesto::VaiA(dentro_il_brano(app, ms_di(dove)))
        }
        // Un «avanti» già dentro l'ultimo tratto del brano non salta: vedi
        // [`avanti_entro`].
        MediaControlEvent::Seek(verso) => match salto(app, verso, PASSO_SALTO_MS) {
            Some(ms) => Gesto::VaiA(ms),
            None => return,
        },
        MediaControlEvent::SeekBy(verso, quanto) => match salto(app, verso, ms_di(quanto)) {
            Some(ms) => Gesto::VaiA(ms),
            None => return,
        },
        // Il volume del sistema non è il volume del lettore, e `OpenUri`
        // chiederebbe di suonare qualcosa che non è in libreria. `Raise` e
        // `Quit` sono gesti della finestra che la scheda non deve avere: da
        // lì si comanda la musica, non il programma.
        MediaControlEvent::SetVolume(_)
        | MediaControlEvent::OpenUri(_)
        | MediaControlEvent::Raise
        | MediaControlEvent::Quit => return,
    };
    premi(app, gesto);
}

/// Dove finisce un salto relativo, senza uscire dal brano. `None`: nessun
/// salto da fare.
///
/// I due versi non sono simmetrici, e non è una svista: indietro il fondo è lo
/// zero, e `saturating_sub` ce lo tiene da sé. Avanti il fondo è la fine del
/// brano, che `saturating_add` non conosce — protegge dal trabocco di un `u64`,
/// cioè da un brano lungo cinquecento milioni di anni, non dalla fine di questo
/// — e la tiene [`avanti_entro`], con un margine: un «avanti di cinque
/// secondi» premuto a tre secondi dalla fine non è una richiesta di cambiare
/// canzone.
fn salto(app: &tauri::AppHandle, verso: SeekDirection, quanto_ms: u64) -> Option<u64> {
    let media = app.state::<StatoMedia>();
    let adesso = media.posizione_ms.load(Ordering::Relaxed);
    match verso {
        SeekDirection::Forward => {
            avanti_entro(adesso, quanto_ms, media.durata_ms.load(Ordering::Relaxed))
        }
        SeekDirection::Backward => Some(adesso.saturating_sub(quanto_ms)),
    }
}

/// Lo stesso millisecondo, ma non oltre la fine del brano.
///
/// # Perché un tetto anche qui
///
/// Il tetto vero sta nel decodificatore e vale per tutti; questo sta prima, e
/// serve a non chiedergli un numero che sta fuori dalla barra del brano: una
/// posizione assoluta mandata dal sistema operativo — la barra trascinata nel
/// riquadro del volume, un'auto collegata in Bluetooth — può parlare di una
/// durata vecchia, quella del brano di prima. Non trattiene nel brano: una
/// posizione sulla fine è la fine, e il brano dopo parte — che è quel che
/// chiede chi trascina la barra fino in fondo. A trattenere i salti relativi è
/// [`avanti_entro`], con il suo margine.
///
/// Con durata sconosciuta non si limita niente: `0` significa che nessun brano
/// è ancora partito, e trattarlo come una lunghezza vera manderebbe ogni salto
/// all'inizio.
fn dentro_il_brano(app: &tauri::AppHandle, ms: u64) -> u64 {
    let durata = app.state::<StatoMedia>().durata_ms.load(Ordering::Relaxed);
    entro_la_durata(ms, durata)
}

/// La regola di [`dentro_il_brano`], senza il posto da cui viene la durata.
///
/// Una funzione a sé unicamente perché si possa provare: leggere `durata_ms`
/// vuole un `AppHandle`, e un `AppHandle` vuole un ciclo degli eventi. La riga
/// è la stessa di prima, spostata di un livello.
fn entro_la_durata(ms: u64, durata: u64) -> u64 {
    if durata == 0 { ms } else { ms.min(durata) }
}

/// Quanto prima della fine si ferma un «avanti» del sistema.
///
/// Un salto sulla fine è la fine: il decodificatore lo ferma poco prima
/// dell'ultimo fotogramma (`rymembra_play`, `MARGINE_FINE_MS` del
/// decodificatore: centoventi millisecondi), e da lì il brano finisce e la coda
/// avanza. Un secondo e mezzo lascia sentire che si è arrivati in fondo — e
/// basta un altro «avanti», o il brano che finisce da sé, per passare oltre.
const MARGINE_FINE_MS: u64 = 1_500;

/// Dove finisce un «avanti» di `quanto` da `adesso`, in un brano lungo
/// `durata`: mai oltre [`MARGINE_FINE_MS`] dalla fine, e mai indietro.
///
/// `None` vuol dire **nessun salto**: si è già dentro l'ultimo tratto, e un
/// salto sul punto in cui si è già non è gratis — azzera il decodificatore e,
/// su una condivisione, rilegge. Con la durata sconosciuta (`0`, vedi
/// [`StatoMedia::durata_ms`]) non si limita niente.
fn avanti_entro(adesso: u64, quanto: u64, durata: u64) -> Option<u64> {
    let voluto = adesso.saturating_add(quanto);
    if durata == 0 {
        return Some(voluto);
    }
    let tetto = durata.saturating_sub(MARGINE_FINE_MS);
    if adesso >= tetto {
        return None;
    }
    Some(voluto.min(tetto))
}

/// I millisecondi di una durata, senza traboccare.
fn ms_di(durata: Duration) -> u64 {
    u64::try_from(durata.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Quel che `souvlaki` 0.8.3 fa di un indirizzo `file://` su Windows,
    /// riga per riga (`platform/windows/mod.rs`, `set_metadata`): se comincia
    /// per `file://` toglie il prefisso e passa il resto a
    /// `StorageFile::GetFileFromPathAsync`. `None` se non comincia così — lì
    /// `souvlaki` prende un'altra strada, `Uri::CreateUri`, che per un file
    /// locale non serve a niente.
    fn percorso_che_souvlaki_consegna(url: &str) -> Option<&str> {
        url.starts_with("file://")
            .then(|| url.trim_start_matches("file://"))
    }

    #[test]
    fn souvlaki_consegna_a_windows_il_percorso_della_copertina_tale_e_quale() {
        // Il difetto che la prova tiene chiuso: `file:///C:/…` diventava
        // `/C:/…`, che Windows rifiuta, e con lui saltava la scheda intera.
        // Quel che arriva al sistema dev'essere il percorso di partenza,
        // lettera di unità e barre rovesce comprese.
        let percorso = r"C:\Users\ada\AppData\Roaming\rymembra\copertine\ab\abcdef.jpg";
        let url = url_di_file(std::path::Path::new(percorso)).expect("un percorso UTF-8");
        assert_eq!(url, format!("file://{percorso}"));
        assert_eq!(percorso_che_souvlaki_consegna(&url), Some(percorso));
    }

    #[test]
    fn il_percorso_consegnato_e_assoluto_per_windows() {
        // `GetFileFromPathAsync` vuole un percorso assoluto: è la proprietà che
        // `/C:/…` perdeva. Su Windows lo sa dire `Path::is_absolute`.
        let url = url_di_file(std::path::Path::new(r"D:\Musica\copertina.jpg"))
            .expect("un percorso UTF-8");
        let consegnato = percorso_che_souvlaki_consegna(&url).expect("comincia per file://");
        assert!(
            !consegnato.starts_with('/'),
            "resta la barra davanti: {consegnato}"
        );
        #[cfg(windows)]
        assert!(
            std::path::Path::new(consegnato).is_absolute(),
            "Windows non lo prende per assoluto: {consegnato}"
        );
    }

    #[test]
    fn gli_spazi_e_i_caratteri_non_ascii_si_lasciano_stare() {
        // Niente codifica percentuale: `souvlaki` non la toglie, e un `%20`
        // arriverebbe a Windows come tre caratteri veri di un nome che non
        // esiste. Spazi e lettere accentate passano tali e quali.
        let percorso = r"D:\Musica mia\Björk\è così.png";
        let url = url_di_file(std::path::Path::new(percorso)).expect("un percorso UTF-8");
        assert_eq!(percorso_che_souvlaki_consegna(&url), Some(percorso));
    }

    #[test]
    fn un_percorso_lungo_perde_il_prefisso_che_windows_non_accetta() {
        let url = url_di_file(std::path::Path::new(r"\\?\C:\copertine\ab.jpg"))
            .expect("un percorso UTF-8");
        assert_eq!(
            percorso_che_souvlaki_consegna(&url),
            Some(r"C:\copertine\ab.jpg")
        );
        let rete = url_di_file(std::path::Path::new(r"\\?\UNC\nas\dati\ab.jpg"))
            .expect("un percorso UTF-8");
        assert_eq!(
            percorso_che_souvlaki_consegna(&rete),
            Some(r"\\nas\dati\ab.jpg")
        );
    }

    #[test]
    fn una_durata_sconosciuta_non_mette_nessun_tetto() {
        // `0` vuol dire «non si sa», non «lungo zero»: con un tetto a zero ogni
        // salto in avanti tornerebbe all'inizio del brano.
        assert_eq!(entro_la_durata(93_000, 0), 93_000);
        assert_eq!(entro_la_durata(0, 0), 0);
    }

    #[test]
    fn un_salto_oltre_la_fine_si_ferma_alla_fine() {
        // Una posizione assoluta del sistema non chiede al motore un numero
        // fuori dalla barra: si ferma alla fine. Non trattiene nel brano — una
        // posizione sulla fine è la fine, e il brano dopo parte — e per gli
        // «avanti» relativi il margine lo mette `avanti_entro`.
        assert_eq!(entro_la_durata(200_000, 187_000), 187_000);
        assert_eq!(entro_la_durata(187_000, 187_000), 187_000);
        // Dentro al brano non si tocca niente.
        assert_eq!(entro_la_durata(12_500, 187_000), 12_500);
    }

    /// «Avanti» vicino alla fine resta nel brano: atterra prima dell'ultimo
    /// tratto, mai sulla fine — che è il brano dopo — e mai indietro.
    #[test]
    fn un_avanti_vicino_alla_fine_resta_nel_brano() {
        let dove = avanti_entro(184_000, 5_000, 187_000).expect("c'è spazio per saltare");
        assert!(
            (184_000..=187_000 - MARGINE_FINE_MS).contains(&dove),
            "il salto è finito sulla fine del brano, o indietro: {dove}"
        );
        // Già dentro il margine: nessun salto, che azzererebbe il
        // decodificatore per tornare dov'era.
        assert_eq!(avanti_entro(186_500, 5_000, 187_000), None);
        // Lontano dalla fine non cambia niente.
        assert_eq!(avanti_entro(12_000, 5_000, 187_000), Some(17_000));
        // Con la durata sconosciuta non si limita niente.
        assert_eq!(avanti_entro(93_000, 5_000, 0), Some(98_000));
    }

    #[test]
    fn una_durata_troppo_grande_satura_invece_di_traboccare() {
        assert_eq!(ms_di(Duration::from_millis(0)), 0);
        assert_eq!(ms_di(Duration::from_secs(5)), 5_000);
        // `Duration::as_millis` è un `u128`: il massimo non ci sta in un `u64`,
        // e la conversione deve fermarsi lì invece di girare intorno.
        assert_eq!(ms_di(Duration::MAX), u64::MAX);
    }
}
