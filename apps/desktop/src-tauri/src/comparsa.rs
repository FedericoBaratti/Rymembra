//! Come compare la finestra, e come ricompare.
//!
//! # Il motore non ha bisogno della finestra
//!
//! Con il vassoio acceso la X **chiude** la finestra — la distrugge, con la sua
//! WebView2 — e il programma resta: la musica, la libreria, i fili di
//! sottofondo, le scorciatoie multimediali e l'icona nell'area di notifica sono
//! del nucleo, e nessuno di loro sta dentro la pagina. È il risparmio che il
//! secondo piano prometteva e non dava: una finestra solo nascosta teneva la
//! pagina viva, con i suoi processi `msedgewebview2` e la loro memoria. Vedi
//! `lib.rs` (`RunEvent::ExitRequested`) per come il programma sopravvive
//! all'ultima finestra.
//!
//! Quel che serve, allora, è un modo di **ricrearla**: la ricrea questo
//! modulo, dalla stessa configurazione da cui Tauri l'ha creata la prima volta
//! (`tauri.conf.json`), e la pagina riparte da capo e chiede al nucleo come
//! stanno le cose, come all'avvio. La chiamano:
//!
//! - «Mostra» del vassoio, e un clic sull'icona ([`crate::vassoio::mostra`]);
//! - un secondo avvio di Rymembra ([`secondo_avvio`]), con o senza link;
//! - un evento che vuole una persona davanti, come un invito di «Ascolta
//!   insieme» (`insieme::filo`).
//!
//! # Dov'era la finestra
//!
//! `tauri.conf.json` non ha una posizione, e dice 1280×820: una finestra
//! ricreata dalla configurazione nasceva sempre di quella misura, non ingrandita,
//! con la posizione a cascata di Windows — il vecchio `hide()` teneva la
//! geometria, e quando la finestra è diventata distruggibile si è perso. Ne
//! tiene memoria [`RICORDO`], aggiornata a eventi finiti ([`rileggi_se_mossa`]) e
//! alla richiesta di chiusura ([`alla_chiusura`]), l'ultimo momento in cui c'è
//! qualcosa da leggere; la rimette [`crea`] sulla finestra nuova, ancora
//! nascosta. Le impostazioni (`ui.window.geometry`, fuori dal profilo: sono i
//! monitor di questa macchina) servono al riavvio — [`ritrova_la_geometria`], e
//! [`ricorda_su_disco`] alla chiusura e all'uscita —. Quel che conta è nel
//! modulo `geometria`, senza finestre: dove rimettere (su un monitor che
//! esiste ancora) e cosa ricordare (il rettangolo **normale**, non quello
//! ingrandito).
//!
//! # L'avvio automatico: ridotta a icona, una volta
//!
//! Lanciata da Windows al login (`--avvio-automatico`, o l'attività di avvio del
//! Store) la finestra non deve piombare davanti a chi sta ancora aprendo il
//! suo lavoro: compare **ridotta a icona nella barra**. Ma solo la **prima**
//! volta: una finestra ricreata dal vassoio la vuole chi ha cliccato, e la
//! vuole davanti. Il «una volta» lo tiene [`PrimaComparsa`], e lo prende il
//! primo che mostra la finestra fra i tre che lo fanno — la pagina quando è
//! pronta (`pronto`), l'attesa di una migrazione (`finestra_mostra`) e la rete
//! di sicurezza dei due secondi — senza che si coordinino.
//!
//! # Quel che una finestra nuova deve riavere
//!
//! Non è solo la pagina. Ogni finestra ha le sue cose che si agganciano
//! all'`HWND`: la guardia dell'Alt da solo, i pulsanti nell'anteprima della
//! barra, il conto di `pagina::NASCOSTA`, la rete di sicurezza che la mostra
//! comunque. Le ridà tutte [`finestra_nata`], e **sta qui** e non nel `setup`
//! perché il `setup` gira una volta, e la finestra può nascere più d'una.
//! Le SMTC non ci sono: stanno su una finestra nativa del nucleo, e sopravvivono
//! (vedi `media::smtc`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use rymembra_app::preferenze::CHIAVE_GEOMETRIA_FINESTRA;
use rymembra_app::settings;
use tauri::Manager as _;

use crate::argomenti::{Argomenti, SecondoAvvio, secondo_avvio as decisione};
use crate::geometria::{self, Area, Geometria, Lettura, Misura, MisuraDellaFinestra, Rimessa};
use crate::stato::{Stato, con_libreria, con_libreria_entro};
use crate::{nota, spegnimento, vassoio};

/// L'etichetta della finestra, quella di `tauri.conf.json`.
const ETICHETTA: &str = "main";

/// Se la prossima comparsa della finestra dev'essere ridotta a icona, e vale una
/// volta sola.
#[derive(Debug, Default)]
pub struct PrimaComparsa(AtomicBool);

impl PrimaComparsa {
    /// Nessuna richiesta: la finestra compare normale.
    #[must_use]
    pub const fn nuova() -> Self {
        Self(AtomicBool::new(false))
    }

    /// Da qui in poi la prossima comparsa è ridotta a icona, o torna normale.
    pub fn imposta(&self, ridotta: bool) {
        self.0.store(ridotta, Ordering::Release);
    }

