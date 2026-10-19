//! La salute del database: ottimizzazione, controllo, istantanee.
//!
//! Tre gesti che SQLite sa fare da sé e che nessuno gli chiedeva:
//!
//! - **`PRAGMA optimize`**, che tiene aggiornate le statistiche con cui il
//!   pianificatore sceglie gli indici. Senza, una libreria che passa da cento a
//!   cinquantamila brani continua a essere interrogata con i piani pensati per
//!   cento. La ricetta è quella della documentazione di SQLite per le
//!   connessioni che vivono a lungo: `optimize=0x10002` all'apertura, poi
//!   `optimize` ogni tanto e prima di chiudere. Vedi [`ottimizza`].
//! - **`PRAGMA quick_check`**, che si accorge di un file danneggiato prima che
//!   se ne accorga una query a metà di un ascolto. Vedi [`controllo_rapido`].
//! - **`VACUUM INTO`** prima di ogni migrazione, cioè una copia coerente del
//!   database com'era, da rimettere a mano se una migrazione va storta o non
//!   piace. Vedi [`Istantanea`].
//!
//! # Quanto costano, misurato
//!
//! Su un database sintetico da cinquantamila brani — un quinto con il testo,
//! trecentomila ascolti, 107 MB — e a cache calda:
//!
//! | gesto                                   | tempo      |
//! |-----------------------------------------|------------|
//! | `optimize=0x10002`, mai analizzato      | 100 ms     |
//! | `optimize=0x10002`, già analizzato      | 1 ms       |
//! | `optimize`                              | < 1 ms     |
//! | `quick_check`                           | 200–240 ms |
//! | `integrity_check`                       | 1,8 s      |
//! | `VACUUM INTO`                           | 370 ms     |
//!
//! Da qui le scelte. `optimize` all'apertura costa i cento millisecondi una
//! volta sola, la prima dopo l'aggiornamento, e poi niente. `quick_check` no:
//! due decimi di secondo a cache calda sono un secondo e più a cache fredda, e
//! molti di più su un disco meccanico o su una share — troppo per l'avvio, che
//! gira sul filo della finestra. Si fa **dopo**, in disparte, su una
//! connessione sua e con un tempo massimo: vedi [`controlla_il_file`]. Il
//! controllo completo, nove volte più lento, non si fa mai da solo.
//! `VACUUM INTO` si paga solo quando c'è una migrazione da applicare, cioè una
//! volta per aggiornamento, e quella è l'occasione in cui vale la pena.

use std::path::{Path, PathBuf};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use rusqlite::{Connection, ErrorCode as CodiceSqlite, OpenFlags};
use rymembra_domain::errors::{AppError, ErrorCode};

/// Quante istantanee di prima delle migrazioni si tengono.
///
/// Due: quella dell'ultimo aggiornamento, e quella di prima — per chi si
/// accorge di un guasto solo dopo il secondo aggiornamento. Di più
/// occuperebbero spazio per offrire un ritorno a versioni che nessuno vuole:
/// ogni istantanea pesa quanto il database intero.
pub const ISTANTANEE_TENUTE: usize = 2;

/// Il margine d'aria oltre l'istantanea, sotto il quale non la si fa.
///
/// Il minimo: 32 MiB. Il margine vero è metà del database, se è di più — vedi
/// [`spazio_da_chiedere`].
const MARGINE_MINIMO: u64 = 32 * 1024 * 1024;

/// Quante righe di `quick_check` si tengono per il diario.
///
/// Otto. Un file danneggiato ne può produrre migliaia, tutte della stessa
/// forma, e quel che serve a chi legge è la prima — dove — e un'idea di quante.
const RIGHE_DEL_CONTROLLO: u32 = 8;

