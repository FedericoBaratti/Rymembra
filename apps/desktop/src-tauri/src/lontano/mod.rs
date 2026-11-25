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
//! - [`specchio`]: la libreria unica — il catalogo degli altri dispositivi come
//!   righe di `tracks`, la posta in uscita, le copertine prese pigramente.
//! - [`identita`]: la chiave di questo dispositivo, nel portachiavi.
//!
//! # Collegarsi
//!
//! Un dispositivo mostra il QR ([`lontano_qr_crea`]), con accanto la propria
//! impronta; l'altro lo legge con la fotocamera o ne incolla il link, vede con
//! chi si collegherebbe — il nome dichiarato e l'impronta,
//! [`lontano_link_esamina`] — e dice sì ([`lontano_collega`]). Chi mostra il
//! QR conferma a sua volta, col nome dichiarato e l'impronta dell'altro
//! (`lontano:richiesta`, poi [`lontano_rispondi`]); da lì i due si ritrovano
//! da soli, per chiave, dovunque siano. Nessuno dei due comandi accende
//! «Raggiungibile da internet»: è un sì a parte ([`lontano_attiva`]).
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
pub(crate) mod specchio;
mod testo;

use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use crate::nucleo::{ComeNucleo, Nucleo};
use rymembra_app::dispositivi::{self, DatiMobili};
use rymembra_domain::errors::{AppError, Domain, ErrorCode, ErrorCodeKind};
use rymembra_lontano::nodo::VALIDITA_INVITO;
use rymembra_lontano::{Domanda, Nodo};

use crate::errore::{Esito, errore};
use crate::stato::adesso_ms;
use filo::{Comune, Modifica, Posta, leggi_a_parte, prendi, scrivi_in_libreria};

pub use ipc::{InvitoIpc, LinkIpc, StatoLontanoIpc};
pub(crate) use sfoglia::rete_a_consumo;
pub(crate) use sfoglia::{ascoltato_qui, ripulisci_le_copertine_di_prima};
pub(crate) use specchio::{
    apri_riga, copertina_di_una_riga, origine_che_suona, posta_nuova, scegli_la_copia,
};
pub(crate) use testo::dal_proprietario;

/// Quanto un comando aspetta che il filo abbia riletto: accendere il nodo
/// vuol dire aprire i socket, e il portachiavi può chiedere un attimo.
const ATTESA_RILETTURA: Duration = Duration::from_secs(15);

/// Quanto [`ferma`] aspetta il filo uscendo. Oltre, si esce lo stesso: gli
/// altri se ne accorgono dal silenzio, mezzo minuto dopo.
const ATTESA_FERMA: Duration = Duration::from_millis(1500);

/// Quanto si aspetta l'altro per dirgli che lo si dimentica.
const ATTESA_DIMENTICAMI: Duration = Duration::from_secs(5);

/// Il corpo dello stato del collegamento.
pub struct CorpoLontano {
    comune: Arc<Comune>,
    /// La cassetta del filo, finché non parte.
    cassetta: Mutex<Option<Receiver<Posta>>>,
}

crate::nucleo::condiviso! {
    /// Lo stato del collegamento: un handle `Clone` sul [`CorpoLontano`], perché il nucleo e
    /// Tauri lo possiedono insieme (vedi `nucleo`).
    pub struct StatoLontano(CorpoLontano);
}

impl StatoLontano {
    /// Lo stato, col filo ancora da far partire.
    #[must_use]
    pub fn nuovo() -> Self {
        let (posta, cassetta) = filo::nuova_cassetta();
        Self::da_corpo(CorpoLontano {
            comune: Arc::new(Comune::nuovo(posta)),
            cassetta: Mutex::new(Some(cassetta)),
        })
    }
}

impl Default for StatoLontano {
    fn default() -> Self {
        Self::nuovo()
    }
}

