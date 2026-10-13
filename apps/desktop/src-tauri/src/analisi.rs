//! La passata sonora, dal lato della finestra.
//!
//! `aether_play::impronta` sa ricavare quarantasette numeri da trenta secondi di
//! audio; `aether_app::sonora` sa quali brani ne aspettano ancora e come si
//! scrivono. Nessuno dei due sa **quando** farlo, e il quando è tutto il
//! problema: l'analisi è l'unica cosa in Aether che legge dal disco a tutta
//! velocità senza che nessuno l'abbia chiesta, e il disco è la stessa risorsa da
//! cui dipende la riproduzione senza buchi.
//!
//! Qui si tiene il tempo e si prende il lucchetto, come fa `arricchimento` per i
//! metadati. Il resto sta altrove.
//!
//! # Due passate in una
//!
//! Dalla 3.0 la passata ha una seconda metà: finite le impronte, si misura
//! quanto forte suona ogni brano (`aether_app::intensita`), che è quel che
//! serve al volume uniforme per i brani senza tag ReplayGain. Stesse regole —
//! lucchetti, musica, radici che non rispondono — con un costo diverso: una
//! decodifica **intera** invece di trenta secondi. Vedi [`intensita`].
//!
//! # L'ordine dei lucchetti
//!
//! Come `nuvola`, `riproduzione` e `arricchimento` dichiarano il proprio, questo
//! modulo dichiara il suo:
//!
//! > **Il filo dell'analisi non prende mai il lucchetto del lettore, e prende
//! > quello della libreria solo in finestre brevi: una per leggere un lotto di
//! > candidati, una per scriverne le impronte.**
//!
//! In mezzo — aprire il file, cercare il centro, decodificare trenta secondi,
//! calcolare — non si tiene niente. Sono da qualche decina di millisecondi su un
//! MP3 locale a decine di secondi su una share che sta morendo, ed è esattamente
//! l'intervallo che non deve mai stare dentro un `Mutex` che la finestra
//! aspetta.
//!
//! Che sia vero non è affidato all'attenzione di chi legge:
//! [`aether_app::playback::sorgente_da_scheda`] non riceve mai una
//! `rusqlite::Connection`, quindi la versione sbagliata di questo filo non si
//! può nemmeno scrivere.
//!
//! # Perché cede alla riproduzione
//!
//! Il filo `aether-preparatore` apre il brano successivo **mentre** il corrente
//! suona, ed è ciò che rende il passaggio fra due tracce campione-esatto. Se
//! l'analisi gli contende la testina proprio in quel momento, il risultato è il
//! buco udibile fra una traccia e l'altra che la 2.1.0 ha chiuso — reintrodotto
//! da una funzione che l'utente non ha chiesto e non vede.
//!
//! Quindi: mentre la musica suona, questo filo non legge. Non rallenta, non
//! riduce i fili: si ferma e riprova fra [`RESPIRO`]. Il costo è che su un
//! computer usato solo per ascoltare l'analisi procede a singhiozzo; il
//! beneficio è che non si sente mai. Fra le due, è la riproduzione che è il
//! programma.
//!
//! Il segnale arriva da [`segna_riproduzione`], che l'orologio di
//! `riproduzione::fili` chiama quattro volte al secondo. Un `AtomicBool` e non
//! un lucchetto sul lettore, per la ragione scritta qui sopra: chiedere al
//! lettore come sta, per sapere se disturbarlo, sarebbe già disturbarlo.
//!
//! # Tre fili, non uno per core
//!
//! Il collo di bottiglia è la lettura, non il calcolo: un FLAC su share di rete
//! è I/O quasi puro, e la FFT su trenta secondi in mono a 22 kHz è dell'ordine
//! dei millisecondi. Con un filo per core si otterrebbero soltanto più richieste
//! in coda sulla stessa testina e più fastidio al preparatore. Tre è il numero
//! che copre la latenza di rete senza saturare niente. Per l'intensità i fili
//! sono due, e il perché sta in [`FILI_INTENSITA`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use aether_app::intensita as volume;
use aether_app::playback::SchedaSorgente;
use aether_app::scadenza::{Mancato, Operaio};
use aether_app::sonora::{self, Misurato};
use aether_domain::errors::AppError;
use tauri::{AppHandle, Manager as _};

