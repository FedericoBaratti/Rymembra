//! Le playlist.
//!
//! # L'identità è il nome
//!
//! `playlist_key` è il nome normalizzato — `aether_domain::keys::PlaylistKey` —
//! ed è `UNIQUE`. Non è una scorciatoia: è l'unico identificativo che due
//! dispositivi possono calcolare da soli e trovarsi d'accordo, e l'importatore
//! dal vecchio database usa già quello. Calcolarlo qui in un altro modo
//! farebbe comparire un doppione alla prima importazione successiva.
//!
//! Il rovescio della medaglia è che **rinominare fa una playlist nuova**, ed è
//! deliberato: senza un identificativo stabile trasmesso fra i dispositivi, un
//! rinomino e una creazione sono indistinguibili.
//!
//! # Le posizioni, e perché si riscrivono tutte
//!
//! `PRIMARY KEY (playlist_id, position)` — la posizione fa parte della chiave, e
//! il vincolo si controlla a **ogni istruzione**, non alla `COMMIT`. Un
//! `UPDATE … SET position = position + 1` su un intervallo è quindi corretto
//! solo se SQLite tocca le righe in ordine decrescente, e sbagliato se le tocca
//! in ordine crescente: il primo spostamento finirebbe su un posto ancora
//! occupato. Quale dei due ordini scelga non è documentato — dipende
//! dall'indice che decide di usare.
//!
//! Perciò [`remove_at`] e [`reorder`] non spostano: **rileggono l'ordine, lo
//! cambiano in memoria e lo riscrivono per intero**. Costa O(n) scritture per
//! un trascinamento invece di O(k), che su liste fatte a mano è rumore, e in
//! cambio toglie una classe di guasti che si manifesterebbe solo su certi
//! elenchi e solo dopo certe modifiche.
//!
//! [`add_tracks`] resta un accodamento: scrive oltre `MAX(position)`, dove non
//! c'è niente con cui scontrarsi.

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::keys::PlaylistKey;
use rusqlite::{Connection, Transaction};

use crate::library::{COLONNE_BRANO, TrackSummary, db_error, now_ms, track_from_row};

/// Una playlist, come la mostra un elenco.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSummary {
    /// L'identificativo di riga, per i comandi.
    pub id: i64,
    /// Il nome normalizzato: l'identità che attraversa la sincronizzazione.
    pub key: String,
    /// Il nome come l'ha scritto chi l'ha creata.
    pub name: String,
    /// La descrizione, se c'è.
    pub description: Option<String>,
    /// È automatica: l'appartenenza la decidono le regole, non le righe.
    pub is_smart: bool,
    /// Quanti brani contiene.
    pub tracks: i64,
    /// Quanto dura in tutto.
    pub duration_ms: i64,
    /// Quando è stata toccata l'ultima volta.
    pub updated_at: i64,
}

/// Le colonne di una playlist, con i suoi aggregati.
///
/// `LEFT JOIN` e non `JOIN`: una playlist appena creata non ha righe, e con una
/// giunzione interna sparirebbe dall'elenco esattamente nel momento in cui
/// l'utente la sta cercando per riempirla.
const ELENCO: &str = "SELECT p.id, p.playlist_key, p.name, p.description, p.is_smart,
            p.updated_at,
            COUNT(pt.track_id)               AS brani,
            COALESCE(SUM(t.duration_ms), 0)  AS durata
     FROM playlists p
     LEFT JOIN playlist_tracks pt ON pt.playlist_id = p.id
     LEFT JOIN tracks t           ON t.id = pt.track_id
     GROUP BY p.id
     ORDER BY p.name COLLATE NOCASE";

fn summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlaylistSummary> {
    Ok(PlaylistSummary {
        id: row.get(0)?,
        key: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        is_smart: row.get::<_, i64>(4)? != 0,
        updated_at: row.get(5)?,
        tracks: row.get(6)?,
        duration_ms: row.get(7)?,
    })
}

