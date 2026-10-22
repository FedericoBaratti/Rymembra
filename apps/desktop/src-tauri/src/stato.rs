//! Quel che l'applicazione tiene aperto mentre gira.
//!
//! Una connessione al database e uno store di copertine, dietro un mutex, e
//! **accanto** al mutex le tre cose che non cambiano mai: dove stanno le
//! copertine, se qualcuno ha chiesto alla scansione di fermarsi, e se una
//! scansione è già in corso. Sono accanto e non dentro perché chi le chiede non
//! deve mettersi in coda dietro chi sta usando il database: il gestore del
//! protocollo `rymembra-cover://` vuole solo comporre un percorso, e
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
//! La cartella è **diversa** da quella dell'applicazione rilasciata (la
//! cartella dati della versione 1.0). Deve esserlo: finché questa non è finita,
//! la vecchia deve restare apribile, e l'importatore la legge in sola lettura.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError, TryLockError};
use std::time::{Duration, Instant};

use crate::nota;
use rusqlite::Connection;
use rymembra_app::covers::CoverStore;
use rymembra_domain::errors::AppError;

/// Come si chiama il file del database dentro la cartella dati.
///
/// Una costante perché non è più aperto da un posto solo: l'arricchimento apre
/// una **seconda** connessione allo stesso file per la sua cache, e due nomi
/// scritti a mano che divergono produrrebbero un database vuoto accanto a quello
/// vero — senza nessun errore, perché SQLite crea il file che non trova.
pub const NOME_DATABASE: &str = "rymembra.db";

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
    /// [`rymembra_app::db::Opened::giornale`].
    pub giornale: String,
    /// La copia fatta prima delle migrazioni, o perché non c'è. `None` quando
    /// non c'era niente da migrare.
    ///
    /// Anche questa sta qui solo per la riga di avvio: se un aggiornamento va
    /// storto, la prima domanda è se c'è una copia da cui ripartire, e dove.
    pub istantanea: Option<rymembra_app::db::Istantanea>,
}

/// Lo stato condiviso fra i comandi.
pub struct Stato {
    /// La libreria, o l'errore che ne ha impedito l'apertura.
    ///
    /// L'errore si conserva invece di far fallire l'avvio: una finestra che si
    /// apre e spiega perché il database non si è aperto è utilizzabile — ci si
    /// può leggere il percorso, cambiare cartella, ripristinare un backup. Un
    /// processo che esce prima di disegnare qualcosa non lo è.
    ///
    /// È lo stesso errore anche quando la libreria non si è nemmeno provata ad
    /// aprire, perché il trasloco dei dati di prima ha detto di no: vedi
    /// [`Stato::rifiutato`].
    pub libreria: Mutex<Result<Libreria, AppError>>,

    /// Dove stanno le copertine, senza passare dal mutex.
    ///
    /// Una copia di `Libreria.covers`, presa una volta sola all'apertura.
    /// [`CoverStore`] è un `PathBuf` dietro un `Clone` gratuito, e quella
    /// cartella non cambia per tutta la vita del processo: tenerla **solo**
    /// dentro il mutex vorrebbe dire chiedere il lucchetto per calcolare un
    /// nome di file — che è quel che il gestore di `rymembra-cover://` faceva a
    /// ogni immagine della griglia, sul filo della finestra, mentre una
    /// scansione lo teneva.
    ///
    /// `None` quando l'apertura è fallita: allora l'errore è nel mutex, e
    /// [`Stato::copertine`] va a prenderlo lì.
    store_copertine: Option<CoverStore>,

    /// La cartella dati, senza chiedere il lucchetto.
    ///
    /// La stessa ragione di `store_copertine`: la chiede il protocollo delle
    /// risorse delle skin, che sta sul filo della finestra, e un percorso che
    /// non cambia mai non vale una coda dietro una scansione.
    cartella_dati: Option<PathBuf>,

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

    /// Chi svegliare quando una scansione finisce: il filo della manutenzione.
    ///
    /// # Perché qui e non nei due posti che scansionano
    ///
    /// Perché una scansione che toglie dei brani lascia le loro copertine senza
    /// nessuno, e la pulizia deve seguirla **qualunque** strada l'abbia fatta
    /// partire — il comando a mano, la passata automatica a fine coda dei
    /// download, e quelle che verranno. L'unico punto per cui passano tutte è il
    /// turno, e il turno sta qui: la sveglia parte quando la sua guardia cade
    /// (vedi [`TurnoDiScansione`]). Una riga in ognuno dei chiamanti sarebbe una
    /// riga che il prossimo chiamante dimentica.
    ///
    /// Dalla 3.0.2 parte solo se la scansione ha detto di aver cambiato
    /// qualcosa ([`TurnoDiScansione::segna_cambiamenti`]): con la sorveglianza
    /// di serie le scansioni sono decine al giorno, quasi tutte su un disco
    /// invariato, e una manutenzione svegliata da ognuna è lavoro per niente.
    ///
    /// Un `OnceLock` perché la manutenzione nasce dopo la libreria, e la
    /// registra una volta sola.
    fine_scansione: OnceLock<Sender<()>>,
}

impl Stato {
    /// Apre la libreria nella cartella dati data.
    pub fn apri(data_dir: PathBuf) -> Self {
        Self::da_libreria(apri_libreria(data_dir))
    }

