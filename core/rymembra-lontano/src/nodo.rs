//! Il nodo: questo dispositivo in rete, visto dal resto di Rymembra.
//!
//! # Bloccante fuori, asincrono dentro
//!
//! Il [`Nodo`] ha un runtime tokio suo, a due fili («rymembra-lontano»), e
//! ogni suo metodo aspetta: chi lo chiama sta su un filo qualunque di
//! Rymembra, mai dentro questo runtime. Quel che arriva dalla rete e deve
//! toccare la libreria passa all'[`Ospite`] su un filo bloccante del runtime
//! (`spawn_blocking`), mai su uno dei due che fanno girare la rete.
//!
//! Chi lo spegne non aspetta chi lo sta usando: [`Nodo::chiudi`] chiude la
//! porta e ogni connessione subito, e chi lo teneva ancora torna con
//! `lontano.spento`; il runtime se ne va con l'ultimo che lo lascia.
//!
//! # Una connessione per dispositivo
//!
//! Con ogni dispositivo associato si tiene al più una connessione, aperta da
//! chi arriva primo: quella in entrata serve anche le domande in uscita, così
//! basta che **uno** dei due sappia raggiungere l'altro (il telefono dietro
//! un NAT che non si buca chiama il PC, e da lì il PC gli risponde e gli
//! chiede). Ogni domanda è un flusso suo: vedi `protocollo`.
//!
//! # La presenza
//!
//! Per ogni dispositivo associato un compito prova a collegarsi, con
//! un'attesa che cresce da [`PRIMA_PAUSA`] a [`ULTIMA_PAUSA`] finché non
//! risponde; collegato, si annuncia e gli chiede chi è, poi aspetta che la
//! connessione cada (il keepalive di QUIC se ne accorge). Un cambio di rete,
//! o l'altro che si collega da sé, sveglia chi sta aspettando.
//!
//! # Il cancello
//!
//! - [`ALPN_LONTANO`]: una chiave che l'[`Ospite`] non dice associata viene
//!   chiusa subito, col motivo `lontano.nonAssociato`. Il cancello si guarda
//!   di nuovo **a ogni domanda**: chi non è più associato riceve
//!   `lontano.nonAssociato`, e perde ogni sua connessione.
//! - La revoca ([`Nodo::revoca`], o [`Nodo::pari`] senza di lei) chiude
//!   tutte le connessioni di quella chiave, in entrata e in uscita, e i suoi
//!   flussi di «Ascolta insieme» già aperti: il nodo le tiene tutte, non solo
//!   quella che usa per chiedere.
//! - Al più [`DOMANDE_INSIEME`] domande servite insieme per dispositivo, su
//!   tutte le sue connessioni.
//! - [`ALPN_ASSOCIA`]: chiuso se non c'è un QR attivo, senza contare il
//!   tentativo; con un QR, al più [`TENTATIVI_AL_MINUTO`] **da ogni
//!   dispositivo**; il gettone vale [`VALIDITA_INVITO`], una volta sola. Un
//!   gettone sbagliato non tocca il QR: di 128 bit, non si indovina, e
//!   bruciarlo dopo qualche errore dava a chiunque in casa il modo di far
//!   fallire ogni collegamento (reperto R01-3). Il QR lo toglie chi lo mostra
//!   quando non è più a schermo ([`Nodo::annulla_invito`]).
//!   Il gettone si confronta a tempo costante. Un gettone giusto non basta:
//!   decide chi mostra il QR ([`Ospite::conferma`]).

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::str::FromStr as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iroh::dns::DnsResolver;
use iroh::endpoint::{
    ConnectError, ConnectingError, Connection, ConnectionError, PortmapperConfig, RecvStream,
    SendStream, VarInt, presets,
};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointAddr, EndpointId, NetReportConfig, RelayMode, RelayUrl, SecretKey};
use rymembra_domain::errors::{AppError, Domain, ErrorCode, ErrorCodeKind};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::sync::{Notify, Semaphore};
use tokio::task::AbortHandle;

use crate::collegamento::{Invito, MASSIMO_INDIRIZZI};
use crate::errori::{chiave, da_testo, dal_rifiuto, errore, inatteso, irraggiungibile, protocollo};
use crate::flusso::Flusso;
use crate::protocollo::{
    ALPN_ASSOCIA, ALPN_INSIEME, ALPN_LONTANO, Associa, Domanda, MASSIMA_ASSOCIA, MASSIMA_DOMANDA,
    MASSIMA_TESTA, MASSIMO_PEZZO, Presentazione, Presenza, Risposta, Testa, cornice,
    lunghezza_entro, valore_entro,
};

/// Quanto vale un QR.
pub const VALIDITA_INVITO: Duration = Duration::from_secs(10 * 60);

/// Tentativi di associazione al minuto da ogni dispositivo (per chiave),
/// mentre c'è un QR.
///
/// Per chiave e non per tutti: con un tetto comune, chi conosce la chiave di
/// questo nodo (in casa la annuncia mDNS) ne consumava i posti, e a chi
/// inquadrava il QR vero arrivava «QR scaduto» (reperto R01-3). Una chiave
/// nuova non costa niente, quindi il tetto non ferma chi ne cambia: limita il
/// lavoro di ognuno, e non toglie niente agli altri.
pub const TENTATIVI_AL_MINUTO: usize = 10;

/// Quanti dispositivi diversi si ricordano per [`TENTATIVI_AL_MINUTO`]. Oltre,
/// si dimentica chi ha bussato meno di recente: rifiutare uno nuovo vorrebbe
/// dire, di nuovo, un tetto comune.
const MASSIME_FONTI: usize = 256;

/// Quanto si aspetta chi decide, sul dispositivo che mostra il QR: come il
/// consenso di un servizio cloud.
pub const ATTESA_CONFERMA: Duration = Duration::from_secs(3 * 60);

/// Quanto si prova a raggiungere un dispositivo prima di dire che non
/// risponde: il primo contatto passa spesso dal relay, poi si buca il NAT.
pub const ATTESA_CONNESSIONE: Duration = Duration::from_secs(15);

/// Domande servite insieme per ogni dispositivo.
pub const DOMANDE_INSIEME: usize = 8;

/// Connessioni vive tenute per ogni dispositivo, in entrata e in uscita, della
/// libreria e di «Ascolta insieme»: oltre, la più vecchia si chiude.
///
/// Un dispositivo vero ne ha una, due quando i due si chiamano insieme, più un
/// flusso di «Ascolta insieme» per verso. Senza tetto un dispositivo collegato
/// poteva aprirne decine di migliaia, ognuna viva col keepalive, col suo stato
/// QUIC e un compito che la serve (reperto R01-4); i posti delle domande
/// ([`DOMANDE_INSIEME`]) reggevano, le connessioni no.
pub const MASSIME_CONNESSIONI: usize = 4;

/// La prima attesa dopo un dispositivo che non risponde.
pub const PRIMA_PAUSA: Duration = Duration::from_secs(5);

/// L'ultima: un PC spento la sera si ritrova al più cinque minuti dopo
/// l'accensione, senza tenere sveglio il telefono a vuoto.
pub const ULTIMA_PAUSA: Duration = Duration::from_secs(5 * 60);

/// Quanto può pesare una risposta di byte: un pezzo, o una copertina grande.
const MASSIMI_BYTE: u64 = 4 * MASSIMO_PEZZO;

/// Il motivo con cui si chiude chi non è associato.
const CHIUSA_NON_ASSOCIATO: &[u8] = b"lontano.nonAssociato";

/// Il motivo con cui si chiude un'associazione senza un QR valido.
const CHIUSA_SENZA_INVITO: &[u8] = b"lontano.gettoneScaduto";

/// Il motivo con cui chi ha letto il QR chiude perché ha rinunciato.
const CHIUSA_ANNULLATA: &[u8] = b"annullata";

/// Il motivo con cui si chiude la connessione più vecchia di un dispositivo
/// che ne ha aperte troppe ([`MASSIME_CONNESSIONI`]). Non è un codice
/// `lontano.*`: l'altro non deve leggerci un «non associato».
const CHIUSA_TROPPE: &[u8] = b"troppe connessioni";

/// L'associazione annullata da questo lato: non è un guasto, e la finestra
/// non lo dice come tale.
fn annullato_qui() -> AppError {
    AppError::new(ErrorCode::InternalAborted {
        what: Some("collegamento annullato".to_owned()),
    })
}

/// La chiave di questo dispositivo: ed25519, la stessa che lo identifica in
/// rete. La custodisce l'app (nel portachiavi), non questo crate.
#[derive(Clone)]
pub struct Identita(SecretKey);

/// Scritto a mano: è un segreto.
impl std::fmt::Debug for Identita {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Identita(…)")
    }
}

impl Identita {
    /// Una chiave nuova, dal generatore del sistema.
    #[must_use]
    pub fn nuova() -> Self {
        Self(SecretKey::generate())
    }

    /// Una chiave custodita.
    #[must_use]
    pub fn da_byte(byte: &[u8; 32]) -> Self {
        Self(SecretKey::from_bytes(byte))
    }

