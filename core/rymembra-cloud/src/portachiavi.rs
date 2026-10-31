//! Le voci del portachiavi: quelle di Google, e quelle dei conti del cloud.
//!
//! Il tratto, le sue tre implementazioni e la ragione per cui i segreti non
//! stanno nel database vivono in [`rymembra_oauth::portachiavi`], da quando i
//! fornitori che chiedono un consenso sono diventati due. Qui restano soltanto i
//! **nomi delle voci**, che di Google sono e non generalizzano: un `refresh_token`
//! senza il prefisso del servizio sarebbe una voce che il collegamento a Spotify
//! sovrascriverebbe senza accorgersene.

pub use rymembra_oauth::portachiavi::{Guasto, InMemoria, Portachiavi, SERVIZIO};

#[cfg(feature = "portachiavi")]
pub use rymembra_oauth::portachiavi::DiSistema;

/// La voce del token di aggiornamento di Google.
pub const GOOGLE_REFRESH_TOKEN: &str = "google.refresh_token";

/// La voce del segreto del client Google com'era fino alla 3.1, quando di app
/// ce n'era una sola.
///
/// Dalla 3.2 il segreto di un'app sta sotto [`nuvola_app_segreto`]; questa voce
/// si legge solo per travasarla nell'app che la migrazione 037 ha creato
/// (`google-ereditata`), e poi si cancella.
pub const GOOGLE_CLIENT_SECRET: &str = "google.client_secret";

/// La voce del segreto di un'app dell'utente: `nuvola.app.<app>.segreto`.
///
/// Per app e non per piattaforma, perché le app Google possono essere più
/// d'una. L'identificativo dell'app non si riusa mai (vedi la 037): una voce
/// rimasta da un'app tolta non può finire a un'app nuova con lo stesso nome.
/// Non si confonde con le voci dei conti, che cominciano col conto
/// (`nuvola.onedrive-…`) e mai con `app`.
#[must_use]
pub fn nuvola_app_segreto(app: &str) -> String {
    format!("nuvola.app.{app}.segreto")
}

/// La voce del token di aggiornamento di un conto del cloud:
/// `nuvola.<conto>.refresh`.
///
/// Per conto e non per servizio: due account OneDrive collegati alla stessa
/// libreria hanno due refresh token, e una voce `onedrive.refresh` sarebbe
/// quella che il secondo collegamento sovrascrive al primo. Il conto contiene
/// già il servizio (`onedrive-k3x7q2ab`), e non contiene segreti: è
/// un'impronta, la stessa che sta in ogni percorso.
#[must_use]
pub fn nuvola_refresh(conto: &str) -> String {
    format!("nuvola.{conto}.refresh")
}

/// La voce del portachiavi con la password di un conto WebDAV (una «password
/// per app», di solito). Mai in `settings`: vedi il `//!`.
#[must_use]
pub fn nuvola_password(conto: &str) -> String {
    format!("nuvola.{conto}.password")
}

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
    fn la_voce_di_un_conto_del_cloud_e_per_conto() {
        assert_eq!(
            nuvola_refresh("onedrive-k3x7q2ab"),
            "nuvola.onedrive-k3x7q2ab.refresh"
        );
        assert_ne!(
            nuvola_refresh("onedrive-k3x7q2ab"),
            nuvola_refresh("onedrive-aaaaaaaa"),
            "due account dello stesso servizio non si sovrascrivono"
        );
        assert!(!nuvola_refresh("onedrive-k3x7q2ab").starts_with("google."));
    }

    #[test]
    fn la_voce_del_segreto_e_per_app() {
        assert_eq!(
            nuvola_app_segreto("google-ereditata"),
            "nuvola.app.google-ereditata.segreto"
        );
        assert_ne!(nuvola_app_segreto("google-aaaaaaaa"), GOOGLE_CLIENT_SECRET);
        // Nessun conto si chiama `app`: un conto è `<servizio>-<impronta>`.
        assert!(!rymembra_domain::nuvola::e_conto("app"));
    }

    #[test]
    fn il_tratto_arriva_da_rymembra_oauth_e_funziona_ancora() {
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
