//! La scheda audio, e la callback che la riempie.
//!
//! # Perché questo file è scritto con più cautela degli altri
//!
//! La callback gira su un filo che il sistema operativo tratta come realtime:
//! ha qualche millisecondo per consegnare il blocco successivo, e se non ce la
//! fa il suono si spezza. Da qui tre divieti che valgono **solo qui** e che
//! altrove sarebbero pedanteria:
//!
//! - **niente allocazioni** — `malloc` può prendere un lucchetto globale;
//! - **niente lucchetti** — il filo che lo tiene può essere sospeso dallo
//!   scheduler, e la callback lo aspetterebbe oltre la sua scadenza;
//! - **niente panici** — non perché il processo cadrebbe (il profilo è
//!   `panic = "unwind"`), ma perché questo filo è del dispositivo audio, non
//!   nostro: un panico qui srotola dentro `cpal`, il flusso muore e i `Drop` di
//!   quel che la callback teneva in mano si eseguono su un filo di cui non
//!   sappiamo niente. E non c'è nessuno a raccogliere i pezzi:
//!   `motore::filo_sorvegliato` sorveglia il filo di **decodifica**, che è un
//!   altro.
//!
//! Per questo la comunicazione con il resto del motore passa solo da atomiche e
//! da un anello senza lucchetti, e ogni lettura dall'anello ha un valore di
//! ripiego invece di una condizione d'errore.
//!
//! # Il flusso resta acceso in pausa — ma non per sempre
//!
//! In pausa la callback scrive silenzio invece di fermare il flusso. Fermarlo
//! costa: su alcuni driver la ripartenza chiede decine di millisecondi, e un
//! «riprendi» deve rispondere subito. Silenzio a flusso acceso è istantaneo, e
//! ha il vantaggio che l'anello resta pieno — riprendere non deve aspettare che
//! il decodificatore lo riempia.
//!
//! Fino alla 2.x questo valeva **per sempre**, e il prezzo lo pagava il
//! computer: un flusso WASAPI acceso tiene in piedi una richiesta di
//! alimentazione — `powercfg /requests` la mostra come «An audio stream is
//! currently in use» — e Windows non va in sospensione finché c'è. Un Rymembra
//! lasciato in pausa la sera teneva sveglio il portatile tutta la notte, e la
//! callback girava a vuoto cento volte al secondo per scrivere zeri.
//!
//! Adesso il flusso **si mette a riposo** — `pause()` di cpal, cioè
//! `IAudioClient::Stop` — dopo trenta secondi di pausa ([`RIPOSO_DOPO`]), e
//! quasi subito quando non c'è più niente da suonare: la coda finita, il
//! lettore fermato, o niente mai suonato dall'apertura. La regola è una
//! funzione pura, [`Sonno::passo`], e la applica il filo che tiene il flusso.
//! Si riprende con [`Uscita::sveglia`], che il motore chiama prima di ogni
//! comando che ha bisogno della callback: vedi la sua documentazione per
//! quali, e perché l'ordine conta.
//!
//! # Sul Bluetooth, la pausa libera le cuffie subito
//!
//! Trenta secondi andavano bene finché il flusso acceso costava soltanto
//! corrente. Con le cuffie Bluetooth *multipoint* — collegate al computer e al
//! telefono insieme — costa le cuffie: finché il computer ci manda silenzio il
//! collegamento A2DP resta suo, e il telefono che comincia a suonare in quei
//! trenta secondi non le trova libere. Chi metteva in pausa Rymembra per guardare
//! un video sul telefono lo sentiva uscire dall'altoparlante del telefono.
//!
//! Così su un'uscita Bluetooth il riposo arriva dopo [`RIPOSO_BLUETOOTH`], cioè
//! appena la porta si è chiusa e il buffer ha finito di suonare. Sulle altre
//! no: un ricevitore HDMI o ottico si riaggancia al segnale a ogni ripartenza e
//! si mangia il primo pezzo di musica, e lì i trenta secondi restano la
//! difesa della ripresa. Lo stesso prezzo il Bluetooth lo pagava già dopo
//! trenta secondi di pausa; adesso lo paga prima, in cambio di cuffie che
//! seguono chi suona.
//!
//! # Niente gradini
//!
//! Un campione che passa di colpo da metà onda a zero è un gradino, e un
//! gradino è un clic. Qui se ne producevano tre: in pausa, alla ripresa e a ogni
//! salto, perché i rami della pausa e dello svuotamento scrivevano zeri al
//! primo blocco utile. Adesso il suono passa da una **porta** (vedi
//! [`Andamento::porta`]) che si chiude in una dozzina di millisecondi prima di
//! tacere e si riapre allo stesso passo; e ogni volta che la porta si chiude
//! del tutto anche il volume riparte da zero, così la ripresa sale con la rampa
//! che c'era già.
//!
//! # Perché l'equalizzatore sta qui e non prima dell'anello
//!
//! Filtrare nel filo che decodifica sarebbe molto più semplice: nessun vincolo
//! di tempo reale, e lo stato dei filtri dentro una struttura normale. Ma
//! l'anello vale duecento millisecondi, quindi ogni cursore risponderebbe un
//! quinto di secondo dopo il dito — e un equalizzatore che risponde in ritardo
//! mentre lo si trascina sembra rotto.
//!
//! Filtrare qui costa un secondo anello, quello dei coefficienti: cinquanta
//! numeri in virgola mobile non si pubblicano con delle atomiche senza che
//! qualcuno possa leggerne metà di una curva e metà dell'altra.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait as _, StreamTrait as _};
use rymembra_domain::errors::{AppError, ErrorCode};

use crate::ancora::AncoraGrezza;
use crate::equalizzatore::{Coefficienti, Stato as StatoEq};
use crate::{Condiviso, limita};

/// La forma con cui il dispositivo si è aperto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatoUscita {
    /// Fotogrammi al secondo.
    pub frequenza: u32,
    /// Quanti canali.
    pub canali: u16,
}

/// Il ponte fra l'orologio del flusso e quello del processo.
///
/// # Perché serve
///
/// Perché `cpal` dice **quando** un blocco suonerà sull'orologio del flusso
/// (`StreamInstant`), che non ha un'epoca nota a nessuno, mentre per stare a
/// tempo con un altro computer serve l'orologio di [`crate::tempo`], l'unico
/// che si possa scrivere e confrontare. Il ponte è una coppia di letture degli
/// stessi istanti sui due orologi: da lì in poi ogni istante del flusso si
/// porta sull'altro con una somma.
///
/// Si misura una volta, appena il flusso è costruito, e non cambia per tutta la
/// sua vita: su WASAPI i due orologi sono lo **stesso** contatore di
/// prestazioni del sistema (`QueryPerformanceCounter`, a passi di cento
/// nanosecondi dalla parte di `cpal`), quindi la distanza fra i due è fissa e
/// misurarla di nuovo non insegnerebbe niente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ponte {
    /// Un istante sull'orologio del flusso, in nanosecondi
    /// (`StreamInstant::as_nanos`).
    pub flusso_ns: u64,
    /// Lo stesso istante sull'orologio del processo ([`crate::tempo`]).
    pub locale_ns: u64,
}

impl Ponte {
    /// Il ponte da tre letture fatte una dopo l'altra: l'orologio del
    /// processo, quello del flusso, di nuovo quello del processo.
    ///
    /// La lettura del flusso sta in mezzo alle altre due, e il suo gemello
    /// locale è il **punto medio**: l'errore è al più metà del tempo che separa
    /// le due letture locali — qualche centinaio di nanosecondi — invece di
    /// tutto.
    #[expect(
        clippy::integer_division,
        reason = "metà di un intervallo in nanosecondi: mezzo nanosecondo non \
                  si misura"
    )]
    #[must_use]
    pub const fn misurato(prima_ns: u64, flusso_ns: u64, dopo_ns: u64) -> Self {
        let locale_ns = prima_ns.saturating_add(dopo_ns.saturating_sub(prima_ns) / 2);
        Self {
            flusso_ns,
            locale_ns,
        }
    }

    /// Un istante dell'orologio del flusso, sull'orologio del processo.
    ///
    /// Una somma con segno: gli istanti del flusso possono stare prima o dopo
    /// quello del ponte. Fuori dal `u64` si resta al bordo invece di traboccare.
    #[must_use]
    pub fn locale_da_flusso(&self, flusso_ns: u64) -> u64 {
        let spostato = i128::from(self.locale_ns)
            .saturating_add(i128::from(flusso_ns))
            .saturating_sub(i128::from(self.flusso_ns))
            .max(0);
        u64::try_from(spostato).unwrap_or(u64::MAX)
    }
}

/// Quanto suono tenere pronto davanti alla callback, nel caso peggiore.
///
/// Duecento millisecondi: abbastanza perché una pausa dello scheduler o una
/// lettura lenta dal disco non si sentano, poco abbastanza perché il cursore
/// non menta di più di così su dove siamo arrivati.
///
/// **Duecento è il minimo garantito, non la riserva vera.** L'anello si
/// dimensiona prima di sapere come si aprirà il dispositivo, quindi su questo
/// numero moltiplica il caso peggiore — 192 kHz per otto canali, vedi
/// `Motore::avvia`. Su un'uscita normale, 48 kHz stereo, quegli stessi campioni
/// sono **circa tre secondi e due decimi** di musica. È il margine su cui si
/// giudica se uno stallo del filo di decodifica si sente o no, e vale la pena
/// leggerlo di lì e non da qui.
pub const RISERVA_MS: u64 = 200;

/// Oltre quanti millisecondi la latenza misurata non si crede più.
///
/// Duecentocinquanta. Non è il tetto di una catena d'uscita vera — un'uscita
/// Bluetooth ne fa molti di più — è il tetto di quel che *questa misura* può
/// dire sensatamente: il numero che arriva da [`annota_latenza`] è la durata del
/// buffer del dispositivo più la latenza che il motore audio di Windows
/// dichiara per il flusso, cioè qualche decina di millisecondi nel caso
/// peggiore. Un quarto di secondo lì dentro non è una catena lenta, è un
/// orologio del dispositivo che ha riportato una sciocchezza — e compensare una
/// sciocchezza sposta il cursore e i testi di un quarto di secondo senza che
/// nessuno possa capire perché.
///
/// Sopra questa soglia la misura si butta e resta solo la correzione a mano, che
/// è un numero che qualcuno ha scelto guardando l'effetto.
pub const LATENZA_MASSIMA_MS: u64 = 250;

/// Dopo quanto di pausa ininterrotta il flusso si mette a riposo.
///
/// Trenta secondi. Abbastanza perché chi mette in pausa per rispondere al
/// telefono ritrovi la ripresa istantanea di sempre; poco rispetto ai minuti
/// dopo cui Windows decide di sospendersi, che è il tempo che il riposo esiste
/// per non rubare. Oltre, riprendere costa il risveglio del flusso — qualche
/// decina di millisecondi, vedi [`Uscita::sveglia`] — dopo un silenzio che
/// nessuno sta misurando.
///
/// Vale per le uscite che **non** sono Bluetooth: per quelle vedi
/// [`RIPOSO_BLUETOOTH`] e il `//!` in testa al file.
pub const RIPOSO_DOPO: Duration = Duration::from_secs(30);

/// Dopo quanto di pausa il flusso si mette a riposo, su un'uscita Bluetooth.
///
/// Un secondo, per la stessa ragione di [`RIPOSO_A_VUOTO`]: la porta deve aver
/// finito di chiudersi e il buffer del dispositivo deve aver suonato quel che
/// aveva. Il filo dell'uscita guarda una volta al secondo ([`PASSO_VEGLIA`]),
/// quindi le cuffie tornano libere fra uno e due secondi dopo la pausa: meno
/// di quanto serve a prendere in mano il telefono e aprire un video.
pub const RIPOSO_BLUETOOTH: Duration = Duration::from_secs(1);

/// Dopo quanto il flusso si mette a riposo quando non c'è più niente da suonare.
///
/// «Niente da suonare» è la pausa con l'anello vuoto: la coda finita, il
/// lettore fermato. Lì aspettare trenta secondi non serve a nessuno — non c'è
/// una ripresa istantanea da proteggere, perché la prossima musica passa per
/// l'apertura di un file.
///
/// Un secondo e non zero: la porta deve aver finito di chiudersi e il buffer
/// del dispositivo deve aver suonato fino in fondo quel che aveva. Fermare il
/// flusso con dentro l'ultima dozzina di millisecondi di un brano vorrebbe dire
/// ritrovarsela davanti alla musica successiva, perché `IAudioClient::Stop`
/// non svuota il buffer.
const RIPOSO_A_VUOTO: Duration = Duration::from_secs(1);

/// Ogni quanto il filo dell'uscita guarda la pausa, a flusso acceso.
///
/// Un secondo: la regola del riposo ragiona in decine di secondi, e un filo che
/// si sveglia una volta al secondo è nulla accanto alla callback, che a flusso
/// acceso gira cento volte al secondo.
const PASSO_VEGLIA: Duration = Duration::from_secs(1);

/// Ogni quanto il filo dell'uscita dà un'occhiata, a flusso a riposo.
///
/// È la rete di sicurezza e non la strada: la strada è [`Uscita::sveglia`], che
/// arriva subito. Questa passata serve al caso in cui qualcuno tolga la pausa
/// senza suonare il campanello — un giorno, un comando nuovo del motore che se
/// ne dimentica — e trasforma quel difetto da «silenzio per sempre» in «un
/// quarto di secondo di ritardo». Quattro risvegli al secondo di un filo che
/// non fa niente non tengono sveglio nessun computer: la richiesta di
/// alimentazione la tiene il flusso, non i fili.
const PASSO_SONNO: Duration = Duration::from_millis(250);

/// Quanto si aspetta, al massimo, il primo blocco di un flusso appena rimesso
/// in moto.
///
/// Di solito ne bastano dieci o venti: il tempo di `IAudioClient::Start` e di
/// un periodo del dispositivo. Il tetto c'è per il dispositivo che non risponde
/// più — lì la callback d'errore ha già detto, o dirà, che è sparito — e
/// aspettarlo oltre vorrebbe dire tenere fermo chi ha premuto «play».
const ATTESA_RISVEGLIO: Duration = Duration::from_millis(250);

/// Quanto dura la porta, in millisecondi: la dissolvenza di pausa, ripresa e
/// salto.
///
/// Dodici. Sotto i dieci il gradino tolto torna a sentirsi come un «tic» più
/// morbido; sopra i venti la pausa comincia a sentirsi in ritardo rispetto al
/// dito. In mezzo, l'orecchio sente un taglio netto e nient'altro.
const DISSOLVENZA_MS: u64 = 12;

/// Quanto in fretta il guadagno raggiunge il valore voluto, per campione.
///
/// Una rampa e non un salto: cambiare il volume di colpo su un'onda a metà
/// corsa produce un gradino, e un gradino è un clic. Con questo coefficiente a
/// 48 kHz la corsa completa dura una quarantina di millisecondi — impercettibile
/// come ritardo, sufficiente a non sentire il gradino.
const RAMPA: f32 = 0.0005;

/// Sotto questa distanza dal voluto, la rampa ci salta sopra.
///
/// # Perché serve
///
/// Perché una rampa esponenziale **non arriva mai**: a ogni campione copre lo
/// 0,05% di quel che manca, e quel che manca non è mai zero. Verso un volume
/// alto è innocuo; verso lo zero — il silenziamento, il cursore tirato giù —
/// il guadagno scende sotto il più piccolo `f32` normale e diventa
/// **subnormale**. Sui processori x86 ogni operazione su un subnormale costa
/// decine di volte una normale, e qui le operazioni sono una per campione dentro
/// la callback in tempo reale: un volume a zero diventava il volume più caro da
/// suonare.
///
/// Un milionesimo è centoventi decibel sotto il fondo scala: nessun gradino di
/// quella misura è udibile, e il guadagno resta un numero normale per sempre.
const AGGANCIO: f32 = 1e-6;

/// Quanto si aspetta che il filo dell'uscita dica di aver aperto il dispositivo.
///
/// Venti secondi: un endpoint locale si apre in decine di millisecondi, uno
/// Bluetooth che si sta ancora negoziando il profilo in qualche secondo. Oltre,
/// non è un dispositivo lento ma un driver piantato, e la risposta giusta è un
/// errore che la riapertura sa riprovare — non un'attesa che non finisce.
const APERTURA_USCITA: Duration = Duration::from_secs(20);

/// La causa di perdita che vuol dire «il dispositivo non c'è più».
///
/// Il numero è quello della tabella in `Motore::perdita`: uno e due sono di
/// questo file, il tre del filo della decodifica.
const CAUSA_SPARITO: u32 = 1;

/// La causa di perdita che vuol dire «guasto del sistema audio».
const CAUSA_GUASTO: u32 = 2;

/// Il dispositivo aperto.
///
/// Tenere questo valore vivo è ciò che tiene aperto il flusso: lasciarlo cadere
/// lo chiude. Il flusso vive su un filo suo, `rymembra-uscita`, che è anche chi
/// decide quando metterlo a riposo e chi lo risveglia.
pub struct Uscita {
    /// Come si è aperto il dispositivo.
    pub formato: FormatoUscita,
    /// Su quale uscita si è aperto **davvero**.
    ///
    /// Non è per forza quella che si era chiesta: un nome che non c'è più fa
    /// ripiegare sul predefinito (vedi [`crate::dispositivi::scegli`]), e chi
    /// sorveglia deve poter confrontare questo con quel che si voleva per
    /// sapere se vale la pena riaprire quando il dispositivo torna.
    pub dispositivo: String,
    /// Quello aperto era il predefinito di sistema nell'istante dell'apertura.
    ///
    /// Serve a distinguere «sto sul predefinito perché è quel che volevo» da
    /// «sto sul predefinito perché quel che volevo non c'era».
    pub era_predefinito: bool,
    /// L'uscita aperta è Bluetooth, per quel che ne sa il sistema.
    ///
    /// Decide quanto presto la pausa la libera: vedi [`RIPOSO_BLUETOOTH`]. Lo
    /// dice `cpal`, che su Windows legge chi ha enumerato il dispositivo
    /// (`BTHENUM`); un'uscita che non sa dirlo vale come non Bluetooth, cioè
    /// tiene i trenta secondi di sempre.
    pub bluetooth: bool,
    /// Il ponte fra l'orologio di questo flusso e quello del processo, misurato
    /// appena il flusso è stato costruito. Vedi [`Ponte`].
    pub ponte: Ponte,
    /// Il campanello del filo che tiene il flusso.
    ///
    /// Non si clona e non esce di qui: lasciarlo cadere è il modo in cui il filo
    /// sa che deve chiudere il flusso e uscire, e un secondo mittente vivo da
    /// qualche altra parte lo terrebbe aperto per sempre.
    ///
    /// Dentro un `Arc` per una ragione sola, che non contraddice la riga sopra:
    /// fuori esce soltanto un [`Campanello`], cioè un riferimento **debole**.
    /// Il mittente forte resta uno, ed è questo: quando l'`Uscita` cade, cade
    /// lui, e il filo esce come prima.
    ordini: Arc<mpsc::Sender<Ordine>>,
}

/// Un modo di svegliare il flusso senza tenere in mano il motore.
///
/// # Il difetto che toglie
///
/// [`Uscita::sveglia`] può aspettare fino a mezzo secondo — il flusso a riposo
/// che riparte, un dispositivo che ci mette a rispondere — e la chiamano i
/// comandi del motore che svuotano l'anello. Nell'applicazione il motore sta
/// dietro il lucchetto del lettore, e due di quei comandi — il salto e lo
/// svuotamento della coda — girano sul filo della finestra: quel mezzo secondo
/// lo passavano col lucchetto in mano, e dietro di loro aspettavano gli
/// annunci del motore, l'orologio e il preparatore del brano dopo.
///
/// Il campanello si prende sotto il lucchetto, si suona **fuori**, e il comando
/// vero arriva dopo, di nuovo sotto il lucchetto: a quel punto il flusso è già
/// sveglio, e la sua `sveglia` è un giro di messaggio.
///
/// # Perché debole
///
/// Perché il mittente vivo è ciò che tiene aperto il flusso (vedi
/// [`Uscita::ordini`]): un campanello forte dimenticato da qualche parte
/// terrebbe acceso un dispositivo che nessuno usa più. Debole, si gonfia solo
/// per il tempo di una `send` e si sgonfia **prima** dell'attesa; se l'`Uscita`
/// nel frattempo è caduta, suonarlo non fa niente.
#[derive(Debug, Clone)]
pub struct Campanello {
    ordini: std::sync::Weak<mpsc::Sender<Ordine>>,
}

impl Campanello {
    /// Sveglia il flusso, se c'è ancora, e torna quando la callback gira — con
    /// lo stesso tetto di [`Uscita::sveglia`].
    pub fn suona(&self) {
        // Il mittente forte vive soltanto in questo blocco: per l'attesa basta
        // la risposta, e tenerlo oltre vorrebbe dire poter tenere aperto un
        // flusso che l'`Uscita` ha già lasciato cadere.
        let attendi = {
            let Some(ordini) = self.ordini.upgrade() else {
                return;
            };
            chiedi_la_sveglia(&ordini)
        };
        if let Some(attendi) = attendi {
            let _ = attendi.recv_timeout(ATTESA_RISVEGLIO.saturating_mul(2));
        }
    }
}

/// Manda l'ordine di sveglia e restituisce da dove arriverà la risposta.
///
/// `None` se il filo non c'è più: il flusso è già chiuso, e non c'è niente da
/// svegliare né nessuno da aspettare.
fn chiedi_la_sveglia(ordini: &mpsc::Sender<Ordine>) -> Option<mpsc::Receiver<()>> {
    let (risposta, attendi) = mpsc::sync_channel(1);
    ordini.send(Ordine::Sveglia(risposta)).ok()?;
    Some(attendi)
}

impl Uscita {
    /// Rimette in moto il flusso se riposava, e torna quando la callback gira.
    ///
    /// # Chi deve chiamarla, e quando
    ///
    /// Il motore, **prima** di ogni comando che ha bisogno della callback:
    ///
    /// - prima di togliere la pausa (`Motore::suona`, `Motore::riprendi`),
    ///   perché a flusso fermo la musica non uscirebbe;
    /// - prima di ogni comando che fa svuotare l'anello (`Motore::suona`,
    ///   `Motore::vai_a`, `Motore::ferma`), e questa è la ragione per cui torna
    ///   **solo quando la callback ha girato**. Lo svuotamento è una stretta di
    ///   mano — il filo della decodifica alza `svuota` e aspetta che la callback
    ///   lo abbassi, per un decimo di secondo al massimo — e a flusso fermo non
    ///   c'è nessuno a stringergliela: passato il decimo di secondo il filo va
    ///   avanti, riempie l'anello col punto nuovo, e la callback risvegliata dopo
    ///   lo svuota **tutto**, punto nuovo compreso. I fotogrammi spinti e non
    ///   contati restano un debito per sempre, e la fine della coda non si
    ///   dichiara più (vedi `Contesto::forse_fine`).
    ///
    /// Chiamarla a flusso acceso costa un giro di messaggio al filo
    /// dell'uscita, cioè niente; e azzera il conto del riposo, che è giusto: chi
    /// salta in pausa sta ancora ascoltando.
    ///
    /// # Quanto può aspettare
    ///
    /// A flusso acceso, il tempo di un messaggio. A flusso a riposo, il tempo
    /// che il dispositivo ci mette a ripartire — dieci, venti millisecondi — e
    /// mai più di mezzo secondo: oltre, il dispositivo non risponde e lo dirà la
    /// callback d'errore, non questa attesa.
    pub fn sveglia(&self) {
        if let Some(attendi) = chiedi_la_sveglia(&self.ordini) {
            let _ = attendi.recv_timeout(ATTESA_RISVEGLIO.saturating_mul(2));
        }
    }

