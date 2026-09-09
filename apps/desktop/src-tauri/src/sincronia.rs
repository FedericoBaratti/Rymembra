//! La sincronia fra dispositivi, dal lato della finestra.
//!
//! Sette comandi, due eventi e un filo di sottofondo. È il gemello di
//! [`crate::nuvola`] e ne segue la forma alla lettera, perché risolve lo stesso
//! problema di orchestrazione: tenere il tempo, prendere il lucchetto in finestre
//! corte, e non tenerlo mai attorno alla rete.
//!
//! # Cosa la distingue dal backup
//!
//! Il backup è un file **solo**, di cui esiste una copia autorevole lassù, e la
//! regola che lo tiene in piedi è che il database locale non si tocca da soli.
//!
//! La sincronia è un file **per dispositivo**, nessuno dei quali è autorevole, e
//! scrive nella libreria da sé. È un cambiamento di regola importante e va detto
//! dove si vede: la paura che motivava l'altra — «una sincronia che riscrive la
//! libreria da sola è una sincronia di cui non ci si accorge finché non ha
//! cancellato qualcosa» — resta giusta, e la risposta è che qui non c'è niente
//! che possa cancellare. Ogni fusione è commutativa, associativa e idempotente;
//! l'unica cosa che sparisce è quel che qualcuno ha tolto **apposta**, e viaggia
//! come lapide. I test di convergenza in `aether-sync` lo dimostrano invece di
//! affermarlo, ed è la ragione per cui questo automatismo si può accendere.
//!
//! # L'ordine dei lucchetti
//!
//! > **Il filo della sincronia non prende mai il lucchetto del lettore, e prende
//! > quello della libreria in tre finestre brevi.**
//!
//! Allineare e leggere; poi la rete, senza niente in mano; poi applicare. Che sia
//! vero non è affidato all'attenzione di chi legge: `aether-sync` non riceve mai
//! una `rusqlite::Connection`, quindi il codice che terrebbe il lucchetto durante
//! una lettura di rete non si può nemmeno scrivere.
//!
//! # Un solo deposito alla volta
//!
//! O una cartella, o Drive. Non entrambi: due depositi vorrebbero dire che lo
//! stesso dispositivo pubblica due documenti con lo stesso nome in due posti, e
//! che l'utente ha due verità diverse senza sapere quale sta guardando. Cambiare
//! deposito butta via la memoria della passata, perché i documenti di una
//! cartella non dicono niente su cosa c'è su un Drive.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use crate::spegnimento::Emette as _;
use aether_app::settings;
use aether_app::sincronia::{self, Cambiamenti, Dispositivo};
use aether_cloud::http::Rete;
use aether_cloud::oauth::Credenziali;
use aether_cloud::pacchetti;
use aether_cloud::portachiavi::{self, DiSistema, Portachiavi as _};
use aether_domain::errors::{AppError, ErrorCode};
use aether_sync::{Cartella, Magazzino, Memoria, Motore};
use serde::Serialize;
use tauri::{AppHandle, Manager as _, State};

use crate::errore::{ErroreIpc, Esito, errore};
use crate::nota;
use crate::stato::{Stato, Turno, adesso_ms, con_libreria};

/// La chiave con cui l'interruttore della sincronia sta in `settings`.
const CHIAVE_ATTIVA: &str = "sincronia.attiva";

/// La chiave del deposito scelto: `cartella` o `drive`.
const CHIAVE_DOVE: &str = "sincronia.dove";

/// La chiave del percorso, quando il deposito è una cartella.
const CHIAVE_CARTELLA: &str = "sincronia.cartella";

/// La chiave del momento dell'ultima passata riuscita.
const CHIAVE_ULTIMA: &str = "sincronia.ultima_ms";

