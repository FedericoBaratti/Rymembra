//! Il filo del collegamento: accende e spegne il nodo, gli dice chi seguire e
//! cosa dire di sé, e tiene il pannello.
//!
//! # Quando il nodo è acceso
//!
//! Con «Raggiungibile da internet» acceso, e:
//!
//! - su Windows sempre, finché gira Rymembra (anche dal vassoio);
//! - sul telefono con «Resta attiva in sottofondo» acceso (`daemon.attivo`)
//!   sempre, finché gira il servizio in primo piano che lo tiene vivo
//!   (`Presenza.kt`): è il punto, che i PC lo trovino quando lo cercano. Costa
//!   batteria — il relay di iroh vuole i suoi keepalive —, e per questo è una
//!   scelta di chi lo usa e non un'abitudine dell'app;
//! - sul telefono senza, mentre lo si usa — l'attività è in vista, o la musica
//!   suona — e per [`SVEGLIO_DOPO`] dopo. Poi si spegne: un telefono in tasca
//!   non tiene aperta una porta per niente, e la batteria ringrazia.
//!
//! Il sottofondo da solo non accende niente verso internet: serve anche
//! «Raggiungibile da internet», che è un sì a parte.
//!
//! # Chi scrive cosa
//!
//! Il filo è il solo a comporre il pannello e a mandarlo alla finestra. I
//! comandi e l'[`super::ospite::Ospite`] toccano [`Comune`] e lo svegliano con
//! la [`Posta`]; lui rilegge la libreria quando serve, e pubblica se qualcosa
//! è cambiato davvero.
//!
//! # Il lucchetto della libreria
//!
//! Il filo **non lo chiede**: legge da una connessione di sola lettura sua
//! ([`leggi_a_parte`]), come le domande degli altri dispositivi
//! (`servizio::leggi_da`). Lo chiedeva con un'attesa di due secondi, e i
//! lavori di sottofondo che lo prendono a turno — una prima analisi dura ore —
//! glielo facevano perdere: il giro non leggeva, il nodo non si accendeva, e
//! il telefono tornato in vista restava irraggiungibile per tutta l'analisi
//! (reperto R01-1). Scrivono, aspettando il loro turno come ogni comando, i
//! comandi ([`scrivi_in_libreria`]) e lo scrittore del registro.
//!
//! # Il registro delle modifiche
//!
//! «Dimentica», un «Dimenticami» ricevuto e un collegamento nuovo cambiano il
//! cancello subito, e restano nel registro finché una lettura della libreria
//! non le vede scritte: vedi [`Comune::registra`].

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

use rymembra_app::dispositivi::{self, Dispositivo, Nuovo};
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_lontano::protocollo::{CAPACITA_CATALOGO, CAPACITA_TESTI};
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

/// Quanto si aspetta la libreria dove si può rinunciare — «visto», gli ascolti
/// da mandare — e quanto aspetta SQLite una lettura a parte. Il filo non ha
/// fretta: occupata, si riprova al giro dopo.
pub(super) const ATTESA_LIBRERIA: Duration = Duration::from_secs(2);

/// Quanto si aspetta la libreria uscendo, per scrivere il registro: un
/// «Dimentica» che non arriva nella libreria tornerebbe dentro al prossimo
/// avvio.
const ATTESA_ULTIMA_SCRITTURA: Duration = Duration::from_secs(2);

/// Dopo un'accensione non riuscita, quanto prima di riprovare da sé.
const RIPROVA_DOPO: Duration = Duration::from_secs(60);

/// Quanto spesso si scrive nella libreria che un dispositivo risponde.
const SCRIVI_VISTO_OGNI: Duration = Duration::from_secs(60);

/// Quanto un dispositivo che non risponde più resta in vista prima di dirlo
/// assente: una connessione che cade e si riapre — il Wi-Fi che passa al 5G,
/// due chiamate incrociate — non deve far sparire la pagina «Sfoglia» per un
/// secondo.
const GRAZIA: Duration = Duration::from_secs(8);

/// Il telefono resta raggiungibile tanto dopo l'ultimo uso, se il sottofondo
/// è spento.
#[cfg_attr(
    not(target_os = "android"),
    allow(
        dead_code,
        reason = "la regola del telefono, che le prove vedono ovunque"
    )
)]
const SVEGLIO_DOPO: Duration = Duration::from_secs(2 * 60);

/// Il telefono deve restare raggiungibile: col sottofondo acceso sempre; senza,
/// finché lo si usa e per [`SVEGLIO_DOPO`] dopo. `fermo_da` è il tempo
/// dall'ultimo uso. Una funzione sola, perché la regola si prova anche dove
/// il telefono non c'è.
#[cfg_attr(
    not(target_os = "android"),
    allow(
        dead_code,
        reason = "la regola del telefono, che le prove vedono ovunque"
    )
)]
fn resta_raggiungibile(sottofondo: bool, in_uso_adesso: bool, fermo_da: Duration) -> bool {
    sottofondo || in_uso_adesso || fermo_da < SVEGLIO_DOPO
}