use crate::nota;
use crate::stato::{Stato, adesso_ms, con_libreria};

/// Quanto si aspetta prima della prima passata.
///
/// Due minuti. Più dell'arricchimento, che ne aspetta uno e mezzo, perché
/// all'avvio la scansione sta ancora leggendo la stessa cartella e il ripristino
/// della coda sta ancora aprendo lo stesso disco. È anche il tempo in cui chi ha
/// appena aperto Aether decide cosa ascoltare — e se sceglie, [`RESPIRO`]
/// prende il posto di questo numero e l'analisi non comincia affatto.
const ATTESA_AVVIO: Duration = Duration::from_secs(2 * 60);

/// Ogni quanto si riprova, se non è successo niente.
///
/// Venti minuti. La passata non ha niente da inseguire: i brani nuovi arrivano
/// da una scansione, e una scansione chiama [`sporca`]. Questo battito serve
/// solo ai casi in cui la sveglia si è persa — un lotto interrotto da un disco
/// staccato, una passata finita mentre la musica suonava.
const INTERVALLO: Duration = Duration::from_secs(20 * 60);

/// Quanto si aspetta che la raffica di brani nuovi finisca.
const RAFFICA: Duration = Duration::from_secs(2 * 60);

/// Quanto si sta fermi quando la musica suona.
///
/// Trenta secondi. Abbastanza da non ricontrollare di continuo, abbastanza poco
/// da ripartire in fretta quando l'ascolto finisce.
const RESPIRO: Duration = Duration::from_secs(30);

/// Quanti brani si misurano insieme.
const FILI: usize = 3;

/// Quanto si concede a un brano prima di considerarlo irraggiungibile.
///
/// Venticinque secondi. La stessa forma della scadenza sull'apertura di un
/// brano, e per la stessa ragione: una share morta non risponde con un errore,
/// resta appesa. Chi scade diventa [`Misurato::NonRaggiungibile`], che ferma il
/// lotto invece di scrivere un fallimento per riga — perché il guasto è della
/// radice, non del file, e segnarlo su mille righe vorrebbe dire ritentarle
/// tutte fra un mese invece che stasera.
const SCADENZA_BRANO: Duration = Duration::from_secs(25);

/// Il nome del filo della scadenza, per il debugger.
const SCADENZA_NOME: &str = "impronta";

/// Quanti brani si misurano insieme per il volume.
///
/// Due e non i tre dell'impronta: qui il collo di bottiglia torna a essere il
/// calcolo — si decodifica il brano intero, non trenta secondi — e la passata
/// gira proprio quando la musica tace, cioè quando il computer può star
/// facendo dell'altro. Due fili finiscono una libreria in una sera e lasciano
/// libero il resto del processore.
const FILI_INTENSITA: usize = 2;

/// Il minimo che si concede a un brano da misurare, prima di contare la sua
/// durata.
///
/// La stessa base di [`SCADENZA_BRANO`]: aprire il file costa lo stesso, ed è
/// l'apertura che su una share morta resta appesa.
const SCADENZA_INTENSITA: Duration = Duration::from_secs(25);

/// Quante volte più in fretta del tempo reale deve andare, almeno, la
/// decodifica di un brano da misurare.
///
/// Venti. Un FLAC su un disco locale si decodifica a qualche centinaio di
/// volte il tempo reale, uno su una share in Wi-Fi a qualche decina; sotto le
/// venti volte la share non sta rispondendo. Alla base si aggiunge quindi la
/// durata divisa per venti: tre minuti di brano, nove secondi in più.
const VELOCITA_MINIMA: u32 = 20;

