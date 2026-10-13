//! L'icona nell'area di notifica, e la X che non spegne.
//!
//! # Cosa cambia, e cosa no
//!
//! Con la preferenza accesa il tasto di chiusura **nasconde** la finestra invece
//! di far uscire il processo: la musica continua, perché non è mai stata nella
//! finestra. Sta tutta in `aether-play`, nei fili `aether-uscita` e
//! `aether-decodifica`, e la pagina non contiene un solo `<audio>` — non c'è
//! niente, dalla parte della finestra, che nascondendola si fermi.
//!
//! Il tasto «riduci a icona» resta quel che era. Chi lo preme sta chiedendo di
//! togliere di mezzo la finestra e sa dove ritrovarla, sulla barra delle
//! applicazioni; chi preme la X sta chiedendo un'altra cosa, ed è la sola su cui
//! questa preferenza ha un'opinione.
//!
//! # Perché serve un'icona, e non è un ornamento
//!
//! Una finestra nascosta senza un modo di tornare è un programma perso: resta
//! nell'elenco dei processi, continua a suonare, e l'unico modo di riprenderlo è
//! il Gestione attività. L'icona **è** la via di ritorno, ed è per questo che
//! [`nasconde`] non chiede soltanto se la preferenza è accesa: chiede se
//! l'icona c'è davvero. Se il sistema l'ha rifiutata, o se le etichette del
//! menù non sono ancora scese dalla finestra, la X torna a chiudere — che è il
//! guasto giusto fra i due possibili.
//!
//! # Perché le etichette arrivano dalla finestra
//!
//! «Mostra Aether» ed «Esci» sono due testi che l'utente legge, e tutti i testi
//! che l'utente legge stanno in `apps/desktop/src/lingue/`, dove
//! `strumenti/lingue.js` controlla che l'italiano e l'inglese abbiano le stesse
//! chiavi. Scrivendoli qui sarebbero gli unici due fuori da quel controllo — e
//! il primo sintomo sarebbe un menù metà in una lingua e metà nell'altra, che
//! nessuno segnala perché sembra una svista di traduzione e non un difetto.
//!
//! La finestra li manda con [`vassoio_lingua`] appena ha applicato la lingua, e
//! di nuovo a ogni cambio.
//!
//! # La musica dal vassoio
//!
//! Dalla 3.0 il menù ha anche le tre voci della musica — riproduci o pausa,
//! successivo, precedente — e l'icona dice cosa sta suonando, nel suggerimento
//! che compare passandoci sopra. Chi tiene Aether in secondo piano la musica la
//! comanda da lì, senza riaprire la finestra.
//!
//! Le voci passano dagli stessi comandi dei tasti della finestra, attraverso
//! [`crate::media::premi`]: fanno esattamente quel che fa il pulsante, e lo
//! fanno su un filo loro, perché il menù arriva sul filo della finestra e
//! «successivo» può dover aprire un file dalla rete. Etichetta e suggerimento
//! li tiene in pari [`riflette`], chiamata da chi manda lo stato alla finestra:
//! una sorgente sola, come per la scheda di Windows.
//!
//! # Perché uno specchio atomico della preferenza
//!
//! Perché a leggerla è il gestore della chiusura, che gira dentro il ciclo degli
//! eventi e deve rispondere **in quel momento** se la finestra va chiusa o
//! nascosta. Chiedere lì il lucchetto della libreria vorrebbe dire poter
//! aspettare la scansione in corso, con la finestra ferma a metà di una
//! chiusura. La verità resta nel database; questa è la copia che si legge di
//! corsa, riallineata a ogni scrittura e una volta all'avvio. È la stessa
//! ragione per cui `Stato::scansione_da_fermare` è un atomico accanto al mutex
//! e non un campo dentro.

use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::Manager as _;
use tauri::State;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::errore::{Esito, errore};
use crate::media::Gesto;
use crate::nota;
use crate::riproduzione::StatoRiproduzione;
use crate::stato::{Stato, con_libreria};

