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
use aether_domain::regole::Insieme;
use rusqlite::{Connection, Transaction};

use crate::library::{
    COLONNE_BRANO, TrackSummary, db_error, now_ms, track_from_row, transazione_di_scrittura,
};

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

/// Quanto può essere lungo il nome di una playlist.
///
/// Duecento caratteri sono tre volte il nome più lungo che si scriva davvero —
/// una data, un posto e un'occasione ci stanno con il doppio di margine — e
/// molto meno di quel che ci si incolla dentro per sbaglio. Il tetto è in
/// caratteri e non in byte: contare i byte darebbe a un nome in giapponese un
/// terzo dello spazio di uno in inglese.
pub const NOME_MASSIMO: usize = 200;

/// Il nome di una playlist che arriva **da fuori**, ridotto a [`NOME_MASSIMO`].
///
/// # Perché due regole per la stessa colonna
///
/// Perché sono due situazioni diverse. Chi **scrive** un nome e ne scrive
/// cinquemila caratteri sta incollando per sbaglio, e il rifiuto di
/// [`nome_utilizzabile`] glielo dice mentre può ancora rimediare. Chi
/// **importa** non ha scritto niente: il titolo di un item dell'Internet
/// Archive, la riga `#PLAYLIST` di un M3U, il nome di una playlist di Spotify
/// arrivano come sono, e un concerto con la scaletta intera nel titolo non è un
/// errore di nessuno. Il tetto della 3.0 li rifiutava tutti, e con loro
/// l'importazione intera — centinaia di brani ritrovati persi per un titolo.
///
/// Quindi qui si **tronca**, su un confine di carattere (si conta in `char`,
/// come il tetto), e si mette un'ellissi in fondo perché chi guarda veda che il
/// nome continuava. Il risultato sta nel tetto **compresa** l'ellissi, e passa
/// da [`nome_utilizzabile`] senza eccezioni.
///
/// Gli spazi prima dell'ellissi si tolgono: «Live at the…» e non «Live at
/// the …», che è il taglio che una persona farebbe a mano.
#[must_use]
pub(crate) fn nome_da_importare(titolo: &str) -> String {
    let titolo = titolo.trim();
    if titolo.chars().count() <= NOME_MASSIMO {
        return titolo.to_owned();
    }
    let testa: String = titolo
        .chars()
        .take(NOME_MASSIMO.saturating_sub(1))
        .collect();
    format!("{}…", testa.trim_end())
}

/// Le colonne di una playlist, con i suoi aggregati, e dopo la giunzione quel
/// che si vuole: un `WHERE` per una playlist sola, niente per tutte.
///
/// `LEFT JOIN` e non `JOIN`: una playlist appena creata non ha righe, e con una
/// giunzione interna sparirebbe dall'elenco esattamente nel momento in cui
/// l'utente la sta cercando per riempirla.
///
/// Una macro e non una costante perché le query sono due — tutte, una — e due
/// copie dello stesso `SELECT` sono due definizioni di «quanto dura una
/// playlist» che prima o poi divergono.
macro_rules! riepilogo {
    ($dove:literal) => {
        concat!(
            "SELECT p.id, p.playlist_key, p.name, p.description, p.is_smart,
                    p.updated_at,
                    COUNT(pt.track_id)               AS brani,
                    COALESCE(SUM(t.duration_ms), 0)  AS durata
             FROM playlists p
             LEFT JOIN playlist_tracks pt ON pt.playlist_id = p.id
             LEFT JOIN tracks t           ON t.id = pt.track_id ",
            $dove,
            " GROUP BY p.id"
        )
    };
}

/// Tutte le playlist, in ordine di nome.
const ELENCO: &str = concat!(riepilogo!(""), " ORDER BY p.name COLLATE NOCASE");

/// Una playlist sola, per identificativo.
const UNA: &str = riepilogo!("WHERE p.id = ?1");

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
    let mut elenco = {
        let mut statement = connection
            .prepare(ELENCO)
            .map_err(|err| db_error("elenco delle playlist", &err))?;
        let rows = statement
            .query_map([], summary_from_row)
            .map_err(|err| db_error("elenco delle playlist", &err))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|err| db_error("elenco delle playlist", &err))?
    };

    // Le playlist intelligenti non hanno righe in `playlist_tracks`, quindi la
    // query qui sopra le conta **zero**: senza questo giro l'elenco direbbe
    // «0 brani» sotto ognuna di loro, che è la cosa più simile a «è rotta».
    //
    // Una query in più per ognuna, e va bene: le playlist sono decine, non
    // migliaia, e questo elenco si chiede quando si apre la barra laterale.
    // L'alternativa — tenere i conteggi in una colonna — sarebbe una copia da
    // aggiornare a ogni ascolto e a ogni scansione, cioè il motivo per cui i
    // brani di una playlist intelligente non si materializzano. Quel che si fa
    // invece è ricordarli finché la libreria non cambia: vedi
    // `smart::conteggi_ricordati`.
    let adesso = now_ms();
    for riga in elenco.iter_mut().filter(|p| p.is_smart) {
        conta_l_intelligente(connection, riga, adesso)?;
    }
    Ok(elenco)
}

/// I numeri di una playlist intelligente, che la giunzione conta zero.
///
/// Un conteggio che fallisce lascia lo zero invece di far fallire l'elenco: una
/// regola che il database rifiuta riguarda quella playlist, e le altre devono
/// potersi aprire lo stesso. Le regole che non si leggono, invece, sono un
/// guasto del database e salgono.
fn conta_l_intelligente(
    connection: &Connection,
    riga: &mut PlaylistSummary,
    adesso: i64,
) -> Result<(), AppError> {
    let insieme = crate::smart::da_json(&regole_grezze(connection, riga.id)?);
    if let Ok((brani, durata)) = crate::smart::conteggi_ricordati(connection, &insieme, adesso) {
        riga.tracks = brani;
        riga.duration_ms = durata;
    }
    Ok(())
}

/// La colonna `rules` così com'è, senza interpretarla.
fn regole_grezze(connection: &Connection, id: i64) -> Result<String, AppError> {
    connection
        .query_row("SELECT rules FROM playlists WHERE id = ?1", [id], |row| {
            row.get::<_, Option<String>>(0)
        })
        .map(|r| r.unwrap_or_default())
        .map_err(|err| db_error("regole di una playlist", &err))
}

