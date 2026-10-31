//! Da un file a campioni pronti per la scheda audio.
//!
//! Tre lavori in uno, e gli ultimi due sono quelli che non si vedono:
//! decodificare, **ricampionare**, e portare i canali del file su quelli del
//! dispositivo. Il dispositivo si apre a una frequenza sola — su Windows in
//! modalità condivisa la decide il sistema, ed è quasi sempre 48000 — mentre i
//! file hanno la loro. La libreria vera di questo progetto è 1421 file a 44100:
//! senza ricampionamento non uscirebbe un suono giusto da nessuno di loro.
//!
//! E soprattutto: due brani a frequenze diverse non potrebbero attaccarsi senza
//! buco, perché il flusso che arriva alla scheda deve avere una frequenza sola
//! dall'inizio alla fine della sessione, non una per brano.
//!
//! # Le tre promesse di questa catena
//!
//! - **Né un fotogramma in più né uno in meno.** Un brano di `n` fotogrammi a
//!   44 100 esce come `ceil(n × 48000 / 44100)` fotogrammi a 48 000, senza il
//!   ritardo del filtro in testa e senza perderne la coda: è quel che fa di un
//!   disco dal vivo un disco senza schiocchi fra una traccia e l'altra. Vedi
//!   [`ricampionatore`].
//! - **Ogni canale al suo posto.** Uno stereo su un impianto 5.1 o su una cuffia
//!   col 7.1 virtuale esce dai due frontali e basta; un 5.1 su due casse segue
//!   ITU-R BS.775. Vedi [`canali`].
//! - **Un salto atterra dove è stato chiesto**, al fotogramma, e non all'inizio
//!   del pacchetto che lo contiene: così audio, cursore e testo sincronizzato
//!   dicono la stessa cosa. Vedi [`Decodificatore::cerca`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};

use rymembra_domain::errors::{AppError, ErrorCode};
use symphonia::core::audio::{AudioSpec, Channels};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_AAC, CODEC_ID_ADPCM_G722, CODEC_ID_ADPCM_IMA_QT, CODEC_ID_ALAC, CODEC_ID_FLAC,
    CODEC_ID_MONKEYS_AUDIO, CODEC_ID_MP1, CODEC_ID_MP2, CODEC_ID_MP3, CODEC_ID_OPUS,
    CODEC_ID_PCM_MULAW, CODEC_ID_PCM_S32LE, CODEC_ID_TTA, CODEC_ID_VORBIS, CODEC_ID_WAVPACK,
};
use symphonia::core::codecs::audio::{
    AudioCodecId, AudioCodecParameters, AudioDecoder, AudioDecoderOptions,
};
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::errors::SeekErrorKind;
use symphonia::core::formats::probe::{Hint, Probe};
use symphonia::core::formats::well_known::{
    FORMAT_ID_AIFF, FORMAT_ID_CAF, FORMAT_ID_OGG, FORMAT_ID_WAVE,
};
use symphonia::core::formats::{
    FormatId, FormatOptions, FormatReader, SeekMode, SeekTo, Track, TrackType,
};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Timestamp;

use crate::Codec;
use crate::ape::FORMAT_ID_APE;

mod canali;
mod opus;
mod ricampionatore;

use canali::Matrice;
use ricampionatore::Ricampionatore;

/// I codec che sappiamo costruire.
///
/// # Perché un registro nostro invece di `symphonia::default::get_codecs()`
///
/// Perché uno dei codec non è di symphonia. Opus lo decodifica
/// [`opus::DecodificatoreOpus`], e symphonia lo scoprirebbe solo se glielo si
/// dice: il registro predefinito è una costante della libreria, e
/// `make_audio_decoder` su un `CODEC_ID_OPUS` risponderebbe «non supportato»
/// dopo che il demultiplatore ha riconosciuto il flusso perfettamente.
///
/// Si costruisce una volta sola e per sempre: dentro ci sono tabelle di
/// descrittori, non stato, e ogni [`Decodificatore::apri`] che se lo rifacesse
/// pagherebbe la costruzione di tutta la catena di symphonia per ottenere la
/// stessa cosa.
static REGISTRO: LazyLock<CodecRegistry> = LazyLock::new(|| {
    let mut registro = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut registro);
    registro.register_audio_decoder::<opus::DecodificatoreOpus>();
    registro
});

/// I contenitori che sappiamo riconoscere.
///
/// # Perché un riconoscitore nostro invece di `symphonia::default::get_probe()`
///
/// Per la stessa ragione di [`REGISTRO`], un contenitore più in là: Monkey's
/// Audio lo legge [`crate::ape::LettoreApe`], che symphonia non porta, e il
/// riconoscitore predefinito è una costante della libreria. Dentro ci sono gli
/// stessi contenitori e gli stessi lettori di tag di quello, nello stesso
/// ordine, più il nostro.
///
/// Si costruisce una volta sola, come il registro: è un elenco di descrittori
/// e un filtro di Bloom, non stato.
static SONDA: LazyLock<Probe> = LazyLock::new(|| {
    let mut sonda = Probe::default();
    symphonia::default::register_enabled_formats(&mut sonda);
    sonda.register_format::<crate::ape::LettoreApe<'_>>();
    sonda
});

/// Le opzioni con cui si costruisce ogni codec.
///
/// # Il gapless, che dalla 0.6 è una faccenda del codec
///
/// Gli encoder mettono del silenzio in testa (il ritardo del codificatore) e in
/// coda (l'imbottitura dell'ultimo pacchetto): senza toglierlo, fra due brani
/// di uno stesso album resta un buco che nel file non c'è — e su un disco che è
/// pensato senza stacchi si sente. In 0.5 lo toglieva il contenitore, con
/// `FormatOptions::enable_gapless`; in 0.6 il contenitore dice soltanto quanto
/// togliere, in `trim_start` e `trim_end` di ogni pacchetto, e lo toglie il
/// codec se questa opzione è accesa. È accesa anche per scelta predefinita di
/// symphonia; qui la si accende per iscritto, perché è una promessa di questo
/// crate e non un'impostazione di una libreria che può cambiare.
fn opzioni_codec() -> AudioDecoderOptions {
    AudioDecoderOptions::default().gapless(true)
}

/// Da dove arrivano i byte di un brano.
///
/// Un tratto nostro e non quello di symphonia, benché symphonia ne abbia uno
/// identico. La ragione è il confine: se `Sorgente` esponesse il
/// `MediaSource` di symphonia, ogni crate che vuole far suonare qualcosa
/// dovrebbe dipendere da symphonia e concordare sulla versione — e la scelta
/// del decodificatore, che deve poter cambiare, smetterebbe di essere un affare
/// interno a questo crate.
///
/// La lunghezza serve al decodificatore per sapere se può saltare. Chi non la
/// conosce dice `None`: il brano si sentirà lo stesso, ma senza cursore.
///
/// I due metodi che seguono hanno un valore predefinito che è quello giusto per
/// un file: un file non è un flusso di rete, e non ha niente da annullare.
pub trait Flusso: std::io::Read + std::io::Seek + Send + Sync {
    /// Quanti byte, se si sanno.
    fn lunghezza(&self) -> Option<u64> {
        None
    }

    /// I byte arrivano dalla rete, e non da un disco?
    ///
    /// # Perché il decodificatore lo vuole sapere
    ///
    /// Per come salta. Su un file il salto «accurato» di symphonia è la scelta
    /// giusta; su un MP3, però, accurato vuol dire contare i fotogrammi dal
    /// punto in cui si è — e per un salto all'indietro dall'inizio del file —
    /// fino al bersaglio. Da un disco sono millisecondi; da un flusso HTTP sono
    /// megabyte chiesti a intervalli, cioè secondi di cursore fermo. Per un
    /// flusso si salta quindi «a grandi linee» — symphonia stima il byte dalla
    /// lunghezza e dal bitrate — e lo scarto fino al bersaglio che fa
    /// [`Decodificatore::prossimo`] rimette la precisione sui timestamp che
    /// symphonia assegna da lì.
    ///
    /// # Dove la precisione non torna: gli MPEG
    ///
    /// Lo scarto rimette la precisione solo se i timestamp sono veri, e lo sono
    /// in FLAC, Ogg, WAV, MP4 e nel lettore APE. In un MP3 (e negli altri MPEG)
    /// il salto a grandi linee di symphonia non solo stima il byte: **assegna**
    /// al pacchetto un timestamp stimato dalla stessa proporzione, senza
    /// passare dalla tabella Xing — e lo scarto lavora su quello. Su un
    /// bitrate costante lo sbaglio è di un fotogramma o due; su uno variabile di
    /// secondi.
    ///
    /// Per un brano intero non si sente: il cursore atterra un po' più in là o
    /// più in qua. Si sente in un **brano d'immagine MP3 con il suo `.cue` in
    /// una cartella cloud**: l'apertura salta all'inizio della traccia con
    /// questo salto, quindi su un bitrate variabile la traccia comincia e finisce
    /// spostata dello sbaglio, e fra due tracce contigue c'è un buco o un pezzo
    /// ripetuto. È un limite dichiarato, e scelto: il salto accurato
    /// rileggerebbe il file dall'inizio, attraverso la rete, dentro la scadenza
    /// dell'apertura del brano — e una traccia in fondo a un'immagine lunga non
    /// si aprirebbe più del tutto. Esatti restano i confini a bitrate costante,
    /// le immagini negli altri formati e le immagini MP3 su un disco o su una
    /// condivisione, che non sono remote.
    fn remoto(&self) -> bool {
        false
    }

    /// Riceve la bandiera con cui chi comanda chiede di smettere di aspettare.
    ///
    /// Chi implementa questo metodo la conserva e la guarda mentre aspetta la
    /// rete — fra un blocco e l'altro, fra un tentativo e l'altro — e, quando la
    /// trova alzata, restituisce subito un errore di lettura invece di
    /// continuare ad aspettare. È ciò che permette a un secondo trascinamento
    /// del cursore di non mettersi in fila dietro al primo, fermo su una rete
    /// lenta. Il resto del giro è in [`Decodificatore::apri_con_annullo`].
    ///
    /// Due regole per chi la implementa:
    ///
    /// - l'errore **non** dev'essere `ErrorKind::Interrupted`: symphonia lo
    ///   considera un «riprova» e rilegge da sé, all'infinito finché la
    ///   bandiera resta alzata;
    /// - la bandiera non si abbassa qui: la abbassa chi l'ha alzata.
    fn ascolta_annullo(&mut self, annullo: Arc<AtomicBool>) {
        let _ = annullo;
    }
}

impl Flusso for std::fs::File {
    fn lunghezza(&self) -> Option<u64> {
        self.metadata().ok().map(|m| m.len())
    }
}

/// Quel che serve per suonare un brano.
///
/// I byte arrivano come [`Flusso`] e non come percorso: è la giuntura che tiene
/// aperto Android, dove non esiste un percorso da aprire ma una concessione del
/// sistema. Chi costruisce questa struttura è `rymembra-app`, che sa da dove
/// prenderli.
pub struct Sorgente {
    /// Quale brano, per poterlo nominare negli errori e negli eventi.
    pub track_id: i64,
    /// I byte.
    pub media: Box<dyn Flusso>,
    /// L'estensione, come suggerimento al riconoscitore di formato.
    pub estensione: Option<String>,
    /// La durata secondo il database, per quando il file non la dichiara.
    pub durata_ms: u64,
    /// Di quanti decibel correggere il brano verso il riferimento di −18 LUFS:
    /// dal tag ReplayGain o dalla misura, secondo quel che `rymembra-app` ha
    /// scelto. `None`: nessuna correzione.
    pub replaygain_db: Option<f32>,
    /// Il picco vero del brano, lineare, se è stato misurato: il tetto a cui
    /// si ferma un guadagno che alza. Vedi [`crate::Livello`].
    pub picco: Option<f32>,
    /// Il pezzo del file che è il brano, quando il brano non è il file intero
    /// ma una traccia di un'immagine CD. `None`: tutto il file. Vedi
    /// [`Segmento`].
    pub segmento: Option<Segmento>,
}

impl std::fmt::Debug for Sorgente {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sorgente")
            .field("track_id", &self.track_id)
            .field("estensione", &self.estensione)
            .field("durata_ms", &self.durata_ms)
            .field("segmento", &self.segmento)
            .finish_non_exhaustive()
    }
}

