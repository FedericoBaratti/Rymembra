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

use aether_domain::errors::{AppError, ErrorCode};
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
use symphonia::core::formats::probe::Hint;
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
/// sistema. Chi costruisce questa struttura è `aether-app`, che sa da dove
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
    /// dal tag ReplayGain o dalla misura, secondo quel che `aether-app` ha
    /// scelto. `None`: nessuna correzione.
    pub replaygain_db: Option<f32>,
    /// Il picco vero del brano, lineare, se è stato misurato: il tetto a cui
    /// si ferma un guadagno che alza. Vedi [`crate::Livello`].
    pub picco: Option<f32>,
}

impl std::fmt::Debug for Sorgente {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sorgente")
            .field("track_id", &self.track_id)
            .field("estensione", &self.estensione)
            .field("durata_ms", &self.durata_ms)
            .finish_non_exhaustive()
    }
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
        if !aether_domain::errors::rete::e_di_rete(&err) {
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
#[derive(Debug, Clone, Copy)]
struct Orologio {
    /// Secondi per unità di timestamp, come frazione.
    numeratore: u128,
    denominatore: u128,
    /// La frequenza del codec.
    frequenza: u32,
    /// Il timestamp del primo fotogramma del brano. Vedi [`origine`].
    origine: i64,
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
        };
        orologio.origine = origine(traccia, ogg, &orologio);
        orologio
    }

    /// Il fotogramma del brano a cui corrisponde un timestamp.
    ///
    /// Con segno, perché un pacchetto può cominciare **prima** dell'inizio del
    /// brano: i primi fotogrammi di un Opus sono il pre-skip, quelli di un MP3
    /// il ritardo del codificatore.
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
        relativo.saturating_mul(scala) / denominatore
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
    fn ts(&self, fotogramma: u64) -> Timestamp {
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
/// contenitori, e chi guarda conosce il contenitore. Lo stesso per Vorbis, che
/// in un Ogg si chiama «Ogg Vorbis» come lo chiama la colonna del database
/// (vedi `etichetta_codec` in `aether-app`): due nomi diversi per lo stesso file
/// sarebbero un cambio di riga quando il brano parte.
fn nome_del_codec(codec: AudioCodecId, contenitore: FormatId) -> Option<&'static str> {
    let pcm = (CODEC_ID_PCM_S32LE..=CODEC_ID_PCM_MULAW).contains(&codec)
        || (CODEC_ID_ADPCM_G722..=CODEC_ID_ADPCM_IMA_QT).contains(&codec);
    if pcm {
        return Some(match contenitore {
            FORMAT_ID_WAVE => "WAV",
            FORMAT_ID_AIFF => "AIFF",
            FORMAT_ID_CAF => "CAF",
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
    /// quello che symphonia userà per dire di no.
    fine_flusso_ms: Option<u64>,
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
        let formato = symphonia::default::get_probe()
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
        // `motore.rs`.
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
        let fine_flusso_ms = fine_da_fotogrammi(
            traccia.num_frames,
            parametri.sample_rate.unwrap_or(frequenza_sorgente),
        );

        let (matrice, ricampionatore) = catena(&forma, canali_uscita, frequenza, track_id)?;
        let ogg = e_ogg(formato.as_ref());
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

        Ok(Self {
            track_id,
            formato,
            decodificatore,
            traccia: numero_traccia,
            forma,
            canali_uscita,
            frequenza_uscita: frequenza,
            matrice,
            ricampionatore,
            orologio: Orologio::da(&traccia, frequenza_sorgente, ogg),
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
        })
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

    /// Si riposiziona, al fotogramma. La coda interna del ricampionatore va
    /// buttata.
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
    /// `motore.rs`, e invertire i rami la fa cadere.
    pub fn cerca(&mut self, ms: u64) -> Result<(), AppError> {
        let bersaglio = self.bersaglio(ms);
        // Dentro l'anello corrente: su un Ogg concatenato symphonia sa saltare
        // solo lì, e i suoi timestamp ricominciano da zero a ogni anello.
        let dentro_ms = bersaglio.saturating_sub(self.origine_ms);
        let punto = fotogrammi_da_ms(dentro_ms, self.orologio.frequenza);
        let partenza = punto.saturating_sub(self.rincorsa);
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
                ts: self.orologio.ts(partenza),
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
    /// L'elenco dei numeri sta in [`aether_domain::errors::rete`], dov'è
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
    fn consuma(
        &mut self,
        inizio: i128,
        quanti: usize,
        forma: Option<Forma>,
        fuori: &mut Vec<f32>,
    ) -> Result<(), AppError> {
        let scarto = self.da_scartare(inizio, quanti);
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
        let utili = campioni
            .get(scarto.saturating_mul(self.forma.canali)..)
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
/// L'elenco dei numeri di sistema sta in [`aether_domain::errors::rete`], dov'è
/// condiviso con chi apre il file invece di leggerlo.
fn se_di_rete(err: &symphonia::core::errors::Error) -> Option<AppError> {
    let symphonia::core::errors::Error::IoError(io) = err else {
        return None;
    };
    if !aether_domain::errors::rete::e_di_rete(io) {
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
mod prove {
    use super::*;

    #[test]
    fn i_fotogrammi_di_una_durata() {
        assert_eq!(fotogrammi_da_ms(1_000, 48_000), 48_000);
        assert_eq!(fotogrammi_da_ms(0, 44_100), 0);
        assert_eq!(ms_da_fotogrammi(48_000, 48_000), 1_000);
        assert_eq!(
            ms_da_fotogrammi(1, 0),
            1_000,
            "una frequenza a zero non divide per zero"
        );
    }

    #[test]
    fn la_fine_del_flusso_dai_fotogrammi_dichiarati() {
        // Mezzo secondo a 44,1 kHz: il campione FLAC di `motore.rs`.
        assert_eq!(fine_da_fotogrammi(Some(22_050), 44_100), Some(500));
        assert_eq!(fine_da_fotogrammi(Some(48_000), 48_000), Some(1_000));
        // Senza `num_frames` non c'è tetto: è il caso di Matroska e di un MP3
        // senza Xing su un flusso, e il salto deve partire com'è per finire
        // nella rete di sicurezza di `cerca`.
        assert_eq!(fine_da_fotogrammi(None, 44_100), None);
        // Una frequenza a zero non deve dividere per zero.
        assert_eq!(fine_da_fotogrammi(Some(22_050), 0), None);
        // Un `num_frames` assurdo satura invece di traboccare: la moltiplicazione
        // per mille si ferma a `u64::MAX`, e la divisione la riporta giù. Il
        // valore non vuol dire niente, ma non è un panico — ed è quel che serve
        // a un contenitore malformato che arriva dal disco di chiunque.
        assert_eq!(
            fine_da_fotogrammi(Some(u64::MAX), 1_000),
            Some(18_446_744_073_709_551)
        );
    }

    #[test]
    fn un_guasto_di_rete_non_e_una_fine_di_brano() {
        // L'ordine dei rami di `cerca` in miniatura. `ERROR_NETNAME_DELETED`,
        // il 64 di Windows, arriva come `IoError` esattamente come la fine del
        // flusso: se questa funzione lo prendesse per una fine, una share morta
        // durante un salto passerebbe in silenzio e nessuno offrirebbe
        // «Riprova».
        let rete = symphonia::core::errors::Error::IoError(std::io::Error::from_raw_os_error(64));
        assert!(!oltre_la_fine(&rete));

        // E i due modi in cui symphonia dice davvero «oltre la fine».
        assert!(oltre_la_fine(&symphonia::core::errors::Error::SeekError(
            SeekErrorKind::OutOfRange
        )));
        assert!(oltre_la_fine(&symphonia::core::errors::Error::IoError(
            std::io::Error::from(std::io::ErrorKind::UnexpectedEof)
        )));
    }

    /// Una traccia come la descrive un demultiplatore 0.6: codec, frequenza,
    /// scala dei tempi, primo timestamp e ritardo stanno in posti diversi.
    fn traccia(
        codec: AudioCodecId,
        frequenza: u32,
        scala: u32,
        inizio: i64,
        ritardo: Option<u32>,
    ) -> Track {
        let mut parametri = AudioCodecParameters::new();
        parametri.for_codec(codec).with_sample_rate(frequenza);
        let mut traccia = Track::new(0);
        traccia
            .with_codec_params(CodecParameters::Audio(parametri))
            .with_time_base(
                symphonia::core::units::TimeBase::try_new(1, scala).expect("scala non nulla"),
            )
            .with_start_ts(Timestamp::new(inizio));
        if let Some(ritardo) = ritardo {
            traccia.with_delay(ritardo);
        }
        traccia
    }

    #[test]
    fn l_orologio_conta_il_pre_skip_di_opus() {
        // Ogg Opus: la posizione granulare conta anche il pre-skip, e il primo
        // pacchetto comincia 312 fotogrammi prima del brano.
        let orologio = Orologio::da(
            &traccia(CODEC_ID_OPUS, 48_000, 48_000, 0, Some(312)),
            48_000,
            true,
        );
        assert_eq!(orologio.fotogramma(Timestamp::new(0)), -312);
        assert_eq!(orologio.fotogramma(Timestamp::new(72_312)), 72_000);
        assert_eq!(orologio.ts(72_000), Timestamp::new(72_312));

        // In MP3 il ritardo sta nei timestamp negativi, e il fotogramma zero è
        // il timestamp zero anche se la traccia lo dichiara.
        let orologio = Orologio::da(
            &traccia(CODEC_ID_MP3, 44_100, 44_100, 0, Some(1_105)),
            44_100,
            false,
        );
        assert_eq!(orologio.fotogramma(Timestamp::new(44_100)), 44_100);
        assert_eq!(orologio.fotogramma(Timestamp::new(-1_105)), -1_105);
        assert_eq!(orologio.ts(44_100), Timestamp::new(44_100));

        // Ogg Vorbis: la traccia comincia al ritardo con il segno meno, e il
        // ritardo la riporta a zero.
        let orologio = Orologio::da(
            &traccia(CODEC_ID_VORBIS, 44_100, 44_100, -1_024, Some(1_024)),
            44_100,
            true,
        );
        assert_eq!(orologio.fotogramma(Timestamp::new(0)), 0);
        assert_eq!(orologio.ts(0), Timestamp::new(0));

        // Una scala dei tempi che non è la frequenza, come in certi MP4.
        let orologio = Orologio::da(
            &traccia(CODEC_ID_AAC, 44_100, 90_000, 0, None),
            44_100,
            false,
        );
        assert_eq!(orologio.fotogramma(Timestamp::new(90_000)), 44_100);
        assert_eq!(orologio.ts(44_100), Timestamp::new(90_000));
    }

    /// Byte in memoria che si comportano come un file.
    struct Byte(std::io::Cursor<Vec<u8>>);

    impl std::io::Read for Byte {
        fn read(&mut self, dove: &mut [u8]) -> std::io::Result<usize> {
            std::io::Read::read(&mut self.0, dove)
        }
    }

    impl std::io::Seek for Byte {
        fn seek(&mut self, da: std::io::SeekFrom) -> std::io::Result<u64> {
            std::io::Seek::seek(&mut self.0, da)
        }
    }

    impl Flusso for Byte {
        fn lunghezza(&self) -> Option<u64> {
            u64::try_from(self.0.get_ref().len()).ok()
        }
    }

    /// Un WAV a 16 bit con i campioni dati, interlacciati su `canali`.
    fn wav(frequenza: u32, canali: u16, campioni: &[i16]) -> Vec<u8> {
        let dati = u32::try_from(campioni.len() * 2).unwrap_or(0);
        let allineamento = canali * 2;
        let mut byte = Vec::with_capacity(44 + campioni.len() * 2);
        byte.extend_from_slice(b"RIFF");
        byte.extend_from_slice(&(36 + dati).to_le_bytes());
        byte.extend_from_slice(b"WAVEfmt ");
        byte.extend_from_slice(&16u32.to_le_bytes());
        byte.extend_from_slice(&1u16.to_le_bytes()); // PCM
        byte.extend_from_slice(&canali.to_le_bytes());
        byte.extend_from_slice(&frequenza.to_le_bytes());
        byte.extend_from_slice(
            &frequenza
                .saturating_mul(u32::from(allineamento))
                .to_le_bytes(),
        );
        byte.extend_from_slice(&allineamento.to_le_bytes());
        byte.extend_from_slice(&16u16.to_le_bytes()); // bit per campione
        byte.extend_from_slice(b"data");
        byte.extend_from_slice(&dati.to_le_bytes());
        for campione in campioni {
            byte.extend_from_slice(&campione.to_le_bytes());
        }
        byte
    }

    /// Un WAV mono a 16 bit che dichiara la frequenza data.
    ///
    /// I campioni sono silenzio: quel che si prova è l'intestazione.
    fn wav_a(frequenza: u32, fotogrammi: usize) -> Vec<u8> {
        wav(frequenza, 1, &vec![0; fotogrammi])
    }

    fn sorgente(byte: Vec<u8>, estensione: &str) -> Sorgente {
        Sorgente {
            track_id: 1,
            media: Box::new(Byte(std::io::Cursor::new(byte))),
            estensione: Some(estensione.to_owned()),
            durata_ms: 0,
            replaygain_db: None,
            picco: None,
        }
    }

    fn sorgente_wav(frequenza: u32) -> Sorgente {
        sorgente(wav_a(frequenza, 1_000), "wav")
    }

    /// Una rampa: il campione `n` vale `n` (modulo 32 768), così il valore
    /// dice da dove viene.
    fn rampa(quanti: usize) -> Vec<i16> {
        (0..quanti)
            .map(|n| i16::try_from(n % 32_768).expect("la rampa sta in un i16"))
            .collect()
    }

    /// Tutto quel che un decodificatore consegna, fino alla fine.
    fn tutto(d: &mut Decodificatore) -> Vec<f32> {
        let mut fuori = Vec::new();
        let mut blocco = Vec::new();
        while d.prossimo(&mut blocco).expect("decodifica") {
            fuori.extend_from_slice(&blocco);
        }
        fuori
    }

    fn attesi(quanti: u64, da: u64, a: u64) -> usize {
        usize::try_from((quanti * a).div_ceil(da)).expect("sta in un usize")
    }

    #[test]
    fn un_wav_che_dichiara_una_frequenza_impossibile_non_si_apre() {
        // Il giro intero, dall'intestazione al rifiuto: un contenitore che si
        // riconosce benissimo e che dichiara tre megahertz. Prima il numero
        // passava di qui senza che nessuno lo guardasse e arrivava al
        // ricampionatore, che ci dimensionava sopra i propri buffer.
        let Err(err) = Decodificatore::apri(sorgente_wav(3_000_000), 48_000, 1) else {
            panic!("una frequenza di tre megahertz non descrive della musica");
        };
        assert_eq!(
            err.code().kind(),
            aether_domain::errors::ErrorCodeKind::PlaybackDecodeFailed,
            "il file c'è e si legge: non è un guasto di rete né un formato ignoto"
        );
        assert!(
            err.cause().unwrap_or_default().contains("fuori scala"),
            "il rifiuto non è il nostro: causa {:?}",
            err.cause()
        );

        // Il contrappeso: la stessa intestazione con un numero vero si apre.
        assert!(Decodificatore::apri(sorgente_wav(44_100), 48_000, 1).is_ok());
    }

    #[test]
    fn la_scala_delle_frequenze_accettabili() {
        // Gli estremi si accettano: 8000 è il telefono, 384000 il DXD, e
        // rifiutare proprio il valore di confine sarebbe un file legittimo che
        // non si apre.
        assert!(frequenza_accettabile(FREQUENZA_MIN));
        assert!(frequenza_accettabile(FREQUENZA_MAX));
        // Le due che questa libreria incontra davvero.
        assert!(frequenza_accettabile(44_100));
        assert!(frequenza_accettabile(48_000));
        // E quel che esce da un'intestazione letta male.
        assert!(!frequenza_accettabile(0));
        assert!(!frequenza_accettabile(FREQUENZA_MIN - 1));
        assert!(!frequenza_accettabile(3_000_000));
        assert!(!frequenza_accettabile(u32::MAX));
    }

    #[test]
    fn un_brano_ricampionato_esce_lungo_esatto() {
        // Un secondo a 44,1 kHz è un secondo a 48 kHz: 48000 fotogrammi, non
        // uno di più né uno di meno. Prima ne mancava una ventina di
        // millisecondi in coda, ed erano sei millisecondi e mezzo di silenzio
        // in testa: la tolleranza delle prove di allora era di mille.
        for quanti in [44_100_usize, 12_345, 1_000] {
            let byte = wav(44_100, 1, &vec![8_000; quanti]);
            let mut d = Decodificatore::apri(sorgente(byte, "wav"), 48_000, 1).expect("si apre");
            let campioni = tutto(&mut d);
            assert_eq!(
                campioni.len(),
                attesi(u64::try_from(quanti).expect("sta"), 44_100, 48_000),
                "{quanti} fotogrammi a 44,1 kHz"
            );
            assert_eq!(d.consegnati(), u64::try_from(campioni.len()).expect("sta"));
            // E nessun silenzio in testa: il livello c'è dal primo fotogramma
            // utile, non dopo i 320 del ritardo del filtro. I primissimi
            // risentono del bordo; al ventesimo il livello è già quello.
            let livello = 8_000.0 / 32_768.0;
            assert!(
                (campioni[20] - livello).abs() < 0.02,
                "al ventesimo fotogramma {} invece di {livello}",
                campioni[20]
            );
        }
    }

    #[test]
    fn un_salto_su_un_wav_atterra_al_fotogramma() {
        // A 48 kHz non c'è ricampionatore, e la rampa dice di sé dove si è: il
        // WAV di symphonia salta al confine del proprio pacchetto simulato, e
        // prima si ricominciava da lì con il cursore sul millisecondo chiesto.
        let byte = wav(48_000, 1, &rampa(24_000));
        let mut d = Decodificatore::apri(sorgente(byte, "wav"), 48_000, 1).expect("si apre");
        d.cerca(250).expect("il salto riesce");
        assert_eq!(d.consegnati(), 12_000);
        let resto = tutto(&mut d);
        assert_eq!(resto.len(), 12_000, "restano esattamente 250 ms");
        assert!(
            (resto[0] - 12_000.0 / 32_768.0).abs() < 1e-6,
            "il primo campione dopo il salto è {} invece del dodicimillesimo",
            resto[0] * 32_768.0
        );
        assert!((resto[1] - 12_001.0 / 32_768.0).abs() < 1e-6);
    }

    #[test]
    fn un_salto_ricampionato_lascia_la_coda_esatta() {
        // A 44,1 kHz con uscita a 48: dopo il salto a 250 ms restano 11 025
        // fotogrammi di sorgente, cioè 12 000 d'uscita tondi. Né il ritardo
        // del filtro in testa né il pacchetto intero di prima.
        let byte = wav(44_100, 1, &vec![4_000; 22_050]);
        let mut d = Decodificatore::apri(sorgente(byte, "wav"), 48_000, 1).expect("si apre");
        d.cerca(250).expect("il salto riesce");
        assert_eq!(d.consegnati(), 12_000);
        let resto = tutto(&mut d);
        assert_eq!(resto.len(), 12_000);
        assert_eq!(d.consegnati(), 24_000);
    }

    /// Un coseno a 1 kHz, a 16 bit e mezza ampiezza, dal fotogramma `da` per
    /// `quanti` fotogrammi: due pezzi consecutivi sono lo stesso segnale.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "mezza ampiezza di un i16 arrotondata sta in un i16"
    )]
    fn coseno(frequenza: u32, da: usize, quanti: usize) -> Vec<i16> {
        (da..da + quanti)
            .map(|n| {
                let t = n as f64 / f64::from(frequenza);
                ((2.0 * std::f64::consts::PI * 1_000.0 * t).cos() * 16_000.0).round() as i16
            })
            .collect()
    }

    /// Un WAV mono a 44,1 kHz col coseno dato, aperto per un'uscita a 48.
    fn brano_a_44(da: usize, quanti: usize) -> Decodificatore {
        let byte = wav(44_100, 1, &coseno(44_100, da, quanti));
        Decodificatore::apri(sorgente(byte, "wav"), 48_000, 1).expect("si apre")
    }

    /// Tutto un brano chiamando [`Decodificatore::prossimo_lasciando_la_coda`],
    /// come fa il motore quando il seguito è in canna.
    fn tutto_lasciando_la_coda(d: &mut Decodificatore) -> Vec<f32> {
        let mut fuori = Vec::new();
        let mut blocco = Vec::new();
        while d
            .prossimo_lasciando_la_coda(&mut blocco)
            .expect("decodifica")
        {
            fuori.extend_from_slice(&blocco);
        }
        fuori
    }

    fn scarto(uno: &[f32], altro: &[f32]) -> f32 {
        uno.iter()
            .zip(altro)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0_f32, f32::max)
    }

    // Il disco spezzato su un confine che non cade sulla griglia d'uscita:
    // 22 051 fotogrammi a 44,1 kHz sono 24 001,09 a 48.
    const PRIMO: usize = 22_051;
    const SECONDO: usize = 13_230;

    #[test]
    fn un_ricampionatore_solo_attraversa_la_giuntura() {
        // Lo stesso disco in un file solo e in due brani: il secondo prende il
        // ricampionatore del primo, e quel che esce deve essere il disco
        // intero, campione per campione.
        let atteso = tutto(&mut brano_a_44(0, PRIMO + SECONDO));

        let mut primo = brano_a_44(0, PRIMO);
        let mut secondo = brano_a_44(PRIMO, SECONDO);
        assert!(primo.puo_passare_la_coda(&secondo));
        let mut fuori = tutto_lasciando_la_coda(&mut primo);
        assert!(primo.coda_sospesa(), "la coda deve restare nel filtro");
        let rinvio = primo.passa_la_coda(&mut secondo).expect("la coda passa");
        assert!(
            rinvio > 0,
            "a fine brano il filtro ha sempre qualcosa dentro"
        );
        assert_eq!(secondo.ereditati(), rinvio);

        // Il conto del primo è quello di un brano intero, coda compresa, anche
        // se l'ultima parte la consegna l'altro.
        let del_primo = attesi(u64::try_from(PRIMO).expect("sta"), 44_100, 48_000);
        let rinvio = usize::try_from(rinvio).expect("sta");
        assert_eq!(fuori.len() + rinvio, del_primo);
        assert_eq!(primo.consegnati(), u64::try_from(del_primo).expect("sta"));

        fuori.extend(tutto(&mut secondo));
        assert_eq!(
            fuori.len(),
            atteso.len(),
            "né un fotogramma in più né in meno"
        );
        assert_eq!(
            secondo.consegnati(),
            u64::try_from(atteso.len() - del_primo).expect("sta"),
            "la coda del primo non è fra i fotogrammi del secondo"
        );
        assert_eq!(secondo.ereditati(), 0);
        let giuntura = scarto(&fuori, &atteso);
        assert!(giuntura < 1e-6, "scarto dal disco intero: {giuntura}");

        // E la prova vede davvero la giuntura: con due ricampionatori, come
        // prima, il secondo brano esce spostato di una frazione di campione e
        // se ne discosta ben oltre.
        let mut vecchio = tutto(&mut brano_a_44(0, PRIMO));
        vecchio.extend(tutto(&mut brano_a_44(PRIMO, SECONDO)));
        assert_eq!(vecchio.len(), atteso.len());
        let prima = scarto(&vecchio, &atteso);
        assert!(prima > 1e-2, "due ricampionatori davano {prima}");
    }

    #[test]
    fn una_coda_che_non_passa_esce_lo_stesso() {
        // Il seguito è già a 48 kHz: non ricampiona, e non può prendere niente.
        // La coda resta dentro finché qualcuno non la chiede, poi esce come a
        // ogni fine brano.
        let mut primo = brano_a_44(0, PRIMO);
        let byte = wav(48_000, 1, &coseno(48_000, 0, 4_800));
        let mut secondo = Decodificatore::apri(sorgente(byte, "wav"), 48_000, 1).expect("si apre");
        assert!(!primo.puo_passare_la_coda(&secondo));

        let mut fuori = tutto_lasciando_la_coda(&mut primo);
        assert!(primo.coda_sospesa());
        assert_eq!(primo.passa_la_coda(&mut secondo), None);
        let mut blocco = Vec::new();
        assert!(primo.consegna_la_coda(&mut blocco).expect("coda"));
        fuori.extend_from_slice(&blocco);
        assert!(!primo.coda_sospesa());
        assert!(!primo.prossimo(&mut blocco).expect("fine"));
        assert_eq!(
            fuori.len(),
            attesi(u64::try_from(PRIMO).expect("sta"), 44_100, 48_000)
        );
        assert_eq!(secondo.ereditati(), 0);
    }

    #[test]
    fn un_salto_butta_la_coda_ereditata() {
        // Dopo il passaggio, un salto sul secondo — o il suo ritorno
        // all'inizio, che è quel che fa il motore quando rimette in canna un
        // brano — non deve far uscire niente del primo.
        let mut primo = brano_a_44(0, PRIMO);
        let mut secondo = brano_a_44(PRIMO, SECONDO);
        let _ = tutto_lasciando_la_coda(&mut primo);
        assert!(primo.passa_la_coda(&mut secondo).is_some());

        secondo.cerca(0).expect("il salto riesce");
        assert_eq!(secondo.ereditati(), 0);
        let da_capo = tutto(&mut secondo);
        let solo = tutto(&mut brano_a_44(PRIMO, SECONDO));
        assert_eq!(da_capo.len(), solo.len());
        let differenza = scarto(&da_capo, &solo);
        assert!(differenza < 1e-6, "è rimasto del primo: {differenza}");

        // E il primo, che in cambio ha avuto il filtro intatto del secondo,
        // riprende da un salto come un brano qualunque.
        primo.cerca(250).expect("il salto riesce");
        assert_eq!(primo.consegnati(), 12_000);
        let resto = tutto(&mut primo);
        assert_eq!(
            resto.len(),
            attesi(u64::try_from(PRIMO).expect("sta"), 44_100, 48_000) - 12_000
        );
    }

    #[test]
    fn un_salto_su_un_flusso_di_rete_e_esatto_lo_stesso() {
        // Il salto «a grandi linee» dei flussi: lo scarto fino al bersaglio
        // rimette la precisione.
        struct DallaRete(Byte);
        impl std::io::Read for DallaRete {
            fn read(&mut self, dove: &mut [u8]) -> std::io::Result<usize> {
                self.0.read(dove)
            }
        }
        impl std::io::Seek for DallaRete {
            fn seek(&mut self, da: std::io::SeekFrom) -> std::io::Result<u64> {
                self.0.seek(da)
            }
        }
        impl Flusso for DallaRete {
            fn lunghezza(&self) -> Option<u64> {
                self.0.lunghezza()
            }
            fn remoto(&self) -> bool {
                true
            }
        }
        let byte = wav(48_000, 1, &rampa(24_000));
        let s = Sorgente {
            track_id: 1,
            media: Box::new(DallaRete(Byte(std::io::Cursor::new(byte)))),
            estensione: Some("wav".to_owned()),
            durata_ms: 0,
            replaygain_db: None,
            picco: None,
        };
        let mut d = Decodificatore::apri(s, 48_000, 1).expect("si apre");
        assert!(d.remoto);
        d.cerca(100).expect("il salto riesce");
        let resto = tutto(&mut d);
        assert_eq!(resto.len(), 24_000 - 4_800);
        assert!((resto[0] - 4_800.0 / 32_768.0).abs() < 1e-6);
    }

    /// Il FLAC di prova di `motore.rs`: mezzo secondo di livello costante a
    /// 44,1 kHz, mono.
    const FLAC_COSTANTE: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/campioni/costante.flac"
    ));

    /// Il tono Opus di prova di `motore.rs`: tre secondi esatti a 48 kHz,
    /// 144 000 fotogrammi tolti pre-skip e coda.
    const OPUS_TONO: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/campioni/tono.opus"
    ));

    #[test]
    fn un_salto_su_un_flac_non_si_ferma_al_blocco() {
        // Il FLAC salta al blocco di 4096 fotogrammi: prima si ricominciava
        // da lì, fino a 93 ms prima del punto chiesto. Ora restano
        // esattamente i 250 ms che mancano: 11 025 fotogrammi di sorgente,
        // 12 000 d'uscita.
        let mut d = Decodificatore::apri(sorgente(FLAC_COSTANTE.to_vec(), "flac"), 48_000, 1)
            .expect("il FLAC si apre");
        assert_eq!(tutto(&mut d).len(), 24_000, "mezzo secondo a 48 kHz");

        let mut d = Decodificatore::apri(sorgente(FLAC_COSTANTE.to_vec(), "flac"), 48_000, 1)
            .expect("il FLAC si apre");
        d.cerca(250).expect("il salto riesce");
        assert_eq!(tutto(&mut d).len(), 12_000);
    }

    #[test]
    fn il_formato_suonato_di_un_flac_ricampionato() {
        // Il FLAC di prova è 44,1 kHz, 16 bit, mono, 154 byte per mezzo
        // secondo: su un'uscita a 48 kHz il motore deve dire tutte e cinque
        // le cose, e la sesta — il ricampionamento — da come le due frequenze
        // differiscono.
        let d = Decodificatore::apri(sorgente(FLAC_COSTANTE.to_vec(), "flac"), 48_000, 2)
            .expect("il FLAC si apre");
        assert_eq!(
            d.formato_suonato(),
            FormatoSuonato {
                codec: Some("FLAC"),
                frequenza: 44_100,
                bit: Some(16),
                canali: 1,
                frequenza_uscita: 48_000,
                kbps: Some(2),
                remoto: false,
            }
        );
    }

    #[test]
    fn il_formato_suonato_di_un_wav_e_di_un_opus() {
        // Il PCM prende il nome dal contenitore; un codec con perdita non ha
        // bit da dire.
        let byte = wav(48_000, 2, &vec![1_000; 48_000]);
        let d = Decodificatore::apri(sorgente(byte, "wav"), 48_000, 2).expect("il WAV si apre");
        let wav = d.formato_suonato();
        assert_eq!((wav.codec, wav.bit, wav.canali), (Some("WAV"), Some(16), 2));
        // 1536 kbit/s di campioni più i 44 byte dell'intestazione.
        assert!(matches!(wav.kbps, Some(1536 | 1537)), "{:?}", wav.kbps);

        let d = Decodificatore::apri(sorgente(OPUS_TONO.to_vec(), "opus"), 48_000, 2)
            .expect("l'Opus si apre");
        let opus = d.formato_suonato();
        assert_eq!((opus.codec, opus.bit), (Some("Opus"), None));
        assert_eq!((opus.frequenza, opus.frequenza_uscita), (48_000, 48_000));
    }

    #[test]
    fn i_nomi_dei_codec() {
        use symphonia::core::codecs::audio::well_known::{CODEC_ID_AC3, CODEC_ID_PCM_S16LE};
        use symphonia::core::formats::well_known::FORMAT_ID_MKV;
        assert_eq!(
            nome_del_codec(CODEC_ID_VORBIS, FORMAT_ID_OGG),
            Some("Ogg Vorbis")
        );
        assert_eq!(
            nome_del_codec(CODEC_ID_VORBIS, FORMAT_ID_MKV),
            Some("Vorbis")
        );
        assert_eq!(
            nome_del_codec(CODEC_ID_PCM_S16LE, FORMAT_ID_AIFF),
            Some("AIFF")
        );
        assert_eq!(
            nome_del_codec(CODEC_ID_PCM_S16LE, FORMAT_ID_MKV),
            Some("PCM")
        );
        assert_eq!(nome_del_codec(CODEC_ID_ALAC, FORMAT_ID_MKV), Some("ALAC"));
        // Un codec che la tabella non nomina: chi mostra ripiega sul database.
        assert_eq!(nome_del_codec(CODEC_ID_AC3, FORMAT_ID_MKV), None);
        assert!(senza_perdita(CODEC_ID_PCM_S16LE));
        assert!(!senza_perdita(CODEC_ID_MP3));
    }

    #[test]
    fn il_bitrate_medio_vuole_byte_e_durata() {
        // 192 kbit/s: 24 000 byte al secondo.
        assert_eq!(kbps_medi(Some(240_000), Some(441_000), 44_100), Some(192));
        assert_eq!(kbps_medi(None, Some(441_000), 44_100), None);
        assert_eq!(kbps_medi(Some(240_000), None, 44_100), None);
        assert_eq!(kbps_medi(Some(240_000), Some(0), 44_100), None);
        assert_eq!(kbps_medi(Some(0), Some(441_000), 44_100), None);
        // Un conto assurdo non diventa un numero assurdo.
        assert_eq!(kbps_medi(Some(u64::MAX), Some(1), 384_000), None);
    }

    #[test]
    fn un_salto_su_un_opus_atterra_al_fotogramma() {
        // Opus è il caso più storto: la posizione granulare conta anche il
        // pre-skip, e il codec ha memoria. A un secondo e mezzo dall'inizio
        // di tre secondi esatti devono restarne un secondo e mezzo esatto.
        let mut d = Decodificatore::apri(sorgente(OPUS_TONO.to_vec(), "opus"), 48_000, 1)
            .expect("l'Opus si apre");
        d.cerca(1_500).expect("il salto riesce");
        assert_eq!(d.consegnati(), 72_000);
        assert_eq!(tutto(&mut d).len(), 72_000);
    }

    #[test]
    fn tornare_all_inizio_di_un_opus_da_lo_stesso_brano() {
        // `cerca(0)` è quel che il motore fa a un brano preparato che deve
        // tornare in canna: il pre-skip, che all'apertura toglie il codec, dopo
        // un salto lo toglie lo scarto — una volta sola, né zero né due.
        let mut d = Decodificatore::apri(sorgente(OPUS_TONO.to_vec(), "opus"), 48_000, 1)
            .expect("l'Opus si apre");
        let mut blocco = Vec::new();
        for _ in 0..10 {
            assert!(d.prossimo(&mut blocco).expect("decodifica"));
        }
        d.cerca(0).expect("il salto riesce");
        assert_eq!(d.consegnati(), 0);
        assert_eq!(tutto(&mut d).len(), 144_000);
    }

    #[test]
    fn un_ogg_concatenato_suona_fino_in_fondo() {
        // Due flussi Ogg completi uno dopo l'altro sono un Ogg concatenato: è
        // la forma delle registrazioni di trasmissioni e di certi dischi
        // interi in un file solo. Al confine symphonia dice `ResetRequired`, e
        // prima quello era un guasto: il brano si fermava a metà.
        let mut byte = OPUS_TONO.to_vec();
        byte.extend_from_slice(OPUS_TONO);
        let mut d = Decodificatore::apri(sorgente(byte, "opus"), 48_000, 1)
            .expect("l'Ogg concatenato si apre");
        let campioni = tutto(&mut d);
        // Ciascun anello toglie il proprio pre-skip e la propria coda.
        assert_eq!(campioni.len(), 2 * 144_000);
        // E dopo il confine c'è ancora il tono, non il silenzio.
        let secondo = &campioni[144_000 + 5_000..2 * 144_000 - 5_000];
        let efficace = (secondo.iter().map(|c| c * c).sum::<f32>() / secondo.len() as f32).sqrt();
        assert!(
            efficace > 0.2,
            "valore efficace dopo il confine: {efficace}"
        );
    }

    /// Un CAF — il Core Audio Format di Apple — con PCM a 16 bit little-endian,
    /// mono, alla frequenza data. Il minimo che il formato ammette: la firma,
    /// la descrizione del flusso e i dati.
    fn caf(frequenza: u32, campioni: &[i16]) -> Vec<u8> {
        let mut byte = Vec::new();
        byte.extend_from_slice(b"caff");
        byte.extend_from_slice(&1u16.to_be_bytes()); // versione
        byte.extend_from_slice(&0u16.to_be_bytes()); // bandiere
        // `desc`: trentadue byte, tutti big-endian.
        byte.extend_from_slice(b"desc");
        byte.extend_from_slice(&32i64.to_be_bytes());
        byte.extend_from_slice(&f64::from(frequenza).to_be_bytes());
        byte.extend_from_slice(b"lpcm");
        byte.extend_from_slice(&2u32.to_be_bytes()); // intero, little-endian
        byte.extend_from_slice(&2u32.to_be_bytes()); // byte per pacchetto
        byte.extend_from_slice(&1u32.to_be_bytes()); // fotogrammi per pacchetto
        byte.extend_from_slice(&1u32.to_be_bytes()); // canali
        byte.extend_from_slice(&16u32.to_be_bytes()); // bit per canale
        // `data`: il conto delle modifiche, e poi i campioni.
        let dati = i64::try_from(campioni.len() * 2 + 4).expect("sta in un i64");
        byte.extend_from_slice(b"data");
        byte.extend_from_slice(&dati.to_be_bytes());
        byte.extend_from_slice(&0u32.to_be_bytes());
        for campione in campioni {
            byte.extend_from_slice(&campione.to_le_bytes());
        }
        byte
    }

    /// Un elemento EBML: l'identificativo così com'è, la lunghezza su otto byte
    /// — la forma più lunga che la norma ammetta, così che la lunghezza di un
    /// elemento non dipenda dai numeri che contiene — e il contenuto.
    fn ebml(id: &[u8], contenuto: &[u8]) -> Vec<u8> {
        let mut byte = id.to_vec();
        byte.push(0x01);
        let lunghezza = u64::try_from(contenuto.len())
            .expect("sta in un u64")
            .to_be_bytes();
        byte.extend_from_slice(lunghezza.get(1..).expect("otto byte"));
        byte.extend_from_slice(contenuto);
        byte
    }

    /// Un intero EBML, su otto byte per la stessa ragione.
    fn ebml_intero(id: &[u8], valore: u64) -> Vec<u8> {
        ebml(id, &valore.to_be_bytes())
    }

    /// Un Matroska audio (`.mka`) con una traccia sola, numero 1.
    ///
    /// `codec` sono gli elementi della traccia che dicono il codec — `CodecID`,
    /// `Audio` e quel che serve — e `blocchi` i pacchetti col loro timestamp in
    /// millisecondi, la scala di quasi tutti i Matroska. Un cluster ogni cento
    /// millisecondi, e l'indice (`Cues`) prima dei cluster: senza, il
    /// demultiplatore di symphonia sa cercare solo in avanti.
    fn matroska(codec: &[u8], blocchi: &[(u64, Vec<u8>)]) -> Vec<u8> {
        const CLUSTER_MS: u64 = 100;

        let testa = ebml(
            &[0x1A, 0x45, 0xDF, 0xA3],
            &[
                ebml_intero(&[0x42, 0x86], 1),
                ebml_intero(&[0x42, 0xF7], 1),
                ebml_intero(&[0x42, 0xF2], 4),
                ebml_intero(&[0x42, 0xF3], 8),
                ebml(&[0x42, 0x82], b"matroska"),
                ebml_intero(&[0x42, 0x87], 4),
                ebml_intero(&[0x42, 0x85], 2),
            ]
            .concat(),
        );
        // Un millisecondo per unità di timestamp, e i nomi di chi l'ha
        // scritto, che la norma vuole e symphonia pretende.
        let info = ebml(
            &[0x15, 0x49, 0xA9, 0x66],
            &[
                ebml_intero(&[0x2A, 0xD7, 0xB1], 1_000_000),
                ebml(&[0x4D, 0x80], b"aether"),
                ebml(&[0x57, 0x41], b"aether"),
            ]
            .concat(),
        );
        let traccia = ebml(
            &[0xAE],
            &[
                ebml_intero(&[0xD7], 1),
                ebml_intero(&[0x73, 0xC5], 1),
                ebml_intero(&[0x83], 2),
                codec.to_vec(),
            ]
            .concat(),
        );
        let tracce = ebml(&[0x16, 0x54, 0xAE, 0x6B], &traccia);

        let mut cluster: Vec<(u64, Vec<u8>)> = Vec::new();
        for (ms, dati) in blocchi {
            let inizio = ms - ms % CLUSTER_MS;
            if cluster.last().is_none_or(|(ts, _)| *ts != inizio) {
                cluster.push((inizio, ebml_intero(&[0xE7], inizio)));
            }
            let mut semplice = vec![0x81];
            semplice.extend_from_slice(
                &i16::try_from(ms - inizio)
                    .expect("sta in un i16")
                    .to_be_bytes(),
            );
            semplice.push(0x80);
            semplice.extend_from_slice(dati);
            if let Some((_, dentro)) = cluster.last_mut() {
                dentro.extend(ebml(&[0xA3], &semplice));
            }
        }
        let cluster: Vec<(u64, Vec<u8>)> = cluster
            .into_iter()
            .map(|(ts, dentro)| (ts, ebml(&[0x1F, 0x43, 0xB6, 0x75], &dentro)))
            .collect();

        // L'indice punta ai cluster con la loro posizione dall'inizio del
        // contenuto del segmento, e sta prima di loro: la sua lunghezza non
        // dipende dai numeri che contiene, quindi si calcola con posizioni
        // qualunque e si riscrive con quelle vere.
        let indice = |posizioni: &[(u64, u64)]| {
            let punti: Vec<u8> = posizioni
                .iter()
                .flat_map(|(ts, posizione)| {
                    ebml(
                        &[0xBB],
                        &[
                            ebml_intero(&[0xB3], *ts),
                            ebml(
                                &[0xB7],
                                &[ebml_intero(&[0xF7], 1), ebml_intero(&[0xF1], *posizione)]
                                    .concat(),
                            ),
                        ]
                        .concat(),
                    )
                })
                .collect();
            ebml(&[0x1C, 0x53, 0xBB, 0x6B], &punti)
        };
        let finte: Vec<(u64, u64)> = cluster.iter().map(|(ts, _)| (*ts, 0)).collect();
        let mut qui = info.len() + tracce.len() + indice(&finte).len();
        let mut posizioni = Vec::new();
        for (ts, c) in &cluster {
            posizioni.push((*ts, u64::try_from(qui).expect("sta")));
            qui += c.len();
        }
        let tutti: Vec<u8> = cluster.into_iter().flat_map(|(_, c)| c).collect();
        let segmento = ebml(
            &[0x18, 0x53, 0x80, 0x67],
            &[info, tracce, indice(&posizioni), tutti].concat(),
        );
        [testa, segmento].concat()
    }

    /// Un Matroska con PCM a 16 bit little-endian, mono a 48 kHz, in blocchi
    /// da dieci millisecondi.
    fn mka(campioni: &[i16]) -> Vec<u8> {
        let audio = ebml(
            &[0xE1],
            &[
                ebml(&[0xB5], &48_000.0_f64.to_be_bytes()),
                ebml_intero(&[0x9F], 1),
                ebml_intero(&[0x62, 0x64], 16),
            ]
            .concat(),
        );
        let codec = [
            ebml(&[0x86], b"A_PCM/INT/LIT"),
            // La durata di un blocco, in nanosecondi: senza, symphonia non sa
            // dove finisce un blocco e un salto atterra sul successivo.
            ebml_intero(&[0x23, 0xE3, 0x83], 10_000_000),
            audio,
        ]
        .concat();
        let blocchi: Vec<(u64, Vec<u8>)> = campioni
            .chunks(480)
            .enumerate()
            .map(|(i, pezzo)| {
                let dati = pezzo.iter().flat_map(|c| c.to_le_bytes()).collect();
                (u64::try_from(i * 10).expect("sta"), dati)
            })
            .collect();
        matroska(&codec, &blocchi)
    }

    /// I pacchetti di un Ogg, nell'ordine in cui escono dalle pagine: il
    /// minimo per rimetterli in un altro contenitore.
    fn pacchetti_ogg(byte: &[u8]) -> Vec<Vec<u8>> {
        let mut pacchetti = Vec::new();
        let mut corrente = Vec::new();
        let mut qui = 0;
        while byte.get(qui..qui + 4) == Some(&b"OggS"[..]) {
            let segmenti = usize::from(*byte.get(qui + 26).expect("intestazione intera"));
            let tabella = byte
                .get(qui + 27..qui + 27 + segmenti)
                .expect("tabella intera");
            let mut dati = qui + 27 + segmenti;
            for lunghezza in tabella {
                let lunghezza = usize::from(*lunghezza);
                corrente.extend_from_slice(byte.get(dati..dati + lunghezza).expect("dati interi"));
                dati += lunghezza;
                if lunghezza < 255 {
                    pacchetti.push(std::mem::take(&mut corrente));
                }
            }
            qui = dati;
        }
        pacchetti
    }

    #[test]
    fn un_caf_suona_e_salta_al_fotogramma() {
        // Il CAF c'era già in symphonia 0.5, spento; in 0.6 è acceso. Mezzo
        // secondo di rampa a 48 kHz: esce identica, e un salto atterra dove
        // deve come su un WAV.
        let mut d = Decodificatore::apri(sorgente(caf(48_000, &rampa(24_000)), "caf"), 48_000, 1)
            .expect("il CAF si apre");
        let tutto_il_brano = tutto(&mut d);
        assert_eq!(tutto_il_brano.len(), 24_000);
        assert!((tutto_il_brano[23_999] - 23_999.0 / 32_768.0).abs() < 1e-6);

        let mut d = Decodificatore::apri(sorgente(caf(48_000, &rampa(24_000)), "caf"), 48_000, 1)
            .expect("il CAF si apre");
        d.cerca(250).expect("il salto riesce");
        let resto = tutto(&mut d);
        assert_eq!(resto.len(), 12_000);
        assert!((resto[0] - 12_000.0 / 32_768.0).abs() < 1e-6);
    }

    #[test]
    fn un_matroska_suona_e_salta_avanti_e_indietro() {
        // Matroska: timestamp al millisecondo, e un salto all'indietro che
        // passa dall'indice. Un secondo di rampa a 48 kHz, mono.
        let mut d = Decodificatore::apri(sorgente(mka(&rampa(48_000)), "mka"), 48_000, 1)
            .expect("il Matroska si apre");
        let tutto_il_brano = tutto(&mut d);
        assert_eq!(tutto_il_brano.len(), 48_000);
        let ultimo = f32::from(rampa(48_000)[47_999]) / 32_768.0;
        assert!((tutto_il_brano[47_999] - ultimo).abs() < 1e-6);

        let mut d = Decodificatore::apri(sorgente(mka(&rampa(48_000)), "mka"), 48_000, 1)
            .expect("il Matroska si apre");
        // Un salto a metà di un blocco: lo scarto fino al bersaglio lo porta
        // al fotogramma, come per gli altri contenitori.
        d.cerca(255).expect("il salto riesce");
        let mut blocco = Vec::new();
        assert!(d.prossimo(&mut blocco).expect("si legge"));
        assert!(
            (blocco[0] - 12_240.0 / 32_768.0).abs() < 1e-6,
            "dopo il salto a 255 ms: {}",
            blocco[0] * 32_768.0
        );
        // E all'indietro, che senza l'indice Matroska non saprebbe fare.
        d.cerca(100).expect("il salto riesce");
        assert!(d.prossimo(&mut blocco).expect("si legge"));
        assert!(
            (blocco[0] - 4_800.0 / 32_768.0).abs() < 1e-6,
            "dopo il salto a 100 ms: {}",
            blocco[0] * 32_768.0
        );
        assert_eq!(d.consegnati(), 4_800 + fotogrammi_in(&blocco, 1));
    }

    #[test]
    fn un_opus_in_matroska_toglie_il_pre_skip_e_non_la_coda() {
        // Gli stessi pacchetti del tono Opus di prova, tolti dall'Ogg e rimessi
        // in un Matroska come li scrive un WebM: l'`OpusHead` nel
        // `CodecPrivate`, il pre-skip come `CodecDelay` in nanosecondi. È la
        // strada per cui il decodificatore Opus riceve canali solo contati, e
        // deve descriverli da sé.
        let pacchetti = pacchetti_ogg(OPUS_TONO);
        let testa = pacchetti.first().expect("l'OpusHead").clone();
        assert_eq!(testa.get(..8), Some(&b"OpusHead"[..]));
        let canali = u64::from(testa[9]);
        let pre_skip = u64::from(u16::from_le_bytes([testa[10], testa[11]]));
        let audio = ebml(
            &[0xE1],
            &[
                ebml(&[0xB5], &48_000.0_f64.to_be_bytes()),
                ebml_intero(&[0x9F], canali),
            ]
            .concat(),
        );
        let codec = [
            ebml(&[0x86], b"A_OPUS"),
            ebml(&[0x63, 0xA2], &testa),
            ebml_intero(&[0x56, 0xAA], (pre_skip * 1_000_000_000).div_ceil(48_000)),
            ebml_intero(&[0x56, 0xBB], 80_000_000),
            audio,
        ]
        .concat();
        // Il primo pacchetto dopo l'intestazione è quello dei commenti; ogni
        // pacchetto audio del tono dura venti millisecondi.
        let blocchi: Vec<(u64, Vec<u8>)> = pacchetti
            .iter()
            .skip(2)
            .enumerate()
            .map(|(i, p)| (u64::try_from(i * 20).expect("sta"), p.clone()))
            .collect();
        let decodificati = u64::try_from(blocchi.len() * 960).expect("sta");
        let byte = matroska(&codec, &blocchi);

        let mut d = Decodificatore::apri(sorgente(byte.clone(), "webm"), 48_000, 1)
            .expect("il WebM si apre");
        let campioni = tutto(&mut d);
        // Il pre-skip lo toglie il decodificatore, come in Ogg. L'imbottitura
        // in coda invece resta: Matroska la dice con `DiscardPadding`, che
        // symphonia 0.6.1 non legge. È il limite per cui un album WebM non
        // sarebbe senza stacchi, e questa prova lo tiene scritto.
        assert_eq!(
            u64::try_from(campioni.len()).expect("sta"),
            decodificati - pre_skip
        );
        assert!(campioni.len() >= 144_000);
        let mezzo = &campioni[48_000..96_000];
        let efficace = (mezzo.iter().map(|c| c * c).sum::<f32>() / mezzo.len() as f32).sqrt();
        assert!(efficace > 0.2, "valore efficace: {efficace}");

        // Il salto: i timestamp di Matroska sono al millisecondo e il ritardo
        // del codec ci entra arrotondato, quindi la precisione è quella — un
        // millisecondo, quarantotto fotogrammi — e non il fotogramma.
        let mut d =
            Decodificatore::apri(sorgente(byte, "webm"), 48_000, 1).expect("il WebM si apre");
        d.cerca(1_500).expect("il salto riesce");
        assert_eq!(d.consegnati(), 72_000);
        let resto = tutto(&mut d);
        let atteso = campioni.len() - 72_000;
        assert!(
            resto.len().abs_diff(atteso) <= 48,
            "dopo il salto a 1,5 s restano {} fotogrammi invece di {atteso}",
            resto.len()
        );
    }

    #[test]
    fn lo_stereo_su_un_5_1_esce_solo_davanti() {
        // Il giro intero, dal file al dispositivo: sinistro e destro distinti,
        // uscita a sei canali.
        let mut campioni = Vec::new();
        for _ in 0..4_800 {
            campioni.push(10_000);
            campioni.push(-20_000);
        }
        let byte = wav(48_000, 2, &campioni);
        let mut d = Decodificatore::apri(sorgente(byte, "wav"), 48_000, 6).expect("si apre");
        let fuori = tutto(&mut d);
        assert_eq!(fuori.len(), 4_800 * 6);
        for fotogramma in fuori.chunks_exact(6) {
            assert!((fotogramma[0] - 10_000.0 / 32_768.0).abs() < 1e-6);
            assert!((fotogramma[1] + 20_000.0 / 32_768.0).abs() < 1e-6);
            assert!(fotogramma[2..].iter().all(|c| *c == 0.0), "{fotogramma:?}");
        }
    }

    /// Byte che, a bandiera alzata, rispondono con un errore come farebbe un
    /// flusso di rete a cui è stato chiesto di smettere.
    struct Annullabile {
        dentro: Byte,
        bandiera: Option<Arc<AtomicBool>>,
        /// Da quale byte in poi le letture guardano la bandiera: prima c'è
        /// l'intestazione, che si legge all'apertura.
        dopo: u64,
    }

    impl std::io::Read for Annullabile {
        fn read(&mut self, dove: &mut [u8]) -> std::io::Result<usize> {
            let posizione = self.dentro.0.position();
            if posizione >= self.dopo
                && self
                    .bandiera
                    .as_ref()
                    .is_some_and(|b| b.load(Ordering::Relaxed))
            {
                // Lo stesso numero che `aether-net` usa per la rete: senza la
                // bandiera diventerebbe un guasto di rete.
                return Err(std::io::Error::from_raw_os_error(64));
            }
            self.dentro.read(dove)
        }
    }

    impl std::io::Seek for Annullabile {
        fn seek(&mut self, da: std::io::SeekFrom) -> std::io::Result<u64> {
            self.dentro.seek(da)
        }
    }

    impl Flusso for Annullabile {
        fn lunghezza(&self) -> Option<u64> {
            self.dentro.lunghezza()
        }
        fn ascolta_annullo(&mut self, annullo: Arc<AtomicBool>) {
            self.bandiera = Some(annullo);
        }
    }

    #[test]
    fn una_lettura_annullata_non_e_un_guasto() {
        // Dieci secondi, e non mezzo: il flusso di symphonia tiene in memoria
        // fino a 64 KiB letti in anticipo, e un file che ci stesse tutto non
        // leggerebbe mai più niente dopo l'apertura.
        let quanti = 480_000;
        let bandiera = Arc::new(AtomicBool::new(false));
        let s = Sorgente {
            track_id: 1,
            media: Box::new(Annullabile {
                dentro: Byte(std::io::Cursor::new(wav(48_000, 1, &rampa(quanti)))),
                bandiera: None,
                dopo: 44,
            }),
            estensione: Some("wav".to_owned()),
            durata_ms: 0,
            replaygain_db: None,
            picco: None,
        };
        let mut d =
            Decodificatore::apri_con_annullo(s, 48_000, 1, Arc::clone(&bandiera)).expect("si apre");

        // A bandiera alzata la lettura si ferma, e quel che esce è un
        // annullamento: non «la rete non risponde», che mostrerebbe «Riprova»
        // — il flusso ha risposto col numero della rete apposta — e nemmeno
        // la fine del brano.
        bandiera.store(true, Ordering::Relaxed);
        let mut blocco = Vec::new();
        let mut esito = Ok(true);
        for _ in 0..1_000 {
            esito = d.prossimo(&mut blocco);
            if esito.is_err() {
                break;
            }
        }
        let Err(err) = esito else {
            panic!("a bandiera alzata la lettura deve fermarsi");
        };
        assert_eq!(
            err.code().kind(),
            aether_domain::errors::ErrorCodeKind::InternalAborted
        );
        assert!(!d.esaurito, "un annullo non chiude il brano");

        // Abbassata la bandiera, il comando nuovo — un salto — trova il brano
        // intero, e atterra dove deve.
        bandiera.store(false, Ordering::Relaxed);
        d.cerca(200).expect("il salto riesce");
        assert!(d.prossimo(&mut blocco).expect("si legge"));
        assert!((blocco[0] - 9_600.0 / 32_768.0).abs() < 1e-6);
        d.cerca(0).expect("il salto riesce");
        assert_eq!(tutto(&mut d).len(), quanti);
    }
}
