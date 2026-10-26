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

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use rymembra_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use rymembra_insieme::messaggi::{
    AncoraSulFilo, Azione, DescrittoreCopia, Diritti, Dove, MASSIMO_AGGIUNTE, MASSIMO_NOME,
    Messaggio, Persona, SorgenteSulFilo, StatoSessione, VoceSulFilo,
};
use rymembra_insieme::{ChiavePubblica, Gettone, IdConnessione, Impostazioni, LettoreRemoto, Rete};
use rymembra_play::sincronia::orologio::Scambio;
use rymembra_play::sincronia::regolatore::DallOspite;
use tauri::{AppHandle, Manager as _};

use super::fasi::{self, Comando, Connessione, Effetto, Evento, Fase, Situazione};
use super::fiducia::{self, Fidati, Vicinato};
use super::gesti::{self, Passo};
use super::preparazione::{self, Preparazione};
use super::segui::{self, Aggancio, Condivisa};
use super::strada::Ruolo;
use super::voci::{self, Copertine, Servite, Servizio};
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
/// rinuncia con `db.locked`. Un comando della persona lo ripeterà; intanto le
/// ancore non si sono fermate dietro a una scansione.
const ATTESA_LIBRERIA: Duration = Duration::from_secs(2);

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
        /// Il gesto, per il diario: vedi [`gesti::nome_dell_azione`].
        nome: &'static str,
        /// Quanto è rimasto in coda prima che l'esecutore lo prendesse, e
        /// quanto ci ha messo: il diario li scrive sopra [`LENTO`].
        attesa: Duration,
        durata: Duration,
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
                // Un panico in un giro non porta via la sessione intera: il
                // giro dopo trova la situazione com'era prima del colpo.
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
        if self.impostazioni.attivo
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
            } => {
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

    // ── la rete ─────────────────────────────────────────────────────────────

    fn accendi(&mut self) -> Result<(), AppError> {
        if self.rete.is_some() {
            return Ok(());
        }
        let dispositivo = self.in_libreria(ATTESA_LIBRERIA, crate::nuvola::dispositivo)?;
        let identita = fiducia::identita(&rymembra_oauth::portachiavi::DiSistema, &dispositivo)?;
        let mia = identita.pubblica();
        let (rete, eventi) = Rete::avvia(Impostazioni {
            identita,
            nome: self.nome.clone(),
            lega: lega(),
            porta: rymembra_insieme::PORTA,
            adesso_ns: rymembra_play::tempo::adesso_ns,
        })
        .map_err(|e| senza_causa(&e))?;
        self.generazione = self.generazione.wrapping_add(1);
        let generazione = self.generazione;
        let posta = self.posta.clone();
        let girato = std::thread::Builder::new()
            .name("rymembra-insieme-rete".to_owned())
            .spawn(move || {
                // Finisce quando la rete smette di mandare (fermata) o quando
                // il filo della sessione non c'è più.
                for evento in eventi {
                    if posta
                        .send(Posta::Rete {
                            generazione,
                            evento,
                            arrivato: Instant::now(),
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            });
        if let Err(err) = girato {
            rete.ferma();
            return Err(
                AppError::new(ErrorCode::InternalUnexpected { detail: None })
                    .with_cause(format!("il filo degli eventi della rete non parte: {err}")),
            );
        }
        let (fidate, bloccate) = self.fidati.per_la_rete();
        rete.pari(fidate, bloccate);
        self.rete = Some(Accesa { rete, mia });
        nota!(
            "[insieme] rete accesa, {} fidati{}",
            self.fidati.elenco().len(),
            if lega().is_loopback() {
                ", solo locale"
            } else {
                ""
            }
        );
        self.prossima_scoperta = 0;
        self.prossimo_collegamento = 0;
        self.nutri(Evento::Acceso);
        Ok(())
    }

    fn ferma_la_rete(&mut self) {
        if let Some(accesa) = self.rete.take() {
            accesa.rete.ferma();
            let tutti = accesa.rete.aspetta_fili(ATTESA_RETE);
            nota!(
                "[insieme] rete spenta{}",
                if tutti {
                    ""
                } else {
                    ", qualche filo ancora in uscita"
                }
            );
        }
        // Gli eventi ancora in volo della rete vecchia si buttano.
        self.generazione = self.generazione.wrapping_add(1);
        self.vicinato.svuota();
        self.capi.clear();
    }

    fn manda(&self, a: IdConnessione, messaggio: &Messaggio) {
        let Some(accesa) = &self.rete else {
            return;
        };
        // Quel che parte deve passare i tetti di chi lo riceve: un messaggio
        // che non li passa chiude la connessione dall'altra parte. Qui non
        // deve succedere mai; se succede, meglio non mandarlo e dirlo.
        if let Err(fuori) = messaggio.valida() {
            nota!(
                "[insieme] messaggio per la connessione #{} fuori dai tetti: {}",
                a.0,
                fuori.0
            );
            return;
        }
        if let Err(err) = accesa.rete.manda(a, messaggio) {
            // La chiusura arriverà come evento, e la sessione la tratterà lì.
            nota!(
                "[insieme] messaggio non partito per la connessione #{} codice={}",
                a.0,
                err.code().kind().code()
            );
        }
    }

    fn mia(&self) -> Option<ChiavePubblica> {
        self.rete.as_ref().map(|a| a.mia)
    }

    fn dalla_rete(&mut self, evento: rymembra_insieme::Evento) {
        use rymembra_insieme::Evento as DallaRete;
        if let Some(riga) = riga_della_rete(&evento, &self.fidati) {
            nota!("{riga}");
        }
        match evento {
            DallaRete::Trovato {
                indirizzo,
                impronta,
            } => self.vicinato.trovato(indirizzo, impronta, ora()),
            DallaRete::Aperta {
                id,
                pari,
                nome,
                ospita,
                in_entrata,
                indirizzo,
            } => {
                // Due istanze con la stessa identità non si parlano: sarebbe
                // un PC che si associa con sé stesso.
                if Some(pari) == self.mia() {
                    if let Some(accesa) = &self.rete {
                        accesa.rete.chiudi(id, None);
                    }
                    return;
                }
                let dichiarato = voci::testo(&nome, MASSIMO_NOME);
                self.capi.insert(id, (indirizzo, pari));
                if !in_entrata {
                    self.vicinato.aperta(id, &dichiarato);
                }
                let fidato = self
                    .fidati
                    .per_chiave(&pari)
                    .filter(|p| !p.bloccato)
                    .cloned();
                // Un dettaglio per il pannello («visto alle…»): se la libreria
                // è occupata si salta, e lo scriverà la prossima connessione.
                if let Some(p) = &fidato {
                    let _ = self.in_libreria(ATTESA_LIBRERIA_BREVE, |c| {
                        rymembra_app::insieme::visto(c, p.id, adesso_ms())
                    });
                }
                self.nutri(Evento::Aperta {
                    id,
                    connessione: Connessione {
                        chiave: pari,
                        nome: fidato
                            .as_ref()
                            .map_or_else(|| dichiarato.clone(), |p| p.nome.clone()),
                        dichiarato,
                        ospita,
                        in_entrata,
                        pari: fidato.map(|p| p.id),
                    },
                });
            }
            DallaRete::Ricevuto { id, messaggio } => {
                if let Messaggio::Esito { id: numero, codice } = &messaggio {
                    self.esito_arrivato(id, *numero, codice.as_deref());
                }
                self.nutri(Evento::Ricevuto { id, messaggio });
            }
            DallaRete::Codice { id, codice } => self.nutri(Evento::Codice { id, codice }),
            // L'host sta associando un altro PC: niente codice per ora, e la
            // connessione resta. Si dice perché, e «Associa» torna premibile.
            DallaRete::AssociazioneRimandata { id, codice } => {
                let nome = Some(self.nome_della_connessione(id));
                self.nutri(Evento::AssociazioneFinita { id });
                self.avvisa(codice.i18n_key(), nome);
            }
            DallaRete::Associato { id, pari } => self.associato(id, pari),
            DallaRete::Orologio { id, campione } => {
                if let Some(seguendo) = &self.seguendo
                    && seguendo.host == id
                {
                    seguendo.condivisa.scambio(Scambio {
                        t1: campione.t1,
                        t2: campione.t2,
                        t3: campione.t3,
                        t4: campione.t4,
                    });
                }
            }
            DallaRete::Chiusa { id, codice } => {
                self.capi.remove(&id);
                self.vicinato.chiusa(id, ora());
                self.nutri(Evento::Chiusa { id, codice });
            }
        }
    }

    /// Chi segue: la risposta a un suo gesto. Nel diario se è un no, o se è
    /// arrivata tardi: è il tempo che la persona ha aspettato.
    fn esito_arrivato(&mut self, da: IdConnessione, numero: u32, codice: Option<&str>) {
        let Some(seguendo) = self.seguendo.as_mut().filter(|s| s.host == da) else {
            return;
        };
        let Some(posto) = seguendo.in_volo.iter().position(|(n, ..)| *n == numero) else {
            return;
        };
        let (_, partito, nome) = seguendo.in_volo.remove(posto);
        let attesa = partito.elapsed();
        if codice.is_some() || attesa >= LENTO {
            nota!(
                "[insieme] gesto «{nome}» risposto dall'host in {} ms codice={}",
                attesa.as_millis(),
                codice.unwrap_or("—")
            );
        }
    }

    fn associato(&mut self, id: IdConnessione, chiave: ChiavePubblica) {
        if Some(chiave) == self.mia() {
            return;
        }
        let nome = self
            .s
            .connessioni
            .get(&id)
            .map(|c| c.dichiarato.clone())
            .unwrap_or_default();
        let esito = self.in_libreria(ATTESA_LIBRERIA, |c| {
            rymembra_app::insieme::associa(c, &chiave.in_testo(), &nome, adesso_ms())
        });
        match esito {
            Ok(pari) => {
                nota!("[insieme] associato: pari #{pari}");
                self.ricarica_fidati();
                self.nutri(Evento::Associato { id, pari });
            }
            Err(err) => {
                nota!(
                    "[insieme] associazione non salvata codice={}",
                    err.code().kind().code()
                );
                // «Non si è salvato niente»: è la frase di `codiceRifiutato`, ed
                // è esattamente quel che è successo.
                if let Some(chiave) =
                    chiave_per_la_finestra(&err, Some(ErrorCodeKind::InsiemeCodiceRifiutato))
                {
                    self.avvisa(chiave, None);
                }
                self.nutri(Evento::AssociazioneFinita { id });
            }
        }
    }

    fn cerca(&mut self, adesso: u64) {
        self.prossima_scoperta = adesso.saturating_add(OGNI_SCOPERTA_MS);
        self.vicinato.pulisci(adesso, &self.fidati);
        let a_mano: Vec<SocketAddr> = self
            .impostazioni
            .indirizzi
            .iter()
            .filter_map(|i| i.parse().ok())
            .collect();
        if let Some(accesa) = &self.rete {
            accesa.rete.cerca(&a_mano);
        }
    }

    fn collega_vicini(&mut self, adesso: u64) {
        self.prossimo_collegamento = adesso.saturating_add(OGNI_COLLEGAMENTO_MS);
        let Some(accesa) = &self.rete else {
            return;
        };
        for (impronta, indirizzo) in
            self.vicinato
                .da_collegare(adesso, accesa.mia.impronta(), &self.fidati)
        {
            // Con l'impronta che la scoperta ha dato: la risposta viaggia in
            // chiaro e la può scrivere chiunque, e la stretta deve dimostrare
            // che dall'altra parte c'è proprio quella chiave.
            let id = accesa.rete.collega_atteso(indirizzo, impronta);
            self.vicinato.collegando(impronta, id);
        }
    }

    // ── chi ospita ──────────────────────────────────────────────────────────

    fn ospita(&mut self) {
        let Some(accesa) = &self.rete else {
            self.errore
                .get_or_insert(AppError::new(ErrorCode::InsiemeSpenta));
            return;
        };
        let porta = match accesa.rete.ospita() {
            Ok(porta) => porta,
            Err(err) => {
                self.errore.get_or_insert(senza_causa(&err));
                return;
            }
        };
        // Gli indirizzi da dettare al collega, una volta per sessione:
        // enumerare le schede a ogni pubblicazione costerebbe una chiamata di
        // sistema per niente.
        let qui = rymembra_insieme::indirizzi_di_qui(lega());
        self.indirizzi_di_qui = qui
            .principale
            .into_iter()
            .chain(qui.altri)
            .map(|ip| format!("{ip}:{porta}"))
            .collect();
        // Il ruolo prima: vedi il `//!`.
        self.ruolo(Ruolo::Host);
        if let Err(err) = ponte::entra_come_host(&self.app) {
            self.ruolo(Ruolo::Fuori);
            accesa.rete.smetti_di_ospitare();
            self.errore.get_or_insert(err);
            return;
        }
        // Le voci dei membri (fase 2) si leggono senza rete: i loro gettoni
        // sono inoltrati, e `lettore_locale` li chiede alla connessione di
        // servizio del membro che li possiede.
        let lettore = accesa.rete.lettore_locale();
        self.dal_filo = Some(DalFilo::nuovo(lettore, self.posta.clone()));
        nota!("[insieme] si ospita");
        self.sessione = self.sessione.wrapping_add(1);
        self.sessione_viva.store(self.sessione, Ordering::Release);
        self.preparazione.svuota();
        self.servite.svuota();
        self.versione = 0;
        self.uid_dei_membri = 0;
        self.impronta_stato = None;
        self.nutri(Evento::Ospitato { porta });
        self.campanello();
    }

    /// Il lettore di chi ospita è cambiato: voci nuove da descrivere, gettoni
    /// da revocare, uno stato e un'ancora da mandare.
    fn campanello(&mut self) {
        if !matches!(self.s.fase, Fase::Ospita(_)) {
            return;
        }
        let Some(istantanea) = ponte::istantanea(&self.app) else {
            return;
        };
        let mut tenere: HashSet<u64> = voci::finestra(&istantanea.coda, istantanea.corrente)
            .iter()
            .copied()
            .collect();
        if let Some(corrente) = istantanea.corrente {
            tenere.insert(corrente);
        }
        // Le voci dei membri restano finché sono in coda, anche fuori dalla
        // finestra: una volta dimenticate non le si saprebbe più descrivere,
        // perché i loro byte non sono di questo PC.
        let altrui: HashSet<u64> = self.servite.altrui().map(|(uid, _)| uid).collect();
        tenere.extend(istantanea.coda.iter().filter(|uid| altrui.contains(uid)));
        // Le voci di questo PC ancora da preparare si chiedono al loro filo,
        // in blocco e nell'ordine in cui serviranno: qui non si aspetta mai il
        // disco. Vedi `preparazione`.
        let candidate: Vec<u64> = preparazione::in_ordine(
            voci::finestra(&istantanea.coda, istantanea.corrente),
            istantanea.corrente,
        )
        .into_iter()
        .filter(|uid| !self.servite.ha(*uid) && *uid != ponte::UID_CALIBRAZIONE)
        .collect();
        let chiedere = self.preparazione.da_chiedere(&candidate, ora());
        if let Some(preparatore) = &self.preparatore {
            // La voce che suona, se non è ancora descritta, passa davanti ai
            // blocchi già in fila: chi segue la aspetta per suonare, e senza
            // di lei le ancore non partono. I blocchi possono essere quelli di
            // una coda appena sostituita, mille voci da un NAS.
            if let Some(corrente) = istantanea.corrente
                && corrente != ponte::UID_CALIBRAZIONE
                && !self.servite.ha(corrente)
                && self.chiesta_subito != Some((self.sessione, corrente))
            {
                self.chiesta_subito = Some((self.sessione, corrente));
                let _ = preparatore.send(DaPreparare::Subito {
                    sessione: self.sessione,
                    uid: corrente,
                });
            }
            for blocco in chiedere.chunks(preparazione::BLOCCO) {
                let _ = preparatore.send(DaPreparare::Blocco {
                    sessione: self.sessione,
                    uids: blocco.to_vec(),
                });
            }
        }
        self.preparazione.tieni(&tenere);
        let conto = self.preparazione.non_mandabili(&tenere);
        if let Some(riga) = self.preparazione.riga_se_cambia(conto) {
            nota!("{riga}");
        }
        let servizi = self.servite.tieni(&tenere);
        self.esegui_servizi(servizi, false);
        if let Some(dal) = &mut self.dal_filo {
            dal.registrate.retain(|uid, _| tenere.contains(uid));
        }
        self.proprietari_di_chi_ospita(&istantanea);
        self.corrente = istantanea.corrente;
        // La calibrazione si legge dal lettore: la chiude anche un «prossimo»
        // o una voce scelta, e una copia tenuta qui resterebbe accesa sui clic
        // che non suonano più. Chiusa così, lo si dice anche a chi ospita.
        let calibrando = ponte::calibrando(&self.app);
        if self.calibrazione && !calibrando {
            self.avvisa(fasi::AVVISO_CALIBRAZIONE_SPENTA, None);
        }
        self.calibrazione = calibrando;
        self.locale = ponte::voce_corrente_locale(&self.app);
        self.fotografa_host();
        self.manda_stato(&istantanea, false);
        self.manda_ancora();
    }

    /// Esegue i servizi di una `Servite`: quella di chi ospita, o quella delle
    /// voci proprie di chi segue (`proprie`), che serve le stesse cose.
    fn esegui_servizi(&mut self, servizi: Vec<Servizio>, proprie: bool) {
        let mut senza = Vec::new();
        {
            let Some(accesa) = &self.rete else {
                return;
            };
            let store = self.libreria().and_then(|s| s.copertine()).ok();
            for servizio in servizi {
                match servizio {
                    Servizio::File(gettone, file) => accesa.rete.servi(gettone, file),
                    Servizio::Copertina(gettone, impronta) => {
                        let byte = store
                            .as_ref()
                            .and_then(|s| std::fs::read(s.path_for(&impronta)).ok())
                            .filter(|b| !b.is_empty() && b.len() <= voci::BYTE_MASSIMI_COPERTINA);
                        match byte {
                            Some(byte) => accesa.rete.servi_copertina(gettone, Arc::from(byte)),
                            None => senza.push(impronta),
                        }
                    }
                    Servizio::Revoca(gettone) => accesa.rete.revoca(&gettone),
                    // Gli inoltri li fa `aggiungi_del_membro`, che sa rifiutare
                    // l'aggiunta quando il trasporto dice di no; qui arrivano
                    // solo per completezza.
                    Servizio::Inoltra(gettone, membro) => {
                        let fatto = self
                            .chiave_del_membro(membro)
                            .is_some_and(|chiave| accesa.rete.inoltra(gettone, chiave));
                        if !fatto {
                            nota!("[insieme] gettone del membro {membro} non inoltrato");
                        }
                    }
                    Servizio::Descrivi { file, track_id } => {
                        if let Some(coda) = &self.descrittori {
                            let _ = coda.send((file, track_id));
                        }
                    }
                }
            }
        }
        for impronta in senza {
            let revoche = if proprie {
                self.seguendo
                    .as_mut()
                    .map(|s| s.proprie.senza_copertina(&impronta))
                    .unwrap_or_default()
            } else {
                self.servite.senza_copertina(&impronta)
            };
            for servizio in revoche {
                if let (Servizio::Revoca(gettone), Some(accesa)) = (servizio, &self.rete) {
                    accesa.rete.revoca(&gettone);
                }
            }
        }
    }

    /// La chiave del membro con quel numero, dalla sua connessione.
    fn chiave_del_membro(&self, membro: u32) -> Option<ChiavePubblica> {
        let Fase::Ospita(o) = &self.s.fase else {
            return None;
        };
        let conn = o.membri.iter().find(|m| m.membro == membro)?.conn;
        self.capi.get(&conn).map(|(_, chiave)| *chiave)
    }

    /// Lo stato ai membri, se è cambiato (o `forzato`), al più ogni
    /// [`OGNI_STATO_MS`] (salvo `forzato`).
    fn manda_stato(&mut self, istantanea: &ponte::Istantanea, forzato: bool) {
        let membri = fasi::connessioni_dei_membri(&self.s);
        if membri.is_empty() {
            self.stato_in_sospeso = false;
            return;
        }
        let persone = fasi::persone_di_qui(&self.s, &self.nome);
        let impronta = ImprontaStato {
            coda: istantanea.coda.clone(),
            descritte: istantanea
                .coda
                .iter()
                .filter(|uid| self.servite.ha(**uid))
                .count(),
            copie: self.copie,
            corrente: istantanea.corrente,
            corrente_descritta: istantanea.corrente.is_some_and(|uid| self.servite.ha(uid)),
            in_pausa: istantanea.in_pausa,
            ripeti: istantanea.ripeti,
            persone: persone.clone(),
        };
        if !forzato && self.impronta_stato.as_ref() == Some(&impronta) {
            self.stato_in_sospeso = false;
            return;
        }
        let adesso = ora();
        let ogni = if self
            .impronta_stato
            .as_ref()
            .is_some_and(|prima| impronta.solo_descrizioni(prima))
        {
            OGNI_DESCRIZIONI_MS
        } else {
            OGNI_STATO_MS
        };
        let dovuto = self.stato_mandato.saturating_add(ogni);
        if !forzato && adesso < dovuto {
            self.stato_in_sospeso = true;
            self.stato_dovuto = dovuto;
            return;
        }
        self.versione = self.versione.saturating_add(1);
        let stato = voci::stato_sessione(self.versione, persone, istantanea, &self.servite);
        let messaggio = Messaggio::Stato(stato);
        for membro in membri {
            self.manda(membro, &messaggio);
        }
        self.impronta_stato = Some(impronta);
        self.stato_mandato = adesso;
        self.stato_in_sospeso = false;
    }

    /// Chi ospita: le descrizioni sono cambiate senza che il lettore lo
    /// sappia (un descrittore di copia): lo stato partirà col giro lento,
    /// salvo che uno più urgente sia già in sospeso.
    fn descrizioni_da_mandare(&mut self) {
        if !matches!(self.s.fase, Fase::Ospita(_)) {
            return;
        }
        let dovuto = self.stato_mandato.saturating_add(OGNI_DESCRIZIONI_MS);
        if !self.stato_in_sospeso || self.stato_dovuto > dovuto {
            self.stato_dovuto = dovuto;
        }
        self.stato_in_sospeso = true;
    }

    /// Uno stato in sospeso che non è solo di descrizioni: chi segue potrebbe
    /// non conoscere ancora la voce che l'ancora nominerebbe.
    fn stato_strutturale_in_sospeso(&self) -> bool {
        self.stato_in_sospeso
            && self.stato_dovuto < self.stato_mandato.saturating_add(OGNI_DESCRIZIONI_MS)
    }

    fn manda_ancora(&mut self) {
        self.prossima_ancora = ora().saturating_add(OGNI_ANCORA_MS);
        let membri = fasi::connessioni_dei_membri(&self.s);
        if membri.is_empty() {
            return;
        }
        let Some(a) = ponte::ancora_di_questo(&self.app) else {
            // Il motore di chi ospita non ha niente in mano — un'apertura da
            // un disco di rete andata male, la coda finita — ma la voce
            // corrente c'è: «ferma» detto esplicitamente. Senza ancora, chi
            // segue continuava a far avanzare l'ultima che aveva e suonava da
            // solo, mentre l'host taceva.
            if let Some(corrente) = self.corrente.filter(|uid| self.servite.ha(*uid))
                && ponte::motore_senza_brano(&self.app)
            {
                let ferma = Messaggio::Ancora(AncoraSulFilo {
                    etichetta: corrente,
                    posizione_us: 0,
                    udito_ns: rymembra_play::tempo::adesso_ns(),
                    in_corso: false,
                    programmata: false,
                });
                for membro in membri {
                    self.manda(membro, &ferma);
                }
            }
            return;
        };
        // Un'ancora per una voce che nessuno stato ha ancora descritto non
        // serve a chi segue: non saprebbe aprirla, e il suo regolatore
        // insisterebbe a riaprire una voce che non ha
        // (`playback.sourceUnavailable`, visto nella prima prova fra due PC).
        // Arriva appena la voce è pronta, ed è la prima che si prepara.
        if a.etichetta != ponte::UID_CALIBRAZIONE && !self.servite.ha(a.etichetta) {
            return;
        }
        // Con uno stato in sospeso, un'ancora che nomina una voce che l'ultimo
        // stato mandato non dava come corrente aspetta quello stato: chi segue
        // non la conoscerebbe ancora. Le altre partono subito — una pausa, una
        // ripresa, un salto nella stessa voce — perché arrivavano dietro allo
        // stato, che chi segue impiega un po' a leggere: una pausa si sentiva
        // mezzo secondo dopo.
        let gia_detta = self
            .impronta_stato
            .as_ref()
            .is_some_and(|i| i.corrente == Some(a.etichetta) && i.corrente_descritta);
        if self.stato_strutturale_in_sospeso()
            && !gia_detta
            && a.etichetta != ponte::UID_CALIBRAZIONE
        {
            return;
        }
        let messaggio = Messaggio::Ancora(AncoraSulFilo {
            etichetta: a.etichetta,
            posizione_us: a.posizione_us,
            udito_ns: a.udito_ns,
            in_corso: a.in_corso,
            programmata: a.programmata,
        });
        for membro in membri {
            self.manda(membro, &messaggio);
        }
    }

    /// Chi ospita: «di Marco» sulle voci dei membri che sono in coda.
    fn proprietari_di_chi_ospita(&mut self, istantanea: &ponte::Istantanea) {
        let persone = fasi::persone_di_qui(&self.s, &self.nome);
        let di: HashMap<u64, u32> = self.servite.altrui().collect();
        let Some(dal) = &self.dal_filo else {
            self.proprietari.clear();
            return;
        };
        self.proprietari = proprietari(&istantanea.coda, &persone, |uid| {
            let membro = di.get(&uid)?;
            let maniglia = dal.registrate.get(&uid)?.1?;
            Some((maniglia, *membro))
        });
    }

    fn fotografa_host(&self) {
        let persone = fasi::persone_di_qui(&self.s, &self.nome);
        let foto = nel_lettore_di_chi_ospita(
            &self.nome,
            persone.len(),
            proprietario_di_chi_ospita(self.corrente, &self.servite, &persone),
            Dettagli {
                agganciato: true,
                locale: self.locale,
                calibrazione: self.calibrazione,
                revisione: self.revisione_voci,
            },
        );
        self.metti_nel_lettore(Some(foto));
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

    // ── chi segue ───────────────────────────────────────────────────────────

    fn entra_come_membro(&mut self, host: IdConnessione) {
        let Some((indirizzo, chiave)) = self.capi.get(&host).copied() else {
            self.nutri(Evento::LettoreRifiuta);
            return;
        };
        // Il ruolo prima: vedi il `//!`. I diritti sono quelli dell'`Ammesso`;
        // senza una sessione da seguire, il più stretto.
        let diritti = match &self.s.fase {
            Fase::Membro(m) => m.diritti,
            _ => Diritti::Ascolta,
        };
        self.ruolo(Ruolo::Membro(diritti));
        if let Err(err) = ponte::entra_come_membro(&self.app) {
            nota!(
                "[insieme] il lettore non entra come membro codice={}",
                err.code().kind().code()
            );
            // Per chi aspettava di entrare il lettore che non entra è un
            // ingresso andato male: `rifiutato` («nessuna risposta, o la
            // risposta è stata no») è la frase più vicina che la finestra ha.
            if let Some(chiave) =
                chiave_per_la_finestra(&err, Some(ErrorCodeKind::InsiemeRifiutato))
            {
                self.avvisa(chiave, None);
            }
            self.nutri(Evento::LettoreRifiuta);
            return;
        }
        let Some(lettore) = self
            .rete
            .as_ref()
            .map(|a| a.rete.lettore(indirizzo, chiave))
        else {
            self.nutri(Evento::LettoreRifiuta);
            return;
        };
        let condivisa = Arc::new(Condivisa::nuova());
        let (sveglia, orecchio) = channel();
        let filo = match segui::avvia(self.app.clone(), Arc::clone(&condivisa), orecchio) {
            Ok(filo) => filo,
            Err(err) => {
                nota!("[insieme] il filo della sincronia non parte: {err}");
                self.nutri(Evento::LettoreRifiuta);
                return;
            }
        };
        self.dal_filo = Some(DalFilo::nuovo(lettore, self.posta.clone()));
        self.seguendo = Some(Seguendo {
            host,
            indirizzo,
            chiave,
            condivisa,
            sveglia,
            filo: Some(filo),
            corrente: None,
            proprie: Servite::default(),
            uid_proprio: 0,
            offerta: false,
            prossimo_comando: 0,
            in_volo: Vec::new(),
            in_attesa: Vec::new(),
            mandate: HashMap::new(),
        });
        nota!("[insieme] si segue la connessione #{}", host.0);
        self.fotografa_membro();
    }

    fn specchia(&mut self, stato: StatoSessione) {
        let Fase::Membro(m) = &self.s.fase else {
            return;
        };
        let io = m.io;
        let (Some(_), Some(dal)) = (&self.seguendo, &self.dal_filo) else {
            return;
        };
        let uids: Vec<u64> = stato.coda.iter().map(|v| v.uid).collect();
        let tenere: HashSet<u64> = uids.iter().copied().collect();
        let nuove: Vec<VoceSulFilo> = stato
            .coda
            .iter()
            .filter(|v| dal.registrate.get(&v.uid).map(|(r, _)| r) != Some(*v))
            .cloned()
            .collect();
        self.registra_dal_filo(nuove);
        if let Some(dal) = &mut self.dal_filo {
            dal.registrate.retain(|uid, _| tenere.contains(uid));
        }
        self.comune
            .copertine
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .tieni(&tenere);
        self.tieni_le_proprie(&stato);
        if let Some(seguendo) = &mut self.seguendo {
            seguendo.corrente = stato.corrente;
        }
        ponte::specchia(
            &self.app,
            &uids,
            stato.corrente,
            stato.in_pausa,
            voci::ripeti_da_filo(stato.ripeti),
        );
        // «di Marco»: le voci che un altro **membro** ha aggiunto. Non quelle
        // di chi ospita, che sono la coda stessa: mille righe tutte «di A»
        // non direbbero niente che la fascia della sessione non dica già, e
        // viaggerebbero verso la finestra a ogni stato. Non le proprie, che
        // sono di chi guarda.
        self.proprietari = match &self.dal_filo {
            Some(dal) => proprietari(&uids, &stato.persone, |uid| {
                let (v, maniglia) = dal.registrate.get(&uid)?;
                if v.di == io || v.di == 0 {
                    return None;
                }
                Some(((*maniglia)?, v.di))
            }),
            None => Vec::new(),
        };
        self.locale = ponte::voce_corrente_locale(&self.app);
        self.fotografa_membro();
    }

    fn segui(&self, ancora: AncoraSulFilo) {
        let Some(seguendo) = &self.seguendo else {
            return;
        };
        let cambia = seguendo.condivisa.ancora(DallOspite {
            etichetta: ancora.etichetta,
            posizione_us: ancora.posizione_us,
            udito_ospite_ns: ancora.udito_ns,
            in_corso: ancora.in_corso,
            programmata: ancora.programmata,
        });
        // Una pausa o un brano nuovo non aspettano il colpo dopo.
        if cambia {
            let _ = seguendo.sveglia.send(());
        }
    }

    fn fotografa_membro(&self) {
        let (Fase::Membro(m), Some(seguendo), Some(dal)) =
            (&self.s.fase, &self.seguendo, &self.dal_filo)
        else {
            return;
        };
        let foto = nel_lettore_di_chi_segue(
            &self.nome_della_connessione(m.host),
            &m.persone,
            m.io,
            m.diritti,
            seguendo
                .corrente
                .and_then(|c| dal.registrate.get(&c))
                .map(|(v, _)| v),
            Dettagli {
                agganciato: seguendo.condivisa.aggancio().agganciato,
                locale: self.locale,
                calibrazione: m.calibrazione,
                revisione: self.revisione_voci,
            },
        );
        self.metti_nel_lettore(Some(foto));
    }

    /// L'aggancio alla finestra: subito quando cambia «agganciato», al più
    /// una volta al secondo per il resto. Allo stesso passo si riguarda se la
    /// voce che suona si legge da un file di qui (fase 3): la copia la trova
    /// il lettore mentre apre, e nessuno lo dice alla sessione.
    fn guarda_l_aggancio(&mut self, adesso: u64) {
        let Some(seguendo) = &self.seguendo else {
            return;
        };
        let aggancio = seguendo.condivisa.aggancio();
        let cambiato = self
            .aggancio_mostrato
            .is_none_or(|prima| prima.agganciato != aggancio.agganciato);
        if cambiato || adesso >= self.aggancio_dal.saturating_add(OGNI_AGGANCIO_MS) {
            self.aggancio_mostrato = Some(aggancio);
            self.aggancio_dal = adesso;
            let locale = ponte::voce_corrente_locale(&self.app);
            if cambiato || locale != self.locale {
                self.locale = locale;
                self.fotografa_membro();
            }
        }
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
        ponte::esci(&self.app);
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

    // ── fase 2: chi ospita esegue i gesti dei membri ────────────────────────

    /// Un gesto di un membro: all'esecutore, che lo fa negli stessi percorsi
    /// dei gesti di chi ospita e poi fa partire la risposta. Qui si prepara
    /// solo quel che è della sessione (le voci di un `Aggiungi`), che non
    /// aspetta né il disco né il lettore.
    fn esegui_comando(&mut self, da: IdConnessione, membro: u32, comando: u32, azione: Azione) {
        let lavoro = match azione {
            Azione::Aggiungi { voci, dove, suona } => {
                match self.aggiungi_del_membro(membro, voci, dove, suona) {
                    Ok(lavoro) => lavoro,
                    Err(codice) => {
                        self.ricevi(Posta::Eseguito {
                            risposta: Some((da, comando)),
                            membro,
                            codice: Some(codice),
                            nome: "aggiungi",
                            attesa: Duration::ZERO,
                            durata: Duration::ZERO,
                        });
                        return;
                    }
                }
            }
            altra => Lavoro::Azione(altra),
        };
        self.affida(Incarico {
            lavoro,
            risposta: Some((da, comando)),
            membro,
            arrivato: Instant::now(),
            sessione: self.sessione,
        });
    }

    /// Un incarico all'esecutore. Se l'esecutore non c'è, il gesto si rifiuta
    /// subito: meglio un no che un membro ad aspettare.
    fn affida(&mut self, incarico: Incarico) {
        let rimandato = match &self.esecutore {
            Some(esecutore) => esecutore.send(incarico).err().map(|err| err.0),
            None => Some(incarico),
        };
        if let Some(incarico) = rimandato {
            self.ricevi(Posta::Eseguito {
                risposta: incarico.risposta,
                membro: incarico.membro,
                codice: Some(ErrorCodeKind::InsiemeRifiutato.code().to_owned()),
                nome: incarico.lavoro.nome(),
                attesa: Duration::ZERO,
                durata: Duration::ZERO,
            });
        }
    }

    /// `Aggiungi` di un membro: le voci si rinumerano e si registrano coi loro
    /// gettoni inoltrati al membro. Metterle in coda, e farne partire la prima,
    /// tocca all'esecutore: è il lavoro che apre.
    fn aggiungi_del_membro(
        &mut self,
        membro: u32,
        voci: Vec<VoceSulFilo>,
        dove: Dove,
        suona: bool,
    ) -> Result<Lavoro, String> {
        let rifiutato = || ErrorCodeKind::InsiemeRifiutato.code().to_owned();
        let (Some(chiave), Some(_)) = (self.chiave_del_membro(membro), &self.dal_filo) else {
            return Err(rifiutato());
        };
        let mut nuove = Vec::with_capacity(voci.len());
        let mut servizi = Vec::new();
        let mut altrui = false;
        for mut v in voci.into_iter().take(MASSIMO_AGGIUNTE) {
            // Gli `uid` li sceglie l'host: quelli del membro valgono solo per
            // lui, e due membri sceglierebbero gli stessi. `di` lo mette
            // l'host: un membro non può dire che una voce è di un altro.
            self.uid_dei_membri = self.uid_dei_membri.saturating_add(1);
            v.uid = UID_DEI_MEMBRI.saturating_add(self.uid_dei_membri);
            v.di = membro;
            match self.servite.ospita_voce(v.clone()) {
                Ok(s) => {
                    servizi.extend(s);
                    nuove.push(v);
                }
                Err(voci::GettoneAltrui) => {
                    altrui = true;
                    break;
                }
            }
        }
        let uids: Vec<u64> = nuove.iter().map(|v| v.uid).collect();
        // Gli inoltri, uno per uno: il trasporto rifiuta un gettone già in
        // uso, e allora l'aggiunta intera si rifiuta (un membro che sceglie il
        // gettone di un brano altrui non deve farsene dare i byte).
        let mut respinti = HashSet::new();
        if !altrui && let Some(accesa) = &self.rete {
            for servizio in servizi {
                if let Servizio::Inoltra(gettone, _) = servizio
                    && !accesa.rete.inoltra(gettone, chiave)
                {
                    respinti.insert(gettone);
                }
            }
        }
        if altrui || !respinti.is_empty() {
            nota!("[insieme] aggiunta del membro {membro} rifiutata: gettone già in uso");
            self.togli_l_aggiunta(&uids, &respinti);
            return Err(rifiutato());
        }
        self.registra_dal_filo(nuove);
        let maniglie: Vec<i64> = self
            .dal_filo
            .as_ref()
            .map(|dal| {
                uids.iter()
                    .filter_map(|uid| dal.registrate.get(uid).and_then(|(_, m)| *m))
                    .collect()
            })
            .unwrap_or_default();
        if maniglie.is_empty() {
            self.togli_l_aggiunta(&uids, &HashSet::new());
            return Err(rifiutato());
        }
        nota!("[insieme] il membro {membro} aggiunge {} voci", uids.len());
        Ok(Lavoro::Accoda {
            maniglie,
            dopo: dove == Dove::Dopo,
            suona: uids.first().copied().filter(|_| suona),
        })
    }

    /// Un'aggiunta rifiutata a metà: le voci già contate si dimenticano, e i
    /// loro inoltri si revocano — tranne quelli che il trasporto ha respinto,
    /// che sono di qualcun altro e restano suoi.
    fn togli_l_aggiunta(&mut self, uids: &[u64], respinti: &HashSet<Gettone>) {
        let revoche: Vec<Servizio> = self
            .servite
            .dimentica(uids)
            .into_iter()
            .filter(|s| !matches!(s, Servizio::Revoca(g) if respinti.contains(g)))
            .collect();
        self.esegui_servizi(revoche, false);
        if let Some(dal) = &mut self.dal_filo {
            for uid in uids {
                dal.registrate.remove(uid);
            }
        }
    }

    /// Chi ospita: le chiavi che possono leggere i gettoni sono quelle dei
    /// membri della sessione, e solo loro. Si rifà a ogni ingresso e uscita
    /// (con `PersoneCambiate`), e diventa vuota quando non si ospita: di
    /// partenza il trasporto non lascia leggere a nessuno.
    fn aggiorna_ammessi(&self) {
        let Some(accesa) = &self.rete else {
            return;
        };
        let chiavi: Vec<ChiavePubblica> = fasi::connessioni_dei_membri(&self.s)
            .iter()
            .filter_map(|conn| self.capi.get(conn).map(|(_, chiave)| *chiave))
            .collect();
        accesa.rete.ammessi(chiavi);
    }

    /// Un membro non c'è più: le sue voci escono dalla coda, e i suoi gettoni
    /// inoltrati si dimenticano.
    fn membro_uscito(&mut self, membro: u32) {
        let uids = self.servite.di_membro(membro);
        if uids.is_empty() {
            return;
        }
        // Togliere la voce che suona vuol dire aprire la dopo: all'esecutore.
        self.affida(Incarico {
            lavoro: Lavoro::Togli(uids.clone()),
            risposta: None,
            membro,
            arrivato: Instant::now(),
            sessione: self.sessione,
        });
        let servizi = self.servite.dimentica(&uids);
        self.esegui_servizi(servizi, false);
        if let Some(dal) = &mut self.dal_filo {
            for uid in &uids {
                dal.registrate.remove(uid);
            }
        }
        nota!(
            "[insieme] il membro {membro} è uscito: {} voci tolte",
            uids.len()
        );
    }

    // ── fase 2: chi segue manda i suoi gesti ────────────────────────────────

    /// Azioni già tradotte, all'host, ognuna col suo numero. Non si aspetta
    /// l'`Esito`: arriva da sé, e se dice di no diventa un avviso.
    fn manda_gesti(&mut self, azioni: Vec<Azione>) {
        let Fase::Membro(m) = &self.s.fase else {
            return;
        };
        let host = m.host;
        for azione in azioni {
            let Some(seguendo) = &mut self.seguendo else {
                return;
            };
            seguendo.prossimo_comando = seguendo.prossimo_comando.wrapping_add(1);
            let id = seguendo.prossimo_comando;
            if seguendo.in_volo.len() >= GESTI_IN_VOLO {
                seguendo.in_volo.remove(0);
            }
            seguendo
                .in_volo
                .push((id, Instant::now(), gesti::nome_dell_azione(&azione)));
            self.manda(host, &Messaggio::Comando { id, azione });
        }
    }

    /// Brani della libreria di questo PC da aggiungere alla sessione: gettoni
    /// e copertine si servono subito, il descrittore (fase 3) si aspetta un
    /// poco, poi `Aggiungi` parte. Vedi [`ATTESA_DESCRITTORI`].
    fn aggiungi_proprie(&mut self, brani: Vec<ponte::BranoDiQuesto>, dove: Dove, suona: bool) {
        let Fase::Membro(_) = &self.s.fase else {
            return;
        };
        let Some(seguendo) = &mut self.seguendo else {
            return;
        };
        let mut uids = Vec::new();
        let mut servizi = Vec::new();
        let mut nuovo_gettone = Gettone::nuovo;
        for brano in &brani {
            seguendo.uid_proprio = seguendo.uid_proprio.saturating_add(1);
            let uid = seguendo.uid_proprio;
            match seguendo.proprie.aggiungi(uid, brano, &mut nuovo_gettone) {
                Ok(mut nuovi) => {
                    servizi.append(&mut nuovi);
                    if seguendo.proprie.ha(uid) {
                        uids.push(uid);
                    }
                }
                Err(err) => {
                    nota!(
                        "[insieme] gettone non creato codice={}",
                        err.code().kind().code()
                    );
                }
            }
        }
        if uids.is_empty() {
            return;
        }
        let attesa = if suona {
            ATTESA_DESCRITTORI_SUONA
        } else {
            ATTESA_DESCRITTORI
        };
        seguendo.in_attesa.push(Aggiunta {
            uids,
            dove,
            suona,
            scade: ora().saturating_add(attesa),
        });
        // La connessione di servizio, la prima volta: è da lì che l'host
        // chiederà i byte di questi gettoni per chi li ascolta.
        let offri = !seguendo.offerta;
        seguendo.offerta = true;
        let (indirizzo, chiave) = (seguendo.indirizzo, seguendo.chiave);
        self.esegui_servizi(servizi, true);
        if offri && let Some(accesa) = &self.rete {
            accesa.rete.offri(indirizzo, chiave);
        }
        self.manda_le_aggiunte();
    }

    /// Le aggiunte pronte partono, nell'ordine: pronta vuol dire che ogni sua
    /// voce ha il descrittore (o non lo avrà), o che l'attesa è finita.
    fn manda_le_aggiunte(&mut self) {
        let Fase::Membro(m) = &self.s.fase else {
            return;
        };
        let (host, io) = (m.host, m.io);
        let adesso = ora();
        let mut pronte = Vec::new();
        if let Some(seguendo) = &mut self.seguendo {
            while let Some(prima) = seguendo.in_attesa.first() {
                let descritte = prima.uids.iter().all(|u| seguendo.proprie.descritta(*u));
                if !descritte && adesso < prima.scade {
                    break;
                }
                let aggiunta = seguendo.in_attesa.remove(0);
                let voci: Vec<VoceSulFilo> = aggiunta
                    .uids
                    .iter()
                    .filter_map(|uid| seguendo.proprie.voce(*uid).cloned())
                    .map(|mut v| {
                        v.di = io;
                        v
                    })
                    .collect();
                for uid in &aggiunta.uids {
                    seguendo.mandate.insert(
                        *uid,
                        Mandata {
                            vista: false,
                            alle: adesso,
                        },
                    );
                }
                seguendo.prossimo_comando = seguendo.prossimo_comando.wrapping_add(1);
                pronte.push(Messaggio::Comando {
                    id: seguendo.prossimo_comando,
                    azione: Azione::Aggiungi {
                        voci,
                        dove: aggiunta.dove,
                        suona: aggiunta.suona,
                    },
                });
            }
        }
        for messaggio in pronte {
            self.manda(host, &messaggio);
        }
    }

    /// Chi segue: le voci proprie che l'host ha tolto dalla coda (o che non vi
    /// sono mai arrivate) smettono di essere servite.
    fn tieni_le_proprie(&mut self, stato: &StatoSessione) {
        let adesso = ora();
        let Fase::Membro(m) = &self.s.fase else {
            return;
        };
        let io = m.io;
        let Some(seguendo) = &mut self.seguendo else {
            return;
        };
        let mut via = Vec::new();
        for uid in seguendo.proprie.uids().collect::<Vec<_>>() {
            let Some(mia) = seguendo.proprie.voce(uid).cloned() else {
                continue;
            };
            let presente = stato
                .coda
                .iter()
                .any(|v| v.di == io && stessa_sorgente(&v.sorgente, &mia.sorgente));
            let Some(mandata) = seguendo.mandate.get_mut(&uid) else {
                // Non ancora partita: aspetta il suo descrittore.
                continue;
            };
            if presente {
                mandata.vista = true;
            } else if mandata.vista || adesso.saturating_sub(mandata.alle) > ATTESA_ECO_MS {
                via.push(uid);
            }
        }
        if via.is_empty() {
            return;
        }
        for uid in &via {
            seguendo.mandate.remove(uid);
        }
        let servizi = seguendo.proprie.dimentica(&via);
        self.esegui_servizi(servizi, true);
    }

    // ── chi ospita: le voci preparate dal loro filo ─────────────────────────

    /// Un blocco di voci preparate: le pronte si descrivono e si servono, le
    /// mancate aspettano il loro tentativo dopo. Poi lo stato si ripubblica,
    /// con le voci nuove dentro.
    fn preparate(
        &mut self,
        sessione: u64,
        esiti: Vec<(u64, Result<ponte::BranoDiQuesto, ponte::Mancanza>)>,
    ) {
        if sessione != self.sessione || !matches!(self.s.fase, Fase::Ospita(_)) {
            return;
        }
        let adesso = ora();
        let mut servizi = Vec::new();
        let mut nuovo_gettone = Gettone::nuovo;
        for (uid, brano) in esiti {
            let brano = match brano {
                Ok(brano) => brano,
                Err(mancanza) => {
                    // Solo un disco che non ha risposto può rispondere la
                    // volta dopo: il resto si lascia subito.
                    self.preparazione
                        .mancata(uid, adesso, preparazione::definitiva(mancanza));
                    continue;
                }
            };
            match self.servite.aggiungi(uid, &brano, &mut nuovo_gettone) {
                Ok(mut nuovi) => {
                    servizi.append(&mut nuovi);
                    if self.servite.ha(uid) {
                        self.preparazione.arrivata(uid);
                    } else {
                        // Un brano che non si può mandare (niente file e niente
                        // riferimento): non cambierà riprovando.
                        self.preparazione.mancata(uid, adesso, true);
                    }
                }
                Err(err) => {
                    nota!(
                        "[insieme] gettone non creato codice={}",
                        err.code().kind().code()
                    );
                    self.preparazione.mancata(uid, adesso, false);
                }
            }
        }
        self.esegui_servizi(servizi, false);
        self.campanello();
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

    fn ricarica_impostazioni(&mut self) {
        if let Ok(impostazioni) =
            self.in_libreria(ATTESA_LIBRERIA, rymembra_app::insieme::impostazioni)
        {
            self.impostazioni = impostazioni;
        }
        self.nome = nome_effettivo(&self.impostazioni);
    }

    fn ricarica_fidati(&mut self) {
        if let Ok(elenco) = self.in_libreria(ATTESA_LIBRERIA, rymembra_app::insieme::pari) {
            self.fidati = Fidati::da(elenco);
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
                self.ricarica_impostazioni();
                if let Some(accesa) = &self.rete {
                    accesa.rete.rinomina(&self.nome);
                }
                if matches!(self.s.fase, Fase::Ospita(_)) {
                    self.esegui(Effetto::PersoneCambiate);
                }
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
                self.ricarica_fidati();
                if let Some(p) = self.fidati.per_id(id) {
                    let nome = p.nome.clone();
                    self.nutri(Evento::Rinominato { pari: id, nome });
                }
                Ok(())
            }
            // La sessione prima, la rete dopo: il `Fine` (o l'`Esco`) che il
            // riduttore manda al PC tolto deve partire mentre la sua chiave è
            // ancora fidata. Dopo `Rete::pari` un PC bloccato è già chiuso, e
            // dall'altra parte si leggerebbe «sparito» invece del perché.
            Richiesta::Dimentica(id) => {
                self.scrivi(|c| rymembra_app::insieme::dimentica(c, id))?;
                nota!("[insieme] dimenticato pari #{id}");
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

/// Chi ospita: il filo che prepara le voci di questo PC, fuori dal filo della
/// sessione. Vedi `preparazione`.
///
/// Un blocco per chiamata a `ponte::voci_di_questo`: il registro guardato una
/// volta, la libreria con una presa sola, il disco su un filo solo con una
/// scadenza per tutto il blocco. Il costo è suo, non delle ancore: ogni blocco
/// torna appena è fatto, e lo stato si ripubblica a ogni blocco.
fn avvia_il_preparatore(app: AppHandle, posta: Sender<Posta>) -> Option<Sender<DaPreparare>> {
    let (coda, orecchio) = channel::<DaPreparare>();
    let avviato = std::thread::Builder::new()
        .name("rymembra-insieme-preparazione".to_owned())
        .spawn(move || {
            let mut in_fila: VecDeque<DaPreparare> = VecDeque::new();
            loop {
                if in_fila.is_empty() {
                    match orecchio.recv() {
                        Ok(richiesta) => in_fila.push_back(richiesta),
                        Err(_) => return,
                    }
                }
                in_fila.extend(orecchio.try_iter());
                if crate::spegnimento::in_uscita() {
                    return;
                }
                let Some((sessione, uids)) = prossima_da_preparare(&mut in_fila) else {
                    continue;
                };
                let esiti = ponte::voci_di_questo(&app, &uids);
                if posta.send(Posta::Preparate { sessione, esiti }).is_err() {
                    return;
                }
            }
        });
    match avviato {
        Ok(_) => Some(coda),
        Err(err) => {
            nota!("[insieme] il filo della preparazione non parte: {err}");
            None
        }
    }
}

/// Una richiesta al filo che prepara le voci di chi ospita.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DaPreparare {
    /// Un blocco di voci, nell'ordine in cui serviranno.
    Blocco { sessione: u64, uids: Vec<u64> },
    /// La voce che suona: davanti a tutto.
    Subito { sessione: u64, uid: u64 },
}

impl DaPreparare {
    const fn sessione(&self) -> u64 {
        match self {
            Self::Blocco { sessione, .. } | Self::Subito { sessione, .. } => *sessione,
        }
    }
}

/// Che cosa preparare adesso fra le richieste in fila, che toglie: le
/// richieste di una sessione vecchia si buttano (i suoi `uid` sono altre voci
/// in quella nuova), e una voce `Subito` passa davanti ai blocchi. Per il
/// resto, l'ordine d'arrivo.
fn prossima_da_preparare(in_fila: &mut VecDeque<DaPreparare>) -> Option<(u64, Vec<u64>)> {
    let ultima = in_fila.iter().map(DaPreparare::sessione).max()?;
    in_fila.retain(|richiesta| richiesta.sessione() == ultima);
    let posto = in_fila
        .iter()
        .position(|richiesta| matches!(richiesta, DaPreparare::Subito { .. }))
        .unwrap_or(0);
    match in_fila.remove(posto)? {
        DaPreparare::Blocco { sessione, uids } => Some((sessione, uids)),
        DaPreparare::Subito { sessione, uid } => Some((sessione, vec![uid])),
    }
}

/// Due valori del campo `insieme` del lettore sono diversi? Per serializzazione:
/// il tipo è della finestra e non sa confrontarsi, e quel che conta è quel che
/// la finestra riceverebbe.
fn diversi(a: &Option<InsiemeNelLettore>, b: &Option<InsiemeNelLettore>) -> bool {
    serde_json::to_string(a).ok() != serde_json::to_string(b).ok()
}

/// Un lavoro dell'esecutore: vedi [`avvia_l_esecutore`].
pub(super) enum Lavoro {
    /// Un gesto di un membro (non `Aggiungi`).
    Azione(Azione),
    /// Le voci di un `Aggiungi`, già registrate, in coda; e la prima a
    /// suonare, se il membro l'ha chiesto.
    Accoda {
        maniglie: Vec<i64>,
        dopo: bool,
        suona: Option<u64>,
    },
    /// Le voci di un membro uscito, fuori dalla coda.
    Togli(Vec<u64>),
}

impl Lavoro {
    /// Il nome per il diario.
    const fn nome(&self) -> &'static str {
        match self {
            Self::Azione(azione) => gesti::nome_dell_azione(azione),
            Self::Accoda { .. } => "aggiungi",
            Self::Togli(_) => "togli le voci di chi esce",
        }
    }
}

/// Un lavoro, e a chi rispondere.
pub(super) struct Incarico {
    lavoro: Lavoro,
    risposta: Option<(IdConnessione, u32)>,
    membro: u32,
    /// Quando è arrivato al filo della sessione: l'attesa in coda si conta da
    /// qui.
    arrivato: Instant,
    /// Di che sessione è: vedi `Filo::sessione_viva`.
    sessione: u64,
}

/// Chi ospita: il filo che esegue i gesti dei membri.
///
/// Sono i comandi della finestra — `pausa`, `prossimo`, `coda_vai`… — e
/// possono aprire una voce: da un disco di rete fino a cinque secondi, dalla
/// rete di un membro fino a quindici. Sul filo della sessione vorrebbe dire
/// niente ancore e niente stato per tutto quel tempo, e una pausa dell'host che
/// non arriva a nessuno. Uno alla volta, nell'ordine d'arrivo: per pausa e
/// salto vince l'ultimo arrivato.
fn avvia_l_esecutore(
    app: AppHandle,
    posta: Sender<Posta>,
    sessione_viva: Arc<AtomicU64>,
) -> Option<Sender<Incarico>> {
    let (coda, orecchio) = channel::<Incarico>();
    let avviato = std::thread::Builder::new()
        .name("rymembra-insieme-esecutore".to_owned())
        .spawn(move || {
            for incarico in orecchio {
                if crate::spegnimento::in_uscita() {
                    return;
                }
                let attesa = incarico.arrivato.elapsed();
                let inizio = Instant::now();
                let nome = incarico.lavoro.nome();
                let codice = if incarico.sessione == sessione_viva.load(Ordering::Acquire) {
                    crate::stato::sorvegliato("rymembra-insieme-esecutore", || {
                        esegui(&app, incarico.lavoro)
                    })
                    .unwrap_or_else(|| Some(ErrorCodeKind::InsiemeRifiutato.code().to_owned()))
                } else {
                    // Di una sessione finita: non si fa, e il membro (se c'è
                    // ancora) sa che non è stato fatto.
                    Some(ErrorCodeKind::InsiemeSuperato.code().to_owned())
                };
                let fatto = Posta::Eseguito {
                    risposta: incarico.risposta,
                    membro: incarico.membro,
                    codice,
                    nome,
                    attesa,
                    durata: inizio.elapsed(),
                };
                if posta.send(fatto).is_err() {
                    return;
                }
            }
        });
    match avviato {
        Ok(_) => Some(coda),
        Err(err) => {
            nota!("[insieme] il filo dei gesti dei membri non parte: {err}");
            None
        }
    }
}

/// Un lavoro dell'esecutore, coi comandi del lettore così come sono: la stessa
/// strada dei gesti di chi ospita, e del tasto multimediale. `None` se è
/// andato (o non c'era niente da fare), altrimenti il codice del rifiuto.
fn esegui(app: &AppHandle, lavoro: Lavoro) -> Option<String> {
    let codice_di = |err: &AppError| codice_sul_filo(err.code().kind().code());
    match lavoro {
        Lavoro::Azione(azione) => fai(app, &azione),
        Lavoro::Accoda {
            maniglie,
            dopo,
            suona,
        } => {
            if let Err(err) = ponte::accoda_maniglie(app, &maniglie, dopo) {
                return Some(codice_di(&err));
            }
            match suona {
                Some(uid) => fai(app, &Azione::Vai { uid }),
                None => None,
            }
        }
        Lavoro::Togli(uids) => ponte::togli_voci(app, &uids)
            .err()
            .map(|err| codice_di(&err)),
    }
}

/// Un'azione diversa da `Aggiungi`, con la coda di adesso: vedi
/// [`gesti::piano`].
fn fai(app: &AppHandle, azione: &Azione) -> Option<String> {
    let rifiutato = || Some(ErrorCodeKind::InsiemeRifiutato.code().to_owned());
    let Some(adesso) = ponte::istantanea(app) else {
        return rifiutato();
    };
    let Some(stato) = app.try_state::<crate::riproduzione::StatoLettore>() else {
        return rifiutato();
    };
    use crate::riproduzione as r;
    let dal_ponte =
        |esito: Result<(), AppError>| esito.map_err(|err| err.code().kind().code().to_owned());
    let dalla_finestra = |esito: crate::errore::Esito<()>| esito.map_err(|err| err.code);
    let esito: Result<(), String> = match gesti::piano(azione, &adesso) {
        Passo::Niente => Ok(()),
        Passo::Superato => Err(ErrorCodeKind::InsiemeSuperato.code().to_owned()),
        Passo::Sconosciuto => Err(ErrorCodeKind::InsiemeGestoNonDisponibile.code().to_owned()),
        Passo::Ripeti(modo) => dal_ponte(r::imposta_ripeti(app, modo)),
        // La posizione la trova il lettore sotto lo stesso lucchetto che la
        // usa: una voce che nel frattempo non c'è più non si suona.
        Passo::Vai(uid) => dal_ponte(ponte::suona_voce(app, uid).map(|_| ())),
        Passo::Togli(uid) => dal_ponte(ponte::togli_voci(app, &[uid])),
        Passo::Pausa => dalla_finestra(r::pausa(app.clone(), stato)),
        Passo::Riprendi => dalla_finestra(r::riprendi(app.clone(), stato)),
        Passo::Prossimo => dalla_finestra(r::prossimo(app.clone(), stato)),
        Passo::Precedente => dalla_finestra(r::precedente(app.clone(), stato)),
        Passo::VaiA(ms) => dalla_finestra(r::vai_a(app.clone(), stato, ms)),
        Passo::Riordina { da, a } => dalla_finestra(r::coda_riordina(app.clone(), stato, da, a)),
    };
    esito.err().map(|codice| codice_sul_filo(&codice))
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

/// Un codice per l'`Esito` sul filo: solo i codici della sessione viaggiano
/// (vedi `messaggi`); ogni altro guasto dell'host è, per chi l'ha chiesto, un
/// gesto rifiutato.
fn codice_sul_filo(codice: &str) -> String {
    let della_sessione = ErrorCodeKind::ALL.iter().any(|kind| {
        kind.domain() == rymembra_domain::errors::Domain::Insieme && kind.code() == codice
    });
    if della_sessione {
        codice.to_owned()
    } else {
        ErrorCodeKind::InsiemeRifiutato.code().to_owned()
    }
}

/// Due voci leggono gli stessi byte: lo stesso gettone, o lo stesso
/// riferimento di catalogo.
fn stessa_sorgente(a: &SorgenteSulFilo, b: &SorgenteSulFilo) -> bool {
    match (a, b) {
        (SorgenteSulFilo::File { gettone: x, .. }, SorgenteSulFilo::File { gettone: y, .. }) => {
            x == y
        }
        (
            SorgenteSulFilo::Catalogo { riferimento: x },
            SorgenteSulFilo::Catalogo { riferimento: y },
        ) => x == y,
        _ => false,
    }
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
mod prove {
    use super::*;
    use rymembra_insieme::messaggi::{Persona, SorgenteSulFilo};
    use rymembra_insieme::{CampioneOrologio, Impronta};

    fn fidati() -> Fidati {
        Fidati::da(vec![rymembra_app::insieme::Pari {
            id: 4,
            chiave: ChiavePubblica([4; 32]).in_testo(),
            nome: "PC di Marco".to_owned(),
            bloccato: false,
            associato_ms: 1,
            visto_ms: None,
        }])
    }

    #[test]
    fn la_voce_che_suona_passa_davanti_e_le_sessioni_vecchie_si_buttano() {
        let mut in_fila: VecDeque<DaPreparare> = VecDeque::from([
            DaPreparare::Blocco {
                sessione: 1,
                uids: vec![1, 2],
            },
            DaPreparare::Blocco {
                sessione: 2,
                uids: vec![10, 11],
            },
            DaPreparare::Blocco {
                sessione: 2,
                uids: vec![12, 13],
            },
            DaPreparare::Subito {
                sessione: 2,
                uid: 40,
            },
        ]);
        assert_eq!(prossima_da_preparare(&mut in_fila), Some((2, vec![40])));
        assert_eq!(prossima_da_preparare(&mut in_fila), Some((2, vec![10, 11])));
        assert_eq!(prossima_da_preparare(&mut in_fila), Some((2, vec![12, 13])));
        assert_eq!(prossima_da_preparare(&mut in_fila), None);
    }

    #[test]
    fn alla_finestra_vanno_solo_chiavi_della_sessione() {
        let della_sessione = AppError::new(ErrorCode::InsiemeIdentitaPersa);
        let del_database = AppError::new(ErrorCode::DbLocked);
        assert_eq!(
            chiave_per_la_finestra(&della_sessione, None),
            Some("errors.insieme.identitaPersa")
        );
        assert_eq!(chiave_per_la_finestra(&del_database, None), None);
        assert_eq!(
            chiave_per_la_finestra(&del_database, Some(ErrorCodeKind::InsiemeRifiutato)),
            Some("errors.insieme.rifiutato")
        );
    }

    #[test]
    fn il_diario_della_rete_non_ha_indirizzi_ne_nomi_ne_codici() {
        let indirizzo: SocketAddr = "192.168.1.57:47811".parse().unwrap();
        let eventi = [
            rymembra_insieme::Evento::Aperta {
                id: IdConnessione(3),
                pari: ChiavePubblica([4; 32]),
                nome: "PC di Marco".to_owned(),
                ospita: true,
                in_entrata: false,
                indirizzo,
            },
            rymembra_insieme::Evento::Aperta {
                id: IdConnessione(5),
                pari: ChiavePubblica([9; 32]),
                nome: "Portatile di Luca".to_owned(),
                ospita: false,
                in_entrata: true,
                indirizzo,
            },
            rymembra_insieme::Evento::Codice {
                id: IdConnessione(5),
                codice: "4821".to_owned(),
            },
            rymembra_insieme::Evento::Chiusa {
                id: IdConnessione(3),
                codice: Some(ErrorCodeKind::InsiemeConnessioneCaduta),
            },
            rymembra_insieme::Evento::Associato {
                id: IdConnessione(5),
                pari: ChiavePubblica([9; 32]),
            },
            rymembra_insieme::Evento::Trovato {
                indirizzo,
                impronta: Impronta([1; 8]),
            },
            rymembra_insieme::Evento::Orologio {
                id: IdConnessione(3),
                campione: CampioneOrologio {
                    t1: 1,
                    t2: 2,
                    t3: 3,
                    t4: 4,
                },
            },
        ];
        let chiave_hex = ChiavePubblica([9; 32]).in_testo();
        let impronta = ChiavePubblica([9; 32]).impronta().to_string();
        let mut righe = Vec::new();
        for evento in &eventi {
            if let Some(riga) = riga_della_rete(evento, &fidati()) {
                for vietato in [
                    "192.168",
                    "47811",
                    "Marco",
                    "Luca",
                    "4821",
                    chiave_hex.as_str(),
                    &chiave_hex[..16],
                    impronta.as_str(),
                ] {
                    assert!(!riga.contains(vietato), "«{vietato}» in «{riga}»");
                }
                righe.push(riga);
            }
        }
        assert_eq!(righe.len(), 5, "{righe:#?}");
        assert!(
            righe[0].contains("pari #4"),
            "il fidato per numero: {}",
            righe[0]
        );
        assert!(righe[1].contains("estraneo"));
        assert!(righe[3].contains("insieme.connessioneCaduta"));
    }

    fn voce(uid: u64, di: u32) -> VoceSulFilo {
        VoceSulFilo {
            uid,
            di,
            titolo: "t".to_owned(),
            artista: "a".to_owned(),
            album: "b".to_owned(),
            durata_ms: 1,
            numero: None,
            anno: None,
            copertina: None,
            sorgente: SorgenteSulFilo::Catalogo {
                riferimento: "x".to_owned(),
            },
            replaygain_db: None,
            picco: None,
            copia: None,
        }
    }

    #[test]
    fn nel_lettore_chi_segue_vede_di_chi_e_la_voce_e_i_suoi_diritti() {
        let persone = vec![
            Persona {
                membro: 0,
                nome: "Sala".to_owned(),

                diritti: Diritti::Tutto,
            },
            Persona {
                membro: 2,
                nome: "Io".to_owned(),

                diritti: Diritti::Tutto,
            },
            Persona {
                membro: 3,
                nome: "Marco".to_owned(),

                diritti: Diritti::Tutto,
            },
        ];
        let dettagli = |agganciato, locale, calibrazione| Dettagli {
            agganciato,
            locale,
            calibrazione,
            revisione: 0,
        };
        let n = nel_lettore_di_chi_segue(
            "Sala (qui)",
            &persone,
            2,
            Diritti::Comanda,
            Some(&voce(1, 0)),
            dettagli(false, false, false),
        );
        assert!(!n.ospita);
        assert_eq!(n.host, "Sala (qui)");
        assert_eq!(n.persone, 3);
        assert_eq!(n.proprietario.as_deref(), Some("Sala"));
        assert!(!n.locale && !n.agganciato && !n.calibrazione);
        assert_eq!(
            n.diritti,
            Diritti::Comanda,
            "quel che chi ospita ha concesso"
        );
        let mia = nel_lettore_di_chi_segue(
            "Sala",
            &persone,
            2,
            Diritti::Tutto,
            Some(&voce(1, 2)),
            dettagli(true, true, true),
        );
        assert_eq!(mia.proprietario, None, "una voce mia non ha proprietario");
        assert!(mia.agganciato && mia.locale && mia.calibrazione);
        let niente = nel_lettore_di_chi_segue(
            "Sala",
            &persone,
            2,
            Diritti::Ascolta,
            None,
            dettagli(true, false, false),
        );
        assert_eq!(niente.proprietario, None);
    }

    #[test]
    fn il_ruolo_va_e_torna_dall_atomico_coi_diritti() {
        for ruolo in [
            Ruolo::Fuori,
            Ruolo::Host,
            Ruolo::Membro(Diritti::Ascolta),
            Ruolo::Membro(Diritti::Comanda),
            Ruolo::Membro(Diritti::Tutto),
        ] {
            assert_eq!(ruolo_dal_byte(ruolo_in_byte(ruolo)), ruolo);
        }
        assert_eq!(ruolo_dal_byte(200), Ruolo::Fuori);
    }

    #[test]
    fn nel_lettore_chi_ospita_e_sempre_agganciato_e_ha_i_gesti() {
        let dettagli = Dettagli {
            agganciato: false,
            locale: true,
            calibrazione: true,
            revisione: 0,
        };
        let n = nel_lettore_di_chi_ospita("Sala", 3, None, dettagli);
        assert!(n.ospita && n.agganciato && n.locale && n.calibrazione);
        assert_eq!(n.diritti, Diritti::Tutto);
        assert_eq!(n.proprietario, None);
        assert_eq!(n.persone, 3);
        let n = nel_lettore_di_chi_ospita("Sala", 3, Some("Marco".to_owned()), dettagli);
        assert_eq!(n.proprietario.as_deref(), Some("Marco"));
    }

    #[test]
    fn il_campo_del_lettore_si_ridisegna_solo_quando_cambia() {
        let dettagli = Dettagli {
            agganciato: true,
            locale: false,
            calibrazione: false,
            revisione: 0,
        };
        let una = Some(nel_lettore_di_chi_ospita("Sala", 2, None, dettagli));
        assert!(
            !diversi(&una, &una.clone()),
            "uguale: niente da ridisegnare"
        );
        assert!(diversi(&una, &None), "all'uscita sì");
        assert!(diversi(&None, &una), "all'ingresso sì");
        let calibrando = Some(nel_lettore_di_chi_ospita(
            "Sala",
            2,
            None,
            Dettagli {
                calibrazione: true,
                revisione: 0,
                ..dettagli
            },
        ));
        assert!(diversi(&una, &calibrando));
    }

    #[test]
    fn chi_ospita_dice_di_chi_e_la_voce_che_suona_solo_se_e_di_un_membro() {
        let persone = vec![
            Persona {
                membro: 0,
                nome: "Sala".to_owned(),

                diritti: Diritti::Tutto,
            },
            Persona {
                membro: 4,
                nome: "Marco".to_owned(),

                diritti: Diritti::Tutto,
            },
        ];
        let mut servite = Servite::default();
        let mut di_marco = voce(1_099_511_627_777, 4);
        di_marco.sorgente = SorgenteSulFilo::File {
            gettone: Gettone([7; 16]),
            estensione: None,
            dimensione: 1,
            segmento: None,
        };
        servite.ospita_voce(di_marco).unwrap();
        assert_eq!(
            proprietario_di_chi_ospita(Some(1_099_511_627_777), &servite, &persone).as_deref(),
            Some("Marco")
        );
        // Una voce di questo PC (che `Servite` descrive con `di: 0`), una che
        // non c'è, e nessuna corrente: nessun proprietario.
        assert_eq!(
            proprietario_di_chi_ospita(Some(3), &servite, &persone),
            None
        );
        assert_eq!(proprietario_di_chi_ospita(None, &servite, &persone), None);
        assert_eq!(
            proprietario_di_chi_ospita(Some(ponte::UID_CALIBRAZIONE), &servite, &persone),
            None
        );
    }

    #[test]
    fn i_proprietari_sono_le_voci_altrui_in_coda_col_nome_del_loro_pc() {
        let persone = vec![
            Persona {
                membro: 0,
                nome: "Sala".to_owned(),

                diritti: Diritti::Tutto,
            },
            Persona {
                membro: 4,
                nome: "Marco".to_owned(),

                diritti: Diritti::Tutto,
            },
        ];
        // Le voci 1 e 3 sono di altri, la 2 di questo PC; la 9 di un membro
        // che non c'è più non si nomina.
        let di = |uid: u64| match uid {
            1 => Some((-11, 0)),
            3 => Some((-13, 4)),
            9 => Some((-19, 7)),
            _ => None,
        };
        let p = proprietari(&[1, 2, 3, 9], &persone, di);
        let coppie: Vec<(i64, &str)> = p.iter().map(|x| (x.id, x.nome.as_str())).collect();
        assert_eq!(coppie, vec![(-11, "Sala"), (-13, "Marco")]);
    }

    #[test]
    fn solo_i_codici_della_sessione_viaggiano_nell_esito() {
        assert_eq!(
            codice_sul_filo("insieme.gestoNonDisponibile"),
            "insieme.gestoNonDisponibile"
        );
        assert_eq!(codice_sul_filo("db.locked"), "insieme.rifiutato");
        assert_eq!(codice_sul_filo("internal.aborted"), "insieme.rifiutato");
    }

    #[test]
    fn la_vetrina_mette_le_scadenze_sull_orologio_di_parete() {
        let mut s = Situazione::default();
        (s, _) = fasi::passo(s, Evento::Acceso, 0);
        (s, _) = fasi::passo(
            s,
            Evento::Aperta {
                id: IdConnessione(3),
                connessione: Connessione {
                    chiave: ChiavePubblica([4; 32]),
                    nome: "PC di Marco".to_owned(),
                    dichiarato: "Marco".to_owned(),
                    ospita: true,
                    in_entrata: false,
                    pari: Some(4),
                },
            },
            0,
        );
        (s, _) = fasi::passo(
            s,
            Evento::Ricevuto {
                id: IdConnessione(3),
                messaggio: Messaggio::Invito {
                    invito: 1,
                    scade_fra_ms: 60_000,
                },
            },
            1_000,
        );
        let impostazioni = rymembra_app::insieme::Impostazioni {
            attivo: true,
            ..Default::default()
        };
        let fidati = fidati();
        let vicinato = Vicinato::default();
        let v = vetrina(&Parti {
            s: &s,
            impostazioni: &impostazioni,
            nome: "Io",
            fidati: &fidati,
            vicinato: &vicinato,
            aggancio: None,
            calibrazione: false,
            proprietari: &[],
            indirizzi_di_qui: &[],
            adesso_ms: 1_700_000_000_000,
            ora: 11_000,
        });
        assert_eq!(v.fase, FaseIpc::Libera);
        assert_eq!(v.inviti.len(), 1);
        assert_eq!(v.inviti[0].scade_ms, 1_700_000_000_000 + 50_000);
        assert_eq!(v.inviti[0].da, "PC di Marco");
        assert_eq!(v.inviti[0].pari, 4);
        assert!(v.fidati[0].presente && v.fidati[0].ospita && !v.fidati[0].in_sessione);
        assert!(v.sincronia.is_none());
    }

    #[test]
    fn la_vetrina_arrotonda_l_errore_al_decimo_di_millisecondo() {
        let s = Situazione::default();
        let impostazioni = rymembra_app::insieme::Impostazioni::default();
        let fidati = Fidati::default();
        let vicinato = Vicinato::default();
        let v = vetrina(&Parti {
            s: &s,
            impostazioni: &impostazioni,
            nome: "Io",
            fidati: &fidati,
            vicinato: &vicinato,
            calibrazione: false,
            proprietari: &[],
            indirizzi_di_qui: &[],
            aggancio: Some(Aggancio {
                agganciato: true,
                errore_ns: Some(1_234_567),
                incertezza_ns: Some(260_000),
            }),
            adesso_ms: 0,
            ora: 0,
        });
        let sincronia = v.sincronia.unwrap();
        assert_eq!(sincronia.errore_ms, Some(1.2));
        assert_eq!(sincronia.incertezza_ms, Some(0.3));
    }
}
