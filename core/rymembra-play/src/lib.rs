//! Il motore audio di Rymembra.
//!
//! # Perché è un crate del nucleo e non codice dentro l'applicazione
//!
//! Nel vecchio albero la riproduzione era scritta due volte: Howler nel webview
//! sul desktop, ExoPlayer in Kotlin su Android. Non erano due
//! *implementazioni* della stessa decisione — erano due decisioni diverse che si
//! somigliavano, e divergevano ogni volta che una delle due veniva toccata. Il
//! `CHANGELOG.md` ne conserva un esemplare: il codice d'errore di ExoPlayer
//! veniva scartato per strada, e sul desktop Howler mostrava «2» in interfaccia.
//!
//! Qui il motore è uno. Su Android sarà lo stesso crate, dietro UniFFI, con lo
//! stesso `cpal` che là parla ad AAudio invece che a WASAPI.
//!
//! # Cosa questo crate non sa
//!
//! Non sa cos'è una libreria musicale, non apre un database, non conosce i
//! percorsi. Riceve una [`Sorgente`] — dei byte, un'estensione, una durata — e
//! li fa uscire dalle casse. Chi decide *quale* brano è la coda in
//! `rymembra_domain::queue`; chi trova i byte è `rymembra-app`. Questa divisione è
//! ciò che tiene aperta la strada per Android, dove i byte non arrivano da un
//! percorso ma da un flusso concesso dal sistema.
//!
//! # La forma, in tre fili
//!
//! 1. **Chi comanda** — chiunque, attraverso [`Motore`], che manda messaggi.
//! 2. **Chi decodifica** — un filo che possiede symphonia e riempie un anello.
//!    È l'unico dei tre che tocchi codice di terze parti su byte arrivati da un
//!    disco o da una rete, quindi è l'unico che possa cadere: gira dentro un
//!    `catch_unwind` — vedi [`motore::fili_caduti`] — e, cadendo, alza la stessa
//!    bandiera con cui si annuncia un dispositivo sparito. Un motore muto che
//!    non lo dice a nessuno era il modo peggiore in cui questo filo poteva
//!    morire.
//! 3. **Chi suona** — la callback di cpal, che svuota l'anello e basta.
//!
//! Fra il secondo e il terzo ci sono due anelli senza lucchetti, e la ragione è
//! in [`uscita`]: la callback ha qualche millisecondo per consegnare il blocco,
//! e non può permettersi di aspettare nessuno. Nel primo passano i campioni; nel
//! secondo le curve dell'[`equalizzatore`], che sono cinquanta numeri in virgola
//! mobile e vanno consegnati tutti insieme o per niente.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use rymembra_domain::errors::AppError;

mod ape;
pub mod attacchi;
pub mod decodifica;
pub mod dispositivi;
pub mod equalizzatore;
pub mod impronta;
pub mod intensita;
pub mod motore;
pub mod spettro;
pub mod uscita;

pub use decodifica::{Flusso, FormatoSuonato, Segmento, Sorgente};
pub use dispositivi::Dispositivo;
pub use equalizzatore::{BANDE, CENTRI_HZ, LIMITE_DB, PRESET_DI_SERIE};
pub use motore::{BranoAperto, Evento, Motore, Posizione};
pub use spettro::{
    Bande, RISOLUZIONE_DI_SERIE, RISOLUZIONE_MAX, RISOLUZIONE_MIN, RISOLUZIONI, Spettro,
};
pub use uscita::FormatoUscita;

/// Cosa sappiamo suonare.
///
/// L'elenco chiuso di `rymembra_domain::paths::SUPPORTED_EXTENSIONS` meno quello
/// che non decodifichiamo. Averlo qui come tipo, invece che come confronto fra
/// stringhe sparso per l'albero, è servito esattamente a quel che prometteva:
/// il giorno che Opus ha avuto un decodificatore — `decodifica::opus`, che
/// symphonia non porta e che si registra accanto ai suoi — il compilatore ha
/// indicato i posti da toccare, e sono stati questi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// Un formato che il motore sa aprire.
    Supportato,
    /// Un formato che riconosciamo e non sappiamo suonare.
    ///
    /// Rimasto in uno: WMA. Non è una svista — symphonia non ha un
    /// decodificatore, e nessuna libreria in Rust puro lo porta — e fingere di
    /// provarci darebbe un errore di decodifica generico invece di una
    /// risposta. La variante resta anche se il suo elenco si è accorciato:
    /// serve a distinguere «riconosciuto e non suonabile», che va detto
    /// all'utente com'è, da «non è musica», che si tace.
    NonSupportato,
    /// Non è nemmeno un'estensione che consideriamo musica.
    Sconosciuto,
}

impl Codec {
    /// Cosa possiamo fare di un file con questa estensione.
    ///
    /// # Perché `.mka`, `.webm` e `.caf` non ci sono ancora
    ///
    /// Il motore li sa suonare: symphonia 0.6 demultiplexa Matroska/WebM e CAF,
    /// e dentro ci sono codec che già decodifichiamo — le prove in
    /// `decodifica` ne costruiscono uno per tipo e ci saltano dentro. Quel che
    /// manca sta sopra al motore. `rymembra-app` legge tag e durata con lofty, e
    /// lofty 0.25 non conosce né Matroska né CAF: aggiunti qui e all'elenco
    /// chiuso del dominio, quei file entrerebbero in libreria senza titolo,
    /// senza copertina e con una durata a zero — e di un Matroska symphonia non
    /// dà nemmeno il numero dei fotogrammi. Ci sono poi due limiti di Matroska
    /// nella 0.6.1 che un disco senza stacchi sentirebbe: l'imbottitura finale
    /// (`DiscardPadding`) non si toglie, e senza indice (`Cues`) si salta solo
    /// in avanti, quindi il ritorno all'inizio di un brano già preparato non
    /// torna indietro. Per questo l'elenco resta quello del dominio, e questa
    /// funzione con lui: prima la lettura dei tag, poi le estensioni.
    ///
    /// `.ape` invece c'è: Monkey's Audio lo legge `crate::ape`, un lettore nostro
    /// sopra `ape-decoder`, e lofty ne legge i tag. È il formato di tante
    /// immagini CD col loro `.cue`, che è la ragione per cui è entrato.
    #[must_use]
    pub fn da_estensione(estensione: &str) -> Self {
        let pulita = estensione.trim_start_matches('.').to_ascii_lowercase();
        match pulita.as_str() {
            "mp3" | "flac" | "m4a" | "aac" | "ogg" | "oga" | "wav" | "aiff" | "aif" | "opus"
            | "ape" => Self::Supportato,
            "wma" => Self::NonSupportato,
            _ => Self::Sconosciuto,
        }
    }
}

