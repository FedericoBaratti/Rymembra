//! La copertina di una riga di un altro dispositivo, per le schede del sistema.
//!
//! # Il difetto che questo modulo toglie
//!
//! Le copertine dei brani degli altri dispositivi stanno nello store di questo
//! (`lontano::copertina_di_una_riga`): le chiede **la finestra**, quando disegna
//! una riga o il lettore, e le mette sotto la loro impronta. La scheda di Android
//! (l'auto, la notifica, la schermata di blocco) e quella di Windows (le SMTC)
//! non passano dalla finestra: leggono lo store, e di un brano del PC che nessuna
//! finestra ha mai mostrato — chi guida col telefono in tasca, lo schermo spento
//! — lo store non ha niente. La scheda usciva senza immagine, e non ne arrivava
//! una dopo.
//!
//! # Come si risolve
//!
//! Se la copertina è nello store, ci si punta. Se non c'è, si chiede all'altro
//! **una volta sola**, in disparte, con la stessa funzione che usa la finestra —
//! niente seconda strada per scaricare, niente seconda cache: quel che arriva sta
//! nello store, dove la finestra e le schede lo trovano tutte —, e a copertina
//! arrivata si ripubblica la scheda: chi chiama dice come. Fra le due cose la
//! scheda esce senza copertina, com'era, e non aspetta la rete.
//!
//! Le voci di «Ascolta insieme» (`insieme-<uid>`) non passano di qui: la loro
//! copertina arriva con la voce e sta già in memoria
//! (`crate::insieme::copertina_su_file`).
//!
//! # Tre cose che non devono succedere
//!
//! - **Due richieste per la stessa copertina.** Lo stato del lettore parte a
//!   ogni pausa e a ogni salto: finché la prima richiesta è in volo, le altre
//!   non ne fanno partire una seconda.
//! - **Un ciclo.** Si ripubblica solo se la copertina è davvero nello store dopo
//!   la richiesta: una che non arriva (l'altro non la ha, è spento) ripubblicata
//!   sarebbe una scheda uguale, che chiede di nuovo, che ripubblica. Un'impronta
//!   che non è arrivata non si richiede per un minuto, e se ne ricordano poche.
//! - **Aspettare la rete dove non si può.** Chi chiama lo fa dentro il lucchetto
//!   dei controlli multimediali: la richiesta parte su un altro filo, sempre.
//!
//! # Il protocollo `rymembra-cover` non aspetta mai la rete
//!
//! Su Android il gestore del protocollo gira dentro `shouldInterceptRequest`, e
//! wry tiene il filo della WebView fermo finché Rust risponde (fino a trenta
//! secondi: `rx.recv_timeout` in `wry/src/android/mod.rs`). Lo stesso filo serve
//! anche le risposte dei comandi che superano gli 8 KiB — Tauri le consegna con
//! una `fetch` sul protocollo `ipc://` (`tauri/src/ipc/channel.rs`,
//! `MAX_JSON_DIRECT_EXECUTE_THRESHOLD`) —, e su un telefono vero, con la Libreria
//! aperta su un dispositivo e le sue copertine ancora fuori dallo store, i fili
//! erano tutti in attesa della rete: le pagine da venti brani in su non tornavano
//! più. [`servi`] risponde quindi **subito** — con i byte se ci sono, con un
//! «non c'è» che non si ricorda (`no-store`) se no — e la copertina si chiede in
//! disparte, una volta sola e a numero chiuso ([`IN_VOLO_PER_LA_FINESTRA`]); quando
//! arriva, l'evento `lontano:copertina` dice alla pagina di richiederla.

use crate::nucleo::{ComeNucleo, Nucleo};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Quanto si aspetta prima di richiedere una copertina che non è arrivata.
const RIPROVA: Duration = Duration::from_secs(60);

/// Quante impronte si ricordano al più, fra quelle in volo e quelle non arrivate.
const RICORDATE: usize = 256;

/// Quante copertine chiede insieme la **finestra**: due dispositivi per i tre
/// posti ciascuno di `COPERTINE_PER_DISPOSITIVO`. Una pagina con centinaia di
/// righe specchio senza copertina ne chiede centinaia, ognuna con un filo in
/// attesa del suo posto per quindici secondi: a numero chiuso se ne chiedono
/// poche, e le altre tornano alla pagina quando ne arriva una
/// (`lontano:copertina`). Le schede di sistema non hanno un tetto: ne chiedono una
/// per brano che suona.
const IN_VOLO_PER_LA_FINESTRA: usize = 6;

