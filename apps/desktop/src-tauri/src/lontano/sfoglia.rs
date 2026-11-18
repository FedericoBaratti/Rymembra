//! La rete delle righe di un altro dispositivo: la sorgente a blocchi con cui si
//! suonano, «Solo Wi-Fi», gli ascolti mandati al proprietario, e la cartella delle
//! copertine che una versione di prima teneva in cache.
//!
//! «Sfoglia <dispositivo>» era qui: la pagina della finestra e i comandi con cui
//! chiedeva gli elenchi all'altro. Con la libreria unica (`specchio.rs`) i brani
//! dell'altro sono righe di `tracks`, e quella pagina è un filtro: non c'è più
//! niente da chiedere a pagine, e le voci «lontane» del registro del lettore se
//! ne sono andate con lei. Il server (`servizio.rs`) risponde ancora alle
//! domande di elenco: un dispositivo con una versione di prima sfoglia **questo**
//! con quelle.
//!
//! # L'ascolto
//!
//! Una riga di un altro dispositivo si apre in rete con [`DaLontano`], una
//! sorgente a blocchi come quella d'«Ascolta insieme» sotto la stessa finestra
//! scorrevole dei cataloghi (`apri_in_rete`, usato da `specchio::apri_riga`).
//! Niente cache dei brani: si è scelto così, e un brano riascoltato si richiede.
//!
//! # Gli ascolti
//!
//! Misurati qui, contati là: [`ascoltato_qui`] li mette in fila nella
//! libreria, e [`manda_gli_ascolti`] li manda quando l'altro risponde. La
//! chiave di ogni ascolto rende innocuo un invio ripetuto.
//!
//! # «Solo Wi-Fi»
//!
//! Su una rete a consumo — i dati mobili del telefono — un brano dell'altro
//! si apre solo con «Usa i dati mobili» acceso. Con «Chiedi», il comando
//! risponde `lontano.reteAConsumo` e la finestra chiede; la risposta diventa
//! l'impostazione. Gli elenchi e le copertine passano sempre: pesano poco, e
//! senza non si saprebbe cosa scegliere (le copertine si chiedono in
//! `specchio::copertina_di_una_riga`). Su Windows la rete non è mai a
//! consumo. La stessa regola vale per «Ascolta insieme» via internet
//! (`lontano::insieme::dati_permessi`): entrare, accettare, invitare e
//! ammettere su una connessione del collegamento.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rymembra_app::dispositivi::{self, AscoltoLontano, DatiMobili};
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::listen::Listen;
use rymembra_lontano::protocollo::MASSIMO_PEZZO;
use rymembra_lontano::{Domanda, Risposta};
use rymembra_net::flusso::Sorgente;
use rymembra_net::http::Pezzo;
use tauri::AppHandle;

use super::comune;
use super::filo::{ATTESA_LIBRERIA, Comune, Posta, in_libreria, prendi};
use crate::nota;
use crate::riproduzione::insieme::ApriInRete;

/// Quanto si chiede all'altro per volta, e quanto si aspetta un salto: vedi
/// `crate::insieme::flusso`, che fa lo stesso per la stessa ragione.
const BLOCCO: u64 = 256 * 1024;

/// Quanto si aspetta un blocco. Via relay, sui dati mobili, un quarto di
/// mebibyte può chiedere qualche secondo; oltre, la rete non c'è.
const ATTESA_BLOCCO: Duration = Duration::from_secs(20);

/// Quanto si aspetta l'altro per un ascolto.
const ATTESA_ASCOLTO: Duration = Duration::from_secs(10);

// ── la sorgente ─────────────────────────────────────────────────────────────

/// Chi legge i pezzi di un brano dell'altro: il nodo; nelle prove, un file in
/// memoria. Restituisce i byte, la dimensione del file e la sua versione.
pub(super) trait Pezzi: Send + Sync + std::fmt::Debug + 'static {
    fn pezzo(&self, brano: i64, da: u64, quanti: u64) -> Result<(Vec<u8>, u64, String), AppError>;
}

/// Il nodo, preso a ogni blocco e lasciato subito: spento «Raggiungibile da
/// internet», il filo lo deve poter fermare davvero, e una sorgente aperta
/// che lo tenesse lo terrebbe acceso finché il brano non finisce.
pub(super) struct ViaNodo {
    pub(super) comune: Arc<Comune>,
    pub(super) chiave: String,
}

impl std::fmt::Debug for ViaNodo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViaNodo").finish_non_exhaustive()
    }
}