/// L'identificativo della voce che riapre la finestra.
const VOCE_MOSTRA: &str = "vassoio.mostra";

/// L'identificativo della voce che esce davvero.
const VOCE_ESCI: &str = "vassoio.esci";

/// L'identificativo della voce che mette in pausa o fa ripartire.
const VOCE_ALTERNA: &str = "vassoio.alterna";

/// L'identificativo della voce del brano dopo.
const VOCE_SUCCESSIVO: &str = "vassoio.successivo";

/// L'identificativo della voce del brano prima.
const VOCE_PRECEDENTE: &str = "vassoio.precedente";

/// Il suggerimento dell'icona quando non suona niente.
///
/// Il nome del programma, non tradotto: la stessa scelta di `display_name` in
/// `media.rs`.
const SUGGERIMENTO_A_RIPOSO: &str = "Aether";

/// Quanti caratteri del suggerimento si tengono.
///
/// Windows ne tiene centoventisette in UTF-16, e taglia da sé quel che avanza
/// — a metà di una lettera, se la lettera ne occupa due. Cento caratteri
/// stanno comunque sotto, con il margine per i caratteri che in UTF-16 ne
/// valgono due; titolo e interprete che li superano insieme sono rari, e il
/// resto lo dice la finestra.
const SUGGERIMENTO_MASSIMO: usize = 100;

/// Le etichette del menù, come le manda la finestra.
#[derive(Clone)]
struct Etichette {
    /// «Mostra Aether».
    mostra: String,
    /// «Esci».
    esci: String,
    /// Le tre voci della musica. `None` se la finestra non le ha mandate — una
    /// finestra di prima della 3.0 — e allora il menù resta quello di prima.
    trasporto: Option<Trasporto>,
}

/// Le etichette delle voci della musica.
#[derive(Clone)]
struct Trasporto {
    /// «Riproduci», quando la musica è ferma.
    riproduci: String,
    /// «Pausa», quando suona.
    pausa: String,
    /// «Successivo».
    successivo: String,
    /// «Precedente».
    precedente: String,
}

/// Quel che l'icona racconta della musica.
#[derive(Clone, PartialEq, Eq)]
struct InCorso {
    /// «Titolo — Interprete», o il nome del programma a musica ferma.
    suggerimento: String,
    /// La musica sta andando: la prima voce dice «Pausa».
    suona: bool,
}

impl InCorso {
    const fn a_riposo() -> Self {
        Self {
            suggerimento: String::new(),
            suona: false,
        }
    }

    /// Il suggerimento da mostrare: quello a riposo se non c'è niente.
    fn testo(&self) -> &str {
        if self.suggerimento.is_empty() {
            SUGGERIMENTO_A_RIPOSO
        } else {
            &self.suggerimento
        }
    }
}

