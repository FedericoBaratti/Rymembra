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
//!
//! # Il lucchetto, non solo la transazione
//!
//! Le sezioni qui sopra parlano di transazioni: quanto a lungo si tiene aperta
//! una scrittura su SQLite. C'è una seconda cosa che dura, e per l'utente conta
//! di più: **il permesso di parlare con la libreria**. Sul desktop la
//! connessione sta dietro un mutex che duecento chiamanti si contendono, e
//! quello che lo tiene fermo ferma anche la finestra, l'audio e la chiusura
//! dell'applicazione.
//!
//! Per questo la scansione non riceve una `Connection`: riceve un [`Deposito`],
//! e gliela chiede una fase per volta. Fra una fase e l'altra il lucchetto è di
//! chi lo vuole. Nelle due fasi lunghe — la camminata sul disco e la lettura dei
//! file — il deposito non viene toccato affatto, ed è la ragione per cui
//! esistono: sono i minuti in cui una share lenta può far aspettare, e in cui
//! nessun altro deve pagare quell'attesa.
//!
//! La contropartita è che fra due prese il database può cambiare sotto i piedi.
//! Le conseguenze sono elencate su [`Scan::run_su`], che è dove si trattano.
//!
//! # Nessuna attesa senza scadenza
//!
//! Camminare e aprire file sono le due cose che su una condivisione di rete non
//! falliscono mai: aspettano. Passano quindi entrambe da
//! [`crate::scadenza`] — la camminata a rate, contando il silenzio fra due file;
//! la lettura su un [`crate::scadenza::Operaio`], che riusa lo stesso filo
//! finché uno non gli si appende addosso. I due numeri stanno in [`Scadenze`],
//! e si possono cambiare da fuori perché le prove non aspettino quindici
//! secondi per dimostrare che qualcosa scade.
//!
//! Una scadenza sola non è però una diagnosi: un file può essere grosso, o il
//! disco pigro. Tre di fila sulla stessa radice sì, e allora la radice si
//! abbandona anche se alla sonda risponde ancora — vedi [`SCADENZE_DI_FILA`].
//! Senza quella regola una share viva ma esausta si farebbe pagare
//! venticinque secondi per ogni file rimasto, uno dopo l'altro.

use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aether_domain::album::{AlbumMember, AlbumRow, album_group_key, build_album_groups};
use aether_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use aether_domain::keys::{TrackKey, TrackKeyInput};
use aether_domain::paths::{PathRules, file_stem, is_under};
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

/// Sotto quante righe una rimozione non è mai «di massa».
///
/// Cinquanta: chi cancella a mano una cartella di un album ne toglie una decina,
/// chi riorganizza la libreria ne muove centinaia — e le seconde non passano
/// nemmeno di qui, perché uno spostamento riconosciuto non è una rimozione. La
/// soglia serve a non far scattare la guardia su una libreria piccolissima, dove
/// «il quinto dei brani» sono tre righe.
const SOGLIA_RIMOZIONI: usize = 50;

/// Oltre quale frazione della libreria una rimozione è sospetta.
///
/// Un quinto, scritto come divisore per non fare aritmetica in virgola mobile su
/// dei conteggi. Non è una soglia di verità — non esiste un numero che separi
/// «ha svuotato il disco» da «gli è caduta la rete» — è la soglia oltre la quale
/// vale la pena aspettare che qualcuno guardi.
const FRAZIONE_SOSPETTA: usize = 5;

/// Quanto silenzio si sopporta fra due file durante una camminata.
///
/// **Non** la durata della camminata: quella su una libreria da centomila file
/// dura legittimamente dei minuti, e limitarla vorrebbe dire dichiarare morto un
/// NAS grande. Quel che si misura è l'intervallo fra due consegne, che su un
/// disco che risponde è dell'ordine dei microsecondi.
///
/// Quindici secondi: molto sopra il singhiozzo di un Wi-Fi che rinegozia — che
/// dura al più qualche secondo — e sotto i quaranta e passa che Windows impiega
/// a rinunciare da solo su un percorso di rete morto. Fra i due estremi il
/// numero non deve essere preciso, deve solo starci in mezzo.
const SILENZIO_CAMMINATA: Duration = Duration::from_secs(15);

/// Quanto si aspetta la lettura di un singolo brano.
///
/// Venticinque secondi, che è lo stesso numero della scadenza sull'analisi di un
/// brano nel livello desktop, e non per caso: è la stessa domanda — «quanto può
/// legittimamente durare l'apertura e la lettura di un file audio prima che
/// convenga concludere che non arriverà?» — e due risposte diverse alla stessa
/// domanda sarebbero due numeri da tenere allineati a mano.
const LETTURA_BRANO: Duration = Duration::from_secs(25);

/// Quante letture scadute di fila bastano a condannare una radice.
///
/// Tre file di fila che non arrivano in venticinque secondi non sono tre file
/// lenti, è una condivisione che non ce la fa. Una sola potrebbe essere una
/// copertina da dieci megabyte in coda a un FLAC, ed è per questo che il numero
/// non è uno: un disco USB pigro non deve poter far sparire dall'esito tutto
/// quel che ci sta sopra.
///
/// La regola vale **in aggiunta** alla sonda di [`Scan::file_illeggibile`], non
/// al suo posto: la sonda riconosce la radice che non risponde più, questo
/// contatore riconosce quella che risponde e non serve. Senza, ogni file
/// rimasto costerebbe venticinque secondi di attesa più otto di sonda, per
/// sempre, e ognuno lascerebbe dietro di sé un filo appeso.
const SCADENZE_DI_FILA: usize = 3;

/// Ogni quanto si guarda se qualcuno ha chiesto di smettere.
///
/// Duecentocinquanta millisecondi: sotto la soglia in cui un «Annulla» sembra
/// non aver funzionato, sopra quella in cui il ciclo d'attesa diventa esso
/// stesso un costo. È il passo con cui la camminata si sveglia mentre non arriva
/// niente, quindi decide anche con quale grana si misura il silenzio.
const BATTITO: Duration = Duration::from_millis(250);

/// I due tempi che una scansione è disposta ad aspettare.
///
/// Un tipo e non due costanti perché devono poter essere **iniettati**: una
/// prova che dimostri cosa succede a una radice arenata non può aspettarne
/// quindici secondi reali, e una che aspetti quindici secondi reali è una prova
/// che nessuno lancerà più.
///
/// Chi non ha opinioni usa [`Scadenze::default`], che sono i numeri pensati per
/// una condivisione SMB su Wi-Fi — il caso peggiore che si sia visto davvero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scadenze {
    /// Silenzio massimo fra due file durante la camminata.
    pub camminata: Duration,
    /// Attesa massima per la lettura di un singolo brano.
    pub brano: Duration,
}

impl Default for Scadenze {
    fn default() -> Self {
        Self {
            camminata: SILENZIO_CAMMINATA,
            brano: LETTURA_BRANO,
        }
    }
}

/// Traduce un errore di SQLite nel catalogo, tenendo il testo originale.
pub(crate) fn db_error(detail: &str, err: &rusqlite::Error) -> AppError {
    AppError::new(ErrorCode::DbQueryFailed {
        detail: Some(detail.to_owned()),
    })
    .with_cause(err.to_string())
}

/// L'ora attuale in millisecondi. Zero se l'orologio è dietro l'epoca.
///
/// # Perché ce n'è una sola per tutta la cassa
///
/// Perché ce n'erano tre — questa, `desiderati::adesso` e la sua gemella in
/// `scrobble` — e le tre si comportavano diversamente su un caso solo: che i
/// millisecondi dall'epoca non entrino in un `i64`. Le altre due rispondevano
/// `i64::MAX`, questa risponde `0`, e resta la risposta di questa.
///
/// La differenza non è osservabile: `i64::MAX` millisecondi cadono nell'anno
/// 292 milioni, mentre l'orologio di sistema più capiente su cui Aether gira —
/// il `FILETIME` di Windows, un `u64` di centinaia di nanosecondi dal 1601 — si
/// esaurisce prima dell'anno 60 000. Sul caso che invece capita davvero, un
/// orologio regolato prima del 1970, tutte e tre dicevano già `0`.
pub(crate) fn now_ms() -> i64 {
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
    /// L'immagine c'era e non si è potuta salvare, con il perché.
    ///
    /// Non fa fallire il brano — la musica si tiene comunque — ma non va
    /// nemmeno lasciata cadere in silenzio: un disco pieno che si mangia
    /// quattrocento copertine di fila è una cosa che chi scansiona deve poter
    /// leggere, e distinguerla da un file illeggibile conta perché la reazione è
    /// diversa. Da qui finisce in [`ScanReport::cover_failures`].
    pub copertina_persa: Option<AppError>,
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

    // L'esito si spacca in due invece di finire in un `.ok()`: quel `.ok()`
    // buttava via l'unica spiegazione che ci fosse di una copertina mancante, e
    // chi guardava l'esito della scansione vedeva un brano senza immagine senza
    // nessun modo di sapere se il file non ne avesse una o se il disco fosse
    // pieno.
    let (cover, copertina_persa) = match tags.cover.as_ref() {
        None => (None, None),
        Some(incorporata) => match covers.store(&incorporata.data, CoverSource::Tag) {
            Ok(salvata) => (Some(salvata), None),
            Err(err) => (None, Some(err)),
        },
    };

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
        copertina_persa,
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
///
/// # Perché anche il percorso, e non il solo identificativo
///
/// Perché fra il momento in cui il piano ha deciso «questa riga va tolta» e il
/// momento in cui la si toglie, la libreria è **stata di qualcun altro**: la
/// scansione lascia il deposito fra una fase e l'altra apposta, e in quel mezzo
/// una sincronizzazione o un'importazione possono aver cancellato quella riga e
/// inserito qualcos'altro. SQLite riusa gli identificativi liberi: la riga 412
/// che il piano voleva togliere può non essere più la riga 412 che c'è adesso.
///
/// Il percorso rende la cancellazione **una domanda sulla riga che si è vista**,
/// non sul numero che aveva. Se non combacia più non si toglie niente, il
/// conteggio lo dice, e la passata dopo — che guarda di nuovo il disco — decide
/// da capo con i fatti aggiornati.
///
/// Le coppie arrivano da `ScanPlan::to_remove`, che porta già entrambi.
fn remove_tracks(tx: &Transaction<'_>, righe: &[(i64, &str)]) -> Result<usize, AppError> {
    let mut statement = tx
        .prepare_cached("DELETE FROM tracks WHERE id = ?1 AND path = ?2")
        .map_err(|err| db_error("rimozione di un brano", &err))?;
    let mut removed = 0;
    for (id, path) in righe {
        removed += statement
            .execute(rusqlite::params![id, path])
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
    /// Copertine che non si sono potute salvare.
    ///
    /// A parte dagli illeggibili perché la reazione di chi legge è diversa:
    /// un'immagine rotta riguarda quel brano, un disco pieno riguarda tutta la
    /// scansione.
    pub cover_failures: Vec<Unreadable>,
    /// Copertine ricodificate ora.
    pub covers_stored: usize,
    /// Copertine già nello store, non ricodificate.
    pub covers_reused: usize,
    /// Gli aggregati ricostruiti.
    pub aggregates: Aggregates,
    /// Quanto è durata.
    pub elapsed_ms: u128,
    /// È stata fermata a metà.
    ///
    /// Non è un errore e non è un fallimento: quel che ha fatto è già scritto, e
    /// la passata dopo riprende da lì. Serve a chi la mostra, per non dire
    /// «scansione completata» a chi ha appena premuto Annulla.
    pub cancelled: bool,
    /// Le radici che non hanno risposto, e che quindi non sono state guardate.
    ///
    /// Va **mostrato**: una scansione che non ha visto la cartella sul NAS ha
    /// fatto un lavoro parziale, e un «completata» che non lo dice fa credere
    /// che i brani mancanti non ci siano più.
    pub radici_saltate: Vec<String>,
    /// Righe che il piano toglierebbe e che la guardia ha lasciato stare.
    ///
    /// Diverso da zero solo nelle scansioni non presidiate (`prudente`): vedi
    /// [`Scan::prudente`].
    pub rimozioni_rinviate: usize,
}

/// Il risultato dell'esplorazione del disco: cosa si è visto, e cosa no.
///
/// Un tipo e non una coppia perché le due metà si leggono insieme: il piano
/// dice cosa fare, l'elenco delle radici saltate dice **su quale porzione di
/// mondo** quel piano è stato calcolato. Chi prende il primo senza il secondo
/// crede di avere una fotografia completa e sta guardando mezza libreria.
#[derive(Debug, Default)]
pub struct Esplorazione {
    /// Cosa la scansione ha deciso di fare, sulle radici che ha visto.
    pub piano: ScanPlan,
    /// Le radici che non hanno risposto: nessun brano loro è stato giudicato.
    pub radici_saltate: Vec<String>,
    /// Qualcuno ha chiesto di smettere mentre si camminava.
    ///
    /// Quando è `true` il piano è **vuoto**, non parziale, e va letto come «non
    /// si è deciso niente». Un piano calcolato su mezza camminata sarebbe
    /// peggio: le radici non ancora guardate non entrano fra quelle viste, e i
    /// loro brani risulterebbero «fuori competenza» — un numero che chi guarda
    /// la finestra leggerebbe come un fatto sul disco invece che come la
    /// conseguenza di aver premuto Annulla.
    pub fermata: bool,
}

/// Una scansione da eseguire.
///
/// # Perché niente è in prestito
///
/// `files` sta dietro un [`Arc`] e `covers` è posseduto, invece dei riferimenti
/// che sarebbero bastati a leggerli. La ragione è la scadenza: quando un file su
/// una share morta non si apre, il filo che ci sta dentro **non si può
/// interrompere**, si abbandona — e un filo abbandonato sopravvive a chi l'ha
/// lanciato. Tutto quel che gli si è dato in mano deve quindi essere `'static`,
/// cioè posseduto, altrimenti non compila. Non è un dettaglio di firma: è la
/// forma che la struttura prende per poter rinunciare ad aspettare.
///
/// Per la stessa ragione questa non è più una struttura `Copy`: un `Arc` e un
/// [`CoverStore`] si clonano — e i cloni costano un contatore e un `PathBuf` —
/// ma non si copiano da soli.
pub struct Scan<'a> {
    /// Da dove arrivano i file: disco locale o Storage Access Framework.
    pub files: Arc<dyn MusicFiles>,
    /// Dove finiscono le copertine.
    ///
    /// Per valore, e non in prestito: è un `PathBuf` dietro un `Clone`, quindi
    /// costa quanto una stringa, e il filo che legge un brano deve poterselo
    /// portare via. Sul desktop questo è anche ciò che permette di prenderlo
    /// **fuori** dal lucchetto della libreria, dove sta insieme alla
    /// connessione pur non cambiando mai.
    pub covers: CoverStore,
    /// Le cartelle da guardare.
    pub roots: &'a [String],
    /// Come si confrontano i percorsi su questo filesystem.
    pub rules: PathRules,
    /// Nessuno sta guardando: davanti a una strage, ci si ferma.
    ///
    /// Una scansione a mano l'ha chiesta qualcuno che è davanti alla finestra e
    /// vede l'esito. Una scansione automatica — quella che la coda dei download
    /// fa quando ha finito — non la vede nessuno, e se sbaglia se ne accorge il
    /// giorno dopo chi non trova più metà libreria.
    ///
    /// Con `prudente` acceso, una passata che toglierebbe una quota enorme di
    /// brani non toglie niente e lo dichiara in
    /// [`ScanReport::rimozioni_rinviate`]. Non è una regola di dominio nascosta
    /// qui dentro: è la stessa cautela di `cancelled`, cioè «nel dubbio, non
    /// distruggere; la passata dopo ci ripensa».
    pub prudente: bool,
    /// Quanto si è disposti ad aspettare il disco.
    ///
    /// [`Scadenze::default`] per il codice vero; numeri piccoli nelle prove, che
    /// altrimenti dovrebbero aspettare quindici secondi per dimostrare che
    /// qualcosa scade.
    pub scadenze: Scadenze,
    /// Qualcuno ha chiesto di smettere?
    ///
    /// Letta nelle due fasi lunghe: **durante la camminata**, dove
    /// `on_progress` non viene chiamato affatto — non c'è ancora niente da
    /// contare — e **fra un file e l'altro** durante la lettura. Senza la
    /// prima, premere Annulla mentre si enumera un NAS non farebbe niente fino
    /// al primo file letto, che potrebbe non arrivare mai; senza la seconda,
    /// fermare la scansione dipenderebbe da cosa risponde il chiamante, e chi
    /// non offre un Annulla — la scansione automatica, che passa un
    /// `Continue` fisso — non riuscirebbe a fermarla nemmeno per chiudere il
    /// programma.
    ///
    /// `on_progress` resta quel che è sempre stato: i numeri dell'avanzamento,
    /// e la facoltà di dire «basta» a chi quei numeri li sta guardando. Le due
    /// cose convivono perché rispondono a domande diverse, e in [`Scan::run_su`]
    /// basta una delle due.
    ///
    /// `None` vuol dire «nessuno può fermarla», ed è la risposta giusta per un
    /// esempio da riga di comando. Chi la offre all'utente passa la stessa
    /// bandiera che legge anche il callback di avanzamento: due bandiere diverse
    /// sarebbero un Annulla che ferma la camminata e non la lettura, o
    /// viceversa.
    pub fermati: Option<&'a dyn Fn() -> bool>,
}

impl std::fmt::Debug for Scan<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scan")
            .field("roots", &self.roots)
            .field("rules", &self.rules)
            .field("prudente", &self.prudente)
            .field("scadenze", &self.scadenze)
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

/// Com'è andata la camminata su **una** radice.
///
/// I tre casi sono tre reazioni diverse, ed è per questo che sono un tipo e non
/// un `Result<Camminata, _>`: visto si giudica, arenato si salta, fermato si
/// smette del tutto.
enum Passeggiata {
    /// La camminata è finita e ha consegnato quel che ha trovato.
    Vista {
        /// I file consegnati.
        file: Vec<DiscoveredFile>,
        /// Nessun ramo perso: vedi [`crate::files::Camminata::completa`].
        completa: bool,
    },
    /// La camminata non è finita: da troppo non arriva più niente.
    ///
    /// Non è «vuota» e non è «parziale», è **non tornata**. Su una condivisione
    /// che non risponde è la forma che prende il guasto, perché il filesystem
    /// non dà nessun errore: resta lì.
    Arenata,
    /// Qualcuno ha chiesto di smettere.
    Fermata,
}

/// Quel che la camminata consegna, un pezzo per volta.
enum Passo {
    /// Un file.
    Trovato(DiscoveredFile),
    /// Non ce ne sono altri.
    Fine {
        /// Nessun ramo perso.
        completa: bool,
    },
    /// La camminata è fallita.
    ///
    /// L'errore viaggia col passo e non lo legge nessuno, ed è una scelta: chi
    /// consuma reagisce a qualunque guasto allo stesso modo — salta la radice —
    /// e non ha dove scriverlo, perché [`ScanReport::unreadable`] è un elenco di
    /// **file** e questo è un guasto che riguarda una cartella intera. Resta
    /// attaccato al passo perché è la sola cosa che dica *perché* quella radice
    /// è uscita dall'esito, e il giorno in cui la si vorrà mettere nel diario
    /// dev'essere già lì invece che ricostruita da capo.
    #[expect(
        dead_code,
        reason = "la causa se la porta appresso il passo che l'ha prodotta; chi salta la radice non ha dove scriverla"
    )]
    Caduta(AppError),
}

