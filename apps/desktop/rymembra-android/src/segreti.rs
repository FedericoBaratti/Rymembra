//! I segreti, nel Keystore di Android.
//!
//! Quel che sul computer sta nel Credential Manager — i token di
//! aggiornamento, le chiavi dei modelli di linguaggio, l'identità di «Ascolta
//! insieme» — su Android sta cifrato con una chiave AES-GCM che vive nel
//! **Keystore**: generata dal sistema, mai esportabile, spesso custodita
//! dall'hardware. Il testo cifrato sta nelle preferenze private dell'app.
//!
//! La conseguenza che conta è quella del backup automatico di Android: anche se
//! le preferenze finissero copiate su un altro telefono, lì la chiave non c'è, e
//! quei byte non si leggono. Un segreto non viaggia mai insieme al modo di
//! aprirlo.

use serde::Serialize;

use crate::{Errore, chiama};

#[derive(Serialize)]
struct Chiave<'a> {
    chiave: &'a str,
}

#[derive(Serialize)]
struct Voce<'a> {
    chiave: &'a str,
    valore: &'a str,
}

/// Legge un segreto. `Ok(None)` se non c'è.
///
/// # Errori
///
/// Se il Keystore non risponde, o se il testo cifrato non si apre più — una
/// chiave cancellata dal sistema, un telefono ripristinato da un backup.
pub fn leggi(chiave: &str) -> Result<Option<String>, Errore> {
    chiama("segreto.leggi", &Chiave { chiave })
}

/// Scrive un segreto, sostituendo quello che c'era.
///
/// # Errori
///
/// Se il Keystore non risponde.
pub fn scrivi(chiave: &str, valore: &str) -> Result<(), Errore> {
    chiama("segreto.scrivi", &Voce { chiave, valore })
}

/// Cancella un segreto. Cancellarne uno che non c'è non è un errore.
///
/// # Errori
///
/// Se le preferenze non si lasciano scrivere.
pub fn cancella(chiave: &str) -> Result<(), Errore> {
    chiama("segreto.cancella", &Chiave { chiave })
}
