//! La libreria come la vedono i dispositivi collegati via internet.
//!
//! # Solo i file
//!
//! Si condividono i brani che sono **file** in una cartella sorvegliata,
//! locale o di rete: è la musica di questo dispositivo, e la si può leggere a
//! pezzi. Non si condividono i brani delle cartelle in cloud (`nuvola://…`),
//! che l'altro può collegare da sé con il suo account e che passerebbero due
//! volte per internet, né quelli di catalogo, che un file non ce l'hanno e
//! che l'altro trova nel catalogo da sé. Un album o un artista c'è se ha
//! almeno un brano condiviso.
//!
//! # Niente percorsi
//!
//! [`BranoRemoto`] è un [`super::TrackSummary`] senza il percorso: all'altro
//! non serve (legge per id), e un percorso dice di una macchina più di quel
//! che serve — il nome dell'utente, le cartelle, i dischi. Al loro posto
//! l'estensione (per il decodificatore), la dimensione, la versione e, per i
//! brani d'immagine CD, il pezzo da suonare.
//!
//! Lo stesso per gli album: la chiave di un disco senza «artista dell'album»
//! porta dentro la cartella in cui sta (`moon safari c:/users/…/moon safari`),
//! e all'altro arriva al suo posto un'impronta ([`impronta_album`]), stabile
//! finché non cambia la chiave, che solo chi serve sa ritradurre.

use rusqlite::OptionalExtension as _;

use super::interrogazioni::{COLONNE_ALBUM, album_from_row};
use super::*;

/// La condizione dei brani condivisi, per `tracks` con l'alias `t`. Il cloud
/// per intervallo di prefisso, come in `enrich`: `'/' + 1` è `'0'`.
const CONDIVISO: &str = "t.path IS NOT NULL AND t.source_service IS NULL \
     AND NOT (t.path >= 'nuvola://' AND t.path < 'nuvola:/0')";

/// La stessa per `tracks` senza alias, dove l'espressione dell'artista deve
/// restare quella dell'indice.
const CONDIVISO_SENZA_ALIAS: &str = "path IS NOT NULL AND source_service IS NULL \
     AND NOT (path >= 'nuvola://' AND path < 'nuvola:/0')";

/// Il pezzo di un'immagine CD da suonare, in fotogrammi da 1/75 di secondo
/// dall'inizio del file. Vedi la migrazione 034.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segmento {
    /// Da dove.
    pub inizio_cd: i64,
    /// Fin dove; `None` per l'ultimo brano, che finisce col file.
    pub fine_cd: Option<i64>,
}

/// Un brano di un altro dispositivo.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranoRemoto {
    /// L'id nella libreria **dell'altro**.
    pub id: i64,
    /// Il titolo.
    pub title: String,
    /// L'interprete.
    pub artist: String,
    /// L'album.
    pub album: String,
    /// L'impronta dell'album ([`impronta_album`]): da ripetere all'altro.
    pub album_key: Option<String>,
    /// Numero di traccia.
    pub track_number: Option<i64>,
    /// Numero di disco.
    pub disc_number: Option<i64>,
    /// Durata in millisecondi.
    pub duration_ms: i64,
    /// L'anno.
    pub year: Option<i64>,
    /// L'impronta della copertina, da chiedere all'altro.
    pub cover_art_hash: Option<String>,
    /// Quante volte l'ha ascoltato il proprietario.
    pub play_count: i64,
    /// È fra i suoi preferiti.
    pub liked: bool,
    /// Il suo voto.
    pub rating: i64,
    /// L'estensione del file da leggere, minuscola e senza punto.
    pub estensione: String,
    /// Quanti byte.
    pub dimensione: i64,
    /// Cambia se il file cambia: dimensione e data.
    pub versione: String,
    /// Per un brano d'immagine CD, il pezzo.
    pub segmento: Option<Segmento>,
}

/// Il file di un brano condiviso, per leggerlo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCondiviso {
    /// Dove sta. Per un brano d'immagine, l'immagine.
    pub percorso: String,
    /// Quanti byte, secondo la libreria.
    pub dimensione: i64,
    /// L'ultima modifica, ms, secondo la libreria.
    pub modificato_ms: i64,
}

/// L'impronta di una chiave d'album, per l'altro: trentadue cifre di BLAKE3.
#[must_use]
pub fn impronta_album(album_key: &str) -> String {
    let hash = blake3::hash(album_key.as_bytes());
    hash.to_hex().chars().take(32).collect()
}

