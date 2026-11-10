//! Gli aggiornamenti, su Android.
//!
//! L'updater di Tauri su Android non esiste: un'applicazione non si sostituisce
//! da sola, la installa il sistema, e solo dopo che chi la usa lo ha
//! confermato. Qui Rymembra fa le due cose che restano da fare a lei:
//! **accorgersi** che è uscita una versione nuova, e **portare giù** l'APK per
//! consegnarlo all'installatore di Android.
//!
//! # Da dove
//!
//! Dalle release di GitHub dello stesso progetto che l'updater del computer
//! legge: l'indirizzo si ricava da `plugins.updater.endpoints` di
//! `tauri.conf.json` (vedi [`progetto`]), così il posto da cui si aggiorna è
//! scritto una volta sola. Una release senza un APK per questo telefono non è
//! un aggiornamento: è una versione per il computer, e qui non si annuncia.
//!
//! # Che cosa si verifica
//!
//! Sul computer l'installer è firmato con una chiave nostra, e l'updater ne
//! verifica la firma prima di eseguirlo. Sul telefono la chiave è quella che
//! firma l'APK, e la verifica sta in tre passi:
//!
//! - l'APK si annuncia solo se si scarica da GitHub
//!   ([`crate::rilasci::allegato_di_github`]), e lo scaricamento segue i
//!   rimandi solo verso GitHub ([`rimando_di_github`]), in HTTPS;
//! - prima di consegnarlo, `Installatore.kt` lo apre e pretende che sia
//!   **questa** app — lo stesso pacchetto — firmata con lo stesso certificato
//!   di quella installata; se no lo cancella, e l'errore torna qui;
//! - l'installatore di Android, alla fine, lo mette sopra Rymembra solo se la
//!   firma è la stessa.
//!
//! Il secondo passo esiste perché il terzo protegge solo il pacchetto che c'è
//! già: un APK con un altro nome di pacchetto per Android non è un
//! aggiornamento ma un'app nuova, e la proporrebbe da installare a un tocco.
//!
//! # I tempi
//!
//! Gli stessi principi del computer, misurati per un telefono: un controllo
//! poco dopo l'apertura, se l'ultimo non è di poche ore fa, poi uno ogni
//! [`INTERVALLO`] finché l'app resta viva — che su un telefono, con la musica
//! sullo sfondo, può voler dire giorni. Senza rete si riprova presto, poi si
//! torna al passo normale.
//!
//! Le firme dei comandi sono quelle di `crate::aggiornamenti` sul computer,
//! parola per parola: il registro dei comandi è uno solo.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use rymembra_app::settings;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_net::{Cancello, Corpo, Metodo, Rete, Richiesta, Rimandi};
use serde::Serialize;
use tauri::{AppHandle, Manager as _, State};

use crate::errore::{ErroreIpc, Esito, errore};
use crate::nota;
use crate::rilasci::{Rilascio, Trovato, progetto_da, rimando_di_github, scegli, troppo_presto};
use crate::spegnimento::Emette as _;
use crate::stato::{Stato, Turno, adesso_ms, con_libreria};

/// Le stesse chiavi del computer: è la stessa preferenza.
const CHIAVE_ATTIVO: &str = "aggiornamenti.attivo";
/// Quando è finito l'ultimo controllo riuscito, in millisecondi.
const CHIAVE_ULTIMO: &str = "aggiornamenti.ultimo_ms";
/// La versione per cui si è detto «non ora».
const CHIAVE_SALTATA: &str = "aggiornamenti.saltata";