    /// Lo stato di un avvio che non ha aperto niente, perché il trasloco dei
    /// dati della versione precedente ha detto di no.
    ///
    /// È lo stato che `apri` produce quando `apri_libreria` fallisce — la
    /// libreria è l'errore, e le due scorciatoie senza lucchetto
    /// (`store_copertine`, `cartella_dati`) sono vuote — con una differenza che
    /// è tutto il punto: qui `apri_libreria` non viene chiamata. Non crea la
    /// cartella dati, non apre il database, non tocca le copertine. Ogni
    /// comando che chiede la libreria si sente rispondere con `errore`, che è
    /// esattamente come l'applicazione tratta da sempre un database che non si
    /// è aperto: non c'è un secondo percorso d'errore da imparare.
    pub fn rifiutato(errore: AppError) -> Self {
        Self::da_libreria(Err(errore))
    }

    /// Lo stato attorno a una libreria già aperta, o all'errore che la sostituisce.
    fn da_libreria(libreria: Result<Libreria, AppError>) -> Self {
        Self {
            store_copertine: libreria.as_ref().ok().map(|l| l.covers.clone()),
            cartella_dati: libreria.as_ref().ok().map(|l| l.data_dir.clone()),
            libreria: Mutex::new(libreria),
            scansione_da_fermare: AtomicBool::new(false),
            scansione_in_corso: AtomicBool::new(false),
            fine_scansione: OnceLock::new(),
        }
    }

    /// Chi svegliare alla fine di ogni scansione. Le chiamate dopo la prima non
    /// fanno niente: il filo della manutenzione è uno.
    pub fn a_fine_scansione(&self, sveglia: Sender<()>) {
        let _ = self.fine_scansione.set(sveglia);
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

    /// La cartella dati, senza chiedere il lucchetto. `None` se la libreria
    /// non si è aperta.
    pub fn cartella_dati(&self) -> Option<&Path> {
        self.cartella_dati.as_deref()
    }

    /// Il diritto di essere l'unica scansione in corso.
    ///
    /// `None` quando ce n'è già una: chi chiama risponde `library.scanBusy` se
    /// gliel'ha chiesto qualcuno, o annota e lascia perdere se è una passata
    /// automatica. Il turno si libera da sé quando la guardia cade — anche
    /// uscendo per un `?`, che è la ragione per cui [`Turno`] esiste.
    ///
    /// Quando cade, la guardia sveglia anche la manutenzione, se la scansione
    /// ha segnato dei cambiamenti: vedi [`TurnoDiScansione`].
    pub fn turno_di_scansione(&self) -> Option<TurnoDiScansione<'_>> {
        Turno::prendi(&self.scansione_in_corso).map(|turno| TurnoDiScansione {
            _turno: turno,
            sveglia: self.fine_scansione.get(),
            cambiato: AtomicBool::new(false),
        })
    }

    /// Lo stesso turno, preso dalla manutenzione: esclude le scansioni come
    /// [`Stato::turno_di_scansione`], e **non** sveglia nessuno quando cade.
    ///
    /// Senza questa seconda porta, la pulizia delle copertine — che prende il
    /// turno per non incrociare una scansione — sveglierebbe sé stessa alla
    /// fine, e da lì una volta ogni dieci secondi per sempre.
    pub fn turno_di_manutenzione(&self) -> Option<Turno<'_>> {
        Turno::prendi(&self.scansione_in_corso)
    }

    /// Qualcuno tiene il turno delle scansioni, adesso.
    ///
    /// Solo una lettura, per chi deve **non** cominciare qualcosa durante una
    /// scansione senza però prenderne il turno: il sondaggio delle cartelle di
    /// rete di `crate::sorveglia`, che non scrive niente e che, prendendo il
    /// turno anche per un istante, farebbe rispondere «occupato» a una
    /// «Scansiona» premuta in quell'istante.
    pub fn scansione_in_corso(&self) -> bool {
        self.scansione_in_corso.load(Ordering::Acquire)
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
/// È l'unico punto dell'applicazione che nomina `rymembra_app::library::Deposito`:
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
///
/// # Le prese si contano
///
/// Una presa che tiene la libreria più di due secondi lo dice nel diario, come
/// ogni altra (vedi [`con_libreria`]), e qui dice anche **quale** presa della
/// scansione era: la prima per i fogli o per il piano, poi le righe doppie, un
/// lotto dopo l'altro, le rimozioni, gli album. Il nome della fase non arriva
/// fin qui — il tratto presta una connessione e non sa a che cosa serva — ma
/// il numero, accanto alla riga `[scansione]` con i tempi di ogni fase, basta a
/// dire chi ha fatto aspettare la finestra.
pub struct DepositoStato<'a> {
    /// Lo stato che apre e richiude.
    stato: &'a Stato,
    /// Quante volte ha prestato la connessione finora.
    prese: u32,
}

impl<'a> DepositoStato<'a> {
    /// Il deposito che apre e richiude questo stato.
    pub fn nuovo(stato: &'a Stato) -> Self {
        Self { stato, prese: 0 }
    }
}

