//! Il controllo degli aggiornamenti, dal lato della finestra.
//!
//! Cinque comandi, due eventi e un filo di sottofondo. La forma è quella di
//! [`crate::sincronia`] — thread nominato, `recv_timeout` come periodicità,
//! `Turno` per non sovrapporsi a se stesso — perché il problema di
//! orchestrazione è lo stesso, e due forme diverse per lo stesso problema sono
//! due forme che divergono il giorno in cui qualcuno ne aggiusta una sola.
//!
//! # Cosa succede davvero, ogni mezz'ora
//!
//! Una `GET` a `github.com` che scarica un `latest.json` di trecento byte. Non
//! ci va dentro niente: nessun identificativo, nessun conteggio, nessun numero
//! di serie. La versione installata non viene nemmeno mandata — il confronto lo
//! fa il programma, qui, dopo aver letto il file. Quel che GitHub vede è che
//! qualcuno da un certo indirizzo IP ha chiesto un file pubblico, cioè quel che
//! vedrebbe di chiunque aprisse la pagina delle release con un browser.
//!
//! È l'unica richiesta che Aether fa senza che nessuno gliel'abbia chiesta, ed
//! è per questo che sta scritta in `PRIVACY.md` con un numero suo e si spegne
//! con un interruttore in Impostazioni.
//!
//! # Perché non si installa da solo
//!
//! Perché installare vuol dire chiudere l'applicazione. Farlo mentre qualcuno
//! sta ascoltando — o peggio, mentre sta riordinando trentamila file sul disco
//! — è il genere di cosa che si perdona una volta sola. Il filo trova, dice, e
//! aspetta: scaricare e installare partono da un tasto e da nient'altro.
//!
//! # La firma
//!
//! L'installer che arriva da GitHub viene verificato con minisign contro la
//! chiave pubblica compilata dentro l'eseguibile, e se la firma non torna non
//! viene eseguito. Non è una formalità: senza, chiunque sappia rispondere al
//! posto di `github.com` — un proxy aziendale, una CA di troppo nel magazzino
//! del sistema, un captive portal — potrebbe far installare un eseguibile
//! qualunque a chi si fida di questo programma. La chiave privata che firma sta
//! nei segreti del repository e non su questo disco.
//!
//! Se `plugins.updater.pubkey` in `tauri.conf.json` è vuoto — cioè in un albero
//! in cui le chiavi non sono ancora state generate — il controllo **non parte
//! affatto**. Un updater che scarica senza poter verificare sarebbe peggio di
//! nessun updater, e annunciare un aggiornamento che poi non si può installare
//! sarebbe solo un modo più lungo di fallire.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use crate::spegnimento::Emette as _;
use aether_app::settings;
use aether_domain::errors::{AppError, ErrorCode};
use serde::Serialize;
use tauri::{AppHandle, Manager as _, State};
use tauri_plugin_updater::{Update, Updater, UpdaterExt as _};

use crate::errore::{ErroreIpc, Esito, errore};
use crate::nota;
use crate::stato::{Stato, Turno, adesso_ms, con_libreria};

/// La chiave con cui l'interruttore del controllo sta in `settings`.
///
/// Assente vuol dire **acceso**: è il valore di serie, e scriverlo all'avvio
/// per dire «sì, davvero acceso» vorrebbe dire una scrittura nel database al
/// primo lancio per non dire niente di nuovo.
const CHIAVE_ATTIVO: &str = "aggiornamenti.attivo";

/// La chiave del momento dell'ultimo controllo riuscito.
const CHIAVE_ULTIMO: &str = "aggiornamenti.ultimo_ms";

/// La chiave della versione che l'utente ha deciso di ignorare.
///
/// Senza, «Non ora» durerebbe mezz'ora: l'avviso tornerebbe al controllo
/// successivo, identico, e la terza volta l'utente imparerebbe a non leggerlo.
const CHIAVE_SALTATA: &str = "aggiornamenti.saltata";