/// Chi risponde adesso e si fida, per nome, come lo dice la notifica del
/// sottofondo («collegata a <PC>»). Nell'ordine del pannello.
#[cfg_attr(
    not(target_os = "android"),
    allow(dead_code, reason = "lo dice la notifica del telefono")
)]
fn nomi_collegati(dispositivi: &[DispositivoIpc]) -> Vec<String> {
    dispositivi
        .iter()
        .filter(|d| d.online && !d.revocato)
        .map(|d| d.nome.clone())
        .collect()
}

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
    /// Cresce quando il QR si annulla da fuori della scheda che lo mostra — la
    /// finestra nel vassoio (decisione D-30): un `lontano_qr_crea` in volo se
    /// ne accorge prima di mettere il suo QR a schermo.
    pub qr_annullati: AtomicU64,
    /// Il registro: le modifiche al cancello fatte qui e che una lettura della
    /// libreria non ha ancora visto scritte, per chiave. Vedi
    /// [`Comune::registra`].
    sospese: Mutex<HashMap<String, Sospesa>>,
    /// Il numero della prossima modifica del registro.
    prossima_modifica: AtomicU64,
    /// Quante scritture del registro sono arrivate nella libreria: chi legge lo
    /// guarda **prima** di leggere, vedi [`tieni_la_lettura`].
    scritture: AtomicU64,
    /// Uno scrittore del registro è in giro ([`scrivi_il_registro`]).
    scrittore: AtomicBool,
    /// «Spegni» sulla notifica del sottofondo è stato premuto, e la libreria
    /// non ha ancora la scelta scritta: finché non c'è, il sottofondo conta
    /// spento, e non lo rimette in piedi la lettura di prima. Lo toglie chi
    /// lo scrive, o l'interruttore che la persona riaccende (la scelta più
    /// recente vince). Solo il telefono lo alza.
    pub spegnere: AtomicBool,
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
            qr_annullati: AtomicU64::new(0),
            sospese: Mutex::new(HashMap::new()),
            prossima_modifica: AtomicU64::new(1),
            scritture: AtomicU64::new(0),
            scrittore: AtomicBool::new(false),
            spegnere: AtomicBool::new(false),
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

    /// Solo il cancello, per le prove che non c'entrano col registro: nel
    /// codice vero il cancello cambia con [`Comune::registra`] o da una
    /// lettura.
    #[cfg(test)]
    pub(super) fn ammetti(&self, chiave: &str, dentro: bool) {
        let mut associati = self
            .associati
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        if dentro {
            associati.insert(chiave_di(chiave));
        } else {
            associati.remove(&chiave_di(chiave));
        }
    }

    /// Una modifica al cancello fatta qui: entra **adesso** — una chiave
    /// collegata entra subito (la connessione che segue l'associazione non
    /// deve trovarlo chiuso), una dimenticata esce subito — e resta nel
    /// registro finché una lettura della libreria non la vede scritta
    /// (decisione D-24). La scrive [`scrivi_il_registro`], appena la libreria
    /// è libera.
    ///
    /// Fino al reperto R01-2 cambiava solo il cancello in memoria, e la
    /// scrittura che non prendeva la libreria in due secondi — una scansione
    /// in corso — si perdeva: la prima lettura riuscita rimetteva dentro il
    /// dispositivo dimenticato, il nodo lo tornava a seguire, e la sua chiave
    /// di «Ascolta insieme» restava fidata, finché la persona non rifaceva il
    /// gesto. Ora una lettura non la disfa ([`tieni_la_lettura`]): ci passa
    /// sopra.
    pub(super) fn registra(&self, chiave: &str, modifica: Modifica) {
        let chiave = chiave_di(chiave);
        let dentro = matches!(modifica, Modifica::Associa(_));
        // Registro e cancello insieme, nell'ordine di `tieni_la_lettura`: una
        // lettura che rifà il cancello non passa in mezzo.
        let mut sospese = prendi(&self.sospese);
        let numero = self.prossima_modifica.fetch_add(1, Ordering::Relaxed);
        sospese.insert(
            chiave.clone(),
            Sospesa {
                modifica,
                numero,
                scritta: None,
            },
        );
        let mut associati = self
            .associati
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        if dentro {
            associati.insert(chiave);
        } else {
            associati.remove(&chiave);
        }
    }

    /// Le modifiche del registro che la libreria non ha ancora, col loro
    /// numero.
    fn da_scrivere(&self) -> Vec<(String, u64, Modifica)> {
        prendi(&self.sospese)
            .iter()
            .filter(|(_, sospesa)| sospesa.scritta.is_none())
            .map(|(chiave, sospesa)| (chiave.clone(), sospesa.numero, sospesa.modifica.clone()))
            .collect()
    }

    /// Queste modifiche sono nella libreria, a transazione chiusa: una
    /// lettura che comincia da qui in poi le vede. Una arrivata dopo per la
    /// stessa chiave (un altro numero) resta da scrivere.
    fn segna_scritte(&self, fatte: &[(String, u64)]) {
        let mut sospese = prendi(&self.sospese);
        let scrittura = self
            .scritture
            .fetch_add(1, Ordering::SeqCst)
            .saturating_add(1);
        for (chiave, numero) in fatte {
            if let Some(sospesa) = sospese.get_mut(chiave)
                && sospesa.numero == *numero
            {
                sospesa.scritta = Some(scrittura);
            }
        }
    }

    /// Il numero dell'ultima scrittura del registro arrivata nella libreria:
    /// chi legge lo prende prima di cominciare.
    fn scritture_viste(&self) -> u64 {
        self.scritture.load(Ordering::SeqCst)
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

/// La chiave di un dispositivo come la scrive la libreria
/// (`rymembra_app::dispositivi`): senza spazi, minuscola.
fn chiave_di(chiave: &str) -> String {
    chiave.trim().to_ascii_lowercase()
}

/// Lo stato della libreria; senza, `internal.aborted`.
fn lo_stato(app: &AppHandle) -> Result<tauri::State<'_, Stato>, AppError> {
    app.try_state::<Stato>().ok_or_else(|| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some("lontano".to_owned()),
        })
    })
}