impl Pezzi for ViaNodo {
    fn pezzo(&self, brano: i64, da: u64, quanti: u64) -> Result<(Vec<u8>, u64, String), AppError> {
        // Il cancello anche da questa parte, a ogni blocco: dopo «Dimentica»
        // il brano di quel dispositivo che stava suonando gli chiedeva ancora
        // i pezzi, e `connessione()` gliene riapriva una (reperto R01-5).
        if !self.comune.associato(&self.chiave) {
            return Err(AppError::new(ErrorCode::LontanoNonAssociato));
        }
        let nodo = self
            .comune
            .nodo()
            .ok_or_else(|| AppError::new(ErrorCode::LontanoSpento))?;
        match nodo.chiedi(
            &self.chiave,
            Domanda::Leggi { brano, da, quanti },
            ATTESA_BLOCCO,
        )? {
            Risposta::Byte {
                byte,
                totale,
                versione,
            } => Ok((byte, totale, versione)),
            _ => Err(AppError::new(ErrorCode::LontanoProtocollo)
                .with_cause("a una lettura non si risponde con dei byte")),
        }
    }
}

/// Finché c'è, il nodo del telefono resta acceso anche a schermo spento: vedi
/// `Comune::in_volo`.
pub(super) struct InVolo(Arc<Comune>);

impl InVolo {
    pub(super) fn nuovo(comune: Arc<Comune>) -> Self {
        comune.in_volo.fetch_add(1, Ordering::Relaxed);
        tieni_il_wifi(&comune);
        Self(comune)
    }
}

impl Drop for InVolo {
    fn drop(&mut self) {
        self.0.in_volo.fetch_sub(1, Ordering::Relaxed);
        tieni_il_wifi(&self.0);
    }
}

/// Sul telefono, finché arriva qualcosa dall'altro il Wi-Fi resta sveglio
/// (`rymembra_android::sistema::wifi`). Sotto un lucchetto che rilegge il
/// conto: due fili che aprono e chiudono insieme non lasciano il Wi-Fi nello
/// stato del primo dei due.
#[cfg(target_os = "android")]
fn tieni_il_wifi(comune: &Comune) {
    static PRESO: Mutex<bool> = Mutex::new(false);
    let mut preso = prendi(&PRESO);
    let voluto = comune.in_volo();
    if *preso == voluto {
        return;
    }
    match rymembra_android::sistema::wifi("lontano", voluto) {
        Ok(()) => *preso = voluto,
        Err(err) => nota!("[lontano] il lucchetto del Wi-Fi non si cambia: {err}"),
    }
}

#[cfg(not(target_os = "android"))]
const fn tieni_il_wifi(_: &Comune) {}

impl std::fmt::Debug for InVolo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InVolo")
    }
}

/// I byte di un brano dell'altro, come [`Sorgente`]: vedi il `//!`.
#[derive(Debug)]
struct DaLontano<P: Pezzi = ViaNodo> {
    pezzi: P,
    brano: i64,
    /// La versione del file, fissata dalla prima risposta e comune a tutte le
    /// aperture della stessa voce: un file cambiato nel frattempo non si cuce
    /// con quello di prima.
    ///
    /// Non quella dell'elenco (`BranoRemoto::versione`): viene dal database
    /// dell'altro, che può essere indietro rispetto al disco — dopo «Rileggi
    /// tutti i tag» (`date_modified = -1`), o con un file ritoccato da fuori
    /// dopo l'ultima scansione — e ogni lettura sarebbe `fileCambiato`.
    versione: Arc<Mutex<Option<String>>>,
    _in_volo: Option<InVolo>,
}

impl<P: Pezzi> DaLontano<P> {
    /// Come `crate::insieme::flusso::DaPari::leggi`: a blocchi, chiedendo
    /// `avanti()` fra l'uno e l'altro. Gli errori passano senza causa: il
    /// lettore li scrive nel diario, e una causa di rete può dire un indirizzo.
    fn leggi(&self, da: u64, quanti: u64, avanti: &dyn Fn() -> bool) -> Result<Pezzo, AppError> {
        let capienza = usize::try_from(quanti.min(MASSIMO_PEZZO)).unwrap_or(0);
        let mut byte = Vec::with_capacity(capienza);
        let mut totale = None;
        let mut letti = 0_u64;
        while letti < quanti {
            if !avanti() {
                return Err(AppError::new(ErrorCode::InternalAborted {
                    what: Some("lettura in rete".to_owned()),
                }));
            }
            let chiesti = quanti.saturating_sub(letti).min(BLOCCO);
            let (mut blocco, dimensione, versione) = self
                .pezzi
                .pezzo(self.brano, da.saturating_add(letti), chiesti)
                .map_err(|err| AppError::new(err.code().clone()))?;
            {
                let mut fissata = prendi(&self.versione);
                match fissata.as_deref() {
                    Some(prima) if prima != versione => {
                        return Err(AppError::new(ErrorCode::LontanoFileCambiato));
                    }
                    Some(_) => {}
                    None => *fissata = Some(versione),
                }
            }
            totale = Some(dimensione);
            // Mai più di quanto chiesto: l'altro non decide quanto si
            // alloca qui.
            blocco.truncate(usize::try_from(chiesti).unwrap_or(usize::MAX));
            let arrivati = u64::try_from(blocco.len()).unwrap_or(u64::MAX);
            byte.extend_from_slice(&blocco);
            letti = letti.saturating_add(arrivati);
            // Meno di quanti chiesti è la fine del file.
            if arrivati < chiesti {
                break;
            }
        }
        Ok(Pezzo {
            byte,
            totale,
            url_finale: None,
        })
    }
}

