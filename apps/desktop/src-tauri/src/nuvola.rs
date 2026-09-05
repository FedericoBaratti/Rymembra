//! Il backup su Drive, dal lato della finestra.
//!
//! Otto comandi, due eventi e un filo di sottofondo. Le decisioni stanno in
//! `aether-domain`, il formato in `aether-app`, la rete in `aether-cloud`: qui
//! si tiene il tempo e si prende il lucchetto — due cose che nessuno di quei tre
//! sa fare, e che sono anche le due che possono far impuntare la riproduzione.
//!
//! # L'ordine dei lucchetti
//!
//! Come `riproduzione` dichiara il suo, questo modulo dichiara il proprio:
//!
//! > **Il filo della nuvola non prende mai il lucchetto del lettore, e prende
//! > quello della libreria solo in due finestre brevi.**
//!
//! Le due finestre sono: leggere lo stato da salvare (millisecondi su
//! millequattrocento righe) e riscrivere le tre chiavi `nuvola.*` alla fine. Tutto
//! quel che sta in mezzo — impronte, gzip, `files.list`, i caricamenti — dura da
//! secondi a minuti e avviene **senza nessun lucchetto**.
//!
//! Che sia vero non è affidato all'attenzione di chi legge: `aether-cloud` non
//! riceve mai una `rusqlite::Connection`, quindi il codice che terrebbe il
//! lucchetto durante una richiesta di rete non si può nemmeno scrivere.
//!
//! # Il segreto del client
//!
//! `option_env!` e non `env!`: una copia del repo senza credenziali deve
//! **compilare**, e limitarsi a dire che il backup non è configurato. Altrimenti
//! clonare il progetto e lanciare `cargo test` diventa impossibile per chiunque
//! non abbia un progetto Google.
//!
//! Le due costanti stanno **qui** e non in `aether-cloud` perché `option_env!`
//! legge l'ambiente del crate che si sta compilando. È anche la stratificazione
//! giusta: `aether-cloud` riceve delle `Credenziali` e resta provabile.
//!
//! Da scrivere perché nessuno «indurisca» la cosa sbagliata: **un client secret
//! dentro un binario desktop non è un segreto**. `strings` lo trova, e la
//! documentazione di Google per le installed app lo dice. Quel che protegge lo
//! scambio è PKCE. L'esposizione vera è che un terzo consumi la quota del
//! progetto; le mitigazioni sono strutturali e stanno nello scope.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use crate::spegnimento::Emette as _;
use aether_app::backup::{self, StatoLocale};
use aether_app::settings;
use aether_cloud::drive::{self, Drive};
use aether_cloud::http::Rete;
use aether_cloud::oauth::{self, Credenziali, Token};
use aether_cloud::pacchetti;
use aether_cloud::portachiavi::{self, DiSistema, Portachiavi as _};
use aether_cloud::servizio::{self, DaRipristinare};
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::paths::PathRules;
use aether_domain::restore::{RestoreInput, RestorePlan, plan_restore};
use serde::Serialize;
use tauri::{AppHandle, Manager as _, State};

use crate::errore::{ErroreIpc, Esito, errore};
use crate::nota;
use crate::stato::{Stato, Turno, adesso_ms, con_libreria};

// ── le credenziali compilate dentro ─────────────────────────────────────────

/// L'identificativo del client OAuth, se chi ha compilato l'ha fornito.
const CLIENT_ID_COMPILATO: Option<&str> = option_env!("AETHER_GOOGLE_CLIENT_ID");

/// Il segreto del client OAuth. Vedi la nota in testa al modulo.
const CLIENT_SECRET_COMPILATO: Option<&str> = option_env!("AETHER_GOOGLE_CLIENT_SECRET");

// ── le chiavi in `settings` ─────────────────────────────────────────────────
//
// Nessuna di queste è un segreto: il token di aggiornamento sta nel portachiavi
// di sistema, e il motivo è scritto in `aether_cloud::portachiavi`.

/// Il backup automatico è acceso.
const CHIAVE_ATTIVO: &str = "nuvola.attivo";
/// L'identificativo del client, quando l'utente l'ha scritto a mano.
const CHIAVE_CLIENT_ID: &str = "nuvola.client_id";
/// L'email dell'account collegato, per mostrarla nelle Impostazioni.
const CHIAVE_EMAIL: &str = "nuvola.email";
/// L'identificativo su Drive del file dei metadati.
const CHIAVE_FILE_ID: &str = "nuvola.file_id";
/// L'impronta dell'ultimo contenuto caricato.
const CHIAVE_IMPRONTA: &str = "nuvola.impronta";
/// Quando è riuscita l'ultima passata.
const CHIAVE_ULTIMO: &str = "nuvola.ultimo_ms";
/// L'identificativo casuale di questo computer.
const CHIAVE_DISPOSITIVO: &str = "nuvola.dispositivo";

// ── i tempi del filo ────────────────────────────────────────────────────────

/// Ogni quanto si salva, quando non succede niente.
const INTERVALLO: Duration = Duration::from_secs(15 * 60);

/// Quanto si aspetta prima della prima passata.
///
/// All'avvio può esserci una scansione davanti, e una passata di backup mentre
/// la libreria sta cambiando sotto salverebbe uno stato intermedio — corretto ma
/// inutile, e pagato in banda.
const ATTESA_AVVIO: Duration = Duration::from_secs(30);

/// Quanto si aspetta che una raffica di modifiche finisca.
///
/// Chi mette dieci cuoricini di fila produce dieci segnali. Senza questa attesa
/// produrrebbe dieci passate; con, ne produce una.
const RAFFICA: Duration = Duration::from_secs(2 * 60);

/// La scadenza dei trasferimenti verso Drive.
///
/// Due minuti, non i quindici secondi dei punti OAuth: qui può passare una skin
/// da venti megabyte su una connessione lenta.
const SCADENZA_DRIVE: Duration = Duration::from_secs(120);

/// Quanti brani del piano si mandano alla finestra con il delta per esteso.
///
/// L'anteprima ne mostra un elenco; millequattrocento righe di «ascolti 3 → 17»
/// non le legge nessuno e costano un messaggio IPC da qualche megabyte. Il
/// **numero** totale c'è comunque.
const QUANTI_MOSTRARNE: usize = 500;