/// Solo una riga di un altro dispositivo ha una copertina da andare a prendere:
/// una riga di questa macchina senza il file della copertina (il tag non ce
/// l'aveva, lo store è stato ripulito) non si chiede a nessuno.
#[must_use]
pub(super) fn e_di_un_altro_dispositivo(percorso: Option<&str>) -> bool {
    percorso.is_some_and(rymembra_app::specchio::e_specchio)
}

/// Quel che serve a chiedere una copertina: lo store, la rete, un filo.
///
/// Un trait perché le prove non hanno né un'applicazione né un altro
/// dispositivo: la cucitura è qui, dove il comportamento dipende da tempi e da
/// chi risponde.
pub(super) trait Ambiente: Clone + Send + 'static {
    /// La copertina di `impronta` è nello store.
    fn nello_store(&self, impronta: &str) -> bool;

    /// La chiede all'altro e la mette nello store. Aspetta la rete: gira sempre
    /// in disparte.
    fn scarica(&self, impronta: &str);

    /// Manda `lavoro` su un altro filo.
    fn in_disparte(&self, lavoro: Box<dyn FnOnce() + Send>);
}

/// Cosa si sa di un'impronta che non è nello store.
enum Stato {
    /// La richiesta è partita e non è tornata.
    InVolo,
    /// È tornata senza la copertina, a quest'ora.
    NonArrivata(Instant),
}

/// Le richieste di copertine fatte da chi non è la finestra.
#[derive(Clone)]
pub(super) struct Richieste {
    ricordate: Arc<Mutex<HashMap<String, Stato>>>,
    riprova: Duration,
}

impl Richieste {
    /// Nessuna richiesta fatta; un'impronta non arrivata si riprova dopo
    /// `riprova`.
    pub(super) fn nuove(riprova: Duration) -> Self {
        Self {
            ricordate: Arc::new(Mutex::new(HashMap::new())),
            riprova,
        }
    }

    /// Chiede la copertina di `impronta` se non è nello store, una volta sola, in
    /// disparte; quando è arrivata chiama `arrivata`.
    ///
    /// Non dà niente indietro: chi chiama ha già guardato lo store, e lo riguarda
    /// quando `arrivata` lo fa ricomporre.
    ///
    /// `tetto` è quante richieste possono essere in volo insieme: a quel punto la
    /// nuova non parte e non si ricorda, e chi la vuole la richiede dopo. Le
    /// schede di sistema non ne hanno (`usize::MAX`).
    pub(super) fn richiedi<A: Ambiente>(
        &self,
        ambiente: &A,
        impronta: &str,
        tetto: usize,
        arrivata: impl FnOnce() + Send + 'static,
    ) {
        if ambiente.nello_store(impronta) || !self.comincia(impronta, tetto) {
            return;
        }
        let richieste = self.clone();
        let ambiente = ambiente.clone();
        let impronta = impronta.to_owned();
        let per_lanciare = ambiente.clone();
        per_lanciare.in_disparte(Box::new(move || {
            // Se lo scaricamento cade a metà — un panico del decoder, che il pool
            // bloccante ingoia — la richiesta non resta «in volo» per sempre: con
            // sei impronte così la finestra non chiederebbe più nessuna copertina
            // (reperto S8-7).
            let mut fine = FineDellaRichiesta {
                richieste,
                impronta,
                segnata: false,
            };
            ambiente.scarica(&fine.impronta);
            // Si ripubblica solo se la copertina è davvero nello store: vedi
            // «Un ciclo» in cima al modulo.
            let portata = ambiente.nello_store(&fine.impronta);
            fine.segna(portata);
            if portata {
                arrivata();
            }
        }));
    }

    /// Segna che la richiesta di `impronta` parte adesso. `false` se non deve
    /// partire: è già in volo, non è arrivata da meno di `riprova`, o ce ne sono
    /// già `tetto` in volo.
    fn comincia(&self, impronta: &str, tetto: usize) -> bool {
        let mut dentro = self
            .ricordate
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match dentro.get(impronta) {
            Some(Stato::InVolo) => return false,
            Some(Stato::NonArrivata(quando)) if quando.elapsed() < self.riprova => return false,
            _ => {}
        }
        if dentro
            .values()
            .filter(|stato| matches!(stato, Stato::InVolo))
            .count()
            >= tetto
        {
            return false;
        }
        if dentro.len() >= RICORDATE && !dentro.contains_key(impronta) {
            // Si dimentica quel che non è arrivato: al più si riprova una
            // volta in più. Quel che è in volo resta, perché la sua fine
            // deve trovare il suo posto.
            dentro.retain(|_, stato| matches!(stato, Stato::InVolo));
            if dentro.len() >= RICORDATE {
                return false;
            }
        }
        dentro.insert(impronta.to_owned(), Stato::InVolo);
        true
    }

