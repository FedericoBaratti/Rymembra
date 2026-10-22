//! Monkey's Audio (`.ape`), letto come un contenitore che consegna PCM.
//!
//! # Perché un lettore di contenitore e non un codec
//!
//! symphonia divide il lavoro in due: il demultiplatore taglia il file in
//! pacchetti, il codec li fa diventare campioni. Monkey's Audio si potrebbe
//! dividere allo stesso modo — `ape-decoder` ha un `FrameDecoder` pensato
//! proprio per questo — ma i due pezzi dovrebbero parlarsi più di quanto un
//! pacchetto sappia dire: dove comincia il tratto dentro il primo byte, quanti
//! fotogrammi contiene, con che versione è stato compresso. Qui invece il
//! lettore fa tutte e due le cose, e consegna pacchetti **già PCM**, interlacciati
//! e little-endian, con i parametri del codec PCM di symphonia giusto per i
//! bit. Il decodificatore PCM che c'è già fa il resto, e al registro dei codec
//! non si aggiunge niente.
//!
//! # A flusso, sempre
//!
//! Monkey's Audio comprime a **tratti** — i suoi `frame`, da 73 728 fotogrammi
//! in su — indipendenti l'uno dall'altro: il predittore si azzera all'inizio di
//! ognuno. Il lettore ne tiene in memoria uno solo, decodificato, e lo consegna
//! a pacchetti di [`PACCHETTO`] fotogrammi; poi passa al seguente. Un salto è
//! aritmetica sulla tabella dei tratti, letta all'apertura insieme
//! all'intestazione, e non legge niente.
//!
//! Mai `decode_all` di `ape-decoder`: di un'immagine CD da settanta minuti
//! farebbe settecento megabyte in memoria, su macchine che non li hanno.
//!
//! # Quel che non si legge
//!
//! I file in virgola mobile: `ape-decoder` 0.3.2 non applica la trasformazione
//! che li riporta a numeri IEEE, e quel che uscirebbe sarebbe rumore. E i file
//! scritti da Monkey's Audio prima della 3.99: vedi [`VERSIONE_MINIMA`]. Gli
//! uni e gli altri si dichiarano non supportati, che è la verità, invece di
//! diventare file «danneggiati» che non lo sono.
//!
//! # Un tratto rovinato, un file troncato
//!
//! Un tratto che non si decodifica diventa **silenzio lungo esattamente quanto
//! lui**, e la lettura passa al tratto dopo. Prima l'errore saliva fino a
//! `decodifica`, che dal demultiplatore non sa saltare niente: un settore
//! rovinato chiudeva il brano — e, dentro un'immagine CD, anche tutti quelli
//! dopo che ci cadevano. Il silenzio, e non un tratto saltato, perché i
//! fotogrammi devono continuare a contare giusti: il cursore, i salti e i
//! confini dei brani di un `.cue` sono tutti numeri di fotogramma del file.
//!
//! Un tratto i cui byte stanno oltre la fine del file invece **chiude il
//! flusso**: il file è troncato, dopo non c'è più niente da suonare, e un brano
//! d'immagine che comincia lì dentro risulta vuoto invece di dare lo stesso
//! errore a ogni brano che segue.
//!
//! Gli errori di lettura veri — la rete caduta, la lettura annullata — passano
//! com'erano e non fanno avanzare niente: chi riprova rilegge lo stesso tratto.
//!
//! Non c'è una riga di diagnostica per il tratto rovinato: questo crate non ha
//! un diario suo, e il motore racconta solo gli errori che salgono.

use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, Mutex, PoisonError};

use ape_decoder::{ApeDecoder, ApeError, ApeResult};
use symphonia::core::audio::{Channels, Position};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::AudioCodecParameters;
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_PCM_S16LE, CODEC_ID_PCM_S24LE, CODEC_ID_PCM_S32LE, CODEC_ID_PCM_U8,
};
use symphonia::core::common::FourCc;
use symphonia::core::errors::{Error, Result, SeekErrorKind};
use symphonia::core::formats::probe::{
    ProbeDataMatchSpec, ProbeFormatData, ProbeableFormat, Score, Scoreable,
};
use symphonia::core::formats::{
    FormatId, FormatInfo, FormatOptions, FormatReader, MediaInfo, SeekMode, SeekTo, SeekedTo, Track,
};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions, ReadBytes, ScopedStream};
use symphonia::core::meta::{Metadata, MetadataLog};
use symphonia::core::packet::Packet;
use symphonia::core::units::{Duration, Timestamp};

