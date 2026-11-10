//! Il collegamento via internet fra i dispositivi di una persona: il telefono
//! fuori casa che ascolta la musica del PC, e viceversa.
//!
//! La rete sta in `rymembra-lontano` (iroh: QUIC, fori nel NAT, e i relay
//! quando non si passa diritti); qui c'è quel che la lega a Rymembra.
//!
//! - [`filo`]: la vita del nodo, il pannello.
//! - [`ospite`]: quel che il nodo chiede all'app — il cancello, le conferme,
//!   le associazioni fatte.
//! - [`servizio`]: le domande degli altri dispositivi sulla libreria di qui.
//! - [`sfoglia`]: la libreria degli altri, vista e ascoltata da qui.
//! - [`identita`]: la chiave di questo dispositivo, nel portachiavi.
//!
//! # Collegarsi
//!
//! Un dispositivo mostra il QR ([`lontano_qr_crea`]), l'altro lo legge con la
//! fotocamera o ne incolla il link ([`lontano_collega`]). Chi mostra il QR
//! conferma (`lontano:richiesta`, poi [`lontano_rispondi`]); da lì i due si
//! ritrovano da soli, per chiave, dovunque siano.
//!
//! # I comandi
//!
//! Tutti sul pool bloccante (`crate::disparte`) tranne [`lontano_stato`] e
//! [`lontano_rispondi`], che non aspettano niente: il nodo può impiegare
//! secondi a raggiungere un altro dispositivo, e l'associazione minuti (chi
//! mostra il QR deve dire di sì).
//!
//! Come per «Ascolta insieme», un errore di rete arriva alla finestra senza
//! la sua causa: può contenere un indirizzo. Il diario ha già quel che serve,
//! dagli eventi del nodo.

mod filo;
mod identita;
pub(crate) mod insieme;
mod ipc;
mod ospite;
pub(crate) mod scarico;
mod servizio;
pub(crate) mod sfoglia;

use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rymembra_app::dispositivi::{self, DatiMobili};
use rymembra_domain::errors::{AppError, Domain, ErrorCode, ErrorCodeKind};
use rymembra_lontano::nodo::VALIDITA_INVITO;
use rymembra_lontano::{Domanda, Nodo};
use tauri::{AppHandle, Manager as _, State};

use crate::errore::{Esito, errore};
use crate::stato::adesso_ms;
use filo::{ATTESA_LIBRERIA, Comune, Posta, in_libreria, prendi};

pub use ipc::{InvitoIpc, StatoLontanoIpc};
pub(crate) use sfoglia::{ascoltato_qui, copertina, e_una_copertina, voce_salvata};

/// Quanto un comando aspetta che il filo abbia riletto: accendere il nodo
/// vuol dire aprire i socket, e il portachiavi può chiedere un attimo.
const ATTESA_RILETTURA: Duration = Duration::from_secs(15);

/// Quanto [`ferma`] aspetta il filo uscendo. Oltre, si esce lo stesso: gli
/// altri se ne accorgono dal silenzio, mezzo minuto dopo.
const ATTESA_FERMA: Duration = Duration::from_millis(1500);

/// Quanto si aspetta l'altro per dirgli che lo si dimentica.
const ATTESA_DIMENTICAMI: Duration = Duration::from_secs(5);

/// Lo stato del collegamento, gestito da Tauri.
pub struct StatoLontano {
    comune: Arc<Comune>,
    /// La cassetta del filo, finché non parte.
    cassetta: Mutex<Option<Receiver<Posta>>>,
}

impl StatoLontano {
    /// Lo stato, col filo ancora da far partire.
    #[must_use]
    pub fn nuovo() -> Self {
        let (posta, cassetta) = channel();
        Self {
            comune: Arc::new(Comune::nuovo(posta)),
            cassetta: Mutex::new(Some(cassetta)),
        }
    }
}

impl Default for StatoLontano {
    fn default() -> Self {
        Self::nuovo()
    }
}

/// Fa partire il filo. Da spento il filo legge le impostazioni e basta.
pub fn avvia_filo(app: &AppHandle) {
    let Some(stato) = app.try_state::<StatoLontano>() else {
        return;
    };
    let Some(cassetta) = stato
        .cassetta
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
    else {
        return;
    };
    filo::avvia(app.clone(), Arc::clone(&stato.comune), cassetta);
}

