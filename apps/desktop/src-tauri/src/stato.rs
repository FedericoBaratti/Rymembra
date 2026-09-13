//! Quel che l'applicazione tiene aperto mentre gira.
//!
//! Una connessione al database e uno store di copertine, dietro un mutex, e
//! **accanto** al mutex le tre cose che non cambiano mai: dove stanno le
//! copertine, se qualcuno ha chiesto alla scansione di fermarsi, e se una
//! scansione è già in corso. Sono accanto e non dentro perché chi le chiede non
//! deve mettersi in coda dietro chi sta usando il database: il gestore del
//! protocollo `aether-cover://` vuole solo comporre un percorso, e
//! `annulla_scansione` vuole alzare un bit — nessuno dei due ha bisogno della
//! connessione, e farglieli chiedere insieme li ferma per tutta la durata di
//! quel che qualcun altro sta facendo.
//!
//! Il mutex non è un pool: SQLite in WAL regge un solo scrittore comunque, e le
//! letture di questa applicazione durano microsecondi. Un pool aggiungerebbe la
//! possibilità che due comandi vedano due transazioni diverse senza toglierne
//! nessuna reale. Quel che va tolto non è la serializzazione delle scritture: è
//! che qualcuno tenga il lucchetto mentre aspetta il **disco**, ed è il motivo
//! per cui la scansione ora lo prende e lo lascia lotto per lotto
//! ([`DepositoStato`]) invece di tenerlo dal primo file all'ultimo.
//!
//! # Dove stanno i dati
//!
//! Nella cartella dati dell'applicazione, chiesta a Tauri invece che composta a
//! mano: su Windows è `%APPDATA%\<identificatore>`, e comporla a mano
//! significherebbe sbagliarla su un profilo mobile o su un utente con la
//! cartella spostata.
//!
//! La cartella è **diversa** da quella dell'applicazione rilasciata
//! (`%APPDATA%\Aether`). Deve esserlo: finché questa non è finita, la vecchia
//! deve restare apribile, e l'importatore la legge in sola lettura.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};
use std::time::{Duration, Instant};

use crate::nota;
use aether_app::covers::CoverStore;
use aether_domain::errors::AppError;
use rusqlite::Connection;

/// Come si chiama il file del database dentro la cartella dati.
///
/// Una costante perché non è più aperto da un posto solo: l'arricchimento apre
/// una **seconda** connessione allo stesso file per la sua cache, e due nomi
/// scritti a mano che divergono produrrebbero un database vuoto accanto a quello
/// vero — senza nessun errore, perché SQLite crea il file che non trova.
pub const NOME_DATABASE: &str = "aether.db";

/// La libreria aperta.
pub struct Libreria {
    /// La connessione al database.
    pub connection: Connection,
    /// Lo store delle copertine.
    pub covers: CoverStore,
    /// Dove stanno i dati, per poterlo mostrare all'utente.
    pub data_dir: PathBuf,
    /// Quante migrazioni ha applicato l'apertura, per la riga di avvio.
    pub migrazioni: usize,
    /// FTS5 è disponibile.
    pub fts5: bool,
    /// Il giornale del database: `wal`, `truncate`, `memory`, o
    /// `<modo>-non-usabile`.
    ///
    /// Sta qui per un motivo solo: finire nella riga di avvio. È l'unica riga di
    /// diario che distingue una libreria veloce da una che si impunta a ogni
    /// scansione — `truncate` vuol dire che WAL non si è accesa, e che letture e
    /// scritture vanno in fila. Il vocabolario completo e il perché stanno su
    /// [`aether_app::db::Opened::giornale`].
    pub giornale: String,
}

/// Lo stato condiviso fra i comandi.
pub struct Stato {
    /// La libreria, o l'errore che ne ha impedito l'apertura.
    ///
    /// L'errore si conserva invece di far fallire l'avvio: una finestra che si
    /// apre e spiega perché il database non si è aperto è utilizzabile — ci si
    /// può leggere il percorso, cambiare cartella, ripristinare un backup. Un
    /// processo che esce prima di disegnare qualcosa non lo è.
    pub libreria: Mutex<Result<Libreria, AppError>>,

    /// Dove stanno le copertine, senza passare dal mutex.
    ///
    /// Una copia di `Libreria.covers`, presa una volta sola all'apertura.
    /// [`CoverStore`] è un `PathBuf` dietro un `Clone` gratuito, e quella
    /// cartella non cambia per tutta la vita del processo: tenerla **solo**
    /// dentro il mutex vorrebbe dire chiedere il lucchetto per calcolare un
    /// nome di file — che è quel che il gestore di `aether-cover://` faceva a
    /// ogni immagine della griglia, sul filo della finestra, mentre una
    /// scansione lo teneva.
    ///
    /// `None` quando l'apertura è fallita: allora l'errore è nel mutex, e
    /// [`Stato::copertine`] va a prenderlo lì.
    store_copertine: Option<CoverStore>,

