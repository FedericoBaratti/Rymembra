//! Il filo del collegamento: accende e spegne il nodo, gli dice chi seguire e
//! cosa dire di sé, e tiene il pannello.
//!
//! # Quando il nodo è acceso
//!
//! Con «Raggiungibile da internet» acceso, e:
//!
//! - su Windows sempre, finché gira Rymembra (anche dal vassoio);
//! - sul telefono mentre lo si usa — l'attività è in vista, o la musica
//!   suona — e per [`SVEGLIO_DOPO`] dopo. Poi si spegne: un telefono in tasca
//!   non tiene aperta una porta per niente, e la batteria ringrazia.
//!
//! # Chi scrive cosa
//!
//! Il filo è il solo a comporre il pannello e a mandarlo alla finestra. I
//! comandi e l'[`super::ospite::Ospite`] toccano [`Comune`] e lo svegliano con
//! la [`Posta`]; lui rilegge la libreria quando serve, e pubblica se qualcosa
//! è cambiato davvero.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

use rymembra_app::dispositivi::{self, Dispositivo};
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_lontano::{
    Evento, Impostazioni, Nodo, PariNoto, Presentazione, Presenza, Relay, Rete, Via,
};
use tauri::{AppHandle, Manager as _};

use super::ipc::{DispositivoIpc, InvitoIpc, RichiestaIpc, StatoLontanoIpc};
use super::{identita, ospite};
use crate::nota;
use crate::spegnimento::Emette as _;
use crate::stato::{Stato, adesso_ms};

/// Ogni quanto il filo si guarda intorno da sé: le impostazioni cambiate da
/// un'altra strada, il telefono andato in tasca.
const GIRO: Duration = Duration::from_secs(20);

/// Quanto si aspetta la libreria. Il filo non ha fretta: occupata, si
/// riprova al giro dopo.
pub(super) const ATTESA_LIBRERIA: Duration = Duration::from_secs(2);

/// Dopo un'accensione non riuscita, quanto prima di riprovare da sé.
const RIPROVA_DOPO: Duration = Duration::from_secs(60);

/// Quanto spesso si scrive nella libreria che un dispositivo risponde.
const SCRIVI_VISTO_OGNI: Duration = Duration::from_secs(60);

/// Quanto un dispositivo che non risponde più resta in vista prima di dirlo
/// assente: una connessione che cade e si riapre — il Wi-Fi che passa al 5G,
/// due chiamate incrociate — non deve far sparire la pagina «Sfoglia» per un
/// secondo.
const GRAZIA: Duration = Duration::from_secs(8);

/// Il telefono resta raggiungibile tanto dopo l'ultimo uso.
#[cfg(target_os = "android")]
const SVEGLIO_DOPO: Duration = Duration::from_secs(2 * 60);

/// La piattaforma, come la si dice agli altri.
pub(super) const PIATTAFORMA: &str = if cfg!(target_os = "android") {
    "android"
} else {
    "windows"
};

/// Quel che arriva al filo.
pub(super) enum Posta {
    /// Le impostazioni o i dispositivi sono cambiati: si rilegge. Chi aspetta
    /// sa quando è fatto.
    Rileggi {
        /// Per chi aspetta.
        risposta: Option<Sender<()>>,
        /// L'ha chiesto la persona: un'accensione non riuscita si riprova
        /// adesso, non dopo [`RIPROVA_DOPO`].
        subito: bool,
    },
    /// Il pannello è cambiato in memoria (un QR, una domanda): si pubblica.
    Pubblica,
    /// Dal nodo.
    Evento(Evento),
    /// La rete del sistema è cambiata.
    CambioDiRete,
    /// C'è un ascolto nuovo da mandare: vedi `sfoglia::ascoltato_qui`.
    Ascolti,
    /// Si esce.
    Ferma(Sender<()>),
}