    /// I byte da custodire.
    #[must_use]
    pub fn byte(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    /// La chiave pubblica, come la si scrive: è l'identità di questo
    /// dispositivo agli occhi degli altri.
    #[must_use]
    pub fn chiave(&self) -> String {
        self.0.public().to_string()
    }
}

/// Dove passa il nodo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rete {
    /// Internet: indirizzi diretti, poi il relay.
    Internet {
        /// Quale relay.
        relay: Relay,
        /// Farsi trovare e cercare gli altri anche nella DHT di BitTorrent.
        dht: bool,
        /// Farsi trovare e cercare gli altri in casa, con mDNS.
        mdns: bool,
    },
    /// Solo questa macchina, senza relay né ricerche: per le prove.
    SoloQuesta,
}

/// Il relay, quando non si riesce a passare diritti.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Relay {
    /// Quelli pubblici di n0.
    N0,
    /// Uno proprio, da questo indirizzo (`https://…`).
    Proprio(String),
}

/// Come si avvia il nodo.
#[derive(Debug, Clone)]
pub struct Impostazioni {
    /// La chiave di questo dispositivo.
    pub identita: Identita,
    /// Dove passa.
    pub rete: Rete,
    /// Come si presenta a chi lo associa.
    pub io: Presentazione,
    /// Cosa dice di sé ai dispositivi associati.
    pub presenza: Presenza,
}

/// Un dispositivo associato, con gli indirizzi che se ne conoscono.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PariNoto {
    /// La sua chiave.
    pub chiave: String,
    /// Indirizzi diretti visti l'ultima volta: i primi da provare.
    pub indirizzi: Vec<SocketAddr>,
}

/// Quel che succede in rete e che l'app deve sapere.
#[derive(Debug, Clone, PartialEq)]
pub enum Evento {
    /// Un dispositivo associato risponde, ed ecco chi è.
    Presente {
        /// La sua chiave.
        chiave: String,
        /// Chi è.
        presenza: Presenza,
    },
    /// Non risponde più.
    Assente {
        /// La sua chiave.
        chiave: String,
    },
    /// Il QR mostrato non vale più: è stato usato, o è scaduto. Chi lo mostra lo toglie dallo schermo —
    /// se nel frattempo non ce n'è già uno nuovo ([`Nodo::invito_attivo`]).
    InvitoFinito,
    /// Qualcosa da scrivere nel diario.
    Diario(String),
}

/// Da dove passa la connessione con un dispositivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    /// Diritta, da indirizzo a indirizzo (anche col NAT bucato).
    Diretta,
    /// Dal relay, che vede solo byte cifrati.
    Relay,
}

/// Quel che il nodo chiede all'app. Tutto bloccante: lo si chiama da un filo
/// bloccante del runtime, mai da uno della rete — tranne
/// [`Ospite::associato`] ed [`Ospite::evento`].
pub trait Ospite: Send + Sync + 'static {
    /// Questa chiave è di un dispositivo associato, e non revocato?
    ///
    /// Il nodo lo chiede a ogni connessione e **prima di ogni domanda**, da
    /// un filo della rete: deve rispondere subito, da quel che ha in memoria,
    /// senza aspettare la libreria. Quando dice no per una chiave che lo era,
    /// le connessioni di quella chiave si chiudono alla sua domanda dopo; per
    /// chiuderle subito, [`Nodo::revoca`].
    fn associato(&self, chiave: &str) -> bool;

    /// Chi ha presentato il gettone giusto vuole collegarsi: lo si chiede a
    /// chi guarda. Torna quando ha risposto, al più dopo [`ATTESA_CONFERMA`]
    /// (e allora è un no).
    fn conferma(&self, chiave: &str, chi: &Presentazione) -> bool;

    /// L'associazione è fatta: da scrivere. La chiama chi mostrava il QR e
    /// chi l'ha letto, ognuno per l'altro.
    fn nuovo_pari(&self, chiave: &str, chi: &Presentazione, indirizzi: &[SocketAddr]);

    /// Una domanda di un dispositivo associato (non `Presenza`, `Annuncio`,
    /// `Dimenticami`: a quelle risponde il nodo).
    ///
    /// # Errori
    ///
    /// Un codice `lontano.*` per l'altro; un errore di un altro dominio gli
    /// arriva come `lontano.protocollo`, e il suo testo resta qui.
    fn rispondi(&self, da: &str, domanda: Domanda) -> Result<Risposta, AppError>;

    /// L'altro ha chiesto di essere dimenticato.
    fn dimenticato(&self, da: &str);

    /// Chi aspettava una conferma ([`Ospite::conferma`]) ha rinunciato: la
    /// connessione dell'associazione si è chiusa. La domanda a chi guarda va
    /// tolta, e [`Ospite::conferma`] deve tornare presto (con un no). Di
    /// serie non fa niente: la conferma scade da sé.
    fn ritirata(&self, chiave: &str) {
        let _ = chiave;
    }

    /// Un dispositivo associato apre un flusso di «Ascolta insieme» (vedi
    /// [`crate::flusso`]): da passare alla sua rete, che lo accoglie come una
    /// connessione in entrata. Di serie si chiude.
    fn insieme(&self, da: &str, flusso: Flusso) {
        let _ = (da, flusso);
    }

    /// Qualcosa è successo.
    fn evento(&self, evento: Evento);
}

/// Il QR mostrato in questo momento.
struct InvitoAttivo {
    gettone: [u8; 16],
    scade: Instant,
}

/// Quel che i compiti del nodo condividono.
struct Condiviso {
    ospite: Arc<dyn Ospite>,
    io: Presentazione,
    presenza: Mutex<Presenza>,
    invito: Mutex<Option<InvitoAttivo>>,
    /// I tentativi di associazione dell'ultimo minuto, per chi li fa: vedi
    /// [`TENTATIVI_AL_MINUTO`].
    tentativi: Mutex<HashMap<EndpointId, VecDeque<Instant>>>,
    connessioni: Mutex<HashMap<EndpointId, Connection>>,
    /// **Tutte** le connessioni vive con ogni dispositivo — in entrata e in
    /// uscita, della libreria e di «Ascolta insieme» — e non solo quella in
    /// `connessioni`: chi esce dal cancello le perde tutte
    /// ([`Condiviso::chiudi_tutte`]).
    aperte: Mutex<HashMap<EndpointId, Vec<Connection>>>,
    /// I posti per le domande di ogni dispositivo ([`DOMANDE_INSIEME`]),
    /// comuni a tutte le sue connessioni: aprirne altre non ne dà di più.
    posti: Mutex<HashMap<EndpointId, Arc<Semaphore>>>,
    /// Un turno per dispositivo per aprire una connessione: dieci domande
    /// insieme a un dispositivo senza connessione ne aprono una, non dieci.
    turni: Mutex<HashMap<EndpointId, Arc<tokio::sync::Mutex<()>>>>,
    noti: Mutex<HashMap<EndpointId, Vec<SocketAddr>>>,
    /// Sveglia i compiti della presenza: un cambio di rete, un dispositivo
    /// che si è collegato da sé.
    sveglia: Notify,
    /// Annulla l'associazione in corso da questo lato ([`Nodo::collega`]).
    annullo: Notify,
    /// Il nodo è acceso: falso da [`Nodo::chiudi`] in poi, anche finché
    /// qualcuno lo tiene ancora. I [`Flusso`] lo guardano prima di ogni
    /// `block_on`, e da chiuso rispondono con un errore invece di usare un
    /// runtime che può non esserci più; i metodi del nodo rispondono
    /// `lontano.spento`.
    vivo: Arc<AtomicBool>,
}

impl std::fmt::Debug for Condiviso {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Condiviso").finish_non_exhaustive()
    }
}

impl Condiviso {
    fn evento(&self, evento: Evento) {
        self.ospite.evento(evento);
    }

    /// La connessione viva con `pari`, se c'è.
    fn viva(&self, pari: EndpointId) -> Option<Connection> {
        chiave(&self.connessioni)
            .get(&pari)
            .filter(|conn| conn.close_reason().is_none())
            .cloned()
    }

    fn registra(&self, pari: EndpointId, conn: &Connection) {
        chiave(&self.connessioni).insert(pari, conn.clone());
        self.sveglia.notify_waiters();
    }

    /// Toglie `conn`, se è ancora quella registrata per `pari`.
    fn togli(&self, pari: EndpointId, conn: &Connection) {
        let mut connessioni = chiave(&self.connessioni);
        if connessioni
            .get(&pari)
            .is_some_and(|attuale| attuale.stable_id() == conn.stable_id())
        {
            connessioni.remove(&pari);
        }
    }