/// Ogni quanto si guarda se è uscita una versione nuova.
///
/// Mezz'ora, che è quel che è stato chiesto ed è anche una scelta difendibile:
/// una release esce qualche volta al mese, quindi la frequenza non serve a
/// scoprirla presto — serve a fare in modo che chi tiene Aether aperto per
/// giorni la scopra comunque, senza dover chiudere e riaprire. Il costo è una
/// richiesta da trecento byte, cioè meno di quel che costa disegnare la lista
/// dei brani una volta.
const INTERVALLO: Duration = Duration::from_secs(30 * 60);

/// Quanto si aspetta prima del primo controllo.
///
/// Due minuti: più tardi di tutti gli altri fili — sincronia venti secondi,
/// nuvola trenta, arricchimento novanta — perché è la cosa meno urgente che
/// l'applicazione possa fare all'apertura. Chi apre Aether vuole ascoltare;
/// sapere che fra un mese c'è una versione nuova può aspettare due minuti.
const ATTESA_AVVIO: Duration = Duration::from_secs(120);

/// Ogni quanto, al massimo, si racconta l'avanzamento di uno scaricamento.
///
/// Il plugin chiama il callback a ogni pezzo che arriva dalla rete: su un
/// installer da settanta megabyte sono migliaia di chiamate, e altrettanti
/// eventi verso una finestra che ne disegna al massimo sessanta al secondo.
const RESPIRO_AVANZAMENTO: Duration = Duration::from_millis(200);

/// La variabile che sostituisce l'endpoint, e che esiste solo in sviluppo.
///
/// Provare un updater senza questa vorrebbe dire pubblicare una release vera
/// per ogni tentativo. Con questa si serve un `latest.json` da una cartella
/// locale e si guarda succedere tutto: l'avviso che compare, il «Non ora» che
/// lo zittisce, l'installer che parte. Il precedente è `AETHER_DATI`, e la
/// ragione è la stessa — le cose irreversibili si giudicano guardandole.
///
/// In rilascio non esiste: l'endpoint di un updater è la cosa che decide quale
/// eseguibile finisce sul computer di qualcuno, e una variabile d'ambiente che
/// la cambia è un modo di installare software altrui che non deve esserci.
#[cfg(debug_assertions)]
const VARIABILE_ENDPOINT: &str = "AETHER_AGGIORNAMENTI_ENDPOINT";

/// Perché il filo si è svegliato.
///
/// Un solo motivo, a differenza della sincronia: niente «sporca» un controllo
/// aggiornamenti, perché non dipende da cosa succede nella libreria.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sveglia {
    /// Qualcuno ha chiesto di controllare adesso.
    Subito,
}

/// Quel che il controllo aggiornamenti tiene aperto mentre l'applicazione gira.
pub struct StatoAggiornamenti {
    /// Come si sveglia il filo.
    sveglia: Sender<Sveglia>,

    /// C'è già un controllo in corso.
    ///
    /// Non dietro un mutex, per la stessa ragione di `StatoSincronia::in_corso`:
    /// un comando che chiede lo stato non deve mettersi in coda dietro la
    /// richiesta di rete di cui vuole parlare.
    in_corso: AtomicBool,

    /// Si sta scaricando o installando adesso.
    ///
    /// Distinto da `in_corso` perché sono due cose che l'utente vede in modo
    /// diverso: un controllo dura un secondo e non si mostra, uno scaricamento
    /// dura minuti e ha una barra.
    installazione: AtomicBool,

    /// L'aggiornamento trovato, se ce n'è uno.
    ///
    /// Si tiene l'oggetto intero e non solo il numero di versione: dentro c'è
    /// l'URL già risolto e la firma già letta, e ricontrollare al momento di
    /// installare vorrebbe dire scaricare qualcosa di diverso da quello che
    /// l'utente ha visto annunciato.
    trovato: Mutex<Option<Update>>,

    /// L'esito dell'ultimo controllo automatico. `None` vuol dire riuscito.
    ultimo_errore: Mutex<Option<ErroreIpc>>,
}

impl StatoAggiornamenti {
    /// Costruisce lo stato e il capo del canale che il filo dovrà ascoltare.
    #[must_use]
    pub fn nuovo() -> (Self, Receiver<Sveglia>) {
        let (sveglia, orecchio) = channel();
        (
            Self {
                sveglia,
                in_corso: AtomicBool::new(false),
                installazione: AtomicBool::new(false),
                trovato: Mutex::new(None),
                ultimo_errore: Mutex::new(None),
            },
            orecchio,
        )
    }
}