/// Il contenitore Monkey's Audio, per chi deve riconoscerlo: è il nome che
/// `decodifica` mostra al posto di «PCM».
///
/// symphonia non ne ha uno fra i suoi. Uno fatto con un codice di quattro
/// lettere ha il bit più alto acceso, e non può coincidere con quelli della
/// libreria, che sono numeri piccoli.
pub(crate) const FORMAT_ID_APE: FormatId = FormatId::new(FourCc::new(*b"MAC "));

/// Come si presenta.
const INFO: FormatInfo = FormatInfo {
    format: FORMAT_ID_APE,
    short_name: "ape",
    long_name: "Monkey's Audio",
};

/// Quanti fotogrammi al massimo in un pacchetto.
///
/// Un tratto ne contiene da settantamila a più di un milione, secondo la
/// compressione scelta: consegnarlo intero vorrebbe dire buffer da megabyte
/// per ogni pacchetto, in tutta la catena che sta sotto. Quattromila sono un
/// blocco FLAC, e il resto del motore è già fatto per quella misura.
const PACCHETTO: u64 = 4_096;

/// La versione più vecchia che `ape-decoder` sa decodificare davvero: la 3.99.
///
/// # Perché non la 3.95 che dichiara
///
/// Perché il crate accetta i file dalla 3.95 in su, ma il suo decodificatore
/// entropico e il suo range coder sono scritti per la 3.99 e oltre — lo dicono
/// le prime righe di `entropy.rs` e `range_coder.rs` — e fra la 3.95 e la 3.98
/// la codifica era un'altra. La prova sul NAS l'ha trovato su un'immagine
/// della 3.97: ffmpeg la decodifica, `ape-decoder` si ferma al primo tratto con
/// «range coder: overflow». Uno su centosettantacinque dei file `.ape` visti;
/// meglio dirli non supportati che farli sembrare rotti. Il rifiuto sta in
/// [`LettoreApe::nuovo`] e non nel riconoscimento: il perché è in
/// [`LettoreApe::score`].
const VERSIONE_MINIMA: u16 = 3_990;

/// Il tratto decodificato più grande che `ape-decoder` accetta di produrre, in
/// byte: oltre, rifiuta ogni tratto con un errore suo.
///
/// Si controlla all'apertura perché un tratto rovinato diventa silenzio: un
/// file i cui tratti sono **tutti** troppo grandi, che la libreria non
/// decodificherà mai, suonerebbe un'ora di silenzio invece di dire che non si
/// sa leggere.
const TRATTO_MASSIMO: u64 = 64 * 1024 * 1024;

/// Quel che il lettore chiede a chi decodifica i tratti: `ApeDecoder` nel
/// lettore vero.
///
/// # Perché un tratto e non `ApeDecoder` e basta
///
/// Per le prove. Un `.ape` non si sa scrivere — non c'è un codificatore a
/// portata delle prove, né in Rust né fra gli strumenti della macchina — e
/// rovinarne uno vero a un tratto preciso vorrebbe dire tenerne uno nel
/// repository. Dietro questo tratto le prove mettono un doppio che rovina, tronca
/// o fa cadere la rete al tratto voluto, e provano il lettore vero — consegna,
/// salti, silenzio — attraverso `FormatReader`.
trait Tratti: Send + Sync {
    /// Quanti tratti ha il file.
    fn quanti(&self) -> u32;

    /// Quanti fotogrammi ha il tratto `tratto`.
    fn blocchi(&self, tratto: u32) -> u64;

    /// Il tratto `tratto`, decodificato: interlacciato e little-endian.
    fn decodifica(&mut self, tratto: u32) -> ApeResult<Vec<u8>>;

    /// In che tratto cade il fotogramma `fotogramma` del file, e quanti
    /// fotogrammi di quel tratto vengono prima di lui.
    fn dove(&mut self, fotogramma: u64) -> ApeResult<(u32, u64)>;

    /// I byte del tratto `tratto` vanno oltre la fine del file?
    fn oltre_i_dati(&self, tratto: u32) -> bool;
}

impl<R: Read + Seek + Send + Sync> Tratti for ApeDecoder<R> {
    fn quanti(&self) -> u32 {
        self.total_frames()
    }

    fn blocchi(&self, tratto: u32) -> u64 {
        u64::from(self.file_info().frame_block_count(tratto))
    }

    fn decodifica(&mut self, tratto: u32) -> ApeResult<Vec<u8>> {
        self.decode_frame(tratto)
    }

    fn dove(&mut self, fotogramma: u64) -> ApeResult<(u32, u64)> {
        let punto = self.seek(fotogramma)?;
        Ok((punto.frame_index, u64::from(punto.skip_samples)))
    }

