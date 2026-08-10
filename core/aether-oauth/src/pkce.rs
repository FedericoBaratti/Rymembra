//! PKCE: il segreto che resta in questo processo, e la sua prova pubblica.
//!
//! # Cosa protegge, davvero
//!
//! Il codice di autorizzazione torna indietro su un socket loopback, dove un
//! altro programma sulla stessa macchina potrebbe intercettarlo. Con PKCE quel
//! codice **da solo non vale niente**: scambiarlo richiede il `code_verifier`,
//! che è rimasto qui dentro e non è mai passato in rete.
//!
//! È anche il motivo per cui un `client_secret` dentro un binario desktop non è
//! un problema di questa gravità: `strings` lo trova in mezzo secondo, ma senza
//! il verifier non se ne fa niente. Vale al contrario, però — chi salta PKCE e
//! si affida al segreto non sta proteggendo nulla.

use aether_domain::errors::{AppError, ErrorCode};
use sha2::{Digest as _, Sha256};

use crate::base64::base64url;

/// Il segreto che resta in questo processo, e la sua prova pubblica.
///
/// Il `verifier` non esce mai da qui finché non è ora di scambiarlo; quel che
/// viaggia nell'indirizzo di autorizzazione è solo il suo digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pkce {
    verifier: String,
    challenge: String,
}

impl Pkce {
    /// Genera una coppia nuova.
    ///
    /// Trentadue byte di casualità del sistema operativo, scritti in base64url:
    /// quarantatré caratteri, dentro i 43..128 che la RFC 7636 impone e composti
    /// dei soli caratteri non riservati, quindi indenni a qualunque codifica li
    /// attraversi.
    ///
    /// # Errori
    ///
    /// `internal.unexpected` se il sistema operativo non fornisce casualità. È
    /// un guasto che non si aggira: un verifier prevedibile toglie a PKCE la
    /// ragione di esistere, e ripiegare su un orologio sarebbe peggio che non
    /// collegarsi.
    pub fn nuovo() -> Result<Self, AppError> {
        let verifier = base64url(&casuale::<32>()?);
        let challenge = base64url(Sha256::digest(verifier.as_bytes()).as_slice());
        Ok(Self {
            verifier,
            challenge,
        })
    }

    /// La prova pubblica, da mettere nell'indirizzo di autorizzazione.
    #[must_use]
    pub fn challenge(&self) -> &str {
        &self.challenge
    }

    /// Il segreto, da mettere **solo** nel modulo dello scambio.
    ///
    /// Non c'è un `Display` e non c'è un `Debug` che lo mostri per caso: chi lo
    /// vuole deve chiamare questa funzione, e chi legge una diff vede che l'ha
    /// chiamata.
    #[must_use]
    pub fn verifier(&self) -> &str {
        &self.verifier
    }
}

/// Un valore da mandare come `state` e da riconoscere al ritorno.
///
/// Sedici byte: non è un segreto crittografico, è quel che distingue la risposta
/// alla **nostra** richiesta da una risposta alla richiesta di qualcun altro che
/// bussa alla stessa porta.
///
/// # Errori
///
/// `internal.unexpected` se il sistema non fornisce casualità.
pub fn stato() -> Result<String, AppError> {
    Ok(base64url(&casuale::<16>()?))
}

/// Un identificativo casuale, buono a distinguere un dispositivo da un altro.
///
/// **Non è un segreto**: finisce in chiaro dentro il backup, ed è quel che
/// permette a una passata di riconoscere se il file lassù l'ha scritto questo
/// computer o un altro. Casuale e non il nome della macchina perché due computer
/// che si chiamano `DESKTOP-PC` non devono scambiarsi per lo stesso — e quando
/// succede, il sintomo è che ciascuno sovrascrive il backup dell'altro invece di
/// fondersi con lui.
///
/// # Errori
///
/// `internal.unexpected` se il sistema non fornisce casualità.
pub fn identificativo() -> Result<String, AppError> {
    Ok(base64url(&casuale::<8>()?))
}

/// `N` byte di casualità del sistema operativo.
fn casuale<const N: usize>() -> Result<[u8; N], AppError> {
    let mut byte = [0u8; N];
    getrandom::fill(&mut byte).map_err(|err| {
        AppError::new(ErrorCode::InternalUnexpected {
            detail: Some("il sistema non fornisce casualità".to_owned()),
        })
        .with_cause(err.to_string())
    })?;
    Ok(byte)
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn il_vettore_di_prova_della_rfc_7636() {
        // Il vettore dell'appendice B della RFC 7636. Se questa prova passa,
        // PKCE è calcolato come il fornitore se lo aspetta; se fallisce, il
        // collegamento fallirebbe solo dall'altra parte, con un messaggio che
        // non dice quale dei due pezzi è sbagliato.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = base64url(Sha256::digest(verifier.as_bytes()).as_slice());
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn un_verifier_e_lungo_quanto_la_rfc_impone() {
        let pkce = Pkce::nuovo().expect("casualità");
        assert_eq!(pkce.verifier().len(), 43, "il minimo della RFC 7636");
        assert!(
            pkce.verifier()
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~')),
            "solo caratteri non riservati: nessuna codifica lo può cambiare"
        );
        // Due chiamate non danno mai la stessa coppia: se la dessero, PKCE non
        // starebbe proteggendo niente.
        let altro = Pkce::nuovo().expect("casualità");
        assert_ne!(pkce.verifier(), altro.verifier());
        assert_ne!(pkce.challenge(), altro.challenge());
    }

    #[test]
    fn lo_stato_e_l_identificativo_non_si_ripetono() {
        assert_ne!(stato().expect("casualità"), stato().expect("casualità"));
        assert_ne!(
            identificativo().expect("casualità"),
            identificativo().expect("casualità")
        );
    }
}
