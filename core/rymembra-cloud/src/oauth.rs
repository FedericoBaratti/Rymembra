//! OAuth 2.0: PKCE, il consenso nel browser, i token.
//!
//! È nato per Google e per il backup, e lo è ancora in tutto quel che il backup
//! tocca: [`collega`], [`rinfresca`], [`SCOPE`] e i codici `sync.*` non sono
//! cambiati di un byte. Con le cartelle dal cloud i fornitori sono diventati
//! tre — vedi [`Fornitore`] — e le funzioni di Google sono diventate il caso
//! di una tabella invece che l'unico.
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
//! Google. Per questo dalla 3.2 ogni app è **dell'utente** (vedi
//! [`rymembra_domain::nuvola_app`]): nessun client sta nel binario, e un
//! identificativo copiato da un PC consuma la quota di chi l'ha registrato, non
//! quella di tutti.
//!
//! # Cosa non finisce mai in un errore
//!
//! Il `code_verifier`, il codice di autorizzazione, l'access token, il refresh
//! token. Gli [`AppError`] di questo crate arrivano alla finestra dentro
//! `ErroreIpc.cause` e da lì nei registri. Le funzioni qui sotto mettono nella
//! causa il **campo `error` di Google** — `invalid_grant`, `invalid_client` —
//! che è quel che serve a capire cosa è successo e non è un segreto.

use std::time::Duration;

use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::nuvola;
use rymembra_domain::nuvola_app::Piattaforma;
use rymembra_oauth::{Attesa, Ospite, Pkce, Servizio, adesso_ms};

use crate::http::{Corpo, Metodo, Rete, Richiesta, Rimandi, percento};

/// Le parti di OAuth che non sanno con chi stanno parlando.
///
/// Re-esportate da qui perché è da qui che ci si aspetta di trovarle: chi legge
/// `oauth::base64url` in `drive` o `oauth::identificativo` nella finestra non ha
/// bisogno di sapere che quel codice ha traslocato in `rymembra-oauth`. La casa
/// vera è quella, e chi scrive codice nuovo può importarlo di là.
pub use rymembra_oauth::{ancora_valido, base64url, da_base64url, identificativo};

/// Con chi si sta parlando, per gli errori del consenso.
pub const GOOGLE: Servizio = Servizio {
    chiave: "google",
    nome: "Google",
};

/// Dove si manda l'utente a dare il consenso.
const AUTORIZZAZIONE: &str = "https://accounts.google.com/o/oauth2/v2/auth";

/// Dove si scambiano codici e token.
const SCAMBIO: &str = "https://oauth2.googleapis.com/token";

/// Dove si revoca.
const REVOCA: &str = "https://oauth2.googleapis.com/revoke";

/// Quel che Rymembra chiede di poter fare.
///
/// `drive.appdata` è una cartella **privata dell'applicazione**: invisibile
/// nell'interfaccia di Drive, illeggibile da qualunque altra app, e cancellata
/// quando l'utente rimuove i dati di Rymembra dal suo account. È uno scope **non
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

/// Con chi si parla per OneDrive.
pub const MICROSOFT: Servizio = Servizio {
    chiave: "microsoft",
    nome: "Microsoft",
};

/// Con chi si parla per Dropbox.
pub const DROPBOX: Servizio = Servizio {
    chiave: "dropbox",
    nome: "Dropbox",
};

/// Le porte su cui Dropbox può rimandare il browser, nell'ordine in cui si
/// provano.
///
/// Dropbox confronta l'indirizzo di ritorno con quelli registrati nella console
/// dell'app **per intero**, porta compresa: una porta scelta dal sistema non
/// sarebbe mai fra quelle. Tre e non una, perché una porta fissa sul computer di
/// qualcuno può essere già presa. Nella console vanno scritti, esattamente,
/// `http://127.0.0.1:53682`, `http://127.0.0.1:53683` e `http://127.0.0.1:53684`
/// — senza barra finale, che sarebbe un altro indirizzo.
pub const PORTE_DROPBOX: [u16; 3] = [53682, 53683, 53684];

/// A chi vanno gli errori di un fornitore.
///
/// La stessa risposta del fornitore — `invalid_grant` — vuol dire due cose
/// diverse a seconda di chi l'ha chiesta: per il backup è «ricollega Google
/// nelle impostazioni del backup» (`sync.authExpired`), per una cartella di
/// OneDrive è «ricollega quell'account» (`cloud.authExpired`, col servizio
/// nominato). Le frasi sono diverse, e i posti dove si va a rimediare pure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Uso {
    /// Il backup su Drive: i codici `sync.*` di sempre.
    Backup,
    /// Le cartelle dal cloud di questo servizio: i codici `cloud.*`.
    Musica(nuvola::Servizio),
}

/// Un fornitore OAuth: dove si manda l'utente, dove si scambiano i token, cosa
/// si chiede, e come torna il browser.
///
/// # Perché una tabella e non un tratto
///
/// Perché fra Google, Microsoft e Dropbox il protocollo è lo stesso — PKCE, un
/// codice, un modulo — e cambiano solo dei **valori**: gli indirizzi, lo scope,
/// due o tre parametri in più, la forma dell'indirizzo di ritorno. Un tratto
/// avrebbe un metodo per ciascuno di quei valori, cioè sarebbe questa tabella
/// scritta tre volte. Quel che davvero diverge — Google manda il refresh token
/// una volta sola, Microsoft lo ruota a ogni rinfresco — lo assorbe
/// [`rinfresca_presso`], che tiene quel che arriva.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fornitore {
    /// Il nome, negli errori del consenso.
    pub servizio: Servizio,
    /// Dove si manda l'utente a dare il consenso.
    pub autorizzazione: &'static str,
    /// Dove si scambiano codici e token.
    pub scambio: &'static str,
    /// Quel che si chiede.
    pub scope: &'static str,
    /// I parametri dell'invito propri di questo fornitore, in coda a quelli
    /// comuni e nell'ordine dato.
    pub parametri: &'static [(&'static str, &'static str)],
    /// Lo scope va ripetuto anche nel rinfresco: Microsoft lo vuole, e senza
    /// risponderebbe con un token per lo scope di serie.
    pub scope_nel_rinfresco: bool,
    /// Come si nomina questa macchina nell'indirizzo di ritorno.
    pub ospite: Ospite,
    /// Le porte fisse da provare; vuoto vuol dire «la sceglie il sistema».
    pub porte: &'static [u16],
    /// A chi vanno gli errori.
    pub uso: Uso,
}

/// Google per il backup: com'è sempre stato.
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
pub const FORNITORE_GOOGLE: Fornitore = Fornitore {
    servizio: GOOGLE,
    autorizzazione: AUTORIZZAZIONE,
    scambio: SCAMBIO,
    scope: SCOPE,
    parametri: &[("access_type", "offline"), ("prompt", "consent")],
    scope_nel_rinfresco: false,
    ospite: Ospite::Loopback,
    porte: &[],
    uso: Uso::Backup,
};