    /// Il campanello da suonare senza tenere in mano l'`Uscita`: vedi
    /// [`Campanello`].
    #[must_use]
    pub fn campanello(&self) -> Campanello {
        Campanello {
            ordini: Arc::downgrade(&self.ordini),
        }
    }
}

/// Quel che si chiede al filo dell'uscita.
enum Ordine {
    /// Rimetti in moto il flusso, se riposa, e rispondi quando la callback gira.
    Sveglia(mpsc::SyncSender<()>),
}

/// Apre il dispositivo e ne tiene il flusso su un filo suo.
///
/// La frequenza e i canali **non si scelgono**: si accetta quel che il
/// dispositivo dichiara. In modalità condivisa su Windows chiederne altri
/// significa farsi convertire il suono da un livello su cui non abbiamo
/// controllo. Meglio prendere la forma del dispositivo e ricampionare noi, dove
/// si può misurare cosa succede.
///
/// Il flusso nasce **a riposo**: finché nessuno chiede di suonare non c'è
/// niente da far uscire, e un flusso acceso a vuoto è proprio quel che tiene
/// sveglio il computer. Lo mette in moto il primo [`Uscita::sveglia`].
pub(crate) fn apri(
    anello: rtrb::Consumer<f32>,
    coefficienti: rtrb::Consumer<Coefficienti>,
    campioni_spettro: rtrb::Producer<f32>,
    condiviso: Arc<Condiviso>,
    voluto: Option<String>,
) -> Result<Uscita, AppError> {
    let (manda_aperta, ricevi_aperta) = mpsc::channel();
    let (ordini, ricevi_ordini) = mpsc::channel::<Ordine>();
    // Una copia per chi resta fuori dalla rete qui sotto: vedi il commento sul
    // `catch_unwind`.
    let sorvegliante = Arc::clone(&condiviso);

    std::thread::Builder::new()
        .name("rymembra-uscita".to_owned())
        .spawn(move || {
            // # Perché una rete anche qui
            //
            // Perché questo filo, se cade, cade **in silenzio**. Il suo
            // `JoinHandle` non lo aspetta nessuno, e con lui se ne va il flusso
            // — il `Drop` di `cpal::Stream` chiude l'endpoint — senza che la
            // callback d'errore dica niente: per lei non è successo niente di
            // cui parlare. Il motore restava in piedi e sano a guardarlo, il
            // filo della decodifica riempiva un anello che nessuno svuotava, e
            // `perso` restava basso: il sorvegliante dei dispositivi non vedeva
            // niente da riaprire. Silenzio, e nessuno a raccontarlo.
            //
            // Quel che c'è dentro è codice di terzi che parla col driver —
            // `play`, `pause`, la costruzione del flusso — ed è da lì che un
            // panico può arrivare. Raccolto qui, alza la bandiera del motore
            // perso con la causa del guasto del sistema audio, che è la strada
            // già scritta per rimettere in piedi un motore nuovo. Un ritorno
            // normale invece — l'`Uscita` lasciata cadere — non alza niente.
            //
            // `AssertUnwindSafe` perché dopo un panico di quel che stava qui
            // dentro non si guarda più niente: il flusso e gli anelli se ne
            // vanno con l'unwind, e di fuori resta solo `Condiviso`, che sono
            // atomiche.
            let esito = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let veglia = Arc::new(Veglia::nuova());
                let anelli = Anelli {
                    campioni: anello,
                    coefficienti,
                    spettro: campioni_spettro,
                };
                match costruisci(anelli, &veglia, &condiviso, voluto.as_deref()) {
                    Ok((flusso, mut aperta)) => {
                        let riposo_in_pausa = if aperta.bluetooth {
                            RIPOSO_BLUETOOTH
                        } else {
                            RIPOSO_DOPO
                        };
                        // Il ponte qui, a flusso costruito e ancora fermo: la
                        // callback non gira finché qualcuno non suona il
                        // campanello, e il campanello si può suonare solo dopo
                        // che l'`Uscita` è arrivata a chi l'ha chiesta — cioè
                        // dopo la `send` qui sotto. Quando la callback legge
                        // il ponte, il ponte c'è.
                        let prima = crate::tempo::adesso_ns();
                        let del_flusso = ns_del_flusso(flusso.now());
                        let dopo = crate::tempo::adesso_ns();
                        let ponte = Ponte::misurato(prima, del_flusso, dopo);
                        veglia.posa_il_ponte(ponte);
                        aperta.ponte = ponte;
                        if manda_aperta.send(Ok(aperta)).is_err() {
                            return;
                        }
                        // Il flusso deve restare vivo qui: è la ragione per cui
                        // questo filo esiste invece di terminare subito. Esce —
                        // e il flusso con lui — quando l'`Uscita` viene
                        // lasciata cadere.
                        let sonno = Sonno::nuovo(riposo_in_pausa);
                        custodisci(&flusso, &ricevi_ordini, &condiviso, &veglia, sonno);
                    }
                    Err(err) => {
                        let _ = manda_aperta.send(Err(err));
                    }
                }
            }));
            // Solo se nessuno l'ha già detto: il panico arriva spesso **dopo**
            // la callback d'errore — il driver che sparisce e poi fa cadere
            // anche chi gli parla — e la sua causa, «dispositivo non più
            // disponibile», è più precisa del «guasto del sistema audio» che si
            // scriverebbe da qui. Sovrascriverla manderebbe il diario e il
            // banner a raccontare il guasto sbagliato.
            if esito.is_err() && !sorvegliante.perso.load(Ordering::Acquire) {
                segnala_perdita(&sorvegliante, CAUSA_GUASTO);
            }
        })
        .map_err(|err| {
            AppError::new(ErrorCode::PlaybackEngineUnavailable)
                .with_cause(format!("filo dell'uscita: {err}"))
        })?;

    // Con una scadenza: aprire un endpoint parla col driver, e un driver che
    // si pianta — un Bluetooth a metà accoppiamento, un DAC che si riavvia —
    // lasciava questa `recv` ad aspettare per sempre. Chi chiama tiene in mano
    // la bandiera della riapertura in corso, e con lei restava appesa ogni
    // riapertura futura, anche quella a mano. Il filo abbandonato non fa danni:
    // se l'apertura arriva dopo, la sua `send` trova il canale chiuso, e il
    // flusso se ne va con lui (vedi il ramo `Ok` qui sopra).
    let aperta = ricevi_aperta
        .recv_timeout(APERTURA_USCITA)
        .map_err(|err| {
            let causa = match err {
                mpsc::RecvTimeoutError::Timeout => format!(
                    "l'apertura del dispositivo non è finita entro {} secondi",
                    APERTURA_USCITA.as_secs()
                ),
                mpsc::RecvTimeoutError::Disconnected => {
                    "il filo dell'uscita è morto prima di aprire".to_owned()
                }
            };
            AppError::new(ErrorCode::PlaybackEngineUnavailable).with_cause(causa)
        })??;

    Ok(Uscita {
        formato: aperta.formato,
        dispositivo: aperta.dispositivo,
        era_predefinito: aperta.era_predefinito,
        bluetooth: aperta.bluetooth,
        ponte: aperta.ponte,
        ordini: Arc::new(ordini),
    })
}

/// Un istante del flusso in nanosecondi, nel `u64` di [`crate::tempo`].
///
/// Un `u128` di nanosecondi esce dal `u64` dopo cinque secoli di contatore: il
/// tetto è un ripiego che non si raggiunge.
fn ns_del_flusso(istante: cpal::StreamInstant) -> u64 {
    u64::try_from(istante.as_nanos()).unwrap_or(u64::MAX)
}

/// Quel che il filo dell'uscita rimanda indietro appena ha aperto.
///
/// Più campi e non uno perché il filo che apre è l'unico che vede il
/// dispositivo: dopo, il `Device` di cpal è già dentro il flusso, e nessuno può
/// più chiedergli come si chiama né com'è collegato.
struct Aperta {
    formato: FormatoUscita,
    dispositivo: String,
    era_predefinito: bool,
    bluetooth: bool,
    /// Il ponte fra gli orologi: lo misura il filo dell'uscita appena
    /// `costruisci` torna, e fino ad allora vale zero.
    ponte: Ponte,
}

/// Il lavoro del filo dell'uscita, finché l'`Uscita` è viva: rispondere al
/// campanello, e mettere il flusso a riposo quando è il momento.
///
/// La regola di *quando* sta in [`Sonno`], che non sa niente né di cpal né di
/// orologi veri; qui c'è solo chi la esegue.
fn custodisci(
    flusso: &cpal::Stream,
    ordini: &mpsc::Receiver<Ordine>,
    condiviso: &Condiviso,
    veglia: &Veglia,
    mut sonno: Sonno,
) {
    loop {
        let passo = if sonno.dorme {
            PASSO_SONNO
        } else {
            PASSO_VEGLIA
        };
        match ordini.recv_timeout(passo) {
            Ok(Ordine::Sveglia(risposta)) => {
                if sonno.sveglia() {
                    rimetti_in_moto(flusso, condiviso, veglia);
                }
                // Chi ha suonato può aver smesso di aspettare: la risposta
                // allora cade nel vuoto, e va bene così.
                let _ = risposta.send(());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let sguardo = Sguardo {
                    in_pausa: condiviso.in_pausa.load(Ordering::Acquire),
                    svuota: condiviso.svuota.load(Ordering::Acquire),
                    a_riposo: veglia.a_riposo.load(Ordering::Acquire),
                    sessione: condiviso.sessione.load(Ordering::Acquire),
                };
                match sonno.passo(sguardo, Instant::now()) {
                    Mossa::Niente => {}
                    // Se il flusso non si ferma, è perché il suo filo è già
                    // morto — e allora la callback d'errore ha già detto
                    // perché. Non c'è niente da aggiungere.
                    Mossa::Riposa => {
                        let _ = flusso.pause();
                        condiviso.dorme.store(true, Ordering::Release);
                    }
                    Mossa::Sveglia => rimetti_in_moto(flusso, condiviso, veglia),
                }
            }
            // L'`Uscita` è stata lasciata cadere: il flusso si chiude uscendo.
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Rimette in moto il flusso e aspetta il primo blocco, con un tetto.
///
/// Aspettare il blocco e non il ritorno di `play()` è tutto il punto: `play()`
/// su WASAPI accoda un comando al filo del flusso e torna, e fra il suo ritorno
/// e la prima callback c'è ancora `IAudioClient::Start` più un periodo del
/// dispositivo. Chi ha suonato il campanello vuole sapere che la callback
/// **gira** — vedi [`Uscita::sveglia`] per la stretta di mano che ne dipende.
fn rimetti_in_moto(flusso: &cpal::Stream, condiviso: &Condiviso, veglia: &Veglia) {
    let prima = veglia.giri.load(Ordering::Acquire);
    if let Err(err) = flusso.play() {
        // Il filo del flusso non c'è più. Di solito la callback d'errore l'ha
        // già detto; se non l'ha fatto lo si dice qui, altrimenti il
        // sorvegliante vedrebbe un motore sano che non suona.
        segnala_perdita(condiviso, codice_errore(err.kind()).unwrap_or(CAUSA_GUASTO));
        return;
    }
    // Sveglio da qui, e non da quando la callback gira: chi guarda questa
    // casella — `Motore::riprendi_a` — vuole sapere se il flusso è stato messo
    // a riposo, non se ha già servito il suo primo blocco.
    condiviso.dorme.store(false, Ordering::Release);
    let scadenza = Instant::now() + ATTESA_RISVEGLIO;
    while veglia.giri.load(Ordering::Acquire) == prima && Instant::now() < scadenza {
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Alza la bandiera del motore perso, con la causa **prima** della bandiera.
///
/// Nell'ordine di `Contesto::caduto`, che scrive le stesse due caselle per
/// l'altra causa: chi legge arriva dall'altra parte — vista `perso` alta, va a
/// chiedere perché — e scrivendole al contrario ci sarebbe una finestra in cui
/// la risposta è ancora zero, cioè «causa sconosciuta» nel diario e nel banner
/// proprio nell'istante che esistono per raccontare.
fn segnala_perdita(condiviso: &Condiviso, codice: u32) {
    condiviso.causa_perdita.store(codice, Ordering::Release);
    condiviso.perso.store(true, Ordering::Release);
}

/// Quel che il filo dell'uscita guarda a ogni passata.
#[derive(Debug, Clone, Copy)]
struct Sguardo {
    /// Il motore ha chiesto silenzio: pausa, fermo o coda finita.
    in_pausa: bool,
    /// Il filo della decodifica aspetta che la callback svuoti l'anello.
    svuota: bool,
    /// La callback ha appena servito un blocco di solo silenzio, con la porta
    /// chiusa e l'anello vuoto: non c'è più niente da suonare. Vedi
    /// [`Veglia::a_riposo`].
    a_riposo: bool,
    /// Una sessione d'ascolto insieme è aperta: vedi `Condiviso::sessione`.
    sessione: bool,
}

/// Cosa deve fare il filo dell'uscita dopo una passata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mossa {
    /// Niente di nuovo.
    Niente,
    /// Mettere il flusso a riposo.
    Riposa,
    /// Rimettere in moto il flusso: la rete di sicurezza di [`PASSO_SONNO`].
    Sveglia,
}

/// Quando il flusso deve riposare: la regola, senza cpal e senza orologi veri.
///
/// # Perché una macchina a stati a parte
///
/// Perché è l'unica parte di questa storia che si può provare. Il flusso vero
/// vuole una scheda audio, e la sospensione del computer si guarda con
/// `powercfg /requests` e non in un test; la **regola** invece — quanto
/// aspettare, cosa azzera il conto, quando non si dorme affatto — è fatta di
/// istanti e bandiere, e riceve gli uni e le altre da chi la chiama.
///
/// # La regola
///
/// - Nasce **a riposo**: niente è ancora stato suonato.
/// - Il campanello ([`Sonno::sveglia`]) sveglia e azzera ogni conto.
/// - A flusso acceso, ogni passata in cui il motore suona azzera i conti. Una
///   pausa ininterrotta di [`Sonno::riposo_in_pausa`] mette a riposo — trenta
///   secondi, uno sul Bluetooth; se in quella pausa non c'è più niente da
///   suonare — l'anello vuoto, cioè coda finita o lettore fermato — basta
///   [`RIPOSO_A_VUOTO`].
/// - Mai a riposo con uno svuotamento in corso: il filo della decodifica sta
///   aspettando proprio la callback.
/// - A flusso a riposo, se il motore suona o aspetta uno svuotamento senza aver
///   suonato il campanello, ci si sveglia lo stesso: è la rete di sicurezza.
///
/// I conti partono dalla **prima passata** che vede la pausa, non dall'istante
/// della pausa: il filo guarda una volta al secondo, e un secondo in più su
/// trenta non cambia niente a nessuno.
#[derive(Debug)]
struct Sonno {
    /// Il flusso è a riposo.
    dorme: bool,
    /// Da quando il motore tace senza interruzioni, a flusso acceso.
    zitto_da: Option<Instant>,
    /// Da quando, in quel silenzio, non c'è più niente da suonare.
    vuoto_da: Option<Instant>,
    /// Quanta pausa ininterrotta mette a riposo: [`RIPOSO_DOPO`], o
    /// [`RIPOSO_BLUETOOTH`] su un'uscita Bluetooth. Si sceglie una volta, quando
    /// il flusso si apre: un'uscita diversa è un flusso nuovo, e un `Sonno`
    /// nuovo.
    riposo_in_pausa: Duration,
}

impl Sonno {
    /// Appena aperto: a riposo, perché niente è ancora stato suonato.
    const fn nuovo(riposo_in_pausa: Duration) -> Self {
        Self {
            dorme: true,
            zitto_da: None,
            vuoto_da: None,
            riposo_in_pausa,
        }
    }

    /// Il campanello. `true` se il flusso riposava e va rimesso in moto.
    const fn sveglia(&mut self) -> bool {
        self.zitto_da = None;
        self.vuoto_da = None;
        let dormiva = self.dorme;
        self.dorme = false;
        dormiva
    }

    /// Una passata: guarda com'è il motore adesso e dice cosa fare.
    ///
    /// Con la sessione accesa non si riposa mai, nemmeno in pausa: chi ascolta
    /// insieme a un altro computer riprende a un istante preciso, e il
    /// risveglio del flusso — qualche decina di millisecondi che nessuno sa
    /// prevedere — non ci sta dentro. E se il flusso stava già riposando, la
    /// sessione lo sveglia, come la rete di sicurezza fa per chi suona.
    fn passo(&mut self, sguardo: Sguardo, adesso: Instant) -> Mossa {
        if self.dorme {
            if !sguardo.in_pausa || sguardo.svuota || sguardo.sessione {
                let _ = self.sveglia();
                return Mossa::Sveglia;
            }
            return Mossa::Niente;
        }
        if !sguardo.in_pausa || sguardo.sessione {
            self.zitto_da = None;
            self.vuoto_da = None;
            return Mossa::Niente;
        }
        let zitto_da = *self.zitto_da.get_or_insert(adesso);
        let vuoto_da = if sguardo.a_riposo {
            Some(*self.vuoto_da.get_or_insert(adesso))
        } else {
            self.vuoto_da = None;
            None
        };
        if sguardo.svuota {
            return Mossa::Niente;
        }
        let stanco = adesso.saturating_duration_since(zitto_da) >= self.riposo_in_pausa
            || vuoto_da.is_some_and(|da| adesso.saturating_duration_since(da) >= RIPOSO_A_VUOTO);
        if !stanco {
            return Mossa::Niente;
        }
        self.dorme = true;
        self.zitto_da = None;
        self.vuoto_da = None;
        Mossa::Riposa
    }
}

/// Quel che la callback racconta al filo dell'uscita, e a nessun altro.
///
/// Non sta in [`Condiviso`] perché il motore non lo guarda: è la conversazione
/// fra chi tiene il flusso e chi lo riempie, e resta dentro questo file.
struct Veglia {
    /// Quanti blocchi la callback ha servito: cresce di uno a ogni chiamata.
    ///
    /// Chi rimette in moto il flusso lo legge prima e aspetta che cambi — è così
    /// che sa che la callback gira davvero, e non soltanto che cpal ha ricevuto
    /// il comando. Vedi [`rimetti_in_moto`].
    giri: AtomicU64,
    /// L'ultimo blocco era tutto silenzio chiesto dal motore, con la porta
    /// chiusa e l'anello vuoto.
    ///
    /// È il segno che non c'è più niente da suonare. Una pausa normale ha
    /// l'anello **pieno** — il decodificatore lo riempie anche a musica ferma,
    /// così la ripresa è istantanea — mentre la coda finita e il lettore fermato
    /// lo lasciano vuoto: il primo per averlo consumato fino in fondo, il
    /// secondo per averlo svuotato. Un anello vuoto per un disco lento mentre si
    /// è in pausa viene scambiato per la fine, e il prezzo è una ripresa che
    /// deve svegliare il flusso: qualche millisecondo.
    a_riposo: AtomicBool,
    /// Il [`Ponte`], metà flusso: lo posa il filo dell'uscita prima che la
    /// callback giri la prima volta, e da lì non cambia più.
    ///
    /// # Perché passa di qui e non dentro la chiusura
    ///
    /// Perché la chiusura della callback si costruisce **prima** del flusso — è
    /// un argomento di `build_output_stream` — e il ponte si può misurare solo
    /// **dopo**, chiedendo l'ora al flusso costruito. Due atomici scritti una
    /// volta e letti con `Acquire` a ogni blocco sono il modo di consegnarglielo
    /// senza lucchetti: la callback lo ricompone in un [`Ponte`] per valore.
    ponte_flusso_ns: AtomicU64,
    /// Il [`Ponte`], metà processo.
    ponte_locale_ns: AtomicU64,
}

impl Veglia {
    /// Nessun giro, nessun riposo, nessun ponte.
    const fn nuova() -> Self {
        Self {
            giri: AtomicU64::new(0),
            a_riposo: AtomicBool::new(false),
            ponte_flusso_ns: AtomicU64::new(0),
            ponte_locale_ns: AtomicU64::new(0),
        }
    }

    /// Consegna il ponte alla callback. Una volta sola, prima che giri.
    fn posa_il_ponte(&self, ponte: Ponte) {
        self.ponte_flusso_ns
            .store(ponte.flusso_ns, Ordering::Release);
        self.ponte_locale_ns
            .store(ponte.locale_ns, Ordering::Release);
    }

    /// Il ponte, per la callback.
    fn ponte(&self) -> Ponte {
        Ponte {
            flusso_ns: self.ponte_flusso_ns.load(Ordering::Acquire),
            locale_ns: self.ponte_locale_ns.load(Ordering::Acquire),
        }
    }
}

/// I tre capi d'anello che la callback legge o riempie, prima che diventino
/// suoi.
///
/// Viaggiano insieme fino a [`costruisci`], che li mette in una [`Voce`] solo
/// quando sa la frequenza d'uscita: la porta si misura in fotogrammi, e i
/// fotogrammi di dodici millisecondi dipendono dalla frequenza.
struct Anelli {
    campioni: rtrb::Consumer<f32>,
    coefficienti: rtrb::Consumer<Coefficienti>,
    spettro: rtrb::Producer<f32>,
}

/// Apre l'uscita scelta e ne costruisce il flusso, **fermo**.
fn costruisci(
    anelli: Anelli,
    veglia: &Arc<Veglia>,
    condiviso: &Arc<Condiviso>,
    voluto: Option<&str>,
) -> Result<(cpal::Stream, Aperta), AppError> {
    // Si sceglie e si apre sulla **stessa** enumerazione: il `Device` che si
    // apre è quello accanto al nome scelto, non uno ripescato per nome da una
    // seconda lettura — fra le due il mondo poteva cambiare, e con due schede
    // dallo stesso nome il ripescaggio prendeva la prima anche quando si era
    // scelta l'altra. La regola di quale prendere sta tutta in
    // `dispositivi::scegli`, che è pura e provata.
    let mut uscite = crate::dispositivi::con_i_device();
    let posto = {
        let nomi: Vec<crate::Dispositivo> = uscite.iter().map(|(d, _)| d.clone()).collect();
        crate::dispositivi::scegli_posto(&nomi, voluto)
    };
    let (scelta, dispositivo) = posto
        .filter(|posto| *posto < uscite.len())
        .map(|posto| uscite.swap_remove(posto))
        .ok_or_else(|| {
            AppError::new(ErrorCode::PlaybackEngineUnavailable)
                .with_cause("nessun dispositivo di uscita".to_owned())
        })?;

    let configurazione = dispositivo
        .default_output_config()
        .map_err(|err| errore_di_apertura(&err))?;

    let formato = FormatoUscita {
        frequenza: configurazione.sample_rate(),
        canali: configurazione.channels(),
    };
    let campione = configurazione.sample_format();
    let configurazione: cpal::StreamConfig = configurazione.into();
    // Chiesto qui, al `Device` che si sta aprendo, perché dopo non lo si può
    // più chiedere a nessuno: vedi [`Aperta`]. Un dispositivo che non sa
    // descriversi vale come non Bluetooth, che è il comportamento di prima.
    let bluetooth = dispositivo
        .description()
        .is_ok_and(|d| d.interface_type() == cpal::InterfaceType::Bluetooth);

    let voce = Voce {
        lettore: anelli.campioni,
        andamento: Andamento::nuovo(formato.frequenza),
        filtro: Filtro {
            stato: StatoEq::nuovo(),
            ricevi: anelli.coefficienti,
        },
        spia: Spia {
            manda: anelli.spettro,
            somma: 0.0,
            quanti: 0,
        },
        veglia: Arc::clone(veglia),
    };
    let stato = Arc::clone(condiviso);

    let su_errore = {
        let stato = Arc::clone(condiviso);
        move |err: cpal::Error| {
            // Il dispositivo è sparito o si è guastato: cuffie staccate,
            // scheda cambiata. Non c'è niente da fare qui dentro se non
            // lasciarne traccia, perché riaprire va fatto da un filo che può
            // bloccarsi. Gli avvisi che non fermano il flusso — un buco nel
            // buffer, la priorità in tempo reale negata — non sono una
            // perdita, e scambiarli per una vorrebbe dire ricostruire il
            // motore per un singhiozzo.
            //
            // Il buco nel buffer però si conta: non ferma niente, ma per chi
            // misura quanto due computer suonano insieme è un pezzo di tempo
            // sparito, e senza contarlo uno sfasamento improvviso non avrebbe
            // una spiegazione.
            if matches!(err.kind(), cpal::ErrorKind::Xrun) {
                stato.xrun.fetch_add(1, Ordering::Relaxed);
            }
            if let Some(codice) = codice_errore(err.kind()) {
                segnala_perdita(&stato, codice);
            }
        }
    };

    // Il formato di campione che il dispositivo dichiara, se lo sappiamo
    // scrivere; altrimenti virgola mobile a 32 bit, che la modalità condivisa
    // di Windows accetta sempre e converte da sé.
    let flusso = match campione {
        cpal::SampleFormat::F64 => {
            flusso_di(&dispositivo, configurazione, voce, stato, su_errore, a_f64)
        }
        cpal::SampleFormat::I32 => {
            flusso_di(&dispositivo, configurazione, voce, stato, su_errore, a_i32)
        }
        cpal::SampleFormat::I24 => {
            flusso_di(&dispositivo, configurazione, voce, stato, su_errore, a_i24)
        }
        cpal::SampleFormat::I16 => {
            flusso_di(&dispositivo, configurazione, voce, stato, su_errore, a_i16)
        }
        cpal::SampleFormat::U16 => {
            flusso_di(&dispositivo, configurazione, voce, stato, su_errore, a_u16)
        }
        _ => flusso_di(&dispositivo, configurazione, voce, stato, su_errore, a_f32),
    }
    .map_err(|err| errore_di_apertura(&err))?;

    // **Niente `play()` qui.** Il flusso resta fermo finché qualcuno non chiede
    // di suonare: vedi [`apri`] e [`Uscita::sveglia`].
    Ok((
        flusso,
        Aperta {
            formato,
            dispositivo: scelta.id,
            era_predefinito: scelta.predefinito,
            bluetooth,
            ponte: Ponte::default(),
        },
    ))
}

/// Costruisce il flusso per un formato di campione, con la sua conversione.
///
/// Generico e non scritto sei volte: le sei versioni differirebbero per il tipo
/// del campione e per una moltiplicazione. `converti` è una funzione e non un
/// puntatore a funzione, così il compilatore la vede e la mette in linea dentro
/// [`riempi`] invece di chiamarla attraverso un indirizzo a ogni campione.
fn flusso_di<T, C>(
    dispositivo: &cpal::Device,
    configurazione: cpal::StreamConfig,
    mut voce: Voce,
    stato: Arc<Condiviso>,
    su_errore: impl FnMut(cpal::Error) + Send + 'static,
    converti: C,
) -> Result<cpal::Stream, cpal::Error>
where
    T: cpal::SizedSample,
    C: Fn(f32) -> T + Copy + Send + 'static,
{
    // La frequenza serve alla callback per tradurre in fotogrammi la latenza che
    // `cpal` riporta in tempo. È un `u32` `Copy`: se lo porta dentro.
    let frequenza = configurazione.sample_rate;
    dispositivo.build_output_stream(
        configurazione,
        move |dati: &mut [T], info: &cpal::OutputCallbackInfo| {
            // L'orologio del processo per primo: è la controprova dell'ancora,
            // e vale quanto è vicino all'istante in cui `cpal` ha letto il suo.
            let ingresso_ns = crate::tempo::adesso_ns();
            // I due istanti di `cpal` una volta sola: la latenza e il battito
            // devono parlare dello stesso blocco.
            let tempi = info.timestamp();
            annota_latenza(tempi, frequenza, &stato);
            let battito = Battito::di(tempi, voce.veglia.ponte(), frequenza, ingresso_ns);
            riempi(dati, &mut voce, &stato, &battito, converti);
        },
        su_errore,
        None,
    )
}

/// Quando suonerà il blocco che la callback sta riempiendo, sull'orologio del
/// processo.
///
/// # Perché un tipo e non `cpal::OutputCallbackInfo`
///
/// Perché quello lo costruisce solo il backend audio, con gli istanti del suo
/// orologio: una prova non può fabbricarlo. Questo invece è fatto di numeri, e
/// le prove di [`riempi`] ci scrivono il tempo che vogliono — un blocco che
/// suona fra un secondo, un avvio che cade a metà del blocco dopo.
#[derive(Debug, Clone, Copy, Default)]
struct Battito {
    /// Quando il primo fotogramma del blocco lascia il dispositivo, in
    /// nanosecondi di [`crate::tempo`]: il `playback` di `cpal`, portato di qua
    /// col [`Ponte`].
    playback_ns: u64,
    /// `playback − callback`: quanto il blocco aspetta nel buffer del
    /// dispositivo. Zero se `cpal` li dà al contrario.
    anticipo_ns: u64,
    /// La frequenza d'uscita.
    frequenza: u32,
    /// L'orologio del processo all'ingresso della callback: la base della
    /// controprova.
    ingresso_ns: u64,
}

impl Battito {
    /// Il battito di un blocco, dagli istanti di `cpal`.
    fn di(
        tempi: cpal::OutputStreamTimestamp,
        ponte: Ponte,
        frequenza: u32,
        ingresso_ns: u64,
    ) -> Self {
        let anticipo = tempi
            .playback
            .checked_duration_since(tempi.callback)
            .unwrap_or(Duration::ZERO);
        Self {
            playback_ns: ponte.locale_da_flusso(ns_del_flusso(tempi.playback)),
            anticipo_ns: u64::try_from(anticipo.as_nanos()).unwrap_or(u64::MAX),
            frequenza,
            ingresso_ns,
        }
    }
}

/// Annota quanti fotogrammi stanno fra questo blocco e le casse.
///
/// # Cosa misura, esattamente
///
/// `cpal` accompagna ogni blocco con due istanti: `callback`, «adesso», e
/// `playback`, «quando si sentirà il primo campione di questo blocco». La loro
/// differenza è la sola cosa che interessa, e usare **solo differenze** è il
/// pregio di questa misura: `StreamInstant` non ha un'epoca che noi conosciamo —
/// su Windows è il contatore di prestazioni del sistema — e nessuna epoca
/// condivisa serve, perché i due istanti vengono dalla stessa sorgente nello
/// stesso momento.
///
/// # Perché è una stima, e di che pezzo della catena
///
/// Su Windows `cpal` parla WASAPI, e `playback` non lo legge da nessuna parte:
/// lo **costruisce**. Prende `callback` da `IAudioClock::GetPosition`, ci somma
/// quel che è già stato consegnato al buffer del dispositivo e non ancora
/// suonato, e — da cpal 0.18 — la latenza che il motore audio di Windows
/// dichiara per il flusso (`IAudioClient::GetStreamLatency`).
///
/// Quindi il numero che esce di qui è **un periodo di dispositivo più la
/// latenza del motore di sistema** — qualche decina di millisecondi a 48 kHz in
/// modalità condivisa — e ancora non la catena intera: non ci sono dentro il
/// driver oltre quel che dichiara, la conversione digitale-analogica, né i
/// millisecondi di un DAC USB. Su un'uscita Bluetooth resta gravemente
/// sottostimato: là la latenza vera sta fra i cento e i duecento millisecondi,
/// e la radio non la dichiara a nessuno.
///
/// Per questo non è da sola: è il primo addendo, e il secondo lo dichiara
/// l'utente (la preferenza `audio.latenza_ms`). Quel che si guadagna misurando
/// è la parte che cambia da sé — un buffer che il driver allarga sotto carico —
/// senza chiedere a nessuno di riaggiustare un cursore.
///
/// # I tre divieti
///
/// Nessuna allocazione, nessun lucchetto, nessun panico: due letture di campi
/// `Copy`, un'aritmetica intera e uno `store` rilassato. Vedi il `//!` in testa
/// al file.
fn annota_latenza(tempi: cpal::OutputStreamTimestamp, frequenza: u32, condiviso: &Condiviso) {
    // `None` quando `playback` precede `callback`, che da cpal 0.18 non
    // dovrebbe succedere più — `playback` è tenuto monotono — e su un orologio
    // costruito a mano non è impossibile lo stesso. In quel caso si lascia
    // l'ultimo valore noto invece di scrivere zero: zero direbbe «nessuna
    // latenza», che è un'affermazione, mentre qui non si sa niente di nuovo.
    let Some(anticipo) = tempi.playback.checked_duration_since(tempi.callback) else {
        return;
    };
    condiviso
        .latenza_fotogrammi
        .store(fotogrammi_di(anticipo, frequenza), Ordering::Relaxed);
}

/// Quanti fotogrammi stanno in una durata, alla frequenza d'uscita.
///
/// In microsecondi e non in secondi in virgola mobile: la callback non deve
/// toccare la FPU per una conversione che in interi è esatta, e un buffer da
/// dieci millisecondi a 48 kHz fa 480 fotogrammi senza arrotondamenti da
/// giustificare.
#[expect(
    clippy::integer_division,
    reason = "il resto è meno di un fotogramma, cioè meno di un campione su \
              quarantottomila al secondo"
)]
fn fotogrammi_di(anticipo: Duration, frequenza: u32) -> u64 {
    let micro = u64::try_from(anticipo.as_micros()).unwrap_or(u64::MAX);
    micro.saturating_mul(u64::from(frequenza)) / 1_000_000
}

/// Da campione normalizzato a campione normalizzato, ma tagliato.
///
/// Sembra la funzione identità e per quasi tutti i campioni lo è. Non lo è per
/// quelli fuori scala, e fuori scala ci si va davvero: la correzione
/// ReplayGain arriva fino a +12 dB, cioè quasi il quadruplo, e il bersaglio
/// «alto» della normalizzazione — quello delle piattaforme di streaming — la
/// porta lì su qualunque brano già forte.
///
/// Dalla 3.0 ci arrivano già piegati da [`limita`] quando la catena alza, e il
/// taglio resta per quel che sfugge: i campioni oltre il fondo scala che un
/// decodificatore con perdita produce da sé, a guadagno pieno.
///
/// Sulle uscite a interi il taglio c'è dentro ogni conversione; su questa, che
/// è il formato che WASAPI offre quasi sempre, i campioni uscivano come
/// venivano. Cosa ne faccia il sistema operativo non è scritto da nessuna parte
/// — di solito taglia, ma «di solito» non è una garanzia su cui costruire il
/// suono, e un driver che invece avvolge produce uno schianto a piena ampiezza
/// esattamente sui picchi.
///
/// Un campione che non è un numero diventa silenzio. `clamp` un `NaN` lo
/// lascia passare com'è, e sulle uscite a interi la conversione `as` lo porta
/// a zero da sola; qui no, e un `NaN` consegnato a WASAPI è un campione che il
/// driver interpreta come vuole. Da dove verrebbe: un decodificatore che
/// sbaglia su un file rovinato, un filtro che diverge — rari, ma questa è
/// l'ultima porta prima delle casse.
fn a_f32(v: f32) -> f32 {
    if v.is_nan() {
        return 0.0;
    }
    v.clamp(-1.0, 1.0)
}

/// Da campione normalizzato a virgola mobile a 64 bit, tagliato come [`a_f32`].
fn a_f64(v: f32) -> f64 {
    f64::from(a_f32(v))
}

/// Da campione normalizzato a intero con segno a 16 bit.
///
/// Il taglio **prima** della moltiplicazione, e non è pignoleria: un valore
/// fuori scala moltiplicato per 32767 esce dall'intervallo di un `i16`, e quel
/// che se ne ricava non è un suono forte — è il segno che si ribalta, cioè uno
/// schianto a piena ampiezza esattamente sui picchi.
#[expect(
    clippy::cast_possible_truncation,
    reason = "dopo il taglio il valore sta in [-32767, 32767], dentro l'i16; e la \
              conversione da virgola mobile a intero satura invece di avvolgere"
)]
fn a_i16(v: f32) -> i16 {
    (v.clamp(-1.0, 1.0) * 32_767.0) as i16
}