/// Quel che il filo condivide con i comandi e con l'ospite.
pub(super) struct Comune {
    /// Il nodo, quando è acceso. In un `Arc` perché un comando lo usa senza
    /// tenere il lucchetto: spento qui si chiude subito, anche per il comando
    /// in volo (`lascia_il_nodo`), e il suo runtime se ne va quando l'ultimo
    /// lo lascia.
    pub nodo: Mutex<Option<Arc<Nodo>>>,
    /// Le chiavi dei dispositivi associati e non revocati: il cancello del
    /// nodo le chiede a ogni connessione, e non deve aspettare la libreria.
    pub associati: RwLock<HashSet<String>>,
    /// Chi risponde adesso, e cosa dice di sé.
    pub presenze: Mutex<HashMap<String, Presenza>>,
    /// Chi aspetta un sì o un no.
    pub richieste: Mutex<Vec<(RichiestaIpc, Sender<bool>)>>,
    /// Il numero della prossima domanda.
    pub prossima: AtomicU64,
    /// Il QR mostrato.
    pub invito: Mutex<Option<InvitoIpc>>,
    /// Il pannello com'è stato pubblicato l'ultima volta.
    pub vetrina: Mutex<StatoLontanoIpc>,
    /// Ascolti e scaricamenti da un altro dispositivo in corso: sul telefono
    /// tengono acceso il nodo anche a schermo spento.
    pub in_volo: AtomicUsize,
    /// Perché il nodo non si è acceso l'ultima volta: per chi l'ha chiesto.
    pub ultimo_errore: Mutex<Option<AppError>>,
    /// I dispositivi online che ospitano una sessione di «Ascolta insieme»,
    /// con la loro chiave di «Ascolta insieme»: vedi `insieme::ospitano`.
    pub ospitano: Mutex<Vec<(String, String)>>,
    /// Cresce a ogni «Annulla» di un collegamento: un comando che stava
    /// ancora accendendo il nodo se ne accorge prima di cominciare ad aspettare.
    pub annulli: AtomicU64,
    /// La cassetta del filo.
    pub posta: Sender<Posta>,
}

impl Comune {
    pub(super) fn nuovo(posta: Sender<Posta>) -> Self {
        Self {
            nodo: Mutex::new(None),
            associati: RwLock::new(HashSet::new()),
            presenze: Mutex::new(HashMap::new()),
            richieste: Mutex::new(Vec::new()),
            prossima: AtomicU64::new(1),
            invito: Mutex::new(None),
            vetrina: Mutex::new(StatoLontanoIpc::default()),
            in_volo: AtomicUsize::new(0),
            ultimo_errore: Mutex::new(None),
            ospitano: Mutex::new(Vec::new()),
            annulli: AtomicU64::new(0),
            posta,
        }
    }

    /// Il nodo acceso, se c'è.
    pub(super) fn nodo(&self) -> Option<Arc<Nodo>> {
        prendi(&self.nodo).clone()
    }

    /// Questa chiave può entrare.
    pub(super) fn associato(&self, chiave: &str) -> bool {
        self.associati
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(chiave)
    }

    /// Una chiave entra (subito: la connessione che segue l'associazione non
    /// deve trovare il cancello chiuso) o esce.
    pub(super) fn ammetti(&self, chiave: &str, dentro: bool) {
        let mut associati = self
            .associati
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        if dentro {
            associati.insert(chiave.to_owned());
        } else {
            associati.remove(chiave);
        }
    }

    /// C'è un ascolto o uno scaricamento in corso.
    #[cfg_attr(
        not(target_os = "android"),
        expect(dead_code, reason = "serve al telefono")
    )]
    pub(super) fn in_volo(&self) -> bool {
        self.in_volo.load(Ordering::Relaxed) > 0
    }

    pub(super) fn sveglia(&self, posta: Posta) {
        let _ = self.posta.send(posta);
    }
}

/// Un lucchetto, anche avvelenato: quel che c'è dentro resta coerente,
/// perché nessuno lo lascia a metà.
pub(super) fn prendi<T>(lucchetto: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    lucchetto.lock().unwrap_or_else(PoisonError::into_inner)
}

/// La libreria, aspettata al più `entro`. Non presa in tempo è `db.locked`.
pub(super) fn in_libreria<T>(
    app: &AppHandle,
    entro: Duration,
    azione: impl FnOnce(&rusqlite::Connection) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let stato = app.try_state::<Stato>().ok_or_else(|| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some("lontano".to_owned()),
        })
    })?;
    crate::stato::con_libreria_entro(&stato, entro, |l| azione(&l.connection)).unwrap_or_else(
        || {
            Err(AppError::new(ErrorCode::DbLocked)
                .with_cause("la libreria è occupata: il collegamento non l'aspetta"))
        },
    )
}

