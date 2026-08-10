//! OAuth 2.0 con Google: PKCE, il consenso nel browser, i token.
//!
//! # Il segreto del client non è un segreto
//!
//! Va detto qui, nel punto in cui qualcuno sarebbe tentato di «indurire» la cosa
//! sbagliata. Il `client_secret` di un'applicazione desktop finisce dentro il
//! binario, e `strings` lo trova in mezzo secondo. La documentazione di Google
//! per le *installed app* lo dice apertamente: per quel tipo di client il
//! segreto non è trattato come una credenziale riservata.
//!
//! Quel che protegge davvero lo scambio è **PKCE**. Il codice di autorizzazione
//! torna indietro su un socket loopback, dove un altro programma sulla stessa
//! macchina potrebbe intercettarlo; con PKCE quel codice da solo non vale
//! niente, perché scambiarlo richiede il `code_verifier` che è rimasto in questo
//! processo e non è mai passato in rete. Senza PKCE, chi ruba il codice e legge
//! il segreto dal binario ottiene i token.
//!
//! L'esposizione vera che resta è che un terzo consumi la quota del progetto
//! Google. Le mitigazioni sono strutturali e stanno altrove: lo scope è solo
//! `drive.appdata`, quindi un client rubato non legge niente del Drive di
//! nessuno, e la quota si sorveglia dalla Console.
//!
//! # Cosa non finisce mai in un errore
//!
//! Il `code_verifier`, il codice di autorizzazione, l'access token, il refresh
//! token. Gli [`AppError`] di questo crate arrivano alla finestra dentro
//! `ErroreIpc.cause` e da lì nei registri. Le funzioni qui sotto mettono nella
//! causa il **campo `error` di Google** — `invalid_grant`, `invalid_client` —
//! che è quel che serve a capire cosa è successo e non è un segreto.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aether_domain::errors::{AppError, ErrorCode};
use sha2::{Digest as _, Sha256};

use crate::http::{Corpo, Metodo, Rete, Richiesta, percento};
use crate::loopback::Attesa;

/// Dove si manda l'utente a dare il consenso.
const AUTORIZZAZIONE: &str = "https://accounts.google.com/o/oauth2/v2/auth";

/// Dove si scambiano codici e token.
const SCAMBIO: &str = "https://oauth2.googleapis.com/token";

/// Dove si revoca.
const REVOCA: &str = "https://oauth2.googleapis.com/revoke";

/// Quel che Aether chiede di poter fare.
///
/// `drive.appdata` è una cartella **privata dell'applicazione**: invisibile
/// nell'interfaccia di Drive, illeggibile da qualunque altra app, e cancellata
/// quando l'utente rimuove i dati di Aether dal suo account. È uno scope **non
/// sensibile**: non fa scattare nessuna verifica di Google e nessun security
/// assessment.
///
/// **Mai `drive.file`, mai `drive`.** Il primo è già sensibile e obbliga alla
/// verifica; il secondo chiede l'intero Drive dell'utente per salvare settanta
/// chilobyte di conteggi d'ascolto, e non c'è modo di scriverlo in una schermata
/// di consenso che non spaventi giustamente chi la legge.
pub const SCOPE: &str = "https://www.googleapis.com/auth/drive.appdata openid email";

/// Quanto si aspetta il consenso prima di rinunciare.
pub const ATTESA_CONSENSO: Duration = Duration::from_secs(3 * 60);

/// La scadenza delle richieste ai punti OAuth.
///
/// Quindici secondi, il valore che il vecchio albero Electron aveva già provato
/// sul campo: sono richieste piccole verso un servizio veloce, e una che non
/// risponde in quindici secondi non risponderà.
pub const SCADENZA: Duration = Duration::from_secs(15);

/// Con quanto anticipo si rinfresca un token che sta per scadere.
///
/// Un minuto: il tempo che una passata di backup può metterci fra il momento in
/// cui controlla la scadenza e quello in cui fa l'ultima richiesta. Senza
/// anticipo, un token valido «ancora due secondi» supererebbe il controllo e
/// darebbe un 401 a metà caricamento.
const ANTICIPO: Duration = Duration::from_secs(60);

/// Il client OAuth di questa applicazione.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credenziali {
    /// L'identificativo del client.
    pub client_id: String,
    /// Il segreto del client. Vedi la nota in testa al modulo.
    pub client_secret: String,
}

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
}