/// Fa partire il filo. Da spento il filo legge le impostazioni e basta.
pub fn avvia_filo(app: &Nucleo) {
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
pub fn ferma(app: &impl ComeNucleo) {
    let app = &app.come_nucleo();
    let Some(stato) = app.try_state::<StatoLontano>() else {
        return;
    };
    let (risposta, attesa) = channel();
    stato.comune.sveglia(Posta::Ferma(risposta));
    let _ = attesa.recv_timeout(ATTESA_FERMA);
    // Il registro non ancora scritto: un «Dimentica» che resta solo in
    // memoria tornerebbe dentro al prossimo avvio.
    filo::scrivi_prima_di_uscire(app, &stato.comune);
}

/// Quel che questo dispositivo dice di sé è cambiato fuori dal filo — «Ascolta
/// insieme» ha cominciato o smesso di ospitare, si è acceso o spento: lo si
/// rilegge e lo si annuncia subito, senza aspettare il giro. Non aspetta.
pub fn presenza_cambiata(app: &impl ComeNucleo) {
    let app = &app.come_nucleo();
    if let Some(stato) = app.try_state::<StatoLontano>() {
        stato.comune.sveglia(Posta::Rileggi {
            risposta: None,
            subito: false,
        });
    }
}

/// La finestra è andata nel vassoio: il QR mostrato non vale più, e uno che
/// sta nascendo non nascerà valido (decisione D-30). Con un QR valido
/// `rymembra/associa/1` resta aperto a chi lo presenta, e chi non vede più la
/// finestra non sa che c'è; la scheda che lo mostra resta montata anche con la
/// finestra nascosta, e da sola non se ne accorge. Non aspetta.
pub fn finestra_nascosta(app: &impl ComeNucleo) {
    let app = &app.come_nucleo();
    if let Some(stato) = app.try_state::<StatoLontano>() {
        annulla_il_qr(&stato.comune);
    }
    // Il dialogo di un link muore con la pagina: non resta un «aperto» che
    // scarterebbe in silenzio i link che verranno.
    posto_del_link().nel_dialogo = false;
}

/// Il QR non vale più, anche quello che sta nascendo in [`lontano_qr_crea`]:
/// vedi [`metti_il_qr`].
fn annulla_il_qr(comune: &Comune) {
    comune.qr_annullati.fetch_add(1, Ordering::AcqRel);
    if let Some(nodo) = comune.nodo() {
        nodo.annulla_invito();
    }
    *prendi(&comune.invito) = None;
    comune.sveglia(Posta::Pubblica);
}

/// Il QR appena creato va nel pannello, se nessuno l'ha annullato mentre
/// nasceva (`annullati` è [`Comune::qr_annullati`] di prima): allora non vale
/// nemmeno nel nodo, e il pannello non lo mostra. `true` se va a schermo.
fn metti_il_qr(comune: &Comune, invito: InvitoIpc, annullati: u64) -> bool {
    let mut mostrato = prendi(&comune.invito);
    if comune.qr_annullati.load(Ordering::Acquire) != annullati {
        drop(mostrato);
        if let Some(nodo) = comune.nodo() {
            nodo.annulla_invito();
        }
        crate::nota!("[lontano] un QR nato con la finestra nel vassoio non vale");
        return false;
    }
    *mostrato = Some(invito);
    drop(mostrato);
    comune.sveglia(Posta::Pubblica);
    true
}

/// La rete del sistema è cambiata (dal telefono): il nodo cerca strade
/// nuove. Non aspetta.
#[cfg_attr(
    not(target_os = "android"),
    expect(dead_code, reason = "serve al telefono")
)]
pub fn cambio_di_rete(app: &impl ComeNucleo) {
    let app = &app.come_nucleo();
    if let Some(stato) = app.try_state::<StatoLontano>() {
        stato.comune.sveglia(Posta::CambioDiRete);
    }
}

/// Il posto del link `rymembra://collega/…` aperto da fuori: **uno solo**, e il
/// primo vince (S4-4).
///
/// Col QR letto dalla fotocamera del sistema l'app può partire da chiusa, e
/// l'evento arriverebbe prima della finestra: il link resta qui finché la pagina
/// non lo prende. Ma `LINK_IN_ATTESA` — un `Option` — era sovrascritto
/// dall'ultimo, e la pagina rimpiazzava il dialogo aperto: chi leggeva
/// «Collegarsi a "PC di casa"?» con l'impronta A se lo vedeva cambiare sotto il
/// dito nello stesso dialogo con l'impronta B. Un link che arriva mentre un altro
/// aspetta di essere preso, o mentre il suo dialogo è aperto, si scarta e lo si
/// scrive nel diario.
#[derive(Debug, Default)]
struct PostoDelLink {
    /// Il link aperto da fuori e non ancora preso dalla pagina.
    in_attesa: Option<String>,
    /// La pagina ha un dialogo di collegamento aperto: lo dice lei
    /// ([`lontano_link_dialogo`]), e si azzera con la pagina e con la finestra.
    nel_dialogo: bool,
}

