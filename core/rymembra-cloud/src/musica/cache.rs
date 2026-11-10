//! La cache su disco dei brani del cloud.
//!
//! # A cosa serve
//!
//! A tre cose. Un brano già sentito riparte senza aspettare la rete; un salto
//! indietro non richiede quel che è già passato; e un brano ascoltato fino in
//! fondo lascia una **copia completa** su cui l'analisi misura volume e
//! impronta senza scaricare niente apposta (vedi il piano, «mentre suonano»).
//! I cataloghi non passano mai di qui: `rymembra_net::flusso` resta senza
//! disco, e la promessa di `TERMS.md` § 2 con lui.
//!
//! # La forma su disco
//!
//! Per ogni brano due file, col nome che viene dall'impronta della chiave
//! (`conto`, `id`, `versione`), così un nome di file non finisce in una
//! cartella che chiunque può sfogliare:
//!
//! - `<impronta>.dati`: i byte arrivati, **uno dopo l'altro nell'ordine in cui
//!   sono arrivati**, non nella posizione che hanno nel brano;
//! - `<impronta>.indice`: un JSON piccolo che dice quale pezzo del brano sta in
//!   quale punto di `.dati` (i [`Segmento`]), quanto è lungo il brano, quando è
//!   stato usato.
//!
//! # Perché i byte in fila e non al loro posto
//!
//! Perché su NTFS un file scritto a un punto lontano dalla fine — la coda di
//! un brano, che il decodificatore legge spesso per prima — si allunga fino a
//! lì, e gli zeri in mezzo occupano il disco davvero: un file «sparso» lo è
//! solo se lo si chiede con una chiamata che qui non si può fare (`unsafe` è
//! vietato in tutto il workspace). Allora una lettura della coda di un FLAC da
//! quaranta megabyte ne avrebbe occupati quaranta, e il tetto avrebbe contato
//! sedici kilobyte. In fila, quel che sta su disco è esattamente quel che è
//! arrivato, e il tetto conta il vero.
//!
//! Il prezzo è che una copia in fila non è un file audio. Quando l'ultimo
//! buco si chiude, se i pezzi non sono arrivati nell'ordine del brano (la coda
//! letta per prima, un salto indietro), la si **ricompone** — una copia in
//! ordine, scritta accanto e rinominata sopra — e da lì `.dati` è il brano,
//! leggibile come un file qualunque: è quello che [`Cache::copia_completa`] dà
//! all'analisi. Un brano letto di fila dalla testa è già in ordine, e non si
//! ricompone: vedi `fondi_contigui`.
//!
//! Prima di rinominare la copia in ordine si cancella l'indice vecchio, e
//! quello nuovo si scrive dopo. Un'interruzione in mezzo lascia un `.dati`
//! che nessun indice nomina, e la riapertura lo butta. Nell'ordine opposto
//! poteva restare l'indice dei pezzi in fila sopra il file già in ordine, e
//! la cache avrebbe servito i byte di un punto del brano al posto di un altro.
//!
//! # Scritture atomiche, e l'ordine
//!
//! Prima i byte, poi `sync_data`, poi l'indice — scritto in un file accanto e
//! rinominato sopra quello vecchio. Un'interruzione fra le due lascia byte in
//! più in fondo a `.dati` che l'indice non conosce (si sovrascrivono alla
//! scrittura dopo), mai un indice che promette byte che non ci sono.
//!
//! L'indice non si riscrive a ogni pezzo: ogni [`OGNI_QUANTI_SALVARE`] byte
//! nuovi, quando la copia si completa, quando si chiude il flusso che la
//! scriveva e, per quel che resta, quando si chiude la cache. Un'interruzione
//! costa al più quegli ultimi byte, che si riscaricano: l'indice ne sa meno
//! del disco, mai di più.
//!
//! # Chi aspetta chi
//!
//! Il lucchetto della cache tiene solo i conti: quali copie, quali segmenti,
//! quanti byte. I byte si leggono e si scrivono fuori, così un pezzo da due
//! megabyte che si scrive su un disco lento non ferma la lettura di un altro
//! brano (quello dopo, che si prepara; lo streaming di «Ascolta insieme»;
//! l'analisi). Per non pestarsi i piedi:
//!
//! - chi scrive **prenota** sotto il lucchetto i buchi che riempirà e il posto
//!   in `.dati` dove li metterà; chi nel frattempo chiede gli stessi byte li
//!   prende dalla rete, e non li scrive una seconda volta;
//! - un segmento entra nell'elenco solo quando i suoi byte sono scritti: chi
//!   legge non trova mai un posto prenotato e ancora vuoto;
//! - una copia in uso (letta, scritta, salvata) non si cancella: la si segna
//!   come tolta, non la si legge e non la si scrive più, e i suoi file se ne
//!   vanno con l'ultimo che la usa;
//! - `sync_data`, l'indice e la ricomposizione li fa il filo della cache
//!   («rymembra-cache»), non chi sta suonando. La ricomposizione aspetta che
//!   nessuno usi la copia prima di rinominare il file, e in quell'attimo chi
//!   chiede quei byte va alla rete.
//!
//! # Mai un guasto per colpa della cache
//!
//! La cache è un acceleratore. Un disco pieno, un file che non si apre, un
//! indice illeggibile: il pezzo passa diretto dalla rete e la voce della cache
//! si butta. Chi ascolta non deve mai sentire un errore che nasce qui.
//!
//! # Il tetto, e chi resta
//!
//! Quando i byte superano il tetto se ne va il brano usato meno di recente.
//! Una copia completa **ancora da misurare** resta finché non è misurata
//! ([`Cache::segna_misurata`]) o finché la cache non supera il doppio del
//! tetto: senza, in un ascolto continuo la misura arriverebbe sempre dopo la
//! pulizia. Così, finché le copie complete non sono misurate, la cache può
//! restare sopra il tetto, fino al doppio. Il segno «da misurare» sta
//! nell'indice: una misura che la chiusura dell'applicazione ha interrotto si
//! richiede alla riapertura ([`Cache::da_misurare`]), e la copia non resta
//! trattenuta per sempre.
//!
//! Un conto scollegato porta via le sue copie ([`Cache::togli_conto`]), anche
//! quella del brano che sta suonando in quel momento.
//!
//! Un brano più lungo del doppio del tetto (un'immagine CD, un hi-res) non
//! entra affatto: passa diretto, come uno senza versione. Entrando avrebbe
//! buttato tutte le altre copie — quella che si sta scrivendo non si tocca —
//! e occupato il disco ben oltre il tetto scelto, per poi essere buttato a sua
//! volta al primo pezzo del brano dopo, dalla regola del doppio: né un
//! riascolto né una misura ne avrebbero avuto niente.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use rymembra_app::files::io_error;
use rymembra_domain::errors::AppError;
use rymembra_domain::nuvola::VoceRemota;
use rymembra_net::flusso::Sorgente;
use rymembra_net::http::Pezzo;
use rymembra_oauth::adesso_ms;
use serde::{Deserialize, Serialize};

/// Il tetto di serie: duecento megabyte, una quarantina di brani in MP3 o
/// una decina in FLAC. Lo si cambia dalle impostazioni.
pub const TETTO_DI_SERIE: u64 = 200 * 1024 * 1024;

/// Ogni quanti byte nuovi di una copia si riscrive il suo indice (oltre che
/// al completamento e alla chiusura del flusso). Quattro megabyte: al più
/// tanto da riscaricare dopo un'interruzione, e un `sync_data` ogni due
/// finestre grandi invece che a ogni finestra.
const OGNI_QUANTI_SALVARE: u64 = 4 * 1024 * 1024;

/// Quanto la ricomposizione aspetta che una copia smetta di essere in uso.
/// Chi la usa la tiene per una lettura o una scrittura sola, e da quando la
/// copia è chiusa non comincia nessuno: basta largamente. Se non basta, la
/// copia resta in fila e si ricompone alla prossima apertura della cache.
const ATTESA_PER_RICOMPORRE: Duration = Duration::from_secs(5);

/// Quanto «Svuota» e «Scollega» aspettano che le copie in uso in quel momento
/// se ne vadano davvero, per tornare con i file già cancellati. Se non basta
/// (una ricomposizione lunga), se ne vanno appena finisce l'uso.
const ATTESA_PER_TOGLIERE: Duration = Duration::from_secs(2);

