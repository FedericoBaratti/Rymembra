//! I controlli multimediali del sistema operativo.
//!
//! # Cosa mancava
//!
//! Rymembra non parlava in nessun modo con Windows. Fra i comandi registrati in
//! [`crate::run`] non ce n'era uno che uscisse dalla finestra: il tasto
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

//!
//! # Due sistemi, un modulo
//!
//! Quel che vale ovunque sta qui: lo stato che i gesti leggono (dove sta il
//! brano, quanto dura), il filo che li esegue in ordine, e la regola dei salti
//! che non escono dal brano. Quel che parla col sistema operativo sta in
//! [`sistema`], che su Windows è `smtc.rs` — le SMTC via `souvlaki` — e su
//! Android `android.rs`, la sessione multimediale. Tutte e due consegnano i
//! tasti a [`premi`], e tutte e due ricevono lo stato da [`aggiorna`].

#[cfg_attr(desktop, path = "smtc.rs")]
#[cfg_attr(mobile, path = "android.rs")]
mod sistema;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tauri::Manager as _;

use crate::nota;
use crate::riproduzione::{StatoLettore, StatoRiproduzione};
use crate::spegnimento::Emette as _;

pub use sistema::{avvia, stacca};

/// Di quanto si sposta un `Seek` senza durata.
///
/// Gli stessi cinque secondi delle frecce in [`crate::riproduzione`]: il gesto
/// è lo stesso, e due passi diversi per la stessa intenzione si notano.
#[cfg_attr(
    mobile,
    allow(dead_code, reason = "Android manda i salti già misurati")
)]
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
    controlli: Mutex<Option<sistema::Controlli>>,
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

    sistema::aggiorna(&media, stato);
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

/// Un gesto sulla musica, da eseguire lontano da chi l'ha ricevuto.
///
/// Li mandano i tasti della scheda di Windows, il menù dell'icona nel vassoio
/// e i pulsanti nell'anteprima della barra: vedi [`premi`].
///
/// Su Android anche quelli che partono dall'auto ([`Gesto::Auto`]): «suona questo
/// album» deve stare in fila con il «riprendi» che l'auto manda subito dopo.
#[derive(Debug, Clone)]
#[cfg_attr(desktop, derive(Copy))]
pub(crate) enum Gesto {
    /// Riprendi.
    Riprendi,
    /// Pausa.
    Pausa,
    /// Pausa se suona, riprendi se è fermo.
    #[cfg_attr(
        mobile,
        allow(
            dead_code,
            reason = "Media3 manda «riproduci» e «pausa», mai «alterna»"
        )
    )]
    Alterna,
    /// Il brano dopo.
    Prossimo,
    /// Il brano prima, o da capo.
    Precedente,
    /// A questo millisecondo del brano.
    VaiA(u64),
    /// Un gesto dello schermo dell'auto: suona una voce, cerca, salta nella
    /// coda, shuffle, ripeti, mi piace. Vedi [`crate::auto`].
    #[cfg(mobile)]
    Auto(crate::auto::GestoAuto),
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
        #[cfg(mobile)]
        Gesto::Auto(gesto) => crate::auto::esegui(app, gesto),
    };
    if let Err(err) = esito {
        // Chi ha chiesto dallo schermo dell'auto non vede la fascia della
        // finestra: glielo si dice anche là.
        #[cfg(mobile)]
        sistema::non_riuscito(&err.code);
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
#[cfg_attr(
    mobile,
    allow(dead_code, reason = "le durate di `souvlaki`: solo sul computer")
)]
fn ms_di(durata: Duration) -> u64 {
    u64::try_from(durata.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod prove {
    use super::*;
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