/// Microsoft, per OneDrive personale.
///
/// - `/consumers`: solo account personali, perché OneDrive for Business non ha
///   né la faccetta `audio` né il delta su una cartella. L'app dell'utente deve
///   quindi ammettere gli account personali, e lo dicono i passi guidati.
/// - Scope: leggere i file e il nome dell'utente; `offline_access` è quel che
///   fa arrivare un refresh token.
/// - Nessun segreto: l'app dell'utente è registrata come client pubblico, e a
///   proteggere lo scambio è PKCE (vedi il `//!`).
/// - Ritorno su `http://localhost` con la porta scelta dal sistema: è la forma
///   che una registrazione «per dispositivi mobili e desktop» accetta, e per
///   `localhost` Entra non guarda la porta. Vedi [`rymembra_oauth::Ospite`].
/// - `prompt=select_account`: senza, un browser già entrato in un account
///   Microsoft darebbe il consenso per quello, e «Collega un altro account»
///   collegherebbe di nuovo lo stesso.
pub const FORNITORE_MICROSOFT: Fornitore = Fornitore {
    servizio: MICROSOFT,
    autorizzazione: "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize",
    scambio: "https://login.microsoftonline.com/consumers/oauth2/v2.0/token",
    scope: "Files.Read User.Read offline_access",
    parametri: &[("response_mode", "query"), ("prompt", "select_account")],
    scope_nel_rinfresco: true,
    ospite: Ospite::Localhost,
    porte: &[],
    uso: Uso::Musica(nuvola::Servizio::OneDrive),
};

/// Dropbox.
///
/// - `token_access_type=offline` è il gemello di `access_type=offline` di
///   Google: senza, arriva solo un access token da quattro ore.
/// - Scope: leggere metadati e contenuti, e il nome dell'account.
/// - Nessun segreto, come per Microsoft.
/// - Ritorno su `127.0.0.1` con le porte fisse di [`PORTE_DROPBOX`], perché
///   Dropbox confronta anche la porta. L'indirizzo e non `localhost`: non c'è
///   niente da risolvere, ed è la forma che la RFC 8252 raccomanda.
pub const FORNITORE_DROPBOX: Fornitore = Fornitore {
    servizio: DROPBOX,
    autorizzazione: "https://www.dropbox.com/oauth2/authorize",
    scambio: "https://api.dropboxapi.com/oauth2/token",
    scope: "files.metadata.read files.content.read account_info.read",
    parametri: &[("token_access_type", "offline")],
    scope_nel_rinfresco: false,
    ospite: Ospite::Loopback,
    porte: &PORTE_DROPBOX,
    uso: Uso::Musica(nuvola::Servizio::Dropbox),
};

/// Google, per le cartelle di musica su Drive.
///
/// Un'app Google dell'utente — di serie la stessa del backup — ma un **altro
/// consenso**: lo scope è
/// `drive.readonly`, per leggere le cartelle dell'utente e non solo la
/// cartella privata dell'app. Stare separati vuol dire che scollegare la
/// musica non spegne il backup, e che lo scope del backup resta quello di
/// sempre ([`SCOPE`], e la prova che lo tiene fermo).
///
/// Separati nel portachiavi, non presso Google: la revoca di un token toglie
/// il consenso dell'app intera per quell'account. Per questo scollegare il
/// backup non revoca niente quando la stessa app serve un conto di Drive.
///
/// `prompt=consent select_account`: il consenso ogni volta, perché Google
/// manda il refresh token solo lì (vedi [`FORNITORE_GOOGLE`]), e la scelta
/// dell'account, perché «Collega un altro account» non ricolleghi lo stesso.
pub const FORNITORE_GOOGLE_MUSICA: Fornitore = Fornitore {
    servizio: GOOGLE,
    autorizzazione: AUTORIZZAZIONE,
    scambio: SCAMBIO,
    scope: SCOPE_MUSICA,
    parametri: &[
        ("access_type", "offline"),
        ("prompt", "consent select_account"),
    ],
    scope_nel_rinfresco: false,
    ospite: Ospite::Loopback,
    porte: &[],
    uso: Uso::Musica(nuvola::Servizio::GoogleDrive),
};

/// Lo scope delle cartelle di musica su Drive: leggere, e solo leggere.
///
/// È uno scope «restricted» per Google: un progetto in stato «Test» ha token
/// che scadono dopo sette giorni, e la guida dice di pubblicarlo.
pub const SCOPE_MUSICA: &str = "https://www.googleapis.com/auth/drive.readonly openid email";

/// Quel che l'utente scrive nella console di una piattaforma perché la sua app
/// funzioni con Rymembra.
///
/// Si ricava dalle **stesse costanti** con cui si parla col fornitore: i passi
/// guidati della finestra mostrano questi valori, e non una copia scritta a
/// mano che un giorno direbbe una porta mentre il codice ne usa un'altra.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registrazione {
    /// Gli indirizzi di ritorno da registrare, esattamente così. Vuoto per
    /// Google: un client «App desktop» accetta il loopback su qualunque porta
    /// senza che lo si scriva.
    pub ritorni: Vec<String>,
    /// I permessi da concedere all'app, uno per voce.
    pub permessi: Vec<&'static str>,
    /// La pagina della console dove si registra.
    pub console: &'static str,
}

/// La registrazione che serve per una piattaforma.
///
/// - **Microsoft:** `http://localhost`, senza porta, perché per `localhost`
///   Entra non la guarda e Rymembra ne sceglie una libera ogni volta. Non
///   `127.0.0.1`: il portale lo accetta in `http` solo dal manifest.
/// - **Dropbox:** le porte di [`PORTE_DROPBOX`], una per riga, perché Dropbox
///   le confronta per intero.
/// - **Google:** gli scope di Drive del backup **e** delle cartelle, perché la
///   stessa app di serie serve a entrambi. `openid` ed `email` li ha ogni app,
///   e non vanno aggiunti.
#[must_use]
pub fn registrazione(piattaforma: Piattaforma) -> Registrazione {
    let ritorni_di = |fornitore: &Fornitore| -> Vec<String> {
        let ospite = fornitore.ospite.nome();
        if fornitore.porte.is_empty() {
            vec![format!("http://{ospite}")]
        } else {
            fornitore
                .porte
                .iter()
                .map(|porta| format!("http://{ospite}:{porta}"))
                .collect()
        }
    };
    let permessi_di = |fornitori: &[&Fornitore]| -> Vec<&'static str> {
        let mut permessi: Vec<&'static str> = Vec::new();
        for fornitore in fornitori {
            for scope in fornitore.scope.split_whitespace() {
                let di_serie = matches!(scope, "openid" | "email");
                if !di_serie && !permessi.contains(&scope) {
                    permessi.push(scope);
                }
            }
        }
        permessi
    };
    match piattaforma {
        Piattaforma::Microsoft => Registrazione {
            ritorni: ritorni_di(&FORNITORE_MICROSOFT),
            permessi: permessi_di(&[&FORNITORE_MICROSOFT]),
            console: "https://entra.microsoft.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade",
        },
        Piattaforma::Dropbox => Registrazione {
            ritorni: ritorni_di(&FORNITORE_DROPBOX),
            permessi: permessi_di(&[&FORNITORE_DROPBOX]),
            console: "https://www.dropbox.com/developers/apps",
        },
        Piattaforma::Google => Registrazione {
            ritorni: Vec::new(),
            permessi: permessi_di(&[&FORNITORE_GOOGLE, &FORNITORE_GOOGLE_MUSICA]),
            console: "https://console.cloud.google.com/apis/credentials",
        },
    }
}

