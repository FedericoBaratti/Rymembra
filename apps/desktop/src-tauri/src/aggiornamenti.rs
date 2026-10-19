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
//! di serie, niente della libreria. **La versione installata non viene nemmeno
//! mandata** — il confronto lo fa il programma, qui, dopo aver letto il file, e
//! non c'è nessun servizio a cui si chieda «sono aggiornato?». Non viene mandato
//! neppure un `User-Agent` che nomini Rymembra, e non è una dimenticanza: vedi
//! [`costruisci`], dove sta scritto perché e cosa costa. Quel che GitHub vede è
//! che qualcuno da un certo indirizzo IP ha chiesto un file pubblico, cioè quel
//! che vedrebbe di chiunque aprisse la pagina delle release con un browser.
//!
//! È l'unica richiesta che Rymembra fa senza che nessuno gliel'abbia chiesta, ed
//! è per questo che sta scritta in `PRIVACY.md` con un numero suo e si spegne
//! con un interruttore in Impostazioni.
//!
//! # Un controllo a ogni avvio
//!
//! È la voce #10 di `TODO.md`: ogni volta che Rymembra si apre, controlla da sé
//! una volta. Il controllo d'avvio parte [`DOPO_PRONTO`] dopo che la finestra
//! si è mostrata (il comando `pronto`), o dopo [`ATTESA_PRONTO_MASSIMA`] se la
//! finestra non lo dice mai. Fino alla 2.4.0 la guardia fra un controllo e
//! l'altro era di mezz'ora, e chi apriva Rymembra due volte nella stessa mezz'ora
//! non controllava la seconda: adesso è di [`GUARDIA`], che basta a non
//! martellare GitHub se il programma si apre e si chiude in un giro, e non
//! toglie il controllo a nessuno che lo riapra dopo pranzo. «Controlla adesso»
//! non guarda niente: chi preme un tasto ha il diritto di vedere che succede
//! qualcosa. E la guardia cede quando l'ultimo controllo aveva trovato una
//! versione nuova — vedi `CHIAVE_TROVATA`.
//!
//! # Senza rete, si riprova presto
//!
//! Un controllo d'avvio senza rete — il portatile aperto in treno, il Wi-Fi
//! che si aggancia dopo Rymembra — non aspetta la mezz'ora: si riprova dopo
//! uno, cinque e quindici minuti ([`RIPROVE_SENZA_RETE`]), e poi si torna al
//! passo normale. Solo il controllo automatico: se `aggiornamenti.attivo` è
//! spento, non riprova niente.
//!
//! # Lo scaricamento ha un limite, e un tasto per fermarlo
//!
//! Il plugin costruisce lo scaricamento senza scadenza. Un installer che
//! smette di arrivare a metà — una rete che cade senza chiudere la connessione
//! — teneva il filo e il tasto «Aggiorna» fermi fino al riavvio. Adesso uno
//! scaricamento che non riceve un byte per [`SILENZIO_MASSIMO`] si ferma e lo
//! dice, e [`aggiornamenti_annulla`] lo ferma quando lo chiede chi guarda.
//!
//! # Prima di uscire, si spegne come si deve
//!
//! Su Windows l'installazione **termina il processo** con `exit(0)`, dal filo
//! dello scaricamento: senza un aggancio, la posizione dell'ascolto, l'ultimo
//! `PRAGMA optimize`, la scheda del sistema e l'icona del vassoio restavano
//! come li trovava la morte del processo. Il plugin offre `on_before_exit`, e
//! lì si fa la stessa sequenza della chiusura normale:
//! [`crate::spegnimento::spegni`], una sola, chiamata da tutti e due i posti.
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

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::spegnimento::Emette as _;
use rymembra_app::settings;
use rymembra_domain::errors::{AppError, ErrorCode};
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

/// La chiave della versione che l'ultimo controllo riuscito ha trovato.
///
/// # Perché si scrive, se l'oggetto per installare vive in memoria
///
/// Perché la guardia d'avvio salta il controllo se l'ultimo è recente — era
/// mezz'ora, adesso [`GUARDIA`] — e l'aggiornamento trovato vive **solo** in
/// memoria. Chi vedeva l'avviso, chiudeva Rymembra e lo riapriva subito non lo
/// rivedeva più: il controllo d'avvio saltava, e il prossimo arrivava dopo
/// mezz'ora. Qui si ricorda soltanto che una versione nuova c'era — basta a
/// far fare comunque il controllo d'avvio, che è l'unico modo di riavere
/// l'oggetto con l'URL e la firma. Si toglie quando un controllo riuscito non
/// trova niente.
const CHIAVE_TROVATA: &str = "aggiornamenti.trovata";

/// Ogni quanto si guarda se è uscita una versione nuova.
///
/// Mezz'ora, che è quel che è stato chiesto ed è anche una scelta difendibile:
/// una release esce qualche volta al mese, quindi la frequenza non serve a
/// scoprirla presto — serve a fare in modo che chi tiene Rymembra aperto per
/// giorni la scopra comunque, senza dover chiudere e riaprire. Il costo è una
/// richiesta da trecento byte, cioè meno di quel che costa disegnare la lista
/// dei brani una volta.
const INTERVALLO: Duration = Duration::from_secs(30 * 60);

/// Quanto dopo `pronto` parte il controllo d'avvio.
///
/// Dieci secondi, ed erano quarantacinque dall'apertura. Il conto si fa dalla
/// finestra mostrata e non dal processo nato, perché è da lì che qualcuno sta
/// guardando: chi apre Rymembra, vede che c'è una versione nuova e lo richiude
/// deve scoprirlo prima di richiuderlo. I dieci secondi sono quelli in cui la
/// finestra disegna la prima schermata e la coda di ieri torna su; la richiesta
/// non tocca il disco e il lucchetto della libreria lo prende per due letture
/// da microsecondi, quindi non contende niente con la sincronia (venti
/// secondi) e la nuvola (trenta).
const DOPO_PRONTO: Duration = Duration::from_secs(10);