/// Ogni quanto si guarda se qualcuno ha scritto qualcosa.
///
/// Cinque minuti e non un quarto d'ora come il backup: là si spediscono
/// megabyte, qui su una libreria ferma la passata è **un'elencazione sola** — su
/// una cartella condivisa nemmeno una richiesta di rete. Il costo di guardare
/// spesso è vicino a zero, e il guadagno è che un cuoricino messo sul telefono si
/// vede sul portatile mentre si è ancora lì a guardare.
const INTERVALLO: Duration = Duration::from_secs(5 * 60);

/// Quanto si aspetta prima della prima passata.
///
/// L'avvio dell'applicazione è il momento in cui si scansiona, si legge la coda e
/// si disegna la finestra. Una passata di sincronia in mezzo a tutto questo
/// rallenta l'unica cosa che l'utente sta guardando.
const ATTESA_AVVIO: Duration = Duration::from_secs(20);

/// Quanto si aspetta che una raffica di modifiche finisca.
const RAFFICA: Duration = Duration::from_secs(30);

/// Il tempo massimo di una richiesta a Drive.
const SCADENZA_DRIVE: Duration = Duration::from_secs(120);

/// Perché il filo si è svegliato.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sveglia {
    /// Qualcosa è cambiato: aspetta che la raffica finisca.
    Sporca,
    /// Qualcuno ha chiesto di sincronizzare adesso.
    Subito,
}

/// Quel che la sincronia tiene aperto mentre l'applicazione gira.
pub struct StatoSincronia {
    /// Come si sveglia il filo.
    sveglia: Sender<Sveglia>,

    /// C'è già una passata in corso.
    ///
    /// Non dietro un mutex, per la stessa ragione di `StatoNuvola::in_corso`: un
    /// comando che chiede lo stato non deve mettersi in coda dietro la passata di
    /// cui vuole parlare.
    in_corso: AtomicBool,

    /// L'esito dell'ultima passata automatica. `None` vuol dire riuscita.
    ultimo_errore: Mutex<Option<ErroreIpc>>,

    /// I documenti già letti, da una passata all'altra.
    ///
    /// È la cosa che rende una passata a vuoto gratuita: senza, ogni cinque
    /// minuti si riscaricherebbero tutti i documenti di tutti i dispositivi per
    /// poi constatare che non è cambiato niente.
    ///
    /// **Il lucchetto non si tiene mentre si va in rete.** Vedi
    /// [`StatoSincronia::epoca_memoria`].
    memoria: Mutex<Memoria>,

    /// Quante volte la memoria è stata buttata via.
    ///
    /// # Il problema che risolve
    ///
    /// La passata dura quanto dura la rete: due minuti su Drive, senza limite
    /// su un deposito che sta in una cartella di rete. Finché il lucchetto della
    /// memoria restava in mano per tutto quel tempo, `sincronia_magazzino` e
    /// `sincronia_dimentica` — che lo vogliono per svuotarla — aspettavano lì, e
    /// con loro la finestra: cambiare deposito durante una sincronizzazione era
    /// un'applicazione che smetteva di rispondere.
    ///
    /// # Perché un'epoca e non un lucchetto più fine
    ///
    /// Perché la memoria **è una cache**: buttarla via costa una rilettura, non
    /// un dato. Quindi la passata se ne prende una copia e lascia subito il
    /// lucchetto; alla fine lo ripiglia e riscrive la copia **solo se nessuno ha
    /// dimenticato niente nel frattempo**. Se l'epoca è cambiata, quella copia
    /// contiene documenti di un deposito che non è più quello, o di un
    /// dispositivo che l'utente ha appena tolto: si lascia cadere, ed è
    /// esattamente quel che «dimentica» voleva dire.
    ///
    /// `Relaxed` basta: il lucchetto della memoria è la barriera vera, e questo
    /// numero viaggia sempre dentro di essa.
    epoca_memoria: AtomicU64,

    /// L'ultima passata, per raccontarla alla finestra.
    ultima: Mutex<Option<Resoconto>>,
}

