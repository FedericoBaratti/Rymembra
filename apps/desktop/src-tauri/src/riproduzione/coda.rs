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

use std::sync::atomic::{AtomicU64, Ordering};

use aether_app::library::TrackSummary;
use aether_app::playback;
use aether_domain::errors::AppError;
use aether_domain::queue::{Queue, QueueSnapshot, RepeatMode};
use serde::Serialize;
use tauri::{Manager as _, State};

use crate::errore::{Esito, errore};
use crate::spegnimento::Emette as _;
use crate::stato::{Stato, adesso_ms, con_libreria};

use super::fili::prepara_prossimo;
use super::{
    Lettore, StatoLettore, avvia_corrente, avvio_fallito, brano_di, chiudi_ascolto, con_lettore,
    manda_stato, manda_stato_con_posizione, seme,
};

// ── la coda di prima ────────────────────────────────────────────────────────

/// Da quanti brani una coda che nessuno ha toccato merita un «Annulla».
///
/// Un gesto che sostituisce la coda è quasi sempre voluto: si è finito un
/// disco e se ne mette un altro, e un avviso a ogni album sarebbe rumore che
/// insegna a non leggere gli avvisi. Quel che si perde **senza volerlo** è una
/// coda lunga — la cartella da duecento brani sostituita da un Invio sulla
/// cartella accanto — o una coda costruita a mano, che ha un valore suo
/// qualunque sia la lunghezza (per questa vale [`Lettore::coda_curata`]).
///
/// Venticinque sta sopra quasi ogni album e sotto quasi ogni cartella, playlist
/// o «tutti i brani»: il confine fra «un disco» e «una sessione».
const SOGLIA_DI_UNA_SESSIONE: usize = 25;

/// Il numero dell'ultima sostituzione, per riconoscere un «Annulla» vecchio.
///
/// L'avviso resta a schermo qualche secondo, e in quei secondi la coda può
/// essere sostituita di nuovo — anche da un gesto che un avviso non lo merita.
/// Senza un numero, premere «Annulla» sul primo avviso rimetterebbe la coda
/// di prima del **secondo** gesto: un annulla che non annulla quel che dice.
/// Statico e non dentro [`Lettore`] perché deve crescere anche attraverso una
/// riapertura del dispositivo, che il lettore lo ricostruisce.
static SOSTITUZIONI: AtomicU64 = AtomicU64::new(0);

/// La coda com'era prima che un gesto la sostituisse.
pub(super) struct CodaDiPrima {
    /// Quale sostituzione l'ha messa da parte; vedi [`SOSTITUZIONI`].
    numero: u64,
    /// I brani, l'ordine e il punto in cui si era.
    istantanea: QueueSnapshot,
    /// Se era stata costruita a mano: torna com'era insieme alla coda.
    curata: bool,
    /// A che punto del brano si era, in millisecondi.
    ms: u64,
    /// Il motore aveva un brano in mano: se no, la coda torna e la musica no.
    avviata: bool,
    /// La musica stava andando, non era in pausa.
    suonava: bool,
}

/// Quel che la finestra riceve quando c'è una coda da poter rimettere.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CodaSostituita {
    /// Da ripassare a [`coda_ripristina`].
    numero: u64,
    /// Quanti brani aveva la coda che se n'è andata.
    brani: usize,
    /// Il gesto era «Svuota», non una coda nuova.
    svuotata: bool,
}