    /// La prossima comparsa è ridotta e non è ancora avvenuta? Come [`Self::prendi`]
    /// ma **senza consumarla**: chi lo chiede non è chi mostra la finestra.
    pub fn in_attesa(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    /// La prossima comparsa dev'essere ridotta? Si dice **sì una volta sola**:
    /// chi lo prende lo consuma, e gli altri mostrano la finestra come sempre.
    pub fn prendi(&self) -> bool {
        self.0.swap(false, Ordering::AcqRel)
    }
}

/// La comparsa di questo avvio.
static COMPARSA: PrimaComparsa = PrimaComparsa::nuova();

/// Gli argomenti con cui questo processo è partito.
static AVVIO: OnceLock<Argomenti> = OnceLock::new();

/// Registra come è partito il programma. Una volta, all'inizio di `run`.
///
/// `dal_login_store` è la risposta dell'attivazione del pacchetto: il Store non
/// passa argomenti, e l'avvio al login si riconosce da lì
/// (`rymembra_barra::avviata_dal_login_store`).
pub fn registra_l_avvio(argomenti: Argomenti, dal_login_store: bool) {
    COMPARSA.imposta(argomenti.automatico || dal_login_store);
    if argomenti.automatico || dal_login_store {
        nota!("[avvio] partito con Windows: la finestra compare ridotta a icona");
    }
    let _ = AVVIO.set(argomenti);
}

/// Qualcuno ha chiesto la finestra davanti: la comparsa ridotta dell'avvio
/// automatico non vale più.
pub fn annulla_la_riduzione() {
    COMPARSA.imposta(false);
}

/// Il nome del contrassegno che «Installa ora» lascia nella cartella dei dati.
const FILE_RILANCIO: &str = "rilancio-aggiornamento";

/// Quanto vale il contrassegno: il tempo che l'installer impiega a installare e a
/// rilanciare il programma, con un margine per un computer lento.
const FINESTRA_DEL_RILANCIO_MS: i64 = 5 * 60 * 1000;

/// Questo avvio è il rilancio dell'installer dopo «Installa ora»?
static RILANCIO: AtomicBool = AtomicBool::new(false);

/// Il contrassegno scritto a `scritto_ms` vale ancora a `adesso_ms`?
#[must_use]
pub fn rilancio_dopo_aggiornamento(scritto_ms: Option<i64>, adesso_ms: i64) -> bool {
    // Un orologio spostato indietro dà un'età negativa: non vale.
    scritto_ms
        .is_some_and(|scritto| (0..=FINESTRA_DEL_RILANCIO_MS).contains(&(adesso_ms - scritto)))
}

/// «Installa ora» sta per lanciare l'installer: lascia un contrassegno nella
/// cartella dei dati.
///
/// # Errori
///
/// Il guasto della scrittura.
pub fn segna_il_rilancio(dati: &std::path::Path, adesso_ms: i64) -> std::io::Result<()> {
    std::fs::write(dati.join(FILE_RILANCIO), adesso_ms.to_string())
}

/// Questo avvio è il rilancio dell'installer dopo «Installa ora»: la finestra
/// compare normale e il link con cui il processo di prima era partito non si
/// ripresenta. Lo chiama l'avvio del nucleo, **prima** di qualunque cosa che
/// mostri la finestra o consegni il link.
pub fn dopo_un_aggiornamento(dati: &std::path::Path) {
    if consuma_il_rilancio(dati, crate::stato::adesso_ms()) {
        COMPARSA.imposta(false);
        RILANCIO.store(true, Ordering::Release);
        nota!("[avvio] rilancio dell'installer dopo «Installa ora»: la finestra compare normale");
    }
}

/// Il rilancio dopo «Installa ora» ha già deciso: il link degli argomenti non si
/// ripresenta.
pub fn e_un_rilancio() -> bool {
    RILANCIO.load(Ordering::Acquire)
}

/// Legge il contrassegno e lo toglie, sempre. `true` se vale.
pub fn consuma_il_rilancio(dati: &std::path::Path, adesso_ms: i64) -> bool {
    let percorso = dati.join(FILE_RILANCIO);
    let scritto = std::fs::read_to_string(&percorso)
        .ok()
        .and_then(|testo| testo.trim().parse::<i64>().ok());
    let _ = std::fs::remove_file(&percorso);
    rilancio_dopo_aggiornamento(scritto, adesso_ms)
}

/// Gli argomenti dell'avvio, o nessuno se non sono stati registrati.
pub fn avvio() -> &'static Argomenti {
    static NESSUNO: Argomenti = Argomenti {
        automatico: false,
        link: None,
    };
    AVVIO.get().unwrap_or(&NESSUNO)
}

/// Mostra la finestra: ridotta a icona se è la comparsa dell'avvio automatico,
/// normale e davanti in ogni altro caso.
///
/// # L'ordine, e perché non basta fidarsi
///
/// Ridotta **prima** e mostrata dopo: nell'altro ordine la finestra appare a
/// misura piena e si riduce con l'animazione di Windows, che a chi sta
/// arrivando al desktop è un lampo di troppo. Ma l'effetto di `minimize` su una
/// finestra che non si vede ancora è quello di `tao` — che la mostra, la riduce
/// e la nasconde di nuovo — e non si può provare qui. Per questo si controlla
/// com'è finita: se la finestra non si vede la si mostra, e se si vede e non è
/// ridotta la si riduce adesso. **Il guasto che non si può permettere** è una
/// finestra nascosta senza un'icona a cui tornare; un lampo, sì.
pub fn mostra(finestra: &tauri::WebviewWindow) {
    if !COMPARSA.prendi() {
        let _ = finestra.show();
        return;
    }
    let _ = finestra.minimize();
    let _ = finestra.show();
    if !finestra.is_visible().unwrap_or(true) {
        let _ = finestra.show();
    }
    if !finestra.is_minimized().unwrap_or(true) {
        let _ = finestra.minimize();
    }
}

// ── dove sta la finestra ────────────────────────────────────────────────────

/// Quel che si sa della finestra normale: l'ultimo rettangolo, e se va scritto.
#[derive(Debug, Default)]
struct Ricordo {
    /// L'ultima geometria conosciuta, normale: vedi [`geometria`].
    geometria: Option<Geometria>,
    /// Cambiata dall'ultima scrittura nelle impostazioni.
    da_scrivere: bool,
    /// Qualcosa è ancora da rimettere sulla finestra, e aspetta che si veda
    /// ripristinata: vedi [`Rimessa`].
    in_attesa: bool,
}

