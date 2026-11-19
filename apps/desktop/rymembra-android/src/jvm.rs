//! La macchina virtuale Java, e la classe `Ponte` dentro di lei.
//!
//! L'unico posto `unsafe` del crate. Le operazioni pericolose sono le due
//! conversioni da puntatore grezzo a oggetto di `jni`, fatte una volta per
//! processo: tutto il resto — trovare la classe, chiamare il metodo, leggere la
//! stringa — passa dall'API sicura di `jni`. E, per il nucleo che parte da un
//! servizio prima di ogni attività (F8), le funzioni che Kotlin chiama per
//! entrare in Rust — `#[no_mangle]`, perché il nome lo cerca la macchina
//! virtuale — e le due chiamate a `ndk_context::{initialize,release}_android_context`.
//!
//! # Da dove vengono i puntatori
//!
//! Da `tao`, che all'apertura dell'attività li consegna a `ndk-context`
//! (`platform_impl/android/ndk_glue.rs`): il `JavaVM` del processo e un
//! riferimento **globale** al contesto dell'applicazione, che `tao` tiene vivo
//! in una statica fino all'uscita. È lo stesso contesto che `cpal` usa per
//! l'uscita audio.
//!
//! Se a partire per primo è un servizio, e non l'attività, `tao` non ha ancora
//! consegnato niente: il ponte lo costruisce allora la funzione che Kotlin
//! chiama ([`avvia_il_nucleo`]), con la macchina virtuale presa dal suo `JNIEnv`
//! e la classe `Ponte` dal suo `JClass` — senza `ndk-context`. `ndk-context`
//! serve ad altri (`cpal` per l'audio, il DNS di `iroh`, `netdev`), e lo mette
//! lo stesso il servizio, alle condizioni di sotto.
//!
//! # Il contesto di `ndk-context` fra il servizio e `tao`
//!
//! Provato sull'emulatore (`diario-f8.md`, F8-0), e vale in questo ordine:
//!
//! 1. `initialize_android_context` va chiamata **una volta sola per processo**:
//!    la seconda sovrascrive e **poi** va in panico (`assert!(previous.is_none())`,
//!    `ndk-context-0.1.1/src/lib.rs:82-88`), e il panico esce da una `extern "C"`
//!    di `tao` senza `catch_unwind`: il processo si abortisce. `catch_unwind` non
//!    è una rete di sicurezza. A decidere se il servizio la può chiamare è
//!    **Kotlin**, che sa se un'attività è mai nata nel processo, con un flag letto
//!    e scritto solo sul filo principale (`Ponte.attivitaGiaNata`).
//! 2. Il riferimento globale al contesto sta in una statica **prima** della
//!    chiamata ([`CONTESTO_GLOBALE`]) e non si libera mai: `ndk-context` ne tiene
//!    il puntatore.
//! 3. Il flag «l'ha messo il servizio» ([`CONTESTO_DEL_SERVIZIO`]) si alza **dopo**
//!    che la chiamata è tornata: solo allora il rilascio può toccare il contesto.
//! 4. L'attività lo rilascia ([`Java_io_github_federicobaratti_rymembra_Ponte_rilasciaIlContestoDelServizio`])
//!    **prima** di `super.onCreate` della prima attività, perché `tao` asserisce
//!    che sia vuoto, e da lì è suo per sempre.
//!
//! # Il cancello
//!
//! Fra il rilascio e il momento in cui `tao` mette il suo contesto
//! (`Rust.onCreate`, dentro `super.onCreate`) `ndk-context` è vuoto: al più
//! 0,55 s, misurato, una volta per processo. In quel tratto `cpal` va in panico
//! e `netdev` si ricorda «non c'è» per sempre. Il cancello ([`aspetta_il_contesto`])
//! è chiuso dal rilascio e riaperto da Kotlin dopo `super.onCreate`: chi apre uno
//! stream audio lo aspetta, al più un paio di secondi.
//!
//! # La porta d'ingresso, e perché è una macro
//!
//! La funzione JNI che avvia il nucleo deve chiamare il nucleo, che sta in
//! `rymembra-desktop`, il quale dipende da questo crate: l'esportazione non può
//! stare qui senza un ciclo. La scrive allora la macro [`esporta_il_nucleo!`],
//! che si invoca una volta in `rymembra-desktop` — come fa Tauri con
//! `android_binding!` — e il cui `unsafe` (l'attributo `#[unsafe(no_mangle)]`)
//! sta **in questo file**.