/// Il nome dei fili della misura, per il debugger.
const INTENSITA_NOME: &str = "intensita";

/// Vero mentre il lettore sta suonando.
///
/// Statico e non dentro [`StatoAnalisi`] perché chi lo scrive è l'orologio di
/// `riproduzione::fili`, che gira quattro volte al secondo e non deve fare una
/// ricerca nel registro degli stati di Tauri per farlo. `Relaxed` basta: non
/// protegge nessun altro dato, e leggerlo con un battito di ritardo vuol dire al
/// più un lotto analizzato mentre parte una canzone.
static SUONA: AtomicBool = AtomicBool::new(false);

/// Il battito della riproduzione dice se c'è musica.
///
/// La chiama `avvia_orologio`, in `riproduzione::fili`. Non è un comando e non
/// passa dalla finestra: è il lettore che parla al filo dell'analisi, e l'unica
/// cosa che si dicono.
pub fn segna_riproduzione(suona: bool) {
    SUONA.store(suona, Ordering::Relaxed);
}

/// Perché il filo si è svegliato.
///
/// Pubblico solo perché compare nel tipo del canale che `main.rs` passa a
/// [`avvia_filo`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sveglia {
    /// Sono entrati brani nuovi: aspetta che la raffica finisca.
    Sporca,
}

/// Quel che l'analisi tiene aperto mentre l'applicazione gira.
pub struct StatoAnalisi {
    /// Come si sveglia il filo.
    sveglia: Sender<Sveglia>,
}

impl StatoAnalisi {
    /// Costruisce lo stato e il capo del canale che il filo dovrà ascoltare.
    #[must_use]
    pub fn nuovo() -> (Self, Receiver<Sveglia>) {
        let (sveglia, orecchio) = channel();
        (Self { sveglia }, orecchio)
    }
}

/// Sono entrati brani nuovi.
///
/// La chiama chi ha appena finito una scansione, come fa con
/// `arricchimento::sporca`. Un canale chiuso vuol dire che il filo non c'è più:
/// non è un guasto di cui la scansione debba occuparsi.
pub fn sporca(app: &AppHandle) {
    if let Some(analisi) = app.try_state::<StatoAnalisi>() {
        let _ = analisi.sveglia.send(Sveglia::Sporca);
    }
}

/// Avvia il filo che analizza da solo.
///
/// Della stessa forma di [`crate::stato::avvia_filo_periodico`], da cui passano
/// nuvola, sincronia e arricchimento: un thread nominato, `recv_timeout` che fa
/// da periodicità **e** da antirimbalzo, nessun timer e nessun runtime
/// asincrono. Il ciclo però resta scritto qui, e il docblock di là dice perché:
/// quanto si aspetta dopo una passata lo decide il suo esito, e la raffica si
/// lascia passare a qualunque sveglia invece che a una di un tipo particolare.
pub fn avvia_filo(app: AppHandle, orecchio: Receiver<Sveglia>) {
    let avviato = std::thread::Builder::new()
        .name("aether-analisi".to_owned())
        .spawn(move || {
            let mut svegliato = match orecchio.recv_timeout(ATTESA_AVVIO) {
                // Canale chiuso: l'applicazione sta uscendo. Non si comincia
                // adesso una passata che nessuno aspetta più.
                Err(RecvTimeoutError::Disconnected) => return,
                Ok(_) => true,
                Err(RecvTimeoutError::Timeout) => false,
            };
            loop {
                // Solo chi è stato svegliato aspetta la raffica: una prima
                // scansione manda una sveglia per lotto, e millequattrocento
                // brani non sono millequattrocento passate. Chi si è svegliato
                // da sé — dal battito o dopo aver ceduto alla musica — non ha
                // nessuna raffica da lasciar passare, e due minuti di attesa
                // sarebbero due minuti in cui il disco è libero e fermo.
                if svegliato && crate::stato::aspetta_la_raffica(&orecchio, RAFFICA).is_break() {
                    return;
                }
                // Chi ha ceduto alla musica ritenta fra mezzo minuto, non fra
                // venti: su un computer acceso per ascoltare, l'intervallo
                // lungo vorrebbe dire tre tentativi a sera e una libreria mai
                // finita. Gli altri esiti non hanno fretta — i brani nuovi
                // arrivano da una scansione, e una scansione sveglia.
                let attesa = match passata(&app) {
                    Fine::Musica => RESPIRO,
                    Fine::Finito | Fine::Irraggiungibile | Fine::Guasto => INTERVALLO,
                    // Non si aspetta niente: non c'è nessun dopo.
                    Fine::Uscita => return,
                };
                svegliato = match orecchio.recv_timeout(attesa) {
                    Err(RecvTimeoutError::Disconnected) => return,
                    Ok(_) => true,
                    // Il timeout **è** il battito periodico: nessun timer.
                    Err(RecvTimeoutError::Timeout) => false,
                };
            }
        });
    if let Err(err) = avviato {
        nota!("[avvio] il filo dell'analisi non è partito: {err}");
    }
}