    /// Una connessione con `pari` da chiudere se esce dal cancello. Quelle già
    /// chiuse si tolgono qui: un dispositivo che si ricollega cento volte
    /// non ne lascia cento.
    ///
    /// Al più [`MASSIME_CONNESSIONI`] per dispositivo: oltre, si chiudono le
    /// più vecchie, che per chi si ricollega spesso sono anche quelle su un
    /// percorso già morto.
    fn tieni(&self, pari: EndpointId, conn: &Connection) {
        let oltre: Vec<Connection> = {
            let mut aperte = chiave(&self.aperte);
            let sue = aperte.entry(pari).or_default();
            sue.retain(|c| c.close_reason().is_none());
            sue.push(conn.clone());
            let troppe = sue.len().saturating_sub(MASSIME_CONNESSIONI);
            sue.drain(..troppe).collect()
        };
        for vecchia in oltre {
            vecchia.close(VarInt::from_u32(0), CHIUSA_TROPPE);
        }
    }

    /// `conn` è finita.
    fn lascia(&self, pari: EndpointId, conn: &Connection) {
        let mut aperte = chiave(&self.aperte);
        if let Some(sue) = aperte.get_mut(&pari) {
            sue.retain(|c| c.stable_id() != conn.stable_id());
            if sue.is_empty() {
                aperte.remove(&pari);
            }
        }
    }

    /// Tiene `conn` finché non si chiude da sé: per le connessioni di
    /// «Ascolta insieme», che vivono in mano al loro [`Flusso`].
    fn tieni_finche_vive(self: &Arc<Self>, pari: EndpointId, conn: &Connection) {
        self.tieni(pari, conn);
        let condiviso = Arc::clone(self);
        let conn = conn.clone();
        tokio::spawn(async move {
            conn.closed().await;
            condiviso.lascia(pari, &conn);
        });
    }

    /// `pari` non è più associato: si chiude ogni sua connessione — la
    /// libreria, le domande in corso, i flussi di «Ascolta insieme» — col
    /// motivo `lontano.nonAssociato`, e i suoi posti si dimenticano.
    fn chiudi_tutte(&self, pari: EndpointId) {
        let sue = chiave(&self.aperte).remove(&pari).unwrap_or_default();
        chiave(&self.connessioni).remove(&pari);
        chiave(&self.posti).remove(&pari);
        for conn in sue {
            conn.close(VarInt::from_u32(1), CHIUSA_NON_ASSOCIATO);
        }
    }

    /// I dispositivi con una connessione aperta.
    fn con_connessioni(&self) -> Vec<EndpointId> {
        chiave(&self.aperte).keys().copied().collect()
    }

    /// I posti delle domande di `pari`: vedi [`Condiviso::posti`].
    fn posti_di(&self, pari: EndpointId) -> Arc<Semaphore> {
        Arc::clone(
            chiave(&self.posti)
                .entry(pari)
                .or_insert_with(|| Arc::new(Semaphore::new(DOMANDE_INSIEME))),
        )
    }

    /// Il cancello: `pari` è associato, adesso.
    fn associato(&self, pari: EndpointId) -> bool {
        self.ospite.associato(&pari.to_string())
    }

    fn indirizzo_di(&self, pari: EndpointId) -> EndpointAddr {
        let mut indirizzo = EndpointAddr::new(pari);
        for diretto in chiave(&self.noti).get(&pari).into_iter().flatten() {
            indirizzo = indirizzo.with_ip_addr(*diretto);
        }
        indirizzo
    }

    /// Un tentativo di associazione in più da `da`: `false` oltre il suo
    /// tetto ([`TENTATIVI_AL_MINUTO`]). Gli altri non ne risentono.
    fn tentativo_ammesso(&self, da: EndpointId) -> bool {
        let mut tentativi = chiave(&self.tentativi);
        let adesso = Instant::now();
        let minuto = Duration::from_secs(60);
        // Chi non bussa da un minuto non conta più.
        tentativi.retain(|_, suoi| {
            while suoi
                .front()
                .is_some_and(|quando| adesso.duration_since(*quando) > minuto)
            {
                suoi.pop_front();
            }
            !suoi.is_empty()
        });
        if !tentativi.contains_key(&da)
            && tentativi.len() >= MASSIME_FONTI
            && let Some(meno_recente) = tentativi
                .iter()
                .min_by_key(|(_, suoi)| suoi.back().copied())
                .map(|(chi, _)| *chi)
        {
            tentativi.remove(&meno_recente);
        }
        let suoi = tentativi.entry(da).or_default();
        if suoi.len() >= TENTATIVI_AL_MINUTO {
            return false;
        }
        suoi.push_back(adesso);
        true
    }

    fn invito_attivo(&self) -> bool {
        chiave(&self.invito)
            .as_ref()
            .is_some_and(|invito| invito.scade > Instant::now())
    }

    /// Il gettone presentato. Giusto, il QR non vale più (una volta sola);
    /// sbagliato, il QR resta com'è — vedi «Il cancello» nel `//!`. L'errore
    /// è il codice per l'altro.
    ///
    /// Un QR che smette di valere qui — usato, trovato scaduto — si dice con
    /// [`Evento::InvitoFinito`], fuori dal lucchetto.
    fn consuma(&self, presentato: &str) -> Result<(), &'static str> {
        let (esito, finito) = {
            let mut invito = chiave(&self.invito);
            match invito.as_ref().filter(|i| i.scade > Instant::now()) {
                None => {
                    let c_era = invito.take().is_some();
                    (Err("lontano.gettoneScaduto"), c_era)
                }
                Some(attivo) => {
                    let giusto = da_esadecimale::<16>(presentato)
                        .is_some_and(|gettone| uguali_a_tempo_costante(&gettone, &attivo.gettone));
                    if giusto {
                        *invito = None;
                        (Ok(()), true)
                    } else {
                        (Err("lontano.gettoneScaduto"), false)
                    }
                }
            }
        };
        if finito {
            self.evento(Evento::InvitoFinito);
        }
        esito
    }
}

/// Il nodo. Vedi il `//!`.
pub struct Nodo {
    runtime: Option<tokio::runtime::Runtime>,
    endpoint: Endpoint,
    router: Router,
    condiviso: Arc<Condiviso>,
    seguiti: Mutex<HashMap<EndpointId, AbortHandle>>,
    con_relay: bool,
}

impl std::fmt::Debug for Nodo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Nodo")
            .field("chiave", &self.endpoint.id().fmt_short().to_string())
            .finish_non_exhaustive()
    }
}

