//! Il filo che decodifica, e quello che tiene il tempo.
//!
//! # Dove sta la posizione
//!
//! Non dove verrebbe da cercarla. Il decodificatore va **avanti** rispetto a
//! quel che si sente: riempie un anello che vale duecento millisecondi, quindi
//! quando ha appena decodificato il secondo 30 di una canzone, dalle casse esce
//! il 29,8. Dire all'utente «30» sarebbe mentire, e si vedrebbe: il cursore
//! arriverebbe in fondo prima della fine del brano.
//!
//! Fra il decodificatore e l'orecchio però non c'è un solo ritardo: ce ne sono
//! **tre**, e distinguerli è tutto il mestiere di questo modulo.
//!
//! 1. **L'anello nostro**, fra il filo della decodifica e la callback. Duecento
//!    millisecondi garantiti, qualche secondo su un'uscita normale — vedi
//!    [`RISERVA_MS`]. Questo lo sappiamo esattamente, perché i fotogrammi che ne
//!    escono li conta la callback: è il contatore
//!    `Condiviso::fotogrammi`.
//! 2. **Il buffer del dispositivo**, fra la callback e il convertitore. La
//!    callback consegna i campioni *al driver*, non alle casse, e quel che
//!    conta — i `suonati` — sono fotogrammi **entrati nel buffer del
//!    dispositivo**. Fino a ieri questo file scriveva che erano «i fotogrammi
//!    che la callback ha davvero consegnato al dispositivo», e la frase era
//!    letteralmente giusta e praticamente falsa: consegnati al dispositivo non
//!    vuol dire usciti dalle casse. Questo pezzo si **misura**:
//!    `crate::uscita::annota_latenza` lo legge da `cpal` a ogni blocco e lo
//!    scrive in `Condiviso::latenza_fotogrammi`.
//! 3. **La catena d'uscita** — mixer di sistema, driver, DAC, e su un'uscita
//!    senza fili la radio. Questo non si misura da nessuna parte: `cpal` non lo
//!    vede, e su Bluetooth vale più degli altri due insieme. Si **dichiara a
//!    mano**, con la preferenza `audio.latenza_ms`.
//!
//! Da qui i due nomi che [`Contesto::aggiorna`] usa e non confonde: i
//! **`suonati`** sono i fotogrammi contati dalla callback (1), gli **`uditi`**
//! sono i `suonati` meno la somma di (2) e (3). Gli `uditi` governano quel che
//! si racconta — quale brano sta suonando e a che punto è — perché è quel che
//! l'orecchio sta ricevendo adesso. I `suonati` grezzi governano una cosa sola,
//! la fine del brano, per la ragione scritta in [`Contesto::forse_fine`].
//!
//! La traduzione da fotogrammi a brano e istante passa dai **segni**: ogni volta
//! che i campioni di un brano nuovo cominciano a entrare nell'anello si annota a
//! quale fotogramma d'uscita cominceranno, e da quale punto del brano. Quando il
//! contatore raggiunge quel numero, quel brano sta suonando — non un istante
//! prima.
//!
//! È lo stesso meccanismo che rende corretto il gapless: fra due brani attaccati
//! non c'è nessun evento, nessuna riapertura, nessuna pausa. C'è un segno.

use std::collections::VecDeque;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use rymembra_domain::errors::AppError;

#[cfg(doc)]
use crate::ancora::Ancora;
use crate::ancora::{Mappa, SegnoPubblico};
use crate::decodifica::{Decodificatore, FormatoSuonato, Sorgente};
use crate::equalizzatore::{BANDE, Coefficienti};
use crate::uscita::FormatoUscita;
#[cfg(doc)]
use crate::uscita::RISERVA_MS;
use crate::{Condiviso, Livello};

mod anello;
mod comandi;
mod maniglia;
mod racconto;

pub use maniglia::{Motore, PresaSpettro};

/// In quanti passi si passa da una curva dell'equalizzatore all'altra.
///
/// Cambiare di colpo i coefficienti di dieci sezioni — cosa che succede
/// caricando un preset — produce un transitorio che si sente come uno schiocco.
/// Dieci passi da quattro millisecondi l'uno, che è il sonno di questo filo,
/// fanno una quarantina di millisecondi: impercettibili come ritardo,
/// sufficienti a non sentire il gradino. È lo stesso ragionamento della `RAMPA`
/// del volume in [`crate::uscita`], applicato dove la rampa non poteva arrivare.
const PASSI_EQ: u8 = 10;

/// In quanto tempo un brano entrante si riprende tutta l'ampiezza, quando
/// quello che usciva è finito prima della fine della curva.
///
/// Quaranta millisecondi, gli stessi della rampa del volume in
/// [`crate::uscita`]: non si sentono come un cambio di livello, e non sono un
/// gradino. Il perché di una ripresa breve invece del resto della dissolvenza
/// sta in [`risali`].
const RIPRESA_MS: u64 = 40;

/// Quanto può durare, al più, una chiamata a [`Contesto::riempi`] prima di
/// tornare al filo: vedi [`Contesto::riempi_anello`].
///
/// Dieci millisecondi: un blocco di un file locale si decodifica in molto
/// meno, quindi su un disco sano riempire l'anello costa al più qualche giro in
/// più, senza sonno in mezzo; e un comando arrivato intanto aspetta lo stesso
/// ordine di grandezza dei quattro millisecondi del sonno del filo.
const BILANCIO_RIEMPI: Duration = Duration::from_millis(10);

/// Il codice di [`Condiviso::causa_perdita`] per il filo di decodifica caduto.
///
/// Uno e due li assegna la callback dell'uscita ai due `StreamError` di cpal
/// (vedi `crate::uscita::codice_errore`); il tre è di qua, e vale «il motore c'è
/// ancora, il dispositivo pure, ma non arriva più un campione». Un numero e non
/// un `enum` perché deve stare in un `AtomicU32` che la callback audio può
/// leggere senza lucchetti.
const CAUSA_FILO_CADUTO: u32 = 3;

/// Quante volte, in questa sessione, il filo della decodifica è caduto.
///
/// # Perché un contatore globale e non un campo del motore
///
/// Perché chi se ne serve arriva **dopo** che quel motore è stato buttato via:
/// il sorvegliante ricostruisce il motore e poi rimette la puntina dov'era, e
/// deve poter distinguere «il dispositivo era sparito» — dove rimettere la
/// puntina è la cosa giusta — da «quel file ha fatto cadere il decodificatore»,
/// dove rimetterla vuol dire un motore ricostruito ogni due secondi. Fra i due
/// istanti il motore in cui il conteggio sarebbe vissuto non esiste più.
///
/// Cresce e basta; nessuno lo azzera. Chi lo legge confronta il valore con
/// quello che aveva la volta scorsa, non con lo zero.
static CADUTI: AtomicU32 = AtomicU32::new(0);