#![allow(
    unsafe_code,
    reason = "vedi il preambolo: i due puntatori di `ndk-context`, e le funzioni esportate a Kotlin"
)]

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, OnceLock, PoisonError};
use std::time::Duration;

pub use jni::JNIEnv;
use jni::JavaVM;
pub use jni::objects::{GlobalRef, JClass, JObject, JString, JValue};
pub use jni::sys::jboolean;

use crate::Errore;

/// Il nome della classe Kotlin, con i punti.
const CLASSE: &str = "io.github.federicobaratti.rymembra.Ponte";

/// La firma JNI di `Ponte.chiama(String, String): String`.
const FIRMA: &str = "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;";

/// Quanti riferimenti locali può creare una chiamata: tre stringhe e il
/// risultato. Il cornice si apre e si chiude a ogni chiamata, perché un filo
/// che resta agganciato — quello principale, quello dei gesti — non perda un
/// riferimento a ogni giro.
const RIFERIMENTI: i32 = 8;

/// La macchina virtuale e la classe, trovate una volta.
struct Ponte {
    vm: JavaVM,
    classe: GlobalRef,
}

static PONTE: OnceLock<Ponte> = OnceLock::new();

/// Il ponte, costruendolo la prima volta.
///
/// Due fili che arrivano insieme la prima volta lo costruiscono tutti e due, e
/// ne resta uno: costa una ricerca di classe in più, una volta nella vita del
/// processo, e risparmia un lucchetto a tutte le chiamate dopo.
fn ponte() -> Result<&'static Ponte, Errore> {
    if let Some(ponte) = PONTE.get() {
        return Ok(ponte);
    }
    let nuovo = costruisci()?;
    Ok(PONTE.get_or_init(|| nuovo))
}

/// Trova la macchina virtuale, il contesto e la classe.
fn costruisci() -> Result<Ponte, Errore> {
    // `android_context` va in panico se `tao` non l'ha ancora inizializzato:
    // succede solo prima che l'attività esista, e lì la risposta giusta è un
    // errore, non un processo che cade.
    let contesto = std::panic::catch_unwind(ndk_context::android_context)
        .map_err(|_| Errore::nuovo("il contesto Android non è ancora pronto"))?;

    // SAFETY: `contesto.vm()` è il puntatore che `tao` ha preso da
    // `JNIEnv::get_java_vm().get_java_vm_pointer()`: il `JavaVM` del processo,
    // che vive quanto il processo. `from_raw` controlla solo che non sia nullo.
    let vm = unsafe { JavaVM::from_raw(contesto.vm().cast()) }
        .map_err(|err| Errore::nuovo(format!("la macchina virtuale: {err}")))?;

    let classe = {
        let mut env = vm
            .attach_current_thread()
            .map_err(|err| Errore::nuovo(format!("agganciarsi alla macchina virtuale: {err}")))?;
        // SAFETY: `contesto.context()` è un riferimento **globale** al contesto
        // dell'applicazione, creato da `tao` con `new_global_ref` e tenuto in
        // una statica fino all'uscita: valido su qualunque filo, per tutta la
        // vita del processo. `JObject` non lo libera quando cade (non ha un
        // `Drop`), quindi qui lo si prende soltanto in prestito.
        let applicazione = unsafe { JObject::from_raw(contesto.context().cast()) };
        let trovata = env.with_local_frame(RIFERIMENTI, |env| {
            let caricatore = env
                .call_method(
                    &applicazione,
                    "getClassLoader",
                    "()Ljava/lang/ClassLoader;",
                    &[],
                )?
                .l()?;
            let nome = env.new_string(CLASSE)?;
            let classe = env
                .call_method(
                    &caricatore,
                    "loadClass",
                    "(Ljava/lang/String;)Ljava/lang/Class;",
                    &[JValue::Object(&nome)],
                )?
                .l()?;
            env.new_global_ref(classe)
        });
        pulisci(&mut env);
        trovata.map_err(|err| Errore::nuovo(format!("la classe {CLASSE}: {err}")))?
    };
    Ok(Ponte { vm, classe })
}

/// Toglie un'eccezione Java rimasta in sospeso, scrivendola nel registro.
///
/// Una chiamata JNI con un'eccezione in sospeso è vietata: la successiva, su
/// questo filo, farebbe cadere il processo.
fn pulisci(env: &mut jni::JNIEnv<'_>) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
}