impl StatoSincronia {
    /// Costruisce lo stato e il capo del canale che il filo dovrà ascoltare.
    #[must_use]
    pub fn nuovo() -> (Self, Receiver<Sveglia>) {
        let (sveglia, orecchio) = channel();
        (
            Self {
                sveglia,
                in_corso: AtomicBool::new(false),
                ultimo_errore: Mutex::new(None),
                memoria: Mutex::new(Memoria::nuova()),
                epoca_memoria: AtomicU64::new(0),
                ultima: Mutex::new(None),
            },
            orecchio,
        )
    }

    /// Butta via i documenti già letti, e lo dice a chi li sta usando.
    ///
    /// Le due righe stanno insieme in un metodo e non sparse nei due comandi che
    /// le chiamano perché dimenticare **senza** far avanzare l'epoca sarebbe un
    /// guasto silenzioso: la passata in corso ha in mano una copia di quel che si
    /// è appena buttato, e alla fine la riscriverebbe al suo posto. Dimenticato
    /// per un istante, e poi di nuovo lì.
    fn dimentica_la_memoria(&self) {
        self.epoca_memoria.fetch_add(1, Ordering::Relaxed);
        self.memoria
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .dimentica();
    }
}

// ── quel che la finestra vede ───────────────────────────────────────────────

/// Lo stato della sincronia, come lo mostrano le Impostazioni.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoSincroniaIpc {
    /// La sincronia automatica è accesa.
    pub attiva: bool,
    /// Dove si depositano i documenti: `"cartella"` o `"drive"`.
    pub dove: String,
    /// La cartella scelta, quando il deposito è una cartella.
    pub cartella: Option<String>,
    /// Il deposito è utilizzabile davvero.
    ///
    /// Una cartella non scelta e un Drive non collegato sono due modi diversi di
    /// non essere pronti, e la finestra deve poterli distinguere da «acceso».
    pub pronta: bool,
    /// L'identificativo di questo dispositivo.
    pub io: String,
    /// I dispositivi conosciuti.
    pub dispositivi: Vec<DispositivoIpc>,
    /// Quando è finita l'ultima passata riuscita.
    pub ultima_ms: Option<i64>,
    /// Una passata è in corso adesso.
    pub in_corso: bool,
    /// Com'è andata l'ultima passata.
    pub resoconto: Option<Resoconto>,
    /// L'ultimo guasto, se c'è stato.
    pub errore: Option<ErroreIpc>,
}

/// Un dispositivo, per la finestra.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DispositivoIpc {
    /// L'identificativo con cui firma i suoi documenti.
    pub id: String,
    /// Come si chiama, se qualcuno gliel'ha detto.
    pub nome: Option<String>,
    /// Se ci si fida di quel che scrive.
    pub fidato: bool,
    /// L'ultima volta che il suo documento è stato letto.
    pub visto_ms: Option<i64>,
    /// È questo computer.
    pub sono_io: bool,
}

/// Cosa è cambiato nell'ultima passata.
///
/// La schermata dei conflitti che si sarebbe tentati di costruire qui non serve:
/// con un CRDT che converge per costruzione non ci sono conflitti da mostrare. Ma
/// **qualcosa** va mostrato, o un automatismo che riscrive la libreria diventa
/// una scatola nera. Questo è quel qualcosa: «142 ascolti, 3 playlist rifatte» è
/// una frase che si legge, «sincronizzato» no.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resoconto {
    /// Quando.
    pub quando_ms: i64,
    /// Quanti documenti altrui sono stati letti.
    pub letti: usize,
    /// Quanti erano già in mano, immutati.
    pub saltati: usize,
    /// Il proprio documento è stato riscritto.
    pub scritto: bool,
    /// Quanti documenti non si sono potuti leggere.
    pub guasti: usize,
    /// Cosa è cambiato nella libreria.
    pub cambiamenti: Cambiamenti,
}

// ── i comandi ───────────────────────────────────────────────────────────────