/// Fa partire il filo.
pub(super) fn avvia(app: AppHandle, comune: Arc<Comune>, cassetta: Receiver<Posta>) {
    let girato = std::thread::Builder::new()
        .name("rymembra-lontano-filo".to_owned())
        .spawn(move || Filo::nuovo(app, comune).gira(&cassetta));
    if let Err(err) = girato {
        nota!("[lontano] il filo non parte: {err}");
    }
}

/// Quel che si legge dalla libreria a ogni giro.
struct Letto {
    impostazioni: dispositivi::Impostazioni,
    dispositivi: Vec<Dispositivo>,
    nome: String,
    insieme_acceso: bool,
    /// I PC fidati di «Ascolta insieme», per chiave: la riga di ciascuno.
    pari_insieme: HashMap<String, i64>,
}

/// La lettura di un giro.
fn leggi(c: &rusqlite::Connection) -> Result<Letto, AppError> {
    let insieme = rymembra_app::insieme::impostazioni(c)?;
    Ok(Letto {
        impostazioni: dispositivi::impostazioni(c)?,
        dispositivi: dispositivi::tutti(c)?,
        nome: insieme
            .nome
            .or_else(crate::insieme::nome_del_dispositivo)
            .unwrap_or_else(|| "Rymembra".to_owned()),
        insieme_acceso: insieme.attivo,
        pari_insieme: rymembra_app::insieme::pari(c)?
            .into_iter()
            .map(|p| (p.chiave, p.id))
            .collect(),
    })
}

/// Tiene la lettura appena fatta e, se è riuscita, rifà da lei il cancello.
/// `true` se è riuscita.
///
/// Con la libreria occupata (una scansione, «Rileggi tutti i tag») si tiene
/// l'ultima lettura per il resto — accendere, spegnere, il nome — ma il
/// cancello non si tocca: resta con le modifiche puntuali che
/// [`Comune::ammetti`] ha fatto nel frattempo, e lo stesso vale per chi il
/// nodo segue (`Nodo::pari`, solo dopo una lettura riuscita). Fino al reperto
/// 02-8 lo si rifaceva dalla lettura vecchia: un dispositivo appena
/// dimenticato tornava dentro, e il nodo lo tornava a seguire, fino alla
/// lettura riuscita dopo; uno appena collegato restava fuori.
fn tieni_la_lettura(
    comune: &Comune,
    letto: &mut Option<Letto>,
    lettura: Result<Letto, AppError>,
) -> bool {
    match lettura {
        Ok(nuova) => {
            {
                let mut associati = comune
                    .associati
                    .write()
                    .unwrap_or_else(PoisonError::into_inner);
                *associati = nuova
                    .dispositivi
                    .iter()
                    .filter(|d| !d.revocato)
                    .map(|d| d.chiave.clone())
                    .collect();
            }
            *letto = Some(nuova);
            true
        }
        Err(err) => {
            if letto.is_none() {
                nota!(
                    "[lontano] la libreria non si legge codice={}: si riprova",
                    err.code().kind().code()
                );
            }
            false
        }
    }
}

/// Il nodo tolto da [`Comune::nodo`]: si chiude subito, anche se un comando
/// lo tiene ancora — nessuno lo raggiunge più, il QR non vale più, e quel
/// comando torna con `lontano.spento` — e il suo runtime se ne va con
/// l'ultimo che lo lascia.
///
/// Fino al reperto 02-7 si fermava solo se nessuno lo teneva: un `collega`
/// che aspettava il sì dell'altro (fino a quattro minuti), o una lettura di
/// «Sfoglia» in volo, lo lasciava acceso e raggiungibile dopo l'interruttore
/// spento, e riacceso subito ne partiva un secondo con la stessa chiave.
fn lascia_il_nodo(nodo: Arc<Nodo>) {
    nodo.chiudi();
    drop(nodo);
}

struct Filo {
    app: AppHandle,
    comune: Arc<Comune>,
    /// L'ultima lettura riuscita; `None` finché non ce n'è una.
    letto: Option<Letto>,
    /// Perché il nodo non si è acceso.
    errore: Option<String>,
    /// Il relay con cui il nodo è acceso.
    acceso_con: Option<Option<String>>,
    /// Non prima di allora, dopo un'accensione non riuscita.
    riprova: Option<Instant>,
    /// Quando si è scritto l'ultima volta che un dispositivo risponde.
    visti: HashMap<String, Instant>,
    /// Il pannello pubblicato, in JSON: si manda solo se cambia.
    pubblicata: Option<String>,
    /// I dispositivi che non rispondono più, ancora in vista fino a quando:
    /// vedi [`GRAZIA`].
    in_forse: HashMap<String, Instant>,
    /// Da dove passava l'ultima volta la connessione con ciascuno: il diario
    /// lo dice quando cambia.
    vie: HashMap<String, Via>,
    /// L'ultima volta che il telefono era in uso.
    #[cfg(target_os = "android")]
    in_uso: Instant,
}

