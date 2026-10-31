//! Il canale cifrato: la stretta Noise `XX_25519_ChaChaPoly_SHA256`, poi i
//! messaggi del trasporto cifrati nei due sensi.
//!
//! # Perché `XX`
//!
//! Nessuno dei due PC conosce in anticipo la chiave dell'altro — al primo
//! contatto non la conosce nessuno, e anche dopo un host trovato con la
//! scoperta è solo un'impronta. `XX` è lo schema in cui le due chiavi statiche
//! viaggiano dentro la stretta, cifrate, e alla fine ciascuno sa quella
//! dell'altro e sa che l'altro la possiede davvero.
//!
//! # Lo stato senza stato
//!
//! Dopo la stretta si passa a `StatelessTransportState`, in cui il nonce di
//! ogni messaggio lo dice chi chiama, invece del `TransportState` che lo
//! conta da sé. Il motivo è che sulla stessa connessione lavorano due fili,
//! uno che legge e uno che scrive, e `TransportState` vuole `&mut self` per
//! tutti e due: messo dietro un `Mutex`, il filo che legge aspetterebbe quello
//! che scrive anche solo per decifrare un ping — e l'istante d'arrivo di un
//! ping è proprio quel che l'orologio misura. Con lo stato senza stato i due
//! condividono le chiavi (in sola lettura, dietro un `Arc`) e ognuno tiene il
//! **suo** contatore: [`Cifratore`] quello in uscita, [`Decifratore`] quello
//! in entrata. Il TCP consegna in ordine, quindi il contatore in entrata è
//! semplicemente il numero di messaggi letti.
//!
//! Un nonce non si riusa mai perché ciascun contatore ha un solo
//! proprietario e cresce solo: è la stessa garanzia di `TransportState`,
//! spostata dal tipo al disegno dei fili.
//!
//! # Le scadenze, e una trappola di Windows
//!
//! Ogni lettura e ogni scrittura su TCP ha una scadenza, e **una scadenza
//! chiude sempre la connessione**. Non è solo una scelta: su Windows dopo un
//! `SO_RCVTIMEO` scaduto lo stato del socket è «indeterminato» (lo dice la
//! documentazione di Winsock: la lettura annullata può aver già consumato dei
//! byte), e continuare a leggere dopo una scadenza vorrebbe dire, una volta
//! ogni tanto, perdere un pezzo di un messaggio cifrato. Per questo il filo
//! che legge non ha un «giro» con scadenze brevi: aspetta il prossimo
//! messaggio fino a [`crate::Tempi::silenzio`], e se non arriva chiude.
//!
//! Misurato su questo PC: uno `shutdown` da un altro filo **non** sveglia una
//! `recv` bloccata — torna solo quando l'altro capo chiude. Chi vuole chiudere
//! una connessione quindi lo dice all'altro capo (`Chiudo`, poi la metà in
//! scrittura), e l'altro, chiudendo a sua volta, sveglia il nostro filo.

use std::io::{ErrorKind, Read as _, Write as _};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use snow::{Builder, StatelessTransportState};

use crate::cornice::{self, ETICHETTA, MASSIMO_NOISE, Ricompositore, Tipo};
use crate::identita::{ChiavePubblica, Identita};

/// Lo schema Noise. Uno solo, scritto qui: è il nome che finisce nell'hash
/// della stretta, quindi due PC con due schemi diversi non si capiscono.
const SCHEMA: &str = "Noise_XX_25519_ChaChaPoly_SHA256";

/// Il prologo: entra nell'hash della stretta, e lega la stretta a questo
/// protocollo. Non contiene la versione, di proposito: due PC di versioni
/// diverse devono riuscire a finire la stretta per dirsi, nel saluto, che
/// sono di versioni diverse (`insieme.versione`), invece di cadere con un
/// errore di decifratura che non spiega niente.
const PROLOGO: &[u8] = b"Rymembra insieme";

/// Oltre questa capacità il buffer d'uscita si restringe dopo l'invio: una
/// connessione che ha mandato una lettura da due megabyte non deve tenerseli
/// per sempre.
const BUFFER_TENUTO: usize = 256 * 1024;