impl Nodo {
    /// Accende il nodo: apre i socket, si collega al relay di casa (senza
    /// aspettarlo) e comincia ad accogliere.
    ///
    /// # Errori
    ///
    /// `internal.unexpected` se il runtime o i socket non si aprono;
    /// `net.badSchema` per un relay proprio che non è un indirizzo.
    pub fn avvia(impostazioni: Impostazioni, ospite: Arc<dyn Ospite>) -> Result<Self, AppError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("rymembra-lontano")
            .enable_all()
            .build()
            .map_err(inatteso)?;
        let condiviso = Arc::new(Condiviso {
            ospite,
            io: impostazioni.io,
            presenza: Mutex::new(impostazioni.presenza),
            invito: Mutex::new(None),
            tentativi: Mutex::new(HashMap::new()),
            connessioni: Mutex::new(HashMap::new()),
            aperte: Mutex::new(HashMap::new()),
            posti: Mutex::new(HashMap::new()),
            turni: Mutex::new(HashMap::new()),
            noti: Mutex::new(HashMap::new()),
            sveglia: Notify::new(),
            annullo: Notify::new(),
            vivo: Arc::new(AtomicBool::new(true)),
        });
        let con_relay = matches!(impostazioni.rete, Rete::Internet { .. });
        let segreto = impostazioni.identita.0;
        let (endpoint, router) = runtime.block_on(async {
            let costruttore = match impostazioni.rete {
                Rete::Internet { relay, dht, mdns } => {
                    let mut costruttore = Endpoint::builder(presets::N0)
                        .dns_resolver(risolutore())
                        .net_report_config(rapporto_di_rete());
                    if let Relay::Proprio(indirizzo) = relay {
                        let url = RelayUrl::from_str(indirizzo.trim()).map_err(|err| {
                            AppError::new(ErrorCode::NetBadSchema {
                                service: Some("lontano".to_owned()),
                                detail: Some(format!("il relay non è un indirizzo: {err}")),
                            })
                        })?;
                        costruttore = costruttore.relay_mode(RelayMode::custom([url]));
                    }
                    if dht {
                        costruttore = costruttore.address_lookup(
                            iroh_mainline_address_lookup::DhtAddressLookup::builder(),
                        );
                    }
                    if mdns {
                        costruttore = costruttore
                            .address_lookup(iroh_mdns_address_lookup::MdnsAddressLookup::builder());
                    }
                    costruttore
                }
                Rete::SoloQuesta => Endpoint::builder(presets::Minimal)
                    .relay_mode(RelayMode::Disabled)
                    .portmapper_config(PortmapperConfig::Disabled)
                    .clear_ip_transports()
                    .bind_addr("127.0.0.1:0")
                    .map_err(inatteso)?,
            };
            let endpoint = costruttore
                .secret_key(segreto)
                .bind()
                .await
                .map_err(inatteso)?;
            let router = Router::builder(endpoint.clone())
                .accept(ALPN_ASSOCIA, Associazione(Arc::clone(&condiviso)))
                .accept(ALPN_LONTANO, Servizio(Arc::clone(&condiviso)))
                .accept(ALPN_INSIEME, Insieme(Arc::clone(&condiviso)))
                .spawn();
            Ok::<_, AppError>((endpoint, router))
        })?;
        Ok(Self {
            runtime: Some(runtime),
            endpoint,
            router,
            condiviso,
            seguiti: Mutex::new(HashMap::new()),
            con_relay,
        })
    }

    /// Il runtime, finché il nodo è acceso: chiuso ([`Nodo::chiudi`]) è
    /// `lontano.spento` anche per chi lo tiene ancora.
    fn runtime(&self) -> Result<&tokio::runtime::Runtime, AppError> {
        if !self.condiviso.vivo.load(Ordering::Acquire) {
            return Err(errore(ErrorCodeKind::LontanoSpento));
        }
        self.runtime
            .as_ref()
            .ok_or_else(|| errore(ErrorCodeKind::LontanoSpento))
    }

    /// L'esito di quel che si stava aspettando: se intanto il nodo è stato
    /// chiuso ([`Nodo::chiudi`]), il guasto che ne è venuto — la connessione
    /// chiusa sotto i piedi, l'altro che non risponde più — si dice per quel
    /// che è, `lontano.spento`, e non come un dispositivo irraggiungibile.
    fn finche_acceso<T>(&self, esito: Result<T, AppError>) -> Result<T, AppError> {
        if esito.is_err() && !self.condiviso.vivo.load(Ordering::Acquire) {
            return Err(errore(ErrorCodeKind::LontanoSpento));
        }
        esito
    }

    /// La chiave pubblica di questo dispositivo.
    #[must_use]
    pub fn chiave(&self) -> String {
        self.endpoint.id().to_string()
    }

    /// I socket aperti, per le prove e per il diario.
    #[must_use]
    pub fn socket(&self) -> Vec<SocketAddr> {
        self.endpoint.bound_sockets()
    }

    /// Un QR nuovo: il gettone, e gli indirizzi per raggiungere questo
    /// dispositivo. Quello di prima non vale più.
    ///
    /// # Errori
    ///
    /// `internal.unexpected` se il generatore del sistema non risponde.
    pub fn invita(&self) -> Result<Invito, AppError> {
        let runtime = self.runtime()?;
        let mut gettone = [0_u8; 16];
        getrandom::fill(&mut gettone).map_err(inatteso)?;
        if self.con_relay {
            // Il relay di casa nel QR: chi lo legge da fuori casa passa da lì.
            // Senza aspettarlo troppo: un QR senza relay vale lo stesso in casa.
            runtime.block_on(async {
                let _ = tokio::time::timeout(Duration::from_secs(4), self.endpoint.online()).await;
            });
        }
        let indirizzo = self.endpoint.addr();
        let mut indirizzi: Vec<SocketAddr> = indirizzo
            .ip_addrs()
            .copied()
            .filter(|a| !a.ip().is_unspecified())
            .collect();
        if indirizzi.is_empty() {
            // Prima che il nodo abbia scoperto le sue schede: i socket aperti,
            // se sono su un indirizzo preciso.
            indirizzi = self
                .endpoint
                .bound_sockets()
                .into_iter()
                .filter(|a| !a.ip().is_unspecified())
                .collect();
        }
        // Prima IPv4, che passa quasi ovunque in casa; i loopback solo senza
        // altro (le prove).
        indirizzi.sort_by_key(|a| (a.ip().is_loopback(), a.is_ipv6()));
        if indirizzi.iter().any(|a| !a.ip().is_loopback()) {
            indirizzi.retain(|a| !a.ip().is_loopback());
        }
        indirizzi.truncate(MASSIMO_INDIRIZZI);
        let relay = indirizzo.relay_urls().next().map(ToString::to_string);
        {
            let mut invito = chiave(&self.condiviso.invito);
            // Chiuso mentre si aspettava il relay: un QR non nasce più. Sotto
            // il lucchetto, che `chiudi` prende dopo aver spento `vivo`.
            if !self.condiviso.vivo.load(Ordering::Acquire) {
                return Err(errore(ErrorCodeKind::LontanoSpento));
            }
            *invito = Some(InvitoAttivo {
                gettone,
                scade: Instant::now() + VALIDITA_INVITO,
            });
        }
        Ok(Invito {
            chiave: *self.endpoint.id().as_bytes(),
            gettone,
            indirizzi,
            relay,
            // Chi legge il QR lo vede prima di collegarsi, come nome
            // dichiarato, accanto all'impronta.
            nome: Some(self.condiviso.io.nome.clone()).filter(|nome| !nome.trim().is_empty()),
        })
    }

    /// L'impronta di questo dispositivo, da mostrare accanto al QR e mentre
    /// si aspetta una conferma: vedi [`crate::collegamento::impronta`].
    #[must_use]
    pub fn impronta(&self) -> String {
        crate::collegamento::impronta(self.endpoint.id().as_bytes())
    }

    /// Il QR mostrato non vale più.
    pub fn annulla_invito(&self) {
        *chiave(&self.condiviso.invito) = None;
    }

    /// C'è un QR che vale ancora: né usato, né scaduto.
    #[must_use]
    pub fn invito_attivo(&self) -> bool {
        self.condiviso.invito_attivo()
    }

    /// Smette di aspettare l'associazione in corso da questo lato
    /// ([`Nodo::collega`], che torna con `internal.aborted`). Chi mostrava il
    /// QR vede la connessione chiudersi e toglie la sua domanda
    /// ([`Ospite::ritirata`]). Senza un'associazione in corso non fa niente.
    pub fn annulla_collegamento(&self) {
        self.condiviso.annullo.notify_waiters();
    }

    /// Collega questo dispositivo a quello del link (il QR letto, o il link
    /// incollato), e torna chi è. Aspetta che l'altro accetti.
    ///
    /// # Errori
    ///
    /// `lontano.linkNonValido`, `lontano.irraggiungibile`,
    /// `lontano.gettoneScaduto`, `lontano.rifiutato`, `lontano.protocollo`.
    pub fn collega(&self, link: &str) -> Result<(String, Presentazione), AppError> {
        let runtime = self.runtime()?;
        let invito = Invito::da_link(link)?;
        let pari = EndpointId::from_bytes(&invito.chiave).map_err(|err| {
            AppError::new(ErrorCode::LontanoLinkNonValido).with_cause(err.to_string())
        })?;
        if pari == self.endpoint.id() {
            return Err(AppError::new(ErrorCode::LontanoLinkNonValido)
                .with_cause("il link è di questo stesso dispositivo"));
        }
        let mut indirizzo = EndpointAddr::new(pari);
        for diretto in &invito.indirizzi {
            indirizzo = indirizzo.with_ip_addr(*diretto);
        }
        if let Some(relay) = invito
            .relay
            .as_deref()
            .and_then(|r| RelayUrl::from_str(r).ok())
        {
            indirizzo = indirizzo.with_relay_url(relay);
        }
        let io = self.condiviso.io.clone();
        let gettone = esadecimale(&invito.gettone);
        let annullo = &self.condiviso.annullo;
        let accolto = self.finche_acceso(runtime.block_on(async {
            // Attivato subito: un «Annulla» che arriva mentre si apre la
            // connessione vale come uno che arriva mentre si aspetta.
            let annullato = annullo.notified();
            tokio::pin!(annullato);
            annullato.as_mut().enable();
            let conn = tokio::select! {
                () = &mut annullato => return Err(annullato_qui()),
                esito = tokio::time::timeout(
                    ATTESA_CONNESSIONE,
                    self.endpoint.connect(indirizzo, ALPN_ASSOCIA),
                ) => esito
                    .map_err(|_| irraggiungibile("il dispositivo del QR non risponde"))?
                    // Chi mostra il QR chiude subito un'associazione senza un
                    // QR che valga: la chiusura può arrivare già qui, e dice
                    // perché (`lontano.gettoneScaduto`), non «non risponde».
                    .map_err(|err| chiusa_nel_collegarsi(&err).unwrap_or_else(|| irraggiungibile(err)))?,
            };
            let esito = tokio::select! {
                () = &mut annullato => {
                    conn.close(VarInt::from_u32(1), CHIUSA_ANNULLATA);
                    return Err(annullato_qui());
                }
                esito = tokio::time::timeout(ATTESA_CONFERMA + Duration::from_secs(30), async {
                    // Ogni passo col motivo della chiusura, se l'altro ha
                    // chiuso: quale passo trovi la connessione chiusa è una
                    // corsa, l'esito non deve esserlo.
                    let (mut invio, mut ricevi) =
                        conn.open_bi().await.map_err(|e| perso(&conn, e))?;
                    scrivi(
                        &mut invio,
                        &Associa::Chiedo { gettone, chi: io },
                        MASSIMA_ASSOCIA,
                    )
                    .await
                        .map_err(|err| chiusa_perche(&conn).unwrap_or(err))?;
                    let _ = invio.finish();
                    // Chi mostra il QR non lo si conosce ancora: la sua
                    // risposta ha il tetto dell'associazione (reperto R15-1).
                    leggi::<Associa>(&mut ricevi, MASSIMA_ASSOCIA)
                        .await
                        .map_err(|err| chiusa_perche(&conn).unwrap_or(err))
                }) => esito.map_err(|_| irraggiungibile("nessuna risposta all'associazione"))?,
            };
            conn.close(VarInt::from_u32(0), b"fatto");
            esito
        }))?;
        match accolto {
            Associa::Accolto { chi } => {
                let chiave_pari = pari.to_string();
                self.condiviso
                    .ospite
                    .nuovo_pari(&chiave_pari, &chi, &invito.indirizzi);
                chiave(&self.condiviso.noti).insert(pari, invito.indirizzi);
                Ok((chiave_pari, chi))
            }
            Associa::Rifiutato { codice } => Err(dal_rifiuto(&codice)),
            Associa::Chiedo { .. } | Associa::Sconosciuta => {
                Err(protocollo("una risposta all'associazione che non lo è"))
            }
        }
    }

    /// Una chiave che non è più associata — dimenticata qui, o che ci ha
    /// dimenticati — esce subito: si smette di seguirla, e si chiude **ogni**
    /// sua connessione, anche quelle aperte prima e i flussi di «Ascolta
    /// insieme» già in corso. Da chiamare dopo che [`Ospite::associato`] dice
    /// no per lei; [`Nodo::pari`] fa lo stesso per chi non è più nell'elenco.
    pub fn revoca(&self, chiave_pari: &str) {
        let Ok(id) = EndpointId::from_str(chiave_pari) else {
            return;
        };
        if let Some(compito) = chiave(&self.seguiti).remove(&id) {
            compito.abort();
        }
        self.condiviso.chiudi_tutte(id);
    }

    /// I dispositivi associati da seguire: chi non c'è più si lascia — con
    /// ogni sua connessione, vedi [`Nodo::revoca`] — chi è nuovo si comincia
    /// a cercare.
    pub fn pari(&self, noti: Vec<PariNoto>) {
        let Ok(runtime) = self.runtime() else {
            return;
        };
        let mut voluti = HashMap::new();
        for noto in noti {
            if let Ok(id) = EndpointId::from_str(&noto.chiave) {
                voluti.insert(id, noto.indirizzi);
            }
        }
        // Gli indirizzi prima dei compiti: il primo tentativo di un compito
        // nuovo li trova già.
        let ids: Vec<EndpointId> = voluti.keys().copied().collect();
        *chiave(&self.condiviso.noti) = voluti;
        let voluti = ids;
        let mut seguiti = chiave(&self.seguiti);
        seguiti.retain(|id, compito| {
            let resta = voluti.contains(id);
            if !resta {
                compito.abort();
            }
            resta
        });
        // Anche chi non si seguiva ancora: una connessione arrivata subito
        // dopo l'associazione, prima del primo elenco.
        for id in self.condiviso.con_connessioni() {
            if !voluti.contains(&id) {
                self.condiviso.chiudi_tutte(id);
            }
        }
        for id in voluti {
            seguiti.entry(id).or_insert_with(|| {
                runtime
                    .spawn(segui(
                        Arc::clone(&self.condiviso),
                        self.endpoint.clone(),
                        id,
                    ))
                    .abort_handle()
            });
        }
    }

    /// Chi aspetta di riprovare un dispositivo che non rispondeva riprova
    /// adesso, dalla prima pausa: la persona ha aperto la sua pagina, e un PC
    /// acceso dopo ore di sonno non deve aspettare l'ultima pausa.
    pub fn riprova_adesso(&self) {
        self.condiviso.sveglia.notify_waiters();
    }

    /// Da dove passa adesso la connessione con `pari`, e il suo tempo di
    /// andata e ritorno. `None` se non c'è una connessione viva.
    #[must_use]
    pub fn via(&self, pari: &str) -> Option<(Via, Duration)> {
        let id = EndpointId::from_str(pari).ok()?;
        let conn = self.condiviso.viva(id)?;
        let percorsi = conn.paths();
        let scelto = percorsi.iter().find(|percorso| percorso.is_selected())?;
        let via = if scelto.is_relay() {
            Via::Relay
        } else {
            Via::Diretta
        };
        Some((via, scelto.rtt()))
    }

    /// Il dispositivo risponde adesso?
    #[must_use]
    pub fn raggiungibile(&self, pari: &str) -> bool {
        EndpointId::from_str(pari).is_ok_and(|id| self.condiviso.viva(id).is_some())
    }

    /// Gli indirizzi diretti da cui risponde adesso, da ricordare.
    #[must_use]
    pub fn indirizzi_di(&self, pari: &str) -> Vec<SocketAddr> {
        let Ok(id) = EndpointId::from_str(pari) else {
            return Vec::new();
        };
        let Some(conn) = self.condiviso.viva(id) else {
            return Vec::new();
        };
        conn.paths()
            .iter()
            .filter_map(|percorso| match percorso.remote_addr() {
                iroh::TransportAddr::Ip(diretto) => Some(*diretto),
                _ => None,
            })
            .collect()
    }

    /// Cosa questo dispositivo dice di sé: cambiata, la si annuncia a chi è
    /// collegato.
    pub fn presenza(&self, presenza: Presenza) {
        let Ok(runtime) = self.runtime() else {
            return;
        };
        {
            let mut attuale = chiave(&self.condiviso.presenza);
            if *attuale == presenza {
                return;
            }
            *attuale = presenza.clone();
        }
        let connessioni: Vec<Connection> = chiave(&self.condiviso.connessioni)
            .values()
            .cloned()
            .collect();
        for conn in connessioni {
            let presenza = presenza.clone();
            runtime.spawn(async move {
                let _ = tokio::time::timeout(
                    Duration::from_secs(10),
                    chiedi_su(&conn, Domanda::Annuncio(presenza)),
                )
                .await;
            });
        }
    }

    /// Una domanda a un dispositivo associato, con la sua scadenza.
    ///
    /// # Errori
    ///
    /// `lontano.irraggiungibile` se non risponde in tempo; quel che risponde
    /// lui (`lontano.nonAssociato`, `lontano.fileCambiato`…).
    pub fn chiedi(
        &self,
        pari: &str,
        domanda: Domanda,
        scadenza: Duration,
    ) -> Result<Risposta, AppError> {
        let runtime = self.runtime()?;
        let id =
            EndpointId::from_str(pari).map_err(|_| errore(ErrorCodeKind::LontanoNonAssociato))?;
        let condiviso = Arc::clone(&self.condiviso);
        let endpoint = self.endpoint.clone();
        self.finche_acceso(runtime.block_on(async move {
            tokio::time::timeout(scadenza, async {
                let conn = connessione(&condiviso, &endpoint, id).await?;
                chiedi_su(&conn, domanda.nei_tetti()).await
            })
            .await
            .map_err(|_| irraggiungibile("la domanda è scaduta"))?
        }))
    }

    /// Un flusso di «Ascolta insieme» verso un dispositivo associato, su una
    /// connessione sua (vedi [`crate::flusso`]), entro `entro`.
    ///
    /// # Errori
    ///
    /// `lontano.nonAssociato` per una chiave che non lo è;
    /// `lontano.irraggiungibile` se non risponde in tempo.
    pub fn apri_insieme(&self, pari: &str, entro: Duration) -> Result<Flusso, AppError> {
        if !self.condiviso.ospite.associato(pari) {
            return Err(errore(ErrorCodeKind::LontanoNonAssociato));
        }
        let runtime = self.runtime()?;
        let id =
            EndpointId::from_str(pari).map_err(|_| errore(ErrorCodeKind::LontanoNonAssociato))?;
        let indirizzo = self.condiviso.indirizzo_di(id);
        let endpoint = self.endpoint.clone();
        let maniglia = runtime.handle().clone();
        let vivo = Arc::clone(&self.condiviso.vivo);
        let condiviso = Arc::clone(&self.condiviso);
        self.finche_acceso(runtime.block_on(async move {
            tokio::time::timeout(entro, async {
                let conn = endpoint
                    .connect(indirizzo, ALPN_INSIEME)
                    .await
                    .map_err(irraggiungibile)?;
                // Come in entrata: la revoca chiude anche i flussi aperti da qui.
                condiviso.tieni_finche_vive(id, &conn);
                if !condiviso.associato(id) {
                    conn.close(VarInt::from_u32(1), CHIUSA_NON_ASSOCIATO);
                    return Err(errore(ErrorCodeKind::LontanoNonAssociato));
                }
                let (invio, ricevi) = conn.open_bi().await.map_err(|err| perso(&conn, err))?;
                Ok(Flusso::nuovo(conn, invio, ricevi, maniglia, vivo))
            })
            .await
            .map_err(|_| irraggiungibile("il dispositivo non risponde"))?
        }))
    }

    /// La rete è cambiata (il Wi-Fi è andato, è arrivato il 5G): si cercano
    /// strade nuove, e chi aspettava riprova subito.
    pub fn cambio_di_rete(&self) {
        let Ok(runtime) = self.runtime() else {
            return;
        };
        let endpoint = self.endpoint.clone();
        runtime.spawn(async move { endpoint.network_change().await });
        self.condiviso.sveglia.notify_waiters();
    }

    /// Spegne il nodo: chiude le connessioni e i socket.
    pub fn ferma(mut self) {
        self.spegni();
    }

    /// Chiude il nodo **adesso**, anche se altri lo tengono ancora (un
    /// comando in volo, in un `Arc`): il QR non vale più, non si segue più
    /// nessuno, non si accoglie più niente — né associazioni, né domande, né
    /// «Ascolta insieme» — e ogni connessione si chiude. Chi lo stava usando
    /// torna con `lontano.spento`, e così ogni chiamata dopo. Il runtime se ne
    /// va con l'ultimo che lo lascia ([`Nodo::ferma`], o il drop). Una volta
    /// sola: le chiamate dopo non fanno niente.
    ///
    /// I socket UDP restano aperti finché vive qualche copia dell'endpoint,
    /// ma non li legge più nessuno: un pacchetto che arriva non ha risposta.
    pub fn chiudi(&self) {
        let Some(runtime) = self.runtime.as_ref() else {
            return;
        };
        // Per primo: da qui chi chiama un metodo, o guarda un flusso, lo
        // trova spento, e chi aspettava e si sveglia per la chiusura lo dice
        // (`finche_acceso`).
        if !self.condiviso.vivo.swap(false, Ordering::AcqRel) {
            return;
        }
        *chiave(&self.condiviso.invito) = None;
        // Chi si stava collegando da qui smette di aspettare, anche se la
        // connessione non c'è ancora.
        self.condiviso.annullo.notify_waiters();
        for (_, compito) in chiave(&self.seguiti).drain() {
            compito.abort();
        }
        // Il router chiude l'endpoint: ogni connessione, in entrata e in
        // uscita, e chi aspetta su di lei si sveglia.
        let router = self.router.clone();
        runtime.block_on(async move {
            let _ = tokio::time::timeout(Duration::from_secs(3), router.shutdown()).await;
        });
    }

    fn spegni(&mut self) {
        self.chiudi();
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_timeout(Duration::from_secs(2));
        }
    }
}

