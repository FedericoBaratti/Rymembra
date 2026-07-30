//! La scansione per intero: dal disco a una libreria che si può interrogare.
//!
//! Il piano lo decide [`aether_domain::scan_plan`], che è puro. Qui si legge
//! quel che il piano dice di leggere e si scrive quel che dice di scrivere.
//!
//! # Leggere è lento, scrivere è istantaneo
//!
//! Millequattrocento file richiedono minuti di lettura tag e ricodifica
//! copertine; le millequattrocento righe corrispondenti si scrivono in
//! millisecondi. Il rapporto fra i due decide la forma di questo modulo: si
//! legge **fuori** dalla transazione, e si entra in transazione solo per
//! scrivere.
//!
//! Il contrario — aprire la transazione e leggere i file dentro — terrebbe una
//! scrittura aperta per minuti. In WAL non bloccherebbe chi legge, ma
//! bloccherebbe ogni altra scrittura per tutta la scansione, e un guasto
//! sull'ultimo file butterebbe via anche il primo.
//!
//! # Perché a lotti, e non tutto in una transazione sola
//!
//! Un lotto per volta: si leggono [`LOTTO`] file, si scrivono, si passa al
//! successivo. Due motivi, e nessuno dei due è la velocità.
//!
//! Il primo è la memoria: tenere centomila righe lette in attesa di una
//! transazione finale vuol dire tenere in RAM centomila testi di canzoni.
//!
//! Il secondo è che **una scansione interrotta a metà non è un danno**. La
//! libreria è un indice del disco, non il dato: quel che è stato scritto è
//! giusto, quel che manca lo trova la passata dopo — che infatti lo classifica
//! come «da inserire» perché confronta col disco, non con un registro di
//! avanzamento. Fra «tutto o niente» e «quel che si è fatto resta», qui il
//! secondo è strettamente migliore. È lo stesso ragionamento delle migrazioni,
//! una per transazione invece che tutte insieme.
//!
//! # Ogni file si legge una volta sola
//!
//! Riconoscere gli spostamenti (vedi [`match_moved_tracks`]) richiede la chiave
//! dei file nuovi, che si conosce solo dopo averli letti. Farne un giro a parte
//! prima della scrittura raddoppierebbe la parte lenta della scansione — e
//! proprio nel caso in cui gli spostamenti ci sono, cioè dopo un riordino, dove
//! riguardano ogni file della libreria. L'appaiamento avviene quindi lotto per
//! lotto, contro l'insieme delle sparizioni ancora libere.

use aether_domain::album::{AlbumMember, AlbumRow, album_group_key, build_album_groups};
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::keys::{TrackKey, TrackKeyInput};
use aether_domain::paths::{PathRules, file_stem};
use aether_domain::scan_plan::{
    DiscoveredFile, KnownTrack, RemoveReason, RemovedIdentity, ScanInput, ScanPlan,
    match_moved_tracks, plan_scan,
};
use rusqlite::{Connection, Row, Transaction};

use crate::covers::{CoverSource, CoverStore, StoredCover};
use crate::files::MusicFiles;
use crate::metadata::read_tags;

/// Quanti file si leggono prima di scriverli.
///
/// Cinquecento è dove il costo di aprire una transazione sparisce nel rumore
/// (una transazione ogni pochi secondi di lettura) senza che un'interruzione
/// costi più di qualche secondo di lavoro da rifare.
pub const LOTTO: usize = 500;

/// Traduce un errore di SQLite nel catalogo, tenendo il testo originale.
fn db_error(detail: &str, err: &rusqlite::Error) -> AppError {
    AppError::new(ErrorCode::DbQueryFailed {
        detail: Some(detail.to_owned()),
    })
    .with_cause(err.to_string())
}

/// L'ora attuale in millisecondi. Zero se l'orologio è dietro l'epoca.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

/// Un brano letto dal disco, nella forma in cui il database lo accetta.
///
/// Le conversioni verso `i64` avvengono qui e non al momento di legare i
/// parametri: SQLite ha un tipo intero solo, e farle sparse nelle query
/// significherebbe ripeterle a ogni istruzione che tocca la stessa colonna.
#[derive(Debug, Clone)]
pub struct TrackRow {
    /// Il percorso come sta sul disco.
    pub path: String,
    /// L'identità fra dispositivi.
    pub track_key: String,
    /// Il titolo, o la radice del nome del file se i tag non ne hanno uno.
    pub title: String,
    /// L'interprete.
    pub artist: String,
    /// L'album.
    pub album: String,
    /// L'artista dell'album.
    pub album_artist: Option<String>,
    /// La chiave di raggruppamento: titolo normalizzato più cartella.
    pub album_key: String,
    /// L'anno.
    pub year: Option<i64>,
    /// Numero di traccia.
    pub track_number: Option<i64>,
    /// Numero di disco.
    pub disc_number: Option<i64>,
    /// Genere.
    pub genre: Option<String>,
    /// Durata in millisecondi.
    pub duration_ms: i64,
    /// Battiti al minuto.
    pub bpm: Option<f64>,
    /// Tonalità.
    pub musical_key: Option<String>,
    /// Commento.
    pub comment: Option<String>,
    /// Testo.
    pub lyrics: Option<String>,
    /// La copertina salvata nello store, se il file ne aveva una leggibile.
    pub cover: Option<StoredCover>,
    /// Bitrate in kbps.
    pub bitrate: Option<i64>,
    /// Frequenza di campionamento.
    pub sample_rate: Option<i64>,
    /// Canali.
    pub channels: Option<i64>,
    /// Il formato.
    pub codec: Option<String>,
    /// Dimensione del file.
    pub file_size: i64,
    /// Data di modifica del file, già troncata.
    pub date_modified: i64,
    /// ReplayGain della traccia.
    pub replaygain_track_db: Option<f64>,
    /// ReplayGain dell'album.
    pub replaygain_album_db: Option<f64>,
    /// Identificativo MusicBrainz della registrazione.
    pub mb_recording_id: Option<String>,
    /// Identificativo MusicBrainz del gruppo di pubblicazione.
    pub mb_release_group_id: Option<String>,
    /// Identificativo MusicBrainz della pubblicazione.
    pub mb_release_id: Option<String>,
}