/// Da campione normalizzato a intero senza segno a 16 bit.
///
/// Il formato senza segno mette lo zero a metà scala: si sposta l'origine di
/// 32768 invece di reinterpretare i bit.
fn a_u16(v: f32) -> u16 {
    // `a_i16` sta in [-32767, 32767], quindi la somma sta in [1, 65535]: dentro
    // un `u16` sempre, e il ripiego non si raggiunge mai.
    u16::try_from(i32::from(a_i16(v)).saturating_add(32_768)).unwrap_or(32_768)
}

/// Da campione normalizzato a intero con segno a 32 bit.
///
/// La moltiplicazione in `f64`: un `f32` ha ventiquattro bit di mantissa, e
/// il fondo scala di un `i32` non ci sta — `2147483647.0` in `f32` diventa
/// `2147483648.0`. In `f64` il conto è esatto, e il taglio a monte tiene il
/// risultato dentro l'intervallo come per [`a_i16`].
#[expect(
    clippy::cast_possible_truncation,
    reason = "dopo il taglio il valore sta in [-2147483647, 2147483647], dentro \
              l'i32; e la conversione da virgola mobile a intero satura"
)]
fn a_i32(v: f32) -> i32 {
    (f64::from(v.clamp(-1.0, 1.0)) * 2_147_483_647.0) as i32
}

/// Da campione normalizzato a intero con segno a 24 bit.
///
/// I 24 bit che cpal 0.18 sa consegnare a WASAPI. Prima un dispositivo che si
/// dichiarava in un formato diverso da `f32`, `i16` e `u16` non si apriva
/// affatto: il ramo dei formati non gestiti rispondeva con un errore. Il valore
/// sta in un `i32` e cpal lo sposta nei bit alti da sé.
#[expect(
    clippy::cast_possible_truncation,
    reason = "dopo il taglio il valore sta in [-8388607, 8388607], cioè dentro \
              i 24 bit; e la conversione da virgola mobile a intero satura"
)]
fn a_i24(v: f32) -> cpal::I24 {
    // Il ripiego — lo zero — non si raggiunge: il taglio tiene il valore dentro
    // l'intervallo che `I24::new` accetta. Ma qui dentro non esiste un modo
    // accettabile di cadere.
    cpal::I24::new((f64::from(v.clamp(-1.0, 1.0)) * 8_388_607.0) as i32).unwrap_or_default()
}

/// La presa dello spettro, come la vede la callback.
///
/// Un `push` che può fallire, e quando fallisce si perde. È la gerarchia
/// giusta: lo spettro è decorazione, il suono no — e l'unica alternativa a
/// perdere un campione sarebbe aspettare che qualcuno svuoti, cioè fare in
/// tempo reale l'unica cosa che il tempo reale vieta.
///
/// Somma i canali di un fotogramma per farne un campione solo: uno spettro
/// stereo sarebbe due grafici, e nessuno guarda due grafici.
struct Spia {
    manda: rtrb::Producer<f32>,
    somma: f32,
    quanti: u16,
}

impl Spia {
    /// Prende un campione di un canale; a fotogramma completo pubblica la media.
    fn campione(&mut self, valore: f32, per_fotogramma: usize) {
        self.somma += valore;
        self.quanti = self.quanti.saturating_add(1);
        if usize::from(self.quanti) >= per_fotogramma {
            let _ = self.manda.push(self.somma / f32::from(self.quanti));
            self.somma = 0.0;
            self.quanti = 0;
        }
    }

    /// Dimentica il fotogramma a metà: dopo uno svuotamento appartiene al punto
    /// di prima, esattamente come i campioni ancora nell'anello.
    const fn azzera(&mut self) {
        self.somma = 0.0;
        self.quanti = 0;
    }
}

/// Quel che la callback si porta dietro da un blocco all'altro.
///
/// Numeri che non stanno in [`Condiviso`] perché nessun altro filo li guarda, e
/// non sono variabili locali perché devono sopravvivere alla fine del blocco.
struct Andamento {
    /// Il guadagno raggiunto dalla rampa del volume, da cui riparte il blocco
    /// dopo.
    guadagno: f32,
    /// Quanto è aperta la porta, in fotogrammi: [`Andamento::corsa`] suona,
    /// zero tace.
    ///
    /// # Perché una porta, oltre alla rampa del volume
    ///
    /// Perché le due rispondono a domande diverse. La rampa insegue il cursore
    /// del volume, e insegue piano — è esponenziale, e verso lo zero non arriva
    /// mai da sola (vedi [`AGGANCIO`]). La porta invece deve **finire**: chi ha
    /// premuto pausa vuole silenzio fra una dozzina di millisecondi, non fra un
    /// tempo che dipende da quanto era alto il volume. Quindi è una retta, un
    /// fotogramma alla volta, che arriva a zero e lì si ferma.
    ///
    /// Un intero e non un `f32`: sommare e togliere un passo in virgola mobile
    /// lascia residui — un «quasi zero» che non è zero, un «quasi uno» che non è
    /// uno — mentre contando fotogrammi gli estremi sono esatti, e la porta
    /// chiusa dura esattamente quel che deve.
    ///
    /// Si muove per **fotogramma** e non per campione: i canali di uno stesso
    /// istante escono con lo stesso guadagno, e un fotogramma si consuma dall'
    /// anello tutto o niente — che è quel che il conto dei fotogrammi dà per
    /// buono.
    porta: u32,
    /// Quanti fotogrammi stanno in [`DISSOLVENZA_MS`] alla frequenza d'uscita:
    /// la porta tutta aperta.
    corsa: u32,
    /// I campioni di un fotogramma servito a metà.
    ///
    /// # Perché non si possono buttare
    ///
    /// Perché l'anello può svuotarsi **in mezzo a un fotogramma**: allora i
    /// campioni presi non sono un multiplo dei canali, e `presi / canali` scarta
    /// il resto. Quei campioni sono usciti dalle casse lo stesso, e i loro
    /// compagni usciranno al blocco dopo — ma il fotogramma che formano insieme
    /// non verrebbe contato da nessuno dei due.
    ///
    /// L'errore non si compensa: va sempre nella stessa direzione, e si somma a
    /// ogni interruzione. Su un disco lento il cursore mente in difetto, sempre
    /// di più, senza che niente lo segnali.
    resto: u64,
    /// A che punto è il gradino in corso, se ce n'è uno. Vedi [`Fase`].
    fase: Fase,
    /// La deriva accumulata, in milionesimi di fotogramma.
    ///
    /// # Perché la deriva si corregge qui
    ///
    /// Perché due schede audio non contano il tempo allo stesso passo: un
    /// quarzo da 48 kHz che ne fa 48 004,8 è avanti di cento parti per milione,
    /// cioè di sei millisecondi al minuto. Il ricampionatore non lo può
    /// assorbire — `FftFixedIn` di rubato lavora a rapporto fisso — e
    /// ricampionare l'uscita intera per un decimillesimo sarebbe un filtro in
    /// più dentro il percorso in tempo reale per niente.
    ///
    /// Si fa come i lettori di rete: a ogni fotogramma di musica a porta aperta
    /// si sommano le parti per milione chieste, e ogni volta che la somma passa
    /// un fotogramma intero se ne toglie uno — due fotogrammi d'anello escono
    /// come la loro media — o se ne aggiunge uno — la media fra l'ultimo uscito
    /// e il prossimo, senza consumarlo. Una media e non un fotogramma buttato o
    /// ripetuto: il gradino fra due campioni lontani è un clic, la media fra
    /// due vicini non si sente. A cinquecento parti per milione — il tetto —
    /// succede una volta ogni duemila fotogrammi, cioè ventiquattro volte al
    /// secondo a 48 kHz.
    accumulo: i64,
    /// I campioni dell'ultimo fotogramma uscito dall'anello, così come vi
    /// erano: la prima metà della media di un fotogramma aggiunto.
    ultimo: [f32; CANALI_DERIVA],
}

/// Quanti canali al massimo può avere un'uscita perché la deriva la corregga.
///
/// Trentadue: più di ogni scheda che si trova in una casa. Oltre, la deriva
/// semplicemente non si applica, invece di chiedere memoria alla callback.
const CANALI_DERIVA: usize = 32;

/// Il tetto della deriva, in parti per milione.
///
/// Cinquecento: dieci volte lo scarto dei quarzi peggiori. Oltre non è una
/// deriva da correggere, è una frequenza sbagliata, e un fotogramma medio ogni
/// meno di duemila comincerebbe a sentirsi.
pub const DERIVA_MASSIMA_PPM: i32 = 500;

/// Un fotogramma intero, nell'unità di [`Andamento::accumulo`].
const MILIONE: i64 = 1_000_000;

/// A che punto è un gradino.
///
/// # Cos'è un gradino
///
/// Uno spostamento netto della musica rispetto al tempo: un pezzo di silenzio
/// inserito, o un pezzo di musica saltato. Serve a chi sta a tempo con un altro
/// computer e scopre di essere avanti o indietro di qualche decina di
/// millisecondi — troppo per la [`Andamento::accumulo`], che ne recupera mezzo
/// al secondo.
///
/// Riusa la porta della pausa, perché ha lo stesso problema: un taglio a metà
/// onda è un clic. Quindi la porta si chiude in [`DISSOLVENZA_MS`], a porta
/// chiusa si salta o si tace, e si riapre allo stesso passo.
///
/// # L'esattezza
///
/// Il fotogramma in cui la porta arriva a zero **consuma** un fotogramma
/// d'anello, a guadagno zero, invece di tacere come nella pausa. Così il conto
/// torna al fotogramma: un salto di N sposta la musica di N esatti, e un
/// silenzio di N inserisce N fotogrammi che non consumano niente — né uno di
/// più per la porta, né uno di meno.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fase {
    /// Nessun gradino: la porta segue la pausa e basta.
    Libera,
    /// La porta si chiude per un gradino di `quanto` fotogrammi: positivo di
    /// silenzio, negativo di musica saltata. La musica che sfuma si consuma e
    /// si conta come sempre.
    Chiude { quanto: i64 },
    /// A porta chiusa. `resto` positivo: fotogrammi di silenzio ancora da
    /// dare; negativo: fotogrammi da saltare che l'anello non aveva ancora.
    Esegue { resto: i64 },
    /// La porta si riapre, al passo con cui si è chiusa.
    Riapre,
}

