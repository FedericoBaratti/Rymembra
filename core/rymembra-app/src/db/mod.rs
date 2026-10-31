//! Apertura del database e catena delle migrazioni.
//!
//! # Le migrazioni vanno solo avanti
//!
//! Non c'è `down`. Una migrazione che sa tornare indietro è una migrazione che
//! dichiara di saper ricostruire informazione che ha appena buttato, e quasi
//! sempre è una bugia. Tornare a una versione precedente dell'app si fa
//! ripristinando un backup, che è un'operazione onesta.
//!
//! Da qui la guardia su [`ErrorCode::DbVersionAhead`]: un database scritto da
//! una versione più nuova non si apre e non si «aggiusta». Aprirlo comunque
//! vorrebbe dire far girare query che non conoscono metà delle colonne, ed è
//! così che si corrompe una libreria invece di rifiutarsi di toccarla.
//!
//! # Il giornale, e perché si ripiega invece di rifiutare
//!
//! [`configure`] chiede `PRAGMA journal_mode = WAL`, e **WAL
//! non funziona su un percorso di rete**: ha bisogno di un file di memoria
//! condivisa (`-shm`) mappato in memoria da tutti i processi che toccano il
//! database, e una condivisione SMB non sa offrirlo. Chi tiene la cartella dati
//! su una share — perché `%APPDATA%` è reindirizzato, o perché `RYMEMBRA_DATI`
//! punta là — si ritrovava un `disk I/O error` raccontato come «il database ha
//! rifiutato una richiesta: riavvia Rymembra», che era falso due volte: non era
//! una richiesta sbagliata, e riavviare non cambiava niente.
//!
//! Quando WAL non si accende si **ripiega** su `TRUNCATE`, e non si rifiuta
//! l'apertura: rifiutare lascerebbe senza libreria chi oggi funziona a metà.
//! E non si **sposta** la cartella dati da soli: spostare i dati di qualcuno a
//! sua insaputa è irreversibile, e un programma che lo fa all'avvio è un
//! programma di cui non si può più prevedere dove tiene le cose.
//!
//! Il prezzo del ripiego va detto, perché si sente: `TRUNCATE` serializza
//! letture e scritture, quindi durante una scansione l'interfaccia si impunta —
//! che è esattamente il motivo per cui WAL era la prima scelta. Lo dicono due
//! cose: la riga di avvio, che porta `giornale=truncate`, e il messaggio di
//! [`ErrorCode::DbNetworkPath`], che consiglia di spostare la cartella dati su
//! un disco locale.

mod manutenzione;
mod migrations;

pub use manutenzione::{
    Controllo, ISTANTANEE_TENUTE, Istantanea, controlla_il_file, controllo_rapido, nome_istantanea,
    ottimizza,
};
pub use migrations::{LATEST_VERSION, MIGRATIONS, Migration};

use std::path::Path;
use std::sync::{Mutex, PoisonError};

use rusqlite::{Connection, ErrorCode as CodiceSqlite};
use rymembra_domain::errors::{AppError, ErrorCode};

/// Quanto aspettare se un'altra connessione sta scrivendo, prima di arrendersi.
///
/// Pubblica perché c'è chi la accorcia per la durata di un'azione — il filo
/// della finestra, che non può aspettare cinque secondi — e poi deve rimetterla
/// esattamente com'era.
pub const BUSY_TIMEOUT_MS: u32 = 5_000;

/// Traduce un errore di SQLite nel catalogo, tenendo il testo originale come causa.
///
/// Per i codici che il chiamante **sa** già — l'apertura, una migrazione — dove
/// leggere `sqlite_error_code()` non aggiungerebbe niente a quel che si sta per
/// dire. Quando il codice non si sa, si passa da [`codice_da_sqlite`].
fn db_error(code: ErrorCode, err: &rusqlite::Error) -> AppError {
    AppError::new(code).with_cause(err.to_string())
}

/// Quel che serve a [`codice_da_sqlite`] per scegliere, e che il punto di
/// chiamata non ha in mano.
///
/// # Perché è uno stato di modulo e non due parametri
///
/// Perché i punti che traducono un errore di SQLite sono **quattrocentosettanta
/// e passa**, e tutti passano da quattro helper che hanno in mano una stringa
/// («apertura della transazione dei vicini») e un `&rusqlite::Error`: non la
/// connessione, non il percorso del file, non il giornale in uso. Portare due
/// parametri in più fino a ognuno di quei punti sarebbe un'ondata di modifiche
/// meccaniche in venti file per un'informazione che è la stessa per tutto il
/// processo — il database della libreria si apre **una volta**, e dove sta e
/// come ha acceso il giornale non cambiano più fino alla chiusura.
///
/// Quindi: lo scrive [`open`] e lo legge [`codice_da_sqlite`]. Un `Mutex` e non
/// tre atomici perché i tre campi vanno letti insieme o non vanno letti: un
/// percorso di una share vecchia accanto a un giornale nuovo descriverebbe un
/// database che non è mai esistito.
#[derive(Debug, Clone)]
struct Contesto {
    /// Dove sta il file, se non è in memoria.
    percorso: Option<String>,
    /// Il percorso è una condivisione di rete, per quel che si riesce a
    /// riconoscerne — vedi [`crate::files::percorso_di_rete`].
    di_rete: bool,
    /// Il giornale è ripiegato: WAL non si è accesa.
    ripiegato: bool,
}

/// Com'era prima di aprire qualsiasi cosa: niente file, niente rete, niente
/// ripiego. È anche lo stato in cui vivono i database in memoria delle prove.
static CONTESTO: Mutex<Contesto> = Mutex::new(Contesto {
    percorso: None,
    di_rete: false,
    ripiegato: false,
});

/// Lo stato corrente, senza farsi fermare da un lucchetto avvelenato.
///
/// Un `Mutex` avvelenato qui vuol dire che un altro filo è panicato mentre
/// aggiornava tre campi descrittivi: il dato è intatto, e rifiutarsi di leggerlo
/// trasformerebbe una diagnosi in un secondo panico **dentro la traduzione di
/// un errore**, cioè nel posto peggiore.
fn contesto() -> Contesto {
    CONTESTO
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Sceglie il codice del catalogo guardando quel che SQLite ha davvero detto.
///
/// # Il difetto che chiude
///
/// Fino alla 2.3.1 **ogni** `rusqlite::Error` diventava
/// [`ErrorCode::DbQueryFailed`], cioè la frase «il database ha rifiutato una
/// richiesta: se succede di continuo, riavvia Rymembra». Un file bloccato per
/// mezzo secondo dalla scansione, una cartella dati diventata di sola lettura,
/// un database corrotto e una share Wi-Fi su cui WAL non può funzionare
/// arrivavano all'utente come la stessa riga, con lo stesso consiglio — che per
/// tre di quei quattro casi non serviva a niente. Nello stesso tempo
/// [`ErrorCode::DbLocked`] era dichiarato nel catalogo, tradotto in due lingue,
/// e **prodotto da nessuna riga di codice**.
///
/// # Le scelte, e perché sono queste
///
/// - `SQLITE_BUSY` / `SQLITE_LOCKED` → [`ErrorCode::DbLocked`], che è
///   ritentabile: è l'unico guasto del database che passa da sé.
/// - `SQLITE_READONLY` → [`ErrorCode::DbReadOnly`]. Aspettare non serve: serve
///   che qualcuno cambi i permessi.
/// - `SQLITE_CORRUPT` / `SQLITE_NOTADB` → [`ErrorCode::DbCorrupt`], che è
///   `Fatal` apposta: continuare a scrivere su un file malformato è il modo di
///   trasformare un backup ancora buono in due file rotti.
/// - `SQLITE_IOERR` → [`ErrorCode::DbNetworkPath`] se il percorso dei dati è di
///   rete **o** il giornale è ripiegato, altrimenti [`ErrorCode::DbIoFailed`].
///   I due dicono all'utente due cose diverse — «sposta la cartella» contro
///   «guarda il disco» — e mandarlo nel posto sbagliato costa un pomeriggio.
/// - `SQLITE_CANTOPEN` → la stessa coppia. Fino alla 2.3.1 cadeva nel ripiego,
///   e il file che non si apre più perché la share si è staccata diceva «il
///   database ha rifiutato una richiesta, il difetto è di Rymembra».
/// - `SQLITE_PROTOCOL` → [`ErrorCode::DbNetworkPath`] con gli stessi indizi,
///   altrimenti [`ErrorCode::DbLocked`]: è il protocollo dei lucchetti di WAL
///   che fallisce, e sul disco locale è una corsa che si riprova.
/// - `SQLITE_CONSTRAINT_FOREIGNKEY` → [`ErrorCode::LibraryTrackNotFound`]. Una
///   chiave esterna che rifiuta una riga vuol dire, in questa libreria, quasi
///   sempre una cosa sola: si stava scrivendo qualcosa **a nome di un brano che
///   non c'è più** — un'aggiunta a una playlist, un ascolto, una correzione —
///   perché una scansione l'ha tolto nel frattempo. Con la musica su un NAS è
///   il caso normale: la share che singhiozza fa sparire righe mentre la
///   finestra ne mostra ancora gli identificativi. Fino alla 3.0 arrivava come
///   «il database ha rifiutato una richiesta, il difetto è di Rymembra».
/// - `SQLITE_CONSTRAINT_UNIQUE` / `_PRIMARYKEY` → [`ErrorCode::DbDuplicate`]:
///   la riga c'è già, e la causa più comune è un'altra operazione che l'ha
///   scritta un attimo prima. Gli altri vincoli (`CHECK`, `NOT NULL`) restano
///   nel ripiego: quelli sì sono una riga sbagliata, cioè un difetto nostro.
/// - `SQLITE_FULL` → [`ErrorCode::FsDiskFull`]: il disco della cartella dati è
///   pieno, e la frase giusta è quella del disco, non quella del database.
/// - `SQLITE_PERM` / `SQLITE_AUTH` → [`ErrorCode::FsPermissionDenied`]: il
///   sistema ha negato l'accesso al file, che è una questione di permessi della
///   cartella e non di richieste sbagliate.
/// - tutto il resto → [`ErrorCode::DbQueryFailed`] con il suo `detail`, com'era.
///   Il ripiego resta un ripiego: un vincolo di forma violato, un tipo che non
///   torna e una sintassi sbagliata sono difetti di Rymembra, non cose da
///   raccontare.
///
/// # La causa, e il codice esteso dentro
///
/// Il `detail` viaggia comunque, e nella causa finiscono **tre** cose: quale
/// operazione (`detail`), cosa ha detto SQLite (il testo di `rusqlite`) e il
/// **codice esteso**. Il terzo mancava, ed era il pezzo che serviva: `rusqlite`
/// scrive «FOREIGN KEY constraint failed» e butta il 787, e un «constraint
/// failed» da solo non dice quale dei sei vincoli, cioè quale dei sei difetti.
/// La causa è la metà che `errore::errore`, nella finestra, scrive nel diario
/// per ogni guasto che arriva a schermo: con questa riga la prossima
/// segnalazione di «il database ha rifiutato» arriva con in mano il numero.
pub fn codice_da_sqlite(detail: &str, err: &rusqlite::Error) -> AppError {
    codice_con_contesto(detail, err, &contesto())
}

/// La decisione di [`codice_da_sqlite`] con il contesto passato a mano.
///
/// Separata perché è l'unica parte che sceglie, e così si prova senza aprire
/// niente e senza dipendere da quel che le altre prove hanno lasciato nello
/// stato del modulo.
fn codice_con_contesto(detail: &str, err: &rusqlite::Error, contesto: &Contesto) -> AppError {
    let Some(codice) = err.sqlite_error_code() else {
        // Non viene da SQLite: è rusqlite che si lamenta di noi — una colonna
        // che non c'è, un tipo che non si converte. Resta quel che era.
        return AppError::new(ErrorCode::DbQueryFailed {
            detail: Some(detail.to_owned()),
        })
        .with_cause(causa(detail, err));
    };
    let esteso = err.sqlite_error().map(|errore| errore.extended_code);
    let code = match codice {
        CodiceSqlite::ConstraintViolation
            if esteso == Some(rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY) =>
        {
            // Senza identificativo: il vincolo non dice quale riga ha
            // rifiutato, e indovinarlo da qui vorrebbe dire rileggere il
            // database dentro la traduzione di un errore. Chi chiama e lo sa
            // — una funzione che ha un solo brano in mano — lo scrive da sé.
            ErrorCode::LibraryTrackNotFound { track_id: None }
        }
        CodiceSqlite::ConstraintViolation
            if matches!(
                esteso,
                Some(
                    rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
                        | rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY
                )
            ) =>
        {
            ErrorCode::DbDuplicate {
                detail: Some(detail.to_owned()),
            }
        }
        CodiceSqlite::DiskFull => ErrorCode::FsDiskFull {
            path: contesto.percorso.clone(),
        },
        CodiceSqlite::PermissionDenied | CodiceSqlite::AuthorizationForStatementDenied => {
            ErrorCode::FsPermissionDenied {
                // Vuoto per un database in memoria, che un percorso non ce
                // l'ha: la frase del codice regge anche senza.
                path: contesto.percorso.clone().unwrap_or_default(),
            }
        }
        CodiceSqlite::DatabaseBusy | CodiceSqlite::DatabaseLocked => ErrorCode::DbLocked,
        CodiceSqlite::ReadOnly => ErrorCode::DbReadOnly,
        CodiceSqlite::DatabaseCorrupt | CodiceSqlite::NotADatabase => ErrorCode::DbCorrupt {
            path: contesto.percorso.clone(),
            // Non è stato messo in quarantena: questa funzione traduce, non
            // sposta file. Chi deciderà di metterlo da parte riempirà il campo.
            quarantined_as: None,
        },
        CodiceSqlite::SystemIoFailure
        | CodiceSqlite::CannotOpen
        | CodiceSqlite::FileLockingProtocolFailed
            if sospetto_di_rete(err, contesto) =>
        {
            ErrorCode::DbNetworkPath {
                path: contesto.percorso.clone(),
            }
        }
        CodiceSqlite::SystemIoFailure | CodiceSqlite::CannotOpen => ErrorCode::DbIoFailed {
            detail: Some(detail.to_owned()),
        },
        // `SQLITE_PROTOCOL` su un disco locale è la corsa fra due connessioni
        // sulla memoria condivisa di WAL: SQLite stesso dice di riprovare, ed è
        // il caso di `DbLocked`.
        CodiceSqlite::FileLockingProtocolFailed => ErrorCode::DbLocked,
        _ => ErrorCode::DbQueryFailed {
            detail: Some(detail.to_owned()),
        },
    };
    AppError::new(code).with_cause(causa(detail, err))
}

/// La causa da scrivere accanto al codice: quale operazione, cosa ha detto
/// SQLite, e con quale codice esteso.
///
/// Il codice esteso si aggiunge solo quando c'è — gli errori di `rusqlite` che
/// non vengono da SQLite non ne hanno — e porta accanto il suo nome quando è uno
/// di quelli che si incontrano davvero: `787` letto in un diario non dice
/// niente a nessuno, `SQLITE_CONSTRAINT_FOREIGNKEY` dice già dove guardare.
fn causa(detail: &str, err: &rusqlite::Error) -> String {
    let Some(errore) = err.sqlite_error() else {
        return format!("{detail}: {err}");
    };
    let esteso = errore.extended_code;
    match nome_esteso(esteso) {
        Some(nome) => format!("{detail}: {err} [sqlite {esteso} {nome}]"),
        None => format!("{detail}: {err} [sqlite {esteso}]"),
    }
}

/// Il nome dei codici estesi che valgono una riga di diario.
///
/// Non tutti: sono una sessantina, e la maggior parte non si vede mai fuori da
/// un banco di prova. Questi sono quelli che dividono in due un codice primario
/// — sei vincoli diversi dietro lo stesso `SQLITE_CONSTRAINT`, la memoria
/// condivisa di WAL dietro lo stesso `SQLITE_IOERR`, l'istantanea scaduta
/// dietro lo stesso `SQLITE_BUSY` — cioè quelli per cui il numero è la diagnosi.
fn nome_esteso(esteso: std::ffi::c_int) -> Option<&'static str> {
    use rusqlite::ffi;
    Some(match esteso {
        ffi::SQLITE_CONSTRAINT_FOREIGNKEY => "SQLITE_CONSTRAINT_FOREIGNKEY",
        ffi::SQLITE_CONSTRAINT_UNIQUE => "SQLITE_CONSTRAINT_UNIQUE",
        ffi::SQLITE_CONSTRAINT_PRIMARYKEY => "SQLITE_CONSTRAINT_PRIMARYKEY",
        ffi::SQLITE_CONSTRAINT_CHECK => "SQLITE_CONSTRAINT_CHECK",
        ffi::SQLITE_CONSTRAINT_NOTNULL => "SQLITE_CONSTRAINT_NOTNULL",
        ffi::SQLITE_CONSTRAINT_TRIGGER => "SQLITE_CONSTRAINT_TRIGGER",
        ffi::SQLITE_BUSY_SNAPSHOT => "SQLITE_BUSY_SNAPSHOT",
        ffi::SQLITE_BUSY_RECOVERY => "SQLITE_BUSY_RECOVERY",
        ffi::SQLITE_IOERR_SHMOPEN => "SQLITE_IOERR_SHMOPEN",
        ffi::SQLITE_IOERR_SHMSIZE => "SQLITE_IOERR_SHMSIZE",
        ffi::SQLITE_IOERR_SHMMAP => "SQLITE_IOERR_SHMMAP",
        ffi::SQLITE_IOERR_LOCK => "SQLITE_IOERR_LOCK",
        ffi::SQLITE_READONLY_DBMOVED => "SQLITE_READONLY_DBMOVED",
        _ => return None,
    })
}