/// Lo stato della sincronia.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
#[tauri::command]
pub fn sincronia_stato(
    stato: State<'_, Stato>,
    sincronia: State<'_, StatoSincronia>,
) -> Esito<StatoSincroniaIpc> {
    stato_ipc(&stato, &sincronia).map_err(errore)
}

/// Accende o spegne la sincronia automatica.
///
/// Accendendola parte subito una passata: chi ha appena acceso un interruttore ha
/// il diritto di vedere che è successo qualcosa, e aspettare cinque minuti senza
/// segnali è indistinguibile da un interruttore rotto.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce.
#[tauri::command(async)]
pub fn sincronia_attiva(
    app: AppHandle,
    stato: State<'_, Stato>,
    sincronia: State<'_, StatoSincronia>,
    accesa: bool,
) -> Esito<StatoSincroniaIpc> {
    con_libreria(&stato, |libreria| {
        settings::write(
            &libreria.connection,
            CHIAVE_ATTIVA,
            if accesa { "1" } else { "0" },
        )
    })
    .map_err(errore)?;
    if accesa {
        let _ = sincronia.sveglia.send(Sveglia::Subito);
    }
    let _ = &app;
    stato_ipc(&stato, &sincronia).map_err(errore)
}

/// Sceglie dove depositare i documenti.
///
/// `dove` è `"cartella"` o `"drive"`. Cambiare deposito butta via la memoria
/// della passata: i documenti letti da una cartella non dicono niente su cosa c'è
/// su un Drive, e tenerli fonderebbe due depositi che l'utente ha voluto
/// separati.
///
/// # Errori
///
/// `fs.pathInvalid` se il percorso non è una cartella utilizzabile.
/// `db.queryFailed` se la scrittura fallisce.
#[tauri::command(async)]
pub fn sincronia_magazzino(
    stato: State<'_, Stato>,
    sincronia: State<'_, StatoSincronia>,
    dove: String,
    cartella: Option<String>,
) -> Esito<StatoSincroniaIpc> {
    (|| -> Result<(), AppError> {
        let dove = match dove.as_str() {
            "cartella" | "drive" => dove.as_str(),
            altro => {
                return Err(AppError::new(ErrorCode::InternalUnexpected {
                    detail: Some(format!("deposito sconosciuto: {altro}")),
                }));
            }
        };
        if dove == "cartella" {
            let percorso = cartella.as_deref().unwrap_or("").trim().to_owned();
            if percorso.is_empty() {
                return Err(AppError::new(ErrorCode::FsPathInvalid { path: percorso }));
            }
            // Si crea adesso e non alla prima passata: un percorso sbagliato deve
            // dirlo mentre l'utente è ancora davanti alla finestra che glielo ha
            // chiesto, non fra cinque minuti dentro un filo di sottofondo.
            std::fs::create_dir_all(&percorso).map_err(|err| {
                AppError::new(ErrorCode::FsPathInvalid {
                    path: percorso.clone(),
                })
                .with_cause(err.to_string())
            })?;
            con_libreria(&stato, |libreria| {
                settings::write(&libreria.connection, CHIAVE_CARTELLA, &percorso)
            })?;
        }
        con_libreria(&stato, |libreria| {
            settings::write(&libreria.connection, CHIAVE_DOVE, dove)
        })
    })()
    .map_err(errore)?;

    sincronia.dimentica_la_memoria();
    stato_ipc(&stato, &sincronia).map_err(errore)
}

/// Sincronizza adesso.
///
/// `(async)`: la passata parla con Drive, e la scadenza di quelle richieste è
/// di due minuti — un comando normale li passerebbe sul filo principale della
/// finestra.
///
/// # Errori
///
/// `sync.busy` se una passata è già in corso; i codici del deposito altrimenti.
#[tauri::command(async)]
pub fn sincronia_adesso(app: AppHandle, sincronia: State<'_, StatoSincronia>) -> Esito<Resoconto> {
    if sincronia
        .in_corso
        .load(std::sync::atomic::Ordering::Acquire)
    {
        return Err(errore(AppError::new(ErrorCode::SyncBusy)));
    }
    let esito = passata_vera(&app);
    ricorda(&app, &esito);
    riferisci(&app);
    esito.map_err(errore)
}