/// Tutte le playlist.
///
/// # Errori
///
/// `db.queryFailed` se la lettura fallisce.
pub fn list(connection: &Connection) -> Result<Vec<PlaylistSummary>, AppError> {
    let mut statement = connection
        .prepare(ELENCO)
        .map_err(|err| db_error("elenco delle playlist", &err))?;
    let rows = statement
        .query_map([], summary_from_row)
        .map_err(|err| db_error("elenco delle playlist", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("elenco delle playlist", &err))
}

/// Una playlist sola, dopo averla toccata.
fn read_one(connection: &Connection, id: i64) -> Result<PlaylistSummary, AppError> {
    // Si rilegge dall'elenco invece di comporre il riepilogo a mano: gli
    // aggregati vengono da una query sola, e ricalcolarli qui vorrebbe dire
    // avere due definizioni di «quanto dura una playlist».
    list(connection)?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| {
            AppError::new(ErrorCode::LibraryPlaylistNotFound {
                playlist_id: Some(id),
            })
        })
}

/// I brani di una playlist, nell'ordine in cui stanno.
///
/// # Errori
///
/// `db.queryFailed` se la lettura fallisce.
pub fn tracks(connection: &Connection, id: i64) -> Result<Vec<TrackSummary>, AppError> {
    let sql = format!(
        "SELECT {COLONNE_BRANO}
         FROM playlist_tracks pt
         JOIN tracks t ON t.id = pt.track_id
         WHERE pt.playlist_id = ?1
         ORDER BY pt.position"
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|err| db_error("brani di una playlist", &err))?;
    let rows = statement
        .query_map([id], track_from_row)
        .map_err(|err| db_error("brani di una playlist", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("brani di una playlist", &err))
}

/// Controlla che una playlist esista e si possa modificare a mano.
fn modificabile(connection: &Connection, id: i64) -> Result<(), AppError> {
    let smart: Option<i64> = connection
        .query_row(
            "SELECT is_smart FROM playlists WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
        .map_err(|err| db_error("verifica di una playlist", &err))?;
    match smart {
        None => Err(AppError::new(ErrorCode::LibraryPlaylistNotFound {
            playlist_id: Some(id),
        })),
        Some(v) if v != 0 => Err(AppError::new(ErrorCode::LibraryPlaylistIsSmart {
            playlist_id: Some(id),
        })),
        Some(_) => Ok(()),
    }
}

/// L'ordine attuale, come lista di identificativi di brano.
///
/// Gli identificativi si ripetono se un brano compare più volte, ed è voluto:
/// questa lista è l'appartenenza, non un insieme.
fn ordine_attuale(tx: &Transaction<'_>, id: i64) -> Result<Vec<i64>, AppError> {
    let mut statement = tx
        .prepare("SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position")
        .map_err(|err| db_error("ordine di una playlist", &err))?;
    let rows = statement
        .query_map([id], |row| row.get(0))
        .map_err(|err| db_error("ordine di una playlist", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("ordine di una playlist", &err))
}

/// Riscrive l'appartenenza di una playlist nell'ordine dato.
///
/// Cancella tutto e reinserisce. È il punto in cui la nota in testa al modulo
/// diventa codice: le posizioni finiscono `0..n` senza salti e senza che
/// nessuna istruzione passi mai per uno stato in cui due righe condividono un
/// posto — la tabella è vuota quando cominciano gli inserimenti.
fn riscrivi_ordine(tx: &Transaction<'_>, id: i64, ordine: &[i64]) -> Result<(), AppError> {
    tx.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?1", [id])
        .map_err(|err| db_error("riscrittura di una playlist", &err))?;
    let mut inserisci = tx
        .prepare(
            "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)",
        )
        .map_err(|err| db_error("riscrittura di una playlist", &err))?;
    for (posizione, track_id) in ordine.iter().enumerate() {
        let posizione = i64::try_from(posizione).unwrap_or(i64::MAX);
        inserisci
            .execute(rusqlite::params![id, track_id, posizione])
            .map_err(|err| db_error("riscrittura di una playlist", &err))?;
    }
    Ok(())
}

/// Segna una playlist come toccata adesso.
fn tocca(connection: &Connection, id: i64) -> Result<(), AppError> {
    connection
        .execute(
            "UPDATE playlists SET updated_at = ?2 WHERE id = ?1",
            rusqlite::params![id, now_ms()],
        )
        .map(|_| ())
        .map_err(|err| db_error("aggiornamento di una playlist", &err))
}

/// Crea una playlist vuota.
///
/// # Errori
///
/// `library.playlistNameInvalid` se il nome non identifica niente;
/// `library.playlistExists` se una playlist con lo stesso nome normalizzato c'è
/// già; `db.queryFailed` se la scrittura fallisce.
pub fn create(connection: &Connection, name: &str) -> Result<PlaylistSummary, AppError> {
    let id = crea_riga(connection, name)?;
    read_one(connection, id)
}

/// Inserisce la riga e basta, restituendo l'identificativo.
///
/// Separata da [`create`] perché l'importazione da Spotify crea la playlist
/// **dentro la propria transazione**, insieme alle voci e ai brani desiderati:
/// una playlist creata fuori sopravviverebbe a un piano annullato, e il piano
/// dell'importazione è per definizione un'esecuzione che viene abbandonata.
///
/// Accetta un `&Connection`, che una `Transaction` è già per deref: così il
/// controllo del nome e il riconoscimento del conflitto restano scritti una
/// volta sola.
pub(crate) fn crea_riga(connection: &Connection, name: &str) -> Result<i64, AppError> {
    let name = name.trim();
    let key = PlaylistKey::compute(Some(name));
    if key.as_str().is_empty() {
        return Err(AppError::new(ErrorCode::LibraryPlaylistNameInvalid {
            name: name.to_owned(),
        }));
    }
    let adesso = now_ms();
    let esito = connection.execute(
        "INSERT INTO playlists (playlist_key, name, created_at, updated_at, is_smart)
         VALUES (?1, ?2, ?3, ?3, 0)",
        rusqlite::params![key.as_str(), name, adesso],
    );
    match esito {
        Ok(_) => Ok(connection.last_insert_rowid()),
        // Il conflitto si riconosce dal vincolo violato, non dal testo del
        // messaggio: quello cambia fra versioni di SQLite, il codice no.
        Err(err) if e_conflitto(&err) => Err(AppError::new(ErrorCode::LibraryPlaylistExists {
            name: name.to_owned(),
        })),
        Err(err) => Err(db_error("creazione di una playlist", &err)),
    }
}

/// È una violazione di unicità?
fn e_conflitto(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ConstraintViolation,
                ..
            },
            _
        )
    )
}