// ── lo stato condiviso ──────────────────────────────────────────────────────

/// Perché il filo si è svegliato.
///
/// Pubblico solo perché compare nel tipo del canale che `main.rs` passa a
/// [`avvia_filo`]. Nessuno fuori da questo modulo ne costruisce uno.
///
/// # Non c'è un «chiudi»
///
/// Il filo si ferma quando il canale si chiude, cioè quando [`StatoNuvola`]
/// viene lasciato cadere insieme al resto dello stato dell'applicazione. Un
/// terzo caso che nessuno manda mai sarebbe una riga che dice il falso — e
/// mandarlo davvero non aggiungerebbe niente: le due finestre di lucchetto sono
/// transazionali, quindi una passata interrotta a metà non lascia niente di
/// scritto per metà. Al massimo la successiva ricarica un file che era già
/// lassù.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sveglia {
    /// Qualcosa è cambiato: aspetta che la raffica finisca.
    Sporca,
    /// Qualcuno ha chiesto di salvare adesso.
    Subito,
}

/// Quel che il backup tiene aperto mentre l'applicazione gira.
pub struct StatoNuvola {
    /// Come si sveglia il filo.
    sveglia: Sender<Sveglia>,

    /// C'è già un'operazione in corso.
    ///
    /// # Perché **non** sta dietro un mutex
    ///
    /// Stessa ragione di `scansione_da_fermare` in `stato.rs`: una passata
    /// automatica tiene occupato il filo per minuti, e un comando `nuvola_stato`
    /// che dovesse chiedere lo stesso lucchetto per sapere se c'è una passata in
    /// corso resterebbe in coda dietro la passata di cui vuole parlare.
    in_corso: AtomicBool,

    /// L'esito dell'ultima passata automatica.
    ///
    /// `None` vuol dire riuscita. Vive qui e non in `settings` perché è una cosa
    /// di questa sessione: un errore di rete di ieri non è un'informazione utile
    /// oggi, e scriverlo su disco lo farebbe sopravvivere al riavvio che quasi
    /// sempre lo risolve.
    ultimo_errore: Mutex<Option<ErroreIpc>>,

    /// L'access token in corso di validità.
    ///
    /// Un token dura un'ora e una passata avviene ogni quarto d'ora: senza
    /// questa cache si chiederebbero a Google quattro rinfreschi all'ora per
    /// niente.
    token: Mutex<Option<Token>>,
}

impl StatoNuvola {
    /// Costruisce lo stato e il capo del canale che il filo dovrà ascoltare.
    #[must_use]
    pub fn nuovo() -> (Self, Receiver<Sveglia>) {
        let (sveglia, orecchio) = channel();
        (
            Self {
                sveglia,
                in_corso: AtomicBool::new(false),
                ultimo_errore: Mutex::new(None),
                token: Mutex::new(None),
            },
            orecchio,
        )
    }
}

/// Un'operazione è già in corso.
fn occupato() -> AppError {
    AppError::new(ErrorCode::SyncBusy)
}

// ── quel che la finestra vede ───────────────────────────────────────────────

/// Lo stato del backup, come lo mostrano le Impostazioni.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoNuvolaIpc {
    /// Ci sono delle credenziali del client: compilate dentro o scritte a mano.
    pub configurato: bool,
    /// C'è un account collegato.
    pub collegato: bool,
    /// Il backup automatico è acceso.
    pub attivo: bool,
    /// L'email dell'account, se la conosciamo.
    pub email: Option<String>,
    /// Quando è riuscita l'ultima passata.
    pub ultimo_ms: Option<i64>,
    /// C'è un'operazione in corso adesso.
    pub in_corso: bool,
    /// Com'è andata l'ultima passata automatica.
    pub errore: Option<ErroreIpc>,
}

/// Un brano che il ripristino cambierebbe.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CambioBranoIpc {
    /// L'interprete, dalla chiave.
    pub artista: String,
    /// Il titolo, dalla chiave.
    pub titolo: String,
    /// L'album, dalla chiave.
    pub album: String,
    /// Gli ascolti adesso.
    pub ascolti_prima: i64,
    /// Gli ascolti dopo.
    pub ascolti_dopo: i64,
    /// Il voto adesso.
    pub voto_prima: u8,
    /// Il voto dopo.
    pub voto_dopo: u8,
    /// Diventerebbe un preferito.
    pub preferito_dopo: bool,
}

/// Una playlist che il ripristino scriverebbe.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CambioPlaylistIpc {
    /// Il nome.
    pub nome: String,
    /// Non esiste ancora qui.
    pub da_creare: bool,
    /// È automatica: riceve le regole, mai l'appartenenza.
    pub automatica: bool,
    /// Quanti dei suoi brani esistono qui.
    pub brani_qui: usize,
    /// Quanti ne aveva nel backup.
    pub brani_nel_backup: usize,
}

/// Una cartella sorvegliata che il ripristino aggiungerebbe.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CartellaIpc {
    /// Il percorso.
    pub percorso: String,
    /// Esiste ancora su questo disco.
    pub esiste: bool,
}

/// Quel che un ripristino farebbe.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PianoRipristinoIpc {
    /// Su Drive c'è un backup da cui ripartire.
    pub c_e_un_backup: bool,
    /// Non c'è niente da fare.
    pub vuoto: bool,
    /// Quando è stato scritto il backup.
    pub generato_ms: i64,
    /// Quanti brani riceverebbero statistiche nuove.
    pub brani_da_aggiornare: usize,
    /// Quanti sono già a posto.
    pub brani_invariati: usize,
    /// I primi cambiamenti, per esteso.
    pub cambi: Vec<CambioBranoIpc>,
    /// I brani del backup di cui qui non c'è nessun file.
    ///
    /// Il ripristino **non li crea**: una riga senza file non si può aprire, e
    /// la scansione successiva la toglierebbe. Sono qui perché chi guarda sappia
    /// cosa gli manca.
    pub assenti: Vec<CambioBranoIpc>,
    /// Quanti ne mancano in tutto.
    pub assenti_totale: usize,
    /// Le playlist da scrivere.
    pub playlist: Vec<CambioPlaylistIpc>,
    /// Quante playlist sono già a posto.
    pub playlist_invariate: usize,
    /// Le cartelle da aggiungere.
    pub cartelle: Vec<CartellaIpc>,
    /// Le skin da installare.
    pub skin_da_installare: Vec<String>,
    /// Quante skin ci sono già.
    pub skin_presenti: usize,
    /// Le bozze da scrivere.
    pub bozze_da_scrivere: Vec<String>,
    /// Quante bozze ci sono già.
    pub bozze_presenti: usize,
    /// La skin che diventerebbe attiva.
    pub skin_attiva: Option<String>,
}

