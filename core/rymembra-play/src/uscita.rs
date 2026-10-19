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
    /// Il campanello del filo che tiene il flusso.
    ///
    /// Non si clona e non esce di qui: lasciarlo cadere è il modo in cui il filo
    /// sa che deve chiudere il flusso e uscire, e un secondo mittente vivo da
    /// qualche altra parte lo terrebbe aperto per sempre.
    ordini: mpsc::Sender<Ordine>,
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
        let (risposta, attendi) = mpsc::sync_channel(1);
        if self.ordini.send(Ordine::Sveglia(risposta)).is_err() {
            // Il filo non c'è più: il flusso è già chiuso, e non c'è niente da
            // svegliare né nessuno da aspettare.
            return;
        }
        let _ = attendi.recv_timeout(ATTESA_RISVEGLIO.saturating_mul(2));
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

    std::thread::Builder::new()
        .name("rymembra-uscita".to_owned())
        .spawn(move || {
            let veglia = Arc::new(Veglia {
                giri: AtomicU64::new(0),
                a_riposo: AtomicBool::new(false),
            });
            let anelli = Anelli {
                campioni: anello,
                coefficienti,
                spettro: campioni_spettro,
            };
            match costruisci(anelli, &veglia, &condiviso, voluto.as_deref()) {
                Ok((flusso, aperta)) => {
                    let riposo_in_pausa = if aperta.bluetooth {
                        RIPOSO_BLUETOOTH
                    } else {
                        RIPOSO_DOPO
                    };
                    if manda_aperta.send(Ok(aperta)).is_err() {
                        return;
                    }
                    // Il flusso deve restare vivo qui: è la ragione per cui
                    // questo filo esiste invece di terminare subito. Esce — e
                    // il flusso con lui — quando l'`Uscita` viene lasciata
                    // cadere.
                    let sonno = Sonno::nuovo(riposo_in_pausa);
                    custodisci(&flusso, &ricevi_ordini, &condiviso, &veglia, sonno);
                }
                Err(err) => {
                    let _ = manda_aperta.send(Err(err));
                }
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
        ordini,
    })
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

    let configurazione = dispositivo.default_output_config().map_err(|err| {
        AppError::new(ErrorCode::PlaybackEngineUnavailable).with_cause(err.to_string())
    })?;

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
    .map_err(|err| {
        AppError::new(ErrorCode::PlaybackEngineUnavailable).with_cause(err.to_string())
    })?;

    // **Niente `play()` qui.** Il flusso resta fermo finché qualcuno non chiede
    // di suonare: vedi [`apri`] e [`Uscita::sveglia`].
    Ok((
        flusso,
        Aperta {
            formato,
            dispositivo: scelta.id,
            era_predefinito: scelta.predefinito,
            bluetooth,
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
            annota_latenza(info, frequenza, &stato);
            riempi(dati, &mut voce, &stato, converti);
        },
        su_errore,
        None,
    )
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
fn annota_latenza(info: &cpal::OutputCallbackInfo, frequenza: u32, condiviso: &Condiviso) {
    let tempi = info.timestamp();
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
}

impl Andamento {
    /// Muto e a porta chiusa: il primo suono salirà da zero.
    fn nuovo(frequenza: u32) -> Self {
        Self {
            guadagno: 0.0,
            porta: 0,
            corsa: corsa_della_porta(frequenza),
            resto: 0,
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
#[expect(
    clippy::integer_division,
    reason = "un fotogramma è esattamente `canali` campioni; il resto sarebbe un \
              fotogramma servito a metà, che non esiste"
)]
fn riempi<T>(dati: &mut [T], voce: &mut Voce, condiviso: &Condiviso, converti: impl Fn(f32) -> T) {
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
    // Idem per lo spettro: chi non ha la schermata aperta non paga un `push`
    // per campione, che a 48 kHz sarebbero quarantottomila al secondo per
    // riempire un anello che nessuno svuota.
    let guarda = condiviso.spettro.load(Ordering::Relaxed);
    // Il limitatore morbido, solo quando qualcosa nella catena **alza**: la
    // normalizzazione sopra l'unità o l'equalizzatore. A guadagno pieno o
    // più basso un brano masterizzato fino a 0 dBFS deve uscire com'è — il
    // ginocchio di [`limita`] sta a −1 dBFS, e piegare i picchi di un disco
    // che nessuno ha alzato vorrebbe dire colorarlo per niente. Deciso per
    // blocco, sul guadagno voluto: la rampa che attraversa l'unità dura
    // qualche millisecondo, e lì la differenza è di un paio di decimi di
    // decibel sui soli picchi.
    let piega = voluto > 1.0 || filtra;
    let mut presi = 0u64;
    let mut mancati = 0u64;
    let mut apertura = voce.andamento.apertura();

    for (indice, posto) in dati.iter_mut().enumerate() {
        // `indice % per_fotogramma` dà il canale: cpal consegna sempre
        // fotogrammi interi, che è la stessa cosa che il conto dei fotogrammi
        // qui sotto dà già per buona.
        let canale = indice % per_fotogramma;
        if canale == 0 {
            voce.andamento.muovi_porta(!tace);
            apertura = voce.andamento.apertura();
        }
        // Porta chiusa e silenzio chiesto: zeri, e l'anello non si tocca — in
        // pausa quel che c'è dentro è la ripresa, e deve restare dov'è.
        if tace && voce.andamento.porta == 0 {
            *posto = converti(0.0);
            continue;
        }
        voce.andamento.guadagno = avvicina(voce.andamento.guadagno, voluto);
        match voce.lettore.pop() {
            Ok(campione) => {
                presi += 1;
                // L'equalizzatore **prima** del volume: così il volume resta
                // l'ultima cosa che tocca il campione e la sua rampa continua a
                // fare il suo lavoro, e così la preamplificazione dei filtri non
                // dipende da quanto è alzato il volume.
                let campione = if filtra {
                    voce.filtro.stato.applica(campione, canale)
                } else {
                    campione
                };
                // Lo spettro prende il campione **qui**: dopo l'equalizzatore,
                // così una curva che alza i bassi si vede nelle barre, e prima
                // del volume, perché uno spettro che si abbassa quando si
                // abbassa la manopola descrive la manopola e non la musica.
                if guarda {
                    voce.spia.campione(campione, per_fotogramma);
                }
                let alzato = campione * voce.andamento.guadagno * apertura;
                *posto = converti(if piega { limita(alzato) } else { alzato });
            }
            Err(_) => {
                // L'anello è vuoto. Suonando vuol dire che il decodificatore
                // non ce l'ha fatta: silenzio, che è un buco — l'alternativa
                // sarebbe ripetere l'ultimo campione, che è un ronzio — e si
                // conta. Sfumando verso il silenzio chiesto no: la coda finita
                // arriva proprio così, e non è un disco lento.
                if !tace {
                    mancati += 1;
                }
                *posto = converti(0.0);
            }
        }
    }

    // Il resto del blocco precedente entra nel conto di questo: i campioni di un
    // fotogramma servito a metà escono dalle casse a cavallo di due callback, e
    // il fotogramma che formano insieme va contato una volta — non zero.
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
    use std::sync::atomic::AtomicU32;

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

    fn condiviso(canali: u16) -> Arc<Condiviso> {
        Arc::new(Condiviso {
            in_pausa: AtomicBool::new(false),
            svuota: AtomicBool::new(false),
            guadagno: AtomicU32::new(1.0f32.to_bits()),
            fotogrammi: AtomicU64::new(0),
            vuoti: AtomicU64::new(0),
            canali: AtomicU32::new(u32::from(canali)),
            latenza_fotogrammi: AtomicU64::new(0),
            perso: AtomicBool::new(false),
            causa_perdita: AtomicU32::new(0),
            abbandonato: AtomicBool::new(false),
            spettro: AtomicBool::new(false),
            in_lettura: std::sync::Mutex::new(None),
        })
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
                veglia: Arc::new(Veglia {
                    giri: AtomicU64::new(0),
                    a_riposo: AtomicBool::new(false),
                }),
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
        riempi(&mut dati, &mut voce, &stato, |v| v);
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
        riempi(&mut dati, &mut voce, &stato, |v| v);

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
        riempi(&mut dati, &mut voce, &stato, |v| v);
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
        riempi(&mut dati, &mut voce, &stato, |v| v);
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
        riempi(&mut dati, &mut voce, &stato, |v| v);
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
        riempi(&mut primo, &mut voce, &stato, |v| v);
        assert!(primo.iter().all(|c| *c > 0.0), "{primo:?}");
        assert!(stato.svuota.load(Ordering::Acquire), "troppo presto");

        let mut secondo = [9.0f32; 10];
        riempi(&mut secondo, &mut voce, &stato, |v| v);
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
                    riempi(&mut dati, &mut voce, &stato, |v| v);
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
        riempi(&mut dati, &mut voce, &stato, |v| v);
        assert_eq!(dati, [0.0; 4]);
        assert_eq!(stato.vuoti.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn i_fotogrammi_si_contano_per_canale_non_per_campione() {
        let stato = condiviso(2);
        let (_manda, mut voce) = voce(anello(16, 8, 0.5), SECCA);
        let mut dati = [0.0f32; 8];
        riempi(&mut dati, &mut voce, &stato, |v| v);
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
        riempi(&mut dati, &mut voce, &stato, |v| v);
        assert_eq!(
            stato.fotogrammi.load(Ordering::Relaxed),
            1,
            "un fotogramma intero è uscito, il secondo è a metà"
        );

        // Il compagno del campione spaiato arriva adesso: insieme fanno il
        // secondo fotogramma, e va contato.
        let _ = scrittore.push(0.5);
        let mut ancora = [0.0f32; 2];
        riempi(&mut ancora, &mut voce, &stato, |v| v);
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
        riempi(&mut dati, &mut voce, &stato, |v| v);
        assert_eq!(
            voce.andamento.resto, 1,
            "un campione spaiato è rimasto in sospeso"
        );

        stato.svuota.store(true, Ordering::Release);
        let mut silenzio = [0.0f32; 2];
        riempi(&mut silenzio, &mut voce, &stato, |v| v);
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
        riempi(&mut dati, &mut voce, &stato, |v| v);
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
        riempi(&mut dati, &mut voce, &stato, a_i16);
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
        riempi(&mut dati, &mut voce, &stato, |v| v);
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
        riempi(&mut dati, &mut voce, &stato, |v| v);
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
        riempi(&mut dati, &mut voce, &stato, |v| v);

        stato.svuota.store(true, Ordering::Release);
        let mut silenzio = [0.0f32; 4];
        riempi(&mut silenzio, &mut voce, &stato, |v| v);
        assert!(!stato.svuota.load(Ordering::Acquire), "doveva disarmarsi");

        // Con la memoria azzerata, un silenzio in ingresso dà un silenzio in
        // uscita. Senza, i filtri avrebbero continuato a scaricare quel che si
        // ricordavano di prima del salto.
        for _ in 0..8 {
            let _ = scrittore.push(0.0);
        }
        let mut dopo = [1.0f32; 8];
        riempi(&mut dopo, &mut voce, &stato, |v| v);
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
            riempi(&mut dati, &mut voce, &stato, |v| v);
            assert_eq!(voce.veglia.giri.load(Ordering::Acquire), atteso);
        }
    }

    #[test]
    fn a_riposo_solo_in_pausa_a_porta_chiusa_e_con_l_anello_vuoto() {
        let stato = condiviso(2);
        let mut dati = [0.0f32; 4];

        // Suonando: mai a riposo, nemmeno con l'anello secco.
        let (_manda, mut vuota) = voce(anello(16, 0, 0.0), SECCA);
        riempi(&mut dati, &mut vuota, &stato, |v| v);
        assert!(!vuota.veglia.a_riposo.load(Ordering::Acquire));

        stato.in_pausa.store(true, Ordering::Release);
        // In pausa con l'anello vuoto — la coda finita, il lettore fermato.
        riempi(&mut dati, &mut vuota, &stato, |v| v);
        assert!(vuota.veglia.a_riposo.load(Ordering::Acquire));

        // In pausa con l'anello pieno — la pausa di chi riprenderà: no.
        let (_manda, mut piena) = voce(anello(16, 16, 0.5), SECCA);
        piena.andamento.porta = 0;
        riempi(&mut dati, &mut piena, &stato, |v| v);
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

    // ── il riposo del flusso ──

    fn sguardo(in_pausa: bool, a_riposo: bool) -> Sguardo {
        Sguardo {
            in_pausa,
            svuota: false,
            a_riposo,
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
        };
        assert_eq!(sonno.passo(aspetta, t0), Mossa::Sveglia);
    }
}