/// I dispositivi conosciuti.
///
/// # Errori
///
/// `db.queryFailed` se la lettura fallisce.
#[tauri::command]
pub fn sincronia_dispositivi(stato: State<'_, Stato>) -> Esito<Vec<DispositivoIpc>> {
    con_libreria(&stato, |libreria| {
        let io = crate::nuvola::dispositivo(&libreria.connection)?;
        Ok(in_ipc(sincronia::dispositivi(&libreria.connection)?, &io))
    })
    .map_err(errore)
}

/// Accetta un dispositivo, dandogli un nome.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce.
#[tauri::command]
pub fn sincronia_accoppia(
    stato: State<'_, Stato>,
    id: String,
    nome: Option<String>,
) -> Esito<Vec<DispositivoIpc>> {
    con_libreria(&stato, |libreria| {
        sincronia::fidati(&libreria.connection, &id, nome.as_deref(), None)?;
        let io = crate::nuvola::dispositivo(&libreria.connection)?;
        Ok(in_ipc(sincronia::dispositivi(&libreria.connection)?, &io))
    })
    .map_err(errore)
}

/// Dimentica un dispositivo.
///
/// Gli ascolti che ha già portato restano: erano ascolti veri, e cancellarli
/// perché non si vuole più sentire quel telefono vorrebbe dire che dimenticare un
/// dispositivo riscrive la storia della libreria.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce.
#[tauri::command(async)]
pub fn sincronia_dimentica(
    stato: State<'_, Stato>,
    sincronia: State<'_, StatoSincronia>,
    id: String,
) -> Esito<Vec<DispositivoIpc>> {
    let elenco = con_libreria(&stato, |libreria| {
        sincronia::dimentica(&libreria.connection, &id)?;
        let io = crate::nuvola::dispositivo(&libreria.connection)?;
        Ok(in_ipc(sincronia::dispositivi(&libreria.connection)?, &io))
    })
    .map_err(errore)?;
    sincronia.dimentica_la_memoria();
    Ok(elenco)
}

// ── il filo di sottofondo ───────────────────────────────────────────────────

/// Segnala che c'è qualcosa di nuovo da sincronizzare.
///
/// Non se ne aggiunge un secondo meccanismo: `nuvola::sporca` è già chiamata da
/// ogni comando che cambia qualcosa, e questa le sta accanto nello stesso punto.
pub fn sporca(app: &AppHandle) {
    if let Some(sincronia) = app.try_state::<StatoSincronia>() {
        let _ = sincronia.sveglia.send(Sveglia::Sporca);
    }
}

/// Avvia il filo che sincronizza da solo.
///
/// Il ciclo sta in [`crate::stato::avvia_filo_periodico`]: è lo stesso della
/// nuvola e dell'arricchimento, con altre tre durate.
pub fn avvia_filo(app: AppHandle, orecchio: Receiver<Sveglia>) {
    let avviato = crate::stato::avvia_filo_periodico(
        "aether-sincronia",
        orecchio,
        ATTESA_AVVIO,
        RAFFICA,
        INTERVALLO,
        // Solo `Sporca` aspetta la raffica; `Subito` è una richiesta a mano, e
        // chi l'ha fatta sta guardando la finestra.
        |sveglia| *sveglia == Sveglia::Sporca,
        move || passata(&app),
    );
    if let Err(err) = avviato {
        nota!("[avvio] il filo della sincronia non è partito: {err}");
    }
}

