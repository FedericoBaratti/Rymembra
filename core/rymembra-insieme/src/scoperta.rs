//! La scoperta: chi ospita, sulla rete locale.
//!
//! # I due datagrammi
//!
//! Sono l'unica cosa che viaggia in chiaro, e per questo dicono il meno
//! possibile:
//!
//! - la **domanda** (40 byte): `RYMI`, versione e tipo (due `u16`), un nonce di
//!   16 byte, 16 byte di riempimento;
//! - la **risposta** (34 byte): `RYMI`, versione e tipo, la porta TCP, i primi
//!   8 byte dell'impronta della chiave, l'eco del nonce.
//!
//! Niente nomi, titoli o indirizzi: chi ascolta la rete vede che un PC ospita
//! e niente di più. Il riempimento rende la domanda più lunga della risposta:
//! con un mittente falso, chi risponde non può essere usato per mandare a un
//! terzo più byte di quelli che ha ricevuto.
//!
//! L'eco del nonce lega la risposta a una domanda nostra recente: una risposta
//! con un'eco che non lo è si butta. Protegge da poco, e va detto con
//! precisione: da chi non vede le nostre domande (non è sulla rete, o non
//! riceve il broadcast) e da risposte vecchie di un giro passato. **Non**
//! autentica chi risponde: la domanda viaggia in broadcast e in chiaro, e
//! chiunque sulla stessa rete la riceve e può rispondere con l'eco giusta,
//! una porta qualunque e l'impronta che vuole, anche quella di un host
//! associato. Chi sia davvero lo dimostra solo la stretta: per gli host
//! trovati si usa `Rete::collega_atteso`, che chiude se la chiave non ha
//! l'impronta annunciata.
//!
//! Per lo stesso motivo una risposta non vale più di una volta per giro: lo
//! stesso host (indirizzo, porta, impronta) dà un solo `Evento::Trovato` per
//! domanda, e al più [`TROVATI_PER_GIRO`] host diversi; il resto si butta. Chi
//! risponde mille volte, o con mille impronte, non riempie il canale degli
//! eventi del desktop.
//!
//! # Chi risponde, e quanto
//!
//! Solo chi ospita ha il socket aperto ([`rispondi`]). Al più 4 risposte al
//! secondo allo stesso indirizzo e 50 in tutto ([`Limitatore`]): una
//! scoperta onesta ne chiede una per giro ogni dieci secondi.
//!
//! # Dove si chiede
//!
//! In broadcast diretto su ogni sottorete IPv4 di ogni scheda, più
//! `255.255.255.255`, più gli indirizzi scritti a mano in unicast. Il broadcast
//! diretto per scheda c'è perché su un PC con più schede (questo ne ha sei)
//! `255.255.255.255` esce da una sola, quella che sceglie la tabella
//! d'instradamento, e la rete giusta può essere un'altra.
//!
//! # Quando la scoperta non basta
//!
//! Una rete che non lascia passare il broadcast (il Wi-Fi degli ospiti, una
//! rete d'ufficio divisa in sottoreti) non fa trovare nessuno: chi si collega
//! scrive l'indirizzo a mano. [`indirizzi_di_qui`] dice a chi ospita quale
//! dettargli.

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use if_addrs::{IfAddr, IfOperStatus};

use crate::condiviso::{Condiviso, RESPIRO};
use crate::errori::chiave;
use crate::identita::Impronta;
use crate::rete::Evento;

/// La firma dei due datagrammi.
const MAGIA: [u8; 4] = *b"RYMI";

/// La versione **del formato dei datagrammi**, separata da quella del
/// protocollo: un PC di un'altra versione del protocollo deve farsi trovare
/// lo stesso, così chi lo trova può dirgli «aggiorna» (`insieme.versione`)
/// invece di non vederlo affatto.
const VERSIONE_SCOPERTA: u16 = 1;

/// Il tipo della domanda.
const DOMANDA: u16 = 1;

/// Il tipo della risposta.
const RISPOSTA: u16 = 2;

/// Quanto è lunga la domanda.
pub(crate) const LUNGHEZZA_DOMANDA: usize = 40;

/// Quanto è lunga la risposta.
pub(crate) const LUNGHEZZA_RISPOSTA: usize = 34;