impl rymembra_app::library::Deposito for DepositoStato<'_> {
    fn con_connessione<T, F>(&mut self, azione: F) -> Result<T, AppError>
    where
        F: FnOnce(&mut Connection) -> Result<T, AppError>,
    {
        self.prese = self.prese.saturating_add(1);
        con_libreria_per(self.stato, Some(self.prese), |libreria| {
            azione(&mut libreria.connection)
        })
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

/// Il turno di una scansione: un [`Turno`], più la sveglia della manutenzione.
///
/// Quando cade — a scansione finita, annullata o fallita, anche uscendo per un
/// `?` — sveglia il filo della manutenzione, se la scansione ha segnato di aver
/// cambiato qualcosa ([`TurnoDiScansione::segna_cambiamenti`]). La manutenzione
/// dopo la sua raffica guarda se sono rimaste copertine senza nessuno e passa
/// `PRAGMA optimize` su una libreria appena cambiata. Prima si sveglia, poi il
/// turno si libera: il `Drop` di questa struttura gira prima di quello dei suoi
/// campi, e la manutenzione aspetta comunque dieci secondi prima di chiederlo.
///
/// # Perché la sveglia va chiesta
///
/// Fino alla 3.0.1 partiva a ogni caduta. Con la sorveglianza delle cartelle
/// di serie le scansioni sono decine al giorno, e quasi tutte trovano il disco
/// com'era: svegliare la manutenzione per ognuna voleva dire contare le
/// copertine orfane e riottimizzare una libreria che non era cambiata. Chi
/// scansiona sa se ha scritto qualcosa — il rapporto lo dice — e lo dice qui;
/// chi non lo dice non sveglia nessuno.
pub struct TurnoDiScansione<'a> {
    /// Il turno vero, che si libera cadendo.
    _turno: Turno<'a>,
    /// Chi svegliare. `None` finché la manutenzione non si è registrata.
    sveglia: Option<&'a Sender<()>>,
    /// La scansione ha cambiato qualcosa: alla caduta si sveglia.
    ///
    /// Un atomico e non un `Cell` perché il turno può essere prestato a un
    /// altro filo per riferimento, e un `Cell` lo renderebbe impossibile senza
    /// che nessuno ne abbia bisogno per altro. Costa uguale.
    cambiato: AtomicBool,
}

impl TurnoDiScansione<'_> {
    /// La scansione ha cambiato qualcosa in libreria: alla fine si sveglia la
    /// manutenzione.
    ///
    /// La chiama chi tiene il turno quando il rapporto dice
    /// `inserted + updated + moved + removed > 0` — e anche quando la scansione
    /// fallisce, se può aver scritto prima di cadere: un lotto già commesso ha
    /// le sue copertine, e una rimozione già fatta le sue orfane. Chiamarla due
    /// volte vale una.
    pub fn segna_cambiamenti(&self) {
        self.cambiato.store(true, Ordering::Relaxed);
    }
}

impl Drop for TurnoDiScansione<'_> {
    fn drop(&mut self) {
        if !self.cambiato.load(Ordering::Relaxed) {
            return;
        }
        if let Some(sveglia) = self.sveglia {
            // Un canale chiuso vuol dire che il filo non c'è più, cioè che si
            // sta uscendo: non è un guasto della scansione.
            let _ = sveglia.send(());
        }
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

/// La salute di un filo di sottofondo, come la si legge da fuori.
///
/// Quel che serve a rispondere a «il backup gira?» senza aprire il diario:
/// quanti giri ha fatto, quante volte è caduto, quando e perché l'ultima.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaluteFilo {
    /// Il nome del filo, lo stesso che compare nel diario.
    pub nome: String,
    /// I giri finiti, caduti compresi.
    pub giri: u64,
    /// Quante volte un giro è caduto in panico.
    pub cadute: u64,
    /// Quante di fila, fino all'ultimo giro: zero vuol dire che l'ultimo è
    /// andato.
    pub cadute_di_fila: u32,
    /// Quando è caduto l'ultima volta, in millisecondi dall'epoca.
    pub ultima_caduta_ms: Option<i64>,
    /// Il messaggio dell'ultimo panico, accorciato. Il dove lo scrive il
    /// gancio dei panici nel diario.
    pub ultimo_panico: Option<String>,
}

/// Il registro della salute dei fili, uno per processo.
static SALUTE: OnceLock<Mutex<std::collections::BTreeMap<&'static str, SaluteFilo>>> =
    OnceLock::new();

