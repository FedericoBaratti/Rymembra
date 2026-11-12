//! Portare avanti quel che una scansione non sa ricostruire.
//!
//! La libreria nuova si ricostruisce dal disco: i tag stanno nei file, le
//! copertine stanno nei file, gli album si riaggregano. Quattro cose no —
//! **conteggi d'ascolto, voti, preferiti, playlist** — perché non stanno da
//! nessuna parte se non nel database che le ha registrate. Questo modulo le
//! prende dal database della versione rilasciata e le porta di là.
//!
//! # Il confronto è per chiave di brano, non per percorso
//!
//! Confrontare i percorsi sarebbe la cosa ovvia e qui non funziona: sulla
//! libreria vera **zero** dei 1707 percorsi del vecchio database esiste ancora,
//! perché il riordino ha spostato ogni file. Un importatore che confronta i
//! percorsi non fallisce — importa zero righe e riporta successo, che è il modo
//! peggiore di perdere una storia d'ascolto.
//!
//! Si confronta quindi [`TrackKey`], calcolata **dai tag** con la stessa
//! funzione da entrambe le parti. È il nome che sopravvive a uno spostamento,
//! a una ricodifica e a un cambio di dispositivo — è esattamente il mestiere
//! per cui esiste.
//!
//! # Si legge in sola lettura, e non si tocca niente di là
//!
//! Il vecchio database si apre con `mode=ro`. Non è prudenza generica: è il
//! database di un'applicazione che l'utente ha ancora installata e che
//! potrebbe riaprire domani, magari perché questa non gli piace. Un
//! importatore che ci scrive dentro — anche solo per segnare «già importato» —
//! toglie quella possibilità.
//!
//! Da cui: il segno di «già importato» non esiste, e l'importazione è invece
//! **idempotente** (vedi [`rymembra_domain::merge`]). Rifarla non raddoppia
//! niente, quindi non serve ricordarsi di averla fatta.

use std::collections::HashMap;

use rusqlite::{Connection, OpenFlags, OptionalExtension as _, Transaction};
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::keys::{PlaylistKey, TrackKey, TrackKeyInput};
use rymembra_domain::merge::{TrackStats, collapse_duplicates, merge_stats};

use crate::library::db_error;

/// Una riga del vecchio database, ridotta a ciò che vale la pena portare.
#[derive(Debug, Clone)]
struct LegacyTrack {
    id: i64,
    track_key: String,
    /// Serve solo alla diagnostica: dire *quale* brano non si è ritrovato.
    label: String,
    stats: TrackStats,
}

/// Una playlist del vecchio database.
#[derive(Debug, Clone)]
struct LegacyPlaylist {
    id: i64,
    key: String,
    name: String,
    description: Option<String>,
    created_at: i64,
    updated_at: i64,
    is_smart: bool,
    rules: Option<String>,
}

/// Cosa l'importazione porterebbe, o ha portato.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    /// Righe lette dal vecchio database.
    pub legacy_tracks: usize,
    /// Righe del vecchio database che avevano qualcosa da portare.
    pub legacy_with_stats: usize,
    /// Brani della libreria nuova che ricevono statistiche.
    pub matched: usize,
    /// Righe del vecchio database la cui chiave non esiste più in libreria.
    ///
    /// Non è un errore ed è la voce più importante del rapporto: sono brani che
    /// l'utente ascoltava e che dal disco sono spariti. Vanno **detti**, non
    /// contati e basta, altrimenti l'importazione dichiara successo mentre
    /// lascia indietro cinquanta ascolti.
    pub unmatched: Vec<String>,
    /// Ascolti totali portati (la somma dei conteggi dopo la fusione).
    pub play_count_carried: i64,
    /// Voti portati.
    pub ratings_carried: usize,
    /// Preferiti portati.
    pub liked_carried: usize,
    /// Righe di cronologia inserite (le già presenti non si duplicano).
    pub history_rows: usize,
    /// Righe di cronologia saltate perché il loro brano non c'è più.
    pub history_orphans: usize,
    /// Playlist create o aggiornate.
    pub playlists: usize,
    /// Voci di playlist inserite.
    pub playlist_entries: usize,
    /// Voci di playlist saltate perché il brano non c'è più.
    pub playlist_orphans: usize,
    /// Lapidi di sincronizzazione portate.
    pub tombstones: usize,
    /// Accanto al file c'è un giornale `-wal` con delle pagine, e non si è
    /// letto: il vecchio database si apre `immutable` (vedi [`open_legacy`]).
    ///
    /// Di solito vuol dire che la vecchia app è aperta, o si è chiusa male, e
    /// le sue ultime modifiche stanno ancora lì. Si dice nel piano perché il
    /// gesto che ripara — chiuderla e riaprire l'importazione — è dell'utente.
    pub journal_ignored: bool,
}

/// Le tabelle che l'importazione legge dal vecchio database.
///
/// Tutte e cinque devono essere **tabelle vere** se ci sono: vedi
/// [`open_legacy`].
const TABELLE_LETTE: [&str; 5] = [
    "tracks",
    "play_history",
    "playlists",
    "playlist_tracks",
    "sync_tombstones",
];

/// Apre il vecchio database in sola lettura, e solo il suo file principale.
///
/// # Perché come un file che arriva da fuori
///
/// Perché lo è. Il percorso viene da un selettore che accetta qualunque `.db`,
/// `.sqlite` o `.sqlite3`, e «ecco la mia libreria della 1.0» può essere un file
/// ricevuto da qualcuno (R06-11). Le tre difese, in quest'ordine:
///
/// - **`immutable=1`**: SQLite legge il file principale e basta, senza mai
///   guardare `-wal` e `-shm`. Un giornale costruito ad arte accanto al file è
///   il caso di RUSTSEC-2026-0459 (memoria non inizializzata nei risultati,
///   SQLite prima della 3.54), e così non si apre nemmeno. Il prezzo: le
///   modifiche che la vecchia app tiene ancora nel giornale — perché è aperta, o
///   si è chiusa male — non si vedono. Il piano lo dice (vedi
///   [`ImportReport::journal_ignored`]).
/// - **`trusted_schema=OFF`** e **`query_only`**: niente funzioni con effetti
///   dentro lo schema, e nessuna scrittura per nessuna strada.
/// - **Tabelle vere**: le cinque che si leggono, se ci sono, devono essere
///   tabelle con le loro pagine (`type = 'table'`, `rootpage > 0`). Una vista
///   farebbe girare la sua interrogazione — anche una costosa — mentre il
///   lucchetto della libreria è preso; una tabella virtuale porterebbe il
///   codice del suo modulo.
///
/// # Errori
///
/// `db.openFailed` se il file non si apre, o se non ha la forma di un vecchio
/// database.
pub fn open_legacy(path: &std::path::Path) -> Result<Connection, AppError> {
    let fallito = |causa: String| {
        AppError::new(ErrorCode::DbOpenFailed {
            path: Some(path.display().to_string()),
        })
        .with_cause(causa)
    };
    let connection = Connection::open_with_flags(
        uri_immutabile(path),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|err| fallito(err.to_string()))?;
    connection
        .execute_batch("PRAGMA trusted_schema = OFF; PRAGMA query_only = 1;")
        .map_err(|err| fallito(err.to_string()))?;
    for tabella in TABELLE_LETTE {
        let forma: Option<(String, i64)> = connection
            .query_row(
                "SELECT type, rootpage FROM sqlite_master WHERE name = ?1 COLLATE NOCASE",
                [tabella],
                |r| Ok((r.get(0)?, r.get::<_, Option<i64>>(1)?.unwrap_or(0))),
            )
            .optional()
            .map_err(|err| fallito(err.to_string()))?;
        if let Some((tipo, pagina)) = forma
            && (tipo != "table" || pagina <= 0)
        {
            return Err(fallito(format!(
                "«{tabella}» non è una tabella ({tipo}): non è un database della versione di prima"
            )));
        }
    }
    Ok(connection)
}