/// Quante porte si provano in broadcast, a partire dalla prima: quelle che
/// l'ascolto prova prima di arrendersi alla porta zero.
const PORTE: u16 = 10;

/// Quanto vale il nonce di una domanda: una risposta più vecchia è di un giro
/// passato, e si butta.
const NONCE_VALIDO: Duration = Duration::from_secs(15);

/// Quanti host diversi si annunciano al desktop per ogni giro di scoperta, al
/// più. Il desktop ne collega sedici; quattro volte tanti lasciano posto a una
/// rete affollata.
pub(crate) const TROVATI_PER_GIRO: usize = 64;

/// Un giro di scoperta: la domanda fatta, e gli host già annunciati.
#[derive(Debug)]
struct Giro {
    quando: Instant,
    nonce: [u8; 16],
    visti: HashSet<(SocketAddr, Impronta)>,
}

/// Quanto vive il socket della ricerca dopo l'ultima `cerca`: il desktop
/// cerca ogni dieci secondi mentre il pannello è aperto, e smette quando
/// entra in una sessione.
const CERCA_VIVO: Duration = Duration::from_secs(30);

/// La domanda.
pub(crate) fn domanda(nonce: &[u8; 16]) -> [u8; LUNGHEZZA_DOMANDA] {
    let mut fuori = [0_u8; LUNGHEZZA_DOMANDA];
    let pezzi: [&[u8]; 4] = [
        &MAGIA,
        &VERSIONE_SCOPERTA.to_be_bytes(),
        &DOMANDA.to_be_bytes(),
        nonce,
    ];
    componi(&mut fuori, &pezzi);
    fuori
}

/// Il nonce di una domanda, se è una domanda di questo formato.
pub(crate) fn leggi_domanda(datagramma: &[u8]) -> Option<[u8; 16]> {
    if datagramma.len() != LUNGHEZZA_DOMANDA {
        return None;
    }
    let resto = testa(datagramma, DOMANDA)?;
    let (nonce, _) = resto.split_first_chunk::<16>()?;
    Some(*nonce)
}

/// La risposta.
pub(crate) fn risposta(
    porta: u16,
    impronta: &Impronta,
    nonce: &[u8; 16],
) -> [u8; LUNGHEZZA_RISPOSTA] {
    let mut fuori = [0_u8; LUNGHEZZA_RISPOSTA];
    let pezzi: [&[u8]; 6] = [
        &MAGIA,
        &VERSIONE_SCOPERTA.to_be_bytes(),
        &RISPOSTA.to_be_bytes(),
        &porta.to_be_bytes(),
        &impronta.0,
        nonce,
    ];
    componi(&mut fuori, &pezzi);
    fuori
}

/// Porta, impronta ed eco di una risposta, se è una risposta di questo
/// formato.
pub(crate) fn leggi_risposta(datagramma: &[u8]) -> Option<(u16, Impronta, [u8; 16])> {
    if datagramma.len() != LUNGHEZZA_RISPOSTA {
        return None;
    }
    let resto = testa(datagramma, RISPOSTA)?;
    let (porta, resto) = resto.split_first_chunk::<2>()?;
    let (impronta, resto) = resto.split_first_chunk::<8>()?;
    let (eco, _) = resto.split_first_chunk::<16>()?;
    Some((u16::from_be_bytes(*porta), Impronta(*impronta), *eco))
}

/// Controlla firma, versione e tipo, e restituisce il resto.
fn testa(datagramma: &[u8], tipo: u16) -> Option<&[u8]> {
    let (magia, resto) = datagramma.split_first_chunk::<4>()?;
    let (versione, resto) = resto.split_first_chunk::<2>()?;
    let (suo_tipo, resto) = resto.split_first_chunk::<2>()?;
    (*magia == MAGIA
        && u16::from_be_bytes(*versione) == VERSIONE_SCOPERTA
        && u16::from_be_bytes(*suo_tipo) == tipo)
        .then_some(resto)
}

/// Copia i pezzi uno dopo l'altro; quel che avanza resta a zero.
fn componi(fuori: &mut [u8], pezzi: &[&[u8]]) {
    let mut dove = 0_usize;
    for pezzo in pezzi {
        let fine = dove.saturating_add(pezzo.len());
        if let Some(posto) = fuori.get_mut(dove..fine) {
            posto.copy_from_slice(pezzo);
        }
        dove = fine;
    }
}

