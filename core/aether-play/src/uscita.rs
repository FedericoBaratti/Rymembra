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
//! # Il flusso resta acceso anche in pausa
//!
//! In pausa la callback scrive silenzio invece di fermare il flusso. Fermarlo
//! davvero costa: su alcuni driver la ripartenza si sente come uno schiocco, e
//! su altri richiede decine di millisecondi. Silenzio a flusso acceso è
//! istantaneo, e ha il vantaggio che l'anello resta pieno — riprendere non deve
//! aspettare che il decodificatore lo riempia.
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
use std::sync::atomic::Ordering;
use std::time::Duration;

use aether_domain::errors::{AppError, ErrorCode};
use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};

use crate::Condiviso;
use crate::equalizzatore::{Coefficienti, Stato as StatoEq};

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
/// buffer del dispositivo, cioè qualche decina di millisecondi nel caso
/// peggiore. Un quarto di secondo lì dentro non è una catena lenta, è un
/// orologio del dispositivo che ha riportato una sciocchezza — e compensare una
/// sciocchezza sposta il cursore e i testi di un quarto di secondo senza che
/// nessuno possa capire perché.
///
/// Sopra questa soglia la misura si butta e resta solo la correzione a mano, che
/// è un numero che qualcuno ha scelto guardando l'effetto.
pub const LATENZA_MASSIMA_MS: u64 = 250;

/// Quanto in fretta il guadagno raggiunge il valore voluto, per campione.
///
/// Una rampa e non un salto: cambiare il volume di colpo su un'onda a metà
/// corsa produce un gradino, e un gradino è un clic. Con questo coefficiente a
/// 48 kHz la corsa completa dura una quarantina di millisecondi — impercettibile
/// come ritardo, sufficiente a non sentire il gradino.
const RAMPA: f32 = 0.0005;

/// Il dispositivo aperto.
///
/// Tenere questo valore vivo è ciò che tiene acceso il flusso: lasciarlo cadere
/// lo ferma. Il flusso di cpal non attraversa i fili su tutte le piattaforme, e
/// per questo vive su un filo suo che resta in attesa.
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
    /// Chiudendolo, il filo del flusso esce e il flusso si ferma.
    _spegni: std::sync::mpsc::Sender<()>,
}

/// Apre il dispositivo predefinito e comincia a chiedere campioni.
///
/// La frequenza e i canali **non si scelgono**: si accetta quel che il
/// dispositivo dichiara. In modalità condivisa su Windows chiederne altri
/// significa farsi rifiutare l'apertura, o farsi convertire il suono da un
/// livello su cui non abbiamo controllo. Meglio prendere la forma del
/// dispositivo e ricampionare noi, dove si può misurare cosa succede.
pub(crate) fn apri(
    anello: rtrb::Consumer<f32>,
    coefficienti: rtrb::Consumer<Coefficienti>,
    campioni_spettro: rtrb::Producer<f32>,
    condiviso: Arc<Condiviso>,
    voluto: Option<String>,
) -> Result<Uscita, AppError> {
    let (manda_formato, ricevi_formato) = std::sync::mpsc::channel();
    let (spegni, attendi_spegnimento) = std::sync::mpsc::channel::<()>();

    std::thread::Builder::new()
        .name("aether-uscita".to_owned())
        .spawn(move || {
            let costruito = costruisci(
                anello,
                coefficienti,
                campioni_spettro,
                &condiviso,
                voluto.as_deref(),
            );
            match costruito {
                Ok((flusso, aperta)) => {
                    if manda_formato.send(Ok(aperta)).is_err() {
                        return;
                    }
                    // Il flusso deve restare vivo qui: è la ragione per cui
                    // questo filo esiste invece di terminare subito.
                    let _flusso = flusso;
                    // Esce quando il `Sender` viene lasciato cadere.
                    while attendi_spegnimento.recv().is_ok() {}
                }
                Err(err) => {
                    let _ = manda_formato.send(Err(err));
                }
            }
        })
        .map_err(|err| {
            AppError::new(ErrorCode::PlaybackEngineUnavailable)
                .with_cause(format!("filo dell'uscita: {err}"))
        })?;

    let aperta = ricevi_formato.recv().map_err(|_| {
        AppError::new(ErrorCode::PlaybackEngineUnavailable)
            .with_cause("il filo dell'uscita è morto prima di aprire".to_owned())
    })??;

    Ok(Uscita {
        formato: aperta.formato,
        dispositivo: aperta.dispositivo,
        era_predefinito: aperta.era_predefinito,
        _spegni: spegni,
    })
}

