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
//!   chiusa subito, col motivo `lontano.nonAssociato`.
//! - [`ALPN_ASSOCIA`]: chiuso se non c'è un QR attivo; al più
//!   [`TENTATIVI_AL_MINUTO`]; il gettone vale [`VALIDITA_INVITO`], una volta
//!   sola, e dopo [`MASSIMI_FALLIMENTI`] gettoni sbagliati il QR non vale più.
//!   Il gettone si confronta a tempo costante. Un gettone giusto non basta:
//!   decide chi mostra il QR ([`Ospite::conferma`]).

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::str::FromStr as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iroh::endpoint::{
    Connection, ConnectionError, PortmapperConfig, RecvStream, SendStream, VarInt, presets,
};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, RelayUrl, SecretKey};
use rymembra_domain::errors::{AppError, Domain, ErrorCode, ErrorCodeKind};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::sync::{Notify, Semaphore};
use tokio::task::AbortHandle;

use crate::collegamento::{Invito, MASSIMO_INDIRIZZI};
use crate::errori::{chiave, da_testo, dal_rifiuto, errore, inatteso, irraggiungibile, protocollo};
use crate::flusso::Flusso;
use crate::protocollo::{
    ALPN_ASSOCIA, ALPN_INSIEME, ALPN_LONTANO, Associa, Domanda, MASSIMO_PEZZO, Presentazione,
    Presenza, Risposta, Testa, cornice, lunghezza, valore,
};

/// Quanto vale un QR.
pub const VALIDITA_INVITO: Duration = Duration::from_secs(10 * 60);

/// Gettoni sbagliati prima che il QR non valga più.
pub const MASSIMI_FALLIMENTI: u32 = 5;

/// Tentativi di associazione al minuto, da chiunque.
pub const TENTATIVI_AL_MINUTO: usize = 10;

/// Quanto si aspetta chi decide, sul dispositivo che mostra il QR: come il
/// consenso di un servizio cloud.
pub const ATTESA_CONFERMA: Duration = Duration::from_secs(3 * 60);

/// Quanto si prova a raggiungere un dispositivo prima di dire che non
/// risponde: il primo contatto passa spesso dal relay, poi si buca il NAT.
pub const ATTESA_CONNESSIONE: Duration = Duration::from_secs(15);

/// Domande servite insieme per ogni dispositivo.
pub const DOMANDE_INSIEME: usize = 8;

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
    /// Il QR mostrato non vale più: è stato usato, o i gettoni sbagliati
    /// l'hanno bruciato, o è scaduto. Chi lo mostra lo toglie dallo schermo —
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
/// bloccante del runtime, mai da uno della rete.
pub trait Ospite: Send + Sync + 'static {
    /// Questa chiave è di un dispositivo associato, e non revocato?
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
    fallimenti: u32,
}

/// Quel che i compiti del nodo condividono.
struct Condiviso {
    ospite: Arc<dyn Ospite>,
    io: Presentazione,
    presenza: Mutex<Presenza>,
    invito: Mutex<Option<InvitoAttivo>>,
    tentativi: Mutex<VecDeque<Instant>>,
    connessioni: Mutex<HashMap<EndpointId, Connection>>,
    /// Un turno per dispositivo per aprire una connessione: dieci domande
    /// insieme a un dispositivo senza connessione ne aprono una, non dieci.
    turni: Mutex<HashMap<EndpointId, Arc<tokio::sync::Mutex<()>>>>,
    noti: Mutex<HashMap<EndpointId, Vec<SocketAddr>>>,
    /// Sveglia i compiti della presenza: un cambio di rete, un dispositivo
    /// che si è collegato da sé.
    sveglia: Notify,
    /// Annulla l'associazione in corso da questo lato ([`Nodo::collega`]).
    annullo: Notify,
    /// Il runtime gira: i [`Flusso`] lo guardano prima di ogni `block_on`, e
    /// dopo lo spegnimento rispondono con un errore invece di usare un runtime
    /// che non c'è più.
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

