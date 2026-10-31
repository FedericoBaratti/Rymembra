//! Il filo della sessione: quello che fa davvero quel che [`super::fasi`]
//! decide.
//!
//! # Un filo, una cassetta
//!
//! Tutto quel che può cambiare la sessione arriva qui come [`Posta`], su un
//! canale solo: gli eventi del trasporto (girati da un filo che non fa altro),
//! il campanello del lettore, i comandi della finestra (con il capo da cui
//! rispondere), le copertine scaricate. Un ordine solo vuol dire nessun
//! lucchetto attorno alla situazione: la tiene questo filo e nessun altro.
//!
//! Il tempo non ha un timer: l'attesa sulla cassetta dura fino alla prossima
//! cosa da fare (un'ancora, una scadenza, un giro di scoperta), e la si fa al
//! risveglio.
//!
//! # Che cosa non tiene mai in mano
//!
//! - **Il lucchetto del lettore**: il ponte (`riproduzione::insieme`) lo prende
//!   da sé, per il tempo di una fotografia o di un comando, e chi lo chiama non
//!   deve avere niente in mano. Le due fotografie di questo modulo per il
//!   lettore e la finestra ([`Comune::nel_lettore`], [`Comune::vetrina`]) si
//!   scrivono in un colpo e si lasciano subito: `nel_lettore` lo legge il
//!   lettore **col suo** lucchetto in mano, e un giro al contrario sarebbe un
//!   abbraccio mortale.
//! - **La libreria mentre parla con la rete**: le letture e le scritture di
//!   `insieme_pari` sono finestre corte, e nessuna chiama il trasporto dentro.
//!   E nessuna aspetta per sempre: una scansione può tenere la libreria per
//!   secondi, e questo filo intanto deve mandare le ancore. Si prende con
//!   [`crate::stato::con_libreria_entro`], e chi non la ottiene rinuncia
//!   (vedi [`ATTESA_LIBRERIA`]).
//! - **Le aperture del lettore**: i gesti dei membri, sull'host, finiscono in
//!   comandi che possono aprire una voce — da un NAS, o dalla rete di un
//!   membro — e un'apertura può durare secondi. Li esegue un filo a parte
//!   (l'esecutore), e l'`Esito` parte quando torna.
//!
//! # Il ruolo, e la gara che non chiude
//!
//! `instrada` legge il ruolo senza lucchetti, da qualunque filo, e la strada la
//! decide **prima** di prendere il lucchetto del lettore. Qui il ruolo si cambia
//! prima di entrare e prima di uscire, ma questo **non basta** a chiudere la
//! gara: un gesto che ha letto il ruolo un istante prima del cambio arriva al
//! lettore dopo, con la strada sbagliata. Un `suona` instradato come
//! `LocaleConVoci` quando si ospitava ancora porta maniglie a una coda appena
//! rimessa com'era; uno instradato come `Locale` un istante prima di ospitare
//! porta righe di libreria in una coda di maniglie. Il gesto **passa**, non si
//! rifiuta.
//!
//! La gara la chiude il lettore, sotto il suo lucchetto: lì la coda e la
//! sessione si vedono nello stesso istante, e un gesto la cui strada non
//! corrisponde più a quel che trova lo riconosce lui (vedi
//! `riproduzione::insieme`). Da questa parte resta solo l'ordine che rende la
//! finestra più stretta: il ruolo cambia prima del lettore, non dopo.
//!
//! # Fase 2 e fase 3, in breve
//!
//! - **Chi segue** manda i suoi gesti già tradotti ([`super::gesti`]) e non
//!   aspetta l'esito; i brani suoi li descrive con gettoni propri, apre la
//!   connessione di servizio (`Rete::offri`) e li manda con `Aggiungi`. Nello
//!   specchio li riconosce dal gettone e li suona dal proprio file.
//! - **Chi ospita** esegue i gesti dei membri con gli stessi comandi della
//!   finestra, dopo il secchiello del riduttore; registra le voci dei membri
//!   con `lettore_locale` e ne inoltra i gettoni; quando un membro esce, le
//!   sue voci escono con lui.
//! - **I descrittori** delle copie (fase 3) li calcola un filo a parte, perché
//!   leggono il disco: una voce parte senza, e lo stato si ripubblica quando
//!   il suo arriva. Chi segue, per i brani suoi, li aspetta un poco prima di
//!   mandarli (vedi [`ATTESA_DESCRITTORI`]).
//!
//! # Il diario
//!
//! Solo codici, conteggi e `pari #id` o `connessione #n`: mai un indirizzo, un
//! nome, un titolo, una chiave, un gettone o un codice di associazione. Le
//! righe che parlano della rete passano da [`riga_della_rete`], che è pura e
//! provata.

mod membro;
mod ospita;
mod rete;

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use rymembra_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use rymembra_insieme::messaggi::{
    Azione, DescrittoreCopia, Diritti, Dove, MASSIMO_NOME, Messaggio, Persona, VoceSulFilo,
};
use rymembra_insieme::{ChiavePubblica, Gettone, IdConnessione, LettoreRemoto, Rete};
use tauri::{AppHandle, Manager as _};

use self::ospita::{DaPreparare, Incarico, avvia_il_preparatore, avvia_l_esecutore};
use super::fasi::{self, Comando, Connessione, Effetto, Evento, Fase, Situazione};
use super::fiducia::{self, Fidati, Vicinato};
use super::preparazione::Preparazione;
use super::segui::{Aggancio, Condivisa};
use super::strada::Ruolo;
use super::voci::{self, Copertine, Servite};
use super::{
    AssociazioneIpc, AvvisoIpc, FaseIpc, FidatoIpc, InsiemeNelLettore, InvitoIpc, PersonaIpc,
    ProprietarioIpc, RichiestaIpc, SessioneIpc, SincroniaIpc, StatoInsiemeIpc, VicinoIpc,
};
use crate::nota;
use crate::riproduzione::insieme as ponte;
use crate::spegnimento::Emette as _;
use crate::stato::{Stato, adesso_ms, con_libreria};

/// Ogni quanto chi ospita manda un'ancora: quattro volte al secondo.
const OGNI_ANCORA_MS: u64 = 250;

/// Lo stato della sessione al più dieci volte al secondo: una raffica di gesti
/// (dieci brani tolti di fila) diventa uno stato solo, e uno stato da mille
/// voci è un centinaio di kilobyte per membro.
const OGNI_STATO_MS: u64 = 100;

/// Lo stato che cambia **solo** perché altre voci sono state descritte (la
/// preparazione che avanza) va al più una volta al secondo. Uno stato è tutta
/// la coda — mille voci, centinaia di kilobyte per membro — e la preparazione
/// di una coda nuova ne cambia il conto a ogni blocco: a dieci al secondo il
/// filo passava due terzi del tempo a rimandarlo, e i gesti dei membri
/// aspettavano in fila dietro (misurato: 9 stati al secondo per i primi venti
/// secondi di ogni sessione, 70 ms l'uno in debug, e una pausa di un membro
/// risposta dopo 2,8 s). La voce che suona, la pausa, la coda e le persone
/// restano a [`OGNI_STATO_MS`].
const OGNI_DESCRIZIONI_MS: u64 = 1_000;

/// Ogni quanto un giro di scoperta, fuori sessione.
const OGNI_SCOPERTA_MS: u64 = 10_000;

/// Ogni quanto si guarda se c'è un vicino da collegare.
const OGNI_COLLEGAMENTO_MS: u64 = 1_000;

/// Ogni quanto la finestra riceve l'errore della sincronia, se è cambiato
/// solo quello: un numero che trema dieci volte al secondo non si legge, e
/// costerebbe un evento ogni colpo.
const OGNI_AGGANCIO_MS: u64 = 1_000;

/// Quanto aspettare, dopo una copertina arrivata, prima di ridisegnare le voci
/// che la usano: le copertine di un album arrivano a raffica.
const RIDISEGNA_DOPO_MS: u64 = 300;

/// Quanto aspettano i fili, al più, quando si ferma tutto: la rete che
/// consegna i congedi e la sincronia che esce. Contati dall'inizio della
/// fermata, e sotto i 400 ms che `super::ferma` concede: la risposta deve
/// arrivare prima che chi esce smetta di aspettarla.
const ATTESA_FILI: Duration = Duration::from_millis(300);

/// Quanto si lascia alla rete per consegnare quel che ha in coda spegnendo.
///
/// `Rete::ferma` chiude con garbo: manda quel che era in coda (il `Fine` a
/// tutti, l'`Esco` all'host) e poi il `Chiudo`. Ma lo fanno i suoi fili, e
/// un'applicazione che esce subito dopo li porterebbe via a metà: dall'altra
/// parte si leggerebbe «host sparito» invece di «sessione terminata».
const ATTESA_RETE: Duration = Duration::from_millis(150);

/// Fase 3, chi segue: quanto un'aggiunta di brani suoi aspetta i
/// descrittori prima di partire senza. Due secondi: su un disco di casa
/// centoventotto kilobyte per brano sono millisecondi, e un NAS che dorme non
/// deve tenere ferma la coda di tutti.
const ATTESA_DESCRITTORI: u64 = 2_000;

/// Lo stesso, quando la prima voce deve partire subito (il doppio clic):
/// mezzo secondo, perché lì qualcuno sta aspettando di sentirla.
const ATTESA_DESCRITTORI_SUONA: u64 = 500;

/// Chi segue: una voce propria mandata e mai comparsa nello stato dell'host
/// dopo tanto si smette di servirla. L'host l'avrà rifiutata.
const ATTESA_ECO_MS: u64 = 30_000;

/// Gli `uid` che chi ospita dà alle voci dei membri partono da qui: quelli
/// delle voci sue li dà il lettore da uno in su, e due contatori che non si
/// vedono non devono potersi incontrare. Un milione di milioni di voci sue
/// prima di arrivarci.
const UID_DEI_MEMBRI: u64 = 1 << 40;

/// Quanto il filo della sessione aspetta la libreria: due secondi, poi
/// rinuncia con `db.locked`; intanto le ancore non si sono fermate dietro a
/// una scansione. Una scrittura chiesta dalla persona che non passa torna a
/// lei con l'errore, e la ripete lei. Una rilettura no: nessuno la
/// ripeterebbe, e la si ritenta da qui (vedi [`RILEGGI_DOPO_MS`]).
const ATTESA_LIBRERIA: Duration = Duration::from_secs(2);

/// Fra quanto si ritenta una rilettura dei fidati o delle impostazioni che
/// non ha trovato la libreria libera: cinque secondi.
///
/// Prima una rilettura non riuscita si buttava in silenzio, e un PC appena
/// bloccato restava fidato in memoria e nel trasporto finché l'app non si
/// riavviava; all'avvio, impostazioni e fidati mai letti accendevano la rete
/// come se l'elenco fosse vuoto, e ogni PC associato si sentiva rispondere
/// «non associato».
const RILEGGI_DOPO_MS: u64 = 5_000;

/// Per le scritture che si possono saltare (l'ultima volta che un fidato si è
/// visto): un quarto di secondo.
const ATTESA_LIBRERIA_BREVE: Duration = Duration::from_millis(250);

