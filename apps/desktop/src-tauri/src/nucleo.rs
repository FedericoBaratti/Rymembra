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

/// Un lavoro che aspetta la fine dell'avvio del nucleo, e sa come è finito.
type Dopo = Box<dyn FnOnce(Esito) + Send>;

/// Come è finito un avvio del nucleo: lo dice chi lo ha fatto ([`Nucleo::avvio_chiuso`])
/// e lo sa chi lo aspetta ([`Nucleo::dopo_l_avvio`]).
///
/// # Perché non basta «finito»
///
/// Un avvio che non riesce non è un avvio finito: se lo fosse, chi arriva dopo —
/// l'attività che nasce dopo un servizio che non ha potuto aprire la libreria —
/// troverebbe un nucleo «c'è già», lo prenderebbe per buono e non lo riproverebbe
/// mai; e `Nucleo::manage` tiene il primo valore per tipo, quindi il guasto di
/// prima resterebbe lì per tutta la vita del processo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Esito {
    /// Il nucleo c'è, intero.
    Riuscito,
    /// Non è partito, e **non ha lasciato niente di vivo**: la libreria non si è
    /// aperta, e prima di quel punto l'avvio non ha fatto nascere un filo. Chi
    /// arriva dopo può rifarlo da zero ([`Nucleo::reclama_l_avvio`] dà di nuovo
    /// [`ChiAvvia::Tu`]).
    Fallito,
    /// È caduto a metà, dopo aver fatto nascere dei fili e registrato degli stati:
    /// il nucleo c'è in parte, e **non** si rifà (un secondo lettore, un secondo
    /// motore audio, un secondo filo di rete sullo stesso processo sono peggio del
    /// guasto). Chi arriva dopo non lo monta: lo dice, e il processo va chiuso.
    Rotto,
}

/// A che punto è l'avvio del nucleo: vedi [`Nucleo::reclama_l_avvio`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fase {
    /// Nessuno ha cominciato.
    DaFare,
    /// Qualcuno ha cominciato.
    InCorso,
    /// Finito, e il nucleo c'è ([`Esito::Riuscito`]).
    Finito,
    /// Non è partito e non ha lasciato niente ([`Esito::Fallito`]): si può rifare.
    Fallito,
    /// Caduto a metà ([`Esito::Rotto`]): non si rifà.
    Rotto,
}

impl Fase {
    /// L'esito di un avvio chiuso, se questa fase lo è.
    fn esito(self) -> Option<Esito> {
        match self {
            Self::DaFare | Self::InCorso => None,
            Self::Finito => Some(Esito::Riuscito),
            Self::Fallito => Some(Esito::Fallito),
            Self::Rotto => Some(Esito::Rotto),
        }
    }
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
    /// È caduto a metà ([`Esito::Rotto`]) e non si rifà.
    Rotto,
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