/// Cosa fare di un link che arriva.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arrivo {
    /// Lo si tiene, e si dice alla finestra.
    Preso,
    /// Un altro aspetta o è nel dialogo: il primo vince, questo si scarta.
    GiaUno,
    /// Non è un link di collegamento.
    NonELink,
}

/// La decisione, senza finestra né sistema.
fn arrivo_del_link(posto: &PostoDelLink, valido: bool) -> Arrivo {
    if !valido {
        Arrivo::NonELink
    } else if posto.in_attesa.is_some() || posto.nel_dialogo {
        Arrivo::GiaUno
    } else {
        Arrivo::Preso
    }
}

static POSTO_DEL_LINK: Mutex<PostoDelLink> = Mutex::new(PostoDelLink {
    in_attesa: None,
    nel_dialogo: false,
});

fn posto_del_link() -> std::sync::MutexGuard<'static, PostoDelLink> {
    POSTO_DEL_LINK
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Un link di collegamento aperto da fuori (Android: l'intento della
/// fotocamera; Windows: `rymembra://collega/…` cliccato, dalla riga di comando
/// — vedi `argomenti`). Se ha la forma giusta, lo si tiene e lo si dice alla
/// finestra, che chiede conferma prima di collegarsi.
///
/// Torna `true` se il link è stato preso: solo allora chi lo ha portato alza la
/// finestra (`comparsa::secondo_avvio`). Un link scartato non toglie il fuoco a
/// nessuno: una pagina web che ne lancia uno a ogni clic non deve poterlo fare.
pub fn link_arrivato(app: &impl ComeNucleo, link: &str) -> bool {
    let app = &app.come_nucleo();
    let valido = rymembra_lontano::Invito::da_link(link).is_ok();
    let mut posto = posto_del_link();
    match arrivo_del_link(&posto, valido) {
        Arrivo::NonELink => {
            crate::nota!("[lontano] un link aperto da fuori non è un link di collegamento");
            false
        }
        Arrivo::GiaUno => {
            crate::nota!(
                "[lontano] un secondo link aperto da fuori mentre il primo aspetta risposta: si scarta, vince il primo"
            );
            false
        }
        Arrivo::Preso => {
            posto.in_attesa = Some(link.to_owned());
            drop(posto);
            app.emetti("lontano:link", link.to_owned());
            true
        }
    }
}

/// Il link aperto da fuori e non ancora preso: vedi [`PostoDelLink`]. Una
/// volta sola.
#[tauri::command]
pub async fn lontano_link_in_attesa() -> Esito<Option<String>> {
    in_disparte(|| Ok(posto_del_link().in_attesa.take())).await
}

/// La pagina dice se ha un dialogo di collegamento aperto: finché è aperto, un
/// altro link non lo sostituisce ([`link_arrivato`]).
///
/// La pagina lo dice a ogni cambio e una volta al montaggio: una pagina nuova non
/// ha dialoghi, e un `true` rimasto di quella di prima si azzera da sé.
#[tauri::command]
pub async fn lontano_link_dialogo(aperto: bool) -> Esito<()> {
    in_disparte(move || {
        posto_del_link().nel_dialogo = aperto;
        Ok(())
    })
    .await
}

/// Con chi si collegherebbe questo link: il nome che dichiara, ripulito, e
/// l'impronta della chiave. Da mostrare prima di [`lontano_collega`], qualunque
/// strada abbia fatto il link — aperto da fuori, letto dallo scanner,
/// incollato.
fn esamina(link: &str) -> Result<LinkIpc, AppError> {
    let invito = rymembra_lontano::Invito::da_link(link)?;
    Ok(LinkIpc {
        link: link.trim().to_owned(),
        // Come ogni nome che arriva da un altro dispositivo: niente controlli
        // né inversioni di direzione, e un tetto.
        nome: invito
            .nome
            .as_deref()
            .map(rymembra_app::insieme::ripulisci_nome)
            .filter(|nome| !nome.is_empty()),
        impronta: invito.impronta(),
    })
}

/// Vedi [`esamina`].
#[tauri::command]
pub async fn lontano_link_esamina(link: String) -> Esito<LinkIpc> {
    in_disparte(move || esamina(&link)).await
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

fn comune(app: &Nucleo) -> Result<Arc<Comune>, AppError> {
    app.try_state::<StatoLontano>()
        .map(|stato| Arc::clone(&stato.comune))
        .ok_or_else(|| {
            AppError::new(ErrorCode::InternalAborted {
                what: Some("lontano".to_owned()),
            })
        })
}

/// Il filo rilegge (e accende o spegne), e torna il pannello.
fn rileggi_e_aspetta(app: &Nucleo) -> Result<StatoLontanoIpc, AppError> {
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
    app: &Nucleo,
    azione: impl FnOnce(&Nodo) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let nodo = comune(app)?
        .nodo()
        .ok_or_else(|| AppError::new(ErrorCode::LontanoSpento))?;
    azione(&nodo)
}

/// Collegare due dispositivi vuole «Raggiungibile da internet» acceso. Spento,
/// è `lontano.spento`, e **resta spento**: accenderlo apre questo dispositivo
/// a internet, e lo si chiede a parte ([`lontano_attiva`], dopo un sì
/// esplicito nella finestra). Un «Collega» su un link arrivato da una pagina
/// web non deve bastare.
fn attivo_per_collegare(c: &rusqlite::Connection) -> Result<(), AppError> {
    if dispositivi::impostazioni(c)?.attivo {
        Ok(())
    } else {
        Err(AppError::new(ErrorCode::LontanoSpento))
    }
}

/// Il nodo acceso per collegare: vedi [`attivo_per_collegare`].
fn acceso_per_collegare(app: &Nucleo) -> Result<(), AppError> {
    leggi_a_parte(app, attivo_per_collegare)?;
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
pub async fn lontano_stato(app: Nucleo) -> Esito<StatoLontanoIpc> {
    in_disparte(move || Ok(prendi(&comune(&app)?.vetrina).clone())).await
}

/// Accende o spegne «Raggiungibile da internet».
#[tauri::command]
pub async fn lontano_attiva(app: Nucleo, attivo: bool) -> Esito<StatoLontanoIpc> {
    in_disparte(move || {
        if attivo {
            chiedi_la_rete_di_casa();
        }
        scrivi_in_libreria(&app, |c| dispositivi::attiva(c, attivo))?;
        rileggi_e_aspetta(&app)
    })
    .await
}

/// Il relay proprio, o nessuno per tornare a quelli di n0. Il nodo riparte.
#[tauri::command]
pub async fn lontano_relay(app: Nucleo, relay: Option<String>) -> Esito<StatoLontanoIpc> {
    in_disparte(move || {
        scrivi_in_libreria(&app, |c| dispositivi::scegli_relay(c, relay.as_deref()))?;
        rileggi_e_aspetta(&app)
    })
    .await
}

/// «Solo Wi-Fi».
#[tauri::command]
pub async fn lontano_dati_mobili(app: Nucleo, scelta: DatiMobili) -> Esito<StatoLontanoIpc> {
    in_disparte(move || {
        scrivi_in_libreria(&app, |c| dispositivi::scegli_dati_mobili(c, scelta))?;
        rileggi_e_aspetta(&app)
    })
    .await
}

/// «Resta attiva in sottofondo» (solo il telefono): accende o spegne il
/// servizio in primo piano che tiene il nodo acceso anche con l'app chiusa.
///
/// Non accende «Raggiungibile da internet»: quello è un sì a parte
/// ([`lontano_attiva`]), e senza i PC non trovano il telefono lo stesso.
/// Fuori da Android è `internal.aborted`: la finestra non lo chiama.
#[tauri::command]
pub async fn lontano_sottofondo(app: Nucleo, attivo: bool) -> Esito<StatoLontanoIpc> {
    #[cfg(target_os = "android")]
    return in_disparte(move || {
        // La scelta più recente vince: un «Spegni» della notifica non ancora
        // scritto non disfa quel che la persona ha appena deciso.
        comune(&app)?.spegnere.store(false, Ordering::Release);
        // La scelta dell'auto si rilegge dopo la scrittura: spegnere il
        // sottofondo spegne anche lei, e il servizio deve sentirlo da qui — se
        // gli si dicesse quella di prima, resterebbe a guardare l'auto fino al
        // giro dopo.
        let parti_con_auto = scrivi_in_libreria(&app, |c| {
            dispositivi::sottofondo(c, attivo)?;
            Ok(dispositivi::impostazioni(c)?.parti_con_auto)
        })?;
        // Subito, e non al giro del filo: la persona vuole vedere la notifica
        // — o sentirla andare — adesso, e al primo «acceso» le si chiedono le
        // notifiche. Il filo poi dice al servizio a chi si è collegati.
        rymembra_android::sistema::sottofondo(attivo, &[], true, attivo && parti_con_auto)
            .map_err(|err| {
                AppError::new(ErrorCode::InternalUnexpected {
                    detail: Some("servizio del sottofondo".to_owned()),
                })
                .with_cause(err.to_string())
            })?;
        rileggi_e_aspetta(&app)
    })
    .await;
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, attivo);
        Err(errore(AppError::new(ErrorCode::InternalAborted {
            what: Some("solo su Android".to_owned()),
        })))
    }
}

/// «Parti con l'auto» (solo il telefono): quando l'auto si collega, Rymembra
/// riprende da sola la coda salvata, al posto dell'ultima app usata.
///
/// Si accende solo col sottofondo acceso — `internal.aborted` se è spento: la
/// finestra non lo permette, e arrivarci vuol dire che si è spento un istante
/// prima — e con lui si spegne (`dispositivi::sottofondo`). Dove la build non
/// la offre (la variante per Google Play) non è un comando che la finestra
/// chiami: la riga non c'è. Il servizio lo sa dal filo del collegamento, a
/// questo giro, ed è `rileggi_e_aspetta` che lo aspetta.
///
/// Fuori da Android è `internal.aborted`: la finestra non lo chiama.
#[tauri::command]
pub async fn lontano_parti_con_auto(app: Nucleo, attivo: bool) -> Esito<StatoLontanoIpc> {
    #[cfg(target_os = "android")]
    return in_disparte(move || {
        scrivi_in_libreria(&app, |c| dispositivi::parti_con_auto(c, attivo))?;
        rileggi_e_aspetta(&app)
    })
    .await;
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, attivo);
        Err(errore(AppError::new(ErrorCode::InternalAborted {
            what: Some("solo su Android".to_owned()),
        })))
    }
}