/// Cammina una radice sola, smettendo di aspettare se smette di arrivare roba.
///
/// La camminata gira su un filo suo e consegna un [`Passo`] per file; qui si
/// consuma con un `recv_timeout` da un [`BATTITO`], e ogni file che arriva
/// rimanda in avanti la scadenza. Quel che si misura è quindi il **silenzio**,
/// non la durata: una libreria enorme su un disco sano non scade mai, una share
/// morta scade sempre.
///
/// # Un guasto non abortisce le altre radici
///
/// Un [`Passo::Caduta`] — la camminata che restituisce un errore invece di
/// finire — vale [`Passeggiata::Arenata`], cioè «di questa radice non si è
/// visto niente», e la scansione prosegue con le altre.
///
/// È un **cambio deliberato**: prima l'errore risaliva con un `?` fino a far
/// fallire l'intera scansione, e il risultato era che una cartella illeggibile
/// sul NAS impediva di scansionare anche il disco interno. La stessa regola che
/// vale per una radice spenta — «si salta quella, non si rinuncia a tutto» —
/// non aveva motivo di non valere anche qui.
fn cammina_una_radice(scan: &Scan<'_>, root: &str) -> Passeggiata {
    let disco = Arc::clone(&scan.files);
    let percorso = root.to_owned();
    let Some(passi) = crate::scadenza::a_rate("camminata", move |manda| {
        // La `send` che fallisce vuol dire che chi ascoltava se n'è andato:
        // niente da consegnare e niente da dire, si smette.
        let mut consegna = |file| {
            if manda.send(Passo::Trovato(file)).is_ok() {
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(())
            }
        };
        let ultimo = match disco.walk_a_rate(&percorso, &mut consegna) {
            Ok(completa) => Passo::Fine { completa },
            Err(err) => Passo::Caduta(err),
        };
        let _ = manda.send(ultimo);
    }) else {
        // Il sistema non dà più fili: per chi chiama è la stessa cosa di una
        // radice che non risponde, e la reazione giusta è la stessa.
        return Passeggiata::Arenata;
    };

    let mut file = Vec::new();
    let mut ultima_consegna = Instant::now();
    // Il primo file ha diritto al doppio, e solo lui: il cronometro parte prima
    // che il produttore abbia fatto la sua prima `read_dir`, quindi quell'attesa
    // comprende l'apertura della radice — su una VPN lenta, un giro di rete che
    // col silenzio fra due file non c'entra niente. Dal secondo in poi si misura
    // quel che la scadenza vuole misurare, e vale il numero pieno.
    let mut margine = scan.scadenze.camminata.saturating_mul(2);
    loop {
        if scan.fermato() {
            return Passeggiata::Fermata;
        }
        match passi.recv_timeout(BATTITO) {
            Ok(Passo::Trovato(trovato)) => {
                file.push(trovato);
                ultima_consegna = Instant::now();
                margine = scan.scadenze.camminata;
            }
            Ok(Passo::Fine { completa }) => return Passeggiata::Vista { file, completa },
            Ok(Passo::Caduta(_)) => return Passeggiata::Arenata,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if ultima_consegna.elapsed() >= margine {
                    return Passeggiata::Arenata;
                }
            }
            // Il filo è finito senza dire come: o è caduto, o non è riuscito a
            // mandare l'ultimo passo. In nessuno dei due casi si sa di aver
            // visto tutto, quindi vale come arenata.
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Passeggiata::Arenata,
        }
    }
}

/// Cosa si è visto guardando il disco, e cosa no.
#[derive(Debug, Default)]
pub struct Ricognizione {
    /// Tutti i file trovati, di tutte le radici che hanno risposto.
    pub trovati: Vec<DiscoveredFile>,
    /// Le radici su cui si è guardato davvero, e su cui quindi si può decidere.
    ///
    /// È l'elenco che va nel piano: una riga che non sta sotto nessuna di queste
    /// è «fuori competenza», non «sparita».
    pub viste: Vec<String>,
    /// Le radici che non hanno risposto, o che si sono arenate a metà.
    pub radici_saltate: Vec<String>,
    /// Qualcuno ha chiesto di smettere prima della fine.
    ///
    /// Quando è `true` le altre tre vanno lette come parziali: la camminata si è
    /// interrotta, e le radici che non si sono ancora guardate non sono né viste
    /// né saltate.
    pub fermata: bool,
}

/// Guarda tutte le radici, senza toccare il database.
///
/// È la prima metà di [`Scan::run_su`], estratta perché è anche la metà lunga:
/// su una libreria di rete può durare minuti, e in quei minuti nessun lucchetto
/// dev'essere in mano a nessuno.
///
/// # Le radici che non rispondono restano fuori
///
/// Una cartella su una condivisione spenta cammina «vuota»: la camminata non
/// fallisce, restituisce zero file. Passata così al dominio, quella radice
/// risulta **guardata e trovata deserta**, e ogni brano che ci stava sotto
/// diventa da cancellare, con dentro voti, preferiti e cronologia.
///
/// La correzione non tocca il dominio: le radici morte semplicemente non entrano
/// in [`Ricognizione::viste`]. Il meccanismo che serve c'è già — una riga che non
/// sta sotto nessuna radice sorvegliata è `untouched`, cioè «fuori competenza» —
/// ed è lo stesso che impedisce a una scansione della sola cartella dei download
/// di svuotare il resto della libreria.
pub fn cammina(scan: &Scan<'_>) -> Ricognizione {
    let mut esito = Ricognizione::default();
    for root in scan.roots {
        if scan.fermato() {
            esito.fermata = true;
            return esito;
        }
        if !scan.files.radice_raggiungibile(root) {
            esito.radici_saltate.push(root.clone());
            continue;
        }
        match cammina_una_radice(scan, root) {
            Passeggiata::Fermata => {
                esito.fermata = true;
                return esito;
            }
            // Saltata **senza risondare**, e la differenza conta. Il gate qui
            // sotto esiste perché una camminata vuota o monca può essere una
            // share morta, e la sonda lo dice; una camminata arenata invece è
            // già la prova, e la sonda a quel punto è una domanda a cui il
            // sistema risponde male: la cartella si apre — dalla cache, o
            // perché è solo il contenuto a non arrivare — e il «sì» farebbe
            // passare per completo un elenco che non è nemmeno finito.
            Passeggiata::Arenata => {
                esito.radici_saltate.push(root.clone());
                continue;
            }
            Passeggiata::Vista { file, completa } => {
                // La rete può cadere **fra** la sonda e la camminata, e in quel
                // caso la camminata non lo dice da sé: torna vuota, o torna
                // monca. Le due si trattano uguali, e si risonda: se adesso la
                // radice non risponde più, quel vuoto non era un vuoto e quella
                // metà non era la libreria.
                //
                // Il caso monco è il più insidioso dei due, perché ha l'aria di
                // essere andato bene: su una libreria grande la share fa in
                // tempo a rispondere per i primi mille file e a morire sugli
                // altri centomila, e quei centomila — senza questo controllo —
                // diventano «spariti», cioè da cancellare, con dentro voti,
                // preferiti e cronologia.
                //
                // La sonda resta il giudice, e non `completa` da sé: una
                // sottocartella a permessi negati sul disco di casa abbassa
                // `completa` esattamente come una share morta, ma risponde alla
                // sonda — e lì la scansione deve proseguire come ha sempre
                // fatto.
                if (!completa || file.is_empty()) && !scan.files.radice_raggiungibile(root) {
                    esito.radici_saltate.push(root.clone());
                    continue;
                }
                esito.trovati.extend(file);
                esito.viste.push(root.clone());
            }
        }
    }
    esito
}

/// Calcola il piano senza toccare niente.
///
/// È la chiamata che precede [`Scan::run`] quando si vuole mostrare all'utente
/// cosa sta per succedere — «sto per togliere 340 brani» — e lasciargli dire di
/// no prima che succeda.
///
/// La camminata è la stessa di [`cammina`], scadenze comprese: le radici che non
/// rispondono restano fuori dal piano, quelle che si arenano pure. Se qualcuno
/// ha chiesto di smettere, il piano che torna è **vuoto** e
/// [`Esplorazione::fermata`] lo dichiara.
///
/// Dalla finestra non si passa di qui ma da [`Scan::run_su`], che il piano se lo
/// calcola da sé: questa prende una `Connection` in prestito per tutta la
/// durata, camminata inclusa, ed è esattamente la cosa che sul desktop non si
/// può fare.
pub fn plan(scan: &Scan<'_>, connection: &Connection) -> Result<Esplorazione, AppError> {
    let ricognizione = cammina(scan);
    if ricognizione.fermata {
        return Ok(Esplorazione {
            piano: ScanPlan::default(),
            radici_saltate: ricognizione.radici_saltate,
            fermata: true,
        });
    }
    let known = known_tracks(connection)?;
    Ok(Esplorazione {
        piano: plan_scan(
            ScanInput {
                roots: &ricognizione.viste,
                found: &ricognizione.trovati,
                known: &known,
            },
            scan.rules,
        ),
        radici_saltate: ricognizione.radici_saltate,
        fermata: false,
    })
}

/// Chi custodisce la connessione, e la presta una fase per volta.
///
/// # Perché un tratto e non una `&mut Connection`
///
/// Perché sul desktop la connessione non è di nessuno: sta dietro un mutex che
/// duecento chiamanti si contendono, e chi lo tiene ferma la finestra, l'audio e
/// la chiusura dell'applicazione. Una scansione che ricevesse `&mut Connection`
/// obbligherebbe chi la chiama a prendere quel lucchetto **prima** e a
/// restituirlo **dopo**, cioè a tenerlo per tutta la camminata e tutta la
/// lettura dei file: i minuti in cui una share lenta fa aspettare.
///
/// Con un deposito la scansione chiede la connessione quando le serve
/// davvero — cinque o sei volte in tutto, ognuna per il tempo di una
/// transazione — e fra una richiesta e l'altra il lucchetto è di chi lo vuole.
///
/// # Perché un metodo generico e non una chiusura
///
/// Perché la connessione non può uscire dal metodo: chi implementa questo tratto
/// sul desktop tiene una guardia di mutex viva per la durata della chiamata, e
/// un `fn connessione(&mut self) -> &mut Connection` non avrebbe modo di dire
/// quando quella guardia si può lasciare andare. Il prezzo è che il tratto non è
/// oggetto-sicuro — niente `dyn Deposito` — e per questo [`Scan::run_su`] è
/// generica.
///
/// # Il contratto
///
/// - **Può aspettare.** Una chiamata a [`Self::con_connessione`] può bloccare
///   quanto serve; è chi la chiama a fare in modo che non ce ne siano dentro le
///   fasi lunghe.
/// - **Fra due chiamate il database cambia.** Il deposito non promette nessuna
///   continuità: un'altra scrittura può essere passata in mezzo. Vedi
///   [`Scan::run_su`] per cosa comporta.
/// - **L'esclusione di una seconda scansione non spetta a qui.** Il nucleo non
///   sa quante scansioni ci sono: prima l'esclusione era un effetto collaterale
///   del lucchetto tenuto tutto il tempo, e togliendo quello va rimessa dove
///   deve stare, cioè in chi decide di avviarne una.
pub trait Deposito {
    /// Presta la connessione per il tempo di `azione`.
    fn con_connessione<T, F>(&mut self, azione: F) -> Result<T, AppError>
    where
        F: FnOnce(&mut Connection) -> Result<T, AppError>;
}

/// Il deposito più semplice che ci sia: la connessione è già in mano.
///
/// È il caso della riga di comando e delle prove, dove nessuno contende niente.
/// Sul desktop l'implementazione vera avvolge il mutex della libreria.
impl Deposito for Connection {
    fn con_connessione<T, F>(&mut self, azione: F) -> Result<T, AppError>
    where
        F: FnOnce(&mut Connection) -> Result<T, AppError>,
    {
        azione(self)
    }
}

/// Un file che non è arrivato in tempo.
///
/// `FsNetworkUnavailable` e non un codice suo: per chi legge l'esito è la stessa
/// cosa — «questo file non si è potuto leggere, e la colpa non è del file» — e
/// soprattutto è la stessa cosa per [`Scan::file_illeggibile`], che su questo
/// codice fa la sonda e decide se abbandonare la radice. Un codice nuovo
/// vorrebbe dire ripetere lì la stessa domanda con un altro nome.
///
/// La causa dice quanto si è aspettato, con lo stesso lessico con cui il motore
/// audio racconta un'apertura scaduta: chi legge il diario incontra la stessa
/// frase per lo stesso guasto.
fn scaduto(path: &str, scadenza: Duration) -> AppError {
    AppError::new(ErrorCode::FsNetworkUnavailable {
        path: Some(path.to_owned()),
    })
    .with_cause(format!(
        "la lettura non è finita entro {} secondi",
        scadenza.as_secs()
    ))
}