/// A quanto si restringe: abbastanza per i messaggi di controllo di tutti i
/// giorni, che non lo faranno ricrescere.
const BUFFER_RISTRETTO: usize = 64 * 1024;

/// Perché il canale si è interrotto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Guasto {
    /// Niente è arrivato (o partito) entro la scadenza.
    Silenzio,
    /// L'altro capo ha chiuso, o la rete è caduta.
    Rete,
    /// L'altro capo ha mandato qualcosa che non è il protocollo.
    Protocollo,
}

impl From<std::io::Error> for Guasto {
    fn from(err: std::io::Error) -> Self {
        match err.kind() {
            ErrorKind::TimedOut | ErrorKind::WouldBlock => Self::Silenzio,
            _ => Self::Rete,
        }
    }
}

/// Il canale dopo la stretta.
pub(crate) struct Canale {
    /// La chiave statica dell'altro capo.
    pub(crate) pari: ChiavePubblica,
    /// L'hash della stretta: entra nel codice di quattro cifre.
    pub(crate) hash: [u8; 32],
    /// Le chiavi di sessione, condivise fra chi legge e chi scrive.
    stato: Arc<StatelessTransportState>,
}

impl Canale {
    /// Divide il canale fra chi cifra e chi decifra, ciascuno col suo
    /// contatore a zero.
    ///
    /// Consuma il canale: due cifratori dello stesso canale ripartirebbero
    /// tutti e due dal nonce zero, e lo stesso nonce con la stessa chiave è
    /// l'unico errore che ChaChaPoly non perdona. Fino alla revisione del 26
    /// ottobre `cifratore()` prendeva `&self`, e niente impediva di chiamarlo
    /// due volte; adesso non si può scrivere.
    pub(crate) fn dividi(self) -> (Cifratore, Decifratore) {
        let cifratore = Cifratore {
            stato: Arc::clone(&self.stato),
            nonce: 0,
            uscita: Vec::new(),
        };
        let decifratore = Decifratore {
            stato: self.stato,
            nonce: 0,
            ricompositore: Ricompositore::default(),
            cifrato: vec![0; MASSIMO_NOISE],
            chiaro: vec![0; MASSIMO_NOISE],
        };
        (cifratore, decifratore)
    }
}

/// Il tetto del primo messaggio della stretta, letto prima di dare a chi
/// bussa un posto fra le strette (vedi `ascolto`).
///
/// Il primo messaggio di `XX` è la chiave effimera: 32 byte, più un carico
/// che Rymembra lascia vuoto. Mille byte lasciano posto a un carico futuro, e
/// leggerli costa niente; di più, prima della stretta, è solo memoria data a
/// chi non si sa chi sia.
pub(crate) const PRIMO_MASSIMO: usize = 1024;

/// Il primo messaggio della stretta, entro `scadenza`: chi apre una
/// connessione deve parlare per primo, e chi non parla non arriva mai alla
/// parte che costa. Vedi `ascolto`.
///
/// # Errori
///
/// Vedi [`Guasto`]; [`Guasto::Protocollo`] oltre [`PRIMO_MASSIMO`].
pub(crate) fn primo_messaggio(presa: &mut TcpStream, scadenza: Instant) -> Result<Vec<u8>, Guasto> {
    let mut testa = [0_u8; 2];
    leggi_esatto(presa, &mut testa, scadenza)?;
    let lunghezza = usize::from(u16::from_be_bytes(testa));
    if lunghezza == 0 || lunghezza > PRIMO_MASSIMO {
        return Err(Guasto::Protocollo);
    }
    let mut messaggio = vec![0_u8; lunghezza];
    leggi_esatto(presa, &mut messaggio, scadenza)?;
    Ok(messaggio)
}

