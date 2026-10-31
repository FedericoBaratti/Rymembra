//! Il backup su Drive, dal lato della finestra.
//!
//! Otto comandi, due eventi e un filo di sottofondo. Le decisioni stanno in
//! `rymembra-domain`, il formato in `rymembra-app`, la rete in `rymembra-cloud`: qui
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
//! Che sia vero non è affidato all'attenzione di chi legge: `rymembra-cloud` non
//! riceve mai una `rusqlite::Connection`, quindi il codice che terrebbe il
//! lucchetto durante una richiesta di rete non si può nemmeno scrivere.
//!
//! # Il client
//!
//! Dalla 3.2 è un'app dell'utente (`nuvola_app`), legata al backup con
//! `nuvola.app`: di serie la stessa delle cartelle di Drive. Nel binario non
//! c'è nessun client — `option_env!` e il ripiego compilato non esistono più —
//! e un backup senza app dice di non essere configurato.
//!
//! Da scrivere perché nessuno «indurisca» la cosa sbagliata: **un client secret
//! di un'app desktop non è un segreto**. La documentazione di Google per le
//! installed app lo dice, e quel che protegge lo scambio è PKCE. Sta nel
//! portachiavi lo stesso, perché è dell'utente e non di Rymembra.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use crate::spegnimento::Emette as _;
use rymembra_app::backup::{self, StatoLocale};
use rymembra_app::nuvola_app;
use rymembra_app::settings;
use rymembra_cloud::drive::{self, Drive};
use rymembra_cloud::http::Rete;
use rymembra_cloud::oauth::{self, Credenziali, Token};
use rymembra_cloud::pacchetti;
use rymembra_cloud::portachiavi::{self, DiSistema, Portachiavi as _};
use rymembra_cloud::servizio::{self, DaRipristinare};
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::paths::PathRules;
use rymembra_domain::restore::{RestoreInput, RestorePlan, plan_restore};
use serde::Serialize;
use tauri::{AppHandle, Manager as _, State};

use crate::disparte::in_disparte;
use crate::errore::{ErroreIpc, Esito, errore};
use crate::nota;
use crate::stato::{Stato, Turno, adesso_ms, con_libreria};

// ── le chiavi in `settings` ─────────────────────────────────────────────────
//
// Nessuna di queste è un segreto: il token di aggiornamento sta nel portachiavi
// di sistema, e il motivo è scritto in `rymembra_cloud::portachiavi`.

/// Il backup automatico è acceso.
const CHIAVE_ATTIVO: &str = "nuvola.attivo";
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

    /// Quante volte la cache del token è stata svuotata da [`dimentica_token`].
    ///
    /// Si cambia e si legge **sotto il lucchetto di `token`**. Un rinfresco
    /// legge l'epoca quando trova la cache vuota, va in rete senza lucchetto,
    /// e rimette il token in cache solo se l'epoca è ancora quella
    /// ([`ricorda_token_se`]): un «Scollega» o un cambio di segreto arrivati
    /// mentre aspettava Google non si fanno scavalcare da un token di prima.
    epoca_token: AtomicU64,

    /// Qualcuno ha premuto «Annulla» mentre si aspetta il consenso.
    ///
    /// Un bit accanto al turno e non dentro: chi lo alza è un comando che deve
    /// rispondere subito, e il turno lo tiene per tre minuti proprio
    /// l'operazione che si vuole fermare. Si abbassa all'inizio di ogni
    /// collegamento.
    annulla_collegamento: AtomicBool,
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
                epoca_token: AtomicU64::new(0),
                annulla_collegamento: AtomicBool::new(false),
            },
            orecchio,
        )
    }
}