/// Perché una passata è finita.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fine {
    /// Non c'è più niente da analizzare.
    Finito,
    /// È cominciata la musica: si riprende dopo.
    Musica,
    /// Una radice non risponde: si riprende al prossimo battito.
    Irraggiungibile,
    /// Il database ha detto di no.
    Guasto,
    /// L'applicazione si sta chiudendo: si smette e non si riprende.
    Uscita,
}

/// Una passata. Non fallisce mai rumorosamente.
///
/// Gira per conto suo, e un `Err` propagato non avrebbe nessuno a cui arrivare.
/// A differenza dell'arricchimento non manda niente alla finestra: l'analisi non
/// scrive nei file dell'utente, non consuma quote di nessun servizio e non ha
/// niente da farsi perdonare. Quel che produce si vede da solo, nella coda che
/// dopo qualche giorno comincia a proporre brani che c'entrano, e nel volume
/// che smette di saltare da un disco all'altro.
///
/// Prima le impronte, poi l'intensità: le impronte costano trenta secondi di
/// lettura a brano e finiscono presto, l'intensità un brano intero. Solo una
/// metà finita lascia il posto all'altra — la musica, la chiusura o una radice
/// che non risponde fermano tutte e due.
fn passata(app: &AppHandle) -> Fine {
    match impronte(app) {
        Fine::Finito => intensita(app),
        altro => altro,
    }
}