/// Un brano che è un pezzo di un file più grande: una traccia di un'immagine
/// CD, descritta dal suo `.cue`.
///
/// Con un segmento il brano **è** il pezzo: si apre posizionato al suo inizio,
/// e posizione, durata, salti e ripresa contano da lì; finisce al suo ultimo
/// campione, esatto, e non al primo pacchetto che lo oltrepassa.
///
/// # Perché i fotogrammi del CD e non i millisecondi
///
/// Perché sono l'unità in cui il foglio è scritto — `INDEX 01 mm:ss:ff`, e un
/// fotogramma è un settantacinquesimo di secondo — e l'unica in cui il confine
/// fra due tracce è esatto. A 44,1 kHz un fotogramma sono 588 campioni tondi, e
/// la fine di una traccia è il primo campione della successiva: né uno in più
/// né uno in meno. In millisecondi quel confine cadrebbe a metà di un campione,
/// e due tracce di fila si sovrapporrebbero di un campione o ne perderebbero
/// uno. La conversione in campioni la fa [`campioni_da_cd`], a ogni frequenza.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segmento {
    /// Quale file: il percorso dell'immagine. Il motore non lo apre — i byte
    /// arrivano nella [`Sorgente`] come sempre — e serve solo a decidere se due
    /// brani sono contigui: vedi [`Segmento::precede`].
    pub immagine: String,
    /// Dove comincia il brano, in fotogrammi CD dall'inizio del file.
    pub inizio_cd: u32,
    /// Dove finisce, escluso, in fotogrammi CD dall'inizio del file. `None`:
    /// fino alla fine del file, com'è l'ultima traccia di un'immagine.
    pub fine_cd: Option<u32>,
}

impl Segmento {
    /// Il brano `prossimo` comincia esattamente dove finisce questo, nello
    /// stesso file?
    ///
    /// Solo allora fra i due non c'è niente — né un buco, né una dissolvenza
    /// — perché sono un segnale solo, tagliato in due dal foglio: la fine di un
    /// movimento che sfuma nel successivo, il pubblico di un disco dal vivo.
    ///
    /// # Perché una definizione così stretta
    ///
    /// Perché ogni passaggio che non sia «la traccia dopo, nell'ordine del
    /// disco» è un passaggio fra due cose che non si continuano, e attaccarle
    /// senza dissolvenza sarebbe un taglio netto in mezzo alla musica. Non sono
    /// contigui la ripetizione dello stesso brano (la fine non è il suo
    /// inizio), la riproduzione casuale e l'ordine inverso (l'inizio del
    /// prossimo non è questa fine), e il ritorno dall'ultima traccia alla prima
    /// (l'ultima non ha una fine scritta: va fino in fondo al file). Seguono la
    /// strada di sempre, con la dissolvenza se è accesa.
    #[must_use]
    pub fn precede(&self, prossimo: &Self) -> bool {
        self.immagine == prossimo.immagine && self.fine_cd == Some(prossimo.inizio_cd)
    }
}

/// I campioni dall'inizio del file fino a un punto del foglio, alla frequenza
/// data: `fotogrammi × frequenza / 75`, per difetto.
///
/// # Perché sui confini assoluti, e mai sulle lunghezze
///
/// Perché a una frequenza che 75 non divide — 32 kHz, 22 050 Hz — il conto ha un
/// resto, e arrotondare le lunghezze vorrebbe dire due tracce di fila che non
/// si toccano più. Arrotondando i **punti** del file, invece, la fine di una
/// traccia e l'inizio della successiva sono lo stesso numero per costruzione, e
/// le tracce di un'immagine la ricoprono esatte. A 44,1 e a 48 kHz il conto è
/// esatto comunque: 588 e 640 campioni per fotogramma.
#[expect(
    clippy::integer_division,
    reason = "il resto è una frazione di campione, e arrotondare sempre per \
              difetto lo stesso punto è ciò che fa combaciare due tracce"
)]
fn campioni_da_cd(fotogrammi: u32, frequenza: u32) -> u64 {
    // Due `u32` moltiplicati stanno in un `u64`: niente da saturare.
    u64::from(fotogrammi) * u64::from(frequenza) / 75
}

/// L'ultimo numero di sistema che il flusso ha incontrato.
///
/// # Perché il guasto va conservato di lato
///
/// Perché symphonia, quando il riconoscimento del contenitore non riesce, dice
/// «nessun lettore adatto» e **butta via** l'errore di lettura che glielo ha
/// impedito. Da fuori, una condivisione morta e un file che non è musica
/// arrivano identici: `Error::Unsupported`, senza causa. Ed è il caso peggiore
/// in cui confonderli, perché «formato non supportato» il catalogo lo dichiara
/// mai ritentabile — cioè niente «Riprova» per un FLAC intatto a cui manca solo
/// il cavo.
///
/// Questa casella è l'unico posto in cui quel numero sopravvive al passaggio.
/// Zero vuol dire «niente»: `ERROR_SUCCESS` non è un errore che qualcuno possa
/// restituire, quindi non serve un `Option` dentro un atomico.
#[derive(Clone, Default)]
struct Guasto(std::sync::Arc<std::sync::atomic::AtomicI32>);

impl Guasto {
    /// Prende nota, se il sistema ha dato un numero.
    fn segna(&self, err: &std::io::Error) {
        if let Some(numero) = err.raw_os_error() {
            self.0.store(numero, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Il guasto annotato era di rete? Allora ecco l'errore da raccontare.
    fn di_rete(&self) -> Option<AppError> {
        let numero = self.0.load(std::sync::atomic::Ordering::Relaxed);
        if numero == 0 {
            return None;
        }
        let err = std::io::Error::from_raw_os_error(numero);
        if !rymembra_domain::errors::rete::e_di_rete(&err) {
            return None;
        }
        Some(
            AppError::new(ErrorCode::FsNetworkUnavailable { path: None })
                .with_cause(err.to_string()),
        )
    }
}

/// Da [`Flusso`] al `MediaSource` di symphonia.
///
/// Il punto in cui il tratto pubblico di questo crate incontra quello della
/// libreria che ci sta sotto, e l'unico: cambiare decodificatore vuol dire
/// riscrivere questa struttura, non l'interfaccia.
struct Ponte {
    interno: Box<dyn Flusso>,
    lunghezza: Option<u64>,
    /// Dove finisce il numero di sistema dell'ultimo guasto. Vedi [`Guasto`].
    guasto: Guasto,
}

impl std::io::Read for Ponte {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.interno
            .read(buf)
            .inspect_err(|err| self.guasto.segna(err))
    }
}

impl std::io::Seek for Ponte {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.interno
            .seek(pos)
            .inspect_err(|err| self.guasto.segna(err))
    }
}

impl symphonia::core::io::MediaSource for Ponte {
    fn is_seekable(&self) -> bool {
        self.lunghezza.is_some()
    }

    fn byte_len(&self) -> Option<u64> {
        self.lunghezza
    }
}

/// La frequenza di campionamento più bassa che accettiamo da un file.
///
/// Ottomila hertz è il telefono: sotto non c'è musica, c'è un contenitore che
/// dichiara un numero che non descrive niente.
const FREQUENZA_MIN: u32 = 8_000;

/// La più alta.
///
/// Trecentottantaquattromila hertz è il DXD, cioè il massimo che un file audio
/// di consumo abbia mai ragione di dichiarare. Il tetto non serve a rifiutare
/// il DSD di qualcuno: serve a rifiutare i tre megahertz che escono da
/// un'intestazione letta male.
const FREQUENZA_MAX: u32 = 384_000;

/// Quanti canali al massimo.
///
/// Trentadue sta comodamente sopra qualunque configurazione reale — il 22.2
/// giapponese ne conta ventiquattro — e comodamente sotto i numeri che si
/// leggono in un'intestazione rovinata.
const CANALI_MAX: usize = 32;

/// La frequenza dichiarata dal file è un numero che può descrivere della musica?
///
/// # Perché serve un controllo, se il numero viene dal file
///
/// Perché **viene dal file**, e un file può arrivare da una condivisione di
/// rete che è morta a metà intestazione: dei byte troncati sono ancora un
/// numero, e quel numero finisce dritto in tre conti che non lo mettono in
/// discussione.
///
/// `FftFixedIn::new` dimensiona i propri buffer sul rapporto fra le due
/// frequenze **ridotto per il massimo comun divisore**: fra 44 100 e 48 000 il
/// divisore è grande e i buffer sono minuscoli, mentre fra un numero qualunque
/// di sette cifre e 48 000 può non esserci niente da semplificare — e allora
/// quei buffer crescono quanto il rapporto, per un file che magari dura tre
/// secondi. Il ricampionatore prima costruiva quella roba e poi ci lavorava
/// dentro.
///
/// `fotogrammi_da_ms` moltiplica per la frequenza e satura: con un numero
/// assurdo il cursore del brano finisce a un anno dall'inizio, e ci finisce
/// **dopo** aver spostato la puntina.
///
/// Rifiutare qui costa un confronto e restituisce un `Result` a chi ha premuto
/// play, che è il posto giusto in cui scoprire che quel file non si apre.
const fn frequenza_accettabile(hz: u32) -> bool {
    hz >= FREQUENZA_MIN && hz <= FREQUENZA_MAX
}

/// La frequenza d'uscita più alta che sappiamo servire.
///
/// Non è [`FREQUENZA_MAX`], che è il tetto dei **file**: la frequenza
/// d'uscita la sceglie il dispositivo — è il formato del mix che l'utente ha
/// impostato in Windows — e un DAC USB offre anche 705,6 e 768 kHz, che nessun
/// file di consumo dichiara. Misurata col tetto dei file, un'uscita così faceva
/// fallire **ogni** brano come `playback.decodeFailed`, cioè «questo file è
/// danneggiato»: l'utente veniva mandato a cercare il guasto nei file.
///
/// Settecentosessantottomila, e non di più, per l'anello: è dimensionato su
/// 192 kHz per otto canali (vedi `Motore::avvia_con_annunci`), che sono gli
/// stessi campioni al secondo di 768 kHz **in stereo** — fin lì, in stereo, la
/// riserva promessa regge; con più canali a queste frequenze l'anello ne tiene
/// meno (a 768 kHz su otto canali, una cinquantina di millisecondi). Oltre i
/// 768 kHz il dispositivo si rifiuta all'apertura come
/// `playback.engineUnavailable` — un problema dell'uscita, non dei brani.
const USCITA_MAX: u32 = 768_000;

/// La frequenza dell'uscita è una che sappiamo servire? Vedi [`USCITA_MAX`].
pub(crate) const fn frequenza_d_uscita_accettabile(hz: u32) -> bool {
    hz >= FREQUENZA_MIN && hz <= USCITA_MAX
}

/// Quanti fotogrammi alla volta entrano nel ricampionatore.
const BLOCCO: usize = 1024;

/// Quanto si resta indietro rispetto all'ultimo fotogramma dichiarato.
const MARGINE_FINE_MS: u64 = 120;

/// Quanto prima del bersaglio si comincia a decodificare, per i codec con
/// memoria.
///
/// # Perché si parte prima, se poi si butta
///
/// Perché MP3, AAC, Vorbis e Opus sono codec a trasformata con sovrapposizione:
/// ogni pacchetto finisce di costruirsi con la metà del successivo, e il primo
/// dopo un azzeramento esce con mezza finestra — un'ammaccatura, o nel caso di
/// Vorbis addirittura niente. Se il bersaglio cade proprio lì, quel che si
/// sente dopo il salto è lo schiocco del codec che si riavvia. Ottanta
/// millisecondi sono quel che RFC 7845 § 4.6 raccomanda per Opus, e bastano
/// largamente agli altri tre; i campioni di quella rincorsa si decodificano e
/// si buttano, esattamente come quelli fra l'inizio del pacchetto e il
/// bersaglio.
///
/// FLAC, ALAC e il PCM non hanno memoria fra un blocco e l'altro: per loro la
/// rincorsa è zero, e su una condivisione di rete sono byte che non si leggono.
const RINCORSA_MS: u64 = 80;

/// Il blocco decodificato più lungo per cui si prepara il buffer in anticipo.
///
/// È il blocco più lungo che FLAC ammetta. Il contenitore dichiara il proprio
/// massimo, e quel numero si usa — ma **viene dal file**: un'intestazione che
/// dichiarasse miliardi di fotogrammi per pacchetto non deve diventare
/// un'allocazione di miliardi di campioni prima ancora di aver decodificato
/// niente. Un pacchetto vero più lungo di così ingrandisce il buffer quando
/// arriva, una volta sola.
const TETTO_BUFFER: u64 = 65_535;

/// Com'è fatto quel che esce dal codec: quanti canali, quali, a che frequenza.
///
/// Si confronta a ogni pacchetto con quello che la catena si aspetta, perché
/// può cambiare a metà brano: fra un anello e l'altro di un Ogg concatenato, e
/// quando il contenitore non aveva dichiarato i canali — o li aveva solo
/// contati, come Matroska — e li descrive solo il codec.
///
/// # Perché non è più `Copy`
///
/// Perché in 0.6 i canali non sono più una maschera di bit ma un tipo che può
/// portare un elenco di etichette (`Channels::Custom`). Il confronto per
/// pacchetto resta senza allocazioni — [`Forma::descrive`] guarda la forma del
/// codec senza copiarla — e una copia si fa solo quando la forma cambia
/// davvero, cioè quasi mai.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Forma {
    canali: usize,
    disposizione: Channels,
    frequenza: u32,
}

impl Forma {
    /// La forma di quel che il codec ha appena consegnato.
    fn di(spec: &AudioSpec) -> Self {
        Self {
            canali: spec.channels().count(),
            disposizione: spec.channels().clone(),
            frequenza: spec.rate(),
        }
    }