/// Com'è andato un ripristino.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EsitoRipristinoIpc {
    /// Quante identità di brano hanno ricevuto statistiche.
    pub brani: usize,
    /// Quante playlist sono state scritte.
    pub playlist: usize,
    /// Quante cartelle sono state aggiunte.
    pub cartelle: usize,
    /// Quante skin sono state installate.
    pub skin: usize,
    /// Quante bozze sono state scritte.
    pub bozze: usize,
    /// La skin attiva è cambiata.
    pub skin_attiva: bool,
    /// I file che il piano chiedeva e che su Drive non c'erano.
    pub mancanti: Vec<String>,
}

// ── i comandi ───────────────────────────────────────────────────────────────

/// Lo stato del backup. Istantaneo: non tocca la rete.
#[tauri::command]
pub fn nuvola_stato(
    stato: State<'_, Stato>,
    nuvola: State<'_, StatoNuvola>,
) -> Esito<StatoNuvolaIpc> {
    stato_ipc(&stato, &nuvola).map_err(errore)
}

/// Collega un account Google.
///
/// Apre la schermata di consenso nel **browser di sistema** e aspetta la
/// risposta su un socket loopback effimero, per non più di tre minuti. Il token
/// di aggiornamento finisce nel portachiavi; l'email in `settings`, che non è un
/// segreto ed è quel che si mostra nelle Impostazioni.
///
/// # Perché `(async)` su una funzione che non è `async`
///
/// Perché un `#[tauri::command]` normale gira **sul filo principale**, quello
/// dell'anello degli eventi della finestra. Questa funzione aspetta che una
/// persona legga una schermata di consenso in un browser: fino a tre minuti in
/// cui l'applicazione sarebbe congelata, senza ridisegnarsi e senza rispondere a
/// niente — compreso il tasto che l'utente premerebbe per annullare.
///
/// `(async)` su una funzione sincrona la manda sulla riserva di fili di Tauri.
/// Il corpo resta bloccante e ordinario; cambia solo **dove** gira. Vale per
/// questo comando e per i tre qui sotto che vanno in rete.
#[tauri::command(async)]
pub fn nuvola_collega(
    app: AppHandle,
    stato: State<'_, Stato>,
    nuvola: State<'_, StatoNuvola>,
) -> Esito<StatoNuvolaIpc> {
    let esito = (|| {
        let _turno = Turno::prendi(&nuvola.in_corso).ok_or_else(occupato)?;
        let credenziali = con_libreria(&stato, |libreria| credenziali(&libreria.connection))?;
        let rete = Rete::nuova("google", oauth::SCADENZA);

        let token = oauth::collega(&rete, &credenziali, |url| {
            tauri_plugin_opener::open_url(url, None::<&str>).map_err(|err| {
                AppError::new(ErrorCode::InternalAborted {
                    what: Some("apertura del browser".to_owned()),
                })
                .with_cause(err.to_string())
            })
        })?;

        let Some(refresh) = token.refresh_token.as_deref() else {
            // Succede solo se `prompt=consent` sparisse dall'indirizzo di
            // autorizzazione. Senza refresh token il backup automatico
            // smetterebbe dopo un'ora, in silenzio: meglio non collegarsi.
            return Err(AppError::new(ErrorCode::SyncAuthExpired)
                .with_cause("Google non ha mandato un token di aggiornamento"));
        };
        DiSistema.scrivi(portachiavi::GOOGLE_REFRESH_TOKEN, refresh)?;

        con_libreria(&stato, |libreria| {
            if let Some(email) = &token.email {
                settings::write(&libreria.connection, CHIAVE_EMAIL, email)?;
            }
            // Collegare accende l'automatico: chi ha appena dato il consenso a
            // Google voleva il backup, non un secondo interruttore da trovare.
            settings::write(&libreria.connection, CHIAVE_ATTIVO, "1")
        })?;

        ricorda_token(&nuvola, token);
        // La prima passata non aspetta il quarto d'ora: chi collega vuole
        // vedere che ha funzionato.
        sporca_subito(&nuvola);
        Ok(())
    })();

    concludi(&app, &stato, &nuvola, esito)
}

/// Chiude un comando: rilascia, riferisce, e restituisce lo stato di **adesso**.
///
/// Lo stato si calcola qui e non dentro il comando, ed è la ragione per cui
/// questa funzione esiste. Dentro, il turno è ancora preso: la risposta direbbe
/// `inCorso: true` per un'operazione appena finita. La finestra riceve due cose
/// — la risposta al comando e l'evento `nuvola:stato` — senza nessuna garanzia
/// su quale arrivi per ultima, e se fosse la risposta il tasto resterebbe
/// disabilitato per sempre.
fn concludi(
    app: &AppHandle,
    stato: &Stato,
    nuvola: &StatoNuvola,
    esito: Result<(), AppError>,
) -> Esito<StatoNuvolaIpc> {
    let adesso = esito
        .and_then(|()| stato_ipc(stato, nuvola))
        .map_err(errore);
    riferisci(app);
    adesso
}

