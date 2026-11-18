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
//! # Scarico, verifico, «Installa»
//!
//! L'aggiornamento è in tre passi, e l'ultimo è di chi usa il telefono. Il
//! tocco su «Aggiorna» scarica l'APK e lo **verifica** — la firma minisign e il
//! certificato di questa app, due controlli sotto lo stesso «Verifico» — e si
//! ferma a `pronto`. Solo il tocco su «Installa» apre l'installatore di Android,
//! con la sua schermata di conferma: così la schermata di sistema compare quando
//! la persona la sta aspettando, e non mentre guarda altro. Il pacchetto resta
//! in attesa finché l'app vive; se l'installatore si chiude senza installare,
//! «Installa» è ancora lì.
//!
//! # Che cosa si verifica
//!
//! Sul computer l'installer è firmato con una chiave nostra, e l'updater ne
//! verifica la firma prima di eseguirlo. Sul telefono le chiavi sono due —
//! quella dell'updater, la stessa del computer, e quella che firma l'APK per
//! Android — e la verifica sta in quattro passi:
//!
//! - l'APK si annuncia solo se si scarica da GitHub
//!   ([`crate::rilasci::allegato_di_github`]) e ha accanto la sua firma
//!   minisign, e lo scaricamento segue i rimandi solo verso GitHub
//!   ([`rimando_di_github`]), in HTTPS;
//! - prima di consegnarlo, l'APK sul disco si verifica con la firma minisign
//!   e la chiave pubblica dell'updater, per la versione annunciata
//!   ([`crate::rilasci::verifica_la_firma`]): se non torna si cancella, come fa
//!   il computer con un installer;
//! - poi `Installatore.kt` lo apre e pretende che sia **questa** app — lo
//!   stesso pacchetto — firmata con lo stesso certificato di quella
//!   installata; se no lo cancella;
//! - l'installatore di Android, alla fine, lo mette sopra Rymembra solo se la
//!   firma è la stessa.
//!
//! Il terzo passo esiste perché il quarto protegge solo il pacchetto che c'è
//! già: un APK con un altro nome di pacchetto per Android non è un
//! aggiornamento ma un'app nuova, e la proporrebbe da installare a un tocco.
//!
//! # Le copie firmate con un'altra chiave
//!
//! Le copie di prova — compilate in debug, firmate con la chiave di prova di
//! Gradle — non possono ricevere l'APK pubblico: Android lo rifiuta. Lo si sa
//! prima di scaricare, dall'impronta del certificato di questa copia
//! ([`crate::rilasci::copia_di_prova`]); e se non lo si sapeva, il primo rifiuto
//! di `Installatore.kt` si ricorda ([`CHIAVE_RIFIUTATA`]). In tutti e due i casi
//! quella versione non si scarica più — erano decine di megabyte a ogni tocco,
//! e un «Riavvia Rymembra» che non serviva — e la finestra spiega come passare
//! alla versione pubblica senza perdere la libreria (`altraChiave`).
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
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use rymembra_android::sistema::ControlloApk;
use rymembra_app::settings;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_net::{Cancello, Corpo, Metodo, Rete, Richiesta, Rimandi};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager as _, State};

use crate::errore::{ErroreIpc, Esito, errore};
use crate::nota;
use crate::rilasci::{
    Rilascio, Trovato, copia_di_prova, non_installabile, note_dal_manifesto,
    passata_d_avvio_saltata, progetto_da, rimando_di_github, scegli, verifica_la_firma,
};
use crate::spegnimento::Emette as _;
use crate::stato::{Stato, Turno, adesso_ms, con_libreria};