/// L'indirizzo `file:` con cui SQLite apre `path` senza il suo giornale.
///
/// SQLite accetta `immutable` solo dentro un URI, e un URI vuole le barre
/// dritte e tre caratteri scritti in percentuale: `%`, `?` e `#` (il resto,
/// spazi e lettere accentate comprese, passa così com'è in UTF-8). Su Windows
/// un disco diventa `file:///C:/…` e una condivisione `file:////server/…`: due
/// barre dopo `file:` aprirebbero un'autorità, e SQLite accetta solo
/// `localhost`. Il prefisso esteso `\\?\` si toglie: dentro un URI non vuol
/// dire niente.
fn uri_immutabile(path: &std::path::Path) -> String {
    let grezzo = path.to_string_lossy();
    let senza_prefisso = grezzo
        .strip_prefix(r"\\?\UNC\")
        .map(|resto| format!(r"\\{resto}"))
        .unwrap_or_else(|| grezzo.strip_prefix(r"\\?\").unwrap_or(&grezzo).to_owned());
    let dritto = if cfg!(windows) {
        senza_prefisso.replace('\\', "/")
    } else {
        senza_prefisso
    };
    let mut scritto = String::with_capacity(dritto.len() + 24);
    for c in dritto.chars() {
        match c {
            '%' => scritto.push_str("%25"),
            '?' => scritto.push_str("%3F"),
            '#' => scritto.push_str("%23"),
            altro => scritto.push(altro),
        }
    }
    let disco = scritto.as_bytes().get(1).is_some_and(|b| *b == b':')
        && scritto
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic);
    let davanti = if scritto.starts_with("//") {
        // Una condivisione: l'autorità vuota, e il percorso che comincia con
        // le sue due barre.
        "file://"
    } else if disco {
        "file:///"
    } else if scritto.starts_with('/') {
        "file://"
    } else {
        "file:"
    };
    format!("{davanti}{scritto}?immutable=1")
}

/// C'è un giornale `-wal` non vuoto accanto al file?
///
/// Un `-wal` di zero byte è quel che resta di una chiusura pulita con il
/// giornale `TRUNCATE`: non porta niente, e dirlo sarebbe un falso allarme.
fn giornale_in_sospeso(file: &std::path::Path) -> bool {
    if file.as_os_str().is_empty() {
        return false;
    }
    let mut giornale = file.as_os_str().to_owned();
    giornale.push("-wal");
    std::fs::metadata(&giornale).is_ok_and(|m| m.len() > 0)
}

/// Una colonna che nel vecchio database può non esserci.
///
/// Diciotto migrazioni accumulate significano che un database fermo a una
/// versione intermedia ha meno colonne di uno aggiornato. Chiedere una colonna
/// che non c'è fa fallire l'intera query, cioè fa fallire l'importazione per
/// intero invece di portare quel che c'è.
///
/// # Perché `PRAGMA table_info` e non un `SELECT` di prova
///
/// La versione ovvia — preparare `SELECT "colonna" FROM tabella LIMIT 0` e
/// guardare se fallisce — **non funziona**, e non fallisce mai. SQLite accetta
/// una stringa fra virgolette doppie che non risolve a un identificatore e la
/// tratta come **stringa letterale**: `SELECT "liked" FROM tracks` su una
/// tabella senza quella colonna restituisce la parola «liked» per ogni riga,
/// senza un errore.
///
/// È una compatibilità storica che SQLite si porta dietro apposta, e qui
/// avrebbe fatto rispondere «sì, c'è» per qualunque colonna — cioè avrebbe
/// lasciato morto proprio il ramo che serve ai database vecchi, che è l'unico
/// motivo per cui questa funzione esiste. Trovata da un test.
fn has_column(connection: &Connection, table: &str, column: &str) -> bool {
    connection
        .prepare(&format!("PRAGMA table_info(\"{table}\")"))
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(1))
                .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
        })
        .is_ok_and(|columns| columns.iter().any(|name| name == column))
}