    /// Qualcuno ha chiesto di fermare la scansione in corso.
    ///
    /// # Perché **non** sta dietro il mutex della libreria
    ///
    /// Perché in metà dei posti in cui la si legge il lucchetto non c'è. La
    /// scansione non lo tiene più per tutta la durata — lo prende e lo lascia a
    /// ogni lotto — ma la parte più lunga, la camminata sulle cartelle, gira
    /// **fuori** da qualunque lucchetto: una bandiera dietro il mutex lì non si
    /// potrebbe nemmeno guardare senza andarselo a prendere apposta, cioè senza
    /// riscoprire il difetto da capo. E chi la alza — `annulla_scansione`, e
    /// l'uscita in `main` — parla dal filo principale, che è precisamente il
    /// filo che non deve mettersi in coda dietro nessuno.
    ///
    /// Un `AtomicBool` accanto al mutex si legge e si scrive senza chiederlo. È
    /// esattamente il caso per cui gli atomici esistono: un bit condiviso fra
    /// due fili, senza niente da tenere coerente insieme a lui.
    scansione_da_fermare: AtomicBool,

    /// Una scansione è in corso.
    ///
    /// # Perché serve adesso e prima no
    ///
    /// Perché prima la guardia era il lucchetto stesso: la seconda scansione
    /// restava ferma su `con_libreria` finché la prima non aveva finito, e poi
    /// rifaceva tutto il lavoro daccapo su una libreria appena aggiornata —
    /// inutile, ma innocuo. Ora che il lucchetto si prende e si lascia lotto per
    /// lotto le due passate si intreccerebbero davvero, e `tracks.path` è
    /// UNIQUE: una delle due vedrebbe fallire l'inserimento di una riga che
    /// l'altra ha appena messo. Il nucleo non esclude più niente da sé — lo dice
    /// la documentazione di `Deposito` — e l'esclusione tocca a chi chiama.
    scansione_in_corso: AtomicBool,
}

impl Stato {
    /// Apre la libreria nella cartella dati data.
    pub fn apri(data_dir: PathBuf) -> Self {
        let libreria = apri_libreria(data_dir);
        Self {
            store_copertine: libreria.as_ref().ok().map(|l| l.covers.clone()),
            libreria: Mutex::new(libreria),
            scansione_da_fermare: AtomicBool::new(false),
            scansione_in_corso: AtomicBool::new(false),
        }
    }

    /// Lo store delle copertine, senza chiedere il lucchetto.
    ///
    /// # Errori
    ///
    /// L'errore con cui la libreria non si è aperta. È l'unico caso in cui
    /// questa funzione tocca il mutex, e lì aspettare non costa: senza libreria
    /// non c'è nessuna scansione che possa tenerlo.
    pub fn copertine(&self) -> Result<CoverStore, AppError> {
        if let Some(store) = &self.store_copertine {
            return Ok(store.clone());
        }
        let guardia = prendi_la_libreria(self);
        match guardia.as_ref() {
            Ok(libreria) => Ok(libreria.covers.clone()),
            Err(errore) => Err(errore.clone()),
        }
    }

    /// Il diritto di essere l'unica scansione in corso.
    ///
    /// `None` quando ce n'è già una: chi chiama risponde `library.scanBusy` se
    /// gliel'ha chiesto qualcuno, o annota e lascia perdere se è una passata
    /// automatica. Il turno si libera da sé quando la guardia cade — anche
    /// uscendo per un `?`, che è la ragione per cui [`Turno`] esiste.
    pub fn turno_di_scansione(&self) -> Option<Turno<'_>> {
        Turno::prendi(&self.scansione_in_corso)
    }

    /// Chiede alla scansione in corso di fermarsi.
    pub fn ferma_scansione(&self) {
        self.scansione_da_fermare.store(true, Ordering::Relaxed);
    }

    /// Azzera la richiesta. Si chiama all'inizio di ogni scansione: senza,
    /// un annullamento arrivato dopo la fine fermerebbe quella successiva.
    pub fn riprendi_scansioni(&self) {
        self.scansione_da_fermare.store(false, Ordering::Relaxed);
    }

    /// Se qualcuno ha chiesto di fermarsi.
    ///
    /// `Relaxed` basta: non c'è nessun altro dato che debba essere visibile
    /// insieme a questo bit, e il ritardo massimo è un file letto in più.
    pub fn scansione_fermata(&self) -> bool {
        self.scansione_da_fermare.load(Ordering::Relaxed)
    }
}