/// La prima metà della passata: le impronte.
fn impronte(app: &AppHandle) -> Fine {
    let mut fatti = 0_usize;
    let mut scritti = false;
    let fine = loop {
        // Prima di ogni lotto, non solo all'inizio: una passata su una libreria
        // grande dura minuti, e la musica può cominciare in mezzo.
        if SUONA.load(Ordering::Relaxed) {
            break Fine::Musica;
        }
        // Idem per la chiusura, e per la stessa ragione: un lotto dura secondi,
        // e in quei secondi la finestra può essere già andata. Quel che si è
        // misurato finora si scrive comunque — la finestra 3 è già passata — e
        // quel che manca resta candidato per il prossimo avvio.
        if crate::spegnimento::in_uscita() {
            break Fine::Uscita;
        }

        // ── finestra 1: quali brani. ────────────────────────────────────────
        let schede = match leggi_lotto(app) {
            Ok(schede) => schede,
            Err(err) => {
                nota!(
                    "[analisi] lotto non letto codice={} causa={}",
                    err.code().kind().code(),
                    err.cause().unwrap_or("—")
                );
                break Fine::Guasto;
            }
        };
        if schede.is_empty() {
            break Fine::Finito;
        }

        // ── in mezzo: nessun lucchetto. ─────────────────────────────────────
        let misurati = misura_lotto(&schede);
        // Una radice che non risponde si abbandona invece di interrogarla file
        // per file: è la lezione che la 2.1.0 ha scritto per la scansione, e
        // qui vale identica. Quel che si è già misurato si scrive lo stesso.
        let fermarsi = misurati
            .iter()
            .any(|m| matches!(m, Misurato::NonRaggiungibile));
        let buoni: Vec<Misurato> = misurati
            .into_iter()
            .filter(|m| !matches!(m, Misurato::NonRaggiungibile))
            .collect();

        // ── finestra 2: scrivere. ───────────────────────────────────────────
        if !buoni.is_empty() {
            match scrivi(app, &buoni) {
                Ok(quanti) => {
                    fatti = fatti.saturating_add(quanti);
                    scritti = true;
                }
                Err(err) => {
                    nota!(
                        "[analisi] impronte non scritte codice={} causa={}",
                        err.code().kind().code(),
                        err.cause().unwrap_or("—")
                    );
                    break Fine::Guasto;
                }
            }
        }
        if fermarsi {
            break Fine::Irraggiungibile;
        }
    };

    // La scala si rifà una volta per passata, non una per lotto: rilegge tutte
    // le impronte della libreria, ed è la sola cosa qui dentro il cui costo
    // cresce col quadrato delle occasioni in cui la si chiama.
    if scritti {
        match ritara(app) {
            Ok(Some(quanti)) => nota!(
                "[analisi] {fatti} impronte nuove, scala rifatta su {quanti} brani ({fine:?})"
            ),
            // Sotto il minimo la scala non esiste ancora, e non è un guasto: è
            // una libreria piccola, o una passata appena cominciata.
            Ok(None) => nota!("[analisi] {fatti} impronte nuove, ancora poche per una scala"),
            Err(err) => nota!(
                "[analisi] scala non rifatta codice={} causa={}",
                err.code().kind().code(),
                err.cause().unwrap_or("—")
            ),
        }
    }
    fine
}

/// Il prossimo lotto di brani da misurare.
///
/// Nessuno stato vuol dire che l'applicazione lo ha già lasciato cadere: si
/// risponde «niente da misurare», che è la verità e fa finire la passata dalla
/// porta normale. Vedi [`crate::spegnimento`] per perché `try_state` e non
/// `state` in tutto questo file: `state` panica, e questo filo gira anche
/// mentre la finestra si chiude.
fn leggi_lotto(app: &AppHandle) -> Result<Vec<SchedaSorgente>, AppError> {
    let Some(stato) = app.try_state::<Stato>() else {
        return Ok(Vec::new());
    };
    con_libreria(&stato, |libreria| {
        sonora::candidati(&libreria.connection, adesso_ms(), sonora::LOTTO)
    })
}

/// Le impronte di un lotto, scritte in una transazione.
///
/// Senza stato non si scrive niente e non si è scritto niente: i brani restano
/// candidati, e la passata dopo li ripesca.
///
/// La transazione è `Immediate`, qui e in [`ritara`]: legge e poi scrive, e una
/// transazione normale che prova a diventare di scrittura mentre l'arricchimento
/// scrive dalla sua connessione riceve `SQLITE_BUSY` subito, senza l'attesa che
/// il `busy_timeout` concede a chi chiede il lucchetto all'inizio.
fn scrivi(app: &AppHandle, misurati: &[Misurato]) -> Result<usize, AppError> {
    let Some(stato) = app.try_state::<Stato>() else {
        return Ok(0);
    };
    con_libreria(&stato, |libreria| {
        let tx = libreria
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|err| db_errore("apertura della transazione delle impronte", &err))?;
        let quanti = sonora::registra(&tx, misurati, adesso_ms())?;
        tx.commit()
            .map_err(|err| db_errore("chiusura della transazione delle impronte", &err))?;
        Ok(quanti)
    })
}