impl Drop for Nodo {
    fn drop(&mut self) {
        self.spegni();
    }
}

/// Segue un dispositivo associato: vedi «La presenza» nel `//!`.
async fn segui(condiviso: Arc<Condiviso>, endpoint: Endpoint, pari: EndpointId) {
    let mut pausa = PRIMA_PAUSA;
    let testo = pari.to_string();
    loop {
        match connessione(&condiviso, &endpoint, pari).await {
            Ok(conn) => {
                pausa = PRIMA_PAUSA;
                let io = chiave(&condiviso.presenza).clone();
                let _ = tokio::time::timeout(
                    Duration::from_secs(10),
                    chiedi_su(&conn, Domanda::Annuncio(io)),
                )
                .await;
                if let Ok(Ok(Risposta::Presenza(presenza))) = tokio::time::timeout(
                    Duration::from_secs(10),
                    chiedi_su(&conn, Domanda::Presenza),
                )
                .await
                {
                    condiviso.evento(Evento::Presente {
                        chiave: testo.clone(),
                        presenza,
                    });
                }
                conn.closed().await;
                condiviso.togli(pari, &conn);
                // Due dispositivi che si chiamano insieme hanno due
                // connessioni: se cade questa e resta l'altra, il dispositivo
                // c'è ancora, e si segue quella.
                if condiviso.viva(pari).is_some() {
                    continue;
                }
                condiviso.evento(Evento::Assente {
                    chiave: testo.clone(),
                });
                // Una connessione che cade appena aperta non fa girare a vuoto.
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(_) => {
                tokio::select! {
                    () = tokio::time::sleep(pausa) => {
                        pausa = pausa.saturating_mul(2).min(ULTIMA_PAUSA);
                    }
                    () = condiviso.sveglia.notified() => {
                        pausa = PRIMA_PAUSA;
                    }
                }
            }
        }
    }
}

/// La connessione con `pari`: quella viva, o una nuova (che da qui serve
/// anche le sue domande).
async fn connessione(
    condiviso: &Arc<Condiviso>,
    endpoint: &Endpoint,
    pari: EndpointId,
) -> Result<Connection, AppError> {
    if let Some(conn) = condiviso.viva(pari) {
        return Ok(conn);
    }
    // Uno alla volta per dispositivo: chi arriva dopo trova la connessione
    // aperta da chi l'ha preceduto.
    let turno = Arc::clone(chiave(&condiviso.turni).entry(pari).or_default());
    let _turno = turno.lock().await;
    if let Some(conn) = condiviso.viva(pari) {
        return Ok(conn);
    }
    let indirizzo = condiviso.indirizzo_di(pari);
    let conn = tokio::time::timeout(
        ATTESA_CONNESSIONE,
        endpoint.connect(indirizzo, ALPN_LONTANO),
    )
    .await
    .map_err(|_| irraggiungibile("il dispositivo non risponde"))?
    .map_err(irraggiungibile)?;
    condiviso.tieni(pari, &conn);
    condiviso.registra(pari, &conn);
    let condiviso = Arc::clone(condiviso);
    let servita = conn.clone();
    tokio::spawn(async move {
        servi(&condiviso, &servita).await;
        condiviso.togli(pari, &servita);
        condiviso.lascia(pari, &servita);
    });
    Ok(conn)
}

/// Le domande che arrivano su una connessione, finché resta aperta. Al più
/// [`DOMANDE_INSIEME`] servite insieme per dispositivo, su tutte le sue
/// connessioni: chi ne apre cento non ne fa servire ottocento.
async fn servi(condiviso: &Arc<Condiviso>, conn: &Connection) {
    let da = conn.remote_id();
    let posti = condiviso.posti_di(da);
    while let Ok((invio, ricevi)) = conn.accept_bi().await {
        let Ok(posto) = Arc::clone(&posti).acquire_owned().await else {
            break;
        };
        let condiviso = Arc::clone(condiviso);
        tokio::spawn(async move {
            let _posto = posto;
            if let Err(guasto) = rispondi_a(&condiviso, da, invio, ricevi).await {
                // Il codice e basta: né la chiave dell'altro né la causa, che
                // per un guasto di rete può portare un indirizzo.
                condiviso.evento(Evento::Diario(format!(
                    "[lontano] una domanda non ha avuto risposta codice={}",
                    guasto.code().kind().code()
                )));
            }
        });
    }
}

/// Una domanda, e la sua risposta.
async fn rispondi_a(
    condiviso: &Arc<Condiviso>,
    da: EndpointId,
    mut invio: SendStream,
    mut ricevi: RecvStream,
) -> Result<(), AppError> {
    let domanda: Domanda =
        tokio::time::timeout(Duration::from_secs(30), leggi(&mut ricevi, MASSIMA_DOMANDA))
            .await
            .map_err(|_| protocollo("la domanda non arriva"))??;
    // Il cancello a ogni domanda, non solo quando la connessione si apre: un
    // dispositivo dimenticato non legge più niente nemmeno sulle connessioni
    // che aveva già, e chi esce le perde tutte. Anche la presenza: chi non è
    // associato non si fa vedere presente.
    if !condiviso.associato(da) {
        scrivi_risposta(&mut invio, Err(errore(ErrorCodeKind::LontanoNonAssociato))).await?;
        let _ = tokio::time::timeout(Duration::from_secs(2), invio.stopped()).await;
        condiviso.chiudi_tutte(da);
        return Ok(());
    }
    let testo = da.to_string();
    let mut dimentica = false;
    let esito = match domanda.nei_tetti() {
        Domanda::Presenza => Ok(Risposta::Presenza(chiave(&condiviso.presenza).clone())),
        Domanda::Annuncio(presenza) => {
            condiviso.evento(Evento::Presente {
                chiave: testo,
                presenza,
            });
            Ok(Risposta::Fatto)
        }
        Domanda::Sconosciuta => Err(protocollo("una domanda di una versione più nuova")),
        Domanda::Dimenticami => {
            dimentica = true;
            let ospite = Arc::clone(&condiviso.ospite);
            tokio::task::spawn_blocking(move || ospite.dimenticato(&testo))
                .await
                .map(|()| Risposta::Fatto)
                .map_err(inatteso)
        }
        altra => {
            let ospite = Arc::clone(&condiviso.ospite);
            tokio::task::spawn_blocking(move || ospite.rispondi(&testo, altra))
                .await
                .map_err(inatteso)
                .and_then(|esito| esito)
        }
    };
    let esito = esito.inspect_err(|guasto| {
        if guasto.code().kind().domain() != Domain::Lontano {
            condiviso.evento(Evento::Diario(format!(
                "[lontano] una domanda è caduta codice={}",
                guasto.code().kind().code()
            )));
        }
    });
    scrivi_risposta(&mut invio, esito).await?;
    if dimentica {
        // Dopo la risposta, che deve arrivare: chiudere prima la perderebbe.
        let _ = tokio::time::timeout(Duration::from_secs(2), invio.stopped()).await;
        condiviso.chiudi_tutte(da);
    }
    Ok(())
}

/// Scrive una risposta: la testa, e per dei byte i byte.
async fn scrivi_risposta(
    invio: &mut SendStream,
    esito: Result<Risposta, AppError>,
) -> Result<(), AppError> {
    match esito {
        Ok(Risposta::Byte {
            byte,
            totale,
            versione,
        }) => {
            let lunghezza = u64::try_from(byte.len()).map_err(protocollo)?;
            scrivi(
                invio,
                &Testa::Byte {
                    lunghezza,
                    totale,
                    versione,
                },
                MASSIMA_TESTA,
            )
            .await?;
            invio.write_all(&byte).await.map_err(irraggiungibile)?;
        }
        Ok(Risposta::Presenza(presenza)) => {
            scrivi(invio, &Testa::Presenza(presenza), MASSIMA_TESTA).await?;
        }
        Ok(Risposta::Dati(dati)) => scrivi(invio, &Testa::Dati { dati }, MASSIMA_TESTA).await?,
        Ok(Risposta::Fatto) => scrivi(invio, &Testa::Fatto, MASSIMA_TESTA).await?,
        Err(guasto) => {
            // Fra due dispositivi viaggiano solo i codici di questo dominio:
            // vedi `errori`.
            let kind = guasto.code().kind();
            let codice = if kind.domain() == Domain::Lontano {
                kind.code()
            } else {
                ErrorCodeKind::LontanoProtocollo.code()
            };
            scrivi(
                invio,
                &Testa::Rifiuto {
                    codice: codice.to_owned(),
                },
                MASSIMA_TESTA,
            )
            .await?;
        }
    }
    invio.finish().map_err(irraggiungibile)
}

/// Una domanda su una connessione già aperta.
async fn chiedi_su(conn: &Connection, domanda: Domanda) -> Result<Risposta, AppError> {
    let (mut invio, mut ricevi) = conn.open_bi().await.map_err(|e| perso(conn, e))?;
    // Il tetto di chi la leggerà: oltre, qui si sa subito perché.
    scrivi(&mut invio, &domanda, MASSIMA_DOMANDA)
        .await
        .map_err(|err| chiusa_perche(conn).unwrap_or(err))?;
    let _ = invio.finish();
    leggi_risposta(&mut ricevi)
        .await
        .map_err(|err| chiusa_perche(conn).unwrap_or(err))
}

/// Legge la risposta: la testa, e per dei byte i byte.
async fn leggi_risposta(ricevi: &mut RecvStream) -> Result<Risposta, AppError> {
    match leggi::<Testa>(ricevi, MASSIMA_TESTA).await? {
        // Come l'annuncio (`Domanda::nei_tetti`): quel che l'altro dice di sé
        // arriva al pannello solo nei tetti.
        Testa::Presenza(presenza) => Ok(Risposta::Presenza(presenza.nei_tetti())),
        Testa::Dati { dati } => Ok(Risposta::Dati(dati)),
        Testa::Fatto => Ok(Risposta::Fatto),
        Testa::Byte {
            lunghezza,
            totale,
            versione,
        } => {
            if lunghezza > MASSIMI_BYTE {
                return Err(protocollo(format!("{lunghezza} byte in una risposta")));
            }
            let mut byte = vec![0_u8; usize::try_from(lunghezza).map_err(protocollo)?];
            ricevi
                .read_exact(&mut byte)
                .await
                .map_err(irraggiungibile)?;
            Ok(Risposta::Byte {
                byte,
                totale,
                versione,
            })
        }
        Testa::Rifiuto { codice } => Err(dal_rifiuto(&codice)),
        Testa::Sconosciuta => Err(protocollo("una risposta di una versione più nuova")),
    }
}

/// Scrive una cornice su un flusso, entro il `tetto` di chi la legge.
async fn scrivi(
    invio: &mut SendStream,
    valore: &impl Serialize,
    tetto: usize,
) -> Result<(), AppError> {
    let byte = cornice(valore, tetto)?;
    invio.write_all(&byte).await.map_err(irraggiungibile)
}

/// Legge una cornice da un flusso: al più `tetto` byte di JSON aperto, che
/// chi legge sceglie secondo cosa si aspetta e da chi (vedi
/// `protocollo::MASSIMA_ASSOCIA`).
async fn leggi<T: DeserializeOwned>(ricevi: &mut RecvStream, tetto: usize) -> Result<T, AppError> {
    let mut testa = [0_u8; 4];
    ricevi
        .read_exact(&mut testa)
        .await
        .map_err(irraggiungibile)?;
    let (quanti, compressa) = lunghezza_entro(testa, tetto)?;
    let mut corpo = vec![0_u8; quanti];
    ricevi
        .read_exact(&mut corpo)
        .await
        .map_err(irraggiungibile)?;
    valore_entro(&corpo, compressa, tetto)
}

/// Il guasto di un flusso, detto col motivo della chiusura se l'altro ha
/// chiuso con un codice (non associato, QR scaduto).
fn perso(conn: &Connection, err: impl std::fmt::Display) -> AppError {
    chiusa_perche(conn).unwrap_or_else(|| irraggiungibile(err))
}

/// Il codice con cui l'altro ha chiuso la connessione, se ne ha dato uno.
fn chiusa_perche(conn: &Connection) -> Option<AppError> {
    motivo(&conn.close_reason()?)
}

/// Lo stesso, per una connessione che l'altro ha chiuso mentre la si apriva.
fn chiusa_nel_collegarsi(err: &ConnectError) -> Option<AppError> {
    match err {
        ConnectError::Connection { source, .. }
        | ConnectError::Connecting {
            source: ConnectingError::ConnectionError { source, .. },
            ..
        } => motivo(source),
        _ => None,
    }
}

/// Il codice `lontano.*` di una chiusura, se l'altro ne ha dato uno.
fn motivo(chiusura: &ConnectionError) -> Option<AppError> {
    match chiusura {
        ConnectionError::ApplicationClosed(chiusura) => std::str::from_utf8(&chiusura.reason)
            .ok()
            .and_then(da_testo)
            .map(errore),
        _ => None,
    }
}

/// L'associazione col QR: vedi «Il cancello» nel `//!`.
#[derive(Debug, Clone)]
struct Associazione(Arc<Condiviso>);

impl ProtocolHandler for Associazione {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let condiviso = &self.0;
        // Prima il QR, poi il tetto: senza un QR a schermo si chiude senza
        // contare. Contati prima, chi conosce la chiave di questo nodo (in
        // casa la annuncia mDNS) consumava i tentativi del minuto quando non
        // c'era niente da indovinare, e il QR vero, mostrato subito dopo, si
        // diceva scaduto a chi lo leggeva davvero (reperto 02-12). E il tetto
        // è di chi bussa, non di tutti (reperto R01-3).
        let da = conn.remote_id();
        if !condiviso.invito_attivo() || !condiviso.tentativo_ammesso(da) {
            conn.close(VarInt::from_u32(1), CHIUSA_SENZA_INVITO);
            return Ok(());
        }
        let esito = tokio::time::timeout(
            ATTESA_CONFERMA + Duration::from_secs(20),
            associa(condiviso, &conn, da),
        )
        .await;
        if let Ok(Err(guasto)) = esito {
            condiviso.evento(Evento::Diario(format!(
                "[lontano] un'associazione non è andata codice={}",
                guasto.code().kind().code()
            )));
        }
        Ok(())
    }
}