/// Legge un file e ne costruisce la riga, salvando la copertina nello store.
///
/// # I ripieghi sui campi obbligatori
///
/// `title`, `artist` e `album` non possono essere nulli nello schema, e i file
/// senza tag esistono. Il titolo ripiega sulla radice del nome del file — che è
/// l'unica cosa che si sa di quel brano, e per come la gente nomina i file è
/// spesso anche giusta; gli altri due sui ripieghi del dominio.
///
/// I ripieghi entrano **anche nel calcolo di [`TrackKey`]**, e non è
/// indifferente: derivandola dai tag grezzi, ogni file senza tag avrebbe chiave
/// `||`, e la sincronizzazione li tratterebbe tutti come lo stesso brano.
///
/// Una copertina che non si decodifica non fa fallire il brano: si perde
/// l'immagine, si tiene la musica.
pub fn read_track(
    files: &dyn MusicFiles,
    covers: &CoverStore,
    file: &DiscoveredFile,
) -> Result<TrackRow, AppError> {
    let tags = read_tags(files, &file.path)?;

    let title = tags
        .title
        .clone()
        .unwrap_or_else(|| file_stem(&file.path).to_owned());
    let artist = tags
        .artist
        .clone()
        .unwrap_or_else(|| aether_domain::album::UNKNOWN_ARTIST.to_owned());
    let album = tags
        .album
        .clone()
        .unwrap_or_else(|| aether_domain::album::UNKNOWN_ALBUM.to_owned());

    let track_key = TrackKey::compute(TrackKeyInput {
        artist: Some(&artist),
        title: Some(&title),
        album: Some(&album),
    })
    .into_string();
    let album_key = album_group_key(&album, &file.path);

    let cover = tags
        .cover
        .as_ref()
        .and_then(|embedded| covers.store(&embedded.data, CoverSource::Tag).ok());

    Ok(TrackRow {
        path: file.path.clone(),
        track_key,
        title,
        artist,
        album,
        album_artist: tags.album_artist,
        album_key,
        year: tags.year.map(i64::from),
        track_number: tags.track_number.map(i64::from),
        disc_number: tags.disc_number.map(i64::from),
        genre: tags.genre,
        duration_ms: i64::try_from(tags.duration_ms).unwrap_or(0),
        bpm: tags.bpm,
        musical_key: tags.musical_key,
        comment: tags.comment,
        lyrics: tags.lyrics,
        cover,
        bitrate: tags.bitrate.map(i64::from),
        sample_rate: tags.sample_rate.map(i64::from),
        channels: tags.channels.map(i64::from),
        codec: tags.codec,
        file_size: i64::try_from(file.size_bytes).unwrap_or(i64::MAX),
        date_modified: file.modified_ms,
        replaygain_track_db: tags.replaygain_track_db.map(f64::from),
        replaygain_album_db: tags.replaygain_album_db.map(f64::from),
        mb_recording_id: tags.mb_recording_id,
        mb_release_group_id: tags.mb_release_group_id,
        mb_release_id: tags.mb_release_id,
    })
}

/// Le righe già in libreria, nella forma che serve a decidere.
///
/// Ordinate per identificativo: l'ordine di arrivo detta l'ordine del piano, e
/// un piano da mostrare all'utente non deve cambiare fra due aperture.
pub fn known_tracks(connection: &Connection) -> Result<Vec<KnownTrack>, AppError> {
    let mut statement = connection
        .prepare("SELECT id, path, date_modified FROM tracks ORDER BY id")
        .map_err(|err| db_error("elenco dei brani noti", &err))?;
    let rows = statement
        .query_map([], |row| {
            Ok(KnownTrack {
                id: row.get(0)?,
                path: row.get(1)?,
                modified_ms: row.get(2)?,
            })
        })
        .map_err(|err| db_error("elenco dei brani noti", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("elenco dei brani noti", &err))
}

/// Le chiavi di brano delle righe date, nello stesso ordine.
///
/// Una riga sparita nel frattempo si salta invece di far fallire tutto: fra il
/// calcolo del piano e questo momento un'altra scrittura può averla tolta, e
/// non è un motivo per rinunciare alla scansione.
fn track_keys_of(connection: &Connection, ids: &[i64]) -> Result<Vec<(i64, String)>, AppError> {
    let mut statement = connection
        .prepare("SELECT track_key FROM tracks WHERE id = ?1")
        .map_err(|err| db_error("chiave di un brano", &err))?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        match statement.query_row([id], |row| row.get::<_, String>(0)) {
            Ok(key) => out.push((*id, key)),
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(err) => return Err(db_error("chiave di un brano", &err)),
        }
    }
    Ok(out)
}

/// Registra la copertina di una riga, se ce n'è una.
///
/// `INSERT OR IGNORE`: la stessa impronta arriva da dodici brani dello stesso
/// album, e la seconda volta non c'è niente da registrare. Va fatto **prima**
/// della riga di `tracks` che la riferisce, altrimenti la chiave esterna cade.
fn insert_cover(tx: &Transaction<'_>, row: &TrackRow, now: i64) -> Result<(), AppError> {
    let Some(cover) = row.cover.as_ref() else {
        return Ok(());
    };
    let mut statement = tx
        .prepare_cached(
            "INSERT OR IGNORE INTO cover_art
               (hash, mime_type, width, height, byte_size, source, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'tag', ?6)",
        )
        .map_err(|err| db_error("registrazione di una copertina", &err))?;
    statement
        .execute(rusqlite::params![
            cover.hash,
            cover.mime_type,
            cover.width,
            cover.height,
            i64::try_from(cover.byte_size).unwrap_or(0),
            now,
        ])
        .map(|_| ())
        .map_err(|err| db_error("registrazione di una copertina", &err))
}

/// I ventotto campi che un brano prende dal file, nell'ordine dei segnaposto,
/// più quel che l'istruzione aggiunge in coda (`?29`).
///
/// Un elenco solo per inserimento e aggiornamento. Scritto due volte, prima o
/// poi le due copie divergono su una colonna, e il risultato è un campo che si
/// popola inserendo e resta al valore vecchio aggiornando — cioè un brano
/// ritaggato che in libreria mostra il tag di prima.
macro_rules! esegui_coi_campi {
    ($statement:expr, $row:expr, $coda:expr) => {{
        let cover_art_hash = $row.cover.as_ref().map(|c| c.hash.as_str());
        $statement.execute(rusqlite::params![
            $row.path,
            $row.track_key,
            $row.title,
            $row.artist,
            $row.album,
            $row.album_artist,
            $row.album_key,
            $row.year,
            $row.track_number,
            $row.disc_number,
            $row.genre,
            $row.duration_ms,
            $row.bpm,
            $row.musical_key,
            $row.comment,
            $row.lyrics,
            cover_art_hash,
            $row.bitrate,
            $row.sample_rate,
            $row.channels,
            $row.codec,
            $row.file_size,
            $row.date_modified,
            $row.replaygain_track_db,
            $row.replaygain_album_db,
            $row.mb_recording_id,
            $row.mb_release_group_id,
            $row.mb_release_id,
            $coda,
        ])
    }};
}

/// Inserisce un brano nuovo.
fn insert_track(tx: &Transaction<'_>, row: &TrackRow, now: i64) -> Result<(), AppError> {
    insert_cover(tx, row, now)?;
    let mut statement = tx
        .prepare_cached(
            "INSERT INTO tracks (
               path, track_key, title, artist, album, album_artist, album_key,
               year, track_number, disc_number, genre, duration_ms, bpm,
               musical_key, comment, lyrics, cover_art_hash, bitrate,
               sample_rate, channels, codec, file_size, date_modified,
               replaygain_track_db, replaygain_album_db, mb_recording_id,
               mb_release_group_id, mb_release_id, date_added, source
             ) VALUES (
               ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
               ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26,
               ?27, ?28, ?29, 'scan'
             )",
        )
        .map_err(|err| db_error("inserimento di un brano", &err))?;
    esegui_coi_campi!(statement, row, now)
        .map(|_| ())
        .map_err(|err| db_error("inserimento di un brano", &err))
}