/// Un QR nuovo, da mostrare: quello di prima non vale più. Con
/// «Raggiungibile da internet» spento è `lontano.spento`: vedi
/// [`attivo_per_collegare`].
#[tauri::command]
pub async fn lontano_qr_crea(app: Nucleo) -> Esito<InvitoIpc> {
    in_disparte(move || {
        let annullati = comune(&app)?.qr_annullati.load(Ordering::Acquire);
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
        // La finestra andata nel vassoio mentre il QR nasceva (decisione
        // D-30): nel pannello non arriva, e la finestra non lo mostra.
        metti_il_qr(&*comune(&app)?, ipc.clone(), annullati);
        Ok(ipc)
    })
    .await
}

/// Il QR mostrato non vale più.
#[tauri::command]
pub async fn lontano_qr_annulla(app: Nucleo) -> Esito<StatoLontanoIpc> {
    in_disparte(move || {
        if let Some(nodo) = comune(&app)?.nodo() {
            nodo.annulla_invito();
        }
        *prendi(&comune(&app)?.invito) = None;
        rileggi_e_aspetta(&app)
    })
    .await
}

/// Collega questo dispositivo a quello del link — dopo che la finestra ha
/// mostrato con chi ([`lontano_link_esamina`]) e la persona ha detto sì, e
/// solo con «Raggiungibile da internet» già acceso ([`attivo_per_collegare`]).
/// Il QR letto, o il link
/// incollato. Aspetta che dall'altra parte dicano di sì.
#[tauri::command]
pub async fn lontano_collega(app: Nucleo, link: String) -> Esito<StatoLontanoIpc> {
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
pub async fn lontano_collega_annulla(app: Nucleo) -> Esito<()> {
    in_disparte(move || {
        let comune = comune(&app)?;
        comune.annulli.fetch_add(1, Ordering::AcqRel);
        if let Some(nodo) = comune.nodo() {
            nodo.annulla_collegamento();
        }
        Ok(())
    })
    .await
}

/// I dispositivi che non rispondevano si riprovano adesso: la persona ha
/// aperto la loro pagina, o quella dei dispositivi. Non aspetta.
#[tauri::command]
pub async fn lontano_cerca_ora(app: Nucleo) -> Esito<()> {
    in_disparte(move || {
        if let Some(nodo) = comune(&app)?.nodo() {
            nodo.riprova_adesso();
        }
        Ok(())
    })
    .await
}

/// La risposta di chi mostra il QR a chi l'ha letto.
#[tauri::command]
pub async fn lontano_rispondi(app: Nucleo, id: u64, accetta: bool) -> Esito<()> {
    in_disparte(move || {
        let comune = comune(&app)?;
        let richieste = prendi(&comune.richieste);
        if let Some((_, risposta)) = richieste.iter().find(|(r, _)| r.id == id) {
            let _ = risposta.send(accetta);
        }
        Ok(())
    })
    .await
}

/// Un nome nuovo per un dispositivo collegato, solo qui.
#[tauri::command]
pub async fn lontano_rinomina(app: Nucleo, chiave: String, nome: String) -> Esito<StatoLontanoIpc> {
    in_disparte(move || {
        scrivi_in_libreria(&app, |c| dispositivi::rinomina(c, &chiave, &nome))?;
        rileggi_e_aspetta(&app)
    })
    .await
}

/// Dimentica un dispositivo: glielo si dice, se risponde, e lo si toglie.
///
/// Fatto subito, anche con la libreria occupata (decisione D-24): esce dal
/// cancello, perde ogni connessione, e il registro tiene la modifica finché la
/// libreria non l'ha scritta — nessuna lettura la disfa (vedi
/// `filo::Comune::registra`). Prima la scrittura rinunciava dopo due secondi,
/// il comando diceva «occupato», e il dispositivo tornava dentro alla prima
/// lettura riuscita (reperto R01-2).
#[tauri::command]
pub async fn lontano_dimentica(app: Nucleo, chiave: String) -> Esito<StatoLontanoIpc> {
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
        comune.registra(&chiave, Modifica::Dimentica);
        // Subito, senza aspettare la rilettura: ogni sua connessione si
        // chiude, anche quelle aperte prima e i flussi di «Ascolta insieme».
        if let Some(nodo) = comune.nodo() {
            nodo.revoca(&chiave);
        }
        // Le sue righe in coda non si aprono più: le righe se ne vanno con lui
        // (`specchio::archivio::comincia_a_dimenticare`, poi a blocchi), e `apri_riga` rifiuta
        // un dispositivo che non è più collegato.
        specchio::dimenticato(&chiave);
        // E le copertine che una versione di prima gli teneva in cache: non
        // devono restare su disco, né uscire da una scheda di sistema.
        sfoglia::togli_le_copertine_di(&app, &chiave);
        // Nella libreria appena si può, con la sua chiave di «Ascolta
        // insieme» che il QR aveva reso fidata: lo scrittore fa rileggere la
        // rete di «Ascolta insieme», che chiude le sue connessioni.
        filo::scrivi_il_registro(&app, &comune);
        rileggi_e_aspetta(&app)
    })
    .await
}

