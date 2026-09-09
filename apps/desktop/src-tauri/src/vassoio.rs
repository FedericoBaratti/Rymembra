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
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::errore::{Esito, errore};
use crate::nota;
use crate::stato::{Stato, con_libreria};

/// L'identificativo della voce che riapre la finestra.
const VOCE_MOSTRA: &str = "vassoio.mostra";

/// L'identificativo della voce che esce davvero.
const VOCE_ESCI: &str = "vassoio.esci";

/// Le due etichette del menù, come le manda la finestra.
#[derive(Clone)]
struct Etichette {
    /// «Mostra Aether».
    mostra: String,
    /// «Esci».
    esci: String,
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
fn mostra(app: &tauri::AppHandle) {
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
fn costruisci_menu(
    app: &tauri::AppHandle,
    etichette: &Etichette,
) -> tauri::Result<Menu<tauri::Wry>> {
    let mostra = MenuItem::with_id(app, VOCE_MOSTRA, &etichette.mostra, true, None::<&str>)?;
    let esci = MenuItem::with_id(app, VOCE_ESCI, &etichette.esci, true, None::<&str>)?;
    Menu::with_items(app, &[&mostra, &esci])
}

/// Mette l'icona nell'area di notifica.
fn costruisci(app: &tauri::AppHandle, menu: &Menu<tauri::Wry>) -> tauri::Result<TrayIcon> {
    let mut costruttore = TrayIconBuilder::new()
        // Il nome del programma, non tradotto: i nomi dei programmi non si
        // traducono. La stessa scelta di `display_name` in `media.rs`.
        .tooltip("Aether")
        .menu(menu)
        // Il menù sul tasto destro soltanto. Il sinistro riapre la finestra,
        // che è quel che si vuole quasi sempre: chi cerca l'icona la cerca per
        // tornare, non per leggere due voci.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, evento| match evento.id.as_ref() {
            VOCE_MOSTRA => mostra(app),
            VOCE_ESCI => esci(app),
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
    let mut posto = stato.icona.lock().unwrap_or_else(PoisonError::into_inner);

    let ci_vuole = deve_esserci(acceso, etichette.as_ref());
    let Some(etichette) = etichette.filter(|_| ci_vuole) else {
        // Spenta, o etichette non ancora arrivate: via l'icona. Cade qui
        // l'ultima copia, e l'area di notifica torna com'era.
        stato.presente.store(false, Ordering::Relaxed);
        drop(posto.take());
        return;
    };

    let menu = match costruisci_menu(app, &etichette) {
        Ok(menu) => menu,
        Err(err) => {
            nota!("[vassoio] il menù non si costruisce: {err}");
            return;
        }
    };

    if let Some(icona) = posto.as_ref() {
        // C'è già: cambiano solo le etichette, e rifarla farebbe lampeggiare
        // l'area di notifica a ogni cambio di lingua.
        if let Err(err) = icona.set_menu(Some(menu)) {
            nota!("[vassoio] il menù non si sostituisce: {err}");
        }
        return;
    }

    match costruisci(app, &menu) {
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

/// Le due etichette del menù, nella lingua della finestra.
///
/// La chiama la finestra appena ha applicato la lingua, e di nuovo a ogni
/// cambio. È anche il momento in cui l'icona compare la prima volta.
#[tauri::command(async)]
pub fn vassoio_lingua(app: tauri::AppHandle, mostra: String, esci: String) {
    if let Some(stato) = app.try_state::<StatoVassoio>() {
        let mut dentro = stato
            .etichette
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *dentro = Some(Etichette { mostra, esci });
    }
    riallinea(&app);
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Due etichette qualunque: qui conta solo che ci siano.
    fn etichette() -> Etichette {
        Etichette {
            mostra: "Mostra Aether".to_owned(),
            esci: "Esci".to_owned(),
        }
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