/// Spegne il nodo uscendo: si chiama da `spegnimento::spegni`. Aspetta al
/// più [`ATTESA_FERMA`]. Ripetibile.
pub fn ferma(app: &AppHandle) {
    let Some(stato) = app.try_state::<StatoLontano>() else {
        return;
    };
    let (risposta, attesa) = channel();
    stato.comune.sveglia(Posta::Ferma(risposta));
    let _ = attesa.recv_timeout(ATTESA_FERMA);
}

/// Quel che questo dispositivo dice di sé è cambiato fuori dal filo — «Ascolta
/// insieme» ha cominciato o smesso di ospitare, si è acceso o spento: lo si
/// rilegge e lo si annuncia subito, senza aspettare il giro. Non aspetta.
pub fn presenza_cambiata(app: &AppHandle) {
    if let Some(stato) = app.try_state::<StatoLontano>() {
        stato.comune.sveglia(Posta::Rileggi {
            risposta: None,
            subito: false,
        });
    }
}

/// La rete del sistema è cambiata (dal telefono): il nodo cerca strade
/// nuove. Non aspetta.
#[cfg_attr(
    not(target_os = "android"),
    expect(dead_code, reason = "serve al telefono")
)]
pub fn cambio_di_rete(app: &AppHandle) {
    if let Some(stato) = app.try_state::<StatoLontano>() {
        stato.comune.sveglia(Posta::CambioDiRete);
    }
}

/// L'ultimo link `rymembra://collega/…` aperto da fuori, finché la finestra
/// non lo prende: col QR letto dalla fotocamera del sistema l'app può partire
/// da chiusa, e l'evento arriverebbe prima della finestra.
static LINK_IN_ATTESA: Mutex<Option<String>> = Mutex::new(None);

/// Un link di collegamento aperto da fuori (Android: l'intento della
/// fotocamera). Se ha la forma giusta, lo si tiene e lo si dice alla
/// finestra, che chiede conferma prima di collegarsi.
#[cfg_attr(
    not(target_os = "android"),
    expect(dead_code, reason = "serve al telefono")
)]
pub fn link_arrivato(app: &AppHandle, link: &str) {
    if rymembra_lontano::Invito::da_link(link).is_err() {
        crate::nota!("[lontano] un link aperto da fuori non è un link di collegamento");
        return;
    }
    *LINK_IN_ATTESA
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(link.to_owned());
    use crate::spegnimento::Emette as _;
    app.emetti("lontano:link", link.to_owned());
}

/// Il link aperto da fuori e non ancora preso: vedi [`LINK_IN_ATTESA`]. Una
/// volta sola.
#[tauri::command]
pub fn lontano_link_in_attesa() -> Esito<Option<String>> {
    Ok(LINK_IN_ATTESA
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take())
}

/// Un errore senza la sua causa, se è del collegamento: vedi il `//!`.
/// `identitaPersa` la tiene: la scrive `identita`, e dice solo quale strada
/// è mancata.
fn senza_causa_di_rete(err: AppError) -> AppError {
    let kind = err.code().kind();
    if kind.domain() == Domain::Lontano && kind != ErrorCodeKind::LontanoIdentitaPersa {
        AppError::new(err.code().clone())
    } else {
        err
    }
}

fn comune(app: &AppHandle) -> Result<Arc<Comune>, AppError> {
    app.try_state::<StatoLontano>()
        .map(|stato| Arc::clone(&stato.comune))
        .ok_or_else(|| {
            AppError::new(ErrorCode::InternalAborted {
                what: Some("lontano".to_owned()),
            })
        })
}

/// Il filo rilegge (e accende o spegne), e torna il pannello.
fn rileggi_e_aspetta(app: &AppHandle) -> Result<StatoLontanoIpc, AppError> {
    let comune = comune(app)?;
    let (risposta, attesa) = channel();
    comune.sveglia(Posta::Rileggi {
        risposta: Some(risposta),
        subito: true,
    });
    attesa.recv_timeout(ATTESA_RILETTURA).map_err(|_| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some("lontano".to_owned()),
        })
        .with_cause("il filo del collegamento non risponde")
    })?;
    Ok(prendi(&comune.vetrina).clone())
}

/// Qualcosa col nodo acceso; spento è `lontano.spento`.
fn nel_nodo<T>(
    app: &AppHandle,
    azione: impl FnOnce(&Nodo) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let nodo = comune(app)?
        .nodo()
        .ok_or_else(|| AppError::new(ErrorCode::LontanoSpento))?;
    azione(&nodo)
}