impl Filo {
    fn nuovo(app: AppHandle, comune: Arc<Comune>) -> Self {
        Self {
            app,
            comune,
            letto: None,
            errore: None,
            acceso_con: None,
            riprova: None,
            visti: HashMap::new(),
            pubblicata: None,
            in_forse: HashMap::new(),
            vie: HashMap::new(),
            #[cfg(target_os = "android")]
            in_uso: Instant::now(),
        }
    }

    fn gira(mut self, cassetta: &Receiver<Posta>) {
        self.riconcilia(false);
        let mut prossimo_giro = Instant::now() + GIRO;
        loop {
            // Il giro, o prima la fine della grazia di un dispositivo.
            let scadenza = self
                .in_forse
                .values()
                .copied()
                .fold(prossimo_giro, Instant::min);
            let posta = cassetta.recv_timeout(scadenza.saturating_duration_since(Instant::now()));
            self.finisce_la_grazia();
            // Anche con la posta che arriva fitta il giro non salta.
            if Instant::now() >= prossimo_giro {
                prossimo_giro = Instant::now() + GIRO;
                self.riconcilia(false);
                if super::sfoglia::da_riprovare() {
                    super::sfoglia::manda_gli_ascolti(&self.app, &self.comune, None);
                }
            }
            match posta {
                Ok(Posta::Rileggi { risposta, subito }) => {
                    self.riconcilia(subito);
                    if let Some(risposta) = risposta {
                        let _ = risposta.send(());
                    }
                }
                Ok(Posta::Pubblica) => self.pubblica(),
                Ok(Posta::Evento(evento)) => self.evento(evento),
                Ok(Posta::CambioDiRete) => {
                    if let Some(nodo) = self.comune.nodo() {
                        nodo.cambio_di_rete();
                    }
                }
                Ok(Posta::Ascolti) => {
                    super::sfoglia::manda_gli_ascolti(&self.app, &self.comune, None);
                }
                Ok(Posta::Ferma(risposta)) => {
                    self.spegni();
                    let _ = risposta.send(());
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    self.spegni();
                    return;
                }
            }
        }
    }

    /// I dispositivi la cui grazia è finita senza che tornassero: assenti
    /// davvero.
    fn finisce_la_grazia(&mut self) {
        let adesso = Instant::now();
        let finiti: Vec<String> = self
            .in_forse
            .iter()
            .filter(|(_, fine)| **fine <= adesso)
            .map(|(chiave, _)| chiave.clone())
            .collect();
        if finiti.is_empty() {
            return;
        }
        {
            let mut presenze = prendi(&self.comune.presenze);
            for chiave in &finiti {
                self.in_forse.remove(chiave);
                presenze.remove(chiave);
            }
        }
        self.pubblica();
    }

    /// Il QR in vista, se non vale più — scaduto, o finito nel nodo — si
    /// toglie. Uno nuovo, che il nodo ha già, resta.
    fn togli_il_qr_finito(&self) {
        let nodo = self.comune.nodo();
        let mut invito = prendi(&self.comune.invito);
        let finito = invito.as_ref().is_some_and(|i| {
            i.scade_ms <= adesso_ms() || nodo.as_ref().is_none_or(|n| !n.invito_attivo())
        });
        if finito {
            *invito = None;
        }
    }