/// La chiave d'album di un'impronta, fra gli album con brani condivisi.
fn chiave_dell_impronta(
    connection: &Connection,
    impronta: &str,
) -> Result<Option<String>, AppError> {
    let cosa = "album di un'impronta";
    let mut statement = connection
        .prepare_cached(&format!(
            "SELECT DISTINCT t.album_key FROM tracks t
             WHERE t.album_key IS NOT NULL AND {CONDIVISO}"
        ))
        .map_err(|err| db_error(cosa, &err))?;
    let chiavi = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|err| db_error(cosa, &err))?;
    for chiave in chiavi {
        let chiave = chiave.map_err(|err| db_error(cosa, &err))?;
        if impronta_album(&chiave) == impronta {
            return Ok(Some(chiave));
        }
    }
    Ok(None)
}

/// La versione di un file, come la dice [`BranoRemoto::versione`]. Chi serve
/// la ricalcola dal disco a ogni lettura.
#[must_use]
pub fn versione(dimensione: i64, modificato_ms: i64) -> String {
    format!("{dimensione}-{modificato_ms}")
}

const COLONNE_REMOTO: &str = "t.id, t.title, t.artist, t.album, t.album_key, t.track_number,
     t.disc_number, t.duration_ms, t.year, t.cover_art_hash, t.play_count, t.liked, t.rating,
     COALESCE(t.immagine, t.path), t.file_size, t.date_modified, t.inizio_cd, t.fine_cd";

fn brano_da_riga(row: &Row<'_>) -> rusqlite::Result<BranoRemoto> {
    let percorso: String = row.get(13)?;
    let dimensione: i64 = row.get(14)?;
    let modificato: i64 = row.get(15)?;
    let inizio: Option<i64> = row.get(16)?;
    Ok(BranoRemoto {
        id: row.get(0)?,
        title: row.get(1)?,
        artist: row.get(2)?,
        album: row.get(3)?,
        album_key: row.get::<_, Option<String>>(4)?.map(|k| impronta_album(&k)),
        track_number: row.get(5)?,
        disc_number: row.get(6)?,
        duration_ms: row.get(7)?,
        year: row.get(8)?,
        cover_art_hash: row.get(9)?,
        play_count: row.get(10)?,
        liked: row.get::<_, i64>(11)? != 0,
        rating: row.get(12)?,
        estensione: estensione(&percorso),
        dimensione,
        versione: versione(dimensione, modificato),
        segmento: inizio.map(|inizio_cd| Segmento {
            inizio_cd,
            fine_cd: row.get(17).ok().flatten(),
        }),
    })
}

/// L'estensione, senza dire altro del percorso. Con `/` e `\` tutti e due:
/// il percorso è di questa macchina, ma una cartella di rete li mescola.
fn estensione(percorso: &str) -> String {
    let nome = percorso.rsplit(['/', '\\']).next().unwrap_or("");
    nome.rsplit_once('.')
        .map(|(_, est)| est.to_ascii_lowercase())
        .filter(|est| est.len() <= 8 && est.bytes().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or_default()
}

fn righe_di_brani(
    connection: &Connection,
    sql: &str,
    parametri: impl rusqlite::Params,
    cosa: &str,
) -> Result<Vec<BranoRemoto>, AppError> {
    let mut statement = connection
        .prepare_cached(sql)
        .map_err(|err| db_error(cosa, &err))?;
    let rows = statement
        .query_map(parametri, brano_da_riga)
        .map_err(|err| db_error(cosa, &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error(cosa, &err))
}

fn righe_di_album(
    connection: &Connection,
    sql: &str,
    parametri: impl rusqlite::Params,
    cosa: &str,
) -> Result<Vec<AlbumSummary>, AppError> {
    let mut statement = connection
        .prepare_cached(sql)
        .map_err(|err| db_error(cosa, &err))?;
    let rows = statement
        .query_map(parametri, album_from_row)
        .map_err(|err| db_error(cosa, &err))?;
    let mut album = rows
        .collect::<Result<Vec<AlbumSummary>, _>>()
        .map_err(|err| db_error(cosa, &err))?;
    for disco in &mut album {
        disco.album_key = impronta_album(&disco.album_key);
    }
    Ok(album)
}

/// Una pagina di brani condivisi.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn brani(
    connection: &Connection,
    ordine: TrackOrder,
    da: i64,
    quanti: i64,
) -> Result<Vec<BranoRemoto>, AppError> {
    let sql = format!(
        "SELECT {COLONNE_REMOTO} FROM tracks t WHERE {CONDIVISO}
         ORDER BY {} LIMIT ?1 OFFSET ?2",
        ordine.sql()
    );
    righe_di_brani(
        connection,
        &sql,
        rusqlite::params![quanti, da],
        "brani condivisi",
    )
}

/// I brani condivisi di un album, dalla sua impronta, nell'ordine del disco.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn brani_album(connection: &Connection, impronta: &str) -> Result<Vec<BranoRemoto>, AppError> {
    let Some(album_key) = chiave_dell_impronta(connection, impronta)? else {
        return Ok(Vec::new());
    };
    let album_key = album_key.as_str();
    let sql = format!(
        "SELECT {COLONNE_REMOTO} FROM tracks t WHERE t.album_key = ?1 AND {CONDIVISO}
         ORDER BY t.disc_number, t.track_number, t.title COLLATE NOCASE"
    );
    righe_di_brani(connection, &sql, [album_key], "brani condivisi di un album")
}