/// Legge dal vecchio database quel che vale la pena portare.
fn read_legacy_tracks(legacy: &Connection) -> Result<Vec<LegacyTrack>, AppError> {
    // `liked`, `liked_at` e `stats_updated_at` sono arrivati tardi nella catena
    // delle migrazioni del vecchio albero: un database non aggiornato non le ha.
    let liked = has_column(legacy, "tracks", "liked");
    let liked_at = has_column(legacy, "tracks", "liked_at");
    let stats_updated = has_column(legacy, "tracks", "stats_updated_at");

    let sql = format!(
        "SELECT id, artist, title, album, play_count, last_played, rating, {}, {}, {}
         FROM tracks ORDER BY id",
        if liked { "liked" } else { "0" },
        if liked_at { "liked_at" } else { "NULL" },
        if stats_updated {
            "stats_updated_at"
        } else {
            "0"
        },
    );

    let mut statement = legacy
        .prepare(&sql)
        .map_err(|err| db_error("lettura dei brani dal vecchio database", &err))?;
    let rows = statement
        .query_map([], |row| {
            let artist: Option<String> = row.get(1)?;
            let title: Option<String> = row.get(2)?;
            let album: Option<String> = row.get(3)?;
            let track_key = TrackKey::compute(TrackKeyInput {
                artist: artist.as_deref(),
                title: title.as_deref(),
                album: album.as_deref(),
            })
            .into_string();
            Ok(LegacyTrack {
                id: row.get(0)?,
                track_key,
                label: format!(
                    "{} — {}",
                    artist.as_deref().unwrap_or("?"),
                    title.as_deref().unwrap_or("?")
                ),
                stats: TrackStats {
                    play_count: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    last_played_at: row.get(5)?,
                    rating: row
                        .get::<_, Option<i64>>(6)?
                        .unwrap_or(0)
                        .clamp(0, 5)
                        .try_into()
                        .unwrap_or(0),
                    liked: row.get::<_, Option<i64>>(7)?.unwrap_or(0) != 0,
                    liked_at: row.get(8)?,
                    stats_updated_at: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                },
            })
        })
        .map_err(|err| db_error("lettura dei brani dal vecchio database", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("lettura dei brani dal vecchio database", &err))
}

/// Ha qualcosa da portare? Una riga senza ascolti, voto e preferito non ha.
fn is_worth_carrying(stats: &TrackStats) -> bool {
    stats.play_count > 0 || stats.rating > 0 || stats.liked || stats.last_played_at.is_some()
}

/// Da chiave di brano all'identificativo della riga nella libreria nuova.
///
/// Una chiave può nominare più righe (lo stesso brano in due formati). Si tiene
/// l'identificativo più basso — la riga più vecchia, quella a cui puntano più
/// probabilmente playlist e cronologia già esistenti — e le altre si elencano,
/// perché ricevano le stesse statistiche: sono lo stesso brano, e vedere due
/// copie con conteggi diversi è un modo di sembrare rotti.
fn index_by_key(connection: &Connection) -> Result<HashMap<String, Vec<i64>>, AppError> {
    let mut statement = connection
        .prepare("SELECT track_key, id FROM tracks ORDER BY id")
        .map_err(|err| db_error("indice per chiave di brano", &err))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|err| db_error("indice per chiave di brano", &err))?;
    let mut index: HashMap<String, Vec<i64>> = HashMap::new();
    for row in rows {
        let (key, id) = row.map_err(|err| db_error("indice per chiave di brano", &err))?;
        index.entry(key).or_default().push(id);
    }
    Ok(index)
}

/// Le statistiche già presenti nella libreria nuova, per identificativo.
fn current_stats(connection: &Connection) -> Result<HashMap<i64, TrackStats>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT id, play_count, last_played_at, rating, liked, liked_at, stats_updated_at
             FROM tracks",
        )
        .map_err(|err| db_error("statistiche correnti", &err))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                TrackStats {
                    play_count: row.get(1)?,
                    last_played_at: row.get(2)?,
                    rating: row.get::<_, i64>(3)?.clamp(0, 5).try_into().unwrap_or(0),
                    liked: row.get::<_, i64>(4)? != 0,
                    liked_at: row.get(5)?,
                    stats_updated_at: row.get(6)?,
                },
            ))
        })
        .map_err(|err| db_error("statistiche correnti", &err))?;
    rows.collect::<Result<HashMap<_, _>, _>>()
        .map_err(|err| db_error("statistiche correnti", &err))
}

/// Cosa succederebbe importando, senza scrivere niente.
///
/// Il piano prima dell'esecuzione, come per la scansione e per il ripristino. Qui
/// serve soprattutto a una cosa: leggere l'elenco di [`ImportReport::unmatched`]
/// **prima** di decidere, perché è l'unico momento in cui si può ancora andare a
/// cercare quei file sul disco.
///
/// # Il piano è l'esecuzione, annullata
///
/// Non è una seconda implementazione che prevede cosa farebbe la prima: è la
/// prima, dentro una transazione che viene abbandonata. Costa quanto
/// l'importazione vera — qualche migliaio di righe da un file locale, niente —
/// e in cambio il piano **non può** dire una cosa diversa da quel che poi
/// succede.
///
/// Ci sono arrivato per la via lunga: la prima versione era una funzione a
/// parte che contava soltanto i brani, e sul database vero annunciava
/// «cronologia 0 righe» mentre di righe da portare ce n'erano 92. Non aveva
/// torto su un calcolo — semplicemente non guardava lì, e chi legge un piano
/// non ha modo di distinguere «zero» da «non l'ho controllato».
///
/// Richiede `&mut` perché apre una transazione per davvero, e quel `&mut` è
/// l'unica traccia onesta di cosa sta facendo.
pub fn plan_import(
    legacy: &Connection,
    connection: &mut Connection,
) -> Result<ImportReport, AppError> {
    run(legacy, connection, false)
}

/// Porta le statistiche, la cronologia, le playlist e le lapidi.
///
/// Tutto dentro una transazione sola, al contrario della scansione: qui non c'è
/// niente di lento — si leggono qualche migliaio di righe da un file locale — e
/// un'importazione a metà lascerebbe una libreria in cui non si sa più quali
/// ascolti sono arrivati e quali no. Dove non c'è un motivo per accettare un
/// esito parziale, non lo si accetta.
pub fn import(legacy: &Connection, connection: &mut Connection) -> Result<ImportReport, AppError> {
    run(legacy, connection, true)
}