/// Mette da parte la coda di adesso, prima che un gesto la sostituisca.
///
/// Si chiama **dentro** il gesto, con il lucchetto del lettore in mano e prima
/// di toccare la coda. `nuovi` sono i brani che la sostituiranno; `None` vuol
/// dire «Svuota».
///
/// Una coda vuota non ha niente da rimettere, e la stessa lista riavviata da un
/// altro brano — il doppio clic su una riga dell'elenco che sta già suonando —
/// non ha perso niente: in quei due casi l'eventuale coda messa da parte prima
/// resta dov'è, perché l'avviso che la offre può essere ancora a schermo.
pub(super) fn ricorda_coda_di_prima(
    app: &tauri::AppHandle,
    lettore: &mut Lettore,
    nuovi: Option<&[i64]>,
) {
    let istantanea = lettore.coda.snapshot();
    if istantanea.tracks.is_empty() || nuovi == Some(istantanea.tracks.as_slice()) {
        return;
    }
    // La coda che il giro guidato ha messo lì e nessuno ha toccato: vedi
    // `Lettore::coda_del_giro`.
    if lettore.coda_del_giro.take().as_ref() == Some(&istantanea.tracks) {
        return;
    }
    let posizione = lettore.motore.posizione();
    let numero = SOSTITUZIONI.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    let brani = istantanea.tracks.len();
    let curata = lettore.coda_curata;
    lettore.coda_di_prima = Some(CodaDiPrima {
        numero,
        istantanea,
        curata,
        ms: posizione.ms,
        avviata: posizione.track_id.is_some(),
        suonava: posizione.track_id.is_some() && !posizione.in_pausa,
    });
    // «Svuota» l'avviso lo ha sempre: è il solo gesto il cui scopo è perdere
    // la coda, ed è anche quello che si preme per sbaglio accanto a «Mescola».
    if nuovi.is_none() || curata || brani >= SOGLIA_DI_UNA_SESSIONE {
        app.emetti(
            "coda:sostituita",
            CodaSostituita {
                numero,
                brani,
                svuotata: nuovi.is_none(),
            },
        );
    }
}

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
        avvia_corrente(app, stato, false, 0)?;
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
        lettore.coda_curata = true;
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
        lettore.coda_curata = true;
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
///
/// # Togliere quel che è in pausa
///
/// Il successivo arriva **fermo**, al suo inizio: chi ha messo in pausa non ha
/// chiesto musica, e togliere una riga dalla coda non è premere play. Prima
/// partiva. E se il motore non aveva ancora niente in mano — la coda di ieri,
/// appena riaperta — non si apre niente: si sposta la coda, come per ogni brano
/// che non sta suonando.
#[tauri::command(async)]
pub fn coda_togli(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    indice: usize,
) -> Esito<()> {
    let partenza = con_lettore(&stato, |lettore| {
        // Stesso spazio di coordinate di `coda_vai`: l'ordine di riproduzione.
        let suonava = lettore.coda.position() == Some(indice);
        let era_ultimo = indice.saturating_add(1) >= lettore.coda.len();
        let motore = lettore.motore.posizione();
        lettore.coda.remove_at(indice);
        lettore.coda_curata = true;
        if !suonava || motore.track_id.is_none() {
            prepara_prossimo(&app, lettore);
            salva_coda(&app, lettore);
            manda_stato(&app, lettore);
            return Ok(None);
        }
        chiudi_ascolto(&app, lettore);
        if !era_ultimo {
            return Ok(Some(motore.in_pausa));
        }
        if lettore.coda.repeat() == RepeatMode::All && lettore.coda.play_at(0).is_some() {
            return Ok(Some(motore.in_pausa));
        }
        lettore.ferma();
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(None)
    })
    .map_err(errore)?;
    if let Some(in_pausa) = partenza {
        avvia_corrente(&app, &stato, in_pausa, 0).map_err(errore)?;
    }
    Ok(())
}