impl Andamento {
    /// Muto e a porta chiusa: il primo suono salirà da zero.
    fn nuovo(frequenza: u32) -> Self {
        Self {
            guadagno: 0.0,
            porta: 0,
            corsa: corsa_della_porta(frequenza),
            resto: 0,
            fase: Fase::Libera,
            accumulo: 0,
            ultimo: [0.0; CANALI_DERIVA],
        }
    }

    /// Un fotogramma di porta: verso l'apertura se si suona, verso la chiusura
    /// se si tace. Agli estremi si resta fermi.
    fn muovi_porta(&mut self, suona: bool) {
        self.porta = if suona {
            self.porta.saturating_add(1).min(self.corsa)
        } else {
            self.porta.saturating_sub(1)
        };
    }

    /// Quanto lascia passare la porta, fra zero e uno.
    ///
    /// Tutta aperta vale **uno esatto**, e non `corsa × (1 / corsa)`, che in
    /// virgola mobile può fare 0,99999994: a porta aperta il campione deve
    /// uscire com'è, e un filtro piatto non deve cambiare l'uscita di un bit.
    #[expect(
        clippy::cast_precision_loss,
        reason = "porta e corsa sono i fotogrammi di dodici millisecondi, poche \
                  migliaia: esatti in un f32"
    )]
    fn apertura(&self) -> f32 {
        if self.porta >= self.corsa {
            1.0
        } else {
            self.porta as f32 / self.corsa.max(1) as f32
        }
    }
}

/// Quanti fotogrammi dura la porta, a questa frequenza.
///
/// Almeno uno, anche a frequenze assurde: una porta che si chiude in zero
/// fotogrammi è il gradino che esiste per togliere.
#[expect(
    clippy::integer_division,
    reason = "i fotogrammi della dissolvenza: un fotogramma in più o in meno su \
              cinquecento non si sente"
)]
fn corsa_della_porta(frequenza: u32) -> u32 {
    let fotogrammi = (u64::from(frequenza) * DISSOLVENZA_MS / 1000).max(1);
    u32::try_from(fotogrammi).unwrap_or(u32::MAX)
}

/// Un passo della rampa del volume, con l'aggancio.
///
/// Vedi [`RAMPA`] per la forma e [`AGGANCIO`] per la ragione dello scatto
/// finale: senza, verso lo zero il guadagno finiva fra i subnormali.
///
/// Lo scatto c'è anche quando il passo non sposta più niente, che è il caso
/// verso un volume alto: vicino a 0,7 lo 0,05% di quel che manca diventa più
/// piccolo della distanza fra due `f32` consecutivi, e la rampa si ferma a
/// qualche centomillesimo dal voluto senza arrivarci mai. Innocuo per
/// l'orecchio — ottanta decibel sotto — ma un guadagno che non raggiunge il
/// cursore è un numero che mente, e con lo scatto la rampa finisce **sempre**.
fn avvicina(guadagno: f32, voluto: f32) -> f32 {
    let manca = voluto - guadagno;
    let dopo = guadagno + manca * RAMPA;
    if manca.abs() < AGGANCIO || dopo.to_bits() == guadagno.to_bits() {
        voluto
    } else {
        dopo
    }
}

/// L'equalizzatore come lo vede la callback: i filtri, e il filo da cui
/// arrivano le curve nuove.
struct Filtro {
    stato: StatoEq,
    ricevi: rtrb::Consumer<Coefficienti>,
}

impl Filtro {
    /// Ritira le curve arrivate dall'ultima volta.
    ///
    /// Si svuota l'anello invece di prenderne una sola: durante un
    /// trascinamento ne arrivano parecchie fra una callback e l'altra, e
    /// l'unica che conta è l'ultima. Lasciarne indietro vorrebbe dire che il
    /// cursore continua a muoversi per qualche decimo di secondo dopo che il
    /// dito si è fermato.
    fn ritira(&mut self) {
        while let Ok(nuovi) = self.ricevi.pop() {
            self.stato.aggiorna(nuovi);
        }
    }
}

/// Tutto quel che la callback possiede.
///
/// In una struttura sola invece che in cinque argomenti: sono le cose che la
/// callback si porta dietro da un blocco all'altro, e [`riempi`] le riceve
/// insieme. Un `Consumer` non è clonabile e non deve esserlo: chi legge dai due
/// anelli è uno solo, ed è questa callback.
struct Voce {
    /// L'anello dei campioni.
    lettore: rtrb::Consumer<f32>,
    /// La rampa, la porta e il fotogramma a metà.
    andamento: Andamento,
    /// L'equalizzatore.
    filtro: Filtro,
    /// La presa dello spettro.
    spia: Spia,
    /// Quel che si racconta al filo dell'uscita.
    veglia: Arc<Veglia>,
}

/// Il corpo della callback, uguale per ogni formato di campione.
///
/// Generico sulla conversione finale invece che scritto sei volte: le sei
/// versioni differirebbero per una moltiplicazione, e sei copie di una
/// procedura in cui non si può sbagliare sono sei posti in cui sbagliare.
///
/// # Suonare e tacere sono lo stesso giro
///
/// Il motore chiede silenzio in due modi: la pausa (`in_pausa`, che vale anche
/// per il fermo e per la coda finita) e lo svuotamento (`svuota`, a ogni salto
/// e a ogni brano nuovo). In tutti e due i casi il giro è lo stesso di quando si
/// suona, con la porta che si chiude invece di aprirsi: i campioni continuano a
/// uscire dall'anello — sfumando — finché la porta non è chiusa, e solo da lì
/// in poi escono zeri senza toccare l'anello. Una pausa consuma quindi una
/// dozzina di millisecondi di musica, che è quel che si è sentito sfumare; la
/// ripresa riparte dal campione dopo.
///
/// Con la porta chiusa il volume torna a zero, così la ripresa sale anche con
/// la rampa del volume; e se era uno svuotamento, **solo allora** l'anello si
/// svuota e la bandiera si abbassa — il punto vecchio sfuma, poi sparisce.
///
/// # Il tempo condiviso
///
/// Dalla Fase 0 di «Ascolta insieme» il giro sa fare tre cose in più, tutte
/// spente finché le loro caselle in [`Condiviso`] valgono zero — e spente il giro
/// è identico al bit a quello di prima, come dice la prova
/// `il_percorso_inattivo_e_identico_al_bit`:
///
/// - **l'avvio programmato** (`avvio_ns`): la musica riparte dal primo fotogramma
///   del blocco che non si sentirà prima dell'istante chiesto;
/// - **il gradino** (`gradino`): silenzio inserito o musica saltata, con la
///   porta che si chiude e si riapre intorno — vedi [`Fase`];
/// - **la deriva** (`deriva_ppm`): un fotogramma tolto o aggiunto ogni tanto —
///   vedi [`Andamento::accumulo`].
///
/// E a ogni blocco con della musica pubblica l'**ancora**: quale fotogramma
/// d'anello si sentirà, e quando. Vedi [`crate::ancora`].
#[expect(
    clippy::integer_division,
    reason = "un fotogramma è esattamente `canali` campioni; il resto sarebbe un \
              fotogramma servito a metà, che non esiste"
)]
fn riempi<T>(
    dati: &mut [T],
    voce: &mut Voce,
    condiviso: &Condiviso,
    battito: &Battito,
    converti: impl Fn(f32) -> T,
) {
    // Il battito, prima di ogni altra cosa: chi ha appena rimesso in moto il
    // flusso aspetta proprio questo numero. Vedi `rimetti_in_moto`.
    voce.veglia.giri.fetch_add(1, Ordering::Release);

    // Le curve nuove prima del resto, anche prima dello svuotamento e della
    // pausa: adottarne una non tocca il suono che sta uscendo, e un ritiro
    // saltato è una curva che arriva un blocco dopo.
    voce.filtro.ritira();

    let svuota = condiviso.svuota.load(Ordering::Acquire);
    let in_pausa = condiviso.in_pausa.load(Ordering::Acquire);
    let tace = svuota || in_pausa;

    let voluto = f32::from_bits(condiviso.guadagno.load(Ordering::Relaxed));
    let canali = condiviso.canali.load(Ordering::Relaxed).max(1);
    // Per l'indice di canale serve un `usize`; il ripiego non si raggiunge, ma
    // qui dentro non esiste un modo accettabile di cadere.
    let per_fotogramma = usize::try_from(canali).unwrap_or(2);
    // Una volta per blocco e non una per campione: con i cursori a zero
    // l'equalizzatore non deve costare niente.
    let filtra = !voce.filtro.stato.piatto();
    let catena = Catena {
        filtra,
        // Idem per lo spettro: chi non ha la schermata aperta non paga un
        // `push` per campione, che a 48 kHz sarebbero quarantottomila al
        // secondo per riempire un anello che nessuno svuota.
        guarda: condiviso.spettro.load(Ordering::Relaxed),
        // Il limitatore morbido, solo quando qualcosa nella catena **alza**:
        // la normalizzazione sopra l'unità o l'equalizzatore. A guadagno pieno
        // o più basso un brano masterizzato fino a 0 dBFS deve uscire com'è —
        // il ginocchio di [`limita`] sta a −1 dBFS, e piegare i picchi di un
        // disco che nessuno ha alzato vorrebbe dire colorarlo per niente.
        // Deciso per blocco, sul guadagno voluto: la rampa che attraversa
        // l'unità dura qualche millisecondo, e lì la differenza è di un paio
        // di decimi di decibel sui soli picchi.
        piega: voluto > 1.0 || filtra,
        per_fotogramma,
    };
    let mut presi = 0u64;
    let mut mancati = 0u64;
    let mut apertura = voce.andamento.apertura();

    // Il resto **prima** del giro: l'ancora vale solo se il blocco comincia su
    // un fotogramma intero, altrimenti il primo campione preso è il compagno
    // di uno già contato e il conteggio non dice più quale fotogramma sia.
    let resto_iniziale = voce.andamento.resto;
    let mut regia = Regia::del_blocco(voce, condiviso, battito, tace, per_fotogramma);
    let mut modo = Modo::Anello;

    for (indice, posto) in dati.iter_mut().enumerate() {
        // `indice % per_fotogramma` dà il canale: cpal consegna sempre
        // fotogrammi interi, che è la stessa cosa che il conto dei fotogrammi
        // qui sotto dà già per buona.
        let canale = indice % per_fotogramma;
        if canale == 0 {
            // Tutte le decisioni si prendono qui, una volta per fotogramma:
            // un fotogramma esce intero dall'anello, intero tace, o intero
            // viene composto dalla deriva — mai mezzo e mezzo.
            let consumati = resto_iniziale
                .saturating_add(presi)
                .saturating_add(regia.saltati);
            let allineato = consumati.is_multiple_of(u64::from(canali));
            modo = regia.fotogramma(
                voce,
                condiviso,
                indice / per_fotogramma,
                tace,
                allineato,
                &mut presi,
            );
            apertura = voce.andamento.apertura();
        }
        match modo {
            // Porta chiusa e silenzio chiesto: zeri, e l'anello non si tocca —
            // in pausa quel che c'è dentro è la ripresa, e deve restare dov'è.
            Modo::Zero => *posto = converti(0.0),
            Modo::Cornice => {
                voce.andamento.guadagno = avvicina(voce.andamento.guadagno, voluto);
                let campione = regia.cornice.get(canale).copied().unwrap_or(0.0);
                *posto = converti(esce(voce, catena, campione, canale, apertura));
            }
            Modo::Anello => {
                voce.andamento.guadagno = avvicina(voce.andamento.guadagno, voluto);
                match voce.lettore.pop() {
                    Ok(campione) => {
                        regia.preso(&mut voce.andamento, indice, canale, campione);
                        presi += 1;
                        *posto = converti(esce(voce, catena, campione, canale, apertura));
                    }
                    Err(_) => {
                        // L'anello è vuoto. Suonando vuol dire che il
                        // decodificatore non ce l'ha fatta: silenzio, che è un
                        // buco — l'alternativa sarebbe ripetere l'ultimo
                        // campione, che è un ronzio — e si conta. Sfumando
                        // verso il silenzio chiesto no: la coda finita arriva
                        // proprio così, e non è un disco lento.
                        if !tace {
                            mancati += 1;
                        }
                        regia.ricorda(&mut voce.andamento, canale, 0.0);
                        *posto = converti(0.0);
                    }
                }
            }
        }
    }

    // Il resto del blocco precedente entra nel conto di questo: i campioni di un
    // fotogramma servito a metà escono dalle casse a cavallo di due callback, e
    // il fotogramma che formano insieme va contato una volta — non zero. Con
    // loro i fotogrammi saltati da un gradino: sono musica passata, e mappa,
    // posizione e fine della coda devono saperlo.
    let campioni = voce
        .andamento
        .resto
        .saturating_add(presi)
        .saturating_add(regia.saltati);
    let canali = u64::from(canali);
    let contati_prima = condiviso
        .fotogrammi
        .fetch_add(campioni / canali, Ordering::Relaxed);
    voce.andamento.resto = campioni % canali;
    if mancati > 0 {
        condiviso.vuoti.fetch_add(mancati, Ordering::Relaxed);
    }

    // L'epoca **dopo** il giro e **prima** di `svuota_anello`. Dopo, perché i
    // campioni di un'epoca nuova arrivano nell'anello solo dopo che il filo
    // della decodifica l'ha alzata: chi li ha appena consumati la vede. Prima,
    // perché l'epoca non può cambiare finché la bandiera dello svuotamento è
    // alzata — il filo aspetta proprio che si abbassi.
    let epoca = condiviso.epoca.load(Ordering::Acquire);
    if resto_iniziale == 0
        && !regia.storto
        && let Some(k) = regia.primo
        && let Some(ancora) = ancora_del_blocco(
            battito,
            k,
            contati_prima.saturating_add(regia.saltati_prima),
            epoca,
            !tace,
        )
    {
        condiviso.ancora.scrivi(&ancora);
    }

    let chiusa = voce.andamento.porta == 0;
    if tace && chiusa {
        // Il volume riparte da zero: la ripresa, o il punto nuovo dopo un
        // salto, salgono con la rampa invece di presentarsi a volume pieno sul
        // primo campione.
        voce.andamento.guadagno = 0.0;
        if svuota {
            svuota_anello(voce, condiviso);
        }
    }
    voce.veglia.a_riposo.store(
        in_pausa && chiusa && voce.lettore.is_empty(),
        Ordering::Release,
    );
}

/// Come esce un fotogramma: la decisione che [`Regia::fotogramma`] prende al
/// primo campione e che vale per tutti i suoi canali.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modo {
    /// Dall'anello, un campione per canale: il giro di sempre.
    Anello,
    /// Zero, e l'anello non si tocca.
    Zero,
    /// Da [`Regia::cornice`]: un fotogramma composto dalla deriva.
    Cornice,
}

/// Le scelte del blocco che valgono per ogni campione.
#[derive(Debug, Clone, Copy)]
struct Catena {
    filtra: bool,
    guarda: bool,
    piega: bool,
    per_fotogramma: usize,
}

/// Un campione d'anello fino alla conversione: equalizzatore, spettro, volume,
/// porta, limitatore. Sempre in quest'ordine e con le stesse operazioni: è il
/// pezzo che tiene il percorso inattivo identico al bit.
#[inline]
fn esce(voce: &mut Voce, catena: Catena, campione: f32, canale: usize, apertura: f32) -> f32 {
    // L'equalizzatore **prima** del volume: così il volume resta l'ultima
    // cosa che tocca il campione e la sua rampa continua a fare il suo
    // lavoro, e così la preamplificazione dei filtri non dipende da quanto è
    // alzato il volume.
    let campione = if catena.filtra {
        voce.filtro.stato.applica(campione, canale)
    } else {
        campione
    };
    // Lo spettro prende il campione **qui**: dopo l'equalizzatore, così una
    // curva che alza i bassi si vede nelle barre, e prima del volume, perché
    // uno spettro che si abbassa quando si abbassa la manopola descrive la
    // manopola e non la musica.
    if catena.guarda {
        voce.spia.campione(campione, catena.per_fotogramma);
    }
    let alzato = campione * voce.andamento.guadagno * apertura;
    if catena.piega { limita(alzato) } else { alzato }
}

/// Le primitive di tempo, per un blocco: chi decide fotogramma per fotogramma
/// se si suona, si tace, si salta o si compone.
///
/// Vive quanto il blocco: quel che deve sopravvivere — la fase del gradino,
/// l'accumulo della deriva — sta in [`Andamento`].
struct Regia {
    /// Il fotogramma del blocco da cui l'avvio programmato lascia suonare;
    /// quelli prima tacciono. `None`: nessuna attesa.
    attesa: Option<usize>,
    /// L'avvio letto a inizio blocco: lo si azzera solo se è ancora quello,
    /// così un avvio nuovo scritto nel frattempo vale per il blocco dopo.
    avvio: u64,
    /// La deriva del blocco in parti per milione, già tagliata; zero: spenta.
    ppm: i64,
    /// I canali dell'uscita.
    canali: usize,
    /// Il fotogramma del blocco da cui è uscito il primo campione d'anello.
    primo: Option<usize>,
    /// Il primo campione d'anello non era il primo canale di un fotogramma:
    /// niente ancora per questo blocco.
    storto: bool,
    /// I campioni saltati da un gradino in questo blocco: si contano.
    saltati: u64,
    /// I fotogrammi saltati prima del primo campione preso: spostano il numero
    /// del fotogramma dell'ancora.
    saltati_prima: u64,
    /// Il fotogramma composto dalla deriva, quando il modo è
    /// [`Modo::Cornice`].
    cornice: [f32; CANALI_DERIVA],
}

/// Dove cade un avvio programmato rispetto al primo fotogramma di un blocco.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cade {
    /// Fra tanti fotogrammi, almeno uno: si tace fin lì.
    Fra(u64),
    /// Già passato, di tanti fotogrammi interi: si parte subito, e quel che si
    /// è perso si salta.
    Ritardo(u64),
}

/// Dove cade l'avvio `avvio_ns` in un blocco il cui primo fotogramma si sente
/// a `playback_ns`.
///
/// Il fotogramma `k` si sente a `playback + k/frequenza`: il primo che non
/// suona **prima** dell'avvio è `k = ⌈(avvio − playback)·frequenza / 10⁹⌉`. In
/// `i128`, perché la differenza può essere negativa e il prodotto per la
/// frequenza non sta in un `i64` oltre i tre anni.
#[expect(
    clippy::integer_division,
    reason = "fotogrammi interi da un tempo: la divisione per eccesso è scritta \
              a mano, e il resto è meno di un fotogramma"
)]
fn dove_cade(avvio_ns: u64, playback_ns: u64, frequenza: u32) -> Cade {
    const MILIARDO: i128 = 1_000_000_000;
    let distanza = i128::from(avvio_ns) - i128::from(playback_ns);
    let frequenza = i128::from(frequenza);
    if distanza > 0 {
        let fra = (distanza.saturating_mul(frequenza) + MILIARDO - 1) / MILIARDO;
        Cade::Fra(u64::try_from(fra).unwrap_or(u64::MAX))
    } else {
        let persi = distanza.saturating_neg().saturating_mul(frequenza) / MILIARDO;
        Cade::Ritardo(u64::try_from(persi).unwrap_or(u64::MAX))
    }
}

/// I nanosecondi di tanti fotogrammi, alla frequenza d'uscita.
#[expect(
    clippy::integer_division,
    reason = "il resto è meno di un nanosecondo per fotogramma: l'orologio che \
              lo deve confrontare va a passi di cento"
)]
fn ns_di_fotogrammi(fotogrammi: u64, frequenza: u32) -> u64 {
    let ns = u128::from(fotogrammi).saturating_mul(1_000_000_000) / u128::from(frequenza.max(1));
    u64::try_from(ns).unwrap_or(u64::MAX)
}

/// L'ancora di un blocco il cui primo campione d'anello è uscito al fotogramma
/// `k` ed era il fotogramma d'anello `contenuto`. `None` senza frequenza.
///
/// # Quando la misura si butta
///
/// Con la stessa regola di `Contesto::ritardo_fotogrammi`: un anticipo oltre
/// [`LATENZA_MASSIMA_MS`] non è un buffer lento, è un orologio che ha detto una
/// sciocchezza. Allora l'istante di partenza è quello della callback — il
/// `playback` meno l'anticipo — e l'ancora lo dice, perché chi la legge sappia
/// che la latenza del buffer non c'è dentro.
fn ancora_del_blocco(
    battito: &Battito,
    k: usize,
    contenuto: u64,
    epoca: u32,
    in_corso: bool,
) -> Option<AncoraGrezza> {
    if battito.frequenza == 0 {
        return None;
    }
    let tetto = LATENZA_MASSIMA_MS.saturating_mul(1_000_000);
    let scartata = battito.anticipo_ns > tetto;
    let (base, controprova) = if scartata {
        (
            battito.playback_ns.saturating_sub(battito.anticipo_ns),
            battito.ingresso_ns,
        )
    } else {
        (
            battito.playback_ns,
            battito.ingresso_ns.saturating_add(battito.anticipo_ns),
        )
    };
    let dopo = ns_di_fotogrammi(u64::try_from(k).unwrap_or(u64::MAX), battito.frequenza);
    Some(AncoraGrezza {
        contenuto,
        udito_ns: base.saturating_add(dopo),
        anticipo_ns: battito.anticipo_ns,
        controprova_ns: controprova.saturating_add(dopo),
        epoca,
        in_corso,
        latenza_scartata: scartata,
    })
}

/// Il gradino è stato eseguito: lo si conta, e la porta si riapre.
fn fatto(condiviso: &Condiviso) -> Fase {
    condiviso.gradini_fatti.fetch_add(1, Ordering::Relaxed);
    Fase::Riapre
}

/// La porta si è riaperta del tutto: non c'è più niente in correzione — a meno
/// che nel frattempo non ne sia stata chiesta un'altra.
///
/// L'ordine conta: prima si abbassa, poi si guarda se c'è altro. Chi chiede un
/// gradino scrive la richiesta **prima** di alzare la bandiera, quindi o la
/// richiesta si vede qui, o la sua bandiera arriva dopo questa scrittura.
fn libera(condiviso: &Condiviso) {
    condiviso.in_correzione.store(false, Ordering::SeqCst);
    if condiviso.gradino.load(Ordering::SeqCst) != 0 {
        condiviso.in_correzione.store(true, Ordering::SeqCst);
    }
}

/// Fa cadere il gradino in corso e quello chiesto: vedi [`Regia::del_blocco`].
///
/// Legge prima di scrivere: a riposo, con niente da far cadere, non scrive
/// niente.
fn lascia_cadere_il_gradino(andamento: &mut Andamento, condiviso: &Condiviso) {
    andamento.fase = Fase::Libera;
    if condiviso.gradino.load(Ordering::Relaxed) != 0 {
        condiviso.gradino.store(0, Ordering::Release);
    }
    if condiviso.in_correzione.load(Ordering::Relaxed) {
        condiviso.in_correzione.store(false, Ordering::Release);
    }
}