impl<P: Pezzi> Sorgente for DaLontano<P> {
    fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
        self.leggi(da, quanti, &|| true)
    }

    fn pezzo_finche(
        &self,
        da: u64,
        quanti: u64,
        avanti: &dyn Fn() -> bool,
    ) -> Result<Pezzo, AppError> {
        self.leggi(da, quanti, avanti)
    }
}

// ── «Solo Wi-Fi» ────────────────────────────────────────────────────────────

/// La rete di adesso è a consumo. Se il telefono non risponde si dice di no:
/// un ponte rotto non deve togliere la musica, e il sistema stesso avvisa dei
/// dati che corrono.
#[cfg(target_os = "android")]
pub(crate) fn rete_a_consumo() -> bool {
    rymembra_android::sistema::rete_a_consumo().unwrap_or_else(|err| {
        nota!("[lontano] la rete a consumo non si sa: {err}");
        false
    })
}

#[cfg(not(target_os = "android"))]
pub(crate) const fn rete_a_consumo() -> bool {
    false
}

/// Un brano dell'altro si può aprire su questa rete: vedi il `//!`.
pub(super) fn dati_permessi(comune: &Comune) -> Result<(), AppError> {
    if !rete_a_consumo() {
        return Ok(());
    }
    match prendi(&comune.vetrina).dati_mobili {
        DatiMobili::Si => Ok(()),
        DatiMobili::Chiedi | DatiMobili::No => Err(AppError::new(ErrorCode::LontanoReteAConsumo)),
    }
}

/// La rete di adesso, per scegliere quale copia di un brano si suona: vedi
/// `rymembra_app::specchio::scelta::scegli_con_la_rete`.
pub(super) fn rete_di_adesso(comune: &Comune) -> rymembra_app::specchio::scelta::Rete {
    use rymembra_app::specchio::scelta::Rete;
    if rete_a_consumo() {
        Rete::AConsumo(prendi(&comune.vetrina).dati_mobili)
    } else {
        Rete::Libera
    }
}

/// L'estensione di un brano dell'altro, se ha la forma di un'estensione.
pub(super) fn estensione_buona(estensione: &str) -> Option<String> {
    let estensione = estensione
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    (!estensione.is_empty()
        && estensione.len() <= 8
        && estensione.bytes().all(|b| b.is_ascii_alphanumeric()))
    .then_some(estensione)
}