/// La stretta `XX`, fino a `scadenza`.
///
/// # Il posto fra le strette
///
/// `curva_fatta` si chiama una volta, appena prima di aspettare un messaggio
/// dalla rete. Per chi risponde col primo messaggio già letto è il momento in
/// cui ha scritto il secondo: il lavoro sulla curva che chi bussa gli può far
/// fare gratis è finito, ed è lì che chi ospita lascia il suo posto fra le
/// strette (vedi «Le strette, e chi non parla» in `ascolto`). Chi non ha un
/// posto da lasciare passa una chiusura vuota. Se la stretta finisce prima,
/// la chiusura se ne va con lei, e quel che teneva con la chiusura.
///
/// # Errori
///
/// [`Guasto::Silenzio`] o [`Guasto::Rete`] se l'altro capo non c'è, o non
/// risponde in tempo; [`Guasto::Protocollo`] se manda qualcosa che non è una
/// stretta Noise di questo schema.
pub(crate) fn stringi(
    presa: &mut TcpStream,
    identita: &Identita,
    iniziatore: bool,
    primo: Option<&[u8]>,
    curva_fatta: impl FnOnce(),
    scadenza: Instant,
) -> Result<Canale, Guasto> {
    let schema = SCHEMA.parse().map_err(|_| Guasto::Protocollo)?;
    let costruttore = Builder::new(schema)
        .local_private_key(identita.segreta())
        .and_then(|b| b.prologue(PROLOGO))
        .map_err(|_| Guasto::Protocollo)?;
    let mut stretta = if iniziatore {
        costruttore.build_initiator()
    } else {
        costruttore.build_responder()
    }
    .map_err(|_| Guasto::Protocollo)?;

    let mut cifrato = vec![0_u8; MASSIMO_NOISE];
    let mut chiaro = vec![0_u8; MASSIMO_NOISE];
    // Chi risponde può aver già letto il primo messaggio (vedi
    // [`primo_messaggio`]): si usa quello, una volta.
    let mut gia_letto = primo;
    let mut curva_fatta = Some(curva_fatta);
    // `XX` ha tre messaggi; il tetto c'è perché un ciclo che dipende da quel
    // che dice l'altro capo deve avere una fine anche se l'altro capo mente.
    for _ in 0..3 {
        if stretta.is_handshake_finished() {
            break;
        }
        if stretta.is_my_turn() {
            let scritti = stretta
                .write_message(&[], &mut cifrato)
                .map_err(|_| Guasto::Protocollo)?;
            let messaggio = cifrato.get(..scritti).ok_or(Guasto::Protocollo)?;
            scrivi_cornice(presa, messaggio, scadenza)?;
        } else if let Some(messaggio) = gia_letto.take() {
            stretta
                .read_message(messaggio, &mut chiaro)
                .map_err(|_| Guasto::Protocollo)?;
        } else {
            // Prima di aspettare la rete: vedi «Il posto fra le strette».
            if let Some(lascia) = curva_fatta.take() {
                lascia();
            }
            let letti = leggi_cornice(presa, &mut cifrato, scadenza)?;
            let messaggio = cifrato.get(..letti).ok_or(Guasto::Protocollo)?;
            stretta
                .read_message(messaggio, &mut chiaro)
                .map_err(|_| Guasto::Protocollo)?;
        }
    }
    if !stretta.is_handshake_finished() {
        return Err(Guasto::Protocollo);
    }
    let hash =
        <[u8; 32]>::try_from(stretta.get_handshake_hash()).map_err(|_| Guasto::Protocollo)?;
    let pari = stretta
        .get_remote_static()
        .and_then(|k| <[u8; 32]>::try_from(k).ok())
        .ok_or(Guasto::Protocollo)?;
    let stato = stretta
        .into_stateless_transport_mode()
        .map_err(|_| Guasto::Protocollo)?;
    Ok(Canale {
        pari: ChiavePubblica(pari),
        hash,
        stato: Arc::new(stato),
    })
}

/// Chi cifra i messaggi in uscita. Ce n'è uno per connessione, nel filo che
/// scrive: vedi «Lo stato senza stato» qui sopra.
pub(crate) struct Cifratore {
    stato: Arc<StatelessTransportState>,
    /// Il nonce del prossimo messaggio Noise in uscita.
    nonce: u64,
    /// Le cornici pronte, da scrivere in un colpo solo.
    uscita: Vec<u8>,
}

