//! Le voci del portachiavi che riguardano Google.
//!
//! Il tratto, le sue tre implementazioni e la ragione per cui i segreti non
//! stanno nel database vivono in [`aether_oauth::portachiavi`], da quando i
//! fornitori che chiedono un consenso sono diventati due. Qui restano soltanto i
//! **nomi delle voci**, che di Google sono e non generalizzano: un `refresh_token`
//! senza il prefisso del servizio sarebbe una voce che il collegamento a Spotify
//! sovrascriverebbe senza accorgersene.

pub use aether_oauth::portachiavi::{Guasto, InMemoria, Portachiavi, SERVIZIO};

#[cfg(feature = "portachiavi")]
pub use aether_oauth::portachiavi::DiSistema;

/// La voce del token di aggiornamento di Google.
pub const GOOGLE_REFRESH_TOKEN: &str = "google.refresh_token";

/// La voce del segreto del client, quando è stato scritto dall'utente.
pub const GOOGLE_CLIENT_SECRET: &str = "google.client_secret";

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn le_voci_di_google_si_nominano_per_esteso() {
        // Il prefisso non è ornamento: `refresh_token` da solo sarebbe la voce
        // che il collegamento a Spotify sovrascriverebbe senza accorgersene.
        assert!(GOOGLE_REFRESH_TOKEN.starts_with("google."));
        assert!(GOOGLE_CLIENT_SECRET.starts_with("google."));
        assert_ne!(GOOGLE_REFRESH_TOKEN, GOOGLE_CLIENT_SECRET);
    }

    #[test]
    fn il_tratto_arriva_da_aether_oauth_e_funziona_ancora() {
        let portachiavi = InMemoria::nuovo();
        assert_eq!(portachiavi.scrivi(GOOGLE_REFRESH_TOKEN, "1//finto"), Ok(()));
        assert_eq!(
            portachiavi.leggi(GOOGLE_REFRESH_TOKEN),
            Ok(Some("1//finto".to_owned()))
        );
        let err = Guasto.leggi(GOOGLE_REFRESH_TOKEN).unwrap_err();
        assert_eq!(err.code().kind().code(), "settings.secretUnavailable");
    }
}