/// Quel che serve a sapere se c'è un'icona, e a rifarla.
pub struct StatoVassoio {
    /// Lo specchio della preferenza — vedi il preambolo.
    attivo: AtomicBool,
    /// L'icona esiste davvero.
    ///
    /// # Perché un atomico e non `icona.lock().is_some()`
    ///
    /// Perché a leggerlo è [`nasconde`], che gira sul **filo principale**,
    /// dentro il gestore della chiusura. Il lucchetto qui sotto può essere in
    /// mano a un comando che sta costruendo l'icona, e costruirla vuol dire
    /// chiedere al filo principale di farlo e aspettare la risposta: il filo
    /// principale fermo su quel lucchetto sarebbe il filo principale che
    /// aspetta se stesso, cioè una finestra che non risponde più a niente.
    ///
    /// Serve una premessa in meno di quanto sembri: fra l'accensione della
    /// preferenza e l'icona che compare, `presente` è ancora falso e la X
    /// chiude — che è già la risposta giusta, perché non c'è ancora niente a
    /// cui tornare.
    presente: AtomicBool,
    /// Le etichette, o `None` finché la finestra non le ha mandate.
    etichette: Mutex<Option<Etichette>>,
    /// L'icona viva, o `None` se non ce n'è.
    ///
    /// `TrayIcon` è contato: l'icona sparisce dall'area di notifica quando cade
    /// l'ultima copia, e non c'è nessun «rimuovi» da chiamare. Togliere il
    /// valore da qui è quindi tutto quel che serve per farla sparire — **quando**
    /// toglierlo è l'altra metà, e sta su [`stacca`].
    icona: Mutex<Option<TrayIcon>>,
    /// La voce «Riproduci»/«Pausa» del menù in uso, per cambiarle etichetta.
    ///
    /// Si rifà insieme al menù: un menù nuovo — la lingua cambiata — ha voci
    /// nuove, e questa si riscrive nello stesso punto.
    alterna: Mutex<Option<MenuItem<tauri::Wry>>>,
    /// Quel che l'icona deve raccontare adesso. Lo scrive [`riflette`], lo
    /// legge chi costruisce l'icona o il menù.
    in_corso: Mutex<InCorso>,
}

impl StatoVassoio {
    /// Uno stato senza icona e senza etichette.
    #[must_use]
    pub const fn nuovo() -> Self {
        Self {
            attivo: AtomicBool::new(false),
            presente: AtomicBool::new(false),
            etichette: Mutex::new(None),
            icona: Mutex::new(None),
            alterna: Mutex::new(None),
            in_corso: Mutex::new(InCorso::a_riposo()),
        }
    }
}

/// La X deve nascondere invece di chiudere?
///
/// Vera solo se la preferenza è accesa **e** l'icona esiste. La seconda metà non
/// è una precauzione teorica: è la differenza fra una finestra che si può
/// riaprire e un processo che continua a suonare senza niente a cui tornare.
///
/// Due atomici e nessun lucchetto, perché la risposta serve sul filo principale
/// mentre una chiusura è a metà — la ragione per esteso sta su
/// [`StatoVassoio::presente`].
pub fn nasconde(app: &tauri::AppHandle) -> bool {
    let Some(stato) = app.try_state::<StatoVassoio>() else {
        return false;
    };
    stato.attivo.load(Ordering::Relaxed) && stato.presente.load(Ordering::Relaxed)
}

/// Riporta la finestra davanti.
///
/// Tutti e tre i gesti, e in quest'ordine: una finestra nascosta **e** ridotta a
/// icona torna visibile con `show()` e resta ridotta, cioè si riaprirebbe in un
/// modo che somiglia molto al non essersi riaperta.
///
/// # Perché non è più privata
///
/// Perché ha un secondo chiamante fuori da qui, e fa la stessa cosa per la stessa
/// ragione: [`crate::una_sola_istanza`], quando qualcuno riapre Aether mentre
/// Aether è già aperta. Quel gesto e il clic sull'icona nel vassoio sono la
/// stessa richiesta — «rimettimi davanti la finestra» — e due funzioni che la
/// soddisfano sono due funzioni che il giorno in cui i gesti giusti diventano
/// quattro ne fanno tre.
pub(crate) fn mostra(app: &tauri::AppHandle) {
    let Some(finestra) = app.get_webview_window("main") else {
        return;
    };
    let _ = finestra.show();
    let _ = finestra.unminimize();
    let _ = finestra.set_focus();
}

/// Esce davvero.
///
/// La bandiera **prima** dell'uscita: il gestore in [`crate::main`] la legge per
/// decidere se impedire la chiusura, e trovandola alzata si tira da parte. In
/// quest'ordine «Esci» funziona qualunque sia l'ordine interno con cui Tauri
/// manda i suoi eventi, che non è una cosa su cui valga la pena scommettere.
fn esci(app: &tauri::AppHandle) {
    crate::spegnimento::chiedi();
    app.exit(0);
}