/// Una chiave di dispositivo: sessantaquattro cifre esadecimali minuscole.
fn e_una_chiave(chiave: &str) -> bool {
    chiave.len() == 64
        && chiave
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// Come si aprono i byte di un brano di un altro dispositivo: la sorgente a
/// blocchi sul nodo, dopo aver guardato «Solo Wi-Fi». Per le voci di Sfoglia e
/// per le righe della libreria unica (`lontano://`), che si aprono allo
/// stesso modo.
pub(super) fn apri_in_rete(comune: &Arc<Comune>, dispositivo: &str, brano: i64) -> ApriInRete {
    let per_aprire = Arc::clone(comune);
    let chiave = dispositivo.to_owned();
    // Fissata alla prima apertura: vedi `DaLontano::versione`.
    let versione = Arc::new(Mutex::new(None::<String>));
    Arc::new(move || {
        // Si guarda adesso e non solo al gesto: la coda può arrivare a questo
        // brano dopo che il Wi-Fi se n'è andato.
        dati_permessi(&per_aprire)?;
        Ok(Box::new(DaLontano {
            pezzi: ViaNodo {
                comune: Arc::clone(&per_aprire),
                chiave: chiave.clone(),
            },
            brano,
            versione: Arc::clone(&versione),
            _in_volo: Some(InVolo::nuovo(Arc::clone(&per_aprire))),
        }) as Box<dyn Sorgente>)
    })
}

// ── gli ascolti ─────────────────────────────────────────────────────────────

/// Un ascolto fatto qui di un brano dell'altro: in fila, da mandargli.
/// Gira sul filo delle scritture del lettore.
pub fn ascoltato_qui(app: &AppHandle, dispositivo: &str, brano: i64, ascolto: &Listen) {
    let lontano = AscoltoLontano {
        brano,
        quando_ms: ascolto.started_at,
        ascoltato_ms: i64::try_from(ascolto.listened_ms).unwrap_or(i64::MAX),
        // Un brano non comincia due volte nello stesso millisecondo: l'inizio
        // e il brano bastano a distinguere ogni ascolto di qui, e chi lo riceve
        // ci mette davanti la chiave di questo dispositivo.
        chiave: format!("{}-{brano}", ascolto.started_at),
    };
    match in_libreria(app, ATTESA_LIBRERIA, |c| {
        dispositivi::ricorda_ascolto(c, dispositivo, &lontano)
    }) {
        Ok(()) => {
            if let Ok(comune) = comune(app) {
                comune.sveglia(Posta::Ascolti);
            }
        }
        Err(err) => nota!(
            "[lontano] un ascolto di un brano di un altro dispositivo non si ricorda codice={}",
            err.code().kind().code()
        ),
    }
}

/// C'è un giro di invii in corso.
static POSTINO: AtomicBool = AtomicBool::new(false);

/// Un invio non è riuscito, o un giro non è partito: il filo riprova al
/// prossimo giro.
static DA_RIPROVARE: AtomicBool = AtomicBool::new(false);

/// Il filo deve riprovare gli invii; lo dice una volta sola.
pub(super) fn da_riprovare() -> bool {
    DA_RIPROVARE.swap(false, Ordering::AcqRel)
}

/// Manda gli ascolti in fila ai dispositivi che rispondono adesso — a `solo`,
/// se è detto. Su un filo suo: il filo del collegamento non aspetta la rete.
pub(super) fn manda_gli_ascolti(app: &AppHandle, comune: &Arc<Comune>, solo: Option<&str>) {
    if comune.nodo().is_none() {
        return;
    }
    let presenti: Vec<String> = match solo {
        Some(chiave) => vec![chiave.to_owned()],
        None => prendi(&comune.presenze).keys().cloned().collect(),
    };
    if presenti.is_empty() {
        return;
    }
    if POSTINO.swap(true, Ordering::AcqRel) {
        DA_RIPROVARE.store(true, Ordering::Release);
        return;
    }
    let app = app.clone();
    let comune = Arc::clone(comune);
    let partito = std::thread::Builder::new()
        .name("rymembra-lontano-ascolti".to_owned())
        .spawn(move || {
            for chiave in presenti {
                if !manda_a(&app, &comune, &chiave) {
                    DA_RIPROVARE.store(true, Ordering::Release);
                }
            }
            POSTINO.store(false, Ordering::Release);
        });
    if let Err(err) = partito {
        nota!("[lontano] il filo degli ascolti non parte: {err}");
        POSTINO.store(false, Ordering::Release);
        DA_RIPROVARE.store(true, Ordering::Release);
    }
}

/// Manda a `chiave` i suoi ascolti in fila, dal più vecchio. `false` se si è
/// fermato prima della fine.
///
/// Solo a un dispositivo ancora collegato, guardato a ogni ascolto: a uno
/// dimenticato non si chiede più niente, e non è un invio da riprovare
/// (reperto R01-5).
fn manda_a(app: &AppHandle, comune: &Comune, chiave: &str) -> bool {
    if !comune.associato(chiave) {
        return true;
    }
    let da_mandare = match in_libreria(app, ATTESA_LIBRERIA, |c| {
        dispositivi::ascolti_da_mandare(c, chiave)
    }) {
        Ok(da_mandare) => da_mandare,
        Err(_) => return false,
    };
    for ascolto in da_mandare {
        if !comune.associato(chiave) {
            return true;
        }
        let Some(nodo) = comune.nodo() else {
            return false;
        };
        let esito = nodo.chiedi(
            chiave,
            Domanda::Ascoltato {
                brano: ascolto.brano,
                quando_ms: ascolto.quando_ms,
                ascoltato_ms: ascolto.ascoltato_ms,
                chiave: ascolto.chiave.clone(),
            },
            ATTESA_ASCOLTO,
        );
        drop(nodo);
        if let Err(err) = esito {
            nota!(
                "[lontano] un ascolto non è arrivato codice={}: si riprova",
                err.code().kind().code()
            );
            return false;
        }
        if let Err(err) = in_libreria(app, ATTESA_LIBRERIA, |c| {
            dispositivi::ascolto_arrivato(c, &ascolto.chiave)
        }) {
            // Resta in fila: rimandato, l'altro lo riconosce dalla chiave.
            nota!(
                "[lontano] un ascolto arrivato non si toglie codice={}",
                err.code().kind().code()
            );
            return false;
        }
    }
    true
}

// ── le copertine ────────────────────────────────────────────────────────────
//
// Di copertine, qui, resta poco: la cartella dove stanno e il modo di toglierle.
// Le copertine di un altro dispositivo non passano più di qui: per le righe della
// libreria unica si chiedono all'altro una volta e si tengono nello store, sotto
// la loro impronta (`specchio::copertina_di_una_riga`). La cartella
// `copertine-lontane` serve adesso alle copertine delle voci di una sessione di
// «Ascolta insieme» (`insieme/su_file.rs`) — che Android serve da una seconda
// radice, `Copertine.kt` — e basta: quel che una versione di prima, con
// «Sfoglia», vi aveva lasciato si toglie una volta all'avvio
// ([`ripulisci_le_copertine_di_prima`]).

/// Dove stanno le copertine lontane: accanto alla cache dei brani del cloud,
/// e per la stessa ragione fuori dai dati che vanno in giro coi profili. La
/// regola è quella di [`crate::cartella_di_cache`]; `None` vuol dire nessuna
/// cache.
pub(crate) fn cartella_copertine(app: &AppHandle) -> Option<PathBuf> {
    crate::cartella_di_cache(app, "copertine-lontane")
}

/// Toglie dalla cache le copertine di un dispositivo: la sua cartella
/// `copertine-lontane/<dispositivo>/`, quella che «Sfoglia» riempiva.
///
/// Per «Dimentica» e «Dimenticami». Con la libreria unica nessuno ci scrive più,
/// ma una cartella lasciata da prima può esserci ancora, e chi non passa da un
/// cancello — la scheda di ripresa, il fornitore di Android (reperto di F1) — la
/// leggerebbe: dimenticato il dispositivo, non deve più uscirne niente.
pub(crate) fn togli_le_copertine_di(app: &AppHandle, dispositivo: &str) {
    if let Some(cartella) = cartella_copertine(app) {
        togli_la_cartella_di(&cartella, dispositivo);
    }
}

/// [`togli_le_copertine_di`] dentro `cartella`. `true` se c'era e se n'è
/// andata. La chiave dev'essere di 64 cifre esadecimali minuscole, e solo
/// allora diventa un nome di cartella: niente `..`, niente separatori.
fn togli_la_cartella_di(cartella: &Path, dispositivo: &str) -> bool {
    if !e_una_chiave(dispositivo) {
        return false;
    }
    match std::fs::remove_dir_all(cartella.join(dispositivo)) {
        Ok(()) => true,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => {
            nota!("[lontano] le copertine di un dispositivo dimenticato non si tolgono: {err}");
            false
        }
    }
}

/// Quel che [`ripulisci_la_cartella`] ha tolto.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Ripulita {
    /// I file piatti.
    file: usize,
    /// Le cartelle intere, coi loro file.
    cartelle: usize,
}