/// La libreria di questa applicazione, vista come un deposito da aprire e
/// richiudere.
///
/// È l'unico punto dell'applicazione che nomina `aether_app::library::Deposito`:
/// il nucleo chiede una connessione **per il tempo di una transazione**, questo
/// gliela dà prendendo il lucchetto e lo rilascia appena la chiusura ritorna.
/// Fra una chiamata e l'altra il lucchetto è libero, ed è quel che rende
/// rispondente la finestra mentre la scansione legge il disco.
///
/// Il prezzo è dichiarato nel contratto del tratto, e va ricordato qui: fra due
/// prestiti il database può essere cambiato sotto i piedi — un altro comando ha
/// cancellato un brano, l'arricchimento ne ha riscritto i tag. Il nucleo è
/// scritto per quello; l'unica cosa che non tollera è **un'altra scansione**, ed
/// è per questo che [`Stato::turno_di_scansione`] esiste.
pub struct DepositoStato<'a>(&'a Stato);

impl<'a> DepositoStato<'a> {
    /// Il deposito che apre e richiude questo stato.
    pub fn nuovo(stato: &'a Stato) -> Self {
        Self(stato)
    }
}

impl aether_app::library::Deposito for DepositoStato<'_> {
    fn con_connessione<T, F>(&mut self, azione: F) -> Result<T, AppError>
    where
        F: FnOnce(&mut Connection) -> Result<T, AppError>,
    {
        con_libreria(self.0, |libreria| azione(&mut libreria.connection))
    }
}

/// Il diritto di essere l'unica operazione in corso.
///
/// Una guardia e non due `store` a mano: fra il primo e il secondo ci sono dei
/// `?`, e un ritorno anticipato lascerebbe il bit alzato per sempre — cioè un
/// filo di sottofondo che non riparte più fino al riavvio, senza nessun errore
/// da nessuna parte.
///
/// Sta qui e non nei due moduli che la usano perché è la stessa identica regola:
/// due copie di una guardia di concorrenza sono due copie che divergono il
/// giorno in cui qualcuno ne aggiusta una sola, e il sintomo sarebbe una passata
/// che si sovrappone a se stessa in uno dei due posti soltanto.
///
/// # Due modi di entrarci, e quale scegliere
///
/// Il turno si **prende** o si **adotta**, e la differenza non è di gusto: è chi
/// alza la bandiera.
///
/// [`Turno::prendi`] la alza lui, in un colpo solo, e risponde `None` se era già
/// alzata. È il modo di sempre, ed è quello giusto quando chi decide di
/// cominciare e chi lavora sono la stessa funzione: un filo periodico che si
/// sveglia, guarda se può, e fa la sua passata.
///
/// [`Turno::adotta`] non la alza: la trova alzata e si impegna ad abbassarla.
/// Serve quando le due cose stanno in due posti diversi, e stanno in due posti
/// diversi per una ragione — nel caso che l'ha fatta nascere, il bit di
/// `StatoAggiornamenti::installazione` si alza nel **comando**, perché fra il
/// clic su «Aggiorna» e l'avvio del filo che scarica c'è abbastanza tempo perché
/// un secondo clic passi, mentre il lavoro da custodire è tutto sul filo. Con
/// `prendi` quel filo non potrebbe mai entrare (la bandiera è già alzata, e
/// giustamente); con due `store` a mano il bit resterebbe su per sempre se il
/// lavoro panicasse, e il tasto resterebbe spento fino al riavvio.
///
/// La regola per scegliere, in una riga: **`prendi` quando la decisione è qui,
/// `adotta` quando la decisione è già stata presa altrove e qui c'è solo la
/// promessa di rilasciare.** Chi usa `adotta` si prende un obbligo che il tipo
/// non può verificare — che la bandiera sia davvero alzata, e che nessun altro
/// la stia già custodendo — ed è per questo che è la seconda scelta e non la
/// prima.
pub struct Turno<'a>(&'a AtomicBool);

impl Turno<'_> {
    /// Prende il turno, se è libero.
    pub fn prendi(bandiera: &AtomicBool) -> Option<Turno<'_>> {
        bandiera
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Turno(bandiera))
    }

    /// Adotta un turno che qualcun altro ha già preso, per rilasciarlo qui.
    ///
    /// Non tocca la bandiera all'andata — chi chiama dichiara che è già alzata —
    /// e la abbassa nel `Drop`, anche srotolando per un panico. È tutto il
    /// guadagno: un lavoro che cade portandosi via il processo lascia comunque un
    /// bit coerente, e l'interfaccia non resta a dire che sta facendo una cosa
    /// che non sta più facendo.
    ///
    /// Quando si usa questo e non [`Turno::prendi`] sta scritto sul tipo.
    ///
    /// # Perché non torna `Option`
    ///
    /// Perché non c'è niente da decidere: la decisione è stata presa dal
    /// `compare_exchange` di chi ha alzato la bandiera, e ripetere qui un
    /// controllo darebbe un `None` che il chiamante potrebbe solo ignorare — cioè
    /// un ramo morto e un bit mai più abbassato, che è esattamente il guasto che
    /// questa funzione esiste per chiudere.
    pub fn adotta(bandiera: &AtomicBool) -> Turno<'_> {
        Turno(bandiera)
    }
}