/// Il fornitore OAuth di un servizio delle cartelle dal cloud; `None` per chi
/// non si collega col consenso nel browser.
#[must_use]
pub const fn fornitore_della_musica(servizio: nuvola::Servizio) -> Option<Fornitore> {
    match servizio {
        nuvola::Servizio::OneDrive => Some(FORNITORE_MICROSOFT),
        nuvola::Servizio::GoogleDrive => Some(FORNITORE_GOOGLE_MUSICA),
        nuvola::Servizio::Dropbox => Some(FORNITORE_DROPBOX),
        nuvola::Servizio::WebDav | nuvola::Servizio::Mega | nuvola::Servizio::MegaLink => None,
    }
}

/// Il client OAuth di questa applicazione.
///
/// [`Debug`] scritto a mano: il segreto non è davvero un segreto (vedi il
/// `//!`), ma finisce in un diario solo se qualcuno lo scrive apposta, e un
/// `{:?}` distratto non è «apposta».
#[derive(Clone, PartialEq, Eq)]
pub struct Credenziali {
    /// L'identificativo del client.
    pub client_id: String,
    /// Il segreto del client. Vedi la nota in testa al modulo.
    ///
    /// `None` per i client pubblici — le app Microsoft e Dropbox, registrate
    /// senza segreto — e allora il campo non va nel modulo: mandarne uno vuoto
    /// sarebbe, per quei fornitori, un segreto sbagliato.
    pub client_secret: Option<String>,
}

impl std::fmt::Debug for Credenziali {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credenziali")
            .field("client_id", &self.client_id)
            .field("client_secret", &self.client_secret.as_ref().map(|_| "…"))
            .finish()
    }
}

/// I token appena ricevuti dal fornitore.
///
/// [`Debug`] scritto a mano: un `derive` stamperebbe l'access token e il token
/// di aggiornamento, cioè le due cose con cui chiunque legga un diario
/// potrebbe entrare nella cartella dell'app sul Drive di qualcun altro.
#[derive(Clone, PartialEq, Eq)]
pub struct Token {
    /// Il token con cui si chiamano le API, valido un'ora circa.
    pub access_token: String,
    /// Il token con cui si ottengono altri access token.
    ///
    /// Google lo manda **solo** al primo consenso, e mai nelle risposte di
    /// rinfresco: perderlo significa dover rimandare l'utente alla schermata di
    /// consenso. È il motivo per cui va nel portachiavi appena arriva.
    ///
    /// Microsoft invece lo **ruota**: ogni rinfresco ne porta uno nuovo, e
    /// quello è il refresh token da tenere. Dopo un rinfresco questo campo è
    /// `Some` solo se il fornitore ne ha mandato uno.
    pub refresh_token: Option<String>,
    /// Quando scade l'access token, in millisecondi dall'epoca.
    pub scade_ms: i64,
    /// L'indirizzo email dell'account, se il fornitore l'ha detto.
    pub email: Option<String>,
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Token")
            .field("access_token", &"…")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "‹presente›"),
            )
            .field("scade_ms", &self.scade_ms)
            .field("email", &self.email)
            .finish()
    }
}

/// L'indirizzo a cui mandare l'utente, e lo `state` che dovrà tornare indietro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invito {
    /// L'indirizzo da aprire nel browser di sistema.
    pub url: String,
    /// Il valore che la risposta dovrà riportare identico.
    pub state: String,
}

/// Compone l'indirizzo della schermata di consenso di Google.
///
/// Solo per le prove: il collegamento passa da [`invito_presso`] con
/// [`FORNITORE_GOOGLE`], e questa resta perché le prove di sempre continuino a
/// chiedere l'indirizzo di Google con le parole di sempre.
#[cfg(test)]
fn invito(credenziali: &Credenziali, pkce: &Pkce, redirect_uri: &str) -> Result<Invito, AppError> {
    invito_presso(&FORNITORE_GOOGLE, credenziali, pkce, redirect_uri)
}

/// Compone l'indirizzo della schermata di consenso di un fornitore.
///
/// I parametri comuni nell'ordine di sempre, poi quelli del fornitore: per
/// Google l'indirizzo che esce è lo stesso, carattere per carattere, che si
/// componeva quando Google era l'unico.
///
/// # Errori
///
/// `internal.unexpected` se il sistema non fornisce casualità per lo `state`.
pub(crate) fn invito_presso(
    fornitore: &Fornitore,
    credenziali: &Credenziali,
    pkce: &Pkce,
    redirect_uri: &str,
) -> Result<Invito, AppError> {
    let state = rymembra_oauth::stato()?;
    let mut url = format!(
        "{}\
         ?client_id={}\
         &redirect_uri={}\
         &response_type=code\
         &scope={}\
         &code_challenge={}\
         &code_challenge_method=S256\
         &state={}",
        fornitore.autorizzazione,
        percento(&credenziali.client_id),
        percento(redirect_uri),
        percento(fornitore.scope),
        percento(pkce.challenge()),
        percento(&state),
    );
    for (nome, valore) in fornitore.parametri {
        url.push('&');
        url.push_str(&percento(nome));
        url.push('=');
        url.push_str(&percento(valore));
    }
    Ok(Invito { url, state })
}

/// Scambia il codice di autorizzazione con i token, presso un fornitore.
///
/// # Errori
///
/// Quelli di [`chiedi`]: `sync.*` per il backup, `cloud.*` per la musica,
/// `net.*` per la rete.
pub(crate) fn scambia_presso(
    rete: &Rete,
    fornitore: &Fornitore,
    credenziali: &Credenziali,
    pkce: &Pkce,
    redirect_uri: &str,
    code: &str,
) -> Result<Token, AppError> {
    let mut campi = vec![
        ("grant_type", "authorization_code".to_owned()),
        ("code", code.to_owned()),
        ("client_id", credenziali.client_id.clone()),
    ];
    if let Some(segreto) = &credenziali.client_secret {
        campi.push(("client_secret", segreto.clone()));
    }
    campi.push(("redirect_uri", redirect_uri.to_owned()));
    campi.push(("code_verifier", pkce.verifier().to_owned()));
    interpreta_token_di(
        fornitore.servizio.chiave,
        &chiedi(rete, fornitore, Domanda::Scambio, &campi)?,
    )
}