/// I due stati che servono ai comandi che lavorano in disparte.
///
/// Un comando che gira sul pool bloccante riceve un `AppHandle` e non dei
/// `State`: il lavoro va mosso dentro una chiusura `'static`, e un prestito
/// dello stato lì dentro non entra. `try_state` e non `state`, per la ragione
/// di sempre: alla chiusura gli stati se ne vanno, e `state` panica.
fn stati(app: &AppHandle) -> Result<(State<'_, Stato>, State<'_, StatoNuvola>), AppError> {
    match (app.try_state::<Stato>(), app.try_state::<StatoNuvola>()) {
        (Some(stato), Some(nuvola)) => Ok((stato, nuvola)),
        _ => Err(AppError::new(ErrorCode::InternalAborted {
            what: Some("backup su Drive".to_owned()),
        })
        .with_cause("lo stato del backup non c'è più")),
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
    /// Il backup ha un'app Google. Se le manca il segreto lo dice
    /// `nuvola_app` (`segretoSalvato`), e collegando `cloud.appSecretMissing`.
    pub configurato: bool,
    /// Quale app, se ne ha una.
    pub app: Option<String>,
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
/// # Dove gira, e perché non più `(async)`
///
/// Un `#[tauri::command]` normale gira **sul filo principale**, quello
/// dell'anello degli eventi della finestra, e questa funzione aspetta che una
/// persona legga una schermata di consenso: fino a tre minuti di applicazione
/// congelata. Fino alla 2.4.0 la risposta era `(async)` su una funzione
/// sincrona, che manda il corpo su un *worker* del runtime — e un worker fermo
/// tre minuti è un posto in meno per ogni altro comando asincrono. Adesso il
/// corpo va sul pool bloccante, con [`in_disparte`], come tutti i comandi di
/// questo modulo che vanno in rete.
///
/// # Annullabile
///
/// [`nuvola_annulla`] alza un bit che l'attesa del consenso guarda a ogni
/// giro: chi ha aperto il consenso per sbaglio non resta tre minuti davanti a
/// un tasto spento. Un collegamento annullato non è un errore — è la risposta
/// che l'utente ha dato — e torna lo stato com'è, senza account.
#[tauri::command]
pub async fn nuvola_collega(app: AppHandle) -> Esito<StatoNuvolaIpc> {
    in_disparte("collegamento a Google", move || collega(&app))
        .await
        .map_err(errore)?
}

/// Ferma l'attesa del consenso, se ce n'è una.
///
/// Torna subito: alza il bit e basta. Chi aspetta se ne accorge entro un
/// decimo di secondo, chiude il servitore locale e lascia il turno, e la
/// finestra lo vede dall'evento `nuvola:stato`.
#[tauri::command]
pub fn nuvola_annulla(nuvola: State<'_, StatoNuvola>) -> Esito<()> {
    nuvola.annulla_collegamento.store(true, Ordering::Release);
    Ok(())
}

/// Il corpo di [`nuvola_collega`], in disparte.
fn collega(app: &AppHandle) -> Esito<StatoNuvolaIpc> {
    let (stato, nuvola) = stati(app).map_err(errore)?;
    let esito = (|| {
        let _turno = Turno::prendi(&nuvola.in_corso).ok_or_else(occupato)?;
        // Un «Annulla» rimasto da un collegamento di prima non deve fermare
        // questo prima ancora di cominciare.
        nuvola.annulla_collegamento.store(false, Ordering::Release);
        // Chi guarda deve vedere subito «Aspetto il consenso…» e il tasto per
        // annullare, non dopo che il browser si è aperto.
        riferisci(app);
        let credenziali = credenziali(&stato)?;
        let rete = Rete::nuova("google", oauth::SCADENZA);
        let fermare = || {
            nuvola.annulla_collegamento.load(Ordering::Acquire) || crate::spegnimento::in_uscita()
        };

        let token = oauth::collega(
            &rete,
            &credenziali,
            |url| {
                tauri_plugin_opener::open_url(url, None::<&str>).map_err(|err| {
                    AppError::new(ErrorCode::InternalAborted {
                        what: Some("apertura del browser".to_owned()),
                    })
                    .with_cause(err.to_string())
                })
            },
            &fermare,
        );
        let token = match token {
            // Annullato da chi guarda: non è un guasto, e la finestra non deve
            // mostrarlo come tale.
            Err(err)
                if matches!(err.code(), ErrorCode::InternalAborted { .. })
                    && nuvola.annulla_collegamento.load(Ordering::Acquire) =>
            {
                return Ok(());
            }
            altro => altro?,
        };

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

    concludi(app, &stato, &nuvola, esito)
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
/// # L'app resta
///
/// Fino alla 3.1 scollegarsi cancellava anche il client scritto a mano. Dalla
/// 3.2 il client è un'app dell'utente, che di serie serve anche le cartelle di
/// Drive: toglierla qui le spegnerebbe in silenzio. Le app si tolgono da «Le
/// tue app».
///
/// In disparte: la revoca va in rete. Vedi [`nuvola_collega`].
#[tauri::command]
pub async fn nuvola_scollega(app: AppHandle) -> Esito<StatoNuvolaIpc> {
    in_disparte("scollegamento da Google", move || {
        let (stato, nuvola) = stati(&app).map_err(errore)?;
        let esito = (|| {
            let _turno = Turno::prendi(&nuvola.in_corso).ok_or_else(occupato)?;
            revoca_se_non_toglie_niente(&stato)?;
            dimentica_l_account(&stato, &nuvola)
        })();
        concludi(&app, &stato, &nuvola, esito)
    })
    .await
    .map_err(errore)?
}

/// Revoca il consenso del backup presso Google, se la revoca non toglie
/// l'accesso a nessun altro.
///
/// # Perché non sempre
///
/// Google revoca il consenso **dell'app** per quell'account, non un token
/// solo: tutti i refresh token che quell'app ha avuto da quell'account se ne
/// vanno insieme. E di serie l'app del backup è anche quella delle cartelle di
/// Drive (`nuvola_app::predefinita`): fino alla 3.2.0 di prova, scollegare il
/// backup spegneva in silenzio le cartelle di musica, che a ogni giro
/// rispondevano «consenso scaduto». Se un conto di Drive usa la stessa app,
/// il backup si scollega lo stesso — portachiavi e impostazioni — ma il
/// consenso resta, e lo si dice nel diario.
fn revoca_se_non_toglie_niente(stato: &Stato) -> Result<(), AppError> {
    let Some(refresh) = DiSistema.leggi(portachiavi::GOOGLE_REFRESH_TOKEN)? else {
        return Ok(());
    };
    let (del_backup, conti) = con_libreria(stato, |l| {
        Ok((
            nuvola_app::app_del_backup(&l.connection)?,
            rymembra_app::nuvola::conti(&l.connection)?,
        ))
    })?;
    if revoca_permessa(del_backup.as_ref(), &conti) {
        let rete = Rete::nuova("google", oauth::SCADENZA);
        drop(oauth::revoca(&rete, &refresh));
    } else {
        nota!(
            "[nuvola] scollegamento del backup: niente revoca presso Google, la stessa app serve le cartelle di Drive"
        );
    }
    Ok(())
}

/// Si può revocare il consenso del backup: c'è la sua app, e nessun conto di
/// Google Drive la usa — legato a lei, o col suo stesso identificativo.
fn revoca_permessa(
    del_backup: Option<&nuvola_app::AppSalvata>,
    conti: &[rymembra_app::nuvola::ContoSalvato],
) -> bool {
    let Some(app) = del_backup else {
        return false;
    };
    !conti.iter().any(|conto| {
        conto.servizio == rymembra_domain::nuvola::Servizio::GoogleDrive
            && (conto.app.as_deref() == Some(app.app.as_str())
                || conto.client.as_deref() == Some(app.client_id.as_str()))
    })
}

/// Dimentica l'account collegato: portachiavi, `settings`, cache.
///
/// Non tocca l'app, e non revoca niente presso Google: è il pezzo comune fra
/// [`nuvola_scollega`], [`nuvola_backup_app`] e `nuvola_app`, quando l'app del
/// backup cambia identificativo o se ne va. La revoca la fa solo «Scollega»
/// (vedi [`revoca_se_non_toglie_niente`]): cambiare app non deve togliere il
/// consenso, perché rimettere l'identificativo di prima (A → B → A) riporta i
/// conti di Drive come erano.
pub(crate) fn dimentica_l_account(stato: &Stato, nuvola: &StatoNuvola) -> Result<(), AppError> {
    DiSistema.cancella(portachiavi::GOOGLE_REFRESH_TOKEN)?;
    dimentica_token(nuvola);

    // `forget` e non `write(.., "")`: una stringa vuota è un terzo stato
    // oltre «c'è» e «non c'è», e lo deve riconoscere ogni lettore — il
    // primo che si dimenta il `.filter(|e| !e.is_empty())` mostra una
    // schermata collegata a un account senza nome. E c'è la ragione che
    // basterebbe da sola: un'identità che l'utente ha chiesto di
    // dimenticare non resta scritta in un file che il backup copia via.
    // `CHIAVE_ATTIVO` no: lì lo zero è un valore, non un'assenza — spento
    // per scelta e mai acceso si comportano uguale, ma solo il primo è
    // quello che l'utente ha appena detto.
    con_libreria(stato, |libreria| {
        settings::write(&libreria.connection, CHIAVE_ATTIVO, "0")?;
        settings::forget(&libreria.connection, CHIAVE_EMAIL)?;
        settings::forget(&libreria.connection, CHIAVE_FILE_ID)?;
        settings::forget(&libreria.connection, CHIAVE_IMPRONTA).map(|_| ())
    })?;
    scorda_errore(nuvola);
    Ok(())
}

/// Sceglie l'app Google del backup (`None`: nessuna).
///
/// # Cambiare app vuol dire scollegarsi
///
/// Un token di aggiornamento appartiene al client che l'ha ottenuto: con un
/// altro client Google risponde `unauthorized_client`, a ogni passata, per
/// sempre. Se l'app che vale cambia davvero — un'altra app, o la stessa con un
/// altro identificativo — l'account si scollega come farebbe
/// [`nuvola_scollega`], e la finestra mostra di nuovo «Collega». Riscegliere
/// la stessa non scollega niente.
///
/// La cartella dell'app su Drive (`drive.appdata`) è di quell'app: con
/// un'altra i backup di prima possono non vedersi più, e la finestra lo dice
/// prima di chiedere.
///
/// In disparte: scollegarsi va in rete.
#[tauri::command]
pub async fn nuvola_backup_app(app: AppHandle, scelta: Option<String>) -> Esito<StatoNuvolaIpc> {
    in_disparte("app del backup", move || {
        let (stato, nuvola) = stati(&app).map_err(errore)?;
        let esito = (|| {
            let _turno = Turno::prendi(&nuvola.in_corso).ok_or_else(occupato)?;
            let identita = |stato: &Stato| -> Result<Option<(String, String)>, AppError> {
                Ok(
                    con_libreria(stato, |l| nuvola_app::app_del_backup(&l.connection))?
                        .map(|app| (app.app, app.client_id)),
                )
            };
            let prima = identita(&stato)?;
            con_libreria(&stato, |l| {
                nuvola_app::lega_il_backup(&l.connection, scelta.as_deref())
            })?;
            if identita(&stato)? != prima {
                dimentica_l_account(&stato, &nuvola)?;
            }
            dimentica_token(&nuvola);
            Ok(())
        })();
        let riuscito = esito.is_ok();
        let fatto = concludi(&app, &stato, &nuvola, esito);
        // Le app e quale fa da predefinita per Google: è cambiato anche quello.
        if riuscito {
            crate::nuvola_app::emetti(&app);
        }
        fatto
    })
    .await
    .map_err(errore)?
}

/// Fa `lavoro` col backup fermo — il suo turno preso — e poi lo dice alla
/// finestra.
///
/// Per `nuvola_app`, quando un'app Google cambia: una passata in corso tiene
/// in mano le credenziali di prima, e le userebbe.
///
/// # Errori
///
/// `sync.busy` se il backup sta lavorando; quelli di `lavoro`.
pub(crate) fn con_il_backup_fermo<T>(
    app: &AppHandle,
    lavoro: impl FnOnce(&Stato, &StatoNuvola) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let (stato, nuvola) = stati(app)?;
    let esito = {
        let _turno = Turno::prendi(&nuvola.in_corso).ok_or_else(occupato)?;
        lavoro(&stato, &nuvola)
    };
    riferisci(app);
    esito
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
/// In disparte: scarica settanta chilobyte e li interpreta, e su una rete
/// lenta sono secondi. Vedi [`nuvola_collega`].
#[tauri::command]
pub async fn nuvola_piano_ripristino(app: AppHandle) -> Esito<PianoRipristinoIpc> {
    in_disparte("piano del ripristino", move || {
        let (stato, nuvola) = stati(&app)?;
        let _turno = Turno::prendi(&nuvola.in_corso).ok_or_else(occupato)?;
        let (dati, qui, credenziali) = leggi_per_passata(&stato)?;
        let scaricato = con_drive(&nuvola, &credenziali, |drive| {
            servizio::scarica_salvataggio(drive)
        })?;
        let Some(da) = scaricato else {
            return Ok(niente_backup());
        };
        let piano = pianifica(&dati, &qui, &da, &cartelle_presenti(&da));
        Ok(in_ipc(&piano, &da))
    })
    .await
    .map_err(errore)?
    .map_err(errore)
}

/// Applica il ripristino.
///
/// **Ricalcola il piano** invece di fidarsi di quello mostrato: fra l'anteprima
/// e la conferma può essere finita una scansione, e le skin appena scritte
/// cambiano cosa resta da fare. È l'invariante di ogni `piano → esegui` di
/// Rymembra: il piano si mostra, l'esecuzione lo rifà.
///
/// In disparte: scarica anche i pacchetti di skin, che arrivano a venti
/// megabyte l'uno. Vedi [`nuvola_collega`].
///
/// Un pacchetto che non scende non ferma il ripristino: finisce fra i
/// mancanti, e il perché nel diario.
#[tauri::command]
pub async fn nuvola_ripristina(app: AppHandle) -> Esito<EsitoRipristinoIpc> {
    in_disparte("ripristino da Drive", move || ripristina(&app))
        .await
        .map_err(errore)?
        .map_err(errore)
}

/// Il corpo di [`nuvola_ripristina`], in disparte.
fn ripristina(app: &AppHandle) -> Result<EsitoRipristinoIpc, AppError> {
    let (stato, nuvola) = stati(app)?;
    (|| {
        let _turno = Turno::prendi(&nuvola.in_corso).ok_or_else(occupato)?;
        let (dati, qui, credenziali) = leggi_per_passata(&stato)?;

        // Prima i file, poi il database. Se si committasse la transazione e poi
        // un download fallisse, resterebbe scritto uno `skin.active` che punta a
        // una skin che non esiste, e Rymembra si riaprirebbe senza sapersi
        // disegnare.
        let (da, esistono, scritti) = con_drive(&nuvola, &credenziali, |drive| {
            let Some(da) = servizio::scarica_salvataggio(drive)? else {
                return Err(AppError::new(ErrorCode::SyncRemoteCorrupt)
                    .with_cause("su Drive non c'è nessun backup"));
            };
            let esistono = cartelle_presenti(&da);
            let proposto = pianifica(&dati, &qui, &da, &esistono);
            let scritti = servizio::scarica_pacchetti(drive, &dati, &da, &proposto, |passo| {
                avanza(app, passo);
            })?;
            Ok((da, esistono, scritti))
        })?;
        for guasto in &scritti.guasti {
            nota!("[nuvola] pacchetto non ripristinato: {guasto}");
        }

        // Si rilegge e si ripianifica con le skin ormai su disco. Le cartelle
        // no: sono quelle di prima, guardate fuori dal lucchetto.
        let applicato = con_libreria(&stato, |libreria| {
            let adesso = backup::stato_locale(&libreria.connection)?;
            let definitivo = pianifica(&libreria.data_dir, &adesso, &da, &esistono);
            backup::applica(&mut libreria.connection, &definitivo)
        })?;
        // Brani, playlist e cartelle sono cambiati sotto la finestra: l'albero
        // delle cartelle si butta, e la vista si rilegge con l'evento.
        crate::cartelle::cambiate(app);

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
/// Il ciclo — thread nominato, `recv_timeout` che fa da periodicità **e** da
/// antirimbalzo — sta in [`crate::stato::avvia_filo_periodico`], accanto agli
/// altri due che hanno la stessa forma. Nessun timer e nessun runtime asincrono:
/// fare quattro richieste HTTP ogni quarto d'ora non giustifica tokio, e con
/// tokio arriverebbe una seconda idea di cos'è un errore e di chi possiede un
/// thread.
pub fn avvia_filo(app: AppHandle, orecchio: Receiver<Sveglia>) {
    let avviato = crate::stato::avvia_filo_periodico(
        "rymembra-nuvola",
        orecchio,
        ATTESA_AVVIO,
        RAFFICA,
        INTERVALLO,
        // Solo `Sporca` fa aspettare: dieci cuoricini di fila sono un
        // salvataggio, non dieci — vedi [`RAFFICA`]. Chi ha chiesto di salvare
        // adesso, invece, sta guardando la finestra.
        |sveglia| *sveglia == Sveglia::Sporca,
        move || passata(&app),
    );
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
///
/// Le credenziali vengono dopo, fuori dal lucchetto: il segreto sta nel
/// portachiavi, che non sempre risponde subito.
fn leggi_per_passata(
    stato: &Stato,
) -> Result<(std::path::PathBuf, StatoLocale, Credenziali), AppError> {
    let (dati, locale, app) = con_libreria(stato, |libreria| {
        Ok((
            libreria.data_dir.clone(),
            backup::stato_locale(&libreria.connection)?,
            nuvola_app::app_del_backup(&libreria.connection)?,
        ))
    })?;
    Ok((dati, locale, credenziali_di(app)?))
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
///
/// # Un rinfresco che arriva dopo «Scollega»
///
/// Il rinfresco va in rete senza lucchetto, e nel frattempo qualcuno può
/// svuotare la cache: «Scollega», o un segreto cambiato. Rimettere in cache il
/// token arrivato dopo voleva dire un'ora di passate di sincronia sul Drive
/// dell'account appena scollegato, o col client di prima. Per questo l'epoca si
/// legge insieme alla cache vuota, e il token rientra solo se è ancora quella;
/// se non lo è e il refresh token non c'è più, si risponde come senza account.
fn accesso(nuvola: &StatoNuvola, credenziali: &Credenziali) -> Result<String, AppError> {
    let epoca = match token_in_cache(nuvola) {
        Ok(access) => return Ok(access),
        Err(epoca) => epoca,
    };

    let senza_account = || {
        AppError::new(ErrorCode::SettingsSecretUnavailable {
            key: portachiavi::GOOGLE_REFRESH_TOKEN.to_owned(),
        })
        .with_cause("nessun account collegato")
    };
    let Some(refresh) = DiSistema.leggi(portachiavi::GOOGLE_REFRESH_TOKEN)? else {
        return Err(senza_account());
    };
    let rete = Rete::nuova("google", oauth::SCADENZA);
    let token = oauth::rinfresca(&rete, credenziali, &refresh)?;
    let access = token.access_token.clone();
    if !ricorda_token_se(nuvola, token, epoca)
        && DiSistema
            .leggi(portachiavi::GOOGLE_REFRESH_TOKEN)?
            .is_none()
    {
        // Scollegato mentre si aspettava Google: il token non entra in cache
        // e non si usa.
        return Err(senza_account());
    }
    // L'epoca è cambiata ma l'account c'è ancora (un segreto nuovo): il token
    // serve a questa richiesta e basta, la prossima rinfresca da capo.
    Ok(access)
}

/// L'access token in cache se vale ancora, altrimenti l'epoca della cache,
/// letta sotto lo stesso lucchetto: vedi [`ricorda_token_se`].
fn token_in_cache(nuvola: &StatoNuvola) -> Result<String, u64> {
    let cache = nuvola
        .token
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(token) = cache.as_ref()
        && oauth::ancora_valido(token.scade_ms)
    {
        return Ok(token.access_token.clone());
    }
    Err(nuvola.epoca_token.load(Ordering::Acquire))
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

/// Mette in cache l'access token appena ottenuto.
fn ricorda_token(nuvola: &StatoNuvola, token: Token) {
    let mut cache = nuvola
        .token
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *cache = Some(token);
}

/// Mette in cache il token di un rinfresco, se dal momento in cui la cache si
/// è trovata vuota (`epoca`, da [`token_in_cache`]) nessuno l'ha svuotata.
///
/// `false` se l'epoca è cambiata: il token è rimasto fuori.
fn ricorda_token_se(nuvola: &StatoNuvola, token: Token, epoca: u64) -> bool {
    let mut cache = nuvola
        .token
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if nuvola.epoca_token.load(Ordering::Acquire) != epoca {
        return false;
    }
    *cache = Some(token);
    true
}

/// Butta via l'access token in cache.
///
/// Si chiama quando cambiano le credenziali o si scollega l'account: un token
/// emesso per il client precedente continuerebbe a funzionare fino alla
/// scadenza, e nascondere per un'ora il fatto che le credenziali nuove non
/// vanno è il modo di far cercare il guasto nel posto sbagliato.
///
/// Cambia anche l'epoca, sotto lo stesso lucchetto: un rinfresco partito prima
/// non rimette il suo token (vedi [`accesso`]).
pub(crate) fn dimentica_token(nuvola: &StatoNuvola) {
    let mut cache = nuvola
        .token
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    nuvola.epoca_token.fetch_add(1, Ordering::AcqRel);
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

/// Le credenziali dell'app del backup (e della sincronia su Drive).
///
/// Il database sotto il lucchetto, il portachiavi fuori: da non chiamare
/// dentro `con_libreria`.
///
/// # Errori
///
/// `cloud.clientMissing` se il backup non ha un'app — il caso di chi non l'ha
/// ancora registrata: senza, il backup non può partire, e dirlo è tutto ciò che
/// si può fare —, `cloud.appSecretMissing` se all'app manca il segreto,
/// `settings.secretUnavailable` se il portachiavi non risponde.
pub(crate) fn credenziali(stato: &Stato) -> Result<Credenziali, AppError> {
    let app = con_libreria(stato, |libreria| {
        nuvola_app::app_del_backup(&libreria.connection)
    })?;
    credenziali_di(app)
}

/// Le credenziali di un'app del backup già letta.
fn credenziali_di(app: Option<nuvola_app::AppSalvata>) -> Result<Credenziali, AppError> {
    let Some(app) = app else {
        return Err(AppError::new(ErrorCode::CloudClientMissing {
            servizio: oauth::GOOGLE.nome.to_owned(),
        })
        .with_cause("il backup non ha un'app Google"));
    };
    crate::nuvola_app::credenziali_dell_app(&app)
}

/// Il backup ha un'app?
fn configurato(connection: &rusqlite::Connection) -> Result<Option<String>, AppError> {
    Ok(nuvola_app::app_del_backup(connection)?.map(|app| app.app))
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
        let app = configurato(&libreria.connection)?;
        Ok(StatoNuvolaIpc {
            configurato: app.is_some(),
            app,
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

/// Per ogni cartella del backup, se c'è su questo disco.
///
/// Il dominio non guarda il disco: chi ha il filesystem decora ogni cartella
/// con la sua esistenza, e chi decide la usa. Si guarda **una** volta e fuori
/// dal lucchetto della libreria: una cartella sul NAS addormentato ci mette
/// secondi a rispondere, e prima lo si faceva dentro, con scansione, play e
/// finestra fermi ad aspettare. Una cartella che non risponde in tempo vale
/// «non c'è», come prima valeva un `is_dir` fallito: è la direzione che non
/// promette.
///
/// Nello stesso ordine di `contenuto.roots`, che è lo stesso di
/// `dal_salvataggio(..).roots`: le cartelle si copiano senza filtri.
fn cartelle_presenti(da: &DaRipristinare) -> Vec<bool> {
    rymembra_app::volumi::esistenze(
        da.contenuto.roots.clone(),
        rymembra_app::volumi::SCADENZA_RISOLUZIONE,
    )
    .into_iter()
    .map(|esistenza| esistenza == rymembra_app::volumi::Esistenza::Presente)
    .collect()
}

/// Costruisce il piano dai due lati. `esistono` è [`cartelle_presenti`].
fn pianifica(
    dati: &std::path::Path,
    qui: &StatoLocale,
    da: &DaRipristinare,
    esistono: &[bool],
) -> RestorePlan {
    let dal_backup = backup::dal_salvataggio(&da.contenuto, &qui.tombstones);
    // Le skin **disponibili**, non solo quelle installate: `plain` viene dal
    // binario, e senza di lei una skin attiva `plain` nel backup sembrerebbe
    // impossibile da ripristinare. Le sane soltanto: una skin il cui archivio
    // non si apre deve risultare mancante, così il piano la riscarica — vedi
    // `pacchetti::elenca_skin_sane`.
    let skin_locali: Vec<String> = pacchetti::elenca_skin_sane(dati)
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
        backup_roots_exist: esistono,
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

    use rymembra_app::nuvola::ContoSalvato;
    use rymembra_app::nuvola_app::AppSalvata;
    use rymembra_domain::nuvola::Servizio;
    use rymembra_domain::nuvola_app::Piattaforma;

    fn app_del_backup() -> AppSalvata {
        AppSalvata {
            app: "google-aaaaaaaa".to_owned(),
            piattaforma: Piattaforma::Google,
            nome: "Google".to_owned(),
            client_id: "1-a.apps.googleusercontent.com".to_owned(),
            creata_ms: 1,
        }
    }

    fn conto(servizio: Servizio, app: Option<&str>, client: Option<&str>) -> ContoSalvato {
        ContoSalvato {
            conto: format!("{}-abcdefgh", servizio.chiave()),
            servizio,
            nome: "mario".to_owned(),
            dettagli: None,
            cursore: None,
            ultimo_ms: None,
            app: app.map(str::to_owned),
            client: client.map(str::to_owned),
        }
    }

    #[test]
    fn la_revoca_risparmia_i_conti_di_drive_della_stessa_app() {
        let app = app_del_backup();
        // Legato all'app del backup.
        let legato = conto(Servizio::GoogleDrive, Some("google-aaaaaaaa"), None);
        assert!(!revoca_permessa(Some(&app), &[legato]));
        // L'app è stata tolta e rimessa: un altro nome, lo stesso client.
        let stesso_client = conto(
            Servizio::GoogleDrive,
            Some("google-bbbbbbbb"),
            Some("1-a.apps.googleusercontent.com"),
        );
        assert!(!revoca_permessa(Some(&app), &[stesso_client]));
    }

    #[test]
    fn la_revoca_si_fa_se_nessun_conto_usa_quel_client() {
        let app = app_del_backup();
        assert!(revoca_permessa(Some(&app), &[]));
        let onedrive = conto(Servizio::OneDrive, Some("google-aaaaaaaa"), None);
        let altra = conto(
            Servizio::GoogleDrive,
            Some("google-cccccccc"),
            Some("2-b.apps.googleusercontent.com"),
        );
        assert!(revoca_permessa(Some(&app), &[onedrive, altra]));
    }

    fn token_di_prova(access: &str) -> Token {
        Token {
            access_token: access.to_owned(),
            refresh_token: None,
            scade_ms: adesso_ms() + 3_600_000,
            email: None,
        }
    }

    #[test]
    fn un_rinfresco_arrivato_dopo_dimentica_token_non_torna_in_cache() {
        let (nuvola, _orecchio) = StatoNuvola::nuovo();
        // Il rinfresco trova la cache vuota e legge l'epoca; mentre aspetta
        // Google, «Scollega» svuota la cache.
        let epoca = token_in_cache(&nuvola).expect_err("cache vuota");
        dimentica_token(&nuvola);
        assert!(!ricorda_token_se(&nuvola, token_di_prova("vecchio"), epoca));
        assert!(
            token_in_cache(&nuvola).is_err(),
            "il token di prima di «Scollega» non è in cache"
        );
    }

    #[test]
    fn un_rinfresco_senza_nessuno_in_mezzo_va_in_cache() {
        let (nuvola, _orecchio) = StatoNuvola::nuovo();
        let epoca = token_in_cache(&nuvola).expect_err("cache vuota");
        assert!(ricorda_token_se(&nuvola, token_di_prova("buono"), epoca));
        assert_eq!(token_in_cache(&nuvola), Ok("buono".to_owned()));
        // Dopo un 401 (`con_drive`) la cache si svuota e il rinfresco dopo
        // legge l'epoca nuova: il suo token entra.
        dimentica_token(&nuvola);
        let epoca = token_in_cache(&nuvola).expect_err("svuotata");
        assert!(ricorda_token_se(&nuvola, token_di_prova("nuovo"), epoca));
        assert_eq!(token_in_cache(&nuvola), Ok("nuovo".to_owned()));
    }

    #[test]
    fn senza_app_del_backup_non_si_revoca() {
        assert!(!revoca_permessa(None, &[]));
    }

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