/// Le risposte concesse: per indirizzo e in tutto, a finestre di un secondo.
#[derive(Debug, Default)]
pub(crate) struct Limitatore {
    per_ip: HashMap<IpAddr, (Instant, u32)>,
    totale: Option<(Instant, u32)>,
}

/// Risposte al secondo allo stesso indirizzo.
const PER_IP: u32 = 4;

/// Risposte al secondo in tutto.
const IN_TUTTO: u32 = 50;

/// Quanti indirizzi si ricordano, al più: oltre, chi è nuovo aspetta che si
/// liberi un posto. Un tetto alla memoria di chi riceve datagrammi da
/// indirizzi falsi, che costano niente a chi li manda.
const IP_RICORDATI: usize = 1024;

impl Limitatore {
    /// Si può rispondere a questo indirizzo adesso? Se sì, la risposta conta.
    pub(crate) fn concedi(&mut self, ip: IpAddr, adesso: Instant) -> bool {
        let finestra = Duration::from_secs(1);
        let in_finestra = |(dal, quante): (Instant, u32)| {
            if adesso.saturating_duration_since(dal) < finestra {
                (dal, quante)
            } else {
                (adesso, 0)
            }
        };
        let totale = in_finestra(self.totale.unwrap_or((adesso, 0)));
        if totale.1 >= IN_TUTTO {
            self.totale = Some(totale);
            return false;
        }
        if !self.per_ip.contains_key(&ip) && self.per_ip.len() >= IP_RICORDATI {
            self.per_ip
                .retain(|_, (dal, _)| adesso.saturating_duration_since(*dal) < finestra);
            if self.per_ip.len() >= IP_RICORDATI {
                return false;
            }
        }
        let suo = in_finestra(self.per_ip.get(&ip).copied().unwrap_or((adesso, 0)));
        if suo.1 >= PER_IP {
            self.per_ip.insert(ip, suo);
            return false;
        }
        self.per_ip.insert(ip, (suo.0, suo.1.saturating_add(1)));
        self.totale = Some((totale.0, totale.1.saturating_add(1)));
        true
    }
}

/// Gli indirizzi di broadcast delle schede di questo PC.
///
/// Con `lega` non specificato (`0.0.0.0`, l'applicazione) sono tutte le
/// sottoreti più `255.255.255.255`; con un indirizzo preciso solo la sottorete
/// della scheda che ce l'ha, e con `127.0.0.1` (le prove, due istanze sulla
/// stessa macchina) nessuna: lì valgono solo gli indirizzi a mano, e una prova
/// non deve mandare pacchetti sulla rete vera.
pub(crate) fn destinazioni(lega: IpAddr) -> Vec<Ipv4Addr> {
    let mut fuori = Vec::new();
    if lega.is_loopback() || lega.is_ipv6() {
        return fuori;
    }
    let schede = if_addrs::get_if_addrs().unwrap_or_default();
    for scheda in schede {
        if matches!(
            scheda.oper_status,
            IfOperStatus::Down | IfOperStatus::NotPresent
        ) || scheda.is_p2p()
        {
            continue;
        }
        let IfAddr::V4(v4) = scheda.addr else {
            continue;
        };
        // Loopback e link-local (169.254, la rete che Windows si inventa
        // quando il DHCP non risponde) non portano a nessun PC.
        if v4.ip.is_loopback() || v4.ip.is_link_local() || v4.prefixlen >= 31 {
            continue;
        }
        if !lega.is_unspecified() && lega != IpAddr::V4(v4.ip) {
            continue;
        }
        let broadcast = v4
            .broadcast
            .unwrap_or_else(|| Ipv4Addr::from(u32::from(v4.ip) | !u32::from(v4.netmask)));
        if !fuori.contains(&broadcast) {
            fuori.push(broadcast);
        }
    }
    if lega.is_unspecified() {
        fuori.push(Ipv4Addr::BROADCAST);
    }
    fuori
}