impl Cifratore {
    /// Cifra un messaggio del trasporto e ne prepara le cornici.
    ///
    /// # Errori
    ///
    /// [`Guasto::Protocollo`] per un carico oltre il tetto: è un errore di chi
    /// lo prepara, e non deve partire.
    pub(crate) fn aggiungi(&mut self, tipo: Tipo, carico: &[u8]) -> Result<(), Guasto> {
        for pezzo in cornice::spezza(tipo, carico).map_err(|_| Guasto::Protocollo)? {
            let lunghezza = pezzo.len().saturating_add(ETICHETTA);
            let testa = u16::try_from(lunghezza).map_err(|_| Guasto::Protocollo)?;
            self.uscita.extend_from_slice(&testa.to_be_bytes());
            let inizio = self.uscita.len();
            self.uscita.resize(inizio.saturating_add(lunghezza), 0);
            let dove = self.uscita.get_mut(inizio..).ok_or(Guasto::Protocollo)?;
            let scritti = self
                .stato
                .write_message(self.nonce, &pezzo, dove)
                .map_err(|_| Guasto::Protocollo)?;
            if scritti != lunghezza {
                return Err(Guasto::Protocollo);
            }
            self.nonce = self.nonce.checked_add(1).ok_or(Guasto::Protocollo)?;
        }
        Ok(())
    }

    /// Solo per le prove: cifra un chiaro qualunque in un messaggio Noise,
    /// senza passare da [`cornice::spezza`] — cioè anche una testa che
    /// annuncia più del tetto, che `spezza` non lascerebbe partire.
    #[cfg(test)]
    pub(crate) fn cifra_chiaro(&mut self, chiaro: &[u8]) -> Result<(), Guasto> {
        let lunghezza = chiaro.len().saturating_add(ETICHETTA);
        let testa = u16::try_from(lunghezza).map_err(|_| Guasto::Protocollo)?;
        self.uscita.extend_from_slice(&testa.to_be_bytes());
        let inizio = self.uscita.len();
        self.uscita.resize(inizio.saturating_add(lunghezza), 0);
        let dove = self.uscita.get_mut(inizio..).ok_or(Guasto::Protocollo)?;
        self.stato
            .write_message(self.nonce, chiaro, dove)
            .map_err(|_| Guasto::Protocollo)?;
        self.nonce = self.nonce.checked_add(1).ok_or(Guasto::Protocollo)?;
        Ok(())
    }

    /// Scrive le cornici pronte. Con `TCP_NODELAY` e un `write_all` solo, un
    /// messaggio piccolo parte in un pacchetto solo.
    ///
    /// # Errori
    ///
    /// Quelli del socket.
    pub(crate) fn manda(&mut self, presa: &mut TcpStream) -> Result<(), Guasto> {
        let esito = presa.write_all(&self.uscita);
        self.uscita.clear();
        if self.uscita.capacity() > BUFFER_TENUTO {
            self.uscita.shrink_to(BUFFER_RISTRETTO);
        }
        esito.map_err(Guasto::from)
    }
}

/// Chi decifra i messaggi in entrata: uno per connessione, nel filo che legge.
pub(crate) struct Decifratore {
    stato: Arc<StatelessTransportState>,
    /// Il nonce del prossimo messaggio Noise in entrata.
    nonce: u64,
    ricompositore: Ricompositore,
    cifrato: Vec<u8>,
    chiaro: Vec<u8>,
}

impl Decifratore {
    /// I tetti dei messaggi che si accettano da qui in avanti, JSON, byte e
    /// JSON compressi, al più `cornice::MASSIMO_MESSAGGIO`. Vedi
    /// `Ricompositore::imposta_tetti`.
    pub(crate) fn imposta_tetti(&mut self, json: usize, byte: usize, compresso: usize) {
        self.ricompositore.imposta_tetti(json, byte, compresso);
    }