    /// Dalla tabella dei tratti, letta all'apertura: un tratto finisce dove
    /// comincia il seguente, e l'ultimo dove l'intestazione dice che finiscono
    /// i dati compressi. La lunghezza del file è quella vista all'apertura.
    ///
    /// Si chiede solo dopo che un tratto non si è decodificato: è ciò che
    /// distingue un file troncato — dove `ape-decoder` dice «lettura corta», un
    /// errore di decodifica e non di lettura — da un tratto rovinato in mezzo a
    /// un file intero.
    fn oltre_i_dati(&self, tratto: u32) -> bool {
        let info = self.file_info();
        let fine = if tratto.saturating_add(1) < self.total_frames() {
            info.seek_byte(tratto.saturating_add(1))
        } else {
            info.seek_byte(0).saturating_add(info.ape_frame_data_bytes)
        };
        info.seek_byte(tratto) >= info.file_bytes || fine > info.file_bytes
    }
}

/// Il flusso, tenuto in comune fra il lettore e `ape-decoder`.
///
/// # Perché in comune
///
/// Perché `ApeDecoder` si prende il flusso e non lo restituisce più, mentre un
/// lettore di symphonia deve saperlo restituire (`into_inner`). Con il flusso
/// dietro un `Arc` il decodificatore legge attraverso questa struttura, e il
/// lettore lo può riprendere quando gli è chiesto. Il lucchetto si prende una
/// volta per lettura, cioè una o due volte per tratto: non costa niente.
struct InComune<'s>(Arc<Mutex<Option<MediaSourceStream<'s>>>>);

impl Read for InComune<'_> {
    /// Legge fino a riempire `dove`, o fino alla fine del file.
    ///
    /// # Perché fino a riempire
    ///
    /// Perché `ape-decoder` legge un tratto con **una** chiamata a `read` e
    /// considera rovinato il tratto se ne arriva di meno, mentre il flusso di
    /// symphonia, per le letture grandi, fa una sola lettura dalla sorgente e
    /// restituisce quel che è arrivato. Da un disco arriva quasi sempre tutto;
    /// da una condivisione di rete no, e un tratto spezzato da una lettura
    /// corta diventerebbe un file «danneggiato» che non lo è.
    fn read(&mut self, dove: &mut [u8]) -> std::io::Result<usize> {
        let mut flusso = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(flusso) = flusso.as_mut() else {
            return Err(std::io::Error::other("il flusso è già stato restituito"));
        };
        let mut letti = 0;
        while let Some(resto) = dove.get_mut(letti..) {
            if resto.is_empty() {
                break;
            }
            match flusso.read(resto) {
                Ok(0) => break,
                Ok(n) => letti += n,
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                Err(err) => return Err(err),
            }
        }
        Ok(letti)
    }
}

impl Seek for InComune<'_> {
    fn seek(&mut self, dove: SeekFrom) -> std::io::Result<u64> {
        let mut flusso = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match flusso.as_mut() {
            Some(flusso) => flusso.seek(dove),
            None => Err(std::io::Error::other("il flusso è già stato restituito")),
        }
    }
}

/// Un errore di `ape-decoder` come lo vuole symphonia.
///
/// Quelli di lettura passano **com'erano**: è lì dentro che viaggiano il
/// numero di sistema di una condivisione caduta e l'errore con cui un flusso
/// annullato smette di aspettare, e `decodifica` li riconosce entrambi. Tutto
/// il resto è un file che non si decodifica.
fn errore(err: ApeError) -> Error {
    match err {
        ApeError::Io(io) => Error::IoError(io),
        ApeError::InvalidFormat(cosa) | ApeError::DecodingError(cosa) => Error::DecodeError(cosa),
        ApeError::InvalidChecksum => Error::DecodeError("ape: il controllo di un tratto non torna"),
        ApeError::UnsupportedVersion(_) => Error::Unsupported("ape: versione troppo vecchia"),
        _ => Error::DecodeError("ape: errore di decodifica"),
    }
}

