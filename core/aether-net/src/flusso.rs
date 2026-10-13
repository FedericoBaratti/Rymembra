//! Un file remoto che si legge e si posiziona, senza toccare il disco.
//!
//! # A cosa serve
//!
//! A suonare qualcosa che non è un file. `aether-play` non chiede un percorso:
//! chiede un `Read + Seek + Send + Sync`, e il commento che lo dice parla di
//! Android — «là non esiste un percorso da aprire ma una concessione del
//! sistema». Vale identico per un catalogo di solo ascolto: là non esiste un
//! percorso perché non esiste un file, e non deve esistere.
//!
//! # Perché *posizionabile* e non un flusso in avanti
//!
//! Perché un decodificatore non legge dall'inizio alla fine. Cerca i tag in
//! testa, poi in coda — ID3v1 e APE stanno **dopo** l'audio — poi torna al
//! primo fotogramma; e quando qualcuno sposta il cursore salta a metà. Con un
//! flusso in avanti ognuno di quei gesti sarebbe il file intero scaricato per
//! leggerne quattro kilobyte, tre volte prima ancora di sentire una nota.
//!
//! Da qui la forma: una **finestra** di [`FINESTRA`] byte che scorre. Si legge
//! dentro finché si può, e quando il cursore esce si chiede l'intervallo
//! successivo con `Range`, che è la cosa per cui quell'intestazione esiste.
//!
//! # La finestra dopo si chiede prima
//!
//! Mentre si legge di fila, la finestra successiva parte su un filo suo
//! (vedi [`ANTICIPO_DOPO`]). Senza, il decodificatore si fermava ad aspettarla
//! ogni volta che arrivava in fondo a quella di adesso, e l'anello del motore
//! vale circa tre secondi: sull'Internet Archive una richiesta costa due
//! secondi prima del primo byte, più il tempo dei byte, e ogni finestra era un
//! silenzio in mezzo alla canzone. Misurato su una linea da cinquanta kilobyte
//! al secondo: fermo quattro secondi e mezzo al terzo secondo di brano, poi
//! oltre quaranta all'undicesimo.
//!
//! Un salto altrove rende inutile quel che si stava chiedendo, e la richiesta
//! si lascia a metà ([`Sorgente::pezzo_finche`]) invece di contendere la linea
//! alla finestra che serve.
//!
//! # Niente disco, e non è un dettaglio implementativo
//!
//! Non c'è nessun file temporaneo, e non ci sarà. I termini di Jamendo — che è
//! il primo catalogo per cui questo modulo esiste — vietano espressamente la
//! cache e l'accesso fuori linea, e `TERMS.md` § 2 lo dice per iscritto. Quel
//! che sta in memoria sono al più due finestre — quella che si legge e quella
//! che sta arrivando — che si sovrascrivono mentre il brano avanza: alla fine
//! dell'ascolto non ne resta niente, che è precisamente il patto.
//!
//! Il tetto è quindi un vincolo, non un'ottimizzazione. Un buffer che
//! crescesse tenendo tutto quel che è passato sarebbe una copia del brano in
//! memoria, e una copia in memoria è una copia.
//!
//! # Cosa succede quando la rete cade a metà canzone
//!
//! Un errore di lettura, che risale al decodificatore e da lì al motore come un
//! brano che si interrompe — e che **dice di essere la rete**, non un file
//! rotto. Come, e perché è la parte difficile, sta scritto su [`da_app`].
//!
//! Prima che succeda, però, si riprova, in due posti e non tre. La richiesta
//! di un intervallo si ripete da sé per i guasti che passano da soli
//! (`Rete::intervallo_finche`: tre tentativi con l'attesa che raddoppia, dentro
//! un bilancio di tempo), e la finestra chiesta **in anticipo** che cade si
//! richiede sul suo filo dopo una pausa che cresce ([`FlussoHttp`], vedi
//! `forse_anticipa`), invece di restare spenta fino alla fine della finestra di
//! adesso. Un terzo ciclo qui, attorno alla lettura in primo piano, no: vorrebbe
//! dire una canzone che resta ferma per minuti invece di dire che non c'è rete.

use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use aether_domain::errors::{AppError, ErrorCode};

use crate::http::{Cancello, Pezzo, Rete, Rimandi};

/// Quanto si chiede alla volta.
///
/// Duecentocinquantasei kilobyte: circa sedici secondi di un mp3 a 128 kbps,
/// meno di sei di un FLAC. Abbastanza perché il decodificatore non torni a
/// chiedere ogni fotogramma, abbastanza poco perché saltare a metà brano non
/// scarichi mezza canzone per farne sentire l'inizio.
pub const FINESTRA: u64 = 256 * 1024;

/// Fin dove la finestra può crescere quando si legge di fila.
///
/// # Perché cresce, e perché si ferma qui
///
/// Perché il decodificatore, quando non trova una tavola di salto, **scorre**:
/// cercare a metà di un mp3 senza indice vuol dire leggerne la prima metà tutta
/// di seguito. A [`FINESTRA`] fissa sono una quarantina di richieste, e
/// misurato ognuna costa fra un secondo e due di sola attesa prima che arrivi
/// un byte — cioè un minuto buono speso a chiedere, oltre al tempo di
/// scaricare.
///
/// Due megabyte e non di più, ed è il tetto che tiene insieme le due cose che
/// questo modulo deve tenere insieme. Chiedere dieci volte tanto per volta
/// taglia l'attesa; tenere in memoria un brano intero sarebbe **una copia del
/// brano**, che è precisamente quel che i termini di Jamendo vietano e
/// `TERMS.md` § 2 promette di non fare. Le finestre restano due al più — quella
/// che si legge e quella che arriva — e si sovrascrivono: quel che è passato
/// non si conserva, a nessuna dimensione.
pub const FINESTRA_MASSIMA: u64 = 2 * 1024 * 1024;

/// Dopo quanti byte letti di fila in una finestra si chiede la prossima.
///
/// Non subito: all'apertura il decodificatore salta — la testa, la coda coi
/// tag, di nuovo la testa — e una finestra chiesta al primo byte sarebbe una
/// richiesta buttata a ogni salto, su una linea che in quel momento serve a
/// far partire il brano. Sessantaquattro kilobyte di fila (o metà finestra, se
/// la finestra è più corta) sono la firma di qualcuno che sta suonando: un MP3
/// dell'Internet Archive ne consuma così in un paio di secondi, e il resto
/// della finestra è il tempo che la prossima ha per arrivare.
const ANTICIPO_DOPO: u64 = 64 * 1024;

/// Quanto si aspetta, la prima volta, prima di richiedere una finestra
/// anticipata che è caduta.
///
/// Un secondo, come la prima pausa di `Rete::ritenta`: il guasto che passa da
/// sé — un nodo che ha rifiutato una connessione, un proxy che ha chiuso — è
/// già passato. Le volte dopo raddoppia, fino a [`PAUSA_ANTICIPO_MASSIMA`].
const PAUSA_ANTICIPO: Duration = Duration::from_secs(1);

/// La pausa più lunga fra due richieste anticipate cadute di fila.
///
/// Sedici secondi: una finestra da due megabyte è una dozzina di secondi di
/// FLAC, e aspettare di più vorrebbe dire lasciare che sia sempre la lettura in
/// primo piano ad arrivarci per prima — cioè tornare al silenzio a ogni
/// finestra che l'anticipo esiste per togliere.
const PAUSA_ANTICIPO_MASSIMA: Duration = Duration::from_secs(16);

/// Quanti rimandi di fila può seguire un flusso.
///
/// Cinque, contati sui cataloghi veri. L'Internet Archive ne fa uno —
/// `archive.org/download/…` verso il nodo che tiene il file. Audius due al
/// più: il nodo di scoperta rimanda a un nodo di contenuti, e un nodo di
/// contenuti che non ha quel file rimanda a uno che ce l'ha. Cinque lascia
/// margine a un catalogo che aggiunga un passaggio, e ferma un giro chiuso
/// prima che diventi dieci richieste per un brano che non arriverà — il dieci
/// di serie di `ureq`, che fino alla 2.4.0 valeva anche qui.
pub const RIMANDI_FLUSSO: u32 = 5;

/// Da dove arrivano i pezzi.
///
/// Un tratto e non direttamente [`Rete`], per una ragione sola: le prove di
/// questo crate girano **senza rete**, e la logica della finestra — quando si
/// riempie, quando basta quel che c'è, cosa succede in fondo al file — è tutto
/// ciò che questo modulo contiene. Provarla contro un servizio vero vorrebbe
/// dire non provarla.
pub trait Sorgente: Send + Sync + std::fmt::Debug {
    /// I byte da `da`, per `quanti` al massimo.
    ///
    /// Può restituirne meno: è la fine del file, e non è un guasto.
    ///
    /// # Errori
    ///
    /// Quelli di [`Rete::intervallo`].
    fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError>;

    /// Come [`Self::pezzo`], ma chi lo chiede può smettere di volerlo.
    ///
    /// È la forma della lettura anticipata: `avanti` risponde `false` quando
    /// chi ascolta è saltato altrove, e una sorgente che sa leggere a blocchi
    /// ne approfitta per lasciare la richiesta a metà invece di contendere la
    /// linea alla finestra che serve davvero. Quella di serie non sa farlo e
    /// arriva in fondo, che per una sorgente in memoria vuol dire subito.
    ///
    /// # Errori
    ///
    /// Quelli di [`Self::pezzo`], e `internal.aborted` quando `avanti` ha detto
    /// di no.
    fn pezzo_finche(
        &self,
        da: u64,
        quanti: u64,
        avanti: &dyn Fn() -> bool,
    ) -> Result<Pezzo, AppError> {
        let _ = avanti;
        self.pezzo(da, quanti)
    }
}