    /// Il prossimo messaggio del trasporto, con l'istante d'arrivo (sulla base
    /// dei tempi di processo) della cornice che lo ha completato.
    ///
    /// Ogni cornice ha `silenzio` di tempo da quando la si comincia ad
    /// aspettare: una connessione che non manda una cornice intera per tanto
    /// così è una connessione caduta. Contano le cornici e non i byte, perché
    /// un capo che manda un byte ogni quattordici secondi non è un capo che
    /// parla.
    ///
    /// # I tetti, quando arriva la testa
    ///
    /// I tetti del messaggio li decide `tetti` — JSON, byte e compressi, come
    /// in [`Decifratore::imposta_tetti`] — **quando ne arriva la testa**, non
    /// prima di cominciare ad aspettarla. È il filo che legge una connessione,
    /// e aspetta anche quindici secondi: intanto l'altro capo può diventare
    /// fidato (un'associazione finita dall'altro filo, il desktop che chiama
    /// `pari`) o smettere di esserlo. Fino alla revisione del 26 ottobre i
    /// tetti si fissavano prima dell'attesa, e il primo `Stato` dopo
    /// un'associazione — fino a centinaia di kibibyte — trovava ancora i quattro
    /// dell'estraneo e chiudeva con `insieme.protocollo`. Il controllo resta
    /// prima dell'allocazione: vedi «Il controllo prima dell'allocazione» in
    /// `cornice`.
    ///
    /// # I compressi
    ///
    /// Un [`Tipo::JsonCompresso`] esce di qui già decompresso, come
    /// [`Tipo::Json`], col tetto dei JSON di quel capo: vedi «La
    /// compressione» in `cornice`. Uno che decompresso non ci sta, o che non
    /// si decomprime, è [`Guasto::Protocollo`].
    ///
    /// # Errori
    ///
    /// Vedi [`Guasto`].
    pub(crate) fn prossimo_coi_tetti(
        &mut self,
        presa: &mut TcpStream,
        silenzio: Duration,
        adesso_ns: fn() -> u64,
        tetti: &dyn Fn() -> (usize, usize, usize),
    ) -> Result<(Tipo, Vec<u8>, u64), Guasto> {
        let (tipo, carico, arrivo) =
            self.prossimo_dentro(presa, silenzio, adesso_ns, Some(tetti))?;
        if tipo != Tipo::JsonCompresso {
            return Ok((tipo, carico, arrivo));
        }
        let json = cornice::gonfia(&carico, self.ricompositore.tetto_json())
            .map_err(|_| Guasto::Protocollo)?;
        Ok((Tipo::Json, json, arrivo))
    }

    /// Solo per le prove: come [`Decifratore::prossimo_coi_tetti`], coi tetti
    /// fissati da [`Decifratore::imposta_tetti`] (di serie, il mebibyte), e coi
    /// messaggi compressi **come sono arrivati**: le prove guardano anche che
    /// cosa viaggia sul filo.
    #[cfg(test)]
    pub(crate) fn prossimo(
        &mut self,
        presa: &mut TcpStream,
        silenzio: Duration,
        adesso_ns: fn() -> u64,
    ) -> Result<(Tipo, Vec<u8>, u64), Guasto> {
        self.prossimo_dentro(presa, silenzio, adesso_ns, None)
    }

    /// Il corpo di [`Decifratore::prossimo_coi_tetti`], e di `prossimo` nelle
    /// prove: il messaggio come è arrivato.
    fn prossimo_dentro(
        &mut self,
        presa: &mut TcpStream,
        silenzio: Duration,
        adesso_ns: fn() -> u64,
        tetti: Option<&dyn Fn() -> (usize, usize, usize)>,
    ) -> Result<(Tipo, Vec<u8>, u64), Guasto> {
        loop {
            let letti = leggi_cornice(presa, &mut self.cifrato, Instant::now() + silenzio)?;
            // L'istante d'arrivo, appena la cornice è intera e **prima** di
            // decifrarla: per un ping è `t2`, per un pong `t4`, e il tempo
            // della decifratura non è tempo di rete.
            let arrivo = adesso_ns();
            if let Some(messaggio) = self.decifra(letti, tetti)? {
                let (tipo, carico) = messaggio;
                return Ok((tipo, carico, arrivo));
            }
        }
    }

    /// Come [`Decifratore::prossimo_coi_tetti`], ma con una scadenza fissa per
    /// tutto e coi tetti fissati prima: è il saluto, che sta dentro i cinque
    /// secondi della stretta.
    ///
    /// # Errori
    ///
    /// Vedi [`Guasto`].
    pub(crate) fn prossimo_entro(
        &mut self,
        presa: &mut TcpStream,
        scadenza: Instant,
    ) -> Result<(Tipo, Vec<u8>), Guasto> {
        loop {
            let letti = leggi_cornice(presa, &mut self.cifrato, scadenza)?;
            if let Some(messaggio) = self.decifra(letti, None)? {
                return Ok(messaggio);
            }
        }
    }