    /// La richiesta di `impronta` è tornata: se la copertina è arrivata non c'è
    /// altro da ricordare (lo store la trova), altrimenti si segna l'ora.
    fn finita(&self, impronta: &str, portata: bool) {
        let mut dentro = self
            .ricordate
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if portata {
            dentro.remove(impronta);
        } else {
            dentro.insert(impronta.to_owned(), Stato::NonArrivata(Instant::now()));
        }
    }
}

/// La fine di una richiesta, anche se il lavoro cade: se non si è segnata da sé
/// ([`FineDellaRichiesta::segna`]), a `Drop` la richiesta torna «non arrivata», e
/// si potrà riprovare fra [`RIPROVA`].
struct FineDellaRichiesta {
    richieste: Richieste,
    impronta: String,
    segnata: bool,
}

impl FineDellaRichiesta {
    /// La richiesta è tornata: `portata` dice se la copertina è nello store.
    fn segna(&mut self, portata: bool) {
        self.richieste.finita(&self.impronta, portata);
        self.segnata = true;
    }
}

impl Drop for FineDellaRichiesta {
    fn drop(&mut self) {
        if !self.segnata {
            self.richieste.finita(&self.impronta, false);
        }
    }
}

// ── l'applicazione ──────────────────────────────────────────────────────────

/// L'applicazione vera: lo store e la rete di [`crate::lontano`].
#[derive(Clone)]
struct ViaApp(Nucleo);

impl Ambiente for ViaApp {
    fn nello_store(&self, impronta: &str) -> bool {
        self.0
            .try_state::<crate::stato::Stato>()
            .and_then(|stato| stato.copertine().ok())
            .is_some_and(|store| {
                store.path_for(impronta).is_file() || store.percorso_media(impronta).is_file()
            })
    }

    fn scarica(&self, impronta: &str) {
        if let Err(err) = crate::lontano::copertina_di_una_riga(&self.0, impronta) {
            crate::nota!(
                "[media] la copertina di un altro dispositivo non arriva: {}",
                err.code().kind().code()
            );
        }
    }

    fn in_disparte(&self, lavoro: Box<dyn FnOnce() + Send>) {
        drop(tauri::async_runtime::spawn_blocking(lavoro));
    }
}

/// Le richieste di tutto il processo: una sola per copertina, comunque la
/// chiedano la scheda di Android, la sua coda o le SMTC.
static RICHIESTE: LazyLock<Richieste> = LazyLock::new(|| Richieste::nuove(RIPROVA));

/// Chiede la copertina di `impronta` all'altro dispositivo, in disparte e una
/// volta sola, se non è già nello store; a copertina arrivata chiama `arrivata`.
/// Vedi [`Richieste::richiedi`].
pub(super) fn richiedi(
    app: &impl ComeNucleo,
    impronta: &str,
    arrivata: impl FnOnce(&Nucleo) + Send + 'static,
) {
    let app = &app.come_nucleo();
    let mano = app.clone();
    RICHIESTE.richiedi(&ViaApp(app.clone()), impronta, usize::MAX, move || {
        arrivata(&mano)
    });
}

/// La risposta del protocollo `rymembra-cover` a una richiesta della finestra:
/// **subito**, senza aspettare la rete. Vedi il `//!`.
///
/// `letta` è quel che lo store ha: con i byte, `rispondi(Some(..))` e basta. Senza,
/// `rispondi(None)` — il gestore dice «non c'è», e non si ricorda — **prima** di
/// qualunque richiesta, e poi la copertina si chiede in disparte, una volta sola
/// e a numero chiuso; quando è nello store si chiama `arrivata`. Pura rispetto
/// alla rete: `ambiente` è quel che le prove sostituiscono.
fn senza_aspettare<A: Ambiente>(
    richieste: &Richieste,
    ambiente: &A,
    impronta: &str,
    letta: Option<Vec<u8>>,
    rispondi: impl FnOnce(Option<Vec<u8>>),
    arrivata: impl FnOnce() + Send + 'static,
) {
    if letta.is_some() {
        rispondi(letta);
        return;
    }
    rispondi(None);
    richieste.richiedi(ambiente, impronta, IN_VOLO_PER_LA_FINESTRA, arrivata);
}