/// Il menù, costruito con le etichette che sono arrivate.
///
/// Restituisce anche la voce «Riproduci»/«Pausa», se c'è, perché
/// [`riflette`] le possa cambiare etichetta senza rifare il menù.
fn costruisci_menu(
    app: &tauri::AppHandle,
    etichette: &Etichette,
    in_corso: &InCorso,
) -> tauri::Result<(Menu<tauri::Wry>, Option<MenuItem<tauri::Wry>>)> {
    let mostra = MenuItem::with_id(app, VOCE_MOSTRA, &etichette.mostra, true, None::<&str>)?;
    let esci = MenuItem::with_id(app, VOCE_ESCI, &etichette.esci, true, None::<&str>)?;
    let Some(trasporto) = etichette.trasporto.as_ref() else {
        return Ok((Menu::with_items(app, &[&mostra, &esci])?, None));
    };
    let alterna = MenuItem::with_id(
        app,
        VOCE_ALTERNA,
        etichetta_alterna(trasporto, in_corso.suona),
        true,
        None::<&str>,
    )?;
    let successivo = MenuItem::with_id(
        app,
        VOCE_SUCCESSIVO,
        &trasporto.successivo,
        true,
        None::<&str>,
    )?;
    let precedente = MenuItem::with_id(
        app,
        VOCE_PRECEDENTE,
        &trasporto.precedente,
        true,
        None::<&str>,
    )?;
    let separatore = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &alterna,
            &successivo,
            &precedente,
            &separatore,
            &mostra,
            &esci,
        ],
    )?;
    Ok((menu, Some(alterna)))
}

/// L'etichetta della prima voce: quel che succede premendola.
fn etichetta_alterna(trasporto: &Trasporto, suona: bool) -> &str {
    if suona {
        &trasporto.pausa
    } else {
        &trasporto.riproduci
    }
}

/// Mette l'icona nell'area di notifica.
fn costruisci(
    app: &tauri::AppHandle,
    menu: &Menu<tauri::Wry>,
    suggerimento: &str,
) -> tauri::Result<TrayIcon> {
    let mut costruttore = TrayIconBuilder::new()
        // Quel che sta suonando, o il nome del programma: vedi [`InCorso`].
        .tooltip(suggerimento)
        .menu(menu)
        // Il menù sul tasto destro soltanto. Il sinistro riapre la finestra,
        // che è quel che si vuole quasi sempre: chi cerca l'icona la cerca per
        // tornare, non per leggere due voci.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, evento| match evento.id.as_ref() {
            VOCE_MOSTRA => mostra(app),
            VOCE_ESCI => esci(app),
            // Il menù arriva sul filo della finestra: i gesti si eseguono
            // altrove. Vedi il preambolo.
            VOCE_ALTERNA => crate::media::premi(app, Gesto::Alterna),
            VOCE_SUCCESSIVO => crate::media::premi(app, Gesto::Prossimo),
            VOCE_PRECEDENTE => crate::media::premi(app, Gesto::Precedente),
            _ => {}
        })
        .on_tray_icon_event(|icona, evento| {
            // Al rilascio e non alla pressione: è come si comporta ogni altra
            // icona dell'area di notifica, e un click che agisce sul premuto si
            // sente come un anticipo.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = evento
            {
                mostra(icona.app_handle());
            }
        });

    // L'icona della finestra, che è già dentro l'eseguibile: chiederne una a
    // parte vorrebbe dire accendere una feature per decodificare PNG e
    // impacchettare una seconda immagine che deve restare uguale alla prima.
    if let Some(immagine) = app.default_window_icon() {
        costruttore = costruttore.icon(immagine.clone());
    }

    costruttore.build(app)
}