/// Accende «Raggiungibile da internet» se è spento: collegare due
/// dispositivi lo chiede, e chi ha premuto «Collega» l'ha già detto.
fn acceso_per_collegare(app: &AppHandle) -> Result<(), AppError> {
    let attivo = in_libreria(app, ATTESA_LIBRERIA, |c| {
        Ok(dispositivi::impostazioni(c)?.attivo)
    })?;
    if !attivo {
        chiedi_la_rete_di_casa();
        in_libreria(app, ATTESA_LIBRERIA, |c| dispositivi::attiva(c, true))?;
    }
    let pannello = rileggi_e_aspetta(app)?;
    if pannello.acceso {
        return Ok(());
    }
    Err(prendi(&comune(app)?.ultimo_errore)
        .clone()
        .unwrap_or_else(|| AppError::new(ErrorCode::LontanoSpento)))
}

/// Su Android 17 la rete di casa si chiede al gesto che la usa. Senza, il
/// collegamento passa dagli indirizzi pubblici e dal relay: non è un errore.
fn chiedi_la_rete_di_casa() {
    if let Err(err) = crate::telefono::rete_locale_o_chiedila() {
        crate::nota!(
            "[lontano] la rete di casa non è concessa codice={}: solo relay e indirizzi pubblici",
            err.code().kind().code()
        );
    }
}

/// Un comando sul pool bloccante, con gli errori senza causa di rete.
async fn in_disparte<T: Send + 'static>(
    lavoro: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Esito<T> {
    crate::disparte::in_disparte("lontano", lavoro)
        .await
        .map_err(errore)?
        .map_err(|err| errore(senza_causa_di_rete(err)))
}

/// Il pannello, adesso.
#[tauri::command]
pub fn lontano_stato(stato: State<'_, StatoLontano>) -> Esito<StatoLontanoIpc> {
    Ok(prendi(&stato.comune.vetrina).clone())
}

/// Accende o spegne «Raggiungibile da internet».
#[tauri::command]
pub async fn lontano_attiva(app: AppHandle, attivo: bool) -> Esito<StatoLontanoIpc> {
    in_disparte(move || {
        if attivo {
            chiedi_la_rete_di_casa();
        }
        in_libreria(&app, ATTESA_LIBRERIA, |c| dispositivi::attiva(c, attivo))?;
        rileggi_e_aspetta(&app)
    })
    .await
}

/// Il relay proprio, o nessuno per tornare a quelli di n0. Il nodo riparte.
#[tauri::command]
pub async fn lontano_relay(app: AppHandle, relay: Option<String>) -> Esito<StatoLontanoIpc> {
    in_disparte(move || {
        in_libreria(&app, ATTESA_LIBRERIA, |c| {
            dispositivi::scegli_relay(c, relay.as_deref())
        })?;
        rileggi_e_aspetta(&app)
    })
    .await
}

/// «Solo Wi-Fi».
#[tauri::command]
pub async fn lontano_dati_mobili(app: AppHandle, scelta: DatiMobili) -> Esito<StatoLontanoIpc> {
    in_disparte(move || {
        in_libreria(&app, ATTESA_LIBRERIA, |c| {
            dispositivi::scegli_dati_mobili(c, scelta)
        })?;
        rileggi_e_aspetta(&app)
    })
    .await
}

/// Un QR nuovo, da mostrare: quello di prima non vale più.
#[tauri::command]
pub async fn lontano_qr_crea(app: AppHandle) -> Esito<InvitoIpc> {
    in_disparte(move || {
        acceso_per_collegare(&app)?;
        let invito = nel_nodo(&app, Nodo::invita)?;
        let link = invito.link();
        let matrice = rymembra_lontano::matrice(&link)?;
        let ipc = InvitoIpc {
            moduli: matrice
                .moduli
                .iter()
                .map(|scuro| if *scuro { '1' } else { '0' })
                .collect(),
            lato: matrice.lato,
            link,
            scade_ms: adesso_ms()
                .saturating_add(i64::try_from(VALIDITA_INVITO.as_millis()).unwrap_or(i64::MAX)),
        };
        let comune = comune(&app)?;
        *prendi(&comune.invito) = Some(ipc.clone());
        comune.sveglia(Posta::Pubblica);
        Ok(ipc)
    })
    .await
}