/// Oltre questo, un giro del filo o un gesto di un membro finiscono nel
/// diario: è più o meno quando una persona comincia ad accorgersene.
const LENTO: Duration = Duration::from_millis(300);

/// Al più una riga di filo lento ogni tanto: un filo che resta indietro per un
/// minuto non deve riempire il diario. Le altre si contano e si dicono nella
/// riga dopo.
const RIGHE_DI_LENTEZZA_OGNI: Duration = Duration::from_secs(5);

/// Quanti gesti senza risposta chi segue ricorda, per il diario: un host che
/// non risponde più non deve far crescere niente.
const GESTI_IN_VOLO: usize = 64;

/// Quel che arriva al filo.
pub(super) enum Posta {
    /// Il lettore di chi ospita è cambiato.
    Campanello,
    /// Un evento del trasporto, della rete di quella generazione.
    Rete {
        /// La generazione della rete che l'ha mandato: dopo uno spegnimento e
        /// una riaccensione, gli eventi della rete vecchia si buttano.
        generazione: u64,
        /// L'evento.
        evento: rymembra_insieme::Evento,
        /// Quando è arrivato dalla rete: quel che passa da qui al giro che lo
        /// legge è coda davanti al filo, e il diario la scrive sopra
        /// [`LENTO`].
        arrivato: Instant,
    },
    /// Un comando della finestra, e dove rispondere.
    Comando {
        /// Che cosa.
        richiesta: Richiesta,
        /// Dove rispondere.
        risposta: Sender<Result<(), AppError>>,
    },
    /// Un giro di scoperta adesso.
    Cerca,
    /// I byte di una copertina, o `None` se non sono arrivati o non vanno bene.
    Copertina {
        /// Quale.
        gettone: Gettone,
        /// I byte.
        byte: Option<Arc<[u8]>>,
    },
    /// L'uscita audio è cambiata.
    UscitaCambiata,
    /// Chi ospita: l'esecutore ha finito un gesto di un membro.
    Eseguito {
        /// A chi rispondere, e con quale numero di comando.
        risposta: Option<(IdConnessione, u32)>,
        /// Il membro che l'aveva chiesto, per il diario.
        membro: u32,
        /// `None` se è andato; altrimenti il codice del rifiuto.
        codice: Option<String>,
        /// Il gesto, per il diario: vedi [`super::gesti::nome_dell_azione`].
        nome: &'static str,
        /// Quanto è rimasto in coda prima che l'esecutore lo prendesse, e
        /// quanto ci ha messo: il diario li scrive sopra [`LENTO`].
        attesa: Duration,
        durata: Duration,
        /// Le voci di un `Aggiungi` rimaste fuori dalla coda (piena): vanno
        /// dimenticate come quelle di un'aggiunta rifiutata. Vedi
        /// [`ospita::esito_dell_aggiunta`].
        fuori: Vec<u64>,
        /// La sessione dell'incarico: gli `uid` ricominciano a ogni sessione,
        /// e quelli di una finita sono altre voci in quella nuova.
        sessione: u64,
    },
    /// Rimettere la rete com'era all'avvio: un aggiornamento non riuscito
    /// dopo che [`super::ferma`] l'aveva spenta.
    Riaccendi,
    /// Fase 2, chi segue: gesti già tradotti, da mandare all'host.
    Gesti(Vec<Azione>),
    /// Fase 2, chi segue: brani della sua libreria da aggiungere.
    Aggiungi {
        /// I brani, letti dal ponte.
        brani: Vec<ponte::BranoDiQuesto>,
        /// Dove.
        dove: Dove,
        /// La prima parte subito.
        suona: bool,
    },
    /// Chi ospita: le voci di questo PC preparate dal loro filo, o `None` per
    /// quelle che non si sono potute preparare.
    Preparate {
        /// Il numero della sessione per cui erano state chieste: gli `uid`
        /// ricominciano a ogni sessione, e un esito vecchio non deve finire
        /// sulla voce nuova con lo stesso numero.
        sessione: u64,
        /// Per `uid`.
        esiti: Vec<(u64, Result<ponte::BranoDiQuesto, ponte::Mancanza>)>,
    },
    /// Fase 3: il descrittore di un file, o `None` se non si è potuto fare.
    Descrittore {
        /// Il file.
        file: voci::ChiaveFile,
        /// Il descrittore.
        copia: Option<DescrittoreCopia>,
    },
    /// L'applicazione esce, o si aggiorna: si spegne tutto.
    Ferma {
        /// Si risponde a lavoro finito.
        risposta: Sender<()>,
    },
}

/// I comandi della finestra.
#[derive(Debug)]
pub(super) enum Richiesta {
    Attiva(bool),
    Nome(String),
    FirewallVisto,
    Ospita,
    Termina,
    Esci,
    Associa(String),
    Codice {
        associazione: u64,
        uguale: bool,
    },
    Invita {
        pari: i64,
        diritti: Diritti,
    },
    Rispondi {
        invito: u64,
        accetta: bool,
    },
    Chiedi(i64),
    Ammetti {
        richiesta: u64,
        accetta: bool,
        diritti: Diritti,
    },
    Allontana(u32),
    Concedi {
        membro: u32,
        diritti: Diritti,
    },
    Rinomina {
        id: i64,
        nome: String,
    },
    Dimentica(i64),
    Blocca {
        id: i64,
        bloccato: bool,
    },
    Indirizzi(Vec<String>),
    Calibra(bool),
}

/// Quel che il filo condivide con chi lo legge senza passare dalla cassetta.
pub(super) struct Comune {
    /// Il ruolo, per `instrada`: vedi [`ruolo_in_byte`].
    pub(super) ruolo: AtomicU8,
    /// Il pannello, per `insieme_stato` e per la risposta dei comandi.
    pub(super) vetrina: Mutex<StatoInsiemeIpc>,
    /// Il campo `insieme` dello stato del lettore.
    pub(super) nel_lettore: Mutex<Option<InsiemeNelLettore>>,
    /// Le copertine arrivate dalla rete.
    pub(super) copertine: Mutex<Copertine>,
}

impl Posta {
    /// Il tipo, per il diario.
    const fn nome(&self) -> &'static str {
        match self {
            Self::Campanello => "campanello",
            Self::Rete { .. } => "rete",
            Self::Comando { .. } => "comando della finestra",
            Self::Cerca => "cerca",
            Self::Copertina { .. } => "copertina",
            Self::UscitaCambiata => "uscita cambiata",
            Self::Eseguito { .. } => "esito di un gesto",
            Self::Riaccendi => "riaccendi",
            Self::Gesti(_) => "gesti",
            Self::Aggiungi { .. } => "aggiungi",
            Self::Preparate { .. } => "voci preparate",
            Self::Descrittore { .. } => "descrittore",
            Self::Ferma { .. } => "ferma",
        }
    }
}

/// Le attese lunghe del filo, per il diario: vedi [`LENTO`]. Una per tipo:
/// i giri lenti e la posta arrivata in ritardo.
#[derive(Default)]
struct Lentezza {
    ultima_riga: Option<Instant>,
    taciuti: u32,
}

impl Lentezza {
    /// Se un'attesa di `quanto` va nel diario adesso: `Some` con quante altre
    /// sono state taciute dall'ultima riga, `None` se è sotto [`LENTO`] o se
    /// una riga è stata scritta da poco (allora si conta).
    fn da_scrivere(&mut self, quanto: Duration, adesso: Instant) -> Option<u32> {
        if quanto < LENTO {
            return None;
        }
        if self
            .ultima_riga
            .is_some_and(|prima| adesso.saturating_duration_since(prima) < RIGHE_DI_LENTEZZA_OGNI)
        {
            self.taciuti = self.taciuti.saturating_add(1);
            return None;
        }
        self.ultima_riga = Some(adesso);
        Some(std::mem::take(&mut self.taciuti))
    }
}

/// Il seguito di una riga di [`Lentezza`]: le altre volte taciute.
fn e_altre(taciuti: u32) -> String {
    if taciuti == 0 {
        String::new()
    } else {
        format!(", e altre {taciuti} volte da poco")
    }
}

impl Comune {
    pub(super) fn nuovo() -> Self {
        Self {
            ruolo: AtomicU8::new(0),
            vetrina: Mutex::new(vetrina_spenta()),
            nel_lettore: Mutex::new(None),
            copertine: Mutex::new(Copertine::default()),
        }
    }
}

/// Il ruolo come si scrive nell'atomico. Chi segue porta con sé i suoi
/// diritti: `instrada` li legge senza lucchetti, come il ruolo.
pub(super) const fn ruolo_in_byte(ruolo: Ruolo) -> u8 {
    match ruolo {
        Ruolo::Fuori => 0,
        Ruolo::Host => 1,
        Ruolo::Membro(Diritti::Ascolta) => 2,
        Ruolo::Membro(Diritti::Comanda) => 3,
        Ruolo::Membro(Diritti::Tutto) => 4,
    }
}

/// Il contrario di [`ruolo_in_byte`]. Un byte che non è di nessun ruolo vale
/// «fuori»: non succede, e se succedesse un gesto locale è il male minore.
pub(super) const fn ruolo_dal_byte(byte: u8) -> Ruolo {
    match byte {
        1 => Ruolo::Host,
        2 => Ruolo::Membro(Diritti::Ascolta),
        3 => Ruolo::Membro(Diritti::Comanda),
        4 => Ruolo::Membro(Diritti::Tutto),
        _ => Ruolo::Fuori,
    }
}

/// Il livello come lo scrive il diario: una parola, senza nomi.
const fn nome_dei_diritti(diritti: Diritti) -> &'static str {
    match diritti {
        Diritti::Ascolta => "ascolta",
        Diritti::Comanda => "comanda",
        Diritti::Tutto => "tutto",
    }
}

/// I millisecondi dell'orologio monotono del processo: quello delle scadenze.
#[expect(
    clippy::integer_division,
    reason = "millisecondi interi dai nanosecondi: il resto non conta per una scadenza"
)]
fn ora() -> u64 {
    rymembra_play::tempo::adesso_ns() / 1_000_000
}

/// Un errore del trasporto, col codice e senza la causa: vedi il `//!`.
fn senza_causa(err: &AppError) -> AppError {
    AppError::new(err.code().clone())
}

/// L'indirizzo su cui la rete si lega. Vedi `RYMEMBRA_INSIEME_LEGA` nella
/// specifica: solo nelle build di debug, per due istanze sullo stesso PC
/// senza passare dal firewall.
fn lega() -> IpAddr {
    #[cfg(debug_assertions)]
    {
        if let Some(ip) = std::env::var("RYMEMBRA_INSIEME_LEGA")
            .ok()
            .and_then(|v| v.trim().parse::<IpAddr>().ok())
        {
            return ip;
        }
    }
    IpAddr::V4(Ipv4Addr::UNSPECIFIED)
}

/// Il nome di questo PC per gli altri: quello scelto, o il nome del computer.
fn nome_effettivo(impostazioni: &rymembra_app::insieme::Impostazioni) -> String {
    impostazioni
        .nome
        .clone()
        .or_else(|| {
            std::env::var("COMPUTERNAME")
                .ok()
                .map(|n| voci::testo(&n, MASSIMO_NOME))
                .filter(|n| !n.is_empty())
        })
        .unwrap_or_else(|| "Rymembra".to_owned())
}