/// Ottiene un access token nuovo da un refresh token di Google.
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
    rinfresca_presso(rete, &FORNITORE_GOOGLE, credenziali, refresh_token)
}

/// Ottiene un access token nuovo da un refresh token, presso un fornitore.
///
/// # Il refresh token che torna
///
/// Si tiene quel che arriva, e solo quello. Google non rimanda il refresh
/// token quando ne rinfresca uno, e allora [`Token::refresh_token`] resta
/// `None`: chi chiama deve tenersi quello che ha, e azzerarlo vorrebbe dire
/// cancellare dal portachiavi l'unica cosa che non si può riottenere senza
/// l'utente. Microsoft invece lo **ruota** a ogni rinfresco, e il nuovo è
/// quello da salvare: buttarlo via (come si faceva quando Google era l'unico)
/// vorrebbe dire continuare a usare quello vecchio finché Microsoft non smette
/// di accettarlo.
///
/// # Errori
///
/// `sync.authExpired` (backup) o `cloud.authExpired` (musica) se il refresh
/// token non vale più; il resto come [`chiedi`].
pub fn rinfresca_presso(
    rete: &Rete,
    fornitore: &Fornitore,
    credenziali: &Credenziali,
    refresh_token: &str,
) -> Result<Token, AppError> {
    let mut campi = vec![
        ("grant_type", "refresh_token".to_owned()),
        ("refresh_token", refresh_token.to_owned()),
        ("client_id", credenziali.client_id.clone()),
    ];
    if let Some(segreto) = &credenziali.client_secret {
        campi.push(("client_secret", segreto.clone()));
    }
    if fornitore.scope_nel_rinfresco {
        campi.push(("scope", fornitore.scope.to_owned()));
    }
    interpreta_token_di(
        fornitore.servizio.chiave,
        &chiedi(rete, fornitore, Domanda::Rinfresco, &campi)?,
    )
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
    // Senza rimandi, come lo scambio: il corpo è il token da revocare. Vedi
    // [`chiedi`].
    let rete = rete.con_rimandi(Rimandi::Nessuno);
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

/// Il giro completo del collegamento con Google: apre il servitore, invita,
/// aspetta, scambia.
///
/// `apri_browser` riceve l'indirizzo del consenso. Sta fuori da questo crate
/// perché aprire una finestra è compito di chi ha un'interfaccia: qui si
/// vedrebbe soltanto una dipendenza da Tauri in un crate che deve poter girare
/// dentro un esempio da riga di comando.
///
/// `fermare` si chiede a ogni giro dell'attesa: è il tasto «Annulla» accanto a
/// «Aspetto il consenso…». Fino alla 2.4.0 non c'era, e chi apriva il consenso
/// per sbaglio restava tre minuti davanti a un tasto spento.
///
/// # Errori
///
/// `internal.timeout` se l'utente non risponde, `internal.aborted` se nega il
/// consenso o se `fermare` dice di sì, `sync.authExpired` se lo scambio viene
/// rifiutato, `sync.clientInvalid` o `sync.clientUnauthorized` se Google non
/// accetta le credenziali del client, `net.*` per la rete.
pub fn collega(
    rete: &Rete,
    credenziali: &Credenziali,
    apri_browser: impl FnOnce(&str) -> Result<(), AppError>,
    fermare: &dyn Fn() -> bool,
) -> Result<Token, AppError> {
    collega_presso(rete, &FORNITORE_GOOGLE, credenziali, apri_browser, fermare)
}

/// Il giro completo del collegamento con un fornitore.
///
/// Per Google è [`collega`], identico. Per la musica cambiano due cose, tutte e
/// due di parole e non di protocollo:
///
/// - un client senza identificativo si ferma **prima** di aprire il browser,
///   con `cloud.clientMissing`: mandare l'utente a una pagina di Microsoft che
///   dice «app non trovata» è peggio che dirglielo qui;
/// - le risposte del browser che non sono un codice diventano `cloud.*`
///   (vedi [`rifiuto_nel_browser`]).
///
/// # Errori
///
/// Quelli di [`collega`] per il backup. Per la musica: `cloud.clientMissing`
/// senza un'app, `cloud.consentDenied` se nel browser si è detto di no,
/// `cloud.appRejected` se il fornitore non accetta l'app, `cloud.authExpired`
/// se lo scambio viene rifiutato, `internal.timeout`, `internal.aborted` per
/// «Annulla», `net.*`.
pub fn collega_presso(
    rete: &Rete,
    fornitore: &Fornitore,
    credenziali: &Credenziali,
    apri_browser: impl FnOnce(&str) -> Result<(), AppError>,
    fermare: &dyn Fn() -> bool,
) -> Result<Token, AppError> {
    if let Uso::Musica(servizio) = fornitore.uso
        && credenziali.client_id.trim().is_empty()
    {
        return Err(AppError::new(ErrorCode::CloudClientMissing {
            servizio: servizio.nome().to_owned(),
        }));
    }
    // Il servitore si apre per primo: la porta fa parte dell'indirizzo di
    // ritorno, e quell'indirizzo fa parte di ciò che si firma con PKCE.
    let attesa = Attesa::apri_con(fornitore.servizio, fornitore.ospite, fornitore.porte)?;
    let redirect_uri = attesa.redirect_uri();
    let pkce = Pkce::nuovo()?;
    let invito = invito_presso(fornitore, credenziali, &pkce, &redirect_uri)?;

    apri_browser(&invito.url)?;
    let risposta = attesa
        .aspetta(&invito.state, ATTESA_CONSENSO, fermare)
        .map_err(|err| rifiuto_nel_browser(fornitore, err, fermare))?;
    if fermare() {
        // Annullato fra la risposta del browser e lo scambio: chi ha premuto
        // «Annulla» non vuole trovarsi collegato un istante dopo.
        return Err(AppError::new(ErrorCode::InternalAborted {
            what: Some(format!("consenso {}", fornitore.servizio.nome)),
        }));
    }
    scambia_presso(
        rete,
        fornitore,
        credenziali,
        &pkce,
        &redirect_uri,
        &risposta.code,
    )
}

/// Quel che il browser ha risposto al posto di un codice, detto per la musica.
///
/// Il servitore di loopback non sa chi lo usa, e dice tutto con
/// `internal.aborted`: l'«Annulla» di chi guarda, e un `error=…` del fornitore
/// con quel valore nella causa. Per il backup resta così, com'è sempre stato.
/// Per la musica il rifiuto del fornitore ha frasi sue:
///
/// - `access_denied`: si è detto di no, o si è chiuso con «Annulla» sulla
///   pagina del fornitore → `cloud.consentDenied`;
/// - `invalid_client`, `unauthorized_client`: l'app non è registrata come
///   serve (non esiste, è di un altro tipo, non ammette i client pubblici) →
///   `cloud.appRejected`, la frase che manda a ricontrollare l'app.
///
/// L'«Annulla» di Rymembra si riconosce da `fermare`, che a quel punto dice di
/// sì, e resta `internal.aborted`: non è un rifiuto, è un ripensamento.
fn rifiuto_nel_browser(
    fornitore: &Fornitore,
    err: AppError,
    fermare: &dyn Fn() -> bool,
) -> AppError {
    let Uso::Musica(servizio) = fornitore.uso else {
        return err;
    };
    if fermare() || !matches!(err.code(), ErrorCode::InternalAborted { .. }) {
        return err;
    }
    match err.cause() {
        Some("access_denied") => {
            AppError::new(ErrorCode::CloudConsentDenied).with_cause("access_denied")
        }
        Some(motivo @ ("invalid_client" | "unauthorized_client")) => {
            AppError::new(ErrorCode::CloudAppRejected {
                servizio: servizio.nome().to_owned(),
            })
            .with_cause(motivo.to_owned())
        }
        _ => err,
    }
}

/// Manda una richiesta a un punto OAuth e restituisce il JSON della risposta.
///
/// **Non si ritenta.** Un 4xx dal punto di scambio significa che la concessione
/// non vale, e ripeterla darà la stessa risposta; un 5xx su un'azione che
/// l'utente sta guardando è meglio riportarlo subito che farlo aspettare mentre
/// riproviamo in silenzio. Il ritentare sta sulle chiamate alle API, dove
/// l'utente non sta guardando.
///
/// **Non si seguono rimandi.** Il corpo di questa richiesta è il codice di
/// autorizzazione, o il refresh token, più il segreto del client: un `307`
/// lo rimanderebbe identico a chiunque il fornitore — o chi si spacciasse per
/// lui dietro un proxy — nominasse. Il punto di scambio risponde sul posto, e
/// un 3xx da lì è un guasto da dire, non una strada da prendere. La politica si
/// impone qui, e non a chi costruisce la rete: i posti che la costruiscono
/// sono tanti, e il punto di scambio uno per fornitore.
fn chiedi(
    rete: &Rete,
    fornitore: &Fornitore,
    domanda: Domanda,
    campi: &[(&str, String)],
) -> Result<serde_json::Value, AppError> {
    let rete = rete.con_rimandi(Rimandi::Nessuno);
    let risposta = rete.esegui(Richiesta {
        metodo: Metodo::Post,
        url: fornitore.scambio,
        intestazioni: &[],
        corpo: Corpo::Modulo(campi),
    })?;

    let corpo: serde_json::Value = serde_json::from_slice(&risposta.corpo).unwrap_or_default();
    if risposta.e_andata() {
        return Ok(corpo);
    }

    let motivo = corpo
        .get("error")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let descrizione = corpo
        .get("error_description")
        .and_then(serde_json::Value::as_str);
    let rifiuto = match fornitore.uso {
        Uso::Backup => rifiuto_del_punto(motivo, descrizione),
        Uso::Musica(servizio) => rifiuto_per_la_musica(servizio, domanda, motivo, descrizione),
    };
    if let Some(rifiuto) = rifiuto {
        return Err(rifiuto);
    }
    Err(rete.stato_a_errore(&risposta, fornitore.scambio))
}

/// Che cosa si sta chiedendo al punto di scambio.
///
/// La stessa parola del fornitore vuol dire cose diverse nei due momenti: vedi
/// [`rifiuto_per_la_musica`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Domanda {
    /// Il codice appena arrivato dal browser, da scambiare coi token.
    Scambio,
    /// Un refresh token salvato, da scambiare con un access token nuovo.
    Rinfresco,
}