async fn associa(
    condiviso: &Arc<Condiviso>,
    conn: &Connection,
    da: EndpointId,
) -> Result<(), AppError> {
    let (mut invio, mut ricevi) = conn.accept_bi().await.map_err(irraggiungibile)?;
    // Prima del gettone, da uno che non si conosce: il tetto
    // dell'associazione, non quello di tutte le cornici (reperto R15-1).
    let domanda: Associa =
        tokio::time::timeout(Duration::from_secs(10), leggi(&mut ricevi, MASSIMA_ASSOCIA))
            .await
            .map_err(|_| protocollo("la domanda di associazione non arriva"))??;
    let Associa::Chiedo { gettone, chi } = domanda else {
        return Err(protocollo("un'associazione senza domanda"));
    };
    let risposta = match condiviso.consuma(&gettone) {
        Err(codice) => Associa::Rifiutato {
            codice: codice.to_owned(),
        },
        Ok(()) => {
            let ospite = Arc::clone(&condiviso.ospite);
            let testo = da.to_string();
            let per_chi = chi.clone();
            let mut attesa = tokio::task::spawn_blocking(move || ospite.conferma(&testo, &per_chi));
            let si = tokio::select! {
                esito = &mut attesa => esito.unwrap_or(false),
                // Chi ha letto il QR ha rinunciato: la domanda a chi guarda
                // non ha più senso, e la si toglie.
                _ = conn.closed() => {
                    let ospite = Arc::clone(&condiviso.ospite);
                    let testo = da.to_string();
                    let _ = tokio::task::spawn_blocking(move || ospite.ritirata(&testo)).await;
                    let _ = attesa.await;
                    return Ok(());
                }
            };
            if si {
                let ospite = Arc::clone(&condiviso.ospite);
                let testo = da.to_string();
                let indirizzi: Vec<SocketAddr> = conn
                    .paths()
                    .iter()
                    .filter_map(|percorso| match percorso.remote_addr() {
                        iroh::TransportAddr::Ip(diretto) => Some(*diretto),
                        _ => None,
                    })
                    .collect();
                chiave(&condiviso.noti).insert(da, indirizzi.clone());
                tokio::task::spawn_blocking(move || ospite.nuovo_pari(&testo, &chi, &indirizzi))
                    .await
                    .map_err(inatteso)?;
                Associa::Accolto {
                    chi: condiviso.io.clone(),
                }
            } else {
                Associa::Rifiutato {
                    codice: ErrorCodeKind::LontanoRifiutato.code().to_owned(),
                }
            }
        }
    };
    scrivi(&mut invio, &risposta, MASSIMA_ASSOCIA).await?;
    let _ = invio.finish();
    // Chi ha chiesto chiude quando ha letto: aspettarlo fa arrivare la
    // risposta intera prima che la connessione se ne vada.
    let _ = tokio::time::timeout(Duration::from_secs(5), conn.closed()).await;
    Ok(())
}