/// La rete accesa, e la chiave di questo PC.
struct Accesa {
    rete: Rete,
    mia: ChiavePubblica,
}

/// Chi segue: tutto quel che serve finché si è nella sessione dell'altro.
struct Seguendo {
    host: IdConnessione,
    /// Dove sta l'host e chi è: per la connessione di servizio (fase 2).
    indirizzo: SocketAddr,
    chiave: ChiavePubblica,
    condivisa: Arc<Condivisa>,
    sveglia: Sender<()>,
    filo: Option<std::thread::JoinHandle<()>>,
    /// La voce corrente dell'ultimo stato specchiato.
    corrente: Option<u64>,
    // fase 2
    /// Le voci che questo PC ha aggiunto, coi loro gettoni, per `uid`
    /// provvisorio: l'host le rinumera, i gettoni restano.
    proprie: Servite,
    uid_proprio: u64,
    /// La connessione di servizio è già stata chiesta.
    offerta: bool,
    prossimo_comando: u32,
    /// I gesti partiti e non ancora risposti: numero, quando, nome. Per il
    /// diario, sopra [`LENTO`]; al più [`GESTI_IN_VOLO`].
    in_volo: Vec<(u32, Instant, &'static str)>,
    /// Le aggiunte che aspettano i descrittori.
    in_attesa: Vec<Aggiunta>,
    /// Le voci proprie partite, per `uid` provvisorio.
    mandate: HashMap<u64, Mandata>,
}

/// Un'aggiunta di voci proprie che aspetta i descrittori (fase 3).
struct Aggiunta {
    uids: Vec<u64>,
    dove: Dove,
    suona: bool,
    scade: u64,
}

/// Una voce propria partita.
struct Mandata {
    /// È comparsa nello stato dell'host.
    vista: bool,
    /// Quando è partita.
    alle: u64,
}

/// Le voci della sessione che arrivano dal filo e chi ne apre i byte: chi
/// segue per tutte quelle dello specchio, chi ospita per quelle dei membri.
struct DalFilo {
    /// Chi legge i gettoni: verso l'host per chi segue, `lettore_locale` per
    /// chi ospita.
    lettore: LettoreRemoto,
    /// Alzata all'uscita: le letture in rete in corso tornano subito.
    sessione_finita: Arc<AtomicBool>,
    /// Il filo delle copertine; finisce quando questo canale cade.
    scaricatore: Option<Sender<Gettone>>,
    /// Le copertine già chieste, arrivate o no.
    chieste: HashSet<Gettone>,
    /// Le voci registrate nel lettore, com'erano sul filo, e la loro maniglia.
    registrate: HashMap<u64, (VoceSulFilo, Option<i64>)>,
    /// Le copertine arrivate da rimettere sulle voci, e da quando.
    da_ridisegnare: HashSet<Gettone>,
    ridisegna_dal: Option<u64>,
}

impl DalFilo {
    fn nuovo(lettore: LettoreRemoto, posta: Sender<Posta>) -> Self {
        Self {
            scaricatore: avvia_le_copertine(lettore.clone(), posta),
            lettore,
            sessione_finita: Arc::new(AtomicBool::new(false)),
            chieste: HashSet::new(),
            registrate: HashMap::new(),
            da_ridisegnare: HashSet::new(),
            ridisegna_dal: None,
        }
    }
}

/// Il filo. Vedi il `//!`.
struct Filo {
    app: AppHandle,
    comune: Arc<Comune>,
    posta: Sender<Posta>,
    s: Situazione,
    rete: Option<Accesa>,
    generazione: u64,
    impostazioni: rymembra_app::insieme::Impostazioni,
    nome: String,
    fidati: Fidati,
    /// Le impostazioni e i fidati non si sono potuti rileggere: quando
    /// ritentare. Vedi [`RILEGGI_DOPO_MS`].
    impostazioni_da_rileggere: Option<u64>,
    fidati_da_rileggere: Option<u64>,
    /// I fidati si sono letti almeno una volta: prima di allora la rete non
    /// si accende (vedi [`Filo::accendi`]).
    fidati_letti: bool,
    /// All'avvio la rete aspetta che impostazioni e fidati si leggano: la
    /// accende la rilettura che riesce.
    accendi_dopo_la_lettura: bool,
    vicinato: Vicinato,
    /// Indirizzo e chiave dell'altro capo, per connessione.
    capi: HashMap<IdConnessione, (SocketAddr, ChiavePubblica)>,
    prossima_scoperta: u64,
    prossimo_collegamento: u64,
    /// L'errore del comando che si sta servendo, se un effetto ne ha dato uno.
    errore: Option<AppError>,
    // chi ospita
    servite: Servite,
    versione: u64,
    /// L'ultima forma dello stato mandata, per non rimandarla uguale.
    impronta_stato: Option<ImprontaStato>,
    stato_mandato: u64,
    stato_in_sospeso: bool,
    /// Quando lo stato in sospeso va mandato: dopo [`OGNI_STATO_MS`] o, se è
    /// cambiato solo per le descrizioni, dopo [`OGNI_DESCRIZIONI_MS`].
    stato_dovuto: u64,
    /// Quanti descrittori di copia sono cambiati in questa vita del filo:
    /// vedi [`ImprontaStato::copie`].
    copie: u64,
    /// Vedi `InsiemeNelLettore::revisione`.
    revisione_voci: u64,
    /// Chi ospita: gli indirizzi da scrivere a mano, letti all'ingresso.
    indirizzi_di_qui: Vec<String>,
    prossima_ancora: u64,
    /// Fase 2: il prossimo `uid` per le voci dei membri.
    uid_dei_membri: u64,
    /// I clic di calibrazione stanno suonando.
    calibrazione: bool,
    /// Chi ospita: la voce corrente dell'ultima istantanea, per dire di chi è.
    corrente: Option<u64>,
    // chi segue
    seguendo: Option<Seguendo>,
    // tutti e due
    /// Le voci arrivate dal filo, finché si è in sessione.
    dal_filo: Option<DalFilo>,
    /// La voce che suona si legge da un file di qui: l'ultima risposta del
    /// lettore, tenuta qui perché `nel_lettore` non la può chiedere.
    locale: bool,
    /// «di Marco»: le voci in coda che non sono di questo PC.
    proprietari: Vec<ProprietarioIpc>,
    /// Il filo dei descrittori (fase 3).
    descrittori: Option<Sender<(voci::ChiaveFile, i64)>>,
    /// Chi ospita: a che punto è la preparazione di ogni voce di questo PC.
    preparazione: Preparazione,
    /// Il filo che prepara le voci, fuori da quello della sessione.
    preparatore: Option<Sender<DaPreparare>>,
    /// La voce che suona chiesta al preparatore fuori fila, e in che sessione:
    /// per chiederla una volta sola.
    chiesta_subito: Option<(u64, u64)>,
    /// Il filo che esegue i gesti dei membri (chi ospita).
    esecutore: Option<Sender<Incarico>>,
    /// La sessione di adesso, letta anche dall'esecutore: un gesto rimasto in
    /// fila dietro un'apertura lunga, della sessione di prima, non si esegue
    /// in quella nuova, dove i suoi `uid` sono altre voci.
    sessione_viva: Arc<AtomicU64>,
    /// Il numero della sessione ospitata, per riconoscere gli esiti vecchi.
    sessione: u64,
    /// I fili della sincronia che stanno uscendo, da aspettare fermando.
    in_uscita: Vec<std::thread::JoinHandle<()>>,
    // la finestra
    pubblicata: Option<String>,
    aggancio_mostrato: Option<Aggancio>,
    aggancio_dal: u64,
    /// La posta della rete arrivata in ritardo: vedi [`Lentezza`].
    ritardi: Lentezza,
}

/// Quel che distingue uno stato da un altro, senza serializzarlo: le voci,
/// la corrente, la pausa, il modo, le persone.
#[derive(Debug, Clone, PartialEq)]
struct ImprontaStato {
    coda: Vec<u64>,
    descritte: usize,
    /// Quante volte un descrittore di copia (fase 3) è cambiato: anche lui
    /// è una descrizione, e va col giro lento.
    copie: u64,
    corrente: Option<u64>,
    /// La voce che suona è descritta: chi segue la aspetta per suonarla, e
    /// non deve aspettare il giro lento delle descrizioni.
    corrente_descritta: bool,
    in_pausa: bool,
    ripeti: rymembra_domain::queue::RepeatMode,
    persone: Vec<Persona>,
}

impl ImprontaStato {
    /// Rispetto a `prima` cambia solo il conto delle voci descritte: vedi
    /// [`OGNI_DESCRIZIONI_MS`].
    fn solo_descrizioni(&self, prima: &Self) -> bool {
        (self.descritte != prima.descritte || self.copie != prima.copie)
            && self.coda == prima.coda
            && self.corrente == prima.corrente
            && self.corrente_descritta == prima.corrente_descritta
            && self.in_pausa == prima.in_pausa
            && self.ripeti == prima.ripeti
            && self.persone == prima.persone
    }
}

/// Fa partire il campanello e il filo. Vedi [`super::avvia_filo`].
pub(super) fn avvia(
    app: AppHandle,
    comune: Arc<Comune>,
    posta: Sender<Posta>,
    cassetta: Receiver<Posta>,
    orecchio: Receiver<()>,
) {
    // Il campanello del lettore diventa posta. Le sveglie arrivate a raffica
    // mentre il filo era occupato valgono una: si svuota il canale prima di
    // passarla.
    let campanello = posta.clone();
    let avviato = std::thread::Builder::new()
        .name("rymembra-insieme-campanello".to_owned())
        .spawn(move || {
            while orecchio.recv().is_ok() {
                while orecchio.try_recv().is_ok() {}
                if campanello.send(Posta::Campanello).is_err() {
                    return;
                }
            }
        });
    if let Err(err) = avviato {
        nota!("[avvio] il campanello di «Ascolta insieme» non è partito: {err}");
    }
    let avviato = std::thread::Builder::new()
        .name("rymembra-insieme".to_owned())
        .spawn(move || {
            let mut filo = Filo {
                app,
                comune,
                posta,
                s: Situazione::default(),
                rete: None,
                generazione: 0,
                impostazioni: rymembra_app::insieme::Impostazioni::default(),
                nome: String::new(),
                fidati: Fidati::default(),
                impostazioni_da_rileggere: None,
                fidati_da_rileggere: None,
                fidati_letti: false,
                accendi_dopo_la_lettura: false,
                vicinato: Vicinato::default(),
                capi: HashMap::new(),
                prossima_scoperta: 0,
                prossimo_collegamento: 0,
                errore: None,
                servite: Servite::default(),
                versione: 0,
                impronta_stato: None,
                stato_mandato: 0,
                stato_in_sospeso: false,
                stato_dovuto: 0,
                copie: 0,
                revisione_voci: 0,
                indirizzi_di_qui: Vec::new(),
                prossima_ancora: 0,
                uid_dei_membri: 0,
                calibrazione: false,
                corrente: None,
                seguendo: None,
                dal_filo: None,
                locale: false,
                proprietari: Vec::new(),
                descrittori: None,
                preparazione: Preparazione::default(),
                preparatore: None,
                chiesta_subito: None,
                esecutore: None,
                sessione_viva: Arc::new(AtomicU64::new(0)),
                sessione: 0,
                in_uscita: Vec::new(),
                pubblicata: None,
                aggancio_mostrato: None,
                aggancio_dal: 0,
                ritardi: Lentezza::default(),
            };
            crate::stato::sorvegliato("rymembra-insieme", || filo.avvio());
            let mut lentezza = Lentezza::default();
            loop {
                let attesa = filo.attesa();
                let posta = match cassetta.recv_timeout(attesa) {
                    Ok(posta) => Some(posta),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                };
                // Un panico in un giro non porta via il filo, ma la sessione
                // non torna com'era: `nutri` scrive la situazione nuova prima
                // di eseguire gli effetti, e il giro dopo la trova così, con
                // gli effetti eseguiti fino al colpo e gli altri mai. Per
                // questo l'uscita dal lettore, che deve sempre finire con
                // `Uscito`, ha il suo sorvegliato (`esci_dal_lettore`), e
                // `Spegni` chiude anche un'uscita rimasta a metà (`fasi`).
                let inizio = Instant::now();
                let cosa = posta.as_ref().map_or("scadenze", Posta::nome);
                crate::stato::sorvegliato("rymembra-insieme", || {
                    if let Some(posta) = posta {
                        filo.ricevi(posta);
                    }
                    filo.scadenze();
                    filo.pubblica();
                });
                let giro = inizio.elapsed();
                if let Some(taciuti) = lentezza.da_scrivere(giro, Instant::now()) {
                    nota!(
                        "[insieme] il filo è rimasto occupato {} ms ({cosa}){}",
                        giro.as_millis(),
                        e_altre(taciuti)
                    );
                }
            }
        });
    if let Err(err) = avviato {
        nota!("[avvio] il filo di «Ascolta insieme» non è partito: {err}");
    }
}

impl Filo {
    // ── l'avvio e il giro ───────────────────────────────────────────────────