/// [`senza_aspettare`] con l'applicazione vera: lo store, la rete, e l'evento
/// `lontano:copertina` a copertina arrivata.
pub(crate) fn servi(
    app: &impl ComeNucleo,
    impronta: &str,
    letta: Option<Vec<u8>>,
    rispondi: impl FnOnce(Option<Vec<u8>>),
) {
    let app = &app.come_nucleo();
    let mano = app.clone();
    let chi = impronta.to_owned();
    senza_aspettare(
        &RICHIESTE,
        &ViaApp(app.clone()),
        impronta,
        letta,
        rispondi,
        move || mano.emetti("lontano:copertina", chi),
    );
}

#[cfg(test)]
mod prove {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Condvar;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::thread::JoinHandle;

    const IMPRONTA: &str = "0123456789abcdef0123456789abcdef";

    /// Un ambiente senza disco e senza rete: lo store è un insieme di impronte,
    /// e lo scaricamento aspetta che la prova lo lasci andare.
    #[derive(Clone)]
    struct Finto {
        nello_store: Arc<Mutex<HashSet<String>>>,
        /// Quante volte è partito lo scaricamento.
        scaricate: Arc<AtomicUsize>,
        /// Se lo scaricamento porta la copertina o torna a mani vuote.
        porta: Arc<Mutex<bool>>,
        /// Lo scaricamento aspetta qui.
        cancello: Arc<(Mutex<bool>, Condvar)>,
        fili: Arc<Mutex<Vec<JoinHandle<()>>>>,
        /// Lo scaricamento cade con un panico, come un decoder su un input ostile.
        panica: Arc<AtomicBool>,
    }

    impl Finto {
        fn nuovo(porta: bool, aperto: bool) -> Self {
            Self {
                nello_store: Arc::new(Mutex::new(HashSet::new())),
                scaricate: Arc::new(AtomicUsize::new(0)),
                porta: Arc::new(Mutex::new(porta)),
                cancello: Arc::new((Mutex::new(aperto), Condvar::new())),
                fili: Arc::new(Mutex::new(Vec::new())),
                panica: Arc::new(AtomicBool::new(false)),
            }
        }

        fn apri(&self) {
            *self.cancello.0.lock().unwrap() = true;
            self.cancello.1.notify_all();
        }

        /// Aspetta che tutto quel che è partito in disparte sia finito.
        fn aspetta(&self) {
            let fili: Vec<_> = self.fili.lock().unwrap().drain(..).collect();
            for filo in fili {
                filo.join().unwrap();
            }
        }
    }

    impl Ambiente for Finto {
        fn nello_store(&self, impronta: &str) -> bool {
            self.nello_store.lock().unwrap().contains(impronta)
        }

        fn scarica(&self, impronta: &str) {
            self.scaricate.fetch_add(1, Ordering::SeqCst);
            assert!(!self.panica.load(Ordering::SeqCst), "il decoder cade");
            let (aperto, quando) = &*self.cancello;
            let mut dentro = aperto.lock().unwrap();
            while !*dentro {
                dentro = quando.wait(dentro).unwrap();
            }
            drop(dentro);
            if *self.porta.lock().unwrap() {
                self.nello_store.lock().unwrap().insert(impronta.to_owned());
            }
        }

        fn in_disparte(&self, lavoro: Box<dyn FnOnce() + Send>) {
            self.fili.lock().unwrap().push(std::thread::spawn(lavoro));
        }
    }

    /// Una ripubblicazione che si conta.
    fn contatore() -> (Arc<AtomicUsize>, impl FnOnce() + Send + 'static) {
        let conti = Arc::new(AtomicUsize::new(0));
        let dentro = Arc::clone(&conti);
        (conti, move || {
            dentro.fetch_add(1, Ordering::SeqCst);
        })
    }