/// Il QR mostrato non vale più.
#[tauri::command]
pub async fn lontano_qr_annulla(app: AppHandle) -> Esito<StatoLontanoIpc> {
    in_disparte(move || {
        if let Some(nodo) = comune(&app)?.nodo() {
            nodo.annulla_invito();
        }
        *prendi(&comune(&app)?.invito) = None;
        rileggi_e_aspetta(&app)
    })
    .await
}

/// Collega questo dispositivo a quello del link: il QR letto, o il link
/// incollato. Aspetta che dall'altra parte dicano di sì.
#[tauri::command]
pub async fn lontano_collega(app: AppHandle, link: String) -> Esito<StatoLontanoIpc> {
    in_disparte(move || {
        // Prima di accendere niente: un testo che non è un link si dice
        // subito.
        rymembra_lontano::Invito::da_link(&link)?;
        let annulli = comune(&app)?.annulli.load(Ordering::Acquire);
        acceso_per_collegare(&app)?;
        // Un «Annulla» arrivato mentre il nodo si accendeva.
        if comune(&app)?.annulli.load(Ordering::Acquire) != annulli {
            return Err(AppError::new(ErrorCode::InternalAborted {
                what: Some("collegamento annullato".to_owned()),
            }));
        }
        nel_nodo(&app, |nodo| nodo.collega(&link))?;
        rileggi_e_aspetta(&app)
    })
    .await
}

/// Smette di aspettare la conferma dell'altro dispositivo: [`lontano_collega`]
/// torna con `internal.aborted`, e dall'altra parte la domanda sparisce.
#[tauri::command]
pub fn lontano_collega_annulla(stato: State<'_, StatoLontano>) -> Esito<()> {
    stato.comune.annulli.fetch_add(1, Ordering::AcqRel);
    if let Some(nodo) = stato.comune.nodo() {
        nodo.annulla_collegamento();
    }
    Ok(())
}

/// I dispositivi che non rispondevano si riprovano adesso: la persona ha
/// aperto la loro pagina, o quella dei dispositivi. Non aspetta.
#[tauri::command]
pub fn lontano_cerca_ora(stato: State<'_, StatoLontano>) -> Esito<()> {
    if let Some(nodo) = stato.comune.nodo() {
        nodo.riprova_adesso();
    }
    Ok(())
}

/// La risposta di chi mostra il QR a chi l'ha letto.
#[tauri::command]
pub fn lontano_rispondi(stato: State<'_, StatoLontano>, id: u64, accetta: bool) -> Esito<()> {
    let richieste = prendi(&stato.comune.richieste);
    if let Some((_, risposta)) = richieste.iter().find(|(r, _)| r.id == id) {
        let _ = risposta.send(accetta);
    }
    Ok(())
}

/// Un nome nuovo per un dispositivo collegato, solo qui.
#[tauri::command]
pub async fn lontano_rinomina(
    app: AppHandle,
    chiave: String,
    nome: String,
) -> Esito<StatoLontanoIpc> {
    in_disparte(move || {
        in_libreria(&app, ATTESA_LIBRERIA, |c| {
            dispositivi::rinomina(c, &chiave, &nome)
        })?;
        rileggi_e_aspetta(&app)
    })
    .await
}

/// Dimentica un dispositivo: glielo si dice, se risponde, e lo si toglie.
#[tauri::command]
pub async fn lontano_dimentica(app: AppHandle, chiave: String) -> Esito<StatoLontanoIpc> {
    in_disparte(move || {
        let comune = comune(&app)?;
        if let Some(nodo) = comune.nodo()
            && nodo.raggiungibile(&chiave)
            && let Err(err) = nodo.chiedi(&chiave, Domanda::Dimenticami, ATTESA_DIMENTICAMI)
        {
            crate::nota!(
                "[lontano] l'altro non ha saputo che lo si dimentica codice={}",
                err.code().kind().code()
            );
        }
        comune.ammetti(&chiave, false);
        let insieme_cambiato = in_libreria(&app, ATTESA_LIBRERIA, |c| {
            dispositivi::dimentica(c, &chiave)
        })?;
        // Anche la sua chiave di «Ascolta insieme», che il QR aveva reso
        // fidata: la rete di «Ascolta insieme» rilegge e chiude le sue
        // connessioni.
        if insieme_cambiato {
            crate::insieme::fidati_cambiati(&app);
        }
        // I suoi brani in coda non si aprono più.
        crate::riproduzione::togli_lontane(&app, &chiave);
        rileggi_e_aspetta(&app)
    })
    .await
}