    fn avvio(&mut self) {
        self.descrittori = avvia_i_descrittori(self.app.clone(), self.posta.clone());
        self.preparatore = avvia_il_preparatore(self.app.clone(), self.posta.clone());
        self.esecutore = avvia_l_esecutore(
            self.app.clone(),
            self.posta.clone(),
            Arc::clone(&self.sessione_viva),
        );
        self.ricarica_impostazioni();
        self.ricarica_fidati();
        if self.impostazioni_da_rileggere.is_some() || !self.fidati_letti {
            // La libreria occupata (una scansione all'avvio): l'interruttore
            // non si sa, o non si sa di chi fidarsi. La rete parte con la
            // rilettura che riesce, non spenta per sbaglio né con un elenco
            // vuoto. Vedi `RILEGGI_DOPO_MS`.
            self.accendi_dopo_la_lettura = true;
        } else if self.impostazioni.attivo
            && let Err(err) = self.accendi()
        {
            nota!(
                "[insieme] all'avvio la rete non parte codice={}",
                err.code().kind().code()
            );
            // Un guasto che non è della sessione (il database, un filo che non
            // parte) resta nel diario: la vetrina dice già «acceso, ma spento».
            if let Some(chiave) = chiave_per_la_finestra(&err, None) {
                self.avvisa(chiave, None);
            }
        }
        self.pubblica();
    }

    /// Fin quando aspettare la posta: la prossima cosa da fare.
    fn attesa(&self) -> Duration {
        let adesso = ora();
        let mut prossima: Option<u64> = fasi::prossima_scadenza(&self.s);
        let mut presto = |quando: u64| {
            prossima = Some(prossima.map_or(quando, |p| p.min(quando)));
        };
        match &self.s.fase {
            Fase::Ospita(_) => {
                if !fasi::connessioni_dei_membri(&self.s).is_empty() {
                    presto(self.prossima_ancora);
                }
                if let Some(quando) = self.preparazione.prossimo_tentativo() {
                    presto(quando);
                }
                if self.stato_in_sospeso {
                    presto(self.stato_dovuto);
                }
            }
            Fase::Libera { .. } if self.rete.is_some() => {
                presto(self.prossima_scoperta);
                presto(self.prossimo_collegamento);
            }
            Fase::Membro(_) => {
                presto(adesso.saturating_add(OGNI_AGGANCIO_MS));
                if let Some(prima) = self.seguendo.as_ref().and_then(|s| s.in_attesa.first()) {
                    presto(prima.scade);
                }
            }
            _ => {}
        }
        // In ogni fase, anche spenta: vedi `RILEGGI_DOPO_MS`.
        for quando in [self.impostazioni_da_rileggere, self.fidati_da_rileggere]
            .into_iter()
            .flatten()
        {
            presto(quando);
        }
        if let Some(dal) = self.dal_filo.as_ref().and_then(|d| d.ridisegna_dal) {
            presto(dal.saturating_add(RIDISEGNA_DOPO_MS));
        }
        let ms = prossima
            .map_or(60_000, |p| p.saturating_sub(adesso))
            .clamp(5, 60_000);
        Duration::from_millis(ms)
    }

    fn ricevi(&mut self, posta: Posta) {
        match posta {
            Posta::Campanello => self.campanello(),
            Posta::Rete {
                generazione,
                evento,
                arrivato,
            } => {
                let ritardo = arrivato.elapsed();
                if let Some(taciuti) = self.ritardi.da_scrivere(ritardo, Instant::now()) {
                    nota!(
                        "[insieme] la posta della rete aspettava da {} ms{}",
                        ritardo.as_millis(),
                        e_altre(taciuti)
                    );
                }
                if generazione == self.generazione && self.rete.is_some() {
                    self.dalla_rete(evento);
                }
            }
            Posta::Comando {
                richiesta,
                risposta,
            } => {
                self.errore = None;
                let esito = self.comando(richiesta);
                let errore = esito.err().or_else(|| self.errore.take());
                // La vetrina prima della risposta: il comando la legge appena
                // torna, e deve trovarci il suo effetto.
                self.pubblica();
                let _ = risposta.send(errore.map_or(Ok(()), Err));
            }
            Posta::Cerca => {
                if matches!(self.s.fase, Fase::Libera { .. }) {
                    self.prossima_scoperta = 0;
                    self.vicinato.riprova_subito(ora());
                    self.prossimo_collegamento = 0;
                }
            }
            Posta::Copertina { gettone, byte } => self.copertina_arrivata(gettone, byte),
            Posta::UscitaCambiata => {
                if let Some(seguendo) = &self.seguendo {
                    seguendo
                        .condivisa
                        .uscita_cambiata
                        .store(true, Ordering::Release);
                }
            }
            Posta::Ferma { risposta } => {
                self.ferma();
                let _ = risposta.send(());
            }
            Posta::Gesti(azioni) => self.manda_gesti(azioni),
            Posta::Aggiungi { brani, dove, suona } => self.aggiungi_proprie(brani, dove, suona),
            Posta::Descrittore { file, copia } => self.descrittore_arrivato(file, copia),
            Posta::Preparate { sessione, esiti } => self.preparate(sessione, esiti),
            Posta::Eseguito {
                risposta,
                membro,
                codice,
                nome,
                attesa,
                durata,
                fuori,
                sessione,
            } => {
                // Le voci registrate e inoltrate per niente: la coda era
                // piena. Solo nella sessione dell'incarico.
                if !fuori.is_empty() && sessione == self.sessione {
                    self.togli_l_aggiunta(&fuori, &HashSet::new());
                }
                if let Some(codice) = &codice {
                    nota!("[insieme] gesto «{nome}» del membro {membro} non fatto codice={codice}");
                }
                if attesa.saturating_add(durata) >= LENTO {
                    nota!(
                        "[insieme] gesto «{nome}» del membro {membro} lento: {} ms in coda, {} ms per farlo",
                        attesa.as_millis(),
                        durata.as_millis()
                    );
                }
                if let Some((da, comando)) = risposta {
                    self.manda(
                        da,
                        &Messaggio::Esito {
                            id: comando,
                            codice,
                        },
                    );
                }
            }
            Posta::Riaccendi => {
                self.ricarica_impostazioni();
                if self.impostazioni.attivo
                    && self.rete.is_none()
                    && let Err(err) = self.accendi()
                {
                    nota!(
                        "[insieme] la rete non riparte codice={}",
                        err.code().kind().code()
                    );
                    if let Some(chiave) = chiave_per_la_finestra(&err, None) {
                        self.avvisa(chiave, None);
                    }
                }
            }
        }
    }

    fn scadenze(&mut self) {
        let adesso = ora();
        self.rileggi_se_tocca(adesso);
        if fasi::prossima_scadenza(&self.s).is_some_and(|s| s <= adesso) {
            self.nutri(Evento::Tempo);
        }
        match &self.s.fase {
            Fase::Ospita(_) => {
                let ritenta = self
                    .preparazione
                    .prossimo_tentativo()
                    .is_some_and(|quando| quando <= adesso);
                if ritenta || (self.stato_in_sospeso && adesso >= self.stato_dovuto) {
                    self.campanello();
                }
                if adesso >= self.prossima_ancora {
                    self.manda_ancora();
                }
            }
            Fase::Libera { .. } if self.rete.is_some() => {
                if adesso >= self.prossima_scoperta {
                    self.cerca(adesso);
                }
                if adesso >= self.prossimo_collegamento {
                    self.collega_vicini(adesso);
                }
            }
            Fase::Membro(_) => {
                if self
                    .seguendo
                    .as_ref()
                    .and_then(|s| s.in_attesa.first())
                    .is_some_and(|prima| adesso >= prima.scade)
                {
                    self.manda_le_aggiunte();
                }
                self.guarda_l_aggancio(adesso);
            }
            _ => {}
        }
        if self
            .dal_filo
            .as_ref()
            .and_then(|d| d.ridisegna_dal)
            .is_some_and(|dal| adesso >= dal.saturating_add(RIDISEGNA_DOPO_MS))
        {
            self.ridisegna();
        }
    }

    /// Le riletture non riuscite, ritentate quando tocca (vedi
    /// [`RILEGGI_DOPO_MS`]), in ogni fase: una rilettura dei fidati serve
    /// anche a rete spenta. Poi la rete che all'avvio le aspettava.
    fn rileggi_se_tocca(&mut self, adesso: u64) {
        if self
            .impostazioni_da_rileggere
            .is_some_and(|quando| quando <= adesso)
        {
            let nome = self.nome.clone();
            self.ricarica_impostazioni();
            if self.nome != nome {
                self.nome_cambiato();
            }
        }
        if self
            .fidati_da_rileggere
            .is_some_and(|quando| quando <= adesso)
        {
            self.ricarica_fidati();
        }
        if self.accendi_dopo_la_lettura
            && self.impostazioni_da_rileggere.is_none()
            && self.fidati_letti
        {
            self.accendi_dopo_la_lettura = false;
            if self.impostazioni.attivo
                && let Err(err) = self.accendi()
            {
                nota!(
                    "[insieme] la rete non parte dopo la rilettura codice={}",
                    err.code().kind().code()
                );
                if let Some(chiave) = chiave_per_la_finestra(&err, None) {
                    self.avvisa(chiave, None);
                }
            }
        }
    }

