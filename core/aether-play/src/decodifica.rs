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
use std::sync::LazyLock;

use aether_domain::errors::{AppError, ErrorCode};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CodecRegistry, Decoder, DecoderOptions};
use symphonia::core::errors::SeekErrorKind;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

use crate::Codec;

mod opus;

/// I codec che sappiamo costruire.
///
/// # Perché un registro nostro invece di `symphonia::default::get_codecs()`
///
/// Perché uno dei codec non è di symphonia. Opus lo decodifica
/// [`opus::DecodificatoreOpus`], e symphonia lo scoprirebbe solo se glielo si
/// dice: il registro predefinito è una costante della libreria, e `make()` su
/// un `CODEC_TYPE_OPUS` risponderebbe «non supportato» dopo che il
/// demultiplatore ha riconosciuto il flusso perfettamente.
///
/// Si costruisce una volta sola e per sempre: dentro ci sono tabelle di
/// descrittori, non stato, e ogni [`Decodificatore::apri`] che se lo rifacesse
/// pagherebbe la costruzione di tutta la catena di symphonia per ottenere la
/// stessa cosa.
static REGISTRO: LazyLock<CodecRegistry> = LazyLock::new(|| {
    let mut registro = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut registro);
    registro.register_all::<opus::DecodificatoreOpus>();
    registro
});

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
/// E in fondo alla catena c'è `SampleBuffer`, che di un conteggio a zero fa un
/// `assert` dentro symphonia — cioè un panico nel filo della decodifica.
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
    /// L'ultimo millisecondo a cui il flusso accetta di saltare.
    ///
    /// Da `n_frames`, non da `durata_ms`: sono due numeri diversi, e questo è
    /// quello che symphonia userà per dire di no.
    fine_flusso_ms: Option<u64>,
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
        if let Some(ext) = sorgente.estensione.as_deref()
            && Codec::da_estensione(ext) == Codec::NonSupportato
        {
            return Err(AppError::new(ErrorCode::PlaybackFormatUnsupported {
                format: Some(ext.to_owned()),
            }));
        }

        let guasto = Guasto::default();
        let flusso = MediaSourceStream::new(
            Box::new(Ponte {
                lunghezza: sorgente.media.lunghezza(),
                interno: sorgente.media,
                guasto: guasto.clone(),
            }),
            symphonia::core::io::MediaSourceStreamOptions::default(),
        );
        // Il ripiego di tutti e tre i modi in cui il contenitore può non
        // riconoscersi. Guarda **prima** se sotto c'era la rete, perché il
        // guasto di rete è ritentabile e questo no.
        let non_supportato = |causa: String| {
            guasto.di_rete().unwrap_or_else(|| {
                AppError::new(ErrorCode::PlaybackFormatUnsupported {
                    format: formato_dichiarato(),
                })
                .with_cause(causa)
            })
        };
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
            // Il riconoscimento del contenitore **legge**, e su un percorso di
            // rete quella lettura può fallire per la rete e non per il formato:
            // senza questo ramo un FLAC intero su una share appena caduta
            // veniva dichiarato «formato non supportato», che il catalogo
            // dichiara mai ritentabile — cioè niente «Riprova», per un file che
            // non ha niente che non va.
            .map_err(|err| se_di_rete(&err).unwrap_or_else(|| non_supportato(err.to_string())))?;

        let formato = riconosciuto.format;
        let traccia = formato
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or_else(|| non_supportato("nessuna traccia audio nel contenitore".to_owned()))?;

        let parametri = traccia.codec_params.clone();
        let numero_traccia = traccia.id;

        let decodificatore = REGISTRO
            .make(&parametri, &DecoderOptions::default())
            .map_err(|err| non_supportato(err.to_string()))?;

        let frequenza_sorgente = parametri.sample_rate.ok_or_else(|| {
            AppError::new(ErrorCode::PlaybackDecodeFailed {
                track_id: Some(track_id),
                format: formato_dichiarato(),
            })
            .with_cause("frequenza di campionamento non dichiarata".to_owned())
        })?;
        let canali_sorgente = parametri.channels.map_or(2, |c| c.count()).max(1);
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
        if !frequenza_accettabile(frequenza_sorgente) {
            return Err(AppError::new(ErrorCode::PlaybackDecodeFailed {
                track_id: Some(track_id),
                format: formato_dichiarato(),
            })
            .with_cause(format!(
                "frequenza dichiarata fuori scala: {frequenza_sorgente} Hz, \
                 attesa fra {FREQUENZA_MIN} e {FREQUENZA_MAX}"
            )));
        }
        if canali_sorgente > CANALI_MAX {
            return Err(AppError::new(ErrorCode::PlaybackDecodeFailed {
                track_id: Some(track_id),
                format: formato_dichiarato(),
            })
            .with_cause(format!(
                "canali dichiarati fuori scala: {canali_sorgente}, al massimo {CANALI_MAX}"
            )));
        }
        // Il tetto dei salti, e vale la pena dire da dove **non** viene:
        // `sorgente.durata_ms` è quel che il database ha registrato alla
        // scansione, e symphonia non l'ha mai vista. Chi rifiuta il salto
        // guarda `n_frames`, quindi il tetto va calcolato da lì.
        let fine_flusso_ms = fine_da_fotogrammi(parametri.n_frames, frequenza_sorgente);

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
            fine_flusso_ms,
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

    /// L'ultimo millisecondo a cui questo flusso accetta di saltare, quando il
    /// contenitore lo dichiara.
    #[must_use]
    pub const fn fine_flusso_ms(&self) -> Option<u64> {
        self.fine_flusso_ms
    }

    /// Si riposiziona. La coda interna del ricampionatore va buttata.
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
    /// Perché nessuno l'ha rotto: symphonia 0.5.5 rifiuta e basta, con
    /// `SeekError(OutOfRange)` quando `ts > n_frames` — FLAC, WAV, AIFF, Ogg,
    /// MP4 — e con `IoError(UnexpectedEof)` su MP3, dove non c'è un
    /// `n_frames` da confrontare e la scansione in avanti finisce i byte.
    /// Trascinare il cursore fino in fondo a un FLAC lo produceva **sempre**, e
    /// il ripiego lo raccontava come `playback.decodeFailed`: il catalogo lo
    /// dichiara mai ritentabile e il testo consiglia di sostituire il file. Qui
    /// invece il brano si dichiara finito, la coda avanza al giro dopo di
    /// [`Decodificatore::prossimo`], e nessuno manda l'utente a buttare un FLAC
    /// intatto.
    ///
    /// # L'ordine dei due riconoscimenti, che è la cosa da non sbagliare
    ///
    /// La rete si guarda **prima**. Una condivisione che muore proprio durante
    /// un salto arriva anche lei come `IoError`, e presa per «fine del brano»
    /// verrebbe inghiottita in silenzio: niente errore, niente «Riprova»,
    /// niente punto a cui tornare — e il brano successivo partirebbe come se
    /// il primo fosse solo finito prima. È il caso che difende
    /// `una_share_che_muore_durante_un_salto_non_dice_che_il_file_e_rotto` in
    /// `motore.rs`, e invertire i due rami la fa cadere.
    pub fn cerca(&mut self, ms: u64) -> Result<(), AppError> {
        let bersaglio = self.bersaglio(ms);
        let secondi = ms_in_secondi(bersaglio);
        // L'esito si lega prima: `self.formato` è preso in prestito mutabile
        // dalla `seek`, e chiamare `self.guasto` dentro il `map_err` vorrebbe
        // dire prenderlo una seconda volta.
        let esito = self.formato.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time: Time::from(secondi),
                track_id: Some(self.traccia),
            },
        );
        if let Err(err) = esito {
            // 1. La rete, che è ritentabile e non è una fine di brano.
            if let Some(rete) = se_di_rete(&err) {
                return Err(rete);
            }
            // 2. Il tetto non arriva ovunque — MP3 non dichiara `n_frames`, e
            //    nemmeno ogni contenitore lo fa: qui il rifiuto si accetta per
            //    quello che è, cioè un brano finito.
            if oltre_la_fine(&err) {
                self.a_fine_flusso(bersaglio);
                return Ok(());
            }
            // 3. Tutto il resto è davvero un guasto.
            return Err(self.guasto(&err));
        }
        // Lo stato interno del decodificatore contiene fotogrammi che
        // appartengono al punto di prima: tenerli produrrebbe uno schiocco.
        self.decodificatore.reset();
        if let Some(r) = self.ricampionatore.as_mut() {
            r.svuota();
        }
        self.esaurito = false;
        // Il bersaglio e non `ms`: la posizione che si riporta è quella a cui si
        // è atterrati, non quella che era stata chiesta. Scriverci `ms`
        // significherebbe un cursore fermo oltre la fine mentre l'audio suona
        // gli ultimi millisecondi.
        self.consegnati = fotogrammi_da_ms(bersaglio, self.frequenza_uscita());
        Ok(())
    }

    /// Dove si salta davvero, dato dove è stato chiesto di saltare.
    ///
    /// # Perché un margine, e non la fine esatta
    ///
    /// Perché `n_frames` è il conteggio del **contenitore**, e non è il numero
    /// di fotogrammi che questo decodificatore consegnerà: `enable_gapless`
    /// toglie il padding dell'encoder *dopo* che quel conteggio è stato letto.
    /// E chi rifiuta il salto non confronta sempre lo stesso numero — su Ogg
    /// symphonia guarda `n_frames + start_ts`, su MP3 si somma il `delay` per
    /// conto suo — quindi mirare all'ultimo fotogramma dichiarato è mirare a un
    /// bersaglio che si sposta.
    ///
    /// [`MARGINE_FINE_MS`] sta sotto la granularità che quel salto ha già: un
    /// blocco FLAC è 4096 fotogrammi, cioè circa 93 ms a 44,1 kHz, e senza
    /// `SEEKTABLE` si atterra comunque al confine del blocco. Centoventi
    /// millisecondi non spostano il punto in cui l'utente si ritrova; una fine
    /// esatta sbagliata di un fotogramma sì, perché diventa un errore.
    fn bersaglio(&self, ms: u64) -> u64 {
        match self.fine_flusso_ms {
            Some(fine) => ms.min(fine.saturating_sub(MARGINE_FINE_MS)),
            // Senza `n_frames` non c'è un tetto da applicare: nemmeno symphonia
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
        self.decodificatore.reset();
        if let Some(r) = self.ricampionatore.as_mut() {
            r.svuota();
        }
        // La fine dichiarata quando c'è: dire `bersaglio` su un salto molto
        // oltre la fine metterebbe il cursore fuori dalla barra.
        let fine = self.fine_flusso_ms.unwrap_or(bersaglio);
        self.consegnati = fotogrammi_da_ms(fine, self.frequenza_uscita());
    }

    const fn frequenza_uscita(&self) -> u32 {
        match self.ricampionatore.as_ref() {
            Some(r) => r.frequenza_uscita,
            None => self.frequenza_sorgente,
        }
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
    fn guasto(&self, err: &symphonia::core::errors::Error) -> AppError {
        se_di_rete(err).unwrap_or_else(|| {
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
                    return Err(self.guasto(&err));
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
                    return Err(self.guasto(&err));
                }
            };

            let spec = *decodificato.spec();
            let quanti = decodificato.frames();
            if quanti == 0 {
                continue;
            }
            // Il ripiego nel verso giusto. `quanti` è un `usize` e la capienza
            // un `u64`: su una macchina a 64 bit la conversione non fallisce
            // mai, ma se fallisse, ripiegare su **zero** vorrebbe dire chiedere
            // a symphonia un buffer che non può contenere il blocco appena
            // decodificato — e symphonia risponde con un `assert`, cioè con un
            // panico nel filo della decodifica. Un errore restituito qui è la
            // stessa notizia detta a chi la sa gestire.
            let Ok(capienza) = u64::try_from(quanti) else {
                self.esaurito = true;
                return Err(AppError::new(ErrorCode::PlaybackDecodeFailed {
                    track_id: Some(self.track_id),
                    format: None,
                })
                .with_cause(format!(
                    "blocco decodificato di {quanti} fotogrammi: non entra in un conteggio"
                )));
            };
            let mut interlacciato = SampleBuffer::<f32>::new(capienza, spec);
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
/// Perché symphonia 0.5.5 non ha un modo solo di dirlo. I contenitori che
/// dichiarano `n_frames` — FLAC, WAV, AIFF, Ogg, MP4 — confrontano e
/// restituiscono `SeekError(OutOfRange)`; MP3 non ha niente da confrontare,
/// quindi scandisce in avanti e finisce i byte, che arriva qui come
/// `IoError(UnexpectedEof)`. Sono lo stesso fatto — «il brano finisce prima» —
/// e chiamarli in due modi diversi vorrebbe dire lasciarne fuori uno.
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
/// `None` quando `n_frames` manca: allora non esiste un tetto da imporre,
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
    /// Costruisce il traduttore fra le due frequenze, se le due frequenze
    /// hanno senso.
    ///
    /// # Perché ricontrolla quel che l'apertura ha già controllato
    ///
    /// Perché il rifiuto di là guarda la frequenza del **file** e questo è il
    /// solo punto che veda anche quella dell'**uscita**, che arriva dal
    /// dispositivo e non dal file. E perché `FftFixedIn::new` alloca in
    /// funzione del rapporto fra le due: chi lo chiama deve poter contare sul
    /// fatto che i due numeri siano già stati guardati, senza doverselo
    /// ricordare.
    ///
    /// # Perché nessun ripiego
    ///
    /// Qui c'era `usize::try_from(da).unwrap_or(44_100)`: una conversione che
    /// non può fallire, con accanto un numero **inventato** per il caso in cui
    /// fallisse. Se mai fosse scattato, il ricampionatore avrebbe convertito da
    /// una frequenza che il file non ha — cioè avrebbe suonato il brano alla
    /// velocità sbagliata, in silenzio e per sempre, senza che niente da
    /// nessuna parte lo dicesse. Un errore restituito è la sola risposta onesta
    /// a un numero che non si sa convertire.
    ///
    /// # Errori
    ///
    /// `playback.decodeFailed` se una delle due frequenze è fuori scala, se i
    /// canali sono zero o troppi, o se rubato rifiuta la coppia.
    fn nuovo(da: u32, a: u32, canali: usize, track_id: i64) -> Result<Self, AppError> {
        let rifiuta = |causa: String| {
            AppError::new(ErrorCode::PlaybackDecodeFailed {
                track_id: Some(track_id),
                format: None,
            })
            .with_cause(causa)
        };
        if !frequenza_accettabile(da) || !frequenza_accettabile(a) {
            return Err(rifiuta(format!(
                "ricampionatore {da}→{a}: frequenza fuori scala, attesa fra \
                 {FREQUENZA_MIN} e {FREQUENZA_MAX}"
            )));
        }
        if canali == 0 || canali > CANALI_MAX {
            return Err(rifiuta(format!(
                "ricampionatore a {canali} canali: attesi fra 1 e {CANALI_MAX}"
            )));
        }
        let (Ok(ingresso_hz), Ok(uscita_hz)) = (usize::try_from(da), usize::try_from(a)) else {
            return Err(rifiuta(format!(
                "ricampionatore {da}→{a}: le frequenze non entrano in un conteggio"
            )));
        };
        let interno = rubato::FftFixedIn::<f32>::new(ingresso_hz, uscita_hz, BLOCCO, 2, canali)
            .map_err(|err| rifiuta(format!("ricampionatore {da}→{a}: {err}")))?;
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
    fn la_fine_del_flusso_dai_fotogrammi_dichiarati() {
        // Mezzo secondo a 44,1 kHz: il campione FLAC di `motore.rs`.
        assert_eq!(fine_da_fotogrammi(Some(22_050), 44_100), Some(500));
        assert_eq!(fine_da_fotogrammi(Some(48_000), 48_000), Some(1_000));
        // Senza `n_frames` non c'è tetto: è il caso dell'MP3, e il salto deve
        // partire com'è per finire nella rete di sicurezza di `cerca`.
        assert_eq!(fine_da_fotogrammi(None, 44_100), None);
        // Una frequenza a zero non deve dividere per zero.
        assert_eq!(fine_da_fotogrammi(Some(22_050), 0), None);
        // Un `n_frames` assurdo satura invece di traboccare: la moltiplicazione
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

    #[test]
    fn i_millisecondi_in_secondi_tengono_la_frazione() {
        assert!((ms_in_secondi(1_500) - 1.5).abs() < f64::EPSILON);
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

    /// Un WAV mono a 16 bit che dichiara la frequenza data.
    ///
    /// I campioni sono silenzio: quel che si prova è l'intestazione.
    fn wav_a(frequenza: u32, fotogrammi: usize) -> Vec<u8> {
        let dati = u32::try_from(fotogrammi * 2).unwrap_or(0);
        let mut byte = Vec::with_capacity(44 + fotogrammi * 2);
        byte.extend_from_slice(b"RIFF");
        byte.extend_from_slice(&(36 + dati).to_le_bytes());
        byte.extend_from_slice(b"WAVEfmt ");
        byte.extend_from_slice(&16u32.to_le_bytes());
        byte.extend_from_slice(&1u16.to_le_bytes()); // PCM
        byte.extend_from_slice(&1u16.to_le_bytes()); // un canale
        byte.extend_from_slice(&frequenza.to_le_bytes());
        byte.extend_from_slice(&frequenza.saturating_mul(2).to_le_bytes());
        byte.extend_from_slice(&2u16.to_le_bytes()); // allineamento
        byte.extend_from_slice(&16u16.to_le_bytes()); // bit per campione
        byte.extend_from_slice(b"data");
        byte.extend_from_slice(&dati.to_le_bytes());
        byte.extend(std::iter::repeat_n(0u8, fotogrammi * 2));
        byte
    }

    fn sorgente_wav(frequenza: u32) -> Sorgente {
        Sorgente {
            track_id: 1,
            media: Box::new(Byte(std::io::Cursor::new(wav_a(frequenza, 1_000)))),
            estensione: Some("wav".to_owned()),
            durata_ms: 0,
            replaygain_db: None,
        }
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
    fn una_frequenza_assurda_non_arriva_al_ricampionatore() {
        // Tre megahertz è il numero che esce da un'intestazione troncata, ed è
        // il caso caro: fra tre milioni e quarantottomila il massimo comun
        // divisore è piccolo, e `FftFixedIn::new` dimensiona i propri buffer su
        // quel rapporto. Prima costruiva il mostro e poi ci lavorava dentro.
        assert!(Ricampionatore::nuovo(3_000_000, 48_000, 2, 1).is_err());
        // Zero da una parte e dall'altra: la frequenza dell'uscita arriva dal
        // dispositivo, non dal file, ed è l'unico posto che la guardi.
        assert!(Ricampionatore::nuovo(0, 48_000, 2, 1).is_err());
        assert!(Ricampionatore::nuovo(44_100, 0, 2, 1).is_err());
        // E il caso normale continua a costruirsi: è tutta la libreria di
        // questo progetto.
        assert!(Ricampionatore::nuovo(44_100, 48_000, 2, 1).is_ok());
    }

    #[test]
    fn nessun_canale_e_troppi_canali_non_costruiscono_niente() {
        assert!(Ricampionatore::nuovo(44_100, 48_000, 0, 1).is_err());
        assert!(Ricampionatore::nuovo(44_100, 48_000, CANALI_MAX + 1, 1).is_err());
        // Il tetto sta sopra qualunque configurazione reale: il 22.2 giapponese
        // ne conta ventiquattro, e deve passare.
        assert!(Ricampionatore::nuovo(44_100, 48_000, 24, 1).is_ok());
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
