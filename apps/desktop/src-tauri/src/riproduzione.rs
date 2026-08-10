//! La riproduzione, dal lato della finestra.
//!
//! Tiene insieme tre cose che vivono in tre posti diversi: la **coda**
//! (`aether_domain::queue`, pura), il **motore** (`aether-play`, che non sa cosa
//! sia una libreria) e il **database** (`aether_app::playback`). Nessuno dei tre
//! conosce gli altri due, ed è voluto — ma qualcuno deve pur presentarli, e quel
//! qualcuno è questo file.
//!
//! # I due lucchetti, e l'ordine in cui si prendono
//!
//! Il lettore sta dietro un mutex **diverso** da quello della libreria. Deve:
//! una scansione tiene il lucchetto della libreria per venti secondi, e se il
//! tasto pausa passasse di lì l'utente premerebbe pausa e non succederebbe
//! niente per venti secondi.
//!
//! Due lucchetti però sono un abbraccio mortale in attesa di succedere, e
//! l'unica difesa che regge è una regola sola: **prima il lettore, poi la
//! libreria, mai il contrario**. Ogni funzione di questo file la rispetta.
//!
//! # Perché il conteggio d'ascolto si scrive qui e non nel motore
//!
//! Perché il motore non sa se quel che ha suonato conta. La regola —
//! `aether_domain::listen::counts_as_play` — è del dominio, e la scrittura è di
//! `aether-app`. Qui c'è solo il filo che le mette in fila.

use std::sync::Mutex;
use std::time::Duration;

use aether_app::library::TrackSummary;
use aether_app::playback::{self, Equalizzazione, Volume};
use aether_domain::errors::AppError;
use aether_domain::listen::ListenTracker;
use aether_domain::queue::{Queue, RepeatMode, Step};
use aether_play::{Evento, Motore, PRESET_DI_SERIE};
use serde::Serialize;
use tauri::{Emitter as _, Manager as _, State};

use crate::errore::{Esito, errore};
use crate::stato::{Stato, adesso_ms, con_libreria};

/// Ogni quanto la finestra riceve la posizione.
///
/// Quattro volte al secondo, non sessanta. Il cursore si muove liscio lo stesso
/// perché la finestra interpola fra un colpo e l'altro; sessanta eventi al
/// secondo attraverso l'IPC sarebbero sessanta serializzazioni JSON al secondo
/// per spostare un pixel, e si vedrebbero nel consumo della batteria molto prima
/// che nella fluidità.
const PASSO_TEMPO: Duration = Duration::from_millis(250);

/// Ogni quanto la finestra riceve le bande dello spettro.
///
/// Trenta volte al secondo, che è sette volte la posizione — e non è una
/// contraddizione con la disciplina qui sopra. La posizione va a quattro perché
/// **la finestra la sa interpolare**: fra un colpo e l'altro il tempo passa da
/// solo, e il cursore avanza senza chiedere niente. Le bande no: fra un colpo e
/// l'altro non c'è niente da indovinare, e a quattro colpi al secondo le barre
/// saltano invece di muoversi.
///
/// Il costo è dieci numeri e nessuna interrogazione al database, e si paga solo
/// mentre la schermata è aperta.
const PASSO_SPETTRO: Duration = Duration::from_millis(33);

/// Ogni quanto il filo dello spettro si sveglia quando nessuno guarda.
///
/// Dorme a lungo invece di uscire: farlo nascere e morire vorrebbe dire
/// coordinare la sua fine con l'apertura successiva, e un quarto di secondo di
/// ritardo all'apertura della schermata non lo vede nessuno.
const PASSO_SPETTRO_FERMO: Duration = Duration::from_millis(250);