impl Drop for Turno<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Aspetta che una raffica di sveglie finisca.
///
/// `Continue` quando per `raffica` non è arrivato più niente: la raffica è
/// esaurita e si può lavorare. `Break` quando il canale si è chiuso, cioè quando
/// l'applicazione sta uscendo.
///
/// # Perché i due esiti si distinguono
///
/// Perché `while orecchio.recv_timeout(raffica).is_ok() {}` li confonde: esce
/// allo stesso modo per un silenzio di due minuti e per un canale chiuso, e chi
/// chiama in entrambi i casi tira dritto e fa la sua passata. Alla chiusura
/// dell'applicazione questo vuol dire cominciare **allora** una passata intera:
/// per la nuvola un caricamento su Drive, per l'arricchimento minuti di
/// richieste di rete e tag riscritti sui file dell'utente — mentre il processo
/// sta uscendo e nessuno aspetta più il filo.
///
/// Sta qui e non nei due moduli che la usano per la stessa ragione di [`Turno`]:
/// due copie di una regola di concorrenza sono due copie che divergono il giorno
/// in cui qualcuno ne aggiusta una sola.
pub fn aspetta_la_raffica<T>(
    orecchio: &std::sync::mpsc::Receiver<T>,
    raffica: std::time::Duration,
) -> std::ops::ControlFlow<()> {
    use std::sync::mpsc::RecvTimeoutError;
    loop {
        match orecchio.recv_timeout(raffica) {
            // Un'altra sveglia: la raffica continua.
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => return std::ops::ControlFlow::Continue(()),
            Err(RecvTimeoutError::Disconnected) => return std::ops::ControlFlow::Break(()),
        }
    }
}

/// Avvia un filo che fa una passata ogni tanto, e prima se qualcuno lo sveglia.
///
/// La forma è quella dei tre fili che salvano, sincronizzano e arricchiscono: un
/// thread nominato, un'`attesa_avvio` prima della prima passata, e poi un
/// `recv_timeout` che fa da periodicità **e** da antirimbalzo — nessun timer,
/// nessun runtime asincrono. Una sveglia per cui `sporca` risponde di sì fa
/// aspettare che la `raffica` finisca prima di lavorare; ogni altra sveglia, e il
/// battito periodico che scade senza che sia arrivato niente, si servono subito.
/// La `passata` gira fuori da qualunque lucchetto: chi la scrive li prende e li
/// lascia dentro, e lo stato che deve sopravvivere fra un giro e l'altro lo tiene
/// catturato, dove nessun altro filo lo guarda.
///
/// Sta qui e non in uno dei tre moduli per la stessa ragione di [`Turno`] e di
/// [`aspetta_la_raffica`]: tre copie di un ciclo di concorrenza sono tre copie
/// che divergono il giorno in cui qualcuno ne aggiusta una sola.
///
/// # Perché due fili di sottofondo **non** passano di qui
///
/// `analisi.rs` decide quanto aspettare dall'esito della passata — mezzo minuto
/// se la musica suonava, venti se ha finito, e nessun dopo se si sta uscendo — e
/// la sua raffica si aspetta quando *è* arrivata una sveglia, non quando ne è
/// arrivata una di un tipo particolare. Sono due semantiche diverse dietro gli
/// stessi nomi, e farcele stare vorrebbe dire due parametri in più usati da un
/// chiamante solo.
///
/// `aggiornamenti.rs` non ha né payload di sveglia né raffica: qualunque cosa
/// arrivi sul canale vuol dire «controlla adesso», e passare di qui gli farebbe
/// dichiarare una durata di raffica che non aspetterebbe mai.
///
/// # Errori
///
/// L'errore di `Builder::spawn` torna a chi chiama invece di essere annotato qui:
/// la riga del diario dice *quale* filo non è partito, e quel nome — «il backup»,
/// «la sincronia» — lo sa il chiamante, non questa funzione.
pub fn avvia_filo_periodico<S: Send + 'static>(
    nome: &'static str,
    orecchio: std::sync::mpsc::Receiver<S>,
    attesa_avvio: Duration,
    raffica: Duration,
    intervallo: Duration,
    sporca: impl Fn(&S) -> bool + Send + 'static,
    mut passata: impl FnMut() + Send + 'static,
) -> std::io::Result<()> {
    use std::sync::mpsc::RecvTimeoutError;
    std::thread::Builder::new()
        .name(nome.to_owned())
        .spawn(move || {
            // `None` è il timeout: nessuna sveglia da esaminare, quindi nessuna
            // raffica da aspettare.
            let mut motivo = match orecchio.recv_timeout(attesa_avvio) {
                // Canale chiuso: l'applicazione sta uscendo.
                Err(RecvTimeoutError::Disconnected) => return,
                Ok(sveglia) => Some(sveglia),
                Err(RecvTimeoutError::Timeout) => None,
            };
            loop {
                // I due esiti dell'attesa si distinguono, ed è quel che impedisce
                // a una chiusura dell'applicazione di far cominciare **adesso**
                // una passata intera: vedi [`aspetta_la_raffica`].
                if motivo.as_ref().is_some_and(&sporca)
                    && aspetta_la_raffica(&orecchio, raffica).is_break()
                {
                    return;
                }
                passata();
                motivo = match orecchio.recv_timeout(intervallo) {
                    Err(RecvTimeoutError::Disconnected) => return,
                    Ok(sveglia) => Some(sveglia),
                    // Il timeout **è** il battito periodico: nessun timer.
                    Err(RecvTimeoutError::Timeout) => None,
                };
            }
        })
        // Il filo si stacca: nessuno lo aspetta, e l'unica cosa che lo ferma è
        // la chiusura del canale.
        .map(|_filo| ())
}