/// L'ottimizzazione di apertura: `PRAGMA optimize=0x10002`.
///
/// È la ricetta di SQLite per le connessioni che vivono a lungo: `0x10000`
/// guarda **tutte** le tabelle e non solo quelle che la connessione ha già
/// interrogato — appena aperta non ne ha interrogata nessuna — e `0x02` fa
/// girare `ANALYZE` dove serve. Dove serve è deciso da SQLite, con un tetto al
/// lavoro che si dà da sé: sul database da cinquantamila brani sono cento
/// millisecondi la prima volta e un millisecondo le successive.
///
/// Gli errori si buttano, come nella negoziazione del giornale: l'ottimizzazione
/// non ha il diritto di impedire l'apertura di una libreria. Un database che non
/// si lascia analizzare si legge e si scrive lo stesso, con i piani di prima.
pub(super) fn ottimizza_all_apertura(connection: &Connection) {
    let _ = connection.execute_batch("PRAGMA optimize=0x10002");
}

/// L'ottimizzazione periodica e di chiusura: `PRAGMA optimize`.
///
/// La documentazione di SQLite chiede di farla «ogni qualche ora» sulle
/// connessioni che restano aperte, e prima di chiuderle. Costa meno di un
/// millisecondo quando non c'è niente da fare — che è quasi sempre — e quando
/// c'è, cioè dopo una scansione che ha cambiato la libreria, è il momento in cui
/// le statistiche vecchie farebbero scegliere il piano sbagliato.
///
/// # Errori
///
/// `db.*` secondo quel che SQLite ha detto. Chi chiama di solito lo annota e va
/// avanti: nessuna ottimizzazione mancata vale un errore davanti all'utente.
pub fn ottimizza(connection: &Connection) -> Result<(), AppError> {
    connection
        .execute_batch("PRAGMA optimize")
        .map_err(|err| super::codice_da_sqlite("ottimizzazione del database", &err))
}

/// Com'è andato un controllo rapido che non ha trovato danni.
///
/// Un danno trovato non è qui: è un `Err` con [`ErrorCode::DbCorrupt`], lo
/// stesso codice che una query su un file malformato produce da sé, con la
/// stessa frase per l'utente — e la stessa gravità, `Fatal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Controllo {
    /// `quick_check` ha risposto `ok`.
    Sano {
        /// Quanto è durato.
        millisecondi: u64,
    },
    /// Il tempo concesso è finito prima della risposta: **non si sa**. Non è
    /// un danno e non è un via libera, ed è il motivo per cui ha un nome suo.
    Interrotto {
        /// Dopo quanto ci si è fermati.
        millisecondi: u64,
    },
}