/// Le stesse chiavi del computer: è la stessa preferenza.
const CHIAVE_ATTIVO: &str = "aggiornamenti.attivo";
/// Quando è finito l'ultimo controllo riuscito, in millisecondi.
const CHIAVE_ULTIMO: &str = "aggiornamenti.ultimo_ms";
/// La versione per cui si è detto «non ora».
const CHIAVE_SALTATA: &str = "aggiornamenti.saltata";
/// La versione che l'ultimo controllo riuscito ha trovato.
///
/// L'aggiornamento trovato vive solo in memoria, e la guardia d'avvio salta
/// il controllo se l'ultimo è di meno di [`GUARDIA`] fa: un processo morto
/// dopo l'avviso — su un telefono succede di continuo — si riapriva senza.
/// Qui si ricorda solo che una versione nuova c'era, quanto basta perché la
/// passata d'avvio si faccia comunque (`rilasci::passata_d_avvio_saltata`),
/// che è il solo modo di riavere l'indirizzo dell'APK. Si toglie quando un
/// controllo riuscito non trova niente. La regola, e la chiave, del computer.
const CHIAVE_TROVATA: &str = "aggiornamenti.trovata";
/// La versione il cui APK Android ha rifiutato perché firmato con un'altra
/// chiave: non si riscarica, e la finestra dice perché. Solo del telefono.
const CHIAVE_RIFIUTATA: &str = "aggiornamenti.rifiutata";

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
/// Il tetto della firma minisign: qualche centinaio di byte, e un margine.
const TETTO_DELLA_FIRMA: u64 = 16 * 1024;
/// Il tetto di `latest.json`, per le note: qualche chilobyte, e un margine.
const TETTO_DEL_MANIFESTO: u64 = 256 * 1024;

/// A che passo è l'aggiornamento: gli stessi nomi del computer
/// (`crate::aggiornamenti`), di cui qui se ne usano tre. `Pronto` è l'APK già
/// scaricato e verificato, che aspetta il tocco su «Installa».
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FaseAggiornamento {
    Scarico,
    Verifico,
    Pronto,
}

/// Quando installare. Sul telefono c'è solo `Subito`: scarica e verifica, e la
/// consegna ad Android è un secondo tocco. `AllaChiusura` esiste perché il
/// comando ha la stessa firma del computer, e qui si rifiuta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Quando {
    Subito,
    AllaChiusura,
}

/// L'APK scaricato e verificato che aspetta «Installa».
struct Pronto {
    /// Il file nella cache degli aggiornamenti.
    apk: std::path::PathBuf,
    /// La versione che porta.
    versione: String,
    /// Da quando aspetta, in millisecondi.
    dal_ms: i64,
}

/// La fase che la pagina legge: il passo in corso vince; senza, un APK in
/// attesa vale `Pronto`.
fn fase_dello_stato(
    in_corso: Option<FaseAggiornamento>,
    apk_pronto: bool,
) -> Option<FaseAggiornamento> {
    in_corso.or_else(|| apk_pronto.then_some(FaseAggiornamento::Pronto))
}

/// Cambia il passo e lo racconta alla pagina.
fn imposta_fase(app: &AppHandle, fase: Option<FaseAggiornamento>) {
    if let Some(aggiornamenti) = app.try_state::<StatoAggiornamenti>() {
        *aggiornamenti
            .fase
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = fase;
    }
    riferisci(app);
}

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
    /// A che passo è lo scaricamento o la verifica in corso.
    fase: Mutex<Option<FaseAggiornamento>>,
    /// L'APK verificato che aspetta «Installa».
    pronto: Mutex<Option<Pronto>>,
    ultimo_errore: Mutex<Option<ErroreIpc>>,
    annulla: AtomicBool,
    pronto_detto: AtomicBool,
    /// Quante volte l'interruttore è stato spento: una risposta arrivata dopo
    /// lo spegnimento non rimette l'avviso. Vedi il computer.
    generazione: AtomicU64,
    /// Questa copia è firmata con un'altra chiave di quella del rilascio
    /// ([`crate::rilasci::copia_di_prova`]): si chiede ad Android una volta
    /// sola, la firma di un'app installata non cambia mentre gira.
    copia_di_prova: OnceLock<Option<bool>>,
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
                fase: Mutex::new(None),
                pronto: Mutex::new(None),
                ultimo_errore: Mutex::new(None),
                annulla: AtomicBool::new(false),
                pronto_detto: AtomicBool::new(false),
                generazione: AtomicU64::new(0),
                copia_di_prova: OnceLock::new(),
            },
            orecchio,
        )
    }

    /// Questa copia è firmata con un'altra chiave: vedi [`Self::copia_di_prova`].
    /// `None` se Android non lo dice; allora decide il rifiuto dopo lo
    /// scaricamento, e il diario lo scrive.
    fn copia_di_prova(&self) -> Option<bool> {
        *self.copia_di_prova.get_or_init(|| {
            let esito = match rymembra_android::sistema::firmatari() {
                Ok(firmatari) => copia_di_prova(&firmatari),
                Err(err) => {
                    nota!("[aggiornamenti] la firma di questa copia non si legge: {err}");
                    None
                }
            };
            match esito {
                Some(true) => nota!(
                    "[aggiornamenti] questa copia è firmata con un'altra chiave del rilascio: \
                     gli APK pubblici non le si installano sopra, e non si scaricano"
                ),
                Some(false) => {}
                None => nota!(
                    "[aggiornamenti] la chiave di questa copia non si sa: decide Android, \
                     dopo lo scaricamento"
                ),
            }
            esito
        })
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
    /// A che passo è l'aggiornamento, o `pronto` se l'APK verificato aspetta
    /// «Installa»: vedi [`fase_dello_stato`].
    pub fase: Option<FaseAggiornamento>,
    /// Da quando l'APK verificato aspetta, in millisecondi.
    pub pronto_dal_ms: Option<i64>,
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
    /// L'APK di questa versione è firmato con un'altra chiave di questa copia:
    /// non si scarica, e la finestra spiega come passare alla versione
    /// pubblica. Vedi [`crate::rilasci::non_installabile`].
    pub altra_chiave: bool,
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
        // Anche l'APK già verificato: chi ha detto di non voler essere avvisato
        // non vuole nemmeno un «Installa» che aspetta.
        scarta_pronto(&aggiornamenti);
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
    // Saltare una versione è non volerla: se il suo APK aspettava «Installa», se
    // ne va.
    let aspetta_questa = aggiornamenti
        .pronto
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_some_and(|pronto| pronto.versione == versione);
    if aspetta_questa {
        scarta_pronto(&aggiornamenti);
    }
    riferisci(&app);
    stato_ipc(&app, &stato, &aggiornamenti).map_err(errore)
}