/// Riscrive una riga esistente con quel che il file dice adesso.
///
/// **Non tocca** `play_count`, `last_played_at`, `rating`, `liked`, `liked_at`,
/// `stats_updated_at` né `date_added`: sono ciò che l'utente ha costruito
/// ascoltando, e non stanno nei file. Un `INSERT OR REPLACE` — che sarebbe la
/// scrittura più corta — le azzererebbe tutte, e in più cambierebbe
/// l'identificativo della riga, togliendo il brano dalle playlist in cui sta.
///
/// Il percorso si riscrive sempre, anche quando l'aggiornamento nasce solo da
/// una data di modifica cambiata: se sul disco il file ora si chiama `A.MP3` e
/// nel database sta come `a.mp3`, questa è l'occasione in cui la deriva si
/// chiude invece di accumularsi.
fn update_track(
    tx: &Transaction<'_>,
    track_id: i64,
    row: &TrackRow,
    now: i64,
) -> Result<(), AppError> {
    insert_cover(tx, row, now)?;
    let mut statement = tx
        .prepare_cached(
            "UPDATE tracks SET
               path = ?1, track_key = ?2, title = ?3, artist = ?4, album = ?5,
               album_artist = ?6, album_key = ?7, year = ?8, track_number = ?9,
               disc_number = ?10, genre = ?11, duration_ms = ?12, bpm = ?13,
               musical_key = ?14, comment = ?15, lyrics = ?16,
               cover_art_hash = ?17, bitrate = ?18, sample_rate = ?19,
               channels = ?20, codec = ?21, file_size = ?22, date_modified = ?23,
               replaygain_track_db = ?24, replaygain_album_db = ?25,
               mb_recording_id = ?26, mb_release_group_id = ?27,
               mb_release_id = ?28
             WHERE id = ?29",
        )
        .map_err(|err| db_error("aggiornamento di un brano", &err))?;
    esegui_coi_campi!(statement, row, track_id)
        .map(|_| ())
        .map_err(|err| db_error("aggiornamento di un brano", &err))
}

/// Toglie le righe indicate.
///
/// # Niente lapidi di sincronizzazione, qui
///
/// `sync_tombstones` serve a far viaggiare una **cancellazione decisa
/// dall'utente**: senza, l'altro dispositivo vedrebbe solo «a me manca un brano»
/// e lo rimanderebbe indietro. Una rimozione da scansione non è quella cosa: è
/// «questo file non è su questo disco», che è un fatto locale.
///
/// Scriverne una qui vorrebbe dire che staccare un disco esterno e riscansionare
/// propaga al telefono la cancellazione di trecento brani che nessuno ha chiesto
/// di cancellare — e le lapidi, per come funzionano, impedirebbero pure di
/// riaverli riattaccando il disco.
fn remove_tracks(tx: &Transaction<'_>, ids: &[i64]) -> Result<usize, AppError> {
    let mut statement = tx
        .prepare_cached("DELETE FROM tracks WHERE id = ?1")
        .map_err(|err| db_error("rimozione di un brano", &err))?;
    let mut removed = 0;
    for id in ids {
        removed += statement
            .execute([id])
            .map_err(|err| db_error("rimozione di un brano", &err))?;
    }
    Ok(removed)
}

/// Quanti album e quanti artisti sono stati ricostruiti.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Aggregates {
    /// Righe in `albums`.
    pub albums: usize,
    /// Righe in `artists`.
    pub artists: usize,
}

fn member_from_row(row: &Row<'_>) -> rusqlite::Result<AlbumMember> {
    Ok(AlbumMember {
        album_key: row.get(0)?,
        album: row.get(1)?,
        album_artist: row.get(2)?,
        artist: row.get(3)?,
        year: row.get(4)?,
        genre: row.get(5)?,
        cover_art_hash: row.get(6)?,
        cover_source: row.get(7)?,
        cover_width: row.get(8)?,
        cover_height: row.get(9)?,
        mb_release_group_id: row.get(10)?,
        mb_release_id: row.get(11)?,
        spotify_album_id: row.get(12)?,
    })
}

/// L'artista sotto cui elencare un brano, in SQL.
///
/// L'artista dell'album quando c'è, altrimenti quello del brano: lo stesso
/// ripiego del dominio. Spezzare un «A feat. B» in due artisti è un mestiere
/// diverso — richiede sapere quali «feat.», «&» e virgole separano davvero due
/// nomi e quali fanno parte di uno solo — e appartiene all'arricchimento, non a
/// una scansione del disco.
const ARTISTA_EFFETTIVO: &str = "TRIM(COALESCE(NULLIF(TRIM(album_artist), ''), artist))";

/// Ricostruisce `albums` e `artists` dai brani.
///
/// # Perché `albums` si butta e `artists` no
///
/// In `albums` non c'è niente che non derivi dai brani: titolo, artista, anno,
/// genere, conteggio, copertina e identificativi li calcola tutti
/// [`build_album_groups`]. Cancellare e riscrivere è quindi la ricostruzione
/// **esatta**, e costa meno di una riconciliazione riga per riga.
///
/// In `artists` invece ci sono `bio`, `image_hash` e gli identificativi esterni:
/// arrivano dall'arricchimento in rete, non dai file, e una scansione che li
/// buttasse costringerebbe a riscaricarli tutti a ogni passata. Lì si aggiunge
/// chi manca e si toglie chi non è più nominato da nessun brano.
///
/// # Il rimappaggio
///
/// I brani nuovi arrivano con la chiave di base (titolo più cartella). Quando
/// due gruppi si fondono per un identificativo autorevole la chiave canonica è
/// un'altra, e va riscritta anche su `tracks`: altrimenti la giunzione
/// brani⋈album lascerebbe fuori la metà del disco che ha conservato la chiave
/// vecchia.
pub fn rebuild_aggregates(tx: &Transaction<'_>) -> Result<Aggregates, AppError> {
    let members = {
        let mut statement = tx
            .prepare(
                "SELECT t.album_key, t.album, t.album_artist, t.artist, t.year, t.genre,
                        t.cover_art_hash, c.source, c.width, c.height,
                        t.mb_release_group_id, t.mb_release_id, t.spotify_album_id
                 FROM tracks t
                 LEFT JOIN cover_art c ON c.hash = t.cover_art_hash
                 WHERE t.album_key IS NOT NULL AND t.album_key <> ''
                 ORDER BY t.id",
            )
            .map_err(|err| db_error("lettura dei brani per gli album", &err))?;
        let rows = statement
            .query_map([], member_from_row)
            .map_err(|err| db_error("lettura dei brani per gli album", &err))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|err| db_error("lettura dei brani per gli album", &err))?
    };

    let groups = build_album_groups(&members);

    {
        let mut statement = tx
            .prepare_cached("UPDATE tracks SET album_key = ?2 WHERE album_key = ?1")
            .map_err(|err| db_error("rimappaggio delle chiavi d'album", &err))?;
        for (from, to) in &groups.remap {
            if from == to {
                continue;
            }
            statement
                .execute(rusqlite::params![from, to])
                .map_err(|err| db_error("rimappaggio delle chiavi d'album", &err))?;
        }
    }

    tx.execute("DELETE FROM albums", [])
        .map_err(|err| db_error("svuotamento degli album", &err))?;
    {
        let mut statement = tx
            .prepare_cached(
                "INSERT INTO albums (album_key, title, artist, year, genre, total_tracks,
                                     cover_art_hash, mb_album_id, spotify_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )
            .map_err(|err| db_error("scrittura di un album", &err))?;
        for album in &groups.albums {
            let AlbumRow {
                album_key,
                title,
                artist,
                year,
                genre,
                total_tracks,
                cover_art_hash,
                mb_album_id,
                spotify_id,
            } = album;
            statement
                .execute(rusqlite::params![
                    album_key,
                    title,
                    artist,
                    year,
                    genre,
                    i64::try_from(*total_tracks).unwrap_or(i64::MAX),
                    cover_art_hash,
                    mb_album_id,
                    spotify_id,
                ])
                .map_err(|err| db_error("scrittura di un album", &err))?;
        }
    }

    tx.execute_batch(&format!(
        "INSERT OR IGNORE INTO artists (name)
           SELECT DISTINCT {ARTISTA_EFFETTIVO} FROM tracks
           WHERE {ARTISTA_EFFETTIVO} <> '';
         DELETE FROM artists
           WHERE name NOT IN (SELECT DISTINCT {ARTISTA_EFFETTIVO} FROM tracks);"
    ))
    .map_err(|err| db_error("ricostruzione degli artisti", &err))?;

    let artists: i64 = tx
        .query_row("SELECT COUNT(*) FROM artists", [], |row| row.get(0))
        .map_err(|err| db_error("conteggio degli artisti", &err))?;

    Ok(Aggregates {
        albums: groups.albums.len(),
        artists: usize::try_from(artists).unwrap_or(0),
    })
}