/// Rinomina una playlist.
///
/// Cambia anche `playlist_key`, perché la chiave **è** il nome: dopo un
/// rinomino la playlist è, per la sincronizzazione, una playlist nuova. Vedi la
/// nota in testa al modulo.
///
/// # Errori
///
/// `library.playlistNotFound`, `library.playlistNameInvalid`,
/// `library.playlistExists`, `db.queryFailed`.
pub fn rename(connection: &Connection, id: i64, name: &str) -> Result<PlaylistSummary, AppError> {
    let name = name.trim();
    let key = PlaylistKey::compute(Some(name));
    if key.as_str().is_empty() {
        return Err(AppError::new(ErrorCode::LibraryPlaylistNameInvalid {
            name: name.to_owned(),
        }));
    }
    let esito = connection.execute(
        "UPDATE playlists SET playlist_key = ?2, name = ?3, updated_at = ?4 WHERE id = ?1",
        rusqlite::params![id, key.as_str(), name, now_ms()],
    );
    match esito {
        Ok(0) => Err(AppError::new(ErrorCode::LibraryPlaylistNotFound {
            playlist_id: Some(id),
        })),
        Ok(_) => read_one(connection, id),
        Err(err) if e_conflitto(&err) => Err(AppError::new(ErrorCode::LibraryPlaylistExists {
            name: name.to_owned(),
        })),
        Err(err) => Err(db_error("rinomino di una playlist", &err)),
    }
}