/// Quante volte il filo della decodifica è caduto da quando l'applicazione è
/// aperta.
///
/// Serve a chi ricostruisce il motore per non rimettere la puntina nello stesso
/// solco: se il numero è cresciuto da quando quel brano era stato aperto
/// l'ultima volta, è quel brano ad aver fatto cadere il filo. Vedi la casella
/// `CADUTI` qui sopra.
#[must_use]
pub fn fili_caduti() -> u32 {
    CADUTI.load(Ordering::Acquire)
}

/// Cosa è successo, per chi sta sopra.
#[derive(Debug, Clone)]
pub enum Evento {
    /// Questo brano ha cominciato a uscire dalle casse **adesso**.
    ///
    /// Con il gapless è anche il segnale che il precedente è finito: non arriva
    /// un «finito» separato perché fra i due non c'è un istante in cui non
    /// suoni niente, e inventarne uno vorrebbe dire mentire su cosa è successo.
    Iniziato {
        /// Quale brano.
        track_id: i64,
    },
    /// Non c'è più niente da suonare: l'ultimo brano è uscito per intero.
    Fermato,
    /// Qualcosa è andato storto.
    Errore(Box<AppError>),
}

/// Dove siamo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Posizione {
    /// Quale brano sta suonando.
    pub track_id: Option<i64>,
    /// A che punto è, in millisecondi.
    pub ms: u64,
    /// Quanto dura, secondo il database.
    pub durata_ms: u64,
    /// È in pausa.
    ///
    /// Unico campo che non viene dall'istantanea: lo riempie
    /// [`Motore::posizione`] leggendo la pausa **voluta** nell'istante in cui
    /// qualcuno chiede dove siamo. Il perché sta in [`Motore::pausa`], e perché
    /// voluta e non in corso sta in `Intese::pausa_voluta`.
    pub in_pausa: bool,
    /// Com'è fatto quel che si sente, secondo il decodificatore.
    ///
    /// Viaggia col segno del brano, come `track_id`: in un passaggio senza
    /// buco cambia al fotogramma in cui il brano nuovo comincia a sentirsi, e
    /// non quando la decodifica lo apre — qualche secondo prima, quanto
    /// l'anello tiene da parte.
    pub formato: Option<FormatoSuonato>,
}

/// I contatori di quel che è andato storto o è stato corretto, dall'apertura.
///
/// Crescono e basta: chi li legge confronta il valore con quello della volta
/// prima, come per [`fili_caduti`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Contatori {
    /// I campioni serviti a vuoto perché l'anello era secco: vedi
    /// [`Motore::vuoti`].
    pub vuoti: u64,
    /// Le volte che il dispositivo è rimasto senza campioni, secondo `cpal`.
    pub xrun: u64,
    /// I gradini eseguiti dalla callback: vedi [`Motore::gradino`].
    pub gradini_fatti: u64,
}

/// Perché [`Motore::riprendi_a`] ha rifiutato l'avvio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RifiutoAvvio {
    /// L'istante chiesto è passato da più di un secondo: non è più un avvio
    /// programmato, è un salto, e va chiesto come tale.
    NelPassato,
}

/// Un [`Evento`], con quel che serve a chi lo riceve per decidere se vale ancora.
///
/// # Perché l'evento non basta più
///
/// Perché da quando gli eventi viaggiano su un filo loro — «rymembra-eventi», vedi
/// [`Motore::avvia_con_annunci`] — fra il momento in cui il motore dice una cosa
/// e quello in cui chi sta sopra la legge passa del tempo. Poco, di solito: il
/// tempo di attraversare un canale. Ma chi sta sopra, per leggerla, prende il
/// suo lucchetto, e in quel lucchetto possono essere successe due cose che
/// l'evento non sa.
///
/// **Il motore è stato sostituito.** È il caso di `Condiviso::abbandonato`, e
/// il controllo che [`Motore::drop`] rende possibile alzando la bandiera andava
/// fatto **dall'altra parte**: l'osservatore passava il controllo, poi si
/// metteva in coda sul lucchetto del lettore tenuto da chi stava sostituendo
/// il motore, e quando lo otteneva parlava al motore nuovo di un brano del
/// vecchio. [`Annuncio::ha_voce`] si chiede **sotto** quel lucchetto, dove la
/// risposta non può più cambiare.
///
/// **Qualcuno ha già voltato pagina.** Un `Iniziato` del brano B annunciato un
/// istante prima che la finestra chiedesse un altro brano arriva dopo la
/// richiesta: letto com'è, farebbe avanzare la coda oltre il brano appena
/// chiesto. [`Annuncio::superato`] lo riconosce.
///
/// E porta la **posizione di quell'istante**, che prima chi riceveva andava a
/// leggersi dal motore mentre il motore aspettava: ora il motore non aspetta
/// più, e fra l'annuncio e la lettura la posizione può essere già stata
/// azzerata. Il caso che se ne accorgerebbe è il guasto di rete, dove «Riprova»
/// deve sapere dove si era **prima** che il motore si fermasse.
pub struct Annuncio {
    /// Cosa è successo.
    pub evento: Evento,
    /// Dove si era nell'istante dell'annuncio.
    ///
    /// Per un [`Evento::Iniziato`] è già quella del brano che comincia; per un
    /// [`Evento::Fermato`] è quella vuota; per un [`Evento::Errore`] è l'ultima
    /// nota prima del guasto.
    pub posizione: Posizione,
    condiviso: Arc<Condiviso>,
    intese: Arc<Intese>,
    /// Quante svolte il filo aveva eseguito quando l'annuncio è partito. Vedi
    /// [`Intese::chieste`].
    svolte: u64,
}

impl Annuncio {
    /// Il motore che l'ha detto è ancora quello in uso.
    ///
    /// Da chiedere **dentro** il lucchetto sotto cui il motore si sostituisce:
    /// fuori, la risposta può cambiare fra la domanda e l'uso. Vedi il tipo.
    #[must_use]
    pub fn ha_voce(&self) -> bool {
        !self.condiviso.abbandonato.load(Ordering::Acquire)
    }

    /// Dopo che l'annuncio è partito qualcuno ha già chiesto al motore di
    /// cambiare strada — un brano nuovo, o lo stop — e il filo non l'aveva
    /// ancora fatto.
    ///
    /// Un annuncio così parla di quel che si sentiva **prima** della richiesta:
    /// chi l'ha fatta ha già chiuso l'ascolto e mosso la coda, e rileggerlo
    /// vorrebbe dire muoverla una seconda volta.
    #[must_use]
    pub fn superato(&self) -> bool {
        self.intese.chieste.load(Ordering::SeqCst) > self.svolte
    }
}

impl std::fmt::Debug for Annuncio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Annuncio")
            .field("evento", &self.evento)
            .field("posizione", &self.posizione)
            .field("svolte", &self.svolte)
            .finish_non_exhaustive()
    }
}