/// Il registro, anche se un panico lo ha avvelenato: è proprio quando qualcuno
/// è caduto che serve leggerlo.
fn salute() -> MutexGuard<'static, std::collections::BTreeMap<&'static str, SaluteFilo>> {
    SALUTE
        .get_or_init(|| Mutex::new(std::collections::BTreeMap::new()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// La salute di tutti i fili sorvegliati che hanno fatto almeno un giro.
#[must_use]
pub fn salute_dei_fili() -> Vec<SaluteFilo> {
    salute().values().cloned().collect()
}

/// La salute dei fili di sottofondo, per la finestra.
///
/// Istantaneo: legge un registro in memoria, non tocca né disco né rete.
#[tauri::command]
pub fn fili_salute() -> crate::errore::Esito<Vec<SaluteFilo>> {
    Ok(salute_dei_fili())
}

/// Un giro di un filo di sottofondo, che se cade in panico non porta via il
/// filo.
///
/// # Il difetto che toglie
///
/// I fili di sottofondo — backup, sincronia, arricchimento, manutenzione,
/// aggiornamenti, scrobbling — sono cicli infiniti, e un panico in un giro
/// srotolava **fuori dal ciclo**: il filo finiva, e con lui il servizio, per il
/// resto della sessione e senza che nessuno lo dicesse. Il gancio scriveva il
/// panico nel diario; il backup intanto non salvava più niente, e la finestra
/// continuava a mostrare l'ora dell'ultimo salvataggio riuscito.
///
/// Qui il giro gira dentro `catch_unwind`: un panico diventa `None`, una riga
/// nel diario e una voce in [`salute_dei_fili`], e il ciclo decide quando
/// riprovare — vedi [`attesa_dopo_la_caduta`]. I panici restano vietati dai
/// lint in questo albero; quelli che arrivano vengono da sotto, da un parser o
/// da un plugin davanti a byte che non si aspettava.
///
/// # Perché `AssertUnwindSafe`
///
/// Perché quel che il giro tocca è di due specie, e nessuna delle due resta a
/// metà in un modo che conti: i lucchetti si riprendono con
/// `unwrap_or_else(PoisonError::into_inner)` dappertutto, e il database ha le
/// sue transazioni, che un panico lascia cadere senza commit.
pub fn sorvegliato<T>(nome: &'static str, giro: impl FnOnce() -> T) -> Option<T> {
    let esito = std::panic::catch_unwind(std::panic::AssertUnwindSafe(giro));
    let mut registro = salute();
    let voce = registro.entry(nome).or_insert_with(|| SaluteFilo {
        nome: nome.to_owned(),
        giri: 0,
        cadute: 0,
        cadute_di_fila: 0,
        ultima_caduta_ms: None,
        ultimo_panico: None,
    });
    voce.giri = voce.giri.saturating_add(1);
    match esito {
        Ok(valore) => {
            voce.cadute_di_fila = 0;
            Some(valore)
        }
        Err(carico) => {
            let messaggio = carico
                .downcast_ref::<&str>()
                .map(|testo| (*testo).to_owned())
                .or_else(|| carico.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panico senza messaggio".to_owned());
            let messaggio: String = messaggio.chars().take(300).collect();
            voce.cadute = voce.cadute.saturating_add(1);
            voce.cadute_di_fila = voce.cadute_di_fila.saturating_add(1);
            voce.ultima_caduta_ms = Some(adesso_ms());
            voce.ultimo_panico = Some(messaggio.clone());
            let di_fila = voce.cadute_di_fila;
            drop(registro);
            nota!("[fili] {nome} è caduto in panico ({di_fila} di fila) e riparte: {messaggio}");
            None
        }
    }
}

/// Quanto aspettare prima di rifare un giro dopo `cadute_di_fila` panici di
/// fila.
///
/// Cinque secondi la prima volta, poi il doppio, fino a dieci minuti. Un
/// panico che dipende da un file storto si ripresenterebbe identico a ogni
/// giro: ripartire subito vorrebbe dire un diario pieno della stessa riga e un
/// processore occupato a cadere. Un panico di passaggio, invece, merita il
/// secondo giro presto.
#[must_use]
pub fn attesa_dopo_la_caduta(cadute_di_fila: u32) -> Duration {
    let raddoppi = cadute_di_fila.saturating_sub(1).min(16);
    Duration::from_secs(5)
        .saturating_mul(1_u32 << raddoppi)
        .min(Duration::from_secs(10 * 60))
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
/// # Sorvegliato
///
/// Ogni passata gira dentro [`sorvegliato`]: una che cade in panico non porta
/// via il filo, e la successiva arriva dopo [`attesa_dopo_la_caduta`] invece
/// che dopo l'intervallo — o prima, se qualcuno sveglia il filo.
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
            let mut cadute_di_fila = 0_u32;
            loop {
                // I due esiti dell'attesa si distinguono, ed è quel che impedisce
                // a una chiusura dell'applicazione di far cominciare **adesso**
                // una passata intera: vedi [`aspetta_la_raffica`].
                if motivo.as_ref().is_some_and(&sporca)
                    && aspetta_la_raffica(&orecchio, raffica).is_break()
                {
                    return;
                }
                let attesa = if sorvegliato(nome, &mut passata).is_some() {
                    cadute_di_fila = 0;
                    intervallo
                } else {
                    cadute_di_fila = cadute_di_fila.saturating_add(1);
                    attesa_dopo_la_caduta(cadute_di_fila).min(intervallo)
                };
                motivo = match orecchio.recv_timeout(attesa) {
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
        .map_err(|err| rymembra_app::files::io_error(&data_dir.display().to_string(), &err))?;
    let covers = CoverStore::open(data_dir.join("copertine"))?;
    let aperto = rymembra_app::db::open(&data_dir.join(NOME_DATABASE))?;
    Ok(Libreria {
        connection: aperto.connection,
        covers,
        data_dir,
        migrazioni: aperto.applied,
        fts5: aperto.fts5,
        giornale: aperto.giornale,
        istantanea: aperto.istantanea,
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
        Ok(libreria) => {
            nota!(
                "[avvio] libreria aperta dati={} migrazioni={} fts5={} giornale={}",
                libreria.data_dir.display(),
                libreria.migrazioni,
                libreria.fts5,
                libreria.giornale
            );
            riga_dell_istantanea(libreria.istantanea.as_ref());
        }
        Err(errore) => nota!(
            "[avvio] libreria NON aperta codice={} causa={}",
            errore.code().kind().code(),
            errore.cause().unwrap_or("—")
        ),
    }
}

/// La riga di diario dell'istantanea di prima delle migrazioni, se c'è stata
/// una migrazione.
///
/// Una riga a parte e non un campo della riga di avvio: c'è solo quando un
/// aggiornamento ha toccato lo schema, e quando c'è porta un percorso intero —
/// quello da rimettere a mano se la migrazione è andata storta. La riga dice
/// anche quando la copia **non** c'è, e perché: è l'informazione che serve di
/// più proprio nel giorno in cui servirebbe la copia.
fn riga_dell_istantanea(istantanea: Option<&rymembra_app::db::Istantanea>) {
    use rymembra_app::db::Istantanea;
    match istantanea {
        None => {}
        Some(Istantanea::Fatta {
            percorso,
            byte,
            millisecondi,
        }) => nota!(
            "[avvio] istantanea prima delle migrazioni: {} ({} MiB, {} ms)",
            percorso.display(),
            byte >> 20,
            millisecondi
        ),
        Some(Istantanea::Presente { percorso }) => nota!(
            "[avvio] istantanea prima delle migrazioni già presente: {}",
            percorso.display()
        ),
        Some(Istantanea::Saltata { motivo }) => {
            nota!("[avvio] istantanea prima delle migrazioni SALTATA: {motivo}");
        }
    }
}

/// Quanto l'uscita aspetta la libreria per l'ultimo `PRAGMA optimize`.
///
/// Un quarto di secondo. L'ottimizzazione in sé costa meno di un millisecondo
/// quasi sempre; l'attesa è per il lucchetto, che all'uscita può tenerlo una
/// scansione a cui si è appena chiesto di fermarsi. Oltre, si rinuncia: le
/// statistiche restano quelle di sei ore fa, e l'`optimize=0x10002` della
/// prossima apertura rimedia. Un'uscita che si impunta per aspettarla no.
const ATTESA_OTTIMIZZAZIONE_USCITA: Duration = Duration::from_millis(250);

/// L'ultimo `PRAGMA optimize` della sessione, quello che la documentazione di
/// SQLite chiede prima di chiudere una connessione.
///
/// # Una volta sola
///
/// La sequenza di spegnimento in `main` gira per ognuno dei tre eventi che
/// dicono «si chiude», e di solito ne arrivano due o tre. La prima volta si
/// ottimizza, le altre no: una bandiera di processo, alzata **prima** di
/// provare, perché anche un tentativo che rinuncia al lucchetto non va
/// ripetuto — la seconda attesa costerebbe un altro quarto di secondo a
/// un'uscita che sta già aspettando.
pub fn ottimizza_uscendo(stato: &Stato) {
    static FATTA: AtomicBool = AtomicBool::new(false);
    if FATTA.swap(true, Ordering::AcqRel) {
        return;
    }
    match con_libreria_entro(stato, ATTESA_OTTIMIZZAZIONE_USCITA, |libreria| {
        rymembra_app::db::ottimizza(&libreria.connection)
    }) {
        Some(Ok(())) => {}
        Some(Err(err)) => nota!(
            "[uscita] ottimizzazione del database non riuscita codice={} causa={}",
            err.code().kind().code(),
            err.cause().unwrap_or("—")
        ),
        None => nota!(
            "[uscita] ottimizzazione del database saltata: lucchetto non preso in {} ms",
            ATTESA_OTTIMIZZAZIONE_USCITA.as_millis()
        ),
    }
}

/// L'orologio, in millisecondi dall'epoca.
///
/// Qui e non in `rymembra-app`: il dominio non ha un orologio — è la sua regola —
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
/// I posti che non possono aspettare sono due. L'**uscita**, che usa
/// [`con_libreria_entro`]: lì un'attesa lunga non è un comando lento, è un
/// processo che non si chiude. E il **filo della finestra**, dove girano i
/// comandi sincroni: lì questa funzione rinuncia da sé dopo qualche secondo,
/// vedi [`con_libreria_dalla_finestra`].
///
/// Il mutex avvelenato — un panico dentro un altro comando mentre teneva il
/// lucchetto — si recupera invece di propagare: i dati dietro sono una
/// connessione SQLite, che un panico in codice Rust non lascia a metà di
/// niente, e rifiutare ogni comando successivo trasformerebbe un guasto isolato
/// in un'applicazione morta fino al riavvio.
///
/// # Chi tiene, oltre a chi aspetta
///
/// La riga di chi aspetta dice chi ha subito l'attesa, non chi l'ha causata: il
/// diario di una sessione vera ha mostrato la finestra ferma sessanta e
/// settanta secondi senza un nome dall'altra parte. Per questo anche chi
/// **tiene** la libreria più di [`TENUTA_DA_NOTARE`] lo scrive, col nome del
/// filo — vedi [`racconta_la_tenuta`]. Sul percorso normale costa un `Instant`
/// e un confronto.
pub fn con_libreria<T>(
    stato: &Stato,
    azione: impl FnOnce(&mut Libreria) -> Result<T, AppError>,
) -> Result<T, AppError> {
    con_libreria_per(stato, None, azione)
}

/// [`con_libreria`], con il numero della presa quando a chiedere è una
/// scansione ([`DepositoStato`]): finisce nella riga di una tenuta lunga.
fn con_libreria_per<T>(
    stato: &Stato,
    presa: Option<u32>,
    azione: impl FnOnce(&mut Libreria) -> Result<T, AppError>,
) -> Result<T, AppError> {
    if sul_filo_della_finestra() {
        return con_libreria_dalla_finestra(stato, azione);
    }
    let mut guardia = prendi_la_libreria(stato);
    let preso = Instant::now();
    let esito = match guardia.as_mut() {
        Ok(libreria) => azione(libreria),
        Err(errore) => Err(errore.clone()),
    };
    // Prima si lascia, poi si racconta: la riga del diario è una scrittura su
    // disco, e non deve allungare proprio l'attesa che sta denunciando.
    drop(guardia);
    racconta_la_tenuta(preso.elapsed(), presa);
    esito
}

/// Oltre quanto chi tiene la libreria lo scrive nel diario.
///
/// Gli stessi due secondi di [`ATTESA_DA_NOTARE`], visti dall'altra parte: là
/// scrive chi aspetta, qui chi fa aspettare. Sotto ci stanno le transazioni
/// legittime — un lotto della scansione, la ricostruzione degli album a regime;
/// sopra c'è chi, tenendola, fa rinunciare la finestra dopo quattro secondi.
const TENUTA_DA_NOTARE: Duration = ATTESA_DA_NOTARE;

/// La riga del diario per un lucchetto tenuto `tenuta` dal filo `filo`, o
/// `None` se è sotto [`TENUTA_DA_NOTARE`].
///
/// `presa` è il numero della presa quando a tenerlo era una scansione: vedi
/// [`DepositoStato`]. Pura apposta, per provare soglia e testo senza un
/// diario.
fn riga_della_tenuta(tenuta: Duration, filo: &str, presa: Option<u32>) -> Option<String> {
    if tenuta <= TENUTA_DA_NOTARE {
        return None;
    }
    let ms = tenuta.as_millis();
    Some(match presa {
        None => format!("[stato] lucchetto tenuto {ms} ms dal filo «{filo}»"),
        Some(presa) => {
            format!("[stato] lucchetto tenuto {ms} ms dal filo «{filo}» (scansione, presa {presa})")
        }
    })
}

/// Scrive nel diario una tenuta lunga, se lo è.
///
/// Da chiamare **dopo** aver lasciato il lucchetto. Il nome del filo si chiede
/// solo sopra la soglia: sotto, tutto il costo è il confronto.
fn racconta_la_tenuta(tenuta: Duration, presa: Option<u32>) {
    if tenuta <= TENUTA_DA_NOTARE {
        return;
    }
    let filo = std::thread::current();
    if let Some(riga) = riga_della_tenuta(tenuta, filo.name().unwrap_or("senza nome"), presa) {
        nota!("{riga}");
    }
}

/// Il filo che disegna la finestra, segnato all'avvio.
///
/// Serve a [`con_libreria`] per una domanda sola: chi sta aspettando la
/// libreria è il filo della finestra? Un comando Tauri scritto senza `(async)`
/// gira **lì**, e lì un'attesa senza limite non è un comando lento: è la
/// finestra intera che non ridisegna, non risponde ai clic e che Windows dopo
/// cinque secondi dichiara «non risponde».
static FILO_DELLA_FINESTRA: std::sync::OnceLock<std::thread::ThreadId> = std::sync::OnceLock::new();

/// Segna il filo corrente come quello della finestra. Va chiamata dal `setup`
/// di Tauri, che gira su quel filo; le chiamate dopo la prima non fanno niente.
pub fn segna_filo_della_finestra() {
    let _ = FILO_DELLA_FINESTRA.set(std::thread::current().id());
}

/// Il filo corrente è quello della finestra.
fn sul_filo_della_finestra() -> bool {
    FILO_DELLA_FINESTRA
        .get()
        .is_some_and(|filo| *filo == std::thread::current().id())
}

/// Quanto il filo della finestra aspetta la libreria prima di rinunciare.
///
/// Quattro secondi. Il limite serve al caso in cui chi tiene il lucchetto sta
/// aspettando un disco che non risponde — una share staccata a metà di una
/// scrittura — e l'attesa, senza, sarebbe di quaranta secondi o per sempre.
/// Sotto i cinque secondi dopo i quali Windows marchia la finestra come «non
/// risponde»; sopra le attese legittime, che sono transazioni da millisecondi e
/// al massimo il lotto di una scansione.
const ATTESA_DELLA_FINESTRA: Duration = Duration::from_secs(4);

/// [`con_libreria`] quando chi chiede è il filo della finestra.
///
/// Rinuncia dopo [`ATTESA_DELLA_FINESTRA`] con `db.locked`, che è ritentabile e
/// ha già la sua frase: «il database è occupato da un'altra operazione». È
/// meglio di una finestra ferma, ed è la verità.
///
/// Il prezzo va detto: un comando sincrono che scrive — un cuoricino, un voto —
/// premuto mentre qualcuno tiene il lucchetto per più di quattro secondi adesso
/// risponde «occupato» invece di riuscire in ritardo. Succede solo quando chi
/// tiene il lucchetto aspetta un disco, perché la scansione lo prende e lo lascia
/// lotto per lotto; e un gesto da ripetere costa meno di una finestra che
/// Windows dichiara morta. Sugli altri fili — i comandi `(async)`, le passate —
/// l'attesa resta quella di sempre.
///
/// # Le attese sono due
///
/// Il mutex è la prima. La seconda è dentro SQLite: preso il lucchetto, una
/// scrittura può trovare il **file** occupato da un'altra connessione — la
/// cache dell'arricchimento, l'analisi, l'indice delle cartelle, che scrivono
/// sullo stesso database dalle loro — e lì il `busy_timeout` della connessione
/// aspetta altri cinque secondi. Quattro più cinque fa nove secondi sul filo
/// della finestra, cioè proprio il «non risponde» che i quattro dovevano
/// evitare. Per la durata dell'azione l'attesa di SQLite prende quel che resta
/// dei quattro secondi, con un minimo che lasci passare le scritture brevi
/// altrui, e dopo torna quella di sempre — anche se l'azione panica, perché la
/// connessione è la stessa per tutti e un'attesa corta dimenticata lì farebbe
/// fallire le passate di sottofondo.
///
/// Cosa non promette: SQLite fa ripartire l'attesa a ogni lucchetto che
/// chiede, quindi un'azione con due scritture separate può aspettare `resta`
/// due volte. I comandi sincroni che scrivono — un cuoricino, un voto — sono
/// una scrittura sola; uno che ne facesse di più dovrebbe chiedersi se è giusto
/// che stia sul filo della finestra.
fn con_libreria_dalla_finestra<T>(
    stato: &Stato,
    azione: impl FnOnce(&mut Libreria) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let inizio = Instant::now();
    let Some(mut guardia) = lucchetto_entro(&stato.libreria, ATTESA_DELLA_FINESTRA) else {
        nota!(
            "[stato] il filo della finestra rinuncia alla libreria dopo {} s",
            ATTESA_DELLA_FINESTRA.as_secs()
        );
        return Err(
            AppError::new(rymembra_domain::errors::ErrorCode::DbLocked).with_cause(format!(
                "lucchetto della libreria non disponibile entro {} s sul filo della finestra",
                ATTESA_DELLA_FINESTRA.as_secs()
            )),
        );
    };
    let preso = Instant::now();
    let libreria = match guardia.as_mut() {
        Ok(libreria) => libreria,
        Err(errore) => return Err(errore.clone()),
    };
    let resta = ATTESA_DELLA_FINESTRA
        .saturating_sub(inizio.elapsed())
        .max(ATTESA_MINIMA_DEL_FILE);
    let _ = libreria.connection.busy_timeout(resta);
    let esito = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| azione(&mut *libreria)));
    let _ = libreria
        .connection
        .busy_timeout(Duration::from_millis(u64::from(
            rymembra_app::db::BUSY_TIMEOUT_MS,
        )));
    // Anche il filo della finestra può tenerla a lungo — è il caso peggiore,
    // perché mentre la tiene la finestra non ridisegna — e lo dice come gli
    // altri, a lucchetto lasciato.
    drop(guardia);
    racconta_la_tenuta(preso.elapsed(), None);
    match esito {
        Ok(esito) => esito,
        Err(panico) => std::panic::resume_unwind(panico),
    }
}

/// L'attesa minima che il filo della finestra concede a SQLite quando il file è
/// occupato da un'altra connessione, anche se il mutex si è già preso quasi
/// tutto il tempo: le scritture delle passate sono transazioni da pochi
/// millisecondi, e un quarto di secondo le lascia finire invece di rispondere
/// «occupato» a un cuoricino per un soffio.
const ATTESA_MINIMA_DEL_FILE: Duration = Duration::from_millis(250);

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
    fn la_fine_di_una_scansione_che_ha_cambiato_sveglia_la_manutenzione_e_la_pulizia_no() {
        // La scansione sveglia la manutenzione quando il suo turno cade — per
        // qualunque strada sia partita — se ha segnato di aver cambiato
        // qualcosa. La pulizia, che prende lo stesso turno dall'altra porta,
        // no: altrimenti si sveglierebbe da sola a ogni giro, per sempre.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let stato = Stato::apri(dir.path().to_path_buf());
        let (sveglia, orecchio) = std::sync::mpsc::channel();
        stato.a_fine_scansione(sveglia);

        {
            let _turno = stato.turno_di_manutenzione().expect("turno libero");
            assert!(
                stato.turno_di_scansione().is_none(),
                "la pulizia esclude le scansioni"
            );
        }
        assert!(
            orecchio.try_recv().is_err(),
            "la pulizia non sveglia nessuno"
        );

        {
            let turno = stato.turno_di_scansione().expect("turno libero");
            assert!(
                stato.turno_di_manutenzione().is_none(),
                "e una scansione esclude la pulizia"
            );
            turno.segna_cambiamenti();
            turno.segna_cambiamenti();
            assert!(orecchio.try_recv().is_err(), "non prima della fine");
        }
        assert!(orecchio.try_recv().is_ok(), "la scansione finita sveglia");
        assert!(
            orecchio.try_recv().is_err(),
            "una volta sola, anche segnata due volte"
        );
        assert!(
            stato.turno_di_scansione().is_some(),
            "e il turno è di nuovo libero"
        );
    }

    #[test]
    fn una_scansione_che_non_ha_cambiato_niente_non_sveglia_nessuno() {
        // Il caso di quasi tutte le passate della sorveglianza: il disco è
        // com'era, e la manutenzione non ha niente da guardare.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let stato = Stato::apri(dir.path().to_path_buf());
        let (sveglia, orecchio) = std::sync::mpsc::channel();
        stato.a_fine_scansione(sveglia);

        drop(stato.turno_di_scansione().expect("turno libero"));
        assert!(
            orecchio.try_recv().is_err(),
            "senza cambiamenti la caduta del turno tace"
        );
        assert!(stato.turno_di_scansione().is_some(), "e il turno è libero");
    }

    #[test]
    fn una_tenuta_lunga_si_racconta_col_nome_del_filo() {
        // La riga che mancava al diario dei sessanta secondi di finestra
        // ferma: chi teneva la libreria, e per quanto.
        let sotto = TENUTA_DA_NOTARE;
        assert_eq!(
            riga_della_tenuta(sotto, "rymembra-analisi", None),
            None,
            "alla soglia non si scrive niente"
        );
        assert_eq!(
            riga_della_tenuta(Duration::from_millis(150), "x", Some(3)),
            None
        );
        assert_eq!(
            riga_della_tenuta(Duration::from_millis(2_001), "rymembra-analisi", None).as_deref(),
            Some("[stato] lucchetto tenuto 2001 ms dal filo «rymembra-analisi»")
        );
        assert_eq!(
            riga_della_tenuta(
                Duration::from_millis(62_480),
                "tokio-runtime-worker",
                Some(7)
            )
            .as_deref(),
            Some(
                "[stato] lucchetto tenuto 62480 ms dal filo «tokio-runtime-worker» \
                 (scansione, presa 7)"
            )
        );
    }

    #[test]
    fn il_deposito_conta_le_sue_prese() {
        // Il numero che la riga di una tenuta lunga porta: una per ogni
        // prestito, a partire da uno.
        use rymembra_app::library::Deposito as _;
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let stato = Stato::apri(dir.path().to_path_buf());
        let mut deposito = DepositoStato::nuovo(&stato);
        for attesa in 1..=3_u32 {
            deposito
                .con_connessione(|_| Ok(()))
                .expect("la libreria è aperta");
            assert_eq!(deposito.prese, attesa);
        }
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

    fn rifiuto_di_prova() -> AppError {
        AppError::new(rymembra_domain::errors::ErrorCode::InternalUnexpected {
            detail: Some("trasloco rifiutato".to_owned()),
        })
        .with_cause("prova")
    }

    #[test]
    fn uno_stato_rifiutato_risponde_come_una_libreria_che_non_si_e_aperta() {
        // Il rifiuto del trasloco non ha un percorso d'errore suo: è lo stato
        // che l'applicazione conosce già, quello di `apri_libreria` fallita.
        // Ogni comando che chiede la libreria deve sentirsi dare l'errore, e
        // nessuna scorciatoia senza lucchetto deve fingere che qualcosa sia
        // aperto.
        let stato = Stato::rifiutato(rifiuto_di_prova());

        let errore = con_libreria(&stato, |_| Ok(())).expect_err("la libreria non c'è");
        assert_eq!(errore.code().kind().code(), "internal.unexpected");
        assert_eq!(errore.cause(), Some("prova"));

        assert!(stato.cartella_dati().is_none());
        let copertine = stato.copertine().expect_err("niente copertine");
        assert_eq!(copertine.code().kind().code(), "internal.unexpected");

        // Gli stessi accessi dell'uscita, che non aspettano niente: nessuna
        // azione eseguita, ma nemmeno un'attesa o un panico.
        let esito = con_libreria_entro(&stato, Duration::from_millis(50), |_| Ok(()));
        assert!(matches!(esito, Some(Err(_))));
        // E la riga di avvio la racconta senza cadere.
        riga_di_avvio(&stato);
    }
}

#[cfg(test)]
mod prove_dei_fili {
    use super::*;

    #[test]
    fn un_giro_che_cade_non_porta_via_il_filo_e_si_vede() {
        let nome = "prova-sorvegliato";
        assert_eq!(sorvegliato(nome, || 3), Some(3));
        let caduto: Option<()> = sorvegliato(nome, || {
            std::panic::resume_unwind(Box::new("un parser davanti a byte storti"))
        });
        assert_eq!(
            caduto, None,
            "il panico diventa un giro mancato, non un filo morto"
        );
        let voce = salute_dei_fili()
            .into_iter()
            .find(|voce| voce.nome == nome)
            .expect("il filo è nel registro");
        assert_eq!(voce.giri, 2);
        assert_eq!(voce.cadute, 1);
        assert_eq!(voce.cadute_di_fila, 1);
        assert_eq!(
            voce.ultimo_panico.as_deref(),
            Some("un parser davanti a byte storti")
        );
        // Un giro buono azzera le cadute di fila, non la storia.
        assert_eq!(sorvegliato(nome, || ()), Some(()));
        let voce = salute_dei_fili()
            .into_iter()
            .find(|voce| voce.nome == nome)
            .expect("il filo è nel registro");
        assert_eq!((voce.cadute, voce.cadute_di_fila), (1, 0));
    }

    #[test]
    fn dopo_una_caduta_si_aspetta_sempre_di_piu_ma_non_troppo() {
        assert_eq!(attesa_dopo_la_caduta(1), Duration::from_secs(5));
        assert_eq!(attesa_dopo_la_caduta(2), Duration::from_secs(10));
        assert_eq!(attesa_dopo_la_caduta(3), Duration::from_secs(20));
        assert_eq!(attesa_dopo_la_caduta(40), Duration::from_secs(600));
    }
}