    // ── il riduttore ────────────────────────────────────────────────────────

    /// Un evento al riduttore, e i suoi effetti eseguiti.
    fn nutri(&mut self, evento: Evento) {
        let (s, effetti) = fasi::passo(std::mem::take(&mut self.s), evento, ora());
        self.s = s;
        // L'uscita del lettore può durare (riapre la coda di prima, magari da
        // un disco di rete): la finestra intanto vede «uscendo».
        if matches!(self.s.fase, Fase::Uscendo { .. }) {
            self.pubblica();
        }
        for effetto in effetti {
            self.esegui(effetto);
        }
    }

    fn esegui(&mut self, effetto: Effetto) {
        match effetto {
            Effetto::Manda { a, messaggio } => self.manda(a, &messaggio),
            Effetto::Avviso { chiave, nome } => self.avvisa(&chiave, nome),
            Effetto::Errore(err) => {
                if self.errore.is_none() {
                    self.errore = Some(err);
                }
            }
            Effetto::MostraFinestra => mostra_finestra(&self.app),
            Effetto::Ospita => self.ospita(),
            Effetto::SmettiDiOspitare => {
                if let Some(accesa) = &self.rete {
                    accesa.rete.smetti_di_ospitare();
                    accesa.rete.revoca_tutto();
                    accesa.rete.ammessi(Vec::new());
                }
                self.servite.svuota();
                self.impronta_stato = None;
                self.stato_in_sospeso = false;
            }
            Effetto::EntraComeMembro { host } => self.entra_come_membro(host),
            Effetto::EsciDalLettore => self.esci_dal_lettore(),
            Effetto::FermaRete => self.ferma_la_rete(),
            Effetto::Sincronizza { id, attivo } => {
                if let Some(accesa) = &self.rete {
                    accesa.rete.sincronizza(id, attivo);
                }
            }
            Effetto::DirittiCambiati(diritti) => {
                // Il ruolo prima: vedi il `//!`. Solo se il lettore è entrato:
                // un'entrata non riuscita ha già rimesso «fuori».
                if matches!(
                    ruolo_dal_byte(self.comune.ruolo.load(Ordering::Acquire)),
                    Ruolo::Membro(_)
                ) {
                    self.ruolo(Ruolo::Membro(diritti));
                }
                nota!(
                    "[insieme] diritti di questo PC: {}",
                    nome_dei_diritti(diritti)
                );
            }
            Effetto::Specchia(stato) => self.specchia(*stato),
            Effetto::Segui(ancora) => {
                self.segui(ancora);
                // I clic della calibrazione si riconoscono dall'ancora (vedi
                // `fasi`): il campo del lettore va rifatto quando cominciano o
                // finiscono, non al prossimo stato.
                if let Fase::Membro(m) = &self.s.fase
                    && m.calibrazione != self.calibrazione
                {
                    self.calibrazione = m.calibrazione;
                    self.fotografa_membro();
                }
            }
            Effetto::Esegui {
                da,
                membro,
                comando,
                azione,
            } => self.esegui_comando(da, membro, comando, *azione),
            Effetto::MembroUscito { membro } => self.membro_uscito(membro),
            Effetto::PersoneCambiate => {
                // Prima chi può leggere, poi lo stato: chi è appena entrato
                // chiederà i byte appena lo riceve.
                self.aggiorna_ammessi();
                if let Some(istantanea) = ponte::istantanea(&self.app) {
                    self.manda_stato(&istantanea, true);
                }
                self.manda_ancora();
            }
            Effetto::AssociaInRete { id } => {
                let esito = self
                    .rete
                    .as_ref()
                    .map(|accesa| accesa.rete.associa(id).map_err(|e| senza_causa(&e)));
                if let Some(Err(err)) = esito {
                    self.errore.get_or_insert(err);
                    self.nutri(Evento::AssociazioneFinita { id });
                }
            }
            Effetto::ConfermaCodice { id, uguale } => {
                if let Some(accesa) = &self.rete {
                    accesa.rete.conferma_codice(id, uguale);
                }
            }
        }
    }

    // ── le voci che arrivano dal filo (tutti e due) ─────────────────────────

    /// La voce del lettore per una voce sul filo, come la vede questo PC.
    ///
    /// - Chi segue, per una voce **sua** (l'ha aggiunta lui): dal suo file, e
    ///   conta come suo ascolto.
    /// - Chi segue, per le altre: dalla rete, attraverso l'host.
    /// - Chi ospita, per le voci dei membri: dalla rete senza rete, cioè dai
    ///   gettoni inoltrati (`lettore_locale`).
    fn voce_registrabile(
        &self,
        v: &VoceSulFilo,
        pronta: bool,
        apri: &dyn Fn(Gettone) -> ponte::ApriInRete,
    ) -> ponte::Voce {
        match &self.s.fase {
            Fase::Membro(m) => {
                if v.di == m.io
                    && let Some((track_id, copertina)) =
                        self.seguendo.as_ref().and_then(|s| s.proprie.propria(v))
                {
                    return voci::voce_propria(v, track_id, copertina);
                }
                voci::voce_da_filo(v, voci::proprietario(&m.persone, v.di, m.io), pronta, apri)
            }
            _ => {
                let persone = fasi::persone_di_qui(&self.s, &self.nome);
                voci::voce_da_filo(v, voci::proprietario(&persone, v.di, 0), pronta, apri)
            }
        }
    }

    /// Registra nel lettore voci arrivate dal filo, e ricorda le loro
    /// maniglie (servono a «di Marco»). Le copertine che mancano si chiedono.
    fn registra_dal_filo(&mut self, dal_filo: Vec<VoceSulFilo>) {
        if dal_filo.is_empty() {
            return;
        }
        let Some(dal) = &self.dal_filo else {
            return;
        };
        let apri = apritore(&dal.lettore, &dal.sessione_finita);
        let mut da_chiedere = Vec::new();
        let mut nuove = Vec::with_capacity(dal_filo.len());
        {
            let mut copertine = self
                .comune
                .copertine
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            for v in &dal_filo {
                // Una voce propria ha la copertina della propria libreria:
                // scaricarla dall'host vorrebbe dire farla andare e tornare.
                let propria = matches!(&self.s.fase, Fase::Membro(m) if v.di == m.io)
                    && self
                        .seguendo
                        .as_ref()
                        .is_some_and(|s| s.proprie.propria(v).is_some());
                let pronta = v.copertina.is_some_and(|g| copertine.ha(&g));
                if let Some(gettone) = v.copertina.filter(|_| !propria) {
                    copertine.collega(v.uid, gettone);
                    if !pronta {
                        da_chiedere.push(gettone);
                    }
                }
                nuove.push(self.voce_registrabile(v, pronta, &apri));
            }
        }
        // Fuori da ogni lucchetto nostro: il ponte prende quello del lettore.
        let maniglie = ponte::registra(&self.app, nuove);
        let Some(dal) = &mut self.dal_filo else {
            return;
        };
        for gettone in da_chiedere {
            if dal.chieste.insert(gettone)
                && let Some(coda) = &dal.scaricatore
            {
                let _ = coda.send(gettone);
            }
        }
        for (posto, v) in dal_filo.into_iter().enumerate() {
            let maniglia = maniglie.get(posto).copied();
            dal.registrate.insert(v.uid, (v, maniglia));
        }
    }

    fn copertina_arrivata(&mut self, gettone: Gettone, byte: Option<Arc<[u8]>>) {
        let Some(dal) = &mut self.dal_filo else {
            return;
        };
        let Some(byte) = byte else {
            return;
        };
        let tenuta = self
            .comune
            .copertine
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .metti(gettone, byte);
        if tenuta {
            dal.da_ridisegnare.insert(gettone);
            dal.ridisegna_dal.get_or_insert(ora());
        }
    }

    /// Le voci la cui copertina è arrivata, registrate di nuovo con la chiave,
    /// e la finestra ridisegnata: `specchia` con la stessa coda non lo fa.
    fn ridisegna(&mut self) {
        let Some(dal) = &mut self.dal_filo else {
            return;
        };
        dal.ridisegna_dal = None;
        let arrivate = std::mem::take(&mut dal.da_ridisegnare);
        let voci: Vec<VoceSulFilo> = dal
            .registrate
            .values()
            .filter(|(v, _)| v.copertina.is_some_and(|g| arrivate.contains(&g)))
            .map(|(v, _)| v.clone())
            .collect();
        if voci.is_empty() {
            return;
        }
        self.registra_dal_filo(voci);
        // La coda della finestra tiene le righe per id e non le richiede: la
        // revisione nuova nel campo del lettore le fa rileggere, e le
        // copertine arrivate si vedono anche nelle righe già disegnate.
        self.revisione_voci = self.revisione_voci.wrapping_add(1);
        match self.s.fase {
            Fase::Ospita(_) => self.fotografa_host(),
            Fase::Membro(_) => self.fotografa_membro(),
            _ => {}
        }
        ponte::aggiorna_finestra(&self.app);
    }

    // ── tutti e due ─────────────────────────────────────────────────────────

    fn esci_dal_lettore(&mut self) {
        // Il ruolo prima: vedi il `//!`.
        self.ruolo(Ruolo::Fuori);
        if let Some(mut seguendo) = self.seguendo.take() {
            seguendo.condivisa.fermo.store(true, Ordering::Release);
            let _ = seguendo.sveglia.send(());
            if let Some(filo) = seguendo.filo.take() {
                self.in_uscita.push(filo);
            }
            // I gettoni delle voci proprie (fase 2) non valgono più.
            if let Some(accesa) = &self.rete {
                accesa.rete.revoca_tutto();
            }
        }
        if let Some(dal) = self.dal_filo.take() {
            // Le letture in rete in corso tornano subito; il filo delle
            // copertine finisce da sé quando il suo canale cade con `dal`.
            dal.sessione_finita.store(true, Ordering::Release);
        }
        // Sorvegliato da solo: il lettore rimette la coda di prima, salva e
        // ridisegna, e un panico lì dentro non deve saltare il resto, che
        // finisce con `Uscito`. Senza, la fase restava `Uscendo` per sempre,
        // l'ascolto aperto (`SmettiDiOspitare`, dopo, non si eseguiva) e la
        // finestra su «uscendo» fino al riavvio.
        crate::stato::sorvegliato("rymembra-insieme", || ponte::esci(&self.app));
        self.servite.svuota();
        self.preparazione.svuota();
        self.sessione = self.sessione.wrapping_add(1);
        self.sessione_viva.store(self.sessione, Ordering::Release);
        self.impronta_stato = None;
        self.stato_in_sospeso = false;
        self.aggancio_mostrato = None;
        self.calibrazione = false;
        self.corrente = None;
        self.locale = false;
        self.proprietari.clear();
        self.uid_dei_membri = 0;
        self.comune
            .copertine
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .svuota();
        // Dopo `esci`, che ha già ridisegnato col campo vecchio: senza questa
        // riga chi è appena uscito vedrebbe ancora i tasti della sessione
        // spenti (shuffle, radio, autoplay, timer).
        self.metti_nel_lettore(None);
        // I fili della sincronia finiti non servono più.
        self.in_uscita.retain(|f| !f.is_finished());
        nota!("[insieme] fuori dalla sessione");
        self.nutri(Evento::Uscito);
    }