/// L'importazione. `commit` decide se resta.
fn run(
    legacy: &Connection,
    connection: &mut Connection,
    commit: bool,
) -> Result<ImportReport, AppError> {
    let legacy_tracks = read_legacy_tracks(legacy)?;
    let index = index_by_key(connection)?;
    let attuali = current_stats(connection)?;

    let mut report = ImportReport {
        legacy_tracks: legacy_tracks.len(),
        journal_ignored: legacy
            .path()
            .is_some_and(|file| giornale_in_sospeso(std::path::Path::new(file))),
        ..ImportReport::default()
    };

    // Le righe del vecchio database raggruppate per chiave: due file dello
    // stesso brano hanno due storie che vanno riunite prima del confronto.
    let mut by_key: HashMap<&str, Vec<&LegacyTrack>> = HashMap::new();
    for track in &legacy_tracks {
        if is_worth_carrying(&track.stats) {
            report.legacy_with_stats += 1;
            by_key.entry(&track.track_key).or_default().push(track);
        }
    }

    // Da identificativo del vecchio database a identificativo nuovo: serve a
    // rimappare cronologia e playlist, che puntano agli id di là.
    let mut remap: HashMap<i64, i64> = HashMap::new();
    for track in &legacy_tracks {
        if let Some(first) = index.get(&track.track_key).and_then(|ids| ids.first()) {
            remap.insert(track.id, *first);
        }
    }

    let tx = crate::library::transazione_di_scrittura(connection)
        .map_err(|err| db_error("apertura della transazione", &err))?;

    // ── statistiche ──
    {
        let mut statement = tx
            .prepare_cached(
                // `rating_at` con il voto, con lo stesso orologio con cui
                // `merge_stats` l'ha scelto: vedi `backup::applica_brani`.
                "UPDATE tracks SET play_count = ?2, last_played_at = ?3, rating = ?4,
                                   liked = ?5, liked_at = ?6, stats_updated_at = ?7,
                                   rating_at = CASE WHEN rating <> ?4
                                                      OR (rating_at IS NULL AND ?4 > 0)
                                                    THEN ?7 ELSE rating_at END
                 WHERE id = ?1",
            )
            .map_err(|err| db_error("scrittura delle statistiche", &err))?;
        // Ordinate per chiave: l'elenco di quel che non si ritrova va mostrato
        // all'utente, e due esecuzioni non devono darne due versioni diverse.
        let mut chiavi: Vec<&&str> = by_key.keys().collect();
        chiavi.sort_unstable();
        for key in chiavi {
            let Some(group) = by_key.get(*key) else {
                continue;
            };
            let stats: Vec<TrackStats> = group.iter().map(|t| t.stats).collect();
            let dal_vecchio = collapse_duplicates(&stats);

            let Some(ids) = index.get(*key) else {
                for track in group {
                    report.unmatched.push(format!(
                        "{} ({} ascolti)",
                        track.label, track.stats.play_count
                    ));
                }
                continue;
            };

            report.matched += ids.len();
            report.play_count_carried += dal_vecchio.play_count;
            if dal_vecchio.rating > 0 {
                report.ratings_carried += ids.len();
            }
            if dal_vecchio.liked {
                report.liked_carried += ids.len();
            }

            for id in ids {
                let locale = attuali.get(id).copied().unwrap_or_default();
                let fuse = merge_stats(&locale, &dal_vecchio);
                statement
                    .execute(rusqlite::params![
                        id,
                        fuse.play_count,
                        fuse.last_played_at,
                        i64::from(fuse.rating),
                        i64::from(fuse.liked),
                        fuse.liked_at,
                        fuse.stats_updated_at,
                    ])
                    .map_err(|err| db_error("scrittura delle statistiche", &err))?;
            }
        }
    }

    import_history(legacy, &tx, &remap, &mut report)?;
    import_playlists(legacy, &tx, &remap, &mut report)?;
    import_tombstones(legacy, &tx, &mut report)?;

    if commit {
        tx.commit()
            .map_err(|err| db_error("chiusura della transazione", &err))?;
    } else {
        // Abbandonata: `Transaction` annulla quando viene lasciata cadere, ma
        // scriverlo per esteso toglie la domanda a chi legge.
        tx.rollback()
            .map_err(|err| db_error("annullamento del piano", &err))?;
    }
    report.unmatched.sort();
    Ok(report)
}

/// Porta la cronologia d'ascolto, senza duplicare le righe già presenti.
///
/// La chiave di un ascolto è la coppia (brano, istante): due ascolti dello
/// stesso brano nello stesso millisecondo sono lo stesso ascolto contato due
/// volte, e senza questa guardia rilanciare l'importazione gonfierebbe le
/// statistiche e lo smart shuffle — che decide cosa non riproporre proprio
/// guardando qui.
fn import_history(
    legacy: &Connection,
    tx: &Transaction<'_>,
    remap: &HashMap<i64, i64>,
    report: &mut ImportReport,
) -> Result<(), AppError> {
    let mut lettura = legacy
        .prepare("SELECT track_id, played_at, ms_played FROM play_history ORDER BY played_at, id")
        .map_err(|err| db_error("lettura della cronologia", &err))?;
    let righe = lettura
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?.unwrap_or(0),
            ))
        })
        .map_err(|err| db_error("lettura della cronologia", &err))?;

    let mut scrittura = tx
        .prepare_cached(
            "INSERT INTO play_history (track_id, played_at, ms_played)
             SELECT ?1, ?2, ?3
             WHERE NOT EXISTS (
               SELECT 1 FROM play_history WHERE track_id = ?1 AND played_at = ?2
             )",
        )
        .map_err(|err| db_error("scrittura della cronologia", &err))?;

    for riga in righe {
        let (legacy_id, played_at, ms_played) =
            riga.map_err(|err| db_error("lettura della cronologia", &err))?;
        let Some(track_id) = remap.get(&legacy_id) else {
            report.history_orphans += 1;
            continue;
        };
        report.history_rows += scrittura
            .execute(rusqlite::params![track_id, played_at, ms_played])
            .map_err(|err| db_error("scrittura della cronologia", &err))?;
    }
    Ok(())
}