/// Quanto si aspetta `pronto` prima di controllare comunque.
///
/// La finestra che non arriva mai a dire `pronto` — uno script che cade, una
/// skin illeggibile — è mostrata lo stesso dalla rete di sicurezza di `main`
/// dopo due secondi; un minuto è largo abbastanza da non anticiparla mai, e
/// stretto abbastanza perché chi ha una finestra rotta riceva lo stesso
/// l'aggiornamento che magari la ripara.
const ATTESA_PRONTO_MASSIMA: Duration = Duration::from_secs(60);

/// Quanto deve essere vecchio l'ultimo controllo riuscito perché quello
/// d'avvio si faccia.
///
/// Cinque minuti, ed era mezz'ora: la guardia esiste per chi apre e chiude il
/// programma a raffica — un collegamento cliccato due volte, un crash all'avvio
/// che si ripete — non per far saltare il controllo a chi riapre Rymembra
/// mezz'ora dopo, che è precisamente chi la voce #10 di `TODO.md` voleva
/// servire.
const GUARDIA: Duration = Duration::from_secs(5 * 60);

/// Dopo quanto si riprova un controllo automatico fallito per la rete.
///
/// Uno, cinque, quindici minuti: la scala di chi apre il portatile prima che
/// il Wi-Fi si agganci, di un router che riparte, di un treno che esce da una
/// galleria. Finiti questi, si torna a [`INTERVALLO`] fino al primo controllo
/// riuscito.
const RIPROVE_SENZA_RETE: [Duration; 3] = [
    Duration::from_secs(60),
    Duration::from_secs(5 * 60),
    Duration::from_secs(15 * 60),
];

/// Quanto può stare fermo uno scaricamento prima che lo si dichiari perso.
///
/// Un minuto senza un byte. Non è una scadenza complessiva — l'installer pesa
/// settanta megabyte, e su una linea lenta ci vogliono minuti — ma il segno di
/// una connessione che non è caduta e non porta più niente.
const SILENZIO_MASSIMO: Duration = Duration::from_secs(60);

/// Ogni quanto il filo dell'installazione guarda lo scaricamento.
const PASSO_SORVEGLIANZA: Duration = Duration::from_millis(100);

/// Quanto si aspetta una risposta prima di dire che non arriva.
///
/// Senza, non c'è nessun limite: un DNS che non risponde, un captive portal che
/// tiene aperta la connessione senza mai rispondere, un proxy aziendale che
/// inghiotte la richiesta — e il filo resta dentro `check()` per sempre. Non è
/// solo un filo appeso: finché quel filo è là dentro il [`Turno`] è preso, e la
/// finestra mostra un controllo «in corso» con «Controlla adesso» disabilitato
/// **fino al prossimo riavvio**. Venti secondi sono larghi per un file di
/// trecento byte anche su una linea pessima, e stretti abbastanza perché al
/// battito dopo si riprovi.
///
/// # Non è la scadenza dello scaricamento
///
/// Vale per il `latest.json` e basta. `Updater::check` costruisce l'oggetto
/// [`Update`] con `timeout: None` cablato dentro — verificato leggendo
/// `updater.rs` del plugin — quindi un installer da settanta megabyte non ha
/// venti secondi per scendere. Sarebbe stato il guasto da introdurre qui senza
/// accorgersene, e la ragione per cui questa riga di prosa esiste.
const SCADENZA_RICHIESTA: Duration = Duration::from_secs(20);

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
/// lo zittisce, l'installer che parte. Il precedente è `RYMEMBRA_DATI`, e la
/// ragione è la stessa — le cose irreversibili si giudicano guardandole.
///
/// In rilascio non esiste: l'endpoint di un updater è la cosa che decide quale
/// eseguibile finisce sul computer di qualcuno, e una variabile d'ambiente che
/// la cambia è un modo di installare software altrui che non deve esserci.
#[cfg(debug_assertions)]
const VARIABILE_ENDPOINT: &str = "RYMEMBRA_AGGIORNAMENTI_ENDPOINT";

/// Perché il filo si è svegliato.
///
/// Niente «sporca» un controllo aggiornamenti, perché non dipende da cosa
/// succede nella libreria: o lo chiede qualcuno, o la finestra si è mostrata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sveglia {
    /// Qualcuno ha chiesto di controllare adesso.
    Subito,
    /// La finestra si è mostrata: da qui parte il conto di [`DOPO_PRONTO`].
    Pronto,
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

    /// Qualcuno ha chiesto di fermare lo scaricamento in corso.
    ///
    /// Si abbassa all'inizio di ogni scaricamento: un «Annulla» premuto ieri
    /// non deve fermare quello di oggi.
    annulla: AtomicBool,

    /// La finestra ha già detto `pronto` una volta.
    ///
    /// `pronto` si chiama a ogni ridisegno completo — una skin cambiata, un
    /// ricaricamento in sviluppo — e il controllo d'avvio è uno.
    pronto_detto: AtomicBool,
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
                annulla: AtomicBool::new(false),
                pronto_detto: AtomicBool::new(false),
            },
            orecchio,
        )
    }
}

