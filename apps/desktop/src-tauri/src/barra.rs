//! I tre pulsanti sotto l'anteprima della finestra, nella barra delle
//! applicazioni.
//!
//! Passando il puntatore sull'icona di Rymembra, sotto l'anteprima compaiono
//! brano precedente, riproduci o pausa e brano successivo — come in foobar2000.
//! Il lavoro con Windows lo fa `rymembra-barra`; qui c'è soltanto quel che
//! collega i pulsanti al lettore.
//!
//! # Le stesse strade del vassoio
//!
//! Un clic diventa un [`Gesto`] consegnato a [`crate::media::premi`], come le
//! voci del vassoio e i tasti della scheda di Windows: tre posti che fanno
//! **letteralmente** la stessa cosa, sullo stesso filo, nell'ordine in cui
//! sono stati premuti. E il clic arriva sul filo della finestra, che non deve
//! aspettare un «successivo» che apre un file dalla rete.
//!
//! I suggerimenti sono le stesse quattro etichette delle voci della musica nel
//! vassoio, che la finestra manda con `vassoio_lingua`: nessun testo nuovo da
//! tradurre, e un cambio di lingua aggiorna tutti e due.
//!
//! L'aspetto si rinfresca da [`riflette`], chiamata da chi manda lo stato alla
//! finestra: una sorgente sola, come per la scheda di Windows e il vassoio.
//!
//! # Lo stato nasce prima dei pulsanti
//!
//! [`StatoBarra`] si registra prima del lettore e i pulsanti si agganciano
//! dopo: il primo stato — la coda di ieri, rimessa all'avvio — arriva presto, e
//! se lo si buttasse perché i pulsanti non ci sono ancora resterebbero spenti
//! fino al primo cambio. Così [`riflette`] lo ricorda comunque, e [`avvia`] lo
//! consegna appena può.

use std::sync::{Mutex, PoisonError};

use rymembra_barra::{Aspetto, Barra, Etichette, Tasto};
use tauri::Manager as _;

use crate::media::Gesto;
use crate::nota;
use crate::riproduzione::StatoRiproduzione;

/// I pulsanti, e quel che devono mostrare.
pub struct StatoBarra {
    /// I pulsanti, se si sono agganciati.
    ///
    /// `None` finché [`avvia`] non ha finito, e per sempre se Windows li ha
    /// rifiutati: un lettore senza pulsanti nell'anteprima è il lettore di
    /// prima, e non c'è niente da mostrare all'utente.
    barra: Mutex<Option<Barra>>,
    /// L'aspetto voluto, anche quando i pulsanti non ci sono ancora.
    aspetto: Mutex<Aspetto>,
}

impl StatoBarra {
    /// Niente pulsanti, l'aspetto di partenza.
    #[must_use]
    pub fn nuovo() -> Self {
        Self {
            barra: Mutex::new(None),
            aspetto: Mutex::new(Aspetto::default()),
        }
    }
}

/// Aggancia i pulsanti alla finestra principale.
///
/// Sul filo della finestra, per forza: è il filo a cui Windows recapita i clic,
/// e l'unico da cui la finestra si lascia agganciare. Da `setup` ci si è già,
/// ma passare da `run_on_main_thread` non dipende da chi chiama.
pub fn avvia(app: &tauri::AppHandle) {
    let mano = app.clone();
    if let Err(err) = app.run_on_main_thread(move || aggancia(&mano)) {
        nota!("[barra] il filo della finestra non risponde: {err}");
    }
}

/// Il lavoro di [`avvia`], sul filo della finestra.
fn aggancia(app: &tauri::AppHandle) {
    let Some(finestra) = app.get_webview_window("main") else {
        nota!("[barra] nessuna finestra: niente pulsanti nell'anteprima");
        return;
    };
    let hwnd = match finestra.hwnd() {
        Ok(hwnd) => hwnd.0.cast(),
        Err(err) => {
            nota!("[barra] la finestra non ha un puntatore: {err}");
            return;
        }
    };
    let mano = app.clone();
    let su_tasto = move |tasto: Tasto| {
        let gesto = match tasto {
            Tasto::Precedente => Gesto::Precedente,
            Tasto::Alterna => Gesto::Alterna,
            Tasto::Successivo => Gesto::Prossimo,
        };
        crate::media::premi(&mano, gesto);
    };
    let barra = match rymembra_barra::aggancia(hwnd, su_tasto, |riga| nota!("{riga}")) {
        Ok(barra) => barra,
        Err(err) => {
            nota!("[barra] niente pulsanti nell'anteprima: {err}");
            return;
        }
    };
    let Some(stato) = app.try_state::<StatoBarra>() else {
        return;
    };
    // Il posto dei pulsanti si tiene **mentre** si legge l'aspetto ricordato
    // fin qui — il primo stato del lettore può essere già passato. Lasciandolo
    // fra le due cose, uno stato arrivato in mezzo troverebbe il posto ancora
    // vuoto e andrebbe perso: così invece aspetta, e arriva dopo questo.
    let mut posto = stato.barra.lock().unwrap_or_else(PoisonError::into_inner);
    let aspetto = stato
        .aspetto
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    barra.mostra(aspetto);
    *posto = Some(barra);
}

/// Rispecchia lo stato del lettore nei pulsanti.
///
/// La chiama chi manda lo stato alla finestra, accanto al vassoio — vedi
/// `crate::riproduzione::manda_stato`. Se niente di quel che i pulsanti
/// mostrano è cambiato, `rymembra-barra` non tocca niente.
pub(crate) fn riflette(app: &tauri::AppHandle, stato: &StatoRiproduzione) {
    cambia(app, |aspetto| {
        aspetto.suona = stato.brano.is_some() && !stato.in_pausa;
        // Senza brano e con la coda vuota non c'è niente da comandare. Con la
        // coda piena e il brano non ancora aperto «riproduci» ha invece un
        // senso: parte la coda.
        aspetto.attivi = stato.brano.is_some() || !stato.coda.is_empty();
    });
}

/// I suggerimenti, nella lingua della finestra.
///
/// La chiama `vassoio_lingua`, con le stesse etichette delle voci della musica
/// nel menù del vassoio.
pub(crate) fn etichette(app: &tauri::AppHandle, etichette: Etichette) {
    cambia(app, |aspetto| aspetto.etichette = etichette);
}

/// Cambia l'aspetto ricordato e lo passa ai pulsanti, se ci sono.
///
/// I due lucchetti si prendono uno dopo l'altro e mai insieme. Li tiene insieme
/// soltanto [`aggancia`], una volta, e dato che qui non se ne tengono mai due
/// non c'è un ordine contrario con cui scontrarsi. `mostra` non aspetta
/// nessuno: mette un messaggio in coda al filo della finestra e torna.
fn cambia(app: &tauri::AppHandle, ritocca: impl FnOnce(&mut Aspetto)) {
    let Some(stato) = app.try_state::<StatoBarra>() else {
        return;
    };
    let aspetto = {
        let mut aspetto = stato.aspetto.lock().unwrap_or_else(PoisonError::into_inner);
        ritocca(&mut aspetto);
        aspetto.clone()
    };
    if let Some(barra) = stato
        .barra
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        barra.mostra(aspetto);
    }
}