/// Scarica l'APK trovato e lo verifica; con l'APK già verificato in attesa, lo
/// consegna all'installatore di Android.
///
/// Torna subito: il lavoro sta su un filo suo e si racconta con
/// `aggiornamenti:stato` (la `fase`: scarico, verifico, pronto) e
/// `aggiornamenti:avanzamento` (i byte). Sono due tocchi, e il secondo è
/// «Installa»: il sistema gli chiede di confermare, e la prima volta di
/// permettere a Rymembra di installare app — vedi [`consegna`].
///
/// Un APK che Android non installerebbe sopra questa copia — firmato con
/// un'altra chiave ([`crate::rilasci::non_installabile`]) — non si scarica: la
/// finestra non offre il tasto, e il comando rifiuta lo stesso.
///
/// # Errori
///
/// `internal.aborted` se non c'è niente da installare, se uno scaricamento è
/// già in corso, se l'APK di questa versione è firmato con un'altra chiave, o
/// se si chiede `AllaChiusura`, che sul telefono non esiste.
#[tauri::command]
pub fn aggiornamenti_installa(
    app: AppHandle,
    aggiornamenti: State<'_, StatoAggiornamenti>,
    quando: Quando,
) -> Esito<()> {
    if quando == Quando::AllaChiusura {
        return Err(errore(AppError::new(ErrorCode::InternalAborted {
            what: Some("sul telefono non c'è l'installazione alla chiusura".to_owned()),
        })));
    }
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
    if non_installabile(
        aggiornamenti.copia_di_prova(),
        versione_rifiutata(&app).as_deref(),
        &trovato.versione,
    ) {
        return Err(errore(AppError::new(ErrorCode::InternalAborted {
            what: Some(
                "l'APK è firmato con un'altra chiave di questa copia: Android non lo installa sopra"
                    .to_owned(),
            ),
        })));
    }
    if aggiornamenti
        .installazione
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(errore(AppError::new(ErrorCode::InternalAborted {
            what: Some("uno scaricamento è già in corso".to_owned()),
        })));
    }
    // L'APK in attesa di *questa* versione, se c'è: «Installa» lo consegna senza
    // riscaricare niente. Non si toglie dal posto: se l'installatore di Android
    // si chiude senza installare, «Installa» è ancora lì.
    let pronto = aggiornamenti
        .pronto
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .filter(|pronto| pronto.versione == trovato.versione)
        .map(|pronto| pronto.apk.clone());
    riferisci(&app);
    let suo = app.clone();
    if let Err(err) = std::thread::Builder::new()
        .name("rymembra-aggiornamento".to_owned())
        .spawn(move || lavora(&suo, &trovato, pronto))
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