/// Quel che la maniglia e il filo della decodifica si dicono **fuori** dalla
/// coda dei comandi.
///
/// Tre caselle, tutte scritte da chi preme e lette da chi decodifica o da chi
/// riceve gli annunci. Non stanno in `Condiviso` perché la callback audio non
/// le guarda: sono accordi fra due fili che possono aspettarsi, non fra uno che
/// può e uno che non può.
struct Intese {
    /// La pausa che chi comanda **vuole**, che non è sempre quella in corso.
    ///
    /// `Condiviso::in_pausa` è quella in corso, e la legge la callback. Le due
    /// divergono in un caso solo, ed è il difetto che questa casella chiude: si
    /// mette in pausa, si sceglie un altro brano, e prima di questa riga
    /// [`Motore::suona`] abbassava subito la pausa in corso — mentre il comando
    /// che butta via l'anello era ancora in coda. Per qualche millisecondo la
    /// callback suonava **quel che era rimasto nell'anello del brano di
    /// prima**: un colpo del vecchio brano prima del nuovo.
    ///
    /// Adesso `suona` scrive qui che non si vuole più la pausa, e la pausa in
    /// corso si abbassa sul filo della decodifica **dopo** che l'anello è stato
    /// svuotato. Chi chiede dove siamo legge questa casella, non l'altra: il
    /// pulsante mostra le due stanghette nell'istante in cui si preme, che è
    /// la ragione per cui la pausa si scrive da qui e non dalla coda — vedi
    /// [`Motore::pausa`].
    pausa_voluta: AtomicBool,
    /// Quante svolte — brani avviati e stop — la maniglia ha chiesto.
    ///
    /// Cresce **prima** che il comando parta, sul filo di chi preme; il suo
    /// gemello `eseguite` cresce sul filo della decodifica quando la svolta è
    /// stata fatta. Finché i due differiscono c'è una svolta in viaggio, e un
    /// annuncio partito in quel tempo parla di quel che c'era prima: vedi
    /// [`Annuncio::superato`].
    chieste: AtomicU64,
    /// Quante svolte il filo della decodifica ha eseguito.
    eseguite: AtomicU64,
}

impl Intese {
    fn nuove() -> Arc<Self> {
        Arc::new(Self {
            // Come `Condiviso::in_pausa`: si comincia fermi.
            pausa_voluta: AtomicBool::new(true),
            chieste: AtomicU64::new(0),
            eseguite: AtomicU64::new(0),
        })
    }

    /// C'è una svolta chiesta e non ancora fatta.
    fn in_viaggio(&self) -> bool {
        self.chieste.load(Ordering::SeqCst) > self.eseguite.load(Ordering::SeqCst)
    }

    /// In pausa, subito: voluta e in corso insieme, in quest'ordine.
    fn pausa(&self, condiviso: &Condiviso) {
        self.pausa_voluta.store(true, Ordering::SeqCst);
        condiviso.in_pausa.store(true, Ordering::SeqCst);
    }

    /// Fuori pausa — ma la pausa in corso si abbassa solo se non c'è una
    /// svolta in viaggio. Se c'è, la abbasserà il filo della decodifica dopo
    /// aver svuotato l'anello: vedi [`Intese::allinea`].
    fn riprendi(&self, condiviso: &Condiviso) {
        self.pausa_voluta.store(false, Ordering::SeqCst);
        if !self.in_viaggio() {
            self.allinea(condiviso);
        }
    }

    /// Porta la pausa in corso dove la vuole chi comanda.
    ///
    /// # Perché la seconda lettura
    ///
    /// Perché fra la lettura e la scrittura qualcuno può mettere in pausa: la
    /// sua scrittura di `in_pausa` arriverebbe **prima** della nostra, e la
    /// nostra la cancellerebbe — musica che riparte dopo che si è premuto
    /// pausa. Rileggendo la volontà dopo aver scritto, chi ha messo in pausa in
    /// mezzo viene visto: [`Intese::pausa`] scrive la volontà prima della pausa
    /// in corso, e con l'ordine `SeqCst` una delle due letture la trova.
    fn allinea(&self, condiviso: &Condiviso) {
        if self.pausa_voluta.load(Ordering::SeqCst) {
            condiviso.in_pausa.store(true, Ordering::SeqCst);
            return;
        }
        condiviso.in_pausa.store(false, Ordering::SeqCst);
        if self.pausa_voluta.load(Ordering::SeqCst) {
            condiviso.in_pausa.store(true, Ordering::SeqCst);
        }
    }
}

/// La dissolvenza verso la pausa del timer di spegnimento, in corso.
#[derive(Clone, Copy)]
struct Sfumatura {
    inizio: Instant,
    durata: Duration,
    /// Arrivata in fondo: la pausa è già stata messa, e il guadagno **resta**
    /// al fondo finché qualcuno non riprende. Vedi [`Contesto::avanza_sfumatura`].
    finita: bool,
}

/// Quanto scende il volume alla fine di una [`Sfumatura`], in decibel.
///
/// Sessanta: sotto non si sente più niente sopra il rumore di una stanza, e la
/// pausa che arriva lì non è un taglio. La discesa è lineare **in decibel**,
/// che all'orecchio è una discesa uniforme; lineare in ampiezza sembrerebbe
/// ferma per metà del tempo e poi cadrebbe tutta alla fine.
const SFUMATURA_DB: f32 = 60.0;

/// Quel che si può chiedere al motore.
enum Comando {
    /// Un brano nuovo, dal millisecondo `da_ms`.
    ///
    /// L'apertura e il salto viaggiano **insieme** perché il filo li faccia
    /// uno dopo l'altro senza decodificare niente in mezzo: con due comandi,
    /// fra l'uno e l'altro il filo riempirebbe l'anello dal punto zero, e quei
    /// campioni si sentirebbero o andrebbero buttati con uno svuotamento in
    /// più. Vedi [`Motore::apri_fermo`].
    ///
    /// Se il brano debba partire fermo non lo dice il comando: la pausa la
    /// scrive la maniglia **prima** di mandarlo, come ogni altra pausa (vedi
    /// [`Motore::pausa`]), e il filo la allinea dopo lo svuotamento come per
    /// ogni brano nuovo. Un campo `in_pausa` qui non avrebbe nessuno a leggerlo.
    Suona {
        brano: Box<BranoAperto>,
        da_ms: u64,
    },
    Prepara(Option<Box<BranoAperto>>),
    Ferma,
    VaiA(u64),
    /// Porta il volume a zero in tanti millisecondi, poi mette in pausa.
    Sfuma {
        ms: u64,
    },
    /// Lascia perdere la sfumatura in corso, se ce n'è una.
    AnnullaSfuma,
    Volume {
        volume: f32,
        muto: bool,
    },
    ReplayGain {
        attivo: bool,
        bersaglio_db: f32,
    },
    Dissolvenza {
        ms: u64,
    },
    Latenza {
        ms: i64,
    },
    Equalizzatore {
        guadagni: [f32; BANDE],
        attivo: bool,
    },
    Chiudi,
}