/// Un file che non si è riusciti a leggere.
#[derive(Debug)]
pub struct Unreadable {
    /// Quale.
    pub path: String,
    /// Perché.
    pub error: AppError,
}

/// Cosa ha fatto una scansione.
#[derive(Debug, Default)]
pub struct ScanReport {
    /// Il piano che è stato deciso, per poterlo mostrare accanto all'esito.
    pub plan: ScanPlan,
    /// Righe inserite.
    pub inserted: usize,
    /// Righe riscritte perché il file era cambiato.
    pub updated: usize,
    /// Righe conservate perché il file si era solo spostato.
    pub moved: usize,
    /// Righe tolte.
    pub removed: usize,
    /// File saltati perché illeggibili.
    pub unreadable: Vec<Unreadable>,
    /// Copertine ricodificate ora.
    pub covers_stored: usize,
    /// Copertine già nello store, non ricodificate.
    pub covers_reused: usize,
    /// Gli aggregati ricostruiti.
    pub aggregates: Aggregates,
    /// Quanto è durata.
    pub elapsed_ms: u128,
}

/// Una scansione da eseguire.
#[derive(Clone, Copy)]
pub struct Scan<'a> {
    /// Da dove arrivano i file: disco locale o Storage Access Framework.
    pub files: &'a dyn MusicFiles,
    /// Dove finiscono le copertine.
    pub covers: &'a CoverStore,
    /// Le cartelle da guardare.
    pub roots: &'a [String],
    /// Come si confrontano i percorsi su questo filesystem.
    pub rules: PathRules,
}

impl std::fmt::Debug for Scan<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scan")
            .field("roots", &self.roots)
            .field("rules", &self.rules)
            .finish_non_exhaustive()
    }
}

/// Dove va a finire una riga letta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Destinazione {
    /// Un brano che in libreria non c'era.
    Nuova,
    /// Una riga esistente, il cui file è cambiato sul posto.
    Aggiorna(i64),
    /// Una riga esistente il cui file si è spostato: si riscrive il percorso.
    Sposta(i64),
}

/// Calcola il piano senza toccare niente.
///
/// È la chiamata che precede [`Scan::run`] quando si vuole mostrare all'utente
/// cosa sta per succedere — «sto per togliere 340 brani» — e lasciargli dire di
/// no prima che succeda.
pub fn plan(scan: &Scan<'_>, connection: &Connection) -> Result<ScanPlan, AppError> {
    let mut found = Vec::new();
    for root in scan.roots {
        found.extend(scan.files.walk(root)?);
    }
    let known = known_tracks(connection)?;
    Ok(plan_scan(
        ScanInput {
            roots: scan.roots,
            found: &found,
            known: &known,
        },
        scan.rules,
    ))
}

