//! Il ponte verso Android.
//!
//! Quel che su Windows fanno il Credential Manager, le SMTC, il Cestino e il
//! dialogo delle cartelle, su Android lo fanno il Keystore, la sessione
//! multimediale, `MediaStore` e il selettore di documenti. Sono tutte API Java,
//! e da Rust ci si arriva soltanto con JNI.
//!
//! # Perché un crate a parte
//!
//! Per la stessa ragione di `rymembra-barra`: parlare con la macchina virtuale
//! Java vuole due righe `unsafe` — i puntatori che `tao` ha consegnato a
//! `ndk-context` diventano oggetti di `jni` — e la radice del workspace vieta
//! `unsafe` con `forbid`. Qui la tabella dei lint è la sua, e l'eccezione sta in
//! un modulo solo, `jvm`.
//!
//! # Una porta sola
//!
//! Il lato Kotlin è l'oggetto `Ponte`, nel progetto Gradle dell'app
//! (`src-tauri/gen/android`). Espone **un** metodo, `chiama(comando,
//! argomenti)`, con argomenti e risposta in JSON. Una firma JNI scritta a mano
//! per ogni funzione sarebbe una firma da tenere allineata a mano su due lati,
//! e un errore lì non lo trova il compilatore: lo trova il telefono, con un
//! `NoSuchMethodError` all'avvio. Così la firma è una, e il resto è `serde`.
//!
//! # Perché non i plugin di Tauri
//!
//! Perché un comando di plugin, su Android, Tauri lo esegue sul filo
//! principale e `run_mobile_plugin` ne aspetta la risposta: chiamato da quel
//! filo — ed è il filo su cui girano i comandi sincroni della finestra — si
//! aspetterebbe da solo per sempre. JNI invece esegue il metodo Kotlin sul filo
//! che chiama, qualunque sia. Quel che in Kotlin deve stare sul filo principale
//! (la sessione multimediale, i selettori) ci va da sé, con un `post`.
//!
//! # Fuori da Android
//!
//! Il crate compila lo stesso, perché fa parte del workspace e `cargo test`
//! dalla radice lo costruisce su Windows: ogni funzione risponde con
//! [`Errore`] «non su Android», e nessuno la chiama.

#[cfg(target_os = "android")]
mod jvm;

pub mod auto;
pub mod segreti;
pub mod sessione;
pub mod sistema;

use std::fmt;

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Un guasto del ponte, già in parole.
///
/// Non porta un codice del catalogo: lo sceglie chi chiama, che sa che cosa
/// stava provando a fare. Qui c'è solo il perché, per il diario.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Errore(String);

impl Errore {
    /// Il guasto con questo testo.
    fn nuovo(testo: impl Into<String>) -> Self {
        Self(testo.into())
    }
}

impl fmt::Display for Errore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Errore {}

/// Quel che `Ponte.chiama` restituisce: il valore, o il perché no.
#[cfg_attr(
    not(target_os = "android"),
    allow(dead_code, reason = "lo legge solo il ramo Android di `chiama`")
)]
#[derive(serde::Deserialize)]
struct Risposta {
    #[serde(default)]
    ok: serde_json::Value,
    #[serde(default)]
    errore: Option<String>,
}

/// Chiama `Ponte.chiama(comando, argomenti)` e legge la risposta.
///
/// # Errori
///
/// Se la macchina virtuale non risponde, se Kotlin dice di no, o se la
/// risposta non ha la forma attesa.
#[cfg(target_os = "android")]
fn chiama<T: DeserializeOwned>(comando: &str, argomenti: &impl Serialize) -> Result<T, Errore> {
    let argomenti = serde_json::to_string(argomenti)
        .map_err(|err| Errore::nuovo(format!("argomenti di {comando}: {err}")))?;
    let testo = jvm::chiama(comando, &argomenti)?;
    let risposta: Risposta = serde_json::from_str(&testo)
        .map_err(|err| Errore::nuovo(format!("risposta di {comando}: {err}")))?;
    if let Some(errore) = risposta.errore {
        return Err(Errore::nuovo(format!("{comando}: {errore}")));
    }
    serde_json::from_value(risposta.ok)
        .map_err(|err| Errore::nuovo(format!("valore di {comando}: {err}")))
}

/// Fuori da Android: nessuno da chiamare.
///
/// # Errori
///
/// Sempre.
#[cfg(not(target_os = "android"))]
fn chiama<T: DeserializeOwned>(comando: &str, argomenti: &impl Serialize) -> Result<T, Errore> {
    let _ = argomenti;
    Err(Errore::nuovo(format!("{comando}: non su Android")))
}