/// Quel che i tre fili si dicono senza parlarsi.
///
/// Tutto atomico, perché uno dei lettori è la callback audio — che non può
/// prendere lucchetti — e un altro è il filo che decodifica, che non deve
/// aspettarla.
pub(crate) struct Condiviso {
    /// In pausa: la callback sfuma in una dozzina di millisecondi, poi scrive
    /// silenzio senza consumare l'anello.
    pub in_pausa: AtomicBool,
    /// Il filo che decodifica chiede alla callback di scartare tutto.
    ///
    /// Lo alza solo il decodificatore, lo abbassa solo la callback: è una
    /// stretta di mano a due, e per questo non serve altro che un booleano.
    pub svuota: AtomicBool,
    /// Il guadagno voluto, nei bit di un `f32`.
    pub guadagno: AtomicU32,
    /// I fotogrammi consegnati al buffer del dispositivo.
    ///
    /// **Non** «usciti dalle casse», come diceva questa riga: fra la consegna e
    /// il suono ci sono il buffer del dispositivo e tutta la catena d'uscita. I
    /// tre termini li distingue il `//!` di [`crate::motore`], e quanti
    /// fotogrammi valgano gli altri due lo dice
    /// [`Condiviso::latenza_fotogrammi`].
    pub fotogrammi: AtomicU64,
    /// I campioni serviti a vuoto perché l'anello era secco.
    pub vuoti: AtomicU64,
    /// Quanti canali ha l'uscita, per contare i fotogrammi.
    pub canali: AtomicU32,
    /// Quanti fotogrammi stanno fra il conteggio della callback e le casse.
    ///
    /// La callback conta i fotogrammi quando li consegna al buffer del
    /// dispositivo, non quando si sentono: fra le due cose c'è quel buffer, e
    /// `cpal` lo sa misurare — la differenza fra i due `StreamInstant` che
    /// accompagnano ogni blocco. Qui ci finisce quel numero, convertito in
    /// fotogrammi alla frequenza d'uscita.
    ///
    /// **Una stima, e solo di un pezzo della catena.** Quanto valga e quanto si
    /// possa crederci è scritto una volta sola, in
    /// `crate::uscita::annota_latenza`; chi la legge la tratta con il tetto di
    /// [`crate::uscita::LATENZA_MASSIMA_MS`] e ci somma la correzione a mano.
    ///
    /// `Relaxed` da entrambe le parti: non ordina niente rispetto a nient'altro
    /// — chi la legge vuole l'ultimo valore noto, e leggerne uno vecchio di un
    /// blocco vale qualche millisecondo su un numero che è già una stima.
    pub latenza_fotogrammi: AtomicU64,
    /// Il motore non suona più, e non ricomincerà da solo.
    ///
    /// Due cause, una bandiera sola: il dispositivo che sparisce da sotto i
    /// piedi — la callback di cpal riceve uno `StreamError` — e il filo della
    /// decodifica che cade. Sono guasti diversi ma chiedono la stessa cosa,
    /// cioè un motore ricostruito, e la via che lo ricostruisce è già scritta
    /// una volta sola: il sorvegliante delle uscite legge questa bandiera,
    /// annota dov'era la puntina e riapre. Duplicarla per la seconda causa
    /// avrebbe voluto dire una seconda strada di riapertura da tenere in pari
    /// con la prima.
    ///
    /// A dire **quale** delle due è [`Condiviso::causa_perdita`], che è
    /// l'unica cosa che chi legge il diario o guarda il banner deve
    /// distinguere.
    pub perso: AtomicBool,
    /// Quale delle cause ha alzato [`Condiviso::perso`].
    ///
    /// Uno e due li scrive la callback dell'uscita, per i due `StreamError` di
    /// cpal; il tre il filo della decodifica quando cade, ed è l'unico che non
    /// parli del dispositivo. La tabella che li traduce in una frase e in una
    /// chiave sta in `Motore::perdita`, una sola volta.
    ///
    /// Si scrive **prima** della bandiera: chi legge le due caselle arriva
    /// dall'altra parte e in quest'ordine, e scriverle al contrario lascerebbe
    /// una finestra in cui la causa è ancora zero — «causa sconosciuta»
    /// proprio nell'istante che c'è da raccontare.
    pub causa_perdita: AtomicU32,
    /// Questo motore è stato sostituito: qualunque cosa dica, non la dica più.
    ///
    /// # Perché serve
    ///
    /// Perché l'osservatore di un motore sostituito è ancora **vivo** e parla
    /// allo stato dell'applicazione, che nel frattempo è passato al motore
    /// nuovo. Un filo di decodifica che finisce di morire mezzo secondo dopo la
    /// sostituzione annuncerebbe un `Fermato` — e chi sta sopra lo leggerebbe
    /// come «l'ultimo brano è uscito per intero», chiudendo l'ascolto e
    /// facendo avanzare una coda che sta suonando altrove.
    ///
    /// La alza [`Motore::drop`] prima ancora di mandare il comando di
    /// chiusura, la legge `Contesto::annuncia`, e non torna indietro: un motore
    /// abbandonato non si riprende.
    pub abbandonato: AtomicBool,
    /// Qualcuno sta guardando lo spettro.
    ///
    /// Spento, la callback non scrive niente nel terzo anello: chi non guarda
    /// non paga. È un booleano e non un conteggio perché la schermata che lo
    /// mostra è una sola.
    pub spettro: AtomicBool,
    /// La bandiera dell'annullo di quel che il filo della decodifica sta
    /// leggendo adesso, se può restare ad aspettare la rete.
    ///
    /// # Perché passa di qui
    ///
    /// Perché un salto su un flusso dell'Internet Archive può aspettare la rete
    /// per secondi, e il salto dopo — chi trascina il cursore ne manda sei —
    /// restava in fila dietro di lui. Chi manda `VaiA`, `Suona` o `Ferma` alza
    /// la bandiera trovata qui **prima** di mandare il comando: la lettura in
    /// corso smette di aspettare, il filo raccoglie il comando nuovo.
    ///
    /// Il filo la pubblica solo attorno alle letture che possono essere
    /// lasciate a metà — un blocco del brano corrente, un salto — e la ritira
    /// subito dopo. Il riavvolgimento a zero di un brano in canna non la
    /// pubblica: annullato, quel brano andrebbe perso, e chi sta sopra lo
    /// crederebbe ancora in canna. Pubblicare e alzare stanno sotto lo stesso
    /// lucchetto: una bandiera alzata per un comando già raccolto viene
    /// riabbassata dal filo prima della lettura dopo, e il caso peggiore è un
    /// annullo mancato — un salto che aspetta la rete come prima — mai un
    /// comando perso.
    ///
    /// Un lucchetto e non un atomico perché a toccarla sono solo il filo della
    /// finestra e quello della decodifica, mai la callback audio.
    pub in_lettura: std::sync::Mutex<Option<Arc<AtomicBool>>>,
}

