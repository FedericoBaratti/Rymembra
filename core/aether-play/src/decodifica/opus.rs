//! Il decodificatore Opus, che symphonia non ha.
//!
//! # Perché è un modulo nostro e non una feature di symphonia
//!
//! Di Ogg Opus symphonia 0.5 sa quasi tutto: il suo demultiplatore riconosce la
//! firma `OpusHead`, misura la durata di ogni pacchetto leggendone il TOC, e —
//! col gapless acceso, come lo accende [`super::Decodificatore::apri`] — dice
//! quanto togliere in testa e in coda. Quel che non ha è chi trasformi quei
//! pacchetti in campioni. Manca il codec, non il contenitore: per questo qui
//! non si scrive un lettore di file, ma solo un [`Decoder`] da mettere nel
//! registro accanto a quelli che symphonia porta già.
//!
//! # Le tre cose che symphonia legge e non usa
//!
//! Il mappatore Ogg legge l'intestazione intera, e poi tre delle cose che ci ha
//! trovato non arrivano a chi decodifica:
//!
//! - il **guadagno d'uscita**, che RFC 7845 § 5.1 obbliga il lettore ad
//!   applicare. È il campo in cui finisce la normalizzazione R128 quando un
//!   file viene riguadagnato senza ricodificarlo: ignorarlo vuol dire suonare
//!   un brano al volume sbagliato senza avere modo di accorgersene;
//! - la **famiglia di mappatura dei canali**, che dice se i canali sono un
//!   flusso solo o più flussi da ricomporre;
//! - il **pre-skip**, che è il caso più insidioso dei tre, perché sembra
//!   gestito e non lo è. Vedi [`DecodificatoreOpus::da_saltare`].
//!
//! Stanno tutti e tre nell'`OpusHead`, che il mappatore ci consegna intero in
//! `extra_data`. Si rileggono da lì, che è l'unica fonte che non passa da
//! nessuna euristica.
//!
//! # Perché `opus-pure` e non uno degli altri due
//!
//! Perché è l'unico dei tre decodificatori Opus in Rust che regga i file veri.
//! Provati tutti e tre sullo stesso mazzo di quindici file incisi da libopus —
//! tono e rumore, mono e stereo, da 16 a 128 kbit/s:
//!
//! - `opus-decoder` 0.1.1 **va in panico** su cinque di quindici, tutti quelli
//!   a bitrate basso, con uno scorrimento in overflow dentro la quantizzazione
//!   vettoriale di CELT. In `debug` è un panico; in `release`, dove i controlli
//!   di overflow sono spenti, sarebbe silenziosamente il numero sbagliato — che
//!   è peggio. E i bitrate bassi non sono un caso di laboratorio: sono la voce,
//!   i podcast, e quasi tutto l'Opus che gira in rete;
//! - `rusty-opus` 0.9.1 non va in panico e restituisce spazzatura: dichiara sei
//!   volte i campioni che ci sono, e quel che ne esce ha valore efficace zero;
//! - `opus-pure` 0.2.1 li decodifica tutti e quindici, e il valore efficace che
//!   ne ricava coincide con quello che libopus ricava dallo stesso file.
//!
//! Il prezzo della scelta è che `opus-pure` non è «senza `unsafe`»: ne ha una
//! settantina di blocchi, tutti per gli intrinseci SIMD di AVX2 e NEON. Non è C
//! e non è FFI — niente cmake, niente libreria di sistema, e il giorno che
//! questo crate girerà su Android cross-compila senza NDK — ma la riga
//! `unsafe_code = "forbid"` del workspace vale sul codice nostro, non sulle
//! dipendenze, e vale la pena dirlo invece di lasciar credere altro.
//!
//! # Cosa non è provato
//!
//! La famiglia 1 — più flussi, cioè il surround — ha il suo ramo qui sotto e
//! **nessun campione che la eserciti**: i codificatori scrivono famiglia 0 per
//! mono e stereo, che è tutto quel che si riesce a mettere in un file di prova
//! abbastanza piccolo da stare in un repository. Il ramo è scritto secondo
//! RFC 7845 § 5.1.1, non verificato.