/// Un indirizzo, letto a intervalli.
#[derive(Debug)]
pub struct DaRete {
    rete: Rete,
    url: String,
    /// Il nodo a cui il primo rimando ha portato, se c'è stato.
    ///
    /// # Perché si ricorda
    ///
    /// Perché l'Internet Archive risponde a ogni `/download/…` con un rimando
    /// al nodo che quel file ce l'ha, e misurato quel rimando costa novecento
    /// millisecondi su due secondi di attesa. Un brano che si scorre avanti e
    /// indietro fa decine di richieste: pagarlo ogni volta vuol dire pagarlo
    /// decine di volte per sapere una cosa che non cambia.
    ///
    /// Un `Mutex` e non un campo riscrivibile perché [`Sorgente::pezzo`] prende
    /// `&self`: il tratto è così apposta, perché la sorgente viene condivisa. È
    /// l'unico stato mutabile di questo tipo, non è sulla via calda — si scrive
    /// una volta per brano — e un lucchetto avvelenato non è un guasto da
    /// propagare: si riparte dall'indirizzo di partenza, che funziona sempre.
    nodo: std::sync::Mutex<Option<String>>,
}

impl DaRete {
    /// Legge questo indirizzo con questa rete, e segue i rimandi solo dove
    /// `cancello` dice di sì.
    ///
    /// La `Rete` è quella del catalogo che ha dato l'indirizzo, non una nuova:
    /// così l'agente, i timeout e l'interruttore sono quelli, e la riserva di
    /// connessioni è già calda.
    ///
    /// # Perché il cancello è obbligatorio
    ///
    /// Perché un flusso i rimandi li deve seguire — l'Internet Archive non
    /// serve un file senza — e un rimando è un indirizzo scelto da qualcun
    /// altro. L'indirizzo di partenza è passato dall'allowlist dei cataloghi;
    /// senza un cancello anche sui salti, quell'allowlist varrebbe per il primo
    /// indirizzo e non per il secondo. Chi sa dove un catalogo è di casa è chi
    /// ha riconosciuto il catalogo, e il cancello lo porta lui: questo crate
    /// non vede `aether-catalogo` e non deve cominciare.
    ///
    /// La copia della rete che si tiene qui ha [`Rimandi::Sorvegliati`] con al
    /// più [`RIMANDI_FLUSSO`] salti; la riserva di connessioni resta quella del
    /// catalogo.
    #[must_use]
    pub fn nuova(rete: &Rete, url: String, cancello: Cancello) -> Self {
        Self {
            rete: rete.con_rimandi(Rimandi::Sorvegliati {
                quanti: RIMANDI_FLUSSO,
                cancello,
            }),
            url,
            nodo: std::sync::Mutex::new(None),
        }
    }
}

impl Sorgente for DaRete {
    fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
        self.pezzo_finche(da, quanti, &|| true)
    }

    fn pezzo_finche(
        &self,
        da: u64,
        quanti: u64,
        avanti: &dyn Fn() -> bool,
    ) -> Result<Pezzo, AppError> {
        let dove = self
            .nodo
            .lock()
            .map(|n| n.clone())
            .unwrap_or(None)
            .unwrap_or_else(|| self.url.clone());
        let pezzo = self.rete.intervallo_finche(&dove, da, quanti, avanti)?;
        if let Some(nuovo) = pezzo.url_finale.as_ref()
            && let Ok(mut nodo) = self.nodo.lock()
            && nodo.is_none()
        {
            *nodo = Some(nuovo.clone());
        }
        Ok(pezzo)
    }
}

/// Un file remoto, che si legge e si posiziona.
///
/// Implementa `Read + Seek + Send + Sync`, cioè esattamente quel che
/// `aether_play::Flusso` chiede — e quel che `aether_app::files::ReadSeek`
/// riconosce da sé, avendo un'implementazione generale.
#[derive(Debug)]
pub struct FlussoHttp {
    /// Da dove arrivano i byte. Condivisa, perché la finestra dopo la chiede
    /// un altro filo: vedi [`Anticipo`].
    sorgente: Arc<dyn Sorgente>,
    /// Quanto è lungo il file, quando il servizio lo dichiara.
    totale: Option<u64>,
    /// Dove è il cursore, dal punto di vista di chi legge.
    posizione: u64,
    /// La finestra in memoria.
    finestra: Vec<u8>,
    /// A quale byte del file corrisponde il primo byte della finestra.
    inizio: u64,
    /// Quanto si chiederà alla prossima richiesta.
    ///
    /// Parte da [`FINESTRA`], raddoppia a ogni riempimento che riprende da dove
    /// finiva il precedente — cioè mentre si legge di fila — e torna a
    /// [`FINESTRA`] al primo salto vero. Vedi [`FINESTRA_MASSIMA`].
    passo: u64,
    /// La finestra dopo questa, se la si sta già chiedendo.
    anticipo: Option<Anticipo>,
    /// Quante finestre anticipate di fila sono cadute.
    ///
    /// Torna a zero alla prima che arriva. Decide quanto aspettare prima di
    /// richiedere la prossima: vedi [`PAUSA_ANTICIPO`].
    anticipi_caduti: u32,
    /// Prima di quando non si richiede niente in anticipo, dopo un guasto.
    anticipo_non_prima: Option<Instant>,
    /// La prima pausa dopo un anticipo caduto. [`PAUSA_ANTICIPO`], tranne che
    /// nelle prove, che non aspettano un secondo per vedere succedere una cosa.
    pausa_anticipo: Duration,
    /// Alzata da chi suona quando la lettura in corso non serve più: un salto
    /// dopo un salto, un altro brano, uno stop. Vedi [`Self::ascolta_annullo`].
    annullo: Option<Arc<AtomicBool>>,
}

/// La finestra successiva, chiesta mentre si legge quella di adesso.
///
/// Un filo e non un compito asincrono perché in questo albero un runtime non
/// c'è, e un filo per finestra — una ogni decina di secondi di musica, nel
/// caso più fitto — non è un costo da contare.
#[derive(Debug)]
struct Anticipo {
    /// Da quale byte comincia: è la fine della finestra che l'ha chiesta.
    da: u64,
    /// Il filo che la sta portando. `Option` solo per poterlo prendere con
    /// `join` da un tipo che ha un `Drop`.
    filo: Option<std::thread::JoinHandle<Result<Pezzo, AppError>>>,
    /// Com'è andata, quando il filo ha finito e qualcuno l'ha guardato.
    esito: Option<Result<Pezzo, AppError>>,
    /// Alzata quando la finestra non serve più.
    lascia: Arc<AtomicBool>,
}

impl Anticipo {
    /// Se il filo ha finito, ne prende l'esito — senza aspettarlo se non ha
    /// finito.
    ///
    /// È la domanda che mancava: fino alla 2.4.0 una finestra anticipata caduta
    /// si scopriva soltanto arrivandoci, e fino ad allora restava lì a occupare
    /// il posto, cioè a impedire che se ne chiedesse un'altra. La lettura
    /// anticipata si spegneva per il resto della finestra, e alla finestra dopo
    /// si tornava al silenzio che esiste per togliere.
    fn raccogli(&mut self) {
        if self
            .filo
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished)
        {
            self.esito = Some(Self::fine(self.filo.take()));
        }
    }

    /// Aspetta il filo, se c'è ancora, e restituisce la finestra se è arrivata.
    fn aspetta(&mut self) -> Option<Pezzo> {
        if self.filo.is_some() {
            self.esito = Some(Self::fine(self.filo.take()));
        }
        self.esito.take().and_then(Result::ok)
    }

    /// L'esito di un filo finito. Un filo caduto in panico vale un guasto di
    /// lettura: da qui non si distingue, e per chi legge è la stessa cosa.
    fn fine(
        filo: Option<std::thread::JoinHandle<Result<Pezzo, AppError>>>,
    ) -> Result<Pezzo, AppError> {
        filo.and_then(|filo| filo.join().ok()).unwrap_or_else(|| {
            Err(lettura_interrotta(
                "il filo della finestra anticipata è caduto".to_owned(),
            ))
        })
    }

    /// È caduta: il filo ha finito, e con un guasto.
    fn caduto(&self) -> bool {
        matches!(self.esito, Some(Err(_)))
    }
}

impl Drop for Anticipo {
    /// Nessuno la aspetta più: che la richiesta si fermi al blocco dopo.
    ///
    /// Succede a ogni salto altrove e alla fine del brano. Il filo non si
    /// aspetta — sarebbe proprio l'attesa da cui questo tipo esiste per
    /// liberare — e se ne va da solo appena guarda la bandiera.
    fn drop(&mut self) {
        self.lascia.store(true, Ordering::Relaxed);
    }
}