    // ── fase 3: i descrittori ───────────────────────────────────────────────

    /// Il descrittore di un file è arrivato dal suo filo: le voci di quel file
    /// lo portano da qui in poi. Chi ospita ripubblica lo stato; chi segue
    /// manda le aggiunte che lo aspettavano.
    fn descrittore_arrivato(&mut self, file: voci::ChiaveFile, copia: Option<DescrittoreCopia>) {
        // Uno stato per ogni descrittore era un campanello e uno stato intero
        // per file: mille brani nuovi in coda, e il filo restava occupato per
        // decine di secondi (misurato: fino a otto secondi di posta in fila,
        // e un «indietro» di un membro risposto dopo dieci). Il descrittore
        // aspetta il giro lento delle descrizioni, al più un secondo.
        if self.servite.imposta_copia(&file, copia.clone()) {
            self.copie = self.copie.wrapping_add(1);
            self.descrizioni_da_mandare();
        }
        if let Some(seguendo) = &mut self.seguendo {
            seguendo.proprie.imposta_copia(&file, copia);
            self.manda_le_aggiunte();
        }
    }

    fn ferma(&mut self) {
        let inizio = std::time::Instant::now();
        // Si esce, o ci si aggiorna: una rilettura che riesce adesso non deve
        // riaccendere la rete. La rimette com'era `Posta::Riaccendi`.
        self.accendi_dopo_la_lettura = false;
        if self.s.fase != Fase::Spenta {
            self.nutri(Evento::Spegni);
        }
        // Se l'uscita del lettore non è passata dal riduttore — una rete già
        // spenta, per dire — il ruolo resta comunque fuori.
        self.ruolo(Ruolo::Fuori);
        while self.in_uscita.iter().any(|f| !f.is_finished()) && inizio.elapsed() < ATTESA_FILI {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.in_uscita.retain(|f| !f.is_finished());
    }

    fn ruolo(&self, ruolo: Ruolo) {
        self.comune
            .ruolo
            .store(ruolo_in_byte(ruolo), Ordering::Release);
    }

    fn avvisa(&self, chiave: &str, nome: Option<String>) {
        self.app.emetti(
            "insieme:avviso",
            AvvisoIpc {
                chiave: chiave.to_owned(),
                nome,
            },
        );
    }

    fn libreria(&self) -> Result<tauri::State<'_, Stato>, AppError> {
        self.app.try_state::<Stato>().ok_or_else(|| {
            AppError::new(ErrorCode::InternalAborted {
                what: Some("insieme".to_owned()),
            })
        })
    }

    /// Rilegge le impostazioni. Con la libreria occupata restano quelle in
    /// memoria — ogni cambio fatto da qui è già lì, dopo la sua scrittura — e
    /// si ritenta: vedi [`RILEGGI_DOPO_MS`]. Nel diario la prima volta.
    fn ricarica_impostazioni(&mut self) {
        match self.in_libreria(ATTESA_LIBRERIA, rymembra_app::insieme::impostazioni) {
            Ok(impostazioni) => {
                self.impostazioni = impostazioni;
                self.impostazioni_da_rileggere = None;
            }
            Err(err) => {
                if self.impostazioni_da_rileggere.is_none() {
                    nota!(
                        "[insieme] impostazioni non rilette codice={}: si riprova",
                        err.code().kind().code()
                    );
                }
                self.impostazioni_da_rileggere = Some(ora().saturating_add(RILEGGI_DOPO_MS));
            }
        }
        self.nome = nome_effettivo(&self.impostazioni);
    }

    /// Il nome di questo PC è cambiato: al trasporto, e a chi è in sessione.
    fn nome_cambiato(&mut self) {
        if let Some(accesa) = &self.rete {
            accesa.rete.rinomina(&self.nome);
        }
        if matches!(self.s.fase, Fase::Ospita(_)) {
            self.esegui(Effetto::PersoneCambiate);
        }
    }

    /// Rilegge i fidati e li passa al trasporto (`Rete::pari`).
    ///
    /// Con la libreria occupata restano quelli in memoria, dove Blocca e
    /// Dimentica hanno già messo il loro cambio (vedi [`Fidati::segna_bloccato`]),
    /// e il trasporto li riceve lo stesso; la rilettura si ritenta (vedi
    /// [`RILEGGI_DOPO_MS`]). I nomi cambiati — una «Rinomina» la cui
    /// rilettura non era riuscita, una riassociazione — vanno anche sulle
    /// connessioni aperte.
    fn ricarica_fidati(&mut self) {
        match self.in_libreria(ATTESA_LIBRERIA, rymembra_app::insieme::pari) {
            Ok(elenco) => {
                let prima = std::mem::replace(&mut self.fidati, Fidati::da(elenco));
                self.fidati_da_rileggere = None;
                self.fidati_letti = true;
                let rinominati: Vec<(i64, String)> = self
                    .fidati
                    .elenco()
                    .iter()
                    .filter(|p| prima.per_id(p.id).is_some_and(|q| q.nome != p.nome))
                    .map(|p| (p.id, p.nome.clone()))
                    .collect();
                for (pari, nome) in rinominati {
                    self.nutri(Evento::Rinominato { pari, nome });
                }
            }
            Err(err) => {
                if self.fidati_da_rileggere.is_none() {
                    nota!(
                        "[insieme] PC associati non riletti codice={}: si riprova",
                        err.code().kind().code()
                    );
                }
                self.fidati_da_rileggere = Some(ora().saturating_add(RILEGGI_DOPO_MS));
            }
        }
        if let Some(accesa) = &self.rete {
            let (fidate, bloccate) = self.fidati.per_la_rete();
            accesa.rete.pari(fidate, bloccate);
        }
    }

    fn nome_della_connessione(&self, id: IdConnessione) -> String {
        self.s
            .connessioni
            .get(&id)
            .map(|c| c.nome.clone())
            .unwrap_or_default()
    }

    // ── i comandi ───────────────────────────────────────────────────────────

    fn comando(&mut self, richiesta: Richiesta) -> Result<(), AppError> {
        match richiesta {
            Richiesta::Attiva(true) => {
                self.scrivi(|c| rymembra_app::insieme::salva_attivo(c, true))?;
                self.impostazioni.attivo = true;
                if let Err(err) = self.accendi() {
                    // L'interruttore torna giù con l'errore accanto: acceso e
                    // senza rete sarebbe un interruttore che mente.
                    let _ = self.scrivi(|c| rymembra_app::insieme::salva_attivo(c, false));
                    self.impostazioni.attivo = false;
                    return Err(err);
                }
                Ok(())
            }
            Richiesta::Attiva(false) => {
                self.scrivi(|c| rymembra_app::insieme::salva_attivo(c, false))?;
                self.impostazioni.attivo = false;
                self.nutri(Evento::Spegni);
                Ok(())
            }
            Richiesta::Nome(nome) => {
                self.scrivi(|c| rymembra_app::insieme::salva_nome(c, &nome))?;
                // Se la rilettura non riesce, il nome nuovo lo porta quella
                // ritentata (`rileggi_se_tocca`).
                self.ricarica_impostazioni();
                self.nome_cambiato();
                Ok(())
            }
            Richiesta::FirewallVisto => {
                self.scrivi(rymembra_app::insieme::segna_firewall_spiegato)?;
                self.impostazioni.firewall_spiegato = true;
                Ok(())
            }
            Richiesta::Ospita => {
                self.nutri(Evento::Comando(Comando::Ospita));
                Ok(())
            }
            Richiesta::Termina => {
                self.nutri(Evento::Comando(Comando::Termina));
                Ok(())
            }
            Richiesta::Esci => {
                self.nutri(Evento::Comando(Comando::Esci));
                Ok(())
            }
            Richiesta::Associa(vicino) => {
                let id = fiducia::impronta_da_testo(&vicino)
                    .and_then(|impronta| self.vicinato.conn_di(impronta))
                    .ok_or_else(|| AppError::new(ErrorCode::InsiemeIrraggiungibile))?;
                self.nutri(Evento::Comando(Comando::Associa { id }));
                Ok(())
            }
            Richiesta::Codice {
                associazione,
                uguale,
            } => {
                self.nutri(Evento::Comando(Comando::ConfermaCodice {
                    associazione,
                    uguale,
                }));
                Ok(())
            }
            Richiesta::Invita { pari, diritti } => {
                self.fidato_attivo(pari)?;
                self.nutri(Evento::Comando(Comando::Invita { pari, diritti }));
                Ok(())
            }
            Richiesta::Rispondi { invito, accetta } => {
                self.nutri(Evento::Comando(Comando::Rispondi { invito, accetta }));
                Ok(())
            }
            Richiesta::Chiedi(pari) => {
                self.fidato_attivo(pari)?;
                self.nutri(Evento::Comando(Comando::Chiedi { pari }));
                Ok(())
            }
            Richiesta::Ammetti {
                richiesta,
                accetta,
                diritti,
            } => {
                self.nutri(Evento::Comando(Comando::Ammetti {
                    richiesta,
                    accetta,
                    diritti,
                }));
                Ok(())
            }
            Richiesta::Allontana(membro) => {
                self.nutri(Evento::Comando(Comando::Allontana { membro }));
                Ok(())
            }
            Richiesta::Concedi { membro, diritti } => {
                nota!(
                    "[insieme] diritti del membro #{membro}: {}",
                    nome_dei_diritti(diritti)
                );
                self.nutri(Evento::Comando(Comando::Concedi { membro, diritti }));
                Ok(())
            }
            Richiesta::Rinomina { id, nome } => {
                self.scrivi(|c| rymembra_app::insieme::rinomina(c, id, &nome))?;
                // Il nome com'è stato scritto (la libreria lo ripulisce) arriva
                // con la rilettura, anche sulle connessioni aperte; con la
                // libreria occupata, con quella ritentata.
                self.ricarica_fidati();
                Ok(())
            }
            // Fidati in memoria per primi: il cambio è scritto, e non deve
            // dipendere da una rilettura che con la libreria occupata non
            // riesce (prima il PC bloccato restava fidato fino al riavvio).
            // Poi la sessione, e la rete per ultima: il `Fine` (o l'`Esco`)
            // che il riduttore manda al PC tolto deve partire mentre la sua
            // chiave è ancora fidata nel trasporto. Dopo `Rete::pari` un PC
            // bloccato è già chiuso, e dall'altra parte si leggerebbe
            // «sparito» invece del perché.
            Richiesta::Dimentica(id) => {
                self.scrivi(|c| rymembra_app::insieme::dimentica(c, id))?;
                nota!("[insieme] dimenticato pari #{id}");
                self.fidati.togli(id);
                self.nutri(Evento::PariTolto { pari: id });
                self.ricarica_fidati();
                Ok(())
            }
            Richiesta::Blocca { id, bloccato } => {
                self.scrivi(|c| rymembra_app::insieme::blocca(c, id, bloccato))?;
                nota!(
                    "[insieme] pari #{id} {}",
                    if bloccato { "bloccato" } else { "sbloccato" }
                );
                self.fidati.segna_bloccato(id, bloccato);
                if bloccato {
                    self.nutri(Evento::PariTolto { pari: id });
                }
                self.ricarica_fidati();
                Ok(())
            }
            Richiesta::Indirizzi(indirizzi) => {
                let salvati =
                    self.scrivi(|c| rymembra_app::insieme::salva_indirizzi(c, &indirizzi))?;
                self.impostazioni.indirizzi = salvati;
                self.prossima_scoperta = 0;
                self.vicinato.riprova_subito(ora());
                self.prossimo_collegamento = 0;
                Ok(())
            }
            Richiesta::Calibra(attiva) => {
                if !matches!(self.s.fase, Fase::Ospita(_)) {
                    return Err(AppError::new(ErrorCode::InsiemeGestoNonDisponibile {
                        gesto: "calibra".to_owned(),
                    }));
                }
                ponte::calibra(&self.app, attiva)?;
                self.calibrazione = ponte::calibrando(&self.app);
                self.fotografa_host();
                nota!(
                    "[insieme] calibrazione {}",
                    if attiva { "accesa" } else { "spenta" }
                );
                // A chi segue lo dicono le ancore (vedi `fasi`); qui, a chi
                // ospita, solo la fine: l'inizio l'ha appena chiesto lui.
                if !attiva {
                    self.avvisa(fasi::AVVISO_CALIBRAZIONE_SPENTA, None);
                }
                Ok(())
            }
        }
    }

