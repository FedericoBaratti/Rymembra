//! Le parti di OAuth 2.0 che non sanno con chi stanno parlando.
//!
//! # Perché esiste
//!
//! Questo codice è nato dentro `aether-cloud`, quando Google Drive era l'unico
//! posto in cui Aether avesse bisogno di un consenso. Con l'arrivo
//! dell'importazione dell'account Spotify i padroni sono diventati due, e
//! copiare PKCE è il genere di duplicazione che si paga più tardi: due
//! implementazioni di una cosa crittografica divergono in silenzio, e la seconda
//! sbaglia dalla parte del server — cioè dove non c'è nessun test a guardarla.
//!
//! # Il confine, e perché passa di qui
//!
//! Dentro sta quel che di OAuth è **uguale per tutti**: la coppia PKCE, la
//! codifica base64url che la RFC 7636 impone, il socket di loopback che
//! raccoglie la risposta del browser, il posto dove si tengono i segreti, e il
//! conto di quando un token è da rinfrescare.
//!
//! Fuori resta tutto quel che è **del fornitore**: gli indirizzi, gli scope, i
//! campi del modulo di scambio, la forma della risposta. Sembrano gli stessi e
//! non lo sono — Google manda il refresh token solo al primo consenso e vuole
//! `access_type=offline`, Spotify lo ruota a ogni rinfresco e con PKCE non vuole
//! nessun segreto del client. Un'astrazione che pretendesse di coprire anche
//! quelli avrebbe un parametro per ogni differenza, cioè sarebbe la somma delle
//! due implementazioni invece della loro parte comune.
//!
//! **Niente HTTP qui dentro.** Non è una privazione: nessuna delle cose in
//! questo crate ha bisogno della rete, e tenercela fuori è ciò che permette di
//! provarle tutte senza un server finto.
//!
//! # Cosa non finisce mai in un errore
//!
//! Il `code_verifier`, il codice di autorizzazione, gli access token, i refresh
//! token. Gli [`aether_domain::errors::AppError`] di questo crate arrivano fino
//! alla finestra dentro `ErroreIpc.cause`, e da lì nei registri: un segreto in
//! chiaro in un log è un segreto che qualcuno può ancora usare.

pub mod base64;
pub mod loopback;
pub mod pkce;
pub mod portachiavi;
pub mod scadenza;

pub use base64::{base64url, da_base64url};
pub use loopback::Attesa;
pub use pkce::{Pkce, identificativo, stato};
pub use portachiavi::Portachiavi;
pub use scadenza::{adesso_ms, ancora_valido};

/// Con chi si sta parlando.
///
/// Serve a due cose che sembrano una sola e non lo sono: nominare il fornitore
/// nei campi di macchina di un errore, e nominarlo in una frase che una persona
/// legge. Tenerle separate evita l'alternativa vera, che è scegliere fra un
/// `service: "Google"` maiuscolo dentro un codice d'errore e un «consenso
/// google» minuscolo dentro un messaggio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Servizio {
    /// Il nome nei campi di macchina: `google`, `spotify`. Minuscolo, stabile,
    /// e non si traduce — ci finiscono dentro i confronti.
    pub chiave: &'static str,
    /// Il nome che una persona legge: «Google», «Spotify».
    pub nome: &'static str,
}
