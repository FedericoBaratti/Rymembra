//! Il consenso di Spotify: PKCE, il browser, i token.
//!
//! # Nessun segreto del client, e non è una svista
//!
//! Google vuole un `client_secret` anche per le applicazioni desktop, e
//! `aether-cloud` lo manda spiegando in testa al modulo perché non sia davvero
//! un segreto. Spotify, con il flusso PKCE, **non lo vuole affatto**: mandarlo
//! non è più sicuro, è una richiesta malformata. È la differenza che rendeva
//! sbagliato astrarre i due fornitori dietro la stessa funzione, ed è il motivo
//! per cui [`aether_oauth`] contiene la coppia PKCE e il socket di loopback ma
//! non i campi del modulo di scambio.
//!
//! Quel che protegge lo scambio è quindi tutto PKCE: il codice di
//! autorizzazione torna su un socket loopback, dove un altro programma sulla
//! stessa macchina potrebbe intercettarlo, e da solo non vale niente perché
//! scambiarlo richiede il `code_verifier` rimasto in questo processo.
//!
//! # Il refresh token di Spotify **ruota**
//!
//! L'altra differenza, e quella che si paga se non la si sa. Google manda il
//! refresh token una volta sola, al primo consenso, e nelle risposte di
//! rinfresco non lo rimanda: chi chiama deve tenersi quello che ha, e
//! `aether_cloud::oauth::rinfresca` azzera il campo apposta perché nessuno lo
//! sovrascriva con `None`.
//!
//! Spotify fa l'opposto: ogni rinfresco **può** restituirne uno nuovo, e quello
//! vecchio smette di valere. Non salvarlo vuol dire che il collegamento
//! funziona finché l'access token dura, poi muore — e muore un'ora dopo, quando
//! nessuno sta più guardando. [`Token::refresh_token`] va quindi scritto nel
//! portachiavi **ogni volta che arriva**, non solo la prima.
//!
//! # La porta effimera è permessa, ed è verificato
//!
//! Spotify vieta `localhost` come indirizzo di ritorno e ammette
//! `http://127.0.0.1`. La porta si può omettere alla registrazione e sceglierla
//! a ogni collegamento (RFC 8252, § 7.3): è precisamente quel che
//! [`aether_oauth::Attesa`] fa già, aprendo sulla porta `0` e lasciando che sia
//! il sistema a darne una libera.
//!
//! # Cosa non finisce mai in un errore
//!
//! Il `code_verifier`, il codice di autorizzazione, l'access token, il refresh
//! token. Nelle cause degli errori finisce il campo `error` di Spotify —
//! `invalid_grant`, `invalid_client` — che serve a capire cosa è successo e non
//! è un segreto.

use std::time::Duration;

use aether_domain::errors::{AppError, ErrorCode};
use aether_net::{Corpo, Metodo, Rete, Richiesta, percento};
use aether_oauth::{Attesa, Pkce, Servizio, adesso_ms};

/// Con chi si sta parlando, per gli errori del consenso.
pub const SPOTIFY: Servizio = Servizio {
    chiave: "spotify",
    nome: "Spotify",
};

/// Dove si manda l'utente a dare il consenso.
const AUTORIZZAZIONE: &str = "https://accounts.spotify.com/authorize";

/// Dove si scambiano codici e token.
const SCAMBIO: &str = "https://accounts.spotify.com/api/token";

/// Quel che Aether chiede di poter fare.
///
/// **Tutti di sola lettura, e uno per uno c'è una ragione.**
///
/// - `user-read-private` — il profilo, e con lui `product`: è l'unico modo di
///   accorgersi che l'account non ha Premium, che dal febbraio 2026 è quel che
///   fa smettere di funzionare un'applicazione in Development Mode. Saperlo
///   prima è la differenza fra un avviso e un guasto senza spiegazione.
/// - `user-library-read` — i «Brani che ti piacciono» e gli album salvati.
/// - `playlist-read-private` e `playlist-read-collaborative` — le playlist che
///   non sono pubbliche. Senza, si importerebbe solo la parte visibile a
///   chiunque, cioè quasi niente.
/// - `user-follow-read` — gli artisti seguiti.
/// - `user-read-recently-played` — gli ultimi cinquanta ascolti.
///
/// # Quel che **non** c'è, di proposito
///
/// Nessuno scope di scrittura: né `playlist-modify-*`, né `user-library-modify`,
/// né `user-follow-modify`. Aether importa da Spotify e non tocca l'account, e
/// questa costante è il posto in cui quella promessa è verificabile — la
/// schermata di consenso elenca quel che c'è scritto qui, e chi la legge deve
/// poter vedere che non c'è niente che modifichi.
///
/// Niente `streaming` né `app-remote-control`: Aether suona i propri file, non
/// il catalogo di Spotify.
///
/// Niente `user-top-read`, che pure era nel piano: `/me/top/tracks` darebbe i
/// brani più ascoltati, e [`aether_domain::spotify_account::AccountSnapshot`]
/// non ha nessun posto dove metterli. Uno scope chiesto e non usato è una riga
/// in più nella schermata di consenso in cambio di niente.
pub const SCOPE: &str = "user-read-private user-library-read playlist-read-private \
                         playlist-read-collaborative user-follow-read user-read-recently-played";