impl Scan<'_> {
    /// Qualcuno ha chiesto di smettere?
    ///
    /// `false` quando nessuno può chiederlo, che è il caso di chi non offre un
    /// Annulla.
    fn fermato(&self) -> bool {
        self.fermati.is_some_and(|chiedi| chiedi())
    }

    /// La radice sorvegliata sotto cui sta questo file, se ce n'è una.
    ///
    /// Serve a due domande sole — «questa radice l'ho già dichiarata morta?» e
    /// «di quale radice è morta la rete?» — e usa la stessa `is_under` con cui
    /// il dominio decide se una riga è di competenza di questa passata. Due
    /// funzioni diverse per la stessa domanda vorrebbero dire che un giorno una
    /// riga può essere sorvegliata per l'una e non per l'altra.
    fn radice_di(&self, path: &str) -> Option<&str> {
        self.roots
            .iter()
            .find(|root| is_under(path, root, self.rules))
            .map(String::as_str)
    }

    /// Un file che non si è potuto leggere: è rotto lui, o è caduta la rete?
    ///
    /// La distinzione costa una sonda — una sola, la prima volta che una radice
    /// dà guai — e vale l'intera scansione: un guasto di rete non è un file
    /// illeggibile da elencare a schermo, è una cartella da smettere di
    /// interrogare. Continuare significherebbe una scadenza per ogni file
    /// rimasto, e un elenco di «illeggibili» lungo quanto la libreria.
    ///
    /// Ci passa anche il file che non è arrivato **in tempo**: una lettura
    /// scaduta è indistinguibile da un guasto di rete finché non si chiede alla
    /// radice come sta, ed è esattamente la domanda che si fa qui.
    ///
    /// Con una risposta sola, però, non si conclude tutto: una radice che alla
    /// sonda risponde e alle letture no resta qui un file illeggibile per volta.
    /// A contare quante volte di fila accade — e a smettere alla terza — è
    /// [`Scan::run_su`], che ha il conto sott'occhio; vedi [`SCADENZE_DI_FILA`].
    fn file_illeggibile(
        &self,
        file: &DiscoveredFile,
        error: AppError,
        abbandonate: &mut Vec<String>,
        illeggibili: &mut Vec<Unreadable>,
    ) {
        if error.code().kind() == ErrorCodeKind::FsNetworkUnavailable
            && let Some(root) = self.radice_di(&file.path)
            && !self.files.radice_raggiungibile(root)
        {
            abbandonate.push(root.to_owned());
            return;
        }
        // Tutto il resto — compreso un guasto di rete su una radice che alla
        // sonda risponde ancora, cioè un singhiozzo — resta quel che era: un
        // file che questa passata non ha letto, detto per nome.
        illeggibili.push(Unreadable {
            path: file.path.clone(),
            error,
        });
    }

    /// Esegue la scansione chiedendo la connessione una fase per volta.
    ///
    /// `on_progress` riceve `(fatti, totale)` sui soli file da leggere, che sono
    /// la parte lenta. Rimozioni e aggregati non entrano nel conteggio perché
    /// non hanno una durata percepibile.
    ///
    /// # Le fasi, e quali non toccano il deposito
    ///
    /// I numeri sono quelli che ritrovi nei commenti del corpo.
    ///
    /// 1. **La camminata.** Nessuna connessione in mano: è la fase che su una
    ///    share lenta dura minuti. Chi si ferma qui non ha deciso niente, e
    ///    infatti non scrive niente — nemmeno una presa.
    /// 2. **Il piano.** Una presa per leggere le righe note, poi una funzione
    ///    pura del dominio che decide.
    /// 3. **Le righe doppie e le sparizioni in sospeso.** Una presa sola.
    /// 4. **La lettura dei file.** Di nuovo niente in mano, e ogni file con una
    ///    scadenza addosso: è l'altra fase lunga.
    /// 5. **La scrittura di ogni lotto.** Una presa per lotto, una transazione
    ///    dentro.
    /// 6. **Le rimozioni rimaste.** Una presa.
    /// 7. **Gli aggregati.** Una presa.
    ///
    /// # Cosa può cambiare sotto i piedi
    ///
    /// Lasciando il deposito fra una fase e l'altra si accetta che il database
    /// cambi in mezzo. Le conseguenze sono quattro, e sono tutte trattate:
    ///
    /// - **Gli identificativi si riusano.** SQLite riassegna i numeri liberi:
    ///   la riga 412 che il piano voleva togliere può essere stata cancellata e
    ///   rimpiazzata da un'altra. Per questo si cancella per identificativo
    ///   **e** percorso — vedi `remove_tracks`.
    /// - **Una riga può essere sparita.** Non è un errore: `track_keys_of` la
    ///   salta, una cancellazione che non trova niente conta zero, e la passata
    ///   dopo decide da capo.
    /// - **`tracks.path` è UNIQUE.** Se un'altra scansione stesse inserendo gli
    ///   stessi file, un inserimento potrebbe fallire. È il motivo per cui
    ///   l'esclusione di una seconda scansione spetta a chi le avvia: qui non
    ///   c'è più il lucchetto tenuto tutto il tempo a garantirla per sbaglio.
    /// - **Le copertine no.** Lo store è indirizzato per contenuto e si scrive
    ///   in modo atomico: due scansioni che salvano la stessa immagine scrivono
    ///   lo stesso file, e `INSERT OR IGNORE` fa il resto.
    ///
    /// # Fermarsi
    ///
    /// Durante la camminata risponde [`Scan::fermati`]: non si è ancora deciso
    /// niente, quindi non si scrive niente e l'esito è un `cancelled` con il
    /// piano vuoto.
    ///
    /// Durante la lettura rispondono in due, e basta uno: `on_progress` che
    /// restituisce [`ControlFlow::Break`], o la stessa [`Scan::fermati`]. La
    /// scansione si ferma allora **alla fine del file in corso**: il lotto
    /// chiude la sua transazione con le righe che ha già letto, e lì si
    /// smette. Non alla fine del lotto — su una share dove ogni apertura costa
    /// mezzo minuto, i quattrocentonovantanove file rimasti sarebbero ore fra
    /// l'Annulla e il momento in cui succede qualcosa. Quel che è stato scritto
    /// resta.
    ///
    /// Non è un annullamento nel senso di «disfare» — è «basta così»: la passata
    /// dopo trova una libreria giusta e incompleta e la finisce da sé. È il
    /// contrario di come si annulla di solito, e il motivo sta nella forma della
    /// scansione: ogni lotto sta nella sua transazione, apposta perché
    /// un'interruzione qualunque — un crollo, una chiusura, una macchina che si
    /// spegne — non lasci niente a metà. Un annullamento è solo l'interruzione
    /// che qualcuno ha chiesto, e trattarla diversamente vorrebbe dire scrivere
    /// un secondo percorso per lo stesso caso.
    pub fn run_su<D: Deposito>(
        &self,
        deposito: &mut D,
        mut on_progress: impl FnMut(usize, usize) -> ControlFlow<()>,
    ) -> Result<ScanReport, AppError> {
        let started = Instant::now();

        // ── 1. il disco, senza niente in mano ──
        let ricognizione = cammina(self);

        // ── 1 bis. chi si è fermato camminando non ha deciso niente ──
        //
        // E quindi non scrive niente: nessuna presa del deposito, nessuna
        // transazione. Il piano resta vuoto perché un piano calcolato su mezza
        // camminata direbbe «spariti» dei brani che stanno sotto radici che non
        // si è fatto in tempo a guardare.
        if ricognizione.fermata {
            return Ok(ScanReport {
                cancelled: true,
                radici_saltate: ricognizione.radici_saltate,
                elapsed_ms: started.elapsed().as_millis(),
                ..ScanReport::default()
            });
        }

        // ── 2. le righe note, e il piano, che è una funzione pura ──
        let known = deposito.con_connessione(|connection| known_tracks(connection))?;
        let mut report = ScanReport {
            plan: plan_scan(
                ScanInput {
                    roots: &ricognizione.viste,
                    found: &ricognizione.trovati,
                    known: &known,
                },
                self.rules,
            ),
            radici_saltate: ricognizione.radici_saltate,
            ..ScanReport::default()
        };

        // ── 3. una presa sola: le doppie se ne vanno, le sparizioni si tengono ──
        //
        // Due righe che nominano lo stesso file: quella che sopravvive sta per
        // ricevere il percorso come la camminata l'ha scritto, e `tracks.path` è
        // UNIQUE — se la doppia fosse ancora lì, l'aggiornamento fallirebbe.
        // Non sono candidate a essere spostamenti: il loro file non si è mosso,
        // è la riga a essere di troppo.
        //
        // Le sparizioni invece restano in sospeso: qualcuna è uno spostamento, e
        // lo si scopre solo leggendo i file nuovi.
        let doppie: Vec<(i64, String)> = report
            .plan
            .to_remove
            .iter()
            .filter(|r| r.reason == RemoveReason::DuplicateRow)
            .map(|r| (r.track.id, r.track.path.clone()))
            .collect();
        let spariti: Vec<i64> = report
            .plan
            .to_remove
            .iter()
            .filter(|r| r.reason == RemoveReason::Disappeared)
            .map(|r| r.track.id)
            .collect();
        let (tolte, mut sospese) = deposito.con_connessione(|connection| {
            let mut tolte = 0;
            if !doppie.is_empty() {
                let tx = connection
                    .transaction()
                    .map_err(|err| db_error("apertura della transazione", &err))?;
                let elenco: Vec<(i64, &str)> = doppie
                    .iter()
                    .map(|(id, path)| (*id, path.as_str()))
                    .collect();
                tolte = remove_tracks(&tx, &elenco)?;
                tx.commit()
                    .map_err(|err| db_error("chiusura della transazione", &err))?;
            }
            let sospese = track_keys_of(connection, &spariti)?;
            Ok((tolte, sospese))
        })?;
        report.removed += tolte;

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
        // Le radici che hanno smesso di rispondere **mentre** si leggeva.
        //
        // La sonda della camminata guarda le radici prima di cominciare; questa
        // è la rete che se ne va dopo, a lettura iniziata. Senza, una share
        // caduta a metà costava una scadenza per ogni file rimasto — e i suoi
        // brani, non riletti, restavano candidati alla rimozione.
        let mut abbandonate: Vec<String> = Vec::new();
        // Quante letture di fila sono scadute su ciascuna radice.
        //
        // Solo le scadenze: un errore vero — un file rotto, un permesso negato
        // — non dice niente sulla salute della condivisione, e non conta né in
        // un senso né nell'altro. Una lettura riuscita azzera il conto della
        // sua radice, perché quel che si sta cercando è una **serie**: alla
        // [`SCADENZE_DI_FILA`]-esima la radice si abbandona anche se alla sonda
        // risponde ancora. I file che hanno pagato la scadenza restano
        // nell'elenco degli illeggibili, detti per nome: sono stati tentati.
        let mut scadenze_di_fila: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        // ── 4. la lettura, di nuovo senza niente in mano ──
        //
        // Un filo solo per tutte le letture, riusato finché uno non gli si
        // appende addosso: centomila file non sono centomila fili.
        let mut operaio = crate::scadenza::Operaio::nuovo("lettura");
        for lotto in da_leggere.chunks(LOTTO) {
            let mut righe: Vec<(TrackRow, Destinazione)> = Vec::with_capacity(lotto.len());
            for (file, destinazione) in lotto {
                // Su una radice già dichiarata morta non si tenta nemmeno: ogni
                // apertura su una share che non risponde costa la scadenza
                // intera, e su una libreria vera i file rimasti sono decine di
                // migliaia. Il primo file paga l'attesa e la scopre per tutti.
                let su_radice_morta = self
                    .radice_di(&file.path)
                    .is_some_and(|root| abbandonate.iter().any(|a| a == root));
                if !su_radice_morta {
                    // Il deposito non è in mano a nessuno qui dentro, ed è il
                    // punto di tutto: è la fase che su una share lenta dura, e
                    // se la connessione fosse presa la finestra aspetterebbe.
                    //
                    // Tutto quel che serve al filo se lo porta via: l'`Arc` dei
                    // file, la cartella delle copertine, il file da leggere. Un
                    // filo che si appende sopravvive a questa iterazione, e non
                    // può tenere in mano niente che appartenga a `self`.
                    let disco = Arc::clone(&self.files);
                    let copertine = self.covers.clone();
                    let quale = file.clone();
                    let letto = operaio.esegui(self.scadenze.brano, move || {
                        read_track(disco.as_ref(), &copertine, &quale)
                    });
                    match letto {
                        Some(Ok(mut row)) => {
                            // La radice ha risposto: la serie di scadenze, se
                            // ce n'era una cominciata, non è una serie.
                            if let Some(root) = self.radice_di(&file.path) {
                                scadenze_di_fila.remove(root);
                            }
                            // Si prende invece di clonarla: la riga sta per essere
                            // consegnata al lotto, e da lì in poi la copertina persa
                            // non serve più a nessuno.
                            if let Some(persa) = row.copertina_persa.take() {
                                report.cover_failures.push(Unreadable {
                                    path: file.path.clone(),
                                    error: persa,
                                });
                            }
                            if let Some(cover) = row.cover.as_ref() {
                                if cover.already_present {
                                    report.covers_reused += 1;
                                } else {
                                    report.covers_stored += 1;
                                }
                            }
                            righe.push((row, *destinazione));
                        }
                        Some(Err(error)) => self.file_illeggibile(
                            file,
                            error,
                            &mut abbandonate,
                            &mut report.unreadable,
                        ),
                        // Non è tornato in tempo: il filo è ancora dentro la
                        // `open`, e da qui in poi non lo si aspetta più. Vale un
                        // guasto di rete, e come tale fa scattare la sonda della
                        // radice — perché la domanda è la stessa: è lento questo
                        // file, o è morta la cartella?
                        None => {
                            self.file_illeggibile(
                                file,
                                scaduto(&file.path, self.scadenze.brano),
                                &mut abbandonate,
                                &mut report.unreadable,
                            );
                            // E poi la seconda domanda, quella che la sonda non
                            // fa: non «la radice risponde?» ma «la radice serve
                            // a qualcosa?». Alla terza scadenza di fila la
                            // risposta è no, e insistere costerebbe una
                            // scadenza e una sonda per ognuno dei file rimasti.
                            if let Some(root) = self.radice_di(&file.path) {
                                let di_fila = scadenze_di_fila.entry(root).or_insert(0);
                                *di_fila += 1;
                                if *di_fila >= SCADENZE_DI_FILA
                                    && !abbandonate.iter().any(|a| a == root)
                                {
                                    abbandonate.push(root.to_owned());
                                }
                            }
                        }
                    }
                }
                fatti += 1;
                // Due modi di sentirsi dire «basta», e ne basta uno. Il callback
                // parla per chi sta guardando la barra; la bandiera parla per
                // tutti gli altri — l'uscita del programma, un chiamante che di
                // avanzamento non sa che farsene e passa un `Continue` fisso.
                if on_progress(fatti, totale).is_break() || self.fermato() {
                    report.cancelled = true;
                    // E si smette **qui**, non alla fine del lotto: le righe già
                    // lette scendono nella transazione qui sotto, e i file
                    // rimasti del lotto — fino a cinquecento, a mezzo minuto
                    // l'uno su una share stanca — non si leggono affatto.
                    break;
                }
            }

            // ── quali di questi file nuovi sono righe che si sono spostate ──
            //
            // Decisione pura: nessun disco, nessun database.
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

            // ── 5. e solo adesso si scrive: una presa, una transazione ──
            let now = now_ms();
            let scritte = deposito.con_connessione(|connection| {
                let tx = connection
                    .transaction()
                    .map_err(|err| db_error("apertura della transazione", &err))?;
                let mut inserite = 0_usize;
                let mut aggiornate = 0_usize;
                let mut spostate = 0_usize;
                for (row, destinazione) in &righe {
                    match *destinazione {
                        Destinazione::Nuova => {
                            insert_track(&tx, row, now)?;
                            inserite += 1;
                        }
                        Destinazione::Aggiorna(id) => {
                            update_track(&tx, id, row, now)?;
                            aggiornate += 1;
                        }
                        Destinazione::Sposta(id) => {
                            update_track(&tx, id, row, now)?;
                            spostate += 1;
                        }
                    }
                }
                tx.commit()
                    .map_err(|err| db_error("chiusura della transazione", &err))?;
                Ok((inserite, aggiornate, spostate))
            })?;
            report.inserted += scritte.0;
            report.updated += scritte.1;
            report.moved += scritte.2;

            // Il lotto è chiuso e scritto: è l'unico punto in cui fermarsi non
            // lascia niente a metà.
            if report.cancelled {
                break;
            }
        }

        // ── le radici che sono morte mentre si leggeva ──
        //
        // I loro brani non hanno avuto la loro occasione: non sono stati riletti
        // perché la share non risponde più, non perché non ci siano. Toglierli
        // sarebbe la stessa cancellazione che la sonda della camminata impedisce
        // all'inizio della passata, spostata di qualche minuto più in là — e a
        // quel punto costerebbe di più, perché una scansione arrivata a metà ha
        // già scritto tutto il resto.
        if !abbandonate.is_empty() {
            let sotto_una_morta = |id: i64| {
                report
                    .plan
                    .to_remove
                    .iter()
                    .find(|r| r.track.id == id)
                    .is_some_and(|r| {
                        abbandonate
                            .iter()
                            .any(|root| is_under(&r.track.path, root, self.rules))
                    })
            };
            sospese.retain(|(id, _)| !sotto_una_morta(*id));
            // E si dice: una passata che non ha visto la cartella sul NAS ha
            // fatto un lavoro parziale, e un «completata» che non lo dichiara fa
            // credere che i brani mancanti non ci siano più.
            for root in abbandonate {
                if !report.radici_saltate.contains(&root) {
                    report.radici_saltate.push(root);
                }
            }
        }

        // ── 6. le sparizioni che nessun file nuovo ha reclamato ──
        //
        // **Saltate** se ci si è fermati a metà, ed è la parte che rende
        // l'annullamento sicuro invece che distruttivo. Queste righe sono brani
        // il cui file non c'è più *dove era*, e restano in sospeso perché uno
        // dei lotti successivi potrebbe ritrovarli altrove — è così che
        // riorganizzare la libreria non azzera conteggi, preferiti e voti.
        //
        // Fermandosi al terzo lotto su dieci, i sette che non sono stati letti
        // non hanno avuto la loro occasione di reclamarli: cancellarli qui
        // vorrebbe dire che premere Annulla a metà di una scansione dopo aver
        // spostato una cartella distrugge le statistiche di tutti i brani che
        // ci stavano dentro.
        if !report.cancelled && !sospese.is_empty() {
            // ── e la guardia contro le stragi che nessuno sta guardando ──
            //
            // Quante righe la libreria conosceva prima di questa passata: le
            // invariate, quelle fuori dalle radici, quelle da rileggere e quelle
            // che il piano toglierebbe. Non c'è un contatore unico su `ScanPlan`
            // perché il piano racconta le decisioni, non l'anagrafica — e
            // sommarle qui tiene la somma accanto all'unica regola che la usa.
            let totale_conosciute = report
                .plan
                .unchanged
                .saturating_add(report.plan.untouched)
                .saturating_add(report.plan.to_update.len())
                .saturating_add(report.plan.to_remove.len());
            // Il caso vero: un disco che non si è montato all'avvio, o una
            // radice che risponde ma è vuota perché il punto di mount è caduto.
            // Dopo la protezione delle radici irraggiungibili resta improbabile,
            // e proprio per questo non costa niente rimandarlo alla prossima
            // passata — quando qualcuno guarda.
            let strage = self.prudente
                && sospese.len() > SOGLIA_RIMOZIONI
                && sospese.len().saturating_mul(FRAZIONE_SOSPETTA) > totale_conosciute;
            if strage {
                report.rimozioni_rinviate = sospese.len();
            } else {
                // Il percorso viene dal piano, cioè da com'era la riga quando la
                // si è giudicata: se nel frattempo qualcuno l'ha cambiata, la
                // cancellazione non la riconosce e non la tocca.
                let elenco: Vec<(i64, String)> = sospese
                    .iter()
                    .filter_map(|(id, _)| {
                        report
                            .plan
                            .to_remove
                            .iter()
                            .find(|r| r.track.id == *id)
                            .map(|r| (*id, r.track.path.clone()))
                    })
                    .collect();
                let tolte = deposito.con_connessione(|connection| {
                    let tx = connection
                        .transaction()
                        .map_err(|err| db_error("apertura della transazione", &err))?;
                    let coppie: Vec<(i64, &str)> = elenco
                        .iter()
                        .map(|(id, path)| (*id, path.as_str()))
                        .collect();
                    let tolte = remove_tracks(&tx, &coppie)?;
                    tx.commit()
                        .map_err(|err| db_error("chiusura della transazione", &err))?;
                    Ok(tolte)
                })?;
                report.removed += tolte;
            }
        }

        // ── 7. gli aggregati, quando i brani sono tutti al loro posto ──
        report.aggregates = deposito.con_connessione(|connection| {
            let tx = connection
                .transaction()
                .map_err(|err| db_error("apertura della transazione", &err))?;
            let aggregati = rebuild_aggregates(&tx)?;
            tx.commit()
                .map_err(|err| db_error("chiusura della transazione", &err))?;
            Ok(aggregati)
        })?;

        report.elapsed_ms = started.elapsed().as_millis();
        Ok(report)
    }

    /// Esegue la scansione su una connessione che si ha già in mano.
    ///
    /// La forma comoda per chi la connessione ce l'ha per sé — la riga di
    /// comando, le prove, il ponte Android. Sul desktop non si passa di qui:
    /// prendere il lucchetto della libreria prima di chiamare e restituirlo dopo
    /// è esattamente ciò che [`Scan::run_su`] esiste per non fare.
    pub fn run(
        &self,
        connection: &mut Connection,
        on_progress: impl FnMut(usize, usize) -> ControlFlow<()>,
    ) -> Result<ScanReport, AppError> {
        self.run_su(connection, on_progress)
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
pub(crate) const COLONNE_BRANO: &str = "t.id, t.path, t.title, t.artist, t.album, t.album_key,
     t.track_number, t.disc_number, t.duration_ms, t.year, t.cover_art_hash,
     t.play_count, t.liked, t.rating";

pub(crate) fn track_from_row(row: &Row<'_>) -> rusqlite::Result<TrackSummary> {
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

/// Le colonne di [`AlbumSummary`], nell'ordine in cui le legge [`album_from_row`].
///
/// Qualificate con `a.`: una delle query che le usano ha un'altra tabella
/// accanto, e `album_key` da solo diventerebbe ambiguo.
pub(crate) const COLONNE_ALBUM: &str =
    "a.album_key, a.title, a.artist, a.year, a.genre, a.total_tracks, a.cover_art_hash";

/// Un album, da una riga di [`COLONNE_ALBUM`].
///
/// Era scritta tre volte identica — la griglia, gli album di un artista, i
/// dischi entrati per ultimi. Tre copie di sette `row.get` numerati sono tre
/// occasioni di sfasare un indice, e uno sfasamento fra `year` e `genre` non è
/// un errore di compilazione: è una griglia che scrive «2019» dove va il genere.
pub(crate) fn album_from_row(row: &Row<'_>) -> rusqlite::Result<AlbumSummary> {
    Ok(AlbumSummary {
        album_key: row.get(0)?,
        title: row.get(1)?,
        artist: row.get(2)?,
        year: row.get(3)?,
        genre: row.get(4)?,
        total_tracks: row.get(5)?,
        cover_art_hash: row.get(6)?,
    })
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
    /// Gli ascoltati più di recente in cima, i mai ascoltati in fondo.
    ///
    /// Diverso da `MostPlayed`, che ordina per **quante volte**: qui conta
    /// **quando**, ed è la domanda a cui risponde la Home. Un brano suonato
    /// una volta stamattina viene prima di uno suonato cento volte l'anno
    /// scorso.
    RecentlyPlayed,
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
            // `IS NULL` per primo, così i mai ascoltati finiscono in fondo
            // invece che in cima: in SQLite `NULL` è più piccolo di tutto, e
            // senza questa colonna un `DESC` li metterebbe per ultimi solo per
            // caso — cioè li metterebbe per primi.
            Self::RecentlyPlayed => {
                "t.last_played_at IS NULL, t.last_played_at DESC, t.title COLLATE NOCASE"
            }
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

/// Una pagina di preferiti.
///
/// # Perché una query e non un filtro sull'elenco dei brani
///
/// La finestra chiedeva duemila brani e teneva quelli col cuore. Due cose
/// storte insieme: legge tutta la libreria a ogni visita, e chi ne ha più di
/// duemila non vede i preferiti che stanno oltre — senza che niente lo dica.
///
/// L'ordine è quello della **decisione**, non del titolo: chi apre i preferiti
/// cerca quasi sempre quello che ha segnato poco fa. `liked_at` è nullo sulle
/// righe segnate prima che quella colonna esistesse, e `IS NULL` in testa
/// all'`ORDER BY` le manda in fondo invece di lasciarle dove capita.
pub fn list_liked(
    connection: &Connection,
    offset: i64,
    limit: i64,
) -> Result<Vec<TrackSummary>, AppError> {
    let sql = format!(
        "SELECT {COLONNE_BRANO} FROM tracks t WHERE t.liked = 1
         ORDER BY t.liked_at IS NULL, t.liked_at DESC, t.title COLLATE NOCASE
         LIMIT ?1 OFFSET ?2"
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("elenco dei preferiti", &err))?;
    let rows = statement
        .query_map(rusqlite::params![limit, offset], track_from_row)
        .map_err(|err| db_error("elenco dei preferiti", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("elenco dei preferiti", &err))
}

/// Un ascolto, con il brano che lo ha prodotto.
///
/// Il brano intero e non il solo identificativo: chi guarda la cronologia
/// guarda dei titoli, e restituire duecento `track_id` obbligherebbe la finestra
/// a duecento richieste — o a una `summaries_by_id` che rimescola l'ordine
/// proprio quando l'ordine *è* il contenuto.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoceCronologia {
    /// La riga di `play_history`, per distinguere due ascolti dello stesso
    /// brano nello stesso secondo.
    pub id: i64,
    /// Il brano, come lo mostra ogni altro elenco.
    pub brano: TrackSummary,
    /// Quando è **cominciato**, in millisecondi dall'epoca: è `played_at`, che
    /// `record_play` scrive all'inizio dell'ascolto.
    pub quando_ms: i64,
    /// Quanto se n'è sentito.
    pub ms_ascoltati: i64,
    /// Da dove viene: `local` se l'ha suonato Aether, `spotify` se importato.
    pub sorgente: String,
}

/// Una pagina di cronologia, dal più recente.
///
/// # Perché esiste solo adesso
///
/// `play_history` si scriveva dal primo giorno e non la leggeva nessuno: la
/// linguetta «Cronologia» della terza colonna era spenta con la sua ragione
/// scritta accanto. Era un difetto piccolo finché quella tabella conteneva
/// soltanto gli ascolti fatti dentro Aether — su una libreria appena aperta,
/// zero righe. Dopo l'importazione di un account Spotify contiene anni, e una
/// schermata che non li mostra diventa la differenza fra aver importato e non
/// averlo fatto.
///
/// # Un ascolto il cui brano non c'è più non compare
///
/// È una `JOIN`, non una `LEFT JOIN`. `play_history.track_id` ha
/// `ON DELETE CASCADE`, quindi il caso è già impossibile per un brano tolto
/// davvero; la `JOIN` è ciò che rende impossibile anche il resto — una riga
/// senza titolo né interprete in mezzo a un elenco di canzoni.
pub fn list_history(
    connection: &Connection,
    offset: i64,
    limit: i64,
) -> Result<Vec<VoceCronologia>, AppError> {
    let sql = format!(
        "SELECT {COLONNE_BRANO}, h.id, h.played_at, h.ms_played, h.source
           FROM play_history AS h
           JOIN tracks AS t ON t.id = h.track_id
          ORDER BY h.played_at DESC, h.id DESC
          LIMIT ?1 OFFSET ?2"
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("cronologia d'ascolto", &err))?;
    // 14 colonne di brano: le successive cominciano da 14, e il conto lo tiene
    // `COLONNE_BRANO` insieme a `track_from_row`. Se una colonna venisse
    // aggiunta là, questi indici si spostano — ed è il motivo per cui la prova
    // qui sotto controlla il campo `quando_ms` e non solo la lunghezza.
    let rows = statement
        .query_map(rusqlite::params![limit, offset], |row| {
            Ok(VoceCronologia {
                brano: track_from_row(row)?,
                id: row.get(14)?,
                quando_ms: row.get(15)?,
                ms_ascoltati: row.get(16)?,
                sorgente: row.get(17)?,
            })
        })
        .map_err(|err| db_error("cronologia d'ascolto", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("cronologia d'ascolto", &err))
}

/// Quanti ascolti ci sono in tutto.
///
/// Serve alla finestra per sapere se c'è una pagina dopo. Una `COUNT(*)` su una
/// tabella di centomila righe con un indice su `played_at` costa poco, e la
/// alternativa — chiedere una riga in più e guardare se arriva — mente sul
/// totale, che qui è l'unica cosa che dice «hai importato dieci anni».
pub fn count_history(connection: &Connection) -> Result<i64, AppError> {
    connection
        .prepare_cached("SELECT COUNT(*) FROM play_history")
        .and_then(|mut statement| statement.query_row([], |row| row.get(0)))
        .map_err(|err| db_error("conteggio della cronologia", &err))
}

/// Un brano solo, per identificativo.
///
/// `None` se la riga non c'è più: capita a un brano tolto dalla libreria mentre
/// la coda lo teneva ancora, e non è un guasto da propagare — è una riga da
/// saltare.
pub fn read_summary(connection: &Connection, id: i64) -> Result<Option<TrackSummary>, AppError> {
    let sql = format!("SELECT {COLONNE_BRANO} FROM tracks t WHERE t.id = ?1");
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("lettura di un brano", &err))?;
    statement
        .query_row([id], track_from_row)
        .map(Some)
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            altro => Err(db_error("lettura di un brano", &altro)),
        })
}

/// Le righe di un elenco di identificativi, **nell'ordine chiesto**.
///
/// L'ordine è il punto. Una `IN (…)` restituisce le righe nell'ordine che fa
/// comodo a SQLite, e chi chiama qui — il pannello della coda — ha un ordine
/// suo che è l'unica cosa che gli interessa: la coda non suona per
/// identificativo crescente. Riordinare a valle costa una mappa e toglie una
/// classe di difetti in cui l'elenco mostrato non è quello che si sentirà.
///
/// Gli identificativi che non esistono più semplicemente non compaiono.
pub fn summaries_by_id(
    connection: &Connection,
    ids: &[i64],
) -> Result<Vec<TrackSummary>, AppError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    // I segnaposto si contano, non si compongono con i valori: un elenco di
    // identificativi resta un elenco di parametri anche quando è lungo.
    let segnaposto = std::iter::repeat_n("?", ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("SELECT {COLONNE_BRANO} FROM tracks t WHERE t.id IN ({segnaposto})");
    let mut statement = connection
        .prepare(&sql)
        .map_err(|err| db_error("brani per identificativo", &err))?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(ids), track_from_row)
        .map_err(|err| db_error("brani per identificativo", &err))?;
    let trovati = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("brani per identificativo", &err))?;

    // Si clona invece di consumare la mappa: la stessa canzone può stare due
    // volte nella stessa coda — accodarla due volte è legittimo — e toglierla
    // dalla mappa alla prima occorrenza la farebbe sparire dalla seconda.
    let per_id: std::collections::HashMap<i64, TrackSummary> =
        trovati.into_iter().map(|b| (b.id, b)).collect();
    Ok(ids
        .iter()
        .filter_map(|id| per_id.get(id).cloned())
        .collect())
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
    let sql = format!(
        "SELECT {COLONNE_ALBUM} FROM albums a
         ORDER BY a.artist COLLATE NOCASE, a.year, a.title COLLATE NOCASE
         LIMIT ?1 OFFSET ?2"
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("elenco degli album", &err))?;
    let rows = statement
        .query_map(rusqlite::params![limit, offset], album_from_row)
        .map_err(|err| db_error("elenco degli album", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("elenco degli album", &err))
}

