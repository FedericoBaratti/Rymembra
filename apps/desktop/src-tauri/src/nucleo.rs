//! Il nucleo del processo: gli stati che non dipendono da una finestra, e l'uscita
//! degli eventi, con o senza la finestra.
//!
//! # Perché esiste
//!
//! Fino alla 3.2 tutto il nucleo — la libreria, il lettore, i controlli del
//! sistema, il collegamento con gli altri dispositivi — nasceva dentro il `setup`
//! di Tauri e parlava con il resto attraverso un `tauri::AppHandle`: gli stati
//! (`app.state::<T>()`), gli eventi (`app.emit`). Su Android, però, Tauri non
//! nasce senza un'attività (`register_android_plugin` → `wry::dispatch` →
//! `first_activity_id().expect("no available activity")`), e quello che deve
//! partire all'accensione del telefono è proprio il nucleo, senza nessuna
//! attività. Il [`Nucleo`] è quel che il nucleo possiede senza Tauri: la mappa
//! degli stati, e un'uscita degli eventi che con la finestra fa `emit` e senza
//! tace. Quando la finestra nasce, il suo `setup` si **aggancia** al nucleo che
//! c'è già ([`Nucleo::aggancia`]): gli stati diventano anche stati gestiti di
//! Tauri — gli stessi oggetti, non copie — e gli eventi cominciano a partire.
//!
//! # Un nucleo per processo
//!
//! [`Nucleo::del_processo`] è l'unico che il programma usa: nasce alla prima
//! richiesta e vive quanto il processo. È così che il servizio che parte
//! all'accensione e l'attività che nasce dopo, nello stesso processo, trovano
//! la stessa libreria, lo stesso lettore, la stessa coda.
//!
//! # Gli stati sono handle
//!
//! Uno stato che il nucleo e Tauri possiedono insieme è un handle `Clone`
//! (`struct StatoX(Arc<CorpoX>)`, vedi [`condiviso!`]): il nucleo ne tiene un
//! clone, Tauri un altro, e i comandi che prendono `State<'_, StatoX>` vedono lo
//! stesso corpo. I moduli che non hanno bisogno di una finestra cercano lo stato
//! nel nucleo ([`Nucleo::try_state`]); quelli che ce l'hanno continuano a
//! cercarlo in Tauri, e lo trovano.
//!
//! # Gli eventi
//!
//! [`Nucleo::emetti`] prende il posto di `Emette::emetti` (`spegnimento.rs`)
//! per le parti del nucleo: con la finestra agganciata fa `emit` — e un errore
//! di `emit` si dice nel diario, la prima volta per ogni evento —; senza non
//! ha nessuno a cui parlare, e la finestra che nascerà dopo rilegge lo stato dai
//! comandi invece di ascoltare quel che ha perso. Un evento che dice un
//! errore (`…:errore`) senza finestra finisce nel diario, perché un guasto di
//! cui nessuno ha letto niente non resta senza traccia.
//!
//! Gli ascoltatori ([`Nucleo::ascolta`]) sono del nucleo, non di Tauri: chi
//! ascolta `scansione:finita` lo sente anche quando la finestra non c'è.

#![allow(
    dead_code,
    reason = "la fetta delle fondamenta (F8): gli usi arrivano con le fette dopo, e questa riga si toglie con l'ultima"
)]

use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock};

use serde::Serialize;
use tauri::{AppHandle, Emitter as _, Manager as _};

/// Uno stato che il nucleo e Tauri possiedono insieme: un handle `Clone` su un
/// corpo che sta dietro un `Arc`, con `Deref` verso il corpo.
///
/// Si usa così: il corpo mantiene la struttura di prima (`CorpoX`), l'handle
/// ha il nome che i comandi e il resto del codice già usano (`StatoX`), e i
/// metodi di `impl StatoX` restano dove sono, perché `self.campo` passa dal
/// `Deref`. Il costruttore fa `Self::da_corpo(CorpoX { .. })`.
macro_rules! condiviso {
    ($(#[$meta:meta])* $vis:vis struct $nome:ident($corpo:ty);) => {
        $(#[$meta])*
        #[derive(Clone)]
        $vis struct $nome(::std::sync::Arc<$corpo>);

        impl $nome {
            /// L'handle di un corpo appena costruito.
            #[allow(
                dead_code,
                reason = "lo usano i costruttori; dove ne manca uno compilato, il corpo si costruisce altrove"
            )]
            pub(crate) fn da_corpo(corpo: $corpo) -> Self {
                Self(::std::sync::Arc::new(corpo))
            }
        }

        impl ::std::ops::Deref for $nome {
            type Target = $corpo;

            fn deref(&self) -> &$corpo {
                &self.0
            }
        }
    };
}
pub(crate) use condiviso;