/// Quanto si aspetta il consenso prima di rinunciare.
pub const ATTESA_CONSENSO: Duration = Duration::from_secs(3 * 60);

/// La scadenza delle richieste ai punti OAuth.
pub const SCADENZA: Duration = Duration::from_secs(15);

/// I token appena ricevuti da Spotify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// Il token con cui si chiamano le API, valido un'ora.
    pub access_token: String,
    /// Il token con cui si ottengono altri access token.
    ///
    /// **Ruota**: vedi la nota in testa al modulo. Va riscritto nel portachiavi
    /// ogni volta che arriva, non solo al primo consenso.
    pub refresh_token: Option<String>,
    /// Quando scade l'access token, in millisecondi dall'epoca.
    pub scade_ms: i64,
}

/// L'indirizzo a cui mandare l'utente, e lo `state` che dovrà tornare indietro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invito {
    /// L'indirizzo da aprire nel browser di sistema.
    pub url: String,
    /// Il valore che la risposta dovrà riportare identico.
    pub state: String,
}

/// Compone l'indirizzo della schermata di consenso.
///
/// # Errori
///
/// `internal.unexpected` se il sistema non fornisce casualità per lo `state`.
pub fn invito(client_id: &str, pkce: &Pkce, redirect_uri: &str) -> Result<Invito, AppError> {
    let state = aether_oauth::stato()?;
    let url = format!(
        "{AUTORIZZAZIONE}\
         ?client_id={}\
         &response_type=code\
         &redirect_uri={}\
         &scope={}\
         &code_challenge_method=S256\
         &code_challenge={}\
         &state={}",
        percento(client_id),
        percento(redirect_uri),
        percento(SCOPE),
        percento(pkce.challenge()),
        percento(&state),
    );
    Ok(Invito { url, state })
}

/// Scambia il codice di autorizzazione con i token.
///
/// # Errori
///
/// `spotify.accountAuthExpired` se Spotify rifiuta la concessione, `net.*` per i
/// guasti di rete e di protocollo.
pub fn scambia(
    rete: &Rete,
    client_id: &str,
    pkce: &Pkce,
    redirect_uri: &str,
    code: &str,
) -> Result<Token, AppError> {
    let campi = [
        ("grant_type", "authorization_code".to_owned()),
        ("code", code.to_owned()),
        ("redirect_uri", redirect_uri.to_owned()),
        ("client_id", client_id.to_owned()),
        ("code_verifier", pkce.verifier().to_owned()),
    ];
    interpreta_token(&chiedi(rete, &campi)?)
}

/// Ottiene un access token nuovo da un refresh token.
///
/// Il [`Token::refresh_token`] che torna **va salvato**: Spotify lo ruota, e
/// quello che si aveva prima può aver smesso di valere in questo istante. Quando
/// invece non ne manda uno nuovo, il campo resta `None` e chi chiama tiene il
/// suo — la differenza fra i due casi è tutta lì, e per questo non si inventa un
/// valore.
///
/// # Errori
///
/// `spotify.accountAuthExpired` se il refresh token non vale più: l'utente ha
/// revocato l'accesso dalla propria pagina, oppure l'applicazione non è più
/// abilitata. In entrambi i casi si ricomincia dal consenso, e nessun tentativo
/// automatico aggiusta niente.
pub fn rinfresca(rete: &Rete, client_id: &str, refresh_token: &str) -> Result<Token, AppError> {
    let campi = [
        ("grant_type", "refresh_token".to_owned()),
        ("refresh_token", refresh_token.to_owned()),
        ("client_id", client_id.to_owned()),
    ];
    interpreta_token(&chiedi(rete, &campi)?)
}