/// `PRAGMA quick_check` sulla connessione data, fermato dopo `entro`.
///
/// # Perché `quick_check` e non `integrity_check`
///
/// Perché costa un nono: controlla la struttura delle pagine e dei record, e
/// salta il confronto fra ogni indice e la sua tabella. I danni che interessano
/// qui — pagine illeggibili dopo un disco che ha perso la corrente, un file
/// troncato da una copia a metà — li vede tutti.
///
/// # Il tempo massimo
///
/// SQLite non ne ha uno per le pragma, e `progress_handler` non è acceso in
/// questo binario. C'è `sqlite3_interrupt`, che si può chiamare da un altro
/// filo: un guardiano aspetta `entro` e, se il controllo non ha ancora finito,
/// lo interrompe. Il controllo interrotto è [`Controllo::Interrotto`], non un
/// errore.
///
/// # Errori
///
/// [`ErrorCode::DbCorrupt`] se il controllo trova qualcosa — le prime righe
/// della risposta finiscono nella causa, per il diario — o se il file è
/// talmente rotto che la pragma stessa non parte. Gli altri `db.*` per il resto
/// (un database occupato, un disco che non risponde).
pub fn controllo_rapido(connection: &Connection, entro: Duration) -> Result<Controllo, AppError> {
    let inizio = Instant::now();
    let righe = con_scadenza(connection, entro, |connection| {
        let mut statement =
            connection.prepare(&format!("PRAGMA quick_check({RIGHE_DEL_CONTROLLO})"))?;
        statement
            .query_map([], |riga| riga.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()
    });
    let millisecondi = u64::try_from(inizio.elapsed().as_millis()).unwrap_or(u64::MAX);
    let righe = match righe {
        Ok(righe) => righe,
        Err(err) if err.sqlite_error_code() == Some(CodiceSqlite::OperationInterrupted) => {
            return Ok(Controllo::Interrotto { millisecondi });
        }
        Err(err) => {
            return Err(super::codice_da_sqlite(
                "controllo rapido del database",
                &err,
            ));
        }
    };
    if righe.len() == 1 && righe.first().is_some_and(|r| r.eq_ignore_ascii_case("ok")) {
        return Ok(Controllo::Sano { millisecondi });
    }
    Err(AppError::new(ErrorCode::DbCorrupt {
        path: super::contesto().percorso,
        // Non si sposta niente: il controllo guarda, non mette in quarantena.
        quarantined_as: None,
    })
    .with_cause(format!("quick_check: {}", righe.join(" | "))))
}

/// [`controllo_rapido`] su una connessione **sua**, aperta apposta e chiusa
/// alla fine.
///
/// # Perché una connessione a parte
///
/// Perché il controllo legge tutto il file, e sulla connessione della libreria
/// vorrebbe dire tenerne il lucchetto per tutto quel tempo — la finestra che
/// aspetta, la riproduzione che non registra l'ascolto. Una connessione sua, in
/// WAL, legge un'istantanea coerente senza fermare nessuno: chi scrive continua
/// a scrivere nel giornale.
///
/// **Solo in WAL**, e sta a chi chiama saperlo: con il giornale ripiegato su
/// `TRUNCATE` un lettore lungo blocca il commit di ogni scrittore, che dopo i
/// cinque secondi di attesa risponde «occupato». Lì il controllo si salta.
///
/// La connessione si apre senza `SQLITE_OPEN_CREATE`: se il file non c'è, il
/// controllo fallisce invece di crearne uno vuoto e dichiararlo sano. E con
/// `query_only`, perché un controllo che scrive non è un controllo.
///
/// # Errori
///
/// Quelli di [`controllo_rapido`], più `db.openFailed` se il file non si apre.
pub fn controlla_il_file(percorso: &Path, entro: Duration) -> Result<Controllo, AppError> {
    let da_aprire = crate::files::percorso_lungo(percorso);
    let connection = Connection::open_with_flags(
        &da_aprire,
        OpenFlags::default().difference(OpenFlags::SQLITE_OPEN_CREATE),
    )
    .map_err(|err| {
        AppError::new(ErrorCode::DbOpenFailed {
            path: Some(percorso.display().to_string()),
        })
        .with_cause(err.to_string())
    })?;
    super::prepara_connessione_secondaria(&connection);
    let _ = connection.pragma_update(None, "query_only", "ON");
    controllo_rapido(&connection, entro).map_err(|errore| {
        // Il percorso del file controllato, non quello del contesto: di solito
        // sono lo stesso, ma questa funzione il suo lo sa per certo.
        if matches!(errore.code(), ErrorCode::DbCorrupt { .. }) {
            let causa = errore.cause().unwrap_or_default().to_owned();
            AppError::new(ErrorCode::DbCorrupt {
                path: Some(percorso.display().to_string()),
                quarantined_as: None,
            })
            .with_cause(causa)
        } else {
            errore
        }
    })
}

/// Esegue `lavoro` e lo interrompe se dura più di `entro`.
///
/// Il guardiano è un filo che aspetta su un canale: se il lavoro finisce prima,
/// il canale si chiude e il guardiano se ne va senza toccare niente; se finisce
/// il tempo, chiama `sqlite3_interrupt` e l'istruzione in corso ritorna
/// `SQLITE_INTERRUPT`. Un'interruzione che arrivasse per un soffio dopo la
/// fine non fa niente: SQLite la ignora quando non c'è niente in corso.
///
/// Se il guardiano non parte — il sistema non dà un filo in più — il lavoro
/// gira senza tempo massimo. È il comportamento di prima di questa funzione, e
/// rinunciare al controllo per questo sarebbe peggio.
fn con_scadenza<T>(
    connection: &Connection,
    entro: Duration,
    lavoro: impl FnOnce(&Connection) -> T,
) -> T {
    let maniglia = connection.get_interrupt_handle();
    let (fatto, attesa) = std::sync::mpsc::channel::<()>();
    let guardiano = std::thread::Builder::new()
        .name("rymembra-scadenza-sqlite".to_owned())
        .spawn(move || {
            if attesa.recv_timeout(entro) == Err(RecvTimeoutError::Timeout) {
                maniglia.interrupt();
            }
        });
    let esito = lavoro(connection);
    drop(fatto);
    if let Ok(guardiano) = guardiano {
        let _ = guardiano.join();
    }
    esito
}

/// Com'è andata l'istantanea di prima delle migrazioni.
///
/// Finisce nella riga di avvio del diario, ed è la sola traccia che resta: se
/// una migrazione va storta, la prima cosa da sapere è se c'è una copia da cui
/// ripartire, e dove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Istantanea {
    /// Fatta adesso.
    Fatta {
        /// Dove sta.
        percorso: PathBuf,
        /// Quanto pesa.
        byte: u64,
        /// Quanto è durata.
        millisecondi: u64,
    },
    /// C'era già una copia della stessa versione, da un avvio precedente in
    /// cui la migrazione non è andata in porto: si tiene quella, che è la copia
    /// del database **prima** di qualunque tentativo.
    Presente {
        /// Dove sta.
        percorso: PathBuf,
    },
    /// Non fatta, e perché. La migrazione procede lo stesso: l'istantanea è
    /// una rete in più, non una condizione.
    Saltata {
        /// Il perché, in una riga da diario.
        motivo: String,
    },
}