/// L'estensione dei byte.
const DATI: &str = "dati";

/// L'estensione dell'indice.
const INDICE: &str = "indice";

/// L'estensione di quel che si sta scrivendo e non è ancora al suo posto.
const PROVVISORIO: &str = "tmp";

/// Quale copia di quale brano.
///
/// La versione è nella chiave: un brano cambiato sul servizio è un'altra
/// chiave, e la copia vecchia non si legge mai al posto della nuova.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Chiave {
    /// Il conto.
    pub conto: String,
    /// L'identificativo del file sul servizio.
    pub id: String,
    /// La versione del contenuto ([`VoceRemota::versione`]).
    pub versione: String,
}

impl Chiave {
    /// La chiave di una voce di un conto. `None` se il servizio non ha detto
    /// una versione: senza, una copia vecchia non si distinguerebbe da una
    /// nuova, e quel brano passa diretto.
    #[must_use]
    pub fn di(conto: &str, voce: &VoceRemota) -> Option<Self> {
        Some(Self {
            conto: conto.to_owned(),
            id: voce.id.clone(),
            versione: voce.versione.clone()?,
        })
    }

    /// Il nome dei file di questa copia: l'impronta della chiave.
    fn nome(&self) -> String {
        let mut impronta = blake3::Hasher::new();
        for parte in [&self.conto, &self.id, &self.versione] {
            impronta.update(parte.as_bytes());
            // Un separatore che nessuna delle tre parti può contenere: senza,
            // («ab», «c») e («a», «bc») sarebbero la stessa impronta.
            impronta.update(&[0]);
        }
        impronta.finalize().to_hex().chars().take(32).collect()
    }

    /// Lo stesso file del servizio, forse in un'altra versione.
    fn stesso_file(&self, altra: &Self) -> bool {
        self.conto == altra.conto && self.id == altra.id
    }
}

/// Un pezzo del brano su disco: i byte `[da, fino)` del brano stanno in
/// `.dati` a partire da `pos`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Segmento {
    da: u64,
    fino: u64,
    pos: u64,
}

impl Segmento {
    fn lunghezza(&self) -> u64 {
        self.fino.saturating_sub(self.da)
    }
}

/// L'indice di una copia, com'è su disco.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Indice {
    conto: String,
    id: String,
    versione: String,
    totale: Option<u64>,
    segmenti: Vec<Segmento>,
    usato_ms: i64,
    da_misurare: bool,
}

/// Una copia, in memoria.
#[derive(Debug)]
struct Copia {
    chiave: Chiave,
    totale: Option<u64>,
    /// In ordine di `da`, senza sovrapposizioni. Solo byte già scritti.
    segmenti: Vec<Segmento>,
    /// I pezzi del brano che qualcuno sta scrivendo adesso: prenotati e non
    /// ancora nei segmenti. Vedi «Chi aspetta chi» nel `//!`.
    in_volo: Vec<(u64, u64)>,
    /// Dove scrivere il prossimo pezzo in `.dati`: dopo l'ultimo byte di un
    /// segmento o di una prenotazione.
    fine_dati: u64,
    /// Per il meno usato di recente: l'orologio, e un contatore per chi è
    /// stato usato nello stesso millisecondo.
    usato: (i64, u64),
    /// Completa e non ancora misurata.
    da_misurare: bool,
    /// Quanti la stanno usando adesso, fuori dal lucchetto: chi legge, chi
    /// scrive, chi salva, chi ricompone.
    in_uso: u32,
    /// Tolta mentre era in uso: se ne va con l'ultimo che la usa, e intanto
    /// non si legge e non si scrive.
    tolta: bool,
    /// La ricomposizione sta per rinominare `.dati`: nessuno comincia a usarla.
    chiusa: bool,
    /// Coperta, e affidata al filo che la ricompone.
    ricomponendo: bool,
    /// L'indice su disco è rimasto indietro.
    sporca: bool,
    /// I byte scritti dopo l'ultimo indice salvato.
    non_salvati: u64,
    /// Un salvataggio è in corso.
    salvando: bool,
    /// E ne è stato chiesto un altro mentre era in corso.
    da_risalvare: bool,
}

impl Copia {
    fn nuova(chiave: Chiave, totale: Option<u64>) -> Self {
        Self {
            chiave,
            totale,
            segmenti: Vec::new(),
            in_volo: Vec::new(),
            fine_dati: 0,
            usato: (0, 0),
            da_misurare: false,
            in_uso: 0,
            tolta: false,
            chiusa: false,
            ricomponendo: false,
            sporca: false,
            non_salvati: 0,
            salvando: false,
            da_risalvare: false,
        }
    }

    /// Si può cominciare a usarla?
    fn usabile(&self) -> bool {
        !self.tolta && !self.chiusa
    }

    /// Tutto il brano è su disco, ricomposto in ordine.
    fn completa(&self) -> bool {
        match (self.totale, self.segmenti.as_slice()) {
            (Some(totale), [unico]) => unico.da == 0 && unico.fino == totale && unico.pos == 0,
            _ => false,
        }
    }

    /// Tutto il brano è su disco, in qualunque ordine.
    fn coperta(&self) -> bool {
        let Some(totale) = self.totale else {
            return false;
        };
        let mut fin_qui = 0;
        for segmento in &self.segmenti {
            if segmento.da > fin_qui {
                return false;
            }
            fin_qui = fin_qui.max(segmento.fino);
        }
        fin_qui >= totale
    }

    /// Il primo segmento che comincia dopo `dove`.
    fn prossimo_dopo(&self, dove: u64) -> Option<u64> {
        self.segmenti.iter().map(|s| s.da).find(|da| *da > dove)
    }

    /// Dove stanno in `.dati` i byte di `[da, fino)`: i tratti `(pos,
    /// quanti)` di fila da `da`, finché i segmenti li coprono, e il punto del
    /// brano a cui arrivano.
    fn tratti(&self, da: u64, fino: u64) -> (Vec<(u64, u64)>, u64) {
        let mut tratti = Vec::new();
        let mut cursore = da;
        while cursore < fino {
            let Some(segmento) = segmento_di(&self.segmenti, cursore) else {
                break;
            };
            let fine = segmento.fino.min(fino);
            tratti.push((
                segmento.pos.saturating_add(cursore - segmento.da),
                fine - cursore,
            ));
            cursore = fine;
        }
        (tratti, cursore)
    }

    /// Le parti di `[da, fino)` che nessuno ha scritto né sta scrivendo.
    fn da_scrivere(&self, da: u64, fino: u64) -> Vec<(u64, u64)> {
        let mut presi: Vec<(u64, u64)> = self
            .segmenti
            .iter()
            .map(|s| (s.da, s.fino))
            .chain(self.in_volo.iter().copied())
            .collect();
        presi.sort_unstable();
        buchi(&presi, da, fino)
    }

    fn indice(&self) -> Indice {
        Indice {
            conto: self.chiave.conto.clone(),
            id: self.chiave.id.clone(),
            versione: self.chiave.versione.clone(),
            totale: self.totale,
            segmenti: self.segmenti.clone(),
            usato_ms: self.usato.0,
            da_misurare: self.da_misurare,
        }
    }
}

/// Quel che il lucchetto protegge.
#[derive(Debug)]
struct Stato {
    copie: HashMap<String, Copia>,
    tetto: u64,
    attiva: bool,
    /// I byte su disco: la somma dei `fine_dati`.
    usati: u64,
    /// Quanti di quei byte sono di copie tolte e ancora in uso: se ne vanno
    /// fra un attimo, e il tetto non li conta più.
    in_uscita: u64,
    /// Il contatore degli usi, per [`Copia::usato`].
    contatore: u64,
    /// L'epoca dei conti tolti in questa sessione: vedi [`Cache::togli_conto`].
    /// Un conto che non c'è vale zero.
    tolti: HashMap<String, u64>,
}

impl Stato {
    /// I byte che contano per il tetto: senza quelli delle copie già tolte.
    fn vivi(&self) -> u64 {
        self.usati.saturating_sub(self.in_uscita)
    }