/// Il nucleo di un processo. Si clona per quasi niente: dentro c'è un `Arc`.
#[derive(Clone)]
pub struct Nucleo(Arc<Dentro>);

/// Come si porta uno stato dentro Tauri: un clone dello stesso handle.
type PerTauri = Box<dyn Fn(&AppHandle) + Send + Sync>;

/// Quel che il nucleo sa di uno stato: l'handle, e come darlo a Tauri.
struct Voce {
    valore: Box<dyn Any + Send + Sync>,
    per_tauri: PerTauri,
}

/// Un ascoltatore del nucleo.
type Ascoltatore = Arc<dyn Fn() + Send + Sync>;

/// Un lavoro che aspetta la fine dell'avvio del nucleo.
type Dopo = Box<dyn FnOnce() + Send>;

/// A che punto è l'avvio del nucleo: vedi [`Nucleo::reclama_l_avvio`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fase {
    /// Nessuno ha cominciato.
    DaFare,
    /// Qualcuno ha cominciato.
    InCorso,
    /// Finito.
    Finito,
}

/// L'avvio, e quel che aspetta che finisca.
struct Avvio {
    fase: Fase,
    /// I lavori di chi ha chiesto di partire a avvio finito.
    dopo: Vec<Dopo>,
}

/// Chi ha cominciato l'avvio del nucleo, visto da chi lo chiede.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChiAvvia {
    /// Tocca a te: il nucleo non è mai partito in questo processo.
    Tu,
    /// Sta partendo adesso, da un altro: aspetta con [`Nucleo::dopo_l_avvio`].
    GiaInCorso,
    /// È già partito.
    GiaFinito,
}

struct Dentro {
    /// Gli stati, per tipo.
    stati: RwLock<HashMap<TypeId, Voce>>,
    /// La finestra, se c'è: l'`AppHandle` di Tauri, **cancellato** in un `dyn Any`. Si
    /// scrive una volta.
    ///
    /// Cancellato perché il distruttore di un `AppHandle` porta con sé quello
    /// dell'intero motore della finestra (`tao`, `wry`), e un eseguibile di prova che
    /// lo raggiunge non parte più: vedi [`Nucleo::emetti`]. Dietro un `dyn Any` il
    /// distruttore lo conosce solo chi crea la scatola, [`Nucleo::aggancia`].
    finestra: OnceLock<Box<dyn Any + Send + Sync>>,
    /// Chi ascolta un evento, per nome.
    ascoltatori: Mutex<Vec<(&'static str, Ascoltatore)>>,
    /// L'avvio.
    avvio: Mutex<Avvio>,
    /// Gli eventi il cui `emit` è già fallito una volta: il diario lo dice una volta sola.
    emit_fallito: Mutex<HashSet<String>>,
}

/// Il nucleo di questo processo, creato alla prima richiesta.
static DEL_PROCESSO: OnceLock<Nucleo> = OnceLock::new();

impl Nucleo {
    /// Un nucleo vuoto, senza finestra. Il programma usa [`Nucleo::del_processo`];
    /// questo serve alle prove.
    #[must_use]
    pub fn nuovo() -> Self {
        Self(Arc::new(Dentro {
            stati: RwLock::new(HashMap::new()),
            finestra: OnceLock::new(),
            ascoltatori: Mutex::new(Vec::new()),
            avvio: Mutex::new(Avvio {
                fase: Fase::DaFare,
                dopo: Vec::new(),
            }),
            emit_fallito: Mutex::new(HashSet::new()),
        }))
    }

