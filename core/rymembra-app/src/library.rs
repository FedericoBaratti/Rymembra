//! La scansione per intero: dal disco a una libreria che si può interrogare.
//!
//! Il piano lo decide [`rymembra_domain::scan_plan`], che è puro. Qui si legge
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
//!
//! # Le immagini CD
//!
//! Un `.cue` accanto a un'immagine fa diventare l'immagine i suoi brani: voci
//! virtuali `….cue|NN` decise dal dominio ([`rymembra_domain::cue`]), una riga
//! per brano con `immagine`, `inizio_cd` e `fine_cd` (migrazione 034). I fogli
//! letti si ricordano in `fogli_cue`, e i brani della stessa immagine si
//! leggono aprendola una volta sola: vedi [`Scan::run_con`].
//!
//! # Un no si ricorda
//!
//! Un file che il lettore rifiuta per com'è fatto non si riapre alla passata
//! dopo, finché non cambia lui o il lettore: vedi la migrazione 032 e
//! [`ScanReport::illeggibili_noti`].
//!
//! # Le radici del cloud: righe prima, tag dopo
//!
//! Come si legge una radice lo dice la sua sorgente ([`MusicFiles::limiti`]):
//! le costanti di questo modulo sono quelle del disco ([`Limiti::DISCO`]), e
//! una cartella del disco si legge esattamente come prima. Una del cloud no:
//! ogni file è una richiesta HTTPS, un `429` chiede di aspettare, e ventimila
//! file letti prima di mostrarne uno sarebbero mezz'ora di libreria vuota.
//! Quindi, per le radici che lo chiedono:
//!
//! - i file nuovi entrano **subito**, senza lettura, come righe provvisorie
//!   ([`riga_provvisoria`]) — tranne dove il piano ha delle sparizioni, che
//!   potrebbero essere spostamenti da riconoscere leggendo;
//! - la lettura vera arriva dopo e le completa sul posto, al più
//!   [`Limiti::letture_per_giro`] per passata: il resto si conta in
//!   [`ScanReport::rimasti`] e si ritrova la volta dopo;
//! - un servizio che chiede di rallentare non è un guasto: le letture si
//!   rimandano, e la radice non si abbandona.

use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusqlite::{Connection, Row, Transaction, TransactionBehavior};
use rymembra_domain::album::{AlbumMember, AlbumRow, album_group_key, build_album_groups};
use rymembra_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use rymembra_domain::keys::{ContentKey, ContentKeyInput, TrackKey, TrackKeyInput};
use rymembra_domain::paths::{
    PathRules, extension_of, file_stem, is_supported_audio_path, is_under, path_key,
};
use rymembra_domain::scan_plan::{
    DiscoveredFile, KnownTrack, PendingUpdate, RemoveReason, RemovedIdentity, ScanInput, ScanPlan,
    SkipReason, match_moved_tracks, plan_scan, stessa_grafia,
};

use rymembra_domain::cue::{Abbinamento, Foglio, brano_d_immagine};
use rymembra_domain::nuvola::TagNoti;

use crate::covers::{CoverSource, CoverStore, StoredCover};
use crate::files::{Limiti, MusicFiles, Voce};
use crate::metadata::read_tags;
use crate::scadenza::Mancato;

mod aggregati;
mod camminata;
mod immagini;
mod interrogazioni;
mod righe;
mod scansione;

pub use aggregati::{
    Aggregates, Toccati, VERSIONE_AGGREGATI, chiedi_ricostruzione_intera, generazione,
    rebuild_aggregates, ricostruisci_aggregati,
};
pub use camminata::{
    Cambiamento, Natura, Ricognizione, cammina, cartelle_da_riguardare, dentro_le_radici,
    piu_esterne,
};
pub use interrogazioni::{
    AlbumSummary, ArtistSummary, Counts, FormatoFile, TrackOrder, TrackSummary, VoceCronologia,
    album_tracks, albums_by_artist, ascoltati_di_recente, count_history, counts, etichetta_codec,
    fts_query, ids_by_path, list_albums, list_artists, list_history, list_liked, list_tracks,
    read_formato, read_summary, recently_added_albums, search, search_count, summaries_by_id,
};
pub use righe::{
    Cancellazione, TrackRow, applica_rimozioni, cancella_brani, known_tracks, read_track,
    riassumi_rimozioni, rileggi_tutti_i_tag,
};
pub use scansione::{Deposito, plan};