/// La libreria, aspettata al più `entro`. Non presa in tempo è `db.locked`.
///
/// Solo per chi può rinunciare e riprovare: «visto», gli ascolti da mandare.
/// Le letture del filo passano da [`leggi_a_parte`], le scritture dei comandi
/// da [`scrivi_in_libreria`]: vedi il `//!`.
pub(super) fn in_libreria<T>(
    app: &AppHandle,
    entro: Duration,
    azione: impl FnOnce(&rusqlite::Connection) -> Result<T, AppError>,
) -> Result<T, AppError> {
    crate::stato::con_libreria_entro(&*lo_stato(app)?, entro, |l| azione(&l.connection))
        .unwrap_or_else(|| {
            Err(AppError::new(ErrorCode::DbLocked)
                .with_cause("la libreria è occupata: il collegamento non l'aspetta"))
        })
}

/// Una lettura della libreria senza il suo lucchetto: da una connessione di
/// sola lettura (`crate::stato::leggi_a_parte`), che in WAL non ferma chi
/// scrive e non ne è fermata (decisione D-25). Fuori dal WAL torna al
/// lucchetto, con [`ATTESA_LIBRERIA`].
pub(super) fn leggi_a_parte<T>(
    app: &AppHandle,
    azione: impl FnOnce(&rusqlite::Connection) -> Result<T, AppError>,
) -> Result<T, AppError> {
    leggi_da(&*lo_stato(app)?, azione)
}

/// [`leggi_a_parte`], sullo stato: la cucitura delle prove.
fn leggi_da<T>(
    stato: &Stato,
    azione: impl FnOnce(&rusqlite::Connection) -> Result<T, AppError>,
) -> Result<T, AppError> {
    crate::stato::leggi_a_parte(stato, ATTESA_LIBRERIA, azione).unwrap_or_else(|| {
        Err(AppError::new(ErrorCode::DbLocked)
            .with_cause("la libreria è occupata, e senza WAL il collegamento non l'aspetta"))
    })
}

/// Una scrittura di un comando: aspetta il suo turno col lucchetto, come ogni
/// comando sul pool bloccante. Prima rinunciava dopo due secondi, e con i
/// lavori di sottofondo in corso «Raggiungibile da internet», il relay, «Solo
/// Wi-Fi» e «Rinomina» rispondevano «il database è occupato» (reperto R01-1).
pub(super) fn scrivi_in_libreria<T>(
    app: &AppHandle,
    azione: impl FnOnce(&rusqlite::Connection) -> Result<T, AppError>,
) -> Result<T, AppError> {
    crate::stato::con_libreria(&*lo_stato(app)?, |l| azione(&l.connection))
}

/// L'identificativo di questo dispositivo nella sincronia, per accendere il
/// nodo: quasi sempre c'è già, e si legge a parte. La prima volta va scritto,
/// e la lettura a parte non scrive: allora si aspetta il proprio turno.
fn dispositivo_di(stato: &Stato) -> Result<String, AppError> {
    leggi_da(stato, crate::nuvola::dispositivo).or_else(|_| {
        crate::stato::con_libreria(stato, |l| crate::nuvola::dispositivo(&l.connection))
    })
}

// ── il registro ─────────────────────────────────────────────────────────────

/// Una modifica al cancello fatta qui: vedi [`Comune::registra`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Modifica {
    /// «Dimentica», da qui: la riga se ne va, e con lei la fiducia che il QR
    /// aveva dato in «Ascolta insieme» (`dispositivi::dimentica`).
    Dimentica,
    /// L'altro ci ha dimenticati: la riga resta, revocata
    /// (`dispositivi::revoca`).
    Revoca,
    /// Un collegamento appena fatto.
    Associa(Collegato),
}

/// Un dispositivo appena collegato, come si è presentato: la riga da scrivere,
/// e da mostrare finché la libreria non ce l'ha.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Collegato {
    /// Il nome che dichiara, com'è arrivato: lo ripulisce chi lo scrive.
    pub nome: String,
    /// La piattaforma che dichiara.
    pub piattaforma: String,
    /// L'identificativo nella sincronia.
    pub dispositivo: String,
    /// La chiave di «Ascolta insieme», se l'ha data.
    pub chiave_insieme: Option<String>,
    /// Gli indirizzi da cui si è presentato.
    pub indirizzi: Vec<String>,
    /// Quando.
    pub associato_ms: i64,
}

impl Collegato {
    /// La riga come la scriverà `dispositivi::associa`, con le sue stesse
    /// pulizie: per il pannello, per chi seguire e per il cancello, finché una
    /// lettura non porta quella vera. Senza `id`, che qui nessuno usa.
    fn come_riga(&self, chiave: &str) -> Dispositivo {
        let mut nome = rymembra_app::insieme::ripulisci_nome(&self.nome);
        if nome.is_empty() {
            nome = chiave.chars().take(8).collect();
        }
        Dispositivo {
            id: 0,
            chiave: chiave.to_owned(),
            nome,
            piattaforma: self
                .piattaforma
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .take(16)
                .collect(),
            dispositivo: self
                .dispositivo
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                .take(64)
                .collect(),
            chiave_insieme: self
                .chiave_insieme
                .as_deref()
                .map(chiave_di)
                .filter(|c| c.len() == 64 && c.bytes().all(|b| b.is_ascii_hexdigit())),
            indirizzi: self
                .indirizzi
                .iter()
                .filter(|a| a.parse::<std::net::SocketAddr>().is_ok())
                .take(dispositivi::MASSIMO_INDIRIZZI)
                .cloned()
                .collect(),
            associato_ms: self.associato_ms,
            visto_ms: Some(self.associato_ms),
            revocato: false,
        }
    }
}

/// Una voce del registro.
#[derive(Debug)]
struct Sospesa {
    modifica: Modifica,
    /// Il suo numero: lo scrittore segna scritta la modifica che ha scritto,
    /// non una arrivata dopo per la stessa chiave.
    numero: u64,
    /// La scrittura che l'ha portata nella libreria ([`Comune::scritture`]),
    /// quando c'è.
    scritta: Option<u64>,
}