/// Lo stato della riproduzione, come lo vede la finestra.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoRiproduzione {
    /// Il brano corrente, per intero: la barra deve disegnarne titolo e copertina.
    pub brano: Option<TrackSummary>,
    /// È fermo.
    pub in_pausa: bool,
    /// A che punto è.
    pub posizione_ms: u64,
    /// Quanto dura.
    pub durata_ms: u64,
    /// Lo shuffle è acceso.
    pub shuffle: bool,
    /// Come si ripete: `off`, `one`, `all`.
    pub ripeti: String,
    /// Il volume, da 0 a 1.
    pub volume: f32,
    /// È silenziato.
    pub muto: bool,
    /// La coda, nell'ordine in cui suonerà.
    ///
    /// Solo gli identificativi: mandare millequattrocento righe intere a ogni
    /// cambio di brano vorrebbe dire spedire qualche megabyte per aggiornare un
    /// titolo. Chi apre il pannello della coda chiede le righe con
    /// [`brani_per_id`].
    pub coda: Vec<i64>,
    /// Dove siamo dentro la coda.
    pub posizione_coda: Option<usize>,
    /// L'equalizzatore è acceso.
    pub eq_attivo: bool,
    /// La curva dell'equalizzatore, in decibel per banda.
    pub eq_guadagni: Vec<f32>,
}

/// La curva dell'equalizzatore da sola.
///
/// Un evento suo invece di [`StatoRiproduzione`]: comporre lo stato intero
/// richiede una lettura del brano corrente dal database, e durante un
/// trascinamento questa roba parte una dozzina di volte al secondo.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoEq {
    /// I filtri sono accesi.
    pub attivo: bool,
    /// Quanti decibel per banda.
    pub guadagni: Vec<f32>,
}

/// Una curva che si può scegliere dall'elenco.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VocePreset {
    /// Come si chiama.
    pub nome: String,
    /// Quanti decibel per banda.
    pub guadagni: Vec<f32>,
    /// Viene con l'applicazione, quindi non si può cancellare.
    pub di_serie: bool,
}

/// Solo il tempo che passa.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tempo {
    /// A che punto è.
    pub posizione_ms: u64,
    /// Quanto dura.
    pub durata_ms: u64,
    /// È fermo.
    pub in_pausa: bool,
}

/// Quel che l'applicazione tiene aperto per suonare.
pub struct Lettore {
    motore: Motore,
    coda: Queue,
    /// L'ascolto in corso, se ce n'è uno.
    ascolto: Option<ListenTracker>,
    volume: Volume,
    eq: Equalizzazione,
}

/// Il lettore, o il motivo per cui non c'è.
///
/// Come per la libreria in [`crate::stato`], il guasto si **conserva** invece di
/// far fallire l'avvio: senza scheda audio l'applicazione resta un catalogo
/// consultabile, e un catalogo consultabile è meglio di una finestra che non si
/// apre.
pub struct StatoLettore {
    pub lettore: Mutex<Result<Lettore, AppError>>,
    /// La schermata dello spettro è aperta.
    ///
    /// Qui e non solo dentro il motore perché lo legge anche il filo che manda
    /// l'evento, e leggerlo di là vorrebbe dire prendere il lucchetto del
    /// lettore trenta volte al secondo per scoprire che non c'è niente da fare.
    pub spettro: std::sync::atomic::AtomicBool,
}