/// La geometria della finestra, in memoria: sopravvive alla finestra, che con la
/// X sul vassoio muore e rinasce. Le impostazioni servono al riavvio.
static RICORDO: Mutex<Ricordo> = Mutex::new(Ricordo {
    geometria: None,
    da_scrivere: false,
    in_attesa: false,
});

fn ricordo() -> MutexGuard<'static, Ricordo> {
    RICORDO
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// La finestra si è spostata o ha cambiato misura, e va riletta a eventi finiti.
static DA_RILEGGERE: AtomicBool = AtomicBool::new(false);

/// Quanto aspettare la libreria per scrivere la geometria: alla chiusura chi
/// aspetta è peggio di chi non scrive, e la memoria resta com'è per la prossima.
const ATTESA_DELLA_LIBRERIA: Duration = Duration::from_millis(300);

/// `Moved` o `Resized` della finestra: si segna che c'è da rileggerla, e basta.
///
/// **Non si legge qui.** `tao` manda `Moved` da `WM_WINDOWPOSCHANGED`, e il
/// `WM_SIZE` che aggiorna il segno «ingrandita» arriva dopo, dentro la stessa
/// chiamata di sistema: leggere in questo istante vedrebbe il rettangolo
/// ingrandito con la finestra «non ingrandita», e lo ricorderebbe come normale.
/// A eventi finiti — [`rileggi_se_mossa`] — lo stato è coerente.
pub fn finestra_mossa() {
    DA_RILEGGERE.store(true, Ordering::Release);
}

/// A eventi finiti (`RunEvent::MainEventsCleared`): rilegge la finestra se si è
/// mossa. Costa un'operazione atomica quando non è successo niente.
pub fn rileggi_se_mossa(app: &tauri::AppHandle) {
    if DA_RILEGGERE.swap(false, Ordering::AcqRel) {
        rileggi(app);
    }
}

/// La finestra com'è adesso, o `None` se il sistema non risponde.
fn leggi(finestra: &tauri::WebviewWindow) -> Option<Lettura> {
    // Posizione e misura **esterne**, della stessa cornice: vedi `geometria`.
    let posizione = finestra.outer_position().ok()?;
    let misura = geometria::misura_da_ricordare(&mut SullaFinestra(finestra))?;
    Some(Lettura {
        x: posizione.x,
        y: posizione.y,
        larghezza: misura.larghezza,
        altezza: misura.altezza,
        ingrandita: finestra.is_maximized().ok()?,
        ridotta: finestra.is_minimized().ok()?,
        schermo_intero: finestra.is_fullscreen().ok()?,
    })
}

/// Rilegge la finestra e aggiorna la memoria.
fn rileggi(app: &tauri::AppHandle) {
    let Some(finestra) = app.get_webview_window(ETICHETTA) else {
        return;
    };
    let Some(adesso) = leggi(&finestra) else {
        return;
    };
    // Quel che aspettava — la finestra era ridotta, o stava per esserlo — si
    // rimette adesso che qualcuno l'ha ripristinata, e **prima** di aggiornare la
    // memoria: quel che si legge è ancora la finestra di prima.
    let aspettava = ricordo().in_attesa;
    if aspettava
        && geometria::e_ora_di_rimettere(
            finestra.is_visible().unwrap_or(false),
            adesso.ridotta,
            adesso.schermo_intero,
        )
    {
        ricordo().in_attesa = false;
        applica(&finestra, true);
        return;
    }
    let mut ricordo = ricordo();
    let nuova = geometria::aggiorna(ricordo.geometria, &adesso);
    if nuova != ricordo.geometria {
        ricordo.geometria = nuova;
        ricordo.da_scrivere = true;
    }
}

/// Le aree di lavoro dei monitor di adesso, la principale per prima.
///
/// L'area di lavoro e non il monitor intero: la barra delle applicazioni non è
/// posto per la finestra. Se il sistema non la dice si ripiega sul monitor.
fn aree(finestra: &tauri::WebviewWindow) -> Vec<Area> {
    let area_di = |monitor: &tauri::Monitor| {
        let lavoro = monitor.work_area();
        if lavoro.size.width == 0 || lavoro.size.height == 0 {
            Area {
                x: monitor.position().x,
                y: monitor.position().y,
                larghezza: monitor.size().width,
                altezza: monitor.size().height,
            }
        } else {
            Area {
                x: lavoro.position.x,
                y: lavoro.position.y,
                larghezza: lavoro.size.width,
                altezza: lavoro.size.height,
            }
        }
    };
    let tutti = finestra
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(area_di)
        .collect();
    let principale = finestra
        .primary_monitor()
        .ok()
        .flatten()
        .map(|monitor| area_di(&monitor));
    geometria::principale_per_prima(tutti, principale)
}

/// La finestra vera, vista da `geometria::rimetti_la_misura`.
struct SullaFinestra<'a>(&'a tauri::WebviewWindow);

impl MisuraDellaFinestra for SullaFinestra<'_> {
    fn esterna(&mut self) -> Option<Misura> {
        let misura = self.0.outer_size().ok()?;
        Some(Misura::nuova(misura.width, misura.height))
    }

    fn interna(&mut self) -> Option<Misura> {
        let misura = self.0.inner_size().ok()?;
        Some(Misura::nuova(misura.width, misura.height))
    }

    fn chiedi_interna(&mut self, misura: Misura) -> Result<(), String> {
        self.0
            .set_size(tauri::PhysicalSize::new(misura.larghezza, misura.altezza))
            .map_err(|err| err.to_string())
    }
}