/// Il nome dell'istantanea della versione `versione` del database `percorso`.
///
/// `rymembra.db.schema-027.bak`: accanto al database, col suo nome davanti — così
/// si trovano in ordine alfabetico vicino a lui — e con la versione dello
/// **schema che contiene**, non di quello verso cui si migrava. Per tornare
/// indietro a mano: chiudere Rymembra, rinominare il database, rinominare
/// l'istantanea in `rymembra.db`, e riaprire **la versione di Rymembra di prima**,
/// perché questa la rimigrerebbe subito.
#[must_use]
pub fn nome_istantanea(percorso: &Path, versione: u32) -> PathBuf {
    let nome = percorso
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    percorso.with_file_name(format!("{nome}.schema-{versione:03}.bak"))
}

/// Quanto spazio chiedere prima di copiare, per un database di `stima` byte.
///
/// L'istantanea, più un margine: metà del database, o [`MARGINE_MINIMO`] se è
/// di più. Il margine non è per l'istantanea — quella sa quanto pesa — ma per
/// quel che viene subito dopo: la migrazione scrive nel giornale, e alcune
/// ricostruiscono una tabella intera. Un'istantanea che lascia il disco pieno
/// fino all'ultimo byte è un'istantanea che fa fallire la migrazione che
/// doveva proteggere.
fn spazio_da_chiedere(stima: u64) -> u64 {
    stima.saturating_add((stima >> 1).max(MARGINE_MINIMO))
}