/// Scollega l'account.
///
/// Revoca presso Google — **best-effort**: chi si scollega deve vedersi
/// scollegato anche senza rete — e pulisce comunque il portachiavi. Non tocca
/// niente su Drive: i file restano, e il giorno in cui l'utente si ricollega
/// sono ancora lì. Toglierli sarebbe una cancellazione che nessuno ha chiesto.
///
/// `(async)`: la revoca va in rete. Vedi [`nuvola_collega`].
#[tauri::command(async)]
pub fn nuvola_scollega(
    app: AppHandle,
    stato: State<'_, Stato>,
    nuvola: State<'_, StatoNuvola>,
) -> Esito<StatoNuvolaIpc> {
    let esito = (|| {
        let _turno = Turno::prendi(&nuvola.in_corso).ok_or_else(occupato)?;
        if let Some(refresh) = DiSistema.leggi(portachiavi::GOOGLE_REFRESH_TOKEN)? {
            let rete = Rete::nuova("google", oauth::SCADENZA);
            drop(oauth::revoca(&rete, &refresh));
        }
        DiSistema.cancella(portachiavi::GOOGLE_REFRESH_TOKEN)?;
        DiSistema.cancella(portachiavi::GOOGLE_CLIENT_SECRET)?;
        dimentica_token(&nuvola);

        // `forget` e non `write(.., "")`: una stringa vuota è un terzo stato
        // oltre «c'è» e «non c'è», e lo deve riconoscere ogni lettore — il
        // primo che si dimenta il `.filter(|e| !e.is_empty())` mostra una
        // schermata collegata a un account senza nome. E c'è la ragione che
        // basterebbe da sola: un'identità che l'utente ha chiesto di
        // dimenticare non resta scritta in un file che il backup copia via.
        // `CHIAVE_ATTIVO` no: lì lo zero è un valore, non un'assenza — spento
        // per scelta e mai acceso si comportano uguale, ma solo il primo è
        // quello che l'utente ha appena detto.
        con_libreria(&stato, |libreria| {
            settings::write(&libreria.connection, CHIAVE_ATTIVO, "0")?;
            settings::forget(&libreria.connection, CHIAVE_EMAIL)?;
            settings::forget(&libreria.connection, CHIAVE_FILE_ID)?;
            settings::forget(&libreria.connection, CHIAVE_IMPRONTA).map(|_| ())
        })?;
        scorda_errore(&nuvola);
        Ok(())
    })();

    concludi(&app, &stato, &nuvola, esito)
}

/// Sostituisce le credenziali del client con quelle scritte dall'utente.
///
/// Serve a chi vuole usare il **proprio** progetto Google invece di quello di
/// Aether: la quota è di chi la paga. Un identificativo vuoto rimette quelle
/// compilate dentro.
#[tauri::command]
pub fn nuvola_credenziali(
    app: AppHandle,
    stato: State<'_, Stato>,
    nuvola: State<'_, StatoNuvola>,
    client_id: String,
    client_secret: String,
) -> Esito<StatoNuvolaIpc> {
    let esito = (|| {
        let id = client_id.trim();
        // Il segreto nel portachiavi e l'identificativo in `settings`: il primo
        // è trattato come un segreto anche se non lo è del tutto, il secondo
        // non lo è mai stato.
        if id.is_empty() {
            con_libreria(&stato, |libreria| {
                settings::forget(&libreria.connection, CHIAVE_CLIENT_ID).map(|_| ())
            })?;
            DiSistema.cancella(portachiavi::GOOGLE_CLIENT_SECRET)?;
        } else {
            con_libreria(&stato, |libreria| {
                settings::write(&libreria.connection, CHIAVE_CLIENT_ID, id)
            })?;
            DiSistema.scrivi(portachiavi::GOOGLE_CLIENT_SECRET, client_secret.trim())?;
        }
        dimentica_token(&nuvola);
        Ok(())
    })();

    concludi(&app, &stato, &nuvola, esito)
}

/// Accende o spegne il backup automatico.
#[tauri::command]
pub fn nuvola_attiva(
    app: AppHandle,
    stato: State<'_, Stato>,
    nuvola: State<'_, StatoNuvola>,
    attivo: bool,
) -> Esito<StatoNuvolaIpc> {
    let esito = (|| {
        con_libreria(&stato, |libreria| {
            settings::write(
                &libreria.connection,
                CHIAVE_ATTIVO,
                if attivo { "1" } else { "0" },
            )
        })?;
        if attivo {
            sporca_subito(&nuvola);
        }
        scorda_errore(&nuvola);
        Ok(())
    })();

    concludi(&app, &stato, &nuvola, esito)
}

/// Chiede una passata adesso.
///
/// Torna **subito**: sveglia il filo e basta. Un comando che aspettasse la fine
/// del caricamento terrebbe fermo il canale IPC per minuti, e la finestra non
/// potrebbe nemmeno mostrare l'avanzamento che il filo sta emettendo.
#[tauri::command]
pub fn nuvola_salva(nuvola: State<'_, StatoNuvola>) -> Esito<()> {
    sporca_subito(&nuvola);
    Ok(())
}

/// Scarica il backup e dice cosa farebbe un ripristino, senza applicarlo.
///
/// `(async)`: scarica settanta chilobyte e li interpreta. Vedi
/// [`nuvola_collega`].
#[tauri::command(async)]
pub fn nuvola_piano_ripristino(
    stato: State<'_, Stato>,
    nuvola: State<'_, StatoNuvola>,
) -> Esito<PianoRipristinoIpc> {
    (|| {
        let _turno = Turno::prendi(&nuvola.in_corso).ok_or_else(occupato)?;
        let (dati, qui, credenziali) = leggi_per_passata(&stato)?;
        let scaricato = con_drive(&nuvola, &credenziali, servizio::scarica_salvataggio)?;
        let Some(da) = scaricato else {
            return Ok(niente_backup());
        };
        let piano = pianifica(&dati, &qui, &da);
        Ok(in_ipc(&piano, &da))
    })()
    .map_err(errore)
}