impl StatoLettore {
    /// Avvia il motore e riprende la coda di ieri.
    pub fn avvia(app: &tauri::AppHandle) -> Self {
        let manico = app.clone();
        let motore = aether_play::avvia(move |evento| su_evento(&manico, evento));
        let lettore = motore.map(|motore| Lettore {
            motore,
            coda: Queue::new(),
            ascolto: None,
            volume: Volume::default(),
            eq: Equalizzazione::default(),
        });
        Self {
            lettore: Mutex::new(lettore),
            spettro: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

/// Stampa se il dispositivo audio si è aperto, e come.
///
/// Una riga sola e c'è sempre, come quella della libreria: quando qualcuno dirà
/// «non si sente niente», la frequenza e i canali con cui il dispositivo si è
/// aperto sono la prima cosa da guardare, e senza questa riga non esisterebbero
/// da nessuna parte.
pub fn riga_di_avvio_lettore(stato: &StatoLettore) {
    let guardia = stato
        .lettore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match guardia.as_ref() {
        Ok(lettore) => {
            let f = lettore.motore.formato();
            println!(
                "[avvio] audio aperto frequenza={} canali={}",
                f.frequenza, f.canali
            );
        }
        Err(err) => eprintln!(
            "[avvio] audio NON aperto codice={} causa={}",
            err.code().kind().code(),
            err.cause().unwrap_or("—")
        ),
    }
}

/// Esegue `azione` sul lettore aperto.
fn con_lettore<T>(
    stato: &StatoLettore,
    azione: impl FnOnce(&mut Lettore) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let mut guardia = stato
        .lettore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match guardia.as_mut() {
        Ok(lettore) => azione(lettore),
        Err(err) => Err(err.clone()),
    }
}

/// Un seme per il mescolamento.
///
/// L'orologio: il dominio non ne ha uno — è la sua regola — quindi glielo porta
/// chi ce l'ha. Due mescolamenti nello stesso nanosecondo darebbero lo stesso
/// ordine, il che è esattamente ciò che serve per provarli e non è un problema
/// per usarli.
fn seme() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        // I bit bassi dei nanosecondi: sono quelli che cambiano, e sono l'unica
        // parte che serve. Troncare i bit alti di un `u128` che conta i
        // nanosecondi dal 1970 toglie l'epoca e lascia il rumore.
        .map_or(0, |d| {
            u64::try_from(d.as_nanos() & u128::from(u64::MAX)).unwrap_or(0)
        })
}

/// Chiude l'ascolto in corso e lo scrive, se conta.
///
/// Prende il lucchetto della libreria: va chiamata quando quello del lettore è
/// già in mano, mai al contrario.
fn chiudi_ascolto(app: &tauri::AppHandle, lettore: &mut Lettore) {
    let Some(tracker) = lettore.ascolto.take() else {
        return;
    };
    let ascolto = tracker.finish(adesso_ms());
    if !ascolto.counts {
        return;
    }
    let stato = app.state::<Stato>();
    let scritto = con_libreria(&stato, |libreria| {
        playback::record_play(&mut libreria.connection, &ascolto)
    });
    match scritto {
        // `true` vuol dire che il conteggio è cresciuto davvero. Segnalarlo
        // anche quando l'ascolto non contava riempirebbe il canale del backup
        // di sveglie a vuoto: chi salta un brano dopo tre secondi non ha
        // cambiato niente da salvare.
        Ok(true) => crate::nuvola::sporca(app),
        Ok(false) => {}
        Err(err) => eprintln!(
            "[riproduzione] ascolto non registrato codice={} causa={}",
            err.code().kind().code(),
            err.cause().unwrap_or("—")
        ),
    }
}

/// Apre la sorgente di un brano.
fn sorgente_di(app: &tauri::AppHandle, track_id: i64) -> Result<aether_play::Sorgente, AppError> {
    let stato = app.state::<Stato>();
    con_libreria(&stato, |libreria| {
        playback::sorgente(
            &libreria.connection,
            &aether_app::files::LocalFiles,
            track_id,
        )
    })
}

/// Dice al motore quale sarà il brano dopo.
///
/// È tutto il gapless: il file successivo viene aperto **mentre** il corrente
/// suona ancora, così quando tocca a lui i suoi campioni sono già pronti. Un
/// brano che non si apre non è un guasto da mostrare adesso: lo si scoprirà
/// quando toccherà a lui, e nel frattempo quello che suona non va interrotto.
fn prepara_prossimo(app: &tauri::AppHandle, lettore: &Lettore) {
    let prossimo = lettore
        .coda
        .peek_next()
        .and_then(|id| sorgente_di(app, id).ok());
    lettore.motore.prepara(prossimo);
}

/// Conserva la coda.
fn salva_coda(app: &tauri::AppHandle, lettore: &Lettore) {
    let istantanea = lettore.coda.snapshot();
    let stato = app.state::<Stato>();
    let _ = con_libreria(&stato, |libreria| {
        playback::save_queue(&libreria.connection, &istantanea)
    });
}

/// Compone lo stato e lo manda alla finestra.
fn manda_stato(app: &tauri::AppHandle, lettore: &Lettore) {
    let stato = costruisci_stato(app, lettore);
    let _ = app.emit("riproduzione:stato", stato);
}

fn costruisci_stato(app: &tauri::AppHandle, lettore: &Lettore) -> StatoRiproduzione {
    let posizione = lettore.motore.posizione();
    let brano = lettore.coda.current().and_then(|id| {
        let stato = app.state::<Stato>();
        con_libreria(&stato, |libreria| {
            aether_app::library::read_summary(&libreria.connection, id)
        })
        .ok()
        .flatten()
    });
    StatoRiproduzione {
        durata_ms: brano.as_ref().map_or(posizione.durata_ms, |b| {
            u64::try_from(b.duration_ms).unwrap_or(0)
        }),
        brano,
        in_pausa: posizione.in_pausa,
        posizione_ms: posizione.ms,
        shuffle: lettore.coda.shuffle(),
        ripeti: nome_ripetizione(lettore.coda.repeat()).to_owned(),
        volume: lettore.volume.volume,
        muto: lettore.volume.muto,
        coda: lettore.coda.in_play_order(),
        posizione_coda: lettore.coda.position(),
        eq_attivo: lettore.eq.attivo,
        eq_guadagni: lettore.eq.guadagni.clone(),
    }
}

const fn nome_ripetizione(repeat: RepeatMode) -> &'static str {
    match repeat {
        RepeatMode::Off => "off",
        RepeatMode::One => "one",
        RepeatMode::All => "all",
    }
}

/// Quel che arriva dal motore.
///
/// Gira su un filo del motore, mai su quello della callback audio: qui si può
/// prendere un lucchetto e scrivere sul database senza rischiare di spezzare il
/// suono.
fn su_evento(app: &tauri::AppHandle, evento: Evento) {
    let stato_lettore = app.state::<StatoLettore>();
    match evento {
        Evento::Iniziato { track_id } => {
            let _ = con_lettore(&stato_lettore, |lettore| {
                // Il brano precedente è finito **adesso**, non prima: con il
                // gapless fra i due non c'è un istante di silenzio, e questo è
                // l'unico momento in cui si sa che è successo.
                chiudi_ascolto(app, lettore);

                // La coda può essere indietro di un passo: il motore ha
                // attaccato il brano che gli era stato preparato.
                if lettore.coda.current() != Some(track_id) {
                    lettore.coda.advance(false);
                }

                let durata = lettore
                    .motore
                    .posizione()
                    .durata_ms
                    .max(durata_di(app, track_id));
                lettore.ascolto = Some(ListenTracker::begin(track_id, durata, adesso_ms()));

                prepara_prossimo(app, lettore);
                salva_coda(app, lettore);
                manda_stato(app, lettore);
                Ok(())
            });
        }
        Evento::Fermato => {
            let _ = con_lettore(&stato_lettore, |lettore| {
                chiudi_ascolto(app, lettore);
                manda_stato(app, lettore);
                Ok(())
            });
        }
        Evento::Errore(err) => {
            eprintln!(
                "[riproduzione] {} causa={}",
                err.code().kind().code(),
                err.cause().unwrap_or("—")
            );
            let _ = app.emit("riproduzione:errore", crate::errore::errore(*err));
        }
    }
}

/// La durata dichiarata dal database.
fn durata_di(app: &tauri::AppHandle, track_id: i64) -> u64 {
    let stato = app.state::<Stato>();
    con_libreria(&stato, |libreria| {
        aether_app::library::read_summary(&libreria.connection, track_id)
    })
    .ok()
    .flatten()
    .map_or(0, |b| u64::try_from(b.duration_ms).unwrap_or(0))
}

/// Avvia il filo che manda la posizione alla finestra.
///
/// Manda solo mentre suona: un'applicazione ferma in secondo piano non deve
/// svegliare il webview quattro volte al secondo per dirgli che non è cambiato
/// niente.
pub fn avvia_orologio(app: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("aether-orologio".to_owned())
        .spawn(move || {
            let mut ultimo_fermo = true;
            loop {
                std::thread::sleep(PASSO_TEMPO);
                let stato_lettore = app.state::<StatoLettore>();
                let tempo = con_lettore(&stato_lettore, |lettore| {
                    let p = lettore.motore.posizione();
                    Ok(Tempo {
                        posizione_ms: p.ms,
                        durata_ms: p.durata_ms,
                        in_pausa: p.in_pausa,
                    })
                });
                let Ok(tempo) = tempo else { continue };
                // Un colpo anche quando si è appena fermato, per non lasciare il
                // cursore a interpolare nel vuoto.
                if !tempo.in_pausa || !ultimo_fermo {
                    let _ = app.emit("riproduzione:tempo", tempo);
                }
                ultimo_fermo = tempo.in_pausa;
            }
        })
        .ok();
}

/// Avvia il filo che manda le bande dello spettro.
///
/// Separato dall'orologio e non un ramo dentro di esso: sono due cadenze
/// diverse per due ragioni diverse — vedi [`PASSO_SPETTRO`] — e infilarle nello
/// stesso ciclo avrebbe voluto dire mandare la posizione trenta volte al
/// secondo o le bande quattro.
///
/// Quando nessuno guarda dorme e non prende nessun lucchetto: la schermata
/// chiusa non costa niente né qui né nella callback audio.
pub fn avvia_spettro(app: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("aether-spettro".to_owned())
        .spawn(move || {
            loop {
                let stato_lettore = app.state::<StatoLettore>();
                if !stato_lettore
                    .spettro
                    .load(std::sync::atomic::Ordering::Relaxed)
                {
                    std::thread::sleep(PASSO_SPETTRO_FERMO);
                    continue;
                }
                let bande = con_lettore(&stato_lettore, |lettore| Ok(lettore.motore.spettro()));
                if let Ok(Some(bande)) = bande {
                    let _ = app.emit("riproduzione:spettro", bande);
                }
                std::thread::sleep(PASSO_SPETTRO);
            }
        })
        .ok();
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
        Ok(())
    });
}