    /// Lo stato di tipo `T`, o `internal.aborted`: come `importa::gestito`, per
    /// chi lavora in disparte e può ritrovarsi vivo mentre gli stati vengono
    /// lasciati cadere (la finestra si sta chiudendo), e lì un panico non è la
    /// risposta.
    ///
    /// # Errori
    ///
    /// `internal.aborted` se lo stato non c'è.
    pub fn gestito<T: Clone + Send + Sync + 'static>(
        &self,
        cosa: &str,
    ) -> Result<T, rymembra_domain::errors::AppError> {
        self.try_state::<T>().ok_or_else(|| {
            rymembra_domain::errors::AppError::new(
                rymembra_domain::errors::ErrorCode::InternalAborted {
                    what: Some(cosa.to_owned()),
                },
            )
            .with_cause("lo stato non c'è nel nucleo")
        })
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
    #[cfg_attr(
        not(target_os = "android"),
        allow(
            dead_code,
            reason = "l'unico ascoltatore è la sessione multimediale di Android; le prove lo usano"
        )
    )]
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
    /// e alla fine dice [`Nucleo::avvio_chiuso`]; gli altri aspettano.
    ///
    /// È il modo in cui il servizio che parte all'accensione e l'attività che
    /// nasce dopo non avviano due volte la libreria e il lettore.
    ///
    /// Dopo un avvio [`Esito::Fallito`] l'avvio torna a chi lo chiede: nessuno ha
    /// niente di vivo da proteggere, e il guasto di prima — la libreria che non si
    /// apriva, magari per un disco pieno che adesso non lo è più — si riprova.
    pub fn reclama_l_avvio(&self) -> ChiAvvia {
        let mut avvio = self.0.avvio.lock().unwrap_or_else(PoisonError::into_inner);
        match avvio.fase {
            Fase::DaFare | Fase::Fallito => {
                avvio.fase = Fase::InCorso;
                ChiAvvia::Tu
            }
            Fase::InCorso => ChiAvvia::GiaInCorso,
            Fase::Finito => ChiAvvia::GiaFinito,
            Fase::Rotto => ChiAvvia::Rotto,
        }
    }

    /// L'avvio è finito, e il nucleo c'è: parte il lavoro di chi aspettava.
    pub fn avvio_finito(&self) {
        self.avvio_chiuso(Esito::Riuscito);
    }

    /// L'avvio è chiuso, comunque sia andata: la fase dice come, e parte il lavoro
    /// di chi aspettava, che lo sa ([`Nucleo::dopo_l_avvio`]).
    pub fn avvio_chiuso(&self, esito: Esito) {
        let da_fare = {
            let mut avvio = self.0.avvio.lock().unwrap_or_else(PoisonError::into_inner);
            avvio.fase = match esito {
                Esito::Riuscito => Fase::Finito,
                Esito::Fallito => Fase::Fallito,
                Esito::Rotto => Fase::Rotto,
            };
            std::mem::take(&mut avvio.dopo)
        };
        for lavoro in da_fare {
            lavoro(esito);
        }
    }

    /// L'avvio è già finito **e il nucleo c'è**: chi arriva dopo — la finestra che
    /// nasce dopo un servizio — non lo avvia, si aggancia.
    #[must_use]
    pub fn e_gia_avviato(&self) -> bool {
        self.0
            .avvio
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .fase
            == Fase::Finito
    }

    /// Fa `lavoro` quando l'avvio è chiuso, dicendogli come è andata: subito, se lo
    /// è già, sul filo che chiama; altrimenti sul filo che lo chiude.
    ///
    /// Se nessuno ha ancora cominciato l'avvio, il lavoro si accoda lo stesso e
    /// parte quando qualcuno lo avrà fatto: accodarsi non è cominciare.
    pub fn dopo_l_avvio(&self, lavoro: impl FnOnce(Esito) + Send + 'static) {
        let mut avvio = self.0.avvio.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(esito) = avvio.fase.esito() {
            drop(avvio);
            lavoro(esito);
        } else {
            avvio.dopo.push(Box::new(lavoro));
        }
    }
}

/// Chiude l'avvio del nucleo che parte da un servizio **comunque finisca** la
/// funzione che lo fa, anche con un panico: la creano subito dopo aver reclamato
/// l'avvio, e il `Drop` dice a [`Nucleo::avvio_chiuso`] come è andata.
///
/// Senza, un panico a metà lascerebbe l'avvio «in corso» per sempre, e la
/// finestra che nasce dopo e lo aspetta resterebbe su «Apro la libreria…».
///
/// # Come decide l'esito
///
/// - [`GuardiaDellAvvio::riuscito`] chiamata, e nessun panico: [`Esito::Riuscito`].
/// - Altrimenti, se [`GuardiaDellAvvio::la_libreria_c_e`] era già stata chiamata —
///   da lì in poi l'avvio fa nascere fili e registra stati —: [`Esito::Rotto`].
/// - Altrimenti: [`Esito::Fallito`]. La libreria non si è aperta, e fin lì non è
///   nato niente che un secondo tentativo possa raddoppiare.
///
/// Se l'esito non è `Riuscito` lo dice anche a chi ha messo `avvisa` (su Android,
/// Kotlin, che altrimenti lascerebbe la notifica su «si sta avviando» per sempre) —
/// **dopo** aver chiuso l'avvio: chi viene avvisato può rientrare subito, e deve
/// trovare l'esito già scritto.
#[cfg_attr(
    not(target_os = "android"),
    allow(
        dead_code,
        reason = "la usa solo il servizio di Android, che avvia il nucleo senza finestra; le prove la provano"
    )
)]
pub struct GuardiaDellAvvio {
    nucleo: Nucleo,
    avvisa: Box<dyn Fn(Esito) + Send>,
    libreria: bool,
    riuscito: bool,
}