impl Regia {
    /// Le decisioni del blocco, prese una volta all'inizio.
    ///
    /// A riposo — avvio a zero, nessun gradino, deriva a zero — legge quattro
    /// atomici e non scrive niente.
    fn del_blocco(
        voce: &mut Voce,
        condiviso: &Condiviso,
        battito: &Battito,
        tace: bool,
        canali: usize,
    ) -> Self {
        let mut regia = Self {
            attesa: None,
            avvio: 0,
            ppm: 0,
            canali,
            primo: None,
            storto: false,
            saltati: 0,
            saltati_prima: 0,
            cornice: [0.0; CANALI_DERIVA],
        };
        if tace {
            // Una pausa o uno svuotamento fanno cadere il gradino, in corso o
            // chiesto: era calcolato su un tempo che non c'è più, e chi
            // l'aveva chiesto lo rifarà sulla musica che ripartirà. La porta
            // si chiude lo stesso, per la pausa.
            lascia_cadere_il_gradino(&mut voce.andamento, condiviso);
            return regia;
        }

        let avvio = condiviso.avvio_ns.load(Ordering::Acquire);
        if avvio != 0 && battito.frequenza > 0 {
            match dove_cade(avvio, battito.playback_ns, battito.frequenza) {
                Cade::Fra(k) => {
                    regia.attesa = Some(usize::try_from(k).unwrap_or(usize::MAX));
                    regia.avvio = avvio;
                }
                Cade::Ritardo(persi) => {
                    let _ = condiviso.avvio_ns.compare_exchange(
                        avvio,
                        0,
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    );
                    // In ritardo e a porta chiusa: si parte adesso, e la musica
                    // che doveva già essere uscita si salta — un gradino come
                    // gli altri, che la porta chiusa fa partire subito. A
                    // porta aperta invece la musica stava già andando, e un
                    // avvio nel passato non ha niente da far ripartire.
                    if persi > 0 && voce.andamento.porta == 0 {
                        condiviso
                            .gradino
                            .fetch_sub(i64::try_from(persi).unwrap_or(i64::MAX), Ordering::AcqRel);
                        condiviso.in_correzione.store(true, Ordering::Release);
                    }
                }
            }
        }

        // Un gradino si prende solo a fase libera e senza un avvio da
        // aspettare: la porta chiusa del gradino consumerebbe musica proprio
        // mentre l'avvio la tiene ferma.
        if regia.attesa.is_none()
            && voce.andamento.fase == Fase::Libera
            && condiviso.gradino.load(Ordering::Acquire) != 0
        {
            let chiesto = condiviso.gradino.swap(0, Ordering::AcqRel);
            if chiesto != 0 {
                voce.andamento.fase = Fase::Chiude { quanto: chiesto };
                condiviso.in_correzione.store(true, Ordering::Release);
            }
        }

        let ppm = condiviso.deriva_ppm.load(Ordering::Relaxed);
        if ppm != 0 && canali <= CANALI_DERIVA {
            regia.ppm = i64::from(ppm.clamp(-DERIVA_MASSIMA_PPM, DERIVA_MASSIMA_PPM));
        }
        regia
    }

    /// Il primo campione di un fotogramma: come esce tutto il fotogramma.
    fn fotogramma(
        &mut self,
        voce: &mut Voce,
        condiviso: &Condiviso,
        numero: usize,
        tace: bool,
        allineato: bool,
        presi: &mut u64,
    ) -> Modo {
        let aspetta = self.aspetta(condiviso, numero);
        if tace || aspetta {
            // La pausa di sempre: la porta si chiude, e chiusa tace.
            voce.andamento.muovi_porta(false);
            return if voce.andamento.porta == 0 {
                Modo::Zero
            } else {
                Modo::Anello
            };
        }
        match voce.andamento.fase {
            Fase::Libera => {
                voce.andamento.muovi_porta(true);
                self.forse_deriva(voce, allineato, presi)
            }
            Fase::Chiude { quanto } => {
                voce.andamento.muovi_porta(false);
                if voce.andamento.porta > 0 {
                    return Modo::Anello;
                }
                // La porta si è chiusa adesso. Il silenzio comincia dal
                // fotogramma dopo, e questo consuma a guadagno zero: vedi
                // l'esattezza in [`Fase`].
                if quanto > 0 {
                    voce.andamento.fase = Fase::Esegue { resto: quanto };
                    return Modo::Anello;
                }
                self.salta_o_aspetta(voce, condiviso, quanto.unsigned_abs(), allineato)
            }
            Fase::Esegue { resto } => {
                if resto > 0 {
                    let resto = resto - 1;
                    voce.andamento.fase = if resto == 0 {
                        fatto(condiviso)
                    } else {
                        Fase::Esegue { resto }
                    };
                    return Modo::Zero;
                }
                self.salta_o_aspetta(voce, condiviso, resto.unsigned_abs(), allineato)
            }
            Fase::Riapre => {
                voce.andamento.muovi_porta(true);
                if voce.andamento.porta >= voce.andamento.corsa {
                    voce.andamento.fase = Fase::Libera;
                    libera(condiviso);
                }
                Modo::Anello
            }
        }
    }

    /// Il fotogramma `numero` deve ancora tacere per l'avvio programmato. Al
    /// fotogramma dell'avvio l'attesa finisce, e la casella si azzera.
    fn aspetta(&mut self, condiviso: &Condiviso, numero: usize) -> bool {
        match self.attesa {
            Some(k) if numero < k => true,
            Some(_) => {
                self.attesa = None;
                let _ = condiviso.avvio_ns.compare_exchange(
                    self.avvio,
                    0,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                );
                false
            }
            None => false,
        }
    }

    /// A porta chiusa, salta quel che manca di un gradino all'indietro.
    ///
    /// Fatto tutto, il fotogramma consuma a guadagno zero e la porta si
    /// riapre; se l'anello non ne aveva abbastanza, il resto resta in sospeso e
    /// il fotogramma tace.
    fn salta_o_aspetta(
        &mut self,
        voce: &mut Voce,
        condiviso: &Condiviso,
        quanti: u64,
        allineato: bool,
    ) -> Modo {
        let mancano = self.salta(voce, quanti, allineato);
        if mancano == 0 {
            voce.andamento.fase = fatto(condiviso);
            return Modo::Anello;
        }
        voce.andamento.fase = Fase::Esegue {
            resto: 0i64.saturating_sub_unsigned(mancano),
        };
        Modo::Zero
    }

    /// Butta fino a `quanti` fotogrammi interi dall'anello, contandoli.
    /// Restituisce quanti ne mancano.
    ///
    /// Un pezzo letto e liberato in un colpo solo: per dei `f32` liberare non
    /// ha niente da distruggere, e il salto costa quanto spostare un indice —
    /// non mille `pop`. Solo a fotogramma intero, perché saltare da metà
    /// fotogramma scambierebbe i canali per sempre.
    #[expect(
        clippy::integer_division,
        reason = "i fotogrammi interi che l'anello contiene: quel che avanza è un \
                  fotogramma a metà, e quello non si salta"
    )]
    fn salta(&mut self, voce: &mut Voce, quanti: u64, allineato: bool) -> u64 {
        if !allineato || quanti == 0 {
            return quanti;
        }
        let canali = self.canali.max(1);
        let interi = u64::try_from(voce.lettore.slots() / canali).unwrap_or(0);
        let fatti = quanti.min(interi);
        if fatti == 0 {
            return quanti;
        }
        let campioni = usize::try_from(fatti).unwrap_or(0).saturating_mul(canali);
        let Ok(pezzo) = voce.lettore.read_chunk(campioni) else {
            return quanti;
        };
        pezzo.commit_all();
        self.saltati = self
            .saltati
            .saturating_add(u64::try_from(campioni).unwrap_or(0));
        if self.primo.is_none() {
            self.saltati_prima = self.saltati_prima.saturating_add(fatti);
        }
        quanti - fatti
    }

    /// Un fotogramma a porta tutta aperta: se la deriva ha accumulato un
    /// fotogramma intero, lo toglie o lo aggiunge qui.
    ///
    /// Mai sul primo fotogramma di musica del blocco: è quello dell'ancora, e
    /// deve essere esattamente il fotogramma d'anello che il conteggio dice.
    fn forse_deriva(&mut self, voce: &mut Voce, allineato: bool, presi: &mut u64) -> Modo {
        if self.ppm == 0
            || self.primo.is_none()
            || !allineato
            || voce.andamento.porta < voce.andamento.corsa
        {
            return Modo::Anello;
        }
        voce.andamento.accumulo = voce.andamento.accumulo.saturating_add(self.ppm);
        let canali = self.canali;
        if voce.andamento.accumulo >= MILIONE {
            // Avanti: due fotogrammi escono come uno, la loro media. Se
            // l'anello non li ha si riprova al fotogramma dopo, con l'accumulo
            // ancora pieno.
            let Ok(pezzo) = voce.lettore.read_chunk(canali.saturating_mul(2)) else {
                return Modo::Anello;
            };
            let (a, b) = pezzo.as_slices();
            let mut tutti = a.iter().chain(b).copied();
            for posto in self.cornice.iter_mut().take(canali) {
                *posto = tutti.next().unwrap_or(0.0);
            }
            for posto in self.cornice.iter_mut().take(canali) {
                *posto = (*posto + tutti.next().unwrap_or(0.0)) * 0.5;
            }
            pezzo.commit_all();
            voce.andamento.accumulo -= MILIONE;
            let due = u64::try_from(canali.saturating_mul(2)).unwrap_or(0);
            *presi = presi.saturating_add(due);
            return Modo::Cornice;
        }
        if voce.andamento.accumulo <= -MILIONE {
            // Indietro: fra l'ultimo uscito e il prossimo si mette la loro
            // media. Il prossimo si **guarda** senza consumarlo — un pezzo
            // letto e non liberato — e uscirà al fotogramma dopo.
            let Ok(pezzo) = voce.lettore.read_chunk(canali) else {
                return Modo::Anello;
            };
            let (a, b) = pezzo.as_slices();
            for ((posto, prima), dopo) in self
                .cornice
                .iter_mut()
                .zip(voce.andamento.ultimo.iter())
                .zip(a.iter().chain(b))
            {
                *posto = (*prima + *dopo) * 0.5;
            }
            voce.andamento.accumulo += MILIONE;
            return Modo::Cornice;
        }
        Modo::Anello
    }

    /// Un campione è uscito dall'anello: annota il primo del blocco, e ricorda
    /// com'era per la deriva.
    fn preso(&mut self, andamento: &mut Andamento, indice: usize, canale: usize, campione: f32) {
        if self.primo.is_none() && !self.storto {
            if canale == 0 {
                self.primo = Some(fotogramma_di(indice, self.canali));
            } else {
                self.storto = true;
            }
        }
        self.ricorda(andamento, canale, campione);
    }

    /// Ricorda l'ultimo campione uscito di un canale: la prima metà della
    /// media di un fotogramma aggiunto. Solo con la deriva accesa.
    fn ricorda(&self, andamento: &mut Andamento, canale: usize, campione: f32) {
        if self.ppm != 0
            && let Some(posto) = andamento.ultimo.get_mut(canale)
        {
            *posto = campione;
        }
    }
}

/// Il fotogramma del blocco a cui appartiene un campione.
#[expect(
    clippy::integer_division,
    reason = "l'indice del fotogramma in un blocco interlacciato: il resto è il \
              canale"
)]
fn fotogramma_di(indice: usize, canali: usize) -> usize {
    indice / canali.max(1)
}

/// Butta quel che c'è nell'anello e abbassa la bandiera dello svuotamento.
///
/// La chiama [`riempi`] quando la porta si è chiusa: dopo un salto, quel che
/// c'è nell'anello appartiene al punto di prima. Va scartato anche — anzi,
/// soprattutto — se siamo in pausa, altrimenti riprendendo si sentirebbe il
/// punto vecchio.
fn svuota_anello(voce: &mut Voce, condiviso: &Condiviso) {
    while voce.lettore.pop().is_ok() {}
    // La coda dei filtri è quel punto vecchio quanto i campioni nell'anello:
    // lasciarla suonare sopra il punto nuovo sarebbe la stessa cosa che non
    // svuotare.
    voce.filtro.stato.azzera();
    voce.spia.azzera();
    // Il fotogramma a metà appartiene al punto di prima, come i campioni
    // nell'anello: portarlo oltre il salto conterebbe un fotogramma vecchio
    // dentro il conteggio nuovo, che il decodificatore sta per azzerare.
    voce.andamento.resto = 0;
    // Anche l'ultimo fotogramma ricordato per la deriva: una media fra il
    // punto vecchio e il nuovo sarebbe un pezzo di tutti e due.
    voce.andamento.ultimo = [0.0; CANALI_DERIVA];
    // La bandiera si abbassa **dopo** aver svuotato, e non prima.
    //
    // # Perché l'ordine conta
    //
    // Perché dall'altra parte c'è un'attesa: `Contesto::scarta_in_volo` alza
    // questa bandiera e resta fermo finché non la vede abbassata, perché solo
    // allora l'anello è certamente vuoto e i contatori si possono azzerare
    // senza correre contro la callback. Abbassandola all'inizio, quell'attesa
    // finiva **prima** dello svuotamento: il filo della decodifica ripartiva,
    // ricominciava a spingere campioni nuovi, e il `while` qui sopra — che era
    // ancora dentro il suo giro — se li mangiava senza contarli.
    //
    // Il risultato era un debito permanente fra i fotogrammi spinti e quelli
    // contati come usciti: qualche migliaio, cioè qualche decina di
    // millisecondi, a ogni salto. Per i brani in mezzo a una coda non si vedeva
    // — a passare al successivo è il decodificatore che finisce, non un
    // conteggio — ma la **fine della coda** la dichiara `Contesto::forse_fine`
    // confrontando quei due numeri, e con un debito aperto quel confronto non
    // era mai vero: l'ultimo brano restava fermo a un dito dalla fine, il tasto
    // diceva che stava suonando, e non succedeva più niente.
    //
    // I fotogrammi che la porta ha fatto sfumare prima di questo punto sono
    // già stati contati, **prima** di questa riga: il filo della decodifica li
    // azzera dopo averla vista, quindi non restano in nessun conto.
    condiviso.svuota.store(false, Ordering::Release);
}

/// Il guasto di un'apertura del dispositivo, detto col codice che dice se
/// riprovare.
///
/// # Perché due codici e non uno
///
/// Fino a questa versione ogni errore di `cpal` all'apertura diventava
/// `playback.engineUnavailable`, che il catalogo dichiara `Fatal` e non
/// ritentabile, e che la finestra traduce con «riavvia Rymembra». Per i guasti
/// più comuni era falso. Il diario vero li mostrava tutti e due: all'avvio
/// `0x8889000F` (`AUDCLNT_E_ENDPOINT_CREATE_FAILED`), e a metà di una
/// riapertura automatica «Failed to get audio clock», che sotto è
/// `AUDCLNT_E_DEVICE_INVALIDATED`. Sono endpoint **in transizione** — cuffie
/// Bluetooth che negoziano ancora il profilo, il predefinito di Windows che
/// cambia proprio mentre lo si apre — e un secondo dopo si aprono benissimo.
///
/// `cpal` porta entrambi a [`cpal::ErrorKind::DeviceNotAvailable`], anche
/// dietro la frase di contesto che aggiunge: il tipo sopravvive al messaggio.
/// Quel tipo, `StreamInvalidated` e `DeviceBusy` — un altro programma che ha
/// preso l'uscita in esclusiva per un attimo — diventano `playback.deviceLost`:
/// ritentabile, e già tradotto con la frase giusta, «Rymembra ne cerca un altro
/// da solo». Il resto — un formato che il dispositivo rifiuta, il servizio audio
/// di Windows spento, un errore che `cpal` non sa classificare — resta
/// `engineUnavailable`: lì riprovare fra un secondo non cambia niente, e il
/// consiglio di controllare la scheda è quello giusto.
///
/// Non tocca il catalogo, ed è voluto: `engineUnavailable` resta fatale per i
/// guasti che lo sono davvero.
fn errore_di_apertura(err: &cpal::Error) -> AppError {
    let codice = match err.kind() {
        cpal::ErrorKind::DeviceNotAvailable
        | cpal::ErrorKind::StreamInvalidated
        | cpal::ErrorKind::DeviceBusy => ErrorCode::PlaybackDeviceLost,
        _ => ErrorCode::PlaybackEngineUnavailable,
    };
    AppError::new(codice).with_cause(err.to_string())
}