/// I token appena ricevuti da Google.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// Il token con cui si chiamano le API, valido un'ora circa.
    pub access_token: String,
    /// Il token con cui si ottengono altri access token.
    ///
    /// Google lo manda **solo** al primo consenso, e mai nelle risposte di
    /// rinfresco: perderlo significa dover rimandare l'utente alla schermata di
    /// consenso. È il motivo per cui va nel portachiavi appena arriva.
    pub refresh_token: Option<String>,
    /// Quando scade l'access token, in millisecondi dall'epoca.
    pub scade_ms: i64,
    /// L'indirizzo email dell'account, se Google l'ha detto.
    pub email: Option<String>,
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
/// # I due parametri che si dimenticano sempre
///
/// `access_type=offline` è ciò che fa mandare a Google un refresh token: senza,
/// si ottiene solo un access token da un'ora e il backup automatico smette di
/// funzionare dopo pranzo, senza dire niente.
///
/// `prompt=consent` forza la schermata anche quando l'utente ha già dato il
/// permesso in passato. Sembra scortese ed è necessario: al secondo consenso
/// Google **non rimanda il refresh token**, quindi chi si scollega e si ricollega
/// si ritroverebbe senza — e la strada «scollega e ricollega» è precisamente
/// quella che uno prende quando qualcosa non va.
///
/// # Errori
///
/// `internal.unexpected` se il sistema non fornisce casualità per lo `state`.
pub fn invito(
    credenziali: &Credenziali,
    pkce: &Pkce,
    redirect_uri: &str,
) -> Result<Invito, AppError> {
    let state = base64url(&casuale::<16>()?);
    let url = format!(
        "{AUTORIZZAZIONE}\
         ?client_id={}\
         &redirect_uri={}\
         &response_type=code\
         &scope={}\
         &code_challenge={}\
         &code_challenge_method=S256\
         &state={}\
         &access_type=offline\
         &prompt=consent",
        percento(&credenziali.client_id),
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
/// `sync.authExpired` se Google rifiuta la concessione, `net.*` per i guasti di
/// rete e di protocollo.
pub fn scambia(
    rete: &Rete,
    credenziali: &Credenziali,
    pkce: &Pkce,
    redirect_uri: &str,
    code: &str,
) -> Result<Token, AppError> {
    let campi = [
        ("grant_type", "authorization_code".to_owned()),
        ("code", code.to_owned()),
        ("client_id", credenziali.client_id.clone()),
        ("client_secret", credenziali.client_secret.clone()),
        ("redirect_uri", redirect_uri.to_owned()),
        ("code_verifier", pkce.verifier.clone()),
    ];
    interpreta_token(&chiedi(rete, &campi)?)
}

/// Ottiene un access token nuovo da un refresh token.
///
/// # Errori
///
/// `sync.authExpired` se il refresh token non vale più — l'utente ha revocato
/// l'accesso, oppure il progetto è rimasto in «Testing» e Google l'ha scaduto
/// dopo sette giorni. In entrambi i casi si ricomincia dal consenso, e nessun
/// tentativo automatico aggiusta niente.
pub fn rinfresca(
    rete: &Rete,
    credenziali: &Credenziali,
    refresh_token: &str,
) -> Result<Token, AppError> {
    let campi = [
        ("grant_type", "refresh_token".to_owned()),
        ("refresh_token", refresh_token.to_owned()),
        ("client_id", credenziali.client_id.clone()),
        ("client_secret", credenziali.client_secret.clone()),
    ];
    let mut token = interpreta_token(&chiedi(rete, &campi)?)?;
    // Google non rimanda il refresh token quando ne rinfresca uno. Chi chiama
    // deve tenersi quello che ha: azzerarlo qui vorrebbe dire cancellare dal
    // portachiavi l'unica cosa che non si può riottenere senza l'utente.
    token.refresh_token = None;
    Ok(token)
}

/// Revoca l'accesso, per quanto Google voglia collaborare.
///
/// Best-effort di proposito: chi si scollega deve vedersi scollegato anche se il
/// computer è senza rete, e la voce del portachiavi va pulita comunque. Un
/// errore qui è un'informazione per chi legge un registro, non una ragione per
/// rifiutare lo scollegamento — chi chiama può ignorarlo.
///
/// # Errori
///
/// `net.*` se la richiesta non arriva o Google rifiuta.
pub fn revoca(rete: &Rete, token: &str) -> Result<(), AppError> {
    let campi = [("token", token.to_owned())];
    let risposta = rete.esegui(Richiesta {
        metodo: Metodo::Post,
        url: REVOCA,
        intestazioni: &[],
        corpo: Corpo::Modulo(&campi),
    })?;
    if risposta.e_andata() {
        return Ok(());
    }
    Err(rete.stato_a_errore(&risposta, REVOCA))
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
/// `internal.timeout` se l'utente non risponde, `internal.aborted` se nega il
/// consenso, `sync.authExpired` se lo scambio viene rifiutato, `net.*` per la
/// rete.
pub fn collega(
    rete: &Rete,
    credenziali: &Credenziali,
    apri_browser: impl FnOnce(&str) -> Result<(), AppError>,
) -> Result<Token, AppError> {
    // Il servitore si apre per primo: la porta fa parte dell'indirizzo di
    // ritorno, e quell'indirizzo fa parte di ciò che si firma con PKCE.
    let attesa = Attesa::apri()?;
    let redirect_uri = attesa.redirect_uri();
    let pkce = Pkce::nuovo()?;
    let invito = invito(credenziali, &pkce, &redirect_uri)?;

    apri_browser(&invito.url)?;
    let risposta = attesa.aspetta(&invito.state, ATTESA_CONSENSO)?;
    scambia(rete, credenziali, &pkce, &redirect_uri, &risposta.code)
}

/// Manda una richiesta a un punto OAuth e restituisce il JSON della risposta.
///
/// **Non si ritenta.** Un 4xx dal punto di scambio significa che la concessione
/// non vale, e ripeterla darà la stessa risposta; un 5xx da Google su un'azione
/// che l'utente sta guardando è meglio riportarlo subito che farlo aspettare
/// mentre riproviamo in silenzio. Il ritentare sta sulle chiamate a Drive, dove
/// l'utente non sta guardando.
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
        return Err(AppError::new(ErrorCode::SyncAuthExpired).with_cause(motivo.to_owned()));
    }
    Err(rete.stato_a_errore(&risposta, SCAMBIO))
}

/// Legge la risposta di un punto OAuth.
fn interpreta_token(corpo: &serde_json::Value) -> Result<Token, AppError> {
    let access_token = corpo
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            AppError::new(ErrorCode::NetBadSchema {
                service: Some("google".to_owned()),
                detail: Some("la risposta non contiene un access token".to_owned()),
            })
        })?
        .to_owned();

    // `expires_in` è in secondi e Google lo manda sempre; l'ora di serie è
    // quella che Google usa comunque, e vale come rete di sicurezza.
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
        email: corpo
            .get("id_token")
            .and_then(serde_json::Value::as_str)
            .and_then(email_dal_id_token),
    })
}