#[cfg_attr(
    not(target_os = "android"),
    allow(
        dead_code,
        reason = "la usa solo il servizio di Android, che avvia il nucleo senza finestra; le prove la provano"
    )
)]
impl GuardiaDellAvvio {
    /// La guardia di un avvio appena reclamato.
    pub fn nuova(nucleo: Nucleo, avvisa: impl Fn(Esito) + Send + 'static) -> Self {
        Self {
            nucleo,
            avvisa: Box::new(avvisa),
            libreria: false,
            riuscito: false,
        }
    }

    /// La libreria è aperta e il nucleo sta per cominciare a farsi: da qui un
    /// avvio che cade lascia un nucleo a metà, e non si rifà.
    pub fn la_libreria_c_e(&mut self) {
        self.libreria = true;
    }

    /// L'avvio è arrivato in fondo.
    pub fn riuscito(&mut self) {
        self.riuscito = true;
    }
}

impl Drop for GuardiaDellAvvio {
    fn drop(&mut self) {
        let esito = if self.riuscito && !std::thread::panicking() {
            Esito::Riuscito
        } else if self.libreria {
            Esito::Rotto
        } else {
            Esito::Fallito
        };
        // **Prima** si chiude l'avvio, **poi** si avvisa Kotlin: appena sa che il nucleo
        // non è partito, Kotlin può rimettere in coda una sveglia che rientra in Rust, e
        // questa deve trovare l'avvio chiuso (`Tu` dopo un `Fallito`, `Rotto` dopo un
        // `Rotto`) e non `GiaInCorso` — che lo lascerebbe «già avviato» per sempre.
        self.nucleo.avvio_chiuso(esito);
        if esito != Esito::Riuscito {
            (self.avvisa)(esito);
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

/// Chi può dare il nucleo del processo: il nucleo stesso, o l'`AppHandle` della
/// finestra, che nel processo c'è sempre quando c'è un `AppHandle`.
///
/// È il ponte fra i due mondi, per le funzioni che chiamano **da tutti e due**:
/// quelle del nucleo, che non hanno una finestra, e quelle della finestra — i
/// comandi, il vassoio, la chiusura — che hanno un `AppHandle`. Prendono
/// `&impl ComeNucleo` e il chiamante passa quel che ha, senza convertire a mano
/// in cento posti.
pub trait ComeNucleo {
    /// Il nucleo di questo processo.
    fn come_nucleo(&self) -> Nucleo;
}

impl ComeNucleo for Nucleo {
    fn come_nucleo(&self) -> Nucleo {
        self.clone()
    }
}

impl ComeNucleo for AppHandle {
    fn come_nucleo(&self) -> Nucleo {
        Nucleo::di(self)
    }
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

    /// Il ponte fra i due mondi dà sempre lo stesso nucleo: quello di cui si parte.
    #[test]
    fn come_nucleo_dal_nucleo_e_il_nucleo_stesso() {
        let nucleo = Nucleo::nuovo();
        assert!(Arc::ptr_eq(&nucleo.come_nucleo().0, &nucleo.0));
    }

    /// Chi lavora in disparte e trova lo stato caduto riceve `internal.aborted`, non
    /// un panico: la finestra si sta chiudendo.
    #[test]
    fn uno_stato_che_manca_e_un_errore_e_non_un_panico() {
        let nucleo = Nucleo::nuovo();
        let errore = nucleo
            .gestito::<Contatore>("contatore")
            .err()
            .expect("lo stato non c'è");
        assert_eq!(errore.code().kind().code(), "internal.aborted");
        nucleo.manage(Contatore(Arc::new(AtomicUsize::new(1))));
        assert!(nucleo.gestito::<Contatore>("contatore").is_ok());
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
        nucleo.dopo_l_avvio(move |esito| {
            assert_eq!(esito, Esito::Riuscito);
            dentro.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(fatto.load(Ordering::SeqCst), 0, "aspetta la fine");
        nucleo.avvio_finito();
        assert_eq!(fatto.load(Ordering::SeqCst), 1, "parte alla fine");
        assert_eq!(nucleo.reclama_l_avvio(), ChiAvvia::GiaFinito);
        let dentro = Arc::clone(&fatto);
        nucleo.dopo_l_avvio(move |_| {
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
        nucleo.dopo_l_avvio(move |_| {
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

    /// R1, il caso A: la libreria non si apre al boot. L'avvio che non è riuscito
    /// **non** è finito: `e_gia_avviato` dice no, e l'attività che nasce dopo — o
    /// un secondo tentativo del servizio — riceve di nuovo l'avvio, invece di
    /// agganciarsi a un nucleo che non c'è e di mostrare per sempre lo stesso guasto.
    #[test]
    fn un_avvio_fallito_non_resta_finito_e_chi_arriva_dopo_lo_rifa() {
        let nucleo = Nucleo::nuovo();
        assert_eq!(nucleo.reclama_l_avvio(), ChiAvvia::Tu);
        nucleo.avvio_chiuso(Esito::Fallito);
        assert!(
            !nucleo.e_gia_avviato(),
            "un avvio fallito si dà per avviato: l'attività si aggancia a un nucleo che non c'è"
        );
        assert_eq!(
            nucleo.reclama_l_avvio(),
            ChiAvvia::Tu,
            "dopo un avvio fallito l'avvio non torna a nessuno"
        );
        assert_eq!(
            nucleo.reclama_l_avvio(),
            ChiAvvia::GiaInCorso,
            "e il secondo tentativo ha il suo reclamo, una volta sola"
        );
        nucleo.avvio_finito();
        assert!(nucleo.e_gia_avviato());
        assert_eq!(nucleo.reclama_l_avvio(), ChiAvvia::GiaFinito);
    }

    /// R1, il caso B: un nucleo caduto a metà non si rifà (nascerebbero due lettori
    /// e due motori audio) e non si dà per buono: chi arriva dopo lo sa.
    #[test]
    fn un_avvio_caduto_a_meta_non_si_rifa_e_non_si_da_per_buono() {
        let nucleo = Nucleo::nuovo();
        assert_eq!(nucleo.reclama_l_avvio(), ChiAvvia::Tu);
        nucleo.avvio_chiuso(Esito::Rotto);
        assert!(
            !nucleo.e_gia_avviato(),
            "un nucleo a metà si dà per avviato"
        );
        assert_eq!(
            nucleo.reclama_l_avvio(),
            ChiAvvia::Rotto,
            "un nucleo a metà si rifà da capo, con fili doppi"
        );
        assert_eq!(nucleo.reclama_l_avvio(), ChiAvvia::Rotto, "e resta così");
    }

    /// Chi aspetta la fine dell'avvio ne riceve l'esito, che sia già finito o no: è
    /// su quello che l'attività sceglie se agganciarsi, rifare l'avvio o dire che
    /// il nucleo non è partito.
    #[test]
    fn chi_aspetta_la_fine_dell_avvio_ne_riceve_l_esito() {
        for (chiuso, atteso) in [
            (Esito::Riuscito, Esito::Riuscito),
            (Esito::Fallito, Esito::Fallito),
            (Esito::Rotto, Esito::Rotto),
        ] {
            // Accodato prima.
            let nucleo = Nucleo::nuovo();
            nucleo.reclama_l_avvio();
            let ricevuto = Arc::new(Mutex::new(None));
            let dentro = Arc::clone(&ricevuto);
            nucleo.dopo_l_avvio(move |esito| *dentro.lock().unwrap() = Some(esito));
            assert_eq!(*ricevuto.lock().unwrap(), None, "parte prima della fine");
            nucleo.avvio_chiuso(chiuso);
            assert_eq!(
                *ricevuto.lock().unwrap(),
                Some(atteso),
                "accodato: {chiuso:?}"
            );
            // Arrivato dopo.
            let dentro = Arc::new(Mutex::new(None));
            let la_stessa = Arc::clone(&dentro);
            nucleo.dopo_l_avvio(move |esito| *la_stessa.lock().unwrap() = Some(esito));
            assert_eq!(*dentro.lock().unwrap(), Some(atteso), "dopo: {chiuso:?}");
        }
    }

    /// Una guardia che registra a Kotlin (o a chi per lui) come è finito l'avvio.
    fn guardia_con_registro(nucleo: &Nucleo) -> (GuardiaDellAvvio, Arc<Mutex<Vec<Esito>>>) {
        let dette = Arc::new(Mutex::new(Vec::new()));
        let dentro = Arc::clone(&dette);
        let guardia = GuardiaDellAvvio::nuova(nucleo.clone(), move |esito| {
            dentro.lock().unwrap().push(esito);
        });
        (guardia, dette)
    }

    /// La guardia chiude l'avvio comunque finisca la funzione, e dice l'esito vero.
    #[test]
    fn la_guardia_chiude_l_avvio_e_dice_come_e_andata() {
        // Arrivato in fondo: riuscito, e a Kotlin non si dice niente.
        let nucleo = Nucleo::nuovo();
        nucleo.reclama_l_avvio();
        {
            let (mut guardia, dette) = guardia_con_registro(&nucleo);
            guardia.la_libreria_c_e();
            guardia.riuscito();
            drop(guardia);
            assert!(dette.lock().unwrap().is_empty());
        }
        assert!(nucleo.e_gia_avviato());

        // La libreria non si è aperta: fallito, e Kotlin lo sa. Si può rifare.
        let nucleo = Nucleo::nuovo();
        nucleo.reclama_l_avvio();
        let (guardia, dette) = guardia_con_registro(&nucleo);
        drop(guardia);
        assert_eq!(*dette.lock().unwrap(), vec![Esito::Fallito]);
        assert_eq!(nucleo.reclama_l_avvio(), ChiAvvia::Tu);

        // Finito senza dire «riuscito» dopo che la libreria c'era: a metà.
        let nucleo = Nucleo::nuovo();
        nucleo.reclama_l_avvio();
        let (mut guardia, dette) = guardia_con_registro(&nucleo);
        guardia.la_libreria_c_e();
        drop(guardia);
        assert_eq!(*dette.lock().unwrap(), vec![Esito::Rotto]);
        assert_eq!(nucleo.reclama_l_avvio(), ChiAvvia::Rotto);
    }

    /// FC5, C2: quando Kotlin viene avvisato l'avvio è **già chiuso**. Kotlin, appena
    /// sa che il nucleo non è partito, può rimettere in coda una sveglia che rientra in
    /// Rust (`reclama_l_avvio`): se l'avvio fosse ancora «in corso» riceverebbe
    /// `GiaInCorso`, Kotlin si segnerebbe «già avviato» e non riproverebbe più.
    #[test]
    fn la_guardia_avvisa_kotlin_a_avvio_gia_chiuso() {
        for (con_la_libreria, atteso) in [(false, ChiAvvia::Tu), (true, ChiAvvia::Rotto)] {
            let nucleo = Nucleo::nuovo();
            nucleo.reclama_l_avvio();
            let visto = Arc::new(Mutex::new(Vec::new()));
            let dentro = Arc::clone(&visto);
            let dal_nucleo = nucleo.clone();
            let mut guardia = GuardiaDellAvvio::nuova(nucleo.clone(), move |_| {
                // Quel che vedrebbe una sveglia che rientra in Rust proprio adesso.
                dentro.lock().unwrap().push(dal_nucleo.reclama_l_avvio());
            });
            if con_la_libreria {
                guardia.la_libreria_c_e();
            }
            drop(guardia);
            assert_eq!(
                *visto.lock().unwrap(),
                vec![atteso],
                "avvisato a avvio ancora in corso (libreria aperta: {con_la_libreria})"
            );
        }
    }

    /// Un panico dentro l'avvio — dopo che la libreria c'era, cioè con dei fili già
    /// nati — non porta il nucleo a «finito»: è `Rotto`, e la pagina non monta.
    #[test]
    fn un_panico_a_meta_dell_avvio_lascia_un_nucleo_rotto_e_non_finito() {
        let nucleo = Nucleo::nuovo();
        nucleo.reclama_l_avvio();
        let (guardia, dette) = guardia_con_registro(&nucleo);
        let esito = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let mut guardia = guardia;
            guardia.la_libreria_c_e();
            // Anche una guardia che ha già detto «riuscito» non salva un panico.
            guardia.riuscito();
            panic!("un panico dentro avvia_il_nucleo");
        }));
        assert!(esito.is_err());
        assert!(
            !nucleo.e_gia_avviato(),
            "dopo un panico a metà il nucleo risulta avviato: la pagina monta su un nucleo a metà"
        );
        assert_eq!(*dette.lock().unwrap(), vec![Esito::Rotto]);
        assert_eq!(nucleo.reclama_l_avvio(), ChiAvvia::Rotto);
    }

    /// Un panico prima che la libreria ci sia non lascia niente di vivo: fallito, si
    /// può rifare.
    #[test]
    fn un_panico_prima_della_libreria_lascia_un_avvio_da_rifare() {
        let nucleo = Nucleo::nuovo();
        nucleo.reclama_l_avvio();
        let (guardia, dette) = guardia_con_registro(&nucleo);
        let esito = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _guardia = guardia;
            panic!("un panico prima della libreria");
        }));
        assert!(esito.is_err());
        assert_eq!(*dette.lock().unwrap(), vec![Esito::Fallito]);
        assert_eq!(nucleo.reclama_l_avvio(), ChiAvvia::Tu);
    }
}