/// Una playlist sola, dopo averla toccata.
///
/// # Solo lei
///
/// Fino alla 2.4 si rileggeva l'elenco intero e se ne teneva una: ogni brano
/// trascinato in una playlist rifaceva i conteggi di **tutte** le
/// intelligenti, cioè una lettura di `tracks` per ognuna, per restituire i
/// numeri di una playlist a mano. Adesso la query è la stessa dell'elenco con
/// un `WHERE` — la stessa macro, quindi la stessa definizione di «quanto dura
/// una playlist» — e un'intelligente conta soltanto sé stessa.
fn read_one(connection: &Connection, id: i64) -> Result<PlaylistSummary, AppError> {
    use rusqlite::OptionalExtension as _;
    let riga = connection
        .prepare_cached(UNA)
        .and_then(|mut statement| statement.query_row([id], summary_from_row).optional())
        .map_err(|err| db_error("lettura di una playlist", &err))?;
    let Some(mut riga) = riga else {
        return Err(AppError::new(ErrorCode::LibraryPlaylistNotFound {
            playlist_id: Some(id),
        }));
    };
    if riga.is_smart {
        conta_l_intelligente(connection, &mut riga, now_ms())?;
    }
    Ok(riga)
}

/// I brani di una playlist, che sia a mano o automatica.
///
/// # Perché una funzione sola per due cose diverse
///
/// Perché per chi guarda **non** sono due cose diverse: è una playlist, e
/// dentro ci sono dei brani. Due funzioni vorrebbero dire che ogni chiamante —
/// l'elenco, l'esportazione, la coda, il backup — deve ricordarsi di chiedere
/// prima `is_smart` e poi la funzione giusta, e il primo che se lo dimentica
/// mostra una playlist intelligente vuota. Il `match` sta qui, una volta sola.
///
/// # Errori
///
/// `db.queryFailed` se la lettura fallisce.
pub fn tracks(connection: &Connection, id: i64) -> Result<Vec<TrackSummary>, AppError> {
    if let Some(insieme) = regole_di(connection, id)? {
        return crate::smart::brani(connection, &insieme, now_ms());
    }
    brani_a_mano(connection, id)
}