/// Alcuni brani condivisi, per id: quelli che non lo sono, o non ci sono
/// più, mancano.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn brano(connection: &Connection, id: i64) -> Result<Option<BranoRemoto>, AppError> {
    let sql = format!("SELECT {COLONNE_REMOTO} FROM tracks t WHERE t.id = ?1 AND {CONDIVISO}");
    Ok(
        righe_di_brani(connection, &sql, [id], "un brano condiviso")?
            .into_iter()
            .next(),
    )
}

/// Una pagina degli album con almeno un brano condiviso.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn album(connection: &Connection, da: i64, quanti: i64) -> Result<Vec<AlbumSummary>, AppError> {
    let sql = format!(
        "SELECT {COLONNE_ALBUM} FROM albums a
         WHERE EXISTS (SELECT 1 FROM tracks t WHERE t.album_key = a.album_key AND {CONDIVISO})
         ORDER BY a.artist COLLATE NOCASE, a.year, a.title COLLATE NOCASE
         LIMIT ?1 OFFSET ?2"
    );
    righe_di_album(
        connection,
        &sql,
        rusqlite::params![quanti, da],
        "album condivisi",
    )
}

/// Gli album di un artista con almeno un suo brano condiviso: la stessa
/// chiave di [`super::albums_by_artist`].
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn album_artista(
    connection: &Connection,
    artista: &str,
    quanti: i64,
) -> Result<Vec<AlbumSummary>, AppError> {
    let sql = format!(
        "SELECT {COLONNE_ALBUM} FROM albums a
         WHERE a.album_key IN (SELECT album_key FROM tracks
                               WHERE {ARTISTA_EFFETTIVO} = ?1 AND {CONDIVISO_SENZA_ALIAS})
         ORDER BY a.year, a.title COLLATE NOCASE
         LIMIT ?2"
    );
    righe_di_album(
        connection,
        &sql,
        rusqlite::params![artista, quanti],
        "album condivisi di un artista",
    )
}