    /// FR2 (richiesta #1): una riga di un altro dispositivo senza la copertina
    /// nello store produce **una** richiesta, e quando la copertina arriva una
    /// seconda pubblicazione della scheda, con lei. Fra le due la scheda è uscita
    /// senza, e non ha aspettato.
    #[test]
    fn una_riga_senza_copertina_nello_store_chiede_e_ripubblica_all_arrivo() {
        let finto = Finto::nuovo(true, false);
        let richieste = Richieste::nuove(RIPROVA);
        let (ripubblicate, arrivata) = contatore();

        // Prima pubblicazione: lo store non ha la copertina, e non si aspetta.
        richieste.richiedi(&finto, IMPRONTA, usize::MAX, arrivata);
        assert_eq!(ripubblicate.load(Ordering::SeqCst), 0, "non ancora");

        // Arriva.
        finto.apri();
        finto.aspetta();
        assert_eq!(finto.scaricate.load(Ordering::SeqCst), 1, "una richiesta");
        assert_eq!(
            ripubblicate.load(Ordering::SeqCst),
            1,
            "una ripubblicazione"
        );

        // Seconda pubblicazione: lo store la ha, e non si chiede di nuovo.
        let (altre, arrivata) = contatore();
        richieste.richiedi(&finto, IMPRONTA, usize::MAX, arrivata);
        finto.aspetta();
        assert_eq!(finto.scaricate.load(Ordering::SeqCst), 1);
        assert_eq!(altre.load(Ordering::SeqCst), 0);
        assert_eq!(ripubblicate.load(Ordering::SeqCst), 1);
    }

    /// Lo stato del lettore parte a ogni pausa: finché la richiesta è in volo
    /// le altre non ne fanno partire una seconda.
    #[test]
    fn finche_la_richiesta_e_in_volo_non_ne_parte_un_altra() {
        let finto = Finto::nuovo(true, false);
        let richieste = Richieste::nuove(RIPROVA);
        let (ripubblicate, _) = contatore();
        for _ in 0..5 {
            let conti = Arc::clone(&ripubblicate);
            richieste.richiedi(&finto, IMPRONTA, usize::MAX, move || {
                conti.fetch_add(1, Ordering::SeqCst);
            });
        }
        finto.apri();
        finto.aspetta();
        assert_eq!(
            finto.scaricate.load(Ordering::SeqCst),
            1,
            "un solo scaricamento"
        );
        assert_eq!(
            ripubblicate.load(Ordering::SeqCst),
            1,
            "e una sola ripubblicazione"
        );
    }

    /// Reperto S8-7: un panico del decoder, ingoiato dal pool bloccante, lasciava la
    /// richiesta «in volo» per sempre. Con `IN_VOLO_PER_LA_FINESTRA` impronte così
    /// la finestra non chiedeva più nessuna copertina dei pari.
    #[test]
    fn un_panico_nello_scaricamento_non_lascia_la_richiesta_in_volo() {
        let finto = Finto::nuovo(true, true);
        finto.panica.store(true, Ordering::SeqCst);
        let richieste = Richieste::nuove(Duration::ZERO);
        for n in 0..IN_VOLO_PER_LA_FINESTRA {
            let impronta = format!("{n:032x}");
            richieste.richiedi(&finto, &impronta, IN_VOLO_PER_LA_FINESTRA, || {});
        }
        let cadute: Vec<_> = finto.fili.lock().unwrap().drain(..).collect();
        assert_eq!(cadute.len(), IN_VOLO_PER_LA_FINESTRA);
        for filo in cadute {
            assert!(filo.join().is_err(), "il panico c'è stato");
        }
        // Un'altra impronta, a numero chiuso: oggi le sei cadute sono ancora in
        // volo, e non parte.
        finto.panica.store(false, Ordering::SeqCst);
        let prima = finto.scaricate.load(Ordering::SeqCst);
        richieste.richiedi(&finto, IMPRONTA, IN_VOLO_PER_LA_FINESTRA, || {});
        finto.aspetta();
        assert_eq!(
            finto.scaricate.load(Ordering::SeqCst),
            prima + 1,
            "la richiesta dopo i panici parte"
        );
        // E una di quelle cadute si riprova: non è arrivata, non è in volo.
        let prima = finto.scaricate.load(Ordering::SeqCst);
        richieste.richiedi(&finto, &format!("{:032x}", 0), 1, || {});
        finto.aspetta();
        assert_eq!(finto.scaricate.load(Ordering::SeqCst), prima + 1);
    }