fn apri_libreria(data_dir: PathBuf) -> Result<Libreria, AppError> {
    std::fs::create_dir_all(&data_dir)
        .map_err(|err| aether_app::files::io_error(&data_dir.display().to_string(), &err))?;
    let covers = CoverStore::open(data_dir.join("copertine"))?;
    let aperto = aether_app::db::open(&data_dir.join(NOME_DATABASE))?;
    Ok(Libreria {
        connection: aperto.connection,
        covers,
        data_dir,
        migrazioni: aperto.applied,
        fts5: aperto.fts5,
        giornale: aperto.giornale,
    })
}

/// Stampa cosa è successo all'apertura.
///
/// Una riga sola, e c'è **sempre**: nel vecchio albero i messaggi di avvio non
/// arrivavano su disco, e quando qualcosa andava storto prima della finestra non
/// restava niente da leggere.
pub fn riga_di_avvio(stato: &Stato) {
    let guardia = stato
        .libreria
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match guardia.as_ref() {
        // `giornale=` è l'ultimo arrivato, e l'unico campo di questa riga che
        // spiega una **lentezza** invece di un guasto: `giornale=truncate` dice
        // che WAL non si è accesa — quasi sempre perché la cartella dati sta su
        // una condivisione di rete — e che da lì viene l'interfaccia che si
        // impunta durante una scansione. Senza questo campo quella domanda non
        // aveva risposta da nessuna parte nel diario.
        Ok(libreria) => nota!(
            "[avvio] libreria aperta dati={} migrazioni={} fts5={} giornale={}",
            libreria.data_dir.display(),
            libreria.migrazioni,
            libreria.fts5,
            libreria.giornale
        ),
        Err(errore) => nota!(
            "[avvio] libreria NON aperta codice={} causa={}",
            errore.code().kind().code(),
            errore.cause().unwrap_or("—")
        ),
    }
}

/// L'orologio, in millisecondi dall'epoca.
///
/// Qui e non in `aether-app`: il dominio non ha un orologio — è la sua regola —
/// e chi glielo porta è l'applicazione. Sta in questo modulo, e non copiato nei
/// due file che lo usano, perché il timestamp che scrive `preferito` e quello
/// che chiude un ascolto devono venire dalla stessa riga: due letture che si
/// somigliano sono due letture che possono divergere di un'unità di misura.
///
/// Un orologio indietro rispetto all'epoca — cosa che succede su una macchina
/// con la data sbagliata — vale zero invece di far cadere il comando: perdere
/// la precedenza in una fusione è meno grave che non poter mettere un
/// preferito.
pub fn adesso_ms() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
    )
    .unwrap_or(0)
}

/// Ogni quanto si riprova a prendere un lucchetto mentre lo si aspetta.
///
/// Cinque millisecondi. `Mutex` non offre un'attesa a termine, e l'unica cosa
/// che c'è è `try_lock`: aspettare vuol dire riprovare. Un passo più corto
/// scalderebbe un processore per niente; uno più lungo aggiungerebbe alla
/// rinuncia dell'uscita un ritardo confrontabile con la rinuncia stessa.
const PASSO_ATTESA: Duration = Duration::from_millis(5);

/// Quanto qualcuno può aspettare la libreria prima che il diario lo racconti.
///
/// Due secondi. Sotto ci stanno tutte le attese legittime — una transazione, una
/// query su una libreria grossa — e sopra c'è soltanto qualcuno che aspetta il
/// disco tenendo il lucchetto. È la soglia che rende diagnosticabile il difetto
/// che questo modulo esiste per non riavere: se un giorno tornerà, sarà scritto,
/// col nome del filo che lo subisce.
const ATTESA_DA_NOTARE: Duration = Duration::from_secs(2);