    /// Quel che il codec ha appena consegnato ha questa forma?
    fn descrive(&self, spec: &AudioSpec) -> bool {
        self.frequenza == spec.rate() && &self.disposizione == spec.channels()
    }
}

/// Da timestamp del contenitore a fotogrammi del brano, e ritorno.
///
/// # Perché non basta moltiplicare per la frequenza
///
/// Perché i timestamp di symphonia non sono sempre fotogrammi. Sono unità di
/// `time_base`, che in MP4 è la scala dei tempi della traccia e non per forza
/// la sua frequenza, e in Matroska è il millisecondo; e non contano sempre dal
/// primo fotogramma che si sente. Da dove contano lo dice [`origine`].
///
/// # Il brano dentro il file
///
/// Con un [`Segmento`] il fotogramma zero del brano non è il primo del file ma
/// il suo inizio, e l'orologio lo sa: [`Self::fotogramma`] e [`Self::ts`]
/// parlano del brano, e tutto quel che ne discende — il bersaglio di un salto,
/// lo scarto fino a lì, i fotogrammi consegnati — conta dall'inizio del pezzo
/// senza saperlo. Solo la rincorsa di un salto guarda il file, perché può
/// cominciare prima del brano: vedi [`Self::ts_nel_file`].
#[derive(Debug, Clone, Copy)]
struct Orologio {
    /// Secondi per unità di timestamp, come frazione.
    numeratore: u128,
    denominatore: u128,
    /// La frequenza del codec.
    frequenza: u32,
    /// Il timestamp del primo fotogramma del file. Vedi [`origine`].
    origine: i64,
    /// Il fotogramma del file da cui comincia il brano: zero, o l'inizio del
    /// segmento in campioni. Vedi [`campioni_da_cd`].
    inizio: u64,
}

impl Orologio {
    /// L'orologio della traccia `traccia`, che il codec consegna a
    /// `frequenza`. `ogg` dice se il contenitore è un Ogg: vedi [`origine`].
    fn da(traccia: &Track, frequenza: u32, ogg: bool) -> Self {
        let (numeratore, denominatore) = match traccia.time_base {
            Some(base) => (u128::from(base.numer.get()), u128::from(base.denom.get())),
            None => (1, u128::from(frequenza.max(1))),
        };
        let mut orologio = Self {
            numeratore,
            denominatore,
            frequenza,
            origine: 0,
            inizio: 0,
        };
        orologio.origine = origine(traccia, ogg, &orologio);
        orologio
    }

    /// Il fotogramma del brano a cui corrisponde un timestamp.
    ///
    /// Con segno, perché un pacchetto può cominciare **prima** dell'inizio del
    /// brano: i primi fotogrammi di un Opus sono il pre-skip, quelli di un MP3
    /// il ritardo del codificatore — e, dentro un'immagine, tutto quel che sta
    /// prima del segmento.
    #[expect(
        clippy::integer_division,
        reason = "per la quasi totalità dei formati la scala è 1/frequenza e la \
                  divisione è esatta; per le altre il resto è una frazione di \
                  fotogramma"
    )]
    fn fotogramma(&self, ts: Timestamp) -> i128 {
        let relativo = i128::from(ts.get()) - i128::from(self.origine);
        let scala = i128::try_from(self.numeratore.saturating_mul(u128::from(self.frequenza)))
            .unwrap_or(i128::MAX);
        let denominatore = i128::try_from(self.denominatore).unwrap_or(1).max(1);
        (relativo.saturating_mul(scala) / denominatore).saturating_sub(i128::from(self.inizio))
    }

    /// Quante unità di timestamp valgono `fotogrammi` fotogrammi, per difetto.
    #[expect(
        clippy::integer_division,
        reason = "l'arrotondamento per difetto è voluto: vedi `Orologio::ts`"
    )]
    fn unita(&self, fotogrammi: u64) -> i64 {
        let scala = self
            .numeratore
            .saturating_mul(u128::from(self.frequenza))
            .max(1);
        let unita = u128::from(fotogrammi).saturating_mul(self.denominatore) / scala;
        i64::try_from(unita).unwrap_or(i64::MAX)
    }

    /// Il timestamp da chiedere a symphonia per il fotogramma `fotogramma` del
    /// brano, arrotondato per difetto: meglio atterrare un soffio prima e
    /// scartare, che un soffio dopo e perdere.
    #[cfg(test)]
    fn ts(&self, fotogramma: u64) -> Timestamp {
        self.ts_nel_file(fotogramma.saturating_add(self.inizio))
    }

    /// Come [`Self::ts`], per un fotogramma contato dall'inizio del **file**.
    ///
    /// Serve alla rincorsa di un salto, che su un codec con memoria comincia
    /// prima del bersaglio: al primo campione di un segmento vuol dire prima
    /// del brano, in un punto che contato dal brano sarebbe negativo.
    fn ts_nel_file(&self, fotogramma: u64) -> Timestamp {
        Timestamp::new(self.origine.saturating_add(self.unita(fotogramma)))
    }
}

/// Il timestamp del primo fotogramma del brano che si sente, cioè il
/// fotogramma zero di [`Orologio`].
///
/// # Perché due regole, e perché si guarda il contenitore
///
/// Perché in 0.6 i demultiplatori non contano tutti allo stesso modo, e la
/// differenza è quella fra un salto esatto e uno che perde il ritardo del
/// codificatore. La documentazione di symphonia dice che `Track::start_ts` è
/// il timestamp del primo fotogramma **decodificato** e `Track::delay` quanti
/// ne vanno buttati da lì: il primo che si sente sta a `start_ts + delay`. Ogg
/// la rispetta alla lettera — Vorbis ha `start_ts` negativo e il ritardo che
/// lo riporta a zero, Opus ha `start_ts` a zero e il pre-skip che conta dentro
/// la posizione granulare (e che [`opus::DecodificatoreOpus`] toglie dai
/// campioni).
///
/// Gli altri no. MP3 e CAF mettono i fotogrammi di ritardo a timestamp
/// **negativi** e lasciano `start_ts` a zero pur dichiarando il ritardo;
/// Matroska sottrae il ritardo del codec dai timestamp e non lo dichiara
/// affatto. Per tutti e tre il fotogramma zero è il timestamp zero, e sommarci
/// il ritardo farebbe saltare ogni volta i primi venticinque millisecondi di un
/// MP3. Per MP4, FLAC, WAV e AIFF le due regole danno lo stesso numero, perché
/// non dichiarano ritardo.
///
/// La regola di Ogg ha anche il pregio di reggere i flussi ritagliati da una
/// trasmissione, che cominciano a una posizione granulare lontana da zero.
fn origine(traccia: &Track, ogg: bool, orologio: &Orologio) -> i64 {
    let inizio = traccia.start_ts.get();
    if !ogg {
        return inizio;
    }
    let ritardo = orologio.unita(u64::from(traccia.delay.unwrap_or(0)));
    inizio.saturating_add(ritardo)
}

/// I parametri audio di una traccia, se è una traccia audio.
fn parametri_audio(traccia: &Track) -> Option<&AudioCodecParameters> {
    traccia
        .codec_params
        .as_ref()
        .and_then(CodecParameters::audio)
}

/// Il contenitore è un Ogg? Vedi [`origine`].
fn e_ogg(formato: &dyn FormatReader) -> bool {
    formato.format_info().format == FORMAT_ID_OGG
}

/// Quanti fotogrammi di rincorsa per questo codec. Vedi [`RINCORSA_MS`].
fn rincorsa(codec: AudioCodecId, frequenza: u32) -> u64 {
    let con_memoria = [
        CODEC_ID_MP1,
        CODEC_ID_MP2,
        CODEC_ID_MP3,
        CODEC_ID_AAC,
        CODEC_ID_VORBIS,
        CODEC_ID_OPUS,
    ]
    .contains(&codec);
    if con_memoria {
        fotogrammi_da_ms(RINCORSA_MS, frequenza)
    } else {
        0
    }
}

/// Com'è fatto davvero quel che si sta suonando, secondo chi lo decodifica.
///
/// # Perché c'è, se il database sa già queste cose
///
/// Perché il database sa quel che `lofty` ha letto alla scansione, e non è
/// sempre quel che si suona: un brano dell'Internet Archive tenuto come FLAC
/// si ascolta dal derivato MP3, un flusso non ha colonne tecniche affatto, e
/// la colonna di un M4A non dice se dentro c'è AAC o ALAC. Il codec che il
/// decodificatore ha in mano non sbaglia su nessuno dei tre.
///
/// E sa una cosa che il file non può sapere: a che frequenza esce. Su Windows
/// in modalità condivisa la decide il sistema, e un 44,1 kHz su un'uscita a
/// 48 viene ricampionato.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatoSuonato {
    /// Il nome del formato, come si scrive: «FLAC», «MP3», «ALAC». `None` per
    /// un codec che [`nome_del_codec`] non nomina: chi mostra ripiega sul
    /// database.
    pub codec: Option<&'static str>,
    /// La frequenza del brano, in hertz.
    pub frequenza: u32,
    /// I bit per campione, solo per i formati senza perdita. Su un MP3 il
    /// numero non esiste: il decodificatore ne riporterebbe uno inventato.
    pub bit: Option<u32>,
    /// Quanti canali ha il brano, non l'uscita.
    pub canali: u16,
    /// La frequenza a cui esce. Diversa da [`Self::frequenza`] vuol dire
    /// ricampionato.
    pub frequenza_uscita: u32,
    /// Il bitrate medio in kbit/s, dai byte della sorgente e dalla durata
    /// dichiarata dal contenitore, quando si sanno tutte e due. Comprende i
    /// tag e la copertina: per un file locale la misura di `lofty` è più
    /// giusta, e chi mostra la preferisce. Serve ai flussi, che non ne hanno
    /// un'altra.
    pub kbps: Option<u32>,
    /// I byte arrivano dalla rete: le colonne del database descrivono il file
    /// del catalogo, e quel che si suona può essere un suo derivato.
    pub remoto: bool,
}

/// Il nome di un codec come lo legge chi ascolta.
///
/// Il PCM non ha un nome suo: un WAV e un AIFF sono lo stesso codec in due
/// contenitori, e chi guarda conosce il contenitore. Un Monkey's Audio arriva
/// qui come PCM anche lui — lo decodifica il suo lettore, vedi [`crate::ape`] —
/// e il nome che chi ascolta conosce è quello del contenitore. Lo stesso per
/// Vorbis, che in un Ogg si chiama «Ogg Vorbis» come lo chiama la colonna del
/// database (vedi `etichetta_codec` in `rymembra-app`): due nomi diversi per lo
/// stesso file sarebbero un cambio di riga quando il brano parte.
fn nome_del_codec(codec: AudioCodecId, contenitore: FormatId) -> Option<&'static str> {
    let pcm = (CODEC_ID_PCM_S32LE..=CODEC_ID_PCM_MULAW).contains(&codec)
        || (CODEC_ID_ADPCM_G722..=CODEC_ID_ADPCM_IMA_QT).contains(&codec);
    if pcm {
        return Some(match contenitore {
            FORMAT_ID_WAVE => "WAV",
            FORMAT_ID_AIFF => "AIFF",
            FORMAT_ID_CAF => "CAF",
            FORMAT_ID_APE => "APE",
            _ => "PCM",
        });
    }
    let nome = match codec {
        CODEC_ID_FLAC => "FLAC",
        CODEC_ID_ALAC => "ALAC",
        CODEC_ID_WAVPACK => "WavPack",
        CODEC_ID_MONKEYS_AUDIO => "APE",
        CODEC_ID_TTA => "TTA",
        CODEC_ID_MP3 => "MP3",
        CODEC_ID_MP2 => "MP2",
        CODEC_ID_MP1 => "MP1",
        CODEC_ID_AAC => "AAC",
        CODEC_ID_OPUS => "Opus",
        CODEC_ID_VORBIS if contenitore == FORMAT_ID_OGG => "Ogg Vorbis",
        CODEC_ID_VORBIS => "Vorbis",
        _ => return None,
    };
    Some(nome)
}

/// Il codec conserva il campione com'era? Solo allora i bit si mostrano.
fn senza_perdita(codec: AudioCodecId) -> bool {
    [
        CODEC_ID_FLAC,
        CODEC_ID_ALAC,
        CODEC_ID_WAVPACK,
        CODEC_ID_MONKEYS_AUDIO,
        CODEC_ID_TTA,
    ]
    .contains(&codec)
        || (CODEC_ID_PCM_S32LE..=CODEC_ID_PCM_MULAW).contains(&codec)
}