use opus_pure::{MAX_PACKET_SAMPLES, OpusDecoder, OpusMSDecoder, SoftClip};
use symphonia::core::audio::{AsAudioBufferRef, AudioBuffer, AudioBufferRef, Signal, SignalSpec};
use symphonia::core::codecs::{
    CODEC_TYPE_OPUS, CodecDescriptor, CodecParameters, Decoder, DecoderOptions, FinalizeResult,
};
use symphonia::core::errors::{Error, Result, unsupported_error};
use symphonia::core::formats::Packet;
use symphonia::core::support_codec;

/// La frequenza a cui Opus decodifica, qualunque cosa dica il file.
///
/// Non è una preferenza nostra: Opus **è** un codec a 48 kHz. Il campo
/// `input_sample_rate` dell'intestazione ricorda dove stava la sorgente prima
/// di essere codificata — un'informazione per chi archivia, non per chi suona —
/// e RFC 7845 § 5.1 dice espressamente di non usarlo per decidere la frequenza
/// d'uscita. Il mappatore Ogg dichiara infatti 48000 nei `CodecParameters`, e
/// portare il flusso dove serve al dispositivo è mestiere del ricampionatore.
const FREQUENZA: u32 = 48_000;

/// Chi decodifica davvero, nelle due forme che la norma prevede.
///
/// La differenza non è un dettaglio di libreria: un pacchetto di famiglia 1
/// contiene più flussi Opus incollati, e darlo al decodificatore semplice non
/// darebbe un errore — darebbe rumore.
enum Motore {
    /// Famiglia 0: un flusso solo, mono o stereo. È quel che scrivono i
    /// codificatori per la musica normale.
    Unico(Box<OpusDecoder>),
    /// Famiglia 1: più flussi, ricomposti secondo la mappatura dichiarata.
    Molti(Box<OpusMSDecoder>),
}

impl Motore {
    /// Decodifica un pacchetto in campioni interlacciati.
    ///
    /// Restituisce quanti fotogrammi per canale sono usciti.
    fn decodifica(&mut self, pacchetto: &[u8], fuori: &mut [f32]) -> Result<usize> {
        let esito = match self {
            Self::Unico(d) => d.decode(pacchetto, MAX_PACKET_SAMPLES, fuori),
            Self::Molti(d) => d.decode(pacchetto, MAX_PACKET_SAMPLES, fuori),
        };
        // Un pacchetto rotto esce come `DecodeError` e non come guasto del
        // flusso, ed è la distinzione che tiene in piedi il ripiego di
        // `prossimo_blocco`: là un `DecodeError` fa saltare il pacchetto e
        // andare avanti, mentre ogni altro errore ferma il brano. Un file con
        // una pagina corrotta a metà si ascolta fino in fondo con un buco,
        // invece di fermarsi lì.
        esito.map_err(|_| Error::DecodeError("opus: pacchetto illeggibile"))
    }

    /// Dimentica lo stato: si è saltato, e il passato non aiuta più.
    ///
    /// L'esito si scarta perché `Decoder::reset` non ne ha uno: un azzeramento
    /// che non riesce lascia il decodificatore com'era, e quel che ne segue —
    /// qualche millisecondo di riallineamento dopo il salto — è esattamente
    /// quel che succede comunque a un codec con stato.
    fn azzera(&mut self) {
        let _ = match self {
            Self::Unico(d) => d.reset_state(),
            Self::Molti(d) => d.reset_state(),
        };
    }
}

/// L'`OpusHead`, per quel che ce ne serve.
struct Intestazione {
    /// Quanti canali escono dal decodificatore.
    canali: usize,
    /// I fotogrammi di avviamento da buttare in testa al flusso.
    pre_skip: u64,
    /// Il guadagno d'uscita, già convertito in fattore lineare.
    guadagno: f32,
    /// Come sono disposti i flussi dentro ogni pacchetto.
    motore: Motore,
}