/// Una volta per processo, **all'avvio**: toglie da `copertine-lontane` quel che
/// le versioni di prima vi hanno lasciato e che adesso non serve a nessuno.
///
/// La cartella aveva una cache con la sua potatura, per le copertine di
/// «Sfoglia». Con la libreria unica quelle copertine stanno nello store, la
/// potatura è andata, e nessuno toglieva più i file piatti
/// `<dispositivo>-<impronta>.jpg` della 3.2.1 (fino a 50 MiB), né le cartelle
/// `<dispositivo>/` della versione dopo, né la cartella di zeri — le copertine di
/// una sessione di «Ascolta insieme» — rimasta da un processo morto a metà
/// sessione (`insieme/su_file.rs`). All'avvio non può esserci una sessione, e
/// vale comunque la domanda: con una in corso non si tocca niente
/// ([`si_ripulisce`]).
///
/// Va su un filo suo, per non fermare l'avvio: sono pochi file, ma su un disco
/// lento sono secondi.
pub(crate) fn ripulisci_le_copertine_di_prima(app: &AppHandle) {
    let sessione_in_corso = crate::insieme::ruolo(app) != crate::insieme::Ruolo::Fuori;
    if !si_ripulisce(sessione_in_corso, &RIPULITA) {
        return;
    }
    let Some(cartella) = cartella_copertine(app) else {
        return;
    };
    drop(tauri::async_runtime::spawn_blocking(move || {
        let ripulita = ripulisci_la_cartella(&cartella);
        if ripulita != Ripulita::default() {
            nota!(
                "[lontano] copertine lontane di una versione di prima tolte: {} file, {} cartelle",
                ripulita.file,
                ripulita.cartelle
            );
        }
    }));
}

/// Se [`ripulisci_le_copertine_di_prima`] ha già fatto, o ha rinunciato.
static RIPULITA: AtomicBool = AtomicBool::new(false);

/// Si ripulisce adesso? Una volta per processo, e mai con una sessione in corso:
/// con una sessione non si segna nemmeno fatta, e la prima occasione senza la
/// farà. Pura, per provarla senza un'applicazione.
fn si_ripulisce(sessione_in_corso: bool, fatta: &AtomicBool) -> bool {
    !sessione_in_corso && !fatta.swap(true, Ordering::AcqRel)
}