/// Una passata automatica. Non fallisce mai rumorosamente.
fn passata(app: &AppHandle) {
    let acceso = app
        .try_state::<Stato>()
        .and_then(|stato| con_libreria(&stato, |libreria| attiva(&libreria.connection)).ok())
        .unwrap_or(false);
    if !acceso {
        return;
    }
    let esito = passata_vera(app);
    ricorda(app, &esito);
    if let Err(err) = &esito {
        nota!(
            "[sincronia] passata non riuscita codice={} causa={}",
            err.code().kind().code(),
            err.cause().unwrap_or("—")
        );
    }
    riferisci(app);
}

/// Il corpo della passata, con le tre finestre di lucchetto.
fn passata_vera(app: &AppHandle) -> Result<Resoconto, AppError> {
    let Some(stato) = app.try_state::<Stato>() else {
        return Err(AppError::new(ErrorCode::SyncBusy));
    };
    let Some(sincronia_stato) = app.try_state::<StatoSincronia>() else {
        return Err(AppError::new(ErrorCode::SyncBusy));
    };
    let Some(_turno) = Turno::prendi(&sincronia_stato.in_corso) else {
        return Err(AppError::new(ErrorCode::SyncBusy));
    };
    let adesso = adesso_ms();

    // ── finestra 1: allineare e leggere. ───────────────────────────────────
    let (io, dove, cartella, credenziali) = con_libreria(&stato, |libreria| {
        Ok((
            crate::nuvola::dispositivo(&libreria.connection)?,
            settings::read(&libreria.connection, CHIAVE_DOVE)?
                .unwrap_or_else(|| "cartella".to_owned()),
            settings::read(&libreria.connection, CHIAVE_CARTELLA)?,
            crate::nuvola::credenziali_condivise(&libreria.connection).ok(),
        ))
    })?;

    let (mio, fidati) = con_libreria(&stato, |libreria| {
        sincronia::allinea(&mut libreria.connection, &io, adesso)?;
        let skin = pacchetti::elenca_skin(&libreria.data_dir);
        let bozze = pacchetti::elenca_bozze(&libreria.data_dir);
        let fidati: Vec<String> = sincronia::dispositivi(&libreria.connection)?
            .into_iter()
            .filter(|dispositivo| dispositivo.fidato)
            .map(|dispositivo| dispositivo.id)
            .collect();
        Ok((
            sincronia::contenuto(&libreria.connection, &io, skin, bozze)?,
            fidati,
        ))
    })?;

    // ── senza nessun lucchetto: da qui in poi si legge il deposito. ─────────
    let deposito = magazzino(app, &dove, cartella.as_deref(), credenziali.as_ref())?;
    // La memoria si prende **in copia**, e il lucchetto si lascia sulla riga
    // dopo. Quel che segue è rete — due minuti su Drive, senza limite su un
    // deposito che sta in una cartella di rete — e per tutto quel tempo il
    // lucchetto deve essere libero, o `sincronia_magazzino` e
    // `sincronia_dimentica` restano lì ad aspettarlo con la finestra dietro.
    // L'epoca si legge adesso: dirà, alla fine, se questa copia ha ancora senso.
    let epoca = sincronia_stato.epoca_memoria.load(Ordering::Relaxed);
    let mut memoria = sincronia_stato
        .memoria
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();

    let esito = {
        // La fiducia si esercita **prima** della fusione, perché dopo non c'è
        // più modo di dire da chi veniva un voto. Il proprio dispositivo non
        // compare in `fidati` e non serve: il motore fonde comunque il proprio
        // documento, e chiedersi se ci si fida di sé è una domanda senza risposta
        // utile.
        let mut motore =
            Motore::nuovo(deposito.as_ref(), io.clone(), &mut memoria).fidandosi_di(fidati);
        motore.passata(&mio, adesso, &|passo| {
            app.emetti(
                "sincronia:avanzamento",
                serde_json::json!({ "fatti": passo.fatti, "totale": passo.totale }),
            );
        })
    };

    // E quel che si è letto torna a disposizione della passata dopo — ma solo se
    // nel frattempo nessuno ha chiesto di dimenticare. Se l'epoca è avanzata,
    // questa copia parla di un deposito che non è più quello o di un dispositivo
    // che l'utente ha appena tolto, e rimetterla al suo posto vorrebbe dire
    // annullare quel gesto senza dirlo a nessuno.
    //
    // Si riscrive anche quando la passata è **fallita**: i documenti già scaricati
    // prima del guasto sono buoni, e buttarli vorrebbe dire riscaricarli tutti al
    // primo tentativo dopo — cioè far pagare di più proprio la rete che sta già
    // andando male.
    {
        let mut in_carica = sincronia_stato
            .memoria
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if sincronia_stato.epoca_memoria.load(Ordering::Relaxed) == epoca {
            *in_carica = memoria;
        }
    }
    let esito = esito?;

    // ── finestra 3: applicare quel che è stato deciso. ──────────────────────
    let cambiamenti = con_libreria(&stato, |libreria| {
        let cambiamenti = sincronia::applica(&mut libreria.connection, &esito.fuso, adesso)?;
        // I dispositivi visti e non ancora accettati si registrano lo stesso,
        // come ignoti: è l'unico modo perché compaiano nell'elenco da cui li si
        // accetta. Un elenco che si popolasse solo dopo l'accettazione sarebbe
        // una porta la cui maniglia sta dall'altra parte.
        sincronia::intravisti(&libreria.connection, &esito.passata.ignoti, adesso)?;
        settings::write(&libreria.connection, CHIAVE_ULTIMA, &adesso.to_string())?;
        Ok(cambiamenti)
    })?;

    Ok(Resoconto {
        quando_ms: adesso,
        letti: esito.passata.letti,
        saltati: esito.passata.saltati,
        scritto: esito.passata.scritto,
        guasti: esito.passata.guasti.len(),
        cambiamenti,
    })
}