/// Gli artisti con almeno un brano condiviso, in ordine alfabetico del nome
/// normalizzato, a pagine: come [`super::list_artists`], contando solo i
/// brani condivisi.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn artisti(
    connection: &Connection,
    da: usize,
    quanti: usize,
) -> Result<Vec<ArtistSummary>, AppError> {
    let cosa = "artisti condivisi";
    let mut statement = connection
        .prepare_cached(&format!(
            "SELECT {ARTISTA_EFFETTIVO} AS nome, COUNT(DISTINCT album_key), COUNT(*)
             FROM tracks
             WHERE {ARTISTA_EFFETTIVO} <> '' AND {CONDIVISO_SENZA_ALIAS}
             GROUP BY nome"
        ))
        .map_err(|err| db_error(cosa, &err))?;
    let mut artisti: Vec<ArtistSummary> = statement
        .query_map([], |row| {
            let name: String = row.get(0)?;
            Ok(ArtistSummary {
                sort_name: rymembra_domain::keys::sort_name(&name),
                name,
                albums: row.get(1)?,
                tracks: row.get(2)?,
                covers: Vec::new(),
            })
        })
        .map_err(|err| db_error(cosa, &err))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error(cosa, &err))?;
    artisti.sort_by(|a, b| {
        a.sort_name
            .cmp(&b.sort_name)
            .then_with(|| a.name.cmp(&b.name))
    });
    let mut pagina: Vec<ArtistSummary> = artisti.into_iter().skip(da).take(quanti).collect();
    if pagina.is_empty() {
        return Ok(pagina);
    }
    // Le copertine solo per la pagina: fino a quattro per artista, dagli
    // album più vecchi, come la griglia di qui.
    let mut copertine = connection
        .prepare_cached(&format!(
            "SELECT MIN(cover_art_hash) AS hash, MIN(year) AS anno
             FROM tracks
             WHERE {ARTISTA_EFFETTIVO} = ?1 AND cover_art_hash IS NOT NULL
               AND {CONDIVISO_SENZA_ALIAS}
             GROUP BY album_key
             ORDER BY anno
             LIMIT 8"
        ))
        .map_err(|err| db_error(cosa, &err))?;
    for artista in &mut pagina {
        let righe = copertine
            .query_map([&artista.name], |row| row.get::<_, String>(0))
            .map_err(|err| db_error(cosa, &err))?;
        for riga in righe {
            let hash = riga.map_err(|err| db_error(cosa, &err))?;
            if artista.covers.len() < 4 && !artista.covers.contains(&hash) {
                artista.covers.push(hash);
            }
        }
    }
    Ok(pagina)
}

/// Cerca fra i brani condivisi, con la stessa traduzione della ricerca di
/// qui ([`fts_query`]).
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn cerca(
    connection: &Connection,
    testo: &str,
    da: i64,
    quanti: i64,
) -> Result<Vec<BranoRemoto>, AppError> {
    let espressione = fts_query(testo);
    if espressione.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT {COLONNE_REMOTO}
         FROM tracks_fts
         JOIN tracks t ON t.id = tracks_fts.rowid
         WHERE tracks_fts MATCH ?1 AND {CONDIVISO}
         ORDER BY bm25(tracks_fts) LIMIT ?2 OFFSET ?3"
    );
    righe_di_brani(
        connection,
        &sql,
        rusqlite::params![espressione, quanti, da],
        "ricerca fra i brani condivisi",
    )
}

/// Quanti brani condivisi risponderebbero a questa ricerca.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn cerca_conteggio(connection: &Connection, testo: &str) -> Result<i64, AppError> {
    let espressione = fts_query(testo);
    if espressione.is_empty() {
        return Ok(0);
    }
    connection
        .prepare_cached(&format!(
            "SELECT COUNT(*) FROM tracks_fts JOIN tracks t ON t.id = tracks_fts.rowid
             WHERE tracks_fts MATCH ?1 AND {CONDIVISO}"
        ))
        .and_then(|mut statement| statement.query_row([&espressione], |row| row.get(0)))
        .map_err(|err| db_error("conteggio della ricerca fra i brani condivisi", &err))
}

/// Il file di un brano condiviso, se lo è e se sta ancora sotto una delle
/// cartelle sorvegliate: una cartella tolta smette di essere condivisa subito,
/// non alla prossima scansione.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn file_del_brano(connection: &Connection, id: i64) -> Result<Option<FileCondiviso>, AppError> {
    let cosa = "file di un brano condiviso";
    let trovato = connection
        .prepare_cached(&format!(
            "SELECT COALESCE(t.immagine, t.path), t.file_size, t.date_modified
             FROM tracks t WHERE t.id = ?1 AND {CONDIVISO}"
        ))
        .and_then(|mut statement| {
            statement
                .query_row([id], |row| {
                    Ok(FileCondiviso {
                        percorso: row.get(0)?,
                        dimensione: row.get(1)?,
                        modificato_ms: row.get(2)?,
                    })
                })
                .optional()
        })
        .map_err(|err| db_error(cosa, &err))?;
    let Some(file) = trovato else {
        return Ok(None);
    };
    let radici: Vec<String> =
        crate::settings::read_json(connection, crate::settings::CHIAVE_CARTELLE)
            .ok()
            .flatten()
            .unwrap_or_default();
    let regole = PathRules::for_current_platform();
    let dentro = radici
        .iter()
        .filter(|r| !r.starts_with("nuvola://"))
        .any(|r| is_under(&file.percorso, r, regole));
    Ok(dentro.then_some(file))
}
