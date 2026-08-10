//! Da un file a campioni pronti per la scheda audio.
//!
//! Due lavori in uno, e il secondo è quello che non si vede: decodificare, e
//! **ricampionare**. Il dispositivo si apre a una frequenza sola — su Windows in
//! modalità condivisa la decide il sistema, ed è quasi sempre 48000 — mentre i
//! file hanno la loro. La libreria vera di questo progetto è 1421 file a 44100:
//! senza ricampionamento non uscirebbe un suono giusto da nessuno di loro.
//!
//! E soprattutto: due brani a frequenze diverse non potrebbero attaccarsi senza
//! buco, perché il flusso che arriva alla scheda deve avere una frequenza sola
//! dall'inizio alla fine della sessione, non una per brano.

use std::collections::VecDeque;

use aether_domain::errors::{AppError, ErrorCode};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions};
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

use crate::Codec;

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
pub trait Flusso: std::io::Read + std::io::Seek + Send + Sync {
    /// Quanti byte, se si sanno.
    fn lunghezza(&self) -> Option<u64> {
        None
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
    /// Il guadagno ReplayGain del brano, se il file ne aveva uno.
    pub replaygain_db: Option<f32>,
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

/// Da [`Flusso`] al `MediaSource` di symphonia.
///
/// Il punto in cui il tratto pubblico di questo crate incontra quello della
/// libreria che ci sta sotto, e l'unico: cambiare decodificatore vuol dire
/// riscrivere questa struttura, non l'interfaccia.
struct Ponte {
    interno: Box<dyn Flusso>,
    lunghezza: Option<u64>,
}

impl std::io::Read for Ponte {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.interno.read(buf)
    }
}

impl std::io::Seek for Ponte {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.interno.seek(pos)
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

/// Quanti fotogrammi alla volta entrano nel ricampionatore.
const BLOCCO: usize = 1024;

/// Un brano aperto, che consegna campioni alla frequenza dell'uscita.
pub struct Decodificatore {
    track_id: i64,
    formato: Box<dyn FormatReader>,
    decodificatore: Box<dyn Decoder>,
    /// L'identificativo che symphonia dà alla traccia dentro il contenitore.
    traccia: u32,
    canali_sorgente: usize,
    canali_uscita: usize,
    frequenza_sorgente: u32,
    ricampionatore: Option<Ricampionatore>,
    /// I fotogrammi consegnati finora, alla frequenza dell'uscita.
    consegnati: u64,
    esaurito: bool,
}

impl Decodificatore {
    /// Apre una sorgente e la prepara per la frequenza e i canali richiesti.
    pub fn apri(sorgente: Sorgente, frequenza: u32, canali: u16) -> Result<Self, AppError> {
        let track_id = sorgente.track_id;
        let estensione = sorgente.estensione.clone();
        let formato_dichiarato = || estensione.clone();

        // Un formato che sappiamo di non saper suonare si dichiara subito, con
        // il codice che esiste apposta nel catalogo. Lasciarlo arrivare al
        // riconoscitore darebbe un «decodifica fallita» generico, che manda a
        // cercare il guasto nel posto sbagliato.
        if let Some(ext) = sorgente.estensione.as_deref() {
            if Codec::da_estensione(ext) == Codec::NonSupportato {
                return Err(AppError::new(ErrorCode::PlaybackFormatUnsupported {
                    format: Some(ext.to_owned()),
                }));
            }
        }

        let flusso = MediaSourceStream::new(
            Box::new(Ponte {
                lunghezza: sorgente.media.lunghezza(),
                interno: sorgente.media,
            }),
            symphonia::core::io::MediaSourceStreamOptions::default(),
        );
        let mut suggerimento = Hint::new();
        if let Some(ext) = sorgente.estensione.as_deref() {
            suggerimento.with_extension(ext);
        }

        let riconosciuto = symphonia::default::get_probe()
            .format(
                &suggerimento,
                flusso,
                &FormatOptions {
                    // Il gapless di symphonia toglie il silenzio di padding che
                    // gli encoder MP3 mettono in testa e in coda. Senza, fra due
                    // brani di uno stesso album resta un buco che nel file non
                    // c'è — e su un disco che è pensato senza stacchi si sente.
                    enable_gapless: true,
                    ..FormatOptions::default()
                },
                &MetadataOptions::default(),
            )
            .map_err(|err| {
                AppError::new(ErrorCode::PlaybackFormatUnsupported {
                    format: formato_dichiarato(),
                })
                .with_cause(err.to_string())
            })?;

        let formato = riconosciuto.format;
        let traccia = formato
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or_else(|| {
                AppError::new(ErrorCode::PlaybackFormatUnsupported {
                    format: formato_dichiarato(),
                })
                .with_cause("nessuna traccia audio nel contenitore".to_owned())
            })?;

        let parametri = traccia.codec_params.clone();
        let numero_traccia = traccia.id;

        let decodificatore = symphonia::default::get_codecs()
            .make(&parametri, &DecoderOptions::default())
            .map_err(|err| {
                AppError::new(ErrorCode::PlaybackFormatUnsupported {
                    format: formato_dichiarato(),
                })
                .with_cause(err.to_string())
            })?;

        let frequenza_sorgente = parametri.sample_rate.ok_or_else(|| {
            AppError::new(ErrorCode::PlaybackDecodeFailed {
                track_id: Some(track_id),
                format: formato_dichiarato(),
            })
            .with_cause("frequenza di campionamento non dichiarata".to_owned())
        })?;
        let canali_sorgente = parametri.channels.map_or(2, |c| c.count()).max(1);
        let canali_uscita = usize::from(canali).max(1);

        let ricampionatore = if frequenza_sorgente == frequenza {
            None
        } else {
            Some(Ricampionatore::nuovo(
                frequenza_sorgente,
                frequenza,
                canali_sorgente,
                track_id,
            )?)
        };

        Ok(Self {
            track_id,
            formato,
            decodificatore,
            traccia: numero_traccia,
            canali_sorgente,
            canali_uscita,
            frequenza_sorgente,
            ricampionatore,
            consegnati: 0,
            esaurito: false,
        })
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

    /// Si riposiziona. La coda interna del ricampionatore va buttata.
    pub fn cerca(&mut self, ms: u64) -> Result<(), AppError> {
        let secondi = ms_in_secondi(ms);
        self.formato
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::from(secondi),
                    track_id: Some(self.traccia),
                },
            )
            .map_err(|err| {
                AppError::new(ErrorCode::PlaybackDecodeFailed {
                    track_id: Some(self.track_id),
                    format: None,
                })
                .with_cause(err.to_string())
            })?;
        // Lo stato interno del decodificatore contiene fotogrammi che
        // appartengono al punto di prima: tenerli produrrebbe uno schiocco.
        self.decodificatore.reset();
        if let Some(r) = self.ricampionatore.as_mut() {
            r.svuota();
        }
        self.esaurito = false;
        self.consegnati = fotogrammi_da_ms(ms, self.frequenza_uscita());
        Ok(())
    }