// ── quel che la finestra vede ───────────────────────────────────────────────

/// Lo stato del controllo aggiornamenti, come lo mostrano le Impostazioni.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoAggiornamentiIpc {
    /// Il controllo periodico è acceso.
    pub attivo: bool,
    /// Questa copia di Aether ha una chiave pubblica con cui verificare.
    ///
    /// Falso in un albero in cui le chiavi non sono state generate, e la
    /// finestra lo usa per non mostrare un interruttore che non comanda niente.
    pub configurato: bool,
    /// La versione installata adesso.
    pub versione_corrente: String,
    /// Quando è finito l'ultimo controllo riuscito.
    pub ultimo_ms: Option<i64>,
    /// L'aggiornamento trovato, se c'è.
    pub disponibile: Option<AggiornamentoIpc>,
    /// Un controllo è in corso adesso.
    pub in_corso: bool,
    /// Uno scaricamento è in corso adesso.
    pub installazione: bool,
    /// L'ultimo guasto, se c'è stato.
    pub errore: Option<ErroreIpc>,
}

/// Un aggiornamento disponibile, per la finestra.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AggiornamentoIpc {
    /// Il numero di versione annunciato.
    pub versione: String,
    /// Le note di rilascio, se il manifesto ne porta.
    pub note: Option<String>,
    /// Quando è stato pubblicato.
    pub data_ms: Option<i64>,
    /// L'utente ha già detto «non ora» per **questa** versione.
    pub saltata: bool,
}

/// Quanto è arrivato di uno scaricamento in corso.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvanzamentoIpc {
    /// I byte già scesi.
    pub scaricati: u64,
    /// Quanti se ne aspettano in tutto, quando il server lo dice.
    pub totale: Option<u64>,
}

// ── i comandi ───────────────────────────────────────────────────────────────

/// Lo stato del controllo aggiornamenti.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
#[tauri::command]
pub fn aggiornamenti_stato(
    app: AppHandle,
    stato: State<'_, Stato>,
    aggiornamenti: State<'_, StatoAggiornamenti>,
) -> Esito<StatoAggiornamentiIpc> {
    stato_ipc(&app, &stato, &aggiornamenti).map_err(errore)
}

/// Accende o spegne il controllo periodico.
///
/// Accendendolo parte subito un controllo, per la stessa ragione della
/// sincronia: chi ha appena acceso un interruttore ha il diritto di vedere che
/// è successo qualcosa, e aspettare mezz'ora senza segnali è indistinguibile da
/// un interruttore rotto.
///
/// Spegnendolo si butta via anche l'aggiornamento già trovato: lasciare
/// l'avviso in piedi dopo che qualcuno ha detto di non voler essere avvisato
/// sarebbe rispondere «va bene» e continuare come prima.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce.
#[tauri::command]
pub fn aggiornamenti_attivo(
    app: AppHandle,
    stato: State<'_, Stato>,
    aggiornamenti: State<'_, StatoAggiornamenti>,
    attivo: bool,
) -> Esito<StatoAggiornamentiIpc> {
    con_libreria(&stato, |libreria| {
        settings::write(
            &libreria.connection,
            CHIAVE_ATTIVO,
            if attivo { "1" } else { "0" },
        )
    })
    .map_err(errore)?;
    if attivo {
        let _ = aggiornamenti.sveglia.send(Sveglia::Subito);
    } else {
        deposita(&aggiornamenti, None);
        azzera_errore(&aggiornamenti);
    }
    stato_ipc(&app, &stato, &aggiornamenti).map_err(errore)
}