/// Prende un lucchetto, ma non più a lungo di `entro`.
///
/// `None` vuol dire **non preso**: chi chiama non ha eseguito niente e deve
/// rimettere a posto quel che aveva già mosso.
///
/// Il veleno si recupera per la stessa ragione di [`con_libreria`], e va
/// recuperato **anche qui**: un `try_lock` su un mutex avvelenato risponde
/// `Poisoned` subito, e trattarlo come «occupato» vorrebbe dire aspettare la
/// scadenza intera per poi rinunciare a un lucchetto che era libero.
fn lucchetto_entro<T>(mutex: &Mutex<T>, entro: Duration) -> Option<MutexGuard<'_, T>> {
    let inizio = Instant::now();
    loop {
        match mutex.try_lock() {
            Ok(guardia) => return Some(guardia),
            Err(TryLockError::Poisoned(veleno)) => return Some(veleno.into_inner()),
            Err(TryLockError::WouldBlock) => {
                if inizio.elapsed() >= entro {
                    return None;
                }
                std::thread::sleep(PASSO_ATTESA);
            }
        }
    }
}

/// Prende il lucchetto della libreria, e scrive nel diario se ha dovuto aspettare.
///
/// Aspetta comunque quanto serve: questa non è una via di rinuncia, è la stessa
/// attesa di prima con un testimone. La riga si scrive **prima** del `lock()`
/// bloccante e non dopo, perché il caso che interessa è quello in cui il
/// bloccante non ritorna mai: se la si scrivesse alla fine, l'unica sessione in
/// cui serviva davvero sarebbe l'unica in cui non c'è.
///
/// La via veloce è un `try_lock`, che nel caso normale — nessuno sta tenendo
/// niente — costa quanto costava il `lock()`.
fn prendi_la_libreria(stato: &Stato) -> MutexGuard<'_, Result<Libreria, AppError>> {
    if let Some(guardia) = lucchetto_entro(&stato.libreria, ATTESA_DA_NOTARE) {
        return guardia;
    }
    let filo = std::thread::current();
    let nome = filo.name().unwrap_or("senza nome");
    nota!(
        "[stato] lucchetto della libreria non preso in {} s: filo «{}» in attesa",
        ATTESA_DA_NOTARE.as_secs(),
        nome
    );
    let attesa = Instant::now();
    let guardia = stato
        .libreria
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    nota!(
        "[stato] lucchetto della libreria preso dopo altri {} ms: filo «{}»",
        attesa.elapsed().as_millis(),
        nome
    );
    guardia
}

/// Esegue `azione` sulla libreria aperta.
///
/// Aspetta il lucchetto **quanto serve**: quasi tutti i chiamanti sono comandi
/// che senza il database non hanno niente da restituire, e rinunciare
/// significherebbe inventare un errore per una condizione che passa da sola. Chi
/// aspetta più di [`ATTESA_DA_NOTARE`] lascia una riga nel diario, che è come si
/// scopre chi tiene il lucchetto troppo a lungo.
///
/// L'unico posto che non può aspettare è l'**uscita**, e infatti usa
/// [`con_libreria_entro`]: lì un'attesa lunga non è un comando lento, è un
/// processo che non si chiude.
///
/// Il mutex avvelenato — un panico dentro un altro comando mentre teneva il
/// lucchetto — si recupera invece di propagare: i dati dietro sono una
/// connessione SQLite, che un panico in codice Rust non lascia a metà di
/// niente, e rifiutare ogni comando successivo trasformerebbe un guasto isolato
/// in un'applicazione morta fino al riavvio.
pub fn con_libreria<T>(
    stato: &Stato,
    azione: impl FnOnce(&mut Libreria) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let mut guardia = prendi_la_libreria(stato);
    match guardia.as_mut() {
        Ok(libreria) => azione(libreria),
        Err(errore) => Err(errore.clone()),
    }
}

