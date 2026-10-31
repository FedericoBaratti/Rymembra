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
//! # Scritture atomiche, e l'ordine
//!
//! Prima i byte, poi `sync_data`, poi l'indice — scritto in un file accanto e
//! rinominato sopra quello vecchio. Un'interruzione fra le due lascia byte in
//! più in fondo a `.dati` che l'indice non conosce (si sovrascrivono alla
//! scrittura dopo), mai un indice che promette byte che non ci sono.
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
use std::sync::{Arc, Mutex, PoisonError};

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
    /// In ordine di `da`, senza sovrapposizioni.
    segmenti: Vec<Segmento>,
    /// Dove scrivere il prossimo pezzo in `.dati`: la somma dei segmenti.
    fine_dati: u64,
    /// Per il meno usato di recente: l'orologio, e un contatore per chi è
    /// stato usato nello stesso millisecondo.
    usato: (i64, u64),
    /// Completa e non ancora misurata.
    da_misurare: bool,
}

impl Copia {
    fn nuova(chiave: Chiave, totale: Option<u64>) -> Self {
        Self {
            chiave,
            totale,
            segmenti: Vec::new(),
            fine_dati: 0,
            usato: (0, 0),
            da_misurare: false,
        }
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

    /// Il segmento che contiene il byte `dove`.
    fn segmento_di(&self, dove: u64) -> Option<&Segmento> {
        self.segmenti.iter().find(|s| s.da <= dove && dove < s.fino)
    }

    /// Il primo segmento che comincia dopo `dove`.
    fn prossimo_dopo(&self, dove: u64) -> Option<u64> {
        self.segmenti.iter().map(|s| s.da).find(|da| *da > dove)
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
    /// Il contatore degli usi, per [`Copia::usato`].
    contatore: u64,
    /// L'epoca dei conti tolti in questa sessione: vedi [`Cache::togli_conto`].
    /// Un conto che non c'è vale zero.
    tolti: HashMap<String, u64>,
}

/// Chi avvisare quando una copia diventa completa.
type Avviso = Arc<dyn Fn(&Chiave) + Send + Sync>;

/// La cache: una cartella, e quel che ci sta dentro. Vedi il `//!`.
pub struct Cache {
    cartella: PathBuf,
    stato: Mutex<Stato>,
    avviso: Mutex<Option<Avviso>>,
}

impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cache")
            .field("cartella", &self.cartella)
            .finish_non_exhaustive()
    }
}

/// Cosa sa dire il disco di una richiesta.
enum DalDisco {
    /// Ecco il pezzo.
    Pezzo(Pezzo),
    /// Va chiesto alla rete: `quanti` byte da dove si era chiesto.
    Manca(u64),
}

