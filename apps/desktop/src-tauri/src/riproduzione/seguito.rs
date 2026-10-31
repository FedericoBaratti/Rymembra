//! Il lavoro lento del lettore, messo in fila fuori dal suo lucchetto: i fili
//! «rymembra-seguito» e «rymembra-scritture», e la fotografia dello stato,
//! scattata sotto il lucchetto e sviluppata fuori per la finestra e il sistema.

use std::time::Duration;

use crate::nota;
use crate::spegnimento::Emette as _;
use crate::stato::{Stato, con_libreria, con_libreria_entro};
use rymembra_app::library::{FormatoFile, TrackSummary};
use rymembra_app::playback;
use rymembra_domain::errors::{AppError, ErrorCodeKind};
use rymembra_domain::queue::RepeatMode;
use rymembra_play::Motore;
use tauri::Manager as _;

use super::fili::quanto_manca;
use super::ospiti::{self, Chi};
use super::suono::nome_normalizzazione;
use super::{
    ATTESA_SCHEDA, GuastoAudio, Lettore, PASSO_ATTESA, StatoLettore, StatoRiproduzione, insieme,
};

/// Un lavoro da fare dopo, in ordine, sul filo «rymembra-seguito».
///
/// Una chiusura e non un `enum` perché i lavori sono di forma diversa — lo
/// stato da comporre e mandare, un ascolto da scrivere, un evento piccolo per
/// la finestra — e l'unica cosa che hanno in comune è l'ordine in cui vanno
/// fatti.
pub(super) type Seguito = Box<dyn FnOnce(&tauri::AppHandle) + Send>;

/// Avvia il filo «rymembra-seguito» e restituisce il canale per parlargli.
///
/// # Perché esiste
///
/// Perché ogni cambiamento del lettore si porta dietro del lavoro lento: lo
/// stato da comporre — una lettura dal database per il titolo e i dati del
/// file — la scheda di Windows da aggiornare — che su un brano con la
/// copertina apre un file e aspetta il sistema — l'icona del vassoio, l'ascolto
/// da scrivere. Fino a ieri quel lavoro si faceva sul posto, dentro il
/// lucchetto del lettore, e su tre fili diversi: quello della finestra per i
/// comandi, quello della decodifica per gli annunci, quello dell'orologio.
/// Un database occupato bastava a tenere il lettore chiuso per secondi, e con
/// lui la pausa premuta dalla finestra.
///
/// Adesso dentro il lucchetto si prende la **fotografia** di quel che serve, e
/// il lavoro va qui. Un filo solo e un canale solo, ed è la proprietà che conta:
/// le fotografie si scattano sotto il lucchetto, quindi nell'ordine in cui il
/// lettore è cambiato, e questo filo le sviluppa nello stesso ordine. La
/// finestra vede gli stati nella sequenza in cui sono successi, come prima —
/// qualche millisecondo più tardi, e senza che nessuno aspetti.
///
/// # Le scritture no
///
/// Le scritture sul database che nascono sotto il lucchetto — l'ascolto
/// finito, il segno di «riprendi dov'eri» a zero, la posizione all'uscita —
/// non stanno qui ma sul filo delle scritture: vedi [`da_scrivere`]. Una
/// scrittura aspetta la libreria quanto serve, e qui dietro di lei restavano
/// fermi lo stato del brano nuovo, la scheda di Windows e il vassoio.
///
/// # Se il filo non parte
///
/// Si torna al lavoro sul posto: il canale ha l'altro capo chiuso, e
/// [`in_ordine`] se ne accorge alla prima spedizione. Lento come prima, ma non
/// muto.
pub(super) fn avvia_seguito(app: &tauri::AppHandle) -> std::sync::mpsc::Sender<Seguito> {
    avvia_fila(app, "rymembra-seguito", |_| {})
}