/// [`ripulisci_le_copertine_di_prima`] dentro `cartella`.
///
/// Toglie **solo** quel che una versione di Rymembra ci ha messo: i file piatti
/// `.jpg` e `.part` — `<dispositivo>-<impronta>.jpg`, `<impronta>.jpg` — e le
/// cartelle col nome di una chiave di dispositivo (64 cifre esadecimali
/// minuscole: quelle dei dispositivi, e quella di zeri delle sessioni). Quel che
/// ha un altro nome resta com'è, e i collegamenti simbolici non si seguono né si
/// toccano: `file_type` non li risolve. Fuori da `cartella` non si esce mai: i
/// nomi sono quelli letti qui, non composti da fuori.
///
/// Una cartella che non c'è non è un guasto. Un file che non si toglie — su
/// Windows, tenuto aperto da qualcuno — si dice nel diario e resta: alla prossima.
fn ripulisci_la_cartella(cartella: &Path) -> Ripulita {
    let mut ripulita = Ripulita::default();
    let Ok(voci) = std::fs::read_dir(cartella) else {
        return ripulita;
    };
    for voce in voci.filter_map(Result::ok) {
        let Ok(tipo) = voce.file_type() else {
            continue;
        };
        let nome = voce.file_name();
        let nome = nome.to_string_lossy();
        let da_togliere = if tipo.is_file() {
            nome.ends_with(".jpg") || nome.ends_with(".part")
        } else {
            tipo.is_dir() && e_una_chiave(&nome)
        };
        if !da_togliere {
            continue;
        }
        let esito = if tipo.is_file() {
            std::fs::remove_file(voce.path())
        } else {
            std::fs::remove_dir_all(voce.path())
        };
        match esito {
            Ok(()) if tipo.is_file() => ripulita.file = ripulita.file.saturating_add(1),
            Ok(()) => ripulita.cartelle = ripulita.cartelle.saturating_add(1),
            Err(err) => nota!("[lontano] una copertina lontana di prima non si toglie: {err}"),
        }
    }
    ripulita
}

#[cfg(test)]
mod prove {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    /// Un file finto in memoria, con la sua versione.
    #[derive(Debug)]
    struct Finto {
        byte: Vec<u8>,
        versione: Mutex<String>,
        richieste: Mutex<Vec<(u64, u64)>>,
    }

    impl Finto {
        fn di(n: usize) -> Self {
            Self {
                byte: (0..n).map(|i| u8::try_from(i % 251).unwrap()).collect(),
                versione: Mutex::new("v1".to_owned()),
                richieste: Mutex::new(Vec::new()),
            }
        }
    }

    impl Pezzi for Arc<Finto> {
        fn pezzo(
            &self,
            _brano: i64,
            da: u64,
            quanti: u64,
        ) -> Result<(Vec<u8>, u64, String), AppError> {
            self.richieste.lock().unwrap().push((da, quanti));
            let inizio = usize::try_from(da).unwrap().min(self.byte.len());
            let fine = inizio
                .saturating_add(usize::try_from(quanti).unwrap())
                .min(self.byte.len());
            Ok((
                self.byte[inizio..fine].to_vec(),
                u64::try_from(self.byte.len()).unwrap(),
                self.versione.lock().unwrap().clone(),
            ))
        }
    }

    fn sorgente(finto: &Arc<Finto>) -> DaLontano<Arc<Finto>> {
        DaLontano {
            pezzi: Arc::clone(finto),
            brano: 7,
            versione: Arc::new(Mutex::new(Some("v1".to_owned()))),
            _in_volo: None,
        }
    }

    /// Una lettura grande va a blocchi, e torna intera.
    #[test]
    fn si_legge_a_blocchi() {
        let finto = Arc::new(Finto::di(700 * 1024));
        let pezzo = sorgente(&finto).pezzo(10, 600 * 1024).unwrap();
        assert_eq!(pezzo.byte.len(), 600 * 1024);
        assert_eq!(pezzo.byte.as_slice(), &finto.byte[10..10 + 600 * 1024]);
        assert_eq!(pezzo.totale, Some(700 * 1024));
        let richieste = finto.richieste.lock().unwrap().clone();
        assert_eq!(richieste.len(), 3);
        assert!(richieste.iter().all(|(_, quanti)| *quanti <= BLOCCO));
    }

    /// La fine del file ferma la lettura senza una richiesta vuota.
    #[test]
    fn la_fine_del_file_non_e_un_guasto() {
        let finto = Arc::new(Finto::di(1000));
        let pezzo = sorgente(&finto).pezzo(900, 1024 * 1024).unwrap();
        assert_eq!(pezzo.byte.len(), 100);
        assert_eq!(finto.richieste.lock().unwrap().len(), 1);
    }