pub(crate) use interrogazioni::{COLONNE_BRANO, track_from_row};

use aggregati::{ARTISTA_EFFETTIVO, PEZZO, aggregati_in_regola, conta_aggregati, segnaposti};
use camminata::{cammina_con, cartella_che_contiene};
use immagini::{
    FoglioLetto, FoglioRicordato, aggiorna_le_memorie, chiavi_dei_fogli, fogli_con_brani,
    fogli_ricordati, immagini_dei_brani, immagini_rinominate, read_image_tracks,
};
use righe::{
    content_keys_of, insert_track, remove_tracks, riga_provvisoria, righe_provvisorie,
    togli_provvisorie, update_track,
};

/// Quanti file si leggono prima di scriverli.
///
/// Cinquecento è dove il costo di aprire una transazione sparisce nel rumore
/// (una transazione ogni pochi secondi di lettura) senza che un'interruzione
/// costi più di qualche secondo di lavoro da rifare.
///
/// È il numero del disco ([`Limiti::DISCO`]): una radice del cloud dice il suo
/// con [`MusicFiles::limiti`], più piccolo, perché lì ogni file costa una
/// richiesta e la finestra deve vedere arrivare i brani man mano.
pub const LOTTO: usize = 500;

/// [`Limiti::DISCO`] sono queste costanti, e nient'altro.
///
/// Le costanti restano qui, con le ragioni dei loro numeri, perché sono la
/// misura del disco e della share di casa; [`Limiti::DISCO`] le porta dove la
/// scansione le chiede radice per radice. Se una delle due cambiasse senza
/// l'altra, la scansione di una cartella del disco non sarebbe più quella di
/// sempre — e lo si saprebbe qui, alla compilazione, invece che da un NAS.
const _: () = {
    assert!(Limiti::DISCO.in_volo == LETTURE_IN_VOLO);
    assert!(Limiti::DISCO.per_file.as_millis() == LETTURA_BRANO.as_millis());
    assert!(Limiti::DISCO.scadenze_di_fila == SCADENZE_DI_FILA);
    assert!(Limiti::DISCO.lotto == LOTTO);
    assert!(Limiti::DISCO.prima_uno_alla_volta);
    assert!(!Limiti::DISCO.prima_le_righe);
    assert!(Limiti::DISCO.letture_per_giro.is_none());
};

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

/// Quante letture si tengono in volo insieme.
///
/// Quattro, come i fili della camminata: su una condivisione SMB l'attesa di
/// un file è quasi tutta andata e ritorno di rete, e quattro richieste in volo
/// la nascondono quasi tutta senza far sembrare un attacco quel che chiede un
/// lettore musicale. Di più non serve a un NAS di casa, e su un disco locale
/// il collo di bottiglia è comunque il lettore dei tag.
const LETTURE_IN_VOLO: usize = 4;

/// Oltre questa dimensione un `.cue` non è un foglio.
///
/// Un megabyte: il foglio più lungo che si sia visto, un doppio dal vivo con
/// titoli lunghi, non arriva a dieci kilobyte. Un file più grosso con quel nome
/// è qualcos'altro, e leggerlo tutto per scoprirlo costerebbe un'attesa di rete
/// per niente.
const FOGLIO_MASSIMO: u64 = 1024 * 1024;