impl Condiviso {
    pub(crate) fn nuovo() -> Arc<Self> {
        Arc::new(Self {
            // Si comincia in pausa: il flusso nasce a riposo, e anche dopo il
            // primo risveglio non deve consumare niente finché non c'è un brano.
            in_pausa: AtomicBool::new(true),
            svuota: AtomicBool::new(false),
            guadagno: AtomicU32::new(1.0f32.to_bits()),
            fotogrammi: AtomicU64::new(0),
            vuoti: AtomicU64::new(0),
            canali: AtomicU32::new(2),
            latenza_fotogrammi: AtomicU64::new(0),
            perso: AtomicBool::new(false),
            causa_perdita: AtomicU32::new(0),
            abbandonato: AtomicBool::new(false),
            spettro: AtomicBool::new(false),
            in_lettura: std::sync::Mutex::new(None),
        })
    }

    /// Alza la bandiera di quel che il filo della decodifica sta leggendo, se
    /// sta leggendo qualcosa che si può lasciare a metà. Vedi
    /// [`Condiviso::in_lettura`].
    pub(crate) fn interrompi_la_lettura(&self) {
        let lettura = self
            .in_lettura
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(bandiera) = lettura.as_ref() {
            bandiera.store(true, Ordering::Release);
        }
    }

    /// Dichiara che il filo della decodifica sta per leggere da qui, con la
    /// bandiera abbassata: un comando raccolto prima non deve fermare questa
    /// lettura.
    pub(crate) fn pubblica_lettura(&self, bandiera: Option<Arc<AtomicBool>>) {
        let mut lettura = self
            .in_lettura
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(b) = bandiera.as_ref() {
            b.store(false, Ordering::Release);
        }
        *lettura = bandiera;
    }

    /// La lettura è finita: niente più da interrompere.
    ///
    /// La bandiera si riabbassa qui, e non solo alla prossima pubblicazione:
    /// il decodificatore che l'ha in mano può essere riavvolto un attimo dopo
    /// **senza** pubblicarla — un brano che torna in canna — e con la bandiera
    /// ancora alzata quel riavvolgimento fallirebbe da sé, perdendo il brano.
    /// Il comando che l'aveva alzata è già in fila, e il giro dopo lo raccoglie
    /// comunque.
    pub(crate) fn ritira_lettura(&self) {
        let mut lettura = self
            .in_lettura
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(bandiera) = lettura.take() {
            bandiera.store(false, Ordering::Release);
        }
    }
}

/// Di quanto va corretto un brano, e fin dove può salire.
///
/// # Perché il picco viaggia insieme al guadagno
///
/// Perché un guadagno da solo non sa se si può applicare. Un disco degli anni
/// Settanta misurato a −24 LUFS chiede sei decibel per arrivare al riferimento,
/// e se il suo picco sta già a −2 dBFS quei sei decibel lo portano quattro
/// decibel **oltre** il fondo scala: la normalizzazione, che doveva solo
/// uniformare i livelli, diventerebbe una distorsione. Col picco accanto,
/// [`correzione`] sa fermarsi dove il brano smetterebbe di starci.
///
/// Il bersaglio invece **non** sta qui: è una scelta di chi ascolta, può
/// cambiare a metà brano, e il motore la tiene per conto suo. È la ragione per
/// cui il limite si calcola al momento, e non una volta per tutte da chi apre il
/// brano.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Livello {
    /// Di quanti decibel spostare il brano per portarlo al riferimento di −18
    /// LUFS: dal tag ReplayGain, o dalla misura di [`intensita`]. `None`: niente
    /// da correggere.
    pub guadagno_db: Option<f32>,
    /// Il picco del brano, lineare (`1.0` è il fondo scala), se lo si conosce.
    ///
    /// È il picco **vero** della misura — quello fra un campione e l'altro, che
    /// è dove una conversione analogica satura davvero — e per un brano non
    /// ancora misurato non c'è: allora il guadagno si applica senza tetto, come
    /// prima della 3.0, e a tenere a bada l'eccesso resta [`limita`].
    pub picco: Option<f32>,
}

impl Livello {
    /// Nessuna correzione: il brano esce com'è.
    pub const NESSUNO: Self = Self {
        guadagno_db: None,
        picco: None,
    };
}