impl Scan<'_> {
    /// Esegue la scansione: cammina, decide, legge, scrive, ricostruisce.
    ///
    /// `on_progress` riceve `(fatti, totale)` sui soli file da leggere, che sono
    /// la parte lenta. Rimozioni e aggregati non entrano nel conteggio perché
    /// non hanno una durata percepibile.
    pub fn run(
        &self,
        connection: &mut Connection,
        mut on_progress: impl FnMut(usize, usize),
    ) -> Result<ScanReport, AppError> {
        let started = std::time::Instant::now();
        let mut report = ScanReport {
            plan: plan(self, connection)?,
            ..ScanReport::default()
        };

        // ── le righe doppie se ne vanno subito ──
        // Due righe che nominano lo stesso file: quella che sopravvive sta per
        // ricevere il percorso come la camminata l'ha scritto, e `tracks.path` è
        // UNIQUE — se la doppia fosse ancora lì, l'aggiornamento fallirebbe.
        // Non sono candidate a essere spostamenti: il loro file non si è mosso,
        // è la riga a essere di troppo.
        let doppie: Vec<i64> = report
            .plan
            .to_remove
            .iter()
            .filter(|r| r.reason == RemoveReason::DuplicateRow)
            .map(|r| r.track.id)
            .collect();
        if !doppie.is_empty() {
            let tx = connection
                .transaction()
                .map_err(|err| db_error("apertura della transazione", &err))?;
            report.removed += remove_tracks(&tx, &doppie)?;
            tx.commit()
                .map_err(|err| db_error("chiusura della transazione", &err))?;
        }

        // ── le sparizioni restano in sospeso: qualcuna è uno spostamento ──
        let spariti: Vec<i64> = report
            .plan
            .to_remove
            .iter()
            .filter(|r| r.reason == RemoveReason::Disappeared)
            .map(|r| r.track.id)
            .collect();
        let mut sospese = track_keys_of(connection, &spariti)?;

        // ── quel che va letto, in un elenco solo ──
        // Inserimenti e aggiornamenti differiscono solo per la presenza di una
        // riga da riusare; leggerli insieme tiene i lotti pieni anche quando gli
        // uni sono pochi e gli altri tanti.
        let mut da_leggere: Vec<(DiscoveredFile, Destinazione)> =
            Vec::with_capacity(report.plan.to_insert.len() + report.plan.to_update.len());
        for file in &report.plan.to_insert {
            da_leggere.push((file.clone(), Destinazione::Nuova));
        }
        for pending in &report.plan.to_update {
            da_leggere.push((
                pending.file.clone(),
                Destinazione::Aggiorna(pending.track_id),
            ));
        }

        let totale = da_leggere.len();
        let mut fatti = 0;
        for lotto in da_leggere.chunks(LOTTO) {
            let mut righe: Vec<(TrackRow, Destinazione)> = Vec::with_capacity(lotto.len());
            for (file, destinazione) in lotto {
                match read_track(self.files, self.covers, file) {
                    Ok(row) => {
                        if let Some(cover) = row.cover.as_ref() {
                            if cover.already_present {
                                report.covers_reused += 1;
                            } else {
                                report.covers_stored += 1;
                            }
                        }
                        righe.push((row, *destinazione));
                    }
                    Err(error) => report.unreadable.push(Unreadable {
                        path: file.path.clone(),
                        error,
                    }),
                }
                fatti += 1;
                on_progress(fatti, totale);
            }

            // ── quali di questi file nuovi sono righe che si sono spostate ──
            if !sospese.is_empty() {
                let posizioni: Vec<usize> = righe
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, d))| *d == Destinazione::Nuova)
                    .map(|(index, _)| index)
                    .collect();
                let chiavi: Vec<&str> = posizioni
                    .iter()
                    .filter_map(|index| righe.get(*index))
                    .map(|(row, _)| row.track_key.as_str())
                    .collect();
                let identita: Vec<RemovedIdentity<'_>> = sospese
                    .iter()
                    .map(|(track_id, track_key)| RemovedIdentity {
                        track_id: *track_id,
                        track_key: track_key.as_str(),
                    })
                    .collect();

                let mut appaiate: Vec<i64> = Vec::new();
                for rematch in match_moved_tracks(&identita, &chiavi) {
                    let Some(&posizione) = posizioni.get(rematch.insert_index) else {
                        continue;
                    };
                    let Some(slot) = righe.get_mut(posizione) else {
                        continue;
                    };
                    slot.1 = Destinazione::Sposta(rematch.track_id);
                    appaiate.push(rematch.track_id);
                }
                sospese.retain(|(track_id, _)| !appaiate.contains(track_id));
            }

            let now = now_ms();
            let tx = connection
                .transaction()
                .map_err(|err| db_error("apertura della transazione", &err))?;
            for (row, destinazione) in &righe {
                match *destinazione {
                    Destinazione::Nuova => {
                        insert_track(&tx, row, now)?;
                        report.inserted += 1;
                    }
                    Destinazione::Aggiorna(id) => {
                        update_track(&tx, id, row, now)?;
                        report.updated += 1;
                    }
                    Destinazione::Sposta(id) => {
                        update_track(&tx, id, row, now)?;
                        report.moved += 1;
                    }
                }
            }
            tx.commit()
                .map_err(|err| db_error("chiusura della transazione", &err))?;
        }

        // ── le sparizioni che nessun file nuovo ha reclamato ──
        if !sospese.is_empty() {
            let ids: Vec<i64> = sospese.iter().map(|(id, _)| *id).collect();
            let tx = connection
                .transaction()
                .map_err(|err| db_error("apertura della transazione", &err))?;
            report.removed += remove_tracks(&tx, &ids)?;
            tx.commit()
                .map_err(|err| db_error("chiusura della transazione", &err))?;
        }

        // ── gli aggregati, quando i brani sono tutti al loro posto ──
        let tx = connection
            .transaction()
            .map_err(|err| db_error("apertura della transazione", &err))?;
        report.aggregates = rebuild_aggregates(&tx)?;
        tx.commit()
            .map_err(|err| db_error("chiusura della transazione", &err))?;

        report.elapsed_ms = started.elapsed().as_millis();
        Ok(report)
    }
}

/// Un brano come lo mostra una lista.
///
/// Non tutte le colonne di `tracks`: testo, commento e impronte non servono a
/// disegnare una riga, e trascinarseli dietro vorrebbe dire spedire qualche
/// megabyte di testi di canzoni verso la finestra a ogni scorrimento.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackSummary {
    /// L'identificativo della riga.
    pub id: i64,
    /// Il percorso.
    pub path: String,
    /// Il titolo.
    pub title: String,
    /// L'interprete.
    pub artist: String,
    /// L'album.
    pub album: String,
    /// La chiave dell'album, per aprirne la scheda.
    pub album_key: Option<String>,
    /// Numero di traccia.
    pub track_number: Option<i64>,
    /// Numero di disco.
    pub disc_number: Option<i64>,
    /// Durata in millisecondi.
    pub duration_ms: i64,
    /// L'anno.
    pub year: Option<i64>,
    /// L'impronta della copertina.
    pub cover_art_hash: Option<String>,
    /// Quante volte è stato ascoltato.
    pub play_count: i64,
    /// È fra i preferiti.
    pub liked: bool,
    /// Il voto, 0–5.
    pub rating: i64,
}

/// Le colonne di [`TrackSummary`], nell'ordine in cui le legge `track_from_row`.
const COLONNE_BRANO: &str = "t.id, t.path, t.title, t.artist, t.album, t.album_key,
     t.track_number, t.disc_number, t.duration_ms, t.year, t.cover_art_hash,
     t.play_count, t.liked, t.rating";

fn track_from_row(row: &Row<'_>) -> rusqlite::Result<TrackSummary> {
    Ok(TrackSummary {
        id: row.get(0)?,
        path: row.get(1)?,
        title: row.get(2)?,
        artist: row.get(3)?,
        album: row.get(4)?,
        album_key: row.get(5)?,
        track_number: row.get(6)?,
        disc_number: row.get(7)?,
        duration_ms: row.get(8)?,
        year: row.get(9)?,
        cover_art_hash: row.get(10)?,
        play_count: row.get(11)?,
        liked: row.get::<_, i64>(12)? != 0,
        rating: row.get(13)?,
    })
}

/// Un album come lo mostra una griglia.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumSummary {
    /// La chiave canonica.
    pub album_key: String,
    /// Il titolo.
    pub title: String,
    /// L'artista.
    pub artist: String,
    /// L'anno.
    pub year: Option<i64>,
    /// Il genere dominante.
    pub genre: Option<String>,
    /// Quanti brani.
    pub total_tracks: i64,
    /// L'impronta della copertina.
    pub cover_art_hash: Option<String>,
}

/// Quanto c'è in libreria.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    /// Brani.
    pub tracks: i64,
    /// Album.
    pub albums: i64,
    /// Artisti.
    pub artists: i64,
    /// Brani fra i preferiti.
    pub liked: i64,
    /// Durata totale in millisecondi.
    pub duration_ms: i64,
}

/// I numeri della libreria, in una query sola.
pub fn counts(connection: &Connection) -> Result<Counts, AppError> {
    connection
        .query_row(
            "SELECT (SELECT COUNT(*) FROM tracks),
                    (SELECT COUNT(*) FROM albums),
                    (SELECT COUNT(*) FROM artists),
                    (SELECT COUNT(*) FROM tracks WHERE liked = 1),
                    (SELECT COALESCE(SUM(duration_ms), 0) FROM tracks)",
            [],
            |row| {
                Ok(Counts {
                    tracks: row.get(0)?,
                    albums: row.get(1)?,
                    artists: row.get(2)?,
                    liked: row.get(3)?,
                    duration_ms: row.get(4)?,
                })
            },
        )
        .map_err(|err| db_error("numeri della libreria", &err))
}

/// Come ordinare un elenco di brani.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackOrder {
    /// Artista, poi album, poi disco e traccia: l'ordine di uno scaffale.
    Shelf,
    /// I più recenti in cima.
    RecentlyAdded,
    /// I più ascoltati in cima.
    MostPlayed,
    /// Titolo alfabetico.
    Title,
}