/// Rifà l'icona a partire da com'è lo stato adesso.
///
/// Due fatti soli la decidono — la preferenza è accesa? le etichette sono
/// arrivate? — e questa funzione è l'unico posto che li legge insieme. Ogni
/// altro punto del modulo cambia uno dei due e poi chiama qui, invece di
/// costruire o distruggere per conto suo: due strade per accendere la stessa
/// icona sono due strade per accenderne due.
fn riallinea(app: &tauri::AppHandle) {
    let Some(stato) = app.try_state::<StatoVassoio>() else {
        return;
    };

    // Le etichette si copiano prima di prendere il lucchetto dell'icona: due
    // lucchetti annidati sono un ordine da rispettare per sempre, e qui non
    // serve a niente.
    let etichette = stato
        .etichette
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();

    let acceso = stato.attivo.load(Ordering::Relaxed);
    // Come le etichette: una copia, presa prima dell'altro lucchetto.
    let in_corso = stato
        .in_corso
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let mut posto = stato.icona.lock().unwrap_or_else(PoisonError::into_inner);

    let ci_vuole = deve_esserci(acceso, etichette.as_ref());
    let Some(etichette) = etichette.filter(|_| ci_vuole) else {
        // Spenta, o etichette non ancora arrivate: via l'icona. Cade qui
        // l'ultima copia, e l'area di notifica torna com'era.
        stato.presente.store(false, Ordering::Relaxed);
        drop(posto.take());
        *stato.alterna.lock().unwrap_or_else(PoisonError::into_inner) = None;
        return;
    };

    let (menu, alterna) = match costruisci_menu(app, &etichette, &in_corso) {
        Ok(fatto) => fatto,
        Err(err) => {
            nota!("[vassoio] il menù non si costruisce: {err}");
            return;
        }
    };
    *stato.alterna.lock().unwrap_or_else(PoisonError::into_inner) = alterna;

    if let Some(icona) = posto.as_ref() {
        // C'è già: cambiano solo le etichette, e rifarla farebbe lampeggiare
        // l'area di notifica a ogni cambio di lingua.
        if let Err(err) = icona.set_menu(Some(menu)) {
            nota!("[vassoio] il menù non si sostituisce: {err}");
        }
        return;
    }

    match costruisci(app, &menu, in_corso.testo()) {
        Ok(icona) => {
            *posto = Some(icona);
            // **Dopo** la costruzione, e non prima: fra le due cose c'è un giro
            // sul filo principale, e in mezzo la X deve ancora chiudere.
            stato.presente.store(true, Ordering::Relaxed);
        }
        // Un'icona che il sistema rifiuta lascia l'applicazione intera, e
        // `nasconde` se ne accorge da sé: senza icona la X torna a chiudere,
        // invece di nascondere una finestra da cui non si tornerebbe.
        Err(err) => nota!("[vassoio] il sistema non concede l'icona: {err}"),
    }
}

/// L'icona deve esserci?
///
/// I due soli fatti che [`riallinea`] mette insieme: lo specchio della preferenza
/// e le etichette scese dalla finestra. Una funzione a sé, e non la riga dentro
/// `riallinea`, per una ragione sola — che questa verità a due fonti si possa
/// provare senza un ciclo degli eventi. Tutto il resto di `riallinea` parla col
/// sistema operativo e resta senza prove; questa metà no, perché sbagliata
/// lascia l'utente con una finestra nascosta e nessun modo di tornarci.
fn deve_esserci(acceso: bool, etichette: Option<&Etichette>) -> bool {
    acceso && etichette.is_some()
}

/// Rispecchia la preferenza e rifà l'icona di conseguenza.
fn ricorda(app: &tauri::AppHandle, attivo: bool) {
    if let Some(stato) = app.try_state::<StatoVassoio>() {
        stato.attivo.store(attivo, Ordering::Relaxed);
    }
    riallinea(app);
}

