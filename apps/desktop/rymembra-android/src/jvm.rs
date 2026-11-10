//! La macchina virtuale Java, e la classe `Ponte` dentro di lei.
//!
//! L'unico posto `unsafe` del crate. Le due operazioni pericolose sono le due
//! conversioni da puntatore grezzo a oggetto di `jni`, fatte una volta per
//! processo: tutto il resto — trovare la classe, chiamare il metodo, leggere la
//! stringa — passa dall'API sicura di `jni`.
//!
//! # Da dove vengono i puntatori
//!
//! Da `tao`, che all'apertura dell'attività li consegna a `ndk-context`
//! (`platform_impl/android/ndk_glue.rs`): il `JavaVM` del processo e un
//! riferimento **globale** al contesto dell'applicazione, che `tao` tiene vivo
//! in una statica fino all'uscita. È lo stesso contesto che `cpal` usa per
//! l'uscita audio.
//!
//! # Perché il caricatore di classi dell'applicazione
//!
//! Perché un filo nato in Rust e agganciato alla macchina virtuale vede solo il
//! caricatore di sistema, che conosce le classi di Android e non quelle
//! dell'app: `FindClass("…/Ponte")` da lì fallisce. Il caricatore giusto è
//! quello del contesto, e la classe trovata si tiene in un riferimento globale.

#![allow(
    unsafe_code,
    reason = "vedi il preambolo: i due puntatori di `ndk-context`"
)]

use std::sync::OnceLock;

use jni::JavaVM;
use jni::objects::{GlobalRef, JClass, JObject, JString, JValue};

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