/// Rimette la finestra dov'era: la posizione, la misura, e ingrandita se lo era
/// e `ingrandisci` lo vuole.
///
/// Prima la posizione e poi la misura: spostata su un monitor con un'altra
/// scalatura, Windows ridimensiona la finestra da sé, e la misura in pixel
/// fisici va rimessa **dopo**. L'ingrandimento per ultimo, perché
/// `set_position` lo toglie. La finestra può essere ancora nascosta — è il caso
/// normale: nasce così, e la mostra [`mostra`] — e `tao` ingrandisce una
/// finestra nascosta senza mostrarla (e la ingrandisce davvero alla `show`).
fn applica(finestra: &tauri::WebviewWindow, ingrandisci: bool) {
    let Some(ricordata) = ricordo().geometria else {
        return;
    };
    let aree = aree(finestra);
    let Some(messa) = geometria::rimetti_su_uno_schermo(&ricordata, &aree) else {
        nota!(
            "[finestra] la geometria ricordata non si rimette: nessun monitor, o una misura \
             che non è quella di una finestra"
        );
        return;
    };
    if let Err(err) = finestra.set_position(tauri::PhysicalPosition::new(messa.x, messa.y)) {
        nota!("[finestra] la posizione non si rimette: {err}");
    }
    let voluta = Misura::nuova(messa.larghezza, messa.altezza);
    match geometria::rimetti_la_misura(&mut SullaFinestra(finestra), voluta) {
        Ok(esito) => nota!(
            "[finestra] misura rimessa: voluta={}x{} chiesta={}x{} ottenuta={}x{}{}",
            voluta.larghezza,
            voluta.altezza,
            esito.chiesta.larghezza,
            esito.chiesta.altezza,
            esito.ottenuta.larghezza,
            esito.ottenuta.altezza,
            if esito.corretta { " (corretta)" } else { "" }
        ),
        Err(err) => nota!("[finestra] la misura non si rimette: {err}"),
    }
    if ingrandisci
        && messa.ingrandita
        && let Err(err) = finestra.maximize()
    {
        nota!("[finestra] l'ingrandimento non si rimette: {err}");
    }
    if messa != ricordata {
        // Riportata su uno schermo che esiste: quel che si è messo è quel che si sa.
        let mut ricordo = ricordo();
        ricordo.geometria = Some(messa);
        ricordo.da_scrivere = true;
    }
}

/// Rimette la geometria ricordata sulla finestra, **quando si può**.
///
/// Dipende da come sta la finestra (vedi [`Rimessa`]): una finestra ridotta a
/// icona, o che sta per comparire ridotta perché Windows ha avviato Rymembra al
/// login, non si ingrandisce — né, se è già ridotta, si sposta — adesso: tocca
/// a chi la ripristina ([`rileggi`]). Si guarda la comparsa ridotta **senza
/// consumarla**: la consuma chi mostra la finestra.
fn rimetti(finestra: &tauri::WebviewWindow) {
    let ridotta = finestra.is_minimized().unwrap_or(false);
    match geometria::come_rimetterla(ridotta, COMPARSA.in_attesa()) {
        Rimessa::Subito => applica(finestra, true),
        Rimessa::SenzaIngrandire => {
            applica(finestra, false);
            ricordo().in_attesa = true;
        }
        Rimessa::Dopo => ricordo().in_attesa = true,
    }
}

/// Alla fine dell'apertura della libreria: legge dalle impostazioni dove stava la
/// finestra l'ultima volta, e ce la rimette **prima** che la pagina la mostri —
/// salvo l'avvio automatico, vedi [`rimetti`]. Sul filo della finestra.
pub fn ritrova_la_geometria(app: &tauri::AppHandle) {
    let Some(stato) = app.try_state::<Stato>() else {
        return;
    };
    let letta = con_libreria(&stato, |libreria| {
        settings::read_json::<Geometria>(&libreria.connection, CHIAVE_GEOMETRIA_FINESTRA)
    });
    match letta {
        Ok(Some(ricordata)) => {
            let mut ricordo = ricordo();
            ricordo.geometria = Some(ricordata);
            ricordo.da_scrivere = false;
            ricordo.in_attesa = false;
        }
        Ok(None) => return,
        Err(err) => {
            nota!(
                "[finestra] la geometria ricordata non si legge: codice={}",
                err.code().kind().code()
            );
            return;
        }
    }
    if let Some(finestra) = app.get_webview_window(ETICHETTA) {
        rimetti(&finestra);
    }
}

/// Scrive la geometria nelle impostazioni, se è cambiata dall'ultima volta.
///
/// Dalla memoria, senza toccare la finestra: lo chiama anche lo spegnimento, da
/// un filo qualunque, quando la finestra può non esserci più. Aspetta la
/// libreria al più [`ATTESA_DELLA_LIBRERIA`]: se non arriva la memoria resta
/// com'è, e la prossima chiusura riprova.
pub fn ricorda_su_disco(app: &tauri::AppHandle) {
    let da_scrivere = {
        let ricordo = ricordo();
        ricordo.geometria.filter(|_| ricordo.da_scrivere)
    };
    let Some(geometria) = da_scrivere else {
        return;
    };
    let Some(stato) = app.try_state::<Stato>() else {
        return;
    };
    let scritta = con_libreria_entro(&stato, ATTESA_DELLA_LIBRERIA, |libreria| {
        settings::write_json(&libreria.connection, CHIAVE_GEOMETRIA_FINESTRA, &geometria)
    });
    match scritta {
        Some(Ok(())) => {
            // Se nel frattempo è cambiata, resta da scrivere.
            let mut ricordo = ricordo();
            if ricordo.geometria == Some(geometria) {
                ricordo.da_scrivere = false;
            }
        }
        // La libreria staccata apposta da «Torna alla copia» non è un guasto: la
        // geometria non si ricorda perché si sta uscendo (D4 di L-WSB33).
        Some(Err(err)) if crate::stato::e_il_ritorno_alla_copia(&err) => {}
        Some(Err(err)) => nota!(
            "[finestra] la geometria non si ricorda: codice={}",
            err.code().kind().code()
        ),
        None => nota!(
            "[finestra] la libreria è occupata: la geometria si ricorderà alla prossima chiusura"
        ),
    }
}