    fn indirizzo_di(&self, pari: EndpointId) -> EndpointAddr {
        let mut indirizzo = EndpointAddr::new(pari);
        for diretto in chiave(&self.noti).get(&pari).into_iter().flatten() {
            indirizzo = indirizzo.with_ip_addr(*diretto);
        }
        indirizzo
    }

    /// Un tentativo di associazione in più: `false` oltre il tetto.
    fn tentativo_ammesso(&self) -> bool {
        let mut tentativi = chiave(&self.tentativi);
        let adesso = Instant::now();
        while tentativi
            .front()
            .is_some_and(|quando| adesso.duration_since(*quando) > Duration::from_secs(60))
        {
            tentativi.pop_front();
        }
        if tentativi.len() >= TENTATIVI_AL_MINUTO {
            return false;
        }
        tentativi.push_back(adesso);
        true
    }

    fn invito_attivo(&self) -> bool {
        chiave(&self.invito)
            .as_ref()
            .is_some_and(|invito| invito.scade > Instant::now())
    }

    /// Il gettone presentato. Giusto, il QR non vale più (una volta sola);
    /// sbagliato, conta un fallimento. L'errore è il codice per l'altro.
    ///
    /// Un QR che smette di valere qui — usato, bruciato, trovato scaduto — si
    /// dice con [`Evento::InvitoFinito`], fuori dal lucchetto.
    fn consuma(&self, presentato: &str) -> Result<(), &'static str> {
        let (esito, finito) = {
            let mut invito = chiave(&self.invito);
            match invito.as_mut().filter(|i| i.scade > Instant::now()) {
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
                        attivo.fallimenti = attivo.fallimenti.saturating_add(1);
                        let bruciato = attivo.fallimenti >= MASSIMI_FALLIMENTI;
                        if bruciato {
                            *invito = None;
                        }
                        (Err("lontano.gettoneScaduto"), bruciato)
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
            tentativi: Mutex::new(VecDeque::new()),
            connessioni: Mutex::new(HashMap::new()),
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
                    let mut costruttore = Endpoint::builder(presets::N0);
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

    fn runtime(&self) -> Result<&tokio::runtime::Runtime, AppError> {
        self.runtime
            .as_ref()
            .ok_or_else(|| errore(ErrorCodeKind::LontanoSpento))
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
        *chiave(&self.condiviso.invito) = Some(InvitoAttivo {
            gettone,
            scade: Instant::now() + VALIDITA_INVITO,
            fallimenti: 0,
        });
        Ok(Invito {
            chiave: *self.endpoint.id().as_bytes(),
            gettone,
            indirizzi,
            relay,
        })
    }

    /// Il QR mostrato non vale più.
    pub fn annulla_invito(&self) {
        *chiave(&self.condiviso.invito) = None;
    }

    /// C'è un QR che vale ancora: né usato, né bruciato, né scaduto.
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
        let accolto = runtime.block_on(async {
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
                    .map_err(irraggiungibile)?,
            };
            let esito = tokio::select! {
                () = &mut annullato => {
                    conn.close(VarInt::from_u32(1), CHIUSA_ANNULLATA);
                    return Err(annullato_qui());
                }
                esito = tokio::time::timeout(ATTESA_CONFERMA + Duration::from_secs(30), async {
                    let (mut invio, mut ricevi) =
                        conn.open_bi().await.map_err(|e| perso(&conn, e))?;
                    scrivi(&mut invio, &Associa::Chiedo { gettone, chi: io }).await?;
                    let _ = invio.finish();
                    leggi::<Associa>(&mut ricevi)
                        .await
                        .map_err(|err| chiusa_perche(&conn).unwrap_or(err))
                }) => esito.map_err(|_| irraggiungibile("nessuna risposta all'associazione"))?,
            };
            conn.close(VarInt::from_u32(0), b"fatto");
            esito
        })?;
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

    /// I dispositivi associati da seguire: chi non c'è più si lascia, chi è
    /// nuovo si comincia a cercare.
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
                if let Some(conn) = chiave(&self.condiviso.connessioni).remove(id) {
                    conn.close(VarInt::from_u32(1), CHIUSA_NON_ASSOCIATO);
                }
            }
            resta
        });
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
        runtime.block_on(async move {
            tokio::time::timeout(scadenza, async {
                let conn = connessione(&condiviso, &endpoint, id).await?;
                chiedi_su(&conn, domanda.nei_tetti()).await
            })
            .await
            .map_err(|_| irraggiungibile("la domanda è scaduta"))?
        })
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
        runtime.block_on(async move {
            tokio::time::timeout(entro, async {
                let conn = endpoint
                    .connect(indirizzo, ALPN_INSIEME)
                    .await
                    .map_err(irraggiungibile)?;
                let (invio, ricevi) = conn.open_bi().await.map_err(|err| perso(&conn, err))?;
                Ok(Flusso::nuovo(conn, invio, ricevi, maniglia, vivo))
            })
            .await
            .map_err(|_| irraggiungibile("il dispositivo non risponde"))?
        })
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

    fn spegni(&mut self) {
        let Some(runtime) = self.runtime.take() else {
            return;
        };
        for (_, compito) in chiave(&self.seguiti).drain() {
            compito.abort();
        }
        let router = self.router.clone();
        runtime.block_on(async move {
            let _ = tokio::time::timeout(Duration::from_secs(3), router.shutdown()).await;
        });
        // Dopo il router, che chiude le connessioni e sveglia chi aspetta: da
        // qui un flusso rimasto in mano a qualcuno non tocca più il runtime.
        self.condiviso.vivo.store(false, Ordering::Release);
        runtime.shutdown_timeout(Duration::from_secs(2));
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
    condiviso.registra(pari, &conn);
    let condiviso = Arc::clone(condiviso);
    let servita = conn.clone();
    tokio::spawn(async move {
        servi(&condiviso, &servita).await;
        condiviso.togli(pari, &servita);
    });
    Ok(conn)
}