/// Dove il filo della decodifica lascia i suoi annunci.
///
/// Una chiusura e non un `Sender` perché le prove in `motore/prove.rs` la vogliono
/// **sincrona** — un registro che si riempie mentre il contesto gira — mentre il
/// motore vero la vuole su un canale. Chi decodifica non sa quale delle due ha
/// in mano, e non deve: in tutti e due i casi consegnare costa un `push`.
type Voce = Box<dyn Fn(Annuncio) + Send>;

/// La posizione com'è adesso, con la pausa voluta: vedi [`Motore::posizione`].
///
/// Fuori dal `impl` perché la legge anche il filo della decodifica, che il
/// motore non ce l'ha ma le due caselle sì — è la posizione che viaggia dentro
/// ogni [`Annuncio`].
fn istantanea(posizione: &std::sync::Mutex<Posizione>, intese: &Intese) -> Posizione {
    let scritta = posizione
        .lock()
        .map_or_else(|avvelenato| *avvelenato.into_inner(), |g| *g);
    Posizione {
        in_pausa: intese.pausa_voluta.load(Ordering::SeqCst),
        ..scritta
    }
}

/// Un brano già aperto, pronto da dare al motore.
///
/// # Perché il motore riceve un brano aperto e non una sorgente da aprire
///
/// Perché aprire **legge**, e su una condivisione di rete legge molto più di
/// quanto sembri: il riconoscimento del contenitore si porta dietro tutti i
/// blocchi di metadati, e su un FLAC anche la copertina incorporata, che sono
/// spesso centinaia di kilobyte. Finché quel lavoro stava dentro il filo di
/// decodifica non c'era nessun posto in cui infilargli una scadenza — il filo
/// era già dentro la `ReadFile`, e da lì non lo tira fuori nessun sistema
/// operativo. La scadenza dei cinque secondi copriva la sola `File::open`, cioè
/// la parte veloce.
///
/// Spostando l'apertura di qua dal confine, chi chiama la può mandare su un filo
/// suo con una scadenza addosso — è quel che fa `sorgente_di` nell'applicazione
/// — e il filo di decodifica riceve qualcosa che è già pronto a consegnare
/// campioni.
///
/// Porta con sé durata e livello: quando toccherà a lui, la [`Sorgente`] da
/// cui vengono non esisterà più — è stata consumata dall'apertura — e senza
/// questi due campi il brano attaccato in gapless arriverebbe in interfaccia
/// con durata zero e senza correzione di volume.
pub struct BranoAperto {
    decodificatore: Decodificatore,
    durata_ms: u64,
    livello: Livello,
    /// L'etichetta che chi sta sopra gli ha dato: vedi
    /// [`BranoAperto::con_etichetta`].
    etichetta: Option<u64>,
}

impl BranoAperto {
    /// Apre una sorgente per un'uscita di questo formato.
    ///
    /// Il formato lo dice [`Motore::formato`]: è quello con cui il dispositivo
    /// si è aperto davvero, e il decodificatore ci ricampiona sopra.
    ///
    /// # Errori
    ///
    /// `playback.formatUnsupported` per un contenitore che non si riconosce,
    /// `fs.networkUnavailable` se a non rispondere è la condivisione —
    /// ritentabile, e chi chiama ci mette accanto il tasto «Riprova».
    ///
    /// # La bandiera dell'annullo
    ///
    /// Ogni brano ne ha una sua, data al decodificatore e, attraverso lui, alla
    /// sorgente: un flusso di rete la guarda mentre aspetta. La alza il motore
    /// quando un salto, un brano nuovo o uno stop rendono inutile la lettura
    /// in corso — vedi `Condiviso::in_lettura`. Un file del disco non aspetta
    /// niente, e per lui la bandiera è un atomico letto per pacchetto.
    pub fn apri(sorgente: Sorgente, formato: FormatoUscita) -> Result<Self, AppError> {
        let durata_ms = sorgente.durata_ms;
        let livello = Livello {
            guadagno_db: sorgente.replaygain_db,
            picco: sorgente.picco,
        };
        let decodificatore = Decodificatore::apri_con_annullo(
            sorgente,
            formato.frequenza,
            formato.canali,
            Arc::new(AtomicBool::new(false)),
        )?;
        Ok(Self {
            decodificatore,
            durata_ms,
            livello,
            etichetta: None,
        })
    }

    /// Quale brano.
    #[must_use]
    pub const fn track_id(&self) -> i64 {
        self.decodificatore.track_id()
    }

    /// La durata che il contenitore dichiara, se la dichiara.
    ///
    /// Serve a chi in tabella aveva una durata zero: un brano del cloud entra
    /// in libreria dall'elenco del servizio, prima che se ne leggano i tag, e
    /// il primo ascolto è il momento in cui la sua durata si sa senza
    /// chiedere un byte in più.
    #[must_use]
    pub const fn durata_dichiarata_ms(&self) -> Option<u64> {
        self.decodificatore.fine_flusso_ms()
    }

    /// Lo stesso brano, con un'etichetta che lo segue fino all'[`Ancora`].
    ///
    /// # Perché non basta `track_id`
    ///
    /// Perché in un ascolto insieme l'identificativo del brano è quello della
    /// libreria di **questo** computer, e l'altro computer ne ha un altro per
    /// lo stesso file — se lo ha. L'etichetta è il numero della voce della
    /// sessione, uguale su tutti i PC: è quello che due ancore devono
    /// confrontare per sapere se parlano dello stesso brano. Fuori da una
    /// sessione non serve, e resta `None`.
    #[must_use]
    pub const fn con_etichetta(mut self, etichetta: u64) -> Self {
        self.etichetta = Some(etichetta);
        self
    }

    /// L'etichetta data con [`BranoAperto::con_etichetta`], se c'è.
    #[must_use]
    pub const fn etichetta(&self) -> Option<u64> {
        self.etichetta
    }
}

impl std::fmt::Debug for BranoAperto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BranoAperto")
            .field("track_id", &self.decodificatore.track_id())
            .field("durata_ms", &self.durata_ms)
            .finish_non_exhaustive()
    }
}