/// Porta le playlist e la loro composizione.
///
/// # Le playlist automatiche non portano le voci
///
/// Una playlist automatica è definita dalle sue **regole**, non dal suo
/// contenuto: ogni dispositivo la ricalcola sui brani che ha davvero. Copiarne
/// l'appartenenza salvata la congelerebbe su una fotografia della vecchia
/// libreria, e resterebbe congelata finché qualcuno non tocca le regole — cioè
/// una playlist «tutti i Subsonica» che non mostra i Subsonica aggiunti dopo.
///
/// Le regole si portano come sono, senza interpretarle: chi le valuta è un
/// altro pezzo, e un importatore che le riscrivesse a modo suo introdurrebbe un
/// secondo formato da tenere allineato.
fn import_playlists(
    legacy: &Connection,
    tx: &Transaction<'_>,
    remap: &HashMap<i64, i64>,
    report: &mut ImportReport,
) -> Result<(), AppError> {
    let has_smart = has_column(legacy, "playlists", "is_smart");
    let sql = format!(
        "SELECT id, name, description, created_at, updated_at, {}, {}
         FROM playlists ORDER BY id",
        if has_smart { "is_smart" } else { "0" },
        if has_column(legacy, "playlists", "rules") {
            "rules"
        } else {
            "NULL"
        },
    );
    let mut lettura = legacy
        .prepare(&sql)
        .map_err(|err| db_error("lettura delle playlist", &err))?;
    let righe = lettura
        .query_map([], |row| {
            let name: String = row.get::<_, Option<String>>(1)?.unwrap_or_default();
            Ok(LegacyPlaylist {
                id: row.get(0)?,
                key: PlaylistKey::compute(Some(&name)).into_string(),
                name,
                description: row.get(2)?,
                created_at: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                updated_at: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                is_smart: row.get::<_, Option<i64>>(5)?.unwrap_or(0) != 0,
                rules: row.get(6)?,
            })
        })
        .map_err(|err| db_error("lettura delle playlist", &err))?;
    let playlists = righe
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("lettura delle playlist", &err))?;

    for playlist in &playlists {
        // La chiave è il nome normalizzato ed è UNIQUE: una playlist già
        // importata si aggiorna invece di far fallire l'inserimento. È l'altra
        // metà dell'idempotenza — senza, la seconda importazione cade qui.
        tx.prepare_cached(
            "INSERT INTO playlists (playlist_key, name, description, created_at, updated_at,
                                    is_smart, rules)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(playlist_key) DO UPDATE SET
               name = excluded.name, description = excluded.description,
               updated_at = MAX(playlists.updated_at, excluded.updated_at),
               is_smart = excluded.is_smart, rules = excluded.rules",
        )
        .and_then(|mut s| {
            s.execute(rusqlite::params![
                playlist.key,
                playlist.name,
                playlist.description,
                playlist.created_at,
                playlist.updated_at,
                i64::from(playlist.is_smart),
                playlist.rules,
            ])
        })
        .map_err(|err| db_error("scrittura di una playlist", &err))?;
        report.playlists += 1;

        if playlist.is_smart {
            continue;
        }

        let nuovo_id: i64 = tx
            .query_row(
                "SELECT id FROM playlists WHERE playlist_key = ?1",
                [&playlist.key],
                |row| row.get(0),
            )
            .map_err(|err| db_error("identificativo di una playlist", &err))?;

        let mut lettura_voci = legacy
            .prepare(
                "SELECT track_id, position FROM playlist_tracks
                 WHERE playlist_id = ?1 ORDER BY position",
            )
            .map_err(|err| db_error("lettura delle voci di playlist", &err))?;
        let voci = lettura_voci
            .query_map([playlist.id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|err| db_error("lettura delle voci di playlist", &err))?;

        // Prima si svuota: è quel che fanno tutti gli altri scrittori di
        // playlist (`riscrivi_ordine`, il ripristino, la sincronia). Senza, una
        // playlist omonima già presente con più voci di quante la legacy ne
        // porti sopravvivrebbe oltre l'ultima riga scritta, e il risultato
        // sarebbe un ibrido di due fonti che nessuno ha chiesto.
        tx.execute(
            "DELETE FROM playlist_tracks WHERE playlist_id = ?1",
            [nuovo_id],
        )
        .map_err(|err| db_error("svuotamento di una playlist da migrare", &err))?;

        // Le posizioni si rinumerano da zero e non si copiano: saltando i brani
        // che non ci sono più resterebbero dei buchi, e `position` è parte della
        // chiave primaria — una playlist con posizioni 0, 3, 7 non è rotta, ma
        // la prima riordinata dall'utente lo diventerebbe.
        let mut scrittura = tx
            .prepare_cached(
                "INSERT OR REPLACE INTO playlist_tracks (playlist_id, track_id, position)
                 VALUES (?1, ?2, ?3)",
            )
            .map_err(|err| db_error("scrittura di una voce di playlist", &err))?;
        let mut posizione: i64 = 0;
        for voce in voci {
            let (legacy_track_id, _) =
                voce.map_err(|err| db_error("lettura delle voci di playlist", &err))?;
            let Some(track_id) = remap.get(&legacy_track_id) else {
                report.playlist_orphans += 1;
                continue;
            };
            scrittura
                .execute(rusqlite::params![nuovo_id, track_id, posizione])
                .map_err(|err| db_error("scrittura di una voce di playlist", &err))?;
            posizione += 1;
            report.playlist_entries += 1;
        }
    }
    Ok(())
}