    /// Rilegge, accende o spegne, e pubblica. `subito`: vedi [`Posta::Rileggi`].
    fn riconcilia(&mut self, subito: bool) {
        let lettura = in_libreria(&self.app, ATTESA_LIBRERIA, leggi);
        let riuscita = tieni_la_lettura(&self.comune, &mut self.letto, lettura);
        let Some(letto) = &self.letto else {
            return;
        };
        let attivo = letto.impostazioni.attivo;
        let relay = letto.impostazioni.relay.clone();
        let voluto = attivo && self.in_uso();
        if self
            .acceso_con
            .as_ref()
            .is_some_and(|prima| *prima != relay)
        {
            nota!("[lontano] il relay è cambiato: il nodo riparte");
            self.spegni();
        }
        if !attivo {
            self.errore = None;
            self.riprova = None;
            *prendi(&self.comune.ultimo_errore) = None;
        }
        if voluto
            && self.comune.nodo().is_none()
            && (subito || self.riprova.is_none_or(|q| Instant::now() >= q))
        {
            match self.accendi(relay) {
                Ok(()) => {
                    self.errore = None;
                    self.riprova = None;
                    *prendi(&self.comune.ultimo_errore) = None;
                }
                Err(err) => {
                    nota!(
                        "[lontano] il nodo non si accende codice={} causa={}",
                        err.code().kind().code(),
                        err.cause().unwrap_or("")
                    );
                    self.errore = Some(err.code().kind().i18n_key().to_owned());
                    *prendi(&self.comune.ultimo_errore) = Some(err);
                    self.riprova = Some(Instant::now() + RIPROVA_DOPO);
                }
            }
        } else if !voluto && self.comune.nodo().is_some() {
            nota!("[lontano] il nodo si spegne");
            self.spegni();
        }
        if let (Some(nodo), Some(letto)) = (self.comune.nodo(), &self.letto) {
            // Chi seguire, solo da una lettura riuscita: vedi
            // `tieni_la_lettura`.
            if riuscita {
                nodo.pari(
                    letto
                        .dispositivi
                        .iter()
                        .filter(|d| !d.revocato)
                        .map(|d| PariNoto {
                            chiave: d.chiave.clone(),
                            indirizzi: d.indirizzi.iter().filter_map(|a| a.parse().ok()).collect(),
                        })
                        .collect(),
                );
            }
            nodo.presenza(self.presenza());
        }
        self.togli_il_qr_finito();
        self.pubblica();
    }

    /// Il telefono è in uso, o lo è stato da poco. Altrove sempre.
    #[cfg(target_os = "android")]
    fn in_uso(&mut self) -> bool {
        if rymembra_android::sistema::in_uso().unwrap_or(true) || self.comune.in_volo() {
            self.in_uso = Instant::now();
        }
        self.in_uso.elapsed() < SVEGLIO_DOPO
    }