    /// L'epoca di un conto: quante volte [`Cache::togli_conto`] l'ha tolto in
    /// questa sessione.
    fn epoca(&self, conto: &str) -> u64 {
        self.tolti.get(conto).copied().unwrap_or(0)
    }

    /// Un uso in più, per il meno usato di recente.
    fn adesso(&mut self) -> (i64, u64) {
        self.contatore = self.contatore.wrapping_add(1);
        (adesso_ms(), self.contatore)
    }
}

/// Chi avvisare quando una copia diventa completa.
type Avviso = Arc<dyn Fn(&Chiave) + Send + Sync>;

/// Quel che fa il filo della cache.
#[derive(Debug)]
enum Lavoro {
    /// `sync_data` e l'indice di una copia.
    Salva(String),
    /// Riscrivere in ordine una copia coperta.
    Ricomponi(String),
    /// Per le prove: risponde quando ha finito quel che c'era in coda prima.
    #[cfg(test)]
    Segnale(mpsc::SyncSender<()>),
}

/// Quel che la cache condivide col suo filo.
struct Dentro {
    cartella: PathBuf,
    stato: Mutex<Stato>,
    /// Suona ogni volta che una copia smette di essere in uso: la
    /// ricomposizione aspetta qui che nessuno la tocchi.
    libera: Condvar,
    avviso: Mutex<Option<Avviso>>,
    /// La coda del filo. `None` se il filo non è partito o la cache si sta
    /// chiudendo: allora un lavoro lo fa subito chi lo chiede.
    lavori: Mutex<Option<mpsc::Sender<Lavoro>>>,
}

/// La cache: una cartella, e quel che ci sta dentro. Vedi il `//!`.
pub struct Cache {
    dentro: Arc<Dentro>,
    /// Il filo «rymembra-cache», che alla chiusura si aspetta.
    filo: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cache")
            .field("cartella", &self.dentro.cartella)
            .finish_non_exhaustive()
    }
}

/// Cosa sa dire il disco di una richiesta.
enum DalDisco {
    /// Il pezzo è già pronto: vuoto, oltre la fine del brano.
    Pezzo(Pezzo),
    /// Si legge da `.dati`, uno dopo l'altro, questi tratti `(pos, quanti)`:
    /// il pezzo avrà questa lunghezza del brano. La copia resta in uso finché
    /// la lettura non si chiude con [`Dentro::letto`].
    Leggi {
        tratti: Vec<(u64, u64)>,
        totale: Option<u64>,
    },
    /// Va chiesto alla rete: `quanti` byte da dove si era chiesto.
    Manca(u64),
}

impl Cache {
    /// Apre la cache in `cartella`, creandola se serve, e ritrova le copie
    /// rimaste dall'ultima volta.
    ///
    /// Quel che non torna — un `.dati` senza indice, un indice che promette
    /// più byte di quelli che ci sono, un file a metà di una scrittura — si
    /// butta: è una cache, e il peggio che costa è riscaricarlo. Una copia
    /// che l'ultima volta si è coperta e non ha fatto in tempo a ricomporsi
    /// si ricompone adesso, dal filo.
    ///
    /// # Errori
    ///
    /// `fs.*` se la cartella non si crea o non si legge.
    pub fn apri(
        cartella: impl Into<PathBuf>,
        tetto: u64,
        attiva: bool,
    ) -> Result<Arc<Self>, AppError> {
        let cartella = cartella.into();
        let nome_cartella = cartella.to_string_lossy().into_owned();
        std::fs::create_dir_all(&cartella).map_err(|err| io_error(&nome_cartella, &err))?;
        let mut copie = HashMap::new();
        let mut usati = 0_u64;
        let voci = std::fs::read_dir(&cartella).map_err(|err| io_error(&nome_cartella, &err))?;
        let mut da_buttare = Vec::new();
        for voce in voci.flatten() {
            let percorso = voce.path();
            let (Some(nome), Some(estensione)) = (
                percorso
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(ToOwned::to_owned),
                percorso
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(ToOwned::to_owned),
            ) else {
                continue;
            };
            match estensione.as_str() {
                INDICE => match ritrova(&cartella, &nome) {
                    Some(copia) => {
                        usati = usati.saturating_add(copia.fine_dati);
                        copie.insert(nome, copia);
                    }
                    None => da_buttare.push(nome),
                },
                PROVVISORIO => {
                    let _ = std::fs::remove_file(&percorso);
                }
                _ => {}
            }
        }
        for nome in da_buttare {
            butta_file(&cartella, &nome);
        }
        // Un `.dati` che nessun indice nomina è byte che nessuno ritroverà.
        if let Ok(voci) = std::fs::read_dir(&cartella) {
            for voce in voci.flatten() {
                let percorso = voce.path();
                let orfano = percorso.extension().and_then(|e| e.to_str()) == Some(DATI)
                    && percorso
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .is_none_or(|nome| !copie.contains_key(nome));
                if orfano {
                    let _ = std::fs::remove_file(&percorso);
                }
            }
        }
        let dentro = Arc::new(Dentro {
            cartella,
            stato: Mutex::new(Stato {
                copie,
                tetto,
                attiva,
                usati,
                in_uscita: 0,
                contatore: 0,
                tolti: HashMap::new(),
            }),
            libera: Condvar::new(),
            avviso: Mutex::new(None),
            lavori: Mutex::new(None),
        });
        let (coda, arrivi) = mpsc::channel::<Lavoro>();
        let filo = std::thread::Builder::new()
            .name("rymembra-cache".to_owned())
            .spawn({
                let dentro = Arc::clone(&dentro);
                move || {
                    // Finisce quando la cache chiude la coda, dopo l'ultimo
                    // lavoro che c'era.
                    for lavoro in arrivi {
                        dentro.fai(lavoro);
                    }
                }
            })
            // Senza filo la cache funziona lo stesso: i lavori li fa chi li
            // chiede, come prima che il filo ci fosse.
            .ok();
        if filo.is_some() {
            *dentro.lavori.lock().unwrap_or_else(PoisonError::into_inner) = Some(coda);
        }
        let da_ricomporre: Vec<String> = {
            let mut guardia = dentro.blocca();
            let stato = &mut *guardia;
            dentro.fai_posto(stato, None);
            stato
                .copie
                .iter_mut()
                .filter(|(_, copia)| copia.coperta() && !copia.completa())
                .map(|(nome, copia)| {
                    copia.ricomponendo = true;
                    nome.clone()
                })
                .collect()
        };
        for nome in da_ricomporre {
            dentro.manda(Lavoro::Ricomponi(nome));
        }
        Ok(Arc::new(Self { dentro, filo }))
    }

    /// Una sorgente che passa da questa cache. Senza chiave (un servizio che
    /// non ha detto la versione) passa diretta.
    ///
    /// Le copie di altre versioni dello stesso file si buttano qui: non le
    /// leggerà più nessuno.
    ///
    /// La sorgente ricorda l'epoca del suo conto: se il conto si scollega
    /// mentre suona, non scrive più (vedi [`Self::togli_conto`]).
    #[must_use]
    pub fn con(self: &Arc<Self>, chiave: Option<Chiave>, sorgente: Box<dyn Sorgente>) -> ConCache {
        let mut epoca = 0;
        if let Some(chiave) = &chiave {
            let mut guardia = self.dentro.blocca();
            let stato = &mut *guardia;
            epoca = stato.epoca(&chiave.conto);
            let vecchie: Vec<String> = stato
                .copie
                .iter()
                .filter(|(_, copia)| copia.chiave.stesso_file(chiave) && copia.chiave != *chiave)
                .map(|(nome, _)| nome.clone())
                .collect();
            for nome in vecchie {
                self.dentro.togli(stato, &nome);
            }
        }
        ConCache {
            cache: Arc::clone(self),
            chiave,
            epoca,
            sorgente,
        }
    }

    /// Quanti byte usa, il tetto, e se è accesa.
    ///
    /// I byte di una copia tolta mentre qualcuno la leggeva non si contano:
    /// se ne vanno appena finisce quella lettura.
    #[must_use]
    pub fn stato(&self) -> (u64, u64, bool) {
        let stato = self.dentro.blocca();
        (stato.vivi(), stato.tetto, stato.attiva)
    }