/// Un brano che comincerà a sentirsi a un certo fotogramma d'uscita.
///
/// # La regola che lega i due numeri
///
/// `da` e `offset_fotogrammi` devono descrivere **lo stesso istante**: `da` dice
/// quando si sente, contato in fotogrammi d'uscita, e `offset_fotogrammi` dice a
/// che punto del brano si è in quel preciso momento. [`Contesto::aggiorna`]
/// somma al secondo il tempo scorso dal primo, quindi due istanti diversi dentro
/// lo stesso segno sfasano la posizione **per tutta la vita del segno**, cioè
/// per tutto il brano.
///
/// I posti che annotano un segno lo fanno per istanti diversi, e ognuno scrive
/// la coppia coerente: l'avvio di un brano (`0`, `0`), un salto (`0`, il
/// fotogramma raggiunto), il passaggio senza buco (`spinti + rinvio`, `0`) e
/// metà dissolvenza (`spinti + meta`, `meta`).
struct Segno {
    /// Da quale fotogramma d'uscita in poi si sente questo brano.
    da: u64,
    track_id: i64,
    durata_ms: u64,
    /// A che punto del brano si è nell'istante indicato da `da`, in fotogrammi
    /// alla frequenza d'uscita.
    ///
    /// Non è «quanto è stato saltato»: è la posizione dentro il brano in
    /// quell'istante, che per un salto coincide col punto raggiunto e per una
    /// dissolvenza vale mezza curva. Vedi la regola in testa al tipo.
    ///
    /// # Perché in fotogrammi e non più in millisecondi
    ///
    /// Era `offset_ms`, e [`Contesto::aggiorna`] gli sommava i millisecondi
    /// scorsi: due troncamenti, uno per addendo, che insieme potevano perdere
    /// quasi due millisecondi — e uno era perso **per tutta la vita del
    /// segno**. Per il cursore non contava niente; per due computer che devono
    /// suonare lo stesso fotogramma sì. Adesso la somma si fa in fotogrammi
    /// esatti e si tronca una volta sola, alla fine.
    offset_fotogrammi: u64,
    /// L'etichetta del brano: vedi [`BranoAperto::con_etichetta`].
    etichetta: Option<u64>,
    replaygain_db: Option<f32>,
    /// A quale ascolto appartiene: un numero nuovo per ogni volta che un brano
    /// **comincia**, lo stesso numero per un salto dentro l'ascolto in corso.
    ///
    /// # Perché non basta `track_id`
    ///
    /// Perché lo stesso brano può cominciare due volte di fila. «Ripeti uno»
    /// prepara il corrente come proprio successivo, e una coda può contenere lo
    /// stesso brano due volte vicine: confrontando l'identificativo, il secondo
    /// inizio non si annunciava, chi sta sopra non preparava il giro dopo, e la
    /// musica si fermava alla seconda ripetizione — o il brano ancora dopo non
    /// partiva mai. L'annuncio segue l'ascolto, e l'ascolto è questo numero.
    ascolto: u64,
    /// Com'è fatto il brano: va in [`Posizione::formato`] quando il segno è
    /// raggiunto.
    formato: Option<FormatoSuonato>,
}

impl Segno {
    /// Il segno come lo vede chi traduce le ancore: vedi [`Mappa`].
    const fn pubblico(&self) -> SegnoPubblico {
        SegnoPubblico {
            da: self.da,
            offset_fotogrammi: self.offset_fotogrammi,
            track_id: self.track_id,
            ascolto: self.ascolto,
            etichetta: self.etichetta,
        }
    }
}

/// Un brano decodificato per intero che si sta ancora sentendo.
///
/// Il perché sta in [`Contesto::uscente`]. Qui interessa solo il numero che gli
/// sta accanto: è **l'ascolto**, non l'identificativo del brano, perché lo
/// stesso brano può essere in fila due volte — «ripeti uno» fa esattamente
/// questo — e allora `track_id` non distingue quale dei due si sta sentendo.
/// Vedi [`Segno::ascolto`].
struct Uscente {
    ascolto: u64,
    brano: BranoAperto,
}