/// Avvia il filo «rymembra-scritture» e restituisce il canale per parlargli.
///
/// La stessa forma del seguito — un filo, un canale, i lavori nell'ordine in
/// cui sono arrivati — per le scritture che il seguito non deve aspettare:
/// vedi [`da_scrivere`]. Dopo ogni lavoro, riuscito o caduto, conta: è il
/// numero che [`pubblica`] guarda per sapere se l'ascolto di prima è già sul
/// disco.
pub(super) fn avvia_scritture(app: &tauri::AppHandle) -> std::sync::mpsc::Sender<Seguito> {
    avvia_fila(app, "rymembra-scritture", |app| {
        if let Some(stato) = app.try_state::<StatoLettore>() {
            stato
                .scritture_fatte
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
    })
}

/// Un filo che esegue i lavori in ordine, e il canale per mandarglieli.
///
/// `dopo_ogni_lavoro` gira dopo ciascuno, anche se è caduto.
fn avvia_fila(
    app: &tauri::AppHandle,
    nome: &'static str,
    dopo_ogni_lavoro: fn(&tauri::AppHandle),
) -> std::sync::mpsc::Sender<Seguito> {
    let (manda, ricevi) = std::sync::mpsc::channel::<Seguito>();
    let manico = app.clone();
    let avviato = std::thread::Builder::new()
        .name(nome.to_owned())
        .spawn(move || {
            for lavoro in ricevi {
                // Un lavoro che cade non deve portarsi via il filo: dietro di
                // lui ci sono tutti gli stati che la finestra deve ancora
                // ricevere, o le scritture che devono ancora arrivare sul
                // disco. La traccia la stampa il gancio del diario, prima di
                // arrivare qui.
                let esito =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lavoro(&manico)));
                if esito.is_err() {
                    nota!("[riproduzione] un lavoro di «{nome}» è caduto: si va avanti");
                }
                dopo_ogni_lavoro(&manico);
            }
        });
    if avviato.is_err() {
        nota!("[riproduzione] il filo «{nome}» non è partito: si lavora sul posto");
    }
    manda
}

/// Mette `lavoro` in fila sul filo «rymembra-seguito», dietro a quel che c'era.
///
/// Si chiama con il lucchetto del lettore in mano quando l'ordine rispetto agli
/// altri cambiamenti del lettore conta — lo stato, l'ascolto, gli eventi che la
/// finestra sovrappone allo stato — ed è questo che ne garantisce l'ordine.
/// Spedire costa un'allocazione e un `push`: niente che valga la pena di far
/// uscire dal lucchetto.
///
/// Se il filo non c'è — non è partito, o l'applicazione sta chiudendo — il
/// lavoro si fa qui, subito.
pub(crate) fn in_ordine(
    app: &tauri::AppHandle,
    lavoro: impl FnOnce(&tauri::AppHandle) + Send + 'static,
) {
    let Some(stato) = app.try_state::<StatoLettore>() else {
        lavoro(app);
        return;
    };
    if let Err(respinto) = stato.seguito.send(Box::new(lavoro)) {
        (respinto.0)(app);
    }
}