/// Cancella una playlist, lasciando una lapide.
///
/// La lapide non è burocrazia: senza, l'altro dispositivo vedrebbe soltanto «a
/// me manca una playlist» e la rimanderebbe indietro. Una cancellazione deve
/// poter viaggiare, altrimenti torna da sola.
///
/// Le righe di `playlist_tracks` se ne vanno da sole: la chiave esterna è
/// dichiarata `ON DELETE CASCADE`.
///
/// # Errori
///
/// `library.playlistNotFound`, `db.queryFailed`.
pub fn delete(connection: &mut Connection, id: i64) -> Result<(), AppError> {
    let tx = connection
        .transaction()
        .map_err(|err| db_error("cancellazione di una playlist", &err))?;

    let key: Option<String> = tx
        .query_row(
            "SELECT playlist_key FROM playlists WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
        .map_err(|err| db_error("cancellazione di una playlist", &err))?;
    let Some(key) = key else {
        return Err(AppError::new(ErrorCode::LibraryPlaylistNotFound {
            playlist_id: Some(id),
        }));
    };

    tx.execute("DELETE FROM playlists WHERE id = ?1", [id])
        .map_err(|err| db_error("cancellazione di una playlist", &err))?;
    tx.execute(
        "INSERT INTO sync_tombstones (kind, key, deleted_at) VALUES ('playlist', ?1, ?2)
         ON CONFLICT(kind, key) DO UPDATE SET deleted_at = MAX(sync_tombstones.deleted_at, excluded.deleted_at)",
        rusqlite::params![key, now_ms()],
    )
    .map_err(|err| db_error("lapide di una playlist", &err))?;

    tx.commit()
        .map_err(|err| db_error("cancellazione di una playlist", &err))
}

/// Aggiunge brani in fondo a una playlist.
///
/// I duplicati sono ammessi — lo stesso brano può comparire più volte, ed è
/// voluto — quindi non c'è nessun controllo da fare oltre alla posizione.
///
/// # Errori
///
/// `library.playlistNotFound`, `library.playlistIsSmart`, `db.queryFailed`.
pub fn add_tracks(
    connection: &mut Connection,
    id: i64,
    track_ids: &[i64],
) -> Result<PlaylistSummary, AppError> {
    modificabile(connection, id)?;
    let tx = connection
        .transaction()
        .map_err(|err| db_error("aggiunta a una playlist", &err))?;
    {
        let prima_libera: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(position) + 1, 0) FROM playlist_tracks WHERE playlist_id = ?1",
                [id],
                |row| row.get(0),
            )
            .map_err(|err| db_error("aggiunta a una playlist", &err))?;
        let mut inserisci = tx
            .prepare(
                "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)",
            )
            .map_err(|err| db_error("aggiunta a una playlist", &err))?;
        for (scarto, track_id) in track_ids.iter().enumerate() {
            let posizione = prima_libera.saturating_add(i64::try_from(scarto).unwrap_or(0));
            inserisci
                .execute(rusqlite::params![id, track_id, posizione])
                .map_err(|err| db_error("aggiunta a una playlist", &err))?;
        }
    }
    tx.commit()
        .map_err(|err| db_error("aggiunta a una playlist", &err))?;
    tocca(connection, id)?;
    read_one(connection, id)
}

/// Toglie il brano che sta in una posizione, chiudendo il buco.
///
/// Le posizioni restano `0..n` senza salti. Non è cosmesi: sono gli indici che
/// la finestra rimanda indietro come bersaglio di uno spostamento, e un buco
/// li farebbe puntare al brano sbagliato.
///
/// Una posizione che non esiste non è un guasto: è una richiesta a vuoto, il
/// caso di un clic su un elenco già cambiato sotto.
///
/// # Errori
///
/// `library.playlistNotFound`, `library.playlistIsSmart`, `db.queryFailed`.
pub fn remove_at(
    connection: &mut Connection,
    id: i64,
    position: i64,
) -> Result<PlaylistSummary, AppError> {
    modificabile(connection, id)?;
    let tx = connection
        .transaction()
        .map_err(|err| db_error("rimozione da una playlist", &err))?;
    let mut ordine = ordine_attuale(&tx, id)?;
    let Ok(indice) = usize::try_from(position) else {
        drop(tx);
        return read_one(connection, id);
    };
    if indice >= ordine.len() {
        drop(tx);
        return read_one(connection, id);
    }
    ordine.remove(indice);
    riscrivi_ordine(&tx, id, &ordine)?;
    tx.commit()
        .map_err(|err| db_error("rimozione da una playlist", &err))?;
    tocca(connection, id)?;
    read_one(connection, id)
}