impl Cache {
    /// Apre la cache in `cartella`, creandola se serve, e ritrova le copie
    /// rimaste dall'ultima volta.
    ///
    /// Quel che non torna — un `.dati` senza indice, un indice che promette
    /// più byte di quelli che ci sono, un file a metà di una scrittura — si
    /// butta: è una cache, e il peggio che costa è riscaricarlo.
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
        let cache = Self {
            cartella,
            stato: Mutex::new(Stato {
                copie,
                tetto,
                attiva,
                usati,
                contatore: 0,
                tolti: HashMap::new(),
            }),
            avviso: Mutex::new(None),
        };
        {
            let mut stato = cache.blocca();
            cache.fai_posto(&mut stato, None);
        }
        Ok(Arc::new(cache))
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
            let mut stato = self.blocca();
            epoca = Self::epoca(&stato, &chiave.conto);
            let vecchie: Vec<String> = stato
                .copie
                .iter()
                .filter(|(_, copia)| copia.chiave.stesso_file(chiave) && copia.chiave != *chiave)
                .map(|(nome, _)| nome.clone())
                .collect();
            for nome in vecchie {
                self.togli(&mut stato, &nome);
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
    #[must_use]
    pub fn stato(&self) -> (u64, u64, bool) {
        let stato = self.blocca();
        (stato.usati, stato.tetto, stato.attiva)
    }

    /// Cambia il tetto e l'interruttore.
    ///
    /// Spenta, la cache non legge e non scrive: le sorgenti passano dirette.
    /// Quel che c'è resta su disco finché qualcuno non chiama [`Self::svuota`].
    pub fn imposta(&self, tetto: u64, attiva: bool) {
        let mut stato = self.blocca();
        stato.tetto = tetto;
        stato.attiva = attiva;
        self.fai_posto(&mut stato, None);
    }

    /// Butta tutto.
    ///
    /// # Errori
    ///
    /// `fs.*` se la cartella non si legge; i file che non si cancellano si
    /// lasciano, e la prossima apertura li riconosce come orfani.
    pub fn svuota(&self) -> Result<(), AppError> {
        let mut stato = self.blocca();
        let nomi: Vec<String> = stato.copie.keys().cloned().collect();
        for nome in nomi {
            self.togli(&mut stato, &nome);
        }
        let nome_cartella = self.cartella.to_string_lossy().into_owned();
        let voci =
            std::fs::read_dir(&self.cartella).map_err(|err| io_error(&nome_cartella, &err))?;
        for voce in voci.flatten() {
            let percorso = voce.path();
            if matches!(
                percorso.extension().and_then(|e| e.to_str()),
                Some(DATI | INDICE | PROVVISORIO)
            ) {
                let _ = std::fs::remove_file(&percorso);
            }
        }
        stato.usati = 0;
        Ok(())
    }

    /// Il file di una copia completa, da leggere come un file locale.
    ///
    /// `None` se la copia non c'è, non è completa, o la cache è spenta: con la
    /// cache spenta il volume viene solo dai tag, e lo si dice così.
    #[must_use]
    pub fn copia_completa(&self, chiave: &Chiave) -> Option<PathBuf> {
        let stato = self.blocca();
        if !stato.attiva {
            return None;
        }
        let nome = chiave.nome();
        let copia = stato.copie.get(&nome)?;
        copia
            .completa()
            .then(|| percorso(&self.cartella, &nome, DATI))
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
        let stato = self.blocca();
        let mut copie: Vec<&Copia> = stato
            .copie
            .values()
            .filter(|copia| copia.da_misurare && copia.completa())
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
    pub fn togli_conto(&self, conto: &str) -> usize {
        let mut stato = self.blocca();
        let epoca = stato.tolti.entry(conto.to_owned()).or_insert(0);
        *epoca = epoca.wrapping_add(1);
        let nomi: Vec<String> = stato
            .copie
            .iter()
            .filter(|(_, copia)| copia.chiave.conto == conto)
            .map(|(nome, _)| nome.clone())
            .collect();
        for nome in &nomi {
            self.togli(&mut stato, nome);
        }
        nomi.len()
    }

    /// L'epoca di un conto: quante volte [`Self::togli_conto`] l'ha tolto in
    /// questa sessione.
    fn epoca(stato: &Stato, conto: &str) -> u64 {
        stato.tolti.get(conto).copied().unwrap_or(0)
    }

    /// La copia è stata misurata: da qui in poi se ne va come le altre.
    pub fn segna_misurata(&self, chiave: &Chiave) {
        let mut stato = self.blocca();
        let nome = chiave.nome();
        let Some(copia) = stato.copie.get_mut(&nome) else {
            return;
        };
        if !copia.da_misurare {
            return;
        }
        copia.da_misurare = false;
        let indice = copia.indice();
        if scrivi_indice(&self.cartella, &nome, &indice).is_err() {
            self.togli(&mut stato, &nome);
        }
        self.fai_posto(&mut stato, None);
    }

    /// Chi chiamare quando una copia diventa completa: il filo dell'analisi,
    /// che così misura mentre la copia c'è.
    ///
    /// Si chiama fuori dal lucchetto della cache, dal filo che ha scritto
    /// l'ultimo pezzo: deve tornare presto.
    pub fn al_completamento(&self, avviso: impl Fn(&Chiave) + Send + Sync + 'static) {
        *self.avviso.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(avviso));
    }

    /// Quel che il disco sa dare di `[da, da + quanti)`.
    fn dal_disco(&self, chiave: &Chiave, da: u64, quanti: u64) -> DalDisco {
        let mut stato = self.blocca();
        let nome = chiave.nome();
        let Some(copia) = stato.copie.get(&nome) else {
            return DalDisco::Manca(quanti);
        };
        let Some(totale) = copia.totale else {
            // Senza lunghezza un pezzo corto direbbe «fine del file» a chi
            // legge: si serve dal disco solo una richiesta coperta per intero.
            return match leggi_coperto(&self.cartella, &nome, copia, da, quanti, false) {
                Some(byte) => {
                    stato.contatore = stato.contatore.wrapping_add(1);
                    let usato = (adesso_ms(), stato.contatore);
                    if let Some(copia) = stato.copie.get_mut(&nome) {
                        copia.usato = usato;
                    }
                    DalDisco::Pezzo(Pezzo {
                        byte,
                        totale: None,
                        url_finale: None,
                    })
                }
                None => DalDisco::Manca(quanti),
            };
        };
        if da >= totale {
            return DalDisco::Pezzo(Pezzo {
                byte: Vec::new(),
                totale: Some(totale),
                url_finale: None,
            });
        }
        let fino = da.saturating_add(quanti).min(totale);
        if copia.segmento_di(da).is_some() {
            if let Some(byte) = leggi_coperto(&self.cartella, &nome, copia, da, fino - da, true) {
                stato.contatore = stato.contatore.wrapping_add(1);
                let usato = (adesso_ms(), stato.contatore);
                if let Some(copia) = stato.copie.get_mut(&nome) {
                    copia.usato = usato;
                }
                // La lunghezza **sempre**, anche da qui: chi apre un flusso la
                // impara dal primo pezzo, e senza il cursore sparirebbe.
                return DalDisco::Pezzo(Pezzo {
                    byte,
                    totale: Some(totale),
                    url_finale: None,
                });
            }
            // Il disco non ha dato quel che l'indice prometteva: la copia non
            // è affidabile, e si butta.
            self.togli(&mut stato, &nome);
            return DalDisco::Manca(quanti);
        }
        // Dalla rete solo fino al prossimo pezzo che c'è già: con la lunghezza
        // nota, un pezzo corto non inganna nessuno.
        let fino = copia
            .prossimo_dopo(da)
            .map_or(fino, |prossimo| prossimo.min(fino));
        DalDisco::Manca(fino - da)
    }

    /// Mette su disco un pezzo arrivato dalla rete, se il conto della sorgente
    /// è ancora quello di quando è nata (`epoca`: vedi [`Self::togli_conto`]).
    fn scrivi(&self, chiave: &Chiave, epoca: u64, da: u64, pezzo: &Pezzo) {
        let completata = {
            let mut stato = self.blocca();
            if !stato.attiva || pezzo.byte.is_empty() || Self::epoca(&stato, &chiave.conto) != epoca
            {
                return;
            }
            let nome = chiave.nome();
            match self.scrivi_dentro(&mut stato, chiave, &nome, da, pezzo) {
                Ok(completata) => {
                    self.fai_posto(&mut stato, Some(&nome));
                    completata
                }
                Err(()) => {
                    self.togli(&mut stato, &nome);
                    false
                }
            }
        };
        if completata {
            let avviso = self
                .avviso
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            if let Some(avviso) = avviso {
                avviso(chiave);
            }
        }
    }

    /// Il corpo di [`Self::scrivi`]: `Ok(true)` se la copia è appena diventata
    /// completa. Un `Err` vuol dire che il disco ha detto di no da qualche
    /// parte, e chi chiama butta la copia.
    fn scrivi_dentro(
        &self,
        stato: &mut Stato,
        chiave: &Chiave,
        nome: &str,
        da: u64,
        pezzo: &Pezzo,
    ) -> Result<bool, ()> {
        stato.contatore = stato.contatore.wrapping_add(1);
        let usato = (adesso_ms(), stato.contatore);
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
            return Ok(false);
        }
        let copia = stato
            .copie
            .entry(nome.to_owned())
            .or_insert_with(|| Copia::nuova(chiave.clone(), pezzo.totale));
        if copia.totale.is_none() {
            copia.totale = pezzo.totale;
        }
        copia.usato = usato;
        // Coperta già prima di questo pezzo: non è una copia che si completa
        // adesso, e non si avvisa una seconda volta.
        let coperta_prima = copia.coperta();
        let fino = da.saturating_add(u64::try_from(pezzo.byte.len()).map_err(|_| ())?);
        let buchi = buchi(&copia.segmenti, da, fino);
        let mut aggiunti = 0_u64;
        if !buchi.is_empty() {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(percorso(&self.cartella, nome, DATI))
                .map_err(|_| ())?;
            file.seek(SeekFrom::Start(copia.fine_dati))
                .map_err(|_| ())?;
            for (inizio, fine) in buchi {
                let dentro = usize::try_from(inizio - da).map_err(|_| ())?;
                let fuori = usize::try_from(fine - da).map_err(|_| ())?;
                let byte = pezzo.byte.get(dentro..fuori).ok_or(())?;
                if disco_pieno() {
                    return Err(());
                }
                file.write_all(byte).map_err(|_| ())?;
                copia.segmenti.push(Segmento {
                    da: inizio,
                    fino: fine,
                    pos: copia.fine_dati,
                });
                copia.fine_dati = copia.fine_dati.saturating_add(fine - inizio);
                // Nel conto subito, buco per buco, insieme a `fine_dati`: se il
                // buco dopo o il `sync_data` falliscono, chi chiama butta la
                // copia e toglie dal conto il suo `fine_dati`, che deve essere
                // esattamente quel che ci era entrato. Contati tutti alla fine,
                // un guasto a metà toglieva byte mai aggiunti, e il conto
                // scendeva sotto il disco fino al riavvio.
                stato.usati = stato.usati.saturating_add(fine - inizio);
                aggiunti = aggiunti.saturating_add(fine - inizio);
            }
            // I byte prima dell'indice: vedi il `//!`.
            file.sync_data().map_err(|_| ())?;
            copia.segmenti.sort_by_key(|s| s.da);
            fondi_contigui(&mut copia.segmenti);
        }
        let copia = stato.copie.get_mut(nome).ok_or(())?;
        let mut completata = false;
        let prima = copia.fine_dati;
        if !coperta_prima && copia.coperta() {
            // Arrivata di fila dalla testa — in un pezzo solo, o a finestre
            // una dopo l'altra come la chiede un flusso che suona — è già un
            // segmento solo in ordine (vedi [`fondi_contigui`]). Solo chi ha
            // letto prima la coda, o è saltato avanti e poi indietro, lascia
            // pezzi fuori posto: quella va ricomposta prima di essere un file
            // che l'analisi sa aprire.
            if !copia.completa() {
                ricomponi(&self.cartella, nome, copia)?;
            }
            copia.da_misurare = true;
            completata = true;
        }
        let dopo = copia.fine_dati;
        if aggiunti > 0 || completata {
            scrivi_indice(&self.cartella, nome, &copia.indice()).map_err(|_| ())?;
        }
        // Ricomposta, la copia pesa quanto il brano: di solito quel che pesava
        // già, ma il conto dei byte deve seguire il disco e non la speranza.
        stato.usati = stato.usati.saturating_sub(prima).saturating_add(dopo);
        Ok(completata)
    }

