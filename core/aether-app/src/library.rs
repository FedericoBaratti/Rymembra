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
//! di contenuto dei file nuovi, che si conosce solo dopo averli letti. Farne un giro a parte
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
use aether_domain::keys::{ContentKey, ContentKeyInput, TrackKey, TrackKeyInput};
use aether_domain::paths::{PathRules, file_stem, is_under};
use aether_domain::scan_plan::{
    DiscoveredFile, KnownTrack, RemoveReason, RemovedIdentity, ScanInput, ScanPlan,
    match_moved_tracks, plan_scan,
};
use rusqlite::{Connection, Row, Transaction, TransactionBehavior};

use crate::covers::{CoverSource, CoverStore, StoredCover};
use crate::files::{MusicFiles, Voce};
use crate::metadata::read_tags;
use crate::scadenza::Mancato;

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
///
/// Tre righe, e delega: la scelta del codice sta in
/// [`crate::db::codice_da_sqlite`], che legge `sqlite_error_code()` e distingue
/// un file occupato da un percorso di rete da un database corrotto. Questo
/// helper è il punto da cui passano alcune centinaia di `map_err` di questo
/// modulo, e **non si toccano**: l'unica cosa che doveva cambiare era cosa
/// risponde questa funzione.
pub(crate) fn db_error(detail: &str, err: &rusqlite::Error) -> AppError {
    crate::db::codice_da_sqlite(detail, err)
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

/// Apre una transazione che prende **subito** il permesso di scrivere.
///
/// # Perché non basta `transaction()`
///
/// `transaction()` apre in modo `DEFERRED`: la transazione comincia come
/// lettura, e diventa scrittura alla prima istruzione che scrive. Finché la
/// connessione è una sola non si vede la differenza. Ma la libreria non è più
/// sola: l'arricchimento e i testi scrivono la loro cache da una seconda
/// connessione, e in WAL una transazione che ha **letto** e poi prova a
/// **scrivere** mentre l'altra ha scritto nel frattempo riceve
/// `SQLITE_BUSY_SNAPSHOT` — e lo riceve subito, senza passare dal
/// `busy_timeout`: aspettare non servirebbe, perché la sua fotografia del
/// database è ormai vecchia e nessuna attesa la rinfresca. Il risultato è una
/// scansione, o una cancellazione, che fallisce per una scrittura che non
/// c'entrava niente.
///
/// `IMMEDIATE` chiede il lucchetto di scrittura all'apertura: se è occupato si
/// aspetta — e lì il `busy_timeout` vale — e una volta presa la transazione non
/// può più trovarsi con una fotografia vecchia fra le mani.
///
/// Serve a ogni transazione che legge e poi scrive. Per una che scrive e basta
/// non cambia niente, e la si può usare lo stesso.
pub(crate) fn transazione_di_scrittura(
    connection: &mut Connection,
) -> rusqlite::Result<Transaction<'_>> {
    connection.transaction_with_behavior(TransactionBehavior::Immediate)
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
    /// L'identità fra dispositivi, calcolata dai valori qui sotto.
    ///
    /// Segue le correzioni dell'utente: `incerti::applica_correzioni` la
    /// ricalcola dai campi corretti, perché è la chiave con cui gli altri
    /// dispositivi devono ritrovare questo brano.
    pub track_key: String,
    /// L'identità di **contenuto**: vedi [`ContentKey`].
    ///
    /// Si calcola dove il file viene letto e da nessun'altra parte, **dai tag
    /// grezzi e non dai campi qui accanto**: quelli portano già i ripieghi, e
    /// un ripiego viene dal nome del file. Nessuna correzione la tocca, ed è
    /// per questo che `scan_plan::match_moved_tracks` riconosce un file
    /// spostato anche di un brano corretto dieci volte — o rinominato, se i
    /// tag non ce li ha.
    pub content_key: String,
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
/// In [`ContentKey`] invece **non entrano**, e la differenza è tutto il punto
/// della colonna. Un ripiego dipende dal nome del file: se lo mettessimo dentro
/// l'identità di contenuto, un file senza tag avrebbe chiave `radice del
/// nome|artista sconosciuto|album sconosciuto`, e rinominarlo — la sola cosa
/// che si può fare per distinguere un file che i tag non ce li ha, e quindi la
/// prima che si fa — lo trasformerebbe in un brano nuovo, con la riga vecchia
/// cancellata e via ascolti, voto e playlist. La chiave di contenuto di quel
/// file è quindi `durata|dimensione`, che un rinomino non tocca.
///
/// Una copertina che non si decodifica non fa fallire il brano: si perde
/// l'immagine, si tiene la musica.
pub fn read_track(
    files: &dyn MusicFiles,
    covers: &CoverStore,
    file: &DiscoveredFile,
) -> Result<TrackRow, AppError> {
    let tags = read_tags(files, &file.path)?;

    let file_size = i64::try_from(file.size_bytes).unwrap_or(i64::MAX);
    let duration_ms = i64::try_from(tags.duration_ms).unwrap_or(0);

    // L'identità di contenuto si calcola **prima dei ripieghi**, ed è per
    // questo che sta sopra e non sotto: le riceve come `Option`, cioè come il
    // file le ha dette. Passarle dopo — quando il titolo è già diventato la
    // radice del nome — legherebbe l'identità di un file senza tag al suo
    // **nome**, e rinominarlo lo duplicherebbe: che è esattamente quel che
    // capita a un file senza tag, perché il nome è l'unica cosa che lo
    // distingue e quindi l'unica che si ha voglia di sistemare.
    let content_key = ContentKey::compute(ContentKeyInput {
        artist: tags.artist.as_deref(),
        title: tags.title.as_deref(),
        album: tags.album.as_deref(),
        duration_ms: Some(duration_ms),
        file_size: Some(file_size),
    })
    .into_string();

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
        content_key,
        title,
        artist,
        album,
        album_artist: tags.album_artist,
        album_key,
        year: tags.year.map(i64::from),
        track_number: tags.track_number.map(i64::from),
        disc_number: tags.disc_number.map(i64::from),
        genre: tags.genre,
        duration_ms,
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
        file_size,
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
///
/// # Le righe senza `content_key` si rileggono
///
/// La migrazione 019 ha lasciato `content_key` a NULL sulle righe che avevano
/// già una correzione, in attesa della «prima riscansione che le rilegge». Ma una
/// riscansione rilegge solo i file cambiati, e un file corretto a mano di solito
/// non lo tocca più nessuno: la chiave restava NULL per sempre. E il trigger
/// della 023, che mette da parte le correzioni di un file che sparisce, le salta
/// proprio quando la chiave manca — cioè perdeva le correzioni più vecchie, che
/// sono quelle che contano di più. Per quelle righe la data qui vale `-1`, che
/// nessun file ha: il piano le vede cambiate e le rilegge una volta, e da lì in
/// poi la data è quella vera.
///
/// La stessa sentinella la mette [`rileggi_tutti_i_tag`], su ogni riga.
///
/// La dimensione viaggia accanto alla data: vedi
/// [`aether_domain::scan_plan::KnownTrack::size_bytes`]. Una dimensione negativa
/// in colonna non è una dimensione, e vale «non si sa».
pub fn known_tracks(connection: &Connection) -> Result<Vec<KnownTrack>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT id, path, CASE WHEN content_key IS NULL THEN -1 ELSE date_modified END,
                    file_size
               FROM tracks WHERE path IS NOT NULL ORDER BY id",
        )
        .map_err(|err| db_error("elenco dei brani noti", &err))?;
    let rows = statement
        .query_map([], |row| {
            Ok(KnownTrack {
                id: row.get(0)?,
                path: row.get(1)?,
                modified_ms: row.get(2)?,
                size_bytes: row
                    .get::<_, Option<i64>>(3)?
                    .and_then(|size| u64::try_from(size).ok()),
            })
        })
        .map_err(|err| db_error("elenco dei brani noti", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("elenco dei brani noti", &err))
}

/// Le chiavi di **contenuto** delle righe date, nello stesso ordine.
///
/// Servono a una cosa sola: chiedere a `match_moved_tracks` se una di queste
/// righe è il file che è appena comparso altrove. La domanda va fatta sulla
/// chiave che non cambia quando l'utente corregge un brano — vedi
/// [`ContentKey`] — altrimenti proprio i brani a cui qualcuno ha dedicato
/// attenzione sono quelli che, spostandosi, si duplicano.
///
/// `COALESCE` perché la migrazione 019 ha lasciato `content_key` a NULL sulle
/// righe che avevano già una sovrascrittura: di quelle i tag grezzi non si
/// conoscono finché non le si rilegge, e fino ad allora vale `track_key`, che è
/// esattamente come si comportavano prima.
///
/// Una riga sparita nel frattempo si salta invece di far fallire tutto: fra il
/// calcolo del piano e questo momento un'altra scrittura può averla tolta, e
/// non è un motivo per rinunciare alla scansione.
fn content_keys_of(connection: &Connection, ids: &[i64]) -> Result<Vec<(i64, String)>, AppError> {
    let mut statement = connection
        .prepare("SELECT COALESCE(content_key, track_key) FROM tracks WHERE id = ?1")
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
///
/// La forma a quattro argomenti prende la copertina da fuori invece che dalla
/// riga: serve a [`update_track`] quando rimette quella dell'arricchimento, che
/// sta in `cover_art` ma non nel file, e quindi non ha uno `StoredCover`.
macro_rules! esegui_coi_campi {
    ($statement:expr, $row:expr, $coda:expr) => {{
        esegui_coi_campi!(
            $statement,
            $row,
            $row.cover.as_ref().map(|c| c.hash.as_str()),
            $coda
        )
    }};
    ($statement:expr, $row:expr, $copertina:expr, $coda:expr) => {{
        let cover_art_hash: Option<&str> = $copertina;
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
            // In coda e non accanto a `track_key`, dove starebbe bene: mettercela
            // lì avrebbe rinumerato a mano tutti i segnaposti che seguono, in
            // due query, ed è il genere di modifica che ne sbaglia uno solo e
            // lo sbaglia in silenzio.
            $row.content_key,
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
               mb_release_group_id, mb_release_id, content_key, date_added,
               source
             ) VALUES (
               ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
               ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26,
               ?27, ?28, ?29, ?30, 'scan'
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
///
/// **Riscrive titolo, interprete e album dai tag del file**, e con essi le tre
/// chiavi. Quel che l'utente ha corretto a mano sta in `track_overrides` e va
/// rimesso sopra subito dopo, con [`crate::incerti::riapplica`]: non lo fa
/// questa funzione perché la scansione è solo uno dei modi in cui una riga viene
/// riscritta dai tag, e la regola è la stessa per tutti — chi riscrive rimette.
///
/// Di quelle tre, `content_key` è l'unica che `riapplica` **non** rimetterà a
/// posto, e non deve: è l'identità del file, non della riga, e questo è il solo
/// punto del programma in cui si scrive. Vedi il `//!` di
/// [`crate::incerti`].
///
/// # Lo strato dell'arricchimento, invece, lo rimette lei
///
/// Prima di scrivere: [`crate::incerti::arricchimento_sopra_i_tag`] guarda,
/// campo per campo, se il file sotto ai campi arricchiti è ancora quello di
/// allora, e restituisce la riga da scrivere con sopra quel che vale ancora.
/// Dentro e non dopo, a differenza della correzione a mano, perché così la riga
/// passa dai tag al risultato con un `UPDATE` solo e `track_key` cambia al più
/// una volta — vedi là il perché.
///
/// Restituisce la `track_key` scritta, che è quella da passare a
/// [`crate::incerti::riapplica_sopra_i_tag`]: la chiave dei tag **più**
/// l'arricchimento rimesso. Passarle quella dei soli tag, com'era prima,
/// riporterebbe un brano arricchito all'identità del suo file un istante dopo
/// averla corretta.
fn update_track(
    tx: &Transaction<'_>,
    track_id: i64,
    row: &TrackRow,
    now: i64,
) -> Result<String, AppError> {
    insert_cover(tx, row, now)?;
    let sopra = crate::incerti::arricchimento_sopra_i_tag(tx, track_id, row)?;
    let (finale, copertina_arricchita) = match sopra.as_ref() {
        Some(sopra) => (&sopra.riga, sopra.copertina.as_deref()),
        None => (row, None),
    };
    let copertina = copertina_arricchita.or_else(|| finale.cover.as_ref().map(|c| c.hash.as_str()));
    let mut statement = tx
        .prepare_cached(
            "UPDATE tracks SET
               path = ?1, title = ?3, artist = ?4, album = ?5,
               -- Non su un brano corretto: vedi `incerti::riapplica_sopra_i_tag`.
               track_key = CASE WHEN EXISTS
                             (SELECT 1 FROM track_overrides WHERE track_id = ?30)
                           THEN track_key ELSE ?2 END,
               album_artist = ?6, album_key = ?7, year = ?8, track_number = ?9,
               disc_number = ?10, genre = ?11, duration_ms = ?12, bpm = ?13,
               musical_key = ?14, comment = ?15, lyrics = ?16,
               cover_art_hash = ?17, bitrate = ?18, sample_rate = ?19,
               channels = ?20, codec = ?21, file_size = ?22, date_modified = ?23,
               replaygain_track_db = ?24, replaygain_album_db = ?25,
               mb_recording_id = ?26, mb_release_group_id = ?27,
               mb_release_id = ?28, content_key = ?29
             WHERE id = ?30",
        )
        .map_err(|err| db_error("aggiornamento di un brano", &err))?;
    esegui_coi_campi!(statement, finale, copertina, track_id)
        .map_err(|err| db_error("aggiornamento di un brano", &err))?;
    Ok(finale.track_key.clone())
}

/// Fa rileggere i tag di ogni file alla prossima scansione.
///
/// # Perché serve, ora che il piano guarda anche la dimensione
///
/// Perché nemmeno data e dimensione insieme vedono tutto. Un taggatore che ferma
/// la data e riscrive dentro lo spazio di riempimento di un ID3v2 lascia il file
/// identico a sé per entrambe, e la scansione non ha modo di accorgersene; e ci
/// sono le righe scritte da una versione che leggeva meno — la normalizzazione
/// NFC dei tag è arrivata con la 3.0, e un brano taggato su un Mac resta
/// introvabile dalla ricerca finché non lo si rilegge. Per questi casi c'è un
/// gesto, e lo fa l'utente dalle Impostazioni.
///
/// # Come
///
/// Con la stessa sentinella che [`known_tracks`] usa per le righe senza
/// `content_key`: `date_modified = -1`, una data che nessun file ha. Il piano
/// vede ogni riga cambiata e la rilegge, e la rilettura riscrive la data vera.
/// Niente bandiere a parte e niente da ricordarsi di spegnere: una scansione
/// interrotta a metà lascia la sentinella solo sulle righe che non ha fatto in
/// tempo a rileggere, che sono esattamente quelle da rileggere la volta dopo.
///
/// La rilettura è quella di sempre, con tutto quel che si porta dietro: le
/// correzioni a mano si rimettono, e l'arricchimento pure, dove il file non
/// l'ha smentito — vedi [`update_track`].
///
/// Restituisce quante righe aspettano la rilettura. I brani di catalogo non
/// hanno un file, e non si contano.
///
/// # E gli album si rifanno da capo
///
/// Chi chiede di rileggere tutto sta dicendo che la libreria in tabella non gli
/// torna, e la risposta giusta comprende gli aggregati: la prossima scansione li
/// ricostruisce per intero invece che per le sole chiavi toccate — vedi
/// [`chiedi_ricostruzione_intera`]. Toccherebbe comunque quasi tutto, ma
/// «quasi» non basta a chi vuole vedere sparire un album fantasma.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn rileggi_tutti_i_tag(connection: &Connection) -> Result<usize, AppError> {
    connection
        .execute(
            "UPDATE tracks SET date_modified = -1
              WHERE path IS NOT NULL AND date_modified <> -1",
            [],
        )
        .map_err(|err| db_error("richiesta di rilettura dei tag", &err))?;
    chiedi_ricostruzione_intera(connection)?;
    connection
        .query_row(
            "SELECT COUNT(*) FROM tracks WHERE path IS NOT NULL AND date_modified = -1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|quante| usize::try_from(quante).unwrap_or(0))
        .map_err(|err| db_error("richiesta di rilettura dei tag", &err))
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
    // `COALESCE(path, fonte_url)`: per un file l'identità è il percorso, per un
    // brano di catalogo è l'indirizzo — in tutti e due i casi la stringa che
    // quella riga non condivide con nessun'altra. Un confronto sul solo `path`
    // qui non toglierebbe **mai** un brano di catalogo, perché in SQL `NULL = ?`
    // non è vero nemmeno quando il parametro è nullo.
    let mut statement = tx
        .prepare_cached("DELETE FROM tracks WHERE id = ?1 AND COALESCE(path, fonte_url) = ?2")
        .map_err(|err| db_error("rimozione di un brano", &err))?;
    let mut removed = 0;
    for (id, path) in righe {
        removed += statement
            .execute(rusqlite::params![id, path])
            .map_err(|err| db_error("rimozione di un brano", &err))?;
    }
    Ok(removed)
}

/// Quel che una cancellazione decisa dall'utente ha portato via.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cancellazione {
    /// Quante righe sono sparite davvero.
    pub tolti: usize,
    /// Quanti di quei file stanno ancora sotto una cartella sorvegliata.
    ///
    /// Cioè: quanti torneranno alla prossima scansione. Non è una diagnostica,
    /// è la cosa da dire a chi ha appena premuto «Togli dalla libreria» e si
    /// aspetta che resti tolto.
    pub torneranno: usize,
}

/// Toglie dalla libreria i brani indicati. **Non tocca i file.**
///
/// # Perché una funzione a parte, e non [`remove_tracks`] reso pubblico
///
/// Perché le due rimozioni non sono la stessa cosa detta due volte. Quella della
/// scansione è una constatazione — «questo file non c'è più su questo disco» —
/// e arriva già dentro la transazione di una passata, in mezzo ad altre sei
/// fasi; questa è una decisione, arriva da sola, e deve portarsi dietro tutto
/// quel che una rimozione isolata richiede: la transazione, gli aggregati, e il
/// conto di quel che tornerà. `remove_tracks` resta la primitiva di tutte e due.
///
/// # Quel che fanno lo schema e i trigger, e non si scrive qui
///
/// Le chiavi esterne sono accese su ogni connessione (vedi `db::configure`), e
/// con la riga se ne vanno in cascata `playlist_tracks`, `play_history`,
/// `enrich_undo`, `track_overrides`, `track_impronta`, `track_intensita`,
/// `brano_vicino` — nei due versi — `brano_vicino_stato`, `settimana_brano` e
/// `track_meta_arricchita`. Il
/// trigger `tracks_fts_delete` toglie il brano dall'indice di ricerca, e
/// `correzioni_restano_orfane` mette da parte le correzioni a mano sotto la
/// `content_key`, dove scadono da sole dopo novanta giorni: se il file viene
/// cancellato davvero non le rivedrà nessuno, e se invece torna con una
/// scansione è giusto che tornino con lui.
///
/// Le tabelle della sincronia non si toccano nemmeno: `sincronia::allinea` le
/// rimette in pari con la libreria all'inizio di ogni passata, ed è pigra
/// apposta perché chi scrive non debba ricordarsene.
///
/// # Niente lapidi di sincronizzazione
///
/// Per la stessa ragione per cui non ne scrive la scansione, detta dall'altro
/// lato: `lapidi.brani` viaggia e si fonde, ma **nessuno la consulta** per
/// impedire il ritorno di un brano — vedi `sincronia::togli_playlist` e
/// `backup::dal_salvataggio`. Una riga di brano non la crea mai una sincronia,
/// la crea la scansione leggendo un file che c'è; se l'altro dispositivo quel
/// file ce l'ha, deve tenerselo. Scriverne una qui sarebbe stato morto.
///
/// # Perché si legge dentro la transazione
///
/// Perché così i percorsi con cui si cancella sono quelli che ci sono **adesso**:
/// la guardia `AND path = ?2` di [`remove_tracks`] esiste per il caso opposto —
/// un piano deciso prima, eseguito dopo, con in mezzo altre scritture — e qui
/// non c'è nessun «prima». Il conto dei tolti coincide quindi con quello delle
/// righe lette, ed è ciò che rende affidabile `torneranno`: si guarda solo quel
/// che è stato tolto davvero.
///
/// # Errori
///
/// `db.queryFailed` se la lettura o la scrittura falliscono.
pub fn cancella_brani(
    connection: &mut Connection,
    ids: &[i64],
    sorvegliate: &[String],
    rules: PathRules,
) -> Result<Cancellazione, AppError> {
    if ids.is_empty() {
        return Ok(Cancellazione::default());
    }
    // `Immediate` perché qui si legge e poi si scrive: vedi
    // [`transazione_di_scrittura`].
    let tx = transazione_di_scrittura(connection)
        .map_err(|err| db_error("cancellazione di brani", &err))?;
    // `Transaction` si deferenzia a `Connection`: la lettura vede quel che la
    // transazione vede, cioè il database un istante prima della cancellazione.
    // Due colonne e non il riepilogo intero: qui serve l'identità della riga,
    // che per un file è il percorso e per un brano di catalogo l'indirizzo.
    //
    // A pezzi di [`PEZZO`], e non con un'unica `IN (…)`: «seleziona tutto» su
    // una libreria di quarantamila brani sono quarantamila parametri, e SQLite
    // ne accetta 32 766 — la cancellazione falliva con un `db.queryFailed` che
    // non diceva niente a nessuno.
    let mut percorsi: Vec<(i64, String, bool)> = Vec::new();
    for pezzo in ids.chunks(PEZZO) {
        let sql = format!(
            "SELECT id, COALESCE(path, fonte_url), path IS NOT NULL
               FROM tracks WHERE id IN ({})",
            segnaposti(pezzo.len())
        );
        let mut statement = tx
            .prepare_cached(&sql)
            .map_err(|err| db_error("cancellazione di brani", &err))?;
        let righe = statement
            .query_map(rusqlite::params_from_iter(pezzo), |riga| {
                Ok((riga.get(0)?, riga.get(1)?, riga.get::<_, i64>(2)? != 0))
            })
            .map_err(|err| db_error("cancellazione di brani", &err))?;
        for riga in righe {
            percorsi.push(riga.map_err(|err| db_error("cancellazione di brani", &err))?);
        }
    }
    if percorsi.is_empty() {
        // Niente da togliere non è un guasto: è il clic su un elenco che è
        // cambiato sotto. Si esce senza rifare gli aggregati, che su una
        // libreria grande costano una lettura intera per niente.
        drop(tx);
        return Ok(Cancellazione::default());
    }
    let coppie: Vec<(i64, &str)> = percorsi
        .iter()
        .map(|(id, identita, _)| (*id, identita.as_str()))
        .collect();
    // Le chiavi **prima** di togliere: dopo, le righe non ci sono più a dire
    // di quale album e di quale artista erano.
    let mut toccati = Toccati::nessuno();
    let quali: Vec<i64> = coppie.iter().map(|(id, _)| *id).collect();
    toccati.segna_brani(&tx, &quali)?;
    let tolti = remove_tracks(&tx, &coppie)?;
    // Gli aggregati nella stessa transazione: un album senza più brani e un
    // artista senza più album devono sparire insieme alle righe, non alla
    // prossima scansione. È il passo 7 di `Scan::run_su`, fatto qui perché qui
    // non c'è una scansione che lo faccia dopo — e fatto solo sugli album e
    // sugli artisti di questi brani.
    ricostruisci_aggregati(&tx, &toccati)?;
    tx.commit()
        .map_err(|err| db_error("cancellazione di brani", &err))?;

    // Solo i file: un brano di catalogo non sta sotto nessuna cartella e
    // nessuna scansione lo riporterà indietro. Contarlo qui vorrebbe dire
    // avvisare «tornerà alla prossima scansione» di qualcosa che non torna.
    let torneranno = percorsi
        .iter()
        .filter(|(_, percorso, e_un_file)| {
            *e_un_file
                && sorvegliate
                    .iter()
                    .any(|radice| is_under(percorso, radice, rules))
        })
        .count();
    Ok(Cancellazione { tolti, torneranno })
}

/// Toglie le rimozioni che una scansione aveva trattenuto, e che qualcuno ha
/// confermato. **Non cammina il disco.**
///
/// # Esattamente quelle
///
/// La conferma arriva su un elenco che la persona ha letto — quante, da quali
/// cartelle — e deve togliere quello: né una riga di più né una di meno.
/// Ricamminare il disco per «rifare la scansione con il permesso» vorrebbe dire
/// togliere un elenco diverso da quello confermato, e magari un elenco che nel
/// frattempo è cresciuto perché la share è caduta del tutto.
///
/// Ogni riga si toglie per identificativo **e** percorso, come in ogni
/// rimozione della scansione: se fra la domanda e la risposta qualcuno ha
/// cambiato quella riga, non la si riconosce e non la si tocca. Il numero
/// restituito dice quante se ne sono tolte davvero.
///
/// # Quel che resta
///
/// Quel che la migrazione 028 mette da parte: ascolti, voto, preferito e posto
/// nelle playlist di ogni riga tolta restano per novanta giorni sotto la
/// `content_key`, e tornano se il file ricompare a una scansione. Una conferma
/// data per sbaglio — «ah, il disco era solo staccato» — costa quindi una
/// riscansione col disco attaccato, non la storia dei brani.
///
/// # Errori
///
/// `db.*` se la scrittura fallisce; in quel caso non si è tolto niente.
pub fn applica_rimozioni(
    connection: &mut Connection,
    elenco: &[RimozioneInAttesa],
) -> Result<usize, AppError> {
    if elenco.is_empty() {
        return Ok(0);
    }
    let tx = transazione_di_scrittura(connection)
        .map_err(|err| db_error("rimozioni confermate", &err))?;
    let coppie: Vec<(i64, &str)> = elenco
        .iter()
        .map(|rimozione| (rimozione.id, rimozione.path.as_str()))
        .collect();
    let mut toccati = Toccati::nessuno();
    let quali: Vec<i64> = coppie.iter().map(|(id, _)| *id).collect();
    toccati.segna_brani(&tx, &quali)?;
    let tolte = remove_tracks(&tx, &coppie)?;
    // Gli aggregati nella stessa transazione, come in `cancella_brani`: qui non
    // c'è una scansione che li rifaccia dopo.
    ricostruisci_aggregati(&tx, &toccati)?;
    tx.commit()
        .map_err(|err| db_error("rimozioni confermate", &err))?;
    Ok(tolte)
}

/// Le rimozioni in attesa, raggruppate per cartella sorvegliata.
///
/// Una voce per radice, nell'ordine delle radici, con il conto e i primi
/// `esempi` percorsi. Una riga che non sta sotto nessuna radice — non dovrebbe
/// succedere: il piano toglie solo quel che sta sotto una radice guardata —
/// finisce in una voce a sé con la radice vuota, invece di sparire dal conto
/// che la persona sta per confermare.
#[must_use]
pub fn riassumi_rimozioni(
    elenco: &[RimozioneInAttesa],
    roots: &[String],
    rules: PathRules,
    esempi: usize,
) -> Vec<RimozioniSottoUnaRadice> {
    let mut voci: Vec<RimozioniSottoUnaRadice> = Vec::new();
    for rimozione in elenco {
        let radice = roots
            .iter()
            .find(|root| is_under(&rimozione.path, root, rules))
            .map_or("", String::as_str);
        let voce = if let Some(posto) = voci.iter().position(|v| v.radice == radice) {
            voci.get_mut(posto)
        } else {
            voci.push(RimozioniSottoUnaRadice {
                radice: radice.to_owned(),
                quante: 0,
                esempi: Vec::new(),
            });
            voci.last_mut()
        };
        if let Some(voce) = voce {
            voce.quante = voce.quante.saturating_add(1);
            if voce.esempi.len() < esempi {
                voce.esempi.push(rimozione.path.clone());
            }
        }
    }
    // Nell'ordine delle radici, e la voce senza radice in fondo: è l'ordine in
    // cui la persona le ha scelte, ed è quello in cui le riconosce.
    voci.sort_by_key(|voce| {
        roots
            .iter()
            .position(|root| *root == voce.radice)
            .unwrap_or(usize::MAX)
    });
    voci
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

/// Ricostruisce `albums` e `artists` dai brani, **tutti**.
///
/// È la ricostruzione intera, e da questa versione è il ripiego: chi ha appena
/// scritto pochi brani passa da [`ricostruisci_aggregati`], che rifà soltanto
/// quel che quei brani nominano e chiama questa quando serve davvero — la
/// prima volta, dopo una migrazione, dopo «Rileggi tutti i tag». Alla fine
/// scrive [`VERSIONE_AGGREGATI`] nel database: è la garanzia su cui la
/// ricostruzione per chiavi si appoggia.
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
///
/// # La transazione
///
/// Legge tutti i brani e poi riscrive due tabelle: la transazione che la ospita
/// va aperta in scrittura fin dall'inizio, con `TransactionBehavior::Immediate`
/// — dentro questa cassa, `transazione_di_scrittura`. Aperta `DEFERRED`, una
/// scrittura dell'arricchimento arrivata fra la lettura e la prima `DELETE` la
/// farebbe fallire con `SQLITE_BUSY_SNAPSHOT`, senza nemmeno aspettare.
pub fn rebuild_aggregates(tx: &Transaction<'_>) -> Result<Aggregates, AppError> {
    #[cfg(test)]
    INTERE_IN_QUESTO_FILO.with(|n| n.set(n.get().saturating_add(1)));
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
    rimappa_le_chiavi(tx, &groups.remap)?;

    tx.execute("DELETE FROM albums", [])
        .map_err(|err| db_error("svuotamento degli album", &err))?;
    scrivi_gli_album(tx, &groups.albums)?;

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

    // Adesso le due tabelle sono esattamente quel che la regola di oggi produce
    // dai brani di oggi: da qui in poi la ricostruzione per chiavi toccate può
    // fidarsi del resto. Vedi [`VERSIONE_AGGREGATI`].
    tx.execute(
        "UPDATE libreria_stato SET aggregati = ?1 WHERE id = 1",
        [VERSIONE_AGGREGATI],
    )
    .map_err(|err| db_error("versione degli aggregati", &err))?;

    Ok(Aggregates {
        albums: groups.albums.len(),
        artists: usize::try_from(artists).unwrap_or(0),
    })
}

// Quante ricostruzioni intere ha fatto il filo corrente: serve alla prova di
// parità per sapere che la strada per chiavi l'ha davvero percorsa, e non è
// ripiegata ogni volta su quella intera. Per filo e non per processo, perché
// le prove girano in parallelo.
#[cfg(test)]
thread_local! {
    static INTERE_IN_QUESTO_FILO: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// La versione della regola con cui si costruiscono `albums` e `artists`.
///
/// # A cosa serve un numero
///
/// La ricostruzione per chiavi toccata — [`ricostruisci_aggregati`] — rifà solo
/// gli album e gli artisti nominati dai brani che una scrittura ha cambiato, e
/// lascia stare il resto. È esatta **a una condizione**: che il resto sia già
/// quel che la regola di adesso produrrebbe. Dopo una ricostruzione intera lo
/// è per definizione, e da lì in poi ogni passata per chiavi lo mantiene; ma
/// una libreria scritta da una versione che raggruppava in un altro modo non lo
/// è, e nessuna passata per chiavi la rimetterebbe in riga — toccherebbe solo
/// gli album che qualcuno ha cambiato.
///
/// Il numero sta nel database, in `libreria_stato.aggregati`, e lo scrive
/// [`rebuild_aggregates`] alla fine di ogni ricostruzione intera. Finché non
/// coincide con questo, [`ricostruisci_aggregati`] ricostruisce per intero. Chi
/// cambia la regola — `aether_domain::album`, o il modo in cui qui si leggono
/// i membri — **alza questo numero**, e la prima scansione dopo l'aggiornamento
/// rimette tutto in riga da sé.
pub const VERSIONE_AGGREGATI: i64 = 1;

/// Oltre quante chiavi d'album toccate conviene rifare tutto.
///
/// Duemila. La ricostruzione per chiavi paga ogni chiave con una lettura per
/// indice e un'ammissione in un elenco `IN`; quella intera paga una lettura di
/// tutta la tabella, una volta. Sopra qualche migliaio di album toccati — la
/// prima scansione, «Rileggi tutti i tag», una cartella enorme appena
/// riorganizzata — la seconda costa meno della prima, e non vale la pena
/// portarsi dietro in memoria un insieme che sta per diventare la libreria.
const TETTO_PER_CHIAVI: usize = 2_000;

/// Quanti valori stanno in un elenco `IN (…)` di una query sola.
///
/// Cinquecento, come [`LOTTO`]: ben sotto i limiti di SQLite sul numero di
/// parametri, e abbastanza perché un elenco di qualche migliaio di chiavi
/// costi una manciata di query invece di una per chiave.
const PEZZO: usize = 500;

/// Gli album e gli artisti che una scrittura ha toccato, **prima e dopo**.
///
/// # Perché prima e dopo
///
/// Perché un brano che cambia album ne tocca due: quello da cui esce, che ha un
/// brano in meno e forse non esiste più, e quello in cui entra. Lo stesso per
/// l'artista. Chi scrive segna i brani **prima** di cambiarli — le chiavi che
/// avevano — e **dopo** — quelle che hanno — con la stessa
/// [`Toccati::segna_brani`]: per una riga tolta la seconda lettura non trova
/// niente, per una inserita la prima, e in tutti e due i casi è giusto così.
///
/// # Perché le chiavi si leggono dal database
///
/// E non si calcolano dai tag appena letti: la chiave d'album di una riga è
/// quella che [`rebuild_aggregates`] ha rimappato, l'artista effettivo è
/// un'espressione SQL ([`ARTISTA_EFFETTIVO`]) con la sua idea di cosa sia uno
/// spazio da togliere, e una correzione a mano rimessa da `incerti` cambia
/// tutte e due dopo la scrittura dei tag. Ricalcolarle in Rust vorrebbe dire
/// una seconda definizione delle stesse due cose; leggerle costa una query per
/// lotto.
///
/// # Quando si smette di contare
///
/// Oltre [`TETTO_PER_CHIAVI`] chiavi d'album l'insieme si butta e resta la
/// bandiera «tutto»: la ricostruzione sarà intera comunque, e tenere in memoria
/// centomila chiavi per poi non usarle non serve a nessuno.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Toccati {
    /// Le chiavi d'album, come stanno in `tracks.album_key`.
    album: std::collections::BTreeSet<String>,
    /// Gli artisti effettivi, come li calcola [`ARTISTA_EFFETTIVO`].
    artisti: std::collections::BTreeSet<String>,
    /// Si è rinunciato a contare: si rifà tutto.
    tutto: bool,
}

impl Toccati {
    /// Nessuna chiave toccata, ancora.
    #[must_use]
    pub fn nessuno() -> Self {
        Self::default()
    }

    /// Tutto toccato: la ricostruzione sarà intera.
    #[must_use]
    pub fn tutti() -> Self {
        Self {
            tutto: true,
            ..Self::default()
        }
    }

    /// Niente da ricostruire.
    #[must_use]
    pub fn e_vuoto(&self) -> bool {
        !self.tutto && self.album.is_empty() && self.artisti.is_empty()
    }

    /// Si rifà tutto, comunque.
    #[must_use]
    pub const fn e_tutto(&self) -> bool {
        self.tutto
    }

    /// Quante chiavi d'album sono segnate.
    #[must_use]
    pub fn quanti_album(&self) -> usize {
        self.album.len()
    }

    /// Aggiunge le chiavi che **adesso** hanno i brani indicati.
    ///
    /// Da chiamare prima di cambiarli e di nuovo dopo: vedi il tipo. Un
    /// identificativo che non c'è non aggiunge niente e non è un errore.
    ///
    /// # Errori
    ///
    /// `db.*` se la lettura fallisce.
    pub fn segna_brani(&mut self, connection: &Connection, ids: &[i64]) -> Result<(), AppError> {
        if self.tutto || ids.is_empty() {
            return Ok(());
        }
        for pezzo in ids.chunks(PEZZO) {
            let sql = format!(
                "SELECT album_key, {ARTISTA_EFFETTIVO} FROM tracks WHERE id IN ({})",
                segnaposti(pezzo.len())
            );
            let mut statement = connection
                .prepare_cached(&sql)
                .map_err(|err| db_error("chiavi dei brani toccati", &err))?;
            let righe = statement
                .query_map(rusqlite::params_from_iter(pezzo), |riga| {
                    Ok((
                        riga.get::<_, Option<String>>(0)?,
                        riga.get::<_, Option<String>>(1)?,
                    ))
                })
                .map_err(|err| db_error("chiavi dei brani toccati", &err))?;
            for riga in righe {
                let (album, artista) =
                    riga.map_err(|err| db_error("chiavi dei brani toccati", &err))?;
                if let Some(album) = album.filter(|a| !a.is_empty()) {
                    self.album.insert(album);
                }
                if let Some(artista) = artista {
                    self.artisti.insert(artista);
                }
            }
            if self.album.len() > TETTO_PER_CHIAVI {
                *self = Self::tutti();
                return Ok(());
            }
        }
        Ok(())
    }

    /// Mette insieme quel che due scritture hanno toccato.
    pub fn unisci(&mut self, altro: Self) {
        if self.tutto || altro.tutto {
            *self = Self::tutti();
            return;
        }
        self.album.extend(altro.album);
        self.artisti.extend(altro.artisti);
        if self.album.len() > TETTO_PER_CHIAVI {
            *self = Self::tutti();
        }
    }
}

/// `?,?,?` per `quanti` parametri. I segnaposto si contano, non si compongono
/// con i valori.
fn segnaposti(quanti: usize) -> String {
    std::iter::repeat_n("?", quanti)
        .collect::<Vec<_>>()
        .join(",")
}

/// `?1,?2,?3` per `quanti` parametri numerati: servono quando lo stesso elenco
/// compare più volte nella stessa query.
fn segnaposti_numerati(quanti: usize) -> String {
    (1..=quanti)
        .map(|n| format!("?{n}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Le righe di `albums` e `artists` si possono rifare per chiavi?
///
/// Sì quando l'ultima ricostruzione intera è stata fatta con la regola di
/// adesso: vedi [`VERSIONE_AGGREGATI`]. Una riga di stato che manca vale «no».
fn aggregati_in_regola(connection: &Connection) -> Result<bool, AppError> {
    use rusqlite::OptionalExtension as _;
    connection
        .query_row(
            "SELECT aggregati FROM libreria_stato WHERE id = 1",
            [],
            |riga| riga.get::<_, i64>(0),
        )
        .optional()
        .map(|versione| versione == Some(VERSIONE_AGGREGATI))
        .map_err(|err| db_error("versione degli aggregati", &err))
}

/// Il segno dello stato di `tracks`: cambia a ogni scrittura, da chiunque venga.
///
/// Lo cambiano tre trigger della migrazione 030, su inserimento, aggiornamento
/// e cancellazione: nessun modulo che scrive deve ricordarsene, ed è il motivo
/// per cui una cache che lo usa come chiave non può restare indietro. Il valore
/// è a caso e non un contatore — la ragione sta nel file della migrazione — e
/// va solo confrontato per uguaglianza: uguale vuol dire «nessuno ha scritto un
/// brano da allora».
///
/// # Prima, non dopo
///
/// Chi lo usa come chiave di una cache lo legge **prima** di calcolare il
/// valore da ricordare. Letto dopo, una scrittura arrivata nel mezzo da
/// un'altra connessione farebbe ricordare il risultato vecchio sotto il segno
/// nuovo, cioè proprio il ritardo che il segno esiste per impedire; letto
/// prima, nello stesso caso si ricorda il risultato nuovo sotto un segno che
/// nessuno rivedrà più, e costa al massimo un calcolo in più.
///
/// # Errori
///
/// `db.*` se la lettura fallisce.
pub fn generazione(connection: &Connection) -> Result<i64, AppError> {
    connection
        .prepare_cached("SELECT generazione FROM libreria_stato WHERE id = 1")
        .and_then(|mut statement| statement.query_row([], |riga| riga.get(0)))
        .map_err(|err| db_error("generazione della libreria", &err))
}

/// Chiede che la prossima ricostruzione sia intera.
///
/// È il ripiego per chi ha cambiato la libreria in un modo che le chiavi
/// toccate non raccontano: «Rileggi tutti i tag», che fa rileggere ogni file.
///
/// # Errori
///
/// `db.*` se la scrittura fallisce.
pub fn chiedi_ricostruzione_intera(connection: &Connection) -> Result<(), AppError> {
    connection
        .execute("UPDATE libreria_stato SET aggregati = 0 WHERE id = 1", [])
        .map(|_| ())
        .map_err(|err| db_error("versione degli aggregati", &err))
}

/// Quanti album e quanti artisti ci sono, senza ricostruire niente.
fn conta_aggregati(connection: &Connection) -> Result<Aggregates, AppError> {
    connection
        .query_row(
            "SELECT (SELECT COUNT(*) FROM albums), (SELECT COUNT(*) FROM artists)",
            [],
            |riga| Ok((riga.get::<_, i64>(0)?, riga.get::<_, i64>(1)?)),
        )
        .map(|(albums, artists)| Aggregates {
            albums: usize::try_from(albums).unwrap_or(0),
            artists: usize::try_from(artists).unwrap_or(0),
        })
        .map_err(|err| db_error("conteggio degli aggregati", &err))
}

/// Rimette in pari `albums` e `artists` dopo una scrittura, rifacendo soltanto
/// quel che la scrittura ha toccato.
///
/// # Quando rifà tutto
///
/// Quando [`Toccati`] lo chiede, quando le chiavi toccate sono più di
/// [`TETTO_PER_CHIAVI`], e quando le tabelle non sono state costruite con la
/// regola di adesso — la prima scansione dopo questa versione, una migrazione,
/// «Rileggi tutti i tag»: vedi [`VERSIONE_AGGREGATI`]. Allora è
/// [`rebuild_aggregates`], identica a prima.
///
/// # Quando non fa niente
///
/// Quando nessuno ha toccato niente: la scansione periodica di una libreria
/// ferma, che fino alla 2.4 rileggeva cinquantamila brani per riscrivere gli
/// stessi album.
///
/// # Perché il risultato è lo stesso della ricostruzione intera
///
/// Gli album si fondono per identificativo — MusicBrainz, Spotify — e un brano
/// nuovo può fondere il suo album con uno che nessuno ha toccato. Non basta
/// quindi rifare le chiavi segnate: si rifà la loro **chiusura**, cioè le
/// chiavi segnate più ogni chiave che condivide con loro un identificativo,
/// finché l'insieme non cresce più. Su quell'insieme la partizione di
/// [`build_album_groups`] è la stessa che darebbe sulla libreria intera — un
/// gruppo fuori non si può fondere con uno dentro senza condividerne un
/// identificativo, e allora sarebbe dentro — e le righe che ne escono sono le
/// stesse, perché ogni scelta del dominio (titolo, artista, copertina, chiave
/// canonica) dipende solo dai membri del gruppo. Una prova in fondo al modulo
/// lo verifica: dopo una serie di modifiche a caso, per chiavi e per intero
/// danno tabelle identiche.
///
/// Per gli artisti è più semplice: un nome è nominato da qualche brano o non lo
/// è, e la risposta cambia solo per i nomi che i brani toccati avevano o hanno.
///
/// # La transazione
///
/// La stessa regola di [`rebuild_aggregates`]: legge e poi scrive, quindi va
/// aperta con [`transazione_di_scrittura`].
///
/// # Errori
///
/// `db.*` se una lettura o una scrittura falliscono.
pub fn ricostruisci_aggregati(
    tx: &Transaction<'_>,
    toccati: &Toccati,
) -> Result<Aggregates, AppError> {
    if toccati.tutto || toccati.album.len() > TETTO_PER_CHIAVI || !aggregati_in_regola(tx)? {
        return rebuild_aggregates(tx);
    }
    if toccati.e_vuoto() {
        return conta_aggregati(tx);
    }
    if !ricostruisci_gli_album(tx, &toccati.album)? {
        // La chiusura è diventata mezza libreria: fusioni a catena per
        // identificativi condivisi. Non si è ancora scritto niente, e da qui la
        // strada intera costa meno.
        return rebuild_aggregates(tx);
    }
    ricostruisci_gli_artisti(tx, &toccati.artisti)?;
    conta_aggregati(tx)
}

/// I membri degli album con queste chiavi, con l'identificativo del brano.
fn membri_delle_chiavi(
    connection: &Connection,
    chiavi: &[&str],
) -> Result<Vec<(i64, AlbumMember)>, AppError> {
    let mut fuori = Vec::new();
    for pezzo in chiavi.chunks(PEZZO) {
        // Le stesse colonne, nello stesso ordine, di `rebuild_aggregates`: è
        // `member_from_row` a leggerle. L'identificativo in coda serve solo a
        // rimettere i membri nell'ordine della lettura intera.
        let sql = format!(
            "SELECT t.album_key, t.album, t.album_artist, t.artist, t.year, t.genre,
                    t.cover_art_hash, c.source, c.width, c.height,
                    t.mb_release_group_id, t.mb_release_id, t.spotify_album_id, t.id
             FROM tracks t
             LEFT JOIN cover_art c ON c.hash = t.cover_art_hash
             WHERE t.album_key IN ({})",
            segnaposti(pezzo.len())
        );
        let mut statement = connection
            .prepare_cached(&sql)
            .map_err(|err| db_error("lettura dei brani per gli album", &err))?;
        let righe = statement
            .query_map(rusqlite::params_from_iter(pezzo), |riga| {
                Ok((riga.get::<_, i64>(13)?, member_from_row(riga)?))
            })
            .map_err(|err| db_error("lettura dei brani per gli album", &err))?;
        for riga in righe {
            fuori.push(riga.map_err(|err| db_error("lettura dei brani per gli album", &err))?);
        }
    }
    Ok(fuori)
}

/// Le chiavi d'album dei brani che portano uno di questi identificativi, o che
/// si chiamano come la chiave canonica che uno di loro produrrebbe.
///
/// È il passo della chiusura in [`ricostruisci_aggregati`]. Si guardano **tutti**
/// gli identificativi dei membri, e non solo quello dominante di ogni gruppo
/// come fa [`build_album_groups`]: l'insieme che ne esce è più largo del
/// necessario, e più largo va bene — un gruppo in più rifatto dà la stessa riga
/// di prima — mentre uno in meno sarebbe una fusione persa.
///
/// `TRIM` sui due lati perché il dominio, prima di confrontare, toglie gli
/// spazi in testa e in coda; una lettura che non lo facesse non vedrebbe due
/// gruppi che invece si fondono.
fn chiavi_che_condividono(
    connection: &Connection,
    identificativi: &[&str],
) -> Result<Vec<String>, AppError> {
    let mut fuori = Vec::new();
    for pezzo in identificativi.chunks(PEZZO) {
        let elenco = segnaposti_numerati(pezzo.len());
        let sql = format!(
            "SELECT DISTINCT album_key FROM tracks
              WHERE album_key IS NOT NULL AND album_key <> ''
                AND (TRIM(mb_release_group_id) IN ({elenco})
                     OR TRIM(mb_release_id) IN ({elenco})
                     OR TRIM(spotify_album_id) IN ({elenco}))"
        );
        let mut statement = connection
            .prepare_cached(&sql)
            .map_err(|err| db_error("album che si fondono", &err))?;
        let righe = statement
            .query_map(rusqlite::params_from_iter(pezzo), |riga| {
                riga.get::<_, String>(0)
            })
            .map_err(|err| db_error("album che si fondono", &err))?;
        for riga in righe {
            fuori.push(riga.map_err(|err| db_error("album che si fondono", &err))?);
        }
        // Le chiavi canoniche che questi identificativi produrrebbero, se ci
        // sono già brani che le portano: una riga rimappata da una
        // ricostruzione di prima, il cui identificativo nel frattempo è
        // cambiato, si fonderebbe per nome e non per identificativo.
        let canoniche: Vec<String> = pezzo
            .iter()
            .flat_map(|id| {
                [
                    format!("mbrg:{id}"),
                    format!("mbr:{id}"),
                    format!("sp:{id}"),
                ]
            })
            .collect();
        for gruppo in canoniche.chunks(PEZZO) {
            let sql = format!(
                "SELECT DISTINCT album_key FROM tracks WHERE album_key IN ({})",
                segnaposti(gruppo.len())
            );
            let mut statement = connection
                .prepare_cached(&sql)
                .map_err(|err| db_error("album che si fondono", &err))?;
            let righe = statement
                .query_map(rusqlite::params_from_iter(gruppo), |riga| {
                    riga.get::<_, String>(0)
                })
                .map_err(|err| db_error("album che si fondono", &err))?;
            for riga in righe {
                fuori.push(riga.map_err(|err| db_error("album che si fondono", &err))?);
            }
        }
    }
    Ok(fuori)
}

/// Rifà le righe di `albums` delle chiavi toccate e di quelle che con loro si
/// fondono. Vedi [`ricostruisci_aggregati`].
///
/// `Ok(false)` vuol dire «ho rinunciato, e non ho scritto niente»: la chiusura
/// ha superato [`TETTO_PER_CHIAVI`] o metà degli album, cioè gli identificativi
/// condivisi legano mezza libreria — un'importazione che ha dato lo stesso
/// gruppo di pubblicazione a cento dischi diversi — e a quel punto la
/// ricostruzione intera costa meno di inseguirli. Chi chiama la fa lui.
fn ricostruisci_gli_album(
    tx: &Transaction<'_>,
    toccate: &std::collections::BTreeSet<String>,
) -> Result<bool, AppError> {
    use std::collections::{BTreeMap, BTreeSet, HashSet};

    // Oltre quante chiavi la chiusura non conviene più: il tetto fisso, o metà
    // degli album che ci sono, se è meno. Una chiusura che abbraccia mezza
    // libreria — identificativi condivisi a catena — costa più della lettura
    // intera che vorrebbe evitare.
    let tetto = TETTO_PER_CHIAVI.min(conta_aggregati(tx)?.albums.div_euclid(2).max(1));

    // ── la chiusura: le chiavi toccate, e chi si fonde con loro ──
    let mut chiavi: BTreeSet<String> = toccate.iter().filter(|k| !k.is_empty()).cloned().collect();
    let mut membri: BTreeMap<i64, AlbumMember> = BTreeMap::new();
    let mut cercati: HashSet<String> = HashSet::new();
    let mut fronte: Vec<String> = chiavi.iter().cloned().collect();
    while !fronte.is_empty() {
        let fronte_ref: Vec<&str> = fronte.iter().map(String::as_str).collect();
        let nuovi = membri_delle_chiavi(tx, &fronte_ref)?;
        let mut identificativi: Vec<String> = Vec::new();
        for (_, membro) in &nuovi {
            for id in [
                membro.mb_release_group_id.as_deref(),
                membro.mb_release_id.as_deref(),
                membro.spotify_album_id.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                let id = id.trim();
                if !id.is_empty() && cercati.insert(id.to_owned()) {
                    identificativi.push(id.to_owned());
                }
            }
        }
        membri.extend(nuovi);
        let identificativi_ref: Vec<&str> = identificativi.iter().map(String::as_str).collect();
        fronte = chiavi_che_condividono(tx, &identificativi_ref)?
            .into_iter()
            .filter(|chiave| chiavi.insert(chiave.clone()))
            .collect();
        if chiavi.len() > tetto {
            return Ok(false);
        }
    }

    // ── la regola del dominio, sugli stessi membri nello stesso ordine ──
    let membri: Vec<AlbumMember> = membri.into_values().collect();
    let groups = build_album_groups(&membri);
    rimappa_le_chiavi(tx, &groups.remap)?;

    // Via le righe di prima — con le chiavi della chiusura — e quelle che
    // portano una chiave canonica che sta per essere riscritta: una riga
    // rimasta indietro con quel nome farebbe fallire l'inserimento.
    let da_togliere: Vec<&str> = chiavi
        .iter()
        .map(String::as_str)
        .chain(groups.albums.iter().map(|album| album.album_key.as_str()))
        .collect::<BTreeSet<&str>>()
        .into_iter()
        .collect();
    for pezzo in da_togliere.chunks(PEZZO) {
        let sql = format!(
            "DELETE FROM albums WHERE album_key IN ({})",
            segnaposti(pezzo.len())
        );
        tx.execute(&sql, rusqlite::params_from_iter(pezzo))
            .map_err(|err| db_error("svuotamento degli album toccati", &err))?;
    }
    scrivi_gli_album(tx, &groups.albums)?;
    Ok(true)
}

/// Riscrive `tracks.album_key` dalle chiavi di base alle canoniche, **tutte
/// insieme**.
///
/// # Il guasto che c'era
///
/// Fino alla 2.4 il rimappaggio era un `UPDATE … WHERE album_key = da` per
/// coppia, una dopo l'altra in ordine alfabetico. Le coppie però possono
/// incatenarsi: un disco che la volta prima era diventato `mbrg:A` e adesso è
/// `mbrg:B`, e nella stessa passata un altro gruppo la cui chiave canonica
/// diventa proprio `mbrg:A`. Applicata prima la seconda coppia, la prima
/// ripescava anche i brani appena arrivati in `mbrg:A` e li portava in
/// `mbrg:B`: brani sotto un album che li conta altrove, e una scheda d'album
/// con un brano in meno e uno fantasma in più. Raro — servono due gruppi che si
/// scambiano un identificativo nella stessa passata — e la prova di parità per
/// chiavi lo ha trovato in due passi.
///
/// # Come
///
/// In due tempi: ogni chiave d'origine va prima in un nome di passaggio che
/// nessuna chiave vera può avere — il carattere di controllo `\u{1}` davanti
/// alla destinazione — e solo dopo, quando nessuna riga porta più una chiave
/// d'origine, i nomi di passaggio diventano le destinazioni. Nessun `UPDATE`
/// può così toccare una riga già rimappata.
fn rimappa_le_chiavi(tx: &Transaction<'_>, remap: &[(String, String)]) -> Result<(), AppError> {
    let cambi: Vec<&(String, String)> = remap.iter().filter(|(da, a)| da != a).collect();
    if cambi.is_empty() {
        return Ok(());
    }
    let mut parcheggia = tx
        .prepare_cached("UPDATE tracks SET album_key = char(1) || ?2 WHERE album_key = ?1")
        .map_err(|err| db_error("rimappaggio delle chiavi d'album", &err))?;
    for (da, a) in &cambi {
        parcheggia
            .execute(rusqlite::params![da, a])
            .map_err(|err| db_error("rimappaggio delle chiavi d'album", &err))?;
    }
    let destinazioni: std::collections::BTreeSet<&str> =
        cambi.iter().map(|(_, a)| a.as_str()).collect();
    let mut arriva = tx
        .prepare_cached("UPDATE tracks SET album_key = ?1 WHERE album_key = char(1) || ?1")
        .map_err(|err| db_error("rimappaggio delle chiavi d'album", &err))?;
    for a in destinazioni {
        arriva
            .execute([a])
            .map_err(|err| db_error("rimappaggio delle chiavi d'album", &err))?;
    }
    Ok(())
}

/// Inserisce le righe di `albums` date. La stessa istruzione per la
/// ricostruzione intera e per quella per chiavi.
fn scrivi_gli_album(tx: &Transaction<'_>, albums: &[AlbumRow]) -> Result<(), AppError> {
    let mut statement = tx
        .prepare_cached(
            "INSERT INTO albums (album_key, title, artist, year, genre, total_tracks,
                                 cover_art_hash, mb_album_id, spotify_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .map_err(|err| db_error("scrittura di un album", &err))?;
    for album in albums {
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
    Ok(())
}

/// Rifà la presenza in `artists` dei nomi toccati: c'è chi è ancora nominato da
/// un brano, se ne va chi non lo è più. Vedi [`ricostruisci_aggregati`].
///
/// Le righe che restano non si riscrivono: `INSERT OR IGNORE`, come nella
/// ricostruzione intera, perché `bio`, `image_hash` e gli identificativi
/// esterni arrivano dall'arricchimento e non dai file.
fn ricostruisci_gli_artisti(
    tx: &Transaction<'_>,
    toccati: &std::collections::BTreeSet<String>,
) -> Result<(), AppError> {
    let nomi: Vec<&str> = toccati.iter().map(String::as_str).collect();
    let mut presenti: std::collections::HashSet<String> = std::collections::HashSet::new();
    for pezzo in nomi.chunks(PEZZO) {
        // Scritta con [`ARTISTA_EFFETTIVO`] e non a mano: è l'espressione su
        // cui la migrazione 030 ha messo l'indice, e SQLite lo usa solo per una
        // domanda scritta identica.
        let sql = format!(
            "SELECT DISTINCT {ARTISTA_EFFETTIVO} FROM tracks
              WHERE {ARTISTA_EFFETTIVO} IN ({})",
            segnaposti(pezzo.len())
        );
        let mut statement = tx
            .prepare_cached(&sql)
            .map_err(|err| db_error("artisti ancora nominati", &err))?;
        let righe = statement
            .query_map(rusqlite::params_from_iter(pezzo), |riga| {
                riga.get::<_, String>(0)
            })
            .map_err(|err| db_error("artisti ancora nominati", &err))?;
        for riga in righe {
            presenti.insert(riga.map_err(|err| db_error("artisti ancora nominati", &err))?);
        }
    }
    let mut inserisci = tx
        .prepare_cached("INSERT OR IGNORE INTO artists (name) VALUES (?1)")
        .map_err(|err| db_error("ricostruzione degli artisti", &err))?;
    let mut togli = tx
        .prepare_cached("DELETE FROM artists WHERE name = ?1")
        .map_err(|err| db_error("ricostruzione degli artisti", &err))?;
    for nome in nomi {
        if presenti.contains(nome) {
            // Il nome vuoto non entra, come nella ricostruzione intera.
            if !nome.is_empty() {
                inserisci
                    .execute([nome])
                    .map_err(|err| db_error("ricostruzione degli artisti", &err))?;
            }
        } else {
            togli
                .execute([nome])
                .map_err(|err| db_error("ricostruzione degli artisti", &err))?;
        }
    }
    Ok(())
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
    /// Diverso da zero quando una passata toglierebbe troppo in una volta:
    /// vedi [`Scan::prudente`] e [`ScanReport::rimozioni_in_attesa`].
    pub rimozioni_rinviate: usize,
    /// Le stesse righe, una per una, per chi può chiedere un sì.
    ///
    /// Piena solo nelle scansioni **presidiate** (`prudente` spento), dove
    /// qualcuno sta guardando e può confermare: la finestra le tiene da parte,
    /// chiede, e se la risposta è sì le passa a [`applica_rimozioni`] — che
    /// toglie **esattamente** queste, senza ricamminare il disco. Vuota nelle
    /// altre, dove nessuno potrebbe rispondere e tenerne l'elenco sarebbe
    /// memoria per niente.
    pub rimozioni_in_attesa: Vec<RimozioneInAttesa>,
    /// Le cartelle che non si sono potute leggere, dentro radici che invece
    /// rispondevano.
    ///
    /// Diverse da [`ScanReport::radici_saltate`]: lì è una cartella sorvegliata
    /// intera a non aver risposto, qui è un ramo dentro una che rispondeva — una
    /// sottocartella a permessi negati, un pezzo di share che singhiozza. I
    /// brani che ci stanno sotto non sono stati giudicati: vedi
    /// [`ScanReport::non_verificati`].
    pub rami_persi: Vec<String>,
    /// Righe che il piano avrebbe dato per sparite e che stanno sotto un ramo
    /// perso: non verificate, e quindi lasciate stare.
    ///
    /// Contate anche fra le `untouched` del piano, che è quel che sono: righe
    /// su cui questa passata non ha competenza.
    pub non_verificati: usize,
}

/// Una riga che la scansione toglierebbe, e che aspetta un sì.
///
/// Identificativo **e** percorso, per la stessa ragione di `remove_tracks`: fra
/// la scansione che l'ha trattenuta e la conferma passa del tempo — quello che
/// serve a una persona per leggere una finestrella — e in quel tempo la riga
/// con quel numero può essere diventata un'altra.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RimozioneInAttesa {
    /// La riga.
    pub id: i64,
    /// Il percorso che aveva quando la si è giudicata sparita.
    pub path: String,
}

/// Le rimozioni in attesa che stanno sotto una stessa radice, in breve.
///
/// È quel che la finestra mostra prima di chiedere: quante, e qualche esempio
/// per riconoscerle. L'elenco intero resta a chi lo applicherà — mandarlo tutto
/// vorrebbe dire spedire migliaia di percorsi per una lista che nessuno scorre.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RimozioniSottoUnaRadice {
    /// La cartella sorvegliata.
    pub radice: String,
    /// Quante righe se ne andrebbero da lì.
    pub quante: usize,
    /// I primi percorsi, nell'ordine del piano.
    pub esempi: Vec<String>,
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
    /// Le cartelle dentro radici vive che non si sono potute leggere: vedi
    /// [`ScanReport::rami_persi`].
    pub rami_persi: Vec<String>,
    /// Le righe risparmiate perché stanno sotto uno di quei rami: vedi
    /// [`ScanReport::non_verificati`].
    pub non_verificati: usize,
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
    /// Nessuno sta guardando: chi riceve l'esito non può confermare niente.
    ///
    /// # La guardia vale per tutte e due
    ///
    /// Una passata che toglierebbe una quota enorme di brani — più di
    /// `SOGLIA_RIMOZIONI` e più di un `FRAZIONE_SOSPETTA`-esimo della
    /// libreria — **non toglie niente**, prudente o no, e lo dichiara in
    /// [`ScanReport::rimozioni_rinviate`]. Non è una regola di dominio nascosta
    /// qui dentro: è la stessa cautela di `cancelled`, cioè «nel dubbio, non
    /// distruggere».
    ///
    /// Fino alla 2.4 valeva solo per le prudenti, perché una scansione a mano
    /// «l'ha chiesta qualcuno che vede l'esito». Lo vede, sì — dopo: l'esito
    /// diceva «1.340 tolti» quando i 1.340 se n'erano già andati, e con loro in
    /// cascata cronologia, playlist, voti e preferiti. Vedere un danno non è
    /// poterlo evitare.
    ///
    /// # Cosa cambia fra le due
    ///
    /// Quel che succede **dopo**. Una scansione a mano (`prudente` spento) ha
    /// qualcuno davanti, e le righe trattenute tornano una per una in
    /// [`ScanReport::rimozioni_in_attesa`]: la finestra chiede, e un sì le
    /// toglie con [`applica_rimozioni`] senza ricamminare il disco. Una
    /// scansione automatica — quella che la coda dei download fa quando ha
    /// finito — non la vede nessuno: le righe trattenute restano un numero, e
    /// la passata dopo ci ripensa.
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
        /// I rami persi, per nome: vedi [`Voce::RamoPerso`].
        rami: Vec<String>,
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
    /// Una cartella dentro cui non si è potuto guardare.
    RamoPerso(String),
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
        let mut consegna = |voce| {
            let passo = match voce {
                Voce::File(file) => Passo::Trovato(file),
                Voce::RamoPerso(ramo) => Passo::RamoPerso(ramo),
            };
            if manda.send(passo).is_ok() {
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
    let mut rami = Vec::new();
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
            // Anche un ramo perso è una consegna: il disco ha risposto, con un
            // rifiuto. Il silenzio che si misura è quello di chi non risponde.
            Ok(Passo::RamoPerso(ramo)) => {
                rami.push(ramo);
                ultima_consegna = Instant::now();
                margine = scan.scadenze.camminata;
            }
            Ok(Passo::Fine { completa }) => {
                return Passeggiata::Vista {
                    file,
                    completa,
                    rami,
                };
            }
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
    /// Le cartelle illeggibili dentro radici che hanno risposto.
    ///
    /// La radice sta fra le [`Ricognizione::viste`] e si giudica, **tranne**
    /// quel che sta sotto uno di questi rami: lì non si è guardato, e una riga
    /// che non si è vista non è una riga sparita. Vedi [`Voce::RamoPerso`].
    pub rami_persi: Vec<String>,
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
            Passeggiata::Vista {
                file,
                completa,
                mut rami,
            } => {
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
                // La sonda resta il giudice della radice, e non `completa` da
                // sé: una sottocartella a permessi negati sul disco di casa
                // abbassa `completa` esattamente come una share morta, ma
                // risponde alla sonda — e lì la scansione deve proseguire.
                if (!completa || file.is_empty()) && !scan.files.radice_raggiungibile(root) {
                    esito.radici_saltate.push(root.clone());
                    continue;
                }
                // Prosegue, ma **non come ha sempre fatto**. Fino alla 2.4 una
                // radice viva con un ramo perso si giudicava per intero, e ogni
                // brano sotto la sottocartella illeggibile risultava sparito:
                // una scansione a mano lo toglieva, con cronologia, playlist,
                // voti e preferiti. Adesso i rami persi viaggiano per nome, e
                // quel che ci sta sotto resta non verificato — vedi
                // `risparmia_i_non_verificati`.
                //
                // Una camminata che si dice monca senza aver detto dove ha
                // perso qualcosa ha rotto il patto di `walk_a_rate`: la si
                // prende in parola sulla sola cosa che ha detto, e il ramo
                // perso è la radice intera. Costa una passata senza rimozioni
                // su quella radice, che è il prezzo giusto di non sapere.
                if !completa && rami.is_empty() {
                    rami.push(root.clone());
                }
                esito.rami_persi.extend(rami);
                esito.trovati.extend(file);
                esito.viste.push(root.clone());
            }
        }
    }
    esito
}

/// Le cartelle da riscansionare dopo che sotto le radici è cambiato qualcosa.
///
/// # A chi serve
///
/// Alla sorveglianza delle cartelle sul desktop: il sistema dice «questo file è
/// comparso», «quella cartella è stata rinominata», e da lì si vuole
/// riscansionare **il meno possibile** — un album, non la libreria. Una
/// scansione con un sottoalbero come radice fa già esattamente quel che serve:
/// giudica solo quel che sta sotto, e quel che sta fuori resta «fuori
/// competenza» (`ScanPlan::untouched`), cioè non può sparire. È lo stesso
/// meccanismo che tiene al sicuro il resto della libreria quando si scansiona
/// la sola cartella dei download, e non serve un altro modo di scansionare.
///
/// # Quale cartella, per ogni cambiamento
///
/// **Quella che lo contiene**, e non il percorso stesso: un file tolto non c'è
/// più, e una cartella rinominata ha due nomi, di cui uno non esiste più. Il
/// genitore c'è — di solito — e camminandolo si vede tanto quel che è arrivato
/// quanto quel che se n'è andato.
///
/// Di solito, perché anche il genitore può essere sparito: si cancella un
/// artista intero, e il sistema racconta prima i file dei suoi album. Allora si
/// risale finché si trova una cartella che `esiste` — al più la radice stessa.
/// Non è pignoleria: una scansione su una cartella che non esiste più la salta
/// come una radice che non risponde, cioè **non toglie niente** (vedi
/// [`cammina`]), e i brani cancellati resterebbero in libreria per sempre.
///
/// Un percorso fuori da tutte le radici non produce niente; la radice stessa
/// cambiata produce la radice.
///
/// # Il minimo indispensabile
///
/// Cartelle una dentro l'altra si riducono alla più esterna: camminarla
/// comprende già l'altra, e due passate sulla stessa musica sono due letture
/// della stessa cartella. L'ordine dell'uscita è quello delle chiavi di
/// percorso, così due chiamate con gli stessi ingressi danno la stessa
/// scansione.
///
/// `esiste` è una domanda al disco passata da fuori: questa funzione resta
/// pura, e chi la chiama sa se il disco è locale — dove chiederlo non costa
/// niente — o no.
#[must_use]
pub fn cartelle_da_riguardare(
    cambiati: &[String],
    radici: &[String],
    rules: PathRules,
    esiste: &dyn Fn(&str) -> bool,
) -> Vec<String> {
    let mut candidate: Vec<String> = Vec::new();
    for cambiato in cambiati {
        let Some(radice) = radici.iter().find(|r| is_under(cambiato, r, rules)) else {
            continue;
        };
        let stessa = |a: &str, b: &str| {
            aether_domain::paths::path_key(a, rules) == aether_domain::paths::path_key(b, rules)
        };
        let mut cartella = if stessa(cambiato, radice) {
            radice.clone()
        } else {
            cartella_che_contiene(cambiato).unwrap_or_else(|| radice.clone())
        };
        // Si risale finché la cartella c'è, senza mai uscire dalla radice.
        while !stessa(&cartella, radice) {
            if !is_under(&cartella, radice, rules) {
                cartella.clone_from(radice);
                break;
            }
            if esiste(&cartella) {
                break;
            }
            cartella = cartella_che_contiene(&cartella).unwrap_or_else(|| radice.clone());
        }
        // La radice si scrive come l'ha scritta chi l'ha aggiunta: è con quella
        // grafia che il piano confronta le maiuscole di quel che sta sotto.
        if stessa(&cartella, radice) {
            cartella.clone_from(radice);
        }
        candidate.push(cartella);
    }
    piu_esterne(candidate, rules)
}

/// Le parti di `cartella` che stanno sotto una radice sorvegliata.
///
/// La cartella stessa se sta dentro una radice; le radici che le stanno dentro
/// se è lei a contenerne; niente se le due non si incontrano. Serve alla
/// scansione dopo i download: limitata alla cartella dove i file sono arrivati,
/// ma mai fuori dalle radici — un file che nessuna scansione completa
/// troverebbe non deve entrare in libreria per la porta di servizio.
#[must_use]
pub fn dentro_le_radici(cartella: &str, radici: &[String], rules: PathRules) -> Vec<String> {
    if radici.iter().any(|r| is_under(cartella, r, rules)) {
        return vec![cartella.to_owned()];
    }
    let dentro: Vec<String> = radici
        .iter()
        .filter(|r| is_under(r, cartella, rules))
        .cloned()
        .collect();
    piu_esterne(dentro, rules)
}

/// La cartella che contiene un percorso, per com'è scritto: tutto quel che sta
/// prima dell'ultimo separatore. `None` per un percorso senza separatori.
///
/// Sul testo e non con `Path::parent`: questo modulo gira anche dove i
/// separatori del sistema non sono quelli di Windows, e i percorsi della
/// libreria sono stringhe scritte dalla camminata.
fn cartella_che_contiene(percorso: &str) -> Option<String> {
    let senza_coda = percorso.trim_end_matches(['\\', '/']);
    let taglio = senza_coda.rfind(['\\', '/'])?;
    let testa = senza_coda.get(..taglio)?;
    // `C:\a` → `C:\`, non `C:`: la radice di un'unità ha il suo separatore.
    if testa.ends_with(':') {
        return senza_coda.get(..=taglio).map(str::to_owned);
    }
    (!testa.is_empty()).then(|| testa.to_owned())
}

/// Toglie i doppioni e le cartelle che stanno dentro un'altra dell'elenco.
///
/// Le cartelle restano scritte come sono arrivate, in ordine di chiave di
/// percorso. Pubblica perché chi mette insieme cartelle da più strade — gli
/// avvisi del sistema e il sondaggio delle cartelle di rete — deve ridurle
/// allo stesso modo prima di farne le radici di una scansione.
#[must_use]
pub fn piu_esterne(mut cartelle: Vec<String>, rules: PathRules) -> Vec<String> {
    use aether_domain::paths::path_key;
    cartelle.sort_by_key(|c| path_key(c, rules));
    let mut tenute: Vec<String> = Vec::new();
    for cartella in cartelle {
        if !tenute.iter().any(|t| is_under(&cartella, t, rules)) {
            tenute.push(cartella);
        }
    }
    tenute
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
            rami_persi: ricognizione.rami_persi,
            non_verificati: 0,
        });
    }
    let known = known_tracks(connection)?;
    let mut piano = plan_scan(
        ScanInput {
            roots: &ricognizione.viste,
            found: &ricognizione.trovati,
            known: &known,
        },
        scan.rules,
    );
    let non_verificati =
        risparmia_i_non_verificati(&mut piano, &ricognizione.rami_persi, scan.rules);
    Ok(Esplorazione {
        piano,
        radici_saltate: ricognizione.radici_saltate,
        fermata: false,
        rami_persi: ricognizione.rami_persi,
        non_verificati,
    })
}

/// Toglie dal piano le rimozioni che stanno sotto un ramo perso.
///
/// # Perché qui e non nel dominio
///
/// Perché il dominio decide su quel che gli si dà — radici, file trovati, righe
/// note — e un ramo perso non è nessuna delle tre: è un pezzo di radice su cui
/// la camminata non ha potuto dire niente. Il piano lo vede come «nessun file
/// trovato là sotto», che per lui è la stessa cosa di «là sotto non c'è più
/// niente». Correggerlo qui, dopo, tiene il dominio puro e la regola accanto
/// all'unico posto che sa dei rami.
///
/// # Cosa si risparmia, e cosa no
///
/// Solo le righe [`RemoveReason::Disappeared`] sotto un ramo perso: sono le
/// sole che il piano ha giudicato su un'assenza, e l'assenza sotto un ramo che
/// non si è letto non prova niente. Le righe doppie
/// ([`RemoveReason::DuplicateRow`]) restano da togliere: sono un fatto del
/// database, non del disco. E niente si aggiunge agli aggiornamenti: un file
/// sotto un ramo perso non è arrivato, quindi non c'è niente da rileggere — le
/// sue righe restano come sono, **né tolte né riscritte**.
///
/// Le righe risparmiate passano fra le `untouched`, che è quel che sono: fuori
/// dalla competenza di questa passata. Tenerle nel conto invece di farle
/// sparire serve anche alla guardia contro le stragi, che somma tutte le righe
/// conosciute per sapere quanto pesa una rimozione.
///
/// Restituisce quante righe ha risparmiato.
fn risparmia_i_non_verificati(piano: &mut ScanPlan, rami: &[String], rules: PathRules) -> usize {
    if rami.is_empty() {
        return 0;
    }
    let prima = piano.to_remove.len();
    piano.to_remove.retain(|rimozione| {
        rimozione.reason != RemoveReason::Disappeared
            || !rami
                .iter()
                .any(|ramo| is_under(&rimozione.track.path, ramo, rules))
    });
    let risparmiate = prima.saturating_sub(piano.to_remove.len());
    piano.untouched = piano.untouched.saturating_add(risparmiate);
    risparmiate
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

/// Un file che ha fatto cadere il lettore dei tag.
///
/// `metadata.tagReadFailed`, cioè la stessa cosa che si dice di un file i cui
/// tag non si leggono: per chi guarda l'esito è esattamente quello, un file
/// storto da ritaggare o da buttare. **Non** `FsNetworkUnavailable` come una
/// scadenza, e la differenza è tutto il punto: quel codice fa sondare la radice
/// e conta verso l'abbandono, mentre un file malformato dice che il disco ha
/// consegnato benissimo — è il contenuto a essere sbagliato. Vedi [`Mancato`].
fn caduto(path: &str, messaggio: Option<&str>) -> AppError {
    let causa = messaggio.map_or_else(
        || "il lettore dei tag è caduto su questo file".to_owned(),
        |testo| format!("il lettore dei tag è caduto su questo file: {testo}"),
    );
    AppError::new(ErrorCode::MetadataTagReadFailed {
        path: Some(path.to_owned()),
    })
    .with_cause(causa)
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
    /// 7. **Gli aggregati.** Una presa, e solo per gli album e gli artisti che
    ///    le fasi 3, 5 e 6 hanno toccato: vedi [`ricostruisci_aggregati`].
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
    /// - **Una riga può essere sparita.** Non è un errore: `content_keys_of` la
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
                rami_persi: ricognizione.rami_persi,
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
        // ── 2 bis. quel che sta sotto un ramo perso non è sparito ──
        //
        // Prima di tutto il resto, perché da qui in poi il piano decide: le
        // sparizioni diventano candidate agli spostamenti e poi rimozioni. Una
        // riga sotto una sottocartella illeggibile non deve arrivare a nessuna
        // delle due — non è stata vista, e non vederla non vuol dire niente.
        report.non_verificati =
            risparmia_i_non_verificati(&mut report.plan, &ricognizione.rami_persi, self.rules);
        report.rami_persi = ricognizione.rami_persi;

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
        // Gli album e gli artisti che questa passata tocca, fase per fase: alla
        // fine si rifanno quelli e basta. Vedi [`ricostruisci_aggregati`].
        let mut toccati = Toccati::nessuno();
        let (tolte, mut sospese) = deposito.con_connessione(|connection| {
            let mut tolte = 0;
            if !doppie.is_empty() {
                let tx = transazione_di_scrittura(connection)
                    .map_err(|err| db_error("apertura della transazione", &err))?;
                let elenco: Vec<(i64, &str)> = doppie
                    .iter()
                    .map(|(id, path)| (*id, path.as_str()))
                    .collect();
                let quali: Vec<i64> = doppie.iter().map(|(id, _)| *id).collect();
                toccati.segna_brani(&tx, &quali)?;
                tolte = remove_tracks(&tx, &elenco)?;
                tx.commit()
                    .map_err(|err| db_error("chiusura della transazione", &err))?;
            }
            let sospese = content_keys_of(connection, &spariti)?;
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
                        Ok(Ok(mut row)) => {
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
                        Ok(Err(error)) => self.file_illeggibile(
                            file,
                            error,
                            &mut abbandonate,
                            &mut report.unreadable,
                        ),
                        // Il lettore dei tag è caduto su questo file: `lofty`
                        // davanti a un'intestazione malformata, un `assert!`
                        // di terzi. Un file illeggibile, detto per nome, e
                        // nient'altro: **non** una scadenza, e non conta verso
                        // l'abbandono della radice. Fino alla 2.4 lo era, e tre
                        // file rotti di fila in una cartella del disco di casa
                        // la facevano abbandonare come «rete non disponibile».
                        //
                        // Anzi: un file che fa cadere il lettore è un file che
                        // è **arrivato**, byte per byte. La radice ha risposto,
                        // e la serie di scadenze — se ce n'era una — si
                        // interrompe come dopo una lettura riuscita.
                        Err(Mancato::Caduto { messaggio }) => {
                            if let Some(root) = self.radice_di(&file.path) {
                                scadenze_di_fila.remove(root);
                            }
                            report.unreadable.push(Unreadable {
                                path: file.path.clone(),
                                error: caduto(&file.path, messaggio.as_deref()),
                            });
                        }
                        // Non è tornato in tempo: il filo è ancora dentro la
                        // `open`, e da qui in poi non lo si aspetta più. Vale un
                        // guasto di rete, e come tale fa scattare la sonda della
                        // radice — perché la domanda è la stessa: è lento questo
                        // file, o è morta la cartella?
                        Err(Mancato::Scaduto) => {
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
                    .map(|(row, _)| row.content_key.as_str())
                    .collect();
                let identita: Vec<RemovedIdentity<'_>> = sospese
                    .iter()
                    .map(|(track_id, content_key)| RemovedIdentity {
                        track_id: *track_id,
                        content_key: content_key.as_str(),
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
            //
            // In scrittura dall'inizio: `riapplica` legge prima di scrivere, e
            // così i trigger delle orfane. Vedi `transazione_di_scrittura`.
            let now = now_ms();
            let scritte = deposito.con_connessione(|connection| {
                let tx = transazione_di_scrittura(connection)
                    .map_err(|err| db_error("apertura della transazione", &err))?;
                let mut inserite = 0_usize;
                let mut aggiornate = 0_usize;
                let mut spostate = 0_usize;
                // Le righe che si riscrivono, segnate **prima**: l'album e
                // l'artista da cui un brano ritaggato o spostato esce.
                let mut scritte_qui: Vec<i64> = righe
                    .iter()
                    .filter_map(|(_, destinazione)| match *destinazione {
                        Destinazione::Aggiorna(id) | Destinazione::Sposta(id) => Some(id),
                        Destinazione::Nuova => None,
                    })
                    .collect();
                toccati.segna_brani(&tx, &scritte_qui)?;
                for (row, destinazione) in &righe {
                    match *destinazione {
                        Destinazione::Nuova => {
                            insert_track(&tx, row, now)?;
                            // `riapplica` anche qui, da quando una correzione
                            // può precedere la riga: un file tolto dalla
                            // libreria e tornato — la cartella rimessa, il
                            // disco ricollegato — ritrova le sue correzioni per
                            // `content_key`, e il trigger della migrazione 023
                            // le ha appena rimesse in `track_overrides`. Senza
                            // questa chiamata starebbero lì senza toccare la
                            // riga. Per quasi tutti i brani nuovi costa una
                            // lettura che non trova niente.
                            let nuova = tx.last_insert_rowid();
                            crate::incerti::riapplica(&tx, nuova)?;
                            scritte_qui.push(nuova);
                            inserite += 1;
                        }
                        Destinazione::Aggiorna(id) => {
                            let chiave = update_track(&tx, id, row, now)?;
                            // `update_track` ha appena rimesso i tag del file
                            // sopra la riga: senza questa chiamata basta che
                            // cambi la data di modifica di un file perché la
                            // correzione dell'utente sparisca da `tracks`
                            // restando orfana in `track_overrides`. La chiave è
                            // quella che `update_track` ha scritto — i tag più
                            // l'arricchimento rimesso — e non `row.track_key`.
                            crate::incerti::riapplica_sopra_i_tag(&tx, id, &chiave)?;
                            aggiornate += 1;
                        }
                        Destinazione::Sposta(id) => {
                            let chiave = update_track(&tx, id, row, now)?;
                            // Lo stesso, e qui conta doppio: un file spostato ha
                            // anche un `album_key` nuovo, che si ricalcola dal
                            // percorso d'arrivo e dall'album **corretto**.
                            crate::incerti::riapplica_sopra_i_tag(&tx, id, &chiave)?;
                            spostate += 1;
                        }
                    }
                }
                // E **dopo**: l'album e l'artista in cui ogni riga è entrata,
                // correzioni a mano comprese.
                toccati.segna_brani(&tx, &scritte_qui)?;
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
            // ── e la guardia contro le stragi ──
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
            // Il caso vero: un disco che non si è montato all'avvio, una radice
            // che risponde ma è vuota perché il punto di mount è caduto, una
            // cartella rinominata fuori da Aether. Dopo la protezione delle
            // radici irraggiungibili e dei rami persi resta improbabile, e
            // proprio per questo non costa niente chiedere prima di farlo.
            //
            // Vale per ogni scansione, prudente o no: vedi `Scan::prudente`.
            // Quel che cambia è solo se l'elenco resta a chi può dire sì.
            let strage = sospese.len() > SOGLIA_RIMOZIONI
                && sospese.len().saturating_mul(FRAZIONE_SOSPETTA) > totale_conosciute;
            // Il percorso viene dal piano, cioè da com'era la riga quando la si
            // è giudicata: se nel frattempo qualcuno l'ha cambiata, la
            // cancellazione non la riconosce e non la tocca.
            let elenco: Vec<RimozioneInAttesa> = sospese
                .iter()
                .filter_map(|(id, _)| {
                    report
                        .plan
                        .to_remove
                        .iter()
                        .find(|r| r.track.id == *id)
                        .map(|r| RimozioneInAttesa {
                            id: *id,
                            path: r.track.path.clone(),
                        })
                })
                .collect();
            if strage {
                report.rimozioni_rinviate = sospese.len();
                if !self.prudente {
                    report.rimozioni_in_attesa = elenco;
                }
            } else {
                let tolte = deposito.con_connessione(|connection| {
                    let tx = transazione_di_scrittura(connection)
                        .map_err(|err| db_error("apertura della transazione", &err))?;
                    let coppie: Vec<(i64, &str)> = elenco
                        .iter()
                        .map(|rimozione| (rimozione.id, rimozione.path.as_str()))
                        .collect();
                    let quali: Vec<i64> = elenco.iter().map(|rimozione| rimozione.id).collect();
                    toccati.segna_brani(&tx, &quali)?;
                    let tolte = remove_tracks(&tx, &coppie)?;
                    tx.commit()
                        .map_err(|err| db_error("chiusura della transazione", &err))?;
                    Ok(tolte)
                })?;
                report.removed += tolte;
            }
        }

        // ── 7. gli aggregati, quando i brani sono tutti al loro posto ──
        //
        // Solo quelli che questa passata ha toccato: una scansione che non ha
        // cambiato niente non rilegge più cinquantamila brani per riscrivere
        // gli stessi album. Per intero quando serve — la prima volta, dopo una
        // migrazione o «Rileggi tutti i tag»: vedi `ricostruisci_aggregati`.
        report.aggregates = deposito.con_connessione(|connection| {
            if toccati.e_vuoto() {
                // Niente scritto, niente transazione: due conteggi e basta.
                if aggregati_in_regola(connection)? {
                    return conta_aggregati(connection);
                }
            }
            let tx = transazione_di_scrittura(connection)
                .map_err(|err| db_error("apertura della transazione", &err))?;
            let aggregati = ricostruisci_aggregati(&tx, &toccati)?;
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
    /// Il percorso. `None` per un brano di catalogo, che un file non ce l'ha.
    pub path: Option<String>,
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
    /// Da quale catalogo arriva, col nome stabile di
    /// [`aether_domain::esterno::Fonte`]. `None` per un file sul disco.
    ///
    /// È il campo con cui la finestra decide se mostrare la pastiglia «in
    /// streaming» e se nascondere i gesti che vogliono un file — mostrare nella
    /// cartella, riscrivere i tag, spostare. Uno solo, e non un booleano
    /// accanto a un nome: due campi che dicono la stessa cosa sono due campi
    /// che un giorno la diranno diversa.
    pub fonte: Option<String>,
    /// La pagina pubblica del brano, da mettere accanto a chi lo ascolta.
    ///
    /// Non è un ornamento: per certe Creative Commons e per i termini di Audius
    /// il rimando visibile è una condizione d'uso, e questo è il campo che la
    /// soddisfa.
    pub fonte_pagina: Option<String>,
    /// Sotto che licenza sta, col nome stabile di
    /// [`aether_domain::esterno::Licenza`].
    pub licenza: Option<String>,
}

/// Le colonne di [`TrackSummary`], nell'ordine in cui le legge `track_from_row`.
pub(crate) const COLONNE_BRANO: &str = "t.id, t.path, t.title, t.artist, t.album, t.album_key,
     t.track_number, t.disc_number, t.duration_ms, t.year, t.cover_art_hash,
     t.play_count, t.liked, t.rating, t.source_service, t.fonte_pagina, t.licenza";

/// Quante sono le colonne di [`COLONNE_BRANO`].
///
/// Esiste perché una query le mette in testa e ne aggiunge altre dopo — la
/// cronologia lo fa — e l'indice della prima colonna aggiunta dipende da
/// questo numero. Scritto una volta sola, aggiungere una colonna al riepilogo
/// resta un cambiamento in un posto solo; scritto a mano in ogni chiamante,
/// sarebbe il genere di disallineamento che si manifesta come «l'ora
/// dell'ascolto è diventata il voto».
pub(crate) const COLONNE_BRANO_QUANTE: usize = 17;

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
        fonte: row.get(14)?,
        fonte_pagina: row.get(15)?,
        licenza: row.get(16)?,
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
    // Le colonne del brano vengono per prime: le quattro dell'ascolto cominciano
    // dove finiscono quelle, e il conto lo tiene [`COLONNE_BRANO_QUANTE`]
    // invece di un numero scritto qui. Quando `COLONNE_BRANO` è cresciuta di
    // tre colonne, questi indici si sono spostati da soli — prima di allora
    // erano scritti a mano, e la prova qui sotto controlla il campo
    // `quando_ms` e non solo la lunghezza proprio per accorgersene.
    let rows = statement
        .query_map(rusqlite::params![limit, offset], |row| {
            Ok(VoceCronologia {
                brano: track_from_row(row)?,
                id: row.get(COLONNE_BRANO_QUANTE)?,
                quando_ms: row.get(COLONNE_BRANO_QUANTE + 1)?,
                ms_ascoltati: row.get(COLONNE_BRANO_QUANTE + 2)?,
                sorgente: row.get(COLONNE_BRANO_QUANTE + 3)?,
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

/// I dati tecnici del file di un brano: cos'è il file, non cosa esce dalle casse.
///
/// La distinzione è una decisione, non una semplificazione. Quel che esce dalla
/// scheda audio può essere ricampionato dal mixer di sistema, e questa struttura
/// non lo sa e non lo dice: descrive il file che sta sul disco. Raccontare
/// l'uscita reale vorrebbe dire una riga che cambia quando si cambia cuffia, per
/// una domanda — «com'è fatta questa edizione» — a cui l'uscita non risponde.
///
/// # Perché non sta in [`TrackSummary`]
///
/// Perché `TrackSummary` è la riga di un elenco, e di righe ce ne sono qualche
/// migliaio per volta, più tutta la coda. Quattro campi in più su ognuna
/// sarebbero quattro campi spediti mille volte per disegnarne **uno**: quello
/// del brano che sta suonando. La parsimonia di quella struttura è dichiarata
/// nella sua carta, e questo tipo esiste per non smentirla.
///
/// # Perché ogni campo è un `Option`
///
/// Perché le quattro colonne sono nullable, e lo sono per forza: `lofty` non
/// ricava le proprietà di tutti i contenitori allo stesso modo, e una libreria
/// scansionata quando quelle colonne non si scrivevano ancora ha le righe vuote.
/// Chi disegna unisce i pezzi che ci sono e tace sugli altri.
///
/// # Perché i numeri sono `i64` e non `u32`
///
/// Perché qui si **legge**, e leggere non deve poter fallire per un dato storto.
/// Le colonne sono `INTEGER` senza `CHECK`: nessuno vi scrive un numero negativo
/// — arrivano dai `u32` e `u8` di `lofty` — ma un `u32` chiesto a una riga
/// negativa è un errore di rusqlite, e quell'errore butterebbe via tutti e
/// quattro i campi per colpa di uno. Con un `i64` il valore assurdo arriva a chi
/// disegna, che lo scarta perché non è positivo: lo stesso posto in cui si
/// scarta un `NULL`, invece di un secondo cammino per il guasto.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatoFile {
    /// Il formato come si scrive a chi guarda: «FLAC», «MP3», «M4A».
    ///
    /// Già passato per [`etichetta_codec`]: in tabella c'è il `Debug` di
    /// `lofty::FileType`, e quel che la finestra deve mostrare non è «Mpeg».
    pub codec: Option<String>,
    /// La frequenza di campionamento, in hertz: 44100, non 44,1.
    ///
    /// In hertz perché è così che sta in tabella e così che `lofty` la legge. I
    /// kilohertz con la virgola sono una questione di lingua, e si fanno dove la
    /// lingua si conosce — nella finestra.
    pub sample_rate: Option<i64>,
    /// Quanti canali: 1, 2, 6, 8.
    ///
    /// Il numero e non il nome. «Stereo» e «5.1» sono parole, e le parole si
    /// traducono: mandarle da qui vorrebbe dire mandare italiano a chi ha
    /// l'interfaccia in inglese.
    pub channels: Option<i64>,
    /// Il bitrate **medio**, in kbit/s, come l'ha calcolato `lofty`.
    ///
    /// Medio e non dichiarato, ed è la ragione per cui si mostra anche sui
    /// formati senza perdita: su un VBR dice quel che il file pesa davvero
    /// invece di quel che l'intestazione promette, e su un FLAC dice quanto è
    /// densa l'edizione — che è l'unica differenza fra due FLAC dello stesso
    /// brano.
    pub bitrate: Option<i64>,
    /// I bit per campione, solo per i formati senza perdita.
    ///
    /// Non è una colonna: lo dice il decodificatore quando il brano suona. Vedi
    /// [`FormatoFile::completato_da`].
    pub bits_per_sample: Option<i64>,
    /// La frequenza a cui il brano esce dalla scheda audio, in hertz.
    ///
    /// Diversa da [`Self::sample_rate`] vuol dire ricampionato. Come
    /// [`Self::bits_per_sample`], lo sa solo chi suona.
    pub output_rate: Option<i64>,
}

impl FormatoFile {
    /// La scheda del database corretta con quel che il decodificatore ha in
    /// mano.
    ///
    /// # Chi vince, e perché
    ///
    /// Il **decodificatore** su codec, frequenza e canali: sono i numeri di
    /// quel che si sente. La colonna può dire FLAC di un brano dell'Internet
    /// Archive che si ascolta dal derivato MP3, M4A di un file che dentro è
    /// ALAC, e niente del tutto per un flusso.
    ///
    /// Il **database** sul bitrate dei file locali: è la misura di `lofty`, che
    /// toglie tag e copertina, mentre il conto del motore è byte diviso durata.
    /// Per un flusso è il contrario — le colonne descrivono il file del catalogo,
    /// e i byte che arrivano sono quelli del derivato — e vince il motore.
    #[must_use]
    pub fn completato_da(self, suonato: &aether_play::FormatoSuonato) -> Self {
        let kbps = suonato.kbps.map(i64::from);
        Self {
            codec: suonato.codec.map(str::to_owned).or(self.codec),
            sample_rate: Some(i64::from(suonato.frequenza)),
            channels: Some(i64::from(suonato.canali)),
            bitrate: if suonato.remoto {
                kbps.or(self.bitrate)
            } else {
                self.bitrate.or(kbps)
            },
            bits_per_sample: suonato.bit.map(i64::from),
            output_rate: Some(i64::from(suonato.frequenza_uscita)),
        }
    }
}

/// Il nome del formato come si scrive, dal `Debug` di `lofty::FileType`.
///
/// # Perché serve una mappa, e perché lavora in lettura
///
/// Perché la scansione scrive `format!("{:?}", tagged.file_type())` (vedi
/// [`crate::metadata::read_tags`]), cioè i nomi delle varianti di
/// un'enumerazione Rust: «Mpeg», «Vorbis», «Mp4». Sono nomi di codice, non nomi
/// di formati, e nessuno dei tre è quel che chi ascolta si aspetta di leggere.
///
/// La correzione sta qui e non nella scansione perché **le librerie già
/// scansionate hanno quel valore in tabella**. Tradurre in scrittura vorrebbe
/// dire una migrazione per riscrivere una colonna che fino a oggi nessuno aveva
/// mai letto, e anche dopo la migrazione le due forme convivrebbero comunque:
/// basta un disco esterno ricollegato, o un profilo ripristinato da un backup
/// più vecchio. Tradurre in lettura funziona su tutto quel che c'è già, e non
/// costa niente.
///
/// # Cosa dice, e cosa non dice
///
/// Il **contenitore**, che è l'unica cosa che la colonna sa. `Mp4 → M4A` e non
/// «AAC»: dentro un M4A ci può stare AAC o ALAC, la colonna non distingue, e
/// scrivere «AAC» su un ALAC vorrebbe dire chiamare «con perdita» un file che
/// non ne ha. Per la stessa ragione `Vorbis → Ogg Vorbis`, che è il nome con cui
/// quel formato si conosce.
///
/// L'unica imprecisione che la mappa accetta è `Mpeg → MP3`. `lofty` tiene
/// MPEG-1 Layer I, II e III in una variante sola, quindi un `.mp2` — che in una
/// libreria di musica non c'è — si leggerebbe «MP3». L'alternativa sarebbe
/// scrivere «MPEG», che è giusto e non dice niente a nessuno.
///
/// # Un nome sconosciuto resta com'è
///
/// `FileType` è `#[non_exhaustive]` e ha una variante `Custom`. Un valore fuori
/// elenco si mostra **così come sta scritto**: è sempre meglio di niente, e non
/// mente mai. Il giorno in cui `lofty` aggiunge un contenitore, Aether ne scrive
/// il nome della variante invece di lasciare un buco — brutto e vero, che è
/// l'ordine giusto fra le due cose.
#[must_use]
pub fn etichetta_codec(grezzo: &str) -> String {
    match grezzo {
        "Aac" => "AAC",
        "Aiff" => "AIFF",
        "Ape" => "APE",
        "Flac" => "FLAC",
        "Mpeg" => "MP3",
        "Mp4" => "M4A",
        "Mpc" => "Musepack",
        "Opus" => "Opus",
        "Vorbis" => "Ogg Vorbis",
        "Speex" => "Speex",
        "Wav" => "WAV",
        "WavPack" => "WavPack",
        altro => return altro.to_owned(),
    }
    .to_owned()
}

/// I dati tecnici del file di un brano, per identificativo.
///
/// `None` se la riga non c'è più, per la stessa ragione di [`read_summary`]: un
/// brano può uscire dalla libreria mentre la coda lo tiene ancora, e non è un
/// guasto da propagare. `Some` con i campi vuoti è un'altra cosa, e vuol dire
/// che la riga c'è ma quei dati non si erano letti.
///
/// Una `SELECT` sulle quattro colonne e non [`COLONNE_BRANO`]: le quattro non
/// stanno in quell'elenco, e metterle là vorrebbe dire spedirle con ogni riga di
/// ogni elenco per disegnarle in un posto solo.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn read_formato(
    connection: &Connection,
    track_id: i64,
) -> Result<Option<FormatoFile>, AppError> {
    let mut statement = connection
        .prepare_cached("SELECT codec, sample_rate, channels, bitrate FROM tracks WHERE id = ?1")
        .map_err(|err| db_error("lettura del formato di un brano", &err))?;
    statement
        .query_row([track_id], |row| {
            Ok(FormatoFile {
                // Una stringa vuota vale come assente: «» non è un formato, e
                // lasciarla passare vorrebbe dire un separatore senza niente
                // davanti nella riga che chi disegna compone.
                codec: row
                    .get::<_, Option<String>>(0)?
                    .filter(|grezzo| !grezzo.trim().is_empty())
                    .map(|grezzo| etichetta_codec(grezzo.trim())),
                sample_rate: row.get(1)?,
                channels: row.get(2)?,
                bitrate: row.get(3)?,
                bits_per_sample: None,
                output_rate: None,
            })
        })
        .map(Some)
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            altro => Err(db_error("lettura del formato di un brano", &altro)),
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
    // identificativi resta un elenco di parametri anche quando è lungo. A pezzi
    // di [`PEZZO`] perché «Riproduci tutto» su una libreria grande fa una coda
    // di decine di migliaia di brani, e SQLite oltre i 32 766 parametri non
    // prepara la query: il pannello della coda restava vuoto con un errore.
    // L'ordine non ne soffre, lo rimette la mappa qui sotto.
    let mut trovati = Vec::with_capacity(ids.len());
    for pezzo in ids.chunks(PEZZO) {
        let sql = format!(
            "SELECT {COLONNE_BRANO} FROM tracks t WHERE t.id IN ({})",
            segnaposti(pezzo.len())
        );
        let mut statement = connection
            .prepare_cached(&sql)
            .map_err(|err| db_error("brani per identificativo", &err))?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(pezzo), track_from_row)
            .map_err(|err| db_error("brani per identificativo", &err))?;
        for riga in rows {
            trovati.push(riga.map_err(|err| db_error("brani per identificativo", &err))?);
        }
    }

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

/// Gli identificativi dei brani a questi percorsi, nello stesso ordine.
///
/// `None` per un percorso che non è in libreria: serve a chi ha dei file in
/// mano — trascinati sulla finestra — e deve sapere quali può mettere in coda,
/// perché la coda è fatta di identificativi e un file che la scansione non ha
/// mai visto non ne ha uno.
///
/// # Due confronti, e perché non `path_key`
///
/// Prima quello esatto, che passa dall'indice `UNIQUE` di `path` ed è il caso
/// di quasi tutti: Esplora risorse consegna il percorso con le stesse maiuscole
/// che la camminata ha scritto. Poi, solo dove il filesystem ignora le
/// maiuscole, lo stesso confronto con `COLLATE NOCASE`, per i file che il primo
/// non ha trovato.
///
/// Fino alla 2.4 quel secondo confronto non aveva un indice, e ogni file
/// lasciato cadere con una grafia diversa leggeva la tabella intera: cento file
/// trascinati da una cartella rinominata nelle maiuscole erano cento letture di
/// cinquantamila righe, sul filo che aspettava di metterli in coda. La 030 gli
/// ha dato `idx_tracks_percorso_piegato`, della stessa forma, e una prova qui
/// sotto guarda il piano di SQLite e fallisce se torna la lettura intera.
///
/// [`aether_domain::paths::path_key`] unificherebbe anche i separatori, e
/// servirebbe calcolarla su ogni riga della tabella: una lettura intera della
/// libreria per ogni file lasciato cadere, per riconciliare una barra che i
/// percorsi di Windows non hanno mai girata dall'altra parte.
pub fn ids_by_path(
    connection: &Connection,
    paths: &[String],
    rules: PathRules,
) -> Result<Vec<Option<i64>>, AppError> {
    use rusqlite::OptionalExtension as _;

    let mut esatto = connection
        .prepare_cached("SELECT id FROM tracks WHERE path = ?1")
        .map_err(|err| db_error("brani per percorso", &err))?;
    let mut piegato = connection
        .prepare_cached("SELECT id FROM tracks WHERE path = ?1 COLLATE NOCASE LIMIT 1")
        .map_err(|err| db_error("brani per percorso", &err))?;
    paths
        .iter()
        .map(|path| {
            let trovato = esatto
                .query_row([path], |row| row.get(0))
                .optional()
                .map_err(|err| db_error("brani per percorso", &err))?;
            if trovato.is_some() || !rules.case_insensitive {
                return Ok(trovato);
            }
            piegato
                .query_row([path], |row| row.get(0))
                .optional()
                .map_err(|err| db_error("brani per percorso", &err))
        })
        .collect()
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

        /// L'identificativo della riga di un brano, che la scansione decide da sé.
        ///
        /// Si cerca per titolo e non per percorso: la camminata scrive il
        /// percorso coi separatori del sistema, mentre quello che la prova ha
        /// composto tiene le barre con cui è stato scritto qui sopra.
        fn id_del_titolo(&self, titolo: &str) -> i64 {
            self.connection
                .query_row("SELECT id FROM tracks WHERE title = ?1", [titolo], |r| {
                    r.get(0)
                })
                .expect("identificativo")
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
    fn i_percorsi_trascinati_ritrovano_i_loro_brani() {
        let mut lib = Libreria::nuova();
        lib.brano("A/01.wav", "Uno", "Tale", "A");
        lib.brano("A/02.wav", "Due", "Tale", "A");
        lib.scansiona();

        // Con i separatori del sistema, come li consegna Esplora risorse e come
        // li scrive la camminata: `join("A/01.wav")` su Windows lascerebbe una
        // barra dritta in mezzo, e quel percorso in tabella non c'è.
        let nativo = |cartella: &str, file: &str| {
            lib.musica()
                .join(cartella)
                .join(file)
                .to_string_lossy()
                .into_owned()
        };
        let primo = nativo("A", "01.wav");
        let secondo = nativo("A", "02.wav");
        let fuori = nativo("B", "03.wav");
        let insensibile = PathRules {
            case_insensitive: true,
        };

        let trovati = ids_by_path(
            &lib.connection,
            &[secondo.clone(), fuori.clone(), primo.clone()],
            insensibile,
        )
        .expect("ricerca per percorso");
        assert_eq!(trovati.len(), 3, "uno per percorso, nello stesso ordine");
        assert!(trovati.first().is_some_and(Option::is_some));
        assert_eq!(trovati.get(1), Some(&None), "quel che non è in libreria");
        assert!(trovati.get(2).is_some_and(Option::is_some));
        assert_ne!(trovati.first(), trovati.get(2));

        // Le maiuscole contano solo dove il filesystem le distingue.
        let gridato = vec![primo.to_uppercase()];
        let piegato = ids_by_path(&lib.connection, &gridato, insensibile).expect("piegato");
        assert_eq!(piegato.first(), trovati.get(2));
        let sensibile = PathRules {
            case_insensitive: false,
        };
        let esatto = ids_by_path(&lib.connection, &gridato, sensibile).expect("esatto");
        assert_eq!(esatto, vec![None]);
    }

    #[test]
    fn la_correzione_sopravvive_a_una_riscansione() {
        // Il difetto che questa prova impedisce, ed è stato vero fino alla
        // 2.3.1: `track_overrides` si riempiva e **nessuno** la rileggeva.
        // Bastava che cambiasse la data di modifica di un file — ritaggarlo,
        // copiarlo, toccarlo — perché la correzione dell'utente sparisse da
        // `tracks` restando orfana nella sua tabella, senza che niente lo
        // dicesse.
        let mut lib = Libreria::nuova();
        lib.brano("Senza/Nome/01.wav", "Traccia 01", "Unknown Artist", "Al");
        assert_eq!(lib.scansiona().inserted, 1);
        let id: i64 = lib
            .connection
            .query_row("SELECT id FROM tracks", [], |r| r.get(0))
            .expect("identificativo");

        crate::incerti::correggi(
            &mut lib.connection,
            id,
            &crate::provenienza::Correzioni {
                titolo: Some("Hey You".to_owned()),
                artista: Some("Pink Floyd".to_owned()),
                ..crate::provenienza::Correzioni::default()
            },
        )
        .expect("correzione");

        // Il file non si tocca: i suoi tag restano quelli sbagliati, ed è il
        // punto. Quel che cambia è la sola cosa che serve a far rileggere la
        // riga, cioè il confronto `existing.modified_ms == file.modified_ms` di
        // `plan_scan`. Riscrivere i tag funzionerebbe anche, ma farebbe
        // dipendere la prova dalla risoluzione dell'orologio del filesystem.
        lib.connection
            .execute("UPDATE tracks SET date_modified = 0 WHERE id = ?1", [id])
            .expect("data di modifica");

        let esito = lib.scansiona();
        assert_eq!(
            esito.updated, 1,
            "la riga deve essere stata riletta, altrimenti la prova non prova niente"
        );

        let (titolo, artista, chiave): (String, String, String) = lib
            .connection
            .query_row(
                "SELECT title, artist, track_key FROM tracks WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("riga");
        assert_eq!(titolo, "Hey You", "il titolo corretto resta");
        assert_eq!(artista, "Pink Floyd");
        // E con esso l'identità: una chiave ricalcolata dai tag del file
        // rilegherebbe in silenzio ascolti e posizioni di un altro brano.
        assert!(chiave.contains("pink floyd"), "chiave: {chiave}");
    }

    #[test]
    fn il_testo_di_un_brano_corretto_resta_suo_dopo_una_riscansione() {
        // Due copie dello stesso brano con gli stessi tag sbagliati — il FLAC e
        // l'mp3 — e una sola corretta, con un testo sincronizzato a mano. La
        // riscansione della copia corretta scriveva prima la chiave dei tag, e
        // il trigger della 022 le portava dietro il testo; poi `riapplica`
        // rimetteva la chiave corretta, ma il ritorno era bloccato dall'altra
        // copia, che quella chiave la porta ancora. Il testo finiva sulla copia
        // che nessuno aveva sincronizzato.
        let mut lib = Libreria::nuova();
        lib.brano("Uno/01.wav", "Traccia 01", "Unknown Artist", "Al");
        lib.brano("Due/01.wav", "Traccia 01", "Unknown Artist", "Al");
        assert_eq!(lib.scansiona().inserted, 2);
        let (id, chiave_dei_tag): (i64, String) = lib
            .connection
            .query_row(
                "SELECT id, track_key FROM tracks WHERE path LIKE '%Uno%'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("la prima copia");

        crate::incerti::correggi(
            &mut lib.connection,
            id,
            &crate::provenienza::Correzioni {
                titolo: Some("Hey You".to_owned()),
                artista: Some("Pink Floyd".to_owned()),
                ..crate::provenienza::Correzioni::default()
            },
        )
        .expect("correzione");
        let chiave_corretta: String = lib
            .connection
            .query_row("SELECT track_key FROM tracks WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .expect("chiave corretta");
        assert_ne!(chiave_corretta, chiave_dei_tag);
        lib.connection
            .execute(
                "INSERT INTO lyrics (track_key, source, synced, checked_at, updated_at)
                 VALUES (?1, 'mano', '[00:01.00]Is there anybody', 1, 1)",
                [&chiave_corretta],
            )
            .expect("testo a mano");

        lib.connection
            .execute("UPDATE tracks SET date_modified = 0 WHERE id = ?1", [id])
            .expect("data di modifica");
        assert_eq!(
            lib.scansiona().updated,
            1,
            "la riga deve essere stata riletta"
        );

        let dove: Vec<String> = lib
            .connection
            .prepare("SELECT track_key FROM lyrics")
            .and_then(|mut s| {
                s.query_map([], |r| r.get::<_, String>(0))
                    .and_then(Iterator::collect)
            })
            .expect("testi");
        assert_eq!(
            dove,
            vec![chiave_corretta],
            "il testo resta alla copia corretta"
        );
    }

    #[test]
    fn gli_elenchi_non_ordinano_la_tabella_a_ogni_pagina() {
        // Il piano di SQLite, e non un cronometro: «USE TEMP B-TREE FOR ORDER
        // BY» vuol dire che ogni pagina da duecento righe ordinava la libreria
        // intera. È la frase che la migrazione 024 esiste per togliere, e questa
        // prova fallisce se un giorno l'ordinamento e l'indice smettono di
        // avere la stessa forma.
        let lib = Libreria::nuova();
        let piano = |sql: &str| -> String {
            let mut statement = lib
                .connection
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .expect("piano");
            let righe = statement
                .query_map([], |r| r.get::<_, String>(3))
                .expect("righe del piano");
            righe
                .collect::<Result<Vec<_>, _>>()
                .expect("piano letto")
                .join(" | ")
        };
        for ordine in [TrackOrder::Shelf, TrackOrder::Title, TrackOrder::MostPlayed] {
            let letto = piano(&format!(
                "SELECT t.id FROM tracks t ORDER BY {} LIMIT 200 OFFSET 0",
                ordine.sql()
            ));
            assert!(
                !letto.contains("TEMP B-TREE"),
                "{ordine:?} ordina la tabella intera: {letto}"
            );
        }
        let album = piano(
            "SELECT a.title FROM albums a
             ORDER BY a.artist COLLATE NOCASE, a.year, a.title COLLATE NOCASE
             LIMIT 200 OFFSET 0",
        );
        assert!(!album.contains("TEMP B-TREE"), "gli album: {album}");
    }

    #[test]
    fn la_correzione_torna_con_il_file() {
        // Il file sparisce — la cartella tolta, il disco scollegato — e la
        // scansione cancella la riga. Il file torna, e la riga nuova deve
        // ritrovare quel che l'utente aveva corretto a mano.
        let mut lib = Libreria::nuova();
        let file = lib.brano("Senza/Nome/01.wav", "Traccia 01", "Unknown Artist", "Al");
        assert_eq!(lib.scansiona().inserted, 1);
        let id: i64 = lib
            .connection
            .query_row("SELECT id FROM tracks", [], |r| r.get(0))
            .expect("identificativo");
        crate::incerti::correggi(
            &mut lib.connection,
            id,
            &crate::provenienza::Correzioni {
                titolo: Some("Hey You".to_owned()),
                artista: Some("Pink Floyd".to_owned()),
                ..crate::provenienza::Correzioni::default()
            },
        )
        .expect("correzione");

        std::fs::remove_file(&file).expect("il file sparisce");
        assert_eq!(lib.scansiona().removed, 1);
        assert_eq!(lib.conta("track_overrides"), 0, "la riga se n'è andata");
        assert_eq!(lib.conta("correzioni_orfane"), 1, "la correzione no");

        lib.brano("Senza/Nome/01.wav", "Traccia 01", "Unknown Artist", "Al");
        assert_eq!(lib.scansiona().inserted, 1);
        let (titolo, artista): (String, String) = lib
            .connection
            .query_row("SELECT title, artist FROM tracks", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .expect("riga");
        assert_eq!(titolo, "Hey You");
        assert_eq!(artista, "Pink Floyd");
        assert_eq!(lib.conta("correzioni_orfane"), 0, "e non resta orfana");
    }

    #[test]
    fn ascolti_voto_e_playlist_tornano_con_il_file() {
        // La migrazione 028 vista da una scansione vera: il file sparisce, la
        // riga se ne va — e con lei, in cascata, cronologia e playlist — e poi
        // il file torna. La riga nuova deve ritrovare tutto quel che l'utente
        // ci aveva costruito sopra.
        let mut lib = Libreria::nuova();
        let file = lib.brano("Disco/01.wav", "Uno", "Artista", "Disco");
        lib.brano("Disco/02.wav", "Due", "Artista", "Disco");
        assert_eq!(lib.scansiona().inserted, 2);
        let uno = lib.id_del_titolo("Uno");
        let due = lib.id_del_titolo("Due");
        lib.connection
            .execute(
                "UPDATE tracks SET play_count = 5, last_played_at = 1000, rating = 4,
                        rating_at = 900, liked = 1, liked_at = 800, stats_updated_at = 900
                  WHERE id = ?1",
                [uno],
            )
            .expect("statistiche");
        lib.connection
            .execute_batch(&format!(
                "INSERT INTO play_history (track_id, played_at, ms_played) VALUES ({uno}, 100, 1000);
                 INSERT INTO play_history (track_id, played_at, ms_played, source)
                 VALUES ({uno}, 200, 2000, 'spotify');"
            ))
            .expect("cronologia");
        let lista = crate::playlists::create(&lib.connection, "Lista")
            .expect("playlist")
            .id;
        crate::playlists::add_tracks(&mut lib.connection, lista, &[due, uno, due])
            .expect("aggiunti");
        let altra = crate::playlists::create(&lib.connection, "Altra")
            .expect("playlist")
            .id;
        crate::playlists::add_tracks(&mut lib.connection, altra, &[uno]).expect("aggiunto");

        std::fs::remove_file(&file).expect("il file sparisce");
        assert_eq!(lib.scansiona().removed, 1);
        assert_eq!(
            lib.conta("play_history"),
            0,
            "la cascata ha portato via gli ascolti"
        );
        assert_eq!(lib.conta("brani_orfani"), 1, "ma sono stati messi da parte");
        assert_eq!(lib.conta("ascolti_orfani"), 2);
        assert_eq!(lib.conta("playlist_orfane"), 2);

        // Una playlist cancellata nel frattempo si porta via il posto che teneva.
        crate::playlists::delete(&mut lib.connection, altra).expect("cancellata");
        assert_eq!(lib.conta("playlist_orfane"), 1);

        lib.brano("Disco/01.wav", "Uno", "Artista", "Disco");
        assert_eq!(lib.scansiona().inserted, 1);
        let tornato = lib.id_del_titolo("Uno");
        assert_ne!(tornato, uno, "è una riga nuova");
        let statistiche: (i64, i64, i64, i64, i64, i64, i64) = lib
            .connection
            .query_row(
                "SELECT play_count, last_played_at, rating, rating_at, liked, liked_at,
                        stats_updated_at
                   FROM tracks WHERE id = ?1",
                [tornato],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                },
            )
            .expect("statistiche");
        assert_eq!(statistiche, (5, 1000, 4, 900, 1, 800, 900));
        let ascolti: i64 = lib
            .connection
            .query_row(
                "SELECT COUNT(*) FROM play_history WHERE track_id = ?1",
                [tornato],
                |r| r.get(0),
            )
            .expect("cronologia");
        assert_eq!(ascolti, 2, "la cronologia è tornata");
        let importato: String = lib
            .connection
            .query_row(
                "SELECT source FROM play_history WHERE track_id = ?1 AND played_at = 200",
                [tornato],
                |r| r.get(0),
            )
            .expect("origine");
        assert_eq!(
            importato, "spotify",
            "un ascolto importato torna importato, e si lascia ancora disfare"
        );
        // Nel buco che aveva lasciato, fra i due «Due».
        let ordine: Vec<String> = crate::playlists::tracks(&lib.connection, lista)
            .expect("brani")
            .into_iter()
            .map(|b| b.title)
            .collect();
        assert_eq!(ordine, ["Due", "Uno", "Due"]);
        assert_eq!(lib.conta("brani_orfani"), 0, "e non resta orfano");
        assert_eq!(lib.conta("ascolti_orfani"), 0);
        assert_eq!(lib.conta("playlist_orfane"), 0);
    }

    #[cfg(windows)]
    #[test]
    fn un_file_rinominato_solo_nelle_maiuscole_tiene_la_riga_e_prende_il_nome() {
        // Windows non cambia la data di un file rinominato, e la chiave del
        // percorso piega il caso: senza guardare la grafia la riga terrebbe il
        // nome vecchio, che su una share Samba sensibile alle maiuscole non si
        // apre più. Stessa riga, stessa storia, nome nuovo.
        let mut lib = Libreria::nuova();
        let file = lib.brano("Disco/Song.wav", "Canzone", "Artista", "Disco");
        assert_eq!(lib.scansiona().inserted, 1);
        let id = lib.id_del_titolo("Canzone");
        lib.connection
            .execute("UPDATE tracks SET play_count = 3 WHERE id = ?1", [id])
            .expect("ascolti");

        let rinominato = file.with_file_name("song.wav");
        std::fs::rename(&file, &rinominato).expect("rinomina");
        let esito = lib.scansiona();
        assert_eq!((esito.inserted, esito.removed, esito.updated), (0, 0, 1));
        let (percorso, ascolti): (String, i64) = lib
            .connection
            .query_row(
                "SELECT path, play_count FROM tracks WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("riga");
        assert!(percorso.ends_with("song.wav"), "{percorso}");
        assert_eq!(ascolti, 3);

        // E la passata dopo non ha più niente da fare.
        assert_eq!(lib.scansiona().plan.unchanged, 1);
    }

    #[test]
    fn una_correzione_di_prima_della_019_si_salva_anche_lei() {
        // Una riga corretta prima della 019: la migrazione le ha lasciato
        // `content_key` a NULL, e il file non è mai cambiato da allora.
        let mut lib = Libreria::nuova();
        let file = lib.brano("Senza/Nome/01.wav", "Traccia 01", "Unknown Artist", "Al");
        assert_eq!(lib.scansiona().inserted, 1);
        let id: i64 = lib
            .connection
            .query_row("SELECT id FROM tracks", [], |r| r.get(0))
            .expect("identificativo");
        crate::incerti::correggi(
            &mut lib.connection,
            id,
            &crate::provenienza::Correzioni {
                titolo: Some("Hey You".to_owned()),
                ..crate::provenienza::Correzioni::default()
            },
        )
        .expect("correzione");
        lib.connection
            .execute("UPDATE tracks SET content_key = NULL WHERE id = ?1", [id])
            .expect("com'era dopo la 019");

        // La riscansione di tutti i giorni la rilegge, una volta.
        assert_eq!(
            lib.scansiona().updated,
            1,
            "la riga senza chiave si rilegge"
        );
        assert_eq!(lib.scansiona().updated, 0, "e poi basta");
        let titolo: String = lib
            .connection
            .query_row("SELECT title FROM tracks WHERE id = ?1", [id], |r| r.get(0))
            .expect("titolo");
        assert_eq!(titolo, "Hey You", "la correzione resta");

        // E il file che sparisce lascia la correzione da parte.
        std::fs::remove_file(&file).expect("il file sparisce");
        assert_eq!(lib.scansiona().removed, 1);
        assert_eq!(
            lib.conta("correzioni_orfane"),
            1,
            "la correzione resta orfana"
        );
    }

    #[test]
    fn la_chiave_di_contenuto_ignora_la_correzione() {
        // Le due chiavi fanno due mestieri diversi, e questa prova lo fissa:
        // `track_key` segue quel che l'utente ha deciso — deve, o gli altri
        // dispositivi cercano il brano sotto il nome sbagliato — mentre
        // `content_key` resta quella dei tag del file, che nessuno ha toccato.
        let mut lib = Libreria::nuova();
        lib.brano("Senza/Nome/01.wav", "Traccia 01", "Unknown Artist", "Al");
        assert_eq!(lib.scansiona().inserted, 1);

        let (id, contenuto_prima, brano_prima): (i64, String, String) = lib
            .connection
            .query_row("SELECT id, content_key, track_key FROM tracks", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .expect("riga");
        assert_eq!(
            contenuto_prima, brano_prima,
            "senza correzioni le due chiavi coincidono: la differenza nasce dopo"
        );

        crate::incerti::correggi(
            &mut lib.connection,
            id,
            &crate::provenienza::Correzioni {
                artista: Some("Pink Floyd".to_owned()),
                ..crate::provenienza::Correzioni::default()
            },
        )
        .expect("correzione");

        let (contenuto, brano): (String, String) = lib
            .connection
            .query_row(
                "SELECT content_key, track_key FROM tracks WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("riga");
        assert!(
            brano.contains("pink floyd"),
            "la chiave di scambio segue la correzione: {brano}"
        );
        assert_eq!(
            contenuto, contenuto_prima,
            "la chiave di contenuto non si muove: la scrivono solo insert_track e update_track"
        );
        assert!(
            contenuto.contains("unknown artist"),
            "e resta quella dei tag del file: {contenuto}"
        );

        // E resiste anche a una riscansione, che ripassa da `update_track` e
        // poi da `riapplica`: il primo la riscrive identica, il secondo non la
        // guarda affatto.
        lib.connection
            .execute("UPDATE tracks SET date_modified = 0 WHERE id = ?1", [id])
            .expect("data di modifica");
        assert_eq!(lib.scansiona().updated, 1);
        let dopo: String = lib
            .connection
            .query_row("SELECT content_key FROM tracks WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .expect("riga");
        assert_eq!(dopo, contenuto_prima);
    }

    #[test]
    fn un_file_spostato_si_ritrova_anche_se_lartista_e_stato_corretto() {
        // **La** prova di questo pacchetto, e il difetto che chiude è perdita di
        // dati: correggere l'artista di un brano gli cambiava l'identità, e da
        // quel momento spostare il suo file non lo faceva più riconoscere. La
        // riga veniva cancellata e reinserita — via ascolti, voto, preferiti,
        // playlist — e con `ON DELETE CASCADE` se ne andava anche la
        // sovrascrittura in `track_overrides`, cioè la correzione stessa.
        //
        // Con `track_key` al posto di `content_key` in `content_keys_of` e nelle
        // chiavi passate a `match_moved_tracks`, qui sotto `moved` è 0 e
        // `inserted` è 1: la prova fallisce, ed è il modo di verificare che
        // stia provando qualcosa.
        let mut lib = Libreria::nuova();
        lib.brano("sfusi/traccia.wav", "Hey You", "unknown", "The Wall");
        lib.scansiona();

        let id: i64 = lib
            .connection
            .query_row("SELECT id FROM tracks", [], |r| r.get(0))
            .expect("id");
        lib.connection
            .execute(
                "UPDATE tracks SET play_count = 42, rating = 5 WHERE id = ?1",
                [id],
            )
            .expect("storia d'ascolto");

        // La correzione: da qui in poi `tracks.track_key` dice «pink floyd» e i
        // tag del file dicono ancora «unknown».
        crate::incerti::correggi(
            &mut lib.connection,
            id,
            &crate::provenienza::Correzioni {
                artista: Some("Pink Floyd".to_owned()),
                ..crate::provenienza::Correzioni::default()
            },
        )
        .expect("correzione");

        // E lo spostamento, col file intatto: è il caso di ogni riordino, di
        // ogni cartella trascinata altrove, di ogni disco rimontato con una
        // lettera diversa.
        let da = lib.musica().join("sfusi/traccia.wav");
        let a = lib.musica().join("Pink Floyd/The Wall/01 Hey You.wav");
        std::fs::create_dir_all(a.parent().expect("cartella")).expect("cartelle");
        std::fs::rename(&da, &a).expect("spostamento");

        let esito = lib.scansiona();

        assert_eq!(esito.moved, 1, "il file si è spostato, non è nuovo");
        assert_eq!(esito.inserted, 0, "un inserimento qui è un brano duplicato");
        assert_eq!(esito.removed, 0);
        assert_eq!(lib.conta("tracks"), 1);

        let (stesso_id, ascolti, voto, artista, percorso): (i64, i64, i64, String, String) = lib
            .connection
            .query_row(
                "SELECT id, play_count, rating, artist, path FROM tracks",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .expect("riga");
        assert_eq!(stesso_id, id, "deve essere la stessa riga, non una nuova");
        assert_eq!(ascolti, 42);
        assert_eq!(voto, 5);
        assert!(percorso.ends_with("01 Hey You.wav"), "percorso: {percorso}");
        // E la correzione è ancora lì: `riapplica` gira dopo `update_track`
        // anche sul ramo `Sposta`, e la riga di `track_overrides` non è stata
        // portata via da nessuna cancellazione.
        assert_eq!(artista, "Pink Floyd");
        assert_eq!(lib.conta("track_overrides"), 1);
    }

    #[test]
    fn senza_tag_la_chiave_e_durata_e_dimensione() {
        // Attraverso la scansione vera, e non chiamando `compute` a mano:
        // l'unica cosa che questa prova può dire e le prove di dominio no è
        // **quali valori** `read_track` passa alla chiave. Se ci passasse i
        // campi già ripiegati — la radice del nome, «Artista sconosciuto» —
        // qui uscirebbero tre segmenti invece di due, e il ramo del ripiego
        // resterebbe quel che era fino a un momento fa: codice che non gira.
        let mut lib = Libreria::nuova();
        let path = lib.musica().join("A/Al/07 Senza tag.wav");
        wav(&path);
        assert_eq!(lib.scansiona().inserted, 1);

        let (contenuto, brano, durata, dimensione): (String, String, i64, i64) = lib
            .connection
            .query_row(
                "SELECT content_key, track_key, duration_ms, file_size FROM tracks",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .expect("riga");

        assert_eq!(
            contenuto,
            format!("{durata}|{dimensione}"),
            "un file senza tag si identifica da quanto dura e quanto pesa"
        );
        // E la dimensione è quella vera sul disco, non un numero qualunque che
        // combacia con sé stesso.
        assert_eq!(
            dimensione,
            i64::try_from(std::fs::metadata(&path).expect("stat").len()).expect("dimensione")
        );

        // `track_key` invece i ripieghi li usa, e deve: è la chiave con cui la
        // sincronizzazione nomina il brano, e `||` renderebbe ogni file senza
        // tag lo stesso brano su ogni dispositivo. Le due chiavi qui divergono
        // apposta.
        assert!(brano.contains("07 senza tag"), "chiave di brano: {brano}");
        assert_ne!(contenuto, brano);
    }

    #[test]
    fn un_file_senza_tag_rinominato_non_si_duplica() {
        // Il caso d'uso vero, ed è quello che il ripiego esiste per servire.
        //
        // Un file senza tag si distingue solo dal nome, quindi il nome è la
        // prima cosa che si sistema. Con l'identità di contenuto derivata dai
        // ripieghi — cioè dalla radice del nome — rinominarlo lo rendeva un
        // brano nuovo: riga cancellata, riga inserita, via ascolti, voto e
        // playlist. Durata e dimensione un rinomino non le tocca.
        //
        // Con i campi ripiegati al posto dei tag grezzi in `read_track`, qui
        // sotto `moved` è 0 e `inserted` è 1: la prova fallisce.
        let mut lib = Libreria::nuova();
        let da = lib.musica().join("sfusi/traccia 001.wav");
        wav(&da);
        lib.scansiona();

        let id: i64 = lib
            .connection
            .query_row("SELECT id FROM tracks", [], |r| r.get(0))
            .expect("id");
        lib.connection
            .execute_batch(&format!(
                "UPDATE tracks SET play_count = 17, rating = 4 WHERE id = {id};
                 INSERT INTO playlists (id, playlist_key, name, created_at, updated_at)
                 VALUES (1, 'p', 'P', 1, 1);
                 INSERT INTO playlist_tracks (playlist_id, track_id, position)
                 VALUES (1, {id}, 0);"
            ))
            .expect("storia d'ascolto");

        // Il rinomino, fuori da Aether: stessa cartella, nome che dice
        // finalmente qualcosa. Il contenuto del file non cambia di un byte.
        let a = lib.musica().join("sfusi/Nick Drake - Pink Moon.wav");
        std::fs::rename(&da, &a).expect("rinomino");

        let esito = lib.scansiona();

        assert_eq!(esito.moved, 1, "è lo stesso file con un nome nuovo");
        assert_eq!(esito.inserted, 0, "un inserimento qui è un brano duplicato");
        assert_eq!(esito.removed, 0);
        assert_eq!(lib.conta("tracks"), 1);

        let (stesso_id, ascolti, voto, titolo): (i64, i64, i64, String) = lib
            .connection
            .query_row(
                "SELECT id, play_count, rating, title FROM tracks",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .expect("riga");
        assert_eq!(stesso_id, id, "deve essere la stessa riga, non una nuova");
        assert_eq!(ascolti, 17);
        assert_eq!(voto, 4);
        assert_eq!(lib.conta("playlist_tracks"), 1, "la playlist regge");
        // E il titolo mostrato segue il nome nuovo, che è quel che l'utente si
        // aspetta: cambia l'etichetta, non l'identità.
        assert_eq!(titolo, "Nick Drake - Pink Moon");
    }

    #[test]
    fn la_migrazione_019_sblocca_i_testi_che_uno_scarto_aveva_congelato() {
        // La riparazione dei dati che viaggia con la 019, provata dove sta il
        // pacchetto che la porta.
        //
        // Il difetto: `imposta_scarto` scriveva `source = 'mano'` anche quando
        // un testo non c'era ancora, e da lì gli `UPSERT` di `ricorda` e
        // `ricorda_esito` — che hanno `WHERE lyrics.source <> 'mano'` per non
        // calpestare mai un testo dell'utente — non scrivevano più niente per
        // quel brano. Nessun testo, mai più.
        let connection = rusqlite::Connection::open_in_memory().expect("database");
        for migrazione in crate::db::MIGRATIONS.iter().filter(|m| m.version < 19) {
            connection
                .execute_batch(migrazione.sql)
                .unwrap_or_else(|err| panic!("migrazione {}: {err}", migrazione.version));
        }

        connection
            .execute_batch(
                "INSERT INTO lyrics (track_key, source, offset_ms, updated_at)
                   VALUES ('bloccato', 'mano', 250, 1);
                 INSERT INTO lyrics (track_key, synced, source, offset_ms, updated_at)
                   VALUES ('a mano', '[00:01.00]parole', 'mano', 0, 1);
                 INSERT INTO lyrics (track_key, source, instrumental, offset_ms, updated_at)
                   VALUES ('strumentale', 'mano', 1, 0, 1);
                 INSERT INTO lyrics (track_key, plain, source, offset_ms, updated_at)
                   VALUES ('piatto', 'parole', 'mano', 0, 1);",
            )
            .expect("righe di partenza");

        let diciannove = crate::db::MIGRATIONS
            .iter()
            .find(|m| m.version == 19)
            .expect("la 019 dev'essere nella catena");
        connection
            .execute_batch(diciannove.sql)
            .expect("la 019 si applica");

        let fonte = |chiave: &str| -> String {
            connection
                .query_row(
                    "SELECT source FROM lyrics WHERE track_key = ?1",
                    [chiave],
                    |r| r.get(0),
                )
                .expect("riga")
        };
        assert_eq!(
            fonte("bloccato"),
            "",
            "la riga nata da uno scarto torna al valore neutro"
        );
        // E lo scarto, che è la sola cosa che quella riga portava, resta.
        let scarto: i64 = connection
            .query_row(
                "SELECT offset_ms FROM lyrics WHERE track_key = 'bloccato'",
                [],
                |r| r.get(0),
            )
            .expect("scarto");
        assert_eq!(scarto, 250);

        // I tre casi che non vanno toccati, e sono la ragione per cui la
        // condizione ha tre clausole invece di una.
        assert_eq!(fonte("a mano"), "mano", "un testo sincronizzato a mano");
        assert_eq!(fonte("strumentale"), "mano", "una risposta «non ha parole»");
        assert_eq!(fonte("piatto"), "mano", "un testo piatto scritto a mano");
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

    // ── la rilettura dei tag e lo strato dell'arricchimento ─────────────────

    /// Scrive i tag come li lascerebbe un programma qualunque: quelli dati e
    /// nient'altro. Senza album se `album` è `None`; con un ReplayGain se
    /// `replaygain` c'è, che è quel che ci aggiunge un calcolatore del volume.
    fn tagga_con(
        path: &Path,
        titolo: &str,
        artista: &str,
        album: Option<&str>,
        replaygain: Option<&str>,
    ) {
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title(titolo.to_owned());
        tag.set_artist(artista.to_owned());
        if let Some(album) = album {
            tag.set_album(album.to_owned());
        }
        if let Some(guadagno) = replaygain {
            tag.insert_text(
                lofty::prelude::ItemKey::ReplayGainTrackGain,
                guadagno.to_owned(),
            );
        }
        tag.save_to_path(path, WriteOptions::default())
            .expect("scrittura dei tag");
    }

    /// Mette sul file una data di modifica precisa, diversa da quelle vicine.
    fn data(path: &Path, secondi: i64) {
        filetime::set_file_mtime(path, filetime::FileTime::from_unix_time(secondi, 0))
            .expect("data di modifica");
    }

    /// Un brano arricchito come lo lascia una passata vera: `enrich::registra`,
    /// con l'annotazione e una copertina arrivata dal catalogo.
    fn arricchisci(lib: &mut Libreria, id: i64, campi: aether_domain::enrich::Fields) {
        let path: String = lib
            .connection
            .query_row("SELECT path FROM tracks WHERE id = ?1", [id], |r| r.get(0))
            .expect("percorso");
        let tx = lib.connection.transaction().expect("transazione");
        let scrittura = crate::enrich::Scrittura {
            track_id: id,
            path,
            campi,
            copertina: None,
            fonte: "mb-release",
            confidenza: 0.95,
            originali: crate::enrich::TagOriginali::default(),
        };
        let esito = crate::enrich::EsitoFile {
            track_id: id,
            copertina: Some(StoredCover {
                hash: "c0ffee".to_owned(),
                width: 1,
                height: 1,
                byte_size: 1,
                mime_type: "image/jpeg",
                already_present: false,
            }),
            copertina_da: Some("provider"),
            applicato: true,
        };
        crate::enrich::registra(
            &tx,
            &crate::enrich::Decisione::default(),
            &[scrittura],
            &[esito],
            1,
        )
        .expect("registrazione");
        tx.commit().expect("chiusura");
    }

    /// Titolo, album, anno, copertina e stato dell'arricchimento di una riga.
    fn come_sta(
        lib: &Libreria,
        id: i64,
    ) -> (String, String, Option<i64>, Option<String>, Option<String>) {
        lib.connection
            .query_row(
                "SELECT title, album, year, cover_art_hash, enrich_status
                   FROM tracks WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .expect("riga")
    }

    /// L'annotazione dell'arricchimento, così com'è in tabella.
    fn annotata(lib: &Libreria, id: i64) -> Option<crate::enrich::CampiArricchiti> {
        crate::enrich::annotazione(&lib.connection, id).expect("lettura dell'annotazione")
    }

    #[test]
    fn un_programma_che_tocca_il_file_non_si_porta_via_l_arricchimento() {
        // Il difetto: un calcolatore di ReplayGain riscrive il file, la data
        // cambia, la scansione lo rilegge — e tornavano «Album sconosciuto» e
        // la copertina grigia, per sempre, perché per l'arricchimento il brano
        // era già fatto.
        let mut lib = Libreria::nuova();
        let path = lib.musica().join("Bjork/01.wav");
        wav(&path);
        tagga_con(&path, "Joga", "Björk", None, None);
        data(&path, 1_600_000_000);
        lib.scansiona();
        let id = lib.id_del_titolo("Joga");
        arricchisci(
            &mut lib,
            id,
            aether_domain::enrich::Fields {
                album: Some("Homogenic".to_owned()),
                year: Some(1997),
                ..aether_domain::enrich::Fields::default()
            },
        );

        // ── il calcolatore del volume: i tag di prima, più un ReplayGain ──
        tagga_con(&path, "Joga", "Björk", None, Some("-6.50 dB"));
        data(&path, 1_600_000_100);
        assert_eq!(lib.scansiona().updated, 1, "il file è cambiato davvero");

        let (titolo, album, anno, copertina, stato) = come_sta(&lib, id);
        assert_eq!(titolo, "Joga");
        assert_eq!(album, "Homogenic", "l'album trovato resta");
        assert_eq!(anno, Some(1997));
        assert_eq!(copertina.as_deref(), Some("c0ffee"), "e la copertina pure");
        assert_eq!(stato.as_deref(), Some("ok"), "non c'è niente da rifare");
        let annotazione = annotata(&lib, id).expect("l'annotazione resta");
        assert_eq!(annotazione.album.as_deref(), Some("Homogenic"));
        assert_eq!(annotazione.copertina.as_deref(), Some("c0ffee"));
        // L'album raggruppa: il brano sta nell'album trovato, non in quello
        // sconosciuto.
        let (chiave, percorso): (String, String) = lib
            .connection
            .query_row(
                "SELECT album_key, path FROM tracks WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("chiave");
        assert_eq!(chiave, album_group_key("Homogenic", &percorso));

        // ── e poi qualcuno lo ritagga davvero: l'album adesso lo dice il file ──
        tagga_con(&path, "Joga", "Björk", Some("Vespertine"), Some("-6.50 dB"));
        data(&path, 1_600_000_200);
        assert_eq!(lib.scansiona().updated, 1);

        let (_, album, anno, copertina, stato) = come_sta(&lib, id);
        assert_eq!(album, "Vespertine", "la parola più recente è del file");
        assert_eq!(anno, Some(1997), "l'anno il file non l'ha toccato");
        assert_eq!(copertina.as_deref(), Some("c0ffee"));
        assert_eq!(stato, None, "e il brano torna in coda all'arricchimento");
        let annotazione = annotata(&lib, id).expect("l'annotazione resta");
        assert_eq!(
            annotazione.album, None,
            "l'album non viene più dal catalogo"
        );
        assert_eq!(annotazione.year, Some(1997));
    }

    #[test]
    fn un_brano_arricchito_spostato_tiene_quel_che_si_era_trovato() {
        // Un file senza tag, arricchito, poi spostato e rinominato. Il titolo
        // «del file» era il ripiego dal nome, e il nome è cambiato: ma dentro
        // il file non è cambiato niente, e il titolo trovato deve restare.
        let mut lib = Libreria::nuova();
        let da = lib.musica().join("sfusi/traccia 01.wav");
        wav(&da);
        lib.scansiona();
        let id = lib.id_del_titolo("traccia 01");
        arricchisci(
            &mut lib,
            id,
            aether_domain::enrich::Fields {
                title: Some("Hunter".to_owned()),
                album: Some("Homogenic".to_owned()),
                ..aether_domain::enrich::Fields::default()
            },
        );

        let a = lib.musica().join("Björk/Homogenic/01 Hunter.wav");
        std::fs::create_dir_all(a.parent().expect("cartella")).expect("cartelle");
        std::fs::rename(&da, &a).expect("spostamento");
        let esito = lib.scansiona();
        assert_eq!(esito.moved, 1);

        let (titolo, album, _, copertina, stato) = come_sta(&lib, id);
        assert_eq!(titolo, "Hunter");
        assert_eq!(album, "Homogenic");
        assert_eq!(copertina.as_deref(), Some("c0ffee"));
        assert_eq!(stato.as_deref(), Some("ok"));
    }

    #[test]
    fn un_annotazione_della_2_4_si_rimette_dove_il_file_tace() {
        // Le annotazioni scritte prima della 3.0 non sanno cosa ci fosse sotto,
        // né quale fosse la copertina. Dove il file non dice niente si rimette
        // — nessuno l'ha ritaggato —, dove dice altro vince il file e il brano
        // torna in coda. La copertina si riconosce da `cover_art.source`.
        let mut lib = Libreria::nuova();
        let path = lib.musica().join("Bjork/02.wav");
        wav(&path);
        tagga_con(&path, "Joga", "Björk", None, None);
        data(&path, 1_600_000_000);
        lib.scansiona();
        let id = lib.id_del_titolo("Joga");
        lib.connection
            .execute_batch(&format!(
                "INSERT INTO cover_art (hash, mime_type, width, height, byte_size, source, created_at)
                 VALUES ('c0ffee', 'image/jpeg', 1, 1, 1, 'provider', 1);
                 UPDATE tracks SET title = 'Jóga', album = 'Homogenic',
                        cover_art_hash = 'c0ffee', enrich_status = 'ok'
                  WHERE id = {id};
                 INSERT INTO track_meta_arricchita (track_id, campi, fonte, confidenza, set_at)
                 VALUES ({id}, '{{\"title\":\"Jóga\",\"album\":\"Homogenic\"}}', 'mb-release', 0.9, 1);"
            ))
            .expect("un arricchimento della 2.4");

        data(&path, 1_600_000_100);
        assert_eq!(lib.scansiona().updated, 1);

        let (titolo, album, _, copertina, stato) = come_sta(&lib, id);
        assert_eq!(
            album, "Homogenic",
            "il file l'album non ce l'ha: si rimette"
        );
        assert_eq!(titolo, "Joga", "il titolo il file ce l'ha: vince lui");
        assert_eq!(copertina.as_deref(), Some("c0ffee"));
        assert_eq!(stato, None, "un campo ritaggato rimette in coda");
        let annotazione = annotata(&lib, id).expect("l'annotazione resta");
        assert_eq!(annotazione.title, None);
        assert_eq!(annotazione.album.as_deref(), Some("Homogenic"));
        assert_eq!(
            annotazione.copertina.as_deref(),
            Some("c0ffee"),
            "e adesso la copertina la ricorda"
        );
    }

    #[test]
    fn un_file_ritaggato_con_la_data_ferma_si_rilegge() {
        // Mp3tag con «conserva la data di modifica»: stessa data, un titolo
        // più lungo, un file più grande. Il piano guardava solo la data.
        let mut lib = Libreria::nuova();
        let path = lib.brano("A/Al/01.wav", "Corto", "Art", "Al");
        data(&path, 1_600_000_000);
        lib.scansiona();

        tagga(&path, "Un titolo molto più lungo di prima", "Art", "Al");
        data(&path, 1_600_000_000);
        let esito = lib.scansiona();
        assert_eq!(esito.updated, 1, "la dimensione è cambiata");
        let titolo: String = lib
            .connection
            .query_row("SELECT title FROM tracks", [], |r| r.get(0))
            .expect("titolo");
        assert_eq!(titolo, "Un titolo molto più lungo di prima");
    }

    #[test]
    fn rileggere_tutti_i_tag_rilegge_ogni_file_una_volta() {
        let mut lib = Libreria::nuova();
        lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
        lib.brano("A/Al/02.wav", "Due", "Art", "Al");
        lib.scansiona();
        assert_eq!(lib.scansiona().updated, 0, "niente è cambiato");

        assert_eq!(rileggi_tutti_i_tag(&lib.connection).expect("richiesta"), 2);
        assert_eq!(lib.scansiona().updated, 2, "tutti riletti");
        // E la sentinella non resta: la rilettura ha riscritto le date vere.
        assert_eq!(lib.scansiona().updated, 0);
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

    /// Le regole dei percorsi delle prove: Windows e macOS ignorano le maiuscole.
    const REGOLE: PathRules = PathRules {
        case_insensitive: true,
    };

    #[test]
    fn cancellare_un_brano_se_ne_porta_dietro_l_album_e_l_artista() {
        let mut lib = Libreria::nuova();
        lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
        let secondo = lib.brano("B/Bl/01.wav", "Due", "Art2", "Bl");
        lib.scansiona();
        assert_eq!(lib.conta("albums"), 2);
        let id = lib.id_del_titolo("Due");
        let radici = vec![lib.root()];

        let esito =
            cancella_brani(&mut lib.connection, &[id], &radici, REGOLE).expect("cancellazione");

        assert_eq!(esito.tolti, 1);
        assert_eq!(lib.conta("tracks"), 1);
        // Gli aggregati nella stessa transazione: senza, resterebbe un album
        // senza brani e un artista senza album.
        assert_eq!(lib.conta("albums"), 1);
        assert_eq!(lib.conta("artists"), 1);
        // Il file non si tocca: questa funzione non sa nemmeno che esiste un
        // disco.
        assert!(secondo.exists(), "il file deve essere ancora al suo posto");
    }

    #[test]
    fn cancellare_a_mano_non_scrive_lapidi() {
        // Per la stessa ragione della scansione, detta dall'altro lato:
        // `lapidi.brani` non la consulta nessuno, e una riga che non protegge
        // niente è stato morto.
        let mut lib = Libreria::nuova();
        lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
        lib.scansiona();
        let id = lib.id_del_titolo("Uno");
        let radici = vec![lib.root()];

        cancella_brani(&mut lib.connection, &[id], &radici, REGOLE).expect("cancellazione");

        assert_eq!(lib.conta("tracks"), 0);
        assert_eq!(lib.conta("sync_tombstones"), 0);
    }

    #[test]
    fn cancellare_dice_quanti_torneranno_alla_prossima_scansione() {
        let mut lib = Libreria::nuova();
        lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
        lib.scansiona();
        let id = lib.id_del_titolo("Uno");

        // Sorvegliata: il file è ancora lì, e la scansione dopo lo rimette.
        let radici = vec![lib.root()];
        let esito =
            cancella_brani(&mut lib.connection, &[id], &radici, REGOLE).expect("cancellazione");
        assert_eq!(esito.tolti, 1);
        assert_eq!(esito.torneranno, 1);
        lib.scansiona();
        assert_eq!(
            lib.conta("tracks"),
            1,
            "torna davvero, non per modo di dire"
        );

        // Fuori da ogni cartella sorvegliata — una radice tolta, un disco
        // staccato — non torna nessuno, e non lo si promette.
        let id = lib.id_del_titolo("Uno");
        let esito = cancella_brani(&mut lib.connection, &[id], &[], REGOLE).expect("cancellazione");
        assert_eq!(esito.tolti, 1);
        assert_eq!(esito.torneranno, 0);
    }

    #[test]
    fn cancellare_un_brano_lo_toglie_dalle_playlist() {
        let mut lib = Libreria::nuova();
        lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
        lib.scansiona();
        let id = lib.id_del_titolo("Uno");
        lib.connection
            .execute_batch(
                "INSERT INTO playlists (id, playlist_key, name, created_at, updated_at)
                 VALUES (1, 'p', 'P', 1, 1);",
            )
            .expect("playlist");
        lib.connection
            .execute(
                "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (1, ?1, 0)",
                [id],
            )
            .expect("riga di playlist");

        cancella_brani(&mut lib.connection, &[id], &[], REGOLE).expect("cancellazione");

        // La cascata, che le chiavi esterne accese rendono possibile.
        assert_eq!(lib.conta("playlist_tracks"), 0);
        // La playlist resta: si è cancellato un brano, non un elenco.
        assert_eq!(lib.conta("playlists"), 1);
    }

    #[test]
    fn cancellare_quel_che_non_c_e_non_e_un_guasto() {
        // Il clic su un elenco cambiato sotto: non si tocca niente, e non si
        // rifanno gli aggregati per niente.
        let mut lib = Libreria::nuova();
        lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
        lib.scansiona();

        let esito =
            cancella_brani(&mut lib.connection, &[4_242], &[], REGOLE).expect("cancellazione");

        assert_eq!(esito.tolti, 0);
        assert_eq!(esito.torneranno, 0);
        assert_eq!(lib.conta("tracks"), 1);
        assert_eq!(lib.conta("albums"), 1);
    }

    /// Più identificativi di quanti parametri SQLite accetti in una query
    /// sola (32 766): quelli veri stanno **in fondo**, dopo il limite, dove
    /// un'unica `IN (…)` non arrivava nemmeno a essere preparata.
    fn oltre_il_limite_dei_parametri(veri: &[i64]) -> Vec<i64> {
        let mut ids: Vec<i64> = (1_000_000..1_040_000).collect();
        ids.extend_from_slice(veri);
        ids
    }

    #[test]
    fn una_coda_di_quarantamila_brani_si_legge_lo_stesso() {
        let mut lib = Libreria::nuova();
        lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
        lib.brano("B/Bl/01.wav", "Due", "Art2", "Bl");
        lib.scansiona();
        let uno = lib.id_del_titolo("Uno");
        let due = lib.id_del_titolo("Due");

        let ids = oltre_il_limite_dei_parametri(&[due, uno, due]);
        let righe = summaries_by_id(&lib.connection, &ids).expect("lettura a pezzi");

        // L'ordine chiesto, doppioni compresi, e niente per chi non esiste.
        let titoli: Vec<&str> = righe.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titoli, ["Due", "Uno", "Due"]);
    }

    #[test]
    fn cancellare_quarantamila_brani_non_supera_il_limite_dei_parametri() {
        let mut lib = Libreria::nuova();
        lib.brano("A/Al/01.wav", "Uno", "Art", "Al");
        lib.brano("B/Bl/01.wav", "Due", "Art2", "Bl");
        lib.scansiona();
        let ids = oltre_il_limite_dei_parametri(&[lib.id_del_titolo("Uno")]);

        let esito = cancella_brani(&mut lib.connection, &ids, &[], REGOLE).expect("cancellazione");

        assert_eq!(esito.tolti, 1);
        assert_eq!(lib.conta("tracks"), 1);
        assert_eq!(lib.conta("albums"), 1);
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
        /// I rami che la camminata di una radice perde, per nome: la
        /// sottocartella a permessi negati sul disco di casa.
        rami_persi: std::collections::HashMap<String, Vec<String>>,
        /// I file che fanno cadere il lettore dei tag: il file malformato su
        /// cui `lofty` panica.
        malformati: std::collections::HashSet<String>,
    }

    impl FintoDisco {
        fn nuovo() -> Self {
            Self {
                rami_persi: std::collections::HashMap::new(),
                malformati: std::collections::HashSet::new(),
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

        /// La camminata su questa radice perde `ramo`, e lo dice: parziale,
        /// con l'indirizzo del pezzo mancante.
        fn perde_il_ramo(mut self, root: &str, ramo: &str) -> Self {
            self.monche.insert(root.to_owned());
            self.rami_persi
                .entry(root.to_owned())
                .or_default()
                .push(ramo.to_owned());
            self
        }

        /// Aprire questo file fa cadere il lettore.
        fn malformato(mut self, path: &str) -> Self {
            self.malformati.insert(path.to_owned());
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
            su_voce: &mut dyn FnMut(crate::files::Voce) -> ControlFlow<()>,
        ) -> Result<bool, AppError> {
            self.guarda_il_lucchetto();
            for file in self.trovati.get(root).cloned().unwrap_or_default() {
                if su_voce(crate::files::Voce::File(file)).is_break() {
                    return Ok(false);
                }
            }
            for ramo in self.rami_persi.get(root).cloned().unwrap_or_default() {
                if su_voce(crate::files::Voce::RamoPerso(ramo)).is_break() {
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
            assert!(
                !self.malformati.contains(path),
                "intestazione malformata: {path}"
            );
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
    /// Una riga come la scrive una scansione di oggi: con la `content_key`, senza
    /// la quale il piano la rileggerebbe (vedi `known_tracks`).
    ///
    /// La dimensione è quella di [`trovato`]: da quando il piano confronta anche
    /// quella, una dimensione diversa farebbe rileggere un file «invariato».
    fn riga(connection: &Connection, id: i64, path: &str, modified_ms: i64) {
        connection
            .execute(
                "INSERT INTO tracks
                     (id, path, track_key, content_key, title, artist, album, duration_ms,
                      file_size, date_added, date_modified)
                 VALUES (?1, ?2, ?3, ?4, 'T', 'A', 'Al', 1000, 65536, 0, ?5)",
                rusqlite::params![
                    id,
                    path,
                    format!("a|t{id}|al"),
                    format!("c{id}"),
                    modified_ms
                ],
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
        // una share morta. Lì la sonda risponde, e la scansione deve andare
        // avanti — altrimenti la protezione diventa «non si cancella più
        // niente», che è un altro modo di avere una libreria sbagliata.
        //
        // Avanti **sul resto della radice**: il ramo perso è detto per nome, e
        // solo quel che ci sta sotto resta non verificato.
        let (_dir, store, connection) = banco();
        riga(&connection, 1, r"C:\musica\c-e.mp3", 10);
        riga(&connection, 2, r"C:\musica\non-c-e-piu.mp3", 10);
        riga(&connection, 3, r"C:\musica\Negata\dietro-la-porta.mp3", 10);

        let disco = Arc::new(
            FintoDisco::nuovo()
                .con_file(r"C:\musica", vec![trovato(r"C:\musica\c-e.mp3", 10)])
                .perde_il_ramo(r"C:\musica", r"C:\musica\Negata"),
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
        let tolte: Vec<i64> = esplorazione
            .piano
            .to_remove
            .iter()
            .map(|r| r.track.id)
            .collect();
        assert_eq!(
            tolte,
            vec![2],
            "la radice risponde: il brano che non c'è più va tolto, quello dietro la porta no"
        );
        assert_eq!(
            esplorazione.rami_persi,
            vec![r"C:\musica\Negata".to_owned()]
        );
        assert_eq!(esplorazione.non_verificati, 1);
        assert_eq!(esplorazione.piano.untouched, 1, "risparmiato, e contato");
    }

    #[test]
    fn una_camminata_monca_che_non_dice_dove_non_toglie_niente_dalla_radice() {
        // Chi si dichiara parziale senza dire dove ha rotto il patto di
        // `walk_a_rate`: lo si prende in parola sulla sola cosa che ha detto, e
        // il ramo perso è la radice intera. La sonda risponde, quindi la radice
        // non è saltata — si inserisce e si aggiorna — ma non si toglie niente.
        let (_dir, store, connection) = banco();
        riga(&connection, 1, r"C:\musica\c-e.mp3", 10);
        riga(&connection, 2, r"C:\musica\forse-c-e.mp3", 10);

        let disco = Arc::new(
            FintoDisco::nuovo()
                .con_file(
                    r"C:\musica",
                    vec![
                        trovato(r"C:\musica\c-e.mp3", 10),
                        trovato(r"C:\musica\nuovo.mp3", 10),
                    ],
                )
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
        assert!(esplorazione.piano.to_remove.is_empty());
        assert_eq!(
            esplorazione.piano.to_insert.len(),
            1,
            "il nuovo entra lo stesso"
        );
        assert_eq!(esplorazione.rami_persi, vec![r"C:\musica".to_owned()]);
    }

    #[test]
    fn una_sottocartella_illeggibile_non_si_porta_via_brani_e_cronologia() {
        // Il guasto verificato a mano: la radice risponde, una sottocartella
        // no, e una scansione a mano — `prudente: false` — toglieva ogni brano
        // là sotto. La cascata si portava via cronologia, playlist, voti e
        // preferiti. Qui la scansione corre per intero, fino alle rimozioni.
        let (_dir, store, mut connection) = banco();
        riga(&connection, 1, r"C:\musica\c-e.mp3", 10);
        riga(&connection, 2, r"C:\musica\Negata\uno.mp3", 10);
        riga(&connection, 3, r"C:\musica\Negata\Sotto\due.mp3", 10);
        riga(&connection, 4, r"C:\musica\non-c-e-piu.mp3", 10);
        connection
            .execute_batch(
                "UPDATE tracks SET play_count = 7, rating = 4, liked = 1 WHERE id = 2;
                 INSERT INTO play_history (track_id, played_at, ms_played) VALUES (2, 1000, 5000);
                 INSERT INTO play_history (track_id, played_at, ms_played) VALUES (3, 2000, 5000);",
            )
            .expect("storia");

        let disco = Arc::new(
            FintoDisco::nuovo()
                .con_file(r"C:\musica", vec![trovato(r"C:\musica\c-e.mp3", 10)])
                .perde_il_ramo(r"C:\musica", r"C:\musica\Negata"),
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
            .run(&mut connection, |_, _| ControlFlow::Continue(()))
            .expect("scansione");

        assert_eq!(report.removed, 1, "solo quello che è sparito davvero");
        assert_eq!(report.non_verificati, 2);
        assert_eq!(report.rami_persi, vec![r"C:\musica\Negata".to_owned()]);
        let rimasti: Vec<i64> = {
            let mut statement = connection
                .prepare("SELECT id FROM tracks ORDER BY id")
                .expect("query");
            statement
                .query_map([], |r| r.get(0))
                .expect("righe")
                .collect::<Result<_, _>>()
                .expect("id")
        };
        assert_eq!(rimasti, vec![1, 2, 3]);
        let (ascolti, voto, cuore): (i64, i64, i64) = connection
            .query_row(
                "SELECT play_count, rating, liked FROM tracks WHERE id = 2",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("statistiche");
        assert_eq!((ascolti, voto, cuore), (7, 4, 1), "né tolte né riscritte");
        let cronologia: i64 = connection
            .query_row("SELECT COUNT(*) FROM play_history", [], |r| r.get(0))
            .expect("cronologia");
        assert_eq!(cronologia, 2, "la cronologia dietro la porta è ancora lì");
    }

    #[test]
    fn tre_file_malformati_di_fila_non_fanno_abbandonare_la_radice() {
        // Il lettore dei tag che panica su un file malformato tornava lo
        // stesso `None` di una lettura scaduta, e contava fra le scadenze di
        // fila: tre file rotti in una cartella del disco di casa, e la radice
        // veniva abbandonata come «rete non disponibile».
        let (_dir, store, mut connection) = banco();
        let disco = Arc::new(
            FintoDisco::nuovo()
                .con_file(
                    r"C:\musica",
                    vec![
                        trovato(r"C:\musica\rotto-1.mp3", 10),
                        trovato(r"C:\musica\rotto-2.mp3", 10),
                        trovato(r"C:\musica\rotto-3.mp3", 10),
                        trovato(r"C:\musica\rotto-4.mp3", 10),
                        trovato(r"C:\musica\dopo.mp3", 10),
                    ],
                )
                .malformato(r"C:\musica\rotto-1.mp3")
                .malformato(r"C:\musica\rotto-2.mp3")
                .malformato(r"C:\musica\rotto-3.mp3")
                .malformato(r"C:\musica\rotto-4.mp3"),
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

        let prima = Instant::now();
        let report = scan
            .run(&mut connection, |_, _| ControlFlow::Continue(()))
            .expect("scansione");

        assert!(
            report.radici_saltate.is_empty(),
            "una radice di file rotti non è una radice morta"
        );
        assert_eq!(
            disco.aperture(),
            5,
            "si prova ogni file, anche dopo i rotti"
        );
        assert_eq!(report.unreadable.len(), 5);
        let cadute = report
            .unreadable
            .iter()
            .filter(|u| u.error.code().kind() == ErrorCodeKind::MetadataTagReadFailed)
            .count();
        assert_eq!(
            cadute, 4,
            "i quattro rotti si dicono file illeggibili, non rete"
        );
        // E nessuno ha aspettato la scadenza di lettura: una caduta si
        // riconosce subito.
        assert!(
            prima.elapsed() < LETTURA_BRANO,
            "una caduta ha aspettato la scadenza: {:?}",
            prima.elapsed()
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
        assert!(
            esito.rimozioni_in_attesa.is_empty(),
            "nessuno può confermare: l'elenco non serve a nessuno"
        );
        let rimasti: i64 = connection
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
            .expect("conteggio");
        assert_eq!(rimasti, 60, "le righe sono ancora tutte lì");
    }

    #[test]
    fn una_scansione_a_mano_chiede_prima_di_togliere_mezza_libreria() {
        // La stessa strage, chiesta a mano. Fino alla 2.4 toglieva senza
        // chiedere: chi guardava l'esito leggeva «60 tolti» a cose fatte. Ora
        // non toglie niente, finisce il resto del lavoro, e consegna l'elenco
        // a chi può dire sì.
        let (_dir, store, mut connection) = banco();
        for id in 1..=60 {
            riga(&connection, id, &format!(r"C:\musica\{id}.mp3"), 10);
        }
        riga(&connection, 61, r"D:\altro\resta.mp3", 10);

        let disco = Arc::new(FintoDisco::nuovo().con_file(
            r"D:\altro",
            vec![
                trovato(r"D:\altro\resta.mp3", 10),
                trovato(r"D:\altro\nuovo.mp3", 10),
            ],
        ));
        let roots = vec![r"C:\musica".to_owned(), r"D:\altro".to_owned()];
        let a_mano = Scan {
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

        let esito = a_mano
            .run(&mut connection, |_, _| ControlFlow::Continue(()))
            .expect("scansione");
        assert_eq!(esito.removed, 0, "senza un sì non si toglie niente");
        assert_eq!(esito.rimozioni_rinviate, 60);
        assert_eq!(esito.rimozioni_in_attesa.len(), 60);
        // Il resto della passata è andato avanti: la radice sana si legge.
        // (Il finto disco non apre file: il nuovo è tentato e resta illeggibile.)
        assert_eq!(
            disco.aperture(),
            1,
            "il file nuovo dell'altra radice è stato tentato"
        );

        // Quel che la finestra mostra: una voce per radice, con gli esempi.
        let riassunto = riassumi_rimozioni(
            &esito.rimozioni_in_attesa,
            &roots,
            PathRules {
                case_insensitive: true,
            },
            3,
        );
        assert_eq!(riassunto.len(), 1);
        let voce = riassunto.first().expect("una voce");
        assert_eq!(voce.radice, r"C:\musica");
        assert_eq!(voce.quante, 60);
        assert_eq!(voce.esempi.len(), 3);

        // Il sì: si tolgono esattamente quelle, senza ricamminare il disco.
        let camminate_prima = disco.sonde.load(std::sync::atomic::Ordering::Relaxed);
        let tolte =
            applica_rimozioni(&mut connection, &esito.rimozioni_in_attesa).expect("conferma");
        assert_eq!(tolte, 60);
        assert_eq!(
            disco.sonde.load(std::sync::atomic::Ordering::Relaxed),
            camminate_prima,
            "confermare non guarda il disco"
        );
        let rimasti: i64 = connection
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
            .expect("conteggio");
        assert_eq!(rimasti, 1, "resta solo quello dell'altra radice");
    }

    #[test]
    fn una_conferma_arrivata_tardi_non_tocca_una_riga_cambiata() {
        // Fra la domanda e il sì passa il tempo di leggere una finestrella, e
        // in quel tempo la riga 1 può essere diventata un'altra: si toglie per
        // identificativo **e** percorso, e quella non si riconosce più.
        let (_dir, _store, mut connection) = banco();
        riga(&connection, 1, r"C:\musica\a.mp3", 10);
        riga(&connection, 2, r"C:\musica\b.mp3", 10);
        let elenco = vec![
            RimozioneInAttesa {
                id: 1,
                path: r"C:\musica\a.mp3".to_owned(),
            },
            RimozioneInAttesa {
                id: 2,
                path: r"C:\musica\b.mp3".to_owned(),
            },
        ];
        connection
            .execute(
                "UPDATE tracks SET path = ?1 WHERE id = 1",
                [r"C:\musica\rinominato.mp3"],
            )
            .expect("rinomina");
        let tolte = applica_rimozioni(&mut connection, &elenco).expect("conferma");
        assert_eq!(tolte, 1);
        let rimasto: String = connection
            .query_row("SELECT path FROM tracks", [], |r| r.get(0))
            .expect("rimasto");
        assert_eq!(rimasto, r"C:\musica\rinominato.mp3");
    }

    // ── la migrazione 028, trigger per trigger ──────────────────────────────

    /// Una riga di brano con la `content_key` scelta da chi prova.
    fn riga_con_chiave(connection: &Connection, id: i64, path: &str, chiave: &str) {
        connection
            .execute(
                "INSERT INTO tracks
                     (id, path, track_key, content_key, title, artist, album, duration_ms,
                      file_size, date_added, date_modified)
                 VALUES (?1, ?2, 'a|t|al', ?3, 'T', 'A', 'Al', 1000, 65536, 0, 10)",
                rusqlite::params![id, path, chiave],
            )
            .expect("riga di brano");
    }

    fn conta(connection: &Connection, tabella: &str) -> i64 {
        connection
            .query_row(&format!("SELECT COUNT(*) FROM {tabella}"), [], |r| r.get(0))
            .expect("conteggio")
    }

    #[test]
    fn un_brano_senza_storia_non_lascia_orfani() {
        // Solo chi ha qualcosa da perdere: una cartella tolta con diecimila
        // brani mai ascoltati non deve scrivere diecimila righe vuote.
        let (_dir, _store, connection) = banco();
        riga_con_chiave(&connection, 1, r"C:\m\a.mp3", "k1");
        connection
            .execute("DELETE FROM tracks WHERE id = 1", [])
            .expect("tolto");
        assert_eq!(conta(&connection, "brani_orfani"), 0);
    }

    #[test]
    fn il_brano_che_torna_va_in_coda_se_il_suo_posto_e_stato_preso() {
        let (_dir, _store, mut connection) = banco();
        riga_con_chiave(&connection, 1, r"C:\m\a.mp3", "ka");
        riga_con_chiave(&connection, 2, r"C:\m\b.mp3", "kb");
        riga_con_chiave(&connection, 3, r"C:\m\c.mp3", "kc");
        let lista = crate::playlists::create(&connection, "Lista")
            .expect("playlist")
            .id;
        crate::playlists::add_tracks(&mut connection, lista, &[1, 2, 3]).expect("aggiunti");
        connection
            .execute("DELETE FROM tracks WHERE id = 1", [])
            .expect("tolto");
        // Un riordino riscrive le posizioni da zero: il buco dove stava «a» è
        // stato preso da «b».
        crate::playlists::reorder(&mut connection, lista, 1, 0).expect("riordinata");

        riga_con_chiave(&connection, 4, r"C:\m\a.mp3", "ka");
        let ordine: Vec<i64> = {
            let mut statement = connection
                .prepare(
                    "SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position",
                )
                .expect("query");
            statement
                .query_map([lista], |r| r.get(0))
                .expect("righe")
                .collect::<Result<_, _>>()
                .expect("ordine")
        };
        assert_eq!(
            ordine,
            vec![3, 2, 4],
            "in fondo, non sopra un altro e non fuori"
        );
    }

    #[test]
    fn due_copie_dello_stesso_file_si_fondono() {
        // Lo stesso album in due cartelle, tolte tutte e due: una voce sola,
        // col conteggio più alto, il voto più recente e tutti gli ascolti.
        let (_dir, _store, connection) = banco();
        riga_con_chiave(&connection, 1, r"C:\m\a.mp3", "k");
        riga_con_chiave(&connection, 2, r"D:\copia\a.mp3", "k");
        connection
            .execute_batch(
                "UPDATE tracks SET play_count = 3, rating = 2, stats_updated_at = 100 WHERE id = 1;
                 UPDATE tracks SET play_count = 9, rating = 5, stats_updated_at = 50 WHERE id = 2;
                 INSERT INTO play_history (track_id, played_at, ms_played) VALUES (1, 10, 1);
                 INSERT INTO play_history (track_id, played_at, ms_played) VALUES (2, 20, 1);
                 DELETE FROM tracks;",
            )
            .expect("tolte");
        let (quante, voto): (i64, i64) = connection
            .query_row(
                "SELECT play_count, rating FROM brani_orfani WHERE content_key = 'k'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("orfano");
        assert_eq!(quante, 9, "il conteggio più alto");
        assert_eq!(voto, 2, "il voto con l'orologio più recente");
        assert_eq!(conta(&connection, "ascolti_orfani"), 2);
    }

    #[test]
    fn un_orfano_scaduto_non_torna_e_se_ne_va() {
        let (_dir, _store, connection) = banco();
        riga_con_chiave(&connection, 1, r"C:\m\a.mp3", "vecchio");
        riga_con_chiave(&connection, 2, r"C:\m\b.mp3", "recente");
        connection
            .execute_batch(
                "UPDATE tracks SET play_count = 4;
                 DELETE FROM tracks WHERE id = 1;",
            )
            .expect("tolto");
        // Novantun giorni fa.
        connection
            .execute(
                "UPDATE brani_orfani
                    SET orfano_at = (CAST(strftime('%s', 'now') AS INTEGER) - 91 * 86400) * 1000",
                [],
            )
            .expect("invecchiato");

        // Torna dopo la scadenza: niente statistiche, e la voce se ne va.
        riga_con_chiave(&connection, 3, r"C:\m\a.mp3", "vecchio");
        let quante: i64 = connection
            .query_row("SELECT play_count FROM tracks WHERE id = 3", [], |r| {
                r.get(0)
            })
            .expect("riga");
        assert_eq!(quante, 0, "scaduto non torna");
        assert_eq!(conta(&connection, "brani_orfani"), 0);

        // E la pulizia passa anche da chi se ne va: una voce scaduta sparisce
        // alla prima rimozione successiva, qualunque sia.
        connection
            .execute("DELETE FROM tracks WHERE id = 3", [])
            .expect("tolto");
        connection
            .execute(
                "INSERT INTO brani_orfani (content_key, play_count, orfano_at)
                 VALUES ('dimenticato', 1, 0)",
                [],
            )
            .expect("orfano antico");
        connection
            .execute("DELETE FROM tracks WHERE id = 2", [])
            .expect("tolto");
        let chiavi: Vec<String> = {
            let mut statement = connection
                .prepare("SELECT content_key FROM brani_orfani ORDER BY content_key")
                .expect("query");
            statement
                .query_map([], |r| r.get(0))
                .expect("righe")
                .collect::<Result<_, _>>()
                .expect("chiavi")
        };
        assert_eq!(chiavi, vec!["recente".to_owned()]);
    }

    #[test]
    fn un_brano_di_catalogo_non_lascia_orfani() {
        // Nessuna scansione lo farà tornare: tenerne la storia novanta giorni
        // vorrebbe dire tenerla per nessuno.
        let (_dir, _store, connection) = banco();
        connection
            .execute(
                "INSERT INTO tracks
                     (id, path, track_key, content_key, title, artist, album, duration_ms,
                      date_added, play_count, source, source_service, fonte_url,
                      licenza, disponibilita)
                 VALUES (1, NULL, 'a|t|al', 'k', 'T', 'A', 'Al', 1000, 0, 3,
                         'catalogo', 'audius', '/v1/tracks/1/stream',
                         'cc-by', 'soloAscolto')",
                [],
            )
            .expect("brano di catalogo");
        connection
            .execute("DELETE FROM tracks WHERE id = 1", [])
            .expect("tolto");
        assert_eq!(conta(&connection, "brani_orfani"), 0);
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

    // ── i dati tecnici del file ─────────────────────────────────────────────

    /// Una riga di `tracks` con i quattro campi tecnici messi a mano.
    ///
    /// A mano e non per scansione: la scansione sa scrivere solo quel che
    /// `lofty` legge da un WAV generato dalle prove, cioè un caso, mentre qui
    /// servono i casi che in libreria ci sono davvero — un «Mpeg», un nome che
    /// questa versione non conosce, e la riga con tutto a `NULL`.
    fn riga_tecnica(
        connection: &Connection,
        id: i64,
        codec: Option<&str>,
        sample_rate: Option<i64>,
        channels: Option<i64>,
        bitrate: Option<i64>,
    ) {
        connection
            .execute(
                "INSERT INTO tracks
                   (id, path, track_key, title, artist, album, file_size,
                    date_added, date_modified, codec, sample_rate, channels, bitrate)
                 VALUES (?1, ?2, 'k', 'T', 'A', 'D', 1000, 1, 1, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    id,
                    format!("C:/M/{id}.x"),
                    codec,
                    sample_rate,
                    channels,
                    bitrate
                ],
            )
            .expect("riga di prova");
    }

    #[test]
    fn il_formato_legge_i_quattro_campi() {
        let db = crate::db::open_in_memory().expect("database");
        riga_tecnica(
            &db.connection,
            1,
            Some("Flac"),
            Some(44_100),
            Some(2),
            Some(1058),
        );

        let formato = read_formato(&db.connection, 1)
            .expect("lettura")
            .expect("la riga c'è");
        assert_eq!(formato.codec.as_deref(), Some("FLAC"));
        assert_eq!(formato.sample_rate, Some(44_100));
        assert_eq!(formato.channels, Some(2));
        assert_eq!(formato.bitrate, Some(1058));
    }

    #[test]
    fn il_formato_degrada_sui_nulli() {
        // Il caso della libreria scansionata quando quelle colonne non si
        // scrivevano: la riga c'è, i dati no. `Some` con tutto vuoto e non
        // `None`, perché le due cose vogliono dire cose diverse — «non so
        // com'è fatto» contro «questo brano non c'è più».
        let db = crate::db::open_in_memory().expect("database");
        riga_tecnica(&db.connection, 7, None, None, None, None);

        let formato = read_formato(&db.connection, 7)
            .expect("lettura")
            .expect("la riga c'è comunque");
        assert_eq!(formato.codec, None);
        assert_eq!(formato.sample_rate, None);
        assert_eq!(formato.channels, None);
        assert_eq!(formato.bitrate, None);
    }

    #[test]
    fn il_formato_di_un_brano_che_non_ce_piu_e_none() {
        // La coda tiene gli identificativi, non le righe: un brano tolto dalla
        // libreria mentre suonava arriva qui come un `id` che non esiste, e
        // `None` è la risposta giusta — non un errore da mostrare.
        let db = crate::db::open_in_memory().expect("database");
        assert_eq!(read_formato(&db.connection, 404).expect("lettura"), None);
    }

    fn suonato(
        codec: Option<&'static str>,
        kbps: Option<u32>,
        remoto: bool,
    ) -> aether_play::FormatoSuonato {
        aether_play::FormatoSuonato {
            codec,
            frequenza: 44_100,
            bit: Some(24),
            canali: 2,
            frequenza_uscita: 48_000,
            kbps,
            remoto,
        }
    }

    fn dal_database(codec: &str, bitrate: Option<i64>) -> FormatoFile {
        FormatoFile {
            codec: Some(codec.to_owned()),
            sample_rate: Some(48_000),
            channels: Some(1),
            bitrate,
            bits_per_sample: None,
            output_rate: None,
        }
    }

    #[test]
    fn il_formato_suonato_corregge_codec_frequenza_e_canali() {
        // Un M4A che dentro è ALAC: la colonna dice il contenitore, il
        // decodificatore il codec. Frequenza e canali sono quelli che escono.
        let unito =
            dal_database("M4A", Some(900)).completato_da(&suonato(Some("ALAC"), Some(950), false));
        assert_eq!(unito.codec.as_deref(), Some("ALAC"));
        assert_eq!(unito.sample_rate, Some(44_100));
        assert_eq!(unito.channels, Some(2));
        assert_eq!(unito.bits_per_sample, Some(24));
        assert_eq!(unito.output_rate, Some(48_000));
        // Il file è locale: vince la misura di `lofty`, senza i tag.
        assert_eq!(unito.bitrate, Some(900));
    }

    #[test]
    fn su_un_flusso_il_bitrate_e_quello_che_arriva() {
        // Un FLAC dell'Internet Archive ascoltato dal derivato MP3: la colonna
        // descrive il file del catalogo, la rete porta l'altro.
        let unito =
            dal_database("FLAC", Some(1000)).completato_da(&suonato(Some("MP3"), Some(192), true));
        assert_eq!(unito.codec.as_deref(), Some("MP3"));
        assert_eq!(unito.bitrate, Some(192));
    }

    #[test]
    fn un_codec_che_il_motore_non_nomina_lascia_quello_del_database() {
        let unito = dal_database("Musepack", None).completato_da(&suonato(None, Some(180), false));
        assert_eq!(unito.codec.as_deref(), Some("Musepack"));
        // Il database tace: meglio il conto del motore che niente.
        assert_eq!(unito.bitrate, Some(180));
    }

    #[test]
    fn etichetta_codec_traduce_mpeg_in_mp3() {
        // I tre nomi che la mappa esiste per correggere: sono i soli in cui il
        // `Debug` di `lofty::FileType` non somiglia a come il formato si
        // chiama. «Mp4» diventa il contenitore e non il codec, perché dentro
        // un M4A ci può stare ALAC.
        assert_eq!(etichetta_codec("Mpeg"), "MP3");
        assert_eq!(etichetta_codec("Mp4"), "M4A");
        assert_eq!(etichetta_codec("Vorbis"), "Ogg Vorbis");
        assert_eq!(etichetta_codec("Flac"), "FLAC");
    }

    #[test]
    fn un_codec_sconosciuto_resta_scritto_come_sta() {
        // `FileType` è `#[non_exhaustive]`: questa prova è il contratto con le
        // versioni future di `lofty`. Un contenitore nuovo si mostra col nome
        // della variante invece di sparire, e chi guarda legge qualcosa di
        // brutto ma vero.
        assert_eq!(etichetta_codec("Dsf"), "Dsf");
        assert_eq!(etichetta_codec("Custom(\"tta\")"), "Custom(\"tta\")");
    }

    /// Una scansione limitata a una cartella, come la fa la sorveglianza.
    fn scansiona_soltanto(lib: &mut Libreria, cartelle: &[String]) -> ScanReport {
        let scan = Scan {
            files: Arc::new(LocalFiles),
            covers: lib.store.clone(),
            roots: cartelle,
            rules: PathRules::for_current_platform(),
            prudente: true,
            scadenze: Scadenze::default(),
            fermati: None,
        };
        scan.run(&mut lib.connection, |_, _| ControlFlow::Continue(()))
            .expect("scansione di un sottoalbero")
    }

    #[test]
    fn un_sottoalbero_non_fa_sparire_il_resto_della_radice() {
        // La garanzia di cui la sorveglianza vive: si riscansiona l'album che
        // è cambiato, e i brani del resto della radice — anche quelli il cui
        // file nel frattempo è sparito davvero — non sono affar suo.
        let mut lib = Libreria::nuova();
        let a = lib.brano("Uno/A/01.wav", "A1", "Uno", "A");
        lib.brano("Uno/A/02.wav", "A2", "Uno", "A");
        lib.brano("Due/B/01.wav", "B1", "Due", "B");
        assert_eq!(lib.scansiona().inserted, 3);

        // Sparisce un file di A, e arriva un file nuovo in B.
        std::fs::remove_file(&a).expect("via un file");
        lib.brano("Due/B/02.wav", "B2", "Due", "B");
        let b = lib.musica().join("Due").join("B");
        let esito = scansiona_soltanto(&mut lib, &[b.to_string_lossy().into_owned()]);
        assert_eq!(esito.inserted, 1, "il brano nuovo di B entra");
        assert_eq!(
            esito.removed, 0,
            "il file sparito di A non è di questa passata"
        );
        assert_eq!(lib.conta("tracks"), 4);

        // Riguardata A, se ne va.
        let a = lib.musica().join("Uno").join("A");
        let esito = scansiona_soltanto(&mut lib, &[a.to_string_lossy().into_owned()]);
        assert_eq!(esito.removed, 1);
        assert_eq!(lib.conta("tracks"), 3);
        assert_eq!(esito.aggregates.albums, 2);
    }

    #[test]
    fn un_sottoalbero_sparito_non_toglie_niente() {
        // La cartella da riguardare non c'è più: la scansione la salta come
        // una radice che non risponde. È il motivo per cui
        // `cartelle_da_riguardare` risale fino a una cartella che esiste.
        let mut lib = Libreria::nuova();
        lib.brano("Uno/A/01.wav", "A1", "Uno", "A");
        assert_eq!(lib.scansiona().inserted, 1);
        let a = lib.musica().join("Uno").join("A");
        std::fs::remove_dir_all(&a).expect("via la cartella");
        let esito = scansiona_soltanto(&mut lib, &[a.to_string_lossy().into_owned()]);
        assert_eq!(esito.removed, 0);
        assert_eq!(esito.radici_saltate.len(), 1);
        // Dalla cartella che la conteneva, invece, la sparizione si vede.
        let uno = lib.musica().join("Uno");
        let cartelle = cartelle_da_riguardare(
            &[a.join("01.wav").to_string_lossy().into_owned()],
            &[lib.root()],
            PathRules::for_current_platform(),
            &|c| std::path::Path::new(c).exists(),
        );
        assert_eq!(cartelle, vec![uno.to_string_lossy().into_owned()]);
        let esito = scansiona_soltanto(&mut lib, &cartelle);
        assert_eq!(esito.removed, 1);
    }

    #[test]
    fn le_cartelle_da_riguardare_sono_le_piu_esterne_che_esistono() {
        let win = PathRules {
            case_insensitive: true,
        };
        let radici = vec![r"C:\Musica".to_owned(), r"D:\Altro".to_owned()];
        let esistono = |c: &str| !c.contains("Sparito");
        let cambiati = [
            // Due file dello stesso album: un album solo.
            r"C:\Musica\Uno\A\01.flac",
            r"C:\Musica\Uno\A\02.flac",
            // Una cartella cancellata con dentro un album: si risale a «Due».
            r"C:\Musica\Due\Sparito\X\01.flac",
            // Fuori da tutte le radici: niente.
            r"E:\Scaricati\x.flac",
            // La radice stessa.
            r"D:\Altro",
            // Dentro una cartella già presa per intero.
            r"D:\Altro\Qualcosa\y.flac",
        ]
        .map(str::to_owned);
        assert_eq!(
            cartelle_da_riguardare(&cambiati, &radici, win, &esistono),
            vec![
                r"C:\Musica\Due".to_owned(),
                r"C:\Musica\Uno\A".to_owned(),
                r"D:\Altro".to_owned(),
            ]
        );
        // Un file subito sotto la radice: la radice.
        assert_eq!(
            cartelle_da_riguardare(&[r"c:\musica\x.flac".to_owned()], &radici, win, &esistono),
            vec![r"C:\Musica".to_owned()]
        );
        // Una radice di unità: il genitore di `X:\a.flac` è `X:\`.
        assert_eq!(cartella_che_contiene(r"X:\a.flac").as_deref(), Some(r"X:\"));
        assert_eq!(cartella_che_contiene("nudo"), None);
    }

    #[test]
    fn i_download_si_scansionano_solo_dentro_le_radici() {
        let win = PathRules {
            case_insensitive: true,
        };
        let radici = vec![r"C:\Musica".to_owned(), r"D:\Scarichi\Buoni".to_owned()];
        assert_eq!(
            dentro_le_radici(r"C:\Musica\Download", &radici, win),
            vec![r"C:\Musica\Download".to_owned()]
        );
        assert_eq!(
            dentro_le_radici(r"D:\Scarichi", &radici, win),
            vec![r"D:\Scarichi\Buoni".to_owned()]
        );
        assert!(dentro_le_radici(r"E:\Altrove", &radici, win).is_empty());
    }
}

/// La ricostruzione per chiavi, la generazione e gli indici della 030, provati
/// su librerie sintetiche scritte direttamente in tabella.
///
/// A parte dalle prove qui sopra perché quelle passano dal disco — file audio
/// veri, tag veri — e queste invece vogliono migliaia di righe in un istante e
/// modifiche a caso: la domanda non è «la scansione legge bene i file», è «le
/// due ricostruzioni danno le stesse tabelle», e per farla a lungo servono
/// righe che costano un `INSERT` e non un WAV.
#[cfg(test)]
mod prove_per_chiavi {
    use super::*;

    /// Un generatore a caso con il seme in mano: una prova che fallisce deve
    /// fallire di nuovo uguale, e il seme sta scritto nel messaggio.
    ///
    /// Il secondo campo decide se gli identificativi esterni si ripetono fra album
    /// diversi: tanto nelle prove di parità, dove le fusioni sono il caso da
    /// mettere alla frusta, e come in una libreria vera nelle misure, dove un
    /// gruppo di pubblicazione è di un disco e delle sue edizioni.
    struct Dado(u64, bool);

    impl Dado {
        fn nuovo(seme: u64) -> Self {
            Self(seme.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1, true)
        }

        /// Gli identificativi come in una libreria taggata con Picard: uno per
        /// disco, condiviso solo dalle sue edizioni.
        fn come_una_libreria_vera(mut self) -> Self {
            self.1 = false;
            self
        }

        /// xorshift64*: abbastanza a caso per scegliere una modifica.
        fn tira(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn fino_a(&mut self, n: usize) -> usize {
            let n = u64::try_from(n.max(1)).expect("un numero piccolo");
            usize::try_from(self.tira() % n).expect("un numero piccolo")
        }

        fn moneta(&mut self, su_cento: u64) -> bool {
            self.tira() % 100 < su_cento
        }
    }

    /// Le copertine che le righe sintetiche possono nominare, con provenienze e
    /// misure diverse: la scelta della copertina d'album le guarda tutte.
    const COPERTINE: &[(&str, &str, i64)] = &[
        ("c0", "tag", 600),
        ("c1", "tag", 300),
        ("c2", "provider", 1200),
        ("c3", "caa", 500),
        ("c4", "spotify", 640),
    ];

    fn database() -> Connection {
        let connection = crate::db::open_in_memory().expect("database").connection;
        for (hash, fonte, lato) in COPERTINE {
            connection
                .execute(
                    "INSERT INTO cover_art (hash, mime_type, width, height, byte_size, source, created_at)
                     VALUES (?1, 'image/jpeg', ?2, ?2, 1000, ?3, 0)",
                    rusqlite::params![hash, lato, fonte],
                )
                .expect("copertina");
        }
        connection
    }

    /// Una riga di brano, con tutto quel che gli aggregati guardano.
    #[derive(Debug, Clone)]
    struct Riga {
        path: String,
        title: String,
        artist: String,
        album: String,
        album_artist: Option<String>,
        year: Option<i64>,
        genre: Option<String>,
        cover: Option<&'static str>,
        mbrg: Option<String>,
        mbr: Option<String>,
        sp: Option<String>,
    }

    impl Riga {
        /// La chiave di base, come la scrive la scansione.
        fn chiave(&self) -> String {
            album_group_key(&self.album, &self.path)
        }
    }

    fn inserisci(connection: &Connection, riga: &Riga) -> i64 {
        connection
            .execute(
                "INSERT INTO tracks (path, track_key, title, artist, album, album_artist,
                                     album_key, year, genre, duration_ms, cover_art_hash,
                                     file_size, date_added, date_modified, content_key,
                                     mb_release_group_id, mb_release_id, spotify_album_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 200000, ?10,
                         1000000, 1, 1, ?2, ?11, ?12, ?13)",
                rusqlite::params![
                    riga.path,
                    format!("{}|{}", riga.artist, riga.title),
                    riga.title,
                    riga.artist,
                    riga.album,
                    riga.album_artist,
                    riga.chiave(),
                    riga.year,
                    riga.genre,
                    riga.cover,
                    riga.mbrg,
                    riga.mbr,
                    riga.sp,
                ],
            )
            .expect("brano sintetico");
        connection.last_insert_rowid()
    }

    /// Riscrive una riga come la riscrive la scansione: i campi dal «file» e la
    /// chiave d'album di base, che la ricostruzione poi rimappa.
    fn riscrivi(connection: &Connection, id: i64, riga: &Riga) {
        connection
            .execute(
                "UPDATE tracks SET path = ?2, title = ?3, artist = ?4, album = ?5,
                        album_artist = ?6, album_key = ?7, year = ?8, genre = ?9,
                        cover_art_hash = ?10, mb_release_group_id = ?11,
                        mb_release_id = ?12, spotify_album_id = ?13
                  WHERE id = ?1",
                rusqlite::params![
                    id,
                    riga.path,
                    riga.title,
                    riga.artist,
                    riga.album,
                    riga.album_artist,
                    riga.chiave(),
                    riga.year,
                    riga.genre,
                    riga.cover,
                    riga.mbrg,
                    riga.mbr,
                    riga.sp,
                ],
            )
            .expect("riscrittura");
    }

    fn leggi(connection: &Connection, id: i64) -> Riga {
        connection
            .query_row(
                "SELECT path, title, artist, album, album_artist, year, genre, cover_art_hash,
                        mb_release_group_id, mb_release_id, spotify_album_id
                   FROM tracks WHERE id = ?1",
                [id],
                |r| {
                    let cover: Option<String> = r.get(7)?;
                    Ok(Riga {
                        path: r.get(0)?,
                        title: r.get(1)?,
                        artist: r.get(2)?,
                        album: r.get(3)?,
                        album_artist: r.get(4)?,
                        year: r.get(5)?,
                        genre: r.get(6)?,
                        cover: cover.and_then(|c| {
                            COPERTINE
                                .iter()
                                .find(|(h, _, _)| *h == c)
                                .map(|(h, _, _)| *h)
                        }),
                        mbrg: r.get(8)?,
                        mbr: r.get(9)?,
                        sp: r.get(10)?,
                    })
                },
            )
            .expect("riga")
    }

    const GENERI: &[&str] = &["Rock", "Jazz", "Alternative Rock", "Elettronica"];

    /// Un brano a caso dentro l'album `album` dell'artista `artista`.
    fn brano_a_caso(dado: &mut Dado, artista: usize, album: usize, traccia: usize) -> Riga {
        let edizione = dado.moneta(15);
        let cartella = if edizione {
            format!(r"C:\Musica\Artista {artista}\Album {album} (Deluxe)")
        } else {
            format!(r"C:\Musica\Artista {artista}\Album {album}")
        };
        // Un disco su cinque è su due dischi, in sottocartelle `CD1`/`CD2`.
        let disco = if album.is_multiple_of(5) {
            format!(r"\CD{}", 1 + dado.fino_a(2))
        } else {
            String::new()
        };
        Riga {
            path: format!(
                r"{cartella}{disco}\{traccia:02} - Brano {}.flac",
                dado.tira()
            ),
            title: format!("Brano {traccia}"),
            artist: if dado.moneta(10) {
                format!("Artista {artista} feat. Ospite {}", dado.fino_a(5))
            } else {
                format!("Artista {artista}")
            },
            album: format!("Album {album}"),
            album_artist: match dado.fino_a(6) {
                0 => Some(format!("Artista {artista}")),
                1 => Some(String::new()),
                2 => Some("  ".to_owned()),
                3 => Some(format!(" Artista {artista} ")),
                _ => None,
            },
            year: dado
                .moneta(80)
                .then(|| 1970 + i64::try_from(album % 50).unwrap_or(0)),
            genre: dado
                .moneta(70)
                .then(|| GENERI[dado.fino_a(GENERI.len())].to_owned()),
            cover: dado
                .moneta(75)
                .then(|| COPERTINE[dado.fino_a(COPERTINE.len())].0),
            // Gli identificativi per album e non per brano, come li scrive
            // Picard: due edizioni nella stessa cartella e due cartelle con lo
            // stesso gruppo di pubblicazione si devono fondere.
            mbrg: dado
                .moneta(40)
                .then(|| format!("rg-{}", if dado.1 { album % 37 } else { album })),
            mbr: dado.moneta(20).then(|| format!("r-{album}")),
            sp: dado
                .moneta(10)
                .then(|| format!("sp-{}", if dado.1 { album % 23 } else { album })),
        }
    }

    /// Le tre tabelle che gli aggregati scrivono, in una forma confrontabile.
    #[derive(Debug, PartialEq, Eq)]
    struct Fotografia {
        albums: Vec<String>,
        artists: Vec<String>,
        chiavi: Vec<(i64, Option<String>)>,
    }

    fn fotografia(connection: &Connection) -> Fotografia {
        let albums = connection
            .prepare(
                "SELECT album_key || '¦' || title || '¦' || artist || '¦' || COALESCE(year, '-')
                        || '¦' || COALESCE(genre, '-') || '¦' || total_tracks || '¦'
                        || COALESCE(cover_art_hash, '-') || '¦' || COALESCE(mb_album_id, '-')
                        || '¦' || COALESCE(spotify_id, '-')
                   FROM albums ORDER BY album_key",
            )
            .expect("album")
            .query_map([], |r| r.get(0))
            .expect("album")
            .collect::<Result<Vec<String>, _>>()
            .expect("album");
        let artists = connection
            .prepare("SELECT name FROM artists ORDER BY name")
            .expect("artisti")
            .query_map([], |r| r.get(0))
            .expect("artisti")
            .collect::<Result<Vec<String>, _>>()
            .expect("artisti");
        let chiavi = connection
            .prepare("SELECT id, album_key FROM tracks ORDER BY id")
            .expect("chiavi")
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("chiavi")
            .collect::<Result<Vec<_>, _>>()
            .expect("chiavi");
        Fotografia {
            albums,
            artists,
            chiavi,
        }
    }

    /// Le righe che stanno in una fotografia e non nell'altra, per un messaggio
    /// che si legga invece di due elenchi da mille righe.
    fn differenze(per_chiavi: &Fotografia, intera: &Fotografia) -> String {
        fn solo<T: PartialEq + std::fmt::Debug>(a: &[T], b: &[T]) -> Vec<String> {
            a.iter()
                .filter(|x| !b.contains(x))
                .map(|x| format!("{x:?}"))
                .collect()
        }
        format!(
            "album solo per chiavi: {:?}\nalbum solo per intero: {:?}\n\
             artisti solo per chiavi: {:?}\nartisti solo per intero: {:?}\n\
             chiavi solo per chiavi: {:?}\nchiavi solo per intero: {:?}",
            solo(&per_chiavi.albums, &intera.albums),
            solo(&intera.albums, &per_chiavi.albums),
            solo(&per_chiavi.artists, &intera.artists),
            solo(&intera.artists, &per_chiavi.artists),
            solo(&per_chiavi.chiavi, &intera.chiavi),
            solo(&intera.chiavi, &per_chiavi.chiavi),
        )
    }

    /// Quel che darebbe la ricostruzione intera **adesso**, senza tenerla: la
    /// transazione cade alla fine e il database resta com'era.
    fn come_la_intera(connection: &mut Connection) -> Fotografia {
        let tx = connection.transaction().expect("transazione");
        rebuild_aggregates(&tx).expect("ricostruzione intera");
        let foto = fotografia(&tx);
        drop(tx);
        foto
    }

    /// Una libreria sintetica di circa `brani` righe, già ricostruita per
    /// intero: è lo stato da cui parte ogni scansione vera.
    fn libreria(connection: &mut Connection, dado: &mut Dado, brani: usize) -> Vec<i64> {
        let tx = connection.transaction().expect("transazione");
        let mut ids = Vec::with_capacity(brani);
        let mut album = 0;
        while ids.len() < brani {
            album += 1;
            let artista = album % 97;
            for traccia in 1..=(6 + dado.fino_a(10)) {
                ids.push(inserisci(&tx, &brano_a_caso(dado, artista, album, traccia)));
            }
        }
        rebuild_aggregates(&tx).expect("ricostruzione");
        tx.commit().expect("commit");
        ids
    }

    /// Una modifica a caso, come la farebbe una scansione o un'importazione,
    /// con le chiavi segnate prima e dopo. Restituisce le chiavi toccate.
    fn modifica_a_caso(
        tx: &Transaction<'_>,
        dado: &mut Dado,
        vivi: &mut Vec<i64>,
        prossimo_album: &mut usize,
    ) -> Toccati {
        let mut toccati = Toccati::nessuno();
        let quale = |dado: &mut Dado, vivi: &[i64]| vivi[dado.fino_a(vivi.len())];
        match dado.fino_a(9) {
            // Un brano nuovo in un album che c'è.
            0 if !vivi.is_empty() => {
                let accanto = leggi(tx, quale(dado, vivi));
                let mut riga = brano_a_caso(dado, 1, 1, 99);
                riga.path = format!(
                    r"{}\99 - Nuovo {}.flac",
                    &accanto.path[..accanto.path.rfind('\\').unwrap_or(0)],
                    dado.tira()
                );
                riga.album = accanto.album.clone();
                riga.artist = accanto.artist.clone();
                riga.mbrg = accanto.mbrg.clone();
                let id = inserisci(tx, &riga);
                toccati.segna_brani(tx, &[id]).expect("dopo");
                vivi.push(id);
            }
            // Un album nuovo che porta l'identificativo di un altro: le due
            // cartelle si devono fondere.
            1 => {
                *prossimo_album += 1;
                let mut gemello = None;
                if !vivi.is_empty() {
                    gemello = leggi(tx, quale(dado, vivi)).mbrg;
                }
                for traccia in 1..=(1 + dado.fino_a(4)) {
                    let mut riga = brano_a_caso(dado, 7, *prossimo_album, traccia);
                    if gemello.is_some() {
                        riga.mbrg.clone_from(&gemello);
                    }
                    let id = inserisci(tx, &riga);
                    toccati.segna_brani(tx, &[id]).expect("dopo");
                    vivi.push(id);
                }
            }
            // Ritaggato: cambia l'album.
            2 if !vivi.is_empty() => {
                let id = quale(dado, vivi);
                toccati.segna_brani(tx, &[id]).expect("prima");
                let mut riga = leggi(tx, id);
                riga.album = format!("Album {}", dado.fino_a(400));
                riga.mbrg = dado.moneta(50).then(|| format!("rg-{}", dado.fino_a(37)));
                riscrivi(tx, id, &riga);
                toccati.segna_brani(tx, &[id]).expect("dopo");
            }
            // Spostato in un'altra cartella.
            3 if !vivi.is_empty() => {
                let id = quale(dado, vivi);
                toccati.segna_brani(tx, &[id]).expect("prima");
                let mut riga = leggi(tx, id);
                riga.path = format!(
                    r"C:\Musica\Spostati {}\{} {}.flac",
                    dado.fino_a(30),
                    riga.title,
                    dado.tira()
                );
                riscrivi(tx, id, &riga);
                toccati.segna_brani(tx, &[id]).expect("dopo");
            }
            // Cambia l'artista, o l'artista dell'album.
            4 if !vivi.is_empty() => {
                let id = quale(dado, vivi);
                toccati.segna_brani(tx, &[id]).expect("prima");
                let mut riga = leggi(tx, id);
                if dado.moneta(50) {
                    riga.artist = format!("Artista {}", dado.fino_a(120));
                } else {
                    riga.album_artist = match dado.fino_a(3) {
                        0 => None,
                        1 => Some(String::new()),
                        _ => Some(format!("Artista {}", dado.fino_a(120))),
                    };
                }
                riscrivi(tx, id, &riga);
                toccati.segna_brani(tx, &[id]).expect("dopo");
            }
            // Cambiano gli identificativi, **senza** rimettere la chiave di
            // base: è quel che lascerebbe un'importazione che scrive solo le
            // colonne esterne. La chiusura deve accorgersene lo stesso.
            5 if !vivi.is_empty() => {
                let id = quale(dado, vivi);
                toccati.segna_brani(tx, &[id]).expect("prima");
                tx.execute(
                    "UPDATE tracks SET mb_release_group_id = ?2, spotify_album_id = ?3
                      WHERE id = ?1",
                    rusqlite::params![
                        id,
                        dado.moneta(70).then(|| format!("rg-{}", dado.fino_a(37))),
                        dado.moneta(20).then(|| format!("sp-{}", dado.fino_a(23))),
                    ],
                )
                .expect("identificativi");
                toccati.segna_brani(tx, &[id]).expect("dopo");
            }
            // Cambia la copertina, o il genere, o l'anno.
            6 if !vivi.is_empty() => {
                let id = quale(dado, vivi);
                toccati.segna_brani(tx, &[id]).expect("prima");
                let mut riga = leggi(tx, id);
                riga.cover = dado
                    .moneta(70)
                    .then(|| COPERTINE[dado.fino_a(COPERTINE.len())].0);
                riga.genre = dado
                    .moneta(70)
                    .then(|| GENERI[dado.fino_a(GENERI.len())].to_owned());
                riga.year = dado
                    .moneta(70)
                    .then(|| 1960 + i64::try_from(dado.fino_a(60)).unwrap_or(0));
                riscrivi(tx, id, &riga);
                toccati.segna_brani(tx, &[id]).expect("dopo");
            }
            // Un album intero se ne va.
            7 if !vivi.is_empty() => {
                let chiave: Option<String> = tx
                    .query_row(
                        "SELECT album_key FROM tracks WHERE id = ?1",
                        [quale(dado, vivi)],
                        |r| r.get(0),
                    )
                    .expect("chiave");
                let ids: Vec<i64> = tx
                    .prepare("SELECT id FROM tracks WHERE album_key IS ?1")
                    .expect("album")
                    .query_map([chiave], |r| r.get(0))
                    .expect("album")
                    .collect::<Result<_, _>>()
                    .expect("album");
                toccati.segna_brani(tx, &ids).expect("prima");
                for id in &ids {
                    tx.execute("DELETE FROM tracks WHERE id = ?1", [id])
                        .expect("cancellazione");
                }
                vivi.retain(|id| !ids.contains(id));
            }
            // Un brano solo se ne va.
            _ if !vivi.is_empty() => {
                let posto = dado.fino_a(vivi.len());
                let id = vivi.swap_remove(posto);
                toccati.segna_brani(tx, &[id]).expect("prima");
                tx.execute("DELETE FROM tracks WHERE id = ?1", [id])
                    .expect("cancellazione");
            }
            _ => {}
        }
        toccati
    }

    #[test]
    fn la_ricostruzione_intera_rifatta_non_cambia_niente() {
        // Il presupposto di tutto il resto: dopo una ricostruzione intera, una
        // seconda dà le stesse tabelle. Se non fosse vero, «per chiavi» non
        // avrebbe un punto fermo con cui confrontarsi.
        let mut connection = database();
        let mut dado = Dado::nuovo(7);
        libreria(&mut connection, &mut dado, 400);
        let prima = fotografia(&connection);
        assert_eq!(come_la_intera(&mut connection), prima);
    }

    #[test]
    fn per_chiavi_e_per_intero_danno_le_stesse_tabelle() {
        // La prova di parità: una libreria sintetica, centinaia di modifiche a
        // caso — brani nuovi, ritaggati, spostati, album fusi per
        // identificativo, album interi che se ne vanno — e dopo **ognuna** la
        // ricostruzione per chiavi deve lasciare esattamente le tabelle che
        // lascerebbe quella intera.
        for seme in [1_u64, 2, 3, 4] {
            let mut connection = database();
            let mut dado = Dado::nuovo(seme);
            let mut vivi = libreria(&mut connection, &mut dado, 300);
            let mut prossimo_album = 10_000;
            let mut per_chiavi_davvero = 0;
            for passo in 0..150 {
                let tx = transazione_di_scrittura(&mut connection).expect("transazione");
                let toccati = modifica_a_caso(&tx, &mut dado, &mut vivi, &mut prossimo_album);
                assert!(!toccati.e_tutto(), "poche modifiche, niente ripiego");
                let intere_prima = INTERE_IN_QUESTO_FILO.with(std::cell::Cell::get);
                ricostruisci_aggregati(&tx, &toccati).expect("per chiavi");
                if INTERE_IN_QUESTO_FILO.with(std::cell::Cell::get) == intere_prima {
                    per_chiavi_davvero += 1;
                }
                tx.commit().expect("commit");
                let per_chiavi = fotografia(&connection);
                let intera = come_la_intera(&mut connection);
                assert!(
                    per_chiavi == intera,
                    "seme {seme}, passo {passo}: le due ricostruzioni divergono\n{}",
                    differenze(&per_chiavi, &intera)
                );
            }
            // E la strada provata dev'essere quella per chiavi: una prova di
            // parità che ripiegasse ogni volta sull'intera confronterebbe
            // l'intera con sé stessa.
            assert!(
                per_chiavi_davvero >= 100,
                "seme {seme}: solo {per_chiavi_davvero} passi su 150 per chiavi"
            );
        }
    }

    #[test]
    fn il_rimappaggio_non_si_incatena() {
        // Il guasto che la prova di parità ha trovato, scritto per esteso. Un
        // disco rimappato la volta prima su `mbrg:A`, i cui brani adesso
        // dicono soprattutto B: diventa `mbrg:B`. Nella stessa passata un
        // brano nuovo, con chiave di base alfabeticamente prima, porta A e
        // diventa `mbrg:A`. Con il rimappaggio coppia per coppia il secondo
        // `UPDATE` ripescava anche lui, e lo portava in `mbrg:B`.
        let mut connection = database();
        let base = |album: &str, path: &str, mbrg: &str| Riga {
            path: path.to_owned(),
            title: "T".to_owned(),
            artist: "X".to_owned(),
            album: album.to_owned(),
            album_artist: None,
            year: None,
            genre: None,
            cover: None,
            mbrg: Some(mbrg.to_owned()),
            mbr: None,
            sp: None,
        };
        let mut disco = Vec::new();
        for (n, mbrg) in ["B", "B", "A"].into_iter().enumerate() {
            let id = inserisci(
                &connection,
                &base("Disco", &format!(r"C:\m\Disco\{n}.flac"), mbrg),
            );
            disco.push(id);
        }
        connection
            .execute("UPDATE tracks SET album_key = 'mbrg:A'", [])
            .expect("rimappati la volta prima");
        let solo = inserisci(&connection, &base("Aaa", r"C:\m\Aaa\1.flac", "A"));
        let tx = transazione_di_scrittura(&mut connection).expect("transazione");
        rebuild_aggregates(&tx).expect("ricostruzione");
        tx.commit().expect("commit");
        let chiave = |id: i64| -> String {
            connection
                .query_row("SELECT album_key FROM tracks WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .expect("chiave")
        };
        assert_eq!(
            chiave(solo),
            "mbrg:A",
            "il brano nuovo è finito nel disco sbagliato"
        );
        for id in disco {
            assert_eq!(chiave(id), "mbrg:B");
        }
    }

    #[test]
    fn senza_la_versione_giusta_si_rifa_tutto() {
        // Una libreria scritta da una regola di prima: un album fantasma che
        // nessun brano nomina. Le chiavi toccate non lo nominano, quindi solo
        // una ricostruzione intera lo può togliere — ed è quella che parte,
        // perché la versione degli aggregati non è quella di adesso.
        let mut connection = database();
        let mut dado = Dado::nuovo(11);
        let ids = libreria(&mut connection, &mut dado, 60);
        connection
            .execute_batch(
                "INSERT INTO albums (album_key, title, artist, total_tracks)
                 VALUES ('fantasma', 'Fantasma', 'Nessuno', 3);
                 UPDATE libreria_stato SET aggregati = 0;",
            )
            .expect("fantasma");
        let tx = transazione_di_scrittura(&mut connection).expect("transazione");
        let mut toccati = Toccati::nessuno();
        toccati.segna_brani(&tx, &ids[..1]).expect("segnati");
        ricostruisci_aggregati(&tx, &toccati).expect("ricostruzione");
        tx.commit().expect("commit");
        let fantasmi: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM albums WHERE album_key = 'fantasma'",
                [],
                |r| r.get(0),
            )
            .expect("conteggio");
        assert_eq!(fantasmi, 0, "la ricostruzione doveva essere intera");
        assert!(aggregati_in_regola(&connection).expect("versione"));
    }

    #[test]
    fn rileggere_tutti_i_tag_chiede_la_ricostruzione_intera() {
        let mut connection = database();
        let mut dado = Dado::nuovo(12);
        libreria(&mut connection, &mut dado, 30);
        assert!(aggregati_in_regola(&connection).expect("versione"));
        rileggi_tutti_i_tag(&connection).expect("rilettura");
        assert!(
            !aggregati_in_regola(&connection).expect("versione"),
            "dopo «Rileggi tutti i tag» la prossima ricostruzione dev'essere intera"
        );
    }

    #[test]
    fn niente_toccato_niente_riscritto() {
        // Una scansione che non ha cambiato niente non scrive niente: né gli
        // album né, quindi, la generazione della libreria.
        let mut connection = database();
        let mut dado = Dado::nuovo(13);
        libreria(&mut connection, &mut dado, 50);
        let prima = generazione(&connection).expect("generazione");
        let foto = fotografia(&connection);
        let tx = transazione_di_scrittura(&mut connection).expect("transazione");
        let aggregati = ricostruisci_aggregati(&tx, &Toccati::nessuno()).expect("niente");
        tx.commit().expect("commit");
        assert_eq!(generazione(&connection).expect("generazione"), prima);
        assert_eq!(fotografia(&connection), foto);
        assert_eq!(aggregati.albums, foto.albums.len());
        assert_eq!(aggregati.artists, foto.artists.len());
    }

    #[test]
    fn troppe_chiavi_e_si_rinuncia_a_contare() {
        let mut connection = database();
        let mut dado = Dado::nuovo(14);
        let ids = libreria(&mut connection, &mut dado, 300);
        // Si abbassa il tetto a mano non si può: è una costante. Si segnano
        // invece due insiemi uniti, e si guarda che l'unione rispetti il tetto.
        let mut tanti = Toccati::nessuno();
        tanti.segna_brani(&connection, &ids).expect("segnati");
        assert!(!tanti.e_tutto());
        assert!(tanti.quanti_album() <= TETTO_PER_CHIAVI);
        let mut tutto = Toccati::tutti();
        tutto.unisci(tanti.clone());
        assert!(tutto.e_tutto());
        let mut ancora = tanti;
        ancora.unisci(Toccati::nessuno());
        assert!(!ancora.e_tutto());
    }

    #[test]
    fn la_generazione_cambia_a_ogni_scrittura_dei_brani() {
        let mut connection = database();
        let mut dado = Dado::nuovo(15);
        let ids = libreria(&mut connection, &mut dado, 10);
        let mut viste = vec![generazione(&connection).expect("generazione")];
        let mut dopo = |connection: &Connection| {
            let adesso = generazione(connection).expect("generazione");
            assert!(
                !viste.contains(&adesso),
                "la generazione è tornata a un valore già visto"
            );
            viste.push(adesso);
        };
        connection
            .execute(
                "UPDATE tracks SET play_count = play_count + 1 WHERE id = ?1",
                [ids[0]],
            )
            .expect("ascolto");
        dopo(&connection);
        let id = inserisci(&connection, &brano_a_caso(&mut dado, 1, 1, 1));
        dopo(&connection);
        connection
            .execute("DELETE FROM tracks WHERE id = ?1", [id])
            .expect("cancellazione");
        dopo(&connection);
        // Una scrittura che non tocca `tracks` non la cambia.
        let ferma = generazione(&connection).expect("generazione");
        connection
            .execute(
                "INSERT INTO settings (key, value) VALUES ('prova', '1')",
                [],
            )
            .expect("impostazione");
        assert_eq!(generazione(&connection).expect("generazione"), ferma);
    }

    /// Il piano di SQLite per una query, una riga per passo.
    fn piano(connection: &Connection, sql: &str) -> String {
        let mut statement = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .expect("piano");
        statement
            .query_map([], |r| r.get::<_, String>(3))
            .expect("righe del piano")
            .collect::<Result<Vec<_>, _>>()
            .expect("piano letto")
            .join(" | ")
    }

    #[test]
    fn i_percorsi_piegati_passano_dall_indice() {
        // La domanda di `ids_by_path` sulle maiuscole: senza la 030 era una
        // lettura intera della tabella per ogni file trascinato.
        let connection = database();
        let letto = piano(
            &connection,
            "SELECT id FROM tracks WHERE path = 'x' COLLATE NOCASE LIMIT 1",
        );
        assert!(
            letto.contains("idx_tracks_percorso_piegato"),
            "il confronto senza maiuscole non usa l'indice: {letto}"
        );
    }

    #[test]
    fn gli_artisti_toccati_passano_dall_indice() {
        // L'indice della 030 è scritto sulla stessa espressione di
        // `ARTISTA_EFFETTIVO`: se un giorno le due divergono, SQLite smette di
        // usarlo in silenzio e ogni ricostruzione torna a leggere tutto.
        let connection = database();
        let letto = piano(
            &connection,
            &format!(
                "SELECT DISTINCT {ARTISTA_EFFETTIVO} FROM tracks
                  WHERE {ARTISTA_EFFETTIVO} IN ('a', 'b')"
            ),
        );
        assert!(
            letto.contains("idx_tracks_artista_effettivo"),
            "la domanda sugli artisti non usa l'indice: {letto}"
        );
    }

    #[test]
    fn i_percorsi_trascinati_si_trovano_anche_con_altre_maiuscole() {
        let mut connection = database();
        let mut dado = Dado::nuovo(16);
        let ids = libreria(&mut connection, &mut dado, 20);
        let vero = leggi(&connection, ids[3]).path;
        let gridato = vero.to_uppercase();
        let trovati = ids_by_path(
            &connection,
            &[vero.clone(), gridato.clone(), r"C:\non\c'è.flac".to_owned()],
            PathRules {
                case_insensitive: true,
            },
        )
        .expect("ricerca");
        assert_eq!(trovati, vec![Some(ids[3]), Some(ids[3]), None]);
        let esatti = ids_by_path(
            &connection,
            &[gridato],
            PathRules {
                case_insensitive: false,
            },
        )
        .expect("ricerca");
        assert_eq!(
            esatti,
            vec![None],
            "dove le maiuscole contano, non si piega"
        );
    }

    /// Quanto ci mette `fare`, in millisecondi, prendendo la migliore di tre:
    /// la prima volta paga le pagine che SQLite non ha ancora in cache.
    fn cronometra<T>(mut fare: impl FnMut() -> T) -> f64 {
        let mut migliore = f64::MAX;
        for _ in 0..3 {
            let inizio = Instant::now();
            std::hint::black_box(fare());
            migliore = migliore.min(inizio.elapsed().as_secs_f64() * 1000.0);
        }
        migliore
    }

    /// Le misure del rapporto di WP21, su cinquantamila brani sintetici.
    ///
    /// `#[ignore]` perché è un cronometro e non una prova: costruire la
    /// libreria vale qualche secondo, e i numeri dicono qualcosa solo letti da
    /// qualcuno. Si lancia con
    /// `cargo test -p aether-app misure_su_cinquantamila_brani -- --ignored --nocapture`.
    #[test]
    #[ignore = "misure per il rapporto, non una prova"]
    fn misure_su_cinquantamila_brani() {
        let mut connection = database();
        let mut dado = Dado::nuovo(2026).come_una_libreria_vera();
        let inizio = Instant::now();
        let ids = libreria(&mut connection, &mut dado, 50_000);
        println!(
            "libreria sintetica: {} brani in {:.0} ms",
            ids.len(),
            inizio.elapsed().as_secs_f64() * 1000.0
        );
        let numeri = counts(&connection).expect("numeri");
        println!(
            "album {} · artisti {} · brani {}",
            numeri.albums, numeri.artists, numeri.tracks
        );

        // ── gli aggregati: prima (sempre intera) e dopo (per chiavi) ──
        let intera = cronometra(|| {
            let tx = connection.transaction().expect("transazione");
            rebuild_aggregates(&tx).expect("intera");
            drop(tx);
        });
        println!("ricostruzione intera (com'era dopo ogni scansione): {intera:.1} ms");
        for quanti in [1_usize, 12, 500] {
            let scelti: Vec<i64> = ids.iter().copied().step_by(97).take(quanti).collect();
            let per_chiavi = cronometra(|| {
                let tx = transazione_di_scrittura(&mut connection).expect("transazione");
                let mut toccati = Toccati::nessuno();
                toccati.segna_brani(&tx, &scelti).expect("prima");
                tx.execute(
                    &format!(
                        "UPDATE tracks SET genre = 'Misura' WHERE id IN ({})",
                        segnaposti(scelti.len())
                    ),
                    rusqlite::params_from_iter(&scelti),
                )
                .expect("modifica");
                toccati.segna_brani(&tx, &scelti).expect("dopo");
                ricostruisci_aggregati(&tx, &toccati).expect("per chiavi");
                drop(tx);
            });
            println!("per chiavi, {quanti} brani toccati (modifica compresa): {per_chiavi:.1} ms");
        }
        let ferma = cronometra(|| {
            let tx = transazione_di_scrittura(&mut connection).expect("transazione");
            ricostruisci_aggregati(&tx, &Toccati::nessuno()).expect("niente");
            drop(tx);
        });
        println!("scansione che non ha cambiato niente: {ferma:.2} ms");

        // ── i file trascinati con altre maiuscole ──
        let gridati: Vec<String> = ids
            .iter()
            .step_by(500)
            .take(100)
            .map(|id| leggi(&connection, *id).path.to_uppercase())
            .collect();
        let regole = PathRules {
            case_insensitive: true,
        };
        let con_indice =
            cronometra(|| ids_by_path(&connection, &gridati, regole).expect("ricerca"));
        connection
            .execute_batch("DROP INDEX idx_tracks_percorso_piegato")
            .expect("via l'indice");
        let senza_indice =
            cronometra(|| ids_by_path(&connection, &gridati, regole).expect("ricerca"));
        connection
            .execute_batch(
                "CREATE INDEX idx_tracks_percorso_piegato ON tracks(path COLLATE NOCASE)",
            )
            .expect("indice rimesso");
        println!(
            "100 file trascinati con altre maiuscole: {senza_indice:.1} ms senza indice, {con_indice:.2} ms con"
        );

        // ── gli artisti toccati: con e senza l'indice sull'espressione ──
        let nomi: std::collections::BTreeSet<String> =
            (0..50).map(|n| format!("Artista {n}")).collect();
        let artisti_con = cronometra(|| {
            let tx = transazione_di_scrittura(&mut connection).expect("transazione");
            ricostruisci_gli_artisti(&tx, &nomi).expect("artisti");
            drop(tx);
        });
        connection
            .execute_batch("DROP INDEX idx_tracks_artista_effettivo")
            .expect("via l'indice");
        let artisti_senza = cronometra(|| {
            let tx = transazione_di_scrittura(&mut connection).expect("transazione");
            ricostruisci_gli_artisti(&tx, &nomi).expect("artisti");
            drop(tx);
        });
        let elenco_senza = cronometra(|| list_artists(&connection).expect("artisti"));
        connection
            .execute_batch(&format!(
                "CREATE INDEX idx_tracks_artista_effettivo ON tracks({ARTISTA_EFFETTIVO})"
            ))
            .expect("indice rimesso");
        let elenco_con = cronometra(|| list_artists(&connection).expect("artisti"));
        println!("50 artisti toccati: {artisti_senza:.1} ms senza indice, {artisti_con:.2} ms con");

        // ── le letture che la finestra chiede: chi supera i 50 ms ──
        println!("elenco degli artisti: {elenco_senza:.1} ms senza indice, {elenco_con:.1} ms con");
        let letture: Vec<(&str, f64)> = vec![
            (
                "numeri (avvio)",
                cronometra(|| counts(&connection).expect("numeri")),
            ),
            (
                "brani, prima pagina per scaffale",
                cronometra(|| list_tracks(&connection, TrackOrder::Shelf, 0, 200).expect("brani")),
            ),
            (
                "brani, pagina a metà per scaffale",
                cronometra(|| {
                    list_tracks(&connection, TrackOrder::Shelf, 25_000, 200).expect("brani")
                }),
            ),
            (
                "brani, pagina in fondo per titolo",
                cronometra(|| {
                    list_tracks(&connection, TrackOrder::Title, 49_800, 200).expect("brani")
                }),
            ),
            (
                "album, pagina a metà",
                cronometra(|| list_albums(&connection, 2_000, 200).expect("album")),
            ),
            (
                "dischi aggiunti di recente",
                cronometra(|| recently_added_albums(&connection, 12).expect("dischi")),
            ),
            (
                "ricerca «brano» (pagina)",
                cronometra(|| search(&connection, "brano", 0, 200).expect("ricerca")),
            ),
            (
                "ricerca «brano» (conteggio)",
                cronometra(|| search_count(&connection, "brano").expect("conteggio")),
            ),
            (
                "ricerca «art» (pagina)",
                cronometra(|| search(&connection, "art", 0, 200).expect("ricerca")),
            ),
            (
                "preferiti",
                cronometra(|| list_liked(&connection, 0, 200).expect("preferiti")),
            ),
        ];
        for (nome, ms) in letture {
            println!("{nome}: {ms:.1} ms");
        }

        // ── le playlist: l'elenco con le intelligenti, e un brano aggiunto ──
        use aether_domain::regole::{
            Campo, Combinazione, Insieme, Operatore, Ordinamento, Regola, Valore,
        };
        let regola = |campo, operatore, valore| Insieme {
            combinazione: Combinazione::Tutte,
            regole: vec![Regola {
                campo,
                operatore,
                valore,
            }],
            limite: None,
            ordinamento: Ordinamento::Scaffale,
        };
        let intelligenti = [
            regola(
                Campo::Artista,
                Operatore::Contiene,
                Valore::Testo("artista 1".into()),
            ),
            regola(
                Campo::Titolo,
                Operatore::Inizia,
                Valore::Testo("brano 1".into()),
            ),
            regola(
                Campo::Genere,
                Operatore::Uguale,
                Valore::Testo("jazz".into()),
            ),
            regola(
                Campo::Album,
                Operatore::NonContiene,
                Valore::Testo("9".into()),
            ),
            regola(Campo::Anno, Operatore::Maggiore, Valore::Numero(1990)),
            regola(Campo::Valutazione, Operatore::Maggiore, Valore::Numero(3)),
        ];
        for (n, insieme) in intelligenti.iter().enumerate() {
            crate::playlists::create_smart(&connection, &format!("Intelligente {n}"), insieme)
                .expect("intelligente");
        }
        let mut a_mano = 0;
        for n in 0..20 {
            a_mano = crate::playlists::create(&connection, &format!("A mano {n}"))
                .expect("playlist")
                .id;
            let brani: Vec<i64> = ids.iter().copied().skip(n * 50).take(50).collect();
            crate::playlists::add_tracks(&mut connection, a_mano, &brani).expect("brani");
        }
        let elenco_com_era = cronometra(|| {
            // Com'era: l'elenco e un conteggio vero per ogni intelligente.
            for insieme in &intelligenti {
                crate::smart::conteggi(&connection, insieme, 0).expect("conteggio");
            }
        });
        let _ = crate::playlists::list(&connection).expect("prima volta");
        let elenco_ricordato = cronometra(|| crate::playlists::list(&connection).expect("elenco"));
        println!(
            "elenco playlist con {} intelligenti: {elenco_com_era:.1} ms di soli conteggi com'era, {elenco_ricordato:.1} ms con i conteggi ricordati",
            intelligenti.len()
        );
        // Un brano aggiunto a una playlist a mano scrive `playlist_tracks`, non
        // `tracks`: la generazione non cambia, e il riepilogo è di lei sola.
        let aggiunta = cronometra(|| {
            crate::playlists::add_tracks(&mut connection, a_mano, &ids[..1]).expect("aggiunta")
        });
        println!(
            "un brano aggiunto a una playlist (riepilogo compreso): {aggiunta:.1} ms, contro {elenco_com_era:.1} ms del solo ricalcolo delle intelligenti com'era"
        );
        let aperta =
            cronometra(|| crate::smart::brani(&connection, &intelligenti[0], 0).expect("brani"));
        let anteprima =
            cronometra(|| crate::smart::quanti(&connection, &intelligenti[0], 0).expect("quanti"));
        println!(
            "playlist intelligente aperta: {aperta:.1} ms · anteprima delle regole: {anteprima:.1} ms"
        );
    }
}
