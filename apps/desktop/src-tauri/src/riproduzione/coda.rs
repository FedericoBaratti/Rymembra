//! La coda, e i gesti che la cambiano.
//!
//! Accodare, mettere subito dopo, saltare, togliere, riordinare, svuotare,
//! ripetere, mescolare — più la coda di ieri che torna all'avvio e la scelta
//! che l'autoplay fa da solo quando dopo non c'è più niente.
//!
//! Fa parte di [`crate::riproduzione`]: la regola dei due lucchetti — prima il
//! lettore, poi la libreria — è scritta là e vale anche qui. Qui si vede in
//! [`gesto_di_coda`], che tiene il gesto e la partenza una fuori dall'altra
//! proprio per non aprire un file con il lucchetto del lettore in mano.

use aether_app::library::TrackSummary;
use aether_app::playback;
use aether_domain::errors::AppError;
use aether_domain::queue::{Queue, RepeatMode};
use tauri::{Manager as _, State};

use crate::errore::{Esito, errore};
use crate::stato::{Stato, adesso_ms, con_libreria};

use super::fili::prepara_prossimo;
use super::{
    Lettore, StatoLettore, avvia_corrente, chiudi_ascolto, con_lettore, manda_stato, seme,
};

/// Il brano che continua la sessione, secondo la libreria.
///
/// Fuori da [`prepara_prossimo`] perché prende il lucchetto della libreria, e
/// la regola di [`crate::riproduzione`] è che lo si prenda **dopo** quello del
/// lettore — che a questo punto è già in mano a chi ci ha chiamati. Tenerla
/// separata rende la sequenza leggibile invece che implicita.
pub(super) fn scegli_da_solo(
    app: &tauri::AppHandle,
    lettore: &Lettore,
) -> Option<aether_app::autoplay::Scelta> {
    let corrente = lettore.coda.current()?;
    // Tutto quel che è già in coda, così l'autoplay non ripropone quel che si
    // è appena sentito.
    let esclusi = lettore.coda.in_play_order();
    let stato = app.state::<Stato>();
    con_libreria(&stato, |libreria| {
        aether_app::autoplay::prossimo(&libreria.connection, corrente, &esclusi, adesso_ms())
    })
    .ok()
    .flatten()
}

/// Conserva la coda.
pub(super) fn salva_coda(app: &tauri::AppHandle, lettore: &Lettore) {
    let istantanea = lettore.coda.snapshot();
    let stato = app.state::<Stato>();
    let _ = con_libreria(&stato, |libreria| {
        playback::save_queue(&libreria.connection, &istantanea)
    });
}