/// Il bitrate medio, da quanti byte e quanti fotogrammi alla frequenza data.
///
/// `None` se manca uno dei tre o se il conto non sta in un `u32`. Arrotonda al
/// kilobit più vicino, come `lofty`.
fn kbps_medi(byte: Option<u64>, fotogrammi: Option<u64>, frequenza: u32) -> Option<u32> {
    let byte = u128::from(byte?);
    let fotogrammi = u128::from(fotogrammi.filter(|&f| f > 0)?);
    let bit_per_secondo = byte.checked_mul(8)?.checked_mul(u128::from(frequenza))?;
    let kbps = bit_per_secondo
        .checked_add(fotogrammi.checked_mul(500)?)?
        .checked_div(fotogrammi.checked_mul(1000)?)?;
    u32::try_from(kbps).ok().filter(|&k| k > 0)
}

/// Un brano aperto, che consegna campioni alla frequenza dell'uscita.
pub struct Decodificatore {
    track_id: i64,
    formato: Box<dyn FormatReader>,
    decodificatore: Box<dyn AudioDecoder>,
    /// L'identificativo che symphonia dà alla traccia dentro il contenitore.
    traccia: u32,
    /// Com'è fatto quel che esce dal codec, secondo l'ultimo pacchetto.
    forma: Forma,
    canali_uscita: usize,
    frequenza_uscita: u32,
    /// Dai canali di [`Self::forma`] a quelli dell'uscita.
    matrice: Matrice,
    ricampionatore: Option<Ricampionatore>,
    orologio: Orologio,
    /// I fotogrammi di rincorsa prima di un bersaglio. Vedi [`RINCORSA_MS`].
    rincorsa: u64,
    /// Dove deve cominciare quel che esce, dopo un salto: il fotogramma del
    /// brano, alla frequenza del codec. Tutto quel che viene prima si
    /// decodifica e si butta.
    salto: Option<u64>,
    /// Il buffer interlacciato, riusato da un pacchetto all'altro.
    campioni: Vec<f32>,
    /// Quanti fotogrammi preparare nel buffer, al primo pacchetto.
    capienza: u64,
    /// I fotogrammi consegnati finora, alla frequenza dell'uscita.
    consegnati: u64,
    esaurito: bool,
    /// Il flusso è finito e la coda del ricampionatore è ancora dentro:
    /// aspetta che il motore la dia al brano dopo o la faccia consegnare. Vedi
    /// [`Self::passa_la_coda`].
    coda_sospesa: bool,
    /// Quanti fotogrammi in testa a quel che uscirà sono del brano prima, che
    /// ha passato a questo il suo ricampionatore con la coda dentro. Non
    /// contano in [`Self::consegnati`].
    ereditati: u64,
    /// L'ultimo millisecondo a cui il flusso accetta di saltare.
    ///
    /// Da `num_frames`, non da `durata_ms`: sono due numeri diversi, e questo è
    /// quello che symphonia userà per dire di no. Con un segmento è la sua
    /// lunghezza, contata dal suo inizio.
    fine_flusso_ms: Option<u64>,
    /// Il pezzo del file che è il brano, se è una traccia d'immagine. Serve a
    /// [`Self::contiguo_a`]; i tempi li porta già [`Orologio::inizio`].
    segmento: Option<Segmento>,
    /// Quanti fotogrammi dura il segmento, alla frequenza del codec: dove si
    /// taglia la fine. `None`: fino alla fine del file. Vedi [`Self::consuma`].
    taglio: Option<u64>,
    /// La fine del segmento è stata raggiunta: il giro dopo chiude il brano,
    /// senza leggere un altro pacchetto. Vedi [`Self::consuma`].
    al_taglio: bool,
    /// Il millisecondo del brano a cui comincia l'anello corrente di un Ogg
    /// concatenato. Zero per tutti gli altri file.
    origine_ms: u64,
    /// I byte arrivano dalla rete. Vedi [`Flusso::remoto`].
    remoto: bool,
    /// La bandiera dell'annullo, se chi ha aperto ne ha data una.
    annullo: Option<Arc<AtomicBool>>,
    /// Com'è fatto quel che si suona. Vedi [`FormatoSuonato`].
    formato_suonato: FormatoSuonato,
}

impl Decodificatore {
    /// Apre una sorgente e la prepara per la frequenza e i canali richiesti.
    pub fn apri(sorgente: Sorgente, frequenza: u32, canali: u16) -> Result<Self, AppError> {
        Self::apri_dentro(sorgente, frequenza, canali, None)
    }

    /// Come [`Self::apri`], con una bandiera per smettere di aspettare la rete.
    ///
    /// # Il giro della bandiera
    ///
    /// Chi comanda la alza quando una lettura in corso non serve più — un
    /// secondo salto mentre il primo aspetta ancora la rete, un brano cambiato,
    /// uno stop — e la abbassa quando il filo della decodifica ha preso in mano
    /// il comando nuovo. Nel mezzo:
    ///
    /// - la sorgente la riceve con [`Flusso::ascolta_annullo`], e un flusso di
    ///   rete smette di aspettare e restituisce un errore;
    /// - qualunque errore arrivi mentre la bandiera è alzata diventa qui
    ///   `internal.aborted`, e non un guasto di rete o un file rovinato: un
    ///   annullamento voluto non deve far comparire «Riprova» né far
    ///   sostituire un file sano. Il decodificatore resta com'è — il comando
    ///   nuovo è, di solito, un altro salto.
    ///
    /// # Errori
    ///
    /// Quelli di [`Self::apri`], più `internal.aborted` se la bandiera si alza
    /// mentre si riconosce il contenitore.
    pub fn apri_con_annullo(
        sorgente: Sorgente,
        frequenza: u32,
        canali: u16,
        annullo: Arc<AtomicBool>,
    ) -> Result<Self, AppError> {
        Self::apri_dentro(sorgente, frequenza, canali, Some(annullo))
    }