/// Legge l'`OpusHead` che il mappatore Ogg ci ha passato in `extra_data`.
///
/// # Perché con `get` e mai con le parentesi quadre
///
/// `indexing_slicing` è vietato in questo workspace, e qui la ragione si vede a
/// occhio nudo: questi byte arrivano dal file di chiunque, la lunghezza la
/// dichiara il file stesso, e un'intestazione tronca è uno dei modi più banali
/// di rompere un Ogg. Con le quadre sarebbe un panico nel filo che decodifica;
/// con `get` è un formato che non si sa aprire, detto a chi ha premuto play.
fn leggi_intestazione(testa: &[u8]) -> Result<Intestazione> {
    // Il mappatore Ogg ha già rifiutato tutto ciò che non comincia così, ma
    // questo decodificatore lo può costruire chiunque abbia dei
    // `CodecParameters`, e fidarsi di una guardia che sta altrove è il modo in
    // cui le guardie si perdono.
    if testa.get(..8) != Some(&b"OpusHead"[..]) {
        return unsupported_error("opus: intestazione senza firma");
    }

    let Some(&canali) = testa.get(9) else {
        return unsupported_error("opus: intestazione senza il numero dei canali");
    };
    let canali = usize::from(canali);
    if canali == 0 {
        return unsupported_error("opus: intestazione con zero canali");
    }

    // Il pre-skip: sedici bit senza segno, in fotogrammi a 48 kHz.
    let (Some(&ps_basso), Some(&ps_alto)) = (testa.get(10), testa.get(11)) else {
        return unsupported_error("opus: intestazione senza pre-skip");
    };
    let pre_skip = u64::from(u16::from_le_bytes([ps_basso, ps_alto]));

    // Il guadagno d'uscita: sedici bit con segno, in dB a virgola fissa Q7.8.
    // `10^(g / 5120)` è `10^((g / 256) / 20)` — la solita conversione da dB a
    // fattore, col 256 della virgola fissa già dentro.
    let (Some(&basso), Some(&alto)) = (testa.get(16), testa.get(17)) else {
        return unsupported_error("opus: intestazione senza guadagno");
    };
    let guadagno = 10.0_f32.powf(f32::from(i16::from_le_bytes([basso, alto])) / 5120.0);

    let Some(&famiglia) = testa.get(18) else {
        return unsupported_error("opus: intestazione senza famiglia di mappatura");
    };

    // `opus-pure` vuole la frequenza con segno. È una costante di cinque cifre:
    // non è una conversione a rischio, è un cambio di tipo.
    let frequenza = FREQUENZA as i32;

    let motore = match famiglia {
        // Famiglia 0: un flusso, e i canali possono essere solo uno o due.
        0 => {
            if canali > 2 {
                return unsupported_error("opus: famiglia 0 con più di due canali");
            }
            Motore::Unico(Box::new(
                OpusDecoder::new(frequenza, canali)
                    .map_err(|_| Error::Unsupported("opus: decodificatore rifiutato"))?,
            ))
        }
        // Famiglia 1: la disposizione la ricava la libreria dalla famiglia e dal
        // numero dei canali, che è la stessa tabella che RFC 7845 § 5.1.1
        // impone al file di dichiarare.
        1 => Motore::Molti(Box::new(
            OpusMSDecoder::new(frequenza, canali, famiglia)
                .map_err(|_| Error::Unsupported("opus: mappatura dei flussi rifiutata"))?,
        )),
        // Le altre famiglie la norma le riserva, e dice di non suonarle.
        _ => return unsupported_error("opus: famiglia di mappatura riservata"),
    };

    Ok(Intestazione {
        canali,
        pre_skip,
        guadagno,
        motore,
    })
}