/// Il lettore di un file Monkey's Audio.
pub(crate) struct LettoreApe<'s> {
    /// Il flusso, per poterlo restituire. Vedi [`InComune`].
    flusso: Arc<Mutex<Option<MediaSourceStream<'s>>>>,
    /// `ApeDecoder` sopra [`InComune`]; un doppio nelle prove. Vedi [`Tratti`].
    decodificatore: Box<dyn Tratti + 's>,
    /// Una traccia sola, la numero zero.
    tracce: Vec<Track>,
    media: MediaInfo,
    metadati: MetadataLog,
    /// I byte di un fotogramma: canali per byte del campione.
    allineamento: usize,
    /// Il byte del silenzio: `0x80` a 8 bit, che sono senza segno con lo zero
    /// a metà scala come nei WAV; zero per tutti gli altri.
    zero: u8,
    /// Quanti fotogrammi ha il file.
    totali: u64,
    /// Quanti fotogrammi ha ogni tratto, tranne l'ultimo.
    per_tratto: u64,
    /// Il tratto da decodificare quando [`Self::pcm`] finisce.
    prossimo_tratto: u32,
    /// Il tratto decodificato, interlacciato e little-endian, e da che byte
    /// riprende la consegna.
    pcm: Vec<u8>,
    letti: usize,
    /// Quanti fotogrammi di silenzio restano da consegnare al posto di un
    /// tratto che non si è decodificato. Si consegnano a pacchetti come il
    /// resto, senza mai tenerli in memoria tutti: un tratto può valere
    /// centinaia di megabyte decodificato.
    silenzio: u64,
    /// Il fotogramma del file a cui comincia quel che si consegna dopo: il
    /// silenzio, se ne resta, o `pcm[letti..]`.
    fotogramma: u64,
    /// Quanti fotogrammi saltare in testa al prossimo tratto decodificato:
    /// dove un salto è atterrato dentro il tratto.
    da_saltare: u64,
}

impl<'s> LettoreApe<'s> {
    fn nuovo(flusso: MediaSourceStream<'s>, opzioni: FormatOptions) -> Result<Self> {
        let flusso = Arc::new(Mutex::new(Some(flusso)));
        // Legge l'intestazione e la tabella dei tratti, e nient'altro: niente
        // audio, e la tabella ha un tetto di quattro mega.
        let decodificatore = ApeDecoder::new(InComune(Arc::clone(&flusso))).map_err(errore)?;
        let info = decodificatore.info();
        if info.version < VERSIONE_MINIMA {
            return Err(Error::Unsupported("ape: versione anteriore alla 3.99"));
        }
        if info.is_floating_point {
            return Err(Error::Unsupported("ape: campioni in virgola mobile"));
        }
        // Quel che `ape-decoder` scrive, guardato nel sorgente della 0.3.2:
        // a 8 bit senza segno, con lo zero a 128, come nei WAV; a 16, 24 e 32
        // bit con segno e little-endian, anche quando il file d'origine era
        // big-endian, perché il crate non lo riconverte.
        let codec = match info.bits_per_sample {
            8 => CODEC_ID_PCM_U8,
            16 => CODEC_ID_PCM_S16LE,
            24 => CODEC_ID_PCM_S24LE,
            32 => CODEC_ID_PCM_S32LE,
            _ => return Err(Error::Unsupported("ape: bit per campione")),
        };
        let bit = u32::from(info.bits_per_sample);
        // I canali come li descrive un WAV dello stesso numero: è da un WAV che
        // il file è stato compresso, quasi sempre.
        let canali = Position::from_wave_channel_count(u32::from(info.channels))
            .map_or(Channels::Discrete(info.channels), Channels::Positioned);
        let per_tratto = u64::from(info.blocks_per_frame);
        if per_tratto.saturating_mul(u64::from(info.block_align)) > TRATTO_MASSIMO {
            return Err(Error::Unsupported("ape: tratti troppo grandi"));
        }

        let mut parametri = AudioCodecParameters::new();
        parametri
            .for_codec(codec)
            .with_sample_rate(info.sample_rate)
            .with_bits_per_sample(bit)
            .with_bits_per_coded_sample(bit)
            .with_channels(canali)
            .with_max_frames_per_packet(PACCHETTO.min(per_tratto).max(1));
        let mut traccia = Track::new(0);
        traccia
            .with_codec_params(CodecParameters::Audio(parametri))
            .with_num_frames(info.total_samples)
            .with_duration(Duration::new(info.total_samples));

        let allineamento = usize::from(info.block_align);
        let zero = if info.bits_per_sample == 8 { 0x80 } else { 0 };
        let totali = info.total_samples;
        Ok(Self {
            flusso,
            media: MediaInfo::from_track(&traccia),
            tracce: vec![traccia],
            metadati: opzioni.external_data.metadata.unwrap_or_default(),
            decodificatore: Box::new(decodificatore),
            allineamento,
            zero,
            totali,
            per_tratto,
            prossimo_tratto: 0,
            pcm: Vec::new(),
            letti: 0,
            silenzio: 0,
            fotogramma: 0,
            da_saltare: 0,
        })
    }