/// Toglie dalla coda dei brani che la libreria sta per non avere più.
///
/// # Perché la coda va sgomberata a mano
///
/// Perché non è una tabella: è un JSON di identificativi dentro `settings`, e
/// nessuna chiave esterna la ripulisce quando la riga del brano se ne va. Un
/// identificativo rimasto lì è un numero che non apre più niente, e chi lo
/// incontra è il motore alla fine del brano in corso — con un
/// `playback.sourceUnavailable` al posto della canzone dopo, che è il modo
/// peggiore di scoprire di aver cancellato qualcosa venti minuti fa.
///
/// # I due modi, e cosa li distingue
///
/// `ferma` è «il file sta per sparire dal disco». Senza, vale il comportamento
/// di [`coda_togli`]: il brano seguente scivola sotto il cursore e parte, perché
/// togliere dalla coda vuol dire «non risuonarlo», non «zitto adesso». Con,
/// invece, il motore si ferma e basta — non perché sia più prudente, ma perché
/// su Windows un file aperto non si può mandare nel Cestino, e finché il motore
/// ha in mano quei campioni l'eliminazione fallirebbe con `fs.inUse`.
///
/// Anche il brano **in canna** tiene aperto il suo file: `prepara_prossimo` lo
/// lascia andare da sé appena non è più il brano dopo, ed è per questo che si
/// chiama anche quando il corrente non è fra i condannati.
///
/// # Perché si guarda se c'è un «dopo» **prima** di togliere
///
/// Perché dopo non si può più: [`Queue::remove_tracks`] lascia il cursore dove
/// è scivolato il seguente, e da lì «c'era ancora qualcosa da sentire?» e «il
/// cursore si è aggrappato al brano di prima, che è già stato ascoltato?» sono
/// indistinguibili. È la stessa distinzione che in [`coda_togli`] si legge
/// dall'indice, detta per un insieme di brani invece che per uno solo.
pub(crate) fn sgombra_dalla_coda(
    app: &tauri::AppHandle,
    stato: &StatoLettore,
    brani: &[i64],
    ferma: bool,
) -> Result<(), AppError> {
    if brani.is_empty() {
        return Ok(());
    }
    let partenza = con_lettore(stato, |lettore| {
        let c_e_un_dopo = lettore.coda.position().is_some_and(|posizione| {
            lettore
                .coda
                .in_play_order()
                .into_iter()
                .skip(posizione.saturating_add(1))
                .any(|id| !brani.contains(&id))
        });
        let motore = lettore.motore.posizione();
        let tolti = lettore.coda.remove_tracks(brani);
        if tolti.quante == 0 {
            return Ok(None);
        }
        // La coda resta «curata»: toglierne un brano non è sostituirla, e chi
        // l'aveva costruita a mano ha ancora diritto all'«Annulla» se qualcuno
        // gliela sostituisce dopo.
        lettore.coda_curata = true;
        if !tolti.cera_il_corrente || motore.track_id.is_none() {
            prepara_prossimo(app, lettore);
            salva_coda(app, lettore);
            manda_stato(app, lettore);
            return Ok(None);
        }
        // L'ascolto di quel che suonava si chiude qui, prima che il motore
        // attacchi qualunque altra cosa: è finito adesso.
        chiudi_ascolto(app, lettore);
        if !ferma {
            if c_e_un_dopo {
                return Ok(Some(motore.in_pausa));
            }
            // Con la ripetizione accesa un dopo c'è comunque, ed è il primo
            // della coda. Senza, non resta che fermarsi.
            if lettore.coda.repeat() == RepeatMode::All && lettore.coda.play_at(0).is_some() {
                return Ok(Some(motore.in_pausa));
            }
        }
        lettore.ferma();
        salva_coda(app, lettore);
        manda_stato(app, lettore);
        Ok(None)
    })?;
    if let Some(in_pausa) = partenza {
        avvia_corrente(app, stato, in_pausa, 0)?;
    }
    Ok(())
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
        lettore.coda_curata = true;
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
        ricorda_coda_di_prima(&app, lettore, None);
        chiudi_ascolto(&app, lettore);
        lettore.coda.clear();
        lettore.coda_curata = false;
        lettore.ferma();
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Mette dei brani in una coda **vuota**, senza farli partire.
///
/// Serve al giro guidato: quattro dei suoi passi — il trasporto, il giudizio,
/// la colonna e lo schermo intero — illuminano quel che si vede **quando c'è un
/// brano**, e al primo avvio non ce n'è nessuno. Si saltavano tutti, cioè il giro
/// spiegava la barra laterale e le impostazioni e taceva proprio sul lettore.
///
/// Con una coda che c'è già non fa niente, e restituisce `false`: il giro non
/// deve toccare quel che qualcuno ha messo in coda. La musica non parte — un
/// giro che comincia a suonare da solo fa saltare sulla sedia chi lo sta
/// leggendo — e «riproduci» da lì funziona come per la coda ripresa all'avvio.
#[tauri::command(async)]
pub fn coda_prepara(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    brani: Vec<i64>,
) -> Esito<bool> {
    con_lettore(&stato, |lettore| {
        if !lettore.coda.is_empty() || brani.is_empty() {
            return Ok(false);
        }
        lettore.coda.play_tracks(brani, 0, seme());
        lettore.coda_curata = false;
        lettore.coda_del_giro = Some(lettore.coda.snapshot().tracks);
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(true)
    })
    .map_err(errore)
}

/// Rimette la coda che l'ultimo gesto aveva sostituito: l'«Annulla» dell'avviso.
///
/// Torna com'era in tutto quel che si può: i brani, l'ordine, il punto della
/// coda, il punto del brano, e se suonava o era in pausa. L'ascolto chiuso dal
/// gesto resta chiuso — è stato un ascolto vero, e ritrattarlo vorrebbe dire
/// togliere dalla cronologia secondi che si sono sentiti.
///
/// Restituisce `false` quando non c'è niente da rimettere: l'avviso era di una
/// sostituzione che nel frattempo ne ha avuta un'altra dopo, o l'«Annulla» è
/// già stato premuto. La finestra chiude l'avviso in tutti e due i casi.
///
/// # I tre tempi
///
/// Gli stessi di [`avvia_corrente`], per la stessa ragione: il brano si apre
/// **fuori** dal lucchetto del lettore. In più, nel terzo tempo, la puntina va
/// dov'era e la pausa si rialza se c'era — la stessa sequenza con cui
/// `riapertura` rimette il brano dopo un dispositivo sparito.
#[tauri::command(async)]
pub fn coda_ripristina(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    numero: u64,
) -> Esito<bool> {
    // ── 1. la coda torna, e si decide se torna anche la musica ──
    // Due `Option` una dentro l'altra: fuori «c'era qualcosa da rimettere»,
    // dentro «e c'è un brano da riaprire».
    let scelta = con_lettore(&stato, |lettore| {
        let Some(prima) = lettore.coda_di_prima.take_if(|p| p.numero == numero) else {
            return Ok(None);
        };
        chiudi_ascolto(&app, lettore);
        lettore.coda = Queue::restore(prima.istantanea);
        lettore.coda_curata = prima.curata;
        let corrente = lettore.coda.current().filter(|_| prima.avviata);
        let Some(id) = corrente else {
            lettore.ferma();
            salva_coda(&app, lettore);
            manda_stato(&app, lettore);
            return Ok(Some(None));
        };
        Ok(Some(Some((
            id,
            lettore.motore.formato(),
            prima.ms,
            prima.suonava,
        ))))
    })
    .map_err(errore)?;
    let Some(partenza) = scelta else {
        return Ok(false);
    };
    let Some((id, formato, ms, suonava)) = partenza else {
        return Ok(true);
    };

    // ── 2. l'apertura, senza niente in mano ──
    // Fallita, la coda è già tornata: il motore si ferma su di lei invece di
    // continuare il brano che la sostituzione aveva avviato. Vedi
    // [`avvio_fallito`].
    let brano = match brano_di(&app, id, formato) {
        Ok(brano) => brano,
        Err(err) => {
            avvio_fallito(&app, &stato, id);
            return Err(errore(err));
        }
    };

    // ── 3. la puntina dov'era ──
    con_lettore(&stato, |lettore| {
        if lettore.coda.current() != Some(id) {
            return Ok(());
        }
        lettore.suona(brano);
        lettore.motore.vai_a(ms);
        if !suonava {
            lettore.motore.pausa();
        }
        prepara_prossimo(&app, lettore);
        salva_coda(&app, lettore);
        manda_stato_con_posizione(&app, lettore, ms);
        Ok(())
    })
    .map_err(errore)?;
    Ok(true)
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