    /// Cambia il tetto e l'interruttore.
    ///
    /// Spenta, la cache non legge e non scrive: le sorgenti passano dirette.
    /// Quel che c'è resta su disco finché qualcuno non chiama [`Self::svuota`].
    pub fn imposta(&self, tetto: u64, attiva: bool) {
        let mut guardia = self.dentro.blocca();
        let stato = &mut *guardia;
        stato.tetto = tetto;
        stato.attiva = attiva;
        self.dentro.fai_posto(stato, None);
    }

    /// Butta tutto.
    ///
    /// Le copie in uso in quel momento (il pezzo che si sta leggendo o
    /// scrivendo, l'indice che si salva) si segnano come tolte, e i loro file
    /// se ne vanno appena l'uso finisce: si aspetta quell'attimo, fino a
    /// [`ATTESA_PER_TOGLIERE`], e intanto il lucchetto è libero.
    ///
    /// # Errori
    ///
    /// `fs.*` se la cartella non si legge; i file che non si cancellano si
    /// lasciano, e la prossima apertura li riconosce come orfani.
    pub fn svuota(&self) -> Result<(), AppError> {
        let dentro = &self.dentro;
        let mut guardia = dentro.blocca();
        let nomi: Vec<String> = guardia.copie.keys().cloned().collect();
        for nome in nomi {
            dentro.togli(&mut guardia, &nome);
        }
        let mut guardia = dentro.aspetta_le_tolte(guardia, |_| true);
        // Il lucchetto resta in mano mentre si spazza la cartella: è un gesto
        // raro, cancellare è svelto, e così nessuno crea nel frattempo una
        // copia nuova i cui file finirebbero nel mucchio.
        let stato = &mut *guardia;
        let nome_cartella = dentro.cartella.to_string_lossy().into_owned();
        let voci =
            std::fs::read_dir(&dentro.cartella).map_err(|err| io_error(&nome_cartella, &err))?;
        for voce in voci.flatten() {
            let percorso = voce.path();
            let in_uso = percorso
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.split('.').next())
                .is_some_and(|nome| stato.copie.contains_key(nome));
            if !in_uso
                && matches!(
                    percorso.extension().and_then(|e| e.to_str()),
                    Some(DATI | INDICE | PROVVISORIO)
                )
            {
                let _ = std::fs::remove_file(&percorso);
            }
        }
        // Restano solo le copie ancora in uso, tutte tolte: il conto si rifà
        // da capo, per non portarsi dietro nessuno scarto.
        stato.usati = stato.copie.values().map(|copia| copia.fine_dati).sum();
        stato.in_uscita = stato.usati;
        Ok(())
    }

    /// Il file di una copia completa, da leggere come un file locale.
    ///
    /// `None` se la copia non c'è, non è completa, o la cache è spenta: con la
    /// cache spenta il volume viene solo dai tag, e lo si dice così.
    #[must_use]
    pub fn copia_completa(&self, chiave: &Chiave) -> Option<PathBuf> {
        let stato = self.dentro.blocca();
        if !stato.attiva {
            return None;
        }
        let nome = chiave.nome();
        let copia = stato.copie.get(&nome)?;
        (copia.usabile() && copia.completa()).then(|| percorso(&self.dentro.cartella, &nome, DATI))
    }

    /// Le copie complete ancora da misurare, dalla meno usata di recente.
    ///
    /// Il segno sta nell'indice e sopravvive a una chiusura, ma l'avviso di
    /// [`Self::al_completamento`] parte una volta sola, quando la copia si
    /// completa: una misura che l'applicazione chiusa non ha fatto in tempo a
    /// fare — in coda, o a metà — non la richiederebbe più nessuno, e la copia
    /// resterebbe trattenuta per sempre, fino al doppio del tetto, buttando
    /// per prime quelle utili. All'avvio chi misura le chiede qui.
    #[must_use]
    pub fn da_misurare(&self) -> Vec<Chiave> {
        let stato = self.dentro.blocca();
        let mut copie: Vec<&Copia> = stato
            .copie
            .values()
            .filter(|copia| copia.da_misurare && copia.usabile() && copia.completa())
            .collect();
        copie.sort_by_key(|copia| copia.usato);
        copie
            .into_iter()
            .map(|copia| copia.chiave.clone())
            .collect()
    }

    /// Toglie tutte le copie di un conto, dalla memoria e dal disco, e dice
    /// quante: è la metà di «Scollega» che riguarda i brani (`PRIVACY.md`
    /// promette che un conto scollegato non lascia brani in cache).
    ///
    /// # Un brano del conto che sta suonando
    ///
    /// Il suo flusso può continuare dopo «Scollega»: un pezzo già in volo, o
    /// il resto di un brano di OneDrive letto da un indirizzo firmato, che non
    /// chiede più il permesso. Scritto qui, ricreerebbe la copia appena tolta.
    /// Per questo il conto cambia epoca, e una [`ConCache`] nata prima non
    /// scrive più niente: legge dalla rete e basta. Un flusso nuovo — il conto
    /// ricollegato — nasce con l'epoca nuova, e torna a scrivere.
    ///
    /// Una copia in uso proprio in quel momento (un pezzo che si legge, un
    /// indice che si salva) se ne va appena l'uso finisce: si aspetta
    /// quell'attimo, come in [`Self::svuota`].
    pub fn togli_conto(&self, conto: &str) -> usize {
        let mut guardia = self.dentro.blocca();
        let stato = &mut *guardia;
        let epoca = stato.tolti.entry(conto.to_owned()).or_insert(0);
        *epoca = epoca.wrapping_add(1);
        let nomi: Vec<String> = stato
            .copie
            .iter()
            .filter(|(_, copia)| copia.chiave.conto == conto && !copia.tolta)
            .map(|(nome, _)| nome.clone())
            .collect();
        for nome in &nomi {
            self.dentro.togli(stato, nome);
        }
        drop(
            self.dentro
                .aspetta_le_tolte(guardia, |copia| copia.chiave.conto == conto),
        );
        nomi.len()
    }

    /// La copia è stata misurata: da qui in poi se ne va come le altre.
    pub fn segna_misurata(&self, chiave: &Chiave) {
        let nome = chiave.nome();
        {
            let mut stato = self.dentro.blocca();
            let Some(copia) = stato.copie.get_mut(&nome) else {
                return;
            };
            if !copia.da_misurare {
                return;
            }
            copia.da_misurare = false;
            copia.sporca = true;
        }
        // Subito, da chi ha misurato: il segno deve arrivare all'indice prima
        // che la copia possa andarsene, o alla riapertura tornerebbe.
        self.dentro.salva(&nome);
        let mut guardia = self.dentro.blocca();
        self.dentro.fai_posto(&mut guardia, None);
    }

    /// Chi chiamare quando una copia diventa completa: il filo dell'analisi,
    /// che così misura mentre la copia c'è.
    ///
    /// Si chiama fuori dal lucchetto della cache: dal filo che ha scritto
    /// l'ultimo pezzo, o dal filo della cache dopo una ricomposizione. Deve
    /// tornare presto.
    pub fn al_completamento(&self, avviso: impl Fn(&Chiave) + Send + Sync + 'static) {
        *self
            .dentro
            .avviso
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(avviso));
    }
}

impl Drop for Cache {
    fn drop(&mut self) {
        // Chiusa la coda, il filo finisce quel che c'era e torna; da qui in
        // poi un lavoro lo fa chi lo chiede.
        drop(
            self.dentro
                .lavori
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        );
        if let Some(filo) = self.filo.take() {
            // Il filo non tiene la cache, solo quel che c'è dentro: non può
            // essere lui a chiuderla. Per scrupolo, non si aspetta da solo.
            if filo.thread().id() != std::thread::current().id() {
                let _ = filo.join();
            }
        }
        // Gli indici rimasti indietro: senza, una chiusura perderebbe gli
        // ultimi pezzi di ogni brano a metà.
        let sporche: Vec<String> = self
            .dentro
            .blocca()
            .copie
            .iter()
            .filter(|(_, copia)| copia.sporca)
            .map(|(nome, _)| nome.clone())
            .collect();
        for nome in sporche {
            self.dentro.salva(&nome);
        }
    }
}