/// Applica il ripristino.
///
/// **Ricalcola il piano** invece di fidarsi di quello mostrato: fra l'anteprima
/// e la conferma può essere finita una scansione, e le skin appena scritte
/// cambiano cosa resta da fare. È la stessa invariante di `esegui_riordino`.
///
/// `(async)`: scarica anche i pacchetti di skin, che arrivano a venti megabyte
/// l'uno. Vedi [`nuvola_collega`].
#[tauri::command(async)]
pub fn nuvola_ripristina(
    app: AppHandle,
    stato: State<'_, Stato>,
    nuvola: State<'_, StatoNuvola>,
) -> Esito<EsitoRipristinoIpc> {
    (|| {
        let _turno = Turno::prendi(&nuvola.in_corso).ok_or_else(occupato)?;
        let (dati, qui, credenziali) = leggi_per_passata(&stato)?;

        // Prima i file, poi il database. Se si committasse la transazione e poi
        // un download fallisse, resterebbe scritto uno `skin.active` che punta a
        // una skin che non esiste, e Aether si riaprirebbe senza sapersi
        // disegnare.
        let (da, scritti) = con_drive(&nuvola, &credenziali, |drive| {
            let Some(da) = servizio::scarica_salvataggio(drive)? else {
                return Err(AppError::new(ErrorCode::SyncRemoteCorrupt)
                    .with_cause("su Drive non c'è nessun backup"));
            };
            let proposto = pianifica(&dati, &qui, &da);
            let scritti = servizio::scarica_pacchetti(drive, &dati, &da, &proposto, |passo| {
                avanza(&app, passo);
            })?;
            Ok((da, scritti))
        })?;

        // Si rilegge e si ripianifica con le skin ormai su disco.
        let applicato = con_libreria(&stato, |libreria| {
            let adesso = backup::stato_locale(&libreria.connection)?;
            let definitivo = pianifica(&libreria.data_dir, &adesso, &da);
            backup::applica(&mut libreria.connection, &definitivo)
        })?;

        Ok(EsitoRipristinoIpc {
            brani: applicato.tracks,
            playlist: applicato.playlists,
            cartelle: applicato.roots,
            skin: scritti.skin,
            bozze: scritti.bozze,
            skin_attiva: applicato.active_skin,
            mancanti: scritti.mancanti,
        })
    })()
    .map_err(errore)
}

// ── il filo di sottofondo ───────────────────────────────────────────────────

/// Segnala che c'è qualcosa di nuovo da salvare.
///
/// Da chiamare **dopo** ogni comando che cambia qualcosa che finisce nel backup.
/// Non fa niente e non fallisce se il backup non è configurato: chiamarla è
/// gratis, e dimenticarla è la ragione per cui una modifica non verrebbe mai
/// salvata.
pub fn sporca(app: &AppHandle) {
    if let Some(nuvola) = app.try_state::<StatoNuvola>() {
        // Un canale chiuso vuol dire che il filo non c'è più: non è un guasto
        // di cui il comando che ha appena messo un cuoricino debba occuparsi.
        let _ = nuvola.sveglia.send(Sveglia::Sporca);
    }
    // E la sincronia insieme, da qui e non da un secondo punto di marcatura
    // sparso per i comandi. Ce n'è già una versione per ogni comando che cambia
    // qualcosa; aggiungerne una seconda vorrebbe dire che il diciottesimo comando
    // ne chiama una delle due e non l'altra, e il sintomo sarebbe una modifica
    // che finisce nel backup ma non sull'altro dispositivo.
    crate::sincronia::sporca(app);
}

/// Segnala il cambiamento **solo se** il comando è andato a buon fine.
///
/// Avvolgere l'esito invece di scrivere due righe a ogni comando non è
/// pigrizia: è che «segnala solo se è riuscito» va deciso una volta. Sparso a
/// mano in diciassette punti, il diciottesimo lo scriverebbe qualcuno che
/// segnala anche sui fallimenti, e il sintomo sarebbe un backup che riparte
/// ogni volta che un comando dà errore.
pub fn se_riuscito<T, E>(app: &AppHandle, esito: Result<T, E>) -> Result<T, E> {
    if esito.is_ok() {
        sporca(app);
    }
    esito
}

/// Chiede una passata senza aspettare la raffica.
fn sporca_subito(nuvola: &StatoNuvola) {
    let _ = nuvola.sveglia.send(Sveglia::Subito);
}

/// Avvia il filo che salva da solo.
///
/// Modellato su `avvia_orologio`: un thread nominato, `recv_timeout` che fa da
/// periodicità **e** da antirimbalzo, nessun timer e nessun runtime asincrono.
/// Fare quattro richieste HTTP ogni quarto d'ora non giustifica tokio, e con
/// tokio arriverebbe una seconda idea di cos'è un errore e di chi possiede un
/// thread.
pub fn avvia_filo(app: AppHandle, orecchio: Receiver<Sveglia>) {
    let avviato = std::thread::Builder::new()
        .name("aether-nuvola".to_owned())
        .spawn(move || {
            let mut motivo = match orecchio.recv_timeout(ATTESA_AVVIO) {
                // Canale chiuso: l'applicazione sta uscendo.
                Err(RecvTimeoutError::Disconnected) => return,
                Ok(sveglia) => sveglia,
                Err(RecvTimeoutError::Timeout) => Sveglia::Subito,
            };
            loop {
                if motivo == Sveglia::Sporca {
                    // Si aspetta che la raffica finisca: dieci cuoricini di fila
                    // sono un salvataggio, non dieci. Si esce da qui quando per
                    // due minuti non arriva più niente — oppure subito, se nel
                    // frattempo il canale si è chiuso.
                    //
                    // I due esiti si distinguono, e non è pedanteria: un
                    // `while … .is_ok()` li confonde, e alla chiusura
                    // dell'applicazione uscirebbe dall'attesa per poi fare una
                    // passata **intera** — cioè un caricamento su Drive mentre il
                    // processo sta uscendo.
                    if crate::stato::aspetta_la_raffica(&orecchio, RAFFICA).is_break() {
                        return;
                    }
                }
                passata(&app);
                motivo = match orecchio.recv_timeout(INTERVALLO) {
                    Err(RecvTimeoutError::Disconnected) => return,
                    Ok(sveglia) => sveglia,
                    // Il timeout **è** il battito periodico: nessun timer.
                    Err(RecvTimeoutError::Timeout) => Sveglia::Subito,
                };
            }
        });
    if let Err(err) = avviato {
        nota!("[avvio] il filo del backup non è partito: {err}");
    }
}

/// Una passata di salvataggio. Non fallisce mai rumorosamente.
///
/// Ogni guasto diventa un evento per la finestra: questo filo gira per conto
/// suo, e un `Err` propagato non avrebbe nessuno a cui arrivare — mentre
/// l'utente ha il diritto di sapere che il suo backup non sta funzionando.
fn passata(app: &AppHandle) {
    let esito = passata_vera(app);
    if let Some(nuvola) = app.try_state::<StatoNuvola>() {
        let mut ultimo = nuvola
            .ultimo_errore
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *ultimo = match &esito {
            Ok(()) => None,
            Err(err) => Some(ErroreIpc::from(err.clone())),
        };
    }
    if let Err(err) = &esito {
        nota!(
            "[nuvola] passata non riuscita codice={} causa={}",
            err.code().kind().code(),
            err.cause().unwrap_or("—")
        );
    }
    riferisci(app);
}