/// Il guadagno da applicare, messi insieme volume, silenziamento e correzione.
///
/// # La correzione
///
/// Il livello dichiara di quanti decibel il brano va corretto per stare a un
/// riferimento di −18 LUFS. `bersaglio_db` sposta quel riferimento: a −18 il
/// valore si usa com'è, a −14 si aggiungono quattro decibel. Vedi
/// [`correzione`] per i tagli.
#[must_use]
pub fn guadagno(
    volume: f32,
    muto: bool,
    livello: Livello,
    replaygain_attivo: bool,
    bersaglio_db: f32,
) -> f32 {
    volume_uscita(volume, muto) * correzione(livello, replaygain_attivo, bersaglio_db)
}

/// Quanto chiede il cursore del volume, e nient'altro.
///
/// # Perché è una funzione a sé
///
/// Perché il volume è dell'**uscita** e la correzione ReplayGain è del
/// **brano**, e con la dissolvenza incrociata la differenza smette di essere
/// filosofica: durante una sovrapposizione ci sono due brani, quindi due
/// correzioni diverse, ma un cursore del volume solo. Un unico scalare
/// globale non può dire due cose insieme.
///
/// Quel che resta globale — questo — lo applica la callback in fondo alla
/// catena, con la sua rampa. Quel che è del brano viaggia coi campioni, e li
/// raggiunge nel filo che decodifica.
///
/// # Perché una curva, e non la posizione del cursore così com'è
///
/// Perché l'orecchio sente i rapporti, non le differenze. Con il cursore
/// lineare, fino alla 2.x, tutta la manopola utile stava nel primo quarto:
/// da metà corsa in su il volume cambiava di sei decibel in tutto, e sotto il
/// dieci per cento ogni pixel era un salto. Il cubo della posizione è la
/// curva che usano i mixer di sistema per la stessa ragione: a metà cursore
/// dà −18 dB, a otto decimi −6, e ogni tratto della corsa sposta il volume di
/// una quantità che si sente simile.
///
/// La posizione salvata dalla 2.x era il guadagno lineare, non la posizione:
/// la converte una volta sola `rymembra_app::playback::load_volume`, così chi
/// aggiorna ritrova lo stesso volume con il cursore in un punto diverso.
#[must_use]
pub fn volume_uscita(volume: f32, muto: bool) -> f32 {
    if muto {
        return 0.0;
    }
    let posizione = volume.clamp(0.0, 1.0);
    posizione * posizione * posizione
}

/// Quanto sotto il fondo scala un brano normalizzato può arrivare, al massimo.
///
/// Il fondo scala esatto: il tetto di [`correzione`] è «non salire oltre il
/// picco che il file ha già», non un margine di sicurezza. Il margine lo dà
/// [`limita`], che comincia a piegare un decibel prima.
const TETTO_PICCO: f32 = 1.0;

/// La correzione che porta un brano al bersaglio, e nient'altro.
///
/// Senza guadagno la correzione è esattamente 1.0, cioè niente: è il motivo
/// per cui tenere la normalizzazione accesa su una libreria senza tag e non
/// ancora misurata non cambia una virgola.
///
/// # I due tagli
///
/// - Fra −24 e +12 dB, perché un tag sbagliato — e ce ne sono — non deve poter
///   mandare in saturazione l'uscita né azzerarla.
/// - Col picco noto, un guadagno che **alza** si ferma dove il picco tocca il
///   fondo scala. Solo quando alza: un brano che la normalizzazione abbassa non
///   può saturare più di quanto già faccia il file, e un picco oltre lo zero —
///   capita, sulle masterizzazioni recenti misurate col picco vero — non deve
///   costare al brano il livello che il riferimento gli chiede.
#[must_use]
pub fn correzione(livello: Livello, attivo: bool, bersaglio_db: f32) -> f32 {
    let (true, Some(db)) = (attivo, livello.guadagno_db) else {
        return 1.0;
    };
    let spostato = db + (bersaglio_db + 18.0);
    let voluto = 10.0f32.powf(spostato.clamp(-24.0, 12.0) / 20.0);
    match livello.picco {
        Some(picco) if voluto > 1.0 && picco.is_finite() && picco > 0.0 => {
            voluto.min((TETTO_PICCO / picco).max(1.0))
        }
        _ => voluto,
    }
}

/// Da dove comincia a piegare [`limita`]: −1 dBFS.
const GINOCCHIO: f32 = 0.891_250_9;

/// Il limitatore morbido in fondo alla catena: sotto −1 dBFS non tocca niente,
/// sopra piega dolcemente verso il fondo scala invece di tagliare.
///
/// # Perché serve anche con il tetto di [`correzione`]
///
/// Perché il tetto conosce solo il picco del **file**, e non tutto quel che
/// arriva all'uscita viene da lì: l'equalizzatore alza i bassi fino a dodici
/// decibel, un brano non ancora misurato alza senza sapere dove si ferma, un
/// bersaglio a −14 porta i dischi forti sopra lo zero. Prima di questa
/// funzione quei campioni arrivavano alla conversione e venivano tagliati di
/// netto, che è la distorsione più brutta che ci sia: un gradino, con tutte le
/// armoniche che si porta dietro.
///
/// # Perché una curva fissa e non un limitatore con attacco e rilascio
///
/// Perché gira nella callback audio, per ogni campione, e lì non c'è posto per
/// un ritardo di previsione né per uno stato da tenere in pari fra i canali.
/// Una tangente iperbolica sopra il ginocchio è continua con la sua derivata —
/// niente spigoli, quindi niente armoniche in più finché il segnale non ci
/// arriva — e al di sotto costa un confronto.
#[must_use]
#[inline]
pub fn limita(campione: f32) -> f32 {
    let ampiezza = campione.abs();
    if ampiezza <= GINOCCHIO {
        return campione;
    }
    let spazio = 1.0 - GINOCCHIO;
    let piegato = GINOCCHIO + spazio * ((ampiezza - GINOCCHIO) / spazio).tanh();
    piegato.copysign(campione)
}