/// Gli indirizzi IPv4 di questo PC che un collega può scrivere a mano.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IndirizziDiQui {
    /// Quello da dettare per primo: l'indirizzo della scheda da cui passa la
    /// strada di serie, cioè quella con cui questo PC sta sulla rete di casa o
    /// dell'ufficio. `None` se questo PC non ha nessun indirizzo da dare.
    pub principale: Option<Ipv4Addr>,
    /// Gli altri, senza [`IndirizziDiQui::principale`] e senza doppioni: prima
    /// quelli delle schede vere, poi quelli delle schede virtuali (macchine
    /// virtuali, WSL, VPN), che un collega sulla stessa rete di solito non
    /// raggiunge.
    pub altri: Vec<Ipv4Addr>,
}

/// I nomi (in minuscolo, come pezzi del nome) delle schede che non sono una
/// rete dove sta un collega: le reti interne delle macchine virtuali, di WSL e
/// dei contenitori, le VPN, il Bluetooth. Vanno in fondo, non via: una VPN
/// d'ufficio può essere proprio la strada giusta, e chi ospita lo sa.
const SCHEDE_VIRTUALI: [&str; 12] = [
    "virtualbox",
    "vbox",
    "vmware",
    "vethernet",
    "hyper-v",
    "wsl",
    "docker",
    "loopback",
    "bluetooth",
    "tailscale",
    "zerotier",
    "vpn",
];

/// Gli indirizzi di questo PC che un collega può scrivere a mano per
/// collegarsi, quando la scoperta non lo trova: una rete che non lascia
/// passare il broadcast (il Wi-Fi degli ospiti, un ufficio diviso in
/// sottoreti).
///
/// Con `lega` preciso (`127.0.0.1` nelle prove, o l'indirizzo scelto per il
/// debug) è quello e basta, senza guardare le schede: è l'unico su cui questo
/// PC ascolta. Con `lega` non specificato sono gli indirizzi IPv4 delle schede
/// accese, senza loopback, link-local (169.254, la rete che Windows si
/// inventa quando il DHCP non risponde) e collegamenti punto a punto.
///
/// Non manda niente e non apre porte in ascolto. Il principale si trova con
/// un socket UDP in uscita «collegato» a un indirizzo di documentazione: su
/// UDP `connect` chiede solo al sistema da che scheda uscirebbe, e nessun
/// pacchetto parte (vedi `strada_di_serie`). Se qualcosa non risponde,
/// restituisce quel che sa — al peggio niente.
#[must_use]
pub fn indirizzi_di_qui(lega: IpAddr) -> IndirizziDiQui {
    if !lega.is_unspecified() {
        return IndirizziDiQui {
            principale: match lega {
                IpAddr::V4(v4) => Some(v4),
                IpAddr::V6(_) => None,
            },
            altri: Vec::new(),
        };
    }
    let schede = if_addrs::get_if_addrs().unwrap_or_default();
    let v4: Vec<(&str, Ipv4Addr, bool, bool)> = schede
        .iter()
        .filter_map(|scheda| match &scheda.addr {
            IfAddr::V4(v4) => Some((
                scheda.name.as_str(),
                v4.ip,
                scheda.is_oper_up(),
                scheda.is_p2p(),
            )),
            IfAddr::V6(_) => None,
        })
        .collect();
    ordina_indirizzi(&v4, strada_di_serie())
}

/// La parte pura di [`indirizzi_di_qui`]: dalle schede — nome, indirizzo,
/// accesa, punto a punto — e dall'indirizzo della strada di serie, se lo si
/// conosce, gli indirizzi da mostrare.
///
/// Il principale è quello della strada di serie se è fra quelli tenuti; se no
/// (nessuna strada, o una che passa da una scheda scartata) il primo di una
/// scheda vera, e in mancanza di quelle il primo di tutti.
fn ordina_indirizzi(
    schede: &[(&str, Ipv4Addr, bool, bool)],
    strada: Option<Ipv4Addr>,
) -> IndirizziDiQui {
    let mut vere: Vec<Ipv4Addr> = Vec::new();
    let mut virtuali: Vec<Ipv4Addr> = Vec::new();
    for (nome, ip, su, p2p) in schede {
        if !su || *p2p || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() {
            continue;
        }
        if vere.contains(ip) || virtuali.contains(ip) {
            continue;
        }
        let nome = nome.to_lowercase();
        if SCHEDE_VIRTUALI.iter().any(|pezzo| nome.contains(pezzo)) {
            virtuali.push(*ip);
        } else {
            vere.push(*ip);
        }
    }
    let principale = strada
        .filter(|ip| vere.contains(ip) || virtuali.contains(ip))
        .or_else(|| vere.first().copied())
        .or_else(|| virtuali.first().copied());
    let altri = vere
        .into_iter()
        .chain(virtuali)
        .filter(|ip| Some(*ip) != principale)
        .collect();
    IndirizziDiQui { principale, altri }
}