impl Dentro {
    fn blocca(&self) -> MutexGuard<'_, Stato> {
        self.stato.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Aspetta, fino a [`ATTESA_PER_TOGLIERE`] e col lucchetto libero, che
    /// le copie tolte scelte da `quali` finiscano di essere usate e se ne
    /// vadano davvero.
    fn aspetta_le_tolte<'a>(
        &self,
        guardia: MutexGuard<'a, Stato>,
        quali: impl Fn(&Copia) -> bool,
    ) -> MutexGuard<'a, Stato> {
        self.libera
            .wait_timeout_while(guardia, ATTESA_PER_TOGLIERE, |stato| {
                stato
                    .copie
                    .values()
                    .any(|copia| copia.tolta && quali(copia))
            })
            .unwrap_or_else(PoisonError::into_inner)
            .0
    }

    /// La cache è accesa?
    fn attiva(&self) -> bool {
        self.blocca().attiva
    }

    /// Affida un lavoro al filo. Senza filo, o col filo andato, lo fa subito
    /// chi chiama: mai col lucchetto della cache in mano.
    fn manda(&self, lavoro: Lavoro) {
        let rimasto = match self
            .lavori
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            Some(coda) => coda.send(lavoro).err().map(|rifiutato| rifiutato.0),
            None => Some(lavoro),
        };
        if let Some(lavoro) = rimasto {
            self.fai(lavoro);
        }
    }

    /// Fa un lavoro: dal filo, o da chi l'ha chiesto se il filo non c'è.
    fn fai(&self, lavoro: Lavoro) {
        match lavoro {
            Lavoro::Salva(nome) => self.salva(&nome),
            Lavoro::Ricomponi(nome) => self.ricomponi(&nome),
            #[cfg(test)]
            Lavoro::Segnale(risposta) => {
                let _ = risposta.send(());
            }
        }
    }

    /// Chi avvisare, chiamato fuori dal lucchetto.
    fn avvisa(&self, chiave: &Chiave) {
        let avviso = self
            .avviso
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(avviso) = avviso {
            avviso(chiave);
        }
    }

    /// Quel che il disco sa dare di `[da, da + quanti)`. Con [`DalDisco::Leggi`]
    /// la copia resta in uso: chi legge la chiude con [`Self::letto`].
    fn dal_disco(&self, nome: &str, da: u64, quanti: u64) -> DalDisco {
        let mut guardia = self.blocca();
        let stato = &mut *guardia;
        let Some(copia) = stato.copie.get_mut(nome).filter(|copia| copia.usabile()) else {
            return DalDisco::Manca(quanti);
        };
        let totale = copia.totale;
        let fino = match totale {
            Some(totale) if da >= totale => {
                return DalDisco::Pezzo(Pezzo {
                    byte: Vec::new(),
                    totale: Some(totale),
                    url_finale: None,
                });
            }
            Some(totale) => da.saturating_add(quanti).min(totale),
            None => da.saturating_add(quanti),
        };
        let (tratti, fin_qui) = copia.tratti(da, fino);
        // Con la lunghezza nota un pezzo corto non inganna nessuno, e basta
        // l'inizio. Senza, direbbe «fine del file» a chi legge: si serve dal
        // disco solo una richiesta coperta per intero.
        let servibile = match totale {
            Some(_) => fin_qui > da,
            None => fin_qui >= fino,
        };
        if !servibile {
            if totale.is_none() {
                return DalDisco::Manca(quanti);
            }
            // Dalla rete solo fino al prossimo pezzo che c'è già.
            let fino = copia
                .prossimo_dopo(da)
                .map_or(fino, |prossimo| prossimo.min(fino));
            return DalDisco::Manca(fino - da);
        }
        copia.in_uso = copia.in_uso.saturating_add(1);
        let usato = stato.adesso();
        if let Some(copia) = stato.copie.get_mut(nome) {
            copia.usato = usato;
        }
        DalDisco::Leggi { tratti, totale }
    }

    /// Chiude una lettura cominciata con [`Self::dal_disco`]. Se il disco non
    /// ha dato quel che l'indice prometteva, la copia non è affidabile e si
    /// butta.
    fn letto(&self, nome: &str, riuscita: bool) {
        let mut guardia = self.blocca();
        let stato = &mut *guardia;
        self.rilascia(stato, nome);
        if !riuscita {
            self.togli(stato, nome);
        }
    }

    /// Mette su disco un pezzo arrivato dalla rete, se il conto della sorgente
    /// è ancora quello di quando è nata (`epoca`: vedi [`Cache::togli_conto`]).
    fn scrivi(&self, chiave: &Chiave, epoca: u64, da: u64, pezzo: &Pezzo) {
        let nome = chiave.nome();
        let Some(prenotati) = self.prenota(chiave, &nome, epoca, da, pezzo) else {
            return;
        };
        // I byte fuori dal lucchetto: vedi «Chi aspetta chi».
        let scritti = scrivi_fuori(
            &percorso(&self.cartella, &nome, DATI),
            da,
            pezzo,
            &prenotati,
        )
        .is_ok();
        let (lavori, completata) = self.registra(&nome, &prenotati, scritti);
        for lavoro in lavori {
            self.manda(lavoro);
        }
        if completata {
            self.avvisa(chiave);
        }
    }

    /// La prima metà di [`Self::scrivi`], sotto il lucchetto: i buchi che
    /// questo pezzo riempie, e il posto dove andranno in `.dati`. `None` se
    /// non c'è niente da scrivere; altrimenti la copia resta in uso fino a
    /// [`Self::registra`].
    fn prenota(
        &self,
        chiave: &Chiave,
        nome: &str,
        epoca: u64,
        da: u64,
        pezzo: &Pezzo,
    ) -> Option<Vec<Segmento>> {
        let mut guardia = self.blocca();
        let stato = &mut *guardia;
        if !stato.attiva || pezzo.byte.is_empty() || stato.epoca(&chiave.conto) != epoca {
            return None;
        }
        let usato = stato.adesso();
        // Un brano che adesso dice un'altra lunghezza sotto la stessa versione
        // non è quello di prima: si ricomincia.
        if stato.copie.get(nome).is_some_and(|c| {
            c.totale.is_some() && pezzo.totale.is_some() && c.totale != pezzo.totale
        }) {
            self.togli(stato, nome);
        }
        // Oltre il doppio del tetto non si comincia nemmeno: vedi il `//!`. Il
        // controllo sta prima della voce, così un brano che non entra non ne
        // crea una vuota (e non cerca file da cancellare) a ogni pezzo. Solo
        // con la lunghezza nota: senza, non si sa dove arriverà.
        let totale = stato
            .copie
            .get(nome)
            .and_then(|c| c.totale)
            .or(pezzo.totale);
        if totale.is_some_and(|totale| totale > stato.tetto.saturating_mul(2)) {
            if stato.copie.contains_key(nome) {
                self.togli(stato, nome);
            }
            return None;
        }
        let fino = da.saturating_add(u64::try_from(pezzo.byte.len()).ok()?);
        let copia = stato
            .copie
            .entry(nome.to_owned())
            .or_insert_with(|| Copia::nuova(chiave.clone(), pezzo.totale));
        // Tolta e ancora in uso (magari proprio qui sopra), o in
        // ricomposizione: questo pezzo passa e basta.
        if !copia.usabile() {
            return None;
        }
        if copia.totale.is_none() {
            copia.totale = pezzo.totale;
        }
        copia.usato = usato;
        let buchi = copia.da_scrivere(da, fino);
        if buchi.is_empty() {
            return None;
        }
        let mut prenotati = Vec::with_capacity(buchi.len());
        let mut aggiunti = 0_u64;
        for (inizio, fine) in buchi {
            prenotati.push(Segmento {
                da: inizio,
                fino: fine,
                pos: copia.fine_dati,
            });
            copia.in_volo.push((inizio, fine));
            // Nel conto subito, insieme a `fine_dati`: se la scrittura cade a
            // metà la copia si butta, e dal conto esce il suo `fine_dati`,
            // che deve essere esattamente quel che ci era entrato.
            copia.fine_dati = copia.fine_dati.saturating_add(fine - inizio);
            aggiunti = aggiunti.saturating_add(fine - inizio);
        }
        copia.in_uso = copia.in_uso.saturating_add(1);
        stato.usati = stato.usati.saturating_add(aggiunti);
        Some(prenotati)
    }

    /// La seconda metà di [`Self::scrivi`], di nuovo sotto il lucchetto: i
    /// segmenti scritti entrano nell'elenco, e la copia smette di essere in
    /// uso. Dice i lavori da affidare al filo, e se la copia si è appena
    /// completata già in ordine (chi chiama avvisa).
    fn registra(&self, nome: &str, prenotati: &[Segmento], scritti: bool) -> (Vec<Lavoro>, bool) {
        let mut guardia = self.blocca();
        let stato = &mut *guardia;
        let mut lavori = Vec::new();
        let mut completata = false;
        if let Some(copia) = stato.copie.get_mut(nome) {
            copia
                .in_volo
                .retain(|volo| !prenotati.iter().any(|p| (p.da, p.fino) == *volo));
            if scritti && !copia.tolta {
                // Coperta già prima di questi segmenti: non è una copia che si
                // completa adesso, e non si avvisa una seconda volta.
                let coperta_prima = copia.coperta();
                copia.segmenti.extend_from_slice(prenotati);
                copia.segmenti.sort_by_key(|s| s.da);
                fondi_contigui(&mut copia.segmenti);
                let nuovi: u64 = prenotati.iter().map(Segmento::lunghezza).sum();
                copia.non_salvati = copia.non_salvati.saturating_add(nuovi);
                copia.sporca = true;
                if !coperta_prima && copia.coperta() {
                    // Arrivata di fila dalla testa — in un pezzo solo, o a
                    // finestre una dopo l'altra come la chiede un flusso che
                    // suona — è già un segmento solo in ordine (vedi
                    // [`fondi_contigui`]). Solo chi ha letto prima la coda, o
                    // è saltato avanti e poi indietro, lascia pezzi fuori
                    // posto: quella va ricomposta, dal filo, prima di essere un
                    // file che l'analisi sa aprire.
                    if copia.completa() {
                        copia.da_misurare = true;
                        completata = true;
                        lavori.push(Lavoro::Salva(nome.to_owned()));
                    } else if !copia.ricomponendo {
                        copia.ricomponendo = true;
                        lavori.push(Lavoro::Ricomponi(nome.to_owned()));
                    }
                } else if copia.non_salvati >= OGNI_QUANTI_SALVARE {
                    lavori.push(Lavoro::Salva(nome.to_owned()));
                }
            }
        }
        self.rilascia(stato, nome);
        if scritti {
            self.fai_posto(stato, Some(nome));
        } else {
            // Il disco ha detto di no da qualche parte: la copia si butta.
            self.togli(stato, nome);
        }
        (lavori, completata)
    }

    /// `sync_data` dei byte, poi l'indice: vedi «Scritture atomiche, e
    /// l'ordine». Un salvataggio alla volta per copia: uno chiesto mentre un
    /// altro è in corso lo rifà quello in corso, con quel che c'è di nuovo.
    /// Se il disco dice di no, la copia si butta.
    fn salva(&self, nome: &str) {
        let mut guardia = self.blocca();
        {
            let Some(copia) = guardia.copie.get_mut(nome) else {
                return;
            };
            if copia.salvando || copia.chiusa {
                // Lo rifà chi salva adesso, o la ricomposizione quando ha
                // finito, che salva comunque.
                copia.da_risalvare = true;
                return;
            }
            copia.salvando = true;
        }
        loop {
            // Quel che si salva in questo giro, preso sotto il lucchetto: i
            // segmenti di adesso, tutti già scritti.
            let giro = guardia.copie.get_mut(nome).and_then(|copia| {
                copia.da_risalvare = false;
                if !copia.sporca || !copia.usabile() {
                    return None;
                }
                copia.sporca = false;
                let byte_nuovi = std::mem::take(&mut copia.non_salvati) > 0;
                copia.in_uso = copia.in_uso.saturating_add(1);
                Some((copia.indice(), byte_nuovi))
            });
            let Some((indice, byte_nuovi)) = giro else {
                break;
            };
            drop(guardia);
            let riuscito = salva_su_disco(&self.cartella, nome, &indice, byte_nuovi).is_ok();
            guardia = self.blocca();
            let stato = &mut *guardia;
            if !riuscito && let Some(copia) = stato.copie.get_mut(nome) {
                copia.salvando = false;
            }
            self.rilascia(stato, nome);
            if !riuscito {
                self.togli(stato, nome);
                return;
            }
            let ancora = stato
                .copie
                .get(nome)
                .is_some_and(|copia| copia.da_risalvare && copia.usabile());
            if !ancora {
                break;
            }
        }
        if let Some(copia) = guardia.copie.get_mut(nome) {
            copia.salvando = false;
        }
    }

    /// Riscrive in ordine una copia coperta, dal filo della cache: vedi il
    /// `//!`, e «Chi aspetta chi».
    fn ricomponi(&self, nome: &str) {
        // Quel che c'è, preso sotto il lucchetto. Coperta vuol dire che non
        // resta niente da scrivere: i segmenti non cambiano più, e la copia
        // si legge mentre la si riscrive.
        let presa = {
            let mut stato = self.blocca();
            stato.copie.get_mut(nome).and_then(|copia| {
                let totale = copia
                    .totale
                    .filter(|_| copia.usabile() && copia.coperta() && !copia.completa());
                let Some(totale) = totale else {
                    copia.ricomponendo = false;
                    return None;
                };
                copia.in_uso = copia.in_uso.saturating_add(1);
                Some((copia.segmenti.clone(), totale, copia.chiave.clone()))
            })
        };
        let Some((segmenti, totale, chiave)) = presa else {
            return;
        };
        let provvisorio = percorso(&self.cartella, nome, PROVVISORIO);
        let scritta = in_ordine(
            &percorso(&self.cartella, nome, DATI),
            &segmenti,
            totale,
            &provvisorio,
        )
        .is_ok();

        // Da qui nessuno comincia a usarla, e si aspetta chi la sta usando.
        let mut guardia = self.blocca();
        self.rilascia(&mut guardia, nome);
        let chiusa = scritta
            && match guardia.copie.get_mut(nome) {
                Some(copia) if copia.usabile() => {
                    copia.chiusa = true;
                    true
                }
                _ => false,
            };
        if !chiusa {
            let stato = &mut *guardia;
            if let Some(copia) = stato.copie.get_mut(nome) {
                copia.ricomponendo = false;
            }
            if !scritta {
                self.togli(stato, nome);
            }
            drop(guardia);
            let _ = std::fs::remove_file(&provvisorio);
            return;
        }
        let (mut guardia, _) = self
            .libera
            .wait_timeout_while(guardia, ATTESA_PER_RICOMPORRE, |stato| {
                stato
                    .copie
                    .get(nome)
                    .is_some_and(|copia| copia.chiusa && copia.in_uso > 0)
            })
            .unwrap_or_else(PoisonError::into_inner);
        // Ancora `chiusa`: è la stessa copia di prima, non una nata nel
        // frattempo con lo stesso nome dopo che quella era stata tolta.
        let libera = match guardia.copie.get_mut(nome) {
            Some(copia) if copia.chiusa && copia.in_uso == 0 => {
                // In uso da parte di chi rinomina: un `togli`, nel frattempo,
                // la segna e basta, e i file restano dove sono.
                copia.in_uso = 1;
                true
            }
            Some(copia) if copia.chiusa => {
                // Qualcuno non la lascia: resta in fila, e la si ricompone
                // alla prossima apertura.
                copia.chiusa = false;
                copia.ricomponendo = false;
                false
            }
            _ => false,
        };
        drop(guardia);
        if !libera {
            let _ = std::fs::remove_file(&provvisorio);
            return;
        }
        let rimpiazzata = rimpiazza(&self.cartella, nome, &provvisorio).is_ok();

        let mut guardia = self.blocca();
        let stato = &mut *guardia;
        let mut fatta = false;
        if let Some(copia) = stato.copie.get_mut(nome) {
            copia.chiusa = false;
            copia.ricomponendo = false;
            if rimpiazzata && !copia.tolta {
                // Ricomposta, la copia pesa quanto il brano: di solito quel che
                // pesava già, ma il conto dei byte deve seguire il disco e non
                // la speranza.
                let prima = copia.fine_dati;
                copia.segmenti = vec![Segmento {
                    da: 0,
                    fino: totale,
                    pos: 0,
                }];
                copia.fine_dati = totale;
                copia.da_misurare = true;
                copia.sporca = true;
                copia.non_salvati = 0;
                stato.usati = stato.usati.saturating_sub(prima).saturating_add(totale);
                fatta = true;
            }
        }
        self.rilascia(stato, nome);
        if !rimpiazzata {
            self.togli(stato, nome);
        }
        drop(guardia);
        if !rimpiazzata {
            let _ = std::fs::remove_file(&provvisorio);
        }
        if fatta {
            self.salva(nome);
            self.avvisa(&chiave);
        }
    }

    /// Un uso della copia è finito. L'ultimo uso di una copia tolta se la
    /// porta via.
    fn rilascia(&self, stato: &mut Stato, nome: &str) {
        let Some(copia) = stato.copie.get_mut(nome) else {
            return;
        };
        copia.in_uso = copia.in_uso.saturating_sub(1);
        if copia.in_uso == 0 {
            let tolta = copia.tolta;
            self.libera.notify_all();
            if tolta {
                self.togli(stato, nome);
            }
        }
    }

    /// Butta via copie finché si sta sotto il tetto. `tenere` non si tocca:
    /// è quella che si sta scrivendo.
    fn fai_posto(&self, stato: &mut Stato, tenere: Option<&str>) {
        loop {
            let vivi = stato.vivi();
            if vivi <= stato.tetto {
                return;
            }
            let oltre_il_doppio = vivi > stato.tetto.saturating_mul(2);
            let vittima = stato
                .copie
                .iter()
                .filter(|(nome, copia)| {
                    !copia.tolta
                        && Some(nome.as_str()) != tenere
                        && (!copia.da_misurare || oltre_il_doppio)
                })
                // Prima quelle già misurate o mai complete; fra quelle, la
                // meno usata di recente.
                .min_by_key(|(_, copia)| (copia.da_misurare, copia.usato))
                .map(|(nome, _)| nome.clone());
            let Some(vittima) = vittima else {
                return;
            };
            self.togli(stato, &vittima);
        }
    }

    /// Toglie una copia: dalla memoria, dal conto dei byte e dal disco. Una
    /// copia in uso si segna soltanto, e se ne va con l'ultimo che la usa
    /// (vedi [`Self::rilascia`]).
    fn togli(&self, stato: &mut Stato, nome: &str) {
        let Some(copia) = stato.copie.get_mut(nome) else {
            return;
        };
        if copia.in_uso > 0 {
            if !copia.tolta {
                copia.tolta = true;
                stato.in_uscita = stato.in_uscita.saturating_add(copia.fine_dati);
            }
            return;
        }
        if let Some(copia) = stato.copie.remove(nome) {
            stato.usati = stato.usati.saturating_sub(copia.fine_dati);
            if copia.tolta {
                stato.in_uscita = stato.in_uscita.saturating_sub(copia.fine_dati);
            }
        }
        butta_file(&self.cartella, nome);
    }

    /// Il flusso che scriveva una copia si chiude: l'indice, se è rimasto
    /// indietro, lo salva il filo.
    fn salva_presto(&self, nome: &str) {
        let sporca = self
            .blocca()
            .copie
            .get(nome)
            .is_some_and(|copia| copia.sporca && copia.usabile());
        if sporca {
            self.manda(Lavoro::Salva(nome.to_owned()));
        }
    }
}

