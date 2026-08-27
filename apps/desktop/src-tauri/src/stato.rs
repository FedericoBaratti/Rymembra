//! Quel che l'applicazione tiene aperto mentre gira.
//!
//! Una connessione al database e uno store di copertine, dietro un mutex. Non
//! un pool: SQLite in WAL regge un solo scrittore comunque, e le letture di
//! questa applicazione durano microsecondi. Un pool aggiungerebbe la possibilità
//! che due comandi vedano due transazioni diverse senza toglierne nessuna reale.
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
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

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

    /// Qualcuno ha chiesto di fermare la scansione in corso.
    ///
    /// # Perché **non** sta dietro il mutex della libreria
    ///
    /// Perché sarebbe irraggiungibile. `scansiona` chiama `con_libreria`, che
    /// tiene il lucchetto per tutta la durata della scansione — venti secondi
    /// sulla libreria vera, la prima volta. Un comando `annulla_scansione` che
    /// chiedesse lo stesso lucchetto resterebbe in coda dietro la scansione che
    /// deve fermare, e arriverebbe a fermarla dopo la fine.
    ///
    /// Un `AtomicBool` accanto al mutex si legge e si scrive senza chiederlo. È
    /// esattamente il caso per cui gli atomici esistono: un bit condiviso fra
    /// due fili, senza niente da tenere coerente insieme a lui.
    scansione_da_fermare: AtomicBool,
}

impl Stato {
    /// Apre la libreria nella cartella dati data.
    pub fn apri(data_dir: PathBuf) -> Self {
        Self {
            libreria: Mutex::new(apri_libreria(data_dir)),
            scansione_da_fermare: AtomicBool::new(false),
        }
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
pub struct Turno<'a>(&'a AtomicBool);

impl Turno<'_> {
    /// Prende il turno, se è libero.
    pub fn prendi(bandiera: &AtomicBool) -> Option<Turno<'_>> {
        bandiera
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Turno(bandiera))
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
        Ok(libreria) => nota!(
            "[avvio] libreria aperta dati={} migrazioni={} fts5={}",
            libreria.data_dir.display(),
            libreria.migrazioni,
            libreria.fts5
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

/// Esegue `azione` sulla libreria aperta.
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
    let mut guardia = stato
        .libreria
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match guardia.as_mut() {
        Ok(libreria) => azione(libreria),
        Err(errore) => Err(errore.clone()),
    }
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
}