    /// Una copertina che non arriva — l'altro non la ha, è spento — non
    /// ripubblica: una scheda uguale chiederebbe di nuovo, e di nuovo
    /// ripubblicherebbe. E non si richiede subito.
    #[test]
    fn una_copertina_che_non_arriva_non_ripubblica_e_non_si_richiede_subito() {
        let finto = Finto::nuovo(false, true);
        let richieste = Richieste::nuove(Duration::from_secs(3600));
        let (ripubblicate, arrivata) = contatore();
        richieste.richiedi(&finto, IMPRONTA, usize::MAX, arrivata);
        finto.aspetta();
        assert_eq!(finto.scaricate.load(Ordering::SeqCst), 1);
        assert_eq!(
            ripubblicate.load(Ordering::SeqCst),
            0,
            "niente da ripubblicare"
        );

        // Un altro stato del lettore, un minuto dopo: ancora niente.
        let (_, arrivata) = contatore();
        richieste.richiedi(&finto, IMPRONTA, usize::MAX, arrivata);
        finto.aspetta();
        assert_eq!(
            finto.scaricate.load(Ordering::SeqCst),
            1,
            "non si richiede prima del tempo"
        );
    }

    /// Passato il tempo, si riprova: l'altro può essere tornato.
    #[test]
    fn passato_il_tempo_una_copertina_non_arrivata_si_riprova() {
        let finto = Finto::nuovo(false, true);
        let richieste = Richieste::nuove(Duration::ZERO);
        for _ in 0..2 {
            let (_, arrivata) = contatore();
            richieste.richiedi(&finto, IMPRONTA, usize::MAX, arrivata);
            finto.aspetta();
        }
        assert_eq!(finto.scaricate.load(Ordering::SeqCst), 2);

        // E se stavolta arriva, arriva.
        *finto.porta.lock().unwrap() = true;
        let (ripubblicate, arrivata) = contatore();
        richieste.richiedi(&finto, IMPRONTA, usize::MAX, arrivata);
        finto.aspetta();
        assert_eq!(ripubblicate.load(Ordering::SeqCst), 1);
    }

    /// Una copertina che lo store ha già non si chiede a nessuno: la scheda la
    /// trova da sé, e non c'è niente da ripubblicare.
    #[test]
    fn una_copertina_gia_nello_store_non_si_chiede() {
        let finto = Finto::nuovo(true, true);
        finto
            .nello_store
            .lock()
            .unwrap()
            .insert(IMPRONTA.to_owned());
        let richieste = Richieste::nuove(RIPROVA);
        let (ripubblicate, arrivata) = contatore();
        richieste.richiedi(&finto, IMPRONTA, usize::MAX, arrivata);
        finto.aspetta();
        assert_eq!(
            finto.scaricate.load(Ordering::SeqCst),
            0,
            "nessuna richiesta"
        );
        assert_eq!(ripubblicate.load(Ordering::SeqCst), 0);
    }

    /// Solo una riga di un altro dispositivo chiede: una di qui senza il file
    /// della copertina non mette in moto la rete.
    #[test]
    fn solo_le_righe_di_un_altro_dispositivo_chiedono() {
        let chiave = "ab".repeat(32);
        assert!(e_di_un_altro_dispositivo(Some(&format!(
            "lontano://{chiave}/42"
        ))));
        assert!(!e_di_un_altro_dispositivo(Some("C:/Musica/uno.mp3")));
        assert!(!e_di_un_altro_dispositivo(Some("nuvola://drive/1.mp3")));
        assert!(!e_di_un_altro_dispositivo(None));
    }

    /// Non si ricorda tutto: un'impronta per ogni brano di un dispositivo che
    /// non risponde farebbe crescere la tabella per tutta la vita del processo.
    #[test]
    fn le_impronte_ricordate_sono_poche() {
        let finto = Finto::nuovo(false, true);
        let richieste = Richieste::nuove(Duration::from_secs(3600));
        for n in 0..(RICORDATE * 3) {
            let impronta = format!("{n:032x}");
            let (_, arrivata) = contatore();
            richieste.richiedi(&finto, &impronta, usize::MAX, arrivata);
            finto.aspetta();
        }
        let ricordate = richieste
            .ricordate
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len();
        assert!(ricordate <= RICORDATE, "{ricordate} impronte ricordate");
    }