/// Il corpo della passata, con le due sole finestre di lucchetto.
fn passata_vera(app: &AppHandle) -> Result<(), AppError> {
    let Some(stato) = app.try_state::<Stato>() else {
        return Ok(());
    };
    let Some(nuvola) = app.try_state::<StatoNuvola>() else {
        return Ok(());
    };
    let Some(_turno) = Turno::prendi(&nuvola.in_corso) else {
        // Un ripristino manuale sta usando la rete. La passata successiva
        // arriva fra un quarto d'ora: non c'è niente da segnalare.
        return Ok(());
    };

    // ── finestra 1: leggere. Millisecondi su 1 400 righe. ───────────────────
    let acceso = con_libreria(&stato, |libreria| attivo(&libreria.connection))?;
    if !acceso {
        return Ok(());
    }
    if DiSistema
        .leggi(portachiavi::GOOGLE_REFRESH_TOKEN)?
        .is_none()
    {
        return Ok(());
    }
    let (dati, locale, credenziali) = leggi_per_passata(&stato)?;
    let dispositivo = con_libreria(&stato, |libreria| dispositivo(&libreria.connection))?;

    // ── senza nessun lucchetto: da qui in poi si va in rete. ────────────────
    let salvata = con_drive(&nuvola, &credenziali, |drive| {
        servizio::salva(drive, &dati, &locale, &dispositivo, adesso_ms(), |passo| {
            avanza(app, passo);
        })
    })?;

    // ── finestra 2: riscrivere tre chiavi. ──────────────────────────────────
    con_libreria(&stato, |libreria| {
        settings::write(&libreria.connection, CHIAVE_FILE_ID, &salvata.file_id)?;
        settings::write(&libreria.connection, CHIAVE_IMPRONTA, &salvata.impronta)?;
        settings::write(
            &libreria.connection,
            CHIAVE_ULTIMO,
            &adesso_ms().to_string(),
        )
    })
}

/// Manda alla finestra un passo di avanzamento.
///
/// Un evento che non parte non è una ragione per far fallire un caricamento
/// riuscito: la finestra può essersi chiusa mentre il filo lavorava.
fn avanza(app: &AppHandle, passo: servizio::Avanzamento) {
    app.emetti(
        "nuvola:avanzamento",
        serde_json::json!({
            "fatti": passo.fatti,
            "totale": passo.totale,
            "cosa": passo.cosa.nome(),
        }),
    );
}

/// Manda alla finestra lo stato aggiornato.
fn riferisci(app: &AppHandle) {
    let (Some(stato), Some(nuvola)) = (app.try_state::<Stato>(), app.try_state::<StatoNuvola>())
    else {
        return;
    };
    if let Ok(ipc) = stato_ipc(&stato, &nuvola) {
        app.emetti("nuvola:stato", ipc);
    }
}

// ── i pezzi condivisi ───────────────────────────────────────────────────────

/// Legge sotto il lucchetto tutto ciò che serve a una passata, e lo rilascia.
///
/// Una funzione sola perché la finestra di lucchetto sia **una**: tre chiamate
/// separate a `con_libreria` sarebbero tre prese e tre rilasci, e fra l'una e
/// l'altra la libreria potrebbe cambiare — producendo uno stato letto a metà da
/// prima di una scansione e a metà da dopo.
fn leggi_per_passata(
    stato: &Stato,
) -> Result<(std::path::PathBuf, StatoLocale, Credenziali), AppError> {
    con_libreria(stato, |libreria| {
        Ok((
            libreria.data_dir.clone(),
            backup::stato_locale(&libreria.connection)?,
            credenziali(&libreria.connection)?,
        ))
    })
}

/// Un client di Drive con un access token valido.
///
/// Riusa quello in cache finché regge, e ne chiede uno nuovo un minuto prima
/// della scadenza: un token dura un'ora e una passata avviene ogni quarto d'ora,
/// quindi senza cache si chiederebbero a Google quattro rinfreschi all'ora per
/// niente.
fn collegamento(nuvola: &StatoNuvola, credenziali: &Credenziali) -> Result<Drive, AppError> {
    let access = accesso(nuvola, credenziali)?;
    Ok(Drive::nuovo(Rete::nuova("drive", SCADENZA_DRIVE), &access))
}

/// Esegue un'operazione su Drive, rifacendola **una volta** se il token scade.
///
/// Un 401 a metà passata succede: l'utente ha revocato l'accesso da un'altra
/// parte, oppure il token è scaduto prima di quel che diceva. La risposta è
/// buttare la cache, chiederne uno nuovo e rifare tutto da capo.
///
/// **Una volta sola.** Al secondo 401 non è più un token vecchio — è
/// un'autorizzazione che non c'è più — e ritentare in cerchio manterrebbe il
/// filo occupato senza mai dirlo a nessuno.
///
/// Rifare l'operazione **intera** e non la sola richiesta fallita è deliberato:
/// una passata è idempotente per costruzione (le impronte fanno saltare quel che
/// è già lassù), quindi ripeterla costa una `files.list` e niente altro.
fn con_drive<T>(
    nuvola: &StatoNuvola,
    credenziali: &Credenziali,
    azione: impl Fn(&Drive) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let esito = azione(&collegamento(nuvola, credenziali)?);
    match esito {
        Err(err) if drive::e_scaduto(&err) => {
            dimentica_token(nuvola);
            azione(&collegamento(nuvola, credenziali)?)
        }
        altro => altro,
    }
}

/// Un access token valido, dalla cache o da Google.
fn accesso(nuvola: &StatoNuvola, credenziali: &Credenziali) -> Result<String, AppError> {
    {
        let cache = nuvola
            .token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(token) = cache.as_ref() {
            if oauth::ancora_valido(token.scade_ms) {
                return Ok(token.access_token.clone());
            }
        }
    }

    let Some(refresh) = DiSistema.leggi(portachiavi::GOOGLE_REFRESH_TOKEN)? else {
        return Err(AppError::new(ErrorCode::SettingsSecretUnavailable {
            key: portachiavi::GOOGLE_REFRESH_TOKEN.to_owned(),
        })
        .with_cause("nessun account collegato"));
    };
    let rete = Rete::nuova("google", oauth::SCADENZA);
    let token = oauth::rinfresca(&rete, credenziali, &refresh)?;
    let access = token.access_token.clone();
    ricorda_token(nuvola, token);
    Ok(access)
}