/// Il giro completo del collegamento: apre il servitore, invita, aspetta,
/// scambia.
///
/// `apri_browser` riceve l'indirizzo del consenso. Sta fuori da questo crate
/// perché aprire una finestra è compito di chi ha un'interfaccia: qui si
/// vedrebbe soltanto una dipendenza da Tauri in un crate che deve poter girare
/// dentro un esempio da riga di comando.
///
/// # Errori
///
/// `spotify.accountNotConfigured` se il `client_id` è vuoto, `internal.timeout`
/// se l'utente non risponde, `internal.aborted` se nega il consenso,
/// `spotify.accountAuthExpired` se lo scambio viene rifiutato, `net.*` per la
/// rete.
pub fn collega(
    rete: &Rete,
    client_id: &str,
    apri_browser: impl FnOnce(&str) -> Result<(), AppError>,
) -> Result<Token, AppError> {
    if client_id.trim().is_empty() {
        return Err(AppError::new(ErrorCode::SpotifyAccountNotConfigured));
    }
    // Il servitore si apre per primo: la porta fa parte dell'indirizzo di
    // ritorno, e quell'indirizzo fa parte di ciò che si firma con PKCE.
    let attesa = Attesa::apri(SPOTIFY)?;
    let redirect_uri = attesa.redirect_uri();
    let pkce = Pkce::nuovo()?;
    let invito = invito(client_id, &pkce, &redirect_uri)?;

    apri_browser(&invito.url)?;
    let risposta = attesa.aspetta(&invito.state, ATTESA_CONSENSO)?;
    scambia(rete, client_id, &pkce, &redirect_uri, &risposta.code)
}

/// Manda una richiesta al punto di scambio e restituisce il JSON.
///
/// **Non si ritenta.** Un 4xx dal punto di scambio significa che la concessione
/// non vale, e ripeterla darà la stessa risposta; un 5xx su un'azione che
/// l'utente sta guardando è meglio riportarlo subito che farlo aspettare mentre
/// riproviamo in silenzio.
fn chiedi(rete: &Rete, campi: &[(&str, String)]) -> Result<serde_json::Value, AppError> {
    let risposta = rete.esegui(Richiesta {
        metodo: Metodo::Post,
        url: SCAMBIO,
        intestazioni: &[],
        corpo: Corpo::Modulo(campi),
    })?;

    let corpo: serde_json::Value = serde_json::from_slice(&risposta.corpo).unwrap_or_default();
    if risposta.e_andata() {
        return Ok(corpo);
    }

    // `invalid_grant` è il caso vero e ha una risposta precisa: rimandare
    // l'utente al consenso. Distinguerlo dagli altri guasti è la differenza fra
    // «riprova più tardi» e «devi ricollegare l'account».
    let motivo = corpo
        .get("error")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if motivo == "invalid_grant" {
        return Err(
            AppError::new(ErrorCode::SpotifyAccountAuthExpired).with_cause(motivo.to_owned())
        );
    }
    Err(rete.stato_a_errore(&risposta, SCAMBIO))
}

/// Legge la risposta del punto di scambio.
fn interpreta_token(corpo: &serde_json::Value) -> Result<Token, AppError> {
    let access_token = corpo
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            AppError::new(ErrorCode::NetBadSchema {
                service: Some(SPOTIFY.chiave.to_owned()),
                detail: Some("la risposta non contiene un access token".to_owned()),
            })
        })?
        .to_owned();

    // `expires_in` è in secondi e Spotify lo manda sempre: l'ora di serie è
    // quella che usa comunque, e vale come rete di sicurezza.
    let dura = corpo
        .get("expires_in")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(3600);

    Ok(Token {
        access_token,
        refresh_token: corpo
            .get("refresh_token")
            .and_then(serde_json::Value::as_str)
            .filter(|token| !token.is_empty())
            .map(ToOwned::to_owned),
        scade_ms: adesso_ms().saturating_add(dura.saturating_mul(1000)),
    })
}

#[cfg(test)]
mod prove {
    use super::*;
    use aether_oauth::ancora_valido;

    #[test]
    fn l_indirizzo_di_consenso_ha_tutto_quel_che_serve() {
        let pkce = Pkce::nuovo().expect("casualità");
        let invito = invito("finto123", &pkce, "http://127.0.0.1:54321").expect("invito");

        assert!(invito.url.starts_with(AUTORIZZAZIONE));
        assert!(invito.url.contains("response_type=code"));
        assert!(invito.url.contains("code_challenge_method=S256"));
        assert!(
            invito
                .url
                .contains(&format!("code_challenge={}", percento(pkce.challenge())))
        );
        assert!(
            invito
                .url
                .contains(&format!("state={}", percento(&invito.state)))
        );
        // `127.0.0.1`, mai `localhost`: Spotify il secondo lo rifiuta.
        assert!(
            invito
                .url
                .contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A54321")
        );
        // E quel che non ci deve essere: il verifier resta in questo processo.
        assert!(!invito.url.contains(pkce.verifier()));
    }