/// Quante righe note fanno sospetta una radice vista senza musica.
///
/// Cinquanta, come [`SOGLIA_RIMOZIONI`] e per la stessa ragione: sotto, una
/// cartella vuota è plausibile — una chiavetta svuotata di proposito — e la
/// guardia non deve scattare su una libreria piccolissima. Sopra, zero brani
/// trovati dove ce n'erano cinquanta è molto più spesso un punto di mount
/// caduto che un utente che ha cancellato tutto.
const RADICE_SOSPETTA: usize = 50;

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
    /// Attesa massima per la lettura di un singolo brano **del disco**.
    ///
    /// Vale per le radici che leggono coi limiti del disco
    /// ([`Limiti::DISCO`]), cioè quelle di sempre. Una sorgente che dichiara
    /// un'attesa sua — il cloud, sessanta secondi — tiene la sua: vedi
    /// [`Scan::scadenza_per`].
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
/// 292 milioni, mentre l'orologio di sistema più capiente su cui Rymembra gira —
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
    ///
    /// Comprende anche le [`ScanReport::radici_vuote`]: per chi legge questo
    /// elenco come «non guardate» sono esattamente quello.
    pub radici_saltate: Vec<String>,
    /// Le radici che hanno risposto **vuote**, e che la guardia per radice ha
    /// saltato: cartelle sorvegliate con almeno cinquanta brani in libreria e
    /// nessun file di musica sul disco. Vedi [`Scan::radici_della_libreria`].
    ///
    /// Stanno **anche** in [`ScanReport::radici_saltate`], perché non sono
    /// state giudicate. Qui a parte perché la domanda di chi riprova è un'altra:
    /// una radice che non risponde si riprova finché risponde, una che risponde
    /// vuota risponderà vuota anche fra cinque minuti — la si riprova quando
    /// cambia qualcosa, o la si giudica con una scansione a mano, dove c'è chi
    /// può confermare.
    pub radici_vuote: Vec<String>,
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
    /// Quanto è durata ogni fase: vedi [`TempiScansione`].
    pub tempi: TempiScansione,
    /// Quante cartelle la camminata ha elencato.
    ///
    /// Le conta chi sa contarle — la camminata vera manda un battito per
    /// cartella (`Voce::Cartella`) — e chi non sa resta a zero.
    pub cartelle_viste: usize,
    /// Quanti file la camminata ha trovato, musica o no, **prima** che le
    /// immagini CD diventino i loro brani: è il numero del disco, non della
    /// libreria.
    pub file_visti: usize,
    /// Quante voci si sono date da leggere al lettore dei tag.
    ///
    /// I brani di un'immagine contano uno per uno anche se l'immagine si apre
    /// una volta sola: è lo stesso conto dell'avanzamento. Su un disco
    /// invariato è zero, ed è il numero da guardare per sapere se una
    /// riscansione è stata gratis.
    pub letti: usize,
    /// Le voci che il piano dava da leggere e che non si sono lette, perché il
    /// lettore le aveva già rifiutate così com'erano: stessa dimensione, stessa
    /// data, stessa [`crate::metadata::VERSIONE_LETTORE`]. Vedi la migrazione
    /// 032.
    pub illeggibili_noti: usize,
    /// Quanti brani d'immagine CD la camminata ha visto, cioè quante voci
    /// virtuali `….cue|NN` sono entrate nel piano al posto delle loro immagini.
    pub brani_d_immagine: usize,
    /// Le righe entrate **senza lettura**, dai soli dati dell'elenco: la prima
    /// passata di una radice del cloud (vedi [`Limiti::prima_le_righe`]).
    /// Sono contate anche fra le [`ScanReport::inserted`]; quando la lettura
    /// le completa, nella stessa passata o in una dopo, si contano fra le
    /// [`ScanReport::updated`].
    pub provvisorie: usize,
    /// Le letture rimandate: voci che il piano dava da leggere e che questa
    /// passata non ha letto, perché la radice ha un tetto di letture per giro
    /// ([`Limiti::letture_per_giro`]) o perché il servizio ha chiesto di
    /// rallentare (`net.rateLimited`, `cloud.quotaExceeded`).
    ///
    /// Non sono illeggibili e non sono perse: le righe restano come sono — le
    /// provvisorie provvisorie —, e la passata dopo le ritrova da leggere da
    /// sé. Diverso da zero vuol dire «rimettimi in fila»: chi ha chiesto la
    /// scansione la rimette dietro alle altre, così una radice del cloud da
    /// ventimila file non tiene il turno per mezz'ora.
    pub rimasti: usize,
}