/// Il decodificatore Opus di Aether.
///
/// Si registra accanto a quelli di symphonia — vedi `super::REGISTRO` — e da lì
/// in poi è indistinguibile dagli altri: chi apre un brano non sa, e non deve
/// sapere, che questo codec arriva da un'altra libreria.
pub(crate) struct DecodificatoreOpus {
    /// Quel che il contenitore ha dichiarato, ripetuto a chi lo chiede.
    parametri: CodecParameters,
    /// Chi decodifica.
    motore: Motore,
    /// Quanti canali escono, per disinterlacciare.
    canali: usize,
    /// Il guadagno dell'intestazione, da applicare a ogni campione.
    guadagno: f32,
    /// Quanti fotogrammi di avviamento restano da buttare via.
    ///
    /// # Il pre-skip, e perché lo applichiamo noi
    ///
    /// Ogni file Opus comincia con qualche millisecondo che il codificatore ha
    /// prodotto solo per avviare i propri filtri: `OpusHead` dice quanti sono, e
    /// RFC 7845 § 4.2 dice di buttarli. Questo è un lavoro che nei contenitori
    /// fa il demultiplatore, e symphonia lo fa: con il gapless acceso mette in
    /// ogni pacchetto un `trim_start` e un `trim_end`, e il decodificatore Vorbis
    /// si limita a ubbidire.
    ///
    /// **Su Opus il `trim_start` arriva sempre a zero**, e non è un dettaglio
    /// teorico: misurato, un campione di tre secondi esatti ne consegnava
    /// 144312 invece di 144000 — i 312 fotogrammi di avviamento, in testa a ogni
    /// brano. La ragione è che symphonia non si fida del pre-skip che pure ha
    /// letto e prova a **dedurlo** dalle pagine, confrontando la posizione
    /// granulare della prima con la durata dei pacchetti che contiene. Su Vorbis
    /// quel conto torna; su Opus no, perché la posizione granulare di Ogg Opus
    /// conta *già* il pre-skip, i due numeri coincidono, e la deduzione conclude
    /// che non c'è niente da togliere. La coda invece si deduce bene, e infatti
    /// il `trim_end` funziona.
    ///
    /// Quindi il numero si rilegge dall'`OpusHead` — l'unica fonte che non passa
    /// da nessuna euristica — e si scala pacchetto per pacchetto. Chi salta a
    /// metà brano non ne paga niente: il contatore è già a zero da un pezzo, e
    /// dopo un [`Decoder::reset`] non si ricarica, perché un salto porta dentro
    /// il brano e non al suo inizio.
    da_saltare: u64,
    /// La curva che riporta i campioni dentro l'unità.
    morbido: SoftClip,
    /// I campioni interlacciati appena usciti dal codec.
    interlacciato: Vec<f32>,
    /// Gli stessi campioni divisi per canale, che è la forma di symphonia.
    fuori: AudioBuffer<f32>,
}

impl DecodificatoreOpus {
    /// Il lavoro vero di [`Decoder::decode`], separato per poter fallire senza
    /// lasciare nel buffer i resti di un pacchetto decodificato a metà.
    fn decodifica_dentro(&mut self, pacchetto: &Packet) -> Result<()> {
        self.fuori.clear();

        let fotogrammi = self
            .motore
            .decodifica(pacchetto.buf(), &mut self.interlacciato)?;
        if fotogrammi == 0 {
            return Ok(());
        }
        let quanti = fotogrammi.saturating_mul(self.canali);

        // Il guadagno dell'intestazione, e poi la curva. L'ordine conta.
        //
        // La decodifica in virgola mobile di Opus **non è limitata a ±1** — la
        // libreria lo dichiara, e libopus si comporta uguale — quindi un file
        // masterizzato al massimo torna indietro appena sopra l'unità, e il
        // guadagno dell'intestazione può alzarlo ancora. Lasciarlo passare
        // vorrebbe dire tagliarlo di netto più avanti, contro il fondo del
        // dispositivo. `SoftClip` è la stessa curva che libopus applica sulla
        // propria strada a interi: piega quel che sporge invece di squadrarlo.
        // Ha memoria fra un pacchetto e l'altro — la fine di una curva è
        // l'inizio della prossima — e per questo vive nella struttura.
        if let Some(campioni) = self.interlacciato.get_mut(..quanti) {
            if (self.guadagno - 1.0).abs() > f32::EPSILON {
                for campione in campioni.iter_mut() {
                    *campione *= self.guadagno;
                }
            }
            self.morbido.apply(campioni);
        }

        self.fuori.render_reserved(Some(fotogrammi));

        // Da interlacciato a per-canale, che è la forma in cui symphonia
        // consegna i campioni a chiunque.
        let canali = self.canali;
        for canale in 0..canali {
            let dentro = self.interlacciato.iter().skip(canale).step_by(canali);
            for (posto, campione) in self.fuori.chan_mut(canale).iter_mut().zip(dentro) {
                *posto = *campione;
            }
        }

        // Il taglio, che su Opus si fa in due mani.
        //
        // La coda la dice symphonia, come farebbe per Vorbis: `trim_end` porta
        // l'imbottitura che il codificatore ha aggiunto in fondo all'ultimo
        // pacchetto, e senza di lei un album senza stacchi ritroverebbe un buco
        // fra un brano e l'altro.
        //
        // La testa invece la contiamo noi, perché `trim_start` su Opus vale
        // sempre zero: il perché sta in [`Self::da_saltare`]. Si prende il
        // maggiore dei due, così il giorno che symphonia imparasse a dedurlo il
        // pre-skip non verrebbe tolto due volte.
        let dichiarato = usize::try_from(pacchetto.trim_start()).unwrap_or(usize::MAX);
        let quanti_ora = u64::try_from(fotogrammi).unwrap_or(u64::MAX);
        let nostro = usize::try_from(self.da_saltare.min(quanti_ora)).unwrap_or(0);
        self.da_saltare = self
            .da_saltare
            .saturating_sub(quanti_ora.min(self.da_saltare));

        let fine = usize::try_from(pacchetto.trim_end()).unwrap_or(usize::MAX);
        self.fuori.trim(dichiarato.max(nostro), fine);

        Ok(())
    }
}