/// Mette una scrittura in fila sul filo «rymembra-scritture», dietro a quelle
/// di prima.
///
/// # Perché un filo suo, e non il seguito
///
/// Perché una scrittura aspetta la libreria **quanto serve**: `con_libreria`
/// fuori dalla finestra non rinuncia, e dentro SQLite c'è anche il
/// `busy_timeout`. Sul seguito, un ascolto finito mentre qualcuno teneva la
/// libreria a lungo — un'importazione di cronologia, la radio che legge
/// migliaia di impronte — teneva dietro di sé lo stato del brano nuovo:
/// titolo, copertina, coda, la scheda di Windows e il vassoio restavano sul
/// brano di prima per tutta la presa.
///
/// # Un filo solo per tutte, e l'ordine che serve
///
/// Ci passano **tutte** le scritture che nascono sotto il lucchetto del
/// lettore — l'ascolto, il segno a zero di [`super::fermato`], la posizione salvata
/// uscendo — e nessun'altra strada le scrive: si chiamano con il lucchetto in
/// mano, quindi arrivano sul disco nell'ordine in cui il lettore è cambiato.
/// Un ascolto di dopo non passa davanti a uno di prima, un segno a zero non
/// cancella una posizione più recente.
///
/// L'ordine rispetto agli stati non si perde del tutto: la cronologia della
/// finestra si rilegge quando il brano cambia, contando sul fatto che
/// l'ascolto di quello di prima sia già scritto. Per questo [`pubblica`]
/// aspetta le scritture messe in fila prima della sua fotografia, ma non più
/// di [`ATTESA_SCHEDA`]: nel caso normale l'ordine è quello di sempre, e con la
/// libreria occupata lo stato parte lo stesso.
///
/// Se il filo non c'è, si scrive qui, subito: lento come prima, ma non muto.
pub(super) fn da_scrivere(
    app: &tauri::AppHandle,
    lavoro: impl FnOnce(&tauri::AppHandle) + Send + 'static,
) {
    use std::sync::atomic::Ordering;
    let Some(stato) = app.try_state::<StatoLettore>() else {
        lavoro(app);
        return;
    };
    stato.scritture_chieste.fetch_add(1, Ordering::AcqRel);
    if let Err(respinto) = stato.scritture.send(Box::new(lavoro)) {
        (respinto.0)(app);
        stato.scritture_fatte.fetch_add(1, Ordering::AcqRel);
    }
}

/// Aspetta che il filo delle scritture abbia finito le prime `quante`, ma non
/// oltre `fine`. Dice se ci è arrivato.
fn aspetta_le_scritture(stato: &StatoLettore, quante: u64, fine: std::time::Instant) -> bool {
    aspetta_il_contatore(&stato.scritture_fatte, quante, fine)
}

/// Aspetta che `contatore` arrivi a `quante`, ma non oltre `fine`. Dice se ci
/// è arrivato.
///
/// Con una scadenza e a piccoli passi, come i lucchetti con una scadenza di
/// `riproduzione`: la regola è la stessa — chi aspetta qui ha qualcuno in fila
/// dietro di sé.
pub(super) fn aspetta_il_contatore(
    contatore: &std::sync::atomic::AtomicU64,
    quante: u64,
    fine: std::time::Instant,
) -> bool {
    loop {
        if contatore.load(std::sync::atomic::Ordering::Acquire) >= quante {
            return true;
        }
        if std::time::Instant::now() >= fine {
            return false;
        }
        std::thread::sleep(PASSO_ATTESA);
    }
}

/// Aspetta una risposta da ciascuno, ma tutti insieme non oltre `fine`. Dice
/// se hanno risposto tutti.
pub(super) fn aspetta_le_risposte(
    risposte: &[std::sync::mpsc::Receiver<()>],
    fine: std::time::Instant,
) -> bool {
    // Tutte, anche dopo una che non ha risposto: una fila il cui segnaposto è
    // caduto non deve togliere all'altra il tempo che resta.
    let mut tutte = true;
    for risposta in risposte {
        let resta = fine.saturating_duration_since(std::time::Instant::now());
        tutte &= risposta.recv_timeout(resta).is_ok();
    }
    tutte
}