/// Controlla adesso, senza aspettare il turno di mezz'ora.
///
/// Sveglia il filo invece di controllare qui: il controllo è una richiesta di
/// rete con una scadenza di secondi, e farla dentro il comando vorrebbe dire
/// tenere occupato un thread dell'IPC mentre la finestra aspetta una risposta
/// che non contiene niente. Quel che l'utente vede arriva dall'evento
/// `aggiornamenti:stato`, come per il controllo automatico — una via sola, non
/// due che possono raccontare cose diverse.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
#[tauri::command]
pub fn aggiornamenti_adesso(
    app: AppHandle,
    stato: State<'_, Stato>,
    aggiornamenti: State<'_, StatoAggiornamenti>,
) -> Esito<StatoAggiornamentiIpc> {
    let _ = aggiornamenti.sveglia.send(Sveglia::Subito);
    stato_ipc(&app, &stato, &aggiornamenti).map_err(errore)
}

/// Dimentica questa versione finché non ne esce un'altra.
///
/// Non spegne niente: al prossimo controllo la richiesta parte lo stesso, e se
/// nel frattempo è uscita la 0.3.0 l'avviso torna. Quel che sparisce è
/// **quella** versione, e solo quella.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce.
#[tauri::command]
pub fn aggiornamenti_salta(
    app: AppHandle,
    stato: State<'_, Stato>,
    aggiornamenti: State<'_, StatoAggiornamenti>,
    versione: String,
) -> Esito<StatoAggiornamentiIpc> {
    con_libreria(&stato, |libreria| {
        settings::write(&libreria.connection, CHIAVE_SALTATA, &versione)
    })
    .map_err(errore)?;
    stato_ipc(&app, &stato, &aggiornamenti).map_err(errore)
}

/// Scarica l'aggiornamento trovato, ne verifica la firma e lo installa.
///
/// Torna subito: il lavoro vero sta su un filo suo, e quel che succede si
/// racconta con `aggiornamenti:avanzamento`. Su Windows, con
/// `installMode: "passive"`, è l'installer NSIS a chiudere Aether e a
/// riaprirlo — quindi da qui in poi questo processo non ha più niente da fare
/// se non lasciare il dispositivo audio e uscire di scena.
///
/// # Errori
///
/// `internal.aborted` se non c'è nessun aggiornamento da installare, o se ce
/// n'è già uno in corso.
#[tauri::command]
pub fn aggiornamenti_installa(
    app: AppHandle,
    aggiornamenti: State<'_, StatoAggiornamenti>,
) -> Esito<()> {
    let Some(aggiornamento) = aggiornamenti
        .trovato
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
    else {
        return Err(errore(AppError::new(ErrorCode::InternalAborted {
            what: Some("nessun aggiornamento da installare".to_owned()),
        })));
    };
    // Il bit si alza **qui**, nel comando, e non nel filo: fra il click e
    // l'avvio del thread c'è abbastanza tempo perché un secondo click passi, e
    // due scaricamenti dello stesso installer che finiscono con due installer
    // che partono insieme non è una cosa che si vuole scoprire sul campo.
    if aggiornamenti
        .installazione
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(errore(AppError::new(ErrorCode::InternalAborted {
            what: Some("uno scaricamento è già in corso".to_owned()),
        })));
    }
    riferisci(&app);

    let suo = app.clone();
    let avviato = std::thread::Builder::new()
        .name("aether-aggiornamento".to_owned())
        .spawn(move || {
            scarica_e_installa(&suo, &aggiornamento);
        });
    if let Err(err) = avviato {
        // Il bit va rimesso giù, o il tasto resta spento per sempre senza che
        // sia successo niente. Capita solo se il sistema operativo non dà più
        // thread, cioè quando l'applicazione ha guai ben più grossi di un
        // aggiornamento non partito — ma «più grossi» non vuol dire che qui si
        // possa lasciare uno stato che mente.
        aggiornamenti.installazione.store(false, Ordering::Release);
        nota!("[aggiornamenti] il filo dell'installazione non è partito: {err}");
        riferisci(&app);
        return Err(errore(
            AppError::new(ErrorCode::InternalUnexpected {
                detail: Some("il filo dell'installazione non è partito".to_owned()),
            })
            .with_cause(err.to_string()),
        ));
    }
    Ok(())
}

// ── il filo di sottofondo ───────────────────────────────────────────────────