// ── i comandi ───────────────────────────────────────────────────────────────

/// Accende o spegne lo spettro.
///
/// Due interruttori e non uno: quello del motore ferma la scrittura
/// nell'anello dalla callback audio, questo ferma l'evento. Spegnendoli si
/// smette di pagare in tutti e due i posti, ed è la ragione per cui il comando
/// esiste invece di lasciare lo spettro sempre acceso.
#[tauri::command]
pub fn spettro(stato: State<'_, StatoLettore>, attivo: bool) -> Esito<()> {
    stato
        .spettro
        .store(attivo, std::sync::atomic::Ordering::Relaxed);
    con_lettore(&stato, |lettore| {
        lettore.motore.guarda_spettro(attivo);
        Ok(())
    })
    .map_err(errore)
}

/// Fa partire una coda nuova a partire dal brano indicato.
#[tauri::command]
pub fn suona(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    brani: Vec<i64>,
    indice: usize,
) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        chiudi_ascolto(&app, lettore);
        lettore.coda.play_tracks(brani, indice, seme());
        avvia_corrente(&app, lettore)
    })
    .map_err(errore)
}

/// Apre il brano corrente della coda e lo fa partire.
fn avvia_corrente(app: &tauri::AppHandle, lettore: &mut Lettore) -> Result<(), AppError> {
    let Some(id) = lettore.coda.current() else {
        lettore.motore.ferma();
        salva_coda(app, lettore);
        manda_stato(app, lettore);
        return Ok(());
    };
    let sorgente = sorgente_di(app, id)?;
    lettore.motore.suona(sorgente);
    prepara_prossimo(app, lettore);
    salva_coda(app, lettore);
    // Lo stato parte subito, senza aspettare che il primo campione esca: il
    // titolo nella barra deve comparire al clic, non un decimo di secondo dopo.
    manda_stato(app, lettore);
    Ok(())
}