/// Il deposito scelto, pronto all'uso.
fn magazzino(
    app: &AppHandle,
    dove: &str,
    cartella: Option<&str>,
    credenziali: Option<&Credenziali>,
) -> Result<Box<dyn Magazzino>, AppError> {
    match dove {
        "drive" => {
            let Some(credenziali) = credenziali else {
                return Err(AppError::new(ErrorCode::SyncAuthExpired));
            };
            let access = crate::nuvola::accesso_condiviso(app, credenziali)?;
            Ok(Box::new(aether_cloud::Drive::nuovo(
                Rete::nuova("drive", SCADENZA_DRIVE),
                &access,
            )))
        }
        _ => {
            let Some(percorso) = cartella.filter(|p| !p.trim().is_empty()) else {
                return Err(AppError::new(ErrorCode::FsPathInvalid {
                    path: String::new(),
                }));
            };
            // I documenti vanno in una sottocartella e non nella radice: la
            // cartella che l'utente sceglie è quasi sempre una cartella di
            // Syncthing o di Dropbox che contiene già altro, e riempirgliela di
            // file `aether-*.json.gz` alla pari con i suoi è un modo di rendersi
            // antipatici che non serve a niente.
            Ok(Box::new(Cartella::nuova(
                PathBuf::from(percorso).join(aether_sync::documento::CARTELLA),
            )))
        }
    }
}

/// Ricorda com'è andata, per la finestra e per il prossimo `sincronia_stato`.
fn ricorda(app: &AppHandle, esito: &Result<Resoconto, AppError>) {
    let Some(sincronia) = app.try_state::<StatoSincronia>() else {
        return;
    };
    let mut ultimo = sincronia
        .ultimo_errore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *ultimo = match esito {
        Ok(_) => None,
        Err(err) => Some(ErroreIpc::from(err.clone())),
    };
    if let Ok(resoconto) = esito {
        let mut ultima = sincronia
            .ultima
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *ultima = Some(*resoconto);
    }
}