    /// Una scrittura nella libreria, in una finestra corta.
    fn scrivi<T>(
        &self,
        azione: impl FnOnce(&rusqlite::Connection) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        self.in_libreria(ATTESA_LIBRERIA, azione)
    }

    /// La libreria, aspettata al più `entro`: vedi [`ATTESA_LIBRERIA`] e il
    /// `//!`. Non presa in tempo è `db.locked`, che si può ritentare.
    fn in_libreria<T>(
        &self,
        entro: Duration,
        azione: impl FnOnce(&rusqlite::Connection) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let stato = self.libreria()?;
        crate::stato::con_libreria_entro(&stato, entro, |l| azione(&l.connection)).unwrap_or_else(
            || {
                Err(AppError::new(ErrorCode::DbLocked)
                    .with_cause("la libreria è occupata: la sessione non l'aspetta"))
            },
        )
    }

    /// Scrive il campo `insieme` dello stato del lettore e, se è cambiato, fa
    /// ridisegnare la finestra: `manda_stato` del lettore l'ha già letto prima
    /// di questa scrittura (all'ingresso, all'uscita, a ogni cambio di voce),
    /// e nessuno lo rileggerebbe. `aggiorna_finestra` non suona il campanello
    /// della sessione: niente anelli.
    fn metti_nel_lettore(&self, foto: Option<InsiemeNelLettore>) {
        let cambiata = {
            let mut cella = self
                .comune
                .nel_lettore
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let cambiata = diversi(&cella, &foto);
            *cella = foto;
            cambiata
        };
        // A lucchetto lasciato: il lettore, ridisegnando, rilegge questa cella.
        if cambiata {
            ponte::aggiorna_finestra(&self.app);
        }
    }

    /// Il fidato c'è e non è bloccato.
    fn fidato_attivo(&self, pari: i64) -> Result<(), AppError> {
        match self.fidati.per_id(pari) {
            Some(p) if !p.bloccato => Ok(()),
            _ => Err(AppError::new(ErrorCode::InsiemeNonAssociato)),
        }
    }

    // ── la finestra ─────────────────────────────────────────────────────────

    fn pubblica(&mut self) {
        let aggancio = match (&self.s.fase, &self.seguendo) {
            (Fase::Membro(_), Some(seguendo)) => Some(
                self.aggancio_mostrato
                    .unwrap_or_else(|| seguendo.condivisa.aggancio()),
            ),
            _ => None,
        };
        let ipc = vetrina(&Parti {
            s: &self.s,
            impostazioni: &self.impostazioni,
            nome: &self.nome,
            fidati: &self.fidati,
            vicinato: &self.vicinato,
            aggancio,
            calibrazione: match &self.s.fase {
                Fase::Membro(m) => m.calibrazione,
                _ => self.calibrazione,
            },
            proprietari: &self.proprietari,
            indirizzi_di_qui: &self.indirizzi_di_qui,
            adesso_ms: adesso_ms(),
            ora: ora(),
        });
        let json = serde_json::to_string(&ipc).unwrap_or_default();
        *self
            .comune
            .vetrina
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = ipc.clone();
        if self.pubblicata.as_deref() != Some(json.as_str()) {
            self.pubblicata = Some(json);
            self.app.emetti("insieme:stato", ipc);
        }
    }
}

/// Chi apre le voci in rete di una sessione: un [`super::flusso::DaPari`] per
/// gettone, sul lettore della sessione.
fn apritore(
    lettore: &LettoreRemoto,
    sessione_finita: &Arc<AtomicBool>,
) -> impl Fn(Gettone) -> ponte::ApriInRete + use<> {
    let lettore = lettore.clone();
    let finita = Arc::clone(sessione_finita);
    move |gettone: Gettone| -> ponte::ApriInRete {
        let lettore = lettore.clone();
        let finita = Arc::clone(&finita);
        Arc::new(move || {
            let sorgente: Box<dyn rymembra_net::flusso::Sorgente> = Box::new(
                super::flusso::DaPari::nuovo(lettore.clone(), gettone, Arc::clone(&finita)),
            );
            Ok(sorgente)
        })
    }
}

/// Il filo che scarica le copertine, fuori da ogni lucchetto: una per volta,
/// nell'ordine in cui le voci sono arrivate.
fn avvia_le_copertine(lettore: LettoreRemoto, posta: Sender<Posta>) -> Option<Sender<Gettone>> {
    let (coda, orecchio) = channel::<Gettone>();
    let avviato = std::thread::Builder::new()
        .name("rymembra-insieme-copertine".to_owned())
        .spawn(move || {
            for gettone in orecchio {
                let byte = lettore
                    .copertina(&gettone)
                    .ok()
                    .and_then(|b| voci::copertina_accettata(&b));
                if posta.send(Posta::Copertina { gettone, byte }).is_err() {
                    return;
                }
            }
        });
    match avviato {
        Ok(_) => Some(coda),
        Err(err) => {
            nota!("[insieme] il filo delle copertine non parte: {err}");
            None
        }
    }
}

/// Due valori del campo `insieme` del lettore sono diversi? Per serializzazione:
/// il tipo è della finestra e non sa confrontarsi, e quel che conta è quel che
/// la finestra riceverebbe.
fn diversi(a: &Option<InsiemeNelLettore>, b: &Option<InsiemeNelLettore>) -> bool {
    serde_json::to_string(a).ok() != serde_json::to_string(b).ok()
}

/// Fase 3: il filo che calcola i descrittori, fuori dal filo della sessione.
///
/// Legge 128 KiB per file (`rymembra_app::insieme::descrittore`) e la chiave
/// di contenuto della riga: un disco di rete che dorme fa aspettare lui, non
/// la sessione. Tiene i descrittori già fatti per file — percorso, dimensione
/// e data — per tutta la vita del processo: un brano ripreso in un'altra
/// sessione non si rilegge. Uno per volta, nell'ordine chiesto.
fn avvia_i_descrittori(
    app: AppHandle,
    posta: Sender<Posta>,
) -> Option<Sender<(voci::ChiaveFile, i64)>> {
    let (coda, orecchio) = channel::<(voci::ChiaveFile, i64)>();
    let avviato = std::thread::Builder::new()
        .name("rymembra-insieme-descrittori".to_owned())
        .spawn(move || {
            let mut fatti: HashMap<voci::ChiaveFile, Option<DescrittoreCopia>> = HashMap::new();
            for (file, track_id) in orecchio {
                if crate::spegnimento::in_uscita() {
                    return;
                }
                let copia = match fatti.get(&file) {
                    Some(copia) => copia.clone(),
                    None => {
                        let chiave = chiave_di_contenuto(&app, track_id);
                        let copia =
                            rymembra_app::insieme::descrittore(&file.percorso, chiave.as_deref())
                                .ok()
                                .and_then(voci::descrittore_sul_filo);
                        fatti.insert(file.clone(), copia.clone());
                        copia
                    }
                };
                if posta.send(Posta::Descrittore { file, copia }).is_err() {
                    return;
                }
            }
        });
    match avviato {
        Ok(_) => Some(coda),
        Err(err) => {
            nota!("[insieme] il filo dei descrittori non parte: {err}");
            None
        }
    }
}

/// La chiave di contenuto di una riga, se ce n'è una: una finestra corta
/// sulla libreria.
fn chiave_di_contenuto(app: &AppHandle, track_id: i64) -> Option<String> {
    let stato = app.try_state::<Stato>()?;
    con_libreria(&stato, |l| {
        use rusqlite::OptionalExtension as _;
        l.connection
            .query_row(
                "SELECT content_key FROM tracks WHERE id = ?1",
                [track_id],
                |riga| riga.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|err| {
                AppError::new(ErrorCode::DbQueryFailed {
                    detail: Some("content_key".to_owned()),
                })
                .with_cause(err.to_string())
            })
    })
    .ok()
    .flatten()
    .flatten()
}

/// La chiave d'avviso per un errore: la sua, se è un codice della sessione;
/// altrimenti quella di `ripiego`, o nessuna. La finestra mostra solo le
/// chiavi `together.notice.*` ed `errors.insieme.*` (vedi
/// `richieste-interfaccia.md` §5) e scarta le altre in silenzio: un
/// `db.locked` mandato qui sarebbe un guasto che nessuno vede.
fn chiave_per_la_finestra(err: &AppError, ripiego: Option<ErrorCodeKind>) -> Option<&'static str> {
    let kind = err.code().kind();
    if kind.domain() == rymembra_domain::errors::Domain::Insieme {
        return Some(kind.i18n_key());
    }
    ripiego.map(ErrorCodeKind::i18n_key)
}

/// Porta la finestra davanti se è nascosta nel vassoio.
fn mostra_finestra(app: &AppHandle) {
    let nascosta = app
        .get_webview_window("main")
        .is_some_and(|f| !f.is_visible().unwrap_or(true));
    if nascosta {
        crate::vassoio::mostra(app);
    }
}