/// Le domande che arrivano su una connessione, finché resta aperta.
async fn servi(condiviso: &Arc<Condiviso>, conn: &Connection) {
    let posti = Arc::new(Semaphore::new(DOMANDE_INSIEME));
    let da = conn.remote_id();
    while let Ok((invio, ricevi)) = conn.accept_bi().await {
        let Ok(posto) = Arc::clone(&posti).acquire_owned().await else {
            break;
        };
        let condiviso = Arc::clone(condiviso);
        let conn = conn.clone();
        tokio::spawn(async move {
            let _posto = posto;
            if let Err(guasto) = rispondi_a(&condiviso, &conn, da, invio, ricevi).await {
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
    conn: &Connection,
    da: EndpointId,
    mut invio: SendStream,
    mut ricevi: RecvStream,
) -> Result<(), AppError> {
    let domanda: Domanda = tokio::time::timeout(Duration::from_secs(30), leggi(&mut ricevi))
        .await
        .map_err(|_| protocollo("la domanda non arriva"))??;
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
        let _ = tokio::time::timeout(Duration::from_secs(2), invio.stopped()).await;
        conn.close(VarInt::from_u32(1), CHIUSA_NON_ASSOCIATO);
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
            )
            .await?;
            invio.write_all(&byte).await.map_err(irraggiungibile)?;
        }
        Ok(Risposta::Presenza(presenza)) => scrivi(invio, &Testa::Presenza(presenza)).await?,
        Ok(Risposta::Dati(dati)) => scrivi(invio, &Testa::Dati { dati }).await?,
        Ok(Risposta::Fatto) => scrivi(invio, &Testa::Fatto).await?,
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
            )
            .await?;
        }
    }
    invio.finish().map_err(irraggiungibile)
}