/// Toglie l'icona mentre il ciclo degli eventi c'è ancora.
///
/// # Perché non basta lasciarla cadere da sola
///
/// La stessa ragione di [`crate::media::stacca`], e vale la pena ripeterla:
/// uno stato gestito viene lasciato cadere alla **fine di tutto**, dopo che il
/// ciclo degli eventi si è dichiarato distrutto. Il `Drop` di un'icona del
/// vassoio però parla al sistema operativo — toglie la voce dall'area di
/// notifica e smonta la finestrella nascosta che le fa da bersaglio — e lo fa
/// sul filo principale, che a quel punto sta smontando se stesso. Chiamata
/// dall'uscita in `main`, l'icona se ne va quando c'è ancora tutto quel che
/// serve a farla andare via.
///
/// # Perché `try_lock`
///
/// Perché qui si è sul filo principale, e il lucchetto potrebbe essere in mano
/// a un comando che sta aspettando **questo** filo per costruire un'icona.
/// Aspettarlo sarebbe l'unico modo di trasformare un'uscita in un blocco. Se
/// non si prende, l'icona cade da sé più tardi: è quel che succedeva prima che
/// questa funzione esistesse, e non è peggio di così.
pub fn stacca(app: &tauri::AppHandle) {
    let Some(stato) = app.try_state::<StatoVassoio>() else {
        return;
    };
    stato.presente.store(false, Ordering::Relaxed);
    if let Ok(mut posto) = stato.icona.try_lock() {
        drop(posto.take());
    }
}

/// Legge la preferenza dal database e la rispecchia. Chiamata all'avvio.
///
/// Non costruisce niente da sé: le etichette non sono ancora arrivate, e
/// [`riallinea`] lo sa. L'icona compare quando la finestra manda i suoi due
/// testi, che è qualche decina di millisecondi dopo.
pub fn avvia(app: &tauri::AppHandle) {
    let stato_app = app.state::<Stato>();
    let letto = con_libreria(&stato_app, |libreria| {
        aether_app::preferenze::secondo_piano(&libreria.connection)
    });
    match letto {
        Ok(attivo) => ricorda(app, attivo),
        // Senza libreria non c'è preferenza da leggere, e spento è il valore
        // giusto: la X chiude, come ha sempre fatto.
        Err(err) => nota!("[vassoio] la preferenza non si legge: {err}"),
    }
}

/// Se il tasto di chiusura manda in secondo piano.
#[tauri::command]
pub fn secondo_piano(stato: State<'_, Stato>) -> Esito<bool> {
    con_libreria(&stato, |libreria| {
        aether_app::preferenze::secondo_piano(&libreria.connection)
    })
    .map_err(errore)
}

/// Accende o spegne il secondo piano. Riporta com'è rimasto.
///
/// Il valore torna **riletto** dal database e non è quello arrivato: è la stessa
/// disciplina di `accento_dinamico_attiva`, e serve perché la finestra disegni
/// l'interruttore da dove lo disegnerà al prossimo avvio. Un interruttore
/// ottimistico che resta acceso su una scrittura fallita è un'impostazione che
/// mente fino al riavvio.
#[tauri::command(async)]
pub fn secondo_piano_attiva(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    attivo: bool,
) -> Esito<bool> {
    let rimasto = con_libreria(&stato, |libreria| {
        aether_app::preferenze::imposta_secondo_piano(&libreria.connection, attivo)?;
        aether_app::preferenze::secondo_piano(&libreria.connection)
    })
    .map_err(errore)?;
    // Fuori da `con_libreria`, a lucchetto già lasciato: costruire l'icona
    // parla col filo principale e aspetta la sua risposta, e farlo tenendo la
    // libreria vorrebbe dire tenerla per tutto quel tempo.
    ricorda(&app, rimasto);
    Ok(rimasto)
}