/// Un `SQLITE_IOERR` che viene dal posto dove sta il file, non dal supporto?
///
/// Tre indizi, in ordine di forza:
///
/// 1. il **codice esteso** dice memoria condivisa — `SQLITE_IOERR_SHMOPEN`,
///    `SQLITE_IOERR_SHMSIZE`, `SQLITE_IOERR_SHMMAP`. Questi tre sono la firma
///    esatta di WAL su un filesystem che non sa mappare il file `-shm`, e non
///    hanno altra spiegazione plausibile: valgono da soli, anche su un percorso
///    che non sembra di rete, perché una lettera mappata non si riconosce dalla
///    forma (vedi [`crate::files::percorso_di_rete`]);
/// 2. il percorso dei dati **è** un UNC;
/// 3. il giornale è **ripiegato**, cioè `PRAGMA journal_mode = WAL` si è già
///    rifiutato una volta all'apertura. È l'indizio che copre il caso della
///    lettera mappata: il percorso non dice niente, ma il giornale sì.
fn sospetto_di_rete(err: &rusqlite::Error, contesto: &Contesto) -> bool {
    // `SQLITE_IOERR_LOCK` accanto ai tre della memoria condivisa: è il
    // filesystem che non sa dare un lucchetto a intervallo di byte, che è la
    // stessa firma — SMB con i lucchetti opportunistici, un NAS con un
    // filesystem che li finge.
    let memoria_condivisa = err.sqlite_error().is_some_and(|errore| {
        matches!(
            errore.extended_code,
            rusqlite::ffi::SQLITE_IOERR_SHMOPEN
                | rusqlite::ffi::SQLITE_IOERR_SHMSIZE
                | rusqlite::ffi::SQLITE_IOERR_SHMMAP
                | rusqlite::ffi::SQLITE_IOERR_LOCK
        )
    });
    memoria_condivisa || contesto.di_rete || contesto.ripiegato
}

/// Apre il database al percorso dato, applicando le migrazioni mancanti.
///
/// Restituisce anche quante migrazioni sono state applicate: è il numero che
/// finisce nella riga di avvio del log, e l'unico modo per accorgersi che un
/// aggiornamento ha toccato il database senza dirlo.
pub fn open(path: &Path) -> Result<Opened, AppError> {
    // Il contesto si scrive **prima** di aprire, e non dopo: se è l'apertura
    // stessa a fallire con un errore di I/O, [`codice_da_sqlite`] deve già
    // sapere dove si stava cercando di aprire.
    {
        let mut guardia = CONTESTO.lock().unwrap_or_else(PoisonError::into_inner);
        *guardia = Contesto {
            percorso: Some(path.display().to_string()),
            di_rete: crate::files::percorso_di_rete(path),
            ripiegato: false,
        };
    }
    // Il percorso che si apre può essere in forma verbatim; quello che finisce
    // nell'errore è sempre l'originale, perché è quello che l'utente riconosce.
    let da_aprire = crate::files::percorso_lungo(path);
    let connection = Connection::open(&da_aprire).map_err(|err| {
        db_error(
            ErrorCode::DbOpenFailed {
                path: Some(path.display().to_string()),
            },
            &err,
        )
    })?;
    open_connection(connection, Some(path))
}

/// Un database in memoria, per i test e per una diagnosi a vuoto.
///
/// **Non tocca il contesto del modulo**, e non per distrazione: un database in
/// memoria non ha un percorso, non sta su una rete e non può produrre un errore
/// di I/O, quindi non ha niente da dire a [`codice_da_sqlite`]. Se lo
/// reimpostasse, una diagnosi a vuoto fatta a libreria aperta cancellerebbe quel
/// che si sa del database vero — cioè proprio mentre si sta diagnosticando.
pub fn open_in_memory() -> Result<Opened, AppError> {
    let connection = Connection::open_in_memory()
        .map_err(|err| db_error(ErrorCode::DbOpenFailed { path: None }, &err))?;
    open_connection(connection, None)
}

/// Un database aperto e portato alla versione corrente.
#[derive(Debug)]
pub struct Opened {
    /// La connessione.
    pub connection: Connection,
    /// La versione da cui si è partiti.
    pub version_before: u32,
    /// Quante migrazioni sono state applicate ora.
    pub applied: usize,
    /// FTS5 è disponibile in questo binario di SQLite.
    pub fts5: bool,
    /// Il giornale in uso, in quattro valori e nessun altro:
    ///
    /// - `wal` — accesa e provata: il caso normale, e quello veloce;
    /// - `truncate` — WAL non è usabile e il ripiego ha tenuto: la libreria
    ///   funziona, e le scansioni fanno impuntare l'interfaccia;
    /// - `memory` — database in memoria, nessun giornale su disco;
    /// - `<modo>-non-usabile` — nemmeno il ripiego è riuscito. Il caso peggiore,
    ///   e quello che ha più bisogno di un nome nel diario.
    ///
    /// Sta qui perché è l'informazione che mancava. La riga di avvio diceva
    /// dove sta il database, quante migrazioni ha applicato e se c'è FTS5, e non
    /// diceva l'unica cosa che distingue una libreria veloce da una che si
    /// impunta a ogni scansione. Con `giornale=truncate` nel diario, «Rymembra è
    /// lento durante la scansione» smette di essere un'impressione.
    pub giornale: String,
    /// La copia fatta prima delle migrazioni, o perché non c'è.
    ///
    /// `None` quando non c'era niente da migrare — il caso di quasi ogni
    /// apertura — o quando non c'era niente da proteggere: un database nuovo, o
    /// in memoria. Vedi [`Istantanea`].
    pub istantanea: Option<Istantanea>,
}

/// Configura, migra e ottimizza una connessione appena aperta.
///
/// `percorso` è `None` per i database in memoria, e decide le due cose che
/// solo un file ha: l'istantanea prima delle migrazioni e l'ottimizzazione di
/// apertura. Ai database in memoria delle prove — centinaia — non servono né
/// l'una né l'altra.
fn open_connection(connection: Connection, percorso: Option<&Path>) -> Result<Opened, AppError> {
    let giornale = configure(&connection)?;
    let fts5 = has_fts5(&connection);
    let version_before = user_version(&connection)?;
    // Solo se c'è qualcosa da migrare **e** qualcosa da proteggere: la versione
    // zero è un database appena creato, e una versione dal futuro non si migra
    // (`migrate` la rifiuta subito dopo).
    let istantanea = percorso
        .filter(|_| version_before > 0 && version_before < LATEST_VERSION)
        .map(|percorso| manutenzione::prima_di_migrare(&connection, percorso, version_before));
    let applied = migrate(&connection)?;
    if percorso.is_some() {
        // Dopo le migrazioni e non prima: SQLite chiede un `optimize` dopo ogni
        // cambio di schema, e un indice appena creato è precisamente la cosa
        // su cui il pianificatore non ha ancora statistiche.
        manutenzione::ottimizza_all_apertura(&connection);
    }
    Ok(Opened {
        connection,
        version_before,
        applied,
        fts5,
        giornale,
        istantanea,
    })
}

/// Prepara una connessione **in più** allo stesso database della libreria.
///
/// # Perché esiste
///
/// Perché le connessioni in più sono due — il deposito delle risposte dei
/// cataloghi e l'indice delle cartelle — e ognuna si scriveva la sua attesa a
/// mano: cinque secondi, contro i [`BUSY_TIMEOUT_MS`] della principale, e le
/// chiavi esterne spente. Due numeri che dicono la stessa cosa in due posti sono
/// due numeri che divergono al primo ritocco.
///
/// Non tocca il giornale e non migra: `journal_mode` è scritto nel file, quindi
/// una connessione in più eredita da sé il ripiego della principale, e due
/// connessioni che migrano lo stesso database insieme sono il guasto da non
/// avere. Le due impostazioni che **non** stanno nel file sono queste.
///
/// Gli errori si ignorano: una connessione in più che non riesce ad aspettare
/// risponde «occupato» alla prima contesa, che è un guasto che si riprova, e
/// rifiutarne l'apertura per questo costerebbe la cache intera.
pub fn prepara_connessione_secondaria(connection: &Connection) {
    let _ = connection.busy_timeout(std::time::Duration::from_millis(u64::from(BUSY_TIMEOUT_MS)));
    let _ = connection.pragma_update(None, "foreign_keys", "ON");
}

/// Le impostazioni di connessione. Vanno rifatte a ogni apertura: non si
/// salvano nel file, tranne `journal_mode`.
///
/// Restituisce il giornale **in uso**, che non è sempre quello chiesto: vedi il
/// `//!` di questo modulo e [`negozia_il_giornale`].
fn configure(connection: &Connection) -> Result<String, AppError> {
    let fail = |err: rusqlite::Error| codice_da_sqlite("pragma di apertura", &err);

    // Il timeout **prima** del giornale, e non è un riordino cosmetico: la sonda
    // di [`wal_funziona`] prende il lucchetto di scrittura, e senza il timeout
    // già in forza un'altra istanza che stesse scrivendo in quell'istante la
    // farebbe fallire subito — facendo ripiegare su `TRUNCATE` una libreria
    // locale perfettamente sana.
    connection
        .busy_timeout(std::time::Duration::from_millis(u64::from(BUSY_TIMEOUT_MS)))
        .map_err(fail)?;

    // Le chiavi esterne sono spente per default in SQLite, e vanno riaccese a
    // ogni connessione. Sono ciò che fa sparire le righe di `playlist_tracks`
    // quando si cancella un brano: senza, restano a puntare nel vuoto e la
    // playlist mostra elementi che non esistono.
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(fail)?;

    let giornale = negozia_il_giornale(connection);
    if giornale.ripiegato {
        // L'annotazione serve a [`codice_da_sqlite`]: da qui in avanti un
        // `SQLITE_IOERR` su questo database diventa `db.networkPath` invece di
        // un generico guasto del disco, anche quando il percorso non ha la forma
        // di un UNC — il caso della lettera mappata.
        CONTESTO
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .ripiegato = true;
    }

    Ok(giornale.nome)
}

/// L'esito della negoziazione del giornale.
struct Giornale {
    /// Il nome per la riga di avvio. Quattro valori e nessun altro:
    ///
    /// - `wal` — accesa **e provata**: il caso normale, e quello veloce;
    /// - `truncate` — si è ripiegati, e il ripiego ha tenuto. La libreria
    ///   funziona e le scansioni fanno impuntare l'interfaccia;
    /// - `memory` — database in memoria: non c'è nessun giornale su disco;
    /// - `<modo>-non-usabile` — WAL risulta accesa, la sonda dice che non
    ///   funziona, e nemmeno il ripiego è riuscito. È il caso peggiore, ed è
    ///   quello che ha più bisogno di un nome nel diario: prima non ne aveva
    ///   nessuno, e si manifestava come un errore per query mezz'ora dopo.
    nome: String,
    /// WAL non è usabile, in un modo o nell'altro.
    ripiegato: bool,
}

/// Il giornale: WAL se si può, `TRUNCATE` se no, e quel che c'è se nemmeno il
/// ripiego riesce.
///
/// # La regola sta nel tipo, non nel commento
///
/// **Non restituisce un `Result`, e non è una dimenticanza.** Rifiutare
/// l'apertura lascerebbe senza libreria chi oggi funziona a metà, e la
/// negoziazione del giornale è precisamente la parte che non ha il diritto di
/// farlo: un database su cui non si riesce a cambiare il giornale è un database
/// che si legge e si scrive comunque, solo più lentamente o con meno garanzie.
/// Togliere il `Result` rende la regola una proprietà della firma, che nessuna
/// modifica distratta può violare senza cambiarla in faccia.
///
/// Il prezzo, dichiarato: gli errori che qui si incontrano **si buttano**. Non è
/// gratis, e i due canali che li sostituiscono sono entrambi migliori di un
/// `Err` che impedisce l'avvio — il nome che finisce nella riga di avvio
/// ([`Giornale::nome`]) e il `ripiegato` che cambia come si raccontano tutti gli
/// errori di I/O successivi ([`codice_da_sqlite`]).
fn negozia_il_giornale(connection: &Connection) -> Giornale {
    // WAL: chi legge non blocca chi scrive. Serve perché la scansione scrive a
    // lungo mentre l'interfaccia continua a leggere la libreria — senza, l'app
    // si impunta per tutta la durata di una riscansione.
    // Restituisce una riga, quindi non si può usare `pragma_update`.
    //
    // L'esito **non si butta più**, ed è la correzione di una riga che c'era e
    // non guardava: su un percorso di rete questa pragma non accende WAL, e
    // fingere che l'avesse fatto era il primo passo verso il `disk I/O error`
    // raccontato come «riavvia Rymembra».
    let risposta = connection
        .query_row("PRAGMA journal_mode = WAL", [], |row| {
            row.get::<_, String>(0)
        })
        .ok()
        .filter(|modo| giornale_accettabile(modo));

    // `memory`: un database senza file non ha un giornale su disco, non c'è
    // niente da sondare e niente su cui ripiegare. Esce di qui prima della
    // sonda, che su di lui non risponderebbe a nessuna domanda.
    if let Some(in_memoria) = risposta
        .as_deref()
        .filter(|modo| !modo.eq_ignore_ascii_case("wal"))
    {
        return Giornale {
            nome: in_memoria.to_ascii_lowercase(),
            ripiegato: false,
        };
    }

    if risposta.is_some() && wal_funziona(connection) {
        // In WAL `NORMAL` non attende il flush del disco a ogni commit, e la
        // garanzia che resta è quella che serve: una transazione committata
        // sopravvive al crash del processo, e solo un crash del **sistema** può
        // portarsi via le ultime. Il dato in gioco è un indice ricostruibile con
        // una scansione, non il file musicale.
        //
        // Fuori da WAL la stessa riga non vorrebbe dire la stessa cosa — ed è il
        // ramo che sta in `ripiega_il_giornale`.
        //
        // Se anche questa non passa non si rifiuta niente: il default di SQLite
        // è `FULL`, cioè più prudente di quel che si stava chiedendo, non meno.
        let _ = connection.pragma_update(None, "synchronous", "NORMAL");
        return Giornale {
            nome: "wal".to_owned(),
            ripiegato: false,
        };
    }

    // Da qui in giù WAL non è usabile, e `ripiegato` vale `true` in entrambe le
    // uscite: è l'unica cosa che conta per come si racconteranno gli errori.
    Giornale {
        nome: ripiega_il_giornale(connection)
            .unwrap_or_else(|| format!("{}-non-usabile", giornale_in_forza(connection))),
        ripiegato: true,
    }
}