    fn apri_dentro(
        mut sorgente: Sorgente,
        frequenza: u32,
        canali: u16,
        annullo: Option<Arc<AtomicBool>>,
    ) -> Result<Self, AppError> {
        let track_id = sorgente.track_id;
        let estensione = sorgente.estensione.clone();
        let formato_dichiarato = || estensione.clone();
        let segmento = sorgente.segmento.take();

        // Un formato che sappiamo di non saper suonare si dichiara subito, con
        // il codice che esiste apposta nel catalogo. Lasciarlo arrivare al
        // riconoscitore darebbe un «decodifica fallita» generico, che manda a
        // cercare il guasto nel posto sbagliato.
        if let Some(ext) = sorgente.estensione.as_deref()
            && Codec::da_estensione(ext) == Codec::NonSupportato
        {
            return Err(AppError::new(ErrorCode::PlaybackFormatUnsupported {
                format: Some(ext.to_owned()),
            }));
        }

        if let Some(bandiera) = annullo.as_ref() {
            sorgente.media.ascolta_annullo(Arc::clone(bandiera));
        }
        let remoto = sorgente.media.remoto();
        let lunghezza = sorgente.media.lunghezza();

        let guasto = Guasto::default();
        let flusso = MediaSourceStream::new(
            Box::new(Ponte {
                lunghezza,
                interno: sorgente.media,
                guasto: guasto.clone(),
            }),
            symphonia::core::io::MediaSourceStreamOptions::default(),
        );
        // Il ripiego di tutti e tre i modi in cui il contenitore può non
        // riconoscersi. Guarda **prima** l'annullo e poi se sotto c'era la
        // rete, perché nessuno dei due è un formato ignoto.
        let non_supportato = |causa: String| {
            annullato(annullo.as_ref())
                .or_else(|| guasto.di_rete())
                .unwrap_or_else(|| {
                    AppError::new(ErrorCode::PlaybackFormatUnsupported {
                        format: formato_dichiarato(),
                    })
                    .with_cause(causa)
                })
        };
        // Un suggerimento che la 0.6.1 riceve e non guarda: il riconoscitore va
        // per contenuto, e l'estensione non sposta niente. Si passa lo stesso,
        // perché è l'interfaccia e perché costa una stringa.
        let mut suggerimento = Hint::new();
        if let Some(ext) = sorgente.estensione.as_deref() {
            suggerimento.with_extension(ext);
        }

        // Il gapless non si chiede più qui ma al codec: vedi [`opzioni_codec`].
        let formato = SONDA
            .probe(
                &suggerimento,
                flusso,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            // Il riconoscimento del contenitore **legge**, e su un percorso di
            // rete quella lettura può fallire per la rete e non per il formato:
            // senza questo ramo un FLAC intero su una share appena caduta
            // veniva dichiarato «formato non supportato», che il catalogo
            // dichiara mai ritentabile — cioè niente «Riprova», per un file che
            // non ha niente che non va.
            .map_err(|err| {
                annullato(annullo.as_ref())
                    .or_else(|| se_di_rete(&err))
                    .unwrap_or_else(|| non_supportato(err.to_string()))
            })?;

        // La prima traccia audio con un codec che symphonia sa nominare; una
        // copia, perché da qui in poi il contenitore serve mutabile.
        let traccia = formato
            .first_track_known_codec(TrackType::Audio)
            .cloned()
            .ok_or_else(|| non_supportato("nessuna traccia audio nel contenitore".to_owned()))?;
        let parametri = parametri_audio(&traccia)
            .cloned()
            .ok_or_else(|| non_supportato("nessuna traccia audio nel contenitore".to_owned()))?;
        let numero_traccia = traccia.id;

        let decodificatore = REGISTRO
            .make_audio_decoder(&parametri, &opzioni_codec())
            .map_err(|err| non_supportato(err.to_string()))?;

        // Frequenza e canali come li dice il codec, e come li dice il
        // contenitore solo se il codec tace: sono quelli di quel che uscirà. Per
        // quasi tutti i formati coincidono; non per Opus in Matroska, che il
        // contenitore conta e basta e il codec descrive (vedi
        // [`opus::DecodificatoreOpus`]).
        let dal_codec = decodificatore.codec_params();
        let frequenza_sorgente =
            dal_codec
                .sample_rate
                .or(parametri.sample_rate)
                .ok_or_else(|| {
                    AppError::new(ErrorCode::PlaybackDecodeFailed {
                        track_id: Some(track_id),
                        format: formato_dichiarato(),
                    })
                    .with_cause("frequenza di campionamento non dichiarata".to_owned())
                })?;
        let disposizione = dal_codec
            .channels
            .clone()
            .or_else(|| parametri.channels.clone());
        let forma = Forma {
            canali: disposizione.as_ref().map_or(2, Channels::count).max(1),
            disposizione: disposizione.unwrap_or_default(),
            frequenza: frequenza_sorgente,
        };
        let canali_uscita = usize::from(canali).max(1);

        // Quel che il file **dichiara** si controlla qui, e qui e non prima: il
        // contenitore è già stato riconosciuto, quindi una share morta a metà
        // intestazione è già uscita di sotto come guasto di rete ritentabile e
        // non viene scambiata per un file con dei numeri assurdi. L'ordine è
        // quello che tiene verde
        // `un_flac_su_una_share_che_muore_non_diventa_un_formato_ignoto` in
        // `motore/prove.rs`.
        //
        // Il codice è `playback.decodeFailed`, che è la verità: il file c'è, si
        // legge, e non contiene qualcosa che si possa suonare. Il perché di un
        // controllo su un numero che viene dal file sta in
        // [`frequenza_accettabile`].
        forma_accettabile(&forma, track_id, formato_dichiarato())?;
        // Il tetto dei salti, e vale la pena dire da dove **non** viene:
        // `sorgente.durata_ms` è quel che il database ha registrato alla
        // scansione, e symphonia non l'ha mai vista. Chi rifiuta il salto
        // guarda `num_frames` della traccia — i fotogrammi che si sentono, già
        // senza ritardo e imbottitura — quindi il tetto va calcolato da lì, e
        // alla frequenza che dichiara il contenitore, che è quella in cui li ha
        // contati.
        //
        // Con un segmento il tetto è la sua fine, contata dal suo inizio — e
        // mai oltre quel che il file contiene davvero, se un foglio sbagliato
        // dichiarasse una fine dopo l'ultimo campione. I punti del foglio
        // diventano campioni alla frequenza del codec, che è quella dei
        // pacchetti da tagliare.
        let inizio = segmento
            .as_ref()
            .map_or(0, |s| campioni_da_cd(s.inizio_cd, frequenza_sorgente));
        let taglio = segmento
            .as_ref()
            .and_then(|s| s.fine_cd)
            .map(|fine| campioni_da_cd(fine, frequenza_sorgente).saturating_sub(inizio));
        let fine_flusso_ms = if segmento.is_some() {
            let nel_file = traccia.num_frames.map(|n| n.saturating_sub(inizio));
            let lunghezza = match (taglio, nel_file) {
                (Some(t), Some(n)) => Some(t.min(n)),
                (t, n) => t.or(n),
            };
            fine_da_fotogrammi(lunghezza, frequenza_sorgente)
        } else {
            fine_da_fotogrammi(
                traccia.num_frames,
                parametri.sample_rate.unwrap_or(frequenza_sorgente),
            )
        };

        let (matrice, ricampionatore) = catena(&forma, canali_uscita, frequenza, track_id)?;
        let ogg = e_ogg(formato.as_ref());
        let mut orologio = Orologio::da(&traccia, frequenza_sorgente, ogg);
        orologio.inizio = inizio;
        let formato_suonato = FormatoSuonato {
            codec: nome_del_codec(parametri.codec, formato.format_info().format),
            frequenza: forma.frequenza,
            bit: senza_perdita(parametri.codec)
                .then(|| parametri.bits_per_sample.or(dal_codec.bits_per_sample))
                .flatten(),
            canali: u16::try_from(forma.canali).unwrap_or(u16::MAX),
            frequenza_uscita: frequenza,
            kbps: kbps_medi(
                lunghezza,
                traccia.num_frames,
                parametri.sample_rate.unwrap_or(frequenza_sorgente),
            ),
            remoto,
        };

        let mut aperto = Self {
            track_id,
            formato,
            decodificatore,
            traccia: numero_traccia,
            forma,
            canali_uscita,
            frequenza_uscita: frequenza,
            matrice,
            ricampionatore,
            orologio,
            rincorsa: rincorsa(parametri.codec, frequenza_sorgente),
            salto: None,
            campioni: Vec::new(),
            capienza: parametri
                .max_frames_per_packet
                .unwrap_or(0)
                .min(TETTO_BUFFER),
            consegnati: 0,
            esaurito: false,
            coda_sospesa: false,
            ereditati: 0,
            fine_flusso_ms,
            origine_ms: 0,
            remoto,
            annullo,
            formato_suonato,
            segmento,
            taglio,
            al_taglio: false,
        };
        // Un segmento che non comincia in testa al file si apre già al suo
        // inizio: è un salto allo zero del brano, con la rincorsa dei codec con
        // memoria — che qui pesca nella traccia prima — e lo scarto fino al
        // campione esatto. Il primo fotogramma che esce è il primo del brano.
        if inizio > 0 {
            aperto.cerca(0)?;
        }
        Ok(aperto)
    }

    /// Com'è fatto quel che questo decodificatore suona.
    #[must_use]
    pub const fn formato_suonato(&self) -> FormatoSuonato {
        self.formato_suonato
    }

    /// La bandiera dell'annullo, se è stato aperto con una. Vedi
    /// [`Self::apri_con_annullo`].
    #[must_use]
    pub fn bandiera(&self) -> Option<Arc<AtomicBool>> {
        self.annullo.clone()
    }

    /// Quale brano sta decodificando.
    #[must_use]
    pub const fn track_id(&self) -> i64 {
        self.track_id
    }

    /// Quanti fotogrammi ha consegnato finora, alla frequenza dell'uscita.
    #[must_use]
    pub const fn consegnati(&self) -> u64 {
        self.consegnati
    }

    /// L'ultimo millisecondo a cui questo flusso accetta di saltare, quando il
    /// contenitore lo dichiara.
    #[must_use]
    pub const fn fine_flusso_ms(&self) -> Option<u64> {
        self.fine_flusso_ms
    }

    /// Il pezzo di file che questo decodificatore suona, se è una traccia
    /// d'immagine.
    #[must_use]
    pub const fn segmento(&self) -> Option<&Segmento> {
        self.segmento.as_ref()
    }

    /// Il brano `seguito` continua questo senza niente in mezzo? Solo se sono
    /// due segmenti dello stesso file e il secondo comincia dove finisce il
    /// primo: vedi [`Segmento::precede`], che dice anche perché non basta di
    /// meno.
    #[must_use]
    pub fn contiguo_a(&self, seguito: &Self) -> bool {
        match (self.segmento.as_ref(), seguito.segmento.as_ref()) {
            (Some(questo), Some(prossimo)) => questo.precede(prossimo),
            _ => false,
        }
    }

    /// Si riposiziona, al fotogramma. La coda interna del ricampionatore va
    /// buttata.
    ///
    /// `ms` conta dall'inizio del brano: per una traccia d'immagine, dal suo
    /// inizio e non da quello del file. Vedi [`Segmento`].
    ///
    /// # Perché il bersaglio si raggiunge scartando
    ///
    /// Perché symphonia non atterra dove gli si chiede: atterra all'inizio del
    /// **pacchetto** che contiene il bersaglio. Su MP3 sono fino a 1152
    /// fotogrammi prima, più i fotogrammi di riserva di bit che il salto si
    /// porta dietro — un centinaio di millisecondi; su FLAC un blocco intero,
    /// 4096 fotogrammi; sul WAV il confine del pacchetto simulato. Prima quel
    /// divario si ignorava e il cursore si metteva sul millisecondo chiesto:
    /// il suono restava indietro di un pacchetto rispetto al cursore e al testo
    /// sincronizzato.
    ///
    /// Ora il bersaglio si annota in [`Self::salto`], e
    /// [`Self::prossimo`] decodifica e butta tutto quel che cade prima,
    /// guardando il timestamp di ogni pacchetto — non `SeekedTo::actual_ts`,
    /// che dice solo dove comincia il primo e non sa dei pacchetti che il
    /// codec rifiuterà. Per i codec con memoria si parte anche un po' prima:
    /// vedi [`RINCORSA_MS`].
    ///
    /// # Perché il guasto passa da [`Decodificatore::guasto`]
    ///
    /// Perché un salto **legge**, e su una condivisione di rete legge parecchio:
    /// `SeekMode::Accurate` va al punto e poi ridecodifica fino al fotogramma
    /// esatto, e su FLAC ci mette in mezzo anche la seek table. È quindi uno dei
    /// punti in cui la share morente si fa sentire per prima — e finché
    /// l'errore diventava un `playback.decodeFailed` qualunque, trascinare il
    /// cursore a rete giù raccontava che il file era danneggiato, senza offrire
    /// «Riprova» e senza lasciare annotato il punto a cui tornare.
    ///
    /// # Perché un salto oltre la fine non è un guasto
    ///
    /// Perché nessuno l'ha rotto: symphonia rifiuta e basta, e lo dice in due
    /// modi — vedi [`oltre_la_fine`].
    /// Trascinare il cursore fino in fondo a un FLAC lo produceva **sempre**, e
    /// il ripiego lo raccontava come `playback.decodeFailed`: il catalogo lo
    /// dichiara mai ritentabile e il testo consiglia di sostituire il file. Qui
    /// invece il brano si dichiara finito, la coda avanza al giro dopo di
    /// [`Decodificatore::prossimo`], e nessuno manda l'utente a buttare un FLAC
    /// intatto.
    ///
    /// # L'ordine dei riconoscimenti, che è la cosa da non sbagliare
    ///
    /// L'annullo si guarda **per primo**: un flusso a cui è stato chiesto di
    /// smettere risponde con un errore che somiglia a un guasto di rete, e non
    /// lo è. Poi la rete. Una condivisione che muore proprio durante un salto
    /// arriva anche lei come `IoError`, e presa per «fine del brano» verrebbe
    /// inghiottita in silenzio: niente errore, niente «Riprova», niente punto a
    /// cui tornare — e il brano successivo partirebbe come se il primo fosse
    /// solo finito prima. È il caso che difende
    /// `una_share_che_muore_durante_un_salto_non_dice_che_il_file_e_rotto` in
    /// `motore/prove.rs`, e invertire i rami la fa cadere.
    pub fn cerca(&mut self, ms: u64) -> Result<(), AppError> {
        let bersaglio = self.bersaglio(ms);
        // Dentro l'anello corrente: su un Ogg concatenato symphonia sa saltare
        // solo lì, e i suoi timestamp ricominciano da zero a ogni anello.
        let dentro_ms = bersaglio.saturating_sub(self.origine_ms);
        let punto = fotogrammi_da_ms(dentro_ms, self.orologio.frequenza);
        // La rincorsa si conta nel file e non nel brano: al primo campione di
        // un segmento parte dentro la traccia prima, che è dove il codec deve
        // scaldarsi. Contata nel brano si fermerebbe a zero, e il primo
        // pacchetto del segmento uscirebbe con lo schiocco che la rincorsa
        // esiste per togliere.
        let partenza = punto
            .saturating_add(self.orologio.inizio)
            .saturating_sub(self.rincorsa);
        // Accurato su tutto quel che non è remoto, anche dentro un'immagine MP3,
        // e lo si paga: il salto accurato di symphonia conta i fotogrammi MPEG
        // dall'inizio del file, e aprire un brano a metà di un'immagine lunga
        // su una condivisione rilegge tutto fino a lì. Quello «a grandi linee»
        // leggerebbe poco, ma stima i timestamp in proporzione ai byte e li
        // sbaglia — di un fotogramma o due su un bitrate costante, di secondi o
        // minuti su uno variabile — e fra due brani contigui sarebbe un salto o
        // una ripetizione che si sente. Su disco e su condivisione l'esattezza
        // vince sulla lentezza.
        //
        // Su un flusso remoto no, nemmeno dentro un'immagine MP3: lì il salto
        // accurato rileggerebbe dall'inizio attraverso la rete, dentro la
        // scadenza dell'apertura, e il brano non si aprirebbe. Le tracce di
        // un'immagine MP3 nel cloud hanno quindi i confini stimati, esatti solo
        // a bitrate costante: è un limite dichiarato, vedi [`Flusso::remoto`].
        let modo = if self.remoto {
            SeekMode::Coarse
        } else {
            SeekMode::Accurate
        };
        // L'esito si lega prima: `self.formato` è preso in prestito mutabile
        // dalla `seek`, e chiamare `self.guasto` dentro il `map_err` vorrebbe
        // dire prenderlo una seconda volta.
        let esito = self.formato.seek(
            modo,
            SeekTo::Timestamp {
                ts: self.orologio.ts_nel_file(partenza),
                track_id: self.traccia,
            },
        );
        if let Err(err) = esito {
            // 1. L'annullo, che non è un guasto di nessun tipo.
            if let Some(annullo) = self.annullato() {
                return Err(annullo);
            }
            // 2. La rete, che è ritentabile e non è una fine di brano.
            if let Some(rete) = se_di_rete(&err) {
                return Err(rete);
            }
            // 3. Il tetto non arriva ovunque — un MP3 senza intestazione Xing
            //    dichiara `num_frames` per stima, e non ogni contenitore lo
            //    dichiara: qui il rifiuto si accetta per quello che è, cioè un
            //    brano finito.
            if oltre_la_fine(&err) {
                // …tranne quando dice che un brano d'immagine sta in un anello
                // dopo di un Ogg concatenato: symphonia cerca solo nell'anello
                // corrente, e quel brano risulterebbe vuoto senza un perché.
                // Si dice invece che non lo sappiamo suonare, come quando
                // l'anello comincia mentre si suona (vedi [`Self::nuovo_anello`]).
                if self.segmento.is_some()
                    && e_ogg(self.formato.as_ref())
                    && self.c_e_un_anello_dopo()?
                {
                    self.esaurito = true;
                    return Err(ogg_concatenato_in_immagine());
                }
                self.a_fine_flusso(bersaglio);
                return Ok(());
            }
            // 4. Tutto il resto è davvero un guasto.
            return Err(self.guasto(&err));
        }
        // Lo stato interno del decodificatore contiene fotogrammi che
        // appartengono al punto di prima: tenerli produrrebbe uno schiocco.
        self.decodificatore.reset();
        if let Some(r) = self.ricampionatore.as_mut() {
            r.svuota();
        }
        // Con la memoria del filtro se ne va anche la coda di un brano prima,
        // se questo l'aveva ereditata: dopo un salto non ne resta niente.
        self.coda_sospesa = false;
        self.ereditati = 0;
        self.esaurito = false;
        self.al_taglio = false;
        self.salto = Some(punto);
        // Il bersaglio e non `ms`: la posizione che si riporta è quella a cui si
        // è atterrati, non quella che era stata chiesta. Scriverci `ms`
        // significherebbe un cursore fermo oltre la fine mentre l'audio suona
        // gli ultimi millisecondi. E da ora in poi è anche il punto esatto da
        // cui esce il suono.
        self.consegnati = fotogrammi_da_ms(
            self.origine_ms.saturating_add(dentro_ms),
            self.frequenza_uscita,
        );
        Ok(())
    }

    /// Dopo l'anello corrente di un Ogg ne comincia un altro?
    ///
    /// Un salto oltre la fine dell'anello, dentro un'immagine, vuol dire una di
    /// due cose: il brano sta in un anello dopo, che non sappiamo suonare, o
    /// sta oltre la fine del file — un'immagine troncata, o un foglio che
    /// descrive più di quel che c'è — e allora è un brano vuoto come in ogni
    /// altro formato. Per saperlo si va a un secondo dalla fine dell'anello e
    /// si legge fino in fondo: qualche pagina, e o comincia un anello nuovo o
    /// finisce il file.
    ///
    /// `false` anche quando non si riesce a saperlo: si resta al brano vuoto,
    /// che era quel che succedeva prima.
    fn c_e_un_anello_dopo(&mut self) -> Result<bool, AppError> {
        /// I pacchetti da leggere al massimo: da un secondo prima della fine
        /// sono qualche decina, e oltre questo qualcosa non va come si crede.
        const TETTO: usize = 1_000;
        let fine = self
            .formato
            .tracks()
            .iter()
            .find(|t| t.id == self.traccia)
            .and_then(|t| Some((t.start_ts, t.num_frames?)));
        let Some((inizio, fotogrammi)) = fine else {
            return Ok(false);
        };
        let un_secondo = u64::from(self.orologio.frequenza);
        let ts = Timestamp::new(
            inizio
                .get()
                .saturating_add(self.orologio.unita(fotogrammi.saturating_sub(un_secondo))),
        );
        let esito = self.formato.seek(
            SeekMode::Accurate,
            SeekTo::Timestamp {
                ts,
                track_id: self.traccia,
            },
        );
        if let Err(err) = esito {
            return self.annullo_o_rete(&err).map_or(Ok(false), Err);
        }
        for _ in 0..TETTO {
            match self.formato.next_packet() {
                Ok(Some(_)) => {}
                Ok(None) => return Ok(false),
                Err(symphonia::core::errors::Error::ResetRequired) => return Ok(true),
                Err(err) => return self.annullo_o_rete(&err).map_or(Ok(false), Err),
            }
        }
        Ok(false)
    }

    /// L'annullo o la rete, se l'errore è uno dei due: i guasti che nessuna
    /// strada deve inghiottire.
    fn annullo_o_rete(&self, err: &symphonia::core::errors::Error) -> Option<AppError> {
        self.annullato().or_else(|| se_di_rete(err))
    }

    /// Dove si salta davvero, dato dove è stato chiesto di saltare.
    ///
    /// # Perché un margine, e non la fine esatta
    ///
    /// Perché `num_frames` è il conteggio del **contenitore**, e non è per forza
    /// il numero di fotogrammi che questo decodificatore consegnerà. In 0.6
    /// dovrebbe contare solo quelli che si sentono, senza ritardo né
    /// imbottitura, ma Ogg ci scrive la posizione granulare finale, che per
    /// Opus conta anche il pre-skip. E chi rifiuta il salto non confronta
    /// sempre lo stesso numero — FLAC guarda `num_frames`, MP3 ci somma
    /// l'imbottitura, Ogg cerca una pagina e non la trova — quindi mirare
    /// all'ultimo fotogramma dichiarato è mirare a un bersaglio che si sposta.
    ///
    /// [`MARGINE_FINE_MS`] sta sotto la granularità che quel salto aveva prima
    /// che si scartasse fino al bersaglio: un blocco FLAC è 4096 fotogrammi,
    /// cioè circa 93 ms a 44,1 kHz. Centoventi millisecondi non spostano il
    /// punto in cui l'utente si ritrova; una fine esatta sbagliata di un
    /// fotogramma sì, perché diventa un errore.
    fn bersaglio(&self, ms: u64) -> u64 {
        match self.fine_flusso_ms {
            Some(fine) => ms.min(fine.saturating_sub(MARGINE_FINE_MS)),
            // Senza `num_frames` non c'è un tetto da applicare: nemmeno symphonia
            // ne ha uno, e inventarne uno da `durata_ms` taglierebbe la coda dei
            // file che nel database sono registrati più corti di quel che sono.
            None => ms,
        }
    }

    /// Porta il decodificatore a flusso esaurito, senza produrre un errore.
    ///
    /// Lo stesso stato in cui lo lascia la fine naturale in
    /// [`Decodificatore::prossimo`], perché è la stessa cosa: il brano è finito.
    /// Le code del ricampionatore si buttano invece di essere consegnate — quel
    /// che contengono appartiene al punto da cui si è saltati via, e uscirebbe
    /// come uno schiocco al posto della fine del brano.
    fn a_fine_flusso(&mut self, bersaglio: u64) {
        self.esaurito = true;
        self.al_taglio = false;
        self.salto = None;
        self.decodificatore.reset();
        if let Some(r) = self.ricampionatore.as_mut() {
            r.svuota();
        }
        self.coda_sospesa = false;
        self.ereditati = 0;
        // La fine dichiarata quando c'è: dire `bersaglio` su un salto molto
        // oltre la fine metterebbe il cursore fuori dalla barra.
        let fine = self.fine_flusso_ms.unwrap_or(bersaglio);
        self.consegnati = fotogrammi_da_ms(fine, self.frequenza_uscita);
    }

    /// La bandiera dell'annullo è alzata? Allora ecco l'errore da dire.
    fn annullato(&self) -> Option<AppError> {
        annullato(self.annullo.as_ref())
    }

    /// Che guasto è, per chi sta sopra.
    ///
    /// # La distinzione che questa funzione esiste per fare
    ///
    /// Un errore che sale da symphonia può essere due cose molto diverse, e
    /// finché erano la stessa l'utente leggeva la peggiore delle due: che il
    /// suo file era danneggiato e conveniva sostituirlo.
    ///
    /// Il file è davvero rotto quando il decodificatore non riesce a farne dei
    /// campioni. Ma se il brano sta su una condivisione di rete e la
    /// condivisione muore mentre suona, quel che arriva qui è un errore di
    /// **lettura** — su Windows il 64, `ERROR_NETNAME_DELETED` — e il file è
    /// intatto: manca il cavo. Il primo non si ritenta, perché rileggere un
    /// file rotto dà lo stesso esito; il secondo sì, perché la rete torna.
    ///
    /// L'elenco dei numeri sta in [`rymembra_domain::errors::rete`], dov'è
    /// condiviso con chi apre il file invece di leggerlo: la stessa share deve
    /// raccontare la stessa cosa a metà brano e all'apertura.
    ///
    /// E prima di tutto l'annullo: vedi [`Self::apri_con_annullo`].
    fn guasto(&self, err: &symphonia::core::errors::Error) -> AppError {
        self.annullato()
            .or_else(|| se_di_rete(err))
            .unwrap_or_else(|| {
                AppError::new(ErrorCode::PlaybackDecodeFailed {
                    track_id: Some(self.track_id),
                    format: None,
                })
                .with_cause(err.to_string())
            })
    }

    /// Il blocco successivo, interlacciato e pronto per la scheda.
    ///
    /// `false` quando il brano è finito. Un pacchetto illeggibile in mezzo non
    /// interrompe: si salta e si va avanti, perché un settore rovinato a metà
    /// canzone deve costare un fruscio, non la fine dell'ascolto. Un errore di
    /// lettura vero invece termina.
    ///
    /// # Niente allocazioni per pacchetto
    ///
    /// Prima ogni pacchetto costruiva un buffer nuovo — decine di allocazioni
    /// al secondo nel filo che deve tenere pieno l'anello. Ora il buffer è uno,
    /// un `Vec` riservato al primo pacchetto sul più lungo che il contenitore
    /// dichiara: `copy_to_vec_interleaved` di symphonia 0.6 lo porta alla
    /// lunghezza del pacchetto senza riallocare finché la capienza basta, e
    /// cresce solo se ne arriva uno più lungo ancora.
    ///
    /// # Errori
    ///
    /// Un guasto di lettura o di decodifica che non si può saltare, o
    /// `internal.aborted` se la bandiera dell'annullo è alzata.
    pub fn prossimo(&mut self, fuori: &mut Vec<f32>) -> Result<bool, AppError> {
        self.prossimo_dentro(fuori, false)
    }

    /// Come [`Self::prossimo`], ma alla fine del flusso la coda del
    /// ricampionatore resta dentro invece di uscire.
    ///
    /// La chiama chi ha già in canna il brano dopo e sa che
    /// [`Self::passa_la_coda`] lo accetterebbe: quando questa restituisce
    /// `false`, chi chiama **deve** chiamare subito o quella o
    /// [`Self::consegna_la_coda`], perché gli ultimi millisecondi del brano
    /// sono ancora nel filtro. Senza ricampionatore, o con il filtro già
    /// vuoto, è identica a [`Self::prossimo`].
    ///
    /// # Errori
    ///
    /// Gli stessi di [`Self::prossimo`].
    pub fn prossimo_lasciando_la_coda(&mut self, fuori: &mut Vec<f32>) -> Result<bool, AppError> {
        self.prossimo_dentro(fuori, true)
    }

    fn prossimo_dentro(&mut self, fuori: &mut Vec<f32>, lascia: bool) -> Result<bool, AppError> {
        fuori.clear();
        if self.esaurito {
            return Ok(false);
        }
        loop {
            // L'annullo si guarda a ogni giro, e non solo sugli errori e alla
            // fine del flusso. Questo ciclo può girare a lungo **senza**
            // consegnare niente: pacchetti che non si decodificano e si
            // saltano, pacchetti di un'altra traccia, pacchetti vuoti — e su
            // una cartella di rete ognuno è una lettura. Un file rovinato per
            // un tratto lungo teneva il filo qui dentro, lettura dopo lettura,
            // mentre il salto o il brano scelto nel frattempo aspettavano in
            // coda. Una lettura d'atomica per pacchetto è niente accanto a
            // quel che costa decodificarlo.
            if let Some(annullo) = self.annullato() {
                return Err(annullo);
            }
            // Il segmento è finito anche se il file no: la fine passa dalla
            // stessa porta della fine del flusso, così la coda del
            // ricampionatore esce — o passa al brano dopo — come a ogni fine
            // brano. E non si legge un pacchetto in più, che su una
            // condivisione di rete è una lettura che non serve a niente.
            if self.al_taglio {
                return self.fine_del_flusso(fuori, lascia);
            }
            let pacchetto = match self.formato.next_packet() {
                Ok(Some(p)) => p,
                // La fine del flusso, che in 0.6 si dice così e non più con un
                // errore di lettura.
                Ok(None) => return self.fine_del_flusso(fuori, lascia),
                // Un Ogg concatenato: comincia un anello nuovo, con i suoi
                // parametri. Prima era un guasto — il brano si fermava lì e la
                // coda saltava al successivo, a metà di una registrazione.
                Err(symphonia::core::errors::Error::ResetRequired) => {
                    if let Err(err) = self.nuovo_anello(fuori) {
                        self.esaurito = true;
                        return Err(err);
                    }
                    if fuori.is_empty() {
                        continue;
                    }
                    self.conta(fuori);
                    return Ok(true);
                }
                Err(err) => return self.fine_o_guasto(&err, fuori, lascia),
            };

            if pacchetto.track_id != self.traccia {
                continue;
            }
            // Il fotogramma del brano da cui comincia quel che il codec
            // consegnerà. In 0.6 il timestamp di un pacchetto è quello del suo
            // primo fotogramma **decodificato**, ritardo del codificatore
            // compreso — negativo, per il primo pacchetto di un MP3 o di un
            // Vorbis — e il codec ne toglie `trim_start` prima di consegnarlo:
            // il primo che esce sta lì. (In 0.5 il contenitore aveva già
            // spostato il timestamp per conto suo.) Un pacchetto senza taglio
            // in testa, che sono quasi tutti, comincia dove dice.
            let inizio = self
                .orologio
                .fotogramma(pacchetto.pts.saturating_add(pacchetto.trim_start));

            let (quanti, forma) = {
                let decodificato = match self.decodificatore.decode(&pacchetto) {
                    Ok(d) => d,
                    Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                    Err(symphonia::core::errors::Error::ResetRequired) => {
                        // Il codec chiede di essere rifatto da capo: si rifà con
                        // i parametri della stessa traccia e si continua.
                        if let Err(err) = self.rifai_codec() {
                            self.esaurito = true;
                            return Err(err);
                        }
                        continue;
                    }
                    Err(err) => {
                        self.esaurito = true;
                        return Err(self.guasto(&err));
                    }
                };

                let spec = decodificato.spec();
                let quanti = decodificato.frames();
                let canali = spec.channels().count();
                if quanti == 0 || canali == 0 {
                    continue;
                }
                // Il buffer riusato. La prima volta si riserva sul pacchetto
                // più lungo che il contenitore dichiara, così che i seguenti ci
                // stiano senza riallocare; un pacchetto ancora più lungo lo fa
                // crescere, una volta. Da lì in poi `copy_to_vec_interleaved`
                // cambia solo la lunghezza.
                let serve = quanti.saturating_mul(canali);
                if self.campioni.capacity() < serve {
                    let voluti = usize::try_from(self.capienza)
                        .unwrap_or(0)
                        .max(quanti)
                        .saturating_mul(canali);
                    self.campioni
                        .reserve(voluti.saturating_sub(self.campioni.len()));
                }
                decodificato.copy_to_vec_interleaved(&mut self.campioni);
                // La forma si copia solo se è cambiata, cioè quasi mai.
                let forma = (!self.forma.descrive(spec)).then(|| Forma::di(spec));
                (quanti, forma)
            };
            let esito = self.consuma(inizio, quanti, forma, fuori);
            if let Err(err) = esito {
                self.esaurito = true;
                return Err(err);
            }

            if !fuori.is_empty() {
                self.conta(fuori);
                return Ok(true);
            }
        }
    }

    /// Il flusso è finito, o almeno così dice: è la fine o un annullo?
    ///
    /// Un annullo non chiude il brano: il comando che l'ha chiesto, di solito un
    /// altro salto, lo riporta dove serve. Si guarda anche qui, e non solo sugli
    /// errori, perché in 0.5 la fine arrivava come errore di lettura e passava
    /// comunque da quella domanda: un flusso di rete a cui si chiede di smettere
    /// può ben rispondere «non c'è altro».
    fn fine_del_flusso(&mut self, fuori: &mut Vec<f32>, lascia: bool) -> Result<bool, AppError> {
        if let Some(annullo) = self.annullato() {
            return Err(annullo);
        }
        // Fine del flusso: quel che resta nel ricampionatore va consegnato,
        // fino all'ultimo fotogramma che spetta al brano — altrimenti si
        // perdono gli ultimi millisecondi di ogni brano, e su un album senza
        // stacchi si sente.
        self.esaurito = true;
        self.salto = None;
        // …a meno che chi chiama abbia in canna il brano che la prende: allora
        // resta nel filtro, e esce attaccata al suo seguito vero invece che a
        // del silenzio. Vedi [`Self::passa_la_coda`].
        if lascia
            && self
                .ricampionatore
                .as_ref()
                .is_some_and(|r| r.in_sospeso() > 0)
        {
            self.coda_sospesa = true;
            return Ok(false);
        }
        self.chiudi_la_coda(fuori)
    }

    /// Il flusso ha smesso di dare pacchetti con un errore: è la fine, un
    /// annullo o un guasto?
    ///
    /// `UnexpectedEof` vale ancora come fine: la 0.6 dice la fine con
    /// `Ok(None)`, ma un file troncato — l'ultimo blocco di un FLAC tagliato a
    /// metà, un Ogg che non arriva alla sua ultima pagina — finisce ancora
    /// così, ed è un brano che si ascolta fino a dove c'è, come prima.
    fn fine_o_guasto(
        &mut self,
        err: &symphonia::core::errors::Error,
        fuori: &mut Vec<f32>,
        lascia: bool,
    ) -> Result<bool, AppError> {
        if let Some(annullo) = self.annullato() {
            return Err(annullo);
        }
        if let symphonia::core::errors::Error::IoError(io) = err
            && io.kind() == std::io::ErrorKind::UnexpectedEof
        {
            return self.fine_del_flusso(fuori, lascia);
        }
        self.esaurito = true;
        Err(self.guasto(err))
    }

    /// Consegna la coda che [`Self::prossimo_lasciando_la_coda`] aveva
    /// lasciato nel filtro, imbottita di silenzio come a ogni fine brano.
    ///
    /// `true` se ne è uscito qualcosa: chi chiama lo tratta come un blocco
    /// qualunque di questo brano, e la chiamata dopo a [`Self::prossimo`] dirà
    /// `false`.
    ///
    /// # Errori
    ///
    /// Quelli di rubato, che su buffer dimensionati da lui non capitano.
    pub fn consegna_la_coda(&mut self, fuori: &mut Vec<f32>) -> Result<bool, AppError> {
        fuori.clear();
        self.chiudi_la_coda(fuori)
    }

    /// La coda del filtro in fondo a `fuori`, fino all'ultimo fotogramma che
    /// spetta al brano.
    fn chiudi_la_coda(&mut self, fuori: &mut Vec<f32>) -> Result<bool, AppError> {
        self.coda_sospesa = false;
        if let Some(r) = self.ricampionatore.as_mut() {
            r.coda(&self.matrice, fuori)?;
        }
        if fuori.is_empty() {
            return Ok(false);
        }
        self.conta(fuori);
        Ok(true)
    }

    /// Il brano dopo può prendere il ricampionatore di questo, con la coda
    /// dentro?
    ///
    /// Solo se tutti e due ricampionano, dalla stessa forma alla stessa
    /// uscita — altrimenti il filtro di uno non è il filtro dell'altro — e se
    /// il seguito non ha ancora consegnato niente: la coda si attacca al suo
    /// primo campione, non a un punto qualunque.
    ///
    /// Il primo campione del **brano**, che per una traccia d'immagine non è
    /// il primo del file: il seguito si apre già saltato al suo inizio, e quel
    /// salto è allo zero del brano — vedi [`Orologio::inizio`]. È ciò che fa di
    /// due tracce contigue un segnale solo, attraversato da un filtro solo come
    /// se il disco non fosse mai stato tagliato.
    #[must_use]
    pub fn puo_passare_la_coda(&self, seguito: &Self) -> bool {
        let intatto = seguito
            .ricampionatore
            .as_ref()
            .is_some_and(Ricampionatore::intatto);
        self.ricampionatore.is_some()
            && intatto
            && self.forma == seguito.forma
            && self.canali_uscita == seguito.canali_uscita
            && self.frequenza_uscita == seguito.frequenza_uscita
            && seguito.consegnati == 0
            && seguito.ereditati == 0
            && seguito.origine_ms == 0
            && !seguito.esaurito
            && seguito.salto.is_none_or(|punto| punto == 0)
    }

    /// Dà al brano dopo il ricampionatore con la coda di questo ancora dentro.
    ///
    /// # Perché
    ///
    /// Perché un album senza stacchi è **un** segnale, e due ricampionatori non
    /// lo sanno rifare: la ragione sta in testa a [`ricampionatore`]. Così la
    /// giuntura la attraversa un filtro solo, e le due metà escono come se
    /// fossero un file unico — alla griglia d'uscita giusta, e con ciascuna
    /// metà che vede l'altra invece del silenzio.
    ///
    /// In cambio il seguito riceve quello del seguito, intatto: se questo brano
    /// venisse ripreso da un salto, `cerca` lo svuota comunque, e la forma è la
    /// stessa per costruzione.
    ///
    /// Restituisce quanti fotogrammi in testa a quel che il seguito consegnerà
    /// sono ancora di questo brano: chi mette in fila i brani ci sposta il
    /// confine, e il seguito non li conta fra i suoi. `None` se lo scambio non
    /// si può fare o se non c'era niente da passare — e allora la coda, se era
    /// rimasta dentro, va consegnata con [`Self::consegna_la_coda`].
    pub fn passa_la_coda(&mut self, seguito: &mut Self) -> Option<u64> {
        if !self.coda_sospesa || !self.puo_passare_la_coda(seguito) {
            return None;
        }
        let ereditati = self
            .ricampionatore
            .as_ref()
            .map_or(0, Ricampionatore::in_sospeso);
        std::mem::swap(&mut self.ricampionatore, &mut seguito.ricampionatore);
        if let Some(r) = seguito.ricampionatore.as_mut() {
            r.passa_a(seguito.track_id);
        }
        if let Some(r) = self.ricampionatore.as_mut() {
            r.passa_a(self.track_id);
        }
        self.coda_sospesa = false;
        // Quei fotogrammi sono di questo brano, anche se li consegnerà l'altro.
        self.consegnati = self.consegnati.saturating_add(ereditati);
        seguito.ereditati = ereditati;
        Some(ereditati)
    }

    /// Quanti fotogrammi in testa alla prossima consegna sono ancora del brano
    /// prima. Vedi [`Self::passa_la_coda`].
    #[must_use]
    pub const fn ereditati(&self) -> u64 {
        self.ereditati
    }

    /// La coda è rimasta nel filtro: vedi [`Self::prossimo_lasciando_la_coda`].
    #[must_use]
    pub const fn coda_sospesa(&self) -> bool {
        self.coda_sospesa
    }

    /// Un pacchetto decodificato, dal salto fino alla scheda.
    ///
    /// I campioni stanno in [`Self::campioni`], interlacciati, e il loro primo
    /// fotogramma è il fotogramma `inizio` del brano. `forma` è quella nuova
    /// del codec, se è cambiata rispetto al pacchetto prima.
    ///
    /// # Il taglio della fine, qui e non dopo
    ///
    /// La fine di un segmento si taglia sui campioni del **codec**, prima del
    /// ricampionatore: è l'unico punto in cui un campione è un campione del
    /// file, e il confine del foglio cade esatto. Dopo il ricampionatore il
    /// confine sarebbe a metà fra due fotogrammi d'uscita, e il filtro avrebbe
    /// già mescolato l'ultimo campione del brano col primo della traccia dopo.
    fn consuma(
        &mut self,
        inizio: i128,
        quanti: usize,
        forma: Option<Forma>,
        fuori: &mut Vec<f32>,
    ) -> Result<(), AppError> {
        let scarto = self.da_scartare(inizio, quanti);
        let quanti = self.entro_il_taglio(inizio, quanti);
        if scarto >= quanti {
            return Ok(());
        }
        if let Some(forma) = forma {
            self.adegua(forma, fuori)?;
        }
        // Il buffer si toglie dalla struttura per il tempo della consegna, e ci
        // torna subito: il ricampionatore e la matrice sono presi in prestito
        // dalla stessa `self`. Lo scambio non alloca — un `Vec` vuoto non ha
        // niente da allocare.
        let campioni = std::mem::take(&mut self.campioni);
        let canali = self.forma.canali;
        let fine = quanti.saturating_mul(canali).min(campioni.len());
        let utili = campioni
            .get(scarto.saturating_mul(canali)..fine)
            .unwrap_or_default();
        let esito = match self.ricampionatore.as_mut() {
            Some(r) => r.spingi(utili, &self.matrice, fuori),
            None => {
                self.matrice.applica_interlacciato(utili, fuori);
                Ok(())
            }
        };
        self.campioni = campioni;
        esito
    }

    /// Quanti fotogrammi di questo pacchetto cadono prima del bersaglio di un
    /// salto, e vanno buttati.
    ///
    /// Tutti, se il pacchetto finisce prima del bersaglio: il salto resta in
    /// attesa del pacchetto dopo. Nessuno, se non c'è un salto in corso — o se
    /// symphonia è atterrata **dopo** il bersaglio, che col salto «a grandi
    /// linee» dei flussi può succedere: allora il cursore si sposta dove
    /// comincia davvero la musica, perché cursore e suono dicano la stessa
    /// cosa.
    fn da_scartare(&mut self, inizio: i128, quanti: usize) -> usize {
        let Some(bersaglio) = self.salto else {
            return 0;
        };
        let bersaglio = i128::from(bersaglio);
        let lungo = i128::try_from(quanti).unwrap_or(i128::MAX);
        if inizio.saturating_add(lungo) <= bersaglio {
            return quanti;
        }
        self.salto = None;
        if inizio > bersaglio {
            let dentro = u64::try_from(inizio).unwrap_or(0);
            let dentro_ms = ms_da_fotogrammi(dentro, self.orologio.frequenza);
            self.consegnati = fotogrammi_da_ms(
                self.origine_ms.saturating_add(dentro_ms),
                self.frequenza_uscita,
            );
            return 0;
        }
        usize::try_from(bersaglio - inizio).unwrap_or(quanti)
    }

    /// Quanti fotogrammi di questo pacchetto stanno prima della fine del
    /// segmento: tutti, senza un segmento o lontano dalla sua fine.
    ///
    /// Quando il pacchetto la raggiunge — anche esattamente col suo ultimo
    /// fotogramma — alza [`Self::al_taglio`]: il giro dopo chiude il brano
    /// invece di leggere ancora.
    fn entro_il_taglio(&mut self, inizio: i128, quanti: usize) -> usize {
        let Some(taglio) = self.taglio else {
            return quanti;
        };
        let restano = i128::from(taglio).saturating_sub(inizio);
        if restano > i128::try_from(quanti).unwrap_or(i128::MAX) {
            return quanti;
        }
        self.al_taglio = true;
        usize::try_from(restano.max(0)).unwrap_or(0)
    }

    /// Mette la catena in forma per i campioni che arrivano.
    ///
    /// Di solito non fa niente: la forma è quella del pacchetto prima. Quando
    /// cambia, la matrice si rifà sempre; il ricampionatore si tiene se
    /// frequenza e numero di canali sono gli stessi — così un Ogg concatenato
    /// con due anelli uguali passa il confine senza nemmeno un fotogramma di
    /// sbavatura — e altrimenti prima si svuota, consegnando fino all'ultimo
    /// quel che apparteneva alla forma vecchia, e poi si rifà.
    fn adegua(&mut self, forma: Forma, fuori: &mut Vec<f32>) -> Result<(), AppError> {
        if forma == self.forma {
            return Ok(());
        }
        forma_accettabile(&forma, self.track_id, None)?;
        let tiene = self.ricampionatore.as_ref().is_some_and(|r| {
            r.frequenza_ingresso() == forma.frequenza && r.canali() == forma.canali
        });
        if tiene {
            self.matrice = Matrice::nuova(&forma.disposizione, forma.canali, self.canali_uscita);
        } else {
            if let Some(mut vecchio) = self.ricampionatore.take() {
                vecchio.coda(&self.matrice, fuori)?;
            }
            let (matrice, ricampionatore) = catena(
                &forma,
                self.canali_uscita,
                self.frequenza_uscita,
                self.track_id,
            )?;
            self.matrice = matrice;
            self.ricampionatore = ricampionatore;
        }
        self.forma = forma;
        Ok(())
    }

    /// Un Ogg concatenato ha cominciato un anello nuovo.
    ///
    /// symphonia ha già riletto le intestazioni e rifatto l'elenco delle
    /// tracce, con identificativi nuovi: si prende la prima traccia audio, si
    /// rifà il codec dai suoi parametri, e il resto — frequenza e canali, che
    /// possono essere cambiati — lo sistema [`Self::adegua`].
    ///
    /// Il cursore non ricomincia: l'anello nuovo comincia dove è arrivato
    /// quello vecchio, contando anche quel che il ricampionatore ha ancora
    /// dentro. Da qui in poi i salti restano dentro l'anello nuovo, perché è
    /// l'unico in cui symphonia sa cercare.
    fn nuovo_anello(&mut self, fuori: &mut Vec<f32>) -> Result<(), AppError> {
        // Dentro un'immagine un anello nuovo non si sa dove metterlo: i tempi
        // del foglio contano dall'inizio del file, i timestamp di symphonia
        // ricominciano da zero a ogni anello, e i salti sanno stare solo
        // nell'anello corrente. Un brano che finisse qui suonerebbe un pezzo
        // sbagliato del disco senza dirlo; meglio dire che questa combinazione
        // non la sappiamo suonare, col codice che esiste apposta.
        if self.segmento.is_some() {
            return Err(ogg_concatenato_in_immagine());
        }
        let senza_audio = || {
            AppError::new(ErrorCode::PlaybackDecodeFailed {
                track_id: Some(self.track_id),
                format: None,
            })
            .with_cause("Ogg concatenato: il nuovo anello non ha tracce audio".to_owned())
        };
        let Some(traccia) = self
            .formato
            .first_track_known_codec(TrackType::Audio)
            .cloned()
        else {
            return Err(senza_audio());
        };
        let Some(parametri) = parametri_audio(&traccia).cloned() else {
            return Err(senza_audio());
        };
        let numero = traccia.id;
        let decodificatore = REGISTRO
            .make_audio_decoder(&parametri, &opzioni_codec())
            .map_err(|err| self.guasto(&err))?;
        // Come all'apertura: frequenza e canali come li dice il codec, e come
        // li dice il contenitore solo se il codec tace.
        let dal_codec = decodificatore.codec_params();
        let frequenza = dal_codec
            .sample_rate
            .or(parametri.sample_rate)
            .unwrap_or(self.forma.frequenza);
        let disposizione = dal_codec
            .channels
            .clone()
            .or_else(|| parametri.channels.clone());

        let in_sospeso = self
            .ricampionatore
            .as_ref()
            .map_or(0, Ricampionatore::in_sospeso);
        // Senza la coda del brano prima, se questo l'aveva ereditata e non è
        // ancora uscita tutta: quei fotogrammi non sono dell'anello vecchio.
        let arrivati = self
            .consegnati
            .saturating_add(fotogrammi_in(fuori, self.canali_uscita))
            .saturating_add(in_sospeso)
            .saturating_sub(self.ereditati);
        self.origine_ms = ms_da_fotogrammi(arrivati, self.frequenza_uscita);
        self.fine_flusso_ms = fine_da_fotogrammi(
            traccia.num_frames,
            parametri.sample_rate.unwrap_or(frequenza),
        )
        .map(|fine| self.origine_ms.saturating_add(fine));

        let ogg = e_ogg(self.formato.as_ref());
        self.decodificatore = decodificatore;
        self.traccia = numero;
        self.orologio = Orologio::da(&traccia, frequenza, ogg);
        self.rincorsa = rincorsa(parametri.codec, frequenza);
        self.salto = None;
        let forma = Forma {
            canali: disposizione
                .as_ref()
                .map_or(self.forma.canali, Channels::count)
                .max(1),
            disposizione: disposizione.unwrap_or_default(),
            frequenza,
        };
        self.adegua(forma, fuori)
    }

    /// Rifà il codec dai parametri della traccia che si sta suonando.
    fn rifai_codec(&mut self) -> Result<(), AppError> {
        let parametri = self
            .formato
            .tracks()
            .iter()
            .find(|t| t.id == self.traccia)
            .and_then(parametri_audio)
            .cloned();
        let Some(parametri) = parametri else {
            return Err(AppError::new(ErrorCode::PlaybackDecodeFailed {
                track_id: Some(self.track_id),
                format: None,
            })
            .with_cause("il codec chiede di ripartire e la traccia non c'è più".to_owned()));
        };
        self.decodificatore = REGISTRO
            .make_audio_decoder(&parametri, &opzioni_codec())
            .map_err(|err| self.guasto(&err))?;
        Ok(())
    }

    /// Aggiunge al conto quel che sta per uscire.
    ///
    /// Tranne la coda ereditata dal brano prima, che è sua: vedi
    /// [`Self::passa_la_coda`].
    fn conta(&mut self, fuori: &[f32]) {
        let usciti = fotogrammi_in(fuori, self.canali_uscita);
        let altrui = usciti.min(self.ereditati);
        self.ereditati -= altrui;
        self.consegnati = self.consegnati.saturating_add(usciti - altrui);
    }
}

/// Un Ogg concatenato dentro un'immagine con il `.cue`: una combinazione che
/// non sappiamo suonare. Vedi [`Decodificatore::nuovo_anello`].
fn ogg_concatenato_in_immagine() -> AppError {
    AppError::new(ErrorCode::PlaybackFormatUnsupported {
        format: Some("ogg".to_owned()),
    })
    .with_cause("Ogg concatenato dentro un'immagine con il .cue".to_owned())
}

/// La matrice dei canali e, se le frequenze differiscono, il ricampionatore.
fn catena(
    forma: &Forma,
    canali_uscita: usize,
    frequenza_uscita: u32,
    track_id: i64,
) -> Result<(Matrice, Option<Ricampionatore>), AppError> {
    let matrice = Matrice::nuova(&forma.disposizione, forma.canali, canali_uscita);
    let ricampionatore = if forma.frequenza == frequenza_uscita {
        None
    } else {
        Some(Ricampionatore::nuovo(
            forma.frequenza,
            frequenza_uscita,
            matrice.ingressi(),
            track_id,
        )?)
    };
    Ok((matrice, ricampionatore))
}

/// Frequenza e canali descrivono della musica? Vedi [`frequenza_accettabile`]
/// e [`CANALI_MAX`].
fn forma_accettabile(
    forma: &Forma,
    track_id: i64,
    formato: Option<String>,
) -> Result<(), AppError> {
    let rifiuta = |causa: String| {
        AppError::new(ErrorCode::PlaybackDecodeFailed {
            track_id: Some(track_id),
            format: formato.clone(),
        })
        .with_cause(causa)
    };
    if !frequenza_accettabile(forma.frequenza) {
        return Err(rifiuta(format!(
            "frequenza dichiarata fuori scala: {} Hz, attesa fra {FREQUENZA_MIN} e {FREQUENZA_MAX}",
            forma.frequenza
        )));
    }
    if forma.canali > CANALI_MAX {
        return Err(rifiuta(format!(
            "canali dichiarati fuori scala: {}, al massimo {CANALI_MAX}",
            forma.canali
        )));
    }
    Ok(())
}

/// La bandiera dell'annullo è alzata? Allora il guasto è un annullamento.
///
/// Una funzione libera per la stessa ragione di [`se_di_rete`]: l'apertura se
/// la chiede quando l'oggetto non esiste ancora.
fn annullato(annullo: Option<&Arc<AtomicBool>>) -> Option<AppError> {
    annullo
        .is_some_and(|bandiera| bandiera.load(Ordering::Relaxed))
        .then(|| {
            AppError::new(ErrorCode::InternalAborted {
                what: Some("lettura del brano".to_owned()),
            })
        })
}

/// Il guasto viene dalla rete? Allora il codice è quello della rete.
///
/// # Perché è una funzione libera e non un metodo
///
/// Perché i tre punti che se la chiedono non hanno tutti un `self` da cui
/// partire: [`Decodificatore::apri`] sta ancora costruendo l'oggetto quando il
/// riconoscimento del contenitore fallisce. Tenerla fuori è ciò che permette
/// alle tre strade — apertura, salto, lettura — di dare la stessa risposta sullo
/// stesso guasto, che è tutto il punto: una share caduta non deve raccontare
/// «formato non supportato» all'apertura, «file danneggiato» al salto e «la rete
/// non risponde» in lettura.
///
/// Quando torna `Some`, la distinzione fra «formato ignoto» e «decodifica
/// fallita» si perde apposta: a rete giù nessuna delle due è vera, e tutte e due
/// sono `Never` ritentabili nel catalogo — cioè manderebbero via il tasto
/// «Riprova» proprio nel caso in cui riprovare è l'unica cosa da fare.
///
/// L'elenco dei numeri di sistema sta in [`rymembra_domain::errors::rete`], dov'è
/// condiviso con chi apre il file invece di leggerlo.
fn se_di_rete(err: &symphonia::core::errors::Error) -> Option<AppError> {
    let symphonia::core::errors::Error::IoError(io) = err else {
        return None;
    };
    if !rymembra_domain::errors::rete::e_di_rete(io) {
        return None;
    }
    Some(AppError::new(ErrorCode::FsNetworkUnavailable { path: None }).with_cause(io.to_string()))
}

/// Il salto è stato rifiutato perché cadeva fuori dal brano?
///
/// # Perché due forme per la stessa cosa
///
/// Perché symphonia non ha un modo solo di dirlo. Quasi sempre è
/// `SeekError(OutOfRange)`: i contenitori che dichiarano `num_frames` — FLAC,
/// WAV, AIFF, MP4 — confrontano, Ogg cerca una pagina e non la trova, e dalla
/// 0.6 anche MP3, che scandisce in avanti, quando finisce i byte lo dice così.
/// In 0.5 invece l'MP3 lo diceva con `IoError(UnexpectedEof)`, e in 0.6 ci
/// arriva ancora chi finisce i byte a metà di un fotogramma — il salto di un
/// MP3 legge l'intestazione e poi salta il corpo, e un file troncato si ferma
/// lì. Sono lo stesso fatto — «il brano finisce prima» — e chiamarli in due
/// modi diversi vorrebbe dire lasciarne fuori uno.
///
/// `UnexpectedEof` e basta, non un `IoError` qualunque: un guasto di rete è
/// anche lui un `IoError`, e chi chiama questa funzione lo ha già escluso un
/// ramo più su. La condizione stretta è la seconda rete sotto quell'ordine.
fn oltre_la_fine(err: &symphonia::core::errors::Error) -> bool {
    match err {
        symphonia::core::errors::Error::SeekError(SeekErrorKind::OutOfRange) => true,
        symphonia::core::errors::Error::IoError(io) => {
            io.kind() == std::io::ErrorKind::UnexpectedEof
        }
        _ => false,
    }
}

/// L'ultimo millisecondo dichiarato, dai fotogrammi del contenitore.
///
/// `None` quando `num_frames` manca: allora non esiste un tetto da imporre,
/// perché non ne ha uno nemmeno chi accetta o rifiuta il salto.
#[expect(
    clippy::integer_division,
    reason = "il resto è la frazione di millisecondo dell'ultimo fotogramma: a \
              44,1 kHz sono quarantaquattro campioni sui ventidue milioni di un \
              disco, e il margine di MARGINE_FINE_MS è mille volte più largo"
)]
fn fine_da_fotogrammi(fotogrammi: Option<u64>, frequenza: u32) -> Option<u64> {
    let frequenza = u64::from(frequenza);
    if frequenza == 0 {
        return None;
    }
    Some(fotogrammi?.saturating_mul(1000) / frequenza)
}