    #[test]
    fn nessun_segreto_del_client_viaggia_mai() {
        // La differenza da Google, resa una prova: con PKCE Spotify non vuole
        // il segreto, e mandarlo non sarebbe più sicuro — sarebbe una richiesta
        // malformata. Se un giorno qualcuno aggiungesse il campo «per
        // simmetria», questa prova glielo dice.
        let pkce = Pkce::nuovo().expect("casualità");
        let invito = invito("finto123", &pkce, "http://127.0.0.1:1").expect("invito");
        assert!(!invito.url.contains("client_secret"));
    }

    #[test]
    fn lo_scope_e_di_sola_lettura_e_non_si_allarga() {
        // Una prova sull'*intenzione*, copiata da quella di `aether-cloud`:
        // allargare uno scope è una riga sola, e la schermata di consenso è
        // l'unico posto in cui l'utente potrebbe accorgersene — dopo aver già
        // deciso di fidarsi.
        for scope in SCOPE.split_whitespace() {
            assert!(
                !scope.contains("modify"),
                "{scope} può scrivere: Aether importa e non tocca l'account"
            );
        }
        assert!(!SCOPE.contains("streaming"));
        assert!(!SCOPE.contains("app-remote-control"));
        assert!(!SCOPE.contains("ugc-image-upload"));
        assert!(!SCOPE.contains("user-read-playback"));
        assert!(
            !SCOPE.contains("user-top-read"),
            "chiesto e mai usato: `AccountSnapshot` non ha dove metterlo"
        );
    }

    #[test]
    fn gli_scope_che_servono_ci_sono_tutti() {
        // L'altra metà della prova sopra: una restrizione di troppo si
        // manifesta come «l'importazione non ha trovato le tue playlist», che
        // sembra un guasto dell'abbinamento e non è.
        for atteso in [
            "user-read-private",
            "user-library-read",
            "playlist-read-private",
            "playlist-read-collaborative",
            "user-follow-read",
            "user-read-recently-played",
        ] {
            assert!(
                SCOPE.split_whitespace().any(|s| s == atteso),
                "manca lo scope {atteso}"
            );
        }
    }

    #[test]
    fn una_risposta_senza_access_token_non_e_un_token() {
        let corpo = serde_json::json!({"token_type": "Bearer"});
        let err = interpreta_token(&corpo).expect_err("senza access token non è un token");
        assert_eq!(err.code().kind().code(), "net.badSchema");
    }

    #[test]
    fn il_refresh_token_nuovo_si_legge_perche_va_salvato() {
        // Spotify lo ruota: quello di prima ha appena smesso di valere. Non
        // leggerlo qui vorrebbe dire che il collegamento muore un'ora dopo,
        // quando nessuno sta più guardando.
        let corpo = serde_json::json!({
            "access_token": "BQfinto",
            "refresh_token": "AQruotato",
            "expires_in": 3600,
            "token_type": "Bearer",
        });
        let token = interpreta_token(&corpo).expect("token");
        assert_eq!(token.access_token, "BQfinto");
        assert_eq!(token.refresh_token.as_deref(), Some("AQruotato"));
        assert!(token.scade_ms > adesso_ms());
        assert!(ancora_valido(token.scade_ms));
    }

    #[test]
    fn un_rinfresco_senza_token_nuovo_lascia_none() {
        // `None` vuol dire «tieni quello che hai», e va distinto da una stringa
        // vuota che cancellerebbe il portachiavi.
        let corpo = serde_json::json!({"access_token": "BQfinto", "refresh_token": ""});
        let token = interpreta_token(&corpo).expect("token");
        assert_eq!(token.refresh_token, None);
    }

    #[test]
    fn il_consenso_si_aspetta_a_nome_di_spotify() {
        assert_eq!(SPOTIFY.chiave, "spotify");
        assert_eq!(SPOTIFY.nome, "Spotify");
    }

    #[test]
    fn senza_client_id_non_si_apre_nessun_browser() {
        // L'unica cosa che in Aether l'utente deve registrare da sé: dirlo con
        // un codice suo evita che un campo vuoto diventi un errore di rete
        // incomprensibile.
        let rete = Rete::nuova("spotify", SCADENZA);
        let esito = collega(&rete, "   ", |_| {
            panic!("il browser non si deve aprire senza un client id")
        });
        assert_eq!(
            esito.err().map(|e| e.code().kind().code().to_owned()),
            Some("spotify.accountNotConfigured".to_owned())
        );
    }
}