/// Il secondo tempo di [`super::manda_stato`], sul filo del seguito.
///
/// Prima le scritture messe in fila prima della fotografia — l'ascolto del
/// brano appena finito, di solito — perché la cronologia della finestra si
/// rilegge quando il brano cambia, e conta di trovarci quell'ascolto. Ma con
/// una scadenza sola per quell'attesa e per la scheda, [`ATTESA_SCHEDA`]: una
/// libreria occupata non tiene fermo lo stato più di così. Vedi
/// [`da_scrivere`].
pub(super) fn pubblica(app: &tauri::AppHandle, abbozzo: Abbozzo) {
    let fine = std::time::Instant::now() + ATTESA_SCHEDA;
    if let Some(stato_lettore) = app.try_state::<StatoLettore>() {
        aspetta_le_scritture(&stato_lettore, abbozzo.scritture, fine);
    }
    let resta = fine.saturating_duration_since(std::time::Instant::now());
    let stato = completa(app, abbozzo, Some(resta));
    // Prima la finestra, che è chi l'utente sta guardando; poi Windows e il
    // vassoio, che per aggiornarsi possono dover aspettare il sistema.
    let copia = stato.clone();
    app.emetti("riproduzione:stato", stato);
    // Uscendo, né l'una né l'altro: la scheda e l'icona stanno per essere
    // staccate, e l'icona per aggiornarsi aspetta il filo della finestra — che
    // in quel momento sta aspettando questo filo, in `fili::salva_uscendo`.
    if crate::spegnimento::in_uscita() {
        return;
    }
    crate::media::aggiorna(app, &copia);
    crate::vassoio::riflette(app, &copia);
    crate::barra::riflette(app, &copia);
}

/// Lo stato come lo sa il lettore, senza la scheda del brano.
///
/// È la fotografia che [`super::manda_stato`] scatta dentro il lucchetto: tutto quel
/// che si legge dal lettore e dal motore, niente di quel che si legge dal
/// database. `stato.brano` e `stato.formato` sono vuoti, e la durata è quella
/// del motore finché [`completa`] non trova quella del database.
pub(super) struct Abbozzo {
    pub(super) stato: StatoRiproduzione,
    /// Di quale brano cercare la scheda.
    corrente: Option<i64>,
    /// Com'è fatto quel che si sente, e di quale brano: lo dice il motore, e
    /// [`completa`] lo usa per correggere le colonne del database.
    suonato: Option<(i64, rymembra_play::FormatoSuonato)>,
    /// Quante scritture erano in fila sul filo delle scritture quando la
    /// fotografia è stata scattata: [`pubblica`] aspetta che siano fatte, con
    /// una scadenza. Letto col lucchetto del lettore in mano, come le
    /// scritture che conta: sono esattamente quelle nate prima di questo stato.
    scritture: u64,
}