/// `Ponte.chiama(comando, argomenti)`, sul filo che chiama.
///
/// # Errori
///
/// Se il ponte non si costruisce, o se la chiamata solleva un'eccezione — cosa
/// che `Ponte.chiama` evita prendendole tutte, ma un `OutOfMemoryError` o una
/// classe mancante non chiedono il permesso.
pub(crate) fn chiama(comando: &str, argomenti: &str) -> Result<String, Errore> {
    let ponte = ponte()?;
    let mut env = ponte
        .vm
        .attach_current_thread()
        .map_err(|err| Errore::nuovo(format!("agganciarsi alla macchina virtuale: {err}")))?;
    let esito = env.with_local_frame(RIFERIMENTI, |env| -> jni::errors::Result<String> {
        let comando = env.new_string(comando)?;
        let argomenti = env.new_string(argomenti)?;
        let classe: &JClass<'_> = ponte.classe.as_obj().into();
        let risposta = env
            .call_static_method(
                classe,
                "chiama",
                FIRMA,
                &[JValue::Object(&comando), JValue::Object(&argomenti)],
            )?
            .l()?;
        let risposta = JString::from(risposta);
        let testo: String = env.get_string(&risposta)?.into();
        Ok(testo)
    });
    pulisci(&mut env);
    esito.map_err(|err| Errore::nuovo(format!("{comando}: {err}")))
}

// ── Il nucleo che parte da un servizio, senza attività (F8) ──────────────────

/// Il contesto di `ndk-context` l'ha messo il servizio, e nessuno lo ha ancora
/// rilasciato. Si alza **dopo** che `initialize_android_context` è tornata, e
/// solo allora il rilascio può toccare il contesto.
static CONTESTO_DEL_SERVIZIO: AtomicBool = AtomicBool::new(false);

/// Il riferimento globale al contesto dell'applicazione che il servizio ha
/// consegnato. Sta qui **prima** della chiamata che lo passa a `ndk-context`, e
/// non si libera mai: `ndk-context` ne tiene il puntatore, e se la chiamata
/// panica quel puntatore deve restare valido.
static CONTESTO_GLOBALE: OnceLock<GlobalRef> = OnceLock::new();

/// Il cancello del contesto: `true` finché `ndk-context` si può leggere.
///
/// Lo chiude [`rilascia_il_contesto_del_servizio`], lo riapre
/// [`contesto_di_tao_pronto`]. Vedi il preambolo del modulo.
static CANCELLO: Mutex<bool> = Mutex::new(true);

/// Chi aspetta il cancello.
static CANCELLO_APERTO: Condvar = Condvar::new();

/// Aspetta, al più `attesa`, che `ndk-context` sia leggibile.
///
/// Chi sta per aprire uno stream audio (`cpal` legge il contesto, e senza
/// contesto va in panico) la chiama prima: nella finestra fra il rilascio del
/// contesto del servizio e quello di `tao` — una volta per processo, al più
/// 0,55 s — aspetta invece di cadere. Di norma il cancello è aperto e non costa
/// che un lucchetto.
///
/// Torna `true` se il cancello è aperto, `false` se è scaduta l'attesa: allora
/// chi chiama prosegue lo stesso, dentro un `catch_unwind`.
pub fn aspetta_il_contesto(attesa: Duration) -> bool {
    let guardia = CANCELLO.lock().unwrap_or_else(PoisonError::into_inner);
    let (guardia, _) = CANCELLO_APERTO
        .wait_timeout_while(guardia, attesa, |aperto| !*aperto)
        .unwrap_or_else(PoisonError::into_inner);
    *guardia
}

fn muovi_il_cancello(aperto: bool) {
    *CANCELLO.lock().unwrap_or_else(PoisonError::into_inner) = aperto;
    CANCELLO_APERTO.notify_all();
}

/// Scrive una riga nel registro di Android (`Log.i`), da un filo già agganciato.
///
/// Prima che il diario di Rymembra sia aperto — il nucleo parte da qui — non c'è
/// altro posto dove scrivere: `stdout` e `stderr` li porta nel registro solo
/// `onFirstActivityCreate`, cioè l'attività.
fn scrivi(env: &mut JNIEnv<'_>, testo: &str) {
    let _ = env.with_local_frame(RIFERIMENTI, |env| -> jni::errors::Result<()> {
        let etichetta = env.new_string("RymembraNucleo")?;
        let riga = env.new_string(testo)?;
        env.call_static_method(
            "android/util/Log",
            "i",
            "(Ljava/lang/String;Ljava/lang/String;)I",
            &[JValue::Object(&etichetta), JValue::Object(&riga)],
        )?;
        Ok(())
    });
    pulisci(env);
}