/// Una modifica del registro sopra una lettura.
fn applica(dispositivi: &mut Vec<Dispositivo>, chiave: &str, modifica: &Modifica) {
    match modifica {
        Modifica::Dimentica => dispositivi.retain(|d| chiave_di(&d.chiave) != chiave),
        Modifica::Revoca => {
            for d in dispositivi
                .iter_mut()
                .filter(|d| chiave_di(&d.chiave) == chiave)
            {
                d.revocato = true;
            }
        }
        Modifica::Associa(collegato) => {
            let riga = collegato.come_riga(chiave);
            match dispositivi
                .iter_mut()
                .find(|d| chiave_di(&d.chiave) == chiave)
            {
                Some(d) => {
                    *d = Dispositivo {
                        id: d.id,
                        visto_ms: d.visto_ms.max(riga.visto_ms),
                        ..riga
                    };
                }
                // Il più recente: in cima, come `dispositivi::tutti`.
                None => dispositivi.insert(0, riga),
            }
        }
    }
}

/// Quel che una passata dello scrittore ha fatto.
#[derive(Debug, Default)]
struct Scritte {
    /// Le modifiche scritte, col loro numero.
    fatte: Vec<(String, u64)>,
    /// La fiducia di «Ascolta insieme» è cambiata: la sua rete rilegge.
    insieme_cambiato: bool,
    /// Perché si è fermata, se si è fermata prima della fine.
    guasto: Option<AppError>,
}

/// Scrive le modifiche nella libreria, una alla volta, fino alla prima che
/// non riesce. Ognuna è una transazione sua (`dispositivi::dimentica` ne apre
/// una), e quelle fatte restano fatte. Scriverne una due volte non cambia
/// niente: dimenticare due volte è dimenticare una volta.
fn scrivi_su(c: &rusqlite::Connection, da_scrivere: &[(String, u64, Modifica)]) -> Scritte {
    let mut scritte = Scritte::default();
    for (chiave, numero, modifica) in da_scrivere {
        let esito = match modifica {
            Modifica::Dimentica => dispositivi::dimentica(c, chiave).and_then(|insieme| {
                // Le sue righe nella libreria unica se ne vanno con lui, dopo
                // aver passato le voci di playlist a un'altra copia.
                rymembra_app::specchio::archivio::dimentica_dispositivo(c, chiave)?;
                Ok(insieme)
            }),
            Modifica::Revoca => dispositivi::revoca(c, chiave).map(|()| false),
            Modifica::Associa(collegato) => associa(c, chiave, collegato),
        };
        match esito {
            Ok(cambiato) => {
                scritte.insieme_cambiato |= cambiato;
                scritte.fatte.push((chiave.clone(), *numero));
            }
            Err(err) => {
                scritte.guasto = Some(err);
                break;
            }
        }
    }
    scritte
}

/// Un collegamento nella libreria, e la fiducia di «Ascolta insieme» per la
/// chiave che il dispositivo ha dato col QR: chiederlo di nuovo in casa
/// sarebbe una seconda associazione degli stessi due dispositivi. Ma la chiave
/// la dichiara l'altro, e niente prova che sia sua: un PC che la persona aveva
/// bloccato resta bloccato, e uno che c'è già tiene il suo nome
/// (`rymembra_app::insieme::fidato_dal_qr`). «Dimentica» la toglie. Un guasto
/// lì non disfa il collegamento: in casa resta il codice. `true` se la fiducia
/// è cambiata.
fn associa(
    c: &rusqlite::Connection,
    chiave: &str,
    collegato: &Collegato,
) -> Result<bool, AppError> {
    dispositivi::associa(
        c,
        Nuovo {
            chiave,
            nome: &collegato.nome,
            piattaforma: &collegato.piattaforma,
            dispositivo: &collegato.dispositivo,
            chiave_insieme: collegato.chiave_insieme.as_deref(),
            indirizzi: &collegato.indirizzi,
        },
        collegato.associato_ms,
    )?;
    let Some(insieme) = collegato.chiave_insieme.as_deref() else {
        return Ok(false);
    };
    match rymembra_app::insieme::fidato_dal_qr(c, insieme, &collegato.nome, collegato.associato_ms)
    {
        Ok(Some(_)) => Ok(true),
        Ok(None) => {
            nota!(
                "[lontano] la chiave di «Ascolta insieme» del dispositivo collegato è di un PC bloccato: resta bloccato"
            );
            Ok(false)
        }
        Err(err) => {
            nota!(
                "[lontano] la chiave di «Ascolta insieme» non si scrive codice={}",
                err.code().kind().code()
            );
            Ok(false)
        }
    }
}

/// Scrive nella libreria il registro, su un filo suo — uno alla volta — che
/// aspetta il suo turno col lucchetto come ogni comando: una scansione lo fa
/// aspettare, non rinunciare. Finito, sveglia il filo, la cui lettura toglie
/// dal registro quel che vede scritto. Un guasto vero si dice nel diario, e si
/// ritenta al giro dopo ([`Filo::riconcilia`]). Non aspetta.
pub(super) fn scrivi_il_registro(app: &AppHandle, comune: &Arc<Comune>) {
    if comune.da_scrivere().is_empty() || comune.scrittore.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    let suo = Arc::clone(comune);
    let partito = std::thread::Builder::new()
        .name("rymembra-lontano-registro".to_owned())
        .spawn(move || scrivi_finche_ce_n_e(&app, &suo));
    if let Err(err) = partito {
        comune.scrittore.store(false, Ordering::Release);
        nota!("[lontano] lo scrittore dei dispositivi non parte: {err}");
    }
}