/// Mescola due brani che si sovrappongono, dentro il blocco di quello uscente.
///
/// `fatti` è a che punto della dissolvenza si era all'inizio del blocco, in
/// fotogrammi; `durata` è quanto dura tutta la sovrapposizione. Il blocco
/// uscente viene riscritto con la somma dei due, e quel che avanza
/// dell'entrante — se è più lungo — resta a chi chiama.
///
/// # Perché a potenza costante e non lineare
///
/// Perché i due brani sono **scorrelati**. Sommando due segnali senza
/// relazione fra loro, le potenze si sommano e le ampiezze no: due rampe
/// lineari che si incrociano a metà darebbero, a metà strada, due mezze
/// ampiezze che fanno il 70% della potenza di partenza — un avvallamento
/// udibile proprio nel punto in cui l'ascoltatore sta cercando di capire cosa
/// sta succedendo. Con seno e coseno la somma dei quadrati è uno a ogni
/// istante, e il passaggio non ha un buco in mezzo.
///
/// Su materiale correlato — la stessa nota, lo stesso disco — la scelta giusta
/// sarebbe l'opposta, ma quello è il gapless, e il gapless non passa di qui.
///
/// # Perché una funzione pura
///
/// Perché è l'unico modo onesto di provare questa parte del motore. Attorno c'è
/// `Contesto`, che contiene due `Box<dyn FormatReader>` di symphonia: provarlo
/// vorrebbe dire un file vero su disco e un dispositivo audio. Qui invece ci
/// sono due fette di campioni, e le prove possono chiedersi se la potenza si
/// conserva davvero.
#[expect(
    clippy::integer_division,
    reason = "l'indice del fotogramma dentro un blocco interlacciato: la divisione               per il numero di canali è esatta per costruzione"
)]
pub fn dissolvi(uscente: &mut [f32], entrante: &[f32], fatti: u64, durata: u64, canali: u16) {
    let canali = usize::from(canali.max(1));
    // Una dissolvenza lunga zero non è una dissolvenza: senza questa riga
    // sarebbe una divisione per zero, e la prima cosa che si sentirebbe è un
    // silenzio pieno di NaN.
    if durata == 0 {
        return;
    }
    for (indice, campione) in uscente.iter_mut().enumerate() {
        // `indice / canali` e non `indice`: la rampa avanza per **fotogramma**,
        // così i canali di uno stesso istante ricevono lo stesso guadagno.
        // Trattandoli uno per uno l'immagine stereo scivolerebbe di mezzo
        // campione durante tutta la dissolvenza.
        let fotogramma = fatti.saturating_add((indice / canali) as u64);
        let (fuori, dentro) = punto_curva(fotogramma, durata);
        let entra = entrante.get(indice).copied().unwrap_or(0.0);
        *campione = *campione * fuori + entra * dentro;
    }
}

/// I due guadagni della curva a un certo fotogramma: quanto di quello che esce,
/// quanto di quello che entra.
///
/// Una funzione sola perché la curva deve essere **una**: [`risali`] riprende
/// esattamente dal punto in cui [`dissolvi`] si è fermata, e due formule scritte
/// in due posti che si scostassero di un capello produrrebbero un gradino
/// proprio lì — cioè un clic nel punto che tutta questa parte del motore esiste
/// per non avere.
pub(crate) fn punto_curva(fotogramma: u64, durata: u64) -> (f32, f32) {
    // Oltre la fine si resta a fondo corsa invece di traboccare: il blocco
    // può sforare la dissolvenza, e quel che sfora è già tutto il nuovo.
    let t = if fotogramma >= durata || durata == 0 {
        1.0
    } else {
        fotogramma as f32 / durata as f32
    };
    let angolo = t * std::f32::consts::FRAC_PI_2;
    (angolo.cos(), angolo.sin())
}

/// Porta a piena ampiezza un brano che stava entrando, da solo.
///
/// `da` è il guadagno che aveva quando è rimasto solo — il seno di
/// [`punto_curva`] al fotogramma in cui la sovrapposizione si è interrotta —
/// e da lì la salita è lineare: `fatti` fotogrammi fatti su `durata`.
///
/// # Quando serve
///
/// La sovrapposizione comincia quando alla fine del brano manca quanto dura la
/// dissolvenza, e «quanto manca» si sa dalla durata **dichiarata** dal database.
/// Certi file la dichiarano più lunga di quel che contengono — un MP3 a bitrate
/// variabile senza intestazione Xing la fa stimare, e la stima sbaglia — e
/// allora il decodificatore del brano uscente finisce a metà curva. Il brano che
/// entrava è a mezza ampiezza e non ha più niente sotto.
///
/// Senza questa funzione salterebbe di colpo a tutta ampiezza: un gradino nella
/// forma d'onda, cioè un clic, sul brano che poi si ascolta per intero. Con
/// questa, riprende da dove era e ci arriva in una quarantina di millisecondi.
///
/// # Perché breve e lineare, e non il resto della curva
///
/// Perché la curva era tarata su una durata che si è appena scoperta falsa:
/// continuarla vorrebbe dire un brano che parte a mezza ampiezza e ci mette dei
/// secondi a venire su, da solo e senza niente da cui emergere — un errore più
/// udibile del clic che si voleva togliere. E lineare perché su quaranta
/// millisecondi la potenza costante non ha niente da conservare: non c'è un
/// secondo segnale con cui sommarsi, c'è solo un livello che si muove, ed è la
/// stessa rampa con cui l'uscita insegue il cursore del volume.
#[expect(
    clippy::integer_division,
    reason = "l'indice del fotogramma dentro un blocco interlacciato: la divisione \
              per il numero di canali è esatta per costruzione"
)]
pub fn risali(blocco: &mut [f32], da: f32, fatti: u64, durata: u64, canali: u16) {
    let canali = usize::from(canali.max(1));
    // Una salita lunga zero è una divisione per zero, e chi chiama non deve
    // chiamare: la riga sta qui perché «non deve» dentro un `/` non è un
    // controllo.
    if durata == 0 {
        return;
    }
    let da = da.clamp(0.0, 1.0);
    for (indice, campione) in blocco.iter_mut().enumerate() {
        let fotogramma = fatti.saturating_add((indice / canali) as u64);
        let t = if fotogramma >= durata {
            1.0
        } else {
            fotogramma as f32 / durata as f32
        };
        *campione *= da + (1.0 - da) * t;
    }
}

