//! Le voci del portachiavi che riguardano Spotify.
//!
//! Il tratto, le sue implementazioni e la ragione per cui i segreti non stanno
//! nel database vivono in [`aether_oauth::portachiavi`]. Qui restano soltanto i
//! **nomi delle voci**, come in `aether_cloud::portachiavi` restano quelli di
//! Google: il prefisso non è ornamento, ed è l'unica cosa che impedisce a un
//! collegamento di sovrascrivere il token dell'altro senza accorgersene.
//!
//! # Cosa ci va, e cosa no
//!
//! Ci va **solo** il refresh token. Il `client_id` no: con PKCE non è un
//! segreto, e la nota in testa a [`crate::account::oauth`] argomenta perché.
//! Sta in `settings`, come `nuvola.client_id`, dove si può leggere e correggere
//! senza cercarlo in un portachiavi di sistema.
//!
//! E l'access token nemmeno: dura un'ora e si riottiene. Scriverlo su disco
//! vorrebbe dire moltiplicare i posti in cui un segreto può restare dopo che ha
//! smesso di servire.

/// La voce del token di aggiornamento di Spotify.
///
/// Va riscritta **a ogni rinfresco**, non solo al primo consenso: Spotify ruota
/// il refresh token, e quello di prima smette di valere. Vedi la nota in testa a
/// [`crate::account::oauth`].
pub const SPOTIFY_REFRESH_TOKEN: &str = "spotify.refresh_token";

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn la_voce_di_spotify_si_nomina_per_esteso() {
        // `refresh_token` da solo sarebbe la voce che il collegamento a Google
        // sovrascriverebbe senza accorgersene, e viceversa.
        assert!(SPOTIFY_REFRESH_TOKEN.starts_with("spotify."));
        assert_ne!(SPOTIFY_REFRESH_TOKEN, "google.refresh_token");
    }
}