    /// Il nucleo di questo processo. Non fallisce mai, e lo stesso `Nucleo` serve
    /// il servizio che parte all'accensione e la finestra che nasce dopo.
    #[must_use]
    pub fn del_processo() -> Self {
        DEL_PROCESSO.get_or_init(Self::nuovo).clone()
    }

    /// Il nucleo, per chi ha in mano un `AppHandle`: è lo stesso del processo.
    ///
    /// L'argomento non serve a trovarlo: dice solo, a chi legge, che da qui si
    /// passa dalla finestra al nucleo.
    #[must_use]
    pub fn di(_app: &AppHandle) -> Self {
        Self::del_processo()
    }

    /// La finestra, se c'è.
    #[must_use]
    pub fn finestra(&self) -> Option<&AppHandle> {
        self.0
            .finestra
            .get()
            .and_then(|finestra| finestra.downcast_ref::<AppHandle>())
    }

    /// Quel che serve solo con una finestra, e che senza non si fa.
    ///
    /// Per le pochissime chiamate dal nucleo a moduli che nascono con la finestra
    /// (il vassoio, il backup): senza finestra la risposta è `None`, e chi chiama
    /// decide che cosa vuol dire.
    pub fn con_finestra<T>(&self, lavoro: impl FnOnce(&AppHandle) -> T) -> Option<T> {
        self.finestra().map(lavoro)
    }

    /// Registra uno stato. Come `Manager::manage`: `false` se quel tipo c'è già.
    ///
    /// Con la finestra agganciata, lo stato entra subito anche in Tauri.
    pub fn manage<T: Clone + Send + Sync + 'static>(&self, valore: T) -> bool {
        let per_tauri = valore.clone();
        {
            let mut stati = self.0.stati.write().unwrap_or_else(PoisonError::into_inner);
            if stati.contains_key(&TypeId::of::<T>()) {
                return false;
            }
            let copia = valore.clone();
            stati.insert(
                TypeId::of::<T>(),
                Voce {
                    valore: Box::new(valore),
                    per_tauri: Box::new(move |app| {
                        app.manage(copia.clone());
                    }),
                },
            );
        }
        // Fuori dal lucchetto: `manage` di Tauri ne ha uno suo. Se l'aggancio
        // arriva in questo istante, questa riga e il suo giro registrano lo
        // stesso handle due volte, e la seconda `manage` non fa niente.
        if let Some(app) = self.finestra() {
            app.manage(per_tauri);
        }
        true
    }