/// Una sorgente che passa dalla cache. Vedi il `//!`.
#[derive(Debug)]
pub struct ConCache {
    cache: Arc<Cache>,
    chiave: Option<Chiave>,
    /// L'epoca del conto quando la sorgente è nata: vedi [`Cache::togli_conto`].
    epoca: u64,
    sorgente: Box<dyn Sorgente>,
}

impl Sorgente for ConCache {
    fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
        self.pezzo_finche(da, quanti, &|| true)
    }

    fn pezzo_finche(
        &self,
        da: u64,
        quanti: u64,
        avanti: &dyn Fn() -> bool,
    ) -> Result<Pezzo, AppError> {
        let dentro = &self.cache.dentro;
        let Some(chiave) = self.chiave.as_ref().filter(|_| dentro.attiva()) else {
            return self.sorgente.pezzo_finche(da, quanti, avanti);
        };
        let nome = chiave.nome();
        let quanti = match dentro.dal_disco(&nome, da, quanti) {
            DalDisco::Pezzo(pezzo) => return Ok(pezzo),
            DalDisco::Leggi { tratti, totale } => {
                // Il disco fuori dal lucchetto, come la rete.
                let byte = leggi_tratti(&percorso(&dentro.cartella, &nome, DATI), &tratti);
                dentro.letto(&nome, byte.is_some());
                match byte {
                    // La lunghezza **sempre**, anche da qui: chi apre un flusso
                    // la impara dal primo pezzo, e senza il cursore sparirebbe.
                    Some(byte) => {
                        return Ok(Pezzo {
                            byte,
                            totale,
                            url_finale: None,
                        });
                    }
                    None => quanti,
                }
            }
            DalDisco::Manca(quanti) => quanti,
        };
        // La rete fuori dal lucchetto: può metterci secondi, e intanto un
        // altro brano deve poter leggere dalla cache.
        let pezzo = self.sorgente.pezzo_finche(da, quanti, avanti)?;
        dentro.scrivi(chiave, self.epoca, da, &pezzo);
        Ok(Pezzo {
            url_finale: None,
            ..pezzo
        })
    }
}