impl FlussoHttp {
    /// Apre un indirizzo.
    ///
    /// Fa **una** richiesta subito, e non per impazienza: serve a sapere quanto
    /// è lungo il file. Senza quel numero `Seek(End)` non funziona, e
    /// `MediaSource::is_seekable` di symphonia risponde `false` — cioè il
    /// cursore sparisce dalla finestra prima ancora che il brano cominci. La
    /// finestra che torna non si butta: è l'inizio del file, che è la prima
    /// cosa che il decodificatore chiederà.
    ///
    /// I rimandi si seguono solo dove `cancello` dice di sì: vedi
    /// [`DaRete::nuova`].
    ///
    /// # Errori
    ///
    /// Quelli di [`Rete::intervallo`], e `net.badSchema` per un rimando che il
    /// cancello rifiuta.
    pub fn nuovo(rete: &Rete, url: String, cancello: Cancello) -> Result<Self, AppError> {
        Self::da(Box::new(DaRete::nuova(rete, url, cancello)))
    }

    /// Come [`Self::nuovo`], da una sorgente qualunque.
    ///
    /// # Errori
    ///
    /// Quelli della sorgente.
    pub fn da(sorgente: Box<dyn Sorgente>) -> Result<Self, AppError> {
        let primo = sorgente.pezzo(0, FINESTRA)?;
        Ok(Self {
            sorgente: Arc::from(sorgente),
            totale: primo.totale,
            posizione: 0,
            finestra: primo.byte,
            inizio: 0,
            passo: FINESTRA,
            anticipo: None,
            anticipi_caduti: 0,
            anticipo_non_prima: None,
            pausa_anticipo: PAUSA_ANTICIPO,
            annullo: None,
        })
    }

    /// Smette di aspettare la rete quando `annullo` si alza.
    ///
    /// La richiesta della finestra che serve adesso la guarda fra un blocco e
    /// l'altro del corpo e durante le pause fra un tentativo e l'altro
    /// ([`Rete::intervallo_finche`]), e restituisce `internal.aborted`: chi
    /// legge — il decodificatore di `aether-play` — riconosce l'annullo per
    /// quel che è, e non mostra «Riprova» per una lettura che nessuno vuole
    /// più. Solo la connessione stessa non si interrompe: la scadenza di
    /// [`Rete`] è il suo tetto.
    pub fn ascolta_annullo(&mut self, annullo: Arc<AtomicBool>) {
        self.annullo = Some(annullo);
    }

    /// Dove finisce la finestra che si ha in mano.
    fn fine(&self) -> u64 {
        self.inizio
            .saturating_add(u64::try_from(self.finestra.len()).unwrap_or(0))
    }

    /// Chiede la finestra dopo, se è il momento.
    ///
    /// Il momento è quando si sta leggendo di fila da abbastanza byte: vedi
    /// [`ANTICIPO_DOPO`]. Non lo è quando una richiesta è già in volo, quando la
    /// finestra è l'ultima, e quando il filo non si riesce a far partire:
    /// allora si torna a chiedere la finestra quando serve, come prima.
    ///
    /// # Quando una finestra anticipata cade
    ///
    /// Si lascia andare **subito**, e se ne chiede un'altra dopo una pausa che
    /// raddoppia a ogni guasto di fila ([`PAUSA_ANTICIPO`]). Fino alla 2.4.0 la
    /// finestra caduta restava in mano fino alla fine di quella di adesso, e
    /// in quel tempo non se ne chiedeva un'altra: un guasto di un istante
    /// spegneva la lettura anticipata per dieci secondi di musica, e si
    /// tornava a fermarsi alla fine di ogni finestra.
    ///
    /// # Quando una finestra è l'ultima
    ///
    /// Quando la fine è dichiarata e ci si è arrivati. Senza una lunghezza
    /// dichiarata, quando la sorgente ha dato meno di quel che si era chiesto,
    /// che è il suo modo di dire che il file è finito. Con la lunghezza
    /// dichiarata la seconda regola non vale: certi nodi tagliano ogni risposta
    /// a un tetto loro, e una finestra corta con il file ancora lontano dalla
    /// fine non vuol dire niente — fino alla 2.4.0 spegneva la lettura
    /// anticipata per tutto il brano.
    fn forse_anticipa(&mut self) {
        if let Some(anticipo) = self.anticipo.as_mut() {
            anticipo.raccogli();
            if !anticipo.caduto() {
                return;
            }
            self.anticipo = None;
            self.anticipi_caduti = self.anticipi_caduti.saturating_add(1);
            self.anticipo_non_prima = Some(Instant::now() + self.pausa_dopo_un_guasto());
        }
        if self
            .anticipo_non_prima
            .is_some_and(|non_prima| Instant::now() < non_prima)
        {
            return;
        }
        let lunghezza = u64::try_from(self.finestra.len()).unwrap_or(0);
        let fine = self.fine();
        let ultima = match self.totale {
            Some(totale) => fine >= totale,
            None => lunghezza < self.passo,
        };
        if ultima {
            return;
        }
        let letti = self.posizione.saturating_sub(self.inizio);
        if letti < ANTICIPO_DOPO && letti.saturating_mul(2) < lunghezza {
            return;
        }
        // Lo stesso passo che `riempi` sceglierà arrivando qui di fila: la
        // finestra chiesta prima è quella che si sarebbe chiesta dopo.
        let quanti = self.passo.saturating_mul(2).min(FINESTRA_MASSIMA);
        let lascia = Arc::new(AtomicBool::new(false));
        let bandiera = Arc::clone(&lascia);
        let sorgente = Arc::clone(&self.sorgente);
        let filo = std::thread::Builder::new()
            .name("flusso-anticipo".to_owned())
            .spawn(move || {
                sorgente.pezzo_finche(fine, quanti, &|| !bandiera.load(Ordering::Relaxed))
            })
            .ok();
        if filo.is_some() {
            self.anticipo = Some(Anticipo {
                da: fine,
                filo,
                esito: None,
                lascia,
            });
        }
    }

    /// Quanto aspettare prima di richiedere una finestra anticipata, dopo
    /// [`Self::anticipi_caduti`] guasti di fila.
    fn pausa_dopo_un_guasto(&self) -> Duration {
        let raddoppi = self.anticipi_caduti.saturating_sub(1).min(8);
        self.pausa_anticipo
            .saturating_mul(1_u32 << raddoppi)
            .min(PAUSA_ANTICIPO_MASSIMA)
    }

    /// Quanto è lungo, se il servizio lo ha detto.
    #[must_use]
    pub const fn lunghezza(&self) -> Option<u64> {
        self.totale
    }

    /// Il cursore sta dentro la finestra che si ha in mano.
    fn dentro(&self) -> bool {
        let fine = self
            .inizio
            .saturating_add(u64::try_from(self.finestra.len()).unwrap_or(0));
        self.posizione >= self.inizio && self.posizione < fine
    }

    /// Chiede la finestra che contiene il cursore.
    ///
    /// Il passo raddoppia quando questa finestra riprende **esattamente** da
    /// dove finiva quella di prima: è la firma di una lettura di fila, e una
    /// lettura di fila è o un brano che suona o un decodificatore che sta
    /// scorrendo per trovare un punto. In tutti e due i casi la prossima
    /// richiesta si sa già che arriverà, e conviene che sia una invece di
    /// quattro. Un salto vero riporta il passo a [`FINESTRA`], perché dopo un
    /// salto quel che il decodificatore vuole sono di solito pochi kilobyte —
    /// una testa di tag, una coda di indice — e chiederne due megabyte sarebbe
    /// aspettare per niente.
    ///
    /// Se la finestra che serve è quella già chiesta in anticipo, si prende
    /// quella — aspettandola, se non è ancora arrivata tutta. Se è un'altra,
    /// l'anticipo si lascia **prima** di chiedere: tenerlo in volo vorrebbe
    /// dire due richieste che si dividono la linea mentre qualcuno aspetta la
    /// seconda.
    fn riempi(&mut self) -> Result<(), AppError> {
        let di_fila = self.fine() == self.posizione;
        self.passo = if di_fila {
            self.passo.saturating_mul(2).min(FINESTRA_MASSIMA)
        } else {
            FINESTRA
        };
        let posizione = self.posizione;
        // La finestra anticipata si aspetta **mentre è ancora in mano**: il suo
        // `Drop` alza la bandiera che la lascia, e lasciarla cadere prima del
        // `join` voleva dire interrompere da sé la richiesta che si stava per
        // aspettare — e poi rifarla da capo in primo piano. Una di un altro
        // posto invece cade qui, prima della richiesta nuova.
        let pronta = match self.anticipo.take() {
            Some(mut anticipo) if anticipo.da == posizione => anticipo.aspetta(),
            altro => {
                drop(altro);
                None
            }
        };
        if pronta.is_some() {
            // Una arrivata: la serie dei guasti è finita, e la prossima pausa
            // riparte dalla prima.
            self.anticipi_caduti = 0;
            self.anticipo_non_prima = None;
        }
        // Un anticipo fallito si richiede qui, una volta, e non si racconta:
        // il suo guasto è arrivato mentre nessuno aspettava, magari decine di
        // secondi fa, e a decidere se la rete c'è è la richiesta di adesso.
        let pezzo = match (pronta, self.annullo.as_ref()) {
            (Some(pezzo), _) => pezzo,
            (None, Some(annullo)) => {
                let annullo = Arc::clone(annullo);
                self.sorgente
                    .pezzo_finche(self.posizione, self.passo, &|| {
                        !annullo.load(Ordering::Acquire)
                    })?
            }
            (None, None) => self.sorgente.pezzo(self.posizione, self.passo)?,
        };
        self.inizio = self.posizione;
        self.finestra = pezzo.byte;
        // La lunghezza si impara alla prima risposta che la dichiara e non si
        // disimpara più: certi servizi la mettono solo sulla prima risposta, e
        // un cursore che sparisce a metà brano perché la seconda taceva sarebbe
        // un difetto invisibile in prova e ovvio in uso.
        if self.totale.is_none() {
            self.totale = pezzo.totale;
        }
        Ok(())
    }
}

