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
//! L'eccezione sono le letture che non possono aspettare e non hanno bisogno
//! di quel che c'è dietro il mutex: le domande dei dispositivi collegati via
//! internet. Quelle vanno su una connessione di sola lettura loro, quando il
//! giornale è WAL: vedi [`leggi_a_parte`].
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
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock, PoisonError, TryLockError};
use std::time::{Duration, Instant};

use crate::nota;
use rusqlite::{Connection, OpenFlags};
use rymembra_app::covers::CoverStore;
use rymembra_domain::errors::{AppError, ErrorCode};

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
    /// Vuoto finché la libreria non è aperta, e per sempre se l'apertura è
    /// fallita: allora l'errore è nel mutex, e [`Stato::copertine`] va a
    /// prenderlo lì. Un `OnceLock` perché la libreria si apre su un filo suo,
    /// dopo che lo stato è già fra quelli gestiti: si scrive una volta, prima
    /// che la libreria entri nel mutex, e da lì non cambia.
    store_copertine: OnceLock<CoverStore>,

    /// La cartella dati, senza chiedere il lucchetto.
    ///
    /// La stessa ragione di `store_copertine`: la chiede il protocollo delle
    /// risorse delle skin, che sta sul filo della finestra, e un percorso che
    /// non cambia mai non vale una coda dietro una scansione. Vuota finché la
    /// libreria non è aperta, come `store_copertine`.
    cartella_dati: OnceLock<PathBuf>,

    /// La cartella in cui la libreria sta, o doveva stare: c'è anche quando
    /// l'apertura è fallita.
    ///
    /// Non è un doppione di `cartella_dati`, che vale «la libreria è aperta» e
    /// come tale la leggono [`leggi_a_parte`] e la scelta della cache. Questa
    /// serve a chi deve aiutare **proprio quando** la libreria non c'è: il gesto
    /// che apre la cartella dei dati dalla fascia d'errore, e quello che rimette
    /// la copia di prima dell'aggiornamento. `None` solo dopo un rifiuto del
    /// trasloco ([`Stato::rifiutato`]), dove la cartella nuova non si tocca.
    cartella_scelta: Option<PathBuf>,

    /// Il giornale della libreria è WAL, com'era all'apertura.
    ///
    /// Lo guarda [`leggi_a_parte`], e sta fuori dal mutex per la stessa ragione
    /// di `cartella_dati`: chiedere il lucchetto per sapere se si può fare a
    /// meno di chiederlo sarebbe il difetto da capo. Il giornale non cambia
    /// dopo l'apertura; prima dell'apertura non c'è.
    in_wal: OnceLock<bool>,

    /// A che punto è l'apertura della libreria, e chi aspetta che finisca.
    ///
    /// # Perché la libreria si apre dopo
    ///
    /// Si apriva nel `setup`, sul filo della finestra, prima che una finestra
    /// esistesse: una migrazione lunga — la copia di prima, gli schemi nuovi
    /// su un database grande — era un processo invisibile, e un secondo doppio
    /// clic finiva all'istanza unica, ferma sullo stesso filo (R06b-5). Adesso
    /// lo stato nasce [`Stato::in_apertura`], si registra, e la libreria si
    /// apre su un filo suo (`lib.rs`, `apri_e_avvia`).
    ///
    /// # Un modo solo di dire «non ancora»
    ///
    /// Finché l'apertura non è finita il mutex tiene `library.opening`, e chi
    /// chiede la libreria da un filo di sottofondo **aspetta** che finisca
    /// (`Stato::aspetta_l_apertura`): per lui niente cambia rispetto a quando
    /// il `setup` la apriva prima di tutto. Chi non può aspettare — il filo
    /// della finestra, chi ha una scadenza — riceve `library.opening`, che è
    /// ritentabile. La finestra, dal canto suo, monta l'applicazione solo ad
    /// apertura finita (`AttesaDellaLibreria.tsx`): i comandi non devono imparare niente.
    apertura: Apertura,

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
    /// Apre la libreria nella cartella dati data, adesso, su questo filo.
    ///
    /// L'apertura in un colpo solo: [`Stato::in_apertura`], la libreria, il
    /// cancello aperto. Per le prove, che vogliono una libreria pronta; l'avvio
    /// passa da `lib.rs`, `apri_e_avvia`.
    #[cfg(test)]
    pub fn apri(data_dir: PathBuf) -> Self {
        let stato = Self::in_apertura(data_dir.clone());
        stato.completa(apri_libreria(data_dir));
        stato.apri_il_cancello();
        stato
    }

    /// Lo stato di una libreria che si sta aprendo in `data_dir`: il mutex
    /// tiene `library.opening`, le scorciatoie senza lucchetto sono vuote, e
    /// chi chiede la libreria da un filo di sottofondo aspetta. La aprono
    /// [`apri_libreria`], [`Stato::completa`] e [`Stato::apri_il_cancello`].
    pub fn in_apertura(data_dir: PathBuf) -> Self {
        Self::da_libreria(Err(in_apertura()), Some(data_dir), FaseLibreria::Apertura)
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
        Self::da_libreria(Err(errore), None, FaseLibreria::Guasto)
    }

    /// Lo stato attorno all'errore che sostituisce la libreria — quello di
    /// un'apertura in corso, o di un rifiuto —, alla cartella in cui la si
    /// cerca, e alla fase da cui si parte.
    fn da_libreria(
        libreria: Result<Libreria, AppError>,
        cartella: Option<PathBuf>,
        fase: FaseLibreria,
    ) -> Self {
        let stato = Self {
            store_copertine: OnceLock::new(),
            cartella_dati: OnceLock::new(),
            cartella_scelta: cartella,
            in_wal: OnceLock::new(),
            apertura: Apertura {
                fase: Mutex::new(fase),
                finita: Condvar::new(),
            },
            libreria: Mutex::new(Err(in_apertura())),
            scansione_da_fermare: AtomicBool::new(false),
            scansione_in_corso: AtomicBool::new(false),
            fine_scansione: OnceLock::new(),
        };
        stato.completa(libreria);
        stato
    }

    /// Mette al suo posto la libreria appena aperta, o l'errore con cui non si
    /// è aperta. Il cancello resta chiuso: lo apre [`Stato::apri_il_cancello`],
    /// quando anche il resto dell'avvio è al suo posto.
    ///
    /// Le scorciatoie senza lucchetto si scrivono **prima** che la libreria
    /// entri nel mutex: chi le legge — il protocollo delle copertine, la
    /// lettura a parte — non deve mai vedere una libreria aperta senza la sua
    /// cartella.
    pub fn completa(&self, libreria: Result<Libreria, AppError>) {
        if let Ok(aperta) = &libreria {
            let _ = self.store_copertine.set(aperta.covers.clone());
            let _ = self.cartella_dati.set(aperta.data_dir.clone());
            let _ = self.in_wal.set(aperta.giornale == "wal");
        }
        *self.libreria.lock().unwrap_or_else(PoisonError::into_inner) = libreria;
    }

    /// Dice che la libreria si sta aggiornando dalla versione `da`: la finestra
    /// scrive «Aggiorno la libreria…» invece di «Apro la libreria…».
    pub fn in_aggiornamento(&self, da: u32) {
        let mut fase = self
            .apertura
            .fase
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !fase.finita() {
            *fase = FaseLibreria::Aggiornamento { da };
        }
    }

    /// L'avvio è finito: si apre il cancello, e chi aspettava la libreria la
    /// trova — aperta, o con l'errore con cui non si è aperta. Restituisce la
    /// fase a cui si è arrivati.
    pub fn apri_il_cancello(&self) -> FaseLibreria {
        let aperta = self
            .libreria
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_ok();
        let arrivata = if aperta {
            FaseLibreria::Pronta
        } else {
            FaseLibreria::Guasto
        };
        *self
            .apertura
            .fase
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = arrivata;
        self.apertura.finita.notify_all();
        arrivata
    }

    /// A che punto è l'apertura della libreria.
    pub fn fase(&self) -> FaseLibreria {
        *self
            .apertura
            .fase
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Aspetta che l'apertura della libreria finisca, ma non oltre `entro`
    /// (`None`: senza scadenza) e non mentre l'applicazione esce.
    ///
    /// L'uscita conta: chi chiude Rymembra durante una migrazione lunga non
    /// deve trovarlo fermo ad aspettare la fine della migrazione — la
    /// posizione da salvare, l'ultimo `optimize` rinunciano, e la migrazione
    /// interrotta si rifà al giro dopo, un passo alla volta come sempre.
    fn aspetta_l_apertura(&self, entro: Option<Duration>) {
        self.aspetta_l_apertura_se(entro, crate::spegnimento::in_uscita);
    }

    /// [`Stato::aspetta_l_apertura`] con la domanda «si sta uscendo?» passata
    /// da fuori: la bandiera vera è una per processo, e le prove non la
    /// possono abbassare.
    fn aspetta_l_apertura_se(&self, entro: Option<Duration>, uscendo: impl Fn() -> bool) {
        /// Ogni quanto chi aspetta guarda se si sta uscendo.
        const PASSO: Duration = Duration::from_millis(100);
        let inizio = Instant::now();
        let mut fase = self
            .apertura
            .fase
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        while !fase.finita() && !uscendo() {
            let passo = match entro {
                Some(entro) => match entro.checked_sub(inizio.elapsed()) {
                    Some(resta) if !resta.is_zero() => resta.min(PASSO),
                    _ => return,
                },
                None => PASSO,
            };
            fase = self
                .apertura
                .finita
                .wait_timeout(fase, passo)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
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
        if let Some(store) = self.store_copertine.get() {
            return Ok(store.clone());
        }
        let guardia = prendi_la_libreria(self);
        match guardia.as_ref() {
            Ok(libreria) => Ok(libreria.covers.clone()),
            Err(errore) => Err(errore.clone()),
        }
    }

    /// La cartella dati, senza chiedere il lucchetto. `None` se la libreria
    /// non si è aperta, o non ancora.
    pub fn cartella_dati(&self) -> Option<&Path> {
        self.cartella_dati.get().map(PathBuf::as_path)
    }

    /// La cartella in cui la libreria sta o doveva stare, anche se non si è
    /// aperta. `None` dopo un rifiuto del trasloco. Vedi il campo.
    pub fn cartella_della_libreria(&self) -> Option<&Path> {
        self.cartella_scelta.as_deref()
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
    /// scansione — o farsi da parte fra un lotto e l'altro — senza però
    /// prenderne il turno: l'analisi (`crate::analisi`) e l'arricchimento
    /// (`crate::arricchimento`), che prendendo il turno anche per un istante
    /// farebbero rispondere «occupato» a una «Scansiona» premuta in
    /// quell'istante. `crate::procura` la legge solo per dire nel diario che la
    /// sua scansione aspetta il turno. La sorveglianza (`crate::sorveglia`)
    /// non la legge: il turno lo prende, prima di decidere che cosa guardare.
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
    #[cfg_attr(
        mobile,
        allow(
            dead_code,
            reason = "fuori dalle prove la usa solo l'updater del computer"
        )
    )]
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
/// Quel che serve a rispondere a «il backup gira?»: quanti giri ha fatto,
/// quante volte è caduto, quando e perché l'ultima. Oggi nessuna schermata la
/// mostra: la legge chi chiama il comando [`fili_salute`] — uno strumento di
/// diagnosi, come lo descrive la documentazione del sito — e per chi usa
/// l'applicazione la traccia di un filo che cade resta il diario.
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

/// La salute dei fili di sottofondo, per chi la chiede.
///
/// Registrato come comando ma senza un chiamante nella finestra, e non è una
/// svista da togliere: è lo strumento di diagnosi che la documentazione del
/// sito cita. Nessuna schermata lo mostra, quindi un filo che cade in panico a
/// ogni giro si vede nel diario e qui, non nella finestra.
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

/// L'errore della libreria che si sta ancora aprendo: vedi [`Stato::in_apertura`].
fn in_apertura() -> AppError {
    AppError::new(ErrorCode::LibraryOpening)
}

/// A che punto è l'apertura della libreria, come la riceve la finestra.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "fase", rename_all = "camelCase")]
pub enum FaseLibreria {
    /// Si sta aprendo.
    Apertura,
    /// Si sta aggiornando: c'è da migrare, e prima la copia
    /// `rymembra.db.schema-<da>.bak`.
    Aggiornamento {
        /// La versione dello schema da cui si parte.
        da: u32,
    },
    /// Aperta, e l'avvio è finito.
    Pronta,
    /// Non si è aperta: il perché è l'errore della libreria. Anche dopo un
    /// rifiuto del trasloco.
    Guasto,
}

impl FaseLibreria {
    /// L'avvio è finito, in un modo o nell'altro.
    fn finita(self) -> bool {
        matches!(self, Self::Pronta | Self::Guasto)
    }
}

/// La fase dell'apertura, e la condizione su cui chi la aspetta dorme.
struct Apertura {
    /// A che punto è.
    fase: Mutex<FaseLibreria>,
    /// Suona quando la fase diventa una fine: vedi [`Stato::apri_il_cancello`].
    finita: Condvar,
}

/// A che punto è l'apertura della libreria.
///
/// Sincrono, e senza il lucchetto della libreria: lo chiede la finestra appena
/// nata, mentre la libreria si apre su un altro filo, e non deve aspettarla.
/// Le fasi successive arrivano con l'evento `libreria:stato`.
#[tauri::command]
pub fn libreria_fase(stato: tauri::State<'_, Stato>) -> FaseLibreria {
    stato.fase()
}

/// La versione dello schema da cui l'apertura di `percorso` migrerà, se
/// migrerà: `Some(da)` con `0 < da < LATEST_VERSION`.
///
/// Si legge con una connessione sua, senza creare il file, senza migrare e
/// con `query_only`, chiusa prima che [`apri_libreria`] apra la sua: `PRAGMA
/// user_version` passa dal giornale, quindi dice la versione vera anche dopo
/// un'uscita senza checkpoint. Non in sola lettura: un lettore WAL senza lo
/// `-shm` non sempre si apre, e la domanda è troppo piccola per un ripiego.
/// Un file che non c'è, che non si apre o che non risponde vale `None`: la
/// finestra dirà «Apro la libreria…», e l'apertura vera racconterà il guasto.
pub fn versione_da_aggiornare(percorso: &Path) -> Option<u32> {
    let connessione = Connection::open_with_flags(
        rymembra_app::files::percorso_lungo(percorso),
        OpenFlags::default().difference(OpenFlags::SQLITE_OPEN_CREATE),
    )
    .ok()?;
    let _ = connessione.pragma_update(None, "query_only", "ON");
    let versione: i64 = connessione
        .query_row("PRAGMA user_version", [], |riga| riga.get(0))
        .ok()?;
    drop(connessione);
    u32::try_from(versione)
        .ok()
        .filter(|da| *da > 0 && *da < rymembra_app::db::LATEST_VERSION)
}

/// Apre la libreria in `data_dir`: la cartella, le copertine, il database con
/// le sue migrazioni. È il lavoro lungo dell'avvio, e gira sul filo
/// dell'apertura (`lib.rs`, `apri_e_avvia`).
pub fn apri_libreria(data_dir: PathBuf) -> Result<Libreria, AppError> {
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

/// La riga del diario che dice che la libreria si apre su un filo suo, e se
/// c'è da aggiornare: la riga di avvio vera arriva quando l'apertura è finita,
/// e fra le due il tempo dell'apertura si legge dai loro orari.
pub fn riga_dell_apertura(da: Option<u32>) {
    match da {
        Some(da) => nota!(
            "[avvio] la libreria si apre in disparte: da aggiornare dallo schema {da} al {}",
            rymembra_app::db::LATEST_VERSION
        ),
        None => nota!("[avvio] la libreria si apre in disparte"),
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
/// SQLite chiede prima di chiudere una connessione, e il giornale riportato nel
/// database.
///
/// # L'ottimizzazione, una volta sola
///
/// La sequenza di spegnimento in `main` gira per ognuno dei tre eventi che
/// dicono «si chiude», e di solito ne arrivano due o tre. La prima volta si
/// ottimizza, le altre no: una bandiera di processo, alzata **prima** di
/// provare, perché anche un tentativo che rinuncia al lucchetto non va
/// ripetuto — l'ottimizzazione costa quasi nulla, ma le statistiche di un
/// secondo giro non dicono niente di più.
///
/// # E il giornale, a ogni chiamata
///
/// Dopo l'ottimizzazione, nella stessa presa, `wal_checkpoint(TRUNCATE)`
/// ([`rymembra_app::db::svuota_il_giornale`]): il processo esce con
/// `process::exit` e la connessione non si chiude mai, quindi senza questo il
/// `rymembra.db-wal` restava pieno dopo ogni uscita — ed era quel giornale a
/// rovinare la copia di prima dell'aggiornamento rimessa a mano. Uno fallito
/// non toglie l'altro: sono due righe di diario diverse.
///
/// Il giornale **non** ha la bandiera, ed è il punto: dev'essere l'ultima
/// scrittura della sessione, e la sequenza che lo precede si ripete. Con un
/// brano in riproduzione ogni giro riscrive la posizione di ripresa
/// (`fili::salva_uscendo`) e, se ne ha, le preferenze audio: un giornale
/// svuotato solo al primo giro restava con l'ultima riscrittura — nel
/// collaudo in Windows Sandbox 4.152 o 8.272 byte, uno o due frame —, mentre
/// senza riproduzione non c'è niente da riscrivere e restava vuoto. Svuotarlo
/// a ogni chiamata, dopo i salvataggi della stessa chiamata, rende l'ultima
/// scrittura sempre lui; a giornale già vuoto il checkpoint non ha niente da
/// fare.
pub fn ottimizza_uscendo(stato: &Stato) {
    static OTTIMIZZATO: AtomicBool = AtomicBool::new(false);
    uscendo_con(stato, &OTTIMIZZATO);
}

/// Il corpo di [`ottimizza_uscendo`], con la bandiera dell'ottimizzazione
/// passata da fuori: quella di processo non permette due prove indipendenti.
fn uscendo_con(stato: &Stato, ottimizzato: &AtomicBool) {
    let ottimizza = !ottimizzato.swap(true, Ordering::AcqRel);
    match con_libreria_entro(stato, ATTESA_OTTIMIZZAZIONE_USCITA, |libreria| {
        Ok((
            ottimizza.then(|| rymembra_app::db::ottimizza(&libreria.connection)),
            rymembra_app::db::svuota_il_giornale(&libreria.connection),
        ))
    }) {
        Some(Ok((ottimizzato, giornale))) => {
            if let Some(Err(err)) = ottimizzato {
                nota!(
                    "[uscita] ottimizzazione del database non riuscita codice={} causa={}",
                    err.code().kind().code(),
                    err.cause().unwrap_or("—")
                );
            }
            match giornale {
                Ok(true) => {}
                Ok(false) => nota!(
                    "[uscita] giornale del database non svuotato: un lettore ne teneva ancora delle pagine"
                ),
                Err(err) => nota!(
                    "[uscita] giornale del database non svuotato codice={} causa={}",
                    err.code().kind().code(),
                    err.cause().unwrap_or("—")
                ),
            }
        }
        Some(Err(err)) => nota!(
            "[uscita] ottimizzazione del database non riuscita codice={} causa={}",
            err.code().kind().code(),
            err.cause().unwrap_or("—")
        ),
        // Alle chiamate dopo la prima non si tentava nessuna ottimizzazione:
        // quel che resta da dire è il giornale.
        None if ottimizza => nota!(
            "[uscita] ottimizzazione del database saltata: lucchetto non preso in {} ms",
            ATTESA_OTTIMIZZAZIONE_USCITA.as_millis()
        ),
        None => nota!(
            "[uscita] giornale del database non svuotato: lucchetto non preso in {} ms",
            ATTESA_OTTIMIZZAZIONE_USCITA.as_millis()
        ),
    }
}

/// «Torna alla copia di prima dell'aggiornamento»: rimette la copia più
/// recente al posto del database, mette da parte quello di adesso, e lascia
/// la libreria **chiusa** fino all'uscita.
///
/// Con la libreria aperta lo fa sulla sua connessione, col lucchetto in mano
/// per tutto il tempo: nessun comando e nessuna scansione scrivono in mezzo.
/// Con la libreria in errore — il caso in cui serve di più: una migrazione
/// fermata a metà — su un'apertura del file fatta apposta, senza migrare.
///
/// Dopo, la libreria è un errore come quella che non si è aperta, e chi chiama
/// chiude l'applicazione: questa versione, riaprendo una copia di uno schema
/// più vecchio, la rimigrerebbe subito.
///
/// # Errori
///
/// `library.scanBusy` durante una scansione; `fs.notFound` se la copia non c'è;
/// quelli di [`rymembra_app::db::torna_alla_copia`], e allora la libreria è
/// com'era.
pub fn torna_alla_copia_di_prima(stato: &Stato) -> Result<rymembra_app::db::Ritorno, AppError> {
    let Some(cartella) = stato.cartella_della_libreria() else {
        return Err(AppError::new(ErrorCode::InternalAborted {
            what: Some("ritorno alla copia di prima dell'aggiornamento".to_owned()),
        })
        .with_cause("nessuna cartella dei dati scelta"));
    };
    let percorso = cartella.join(NOME_DATABASE);
    let Some(copia) = rymembra_app::db::copia_di_prima(&percorso) else {
        return Err(AppError::new(ErrorCode::FsNotFound {
            path: format!("{}.schema-NNN.bak", percorso.display()),
        }));
    };
    let Some(_turno) = stato.turno_di_manutenzione() else {
        return Err(AppError::new(ErrorCode::LibraryScanBusy));
    };
    let mut guardia = prendi_la_libreria(stato);
    let ritorno = match guardia.as_mut() {
        Ok(libreria) => {
            rymembra_app::db::torna_alla_copia(&mut libreria.connection, &percorso, &copia)?
        }
        Err(_) => rymembra_app::db::torna_alla_copia_del_file(&percorso, &copia)?,
    };
    // La connessione si chiude qui, cadendo: da adesso ogni comando risponde
    // con questo errore, e nessuno scrive più nella copia rimessa.
    *guardia = Err(AppError::new(ErrorCode::InternalAborted {
        what: Some("libreria tornata alla copia di prima dell'aggiornamento".to_owned()),
    }));
    Ok(ritorno)
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
    // Un filo di sottofondo aspetta che la libreria si apra: per lui è come
    // quando il `setup` la apriva prima di farlo partire. Vedi `apertura`.
    stato.aspetta_l_apertura(None);
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
    // L'apertura si aspetta anche qui, ma dentro la stessa scadenza; e non sul
    // filo della finestra, che non aspetta mai: lì l'errore `library.opening`
    // è la risposta giusta.
    let inizio = Instant::now();
    if !sul_filo_della_finestra() {
        stato.aspetta_l_apertura(Some(entro));
    }
    let mut guardia = lucchetto_entro(&stato.libreria, entro.saturating_sub(inizio.elapsed()))?;
    Some(match guardia.as_mut() {
        Ok(libreria) => azione(libreria),
        Err(errore) => Err(errore.clone()),
    })
}

/// Una lettura della libreria che non chiede il suo lucchetto: su una
/// connessione di sola lettura aperta apposta, quando il giornale è WAL.
///
/// # Perché
///
/// Il lucchetto lo prendono a turno, in finestre brevi, tutti i lavori di
/// sottofondo — scansione, analisi, arricchimento, pulizie — e il primo che si
/// mette in coda col blocco lo riceve appena si libera. Chi invece lo vuole
/// entro una scadenza ([`con_libreria_entro`]) riprova ogni cinque millisecondi
/// e può perdere la gara per tutto il tempo in cui quei lavori girano. Era il
/// caso delle domande dei dispositivi collegati via internet (reperto 00-9):
/// dopo una scansione ogni «Sfoglia» rispondeva «non raggiungibile» per
/// minuti, e su una libreria vera la prima analisi dura ore. In WAL chi legge
/// da una connessione sua vede l'ultima transazione chiusa, e non ferma né è
/// fermato da chi scrive: il lucchetto qui non serve, chiunque lo tenga e per
/// quanto.
///
/// # Quando si torna al lucchetto
///
/// Con il giornale ripiegato su `TRUNCATE` — lì un lettore in più bloccherebbe
/// il commit di chi scrive, ed è la stessa regola del controllo rapido in
/// `manutenzione::controlla` — e senza una libreria aperta. Anche se la
/// connessione non si apre: allora lo si dice nel diario. In questi casi vale
/// [`con_libreria_entro`], `None` compreso.
///
/// # La connessione
///
/// Si apre a ogni chiamata e si chiude alla fine, come quella dell'indice delle
/// cartelle (`cartelle.rs`): qualche millisecondo accanto a una domanda che
/// viaggia in rete, e niente di condiviso fra due domande insieme, cioè nessun
/// altro lucchetto da aspettare. `SQLITE_OPEN_READ_ONLY` dice nel tipo che di
/// qui non passa nessuna scrittura né nessuna migrazione. L'attesa di SQLite,
/// se il file è occupato in un modo che anche un lettore WAL sente, è `entro`:
/// lo stesso patto della funzione.
pub fn leggi_a_parte<T>(
    stato: &Stato,
    entro: Duration,
    azione: impl FnOnce(&Connection) -> Result<T, AppError>,
) -> Option<Result<T, AppError>> {
    match lettore_a_parte(stato, entro) {
        Some(Ok(connessione)) => Some(azione(&connessione)),
        Some(Err(err)) => {
            lettore_che_non_si_apre(&err);
            con_libreria_entro(stato, entro, |libreria| azione(&libreria.connection))
        }
        None => con_libreria_entro(stato, entro, |libreria| azione(&libreria.connection)),
    }
}

/// Quanto una lettura della finestra aspetta, dentro SQLite, un file occupato in
/// un modo che anche un lettore WAL sente. Le stesse cinque secondi della
/// connessione principale (`BUSY_TIMEOUT_MS`): non è il filo della finestra a
/// aspettare — la lettura gira sul pool bloccante (`disparte::leggi_a_parte`).
const ATTESA_DEL_LETTORE_DELLA_FINESTRA: Duration = Duration::from_secs(5);

/// Come [`leggi_a_parte`], per le letture della **finestra**: quelle non possono
/// rinunciare. `leggi_a_parte` risponde `None` se, senza WAL, il lucchetto non
/// arriva entro `entro` — e una lista che non si legge non è una risposta da
/// mostrare. Qui, se non si può leggere a parte (giornale non WAL, libreria non
/// ancora aperta, connessione che non si apre) si ripiega su [`con_libreria`],
/// che aspetta quanto serve, come ogni comando.
///
/// Gira sul pool bloccante, mai sul filo della finestra: lo garantisce chi la
/// chiama (`disparte::leggi_a_parte`).
///
/// # Errori
///
/// Quelli dell'azione, e quelli di [`con_libreria`] se si ripiega.
pub fn leggi_o_aspetta<T>(
    stato: &Stato,
    azione: impl FnOnce(&Connection) -> Result<T, AppError>,
) -> Result<T, AppError> {
    match lettore_a_parte(stato, ATTESA_DEL_LETTORE_DELLA_FINESTRA) {
        Some(Ok(connessione)) => azione(&connessione),
        Some(Err(err)) => {
            lettore_che_non_si_apre(&err);
            con_libreria(stato, |libreria| azione(&libreria.connection))
        }
        None => con_libreria(stato, |libreria| azione(&libreria.connection)),
    }
}

/// La connessione di sola lettura, se il giornale è WAL e la libreria è aperta;
/// `None` altrimenti. Una per chiamata: vedi [`leggi_a_parte`].
fn lettore_a_parte(stato: &Stato, entro: Duration) -> Option<Result<Connection, AppError>> {
    stato
        .cartella_dati()
        .filter(|_| stato.in_wal.get().copied().unwrap_or(false))
        .map(|cartella| apri_un_lettore(&cartella.join(NOME_DATABASE), entro))
}

/// Una connessione di sola lettura che non si apre si dice nel diario, e si
/// ripiega sul lucchetto: la lettura non si perde.
fn lettore_che_non_si_apre(err: &AppError) {
    nota!(
        "[stato] la lettura a parte non si apre, si aspetta la libreria codice={} causa={}",
        err.code().kind().code(),
        err.cause().unwrap_or("—")
    );
}

/// La connessione di sola lettura di [`leggi_a_parte`].
///
/// # Errori
///
/// `db.openFailed` col percorso, se il file non si apre in lettura.
fn apri_un_lettore(percorso: &Path, attesa: Duration) -> Result<Connection, AppError> {
    // Dal percorso lungo, come la connessione principale: con una cartella dati
    // oltre i 260 caratteri la principale si apre, e questa deve aprirsi uguale.
    let connessione = Connection::open_with_flags(
        rymembra_app::files::percorso_lungo(percorso),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|err| {
        AppError::new(ErrorCode::DbOpenFailed {
            path: Some(percorso.display().to_string()),
        })
        .with_cause(err.to_string())
    })?;
    rymembra_app::db::prepara_connessione_secondaria(&connessione);
    // Dopo, perché `prepara_connessione_secondaria` mette l'attesa di sempre:
    // qui vale quella di chi chiede.
    let _ = connessione.busy_timeout(attesa);
    Ok(connessione)
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Il giornale svuotato è l'ultima scrittura di ogni chiamata dell'uscita.
    ///
    /// Con la bandiera dell'ottimizzazione sua e non quella di processo
    /// ([`ottimizza_uscendo`]): così non dipende da chi altro l'ha già alzata.
    #[test]
    fn uscendo_il_giornale_resta_vuoto() {
        let ottimizzato = AtomicBool::new(false);
        // Il processo esce con `process::exit`, senza chiudere la connessione:
        // quel che `uscendo_con` lascia sul disco è quel che resta. Un
        // `-wal` pieno accanto a `rymembra.db` è quello che rovinava la copia
        // di prima dell'aggiornamento rimessa a mano (R06b-2).
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        con_libreria(&stato, |libreria| {
            rymembra_app::settings::write(&libreria.connection, "player.position", "42")
        })
        .expect("una scrittura");
        let giornale = dir.path().join(format!("{NOME_DATABASE}-wal"));
        assert!(
            std::fs::metadata(&giornale).is_ok_and(|dati| dati.len() > 0),
            "la scrittura sta nel giornale"
        );

        uscendo_con(&stato, &ottimizzato);

        assert!(
            ottimizzato.load(Ordering::Acquire),
            "la prima chiamata ottimizza"
        );
        assert_eq!(
            std::fs::metadata(&giornale).map_or(0, |dati| dati.len()),
            0,
            "uscendo il giornale torna nel database"
        );

        // La sequenza di spegnimento gira due o tre volte di fila — alla
        // chiusura della finestra, a `ExitRequested`, a `Exit` — e con un brano
        // in riproduzione la seconda riscrive la posizione (`chiudi_uscendo`) un
        // attimo dopo il giornale svuotato. Nel collaudo in Windows Sandbox
        // restavano 4.152 o 8.272 byte nel `-wal`: 32 di intestazione più uno o
        // due frame da 24 + 4.096. Il giornale svuotato dev'essere l'ultima
        // scrittura di **ogni** chiamata, non solo della prima (D-55).
        con_libreria(&stato, |libreria| {
            rymembra_app::settings::write(&libreria.connection, "player.position", "43")
        })
        .expect("la scrittura della seconda chiamata");
        assert!(
            std::fs::metadata(&giornale).is_ok_and(|dati| dati.len() > 0),
            "la scrittura arrivata dopo l'uscita sta di nuovo nel giornale"
        );

        uscendo_con(&stato, &ottimizzato);

        assert_eq!(
            std::fs::metadata(&giornale).map_or(0, |dati| dati.len()),
            0,
            "anche alla seconda chiamata il giornale torna nel database"
        );
    }

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

    // ── l'apertura fuori dal `setup` (R06b-5) ───────────────────────────

    #[test]
    fn una_libreria_in_apertura_fa_aspettare_e_poi_si_apre() {
        // La cucitura dell'apertura: lo stato nasce «in apertura» e si
        // registra, la libreria arriva da un altro filo. Chi non può aspettare
        // riceve `library.opening`, ritentabile; chi lavora in sottofondo
        // aspetta, e trova la libreria aperta; le scorciatoie senza lucchetto
        // si riempiono solo allora.
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::in_apertura(dir.path().to_path_buf());
        assert_eq!(stato.fase(), FaseLibreria::Apertura);
        assert!(stato.cartella_dati().is_none(), "niente scorciatoie prima");
        assert_eq!(
            stato.cartella_della_libreria(),
            Some(dir.path()),
            "la cartella in cui la si cerca si sa da subito"
        );
        let copertine = stato.copertine().expect_err("nessuna copertina prima");
        assert_eq!(copertine.code().kind().code(), "library.opening");

        let scaduta = con_libreria_entro(&stato, Duration::from_millis(50), |_| Ok(()))
            .expect("il lucchetto è libero")
            .expect_err("la libreria non c'è ancora");
        assert_eq!(scaduta.code().kind().code(), "library.opening");
        assert!(scaduta.is_retryable());

        // Chi lavora in sottofondo aspetta. La domanda «si sta uscendo?» è
        // passata a mano: la bandiera vera è una per tutto il binario delle
        // prove, e un'altra prova la alza (`spegnimento`).
        std::thread::scope(|filo| {
            let in_attesa = filo.spawn(|| {
                stato.aspetta_l_apertura_se(None, || false);
                con_libreria(&stato, |l| Ok(l.data_dir.clone()))
            });
            std::thread::sleep(Duration::from_millis(100));
            assert!(!in_attesa.is_finished(), "chi lavora in sottofondo aspetta");
            stato.completa(apri_libreria(dir.path().to_path_buf()));
            assert_eq!(stato.apri_il_cancello(), FaseLibreria::Pronta);
            let trovata = in_attesa.join().expect("il filo").expect("la libreria");
            assert_eq!(trovata, dir.path());
        });
        assert_eq!(stato.fase(), FaseLibreria::Pronta);
        assert_eq!(stato.cartella_dati(), Some(dir.path()));
        // E chi aspetta mentre l'applicazione esce non resta appeso a una
        // migrazione: rinuncia subito.
        let ancora = Stato::in_apertura(dir.path().to_path_buf());
        let inizio = Instant::now();
        ancora.aspetta_l_apertura_se(None, || true);
        assert!(inizio.elapsed() < Duration::from_secs(1));
        assert!(stato.copertine().is_ok());
        assert_eq!(stato.in_wal.get().copied(), Some(true));
    }

    #[test]
    fn una_libreria_che_non_si_apre_finisce_in_guasto() {
        // L'apertura che fallisce non lascia nessuno ad aspettare: il cancello
        // si apre col guasto, e chi chiede la libreria riceve quello, come
        // quando il `setup` la apriva da sé.
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::in_apertura(dir.path().to_path_buf());
        stato.completa(Err(AppError::new(ErrorCode::DbVersionAhead {
            db_version: 99,
            app_version: rymembra_app::db::LATEST_VERSION,
        })));
        assert_eq!(stato.apri_il_cancello(), FaseLibreria::Guasto);
        let errore = con_libreria(&stato, |_| Ok(())).expect_err("non si è aperta");
        assert_eq!(errore.code().kind().code(), "db.versionAhead");
        assert!(stato.cartella_dati().is_none());
        // Un rifiuto del trasloco è un guasto da subito: nessuno aspetta.
        assert_eq!(
            Stato::rifiutato(rifiuto_di_prova()).fase(),
            FaseLibreria::Guasto
        );
    }

    #[test]
    fn uno_schema_vecchio_si_annuncia_come_aggiornamento() {
        // «Aggiorno la libreria…» solo quando c'è da migrare: un file di uno
        // schema di prima. Uno nuovo, uno già aggiornato e uno che non c'è
        // dicono «Apro la libreria…», e nessuno viene creato.
        let dir = tempfile::tempdir().expect("cartella");
        let vecchio = dir.path().join("vecchio.db");
        {
            let connessione = Connection::open(&vecchio).expect("apertura");
            for migrazione in rymembra_app::db::MIGRATIONS
                .iter()
                .filter(|m| m.version <= 38)
            {
                connessione
                    .execute_batch(migrazione.sql)
                    .expect("migrazione");
            }
            connessione
                .execute_batch("PRAGMA user_version = 38;")
                .expect("versione");
        }
        assert_eq!(versione_da_aggiornare(&vecchio), Some(38));
        let aggiornato = dir.path().join("aggiornato.db");
        drop(rymembra_app::db::open(&aggiornato).expect("apertura"));
        assert_eq!(versione_da_aggiornare(&aggiornato), None);
        let mancante = dir.path().join("mancante.db");
        assert_eq!(versione_da_aggiornare(&mancante), None);
        assert!(!mancante.exists(), "la domanda non crea niente");

        let stato = Stato::in_apertura(dir.path().to_path_buf());
        stato.in_aggiornamento(38);
        assert_eq!(stato.fase(), FaseLibreria::Aggiornamento { da: 38 });
    }

    #[test]
    fn la_fase_si_scrive_come_la_legge_la_finestra() {
        // `FaseLibreria` in `ipc/libreria.ts`.
        let scritta = |fase| serde_json::to_value(fase).expect("serializzata");
        assert_eq!(
            scritta(FaseLibreria::Apertura),
            serde_json::json!({ "fase": "apertura" })
        );
        assert_eq!(
            scritta(FaseLibreria::Aggiornamento { da: 38 }),
            serde_json::json!({ "fase": "aggiornamento", "da": 38 })
        );
        assert_eq!(
            scritta(FaseLibreria::Pronta),
            serde_json::json!({ "fase": "pronta" })
        );
        assert_eq!(
            scritta(FaseLibreria::Guasto),
            serde_json::json!({ "fase": "guasto" })
        );
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

/// Le misure di FR2: quanto costa la rilettura della pagina di un artista, sotto il
/// lucchetto e fuori. Vedi `stato/misure.rs`.
#[cfg(test)]
mod misure;

/// La lettura della finestra senza il lucchetto: le prove (FR2).
#[cfg(test)]
mod prove_a_parte;