/// Rifà la scala su tutta la libreria.
///
/// Senza stato la scala resta quella di prima, che è vecchia di un lotto e non
/// di più: la passata dopo la rifà.
fn ritara(app: &AppHandle) -> Result<Option<usize>, AppError> {
    let Some(stato) = app.try_state::<Stato>() else {
        return Ok(None);
    };
    con_libreria(&stato, |libreria| {
        let tx = libreria
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|err| db_errore("apertura della transazione della scala", &err))?;
        let quanti = sonora::ritara(&tx, adesso_ms())?;
        tx.commit()
            .map_err(|err| db_errore("chiusura della transazione della scala", &err))?;
        Ok(quanti)
    })
}

/// Misura un lotto su [`FILI`] fili, senza tenere niente.
///
/// I pezzi si dividono in anticipo invece di pescare da una coda condivisa:
/// dividere male costa qualche secondo alla fine del lotto, una coda condivisa
/// costerebbe un lucchetto in più — e questo è il modulo che sui lucchetti ha
/// appena scritto tre paragrafi.
fn misura_lotto(schede: &[SchedaSorgente]) -> Vec<Misurato> {
    if schede.is_empty() {
        return Vec::new();
    }
    let per_filo = schede.len().div_ceil(FILI).max(1);
    std::thread::scope(|ambito| {
        let mani: Vec<_> = schede
            .chunks(per_filo)
            .map(|pezzo| ambito.spawn(move || pezzo.iter().map(misura_uno).collect::<Vec<_>>()))
            .collect();
        mani.into_iter()
            // Un filo che è morto non ha misurato niente, e i suoi brani
            // restano candidati: la passata dopo li ripesca. È l'unico modo di
            // perdere lavoro qui dentro, e costa una rilettura.
            .filter_map(|mano| mano.join().ok())
            .flatten()
            .collect()
    })
}

/// Un brano, con la scadenza addosso.
fn misura_uno(scheda: &SchedaSorgente) -> Misurato {
    let copia = scheda.clone();
    aether_app::scadenza::con_scadenza(SCADENZA_NOME, SCADENZA_BRANO, move || {
        sonora::misura(&aether_app::files::LocalFiles, &copia)
    })
    .unwrap_or(Misurato::NonRaggiungibile)
}

/// La seconda metà della passata: quanto forte suona ogni brano.
///
/// La forma è quella di [`impronte`] — leggere un lotto, misurare senza
/// lucchetti, scrivere — con due differenze. La musica si guarda anche fra un
/// brano e l'altro dentro il lotto, non solo fra un lotto e l'altro: un lotto
/// d'impronte dura secondi, uno di brani interi può durare un minuto, e il
/// preparatore non può aspettarlo. E un brano che fa cadere il decodificatore
/// si segna illeggibile invece di fermare la passata: è il contenuto a essere
/// storto, e senza segnarlo lo stesso brano — il più recente, quindi il primo
/// candidato — si ripresenterebbe in testa a ogni passata.
fn intensita(app: &AppHandle) -> Fine {
    let mut fatti = 0_usize;
    let fine = loop {
        if SUONA.load(Ordering::Relaxed) {
            break Fine::Musica;
        }
        if crate::spegnimento::in_uscita() {
            break Fine::Uscita;
        }

        // ── finestra 1: quali brani. ────────────────────────────────────────
        let Some(stato) = app.try_state::<Stato>() else {
            break Fine::Uscita;
        };
        let schede = match con_libreria(&stato, |libreria| {
            volume::candidati(&libreria.connection, adesso_ms(), volume::LOTTO)
        }) {
            Ok(schede) => schede,
            Err(err) => {
                nota!(
                    "[analisi] lotto del volume non letto codice={} causa={}",
                    err.code().kind().code(),
                    err.cause().unwrap_or("—")
                );
                break Fine::Guasto;
            }
        };
        if schede.is_empty() {
            break Fine::Finito;
        }

        // ── in mezzo: nessun lucchetto. ─────────────────────────────────────
        let misurati = misura_intensita(&schede);
        let fermarsi = misurati
            .iter()
            .any(|m| matches!(m, volume::Misurato::NonRaggiungibile));
        let buoni: Vec<volume::Misurato> = misurati
            .into_iter()
            .filter(|m| !matches!(m, volume::Misurato::NonRaggiungibile))
            .collect();

        // ── finestra 2: scrivere. ───────────────────────────────────────────
        if !buoni.is_empty() {
            let scritto = con_libreria(&stato, |libreria| {
                let tx = libreria
                    .connection
                    .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                    .map_err(|err| db_errore("apertura della transazione del volume", &err))?;
                let quanti = volume::registra(&tx, &buoni, adesso_ms())?;
                tx.commit()
                    .map_err(|err| db_errore("chiusura della transazione del volume", &err))?;
                Ok(quanti)
            });
            match scritto {
                Ok(quanti) => fatti = fatti.saturating_add(quanti),
                Err(err) => {
                    nota!(
                        "[analisi] misure del volume non scritte codice={} causa={}",
                        err.code().kind().code(),
                        err.cause().unwrap_or("—")
                    );
                    break Fine::Guasto;
                }
            }
        }
        if fermarsi {
            break Fine::Irraggiungibile;
        }
    };
    if fatti > 0 {
        nota!("[analisi] {fatti} brani misurati per il volume ({fine:?})");
    }
    fine
}