/// Copia il database com'è, prima di migrarlo. Non fallisce: vedi
/// [`Istantanea::Saltata`].
///
/// # I passi
///
/// 1. se l'istantanea di questa versione c'è già, si tiene quella;
/// 2. si stima quanto peserà la copia: le pagine usate, senza quelle libere
///    che `VACUUM INTO` non copia;
/// 3. **si controlla lo spazio** chiedendolo davvero: il file temporaneo si
///    allunga fino alla stima più il margine, e su NTFS allungare un file vuol
///    dire riservargli i cluster — se il disco non li ha, risponde «disco
///    pieno» adesso invece che a metà copia. Poi lo si riaccorcia a zero, che
///    è quel che `VACUUM INTO` vuole trovare. La libreria standard non ha una
///    domanda «quanto spazio libero c'è», e chiederlo a Windows vorrebbe dire
///    codice `unsafe`, che in questo crate è vietato;
/// 4. `VACUUM INTO` nel temporaneo, e un rinomina sul nome vero: un'istantanea
///    col nome giusto è sempre intera. Se la prova dello spazio ha mentito —
///    su un filesystem che allunga i file senza riservare niente — il guasto
///    arriva qui come `SQLITE_FULL`, e si pulisce allo stesso modo;
/// 5. delle istantanee si tengono le [`ISTANTANEE_TENUTE`] più recenti.
pub(super) fn prima_di_migrare(
    connection: &Connection,
    percorso: &Path,
    versione: u32,
) -> Istantanea {
    let inizio = Instant::now();
    let destinazione = nome_istantanea(percorso, versione);
    if destinazione.is_file() {
        return Istantanea::Presente {
            percorso: destinazione,
        };
    }
    let saltata = |motivo: String| Istantanea::Saltata { motivo };

    let Some(stima) = pagine_usate(connection) else {
        return saltata("la dimensione del database non si legge".to_owned());
    };
    let mut parziale = destinazione.clone().into_os_string();
    parziale.push(".parziale");
    let parziale = PathBuf::from(parziale);
    // Un avanzo di un tentativo interrotto: `VACUUM INTO` vuole un file che
    // non c'è o che è vuoto, e questo non si sa com'è.
    let _ = std::fs::remove_file(&parziale);

    if let Err(motivo) = prova_lo_spazio(&parziale, spazio_da_chiedere(stima)) {
        let _ = std::fs::remove_file(&parziale);
        return saltata(motivo);
    }

    let Some(verso) = crate::files::percorso_lungo(&parziale)
        .to_str()
        .map(ToOwned::to_owned)
    else {
        let _ = std::fs::remove_file(&parziale);
        return saltata("il percorso della cartella dati non è UTF-8".to_owned());
    };
    if let Err(err) = connection.execute("VACUUM INTO ?1", [verso]) {
        let _ = std::fs::remove_file(&parziale);
        let motivo = if err.sqlite_error_code() == Some(CodiceSqlite::DiskFull) {
            format!("spazio esaurito durante la copia: {err}")
        } else {
            format!("copia non riuscita: {err}")
        };
        return saltata(motivo);
    }
    if let Err(err) = std::fs::rename(&parziale, &destinazione) {
        let _ = std::fs::remove_file(&parziale);
        return saltata(format!("la copia non ha preso il suo nome: {err}"));
    }

    tieni_le_ultime(percorso, &destinazione);
    Istantanea::Fatta {
        byte: std::fs::metadata(&destinazione).map_or(0, |dati| dati.len()),
        percorso: destinazione,
        millisecondi: u64::try_from(inizio.elapsed().as_millis()).unwrap_or(u64::MAX),
    }
}

/// Quanti byte occupano le pagine usate del database: quel che `VACUUM INTO`
/// scriverà, a meno di qualche pagina di indice ricompattata.
fn pagine_usate(connection: &Connection) -> Option<u64> {
    let leggi = |pragma: &str| {
        connection
            .query_row(&format!("PRAGMA {pragma}"), [], |riga| {
                riga.get::<_, i64>(0)
            })
            .ok()
            .and_then(|valore| u64::try_from(valore).ok())
    };
    let pagine = leggi("page_count")?;
    let libere = leggi("freelist_count")?;
    let dimensione = leggi("page_size")?;
    Some(pagine.saturating_sub(libere).saturating_mul(dimensione))
}

/// Chiede al disco `byte` byte per `percorso`, e li restituisce.
///
/// Il file resta, **vuoto**: è il posto in cui `VACUUM INTO` scriverà. `Err`
/// con il perché, in una riga da diario, se il disco non li ha.
fn prova_lo_spazio(percorso: &Path, byte: u64) -> Result<(), String> {
    let mebibyte = |byte: u64| byte >> 20;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(percorso)
        .map_err(|err| format!("la copia non si può creare: {err}"))?;
    file.set_len(byte).map_err(|err| {
        if matches!(
            err.kind(),
            std::io::ErrorKind::StorageFull | std::io::ErrorKind::QuotaExceeded
        ) {
            format!(
                "spazio insufficiente: servono {} MiB fra copia e margine",
                mebibyte(byte)
            )
        } else {
            format!("prova dello spazio non riuscita: {err}")
        }
    })?;
    file.set_len(0)
        .map_err(|err| format!("prova dello spazio non richiusa: {err}"))
}