/// Ogni quanto si ricontrolla finché l'app resta viva.
const INTERVALLO: Duration = Duration::from_secs(6 * 60 * 60);
/// Quanto dopo `pronto` parte il controllo d'avvio: l'avvio ha già abbastanza
/// da fare.
const DOPO_PRONTO: Duration = Duration::from_secs(10);
/// Quanto si aspetta `pronto` prima di controllare comunque.
const ATTESA_PRONTO_MASSIMA: Duration = Duration::from_secs(60);
/// Un controllo d'avvio più vicino di così all'ultimo riuscito si salta: chi
/// apre l'app dieci volte al giorno non fa dieci richieste.
const GUARDIA: Duration = Duration::from_secs(6 * 60 * 60);
/// Senza rete: si riprova a uno, cinque e quindici minuti, poi al passo.
const RIPROVE_SENZA_RETE: [Duration; 3] = [
    Duration::from_secs(60),
    Duration::from_secs(5 * 60),
    Duration::from_secs(15 * 60),
];
/// La scadenza della domanda a GitHub.
const SCADENZA_RICHIESTA: Duration = Duration::from_secs(20);
/// Ogni quanto si racconta l'avanzamento dello scaricamento.
const RESPIRO_AVANZAMENTO: Duration = Duration::from_millis(200);
/// Quanti rimandi si seguono scaricando l'APK: GitHub ne fa uno, verso chi
/// serve i byte; uno in più per un progetto rinominato, e uno di margine.
const RIMANDI_DELL_APK: u32 = 3;

/// Perché il filo si è svegliato.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sveglia {
    /// Qualcuno ha chiesto di controllare adesso.
    Subito,
    /// La finestra si è mostrata.
    Pronto,
}

/// Quel che il controllo aggiornamenti tiene aperto mentre l'app gira.
pub struct StatoAggiornamenti {
    sveglia: Sender<Sveglia>,
    in_corso: AtomicBool,
    installazione: AtomicBool,
    trovato: Mutex<Option<Trovato>>,
    ultimo_errore: Mutex<Option<ErroreIpc>>,
    annulla: AtomicBool,
    pronto_detto: AtomicBool,
    /// Quante volte l'interruttore è stato spento: una risposta arrivata dopo
    /// lo spegnimento non rimette l'avviso. Vedi il computer.
    generazione: AtomicU64,
}

impl StatoAggiornamenti {
    /// Lo stato, e il capo del canale che il filo ascolta.
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
                annulla: AtomicBool::new(false),
                pronto_detto: AtomicBool::new(false),
                generazione: AtomicU64::new(0),
            },
            orecchio,
        )
    }
}

/// La finestra si è mostrata: comincia il conto del controllo d'avvio. Solo la
/// prima volta conta.
pub fn finestra_pronta(app: &AppHandle) {
    if let Some(aggiornamenti) = app.try_state::<StatoAggiornamenti>()
        && !aggiornamenti.pronto_detto.swap(true, Ordering::AcqRel)
    {
        let _ = aggiornamenti.sveglia.send(Sveglia::Pronto);
    }
}

// ── quel che la finestra vede ───────────────────────────────────────────────

/// Lo stato del controllo aggiornamenti, nella stessa forma del computer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoAggiornamentiIpc {
    /// Il controllo periodico è acceso.
    pub attivo: bool,
    /// Questa copia sa dove guardare: c'è un progetto GitHub nella
    /// configurazione.
    pub configurato: bool,
    /// Sul telefono nessun negozio tiene aggiornata Rymembra.
    pub gestito_dal_store: bool,
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
    /// L'ultimo guasto.
    pub errore: Option<ErroreIpc>,
}

/// Un aggiornamento disponibile, per la finestra.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AggiornamentoIpc {
    /// Il numero di versione annunciato.
    pub versione: String,
    /// Le note di rilascio.
    pub note: Option<String>,
    /// Quando è stato pubblicato.
    pub data_ms: Option<i64>,
    /// L'utente ha già detto «non ora» per questa versione.
    pub saltata: bool,
}

/// Quanto è arrivato di uno scaricamento in corso.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct AvanzamentoIpc {
    scaricati: u64,
    totale: Option<u64>,
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

/// Accende o spegne il controllo periodico. Accendendolo si controlla subito;
/// spegnendolo l'avviso già trovato se ne va.
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
        aggiornamenti.generazione.fetch_add(1, Ordering::AcqRel);
        *aggiornamenti
            .trovato
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        *aggiornamenti
            .ultimo_errore
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
    }
    riferisci(&app);
    stato_ipc(&app, &stato, &aggiornamenti).map_err(errore)
}