/// Mette in pausa.
#[tauri::command]
pub fn pausa(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        lettore.motore.pausa();
        if let Some(a) = lettore.ascolto.as_mut() {
            a.pause(adesso_ms());
        }
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Riprende.
#[tauri::command]
pub fn riprendi(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        // Coda ripresa dall'avvio: il motore non ha ancora niente in mano, e
        // «riprendi» deve voler dire «comincia».
        if lettore.motore.posizione().track_id.is_none() {
            return avvia_corrente(&app, lettore);
        }
        lettore.motore.riprendi();
        if let Some(a) = lettore.ascolto.as_mut() {
            a.resume(adesso_ms());
        }
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Alterna fra pausa e ripresa.
#[tauri::command]
pub fn alterna(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    let in_pausa =
        con_lettore(&stato, |lettore| Ok(lettore.motore.posizione().in_pausa)).map_err(errore)?;
    if in_pausa {
        riprendi(app, stato)
    } else {
        pausa(app, stato)
    }
}

/// Passa al brano dopo.
#[tauri::command]
pub fn prossimo(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        chiudi_ascolto(&app, lettore);
        match lettore.coda.advance(true) {
            Step::Track(_) => avvia_corrente(&app, lettore),
            Step::Restart => {
                lettore.motore.vai_a(0);
                manda_stato(&app, lettore);
                Ok(())
            }
            Step::Stop => {
                lettore.motore.ferma();
                manda_stato(&app, lettore);
                Ok(())
            }
        }
    })
    .map_err(errore)
}