/// Come [`scrivi`], da un filo qualunque: lo aggancia alla macchina virtuale.
fn scrivi_dal_filo(testo: &str) {
    if let Ok(ponte) = ponte()
        && let Ok(mut env) = ponte.vm.attach_current_thread()
    {
        scrivi(&mut env, testo);
    }
}

/// `Ponte.avviaNucleo(contesto, dati, inizializzaContesto)`: il servizio entra in
/// Rust, prima di ogni attività. La chiama la macro [`esporta_il_nucleo!`].
///
/// Prende la macchina virtuale dall'`env` e la classe `Ponte` dal `JClass`, che
/// per una funzione statica è la sua classe: niente `ndk-context` per il ponte.
/// Se Kotlin lo chiede (`inizializza`: nessuna attività è mai nata), mette
/// anche `ndk-context`, alle condizioni del preambolo. Poi fa partire `avvia`, con
/// la cartella dei dati, su un filo suo: questa funzione torna subito, perché il
/// filo che chiama è quello principale del servizio.
///
/// Un guasto lo dice a Kotlin con un'eccezione, che la notifica del servizio
/// racconta. Mai un panico fuori: uscirebbe da una `extern "system"`.
pub fn avvia_il_nucleo(
    env: &mut JNIEnv<'_>,
    classe: &JClass<'_>,
    contesto: &JObject<'_>,
    dati: &JString<'_>,
    inizializza: bool,
    avvia: fn(PathBuf),
) {
    let esito = catch_unwind(AssertUnwindSafe(|| {
        prepara_il_nucleo(env, classe, contesto, dati, inizializza, avvia)
    }));
    let guasto = match esito {
        Ok(Ok(())) => return,
        Ok(Err(err)) => err.to_string(),
        Err(_) => "panico".to_owned(),
    };
    scrivi(env, &format!("il nucleo non parte: {guasto}"));
    let _ = env.throw_new("java/lang/IllegalStateException", guasto);
}

fn prepara_il_nucleo(
    env: &mut JNIEnv<'_>,
    classe: &JClass<'_>,
    contesto: &JObject<'_>,
    dati: &JString<'_>,
    inizializza: bool,
    avvia: fn(PathBuf),
) -> Result<(), Errore> {
    let guasto = |err: jni::errors::Error| Errore::nuovo(err.to_string());
    let vm = env.get_java_vm().map_err(guasto)?;
    let cartella: String = env.get_string(dati).map_err(guasto)?.into();
    if inizializza && !CONTESTO_DEL_SERVIZIO.load(Ordering::SeqCst) {
        // Prima il riferimento globale in una statica, che non lo libera mai: se
        // la funzione sotto panica, il puntatore che ha già messo resta valido.
        let applicazione = match CONTESTO_GLOBALE.get() {
            Some(gia) => gia,
            None => {
                let nuovo = env.new_global_ref(contesto).map_err(guasto)?;
                CONTESTO_GLOBALE.get_or_init(|| nuovo)
            }
        };
        // SAFETY: il `JavaVM` è quello del processo, che vive quanto lui; il
        // contesto è un riferimento **globale**, valido su ogni filo, e
        // `CONTESTO_GLOBALE` non lo libera mai. È la stessa coppia di puntatori
        // che `tao` consegna all'apertura dell'attività; la funzione è `unsafe`
        // perché va chiamata una volta sola, e solo se `tao` non l'ha già fatto:
        // lo garantisce il chiamante Kotlin (`Ponte.attivitaGiaNata`, sul filo
        // principale), non questo file, e `CONTESTO_DEL_SERVIZIO` impedisce che il
        // servizio la chiami due volte.
        unsafe {
            ndk_context::initialize_android_context(
                vm.get_java_vm_pointer().cast(),
                applicazione.as_obj().as_raw().cast(),
            );
        }
        // Dopo, e solo se non ha panicato: il rilascio non deve toccare il
        // contesto di `tao`.
        CONTESTO_DEL_SERVIZIO.store(true, Ordering::SeqCst);
        scrivi(env, "ndk-context inizializzato dal servizio");
    }
    let classe_globale = env.new_global_ref(classe).map_err(guasto)?;
    let nuovo = Ponte {
        vm,
        classe: classe_globale,
    };
    if PONTE.set(nuovo).is_err() {
        scrivi(env, "il ponte c'era già");
    }
    std::thread::Builder::new()
        .name("rymembra-nucleo".to_owned())
        .spawn(move || {
            scrivi_dal_filo("il nucleo parte dal servizio");
            let esito = catch_unwind(AssertUnwindSafe(|| avvia(PathBuf::from(cartella))));
            if esito.is_err() {
                scrivi_dal_filo("il nucleo è caduto: vedi il diario");
            }
        })
        .map(|_| ())
        .map_err(|err| Errore::nuovo(format!("il filo del nucleo non parte: {err}")))
}