/// Il corpo dello scrittore di [`scrivi_il_registro`].
fn scrivi_finche_ce_n_e(app: &AppHandle, comune: &Comune) {
    loop {
        let da_scrivere = comune.da_scrivere();
        if da_scrivere.is_empty() {
            comune.scrittore.store(false, Ordering::Release);
            // Una modifica arrivata fra il conto e qui ha trovato lo scrittore
            // ancora in giro, e non ne ha fatto partire un altro: la si prende
            // adesso, se nessuno è partito nel frattempo.
            if comune.da_scrivere().is_empty() || comune.scrittore.swap(true, Ordering::AcqRel) {
                return;
            }
            continue;
        }
        let scritte = lo_stato(app)
            .and_then(|stato| {
                crate::stato::con_libreria(&stato, |l| Ok(scrivi_su(&l.connection, &da_scrivere)))
            })
            .unwrap_or_else(|err| Scritte {
                guasto: Some(err),
                ..Scritte::default()
            });
        comune.segna_scritte(&scritte.fatte);
        if scritte.insieme_cambiato {
            // La rete di «Ascolta insieme» rilegge, e chiude le connessioni
            // della chiave che non è più fidata.
            crate::insieme::fidati_cambiati(app);
        }
        if !scritte.fatte.is_empty() {
            comune.sveglia(Posta::Rileggi {
                risposta: None,
                subito: false,
            });
        }
        if let Some(err) = scritte.guasto {
            nota!(
                "[lontano] una modifica dei dispositivi non si scrive codice={}: si riprova al giro",
                err.code().kind().code()
            );
            comune.scrittore.store(false, Ordering::Release);
            return;
        }
    }
}