/// La finestra si è mostrata: comincia il conto del controllo d'avvio.
///
/// La chiama il comando `pronto` di `main`. Solo la prima volta conta: vedi
/// `StatoAggiornamenti::pronto_detto`.
pub fn finestra_pronta(app: &AppHandle) {
    if let Some(aggiornamenti) = app.try_state::<StatoAggiornamenti>()
        && !aggiornamenti.pronto_detto.swap(true, Ordering::AcqRel)
    {
        let _ = aggiornamenti.sveglia.send(Sveglia::Pronto);
    }
}

// ── quel che la finestra vede ───────────────────────────────────────────────

/// Lo stato del controllo aggiornamenti, come lo mostrano le Impostazioni.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoAggiornamentiIpc {
    /// Il controllo periodico è acceso.
    pub attivo: bool,
    /// Questa copia di Rymembra ha una chiave pubblica con cui verificare.
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
    // Anche a chi non ha chiesto: vedi `riferisci`.
    riferisci(&app);
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
    // Anche a chi non ha chiesto: vedi `riferisci`.
    riferisci(&app);
    stato_ipc(&app, &stato, &aggiornamenti).map_err(errore)
}

/// Scarica l'aggiornamento trovato, ne verifica la firma e lo installa.
///
/// Torna subito: il lavoro vero sta su un filo suo, e quel che succede si
/// racconta con `aggiornamenti:avanzamento`. Su Windows, con
/// `installMode: "passive"`, è l'installer NSIS a chiudere Rymembra e a
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
        .name("rymembra-aggiornamento".to_owned())
        .spawn(move || {
            scarica_e_installa(&suo, &aggiornamento);
        });
    if let Err(err) = avviato {
        // Il bit va rimesso giù, o il tasto resta spento per sempre senza che
        // sia successo niente. Capita solo se il sistema operativo non dà più
        // thread, cioè quando l'applicazione ha guai ben più grossi di un
        // aggiornamento non partito — ma «più grossi» non vuol dire che qui si
        // possa lasciare uno stato che mente.
        //
        // È l'**unico** `store` a mano che resta su questa bandiera, ed è giusto
        // che sia a mano: il filo non è nato, quindi non c'è nessun
        // `Turno::adotta` là dentro che possa abbassarla. Dove il filo parte, il
        // rilascio è suo — vedi `scarica_e_installa`.
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

/// Ferma lo scaricamento in corso.
///
/// Torna subito: alza un bit che il filo dell'installazione guarda dieci volte
/// al secondo. Fermato lì, lo scaricamento non lascia niente — i byte stanno in
/// memoria, non su disco — e il tasto «Aggiorna» torna premibile.
///
/// Dopo che lo scaricamento è finito non ferma più niente: l'installer è già
/// partito, e da lì non si torna indietro.
#[tauri::command]
pub fn aggiornamenti_annulla(aggiornamenti: State<'_, StatoAggiornamenti>) -> Esito<()> {
    aggiornamenti.annulla.store(true, Ordering::Release);
    Ok(())
}

// ── il filo di sottofondo ───────────────────────────────────────────────────

/// Avvia il filo che controlla da solo.
///
/// # La guardia d'avvio
///
/// La prima passata è l'unica che può essere di troppo: chi apre e chiude
/// Rymembra a raffica farebbe una richiesta per apertura. Quindi la sola passata
/// d'avvio guarda quando è andato a buon fine l'ultimo controllo, e se è di
/// meno di [`GUARDIA`] non la fa — vedi [`troppo_presto`]. Il battito
/// periodico, dopo, non guarda niente: `INTERVALLO` è passato per definizione.
///
/// Una sveglia «Controlla adesso» arrivata **durante** l'attesa d'avvio — o
/// l'interruttore appena acceso — vale come controllo d'avvio, e la guardia non
/// la vede: è un tasto premuto da una persona che sta guardando la finestra.
///
/// # I giri dopo
///
/// Quanto aspettare lo decide l'esito: [`prossima_attesa`].
///
/// Il corpo di ogni giro gira **sorvegliato** ([`crate::stato::sorvegliato`]):
/// un panico dentro il plugin non porta via il filo, che riprova al giro dopo.
pub fn avvia_filo(app: AppHandle, orecchio: Receiver<Sveglia>) {
    let avviato = std::thread::Builder::new()
        .name("rymembra-aggiornamenti".to_owned())
        .spawn(move || {
            // Una sveglia è sempre una persona; la scadenza, mai. Vedi
            // `passata`.
            let Some(mut a_mano) = aspetta_l_avvio(&orecchio) else {
                return;
            };
            let mut guardia = !a_mano;
            let mut riprove = 0_usize;
            loop {
                if crate::spegnimento::in_uscita() {
                    return;
                }
                // Il cortocircuito del `&&` non è un dettaglio di stile: tiene
                // la lettura dal database fuori da ogni giro tranne, al più, il
                // primo. La guardia cede quando l'ultimo controllo aveva
                // trovato una versione nuova: vedi `CHIAVE_TROVATA`.
                let giro = if guardia
                    && troppo_presto(ultimo_controllo(&app), adesso_ms())
                    && !versione_trovata_prima(&app)
                {
                    nota!(
                        "[aggiornamenti] la passata d'avvio salta: l'ultimo controllo è di meno di {} minuti fa",
                        GUARDIA.as_secs().div_euclid(60)
                    );
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

/// Aspetta il momento del controllo d'avvio.
///
/// `pronto` più [`DOPO_PRONTO`]; o [`ATTESA_PRONTO_MASSIMA`] se `pronto` non
/// arriva; o subito, se nel frattempo qualcuno preme «Controlla adesso».
/// Restituisce se il controllo è stato chiesto a mano, e `None` se il canale si
/// è chiuso — cioè se si sta uscendo.
fn aspetta_l_avvio(orecchio: &Receiver<Sveglia>) -> Option<bool> {
    match orecchio.recv_timeout(ATTESA_PRONTO_MASSIMA) {
        Err(RecvTimeoutError::Disconnected) => None,
        Err(RecvTimeoutError::Timeout) => Some(false),
        Ok(Sveglia::Subito) => Some(true),
        Ok(Sveglia::Pronto) => aspetta(orecchio, DOPO_PRONTO),
    }
}

/// Aspetta `quanto`, o fino a una richiesta a mano.
///
/// `Some(true)` per «Controlla adesso», `Some(false)` quando il tempo è
/// scaduto da sé, `None` quando il canale si è chiuso. Un `Pronto` arrivato
/// qui — la finestra che si ridisegna — non interrompe niente.
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

/// Com'è andato un giro del filo: è quel che decide quanto aspettare il
/// prossimo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Giro {
    /// Il controllo è andato a buon fine.
    Riuscito,
    /// Non c'era rete, o non rispondeva: si riprova presto.
    SenzaRete,
    /// Un guasto che non passa da sé — il manifesto storto, una firma: si
    /// riprova al passo normale.
    Fallito,
    /// Non si è controllato: spento, non configurato, già in corso, o la
    /// guardia d'avvio.
    Saltato,
}

/// Quanto aspettare il prossimo giro, e quante riprove senza rete restano.
///
/// Pura, e la ragione è la stessa di [`troppo_presto`]: è la riga che decide
/// se un portatile senza rete all'avvio scopre l'aggiornamento dopo un minuto o
/// dopo mezz'ora, e una riga così si prova.
///
/// Dopo un giro senza rete, la prossima delle [`RIPROVE_SENZA_RETE`]; finite
/// quelle, [`INTERVALLO`] senza azzerare il conto — un computer che resta
/// offline per giorni non deve riprovare ogni minuto a ogni mezz'ora. Ogni
/// altro esito azzera il conto e torna a [`INTERVALLO`].
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

/// Quando è andato a buon fine l'ultimo controllo, se il database lo sa.
///
/// `None` per tutto quel che non è una risposta: libreria non aperta, chiave mai
/// scritta, testo che non è un numero. Sono tutti casi in cui la risposta giusta
/// è «non si sa», e non sapere vuol dire controllare.
fn ultimo_controllo(app: &AppHandle) -> Option<i64> {
    let stato = app.try_state::<Stato>()?;
    con_libreria(&stato, |libreria| {
        Ok(settings::read(&libreria.connection, CHIAVE_ULTIMO)?
            .and_then(|testo| testo.parse::<i64>().ok()))
    })
    .ok()
    .flatten()
}

/// L'ultimo controllo riuscito aveva trovato una versione nuova?
///
/// Falso per tutto quel che non è un sì: libreria non aperta, chiave mai
/// scritta. Vuol dire lasciare la guardia com'era.
fn versione_trovata_prima(app: &AppHandle) -> bool {
    let Some(stato) = app.try_state::<Stato>() else {
        return false;
    };
    con_libreria(&stato, |libreria| {
        settings::read(&libreria.connection, CHIAVE_TROVATA)
    })
    .ok()
    .flatten()
    .is_some_and(|versione| !versione.trim().is_empty())
}

/// L'ultimo controllo è troppo recente perché valga la pena rifarlo?
///
/// Pura, e con l'orologio passato da fuori, perché è la riga che decide se dieci
/// riaperture in un minuto sono dieci richieste o una: una riga così si prova.
/// Il confine è [`GUARDIA`].
///
/// # I due modi in cui si risponde «no»
///
/// Senza un `ultimo_ms` — prima installazione, o chiave cancellata — si
/// controlla: non sapere non è una ragione per stare zitti.
///
/// E si controlla anche quando l'istante salvato è nel **futuro**, che succede a
/// chiunque sposti avanti l'orologio del computer e poi lo rimetta indietro. Un
/// `adesso - ultimo` negativo confrontato con una durata darebbe «recente», e
/// quella copia di Rymembra non controllerebbe più nulla fino al giorno scritto nel
/// database. La sottrazione è satura e il confronto vuole un risultato **non
/// negativo**: un istante impossibile vale come nessun istante.
fn troppo_presto(ultimo_ms: Option<i64>, adesso_ms: i64) -> bool {
    let Some(ultimo_ms) = ultimo_ms else {
        return false;
    };
    let passati = adesso_ms.saturating_sub(ultimo_ms);
    // `try_from` e non `as`: la conversione è infallibile per cinque minuti di
    // millisecondi, ma scriverlo con un cast vorrebbe dire una deroga al lint
    // che vieta i troncamenti silenziosi in tutto il resto del programma.
    let guardia = i64::try_from(GUARDIA.as_millis()).unwrap_or(i64::MAX);
    (0..guardia).contains(&passati)
}

/// Un controllo. Non fallisce mai rumorosamente.
///
/// `a_mano` è il controllo chiesto da una persona — «Controlla adesso», o
/// l'interruttore appena acceso — e quello non guarda l'interruttore. Lo
/// guardava: con il controllo automatico spento il tasto non faceva niente, né
/// una richiesta né un evento, e il suggerimento sotto l'interruttore diceva
/// proprio di usare quel tasto per controllare quando si vuole.
fn passata(app: &AppHandle, a_mano: bool) -> Giro {
    if !configurato(app) {
        return Giro::Saltato;
    }
    let acceso = a_mano
        || app
            .try_state::<Stato>()
            .and_then(|stato| con_libreria(&stato, |libreria| attivo(&libreria.connection)).ok())
            .unwrap_or(true);
    if !acceso {
        return Giro::Saltato;
    }
    let Some(aggiornamenti) = app.try_state::<StatoAggiornamenti>() else {
        return Giro::Saltato;
    };
    // Se un controllo è già in corso — l'utente ha premuto «controlla adesso»
    // proprio mentre scadeva la mezz'ora — questo giro salta e basta.
    //
    // `Turno` e non due `store`, per la ragione scritta sulla guardia stessa: in
    // mezzo ci sono dei `?` e due `return`, e un bit lasciato alzato qui è una
    // finestra che dice «controllo in corso» per sempre, con «Controlla adesso»
    // spento e nessun errore da nessuna parte. Il `Drop` lo abbassa anche
    // uscendo da un ramo che nessuno ha ancora scritto.
    //
    // Quel che il `Drop` **non** può fare è arrivare: resta preso per tutto il
    // tempo che `controlla` passa dentro la richiesta di rete, e senza una
    // scadenza quel tempo non ha un limite. Le due cose si tengono insieme —
    // vedi [`SCADENZA_RICHIESTA`].
    let Some(_turno) = Turno::prendi(&aggiornamenti.in_corso) else {
        return Giro::Saltato;
    };
    riferisci(app);

    let esito = controlla(app);
    let giro = match &esito {
        Ok(_) => Giro::Riuscito,
        Err(err) if err.is_retryable() => Giro::SenzaRete,
        Err(_) => Giro::Fallito,
    };
    match esito {
        Ok(trovato) => {
            let versione = trovato.as_ref().map(|nuovo| nuovo.version.clone());
            deposita(&aggiornamenti, trovato);
            azzera_errore(&aggiornamenti);
            if let Some(stato) = app.try_state::<Stato>() {
                let _ = con_libreria(&stato, |libreria| {
                    settings::write(
                        &libreria.connection,
                        CHIAVE_ULTIMO,
                        &adesso_ms().to_string(),
                    )?;
                    match &versione {
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
            let mut ultimo = aggiornamenti
                .ultimo_errore
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *ultimo = Some(ErroreIpc::from(err));
        }
    }
    drop(_turno);
    riferisci(app);
    giro
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

/// Costruisce l'updater, con la scadenza e l'endpoint di prova.
///
/// La scadenza va messa **qui** e non nella configurazione: `tauri.conf.json` non
/// ha un campo per dirla, e il costruttore è l'unico posto in cui il plugin
/// l'accetta. Vale per il `latest.json` e non per lo scaricamento
/// dell'installer, che è quel che si vuole: vedi [`SCADENZA_RICHIESTA`].
///
/// # Perché qui **non** si mette un `User-Agent`, ed è una scelta
///
/// Perché mandarne uno vorrebbe dire mandare il nome del programma, e con lui la
/// versione installata — e il §6 di `PRIVACY.md` promette il contrario, per
/// esteso: «nemmeno la versione che hai installata viaggia». Quella frase è un
/// impegno preso con chi legge, non una descrizione da tenere aggiornata: si
/// mantiene non scrivendo questa riga. Quel che parte resta
/// `tauri-plugin-updater/<versione del plugin>`, cioè il nome di una libreria,
/// che di questa copia di Rymembra non dice niente.
///
/// Sta scritto perché **un'assenza non si distingue da una dimenticanza**, e la
/// prossima persona che passa di qui aggiungerebbe l'intestazione «per
/// educazione verso GitHub» senza sapere di star rompendo una promessa.
///
/// # Il prezzo, che si accetta sapendolo
///
/// Un proxy che lascia passare solo i `User-Agent` che riconosce — la regola
/// «blocca tutto ciò che non è un browser», comune nei filtri aziendali — vede
/// passare il nome di una libreria e lo blocca. E lo fa **in silenzio**: il
/// guasto arriva come `net.offline`, indistinguibile da una rete che non c'è, e
/// a nessuno viene in mente un'intestazione. È un sintomo difficile da
/// diagnosticare al buio, e per questo la diagnosi sta scritta qui prima che
/// serva: **se qualcuno segnala che gli aggiornamenti non si trovano mai da una
/// certa rete, mentre da quella stessa rete il browser apre senza problemi la
/// pagina delle release, questa è la prima cosa da guardare.** Il rimedio, se
/// servirà, è una decisione da prendere con `PRIVACY.md` aperto accanto — non una
/// riga da aggiungere di corsa.
fn costruisci(app: &AppHandle) -> Result<Updater, AppError> {
    // `on_before_exit` **sostituisce** quel che il plugin ci mette di suo —
    // `cleanup_before_exit`, che chiude le finestre e gli stati di Tauri — e
    // quindi lo si rifà qui, dopo la sequenza nostra. L'ordine conta: prima si
    // scrive la posizione dell'ascolto e si stacca la scheda del sistema, che
    // hanno bisogno degli stati, poi Tauri smonta gli stati.
    let mano = app.clone();
    let costruttore = app
        .updater_builder()
        .timeout(SCADENZA_RICHIESTA)
        .on_before_exit(move || {
            nota!("[aggiornamenti] l'installer prende il posto di Rymembra: si spegne tutto");
            crate::spegnimento::spegni(&mano);
            mano.cleanup_before_exit();
        });
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
///
/// # Chi abbassa il bit dell'installazione
///
/// Lo abbassa il `Drop` di un [`Turno`] **adottato**: il bit l'ha alzato il
/// comando, un istante prima di avviare questo filo, e là sta scritto perché
/// (fra il clic e lo `spawn` ci sta un secondo clic). Qui c'è solo la promessa di
/// rilasciarlo, che è esattamente il caso per cui `Turno::adotta` esiste.
///
/// Con uno `store(false)` a mano — com'era — il bit restava alzato se il lavoro
/// srotolava per un panico, e il panico non è teorico: `download_and_install`
/// gira dentro codice del plugin, su byte arrivati dalla rete. Il sintomo era il
/// tasto «Aggiorna» spento fino al riavvio, senza nessun errore da nessuna parte
/// — cioè la stessa famiglia di guasti per cui `Turno` è stato scritto.
fn scarica_e_installa(app: &AppHandle, aggiornamento: &Update) {
    // Prima di tutto il resto, e prima di qualunque cosa che possa cadere.
    // L'ordine di dichiarazione è l'ordine di rilascio al contrario: il turno si
    // scioglie prima del prestito dello stato da cui pende.
    let aggiornamenti = app.try_state::<StatoAggiornamenti>();
    let turno = aggiornamenti
        .as_ref()
        .map(|stato| Turno::adotta(&stato.installazione));

    if let Some(stato) = aggiornamenti.as_ref() {
        stato.annulla.store(false, Ordering::Release);
    }

    // Scaricare e installare in due tempi, e la musica si ferma **in mezzo**:
    // prima che l'installer prenda in mano il processo, perché il plugin, su
    // Windows, esce con `process::exit(0)` — senza la pausa il dispositivo audio
    // si chiuderebbe di colpo a metà di un brano, con il rumore che ne consegue,
    // e la posizione dell'ascolto in corso non verrebbe scritta da nessuna
    // parte. Ma non prima dello scaricamento, dov'era: sono minuti in cui
    // l'applicazione funziona ancora, e un disco zittito al clic su «Aggiorna»
    // per tutto quel tempo — o per sempre, se lo scaricamento o la firma non
    // vanno — era una pausa che nessuno aveva chiesto.
    let esito = scarica(app, aggiornamento).and_then(|byte| {
        fermare_la_musica(app);
        aggiornamento.install(byte).map_err(guasto)
    });

    // Se si arriva qui su Windows, è andata male: l'installazione riuscita non
    // torna mai, perché il plugin fa uscire il processo. Un annullamento non è
    // un guasto: non si annota e non si mostra.
    if let (Some(aggiornamenti), Err(err)) = (aggiornamenti.as_ref(), esito)
        && !matches!(err.code(), ErrorCode::InternalAborted { .. })
    {
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
    // Il turno **prima** del racconto, come nella passata: la finestra deve
    // ricevere uno stato in cui l'installazione è già finita, non uno che dice
    // ancora di sì.
    drop(turno);
    riferisci(app);
}

/// Scarica l'installer e ne verifica la firma, sorvegliando lo scaricamento.
///
/// # Perché non basta `block_on(download)`, com'era
///
/// Perché il plugin costruisce lo scaricamento **senza scadenza** (il
/// `timeout` dell'oggetto [`Update`] nasce `None`), e una scadenza
/// complessiva qui sarebbe sbagliata comunque: settanta megabyte su una linea
/// lenta ci mettono minuti, e sono minuti buoni. Quel che va fermato è il
/// **silenzio**: una connessione che non cade e non porta più niente, che
/// teneva il filo — e il tasto «Aggiorna» spento — fino al riavvio.
///
/// Lo scaricamento gira quindi sul runtime come compito suo, e questo filo lo
/// guarda ogni [`PASSO_SORVEGLIANZA`]: se da [`SILENZIO_MASSIMO`] non arriva un
/// byte, o se qualcuno ha premuto «Annulla» ([`aggiornamenti_annulla`]), o se
/// l'applicazione sta uscendo, lo interrompe. I byte arrivati restano in
/// memoria e se ne vanno con lui: niente da pulire sul disco.
///
/// # Errori
///
/// `internal.aborted` se è stato annullato, `net.timeout` se si è fermato, e
/// quelli di [`guasto`] per tutto quel che dice il plugin — firma compresa.
fn scarica(app: &AppHandle, aggiornamento: &Update) -> Result<Vec<u8>, AppError> {
    let inizio = Instant::now();
    // Quando è arrivato l'ultimo byte, in millisecondi da `inizio`. Lo scrive il
    // compito dello scaricamento, lo legge questo filo.
    let ultimo_byte = Arc::new(AtomicU64::new(0));
    let suo_ultimo = Arc::clone(&ultimo_byte);
    let mano = app.clone();
    let mut scaricati: u64 = 0;
    let mut ultimo_detto = Instant::now()
        .checked_sub(RESPIRO_AVANZAMENTO)
        .unwrap_or_else(Instant::now);
    let per_pezzo = move |quanto: usize, totale: Option<u64>| {
        scaricati = scaricati.saturating_add(u64::try_from(quanto).unwrap_or(0));
        suo_ultimo.store(
            u64::try_from(inizio.elapsed().as_millis()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        // Il primo pezzo e l'ultimo si dicono sempre; quelli in mezzo con un
        // respiro, o la finestra riceverebbe migliaia di eventi per disegnare
        // una barra che si muove di un pixel.
        let adesso = Instant::now();
        let finito = totale.is_some_and(|totale| scaricati >= totale);
        if finito || adesso.duration_since(ultimo_detto) >= RESPIRO_AVANZAMENTO {
            ultimo_detto = adesso;
            mano.emetti(
                "aggiornamenti:avanzamento",
                AvanzamentoIpc { scaricati, totale },
            );
        }
    };
    let suo = aggiornamento.clone();
    let lavoro = tauri::async_runtime::spawn(async move { suo.download(per_pezzo, || {}).await });

    let mut fermato: Option<AppError> = None;
    while !lavoro.inner().is_finished() {
        let annullato = crate::spegnimento::in_uscita()
            || app
                .try_state::<StatoAggiornamenti>()
                .is_some_and(|stato| stato.annulla.load(Ordering::Acquire));
        let silenzio = inizio
            .elapsed()
            .saturating_sub(Duration::from_millis(ultimo_byte.load(Ordering::Relaxed)));
        if annullato {
            fermato = Some(AppError::new(ErrorCode::InternalAborted {
                what: Some("scaricamento dell'aggiornamento".to_owned()),
            }));
        } else if silenzio >= SILENZIO_MASSIMO {
            fermato = Some(
                AppError::new(ErrorCode::NetTimeout {
                    url: None,
                    timeout_ms: u64::try_from(SILENZIO_MASSIMO.as_millis()).ok(),
                })
                .with_cause(format!(
                    "lo scaricamento dell'aggiornamento non riceve niente da {} secondi",
                    SILENZIO_MASSIMO.as_secs()
                )),
            );
        }
        if fermato.is_some() {
            lavoro.abort();
            break;
        }
        std::thread::sleep(PASSO_SORVEGLIANZA);
    }
    let esito = tauri::async_runtime::block_on(lavoro);
    if let Some(fermato) = fermato {
        return Err(fermato);
    }
    match esito {
        Ok(scaricato) => scaricato.map_err(guasto),
        Err(err) => Err(AppError::new(ErrorCode::InternalAborted {
            what: Some("scaricamento dell'aggiornamento".to_owned()),
        })
        .with_cause(err.to_string())),
    }
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
///
/// # Anche dopo i comandi che lo restituiscono
///
/// Perché lo stato lo guardano in due, la fascia e la scheda delle
/// Impostazioni, e un comando risponde solo a chi l'ha chiamato. Spegnendo il
/// controllo dalla scheda la fascia restava su con un «Aggiorna» che poi falliva
/// — l'aggiornamento trovato era stato buttato — e «Salta questa versione» dalla
/// fascia lasciava alla scheda un «Riproponi» che non compariva.
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

#[cfg(test)]
mod prove {
    use super::*;

    /// Un database vero, migrato, in memoria: la fixture di `rymembra-app`.
    fn libreria() -> rusqlite::Connection {
        rymembra_app::db::open_in_memory()
            .expect("un database in memoria si apre sempre")
            .connection
    }

    #[test]
    fn senza_nessuna_preferenza_scritta_il_controllo_e_acceso() {
        // È il confronto scritto al contrario — `!= Some("0")` e non
        // `== Some("1")` — e la differenza si vede solo qui: su un'installazione
        // nuova la chiave non esiste, e chi non ha mai detto niente deve sapere
        // che è uscita una versione nuova.
        let connection = libreria();
        assert!(attivo(&connection).expect("la lettura riesce"));
    }

    #[test]
    fn solo_uno_zero_esplicito_spegne_il_controllo() {
        let connection = libreria();
        settings::write(&connection, CHIAVE_ATTIVO, "0").expect("la scrittura riesce");
        assert!(!attivo(&connection).expect("la lettura riesce"));

        settings::write(&connection, CHIAVE_ATTIVO, "1").expect("la scrittura riesce");
        assert!(attivo(&connection).expect("la lettura riesce"));
    }

    #[test]
    fn i_tempi_si_tengono_fra_loro() {
        // La guardia deve essere più corta della periodicità, o il battito
        // periodico cadrebbe dentro la guardia di un avvio vicino; e il
        // controllo d'avvio deve partire prima che la sincronia (venti
        // secondi) e la nuvola (trenta) si sveglino.
        assert!(GUARDIA < INTERVALLO);
        assert!(DOPO_PRONTO < Duration::from_secs(20));
        assert!(DOPO_PRONTO < ATTESA_PRONTO_MASSIMA);
        // La rete di sicurezza di `main` mostra la finestra dopo due secondi:
        // l'attesa di `pronto` deve essere molto più larga, o la si
        // anticiperebbe.
        assert!(ATTESA_PRONTO_MASSIMA > Duration::from_secs(10));
        // Una scadenza più lunga del primo ritentativo vorrebbe dire una
        // richiesta ancora appesa quando parte la seconda.
        assert!(
            RIPROVE_SENZA_RETE
                .first()
                .is_some_and(|prima| SCADENZA_RICHIESTA < *prima)
        );
        assert!(RIPROVE_SENZA_RETE.iter().all(|attesa| *attesa < INTERVALLO));
    }

    #[test]
    fn la_passata_di_avvio_salta_solo_se_il_controllo_e_di_pochi_minuti_fa() {
        let adesso = 1_700_000_000_000;
        let guardia = 5 * 60 * 1_000;
        // Un minuto fa: si salta. È il caso di chi apre e chiude a raffica.
        assert!(troppo_presto(Some(adesso - 60 * 1_000), adesso));
        assert!(troppo_presto(Some(adesso - guardia + 1), adesso));
        // Il confine esatto, e oltre: si controlla. Chi riapre Rymembra dieci
        // minuti dopo — la voce #10 di `TODO.md` — controlla.
        assert!(!troppo_presto(Some(adesso - guardia), adesso));
        assert!(!troppo_presto(Some(adesso - 10 * 60 * 1_000), adesso));
        // Mai controllato: si controlla.
        assert!(!troppo_presto(None, adesso));
        // Un istante nel futuro — orologio spostato avanti e poi rimesso a
        // posto — non deve zittire gli aggiornamenti fino a quella data.
        assert!(!troppo_presto(Some(adesso + guardia), adesso));
    }

    #[test]
    fn senza_rete_si_riprova_a_uno_cinque_e_quindici_minuti() {
        let minuti = |m: u64| Duration::from_secs(m * 60);
        let (prima, conto) = prossima_attesa(Giro::SenzaRete, 0);
        assert_eq!((prima, conto), (minuti(1), 1));
        let (seconda, conto) = prossima_attesa(Giro::SenzaRete, conto);
        assert_eq!((seconda, conto), (minuti(5), 2));
        let (terza, conto) = prossima_attesa(Giro::SenzaRete, conto);
        assert_eq!((terza, conto), (minuti(15), 3));
        // Finite le tre, il passo normale — e il conto resta, così un computer
        // offline per giorni non riprova ogni minuto a ogni mezz'ora.
        let (dopo, conto) = prossima_attesa(Giro::SenzaRete, conto);
        assert_eq!((dopo, conto), (INTERVALLO, 3));
        // Un controllo riuscito azzera tutto.
        assert_eq!(prossima_attesa(Giro::Riuscito, conto), (INTERVALLO, 0));
        // E un guasto che non è la rete non accorcia niente.
        assert_eq!(prossima_attesa(Giro::Fallito, 0), (INTERVALLO, 0));
        assert_eq!(prossima_attesa(Giro::Saltato, 2), (INTERVALLO, 0));
    }

    #[test]
    fn un_pronto_tardivo_non_interrompe_l_attesa_e_un_tasto_si() {
        let (manda, orecchio) = channel();
        // Un `Pronto` che arriva mentre si aspetta il giro dopo — la finestra
        // che si ridisegna — si ignora, e il tempo scade da sé.
        manda.send(Sveglia::Pronto).expect("canale aperto");
        assert_eq!(aspetta(&orecchio, Duration::from_millis(30)), Some(false));
        // «Controlla adesso» interrompe subito.
        manda.send(Sveglia::Subito).expect("canale aperto");
        assert_eq!(aspetta(&orecchio, Duration::from_secs(60)), Some(true));
        // Un canale chiuso è l'uscita.
        drop(manda);
        assert_eq!(aspetta(&orecchio, Duration::from_secs(60)), None);
    }

    #[test]
    fn il_controllo_d_avvio_aspetta_pronto_poi_dieci_secondi_o_un_tasto() {
        let (manda, orecchio) = channel();
        // Un tasto durante l'attesa di `pronto` vale come controllo d'avvio,
        // e a mano.
        manda.send(Sveglia::Subito).expect("canale aperto");
        assert_eq!(aspetta_l_avvio(&orecchio), Some(true));
        // `Pronto` e poi un tasto nei dieci secondi: a mano anche lui.
        manda.send(Sveglia::Pronto).expect("canale aperto");
        manda.send(Sveglia::Subito).expect("canale aperto");
        assert_eq!(aspetta_l_avvio(&orecchio), Some(true));
    }

    #[test]
    fn una_richiesta_a_mano_non_rispetta_la_guardia() {
        // La composizione che sta nel filo, nei due versi: con la guardia alzata
        // — cioè quando la sveglia è stato il timeout d'avvio — un controllo
        // recente fa saltare la passata; con la guardia abbassata — «Controlla
        // adesso», o l'interruttore appena acceso — lo stesso istante recente
        // non ferma niente.
        let adesso = 1_700_000_000_000;
        let recente = Some(adesso - 60 * 1_000);
        assert!(
            troppo_presto(recente, adesso),
            "l'istante è recente davvero"
        );

        let dal_timeout = true;
        assert!(dal_timeout && troppo_presto(recente, adesso));

        let a_mano = false;
        assert!(!(a_mano && troppo_presto(recente, adesso)));
    }

    #[test]
    fn il_turno_abbassa_in_corso_anche_uscendo_per_errore() {
        // La ragione per cui `in_corso` non è due `store` a mano: la funzione
        // che lo alza ha dei `?` in mezzo, e un'uscita anticipata lascerebbe la
        // finestra con «controllo in corso» e «Controlla adesso» spento fino al
        // riavvio. Qui si esce con un `Err` prima di qualunque rilascio
        // esplicito, e il bit deve essere giù comunque.
        let (aggiornamenti, _orecchio) = StatoAggiornamenti::nuovo();
        assert!(!aggiornamenti.in_corso.load(Ordering::Relaxed));

        fn con_un_guasto(bandiera: &AtomicBool) -> Result<(), &'static str> {
            let _turno = Turno::prendi(bandiera).ok_or("il turno era già preso")?;
            assert!(bandiera.load(Ordering::Relaxed), "preso vuol dire alzato");
            // Nessun `drop` e nessuno `store`: si esce da qui.
            Err("la rete non risponde")
        }

        assert!(con_un_guasto(&aggiornamenti.in_corso).is_err());
        assert!(
            !aggiornamenti.in_corso.load(Ordering::Relaxed),
            "il Drop del Turno ha abbassato il bit uscendo per errore"
        );
        // E il turno si può riprendere: è questo che rende il filo capace di
        // ripartire al battito dopo.
        assert!(Turno::prendi(&aggiornamenti.in_corso).is_some());
    }

    #[test]
    fn un_valore_che_nessuno_scrive_non_spegne_niente() {
        // Il default è acceso, e resta acceso per qualunque cosa che non sia
        // lo zero: una chiave rimasta lì da una versione vecchia, o scritta a
        // mano nel database, non deve poter zittire gli aggiornamenti di
        // sicurezza per distrazione.
        let connection = libreria();
        settings::write(&connection, CHIAVE_ATTIVO, "").expect("la scrittura riesce");
        assert!(attivo(&connection).expect("la lettura riesce"));
        settings::write(&connection, CHIAVE_ATTIVO, "no").expect("la scrittura riesce");
        assert!(attivo(&connection).expect("la lettura riesce"));
    }
}