/// La libreria per i dispositivi associati: vedi «Il cancello» nel `//!`.
#[derive(Debug, Clone)]
struct Servizio(Arc<Condiviso>);

impl ProtocolHandler for Servizio {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let condiviso = &self.0;
        let da = conn.remote_id();
        // Una lettura e basta (vedi `Ospite::associato`): senza un filo
        // bloccante, che cento connessioni potrebbero tenere tutti occupati.
        if !condiviso.associato(da) {
            conn.close(VarInt::from_u32(1), CHIUSA_NON_ASSOCIATO);
            return Ok(());
        }
        condiviso.tieni(da, &conn);
        condiviso.registra(da, &conn);
        servi(condiviso, &conn).await;
        condiviso.togli(da, &conn);
        condiviso.lascia(da, &conn);
        Ok(())
    }
}

/// «Ascolta insieme» via internet, per i dispositivi associati: il cancello
/// della libreria, poi il primo bi-stream, che diventa un [`Flusso`] per
/// l'[`Ospite`]. La connessione la tiene il flusso: questo compito finisce.
#[derive(Debug, Clone)]
struct Insieme(Arc<Condiviso>);

impl ProtocolHandler for Insieme {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let condiviso = &self.0;
        let id = conn.remote_id();
        let da = id.to_string();
        if !condiviso.associato(id) {
            conn.close(VarInt::from_u32(1), CHIUSA_NON_ASSOCIATO);
            return Ok(());
        }
        // Tenuta da qui: una revoca che arriva mentre si aspetta il flusso, o
        // quando è già in mano a «Ascolta insieme», la chiude.
        condiviso.tieni_finche_vive(id, &conn);
        // Chi apre parla per primo (la stretta Noise), quindi il flusso arriva
        // con i suoi primi byte; uno che non arriva in tempo è una connessione
        // che non serve.
        let Ok(Ok((invio, ricevi))) =
            tokio::time::timeout(ATTESA_CONNESSIONE, conn.accept_bi()).await
        else {
            conn.close(VarInt::from_u32(0), b"");
            return Ok(());
        };
        // Di nuovo: la revoca può essere arrivata fra il primo sguardo e
        // `tieni`, e allora non l'avrebbe chiusa.
        if !condiviso.associato(id) {
            conn.close(VarInt::from_u32(1), CHIUSA_NON_ASSOCIATO);
            return Ok(());
        }
        let flusso = Flusso::nuovo(
            conn,
            invio,
            ricevi,
            tokio::runtime::Handle::current(),
            Arc::clone(&condiviso.vivo),
        );
        let ospite = Arc::clone(&condiviso.ospite);
        let _ = tokio::task::spawn_blocking(move || ospite.insieme(&da, flusso)).await;
        Ok(())
    }
}