struct Contesto {
    comandi: Receiver<Comando>,
    produttore: rtrb::Producer<f32>,
    condiviso: Arc<Condiviso>,
    posizione: Arc<std::sync::Mutex<Posizione>>,
    /// I segni pubblicati per chi traduce le ancore: vedi [`Mappa`].
    ///
    /// Un lucchetto accanto a quello della posizione, e per la stessa ragione:
    /// lo prendono questo filo e quello di chi comanda, mai la callback.
    mappa: Arc<Mutex<Mappa>>,
    /// I segni, la latenza o l'epoca sono cambiati da quando la mappa è stata
    /// pubblicata. Si ripubblica alla fine di [`Contesto::aggiorna`], una volta
    /// per giro al più: i segni cambiano a ogni brano e a ogni salto, non a ogni
    /// blocco.
    mappa_sporca: bool,
    /// L'ultimo segno tolto dalla fila da [`Contesto::aggiorna`]: vedi
    /// `Mappa::tolto` per il perché serve ancora.
    tolto: Option<SegnoPubblico>,
    /// L'etichetta del brano in `corrente`: segue `corrente` in ogni scambio,
    /// come [`Contesto::durata_corrente_ms`].
    etichetta_corrente: Option<u64>,
    /// Dove vanno gli annunci: vedi [`Voce`] e [`voce_su_filo`].
    voce: Voce,
    /// Gli accordi con la maniglia: vedi [`Intese`].
    intese: Arc<Intese>,
    formato: FormatoUscita,
    corrente: Option<Decodificatore>,
    /// A quale ascolto appartiene `corrente`.
    ///
    /// # Perché non basta il suo `track_id`
    ///
    /// Per la stessa ragione di [`Segno::ascolto`]: con «ripeti uno» il brano
    /// che si decodifica e quello che si sente possono avere lo stesso
    /// identificativo ed essere due ascolti diversi. Negli ultimi secondi di
    /// ogni ripetizione il decodificatore è già sulla ripetizione dopo, e
    /// [`Contesto::riporta_sotto_la_puntina`] — che doveva riconoscerlo — la
    /// scambiava per quella che si sentiva: il salto cadeva sulla ripetizione
    /// successiva col numero d'ascolto di quella in corso, la successiva non
    /// veniva mai annunciata, chi sta sopra non preparava la terza, e la musica
    /// si fermava.
    ascolto_corrente: u64,
    /// A quale ascolto appartiene l'entrante di una dissolvenza: il numero del
    /// segno annotato a metà curva. Vale solo finché `entrante` c'è.
    ascolto_entrante: u64,
    /// La discesa verso la pausa del timer di spegnimento, se ce n'è una.
    /// Vedi [`Motore::sfuma`].
    sfumatura: Option<Sfumatura>,
    /// Il segno dell'ultimo salto aspetta il primo blocco per sapere dove si è
    /// atterrati davvero.
    ///
    /// Su un file il punto d'arrivo lo sa già [`Decodificatore::cerca`]. Su un
    /// flusso di rete il salto è grossolano — si atterra sul pacchetto più
    /// vicino, e rincorrere il fotogramma esatto vorrebbe dire scaricare tutti
    /// i byte in mezzo — e dove si è finiti lo dice solo il primo blocco
    /// decodificato. Con il numero chiesto il cursore e i testi resterebbero
    /// sfasati di quel pacchetto per tutto il resto del brano.
    riallinea_segno: bool,
    /// Il brano che si sta ancora **sentendo** quando la decodifica è già
    /// passata a quello dopo.
    ///
    /// # Perché va tenuto invece di essere lasciato cadere
    ///
    /// Perché `corrente` è il decodificatore, e il decodificatore corre avanti
    /// di tutto quel che l'anello tiene da parte: su un'uscita a 48 kHz stereo
    /// sono più di tre secondi (vedi [`crate::uscita::RISERVA_MS`], che spiega
    /// perché la riserva dichiarata sia molto meno di quel che l'anello regge).
    /// Negli ultimi tre secondi di ogni brano, quindi, il brano che si sente e
    /// il brano che si decodifica sono **due brani diversi**, e finché questo
    /// campo non c'era il decodificatore di quello che si sentiva veniva
    /// lasciato cadere in [`Contesto::passa_al_prossimo`].
    ///
    /// Se ne accorgeva chi trascinava il cursore in quei tre secondi: il salto
    /// cadeva sul decodificatore del brano **successivo** — si sentiva quello,
    /// dal punto chiesto, mentre l'interfaccia continuava a mostrare il titolo
    /// di prima e il cursore correva oltre la fine della barra. Poi il brano
    /// dopo finiva, in canna non c'era rimasto niente, e il motore restava lì:
    /// nessun `Iniziato`, nessun `Fermato`, la coda ferma.
    ///
    /// Vive quanto il **segno** a cui appartiene: lo posa
    /// [`Contesto::passa_al_prossimo`], lo riprende
    /// [`Contesto::riporta_sotto_la_puntina`], e lo lasciano cadere
    /// [`Contesto::aggiorna`] — quando quel segno esce dalla fila, cioè quando
    /// l'ultimo dei suoi campioni è stato udito — e
    /// [`Contesto::scarta_in_volo`]. Non è una cache: tenerlo più a lungo
    /// vorrebbe dire un file aperto in più per tutta la durata del brano
    /// seguente, che su Windows è un file che non si può cancellare.
    uscente: Option<Uscente>,
    prossimo: Option<BranoAperto>,
    /// Dove comincia ogni brano, in fotogrammi d'uscita.
    segni: VecDeque<Segno>,
    /// Campioni decodificati che non sono ancora entrati nell'anello.
    resto: VecDeque<f32>,
    blocco: Vec<f32>,
    /// Fotogrammi spinti nell'anello da quando si è azzerato il conteggio.
    spinti: u64,
    /// L'ultimo ascolto annunciato con [`Evento::Iniziato`]: vedi
    /// [`Segno::ascolto`].
    annunciato: Option<u64>,
    /// Il numero dell'ultimo ascolto cominciato. Vedi [`Contesto::nuovo_ascolto`].
    ascolti: u64,
    /// L'evento di fine è già stato mandato.
    fine_dichiarata: bool,
    volume: f32,
    muto: bool,
    replaygain_attivo: bool,
    bersaglio_db: f32,
    /// La correzione del brano che si sta **decodificando**.
    ///
    /// Non di quello che si sente: da quando il guadagno del brano viaggia coi
    /// campioni invece di stare in uno scalare globale, quel che conta è a chi
    /// appartengono i campioni che stanno uscendo dal decodificatore adesso —
    /// e con una dissolvenza in corso i due non sono lo stesso brano.
    rg_decodifica: Livello,
    /// La correzione del brano **prima** di quello che si decodifica, per la
    /// sua coda che esce dal ricampionatore passato al seguito: vedi
    /// [`Contesto::decodifica_un_blocco`]. Conta solo finché
    /// [`Decodificatore::ereditati`] non è zero.
    rg_altrui: Livello,
    /// Quanto dura la sovrapposizione fra due brani. Zero: nessuna.
    dissolvenza_ms: u64,
    /// La latenza della catena d'uscita dichiarata a mano, in millisecondi.
    ///
    /// Il terzo dei tre ritardi del `//!`, quello che nessuna misura vede.
    /// Positivo toglie alla posizione riportata, negativo le aggiunge. Vedi
    /// [`Motore::latenza`].
    latenza_manuale_ms: i64,
    /// La durata dichiarata del brano corrente, per sapere quando finisce.
    durata_corrente_ms: u64,
    /// Quanti fotogrammi di sovrapposizione sono già stati mescolati.
    dissolvenza_fatti: u64,
    /// Quanto dura la sovrapposizione **cominciata**, in fotogrammi.
    ///
    /// Congelata quando comincia invece di essere ricalcolata a ogni blocco: la
    /// manopola si può muovere mentre due brani si stanno già sovrapponendo, e
    /// cambiare il denominatore a metà curva sposterebbe il guadagno di colpo —
    /// un gradino proprio nel punto in cui la dissolvenza esiste per non farne.
    /// Il valore nuovo vale dal passaggio dopo, che è quel che il comando
    /// promette a chi lo manda.
    dissolvenza_durata: u64,
    /// Il segno del brano entrante è già stato annotato.
    segno_dissolvenza: bool,
    /// Il brano che sta entrando, per tutta la sovrapposizione.
    ///
    /// # Perché non basta `prossimo`
    ///
    /// Perché a metà sovrapposizione il brano entrante viene annunciato, e chi
    /// sta sopra risponde all'annuncio preparando il brano **ancora dopo** —
    /// cioè scrivendo in `prossimo` mentre la dissolvenza lo sta ancora
    /// leggendo. Finché i due erano la stessa casella, la seconda metà della
    /// curva faceva entrare il brano sbagliato e poi il motore attaccava
    /// quello: chi ascoltava sentiva gracchiare e si ritrovava un brano più
    /// avanti nella coda. Qui il brano che entra è al riparo, e `prossimo`
    /// torna a essere quel che dice di essere.
    entrante: Option<BranoAperto>,
    /// Il blocco appena uscito dal decodificatore del brano che entra.
    blocco_entrante: Vec<f32>,
    /// I campioni dell'entrante decodificati e non ancora mescolati.
    ///
    /// Due decodificatori consegnano blocchi di lunghezza diversa — un
    /// pacchetto MP3 sono 1152 fotogrammi, uno FLAC anche 4096 — e mescolarne
    /// uno contro l'altro vorrebbe dire buttare via quel che avanza o riempire
    /// di silenzio quel che manca. Tre quarti del brano entrante nel cestino a
    /// ogni blocco: è così che una dissolvenza diventa un raspare. Da qui
    /// invece la sovrapposizione prende i campioni che le servono, esatti, e
    /// tiene il resto per il blocco dopo.
    coda_entrante: Vec<f32>,
    /// Quanto dura la ripresa del brano entrante. Zero: nessuna in corso.
    ///
    /// Si accende quando il brano uscente finisce **prima** della fine della
    /// curva — succede quando la durata dichiarata dal database è più lunga
    /// dell'audio vero — e il brano che stava entrando resta solo a mezza
    /// ampiezza. Vedi [`risali`].
    entrata_durata: u64,
    /// A che punto è quella ripresa, in fotogrammi.
    entrata_fatti: u64,
    /// Da quale guadagno è ripartita: quello che la curva gli aveva dato.
    entrata_da: f32,
    /// Da qui le curve dell'equalizzatore raggiungono la callback.
    coefficienti: rtrb::Producer<Coefficienti>,
    /// La curva che sta suonando adesso, in decibel.
    eq_correnti: [f32; BANDE],
    /// Quella verso cui si sta andando.
    eq_bersaglio: [f32; BANDE],
    /// Quanti passi mancano per arrivarci.
    eq_passi: u8,
    /// Una curva che l'anello non ha accettato, da riprovare al giro dopo.
    eq_in_attesa: Option<Coefficienti>,
    /// Il campanello del flusso, da suonare prima di ogni svuotamento: vedi
    /// [`Contesto::scarta_in_volo`]. Debole, come ogni campanello: non tiene
    /// aperto un flusso che l'`Uscita` ha lasciato cadere.
    campanello: crate::uscita::Campanello,
}