/// Il codice di un rifiuto del punto di scambio, per i tre motivi che hanno
/// un rimedio loro.
///
/// - `invalid_grant`: la concessione non vale più — l'utente ha revocato
///   l'accesso, o il progetto è in «Testing» e Google l'ha scaduta. Si
///   ricollega l'account: `sync.authExpired`.
/// - `invalid_client`: Google non riconosce il client — l'identificativo o il
///   segreto sono sbagliati, o il client è stato cancellato. Ricollegare non
///   serve: si correggono le credenziali. `sync.clientInvalid`.
/// - `unauthorized_client`: il client c'è ma non può fare questo — non è di
///   tipo «App desktop», o il token di aggiornamento l'ha emesso un **altro**
///   client. `sync.clientUnauthorized`.
///
/// Fino alla 2.4.0 gli ultimi due arrivavano come «il server ha risposto con un
/// errore», e il consiglio implicito era riprovare: cioè l'unica cosa che non
/// poteva funzionare. La descrizione di Google va nella causa: non è un
/// segreto, ed è quel che serve a chi legge un diario.
///
/// `None` per tutto il resto, che passa dalla traduzione normale dello stato.
fn rifiuto_del_punto(motivo: &str, descrizione: Option<&str>) -> Option<AppError> {
    let codice = match motivo {
        "invalid_grant" => ErrorCode::SyncAuthExpired,
        "invalid_client" => ErrorCode::SyncClientInvalid,
        "unauthorized_client" => ErrorCode::SyncClientUnauthorized,
        _ => return None,
    };
    let causa = descrizione.map_or_else(|| motivo.to_owned(), |d| format!("{motivo}: {d}"));
    Some(AppError::new(codice).with_cause(causa))
}

/// Come [`rifiuto_del_punto`], per le cartelle dal cloud.
///
/// - `invalid_grant`, e i due di Microsoft che vogliono lo stesso rimedio
///   (`interaction_required`, `consent_required`: il consenso va ridato) →
///   `cloud.authExpired`, «ricollega l'account»;
/// - `invalid_client` → `cloud.appRejected`: l'app non esiste o il segreto
///   non è il suo, e ricollegare non serve finché non si corregge;
/// - `unauthorized_client` dipende dal momento. Nello **scambio** del codice è
///   l'app registrata male (per Microsoft, di solito, «Consenti flussi client
///   pubblici» lasciato a «No») → `cloud.appRejected`. Nel **rinfresco** è il
///   caso di Google di un refresh token emesso per un altro client — un conto
///   collegato con un'app che non è più quella — e il rimedio è ricollegare →
///   `cloud.authExpired`.
///
/// Il servizio viaggia nel codice, per il diario; le frasi non lo ripetono.
fn rifiuto_per_la_musica(
    servizio: nuvola::Servizio,
    domanda: Domanda,
    motivo: &str,
    descrizione: Option<&str>,
) -> Option<AppError> {
    let nome = servizio.nome().to_owned();
    let codice = match (motivo, domanda) {
        ("invalid_grant" | "interaction_required" | "consent_required", _)
        | ("unauthorized_client", Domanda::Rinfresco) => {
            ErrorCode::CloudAuthExpired { servizio: nome }
        }
        ("invalid_client", _) | ("unauthorized_client", Domanda::Scambio) => {
            ErrorCode::CloudAppRejected { servizio: nome }
        }
        _ => return None,
    };
    let causa = descrizione.map_or_else(|| motivo.to_owned(), |d| format!("{motivo}: {d}"));
    Some(AppError::new(codice).with_cause(causa))
}