/// Come [`con_libreria`], ma rinuncia se il lucchetto non arriva entro `entro`.
///
/// `None` vuol dire **azione non eseguita**: non è un errore da mostrare, è un
/// «non adesso», e chi chiama deve rimettere a posto quel che aveva già mosso —
/// una bandiera «da salvare» abbassata va rialzata, o quel salvataggio non lo
/// tenterà più nessuno. `Some(Err(_))` invece è l'azione eseguita e fallita: le
/// due cose non si confondono di proposito.
///
/// Serve all'uscita, che è l'unico posto in cui aspettare è peggio che non fare:
/// un'applicazione che non si chiude è il guasto che l'utente risolve
/// terminando il processo, cioè lasciando il WAL sporco.
pub fn con_libreria_entro<T>(
    stato: &Stato,
    entro: Duration,
    azione: impl FnOnce(&mut Libreria) -> Result<T, AppError>,
) -> Option<Result<T, AppError>> {
    let mut guardia = lucchetto_entro(&stato.libreria, entro)?;
    Some(match guardia.as_mut() {
        Ok(libreria) => azione(libreria),
        Err(errore) => Err(errore.clone()),
    })
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn il_turno_si_libera_anche_uscendo_per_la_via_breve() {
        // La ragione per cui è una guardia e non due `store`: fra il primo e il
        // secondo ci sono dei `?`, e un turno rimasto alzato è un filo che non
        // riparte più fino al riavvio, senza nessun errore da nessuna parte.
        let bandiera = AtomicBool::new(false);
        fn esce_subito(bandiera: &AtomicBool) -> Option<()> {
            let _turno = Turno::prendi(bandiera)?;
            None
        }
        assert_eq!(esce_subito(&bandiera), None);
        assert!(
            !bandiera.load(Ordering::Acquire),
            "il turno è tornato libero"
        );
    }

    #[test]
    fn un_turno_adottato_si_libera_anche_se_il_lavoro_panica() {
        // È il caso per cui `adotta` è stata scritta: il bit l'alza un comando,
        // il lavoro sta su un altro filo, e se quel lavoro cade srotolando il bit
        // deve tornare giù comunque — altrimenti l'interfaccia resta a dire che
        // sta scaricando qualcosa che non sta scaricando, fino al riavvio.
        //
        // `panic = "unwind"` è dichiarato nel `Cargo.toml` di radice, con tre
        // ragioni di cui una è precisamente questa: sotto `abort` nessun `Drop`
        // verrebbe eseguito, e questa prova non avrebbe senso.
        let bandiera = AtomicBool::new(false);
        // Come fa il comando: alzata qui, custodita là.
        bandiera.store(true, Ordering::Release);

        let caduto = std::panic::catch_unwind(|| {
            let _turno = Turno::adotta(&bandiera);
            assert!(
                bandiera.load(Ordering::Acquire),
                "adottata vuol dire alzata"
            );
            panic!("il plugin è caduto a metà dello scaricamento");
        });

        assert!(caduto.is_err(), "il panico è arrivato fin qui");
        assert!(
            !bandiera.load(Ordering::Acquire),
            "il Drop ha abbassato la bandiera srotolando"
        );
        // E adesso `prendi` riesce di nuovo: è questo che rende il tasto
        // «Aggiorna» riutilizzabile senza riavviare.
        assert!(Turno::prendi(&bandiera).is_some());
    }

    #[test]
    fn adottare_non_alza_niente_da_se() {
        // La differenza fra i due costruttori, scritta come prova: `prendi`
        // decide e alza, `adotta` si fida e non tocca. Se un giorno `adotta`
        // cominciasse ad alzare la bandiera, il comando che l'ha già alzata non
        // se ne accorgerebbe — ma il suo `compare_exchange`, che è la difesa dal
        // doppio clic, diventerebbe una cerimonia.
        let bandiera = AtomicBool::new(false);
        {
            let _turno = Turno::adotta(&bandiera);
            assert!(
                !bandiera.load(Ordering::Acquire),
                "adotta non alza: la bandiera è come l'ha trovata"
            );
        }
        // E all'uscita la abbassa comunque, che da `false` vuol dire lasciarla
        // dov'era: la promessa è «a valle è bassa», non «è stata cambiata».
        assert!(!bandiera.load(Ordering::Acquire));
    }

    #[test]
    fn una_raffica_esaurita_e_un_canale_chiuso_non_sono_la_stessa_cosa() {
        // Il difetto che questa prova impedisce: `while … .is_ok() {}` esce allo
        // stesso modo per un silenzio e per un canale chiuso, e chi chiama fa la
        // sua passata in tutti e due i casi. Alla chiusura dell'applicazione
        // quella passata è un caricamento su Drive, o dei tag riscritti sui file
        // dell'utente, cominciati mentre il processo sta uscendo.
        let breve = std::time::Duration::from_millis(20);

        // Silenzio: la raffica è finita, si lavora.
        let (manda, orecchio) = std::sync::mpsc::channel::<u8>();
        assert!(
            aspetta_la_raffica(&orecchio, breve).is_continue(),
            "un silenzio vuol dire che la raffica è esaurita"
        );

        // Le sveglie si consumano tutte, e poi si continua lo stesso.
        for _ in 0..5 {
            manda.send(1).expect("sveglia");
        }
        assert!(aspetta_la_raffica(&orecchio, breve).is_continue());

        // Canale chiuso: non si lavora più.
        drop(manda);
        assert!(
            aspetta_la_raffica(&orecchio, breve).is_break(),
            "l'applicazione sta uscendo: il filo deve fermarsi, non fare una passata"
        );
    }

    #[test]
    fn due_operazioni_insieme_non_si_intrecciano() {
        let bandiera = AtomicBool::new(false);
        let primo = Turno::prendi(&bandiera);
        assert!(primo.is_some());
        assert!(
            Turno::prendi(&bandiera).is_none(),
            "una passata automatica e un'operazione a mano non devono sovrapporsi"
        );
        drop(primo);
        assert!(Turno::prendi(&bandiera).is_some(), "e dopo si può di nuovo");
    }

    #[test]
    fn una_scansione_alla_volta() {
        // Due che partono insieme: una entra, l'altra si sente dire di no. È il
        // caso vero — la scansione a mano e quella automatica della coda dei
        // download — e quel che impedisce è due passate che si contendono
        // l'inserimento della stessa riga in `tracks`, dove `path` è UNIQUE.
        let bandiera = std::sync::Arc::new(AtomicBool::new(false));
        let quanti = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let via = std::sync::Arc::new(std::sync::Barrier::new(8));
        let mut fili = Vec::new();
        for _ in 0..8 {
            let bandiera = std::sync::Arc::clone(&bandiera);
            let quanti = std::sync::Arc::clone(&quanti);
            let via = std::sync::Arc::clone(&via);
            fili.push(std::thread::spawn(move || {
                via.wait();
                if let Some(turno) = Turno::prendi(&bandiera) {
                    quanti.fetch_add(1, Ordering::AcqRel);
                    // Il turno si tiene fino alla fine: senza, gli altri
                    // potrebbero entrare a loro volta e la prova non direbbe
                    // niente.
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    drop(turno);
                }
            }));
        }
        for filo in fili {
            filo.join().expect("il filo è finito");
        }
        assert_eq!(
            quanti.load(Ordering::Acquire),
            1,
            "una sola scansione ha ottenuto il turno"
        );
        assert!(
            Turno::prendi(&bandiera).is_some(),
            "e quando ha finito il turno è di nuovo libero"
        );
    }

    #[test]
    fn un_lucchetto_a_scadenza_rinuncia_invece_di_aspettare() {
        // La ragione per cui esiste: all'uscita, aspettare un lucchetto che una
        // scansione tiene vuol dire un processo che non si chiude, e un utente
        // che lo termina lasciando il WAL sporco.
        let mutex = Mutex::new(0u8);
        let tenuto = mutex.lock().expect("il primo lucchetto");
        let inizio = Instant::now();
        let secondo = lucchetto_entro(&mutex, Duration::from_millis(60));
        let passato = inizio.elapsed();
        assert!(secondo.is_none(), "ha rinunciato");
        assert!(
            passato < Duration::from_millis(400),
            "ha rinunciato quando aveva detto: {passato:?}"
        );
        drop(tenuto);
    }

    #[test]
    fn un_lucchetto_a_scadenza_lo_prende_se_si_libera_in_tempo() {
        // L'altra metà: la scadenza non è una rinuncia preventiva. Nel caso
        // normale — nessuno tiene niente, o lo tiene per un istante — il
        // lucchetto arriva.
        let mutex = std::sync::Arc::new(Mutex::new(0u8));
        let altro = std::sync::Arc::clone(&mutex);
        // Il segnale parte **dopo** che il filo ha il lucchetto in mano: senza,
        // questa prova a volte lo prenderebbe per prima e non dimostrerebbe
        // niente.
        let (preso_lui, aspetta) = std::sync::mpsc::channel::<()>();
        let tiene = std::thread::spawn(move || {
            let mut dentro = altro.lock().expect("il lucchetto del filo");
            let _ = preso_lui.send(());
            std::thread::sleep(Duration::from_millis(40));
            *dentro = 7;
        });
        aspetta.recv().expect("il filo ha preso il lucchetto");
        let preso = lucchetto_entro(&mutex, Duration::from_secs(5));
        assert_eq!(
            preso.map(|g| *g),
            Some(7),
            "ha aspettato quel che serviva e ha visto la scrittura dell'altro"
        );
        tiene.join().expect("il filo è finito");
    }

    #[test]
    fn un_lucchetto_avvelenato_si_prende_lo_stesso() {
        // `try_lock` su un mutex avvelenato risponde `Poisoned` **subito**:
        // trattarlo come «occupato» vorrebbe dire consumare la scadenza intera
        // per poi rinunciare a un lucchetto che era libero. Da lì in poi ogni
        // comando che tocca la libreria fallirebbe fino al riavvio.
        let mutex = std::sync::Arc::new(Mutex::new(3u8));
        let altro = std::sync::Arc::clone(&mutex);
        let caduto = std::thread::spawn(move || {
            let _dentro = altro.lock().expect("il lucchetto del filo");
            panic!("un panico con il lucchetto in mano");
        });
        assert!(caduto.join().is_err(), "il filo è caduto tenendolo");
        assert!(mutex.is_poisoned(), "e il mutex è avvelenato");

        let inizio = Instant::now();
        let preso = lucchetto_entro(&mutex, Duration::from_secs(5));
        assert_eq!(preso.map(|g| *g), Some(3), "il veleno si recupera");
        assert!(
            inizio.elapsed() < Duration::from_millis(400),
            "e si recupera subito, non alla scadenza"
        );
    }
}