    /// Chi ha smesso di volere i byte non aspetta il blocco dopo.
    #[test]
    fn un_salto_ferma_la_lettura() {
        let finto = Arc::new(Finto::di(2 * 1024 * 1024));
        let chiesti = AtomicUsize::new(0);
        let esito = sorgente(&finto).pezzo_finche(0, 2 * 1024 * 1024, &|| {
            chiesti.fetch_add(1, Ordering::Relaxed) < 1
        });
        assert!(esito.is_err());
        assert_eq!(finto.richieste.lock().unwrap().len(), 1);
    }

    /// Un file cambiato fra un blocco e l'altro non si cuce.
    #[test]
    fn un_file_cambiato_si_dice() {
        let finto = Arc::new(Finto::di(1000));
        *finto.versione.lock().unwrap() = "v2".to_owned();
        let errore = sorgente(&finto).pezzo(0, 100).unwrap_err();
        assert_eq!(
            errore.code().kind(),
            rymembra_domain::errors::ErrorCodeKind::LontanoFileCambiato
        );
    }

    /// La versione dell'elenco può essere vecchia: conta quella del disco,
    /// fissata alla prima lettura e comune alle aperture della voce.
    #[test]
    fn la_versione_si_fissa_alla_prima_lettura() {
        let finto = Arc::new(Finto::di(1000));
        *finto.versione.lock().unwrap() = "v9".to_owned();
        let fissata = Arc::new(Mutex::new(None));
        let apri = || DaLontano {
            pezzi: Arc::clone(&finto),
            brano: 7,
            versione: Arc::clone(&fissata),
            _in_volo: None,
        };
        assert_eq!(apri().pezzo(0, 100).unwrap().byte.len(), 100);
        assert_eq!(fissata.lock().unwrap().as_deref(), Some("v9"));
        // Un'altra apertura della stessa voce, dopo un salto: stessa versione.
        assert_eq!(apri().pezzo(500, 100).unwrap().byte.len(), 100);
        // Il file cambia sotto: non si cuce.
        *finto.versione.lock().unwrap() = "v10".to_owned();
        assert_eq!(
            apri().pezzo(600, 100).unwrap_err().code().kind(),
            rymembra_domain::errors::ErrorCodeKind::LontanoFileCambiato
        );
    }

    #[test]
    fn le_estensioni() {
        assert_eq!(estensione_buona("FLAC").as_deref(), Some("flac"));
        assert_eq!(estensione_buona(".mp3").as_deref(), Some("mp3"));
        assert_eq!(estensione_buona(""), None);
        assert_eq!(estensione_buona("a/b"), None);
        assert_eq!(estensione_buona("lunghissima"), None);
    }

    #[test]
    fn una_voce_di_un_dispositivo_dimenticato_non_chiede_piu() {
        // Reperto R01-5: dopo «Dimentica» il brano di quel dispositivo che
        // stava suonando gli chiedeva ancora i pezzi, e `connessione()` gliene
        // riapriva una.
        let (posta, _cassetta) = std::sync::mpsc::channel();
        let comune = Arc::new(Comune::nuovo(posta));
        let chiave = "c".repeat(64);
        comune.ammetti(&chiave, false);
        let voce = ViaNodo {
            comune: Arc::clone(&comune),
            chiave,
        };
        assert_eq!(
            voce.pezzo(1, 0, 1024).map_err(|err| err.code().kind()),
            Err(rymembra_domain::errors::ErrorCodeKind::LontanoNonAssociato)
        );
    }