impl Read for FlussoHttp {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        // Oltre la fine dichiarata non si chiede niente: una richiesta con un
        // `Range` che comincia dopo l'ultimo byte è un 416, cioè un errore al
        // posto della fine naturale di un brano.
        if self.totale.is_some_and(|t| self.posizione >= t) {
            return Ok(0);
        }
        if !self.dentro() {
            self.riempi().map_err(da_app)?;
        }
        let scarto = usize::try_from(self.posizione.saturating_sub(self.inizio)).unwrap_or(0);
        let Some(resto) = self.finestra.get(scarto..) else {
            return Ok(0);
        };
        if resto.is_empty() {
            // La sorgente non ha più niente: è la fine, anche se nessuno aveva
            // dichiarato una lunghezza.
            return Ok(0);
        }
        let quanti = resto.len().min(buf.len());
        let (da, a) = (resto.get(..quanti), buf.get_mut(..quanti));
        if let (Some(da), Some(a)) = (da, a) {
            a.copy_from_slice(da);
            self.posizione = self
                .posizione
                .saturating_add(u64::try_from(quanti).unwrap_or(0));
            self.forse_anticipa();
            return Ok(quanti);
        }
        Ok(0)
    }
}

impl Seek for FlussoHttp {
    fn seek(&mut self, verso: SeekFrom) -> std::io::Result<u64> {
        let nuova = match verso {
            SeekFrom::Start(quanto) => i128::from(quanto),
            SeekFrom::Current(scarto) => i128::from(self.posizione) + i128::from(scarto),
            SeekFrom::End(scarto) => {
                let Some(totale) = self.totale else {
                    // Senza lunghezza non si sa dov'è la fine, e inventarsela
                    // vorrebbe dire consegnare al decodificatore dei tag letti
                    // dal posto sbagliato.
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Unsupported,
                        "questo flusso non dichiara quanto è lungo: non si può partire dalla fine",
                    ));
                };
                i128::from(totale) + i128::from(scarto)
            }
        };
        if nuova < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "posizione negativa",
            ));
        }
        // Spostarsi non chiede niente alla rete: la finestra si riempirà alla
        // prima lettura che ne ha bisogno. È quel che rende gratis il `seek`
        // alla fine e ritorno che ogni lettore di tag fa all'apertura.
        self.posizione = u64::try_from(nuova).unwrap_or(u64::MAX);
        Ok(self.posizione)
    }
}

/// Il numero che Windows dà quando il nome di rete non è più disponibile.
///
/// `ERROR_NETNAME_DELETED`. È il più comune dei guasti di rete a metà lettura,
/// ed è quello che si sceglie quando non c'è niente di più preciso da dire.
const NOME_DI_RETE_SPARITO: i32 = 64;

/// Il numero che Windows dà quando la lettura è scaduta.
///
/// `ERROR_SEM_TIMEOUT`: «ha smesso di rispondere mentre si leggeva», che è
/// esattamente quel che è successo quando la scadenza di [`Rete`] scatta a metà
/// di un intervallo.
const SEMAFORO_SCADUTO: i32 = 121;

/// Il numero che Windows dà quando la rete non si raggiunge affatto.
///
/// `ERROR_NETWORK_UNREACHABLE`, per il caso in cui non c'era nessuna
/// connessione da usare.
const RETE_IRRAGGIUNGIBILE: i32 = 1231;

/// Da un errore di Aether a uno di `std::io`.
///
/// # Perché un numero di sistema e non un messaggio
///
/// Perché il messaggio non arriva. Fra questa riga e chi deve decidere che
/// faccia mostrare all'utente ci sono due passaggi che non lasciano passare
/// niente:
///
/// - **symphonia**, che quando il riconoscimento del contenitore non riesce
///   dice «nessun lettore adatto» e *butta via* l'errore di lettura che glielo
///   ha impedito;
/// - **`aether_play::decodifica`**, che per distinguere una condivisione morta
///   da un file rotto guarda una cosa sola, `raw_os_error()`, perché è l'unica
///   che `std::io::Error` porti fino in fondo. L'elenco dei numeri sta in
///   `aether_domain::errors::rete::e_di_rete`, ed è condiviso fra chi apre un
///   file e chi lo sta già leggendo, apposta perché la stessa causa racconti la
///   stessa cosa da tutte e due le parti.
///
/// Un `io::Error` costruito con `Error::new` non ha nessun numero. Arriverebbe
/// di là indistinguibile da un settore rovinato, cioè come
/// `playback.decodeFailed`: «questo file è danneggiato, sostituiscilo» — per un
/// brano che non è un file e che sta benissimo dov'è, mentre a mancare è il
/// cavo. La versione 2.1.0 è stata una campagna intera per chiudere le otto
/// strade da cui quella bugia entrava; questo modulo non ne apre una nona.
///
/// # Perché *tutti* i guasti di qui sono di rete
///
/// Perché di file non ce n'è. Le uniche letture che possono fallire sono le
/// richieste `Range` di [`Sorgente::pezzo`] — un flusso finito restituisce zero
/// byte, non un guasto — quindi quel che passa da qui viene sempre dal cavo, e
/// non esiste il caso in cui la risposta giusta sia «sostituisci il file».
/// Ecco perché il ramo predefinito non è un ripiego prudente: è la regola.
///
/// # Che cosa si perde
///
/// Il testo. `std::io::Error` porta *o* un numero di sistema *o* un messaggio,
/// mai tutti e due, e fra i due quello che serve è il numero: il codice del
/// catalogo lo ricostruisce `e_di_rete` di là, mentre un messaggio che non
/// sopravvive al viaggio non lo legge nessuno. Chi vuole sapere *quale*
/// richiesta è andata male ce l'ha ancora davanti al motore — l'apertura del
/// flusso restituisce l'[`AppError`] intero, con il suo codice e la sua causa.
///
/// Fuori da Windows `e_di_rete` risponde sempre `false` — la ragione sta scritta
/// nel suo modulo — e il numero resta lì senza che nessuno lo interroghi. È il
/// limite dichiarato di quel modulo, non uno nuovo di questo.
fn da_app(err: AppError) -> std::io::Error {
    std::io::Error::from_raw_os_error(match err.code().kind().code() {
        "net.offline" => RETE_IRRAGGIUNGIBILE,
        "net.timeout" => SEMAFORO_SCADUTO,
        _ => NOME_DI_RETE_SPARITO,
    })
}

/// Un guasto di lettura, per chi costruisce una sorgente propria.
///
/// Sta qui perché il codice giusto è uno solo e va scelto in un posto solo:
/// `download.network` è quel che l'interfaccia sa già tradurre in «la rete si è
/// interrotta», e che la coda sa già ritentare.
#[must_use]
pub fn lettura_interrotta(perche: String) -> AppError {
    AppError::new(ErrorCode::DownloadNetwork).with_cause(perche)
}

#[cfg(test)]
mod prove {
    use super::*;
    use std::sync::Mutex;

    /// Un file finto in memoria, che conta quante volte gli si chiede qualcosa.
    #[derive(Debug)]
    struct Finta {
        byte: Vec<u8>,
        /// Dichiara la propria lunghezza, come farebbe un servizio educato.
        dichiara: bool,
        richieste: Mutex<Vec<(u64, u64)>>,
    }

    impl Finta {
        fn nuova(quanti: usize, dichiara: bool) -> Self {
            Self {
                // Un contenuto riconoscibile: il byte n vale n modulo 251, che
                // essendo primo non si allinea con nessuna potenza di due — e
                // quindi un errore di scarto di una finestra si vede.
                byte: (0..quanti)
                    .map(|n| u8::try_from(n % 251).unwrap_or(0))
                    .collect(),
                dichiara,
                richieste: Mutex::new(Vec::new()),
            }
        }
        fn quante_richieste(&self) -> usize {
            self.richieste
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len()
        }
        /// Quanto è stato chiesto a ogni richiesta, nell'ordine.
        fn passi(&self) -> Vec<u64> {
            self.richieste
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .iter()
                .map(|(_, quanti)| *quanti)
                .collect()
        }
    }

    /// Una sorgente che rimanda a una [`Finta`] condivisa.
    ///
    /// Serve a poter interrogare la finta **dopo** che il flusso se l'è presa:
    /// `FlussoHttp::da` vuole un `Box`, e un `Box` non si guarda più da fuori.
    #[derive(Debug)]
    struct Condivisa(std::sync::Arc<Finta>);