    /// Lo stato di tipo `T`, se c'è.
    #[must_use]
    pub fn try_state<T: Clone + Send + Sync + 'static>(&self) -> Option<T> {
        self.0
            .stati
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&TypeId::of::<T>())
            .and_then(|voce| voce.valore.downcast_ref::<T>())
            .cloned()
    }

    /// Lo stato di tipo `T`; **va in panico** se non c'è, come `Manager::state`,
    /// di cui prende il posto per le parti del nucleo che lo usavano. Chi può
    /// trovarsi a nucleo non ancora riempito usa [`Nucleo::try_state`].
    #[must_use]
    pub fn state<T: Clone + Send + Sync + 'static>(&self) -> T {
        self.try_state::<T>().unwrap_or_else(|| non_gestito::<T>())
    }

    /// Si aggancia alla finestra: gli stati che il nucleo ha già diventano anche
    /// stati gestiti di Tauri, e da qui gli eventi partono verso la pagina.
    ///
    /// Una volta sola per processo: `false` se c'era già una finestra agganciata.
    /// Va chiamata nel `setup` di Tauri, prima di ogni altra cosa che registri
    /// stati: i comandi, che li cercano per tipo, non devono trovarne meno di
    /// quelli che il nucleo ha.
    pub fn aggancia(&self, app: &AppHandle) -> bool {
        if self.0.finestra.set(Box::new(app.clone())).is_err() {
            return false;
        }
        let stati = self.0.stati.read().unwrap_or_else(PoisonError::into_inner);
        for voce in stati.values() {
            (voce.per_tauri)(app);
        }
        true
    }

    /// Manda un evento alla pagina, se c'è. Con l'uscita già cominciata non fa
    /// niente: vedi `spegnimento::Emette`, che è il motivo e che questo ripete.
    ///
    /// # Perché è diviso in tre
    ///
    /// Le prove non toccano mai l'`AppHandle`: un `emit` vero porta nel binario di
    /// prova il motore della finestra (`tao`, `wry`), che chiede al sistema
    /// `TaskDialogIndirect` di `comctl32` versione 6, e un eseguibile di prova senza
    /// il manifesto che la chiede non parte più (`STATUS_ENTRYPOINT_NOT_FOUND`).
    /// Per questo la parte con la finestra ([`Nucleo::alla_finestra`]) è a parte, e
    /// le prove girano su [`Nucleo::notifica`] e [`Nucleo::senza_finestra`].
    pub fn emetti<C: Serialize + Clone>(&self, evento: &str, carico: C) {
        if crate::spegnimento::in_uscita() {
            return;
        }
        self.notifica(evento);
        match self.finestra() {
            Some(app) => self.alla_finestra(app, evento, carico),
            None => self.senza_finestra(evento, &carico),
        }
    }

    /// L'evento con la finestra agganciata: un `emit`, e un errore si dice nel
    /// diario la prima volta per ogni evento.
    fn alla_finestra<C: Serialize + Clone>(&self, app: &AppHandle, evento: &str, carico: C) {
        if let Err(err) = app.emit(evento, carico) {
            let prima_volta = self
                .0
                .emit_fallito
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(evento.to_owned());
            if prima_volta {
                crate::nota!("[nucleo] l'evento {evento} non parte: {err}");
            }
        }
    }

    /// L'evento senza finestra: nessuno ascolta, ma un errore non si perde, e
    /// resta nel diario.
    fn senza_finestra<C: Serialize>(&self, evento: &str, carico: &C) {
        if let Some(riga) = riga_senza_finestra(evento, carico) {
            crate::nota!("{riga}");
        }
    }

    /// Chiama `ascoltatore` ogni volta che il nucleo emette `evento`, con o
    /// senza finestra. È il `listen_any` di Tauri, ma del nucleo.
    pub fn ascolta(&self, evento: &'static str, ascoltatore: impl Fn() + Send + Sync + 'static) {
        self.0
            .ascoltatori
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((evento, Arc::new(ascoltatore)));
    }

    /// Avvisa chi ascolta `evento`. Gli ascoltatori girano fuori dal lucchetto.
    fn notifica(&self, evento: &str) {
        let da_chiamare: Vec<Ascoltatore> = self
            .0
            .ascoltatori
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(nome, _)| *nome == evento)
            .map(|(_, ascoltatore)| Arc::clone(ascoltatore))
            .collect();
        for ascoltatore in da_chiamare {
            ascoltatore();
        }
    }

    /// Chi avvia il nucleo di questo processo? Il primo che lo chiede: lui lo avvia
    /// e alla fine dice [`Nucleo::avvio_finito`]; gli altri aspettano.
    ///
    /// È il modo in cui il servizio che parte all'accensione e l'attività che
    /// nasce dopo non avviano due volte la libreria e il lettore.
    pub fn reclama_l_avvio(&self) -> ChiAvvia {
        let mut avvio = self.0.avvio.lock().unwrap_or_else(PoisonError::into_inner);
        match avvio.fase {
            Fase::DaFare => {
                avvio.fase = Fase::InCorso;
                ChiAvvia::Tu
            }
            Fase::InCorso => ChiAvvia::GiaInCorso,
            Fase::Finito => ChiAvvia::GiaFinito,
        }
    }

    /// L'avvio è finito: parte il lavoro di chi aspettava.
    pub fn avvio_finito(&self) {
        let da_fare = {
            let mut avvio = self.0.avvio.lock().unwrap_or_else(PoisonError::into_inner);
            avvio.fase = Fase::Finito;
            std::mem::take(&mut avvio.dopo)
        };
        for lavoro in da_fare {
            lavoro();
        }
    }

    /// Fa `lavoro` quando l'avvio è finito: subito, se lo è già, sul filo che
    /// chiama; altrimenti sul filo che lo finisce.
    ///
    /// Se nessuno ha ancora cominciato l'avvio, il lavoro si accoda lo stesso e
    /// parte quando qualcuno lo avrà fatto: accodarsi non è cominciare.
    pub fn dopo_l_avvio(&self, lavoro: impl FnOnce() + Send + 'static) {
        let mut avvio = self.0.avvio.lock().unwrap_or_else(PoisonError::into_inner);
        if avvio.fase == Fase::Finito {
            drop(avvio);
            lavoro();
        } else {
            avvio.dopo.push(Box::new(lavoro));
        }
    }
}