/// Manda alla finestra lo stato aggiornato.
fn riferisci(app: &AppHandle) {
    let (Some(stato), Some(sincronia)) =
        (app.try_state::<Stato>(), app.try_state::<StatoSincronia>())
    else {
        return;
    };
    if let Ok(ipc) = stato_ipc(&stato, &sincronia) {
        app.emetti("sincronia:stato", ipc);
    }
}

// ── i pezzi condivisi ───────────────────────────────────────────────────────

/// La sincronia automatica è accesa?
fn attiva(connection: &rusqlite::Connection) -> Result<bool, AppError> {
    Ok(settings::read(connection, CHIAVE_ATTIVA)?.as_deref() == Some("1"))
}

/// Lo stato, nella forma che la finestra riceve.
fn stato_ipc(stato: &Stato, sincronia: &StatoSincronia) -> Result<StatoSincroniaIpc, AppError> {
    let errore = sincronia
        .ultimo_errore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let resoconto = *sincronia
        .ultima
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let collegato = DiSistema
        .leggi(portachiavi::GOOGLE_REFRESH_TOKEN)
        .unwrap_or_default()
        .is_some();

    con_libreria(stato, |libreria| {
        let io = crate::nuvola::dispositivo(&libreria.connection)?;
        let dove = settings::read(&libreria.connection, CHIAVE_DOVE)?
            .unwrap_or_else(|| "cartella".to_owned());
        let cartella = settings::read(&libreria.connection, CHIAVE_CARTELLA)?;
        let pronta = match dove.as_str() {
            "drive" => collegato,
            _ => cartella.as_deref().is_some_and(|p| !p.trim().is_empty()),
        };
        Ok(StatoSincroniaIpc {
            attiva: attiva(&libreria.connection)?,
            dove,
            cartella,
            pronta,
            dispositivi: in_ipc(sincronia::dispositivi(&libreria.connection)?, &io),
            io,
            ultima_ms: settings::read(&libreria.connection, CHIAVE_ULTIMA)?
                .and_then(|quando| quando.parse().ok()),
            in_corso: sincronia
                .in_corso
                .load(std::sync::atomic::Ordering::Acquire),
            resoconto,
            errore,
        })
    })
}

/// I dispositivi nella forma della finestra, con sé stessi riconosciuti.
fn in_ipc(elenco: Vec<Dispositivo>, io: &str) -> Vec<DispositivoIpc> {
    elenco
        .into_iter()
        .map(|dispositivo| DispositivoIpc {
            sono_io: dispositivo.id == io,
            id: dispositivo.id,
            nome: dispositivo.nome,
            fidato: dispositivo.fidato,
            visto_ms: dispositivo.visto_ms,
        })
        .collect()
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Un database vero, migrato, in memoria: la fixture di `aether-app`.
    fn libreria() -> rusqlite::Connection {
        aether_app::db::open_in_memory()
            .expect("un database in memoria si apre sempre")
            .connection
    }

    #[test]
    fn su_una_libreria_nuova_la_sincronia_e_spenta() {
        // Il confronto opposto a quello degli aggiornamenti, e di proposito:
        // questa scrive nella libreria da sé, e una cosa che scrive nella
        // libreria non si accende perché nessuno ha detto il contrario.
        let connection = libreria();
        assert!(!attiva(&connection).expect("la lettura riesce"));
    }

    #[test]
    fn solo_un_uno_esplicito_la_accende() {
        let connection = libreria();
        settings::write(&connection, CHIAVE_ATTIVA, "1").expect("la scrittura riesce");
        assert!(attiva(&connection).expect("la lettura riesce"));

        settings::write(&connection, CHIAVE_ATTIVA, "0").expect("la scrittura riesce");
        assert!(!attiva(&connection).expect("la lettura riesce"));
    }

    #[test]
    fn un_valore_storto_lascia_la_sincronia_spenta() {
        let connection = libreria();
        settings::write(&connection, CHIAVE_ATTIVA, "true").expect("la scrittura riesce");
        assert!(!attiva(&connection).expect("la lettura riesce"));
    }
}