/// Una domanda su una connessione già aperta.
async fn chiedi_su(conn: &Connection, domanda: Domanda) -> Result<Risposta, AppError> {
    let (mut invio, mut ricevi) = conn.open_bi().await.map_err(|e| perso(conn, e))?;
    scrivi(&mut invio, &domanda)
        .await
        .map_err(|err| chiusa_perche(conn).unwrap_or(err))?;
    let _ = invio.finish();
    leggi_risposta(&mut ricevi)
        .await
        .map_err(|err| chiusa_perche(conn).unwrap_or(err))
}

/// Legge la risposta: la testa, e per dei byte i byte.
async fn leggi_risposta(ricevi: &mut RecvStream) -> Result<Risposta, AppError> {
    match leggi::<Testa>(ricevi).await? {
        Testa::Presenza(presenza) => Ok(Risposta::Presenza(presenza)),
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

/// Scrive una cornice su un flusso.
async fn scrivi(invio: &mut SendStream, valore: &impl Serialize) -> Result<(), AppError> {
    let byte = cornice(valore)?;
    invio.write_all(&byte).await.map_err(irraggiungibile)
}

/// Legge una cornice da un flusso.
async fn leggi<T: DeserializeOwned>(ricevi: &mut RecvStream) -> Result<T, AppError> {
    let mut testa = [0_u8; 4];
    ricevi
        .read_exact(&mut testa)
        .await
        .map_err(irraggiungibile)?;
    let (quanti, compressa) = lunghezza(testa)?;
    let mut corpo = vec![0_u8; quanti];
    ricevi
        .read_exact(&mut corpo)
        .await
        .map_err(irraggiungibile)?;
    valore(&corpo, compressa)
}

/// Il guasto di un flusso, detto col motivo della chiusura se l'altro ha
/// chiuso con un codice (non associato, QR scaduto).
fn perso(conn: &Connection, err: impl std::fmt::Display) -> AppError {
    chiusa_perche(conn).unwrap_or_else(|| irraggiungibile(err))
}

/// Il codice con cui l'altro ha chiuso la connessione, se ne ha dato uno.
fn chiusa_perche(conn: &Connection) -> Option<AppError> {
    match conn.close_reason()? {
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
        if !condiviso.tentativo_ammesso() || !condiviso.invito_attivo() {
            conn.close(VarInt::from_u32(1), CHIUSA_SENZA_INVITO);
            return Ok(());
        }
        let da = conn.remote_id();
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
    let domanda: Associa = tokio::time::timeout(Duration::from_secs(10), leggi(&mut ricevi))
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
    scrivi(&mut invio, &risposta).await?;
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
        let ospite = Arc::clone(&condiviso.ospite);
        let testo = da.to_string();
        let ammesso = tokio::task::spawn_blocking(move || ospite.associato(&testo))
            .await
            .unwrap_or(false);
        if !ammesso {
            conn.close(VarInt::from_u32(1), CHIUSA_NON_ASSOCIATO);
            return Ok(());
        }
        condiviso.registra(da, &conn);
        servi(condiviso, &conn).await;
        condiviso.togli(da, &conn);
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
        let da = conn.remote_id().to_string();
        let ospite = Arc::clone(&condiviso.ospite);
        let chi = da.clone();
        let ammesso = tokio::task::spawn_blocking(move || ospite.associato(&chi))
            .await
            .unwrap_or(false);
        if !ammesso {
            conn.close(VarInt::from_u32(1), CHIUSA_NON_ASSOCIATO);
            return Ok(());
        }
        // Chi apre parla per primo (la stretta Noise), quindi il flusso arriva
        // con i suoi primi byte; uno che non arriva in tempo è una connessione
        // che non serve.
        let Ok(Ok((invio, ricevi))) =
            tokio::time::timeout(ATTESA_CONNESSIONE, conn.accept_bi()).await
        else {
            conn.close(VarInt::from_u32(0), b"");
            return Ok(());
        };
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
    fn una_chiave_custodita_e_la_stessa() {
        let identita = Identita::nuova();
        let ritrovata = Identita::da_byte(&identita.byte());
        assert_eq!(identita.chiave(), ritrovata.chiave());
        assert!(!format!("{identita:?}").contains(&identita.chiave()));
    }
}