/// La riga di diario per un evento che nessuno riceve, se vale la pena: solo
/// quelli che dicono un errore (`…:errore`), con quel che portano.
fn riga_senza_finestra<C: Serialize>(evento: &str, carico: &C) -> Option<String> {
    if !evento.ends_with(":errore") {
        return None;
    }
    let testo = serde_json::to_string(carico).unwrap_or_else(|err| format!("illeggibile ({err})"));
    Some(format!("[nucleo] senza finestra, {evento}: {testo}"))
}

/// Lo stato che `Manager::state` non trova: un errore di chi scrive il codice.
#[cold]
fn non_gestito<T>() -> T {
    #[allow(
        clippy::panic,
        reason = "è il comportamento di `Manager::state`, che questo metodo sostituisce: chi può non trovare lo stato usa `try_state`"
    )]
    {
        panic!(
            "lo stato {} non è registrato nel nucleo",
            std::any::type_name::<T>()
        )
    }
}

/// Il nucleo è un argomento dei comandi, come l'`AppHandle`: quello del processo.
///
/// Così un comando che prima prendeva `app: tauri::AppHandle` per cercare gli
/// stati e mandare eventi prende `app: Nucleo` e non cambia riga.
impl<'de, R: tauri::Runtime> tauri::ipc::CommandArg<'de, R> for Nucleo {
    fn from_command(
        _comando: tauri::ipc::CommandItem<'de, R>,
    ) -> Result<Self, tauri::ipc::InvokeError> {
        Ok(Self::del_processo())
    }
}

#[cfg(test)]
mod prove {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// Uno stato di prova: un handle su un contatore, come gli stati veri.
    #[derive(Clone)]
    struct Contatore(Arc<AtomicUsize>);

    #[test]
    fn uno_stato_registrato_si_ritrova_ed_e_lo_stesso_oggetto() {
        let nucleo = Nucleo::nuovo();
        assert!(nucleo.try_state::<Contatore>().is_none(), "nasce vuoto");
        let contatore = Contatore(Arc::new(AtomicUsize::new(0)));
        assert!(nucleo.manage(contatore.clone()));
        contatore.0.fetch_add(3, Ordering::SeqCst);
        let trovato = nucleo.try_state::<Contatore>().expect("registrato");
        assert_eq!(
            trovato.0.load(Ordering::SeqCst),
            3,
            "è l'handle dello stesso corpo, non una copia"
        );
    }

    #[test]
    fn un_tipo_si_registra_una_volta_sola() {
        let nucleo = Nucleo::nuovo();
        let primo = Contatore(Arc::new(AtomicUsize::new(1)));
        let secondo = Contatore(Arc::new(AtomicUsize::new(2)));
        assert!(nucleo.manage(primo));
        assert!(!nucleo.manage(secondo), "come `Manager::manage`");
        assert_eq!(
            nucleo
                .try_state::<Contatore>()
                .map(|c| c.0.load(Ordering::SeqCst)),
            Some(1),
            "resta il primo"
        );
    }

    #[test]
    fn un_nucleo_clonato_e_lo_stesso_nucleo() {
        let nucleo = Nucleo::nuovo();
        let clone = nucleo.clone();
        nucleo.manage(Contatore(Arc::new(AtomicUsize::new(7))));
        assert!(clone.try_state::<Contatore>().is_some());
    }

    #[test]
    fn il_nucleo_del_processo_e_uno_solo() {
        let a = Nucleo::del_processo();
        let b = Nucleo::del_processo();
        assert!(Arc::ptr_eq(&a.0, &b.0), "due richieste, due nuclei");
    }