/// Riprende la coda conservata, senza far partire niente.
///
/// La coda torna, la musica no: un'applicazione che comincia a suonare da sola
/// all'avvio è un'applicazione che fa saltare sulla sedia chi l'ha aperta per
/// cercare una canzone.
pub fn riprendi_coda(app: &tauri::AppHandle) {
    let stato_lettore = app.state::<StatoLettore>();
    let _ = con_lettore(&stato_lettore, |lettore| {
        let stato = app.state::<Stato>();
        let istantanea = con_libreria(&stato, |libreria| {
            playback::load_queue(&libreria.connection)
        })?;
        lettore.coda = Queue::restore(istantanea);
        let volume = con_libreria(&stato, |libreria| {
            playback::load_volume(&libreria.connection)
        })
        .unwrap_or_default();
        lettore.volume = volume;
        lettore.motore.volume(volume.volume, volume.muto);
        let eq = con_libreria(&stato, |libreria| playback::load_eq(&libreria.connection))
            .unwrap_or_default();
        lettore.motore.equalizzatore(&eq.guadagni, eq.attivo);
        lettore.eq = eq;
        // La normalizzazione si rimanda al motore anche quando coincide con il
        // suo valore di serie: costa un comando su una coda che è già lì, e
        // toglie di mezzo la domanda «chi dei due ha ragione» il giorno in cui
        // uno dei due valori cambiasse.
        let normalizzazione = con_libreria(&stato, |libreria| {
            playback::load_replaygain(&libreria.connection)
        })
        .unwrap_or_default();
        lettore
            .motore
            .replaygain(normalizzazione.attivo, normalizzazione.bersaglio_db);
        lettore.normalizzazione = normalizzazione;

        // L'autoplay non si manda al motore — il motore non sa cosa sia una
        // libreria, ed è giusto così. Serve solo a `prepara_prossimo`, che è
        // di qui.
        lettore.autoplay = con_libreria(&stato, |libreria| {
            playback::load_autoplay(&libreria.connection)
        })
        .unwrap_or(false);

        // La dissolvenza invece sì: è il motore a doverla fare, ed è l'unico
        // che sa quando un brano sta per finire. Si manda anche quando vale
        // zero, per la stessa ragione della normalizzazione qui sopra.
        let dissolvenza_s = con_libreria(&stato, |libreria| {
            playback::load_crossfade(&libreria.connection)
        })
        .unwrap_or(0);
        lettore
            .motore
            .dissolvenza(dissolvenza_s.saturating_mul(1000));
        lettore.dissolvenza_s = dissolvenza_s;

        // E la latenza d'uscita dichiarata a mano, per la stessa ragione: la
        // compensazione la fa il motore, e un motore appena aperto non sa niente
        // di quel che l'utente aveva dichiarato. Senza questa riga il cursore
        // delle impostazioni mostrerebbe il numero scritto e la posizione non ne
        // terrebbe conto, che è la peggiore delle due bugie possibili.
        let latenza_ms = con_libreria(&stato, |libreria| {
            playback::load_latenza(&libreria.connection)
        })
        .unwrap_or(0);
        lettore.motore.latenza(latenza_ms);
        lettore.latenza_ms = latenza_ms;

        // Quante barre vuole vedere chi guarda. Si applica all'avvio anche se
        // la schermata è chiusa: costa un messaggio e vuol dire che la prima
        // apertura mostra la scena giusta invece di quella di serie per un
        // fotogramma.
        let bande = con_libreria(&stato, |libreria| {
            playback::load_spettro_bande(&libreria.connection)
        })
        .unwrap_or(aether_play::RISOLUZIONE_DI_SERIE);
        app.state::<StatoLettore>()
            .spettro_bande
            .store(bande, std::sync::atomic::Ordering::Relaxed);
        lettore.motore.spettro_dettaglio(bande);
        Ok(())
    });
}

/// Un gesto sulla coda, e poi — se il gesto lo chiede — la partenza.
///
/// Esiste per tenere le due cose **una fuori dall'altra**: il gesto vuole il
/// lucchetto del lettore e dura microsecondi, la partenza apre un file e non
/// deve averlo in mano. Il `bool` che il gesto restituisce è «adesso fai
/// partire il corrente», e girare quella decisione qui invece di chiamare
/// [`avvia_corrente`] da dentro la chiusura è ciò che impedisce di riprendere un
/// `Mutex` che è già preso — cioè un blocco irreversibile della finestra.
pub(super) fn gesto_di_coda(
    app: &tauri::AppHandle,
    stato: &StatoLettore,
    gesto: impl FnOnce(&mut Lettore) -> Result<bool, AppError>,
) -> Result<(), AppError> {
    if con_lettore(stato, gesto)? {
        avvia_corrente(app, stato)?;
    }
    Ok(())
}

// ── i comandi della coda ────────────────────────────────────────────────────

/// Accoda in fondo.
#[tauri::command(async)]
pub fn coda_accoda(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    brani: Vec<i64>,
) -> Esito<()> {
    gesto_di_coda(&app, &stato, |lettore| {
        let era_vuota = lettore.coda.is_empty();
        lettore.coda.enqueue(&brani);
        if era_vuota {
            return Ok(true);
        }
        prepara_prossimo(&app, lettore);
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(false)
    })
    .map_err(errore)
}

/// Mette subito dopo il brano corrente.
#[tauri::command(async)]
pub fn coda_dopo(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    brani: Vec<i64>,
) -> Esito<()> {
    gesto_di_coda(&app, &stato, |lettore| {
        let era_vuota = lettore.coda.is_empty();
        lettore.coda.play_next(&brani);
        if era_vuota {
            return Ok(true);
        }
        prepara_prossimo(&app, lettore);
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(false)
    })
    .map_err(errore)
}