    /// Decodifica il tratto dopo. `false` se il file è finito.
    ///
    /// Un tratto che non si decodifica diventa silenzio, e uno oltre la fine
    /// dei dati chiude il flusso: il perché è in testa al modulo.
    fn decodifica_il_prossimo(&mut self) -> Result<bool> {
        let tratto = self.prossimo_tratto;
        if tratto >= self.decodificatore.quanti() {
            return Ok(false);
        }
        let decodificato = match self.decodificatore.decodifica(tratto) {
            Ok(pcm) => Some(pcm),
            // La rete, l'annullo: passano com'erano, e niente avanza — né il
            // tratto né il punto del salto — così chi riprova rilegge lo stesso.
            Err(ApeError::Io(io)) => return Err(Error::IoError(io)),
            // Troncato: dopo non c'è più niente. Il tratto si porta in fondo,
            // così chi chiede ancora sente la fine senza un'altra lettura.
            Err(_) if self.decodificatore.oltre_i_dati(tratto) => {
                self.prossimo_tratto = self.decodificatore.quanti();
                self.da_saltare = 0;
                self.pcm.clear();
                self.letti = 0;
                return Ok(false);
            }
            Err(_) => None,
        };
        self.prossimo_tratto = tratto.saturating_add(1);
        let saltati = std::mem::take(&mut self.da_saltare);
        self.fotogramma = u64::from(tratto)
            .saturating_mul(self.per_tratto)
            .saturating_add(saltati);
        match decodificato {
            Some(pcm) => {
                self.pcm = pcm;
                let byte = usize::try_from(saltati)
                    .unwrap_or(usize::MAX)
                    .saturating_mul(self.allineamento);
                self.letti = byte.min(self.pcm.len());
            }
            // Rovinato: tanti fotogrammi di silenzio quanti ne avrebbe dati,
            // meno quelli che un salto aveva già lasciato indietro.
            None => {
                self.pcm.clear();
                self.letti = 0;
                self.silenzio = self.decodificatore.blocchi(tratto).saturating_sub(saltati);
            }
        }
        Ok(true)
    }

    /// Un pacchetto di silenzio, se ne resta da consegnare.
    fn pacchetto_di_silenzio(&mut self) -> Option<Packet> {
        if self.silenzio == 0 || self.allineamento == 0 {
            return None;
        }
        let quanti = self.silenzio.min(PACCHETTO);
        let byte = usize::try_from(quanti)
            .unwrap_or(usize::MAX)
            .saturating_mul(self.allineamento);
        let dati: Box<[u8]> = vec![self.zero; byte].into_boxed_slice();
        let pts = Timestamp::new(i64::try_from(self.fotogramma).unwrap_or(i64::MAX));
        self.silenzio -= quanti;
        self.fotogramma = self.fotogramma.saturating_add(quanti);
        Some(Packet::new(0, pts, Duration::new(quanti), dati))
    }
}

impl Scoreable for LettoreApe<'_> {
    fn score(mut flusso: ScopedStream<&mut MediaSourceStream<'_>>) -> Result<Score> {
        let firma = flusso.read_quad_bytes()?;
        let versione = u16::from_le_bytes(flusso.read_double_bytes()?);
        // Si riconosce **ogni** Monkey's Audio, anche quelli che non sappiamo
        // decodificare: a quelli dice di no [`LettoreApe::nuovo`], e il no
        // ferma il riconoscimento.
        //
        // Il contrario — non riconoscerli — sembrava la stessa cosa, ed era
        // rumore bianco. Il riconoscitore, non trovato chi li legge, continua a
        // cercare dentro il file, e i Monkey's Audio vecchi conservano lì dentro
        // l'intestazione del WAV da cui sono nati: il lettore WAV la trovava, e
        // suonava i byte compressi come se fossero campioni. Visto sul NAS, su
        // un'immagine della 3.97.
        //
        // La versione serve solo a non scambiare per Monkey's Audio quattro
        // byte qualunque: le sue sono nelle migliaia — 3990 è la 3.99.
        if !matches!(&firma, b"MAC " | b"MACF") || versione < 1_000 {
            return Ok(Score::Unsupported);
        }
        Ok(Score::Supported(255))
    }
}

impl ProbeableFormat<'_> for LettoreApe<'_> {
    fn try_probe_new(
        flusso: MediaSourceStream<'_>,
        opzioni: FormatOptions,
    ) -> Result<Box<dyn FormatReader + '_>> {
        Ok(Box::new(LettoreApe::nuovo(flusso, opzioni)?))
    }

    fn probe_data() -> &'static [ProbeFormatData] {
        &[ProbeFormatData {
            spec: ProbeDataMatchSpec {
                extensions: &["ape"],
                mime_types: &["audio/ape", "audio/x-ape"],
                markers: &[b"MAC ", b"MACF"],
            },
            info: INFO,
        }]
    }
}