/// La finestra sta per chiudersi (`CloseRequested`): è l'ultimo momento in cui si
/// può leggerla — alla `Destroyed` non c'è più — e si scrive adesso.
pub fn alla_chiusura(app: &tauri::AppHandle) {
    rileggi(app);
    ricorda_su_disco(app);
}

/// Se una ricreazione è già in coda: due richieste vicine — un clic
/// sull'icona e un secondo avvio — non devono fare due finestre.
static RICREANDO: AtomicBool = AtomicBool::new(false);

/// Se la finestra non c'è, la ricrea dalla configurazione.
///
/// Non fa niente se si sta uscendo (toccare l'`AppHandle` durante la
/// demolizione è il modo in cui questa applicazione moriva: vedi
/// `spegnimento`), né se la finestra c'è già. La costruzione si fa **sul filo
/// della finestra**: da chi chiama può non essere quello, e creare una WebView2
/// altrove vuol dire aspettare lui a bloccare chi chiama.
pub fn ricrea_se_manca(app: &tauri::AppHandle) {
    if spegnimento::in_uscita() || app.get_webview_window(ETICHETTA).is_some() {
        return;
    }
    if RICREANDO.swap(true, Ordering::AcqRel) {
        return;
    }
    let mano = app.clone();
    let spedita = app.run_on_main_thread(move || {
        crea(&mano);
        RICREANDO.store(false, Ordering::Release);
    });
    if let Err(err) = spedita {
        RICREANDO.store(false, Ordering::Release);
        nota!("[finestra] il filo della finestra non risponde, la finestra non rinasce: {err}");
    }
}

/// Costruisce la finestra dalla configurazione. Sul filo della finestra.
fn crea(app: &tauri::AppHandle) {
    if spegnimento::in_uscita() || app.get_webview_window(ETICHETTA).is_some() {
        return;
    }
    let Some(configurazione) = app.config().app.windows.first() else {
        nota!("[finestra] la configurazione non ha una finestra: non rinasce");
        return;
    };
    let costruita = tauri::WebviewWindowBuilder::from_config(app, configurazione)
        .and_then(tauri::WebviewWindowBuilder::build);
    match costruita {
        Ok(finestra) => {
            nota!("[finestra] il vassoio la riapre: la finestra rinasce dalla configurazione");
            // Dov'era, sulla finestra ancora nascosta: la mostra `mostra`.
            rimetti(&finestra);
            finestra_nata(app);
        }
        Err(err) => nota!("[finestra] la finestra non rinasce: {err}"),
    }
}

/// Quel che una finestra **nata dopo l'avvio** deve riavere. Vedi il preambolo.
fn finestra_nata(app: &tauri::AppHandle) {
    crate::pagina::finestra_nuova();
    // La guardia dell'Alt e i pulsanti nell'anteprima si appendono all'`HWND`, e
    // quello vecchio è morto con la finestra.
    crate::barra::ferma_alt(app);
    crate::barra::avvia(app);
    // E la rete di sicurezza: una pagina che non arriva a dire `pronto` non deve
    // lasciare una finestra nuova nascosta per sempre.
    crate::rete_di_sicurezza(app);
}