/// Legge la risposta del punto OAuth di Google.
#[cfg(test)]
fn interpreta_token(corpo: &serde_json::Value) -> Result<Token, AppError> {
    interpreta_token_di(GOOGLE.chiave, corpo)
}

/// Legge la risposta di un punto OAuth; `chiave` nomina il fornitore se la
/// risposta non ha la forma attesa.
fn interpreta_token_di(chiave: &str, corpo: &serde_json::Value) -> Result<Token, AppError> {
    let access_token = corpo
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            AppError::new(ErrorCode::NetBadSchema {
                service: Some(chiave.to_owned()),
                detail: Some("la risposta non contiene un access token".to_owned()),
            })
        })?
        .to_owned();

    // `expires_in` è in secondi e i tre fornitori lo mandano sempre; l'ora di
    // serie è quella che Google usa comunque, e vale come rete di sicurezza.
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

#[cfg(test)]
mod prove {
    use super::*;

    /// Le credenziali finte delle prove.
    fn credenziali() -> Credenziali {
        Credenziali {
            client_id: "123.apps.googleusercontent.com".to_owned(),
            client_secret: Some("GOCSPX-finto".to_owned()),
        }
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
        assert!(!invito.url.contains(pkce.verifier()));
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
    fn i_rifiuti_del_client_hanno_i_loro_codici() {
        let codice = |motivo: &str| {
            rifiuto_del_punto(motivo, Some("descrizione di Google"))
                .map(|err| err.code().kind().code())
        };
        assert_eq!(codice("invalid_grant"), Some("sync.authExpired"));
        assert_eq!(codice("invalid_client"), Some("sync.clientInvalid"));
        assert_eq!(
            codice("unauthorized_client"),
            Some("sync.clientUnauthorized")
        );
        assert_eq!(
            codice("invalid_request"),
            None,
            "il resto passa dalla traduzione normale"
        );
        let err = rifiuto_del_punto("invalid_client", Some("The OAuth client was not found."))
            .expect("un rifiuto");
        assert!(
            !err.is_retryable(),
            "credenziali sbagliate non si aggiustano ritentando"
        );
        assert!(
            err.cause().is_some_and(|c| c.contains("not found")),
            "la descrizione di Google serve a chi legge il diario"
        );
    }

    #[test]
    fn i_segreti_non_compaiono_nel_debug() {
        let token = Token {
            access_token: "ya29.segretissimo".to_owned(),
            refresh_token: Some("1//anche-questo".to_owned()),
            scade_ms: 1,
            email: Some("tizio@example.com".to_owned()),
        };
        let scritto = format!("{token:?}");
        assert!(!scritto.contains("segretissimo"));
        assert!(!scritto.contains("anche-questo"));
        assert!(
            scritto.contains("tizio@example.com"),
            "l'email non è un segreto"
        );

        let scritto = format!("{:?}", credenziali());
        assert!(!scritto.contains("GOCSPX-finto"));
        assert!(scritto.contains("123.apps.googleusercontent.com"));
    }

    #[test]
    fn il_consenso_si_aspetta_a_nome_di_google() {
        // `Attesa` ora serve due fornitori: questa prova è ciò che impedisce a
        // un errore del collegamento a Drive di dire «consenso Spotify».
        assert_eq!(GOOGLE.chiave, "google");
        assert_eq!(GOOGLE.nome, "Google");
    }
    // ── i fornitori della musica ────────────────────────────────────────────

    use crate::finto::{Finto, Vista, json};

    /// Un client pubblico, come quelli di Microsoft e Dropbox.
    fn pubblico() -> Credenziali {
        Credenziali {
            client_id: "00000000-app-id".to_owned(),
            client_secret: None,
        }
    }

    /// Un fornitore uguale a `base`, ma che scambia i token col servitore
    /// finto. L'indirizzo deve vivere quanto il programma: nelle prove lo si
    /// lascia andare apposta.
    fn presso(base: Fornitore, finto: &Finto) -> Fornitore {
        let scambio: &'static str = format!("{}/token", finto.base).leak();
        Fornitore { scambio, ..base }
    }

    #[test]
    fn l_invito_di_google_e_identico_a_prima() {
        // L'indirizzo come lo componeva la 3.1.0, quando Google era l'unico:
        // la tabella dei fornitori non deve averne cambiato un carattere.
        let pkce = Pkce::nuovo().expect("casualità");
        let fatto = invito(&credenziali(), &pkce, "http://127.0.0.1:54321").expect("invito");
        let atteso = format!(
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
            percento(&credenziali().client_id),
            percento("http://127.0.0.1:54321"),
            percento(SCOPE),
            percento(pkce.challenge()),
            percento(&fatto.state),
        );
        assert_eq!(fatto.url, atteso);
        assert_eq!(
            FORNITORE_GOOGLE.scope, SCOPE,
            "lo scope del backup è quello"
        );
        assert_eq!(FORNITORE_GOOGLE.uso, Uso::Backup);
        assert_eq!(FORNITORE_GOOGLE.ospite, Ospite::Loopback);
        assert!(
            FORNITORE_GOOGLE.porte.is_empty(),
            "porta scelta dal sistema"
        );
    }

    #[test]
    fn la_registrazione_dice_quel_che_il_codice_usa() {
        // I passi guidati mostrano questi valori: devono essere quelli con cui
        // si parla davvero col fornitore.
        let microsoft = registrazione(Piattaforma::Microsoft);
        assert_eq!(microsoft.ritorni, ["http://localhost"]);
        assert_eq!(
            microsoft.permessi,
            ["Files.Read", "User.Read", "offline_access"]
        );
        let dropbox = registrazione(Piattaforma::Dropbox);
        assert_eq!(
            dropbox.ritorni,
            [
                "http://127.0.0.1:53682",
                "http://127.0.0.1:53683",
                "http://127.0.0.1:53684"
            ]
        );
        assert_eq!(
            dropbox.permessi,
            FORNITORE_DROPBOX
                .scope
                .split_whitespace()
                .collect::<Vec<_>>()
        );
        let google = registrazione(Piattaforma::Google);
        assert!(google.ritorni.is_empty(), "un client desktop non ne vuole");
        assert_eq!(
            google.permessi,
            [
                "https://www.googleapis.com/auth/drive.appdata",
                "https://www.googleapis.com/auth/drive.readonly"
            ],
            "backup e cartelle con la stessa app"
        );
        for piattaforma in Piattaforma::tutte() {
            let r = registrazione(piattaforma);
            assert!(r.console.starts_with("https://"), "{piattaforma:?}");
            assert!(
                !r.permessi
                    .contains(&"https://www.googleapis.com/auth/drive"),
                "mai il Drive intero in scrittura"
            );
            for ritorno in &r.ritorni {
                assert!(
                    !ritorno.ends_with('/'),
                    "la barra finale è un altro indirizzo"
                );
            }
        }
    }

    #[test]
    fn ogni_servizio_col_browser_ha_il_suo_fornitore() {
        for servizio in nuvola::Servizio::tutti() {
            let fornitore = fornitore_della_musica(servizio);
            assert_eq!(fornitore.is_some(), servizio.piattaforma().is_some());
            if let Some(fornitore) = fornitore {
                assert_eq!(fornitore.uso, Uso::Musica(servizio));
            }
        }
    }

    #[test]
    fn l_invito_di_microsoft_chiede_onedrive_in_lettura() {
        let pkce = Pkce::nuovo().expect("casualità");
        let invito = invito_presso(
            &FORNITORE_MICROSOFT,
            &pubblico(),
            &pkce,
            "http://localhost:50000",
        )
        .expect("invito");
        assert!(invito.url.starts_with(
            "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize?client_id="
        ));
        assert!(
            invito
                .url
                .contains("scope=Files.Read%20User.Read%20offline_access"),
            "{}",
            invito.url
        );
        assert!(
            invito
                .url
                .contains("redirect_uri=http%3A%2F%2Flocalhost%3A50000")
        );
        assert!(invito.url.contains("code_challenge_method=S256"));
        assert!(invito.url.contains("response_mode=query"));
        assert!(invito.url.contains("prompt=select_account"));
        assert!(
            !invito.url.contains("access_type"),
            "è un parametro di Google: a Microsoft non dice niente"
        );
        assert_eq!(FORNITORE_MICROSOFT.ospite, Ospite::Localhost);
        assert!(FORNITORE_MICROSOFT.porte.is_empty());
    }

    #[test]
    fn l_invito_di_dropbox_chiede_il_token_che_dura() {
        let pkce = Pkce::nuovo().expect("casualità");
        let invito = invito_presso(
            &FORNITORE_DROPBOX,
            &pubblico(),
            &pkce,
            "http://127.0.0.1:53682",
        )
        .expect("invito");
        assert!(
            invito
                .url
                .starts_with("https://www.dropbox.com/oauth2/authorize?")
        );
        assert!(invito.url.contains("token_access_type=offline"));
        assert!(
            invito
                .url
                .contains("scope=files.metadata.read%20files.content.read%20account_info.read")
        );
        assert_eq!(FORNITORE_DROPBOX.porte, &[53682, 53683, 53684]);
        assert_eq!(FORNITORE_DROPBOX.ospite, Ospite::Loopback);
    }

    #[test]
    fn nessuno_scope_della_musica_chiede_di_scrivere() {
        // Rymembra legge e basta: uno scope che scrive è una riga, e
        // cambierebbe quel che la schermata di consenso chiede alla gente.
        for fornitore in [FORNITORE_MICROSOFT, FORNITORE_DROPBOX] {
            for scope in fornitore.scope.split_whitespace() {
                assert!(!scope.to_ascii_lowercase().contains("write"), "{scope}");
            }
        }
    }

    /// Un punto di scambio finto che risponde sempre con `stato` e `corpo`.
    fn punto(stato: u16, corpo: serde_json::Value) -> Finto {
        Finto::avvia(move |_: &Vista, _: &str| json(stato, &corpo))
    }

    #[test]
    fn un_client_pubblico_non_manda_il_segreto() {
        let finto = punto(
            200,
            serde_json::json!({"access_token": "a", "refresh_token": "r", "expires_in": 3600}),
        );
        let fornitore = presso(FORNITORE_MICROSOFT, &finto);
        let pkce = Pkce::nuovo().expect("casualità");
        let token = scambia_presso(
            &finto.rete(),
            &fornitore,
            &pubblico(),
            &pkce,
            "http://localhost:5",
            "codice",
        )
        .expect("token");
        assert_eq!(token.refresh_token.as_deref(), Some("r"));
        let vista = finto.viste().remove(0);
        assert_eq!(vista.metodo, "POST", "un modulo, mai nell'indirizzo");
        assert_eq!(vista.campo("client_secret"), None, "{}", vista.corpo);
        assert_eq!(vista.campo("code").as_deref(), Some("codice"));
        assert_eq!(
            vista.campo("code_verifier").as_deref(),
            Some(pkce.verifier())
        );
        assert_eq!(
            vista.campo("redirect_uri").as_deref(),
            Some("http://localhost:5")
        );
    }

    #[test]
    fn google_manda_ancora_il_suo_segreto() {
        let finto = punto(200, serde_json::json!({"access_token": "a"}));
        let fornitore = presso(FORNITORE_GOOGLE, &finto);
        rinfresca_presso(&finto.rete(), &fornitore, &credenziali(), "1//vecchio").expect("token");
        let vista = finto.viste().remove(0);
        assert_eq!(
            vista.campo("client_secret").as_deref(),
            Some("GOCSPX-finto")
        );
        assert_eq!(vista.campo("grant_type").as_deref(), Some("refresh_token"));
        assert_eq!(
            vista.campo("scope"),
            None,
            "Google non lo vuole nel rinfresco"
        );
    }

    #[test]
    fn il_refresh_token_ruotato_torna_a_chi_ha_rinfrescato() {
        // Microsoft ne manda uno nuovo a ogni rinfresco: buttarlo via, come
        // si faceva quando Google era l'unico, vorrebbe dire continuare con
        // quello vecchio finché smette di valere.
        let finto = punto(
            200,
            serde_json::json!({"access_token": "nuovo", "refresh_token": "ruotato", "expires_in": 3600}),
        );
        let fornitore = presso(FORNITORE_MICROSOFT, &finto);
        let token =
            rinfresca_presso(&finto.rete(), &fornitore, &pubblico(), "vecchio").expect("token");
        assert_eq!(token.access_token, "nuovo");
        assert_eq!(token.refresh_token.as_deref(), Some("ruotato"));
        let vista = finto.viste().remove(0);
        assert_eq!(vista.campo("refresh_token").as_deref(), Some("vecchio"));
        assert_eq!(
            vista.campo("scope").as_deref(),
            Some("Files.Read User.Read offline_access"),
            "Microsoft vuole lo scope anche nel rinfresco"
        );
    }

    #[test]
    fn senza_rotazione_il_refresh_token_resta_quello_di_chi_chiama() {
        let finto = punto(200, serde_json::json!({"access_token": "nuovo"}));
        let fornitore = presso(FORNITORE_DROPBOX, &finto);
        let token =
            rinfresca_presso(&finto.rete(), &fornitore, &pubblico(), "vecchio").expect("token");
        assert_eq!(
            token.refresh_token, None,
            "niente di nuovo: chi chiama si tiene il suo"
        );
        assert_eq!(finto.viste().remove(0).campo("scope"), None);
    }

    #[test]
    fn i_rifiuti_della_musica_parlano_di_cloud() {
        let caso = |base: Fornitore, motivo: &str| {
            let finto = punto(
                400,
                serde_json::json!({"error": motivo, "error_description": "AADSTS70000: scaduto"}),
            );
            let fornitore = presso(base, &finto);
            rinfresca_presso(&finto.rete(), &fornitore, &pubblico(), "vecchio")
                .expect_err("rifiutato")
        };

        let scaduto = caso(FORNITORE_MICROSOFT, "invalid_grant");
        assert_eq!(
            scaduto.code(),
            &ErrorCode::CloudAuthExpired {
                servizio: "OneDrive".to_owned()
            }
        );
        assert!(scaduto.cause().is_some_and(|c| c.contains("AADSTS70000")));
        let ridare = caso(FORNITORE_MICROSOFT, "interaction_required");
        assert_eq!(ridare.code().kind().code(), "cloud.authExpired");
        let client = caso(FORNITORE_DROPBOX, "invalid_client");
        assert_eq!(
            client.code(),
            &ErrorCode::CloudAppRejected {
                servizio: "Dropbox".to_owned()
            }
        );
        // Nel rinfresco `unauthorized_client` è un token di un altro client:
        // si ricollega, l'app va bene.
        let altro_client = caso(FORNITORE_GOOGLE_MUSICA, "unauthorized_client");
        assert_eq!(altro_client.code().kind().code(), "cloud.authExpired");
        let altro = caso(FORNITORE_DROPBOX, "invalid_request");
        assert_eq!(
            altro.code().kind().code(),
            "net.http",
            "il resto passa dalla traduzione normale"
        );

        // E il backup resta com'era.
        let backup = caso(FORNITORE_GOOGLE, "invalid_grant");
        assert_eq!(backup.code().kind().code(), "sync.authExpired");
    }

    /// Il browser finto: legge dall'invito la porta di ritorno e lo `state`,
    /// e bussa riportando `riporta`.
    fn browser(
        ritorno_atteso: &'static str,
        riporta: &'static str,
    ) -> impl FnOnce(&str) -> Result<(), AppError> {
        move |url: &str| {
            let valore = |nome: &str| {
                url.split(['?', '&'])
                    .find_map(|c| c.strip_prefix(&format!("{nome}=")))
                    .map(ToOwned::to_owned)
                    .unwrap_or_default()
            };
            let state = valore("state");
            let ritorno = valore("redirect_uri")
                .replace("%3A", ":")
                .replace("%2F", "/");
            assert!(ritorno.starts_with(ritorno_atteso), "{ritorno}");
            let porta: u16 = ritorno
                .rsplit(':')
                .next()
                .and_then(|p| p.parse().ok())
                .expect("una porta");
            std::thread::spawn(move || {
                let mut flusso =
                    std::net::TcpStream::connect(("127.0.0.1", porta)).expect("connessione");
                let richiesta =
                    format!("GET /?{riporta}&state={state} HTTP/1.1\r\nHost: x\r\n\r\n");
                let _ = std::io::Write::write_all(&mut flusso, richiesta.as_bytes());
                let mut fine = String::new();
                let _ = std::io::Read::read_to_string(&mut flusso, &mut fine);
            });
            Ok(())
        }
    }

    #[test]
    fn il_collegamento_con_microsoft_va_e_torna_su_localhost() {
        let finto = punto(
            200,
            serde_json::json!({"access_token": "a", "refresh_token": "r", "expires_in": 3600}),
        );
        let fornitore = presso(FORNITORE_MICROSOFT, &finto);
        let token = collega_presso(
            &finto.rete(),
            &fornitore,
            &pubblico(),
            browser("http://localhost:", "code=c%2F1"),
            &|| false,
        )
        .expect("collegato");
        assert_eq!(token.refresh_token.as_deref(), Some("r"));
        let vista = finto.viste().remove(0);
        assert_eq!(vista.campo("code").as_deref(), Some("c/1"));
        assert!(
            vista
                .campo("redirect_uri")
                .is_some_and(|r| r.starts_with("http://localhost:")),
            "lo scambio ripete l'indirizzo dell'invito, parola per parola"
        );
    }

    #[test]
    fn un_no_nel_browser_e_un_consenso_negato() {
        let finto = punto(200, serde_json::json!({"access_token": "a"}));
        let fornitore = presso(FORNITORE_MICROSOFT, &finto);
        let err = collega_presso(
            &finto.rete(),
            &fornitore,
            &pubblico(),
            browser("http://localhost:", "error=access_denied"),
            &|| false,
        )
        .expect_err("negato");
        assert_eq!(err.code(), &ErrorCode::CloudConsentDenied);
        assert!(finto.viste().is_empty(), "nessuno scambio dopo un no");
    }

    #[test]
    fn un_app_non_ammessa_nel_browser_e_un_app_rifiutata() {
        let finto = punto(200, serde_json::json!({"access_token": "a"}));
        let fornitore = presso(FORNITORE_MICROSOFT, &finto);
        let err = collega_presso(
            &finto.rete(),
            &fornitore,
            &pubblico(),
            browser("http://localhost:", "error=unauthorized_client"),
            &|| false,
        )
        .expect_err("rifiutata");
        assert_eq!(err.code().kind().code(), "cloud.appRejected");
    }

    #[test]
    fn senza_identificativo_non_si_apre_nemmeno_il_browser() {
        let aperto = std::sync::atomic::AtomicBool::new(false);
        let err = collega_presso(
            &Rete::nuova("prova", SCADENZA),
            &FORNITORE_DROPBOX,
            &Credenziali {
                client_id: "  ".to_owned(),
                client_secret: None,
            },
            |_| {
                aperto.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            },
            &|| false,
        )
        .expect_err("manca l'app");
        assert_eq!(
            err.code(),
            &ErrorCode::CloudClientMissing {
                servizio: "Dropbox".to_owned()
            }
        );
        assert!(!aperto.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn l_annulla_di_rymembra_resta_un_annullo() {
        // «Annulla» premuto mentre si aspetta: non è un consenso negato, e la
        // finestra non deve dirlo come tale.
        let err = rifiuto_nel_browser(
            &FORNITORE_MICROSOFT,
            AppError::new(ErrorCode::InternalAborted { what: None }),
            &|| true,
        );
        assert_eq!(err.code().kind().code(), "internal.aborted");
        // E per il backup il rifiuto nel browser resta quello di sempre.
        let err = rifiuto_nel_browser(
            &FORNITORE_GOOGLE,
            AppError::new(ErrorCode::InternalAborted { what: None }).with_cause("access_denied"),
            &|| false,
        );
        assert_eq!(err.code().kind().code(), "internal.aborted");
    }
}