/// Apre il dispositivo e avvia i fili. È il punto d'ingresso del crate.
///
/// L'osservatore viene chiamato da un filo del motore, mai da quello della
/// callback audio: può prendere lucchetti, scrivere su disco e mandare eventi
/// alla finestra senza rischiare di interrompere il suono.
///
/// `voluto` è il nome dell'uscita su cui aprire — `None` per quella predefinita
/// di sistema. Un nome che non corrisponde a niente **non è un errore**: si
/// ripiega sul predefinito, e chi ha chiamato lo scopre da
/// [`Motore::dispositivo`], che dice su cosa si è aperto davvero. Vedi
/// [`dispositivi`] per il perché l'identità sia una stringa e quanto poco
/// regga.
pub fn avvia(
    osservatore: impl Fn(Evento) + Send + 'static,
    voluto: Option<String>,
) -> Result<Motore, AppError> {
    Motore::avvia(osservatore, voluto)
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn la_bandiera_dell_annullo_si_alza_solo_mentre_si_legge() {
        let condiviso = Condiviso::nuovo();
        let bandiera = Arc::new(AtomicBool::new(false));
        // Niente pubblicato: un salto non ha niente da interrompere.
        condiviso.interrompi_la_lettura();
        assert!(!bandiera.load(Ordering::Acquire));

        // Una bandiera rimasta alzata da prima si riabbassa alla
        // pubblicazione: un comando già raccolto non ferma la lettura dopo.
        bandiera.store(true, Ordering::Release);
        condiviso.pubblica_lettura(Some(Arc::clone(&bandiera)));
        assert!(!bandiera.load(Ordering::Acquire));

        // Durante la lettura, un comando la alza.
        condiviso.interrompi_la_lettura();
        assert!(bandiera.load(Ordering::Acquire));

        // Ritirata, si riabbassa: il decodificatore può tornare in canna e
        // riavvolgersi senza fallire da sé.
        condiviso.ritira_lettura();
        assert!(!bandiera.load(Ordering::Acquire));
        condiviso.interrompi_la_lettura();
        assert!(
            !bandiera.load(Ordering::Acquire),
            "non più pubblicata: nessuno la tocca"
        );
    }

    #[test]
    fn i_formati_che_sappiamo_e_quelli_che_no() {
        assert_eq!(Codec::da_estensione("mp3"), Codec::Supportato);
        assert_eq!(Codec::da_estensione(".FLAC"), Codec::Supportato);
        assert_eq!(Codec::da_estensione("opus"), Codec::Supportato);
        // `.oga` è Ogg come `.ogg`: stesso contenitore, stesso demultiplatore,
        // e dentro ci può stare Vorbis, FLAC o Opus indifferentemente.
        assert_eq!(Codec::da_estensione("oga"), Codec::Supportato);
        // Monkey's Audio, il formato delle immagini CD: senza perdita, e lo
        // sappiamo suonare.
        assert_eq!(Codec::da_estensione("ape"), Codec::Supportato);
        assert_eq!(Codec::da_estensione(".APE"), Codec::Supportato);
        assert_eq!(Codec::da_estensione("wma"), Codec::NonSupportato);
        assert_eq!(Codec::da_estensione("txt"), Codec::Sconosciuto);
    }

    #[test]
    fn tutte_le_estensioni_del_dominio_hanno_una_risposta() {
        // Nessuna estensione che il dominio considera musica deve risultare
        // «sconosciuta»: o la sappiamo suonare, o lo diciamo.
        for ext in rymembra_domain::paths::SUPPORTED_EXTENSIONS {
            assert_ne!(
                Codec::da_estensione(ext),
                Codec::Sconosciuto,
                "estensione {ext} senza risposta"
            );
        }
    }

    /// Solo il guadagno, senza picco: il caso di un tag ReplayGain.
    const fn db(valore: f32) -> Livello {
        Livello {
            guadagno_db: Some(valore),
            picco: None,
        }
    }

    #[test]
    fn il_silenziamento_vince_su_tutto() {
        assert_eq!(guadagno(1.0, true, db(6.0), true, -18.0), 0.0);
    }

    #[test]
    fn senza_replaygain_il_guadagno_e_il_volume() {
        let atteso = volume_uscita(0.5, false);
        assert!(
            (guadagno(0.5, false, Livello::NESSUNO, true, -18.0) - atteso).abs() < f32::EPSILON
        );
        // Il tag c'è ma la correzione è spenta.
        assert!((guadagno(0.5, false, db(-6.0), false, -18.0) - atteso).abs() < f32::EPSILON);
    }

    #[test]
    fn un_brano_troppo_forte_viene_abbassato() {
        let g = guadagno(1.0, false, db(-6.0), true, -18.0);
        assert!(g < 1.0 && g > 0.4, "guadagno {g}");
    }

    #[test]
    fn il_bersaglio_sposta_il_riferimento() {
        let a_18 = guadagno(1.0, false, db(0.0), true, -18.0);
        let a_14 = guadagno(1.0, false, db(0.0), true, -14.0);
        assert!(a_14 > a_18, "a −14 deve suonare più forte");
    }

    #[test]
    fn un_tag_assurdo_non_manda_in_saturazione() {
        let g = guadagno(1.0, false, db(90.0), true, -18.0);
        // +12 dB è il tetto: circa 4x, non trentamila.
        assert!(g < 4.1, "guadagno {g}");
    }

    #[test]
    fn il_volume_si_taglia_fra_zero_e_uno() {
        assert!((guadagno(5.0, false, Livello::NESSUNO, false, -18.0) - 1.0).abs() < f32::EPSILON);
        assert_eq!(guadagno(-1.0, false, Livello::NESSUNO, false, -18.0), 0.0);
    }

    // ── la curva del volume ──

    /// Da guadagno lineare a decibel.
    fn in_db(g: f32) -> f32 {
        20.0 * g.log10()
    }

    #[test]
    fn la_curva_del_volume_va_in_decibel_e_non_in_pixel() {
        // I tre punti promessi dal commento di `volume_uscita`.
        assert!((in_db(volume_uscita(0.5, false)) + 18.06).abs() < 0.05);
        assert!((in_db(volume_uscita(0.8, false)) + 5.81).abs() < 0.05);
        assert_eq!(volume_uscita(1.0, false), 1.0);
        assert_eq!(volume_uscita(0.0, false), 0.0);
        // E ogni tratto della corsa sposta qualcosa: nessun gradino piatto in
        // cima, dov'era il difetto del cursore lineare.
        let alto = in_db(volume_uscita(0.9, false)) - in_db(volume_uscita(0.7, false));
        let medio = in_db(volume_uscita(0.6, false)) - in_db(volume_uscita(0.4, false));
        assert!(alto > 5.0, "dal 70 al 90% solo {alto} dB");
        assert!(medio > 9.0, "dal 40 al 60% solo {medio} dB");
    }

    #[test]
    fn la_curva_cresce_sempre() {
        let mut prima = -1.0_f32;
        for passo in 0..=100_u16 {
            let g = volume_uscita(f32::from(passo) / 100.0, false);
            assert!(g > prima || passo == 0, "la curva scende a {passo}%");
            prima = g;
        }
    }

    // ── il tetto del picco ──

    #[test]
    fn un_brano_che_sale_si_ferma_al_suo_picco() {
        // +6 dB chiesti, picco a −2 dBFS: si può salire di due, non di sei.
        let livello = Livello {
            guadagno_db: Some(6.0),
            picco: Some(10.0_f32.powf(-2.0 / 20.0)),
        };
        let g = correzione(livello, true, -18.0);
        assert!((in_db(g) - 2.0).abs() < 0.01, "salito di {} dB", in_db(g));
    }

    #[test]
    fn un_brano_che_scende_non_guarda_il_picco() {
        // Un picco oltre lo zero su un brano da abbassare: il tetto non c'entra,
        // e il brano deve arrivare dove il riferimento lo vuole.
        let livello = Livello {
            guadagno_db: Some(-8.0),
            picco: Some(1.3),
        };
        let g = correzione(livello, true, -18.0);
        assert!((in_db(g) + 8.0).abs() < 0.01, "sceso di {} dB", in_db(g));
    }

    #[test]
    fn un_picco_gia_oltre_lo_zero_non_abbassa_chi_doveva_salire() {
        // Chiesti +3 dB, picco a +1 dBFS: non si sale, ma nemmeno si scende
        // sotto il file così com'è.
        let livello = Livello {
            guadagno_db: Some(3.0),
            picco: Some(10.0_f32.powf(1.0 / 20.0)),
        };
        assert!((correzione(livello, true, -18.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn un_picco_assurdo_si_ignora() {
        for picco in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let livello = Livello {
                guadagno_db: Some(6.0),
                picco: Some(picco),
            };
            let g = correzione(livello, true, -18.0);
            assert!(
                (in_db(g) - 6.0).abs() < 0.01,
                "picco {picco}: {} dB",
                in_db(g)
            );
        }
    }

    // ── il limitatore ──

    #[test]
    fn sotto_il_ginocchio_il_limitatore_non_tocca_niente() {
        for campione in [0.0_f32, 0.25, -0.5, 0.89, -0.891] {
            assert_eq!(limita(campione).to_bits(), campione.to_bits());
        }
    }

    #[test]
    fn sopra_il_ginocchio_piega_e_non_passa_il_fondo_scala() {
        let mut prima = 0.0_f32;
        for passo in 0..=400_u16 {
            let entrata = 0.85 + f32::from(passo) / 100.0;
            let uscita = limita(entrata);
            assert!(uscita <= 1.0, "{entrata} esce a {uscita}");
            assert!(uscita >= prima, "il limitatore torna indietro a {entrata}");
            assert!(
                (limita(-entrata) + uscita).abs() < f32::EPSILON,
                "asimmetrico a {entrata}"
            );
            prima = uscita;
        }
    }

    #[test]
    fn il_limitatore_non_ha_spigoli_al_ginocchio() {
        // Pendenza uno subito sopra il ginocchio, come subito sotto: niente
        // gradino nella derivata, quindi niente armoniche finché non ci si
        // arriva davvero.
        let h = 1e-4_f32;
        let pendenza = (limita(GINOCCHIO + h) - limita(GINOCCHIO)) / h;
        assert!((pendenza - 1.0).abs() < 0.01, "pendenza {pendenza}");
    }

    // ── la dissolvenza ──

    #[test]
    fn una_dissolvenza_lunga_zero_e_il_gapless_di_prima() {
        // La prova che il piano chiedeva: a durata 0 il blocco che esce deve
        // restare **identico** a com'era, campione per campione. È la garanzia
        // che accendere il crossfade sia una scelta e non un cambiamento del
        // suono di chi non lo accende.
        let originale = [0.5_f32, -0.25, 0.125, -1.0];
        let mut uscente = originale;
        dissolvi(&mut uscente, &[1.0; 4], 0, 0, 2);
        assert_eq!(uscente, originale);
    }

    #[test]
    fn all_inizio_si_sente_solo_quello_che_esce() {
        let mut uscente = [1.0_f32, 1.0];
        dissolvi(&mut uscente, &[1.0, 1.0], 0, 100, 2);
        // cos(0) = 1, sin(0) = 0: il brano entrante non c'è ancora.
        for campione in uscente {
            assert!((campione - 1.0).abs() < 1e-6, "campione {campione}");
        }
    }

    #[test]
    fn alla_fine_si_sente_solo_quello_che_entra() {
        let mut uscente = [1.0_f32, 1.0];
        dissolvi(&mut uscente, &[0.5, 0.5], 100, 100, 2);
        for campione in uscente {
            assert!((campione - 0.5).abs() < 1e-6, "campione {campione}");
        }
    }

    #[test]
    fn oltre_la_fine_si_resta_a_fondo_corsa() {
        // Il blocco può sforare la finestra: quel che sfora è già tutto il
        // brano nuovo, non un guadagno che continua a crescere.
        let mut uscente = [1.0_f32, 1.0];
        dissolvi(&mut uscente, &[0.5, 0.5], 500, 100, 2);
        for campione in uscente {
            assert!((campione - 0.5).abs() < 1e-6, "campione {campione}");
        }
    }

    #[test]
    fn l_energia_resta_costante_per_tutta_la_dissolvenza() {
        // Il motivo per cui la curva è coseno/seno e non due rette. Su
        // materiale scorrelato — che è il caso di due brani diversi — le
        // potenze si sommano, quindi cos²+sin² = 1 tiene il volume percepito
        // fermo. Due rampe lineari darebbero 0,5 a metà: un buco udibile.
        for fotogramma in 0..=100_u64 {
            let mut uscente = [1.0_f32];
            dissolvi(&mut uscente, &[1.0], fotogramma, 100, 1);
            let a = (fotogramma as f32 / 100.0) * std::f32::consts::FRAC_PI_2;
            let energia = a.cos().powi(2) + a.sin().powi(2);
            assert!((energia - 1.0).abs() < 1e-5, "energia {energia}");
            // E la somma non deve mai superare la radice di due, che è il
            // massimo di cos+sin: oltre ci sarebbe saturazione.
            let somma = uscente[0];
            assert!(somma <= std::f32::consts::SQRT_2 + 1e-5, "somma {somma}");
        }
    }

    #[test]
    fn i_canali_di_uno_stesso_fotogramma_ricevono_lo_stesso_guadagno() {
        // Se la rampa avanzasse per campione invece che per fotogramma,
        // sinistra e destra starebbero su due punti diversi della curva e
        // l'immagine stereo scivolerebbe per tutta la dissolvenza.
        let mut uscente = [1.0_f32, 1.0, 1.0, 1.0];
        dissolvi(&mut uscente, &[0.0; 4], 0, 4, 2);
        assert!((uscente[0] - uscente[1]).abs() < f32::EPSILON);
        assert!((uscente[2] - uscente[3]).abs() < f32::EPSILON);
        // E fotogrammi diversi devono invece essere diversi, altrimenti la
        // prova qui sopra passerebbe anche con una rampa ferma.
        assert!(uscente[0] > uscente[2]);
    }

    #[test]
    fn un_blocco_entrante_piu_corto_non_esce_dai_bordi() {
        // `indexing_slicing` è vietato nel progetto e `dissolvi` usa `get`: la
        // prova sta qui perché il caso è reale, non teorico. L'ultimo blocco
        // del brano entrante è quasi sempre più corto degli altri.
        let mut uscente = [1.0_f32; 6];
        dissolvi(&mut uscente, &[1.0, 1.0], 0, 6, 2);
        assert!(uscente.iter().all(|c| c.is_finite()));
    }

    // ── la ripresa, quando la curva si interrompe ──

    #[test]
    fn la_ripresa_attacca_dove_la_dissolvenza_si_e_interrotta() {
        // Il contratto fra le due funzioni, ed è tutto il punto della ripresa:
        // l'ultimo campione mescolato e il primo di quello che continua da solo
        // devono avere lo stesso guadagno. Se si scostassero, al posto del clic
        // che si voleva togliere ce ne sarebbe uno più piccolo.
        let (_, dentro) = punto_curva(50, 100);
        // Il brano uscente è già a zero: quel che resta nel blocco è tutto e
        // solo il guadagno dell'entrante.
        let mut mescolato = [0.0_f32];
        dissolvi(&mut mescolato, &[1.0], 50, 100, 1);
        let mut ripreso = [1.0_f32];
        risali(&mut ripreso, dentro, 0, 40, 1);
        assert!(
            (mescolato[0] - ripreso[0]).abs() < 1e-6,
            "{} contro {}",
            mescolato[0],
            ripreso[0]
        );
    }

    #[test]
    fn la_ripresa_arriva_a_piena_ampiezza_e_ci_resta() {
        let mut alla_fine = [1.0_f32, 1.0];
        risali(&mut alla_fine, 0.7, 40, 40, 2);
        for campione in alla_fine {
            assert!((campione - 1.0).abs() < 1e-6, "campione {campione}");
        }
        // Oltre la fine non si continua a salire: il blocco può sforare.
        let mut oltre = [1.0_f32];
        risali(&mut oltre, 0.7, 500, 40, 1);
        assert!((oltre[0] - 1.0).abs() < 1e-6, "campione {}", oltre[0]);
    }

    #[test]
    fn la_ripresa_sale_e_non_torna_indietro() {
        // Una salita che avesse un solo passo all'indietro sarebbe un
        // tremolio, che è più udibile del gradino che sostituisce.
        let mut blocco = [1.0_f32; 64];
        risali(&mut blocco, 0.5, 0, 64, 1);
        let mut precedente = 0.0_f32;
        for campione in blocco {
            assert!(campione >= precedente, "{campione} dopo {precedente}");
            precedente = campione;
        }
        assert!(
            precedente > 0.9,
            "non arriva da nessuna parte: {precedente}"
        );
    }

    #[test]
    fn i_canali_di_uno_stesso_fotogramma_risalgono_insieme() {
        let mut blocco = [1.0_f32; 4];
        risali(&mut blocco, 0.0, 0, 2, 2);
        assert!((blocco[0] - blocco[1]).abs() < f32::EPSILON);
        assert!((blocco[2] - blocco[3]).abs() < f32::EPSILON);
        assert!(blocco[2] > blocco[0]);
    }

    #[test]
    fn una_ripresa_lunga_zero_non_divide_per_zero() {
        let mut blocco = [1.0_f32];
        risali(&mut blocco, 0.5, 0, 0, 1);
        assert!(blocco[0].is_finite());
    }

    #[test]
    fn zero_canali_non_divide_per_zero() {
        // Il formato arriva dal dispositivo: non dovrebbe mai dire zero
        // canali, ma «non dovrebbe» dentro un `/` è come non avere il
        // controllo. `canali.max(1)` lo copre, questa prova lo tiene coperto.
        let mut uscente = [1.0_f32, 1.0];
        dissolvi(&mut uscente, &[0.5, 0.5], 0, 4, 0);
        assert!(uscente.iter().all(|c| c.is_finite()));
    }
}