impl TrackOrder {
    /// La clausola `ORDER BY`. Chiusa in un `match`, mai composta da stringhe
    /// che arrivano da fuori: un ordinamento scelto dall'interfaccia non deve
    /// poter diventare un'iniezione.
    const fn sql(self) -> &'static str {
        match self {
            Self::Shelf => {
                "t.artist COLLATE NOCASE, t.album COLLATE NOCASE,
                 t.disc_number, t.track_number, t.title COLLATE NOCASE"
            }
            Self::RecentlyAdded => "t.date_added DESC, t.id DESC",
            Self::MostPlayed => "t.play_count DESC, t.last_played_at DESC, t.title COLLATE NOCASE",
            Self::Title => "t.title COLLATE NOCASE, t.artist COLLATE NOCASE",
        }
    }
}

/// Una pagina di brani.
pub fn list_tracks(
    connection: &Connection,
    order: TrackOrder,
    offset: i64,
    limit: i64,
) -> Result<Vec<TrackSummary>, AppError> {
    let sql = format!(
        "SELECT {COLONNE_BRANO} FROM tracks t ORDER BY {} LIMIT ?1 OFFSET ?2",
        order.sql()
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("elenco dei brani", &err))?;
    let rows = statement
        .query_map(rusqlite::params![limit, offset], track_from_row)
        .map_err(|err| db_error("elenco dei brani", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("elenco dei brani", &err))
}

/// I brani di un album, nell'ordine del disco.
pub fn album_tracks(
    connection: &Connection,
    album_key: &str,
) -> Result<Vec<TrackSummary>, AppError> {
    let sql = format!(
        "SELECT {COLONNE_BRANO} FROM tracks t WHERE t.album_key = ?1
         ORDER BY t.disc_number, t.track_number, t.title COLLATE NOCASE"
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("brani di un album", &err))?;
    let rows = statement
        .query_map([album_key], track_from_row)
        .map_err(|err| db_error("brani di un album", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("brani di un album", &err))
}

/// Una pagina di album, per artista e anno.
pub fn list_albums(
    connection: &Connection,
    offset: i64,
    limit: i64,
) -> Result<Vec<AlbumSummary>, AppError> {
    let mut statement = connection
        .prepare_cached(
            "SELECT album_key, title, artist, year, genre, total_tracks, cover_art_hash
             FROM albums
             ORDER BY artist COLLATE NOCASE, year, title COLLATE NOCASE
             LIMIT ?1 OFFSET ?2",
        )
        .map_err(|err| db_error("elenco degli album", &err))?;
    let rows = statement
        .query_map(rusqlite::params![limit, offset], |row| {
            Ok(AlbumSummary {
                album_key: row.get(0)?,
                title: row.get(1)?,
                artist: row.get(2)?,
                year: row.get(3)?,
                genre: row.get(4)?,
                total_tracks: row.get(5)?,
                cover_art_hash: row.get(6)?,
            })
        })
        .map_err(|err| db_error("elenco degli album", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("elenco degli album", &err))
}

/// Traduce quel che l'utente ha scritto in un'espressione FTS5.
///
/// Ogni parola diventa una stringa fra virgolette con un asterisco in coda.
/// Virgolettata perché altrimenti `AND`, `OR`, `NOT`, `NEAR`, l'asterisco, il
/// trattino e le parentesi verrebbero letti come operatori: chi cerca `AC/DC` o
/// `Where Are You` otterrebbe un errore di sintassi invece dei suoi dischi.
/// L'asterisco perché in una libreria musicale si cerca mentre si digita, e
/// `bjor` deve già trovare Björk.
///
/// Le virgolette dentro una parola si raddoppiano, che è come FTS5 le protegge.
#[must_use]
pub fn fts_query(input: &str) -> String {
    input
        .split_whitespace()
        .map(|token| format!("\"{}\"*", token.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Cerca nella libreria.
///
/// L'ordine è quello di `bm25`, la rilevanza di FTS5: una parola rara pesa più
/// di una comune, e un riscontro in un campo corto pesa più dello stesso
/// riscontro perso dentro uno lungo.
pub fn search(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<TrackSummary>, AppError> {
    let expression = fts_query(query);
    if expression.is_empty() {
        return Ok(Vec::new());
    }
    // Il nome della tabella e non un alias: in FTS5 il lato sinistro di `MATCH`
    // e l'argomento di `bm25` devono nominare la tabella virtuale per esteso, e
    // con un alias SQLite risponde che quella colonna non esiste.
    let sql = format!(
        "SELECT {COLONNE_BRANO}
         FROM tracks_fts
         JOIN tracks t ON t.id = tracks_fts.rowid
         WHERE tracks_fts MATCH ?1
         ORDER BY bm25(tracks_fts) LIMIT ?2"
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("ricerca", &err))?;
    let rows = statement
        .query_map(
            rusqlite::params![expression, i64::try_from(limit).unwrap_or(i64::MAX)],
            track_from_row,
        )
        .map_err(|err| db_error("ricerca", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("ricerca", &err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::LocalFiles;
    use aether_domain::paths::MIN_TRACK_BYTES;
    use lofty::config::WriteOptions;
    use lofty::prelude::{Accessor, TagExt};
    use lofty::tag::{Tag, TagType};
    use std::path::{Path, PathBuf};

    /// Una libreria finta su disco, con dentro file audio veri.
    struct Libreria {
        dir: tempfile::TempDir,
        store: CoverStore,
        connection: Connection,
    }

    /// Un WAV valido di silenzio, sopra la soglia dei [`MIN_TRACK_BYTES`].
    ///
    /// File audio veri e non byte finti: un doppio del lettore di tag proverebbe
    /// che questo modulo sa parlare col doppio. Quel che deve reggere è `lofty`
    /// su un file che esiste davvero, comprese le sue idee su dove stia un tag.
    fn wav(path: &Path) {
        let campioni = usize::try_from(MIN_TRACK_BYTES).unwrap_or(32_768) + 1_024;
        let dati = campioni.next_multiple_of(2);
        let mut out = Vec::with_capacity(dati + 44);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&u32::try_from(dati + 36).unwrap_or(0).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes()); // lunghezza del blocco fmt
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&44_100u32.to_le_bytes());
        out.extend_from_slice(&88_200u32.to_le_bytes()); // byte al secondo
        out.extend_from_slice(&2u16.to_le_bytes()); // allineamento
        out.extend_from_slice(&16u16.to_le_bytes()); // bit per campione
        out.extend_from_slice(b"data");
        out.extend_from_slice(&u32::try_from(dati).unwrap_or(0).to_le_bytes());
        out.resize(out.len() + dati, 0);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("cartelle");
        }
        std::fs::write(path, &out).expect("scrittura del wav");
    }

    /// Scrive i tag dentro un file già esistente.
    fn tagga(path: &Path, titolo: &str, artista: &str, album: &str) {
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title(titolo.to_owned());
        tag.set_artist(artista.to_owned());
        tag.set_album(album.to_owned());
        tag.save_to_path(path, WriteOptions::default())
            .expect("scrittura dei tag");
    }

    impl Libreria {
        fn nuova() -> Self {
            let dir = tempfile::tempdir().expect("cartella temporanea");
            let store = CoverStore::open(dir.path().join("copertine")).expect("store");
            let connection = crate::db::open_in_memory().expect("database").connection;
            Self {
                dir,
                store,
                connection,
            }
        }

        /// La cartella della musica: sotto la radice, così lo store delle
        /// copertine non finisce dentro quel che si scansiona.
        fn musica(&self) -> PathBuf {
            self.dir.path().join("musica")
        }

        fn root(&self) -> String {
            self.musica().to_string_lossy().into_owned()
        }

        /// Aggiunge un brano taggato in `relativo`, che è un percorso dentro la
        /// cartella della musica.
        fn brano(&self, relativo: &str, titolo: &str, artista: &str, album: &str) -> PathBuf {
            let path = self.musica().join(relativo);
            wav(&path);
            tagga(&path, titolo, artista, album);
            path
        }

        fn scansiona(&mut self) -> ScanReport {
            let roots = vec![self.root()];
            let scan = Scan {
                files: &LocalFiles,
                covers: &self.store,
                roots: &roots,
                rules: PathRules::for_current_platform(),
            };
            scan.run(&mut self.connection, |_, _| {})
                .expect("scansione")
        }

        fn conta(&self, tabella: &str) -> i64 {
            self.connection
                .query_row(&format!("SELECT COUNT(*) FROM {tabella}"), [], |r| r.get(0))
                .expect("conteggio")
        }
    }

    #[test]
    fn una_scansione_riempie_la_libreria_e_la_rende_cercabile() {
        let mut lib = Libreria::nuova();
        lib.brano("Björk/Homogenic/01.wav", "Jóga", "Björk", "Homogenic");
        lib.brano(
            "Björk/Homogenic/02.wav",
            "Bachelorette",
            "Björk",
            "Homogenic",
        );
        lib.brano("Altro/Raccolta/01.wav", "Pezzo", "Tale", "Raccolta");

        let esito = lib.scansiona();

        assert_eq!(esito.inserted, 3);
        assert_eq!(esito.updated, 0);
        assert_eq!(esito.removed, 0);
        assert!(esito.unreadable.is_empty(), "{:?}", esito.unreadable);
        assert_eq!(lib.conta("tracks"), 3);
        // Due cartelle diverse, due album; gli artisti seguono.
        assert_eq!(esito.aggregates.albums, 2);
        assert_eq!(esito.aggregates.artists, 2);
        assert_eq!(lib.conta("albums"), 2);

        // Interrogabile davvero: la piegatura del tokenizzatore attraversa tutto
        // il giro, dal tag sul disco alla riga trovata.
        let trovati = search(&lib.connection, "bjork", 10).expect("ricerca");
        assert_eq!(trovati.len(), 2);
        let per_titolo = search(&lib.connection, "bachelor", 10).expect("ricerca");
        assert_eq!(
            per_titolo.first().map(|t| t.title.as_str()),
            Some("Bachelorette"),
            "una ricerca si fa mentre si digita"
        );
    }

    #[test]
    fn riscansionare_non_rilegge_niente() {
        // È il numero che rende una riscansione sopportabile: se questo cade,
        // aprire l'app rilegge centomila file.
        let mut lib = Libreria::nuova();
        lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
        lib.brano("A/Al/02.wav", "Due", "Art", "Al");
        lib.scansiona();

        let secondo = lib.scansiona();
        assert_eq!(secondo.inserted, 0);
        assert_eq!(secondo.updated, 0);
        assert_eq!(secondo.removed, 0);
        assert_eq!(secondo.plan.unchanged, 2);
        assert_eq!(lib.conta("tracks"), 2);
    }

    #[test]
    fn un_riordino_non_azzera_ascolti_ne_playlist() {
        // Il caso vero, e il motivo per cui gli spostamenti si riconoscono: dopo
        // un riordino OGNI file ha un percorso nuovo. Senza appaiamento la
        // libreria si svuota e si riempie di righe nuove, e con le righe vecchie
        // se ne vanno conteggi, preferiti e appartenenza alle playlist.
        let mut lib = Libreria::nuova();
        lib.brano("sfusi/traccia.wav", "Uno", "Art", "Al");
        lib.scansiona();

        let id: i64 = lib
            .connection
            .query_row("SELECT id FROM tracks", [], |r| r.get(0))
            .expect("id");
        lib.connection
            .execute_batch(&format!(
                "UPDATE tracks SET play_count = 42, rating = 5, liked = 1 WHERE id = {id};
                 INSERT INTO playlists (id, playlist_key, name, created_at, updated_at)
                 VALUES (1, 'p', 'P', 1, 1);
                 INSERT INTO playlist_tracks (playlist_id, track_id, position)
                 VALUES (1, {id}, 0);"
            ))
            .expect("storia d'ascolto");

        // Il riordino: stesso file, cartella nuova.
        let da = lib.musica().join("sfusi/traccia.wav");
        let a = lib.musica().join("Art/Al/01 Uno.wav");
        std::fs::create_dir_all(a.parent().expect("cartella")).expect("cartelle");
        std::fs::rename(&da, &a).expect("spostamento");

        let esito = lib.scansiona();

        assert_eq!(esito.moved, 1, "il file si è spostato, non è nuovo");
        assert_eq!(esito.inserted, 0);
        assert_eq!(esito.removed, 0);
        assert_eq!(lib.conta("tracks"), 1);

        let (stesso_id, ascolti, voto, percorso): (i64, i64, i64, String) = lib
            .connection
            .query_row("SELECT id, play_count, rating, path FROM tracks", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .expect("riga");
        assert_eq!(stesso_id, id, "deve essere la stessa riga, non una nuova");
        assert_eq!(ascolti, 42);
        assert_eq!(voto, 5);
        assert!(percorso.ends_with("01 Uno.wav"), "percorso: {percorso}");
        assert_eq!(lib.conta("playlist_tracks"), 1, "la playlist regge");
    }

    #[test]
    fn un_brano_ritaggato_conserva_le_statistiche() {
        let mut lib = Libreria::nuova();
        let path = lib.brano("A/Al/01.wav", "Titolo vecchio", "Art", "Al");
        lib.scansiona();
        lib.connection
            .execute("UPDATE tracks SET play_count = 7", [])
            .expect("ascolti");

        // Ritaggato sul posto: cambia il contenuto, non il percorso.
        tagga(&path, "Titolo nuovo", "Art", "Al");
        // La data di modifica deve risultare diversa: su un filesystem che la
        // riporta al millisecondo due scritture di fila possono cadere nello
        // stesso istante, e il piano direbbe «invariato».
        filetime::set_file_mtime(&path, filetime::FileTime::from_unix_time(1_700_000_000, 0))
            .expect("data di modifica");

        let esito = lib.scansiona();
        assert_eq!(esito.updated, 1);
        assert_eq!(esito.inserted, 0);

        let (titolo, ascolti): (String, i64) = lib
            .connection
            .query_row("SELECT title, play_count FROM tracks", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .expect("riga");
        assert_eq!(titolo, "Titolo nuovo");
        assert_eq!(ascolti, 7, "riscrivere i tag non azzera l'ascolto");
    }

    #[test]
    fn un_file_sparito_se_ne_va_e_si_porta_dietro_l_album() {
        let mut lib = Libreria::nuova();
        lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
        let secondo = lib.brano("B/Bl/01.wav", "Due", "Art2", "Bl");
        lib.scansiona();
        assert_eq!(lib.conta("albums"), 2);

        std::fs::remove_file(&secondo).expect("cancellazione");
        let esito = lib.scansiona();

        assert_eq!(esito.removed, 1);
        assert_eq!(esito.moved, 0, "non c'è nessun file nuovo a reclamarlo");
        assert_eq!(lib.conta("tracks"), 1);
        // Gli aggregati seguono: un album senza brani non deve restare in giro.
        assert_eq!(lib.conta("albums"), 1);
        assert_eq!(lib.conta("artists"), 1);
    }

    #[test]
    fn una_scansione_non_scrive_lapidi() {
        // Staccare un disco esterno non è «cancella questi brani»: una lapide
        // propagherebbe al telefono una cancellazione che nessuno ha chiesto.
        let mut lib = Libreria::nuova();
        let path = lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
        lib.scansiona();
        std::fs::remove_file(&path).expect("cancellazione");
        lib.scansiona();
        assert_eq!(lib.conta("tracks"), 0);
        assert_eq!(lib.conta("sync_tombstones"), 0);
    }

    #[test]
    fn l_arricchimento_degli_artisti_sopravvive_a_una_riscansione() {
        // `albums` si ricostruisce da zero perché non contiene niente d'altro;
        // `artists` no, perché la biografia non sta nei file.
        let mut lib = Libreria::nuova();
        lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
        lib.scansiona();
        lib.connection
            .execute("UPDATE artists SET bio = 'una biografia'", [])
            .expect("arricchimento");

        lib.brano("A/Al/02.wav", "Due", "Art", "Al");
        lib.scansiona();

        let bio: Option<String> = lib
            .connection
            .query_row("SELECT bio FROM artists WHERE name = 'Art'", [], |r| {
                r.get(0)
            })
            .expect("artista");
        assert_eq!(bio.as_deref(), Some("una biografia"));
    }

    #[test]
    fn quel_che_non_e_audio_non_ferma_la_scansione() {
        let mut lib = Libreria::nuova();
        lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
        // Un file con l'estensione giusta e dentro spazzatura: passa il filtro
        // del piano (estensione e dimensione) e cade sulla lettura dei tag.
        let finto = lib.musica().join("A/Al/rotto.mp3");
        std::fs::write(&finto, vec![0u8; 40_000]).expect("scrittura");

        let esito = lib.scansiona();
        assert_eq!(esito.inserted, 1, "il brano buono entra lo stesso");
        assert_eq!(esito.unreadable.len(), 1);
        assert!(
            esito
                .unreadable
                .first()
                .is_some_and(|u| u.path.ends_with("rotto.mp3"))
        );
        assert_eq!(lib.conta("tracks"), 1);
    }

    #[test]
    fn un_file_senza_tag_prende_il_nome_dal_disco() {
        let mut lib = Libreria::nuova();
        wav(&lib.musica().join("A/Al/07 Senza tag.wav"));
        let esito = lib.scansiona();

        assert_eq!(esito.inserted, 1);
        let (titolo, chiave): (String, String) = lib
            .connection
            .query_row("SELECT title, track_key FROM tracks", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .expect("riga");
        assert_eq!(titolo, "07 Senza tag");
        // La chiave si deriva dai ripieghi, non dai tag vuoti: fosse `||`, ogni
        // file senza tag sarebbe lo stesso brano per la sincronizzazione.
        assert_ne!(chiave, "||");
        assert!(chiave.contains("07 senza tag"), "chiave: {chiave}");
    }

    #[test]
    fn i_dischi_multipli_restano_una_pubblicazione_sola() {
        let mut lib = Libreria::nuova();
        lib.brano("Art/Opera/CD1/01.wav", "Uno", "Art", "Opera");
        lib.brano("Art/Opera/CD2/01.wav", "Due", "Art", "Opera");
        let esito = lib.scansiona();

        assert_eq!(esito.aggregates.albums, 1);
        let (titolo, brani): (String, i64) = lib
            .connection
            .query_row("SELECT title, total_tracks FROM albums", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .expect("album");
        assert_eq!(titolo, "Opera");
        assert_eq!(brani, 2);
    }

    #[test]
    fn la_ricerca_non_si_rompe_sui_caratteri_da_operatore() {
        // `AC/DC`, un trattino, una virgoletta: senza virgolettatura FTS5 li
        // legge come sintassi e la ricerca fallisce invece di non trovare nulla.
        let mut lib = Libreria::nuova();
        lib.brano("AC-DC/Back/01.wav", "T.N.T.", "AC/DC", "Back in Black");
        lib.scansiona();

        for query in ["AC/DC", "back - black", "\"virgolette", "NEAR", "OR", "*"] {
            let esito = search(&lib.connection, query, 10);
            assert!(
                esito.is_ok(),
                "{query} ha fatto fallire la ricerca: {esito:?}"
            );
        }
        assert_eq!(
            search(&lib.connection, "AC/DC", 10).expect("ricerca").len(),
            1
        );
        assert!(
            search(&lib.connection, "", 10).expect("ricerca").is_empty(),
            "una ricerca vuota non è un errore, è nessun risultato"
        );
    }

    #[test]
    fn una_copertina_condivisa_si_salva_una_volta_sola() {
        let mut lib = Libreria::nuova();
        let copertina = {
            let mut buffer = image::RgbImage::new(300, 300);
            for (x, y, pixel) in buffer.enumerate_pixels_mut() {
                *pixel = image::Rgb([
                    u8::try_from(x % 256).unwrap_or(0),
                    u8::try_from(y % 256).unwrap_or(0),
                    90,
                ]);
            }
            let mut out = Vec::new();
            image::DynamicImage::ImageRgb8(buffer)
                .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
                .expect("codifica");
            out
        };

        for (nome, titolo) in [("01.wav", "Uno"), ("02.wav", "Due")] {
            let path = lib.musica().join("Art/Al").join(nome);
            wav(&path);
            let mut tag = Tag::new(TagType::Id3v2);
            tag.set_title(titolo.to_owned());
            tag.set_artist("Art".to_owned());
            tag.set_album("Al".to_owned());
            tag.push_picture(
                lofty::picture::Picture::unchecked(copertina.clone())
                    .pic_type(lofty::picture::PictureType::CoverFront)
                    .mime_type(lofty::picture::MimeType::Png)
                    .build(),
            );
            tag.save_to_path(&path, WriteOptions::default())
                .expect("tag con copertina");
        }

        let esito = lib.scansiona();
        assert_eq!(esito.covers_stored, 1, "ricodificata una volta");
        assert_eq!(esito.covers_reused, 1, "la seconda era già lì");
        assert_eq!(lib.conta("cover_art"), 1);
        // E l'album la eredita dai suoi brani.
        let hash: Option<String> = lib
            .connection
            .query_row("SELECT cover_art_hash FROM albums", [], |r| r.get(0))
            .expect("album");
        assert!(hash.is_some(), "l'album deve avere la copertina dei brani");
    }
}