/// Quanti fotogrammi ci sono in un blocco interlacciato.
#[expect(
    clippy::integer_division,
    reason = "un fotogramma è esattamente `canali` campioni: la divisione è esatta \
              per costruzione, e un blocco che non fosse un multiplo dei canali \
              sarebbe già rotto a monte"
)]
fn fotogrammi_in(campioni: &[f32], canali: usize) -> u64 {
    if canali == 0 {
        return 0;
    }
    u64::try_from(campioni.len() / canali).unwrap_or(0)
}

/// I fotogrammi corrispondenti a una durata.
#[expect(
    clippy::integer_division,
    reason = "il resto è la frazione di fotogramma di un millisecondo: a 48 kHz \
              sono quarantotto campioni, cioè un millesimo di secondo di scarto \
              sul punto in cui si atterra dopo un salto"
)]
fn fotogrammi_da_ms(ms: u64, frequenza: u32) -> u64 {
    ms.saturating_mul(u64::from(frequenza)) / 1000
}

/// La durata corrispondente a dei fotogrammi, per difetto.
#[expect(
    clippy::integer_division,
    reason = "il resto è una frazione di millisecondo, sotto qualunque cosa un \
              cursore sappia mostrare"
)]
fn ms_da_fotogrammi(fotogrammi: u64, frequenza: u32) -> u64 {
    let frequenza = u64::from(frequenza).max(1);
    fotogrammi.saturating_mul(1000) / frequenza
}

#[cfg(test)]
mod prove;