/// La fotografia del lettore: vedi [`Abbozzo`].
pub(super) fn abbozzo(app: &tauri::AppHandle, lettore: &Lettore) -> Abbozzo {
    let posizione = lettore.motore.posizione();
    // Un dispositivo perso è fermo, comunque la pensi il motore.
    //
    // `Condiviso::in_pausa` lo alzano solo pausa, stop e fine del brano: la
    // callback di `cpal` che segnala il guasto non lo tocca, e non deve — gira
    // dentro il backend audio. Il risultato era uno stato che diceva
    // `inPausa: false` con le cuffie staccate, e la finestra ci si ancorava: il
    // cursore continuava a interpolare da solo, oltre la fine del brano, mentre
    // il pulsante accanto mostrava «riprendi». Cioè esattamente la bugia che
    // `GuastoAudio` è stato aggiunto per togliere.
    //
    // Si corregge qui e non nella finestra perché di stato ce n'è uno: le
    // schermate che leggono `inPausa` sono tre, e ognuna che se lo aggiusti per
    // conto suo è un'occasione di non farlo.
    // `try_state` e non `state`: la fotografia si scatta anche dai fili di
    // sottofondo — gli annunci, l'orologio, il sorvegliante — e uno stato che
    // non c'è più vuol dire un'applicazione che si chiude, non un panico.
    let stato_lettore = app.try_state::<StatoLettore>();
    let guasto = guasto_di(&lettore.motore).map(|mut guasto| {
        // Il motore si è perso **sull'uscita fissata**: è lei che si aspetta.
        // `uscita_voluta` è un lucchetto foglia — nessuno lo tiene mentre
        // chiede quello del lettore — quindi prenderlo qui, col lettore in
        // mano, non cambia l'ordine di niente.
        let voluta = stato_lettore.as_ref().and_then(|stato| {
            stato
                .uscita_voluta
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        });
        // Non quando a cadere è stato il filo della decodifica: lì l'uscita è
        // al suo posto, e «in attesa delle cuffie» manderebbe a cercare un
        // cavo che non c'entra.
        guasto.attesa = voluta
            .filter(|nome| !lettore.motore.filo_caduto() && nome == lettore.motore.dispositivo());
        guasto
    });
    Abbozzo {
        // La voce di calibrazione non sta in coda: mentre suona, la barra
        // mostra lei e non il brano che ripartirà dopo, che con la posizione
        // dei clic sotto direbbe un punto che non esiste.
        corrente: insieme::corrente_da_mostrare(lettore),
        suonato: posizione.track_id.zip(posizione.formato),
        scritture: stato_lettore.as_ref().map_or(0, |stato| {
            stato
                .scritture_chieste
                .load(std::sync::atomic::Ordering::Acquire)
        }),
        stato: StatoRiproduzione {
            durata_ms: posizione.durata_ms,
            brano: None,
            in_pausa: posizione.in_pausa || guasto.is_some(),
            // In sessione, fra una partenza programmata e il primo campione
            // il motore dice ancora il punto del brano di prima: si mostra
            // quello promesso. Vedi `insieme::posizione_da_mostrare`.
            posizione_ms: insieme::posizione_da_mostrare(lettore, &posizione)
                .unwrap_or(posizione.ms),
            shuffle: lettore.coda.shuffle(),
            ripeti: nome_ripetizione(lettore.coda.repeat()).to_owned(),
            volume: lettore.volume.volume,
            muto: lettore.volume.muto,
            coda: lettore.coda.in_play_order(),
            posizione_coda: lettore.coda.position(),
            eq_attivo: lettore.eq.attivo,
            eq_guadagni: lettore.eq.guadagni.clone(),
            replaygain: nome_normalizzazione(lettore.normalizzazione).to_owned(),
            // Solo una lettura atomica: `try_state` restituisce il registro,
            // non prende il lucchetto — che del resto è già in mano a chi ci ha
            // chiamati.
            spegnimento_ms: stato_lettore.as_ref().and_then(|stato| quanto_manca(stato)),
            autoplay: lettore.autoplay,
            dissolvenza_s: lettore.dissolvenza_s,
            latenza_ms: lettore.latenza_ms,
            // Dal dominio, non da qui: è la stessa costante che `riga_attiva`
            // usa nel nucleo, e ripeterla qui sarebbe la terza copia di un
            // numero che questa release esiste in parte per ridurre a una.
            anticipo_ms: rymembra_domain::testo::ANTICIPO_MS,
            // Il motivo vale per il brano che **verrà**, non per uno qualunque:
            // se nel frattempo la coda è cambiata, la frase parlerebbe di un
            // brano che non c'è più. Confrontare costa un `Option<i64>` e toglie
            // l'unico modo in cui questa riga poteva mentire.
            motivo_prossimo: lettore
                .motivo_prossimo
                .filter(|(id, _)| Some(*id) == lettore.coda.peek_next())
                .map(|(_, codice)| codice.to_owned()),
            audio: guasto,
            uscita: Some(lettore.motore.dispositivo().to_owned()),
            formato: None,
            // Una lettura della fotografia che la sessione tiene per conto
            // suo: nessun lucchetto del lettore, che qui è già in mano. Vedi
            // `crate::insieme::nel_lettore`.
            insieme: crate::insieme::nel_lettore(app),
        },
    }
}