/// Controlla adesso: sveglia il filo, e l'esito arriva con
/// `aggiornamenti:stato`.
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
    riferisci(&app);
    stato_ipc(&app, &stato, &aggiornamenti).map_err(errore)
}

/// Scarica l'APK trovato e lo consegna all'installatore di Android.
///
/// Torna subito: lo scaricamento sta su un filo suo e si racconta con
/// `aggiornamenti:avanzamento`. Da lì in poi decide chi usa il telefono: il
/// sistema gli chiede di confermare, e la prima volta di permettere a Rymembra
/// di installare app — vedi [`consegna`].
///
/// # Errori
///
/// `internal.aborted` se non c'è niente da installare, o se uno scaricamento
/// è già in corso.
#[tauri::command]
pub fn aggiornamenti_installa(
    app: AppHandle,
    aggiornamenti: State<'_, StatoAggiornamenti>,
) -> Esito<()> {
    let Some(trovato) = aggiornamenti
        .trovato
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
    else {
        return Err(errore(AppError::new(ErrorCode::InternalAborted {
            what: Some("nessun aggiornamento da installare".to_owned()),
        })));
    };
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
    if let Err(err) = std::thread::Builder::new()
        .name("rymembra-aggiornamento".to_owned())
        .spawn(move || scarica_e_consegna(&suo, &trovato))
    {
        aggiornamenti.installazione.store(false, Ordering::Release);
        riferisci(&app);
        return Err(errore(
            AppError::new(ErrorCode::InternalUnexpected {
                detail: Some("il filo dell'aggiornamento non è partito".to_owned()),
            })
            .with_cause(err.to_string()),
        ));
    }
    Ok(())
}

/// Ferma lo scaricamento in corso. Dopo che l'APK è arrivato non ferma più
/// niente: l'installatore è del sistema.
#[tauri::command]
pub fn aggiornamenti_annulla(aggiornamenti: State<'_, StatoAggiornamenti>) -> Esito<()> {
    aggiornamenti.annulla.store(true, Ordering::Release);
    Ok(())
}

// ── il filo di sottofondo ───────────────────────────────────────────────────

/// Come è andato un giro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Giro {
    Riuscito,
    SenzaRete,
    Fallito,
    Saltato,
}

/// Avvia il filo che controlla da solo.
pub fn avvia_filo(app: AppHandle, orecchio: Receiver<Sveglia>) {
    let avviato = std::thread::Builder::new()
        .name("rymembra-aggiornamenti".to_owned())
        .spawn(move || {
            let Some(mut a_mano) = aspetta_l_avvio(&orecchio) else {
                return;
            };
            let mut guardia = !a_mano;
            let mut riprove = 0_usize;
            loop {
                if crate::spegnimento::in_uscita() {
                    return;
                }
                let guardia_ms = i64::try_from(GUARDIA.as_millis()).unwrap_or(i64::MAX);
                let giro = if guardia
                    && troppo_presto(ultimo_controllo(&app), adesso_ms(), guardia_ms)
                {
                    Giro::Saltato
                } else {
                    crate::stato::sorvegliato("rymembra-aggiornamenti", || passata(&app, a_mano))
                        .unwrap_or(Giro::Fallito)
                };
                guardia = false;
                let (attesa, dopo) = prossima_attesa(giro, riprove);
                riprove = dopo;
                match aspetta(&orecchio, attesa) {
                    None => return,
                    Some(chiesto) => a_mano = chiesto,
                }
            }
        });
    if let Err(err) = avviato {
        nota!("[avvio] il filo degli aggiornamenti non è partito: {err}");
    }
}

/// `pronto` più [`DOPO_PRONTO`], o [`ATTESA_PRONTO_MASSIMA`], o subito se
/// qualcuno preme «Controlla adesso». `None` se si sta uscendo.
fn aspetta_l_avvio(orecchio: &Receiver<Sveglia>) -> Option<bool> {
    match orecchio.recv_timeout(ATTESA_PRONTO_MASSIMA) {
        Err(RecvTimeoutError::Disconnected) => None,
        Err(RecvTimeoutError::Timeout) => Some(false),
        Ok(Sveglia::Subito) => Some(true),
        Ok(Sveglia::Pronto) => aspetta(orecchio, DOPO_PRONTO),
    }
}