/// Un access token valido per chi non è il backup.
///
/// Esiste perché la sincronia può depositare i suoi documenti sullo stesso Drive,
/// e un secondo posto che rinfresca il token vorrebbe dire due cache che si
/// invalidano a vicenda: due rinfreschi ogni volta che uno dei due lavora, e un
/// `invalid_grant` il giorno che Google smette di accettare il refresh token
/// vecchio dopo averne emesso uno nuovo. La cache resta **una**, qui.
///
/// # Errori
///
/// `settings.secretUnavailable` se non c'è un account collegato, `net.*` se il
/// rinfresco non riesce.
pub(crate) fn accesso_condiviso(
    app: &AppHandle,
    credenziali: &Credenziali,
) -> Result<String, AppError> {
    let Some(nuvola) = app.try_state::<StatoNuvola>() else {
        return Err(AppError::new(ErrorCode::SettingsSecretUnavailable {
            key: portachiavi::GOOGLE_REFRESH_TOKEN.to_owned(),
        })
        .with_cause("il backup non è avviato"));
    };
    accesso(&nuvola, credenziali)
}

/// Le credenziali del client, per chi non è il backup.
///
/// # Errori
///
/// `settings.secretUnavailable` se il client Google non è configurato.
pub(crate) fn credenziali_condivise(
    connection: &rusqlite::Connection,
) -> Result<Credenziali, AppError> {
    credenziali(connection)
}

/// Mette in cache l'access token appena ottenuto.
fn ricorda_token(nuvola: &StatoNuvola, token: Token) {
    let mut cache = nuvola
        .token
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *cache = Some(token);
}

/// Butta via l'access token in cache.
///
/// Si chiama quando cambiano le credenziali o si scollega l'account: un token
/// emesso per il client precedente continuerebbe a funzionare fino alla
/// scadenza, e nascondere per un'ora il fatto che le credenziali nuove non
/// vanno è il modo di far cercare il guasto nel posto sbagliato.
fn dimentica_token(nuvola: &StatoNuvola) {
    let mut cache = nuvola
        .token
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *cache = None;
}

/// Dimentica l'errore dell'ultima passata.
fn scorda_errore(nuvola: &StatoNuvola) {
    let mut ultimo = nuvola
        .ultimo_errore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *ultimo = None;
}

/// Le credenziali del client: quelle dell'utente, o quelle compilate dentro.
///
/// # Errori
///
/// `settings.secretUnavailable` se non ce ne sono né di un tipo né dell'altro —
/// il caso di chi ha clonato il repo e compilato senza le variabili d'ambiente.
/// Non è un guasto da nascondere: senza credenziali il backup non può partire, e
/// dirlo è tutto ciò che si può fare.
fn credenziali(connection: &rusqlite::Connection) -> Result<Credenziali, AppError> {
    let client_id = settings::read(connection, CHIAVE_CLIENT_ID)?
        .filter(|id| !id.trim().is_empty())
        .or_else(|| CLIENT_ID_COMPILATO.map(ToOwned::to_owned));
    let client_secret = DiSistema
        .leggi(portachiavi::GOOGLE_CLIENT_SECRET)?
        .filter(|segreto| !segreto.trim().is_empty())
        .or_else(|| CLIENT_SECRET_COMPILATO.map(ToOwned::to_owned));

    match (client_id, client_secret) {
        (Some(client_id), Some(client_secret)) => Ok(Credenziali {
            client_id,
            client_secret,
        }),
        _ => Err(AppError::new(ErrorCode::SettingsSecretUnavailable {
            key: CHIAVE_CLIENT_ID.to_owned(),
        })
        .with_cause("il client Google non è configurato")),
    }
}

/// Ci sono delle credenziali utilizzabili?
fn configurato(connection: &rusqlite::Connection) -> bool {
    credenziali(connection).is_ok()
}

/// Il backup automatico è acceso?
fn attivo(connection: &rusqlite::Connection) -> Result<bool, AppError> {
    Ok(settings::read(connection, CHIAVE_ATTIVO)?.as_deref() == Some("1"))
}

/// L'identificativo casuale di questo computer, generandolo la prima volta.
///
/// Uno solo per tutto il programma, e non uno per funzione: lo usano il backup,
/// per firmare il salvataggio, e la sincronia, per intestarsi i propri ascolti.
/// Se fossero due, gli ascolti di questo computer arriverebbero all'altro
/// dispositivo come quelli di un terzo che non esiste — e ogni reinstallazione
/// ne inventerebbe un altro ancora.
pub(crate) fn dispositivo(connection: &rusqlite::Connection) -> Result<String, AppError> {
    if let Some(id) =
        settings::read(connection, CHIAVE_DISPOSITIVO)?.filter(|id| !id.trim().is_empty())
    {
        return Ok(id);
    }
    let id = oauth::identificativo()?;
    settings::write(connection, CHIAVE_DISPOSITIVO, &id)?;
    Ok(id)
}

/// Lo stato, nella forma che la finestra riceve.
fn stato_ipc(stato: &Stato, nuvola: &StatoNuvola) -> Result<StatoNuvolaIpc, AppError> {
    let collegato = DiSistema
        .leggi(portachiavi::GOOGLE_REFRESH_TOKEN)
        // Un portachiavi che non risponde vale «non collegato»: è la direzione
        // che non promette. Il guasto vero si vedrà appena si prova a fare
        // qualcosa, con il suo codice.
        .unwrap_or_default()
        .is_some();
    let errore = nuvola
        .ultimo_errore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();

    con_libreria(stato, |libreria| {
        Ok(StatoNuvolaIpc {
            configurato: configurato(&libreria.connection),
            collegato,
            attivo: attivo(&libreria.connection)?,
            email: settings::read(&libreria.connection, CHIAVE_EMAIL)?
                .filter(|email| !email.is_empty()),
            ultimo_ms: settings::read(&libreria.connection, CHIAVE_ULTIMO)?
                .and_then(|quando| quando.parse().ok()),
            in_corso: nuvola.in_corso.load(Ordering::Acquire),
            errore,
        })
    })
}