/// Le etichette del menù, nella lingua della finestra.
///
/// La chiama la finestra appena ha applicato la lingua, e di nuovo a ogni
/// cambio. È anche il momento in cui l'icona compare la prima volta.
///
/// Le quattro della musica sono facoltative: senza — una finestra che non le
/// manda — il menù resta quello di due voci, invece di mostrarne quattro
/// senza nome.
#[tauri::command(async)]
pub fn vassoio_lingua(
    app: tauri::AppHandle,
    mostra: String,
    esci: String,
    riproduci: Option<String>,
    pausa: Option<String>,
    successivo: Option<String>,
    precedente: Option<String>,
) {
    let trasporto = match (riproduci, pausa, successivo, precedente) {
        (Some(riproduci), Some(pausa), Some(successivo), Some(precedente)) => Some(Trasporto {
            riproduci,
            pausa,
            successivo,
            precedente,
        }),
        _ => None,
    };
    // Le stesse etichette fanno da suggerimento ai pulsanti nell'anteprima:
    // vedi `crate::barra`.
    if let Some(trasporto) = &trasporto {
        crate::barra::etichette(
            &app,
            aether_barra::Etichette {
                precedente: trasporto.precedente.clone(),
                riproduci: trasporto.riproduci.clone(),
                pausa: trasporto.pausa.clone(),
                successivo: trasporto.successivo.clone(),
            },
        );
    }
    if let Some(stato) = app.try_state::<StatoVassoio>() {
        let mut dentro = stato
            .etichette
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *dentro = Some(Etichette {
            mostra,
            esci,
            trasporto,
        });
    }
    riallinea(&app);
}

/// Rispecchia lo stato del lettore nell'icona: il suggerimento e la prima voce.
///
/// La chiama chi manda lo stato alla finestra, sul filo del seguito del
/// lettore — vedi `crate::riproduzione::manda_stato` — con lo stato già
/// composto. Se niente di quel che l'icona racconta è cambiato non tocca
/// niente: lo stato parte a ogni salto e a ogni ritocco della coda, e
/// riscrivere il suggerimento ogni volta vorrebbe dire un giro sul filo della
/// finestra per dire la stessa cosa.
///
/// # Perché i lucchetti si lasciano prima di parlare all'icona
///
/// Perché cambiare suggerimento o etichetta vuol dire chiedere al filo della
/// finestra di farlo e **aspettare** la risposta. Con un lucchetto di questo
/// modulo in mano, basterebbe che quel filo lo stesse aspettando per non
/// rispondere più a nessuno: si prendono le copie di quel che serve, si
/// lasciano i lucchetti, e poi si parla.
pub(crate) fn riflette(app: &tauri::AppHandle, stato: &StatoRiproduzione) {
    let Some(vassoio) = app.try_state::<StatoVassoio>() else {
        return;
    };
    let nuovo = in_corso_di(stato);
    {
        let mut in_corso = vassoio
            .in_corso
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if *in_corso == nuovo {
            return;
        }
        in_corso.clone_from(&nuovo);
    }
    let icona = vassoio
        .icona
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let alterna = vassoio
        .alterna
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let trasporto = vassoio
        .etichette
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .and_then(|etichette| etichette.trasporto.clone());

    if let Some(icona) = icona
        && let Err(err) = icona.set_tooltip(Some(nuovo.testo()))
    {
        nota!("[vassoio] il suggerimento non si aggiorna: {err}");
    }
    if let (Some(voce), Some(trasporto)) = (alterna, trasporto)
        && let Err(err) = voce.set_text(etichetta_alterna(&trasporto, nuovo.suona))
    {
        nota!("[vassoio] la voce della pausa non si aggiorna: {err}");
    }
}

/// Quel che l'icona deve raccontare di questo stato.
fn in_corso_di(stato: &StatoRiproduzione) -> InCorso {
    let Some(brano) = stato.brano.as_ref() else {
        return InCorso::a_riposo();
    };
    InCorso {
        suggerimento: suggerimento(&brano.title, &brano.artist),
        suona: !stato.in_pausa,
    }
}

