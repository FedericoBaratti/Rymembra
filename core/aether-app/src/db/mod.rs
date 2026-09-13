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
//! su una share — perché `%APPDATA%` è reindirizzato, o perché `AETHER_DATI`
//! punta là — si ritrovava un `disk I/O error` raccontato come «il database ha
//! rifiutato una richiesta: riavvia Aether», che era falso due volte: non era
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

mod migrations;

pub use migrations::{LATEST_VERSION, MIGRATIONS, Migration};

use std::path::Path;
use std::sync::{Mutex, PoisonError};

use aether_domain::errors::{AppError, ErrorCode};
use rusqlite::{Connection, ErrorCode as CodiceSqlite};

/// Quanto aspettare se un'altra connessione sta scrivendo, prima di arrendersi.
const BUSY_TIMEOUT_MS: u32 = 5_000;

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
/// richiesta: se succede di continuo, riavvia Aether». Un file bloccato per
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
/// - tutto il resto → [`ErrorCode::DbQueryFailed`] con il suo `detail`, com'era.
///   Il ripiego resta un ripiego: un vincolo violato, un tipo che non torna e
///   una sintassi sbagliata sono difetti di Aether, non cose da raccontare.
///
/// Il `detail` viaggia comunque, e il testo di `rusqlite` finisce nella causa:
/// sono le due metà che dicono *quale* operazione è fallita e *come*, ed è
/// quello che si legge nel diario quando un utente segnala qualcosa.
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
        .with_cause(err.to_string());
    };
    let code = match codice {
        CodiceSqlite::DatabaseBusy | CodiceSqlite::DatabaseLocked => ErrorCode::DbLocked,
        CodiceSqlite::ReadOnly => ErrorCode::DbReadOnly,
        CodiceSqlite::DatabaseCorrupt | CodiceSqlite::NotADatabase => ErrorCode::DbCorrupt {
            path: contesto.percorso.clone(),
            // Non è stato messo in quarantena: questa funzione traduce, non
            // sposta file. Chi deciderà di metterlo da parte riempirà il campo.
            quarantined_as: None,
        },
        CodiceSqlite::SystemIoFailure if sospetto_di_rete(err, contesto) => {
            ErrorCode::DbNetworkPath {
                path: contesto.percorso.clone(),
            }
        }
        CodiceSqlite::SystemIoFailure => ErrorCode::DbIoFailed {
            detail: Some(detail.to_owned()),
        },
        _ => ErrorCode::DbQueryFailed {
            detail: Some(detail.to_owned()),
        },
    };
    AppError::new(code).with_cause(err.to_string())
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
    let memoria_condivisa = err.sqlite_error().is_some_and(|errore| {
        matches!(
            errore.extended_code,
            rusqlite::ffi::SQLITE_IOERR_SHMOPEN
                | rusqlite::ffi::SQLITE_IOERR_SHMSIZE
                | rusqlite::ffi::SQLITE_IOERR_SHMMAP
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
    open_connection(connection)
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
    open_connection(connection)
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
    /// impunta a ogni scansione. Con `giornale=truncate` nel diario, «Aether è
    /// lento durante la scansione» smette di essere un'impressione.
    pub giornale: String,
}

fn open_connection(connection: Connection) -> Result<Opened, AppError> {
    let giornale = configure(&connection)?;
    let fts5 = has_fts5(&connection);
    let version_before = user_version(&connection)?;
    let applied = migrate(&connection)?;
    Ok(Opened {
        connection,
        version_before,
        applied,
        fts5,
        giornale,
    })
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
    // raccontato come «riavvia Aether».
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
            "il database è stato scritto da una versione più nuova di Aether: \
             ripristina un backup invece di aprirlo con questa",
        ));
    }

    let mut applied = 0;
    for migration in MIGRATIONS.iter().filter(|m| m.version > current) {
        connection.execute_batch("BEGIN").map_err(|err| {
            db_error(
                ErrorCode::DbMigrationFailed {
                    from: current,
                    to: migration.version,
                    step: Some(migration.name.to_owned()),
                },
                &err,
            )
        })?;

        let outcome = connection.execute_batch(migration.sql).and_then(|()| {
            connection.execute_batch(&format!("PRAGMA user_version = {}", migration.version))
        });

        match outcome {
            Ok(()) => {
                connection.execute_batch("COMMIT").map_err(|err| {
                    db_error(
                        ErrorCode::DbMigrationFailed {
                            from: current,
                            to: migration.version,
                            step: Some(migration.name.to_owned()),
                        },
                        &err,
                    )
                })?;
                applied += 1;
            }
            Err(err) => {
                // Il rollback può fallire a sua volta (connessione morta): in
                // quel caso l'errore da riportare resta il primo, che è quello
                // che dice cosa è andato storto davvero.
                let _ = connection.execute_batch("ROLLBACK");
                return Err(db_error(
                    ErrorCode::DbMigrationFailed {
                        from: current,
                        to: migration.version,
                        step: Some(migration.name.to_owned()),
                    },
                    &err,
                ));
            }
        }
    }

    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_domain::errors::ErrorCodeKind;
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
            percorso: Some(r"C:\Users\tizio\AppData\Roaming\aether\aether.db".into()),
            di_rete: false,
            ripiegato: false,
        }
    }

    #[test]
    fn codice_da_sqlite_distingue_locked_da_query_failed() {
        // Il difetto, nella sua forma più breve: questi due arrivavano
        // all'utente come la stessa frase, con lo stesso consiglio di riavviare.
        // Uno passa da sé in mezzo secondo, l'altro è un difetto di Aether.
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
        // resta quel che era: è un difetto di Aether, non una cosa da raccontare.
        let nostro = codice_con_contesto(
            "lettura di una colonna",
            &rusqlite::Error::InvalidColumnIndex(7),
            &locale,
        );
        assert_eq!(nostro.code().kind(), ErrorCodeKind::DbQueryFailed);
    }

    #[test]
    fn codice_da_sqlite_riconosce_il_corrotto() {
        let locale = contesto_locale();
        for malformato in [rusqlite::ffi::SQLITE_CORRUPT, rusqlite::ffi::SQLITE_NOTADB] {
            let errore = codice_con_contesto("lettura degli album", &guasto(malformato), &locale);
            assert_eq!(errore.code().kind(), ErrorCodeKind::DbCorrupt);
            // `Fatal` apposta: continuare a scrivere su un file malformato
            // trasforma un backup ancora buono in due file rotti.
            assert_eq!(errore.severity(), aether_domain::errors::Severity::Fatal);
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
                percorso: Some(r"\\nas\aether\aether.db".into()),
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
                percorso: Some(r"Z:\aether\aether.db".into()),
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
        let aperto = open(&dir.path().join("aether.db")).expect("apertura");
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
        let aperto = open(&dir.path().join("aether.db")).expect("apertura");
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
        let percorso = dir.path().join("aether.db");
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
        let aperto = open(&dir.path().join("aether.db")).expect("apertura");
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
        // invece che come «riavvia Aether».
        //
        // *(Lungo la strada si è provato con una connessione di sola lettura, e
        // non serve: su un database in WAL la sonda **passa** — il lucchetto di
        // scrittura vive nel file di memoria condivisa, che è scrivibile, e una
        // transazione vuota non tocca il database — e su un database fuori da WAL
        // il ripiego **riesce**, perché fra due giornali di rollback il cambio è
        // una faccenda di connessione e non di file. Entrambi sono il
        // comportamento giusto, e nessuno dei due è un doppio guasto.)*
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let percorso = dir.path().join("aether.db");
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
    fn riaprire_non_riapplica_niente() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let path = dir.path().join("aether.db");

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
        let path = dir.path().join("aether.db");
        {
            let db = open(&path).expect("apertura");
            db.connection
                .execute_batch(&format!("PRAGMA user_version = {}", LATEST_VERSION + 5))
                .expect("finge una versione futura");
        }

        let errore = open(&path).expect_err("deve rifiutarsi");
        assert_eq!(errore.code().kind(), ErrorCodeKind::DbVersionAhead);
        assert_eq!(errore.severity(), aether_domain::errors::Severity::Fatal);
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