/// Aspetta `quanto`, o una sveglia a mano. `Some(true)` se è stata premuta.
fn aspetta(orecchio: &Receiver<Sveglia>, quanto: Duration) -> Option<bool> {
    let fine = Instant::now() + quanto;
    loop {
        match orecchio.recv_timeout(fine.saturating_duration_since(Instant::now())) {
            Err(RecvTimeoutError::Disconnected) => return None,
            Err(RecvTimeoutError::Timeout) => return Some(false),
            Ok(Sveglia::Subito) => return Some(true),
            Ok(Sveglia::Pronto) => {}
        }
    }
}

/// Quanto aspettare dopo un giro, e quante riprove senza rete restano.
fn prossima_attesa(giro: Giro, riprove: usize) -> (Duration, usize) {
    match giro {
        Giro::SenzaRete => RIPROVE_SENZA_RETE
            .get(riprove)
            .map_or((INTERVALLO, riprove), |attesa| {
                (*attesa, riprove.saturating_add(1))
            }),
        Giro::Riuscito | Giro::Fallito | Giro::Saltato => (INTERVALLO, 0),
    }
}

fn ultimo_controllo(app: &AppHandle) -> Option<i64> {
    let stato = app.try_state::<Stato>()?;
    con_libreria(&stato, |libreria| {
        Ok(settings::read(&libreria.connection, CHIAVE_ULTIMO)?
            .and_then(|testo| testo.parse::<i64>().ok()))
    })
    .ok()
    .flatten()
}

/// Un giro: controlla, deposita quel che ha trovato, racconta.
fn passata(app: &AppHandle, a_mano: bool) -> Giro {
    let Some(progetto) = progetto(app) else {
        return Giro::Saltato;
    };
    let Some(aggiornamenti) = app.try_state::<StatoAggiornamenti>() else {
        return Giro::Saltato;
    };
    let partenza = aggiornamenti.generazione.load(Ordering::Acquire);
    let acceso = a_mano
        || app
            .try_state::<Stato>()
            .and_then(|stato| {
                con_libreria(&stato, |libreria| {
                    Ok(
                        settings::read(&libreria.connection, CHIAVE_ATTIVO)?.as_deref()
                            != Some("0"),
                    )
                })
                .ok()
            })
            .unwrap_or(true);
    if !acceso {
        return Giro::Saltato;
    }
    let Some(turno) = Turno::prendi(&aggiornamenti.in_corso) else {
        return Giro::Saltato;
    };
    riferisci(app);

    let corrente = app.package_info().version.to_string();
    let esito = controlla(&progetto, &corrente);
    let giro = match &esito {
        Ok(_) => Giro::Riuscito,
        Err(err) if err.is_retryable() => Giro::SenzaRete,
        Err(_) => Giro::Fallito,
    };
    match esito {
        Ok(trovato) => {
            if aggiornamenti.generazione.load(Ordering::Acquire) == partenza {
                *aggiornamenti
                    .trovato
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = trovato;
            }
            *aggiornamenti
                .ultimo_errore
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = None;
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
            *aggiornamenti
                .ultimo_errore
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(ErroreIpc::from(err));
        }
    }
    drop(turno);
    riferisci(app);
    giro
}

// ── GitHub ──────────────────────────────────────────────────────────────────

/// `proprietario/progetto` su GitHub, dall'indirizzo dell'updater del computer
/// (`https://github.com/<proprietario>/<progetto>/releases/…`). `None` se la
/// configurazione non ne ha uno: allora questa copia non è configurata.
fn progetto(app: &AppHandle) -> Option<String> {
    let updater = app.config().plugins.0.get("updater")?;
    let indirizzo = updater.get("endpoints")?.as_array()?.first()?.as_str()?;
    progetto_da(indirizzo)
}