/// L'indirizzo di questo PC sulla strada di serie: quello da cui uscirebbe un
/// pacchetto per Internet, cioè, su un PC con più schede, quello della rete
/// vera e non di una macchina virtuale.
///
/// # Perché non manda niente
///
/// Un socket UDP «collegato» con `connect` non manda pacchetti: `connect` su
/// UDP fissa soltanto la destinazione di serie, e per farlo il sistema sceglie
/// nella tabella d'instradamento la scheda e l'indirizzo d'uscita — che è
/// proprio la domanda. Nessun datagramma parte, perché nessuno chiama `send`.
/// La destinazione è `192.0.2.1:9`: TEST-NET-1 (RFC 5737), indirizzi di
/// documentazione che nessuno instrada, e la porta del «discard».
///
/// E non apre porte in ascolto: il socket è in uscita, legato a una porta
/// qualunque come quello della ricerca (vedi `cercatore`), non riceve niente
/// e si chiude uscendo di qui. Il firewall di Windows chiede il permesso a chi
/// ascolta, e qui non ascolta nessuno.
fn strada_di_serie() -> Option<Ipv4Addr> {
    let presa = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    presa.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    match presa.local_addr().ok()?.ip() {
        IpAddr::V4(v4) if !v4.is_unspecified() => Some(v4),
        _ => None,
    }
}

/// Il socket di chi cerca, e le domande fatte di recente.
pub(crate) struct Cercatore {
    presa: UdpSocket,
    /// I giri recenti.
    giri: Mutex<VecDeque<Giro>>,
    /// L'ultima `cerca`.
    ultima: Mutex<Instant>,
    /// Il filo che riceve le risposte è vivo.
    vivo: AtomicBool,
}

impl std::fmt::Debug for Cercatore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cercatore").finish_non_exhaustive()
    }
}

impl Cercatore {
    /// Una risposta da annunciare? Sì se l'eco è di un giro recente, se in
    /// quel giro quell'host non si è già annunciato, e se il giro non ne ha
    /// già annunciati [`TROVATI_PER_GIRO`]. Se sì, se ne prende nota.
    fn da_annunciare(
        &self,
        eco: &[u8; 16],
        indirizzo: SocketAddr,
        impronta: Impronta,
        adesso: Instant,
    ) -> bool {
        let mut giri = chiave(&self.giri);
        let Some(giro) = giri.iter_mut().find(|giro| {
            giro.nonce == *eco && adesso.saturating_duration_since(giro.quando) < NONCE_VALIDO
        }) else {
            return false;
        };
        if giro.visti.len() >= TROVATI_PER_GIRO {
            return false;
        }
        giro.visti.insert((indirizzo, impronta))
    }
}

/// Un giro di scoperta. Vedi `Rete::cerca`.
pub(crate) fn cerca(rete: &Arc<Condiviso>, a_mano: &[SocketAddr]) {
    if rete.fermo() {
        return;
    }
    let Some(cercatore) = cercatore(rete) else {
        return;
    };
    let mut nonce = [0_u8; 16];
    if getrandom::fill(&mut nonce).is_err() {
        return;
    }
    let adesso = Instant::now();
    {
        let mut giri = chiave(&cercatore.giri);
        giri.push_back(Giro {
            quando: adesso,
            nonce,
            visti: HashSet::new(),
        });
        while giri
            .front()
            .is_some_and(|giro| adesso.saturating_duration_since(giro.quando) >= NONCE_VALIDO)
            || giri.len() > 8
        {
            giri.pop_front();
        }
    }
    *chiave(&cercatore.ultima) = adesso;
    let domanda = domanda(&nonce);
    // Gli errori di invio si ignorano uno per uno: una scheda senza rete non
    // deve impedire di cercare sulle altre.
    for broadcast in destinazioni(rete.lega) {
        for scarto in 0..PORTE {
            let Some(porta) = rete.porta.checked_add(scarto) else {
                break;
            };
            let _ = cercatore
                .presa
                .send_to(&domanda, SocketAddr::from((broadcast, porta)));
        }
    }
    for indirizzo in a_mano {
        let _ = cercatore.presa.send_to(&domanda, indirizzo);
    }
}