    #[cfg(not(target_os = "android"))]
    #[expect(
        clippy::unused_self,
        reason = "la stessa forma della versione per il telefono"
    )]
    fn in_uso(&self) -> bool {
        true
    }

    fn accendi(&mut self, relay: Option<String>) -> Result<(), AppError> {
        let dispositivo = in_libreria(&self.app, ATTESA_LIBRERIA, crate::nuvola::dispositivo)?;
        let identita = identita::identita(&rymembra_oauth::portachiavi::DiSistema, &dispositivo)?;
        let io = Presentazione {
            nome: self
                .letto
                .as_ref()
                .map_or_else(|| "Rymembra".to_owned(), |l| l.nome.clone()),
            piattaforma: PIATTAFORMA.to_owned(),
            chiave_insieme: crate::insieme::chiave_pubblica(&dispositivo),
            dispositivo,
        };
        let rete = Rete::Internet {
            relay: relay.clone().map_or(Relay::N0, Relay::Proprio),
            dht: true,
            // In casa con mDNS solo dove la rete di casa è concessa: su
            // Android 17 senza il permesso si fermerebbe nel sistema.
            mdns: crate::telefono::rete_locale_concessa(),
        };
        let ospite = Arc::new(ospite::Ospite::nuovo(
            self.app.clone(),
            Arc::clone(&self.comune),
        ));
        let nodo = Nodo::avvia(
            Impostazioni {
                identita,
                rete,
                io,
                presenza: self.presenza(),
            },
            ospite,
        )?;
        nota!(
            "[lontano] nodo acceso, {} dispositivi, relay {}",
            self.letto.as_ref().map_or(0, |l| l.dispositivi.len()),
            if relay.is_some() { "proprio" } else { "n0" }
        );
        *prendi(&self.comune.nodo) = Some(Arc::new(nodo));
        self.acceso_con = Some(relay);
        Ok(())
    }

    /// Spegne il nodo, e con lui il QR e le domande in attesa (che diventano
    /// un no).
    fn spegni(&mut self) {
        let nodo = prendi(&self.comune.nodo).take();
        prendi(&self.comune.richieste).clear();
        *prendi(&self.comune.invito) = None;
        prendi(&self.comune.presenze).clear();
        self.in_forse.clear();
        self.vie.clear();
        self.acceso_con = None;
        if let Some(nodo) = nodo {
            lascia_il_nodo(nodo);
        }
        self.pubblica();
    }

    /// Cosa questo dispositivo dice di sé.
    fn presenza(&self) -> Presenza {
        Presenza {
            nome: self
                .letto
                .as_ref()
                .map_or_else(|| "Rymembra".to_owned(), |l| l.nome.clone()),
            piattaforma: PIATTAFORMA.to_owned(),
            versione: env!("CARGO_PKG_VERSION").to_owned(),
            insieme_acceso: self.letto.as_ref().is_some_and(|l| l.insieme_acceso),
            insieme_ospita: matches!(
                crate::insieme::ruolo(&self.app),
                crate::insieme::Ruolo::Host
            ),
        }
    }

    fn evento(&mut self, evento: Evento) {
        match evento {
            Evento::Presente { chiave, presenza } => {
                // Tornato prima della fine della grazia: non se n'era andato.
                self.in_forse.remove(&chiave);
                let nuovo = prendi(&self.comune.presenze)
                    .insert(chiave.clone(), presenza)
                    .is_none();
                // Appena risponde, gli si mandano gli ascolti rimasti in fila.
                if nuovo {
                    super::sfoglia::manda_gli_ascolti(&self.app, &self.comune, Some(&chiave));
                }
                let da_scrivere = self
                    .visti
                    .get(&chiave)
                    .is_none_or(|quando| quando.elapsed() >= SCRIVI_VISTO_OGNI);
                if da_scrivere {
                    self.visti.insert(chiave.clone(), Instant::now());
                    let indirizzi: Vec<String> = self
                        .comune
                        .nodo()
                        .map(|n| n.indirizzi_di(&chiave))
                        .unwrap_or_default()
                        .iter()
                        .map(ToString::to_string)
                        .collect();
                    let adesso = adesso_ms();
                    match in_libreria(&self.app, ATTESA_LIBRERIA, |c| {
                        dispositivi::visto(c, &chiave, adesso, &indirizzi)
                    }) {
                        Ok(()) => {
                            if let Some(letto) = &mut self.letto
                                && let Some(d) =
                                    letto.dispositivi.iter_mut().find(|d| d.chiave == chiave)
                            {
                                d.visto_ms = Some(adesso);
                            }
                        }
                        Err(err) => nota!(
                            "[lontano] «visto» non si scrive codice={}",
                            err.code().kind().code()
                        ),
                    }
                }
                self.pubblica();
            }
            Evento::Assente { chiave } => {
                // Resta in vista per la grazia: vedi [`GRAZIA`].
                if prendi(&self.comune.presenze).contains_key(&chiave) {
                    self.in_forse.insert(chiave, Instant::now() + GRAZIA);
                }
            }
            Evento::InvitoFinito => {
                self.togli_il_qr_finito();
                self.pubblica();
            }
            Evento::Diario(testo) => nota!("[lontano] {testo}"),
        }
    }

    /// Compone il pannello e lo manda, se è cambiato.
    fn pubblica(&mut self) {
        let presenze = prendi(&self.comune.presenze).clone();
        let nodo = self.comune.nodo();
        let vie = self.guarda_le_vie(nodo.as_deref(), &presenze);
        let ipc = StatoLontanoIpc {
            attivo: self.letto.as_ref().is_some_and(|l| l.impostazioni.attivo),
            acceso: nodo.is_some(),
            chiave: nodo.as_ref().map(|n| n.chiave()),
            impronta: nodo.as_ref().map(|n| n.impronta()),
            relay: self
                .letto
                .as_ref()
                .and_then(|l| l.impostazioni.relay.clone()),
            dati_mobili: self
                .letto
                .as_ref()
                .map(|l| l.impostazioni.dati_mobili)
                .unwrap_or_default(),
            errore: self.errore.clone(),
            dispositivi: self
                .letto
                .as_ref()
                .map(|l| {
                    l.dispositivi
                        .iter()
                        .map(|d| {
                            let presenza = presenze.get(&d.chiave);
                            DispositivoIpc {
                                chiave: d.chiave.clone(),
                                nome: d.nome.clone(),
                                piattaforma: d.piattaforma.clone(),
                                dispositivo: d.dispositivo.clone(),
                                online: presenza.is_some(),
                                visto_ms: d.visto_ms,
                                associato_ms: d.associato_ms,
                                revocato: d.revocato,
                                versione: presenza.map(|p| p.versione.clone()),
                                insieme_acceso: presenza.is_some_and(|p| p.insieme_acceso),
                                insieme_ospita: presenza.is_some_and(|p| p.insieme_ospita),
                                insieme: d.chiave_insieme.as_ref().and_then(|k| {
                                    l.pari_insieme.get(&k.to_ascii_lowercase()).copied()
                                }),
                                via: vie.get(&d.chiave).map(|via| {
                                    match via {
                                        Via::Diretta => "diretto",
                                        Via::Relay => "relay",
                                    }
                                    .to_owned()
                                }),
                            }
                        })
                        .collect()
                })
                .unwrap_or_default(),
            invito: prendi(&self.comune.invito).clone(),
            richieste: prendi(&self.comune.richieste)
                .iter()
                .map(|(r, _)| r.clone())
                .collect(),
        };
        let ospitano: Vec<(String, String)> = self
            .letto
            .as_ref()
            .map(|l| {
                l.dispositivi
                    .iter()
                    .filter(|d| !d.revocato)
                    .filter(|d| presenze.get(&d.chiave).is_some_and(|p| p.insieme_ospita))
                    .filter_map(|d| Some((d.chiave.clone(), d.chiave_insieme.clone()?)))
                    .collect()
            })
            .unwrap_or_default();
        let cambiati = {
            let mut prima = prendi(&self.comune.ospitano);
            let cambiati = *prima != ospitano;
            *prima = ospitano;
            cambiati
        };
        // Un dispositivo che comincia a ospitare si vede subito fra i vicini di
        // «Ascolta insieme», non alla prossima scoperta.
        if cambiati {
            crate::insieme::vicini_cambiati(&self.app);
        }
        let json = serde_json::to_string(&ipc).unwrap_or_default();
        *prendi(&self.comune.vetrina) = ipc.clone();
        if self.pubblicata.as_deref() != Some(json.as_str()) {
            self.pubblicata = Some(json);
            self.app.emetti("lontano:stato", ipc);
        }
    }

    /// Da dove passa la connessione con ogni dispositivo che risponde. Nel
    /// diario quando cambia — il primo contatto passa spesso dal relay, poi
    /// il NAT si buca — senza indirizzi né chiavi: la piattaforma e il tempo
    /// di andata e ritorno bastano a leggere una prova.
    fn guarda_le_vie(
        &mut self,
        nodo: Option<&Nodo>,
        presenze: &HashMap<String, Presenza>,
    ) -> HashMap<String, Via> {
        let mut vie = HashMap::new();
        if let Some(nodo) = nodo {
            for (chiave, presenza) in presenze {
                let Some((via, rtt)) = nodo.via(chiave) else {
                    continue;
                };
                if self.vie.get(chiave) != Some(&via) {
                    nota!(
                        "[lontano] un dispositivo ({}) passa {}, rtt={} ms",
                        presenza
                            .piattaforma
                            .chars()
                            .filter(char::is_ascii_alphanumeric)
                            .take(16)
                            .collect::<String>(),
                        match via {
                            Via::Diretta => "diritto",
                            Via::Relay => "dal relay",
                        },
                        rtt.as_millis()
                    );
                }
                vie.insert(chiave.clone(), via);
            }
        }
        self.vie.clone_from(&vie);
        vie
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Una chiave di dispositivo finta: 64 cifre esadecimali.
    fn chiave_finta(cifra: char) -> String {
        std::iter::repeat_n(cifra, 64).collect()
    }

    fn collega(c: &rusqlite::Connection, chiave: &str) {
        dispositivi::associa(
            c,
            dispositivi::Nuovo {
                chiave,
                nome: "Telefono",
                piattaforma: "android",
                dispositivo: "telefono-di-prova",
                chiave_insieme: None,
                indirizzi: &[],
            },
            0,
        )
        .unwrap();
    }

    fn occupata() -> Result<Letto, AppError> {
        Err(AppError::new(ErrorCode::DbLocked).with_cause("una scansione tiene la libreria"))
    }

    #[test]
    fn con_la_libreria_occupata_il_cancello_resta_com_e() {
        // Reperto 02-8: con la rilettura che non prendeva la libreria, il
        // cancello si rifaceva dalla lettura di prima. Un dispositivo appena
        // dimenticato tornava dentro (e `pari()` lo tornava a seguire) fino
        // alla lettura riuscita dopo; uno appena collegato restava fuori.
        let db = rymembra_app::db::open_in_memory().unwrap();
        let c = &db.connection;
        let perso = chiave_finta('a');
        collega(c, &perso);
        let (posta, _cassetta) = std::sync::mpsc::channel();
        let comune = Comune::nuovo(posta);
        let mut letto = None;
        assert!(tieni_la_lettura(&comune, &mut letto, leggi(c)));
        assert!(comune.associato(&perso));

        // «Dimentica»: fuori subito, e la riga se ne va.
        comune.ammetti(&perso, false);
        dispositivi::dimentica(c, &perso).unwrap();
        assert!(!tieni_la_lettura(&comune, &mut letto, occupata()));
        assert!(
            !comune.associato(&perso),
            "il dispositivo dimenticato resta fuori"
        );

        // Un collegamento appena fatto: dentro subito, e ci resta.
        let nuovo = chiave_finta('b');
        collega(c, &nuovo);
        comune.ammetti(&nuovo, true);
        assert!(!tieni_la_lettura(&comune, &mut letto, occupata()));
        assert!(comune.associato(&nuovo), "il nuovo non resta fuori");

        // La lettura che riesce dice com'è la libreria.
        assert!(tieni_la_lettura(&comune, &mut letto, leggi(c)));
        assert!(!comune.associato(&perso));
        assert!(comune.associato(&nuovo));
    }

    /// Un'app che non conosce nessuno.
    struct Nessuno;

    impl rymembra_lontano::Ospite for Nessuno {
        fn associato(&self, _chiave: &str) -> bool {
            false
        }

        fn conferma(&self, _chiave: &str, _chi: &Presentazione) -> bool {
            false
        }

        fn nuovo_pari(
            &self,
            _chiave: &str,
            _chi: &Presentazione,
            _indirizzi: &[std::net::SocketAddr],
        ) {
        }

        fn rispondi(
            &self,
            _da: &str,
            _domanda: rymembra_lontano::Domanda,
        ) -> Result<rymembra_lontano::Risposta, AppError> {
            Err(AppError::new(ErrorCode::LontanoNonAssociato))
        }

        fn dimenticato(&self, _da: &str) {}

        fn evento(&self, _evento: Evento) {}
    }

    #[test]
    fn spento_qui_il_nodo_si_chiude_anche_se_un_comando_lo_tiene() {
        // Reperto 02-7: spento «Raggiungibile da internet», il nodo si
        // fermava solo se nessun comando lo teneva. Un `collega` in attesa del
        // sì (fino a quattro minuti), o una lettura di «Sfoglia» in volo, lo
        // teneva acceso e raggiungibile — col QR ancora valido — finché non
        // finiva.
        let nodo = Arc::new(
            Nodo::avvia(
                Impostazioni {
                    identita: rymembra_lontano::Identita::nuova(),
                    rete: Rete::SoloQuesta,
                    io: Presentazione {
                        nome: "PC".to_owned(),
                        piattaforma: "windows".to_owned(),
                        dispositivo: "pc-di-prova".to_owned(),
                        chiave_insieme: None,
                    },
                    presenza: Presenza::default(),
                },
                Arc::new(Nessuno),
            )
            .unwrap(),
        );
        nodo.invita().unwrap();
        assert!(nodo.invito_attivo());
        // Il comando in volo.
        let tenuto = Arc::clone(&nodo);
        lascia_il_nodo(nodo);
        assert!(!tenuto.invito_attivo(), "il QR non vale più");
        let altro = rymembra_lontano::Identita::nuova().chiave();
        assert_eq!(
            tenuto
                .chiedi(
                    &altro,
                    rymembra_lontano::Domanda::Presenza,
                    Duration::from_secs(2)
                )
                .unwrap_err()
                .code()
                .kind(),
            rymembra_domain::errors::ErrorCodeKind::LontanoSpento,
            "chi lo tiene ancora lo trova spento"
        );
    }
}