/// La riga del diario per un evento della rete, se ne merita una. Pura: vedi
/// il `//!` per quel che non ci va.
#[must_use]
pub(super) fn riga_della_rete(
    evento: &rymembra_insieme::Evento,
    fidati: &Fidati,
) -> Option<String> {
    use rymembra_insieme::Evento as DallaRete;
    let chi = |chiave: &ChiavePubblica| {
        fidati
            .per_chiave(chiave)
            .map_or_else(|| "estraneo".to_owned(), |p| format!("pari #{}", p.id))
    };
    match evento {
        DallaRete::Aperta {
            id,
            pari,
            ospita,
            in_entrata,
            ..
        } => Some(format!(
            "[insieme] connessione #{} aperta {} con {}{}",
            id.0,
            if *in_entrata {
                "in entrata"
            } else {
                "in uscita"
            },
            chi(pari),
            if *ospita { " (ospita)" } else { "" }
        )),
        DallaRete::Chiusa { id, codice } => Some(format!(
            "[insieme] connessione #{} chiusa codice={}",
            id.0,
            codice.map_or("—", ErrorCodeKind::code)
        )),
        DallaRete::Associato { id, .. } => Some(format!(
            "[insieme] associazione riuscita sulla connessione #{}",
            id.0
        )),
        DallaRete::Codice { id, .. } => Some(format!(
            "[insieme] codice di associazione pronto sulla connessione #{}",
            id.0
        )),
        DallaRete::AssociazioneRimandata { id, codice } => Some(format!(
            "[insieme] associazione rimandata sulla connessione #{} codice={}",
            id.0,
            codice.code()
        )),
        DallaRete::Trovato { .. } | DallaRete::Ricevuto { .. } | DallaRete::Orologio { .. } => None,
    }
}

// ── le fotografie ───────────────────────────────────────────────────────────

/// Chi ospita, per il lettore. `locale`: la voce corrente si legge da un file
/// di questo PC.
fn nel_lettore_di_chi_ospita(
    nome: &str,
    persone: usize,
    proprietario: Option<String>,
    dettagli: Dettagli,
) -> InsiemeNelLettore {
    InsiemeNelLettore {
        ospita: true,
        host: nome.to_owned(),
        persone: u32::try_from(persone).unwrap_or(u32::MAX),
        proprietario,
        locale: dettagli.locale,
        // Chi ospita è l'orologio di tutti: agganciato per definizione.
        agganciato: true,
        diritti: Diritti::Tutto,
        calibrazione: dettagli.calibrazione,
        revisione: dettagli.revisione,
    }
}

/// Chi ospita: di chi è la voce che suona. Il nome del membro che l'ha
/// aggiunta — lo stesso di «di Marco» in `proprietari` — e `None` per una voce
/// di questo PC, per la calibrazione, o senza una voce corrente.
fn proprietario_di_chi_ospita(
    corrente: Option<u64>,
    servite: &Servite,
    persone: &[Persona],
) -> Option<String> {
    let voce = servite.voce(corrente?)?;
    voci::proprietario(persone, voce.di, 0)
}

/// Quel che di chi segue cambia da sé, fuori dallo stato.
#[derive(Debug, Clone, Copy)]
struct Dettagli {
    agganciato: bool,
    /// La voce che suona si legge da un file di qui (fase 3, o una voce sua).
    locale: bool,
    calibrazione: bool,
    /// Vedi `InsiemeNelLettore::revisione`.
    revisione: u64,
}

/// Chi segue, per il lettore.
fn nel_lettore_di_chi_segue(
    host: &str,
    persone: &[Persona],
    io: u32,
    diritti: Diritti,
    corrente: Option<&VoceSulFilo>,
    dettagli: Dettagli,
) -> InsiemeNelLettore {
    InsiemeNelLettore {
        ospita: false,
        host: host.to_owned(),
        persone: u32::try_from(persone.len()).unwrap_or(u32::MAX),
        proprietario: corrente.and_then(|v| voci::proprietario(persone, v.di, io)),
        locale: dettagli.locale,
        agganciato: dettagli.agganciato,
        // I gesti di chi segue vanno all'host, che fa quelli che ha concesso.
        diritti,
        calibrazione: dettagli.calibrazione,
        revisione: dettagli.revisione,
    }
}

/// «di Marco»: per ogni voce della coda che non è di questo PC, la sua
/// maniglia e il nome del proprietario. `di` dice, per un `uid`, maniglia e
/// numero del proprietario, o `None` per una voce di questo PC.
fn proprietari(
    coda: &[u64],
    persone: &[Persona],
    di: impl Fn(u64) -> Option<(i64, u32)>,
) -> Vec<ProprietarioIpc> {
    coda.iter()
        .filter_map(|uid| {
            let (id, membro) = di(*uid)?;
            let nome = persone.iter().find(|p| p.membro == membro)?.nome.clone();
            Some(ProprietarioIpc { id, nome })
        })
        .collect()
}

/// I pezzi da cui si fa la vetrina.
struct Parti<'a> {
    s: &'a Situazione,
    impostazioni: &'a rymembra_app::insieme::Impostazioni,
    nome: &'a str,
    fidati: &'a Fidati,
    vicinato: &'a Vicinato,
    aggancio: Option<Aggancio>,
    calibrazione: bool,
    proprietari: &'a [ProprietarioIpc],
    indirizzi_di_qui: &'a [String],
    adesso_ms: i64,
    ora: u64,
}

/// La vetrina prima che il filo abbia letto qualcosa.
pub(super) fn vetrina_spenta() -> StatoInsiemeIpc {
    StatoInsiemeIpc {
        attivo: false,
        nome: String::new(),
        fase: FaseIpc::Spenta,
        porta: None,
        firewall_spiegato: false,
        sessione: None,
        vicini: Vec::new(),
        fidati: Vec::new(),
        associazione: None,
        inviti: Vec::new(),
        richieste: Vec::new(),
        indirizzi: Vec::new(),
        indirizzi_di_qui: Vec::new(),
        sincronia: None,
    }
}

/// Il pannello, dai pezzi. Pura.
fn vetrina(p: &Parti<'_>) -> StatoInsiemeIpc {
    let s = p.s;
    let in_epoca = |scade: u64| {
        p.adesso_ms
            .saturating_add(i64::try_from(scade.saturating_sub(p.ora)).unwrap_or(i64::MAX))
    };
    let nome_conn = |id: &IdConnessione| {
        s.connessioni
            .get(id)
            .map(|c| c.nome.clone())
            .unwrap_or_default()
    };
    let pari_conn = |id: &IdConnessione| s.connessioni.get(id).and_then(|c| c.pari).unwrap_or(0);
    let fase = match &s.fase {
        Fase::Spenta => FaseIpc::Spenta,
        Fase::Libera { .. } => FaseIpc::Libera,
        Fase::Entrando { .. } => FaseIpc::Entrando,
        Fase::Ospita(_) => FaseIpc::Ospita,
        Fase::Membro(_) => FaseIpc::Membro,
        Fase::Uscendo { .. } => FaseIpc::Uscendo,
    };
    let (sessione, porta, richieste, in_sessione) = match &s.fase {
        Fase::Ospita(o) => (
            Some(SessioneIpc {
                host: p.nome.to_owned(),
                mia: true,
                persone: fasi::persone_di_qui(s, p.nome)
                    .into_iter()
                    .map(|persona| PersonaIpc {
                        io: persona.membro == 0,
                        membro: persona.membro,
                        nome: persona.nome,
                        diritti: persona.diritti,
                    })
                    .collect(),
                calibrazione: p.calibrazione,
                proprietari: p.proprietari.to_vec(),
            }),
            Some(o.porta),
            o.richieste
                .iter()
                .map(|r| RichiestaIpc {
                    id: r.numero,
                    da: nome_conn(&r.da),
                    pari: pari_conn(&r.da),
                    scade_ms: in_epoca(r.scade),
                })
                .collect(),
            o.membri.iter().map(|m| m.conn).collect::<Vec<_>>(),
        ),
        Fase::Membro(m) => (
            Some(SessioneIpc {
                host: nome_conn(&m.host),
                mia: false,
                persone: m
                    .persone
                    .iter()
                    .map(|persona| PersonaIpc {
                        io: persona.membro == m.io,
                        membro: persona.membro,
                        nome: persona.nome.clone(),
                        diritti: persona.diritti,
                    })
                    .collect(),
                calibrazione: p.calibrazione,
                proprietari: p.proprietari.to_vec(),
            }),
            None,
            Vec::new(),
            vec![m.host],
        ),
        // Mentre si entra la fascia dice già da chi: l'host c'è, le persone
        // le dirà il primo stato.
        Fase::Entrando { host, .. } => (
            Some(SessioneIpc {
                host: nome_conn(host),
                mia: false,
                persone: Vec::new(),
                calibrazione: false,
                proprietari: Vec::new(),
            }),
            None,
            Vec::new(),
            Vec::new(),
        ),
        _ => (None, None, Vec::new(), Vec::new()),
    };
    let inviti = match &s.fase {
        Fase::Libera { inviti } => inviti
            .iter()
            .map(|i| InvitoIpc {
                id: i.numero,
                da: nome_conn(&i.da),
                pari: pari_conn(&i.da),
                scade_ms: in_epoca(i.scade),
            })
            .collect(),
        _ => Vec::new(),
    };
    let fidati = p
        .fidati
        .elenco()
        .iter()
        .map(|pari| {
            let sue: Vec<(&IdConnessione, &Connessione)> = s
                .connessioni
                .iter()
                .filter(|(_, c)| c.pari == Some(pari.id))
                .collect();
            FidatoIpc {
                id: pari.id,
                nome: pari.nome.clone(),
                impronta: rymembra_insieme::ChiavePubblica::da_testo(&pari.chiave)
                    .map(|c| c.impronta().to_string())
                    .unwrap_or_default(),
                bloccato: pari.bloccato,
                associato_ms: pari.associato_ms,
                visto_ms: pari.visto_ms,
                presente: !sue.is_empty(),
                ospita: sue.iter().any(|(_, c)| c.ospita && !c.in_entrata),
                in_sessione: sue.iter().any(|(id, _)| in_sessione.contains(id)),
            }
        })
        .collect();
    let vicini = p
        .vicinato
        .visibili()
        .map(|(impronta, v)| {
            let fidato = p.fidati.per_impronta(&impronta);
            VicinoIpc {
                chiave: fiducia::impronta_in_testo(impronta),
                impronta: impronta.to_string(),
                nome: v.nome.clone(),
                pari: fidato.map(|f| f.id),
                bloccato: fidato.is_some_and(|f| f.bloccato),
            }
        })
        .collect();
    StatoInsiemeIpc {
        attivo: p.impostazioni.attivo,
        nome: p.nome.to_owned(),
        fase,
        porta,
        firewall_spiegato: p.impostazioni.firewall_spiegato,
        sessione,
        vicini,
        fidati,
        associazione: s.associazione.as_ref().map(|a| AssociazioneIpc {
            id: a.conn.0,
            nome: nome_conn(&a.conn),
            codice: a.codice.clone(),
            confermato: a.confermato,
        }),
        inviti,
        richieste,
        indirizzi: p.impostazioni.indirizzi.clone(),
        indirizzi_di_qui: if matches!(s.fase, Fase::Ospita(_)) {
            p.indirizzi_di_qui.to_vec()
        } else {
            Vec::new()
        },
        sincronia: p.aggancio.map(|a| SincroniaIpc {
            agganciato: a.agganciato,
            // Al decimo di millisecondo: più fine è rumore, e un numero che
            // cambia a ogni evento ridisegnerebbe il pannello per niente.
            errore_ms: a.errore_ns.map(|e| (e as f64 / 100_000.0).round() / 10.0),
            incertezza_ms: a
                .incertezza_ns
                .map(|e| (e as f64 / 100_000.0).round() / 10.0),
        }),
    }
}

#[cfg(test)]
mod prove;