/// Il filo della decodifica, dentro la rete che lo raccoglie se cade.
///
/// # Perché un `catch_unwind` in un albero senza `unwrap`
///
/// Perché la regola che vieta `unwrap`, `expect`, `panic!` e l'indicizzazione
/// vale sul **nostro** codice, e questo filo passa la sua vita dentro codice di
/// terzi: symphonia decodifica byte arrivati da un disco o da una condivisione
/// di rete, rubato ricampiona. Un pacchetto troncato a metà da una Wi-Fi che
/// respira male è un ingresso che quei due non hanno mai visto, e un `assert!`
/// dentro un decodificatore è un modo perfettamente normale di reagirvi. La
/// regola ci difende da quel che scriviamo noi; questa funzione da quel che
/// chiamiamo.
///
/// # Cosa succedeva senza
///
/// Il `JoinHandle` di questo filo viene buttato via — nessuno lo aspetta — e il
/// profilo di rilascio è `panic = "unwind"`: il filo si smontava in silenzio, e
/// da fuori non cambiava **niente**. La callback di cpal continuava a girare su
/// un anello che nessuno riempiva più, cioè a consegnare zeri al dispositivo
/// per sempre; `perso` restava basso, quindi il sorvegliante delle uscite non
/// vedeva niente da riaprire; i comandi finivano in un canale senza ricevitore
/// e sparivano. L'utente premeva pausa e non succedeva nulla, premeva
/// «prossimo» e non succedeva nulla. Nel registro eventi di Windows non
/// compariva niente, perché il processo era vivo.
///
/// # Perché il contesto si presta invece di essere consumato
///
/// Perché quel che serve **dopo** la caduta sta tutto dentro: l'osservatore, per
/// annunciare il guasto a chi sta sopra, e `condiviso`, per alzare la bandiera
/// che fa ricostruire il motore. Passando `ctx` per valore dentro la chiusura,
/// dopo l'unwind sarebbe irraggiungibile — sarebbe stato smontato con lo stack
/// — e non resterebbe nessuno a dire cos'è successo. Prestandolo, il contesto
/// vive qui fuori e la chiusura lo tocca soltanto.
///
/// # Cosa dichiara `AssertUnwindSafe`
///
/// Che un `Contesto` osservato **dopo** un unwind è ancora buono da leggere. È
/// vero per quel poco che se ne fa: [`Contesto::caduto`] tocca la posizione — un
/// mutex, e un mutex avvelenato lo recupera — e l'osservatore, che è una
/// `Fn` senza stato mutabile suo. I decodificatori, le code e i buffer possono
/// benissimo essere rimasti a metà: nessuno li guarda più, perché questo filo
/// non decodificherà più niente e il motore intero verrà buttato.
///
/// La traccia del panico non si raccoglie: la stampa il gancio globale
/// installato da `crate::diario`, che gira **prima** dell'unwind e quindi prima
/// di qui. Di qua passa solo il messaggio, che è quel che va nel banner.
fn filo_sorvegliato(mut ctx: Contesto) {
    let esito = std::panic::catch_unwind(AssertUnwindSafe(|| filo(&mut ctx)));
    if let Err(carico) = esito {
        ctx.caduto(&messaggio_del_panico(carico.as_ref()));
    }
}

/// Il messaggio di un panico, che è tutto quel che se ne può raccontare.
///
/// `panic!("…")` con una stringa letterale consegna un `&'static str`, uno con
/// argomenti da formattare una `String`, e un panico che arriva da altrove può
/// portare qualunque cosa. I primi due si leggono, il terzo no: dire «causa
/// sconosciuta» è meglio che stampare l'indirizzo di una scatola.
fn messaggio_del_panico(carico: &(dyn std::any::Any + Send)) -> String {
    if let Some(testo) = carico.downcast_ref::<&'static str>() {
        return (*testo).to_owned();
    }
    if let Some(testo) = carico.downcast_ref::<String>() {
        return testo.clone();
    }
    "causa sconosciuta".to_owned()
}

/// I comandi arrivati insieme, ridotti a quelli che vale ancora la pena di
/// eseguire.
///
/// # Il difetto che chiude
///
/// Sei trascinate sulla barra del tempo sono sei `VaiA`, e il filo li eseguiva
/// tutti e sei. Su un file locale non si vede: un salto è una `seek` nel
/// buffer. Su un brano di rete ognuno è un `SeekMode::Accurate` che chiede al
/// catalogo **tutti i byte in mezzo**, e su un nodo che consegna a cinquanta
/// chilobyte al secondo sono decine di secondi l'uno. In fila diventano minuti:
/// due minuti misurati, con il cursore fermo, la finestra che dice che sta
/// suonando, e ogni altro comando in coda dietro di loro — compreso il «suona»
/// di un brano che sta sul disco e non avrebbe niente da aspettare.
///
/// Di quei sei salti ne conta uno: l'ultimo. Gli altri cinque sono posizioni
/// che nessuno ha mai sentito, e attraversarle non è un servizio reso a
/// nessuno.
///
/// # Le due regole
///
/// **Di più salti nello stesso lotto sopravvive l'ultimo.** Non serve che siano
/// attaccati: fra due salti possono esserci un volume o un equalizzatore, e
/// nessuno dei due guarda dove si è: il secondo salto cancella il primo
/// comunque.
///
/// **Un salto seguito da `Suona` o `Ferma` non si fa affatto.** Quei due
/// rifanno la posizione da capo, quindi il salto che li precede è lavoro che
/// verrebbe buttato appena finito — ed è il caso peggiore, perché è
/// esattamente il gesto di chi, stufo di aspettare, mette su un altro brano.
///
/// Una passata sola, dal fondo verso l'inizio, con una domanda che si porta
/// dietro: «quel che ho già visto rende inutile un salto?».
fn compatta(lotto: &mut Vec<Comando>) {
    let mut superfluo = false;
    let mut i = lotto.len();
    while i > 0 {
        i -= 1;
        let Some(comando) = lotto.get(i) else { break };
        match comando {
            Comando::VaiA(_) if superfluo => {
                // `remove` è lineare, e va benissimo: un lotto è quel che è
                // arrivato in quattro millisecondi, cioè una manciata.
                lotto.remove(i);
            }
            Comando::VaiA(_) | Comando::Suona { .. } | Comando::Ferma => superfluo = true,
            _ => {}
        }
    }
}

