//! La passata sonora, dal lato della finestra.
//!
//! `rymembra_play::impronta` sa ricavare quarantasette numeri da trenta secondi di
//! audio; `rymembra_app::sonora` sa quali brani ne aspettano ancora e come si
//! scrivono. Nessuno dei due sa **quando** farlo, e il quando è tutto il
//! problema: l'analisi è l'unica cosa in Rymembra che legge dal disco a tutta
//! velocità senza che nessuno l'abbia chiesta, e il disco è la stessa risorsa da
//! cui dipende la riproduzione senza buchi.
//!
//! Qui si tiene il tempo e si prende il lucchetto, come fa `arricchimento` per i
//! metadati. Il resto sta altrove.
//!
//! # Due passate in una
//!
//! Dalla 3.0 la passata ha una seconda metà: finite le impronte, si misura
//! quanto forte suona ogni brano (`rymembra_app::intensita`), che è quel che
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
//! [`rymembra_app::playback::sorgente_da_scheda`] non riceve mai una
//! `rusqlite::Connection`, quindi la versione sbagliata di questo filo non si
//! può nemmeno scrivere.
//!
//! # Perché cede alla riproduzione
//!
//! Il filo `rymembra-preparatore` apre il brano successivo **mentre** il corrente
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
//! # L'eccezione: i brani del cloud, «mentre suonano»
//!
//! Un brano di OneDrive (o di un altro servizio) non si scarica apposta per
//! misurarlo: si misura la copia che l'ascolto ha già lasciato completa nella
//! cache ([`misura_dal_cloud`]). E si misura **subito**, anche se la musica
//! suona, con un filo solo e un brano alla volta. Non contraddice la regola
//! qui sotto, che esiste per non contendere la testina — o la share — al brano
//! in corso: la copia sta sul disco locale dei dati, e il brano in corso, se è
//! del cloud, arriva dalla rete. Aspettare la pausa vorrebbe dire, in un
//! ascolto continuo, non arrivarci mai: la cache ha un tetto, e una copia
//! trattenuta per la misura se ne va lo stesso quando la cache supera il
//! doppio del tetto.
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

use rymembra_app::intensita as volume;
use rymembra_app::playback::SchedaSorgente;
use rymembra_app::scadenza::{Mancato, Operaio};
use rymembra_app::sonora::{self, Misurato};
use rymembra_domain::errors::AppError;
use tauri::{AppHandle, Manager as _};

use crate::nota;
use crate::stato::{Stato, adesso_ms, con_libreria};