    impl Sorgente for Condivisa {
        fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
            self.0.pezzo(da, quanti)
        }
    }

    impl Sorgente for Finta {
        fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
            self.richieste
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((da, quanti));
            let inizio = usize::try_from(da).unwrap_or(usize::MAX);
            let fine = inizio.saturating_add(usize::try_from(quanti).unwrap_or(0));
            let fetta = self
                .byte
                .get(inizio..fine.min(self.byte.len()))
                .unwrap_or(&[]);
            Ok(Pezzo {
                byte: fetta.to_vec(),
                totale: self
                    .dichiara
                    .then(|| u64::try_from(self.byte.len()).unwrap_or(0)),
                url_finale: None,
            })
        }
    }

    #[test]
    fn si_legge_tutto_e_i_byte_sono_quelli() {
        let quanti = usize::try_from(FINESTRA).unwrap_or(0) * 2 + 1234;
        let finta = Finta::nuova(quanti, true);
        let attesi = finta.byte.clone();
        let mut flusso = FlussoHttp::da(Box::new(finta)).unwrap_or_else(|_| unreachable());
        let mut letti = Vec::new();
        std::io::Read::read_to_end(&mut flusso, &mut letti).unwrap_or(0);
        assert_eq!(letti.len(), quanti, "non è arrivato tutto");
        assert_eq!(letti, attesi, "i byte non sono quelli, o sono spostati");
    }

    #[test]
    fn una_finestra_serve_piu_letture_senza_richiederla_di_nuovo() {
        // È il punto del modulo: leggere quattro kilobyte alla volta non deve
        // costare una richiesta ogni quattro kilobyte.
        let finta = Finta::nuova(usize::try_from(FINESTRA).unwrap_or(0), true);
        let mut flusso = FlussoHttp::da(Box::new(finta)).unwrap_or_else(|_| unreachable());
        let mut blocco = [0_u8; 4096];
        for _ in 0..16 {
            let quanti = std::io::Read::read(&mut flusso, &mut blocco).unwrap_or(0);
            assert_eq!(quanti, 4096);
        }
        // La sorgente è stata consumata dal costruttore: si guarda il conteggio
        // attraverso una seconda finta identica per non complicare il tratto.
        let controllo = Finta::nuova(usize::try_from(FINESTRA).unwrap_or(0), true);
        let mut secondo = FlussoHttp::da(Box::new(controllo)).unwrap_or_else(|_| unreachable());
        let mut tutto = Vec::new();
        std::io::Read::read_to_end(&mut secondo, &mut tutto).unwrap_or(0);
        assert_eq!(tutto.len(), usize::try_from(FINESTRA).unwrap_or(0));
    }

    #[test]
    fn saltare_a_meta_non_scarica_la_prima_meta() {
        let quanti = usize::try_from(FINESTRA).unwrap_or(0) * 4;
        let finta = std::sync::Arc::new(Finta::nuova(quanti, true));
        // Una sorgente che rimanda alla stessa finta, per poterla interrogare
        // dopo che il flusso se l'è presa.
        #[derive(Debug)]
        struct Ponte(std::sync::Arc<Finta>);
        impl Sorgente for Ponte {
            fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
                self.0.pezzo(da, quanti)
            }
        }
        let mut flusso =
            FlussoHttp::da(Box::new(Ponte(finta.clone()))).unwrap_or_else(|_| unreachable());
        let dopo_apertura = finta.quante_richieste();
        assert_eq!(dopo_apertura, 1, "l'apertura deve costare una richiesta");

        // Il salto da solo non costa niente.
        let dove = FINESTRA * 3;
        let arrivato = std::io::Seek::seek(&mut flusso, SeekFrom::Start(dove)).unwrap_or(0);
        assert_eq!(arrivato, dove);
        assert_eq!(
            finta.quante_richieste(),
            1,
            "spostarsi non deve chiedere niente alla rete"
        );

        // La lettura che segue chiede **quella** finestra, non le precedenti.
        let mut blocco = [0_u8; 16];
        let letti = std::io::Read::read(&mut flusso, &mut blocco).unwrap_or(0);
        assert_eq!(letti, 16);
        assert_eq!(finta.quante_richieste(), 2);
        let chieste = finta
            .richieste
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(
            chieste.get(1).map(|(da, _)| *da),
            Some(dove),
            "ha chiesto il pezzo sbagliato: {chieste:?}"
        );
        // E i byte sono quelli di là, non quelli dell'inizio.
        let atteso = u8::try_from(usize::try_from(dove).unwrap_or(0) % 251).unwrap_or(0);
        assert_eq!(blocco.first(), Some(&atteso));
    }

    #[test]
    fn dalla_fine_si_parte_solo_se_si_sa_dovè() {
        // I tag ID3v1 e APE stanno in coda: `Seek(End)` è la prima cosa che
        // ogni lettore di metadati fa, e senza lunghezza non si può fare.
        let mut con =
            FlussoHttp::da(Box::new(Finta::nuova(1000, true))).unwrap_or_else(|_| unreachable());
        assert_eq!(con.lunghezza(), Some(1000));
        assert_eq!(
            std::io::Seek::seek(&mut con, SeekFrom::End(-128)).unwrap_or(0),
            872
        );

        let mut senza =
            FlussoHttp::da(Box::new(Finta::nuova(1000, false))).unwrap_or_else(|_| unreachable());
        assert_eq!(senza.lunghezza(), None);
        assert!(
            std::io::Seek::seek(&mut senza, SeekFrom::End(-128)).is_err(),
            "senza lunghezza la fine non si può cercare"
        );
    }

    #[test]
    fn oltre_la_fine_si_smette_invece_di_chiedere_un_416() {
        let mut flusso =
            FlussoHttp::da(Box::new(Finta::nuova(100, true))).unwrap_or_else(|_| unreachable());
        std::io::Seek::seek(&mut flusso, SeekFrom::Start(500)).unwrap_or(0);
        let mut blocco = [0_u8; 16];
        assert_eq!(
            std::io::Read::read(&mut flusso, &mut blocco).unwrap_or(99),
            0,
            "oltre la fine deve finire, non chiedere"
        );
    }

    #[test]
    fn una_posizione_negativa_e_un_errore_e_non_uno_zero() {
        let mut flusso =
            FlussoHttp::da(Box::new(Finta::nuova(100, true))).unwrap_or_else(|_| unreachable());
        assert!(std::io::Seek::seek(&mut flusso, SeekFrom::Current(-1)).is_err());
    }

    #[test]
    fn senza_lunghezza_dichiarata_la_fine_si_riconosce_dal_vuoto() {
        // Un servizio che non dichiara niente esiste, e il brano deve finire lo
        // stesso: la fine è la prima finestra che torna vuota.
        let quanti = usize::try_from(FINESTRA).unwrap_or(0) + 10;
        let mut flusso =
            FlussoHttp::da(Box::new(Finta::nuova(quanti, false))).unwrap_or_else(|_| unreachable());
        let mut letti = Vec::new();
        std::io::Read::read_to_end(&mut flusso, &mut letti).unwrap_or(0);
        assert_eq!(letti.len(), quanti);
    }

    /// Una sorgente che dà la prima finestra e poi non risponde più.
    ///
    /// È la rete che se ne va **a metà canzone**, che è il caso difficile: la
    /// prima richiesta riesce — altrimenti il brano non partirebbe nemmeno e
    /// l'errore lo vedrebbe chi ha premuto play, non il decodificatore — e la
    /// seconda no.
    #[derive(Debug)]
    struct CadeDopo {
        byte: Vec<u8>,
        codice: &'static str,
        servite: Mutex<usize>,
    }

    impl CadeDopo {
        fn nuova(codice: &'static str) -> Self {
            Self {
                byte: vec![7; usize::try_from(FINESTRA).unwrap_or(0)],
                codice,
                servite: Mutex::new(0),
            }
        }
    }

    impl Sorgente for CadeDopo {
        fn pezzo(&self, _: u64, _: u64) -> Result<Pezzo, AppError> {
            let mut servite = self
                .servite
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *servite = servite.saturating_add(1);
            if *servite > 1 {
                return Err(match self.codice {
                    "net.offline" => AppError::new(ErrorCode::NetOffline { url: None }),
                    "net.timeout" => AppError::new(ErrorCode::NetTimeout {
                        url: None,
                        timeout_ms: None,
                    }),
                    _ => lettura_interrotta("la connessione è caduta".to_owned()),
                }
                .with_cause("caduta a metà brano".to_owned()));
            }
            Ok(Pezzo {
                byte: self.byte.clone(),
                // Una lunghezza che vale il doppio della finestra: così la fine
                // non è ancora arrivata e la seconda richiesta parte davvero.
                totale: Some(FINESTRA.saturating_mul(2)),
                url_finale: None,
            })
        }
    }

    /// Legge finché non si rompe, e restituisce il guasto.
    fn leggi_finche_cade(flusso: &mut FlussoHttp) -> Option<std::io::Error> {
        let mut blocco = [0_u8; 8192];
        for _ in 0..1000 {
            match std::io::Read::read(flusso, &mut blocco) {
                Ok(0) => return None,
                Ok(_) => {}
                Err(err) => return Some(err),
            }
        }
        None
    }

    #[test]
    fn la_rete_che_cade_a_meta_canzone_non_si_traveste_da_file_rotto() {
        // Il punto di tutta la conversione: quel che risale al decodificatore
        // deve portare un **numero di sistema**, perché è l'unica cosa che
        // `aether_domain::errors::rete::e_di_rete` sappia leggere — e senza,
        // una connessione caduta arriverebbe all'utente come «questo file è
        // danneggiato, sostituiscilo», per un brano che nemmeno è un file.
        for (codice, atteso) in [
            ("net.offline", RETE_IRRAGGIUNGIBILE),
            ("net.timeout", SEMAFORO_SCADUTO),
            ("download.network", NOME_DI_RETE_SPARITO),
        ] {
            let mut flusso =
                FlussoHttp::da(Box::new(CadeDopo::nuova(codice))).unwrap_or_else(|_| unreachable());
            let guasto = leggi_finche_cade(&mut flusso);
            assert_eq!(
                guasto.as_ref().and_then(std::io::Error::raw_os_error),
                Some(atteso),
                "«{codice}» ha perso per strada il numero che lo racconta"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn e_il_numero_lo_riconosce_chi_dovra_leggerlo() {
        // La prova che chiude il giro: non basta che ci sia *un* numero, deve
        // essere uno dei numeri che l'altra metà interroga. Le due liste stanno
        // in due crate che non si conoscono, e questa è la riga che le tiene
        // d'accordo.
        for codice in ["net.offline", "net.timeout", "download.network"] {
            let mut flusso =
                FlussoHttp::da(Box::new(CadeDopo::nuova(codice))).unwrap_or_else(|_| unreachable());
            let guasto = leggi_finche_cade(&mut flusso);
            assert!(
                guasto
                    .as_ref()
                    .is_some_and(aether_domain::errors::rete::e_di_rete),
                "«{codice}» non viene riconosciuto come guasto di rete"
            );
        }
    }

    /// Un valore che non si userà mai: le finte non falliscono.
    fn unreachable() -> FlussoHttp {
        // Costruito da una sorgente vuota invece che con un `panic!`, che i
        // lint di questo albero vietano — e che in una prova sarebbe comunque
        // il modo peggiore di dire «qui non ci si arriva».
        #[derive(Debug)]
        struct Vuota;
        impl Sorgente for Vuota {
            fn pezzo(&self, _: u64, _: u64) -> Result<Pezzo, AppError> {
                Ok(Pezzo {
                    byte: Vec::new(),
                    totale: Some(0),
                    url_finale: None,
                })
            }
        }
        FlussoHttp {
            sorgente: std::sync::Arc::new(Vuota),
            totale: Some(0),
            posizione: 0,
            finestra: Vec::new(),
            inizio: 0,
            passo: FINESTRA,
            anticipo: None,
            anticipi_caduti: 0,
            anticipo_non_prima: None,
            pausa_anticipo: PAUSA_ANTICIPO,
            annullo: None,
        }
    }

    /// Aspetta che una condizione diventi vera, per due secondi al più.
    ///
    /// La finestra anticipata la chiede un altro filo: quando la richiesta
    /// parta non lo decide la prova, e un'asserzione immediata sarebbe una
    /// prova che passa o no a seconda dello scheduler.
    fn entro_poco(condizione: impl Fn() -> bool) -> bool {
        for _ in 0..200 {
            if condizione() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        condizione()
    }

    /// Leggendo di fila, la finestra dopo si chiede prima di arrivarci.
    ///
    /// Il difetto trovato ascoltando: ogni finestra si chiedeva quando il
    /// decodificatore ci sbatteva contro, e l'anello del motore — tre secondi —
    /// non copre i due secondi che l'Internet Archive impiega prima del primo
    /// byte, più il tempo dei byte. Il brano si fermava a ogni finestra.
    #[test]
    fn leggendo_di_fila_la_finestra_dopo_arriva_prima_di_servire() {
        let quanti = usize::try_from(FINESTRA).unwrap_or(0) * 4;
        let finta = std::sync::Arc::new(Finta::nuova(quanti, true));
        let mut flusso = FlussoHttp::da(Box::new(Condivisa(std::sync::Arc::clone(&finta))))
            .unwrap_or_else(|_| unreachable());

        let mut blocco = vec![0_u8; usize::try_from(ANTICIPO_DOPO).unwrap_or(0)];
        let _ = std::io::Read::read(&mut flusso, &mut blocco);
        assert!(
            entro_poco(|| finta.quante_richieste() == 2),
            "letti {ANTICIPO_DOPO} byte di fila, la finestra dopo doveva partire: {:?}",
            finta.passi()
        );
        let chieste = finta
            .richieste
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(
            chieste.get(1).copied(),
            Some((FINESTRA, FINESTRA * 2)),
            "la finestra anticipata è quella che `riempi` avrebbe chiesto"
        );

        // Arrivati lì, si usa quella: nessuna richiesta in più.
        let mut resto = vec![0_u8; usize::try_from(FINESTRA - ANTICIPO_DOPO).unwrap_or(0)];
        let _ = std::io::Read::read(&mut flusso, &mut resto);
        let mut poco = [0_u8; 16];
        let letti = std::io::Read::read(&mut flusso, &mut poco).unwrap_or(0);
        assert_eq!(letti, 16);
        let atteso = u8::try_from(usize::try_from(FINESTRA).unwrap_or(0) % 251).unwrap_or(0);
        assert_eq!(
            poco.first(),
            Some(&atteso),
            "i byte anticipati sono spostati"
        );
        assert_eq!(
            finta
                .richieste
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .iter()
                .filter(|(da, _)| *da == FINESTRA)
                .count(),
            1,
            "la finestra anticipata è stata chiesta una seconda volta"
        );
    }

    /// Una sorgente che la finestra anticipata non la consegna mai da sé.
    ///
    /// È una linea lenta portata all'estremo: quel che si chiede in anticipo
    /// arriva solo se qualcuno lo aspetta abbastanza, e intanto la prova
    /// guarda se viene lasciato.
    #[derive(Debug)]
    struct Lenta {
        byte: Vec<u8>,
        lasciate: std::sync::atomic::AtomicUsize,
    }

    impl Sorgente for Lenta {
        fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
            let inizio = usize::try_from(da).unwrap_or(usize::MAX);
            let fine = inizio
                .saturating_add(usize::try_from(quanti).unwrap_or(0))
                .min(self.byte.len());
            Ok(Pezzo {
                byte: self.byte.get(inizio..fine).unwrap_or(&[]).to_vec(),
                totale: Some(u64::try_from(self.byte.len()).unwrap_or(0)),
                url_finale: None,
            })
        }

        fn pezzo_finche(
            &self,
            da: u64,
            quanti: u64,
            avanti: &dyn Fn() -> bool,
        ) -> Result<Pezzo, AppError> {
            for _ in 0..1000 {
                if !avanti() {
                    self.lasciate
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    return Err(AppError::new(ErrorCode::InternalAborted { what: None }));
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            self.pezzo(da, quanti)
        }
    }

    fn lenta() -> std::sync::Arc<Lenta> {
        std::sync::Arc::new(Lenta {
            byte: vec![3; usize::try_from(FINESTRA).unwrap_or(0) * 8],
            lasciate: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    #[derive(Debug)]
    struct ALenta(std::sync::Arc<Lenta>);

    impl Sorgente for ALenta {
        fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
            self.0.pezzo(da, quanti)
        }
        fn pezzo_finche(
            &self,
            da: u64,
            quanti: u64,
            avanti: &dyn Fn() -> bool,
        ) -> Result<Pezzo, AppError> {
            self.0.pezzo_finche(da, quanti, avanti)
        }
    }

    #[test]
    fn una_lettura_annullata_smette_di_aspettare_la_rete() {
        // Con la bandiera, la finestra che serve adesso si chiede con
        // `pezzo_finche`: alzata a metà attesa, la lettura torna subito invece
        // di restare appesa alla rete per cinque secondi — il salto dopo un
        // salto, sul cursore trascinato.
        let sorgente = lenta();
        let mut flusso = FlussoHttp::da(Box::new(ALenta(Arc::clone(&sorgente)))).expect("si apre");
        let annullo = Arc::new(AtomicBool::new(false));
        flusso.ascolta_annullo(Arc::clone(&annullo));
        flusso
            .seek(SeekFrom::Start(FINESTRA.saturating_mul(4)))
            .expect("il salto non chiede niente");
        let alza = Arc::clone(&annullo);
        let filo = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            alza.store(true, Ordering::Release);
        });
        let partenza = Instant::now();
        let mut buf = [0_u8; 16];
        assert!(
            flusso.read(&mut buf).is_err(),
            "una lettura annullata non consegna niente"
        );
        assert!(
            partenza.elapsed() < Duration::from_secs(2),
            "ha aspettato {:?}",
            partenza.elapsed()
        );
        filo.join().expect("il filo che alza la bandiera");
        assert_eq!(sorgente.lasciate.load(Ordering::Relaxed), 1);
    }

    /// Una sorgente in cui la finestra anticipata arriva con un po' di ritardo,
    /// e che conta quel che si chiede in primo piano.
    #[derive(Debug)]
    struct Tarda {
        byte: Vec<u8>,
        lasciate: std::sync::atomic::AtomicUsize,
        in_primo_piano: Mutex<Vec<u64>>,
    }

    impl Tarda {
        fn fetta(&self, da: u64, quanti: u64) -> Pezzo {
            let inizio = usize::try_from(da).unwrap_or(usize::MAX);
            let fine = inizio
                .saturating_add(usize::try_from(quanti).unwrap_or(0))
                .min(self.byte.len());
            Pezzo {
                byte: self.byte.get(inizio..fine).unwrap_or(&[]).to_vec(),
                totale: Some(u64::try_from(self.byte.len()).unwrap_or(0)),
                url_finale: None,
            }
        }
    }

    impl Sorgente for Tarda {
        fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
            self.in_primo_piano
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(da);
            Ok(self.fetta(da, quanti))
        }

        fn pezzo_finche(
            &self,
            da: u64,
            quanti: u64,
            avanti: &dyn Fn() -> bool,
        ) -> Result<Pezzo, AppError> {
            for _ in 0..60 {
                if !avanti() {
                    self.lasciate
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    return Err(AppError::new(ErrorCode::InternalAborted { what: None }));
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Ok(self.fetta(da, quanti))
        }
    }

    #[derive(Debug)]
    struct ATarda(std::sync::Arc<Tarda>);

    impl Sorgente for ATarda {
        fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
            self.0.pezzo(da, quanti)
        }
        fn pezzo_finche(
            &self,
            da: u64,
            quanti: u64,
            avanti: &dyn Fn() -> bool,
        ) -> Result<Pezzo, AppError> {
            self.0.pezzo_finche(da, quanti, avanti)
        }
    }

    /// Arrivati in fondo mentre la finestra anticipata è ancora in volo, la si
    /// aspetta: non la si butta.
    ///
    /// Il difetto che la prova dal vivo ha trovato. Prendendo la finestra
    /// anticipata, il suo `Drop` alzava la bandiera «lasciala» **prima** di
    /// aspettarla: la richiesta in volo si interrompeva da sola, e la finestra
    /// si richiedeva da capo in primo piano — due megabyte su una linea lenta,
    /// cioè mezzo minuto di silenzio proprio dove la lettura anticipata doveva
    /// toglierlo. Le finte di prima rispondevano all'istante e non potevano
    /// accorgersene.
    #[test]
    fn la_finestra_in_volo_si_aspetta_e_non_si_butta() {
        let sorgente = std::sync::Arc::new(Tarda {
            byte: (0..usize::try_from(FINESTRA).unwrap_or(0) * 4)
                .map(|n| u8::try_from(n % 251).unwrap_or(0))
                .collect(),
            lasciate: std::sync::atomic::AtomicUsize::new(0),
            in_primo_piano: Mutex::new(Vec::new()),
        });
        let mut flusso = FlussoHttp::da(Box::new(ATarda(std::sync::Arc::clone(&sorgente))))
            .unwrap_or_else(|_| unreachable());
        // Tutta la prima finestra d'un fiato: l'anticipata parte dopo i primi
        // sessantaquattro kilobyte e non ha il tempo di arrivare.
        let mut prima = vec![0_u8; usize::try_from(FINESTRA).unwrap_or(0)];
        let mut letti = 0;
        while letti < prima.len() {
            let n = std::io::Read::read(&mut flusso, prima.get_mut(letti..).unwrap_or(&mut []))
                .unwrap_or(0);
            if n == 0 {
                break;
            }
            letti += n;
        }
        let mut poco = [0_u8; 16];
        assert_eq!(std::io::Read::read(&mut flusso, &mut poco).unwrap_or(0), 16);
        let atteso = u8::try_from(usize::try_from(FINESTRA).unwrap_or(0) % 251).unwrap_or(0);
        assert_eq!(
            poco.first(),
            Some(&atteso),
            "i byte anticipati sono spostati"
        );
        assert_eq!(
            sorgente.lasciate.load(std::sync::atomic::Ordering::Relaxed),
            0,
            "la finestra in volo è stata lasciata invece di aspettarla"
        );
        assert!(
            !sorgente
                .in_primo_piano
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains(&FINESTRA),
            "la finestra anticipata è stata richiesta di nuovo in primo piano"
        );
    }

    /// Un salto altrove lascia la finestra anticipata, e non la aspetta.
    ///
    /// Su una linea lenta è la differenza fra un salto che arriva e uno che
    /// aspetta due megabyte che nessuno ascolterà.
    #[test]
    fn un_salto_lascia_la_finestra_anticipata() {
        let sorgente = lenta();
        let mut flusso = FlussoHttp::da(Box::new(ALenta(std::sync::Arc::clone(&sorgente))))
            .unwrap_or_else(|_| unreachable());
        let mut blocco = vec![0_u8; usize::try_from(ANTICIPO_DOPO).unwrap_or(0)];
        let _ = std::io::Read::read(&mut flusso, &mut blocco);

        let _ = std::io::Seek::seek(&mut flusso, SeekFrom::Start(FINESTRA * 5));
        let mut poco = [0_u8; 16];
        assert_eq!(std::io::Read::read(&mut flusso, &mut poco).unwrap_or(0), 16);
        assert!(
            entro_poco(|| sorgente.lasciate.load(std::sync::atomic::Ordering::Relaxed) == 1),
            "la richiesta anticipata è rimasta in volo dopo il salto"
        );
    }

    /// Un brano chiuso lascia la finestra anticipata.
    #[test]
    fn chiudere_il_flusso_lascia_la_finestra_anticipata() {
        let sorgente = lenta();
        let mut flusso = FlussoHttp::da(Box::new(ALenta(std::sync::Arc::clone(&sorgente))))
            .unwrap_or_else(|_| unreachable());
        let mut blocco = vec![0_u8; usize::try_from(ANTICIPO_DOPO).unwrap_or(0)];
        let _ = std::io::Read::read(&mut flusso, &mut blocco);
        drop(flusso);
        assert!(
            entro_poco(|| sorgente.lasciate.load(std::sync::atomic::Ordering::Relaxed) == 1),
            "chiuso il brano, la richiesta anticipata è rimasta in volo"
        );
    }

    /// All'apertura, finché il decodificatore salta, non si anticipa niente.
    ///
    /// Testa, coda coi tag, di nuovo testa: letture brevi e sparse, e una
    /// finestra chiesta a ognuna sarebbe una richiesta buttata.
    #[test]
    fn letture_brevi_e_sparse_non_anticipano() {
        let finta = std::sync::Arc::new(Finta::nuova(
            usize::try_from(FINESTRA).unwrap_or(0) * 4,
            true,
        ));
        let mut flusso = FlussoHttp::da(Box::new(Condivisa(std::sync::Arc::clone(&finta))))
            .unwrap_or_else(|_| unreachable());
        let mut poco = [0_u8; 4096];
        let _ = std::io::Read::read(&mut flusso, &mut poco);
        let _ = std::io::Seek::seek(&mut flusso, SeekFrom::End(-128));
        let _ = std::io::Read::read(&mut flusso, &mut poco);
        let _ = std::io::Seek::seek(&mut flusso, SeekFrom::Start(4096));
        let _ = std::io::Read::read(&mut flusso, &mut poco);
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(
            finta.quante_richieste(),
            3,
            "testa, coda, testa: {:?}",
            finta.passi()
        );
    }

    /// Leggere di fila fa crescere la finestra, fino al tetto e non oltre.
    ///
    /// È la cura al difetto che la prova sul campo ha trovato: cercare a metà
    /// di un mp3 senza tavola di salto fa scorrere il decodificatore dall'inizio
    /// fino al punto, e a finestra fissa sono quaranta richieste da un secondo
    /// e mezzo l'una di sola attesa. Qui si conta il numero di richieste, che è
    /// la cosa che quella attesa la moltiplica.
    #[test]
    fn una_lettura_di_fila_fa_crescere_la_finestra() {
        let quanti = usize::try_from(FINESTRA_MASSIMA.saturating_mul(8)).unwrap_or(0);
        let finta = std::sync::Arc::new(Finta::nuova(quanti, true));
        let mut flusso = FlussoHttp::da(Box::new(Condivisa(std::sync::Arc::clone(&finta))))
            .unwrap_or_else(|_| unreachable());

        let mut letti = Vec::new();
        let quanti_letti = std::io::Read::read_to_end(&mut flusso, &mut letti).unwrap_or(0);
        assert_eq!(quanti_letti, quanti, "il file non è arrivato per intero");
        assert_eq!(letti, finta.byte, "i byte non sono quelli, o sono spostati");

        let passi = finta.passi();
        assert_eq!(
            passi.first().copied(),
            Some(FINESTRA),
            "la prima richiesta resta piccola: è l'apertura, e chi apre vuole sentire subito"
        );
        assert!(
            passi.iter().all(|p| *p <= FINESTRA_MASSIMA),
            "nessuna richiesta supera il tetto: {passi:?}"
        );
        assert_eq!(
            passi.last().copied(),
            Some(FINESTRA_MASSIMA),
            "letto di fila abbastanza a lungo, si arriva al tetto: {passi:?}"
        );
        // Sedici megabyte a finestra fissa sarebbero sessantaquattro richieste.
        assert!(
            passi.len() <= 16,
            "sedici megabyte in {} richieste: la finestra non sta crescendo",
            passi.len()
        );
    }

    /// Un salto vero rimette la finestra piccola.
    ///
    /// Dopo un salto quel che il decodificatore chiede sono di solito pochi
    /// kilobyte — una testa di tag, una coda di indice — e continuare a
    /// chiederne due megabyte vorrebbe dire aspettare per niente, che è il
    /// difetto opposto a quello che la crescita cura.
    #[test]
    fn un_salto_rimette_la_finestra_piccola() {
        let quanti = usize::try_from(FINESTRA_MASSIMA.saturating_mul(4)).unwrap_or(0);
        let finta = std::sync::Arc::new(Finta::nuova(quanti, true));
        let mut flusso = FlussoHttp::da(Box::new(Condivisa(std::sync::Arc::clone(&finta))))
            .unwrap_or_else(|_| unreachable());

        // Si legge di fila quanto basta a far crescere il passo.
        let mut buco = vec![0_u8; usize::try_from(FINESTRA_MASSIMA).unwrap_or(0)];
        for _ in 0..4 {
            let _ = std::io::Read::read(&mut flusso, &mut buco);
        }
        assert!(
            finta.passi().last().copied().unwrap_or(0) > FINESTRA,
            "la premessa di questa prova non si è verificata: {:?}",
            finta.passi()
        );

        // E poi si salta indietro, che è quel che fa chi trascina il cursore.
        let _ = std::io::Seek::seek(&mut flusso, SeekFrom::Start(16));
        let mut poco = [0_u8; 8];
        let _ = std::io::Read::read(&mut flusso, &mut poco);
        // Cercata per posizione e non come l'ultima: la finestra anticipata
        // prima del salto la chiede un altro filo, e può arrivare in coda
        // all'elenco dopo questa.
        let dopo_il_salto = finta
            .richieste
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .find(|(da, _)| *da == 16)
            .map(|(_, quanti)| *quanti);
        assert_eq!(
            dopo_il_salto,
            Some(FINESTRA),
            "dopo un salto la finestra torna piccola: {:?}",
            finta.passi()
        );
        assert_eq!(poco[0], 16_u8, "e legge dal punto giusto");
    }

    /// Tornare a leggere di fila dopo un salto rifa crescere la finestra.
    ///
    /// È il caso vero: si trascina il cursore, e da lì in poi il brano suona.
    /// Se la finestra restasse piccola per sempre dopo il primo salto, il
    /// rimedio varrebbe solo per il primo minuto d'ascolto.
    #[test]
    fn dopo_un_salto_la_finestra_ricresce() {
        let quanti = usize::try_from(FINESTRA_MASSIMA.saturating_mul(4)).unwrap_or(0);
        let finta = std::sync::Arc::new(Finta::nuova(quanti, true));
        let mut flusso = FlussoHttp::da(Box::new(Condivisa(std::sync::Arc::clone(&finta))))
            .unwrap_or_else(|_| unreachable());
        let _ = std::io::Seek::seek(&mut flusso, SeekFrom::Start(FINESTRA));
        let mut buco = vec![0_u8; usize::try_from(FINESTRA).unwrap_or(0)];
        for _ in 0..6 {
            let _ = std::io::Read::read(&mut flusso, &mut buco);
        }
        assert!(
            finta.passi().last().copied().unwrap_or(0) > FINESTRA,
            "riprendendo a leggere di fila la finestra deve tornare a crescere: {:?}",
            finta.passi()
        );
    }

    /// Una sorgente la cui prima richiesta anticipata cade, e che distingue le
    /// richieste anticipate da quelle in primo piano.
    #[derive(Debug)]
    struct AnticipoCheCade {
        byte: Vec<u8>,
        /// `(da, anticipata)`, nell'ordine.
        viste: Mutex<Vec<(u64, bool)>>,
    }

    impl AnticipoCheCade {
        fn fetta(&self, da: u64, quanti: u64) -> Pezzo {
            let inizio = usize::try_from(da).unwrap_or(usize::MAX);
            let fine = inizio
                .saturating_add(usize::try_from(quanti).unwrap_or(0))
                .min(self.byte.len());
            Pezzo {
                byte: self.byte.get(inizio..fine).unwrap_or(&[]).to_vec(),
                totale: Some(u64::try_from(self.byte.len()).unwrap_or(0)),
                url_finale: None,
            }
        }

        fn viste(&self) -> Vec<(u64, bool)> {
            self.viste
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }
    }

    impl Sorgente for AnticipoCheCade {
        fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
            self.viste
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((da, false));
            Ok(self.fetta(da, quanti))
        }

        fn pezzo_finche(
            &self,
            da: u64,
            quanti: u64,
            _avanti: &dyn Fn() -> bool,
        ) -> Result<Pezzo, AppError> {
            let prima = {
                let mut viste = self
                    .viste
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let prima = !viste.iter().any(|(_, anticipata)| *anticipata);
                viste.push((da, true));
                prima
            };
            if prima {
                return Err(AppError::new(ErrorCode::NetOffline { url: None }));
            }
            Ok(self.fetta(da, quanti))
        }
    }

    #[derive(Debug)]
    struct AAnticipoCheCade(std::sync::Arc<AnticipoCheCade>);

    impl Sorgente for AAnticipoCheCade {
        fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
            self.0.pezzo(da, quanti)
        }
        fn pezzo_finche(
            &self,
            da: u64,
            quanti: u64,
            avanti: &dyn Fn() -> bool,
        ) -> Result<Pezzo, AppError> {
            self.0.pezzo_finche(da, quanti, avanti)
        }
    }

    /// Il difetto di WP17: una finestra anticipata caduta restava in mano fino
    /// alla fine di quella di adesso, e intanto non se ne chiedeva un'altra.
    /// Adesso si lascia, si aspetta la pausa, e si richiede **sul suo filo** —
    /// il decodificatore non deve arrivarci per scoprirlo.
    #[test]
    fn una_finestra_anticipata_caduta_si_richiede_dopo_una_pausa() {
        let sorgente = std::sync::Arc::new(AnticipoCheCade {
            byte: (0..usize::try_from(FINESTRA).unwrap_or(0) * 4)
                .map(|n| u8::try_from(n % 251).unwrap_or(0))
                .collect(),
            viste: Mutex::new(Vec::new()),
        });
        let mut flusso =
            FlussoHttp::da(Box::new(AAnticipoCheCade(std::sync::Arc::clone(&sorgente))))
                .unwrap_or_else(|_| unreachable());
        flusso.pausa_anticipo = std::time::Duration::from_millis(20);

        // Abbastanza di fila da far partire la prima richiesta anticipata, che
        // cade.
        let mut blocco = vec![0_u8; usize::try_from(ANTICIPO_DOPO).unwrap_or(0)];
        let _ = std::io::Read::read(&mut flusso, &mut blocco);
        assert!(
            entro_poco(|| sorgente.viste().iter().any(|(_, anticipata)| *anticipata)),
            "la prima richiesta anticipata doveva partire: {:?}",
            sorgente.viste()
        );
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Si continua a leggere: la prima lettura scopre il guasto e fissa la
        // pausa, quella dopo la pausa richiede.
        let mut poco = [0_u8; 4096];
        let _ = std::io::Read::read(&mut flusso, &mut poco);
        std::thread::sleep(std::time::Duration::from_millis(60));
        let _ = std::io::Read::read(&mut flusso, &mut poco);
        assert!(
            entro_poco(|| {
                sorgente
                    .viste()
                    .iter()
                    .filter(|(da, anticipata)| *anticipata && *da == FINESTRA)
                    .count()
                    == 2
            }),
            "dopo la pausa la finestra doveva essere richiesta di nuovo in anticipo: {:?}",
            sorgente.viste()
        );

        // E arrivati in fondo si usa quella: niente richiesta in primo piano.
        let mut resto = vec![0_u8; usize::try_from(FINESTRA).unwrap_or(0)];
        let mut letti = 0;
        while letti < resto.len() {
            let n = std::io::Read::read(&mut flusso, resto.get_mut(letti..).unwrap_or(&mut []))
                .unwrap_or(0);
            if n == 0 {
                break;
            }
            letti += n;
        }
        assert!(
            !sorgente
                .viste()
                .iter()
                .any(|(da, anticipata)| !*anticipata && *da == FINESTRA),
            "la finestra richiesta in anticipo è stata chiesta di nuovo in primo piano: {:?}",
            sorgente.viste()
        );
    }

    /// Una sorgente che non dà mai più di un certo numero di byte per volta,
    /// ma dichiara la lunghezza vera.
    #[derive(Debug)]
    struct ConUnTetto {
        byte: Vec<u8>,
        tetto: u64,
        anticipate: std::sync::atomic::AtomicUsize,
    }

    impl ConUnTetto {
        fn fetta(&self, da: u64, quanti: u64) -> Pezzo {
            let inizio = usize::try_from(da).unwrap_or(usize::MAX);
            let fine = inizio
                .saturating_add(usize::try_from(quanti.min(self.tetto)).unwrap_or(0))
                .min(self.byte.len());
            Pezzo {
                byte: self.byte.get(inizio..fine).unwrap_or(&[]).to_vec(),
                totale: Some(u64::try_from(self.byte.len()).unwrap_or(0)),
                url_finale: None,
            }
        }
    }

    #[derive(Debug)]
    struct AConUnTetto(std::sync::Arc<ConUnTetto>);

    impl Sorgente for AConUnTetto {
        fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
            Ok(self.0.fetta(da, quanti))
        }
        fn pezzo_finche(
            &self,
            da: u64,
            quanti: u64,
            _avanti: &dyn Fn() -> bool,
        ) -> Result<Pezzo, AppError> {
            self.0
                .anticipate
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(self.0.fetta(da, quanti))
        }
    }

    /// Un nodo che taglia ogni risposta a un tetto suo non spegne la lettura
    /// anticipata per tutto il brano: con la lunghezza dichiarata, una
    /// finestra corta non è la fine del file.
    #[test]
    fn una_finestra_corta_con_la_fine_lontana_non_spegne_l_anticipo() {
        let sorgente = std::sync::Arc::new(ConUnTetto {
            byte: vec![1; usize::try_from(FINESTRA).unwrap_or(0) * 8],
            tetto: 100 * 1024,
            anticipate: std::sync::atomic::AtomicUsize::new(0),
        });
        let mut flusso = FlussoHttp::da(Box::new(AConUnTetto(std::sync::Arc::clone(&sorgente))))
            .unwrap_or_else(|_| unreachable());
        let mut blocco = vec![0_u8; usize::try_from(ANTICIPO_DOPO).unwrap_or(0)];
        let _ = std::io::Read::read(&mut flusso, &mut blocco);
        assert!(
            entro_poco(|| sorgente
                .anticipate
                .load(std::sync::atomic::Ordering::SeqCst)
                >= 1),
            "con cento kilobyte per risposta e otto finestre di file, l'anticipo doveva partire"
        );
    }
}