/// Torna al brano prima, o ricomincia questo.
#[tauri::command]
pub fn precedente(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        let dove = lettore.motore.posizione().ms;
        match lettore.coda.previous(dove) {
            Step::Track(_) => {
                chiudi_ascolto(&app, lettore);
                avvia_corrente(&app, lettore)
            }
            // Ricominciare lo stesso brano non chiude l'ascolto: è ancora quello.
            Step::Restart => {
                lettore.motore.vai_a(0);
                manda_stato(&app, lettore);
                Ok(())
            }
            Step::Stop => Ok(()),
        }
    })
    .map_err(errore)
}

/// Salta a un punto del brano.
#[tauri::command]
pub fn vai_a(app: tauri::AppHandle, stato: State<'_, StatoLettore>, ms: u64) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        lettore.motore.vai_a(ms);
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Cambia volume e silenziamento.
#[tauri::command]
pub fn volume(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    volume: f32,
    muto: bool,
) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        lettore.volume = Volume {
            volume: volume.clamp(0.0, 1.0),
            muto,
        };
        lettore
            .motore
            .volume(lettore.volume.volume, lettore.volume.muto);
        let salvato = lettore.volume;
        let stato_app = app.state::<Stato>();
        let _ = con_libreria(&stato_app, |libreria| {
            playback::save_volume(&libreria.connection, salvato)
        });
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Cambia la curva dell'equalizzatore.
///
/// **Non manda [`StatoRiproduzione`]**, e non è una svista: comporre quello
/// stato richiede una lettura del brano corrente dal database, e questo comando
/// arriva una dozzina di volte al secondo finché un cursore è sotto il dito.
/// Parte invece `riproduzione:eq`, che sono due campi e nessuna query — quanto
/// basta perché il pannello nella barra e la pagina delle impostazioni restino
/// d'accordo fra loro.
#[tauri::command]
pub fn equalizzatore(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    guadagni: Vec<f32>,
    attivo: bool,
) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        lettore.motore.equalizzatore(&guadagni, attivo);
        let stato_app = app.state::<Stato>();
        // Il taglio e la normalizzazione li fa `save_eq`; si rilegge quel che ha
        // scritto invece di fidarsi di quel che è arrivato, così la finestra
        // vede lo stesso valore che rivedrà alla prossima apertura.
        let salvata = con_libreria(&stato_app, |libreria| {
            playback::save_eq(
                &libreria.connection,
                &Equalizzazione {
                    attivo,
                    guadagni: guadagni.clone(),
                },
            )?;
            playback::load_eq(&libreria.connection)
        });
        lettore.eq = salvata.unwrap_or(Equalizzazione { attivo, guadagni });
        let _ = app.emit(
            "riproduzione:eq",
            StatoEq {
                attivo: lettore.eq.attivo,
                guadagni: lettore.eq.guadagni.clone(),
            },
        );
        Ok(())
    })
    .map_err(errore)
}