/// Delle istantanee di `percorso` tiene le [`ISTANTANEE_TENUTE`] più recenti,
/// e `appena_fatta` sempre.
///
/// «Recenti» per data di modifica e non per versione: dopo il ripristino di un
/// backup vecchio si migra da una versione più bassa, e la sua istantanea è
/// quella che conta di più anche se il numero è più piccolo. Un'istantanea che
/// non si lascia cancellare resta: occupa spazio, non fa danni.
fn tieni_le_ultime(percorso: &Path, appena_fatta: &Path) {
    let Some(cartella) = percorso.parent() else {
        return;
    };
    let nome = percorso
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let prefisso = format!("{nome}.schema-");
    let Ok(voci) = std::fs::read_dir(cartella) else {
        return;
    };
    let mut istantanee: Vec<(std::time::SystemTime, PathBuf)> = voci
        .flatten()
        .filter_map(|voce| {
            let nome = voce.file_name().to_str()?.to_owned();
            let versione = nome.strip_prefix(&prefisso)?.strip_suffix(".bak")?;
            if versione.is_empty() || !versione.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let quando = voce.metadata().ok()?.modified().ok()?;
            Some((quando, voce.path()))
        })
        .collect();
    istantanee.sort_by_key(|(quando, _)| std::cmp::Reverse(*quando));
    for (_, vecchia) in istantanee.into_iter().skip(ISTANTANEE_TENUTE) {
        if vecchia != appena_fatta {
            let _ = std::fs::remove_file(vecchia);
        }
    }
}

#[cfg(test)]
mod prove {
    use super::*;
    use rymembra_domain::errors::ErrorCodeKind;

    #[test]
    fn il_controllo_di_un_database_sano_dice_sano() {
        let dir = tempfile::tempdir().expect("cartella");
        let percorso = dir.path().join("rymembra.db");
        let aperto = super::super::open(&percorso).expect("apertura");
        assert!(matches!(
            controllo_rapido(&aperto.connection, Duration::from_secs(30)),
            Ok(Controllo::Sano { .. })
        ));
        // E da una connessione sua, con la libreria aperta accanto: è il caso
        // vero, quello del filo di manutenzione.
        assert!(matches!(
            controlla_il_file(&percorso, Duration::from_secs(30)),
            Ok(Controllo::Sano { .. })
        ));
    }

    #[test]
    fn il_controllo_di_un_file_che_non_c_e_non_lo_crea() {
        let dir = tempfile::tempdir().expect("cartella");
        let percorso = dir.path().join("sparito.db");
        controlla_il_file(&percorso, Duration::from_secs(5)).expect_err("non c'è");
        assert!(
            !percorso.exists(),
            "un controllo non crea database vuoti da dichiarare sani"
        );
    }

    #[test]
    fn un_database_danneggiato_e_db_corrupt() {
        // Un database vero, con abbastanza righe da occupare decine di pagine,
        // chiuso per bene — così il giornale è tornato dentro il file — e poi
        // rovinato nella metà centrale, pagina per pagina, con della
        // spazzatura. Tante pagine e non una: una pagina sola potrebbe cadere
        // sull'elenco delle libere, che il controllo rapido non apre. È il file
        // di chi ha perso la corrente mentre il disco scriveva.
        let dir = tempfile::tempdir().expect("cartella");
        let percorso = dir.path().join("rymembra.db");
        {
            let aperto = super::super::open(&percorso).expect("apertura");
            let testo = "x".repeat(400);
            for i in 0..600 {
                aperto
                    .connection
                    .execute(
                        "INSERT INTO tracks (path, track_key, title, artist, album,
                                             file_size, date_added, date_modified, lyrics)
                         VALUES (?1, ?1, 'T', 'A', 'B', 1, 0, 0, ?2)",
                        rusqlite::params![format!("C:/m/{i}.flac"), testo],
                    )
                    .expect("riga");
            }
        }
        let mut byte = std::fs::read(&percorso).expect("lettura");
        let quarto = byte.len() >> 2;
        let da = quarto - quarto % 4096;
        for b in byte.iter_mut().skip(da).take(2 * quarto) {
            *b = 0x5a;
        }
        std::fs::write(&percorso, &byte).expect("scrittura");