/// L'email dentro un id token, **senza** verificarne la firma.
///
/// # Perché non verificare la firma è corretto qui, e solo qui
///
/// Verificare un JWT serve a chi lo riceve da terzi e deve stabilire se
/// crederci. Questo token è arrivato in risposta diretta a una nostra richiesta
/// HTTPS verso `oauth2.googleapis.com`: il canale è già autenticato dal
/// certificato, e chi potesse manometterlo potrebbe manomettere anche
/// l'access token — cioè la firma non proteggerebbe da niente che non sia già
/// perso. È l'eccezione che la documentazione di Google descrive per i token
/// ottenuti direttamente dal punto di scambio.
///
/// Vale perché di quell'email non facciamo **nessun uso di sicurezza**: la si
/// mostra nelle Impostazioni, per far sapere a chi guarda su quale account sta
/// salvando. Il giorno in cui servisse a decidere un accesso, questa funzione
/// non basterebbe più.
fn email_dal_id_token(id_token: &str) -> Option<String> {
    let carico = id_token.split('.').nth(1)?;
    let byte = da_base64url(carico)?;
    let campi: serde_json::Value = serde_json::from_slice(&byte).ok()?;
    campi
        .get("email")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

/// L'access token è ancora buono fra un minuto?
///
/// Il minuto di anticipo è il punto: vedi [`ANTICIPO`].
#[must_use]
pub fn ancora_valido(scade_ms: i64) -> bool {
    let anticipo = i64::try_from(ANTICIPO.as_millis()).unwrap_or(60_000);
    adesso_ms().saturating_add(anticipo) < scade_ms
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

/// L'orologio, in millisecondi dall'epoca.
fn adesso_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|quanto| i64::try_from(quanto.as_millis()).ok())
        .unwrap_or(0)
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

/// L'alfabeto base64url: `-` e `_` al posto di `+` e `/`.
const ALFABETO: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Codifica in base64url **senza riempimento**.
///
/// Senza `=` finali perché è quel che la RFC 7636 impone per PKCE, e perché un
/// `=` dentro una `query` va poi codificato a sua volta: due modi di sbagliare
/// tolti insieme.
///
/// Scritta con iteratori e `get`, non con indici: `indexing_slicing` è `deny` in
/// questo workspace, e in una funzione che maneggia byte crittografici quella
/// regola vale il doppio — un indice fuori posto qui non darebbe un errore, ma
/// una stringa sbagliata che fallisce solo dalla parte di Google.
#[must_use]
pub fn base64url(byte: &[u8]) -> String {
    let mut fuori = String::new();
    for gruppo in byte.chunks(3) {
        let mut pezzi = gruppo.iter().copied();
        let primo = pezzi.next().unwrap_or(0);
        let secondo = pezzi.next();
        let terzo = pezzi.next();
        let impacchettato = (u32::from(primo) << 16)
            | (u32::from(secondo.unwrap_or(0)) << 8)
            | u32::from(terzo.unwrap_or(0));
        // Tre byte fanno quattro caratteri; due ne fanno tre, uno ne fa due. È
        // il conto che rende la codifica reversibile senza riempimento.
        let quanti = match (secondo, terzo) {
            (None, _) => 2,
            (Some(_), None) => 3,
            (Some(_), Some(_)) => 4,
        };
        for spostamento in [18u32, 12, 6, 0].into_iter().take(quanti) {
            let sestina = usize::try_from((impacchettato >> spostamento) & 63).unwrap_or(0);
            if let Some(carattere) = ALFABETO.get(sestina) {
                fuori.push(char::from(*carattere));
            }
        }
    }
    fuori
}

/// Decodifica base64url, con o senza riempimento.
///
/// `None` se compare un carattere che non appartiene all'alfabeto. Serve a
/// leggere il carico di un id token, che Google manda senza `=`.
#[must_use]
pub fn da_base64url(testo: &str) -> Option<Vec<u8>> {
    let mut fuori = Vec::new();
    let mut accumulatore: u32 = 0;
    let mut bit: u32 = 0;
    for carattere in testo.bytes() {
        if carattere == b'=' {
            break;
        }
        let valore = ALFABETO.iter().position(|c| *c == carattere)?;
        accumulatore = (accumulatore << 6) | u32::try_from(valore).unwrap_or(0);
        bit += 6;
        if bit >= 8 {
            bit -= 8;
            fuori.push(u8::try_from((accumulatore >> bit) & 0xFF).unwrap_or(0));
        }
    }
    Some(fuori)
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Le credenziali finte delle prove.
    fn credenziali() -> Credenziali {
        Credenziali {
            client_id: "123.apps.googleusercontent.com".to_owned(),
            client_secret: "GOCSPX-finto".to_owned(),
        }
    }

    #[test]
    fn il_vettore_di_prova_della_rfc_7636() {
        // Il vettore dell'appendice B della RFC 7636. Se questa prova passa,
        // PKCE è calcolato come Google se lo aspetta; se fallisce, il
        // collegamento fallirebbe solo dalla parte di Google, con un messaggio
        // che non dice quale dei due pezzi è sbagliato.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = base64url(Sha256::digest(verifier.as_bytes()).as_slice());
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn base64url_non_usa_ne_piu_ne_barre_ne_riempimento() {
        // I tre byte che producono `+` e `/` in base64 normale. In un indirizzo
        // una barra spezzerebbe il percorso e un più diventerebbe uno spazio.
        let byte = [0xFBu8, 0xFF, 0xBF];
        let codificato = base64url(&byte);
        assert!(!codificato.contains('+'));
        assert!(!codificato.contains('/'));
        assert!(!codificato.contains('='));
        assert_eq!(da_base64url(&codificato), Some(byte.to_vec()));
    }

    #[test]
    fn base64url_fa_andata_e_ritorno_su_ogni_lunghezza() {
        // Le tre code possibili: zero, uno o due byte oltre il gruppo da tre.
        for quanti in 0..12usize {
            let byte: Vec<u8> = (0..quanti)
                .map(|i| u8::try_from(i * 7 % 256).unwrap_or(0))
                .collect();
            assert_eq!(
                da_base64url(&base64url(&byte)),
                Some(byte.clone()),
                "lunghezza {quanti}"
            );
        }
    }

    #[test]
    fn un_carattere_estraneo_non_si_decodifica() {
        assert_eq!(da_base64url("abc!"), None);
        assert_eq!(
            da_base64url("ab+c"),
            None,
            "il più non è di questo alfabeto"
        );
    }

    #[test]
    fn un_verifier_e_lungo_quanto_la_rfc_impone() {
        let pkce = Pkce::nuovo().expect("casualità");
        assert_eq!(pkce.verifier.len(), 43, "il minimo della RFC 7636");
        assert!(
            pkce.verifier
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~')),
            "solo caratteri non riservati: nessuna codifica lo può cambiare"
        );
        // Due chiamate non danno mai la stessa coppia: se la dessero, PKCE non
        // starebbe proteggendo niente.
        let altro = Pkce::nuovo().expect("casualità");
        assert_ne!(pkce.verifier, altro.verifier);
        assert_ne!(pkce.challenge, altro.challenge);
    }

    #[test]
    fn l_indirizzo_di_consenso_ha_tutto_quel_che_serve() {
        let pkce = Pkce::nuovo().expect("casualità");
        let invito = invito(&credenziali(), &pkce, "http://127.0.0.1:54321").expect("invito");

        assert!(invito.url.starts_with(AUTORIZZAZIONE));
        assert!(invito.url.contains("code_challenge_method=S256"));
        // I due che si dimenticano, e senza i quali il backup smette da solo.
        assert!(invito.url.contains("access_type=offline"));
        assert!(invito.url.contains("prompt=consent"));
        assert!(invito.url.contains("response_type=code"));
        assert!(
            invito
                .url
                .contains(&format!("state={}", percento(&invito.state)))
        );
        assert!(
            invito
                .url
                .contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A54321")
        );
        // Lo scope esatto: gli spazi come `%20`, mai come `+`.
        assert!(
            invito.url.contains(
                "scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fdrive.appdata%20openid%20email"
            ),
            "scope sbagliato in: {}",
            invito.url
        );
        // E quel che non ci deve essere: il verifier resta in questo processo.
        assert!(!invito.url.contains(&pkce.verifier));
    }

    #[test]
    fn lo_scope_non_chiede_niente_di_piu_del_necessario() {
        // Una prova sull'*intenzione*: allargare lo scope è una riga sola, e
        // obbligherebbe il progetto a una verifica di Google che nessuno si
        // aspetta finché non arriva.
        assert!(!SCOPE.contains("auth/drive.file"));
        assert!(!SCOPE.split_whitespace().any(|s| s.ends_with("auth/drive")));
    }

    #[test]
    fn una_risposta_senza_access_token_non_e_un_token() {
        let corpo = serde_json::json!({"token_type": "Bearer"});
        let err = interpreta_token(&corpo).unwrap_err();
        assert_eq!(err.code().kind().code(), "net.badSchema");
    }

    #[test]
    fn una_risposta_buona_si_legge_tutta() {
        // Il carico di un id token, com'è: base64url senza riempimento.
        let carico = base64url(br#"{"email":"tizio@example.com","sub":"1"}"#);
        let corpo = serde_json::json!({
            "access_token": "ya29.finto",
            "refresh_token": "1//finto",
            "expires_in": 3599,
            "id_token": format!("intestazione.{carico}.firma"),
        });
        let token = interpreta_token(&corpo).expect("token");
        assert_eq!(token.access_token, "ya29.finto");
        assert_eq!(token.refresh_token.as_deref(), Some("1//finto"));
        assert_eq!(token.email.as_deref(), Some("tizio@example.com"));
        assert!(token.scade_ms > adesso_ms(), "scade nel futuro");
        assert!(ancora_valido(token.scade_ms));
    }

    #[test]
    fn un_id_token_illeggibile_non_fa_fallire_il_collegamento() {
        // L'email è un ornamento delle Impostazioni: non poterla leggere non è
        // una ragione per rifiutare un token che funziona.
        let corpo = serde_json::json!({
            "access_token": "ya29.finto",
            "id_token": "questo non è un jwt",
        });
        let token = interpreta_token(&corpo).expect("token");
        assert_eq!(token.email, None);
    }

    #[test]
    fn un_token_che_scade_fra_poco_e_gia_da_rinfrescare() {
        // Il minuto di anticipo: senza, un token «valido ancora due secondi»
        // supererebbe il controllo e darebbe un 401 a metà caricamento.
        assert!(!ancora_valido(adesso_ms() + 30_000));
        assert!(ancora_valido(adesso_ms() + 120_000));
        assert!(!ancora_valido(adesso_ms() - 1));
    }
}