/// Aggiunge alla fotografia la scheda del brano, dal database o dal ricordo.
///
/// `attesa` è quanto aspettare la libreria: `None` vuol dire la regola di
/// sempre di `con_libreria` — quattro secondi sul filo della finestra, senza
/// limite altrove — e la usa chi chiede lo stato per disegnarsi la prima volta.
/// Il filo del seguito passa invece quel che resta di [`ATTESA_SCHEDA`] dopo
/// l'attesa delle scritture (vedi [`pubblica`]): dietro di lui aspettano tutti
/// gli stati che vengono dopo.
pub(super) fn completa(
    app: &tauri::AppHandle,
    abbozzo: Abbozzo,
    attesa: Option<Duration>,
) -> StatoRiproduzione {
    let Abbozzo {
        mut stato,
        corrente,
        suonato,
        scritture: _,
    } = abbozzo;
    // La riga e i suoi dati tecnici in **una** presa del lucchetto della
    // libreria, non due. Due `con_libreria` di seguito per lo stesso brano
    // sarebbero due attese su un mutex che la riapertura del database e la
    // scansione si passano di mano, per due query che durano microsecondi.
    //
    // La preferenza si legge **prima** della seconda query, e non dopo: se è
    // spenta la query non si fa affatto. Vale anche al contrario — la lettura
    // della preferenza è un'altra riga di `settings`, e sta dentro la stessa
    // presa per la stessa ragione.
    //
    // Quando la libreria non risponde, la scheda si prende dal ricordo: vedi
    // `StatoLettore::scheda_corrente`.
    // Gli stati con `try_state`: questa metà gira sul filo del seguito, e
    // uscendo gli stati possono non esserci più. Senza la libreria si legge il
    // ricordo; senza il lettore non c'è nemmeno un ricordo, e lo stato parte
    // senza scheda.
    let (brano, formato) = corrente.map_or((None, None), |id| {
        // Una voce di sessione la scheda ce l'ha già in memoria, e il ricordo
        // non le serve: non c'è un database da aspettare.
        let id = match ospiti::chi(app, id) {
            Chi::Libreria(riga) => riga,
            Chi::Voce(voce) => return scheda_di_voce(app, id, &voce, attesa),
            Chi::Sparita => return (None, None),
        };
        let Some(stato_lettore) = app.try_state::<StatoLettore>() else {
            return (None, None);
        };
        let leggi = |libreria: &mut crate::stato::Libreria| {
            let brano = rymembra_app::library::read_summary(&libreria.connection, id)?;
            let formato = if playback::load_formato_visibile(&libreria.connection)? {
                rymembra_app::library::read_formato(&libreria.connection, id)?
            } else {
                None
            };
            Ok((brano, formato))
        };
        let letto = app.try_state::<Stato>().and_then(|stato_app| match attesa {
            Some(entro) => con_libreria_entro(&stato_app, entro, leggi),
            None => Some(con_libreria(&stato_app, leggi)),
        });
        let mut ricordo = stato_lettore
            .scheda_corrente
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match letto {
            Some(Ok((brano, formato))) => {
                *ricordo = brano
                    .as_ref()
                    .map(|scheda| (id, scheda.clone(), formato.clone()));
                (brano, formato)
            }
            Some(Err(_)) | None => ricordo
                .as_ref()
                .filter(|(di, _, _)| *di == id)
                .map_or((None, None), |(_, scheda, formato)| {
                    (Some(scheda.clone()), formato.clone())
                }),
        }
    });
    if let Some(scheda) = brano.as_ref() {
        stato.durata_ms = u64::try_from(scheda.duration_ms).unwrap_or(0);
    }
    stato.brano = brano;
    // Quel che dice il motore vale solo per il brano di cui parla: fra la
    // scelta di un brano e il suo primo campione udibile la coda è già sul
    // nuovo, e il motore ancora sul vecchio. In quell'istante si mostrano le
    // colonne, e lo stato del segno dopo le corregge.
    stato.formato =
        formato.map(
            |dal_database| match suonato.filter(|(di, _)| Some(*di) == corrente) {
                Some((_, quel_che_si_sente)) => dal_database.completato_da(&quel_che_si_sente),
                None => dal_database,
            },
        );
    stato
}