/// WAL funziona davvero, o si è soltanto accesa?
///
/// # Perché la pragma non basta, e questa sonda esiste
///
/// Perché `PRAGMA journal_mode = WAL` su una condivisione di rete **risponde
/// spesso `wal`** e lascia il guasto per dopo: il file di memoria condivisa
/// (`-shm`) serve alla **prima transazione**, non alla pragma, e su SMB è lì che
/// non si crea. Senza questa sonda il ripiego sarebbe codice morto proprio sulle
/// macchine per cui è stato scritto: l'utente avrebbe il messaggio giusto — i
/// codici estesi `SQLITE_IOERR_SHM*` ci arrivano da soli — ma mezz'ora dopo, in
/// mezzo a una scansione, e senza nessuna mitigazione. Cioè saprebbe la causa e
/// continuerebbe a subirla.
///
/// # `BEGIN IMMEDIATE`, e cosa costa
///
/// È il gesto più economico che obbliga SQLite a prendere il lucchetto di
/// scrittura, cioè a leggere e scrivere l'indice del WAL: una transazione vuota,
/// che non tocca nessuna riga e non scrive niente nel giornale. **5,5
/// microsecondi** misurati su disco locale, contro i 22 millisecondi di
/// un'apertura intera con le migrazioni: un quattromillesimo dell'avvio.
///
/// # Un database occupato non è un database che non sa fare WAL
///
/// `SQLITE_BUSY` e `SQLITE_LOCKED` valgono **sonda passata**, ed è la parte da
/// non sbagliare: un altro scrittore è esattamente il caso che WAL esiste per
/// permettere. Trattarli come guasti farebbe ripiegare su `TRUNCATE` una
/// libreria locale perfettamente sana solo perché due istanze si sono avviate
/// nello stesso momento — cioè farebbe diventare questa sonda la causa del
/// problema che cerca.
fn wal_funziona(connection: &Connection) -> bool {
    let Err(err) = connection.execute_batch("BEGIN IMMEDIATE; COMMIT;") else {
        return true;
    };
    // Se `BEGIN` è passato e `COMMIT` no, la transazione resterebbe aperta per
    // tutta la vita della connessione e ogni scrittura dopo di lei finirebbe
    // dentro. Il `ROLLBACK` può non avere niente da chiudere, e allora non
    // importa: è la stessa regola già scritta in `migrate`.
    let _ = connection.execute_batch("ROLLBACK");
    matches!(
        err.sqlite_error_code(),
        Some(CodiceSqlite::DatabaseBusy | CodiceSqlite::DatabaseLocked)
    )
}

/// Il giornale che la pragma ha risposto va bene così com'è?
///
/// Due valori, e il secondo è il motivo per cui questa funzione esiste invece
/// di un `!= "wal"`:
///
/// - `wal` è quel che si era chiesto;
/// - `memory` è quel che un database **in memoria** risponde, e non è un
///   ripiego: un database senza file non ha un giornale su disco, `memory` è
///   l'unica risposta possibile e non si può cambiare. Trattarlo come un guasto
///   vorrebbe dire che ogni prova di questa cassa — e sono centinaia — aprirebbe
///   il suo database in memoria annotando un ripiego che non è avvenuto, e con
///   lui il sospetto di rete che fa scegliere [`ErrorCode::DbNetworkPath`].
///
/// Tutto il resto (`delete`, `truncate`, `persist`, `off`) vuol dire che WAL non
/// si è accesa, ed è il caso da ripiego.
fn giornale_accettabile(modo: &str) -> bool {
    modo.eq_ignore_ascii_case("wal") || modo.eq_ignore_ascii_case("memory")
}

/// Il ripiego: `TRUNCATE` e `synchronous = FULL`. `None` se non c'è stato verso.
///
/// `TRUNCATE` e non `DELETE` perché non cancella e ricrea il file di giornale a
/// ogni transazione, lo azzera: un giro di metadati in meno per commit, che su
/// una condivisione di rete è il giro che costa.
///
/// `FULL` e non `NORMAL` perché l'argomento di `NORMAL` **vale dentro WAL**, e
/// qui dentro WAL non ci siamo più. Con un giornale di rollback `NORMAL`
/// significa che una transazione committata può non essere sul disco quando il
/// giornale viene azzerato, e se la corrente manca in quella finestra il
/// database non è indietro di qualche ascolto: è **incoerente**. Il costo di
/// `FULL` è un `fsync` per commit, che è caro; il costo di sbagliare è il file
/// che questa funzione esiste per salvare.
///
/// # Perché `Option` e non `Result`
///
/// Perché il ripiego è l'ultima spiaggia, e l'unica cosa che chi chiama può fare
/// con un errore qui è rinunciare ad aprire la libreria — che è vietato. Uscire
/// senza il giornale chiesto **non è** uscire senza database: il file resta
/// leggibile e scrivibile con il giornale che ha. Il fatto che il ripiego non
/// sia riuscito non si perde, e si legge in due posti: nel nome
/// `<modo>-non-usabile` della riga di avvio, e nel `ripiegato` del contesto, che
/// da quel momento fa raccontare ogni guasto di I/O come un guasto del posto
/// dove stanno i dati.
///
/// Il `synchronous` che non passa **non** fa fallire il ripiego: un giornale
/// ripiegato con il `synchronous` di serie — che è `FULL`, cioè quel che si
/// stava chiedendo — è esattamente ciò che si voleva.
fn ripiega_il_giornale(connection: &Connection) -> Option<String> {
    let giornale = connection
        .query_row("PRAGMA journal_mode = TRUNCATE", [], |row| {
            row.get::<_, String>(0)
        })
        .ok()?;
    // Il ripiego ha senso solo se ha davvero preso: su un file che non si lascia
    // cambiare, `journal_mode` risponde il modo che resta in forza, e chiamarlo
    // ripiego sarebbe una bugia nella riga di avvio.
    if !giornale.eq_ignore_ascii_case("truncate") {
        return None;
    }
    let _ = connection.pragma_update(None, "synchronous", "FULL");
    Some(giornale.to_ascii_lowercase())
}

/// Il giornale in forza adesso, per poterlo nominare nel diario.
///
/// `ignoto` quando nemmeno la lettura passa: un nome finto sarebbe peggio di
/// un'ammissione.
fn giornale_in_forza(connection: &Connection) -> String {
    connection
        .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
        .map(|modo| modo.to_ascii_lowercase())
        .unwrap_or_else(|_| "ignoto".to_owned())
}

/// FTS5 è compilato dentro questo SQLite?
///
/// Si prova a creare una tabella temporanea invece di leggere le opzioni di
/// compilazione: è la stessa cosa che farà la migrazione, quindi risponde alla
/// domanda che conta davvero.
fn has_fts5(connection: &Connection) -> bool {
    connection
        .execute_batch(
            "CREATE VIRTUAL TABLE temp.__fts5_probe USING fts5(x); DROP TABLE temp.__fts5_probe;",
        )
        .is_ok()
}

fn user_version(connection: &Connection) -> Result<u32, AppError> {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_err(|err| codice_da_sqlite("lettura di user_version", &err))
        .map(|value| u32::try_from(value).unwrap_or(0))
}

/// Applica le migrazioni mancanti, ognuna nella sua transazione.
///
/// Una transazione per migrazione e non una per tutte: se la terza fallisce, le
/// prime due restano applicate e `user_version` lo dice. Il contrario — tutto
/// in un'unica transazione — sembra più sicuro ma trasforma un aggiornamento
/// parziale in un aggiornamento che non parte mai, perché al riavvio ritenta
/// dalla prima e ri-fallisce sulla terza.
fn migrate(connection: &Connection) -> Result<usize, AppError> {
    let current = user_version(connection)?;

    if current > LATEST_VERSION {
        return Err(AppError::new(ErrorCode::DbVersionAhead {
            db_version: current,
            app_version: LATEST_VERSION,
        })
        .with_message(
            "il database è stato scritto da una versione più nuova di Rymembra: \
             ripristina un backup invece di aprirlo con questa",
        ));
    }

    let mut applied = 0;
    for migration in MIGRATIONS.iter().filter(|m| m.version > current) {
        let fallita = |err: &rusqlite::Error| {
            db_error(
                ErrorCode::DbMigrationFailed {
                    from: current,
                    to: migration.version,
                    step: Some(migration.name.to_owned()),
                },
                err,
            )
        };

        // Le chiavi esterne si spengono **prima** della transazione, perché è
        // l'unico momento in cui SQLite ascolta: dentro una transazione quel
        // pragma non fa niente e non lo dice. Vedi [`Migration::ricostruisce`]
        // per cosa succederebbe senza.
        if migration.ricostruisce {
            connection
                .pragma_update(None, "foreign_keys", "OFF")
                .map_err(|err| fallita(&err))?;
        }

        // Da qui in poi ogni uscita deve riaccenderle, compresa quella per
        // errore: una connessione che prosegue con le chiavi esterne spente
        // lascerebbe scrivere righe orfane per tutta la sessione.
        //
        // Sulle uscite per errore il ritorno si scarta, ed è giusto: si sta
        // già tornando un errore, e la libreria non si aprirà. Sull'uscita
        // buona no — vedi sotto.
        let riaccendi = |connection: &Connection| {
            if migration.ricostruisce {
                let _ = connection.pragma_update(None, "foreign_keys", "ON");
            }
        };

        if let Err(err) = connection.execute_batch("BEGIN") {
            riaccendi(connection);
            return Err(fallita(&err));
        }

        let outcome = connection
            .execute_batch(migration.sql)
            .and_then(|()| verifica_le_chiavi(connection, migration.ricostruisce))
            .and_then(|()| {
                connection.execute_batch(&format!("PRAGMA user_version = {}", migration.version))
            });

        match outcome {
            Ok(()) => {
                if let Err(err) = connection.execute_batch("COMMIT") {
                    // Un `COMMIT` che fallisce — `SQLITE_BUSY`, un disco che
                    // non scrive — lascia la transazione aperta, e dentro una
                    // transazione `PRAGMA foreign_keys` non fa niente: senza
                    // questo `ROLLBACK` anche la riga dopo sarebbe un gesto a
                    // vuoto. Come sotto, l'errore da dire resta il primo.
                    let _ = connection.execute_batch("ROLLBACK");
                    riaccendi(connection);
                    return Err(fallita(&err));
                }
                riaccendi(connection);
                // E si **verifica** che siano tornate accese, invece di
                // sperarlo. Il commento tre righe sopra dice cosa costerebbe
                // sbagliarsi — righe orfane per tutta la sessione — e una
                // promessa di quel peso scritta come `let _ = …` è una
                // promessa che nessuno mantiene se un giorno smette di valere.
                // Qui la migrazione è andata bene: se l'unica cosa che resta
                // da fare fallisce, è meglio non aprire la libreria che
                // aprirla senza rete.
                if migration.ricostruisce {
                    let accese: i64 = connection
                        .query_row("PRAGMA foreign_keys", [], |riga| riga.get(0))
                        .map_err(|err| fallita(&err))?;
                    if accese != 1 {
                        return Err(db_error(
                            ErrorCode::DbMigrationFailed {
                                from: current,
                                to: migration.version,
                                step: Some(migration.name.to_owned()),
                            },
                            &rusqlite::Error::SqliteFailure(
                                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_MISUSE),
                                Some(
                                    "le chiavi esterne non si sono riaccese dopo la ricostruzione"
                                        .to_owned(),
                                ),
                            ),
                        ));
                    }
                }
                applied += 1;
            }
            Err(err) => {
                // Il rollback può fallire a sua volta (connessione morta): in
                // quel caso l'errore da riportare resta il primo, che è quello
                // che dice cosa è andato storto davvero.
                let _ = connection.execute_batch("ROLLBACK");
                riaccendi(connection);
                return Err(fallita(&err));
            }
        }
    }

    Ok(applied)
}