    const fn frequenza_uscita(&self) -> u32 {
        match self.ricampionatore.as_ref() {
            Some(r) => r.frequenza_uscita,
            None => self.frequenza_sorgente,
        }
    }

    /// Il blocco successivo, interlacciato e pronto per la scheda.
    ///
    /// `false` quando il brano è finito. Un pacchetto illeggibile in mezzo non
    /// interrompe: si salta e si va avanti, perché un settore rovinato a metà
    /// canzone deve costare un fruscio, non la fine dell'ascolto. Un errore di
    /// lettura vero invece termina.
    pub fn prossimo(&mut self, fuori: &mut Vec<f32>) -> Result<bool, AppError> {
        fuori.clear();
        if self.esaurito {
            return Ok(false);
        }
        loop {
            let pacchetto = match self.formato.next_packet() {
                Ok(p) => p,
                Err(symphonia::core::errors::Error::IoError(err))
                    if err.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    // Fine del flusso: quel che resta nel ricampionatore va
                    // consegnato, altrimenti si perdono gli ultimi millisecondi
                    // di ogni brano — e su un album senza stacchi si sente.
                    self.esaurito = true;
                    if let Some(r) = self.ricampionatore.as_mut() {
                        r.coda(fuori, self.canali_uscita)?;
                    }
                    self.consegnati = self
                        .consegnati
                        .saturating_add(fotogrammi_in(fuori, self.canali_uscita));
                    return Ok(!fuori.is_empty());
                }
                Err(err) => {
                    self.esaurito = true;
                    return Err(AppError::new(ErrorCode::PlaybackDecodeFailed {
                        track_id: Some(self.track_id),
                        format: None,
                    })
                    .with_cause(err.to_string()));
                }
            };

            if pacchetto.track_id() != self.traccia {
                continue;
            }

            let decodificato = match self.decodificatore.decode(&pacchetto) {
                Ok(d) => d,
                Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                Err(err) => {
                    self.esaurito = true;
                    return Err(AppError::new(ErrorCode::PlaybackDecodeFailed {
                        track_id: Some(self.track_id),
                        format: None,
                    })
                    .with_cause(err.to_string()));
                }
            };

            let spec = *decodificato.spec();
            let quanti = decodificato.frames();
            if quanti == 0 {
                continue;
            }
            let mut interlacciato =
                SampleBuffer::<f32>::new(u64::try_from(quanti).unwrap_or(0), spec);
            interlacciato.copy_interleaved_ref(decodificato);

            match self.ricampionatore.as_mut() {
                Some(r) => r.spingi(
                    interlacciato.samples(),
                    self.canali_sorgente,
                    fuori,
                    self.canali_uscita,
                )?,
                None => adatta_canali(
                    interlacciato.samples(),
                    self.canali_sorgente,
                    self.canali_uscita,
                    fuori,
                ),
            }

            if !fuori.is_empty() {
                self.consegnati = self
                    .consegnati
                    .saturating_add(fotogrammi_in(fuori, self.canali_uscita));
                return Ok(true);
            }
        }
    }
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

/// I millisecondi in secondi, senza perdere la parte frazionaria.
fn ms_in_secondi(ms: u64) -> f64 {
    // `u32` per stare dentro la conversione esatta verso `f64`; sono più di
    // milletrecento ore, cioè oltre qualunque brano.
    f64::from(u32::try_from(ms).unwrap_or(u32::MAX)) / 1000.0
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

/// Porta un blocco interlacciato dal numero di canali della sorgente a quello
/// dell'uscita.
///
/// Mono verso stereo si duplica; il resto si tronca o si ripete. Non è un
/// downmix vero — sommare i canali di un 5.1 richiede i coefficienti giusti,
/// e sbagliarli è peggio che non farlo — ma nessun file di questa libreria ha
/// più di due canali, e il caso che conta davvero è il mono che deve uscire da
/// entrambe le casse invece che da una sola.
fn adatta_canali(campioni: &[f32], da: usize, a: usize, fuori: &mut Vec<f32>) {
    if da == 0 || a == 0 {
        return;
    }
    if da == a {
        fuori.extend_from_slice(campioni);
        return;
    }
    for blocco in campioni.chunks_exact(da) {
        for canale in 0..a {
            // Meno canali in ingresso che in uscita: si ripete l'ultimo (mono
            // su due casse). Di più: si prendono i primi.
            let preso = blocco.get(canale.min(da.saturating_sub(1))).copied();
            fuori.push(preso.unwrap_or(0.0));
        }
    }
}

/// Il ricampionatore, con le sue code per canale.
///
/// Rubato lavora a blocchi di dimensione fissa e su dati **per canale**, mentre
/// il decodificatore consegna blocchi di lunghezza variabile e interlacciati.
/// Questa struttura è il traduttore fra le due forme: accumula, converte quando
/// ha abbastanza roba, e conserva il resto per la volta dopo.
struct Ricampionatore {
    interno: rubato::FftFixedIn<f32>,
    frequenza_uscita: u32,
    /// Una coda per canale, in attesa di fare un blocco intero.
    code: Vec<VecDeque<f32>>,
    /// Buffer riusati, per non allocare a ogni blocco.
    ingresso: Vec<Vec<f32>>,
    uscita: Vec<Vec<f32>>,
    track_id: i64,
}

impl Ricampionatore {
    fn nuovo(da: u32, a: u32, canali: usize, track_id: i64) -> Result<Self, AppError> {
        let interno = rubato::FftFixedIn::<f32>::new(
            usize::try_from(da).unwrap_or(44_100),
            usize::try_from(a).unwrap_or(48_000),
            BLOCCO,
            2,
            canali,
        )
        .map_err(|err| {
            AppError::new(ErrorCode::PlaybackDecodeFailed {
                track_id: Some(track_id),
                format: None,
            })
            .with_cause(format!("ricampionatore {da}→{a}: {err}"))
        })?;
        let uscita = rubato::Resampler::output_buffer_allocate(&interno, true);
        Ok(Self {
            interno,
            frequenza_uscita: a,
            code: vec![VecDeque::new(); canali],
            ingresso: vec![vec![0.0; BLOCCO]; canali],
            uscita,
            track_id,
        })
    }

    fn svuota(&mut self) {
        for c in &mut self.code {
            c.clear();
        }
    }

    /// Accumula un blocco interlacciato e consegna quel che ne esce.
    fn spingi(
        &mut self,
        campioni: &[f32],
        canali_sorgente: usize,
        fuori: &mut Vec<f32>,
        canali_uscita: usize,
    ) -> Result<(), AppError> {
        if canali_sorgente == 0 {
            return Ok(());
        }
        for blocco in campioni.chunks_exact(canali_sorgente) {
            for (canale, coda) in self.code.iter_mut().enumerate() {
                coda.push_back(blocco.get(canale).copied().unwrap_or(0.0));
            }
        }
        while self.code.first().is_some_and(|c| c.len() >= BLOCCO) {
            for (coda, piano) in self.code.iter_mut().zip(self.ingresso.iter_mut()) {
                for posto in piano.iter_mut() {
                    *posto = coda.pop_front().unwrap_or(0.0);
                }
            }
            self.converti(false, fuori, canali_uscita)?;
        }
        Ok(())
    }

    /// Consegna quel che resta nelle code alla fine del brano.
    fn coda(&mut self, fuori: &mut Vec<f32>, canali_uscita: usize) -> Result<(), AppError> {
        let resto = self.code.first().map_or(0, VecDeque::len);
        if resto == 0 {
            return Ok(());
        }
        for (coda, piano) in self.code.iter_mut().zip(self.ingresso.iter_mut()) {
            for posto in piano.iter_mut() {
                *posto = coda.pop_front().unwrap_or(0.0);
            }
        }
        self.converti(true, fuori, canali_uscita)
    }

    fn converti(
        &mut self,
        parziale: bool,
        fuori: &mut Vec<f32>,
        canali_uscita: usize,
    ) -> Result<(), AppError> {
        use rubato::Resampler as _;
        let esito = if parziale {
            self.interno
                .process_partial_into_buffer(Some(&self.ingresso), &mut self.uscita, None)
        } else {
            self.interno
                .process_into_buffer(&self.ingresso, &mut self.uscita, None)
        };
        let (_, prodotti) = esito.map_err(|err| {
            AppError::new(ErrorCode::PlaybackDecodeFailed {
                track_id: Some(self.track_id),
                format: None,
            })
            .with_cause(err.to_string())
        })?;
        // Da per-canale a interlacciato, adattando il numero di canali.
        for fotogramma in 0..prodotti {
            for canale in 0..canali_uscita {
                let sorgente = canale.min(self.uscita.len().saturating_sub(1));
                let valore = self
                    .uscita
                    .get(sorgente)
                    .and_then(|piano| piano.get(fotogramma))
                    .copied()
                    .unwrap_or(0.0);
                fuori.push(valore);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn il_mono_esce_da_entrambe_le_casse() {
        let mut fuori = Vec::new();
        adatta_canali(&[0.5, -0.5], 1, 2, &mut fuori);
        assert_eq!(fuori, vec![0.5, 0.5, -0.5, -0.5]);
    }

    #[test]
    fn stereo_su_stereo_non_tocca_niente() {
        let mut fuori = Vec::new();
        adatta_canali(&[1.0, 2.0, 3.0, 4.0], 2, 2, &mut fuori);
        assert_eq!(fuori, vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn piu_canali_in_ingresso_si_troncano() {
        let mut fuori = Vec::new();
        adatta_canali(&[1.0, 2.0, 3.0], 3, 2, &mut fuori);
        assert_eq!(fuori, vec![1.0, 2.0]);
    }

    #[test]
    fn zero_canali_non_esplode() {
        let mut fuori = Vec::new();
        adatta_canali(&[1.0], 0, 2, &mut fuori);
        adatta_canali(&[1.0], 2, 0, &mut fuori);
        assert!(fuori.is_empty());
    }

    #[test]
    fn i_fotogrammi_di_una_durata() {
        assert_eq!(fotogrammi_da_ms(1_000, 48_000), 48_000);
        assert_eq!(fotogrammi_da_ms(0, 44_100), 0);
    }

    #[test]
    fn i_millisecondi_in_secondi_tengono_la_frazione() {
        assert!((ms_in_secondi(1_500) - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    #[expect(clippy::integer_division, reason = "campioni stereo in fotogrammi")]
    fn il_ricampionatore_cambia_la_lunghezza_nel_verso_giusto() {
        let mut r = Ricampionatore::nuovo(44_100, 48_000, 2, 1).expect("costruito");
        let mut fuori = Vec::new();
        // Un secondo di silenzio a 44100, stereo.
        let ingresso = vec![0.0f32; 44_100 * 2];
        r.spingi(&ingresso, 2, &mut fuori, 2).expect("spinto");
        r.coda(&mut fuori, 2).expect("coda");
        let fotogrammi = fuori.len() / 2;
        // Circa 48000, a meno del ritardo del filtro.
        assert!(
            (47_000..=49_000).contains(&fotogrammi),
            "fotogrammi prodotti: {fotogrammi}"
        );
    }
}