/// Il cercatore vivo, creandolo se serve.
fn cercatore(rete: &Arc<Condiviso>) -> Option<Arc<Cercatore>> {
    let mut posto = chiave(&rete.cercatore);
    if let Some(cercatore) = posto.as_ref()
        && cercatore.vivo.load(Ordering::SeqCst)
    {
        return Some(Arc::clone(cercatore));
    }
    // In uscita: legato a `lega:0`, una porta qualunque. Non è un ascolto, e
    // non fa comparire la richiesta del firewall (la sonda della fase 0).
    let presa = UdpSocket::bind(SocketAddr::new(rete.lega, 0)).ok()?;
    presa.set_broadcast(true).ok()?;
    presa.set_read_timeout(Some(RESPIRO)).ok()?;
    let cercatore = Arc::new(Cercatore {
        presa,
        giri: Mutex::new(VecDeque::new()),
        ultima: Mutex::new(Instant::now()),
        vivo: AtomicBool::new(true),
    });
    let per_il_filo = Arc::clone(&cercatore);
    if !rete.filo("cerca", move |rete| ricevi_risposte(&rete, &per_il_filo)) {
        return None;
    }
    *posto = Some(Arc::clone(&cercatore));
    Some(cercatore)
}

/// Il filo che riceve le risposte, finché si cerca.
fn ricevi_risposte(rete: &Condiviso, cercatore: &Cercatore) {
    let mia = rete.identita.pubblica().impronta();
    let mut buffer = [0_u8; 64];
    loop {
        if rete.fermo() || chiave(&cercatore.ultima).elapsed() > CERCA_VIVO {
            // Si esce sotto il lucchetto del posto, ricontrollando: una
            // `cerca` arrivata un istante prima avrà rinnovato `ultima`, e
            // uscire lo stesso vorrebbe dire una domanda partita senza nessuno
            // che ascolti le risposte.
            let mut posto = chiave(&rete.cercatore);
            if rete.fermo() || chiave(&cercatore.ultima).elapsed() > CERCA_VIVO {
                cercatore.vivo.store(false, Ordering::SeqCst);
                *posto = None;
                return;
            }
        }
        match cercatore.presa.recv_from(&mut buffer) {
            Ok((quanti, da)) => {
                let adesso = Instant::now();
                let Some((porta, impronta, eco)) = buffer.get(..quanti).and_then(leggi_risposta)
                else {
                    continue;
                };
                // La propria risposta (si ospita e si cerca insieme, e il
                // broadcast torna indietro) non è un PC trovato.
                let indirizzo = SocketAddr::new(da.ip(), porta);
                if porta == 0
                    || impronta == mia
                    || !cercatore.da_annunciare(&eco, indirizzo, impronta, adesso)
                {
                    continue;
                }
                rete.emetti(Evento::Trovato {
                    indirizzo,
                    impronta,
                });
            }
            // La scadenza del giro, o il reset che Windows riporta su UDP
            // quando una domanda a mano ha trovato una porta chiusa (ICMP
            // «porta irraggiungibile»): niente di cui preoccuparsi, si
            // continua. La pausa c'è per non girare a vuoto se un errore
            // tornasse subito per sempre.
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => std::thread::sleep(RESPIRO),
        }
    }
}