/// «Titolo — Interprete», tagliato perché Windows lo mostri intero.
///
/// Senza interprete resta il titolo, senza il trattino davanti al vuoto. Il
/// taglio è sui caratteri e non sui byte, così non spezza mai una lettera, e
/// dice di esserci con i puntini: vedi [`SUGGERIMENTO_MASSIMO`].
fn suggerimento(titolo: &str, interprete: &str) -> String {
    let intero = if interprete.trim().is_empty() {
        titolo.trim().to_owned()
    } else {
        format!("{} — {}", titolo.trim(), interprete.trim())
    };
    if intero.chars().count() <= SUGGERIMENTO_MASSIMO {
        return intero;
    }
    let mut tagliato: String = intero
        .chars()
        .take(SUGGERIMENTO_MASSIMO.saturating_sub(1))
        .collect();
    tagliato.push('…');
    tagliato
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Due etichette qualunque: qui conta solo che ci siano.
    fn etichette() -> Etichette {
        Etichette {
            mostra: "Mostra Aether".to_owned(),
            esci: "Esci".to_owned(),
            trasporto: None,
        }
    }

    fn trasporto() -> Trasporto {
        Trasporto {
            riproduci: "Riproduci".to_owned(),
            pausa: "Pausa".to_owned(),
            successivo: "Successivo".to_owned(),
            precedente: "Precedente".to_owned(),
        }
    }

    #[test]
    fn la_prima_voce_dice_quel_che_succede_premendola() {
        // Mentre suona la voce mette in pausa, e si chiama così; a musica ferma
        // la fa ripartire. È la convenzione del pulsante nella finestra.
        assert_eq!(etichetta_alterna(&trasporto(), true), "Pausa");
        assert_eq!(etichetta_alterna(&trasporto(), false), "Riproduci");
    }

    #[test]
    fn il_suggerimento_dice_titolo_e_interprete() {
        assert_eq!(suggerimento("Hunter", "Björk"), "Hunter — Björk");
        // Senza interprete non resta un trattino davanti al vuoto.
        assert_eq!(suggerimento("Hunter", "  "), "Hunter");
    }

    #[test]
    fn un_suggerimento_lungo_si_taglia_senza_spezzare_una_lettera() {
        let titolo = "è".repeat(150);
        let tagliato = suggerimento(&titolo, "Interprete");
        assert_eq!(tagliato.chars().count(), SUGGERIMENTO_MASSIMO);
        assert!(tagliato.ends_with('…'), "il taglio deve dirsi: {tagliato}");
    }

    #[test]
    fn a_musica_ferma_il_suggerimento_e_il_nome_del_programma() {
        assert_eq!(InCorso::a_riposo().testo(), SUGGERIMENTO_A_RIPOSO);
    }

    #[test]
    fn con_la_preferenza_accesa_e_le_etichette_arrivate_l_icona_ci_vuole() {
        assert!(deve_esserci(true, Some(&etichette())));
    }

    #[test]
    fn la_preferenza_accesa_da_sola_non_basta() {
        // È il caso che il preambolo del modulo chiama «il guasto giusto fra i
        // due possibili»: finché le etichette non sono scese dalla finestra non
        // c'è nessun menù da costruire, e la X torna a chiudere. Un'icona senza
        // menù sarebbe una via di ritorno che non si sa nominare.
        assert!(!deve_esserci(true, None));
    }

    #[test]
    fn le_etichette_da_sole_non_accendono_niente() {
        // La finestra manda le etichette a ogni cambio di lingua, anche a
        // preferenza spenta: se bastassero loro, l'icona comparirebbe a chi non
        // l'ha mai chiesta.
        assert!(!deve_esserci(false, Some(&etichette())));
    }

    #[test]
    fn spenta_e_senza_etichette_non_c_e_niente_da_montare() {
        assert!(!deve_esserci(false, None));
    }
}