/// Avvia il filo che controlla da solo.
pub fn avvia_filo(app: AppHandle, orecchio: Receiver<Sveglia>) {
    let avviato = std::thread::Builder::new()
        .name("aether-aggiornamenti".to_owned())
        .spawn(move || {
            match orecchio.recv_timeout(ATTESA_AVVIO) {
                Err(RecvTimeoutError::Disconnected) => return,
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            }
            loop {
                passata(&app);
                match orecchio.recv_timeout(INTERVALLO) {
                    Err(RecvTimeoutError::Disconnected) => return,
                    Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                }
            }
        });
    if let Err(err) = avviato {
        nota!("[avvio] il filo degli aggiornamenti non è partito: {err}");
    }
}

/// Un controllo automatico. Non fallisce mai rumorosamente.
fn passata(app: &AppHandle) {
    if !configurato(app) {
        return;
    }
    let acceso = app
        .try_state::<Stato>()
        .and_then(|stato| con_libreria(&stato, |libreria| attivo(&libreria.connection)).ok())
        .unwrap_or(true);
    if !acceso {
        return;
    }
    let Some(aggiornamenti) = app.try_state::<StatoAggiornamenti>() else {
        return;
    };
    // Se un controllo è già in corso — l'utente ha premuto «controlla adesso»
    // proprio mentre scadeva la mezz'ora — questo giro salta e basta.
    let Some(_turno) = Turno::prendi(&aggiornamenti.in_corso) else {
        return;
    };
    riferisci(app);

    let esito = controlla(app);
    match esito {
        Ok(trovato) => {
            deposita(&aggiornamenti, trovato);
            azzera_errore(&aggiornamenti);
            if let Some(stato) = app.try_state::<Stato>() {
                let _ = con_libreria(&stato, |libreria| {
                    settings::write(
                        &libreria.connection,
                        CHIAVE_ULTIMO,
                        &adesso_ms().to_string(),
                    )
                });
            }
        }
        Err(err) => {
            nota!(
                "[aggiornamenti] controllo non riuscito codice={} causa={}",
                err.code().kind().code(),
                err.cause().unwrap_or("—")
            );
            let mut ultimo = aggiornamenti
                .ultimo_errore
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *ultimo = Some(ErroreIpc::from(err));
        }
    }
    drop(_turno);
    riferisci(app);
}

/// La richiesta vera e propria.
///
/// `block_on` e non `spawn`: siamo su uno `std::thread` nostro, fuori dal
/// runtime asincrono di Tauri, ed è esattamente il caso in cui bloccare è
/// lecito. Bloccare *dentro* il runtime sarebbe un'altra cosa — occuperebbe un
/// worker che serve a tutto il resto — e non succede, perché questo filo non ci
/// è mai entrato.
fn controlla(app: &AppHandle) -> Result<Option<Update>, AppError> {
    let updater = costruisci(app)?;
    tauri::async_runtime::block_on(updater.check()).map_err(guasto)
}

/// Costruisce l'updater, con l'endpoint di prova se ce n'è uno.
fn costruisci(app: &AppHandle) -> Result<Updater, AppError> {
    let costruttore = app.updater_builder();
    #[cfg(debug_assertions)]
    let costruttore = match std::env::var(VARIABILE_ENDPOINT) {
        Ok(scelto) if !scelto.trim().is_empty() => {
            let indirizzo: tauri::Url = scelto.trim().parse().map_err(|err| {
                AppError::new(ErrorCode::InternalUnexpected {
                    detail: Some(format!("{VARIABILE_ENDPOINT} non è un URL: {err}")),
                })
            })?;
            nota!("[aggiornamenti] endpoint sostituito: {indirizzo}");
            costruttore.endpoints(vec![indirizzo]).map_err(guasto)?
        }
        _ => costruttore,
    };
    costruttore.build().map_err(guasto)
}