/// Quel che il filo dell'uscita rimanda indietro appena ha aperto.
///
/// Tre campi e non uno perché il filo che apre è l'unico che vede il
/// dispositivo: dopo, il `Device` di cpal è già dentro il flusso, e il flusso
/// non attraversa i fili — nessuno può più chiedergli come si chiama.
struct Aperta {
    formato: FormatoUscita,
    dispositivo: String,
    era_predefinito: bool,
}

fn costruisci(
    anello: rtrb::Consumer<f32>,
    coefficienti: rtrb::Consumer<Coefficienti>,
    campioni_spettro: rtrb::Producer<f32>,
    condiviso: &Arc<Condiviso>,
    voluto: Option<&str>,
) -> Result<(cpal::Stream, Aperta), AppError> {
    let host = cpal::default_host();
    // L'elenco intero e non `default_output_device()`: da quando l'uscita si
    // può scegliere, il predefinito è soltanto il ripiego. La regola di quale
    // prendere sta tutta in `dispositivi::scegli`, che è pura e provata — qui
    // resta il lavoro che una funzione pura non può fare, cioè aprire.
    let uscite = crate::dispositivi::elenco();
    let scelta = crate::dispositivi::scegli(&uscite, voluto).ok_or_else(|| {
        AppError::new(ErrorCode::PlaybackEngineUnavailable)
            .with_cause("nessun dispositivo di uscita".to_owned())
    })?;
    let nome = scelta.id.clone();
    let era_predefinito = scelta.predefinito;

    // Dal nome al `Device`, che è l'unica strada che cpal offre: `elenco` ha
    // consumato il suo iteratore per leggere i nomi, e un `Device` non si
    // ricava da una stringa. Fra le due enumerazioni il mondo può essere
    // cambiato — una scheda staccata proprio adesso — e in quel caso si
    // ripiega sul predefinito invece di dire di no: è la stessa regola di
    // `scegli`, un istante più tardi.
    let dispositivo = host
        .output_devices()
        .ok()
        .and_then(|mut uscite| uscite.find(|d| d.name().is_ok_and(|suo| suo == nome)))
        .or_else(|| host.default_output_device())
        .ok_or_else(|| {
            AppError::new(ErrorCode::PlaybackEngineUnavailable)
                .with_cause(format!("l'uscita «{nome}» è sparita mentre la si apriva"))
        })?;
    // Il nome si rilegge dal dispositivo davvero preso: se il ramo di ripiego
    // qui sopra ha scelto il predefinito, `nome` mentirebbe — e chi sorveglia
    // confronta proprio questa stringa per decidere se riaprire, quindi una
    // bugia qui vale un anello di riaperture che non finisce.
    let nome = dispositivo.name().unwrap_or(nome);

    let configurazione = dispositivo.default_output_config().map_err(|err| {
        AppError::new(ErrorCode::PlaybackEngineUnavailable).with_cause(err.to_string())
    })?;

    let formato = FormatoUscita {
        frequenza: configurazione.sample_rate().0,
        canali: configurazione.channels(),
    };
    let campione = configurazione.sample_format();
    let configurazione: cpal::StreamConfig = configurazione.into();

    // Un `Consumer` non è clonabile e non deve esserlo: chi legge dai due anelli
    // è uno solo, ed è questa callback.
    let mut lettore = anello;
    let mut filtro = Filtro {
        stato: StatoEq::nuovo(),
        ricevi: coefficienti,
    };
    let mut spia = Spia {
        manda: campioni_spettro,
        somma: 0.0,
        quanti: 0,
    };
    let stato = Arc::clone(condiviso);
    let mut andamento = Andamento {
        guadagno: 0.0,
        resto: 0,
    };

    let su_errore = {
        let stato = Arc::clone(condiviso);
        move |err: cpal::StreamError| {
            // Il dispositivo è sparito: cuffie staccate, scheda cambiata. Non
            // c'è niente da fare qui dentro se non lasciarne traccia, perché
            // riaprire va fatto da un filo che può bloccarsi.
            //
            // La causa **prima** della bandiera, e nell'ordine di
            // `Contesto::caduto`, che scrive le stesse due caselle per l'altra
            // causa: chi legge arriva dall'altra parte — vista `perso` alta, va
            // a chiedere perché — e scrivendole al contrario ci sarebbe una
            // finestra in cui la risposta è ancora zero, cioè «causa
            // sconosciuta» nel diario e nel banner proprio nell'istante che
            // esistono per raccontare.
            stato
                .causa_perdita
                .store(codice_errore(&err), Ordering::Release);
            stato.perso.store(true, Ordering::Release);
        }
    };

    // La frequenza serve nelle tre callback per tradurre in fotogrammi la
    // latenza che `cpal` riporta in tempo. È un `u32` `Copy`, quindi ognuna delle
    // tre se ne prende la sua copia e nessuna la condivide con le altre.
    let frequenza = formato.frequenza;

    let flusso = match campione {
        cpal::SampleFormat::F32 => dispositivo.build_output_stream(
            &configurazione,
            move |dati: &mut [f32], info: &cpal::OutputCallbackInfo| {
                annota_latenza(info, frequenza, &stato);
                riempi(
                    dati,
                    &mut lettore,
                    &stato,
                    &mut andamento,
                    &mut filtro,
                    &mut spia,
                    a_f32,
                );
            },
            su_errore,
            None,
        ),
        cpal::SampleFormat::I16 => dispositivo.build_output_stream(
            &configurazione,
            move |dati: &mut [i16], info: &cpal::OutputCallbackInfo| {
                annota_latenza(info, frequenza, &stato);
                riempi(
                    dati,
                    &mut lettore,
                    &stato,
                    &mut andamento,
                    &mut filtro,
                    &mut spia,
                    a_i16,
                );
            },
            su_errore,
            None,
        ),
        cpal::SampleFormat::U16 => dispositivo.build_output_stream(
            &configurazione,
            move |dati: &mut [u16], info: &cpal::OutputCallbackInfo| {
                annota_latenza(info, frequenza, &stato);
                riempi(
                    dati,
                    &mut lettore,
                    &stato,
                    &mut andamento,
                    &mut filtro,
                    &mut spia,
                    a_u16,
                );
            },
            su_errore,
            None,
        ),
        altro => {
            return Err(AppError::new(ErrorCode::PlaybackEngineUnavailable)
                .with_cause(format!("formato di campione non gestito: {altro}")));
        }
    }
    .map_err(|err| {
        AppError::new(ErrorCode::PlaybackEngineUnavailable).with_cause(err.to_string())
    })?;

    flusso.play().map_err(|err| {
        AppError::new(ErrorCode::PlaybackEngineUnavailable).with_cause(err.to_string())
    })?;

    Ok((
        flusso,
        Aperta {
            formato,
            dispositivo: nome,
            era_predefinito,
        },
    ))
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
/// lo **costruisce**. Prende `callback` da `IAudioClock::GetPosition` e ci somma
/// la durata dello spazio libero nel buffer del dispositivo, che ricava da
/// `GetCurrentPadding`. Il commento di `cpal` su quella funzione lo dichiara
/// espressamente una stima, e aggiunge che dopo il buffer c'è «probabilmente un
/// altro po' di latenza» che non sa come determinare.
///
/// Quindi il numero che esce di qui è **un periodo di dispositivo** — una
/// decina di millisecondi a 48 kHz in modalità condivisa — e non la catena
/// d'uscita: non ci sono dentro il mixer di sistema, il driver, la conversione
/// digitale-analogica, né i millisecondi di un DAC USB. Su un'uscita Bluetooth è
/// gravemente sottostimato: là la latenza vera sta fra i cento e i duecento
/// millisecondi, e questa misura continua a dire dieci.
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
    // `None` quando `playback` precede `callback`, che non dovrebbe succedere e
    // su un orologio costruito a mano non è impossibile. In quel caso si lascia
    // l'ultimo valore noto invece di scrivere zero: zero direbbe «nessuna
    // latenza», che è un'affermazione, mentre qui non si sa niente di nuovo.
    let Some(anticipo) = tempi.playback.duration_since(&tempi.callback) else {
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
/// Fino a ieri qui c'era `|v| v`. Sulle due uscite a interi il taglio c'era
/// già dentro [`a_i16`]; su questa, che è il formato che WASAPI offre quasi
/// sempre, i campioni uscivano come venivano. Cosa ne faccia il sistema
/// operativo non è scritto da nessuna parte — di solito taglia, ma «di solito»
/// non è una garanzia su cui costruire il suono, e un driver che invece
/// avvolge produce uno schianto a piena ampiezza esattamente sui picchi.
fn a_f32(v: f32) -> f32 {
    v.clamp(-1.0, 1.0)
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
/// Due numeri che non stanno in [`Condiviso`] perché nessun altro filo li
/// guarda, e non sono variabili locali perché devono sopravvivere alla fine del
/// blocco. Stanno insieme in una struttura invece che sciolti per una ragione
/// prosaica: [`riempi`] ha già sette argomenti.
struct Andamento {
    /// Il guadagno raggiunto dalla rampa, da cui riparte il blocco dopo.
    guadagno: f32,
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

/// Il corpo della callback, uguale per ogni formato di campione.
///
/// Generico sulla conversione finale invece che scritto tre volte: le tre
/// versioni differirebbero per una moltiplicazione, e tre copie di una
/// procedura in cui non si può sbagliare sono tre posti in cui sbagliare.
#[expect(
    clippy::integer_division,
    reason = "un fotogramma è esattamente `canali` campioni; il resto sarebbe un \
              fotogramma servito a metà, che non esiste"
)]
fn riempi<T>(
    dati: &mut [T],
    lettore: &mut rtrb::Consumer<f32>,
    condiviso: &Condiviso,
    andamento: &mut Andamento,
    filtro: &mut Filtro,
    spia: &mut Spia,
    converti: impl Fn(f32) -> T,
) {
    // Le curve nuove prima di ogni altra cosa, anche prima dello svuotamento e
    // della pausa: adottarne una non tocca il suono che sta uscendo, e un
    // ritiro saltato è una curva che arriva un blocco dopo.
    filtro.ritira();

    // Poi lo svuotamento: dopo un salto, quel che c'è nell'anello appartiene al
    // punto di prima. Va scartato anche — anzi, soprattutto — se siamo in
    // pausa, altrimenti riprendendo si sentirebbe il punto vecchio.
    if condiviso.svuota.swap(false, Ordering::AcqRel) {
        while lettore.pop().is_ok() {}
        // La coda dei filtri è quel punto vecchio quanto i campioni
        // nell'anello: lasciarla suonare sopra il punto nuovo sarebbe la stessa
        // cosa che non svuotare.
        filtro.stato.azzera();
        spia.azzera();
        // Il fotogramma a metà appartiene al punto di prima, come i campioni
        // nell'anello: portarlo oltre il salto conterebbe un fotogramma vecchio
        // dentro il conteggio nuovo, che il decodificatore ha appena azzerato.
        andamento.resto = 0;
        for posto in dati.iter_mut() {
            *posto = converti(0.0);
        }
        return;
    }

    if condiviso.in_pausa.load(Ordering::Acquire) {
        for posto in dati.iter_mut() {
            *posto = converti(0.0);
        }
        return;
    }

    let voluto = f32::from_bits(condiviso.guadagno.load(Ordering::Relaxed));
    let canali = condiviso.canali.load(Ordering::Relaxed).max(1);
    // Per l'indice di canale serve un `usize`; il ripiego non si raggiunge, ma
    // qui dentro non esiste un modo accettabile di cadere.
    let per_fotogramma = usize::try_from(canali).unwrap_or(2);
    // Una volta per blocco e non una per campione: con i cursori a zero
    // l'equalizzatore non deve costare niente.
    let filtra = !filtro.stato.piatto();
    // Idem per lo spettro: chi non ha la schermata aperta non paga un `push`
    // per campione, che a 48 kHz sarebbero quarantottomila al secondo per
    // riempire un anello che nessuno svuota.
    let guarda = condiviso.spettro.load(Ordering::Relaxed);
    let mut presi = 0u64;
    let mut mancati = 0u64;

    for (indice, posto) in dati.iter_mut().enumerate() {
        andamento.guadagno += (voluto - andamento.guadagno) * RAMPA;
        match lettore.pop() {
            Ok(campione) => {
                presi += 1;
                // L'equalizzatore **prima** del volume: così il volume resta
                // l'ultima cosa che tocca il campione e la sua rampa continua a
                // fare il suo lavoro, e così la preamplificazione dei filtri non
                // dipende da quanto è alzato il volume.
                //
                // `indice % per_fotogramma` dà il canale: cpal consegna sempre
                // fotogrammi interi, che è la stessa cosa che il conto dei
                // fotogrammi qui sotto dà già per buona.
                let campione = if filtra {
                    filtro.stato.applica(campione, indice % per_fotogramma)
                } else {
                    campione
                };
                // Lo spettro prende il campione **qui**: dopo l'equalizzatore,
                // così una curva che alza i bassi si vede nelle barre, e prima
                // del volume, perché uno spettro che si abbassa quando si
                // abbassa la manopola descrive la manopola e non la musica.
                if guarda {
                    spia.campione(campione, per_fotogramma);
                }
                *posto = converti(campione * andamento.guadagno);
            }
            Err(_) => {
                // L'anello è vuoto: il decodificatore non ce l'ha fatta.
                // Silenzio, che è un buco; l'alternativa sarebbe ripetere
                // l'ultimo campione, che è un ronzio. Il buco almeno si sente
                // per quel che è.
                mancati += 1;
                *posto = converti(0.0);
            }
        }
    }

    // Il resto del blocco precedente entra nel conto di questo: i campioni di un
    // fotogramma servito a metà escono dalle casse a cavallo di due callback, e
    // il fotogramma che formano insieme va contato una volta — non zero.
    let campioni = andamento.resto.saturating_add(presi);
    let canali = u64::from(canali);
    condiviso
        .fotogrammi
        .fetch_add(campioni / canali, Ordering::Relaxed);
    andamento.resto = campioni % canali;
    if mancati > 0 {
        condiviso.vuoti.fetch_add(mancati, Ordering::Relaxed);
    }
}

/// Un numero che distingue le cause di perdita del dispositivo.
const fn codice_errore(err: &cpal::StreamError) -> u32 {
    match err {
        cpal::StreamError::DeviceNotAvailable => 1,
        cpal::StreamError::BackendSpecific { .. } => 2,
    }
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::equalizzatore::{BANDE, LIMITE_DB};
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};

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
        })
    }

    /// Una spia che non guarda.
    ///
    /// Le prove di questo file parlano della callback, non dello spettro: la
    /// presa c'è perché la firma la vuole, e il suo anello nessuno lo legge.
    fn spia() -> Spia {
        let (manda, _tenuto) = rtrb::RingBuffer::<f32>::new(8);
        Spia {
            manda,
            somma: 0.0,
            quanti: 0,
        }
    }

    /// Un filtro fermo, e la maniglia per mandargli delle curve.
    fn filtro() -> (rtrb::Producer<Coefficienti>, Filtro) {
        let (manda, ricevi) = rtrb::RingBuffer::<Coefficienti>::new(4);
        (
            manda,
            Filtro {
                stato: StatoEq::nuovo(),
                ricevi,
            },
        )
    }

    /// L'aritmetica della latenza, che è l'unica metà provabile.
    ///
    /// [`annota_latenza`] non ha una prova sua, e non è una dimenticanza:
    /// `cpal::OutputCallbackInfo` non si costruisce da fuori — non ha campi
    /// pubblici né costruttore, e gli istanti dentro li fa il backend audio.
    /// Provarla vorrebbe dire un dispositivo vero, cioè una prova che sulla CI
    /// non gira. Quel che si poteva separare è questa conversione; quel che resta
    /// nella funzione è una lettura di due campi `Copy` e uno `store`.
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

    #[test]
    fn in_pausa_esce_silenzio_e_l_anello_non_si_consuma() {
        let (mut scrittore, lettore) = rtrb::RingBuffer::<f32>::new(16);
        for _ in 0..8 {
            let _ = scrittore.push(0.9);
        }
        let stato = condiviso(2);
        stato.in_pausa.store(true, Ordering::Release);
        let mut lettore = lettore;
        let mut andamento = Andamento {
            guadagno: 1.0,
            resto: 0,
        };
        let (_manda, mut filtro) = filtro();
        let mut dati = [1.0f32; 4];
        riempi(
            &mut dati,
            &mut lettore,
            &stato,
            &mut andamento,
            &mut filtro,
            &mut spia(),
            |v| v,
        );
        assert_eq!(dati, [0.0; 4]);
        // Niente è stato consumato: riprendere non deve aspettare.
        assert_eq!(lettore.slots(), 8);
        assert_eq!(stato.fotogrammi.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn lo_svuotamento_scarta_tutto_e_si_disarma() {
        let (mut scrittore, mut lettore) = rtrb::RingBuffer::<f32>::new(16);
        for _ in 0..8 {
            let _ = scrittore.push(0.9);
        }
        let stato = condiviso(2);
        stato.svuota.store(true, Ordering::Release);
        let mut andamento = Andamento {
            guadagno: 1.0,
            resto: 0,
        };
        let (_manda, mut filtro) = filtro();
        let mut dati = [1.0f32; 4];
        riempi(
            &mut dati,
            &mut lettore,
            &stato,
            &mut andamento,
            &mut filtro,
            &mut spia(),
            |v| v,
        );
        assert_eq!(dati, [0.0; 4]);
        assert_eq!(lettore.slots(), 0, "l'anello doveva restare vuoto");
        assert!(!stato.svuota.load(Ordering::Acquire), "doveva disarmarsi");
    }

    #[test]
    fn un_anello_vuoto_da_silenzio_e_lo_conta() {
        let (_scrittore, mut lettore) = rtrb::RingBuffer::<f32>::new(16);
        let stato = condiviso(2);
        let mut andamento = Andamento {
            guadagno: 1.0,
            resto: 0,
        };
        let (_manda, mut filtro) = filtro();
        let mut dati = [1.0f32; 4];
        riempi(
            &mut dati,
            &mut lettore,
            &stato,
            &mut andamento,
            &mut filtro,
            &mut spia(),
            |v| v,
        );
        assert_eq!(dati, [0.0; 4]);
        assert_eq!(stato.vuoti.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn i_fotogrammi_si_contano_per_canale_non_per_campione() {
        let (mut scrittore, mut lettore) = rtrb::RingBuffer::<f32>::new(16);
        for _ in 0..8 {
            let _ = scrittore.push(0.5);
        }
        let stato = condiviso(2);
        let mut andamento = Andamento {
            guadagno: 1.0,
            resto: 0,
        };
        let (_manda, mut filtro) = filtro();
        let mut dati = [0.0f32; 8];
        riempi(
            &mut dati,
            &mut lettore,
            &stato,
            &mut andamento,
            &mut filtro,
            &mut spia(),
            |v| v,
        );
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
        let (mut scrittore, mut lettore) = rtrb::RingBuffer::<f32>::new(64);
        let stato = condiviso(2);
        let (_manda, mut filtro) = filtro();
        let mut andamento = Andamento {
            guadagno: 1.0,
            resto: 0,
        };

        // Tre campioni per un'uscita da quattro: un fotogramma e mezzo su due
        // canali. L'anello si secca esattamente in mezzo al secondo.
        for _ in 0..3 {
            let _ = scrittore.push(0.5);
        }
        let mut dati = [0.0f32; 4];
        riempi(
            &mut dati,
            &mut lettore,
            &stato,
            &mut andamento,
            &mut filtro,
            &mut spia(),
            |v| v,
        );
        assert_eq!(
            stato.fotogrammi.load(Ordering::Relaxed),
            1,
            "un fotogramma intero è uscito, il secondo è a metà"
        );

        // Il compagno del campione spaiato arriva adesso: insieme fanno il
        // secondo fotogramma, e va contato.
        let _ = scrittore.push(0.5);
        let mut ancora = [0.0f32; 2];
        riempi(
            &mut ancora,
            &mut lettore,
            &stato,
            &mut andamento,
            &mut filtro,
            &mut spia(),
            |v| v,
        );
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
        let (mut scrittore, mut lettore) = rtrb::RingBuffer::<f32>::new(64);
        let stato = condiviso(2);
        let (_manda, mut filtro) = filtro();
        let mut andamento = Andamento {
            guadagno: 1.0,
            resto: 0,
        };
        let _ = scrittore.push(0.5);
        let mut dati = [0.0f32; 2];
        riempi(
            &mut dati,
            &mut lettore,
            &stato,
            &mut andamento,
            &mut filtro,
            &mut spia(),
            |v| v,
        );
        assert_eq!(
            andamento.resto, 1,
            "un campione spaiato è rimasto in sospeso"
        );

        stato.svuota.store(true, Ordering::Release);
        let mut silenzio = [0.0f32; 2];
        riempi(
            &mut silenzio,
            &mut lettore,
            &stato,
            &mut andamento,
            &mut filtro,
            &mut spia(),
            |v| v,
        );
        assert_eq!(
            andamento.resto, 0,
            "il fotogramma a metà era del punto di prima"
        );
    }

    #[test]
    fn il_guadagno_sale_per_rampa_e_non_di_colpo() {
        let (mut scrittore, mut lettore) = rtrb::RingBuffer::<f32>::new(64);
        for _ in 0..64 {
            let _ = scrittore.push(1.0);
        }
        let stato = condiviso(2);
        let mut andamento = Andamento {
            guadagno: 0.0,
            resto: 0,
        };
        let (_manda, mut filtro) = filtro();
        let mut dati = [0.0f32; 64];
        riempi(
            &mut dati,
            &mut lettore,
            &stato,
            &mut andamento,
            &mut filtro,
            &mut spia(),
            |v| v,
        );
        // Il primo campione è quasi zero, non uno: nessun gradino.
        let primo = dati.first().copied().unwrap_or(1.0);
        assert!(primo < 0.01, "primo campione: {primo}");
        assert!(*dati.last().unwrap_or(&0.0) > primo);
    }

    #[test]
    fn il_taglio_evita_che_un_picco_avvolga_il_segno() {
        let (mut scrittore, mut lettore) = rtrb::RingBuffer::<f32>::new(8);
        for _ in 0..4 {
            let _ = scrittore.push(4.0);
        }
        let stato = condiviso(2);
        let mut andamento = Andamento {
            guadagno: 1.0,
            resto: 0,
        };
        let (_manda, mut filtro) = filtro();
        let mut dati = [0i16; 4];
        riempi(
            &mut dati,
            &mut lettore,
            &stato,
            &mut andamento,
            &mut filtro,
            &mut spia(),
            a_i16,
        );
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
    }

    #[test]
    fn un_filtro_piatto_non_cambia_l_uscita() {
        let (mut scrittore, mut lettore) = rtrb::RingBuffer::<f32>::new(8);
        for valore in [0.1f32, -0.2, 0.3, -0.4] {
            let _ = scrittore.push(valore);
        }
        let stato = condiviso(2);
        let mut andamento = Andamento {
            guadagno: 1.0,
            resto: 0,
        };
        let (_manda, mut filtro) = filtro();
        let mut dati = [0.0f32; 4];
        riempi(
            &mut dati,
            &mut lettore,
            &stato,
            &mut andamento,
            &mut filtro,
            &mut spia(),
            |v| v,
        );
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
        let (mut scrittore, mut lettore) = rtrb::RingBuffer::<f32>::new(64);
        for _ in 0..64 {
            let _ = scrittore.push(0.5);
        }
        let stato = condiviso(2);
        let mut andamento = Andamento {
            guadagno: 1.0,
            resto: 0,
        };
        let (mut manda, mut filtro) = filtro();
        // Tutto abbassato: il preamp resta a uno e i filtri tagliano.
        let _ = manda.push(Coefficienti::calcola(&[-LIMITE_DB; BANDE], true, 48_000));
        let mut dati = [0.0f32; 64];
        riempi(
            &mut dati,
            &mut lettore,
            &stato,
            &mut andamento,
            &mut filtro,
            &mut spia(),
            |v| v,
        );
        assert!(!filtro.stato.piatto(), "la curva doveva essere ritirata");
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
        let (mut scrittore, mut lettore) = rtrb::RingBuffer::<f32>::new(128);
        for _ in 0..64 {
            let _ = scrittore.push(0.9);
        }
        let stato = condiviso(2);
        let mut andamento = Andamento {
            guadagno: 1.0,
            resto: 0,
        };
        let (mut manda, mut filtro) = filtro();
        let _ = manda.push(Coefficienti::calcola(&[LIMITE_DB; BANDE], true, 48_000));
        let mut dati = [0.0f32; 64];
        riempi(
            &mut dati,
            &mut lettore,
            &stato,
            &mut andamento,
            &mut filtro,
            &mut spia(),
            |v| v,
        );

        stato.svuota.store(true, Ordering::Release);
        let mut silenzio = [0.0f32; 4];
        riempi(
            &mut silenzio,
            &mut lettore,
            &stato,
            &mut andamento,
            &mut filtro,
            &mut spia(),
            |v| v,
        );

        // Con la memoria azzerata, un silenzio in ingresso dà un silenzio in
        // uscita. Senza, i filtri avrebbero continuato a scaricare quel che si
        // ricordavano di prima del salto.
        for _ in 0..8 {
            let _ = scrittore.push(0.0);
        }
        let mut dopo = [1.0f32; 8];
        riempi(
            &mut dopo,
            &mut lettore,
            &stato,
            &mut andamento,
            &mut filtro,
            &mut spia(),
            |v| v,
        );
        for (n, valore) in dopo.iter().enumerate() {
            assert!(valore.abs() < 1e-6, "campione {n} vale {valore}, non zero");
        }
    }
}