    /// «Dimentica» e «Dimenticami» tolgono la cartella delle copertine del
    /// dispositivo, e solo la sua.
    #[test]
    fn dimenticare_un_dispositivo_toglie_le_sue_copertine_e_solo_quelle() {
        let cartella = std::env::temp_dir().join(format!(
            "rymembra-prova-togli-copertine-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&cartella);
        let (a, b) = ("a".repeat(64), "b".repeat(64));
        for dispositivo in [&a, &b] {
            let dentro = cartella.join(dispositivo);
            std::fs::create_dir_all(&dentro).unwrap();
            std::fs::write(dentro.join("0123456789abcdef.jpg"), b"x").unwrap();
        }
        std::fs::write(cartella.join("vecchia.jpg"), b"x").unwrap();

        assert!(togli_la_cartella_di(&cartella, &a));
        assert!(!cartella.join(&a).exists(), "le sue copertine se ne vanno");
        assert!(
            cartella.join(&b).join("0123456789abcdef.jpg").exists(),
            "quelle di un altro restano"
        );
        assert!(cartella.join("vecchia.jpg").exists());
        assert!(
            !togli_la_cartella_di(&cartella, &a),
            "due volte è una volta"
        );

        // Una chiave che non è una chiave non diventa un percorso.
        for storta in ["", "..", "../b", &b.to_uppercase(), &"b".repeat(63)] {
            assert!(!togli_la_cartella_di(&cartella, storta), "{storta:?}");
        }
        assert!(cartella.join(&b).exists());
        let _ = std::fs::remove_dir_all(&cartella);
    }

    /// FR2: la cartella `copertine-lontane` non si pota più — la potatura c'era
    /// per le copertine di «Sfoglia» — e quel che le versioni di prima vi hanno
    /// lasciato resta: i file piatti `<dispositivo>-<impronta>.jpg`, le cartelle
    /// `<dispositivo>/` (le copertine delle righe di un altro dispositivo stanno
    /// nello store), e la cartella di zeri delle sessioni di un processo morto.
    /// Si toglie una volta all'avvio, e solo quello: niente di quel che sta fuori
    /// dalla cartella, e niente di quel che nessuna versione ci ha mai messo.
    #[test]
    fn la_cartella_delle_copertine_lontane_si_ripulisce_di_quel_che_ci_hanno_lasciato() {
        let fuori = tempfile::tempdir().unwrap();
        let cartella = fuori.path().join("copertine-lontane");
        let (a, b) = ("a".repeat(64), "b".repeat(64));
        let zeri = "0".repeat(64);
        let impronta = "0123456789abcdef";
        // Quel che c'era: i file piatti di tre forme, le cartelle di due dispositivi, e
        // quella di zeri con una copertina di sessione e un `.part` a metà.
        std::fs::create_dir_all(&cartella).unwrap();
        for nome in [
            format!("{a}-{impronta}.jpg"),
            format!("{impronta}.jpg"),
            format!("{b}-{impronta}.part"),
        ] {
            std::fs::write(cartella.join(nome), b"x").unwrap();
        }
        for dispositivo in [&a, &b, &zeri] {
            let dentro = cartella.join(dispositivo);
            std::fs::create_dir_all(&dentro).unwrap();
            std::fs::write(dentro.join(format!("{impronta}.jpg")), b"x").unwrap();
        }
        std::fs::write(cartella.join(&zeri).join("abc.part"), b"x").unwrap();
        // Quel che nessuna versione ci ha messo, e che non è nostro da togliere.
        std::fs::write(cartella.join("appunti.txt"), b"x").unwrap();
        std::fs::create_dir_all(cartella.join("una-cartella")).unwrap();
        std::fs::write(cartella.join("una-cartella").join("dentro.jpg"), b"x").unwrap();
        let quasi = "c".repeat(63);
        std::fs::create_dir_all(cartella.join(&quasi)).unwrap();
        std::fs::write(cartella.join(&quasi).join("dentro.jpg"), b"x").unwrap();
        // E fuori dalla cartella, accanto a lei e dentro un'altra.
        std::fs::write(fuori.path().join("copertine.jpg"), b"x").unwrap();
        std::fs::create_dir_all(fuori.path().join("altra")).unwrap();
        std::fs::write(
            fuori
                .path()
                .join("altra")
                .join(format!("{a}-{impronta}.jpg")),
            b"x",
        )
        .unwrap();

        let ripulita = ripulisci_la_cartella(&cartella);
        assert_eq!(
            ripulita,
            Ripulita {
                file: 3,
                cartelle: 3
            }
        );

        let rimasti: std::collections::BTreeSet<String> = std::fs::read_dir(&cartella)
            .unwrap()
            .map(|v| v.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            rimasti,
            [
                "appunti.txt".to_owned(),
                "una-cartella".to_owned(),
                quasi.clone()
            ]
            .into_iter()
            .collect(),
            "resta quel che nessuna versione ci ha messo"
        );
        assert!(cartella.join("una-cartella").join("dentro.jpg").exists());
        assert!(cartella.join(&quasi).join("dentro.jpg").exists());
        assert!(
            fuori.path().join("copertine.jpg").exists(),
            "fuori non si tocca"
        );
        assert!(
            fuori
                .path()
                .join("altra")
                .join(format!("{a}-{impronta}.jpg"))
                .exists(),
            "nemmeno un'altra cartella"
        );

        // Una seconda volta non trova più niente, e una cartella che non c'è non è un guasto.
        assert_eq!(ripulisci_la_cartella(&cartella), Ripulita::default());
        assert_eq!(
            ripulisci_la_cartella(&fuori.path().join("non-c-e")),
            Ripulita::default()
        );
    }

    /// FR2: una volta per processo, e mai con una sessione in corso — le
    /// copertine di «Ascolta insieme» stanno proprio in quella cartella.
    #[test]
    fn la_pulizia_si_fa_una_volta_e_non_con_una_sessione_in_corso() {
        let fatta = AtomicBool::new(false);
        assert!(
            !si_ripulisce(true, &fatta),
            "con una sessione non si tocca niente"
        );
        assert!(
            !fatta.load(Ordering::Acquire),
            "e non si segna fatta: la prima occasione senza sessione la fa"
        );
        assert!(si_ripulisce(false, &fatta), "la prima volta sì");
        assert!(!si_ripulisce(false, &fatta), "poi più");
        assert!(!si_ripulisce(true, &fatta));
    }
}