/// Misura un lotto su [`FILI_INTENSITA`] fili, senza tenere niente.
///
/// Ogni filo ha un [`Operaio`] suo: un filo creato per brano sarebbe uno ogni
/// pochi decimi di secondo, e l'operaio distingue un brano che fa cadere il
/// decodificatore da uno che non torna — le due cose che qui vanno trattate
/// all'opposto. Prima di ogni brano si guarda se la musica è ripartita: i
/// brani rimasti non si misurano, non si scrivono, e restano candidati.
fn misura_intensita(schede: &[SchedaSorgente]) -> Vec<volume::Misurato> {
    if schede.is_empty() {
        return Vec::new();
    }
    let per_filo = schede.len().div_ceil(FILI_INTENSITA).max(1);
    std::thread::scope(|ambito| {
        let mani: Vec<_> = schede
            .chunks(per_filo)
            .map(|pezzo| {
                ambito.spawn(move || {
                    let mut operaio = Operaio::nuovo(INTENSITA_NOME);
                    pezzo
                        .iter()
                        .take_while(|_| {
                            !SUONA.load(Ordering::Relaxed) && !crate::spegnimento::in_uscita()
                        })
                        .map(|scheda| misura_volume(&mut operaio, scheda))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        mani.into_iter()
            .filter_map(|mano| mano.join().ok())
            .flatten()
            .collect()
    })
}

/// Un brano, con una scadenza che cresce con la sua durata.
fn misura_volume(operaio: &mut Operaio, scheda: &SchedaSorgente) -> volume::Misurato {
    let copia = scheda.clone();
    match operaio.esegui(scadenza_volume(scheda), move || {
        volume::misura(&aether_app::files::LocalFiles, &copia)
    }) {
        Ok(misurato) => misurato,
        // Il file è arrivato ed è il contenuto a essere storto: si segna, e
        // si ritenta fra un mese come ogni illeggibile.
        Err(Mancato::Caduto { messaggio }) => {
            nota!(
                "[analisi] il decodificatore è caduto misurando il volume brano={} causa={}",
                scheda.track_id,
                messaggio.as_deref().unwrap_or("—")
            );
            volume::Misurato::Negata {
                track_id: scheda.track_id,
                esito: volume::ILLEGGIBILE,
            }
        }
        Err(Mancato::Scaduto) => volume::Misurato::NonRaggiungibile,
    }
}

/// Quanto si concede a un brano: la base più la sua durata a
/// [`VELOCITA_MINIMA`].
fn scadenza_volume(scheda: &SchedaSorgente) -> Duration {
    let durata = Duration::from_millis(u64::try_from(scheda.durata_ms).unwrap_or(0));
    SCADENZA_INTENSITA.saturating_add(durata / VELOCITA_MINIMA)
}

/// Un guasto del database, nella forma del catalogo.
///
/// Delega a [`aether_app::db::codice_da_sqlite`], che guarda il codice di SQLite
/// e distingue un database occupato — che si riprova — da una share che non
/// risponde. Prima rispondeva `db.queryFailed` a qualunque guasto, e chi la
/// leggeva si trovava consigliato di riavviare Aether per un file che era
/// occupato da mezzo secondo dalla scansione.
fn db_errore(cosa: &str, err: &rusqlite::Error) -> AppError {
    aether_app::db::codice_da_sqlite(cosa, err)
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Il segnale della riproduzione va e viene.
    ///
    /// Sembra una prova su niente, ed è la prova che il filo dell'analisi ha un
    /// modo di sapere che la musica suona **senza** chiedere al lettore. Il
    /// giorno in cui qualcuno sostituisce l'atomica con una lettura di
    /// `con_lettore`, questa continua a passare e il gapless si rompe — quindi
    /// vale anche la riga di commento che la accompagna.
    #[test]
    fn il_battito_alza_e_abbassa_il_segnale() {
        segna_riproduzione(true);
        assert!(SUONA.load(Ordering::Relaxed), "il filo non sa che si suona");
        segna_riproduzione(false);
        assert!(
            !SUONA.load(Ordering::Relaxed),
            "il filo crede che si suoni ancora"
        );
    }

    /// Un lotto vuoto non fa partire nessun filo e non è un guasto.
    #[test]
    fn un_lotto_vuoto_non_misura_niente() {
        assert!(misura_lotto(&[]).is_empty());
        assert!(misura_intensita(&[]).is_empty());
    }

    /// La scadenza della misura cresce con la durata, e non scende mai sotto
    /// la base: un brano di durata ignota ha comunque il tempo di aprirsi.
    #[test]
    fn la_scadenza_del_volume_cresce_con_la_durata() {
        let scheda = |durata_ms| SchedaSorgente {
            track_id: 1,
            collocazione: aether_app::playback::Collocazione::File("x.flac".to_owned()),
            durata_ms,
            replaygain_db: None,
            picco: None,
        };
        assert_eq!(scadenza_volume(&scheda(0)), SCADENZA_INTENSITA);
        assert_eq!(scadenza_volume(&scheda(-5)), SCADENZA_INTENSITA);
        // Un'ora: tre minuti in più.
        assert_eq!(
            scadenza_volume(&scheda(3_600_000)),
            SCADENZA_INTENSITA + Duration::from_secs(180)
        );
    }

    /// La divisione in pezzi copre tutte le schede, comunque siano tante.
    ///
    /// `chunks` va in panico con zero, e `div_ceil` di un lotto più piccolo del
    /// numero dei fili dà uno: la prova serve a fissare che non ci sia una
    /// libreria di due brani che fa cadere il filo di sottofondo.
    #[test]
    fn i_pezzi_coprono_il_lotto_a_ogni_misura() {
        for quante in 1..=(sonora::LOTTO * 2) {
            let per_filo = quante.div_ceil(FILI).max(1);
            let finte = vec![0_u8; quante];
            let pezzi: Vec<_> = finte.chunks(per_filo).collect();
            assert!(!pezzi.is_empty(), "nessun pezzo per {quante} schede");
            assert!(
                pezzi.len() <= FILI,
                "più di {FILI} fili per {quante} schede"
            );
            let somma: usize = pezzi.iter().map(|p| p.len()).sum();
            assert_eq!(somma, quante, "il lotto da {quante} ha perso delle schede");
        }
    }
}