impl Drop for ConCache {
    fn drop(&mut self) {
        if let Some(chiave) = &self.chiave {
            self.cache.dentro.salva_presto(&chiave.nome());
        }
    }
}

#[cfg(test)]
thread_local! {
    /// Per le prove: un disco che si riempie a metà pezzo. Quante scritture
    /// di un buco riescono ancora, su questo filo, prima che il disco dica di
    /// no; `None`, nessun guasto.
    static SCRITTURE_PRIMA_DEL_GUASTO: std::cell::Cell<Option<u32>> =
        const { std::cell::Cell::new(None) };
}

/// Il disco dice di no alla prossima scrittura? Solo nelle prove: vedi
/// `SCRITTURE_PRIMA_DEL_GUASTO`.
#[cfg(test)]
fn disco_pieno() -> bool {
    SCRITTURE_PRIMA_DEL_GUASTO.with(|resta| match resta.get() {
        Some(0) => true,
        Some(n) => {
            resta.set(Some(n.saturating_sub(1)));
            false
        }
        None => false,
    })
}

/// Fuori dalle prove il disco risponde da sé, con `write_all`.
#[cfg(not(test))]
const fn disco_pieno() -> bool {
    false
}

/// Il percorso di un file della cache.
fn percorso(cartella: &Path, nome: &str, estensione: &str) -> PathBuf {
    cartella.join(format!("{nome}.{estensione}"))
}

/// Cancella i file di una copia, quelli che ci sono.
fn butta_file(cartella: &Path, nome: &str) {
    for estensione in [DATI, INDICE] {
        let _ = std::fs::remove_file(percorso(cartella, nome, estensione));
    }
}

/// Il segmento che contiene il byte `dove` del brano.
fn segmento_di(segmenti: &[Segmento], dove: u64) -> Option<&Segmento> {
    segmenti.iter().find(|s| s.da <= dove && dove < s.fino)
}

/// Le parti di `[da, fino)` che nessuno degli intervalli `presi` copre.
/// `presi` è in ordine di inizio, e gli intervalli possono sovrapporsi.
fn buchi(presi: &[(u64, u64)], da: u64, fino: u64) -> Vec<(u64, u64)> {
    let mut buchi = Vec::new();
    let mut cursore = da;
    // In ordine di inizio basta una passata.
    for &(inizio, fine) in presi {
        if fine <= cursore {
            continue;
        }
        if inizio >= fino {
            break;
        }
        if inizio > cursore {
            buchi.push((cursore, inizio.min(fino)));
        }
        cursore = cursore.max(fine);
        if cursore >= fino {
            break;
        }
    }
    if cursore < fino {
        buchi.push((cursore, fino));
    }
    buchi
}

