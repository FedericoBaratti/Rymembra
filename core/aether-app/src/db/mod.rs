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

mod migrations;

pub use migrations::{LATEST_VERSION, MIGRATIONS, Migration};

use std::path::Path;

use aether_domain::errors::{AppError, ErrorCode};
use rusqlite::Connection;

/// Quanto aspettare se un'altra connessione sta scrivendo, prima di arrendersi.
const BUSY_TIMEOUT_MS: u32 = 5_000;

/// Traduce un errore di SQLite nel catalogo, tenendo il testo originale come causa.
fn db_error(code: ErrorCode, err: &rusqlite::Error) -> AppError {
    AppError::new(code).with_cause(err.to_string())
}

/// Apre il database al percorso dato, applicando le migrazioni mancanti.
///
/// Restituisce anche quante migrazioni sono state applicate: è il numero che
/// finisce nella riga di avvio del log, e l'unico modo per accorgersi che un
/// aggiornamento ha toccato il database senza dirlo.
pub fn open(path: &Path) -> Result<Opened, AppError> {
    let connection = Connection::open(path).map_err(|err| {
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
}

fn open_connection(connection: Connection) -> Result<Opened, AppError> {
    configure(&connection)?;
    let fts5 = has_fts5(&connection);
    let version_before = user_version(&connection)?;
    let applied = migrate(&connection)?;
    Ok(Opened {
        connection,
        version_before,
        applied,
        fts5,
    })
}

/// Le impostazioni di connessione. Vanno rifatte a ogni apertura: non si
/// salvano nel file, tranne `journal_mode`.
fn configure(connection: &Connection) -> Result<(), AppError> {
    let fail = |err: rusqlite::Error| {
        db_error(
            ErrorCode::DbQueryFailed {
                detail: Some("pragma di apertura".into()),
            },
            &err,
        )
    };

    // WAL: chi legge non blocca chi scrive. Serve perché la scansione scrive a
    // lungo mentre l'interfaccia continua a leggere la libreria — senza, l'app
    // si impunta per tutta la durata di una riscansione.
    // Restituisce una riga, quindi non si può usare `pragma_update`.
    connection
        .query_row("PRAGMA journal_mode = WAL", [], |row| {
            row.get::<_, String>(0)
        })
        .map_err(fail)?;

    // Le chiavi esterne sono spente per default in SQLite, e vanno riaccese a
    // ogni connessione. Sono ciò che fa sparire le righe di `playlist_tracks`
    // quando si cancella un brano: senza, restano a puntare nel vuoto e la
    // playlist mostra elementi che non esistono.
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(fail)?;

    // In WAL, `NORMAL` non attende il flush del disco a ogni commit. Il rischio
    // è perdere le ultime transazioni se manca la corrente; il dato in gioco è
    // un indice ricostruibile con una scansione, non il file musicale.
    connection
        .pragma_update(None, "synchronous", "NORMAL")
        .map_err(fail)?;

    connection
        .busy_timeout(std::time::Duration::from_millis(u64::from(BUSY_TIMEOUT_MS)))
        .map_err(fail)?;

    Ok(())
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
        .map_err(|err| {
            db_error(
                ErrorCode::DbQueryFailed {
                    detail: Some("lettura di user_version".into()),
                },
                &err,
            )
        })
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