/// Un secondo processo è partito mentre questo c'è già: cosa fare.
///
/// `argomenti` è la riga di comando del secondo, col nome dell'eseguibile in
/// testa. La decisione è di [`crate::argomenti::secondo_avvio`] e si prova lì;
/// qui si esegue.
pub fn secondo_avvio(app: &tauri::AppHandle, argomenti: &[String]) {
    let letti = Argomenti::da(argomenti.iter().skip(1));
    match decisione(&letti) {
        SecondoAvvio::Ignora => {
            nota!("[avvio] un secondo avvio automatico: la finestra resta com'è");
        }
        SecondoAvvio::Mostra => vassoio::mostra(app),
        SecondoAvvio::ConLink(link) => {
            // Il link passa dalla stessa conferma di sempre — vedi
            // `argomenti` e `lontano::link_arrivato` — e la finestra, se manca,
            // nasce per mostrargliela: la pagina prende il link rimasto in
            // attesa appena si monta.
            if crate::lontano::link_arrivato(app, &link) {
                vassoio::mostra(app);
            } else {
                // Scartato (non è un link, o un altro aspetta): non si alza
                // niente — una pagina web non deve poter togliere il fuoco al
                // browser a ogni clic (S4-4).
                nota!("[avvio] un link scartato: la finestra resta com'è");
            }
        }
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    // ── il rilancio dopo «Installa ora» (S5-6) ──────────────────────────────

    /// «Installa ora» da un'istanza partita con Windows (`--avvio-automatico`) o con
    /// un link: l'updater passa a NSIS `/R /ARGS <gli argomenti di prima>`, e
    /// l'installer rilancia il programma con gli stessi. La copia nuova partiva
    /// come all'accensione di Windows — ridotta a icona — e ripresentava il link,
    /// davanti a chi aveva appena premuto «Installa ora». Il contrassegno lascia
    /// dire al processo nuovo che quegli argomenti sono vecchi.
    #[test]
    fn il_rilancio_dell_installer_vale_per_pochi_minuti() {
        let t = 1_000_000_000_i64;
        assert!(rilancio_dopo_aggiornamento(Some(t), t));
        assert!(rilancio_dopo_aggiornamento(Some(t), t + 20_000));
        assert!(rilancio_dopo_aggiornamento(
            Some(t),
            t + FINESTRA_DEL_RILANCIO_MS
        ));
        // Scaduto: l'installer non è mai partito, e un avvio di Windows è un avvio di Windows.
        assert!(!rilancio_dopo_aggiornamento(
            Some(t),
            t + FINESTRA_DEL_RILANCIO_MS + 1
        ));
        // Nessun contrassegno, o un orologio spostato indietro: niente.
        assert!(!rilancio_dopo_aggiornamento(None, t));
        assert!(!rilancio_dopo_aggiornamento(Some(t), t - 1));
    }

    #[test]
    fn il_contrassegno_si_scrive_si_legge_e_si_consuma_una_volta() {
        let cartella =
            std::env::temp_dir().join(format!("rymembra-rilancio-{}", std::process::id()));
        std::fs::create_dir_all(&cartella).expect("la cartella si crea");
        let t = 1_700_000_000_000_i64;
        segna_il_rilancio(&cartella, t).expect("si scrive");
        assert!(consuma_il_rilancio(&cartella, t + 10_000));
        assert!(
            !cartella.join(FILE_RILANCIO).exists(),
            "va tolto alla lettura"
        );
        assert!(
            !consuma_il_rilancio(&cartella, t + 10_000),
            "una volta sola"
        );
        // Scaduto: si toglie lo stesso, e non vale.
        segna_il_rilancio(&cartella, t).expect("si scrive");
        assert!(!consuma_il_rilancio(
            &cartella,
            t + FINESTRA_DEL_RILANCIO_MS + 1
        ));
        assert!(!cartella.join(FILE_RILANCIO).exists());
        // Un contenuto che non è un numero non vale, e si toglie.
        std::fs::write(cartella.join(FILE_RILANCIO), "non un numero").expect("si scrive");
        assert!(!consuma_il_rilancio(&cartella, t));
        assert!(!cartella.join(FILE_RILANCIO).exists());
        let _ = std::fs::remove_dir_all(&cartella);
    }

    /// Il contrassegno si legge **prima** che qualcuno mostri la finestra — la
    /// rete di sicurezza dei due secondi, o `finestra_mostra` durante una
    /// migrazione — e prima che il link del processo parta: nell'avvio del nucleo,
    /// dove la cartella dei dati si sa e la libreria non è ancora aperta.
    #[test]
    fn il_rilancio_si_legge_prima_del_link_e_della_rete_di_sicurezza() {
        let lib = include_str!("lib.rs");
        let setup = lib
            .split(".setup(|app| {")
            .nth(1)
            .and_then(|resto| resto.split("\n        })\n").next())
            .unwrap_or_default();
        let da_capo = lib
            .split("fn avvia_da_capo(")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        let legge = da_capo
            .find("comparsa::dopo_un_aggiornamento(")
            .expect("l'avvio non legge il contrassegno del rilancio");
        assert!(legge < da_capo.find("apri_e_avvia(").unwrap_or(usize::MAX));
        let link = setup
            .find("comparsa::avvio().link")
            .expect("il setup consegna il link");
        assert!(
            setup
                .get(link..link + 200)
                .unwrap_or_default()
                .contains("comparsa::e_un_rilancio()"),
            "il link del rilancio si ripresenta"
        );
        assert!(setup.find("avvia_da_capo(").unwrap_or(usize::MAX) < link);
        assert!(link < setup.find("rete_di_sicurezza(").unwrap_or(0));
        // E chi lancia l'installer lo segna prima.
        let aggiornamenti = include_str!("aggiornamenti.rs");
        let subito = aggiornamenti
            .split("Quando::Subito => {")
            .nth(1)
            .and_then(|resto| resto.split("Quando::AllaChiusura").next())
            .unwrap_or_default();
        let segna = subito
            .find("segna_il_rilancio(app)")
            .expect("«Installa ora» non segna il rilancio");
        assert!(segna < subito.find(".install(byte)").unwrap_or(0));
    }

    #[test]
    fn la_comparsa_ridotta_si_consuma_alla_prima_finestra_mostrata() {
        let comparsa = PrimaComparsa::nuova();
        // Senza richiesta, ogni comparsa è normale.
        assert!(!comparsa.prendi());
        // Con la richiesta, il primo che mostra la finestra la riduce...
        comparsa.imposta(true);
        assert!(comparsa.prendi());
        // ...e gli altri due — `pronto`, `finestra_mostra`, la rete di
        // sicurezza — la mostrano com'è: una finestra ricreata dal vassoio la
        // vuole chi ha cliccato, e la vuole davanti.
        assert!(!comparsa.prendi());
        assert!(!comparsa.prendi());
    }

    /// Chi rimette la geometria guarda se la comparsa ridotta dell'avvio
    /// automatico è ancora da fare **senza consumarla**: se la consumasse, la
    /// finestra comparirebbe normale e non ridotta.
    #[test]
    fn guardare_la_comparsa_ridotta_non_la_consuma() {
        let comparsa = PrimaComparsa::nuova();
        assert!(!comparsa.in_attesa());
        comparsa.imposta(true);
        assert!(comparsa.in_attesa());
        assert!(comparsa.in_attesa(), "guardarla non la consuma");
        assert!(comparsa.prendi(), "e chi mostra la finestra la trova");
        assert!(!comparsa.in_attesa(), "consumata, non c'è più");
    }

    /// Le tre strade che mostrano la finestra passano tutte da [`mostra`]: se
    /// una facesse `show()` da sé, l'avvio automatico la alzerebbe a metà.
    #[test]
    fn le_tre_comparse_passano_dalla_stessa_funzione() {
        let lib = include_str!("lib.rs");
        let corpo = |firma: &str| {
            lib.split(firma)
                .nth(1)
                .and_then(|resto| resto.split("\n}\n").next())
                .unwrap_or_default()
                .to_owned()
        };
        for (firma, nome) in [
            ("fn pronto(", "pronto"),
            ("fn finestra_mostra(", "finestra_mostra"),
            ("fn rete_di_sicurezza(", "rete_di_sicurezza"),
        ] {
            let testo = corpo(firma);
            assert!(!testo.is_empty(), "{nome} non si trova");
            assert!(
                testo.contains("comparsa::mostra("),
                "{nome} non passa da comparsa::mostra"
            );
        }
    }

    /// Una finestra nata dopo l'avvio deve riavere tutto quel che l'avvio le dava.
    #[test]
    fn una_finestra_nuova_riprende_guardia_pulsanti_e_rete_di_sicurezza() {
        let sorgente = include_str!("comparsa.rs");
        let sorgente = sorgente.split("#[cfg(test)]").next().unwrap_or(sorgente);
        let corpo = sorgente
            .split("fn finestra_nata(")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        for chiamata in [
            "pagina::finestra_nuova()",
            "barra::ferma_alt(app)",
            "barra::avvia(app)",
            "rete_di_sicurezza(app)",
        ] {
            assert!(corpo.contains(chiamata), "manca {chiamata}");
        }
    }

    /// Il secondo avvio con link e quello senza finestra passano per la stessa
    /// strada che alza o ricrea la finestra.
    #[test]
    fn il_secondo_avvio_alza_o_ricrea_la_finestra_dal_vassoio() {
        let sorgente = include_str!("comparsa.rs");
        let sorgente = sorgente.split("#[cfg(test)]").next().unwrap_or(sorgente);
        let corpo = sorgente
            .split("pub fn secondo_avvio(")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        assert_eq!(corpo.matches("vassoio::mostra(app)").count(), 2);
        assert!(corpo.contains("lontano::link_arrivato(app, &link)"));
    }

    // ── dove sta la finestra (collaudo in Windows Sandbox, D1) ──────────────
    //
    // La regola vera — dove rimettere, cosa ricordare — è in `geometria` e si
    // prova lì. Qui si prova il **cablaggio**, che un `AppHandle` finto non
    // permette di far girare: chi legge, chi scrive, e in che ordine.

    /// Una finestra ricreata dal vassoio riprende posizione, misura e stato
    /// ingrandito sulla finestra ancora nascosta, **prima** che la pagina la
    /// mostri: dopo `build` e prima di `finestra_nata`, che arma la rete di
    /// sicurezza che la mostra comunque.
    #[test]
    fn una_finestra_ricreata_riprende_la_geometria_prima_di_mostrarsi() {
        let sorgente = include_str!("comparsa.rs");
        let sorgente = sorgente.split("#[cfg(test)]").next().unwrap_or(sorgente);
        let corpo = sorgente
            .split("fn crea(")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        let costruita = corpo
            .find("WebviewWindowBuilder::from_config")
            .expect("la finestra non si costruisce dalla configurazione");
        let rimessa = corpo
            .find("rimetti(&finestra)")
            .expect("la finestra ricreata non riprende la geometria");
        let nata = corpo
            .find("finestra_nata(app)")
            .expect("la finestra ricreata non riprende guardia e pulsanti");
        assert!(costruita < rimessa && rimessa < nata);
    }

    /// D1: la misura si ricorda **esterna** — la stessa cornice della posizione — e si
    /// rimette dopo la posizione, chiedendola alla cornice della finestra nata:
    /// `set_size` a mano, con la misura interna di prima, la faceva crescere di
    /// trenta pixel a ogni ricreazione (L-WSB33).
    #[test]
    fn la_misura_si_ricorda_esterna_e_si_rimette_dopo_la_posizione() {
        let sorgente = include_str!("comparsa.rs");
        let sorgente = sorgente.split("#[cfg(test)]").next().unwrap_or(sorgente);
        let corpo = |firma: &str| {
            sorgente
                .split(firma)
                .nth(1)
                .and_then(|resto| resto.split("\n}\n").next())
                .unwrap_or_default()
        };
        let leggi = corpo("fn leggi(");
        assert!(leggi.contains("geometria::misura_da_ricordare("));
        assert!(
            !leggi.contains("inner_size"),
            "si ricorda la misura interna"
        );
        let applica = corpo("fn applica(");
        let posizione = applica
            .find("set_position(")
            .expect("la posizione non si rimette");
        let misura = applica
            .find("geometria::rimetti_la_misura(")
            .expect("la misura non passa dalla cornice");
        assert!(
            posizione < misura,
            "la misura si rimette prima della posizione"
        );
        assert!(
            !applica.contains(".set_size("),
            "la misura si chiede alla finestra cruda, senza la sua cornice"
        );
    }

    /// Dove si legge la finestra: `Moved` e `Resized` **segnano** soltanto, e si
    /// legge a eventi finiti (`MainEventsCleared`). `tao` manda `Moved` prima
    /// del `WM_SIZE` che aggiorna «ingrandita»: leggere lì ricorderebbe il
    /// rettangolo ingrandito come normale.
    #[test]
    fn la_finestra_si_rilegge_a_eventi_finiti_e_non_a_ogni_spostamento() {
        let lib = include_str!("lib.rs");
        let ciclo = lib
            .split("Ok(app) => app.run(|mano, evento| {")
            .nth(1)
            .and_then(|resto| resto.split("\n        }),\n").next())
            .unwrap_or_default();
        let segna = ciclo
            .find("comparsa::finestra_mossa()")
            .expect("il ciclo non segna gli spostamenti della finestra");
        let davanti = ciclo.get(..segna).unwrap_or_default();
        assert!(
            davanti.contains("tauri::WindowEvent::Moved(_)")
                && davanti.contains("tauri::WindowEvent::Resized(_)"),
            "si segna per `Moved` e `Resized`, tutti e due"
        );
        let rilegge = ciclo
            .find("comparsa::rileggi_se_mossa(mano)")
            .expect("il ciclo non rilegge la finestra");
        assert!(
            ciclo
                .get(..rilegge)
                .unwrap_or_default()
                .contains("tauri::RunEvent::MainEventsCleared"),
            "si rilegge a eventi finiti"
        );
        // Né la lettura né la memoria sono dietro il ramo del telefono.
        assert!(
            ciclo
                .get(..segna)
                .unwrap_or_default()
                .contains("#[cfg(desktop)]")
        );
        let sorgente = include_str!("comparsa.rs");
        let sorgente = sorgente.split("#[cfg(test)]").next().unwrap_or(sorgente);
        let segna_la_finestra = sorgente
            .split("pub fn finestra_mossa(")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        assert!(
            !segna_la_finestra.contains("outer_position")
                && !segna_la_finestra.contains("is_maximized"),
            "`finestra_mossa` legge la finestra invece di segnare e basta"
        );
    }

    /// L'avvio automatico non deve ricevere un lampo a schermo intero: la
    /// geometria si rimette secondo `come_rimetterla` — che guarda se la finestra
    /// è ridotta o sta per esserlo, senza consumare la comparsa — e quel che
    /// aspetta si rimette solo quando la finestra si vede ripristinata, **prima**
    /// di aggiornare la memoria con quel che si legge.
    #[test]
    fn la_geometria_non_ingrandisce_una_finestra_che_sta_per_comparire_ridotta() {
        let sorgente = include_str!("comparsa.rs");
        let sorgente = sorgente.split("#[cfg(test)]").next().unwrap_or(sorgente);
        let corpo_di = |firma: &str| {
            sorgente
                .split(firma)
                .nth(1)
                .and_then(|resto| resto.split("\n}\n").next())
                .unwrap_or_default()
                .to_owned()
        };
        let rimetti = corpo_di("fn rimetti(");
        assert!(rimetti.contains("geometria::come_rimetterla("));
        assert!(
            rimetti.contains("COMPARSA.in_attesa()"),
            "non guarda la comparsa ridotta dell'avvio automatico"
        );
        assert!(
            !rimetti.contains("COMPARSA.prendi()"),
            "consumerebbe la comparsa ridotta: la finestra comparirebbe normale"
        );
        assert!(rimetti.contains("Rimessa::SenzaIngrandire => {"));
        let rileggi = corpo_di("fn rileggi(");
        let ora = rileggi
            .find("geometria::e_ora_di_rimettere(")
            .expect("quel che aspettava non si rimette mai");
        let aggiorna = rileggi
            .find("geometria::aggiorna(")
            .expect("la memoria non si aggiorna");
        assert!(ora < aggiorna, "si aggiorna la memoria prima di rimettere");
        // L'ingrandimento è dietro un parametro: `rimetti` lo nega finché la
        // finestra sta per comparire ridotta.
        let applica = corpo_di("fn applica(");
        let chiede = applica
            .find("if ingrandisci")
            .expect("l'ingrandimento non dipende dal parametro");
        let ingrandisce = applica
            .find("finestra.maximize()")
            .expect("la finestra ingrandita non si ingrandisce");
        assert!(chiede < ingrandisce);
    }

    /// La geometria si scrive alla `CloseRequested` — l'ultimo momento in cui la
    /// finestra c'è — e **prima** di decidere se la chiusura passa o no, anche
    /// se si sta uscendo.
    #[test]
    fn la_geometria_si_scrive_alla_richiesta_di_chiusura_prima_di_decidere() {
        let lib = include_str!("lib.rs");
        let gestore = lib
            .split(".on_window_event(|finestra, evento| {")
            .nth(1)
            .and_then(|resto| resto.split("\n        })\n").next())
            .unwrap_or_default();
        let scrive = gestore
            .find("comparsa::alla_chiusura(finestra.app_handle())")
            .expect("la richiesta di chiusura non ricorda la geometria");
        let decide = gestore
            .find("spegnimento::in_uscita()")
            .expect("il gestore non guarda l'uscita");
        assert!(
            scrive < decide,
            "si ricorda dopo aver deciso, e a volte mai"
        );
    }

    /// Un riavvio normale riparte dov'era: la geometria si rimette quando la
    /// libreria è aperta, **prima** che il cancello si apra e la pagina — che
    /// mostra la finestra — monti.
    #[test]
    fn all_avvio_la_geometria_si_rimette_prima_che_la_pagina_monti() {
        let lib = include_str!("lib.rs");
        let corpo = lib
            .split("fn finisci_l_avvio(")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        let rimette = corpo
            .find("comparsa::ritrova_la_geometria(app)")
            .expect("l'avvio non rimette la geometria");
        let cancello = corpo
            .find("apri_il_cancello()")
            .expect("l'avvio non apre il cancello");
        assert!(rimette < cancello);
    }

    /// Chi esce con «Esci» non passa dalla `CloseRequested`: la geometria si
    /// scrive anche nella sequenza di spegnimento, dopo i salvataggi e prima del
    /// giornale (che dev'essere l'ultima scrittura), e non dietro la prima volta.
    #[test]
    fn all_uscita_la_geometria_si_scrive_fra_i_salvataggi_e_il_giornale() {
        let sorgente = include_str!("spegnimento.rs");
        let da = sorgente.find("pub fn spegni(").expect("la sequenza c'è");
        let a = sorgente
            .find("pub trait Emette")
            .expect("il tratto che segue c'è");
        let corpo = sorgente.get(da..a).expect("il corpo della sequenza");
        let salva = corpo
            .find("crate::riproduzione::salva_uscendo(mano)")
            .expect("i salvataggi dell'ascolto");
        let scrive = corpo
            .find("crate::comparsa::ricorda_su_disco(mano)")
            .expect("lo spegnimento non scrive la geometria");
        let svuota = corpo
            .find("crate::manutenzione::uscendo(mano)")
            .expect("l'uscita del database");
        assert!(salva < scrive && scrive < svuota);
        assert!(
            !corpo
                .get(salva..svuota)
                .unwrap_or_default()
                .contains("if prima_volta"),
            "la geometria si scrive solo la prima volta"
        );
        // E sul telefono, dove non c'è una finestra da ricordare, non esiste.
        let davanti = corpo.get(..scrive).unwrap_or_default();
        assert!(
            davanti
                .rsplit('\n')
                .nth(1)
                .is_some_and(|riga| riga.contains("#[cfg(desktop)]")),
            "la chiamata non è riservata al computer"
        );
    }
}