/// Quanto è durata ogni fase di una scansione, in millisecondi.
///
/// Per chi vuole sapere **dove** va il tempo: la riga del diario sul desktop,
/// l'esempio da riga di comando. Le fasi sono quelle di [`Scan::run_su`]; quel
/// che manca alla somma per fare `elapsed_ms` sono le prese brevi del deposito
/// fra l'una e l'altra.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TempiScansione {
    /// La camminata sul disco.
    pub camminata_ms: u64,
    /// I fogli `.cue`: la cache, le letture di quelli cambiati, l'espansione
    /// delle immagini nei loro brani.
    pub fogli_ms: u64,
    /// Le righe note, la memoria dei rifiuti e il piano.
    pub piano_ms: u64,
    /// La lettura dei file, senza le scritture dei lotti.
    pub lettura_ms: u64,
    /// Le transazioni di scrittura: le doppie, i lotti, le rimozioni.
    pub scrittura_ms: u64,
    /// La ricostruzione di album e artisti.
    pub aggregati_ms: u64,
}

/// I millisecondi passati da `inizio`, in un `u64`.
fn millisecondi(inizio: Instant) -> u64 {
    u64::try_from(inizio.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Quel che un'osservazione della scansione racconta. Passato a
/// [`Scan::run_con`].
///
/// Due fasi e non una, perché sono due attese diverse per chi guarda: durante
/// la camminata non si sa ancora quanti file ci siano da leggere — si sa solo
/// quante cartelle si sono guardate — e senza un numero la finestra mostrerebbe
/// una barra ferma per tutti i secondi in cui un NAS si fa elencare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Avanzamento {
    /// Durante la camminata: cartelle elencate e file trovati finora, su
    /// tutte le radici. Non più di quattro volte al secondo.
    Camminata {
        /// Le cartelle elencate.
        cartelle: usize,
        /// I file trovati, musica o no.
        file: usize,
    },
    /// Durante la lettura: come l'`on_progress` di [`Scan::run_su`].
    Lettura {
        /// Le voci lette, saltate o rifiutate finora.
        fatti: usize,
        /// Le voci da leggere in tutto.
        totale: usize,
    },
}

/// Ogni quanto, al più, si racconta la camminata.
///
/// Quattro volte al secondo: abbastanza perché il numero delle cartelle
/// scorra, e abbastanza poche perché raccontarlo non costi niente a chi lo
/// riceve — sul desktop ogni racconto è un evento verso la finestra.
const RACCONTO_CAMMINATA: Duration = Duration::from_millis(250);

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
    /// Le cartelle sorvegliate della libreria, come sono nelle impostazioni.
    ///
    /// Non sono sempre le `roots`: la sorveglianza e i download guardano
    /// sottocartelle. Servono alla guardia per radice delle scansioni
    /// prudenti, che vale **solo** per le radici della passata che coincidono
    /// con una di queste: vedi `Scan::guardia_per_radice`. Vuoto vuol dire
    /// «nessuna guardia per radice».
    pub radici_della_libreria: &'a [String],
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
            .field("radici_della_libreria", &self.radici_della_libreria)
            .field("rules", &self.rules)
            .field("prudente", &self.prudente)
            .field("scadenze", &self.scadenze)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;

/// La ricostruzione per chiavi, la generazione e gli indici della 030, provati
/// su librerie sintetiche scritte direttamente in tabella.
///
/// A parte dalle prove qui sopra perché quelle passano dal disco — file audio
/// veri, tag veri — e queste invece vogliono migliaia di righe in un istante e
/// modifiche a caso: la domanda non è «la scansione legge bene i file», è «le
/// due ricostruzioni danno le stesse tabelle», e per farla a lungo servono
/// righe che costano un `INSERT` e non un WAV.
#[cfg(test)]
mod prove_per_chiavi;
