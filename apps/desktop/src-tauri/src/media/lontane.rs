//! La copertina di un brano di un altro dispositivo, per le schede del sistema.
//!
//! # Il difetto che questo modulo toglie
//!
//! Un brano che suona dal PC collegato porta come copertina una chiave
//! (`lontano-<dispositivo>-<impronta>`), non un'impronta della libreria. La
//! finestra la sa chiedere, passando da `rymembra-cover` a
//! [`crate::lontano::copertina`], che la scarica e la tiene in cache. La scheda
//! di Android (l'auto, la notifica, la schermata di blocco) e quella di Windows
//! (le SMTC) invece vogliono un **file**, e lo cercavano nello store delle
//! copertine di questo dispositivo: lì la chiave non è un'impronta, il percorso
//! veniva sbagliato, e la scheda restava senza immagine.
//!
//! # Come si risolve
//!
//! Se il file è già nella cache delle copertine lontane, ci si punta. Se non
//! c'è, si chiede all'altro **una volta sola**, in disparte, e a copertina
//! arrivata si ripubblica la scheda: chi chiama dice come. Fra le due cose la
//! scheda esce senza copertina, com'era, e non aspetta la rete.
//!
//! Le voci di «Ascolta insieme» (`insieme-<uid>`) avevano lo stesso difetto, ma
//! non passano di qui: la loro copertina arriva con la voce e sta già in
//! memoria, quindi non c'è niente da chiedere. La prima volta che una scheda la
//! vuole come file si scrive in una cartella riservata della stessa cache
//! (`crate::insieme::copertina_su_file`), e alla fine della sessione se ne va.
//!
//! # Tre cose che non devono succedere
//!
//! - **Due richieste per la stessa copertina.** Lo stato del lettore parte a
//!   ogni pausa e a ogni salto: finché la prima richiesta è in volo, le altre
//!   non ne fanno partire una seconda.
//! - **Un ciclo.** Si ripubblica solo se la copertina è davvero in cache dopo
//!   la richiesta: una che non arriva (l'altro non la ha, è spento, la cache
//!   non c'è) ripubblicata sarebbe una scheda uguale, che chiede di nuovo, che
//!   ripubblica. Una chiave che non è arrivata non si richiede per un minuto,
//!   e se ne ricordano poche.
//! - **Aspettare la rete dove non si può.** `trova` gira dentro il lucchetto
//!   dei controlli multimediali: la richiesta parte su un altro filo, sempre.
//!
//! # Il cancello
//!
//! Il file si guarda solo se il dispositivo è ancora collegato
//! ([`crate::lontano::sfoglia::file_in_cache`]): le copertine di un dispositivo
//! dimenticato non devono restare sulla schermata di blocco (reperto R01-5,
//! lo stesso cancello di `copertina_in`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Quanto si aspetta prima di richiedere una copertina che non è arrivata.
const RIPROVA: Duration = Duration::from_secs(60);

/// Quante chiavi si ricordano al più, fra quelle in volo e quelle non arrivate.
const RICORDATE: usize = 256;

/// Quel che serve a trovare una copertina: la cache, la rete, un filo.
///
/// Un trait perché le prove non hanno né un'applicazione né un altro
/// dispositivo: la cucitura è qui, dove il comportamento dipende da tempi e
/// da chi risponde.
pub(super) trait Ambiente: Clone + Send + 'static {
    /// Il file della copertina di `chiave`, se è in cache **e** il dispositivo
    /// è ancora collegato.
    fn in_cache(&self, chiave: &str) -> Option<PathBuf>;

    /// Chiede l'immagine all'altro e la mette in cache. Aspetta la rete:
    /// gira sempre in disparte.
    fn scarica(&self, chiave: &str);

    /// Manda `lavoro` su un altro filo.
    fn in_disparte(&self, lavoro: Box<dyn FnOnce() + Send>);
}

/// Cosa si sa di una chiave che non è in cache.
enum Stato {
    /// La richiesta è partita e non è tornata.
    InVolo,
    /// È tornata senza la copertina, a quest'ora.
    NonArrivata(Instant),
}

/// Le richieste di copertine lontane fatte da chi non è la finestra.
#[derive(Clone)]
pub(super) struct Richieste {
    ricordate: Arc<Mutex<HashMap<String, Stato>>>,
    riprova: Duration,
}

impl Richieste {
    /// Nessuna richiesta fatta; una chiave non arrivata si riprova dopo
    /// `riprova`.
    pub(super) fn nuove(riprova: Duration) -> Self {
        Self {
            ricordate: Arc::new(Mutex::new(HashMap::new())),
            riprova,
        }
    }