/// Le risposte «Lasciali» ricordate: per dispositivo, di quante righe (FR2).
///
/// La finestra le legge una volta all'apertura, per non rifare la domanda sulle
/// righe che un dispositivo non manda più alla stessa cifra dopo un riavvio.
/// Sola lettura, su una connessione a parte: all'avvio la libreria può essere
/// occupata da una scansione, e una domanda già risolta non deve aspettarla.
#[tauri::command]
pub async fn lontano_specchio_lasciati(
    app: Nucleo,
) -> Esito<std::collections::BTreeMap<String, usize>> {
    in_disparte(move || leggi_a_parte(&app, dispositivi::righe_lasciate)).await
}

/// Ricorda che la persona ha detto «Lasciali» a `quante` righe di `chiave`; senza
/// un numero (`None`: «Togli») dimentica la sua risposta (FR2).
///
/// Sta qui e non in `specchio.rs` perché il ricordo è una chiave delle
/// impostazioni del collegamento (`dispositivi::CHIAVE_LASCIATI`), non una
/// regola dello specchio: le regole e la loro conferma (`lontano_specchio_conferma`)
/// restano dove sono.
///
/// # Errori
///
/// `lontano.protocollo` per una chiave che non è di 64 cifre esadecimali.
#[tauri::command]
pub async fn lontano_specchio_lascia(
    app: Nucleo,
    chiave: String,
    quante: Option<usize>,
) -> Esito<()> {
    in_disparte(move || {
        scrivi_in_libreria(&app, |c| dispositivi::ricorda_lasciati(c, &chiave, quante))
    })
    .await
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn collegarsi_non_accende_da_se_raggiungibile_da_internet() {
        let db = rymembra_app::db::open_in_memory().unwrap();
        let c = &db.connection;
        let err = attivo_per_collegare(c).unwrap_err();
        assert_eq!(err.code().kind(), ErrorCodeKind::LontanoSpento);
        assert!(
            !dispositivi::impostazioni(c).unwrap().attivo,
            "resta spento: lo accende un sì a parte"
        );
        dispositivi::attiva(c, true).unwrap();
        attivo_per_collegare(c).unwrap();
    }

    /// Un nodo solo su questa macchina, per le prove del QR.
    struct Nessuno;

    impl rymembra_lontano::Ospite for Nessuno {
        fn associato(&self, _chiave: &str) -> bool {
            false
        }
        fn conferma(&self, _chiave: &str, _chi: &rymembra_lontano::Presentazione) -> bool {
            false
        }
        fn nuovo_pari(
            &self,
            _chiave: &str,
            _chi: &rymembra_lontano::Presentazione,
            _indirizzi: &[std::net::SocketAddr],
        ) {
        }
        fn rispondi(
            &self,
            _da: &str,
            _domanda: Domanda,
        ) -> Result<rymembra_lontano::Risposta, AppError> {
            Err(AppError::new(ErrorCode::LontanoNonAssociato))
        }
        fn dimenticato(&self, _da: &str) {}
        fn evento(&self, _evento: rymembra_lontano::Evento) {}
    }

    fn nodo() -> Nodo {
        Nodo::avvia(
            rymembra_lontano::Impostazioni {
                identita: rymembra_lontano::Identita::nuova(),
                rete: rymembra_lontano::Rete::SoloQuesta,
                io: rymembra_lontano::Presentazione::default(),
                presenza: rymembra_lontano::Presenza::default(),
            },
            Arc::new(Nessuno),
        )
        .unwrap()
    }

    fn invito_ipc() -> InvitoIpc {
        InvitoIpc {
            link: "rymembra://collega/x".to_owned(),
            lato: 1,
            moduli: "1".to_owned(),
            scade_ms: 0,
        }
    }

    #[test]
    fn la_finestra_nel_vassoio_annulla_il_qr() {
        // Decisione D-30: il QR restava valido per i suoi dieci minuti con la
        // finestra nel vassoio — la scheda che lo mostra resta montata — e
        // `rymembra/associa/1` aperto a chi lo presentava.
        let (posta, _cassetta) = filo::nuova_cassetta();
        let comune = Comune::nuovo(posta);
        *prendi(&comune.nodo) = Some(Arc::new(nodo()));
        let annullati = comune.qr_annullati.load(Ordering::Acquire);
        comune.nodo().unwrap().invita().unwrap();
        assert!(metti_il_qr(&comune, invito_ipc(), annullati));
        assert!(prendi(&comune.invito).is_some());

        annulla_il_qr(&comune);
        assert!(
            prendi(&comune.invito).is_none(),
            "il pannello non lo mostra"
        );
        assert!(
            !comune.nodo().unwrap().invito_attivo(),
            "e il nodo non lo accoglie"
        );
    }

    #[test]
    fn un_qr_che_nasce_mentre_la_finestra_va_nel_vassoio_non_vale() {
        let (posta, _cassetta) = filo::nuova_cassetta();
        let comune = Comune::nuovo(posta);
        *prendi(&comune.nodo) = Some(Arc::new(nodo()));
        // «Mostra il QR»: il comando comincia…
        let annullati = comune.qr_annullati.load(Ordering::Acquire);
        // …la finestra va nel vassoio mentre il nodo aspetta il relay…
        annulla_il_qr(&comune);
        // …e il QR nasce dopo.
        comune.nodo().unwrap().invita().unwrap();
        assert!(!metti_il_qr(&comune, invito_ipc(), annullati));
        assert!(prendi(&comune.invito).is_none());
        assert!(!comune.nodo().unwrap().invito_attivo());
    }

    #[test]
    fn un_link_si_esamina_prima_di_collegarsi() {
        let invito = rymembra_lontano::Invito {
            chiave: [7; 32],
            gettone: [9; 16],
            indirizzi: Vec::new(),
            relay: None,
            nome: Some(" PC\u{202E} di casa ".to_owned()),
        };
        let visto = esamina(&format!("  {}\n", invito.link())).unwrap();
        assert_eq!(visto.link, invito.link());
        assert_eq!(visto.nome.as_deref(), Some("PC di casa"), "ripulito");
        assert_eq!(visto.impronta, invito.impronta());
        assert!(!visto.impronta.is_empty());
        // Un nome che ripulito non dice niente non c'è.
        let muto = rymembra_lontano::Invito {
            nome: Some("\u{202E}\t".to_owned()),
            ..invito
        };
        assert_eq!(esamina(&muto.link()).unwrap().nome, None);
        assert_eq!(
            esamina("https://esempio.it").unwrap_err().code().kind(),
            ErrorCodeKind::LontanoLinkNonValido
        );
    }

    // ── un link alla volta, e il primo vince (S4-4) ─────────────────────────

    /// Una pagina web che lancia `rymembra://collega/…` in sequenza non deve
    /// sostituire il dialogo di quello che l'utente sta leggendo — nome dichiarato
    /// uguale, impronta diversa — né portare la finestra davanti per quelli che il
    /// nucleo scarta.
    #[test]
    fn il_primo_link_vince_e_quelli_dopo_si_scartano() {
        let libero = PostoDelLink::default();
        assert_eq!(arrivo_del_link(&libero, true), Arrivo::Preso);
        // Uno aspetta che la pagina lo prenda.
        let in_attesa = PostoDelLink {
            in_attesa: Some("rymembra://collega/A".to_owned()),
            nel_dialogo: false,
        };
        assert_eq!(arrivo_del_link(&in_attesa, true), Arrivo::GiaUno);
        // La pagina l'ha preso e il dialogo è aperto.
        let nel_dialogo = PostoDelLink {
            in_attesa: None,
            nel_dialogo: true,
        };
        assert_eq!(arrivo_del_link(&nel_dialogo, true), Arrivo::GiaUno);
    }

    #[test]
    fn quel_che_non_e_un_link_non_si_prende_e_non_alza_niente() {
        for posto in [
            PostoDelLink::default(),
            PostoDelLink {
                in_attesa: Some("x".to_owned()),
                nel_dialogo: true,
            },
        ] {
            assert_eq!(arrivo_del_link(&posto, false), Arrivo::NonELink);
        }
    }

    /// Solo un link preso alza la finestra, e il dialogo si dice al nucleo.
    #[test]
    fn solo_un_link_preso_alza_la_finestra_e_il_dialogo_si_azzera_con_la_pagina() {
        let comparsa = include_str!("../comparsa.rs");
        let comparsa = comparsa.split("#[cfg(test)]").next().unwrap_or(comparsa);
        let con_link = comparsa
            .split("SecondoAvvio::ConLink(link) => {")
            .nth(1)
            .and_then(|resto| resto.split("\n        }\n").next())
            .unwrap_or_default();
        assert!(
            con_link.contains("if crate::lontano::link_arrivato(app, &link)"),
            "un link scartato alza la finestra lo stesso"
        );
        let nascosta = include_str!("mod.rs")
            .split("pub fn finestra_nascosta(")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        assert!(nascosta.contains("nel_dialogo = false"));
        let lib = include_str!("../lib.rs");
        assert!(lib.contains("lontano::lontano_link_dialogo,"));
    }
}