        let errore =
            controlla_il_file(&percorso, Duration::from_secs(30)).expect_err("il danno si vede");
        assert_eq!(errore.code().kind(), ErrorCodeKind::DbCorrupt);
        assert_eq!(
            errore.severity(),
            rymembra_domain::errors::Severity::Fatal,
            "lo stesso codice, e la stessa gravità, di una query su un file rotto"
        );
        assert!(
            matches!(errore.code(), ErrorCode::DbCorrupt { path: Some(p), .. } if p.contains("rymembra.db")),
            "il percorso viaggia con l'errore: {:?}",
            errore.code()
        );
    }

    #[test]
    fn la_scadenza_interrompe_davvero() {
        // Una query che non finisce mai: la ricorsione senza limite. Il
        // guardiano deve fermarla, e presto.
        let connection = Connection::open_in_memory().expect("database");
        let inizio = Instant::now();
        let esito = con_scadenza(&connection, Duration::from_millis(50), |c| {
            c.query_row(
                "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n)
                 SELECT max(x) FROM n",
                [],
                |r| r.get::<_, i64>(0),
            )
        });
        let err = esito.expect_err("interrotta");
        assert_eq!(
            err.sqlite_error_code(),
            Some(CodiceSqlite::OperationInterrupted)
        );
        assert!(inizio.elapsed() < Duration::from_secs(5));

        // E l'interruzione non resta appesa alla connessione: la query dopo
        // parte e finisce.
        let uno: i64 = connection
            .query_row("SELECT 1", [], |r| r.get(0))
            .expect("la connessione è ancora buona");
        assert_eq!(uno, 1);
    }

    #[test]
    fn ottimizzare_non_rompe_niente() {
        let db = super::super::open_in_memory().expect("database");
        ottimizza(&db.connection).expect("optimize");
        ottimizza_all_apertura(&db.connection);
    }

    /// Un database su disco lasciato alla versione `versione`, con un brano.
    fn database_alla(percorso: &Path, versione: u32) {
        let connection = Connection::open(percorso).expect("apertura");
        for migrazione in super::super::MIGRATIONS
            .iter()
            .filter(|m| m.version <= versione)
        {
            connection
                .execute_batch(migrazione.sql)
                .unwrap_or_else(|err| panic!("migrazione {}: {err}", migrazione.version));
        }
        connection
            .execute_batch(&format!(
                "PRAGMA user_version = {versione};
                 INSERT INTO tracks (path, track_key, title, artist, album,
                                     file_size, date_added, date_modified)
                 VALUES ('C:/m/a.flac', 'k', 'Prima', 'A', 'B', 1, 0, 0);"
            ))
            .expect("versione e brano");
    }

    #[test]
    fn prima_di_migrare_si_fa_l_istantanea_e_se_ne_tengono_due() {
        let dir = tempfile::tempdir().expect("cartella");
        let percorso = dir.path().join("rymembra.db");
        let versione = super::super::LATEST_VERSION - 1;
        database_alla(&percorso, versione);

        // Due istantanee di aggiornamenti passati, più vecchie.
        let antiche = [
            nome_istantanea(&percorso, versione - 3),
            nome_istantanea(&percorso, versione - 2),
        ];
        for (i, antica) in antiche.iter().enumerate() {
            std::fs::write(antica, b"vecchia").expect("istantanea vecchia");
            let quando = filetime::FileTime::from_unix_time(
                1_600_000_000 + i64::try_from(i).expect("piccolo"),
                0,
            );
            filetime::set_file_mtime(antica, quando).expect("data");
        }

        let aperto = super::super::open(&percorso).expect("apertura con migrazione");
        assert_eq!(aperto.applied, 1);
        let attesa = nome_istantanea(&percorso, versione);
        match &aperto.istantanea {
            Some(Istantanea::Fatta { percorso, byte, .. }) => {
                assert_eq!(percorso, &attesa);
                assert!(*byte > 0);
            }
            altro => panic!("attesa un'istantanea fatta, arrivato {altro:?}"),
        }

        // L'istantanea è il database **com'era**: alla versione di prima, col
        // suo brano, e apribile da solo.
        let copia = Connection::open(&attesa).expect("la copia si apre");
        let (v, titolo): (i64, String) = copia
            .query_row(
                "SELECT (SELECT user_version FROM pragma_user_version), title FROM tracks",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("lettura della copia");
        assert_eq!(v, i64::from(versione));
        assert_eq!(titolo, "Prima");
        drop(copia);

        // Se ne tengono due: la nuova e la più recente delle vecchie.
        assert!(attesa.exists());
        assert!(antiche[1].exists(), "la penultima resta");
        assert!(!antiche[0].exists(), "la terza se ne va");
        // E nessun temporaneo resta in giro.
        let parziali = std::fs::read_dir(dir.path())
            .expect("cartella")
            .flatten()
            .filter(|v| v.file_name().to_string_lossy().ends_with(".parziale"))
            .count();
        assert_eq!(parziali, 0);
    }

    #[test]
    fn un_database_nuovo_o_gia_aggiornato_non_fa_istantanee() {
        let dir = tempfile::tempdir().expect("cartella");
        let percorso = dir.path().join("rymembra.db");
        // Nuovo: non c'è niente da proteggere.
        let primo = super::super::open(&percorso).expect("apertura");
        assert_eq!(primo.istantanea, None);
        drop(primo);
        // Già alla versione corrente: nessuna migrazione, nessuna copia.
        let secondo = super::super::open(&percorso).expect("riapertura");
        assert_eq!(secondo.istantanea, None);
        let copie = std::fs::read_dir(dir.path())
            .expect("cartella")
            .flatten()
            .filter(|v| v.file_name().to_string_lossy().contains(".schema-"))
            .count();
        assert_eq!(copie, 0);
    }

    #[test]
    fn un_istantanea_gia_presente_si_tiene() {
        // Un aggiornamento la cui migrazione è fallita, e un secondo avvio: la
        // copia del primo tentativo è quella del database intatto, e vale più
        // di una copia fatta adesso.
        let dir = tempfile::tempdir().expect("cartella");
        let percorso = dir.path().join("rymembra.db");
        let versione = super::super::LATEST_VERSION - 1;
        database_alla(&percorso, versione);
        let gia = nome_istantanea(&percorso, versione);
        std::fs::write(&gia, b"la copia del primo tentativo").expect("copia");

        let aperto = super::super::open(&percorso).expect("apertura");
        assert_eq!(
            aperto.istantanea,
            Some(Istantanea::Presente {
                percorso: gia.clone()
            })
        );
        assert_eq!(
            std::fs::read(&gia).expect("lettura"),
            b"la copia del primo tentativo"
        );
    }

    #[test]
    fn lo_spazio_si_chiede_con_il_margine() {
        assert_eq!(spazio_da_chiedere(0), MARGINE_MINIMO);
        assert_eq!(spazio_da_chiedere(10), 10 + MARGINE_MINIMO);
        let grande = 1024 * 1024 * 1024;
        assert_eq!(spazio_da_chiedere(grande), grande + (grande >> 1));
        assert_eq!(
            spazio_da_chiedere(u64::MAX),
            u64::MAX,
            "niente traboccamenti"
        );

        // E la prova vera lascia un file vuoto, pronto per `VACUUM INTO`.
        let dir = tempfile::tempdir().expect("cartella");
        let prova = dir.path().join("prova");
        prova_lo_spazio(&prova, 4 * 1024 * 1024).expect("quattro mebibyte ci sono");
        assert_eq!(std::fs::metadata(&prova).expect("file").len(), 0);
    }
}