/// Chiede a GitHub l'ultima release, e la versione nuova se ce n'è una con un
/// APK per questo telefono.
///
/// # Errori
///
/// `net.*` per la rete; `net.badSchema` per una risposta che non è una release.
fn controlla(progetto: &str, corrente: &str) -> Result<Option<Trovato>, AppError> {
    let url = format!("https://api.github.com/repos/{progetto}/releases/latest");
    let rete = Rete::nuova("aggiornamenti", SCADENZA_RICHIESTA);
    let risposta = rete.esegui(Richiesta {
        metodo: Metodo::Get,
        url: &url,
        intestazioni: &[("Accept", "application/vnd.github+json")],
        corpo: Corpo::Niente,
    })?;
    // Nessuna release pubblicata: niente da annunciare, e non è un guasto.
    if risposta.stato == 404 {
        return Ok(None);
    }
    if !risposta.e_andata() {
        return Err(rete.stato_a_errore(&risposta, &url));
    }
    let rilascio: Rilascio = serde_json::from_slice(&risposta.corpo).map_err(|err| {
        AppError::new(ErrorCode::NetBadSchema {
            service: Some("aggiornamenti".to_owned()),
            detail: Some(err.to_string()),
        })
    })?;
    Ok(scegli(rilascio, corrente, std::env::consts::ARCH))
}

// ── l'installazione ─────────────────────────────────────────────────────────

/// Scarica l'APK nella cache — o riusa quello già sceso — e lo consegna.
fn scarica_e_consegna(app: &AppHandle, trovato: &Trovato) {
    let Some(aggiornamenti) = app.try_state::<StatoAggiornamenti>() else {
        return;
    };
    let turno = Turno::adotta(&aggiornamenti.installazione);
    aggiornamenti.annulla.store(false, Ordering::Release);
    let esito = scarica(app, &aggiornamenti, trovato).and_then(|percorso| consegna(&percorso));
    if let Err(err) = esito {
        // Annullato a mano non è un guasto: la fascia torna com'era.
        if !matches!(err.code(), ErrorCode::InternalAborted { .. }) {
            nota!(
                "[aggiornamenti] installazione non riuscita codice={} causa={}",
                err.code().kind().code(),
                err.cause().unwrap_or("—")
            );
            *aggiornamenti
                .ultimo_errore
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(ErroreIpc::from(err));
        }
    }
    drop(turno);
    riferisci(app);
}

/// L'APK nella cache dell'app, scaricato se non c'è già intero.
///
/// Si tiene per nome di file: se la prima volta Android ha chiesto il permesso
/// di installare app, il secondo «Aggiorna» non scarica di nuovo trenta
/// megabyte.
fn scarica(
    app: &AppHandle,
    aggiornamenti: &StatoAggiornamenti,
    trovato: &Trovato,
) -> Result<std::path::PathBuf, AppError> {
    let cache = rymembra_android::sistema::cartella_cache().map_err(|err| {
        AppError::new(ErrorCode::InternalUnexpected {
            detail: Some("la cartella della cache".to_owned()),
        })
        .with_cause(err.to_string())
    })?;
    let cartella = cache.join("aggiornamenti");
    std::fs::create_dir_all(&cartella)
        .map_err(|err| rymembra_app::files::io_error(&cartella.display().to_string(), &err))?;
    // Il nome viene da GitHub: se ne tiene solo l'ultimo pezzo, così un nome
    // con delle barre non esce dalla cartella.
    let nome = std::path::Path::new(&trovato.nome)
        .file_name()
        .map_or_else(|| "rymembra.apk".into(), ToOwned::to_owned);
    let finale = cartella.join(nome);
    if let (Some(peso), Ok(meta)) = (trovato.peso, std::fs::metadata(&finale))
        && meta.len() == peso
    {
        return Ok(finale);
    }
    // I vecchi APK se ne vanno: la cache non deve tenerne uno per versione.
    if let Ok(voci) = std::fs::read_dir(&cartella) {
        for voce in voci.flatten() {
            let _ = std::fs::remove_file(voce.path());
        }
    }
    let parziale = finale.with_extension("apk.parziale");
    let mut file = std::fs::File::create(&parziale)
        .map_err(|err| rymembra_app::files::io_error(&parziale.display().to_string(), &err))?;
    let mut ultimo = Instant::now() - RESPIRO_AVANZAMENTO;
    let mut avanzamento = |scaricati: u64, totale: Option<u64>| {
        if ultimo.elapsed() >= RESPIRO_AVANZAMENTO {
            ultimo = Instant::now();
            app.emetti(
                "aggiornamenti:avanzamento",
                AvanzamentoIpc {
                    scaricati,
                    totale: totale.or(trovato.peso),
                },
            );
        }
    };
    let annullato = || aggiornamenti.annulla.load(Ordering::Acquire);
    // L'indirizzo è di GitHub (`rilasci::scegli`), e GitHub lo serve da
    // un altro ospite suo: si segue quello, e nessun altro.
    let rete = Rete::per_prelievo("aggiornamenti").con_rimandi(Rimandi::Sorvegliati {
        quanti: RIMANDI_DELL_APK,
        cancello: Cancello::nuovo(rimando_di_github),
    });
    let esito = rete.preleva(
        &trovato.indirizzo,
        &[],
        &mut file,
        &annullato,
        &mut avanzamento,
    );
    drop(file);
    match esito {
        Ok(_) => std::fs::rename(&parziale, &finale)
            .map(|()| finale.clone())
            .map_err(|err| rymembra_app::files::io_error(&finale.display().to_string(), &err)),
        Err(err) => {
            let _ = std::fs::remove_file(&parziale);
            Err(err)
        }
    }
}