/// Il registro ancora da scrivere, uscendo: al più [`ATTESA_ULTIMA_SCRITTURA`],
/// e se la libreria non arriva lo si dice nel diario.
pub(super) fn scrivi_prima_di_uscire(app: &AppHandle, comune: &Comune) {
    let da_scrivere = comune.da_scrivere();
    if da_scrivere.is_empty() {
        return;
    }
    let Ok(stato) = lo_stato(app) else {
        return;
    };
    let esito = crate::stato::con_libreria_entro(&stato, ATTESA_ULTIMA_SCRITTURA, |l| {
        Ok(scrivi_su(&l.connection, &da_scrivere))
    });
    match esito {
        Some(Ok(scritte)) => {
            comune.segna_scritte(&scritte.fatte);
            if let Some(err) = scritte.guasto {
                nota!(
                    "[lontano] uscendo, una modifica dei dispositivi non si scrive codice={}",
                    err.code().kind().code()
                );
            }
        }
        Some(Err(err)) => nota!(
            "[lontano] uscendo, le modifiche dei dispositivi non si scrivono codice={}",
            err.code().kind().code()
        ),
        None => nota!(
            "[lontano] uscendo, {} modifiche dei dispositivi non si scrivono: la libreria è occupata",
            da_scrivere.len()
        ),
    }
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

/// Tiene la lettura appena fatta, ci mette sopra il registro, e da lì rifà il
/// cancello. `true` se la lettura è riuscita. `vista` è
/// [`Comune::scritture_viste`] preso **prima** di leggere.
///
/// Il registro sopra la lettura, sempre: dispositivi, cancello, chi il nodo
/// segue (`Nodo::pari`) e il pannello vengono tutti dallo stesso elenco. Una
/// modifica esce dal registro solo quando una lettura **cominciata dopo** che
/// era scritta riesce: una lettura presa un attimo prima — dalla connessione a
/// parte, che vede l'ultima transazione chiusa — non la disfa (reperto
/// R01-2). Da lì decide la libreria.
///
/// Con la lettura che non riesce (fuori dal WAL, la libreria occupata) si
/// tiene l'ultima, col registro sopra: lo stesso cancello che c'era, con le
/// modifiche fatte nel frattempo. Fino al reperto 02-8 lo si rifaceva dalla
/// lettura vecchia, senza le modifiche.
fn tieni_la_lettura(
    comune: &Comune,
    letto: &mut Option<Letto>,
    lettura: Result<Letto, AppError>,
    vista: u64,
) -> bool {
    let riuscita = match lettura {
        Ok(nuova) => {
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
    };
    let mut sospese = prendi(&comune.sospese);
    if riuscita {
        sospese.retain(|_, sospesa| sospesa.scritta.is_none_or(|scrittura| scrittura > vista));
    }
    let Some(letto) = letto.as_mut() else {
        // Ancora nessuna lettura: il cancello è quello delle modifiche.
        return riuscita;
    };
    for (chiave, sospesa) in sospese.iter() {
        applica(&mut letto.dispositivi, chiave, &sospesa.modifica);
    }
    let mut associati = comune
        .associati
        .write()
        .unwrap_or_else(PoisonError::into_inner);
    *associati = letto
        .dispositivi
        .iter()
        .filter(|d| !d.revocato)
        .map(|d| chiave_di(&d.chiave))
        .collect();
    riuscita
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
    /// L'ultimo guasto del ponte nel dire al sistema se il sottofondo è
    /// voluto: nel diario una volta, non a ogni giro.
    #[cfg(target_os = "android")]
    guasto_sottofondo: Option<String>,
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
            #[cfg(target_os = "android")]
            guasto_sottofondo: None,
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
        // «Spegni» sulla notifica va nella libreria prima della lettura, che
        // così la trova scritta.
        #[cfg(target_os = "android")]
        self.scrivi_lo_spegnimento_della_notifica();
        // Prima di leggere: vedi `tieni_la_lettura`.
        let vista = self.comune.scritture_viste();
        let lettura = leggi_a_parte(&self.app, leggi);
        tieni_la_lettura(&self.comune, &mut self.letto, lettura, vista);
        // Quel che il registro non ha ancora scritto: un guasto di prima si
        // ritenta qui, a ogni giro.
        scrivi_il_registro(&self.app, &self.comune);
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
            // Chi seguire: dallo stesso elenco del cancello, la lettura col
            // registro sopra (vedi `tieni_la_lettura`).
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
            nodo.presenza(self.presenza());
        }
        self.togli_il_qr_finito();
        self.pubblica();
        // A ogni giro, anche senza novità: un servizio che il sistema ha
        // rifiutato dallo sfondo riparte appena l'app torna in vista, e uno
        // chiuso torna.
        #[cfg(target_os = "android")]
        self.dillo_al_servizio();
    }

    /// Il telefono è in uso, o lo è stato da poco — o ha il sottofondo acceso,
    /// e allora sempre. Altrove sempre.
    #[cfg(target_os = "android")]
    fn in_uso(&mut self) -> bool {
        let sottofondo = self.sottofondo_acceso();
        let adesso = rymembra_android::sistema::in_uso().unwrap_or(true) || self.comune.in_volo();
        // Anche col sottofondo si segna l'ora: spento, il nodo non si chiude
        // sotto i piedi di chi l'ha appena spento, ma `SVEGLIO_DOPO` dopo.
        if sottofondo || adesso {
            self.in_uso = Instant::now();
        }
        resta_raggiungibile(sottofondo, adesso, self.in_uso.elapsed())
    }

    /// «Resta attiva in sottofondo», com'è adesso: quel che la libreria dice,
    /// meno uno «Spegni» della notifica non ancora scritto.
    fn sottofondo_acceso(&self) -> bool {
        self.letto
            .as_ref()
            .is_some_and(|l| l.impostazioni.sottofondo)
            && !self.comune.spegnere.load(Ordering::Acquire)
    }

    /// «Spegni» sulla notifica del servizio: Kotlin ha già fermato il servizio e
    /// lo ricorda, e qui lo si scrive nelle impostazioni. La libreria si aspetta
    /// al più [`ATTESA_LIBRERIA`], come «visto»: occupata, `spegnere` tiene il
    /// sottofondo spento e si riprova al giro dopo.
    #[cfg(target_os = "android")]
    fn scrivi_lo_spegnimento_della_notifica(&self) {
        let spegnere = &self.comune.spegnere;
        if !spegnere.load(Ordering::Acquire)
            && rymembra_android::sistema::sottofondo_spento_dalla_notifica().unwrap_or(false)
        {
            nota!("[lontano] «Spegni» sulla notifica: il sottofondo si spegne");
            spegnere.store(true, Ordering::Release);
        }
        if !spegnere.load(Ordering::Acquire) {
            return;
        }
        match in_libreria(&self.app, ATTESA_LIBRERIA, |c| {
            dispositivi::sottofondo(c, false)
        }) {
            Ok(()) => spegnere.store(false, Ordering::Release),
            Err(err) => nota!(
                "[lontano] il sottofondo spento dalla notifica non si scrive codice={}: si riprova al giro",
                err.code().kind().code()
            ),
        }
    }

    /// Dice al servizio del telefono se il sottofondo è voluto e a chi si è
    /// collegati, a ogni pubblicazione: il servizio rifà la notifica solo se la
    /// frase cambia. Prima della prima lettura riuscita no: «spento» sarebbe
    /// un'ipotesi, e fermerebbe un servizio che la libreria vuole acceso.
    #[cfg(target_os = "android")]
    fn dillo_al_servizio(&mut self) {
        if self.letto.is_none() {
            return;
        }
        let acceso = self.sottofondo_acceso();
        let collegati = if acceso {
            nomi_collegati(&prendi(&self.comune.vetrina).dispositivi)
        } else {
            Vec::new()
        };
        match rymembra_android::sistema::sottofondo(acceso, &collegati, false) {
            Ok(()) => self.guasto_sottofondo = None,
            Err(err) => {
                let testo = err.to_string();
                if self.guasto_sottofondo.as_ref() != Some(&testo) {
                    nota!("[lontano] il servizio del sottofondo non risponde: {testo}");
                }
                self.guasto_sottofondo = Some(testo);
            }
        }
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
        let dispositivo = dispositivo_di(&*lo_stato(&self.app)?)?;
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
            // Sa servire e usare il catalogo (libreria unica) e il testo della
            // canzone di un brano dell'altro: le domande nuove non si fanno a
            // chi non lo dichiara.
            capacita: vec![CAPACITA_CATALOGO.to_owned(), CAPACITA_TESTI.to_owned()],
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
                    // E si riguarda il suo catalogo, con la posta che aspetta.
                    super::specchio::presente(&self.app, &chiave);
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
            sottofondo: self.sottofondo_acceso(),
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
            // Chi è collegato è cambiato, o il sottofondo: la notifica si rifà
            // adesso, non al giro dopo.
            #[cfg(target_os = "android")]
            self.dillo_al_servizio();
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
    fn col_sottofondo_acceso_il_telefono_non_va_a_dormire() {
        // Senza il sottofondo il nodo si spegne `SVEGLIO_DOPO` dopo l'ultimo
        // uso: un telefono in tasca non tiene aperta una porta per niente.
        // Col sottofondo, deciso da chi lo usa, no — è il punto: i PC lo
        // raggiungono sempre.
        let da_molto = SVEGLIO_DOPO * 1_000;
        assert!(resta_raggiungibile(true, false, da_molto));
        assert!(resta_raggiungibile(true, true, da_molto));
        // Spento, vale la regola di prima, alla soglia compresa.
        assert!(resta_raggiungibile(false, true, da_molto));
        assert!(resta_raggiungibile(
            false,
            false,
            SVEGLIO_DOPO - Duration::from_millis(1)
        ));
        assert!(!resta_raggiungibile(false, false, SVEGLIO_DOPO));
        assert!(!resta_raggiungibile(false, false, da_molto));
    }

    /// Un dispositivo del pannello, con quel che serve a «collegata a …».
    fn nel_pannello(nome: &str, online: bool, revocato: bool) -> DispositivoIpc {
        DispositivoIpc {
            chiave: chiave_finta('c'),
            nome: nome.to_owned(),
            piattaforma: "windows".to_owned(),
            dispositivo: "pc-di-prova".to_owned(),
            online,
            visto_ms: None,
            associato_ms: 0,
            revocato,
            versione: None,
            insieme_acceso: false,
            insieme_ospita: false,
            insieme: None,
            via: None,
        }
    }

    #[test]
    fn la_notifica_nomina_solo_chi_risponde_e_si_fida() {
        // «Rymembra è attiva · collegata a <PC>»: chi non risponde, o ci ha
        // revocati, non è «collegato», e la notifica non deve dirlo.
        let pannello = [
            nel_pannello("PC di casa", true, false),
            nel_pannello("Portatile", false, false),
            nel_pannello("Ufficio", true, true),
            nel_pannello("Studio", true, false),
        ];
        assert_eq!(
            nomi_collegati(&pannello),
            vec!["PC di casa".to_owned(), "Studio".to_owned()]
        );
        assert!(nomi_collegati(&[]).is_empty());
        assert!(nomi_collegati(&[nel_pannello("Spento", false, false)]).is_empty());
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
        assert!(tieni_la_lettura(&comune, &mut letto, leggi(c), 0));
        assert!(comune.associato(&perso));

        // «Dimentica»: fuori subito, e la riga se ne va.
        comune.registra(&perso, Modifica::Dimentica);
        dispositivi::dimentica(c, &perso).unwrap();
        assert!(!tieni_la_lettura(&comune, &mut letto, occupata(), 0));
        assert!(
            !comune.associato(&perso),
            "il dispositivo dimenticato resta fuori"
        );

        // Un collegamento appena fatto: dentro subito, e ci resta.
        let nuovo = chiave_finta('b');
        collega(c, &nuovo);
        comune.registra(&nuovo, Modifica::Associa(collegato("Telefono")));
        assert!(!tieni_la_lettura(&comune, &mut letto, occupata(), 0));
        assert!(comune.associato(&nuovo), "il nuovo non resta fuori");

        // La lettura che riesce dice com'è la libreria.
        assert!(tieni_la_lettura(&comune, &mut letto, leggi(c), 0));
        assert!(!comune.associato(&perso));
        assert!(comune.associato(&nuovo));
    }

    /// Un dispositivo appena collegato, come arriva dal nodo.
    fn collegato(nome: &str) -> Collegato {
        Collegato {
            nome: nome.to_owned(),
            piattaforma: "android".to_owned(),
            dispositivo: "telefono-di-prova".to_owned(),
            chiave_insieme: None,
            indirizzi: Vec::new(),
            associato_ms: 5,
        }
    }

    /// Lo scrittore del registro, sulla libreria della prova.
    fn scrivi_il_registro_su(c: &rusqlite::Connection, comune: &Comune) {
        let scritte = scrivi_su(c, &comune.da_scrivere());
        assert!(scritte.guasto.is_none(), "{:?}", scritte.guasto);
        comune.segna_scritte(&scritte.fatte);
    }

    #[test]
    fn una_revoca_non_scritta_non_si_disfa_alla_lettura_dopo() {
        // Reperto R01-2: «Dimentica» con la libreria occupata cambiava il
        // cancello solo in memoria; la scrittura non prendeva la libreria, e
        // la prima lettura riuscita rimetteva dentro il dispositivo
        // dimenticato — per sempre, se nessuno rifaceva il gesto.
        let db = rymembra_app::db::open_in_memory().unwrap();
        let c = &db.connection;
        let perso = chiave_finta('a');
        collega(c, &perso);
        let (posta, _cassetta) = std::sync::mpsc::channel();
        let comune = Comune::nuovo(posta);
        let mut letto = None;
        assert!(tieni_la_lettura(
            &comune,
            &mut letto,
            leggi(c),
            comune.scritture_viste()
        ));
        assert!(comune.associato(&perso));

        // Fuori in memoria; la riga è ancora nella libreria.
        comune.registra(&perso, Modifica::Dimentica);
        let presa_prima = leggi(c);
        let vista_prima = comune.scritture_viste();
        assert!(tieni_la_lettura(
            &comune,
            &mut letto,
            leggi(c),
            comune.scritture_viste()
        ));
        assert!(
            !comune.associato(&perso),
            "una lettura riuscita non rimette dentro un dispositivo dimenticato"
        );
        assert!(
            letto
                .as_ref()
                .is_some_and(|l| l.dispositivi.iter().all(|d| d.chiave != perso)),
            "né nel pannello, né fra chi il nodo segue"
        );

        // La libreria si libera, e lo scrittore la scrive.
        scrivi_il_registro_su(c, &comune);
        assert!(comune.da_scrivere().is_empty());
        assert!(dispositivi::uno(c, &perso).unwrap().is_none());
        // Una lettura presa prima della scrittura non la disfa.
        assert!(tieni_la_lettura(
            &comune,
            &mut letto,
            presa_prima,
            vista_prima
        ));
        assert!(!comune.associato(&perso), "una lettura vecchia, riuscita");
        // Una cominciata dopo la vede: da lì decide la libreria.
        assert!(tieni_la_lettura(
            &comune,
            &mut letto,
            leggi(c),
            comune.scritture_viste()
        ));
        assert!(!comune.associato(&perso));
        assert!(prendi(&comune.sospese).is_empty(), "il registro è vuoto");
    }

    #[test]
    fn un_collegamento_e_un_dimenticami_non_scritti_valgono_finche_la_libreria_non_li_ha() {
        // Decisione D-24, anche per un collegamento nuovo e per un
        // «Dimenticami» ricevuto: prima la scrittura rinunciava (dieci
        // secondi), e la prima lettura chiudeva fuori il dispositivo appena
        // accolto, o lasciava collegato per sempre chi ci aveva dimenticati.
        let db = rymembra_app::db::open_in_memory().unwrap();
        let c = &db.connection;
        let altro = chiave_finta('d');
        collega(c, &altro);
        let (posta, _cassetta) = std::sync::mpsc::channel();
        let comune = Comune::nuovo(posta);
        let mut letto = None;
        assert!(tieni_la_lettura(&comune, &mut letto, leggi(c), 0));

        // Un collegamento mentre una scansione tiene la libreria, con la
        // chiave come la scrive il nodo (maiuscole comprese), e un
        // «Dimenticami» dall'altro.
        let nuovo = chiave_finta('e');
        comune.registra(
            &nuovo.to_ascii_uppercase(),
            Modifica::Associa(collegato(" Telefono\u{202E} ")),
        );
        comune.registra(&altro, Modifica::Revoca);
        assert!(tieni_la_lettura(&comune, &mut letto, leggi(c), 0));
        assert!(comune.associato(&nuovo), "il nuovo è dentro");
        assert!(!comune.associato(&altro), "chi ci ha dimenticati è fuori");
        let righe = &letto.as_ref().expect("letto").dispositivi;
        let riga = righe
            .iter()
            .find(|d| d.chiave == nuovo)
            .expect("il nuovo è nel pannello");
        assert_eq!(riga.nome, "Telefono", "ripulito come lo scrive la libreria");
        assert!(
            righe
                .iter()
                .find(|d| d.chiave == altro)
                .is_some_and(|d| d.revocato),
            "chi ci ha dimenticati resta in vista, revocato"
        );

        // Scritti: la lettura dopo porta le righe vere, e il registro si
        // svuota.
        scrivi_il_registro_su(c, &comune);
        assert!(tieni_la_lettura(
            &comune,
            &mut letto,
            leggi(c),
            comune.scritture_viste()
        ));
        assert!(prendi(&comune.sospese).is_empty());
        assert!(comune.associato(&nuovo));
        assert!(!comune.associato(&altro));
        assert_eq!(
            dispositivi::uno(c, &nuovo).unwrap().expect("scritto").nome,
            "Telefono"
        );
        assert!(dispositivi::uno(c, &altro).unwrap().expect("c'è").revocato);
    }

    #[test]
    fn una_modifica_arrivata_mentre_si_scrive_resta_da_scrivere() {
        let db = rymembra_app::db::open_in_memory().unwrap();
        let c = &db.connection;
        let chiave = chiave_finta('f');
        collega(c, &chiave);
        let (posta, _cassetta) = std::sync::mpsc::channel();
        let comune = Comune::nuovo(posta);
        comune.registra(&chiave, Modifica::Revoca);
        let in_scrittura = comune.da_scrivere();
        // Mentre lo scrittore lavora, la persona lo dimentica.
        comune.registra(&chiave, Modifica::Dimentica);
        let scritte = scrivi_su(c, &in_scrittura);
        comune.segna_scritte(&scritte.fatte);
        assert_eq!(
            comune
                .da_scrivere()
                .into_iter()
                .map(|(_, _, modifica)| modifica)
                .collect::<Vec<_>>(),
            vec![Modifica::Dimentica],
            "«Dimentica» non si perde dietro la revoca già scritta"
        );
        assert!(!comune.associato(&chiave));
    }

    #[test]
    fn con_un_lavoro_lungo_il_filo_legge_e_il_nodo_si_accende() {
        // Reperto R01-1: il filo chiedeva la libreria con
        // `con_libreria_entro` (2 s), e i lavori di sottofondo che la prendono
        // a turno — una prima analisi dura ore — gliela facevano perdere: la
        // lettura del giro e l'accensione tornavano `db.locked`, e il nodo
        // restava spento. Il lavoro lento è simulato come in
        // `servizio::prove::una_scrittura_lunga_qui_non_ferma_chi_sfoglia_da_lontano`.
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        let stato = &stato;
        let giornale =
            crate::stato::con_libreria(stato, |l| Ok(l.giornale.clone())).expect("libreria aperta");
        assert_eq!(giornale, "wal", "senza WAL questa prova non prova niente");
        let io = crate::stato::con_libreria(stato, |l| crate::nuvola::dispositivo(&l.connection))
            .expect("l'identificativo di questo dispositivo");

        let (preso, lavoro_partito) = std::sync::mpsc::channel::<()>();
        let (finito, filo_finito) = std::sync::mpsc::channel::<()>();
        std::thread::scope(|ambito| {
            let lavoro = ambito.spawn(move || {
                crate::stato::con_libreria(stato, |l| {
                    let tx = l
                        .connection
                        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                        .map_err(|err| rymembra_app::db::codice_da_sqlite("prova", &err))?;
                    dispositivi::attiva(&tx, true)?;
                    let _ = preso.send(());
                    let _ = filo_finito.recv_timeout(ATTESA_LIBRERIA * 4);
                    tx.commit()
                        .map_err(|err| rymembra_app::db::codice_da_sqlite("prova", &err))
                })
            });
            lavoro_partito
                .recv()
                .expect("il lavoro ha preso la libreria");

            let inizio = Instant::now();
            let lettura = leggi_da(stato, leggi);
            let dispositivo = dispositivo_di(stato);
            let tempo = inizio.elapsed();
            let _ = finito.send(());
            lavoro
                .join()
                .expect("il lavoro non cade")
                .expect("il lavoro scrive");

            let lettura = lettura.expect("il filo legge il giro");
            assert!(
                !lettura.impostazioni.attivo,
                "quel che era scritto, non la scrittura ancora aperta"
            );
            assert_eq!(dispositivo.expect("e chi è, per accendere il nodo"), io);
            assert!(tempo < ATTESA_LIBRERIA, "ha aspettato il lavoro: {tempo:?}");
        });
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