/// Scarica, verifica e installa. Gira su un filo suo e non torna, se riesce.
fn scarica_e_installa(app: &AppHandle, aggiornamento: &Update) {
    // Fermare la musica prima che l'installer prenda in mano il processo. Il
    // plugin, su Windows, esce con `process::exit(0)`: senza questa riga il
    // dispositivo audio si chiuderebbe di colpo a metà di un brano, con il
    // rumore che ne consegue, e la posizione dell'ascolto in corso non
    // verrebbe scritta da nessuna parte.
    fermare_la_musica(app);

    let mut scaricati: u64 = 0;
    let mut ultimo_detto = Instant::now()
        .checked_sub(RESPIRO_AVANZAMENTO)
        .unwrap_or_else(Instant::now);
    let per_pezzo = |quanto: usize, totale: Option<u64>| {
        scaricati = scaricati.saturating_add(u64::try_from(quanto).unwrap_or(0));
        // Il primo pezzo e l'ultimo si dicono sempre; quelli in mezzo con un
        // respiro, o la finestra riceverebbe migliaia di eventi per disegnare
        // una barra che si muove di un pixel.
        let adesso = Instant::now();
        let finito = totale.is_some_and(|totale| scaricati >= totale);
        if finito || adesso.duration_since(ultimo_detto) >= RESPIRO_AVANZAMENTO {
            ultimo_detto = adesso;
            app.emetti(
                "aggiornamenti:avanzamento",
                AvanzamentoIpc { scaricati, totale },
            );
        }
    };

    let esito =
        tauri::async_runtime::block_on(aggiornamento.download_and_install(per_pezzo, || {}));

    // Se si arriva qui su Windows, è andata male: l'installazione riuscita non
    // torna mai, perché il plugin fa uscire il processo.
    if let Some(aggiornamenti) = app.try_state::<StatoAggiornamenti>() {
        aggiornamenti.installazione.store(false, Ordering::Release);
        if let Err(err) = esito {
            let err = guasto(err);
            nota!(
                "[aggiornamenti] installazione non riuscita codice={} causa={}",
                err.code().kind().code(),
                err.cause().unwrap_or("—")
            );
            let mut ultimo = aggiornamenti
                .ultimo_errore
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *ultimo = Some(ErroreIpc::from(err));
        }
    }
    riferisci(app);
}

/// Mette in pausa, se c'è un lettore e se sta suonando.
fn fermare_la_musica(app: &AppHandle) {
    if app
        .try_state::<crate::riproduzione::StatoLettore>()
        .is_none()
    {
        return;
    }
    let lettore = app.state::<crate::riproduzione::StatoLettore>();
    let _ = crate::riproduzione::pausa(app.clone(), lettore);
}

/// Manda alla finestra lo stato aggiornato.
fn riferisci(app: &AppHandle) {
    let (Some(stato), Some(aggiornamenti)) = (
        app.try_state::<Stato>(),
        app.try_state::<StatoAggiornamenti>(),
    ) else {
        return;
    };
    if let Ok(ipc) = stato_ipc(app, &stato, &aggiornamenti) {
        app.emetti("aggiornamenti:stato", ipc);
    }
}

// ── i pezzi condivisi ───────────────────────────────────────────────────────

/// Il controllo periodico è acceso?
///
/// Assente vuol dire sì: è il valore di serie, dichiarato in `PRIVACY.md`.
fn attivo(connection: &rusqlite::Connection) -> Result<bool, AppError> {
    Ok(settings::read(connection, CHIAVE_ATTIVO)?.as_deref() != Some("0"))
}

/// C'è una chiave pubblica con cui verificare le firme?
///
/// Si legge dalla configurazione compilata dentro l'eseguibile, che è l'unico
/// posto dove sta: `plugins.updater.pubkey`. Vuota vuol dire che questo albero
/// non ha ancora chiavi di firma, e allora il controllo non parte affatto.
fn configurato(app: &AppHandle) -> bool {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|conf| conf.get("pubkey"))
        .and_then(serde_json::Value::as_str)
        .is_some_and(|chiave| !chiave.trim().is_empty())
}

/// Mette via l'aggiornamento trovato, o lo toglie.
fn deposita(aggiornamenti: &StatoAggiornamenti, trovato: Option<Update>) {
    let mut posto = aggiornamenti
        .trovato
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *posto = trovato;
}

/// Dimentica l'ultimo guasto.
fn azzera_errore(aggiornamenti: &StatoAggiornamenti) {
    let mut ultimo = aggiornamenti
        .ultimo_errore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *ultimo = None;
}