fn filo(ctx: &mut Contesto) {
    // Fuori dal giro: la stessa scatola serve tutte le passate, e dopo la prima
    // non chiede più memoria a nessuno.
    let mut lotto: Vec<Comando> = Vec::new();
    loop {
        // I comandi in attesa, tutti, prima di lavorare: fra un «pausa» e un
        // «riprendi» arrivati insieme deve vincere il secondo, non il primo.
        // Si raccolgono e **poi** si eseguono, perché per dire quali contano
        // bisogna averli visti tutti — vedi [`compatta`].
        lotto.clear();
        loop {
            match ctx.comandi.try_recv() {
                Ok(Comando::Chiudi) | Err(TryRecvError::Disconnected) => return,
                Ok(comando) => lotto.push(comando),
                Err(TryRecvError::Empty) => break,
            }
        }
        compatta(&mut lotto);
        for comando in lotto.drain(..) {
            ctx.esegui(comando);
        }
        let lavorato = ctx.riempi();
        ctx.avanza_eq();
        ctx.avanza_sfumatura();
        ctx.aggiorna();
        if !lavorato {
            // Niente da decodificare, o anello pieno: quattro millisecondi sono
            // molto meno della riserva e molto più di un giro a vuoto.
            std::thread::sleep(Duration::from_millis(4));
        }
    }
}

/// Il conto di [`Motore::riprendi_a`]: l'istante da scrivere in
/// `Condiviso::avvio_ns`, o il rifiuto.
///
/// Fuori dal metodo perché lo si possa provare: `Motore` apre un dispositivo,
/// e le prove non ne hanno uno. `adesso_ns` lo legge chi chiama **dopo** aver
/// svegliato il flusso, per la ragione scritta là.
///
/// - La latenza dichiarata si **toglie**: `quando_ns` è un istante
///   dell'orecchio, l'avvio è l'istante in cui il fotogramma lascia il
///   dispositivo. Sommarla metterebbe chi ospita due latenze fuori passo dai
///   membri.
/// - Zero vuol dire «nessun avvio»: un istante vero non ci cade mai, ma se ci
///   cadesse un nanosecondo in più non si sente.
/// - Fino a un secondo nel passato è ancora un avvio, e la callback salta la
///   musica già dovuta; oltre è [`RifiutoAvvio::NelPassato`].
fn istante_d_avvio(quando_ns: u64, latenza_ms: i64, adesso_ns: u64) -> Result<u64, RifiutoAvvio> {
    let avvio = i128::from(quando_ns) - i128::from(latenza_ms).saturating_mul(1_000_000);
    let avvio = u64::try_from(avvio.max(1)).unwrap_or(u64::MAX);
    if avvio.saturating_add(1_000_000_000) < adesso_ns {
        return Err(RifiutoAvvio::NelPassato);
    }
    Ok(avvio)
}

/// Avvicina una curva alla successiva, sapendo quanti passi restano dopo questo.
///
/// Con `rimasti` a zero si atterra esatto invece di avvicinarsi per frazioni:
/// senza, resterebbe per sempre un decimo di decibel di scarto fra quel che
/// l'utente ha chiesto e quel che sente — e chi salva un preset salverebbe una
/// curva leggermente diversa da quella che stava ascoltando.
fn avvicina(correnti: &mut [f32; BANDE], bersaglio: &[f32; BANDE], rimasti: u8) {
    let divisore = f32::from(rimasti) + 1.0;
    for (corrente, voluto) in correnti.iter_mut().zip(bersaglio) {
        *corrente = if rimasti == 0 {
            *voluto
        } else {
            *corrente + (*voluto - *corrente) / divisore
        };
    }
}

/// Quanto del volume resta dopo `passato` di una sfumatura lunga `durata`.
///
/// Lineare in decibel, da zero a −[`SFUMATURA_DB`]: il perché sta sulla
/// costante. Oltre la fine resta al fondo invece di risalire, e una durata
/// nulla è già finita.
fn fattore_di_sfumatura(passato: Duration, durata: Duration) -> f32 {
    if durata.is_zero() {
        return 10.0f32.powf(-SFUMATURA_DB / 20.0);
    }
    let t = (passato.as_secs_f32() / durata.as_secs_f32()).clamp(0.0, 1.0);
    10.0f32.powf(-SFUMATURA_DB * t / 20.0)
}

/// Quanti fotogrammi ci sono in tanti campioni interlacciati.
#[expect(
    clippy::integer_division,
    reason = "un fotogramma è esattamente `canali` campioni: divisione esatta per \
              costruzione"
)]
fn fotogrammi(campioni: usize, canali: u16) -> u64 {
    let canali = u64::from(canali).max(1);
    u64::try_from(campioni).unwrap_or(0) / canali
}

/// I fotogrammi che stanno in un tempo, alla frequenza d'uscita.
#[expect(
    clippy::integer_division,
    reason = "il resto è meno di un millisecondo di campioni: sulla soglia \
              della dissolvenza vale un fotogramma su quarantottomila"
)]
fn fotogrammi_da_ms(ms: u64, frequenza: u32) -> u64 {
    ms.saturating_mul(u64::from(frequenza)) / 1000
}

/// Come [`fotogrammi_da_ms`], ma su un tempo che può essere negativo.
///
/// Serve alla correzione di latenza dichiarata a mano, che ha due versi: si può
/// dire «il suono esce dopo» e «la posizione la racconti tardi». Il segno si
/// porta fuori e si rimette dopo, invece di scrivere una seconda divisione, così
/// la regola di arrotondamento resta una sola.
fn fotogrammi_da_ms_con_segno(ms: i64, frequenza: u32) -> i64 {
    let quanti = fotogrammi_da_ms(ms.unsigned_abs(), frequenza);
    let quanti = i64::try_from(quanti).unwrap_or(i64::MAX);
    if ms < 0 {
        quanti.saturating_neg()
    } else {
        quanti
    }
}

/// I millisecondi corrispondenti a tanti fotogrammi.
#[expect(
    clippy::integer_division,
    reason = "il cursore si disegna in millisecondi interi: il resto è meno \
              di un millisecondo, cioè meno di un pixel su qualunque barra"
)]
fn ms_da_fotogrammi(fotogrammi: u64, frequenza: u32) -> u64 {
    let frequenza = u64::from(frequenza).max(1);
    fotogrammi.saturating_mul(1000) / frequenza
}

#[cfg(test)]
mod prove;