/// Salta a una posizione della coda.
#[tauri::command(async)]
pub fn coda_vai(app: tauri::AppHandle, stato: State<'_, StatoLettore>, indice: usize) -> Esito<()> {
    gesto_di_coda(&app, &stato, |lettore| {
        // L'indice arriva dalla coda **come si vede**, cioè nell'ordine di
        // riproduzione: con lo shuffle acceso non è quello dell'elenco interno.
        if lettore.coda.play_at(indice).is_none() {
            return Ok(false);
        }
        chiudi_ascolto(&app, lettore);
        Ok(true)
    })
    .map_err(errore)
}

/// Toglie dalla coda.
///
/// # Togliere quel che sta suonando
///
/// `Queue::remove` lascia il cursore dov'era — «non risuonarlo, non zitto
/// adesso» — e lì è scivolato il brano seguente: la finestra mostra quindi
/// subito il titolo del successivo. Senza le righe qui sotto, però, il motore
/// continuerebbe a far uscire i campioni del brano appena tolto: la barra
/// direbbe una cosa e le casse un'altra, e alla fine di quel brano la coda
/// avanzerebbe ancora, saltando il titolo che era stato annunciato. Si fa
/// quindi partire il nuovo corrente, come in [`coda_vai`], e l'ascolto del
/// brano tolto si chiude qui — prima che il motore attacchi il successivo —
/// perché è finito adesso.
///
/// Se però era l'**ultimo**, dietro di lui non è scivolato nessuno: il cursore
/// si aggrappa al brano di prima, che è già stato ascoltato, e farlo ripartire
/// sarebbe la sorpresa peggiore delle due. Con la ripetizione accesa un dopo
/// c'è comunque, ed è il primo della coda; senza, non resta che fermarsi.
#[tauri::command(async)]
pub fn coda_togli(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    indice: usize,
) -> Esito<()> {
    gesto_di_coda(&app, &stato, |lettore| {
        // Stesso spazio di coordinate di `coda_vai`: l'ordine di riproduzione.
        let suonava = lettore.coda.position() == Some(indice);
        let era_ultimo = indice.saturating_add(1) >= lettore.coda.len();
        lettore.coda.remove_at(indice);
        if !suonava {
            prepara_prossimo(&app, lettore);
            salva_coda(&app, lettore);
            manda_stato(&app, lettore);
            return Ok(false);
        }
        chiudi_ascolto(&app, lettore);
        if !era_ultimo {
            return Ok(true);
        }
        if lettore.coda.repeat() == RepeatMode::All && lettore.coda.play_at(0).is_some() {
            return Ok(true);
        }
        lettore.motore.ferma();
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(false)
    })
    .map_err(errore)
}

/// Sposta un brano dentro la coda.
#[tauri::command(async)]
pub fn coda_riordina(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    da: usize,
    a: usize,
) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        lettore.coda.reorder(da, a);
        prepara_prossimo(&app, lettore);
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Svuota la coda e ferma tutto.
#[tauri::command]
pub fn coda_svuota(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        chiudi_ascolto(&app, lettore);
        lettore.coda.clear();
        lettore.motore.ferma();
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Passa al modo di ripetizione successivo.
#[tauri::command(async)]
pub fn ripeti(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        lettore.coda.cycle_repeat();
        // Il prossimo cambia: con «ripeti uno» è di nuovo questo.
        prepara_prossimo(&app, lettore);
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Accende o spegne lo shuffle.
#[tauri::command(async)]
pub fn mescola(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        lettore.coda.toggle_shuffle(seme());
        prepara_prossimo(&app, lettore);
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Le righe di una lista di identificativi, per il pannello della coda.
///
/// L'ordine chiesto si conserva: la coda non è ordinata come il database, e
/// restituirla ordinata per `id` vorrebbe dire mostrare all'utente un elenco che
/// non è quello che sentirà.
#[tauri::command]
pub fn brani_per_id(stato: State<'_, Stato>, brani: Vec<i64>) -> Esito<Vec<TrackSummary>> {
    con_libreria(&stato, |libreria| {
        aether_app::library::summaries_by_id(&libreria.connection, &brani)
    })
    .map_err(errore)
}