/// Da un guasto del plugin a un errore del catalogo.
///
/// I codici sono quelli che esistono già: aggiungerne di nuovi al catalogo
/// vorrebbe dire una chiave di traduzione per ognuno in ogni lingua, per
/// distinguere fra loro dei casi che l'utente vede allo stesso modo — «non ho
/// potuto controllare». Quel che invece va distinto è il **tipo**: una rete che
/// non risponde si ritenta da sola fra mezz'ora, una firma che non torna no, e
/// quella differenza è già dentro `retryable`.
fn guasto(err: tauri_plugin_updater::Error) -> AppError {
    use tauri_plugin_updater::Error as Guasto;
    let dettaglio = err.to_string();
    match err {
        // La rete: succede, si ritenta al giro dopo e non si dice niente a
        // nessuno.
        Guasto::Reqwest(_) | Guasto::Network(_) | Guasto::Io(_) => {
            AppError::new(ErrorCode::NetOffline { url: None }).with_cause(dettaglio)
        }
        // Il manifesto non ha la forma attesa, o non parla di questa
        // piattaforma. È un guasto di chi pubblica, non di chi installa.
        Guasto::ReleaseNotFound
        | Guasto::Serialization(_)
        | Guasto::Semver(_)
        | Guasto::TargetNotFound(_)
        | Guasto::TargetsNotFound(_)
        | Guasto::EmptyEndpoints => AppError::new(ErrorCode::NetBadSchema {
            service: Some("aggiornamenti".to_owned()),
            detail: Some(dettaglio.clone()),
        })
        .with_cause(dettaglio),
        // La firma. Sta insieme al resto e non ha un codice suo, e quel che la
        // distingue nel log è la causa: «The signature verification failed».
        // La parola «minisign» lì dentro **non c'è** — la variante del plugin
        // è `#[error(transparent)]` e lascia passare tale e quale il messaggio
        // di `minisign_verify` — quindi cercare quella non darebbe niente, ed
        // è l'errore che si farebbe proprio nel momento in cui si ha fretta.
        // È questa riga che va guardata se un aggiornamento non si installa su
        // una macchina sola: vuol dire che quel che è arrivato lì non l'abbiamo
        // firmato noi.
        altro => AppError::new(ErrorCode::InternalUnexpected {
            detail: Some(altro.to_string()),
        })
        .with_cause(dettaglio),
    }
}

/// Lo stato, nella forma che la finestra riceve.
fn stato_ipc(
    app: &AppHandle,
    stato: &Stato,
    aggiornamenti: &StatoAggiornamenti,
) -> Result<StatoAggiornamentiIpc, AppError> {
    let (acceso, ultimo_ms, saltata) = con_libreria(stato, |libreria| {
        let acceso = attivo(&libreria.connection)?;
        let ultimo_ms = settings::read(&libreria.connection, CHIAVE_ULTIMO)?
            .and_then(|testo| testo.parse::<i64>().ok());
        let saltata = settings::read(&libreria.connection, CHIAVE_SALTATA)?;
        Ok((acceso, ultimo_ms, saltata))
    })?;

    let disponibile = aggiornamenti
        .trovato
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .map(|trovato| AggiornamentoIpc {
            versione: trovato.version.clone(),
            note: trovato
                .body
                .as_deref()
                .map(str::trim)
                .filter(|note| !note.is_empty())
                .map(ToOwned::to_owned),
            // Da secondi a millisecondi, che è l'unità di ogni altro istante
            // che attraversa questo IPC.
            data_ms: trovato
                .date
                .map(|quando| quando.unix_timestamp().saturating_mul(1_000)),
            saltata: saltata.as_deref() == Some(trovato.version.as_str()),
        });

    let errore = aggiornamenti
        .ultimo_errore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();

    Ok(StatoAggiornamentiIpc {
        attivo: acceso,
        configurato: configurato(app),
        versione_corrente: app.package_info().version.to_string(),
        ultimo_ms,
        disponibile,
        in_corso: aggiornamenti.in_corso.load(Ordering::Relaxed),
        installazione: aggiornamenti.installazione.load(Ordering::Relaxed),
        errore,
    })
}