    /// Decifra la cornice appena letta e la passa al ricompositore. Se la
    /// cornice comincia un messaggio nuovo e c'è chi decide i tetti, li si
    /// chiede adesso: vedi [`Decifratore::prossimo_coi_tetti`].
    fn decifra(
        &mut self,
        letti: usize,
        tetti: Option<&dyn Fn() -> (usize, usize, usize)>,
    ) -> Result<Option<(Tipo, Vec<u8>)>, Guasto> {
        if letti < ETICHETTA {
            return Err(Guasto::Protocollo);
        }
        let cifrato = self.cifrato.get(..letti).ok_or(Guasto::Protocollo)?;
        let chiari = self
            .stato
            .read_message(self.nonce, cifrato, &mut self.chiaro)
            .map_err(|_| Guasto::Protocollo)?;
        self.nonce = self.nonce.checked_add(1).ok_or(Guasto::Protocollo)?;
        let chiaro = self.chiaro.get(..chiari).ok_or(Guasto::Protocollo)?;
        if let Some(tetti) = tetti
            && self.ricompositore.fra_due_messaggi()
        {
            let (json, byte, compresso) = tetti();
            self.ricompositore.imposta_tetti(json, byte, compresso);
        }
        self.ricompositore
            .aggiungi(chiaro)
            .map_err(|_| Guasto::Protocollo)
    }
}

/// Scrive un messaggio Noise con la sua lunghezza davanti, entro `scadenza`.
fn scrivi_cornice(
    presa: &mut TcpStream,
    messaggio: &[u8],
    scadenza: Instant,
) -> Result<(), Guasto> {
    let lunghezza = u16::try_from(messaggio.len()).map_err(|_| Guasto::Protocollo)?;
    let resta = scadenza.saturating_duration_since(Instant::now());
    if resta.is_zero() {
        return Err(Guasto::Silenzio);
    }
    presa.set_write_timeout(Some(resta))?;
    let mut cornice = Vec::with_capacity(messaggio.len().saturating_add(2));
    cornice.extend_from_slice(&lunghezza.to_be_bytes());
    cornice.extend_from_slice(messaggio);
    presa.write_all(&cornice)?;
    Ok(())
}

/// Legge un messaggio Noise in `buffer` (lungo [`MASSIMO_NOISE`]), entro
/// `scadenza`. Restituisce quanti byte.
fn leggi_cornice(
    presa: &mut TcpStream,
    buffer: &mut [u8],
    scadenza: Instant,
) -> Result<usize, Guasto> {
    let mut testa = [0_u8; 2];
    leggi_esatto(presa, &mut testa, scadenza)?;
    let lunghezza = usize::from(u16::from_be_bytes(testa));
    if lunghezza == 0 {
        return Err(Guasto::Protocollo);
    }
    let dove = buffer.get_mut(..lunghezza).ok_or(Guasto::Protocollo)?;
    leggi_esatto(presa, dove, scadenza)?;
    Ok(lunghezza)
}