/// Sposta un brano da una posizione a un'altra.
///
/// Le posizioni fuori dall'elenco non sono un guasto: un trascinamento su una
/// lista già cambiata sotto non deve dare un errore, deve non fare niente.
/// L'arrivo si taglia sull'ultimo posto valido, che è quel che l'utente intende
/// quando lascia il brano oltre la fine.
///
/// # Errori
///
/// `library.playlistNotFound`, `library.playlistIsSmart`, `db.queryFailed`.
pub fn reorder(
    connection: &mut Connection,
    id: i64,
    from: i64,
    to: i64,
) -> Result<PlaylistSummary, AppError> {
    modificabile(connection, id)?;
    let tx = connection
        .transaction()
        .map_err(|err| db_error("riordino di una playlist", &err))?;
    let mut ordine = ordine_attuale(&tx, id)?;

    let partenza = usize::try_from(from).ok().filter(|i| *i < ordine.len());
    let Some(partenza) = partenza else {
        drop(tx);
        return read_one(connection, id);
    };
    let brano = ordine.remove(partenza);
    // Dopo la rimozione l'elenco è più corto di uno: l'ultimo posto in cui si
    // può inserire è `len()`, non `len() - 1`.
    let arrivo = usize::try_from(to).unwrap_or(0).min(ordine.len());
    ordine.insert(arrivo, brano);

    riscrivi_ordine(&tx, id, &ordine)?;
    tx.commit()
        .map_err(|err| db_error("riordino di una playlist", &err))?;
    tocca(connection, id)?;
    read_one(connection, id)
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Una libreria in memoria con quattro brani, per non ripetere l'impalcatura.
    ///
    /// Le righe si inseriscono a mano invece di passare da una scansione: qui si
    /// provano le playlist, e far dipendere ogni prova da file WAV su disco
    /// legherebbe questi test a un guasto del lettore di tag.
    fn libreria() -> Connection {
        let connection = crate::db::open_in_memory().expect("database").connection;
        connection
            .execute_batch(
                "INSERT INTO tracks
                     (id, path, track_key, title, artist, album, duration_ms,
                      file_size, date_added, date_modified)
                 VALUES (1, 'a.mp3', 'k1', 'Uno',    'Art', 'Al', 1000, 1, 1, 1),
                        (2, 'b.mp3', 'k2', 'Due',    'Art', 'Al', 2000, 1, 1, 1),
                        (3, 'c.mp3', 'k3', 'Tre',    'Art', 'Al', 3000, 1, 1, 1),
                        (4, 'd.mp3', 'k4', 'Quattro','Art', 'Al', 4000, 1, 1, 1);",
            )
            .expect("brani");
        connection
    }

    fn posizioni(connection: &Connection, id: i64) -> Vec<String> {
        tracks(connection, id)
            .expect("brani")
            .into_iter()
            .map(|b| b.title)
            .collect()
    }

    #[test]
    fn una_playlist_nasce_vuota_e_si_ritrova() {
        let connection = libreria();
        let creata = create(&connection, "Da correre").expect("creata");
        assert_eq!(creata.tracks, 0);
        assert_eq!(creata.duration_ms, 0);
        assert!(!creata.is_smart);
        let elenco = list(&connection).expect("elenco");
        assert_eq!(elenco.len(), 1);
        assert_eq!(elenco[0].name, "Da correre");
    }

    #[test]
    fn due_nomi_che_normalizzano_uguale_sono_la_stessa_playlist() {
        // È il punto dell'intero modulo: la chiave è il nome piegato, e due
        // playlist con lo stesso nome sono la stessa playlist. Senza questo, la
        // prima importazione dal vecchio database ne creerebbe un doppione.
        let connection = libreria();
        create(&connection, "Rock ’n’ Roll").expect("creata");
        let err = create(&connection, "rock 'n' roll").expect_err("doppione");
        assert_eq!(err.code().kind().code(), "library.playlistExists");
    }

    #[test]
    fn un_nome_di_sola_punteggiatura_non_identifica_niente() {
        let connection = libreria();
        let err = create(&connection, "   ---   ").expect_err("nome vuoto");
        assert_eq!(err.code().kind().code(), "library.playlistNameInvalid");
    }

    #[test]
    fn i_brani_si_aggiungono_in_fondo_e_restano_in_ordine() {
        let mut connection = libreria();
        let p = create(&connection, "Mista").expect("creata").id;
        add_tracks(&mut connection, p, &[3, 1]).expect("aggiunti");
        let dopo = add_tracks(&mut connection, p, &[2]).expect("aggiunti");
        assert_eq!(posizioni(&connection, p), ["Tre", "Uno", "Due"]);
        assert_eq!(dopo.tracks, 3);
        assert_eq!(dopo.duration_ms, 6000);
    }

    #[test]
    fn lo_stesso_brano_puo_stare_due_volte() {
        let mut connection = libreria();
        let p = create(&connection, "Ossessione").expect("creata").id;
        add_tracks(&mut connection, p, &[1, 1, 1]).expect("aggiunti");
        assert_eq!(posizioni(&connection, p), ["Uno", "Uno", "Uno"]);
    }

    #[test]
    fn togliere_chiude_il_buco() {
        let mut connection = libreria();
        let p = create(&connection, "Mista").expect("creata").id;
        add_tracks(&mut connection, p, &[1, 2, 3]).expect("aggiunti");
        remove_at(&mut connection, p, 1).expect("tolto");
        assert_eq!(posizioni(&connection, p), ["Uno", "Tre"]);
        // Senza la chiusura del buco la posizione 2 resterebbe occupata e
        // l'aggiunta successiva finirebbe in 3, lasciando un vuoto che il
        // riordino userebbe come bersaglio valido.
        add_tracks(&mut connection, p, &[4]).expect("aggiunto");
        assert_eq!(posizioni(&connection, p), ["Uno", "Tre", "Quattro"]);
    }

    #[test]
    fn spostare_in_avanti() {
        let mut connection = libreria();
        let p = create(&connection, "Mista").expect("creata").id;
        add_tracks(&mut connection, p, &[1, 2, 3, 4]).expect("aggiunti");
        reorder(&mut connection, p, 0, 2).expect("spostato");
        assert_eq!(posizioni(&connection, p), ["Due", "Tre", "Uno", "Quattro"]);
    }

    #[test]
    fn spostare_indietro() {
        let mut connection = libreria();
        let p = create(&connection, "Mista").expect("creata").id;
        add_tracks(&mut connection, p, &[1, 2, 3, 4]).expect("aggiunti");
        reorder(&mut connection, p, 3, 1).expect("spostato");
        assert_eq!(posizioni(&connection, p), ["Uno", "Quattro", "Due", "Tre"]);
    }

    #[test]
    fn uno_scambio_di_vicini_non_viola_la_chiave() {
        // Il caso che la riscrittura integrale esiste per reggere: uno
        // spostamento in place metterebbe due righe sulla stessa posizione, e
        // SQLite rifiuterebbe l'istruzione — il vincolo si controlla subito,
        // non alla COMMIT.
        let mut connection = libreria();
        let p = create(&connection, "Mista").expect("creata").id;
        add_tracks(&mut connection, p, &[1, 2]).expect("aggiunti");
        reorder(&mut connection, p, 0, 1).expect("scambiati");
        assert_eq!(posizioni(&connection, p), ["Due", "Uno"]);
    }

    #[test]
    fn le_posizioni_restano_contigue_dopo_ogni_modifica() {
        // La proprietà da cui dipende tutto il resto: la finestra manda indietro
        // indici, e un buco nelle posizioni li farebbe puntare al brano
        // sbagliato. Si controlla sulla tabella e non sui titoli, perché è lì
        // che il difetto vivrebbe.
        let mut connection = libreria();
        let p = create(&connection, "Mista").expect("creata").id;
        add_tracks(&mut connection, p, &[1, 2, 3, 4]).expect("aggiunti");
        remove_at(&mut connection, p, 1).expect("tolto");
        reorder(&mut connection, p, 2, 0).expect("spostato");
        remove_at(&mut connection, p, 0).expect("tolto");

        let mut statement = connection
            .prepare(
                "SELECT position FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position",
            )
            .expect("query");
        let posizioni: Vec<i64> = statement
            .query_map([p], |row| row.get(0))
            .expect("righe")
            .collect::<Result<_, _>>()
            .expect("posizioni");
        assert_eq!(posizioni, [0, 1]);
    }

    #[test]
    fn un_indice_fuori_elenco_non_fa_niente() {
        let mut connection = libreria();
        let p = create(&connection, "Mista").expect("creata").id;
        add_tracks(&mut connection, p, &[1, 2]).expect("aggiunti");
        // Un trascinamento su una lista già cambiata sotto: niente errore,
        // niente modifica.
        reorder(&mut connection, p, 7, 0).expect("a vuoto");
        remove_at(&mut connection, p, 7).expect("a vuoto");
        assert_eq!(posizioni(&connection, p), ["Uno", "Due"]);
        // Oltre la fine si lascia in fondo: è quel che si intende lasciandolo lì.
        reorder(&mut connection, p, 0, 99).expect("in fondo");
        assert_eq!(posizioni(&connection, p), ["Due", "Uno"]);
    }

    #[test]
    fn rinominare_cambia_la_chiave() {
        let connection = libreria();
        let p = create(&connection, "Prima").expect("creata");
        let dopo = rename(&connection, p.id, "Seconda").expect("rinominata");
        assert_eq!(dopo.name, "Seconda");
        assert_ne!(dopo.key, p.key);
        // Il nome vecchio torna libero: dopo un rinomino è una playlist nuova.
        create(&connection, "Prima").expect("il nome è tornato libero");
    }

    #[test]
    fn cancellare_lascia_una_lapide() {
        let mut connection = libreria();
        let p = create(&connection, "Da buttare").expect("creata");
        add_tracks(&mut connection, p.id, &[1, 2]).expect("aggiunti");
        delete(&mut connection, p.id).expect("cancellata");

        assert!(list(&connection).expect("elenco").is_empty());
        let lapidi: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sync_tombstones WHERE kind = 'playlist' AND key = ?1",
                [&p.key],
                |row| row.get(0),
            )
            .expect("lapidi");
        assert_eq!(lapidi, 1, "senza lapide la cancellazione non viaggia");
        // Le righe se ne vanno con la playlist, per la chiave esterna.
        let righe: i64 = connection
            .query_row("SELECT COUNT(*) FROM playlist_tracks", [], |row| row.get(0))
            .expect("righe");
        assert_eq!(righe, 0);
    }

    #[test]
    fn una_playlist_automatica_non_si_modifica_a_mano() {
        let mut connection = libreria();
        connection
            .execute_batch(
                "INSERT INTO playlists (id, playlist_key, name, created_at, updated_at, is_smart, rules)
                 VALUES (9, 'auto', 'Auto', 1, 1, 1, '{}');",
            )
            .expect("playlist automatica");
        let err = add_tracks(&mut connection, 9, &[1]).expect_err("non modificabile");
        assert_eq!(err.code().kind().code(), "library.playlistIsSmart");
        // E non è ritentabile: riprovare darebbe lo stesso esito.
        assert!(!err.is_retryable());
    }

    #[test]
    fn una_playlist_che_non_esiste_lo_dice() {
        let mut connection = libreria();
        let err = add_tracks(&mut connection, 404, &[1]).expect_err("non c'è");
        assert_eq!(err.code().kind().code(), "library.playlistNotFound");
    }
}