/// Le regole di una playlist, se è intelligente.
///
/// `Ok(None)` quando è una playlist normale — non è un errore, è la
/// maggioranza dei casi.
///
/// # Errori
///
/// `library.playlistNotFound` se la playlist non c'è; `db.queryFailed` se il
/// database non risponde.
pub fn regole_di(connection: &Connection, id: i64) -> Result<Option<Insieme>, AppError> {
    let riga: Option<(i64, Option<String>)> = connection
        .query_row(
            "SELECT is_smart, rules FROM playlists WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map(Some)
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
        .map_err(|err| db_error("regole di una playlist", &err))?;
    let Some((smart, rules)) = riga else {
        return Err(AppError::new(ErrorCode::LibraryPlaylistNotFound {
            playlist_id: Some(id),
        }));
    };
    if smart == 0 {
        return Ok(None);
    }
    // Una playlist marcata intelligente e senza regole vale insieme vuoto, cioè
    // tutta la libreria. È visibile subito e si corregge dall'interfaccia; un
    // errore qui renderebbe la playlist impossibile perfino da aprire per
    // sistemarla.
    Ok(Some(crate::smart::da_json(rules.as_deref().unwrap_or(""))))
}

/// I brani di una playlist a mano, nell'ordine in cui stanno.
fn brani_a_mano(connection: &Connection, id: i64) -> Result<Vec<TrackSummary>, AppError> {
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
pub(crate) fn riscrivi_ordine(
    tx: &Transaction<'_>,
    id: i64,
    ordine: &[i64],
) -> Result<(), AppError> {
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
/// Il nome di una playlist si può usare?
///
/// Due condizioni, in un posto solo perché sono le stesse per chi crea e per
/// chi rinomina: prima erano scritte due volte, e la seconda era scritta zero.
///
/// **Identifica qualcosa.** La chiave normalizzata non può essere vuota, o due
/// playlist di sola punteggiatura sarebbero la stessa riga.
///
/// **Sta in una riga.** Il nome finisce in una voce della colonna, in un menù e
/// in un titolo, e tutti e tre sono righe. Chi ci incolla un testo non sta
/// dando un nome: il database lo accettava, e da lì in poi la colonna aveva una
/// voce alta una pagina che nessuna larghezza rimetteva a posto. Vedi
/// [`NOME_MASSIMO`].
fn nome_utilizzabile(name: &str) -> Result<(), AppError> {
    if PlaylistKey::compute(Some(name)).as_str().is_empty() {
        return Err(AppError::new(ErrorCode::LibraryPlaylistNameInvalid {
            name: name.to_owned(),
        }));
    }
    let quanti = name.chars().count();
    if quanti > NOME_MASSIMO {
        return Err(AppError::new(ErrorCode::LibraryPlaylistNameInvalid {
            // Troncato: il nome viaggia dentro il codice d'errore, e rimetterci
            // dentro i cinquemila caratteri appena rifiutati vorrebbe dire
            // farli passare lo stesso, per un'altra porta.
            name: name.chars().take(NOME_MASSIMO).collect(),
        })
        .with_cause(format!(
            "il nome è di {quanti} caratteri, e il tetto è {NOME_MASSIMO}"
        )));
    }
    Ok(())
}

pub(crate) fn crea_riga(connection: &Connection, name: &str) -> Result<i64, AppError> {
    let name = name.trim();
    nome_utilizzabile(name)?;
    let key = PlaylistKey::compute(Some(name));
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

/// Crea una playlist intelligente.
///
/// # Perché è una funzione a parte e non un parametro di [`create`]
///
/// Perché sono due operazioni con due esiti diversi: una playlist a mano nasce
/// vuota e si riempie, una intelligente nasce **piena** — la sua appartenenza
/// esiste già, è la libreria filtrata. Un booleano nella firma di `create`
/// sarebbe il parametro che nessuno ricorda in che verso va, e per giunta
/// lascerebbe `create` senza un posto dove mettere le regole.
///
/// # Errori
///
/// `library.playlistNameInvalid`, `library.playlistExists`, `db.queryFailed`.
pub fn create_smart(
    connection: &Connection,
    name: &str,
    insieme: &Insieme,
) -> Result<PlaylistSummary, AppError> {
    let regole = crate::smart::a_json(insieme)?;
    // Un savepoint tiene insieme i due passi: se la scrittura delle regole
    // fallisse dopo la creazione della riga, resterebbe una playlist A MANO
    // vuota col nome richiesto — e il nome, ormai occupato, farebbe fallire
    // anche il tentativo successivo. Savepoint e non `transaction()`, che
    // vorrebbe una connessione mutabile che nessun chiamante ha.
    connection
        .execute_batch("SAVEPOINT crea_smart")
        .map_err(|err| db_error("apertura della creazione di una playlist", &err))?;
    let esito = crea_riga(connection, name).and_then(|id| {
        connection
            .execute(
                "UPDATE playlists SET is_smart = 1, rules = ?2 WHERE id = ?1",
                rusqlite::params![id, regole],
            )
            .map_err(|err| db_error("regole di una playlist nuova", &err))?;
        read_one(connection, id)
    });
    match esito {
        Ok(riassunto) => {
            connection
                .execute_batch("RELEASE crea_smart")
                .map_err(|err| db_error("chiusura della creazione di una playlist", &err))?;
            Ok(riassunto)
        }
        Err(err) => {
            // Best-effort: l'errore da riportare è quello vero, non quello
            // del disfacimento.
            let _ = connection.execute_batch("ROLLBACK TO crea_smart; RELEASE crea_smart");
            Err(err)
        }
    }
}

/// Riscrive le regole di una playlist intelligente.
///
/// # Errori
///
/// `library.playlistNotFound` se non c'è; `library.playlistIsSmart` **al
/// contrario** — `library.playlistNotFound` con il dettaglio — se la playlist
/// non è intelligente: cambiare le regole di una playlist a mano vorrebbe dire
/// convertirla, e una conversione butta via delle righe che qualcuno ha messo
/// lì a una a una. Se serve, si crea una playlist nuova.
pub fn set_rules(
    connection: &Connection,
    id: i64,
    insieme: &Insieme,
) -> Result<PlaylistSummary, AppError> {
    if regole_di(connection, id)?.is_none() {
        return Err(AppError::new(ErrorCode::LibraryPlaylistNotFound {
            playlist_id: Some(id),
        })
        .with_cause("non è una playlist intelligente: le regole non si applicano"));
    }
    connection
        .execute(
            "UPDATE playlists SET rules = ?2, updated_at = ?3 WHERE id = ?1",
            rusqlite::params![id, crate::smart::a_json(insieme)?, now_ms()],
        )
        .map_err(|err| db_error("regole di una playlist", &err))?;
    read_one(connection, id)
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
    nome_utilizzabile(name)?;
    let key = PlaylistKey::compute(Some(name));
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
/// È [`cancella`] senza la fotografia, per chi non ha un «Annulla» da offrire.
///
/// # Errori
///
/// `library.playlistNotFound`, `db.queryFailed`.
pub fn delete(connection: &mut Connection, id: i64) -> Result<(), AppError> {
    cancella(connection, id).map(|_| ())
}

/// Quel che serve per rimettere al suo posto una playlist appena cancellata.
///
/// # Perché una fotografia e non una cancellazione rimandata
///
/// La strada ovvia per un «Annulla» sarebbe non cancellare finché l'avviso è a
/// schermo, e cancellare davvero alla scadenza. Ha un buco proprio dove fa più
/// male: se in quei secondi l'applicazione si chiude, o cade, la scadenza non
/// arriva mai, e la playlist che l'utente ha visto sparire **torna** al
/// prossimo avvio — e con lei, alla prima sincronizzazione, sugli altri
/// dispositivi. Per evitarlo servirebbe un registro delle cancellazioni in
/// attesa che sopravviva alla chiusura, cioè una seconda verità sullo stato
/// delle playlist da tenere allineata con la prima.
///
/// Qui invece si cancella **subito**, nella stessa transazione di sempre, e si
/// tiene in mano quel che serve per tornare indietro: è la forma
/// dell'«Annulla» della coda sostituita (`riproduzione::coda`), che sostituisce
/// e intanto ricorda la coda di prima. Una chiusura nel mezzo perde soltanto
/// la possibilità di annullare, che è quel che chi chiude si aspetta; niente
/// resuscita, e niente si perde finché l'avviso c'è.
///
/// # Cosa c'è dentro
///
/// Tutto quel che la cancellazione porta via, e soltanto quello: la riga di
/// `playlists` colonna per colonna — letta per nome e non per elenco scritto
/// qui, così una colonna aggiunta da una migrazione futura torna anche lei —,
/// i brani nell'ordine in cui stavano, i posti tenuti in `playlist_orfane` per
/// i brani usciti dalla libreria (vanno via in cascata), i desiderati che
/// aspettavano di entrare in questa playlist (perdono l'aggancio con
/// `ON DELETE SET NULL`), e la lapide che c'era già prima per lo stesso nome.
#[derive(Debug, Clone)]
pub struct Cancellata {
    /// L'identificativo che aveva.
    id: i64,
    /// La chiave, per la lapide.
    chiave: String,
    /// Il nome, per dire quale playlist è stata cancellata.
    nome: String,
    /// La riga di `playlists`, colonna per colonna.
    riga: Vec<(String, rusqlite::types::Value)>,
    /// I brani, nell'ordine in cui stavano; ripetuti se ripetuti.
    brani: Vec<i64>,
    /// I posti tenuti per chi è uscito dalla libreria: chiave e posizione.
    posti_orfani: Vec<(String, i64)>,
    /// I desiderati che sarebbero entrati qui.
    desiderati: Vec<i64>,
    /// La data della lapide che c'era già per questa chiave, se c'era.
    lapide_di_prima: Option<i64>,
}

impl Cancellata {
    /// Il nome della playlist, com'era scritto.
    #[must_use]
    pub fn nome(&self) -> &str {
        &self.nome
    }

    /// L'identificativo che aveva.
    #[must_use]
    pub fn id(&self) -> i64 {
        self.id
    }
}

/// Cancella una playlist e restituisce quel che serve a rimetterla.
///
/// Fa esattamente quel che faceva [`delete`] — la riga se ne va, le sue righe
/// in cascata, la lapide resta — e prima di farlo, nella stessa transazione,
/// fotografa quel che sta per andarsene. Vedi [`Cancellata`].
///
/// # Errori
///
/// `library.playlistNotFound`, `db.queryFailed`.
pub fn cancella(connection: &mut Connection, id: i64) -> Result<Cancellata, AppError> {
    // `Immediate` in questa e nelle tre qui sotto: leggono e poi scrivono, e
    // una transazione che parte in lettura e passa alla scrittura non aspetta
    // chi scrive nel frattempo — con WAL fallisce subito con
    // `SQLITE_BUSY_SNAPSHOT`, senza passare dall'attesa di cinque secondi.
    // Vedi [`crate::library::transazione_di_scrittura`].
    let tx = transazione_di_scrittura(connection)
        .map_err(|err| db_error("cancellazione di una playlist", &err))?;

    let fotografia = fotografa(&tx, id)?;

    tx.execute("DELETE FROM playlists WHERE id = ?1", [id])
        .map_err(|err| db_error("cancellazione di una playlist", &err))?;
    tx.execute(
        "INSERT INTO sync_tombstones (kind, key, deleted_at) VALUES ('playlist', ?1, ?2)
         ON CONFLICT(kind, key) DO UPDATE SET deleted_at = MAX(sync_tombstones.deleted_at, excluded.deleted_at)",
        rusqlite::params![fotografia.chiave, now_ms()],
    )
    .map_err(|err| db_error("lapide di una playlist", &err))?;

    tx.commit()
        .map_err(|err| db_error("cancellazione di una playlist", &err))?;
    Ok(fotografia)
}

/// Legge tutto quel che la cancellazione di `id` porterebbe via.
fn fotografa(tx: &Transaction<'_>, id: i64) -> Result<Cancellata, AppError> {
    use rusqlite::OptionalExtension as _;
    let guasto = |err: rusqlite::Error| db_error("fotografia di una playlist", &err);

    let riga: Option<Vec<(String, rusqlite::types::Value)>> = {
        let mut statement = tx
            .prepare("SELECT * FROM playlists WHERE id = ?1")
            .map_err(guasto)?;
        let colonne: Vec<String> = statement
            .column_names()
            .into_iter()
            .map(str::to_owned)
            .collect();
        statement
            .query_row([id], |row| {
                colonne
                    .iter()
                    .enumerate()
                    .map(|(indice, colonna)| Ok((colonna.clone(), row.get(indice)?)))
                    .collect()
            })
            .optional()
            .map_err(guasto)?
    };
    let Some(riga) = riga else {
        return Err(AppError::new(ErrorCode::LibraryPlaylistNotFound {
            playlist_id: Some(id),
        }));
    };
    let testo = |nome: &str| {
        riga.iter()
            .find(|(colonna, _)| colonna == nome)
            .and_then(|(_, valore)| match valore {
                rusqlite::types::Value::Text(testo) => Some(testo.clone()),
                _ => None,
            })
            .unwrap_or_default()
    };
    let chiave = testo("playlist_key");
    let nome = testo("name");

    let brani = ordine_attuale(tx, id)?;
    let posti_orfani = {
        let mut statement = tx
            .prepare("SELECT content_key, position FROM playlist_orfane WHERE playlist_id = ?1")
            .map_err(guasto)?;
        statement
            .query_map([id], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(guasto)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(guasto)?
    };
    let desiderati = {
        let mut statement = tx
            .prepare("SELECT id FROM desiderati WHERE playlist_id = ?1")
            .map_err(guasto)?;
        statement
            .query_map([id], |row| row.get(0))
            .map_err(guasto)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(guasto)?
    };
    let lapide_di_prima = tx
        .query_row(
            "SELECT deleted_at FROM sync_tombstones WHERE kind = 'playlist' AND key = ?1",
            [&chiave],
            |row| row.get(0),
        )
        .optional()
        .map_err(guasto)?;

    Ok(Cancellata {
        id,
        chiave,
        nome,
        riga,
        brani,
        posti_orfani,
        desiderati,
        lapide_di_prima,
    })
}

/// Rimette una playlist cancellata com'era, e la restituisce.
///
/// Tutto in una transazione: o torna intera, o non torna niente.
///
/// # Com'era, tranne due cose
///
/// **La data di modifica è adesso.** Non è un dettaglio: nel frattempo la
/// lapide può essere già partita verso Drive o verso l'altro dispositivo, e la
/// fusione (`aether_sync::fusione`) tiene una playlist cancellata solo se è
/// stata toccata **dopo** la lapide. Con la data di prima la playlist tornerebbe
/// qui e sparirebbe di nuovo alla prima sincronizzazione; con quella di adesso è
/// «rifatta dopo la cancellazione», che è esattamente quel che è successo. La
/// lapide di qui torna invece com'era: tolta, o con la data che aveva già.
///
/// **I brani usciti dalla libreria nel frattempo restano fuori**, e le
/// posizioni si ricompattano: la chiave esterna rifiuterebbe la loro riga, e un
/// buco nelle posizioni farebbe puntare la finestra al brano sbagliato (vedi la
/// nota in testa al modulo).
///
/// L'identificativo è quello di prima se è ancora libero. Se nel frattempo
/// l'ha preso una playlist nuova — SQLite riusa il più alto quando lo si libera
/// — se ne prende uno nuovo, e le righe vanno sotto quello.
///
/// # Errori
///
/// `library.playlistExists` se nel frattempo è nata un'altra playlist con lo
/// stesso nome: la chiave è il nome, e due righe non possono averla. Non si
/// tocca niente. `db.*` se la scrittura fallisce.
pub fn ripristina(
    connection: &mut Connection,
    cancellata: &Cancellata,
) -> Result<PlaylistSummary, AppError> {
    let guasto = |err: rusqlite::Error| db_error("ripristino di una playlist", &err);
    let tx = transazione_di_scrittura(connection).map_err(guasto)?;

    let nome_preso: bool = tx
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM playlists WHERE playlist_key = ?1)",
            [&cancellata.chiave],
            |row| row.get(0),
        )
        .map_err(guasto)?;
    if nome_preso {
        return Err(AppError::new(ErrorCode::LibraryPlaylistExists {
            name: cancellata.nome.clone(),
        }));
    }
    let id_libero: bool = tx
        .query_row(
            "SELECT NOT EXISTS (SELECT 1 FROM playlists WHERE id = ?1)",
            [cancellata.id],
            |row| row.get(0),
        )
        .map_err(guasto)?;

    // I nomi delle colonne vengono dal database stesso, non da chi chiama:
    // tra virgolette lo stesso, perché `name` e `rules` sono parole che SQLite
    // conosce.
    let adesso = now_ms();
    let colonne: Vec<&(String, rusqlite::types::Value)> = cancellata
        .riga
        .iter()
        .filter(|(colonna, _)| id_libero || colonna != "id")
        .collect();
    let nomi = colonne
        .iter()
        .map(|(colonna, _)| format!("\"{}\"", colonna.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(", ");
    let segnaposti = (1..=colonne.len())
        .map(|indice| format!("?{indice}"))
        .collect::<Vec<_>>()
        .join(", ");
    let valori = colonne.iter().map(|(colonna, valore)| {
        if colonna == "updated_at" {
            rusqlite::types::Value::Integer(adesso)
        } else {
            valore.clone()
        }
    });
    tx.execute(
        &format!("INSERT INTO playlists ({nomi}) VALUES ({segnaposti})"),
        rusqlite::params_from_iter(valori),
    )
    .map_err(guasto)?;
    let id = if id_libero {
        cancellata.id
    } else {
        tx.last_insert_rowid()
    };

    let ancora: Vec<i64> = {
        let mut esiste = tx
            .prepare("SELECT 1 FROM tracks WHERE id = ?1")
            .map_err(guasto)?;
        let mut ancora = Vec::with_capacity(cancellata.brani.len());
        for track_id in &cancellata.brani {
            if esiste.exists([track_id]).map_err(guasto)? {
                ancora.push(*track_id);
            }
        }
        ancora
    };
    riscrivi_ordine(&tx, id, &ancora)?;

    for (content_key, posizione) in &cancellata.posti_orfani {
        tx.execute(
            "INSERT OR IGNORE INTO playlist_orfane (content_key, playlist_id, position)
             SELECT ?1, ?2, ?3
             WHERE EXISTS (SELECT 1 FROM brani_orfani WHERE content_key = ?1)",
            rusqlite::params![content_key, id, posizione],
        )
        .map_err(guasto)?;
    }
    for desiderato in &cancellata.desiderati {
        // Solo se nessuno l'ha riagganciato nel frattempo a un'altra playlist.
        tx.execute(
            "UPDATE desiderati SET playlist_id = ?1 WHERE id = ?2 AND playlist_id IS NULL",
            rusqlite::params![id, desiderato],
        )
        .map_err(guasto)?;
    }
    match cancellata.lapide_di_prima {
        None => tx.execute(
            "DELETE FROM sync_tombstones WHERE kind = 'playlist' AND key = ?1",
            [&cancellata.chiave],
        ),
        Some(quando) => tx.execute(
            "UPDATE sync_tombstones SET deleted_at = ?2 WHERE kind = 'playlist' AND key = ?1",
            rusqlite::params![cancellata.chiave, quando],
        ),
    }
    .map_err(guasto)?;

    tx.commit().map_err(guasto)?;
    read_one(connection, id)
}

/// Com'è andata un'aggiunta a una playlist.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Aggiunta {
    /// La playlist dopo l'aggiunta.
    pub playlist: PlaylistSummary,
    /// Quanti brani sono entrati.
    pub aggiunti: usize,
    /// Quanti si sono lasciati fuori perché in libreria non c'erano più.
    ///
    /// Il numero da dire a chi ha trascinato: senza, dieci brani trascinati e
    /// otto arrivati sembrano un'aggiunta riuscita a metà per un guasto.
    pub saltati: usize,
}

/// Aggiunge brani in fondo a una playlist, e dice quanti non c'erano più.
///
/// I duplicati sono ammessi — lo stesso brano può comparire più volte, ed è
/// voluto — quindi l'unico controllo, oltre alla posizione, è che il brano
/// **esista ancora**.
///
/// # Perché quel controllo, e perché qui
///
/// Perché gli identificativi arrivano dalla finestra, e la finestra li ha letti
/// prima: fra il momento in cui si è aperta la selezione e quello in cui si è
/// lasciata sulla playlist una scansione può aver tolto dei brani. Con la
/// musica su un NAS è il caso normale — la share che singhiozza durante una
/// scansione fa sparire righe mentre l'elenco a schermo le mostra ancora — e la
/// chiave esterna di `playlist_tracks` rifiutava l'inserimento del primo
/// brano sparito, facendo fallire **tutta** l'aggiunta con «il database ha
/// rifiutato una richiesta». Due brani spariti su cento costavano i cento.
///
/// Dentro la stessa transazione dell'inserimento, e `Immediate`: fra il
/// controllo e la scrittura nessun altro scrittore può togliere il brano.
///
/// # Errori
///
/// `library.playlistNotFound`, `library.playlistIsSmart`, `db.*`.
pub fn aggiungi_brani(
    connection: &mut Connection,
    id: i64,
    track_ids: &[i64],
) -> Result<Aggiunta, AppError> {
    let tx = transazione_di_scrittura(connection)
        .map_err(|err| db_error("aggiunta a una playlist", &err))?;
    modificabile(&tx, id)?;
    let (aggiunti, saltati) = accoda(&tx, id, track_ids)?;
    // Toccata solo se è cambiata: una data nuova su una playlist identica
    // sveglierebbe la sincronizzazione per niente.
    if aggiunti > 0 {
        tocca(&tx, id)?;
    }
    tx.commit()
        .map_err(|err| db_error("aggiunta a una playlist", &err))?;
    Ok(Aggiunta {
        playlist: read_one(connection, id)?,
        aggiunti,
        saltati,
    })
}

/// Crea una playlist e ci mette dentro dei brani, in una transazione sola.
///
/// # Perché non [`create`] e poi [`aggiungi_brani`]
///
/// Perché erano due chiamate dalla finestra, e fra le due poteva succedere di
/// tutto: un'aggiunta che falliva lasciava una playlist **vuota** col nome
/// appena scelto, e il nome — ormai occupato — faceva fallire anche il secondo
/// tentativo con «esiste già». Qui la riga e i brani entrano insieme o non
/// entra niente.
///
/// I brani che nel frattempo sono usciti dalla libreria si saltano e si
/// contano, come in [`aggiungi_brani`]: la playlist nasce lo stesso, perché il
/// nome l'ha scelto chi ha chiesto di crearla.
///
/// # Errori
///
/// `library.playlistNameInvalid`, `library.playlistExists`, `db.*`. In tutti e
/// tre i casi la playlist non c'è.
pub fn crea_e_aggiungi(
    connection: &mut Connection,
    name: &str,
    track_ids: &[i64],
) -> Result<Aggiunta, AppError> {
    let tx = transazione_di_scrittura(connection)
        .map_err(|err| db_error("creazione di una playlist", &err))?;
    let id = crea_riga(&tx, name)?;
    let (aggiunti, saltati) = accoda(&tx, id, track_ids)?;
    tx.commit()
        .map_err(|err| db_error("creazione di una playlist", &err))?;
    Ok(Aggiunta {
        playlist: read_one(connection, id)?,
        aggiunti,
        saltati,
    })
}

/// Scrive dei brani in fondo a una playlist, saltando quelli che non ci sono
/// più, e dice quanti sono entrati e quanti saltati.
///
/// Dentro la transazione di chi chiama, e `Immediate`: fra il controllo e la
/// scrittura nessun altro scrittore può togliere il brano. Vedi
/// [`aggiungi_brani`] per il perché del controllo.
fn accoda(tx: &Transaction<'_>, id: i64, track_ids: &[i64]) -> Result<(usize, usize), AppError> {
    let guasto = |err: rusqlite::Error| db_error("aggiunta a una playlist", &err);
    let mut aggiunti = 0_usize;
    let mut saltati = 0_usize;
    let prima_libera: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM playlist_tracks WHERE playlist_id = ?1",
            [id],
            |row| row.get(0),
        )
        .map_err(guasto)?;
    let mut esiste = tx
        .prepare("SELECT 1 FROM tracks WHERE id = ?1")
        .map_err(guasto)?;
    let mut inserisci = tx
        .prepare(
            "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)",
        )
        .map_err(guasto)?;
    let mut posizione = prima_libera;
    for track_id in track_ids {
        if !esiste.exists([track_id]).map_err(guasto)? {
            saltati += 1;
            continue;
        }
        inserisci
            .execute(rusqlite::params![id, track_id, posizione])
            .map_err(guasto)?;
        posizione = posizione.saturating_add(1);
        aggiunti += 1;
    }
    Ok((aggiunti, saltati))
}

/// Aggiunge brani in fondo a una playlist.
///
/// [`aggiungi_brani`] senza il conto dei saltati, per chi non ha dove dirlo.
///
/// # Errori
///
/// Gli stessi di [`aggiungi_brani`].
pub fn add_tracks(
    connection: &mut Connection,
    id: i64,
    track_ids: &[i64],
) -> Result<PlaylistSummary, AppError> {
    aggiungi_brani(connection, id, track_ids).map(|fatta| fatta.playlist)
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
    let tx = transazione_di_scrittura(connection)
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
    let tx = transazione_di_scrittura(connection)
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

    #[test]
    fn una_playlist_intelligente_e_la_sua_interrogazione() {
        use aether_domain::regole::{Campo, Operatore, Regola, Valore};

        let c = libreria();
        let insieme = Insieme {
            regole: vec![Regola {
                campo: Campo::Durata,
                operatore: Operatore::Maggiore,
                valore: Valore::Numero(2500),
            }],
            ..Insieme::default()
        };
        let creata = create_smart(&c, "I lunghi", &insieme).expect("creata");
        assert!(creata.is_smart);
        // Nasce **piena**: l'appartenenza esiste già, è la libreria filtrata.
        assert_eq!(creata.tracks, 2, "l'elenco deve contare i brani veri");
        assert_eq!(creata.duration_ms, 7000);
        // In ordine da scaffale: stesso artista e stesso album, nessun numero
        // di traccia, quindi decide il titolo.
        assert_eq!(posizioni(&c, creata.id), ["Quattro", "Tre"]);

        // E cambia da sola quando cambia la libreria, senza che nessuno la
        // tocchi: è la ragione per cui i brani non si materializzano.
        c.execute(
            "INSERT INTO tracks (id, path, track_key, title, artist, album,
                                 duration_ms, file_size, date_added, date_modified)
             VALUES (5, 'e.mp3', 'k5', 'Cinque', 'Art', 'Al', 9000, 1, 1, 1)",
            [],
        )
        .expect("brano nuovo");
        assert_eq!(posizioni(&c, creata.id).len(), 3);
    }

    #[test]
    fn una_playlist_intelligente_non_si_modifica_a_mano() {
        // Il controllo c'era da sempre — `modificabile` — e fino a oggi non
        // c'era modo di creare una playlist che lo facesse scattare.
        let mut c = libreria();
        let creata = create_smart(&c, "Tutto", &Insieme::default()).expect("creata");
        let esito = add_tracks(&mut c, creata.id, &[1]);
        assert!(matches!(
            esito.map_err(|e| e.code().clone()),
            Err(ErrorCode::LibraryPlaylistIsSmart { .. })
        ));
    }

    #[test]
    fn le_regole_di_una_playlist_a_mano_non_si_riscrivono() {
        // Cambiare le regole di una playlist a mano vorrebbe dire convertirla,
        // e una conversione butta via righe messe lì a una a una.
        let c = libreria();
        let creata = create(&c, "A mano").expect("creata");
        assert!(set_rules(&c, creata.id, &Insieme::default()).is_err());
        assert_eq!(regole_di(&c, creata.id), Ok(None));
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
    fn un_brano_sparito_nel_frattempo_si_salta_e_non_ferma_gli_altri() {
        // Il caso del NAS: la finestra ha ancora l'identificativo 99 di un
        // brano che una scansione ha tolto. Prima la chiave esterna rifiutava
        // quella riga e con lei **tutta** l'aggiunta; adesso entrano gli altri
        // e il conto dice quanti mancavano.
        let mut connection = libreria();
        let p = create(&connection, "Da un NAS").expect("creata").id;
        let fatta = aggiungi_brani(&mut connection, p, &[3, 99, 1, 98]).expect("aggiunti");
        assert_eq!(fatta.aggiunti, 2);
        assert_eq!(fatta.saltati, 2);
        assert_eq!(fatta.playlist.tracks, 2);
        // Le posizioni restano contigue: il buco del brano saltato non c'è.
        assert_eq!(posizioni(&connection, p), ["Tre", "Uno"]);

        // E un'aggiunta fatta solo di brani spariti non è un guasto: è
        // un'aggiunta di niente, detta come tale.
        let vuota = aggiungi_brani(&mut connection, p, &[97]).expect("niente da aggiungere");
        assert_eq!((vuota.aggiunti, vuota.saltati), (0, 1));
        assert_eq!(posizioni(&connection, p), ["Tre", "Uno"]);
    }

    #[test]
    fn un_titolo_importato_troppo_lungo_si_tronca_e_passa() {
        // La regressione della 3.0: il tetto dei nomi rifiutava l'importazione
        // intera di un concerto con la scaletta nel titolo.
        let lunghissimo = "Live at the Fillmore — ".repeat(40);
        let corto = nome_da_importare(&lunghissimo);
        assert!(corto.chars().count() <= NOME_MASSIMO);
        assert!(
            corto.chars().count() > NOME_MASSIMO - 5,
            "si taglia al tetto, non prima"
        );
        assert!(corto.ends_with('…'));
        assert!(
            !corto.trim_end_matches('…').ends_with(' '),
            "niente spazio prima dell'ellissi: «{corto}»"
        );
        assert!(nome_utilizzabile(&corto).is_ok(), "il nome troncato passa");

        // Il taglio cade su un confine di carattere anche quando i caratteri
        // sono di più byte: si conta in `char`, come il tetto.
        let giapponese = "東京".repeat(300);
        let tagliato = nome_da_importare(&giapponese);
        assert_eq!(tagliato.chars().count(), NOME_MASSIMO);
        assert!(nome_utilizzabile(&tagliato).is_ok());

        // Sotto il tetto non si tocca niente, a parte gli spazi ai bordi.
        assert_eq!(nome_da_importare("  Serata  "), "Serata");
        // E chi scrive un nome a mano continua a sentirsi dire di no.
        assert!(nome_utilizzabile(&lunghissimo).is_err());
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
    fn un_brano_cancellato_esce_dalla_playlist_e_non_sposta_il_bersaglio() {
        // La cascata delle chiavi esterne toglie la riga di `playlist_tracks` ma
        // **non** ricompatta le posizioni: dopo questa cancellazione restano
        // occupate la 0, la 2 e la 3. È innocuo, e questa prova è il motivo per
        // cui lo si può affermare: `remove_at` e `reorder` non leggono quel
        // numero, lavorano sull'elenco ordinato che `ordine_attuale` dà — e al
        // primo gesto lo riscrivono compatto.
        let mut connection = libreria();
        let p = create(&connection, "Mista").expect("creata").id;
        add_tracks(&mut connection, p, &[1, 2, 3, 4]).expect("aggiunti");

        connection
            .execute("DELETE FROM tracks WHERE id = 2", [])
            .expect("cancellazione");
        assert_eq!(posizioni(&connection, p), ["Uno", "Tre", "Quattro"]);

        // La posizione 1 di quel che si vede è «Tre», non il buco lasciato da
        // «Due».
        remove_at(&mut connection, p, 1).expect("tolto");
        assert_eq!(posizioni(&connection, p), ["Uno", "Quattro"]);
        reorder(&mut connection, p, 1, 0).expect("spostato");
        assert_eq!(posizioni(&connection, p), ["Quattro", "Uno"]);
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
    fn crea_e_aggiungi_mette_la_playlist_e_i_brani_insieme() {
        // «Nuova playlist e aggiungi» in un colpo solo, e con il conto di chi
        // nel frattempo era uscito dalla libreria.
        let mut connection = libreria();
        let fatta = crea_e_aggiungi(&mut connection, "Al volo", &[2, 99, 1]).expect("creata");
        assert_eq!((fatta.aggiunti, fatta.saltati), (2, 1));
        assert_eq!(fatta.playlist.name, "Al volo");
        assert_eq!(fatta.playlist.tracks, 2);
        assert_eq!(fatta.playlist.duration_ms, 3000);
        assert_eq!(posizioni(&connection, fatta.playlist.id), ["Due", "Uno"]);
    }

    #[test]
    fn se_l_aggiunta_fallisce_non_resta_una_playlist_vuota() {
        // Il difetto di prima: due chiamate, e se la seconda falliva restava la
        // playlist vuota col nome occupato. Un vincolo che rifiuta ogni riga di
        // `playlist_tracks` fa fallire l'aggiunta a metà strada.
        let mut connection = libreria();
        connection
            .execute_batch(
                "CREATE TRIGGER rifiuta BEFORE INSERT ON playlist_tracks
                 BEGIN SELECT RAISE(ABORT, 'rifiutata'); END;",
            )
            .expect("vincolo");
        assert!(crea_e_aggiungi(&mut connection, "Mai nata", &[1, 2]).is_err());
        assert!(
            list(&connection).expect("elenco").is_empty(),
            "la playlist non deve sopravvivere all'aggiunta fallita"
        );
        connection
            .execute_batch("DROP TRIGGER rifiuta;")
            .expect("vincolo tolto");
        // E il nome è ancora libero: il secondo tentativo riesce.
        let fatta = crea_e_aggiungi(&mut connection, "Mai nata", &[1]).expect("creata");
        assert_eq!(fatta.aggiunti, 1);
    }

    #[test]
    fn crea_e_aggiungi_con_un_nome_preso_non_tocca_niente() {
        let mut connection = libreria();
        create(&connection, "Rock").expect("creata");
        let err = crea_e_aggiungi(&mut connection, "rock", &[1, 2]).expect_err("doppione");
        assert_eq!(err.code().kind().code(), "library.playlistExists");
        let elenco = list(&connection).expect("elenco");
        assert_eq!(elenco.len(), 1);
        assert_eq!(elenco[0].tracks, 0, "la playlist che c'era non si riempie");
    }

    /// Quante righe dice una query di conteggio.
    fn conta(connection: &Connection, sql: &str) -> i64 {
        connection
            .query_row(sql, [], |row| row.get(0))
            .expect("conteggio")
    }

    #[test]
    fn cancellare_e_ripristinare_rimette_tutto_com_era() {
        let mut connection = libreria();
        let p = create(&connection, "Serata").expect("creata");
        add_tracks(&mut connection, p.id, &[3, 1, 3]).expect("aggiunti");
        connection
            .execute_batch(&format!(
                "UPDATE playlists SET description = 'Per sabato', created_at = 7,
                                      updated_at = 8 WHERE id = {id};
                 INSERT INTO brani_orfani (content_key, orfano_at) VALUES ('via', 1);
                 INSERT INTO playlist_orfane (content_key, playlist_id, position)
                      VALUES ('via', {id}, 1);
                 INSERT INTO desiderati (track_key, title, source_kind, source_id,
                                         source_title, added_at, playlist_id, position)
                      VALUES ('kx', 'Manca', 'playlist', 'sx', 'Fuori', 1, {id}, 3);",
                id = p.id
            ))
            .expect("contorno");

        let cancellata = cancella(&mut connection, p.id).expect("cancellata");
        assert_eq!(cancellata.nome(), "Serata");
        assert!(list(&connection).expect("elenco").is_empty());
        assert_eq!(
            conta(
                &connection,
                "SELECT COUNT(*) FROM sync_tombstones WHERE kind = 'playlist'"
            ),
            1
        );
        assert_eq!(
            conta(&connection, "SELECT COUNT(*) FROM playlist_orfane"),
            0
        );
        assert_eq!(
            conta(
                &connection,
                "SELECT COUNT(*) FROM desiderati WHERE playlist_id IS NULL"
            ),
            1
        );

        let tornata = ripristina(&mut connection, &cancellata).expect("ripristinata");
        assert_eq!(tornata.id, p.id, "l'identificativo era libero: è lo stesso");
        assert_eq!(tornata.name, "Serata");
        assert_eq!(tornata.key, p.key);
        assert_eq!(tornata.description.as_deref(), Some("Per sabato"));
        assert_eq!(tornata.tracks, 3);
        assert_eq!(posizioni(&connection, p.id), ["Tre", "Uno", "Tre"]);
        // La data di modifica è adesso: la lapide può essere già partita, e
        // solo una playlist toccata dopo di lei sopravvive alla fusione.
        assert!(tornata.updated_at > 8);
        assert_eq!(
            conta(&connection, "SELECT created_at FROM playlists"),
            7,
            "la nascita resta quella di prima"
        );
        assert_eq!(
            conta(&connection, "SELECT COUNT(*) FROM sync_tombstones"),
            0,
            "la lapide di questa cancellazione se ne va con lei"
        );
        assert_eq!(
            conta(
                &connection,
                &format!(
                    "SELECT COUNT(*) FROM playlist_orfane WHERE playlist_id = {}",
                    p.id
                )
            ),
            1
        );
        assert_eq!(
            conta(
                &connection,
                &format!(
                    "SELECT COUNT(*) FROM desiderati WHERE playlist_id = {}",
                    p.id
                )
            ),
            1
        );
    }

    #[test]
    fn un_ripristino_rimette_la_lapide_che_c_era_gia() {
        // Una playlist cancellata tempo fa e rifatta col nome di prima: la sua
        // lapide c'era già, e annullare la seconda cancellazione non deve
        // cancellare anche il ricordo della prima.
        let mut connection = libreria();
        let p = create(&connection, "Ritorno").expect("creata");
        connection
            .execute(
                "INSERT INTO sync_tombstones (kind, key, deleted_at) VALUES ('playlist', ?1, 5)",
                [&p.key],
            )
            .expect("lapide vecchia");
        let cancellata = cancella(&mut connection, p.id).expect("cancellata");
        assert!(conta(&connection, "SELECT deleted_at FROM sync_tombstones") > 5);
        ripristina(&mut connection, &cancellata).expect("ripristinata");
        assert_eq!(
            conta(&connection, "SELECT deleted_at FROM sync_tombstones"),
            5
        );
    }

    #[test]
    fn un_ripristino_salta_i_brani_usciti_e_resta_contiguo() {
        let mut connection = libreria();
        let p = create(&connection, "Mista").expect("creata").id;
        add_tracks(&mut connection, p, &[1, 2, 3]).expect("aggiunti");
        let cancellata = cancella(&mut connection, p).expect("cancellata");
        connection
            .execute("DELETE FROM tracks WHERE id = 2", [])
            .expect("brano tolto");
        let tornata = ripristina(&mut connection, &cancellata).expect("ripristinata");
        assert_eq!(tornata.tracks, 2);
        assert_eq!(posizioni(&connection, p), ["Uno", "Tre"]);
        assert_eq!(
            conta(&connection, "SELECT MAX(position) FROM playlist_tracks"),
            1,
            "nessun buco nelle posizioni"
        );
    }

    #[test]
    fn un_ripristino_con_l_identificativo_ripreso_ne_prende_un_altro() {
        // SQLite riusa il più alto quando lo si libera: una playlist creata
        // nei secondi dell'«Annulla» prende proprio quello.
        let mut connection = libreria();
        let p = create(&connection, "Prima").expect("creata");
        add_tracks(&mut connection, p.id, &[4]).expect("aggiunto");
        let cancellata = cancella(&mut connection, p.id).expect("cancellata");
        let nuova = create(&connection, "Nel frattempo").expect("creata");
        assert_eq!(nuova.id, p.id, "la premessa della prova");
        let tornata = ripristina(&mut connection, &cancellata).expect("ripristinata");
        assert_ne!(tornata.id, nuova.id);
        assert_eq!(posizioni(&connection, tornata.id), ["Quattro"]);
        assert!(
            posizioni(&connection, nuova.id).is_empty(),
            "l'altra resta com'è"
        );
    }

    #[test]
    fn un_ripristino_col_nome_ripreso_non_tocca_niente() {
        let mut connection = libreria();
        let p = create(&connection, "Doppia").expect("creata");
        let cancellata = cancella(&mut connection, p.id).expect("cancellata");
        create(&connection, "doppia").expect("rifatta a mano");
        let err = ripristina(&mut connection, &cancellata).expect_err("nome preso");
        assert_eq!(err.code().kind().code(), "library.playlistExists");
        assert_eq!(list(&connection).expect("elenco").len(), 1);
        assert_eq!(
            conta(&connection, "SELECT COUNT(*) FROM sync_tombstones"),
            1,
            "la lapide resta: la cancellazione non è stata annullata"
        );
    }

    #[test]
    fn una_playlist_intelligente_torna_con_le_sue_regole() {
        use aether_domain::regole::{Campo, Operatore, Regola, Valore};
        let mut connection = libreria();
        let insieme = Insieme {
            regole: vec![Regola {
                campo: Campo::Durata,
                operatore: Operatore::Maggiore,
                valore: Valore::Numero(2500),
            }],
            ..Insieme::default()
        };
        let p = create_smart(&connection, "Lunghi", &insieme).expect("creata");
        let cancellata = cancella(&mut connection, p.id).expect("cancellata");
        let tornata = ripristina(&mut connection, &cancellata).expect("ripristinata");
        assert!(tornata.is_smart);
        assert_eq!(tornata.tracks, 2);
        assert_eq!(regole_di(&connection, tornata.id), Ok(Some(insieme)));
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