/// Porta le lapidi: le cancellazioni che l'utente ha deciso davvero.
///
/// Sono l'opposto di quelle che una scansione **non** scrive. Qui vanno portate
/// proprio perché sono decisioni: senza, un brano tolto a mano prima della
/// migrazione tornerebbe dall'altro dispositivo alla prima sincronizzazione.
fn import_tombstones(
    legacy: &Connection,
    tx: &Transaction<'_>,
    report: &mut ImportReport,
) -> Result<(), AppError> {
    let mut lettura = legacy
        .prepare("SELECT kind, key, deleted_at FROM sync_tombstones")
        .map_err(|err| db_error("lettura delle lapidi", &err))?;
    let righe = lettura
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?.unwrap_or(0),
            ))
        })
        .map_err(|err| db_error("lettura delle lapidi", &err))?;

    let mut scrittura = tx
        .prepare_cached(
            "INSERT INTO sync_tombstones (kind, key, deleted_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(kind, key) DO UPDATE SET
               deleted_at = MAX(sync_tombstones.deleted_at, excluded.deleted_at)",
        )
        .map_err(|err| db_error("scrittura delle lapidi", &err))?;
    for riga in righe {
        let (kind, key, deleted_at) = riga.map_err(|err| db_error("lettura delle lapidi", &err))?;
        // Il vincolo dello schema nuovo accetta solo 'track' e 'playlist': una
        // lapide di un tipo che non conosciamo si salta invece di far fallire
        // tutta l'importazione per una riga.
        if kind != "track" && kind != "playlist" {
            continue;
        }
        report.tombstones += scrittura
            .execute(rusqlite::params![kind, key, deleted_at])
            .map_err(|err| db_error("scrittura delle lapidi", &err))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un vecchio database con lo schema della versione rilasciata, ridotto ai
    /// pezzi che l'importatore guarda.
    fn vecchio() -> Connection {
        let c = Connection::open_in_memory().expect("memoria");
        c.execute_batch(
            "CREATE TABLE tracks (
               id INTEGER PRIMARY KEY, path TEXT, title TEXT, artist TEXT, album TEXT,
               play_count INTEGER DEFAULT 0, last_played INTEGER, rating INTEGER DEFAULT 0,
               liked INTEGER DEFAULT 0, liked_at INTEGER, stats_updated_at INTEGER DEFAULT 0);
             CREATE TABLE play_history (
               id INTEGER PRIMARY KEY, track_id INTEGER, played_at INTEGER, ms_played INTEGER);
             CREATE TABLE playlists (
               id INTEGER PRIMARY KEY, name TEXT, description TEXT, created_at INTEGER,
               updated_at INTEGER, is_smart INTEGER DEFAULT 0, rules TEXT);
             CREATE TABLE playlist_tracks (
               playlist_id INTEGER, track_id INTEGER, position INTEGER);
             CREATE TABLE sync_tombstones (kind TEXT, key TEXT, deleted_at INTEGER);",
        )
        .expect("schema vecchio");
        c
    }

    /// La libreria nuova, con dei brani già scansionati.
    fn nuovo(brani: &[(&str, &str, &str, &str)]) -> Connection {
        let c = crate::db::open_in_memory().expect("database").connection;
        for (index, (path, title, artist, album)) in brani.iter().enumerate() {
            let key = TrackKey::compute(TrackKeyInput {
                artist: Some(artist),
                title: Some(title),
                album: Some(album),
            })
            .into_string();
            c.execute(
                "INSERT INTO tracks (id, path, track_key, title, artist, album,
                                     file_size, date_added, date_modified)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 5000000, 1, 1)",
                rusqlite::params![
                    i64::try_from(index).unwrap_or(0) + 1,
                    path,
                    key,
                    title,
                    artist,
                    album
                ],
            )
            .expect("brano");
        }
        c
    }

    fn aggiungi_vecchio(
        c: &Connection,
        id: i64,
        path: &str,
        title: &str,
        artist: &str,
        album: &str,
        play_count: i64,
    ) {
        c.execute(
            "INSERT INTO tracks (id, path, title, artist, album, play_count, last_played)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1000)",
            rusqlite::params![id, path, title, artist, album, play_count],
        )
        .expect("brano vecchio");
    }

    #[test]
    fn un_percorso_cambiato_non_perde_la_storia_d_ascolto() {
        // Il caso vero: dopo il riordino nessun percorso del vecchio database
        // esiste più. Confrontando i percorsi si importerebbe zero.
        let legacy = vecchio();
        aggiungi_vecchio(
            &legacy,
            1,
            r"C:\Vecchio\posto\Quello che.mp3",
            "Quello che",
            "99 Posse",
            "Corto circuito",
            15,
        );
        let mut db = nuovo(&[(
            r"C:\Music\99 Posse\Corto circuito\01.mp3",
            "Quello che",
            "99 Posse",
            "Corto circuito",
        )]);

        let esito = import(&legacy, &mut db).expect("importazione");
        assert_eq!(esito.matched, 1);
        assert!(esito.unmatched.is_empty());

        let ascolti: i64 = db
            .query_row("SELECT play_count FROM tracks", [], |r| r.get(0))
            .expect("ascolti");
        assert_eq!(ascolti, 15);
    }

    #[test]
    fn rilanciare_l_importazione_non_raddoppia_niente() {
        let legacy = vecchio();
        aggiungi_vecchio(&legacy, 1, "x", "T", "A", "Al", 15);
        legacy
            .execute(
                "INSERT INTO play_history (track_id, played_at, ms_played) VALUES (1, 5000, 200)",
                [],
            )
            .expect("cronologia");
        let mut db = nuovo(&[("p", "T", "A", "Al")]);

        let prima = import(&legacy, &mut db).expect("prima");
        let seconda = import(&legacy, &mut db).expect("seconda");

        assert_eq!(prima.history_rows, 1);
        assert_eq!(seconda.history_rows, 0, "la seconda non reinserisce");
        let (ascolti, righe): (i64, i64) = db
            .query_row(
                "SELECT (SELECT play_count FROM tracks), (SELECT COUNT(*) FROM play_history)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("conteggi");
        assert_eq!(ascolti, 15);
        assert_eq!(righe, 1);
    }

    #[test]
    fn quel_che_non_si_ritrova_viene_detto() {
        // Il difetto peggiore possibile per un importatore: dichiarare successo
        // avendo lasciato indietro cinquanta ascolti.
        let legacy = vecchio();
        aggiungi_vecchio(&legacy, 1, "x", "C'e", "A", "Al", 3);
        aggiungi_vecchio(&legacy, 2, "y", "Sparito", "B", "Bl", 50);
        let mut db = nuovo(&[("p", "C'e", "A", "Al")]);

        let esito = import(&legacy, &mut db).expect("importazione");
        assert_eq!(esito.matched, 1);
        assert_eq!(esito.unmatched.len(), 1);
        assert!(
            esito
                .unmatched
                .first()
                .is_some_and(|s| s.contains("Sparito") && s.contains("50")),
            "{:?}",
            esito.unmatched
        );
    }

    #[test]
    fn due_file_dello_stesso_brano_uniscono_gli_ascolti() {
        let legacy = vecchio();
        aggiungi_vecchio(&legacy, 1, "a.mp3", "T", "A", "Al", 4);
        aggiungi_vecchio(&legacy, 2, "a.flac", "T", "A", "Al", 6);
        let mut db = nuovo(&[("p", "T", "A", "Al")]);

        import(&legacy, &mut db).expect("importazione");
        let ascolti: i64 = db
            .query_row("SELECT play_count FROM tracks", [], |r| r.get(0))
            .expect("ascolti");
        assert_eq!(ascolti, 10);
    }

    #[test]
    fn le_statistiche_gia_presenti_non_si_perdono() {
        // Chi ha già usato la versione nuova prima di importare non deve
        // vedersi azzerare quel che ha fatto qui.
        let legacy = vecchio();
        aggiungi_vecchio(&legacy, 1, "x", "T", "A", "Al", 5);
        let mut db = nuovo(&[("p", "T", "A", "Al")]);
        db.execute("UPDATE tracks SET play_count = 20, rating = 5", [])
            .expect("uso locale");

        import(&legacy, &mut db).expect("importazione");
        let (ascolti, voto): (i64, i64) = db
            .query_row("SELECT play_count, rating FROM tracks", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .expect("riga");
        assert_eq!(ascolti, 20, "il conteggio più alto sopravvive");
        assert_eq!(voto, 5);
    }

    #[test]
    fn un_voto_importato_porta_la_sua_data() {
        // Come nel ripristino da backup: senza `rating_at` la sincronia non
        // pubblica il voto, e qualunque voto remoto lo sovrascrive.
        let legacy = vecchio();
        aggiungi_vecchio(&legacy, 1, "x", "T", "A", "Al", 5);
        legacy
            .execute("UPDATE tracks SET rating = 4, stats_updated_at = 7000", [])
            .expect("voto vecchio");
        let mut db = nuovo(&[("p", "T", "A", "Al")]);

        import(&legacy, &mut db).expect("importazione");
        let (voto, quando): (i64, Option<i64>) = db
            .query_row("SELECT rating, rating_at FROM tracks", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .expect("riga");
        assert_eq!((voto, quando), (4, Some(7000)));
    }

    #[test]
    fn una_playlist_automatica_porta_le_regole_e_non_le_voci() {
        // Congelarne il contenuto vorrebbe dire una playlist «tutti i
        // Subsonica» che non mostra i Subsonica aggiunti dopo.
        let legacy = vecchio();
        aggiungi_vecchio(&legacy, 1, "x", "T", "Subsonica", "Al", 1);
        let regole = r#"{"combinator":"and","rules":[{"field":"artist","op":"contains","value":"subsonica"}]}"#;
        legacy
            .execute(
                "INSERT INTO playlists (id, name, created_at, updated_at, is_smart, rules)
                 VALUES (1, 'Subsonica', 10, 20, 1, ?1)",
                [regole],
            )
            .expect("playlist");
        legacy
            .execute(
                "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (1, 1, 0)",
                [],
            )
            .expect("voce");
        let mut db = nuovo(&[("p", "T", "Subsonica", "Al")]);

        let esito = import(&legacy, &mut db).expect("importazione");
        assert_eq!(esito.playlists, 1);
        assert_eq!(esito.playlist_entries, 0, "le voci non si copiano");

        let (nome, smart, salvate): (String, i64, Option<String>) = db
            .query_row("SELECT name, is_smart, rules FROM playlists", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .expect("playlist");
        assert_eq!(nome, "Subsonica");
        assert_eq!(smart, 1);
        assert_eq!(
            salvate.as_deref(),
            Some(regole),
            "le regole passano intatte"
        );
    }

    #[test]
    fn una_playlist_normale_si_rinumera_saltando_i_brani_spariti() {
        // Copiare le posizioni lascerebbe dei buchi, e `position` è parte della
        // chiave primaria.
        let legacy = vecchio();
        aggiungi_vecchio(&legacy, 1, "a", "Uno", "A", "Al", 1);
        aggiungi_vecchio(&legacy, 2, "b", "Sparito", "A", "Al", 1);
        aggiungi_vecchio(&legacy, 3, "c", "Tre", "A", "Al", 1);
        legacy
            .execute(
                "INSERT INTO playlists (id, name, created_at, updated_at) VALUES (1, 'Mia', 1, 2)",
                [],
            )
            .expect("playlist");
        legacy
            .execute_batch("INSERT INTO playlist_tracks VALUES (1, 1, 0), (1, 2, 1), (1, 3, 2);")
            .expect("voci");
        let mut db = nuovo(&[("p1", "Uno", "A", "Al"), ("p3", "Tre", "A", "Al")]);

        let esito = import(&legacy, &mut db).expect("importazione");
        assert_eq!(esito.playlist_entries, 2);
        assert_eq!(esito.playlist_orphans, 1);

        let posizioni: Vec<i64> = db
            .prepare("SELECT position FROM playlist_tracks ORDER BY position")
            .and_then(|mut s| {
                s.query_map([], |r| r.get(0))
                    .and_then(std::iter::Iterator::collect)
            })
            .expect("posizioni");
        assert_eq!(posizioni, vec![0, 1], "rinumerate senza buchi");
    }

    #[test]
    fn le_lapidi_passano_e_quelle_sconosciute_si_saltano() {
        let legacy = vecchio();
        legacy
            .execute_batch(
                "INSERT INTO sync_tombstones VALUES ('track', 'a|b|c', 100),
                                                    ('playlist', 'mia', 200),
                                                    ('podcast', 'x', 300);",
            )
            .expect("lapidi");
        let mut db = nuovo(&[]);

        let esito = import(&legacy, &mut db).expect("importazione");
        assert_eq!(esito.tombstones, 2, "la terza non è un tipo che conosciamo");
        let quante: i64 = db
            .query_row("SELECT COUNT(*) FROM sync_tombstones", [], |r| r.get(0))
            .expect("conteggio");
        assert_eq!(quante, 2);
    }

    #[test]
    fn un_database_vecchio_senza_le_colonne_recenti_si_importa_lo_stesso() {
        // `liked`, `liked_at` e `stats_updated_at` sono arrivate tardi nella
        // catena delle migrazioni: chiederle a un database fermo prima farebbe
        // fallire tutta l'importazione invece di portare quel che c'è.
        let legacy = Connection::open_in_memory().expect("memoria");
        legacy
            .execute_batch(
                "CREATE TABLE tracks (
                   id INTEGER PRIMARY KEY, path TEXT, title TEXT, artist TEXT, album TEXT,
                   play_count INTEGER, last_played INTEGER, rating INTEGER);
                 CREATE TABLE play_history (
                   id INTEGER PRIMARY KEY, track_id INTEGER, played_at INTEGER, ms_played INTEGER);
                 CREATE TABLE playlists (
                   id INTEGER PRIMARY KEY, name TEXT, description TEXT,
                   created_at INTEGER, updated_at INTEGER);
                 CREATE TABLE playlist_tracks (
                   playlist_id INTEGER, track_id INTEGER, position INTEGER);
                 CREATE TABLE sync_tombstones (kind TEXT, key TEXT, deleted_at INTEGER);
                 INSERT INTO tracks (id, title, artist, album, play_count, rating)
                 VALUES (1, 'T', 'A', 'Al', 9, 4);",
            )
            .expect("schema intermedio");
        let mut db = nuovo(&[("p", "T", "A", "Al")]);

        let esito = import(&legacy, &mut db).expect("importazione");
        assert_eq!(esito.matched, 1);
        let (ascolti, voto): (i64, i64) = db
            .query_row("SELECT play_count, rating FROM tracks", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .expect("riga");
        assert_eq!(ascolti, 9);
        assert_eq!(voto, 4);
    }

    #[test]
    fn l_indirizzo_del_vecchio_database_e_un_uri_che_sqlite_capisce() {
        use std::path::Path;
        // I tre caratteri che in un URI vogliono dire altro si scrivono in
        // percentuale; spazi e lettere accentate passano così come sono.
        assert_eq!(
            uri_immutabile(Path::new("vecchio %20 #1?.db")),
            "file:vecchio %2520 %231%3F.db?immutable=1"
        );
        assert_eq!(
            uri_immutabile(Path::new("/home/io/Città/vecchio.db")),
            "file:///home/io/Città/vecchio.db?immutable=1"
        );
        if cfg!(windows) {
            assert_eq!(
                uri_immutabile(Path::new(r"C:\Users\Io\Musica vecchia\aether#2.db")),
                "file:///C:/Users/Io/Musica vecchia/aether%232.db?immutable=1"
            );
            assert_eq!(
                uri_immutabile(Path::new(r"\\nas\musica\aether.db")),
                "file:////nas/musica/aether.db?immutable=1",
                "una condivisione: l'autorità resta vuota"
            );
            assert_eq!(
                uri_immutabile(Path::new(r"\\?\C:\dati\aether.db")),
                "file:///C:/dati/aether.db?immutable=1"
            );
            assert_eq!(
                uri_immutabile(Path::new(r"\\?\UNC\nas\musica\aether.db")),
                "file:////nas/musica/aether.db?immutable=1"
            );
        }
    }

    #[test]
    fn un_vecchio_database_con_un_nome_strano_si_apre() {
        // L'indirizzo `file:` non deve rompere quel che prima si apriva: spazi,
        // `#`, `%` e lettere accentate nel percorso.
        let cartella = tempfile::tempdir().expect("cartella di prova");
        let percorso = cartella.path().join("Città vecchia #1 100%.db");
        let c = Connection::open(&percorso).expect("file");
        c.execute_batch(SCHEMA_VECCHIO).expect("schema");
        c.execute(
            "INSERT INTO tracks (id, title, artist, album, play_count) VALUES (1, 'T', 'A', 'Al', 3)",
            [],
        )
        .expect("riga");
        drop(c);
        let legacy = open_legacy(&percorso).expect("si apre");
        let righe: i64 = legacy
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
            .expect("conteggio");
        assert_eq!(righe, 1);
        let mut db = nuovo(&[("p", "T", "A", "Al")]);
        let piano = plan_import(&legacy, &mut db).expect("piano");
        assert!(!piano.journal_ignored, "nessun giornale, niente da dire");
    }

    /// Lo schema di [`vecchio`], per un file vero su disco.
    const SCHEMA_VECCHIO: &str = "CREATE TABLE tracks (
           id INTEGER PRIMARY KEY, path TEXT, title TEXT, artist TEXT, album TEXT,
           play_count INTEGER DEFAULT 0, last_played INTEGER, rating INTEGER DEFAULT 0,
           liked INTEGER DEFAULT 0, liked_at INTEGER, stats_updated_at INTEGER DEFAULT 0);
         CREATE TABLE play_history (
           id INTEGER PRIMARY KEY, track_id INTEGER, played_at INTEGER, ms_played INTEGER);
         CREATE TABLE playlists (
           id INTEGER PRIMARY KEY, name TEXT, description TEXT, created_at INTEGER,
           updated_at INTEGER, is_smart INTEGER DEFAULT 0, rules TEXT);
         CREATE TABLE playlist_tracks (
           playlist_id INTEGER, track_id INTEGER, position INTEGER);
         CREATE TABLE sync_tombstones (kind TEXT, key TEXT, deleted_at INTEGER);";

    #[test]
    fn un_vecchio_database_con_una_vista_al_posto_della_tabella_si_rifiuta() {
        // R06-11: il file lo sceglie l'utente da un selettore che accetta
        // qualunque `.db`, e può arrivare da chiunque. Una vista al posto di
        // `tracks` farebbe girare la sua interrogazione — anche una costosa —
        // con il lucchetto della libreria preso. Un database della versione di
        // prima ha tabelle vere, e quel che non lo è si rifiuta prima di leggere.
        let cartella = tempfile::tempdir().expect("cartella di prova");
        let percorso = cartella.path().join("vecchio.db");
        let c = Connection::open(&percorso).expect("file");
        c.execute_batch(
            &SCHEMA_VECCHIO.replace("CREATE TABLE tracks (", "CREATE TABLE brani_veri ("),
        )
        .expect("schema");
        c.execute_batch(
            "CREATE VIEW tracks AS
               SELECT id, path, title, artist, album, play_count, last_played, rating,
                      liked, liked_at, stats_updated_at FROM brani_veri;",
        )
        .expect("vista");
        drop(c);

        let esito = open_legacy(&percorso);
        assert!(
            matches!(
                esito.as_ref().map(|_| ()).map_err(|e| e.code().clone()),
                Err(ErrorCode::DbOpenFailed { .. })
            ),
            "una vista non è la tabella di un vecchio database: {:?}",
            esito.map(|_| ())
        );
    }

    #[test]
    fn il_giornale_accanto_al_vecchio_database_non_si_legge() {
        // R06-11: un `-wal` (con il suo `-shm`) costruito da altri accanto al
        // file è il caso di RUSTSEC-2026-0459. Il vecchio database si apre
        // `immutable`: solo il file principale. Quel che sta soltanto nel
        // giornale non si legge, e il piano lo dice, perché un giornale vero lo
        // lascia la vecchia app ancora aperta (o caduta).
        let cartella = tempfile::tempdir().expect("cartella di prova");
        let percorso = cartella.path().join("vecchio.db");
        let scrittore = Connection::open(&percorso).expect("file");
        scrittore
            .pragma_update(None, "journal_mode", "WAL")
            .expect("giornale WAL");
        scrittore.execute_batch(SCHEMA_VECCHIO).expect("schema");
        scrittore
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .expect("lo schema nel file principale");
        scrittore
            .pragma_update(None, "wal_autocheckpoint", 0)
            .expect("niente travaso automatico");
        scrittore
            .execute(
                "INSERT INTO tracks (id, title, artist, album, play_count, last_played)
                 VALUES (1, 'T', 'A', 'Al', 7, 1000)",
                [],
            )
            .expect("una riga che sta solo nel giornale");

        let legacy = open_legacy(&percorso).expect("il file principale si apre");
        let righe: i64 = legacy
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
            .expect("conteggio");
        assert_eq!(righe, 0, "le righe che stanno solo nel -wal non si leggono");

        let mut db = nuovo(&[("p", "T", "A", "Al")]);
        let piano = plan_import(&legacy, &mut db).expect("piano");
        assert!(
            piano.journal_ignored,
            "il piano dice che il giornale è rimasto fuori"
        );
        drop(scrittore);
    }

    #[test]
    fn il_piano_non_scrive_niente() {
        let legacy = vecchio();
        aggiungi_vecchio(&legacy, 1, "x", "T", "A", "Al", 15);
        let mut db = nuovo(&[("p", "T", "A", "Al")]);

        let piano = plan_import(&legacy, &mut db).expect("piano");
        assert_eq!(piano.matched, 1);
        assert_eq!(piano.play_count_carried, 15);

        let ascolti: i64 = db
            .query_row("SELECT play_count FROM tracks", [], |r| r.get(0))
            .expect("ascolti");
        assert_eq!(ascolti, 0, "il piano si guarda, non si applica");
    }

    #[test]
    fn il_piano_dice_esattamente_quel_che_fara_l_esecuzione() {
        // Il difetto per cui il piano è diventato «l'esecuzione annullata»:
        // contava i brani e non guardava cronologia, playlist e lapidi, quindi
        // annunciava «0 righe» dove c'era da portare. Un piano che tace su una
        // voce e uno che riporta zero sono indistinguibili da fuori.
        let legacy = vecchio();
        aggiungi_vecchio(&legacy, 1, "x", "T", "A", "Al", 15);
        legacy
            .execute_batch(
                "INSERT INTO play_history (track_id, played_at, ms_played)
                   VALUES (1, 100, 5), (1, 200, 6);
                 INSERT INTO playlists (id, name, created_at, updated_at)
                   VALUES (1, 'Mia', 1, 2);
                 INSERT INTO playlist_tracks VALUES (1, 1, 0);
                 INSERT INTO sync_tombstones VALUES ('track', 'x|y|z', 9);",
            )
            .expect("dati");
        let mut db = nuovo(&[("p", "T", "A", "Al")]);

        let piano = plan_import(&legacy, &mut db).expect("piano");
        let esito = import(&legacy, &mut db).expect("esecuzione");

        assert_eq!(piano, esito, "il piano deve prevedersi per intero");
        assert_eq!(esito.history_rows, 2);
        assert_eq!(esito.playlists, 1);
        assert_eq!(esito.playlist_entries, 1);
        assert_eq!(esito.tombstones, 1);
    }
}