impl FormatReader for LettoreApe<'_> {
    fn format_info(&self) -> &FormatInfo {
        &INFO
    }

    fn media_info(&self) -> &MediaInfo {
        &self.media
    }

    fn metadata(&mut self) -> Metadata<'_> {
        self.metadati.metadata()
    }

    /// Si posiziona al fotogramma chiesto, esatto.
    ///
    /// Il tratto che lo contiene si decodifica alla lettura dopo, e da lì si
    /// consegna dal fotogramma chiesto: il salto atterra dove deve, senza
    /// niente da scartare per chi sta sopra. `ape-decoder` dice in quale tratto
    /// e a che punto; la decodifica vera la fa [`Self::next_packet`], così due
    /// salti di fila non decodificano due tratti.
    fn seek(&mut self, _modo: SeekMode, a: SeekTo) -> Result<SeekedTo> {
        let voluto = match a {
            SeekTo::Timestamp { ts, .. } => ts,
            SeekTo::Time { time, .. } => self
                .tracce
                .first()
                .and_then(|t| t.time_base)
                .and_then(|base| base.calc_timestamp(time))
                .ok_or(Error::SeekError(SeekErrorKind::OutOfRange))?,
        };
        // Come il WAV di symphonia: prima dell'inizio o dopo la fine non si
        // va, e la fine esatta è un posto valido da cui non esce più niente.
        let fotogramma =
            u64::try_from(voluto.get()).map_err(|_| Error::SeekError(SeekErrorKind::OutOfRange))?;
        if fotogramma > self.totali {
            return Err(Error::SeekError(SeekErrorKind::OutOfRange));
        }
        self.pcm.clear();
        self.letti = 0;
        self.silenzio = 0;
        self.fotogramma = fotogramma;
        if fotogramma == self.totali {
            self.prossimo_tratto = self.decodificatore.quanti();
            self.da_saltare = 0;
        } else {
            let (tratto, saltati) = self.decodificatore.dove(fotogramma).map_err(errore)?;
            self.prossimo_tratto = tratto;
            self.da_saltare = saltati;
        }
        Ok(SeekedTo {
            track_id: 0,
            required_ts: voluto,
            actual_ts: voluto,
        })
    }

    fn tracks(&self) -> &[Track] {
        &self.tracce
    }

    #[expect(
        clippy::integer_division,
        reason = "i byte rimasti divisi per quelli di un fotogramma: un resto \
                  sarebbe un tratto decodificato a metà fotogramma, e si lascia"
    )]
    fn next_packet(&mut self) -> Result<Option<Packet>> {
        loop {
            if let Some(pacchetto) = self.pacchetto_di_silenzio() {
                return Ok(Some(pacchetto));
            }
            let resto = self.pcm.len().saturating_sub(self.letti);
            if self.allineamento > 0 && resto >= self.allineamento {
                let quanti = u64::try_from(resto / self.allineamento)
                    .unwrap_or(u64::MAX)
                    .min(PACCHETTO);
                let fine = self.letti.saturating_add(
                    usize::try_from(quanti)
                        .unwrap_or(usize::MAX)
                        .saturating_mul(self.allineamento),
                );
                let dati: Box<[u8]> = self.pcm.get(self.letti..fine).unwrap_or_default().into();
                let pts = Timestamp::new(i64::try_from(self.fotogramma).unwrap_or(i64::MAX));
                self.letti = fine;
                self.fotogramma = self.fotogramma.saturating_add(quanti);
                return Ok(Some(Packet::new(0, pts, Duration::new(quanti), dati)));
            }
            if !self.decodifica_il_prossimo()? {
                return Ok(None);
            }
        }
    }

    fn into_inner<'a>(self: Box<Self>) -> MediaSourceStream<'a>
    where
        Self: 'a,
    {
        let preso = self
            .flusso
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        // Non capita: il flusso si riprende solo qui, e questa funzione
        // consuma il lettore. Ma «non capita» non è un motivo per un panico.
        preso.unwrap_or_else(|| {
            MediaSourceStream::new(
                Box::new(std::io::Cursor::new(Vec::<u8>::new())),
                MediaSourceStreamOptions::default(),
            )
        })
    }
}

#[cfg(test)]
mod prove {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Un decodificatore di tratti finto, mono a 16 bit: il fotogramma `n` del
    /// file vale `n` (modulo 32 768), così ogni campione dice da dove viene.
    /// Rovina, tronca o fa cadere la rete ai tratti che gli si dicono.
    struct Finto {
        per_tratto: u64,
        totali: u64,
        /// Tratti che non si decodificano: il controllo non torna.
        rovinati: Vec<u32>,
        /// Da questo tratto in poi i byte sono oltre la fine del file.
        troncato_da: Option<u32>,
        /// Questo tratto, la prima volta, trova la condivisione caduta.
        rete_una_volta: Option<u32>,
        /// Quante volte si è provato a decodificare, tratto o no.
        decodifiche: Arc<AtomicUsize>,
    }

