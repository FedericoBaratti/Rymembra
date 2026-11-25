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
//! # Due distribuzioni
//!
//! Questa è la distribuzione `github`: l'APK delle release, con l'aggiornatore.
//! La distribuzione `play` — la build per il Google Play Store, che per la policy
//! si aggiorna solo dal Play e non può chiedere `REQUEST_INSTALL_PACKAGES` — non
//! ha aggiornatore: non controlla niente, non contatta nessuno, e la finestra lo
//! dice come dice del Microsoft Store sul computer (`gestito_dal_store`). Quale
//! delle due sia lo dice `BuildConfig.DISTRIBUZIONE`, impostata dalla proprietà
//! Gradle `distribuzione` (`app/build.gradle.kts`), e Rust la legge una volta
//! ([`senza_aggiornatore`]): un `.so` solo serve tutte e due.
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
use tauri::{AppHandle, Manager as _};

use crate::corsia::LIBRERIA;
use crate::disparte::{comanda, stato_di};
use crate::errore::{ErroreIpc, Esito, errore};
use crate::nota;
use crate::rilasci::{
    RifiutoDellaFirma, Rilascio, Trovato, copia_di_prova, non_installabile, note_dal_manifesto,
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

/// Questa copia non ha l'aggiornatore: è la distribuzione `play`.
///
/// Si chiede ad Android una volta — la distribuzione non cambia mentre l'app
/// gira — e si ricorda solo una risposta che c'è stata. **Se il ponte non
/// risponde si sta dalla parte prudente**: niente aggiornatore, niente richieste
/// a GitHub da una build che forse non le deve fare, e una riga nel diario, una
/// volta sola. Il ponte non risponde solo se l'app è in uno stato in cui non c'è
/// niente da aggiornare comunque.
fn senza_aggiornatore() -> bool {
    static PLAY: OnceLock<bool> = OnceLock::new();
    static DETTO: AtomicBool = AtomicBool::new(false);
    if let Some(valore) = PLAY.get() {
        return *valore;
    }
    match rymembra_android::sistema::distribuzione() {
        Ok(nome) => *PLAY.get_or_init(|| nome == "play"),
        Err(err) => {
            if !DETTO.swap(true, Ordering::Relaxed) {
                nota!(
                    "[aggiornamenti] la distribuzione non si legge: {err}. Niente aggiornamenti,                      per prudenza"
                );
            }
            true
        }
    }
}

/// L'APK scaricato e verificato che aspetta «Installa».
struct Pronto {
    /// Il file nella cache degli aggiornamenti.
    apk: std::path::PathBuf,
    /// La firma minisign con cui si è verificato, il testo del `.sig`: serve a
    /// ricontrollare il file contro di lei un attimo prima di consegnarlo
    /// ([`verifica`]), dopo le ore o i giorni in cui ha aspettato nella cache.
    firma: String,
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
    /// Nella distribuzione `play` è il Google Play a tenerla aggiornata; in quella
    /// `github` nessun negozio lo fa.
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
pub async fn aggiornamenti_stato(app: AppHandle) -> Esito<StatoAggiornamentiIpc> {
    comanda("stato degli aggiornamenti", move || {
        let stato = stato_di::<Stato>(&app)?;
        let aggiornamenti = stato_di::<StatoAggiornamenti>(&app)?;
        stato_ipc(&app, &stato, &aggiornamenti).map_err(errore)
    })
    .await
}

/// Accende o spegne il controllo periodico. Accendendolo si controlla subito;
/// spegnendolo l'avviso già trovato se ne va.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce.
#[tauri::command]
pub async fn aggiornamenti_attivo(app: AppHandle, attivo: bool) -> Esito<StatoAggiornamentiIpc> {
    // Sulla corsia della libreria: un interruttore dove l'ultimo vince.
    LIBRERIA
        .comanda("aggiornamenti attivi", move || {
            let stato = stato_di::<Stato>(&app)?;
            let aggiornamenti = stato_di::<StatoAggiornamenti>(&app)?;
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
                // Un APK che sta ancora scendendo si ferma: non vale la pena finirlo. E
                // quello che finisse comunque — il filo guarda l'annullo solo mentre
                // scarica — non si tiene: la generazione è cambiata (vedi
                // [`deposita_pronto_se_vale`]). Prima l'una e poi lo svuotamento, perché
                // il deposito controlla la generazione sotto lo stesso lucchetto.
                if aggiornamenti.installazione.load(Ordering::Acquire) {
                    aggiornamenti.annulla.store(true, Ordering::Release);
                }
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
        })
        .await
}

/// Controlla adesso: sveglia il filo, e l'esito arriva con
/// `aggiornamenti:stato`.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
#[tauri::command]
pub async fn aggiornamenti_adesso(app: AppHandle) -> Esito<StatoAggiornamentiIpc> {
    comanda("controllo degli aggiornamenti", move || {
        let stato = stato_di::<Stato>(&app)?;
        let aggiornamenti = stato_di::<StatoAggiornamenti>(&app)?;
        let _ = aggiornamenti.sveglia.send(Sveglia::Subito);
        stato_ipc(&app, &stato, &aggiornamenti).map_err(errore)
    })
    .await
}

/// Dimentica questa versione finché non ne esce un'altra.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce.
#[tauri::command]
pub async fn aggiornamenti_salta(app: AppHandle, versione: String) -> Esito<StatoAggiornamentiIpc> {
    LIBRERIA
        .comanda("versione saltata", move || {
            let stato = stato_di::<Stato>(&app)?;
            let aggiornamenti = stato_di::<StatoAggiornamenti>(&app)?;
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
        })
        .await
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
pub async fn aggiornamenti_installa(app: AppHandle, quando: Quando) -> Esito<()> {
    comanda("installazione dell'aggiornamento", move || {
        installa_ora(&app, quando)
    })
    .await
}

/// Il corpo di [`aggiornamenti_installa`], sul pool bloccante.
fn installa_ora(app: &AppHandle, quando: Quando) -> Esito<()> {
    let aggiornamenti = stato_di::<StatoAggiornamenti>(app)?;
    if quando == Quando::AllaChiusura {
        return Err(errore(AppError::new(ErrorCode::InternalAborted {
            what: Some("sul telefono non c'è l'installazione alla chiusura".to_owned()),
        })));
    }
    if senza_aggiornatore() {
        return Err(errore(AppError::new(ErrorCode::InternalAborted {
            what: Some("questa distribuzione si aggiorna dal Google Play".to_owned()),
        })));
    }
    // La generazione **prima** di guardare cosa c'è da installare: se l'interruttore
    // si spegne dopo questo punto, il filo se ne accorge e non tiene il pacchetto;
    // se si è spento prima, `trovato` è già vuoto e qui si esce (S5-3).
    let partenza = aggiornamenti.generazione.load(Ordering::Acquire);
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
        versione_rifiutata(app).as_deref(),
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
    // Col file va la sua firma: la consegna la ricontrolla ([`lavora`]).
    let pronto = aggiornamenti
        .pronto
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .filter(|pronto| pronto.versione == trovato.versione)
        .map(|pronto| (pronto.apk.clone(), pronto.firma.clone()));
    riferisci(app);
    let suo = app.clone();
    if let Err(err) = std::thread::Builder::new()
        .name("rymembra-aggiornamento".to_owned())
        .spawn(move || lavora(&suo, &trovato, pronto, partenza))
    {
        aggiornamenti.installazione.store(false, Ordering::Release);
        riferisci(app);
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
pub async fn aggiornamenti_annulla(app: AppHandle) -> Esito<()> {
    comanda("annullo dell'aggiornamento", move || {
        let aggiornamenti = stato_di::<StatoAggiornamenti>(&app)?;
        if aggiornamenti.installazione.load(Ordering::Acquire) {
            aggiornamenti.annulla.store(true, Ordering::Release);
        } else if scarta_pronto(&aggiornamenti) {
            riferisci(&app);
        }
        Ok(())
    })
    .await
}

/// Butta l'APK in attesa, se c'è. `true` se c'era.
fn scarta_pronto(aggiornamenti: &StatoAggiornamenti) -> bool {
    scarta_pronto_se(aggiornamenti, |_| true)
}

/// Butta l'APK in attesa se `da_buttare` lo vuole, e con lui la notifica
/// «Pronto da installare». `true` se l'ha buttato.
///
/// È **l'unico** posto che toglie il pacchetto pronto — «Non installare», «Non
/// ora», un APK che Android rifiuterebbe, una versione più nuova, un guasto —,
/// così la notifica non resta a offrire un «Installa» che non c'è più. La
/// scelta e il gesto stanno sotto lo stesso lucchetto, perché fra l'una e
/// l'altro un altro filo può aver messo un pacchetto che va tenuto; la
/// notifica si spegne dopo, a lucchetto lasciato: il ponte è una chiamata a
/// Kotlin.
fn scarta_pronto_se(
    aggiornamenti: &StatoAggiornamenti,
    da_buttare: impl FnOnce(&Pronto) -> bool,
) -> bool {
    let buttato = {
        let mut pronto = aggiornamenti
            .pronto
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let buttare = pronto.as_ref().is_some_and(da_buttare);
        if buttare {
            *pronto = None;
        }
        buttare
    };
    if buttato {
        notifica_pronto(false, "");
    }
    buttato
}

/// Accende, o spegne, la notifica di sistema «Pronto da installare»: vedi
/// [`rymembra_android::sistema::aggiornamento_pronto`], che dice quando Kotlin
/// la mostra. Una notifica che non si può dare non ferma l'aggiornamento — la
/// scheda dell'app c'è comunque — ma si scrive nel diario.
fn notifica_pronto(acceso: bool, versione: &str) {
    if let Err(err) = rymembra_android::sistema::aggiornamento_pronto(acceso, versione) {
        nota!(
            "[aggiornamenti] la notifica «pronto da installare» non si {}: {err}",
            if acceso { "accende" } else { "spegne" }
        );
    }
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
    // Nella distribuzione `play` non si controlla e non si contatta nessuno: gli
    // aggiornamenti li fa il Play.
    if senza_aggiornatore() {
        return Giro::Saltato;
    }
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
                // si offre da installare, e la sua notifica se ne va.
                scarta_pronto_se(&aggiornamenti, |pronto| {
                    versione.as_deref() != Some(pronto.versione.as_str())
                });
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
///
/// `partenza` è la generazione dell'interruttore com'era quando il filo è
/// partito: se intanto «Controlla se ci sono aggiornamenti» è stato spento, il
/// pacchetto che finisce non si tiene in attesa e non accende la notifica
/// (S5-3, [`deposita_pronto_se_vale`]).
fn lavora(
    app: &AppHandle,
    trovato: &Trovato,
    pronto: Option<(std::path::PathBuf, String)>,
    partenza: u64,
) {
    let Some(aggiornamenti) = app.try_state::<StatoAggiornamenti>() else {
        return;
    };
    let turno = Turno::adotta(&aggiornamenti.installazione);
    aggiornamenti.annulla.store(false, Ordering::Release);

    // Consegna: l'APK è già verificato, ma ha aspettato nella cache, magari per
    // giorni: la firma minisign si ricontrolla contro i byte che stanno ora sul
    // disco prima di aprire l'installatore, come promette SECURITY.md (S5-4). Kotlin
    // ripete poi pacchetto e certificato.
    let esito = if let Some((percorso, firma)) = pronto {
        verifica(app, trovato, &firma, &percorso).and_then(|()| consegna(&percorso))
    } else {
        imposta_fase(app, Some(FaseAggiornamento::Scarico));
        scarica_la_firma(trovato).and_then(|firma| {
            let percorso = scarica(app, &aggiornamenti, trovato)?;
            imposta_fase(app, Some(FaseAggiornamento::Verifico));
            verifica(app, trovato, &firma, &percorso)?;
            match controlla_il_certificato(&percorso)? {
                ControlloApk::AltraChiave => Ok(Consegnato::AltraChiave),
                ControlloApk::Buono => {
                    if tieni_per_installa(&aggiornamenti, trovato, firma, percorso, partenza) {
                        // Chi ha toccato «Aggiorna» può essere già altrove: gli si dice
                        // che c'è da installare.
                        notifica_pronto(true, &trovato.versione);
                        // Spento proprio ora: il posto è stato svuotato prima che il
                        // pacchetto ci arrivasse o subito dopo, e `scarta_pronto` non
                        // avrebbe trovato niente da buttare né da spegnere. La
                        // notifica si rispegne qui.
                        if aggiornamenti.generazione.load(Ordering::Acquire) != partenza {
                            notifica_pronto(false, "");
                        }
                    }
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
/// `true` se è stato tenuto; `false` se l'interruttore è stato spento da quando è
/// partito lo scaricamento ([`deposita_pronto_se_vale`]): allora il file resta
/// nella cache, come per ogni «Non installare», ma nessuno lo offre.
fn tieni_per_installa(
    aggiornamenti: &StatoAggiornamenti,
    trovato: &Trovato,
    firma: String,
    percorso: std::path::PathBuf,
    partenza: u64,
) -> bool {
    let tenuto = deposita_pronto_se_vale(
        &aggiornamenti.pronto,
        &aggiornamenti.generazione,
        partenza,
        Pronto {
            apk: percorso,
            firma,
            versione: trovato.versione.clone(),
            dal_ms: adesso_ms(),
        },
    );
    if !tenuto {
        nota!(
            "[aggiornamenti] il controllo è stato spento mentre la {} scendeva: \
             l'APK non si tiene in attesa",
            trovato.versione
        );
    }
    tenuto
}

/// Mette `pronto` nel posto **solo se** la generazione è ancora quella di
/// `partenza`, cioè se l'interruttore non è stato spento da quando il filo è
/// partito. `true` se l'ha messo.
///
/// Il controllo sta sotto il lucchetto del posto, e chi spegne l'interruttore
/// alza la generazione **prima** di svuotarlo ([`aggiornamenti_attivo`]): un
/// deposito che passa il controllo sta già nel posto quando lo svuotamento lo
/// raggiunge, e uno che arriva dopo trova la generazione cambiata. La stessa
/// regola con cui `passata` protegge `trovato`: un pacchetto finito dopo lo
/// spegnimento non resta in attesa, e non accende la notifica «Pronto da
/// installare» a un controllo spento.
fn deposita_pronto_se_vale(
    posto: &Mutex<Option<Pronto>>,
    generazione: &AtomicU64,
    partenza: u64,
    pronto: Pronto,
) -> bool {
    let mut posto = posto.lock().unwrap_or_else(PoisonError::into_inner);
    if generazione.load(Ordering::Acquire) != partenza {
        return false;
    }
    *posto = Some(pronto);
    true
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
/// consegna: per l'APK riusato dalla cache, e per quello in attesa di «Installa»
/// ([`Pronto::firma`], [`lavora`]). Un file troncato o cambiato lì dentro non
/// arriva all'installatore. Se non torna si cancella, e il prossimo «Aggiorna»
/// lo riscarica.
///
/// # Errori
///
/// `update.signatureInvalid` se la firma non torna — come il computer per un
/// installer che non è firmato con la chiave di Rymembra: la finestra dice che
/// non si è installato e che conviene riprovare più tardi. Con la causa, che è
/// quel che si guarda se un aggiornamento non si installa su un telefono solo.
/// `internal.unexpected` se non si è potuto verificare (la chiave dell'app non
/// si legge, l'APK non si apre né si legge): lì non si sa niente dell'APK, e
/// dire «non è firmato da noi» sarebbe falso ([`RifiutoDellaFirma`]).
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
        .map_err(|err| RifiutoDellaFirma::Guasto(format!("l'APK non si apre: {err}")))
        .and_then(|file| {
            let mut lettore = std::io::BufReader::new(file);
            verifica_la_firma(&chiave, firma, &trovato.versione, &mut lettore)
        });
    if let Err(rifiuto) = esito {
        let _ = std::fs::remove_file(percorso);
        return Err(rifiuto.in_errore());
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
        configurato: !senza_aggiornatore()
            && progetto(app).is_some()
            && chiave_pubblica(app).is_some(),
        // Sul Play a tenerla aggiornata è il Play: la finestra lo dice invece di
        // «manca la chiave» (`settings.update.play`).
        gestito_dal_store: senza_aggiornatore(),
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