/// Ferma lo scaricamento in corso, o butta l'APK che aspetta «Installa». Dopo
/// che l'APK è arrivato non ferma più niente: l'installatore è del sistema.
///
/// Con un APK in attesa e nessuno scaricamento, «Annulla» è il ripensamento:
/// «Installa» se ne va, e l'avviso torna com'era. Il file resta nella cache, e
/// il prossimo «Aggiorna» lo riusa senza riscaricarlo ([`scarica`]).
#[tauri::command]
pub fn aggiornamenti_annulla(
    app: AppHandle,
    aggiornamenti: State<'_, StatoAggiornamenti>,
) -> Esito<()> {
    if aggiornamenti.installazione.load(Ordering::Acquire) {
        aggiornamenti.annulla.store(true, Ordering::Release);
    } else if scarta_pronto(&aggiornamenti) {
        riferisci(&app);
    }
    Ok(())
}

/// Butta l'APK in attesa, se c'è. `true` se c'era.
fn scarta_pronto(aggiornamenti: &StatoAggiornamenti) -> bool {
    aggiornamenti
        .pronto
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .is_some()
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
                // Il `&&` tiene le letture dal database fuori da ogni giro
                // tranne il primo. La guardia cede se l'ultimo controllo aveva
                // trovato una versione nuova: vedi `CHIAVE_TROVATA`.
                let giro = if guardia
                    && passata_d_avvio_saltata(
                        ultimo_controllo(&app),
                        adesso_ms(),
                        guardia_ms,
                        versione_trovata_prima(&app).as_deref(),
                    ) {
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

/// La versione che l'ultimo controllo riuscito aveva trovato, se il database
/// la ricorda: vedi `CHIAVE_TROVATA`. `None` per tutto quel che non è una
/// risposta, che vuol dire lasciare la guardia com'era.
fn versione_trovata_prima(app: &AppHandle) -> Option<String> {
    let stato = app.try_state::<Stato>()?;
    con_libreria(&stato, |libreria| {
        settings::read(&libreria.connection, CHIAVE_TROVATA)
    })
    .ok()
    .flatten()
}

/// La versione il cui APK Android ha rifiutato per la chiave, se il database la
/// ricorda: vedi [`CHIAVE_RIFIUTATA`].
fn versione_rifiutata(app: &AppHandle) -> Option<String> {
    let stato = app.try_state::<Stato>()?;
    con_libreria(&stato, |libreria| {
        settings::read(&libreria.connection, CHIAVE_RIFIUTATA)
    })
    .ok()
    .flatten()
}

/// Un giro: controlla, deposita quel che ha trovato, racconta.
fn passata(app: &AppHandle, a_mano: bool) -> Giro {
    // Senza la chiave dell'updater un APK non si verifica: annunciarlo sarebbe
    // solo un modo più lungo di fallire. Come sul computer.
    let Some(progetto) = progetto(app).filter(|_| chiave_pubblica(app).is_some()) else {
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
            let versione = trovato.as_ref().map(|nuovo| nuovo.versione.clone());
            // Spento intanto: il controllo è fatto (l'ora si scrive), ma quel
            // che ha trovato non si mostra e non si ricorda.
            let vale = aggiornamenti.generazione.load(Ordering::Acquire) == partenza;
            if vale {
                *aggiornamenti
                    .trovato
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = trovato;
                // Un APK in attesa di una versione che il controllo non annuncia
                // più — è uscita la successiva, o la release è stata ritirata — non
                // si offre da installare.
                let mut pronto = aggiornamenti
                    .pronto
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if pronto
                    .as_ref()
                    .is_some_and(|pronto| versione.as_deref() != Some(pronto.versione.as_str()))
                {
                    *pronto = None;
                }
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
                    )?;
                    match &versione {
                        _ if !vale => Ok(()),
                        Some(versione) => {
                            settings::write(&libreria.connection, CHIAVE_TROVATA, versione)
                        }
                        None => settings::forget(&libreria.connection, CHIAVE_TROVATA).map(|_| ()),
                    }
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

/// La chiave pubblica dell'updater (`plugins.updater.pubkey`): la stessa che
/// verifica gli installer del computer, e qui gli APK. `None` in un albero
/// compilato senza chiavi.
fn chiave_pubblica(app: &AppHandle) -> Option<String> {
    let updater = app.config().plugins.0.get("updater")?;
    updater
        .get("pubkey")?
        .as_str()
        .map(str::trim)
        .filter(|chiave| !chiave.is_empty())
        .map(ToOwned::to_owned)
}

/// Un client che segue i rimandi soltanto verso gli ospiti degli allegati di
/// GitHub: per l'APK, la sua firma e `latest.json`.
fn rete_degli_allegati(rete: &Rete) -> Rete {
    rete.con_rimandi(Rimandi::Sorvegliati {
        quanti: RIMANDI_DELL_APK,
        cancello: Cancello::nuovo(rimando_di_github),
    })
}

/// Le note della versione trovata, dal `latest.json` della stessa release:
/// quelle del computer (vedi `rilasci::note_dal_manifesto`). Un guasto qui non
/// ferma l'avviso: lo lascia senza note, e lo scrive nel diario.
fn note_della_release(indirizzo: &str, versione: &str) -> Option<String> {
    let rete = rete_degli_allegati(&Rete::nuova("aggiornamenti", SCADENZA_RICHIESTA));
    let mut corpo = Vec::new();
    match rete.preleva(
        indirizzo,
        &[],
        &mut corpo,
        TETTO_DEL_MANIFESTO,
        &|| false,
        &mut |_, _| {},
    ) {
        Ok(_) => {
            let note = note_dal_manifesto(&corpo, versione);
            if note.is_none() {
                nota!("[aggiornamenti] latest.json non ha note per la {versione}");
            }
            note
        }
        Err(err) => {
            nota!(
                "[aggiornamenti] le note non si leggono: codice={} causa={}",
                err.code().kind().code(),
                err.cause().unwrap_or("—")
            );
            None
        }
    }
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
    let Some(mut trovato) = scegli(rilascio, corrente, std::env::consts::ARCH) else {
        return Ok(None);
    };
    // Le note solo quando c'è qualcosa da annunciare: una richiesta in più ogni
    // tanto, non a ogni controllo.
    if let Some(manifesto) = trovato.manifesto.clone() {
        trovato.note = note_della_release(&manifesto, &trovato.versione);
    }
    Ok(Some(trovato))
}

// ── l'installazione ─────────────────────────────────────────────────────────

/// Il filo dell'aggiornamento: con l'APK già verificato lo consegna ad Android;
/// senza, scarica la firma e l'APK nella cache — o riusa quello già sceso —, li
/// verifica, e si ferma a `pronto`.
///
/// «Verifico» sono due controlli di fila, e vengono prima di qualunque
/// schermata di sistema: la firma minisign dell'APK ([`verifica`]) e il
/// certificato di questa app ([`controlla`], in `Installatore.kt`). Un APK che
/// non passa si cancella, e «Installa» non compare.
///
/// La firma prima dell'APK: una firma che non arriva non vale quaranta
/// megabyte scaricati per niente.
fn lavora(app: &AppHandle, trovato: &Trovato, pronto: Option<std::path::PathBuf>) {
    let Some(aggiornamenti) = app.try_state::<StatoAggiornamenti>() else {
        return;
    };
    let turno = Turno::adotta(&aggiornamenti.installazione);
    aggiornamenti.annulla.store(false, Ordering::Release);

    // Consegna: l'APK è già verificato, si apre l'installatore e basta.
    let esito = if let Some(percorso) = pronto {
        consegna(&percorso)
    } else {
        imposta_fase(app, Some(FaseAggiornamento::Scarico));
        scarica_la_firma(trovato).and_then(|firma| {
            let percorso = scarica(app, &aggiornamenti, trovato)?;
            imposta_fase(app, Some(FaseAggiornamento::Verifico));
            verifica(app, trovato, &firma, &percorso)?;
            match controlla_il_certificato(&percorso)? {
                ControlloApk::AltraChiave => Ok(Consegnato::AltraChiave),
                ControlloApk::Buono => {
                    tieni_per_installa(&aggiornamenti, trovato, percorso);
                    Ok(Consegnato::InAttesa)
                }
            }
        })
    };
    match esito {
        Ok(Consegnato::AlSistema | Consegnato::InAttesa) => {}
        Ok(Consegnato::AltraChiave) => {
            // Non un guasto da mostrare in rosso: un esito che la fascia
            // spiega (`altraChiave`), e che si ricorda perché il prossimo tocco
            // non riscarichi tutto per sentirsi dire la stessa cosa.
            nota!(
                "[aggiornamenti] Android non installa la {} sopra questa copia: è firmata con \
                 un'altra chiave. Non la si riscarica",
                trovato.versione
            );
            // Un guasto di un tentativo di prima non è più quel che c'è da dire.
            *aggiornamenti
                .ultimo_errore
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = None;
            // L'APK pronto, se c'era, non vale più.
            scarta_pronto(&aggiornamenti);
            if let Some(stato) = app.try_state::<Stato>()
                && let Err(err) = con_libreria(&stato, |libreria| {
                    settings::write(&libreria.connection, CHIAVE_RIFIUTATA, &trovato.versione)
                })
            {
                nota!(
                    "[aggiornamenti] il rifiuto non si ricorda: codice={}",
                    err.code().kind().code()
                );
            }
        }
        Err(err) => {
            // Un APK che non si consegna non è più un APK da offrire: se era
            // quello in attesa, Kotlin lo ha cancellato.
            scarta_pronto(&aggiornamenti);
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
    }
    drop(turno);
    imposta_fase(app, None);
}

/// Il controllo del certificato di un APK già verificato con minisign: vedi
/// [`rymembra_android::sistema::controlla_apk`].
///
/// # Errori
///
/// `internal.unexpected` con la causa se l'APK non è questa app (Kotlin l'ha
/// cancellato) o se il ponte non risponde.
fn controlla_il_certificato(percorso: &std::path::Path) -> Result<ControlloApk, AppError> {
    rymembra_android::sistema::controlla_apk(percorso).map_err(|err| {
        AppError::new(ErrorCode::InternalUnexpected {
            detail: Some("il certificato dell'aggiornamento".to_owned()),
        })
        .with_cause(err.to_string())
    })
}

/// L'APK verificato aspetta «Installa»: vedi [`StatoAggiornamenti::pronto`].
fn tieni_per_installa(
    aggiornamenti: &StatoAggiornamenti,
    trovato: &Trovato,
    percorso: std::path::PathBuf,
) {
    *aggiornamenti
        .pronto
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(Pronto {
        apk: percorso,
        versione: trovato.versione.clone(),
        dal_ms: adesso_ms(),
    });
}

/// La firma minisign dell'APK, il testo del `.sig` della release.
///
/// # Errori
///
/// `net.*` per la rete; `net.badSchema` per un corpo che non è testo.
fn scarica_la_firma(trovato: &Trovato) -> Result<String, AppError> {
    let rete = rete_degli_allegati(&Rete::nuova("aggiornamenti", SCADENZA_RICHIESTA));
    let mut corpo = Vec::new();
    rete.preleva(
        &trovato.firma,
        &[],
        &mut corpo,
        TETTO_DELLA_FIRMA,
        &|| false,
        &mut |_, _| {},
    )?;
    String::from_utf8(corpo).map_err(|_| {
        AppError::new(ErrorCode::NetBadSchema {
            service: Some("aggiornamenti".to_owned()),
            detail: Some("la firma dell'APK non è testo".to_owned()),
        })
    })
}

/// L'APK sul disco è quello firmato con la chiave dell'updater per la
/// versione annunciata ([`crate::rilasci::verifica_la_firma`]). Si rifà a ogni
/// consegna, anche per l'APK riusato dalla cache: un file troncato o cambiato
/// lì dentro non arriva all'installatore. Se non torna si cancella, e il
/// prossimo «Aggiorna» lo riscarica.
///
/// # Errori
///
/// `internal.unexpected` con la causa, come il computer per un installer la
/// cui firma non torna: è quel che si guarda se un aggiornamento non si
/// installa su un telefono solo.
fn verifica(
    app: &AppHandle,
    trovato: &Trovato,
    firma: &str,
    percorso: &std::path::Path,
) -> Result<(), AppError> {
    let guasto = |causa: String| {
        AppError::new(ErrorCode::InternalUnexpected {
            detail: Some("la firma dell'aggiornamento".to_owned()),
        })
        .with_cause(causa)
    };
    let chiave = chiave_pubblica(app)
        .ok_or_else(|| guasto("manca la chiave pubblica dell'updater".to_owned()))?;
    let esito = std::fs::File::open(percorso)
        .map_err(|err| format!("l'APK non si apre: {err}"))
        .and_then(|file| {
            let mut lettore = std::io::BufReader::new(file);
            verifica_la_firma(&chiave, firma, &trovato.versione, &mut lettore)
        });
    if let Err(causa) = esito {
        let _ = std::fs::remove_file(percorso);
        return Err(guasto(causa));
    }
    Ok(())
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
    let rete = rete_degli_allegati(&Rete::per_prelievo("aggiornamenti"));
    // Un tetto ai byte, perché l'unico limite era la scadenza del prelievo, e
    // un corpo senza fine riempiva la cache del telefono fin lì. Mezzo
    // gibibyte: molte volte un APK, e se un giorno non bastasse l'errore lo
    // dice («supera il tetto di 512 MB»).
    let tetto_apk: u64 = 512 * 1024 * 1024;
    let esito = rete.preleva(
        &trovato.indirizzo,
        &[],
        &mut file,
        tetto_apk,
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

/// Com'è andata la consegna dell'APK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Consegnato {
    /// L'installatore di Android è aperto, o la pagina del permesso di
    /// installare app: da lì decide chi usa il telefono.
    AlSistema,
    /// L'APK è scaricato e verificato e aspetta il tocco su «Installa».
    InAttesa,
    /// L'APK è firmato con un'altra chiave di questa copia, e Android non lo
    /// installerebbe sopra: `Installatore.kt` l'ha già cancellato.
    AltraChiave,
}

/// Consegna l'APK all'installatore di Android.
///
/// Prima `Installatore.kt` lo verifica: un APK che non è questa app — un altro
/// pacchetto, un archivio che non si legge — torna qui come errore; uno
/// firmato con un'altra chiave come [`Consegnato::AltraChiave`]. In tutti e
/// due i casi è già cancellato, così il prossimo «Aggiorna» non lo riusa.
///
/// Se Rymembra non ha ancora il permesso di installare app, Android apre la
/// pagina dove lo si concede: chi la chiude torna a «Aggiorna», che stavolta
/// non scarica niente e apre l'installatore.
fn consegna(percorso: &std::path::Path) -> Result<Consegnato, AppError> {
    match rymembra_android::sistema::installa_apk(percorso) {
        Ok(rymembra_android::sistema::Installazione::Installatore) => Ok(Consegnato::AlSistema),
        Ok(rymembra_android::sistema::Installazione::Permesso) => {
            nota!("[aggiornamenti] Android chiede il permesso di installare app");
            Ok(Consegnato::AlSistema)
        }
        Ok(rymembra_android::sistema::Installazione::AltraChiave) => Ok(Consegnato::AltraChiave),
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
    let (attivo, ultimo_ms, saltata, rifiutata) = con_libreria(stato, |libreria| {
        let attivo = settings::read(&libreria.connection, CHIAVE_ATTIVO)?.as_deref() != Some("0");
        let ultimo_ms = settings::read(&libreria.connection, CHIAVE_ULTIMO)?
            .and_then(|testo| testo.parse::<i64>().ok());
        let saltata = settings::read(&libreria.connection, CHIAVE_SALTATA)?;
        let rifiutata = settings::read(&libreria.connection, CHIAVE_RIFIUTATA)?;
        Ok((attivo, ultimo_ms, saltata, rifiutata))
    })?;
    let trovato = aggiornamenti
        .trovato
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let in_corso_fase = *aggiornamenti
        .fase
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let pronto_dal_ms = aggiornamenti
        .pronto
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|pronto| pronto.dal_ms);
    // Fuori dal lucchetto: la prima volta chiede la firma ad Android.
    let disponibile = trovato.map(|trovato| AggiornamentoIpc {
        altra_chiave: non_installabile(
            aggiornamenti.copia_di_prova(),
            rifiutata.as_deref(),
            &trovato.versione,
        ),
        saltata: saltata.as_deref() == Some(trovato.versione.as_str()),
        versione: trovato.versione,
        note: trovato.note,
        data_ms: trovato.data_ms,
    });
    Ok(StatoAggiornamentiIpc {
        attivo,
        configurato: progetto(app).is_some() && chiave_pubblica(app).is_some(),
        gestito_dal_store: false,
        versione_corrente: app.package_info().version.to_string(),
        ultimo_ms,
        disponibile,
        in_corso: aggiornamenti.in_corso.load(Ordering::Relaxed),
        installazione: aggiornamenti.installazione.load(Ordering::Relaxed),
        fase: fase_dello_stato(in_corso_fase, pronto_dal_ms.is_some()),
        pronto_dal_ms,
        errore: aggiornamenti
            .ultimo_errore
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone(),
    })
}