/// Chi ospita: risponde alle domande sul socket UDP della sua porta, finché
/// l'ascolto è quello di questa generazione.
pub(crate) fn rispondi(rete: &Condiviso, presa: &UdpSocket, porta: u16, generazione: u64) {
    let _ = presa.set_read_timeout(Some(RESPIRO));
    let impronta = rete.identita.pubblica().impronta();
    let mut limitatore = Limitatore::default();
    let mut buffer = [0_u8; 64];
    while !rete.fermo() && rete.generazione.load(Ordering::SeqCst) == generazione {
        match presa.recv_from(&mut buffer) {
            Ok((quanti, da)) => {
                let Some(nonce) = buffer.get(..quanti).and_then(leggi_domanda) else {
                    continue;
                };
                if limitatore.concedi(da.ip(), Instant::now()) {
                    let _ = presa.send_to(&risposta(porta, &impronta, &nonce), da);
                }
            }
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => std::thread::sleep(RESPIRO),
        }
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn i_datagrammi_hanno_la_misura_dichiarata_e_tornano_uguali() {
        let nonce = [7_u8; 16];
        let d = domanda(&nonce);
        assert_eq!(d.len(), 40);
        assert_eq!(leggi_domanda(&d), Some(nonce));
        let impronta = Impronta([1, 2, 3, 4, 5, 6, 7, 8]);
        let r = risposta(47_811, &impronta, &nonce);
        assert_eq!(r.len(), 34);
        assert_eq!(leggi_risposta(&r), Some((47_811, impronta, nonce)));
        assert!(
            r.len() < d.len(),
            "la risposta è più corta della domanda: niente amplificazione"
        );
    }

    #[test]
    fn i_datagrammi_storti_non_passano() {
        let nonce = [7_u8; 16];
        let d = domanda(&nonce);
        let r = risposta(1, &Impronta([0; 8]), &nonce);
        assert_eq!(leggi_domanda(&r), None, "una risposta non è una domanda");
        assert_eq!(leggi_risposta(&d), None, "né il contrario");
        assert_eq!(leggi_domanda(d.get(..39).expect("dentro")), None, "corta");
        let mut altra_magia = d;
        if let Some(primo) = altra_magia.first_mut() {
            *primo = b'X';
        }
        assert_eq!(leggi_domanda(&altra_magia), None);
        let mut altra_versione = r;
        if let Some(versione) = altra_versione.get_mut(5) {
            *versione = 9;
        }
        assert_eq!(leggi_risposta(&altra_versione), None);
    }

    #[test]
    fn una_risposta_con_l_eco_sbagliata_non_e_nostra_e_una_ripetuta_non_vale_due_volte() {
        let presa = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("socket");
        let cercatore = Cercatore {
            presa,
            giri: Mutex::new(VecDeque::from([Giro {
                quando: Instant::now(),
                nonce: [1_u8; 16],
                visti: HashSet::new(),
            }])),
            ultima: Mutex::new(Instant::now()),
            vivo: AtomicBool::new(true),
        };
        let adesso = Instant::now();
        let host = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 2), 47_811));
        let impronta = Impronta([3; 8]);
        assert!(
            !cercatore.da_annunciare(&[2; 16], host, impronta, adesso),
            "eco sbagliata"
        );
        assert!(
            !cercatore.da_annunciare(&[1; 16], host, impronta, adesso + NONCE_VALIDO),
            "eco di un giro vecchio"
        );
        assert!(cercatore.da_annunciare(&[1; 16], host, impronta, adesso));
        assert!(
            !cercatore.da_annunciare(&[1; 16], host, impronta, adesso),
            "lo stesso host, lo stesso giro"
        );
        // Mille impronte inventate: ne passano al più `TROVATI_PER_GIRO`.
        let passate = (0_u16..1_000)
            .filter(|i| {
                let [a, b] = i.to_be_bytes();
                cercatore.da_annunciare(&[1; 16], host, Impronta([a, b, 0, 0, 0, 0, 0, 1]), adesso)
            })
            .count();
        assert_eq!(passate, TROVATI_PER_GIRO - 1);
    }

    #[test]
    fn al_piu_quattro_risposte_al_secondo_per_indirizzo_e_cinquanta_in_tutto() {
        let zero = Instant::now();
        let mut limitatore = Limitatore::default();
        let uno = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let concesse = (0..10).filter(|_| limitatore.concedi(uno, zero)).count();
        assert_eq!(concesse, 4);
        assert!(
            limitatore.concedi(uno, zero + Duration::from_secs(1)),
            "un secondo dopo si ricomincia"
        );

        let mut limitatore = Limitatore::default();
        let concesse = (0_u8..100)
            .filter(|i| limitatore.concedi(IpAddr::V4(Ipv4Addr::new(10, 0, 1, *i)), zero))
            .count();
        assert_eq!(concesse, 50, "il tetto totale con cento indirizzi diversi");
    }

    #[test]
    fn con_127_0_0_1_non_si_manda_niente_in_broadcast() {
        assert!(destinazioni(IpAddr::V4(Ipv4Addr::LOCALHOST)).is_empty());
    }

    #[test]
    fn con_un_indirizzo_preciso_gli_indirizzi_di_qui_sono_quello_e_basta() {
        assert_eq!(
            indirizzi_di_qui(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            IndirizziDiQui {
                principale: Some(Ipv4Addr::LOCALHOST),
                altri: Vec::new(),
            },
            "le schede vere non si guardano"
        );
        assert_eq!(
            indirizzi_di_qui(IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)),
            IndirizziDiQui::default(),
            "un indirizzo IPv6 preciso non dà un IPv4 da scrivere"
        );
    }

    /// Le schede di un PC d'ufficio inventato: due vere, quattro virtuali,
    /// e quelle da scartare.
    fn schede() -> Vec<(&'static str, Ipv4Addr, bool, bool)> {
        vec![
            ("vEthernet (WSL)", Ipv4Addr::new(172, 20, 0, 1), true, false),
            ("Ethernet", Ipv4Addr::new(10, 88, 2, 73), true, false),
            (
                "Loopback Pseudo-Interface 1",
                Ipv4Addr::LOCALHOST,
                true,
                false,
            ),
            ("Wi-Fi", Ipv4Addr::new(169, 254, 3, 4), true, false),
            ("Ethernet 2", Ipv4Addr::new(192, 168, 56, 9), false, false),
            ("VPN d'ufficio", Ipv4Addr::new(10, 200, 0, 5), true, true),
            (
                "VirtualBox Host-Only Network",
                Ipv4Addr::new(192, 168, 56, 1),
                true,
                false,
            ),
            ("Wi-Fi 2", Ipv4Addr::new(192, 168, 1, 20), true, false),
            ("Wi-Fi 3", Ipv4Addr::new(192, 168, 1, 20), true, false),
            ("Tailscale", Ipv4Addr::new(100, 64, 0, 7), true, false),
            ("Docker NAT", Ipv4Addr::UNSPECIFIED, true, false),
        ]
    }

    #[test]
    fn gli_indirizzi_di_qui_tengono_le_schede_accese_e_mettono_in_fondo_le_virtuali() {
        let strada = Some(Ipv4Addr::new(192, 168, 1, 20));
        assert_eq!(
            ordina_indirizzi(&schede(), strada),
            IndirizziDiQui {
                principale: strada,
                altri: vec![
                    Ipv4Addr::new(10, 88, 2, 73),
                    Ipv4Addr::new(172, 20, 0, 1),
                    Ipv4Addr::new(192, 168, 56, 1),
                    Ipv4Addr::new(100, 64, 0, 7),
                ],
            },
            "via loopback, link-local, spente, punto a punto e il doppione"
        );
    }

    #[test]
    fn senza_una_strada_buona_il_principale_e_la_prima_scheda_vera() {
        let atteso = IndirizziDiQui {
            principale: Some(Ipv4Addr::new(10, 88, 2, 73)),
            altri: vec![
                Ipv4Addr::new(192, 168, 1, 20),
                Ipv4Addr::new(172, 20, 0, 1),
                Ipv4Addr::new(192, 168, 56, 1),
                Ipv4Addr::new(100, 64, 0, 7),
            ],
        };
        assert_eq!(ordina_indirizzi(&schede(), None), atteso, "nessuna strada");
        assert_eq!(
            ordina_indirizzi(&schede(), Some(Ipv4Addr::new(10, 200, 0, 5))),
            atteso,
            "una strada che passa da una scheda scartata"
        );
        // La strada di serie su una scheda virtuale vale lo stesso: è quella
        // da cui questo PC esce davvero.
        let virtuale = ordina_indirizzi(&schede(), Some(Ipv4Addr::new(100, 64, 0, 7)));
        assert_eq!(virtuale.principale, Some(Ipv4Addr::new(100, 64, 0, 7)));
        assert_eq!(virtuale.altri.len(), 4);
        // Solo schede virtuali: la prima, piuttosto che niente.
        let solo_virtuali = [(
            "vEthernet (Default Switch)",
            Ipv4Addr::new(172, 30, 0, 1),
            true,
            false,
        )];
        assert_eq!(
            ordina_indirizzi(&solo_virtuali, None).principale,
            Some(Ipv4Addr::new(172, 30, 0, 1))
        );
        assert_eq!(ordina_indirizzi(&[], None), IndirizziDiQui::default());
    }
}