/// La scheda e il formato di una voce di sessione, per [`completa`].
///
/// La scheda viene dalla voce. Il formato no: la riga dei dati tecnici si
/// mostra se la preferenza è accesa, e allora per una voce che apre un file
/// di questo PC si leggono le colonne della sua riga, per le altre si parte da
/// niente e lo riempie il decodificatore (vedi [`FormatoFile::completato_da`]):
/// di un file che sta su un altro PC il database di qui non sa niente, ma
/// quel che si sente lo sa il motore.
fn scheda_di_voce(
    app: &tauri::AppHandle,
    maniglia: i64,
    voce: &ospiti::Voce,
    attesa: Option<Duration>,
) -> (Option<TrackSummary>, Option<FormatoFile>) {
    let scheda = ospiti::scheda(maniglia, voce);
    let riga = ospiti::riga_dietro(app, maniglia);
    let leggi = |libreria: &mut crate::stato::Libreria| {
        if !playback::load_formato_visibile(&libreria.connection)? {
            return Ok(None);
        }
        let dal_database = match riga {
            Some(riga) => rymembra_app::library::read_formato(&libreria.connection, riga)?,
            None => None,
        };
        Ok(Some(dal_database.unwrap_or(FormatoFile {
            codec: None,
            sample_rate: None,
            channels: None,
            bitrate: None,
            bits_per_sample: None,
            output_rate: None,
        })))
    };
    let formato = app
        .try_state::<Stato>()
        .and_then(|stato_app| match attesa {
            Some(entro) => con_libreria_entro(&stato_app, entro, leggi),
            None => Some(con_libreria(&stato_app, leggi)),
        })
        .and_then(Result::ok)
        .flatten();
    (Some(scheda), formato)
}

/// Il guasto del motore, se ce n'è uno.
///
/// # Perché il codice non è sempre quello del dispositivo perso
///
/// Perché `rymembra_play` alza una bandiera sola per due guasti diversi — la
/// ragione sta in `Condiviso::perso`, e in breve è che la cura è la stessa: un
/// motore nuovo. Al banner però la differenza serve, perché le due frasi
/// mandano l'utente in due posti opposti: `playback.deviceLost` dice che
/// l'audio se n'è andato, cioè «guarda i cavi», e sarebbe il consiglio
/// sbagliato quando il dispositivo è al suo posto e a essersi interrotto è il
/// decodificatore. `playback.stalled` dice invece proprio quello, ed è già nel
/// catalogo e già tradotto.
///
/// La causa in chiaro resta quella del motore in tutti e due i casi: è la
/// stessa tabella, letta due volte, che dice «dispositivo non più disponibile»
/// o «il decodificatore si è interrotto».
pub(super) fn guasto_di(motore: &Motore) -> Option<GuastoAudio> {
    motore.causa_perdita().map(|causa| GuastoAudio {
        codice: if motore.filo_caduto() {
            ErrorCodeKind::PlaybackStalled
        } else {
            ErrorCodeKind::PlaybackDeviceLost
        }
        .code()
        .to_owned(),
        causa: causa.to_owned(),
        // Lo stesso bit letto una seconda volta, non una seconda verità: i
        // rami di `codice_perdita` sono quelli di `causa_perdita`, detti in
        // una parola che si può cercare in un dizionario invece che in una
        // frase che si può solo stampare.
        causa_codice: motore.codice_perdita().map(str::to_owned),
        riapribile: true,
        // Lo scrive [`abbozzo`], che sa qual è l'uscita fissata.
        attesa: None,
    })
}

/// Il guasto di un motore che non si è proprio aperto.
pub(super) fn guasto_di_apertura(err: &AppError) -> GuastoAudio {
    GuastoAudio {
        codice: err.code().kind().code().to_owned(),
        causa: err
            .cause()
            .unwrap_or("il motore audio non si è aperto")
            .to_owned(),
        // Niente codice della causa: qui la causa **è** un `AppError`, e il
        // suo codice sta già nel campo qui sopra. La finestra lo traduce da
        // lì, come traduce ogni altro errore del nucleo.
        causa_codice: None,
        riapribile: true,
        attesa: None,
    }
}

pub(super) const fn nome_ripetizione(repeat: RepeatMode) -> &'static str {
    match repeat {
        RepeatMode::Off => "off",
        RepeatMode::One => "one",
        RepeatMode::All => "all",
    }
}