/// Dopo una ricostruzione: è rimasto qualcosa a puntare nel vuoto?
///
/// `PRAGMA foreign_key_check` costa una scansione delle tabelle con chiavi
/// esterne, quindi si paga solo quando c'è qualcosa da controllare. È la rete
/// sotto il trapezio della migrazione 26: con `foreign_keys` spento SQLite
/// accetta in silenzio un travaso che lasci righe figlie senza genitore, e
/// senza questa verifica il danno si scoprirebbe mesi dopo, come una playlist
/// che contiene brani che non esistono.
///
/// Restituisce un errore di SQLite e non un [`AppError`] perché si incatena
/// dentro l'`and_then` di [`migrate`], dove il rollback è già previsto: un tipo
/// diverso qui vorrebbe dire duplicare quel rollback.
fn verifica_le_chiavi(connection: &Connection, serve: bool) -> Result<(), rusqlite::Error> {
    if !serve {
        return Ok(());
    }
    let rotte = connection
        .prepare("PRAGMA foreign_key_check")
        .and_then(|mut statement| statement.exists([]))?;
    if rotte {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY),
            Some(
                "la ricostruzione ha lasciato righe che puntano a un brano che non c'è più"
                    .to_owned(),
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rymembra_domain::errors::ErrorCodeKind;
    use std::time::Duration;

    #[test]
    fn un_database_nuovo_arriva_alla_versione_corrente() {
        let db = open_in_memory().expect("apertura in memoria");
        assert_eq!(db.version_before, 0);
        assert_eq!(db.applied, MIGRATIONS.len());
        assert_eq!(
            user_version(&db.connection).expect("versione"),
            LATEST_VERSION
        );
    }

    /// Un errore di SQLite costruito a mano, dal suo codice di risultato.
    ///
    /// Serve perché i casi che contano — il database occupato, la share che non
    /// risponde, il file malformato — non si producono a comando su una macchina
    /// sana, e nessuna prova di questa cassa tocca la rete. Quel che si prova è
    /// la **traduzione**, che è dove stava il difetto.
    fn guasto(codice: std::ffi::c_int) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(codice), None)
    }

    /// Il contesto di un database locale e sano.
    fn contesto_locale() -> Contesto {
        Contesto {
            percorso: Some(r"C:\Users\tizio\AppData\Roaming\rymembra\rymembra.db".into()),
            di_rete: false,
            ripiegato: false,
        }
    }

    #[test]
    fn codice_da_sqlite_distingue_locked_da_query_failed() {
        // Il difetto, nella sua forma più breve: questi due arrivavano
        // all'utente come la stessa frase, con lo stesso consiglio di riavviare.
        // Uno passa da sé in mezzo secondo, l'altro è un difetto di Rymembra.
        let locale = contesto_locale();

        for occupato in [rusqlite::ffi::SQLITE_BUSY, rusqlite::ffi::SQLITE_LOCKED] {
            let errore = codice_con_contesto("lettura dei brani", &guasto(occupato), &locale);
            assert_eq!(errore.code().kind(), ErrorCodeKind::DbLocked);
            assert!(
                errore.is_retryable(),
                "un database occupato si riprova: è il senso del codice"
            );
        }

        // Il ripiego resta il ripiego, e si porta dietro il suo `detail`: senza,
        // il diario direbbe che «una query è fallita» senza dire quale.
        let sconosciuto = codice_con_contesto(
            "lettura dei brani",
            &guasto(rusqlite::ffi::SQLITE_CONSTRAINT),
            &locale,
        );
        assert_eq!(sconosciuto.code().kind(), ErrorCodeKind::DbQueryFailed);
        assert!(!sconosciuto.is_retryable());

        // La sola lettura non è un database occupato: aspettare non cambia i
        // permessi di una cartella.
        let sola_lettura = codice_con_contesto(
            "scrittura di un voto",
            &guasto(rusqlite::ffi::SQLITE_READONLY),
            &locale,
        );
        assert_eq!(sola_lettura.code().kind(), ErrorCodeKind::DbReadOnly);
        assert!(!sola_lettura.is_retryable());

        // E un errore che non viene da SQLite — rusqlite che si lamenta di noi —
        // resta quel che era: è un difetto di Rymembra, non una cosa da raccontare.
        let nostro = codice_con_contesto(
            "lettura di una colonna",
            &rusqlite::Error::InvalidColumnIndex(7),
            &locale,
        );
        assert_eq!(nostro.code().kind(), ErrorCodeKind::DbQueryFailed);
    }

    #[test]
    fn il_file_che_non_si_apre_non_e_un_difetto_di_rymembra() {
        let locale = contesto_locale();
        let di_rete = Contesto {
            percorso: Some(r"\\nas\casa\rymembra\rymembra.db".into()),
            di_rete: true,
            ripiegato: false,
        };
        let cantopen = guasto(rusqlite::ffi::SQLITE_CANTOPEN);
        assert_eq!(
            codice_con_contesto("apertura", &cantopen, &di_rete)
                .code()
                .kind(),
            ErrorCodeKind::DbNetworkPath
        );
        assert_eq!(
            codice_con_contesto("apertura", &cantopen, &locale)
                .code()
                .kind(),
            ErrorCodeKind::DbIoFailed
        );

        let protocollo = guasto(rusqlite::ffi::SQLITE_PROTOCOL);
        assert_eq!(
            codice_con_contesto("scrittura", &protocollo, &di_rete)
                .code()
                .kind(),
            ErrorCodeKind::DbNetworkPath
        );
        let corsa = codice_con_contesto("scrittura", &protocollo, &locale);
        assert_eq!(corsa.code().kind(), ErrorCodeKind::DbLocked);
        assert!(corsa.is_retryable());

        // Il lucchetto a intervallo di byte che il filesystem non sa dare vale
        // da solo, come la memoria condivisa: una lettera mappata non si
        // riconosce dalla forma.
        let lucchetto = guasto(rusqlite::ffi::SQLITE_IOERR_LOCK);
        assert_eq!(
            codice_con_contesto("scrittura", &lucchetto, &locale)
                .code()
                .kind(),
            ErrorCodeKind::DbNetworkPath
        );
    }

    #[test]
    fn codice_da_sqlite_riconosce_il_corrotto() {
        let locale = contesto_locale();
        for malformato in [rusqlite::ffi::SQLITE_CORRUPT, rusqlite::ffi::SQLITE_NOTADB] {
            let errore = codice_con_contesto("lettura degli album", &guasto(malformato), &locale);
            assert_eq!(errore.code().kind(), ErrorCodeKind::DbCorrupt);
            // `Fatal` apposta: continuare a scrivere su un file malformato
            // trasforma un backup ancora buono in due file rotti.
            assert_eq!(errore.severity(), rymembra_domain::errors::Severity::Fatal);
            assert!(!errore.is_retryable());
        }
        // Il percorso del file viaggia con l'errore, perché è la prima cosa che
        // serve a chi deve ripristinare un backup.
        let errore = codice_con_contesto(
            "lettura degli album",
            &guasto(rusqlite::ffi::SQLITE_CORRUPT),
            &locale,
        );
        assert!(
            errore
                .code()
                .i18n_key()
                .eq_ignore_ascii_case("errors.db.corrupt")
        );
        assert!(
            errore.cause().is_some(),
            "il testo di rusqlite non si perde"
        );
    }

    #[test]
    fn un_guasto_di_io_dice_la_rete_solo_quando_ce_ne_e_motivo() {
        // Lo stesso `SQLITE_IOERR` vuole due frasi diverse: «sposta la cartella
        // dati» e «guarda il disco». Mandare l'utente nel posto sbagliato costa
        // un pomeriggio, e fino alla 2.3.1 li mandava entrambi su «riavvia».
        let locale = contesto_locale();
        let ioerr = guasto(rusqlite::ffi::SQLITE_IOERR);

        let su_disco = codice_con_contesto("scrittura di un ascolto", &ioerr, &locale);
        assert_eq!(su_disco.code().kind(), ErrorCodeKind::DbIoFailed);

        let su_share = codice_con_contesto(
            "scrittura di un ascolto",
            &ioerr,
            &Contesto {
                percorso: Some(r"\\nas\rymembra\rymembra.db".into()),
                di_rete: true,
                ripiegato: false,
            },
        );
        assert_eq!(su_share.code().kind(), ErrorCodeKind::DbNetworkPath);

        // Il caso della lettera mappata: `Z:\` non si riconosce dalla forma, ma
        // il giornale che non si è acceso lo dice al posto suo.
        let lettera_mappata = codice_con_contesto(
            "scrittura di un ascolto",
            &ioerr,
            &Contesto {
                percorso: Some(r"Z:\rymembra\rymembra.db".into()),
                di_rete: false,
                ripiegato: true,
            },
        );
        assert_eq!(lettera_mappata.code().kind(), ErrorCodeKind::DbNetworkPath);

        // E l'indizio più forte dei tre: il codice esteso della memoria
        // condivisa è la firma di WAL su un filesystem che non sa mapparla, e
        // vale da solo anche su un contesto che sembra locale.
        for shm in [
            rusqlite::ffi::SQLITE_IOERR_SHMOPEN,
            rusqlite::ffi::SQLITE_IOERR_SHMSIZE,
            rusqlite::ffi::SQLITE_IOERR_SHMMAP,
        ] {
            let errore = codice_con_contesto("lettura dei brani", &guasto(shm), &locale);
            assert_eq!(
                errore.code().kind(),
                ErrorCodeKind::DbNetworkPath,
                "il codice esteso {shm} è la memoria condivisa di WAL"
            );
        }
    }

    /// Lo stesso errore, con il messaggio che SQLite ci mette davvero.
    ///
    /// È la forma che arriva dal vivo, e quella in cui il codice esteso si
    /// perdeva: con il messaggio dentro, il testo di `rusqlite` è soltanto
    /// «FOREIGN KEY constraint failed», senza il 787.
    fn guasto_con_testo(codice: std::ffi::c_int, testo: &str) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(codice), Some(testo.to_owned()))
    }

    #[test]
    fn i_vincoli_si_distinguono_invece_di_finire_nel_ripiego() {
        let locale = contesto_locale();

        // La chiave esterna: un brano che una scansione ha tolto mentre la
        // finestra ne teneva ancora l'identificativo. Non è un difetto di
        // Rymembra, ed era la riga che diceva che lo fosse.
        let esterna = codice_con_contesto(
            "aggiunta a una playlist",
            &guasto_con_testo(
                rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY,
                "FOREIGN KEY constraint failed",
            ),
            &locale,
        );
        assert_eq!(esterna.code().kind(), ErrorCodeKind::LibraryTrackNotFound);

        // L'unicità, e la chiave primaria che è la stessa cosa: la riga c'è già.
        for doppione in [
            rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE,
            rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY,
        ] {
            let errore = codice_con_contesto("scansione", &guasto(doppione), &locale);
            assert_eq!(errore.code().kind(), ErrorCodeKind::DbDuplicate);
            assert!(errore.is_retryable(), "chi rifà rilegge, e passa");
        }

        // Gli altri vincoli restano nel ripiego: un `NOT NULL` o un `CHECK`
        // violati sono una riga scritta male, cioè davvero un difetto nostro.
        for di_forma in [
            rusqlite::ffi::SQLITE_CONSTRAINT_NOTNULL,
            rusqlite::ffi::SQLITE_CONSTRAINT_CHECK,
        ] {
            let errore = codice_con_contesto("scrittura", &guasto(di_forma), &locale);
            assert_eq!(errore.code().kind(), ErrorCodeKind::DbQueryFailed);
        }
    }

    #[test]
    fn disco_pieno_e_permessi_parlano_come_il_disco() {
        let locale = contesto_locale();

        let pieno = codice_con_contesto(
            "scrittura di un ascolto",
            &guasto(rusqlite::ffi::SQLITE_FULL),
            &locale,
        );
        assert_eq!(pieno.code().kind(), ErrorCodeKind::FsDiskFull);
        // Il percorso del database viaggia con l'errore: è il disco da liberare.
        assert!(matches!(
            pieno.code(),
            ErrorCode::FsDiskFull { path: Some(p) } if p.ends_with("rymembra.db")
        ));

        for negato in [rusqlite::ffi::SQLITE_PERM, rusqlite::ffi::SQLITE_AUTH] {
            let errore = codice_con_contesto("scrittura", &guasto(negato), &locale);
            assert_eq!(errore.code().kind(), ErrorCodeKind::FsPermissionDenied);
        }

        // In memoria il percorso non c'è, e la traduzione non deve cadere per
        // questo: il codice resta quello, con un percorso vuoto.
        let senza_file = codice_con_contesto(
            "scrittura",
            &guasto(rusqlite::ffi::SQLITE_PERM),
            &Contesto {
                percorso: None,
                di_rete: false,
                ripiegato: false,
            },
        );
        assert_eq!(senza_file.code().kind(), ErrorCodeKind::FsPermissionDenied);
    }

    #[test]
    fn la_causa_porta_l_operazione_e_il_codice_esteso() {
        // Il motivo per cui la prossima segnalazione si potrà leggere: nel
        // diario finisce la causa, e la causa deve dire quale operazione, cosa
        // ha detto SQLite e **quale** vincolo. Prima arrivava soltanto
        // «constraint failed».
        let errore = codice_con_contesto(
            "riscrittura di una playlist",
            &guasto_con_testo(
                rusqlite::ffi::SQLITE_CONSTRAINT_NOTNULL,
                "NOT NULL constraint failed: playlist_tracks.track_id",
            ),
            &contesto_locale(),
        );
        assert_eq!(errore.code().kind(), ErrorCodeKind::DbQueryFailed);
        let causa = errore.cause().expect("una causa c'è sempre");
        assert!(causa.contains("riscrittura di una playlist"), "{causa}");
        assert!(causa.contains("playlist_tracks.track_id"), "{causa}");
        assert!(causa.contains("1299"), "{causa}");
        assert!(causa.contains("SQLITE_CONSTRAINT_NOTNULL"), "{causa}");

        // Un codice esteso senza nome resta un numero, che è meglio di niente.
        let ignoto = codice_con_contesto(
            "lettura",
            &guasto(rusqlite::ffi::SQLITE_ERROR),
            &contesto_locale(),
        );
        assert!(
            ignoto
                .cause()
                .is_some_and(|c| c.starts_with("lettura: ") && c.contains("[sqlite 1]")),
            "{:?}",
            ignoto.cause()
        );

        // E l'errore che non viene da SQLite non inventa un codice che non ha.
        let nostro = codice_con_contesto(
            "lettura di una colonna",
            &rusqlite::Error::InvalidColumnIndex(7),
            &contesto_locale(),
        );
        assert!(
            nostro.cause().is_some_and(
                |c| c.starts_with("lettura di una colonna: ") && !c.contains("[sqlite")
            ),
            "{:?}",
            nostro.cause()
        );
    }

    #[test]
    fn il_giornale_ripiega_quando_wal_non_si_accende() {
        // La decisione, che è la parte che può sbagliare. `memory` non è un
        // ripiego: è l'unica risposta che un database in memoria può dare, e
        // trattarla come un guasto annoterebbe un sospetto di rete in ognuna
        // delle centinaia di prove di questa cassa.
        assert!(giornale_accettabile("wal"));
        assert!(
            giornale_accettabile("WAL"),
            "la pragma non promette il caso"
        );
        assert!(giornale_accettabile("memory"));
        for ripiego in ["delete", "truncate", "persist", "off"] {
            assert!(
                !giornale_accettabile(ripiego),
                "«{ripiego}» vuol dire che WAL non si è accesa"
            );
        }

        // E il ripiego vero, su un database vero: `TRUNCATE` in forza e
        // `synchronous = FULL`, che è la metà che nessuno nota se manca —
        // `NORMAL` fuori da WAL lascia una finestra in cui una transazione
        // committata non è sul disco mentre il giornale viene azzerato.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let aperto = open(&dir.path().join("rymembra.db")).expect("apertura");
        assert_eq!(aperto.giornale, "wal", "su un disco locale WAL si accende");

        let giornale = ripiega_il_giornale(&aperto.connection).expect("ripiego");
        assert_eq!(giornale, "truncate");
        let in_forza: String = aperto
            .connection
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .expect("lettura del giornale");
        assert!(in_forza.eq_ignore_ascii_case("truncate"));
        let sincrono: i64 = aperto
            .connection
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .expect("lettura di synchronous");
        assert_eq!(sincrono, 2, "FULL vale 2: NORMAL sarebbe 1");

        // Un database in memoria si dichiara per quel che è, e non finge WAL.
        let in_memoria = open_in_memory().expect("apertura in memoria");
        assert_eq!(in_memoria.giornale, "memory");
    }

    #[test]
    fn la_sonda_vede_la_differenza_fra_un_wal_sano_e_un_occupato() {
        // Il caso normale: WAL accesa, sonda passata, e nessun ripiego annotato.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let aperto = open(&dir.path().join("rymembra.db")).expect("apertura");
        assert!(
            wal_funziona(&aperto.connection),
            "su un disco locale la sonda deve passare"
        );

        // E il caso da non sbagliare: un database **occupato** vale sonda
        // passata. Una transazione di scrittura già aperta su un'altra
        // connessione è esattamente lo scenario che WAL esiste per permettere, e
        // se la sonda lo leggesse come un guasto farebbe ripiegare su `TRUNCATE`
        // una libreria locale sana solo perché due istanze si sono avviate
        // insieme — diventando lei la causa del problema che cerca.
        //
        // `busy_timeout` a zero per non aspettare cinque secondi dentro una
        // prova: quel che si sta provando è la classificazione dell'errore, non
        // la pazienza.
        let percorso = dir.path().join("rymembra.db");
        let seconda = Connection::open(&percorso).expect("seconda connessione");
        seconda.busy_timeout(Duration::ZERO).expect("senza attesa");
        aperto
            .connection
            .execute_batch("BEGIN IMMEDIATE")
            .expect("uno scrittore che tiene il lucchetto");
        assert!(
            wal_funziona(&seconda),
            "un database occupato non è un database che non sa fare WAL"
        );
        aperto
            .connection
            .execute_batch("COMMIT")
            .expect("lo scrittore finisce");
    }

    #[test]
    fn la_sonda_di_wal_non_costa_niente() {
        // La sonda gira a **ogni** apertura, quindi il suo costo è un costo
        // d'avvio. Misurata: 5,5 microsecondi su disco locale, contro i 22
        // millisecondi di un'apertura intera con le migrazioni.
        //
        // Il tetto è volutamente larghissimo — mezzo secondo per mille sonde,
        // cioè cento volte il misurato — perché il numero da difendere non è
        // «cinque microsecondi», è «non fa un `fsync`». Il giorno in cui qualcuno
        // cambiasse la sonda in qualcosa che tocca il disco per davvero, mille
        // giri ci metterebbero dei secondi e questa riga lo direbbe.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let aperto = open(&dir.path().join("rymembra.db")).expect("apertura");
        let prima = std::time::Instant::now();
        for _ in 0..1000 {
            assert!(wal_funziona(&aperto.connection));
        }
        let passato = prima.elapsed();
        assert!(
            passato < Duration::from_millis(500),
            "mille sonde hanno richiesto {passato:?}: la sonda ha smesso di essere gratis"
        );
    }

    #[test]
    fn il_doppio_guasto_non_fa_fallire_l_apertura() {
        // **La regola che questa prova difende**: la negoziazione del giornale non
        // ha il diritto di impedire l'apertura. Rifiutare lascerebbe senza
        // libreria chi oggi funziona a metà, e il caso peggiore è quello in cui
        // WAL non si accende **e** nemmeno il ripiego riesce.
        //
        // Si riproduce senza toccare la rete, con il file che rende impossibili
        // tutte e tre le pragma: quattro kibibyte di spazzatura al posto del
        // database. `Connection::open` passa — SQLite legge l'intestazione al
        // primo comando, non all'apertura — e da lì in avanti ogni pragma del
        // giornale risponde `SQLITE_NOTADB`.
        //
        // Non è un caso di scuola: è il file dati di qualcuno finito a metà di una
        // copia, o troncato da un disco pieno. E quel che deve succedere è
        // esattamente questo — `configure` non si rifiuta, perché non è lui che ha
        // il diritto di dichiarare morta una libreria: quel verdetto arriva un
        // passo più in là, da `user_version`, e adesso arriva come `db.corrupt`
        // invece che come «riavvia Rymembra».
        //
        // *(Lungo la strada si è provato con una connessione di sola lettura, e
        // non serve: su un database in WAL la sonda **passa** — il lucchetto di
        // scrittura vive nel file di memoria condivisa, che è scrivibile, e una
        // transazione vuota non tocca il database — e su un database fuori da WAL
        // il ripiego **riesce**, perché fra due giornali di rollback il cambio è
        // una faccenda di connessione e non di file. Entrambi sono il
        // comportamento giusto, e nessuno dei due è un doppio guasto.)*
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let percorso = dir.path().join("rymembra.db");
        std::fs::write(&percorso, vec![0x5a_u8; 4096]).expect("spazzatura al posto del database");

        let spazzatura = Connection::open(&percorso).expect("l'apertura è pigra, e passa");

        let giornale = negozia_il_giornale(&spazzatura);
        assert!(
            giornale.ripiegato,
            "WAL non è usabile qui, e va annotato: è l'indizio che fa \
             raccontare i guasti di I/O come guasti del posto dove stanno i dati"
        );
        assert_eq!(
            giornale.nome, "ignoto-non-usabile",
            "il caso peggiore ha bisogno di un nome nel diario: nemmeno la \
             lettura del giornale passa, e «ignoto» è più onesto di un nome finto"
        );

        // E la regola, detta per intero: `configure` **torna `Ok`**. Non c'è ramo
        // di `negozia_il_giornale` che possa restituire un errore — non ha un
        // `Result` — e questa riga è ciò che se ne accorge se qualcuno glielo
        // rimettesse passando per `configure`.
        let nome = configure(&spazzatura).expect("l'apertura non deve fallire");
        assert_eq!(nome, "ignoto-non-usabile");

        // E il verdetto vero arriva dove gli tocca, col nome giusto: è la
        // controprova che non rifiutarsi qui non vuol dire far finta di niente.
        let errore = open(&percorso).expect_err("un file che non è un database");
        assert_eq!(errore.code().kind(), ErrorCodeKind::DbCorrupt);

        // Il contesto si rimette com'era. È lo stato di tutto il processo, e qui
        // dentro l'hanno scritto due volte — `configure` e l'`open` della riga
        // sopra — con il percorso di un file temporaneo che sta per sparire.
        // Lasciare in giro un segno che descrive un database che non c'è più
        // sarebbe una bugia in attesa di qualcuno che la creda.
        *CONTESTO.lock().unwrap_or_else(PoisonError::into_inner) = Contesto {
            percorso: None,
            di_rete: false,
            ripiegato: false,
        };
    }

    #[test]
    fn fts5_c_e_su_questa_piattaforma() {
        // È il motivo per cui SQLite è compilato dentro il binario invece di
        // usare quello di sistema. Se questo test cade su Android, la ricerca
        // dovrebbe ripiegare su LIKE — cioè due comportamenti diversi, che è
        // ciò che si sta eliminando.
        let db = open_in_memory().expect("apertura");
        assert!(db.fts5, "FTS5 deve essere disponibile");
    }

    #[test]
    fn sqlite_non_scende_sotto_la_3_53_2() {
        // CVE-2026-11824: un traboccamento di memoria dentro FTS5, chiuso nella
        // 3.53.2. Rymembra apre database che non ha scritto lui — quello della
        // versione rilasciata, in `import_legacy` — e FTS5 lo usa per la
        // ricerca: una `rusqlite` tornata indietro per sbaglio riaprirebbe il
        // buco senza che nessun'altra prova se ne accorga. Si chiede a SQLite e
        // non al manifesto, perché conta quello che è finito nel binario.
        let connessione = Connection::open_in_memory().expect("apertura");
        let dichiarata: String = connessione
            .query_row("SELECT sqlite_version()", [], |riga| riga.get(0))
            .expect("versione");
        assert!(
            rusqlite::version_number() >= 3_053_002,
            "SQLite {dichiarata} è sotto la 3.53.2"
        );
    }

    #[test]
    fn riaprire_non_riapplica_niente() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let path = dir.path().join("rymembra.db");

        let prima = open(&path).expect("prima apertura");
        assert_eq!(prima.applied, MIGRATIONS.len());
        drop(prima);

        let seconda = open(&path).expect("seconda apertura");
        assert_eq!(seconda.version_before, LATEST_VERSION);
        assert_eq!(seconda.applied, 0, "una riapertura non deve migrare");
    }

    #[test]
    fn un_database_dal_futuro_non_si_apre() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let path = dir.path().join("rymembra.db");
        {
            let db = open(&path).expect("apertura");
            db.connection
                .execute_batch(&format!("PRAGMA user_version = {}", LATEST_VERSION + 5))
                .expect("finge una versione futura");
        }

        let errore = open(&path).expect_err("deve rifiutarsi");
        assert_eq!(errore.code().kind(), ErrorCodeKind::DbVersionAhead);
        assert_eq!(errore.severity(), rymembra_domain::errors::Severity::Fatal);
    }

    #[test]
    fn un_ascolto_nasce_locale_e_puo_essere_solo_una_delle_due_cose() {
        // La colonna esiste per una ragione sola: rendere possibile «dimentica
        // gli ascolti importati», che è una `DELETE` mirata. Se un valore
        // storto potesse entrare, quella `DELETE` mancherebbe delle righe — e
        // ce ne si accorgerebbe solo nel momento in cui si sta cercando di
        // rimediare a qualcosa, cioè nel peggiore.
        let db = open_in_memory().expect("apertura");
        db.connection
            .execute_batch(
                "INSERT INTO tracks (path, track_key, title, artist, album, file_size,
                                     date_added, date_modified)
                 VALUES ('C:/m/a.mp3', 'a|b|c', 'T', 'A', 'Al', 1, 0, 0);
                 INSERT INTO play_history (track_id, played_at, ms_played)
                 VALUES (1, 1000, 120000);",
            )
            .expect("un ascolto vero");

        let source: String = db
            .connection
            .query_row("SELECT source FROM play_history WHERE id = 1", [], |r| {
                r.get(0)
            })
            .expect("lettura");
        assert_eq!(source, "local", "quel che c'era prima resta quel che era");

        db.connection
            .execute(
                "INSERT INTO play_history (track_id, played_at, ms_played, source)
                 VALUES (1, 2000, 120000, 'spotify')",
                [],
            )
            .expect("un ascolto importato");

        db.connection
            .execute(
                "INSERT INTO play_history (track_id, played_at, ms_played, source)
                 VALUES (1, 3000, 120000, 'sptoify')",
                [],
            )
            .expect_err("un refuso non deve poter entrare");
    }

    #[test]
    fn la_coppia_brano_istante_ha_il_suo_indice_e_il_vecchio_se_n_e_andato() {
        // È l'interrogazione che l'importazione di un archivio fa decine di
        // migliaia di volte, una per riga di cronologia. Senza la coppia,
        // ciascuna scorre tutti gli ascolti di quel brano.
        let db = open_in_memory().expect("apertura");
        let indici: Vec<String> = {
            let mut q = db
                .connection
                .prepare(
                    "SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='play_history'",
                )
                .expect("elenco indici");
            q.query_map([], |r| r.get::<_, String>(0))
                .expect("lettura")
                .filter_map(Result::ok)
                .collect()
        };
        assert!(
            indici.iter().any(|n| n == "idx_play_history_brano_quando"),
            "manca l'indice della coppia: {indici:?}"
        );
        assert!(
            !indici.iter().any(|n| n == "idx_play_history_track"),
            "il vecchio indice è un prefisso del nuovo, e mantenerne due \
             costa un albero in più a ogni inserimento: {indici:?}"
        );
    }

    #[test]
    fn una_playlist_ricorda_da_quale_playlist_di_spotify_viene() {
        // Senza, chi rinomina una playlist su Spotify se ne ritrova due qui
        // alla sincronizzazione dopo: `playlist_key` nasce dal nome.
        let db = open_in_memory().expect("apertura");
        db.connection
            .execute_batch(
                "INSERT INTO playlists (playlist_key, name, created_at, updated_at,
                                        source_playlist_id)
                 VALUES ('corsa', 'Corsa', 0, 0, '37i9dQZF1DXcBWIGoYBM5M');
                 -- Due playlist senza identificativo non si pestano i piedi:
                 -- l'indice è parziale apposta.
                 INSERT INTO playlists (playlist_key, name, created_at, updated_at)
                 VALUES ('a', 'A', 0, 0);
                 INSERT INTO playlists (playlist_key, name, created_at, updated_at)
                 VALUES ('b', 'B', 0, 0);",
            )
            .expect("playlist");

        let quante: i64 = db
            .connection
            .query_row(
                "SELECT COUNT(*) FROM playlists WHERE source_playlist_id IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .expect("conteggio");
        assert_eq!(quante, 1);
    }

    #[test]
    fn scollegare_un_account_e_una_delete_non_quattro_stringhe_vuote() {
        // La differenza con `nuvola.*` in `settings`, dove «scollega» scrive
        // stringa vuota in quattro chiavi e le righe restano.
        let db = open_in_memory().expect("apertura");
        db.connection
            .execute_batch(
                "INSERT INTO spotify_account (spotify_user_id, display_name, last_source)
                 VALUES ('tizio', 'Tizio', 'archivio')",
            )
            .expect("account");

        // Ricollegare lo stesso account aggiorna, non aggiunge.
        db.connection
            .execute_batch(
                "INSERT INTO spotify_account (spotify_user_id, display_name, last_source)
                 VALUES ('tizio', 'Tizio', 'api')
                 ON CONFLICT (spotify_user_id) DO UPDATE SET last_source = excluded.last_source",
            )
            .expect("ricollegamento");

        db.connection
            .execute_batch(
                "INSERT INTO spotify_account (spotify_user_id, last_source)
                 VALUES ('caio', 'posta')",
            )
            .expect_err("una provenienza inventata non deve poter entrare");

        assert_eq!(
            db.connection
                .execute("DELETE FROM spotify_account", [])
                .expect("scollegamento"),
            1,
            "una riga sola, e se ne va tutta"
        );
    }

    /// Porta un database vuoto fino alla versione voluta, compresa.
    ///
    /// Serve a una cosa sola: mettere dei dati **prima** di una migrazione, che
    /// è l'unico modo di provare che quella migrazione non se li porta via.
    /// `migrate` applica tutto quel che manca, quindi da sola non lascia
    /// nessuna finestra in cui scrivere.
    fn fino_alla(connection: &Connection, versione: u32) {
        for migrazione in MIGRATIONS.iter().filter(|m| m.version <= versione) {
            connection
                .execute_batch(migrazione.sql)
                .unwrap_or_else(|err| panic!("migrazione {}: {err}", migrazione.version));
        }
        connection
            .execute_batch(&format!("PRAGMA user_version = {versione}"))
            .expect("versione");
    }

    /// Una libreria con dentro le quattro cose che si perderebbero.
    fn libreria_da_perdere(connection: &Connection) {
        connection
            .execute_batch(
                "INSERT INTO tracks (id, path, track_key, title, artist, album,
                                     file_size, date_added, date_modified, rating, play_count)
                 VALUES (1, 'C:/M/a.flac', 'a|b|c', 'A', 'B', 'C', 5000000, 1, 1, 4, 7);
                 INSERT INTO playlists (id, playlist_key, name, created_at, updated_at)
                 VALUES (1, 'p', 'P', 1, 1);
                 INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (1, 1, 0);
                 INSERT INTO play_history (track_id, played_at, ms_played) VALUES (1, 1700, 240000);
                 INSERT INTO track_overrides (track_id, campi, set_at)
                 VALUES (1, '{\"artista\":\"Pink Floyd\"}', 1);",
            )
            .expect("dati di prova");
    }

    /// Quante righe ha una tabella.
    fn quante(connection: &Connection, tabella: &str) -> i64 {
        connection
            .query_row(&format!("SELECT COUNT(*) FROM {tabella}"), [], |r| r.get(0))
            .unwrap_or_else(|err| panic!("conteggio di {tabella}: {err}"))
    }

    /// La migrazione che ricostruisce `tracks` non porta via il resto.
    ///
    /// # Cos'è questa prova
    ///
    /// La riproduzione del disastro che la 26 ha evitato, e che fino a qui era
    /// raccontata nel CHANGELOG e in nessun file eseguibile.
    ///
    /// SQLite non sa togliere un `NOT NULL`: per rendere `tracks.path`
    /// nullabile la 26 costruisce una tabella nuova, travasa, **cancella la
    /// vecchia** e rinomina. Il passo pericoloso è il terzo: dieci tabelle
    /// referenziano `tracks(id)` con `ON DELETE CASCADE`, e con le chiavi
    /// esterne accese `DROP TABLE` esegue una cancellazione implicita che le
    /// fa scattare tutte. Cronologia, playlist, voti e correzioni a mano
    /// sparirebbero in silenzio, dentro una migrazione che riporta successo.
    ///
    /// Per questo `Migration::ricostruisce` esiste e per questo `migrate`
    /// spegne `foreign_keys` **fuori** dalla transazione. Togliendo quel campo
    /// alla voce 26, questa prova diventa rossa: è il suo unico scopo.
    #[test]
    fn la_ricostruzione_della_ventisei_non_porta_via_niente() {
        let connection = Connection::open_in_memory().expect("apertura in memoria");
        // Accese, come le accende `open`. Spente, questa prova non proverebbe
        // niente: la cascata che deve non scattare non scatterebbe comunque.
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .expect("chiavi esterne");

        fino_alla(&connection, 25);
        libreria_da_perdere(&connection);

        let applicate = migrate(&connection).expect("la 26 deve passare");
        // Quante ne mancano dalla 25, non «una»: il numero scritto a mano
        // diventava rosso alla migrazione dopo, e faceva sembrare guasta la
        // ricostruzione per il solo fatto che qualcuno aveva aggiunto una riga
        // in fondo a `MIGRATIONS`.
        assert_eq!(
            u32::try_from(applicate).expect("il conto delle migrazioni sta in un u32"),
            LATEST_VERSION - 25,
            "dalla 25 mancano tutte quelle dopo"
        );
        assert_eq!(user_version(&connection).expect("versione"), LATEST_VERSION);

        // Le quattro cose, una per una.
        assert_eq!(quante(&connection, "tracks"), 1, "il brano");
        assert_eq!(quante(&connection, "playlist_tracks"), 1, "la playlist");
        assert_eq!(quante(&connection, "play_history"), 1, "la cronologia");
        assert_eq!(quante(&connection, "track_overrides"), 1, "le correzioni");

        // E il brano è ancora quello, non un guscio con lo stesso numero.
        let (titolo, voto, ascolti): (String, i64, i64) = connection
            .query_row(
                "SELECT title, rating, play_count FROM tracks WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("il brano");
        assert_eq!((titolo.as_str(), voto, ascolti), ("A", 4, 7));

        // Le chiavi esterne sono tornate accese: una sessione che proseguisse
        // con quelle spente lascerebbe scrivere righe orfane fino alla chiusura.
        let accese: i64 = connection
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .expect("lettura pragma");
        assert_eq!(accese, 1, "riaccese dopo la ricostruzione");

        // E la cascata funziona ancora: la tabella nuova ha davvero le sue
        // chiavi, non solo le sue colonne.
        connection
            .execute("DELETE FROM tracks WHERE id = 1", [])
            .expect("cancella");
        assert_eq!(quante(&connection, "playlist_tracks"), 0);
        assert_eq!(quante(&connection, "play_history"), 0);
        assert_eq!(quante(&connection, "track_overrides"), 0);
    }

    /// La rete sotto il trapezio esiste e regge.
    ///
    /// `verifica_le_chiavi` gira solo per le migrazioni che ricostruiscono, e
    /// una rete che non si prova non si sa se è appesa. Qui si semina una riga
    /// orfana **prima** della 26 — cosa che solo le chiavi esterne spente
    /// permettono — e si pretende che la migrazione si rifiuti di confermare.
    #[test]
    fn una_riga_orfana_ferma_la_ricostruzione() {
        let connection = Connection::open_in_memory().expect("apertura in memoria");
        connection
            .pragma_update(None, "foreign_keys", "OFF")
            .expect("chiavi esterne");
        fino_alla(&connection, 25);
        connection
            .execute_batch(
                "INSERT INTO play_history (track_id, played_at, ms_played)
                 VALUES (999, 1700, 1000);",
            )
            .expect("la riga orfana");
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .expect("chiavi esterne");

        let esito = migrate(&connection);
        assert!(
            esito.is_err(),
            "una ricostruzione che lascia righe nel vuoto non si conferma"
        );
        assert_eq!(
            user_version(&connection).expect("versione"),
            25,
            "e il database resta dov'era"
        );
    }

    #[test]
    fn le_chiavi_esterne_sono_accese() {
        // Spente (il default di SQLite) le righe di playlist_tracks
        // sopravviverebbero al brano, e la playlist mostrerebbe voci fantasma.
        let db = open_in_memory().expect("apertura");
        let on: i64 = db
            .connection
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .expect("lettura pragma");
        assert_eq!(on, 1);
    }

    #[test]
    fn cancellare_un_brano_lo_toglie_dalle_playlist() {
        let db = open_in_memory().expect("apertura");
        let c = &db.connection;
        c.execute_batch(
            "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size, date_added, date_modified)
             VALUES (1, 'C:/M/a.mp3', 'a|b|c', 'A', 'B', 'C', 5000000, 1, 1);
             INSERT INTO playlists (id, playlist_key, name, created_at, updated_at)
             VALUES (1, 'p', 'P', 1, 1);
             INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (1, 1, 0);",
        )
        .expect("dati di prova");

        c.execute("DELETE FROM tracks WHERE id = 1", [])
            .expect("cancella");
        let rimaste: i64 = c
            .query_row("SELECT COUNT(*) FROM playlist_tracks", [], |r| r.get(0))
            .expect("conteggio");
        assert_eq!(rimaste, 0, "la cascata deve aver ripulito");
    }

    #[test]
    fn la_ricerca_ignora_i_diacritici() {
        // Il tokenizzatore piega da sé: cercare senza accenti trova con accenti,
        // senza che nessuno normalizzi a mano da questa parte.
        let db = open_in_memory().expect("apertura");
        let c = &db.connection;
        c.execute(
            "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size, date_added, date_modified)
             VALUES (1, 'C:/M/a.mp3', 'k', 'Jóga', 'Björk', 'Homogenic', 5000000, 1, 1)",
            [],
        )
        .expect("inserimento");

        let trovati: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH 'bjork'",
                [],
                |r| r.get(0),
            )
            .expect("ricerca");
        assert_eq!(trovati, 1, "«bjork» deve trovare «Björk»");
    }

    #[test]
    fn l_indice_di_ricerca_segue_le_modifiche() {
        // Un indice che si scorda un aggiornamento produce brani che esistono e
        // non si trovano cercandoli: è un guasto che non si nota provando a mano.
        let db = open_in_memory().expect("apertura");
        let c = &db.connection;
        c.execute(
            "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size, date_added, date_modified)
             VALUES (1, 'C:/M/a.mp3', 'k', 'Vecchio', 'X', 'Y', 5000000, 1, 1)",
            [],
        )
        .expect("inserimento");

        c.execute("UPDATE tracks SET title = 'Nuovo' WHERE id = 1", [])
            .expect("rinomina");

        let vecchio: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH 'vecchio'",
                [],
                |r| r.get(0),
            )
            .expect("ricerca");
        let nuovo: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH 'nuovo'",
                [],
                |r| r.get(0),
            )
            .expect("ricerca");
        assert_eq!(vecchio, 0, "il titolo vecchio non deve restare nell'indice");
        assert_eq!(nuovo, 1);

        c.execute("DELETE FROM tracks WHERE id = 1", [])
            .expect("cancella");
        let dopo: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH 'nuovo'",
                [],
                |r| r.get(0),
            )
            .expect("ricerca");
        assert_eq!(dopo, 0, "cancellare un brano lo toglie dall'indice");
    }

    /// Quante righe ha il deposito dei segmenti dell'indice di ricerca.
    ///
    /// Ogni transazione che tocca l'indice ci lascia un segmento nuovo, anche
    /// quando rimette gli stessi titoli di prima: è il segno, visibile da fuori,
    /// che un trigger ha riscritto una riga nell'indice.
    fn segmenti_di_ricerca(connection: &Connection) -> i64 {
        quante(connection, "tracks_fts_data")
    }

    /// Un brano solo, con i campi che la tabella pretende.
    fn un_brano(connection: &Connection) {
        connection
            .execute(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size,
                                     date_added, date_modified)
                 VALUES (1, 'C:/M/a.mp3', 'k', 'Vecchio', 'X', 'Y', 5000000, 1, 1)",
                [],
            )
            .expect("inserimento");
    }

    /// Quanti brani trova una ricerca.
    fn trovati(connection: &Connection, parola: &str) -> i64 {
        connection
            .query_row(
                "SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH ?1",
                [parola],
                |r| r.get(0),
            )
            .expect("ricerca")
    }

    #[test]
    fn l_indice_di_ricerca_si_riscrive_solo_quando_cambia_il_testo() {
        // Prima della 33 un ascolto contato riscriveva la riga nell'indice: è
        // la prova che la sonda vede quel che deve vedere. Senza questa metà,
        // una sonda che non si muove mai passerebbe in tutti e due i casi.
        let prima = Connection::open_in_memory().expect("apertura in memoria");
        fino_alla(&prima, 32);
        un_brano(&prima);
        let segmenti = segmenti_di_ricerca(&prima);
        prima
            .execute(
                "UPDATE tracks SET play_count = play_count + 1 WHERE id = 1",
                [],
            )
            .expect("un ascolto");
        assert!(
            segmenti_di_ricerca(&prima) > segmenti,
            "alla 32 un ascolto passa ancora dall'indice"
        );

        // Dalla 33: ascolti, la data rimessa a -1 da «Rileggi tutti i tag» e la
        // chiave d'album riscritta dagli aggregati non toccano l'indice.
        let db = open_in_memory().expect("apertura");
        let c = &db.connection;
        un_brano(c);
        let segmenti = segmenti_di_ricerca(c);
        c.execute_batch(
            "UPDATE tracks SET play_count = play_count + 1 WHERE id = 1;
             UPDATE tracks SET date_modified = -1 WHERE id = 1;
             UPDATE tracks SET album_key = 'y m' WHERE id = 1;",
        )
        .expect("scritture che non riguardano il testo");
        assert_eq!(segmenti_di_ricerca(c), segmenti, "l'indice non si è mosso");
        assert_eq!(trovati(c, "vecchio"), 1, "e trova ancora quel che trovava");

        // Il titolo sì: la riga si riscrive, il nuovo si trova e il vecchio no.
        c.execute("UPDATE tracks SET title = 'Nuovo' WHERE id = 1", [])
            .expect("rinomina");
        assert!(segmenti_di_ricerca(c) > segmenti);
        assert_eq!(trovati(c, "nuovo"), 1);
        assert_eq!(trovati(c, "vecchio"), 0);
    }

    #[test]
    fn i_brani_d_immagine_e_le_due_memorie_della_scansione_hanno_il_loro_posto() {
        // Le tre colonne dei brani d'immagine, la memoria dei rifiuti e la cache
        // dei fogli: quel che la scansione scrive dalla 34 in poi.
        let db = open_in_memory().expect("apertura");
        let c = &db.connection;
        c.execute_batch(
            "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size,
                                 date_added, date_modified, immagine, inizio_cd, fine_cd)
             VALUES (1, 'C:/M/Disco.cue|01', 'k1', 'Uno', 'X', 'Y', 5000000, 1, 1,
                     'C:/M/Disco.flac', 0, 15000),
                    (2, 'C:/M/Disco.cue|02', 'k2', 'Due', 'X', 'Y', 5000000, 1, 1,
                     'C:/M/Disco.flac', 15000, NULL),
                    (3, 'C:/M/a.mp3', 'k3', 'Tre', 'X', 'Y', 5000000, 1, 1, NULL, NULL, NULL);
             INSERT INTO file_illeggibili (chiave, dimensione, modificato_ms, versione,
                                           causa, visto_ms)
             VALUES ('c:/m/b.wma', 10, 20, 1, 'formato', 30);
             INSERT INTO fogli_cue (chiave, dimensione, modificato_ms, foglio)
             VALUES ('c:/m/disco.cue', 100, 200, 'null');",
        )
        .expect("le tabelle nuove");
        assert_eq!(
            colonna(
                c,
                "SELECT path FROM tracks WHERE immagine IS NOT NULL ORDER BY path"
            ),
            ["C:/M/Disco.cue|01", "C:/M/Disco.cue|02"]
        );
        // L'indice parziale è quello che risponde a «i brani di questa immagine».
        let piano: String = c
            .query_row(
                "EXPLAIN QUERY PLAN SELECT id FROM tracks WHERE immagine = 'C:/M/Disco.flac'",
                [],
                |r| r.get(3),
            )
            .expect("piano");
        assert!(piano.contains("idx_tracks_immagine"), "{piano}");
        assert_eq!(quante(c, "file_illeggibili"), 1);
        assert_eq!(quante(c, "fogli_cue"), 1);
    }

    /// Lo schema intero, riga per riga: tabelle, indici e trigger col loro SQL.
    fn schema(connection: &Connection) -> Vec<String> {
        colonna(
            connection,
            "SELECT type || ' ' || name || ' ' || COALESCE(sql, '')
               FROM sqlite_master ORDER BY type, name",
        )
    }

    #[test]
    fn la_trentacinque_aggiunge_i_pari_e_non_tocca_il_resto() {
        // Una libreria alla 34 con dentro le cose che si perderebbero, poi la
        // 35 per la strada vera: la tabella c'è, e il resto è com'era.
        let connection = Connection::open_in_memory().expect("apertura in memoria");
        fino_alla(&connection, 34);
        libreria_da_perdere(&connection);
        let prima = schema(&connection);

        let applicate = migrate(&connection).expect("la 35");
        assert_eq!(
            u32::try_from(applicate).expect("il conto delle migrazioni sta in un u32"),
            LATEST_VERSION - 34
        );
        for tabella in [
            "tracks",
            "playlist_tracks",
            "play_history",
            "track_overrides",
        ] {
            assert_eq!(quante(&connection, tabella), 1, "{tabella}");
        }
        // Solo righe in più nello schema: nessuna tabella di prima è cambiata.
        let dopo = schema(&connection);
        assert!(
            prima.iter().all(|riga| dopo.contains(riga)),
            "la 35 ha toccato qualcosa che c'era già"
        );
        // E nessuna chiave esterna, in nessuno dei due versi: è quel che rende
        // economico il ritorno indietro scritto in testa alla 035.
        assert_eq!(
            quante(&connection, "pragma_foreign_key_list('insieme_pari')"),
            0
        );
    }

    #[test]
    fn i_vincoli_della_trentacinque_tengono() {
        let db = open_in_memory().expect("apertura");
        let c = &db.connection;
        let chiave = "ab".repeat(32);
        let inserisci = |chiave: &str, bloccato: i64| {
            c.execute(
                "INSERT INTO insieme_pari (chiave, nome, bloccato, associato_ms)
                 VALUES (?1, 'PC', ?2, 1)",
                rusqlite::params![chiave, bloccato],
            )
        };
        inserisci(&chiave, 0).expect("una chiave in forma");
        assert!(inserisci(&chiave, 0).is_err(), "una chiave è un PC solo");
        for storta in [
            "AB".repeat(32),
            "ab".repeat(31),
            format!("{}g", "a".repeat(63)),
        ] {
            assert!(inserisci(&storta, 0).is_err(), "{storta}");
        }
        assert!(inserisci(&"cd".repeat(32), 2).is_err(), "bloccato è 0 o 1");
        assert_eq!(quante(c, "insieme_pari"), 1);
        let visto: Option<i64> = c
            .query_row("SELECT visto_ms FROM insieme_pari", [], |r| r.get(0))
            .expect("letto");
        assert_eq!(visto, None, "mai visto finché non si fa vedere");
    }

    #[test]
    fn togliere_la_trentacinque_a_mano_riporta_lo_schema_della_trentaquattro() {
        // Il ritorno indietro a mano scritto in testa alla 035 — `DROP TABLE` e
        // `user_version` — deve dare **esattamente** il database che una 3.0.2
        // si aspetta: se un giorno la 035 crescesse di un indice o di un
        // trigger e il commento no, questa prova lo direbbe.
        let alla_34 = Connection::open_in_memory().expect("apertura in memoria");
        fino_alla(&alla_34, 34);

        let tornata = Connection::open_in_memory().expect("apertura in memoria");
        fino_alla(&tornata, 35);
        tornata
            .execute(
                "INSERT INTO insieme_pari (chiave, nome, associato_ms) VALUES (?1, 'PC', 1)",
                ["ab".repeat(32)],
            )
            .expect("un PC associato");
        tornata
            .execute_batch("DROP TABLE insieme_pari; PRAGMA user_version = 34;")
            .expect("ritorno a mano");

        assert_eq!(schema(&tornata), schema(&alla_34));
        assert_eq!(user_version(&tornata).expect("versione"), 34);
    }

    #[test]
    fn la_trentasei_aggiunge_lo_specchio_e_non_tocca_il_resto() {
        // Una libreria alla 35 con dentro le cose che si perderebbero, poi la
        // 36 per la strada vera: le tre tabelle ci sono, e il resto è com'era.
        let connection = Connection::open_in_memory().expect("apertura in memoria");
        fino_alla(&connection, 35);
        libreria_da_perdere(&connection);
        let prima = schema(&connection);

        let applicate = migrate(&connection).expect("la 36");
        assert_eq!(
            u32::try_from(applicate).expect("il conto delle migrazioni sta in un u32"),
            LATEST_VERSION - 35
        );
        for tabella in [
            "tracks",
            "playlist_tracks",
            "play_history",
            "track_overrides",
        ] {
            assert_eq!(quante(&connection, tabella), 1, "{tabella}");
        }
        let dopo = schema(&connection);
        assert!(
            prima.iter().all(|riga| dopo.contains(riga)),
            "la 36 ha toccato qualcosa che c'era già"
        );
        // Una cache: nessuna chiave esterna, in nessuno dei due versi, così
        // buttarla non tocca una riga di libreria.
        for tabella in ["nuvola_conti", "nuvola_voci", "nuvola_radici"] {
            assert_eq!(quante(&connection, tabella), 0, "{tabella}");
            assert_eq!(
                quante(
                    &connection,
                    &format!("pragma_foreign_key_list('{tabella}')")
                ),
                0,
                "{tabella}"
            );
        }
    }

    #[test]
    fn i_vincoli_della_trentasei_tengono() {
        let db = open_in_memory().expect("apertura");
        let c = &db.connection;
        let voce = |id: &str, cartella: i64| {
            c.execute(
                "INSERT INTO nuvola_voci (conto, id, genitore, nome, segmento, cartella,
                                          dimensione, modificato_ms)
                 VALUES ('onedrive-abcdefgh', ?1, NULL, 'a.flac', 'a.flac', ?2, 1, 1)",
                rusqlite::params![id, cartella],
            )
        };
        voce("1", 0).expect("una voce");
        assert!(voce("1", 0).is_err(), "(conto, id) è una voce sola");
        assert!(voce("2", 2).is_err(), "cartella è 0 o 1");
        let radice = |percorso: &str, completa: i64| {
            c.execute(
                "INSERT INTO nuvola_radici (percorso, conto, cima, completa)
                 VALUES (?1, 'onedrive-abcdefgh', 'X', ?2)",
                rusqlite::params![percorso, completa],
            )
        };
        radice("nuvola://onedrive-abcdefgh/Musica", 1).expect("una radice");
        assert!(radice("nuvola://onedrive-abcdefgh/Musica", 0).is_err());
        assert!(radice("nuvola://onedrive-abcdefgh/Altro", 3).is_err());
        // L'indice dei figli è quello che risponde a «i figli di questa
        // cartella».
        let piano: String = c
            .query_row(
                "EXPLAIN QUERY PLAN SELECT id FROM nuvola_voci
                  WHERE conto = 'onedrive-abcdefgh' AND genitore = 'X'",
                [],
                |r| r.get(3),
            )
            .expect("piano");
        assert!(piano.contains("idx_nuvola_voci_genitore"), "{piano}");
    }

    #[test]
    fn togliere_la_trentasei_a_mano_riporta_lo_schema_della_trentacinque() {
        // Il ritorno indietro scritto in testa alla 036, parola per parola: se
        // un giorno la 036 crescesse di un indice e il commento no, questa
        // prova lo direbbe.
        let alla_35 = Connection::open_in_memory().expect("apertura in memoria");
        fino_alla(&alla_35, 35);

        let tornata = Connection::open_in_memory().expect("apertura in memoria");
        fino_alla(&tornata, 36);
        tornata
            .execute_batch(
                "INSERT INTO nuvola_conti (conto, servizio, nome)
                 VALUES ('onedrive-abcdefgh', 'onedrive', 'x@y');
                 INSERT INTO nuvola_voci (conto, id, nome, segmento, cartella, dimensione,
                                          modificato_ms)
                 VALUES ('onedrive-abcdefgh', '1', 'a.flac', 'a.flac', 0, 1, 1);",
            )
            .expect("uno specchio");
        tornata
            .execute_batch(
                "DROP INDEX idx_nuvola_voci_genitore; DROP TABLE nuvola_voci;
                 DROP TABLE nuvola_radici; DROP TABLE nuvola_conti; PRAGMA user_version = 35;",
            )
            .expect("ritorno a mano");

        assert_eq!(schema(&tornata), schema(&alla_35));
        assert_eq!(user_version(&tornata).expect("versione"), 35);
    }

    #[test]
    fn la_trentasette_porta_il_client_del_backup_in_un_app() {
        // Una libreria alla 36 di chi aveva scritto il client del suo progetto
        // Google nelle Impostazioni del backup, e una cartella di Drive e una
        // di OneDrive collegate.
        let connection = Connection::open_in_memory().expect("apertura in memoria");
        fino_alla(&connection, 36);
        libreria_da_perdere(&connection);
        connection
            .execute_batch(
                "INSERT INTO settings (key, value) VALUES
                   ('nuvola.client_id', ' 111-aaa.apps.googleusercontent.com '),
                   ('nuvola.microsoft.client_id', 'compilato-a-mano'),
                   ('nuvola.dropbox.app_key', 'compilata-a-mano'),
                   ('nuvola.attivo', '1');
                 INSERT INTO nuvola_conti (conto, servizio, nome)
                 VALUES ('gdrive-aaaaaaaa', 'gdrive', 'a@b'),
                        ('onedrive-aaaaaaaa', 'onedrive', 'a@b');",
            )
            .expect("la libreria di prima");
        let prima = schema(&connection);

        migrate(&connection).expect("la 37");

        let app: (String, String, String, String) = connection
            .query_row(
                "SELECT app, piattaforma, nome, client_id FROM nuvola_app",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .expect("un'app sola");
        assert_eq!(
            app,
            (
                "google-ereditata".to_owned(),
                "google".to_owned(),
                "Google".to_owned(),
                "111-aaa.apps.googleusercontent.com".to_owned()
            )
        );
        let valore = |chiave: &str| -> Option<String> {
            connection
                .query_row("SELECT value FROM settings WHERE key = ?1", [chiave], |r| {
                    r.get(0)
                })
                .ok()
        };
        assert_eq!(valore("nuvola.app").as_deref(), Some("google-ereditata"));
        assert_eq!(
            valore("nuvola.attivo").as_deref(),
            Some("1"),
            "il resto resta"
        );
        for vecchia in [
            "nuvola.client_id",
            "nuvola.microsoft.client_id",
            "nuvola.dropbox.app_key",
        ] {
            assert_eq!(valore(vecchia), None, "{vecchia}");
        }
        let legame = |conto: &str| -> (Option<String>, Option<String>) {
            connection
                .query_row(
                    "SELECT app, client FROM nuvola_conti WHERE conto = ?1",
                    [conto],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .expect("il conto")
        };
        assert_eq!(
            legame("gdrive-aaaaaaaa"),
            (
                Some("google-ereditata".to_owned()),
                Some("111-aaa.apps.googleusercontent.com".to_owned())
            ),
            "Drive era collegato col client del backup"
        );
        assert_eq!(
            legame("onedrive-aaaaaaaa"),
            (None, None),
            "OneDrive aveva un'app compilata, che non c'è più"
        );
        // Le righe di libreria restano, e quel che c'era resta com'era (tranne
        // `nuvola_conti`, che ha due colonne in più).
        for tabella in [
            "tracks",
            "playlist_tracks",
            "play_history",
            "track_overrides",
        ] {
            assert_eq!(quante(&connection, tabella), 1, "{tabella}");
        }
        let dopo = schema(&connection);
        assert!(
            prima
                .iter()
                .filter(|riga| !riga.contains("nuvola_conti"))
                .all(|riga| dopo.contains(riga)),
            "la 37 ha toccato qualcosa che c'era già"
        );
        assert_eq!(
            quante(&connection, "pragma_foreign_key_list('nuvola_app')"),
            0
        );
    }

    #[test]
    fn la_trentasette_senza_client_non_crea_niente() {
        let connection = Connection::open_in_memory().expect("apertura in memoria");
        fino_alla(&connection, 36);
        connection
            .execute_batch(
                "INSERT INTO settings (key, value) VALUES ('nuvola.client_id', '   ');
                 INSERT INTO nuvola_conti (conto, servizio, nome)
                 VALUES ('gdrive-aaaaaaaa', 'gdrive', 'a@b');",
            )
            .expect("un client vuoto");
        migrate(&connection).expect("la 37");
        assert_eq!(quante(&connection, "nuvola_app"), 0);
        assert_eq!(
            quante(&connection, "settings WHERE key LIKE 'nuvola.%'"),
            0,
            "né l'app del backup né la chiave vuota"
        );
        let app: Option<String> = connection
            .query_row("SELECT app FROM nuvola_conti", [], |r| r.get(0))
            .expect("il conto");
        assert_eq!(app, None);
    }

    #[test]
    fn i_vincoli_della_trentasette_tengono() {
        let db = open_in_memory().expect("apertura");
        let c = &db.connection;
        let app = |app: &str, piattaforma: &str, client: &str| {
            c.execute(
                "INSERT INTO nuvola_app (app, piattaforma, nome, client_id, creata_ms)
                 VALUES (?1, ?2, 'Nome', ?3, 1)",
                rusqlite::params![app, piattaforma, client],
            )
        };
        app("google-aaaaaaaa", "google", "x").expect("un'app");
        assert!(
            app("google-aaaaaaaa", "google", "y").is_err(),
            "l'app è una"
        );
        assert!(
            app("google-bbbbbbbb", "google", "x").is_err(),
            "lo stesso client due volte"
        );
        app("dropbox-aaaaaaaa", "dropbox", "x").expect("lo stesso testo su un'altra piattaforma");
        assert!(
            app("mega-aaaaaaaa", "mega", "z").is_err(),
            "piattaforma sconosciuta"
        );
        assert!(
            app("google-cccccccc", "google", "  ").is_err(),
            "client vuoto"
        );
        assert!(
            c.execute(
                "INSERT INTO nuvola_app (app, piattaforma, nome, client_id, creata_ms)
                 VALUES ('google-dddddddd', 'google', ' ', 'w', 1)",
                [],
            )
            .is_err(),
            "nome vuoto"
        );
    }

    #[test]
    fn togliere_la_trentasette_a_mano_riporta_la_trentasei() {
        // Il ritorno indietro scritto in testa alla 037, parola per parola.
        // `DROP COLUMN` riscrive il testo di `CREATE TABLE nuvola_conti` a modo
        // suo: quella tabella si confronta colonna per colonna, il resto riga
        // per riga.
        let alla_36 = Connection::open_in_memory().expect("apertura in memoria");
        fino_alla(&alla_36, 36);

        let tornata = Connection::open_in_memory().expect("apertura in memoria");
        fino_alla(&tornata, 36);
        tornata
            .execute(
                "INSERT INTO settings (key, value) VALUES ('nuvola.client_id', '1-a.apps.googleusercontent.com')",
                [],
            )
            .expect("il client di prima");
        migrate(&tornata).expect("la 37");
        // `migrate` porta all'ultima: prima si toglie quel che è venuto dopo
        // la 037, col ritorno indietro scritto in testa a ciascuna.
        tornata
            .execute_batch("DROP INDEX idx_brano_vicino_vicino; PRAGMA user_version = 37;")
            .expect("ritorno a mano dalla 38");
        tornata
            .execute_batch(
                "INSERT OR REPLACE INTO settings (key, value)
                   SELECT 'nuvola.client_id', client_id FROM nuvola_app
                    WHERE app = (SELECT value FROM settings WHERE key = 'nuvola.app')
                      AND piattaforma = 'google';
                 DELETE FROM settings WHERE key = 'nuvola.app';
                 ALTER TABLE nuvola_conti DROP COLUMN client;
                 ALTER TABLE nuvola_conti DROP COLUMN app;
                 DROP TABLE nuvola_app; PRAGMA user_version = 36;",
            )
            .expect("ritorno a mano");

        let senza_conti = |c: &Connection| -> Vec<String> {
            schema(c)
                .into_iter()
                .filter(|riga| !riga.starts_with("table nuvola_conti "))
                .collect()
        };
        assert_eq!(senza_conti(&tornata), senza_conti(&alla_36));
        let colonne = |c: &Connection| {
            colonna(
                c,
                "SELECT name || ' ' || type || ' ' || \"notnull\" || ' ' || pk
                   FROM pragma_table_info('nuvola_conti') ORDER BY cid",
            )
        };
        assert_eq!(colonne(&tornata), colonne(&alla_36));
        assert_eq!(user_version(&tornata).expect("versione"), 36);
        let client: String = tornata
            .query_row(
                "SELECT value FROM settings WHERE key = 'nuvola.client_id'",
                [],
                |r| r.get(0),
            )
            .expect("il client torna dove la 3.1 lo cerca");
        assert_eq!(client, "1-a.apps.googleusercontent.com");
    }

    #[test]
    fn la_cascata_dei_vicini_ha_un_indice() {
        // Ogni brano tolto cerca in `brano_vicino` le righe che lo nominano
        // come vicino, per la `ON DELETE CASCADE`: senza un indice che
        // cominci con `vicino_id` è una lettura intera della tabella per ogni
        // brano, con il lucchetto della libreria in mano.
        let db = open_in_memory().expect("database");
        let piano: Vec<String> = {
            let mut query = db
                .connection
                .prepare("EXPLAIN QUERY PLAN SELECT 1 FROM brano_vicino WHERE vicino_id = ?1")
                .expect("piano");
            query
                .query_map([7_i64], |riga| riga.get::<_, String>(3))
                .expect("lettura")
                .collect::<Result<_, _>>()
                .expect("righe del piano")
        };
        assert!(
            piano
                .iter()
                .any(|riga| riga.contains("USING INDEX idx_brano_vicino_vicino")
                    || riga.contains("USING COVERING INDEX idx_brano_vicino_vicino")),
            "{piano:?}"
        );
        assert!(
            !piano.iter().any(|riga| riga.starts_with("SCAN")),
            "{piano:?}"
        );
    }

    #[test]
    fn togliere_la_trentotto_a_mano_riporta_la_trentasette() {
        // Il ritorno indietro scritto in testa alla 038, parola per parola.
        let alla_37 = Connection::open_in_memory().expect("apertura in memoria");
        fino_alla(&alla_37, 37);

        let tornata = Connection::open_in_memory().expect("apertura in memoria");
        fino_alla(&tornata, 38);
        tornata
            .execute_batch("DROP INDEX idx_brano_vicino_vicino; PRAGMA user_version = 37;")
            .expect("ritorno a mano");

        assert_eq!(schema(&tornata), schema(&alla_37));
        assert_eq!(user_version(&tornata).expect("versione"), 37);
    }

    /// La colonna di testo di una query, riga per riga, nell'ordine chiesto.
    fn colonna(connection: &Connection, sql: &str) -> Vec<String> {
        let mut query = connection
            .prepare(sql)
            .unwrap_or_else(|err| panic!("{sql}: {err}"));
        query
            .query_map([], |riga| riga.get::<_, String>(0))
            .expect("lettura")
            .filter_map(Result::ok)
            .collect()
    }

    /// Un desiderato, con i soli campi che la tabella pretende.
    fn desiderato(
        connection: &Connection,
        chiave: &str,
        titolo: &str,
        sorgente: &str,
        quando: i64,
    ) {
        connection
            .execute(
                "INSERT INTO desiderati (track_key, title, source_kind, source_id,
                                         source_title, added_at)
                 VALUES (?1, ?2, 'playlist', ?3, 'Una playlist', ?4)",
                rusqlite::params![chiave, titolo, sorgente, quando],
            )
            .expect("desiderato");
    }

    /// Un database alla 30, com'era quello di chi veniva dalla versione di
    /// prima, con le sorgenti scritte col nome di allora — e accanto quelle che
    /// non devono essere toccate.
    fn libreria_con_le_sorgenti_di_prima(connection: &Connection) {
        // Quelle da riscrivere: il prefisso di prima, davanti a tutto il resto.
        desiderato(connection, "k1", "Uno", "aether:archivio:playlist:corsa", 1);
        desiderato(connection, "k2", "Due", "aether:spotify:preferiti", 2);
        // Quelle da lasciare: un identificativo vero di Spotify, e tre stringhe
        // che somigliano al prefisso senza esserlo. Il confronto è esatto: con
        // un `LIKE` la seconda verrebbe riscritta (ignora le maiuscole), e con
        // un `substr` sbagliato la terza.
        desiderato(connection, "k3", "Tre", "37i9dQZF1DXcBWIGoYBM5M", 3);
        desiderato(connection, "k4", "Quattro", "AETHER:maiuscole", 4);
        desiderato(connection, "k5", "Cinque", "xaether:dentro", 5);
        desiderato(connection, "k6", "Sei", "aether-senza-due-punti", 6);

        connection
            .execute_batch(
                "INSERT INTO playlists (playlist_key, name, created_at, updated_at,
                                        source_playlist_id, source_service)
                 VALUES ('corsa', 'Corsa', 10, 20, 'aether:archivio:playlist:corsa', 'spotify');
                 INSERT INTO playlists (playlist_key, name, created_at, updated_at,
                                        source_playlist_id, source_service)
                 VALUES ('vera', 'Vera', 10, 20, '37i9dQZF1DXcBWIGoYBM5M', 'spotify');
                 INSERT INTO playlists (playlist_key, name, created_at, updated_at)
                 VALUES ('mia', 'Mia', 10, 20);
                 INSERT INTO import_reports (source_id, saved_at, payload)
                 VALUES ('aether:spotify:preferiti', 1,
                         '{\"kind\":\"preferiti\",\"sourceId\":\"aether:spotify:preferiti\",\"resolved\":3}');
                 INSERT INTO import_reports (source_id, saved_at, payload)
                 VALUES ('37i9dQZF1DXcBWIGoYBM5M', 2,
                         '{\"kind\":\"playlist\",\"sourceId\":\"37i9dQZF1DXcBWIGoYBM5M\"}');
                 INSERT INTO import_reports (source_id, saved_at, payload)
                 VALUES ('aether:rotto', 3, 'non è un JSON');",
            )
            .expect("sorgenti di prima");
    }

    #[test]
    fn le_sorgenti_di_prima_si_riscrivono_e_le_altre_no() {
        let connection = Connection::open_in_memory().expect("apertura in memoria");
        fino_alla(&connection, 30);
        libreria_con_le_sorgenti_di_prima(&connection);

        let applicate = migrate(&connection).expect("la 31 deve passare");
        // Quante ne mancano dalla 30, e non «una»: il numero scritto a mano
        // diventava rosso alla prima migrazione aggiunta dopo la 31.
        assert_eq!(
            u32::try_from(applicate).expect("il conto delle migrazioni sta in un u32"),
            LATEST_VERSION - 30,
            "dalla 30 mancano la 31 e tutte quelle dopo"
        );
        assert_eq!(user_version(&connection).expect("versione"), LATEST_VERSION);

        // I desiderati: il resto della stringa è intatto, e l'ordine è quello
        // di inserimento.
        assert_eq!(
            colonna(
                &connection,
                "SELECT source_id FROM desiderati ORDER BY added_at"
            ),
            [
                "rymembra:archivio:playlist:corsa",
                "rymembra:spotify:preferiti",
                "37i9dQZF1DXcBWIGoYBM5M",
                "AETHER:maiuscole",
                "xaether:dentro",
                "aether-senza-due-punti",
            ]
        );
        // Nessuna riga se n'è andata, e nessun'altra colonna è cambiata.
        assert_eq!(quante(&connection, "desiderati"), 6);
        assert_eq!(
            colonna(
                &connection,
                "SELECT title FROM desiderati ORDER BY added_at"
            ),
            ["Uno", "Due", "Tre", "Quattro", "Cinque", "Sei"]
        );

        // Le playlist: l'identificativo di Spotify e il vuoto restano com'erano,
        // e `updated_at` non si muove.
        assert_eq!(
            colonna(
                &connection,
                "SELECT COALESCE(source_playlist_id, '-') FROM playlists ORDER BY id"
            ),
            [
                "rymembra:archivio:playlist:corsa",
                "37i9dQZF1DXcBWIGoYBM5M",
                "-"
            ]
        );
        assert_eq!(
            colonna(
                &connection,
                "SELECT CAST(updated_at AS TEXT) FROM playlists ORDER BY id"
            ),
            ["20", "20", "20"]
        );

        // I rapporti: la chiave e il campo dentro il JSON dicono la stessa
        // cosa, e il resto del JSON è lo stesso. Quello che non è un JSON cambia
        // chiave e non si tocca nel contenuto.
        assert_eq!(
            colonna(
                &connection,
                "SELECT source_id FROM import_reports ORDER BY saved_at"
            ),
            [
                "rymembra:spotify:preferiti",
                "37i9dQZF1DXcBWIGoYBM5M",
                "rymembra:rotto"
            ]
        );
        let riscritto = colonna(
            &connection,
            "SELECT payload FROM import_reports WHERE saved_at = 1",
        );
        let json: serde_json::Value = serde_json::from_str(&riscritto[0]).expect("JSON valido");
        assert_eq!(json["sourceId"], "rymembra:spotify:preferiti");
        assert_eq!(json["kind"], "preferiti");
        assert_eq!(json["resolved"], 3);
        assert_eq!(
            colonna(
                &connection,
                "SELECT payload FROM import_reports WHERE saved_at = 3"
            ),
            ["non è un JSON"]
        );
    }

    #[test]
    fn in_una_collisione_vince_la_riga_nuova() {
        let connection = Connection::open_in_memory().expect("apertura in memoria");
        fino_alla(&connection, 30);
        // Lo stesso brano della stessa importazione, scritto una volta per
        // prefisso: l'indice unico `(track_key, source_id)` non lascerebbe
        // convivere le due righe dopo la riscrittura.
        desiderato(&connection, "k", "Vecchio", "aether:archivio:album:x", 1);
        desiderato(&connection, "k", "Nuovo", "rymembra:archivio:album:x", 2);
        // Un brano diverso con la stessa sorgente non collide, e si riscrive.
        desiderato(&connection, "altro", "Altro", "aether:archivio:album:x", 3);
        connection
            .execute_batch(
                "INSERT INTO import_reports (source_id, saved_at, payload)
                 VALUES ('aether:y', 1, '{\"sourceId\":\"aether:y\",\"title\":\"vecchio\"}');
                 INSERT INTO import_reports (source_id, saved_at, payload)
                 VALUES ('rymembra:y', 2, '{\"sourceId\":\"rymembra:y\",\"title\":\"nuovo\"}');",
            )
            .expect("rapporti in collisione");

        migrate(&connection).expect("la 31 passa anche con le collisioni");

        assert_eq!(
            colonna(
                &connection,
                "SELECT title || '@' || source_id FROM desiderati ORDER BY added_at"
            ),
            [
                "Nuovo@rymembra:archivio:album:x",
                "Altro@rymembra:archivio:album:x"
            ],
            "la riga nuova resta, la vecchia in collisione se ne va"
        );
        assert_eq!(
            colonna(&connection, "SELECT payload FROM import_reports"),
            [r#"{"sourceId":"rymembra:y","title":"nuovo"}"#]
        );
    }

    #[test]
    fn la_trentuno_si_puo_rieseguire_senza_effetti() {
        let connection = Connection::open_in_memory().expect("apertura in memoria");
        fino_alla(&connection, 30);
        libreria_con_le_sorgenti_di_prima(&connection);
        migrate(&connection).expect("la 31");

        let istantanea = |connection: &Connection| {
            let mut righe = colonna(
                connection,
                "SELECT 'd|' || track_key || '|' || source_id FROM desiderati
                 UNION ALL
                 SELECT 'p|' || playlist_key || '|' || COALESCE(source_playlist_id, '-')
                   FROM playlists
                 UNION ALL
                 SELECT 'r|' || source_id || '|' || payload FROM import_reports",
            );
            righe.sort();
            righe
        };
        let prima = istantanea(&connection);

        // Il file della 31 eseguito di nuovo, a mano: dopo di lei nessuna riga
        // comincia più per il prefisso di prima, quindi non ha niente da fare.
        let trentuno = MIGRATIONS
            .iter()
            .find(|m| m.version == 31)
            .expect("la 31 è nell'elenco");
        connection
            .execute_batch(trentuno.sql)
            .expect("la 31 riletta");
        assert_eq!(istantanea(&connection), prima);

        // E una riapertura non la riapplica.
        assert_eq!(migrate(&connection).expect("niente da fare"), 0);
        assert_eq!(istantanea(&connection), prima);
    }

    #[test]
    fn aprire_il_file_di_prima_lascia_l_istantanea_della_trenta() {
        // La strada vera: un file alla 30 sul disco, `open`, e la copia di
        // sicurezza col nome di adesso accanto al database con quello di adesso.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let percorso = dir.path().join("rymembra.db");
        {
            let connection = Connection::open(&percorso).expect("creazione");
            fino_alla(&connection, 30);
            libreria_con_le_sorgenti_di_prima(&connection);
        }

        let aperto = open(&percorso).expect("apertura con migrazione");
        assert_eq!(aperto.version_before, 30);
        assert_eq!(
            u32::try_from(aperto.applied).expect("il conto delle migrazioni sta in un u32"),
            LATEST_VERSION - 30
        );
        assert_eq!(
            colonna(
                &aperto.connection,
                "SELECT source_id FROM desiderati ORDER BY added_at LIMIT 2"
            ),
            [
                "rymembra:archivio:playlist:corsa",
                "rymembra:spotify:preferiti"
            ]
        );

        let attesa = nome_istantanea(&percorso, 30);
        assert_eq!(
            attesa.file_name().and_then(|n| n.to_str()),
            Some("rymembra.db.schema-030.bak")
        );
        assert!(
            matches!(&aperto.istantanea, Some(Istantanea::Fatta { percorso, .. }) if *percorso == attesa),
            "{:?}",
            aperto.istantanea
        );
        // La copia è il database **prima** della 31: le sorgenti sono ancora
        // quelle di allora, ed è per questo che serve.
        let copia = Connection::open(&attesa).expect("la copia si apre");
        assert_eq!(
            colonna(
                &copia,
                "SELECT source_id FROM desiderati ORDER BY added_at LIMIT 1"
            ),
            ["aether:archivio:playlist:corsa"]
        );
        assert_eq!(user_version(&copia).expect("versione della copia"), 30);
    }

    #[test]
    fn ogni_migrazione_sul_disco_sta_anche_nell_elenco() {
        // Il difetto che questa prova tiene chiuso è già successo:
        // `016_metadati.sql` è stato scritto, salvato, e **mai eseguito da
        // nessuna libreria** — esisteva come file e non era in `MIGRATIONS`. Le
        // colonne su cui poggiava la scheda della salute dei metadati non
        // c'erano da nessuna parte, e non c'era niente che se ne accorgesse:
        // `le_versioni_delle_migrazioni_sono_crescenti_e_uniche` guarda solo
        // l'elenco, e un file dimenticato dall'elenco è invisibile all'elenco.
        //
        // Si contano i file, non i nomi: un `include_str!` che punta a un file
        // che non c'è non compila nemmeno, quindi il verso «elenco → disco» è
        // già garantito dal compilatore. Quello che manca è il verso opposto.
        let cartella = concat!(env!("CARGO_MANIFEST_DIR"), "/src/db/schema");
        let mut sul_disco: Vec<String> = std::fs::read_dir(cartella)
            .expect("la cartella dello schema")
            .filter_map(|voce| {
                let nome = voce.ok()?.file_name().to_string_lossy().into_owned();
                nome.ends_with(".sql").then_some(nome)
            })
            .collect();
        sul_disco.sort();

        assert_eq!(
            sul_disco.len(),
            MIGRATIONS.len(),
            "sul disco ci sono {} migrazioni e nell'elenco {}: {:?}",
            sul_disco.len(),
            MIGRATIONS.len(),
            sul_disco
        );

        // E i nomi corrispondono uno a uno, in ordine: contarle sole lascerebbe
        // passare un file aggiunto insieme a uno tolto.
        for (voce, migrazione) in sul_disco.iter().zip(MIGRATIONS) {
            let atteso = format!("{:03}_{}.sql", migrazione.version, migrazione.name);
            // Il nome del file usa i trattini bassi dove il nome della
            // migrazione usa quelli alti: `013_testi_senza_tempi.sql` sta in
            // elenco come `testi-senza-tempi`.
            assert_eq!(
                voce,
                &atteso.replace('-', "_"),
                "il file {voce} non corrisponde alla migrazione {}",
                migrazione.name
            );
        }
    }

    #[test]
    fn le_versioni_delle_migrazioni_sono_crescenti_e_uniche() {
        let mut atteso = 0;
        for migration in MIGRATIONS {
            atteso += 1;
            assert_eq!(
                migration.version, atteso,
                "le versioni devono essere consecutive da 1: {} è fuori posto",
                migration.name
            );
            assert!(
                !migration.sql.trim().is_empty(),
                "{} è vuota",
                migration.name
            );
        }
        assert_eq!(LATEST_VERSION, atteso);
    }
}