/// Il risolutore DNS del nodo: **solo** i server del sistema.
///
/// Di serie iroh, quando quelli del sistema non rispondono (la rete di un
/// albergo, una VPN che filtra, l'avvio prima che la rete sia pronta), rifà
/// le stesse domande ai DNS pubblici di Cloudflare, Google e Quad9: i nomi
/// dei relay e `_iroh.<chiave del dispositivo collegato>.dns.iroh.link`, cioè
/// l'identificativo stabile di un dispositivo dell'utente, a tre società che
/// `PRIVACY.md` non nomina. Senza ripiego, una rete senza DNS è una rete
/// senza relay: si passa diritti, o si aspetta che il DNS torni.
///
/// Su Android i server del sistema iroh li legge da `LinkProperties` della
/// rete attiva (JNI, col contesto che tao inizializza all'avvio
/// dell'attività), e li rilegge a ogni cambio di rete.
fn risolutore() -> DnsResolver {
    DnsResolver::builder()
        .with_system_defaults()
        .disable_fallback()
        .build()
}

/// Il rapporto di rete del nodo, senza il controllo del portale: è una
/// `GET http://<relay>/generate_204` **in chiaro**, e `PRIVACY.md` promette
/// che dal collegamento non esce niente in chiaro. Le sonde HTTPS restano:
/// su una rete che chiude QUIC sono il solo modo di trovare il relay di casa.
fn rapporto_di_rete() -> NetReportConfig {
    let mut rapporto = NetReportConfig::default();
    rapporto.captive_portal_check = false;
    rapporto
}

/// Esadecimale minuscolo.
fn esadecimale(byte: &[u8]) -> String {
    use std::fmt::Write as _;
    byte.iter()
        .fold(String::with_capacity(byte.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Il contrario di [`esadecimale`], per una lunghezza esatta.
fn da_esadecimale<const N: usize>(testo: &str) -> Option<[u8; N]> {
    if testo.len() != N * 2 {
        return None;
    }
    let mut byte = [0_u8; N];
    for (i, uno) in byte.iter_mut().enumerate() {
        *uno = u8::from_str_radix(testo.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(byte)
}

/// Due gettoni uguali, guardando sempre tutti i byte: il tempo non dice fino
/// a dove erano giusti.
fn uguali_a_tempo_costante(uno: &[u8], altro: &[u8]) -> bool {
    uno.len() == altro.len()
        && uno
            .iter()
            .zip(altro)
            .fold(0_u8, |differenze, (a, b)| differenze | (a ^ b))
            == 0
}

#[cfg(test)]
mod prove_connessioni;

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn esadecimale_avanti_e_indietro() {
        let byte = [0_u8, 1, 0xab, 0xff];
        assert_eq!(esadecimale(&byte), "0001abff");
        assert_eq!(da_esadecimale::<4>("0001abff"), Some(byte));
        assert_eq!(da_esadecimale::<4>("0001abf"), None);
        assert_eq!(da_esadecimale::<4>("zz01abff"), None);
        assert!(uguali_a_tempo_costante(&byte, &byte));
        assert!(!uguali_a_tempo_costante(&byte, &[0, 1, 0xab, 0xfe]));
        assert!(!uguali_a_tempo_costante(&byte, &[0, 1]));
    }

    #[test]
    fn il_dns_e_solo_quello_del_sistema() {
        // Il `Debug` del risolutore porta la sua configurazione (nessuna rete:
        // il sistema si legge alla prima domanda): i server di ripiego ci
        // sarebbero scritti.
        let scritto = format!("{:?}", risolutore());
        assert!(scritto.contains("use_system_config: true"), "{scritto}");
        for pubblico in [
            "1.1.1.1",
            "1.0.0.1",
            "8.8.8.8",
            "8.8.4.4",
            "9.9.9.9",
            "149.112.112.112",
        ] {
            assert!(!scritto.contains(pubblico), "{pubblico} in {scritto}");
        }
    }

    #[test]
    fn niente_controllo_del_portale_in_chiaro() {
        let rapporto = rapporto_di_rete();
        assert!(
            !rapporto.captive_portal_check,
            "GET http://<relay>/generate_204"
        );
        assert!(
            rapporto.https_probes,
            "le sonde HTTPS restano: su una rete che chiude QUIC trovano il relay"
        );
    }

    #[test]
    fn una_chiave_custodita_e_la_stessa() {
        let identita = Identita::nuova();
        let ritrovata = Identita::da_byte(&identita.byte());
        assert_eq!(identita.chiave(), ritrovata.chiave());
        assert!(!format!("{identita:?}").contains(&identita.chiave()));
    }
}