/// Quanto si aspetta prima della prima passata.
///
/// Due minuti. Più dell'arricchimento, che ne aspetta uno e mezzo, perché
/// all'avvio la scansione sta ancora leggendo la stessa cartella e il ripristino
/// della coda sta ancora aprendo lo stesso disco. È anche il tempo in cui chi ha
/// appena aperto Rymembra decide cosa ascoltare — e se sceglie, [`RESPIRO`]
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
/// lotto — gli altri fili non prendono un brano nuovo, vedi
/// [`misura_lotto_con`] — invece di scrivere un fallimento per riga: il guasto
/// è della radice, non del file, e segnarlo su mille righe vorrebbe dire
/// ritentarle tutte fra un mese invece che stasera.
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
///
/// # Sorvegliato
///
/// Ogni passata gira dentro [`crate::stato::sorvegliato`], come quelle dei fili
/// che passano da `avvia_filo_periodico`. Prima non era così: un panico in una
/// passata srotolava fuori dal ciclo, e il filo finiva — niente più impronte né
/// volume per il resto della sessione, e niente che lo dicesse. Adesso diventa
/// una riga nel diario e una voce nella salute dei fili, e la passata dopo
/// arriva dopo [`crate::stato::attesa_dopo_la_caduta`] — mai oltre
/// [`INTERVALLO`] — o prima, se una scansione sveglia il filo.
///
/// La sorveglianza è la rete sotto la rete: un brano che fa cadere il
/// decodificatore è già fermato più in basso, uno per uno, da [`misura_uno`] e
/// da [`misura_volume`]. Qui arrivano solo i panici che nessuno ha previsto.
pub fn avvia_filo(app: AppHandle, orecchio: Receiver<Sveglia>) {
    let avviato = std::thread::Builder::new()
        .name("rymembra-analisi".to_owned())
        .spawn(move || {
            let mut svegliato = match orecchio.recv_timeout(ATTESA_AVVIO) {
                // Canale chiuso: l'applicazione sta uscendo. Non si comincia
                // adesso una passata che nessuno aspetta più.
                Err(RecvTimeoutError::Disconnected) => return,
                Ok(_) => true,
                Err(RecvTimeoutError::Timeout) => false,
            };
            let mut cadute_di_fila = 0_u32;
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
                let attesa = match crate::stato::sorvegliato("rymembra-analisi", || passata(&app)) {
                    Some(fine) => {
                        cadute_di_fila = 0;
                        match fine {
                            // Una scansione in corso si aspetta come la musica:
                            // mezzo minuto e si riguarda. Non c'è altra sveglia
                            // su cui contare — una scansione che non ha cambiato
                            // niente non chiama [`sporca`] — e venti minuti
                            // sarebbero una passata persa per una scansione da
                            // dieci secondi.
                            Fine::Musica | Fine::Scansione => RESPIRO,
                            Fine::Finito | Fine::Irraggiungibile | Fine::Guasto => INTERVALLO,
                            // Non si aspetta niente: non c'è nessun dopo.
                            Fine::Uscita => return,
                        }
                    }
                    // Caduta in panico: `sorvegliato` l'ha già scritto. Si
                    // riprova presto la prima volta e sempre più piano dopo,
                    // perché un panico che dipende da un file torna identico.
                    None => {
                        cadute_di_fila = cadute_di_fila.saturating_add(1);
                        crate::stato::attesa_dopo_la_caduta(cadute_di_fila).min(INTERVALLO)
                    }
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
    /// C'è una scansione in corso: si riprende quando ha finito.
    Scansione,
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
/// metà finita lascia il posto all'altra — la musica, una scansione, la
/// chiusura o una radice che non risponde fermano tutte e due.
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
        // E una scansione, prima di ogni lotto: vedi [`scansione_in_corso`].
        if scansione_in_corso(app) {
            break Fine::Scansione;
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
        // qui vale identica. Dentro il lotto la ferma `misura_lotto_con` al
        // primo brano che non arriva; qui si ferma la passata. Quel che si è
        // già misurato si scrive lo stesso.
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

/// Una scansione tiene il turno, adesso.
///
/// L'analisi non parte mentre ce n'è una, e si ferma fra un lotto e l'altro se
/// ne comincia una. Le due vogliono le stesse cose — il disco, spesso la
/// stessa share, e il lucchetto della libreria per scrivere — e la scansione
/// ha qualcuno che la aspetta: chi ha premuto «Scansiona», o un CD nuovo che
/// deve comparire. Una lettura del turno, senza prenderlo: vedi
/// `Stato::scansione_in_corso`. Senza stato non c'è nessuna scansione.
fn scansione_in_corso(app: &AppHandle) -> bool {
    app.try_state::<Stato>()
        .is_some_and(|stato| stato.scansione_in_corso())
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

// ── i brani del cloud ───────────────────────────────────────────────────────

/// Una copia completa da misurare: la sua chiave nella cache.
type DaMisurare = rymembra_cloud::musica::Chiave;

/// Il canale del filo che misura le copie del cloud, avviato alla prima
/// richiesta.
static MISURE_DAL_CLOUD: std::sync::OnceLock<Sender<(AppHandle, DaMisurare)>> =
    std::sync::OnceLock::new();

/// Misura i brani di una copia appena completata nella cache.
///
/// Torna subito: la richiesta va al filo `rymembra-analisi-cloud`, che misura
/// un brano alla volta. Vedi «L'eccezione» nel commento in testa.
pub fn misura_dal_cloud(app: &AppHandle, chiave: DaMisurare) {
    let posta = MISURE_DAL_CLOUD.get_or_init(|| {
        let (posta, orecchio) = channel::<(AppHandle, DaMisurare)>();
        let partito = std::thread::Builder::new()
            .name("rymembra-analisi-cloud".to_owned())
            .spawn(move || {
                let mut operaio = Operaio::nuovo(SCADENZA_NOME);
                while let Ok((app, chiave)) = orecchio.recv() {
                    if crate::spegnimento::in_uscita() {
                        return;
                    }
                    misura_la_copia(&app, &mut operaio, &chiave);
                }
            });
        if let Err(guasto) = partito {
            nota!("[analisi] il filo dei brani del cloud non parte: {guasto}");
        }
        posta
    });
    let _ = posta.send((app.clone(), chiave));
}

/// Le copie del cloud che l'ultima sessione ha lasciato da misurare
/// (`Cache::da_misurare`): la chiusura è arrivata con la richiesta ancora in
/// coda, o a metà misura, e l'avviso di completamento per loro non ripartirà.
/// Senza questa strada la cache le tratterrebbe per sempre.
///
/// Vanno al filo di [`misura_dal_cloud`] dopo [`ATTESA_AVVIO`], come il resto
/// dell'analisi: all'avvio la scansione sta ancora leggendo e la coda si sta
/// riaprendo, e queste copie non hanno fretta — sono già sul disco dei dati, e
/// la cache le tiene finché non sono misurate. Un filo solo, e solo se c'è
/// qualcosa da riproporre.
pub fn riproponi_dal_cloud(app: &AppHandle, chiavi: Vec<DaMisurare>) {
    if chiavi.is_empty() {
        return;
    }
    let mano = app.clone();
    let partito = std::thread::Builder::new()
        .name("rymembra-analisi-cloud-rimaste".to_owned())
        .spawn(move || {
            std::thread::sleep(ATTESA_AVVIO);
            if crate::spegnimento::in_uscita() {
                return;
            }
            for chiave in chiavi {
                misura_dal_cloud(&mano, chiave);
            }
        });
    if let Err(guasto) = partito {
        nota!("[analisi] il filo delle copie del cloud rimaste non parte: {guasto}");
    }
}

/// I brani di una copia — il file, o i brani d'immagine CD che lo usano —
/// misurati per impronta e volume, poi la copia lasciata andare.
fn misura_la_copia(app: &AppHandle, operaio: &mut Operaio, chiave: &DaMisurare) {
    // Senza libreria l'applicazione si sta chiudendo: la copia resta da
    // misurare, e la misura si rifà alla prossima sessione.
    let Some(stato) = app.try_state::<Stato>() else {
        return;
    };
    let percorso = crate::nuvola_musica::percorso_della_copia(app, chiave);
    let cloud_registrato = app
        .try_state::<crate::nuvola_musica::StatoMusicaNuvola>()
        .is_some();
    if copia_senza_brani(cloud_registrato, percorso.as_deref()) {
        // Nessun brano da misurare: la copia non ha più niente da aspettare.
        crate::nuvola_musica::copia_misurata(app, chiave);
        return;
    }
    let Some(percorso) = percorso else {
        return;
    };
    let brani: Vec<i64> = con_libreria(&stato, |libreria| {
        let mut domanda = libreria
            .connection
            .prepare_cached("SELECT id FROM tracks WHERE path = ?1 OR immagine = ?1")
            .map_err(|err| db_errore("brani di una copia del cloud", &err))?;
        let righe = domanda
            .query_map([&percorso], |riga| riga.get::<_, i64>(0))
            .map_err(|err| db_errore("brani di una copia del cloud", &err))?;
        righe
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| db_errore("brani di una copia del cloud", &err))
    })
    .unwrap_or_default();
    let files = crate::nuvola_musica::archivi_per_l_analisi(app);
    let mut misurati = 0_usize;
    for brano in brani {
        let schede = con_libreria(&stato, |libreria| {
            Ok((
                sonora::scheda_per_id(&libreria.connection, brano)?,
                volume::scheda_per_id(&libreria.connection, brano)?,
            ))
        });
        let Ok((impronta, intensita)) = schede else {
            continue;
        };
        if let Some(scheda) = impronta {
            let files = std::sync::Arc::clone(&files);
            let misurato = operaio
                .esegui(SCADENZA_BRANO, move || sonora::misura(&*files, &scheda))
                .unwrap_or(Misurato::NonRaggiungibile);
            if !matches!(misurato, Misurato::NonRaggiungibile) && scrivi(app, &[misurato]).is_ok() {
                misurati = misurati.saturating_add(1);
            }
        }
        if let Some(scheda) = intensita {
            let files = std::sync::Arc::clone(&files);
            let scadenza = scadenza_volume(&scheda);
            let misurato = operaio
                .esegui(scadenza, move || volume::misura(&*files, &scheda))
                .unwrap_or(volume::Misurato::NonRaggiungibile);
            if !matches!(misurato, volume::Misurato::NonRaggiungibile) {
                let scritto = con_libreria(&stato, |libreria| {
                    let tx = libreria
                        .connection
                        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                        .map_err(|err| db_errore("apertura della transazione del volume", &err))?;
                    let quanti = volume::registra(&tx, &[misurato], adesso_ms())?;
                    tx.commit()
                        .map_err(|err| db_errore("chiusura della transazione del volume", &err))?;
                    Ok(quanti)
                });
                if scritto.is_ok() {
                    misurati = misurati.saturating_add(1);
                }
            }
        }
    }
    if misurati > 0 {
        nota!("[analisi] copia del cloud misurata: {misurati} misure");
    }
    // Misurata o no (un brano già misurato, un file muto), la copia non ha
    // più niente da aspettare: torna una copia qualunque della cache.
    crate::nuvola_musica::copia_misurata(app, chiave);
}

/// La copia non ha brani da misurare, e va lasciata andare senza misura?
///
/// Sì quando lo stato del cloud c'è e la voce della copia non è più nello
/// specchio (`percorso` assente): è stata scollegata o cancellata sul servizio,
/// e trattenerla «da misurare» la terrebbe nella cache per sempre — la cache
/// la tiene fino al doppio del tetto, e intanto butta per prime le copie utili.
/// No quando manca lo stato del cloud — l'applicazione si chiude, o non l'ha
/// ancora registrato — perché allora il percorso manca per tutte, e la misura
/// va rifatta, non persa.
fn copia_senza_brani(cloud_registrato: bool, percorso: Option<&str>) -> bool {
    cloud_registrato && percorso.is_none()
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
    misura_lotto_con(schede, SCADENZA_BRANO, si_puo_continuare, |scheda| {
        sonora::misura(&rymembra_app::files::LocalFiles, &scheda)
    })
}

/// La musica non è ripartita e l'applicazione non sta uscendo: un lotto può
/// passare al brano dopo.
///
/// Una funzione e non le due letture dentro i fili del lotto perché le prove
/// passano la loro: [`SUONA`] è di tutto il processo, e la prova del battito la
/// alza mentre le altre girano.
fn si_puo_continuare() -> bool {
    !SUONA.load(Ordering::Relaxed) && !crate::spegnimento::in_uscita()
}

/// [`misura_lotto`] con la misura, la scadenza e la domanda «si continua?»
/// date: la cucitura che le prove usano per far cadere un brano a comando.
///
/// # Una radice che non risponde ferma il lotto
///
/// Al primo [`Misurato::NonRaggiungibile`], di qualunque filo, gli altri non
/// prendono più un brano nuovo: quel che hanno in volo finisce, il resto del
/// lotto non si tocca e resta candidato, come i lotti che vengono dopo. È la
/// regola che la scansione ha imparato nella 2.1.0 (`SCADENZE_DI_FILA` in
/// `rymembra_app::library`): senza, una share viva ma esausta — o appesa al
/// risveglio — si faceva interrogare un brano dopo l'altro, venticinque secondi
/// ciascuno, e ogni scadenza lasciava indietro un filo fermo dentro una `open`.
/// Il prezzo è piccolo: in un lotto misto, i brani locali dopo il primo «non
/// raggiungibile» slittano alla passata dopo, che la radice morta ferma
/// comunque.
fn misura_lotto_con(
    schede: &[SchedaSorgente],
    scadenza: Duration,
    continua: fn() -> bool,
    misura: fn(SchedaSorgente) -> Misurato,
) -> Vec<Misurato> {
    if schede.is_empty() {
        return Vec::new();
    }
    let per_filo = schede.len().div_ceil(FILI).max(1);
    let radice_morta = AtomicBool::new(false);
    std::thread::scope(|ambito| {
        let radice_morta = &radice_morta;
        let mani: Vec<_> = schede
            .chunks(per_filo)
            .map(|pezzo| {
                ambito.spawn(move || {
                    // Un operaio per filo, come in [`misura_intensita`]: vedi
                    // [`misura_uno`] per perché non basta `con_scadenza`.
                    let mut operaio = Operaio::nuovo(SCADENZA_NOME);
                    pezzo
                        .iter()
                        .take_while(|_| !radice_morta.load(Ordering::Relaxed) && continua())
                        .map(|scheda| {
                            let misurato = misura_uno(&mut operaio, scheda, scadenza, misura);
                            if matches!(misurato, Misurato::NonRaggiungibile) {
                                radice_morta.store(true, Ordering::Relaxed);
                            }
                            misurato
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        mani.into_iter()
            // Un filo che è morto non ha misurato niente, e i suoi brani
            // restano candidati: la passata dopo li ripesca. Non succede per un
            // brano storto — quello lo ferma [`misura_uno`], uno per uno — ma
            // resta la rete sotto un panico che nessuno ha previsto.
            .filter_map(|mano| mano.join().ok())
            .flatten()
            .collect()
    })
}

/// Un brano, con la scadenza addosso e il suo panico chiuso dentro.
///
/// # Il difetto che toglie
///
/// Prima la misura girava dentro `con_scadenza`, che risponde `None` sia quando
/// il tempo scade sia quando il filo del lavoro **muore**: un panico del
/// decodificatore chiudeva il canale senza risposta, e diventava
/// [`Misurato::NonRaggiungibile`] — cioè «la radice non risponde». Il lotto si
/// fermava, il brano non si segnava, e alla passata dopo era di nuovo il primo
/// candidato: di nuovo un panico, di nuovo il lotto fermo. I brani dietro di lui
/// non venivano misurati mai.
///
/// [`Operaio`] distingue le due cose, come già fa per l'intensità: un panico è
/// [`Mancato::Caduto`], il file arriva ed è il contenuto a essere storto, e si
/// segna [`sonora::ILLEGGIBILE`] — l'esito che si ritenta fra
/// [`sonora::RITENTA_MS`], lo stesso che la migrazione
/// `021_impronte_illeggibili` ha rimesso in coda quando gli Opus hanno smesso
/// di esserlo. Solo [`Mancato::Scaduto`] vale ancora «non raggiungibile».
fn misura_uno(
    operaio: &mut Operaio,
    scheda: &SchedaSorgente,
    scadenza: Duration,
    misura: fn(SchedaSorgente) -> Misurato,
) -> Misurato {
    let copia = scheda.clone();
    match operaio.esegui(scadenza, move || misura(copia)) {
        Ok(misurato) => misurato,
        Err(Mancato::Caduto { messaggio }) => {
            nota!(
                "[analisi] il decodificatore è caduto calcolando l'impronta brano={} causa={}",
                scheda.track_id,
                messaggio.as_deref().unwrap_or("—")
            );
            Misurato::Negata {
                track_id: scheda.track_id,
                esito: sonora::ILLEGGIBILE,
            }
        }
        Err(Mancato::Scaduto) => Misurato::NonRaggiungibile,
    }
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
        if stato.scansione_in_corso() {
            break Fine::Scansione;
        }
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
/// brani rimasti non si misurano, non si scrivono, e restano candidati. E si
/// guarda se un altro filo ha trovato la radice morta: vedi «Una radice che non
/// risponde ferma il lotto» su [`misura_lotto_con`], che qui vale identica.
fn misura_intensita(schede: &[SchedaSorgente]) -> Vec<volume::Misurato> {
    misura_intensita_con(schede, scadenza_volume, si_puo_continuare, |scheda| {
        volume::misura(&rymembra_app::files::LocalFiles, &scheda)
    })
}

/// [`misura_intensita`] con la scadenza, la domanda «si continua?» e la misura
/// date: la cucitura delle prove, come [`misura_lotto_con`].
fn misura_intensita_con(
    schede: &[SchedaSorgente],
    scadenza: fn(&SchedaSorgente) -> Duration,
    continua: fn() -> bool,
    misura: fn(SchedaSorgente) -> volume::Misurato,
) -> Vec<volume::Misurato> {
    if schede.is_empty() {
        return Vec::new();
    }
    let per_filo = schede.len().div_ceil(FILI_INTENSITA).max(1);
    let radice_morta = AtomicBool::new(false);
    std::thread::scope(|ambito| {
        let radice_morta = &radice_morta;
        let mani: Vec<_> = schede
            .chunks(per_filo)
            .map(|pezzo| {
                ambito.spawn(move || {
                    let mut operaio = Operaio::nuovo(INTENSITA_NOME);
                    pezzo
                        .iter()
                        .take_while(|_| !radice_morta.load(Ordering::Relaxed) && continua())
                        .map(|scheda| {
                            let misurato =
                                misura_volume(&mut operaio, scheda, scadenza(scheda), misura);
                            if matches!(misurato, volume::Misurato::NonRaggiungibile) {
                                radice_morta.store(true, Ordering::Relaxed);
                            }
                            misurato
                        })
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
fn misura_volume(
    operaio: &mut Operaio,
    scheda: &SchedaSorgente,
    scadenza: Duration,
    misura: fn(SchedaSorgente) -> volume::Misurato,
) -> volume::Misurato {
    let copia = scheda.clone();
    match operaio.esegui(scadenza, move || misura(copia)) {
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
/// Delega a [`rymembra_app::db::codice_da_sqlite`], che guarda il codice di SQLite
/// e distingue un database occupato — che si riprova — da una share che non
/// risponde. Prima rispondeva `db.queryFailed` a qualunque guasto, e chi la
/// leggeva si trovava consigliato di riavviare Rymembra per un file che era
/// occupato da mezzo secondo dalla scansione.
fn db_errore(cosa: &str, err: &rusqlite::Error) -> AppError {
    rymembra_app::db::codice_da_sqlite(cosa, err)
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

    /// Una scheda finta: le prove della misura non aprono nessun file.
    fn scheda(track_id: i64) -> SchedaSorgente {
        SchedaSorgente {
            track_id,
            collocazione: rymembra_app::playback::Collocazione::File(format!("{track_id}.flac")),
            durata_ms: 180_000,
            replaygain_db: None,
            picco: None,
        }
    }

    /// Un brano che fa cadere il decodificatore si segna illeggibile, e gli
    /// altri del lotto — anche quelli dopo di lui sullo stesso filo — si
    /// misurano lo stesso.
    ///
    /// Prima il panico cadeva sul filo della scadenza, che chiudeva il canale
    /// senza rispondere: valeva «non raggiungibile», cioè una radice morta. Il
    /// lotto si fermava, il brano non si segnava, e alla passata dopo era di
    /// nuovo il primo candidato — e di nuovo fermava tutto. I brani dietro di
    /// lui non venivano misurati mai.
    #[test]
    fn un_brano_che_fa_cadere_il_decodificatore_non_ferma_il_lotto() {
        // Quattro schede su tre fili sono due pezzi da due: [1, 2] e [3, 4].
        // Il primo del primo pezzo cade, e il secondo deve passare lo stesso.
        let schede: Vec<SchedaSorgente> = (1..=4).map(scheda).collect();
        let misurati = misura_lotto_con(
            &schede,
            Duration::from_secs(10),
            || true,
            |scheda| {
                assert_ne!(scheda.track_id, 1, "il decodificatore cade su questo file");
                Misurato::Fatta {
                    track_id: scheda.track_id,
                    vettore: vec![1.0],
                }
            },
        );

        assert!(
            !misurati
                .iter()
                .any(|m| matches!(m, Misurato::NonRaggiungibile)),
            "un file storto non è una radice morta: {misurati:?}"
        );
        assert!(
            misurati.iter().any(|m| matches!(
                m,
                Misurato::Negata { track_id: 1, esito } if *esito == sonora::ILLEGGIBILE
            )),
            "il brano che cade si segna illeggibile: {misurati:?}"
        );
        let fatti: Vec<i64> = misurati
            .iter()
            .filter_map(|m| match m {
                Misurato::Fatta { track_id, .. } => Some(*track_id),
                _ => None,
            })
            .collect();
        assert_eq!(fatti.len(), 3, "gli altri tre si misurano: {misurati:?}");
        assert!(fatti.contains(&2), "anche quello dopo, sullo stesso filo");
    }

    /// Una radice che non risponde ferma il lotto al primo brano che scade,
    /// invece di interrogarla file per file: è la regola che la scansione ha
    /// imparato nella 2.1.0 (`SCADENZE_DI_FILA`), e su una share appesa valeva
    /// ventidue brani da venticinque secondi per filo.
    ///
    /// Nove schede su tre fili, una misura che non torna mai entro la
    /// scadenza: ogni filo può avere in volo al più il suo primo brano quando
    /// il primo «non raggiungibile» arriva, quindi non più di [`FILI`].
    #[test]
    fn una_radice_che_non_risponde_ferma_il_lotto() {
        let schede: Vec<SchedaSorgente> = (1..=9).map(scheda).collect();
        let misurati = misura_lotto_con(
            &schede,
            Duration::from_millis(50),
            || true,
            |scheda| {
                std::thread::sleep(Duration::from_millis(300));
                Misurato::Fatta {
                    track_id: scheda.track_id,
                    vettore: vec![1.0],
                }
            },
        );
        let morti = misurati
            .iter()
            .filter(|m| matches!(m, Misurato::NonRaggiungibile))
            .count();
        assert!(
            (1..=FILI).contains(&morti),
            "la radice morta va interrogata al più una volta per filo: {morti} su {}",
            schede.len()
        );
        assert_eq!(
            misurati.len(),
            morti,
            "dopo il primo «non raggiungibile» non si misura più niente: {misurati:?}"
        );
    }

    /// Lo stesso per il volume: un brano che scade ferma il lotto, invece di
    /// lasciare ogni filo a interrogare la share brano dopo brano, ciascuno con
    /// la sua scadenza più un ventesimo della durata.
    #[test]
    fn una_radice_che_non_risponde_ferma_il_lotto_del_volume() {
        let schede: Vec<SchedaSorgente> = (1..=8).map(scheda).collect();
        let misurati = misura_intensita_con(
            &schede,
            |_| Duration::from_millis(50),
            || true,
            |scheda| {
                std::thread::sleep(Duration::from_millis(300));
                volume::Misurato::Negata {
                    track_id: scheda.track_id,
                    esito: volume::MUTO,
                }
            },
        );
        let morti = misurati
            .iter()
            .filter(|m| matches!(m, volume::Misurato::NonRaggiungibile))
            .count();
        assert!(
            (1..=FILI_INTENSITA).contains(&morti),
            "la radice morta va interrogata al più una volta per filo: {morti} su {}",
            schede.len()
        );
        assert_eq!(
            misurati.len(),
            morti,
            "dopo il primo «non raggiungibile» non si misura più niente"
        );
    }

    /// La musica che riparte ferma il lotto delle impronte fra un brano e
    /// l'altro, come già quello del volume: i brani rimasti restano candidati.
    /// Una copia della cache la cui voce non è più nello specchio — scollegata,
    /// cancellata sul servizio — non ha brani da misurare, e si lascia andare:
    /// trattenuta, restava «da misurare» per sempre. Non quando manca lo stato
    /// del cloud — l'applicazione si chiude, o non l'ha ancora registrato —
    /// perché lì la misura va rifatta, non persa.
    #[test]
    fn una_copia_senza_voce_si_lascia_andare_solo_a_cloud_registrato() {
        assert!(copia_senza_brani(true, None));
        assert!(!copia_senza_brani(false, None));
        assert!(!copia_senza_brani(true, Some("cloud://conto/id")));
        assert!(!copia_senza_brani(false, Some("cloud://conto/id")));
    }

    #[test]
    fn la_musica_che_riparte_ferma_il_lotto() {
        let schede: Vec<SchedaSorgente> = (1..=6).map(scheda).collect();
        let misurati = misura_lotto_con(
            &schede,
            Duration::from_secs(10),
            || false,
            |scheda| Misurato::Fatta {
                track_id: scheda.track_id,
                vettore: vec![1.0],
            },
        );
        assert!(misurati.is_empty(), "{misurati:?}");
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
            collocazione: rymembra_app::playback::Collocazione::File("x.flac".to_owned()),
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