    /// Il file della copertina di `chiave`, se è già in cache.
    ///
    /// Se non c'è e `chiedi`, la chiede all'altro in disparte, una volta
    /// sola: quando è arrivata chiama `arrivata`. Senza `chiedi`, una
    /// copertina che non è in cache non la si va a cercare: per le voci della
    /// coda che l'auto mostra, che non devono mettere in fila cinquanta
    /// richieste.
    pub(super) fn trova<A: Ambiente>(
        &self,
        ambiente: &A,
        chiave: &str,
        chiedi: bool,
        arrivata: impl FnOnce() + Send + 'static,
    ) -> Option<PathBuf> {
        if let Some(file) = ambiente.in_cache(chiave) {
            return Some(file);
        }
        if !chiedi || !self.comincia(chiave) {
            return None;
        }
        let richieste = self.clone();
        let ambiente = ambiente.clone();
        let chiave = chiave.to_owned();
        let per_lanciare = ambiente.clone();
        per_lanciare.in_disparte(Box::new(move || {
            ambiente.scarica(&chiave);
            // Si ripubblica solo se la copertina è davvero in cache: vedi «Un
            // ciclo» in cima al modulo.
            let portata = ambiente.in_cache(&chiave).is_some();
            richieste.finita(&chiave, portata);
            if portata {
                arrivata();
            }
        }));
        None
    }

    /// Segna che la richiesta di `chiave` parte adesso. `false` se non deve
    /// partire: è già in volo, o non è arrivata da meno di `riprova`.
    fn comincia(&self, chiave: &str) -> bool {
        let mut dentro = self
            .ricordate
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match dentro.get(chiave) {
            Some(Stato::InVolo) => return false,
            Some(Stato::NonArrivata(quando)) if quando.elapsed() < self.riprova => return false,
            _ => {}
        }
        if dentro.len() >= RICORDATE && !dentro.contains_key(chiave) {
            // Si dimentica quel che non è arrivato: al più si riprova una
            // volta in più. Quel che è in volo resta, perché la sua fine
            // deve trovare il suo posto.
            dentro.retain(|_, stato| matches!(stato, Stato::InVolo));
            if dentro.len() >= RICORDATE {
                return false;
            }
        }
        dentro.insert(chiave.to_owned(), Stato::InVolo);
        true
    }

    /// La richiesta di `chiave` è tornata: se la copertina è arrivata non c'è
    /// altro da ricordare (la cache la trova), altrimenti si segna l'ora.
    fn finita(&self, chiave: &str, portata: bool) {
        let mut dentro = self
            .ricordate
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if portata {
            dentro.remove(chiave);
        } else {
            dentro.insert(chiave.to_owned(), Stato::NonArrivata(Instant::now()));
        }
    }
}

// ── l'applicazione ──────────────────────────────────────────────────────────

/// L'applicazione vera: la cache e la rete di [`crate::lontano`].
#[derive(Clone)]
struct ViaApp(tauri::AppHandle);

impl Ambiente for ViaApp {
    fn in_cache(&self, chiave: &str) -> Option<PathBuf> {
        crate::lontano::sfoglia::file_in_cache(&self.0, chiave)
    }