    /// Butta via copie finché si sta sotto il tetto. `tenere` non si tocca:
    /// è quella che si sta scrivendo.
    fn fai_posto(&self, stato: &mut Stato, tenere: Option<&str>) {
        loop {
            if stato.usati <= stato.tetto {
                return;
            }
            let oltre_il_doppio = stato.usati > stato.tetto.saturating_mul(2);
            let vittima = stato
                .copie
                .iter()
                .filter(|(nome, copia)| {
                    Some(nome.as_str()) != tenere && (!copia.da_misurare || oltre_il_doppio)
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

    /// Toglie una copia: dalla memoria, dal conto dei byte e dal disco.
    fn togli(&self, stato: &mut Stato, nome: &str) {
        if let Some(copia) = stato.copie.remove(nome) {
            stato.usati = stato.usati.saturating_sub(copia.fine_dati);
        }
        butta_file(&self.cartella, nome);
    }

    fn blocca(&self) -> std::sync::MutexGuard<'_, Stato> {
        self.stato.lock().unwrap_or_else(PoisonError::into_inner)
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
        let Some(chiave) = self.chiave.as_ref().filter(|_| self.cache.stato().2) else {
            return self.sorgente.pezzo_finche(da, quanti, avanti);
        };
        let quanti = match self.cache.dal_disco(chiave, da, quanti) {
            DalDisco::Pezzo(pezzo) => return Ok(pezzo),
            DalDisco::Manca(quanti) => quanti,
        };
        // La rete fuori dal lucchetto: può metterci secondi, e intanto un
        // altro brano deve poter leggere dalla cache.
        let pezzo = self.sorgente.pezzo_finche(da, quanti, avanti)?;
        self.cache.scrivi(chiave, self.epoca, da, &pezzo);
        Ok(Pezzo {
            url_finale: None,
            ..pezzo
        })
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

/// Le parti di `[da, fino)` che nessun segmento copre.
fn buchi(segmenti: &[Segmento], da: u64, fino: u64) -> Vec<(u64, u64)> {
    let mut buchi = Vec::new();
    let mut cursore = da;
    // I segmenti sono in ordine e non si sovrappongono: basta una passata.
    for segmento in segmenti {
        if segmento.fino <= cursore {
            continue;
        }
        if segmento.da >= fino {
            break;
        }
        if segmento.da > cursore {
            buchi.push((cursore, segmento.da.min(fino)));
        }
        cursore = cursore.max(segmento.fino);
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
/// intero alla fine di ogni brano, col lucchetto della cache in mano.
///
/// Contigui nel brano ma non in `.dati` (la coda letta per prima) restano
/// due: lì la ricomposizione serve davvero. `segmenti` è già in ordine di
/// `da` e senza sovrapposizioni, e così resta; `fine_dati`, la somma delle
/// lunghezze, non cambia.
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

/// I byte `[da, da + quanti)` dal disco, se i segmenti li coprono di fila.
///
/// Con `basta_l_inizio` si accontenta di quel che c'è di fila da `da` (con la
/// lunghezza nota un pezzo corto va bene); senza, vuole tutto.
fn leggi_coperto(
    cartella: &Path,
    nome: &str,
    copia: &Copia,
    da: u64,
    quanti: u64,
    basta_l_inizio: bool,
) -> Option<Vec<u8>> {
    let fino = da.saturating_add(quanti);
    let mut file = File::open(percorso(cartella, nome, DATI)).ok()?;
    let mut byte = Vec::with_capacity(usize::try_from(quanti.min(4 * 1024 * 1024)).ok()?);
    let mut cursore = da;
    while cursore < fino {
        let Some(segmento) = copia.segmento_di(cursore) else {
            break;
        };
        let fine = segmento.fino.min(fino);
        let posizione = segmento.pos.saturating_add(cursore - segmento.da);
        let mut pezzo = vec![0_u8; usize::try_from(fine - cursore).ok()?];
        file.seek(SeekFrom::Start(posizione)).ok()?;
        file.read_exact(&mut pezzo).ok()?;
        byte.extend_from_slice(&pezzo);
        cursore = fine;
    }
    let completo = cursore >= fino;
    (completo || (basta_l_inizio && cursore > da)).then_some(byte)
}

/// Riscrive `.dati` in ordine, ora che c'è tutto.
fn ricomponi(cartella: &Path, nome: &str, copia: &mut Copia) -> Result<(), ()> {
    let totale = copia.totale.ok_or(())?;
    let provvisorio = percorso(cartella, nome, PROVVISORIO);
    if in_ordine(cartella, nome, copia, totale, &provvisorio).is_err() {
        let _ = std::fs::remove_file(&provvisorio);
        return Err(());
    }
    copia.segmenti = vec![Segmento {
        da: 0,
        fino: totale,
        pos: 0,
    }];
    copia.fine_dati = totale;
    Ok(())
}

/// Il corpo di [`ricomponi`]: copia i segmenti in ordine in `provvisorio`, e
/// lo rinomina sopra `.dati`.
fn in_ordine(
    cartella: &Path,
    nome: &str,
    copia: &Copia,
    totale: u64,
    provvisorio: &Path,
) -> std::io::Result<()> {
    let mut sorgente = File::open(percorso(cartella, nome, DATI))?;
    let mut destinazione = File::create(provvisorio)?;
    let mut cursore = 0_u64;
    let mut blocco = vec![0_u8; 256 * 1024];
    while cursore < totale {
        let segmento = copia
            .segmento_di(cursore)
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
    destinazione.sync_all()?;
    // Chiusi tutti e due prima di rinominare: su Windows un file aperto da
    // questo processo senza condivisione non si sostituisce.
    drop(destinazione);
    drop(sorgente);
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
    let fine_dati: u64 = segmenti.iter().map(Segmento::lunghezza).sum();
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
    Some(Copia {
        chiave,
        totale: indice.totale,
        segmenti,
        fine_dati,
        usato: (indice.usato_ms, 0),
        da_misurare: indice.da_misurare,
    })
}

#[cfg(test)]
mod prove;