/// Riempie `buffer`, entro `scadenza`.
///
/// La scadenza del socket si rimette prima di ogni `read` col tempo che
/// resta: così è una scadenza per tutta la cornice, e non per ogni pezzo che
/// il TCP consegna.
fn leggi_esatto(presa: &mut TcpStream, buffer: &mut [u8], scadenza: Instant) -> Result<(), Guasto> {
    let mut fatti = 0;
    while fatti < buffer.len() {
        let resta = scadenza.saturating_duration_since(Instant::now());
        if resta.is_zero() {
            return Err(Guasto::Silenzio);
        }
        presa.set_read_timeout(Some(resta))?;
        let dove = buffer.get_mut(fatti..).ok_or(Guasto::Protocollo)?;
        match presa.read(dove) {
            Ok(0) => return Err(Guasto::Rete),
            Ok(letti) => fatti = fatti.saturating_add(letti),
            Err(err) if err.kind() == ErrorKind::Interrupted => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod prove {
    use super::*;
    use std::net::{Ipv4Addr, TcpListener};

    /// Due capi di una connessione su `127.0.0.1`, stretta fatta.
    pub(crate) fn coppia() -> ((TcpStream, Canale), (TcpStream, Canale)) {
        let ascolto = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("ascolto");
        let indirizzo = ascolto.local_addr().expect("indirizzo");
        let identita_b = Identita::nuova().expect("chiave");
        let filo = std::thread::spawn(move || {
            let (mut presa, _) = ascolto.accept().expect("accetta");
            let canale = stringi(
                &mut presa,
                &identita_b,
                false,
                None,
                || (),
                Instant::now() + Duration::from_secs(5),
            )
            .expect("stretta dal lato che risponde");
            (presa, canale)
        });
        let identita_a = Identita::nuova().expect("chiave");
        let mut presa = TcpStream::connect(indirizzo).expect("connessione");
        let canale = stringi(
            &mut presa,
            &identita_a,
            true,
            None,
            || (),
            Instant::now() + Duration::from_secs(5),
        )
        .expect("stretta dal lato che comincia");
        ((presa, canale), filo.join().expect("filo"))
    }

    fn adesso() -> u64 {
        1
    }

    #[test]
    fn la_stretta_da_a_ciascuno_la_chiave_dell_altro_e_lo_stesso_hash() {
        let ((_, a), (_, b)) = coppia();
        assert_ne!(a.pari, b.pari);
        assert_eq!(
            a.hash, b.hash,
            "l'hash della stretta è lo stesso sui due capi"
        );
    }

    #[test]
    fn un_mebibyte_attraversa_il_canale_spezzato_e_torna_intero() {
        let ((mut presa_a, a), (mut presa_b, b)) = coppia();
        let carico: Vec<u8> = (0..cornice::MASSIMO_MESSAGGIO)
            .map(|i| u8::try_from(i % 253).unwrap_or(0))
            .collect();
        let atteso = carico.clone();
        let scrittore = std::thread::spawn(move || {
            let (mut cifratore, _) = a.dividi();
            cifratore.aggiungi(Tipo::Byte, &carico).expect("nel tetto");
            cifratore.aggiungi(Tipo::Json, b"{}").expect("nel tetto");
            cifratore.manda(&mut presa_a).expect("scritto");
            presa_a
        });
        let (_, mut decifratore) = b.dividi();
        let (tipo, letto, _) = decifratore
            .prossimo(&mut presa_b, Duration::from_secs(5), adesso)
            .expect("arriva");
        assert_eq!(tipo, Tipo::Byte);
        assert!(letto == atteso, "il mebibyte torna uguale");
        let (tipo, letto, _) = decifratore
            .prossimo(&mut presa_b, Duration::from_secs(5), adesso)
            .expect("arriva anche il secondo");
        assert_eq!((tipo, letto), (Tipo::Json, b"{}".to_vec()));
        drop(scrittore.join().expect("filo"));
    }

    #[test]
    fn un_mebibyte_e_un_byte_non_parte() {
        let ((_, a), _) = coppia();
        let (mut cifratore, _) = a.dividi();
        assert_eq!(
            cifratore.aggiungi(Tipo::Byte, &vec![0; cornice::MASSIMO_MESSAGGIO + 1]),
            Err(Guasto::Protocollo)
        );
    }

    #[test]
    fn una_cornice_manomessa_non_si_decifra() {
        let ((mut presa_a, a), (mut presa_b, b)) = coppia();
        let (mut cifratore, _) = a.dividi();
        cifratore.aggiungi(Tipo::Json, b"{}").expect("nel tetto");
        // Un bit cambiato nell'ultimo byte, cioè nell'etichetta.
        if let Some(ultimo) = cifratore.uscita.last_mut() {
            *ultimo ^= 1;
        }
        cifratore.manda(&mut presa_a).expect("scritto");
        assert_eq!(
            b.dividi()
                .1
                .prossimo(&mut presa_b, Duration::from_secs(5), adesso)
                .map(|_| ()),
            Err(Guasto::Protocollo)
        );
    }

    #[test]
    fn senza_niente_in_arrivo_scade_il_silenzio() {
        let (_tenuta, (mut presa_b, b)) = coppia();
        let inizio = Instant::now();
        assert_eq!(
            b.dividi()
                .1
                .prossimo(&mut presa_b, Duration::from_millis(200), adesso)
                .map(|_| ()),
            Err(Guasto::Silenzio)
        );
        assert!(inizio.elapsed() >= Duration::from_millis(150));
    }
}