    fn scarica(&self, chiave: &str) {
        if let Err(err) = crate::lontano::copertina(&self.0, chiave) {
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

/// Il file della copertina di `chiave` (una chiave lontana), se è già in
/// cache; altrimenti la richiede una volta sola, e a copertina arrivata
/// chiama `arrivata`. Vedi [`Richieste::trova`].
pub(super) fn file_di(
    app: &tauri::AppHandle,
    chiave: &str,
    chiedi: bool,
    arrivata: impl FnOnce(&tauri::AppHandle) + Send + 'static,
) -> Option<PathBuf> {
    let mano = app.clone();
    RICHIESTE.trova(&ViaApp(app.clone()), chiave, chiedi, move || {
        arrivata(&mano)
    })
}

#[cfg(test)]
mod prove {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Condvar;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread::JoinHandle;

    const CHIAVE: &str =
        "lontano-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-0123456789abcdef";

    /// Un ambiente senza disco e senza rete: la cache è un insieme di chiavi, e
    /// lo scaricamento aspetta che la prova lo lasci andare.
    #[derive(Clone)]
    struct Finto {
        in_cache: Arc<Mutex<HashSet<String>>>,
        /// Quante volte è partito lo scaricamento.
        scaricate: Arc<AtomicUsize>,
        /// Se lo scaricamento porta la copertina o torna a mani vuote.
        porta: Arc<Mutex<bool>>,
        /// Lo scaricamento aspetta qui.
        cancello: Arc<(Mutex<bool>, Condvar)>,
        fili: Arc<Mutex<Vec<JoinHandle<()>>>>,
    }

    impl Finto {
        fn nuovo(porta: bool, aperto: bool) -> Self {
            Self {
                in_cache: Arc::new(Mutex::new(HashSet::new())),
                scaricate: Arc::new(AtomicUsize::new(0)),
                porta: Arc::new(Mutex::new(porta)),
                cancello: Arc::new((Mutex::new(aperto), Condvar::new())),
                fili: Arc::new(Mutex::new(Vec::new())),
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
        fn in_cache(&self, chiave: &str) -> Option<PathBuf> {
            self.in_cache
                .lock()
                .unwrap()
                .contains(chiave)
                .then(|| PathBuf::from(format!("cache/{chiave}.jpg")))
        }

        fn scarica(&self, chiave: &str) {
            self.scaricate.fetch_add(1, Ordering::SeqCst);
            let (aperto, quando) = &*self.cancello;
            let mut dentro = aperto.lock().unwrap();
            while !*dentro {
                dentro = quando.wait(dentro).unwrap();
            }
            drop(dentro);
            if *self.porta.lock().unwrap() {
                self.in_cache.lock().unwrap().insert(chiave.to_owned());
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

    /// F1 (#1): la scheda esce una prima volta senza copertina, e quando la
    /// copertina arriva ne esce una seconda, con lei.
    #[test]
    fn una_copertina_arrivata_produce_una_seconda_pubblicazione() {
        let finto = Finto::nuovo(true, false);
        let richieste = Richieste::nuove(RIPROVA);
        let (ripubblicate, arrivata) = contatore();

        // Prima pubblicazione: la copertina non c'è, e non si aspetta.
        assert_eq!(richieste.trova(&finto, CHIAVE, true, arrivata), None);
        assert_eq!(ripubblicate.load(Ordering::SeqCst), 0, "non ancora");

        // Arriva.
        finto.apri();
        finto.aspetta();
        assert_eq!(
            ripubblicate.load(Ordering::SeqCst),
            1,
            "una ripubblicazione"
        );

        // Seconda pubblicazione: il file c'è, e senza chiedere di nuovo.
        let (altre, arrivata) = contatore();
        assert_eq!(
            richieste.trova(&finto, CHIAVE, true, arrivata),
            Some(PathBuf::from(format!("cache/{CHIAVE}.jpg")))
        );
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
            let esito = richieste.trova(&finto, CHIAVE, true, move || {
                conti.fetch_add(1, Ordering::SeqCst);
            });
            assert_eq!(esito, None);
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

    /// Una copertina che non arriva — l'altro non la ha, è spento, la cache
    /// non c'è — non ripubblica: una scheda uguale chiederebbe di nuovo, e di
    /// nuovo ripubblicherebbe. E non si richiede subito.
    #[test]
    fn una_copertina_che_non_arriva_non_ripubblica_e_non_si_richiede_subito() {
        let finto = Finto::nuovo(false, true);
        let richieste = Richieste::nuove(Duration::from_secs(3600));
        let (ripubblicate, arrivata) = contatore();
        assert_eq!(richieste.trova(&finto, CHIAVE, true, arrivata), None);
        finto.aspetta();
        assert_eq!(finto.scaricate.load(Ordering::SeqCst), 1);
        assert_eq!(
            ripubblicate.load(Ordering::SeqCst),
            0,
            "niente da ripubblicare"
        );

        // Un altro stato del lettore, un minuto dopo: ancora niente.
        let (_, arrivata) = contatore();
        assert_eq!(richieste.trova(&finto, CHIAVE, true, arrivata), None);
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
            assert_eq!(richieste.trova(&finto, CHIAVE, true, arrivata), None);
            finto.aspetta();
        }
        assert_eq!(finto.scaricate.load(Ordering::SeqCst), 2);

        // E se stavolta arriva, arriva.
        *finto.porta.lock().unwrap() = true;
        let (ripubblicate, arrivata) = contatore();
        assert_eq!(richieste.trova(&finto, CHIAVE, true, arrivata), None);
        finto.aspetta();
        assert_eq!(ripubblicate.load(Ordering::SeqCst), 1);
    }

    /// La coda che l'auto mostra guarda la cache e basta, tranne per il brano
    /// dopo: `chiedi` è falso per le altre voci.
    #[test]
    fn senza_chiedere_si_guarda_la_cache_e_non_la_rete() {
        let finto = Finto::nuovo(true, true);
        let richieste = Richieste::nuove(RIPROVA);
        let (_, arrivata) = contatore();
        assert_eq!(richieste.trova(&finto, CHIAVE, false, arrivata), None);
        finto.aspetta();
        assert_eq!(
            finto.scaricate.load(Ordering::SeqCst),
            0,
            "nessuna richiesta"
        );

        // Quel che è già in cache si trova comunque, chiedendo o no.
        finto.in_cache.lock().unwrap().insert(CHIAVE.to_owned());
        for chiedi in [false, true] {
            let (_, arrivata) = contatore();
            assert!(richieste.trova(&finto, CHIAVE, chiedi, arrivata).is_some());
        }
        finto.aspetta();
        assert_eq!(finto.scaricate.load(Ordering::SeqCst), 0);
    }

    /// Non si ricorda tutto: una chiave per ogni brano di un dispositivo che
    /// non risponde farebbe crescere la tabella per tutta la vita del processo.
    #[test]
    fn le_chiavi_ricordate_sono_poche() {
        let finto = Finto::nuovo(false, true);
        let richieste = Richieste::nuove(Duration::from_secs(3600));
        for n in 0..(RICORDATE * 3) {
            let chiave = format!("lontano-{}-{n:016x}", "b".repeat(64));
            let (_, arrivata) = contatore();
            richieste.trova(&finto, &chiave, true, arrivata);
            finto.aspetta();
        }
        let ricordate = richieste
            .ricordate
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len();
        assert!(ricordate <= RICORDATE, "{ricordate} chiavi ricordate");
    }
}