/// Costruisce il piano dai due lati.
fn pianifica(dati: &std::path::Path, qui: &StatoLocale, da: &DaRipristinare) -> RestorePlan {
    let dal_backup = backup::dal_salvataggio(&da.contenuto, &qui.tombstones);
    // Il dominio non guarda il disco: chi ha il filesystem decora ogni cartella
    // con la sua esistenza, e chi decide la usa.
    let esistono: Vec<bool> = dal_backup
        .roots
        .iter()
        .map(|radice| std::path::Path::new(radice).is_dir())
        .collect();
    // Le skin **disponibili**, non solo quelle installate: `plain` viene dal
    // binario, e senza di lei una skin attiva `plain` nel backup sembrerebbe
    // impossibile da ripristinare.
    let skin_locali: Vec<String> = pacchetti::elenca_skin(dati)
        .into_keys()
        .chain(std::iter::once(pacchetti::DI_SERIE.to_owned()))
        .collect();
    let bozze_locali: Vec<String> = pacchetti::elenca_bozze(dati).into_keys().collect();

    plan_restore(&RestoreInput {
        local_tracks: &qui.tracks,
        backup_tracks: &dal_backup.tracks,
        local_playlists: &qui.playlists,
        backup_playlists: &dal_backup.playlists,
        local_roots: &qui.roots,
        backup_roots: &dal_backup.roots,
        backup_roots_exist: &esistono,
        local_skins: &skin_locali,
        backup_skins: &dal_backup.skins,
        local_drafts: &bozze_locali,
        backup_drafts: &dal_backup.drafts,
        local_active_skin: qui.active_skin.as_deref(),
        backup_active_skin: dal_backup.active_skin.as_deref(),
        path_rules: PathRules::for_current_platform(),
    })
}

/// Il piano vuoto di chi non ha ancora nessun backup.
fn niente_backup() -> PianoRipristinoIpc {
    PianoRipristinoIpc {
        c_e_un_backup: false,
        vuoto: true,
        generato_ms: 0,
        brani_da_aggiornare: 0,
        brani_invariati: 0,
        cambi: Vec::new(),
        assenti: Vec::new(),
        assenti_totale: 0,
        playlist: Vec::new(),
        playlist_invariate: 0,
        cartelle: Vec::new(),
        skin_da_installare: Vec::new(),
        skin_presenti: 0,
        bozze_da_scrivere: Vec::new(),
        bozze_presenti: 0,
        skin_attiva: None,
    }
}

/// Il piano, nella forma che la finestra riceve.
fn in_ipc(piano: &RestorePlan, da: &DaRipristinare) -> PianoRipristinoIpc {
    PianoRipristinoIpc {
        c_e_un_backup: true,
        vuoto: piano.is_empty(),
        generato_ms: da.generato_ms,
        brani_da_aggiornare: piano.tracks.len(),
        brani_invariati: piano.tracks_unchanged,
        cambi: piano
            .tracks
            .iter()
            .take(QUANTI_MOSTRARNE)
            .map(|cambio| {
                let (artista, titolo, album) = pezzi(cambio.key.as_str());
                CambioBranoIpc {
                    artista,
                    titolo,
                    album,
                    ascolti_prima: cambio.before.play_count,
                    ascolti_dopo: cambio.after.play_count,
                    voto_prima: cambio.before.rating,
                    voto_dopo: cambio.after.rating,
                    preferito_dopo: cambio.after.liked,
                }
            })
            .collect(),
        assenti: piano
            .tracks_absent
            .iter()
            .take(QUANTI_MOSTRARNE)
            .map(|chiave| {
                let (artista, titolo, album) = pezzi(chiave.as_str());
                CambioBranoIpc {
                    artista,
                    titolo,
                    album,
                    ascolti_prima: 0,
                    ascolti_dopo: 0,
                    voto_prima: 0,
                    voto_dopo: 0,
                    preferito_dopo: false,
                }
            })
            .collect(),
        assenti_totale: piano.tracks_absent.len(),
        playlist: piano
            .playlists
            .iter()
            .map(|cambio| CambioPlaylistIpc {
                nome: cambio.playlist.name.clone(),
                da_creare: cambio.is_new,
                automatica: cambio.playlist.is_smart,
                brani_qui: cambio.playlist.members.len(),
                brani_nel_backup: cambio.members_in_backup,
            })
            .collect(),
        playlist_invariate: piano.playlists_unchanged,
        cartelle: piano
            .roots_to_add
            .iter()
            .map(|radice| CartellaIpc {
                percorso: radice.path.clone(),
                esiste: radice.exists,
            })
            .collect(),
        skin_da_installare: piano.skins_to_install.clone(),
        skin_presenti: piano.skins_present,
        bozze_da_scrivere: piano.drafts_to_write.clone(),
        bozze_presenti: piano.drafts_present,
        skin_attiva: piano.active_skin.clone(),
    }
}

/// I tre pezzi di una chiave di brano, per poterli mostrare separati.
///
/// Sono la forma **normalizzata** dei tag — minuscole, senza punteggiatura — e
/// non i titoli veri: il backup non porta i titoli, porta le identità. Mostrarli
/// così è meno bello e più onesto di inventarsi una ricapitalizzazione, ed è
/// comunque abbastanza per riconoscere un brano che manca.
fn pezzi(chiave: &str) -> (String, String, String) {
    let mut parti = chiave.split('|');
    (
        parti.next().unwrap_or_default().to_owned(),
        parti.next().unwrap_or_default().to_owned(),
        parti.next().unwrap_or_default().to_owned(),
    )
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn una_chiave_si_divide_in_tre_pezzi() {
        assert_eq!(
            pezzi("blue oyster cult|dont fear the reaper|agents of fortune"),
            (
                "blue oyster cult".to_owned(),
                "dont fear the reaper".to_owned(),
                "agents of fortune".to_owned()
            )
        );
    }

    #[test]
    fn una_chiave_di_un_file_senza_tag_non_fa_cadere_niente() {
        // `||` è un'identità legittima: un file senza nessun tag.
        assert_eq!(pezzi("||"), (String::new(), String::new(), String::new()));
        assert_eq!(pezzi(""), (String::new(), String::new(), String::new()));
    }
}