/// Quale causa di perdita è un errore del flusso, se lo è.
///
/// `None` per gli avvisi che non fermano niente: un buco nel buffer (`Xrun`),
/// la priorità in tempo reale negata (`RealtimeDenied`), un cambio di percorso
/// già seguito da solo (`DeviceChanged`). Il flusso continua a suonare, e
/// alzare `perso` per uno di questi vorrebbe dire buttare il motore e
/// riaprire tutto per un singhiozzo.
///
/// Per il resto, due numeri e non uno per tipo: la tabella che li traduce in
/// una frase sta in `Motore::perdita`, e alla frase serve sapere solo se il
/// dispositivo se n'è andato — riaprire quasi sempre funziona — o se è tutto il
/// resto, dove riaprire è un tentativo e non una cura.
const fn codice_errore(tipo: cpal::ErrorKind) -> Option<u32> {
    match tipo {
        cpal::ErrorKind::Xrun
        | cpal::ErrorKind::RealtimeDenied
        | cpal::ErrorKind::DeviceChanged => None,
        cpal::ErrorKind::DeviceNotAvailable
        | cpal::ErrorKind::StreamInvalidated
        | cpal::ErrorKind::HostUnavailable => Some(CAUSA_SPARITO),
        _ => Some(CAUSA_GUASTO),
    }
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::equalizzatore::{BANDE, LIMITE_DB};

    /// Una frequenza a cui la porta si chiude in un fotogramma solo.
    ///
    /// Le prove che parlano d'altro — il conto dei fotogrammi, i filtri, lo
    /// svuotamento — non devono attraversare una dissolvenza per arrivare al
    /// punto: a cento fotogrammi al secondo dodici millisecondi sono un
    /// fotogramma, e la porta si comporta come l'interruttore di prima.
    const SECCA: u32 = 100;

    /// Una frequenza a cui la porta si chiude in dodici fotogrammi: abbastanza
    /// per guardarla muoversi, poco abbastanza per stare in un blocco di prova.
    const LENTA: u32 = 1_000;

    /// Il battito delle prove che non parlano di tempo: nessuna frequenza,
    /// quindi nessuna ancora e nessun avvio programmato. Il giro che ne esce è
    /// quello di sempre.
    const SENZA_OROLOGIO: Battito = Battito {
        playback_ns: 0,
        anticipo_ns: 0,
        frequenza: 0,
        ingresso_ns: 0,
    };

    /// Le caselle condivise di un motore che sta suonando, con tanti canali.
    ///
    /// Da `Condiviso::nuovo` e non scritte a mano: le caselle crescono, e una
    /// copia del valore di riposo qui dentro sarebbe la seconda verità da
    /// tenere in pari con la prima.
    fn condiviso(canali: u16) -> Arc<Condiviso> {
        let condiviso = Condiviso::nuovo();
        condiviso.in_pausa.store(false, Ordering::Release);
        condiviso.canali.store(u32::from(canali), Ordering::Relaxed);
        condiviso
    }

    /// La voce di una callback che sta già suonando: porta aperta e volume al
    /// voluto, così le prove partono dal regime e non da una salita.
    ///
    /// Restituisce anche la maniglia per mandarle delle curve. La spia non
    /// guarda: le prove di questo file parlano della callback, non dello
    /// spettro, e il suo anello nessuno lo legge.
    fn voce(lettore: rtrb::Consumer<f32>, frequenza: u32) -> (rtrb::Producer<Coefficienti>, Voce) {
        let (manda, ricevi) = rtrb::RingBuffer::<Coefficienti>::new(4);
        let (spia, _tenuto) = rtrb::RingBuffer::<f32>::new(8);
        let mut andamento = Andamento::nuovo(frequenza);
        andamento.guadagno = 1.0;
        andamento.porta = andamento.corsa;
        (
            manda,
            Voce {
                lettore,
                andamento,
                filtro: Filtro {
                    stato: StatoEq::nuovo(),
                    ricevi,
                },
                spia: Spia {
                    manda: spia,
                    somma: 0.0,
                    quanti: 0,
                },
                veglia: Arc::new(Veglia::nuova()),
            },
        )
    }

    /// Un anello con `quanti` campioni tutti uguali dentro.
    fn anello(capacita: usize, quanti: usize, valore: f32) -> rtrb::Consumer<f32> {
        let (mut scrittore, lettore) = rtrb::RingBuffer::<f32>::new(capacita);
        for _ in 0..quanti {
            let _ = scrittore.push(valore);
        }
        lettore
    }

    /// L'aritmetica della latenza, che è l'unica metà provabile.
    ///
    /// [`annota_latenza`] non ha una prova sua, e non è una dimenticanza:
    /// `cpal::OutputCallbackInfo` lo costruisce il backend audio con gli
    /// istanti del suo orologio. Provarla vorrebbe dire un dispositivo vero,
    /// cioè una prova che sulla CI non gira. Quel che si poteva separare è
    /// questa conversione; quel che resta nella funzione è una lettura di due
    /// campi `Copy` e uno `store`.
    #[test]
    fn la_latenza_si_converte_in_fotogrammi() {
        // Il buffer tipico di WASAPI in modalità condivisa: dieci millisecondi,
        // che a 48 kHz sono 480 fotogrammi.
        assert_eq!(fotogrammi_di(Duration::from_millis(10), 48_000), 480);
        assert_eq!(fotogrammi_di(Duration::ZERO, 48_000), 0);
        // E il resto si perde verso il basso: mezzo fotogramma non è un
        // fotogramma.
        assert_eq!(fotogrammi_di(Duration::from_micros(10), 48_000), 0);
    }

    // ── la porta e la rampa ──

    #[test]
    fn la_porta_dura_una_dozzina_di_millisecondi() {
        // A 48 kHz dodici millisecondi sono 576 fotogrammi, a 44,1 kHz 529.
        assert_eq!(corsa_della_porta(48_000), 576);
        assert_eq!(corsa_della_porta(44_100), 529);
        // E almeno un fotogramma anche dove dodici millisecondi non ne fanno
        // nemmeno uno: una porta di zero fotogrammi sarebbe il gradino di prima.
        assert_eq!(corsa_della_porta(10), 1);
        assert_eq!(corsa_della_porta(0), 1);
    }

    #[test]
    fn la_porta_si_chiude_e_si_riapre_in_una_corsa_esatta() {
        let mut andamento = Andamento::nuovo(48_000);
        andamento.porta = andamento.corsa;
        assert_eq!(andamento.apertura().to_bits(), 1.0f32.to_bits());
        let mut precedente = andamento.apertura();
        let mut passi = 0;
        while andamento.porta > 0 {
            andamento.muovi_porta(false);
            passi += 1;
            let adesso = andamento.apertura();
            assert!(adesso < precedente, "la porta non scende: {adesso}");
            precedente = adesso;
        }
        // Zero esatto, non un residuo: una porta «quasi chiusa» moltiplicata
        // per un campione darebbe un numero piccolissimo invece del silenzio.
        assert_eq!(passi, 576);
        assert_eq!(andamento.apertura().to_bits(), 0.0f32.to_bits());
        andamento.muovi_porta(false);
        assert_eq!(andamento.porta, 0, "chiusa resta chiusa");
        // E all'insù lo stesso: uno esatto, e lì si ferma.
        for _ in 0..1_000 {
            andamento.muovi_porta(true);
        }
        assert_eq!(andamento.porta, andamento.corsa);
        assert_eq!(andamento.apertura().to_bits(), 1.0f32.to_bits());
    }

    #[test]
    fn la_rampa_verso_lo_zero_non_finisce_fra_i_subnormali() {
        // Il difetto che questa prova impedisce: `g += (0 − g) × 0,0005` non
        // arriva mai a zero, e dopo qualche decina di migliaia di campioni il
        // guadagno diventava subnormale — cioè decine di volte più caro da
        // moltiplicare, dentro la callback in tempo reale.
        let mut guadagno = 1.0f32;
        for _ in 0..1_000_000 {
            guadagno = avvicina(guadagno, 0.0);
            assert!(!guadagno.is_subnormal(), "subnormale: {guadagno:e}");
        }
        assert_eq!(guadagno.to_bits(), 0.0f32.to_bits(), "doveva agganciarsi");
    }

    #[test]
    fn la_rampa_si_aggancia_anche_all_insu() {
        let mut guadagno = 0.0f32;
        for _ in 0..1_000_000 {
            guadagno = avvicina(guadagno, 0.7);
        }
        assert_eq!(guadagno.to_bits(), 0.7f32.to_bits());
        // E un passo solo, lontano dal bersaglio, resta un passo di rampa.
        let uno = avvicina(0.0, 1.0);
        assert!((uno - RAMPA).abs() < f32::EPSILON, "{uno}");
    }

    // ── pausa, ripresa e salto ──

    #[test]
    fn a_porta_chiusa_in_pausa_esce_silenzio_e_l_anello_non_si_consuma() {
        let stato = condiviso(2);
        stato.in_pausa.store(true, Ordering::Release);
        let (_manda, mut voce) = voce(anello(16, 8, 0.9), LENTA);
        voce.andamento.porta = 0;
        let mut dati = [1.0f32; 4];
        riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
        assert_eq!(dati, [0.0; 4]);
        // Niente è stato consumato: riprendere non deve aspettare.
        assert_eq!(voce.lettore.slots(), 8);
        assert_eq!(stato.fotogrammi.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn la_pausa_sfuma_prima_di_tacere() {
        // Il clic che questa prova impedisce: il ramo della pausa scriveva zeri
        // dal primo campione, e l'onda passava da dove si trovava a zero in un
        // colpo solo.
        let stato = condiviso(2);
        stato.in_pausa.store(true, Ordering::Release);
        let (_manda, mut voce) = voce(anello(128, 128, 0.5), LENTA);
        let mut dati = [9.0f32; 40];
        riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);

        // A 1 kHz la porta si chiude in dodici fotogrammi: i primi undici
        // escono, sempre più piano, e da lì solo silenzio.
        let mut precedente = f32::INFINITY;
        for (n, fotogramma) in dati.chunks(2).enumerate() {
            let [sinistro, destro] = fotogramma else {
                panic!("fotogramma spezzato");
            };
            assert!(
                (sinistro - destro).abs() < f32::EPSILON,
                "i due canali di uno stesso istante escono insieme"
            );
            assert!(*sinistro <= precedente, "fotogramma {n} risale");
            precedente = *sinistro;
            if n < 11 {
                assert!(*sinistro > 0.0, "fotogramma {n} già muto");
            } else {
                assert_eq!(*sinistro, 0.0, "fotogramma {n} suona ancora");
            }
        }
        // Quel che è sfumato è uscito dall'anello, ed è stato contato: la
        // ripresa riparte dal campione dopo.
        assert_eq!(voce.lettore.slots(), 128 - 22);
        assert_eq!(stato.fotogrammi.load(Ordering::Relaxed), 11);
        // E con la porta chiusa il volume è tornato a zero: la ripresa salirà.
        assert_eq!(voce.andamento.guadagno.to_bits(), 0.0f32.to_bits());
        // Una pausa non è un disco lento: il silenzio chiesto non si conta fra
        // i buchi.
        assert_eq!(stato.vuoti.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn la_ripresa_sale_da_zero_senza_gradino() {
        let stato = condiviso(2);
        let (_manda, mut voce) = voce(anello(512, 512, 0.8), LENTA);
        // Dove la pausa l'ha lasciata: porta chiusa, volume a zero.
        voce.andamento.porta = 0;
        voce.andamento.guadagno = 0.0;
        let mut dati = [0.0f32; 256];
        riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
        let primo = dati.first().copied().unwrap_or(1.0);
        assert!(primo < 0.01, "primo campione della ripresa: {primo}");
        // Sale e non torna indietro: un passo all'indietro sarebbe un tremolio.
        for coppia in dati.windows(2) {
            let [prima, dopo] = coppia else { continue };
            assert!(dopo >= prima, "{dopo} dopo {prima}");
        }
        assert_eq!(
            voce.andamento.porta, voce.andamento.corsa,
            "la porta doveva aprirsi del tutto"
        );
    }

    #[test]
    fn una_ripresa_a_meta_dissolvenza_torna_su_da_dove_era() {
        // Pausa e ripresa più vicine della dissolvenza: la porta è a metà, e la
        // ripresa deve ripartire da lì — non da uno, che sarebbe il gradino, né
        // da zero, che sarebbe un buco.
        let stato = condiviso(1);
        let (_manda, mut voce) = voce(anello(64, 64, 1.0), LENTA);
        voce.andamento.porta = 6;
        let mut dati = [0.0f32; 1];
        riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
        let uscito = dati.first().copied().unwrap_or(0.0);
        // Sei fotogrammi su dodici, più il passo di questo: sette dodicesimi.
        assert!((uscito - 7.0 / 12.0).abs() < 1e-6, "{uscito}");
    }

    #[test]
    fn a_porta_chiusa_lo_svuotamento_scarta_tutto_e_si_disarma() {
        let stato = condiviso(2);
        stato.svuota.store(true, Ordering::Release);
        let (_manda, mut voce) = voce(anello(16, 8, 0.9), LENTA);
        voce.andamento.porta = 0;
        let mut dati = [1.0f32; 4];
        riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
        assert_eq!(dati, [0.0; 4]);
        assert_eq!(voce.lettore.slots(), 0, "l'anello doveva restare vuoto");
        assert!(!stato.svuota.load(Ordering::Acquire), "doveva disarmarsi");
    }

    #[test]
    fn un_salto_a_musica_in_corso_sfuma_il_punto_vecchio_prima_di_buttarlo() {
        // Il salto era il terzo clic: lo svuotamento scriveva zeri al primo
        // blocco, a metà onda. Adesso il punto vecchio sfuma, e solo a porta
        // chiusa l'anello si svuota e la bandiera si abbassa.
        let stato = condiviso(1);
        stato.svuota.store(true, Ordering::Release);
        let (_manda, mut voce) = voce(anello(64, 64, 1.0), LENTA);

        // Un blocco più corto della dissolvenza: la porta non è ancora chiusa,
        // e la bandiera deve restare alzata — chi aspetta dall'altra parte
        // aspetta ancora.
        let mut primo = [0.0f32; 6];
        riempi(&mut primo, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
        assert!(primo.iter().all(|c| *c > 0.0), "{primo:?}");
        assert!(stato.svuota.load(Ordering::Acquire), "troppo presto");

        let mut secondo = [9.0f32; 10];
        riempi(&mut secondo, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
        assert!(!stato.svuota.load(Ordering::Acquire), "doveva disarmarsi");
        assert_eq!(voce.lettore.slots(), 0, "il punto vecchio doveva sparire");
        assert_eq!(secondo.last().copied(), Some(0.0));
        // Il punto nuovo salirà da zero.
        assert_eq!(voce.andamento.guadagno.to_bits(), 0.0f32.to_bits());
    }

    #[test]
    #[expect(clippy::integer_division, reason = "campioni stereo in fotogrammi")]
    fn chi_aspetta_lo_svuotamento_non_perde_i_campioni_spinti_subito_dopo() {
        // Il protocollo è quello di `Contesto::scarta_in_volo`: il filo della
        // decodifica alza `svuota`, resta fermo finché non la vede abbassata —
        // perché solo allora l'anello è certamente vuoto — e **solo allora**
        // azzera i contatori e ricomincia a spingere.
        //
        // Finché la bandiera si abbassava all'inizio della callback, quella
        // attesa finiva prima dello svuotamento: i campioni spinti subito dopo
        // finivano dentro il `while` che stava ancora svuotando, e uscivano dal
        // conto dei fotogrammi per sempre. Il debito non si richiudeva più, e a
        // pagarlo era la fine della coda — vedi il commento in `svuota_anello`,
        // e `Contesto::forse_fine`.
        //
        // Alla frequenza vera: la porta impiega qualche blocco a chiudersi, ed
        // è proprio in quei blocchi che la bandiera deve restare alzata.
        const ANELLO: usize = 65_536;
        const DA_CONTARE: usize = 4_096;
        let (mut scrittore, lettore) = rtrb::RingBuffer::<f32>::new(ANELLO);
        let stato = condiviso(2);
        let fermati = Arc::new(AtomicBool::new(false));

        let callback = {
            let stato = Arc::clone(&stato);
            let fermati = Arc::clone(&fermati);
            std::thread::spawn(move || {
                let (_manda, mut voce) = voce(lettore, 48_000);
                let mut dati = [0.0f32; 512];
                while !fermati.load(Ordering::Acquire) {
                    riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
                    // Il passo di un dispositivo vero, e non un giro a vuoto il
                    // più in fretta possibile: senza, questo filo svuoterebbe
                    // l'anello in pochi microsecondi e la finestra da riprodurre
                    // non si aprirebbe mai.
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
        };

        // Prima si aspetta che la callback stia davvero girando — con lei non
        // ancora partita la bandiera resterebbe alzata fino al suo primo giro, e
        // lo svuotamento sarebbe finito prima ancora di cominciare.
        let _ = scrittore.push(0.9);
        for _ in 0..1_000 {
            if stato.fotogrammi.load(Ordering::Relaxed) > 0 || scrittore.slots() == ANELLO {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        // Poi l'anello si riempie fino all'orlo, perché il `while` che lo svuota
        // ci metta un tempo misurabile: è la finestra in cui il difetto viveva.
        while scrittore.push(0.9).is_ok() {}

        stato.svuota.store(true, Ordering::Release);
        // L'attesa del filo, con un giro stretto invece dei millisecondi veri:
        // qui non si vuole dormire *attraverso* l'istante in cui la bandiera si
        // abbassa, perché è proprio quell'istante che la prova guarda.
        let scadenza = Instant::now() + Duration::from_secs(5);
        while stato.svuota.load(Ordering::Acquire) && Instant::now() < scadenza {
            std::hint::spin_loop();
        }
        assert!(
            !stato.svuota.load(Ordering::Acquire),
            "la callback non ha mai abbassato la bandiera"
        );
        // L'invariante, guardata dal lato che la usa: quando il filo vede la
        // bandiera abbassata, l'anello **è** vuoto. È la promessa su cui
        // `scarta_in_volo` azzera i contatori.
        assert_eq!(
            scrittore.slots(),
            ANELLO,
            "la bandiera si è abbassata con l'anello ancora pieno"
        );
        // Da qui in poi il filo azzera il conto e ricomincia a spingere: è
        // esattamente quel che fa `scarta_in_volo`.
        stato.fotogrammi.store(0, Ordering::Relaxed);
        let mut spinti = 0usize;
        while spinti < DA_CONTARE {
            if scrittore.push(0.5).is_ok() {
                spinti = spinti.saturating_add(1);
            }
        }

        // Il tempo che la callback consumi quel che è stato spinto.
        for _ in 0..1_000 {
            if stato.fotogrammi.load(Ordering::Relaxed) >= (DA_CONTARE / 2) as u64 {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        fermati.store(true, Ordering::Release);
        let _ = callback.join();

        assert_eq!(
            stato.fotogrammi.load(Ordering::Relaxed),
            (DA_CONTARE / 2) as u64,
            "la callback ha contato meno fotogrammi di quanti ne sono stati \
             spinti: lo svuotamento si è mangiato i campioni arrivati dopo"
        );
    }

    // ── il conto dei fotogrammi ──

    #[test]
    fn un_anello_vuoto_da_silenzio_e_lo_conta() {
        let stato = condiviso(2);
        let (_manda, mut voce) = voce(anello(16, 0, 0.0), SECCA);
        let mut dati = [1.0f32; 4];
        riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
        assert_eq!(dati, [0.0; 4]);
        assert_eq!(stato.vuoti.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn i_fotogrammi_si_contano_per_canale_non_per_campione() {
        let stato = condiviso(2);
        let (_manda, mut voce) = voce(anello(16, 8, 0.5), SECCA);
        let mut dati = [0.0f32; 8];
        riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
        // Otto campioni su due canali sono quattro fotogrammi.
        assert_eq!(stato.fotogrammi.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn un_fotogramma_a_cavallo_di_due_blocchi_si_conta_una_volta() {
        // Il difetto che questa prova impedisce: `presi / canali` buttava il
        // resto. Con l'anello che si svuota a metà fotogramma, i campioni presi
        // non sono un multiplo dei canali e il fotogramma spezzato non veniva
        // contato da nessuno dei due blocchi. L'errore non si compensa — va
        // sempre nella stessa direzione — quindi su un disco lento il cursore
        // resta indietro sempre di più, in silenzio.
        let (mut scrittore, lettore) = rtrb::RingBuffer::<f32>::new(64);
        let stato = condiviso(2);
        let (_manda, mut voce) = voce(lettore, SECCA);

        // Tre campioni per un'uscita da quattro: un fotogramma e mezzo su due
        // canali. L'anello si secca esattamente in mezzo al secondo.
        for _ in 0..3 {
            let _ = scrittore.push(0.5);
        }
        let mut dati = [0.0f32; 4];
        riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
        assert_eq!(
            stato.fotogrammi.load(Ordering::Relaxed),
            1,
            "un fotogramma intero è uscito, il secondo è a metà"
        );

        // Il compagno del campione spaiato arriva adesso: insieme fanno il
        // secondo fotogramma, e va contato.
        let _ = scrittore.push(0.5);
        let mut ancora = [0.0f32; 2];
        riempi(&mut ancora, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
        assert_eq!(
            stato.fotogrammi.load(Ordering::Relaxed),
            2,
            "quattro campioni su due canali sono due fotogrammi, comunque \
             siano stati consegnati"
        );
    }

    #[test]
    fn lo_svuotamento_dimentica_anche_il_fotogramma_a_meta() {
        // Dopo un salto il decodificatore azzera il conteggio: un resto portato
        // oltre lo svuotamento conterebbe un fotogramma del punto vecchio dentro
        // quello nuovo.
        let stato = condiviso(2);
        let (_manda, mut voce) = voce(anello(64, 1, 0.5), SECCA);
        let mut dati = [0.0f32; 2];
        riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
        assert_eq!(
            voce.andamento.resto, 1,
            "un campione spaiato è rimasto in sospeso"
        );

        stato.svuota.store(true, Ordering::Release);
        let mut silenzio = [0.0f32; 2];
        riempi(&mut silenzio, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
        assert_eq!(
            voce.andamento.resto, 0,
            "il fotogramma a metà era del punto di prima"
        );
    }

    #[test]
    fn il_guadagno_sale_per_rampa_e_non_di_colpo() {
        let stato = condiviso(2);
        let (_manda, mut voce) = voce(anello(64, 64, 1.0), SECCA);
        voce.andamento.guadagno = 0.0;
        let mut dati = [0.0f32; 64];
        riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
        // Il primo campione è quasi zero, non uno: nessun gradino.
        let primo = dati.first().copied().unwrap_or(1.0);
        assert!(primo < 0.01, "primo campione: {primo}");
        assert!(*dati.last().unwrap_or(&0.0) > primo);
    }

    // ── le conversioni ──

    #[test]
    fn il_taglio_evita_che_un_picco_avvolga_il_segno() {
        let stato = condiviso(2);
        let (_manda, mut voce) = voce(anello(8, 4, 4.0), SECCA);
        let mut dati = [0i16; 4];
        riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, a_i16);
        assert!(dati.iter().all(|&v| v > 0), "un picco è diventato negativo");
    }

    #[test]
    fn l_uscita_in_virgola_mobile_taglia_i_campioni_fuori_scala() {
        // Il caso che il bersaglio «alto» della normalizzazione rende comune:
        // una correzione ReplayGain che porta un brano già forte sopra l'uno.
        // Prima questa uscita passava il valore com'era, e cosa ne facesse il
        // driver non era scritto da nessuna parte.
        assert!(
            (a_f32(1.8) - 1.0).abs() < f32::EPSILON,
            "non ha tagliato sopra"
        );
        assert!(
            (a_f32(-1.8) + 1.0).abs() < f32::EPSILON,
            "non ha tagliato sotto"
        );
        // E per tutto il resto, che è quasi sempre, non deve toccare niente.
        for dentro in [0.0f32, 0.5, -0.5, 1.0, -1.0] {
            assert!(
                (a_f32(dentro) - dentro).abs() < f32::EPSILON,
                "ha cambiato un campione che stava già in scala: {dentro}"
            );
        }
        // Un campione che non è un numero esce silenzio, non `NaN`: `clamp`
        // da solo lo lasciava passare fino al driver.
        assert!(a_f32(f32::NAN).abs() < f32::EPSILON);
        assert!(a_f64(f32::NAN).abs() < f64::EPSILON);
        assert!((a_f32(f32::INFINITY) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn le_uscite_larghe_tagliano_e_non_avvolgono() {
        // I formati che cpal 0.18 sa consegnare: 24 e 32 bit interi, 64 in
        // virgola mobile. Stessa regola dei 16 bit — il fondo scala è il tetto,
        // e un picco oltre il tetto resta un picco, non un segno ribaltato.
        assert_eq!(a_i32(4.0), 2_147_483_647);
        assert_eq!(a_i32(-4.0), -2_147_483_647);
        assert_eq!(a_i32(0.0), 0);
        assert_eq!(a_i24(4.0).inner(), 8_388_607);
        assert_eq!(a_i24(-4.0).inner(), -8_388_607);
        assert_eq!(a_i24(0.0).inner(), 0);
        assert!((a_f64(1.8) - 1.0).abs() < f64::EPSILON);
        assert!((a_f64(0.25) - 0.25).abs() < f64::EPSILON);
    }

    // ── l'equalizzatore ──

    #[test]
    fn un_filtro_piatto_non_cambia_l_uscita() {
        let (mut scrittore, lettore) = rtrb::RingBuffer::<f32>::new(8);
        for valore in [0.1f32, -0.2, 0.3, -0.4] {
            let _ = scrittore.push(valore);
        }
        let stato = condiviso(2);
        let (_manda, mut voce) = voce(lettore, SECCA);
        let mut dati = [0.0f32; 4];
        riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
        // Il guadagno parte già a uno, quindi la rampa non sposta niente di
        // percettibile: quel che è entrato è quel che esce.
        for (uscito, atteso) in dati.iter().zip([0.1f32, -0.2, 0.3, -0.4]) {
            assert!(
                (uscito - atteso).abs() < 1e-3,
                "{uscito} invece di {atteso}"
            );
        }
    }

    #[test]
    fn una_curva_mandata_viene_adottata_subito() {
        let stato = condiviso(2);
        let (mut manda, mut voce) = voce(anello(64, 64, 0.5), SECCA);
        // Tutto abbassato: il preamp resta a uno e i filtri tagliano.
        let _ = manda.push(Coefficienti::calcola(&[-LIMITE_DB; BANDE], true, 48_000));
        let mut dati = [0.0f32; 64];
        riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
        assert!(
            !voce.filtro.stato.piatto(),
            "la curva doveva essere ritirata"
        );
        let ultimo = dati.last().copied().unwrap_or(0.5);
        assert!(
            ultimo.abs() < 0.5,
            "l'ultimo campione vale {ultimo}: i filtri non hanno toccato niente"
        );
    }

    #[test]
    fn lo_svuotamento_azzera_anche_la_coda_dei_filtri() {
        // Il difetto che questa prova impedisce: dopo un salto, la coda dei
        // filtri continua a suonare il punto vecchio sopra quello nuovo.
        let (mut scrittore, lettore) = rtrb::RingBuffer::<f32>::new(128);
        for _ in 0..64 {
            let _ = scrittore.push(0.9);
        }
        let stato = condiviso(2);
        let (mut manda, mut voce) = voce(lettore, SECCA);
        let _ = manda.push(Coefficienti::calcola(&[LIMITE_DB; BANDE], true, 48_000));
        let mut dati = [0.0f32; 64];
        riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);

        stato.svuota.store(true, Ordering::Release);
        let mut silenzio = [0.0f32; 4];
        riempi(&mut silenzio, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
        assert!(!stato.svuota.load(Ordering::Acquire), "doveva disarmarsi");

        // Con la memoria azzerata, un silenzio in ingresso dà un silenzio in
        // uscita. Senza, i filtri avrebbero continuato a scaricare quel che si
        // ricordavano di prima del salto.
        for _ in 0..8 {
            let _ = scrittore.push(0.0);
        }
        let mut dopo = [1.0f32; 8];
        riempi(&mut dopo, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
        for (n, valore) in dopo.iter().enumerate() {
            assert!(valore.abs() < 1e-6, "campione {n} vale {valore}, non zero");
        }
    }

    // ── quel che la callback racconta al filo dell'uscita ──

    #[test]
    fn ogni_blocco_batte_un_giro() {
        let stato = condiviso(2);
        let (_manda, mut voce) = voce(anello(16, 0, 0.0), SECCA);
        let mut dati = [0.0f32; 4];
        for atteso in 1..=3u64 {
            riempi(&mut dati, &mut voce, &stato, &SENZA_OROLOGIO, |v| v);
            assert_eq!(voce.veglia.giri.load(Ordering::Acquire), atteso);
        }
    }

    #[test]
    fn a_riposo_solo_in_pausa_a_porta_chiusa_e_con_l_anello_vuoto() {
        let stato = condiviso(2);
        let mut dati = [0.0f32; 4];

        // Suonando: mai a riposo, nemmeno con l'anello secco.
        let (_manda, mut vuota) = voce(anello(16, 0, 0.0), SECCA);
        riempi(&mut dati, &mut vuota, &stato, &SENZA_OROLOGIO, |v| v);
        assert!(!vuota.veglia.a_riposo.load(Ordering::Acquire));

        stato.in_pausa.store(true, Ordering::Release);
        // In pausa con l'anello vuoto — la coda finita, il lettore fermato.
        riempi(&mut dati, &mut vuota, &stato, &SENZA_OROLOGIO, |v| v);
        assert!(vuota.veglia.a_riposo.load(Ordering::Acquire));

        // In pausa con l'anello pieno — la pausa di chi riprenderà: no.
        let (_manda, mut piena) = voce(anello(16, 16, 0.5), SECCA);
        piena.andamento.porta = 0;
        riempi(&mut dati, &mut piena, &stato, &SENZA_OROLOGIO, |v| v);
        assert!(!piena.veglia.a_riposo.load(Ordering::Acquire));
    }

    #[test]
    fn gli_avvisi_non_sono_perdite() {
        // Un buco nel buffer o la priorità negata non fermano il flusso: alzare
        // `perso` per loro vorrebbe dire ricostruire il motore per un
        // singhiozzo.
        assert_eq!(codice_errore(cpal::ErrorKind::Xrun), None);
        assert_eq!(codice_errore(cpal::ErrorKind::RealtimeDenied), None);
        assert_eq!(codice_errore(cpal::ErrorKind::DeviceChanged), None);
        // Il dispositivo sparito e il flusso da ricostruire sono «non c'è più».
        assert_eq!(
            codice_errore(cpal::ErrorKind::DeviceNotAvailable),
            Some(CAUSA_SPARITO)
        );
        assert_eq!(
            codice_errore(cpal::ErrorKind::StreamInvalidated),
            Some(CAUSA_SPARITO)
        );
        // Tutto il resto è un guasto.
        assert_eq!(
            codice_errore(cpal::ErrorKind::BackendError),
            Some(CAUSA_GUASTO)
        );
        assert_eq!(
            codice_errore(cpal::ErrorKind::DeviceBusy),
            Some(CAUSA_GUASTO)
        );
    }

    #[test]
    fn un_endpoint_in_transizione_si_riprova() {
        use rymembra_domain::errors::ErrorCodeKind;

        // I due guasti del diario vero, come li consegna `cpal`: l'endpoint
        // che non si crea all'avvio, e l'orologio che non si ottiene a metà
        // riapertura perché il dispositivo è stato invalidato. Tutti e due
        // arrivano come «dispositivo non disponibile», col numero nel testo.
        let creazione = cpal::Error::with_message(
            cpal::ErrorKind::DeviceNotAvailable,
            "Elemento non trovato. (0x8889000F)",
        );
        let orologio = cpal::Error::with_message(
            cpal::ErrorKind::DeviceNotAvailable,
            "Failed to get audio clock: (0x88890004)",
        );
        for guasto in [&creazione, &orologio] {
            let err = errore_di_apertura(guasto);
            assert_eq!(err.code().kind(), ErrorCodeKind::PlaybackDeviceLost);
            assert!(err.is_retryable(), "un endpoint in transizione si riprova");
            assert!(
                err.cause().is_some_and(|c| c.contains("0x8889")),
                "il numero deve restare nel diario"
            );
        }
        // Anche il flusso da ricostruire e l'uscita presa in esclusiva per un
        // attimo: sono attese, non guasti.
        for tipo in [
            cpal::ErrorKind::StreamInvalidated,
            cpal::ErrorKind::DeviceBusy,
        ] {
            assert!(errore_di_apertura(&cpal::Error::new(tipo)).is_retryable());
        }
        // Un formato che il dispositivo rifiuta resta quel che era: riprovare
        // fra un secondo non lo cambia.
        let formato = errore_di_apertura(&cpal::Error::new(cpal::ErrorKind::UnsupportedConfig));
        assert_eq!(
            formato.code().kind(),
            ErrorCodeKind::PlaybackEngineUnavailable
        );
        assert!(!formato.is_retryable());
    }

    #[test]
    fn un_campanello_di_un_uscita_caduta_non_fa_niente() {
        // Il campanello è debole: caduto il mittente forte, suonarlo deve
        // tornare subito invece di aspettare una risposta che nessuno darà, e
        // soprattutto non deve aver tenuto vivo il canale.
        let (manda, ricevi) = mpsc::channel::<Ordine>();
        let forte = Arc::new(manda);
        let campanello = Campanello {
            ordini: Arc::downgrade(&forte),
        };
        drop(forte);
        let prima = Instant::now();
        campanello.suona();
        assert!(prima.elapsed() < ATTESA_RISVEGLIO, "ha aspettato il vuoto");
        assert!(
            matches!(
                ricevi.recv_timeout(Duration::from_millis(10)),
                Err(mpsc::RecvTimeoutError::Disconnected)
            ),
            "il campanello ha tenuto vivo il mittente"
        );
    }

    // ── il riposo del flusso ──

    fn sguardo(in_pausa: bool, a_riposo: bool) -> Sguardo {
        Sguardo {
            in_pausa,
            svuota: false,
            a_riposo,
            sessione: false,
        }
    }

    /// Un `Sonno` a flusso acceso, come dopo il primo campanello.
    fn sveglio() -> Sonno {
        sveglio_dopo(RIPOSO_DOPO)
    }

    /// Lo stesso, su un'uscita che riposa dopo `riposo_in_pausa` di pausa.
    fn sveglio_dopo(riposo_in_pausa: Duration) -> Sonno {
        let mut sonno = Sonno::nuovo(riposo_in_pausa);
        assert!(sonno.sveglia(), "appena aperto, il flusso riposa");
        sonno
    }

    #[test]
    fn appena_aperto_il_flusso_riposa_finche_nessuno_suona() {
        // Niente è ancora stato suonato: un flusso acceso a vuoto è proprio
        // quel che teneva sveglio il computer.
        let mut sonno = Sonno::nuovo(RIPOSO_DOPO);
        let t0 = Instant::now();
        assert_eq!(sonno.passo(sguardo(true, true), t0), Mossa::Niente);
        assert!(sonno.dorme);
        // Il campanello lo mette in moto una volta sola.
        assert!(sonno.sveglia());
        assert!(!sonno.sveglia(), "già sveglio: niente da rimettere in moto");
    }

    #[test]
    fn trenta_secondi_di_pausa_mettono_il_flusso_a_riposo() {
        let mut sonno = sveglio();
        let t0 = Instant::now();
        assert_eq!(sonno.passo(sguardo(true, false), t0), Mossa::Niente);
        let quasi = t0 + RIPOSO_DOPO - Duration::from_secs(1);
        assert_eq!(sonno.passo(sguardo(true, false), quasi), Mossa::Niente);
        assert_eq!(
            sonno.passo(sguardo(true, false), t0 + RIPOSO_DOPO),
            Mossa::Riposa
        );
        assert!(sonno.dorme);
        // A riposo non si riposa due volte.
        let dopo = t0 + RIPOSO_DOPO + Duration::from_secs(5);
        assert_eq!(sonno.passo(sguardo(true, false), dopo), Mossa::Niente);
    }

    #[test]
    fn suonare_anche_un_istante_azzera_il_conto() {
        let mut sonno = sveglio();
        let t0 = Instant::now();
        assert_eq!(sonno.passo(sguardo(true, false), t0), Mossa::Niente);
        let meta = t0 + Duration::from_secs(20);
        assert_eq!(sonno.passo(sguardo(false, false), meta), Mossa::Niente);
        // Di nuovo in pausa: il conto riparte da qui, non da `t0`.
        assert_eq!(
            sonno.passo(sguardo(true, false), meta + Duration::from_secs(1)),
            Mossa::Niente
        );
        assert_eq!(
            sonno.passo(sguardo(true, false), t0 + RIPOSO_DOPO),
            Mossa::Niente,
            "il conto doveva ripartire dalla seconda pausa"
        );
        assert_eq!(
            sonno.passo(
                sguardo(true, false),
                meta + Duration::from_secs(1) + RIPOSO_DOPO
            ),
            Mossa::Riposa
        );
    }

    #[test]
    fn anche_il_campanello_azzera_il_conto() {
        // Chi salta in pausa sta ancora ascoltando.
        let mut sonno = sveglio();
        let t0 = Instant::now();
        assert_eq!(sonno.passo(sguardo(true, false), t0), Mossa::Niente);
        assert!(!sonno.sveglia());
        let dopo = t0 + Duration::from_secs(25);
        assert_eq!(sonno.passo(sguardo(true, false), dopo), Mossa::Niente);
        assert_eq!(
            sonno.passo(sguardo(true, false), t0 + RIPOSO_DOPO),
            Mossa::Niente
        );
    }

    #[test]
    fn senza_niente_da_suonare_il_riposo_arriva_quasi_subito() {
        // La coda finita o il lettore fermato: l'anello è vuoto, e trenta
        // secondi di attesa non proteggono nessuna ripresa istantanea.
        let mut sonno = sveglio();
        let t0 = Instant::now();
        assert_eq!(sonno.passo(sguardo(true, true), t0), Mossa::Niente);
        assert_eq!(
            sonno.passo(sguardo(true, true), t0 + RIPOSO_A_VUOTO),
            Mossa::Riposa
        );
    }

    #[test]
    fn un_anello_che_si_riempie_torna_ai_trenta_secondi() {
        let mut sonno = sveglio();
        let t0 = Instant::now();
        assert_eq!(sonno.passo(sguardo(true, true), t0), Mossa::Niente);
        // Il decodificatore ha riempito l'anello: era una pausa vera.
        assert_eq!(
            sonno.passo(sguardo(true, false), t0 + Duration::from_millis(500)),
            Mossa::Niente
        );
        assert_eq!(
            sonno.passo(sguardo(true, false), t0 + RIPOSO_A_VUOTO * 3),
            Mossa::Niente
        );
    }

    #[test]
    fn con_uno_svuotamento_in_corso_non_si_riposa() {
        // Il filo della decodifica sta aspettando proprio la callback.
        let mut sonno = sveglio();
        let t0 = Instant::now();
        let aspetta = Sguardo {
            in_pausa: true,
            svuota: true,
            a_riposo: true,
            sessione: false,
        };
        assert_eq!(sonno.passo(aspetta, t0), Mossa::Niente);
        assert_eq!(sonno.passo(aspetta, t0 + RIPOSO_DOPO * 2), Mossa::Niente);
        assert!(!sonno.dorme);
    }

    #[test]
    fn sul_bluetooth_la_pausa_libera_le_cuffie_in_un_secondo() {
        // Le cuffie multipoint passano al telefono solo se il computer smette
        // di mandare: trenta secondi di silenzio le tenevano occupate.
        let mut sonno = sveglio_dopo(RIPOSO_BLUETOOTH);
        let t0 = Instant::now();
        assert_eq!(sonno.passo(sguardo(true, false), t0), Mossa::Niente);
        assert_eq!(
            sonno.passo(sguardo(true, false), t0 + Duration::from_millis(500)),
            Mossa::Niente,
            "mezzo secondo: la porta e il buffer non hanno ancora finito"
        );
        assert_eq!(
            sonno.passo(sguardo(true, false), t0 + RIPOSO_BLUETOOTH),
            Mossa::Riposa
        );
        assert!(sonno.dorme);
    }

    #[test]
    fn sul_bluetooth_suonare_azzera_il_conto_lo_stesso() {
        let mut sonno = sveglio_dopo(RIPOSO_BLUETOOTH);
        let t0 = Instant::now();
        assert_eq!(sonno.passo(sguardo(true, false), t0), Mossa::Niente);
        let ripreso = t0 + Duration::from_millis(800);
        assert_eq!(sonno.passo(sguardo(false, false), ripreso), Mossa::Niente);
        // Di nuovo in pausa: il secondo si conta da qui.
        let ancora = ripreso + Duration::from_millis(100);
        assert_eq!(sonno.passo(sguardo(true, false), ancora), Mossa::Niente);
        assert_eq!(
            sonno.passo(sguardo(true, false), t0 + RIPOSO_BLUETOOTH),
            Mossa::Niente,
            "il conto doveva ripartire dalla seconda pausa"
        );
        assert_eq!(
            sonno.passo(sguardo(true, false), ancora + RIPOSO_BLUETOOTH),
            Mossa::Riposa
        );
    }

    #[test]
    fn neanche_sul_bluetooth_si_riposa_con_uno_svuotamento_in_corso() {
        let mut sonno = sveglio_dopo(RIPOSO_BLUETOOTH);
        let t0 = Instant::now();
        let aspetta = Sguardo {
            in_pausa: true,
            svuota: true,
            a_riposo: false,
            sessione: false,
        };
        assert_eq!(sonno.passo(aspetta, t0), Mossa::Niente);
        assert_eq!(
            sonno.passo(aspetta, t0 + RIPOSO_BLUETOOTH * 10),
            Mossa::Niente
        );
        assert!(!sonno.dorme);
    }

    #[test]
    fn fuori_dal_bluetooth_la_pausa_aspetta_ancora_trenta_secondi() {
        // Un ricevitore HDMI o ottico si mangia l'inizio a ogni ripartenza:
        // lì la ripresa istantanea vale più delle cuffie libere.
        let mut sonno = sveglio();
        let t0 = Instant::now();
        assert_eq!(sonno.passo(sguardo(true, false), t0), Mossa::Niente);
        assert_eq!(
            sonno.passo(sguardo(true, false), t0 + RIPOSO_BLUETOOTH * 5),
            Mossa::Niente
        );
        assert_eq!(
            sonno.passo(sguardo(true, false), t0 + RIPOSO_DOPO),
            Mossa::Riposa
        );
    }

    #[test]
    fn la_rete_di_sicurezza_sveglia_chi_suona_senza_campanello() {
        let mut sonno = Sonno::nuovo(RIPOSO_DOPO);
        let t0 = Instant::now();
        // Qualcuno ha tolto la pausa senza chiamare `Uscita::sveglia`.
        assert_eq!(sonno.passo(sguardo(false, false), t0), Mossa::Sveglia);
        assert!(!sonno.dorme);
        // E qualcuno aspetta uno svuotamento a flusso fermo.
        let mut sonno = Sonno::nuovo(RIPOSO_DOPO);
        let aspetta = Sguardo {
            in_pausa: true,
            svuota: true,
            a_riposo: false,
            sessione: false,
        };
        assert_eq!(sonno.passo(aspetta, t0), Mossa::Sveglia);
    }

    // ── il percorso inattivo, identico al bit ───────────────────────────────

    /// Un lato del confronto: una callback con il suo anello e le sue caselle.
    struct Lato {
        voce: Voce,
        stato: Arc<Condiviso>,
        scrittore: rtrb::Producer<f32>,
        curve: rtrb::Producer<Coefficienti>,
    }

    /// Una callback appena aperta: porta chiusa, volume a zero.
    fn lato(canali: u16, frequenza: u32) -> Lato {
        let (scrittore, lettore) = rtrb::RingBuffer::<f32>::new(4_096);
        let stato = condiviso(canali);
        let (curve, mut voce) = voce(lettore, frequenza);
        voce.andamento.porta = 0;
        voce.andamento.guadagno = 0.0;
        Lato {
            voce,
            stato,
            scrittore,
            curve,
        }
    }

    /// Numeri pseudocasuali fra −1,3 e 1,3: oltre il fondo scala apposta, così
    /// anche il limitatore e i tagli delle conversioni passano dal confronto.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "un numero fra −1,3 e 1,3 sta in un f32"
    )]
    fn rumore(seme: &mut u32, quanti: usize) -> Vec<f32> {
        (0..quanti)
            .map(|_| {
                *seme = seme.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (f64::from(*seme >> 8) / f64::from(1u32 << 24) * 2.6 - 1.3) as f32
            })
            .collect()
    }

    /// Un passo del copione: cosa cambia prima del blocco, quanti campioni
    /// entrano nell'anello, quanto è lungo il blocco.
    struct Passo {
        pausa: bool,
        svuota: bool,
        guadagno: f32,
        spettro: bool,
        curva: bool,
        entrano: usize,
        blocco: usize,
    }

    /// Lo stesso copione sulla callback di adesso e su quella di prima: ogni
    /// campione, ogni contatore, ogni pezzo di stato devono coincidere.
    ///
    /// La callback di adesso riceve un battito **vero** — una frequenza, degli
    /// istanti — così il confronto copre anche il lavoro che fa sempre,
    /// pubblicare l'ancora: deve costare tempo, non suono.
    fn stesso_copione<T: Copy>(converti: fn(f32) -> T, in_bit: fn(T) -> u64) {
        let copione = [
            // La ripresa da porta chiusa, col limitatore acceso.
            Passo {
                pausa: false,
                svuota: false,
                guadagno: 1.3,
                spettro: false,
                curva: false,
                entrano: 400,
                blocco: 64,
            },
            // Un blocco che non è un multiplo dei canali.
            Passo {
                pausa: false,
                svuota: false,
                guadagno: 1.3,
                spettro: true,
                curva: false,
                entrano: 0,
                blocco: 7,
            },
            Passo {
                pausa: false,
                svuota: false,
                guadagno: 0.4,
                spettro: false,
                curva: false,
                entrano: 0,
                blocco: 64,
            },
            // La pausa che sfuma a metà blocco, poi il silenzio.
            Passo {
                pausa: true,
                svuota: false,
                guadagno: 0.4,
                spettro: false,
                curva: false,
                entrano: 0,
                blocco: 20,
            },
            Passo {
                pausa: true,
                svuota: false,
                guadagno: 0.4,
                spettro: false,
                curva: false,
                entrano: 0,
                blocco: 64,
            },
            // La ripresa, con l'anello che si secca a metà fotogramma.
            Passo {
                pausa: false,
                svuota: false,
                guadagno: 0.9,
                spettro: false,
                curva: false,
                entrano: 0,
                blocco: 512,
            },
            Passo {
                pausa: false,
                svuota: false,
                guadagno: 0.9,
                spettro: false,
                curva: false,
                entrano: 3,
                blocco: 8,
            },
            Passo {
                pausa: false,
                svuota: false,
                guadagno: 0.9,
                spettro: false,
                curva: true,
                entrano: 101,
                blocco: 64,
            },
            // Un salto: il punto vecchio sfuma, poi sparisce.
            Passo {
                pausa: false,
                svuota: true,
                guadagno: 0.9,
                spettro: false,
                curva: false,
                entrano: 50,
                blocco: 10,
            },
            Passo {
                pausa: false,
                svuota: true,
                guadagno: 0.9,
                spettro: false,
                curva: false,
                entrano: 0,
                blocco: 64,
            },
            Passo {
                pausa: false,
                svuota: false,
                guadagno: 1.0,
                spettro: false,
                curva: false,
                entrano: 300,
                blocco: 128,
            },
        ];
        let mut adesso = lato(2, LENTA);
        let mut prima = lato(2, LENTA);
        let mut seme = 7u32;
        let mut battito = Battito {
            playback_ns: 1_000_000_000,
            anticipo_ns: 9_000_000,
            frequenza: LENTA,
            ingresso_ns: 990_000_000,
        };
        for (numero, passo) in copione.iter().enumerate() {
            let entrano = rumore(&mut seme, passo.entrano);
            for lato in [&mut adesso, &mut prima] {
                lato.stato.in_pausa.store(passo.pausa, Ordering::Release);
                lato.stato.svuota.store(passo.svuota, Ordering::Release);
                lato.stato
                    .guadagno
                    .store(passo.guadagno.to_bits(), Ordering::Relaxed);
                lato.stato.spettro.store(passo.spettro, Ordering::Relaxed);
                if passo.curva {
                    let mut curva = [0.0; BANDE];
                    curva[2] = 6.0;
                    let _ = lato.curve.push(Coefficienti::calcola(&curva, true, 48_000));
                }
                for campione in &entrano {
                    let _ = lato.scrittore.push(*campione);
                }
            }
            let mut fuori_adesso = vec![converti(9.0); passo.blocco];
            let mut fuori_prima = vec![converti(9.0); passo.blocco];
            riempi(
                &mut fuori_adesso,
                &mut adesso.voce,
                &adesso.stato,
                &battito,
                converti,
            );
            riempi_di_prima(&mut fuori_prima, &mut prima.voce, &prima.stato, converti);
            battito.playback_ns += 500_000 * u64::try_from(passo.blocco).unwrap_or(0);

            for (n, (a, b)) in fuori_adesso.iter().zip(&fuori_prima).enumerate() {
                assert_eq!(
                    in_bit(*a),
                    in_bit(*b),
                    "passo {numero}, campione {n}: il percorso inattivo è cambiato"
                );
            }
            let contatori = |l: &Lato| {
                (
                    l.stato.fotogrammi.load(Ordering::Relaxed),
                    l.stato.vuoti.load(Ordering::Relaxed),
                    l.stato.svuota.load(Ordering::Acquire),
                    l.voce.andamento.resto,
                    l.voce.andamento.porta,
                    l.voce.andamento.guadagno.to_bits(),
                    l.voce.veglia.a_riposo.load(Ordering::Acquire),
                    l.voce.lettore.slots(),
                )
            };
            assert_eq!(
                contatori(&adesso),
                contatori(&prima),
                "passo {numero}: lo stato della callback è cambiato"
            );
        }
        // E il lavoro in più c'è stato davvero: l'ancora è stata pubblicata,
        // senza che il suono se ne accorgesse.
        assert!(adesso.stato.ancora.leggi().is_some());
        assert_eq!(adesso.stato.gradini_fatti.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn il_percorso_inattivo_e_identico_al_bit() {
        // La regola della Fase 0: con l'avvio a zero, nessun gradino, nessuna
        // deriva e la sessione spenta, la callback deve suonare **esattamente**
        // come prima — non «quasi», non entro una tolleranza. Chi non usa
        // l'ascolto insieme non deve poter sentire che esiste.
        stesso_copione(|v| v, |v: f32| u64::from(v.to_bits()));
        stesso_copione(a_i16, |v: i16| {
            u64::from(u16::from_ne_bytes(v.to_ne_bytes()))
        });
    }

    // ── l'ancora ────────────────────────────────────────────────────────────

    /// Un milione di nanosecondi: un fotogramma a [`LENTA`], dove i conti del
    /// tempo si fanno a mente.
    const MS: u64 = 1_000_000;

    /// Il battito di un blocco che si sentirà a `playback_ns`, a [`LENTA`].
    const fn battito(playback_ns: u64) -> Battito {
        Battito {
            playback_ns,
            anticipo_ns: 10 * MS,
            frequenza: LENTA,
            ingresso_ns: playback_ns.saturating_sub(10 * MS + 3_000),
        }
    }

    /// Un anello mono in cui ogni campione dice da dove viene: il numero del
    /// fotogramma, in sessantaquattromillesimi. Esatto in un `f32`, e la media
    /// di due vicini cade esattamente a metà.
    fn anello_contato(
        capacita: usize,
        quanti: usize,
    ) -> (rtrb::Producer<f32>, rtrb::Consumer<f32>) {
        let (mut scrittore, lettore) = rtrb::RingBuffer::<f32>::new(capacita);
        for n in 0..quanti {
            let _ = scrittore.push(numero_in_campione(n + 1));
        }
        (scrittore, lettore)
    }

    fn numero_in_campione(n: usize) -> f32 {
        f32::from(u16::try_from(n).unwrap_or(u16::MAX)) / 65_536.0
    }

    /// Da campione a numero di fotogramma, in metà: un numero dispari è una
    /// media fra due fotogrammi.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "i campioni di prova sono numeri di fotogramma piccoli e positivi"
    )]
    fn meta_di(campione: f32) -> u64 {
        (campione * 131_072.0).round() as u64
    }

    #[test]
    fn l_indice_dell_ancora_e_il_contatore_prima_del_blocco() {
        let stato = condiviso(1);
        stato.fotogrammi.store(1_000, Ordering::Relaxed);
        stato.epoca.store(4, Ordering::Release);
        let (_scrittore, lettore) = anello_contato(256, 200);
        let (_manda, mut voce) = voce(lettore, LENTA);
        let mut dati = [0.0f32; 16];
        riempi(&mut dati, &mut voce, &stato, &battito(5_000 * MS), |v| v);
        let ancora = stato
            .ancora
            .leggi()
            .expect("un blocco di musica pubblica l'ancora");
        assert_eq!(ancora.contenuto, 1_000, "il primo fotogramma del blocco");
        assert_eq!(ancora.udito_ns, 5_000 * MS);
        assert_eq!(ancora.controprova_ns, 5_000 * MS - 3_000);
        assert_eq!(ancora.epoca, 4);
        assert!(ancora.in_corso && !ancora.latenza_scartata);

        // Il blocco dopo comincia dove questo ha finito.
        riempi(&mut dati, &mut voce, &stato, &battito(5_016 * MS), |v| v);
        let dopo = stato.ancora.leggi().expect("anche il secondo");
        assert_eq!(dopo.contenuto, 1_016);
        assert_eq!(dopo.udito_ns, 5_016 * MS);
    }

    #[test]
    fn l_ancora_e_esatta_anche_con_zeri_in_testa() {
        // L'avvio programmato tace per i primi tre fotogrammi: l'ancora è il
        // quarto, che si sente tre millesimi di secondo dopo l'inizio del
        // blocco, ed è il fotogramma d'anello che il contatore aveva prima.
        let stato = condiviso(1);
        stato.fotogrammi.store(77, Ordering::Relaxed);
        let (_scrittore, lettore) = anello_contato(256, 200);
        let (_manda, mut voce) = voce(lettore, LENTA);
        voce.andamento.porta = 0;
        stato.avvio_ns.store(2_003 * MS, Ordering::Release);
        let mut dati = [9.0f32; 16];
        riempi(&mut dati, &mut voce, &stato, &battito(2_000 * MS), |v| v);
        assert_eq!(&dati[..3], &[0.0; 3], "prima dell'avvio, silenzio");
        assert!(dati[3] > 0.0, "all'avvio, musica");
        let ancora = stato.ancora.leggi().expect("c'è musica nel blocco");
        assert_eq!(ancora.contenuto, 77);
        assert_eq!(ancora.udito_ns, 2_003 * MS);
        assert_eq!(stato.avvio_ns.load(Ordering::Acquire), 0, "avvio consumato");
    }

    #[test]
    fn nessuna_ancora_senza_musica_o_a_meta_fotogramma() {
        // Senza musica: in pausa a porta chiusa.
        let stato = condiviso(2);
        stato.in_pausa.store(true, Ordering::Release);
        let (_manda, mut ferma) = voce(anello(64, 64, 0.5), LENTA);
        ferma.andamento.porta = 0;
        let mut dati = [0.0f32; 16];
        riempi(&mut dati, &mut ferma, &stato, &battito(MS), |v| v);
        assert_eq!(
            stato.ancora.leggi(),
            None,
            "in pausa non c'è niente da ancorare"
        );

        // Senza musica: l'anello è secco.
        stato.in_pausa.store(false, Ordering::Release);
        let (_manda, mut secca) = voce(anello(64, 0, 0.0), LENTA);
        riempi(&mut dati, &mut secca, &stato, &battito(MS), |v| v);
        assert_eq!(stato.ancora.leggi(), None, "un anello secco non è musica");

        // A metà fotogramma: il primo campione preso è il compagno di uno già
        // contato, e il contatore non dice quale fotogramma sia.
        let (_manda, mut storta) = voce(anello(64, 64, 0.5), LENTA);
        storta.andamento.resto = 1;
        riempi(&mut dati, &mut storta, &stato, &battito(MS), |v| v);
        assert_eq!(stato.ancora.leggi(), None, "un resto aperto non si ancora");
    }

    #[test]
    fn una_latenza_assurda_ancora_alla_callback_e_lo_dice() {
        let stato = condiviso(1);
        let (_scrittore, lettore) = anello_contato(64, 64);
        let (_manda, mut voce) = voce(lettore, LENTA);
        let mut dati = [0.0f32; 4];
        let assurdo = Battito {
            anticipo_ns: (LATENZA_MASSIMA_MS + 1) * MS,
            ..battito(9_000 * MS)
        };
        riempi(&mut dati, &mut voce, &stato, &assurdo, |v| v);
        let ancora = stato.ancora.leggi().expect("c'è musica");
        assert!(ancora.latenza_scartata);
        assert_eq!(ancora.udito_ns, (9_000 - LATENZA_MASSIMA_MS - 1) * MS);
    }

    // ── l'avvio programmato ─────────────────────────────────────────────────

    /// Fa girare blocchi da `blocco` fotogrammi mono finché non esce musica, e
    /// dice a che istante esce il primo campione non nullo e quanti fotogrammi
    /// d'anello erano stati consumati prima.
    fn primo_suono(avvio_ns: u64, inizio_ns: u64, blocco: usize) -> (u64, u64) {
        let stato = condiviso(1);
        let (_manda, mut voce) = voce(anello(4_096, 4_096, 1.0), LENTA);
        voce.andamento.porta = 0;
        voce.andamento.guadagno = 0.0;
        stato.avvio_ns.store(avvio_ns, Ordering::Release);
        let mut playback = inizio_ns;
        for _ in 0..100 {
            let prima = stato.fotogrammi.load(Ordering::Relaxed);
            let mut dati = vec![0.0f32; blocco];
            riempi(&mut dati, &mut voce, &stato, &battito(playback), |v| v);
            if let Some(n) = dati.iter().position(|c| *c != 0.0) {
                return (playback + MS * u64::try_from(n).unwrap_or(0), prima);
            }
            assert_eq!(
                stato.fotogrammi.load(Ordering::Relaxed),
                0,
                "si è consumata musica prima dell'avvio"
            );
            playback += MS * u64::try_from(blocco).unwrap_or(0);
        }
        panic!("la musica non è mai partita");
    }

    #[test]
    fn l_avvio_programmato_e_esatto_a_cavallo_di_piu_blocchi() {
        // Cento millisecondi e tre decimi dopo l'inizio del primo blocco, con
        // blocchi da sessantaquattro: il primo tace tutto e non tocca l'anello,
        // il secondo tace fino al fotogramma che si sente al centounesimo
        // millisecondo — il primo che non suona prima dell'avvio.
        let avvio = 1_000 * MS + 100 * MS + 300_000;
        let (suona, consumati) = primo_suono(avvio, 1_000 * MS, 64);
        assert_eq!(suona, 1_101 * MS);
        assert_eq!(consumati, 0);
    }

    #[test]
    fn l_avvio_non_suona_mai_prima_della_scadenza() {
        // Ogni sfasamento fra l'avvio e la griglia dei fotogrammi, e ogni
        // posizione rispetto ai blocchi: il primo suono non è mai prima
        // dell'avvio, e mai più di un fotogramma dopo.
        for dopo_ns in [
            0,
            1,
            999_999,
            MS,
            MS + 1,
            5 * MS + 500_000,
            63 * MS + 999_999,
            64 * MS,
            200 * MS + 7,
        ] {
            let avvio = 1_000 * MS + dopo_ns;
            let (suona, _) = primo_suono(avvio, 1_000 * MS, 64);
            assert!(
                suona >= avvio,
                "{dopo_ns} ns: suona a {suona}, prima di {avvio}"
            );
            assert!(
                suona < avvio + MS,
                "{dopo_ns} ns: suona a {suona}, troppo dopo {avvio}"
            );
        }
    }

    #[test]
    fn un_avvio_in_ritardo_diventa_un_salto() {
        // Dieci millisecondi e mezzo di ritardo: si parte subito, e i dieci
        // fotogrammi interi che dovevano già essere usciti si saltano.
        let stato = condiviso(1);
        let (_scrittore, lettore) = anello_contato(1_024, 1_000);
        let (_manda, mut voce) = voce(lettore, LENTA);
        voce.andamento.porta = 0;
        voce.andamento.guadagno = 1.0;
        let playback = 3_000 * MS;
        stato
            .avvio_ns
            .store(playback - 10 * MS - 500_000, Ordering::Release);
        let mut dati = [0.0f32; 64];
        riempi(&mut dati, &mut voce, &stato, &battito(playback), |v| v);

        assert_eq!(stato.avvio_ns.load(Ordering::Acquire), 0);
        assert_eq!(stato.gradini_fatti.load(Ordering::Relaxed), 1);
        // Nessun fotogramma che non consuma: i dieci saltati più i
        // sessantaquattro del blocco.
        assert_eq!(stato.fotogrammi.load(Ordering::Relaxed), 74);
        // E l'ancora sa che il primo fotogramma uscito è il decimo.
        let ancora = stato.ancora.leggi().expect("c'è musica");
        assert_eq!(ancora.contenuto, 10);
        assert_eq!(ancora.udito_ns, playback);
        // Il secondo fotogramma del blocco è l'undicesimo dell'anello.
        assert_eq!(meta_di(dati[1] * 12.0), 2 * (11 + 1), "{}", dati[1]);
    }

    // ── il gradino ──────────────────────────────────────────────────────────

    /// Quel che un gradino fa a una callback che sta suonando: fotogrammi
    /// d'anello consumati, fotogrammi d'uscita che non ne hanno consumato
    /// nessuno, e l'ultimo campione uscito.
    fn con_un_gradino(gradino: i64, blocchi: usize) -> (u64, u64, f32, Arc<Condiviso>) {
        let stato = condiviso(1);
        let (_scrittore, lettore) = anello_contato(4_096, 4_000);
        let (_manda, mut voce) = voce(lettore, LENTA);
        if gradino != 0 {
            stato.gradino.store(gradino, Ordering::Release);
        }
        let mut usciti = 0u64;
        let mut ultimo = 0.0;
        for _ in 0..blocchi {
            let mut dati = [0.0f32; 64];
            riempi(&mut dati, &mut voce, &stato, &battito(MS), |v| v);
            usciti += 64;
            ultimo = dati[63];
        }
        let consumati = stato.fotogrammi.load(Ordering::Relaxed);
        (consumati, usciti.saturating_sub(consumati), ultimo, stato)
    }

    #[test]
    fn un_gradino_in_avanti_fa_salire_il_contatore_di_n() {
        let (senza, fermi_senza, ultimo_senza, _) = con_un_gradino(0, 4);
        let (con, fermi_con, ultimo_con, stato) = con_un_gradino(-30, 4);
        assert_eq!(
            con - senza,
            30,
            "trenta fotogrammi saltati, né uno di più né uno di meno"
        );
        assert_eq!(fermi_senza, 0);
        assert_eq!(
            fermi_con, 0,
            "un salto non deve lasciare fotogrammi che non consumano"
        );
        assert_eq!(
            meta_di(ultimo_con) - meta_di(ultimo_senza),
            60,
            "la musica è avanti di trenta"
        );
        assert_eq!(stato.gradini_fatti.load(Ordering::Relaxed), 1);
        assert!(
            !stato.in_correzione.load(Ordering::Acquire),
            "la porta si è riaperta"
        );
    }

    #[test]
    fn un_gradino_all_indietro_lascia_fermo_il_contatore_per_n_fotogrammi() {
        let (senza, _, ultimo_senza, _) = con_un_gradino(0, 4);
        let (con, fermi, ultimo_con, stato) = con_un_gradino(30, 4);
        assert_eq!(
            senza - con,
            30,
            "trenta fotogrammi di silenzio non consumano niente"
        );
        assert_eq!(
            fermi, 30,
            "esattamente trenta fotogrammi d'uscita senza musica"
        );
        assert_eq!(
            meta_di(ultimo_senza) - meta_di(ultimo_con),
            60,
            "la musica è indietro di trenta"
        );
        assert_eq!(stato.gradini_fatti.load(Ordering::Relaxed), 1);
        assert!(!stato.in_correzione.load(Ordering::Acquire));
    }

    #[test]
    fn un_gradino_oltre_l_anello_resta_in_sospeso() {
        // Quaranta fotogrammi nell'anello, cinquanta da saltare. La porta si
        // chiude consumandone undici, il salto prende i ventinove rimasti, e
        // gli altri ventuno aspettano che il decodificatore ne porti.
        let stato = condiviso(1);
        let (mut scrittore, lettore) = anello_contato(4_096, 40);
        let (_manda, mut voce) = voce(lettore, LENTA);
        stato.gradino.store(-50, Ordering::Release);
        let mut dati = [0.0f32; 64];
        riempi(&mut dati, &mut voce, &stato, &battito(MS), |v| v);
        assert_eq!(stato.fotogrammi.load(Ordering::Relaxed), 40);
        assert_eq!(voce.andamento.fase, Fase::Esegue { resto: -21 });
        assert!(stato.in_correzione.load(Ordering::Acquire));
        assert_eq!(stato.gradini_fatti.load(Ordering::Relaxed), 0);
        assert_eq!(
            stato.vuoti.load(Ordering::Relaxed),
            0,
            "a porta chiusa non è un buco"
        );

        // Arriva altra musica: il salto finisce, e da lì si suona.
        for n in 40..140 {
            let _ = scrittore.push(numero_in_campione(n + 1));
        }
        riempi(&mut dati, &mut voce, &stato, &battito(MS), |v| v);
        assert_eq!(stato.gradini_fatti.load(Ordering::Relaxed), 1);
        assert_eq!(stato.fotogrammi.load(Ordering::Relaxed), 40 + 21 + 64);
    }

    #[test]
    fn una_pausa_fa_cadere_il_gradino() {
        let stato = condiviso(1);
        let (_manda, mut voce) = voce(anello(4_096, 4_000, 0.5), LENTA);
        stato.gradino.store(25, Ordering::Release);
        let mut dati = [0.0f32; 4];
        riempi(&mut dati, &mut voce, &stato, &battito(MS), |v| v);
        assert!(matches!(voce.andamento.fase, Fase::Chiude { quanto: 25 }));
        stato.in_pausa.store(true, Ordering::Release);
        stato.gradino.store(-8, Ordering::Release);
        riempi(&mut dati, &mut voce, &stato, &battito(MS), |v| v);
        assert_eq!(voce.andamento.fase, Fase::Libera);
        assert_eq!(stato.gradino.load(Ordering::Acquire), 0);
        assert!(!stato.in_correzione.load(Ordering::Acquire));
    }

    // ── la deriva ───────────────────────────────────────────────────────────

    /// Cento blocchi da centouno fotogrammi mono, con la deriva data: le medie
    /// uscite, e i fotogrammi consumati meno quelli usciti.
    fn con_la_deriva(ppm: i32) -> (usize, i64) {
        let stato = condiviso(1);
        let (_scrittore, lettore) = anello_contato(16_384, 12_000);
        let (_manda, mut voce) = voce(lettore, LENTA);
        stato.deriva_ppm.store(ppm, Ordering::Relaxed);
        let mut medie = 0usize;
        for _ in 0..100 {
            let mut dati = [0.0f32; 101];
            riempi(&mut dati, &mut voce, &stato, &battito(MS), |v| v);
            medie += dati.iter().filter(|c| meta_di(**c) % 2 == 1).count();
        }
        let consumati = i64::try_from(stato.fotogrammi.load(Ordering::Relaxed)).unwrap_or(0);
        (medie, consumati - 10_100)
    }

    #[test]
    fn la_deriva_interviene_un_numero_esatto_di_volte() {
        // Diecimila fotogrammi che possono intervenire — il primo di ogni
        // blocco no — a cinquecento parti per milione: cinque interventi, né
        // quattro né sei.
        assert_eq!(con_la_deriva(500).0, 5);
        assert_eq!(con_la_deriva(-500).0, 5);
        assert_eq!(con_la_deriva(0).0, 0);
        // Oltre il tetto vale il tetto.
        assert_eq!(con_la_deriva(5_000).0, 5);
    }

    #[test]
    fn i_fotogrammi_tolti_si_contano_e_quelli_aggiunti_no() {
        // Avanti: ogni intervento consuma un fotogramma in più, e si conta.
        assert_eq!(con_la_deriva(500).1, 5);
        // Indietro: ogni intervento esce senza consumare niente.
        assert_eq!(con_la_deriva(-500).1, -5);
    }

    #[test]
    fn la_deriva_non_tocca_mai_il_primo_fotogramma_del_blocco() {
        // L'accumulo è già oltre il fotogramma intero: il primo fotogramma che
        // può intervenire interviene. Non è mai il primo del blocco — quello è
        // l'ancora — ma sempre il secondo.
        for ppm in [500, -500] {
            let stato = condiviso(1);
            let (_scrittore, lettore) = anello_contato(4_096, 4_000);
            let (_manda, mut voce) = voce(lettore, LENTA);
            stato.deriva_ppm.store(ppm, Ordering::Relaxed);
            for _ in 0..10 {
                voce.andamento.accumulo = i64::from(ppm.signum()) * 3 * MILIONE;
                let mut dati = [0.0f32; 4];
                riempi(&mut dati, &mut voce, &stato, &battito(MS), |v| v);
                assert_eq!(meta_di(dati[0]) % 2, 0, "{ppm} ppm: il primo è una media");
                assert_eq!(meta_di(dati[1]) % 2, 1, "{ppm} ppm: il secondo non lo è");
            }
        }
    }

    // ── il riposo e il ponte ────────────────────────────────────────────────

    #[test]
    fn con_la_sessione_accesa_il_flusso_non_riposa_mai() {
        let mut sonno = sveglio_dopo(RIPOSO_BLUETOOTH);
        let t0 = Instant::now();
        let insieme = Sguardo {
            in_pausa: true,
            svuota: false,
            a_riposo: true,
            sessione: true,
        };
        for secondi in 0..120 {
            assert_eq!(
                sonno.passo(insieme, t0 + Duration::from_secs(secondi)),
                Mossa::Niente
            );
        }
        assert!(!sonno.dorme);
        // E un flusso già a riposo la sessione lo sveglia.
        let mut sonno = Sonno::nuovo(RIPOSO_DOPO);
        assert_eq!(sonno.passo(insieme, t0), Mossa::Sveglia);
        // Spenta la sessione, il riposo torna quello di sempre.
        let solo = Sguardo {
            sessione: false,
            ..insieme
        };
        assert_eq!(sonno.passo(solo, t0), Mossa::Niente);
        assert_eq!(sonno.passo(solo, t0 + RIPOSO_A_VUOTO), Mossa::Riposa);
    }

    #[test]
    fn il_ponte_porta_un_istante_del_flusso_sull_orologio_del_processo() {
        // Il punto medio delle due letture locali.
        let ponte = Ponte::misurato(10_000, 5_000, 10_400);
        assert_eq!(ponte.locale_ns, 10_200);
        assert_eq!(ponte.locale_da_flusso(5_000), 10_200);
        assert_eq!(ponte.locale_da_flusso(7_500), 12_700);
        assert_eq!(ponte.locale_da_flusso(4_000), 9_200);
        // Prima dell'inizio dell'orologio locale si resta a zero.
        assert_eq!(ponte.locale_da_flusso(0), 5_200);
        assert_eq!(Ponte::misurato(0, 9_000, 0).locale_da_flusso(0), 0);
        assert_eq!(ponte.locale_da_flusso(u64::MAX), u64::MAX);
    }

    #[test]
    fn dove_cade_l_avvio_rispetto_al_blocco() {
        assert_eq!(dove_cade(1_000, 1_000, 48_000), Cade::Ritardo(0));
        assert_eq!(dove_cade(1_001, 1_000, 1_000), Cade::Fra(1));
        assert_eq!(dove_cade(1_000 + 3 * MS, 1_000, 1_000), Cade::Fra(3));
        assert_eq!(
            dove_cade(1_000, 1_000 + 3 * MS + 1, 1_000),
            Cade::Ritardo(3)
        );
    }

    // ── il percorso di prima, congelato ─────────────────────────────────────
    //
    // `riempi` e `svuota_anello` com'erano prima delle primitive di tempo di
    // «Ascolta insieme», copiati qui senza i commenti e mai più toccati. Sono la
    // misura del percorso inattivo: con tutte le caselle nuove a zero la callback
    // di adesso deve dare, campione per campione, quel che dà questa.

    #[expect(clippy::integer_division, reason = "come l'originale")]
    fn riempi_di_prima<T>(
        dati: &mut [T],
        voce: &mut Voce,
        condiviso: &Condiviso,
        converti: impl Fn(f32) -> T,
    ) {
        voce.veglia.giri.fetch_add(1, Ordering::Release);

        voce.filtro.ritira();

        let svuota = condiviso.svuota.load(Ordering::Acquire);
        let in_pausa = condiviso.in_pausa.load(Ordering::Acquire);
        let tace = svuota || in_pausa;

        let voluto = f32::from_bits(condiviso.guadagno.load(Ordering::Relaxed));
        let canali = condiviso.canali.load(Ordering::Relaxed).max(1);
        let per_fotogramma = usize::try_from(canali).unwrap_or(2);
        let filtra = !voce.filtro.stato.piatto();
        let guarda = condiviso.spettro.load(Ordering::Relaxed);
        let piega = voluto > 1.0 || filtra;
        let mut presi = 0u64;
        let mut mancati = 0u64;
        let mut apertura = voce.andamento.apertura();

        for (indice, posto) in dati.iter_mut().enumerate() {
            let canale = indice % per_fotogramma;
            if canale == 0 {
                voce.andamento.muovi_porta(!tace);
                apertura = voce.andamento.apertura();
            }
            if tace && voce.andamento.porta == 0 {
                *posto = converti(0.0);
                continue;
            }
            voce.andamento.guadagno = avvicina(voce.andamento.guadagno, voluto);
            match voce.lettore.pop() {
                Ok(campione) => {
                    presi += 1;
                    let campione = if filtra {
                        voce.filtro.stato.applica(campione, canale)
                    } else {
                        campione
                    };
                    if guarda {
                        voce.spia.campione(campione, per_fotogramma);
                    }
                    let alzato = campione * voce.andamento.guadagno * apertura;
                    *posto = converti(if piega { limita(alzato) } else { alzato });
                }
                Err(_) => {
                    if !tace {
                        mancati += 1;
                    }
                    *posto = converti(0.0);
                }
            }
        }

        let campioni = voce.andamento.resto.saturating_add(presi);
        let canali = u64::from(canali);
        condiviso
            .fotogrammi
            .fetch_add(campioni / canali, Ordering::Relaxed);
        voce.andamento.resto = campioni % canali;
        if mancati > 0 {
            condiviso.vuoti.fetch_add(mancati, Ordering::Relaxed);
        }

        let chiusa = voce.andamento.porta == 0;
        if tace && chiusa {
            voce.andamento.guadagno = 0.0;
            if svuota {
                svuota_anello_di_prima(voce, condiviso);
            }
        }
        voce.veglia.a_riposo.store(
            in_pausa && chiusa && voce.lettore.is_empty(),
            Ordering::Release,
        );
    }

    fn svuota_anello_di_prima(voce: &mut Voce, condiviso: &Condiviso) {
        while voce.lettore.pop().is_ok() {}
        voce.filtro.stato.azzera();
        voce.spia.azzera();
        voce.andamento.resto = 0;
        condiviso.svuota.store(false, Ordering::Release);
    }
}
