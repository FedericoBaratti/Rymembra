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
//! Vale anche in una sessione d'ascolto insieme, dove la ripresa è
//! programmata a un istante preciso: `Motore::riprendi_a` sveglia il flusso
//! prima di programmarla, e chi sceglie l'istante lo sveglia ancora prima.
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

use crate::equalizzatore::{Coefficienti, Stato as StatoEq};
use crate::{Condiviso, limita};

mod regia;

use regia::{Regia, ancora_del_blocco};

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

#[cfg(test)]
impl Campanello {
    /// Un campanello che non sveglia niente: quello del banco di prova del
    /// motore, che un dispositivo non ce l'ha.
    pub(crate) fn spento() -> Self {
        Self {
            ordini: std::sync::Weak::new(),
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
    ///   c'è nessuno a stringergliela. Passato il decimo di secondo il filo va
    ///   avanti, ma non spinge il punto nuovo nell'anello finché la bandiera
    ///   resta alta: la callback risvegliata dopo svuota **tutto** quel che
    ///   trova, senza contarlo, e un punto nuovo spinto prima sarebbe stato un
    ///   debito per sempre fra fotogrammi spinti e suonati — la fine della coda
    ///   non si dichiarava più (vedi `Contesto::forse_fine`). Lo svuotamento
    ///   aspettato così però arriva solo con la rete di sicurezza, cioè in
    ///   ritardo: per questo il filo suona anche lui il campanello, appena
    ///   prima di alzare la bandiera (vedi `Contesto::scarta_in_volo`), perché
    ///   fra questa chiamata e lo svuotamento può passare una ricerca lunga.
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
                };
                match sonno.passo(sguardo, Instant::now()) {
                    Mossa::Niente => {}
                    // Se il flusso non si ferma, è perché il suo filo è già
                    // morto — e allora la callback d'errore ha già detto
                    // perché. Non c'è niente da aggiungere.
                    Mossa::Riposa => {
                        let _ = flusso.pause();
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
    /// Una sessione d'ascolto insieme non cambia la regola. Nella 3.1 la
    /// cambiava: il flusso restava sveglio per tutta la sessione, perché
    /// `Motore::riprendi_a` non sapeva ripartire a un istante preciso da un
    /// flusso a riposo — e le cuffie Bluetooth, sotto un silenzio che non
    /// finiva mai, non tornavano più libere. Adesso `riprendi_a` il flusso lo
    /// sveglia da sé, prima di programmare.
    fn passo(&mut self, sguardo: Sguardo, adesso: Instant) -> Mossa {
        if self.dorme {
            if !sguardo.in_pausa || sguardo.svuota {
                let _ = self.sveglia();
                return Mossa::Sveglia;
            }
            return Mossa::Niente;
        }
        if !sguardo.in_pausa {
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
    // Il formato del mix lo sceglie chi ha impostato l'uscita in Windows, e
    // `cpal` non ci mette un tetto. Uno che non sappiamo servire si dice qui,
    // come guasto del dispositivo: lasciato passare, faceva fallire ogni brano
    // nel ricampionatore come «file danneggiato». Vedi
    // `decodifica::USCITA_MAX`.
    if !crate::decodifica::frequenza_d_uscita_accettabile(formato.frequenza) {
        return Err(
            AppError::new(ErrorCode::PlaybackEngineUnavailable).with_cause(format!(
                "uscita a {} Hz: il motore serve uscite fino a 768 kHz",
                formato.frequenza
            )),
        );
    }
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
/// Dalla 3.0 ci arrivano già piegati da [`limita`] quando la catena alza — il
/// guadagno, l'equalizzatore, o i campioni che il filo della decodifica ha
/// alzato, vedi `Condiviso::piega_fino` — e il taglio resta per quel che
/// sfugge: i campioni oltre il fondo scala che un decodificatore con perdita
/// produce da sé, a guadagno pieno.
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
    // I campioni che il filo della decodifica ha alzato — la correzione del
    // brano sopra l'unità, la somma di una dissolvenza — stanno nell'anello fino
    // a questo fotogramma. Vedi `Condiviso::piega_fino`.
    let alzati =
        condiviso.fotogrammi.load(Ordering::Relaxed) < condiviso.piega_fino.load(Ordering::Relaxed);
    let catena = Catena {
        filtra,
        // Idem per lo spettro: chi non ha la schermata aperta non paga un
        // `push` per campione, che a 48 kHz sarebbero quarantottomila al
        // secondo per riempire un anello che nessuno svuota.
        guarda: condiviso.spettro.load(Ordering::Relaxed),
        // Il limitatore morbido, solo quando qualcosa nella catena **alza**:
        // il guadagno qui sopra l'unità, l'equalizzatore, o dei campioni che
        // il filo della decodifica ha già alzato prima dell'anello — la
        // normalizzazione, che dalla 3.0 sta nei campioni e non più in
        // `voluto`, e la somma di una dissolvenza. Guardando solo `voluto`,
        // che è il volume e non supera mai uno, quei campioni arrivavano alla
        // conversione e ci venivano tagliati di netto. A guadagno pieno o più
        // basso un brano masterizzato fino a 0 dBFS che nessuno ha alzato deve
        // uscire com'è — il ginocchio di [`limita`] sta a −1 dBFS, e piegare i
        // suoi picchi vorrebbe dire colorarlo per niente. Deciso per blocco: la
        // rampa che attraversa l'unità dura qualche millisecondo, e lì la
        // differenza è di un paio di decimi di decibel sui soli picchi.
        piega: voluto > 1.0 || filtra || alzati,
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
mod prove;