    /// Quel che `emetti` fa senza finestra, senza la bandiera dell'uscita (una
    /// statica di tutto il processo, che un'altra prova può aver alzato) e senza
    /// l'`AppHandle` (vedi [`Nucleo::emetti`]): gli ascoltatori, poi il diario.
    fn emetti_senza_finestra<C: Serialize>(nucleo: &Nucleo, evento: &str, carico: &C) {
        assert!(nucleo.finestra().is_none(), "la prova non ha una finestra");
        nucleo.notifica(evento);
        nucleo.senza_finestra(evento, carico);
    }

    /// L'uscita senza finestra: gli ascoltatori sentono lo stesso, e un evento
    /// qualunque non fallisce né si accumula da nessuna parte.
    #[test]
    fn senza_finestra_gli_eventi_arrivano_agli_ascoltatori_e_a_nessun_altro() {
        let nucleo = Nucleo::nuovo();
        let sentiti = Arc::new(AtomicUsize::new(0));
        let dentro = Arc::clone(&sentiti);
        nucleo.ascolta("scansione:finita", move || {
            dentro.fetch_add(1, Ordering::SeqCst);
        });
        emetti_senza_finestra(&nucleo, "scansione:finita", &());
        emetti_senza_finestra(&nucleo, "altro:evento", &42_u32);
        emetti_senza_finestra(&nucleo, "scansione:finita", &());
        assert_eq!(
            sentiti.load(Ordering::SeqCst),
            2,
            "solo l'evento ascoltato, e a ogni volta"
        );
    }

    /// Un errore senza finestra non si perde: finisce nel diario, con quel che
    /// dice. Gli altri eventi no.
    #[test]
    fn un_errore_senza_finestra_lascia_una_riga_nel_diario() {
        let riga = riga_senza_finestra("riproduzione:errore", &"non riesce a suonarlo")
            .expect("un errore ha la sua riga");
        assert!(
            riga.contains("riproduzione:errore") && riga.contains("non riesce a suonarlo"),
            "l'errore non è nel diario: {riga:?}"
        );
        assert!(
            riga_senza_finestra("riproduzione:stato", &"ok").is_none(),
            "un evento che non è un errore non va nel diario"
        );
    }

    #[test]
    fn un_ascoltatore_puo_registrarne_un_altro_senza_bloccarsi() {
        let nucleo = Nucleo::nuovo();
        let dentro = nucleo.clone();
        nucleo.ascolta("a:b", move || {
            dentro.ascolta("c:d", || {});
        });
        nucleo.notifica("a:b");
    }

    #[test]
    fn l_avvio_lo_fa_il_primo_e_gli_altri_aspettano() {
        let nucleo = Nucleo::nuovo();
        assert_eq!(nucleo.reclama_l_avvio(), ChiAvvia::Tu);
        assert_eq!(nucleo.reclama_l_avvio(), ChiAvvia::GiaInCorso);
        let fatto = Arc::new(AtomicUsize::new(0));
        let dentro = Arc::clone(&fatto);
        nucleo.dopo_l_avvio(move || {
            dentro.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(fatto.load(Ordering::SeqCst), 0, "aspetta la fine");
        nucleo.avvio_finito();
        assert_eq!(fatto.load(Ordering::SeqCst), 1, "parte alla fine");
        assert_eq!(nucleo.reclama_l_avvio(), ChiAvvia::GiaFinito);
        let dentro = Arc::clone(&fatto);
        nucleo.dopo_l_avvio(move || {
            dentro.fetch_add(10, Ordering::SeqCst);
        });
        assert_eq!(
            fatto.load(Ordering::SeqCst),
            11,
            "a avvio finito parte subito"
        );
    }

    #[test]
    fn un_lavoro_accodato_prima_che_l_avvio_cominci_parte_alla_sua_fine() {
        let nucleo = Nucleo::nuovo();
        let fatto = Arc::new(AtomicUsize::new(0));
        let dentro = Arc::clone(&fatto);
        nucleo.dopo_l_avvio(move || {
            dentro.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(fatto.load(Ordering::SeqCst), 0);
        assert_eq!(
            nucleo.reclama_l_avvio(),
            ChiAvvia::Tu,
            "accodarsi non è cominciare: l'avvio tocca ancora a chi lo reclama"
        );
        nucleo.avvio_finito();
        assert_eq!(fatto.load(Ordering::SeqCst), 1);
    }
}