/// Le curve fra cui si può scegliere: prima quelle di serie, poi le proprie.
///
/// In quest'ordine perché è quello in cui si guardano: chi apre l'elenco la
/// prima volta non ha curve sue, e chi ne ha vuole trovarle in fondo, sempre
/// nello stesso posto, invece che mescolate alfabeticamente alle altre.
#[tauri::command]
pub fn eq_preset_elenco(stato: State<'_, Stato>) -> Esito<Vec<VocePreset>> {
    let mut elenco: Vec<VocePreset> = PRESET_DI_SERIE
        .iter()
        .map(|(nome, guadagni)| VocePreset {
            nome: (*nome).to_owned(),
            guadagni: guadagni.to_vec(),
            di_serie: true,
        })
        .collect();
    let miei = con_libreria(&stato, |libreria| {
        playback::load_preset_eq(&libreria.connection)
    })
    .map_err(errore)?;
    elenco.extend(miei.into_iter().map(|p| VocePreset {
        nome: p.nome,
        guadagni: p.guadagni,
        di_serie: false,
    }));
    Ok(elenco)
}

/// Salva la curva corrente sotto un nome.
///
/// `false` se il nome era vuoto. Un nome che c'è già sostituisce quella curva:
/// la regola sta in `aether_app::playback::salva_preset`, con la sua ragione.
#[tauri::command]
pub fn eq_preset_salva(
    stato: State<'_, Stato>,
    stato_lettore: State<'_, StatoLettore>,
    nome: String,
) -> Esito<bool> {
    let guadagni =
        con_lettore(&stato_lettore, |lettore| Ok(lettore.eq.guadagni.clone())).map_err(errore)?;
    con_libreria(&stato, |libreria| {
        playback::salva_preset(&libreria.connection, &nome, &guadagni)
    })
    .map_err(errore)
}

/// Toglie una curva salvata. `false` se non ce n'era una con quel nome.
///
/// Quelle di serie non passano di qui: non stanno nel database, quindi non c'è
/// niente da togliere e la finestra non ne offre il comando.
#[tauri::command]
pub fn eq_preset_cancella(stato: State<'_, Stato>, nome: String) -> Esito<bool> {
    con_libreria(&stato, |libreria| {
        playback::cancella_preset(&libreria.connection, &nome)
    })
    .map_err(errore)
}

/// Lo stato corrente, per quando la finestra si apre.
#[tauri::command]
pub fn riproduzione_stato(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
) -> Esito<StatoRiproduzione> {
    con_lettore(&stato, |lettore| Ok(costruisci_stato(&app, lettore))).map_err(errore)
}

/// Accoda in fondo.
#[tauri::command]
pub fn coda_accoda(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    brani: Vec<i64>,
) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        let era_vuota = lettore.coda.is_empty();
        lettore.coda.enqueue(&brani);
        if era_vuota {
            return avvia_corrente(&app, lettore);
        }
        prepara_prossimo(&app, lettore);
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Mette subito dopo il brano corrente.
#[tauri::command]
pub fn coda_dopo(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    brani: Vec<i64>,
) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        let era_vuota = lettore.coda.is_empty();
        lettore.coda.play_next(&brani);
        if era_vuota {
            return avvia_corrente(&app, lettore);
        }
        prepara_prossimo(&app, lettore);
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Salta a una posizione della coda.
#[tauri::command]
pub fn coda_vai(app: tauri::AppHandle, stato: State<'_, StatoLettore>, indice: usize) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        // L'indice arriva dalla coda **come si vede**, cioè nell'ordine di
        // riproduzione: con lo shuffle acceso non è quello dell'elenco interno.
        if lettore.coda.play_at(indice).is_none() {
            return Ok(());
        }
        chiudi_ascolto(&app, lettore);
        avvia_corrente(&app, lettore)
    })
    .map_err(errore)
}

/// Toglie dalla coda.
#[tauri::command]
pub fn coda_togli(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    indice: usize,
) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        lettore.coda.remove_at(indice);
        prepara_prossimo(&app, lettore);
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Sposta un brano dentro la coda.
#[tauri::command]
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
#[tauri::command]
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
#[tauri::command]
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