    impl Finto {
        fn nuovo(per_tratto: u64, totali: u64) -> Self {
            Self {
                per_tratto,
                totali,
                rovinati: Vec::new(),
                troncato_da: None,
                rete_una_volta: None,
                decodifiche: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn troncato(&self, tratto: u32) -> bool {
            self.troncato_da.is_some_and(|da| tratto >= da)
        }
    }

    impl Tratti for Finto {
        fn quanti(&self) -> u32 {
            u32::try_from(self.totali.div_ceil(self.per_tratto)).expect("sta in un u32")
        }

        fn blocchi(&self, tratto: u32) -> u64 {
            let inizio = u64::from(tratto) * self.per_tratto;
            self.per_tratto.min(self.totali.saturating_sub(inizio))
        }

        fn decodifica(&mut self, tratto: u32) -> ApeResult<Vec<u8>> {
            self.decodifiche.fetch_add(1, Ordering::Relaxed);
            if self.rete_una_volta == Some(tratto) {
                self.rete_una_volta = None;
                return Err(ApeError::Io(std::io::Error::from_raw_os_error(64)));
            }
            // Quel che `ape-decoder` dice davvero di un file troncato.
            if self.troncato(tratto) {
                return Err(ApeError::DecodingError("short read on frame data"));
            }
            if self.rovinati.contains(&tratto) {
                return Err(ApeError::InvalidChecksum);
            }
            let inizio = u64::from(tratto) * self.per_tratto;
            let mut pcm = Vec::new();
            for n in inizio..inizio + self.blocchi(tratto) {
                pcm.extend_from_slice(&della_rampa(n).to_le_bytes());
            }
            Ok(pcm)
        }

        fn dove(&mut self, fotogramma: u64) -> ApeResult<(u32, u64)> {
            let tratto = u32::try_from(fotogramma.div_euclid(self.per_tratto)).expect("sta");
            Ok((tratto, fotogramma.rem_euclid(self.per_tratto)))
        }

        fn oltre_i_dati(&self, tratto: u32) -> bool {
            self.troncato(tratto)
        }
    }

    fn della_rampa(n: u64) -> i16 {
        i16::try_from(n % 32_768).expect("sta in un i16")
    }

    /// Il lettore vero, col doppio al posto di `ape-decoder`.
    fn lettore(finto: Finto) -> LettoreApe<'static> {
        let traccia = Track::new(0);
        LettoreApe {
            flusso: Arc::new(Mutex::new(None)),
            media: MediaInfo::from_track(&traccia),
            tracce: vec![traccia],
            metadati: MetadataLog::default(),
            allineamento: 2,
            zero: 0,
            totali: finto.totali,
            per_tratto: finto.per_tratto,
            decodificatore: Box::new(finto),
            prossimo_tratto: 0,
            pcm: Vec::new(),
            letti: 0,
            silenzio: 0,
            fotogramma: 0,
            da_saltare: 0,
        }
    }

    fn salta(l: &mut LettoreApe<'_>, fotogramma: i64) {
        l.seek(
            SeekMode::Accurate,
            SeekTo::Timestamp {
                ts: Timestamp::new(fotogramma),
                track_id: 0,
            },
        )
        .expect("il salto riesce");
    }

    /// Tutto quel che il lettore consegna da `da` in poi, controllando che
    /// ogni pacchetto cominci esattamente dove finiva il precedente.
    fn tutto(l: &mut LettoreApe<'_>, da: u64) -> Vec<i16> {
        let mut campioni = Vec::new();
        let mut atteso = da;
        while let Some(p) = l.next_packet().expect("si legge") {
            assert_eq!(
                p.pts.get(),
                i64::try_from(atteso).expect("sta"),
                "un buco nei tempi"
            );
            assert!(p.dur.get() <= PACCHETTO);
            let (coppie, avanzo) = p.data.as_chunks::<2>();
            assert!(avanzo.is_empty());
            assert_eq!(u64::try_from(coppie.len()).expect("sta"), p.dur.get());
            campioni.extend(coppie.iter().map(|c| i16::from_le_bytes(*c)));
            atteso += p.dur.get();
        }
        campioni
    }