/// Fonde i segmenti contigui **in tutte e due le coordinate**: quello dopo
/// comincia nel brano dove finisce quello prima, e in `.dati` subito dopo i
/// suoi byte. È il caso di un brano letto di fila dalla testa, che arriva a
/// finestre (da 256 KiB a 2 MiB, vedi `rymembra_net::flusso`): senza fonderle,
/// una copia arrivata già in ordine restava di decine di segmenti, e
/// [`Copia::completa`] — che ne vuole uno solo — la faceva ricomporre per
/// intero alla fine di ogni brano.
///
/// Contigui nel brano ma non in `.dati` (la coda letta per prima) restano
/// due: lì la ricomposizione serve davvero. `segmenti` è già in ordine di
/// `da` e senza sovrapposizioni, e così resta.
fn fondi_contigui(segmenti: &mut Vec<Segmento>) {
    segmenti.dedup_by(|dopo, prima| {
        let contiguo =
            prima.fino == dopo.da && prima.pos.saturating_add(prima.lunghezza()) == dopo.pos;
        if contiguo {
            prima.fino = dopo.fino;
        }
        contiguo
    });
}

/// Legge da `.dati`, uno dopo l'altro, i tratti `(pos, quanti)`. `None` se
/// il disco non dà tutto quel che si chiede.
fn leggi_tratti(dati: &Path, tratti: &[(u64, u64)]) -> Option<Vec<u8>> {
    let mut file = File::open(dati).ok()?;
    let tutti: u64 = tratti.iter().map(|(_, quanti)| *quanti).sum();
    let mut byte = vec![0_u8; usize::try_from(tutti).ok()?];
    let mut dove = 0_usize;
    for &(pos, quanti) in tratti {
        let fine = dove.checked_add(usize::try_from(quanti).ok()?)?;
        file.seek(SeekFrom::Start(pos)).ok()?;
        file.read_exact(byte.get_mut(dove..fine)?).ok()?;
        dove = fine;
    }
    Some(byte)
}

/// Scrive in `.dati` i pezzi prenotati di `pezzo`, che comincia al byte `da`
/// del brano. Un `Err` vuol dire che il disco ha detto di no da qualche
/// parte, e chi chiama butta la copia.
fn scrivi_fuori(dati: &Path, da: u64, pezzo: &Pezzo, prenotati: &[Segmento]) -> Result<(), ()> {
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(dati)
        .map_err(|_| ())?;
    for segmento in prenotati {
        let dentro = usize::try_from(segmento.da - da).map_err(|_| ())?;
        let fuori = usize::try_from(segmento.fino - da).map_err(|_| ())?;
        let byte = pezzo.byte.get(dentro..fuori).ok_or(())?;
        if disco_pieno() {
            return Err(());
        }
        file.seek(SeekFrom::Start(segmento.pos)).map_err(|_| ())?;
        file.write_all(byte).map_err(|_| ())?;
    }
    Ok(())
}

/// `sync_data` di `.dati` se ci sono byte nuovi, poi l'indice.
fn salva_su_disco(
    cartella: &Path,
    nome: &str,
    indice: &Indice,
    byte_nuovi: bool,
) -> std::io::Result<()> {
    if byte_nuovi {
        // Aperto in scrittura: su Windows `FlushFileBuffers` lo vuole. Porta
        // su disco quel che hanno scritto tutti, non solo questo handle.
        OpenOptions::new()
            .write(true)
            .open(percorso(cartella, nome, DATI))?
            .sync_data()?;
    }
    scrivi_indice(cartella, nome, indice)
}

/// Copia in `provvisorio`, in ordine, i segmenti di `.dati` che coprono il
/// brano, e lo porta su disco. Non rinomina: lo fa [`rimpiazza`], quando
/// nessuno usa più la copia.
fn in_ordine(
    dati: &Path,
    segmenti: &[Segmento],
    totale: u64,
    provvisorio: &Path,
) -> std::io::Result<()> {
    let mut sorgente = File::open(dati)?;
    let mut destinazione = File::create(provvisorio)?;
    let mut cursore = 0_u64;
    let mut blocco = vec![0_u8; 256 * 1024];
    while cursore < totale {
        let segmento = segmento_di(segmenti, cursore)
            .copied()
            .ok_or_else(|| std::io::Error::other("un buco in una copia coperta"))?;
        let mut resto = segmento.fino.min(totale) - cursore;
        sorgente.seek(SeekFrom::Start(segmento.pos + (cursore - segmento.da)))?;
        while resto > 0 {
            let quanti = usize::try_from(resto.min(256 * 1024)).unwrap_or(256 * 1024);
            let pezzo = blocco
                .get_mut(..quanti)
                .ok_or_else(|| std::io::Error::other("blocco"))?;
            sorgente.read_exact(pezzo)?;
            destinazione.write_all(pezzo)?;
            resto = resto.saturating_sub(u64::try_from(quanti).unwrap_or(resto));
        }
        cursore = segmento.fino.min(totale);
    }
    destinazione.sync_all()
}

/// Mette la copia in ordine al posto di quella in fila. Prima se ne va
/// l'indice vecchio, che parla dei pezzi in fila: vedi il `//!`. Si chiama
/// senza nessun file della copia aperto, perché su Windows un file aperto da
/// questo processo può non lasciarsi sostituire.
fn rimpiazza(cartella: &Path, nome: &str, provvisorio: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(percorso(cartella, nome, INDICE)) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    std::fs::rename(provvisorio, percorso(cartella, nome, DATI))
}

/// Scrive l'indice di una copia: in un file accanto, poi al suo posto.
fn scrivi_indice(cartella: &Path, nome: &str, indice: &Indice) -> std::io::Result<()> {
    let testo = serde_json::to_vec(indice).map_err(std::io::Error::other)?;
    let provvisorio = cartella.join(format!("{nome}.{INDICE}.{PROVVISORIO}"));
    {
        let mut file = File::create(&provvisorio)?;
        file.write_all(&testo)?;
        file.sync_all()?;
    }
    std::fs::rename(&provvisorio, percorso(cartella, nome, INDICE))
}

/// Una copia dal suo indice su disco, se torna tutto.
fn ritrova(cartella: &Path, nome: &str) -> Option<Copia> {
    let testo = std::fs::read(percorso(cartella, nome, INDICE)).ok()?;
    let indice: Indice = serde_json::from_slice(&testo).ok()?;
    let chiave = Chiave {
        conto: indice.conto,
        id: indice.id,
        versione: indice.versione,
    };
    // Il nome deve venire dalla chiave: un indice copiato a mano sotto un
    // altro nome servirebbe i byte di un brano al posto di un altro.
    if chiave.nome() != nome {
        return None;
    }
    let mut segmenti = indice.segmenti;
    segmenti.sort_by_key(|s| s.da);
    // Dove finisce l'ultimo byte promesso, non la somma delle lunghezze: con
    // più scritture insieme `.dati` può avere un buco (una chiusura brusca
    // mentre ne finiva una, quando una prenotata dopo era già nell'indice), e
    // la prossima scrittura va dopo tutto quel che l'indice nomina.
    let promessi = segmenti
        .iter()
        .map(|s| s.pos.saturating_add(s.lunghezza()))
        .max()
        .unwrap_or(0);
    let su_disco = std::fs::metadata(percorso(cartella, nome, DATI))
        .map(|m| m.len())
        .unwrap_or(0);
    let sovrapposti = segmenti.windows(2).any(|coppia| match coppia {
        [prima, dopo] => prima.fino > dopo.da,
        _ => false,
    });
    if su_disco < promessi || sovrapposti || segmenti.iter().any(|s| s.fino <= s.da) {
        return None;
    }
    let mut copia = Copia::nuova(chiave, indice.totale);
    copia.segmenti = segmenti;
    copia.fine_dati = promessi;
    copia.usato = (indice.usato_ms, 0);
    copia.da_misurare = indice.da_misurare;
    Some(copia)
}

#[cfg(test)]
mod prove;