/// Consegna l'APK all'installatore di Android.
///
/// Prima `Installatore.kt` lo verifica: un APK che non è questa app — un altro
/// pacchetto, o un'altra firma — torna qui come errore, ed è già cancellato,
/// così il prossimo «Aggiorna» non lo riusa.
///
/// Se Rymembra non ha ancora il permesso di installare app, Android apre la
/// pagina dove lo si concede: chi la chiude torna a «Aggiorna», che stavolta
/// non scarica niente e apre l'installatore.
fn consegna(percorso: &std::path::Path) -> Result<(), AppError> {
    match rymembra_android::sistema::installa_apk(percorso) {
        Ok(rymembra_android::sistema::Installazione::Installatore) => Ok(()),
        Ok(rymembra_android::sistema::Installazione::Permesso) => {
            nota!("[aggiornamenti] Android chiede il permesso di installare app");
            Ok(())
        }
        Err(err) => Err(AppError::new(ErrorCode::InternalUnexpected {
            detail: Some("l'installatore di Android".to_owned()),
        })
        .with_cause(err.to_string())),
    }
}

// ── lo stato ────────────────────────────────────────────────────────────────

/// Manda lo stato a chi ascolta, anche a chi non l'ha chiesto.
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

fn stato_ipc(
    app: &AppHandle,
    stato: &Stato,
    aggiornamenti: &StatoAggiornamenti,
) -> Result<StatoAggiornamentiIpc, AppError> {
    let (attivo, ultimo_ms, saltata) = con_libreria(stato, |libreria| {
        let attivo = settings::read(&libreria.connection, CHIAVE_ATTIVO)?.as_deref() != Some("0");
        let ultimo_ms = settings::read(&libreria.connection, CHIAVE_ULTIMO)?
            .and_then(|testo| testo.parse::<i64>().ok());
        let saltata = settings::read(&libreria.connection, CHIAVE_SALTATA)?;
        Ok((attivo, ultimo_ms, saltata))
    })?;
    let disponibile = aggiornamenti
        .trovato
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|trovato| AggiornamentoIpc {
            versione: trovato.versione.clone(),
            note: trovato.note.clone(),
            data_ms: trovato.data_ms,
            saltata: saltata.as_deref() == Some(trovato.versione.as_str()),
        });
    Ok(StatoAggiornamentiIpc {
        attivo,
        configurato: progetto(app).is_some(),
        gestito_dal_store: false,
        versione_corrente: app.package_info().version.to_string(),
        ultimo_ms,
        disponibile,
        in_corso: aggiornamenti.in_corso.load(Ordering::Relaxed),
        installazione: aggiornamenti.installazione.load(Ordering::Relaxed),
        errore: aggiornamenti
            .ultimo_errore
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone(),
    })
}