impl Decoder for DecodificatoreOpus {
    fn try_new(parametri: &CodecParameters, _: &DecoderOptions) -> Result<Self> {
        if parametri.codec != CODEC_TYPE_OPUS {
            return unsupported_error("opus: non è un flusso Opus");
        }

        let Some(testa) = parametri.extra_data.as_ref() else {
            return unsupported_error("opus: manca l'intestazione OpusHead");
        };
        let Intestazione {
            canali,
            pre_skip,
            guadagno,
            motore,
        } = leggi_intestazione(testa)?;

        // La disposizione dei canali la calcola il mappatore Ogg dagli stessi
        // byte che abbiamo appena riletto, e la sua è più ricca della nostra:
        // sa dire *quali* canali sono, non solo quanti. Si usa quella, e si
        // controlla che i due conti coincidano — se non coincidessero, il
        // buffer avrebbe una forma e il decodificatore un'altra.
        let Some(disposizione) = parametri.channels else {
            return unsupported_error("opus: il contenitore non dice quali canali");
        };
        if disposizione.count() != canali {
            return unsupported_error("opus: contenitore e intestazione contano canali diversi");
        }

        // Il buffer si alloca su centoventi millisecondi — il pacchetto più
        // lungo che la norma ammetta — una volta sola, invece che pacchetto per
        // pacchetto: la libreria rifiuta con «buffer troppo piccolo» se il
        // posto non basta, e rifarne il conto a ogni pacchetto sarebbe un
        // secondo posto in cui sbagliarlo.
        let capienza = u64::try_from(MAX_PACKET_SAMPLES).unwrap_or(u64::MAX);

        Ok(Self {
            parametri: parametri.clone(),
            motore,
            canali,
            guadagno,
            da_saltare: pre_skip,
            morbido: SoftClip::new(canali),
            interlacciato: vec![0.0; MAX_PACKET_SAMPLES.saturating_mul(canali)],
            fuori: AudioBuffer::new(capienza, SignalSpec::new(FREQUENZA, disposizione)),
        })
    }

    fn supported_codecs() -> &'static [CodecDescriptor] {
        &[support_codec!(CODEC_TYPE_OPUS, "opus", "Opus")]
    }

    fn reset(&mut self) {
        self.motore.azzera();
        // Anche la curva: la sua memoria descrive come finiva il campione di
        // prima, e dopo un salto quel campione non c'è più.
        self.morbido.reset();
    }

    fn codec_params(&self) -> &CodecParameters {
        &self.parametri
    }

    fn decode(&mut self, pacchetto: &Packet) -> Result<AudioBufferRef<'_>> {
        match self.decodifica_dentro(pacchetto) {
            Ok(()) => Ok(self.fuori.as_audio_buffer_ref()),
            Err(guasto) => {
                self.fuori.clear();
                Err(guasto)
            }
        }
    }

    fn finalize(&mut self) -> FinalizeResult {
        FinalizeResult::default()
    }

    fn last_decoded(&self) -> AudioBufferRef<'_> {
        self.fuori.as_audio_buffer_ref()
    }
}