/// `Ponte.rilasciaIlContestoDelServizio()`: l'attività sta per nascere, e `tao`
/// asserirà che `ndk-context` sia vuoto. Rilascia quello del servizio, se c'è, e
/// chiude il cancello fino a [`contesto_di_tao_pronto`].
#[allow(non_snake_case, reason = "il nome lo fissa la convenzione di JNI")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_federicobaratti_rymembra_Ponte_rilasciaIlContestoDelServizio<
    'local,
>(
    mut env: JNIEnv<'local>,
    _classe: JClass<'local>,
) {
    let esito = catch_unwind(rilascia_il_contesto_del_servizio);
    match esito {
        Ok(true) => scrivi(&mut env, "ndk-context rilasciato, tocca a tao"),
        Ok(false) => {}
        Err(_) => scrivi(&mut env, "il rilascio di ndk-context è caduto"),
    }
}

/// Il rilascio vero. `true` se c'era un contesto del servizio e lo ha tolto.
fn rilascia_il_contesto_del_servizio() -> bool {
    if !CONTESTO_DEL_SERVIZIO.swap(false, Ordering::SeqCst) {
        return false;
    }
    // Il cancello **prima**: chi legge `ndk-context` da ora in poi aspetta.
    muovi_il_cancello(false);
    // SAFETY: lo ha inizializzato `prepara_il_nucleo`, che ha messo
    // `CONTESTO_DEL_SERVIZIO` a vero soltanto dopo che la funzione gemella è
    // tornata senza panico: il contesto c'è ed è quello del servizio, e `tao`,
    // che nasce dopo questa chiamata, non lo ha ancora toccato (Kotlin la chiama
    // prima di `super.onCreate` della prima attività).
    unsafe { ndk_context::release_android_context() };
    true
}

/// `Ponte.contestoDiTaoPronto()`: `super.onCreate` della prima attività è finito,
/// e `tao` ha messo il suo contesto. Il cancello si riapre.
#[allow(non_snake_case, reason = "il nome lo fissa la convenzione di JNI")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_federicobaratti_rymembra_Ponte_contestoDiTaoPronto<'local>(
    _env: JNIEnv<'local>,
    _classe: JClass<'local>,
) {
    muovi_il_cancello(true);
}

/// Esporta a Kotlin la funzione che avvia il nucleo da un servizio:
/// `Ponte.avviaNucleo(contesto: Context, dati: String, inizializzaContesto: Boolean)`.
///
/// Si invoca **una volta**, nel crate dell'applicazione (`rymembra-desktop`), con
/// la funzione che avvia il nucleo e prende la cartella dei dati dell'app. Il
/// perché di una macro e non di una funzione: vedi il preambolo del modulo. Il
/// suo `unsafe` è l'attributo `#[unsafe(no_mangle)]`, e sta qui, in questo file;
/// tutto il lavoro sta in [`avvia_il_nucleo`].
#[macro_export]
macro_rules! esporta_il_nucleo {
    ($avvia:path) => {
        #[doc = "`Ponte.avviaNucleo`: la porta da cui Kotlin avvia il nucleo (`esporta_il_nucleo!`)."]
        #[allow(non_snake_case, reason = "il nome lo fissa la convenzione di JNI")]
        #[unsafe(no_mangle)]
        pub extern "system" fn Java_io_github_federicobaratti_rymembra_Ponte_avviaNucleo<'local>(
            mut env: $crate::jvm::JNIEnv<'local>,
            classe: $crate::jvm::JClass<'local>,
            contesto: $crate::jvm::JObject<'local>,
            dati: $crate::jvm::JString<'local>,
            inizializza: $crate::jvm::jboolean,
        ) {
            $crate::jvm::avvia_il_nucleo(
                &mut env,
                &classe,
                &contesto,
                &dati,
                inizializza != 0,
                $avvia,
            );
        }
    };
}