/// I dischi entrati in libreria per ultimi.
///
/// # Perché dischi e non brani
///
/// Il ripiano «aggiunti di recente» della Home chiedeva i dodici brani con la
/// `date_added` più alta. Ma la musica non entra un brano alla volta: entra una
/// cartella alla volta, e una cartella è un disco. Le dodici righe erano dodici
/// tracce **dello stesso album** — per giunta in ordine arbitrario, perché
/// condividono la `date_added` al secondo e il criterio di spareggio era l'id.
/// Un ripiano che mostra dodici volte la stessa copertina non dice «cos'è
/// entrato di nuovo»: dice «l'ultima cartella», e la dice dodici volte.
///
/// # L'ordine di spareggio, e perché non è il titolo
///
/// Uno spareggio ci vuole: due dischi entrati nello stesso istante devono
/// presentarsi nello stesso ordine a ogni apertura, o il ripiano si riscrive da
/// solo fra una visita e l'altra senza che sia successo niente. Il candidato
/// ovvio era il titolo, e sarebbe stato sbagliato — su una libreria arrivata
/// tutta in una volta (una cartella sola scansionata una volta sola: il caso
/// normale al primo avvio, dove `date_added` ha *un* valore per migliaia di
/// brani) il titolo diventa l'unico criterio, e «aggiunti di recente» mostra i
/// dodici dischi che cominciano per A. Corretto e illeggibile.
///
/// Quindi `MAX(t.id)`: l'ordine in cui le righe sono state scritte, che è più
/// fine della data al millisecondo e vuol dire la stessa cosa — chi è entrato
/// per ultimo sta davanti. È stabile quanto il titolo, perché un id non cambia
/// più, e non ha bisogno di essere letto dalla tabella: in SQLite l'id **è** il
/// rowid, e sta dentro ogni voce dell'indice che serve il raggruppamento.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn recently_added_albums(
    connection: &Connection,
    limit: i64,
) -> Result<Vec<AlbumSummary>, AppError> {
    // Il raggruppamento in una sottoquery e non sull'esterna: così gira su
    // `idx_tracks_album_added` e ne esce una riga per album, invece di tutte le
    // tracce della libreria da scartare dopo.
    let sql = format!(
        "SELECT {COLONNE_ALBUM} FROM albums a
         JOIN (SELECT album_key, MAX(date_added) AS entrato, MAX(id) AS ultima
               FROM tracks WHERE album_key IS NOT NULL
               GROUP BY album_key) u ON u.album_key = a.album_key
         ORDER BY u.entrato DESC, u.ultima DESC
         LIMIT ?1"
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("dischi aggiunti di recente", &err))?;
    let rows = statement
        .query_map(rusqlite::params![limit], album_from_row)
        .map_err(|err| db_error("dischi aggiunti di recente", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("dischi aggiunti di recente", &err))
}

/// Una pagina degli album di un artista.
///
/// Confronto esatto sul nome come sta nei tag, che è la stessa chiave con cui
/// [`list_artists`] raggruppa: la normalizzazione serve a **ordinare** gli
/// artisti, non a fonderli, e due grafie diverse restano due artisti anche
/// nella griglia.
///
/// Esiste perché la pagina di un artista filtrava nella finestra la pagina di
/// album già scaricata: chi stava oltre l'ultimo album chiesto vedeva una
/// pagina vuota sotto un titolo che diceva «tre album».
pub fn albums_by_artist(
    connection: &Connection,
    artist: &str,
    offset: i64,
    limit: i64,
) -> Result<Vec<AlbumSummary>, AppError> {
    let sql = format!(
        "SELECT {COLONNE_ALBUM} FROM albums a
         WHERE a.artist = ?1
         ORDER BY a.year, a.title COLLATE NOCASE
         LIMIT ?2 OFFSET ?3"
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("album di un artista", &err))?;
    let rows = statement
        .query_map(rusqlite::params![artist, limit, offset], album_from_row)
        .map_err(|err| db_error("album di un artista", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("album di un artista", &err))
}

/// Un artista come lo mostra la griglia.
///
/// # Perché le copertine sono quattro e vengono dagli album
///
/// Nessuna immagine d'artista esisterà mai: non c'è rete, e un lettore locale
/// che va a prendersi i ritratti su internet è un lettore locale che chiama
/// casa. Il ritratto è un mosaico due per due delle copertine dei suoi dischi —
/// dati che ci sono già, e che per di più dicono qualcosa di vero: si riconosce
/// un artista dai suoi album prima che dalla sua faccia.
///
/// Quattro perché il mosaico ne mostra quattro. Chi ne ha uno solo ottiene una
/// copertina sola a tutto riquadro, che è il caso giusto e non un ripiego.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistSummary {
    /// Il nome come sta nei tag.
    pub name: String,
    /// Il nome sotto cui ordinarlo, senza articolo iniziale.
    pub sort_name: String,
    /// Quanti album.
    pub albums: i64,
    /// Quanti brani.
    pub tracks: i64,
    /// Fino a quattro copertine, dalle più vecchie alle più recenti.
    pub covers: Vec<String>,
}

/// Gli artisti della libreria, in ordine alfabetico del nome normalizzato.
///
/// # Perché niente pagina, e perché l'ordine si fa qui
///
/// Gli artisti sono pochi — un ordine di grandezza meno dei brani — e la vista
/// li mostra tutti con un indice alfabetico laterale invece che a pagine: si
/// salta alla lettera, non alla pagina sette. Chiederne una fetta alla volta
/// vorrebbe dire non poter dire quante lettere esistono.
///
/// L'ordinamento **non** è in SQL. `sort_name` è una regola del dominio — «The
/// Cure» sta sotto C — e SQLite non può chiamarla senza che gliela si registri
/// come funzione, cioè senza duplicarla. Ordinare qui la tiene in un posto solo,
/// e quel posto è lo stesso che userà Android.
pub fn list_artists(connection: &Connection) -> Result<Vec<ArtistSummary>, AppError> {
    let mut statement = connection
        .prepare_cached(&format!(
            "SELECT {ARTISTA_EFFETTIVO} AS nome,
                    COUNT(DISTINCT album_key) AS album,
                    COUNT(*)                  AS brani
             FROM tracks
             WHERE {ARTISTA_EFFETTIVO} <> ''
             GROUP BY nome"
        ))
        .map_err(|err| db_error("elenco degli artisti", &err))?;
    let mut artisti: Vec<ArtistSummary> = statement
        .query_map([], |row| {
            let name: String = row.get(0)?;
            Ok(ArtistSummary {
                sort_name: aether_domain::keys::sort_name(&name),
                name,
                albums: row.get(1)?,
                tracks: row.get(2)?,
                covers: Vec::new(),
            })
        })
        .map_err(|err| db_error("elenco degli artisti", &err))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("elenco degli artisti", &err))?;

    // Le copertine in una seconda passata e non in una sottoquery correlata: una
    // riga per album invece di una per brano, e il raggruppamento a quattro si
    // fa dove è leggibile. Su una libreria vera sono qualche migliaio di righe.
    let mut copertine = connection
        .prepare_cached(&format!(
            "SELECT {ARTISTA_EFFETTIVO} AS nome, MIN(cover_art_hash) AS hash, MIN(year) AS anno
             FROM tracks
             WHERE {ARTISTA_EFFETTIVO} <> '' AND cover_art_hash IS NOT NULL
             GROUP BY nome, album_key
             ORDER BY nome, anno"
        ))
        .map_err(|err| db_error("copertine degli artisti", &err))?;
    let mut per_artista: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let righe = copertine
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|err| db_error("copertine degli artisti", &err))?;
    for riga in righe {
        let (nome, hash) = riga.map_err(|err| db_error("copertine degli artisti", &err))?;
        let elenco = per_artista.entry(nome).or_default();
        // Quattro, e le stesse quattro a ogni chiamata: l'ordine della query è
        // deterministico, quindi il mosaico di un artista non cambia disegno da
        // un'apertura all'altra.
        if elenco.len() < 4 && !elenco.contains(&hash) {
            elenco.push(hash);
        }
    }
    for artista in &mut artisti {
        if let Some(trovate) = per_artista.remove(&artista.name) {
            artista.covers = trovate;
        }
    }

    artisti.sort_by(|a, b| {
        a.sort_name
            .cmp(&b.sort_name)
            // A parità di chiave d'ordinamento — due artisti che differiscono
            // solo per accenti o punteggiatura — decide il nome vero, altrimenti
            // l'ordine dipenderebbe da come `HashMap` ha girato.
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(artisti)
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
    offset: i64,
    limit: i64,
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
         ORDER BY bm25(tracks_fts) LIMIT ?2 OFFSET ?3"
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("ricerca", &err))?;
    let rows = statement
        .query_map(rusqlite::params![expression, limit, offset], track_from_row)
        .map_err(|err| db_error("ricerca", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("ricerca", &err))
}

/// Quanti brani risponderebbero a questa ricerca.
///
/// Serve a poter scrivere «312 risultati» invece di «60 risultati», che è quel
/// che l'intestazione diceva quando il numero era la lunghezza della prima
/// pagina: un conteggio che si ferma al limite non è un conteggio, è il limite
/// scritto in lettere.
///
/// Passa dalla **stessa** [`fts_query`] della ricerca: due traduzioni diverse
/// della stessa stringa sono due occasioni di contare righe che poi non
/// arrivano.
pub fn search_count(connection: &Connection, query: &str) -> Result<i64, AppError> {
    let expression = fts_query(query);
    if expression.is_empty() {
        return Ok(0);
    }
    connection
        .prepare_cached("SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH ?1")
        .and_then(|mut statement| statement.query_row([&expression], |row| row.get(0)))
        .map_err(|err| db_error("conteggio della ricerca", &err))
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
                files: Arc::new(LocalFiles),
                covers: self.store.clone(),
                roots: &roots,
                rules: PathRules::for_current_platform(),
                prudente: false,
                scadenze: Scadenze::default(),
                fermati: None,
            };
            scan.run(&mut self.connection, |_, _| ControlFlow::Continue(()))
                .expect("scansione")
        }

        /// Come `scansiona`, ma si ferma dopo `quanti` file letti.
        fn scansiona_e_ferma(&mut self, quanti: usize) -> ScanReport {
            let roots = vec![self.root()];
            let scan = Scan {
                files: Arc::new(LocalFiles),
                covers: self.store.clone(),
                roots: &roots,
                rules: PathRules::for_current_platform(),
                prudente: false,
                scadenze: Scadenze::default(),
                fermati: None,
            };
            scan.run(&mut self.connection, |fatti, _| {
                if fatti >= quanti {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            })
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
        let trovati = search(&lib.connection, "bjork", 0, 10).expect("ricerca");
        assert_eq!(trovati.len(), 2);
        let per_titolo = search(&lib.connection, "bachelor", 0, 10).expect("ricerca");
        assert_eq!(
            per_titolo.first().map(|t| t.title.as_str()),
            Some("Bachelorette"),
            "una ricerca si fa mentre si digita"
        );
    }

    #[test]
    fn una_scansione_fermata_lascia_una_libreria_giusta_e_incompleta() {
        // Il patto dell'annullamento: quel che è stato letto è scritto, il resto
        // lo finisce la passata dopo. Con il lotto da 500 e sei file, tutto sta
        // in un lotto solo — quindi il conteggio dice quanti ne sono stati letti
        // prima dello stop, e il taglio avviene alla chiusura del lotto.
        let mut lib = Libreria::nuova();
        for n in 1..=6 {
            lib.brano(
                &format!("Tale/Al/{n:02}.wav"),
                &format!("Pezzo {n}"),
                "Tale",
                "Al",
            );
        }

        let esito = lib.scansiona_e_ferma(3);
        assert!(esito.cancelled, "l'esito deve dire che è stata fermata");
        // I brani letti prima dello stop ci sono, e sono righe vere: la
        // transazione del lotto è stata chiusa comunque.
        assert!(lib.conta("tracks") > 0, "quel che è stato letto resta");

        // E la passata dopo finisce, senza rileggere quel che c'era già.
        let seconda = lib.scansiona();
        assert!(!seconda.cancelled);
        assert_eq!(lib.conta("tracks"), 6, "la seconda passata completa");
    }

    #[test]
    fn fermare_una_scansione_non_cancella_i_brani_spostati() {
        // La parte che rende l'annullamento sicuro invece che distruttivo.
        //
        // Un brano il cui file non è più dove era resta **in sospeso**: potrebbe
        // essersi spostato, e uno dei lotti successivi potrebbe ritrovarlo. Se
        // ci si ferma prima, quei lotti non ci sono stati — e cancellare le
        // righe in sospeso vorrebbe dire che premere Annulla dopo aver
        // riorganizzato una cartella azzera conteggi, preferiti e voti di tutti
        // i brani che ci stavano dentro.
        let mut lib = Libreria::nuova();
        lib.brano("Tale/Al/01.wav", "Pezzo", "Tale", "Al");
        lib.scansiona();
        assert_eq!(lib.conta("tracks"), 1);

        // Gli si dà un voto: è la cosa che una cancellazione distruggerebbe e
        // che nessuna riscansione potrebbe ricostruire.
        lib.connection
            .execute("UPDATE tracks SET rating = 5, liked = 1", [])
            .expect("voto");

        // Il file si sposta, e insieme arriva altra musica da leggere.
        let da = lib.musica().join("Tale/Al/01.wav");
        let a = lib.musica().join("Tale/Altrove/01.wav");
        std::fs::create_dir_all(a.parent().expect("cartella")).expect("cartella");
        std::fs::rename(&da, &a).expect("spostamento");
        for n in 2..=5 {
            lib.brano(
                &format!("Nuovi/Al/{n:02}.wav"),
                &format!("Nuovo {n}"),
                "Altri",
                "Al",
            );
        }

        // Ci si ferma al primo file letto: la riga sparita è ancora in sospeso.
        let esito = lib.scansiona_e_ferma(1);
        assert!(esito.cancelled);
        assert_eq!(
            esito.removed, 0,
            "una scansione fermata non toglie righe in sospeso"
        );

        let voto: i64 = lib
            .connection
            .query_row("SELECT COALESCE(MAX(rating), -1) FROM tracks", [], |r| {
                r.get(0)
            })
            .expect("voto");
        assert_eq!(voto, 5, "il voto del brano spostato è sopravvissuto");
    }

    #[test]
    fn gli_artisti_si_ordinano_senza_articolo() {
        // La regola che la vista Artisti promette: «The Cure» sotto C. Vive nel
        // dominio (`sort_name`) e non in SQL, e questo test è il posto in cui si
        // vede che il giro completo — tag sul disco, riga in libreria, elenco
        // ordinato — la rispetta davvero.
        let mut lib = Libreria::nuova();
        lib.brano(
            "Cure/Disintegration/01.wav",
            "Plainsong",
            "The Cure",
            "Disintegration",
        );
        lib.brano(
            "Cure/Disintegration/02.wav",
            "Lovesong",
            "The Cure",
            "Disintegration",
        );
        lib.brano("Cure/Wish/01.wav", "Open", "The Cure", "Wish");
        lib.brano(
            "Air/Moon Safari/01.wav",
            "La femme d'argent",
            "Air",
            "Moon Safari",
        );
        lib.brano(
            "Doors/Strange Days/01.wav",
            "Strange Days",
            "The Doors",
            "Strange Days",
        );
        lib.scansiona();

        let artisti = list_artists(&lib.connection).expect("elenco");
        let nomi: Vec<&str> = artisti.iter().map(|a| a.name.as_str()).collect();
        // Air, poi Cure, poi Doors: l'articolo non conta, e il nome mostrato
        // resta quello vero.
        assert_eq!(nomi, ["Air", "The Cure", "The Doors"]);

        let cure = artisti
            .iter()
            .find(|a| a.name == "The Cure")
            .expect("i Cure");
        assert_eq!(cure.sort_name, "cure");
        assert_eq!(cure.albums, 2, "due album");
        assert_eq!(cure.tracks, 3, "tre brani");
    }

    /// Una PNG diversa per ogni numero, per avere impronte diverse.
    fn png(seme: u8) -> Vec<u8> {
        let mut buffer = image::RgbImage::new(64, 64);
        for (x, y, pixel) in buffer.enumerate_pixels_mut() {
            *pixel = image::Rgb([
                u8::try_from(x % 256).unwrap_or(0),
                u8::try_from(y % 256).unwrap_or(0),
                seme,
            ]);
        }
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(buffer)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .expect("codifica");
        out
    }

    #[test]
    fn il_mosaico_di_un_artista_si_ferma_a_quattro() {
        // Il ritratto è due per due: chiedere tutte le copertine di chi ha
        // trenta dischi vorrebbe dire spedirne ventisei che nessuno disegna.
        let mut lib = Libreria::nuova();
        for n in 1..=6u8 {
            let path = lib.musica().join(format!("Tale/Album {n}")).join("01.wav");
            wav(&path);
            let mut tag = Tag::new(TagType::Id3v2);
            tag.set_title("Pezzo".to_owned());
            tag.set_artist("Tale".to_owned());
            tag.set_album(format!("Album {n}"));
            tag.push_picture(
                lofty::picture::Picture::unchecked(png(n * 40))
                    .pic_type(lofty::picture::PictureType::CoverFront)
                    .mime_type(lofty::picture::MimeType::Png)
                    .build(),
            );
            tag.save_to_path(&path, WriteOptions::default())
                .expect("tag con copertina");
        }
        lib.scansiona();

        let artisti = list_artists(&lib.connection).expect("elenco");
        let tale = artisti.first().expect("un artista");
        assert_eq!(tale.albums, 6);
        assert_eq!(tale.covers.len(), 4, "il mosaico ne mostra quattro");
        // E sono quattro **diverse**: quattro volte la stessa immagine sarebbe
        // un mosaico che sembra un errore di disegno.
        let distinte: std::collections::HashSet<&String> = tale.covers.iter().collect();
        assert_eq!(distinte.len(), 4);
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
            let esito = search(&lib.connection, query, 0, 10);
            assert!(
                esito.is_ok(),
                "{query} ha fatto fallire la ricerca: {esito:?}"
            );
        }
        assert_eq!(
            search(&lib.connection, "AC/DC", 0, 10)
                .expect("ricerca")
                .len(),
            1
        );
        assert!(
            search(&lib.connection, "", 0, 10)
                .expect("ricerca")
                .is_empty(),
            "una ricerca vuota non è un errore, è nessun risultato"
        );
    }

    #[test]
    fn la_ricerca_si_impagina_e_sa_quanti_ne_ha() {
        // Il difetto che chiude: l'intestazione scriveva «N risultati» dove N
        // era la lunghezza della prima pagina, cioè il limite travestito da
        // conteggio. E oltre quel limite non c'era modo di arrivare.
        let mut lib = Libreria::nuova();
        for n in 1..=7 {
            lib.brano(
                &format!("Tale/Al/{n:02}.wav"),
                &format!("Pezzo {n}"),
                "Tale",
                "Al",
            );
        }
        lib.scansiona();

        assert_eq!(
            search_count(&lib.connection, "pezzo").expect("conteggio"),
            7,
            "il conteggio non si ferma alla pagina"
        );
        let prima = search(&lib.connection, "pezzo", 0, 3).expect("ricerca");
        let seconda = search(&lib.connection, "pezzo", 3, 3).expect("ricerca");
        let terza = search(&lib.connection, "pezzo", 6, 3).expect("ricerca");
        assert_eq!((prima.len(), seconda.len(), terza.len()), (3, 3, 1));

        // Le tre pagine non si sovrappongono e coprono tutto: senza un ordine
        // stabile due fette consecutive potrebbero ripetere una riga e saltarne
        // un'altra, che è il difetto che l'impaginazione porta con sé.
        let mut visti: Vec<i64> = prima
            .iter()
            .chain(&seconda)
            .chain(&terza)
            .map(|b| b.id)
            .collect();
        visti.sort_unstable();
        visti.dedup();
        assert_eq!(visti.len(), 7, "sette righe distinte in tre pagine");

        assert_eq!(
            search_count(&lib.connection, "").expect("conteggio"),
            0,
            "una ricerca vuota conta zero, non tutto"
        );
    }

    #[test]
    fn i_preferiti_sono_una_query_e_non_un_filtro() {
        // Prima si chiedevano duemila brani e si tenevano quelli col cuore:
        // tutta la libreria letta a ogni visita, e chi ne ha più di duemila non
        // vedeva i preferiti oltre — senza che niente lo dicesse.
        let mut lib = Libreria::nuova();
        for n in 1..=5 {
            lib.brano(
                &format!("Tale/Al/{n:02}.wav"),
                &format!("Pezzo {n}"),
                "Tale",
                "Al",
            );
        }
        lib.scansiona();

        // Tre segnati, in tre momenti diversi: l'ordine atteso è quello della
        // decisione, dal più recente.
        lib.connection
            .execute(
                "UPDATE tracks SET liked = 1, liked_at = 300 WHERE title = 'Pezzo 1'",
                [],
            )
            .expect("cuore");
        lib.connection
            .execute(
                "UPDATE tracks SET liked = 1, liked_at = 100 WHERE title = 'Pezzo 3'",
                [],
            )
            .expect("cuore");
        // Segnato prima che `liked_at` esistesse: va in fondo, non dove capita.
        lib.connection
            .execute(
                "UPDATE tracks SET liked = 1, liked_at = NULL WHERE title = 'Pezzo 5'",
                [],
            )
            .expect("cuore");

        let tutti = list_liked(&lib.connection, 0, 100).expect("preferiti");
        assert_eq!(
            tutti.iter().map(|b| b.title.as_str()).collect::<Vec<_>>(),
            ["Pezzo 1", "Pezzo 3", "Pezzo 5"]
        );
        assert!(
            tutti.iter().all(|b| b.liked),
            "solo brani col cuore, e la query non lo deve dimenticare"
        );

        // E si impagina come gli altri elenchi.
        let pagina = list_liked(&lib.connection, 1, 1).expect("preferiti");
        assert_eq!(pagina.len(), 1);
        assert_eq!(pagina.first().map(|b| b.title.as_str()), Some("Pezzo 3"));
    }

    #[test]
    fn la_cronologia_si_legge_dal_piu_recente_e_porta_il_brano_con_se() {
        // La tabella si scriveva dal primo giorno e non la leggeva nessuno.
        // Questa prova tiene fermi i due punti in cui è facile sbagliare: il
        // verso dell'ordine — chi apre la cronologia guarda l'ultima cosa che ha
        // ascoltato, non la prima in assoluto — e gli indici delle colonne dopo
        // quelle del brano, che si spostano se `COLONNE_BRANO` ne guadagna una.
        let mut lib = Libreria::nuova();
        lib.brano("Tale/Al/01.wav", "Primo", "Tale", "Al");
        lib.brano("Tale/Al/02.wav", "Secondo", "Tale", "Al");
        lib.scansiona();

        let ids: Vec<i64> = {
            let mut s = lib
                .connection
                .prepare("SELECT id FROM tracks ORDER BY title")
                .expect("brani");
            let righe = s.query_map([], |r| r.get(0)).expect("query");
            righe.collect::<Result<_, _>>().expect("ids")
        };
        let (primo, secondo) = (ids.first().copied(), ids.get(1).copied());

        for (id, quando, ms, sorgente) in [
            (primo, 1_000_i64, 200_000_i64, "local"),
            (secondo, 3_000, 150_000, "spotify"),
            (primo, 2_000, 210_000, "local"),
        ] {
            lib.connection
                .execute(
                    "INSERT INTO play_history (track_id, played_at, ms_played, source)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![id, quando, ms, sorgente],
                )
                .expect("ascolto");
        }

        let pagina = list_history(&lib.connection, 0, 10).expect("cronologia");
        assert_eq!(count_history(&lib.connection), Ok(3));
        assert_eq!(
            pagina.iter().map(|v| v.quando_ms).collect::<Vec<_>>(),
            [3_000, 2_000, 1_000],
            "il più recente in cima"
        );
        let capo = pagina.first().expect("una riga");
        assert_eq!(capo.brano.title, "Secondo");
        assert_eq!(capo.ms_ascoltati, 150_000);
        assert_eq!(capo.sorgente, "spotify");

        // E si impagina come tutti gli altri elenchi.
        let seconda = list_history(&lib.connection, 2, 10).expect("cronologia");
        assert_eq!(seconda.len(), 1);
        assert_eq!(seconda.first().map(|v| v.quando_ms), Some(1_000));
    }

    #[test]
    fn un_brano_tolto_porta_via_i_suoi_ascolti() {
        // `ON DELETE CASCADE` più la `JOIN`: due difese sullo stesso caso, e la
        // seconda è quella che regge se un giorno la prima venisse tolta da una
        // migrazione. Una riga di cronologia senza titolo né interprete, in
        // mezzo a un elenco di canzoni, non è un dato mancante — è una riga
        // vuota che l'utente legge come un guasto.
        let mut lib = Libreria::nuova();
        lib.brano("Tale/Al/01.wav", "Solo", "Tale", "Al");
        lib.scansiona();
        lib.connection
            .execute(
                "INSERT INTO play_history (track_id, played_at, ms_played)
                 SELECT id, 500, 120000 FROM tracks",
                [],
            )
            .expect("ascolto");
        assert_eq!(list_history(&lib.connection, 0, 10).map(|v| v.len()), Ok(1));

        std::fs::remove_file(lib.musica().join("Tale/Al/01.wav")).expect("cancellato");
        lib.scansiona();
        assert_eq!(list_history(&lib.connection, 0, 10), Ok(vec![]));
    }

    #[test]
    fn gli_album_di_un_artista_si_chiedono_al_database() {
        // Prima si filtrava nella finestra la pagina di album già scaricata:
        // un artista i cui dischi stavano oltre l'ultimo album chiesto dava una
        // pagina vuota sotto un titolo che diceva «due album».
        let mut lib = Libreria::nuova();
        lib.brano("Tale/Primo/01.wav", "A", "Tale", "Primo");
        lib.brano("Tale/Secondo/01.wav", "B", "Tale", "Secondo");
        lib.brano("Altro/Terzo/01.wav", "C", "Altro", "Terzo");
        lib.scansiona();

        let suoi = albums_by_artist(&lib.connection, "Tale", 0, 100).expect("album");
        assert_eq!(
            suoi.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(),
            ["Primo", "Secondo"]
        );
        assert!(
            suoi.iter().all(|a| a.artist == "Tale"),
            "nessun disco di qualcun altro"
        );

        // Confronto esatto sul nome dei tag — `albums.artist` non è dichiarata
        // `COLLATE NOCASE` — e deve restare così: la normalizzazione serve a
        // **ordinare** gli artisti, non a fonderli, e la griglia mostra la
        // stessa chiave con cui `list_artists` raggruppa. Se un giorno le due
        // dovessero fondersi, va deciso in un posto solo e questa prova cade.
        assert!(
            albums_by_artist(&lib.connection, "tale", 0, 100)
                .expect("album")
                .is_empty(),
            "il raggruppamento e il filtro devono usare la stessa chiave"
        );

        let pagina = albums_by_artist(&lib.connection, "Tale", 1, 1).expect("album");
        assert_eq!(pagina.first().map(|a| a.title.as_str()), Some("Secondo"));
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

    #[test]
    fn i_dischi_aggiunti_di_recente_sono_uno_per_album_col_piu_nuovo_in_cima() {
        // Il difetto che questa query esiste per togliere: chiedendo i brani per
        // data d'ingresso, le prime dodici righe sono dodici tracce dello stesso
        // disco — perché la musica non entra un brano alla volta, entra una
        // cartella alla volta.
        let mut lib = Libreria::nuova();
        for n in 1..=3 {
            lib.brano(
                &format!("Tale/Vecchio/{n:02}.wav"),
                &format!("Pezzo {n}"),
                "Tale",
                "Vecchio",
            );
        }
        for n in 1..=3 {
            lib.brano(
                &format!("Tale/Nuovo/{n:02}.wav"),
                &format!("Brano {n}"),
                "Tale",
                "Nuovo",
            );
        }
        lib.scansiona();

        // Le date si scrivono a mano perché una scansione sola le mette tutte
        // nello stesso istante: è esattamente il caso in cui l'ordinamento per
        // brano non aveva niente da dire, e quello per disco deve averlo.
        lib.connection
            .execute(
                "UPDATE tracks SET date_added = 100 WHERE album = 'Vecchio'",
                [],
            )
            .expect("date del disco vecchio");
        lib.connection
            .execute(
                "UPDATE tracks SET date_added = 200 WHERE album = 'Nuovo'",
                [],
            )
            .expect("date del disco nuovo");

        let dischi = recently_added_albums(&lib.connection, 12).expect("dischi recenti");

        assert_eq!(dischi.len(), 2, "due dischi, non sei brani");
        assert_eq!(
            dischi.first().map(|a| a.title.as_str()),
            Some("Nuovo"),
            "l'ultimo entrato sta in cima"
        );
        assert_eq!(dischi.get(1).map(|a| a.title.as_str()), Some("Vecchio"));

        // E il limite conta dischi, non tracce: è la misura del ripiano.
        let uno = recently_added_albums(&lib.connection, 1).expect("dischi recenti");
        assert_eq!(uno.len(), 1);
        assert_eq!(uno.first().map(|a| a.title.as_str()), Some("Nuovo"));
    }

    #[test]
    fn a_pari_data_conta_l_ordine_di_scrittura_e_non_il_titolo() {
        // Il caso normale al primo avvio: una cartella sola, una scansione
        // sola, e `date_added` con un valore solo per tutta la libreria. Lì lo
        // spareggio **è** l'ordinamento, e in ordine alfabetico «aggiunti di
        // recente» sarebbe la lettera A.
        let mut lib = Libreria::nuova();
        lib.brano("Tale/Zulu/01.wav", "Uno", "Tale", "Zulu");
        lib.brano("Tale/Alfa/01.wav", "Uno", "Tale", "Alfa");
        lib.scansiona();
        lib.connection
            .execute("UPDATE tracks SET date_added = 500", [])
            .expect("stessa data per tutti");
        // La riga scritta per ultima, qualunque sia il titolo del suo disco.
        let ultimo: String = lib
            .connection
            .query_row(
                "SELECT album FROM tracks ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .expect("l'ultima riga");

        let titoli =
            |dischi: Vec<AlbumSummary>| dischi.into_iter().map(|a| a.title).collect::<Vec<_>>();
        let primo = titoli(recently_added_albums(&lib.connection, 12).expect("dischi"));
        let secondo = titoli(recently_added_albums(&lib.connection, 12).expect("dischi"));

        assert_eq!(
            primo.first().map(String::as_str),
            Some(ultimo.as_str()),
            "davanti va l'ultimo scritto, non il primo in alfabeto"
        );
        assert_eq!(primo.len(), 2);
        // E due volte di fila la stessa risposta: un ripiano che si rimescola da
        // solo fra una visita e l'altra sembra rotto anche quando non lo è.
        assert_eq!(primo, secondo);
    }

    // ── le radici che non rispondono ────────────────────────────────────────
    //
    // Il guasto che queste prove tengono chiuso è il più caro di tutti: con la
    // radice su una condivisione spenta, `walk` non falliva — restituiva zero
    // file — e il piano dichiarava sparito ogni brano che ci stava sotto. La
    // scansione dopo li cancellava, e con loro voti, preferiti, conteggi e
    // cronologia: le sole cose in libreria che una riscansione non ricostruisce.
    //
    // Qui non serve un disco vero: serve un disco che **mente** nel modo
    // preciso in cui mente una share morta, cioè camminando vuoto.

    /// Un disco finto che può dichiararsi irraggiungibile.
    struct FintoDisco {
        /// Cosa si trova sotto ogni radice viva.
        trovati: std::collections::HashMap<String, Vec<DiscoveredFile>>,
        /// Le radici che la sonda dichiara morte da subito.
        morte: std::collections::HashSet<String>,
        /// Le radici che rispondono alla prima sonda e non alla seconda: è la
        /// rete che se ne va **fra** la sonda e la camminata, cioè la finestra
        /// che la risonda esiste per chiudere.
        cade_in_corsa: std::collections::HashSet<String>,
        /// Quante sonde sono state fatte finora.
        ///
        /// Un atomico e non un contatore normale perché `MusicFiles` è `Sync` e
        /// la sonda si chiama da dietro un `&self`: una cella mutabile qui non
        /// compilerebbe nemmeno.
        sonde: std::sync::atomic::AtomicUsize,
        /// Radici i cui file non si aprono perché la rete se n'è andata.
        rete_giu: std::collections::HashSet<String>,
        /// Quante volte è stato chiesto di aprire un file.
        ///
        /// È il numero che dice se la scansione ha smesso di insistere: su una
        /// share vera ogni tentativo in più costa una scadenza intera, e le
        /// scadenze non si sommano su un filo solo — si sommano in fili
        /// abbandonati, uno per file.
        aperture: std::sync::atomic::AtomicUsize,
        /// Radici che rispondono ma di cui si vede solo un pezzo.
        ///
        /// La share che muore **durante** la camminata: `walkdir` perde i rami
        /// che non rispondono più e prosegue in silenzio, quindi torna un elenco
        /// che sembra buono e non lo è.
        monche: std::collections::HashSet<String>,
        /// Radici la cui camminata consegna quel che ha e poi **non finisce**.
        ///
        /// Il guasto che nessun `Result` sa dire: la share non fallisce, resta
        /// lì. Un `walk` intero non tornerebbe mai; a rate, il silenzio si vede.
        arenate: std::collections::HashSet<String>,
        /// Radici i cui file si aprono, ma con comodo.
        lente: std::collections::HashSet<String>,
        /// Quanto dorme il disco dove ha promesso di essere lento o arenato.
        ///
        /// Uno solo per tutto il doppio: le prove che lo usano ne hanno bisogno
        /// di un valore solo, e due manopole per la stessa attesa sarebbero due
        /// numeri da tenere allineati a mano.
        attesa: std::time::Duration,
        /// La bandiera che il deposito alza mentre presta la connessione.
        ///
        /// Quando c'è, il disco la guarda a ogni camminata e a ogni apertura:
        /// se la trova alzata, qualcuno sta tenendo il lucchetto mentre si fa
        /// I/O — cioè esattamente il difetto che tutto questo lavoro toglie.
        prestata: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
        /// Il disco ha trovato la connessione prestata mentre lavorava.
        ///
        /// Una bandiera e non un `assert!`, perché la camminata gira su un filo
        /// suo: un panico là dentro chiuderebbe il canale, la scansione lo
        /// leggerebbe come «radice arenata» e la prova passerebbe verde senza
        /// aver provato niente.
        sorpresa: std::sync::atomic::AtomicBool,
    }

    impl FintoDisco {
        fn nuovo() -> Self {
            Self {
                trovati: std::collections::HashMap::new(),
                morte: std::collections::HashSet::new(),
                cade_in_corsa: std::collections::HashSet::new(),
                monche: std::collections::HashSet::new(),
                rete_giu: std::collections::HashSet::new(),
                arenate: std::collections::HashSet::new(),
                lente: std::collections::HashSet::new(),
                attesa: std::time::Duration::ZERO,
                prestata: None,
                sorpresa: std::sync::atomic::AtomicBool::new(false),
                aperture: std::sync::atomic::AtomicUsize::new(0),
                sonde: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        fn con_file(mut self, root: &str, file: Vec<DiscoveredFile>) -> Self {
            self.trovati.insert(root.to_owned(), file);
            self
        }

        fn morta(mut self, root: &str) -> Self {
            self.morte.insert(root.to_owned());
            self
        }

        fn cade_dopo_la_sonda(mut self, root: &str) -> Self {
            self.cade_in_corsa.insert(root.to_owned());
            self
        }

        /// La camminata su questa radice torna quel che le è stato dato, ma
        /// dichiarandosi parziale.
        fn cammina_a_meta(mut self, root: &str) -> Self {
            self.monche.insert(root.to_owned());
            self
        }

        /// I file di questa radice non si aprono: la rete non c'è più.
        fn senza_rete(mut self, root: &str) -> Self {
            self.rete_giu.insert(root.to_owned());
            self
        }

        /// La camminata su questa radice consegna quel che ha e poi si pianta.
        fn si_arena(mut self, root: &str, attesa: std::time::Duration) -> Self {
            self.arenate.insert(root.to_owned());
            self.attesa = attesa;
            self
        }

        /// I file di questa radice si aprono, ma dopo `attesa`.
        fn apre_lentamente(mut self, root: &str, attesa: std::time::Duration) -> Self {
            self.lente.insert(root.to_owned());
            self.attesa = attesa;
            self
        }

        /// Tiene d'occhio la bandiera del deposito mentre lavora.
        fn sorveglia(mut self, prestata: &std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
            self.prestata = Some(std::sync::Arc::clone(prestata));
            self
        }

        fn aperture(&self) -> usize {
            self.aperture.load(std::sync::atomic::Ordering::Relaxed)
        }

        /// Il disco ha lavorato mentre la connessione era prestata?
        fn sorpresa(&self) -> bool {
            self.sorpresa.load(std::sync::atomic::Ordering::SeqCst)
        }

        /// Da chiamare in ogni punto in cui il disco fa I/O.
        fn guarda_il_lucchetto(&self) {
            if let Some(prestata) = self.prestata.as_ref()
                && prestata.load(std::sync::atomic::Ordering::SeqCst)
            {
                self.sorpresa
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }

        /// Dorme, se la radice di questo percorso ha promesso di essere lenta.
        fn forse_dormi(&self, path: &str) {
            if self
                .lente
                .iter()
                .any(|root| path.to_lowercase().starts_with(&root.to_lowercase()))
            {
                std::thread::sleep(self.attesa);
            }
        }
    }

    impl MusicFiles for FintoDisco {
        fn walk(&self, root: &str) -> Result<crate::files::Camminata, AppError> {
            self.guarda_il_lucchetto();
            Ok(crate::files::Camminata {
                file: self.trovati.get(root).cloned().unwrap_or_default(),
                completa: !self.monche.contains(root),
            })
        }

        /// Consegna i file uno per uno e, sulle radici arenate, non finisce.
        ///
        /// L'ordine conta: prima si consegna, poi ci si pianta. È il caso vero —
        /// la share risponde per i primi file e poi smette — ed è anche l'unico
        /// che distingua «arenata» da «morta», perché una morta non consegna
        /// niente e la sonda basterebbe a scoprirla.
        fn walk_a_rate(
            &self,
            root: &str,
            su_file: &mut dyn FnMut(DiscoveredFile) -> ControlFlow<()>,
        ) -> Result<bool, AppError> {
            self.guarda_il_lucchetto();
            for file in self.trovati.get(root).cloned().unwrap_or_default() {
                if su_file(file).is_break() {
                    return Ok(false);
                }
            }
            if self.arenate.contains(root) {
                std::thread::sleep(self.attesa);
            }
            Ok(!self.monche.contains(root))
        }

        fn open(
            &self,
            path: &str,
        ) -> Result<Box<dyn crate::files::ReadSeek + Send + Sync>, AppError> {
            self.aperture
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.guarda_il_lucchetto();
            self.forse_dormi(path);
            if self
                .rete_giu
                .iter()
                .any(|root| path.to_lowercase().starts_with(&root.to_lowercase()))
            {
                return Err(AppError::new(ErrorCode::FsNetworkUnavailable {
                    path: Some(path.to_owned()),
                }));
            }
            Err(AppError::new(ErrorCode::FsNotFound {
                path: path.to_owned(),
            }))
        }

        fn radice_raggiungibile(&self, root: &str) -> bool {
            let prima = self
                .sonde
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if self.morte.contains(root) {
                return false;
            }
            !(self.cade_in_corsa.contains(root) && prima > 0)
        }
    }

    /// Un file trovato, abbastanza grande da non essere scartato come avanzo.
    fn trovato(path: &str, modified_ms: i64) -> DiscoveredFile {
        DiscoveredFile {
            path: path.to_owned(),
            size_bytes: MIN_TRACK_BYTES.saturating_mul(2),
            modified_ms,
        }
    }

    /// Una riga in `tracks` scritta a mano: qui interessa il percorso, non i tag.
    fn riga(connection: &Connection, id: i64, path: &str, modified_ms: i64) {
        connection
            .execute(
                "INSERT INTO tracks
                     (id, path, track_key, title, artist, album, duration_ms,
                      file_size, date_added, date_modified)
                 VALUES (?1, ?2, ?3, 'T', 'A', 'Al', 1000, 99999, 0, ?4)",
                rusqlite::params![id, path, format!("a|t{id}|al"), modified_ms],
            )
            .expect("riga di brano");
    }

    /// Un banco senza disco vero: database in memoria e store delle copertine.
    fn banco() -> (tempfile::TempDir, CoverStore, Connection) {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let store = CoverStore::open(dir.path().join("copertine")).expect("store");
        let connection = crate::db::open_in_memory().expect("database").connection;
        (dir, store, connection)
    }

    #[test]
    fn una_radice_che_non_risponde_non_fa_sparire_i_suoi_brani() {
        let (_dir, store, connection) = banco();
        // Due brani sul NAS, uno sul disco interno.
        riga(&connection, 1, r"\\nas\musica\a.mp3", 10);
        riga(&connection, 2, r"\\nas\musica\b.mp3", 10);
        riga(&connection, 3, r"C:\musica\c.mp3", 10);

        let disco = Arc::new(
            FintoDisco::nuovo()
                .con_file(r"C:\musica", vec![trovato(r"C:\musica\c.mp3", 10)])
                .morta(r"\\nas\musica"),
        );
        let roots = vec![r"\\nas\musica".to_owned(), r"C:\musica".to_owned()];
        let scan = Scan {
            files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
            covers: store.clone(),
            roots: &roots,
            rules: PathRules {
                case_insensitive: true,
            },
            prudente: false,
            scadenze: Scadenze::default(),
            fermati: None,
        };

        let esplorazione = plan(&scan, &connection).expect("piano");

        assert_eq!(
            esplorazione.radici_saltate,
            vec![r"\\nas\musica".to_owned()],
            "la radice morta va dichiarata, non taciuta"
        );
        assert!(
            esplorazione.piano.to_remove.is_empty(),
            "nessun brano del NAS deve risultare sparito: {:?}",
            esplorazione.piano.to_remove
        );
        assert_eq!(
            esplorazione.piano.untouched, 2,
            "i due del NAS sono fuori competenza, non spariti"
        );
        // E l'altra radice si pianifica normalmente: una share giù non deve
        // fermare la scansione del disco interno.
        assert_eq!(esplorazione.piano.unchanged, 1);
    }

    #[test]
    fn una_radice_che_cade_fra_la_sonda_e_la_camminata_si_salta_lo_stesso() {
        // La finestra stretta: la sonda risponde, poi la rete se ne va, e la
        // camminata torna vuota senza dire perché. Senza la risonda quel vuoto
        // varrebbe «la cartella è stata svuotata».
        let (_dir, store, connection) = banco();
        riga(&connection, 1, r"\\nas\musica\a.mp3", 10);

        let disco = Arc::new(FintoDisco::nuovo().cade_dopo_la_sonda(r"\\nas\musica"));
        let roots = vec![r"\\nas\musica".to_owned()];
        let scan = Scan {
            files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
            covers: store.clone(),
            roots: &roots,
            rules: PathRules {
                case_insensitive: true,
            },
            prudente: false,
            scadenze: Scadenze::default(),
            fermati: None,
        };

        let esplorazione = plan(&scan, &connection).expect("piano");
        assert_eq!(
            esplorazione.radici_saltate,
            vec![r"\\nas\musica".to_owned()]
        );
        assert!(esplorazione.piano.to_remove.is_empty());
        assert_eq!(esplorazione.piano.untouched, 1);
    }

    #[test]
    fn una_camminata_a_meta_non_fa_sparire_quel_che_non_ha_visto() {
        // Il caso peggiore dei tre, perché ha l'aria di essere andato bene: la
        // sonda risponde, la camminata **trova roba**, e intanto la share è
        // morta a metà strada. Senza `Camminata::completa` quel che non si è
        // visto diventa «sparito», cioè da cancellare — su una libreria vera
        // sono decine di migliaia di righe, con dentro voti e cronologia.
        let (_dir, store, connection) = banco();
        riga(&connection, 1, r"\\nas\musica\visto.mp3", 10);
        riga(&connection, 2, r"\\nas\musica\non-visto.mp3", 10);
        riga(&connection, 3, r"\\nas\musica\nemmeno-questo.mp3", 10);

        let disco = Arc::new(
            FintoDisco::nuovo()
                // Di tre file ne torna uno: gli altri due stanno nei rami che
                // `walkdir` ha perso quando la rete se n'è andata.
                .con_file(
                    r"\\nas\musica",
                    vec![trovato(r"\\nas\musica\visto.mp3", 10)],
                )
                .cammina_a_meta(r"\\nas\musica")
                .cade_dopo_la_sonda(r"\\nas\musica"),
        );
        let roots = vec![r"\\nas\musica".to_owned()];
        let scan = Scan {
            files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
            covers: store.clone(),
            roots: &roots,
            rules: PathRules {
                case_insensitive: true,
            },
            // Non presidiata **no**: questa è la scansione che l'utente ha
            // chiesto guardando la finestra, quella senza guardia anti-strage.
            // Se la protezione non stesse qui, sotto non ci sarebbe niente.
            prudente: false,
            scadenze: Scadenze::default(),
            fermati: None,
        };

        let esplorazione = plan(&scan, &connection).expect("piano");
        assert_eq!(
            esplorazione.radici_saltate,
            vec![r"\\nas\musica".to_owned()],
            "una radice vista a metà è una radice non vista"
        );
        assert!(
            esplorazione.piano.to_remove.is_empty(),
            "nessuna rimozione da un elenco parziale: {:?}",
            esplorazione.piano.to_remove
        );
        assert_eq!(
            esplorazione.piano.untouched, 3,
            "tutti e tre fuori competenza, compreso quello che si era visto"
        );
    }

    #[test]
    fn una_camminata_a_meta_su_disco_sano_non_ferma_niente() {
        // Il contrappeso, e serve quanto il test qui sopra: una sottocartella a
        // permessi negati sul disco di casa abbassa `completa` esattamente come
        // una share morta. Lì la sonda risponde, e la scansione deve fare quel
        // che ha sempre fatto — altrimenti la protezione diventa «non si
        // cancella più niente», che è un altro modo di avere una libreria
        // sbagliata.
        let (_dir, store, connection) = banco();
        riga(&connection, 1, r"C:\musica\c-e.mp3", 10);
        riga(&connection, 2, r"C:\musica\non-c-e-piu.mp3", 10);

        let disco = Arc::new(
            FintoDisco::nuovo()
                .con_file(r"C:\musica", vec![trovato(r"C:\musica\c-e.mp3", 10)])
                .cammina_a_meta(r"C:\musica"),
        );
        let roots = vec![r"C:\musica".to_owned()];
        let scan = Scan {
            files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
            covers: store.clone(),
            roots: &roots,
            rules: PathRules {
                case_insensitive: true,
            },
            prudente: false,
            scadenze: Scadenze::default(),
            fermati: None,
        };

        let esplorazione = plan(&scan, &connection).expect("piano");
        assert!(esplorazione.radici_saltate.is_empty());
        assert_eq!(
            esplorazione.piano.to_remove.len(),
            1,
            "la radice risponde: il brano che non c'è più va tolto"
        );
    }

    #[test]
    fn una_radice_che_muore_a_meta_lettura_si_abbandona_invece_di_insistere() {
        // La rete se ne va **dopo** che la scansione è cominciata: la sonda
        // iniziale aveva detto sì, la camminata ha trovato i file, e il guasto
        // arriva alla prima apertura. Due cose devono succedere, e nessuna delle
        // due succedeva: non si insiste sugli altri file — su una share vera
        // ognuno costerebbe la scadenza di lettura per intero, e un filo
        // abbandonato a testa — e i brani di quella radice non si cancellano,
        // perché non è stato dimostrato che non ci siano, solo che non si
        // riesce a guardarli.
        let (_dir, store, mut connection) = banco();
        riga(&connection, 1, r"\\nas\musica\sparito-1.mp3", 10);
        riga(&connection, 2, r"\\nas\musica\sparito-2.mp3", 10);

        let disco = Arc::new(
            FintoDisco::nuovo()
                .con_file(
                    r"\\nas\musica",
                    vec![
                        trovato(r"\\nas\musica\nuovo-1.mp3", 10),
                        trovato(r"\\nas\musica\nuovo-2.mp3", 10),
                        trovato(r"\\nas\musica\nuovo-3.mp3", 10),
                    ],
                )
                .senza_rete(r"\\nas\musica")
                // La sonda risponde la prima volta — quella della camminata — e
                // non la seconda, che è quella chiesta dal guasto in lettura.
                .cade_dopo_la_sonda(r"\\nas\musica"),
        );
        let roots = vec![r"\\nas\musica".to_owned()];
        let scan = Scan {
            files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
            covers: store.clone(),
            roots: &roots,
            rules: PathRules {
                case_insensitive: true,
            },
            prudente: false,
            scadenze: Scadenze::default(),
            fermati: None,
        };

        let report = scan
            .run(&mut connection, |_, _| ControlFlow::Continue(()))
            .expect("scansione");

        assert_eq!(
            disco.aperture(),
            1,
            "il primo file paga l'attesa e la scopre per tutti: gli altri due              non vanno nemmeno tentati"
        );
        assert_eq!(
            report.removed, 0,
            "i brani di una radice che non risponde non sono brani spariti"
        );
        assert_eq!(
            report.radici_saltate,
            vec![r"\\nas\musica".to_owned()],
            "e la passata deve dire che non ha guardato lì"
        );
        assert!(
            report.unreadable.is_empty(),
            "un guasto di rete non è un elenco di file illeggibili: {:?}",
            report.unreadable
        );
    }

    #[test]
    fn un_singhiozzo_di_rete_non_fa_abbandonare_la_radice() {
        // Il contrappeso: la rete dà un errore su un file, ma la radice alla
        // sonda risponde ancora. Non è una share morta, è un incidente — e la
        // scansione deve proseguire sugli altri file, elencando quello perso
        // per nome invece di dichiarare morta l'intera cartella.
        let (_dir, store, mut connection) = banco();

        let disco = Arc::new(
            FintoDisco::nuovo()
                .con_file(
                    r"\\nas\musica",
                    vec![
                        trovato(r"\\nas\musica\a.mp3", 10),
                        trovato(r"\\nas\musica\b.mp3", 10),
                    ],
                )
                .senza_rete(r"\\nas\musica"),
        );
        let roots = vec![r"\\nas\musica".to_owned()];
        let scan = Scan {
            files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
            covers: store.clone(),
            roots: &roots,
            rules: PathRules {
                case_insensitive: true,
            },
            prudente: false,
            scadenze: Scadenze::default(),
            fermati: None,
        };

        let report = scan
            .run(&mut connection, |_, _| ControlFlow::Continue(()))
            .expect("scansione");

        assert_eq!(disco.aperture(), 2, "si prova ogni file, non solo il primo");
        assert_eq!(report.unreadable.len(), 2);
        assert!(report.radici_saltate.is_empty());
    }

    #[test]
    fn una_radice_davvero_vuota_resta_una_radice_vuota() {
        // Il contrario del caso sopra, ed è quel che impedisce alla protezione
        // di diventare «non si cancella più niente»: la cartella risponde, ci si
        // è guardato dentro, non c'è nulla. Quei brani vanno tolti.
        let (_dir, store, connection) = banco();
        riga(&connection, 1, r"C:\musica\a.mp3", 10);

        let disco = Arc::new(FintoDisco::nuovo());
        let roots = vec![r"C:\musica".to_owned()];
        let scan = Scan {
            files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
            covers: store.clone(),
            roots: &roots,
            rules: PathRules {
                case_insensitive: true,
            },
            prudente: false,
            scadenze: Scadenze::default(),
            fermati: None,
        };

        let esplorazione = plan(&scan, &connection).expect("piano");
        assert!(esplorazione.radici_saltate.is_empty());
        assert_eq!(esplorazione.piano.to_remove.len(), 1);
    }

    #[test]
    fn una_scansione_non_presidiata_non_toglie_mezza_libreria_da_sola() {
        // La seconda difesa, quella che vale anche per i casi che la sonda non
        // vede: un punto di mount che risponde ed è vuoto, un disco che non si è
        // montato all'avvio. Nessuno sta guardando, quindi nel dubbio si rimanda.
        let (_dir, store, mut connection) = banco();
        for id in 1..=60 {
            riga(&connection, id, &format!(r"C:\musica\{id}.mp3"), 10);
        }

        let disco = Arc::new(FintoDisco::nuovo());
        let roots = vec![r"C:\musica".to_owned()];
        let prudente = Scan {
            files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
            covers: store.clone(),
            roots: &roots,
            rules: PathRules {
                case_insensitive: true,
            },
            prudente: true,
            scadenze: Scadenze::default(),
            fermati: None,
        };

        let esito = prudente
            .run(&mut connection, |_, _| ControlFlow::Continue(()))
            .expect("scansione");
        assert_eq!(esito.removed, 0, "non doveva togliere niente");
        assert_eq!(esito.rimozioni_rinviate, 60);
        let rimasti: i64 = connection
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
            .expect("conteggio");
        assert_eq!(rimasti, 60, "le righe sono ancora tutte lì");

        // …e la stessa passata chiesta a mano toglie eccome: la guardia protegge
        // dall'automatismo, non dall'utente. Chi ha davvero svuotato la cartella
        // e rifà la scansione deve vedere la libreria seguirlo.
        let a_mano = Scan {
            prudente: false,
            ..prudente
        };
        let esito = a_mano
            .run(&mut connection, |_, _| ControlFlow::Continue(()))
            .expect("scansione");
        assert_eq!(esito.removed, 60);
        assert_eq!(esito.rimozioni_rinviate, 0);
    }

    #[test]
    fn la_guardia_non_scatta_sulle_rimozioni_normali() {
        // Cancellare un album non è una strage: sotto la soglia la scansione
        // automatica toglie come ha sempre fatto, altrimenti la protezione
        // diventerebbe una libreria che non si aggiorna più da sola.
        let (_dir, store, mut connection) = banco();
        for id in 1..=100 {
            riga(&connection, id, &format!(r"C:\musica\{id}.mp3"), 10);
        }
        // Novanta restano al loro posto, dieci sono spariti.
        let presenti: Vec<DiscoveredFile> = (1..=90)
            .map(|id| trovato(&format!(r"C:\musica\{id}.mp3"), 10))
            .collect();
        let disco = Arc::new(FintoDisco::nuovo().con_file(r"C:\musica", presenti));
        let roots = vec![r"C:\musica".to_owned()];
        let scan = Scan {
            files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
            covers: store.clone(),
            roots: &roots,
            rules: PathRules {
                case_insensitive: true,
            },
            prudente: true,
            scadenze: Scadenze::default(),
            fermati: None,
        };

        let esito = scan
            .run(&mut connection, |_, _| ControlFlow::Continue(()))
            .expect("scansione");
        assert_eq!(esito.removed, 10);
        assert_eq!(esito.rimozioni_rinviate, 0);
    }
    // ── il tempo, e il lucchetto ────────────────────────────────────────────
    //
    // Le prove qui sopra tengono chiuso il guasto in cui la rete **fallisce**.
    // Queste tengono chiuso quello in cui la rete **aspetta**, che è più
    // insidioso perché non produce nessun errore: la finestra si ferma, Windows
    // dichiara «non risponde», e l'utente termina il processo.

    /// Un deposito che conta le prese e dice quando è in mano a qualcuno.
    ///
    /// Le due cose insieme, perché la domanda è una sola: la scansione chiede la
    /// connessione **abbastanza spesso** da lavorare, e **mai** mentre guarda il
    /// disco.
    struct DepositoContato {
        connection: Connection,
        /// Alzata per tutta la durata di una presa.
        ///
        /// La legge il finto disco: se la trova alzata mentre cammina o mentre
        /// apre un file, qualcuno sta tenendo il lucchetto durante l'I/O — cioè
        /// esattamente il difetto per cui esiste [`Deposito`].
        prestata: Arc<std::sync::atomic::AtomicBool>,
        /// Quante volte la connessione è stata chiesta.
        prestiti: usize,
    }

    impl DepositoContato {
        fn nuovo(connection: Connection) -> Self {
            Self {
                connection,
                prestata: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                prestiti: 0,
            }
        }
    }

    impl Deposito for DepositoContato {
        fn con_connessione<T, F>(&mut self, azione: F) -> Result<T, AppError>
        where
            F: FnOnce(&mut Connection) -> Result<T, AppError>,
        {
            self.prestiti += 1;
            self.prestata
                .store(true, std::sync::atomic::Ordering::SeqCst);
            let esito = azione(&mut self.connection);
            self.prestata
                .store(false, std::sync::atomic::Ordering::SeqCst);
            esito
        }
    }

    /// Un deposito che cambia le carte in tavola fra due prese.
    ///
    /// È il mondo vero, non un dispetto: lasciare la connessione fra una fase e
    /// l'altra significa che un'altra scrittura può passare in mezzo, e questa
    /// prova mette quella scrittura esattamente dove fa più danno.
    struct DepositoDispettoso {
        connection: Connection,
        prestiti: usize,
        /// Dopo la presa numero questa, la riga 1 cambia percorso.
        quando: usize,
    }

    impl Deposito for DepositoDispettoso {
        fn con_connessione<T, F>(&mut self, azione: F) -> Result<T, AppError>
        where
            F: FnOnce(&mut Connection) -> Result<T, AppError>,
        {
            self.prestiti += 1;
            let esito = azione(&mut self.connection);
            if self.prestiti == self.quando {
                self.connection
                    .execute(
                        "UPDATE tracks SET path = ?1 WHERE id = 1",
                        [r"C:\musica\rinominato.mp3"],
                    )
                    .expect("rinomina");
            }
            esito
        }
    }

    /// Scadenze cortissime: una prova non può aspettare quindici secondi.
    fn svelte() -> Scadenze {
        Scadenze {
            camminata: Duration::from_millis(100),
            brano: Duration::from_millis(100),
        }
    }

    #[test]
    fn una_camminata_che_si_arena_non_fa_sparire_i_suoi_brani() {
        // Il guasto nuovo, e il più caro: la share non è spenta — risponde alla
        // sonda — ma la camminata dentro non finisce. Con un `walk` intero la
        // scansione starebbe lì finché Windows non rinuncia da solo, tenendo
        // ferma la finestra; e se qualcuno la interrompesse, i brani non
        // enumerati diventerebbero «spariti».
        let (_dir, store, mut connection) = banco();
        riga(&connection, 1, r"\\nas\musica\a.mp3", 10);
        riga(&connection, 2, r"\\nas\musica\b.mp3", 10);
        riga(&connection, 3, r"\\nas\musica\c.mp3", 10);

        // Nessun file consegnato, e poi il silenzio. La sonda dice sempre di sì:
        // è il punto: la radice **risponde**, quindi risondarla non scoprirebbe
        // niente.
        let disco = Arc::new(FintoDisco::nuovo().si_arena(r"\\nas\musica", Duration::from_secs(5)));
        let roots = vec![r"\\nas\musica".to_owned()];
        let scan = Scan {
            files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
            covers: store.clone(),
            roots: &roots,
            rules: PathRules {
                case_insensitive: true,
            },
            prudente: false,
            scadenze: svelte(),
            fermati: None,
        };

        let prima = std::time::Instant::now();
        let report = scan
            .run(&mut connection, |_, _| ControlFlow::Continue(()))
            .expect("scansione");
        let passato = prima.elapsed();

        assert!(
            passato < Duration::from_secs(1),
            "ha aspettato la fine della camminata: {passato:?}"
        );
        assert_eq!(
            report.radici_saltate,
            vec![r"\\nas\musica".to_owned()],
            "una radice che non finisce di camminare è una radice non vista"
        );
        assert_eq!(
            report.removed, 0,
            "i suoi brani non sono spariti: non si è potuto guardare"
        );
        assert_eq!(
            report.plan.untouched, 3,
            "tutti e tre fuori competenza, non da cancellare"
        );
        let rimasti: i64 = connection
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
            .expect("conteggio");
        assert_eq!(rimasti, 3, "le righe sono ancora tutte lì");
    }

    #[test]
    fn un_brano_che_non_si_apre_in_tempo_vale_una_radice_che_non_risponde() {
        // La share che smette **dopo** la camminata: i file sono stati elencati,
        // e poi le aperture non tornano più. Un'attesa che non finisce non è un
        // errore che qualcuno riporti, quindi va trasformata in uno — e da lì in
        // poi vale quel che vale un guasto di rete: si chiede alla radice come
        // sta, e se non risponde si smette di insistere.
        let (_dir, store, mut connection) = banco();

        let disco = Arc::new(
            FintoDisco::nuovo()
                .con_file(
                    r"\\nas\musica",
                    vec![
                        trovato(r"\\nas\musica\uno.mp3", 10),
                        trovato(r"\\nas\musica\due.mp3", 10),
                        trovato(r"\\nas\musica\tre.mp3", 10),
                    ],
                )
                .apre_lentamente(r"\\nas\musica", Duration::from_secs(5))
                // La sonda risponde alla camminata e non alla domanda che nasce
                // dalla lettura scaduta: la rete se n'è andata in mezzo.
                .cade_dopo_la_sonda(r"\\nas\musica"),
        );
        let roots = vec![r"\\nas\musica".to_owned()];
        let scan = Scan {
            files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
            covers: store.clone(),
            roots: &roots,
            rules: PathRules {
                case_insensitive: true,
            },
            prudente: false,
            scadenze: svelte(),
            fermati: None,
        };

        let report = scan
            .run(&mut connection, |_, _| ControlFlow::Continue(()))
            .expect("scansione");

        assert_eq!(
            disco.aperture(),
            1,
            "il primo file paga l'attesa e la scopre per tutti"
        );
        assert!(
            report.unreadable.is_empty(),
            "una share morta non è un elenco di file illeggibili: {:?}",
            report.unreadable
        );
        assert_eq!(
            report.radici_saltate,
            vec![r"\\nas\musica".to_owned()],
            "e la passata deve dire che non ha guardato lì"
        );
    }

    #[test]
    fn un_brano_lento_su_una_radice_viva_resta_un_file_illeggibile() {
        // Il contrappeso, e serve quanto la prova qui sopra: un file che non si
        // apre in tempo su una cartella che risponde è un incidente di quel
        // file, non una diagnosi sulla cartella. Se la scadenza abbandonasse la
        // radice da sé, un disco lento — una chiavetta USB, un file enorme —
        // farebbe sparire dalla scansione tutto quel che ci sta sopra.
        let (_dir, store, mut connection) = banco();

        let disco = Arc::new(
            FintoDisco::nuovo()
                .con_file(
                    r"\\nas\musica",
                    vec![
                        trovato(r"\\nas\musica\a.mp3", 10),
                        trovato(r"\\nas\musica\b.mp3", 10),
                    ],
                )
                // Lento, ma vivo: la sonda dice sempre di sì.
                .apre_lentamente(r"\\nas\musica", Duration::from_secs(5)),
        );
        let roots = vec![r"\\nas\musica".to_owned()];
        let scan = Scan {
            files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
            covers: store.clone(),
            roots: &roots,
            rules: PathRules {
                case_insensitive: true,
            },
            prudente: false,
            scadenze: svelte(),
            fermati: None,
        };

        let report = scan
            .run(&mut connection, |_, _| ControlFlow::Continue(()))
            .expect("scansione");

        assert_eq!(disco.aperture(), 2, "si prova ogni file, non solo il primo");
        assert_eq!(
            report.unreadable.len(),
            2,
            "due file che non sono arrivati in tempo, detti per nome"
        );
        assert!(
            report.radici_saltate.is_empty(),
            "la cartella risponde: non si abbandona"
        );
    }

    #[test]
    fn tre_scadenze_di_fila_valgono_una_radice_che_non_risponde() {
        // Dove finisce la pazienza della prova qui sopra. Un file lento è un
        // incidente, due sono una coincidenza; tre di fila sulla stessa radice
        // sono una condivisione che non ce la fa, e la sonda non se ne accorge —
        // risponde, e continuerà a rispondere. Senza questa regola il quarto
        // file, e ognuno dei diecimila dopo di lui, costerebbe la scadenza
        // intera più la sonda, e lascerebbe dietro di sé un filo appeso.
        let (_dir, store, mut connection) = banco();

        let disco = Arc::new(
            FintoDisco::nuovo()
                .con_file(
                    r"\\nas\musica",
                    vec![
                        trovato(r"\\nas\musica\a.mp3", 10),
                        trovato(r"\\nas\musica\b.mp3", 10),
                        trovato(r"\\nas\musica\c.mp3", 10),
                        trovato(r"\\nas\musica\d.mp3", 10),
                    ],
                )
                // Viva alla sonda, inutile alle letture: il caso che il
                // contatore esiste per riconoscere.
                .apre_lentamente(r"\\nas\musica", Duration::from_secs(5)),
        );
        let roots = vec![r"\\nas\musica".to_owned()];
        let scan = Scan {
            files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
            covers: store.clone(),
            roots: &roots,
            rules: PathRules {
                case_insensitive: true,
            },
            prudente: false,
            scadenze: svelte(),
            fermati: None,
        };

        let report = scan
            .run(&mut connection, |_, _| ControlFlow::Continue(()))
            .expect("scansione");

        assert_eq!(
            disco.aperture(),
            3,
            "il quarto file non si tenta nemmeno: la radice è già stata dichiarata inutile"
        );
        assert_eq!(
            report.unreadable.len(),
            3,
            "i tre che hanno pagato la scadenza restano detti per nome: {:?}",
            report.unreadable
        );
        assert_eq!(
            report.radici_saltate,
            vec![r"\\nas\musica".to_owned()],
            "e la passata deve dire che di lì in poi non ha guardato"
        );
        assert_eq!(
            report.removed, 0,
            "una radice abbandonata non fa sparire i suoi brani"
        );
    }

    #[test]
    fn la_scansione_non_tiene_la_connessione_mentre_guarda_il_disco() {
        // La prova che riassume tutto il lavoro. Non conta quanto duri una
        // scansione: conta che mentre dura, il lucchetto della libreria sia di
        // chi lo vuole — la finestra che si ridisegna, l'audio, la chiusura
        // dell'applicazione.
        let (_dir, store, connection) = banco();
        // Una riga che sul disco non c'è più: serve a far accadere anche la fase
        // delle rimozioni, così le prese da contare sono tutte.
        riga(&connection, 1, r"C:\musica\sparito.mp3", 10);

        let mut deposito = DepositoContato::nuovo(connection);
        let disco = Arc::new(
            FintoDisco::nuovo()
                .con_file(
                    r"C:\musica",
                    vec![
                        trovato(r"C:\musica\a.mp3", 10),
                        trovato(r"C:\musica\b.mp3", 10),
                    ],
                )
                .sorveglia(&deposito.prestata),
        );
        let roots = vec![r"C:\musica".to_owned()];
        let scan = Scan {
            files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
            covers: store.clone(),
            roots: &roots,
            rules: PathRules {
                case_insensitive: true,
            },
            prudente: false,
            scadenze: Scadenze::default(),
            fermati: None,
        };

        let report = scan
            .run_su(&mut deposito, |_, _| ControlFlow::Continue(()))
            .expect("scansione");

        assert!(
            !disco.sorpresa(),
            "il disco ha lavorato mentre la connessione era prestata"
        );
        // E il contrario della stessa medaglia: la connessione va chiesta e
        // restituita più volte, non presa una volta sola e tenuta. Le prese sono
        // il piano, le doppie e le sparizioni, il lotto, le rimozioni e gli
        // aggregati.
        assert!(
            deposito.prestiti >= 4,
            "la scansione ha preso la connessione {} volte: se fossero una o \
             due, la starebbe tenendo",
            deposito.prestiti
        );
        assert_eq!(report.removed, 1, "e il lavoro l'ha fatto lo stesso");
    }

    #[test]
    fn annullare_durante_la_camminata_non_scrive_niente() {
        // Premere Annulla mentre si enumera un NAS: prima non succedeva niente
        // fino al primo file letto, che su una share lenta poteva non arrivare
        // mai. Adesso la camminata guarda la bandiera a ogni battito — e chi si
        // ferma lì non ha deciso niente, quindi non scrive niente.
        let (_dir, store, connection) = banco();
        riga(&connection, 1, r"C:\musica\a.mp3", 10);

        let mut deposito = DepositoContato::nuovo(connection);
        let disco = Arc::new(FintoDisco::nuovo());
        let roots = vec![r"C:\musica".to_owned()];
        let basta: &dyn Fn() -> bool = &|| true;
        let scan = Scan {
            files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
            covers: store.clone(),
            roots: &roots,
            rules: PathRules {
                case_insensitive: true,
            },
            prudente: false,
            scadenze: svelte(),
            fermati: Some(basta),
        };

        let report = scan
            .run_su(&mut deposito, |_, _| ControlFlow::Continue(()))
            .expect("scansione");

        assert!(report.cancelled, "l'esito deve dire che è stata fermata");
        assert_eq!(
            deposito.prestiti, 0,
            "non si è deciso niente: la connessione non serviva nemmeno"
        );
        assert!(
            report.plan.to_remove.is_empty(),
            "il piano di una camminata interrotta è vuoto, non parziale"
        );
        let rimasti: i64 = deposito
            .connection
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
            .expect("conteggio");
        assert_eq!(rimasti, 1, "la riga è ancora lì");
    }

    #[test]
    fn la_cancellazione_guarda_anche_il_percorso() {
        // Il prezzo di lasciare la connessione fra una fase e l'altra: fra il
        // momento in cui il piano dice «togli la riga 1» e il momento in cui la
        // si toglie, quella riga può essere diventata un'altra cosa. SQLite
        // riusa gli identificativi liberi, quindi il numero da solo non è una
        // prova d'identità: il percorso sì.
        let (_dir, store, connection) = banco();
        riga(&connection, 1, r"C:\musica\a.mp3", 10);

        // Il piano si calcola alla prima presa e le sparizioni alla seconda;
        // subito dopo la seconda, qualcun altro rinomina la riga.
        let mut deposito = DepositoDispettoso {
            connection,
            prestiti: 0,
            quando: 2,
        };
        let disco = Arc::new(FintoDisco::nuovo());
        let roots = vec![r"C:\musica".to_owned()];
        let scan = Scan {
            files: Arc::clone(&disco) as Arc<dyn MusicFiles>,
            covers: store.clone(),
            roots: &roots,
            rules: PathRules {
                case_insensitive: true,
            },
            prudente: false,
            scadenze: Scadenze::default(),
            fermati: None,
        };

        let report = scan
            .run_su(&mut deposito, |_, _| ControlFlow::Continue(()))
            .expect("scansione");

        assert_eq!(
            report.removed, 0,
            "la riga da togliere non è più quella che si era vista"
        );
        let rimasti: i64 = deposito
            .connection
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
            .expect("conteggio");
        assert_eq!(rimasti, 1, "e quella che c'è adesso non si tocca");
    }
}