    /// Il protocollo `rymembra-cover` non aspetta mai la rete (FC1, la WebView
    /// bloccata sul telefono vero): con la rete lenta — qui ferma, finché la
    /// prova non la lascia andare — la risposta alla finestra è **subito**, e
    /// quando la copertina arriva la pagina lo sa.
    ///
    /// Sul telefono il gestore gira dentro `shouldInterceptRequest` e quel filo
    /// resta fermo finché risponde: le copertine di una Libreria con centinaia di
    /// righe specchio lo tenevano occupato per la durata della rete, e le risposte
    /// dei comandi oltre gli 8 KiB — che passano dallo stesso posto — non tornavano.
    #[test]
    fn la_finestra_riceve_la_risposta_subito_anche_con_la_rete_ferma() {
        let finto = Finto::nuovo(true, false);
        let richieste = Richieste::nuove(RIPROVA);
        let risposte: Arc<Mutex<Vec<Option<Vec<u8>>>>> = Arc::new(Mutex::new(Vec::new()));
        let (arrivate, arrivata) = contatore();

        let inizio = Instant::now();
        let dentro = Arc::clone(&risposte);
        senza_aspettare(
            &richieste,
            &finto,
            IMPRONTA,
            None,
            move |r| dentro.lock().unwrap().push(r),
            arrivata,
        );
        assert!(
            inizio.elapsed() < Duration::from_millis(500),
            "ha aspettato la rete: {:?}",
            inizio.elapsed()
        );
        assert_eq!(
            *risposte.lock().unwrap(),
            vec![None],
            "«non c'è», subito, senza aspettare l'altro dispositivo"
        );
        assert_eq!(arrivate.load(Ordering::SeqCst), 0, "ancora niente");

        // La rete risponde: la copertina entra nello store e la pagina lo sa.
        finto.apri();
        finto.aspetta();
        assert_eq!(finto.scaricate.load(Ordering::SeqCst), 1, "una richiesta");
        assert_eq!(arrivate.load(Ordering::SeqCst), 1, "e la pagina avvisata");
    }

    /// Una copertina già nello store si serve com'era, e la rete non si tocca.
    #[test]
    fn una_copertina_nello_store_non_chiede_niente() {
        let finto = Finto::nuovo(true, false);
        let richieste = Richieste::nuove(RIPROVA);
        let risposte: Arc<Mutex<Vec<Option<Vec<u8>>>>> = Arc::new(Mutex::new(Vec::new()));
        let dentro = Arc::clone(&risposte);
        let (arrivate, arrivata) = contatore();
        senza_aspettare(
            &richieste,
            &finto,
            IMPRONTA,
            Some(vec![1, 2, 3]),
            move |r| dentro.lock().unwrap().push(r),
            arrivata,
        );
        finto.aspetta();
        assert_eq!(*risposte.lock().unwrap(), vec![Some(vec![1, 2, 3])]);
        assert_eq!(finto.scaricate.load(Ordering::SeqCst), 0);
        assert_eq!(arrivate.load(Ordering::SeqCst), 0);
    }

    /// Una pagina con centinaia di copertine mancanti non apre centinaia di
    /// richieste insieme — ognuna con un filo ad aspettare il suo posto per
    /// quindici secondi —: ne partono [`IN_VOLO_PER_LA_FINESTRA`], e le altre
    /// non si ricordano, così si richiedono quando c'è posto.
    #[test]
    fn la_finestra_chiede_a_numero_chiuso_e_le_altre_si_richiedono_dopo() {
        let finto = Finto::nuovo(true, false);
        let richieste = Richieste::nuove(RIPROVA);
        for n in 0..(IN_VOLO_PER_LA_FINESTRA * 4) {
            let impronta = format!("{n:032x}");
            senza_aspettare(&richieste, &finto, &impronta, None, |_| {}, || {});
        }
        assert_eq!(
            richieste
                .ricordate
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .len(),
            IN_VOLO_PER_LA_FINESTRA,
            "solo quelle in volo si ricordano"
        );
        // Arrivano: c'è di nuovo posto, e una già scartata parte.
        finto.apri();
        finto.aspetta();
        assert_eq!(
            finto.scaricate.load(Ordering::SeqCst),
            IN_VOLO_PER_LA_FINESTRA
        );
        let (arrivate, arrivata) = contatore();
        let scartata = format!("{:032x}", IN_VOLO_PER_LA_FINESTRA * 4 - 1);
        senza_aspettare(&richieste, &finto, &scartata, None, |_| {}, arrivata);
        finto.aspetta();
        assert_eq!(arrivate.load(Ordering::SeqCst), 1, "ora parte");
    }
}