    #[test]
    fn un_tratto_rovinato_diventa_silenzio_lungo_quanto_lui() {
        // Tre tratti da 10 000 e l'ultimo da 5 000; il secondo non si
        // decodifica. Prima il suo errore chiudeva il brano a 10 000.
        let mut finto = Finto::nuovo(10_000, 35_000);
        finto.rovinati = vec![1];
        let mut l = lettore(finto);
        let campioni = tutto(&mut l, 0);
        assert_eq!(campioni.len(), 35_000, "il file è lungo quanto dice");
        for (n, c) in campioni.iter().enumerate() {
            let n = u64::try_from(n).expect("sta");
            let atteso = if (10_000..20_000).contains(&n) {
                0
            } else {
                della_rampa(n)
            };
            assert_eq!(*c, atteso, "il fotogramma {n}");
        }

        // Un salto dentro il tratto rovinato: il silenzio comincia dal punto
        // chiesto e finisce dove finisce il tratto, poi riprende la musica.
        salta(&mut l, 12_345);
        let resto = tutto(&mut l, 12_345);
        assert_eq!(resto.len(), 35_000 - 12_345);
        assert!(resto[..7_655].iter().all(|c| *c == 0));
        assert_eq!(resto[7_655], della_rampa(20_000));

        // E l'ultimo tratto, più corto, rovinato anche lui: silenzio della sua
        // lunghezza, non di quella degli altri.
        let mut finto = Finto::nuovo(10_000, 35_000);
        finto.rovinati = vec![3];
        let campioni = tutto(&mut lettore(finto), 0);
        assert_eq!(campioni.len(), 35_000);
        assert!(campioni[30_000..].iter().all(|c| *c == 0));
    }

    #[test]
    fn il_silenzio_a_8_bit_sta_a_meta_scala() {
        // A 8 bit i campioni sono senza segno: lo zero vero è 128, e uno zero
        // di byte sarebbe il massimo negativo, uno scalino in piena scala.
        let mut finto = Finto::nuovo(10_000, 20_000);
        finto.rovinati = vec![0];
        let mut l = lettore(finto);
        l.allineamento = 1;
        l.zero = 0x80;
        let p = l.next_packet().expect("si legge").expect("c'è");
        assert_eq!(p.dur.get(), PACCHETTO);
        assert_eq!(p.data.len(), 4_096);
        assert!(p.data.iter().all(|b| *b == 0x80));
    }

    #[test]
    fn un_ape_troncato_finisce_dove_finisce_l_audio() {
        // I byte del terzo tratto non ci sono: il flusso finisce dopo il
        // secondo, normalmente, e non con un errore.
        let mut finto = Finto::nuovo(10_000, 35_000);
        finto.troncato_da = Some(2);
        let decodifiche = Arc::clone(&finto.decodifiche);
        let mut l = lettore(finto);
        let campioni = tutto(&mut l, 0);
        assert_eq!(campioni.len(), 20_000);
        for (n, c) in campioni.iter().enumerate() {
            assert_eq!(*c, della_rampa(u64::try_from(n).expect("sta")));
        }
        // Chi chiede ancora sente la fine senza un'altra lettura: su una
        // condivisione sarebbe una richiesta per niente a ogni giro.
        let prima = decodifiche.load(Ordering::Relaxed);
        assert!(l.next_packet().expect("si legge").is_none());
        assert!(l.next_packet().expect("si legge").is_none());
        assert_eq!(decodifiche.load(Ordering::Relaxed), prima);

        // I brani d'immagine che cominciano lì dentro: vuoti, e non un errore
        // ripetuto a ognuno.
        for punto in [25_000, 31_000] {
            salta(&mut l, punto);
            assert!(l.next_packet().expect("non è un errore").is_none());
        }

        // E tornare indietro ritrova la musica che c'è.
        salta(&mut l, 15_000);
        assert_eq!(tutto(&mut l, 15_000).len(), 5_000);
    }

    #[test]
    fn la_rete_che_cade_passa_com_era_e_non_salta_il_tratto() {
        // Un errore di lettura vero non è un tratto rovinato: sale com'era —
        // col suo numero di sistema, che `decodifica` riconosce come rete — e
        // il tratto resta da leggere, punto del salto compreso.
        let mut finto = Finto::nuovo(10_000, 30_000);
        finto.rete_una_volta = Some(1);
        let mut l = lettore(finto);
        salta(&mut l, 14_000);
        let Err(Error::IoError(io)) = l.next_packet() else {
            panic!("la caduta della rete deve salire com'è");
        };
        assert_eq!(io.raw_os_error(), Some(64));
        let resto = tutto(&mut l, 14_000);
        assert_eq!(resto.len(), 16_000);
        assert_eq!(resto[0], della_rampa(14_000), "il salto non si è perso");
    }
}
