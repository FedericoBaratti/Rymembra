//! Le cartelle dal cloud: i conti collegati, il loro filo, i comandi della
//! finestra.
//!
//! Non è il backup (`nuvola.rs`), benché parli anche lui con Google: il backup
//! scrive nella cartella privata dell'app, qui si **leggono** le cartelle di
//! musica dell'utente — OneDrive, Google Drive, Dropbox, WebDAV, MEGA — e si
//! mettono in libreria accanto a quelle del disco. Due consensi diversi, due
//! scope diversi, due posti dove rimediare quando qualcosa non va.
//!
//! # Chi fa cosa
//!
//! - `rymembra-cloud` parla coi servizi (i [`Deposito`]) e tiene la cache dei
//!   brani; non vede il database.
//! - Lo specchio (`rymembra_app::nuvola`) sa cosa c'è nelle cartelle scelte,
//!   senza chiederlo alla rete; si salva nel database.
//! - Questo modulo li tiene insieme: un filo per conto che elenca, ascolta i
//!   cambiamenti, aggiorna lo specchio e chiede alla sorveglianza di
//!   riguardare le cartelle toccate. La scansione fa il resto con le sue
//!   regole — passata A, righe subito; passata B, i tag a pezzi.
//!
//! # «Istantaneo», qui
//!
//! La cartella scelta entra fra le sorvegliate appena il servizio ha detto
//! dove sta (la catena dei suoi antenati e la prima pagina dei figli); la
//! prima scansione parte con quella pagina, e le pagine dopo arrivano in
//! libreria a ritmo di [`RITMO_SCANSIONI`] mentre l'elenco scorre. Nessuno
//! aspetta la fine dell'elenco per vedere i primi brani.

use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

use rymembra_app::files::{Archivi, LocalFiles, MusicFiles};
use rymembra_app::nuvola::{
    self as specchio, ContoSalvato, RadiceSalvata, Specchio, SpecchioCondiviso,
};
use rymembra_app::nuvola_app::{self, AppSalvata, StatoDelConto};
use rymembra_app::profilo::radici::CartelleOrdinate;
use rymembra_app::settings;
use rymembra_cloud::http::Rete;
use rymembra_cloud::musica::{
    Accesso, ArchivioNuvola, Cache, Chiave, CopieComplete, Depositi, Deposito, Dropbox,
    GoogleDrive, OneDrive, TETTO_DI_SERIE, WebDav, conto_di, cursore_scaduto,
};
use rymembra_cloud::oauth::{self, fornitore_della_musica};
use rymembra_cloud::portachiavi::{self, DiSistema, Portachiavi};
use rymembra_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use rymembra_domain::nuvola::{Servizio, conto_di as conto_del_percorso, e_virtuale, segmenti};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager as _, State};

use crate::disparte::in_disparte;
use crate::errore::{ErroreIpc, Esito, errore};
use crate::nota;
use crate::spegnimento::Emette as _;
use crate::stato::{Stato, Turno, adesso_ms, con_libreria};

/// La cache dei brani accesa o spenta. Di serie accesa.
const CHIAVE_CACHE_ATTIVA: &str = "nuvola.cache.attiva";

/// Il tetto della cache, in MB. Di serie [`TETTO_DI_SERIE`].
const CHIAVE_CACHE_TETTO: &str = "nuvola.cache.tetto_mb";

/// I tetti che la finestra offre, in MB.
const TETTI_OFFERTI: [u64; 6] = [200, 500, 1_000, 2_000, 5_000, 10_000];

/// Ogni quanto si chiedono i cambiamenti a un servizio che non sa avvisare.
///
/// Quarantacinque secondi: l'obiettivo è «un file cambiato su OneDrive si vede
/// entro un minuto», e una richiesta di delta senza novità è una risposta di
/// poche centinaia di byte. Chi sa aspettare i cambiamenti (Dropbox, MEGA)
/// usa questo tempo come attesa del long-poll e risponde prima.
const OGNI: Duration = Duration::from_secs(45);

/// Al più una scansione ogni tanto mentre l'elenco iniziale scorre.
///
/// Due secondi: la prima pagina va in libreria subito, le altre a gruppi. Una
/// scansione per pagina vorrebbe dire una scansione ogni duecento file — e un
/// turno di scansione conteso con le cartelle del disco.
const RITMO_SCANSIONI: Duration = Duration::from_secs(2);

/// La prima pausa dopo un guasto di rete; poi raddoppia fino a
/// [`PAUSA_MASSIMA`].
const PAUSA_GUASTO: Duration = Duration::from_secs(30);

/// La pausa più lunga fra due tentativi dopo dei guasti di fila.
const PAUSA_MASSIMA: Duration = Duration::from_secs(10 * 60);

// ── lo stato ────────────────────────────────────────────────────────────────

/// Com'è un conto adesso, per la finestra.
#[derive(Debug, Clone, Default)]
struct Andamento {
    /// L'elenco iniziale di qualche sua radice sta scorrendo.
    elenco: bool,
    /// Quante voci sono arrivate finora, mentre `elenco`.
    voci: u64,
    /// L'ultimo guasto, se l'ultima domanda al servizio è fallita.
    guasto: Option<AppError>,
    /// L'ultima volta che il servizio ha risposto.
    ultimo_ms: Option<i64>,
    /// Una scrittura dello specchio è caduta e nemmeno la rilettura del conto
    /// è riuscita: il giro dopo comincia rileggendolo. Vedi [`salva_toccate`].
    da_rileggere: bool,
    /// Le radici il cui elenco ha risposto «non c'è»: dette nel diario una
    /// volta, non a ogni giro.
    perse: HashSet<String>,
}

/// Quel che le cartelle dal cloud tengono aperto mentre l'applicazione gira.
pub struct StatoMusicaNuvola {
    specchio: SpecchioCondiviso,
    depositi: Depositi,
    /// `None` se la cartella della cache non si è potuta aprire: allora il
    /// cloud non c'è, e lo si dice.
    archivio: Option<Arc<ArchivioNuvola>>,
    andamenti: Mutex<HashMap<String, Andamento>>,
    /// Come si sveglia il filo di un conto (una radice nuova, «Aggiorna ora»,
    /// uno scollegamento).
    sveglie: Mutex<HashMap<String, Sender<()>>>,
    /// «Annulla» durante l'attesa del consenso.
    annulla: AtomicBool,
    /// Un collegamento è in corso.
    collegando: AtomicBool,
}

impl StatoMusicaNuvola {
    fn sveglia(&self, conto: &str) {
        if let Some(sveglia) = self
            .sveglie
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(conto)
        {
            let _ = sveglia.send(());
        }
    }

    fn deposito(&self, conto: &str) -> Option<Arc<dyn Deposito>> {
        self.depositi
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(conto)
            .cloned()
    }

    /// L'ultimo guasto del conto, se l'ultima domanda al servizio è fallita.
    fn guasto(&self, conto: &str) -> Option<AppError> {
        self.andamenti
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(conto)
            .and_then(|andamento| andamento.guasto.clone())
    }

    /// Vedi [`ArchivioNuvola::segna_respinto`]. Senza cloud non c'è niente da
    /// segnare.
    fn segna_respinto(&self, conto: &str, respinto: bool) {
        if let Some(archivio) = &self.archivio {
            archivio.segna_respinto(conto, respinto);
        }
    }

    fn andamento(&self, conto: &str, cambia: impl FnOnce(&mut Andamento)) {
        let mut andamenti = self
            .andamenti
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        cambia(andamenti.entry(conto.to_owned()).or_default());
    }
}

/// Lo stato, o l'errore di chi arriva mentre l'applicazione si chiude.
fn stati(app: &AppHandle) -> Result<(State<'_, Stato>, State<'_, StatoMusicaNuvola>), AppError> {
    match (
        app.try_state::<Stato>(),
        app.try_state::<StatoMusicaNuvola>(),
    ) {
        (Some(stato), Some(nuvola)) => Ok((stato, nuvola)),
        _ => Err(AppError::new(ErrorCode::InternalAborted {
            what: Some("cartelle dal cloud".to_owned()),
        })
        .with_cause("lo stato delle cartelle dal cloud non c'è più")),
    }
}

/// Le sorgenti dei file per chi li apre: il disco, e il cloud se c'è.
///
/// È ciò che la scansione, la riproduzione e i testi ricevono al posto di
/// `LocalFiles`. Un percorso del cloud senza cloud non si apre, e la sua
/// radice non è raggiungibile: la scansione tiene le righe come sono.
#[must_use]
pub fn archivi(app: &AppHandle) -> Arc<dyn MusicFiles> {
    let nuvola = app
        .try_state::<StatoMusicaNuvola>()
        .and_then(|stato| stato.archivio.clone())
        .map(|archivio| archivio as Arc<dyn MusicFiles>);
    Arc::new(Archivi {
        locali: LocalFiles,
        nuvola,
    })
}

/// Una scansione che ha rimandato delle letture (`ScanReport::rimasti`) cede
/// il turno: le sue cartelle del cloud tornano in fila **dietro** a quel che
/// la sorveglianza ha già in mano, e le letture riprendono al giro dopo.
///
/// È ciò che impedisce alla passata dei tag di una libreria da ventimila
/// brani su OneDrive — decine di minuti — di tenere ferme le scansioni delle
/// cartelle del disco: legge a pezzi, e fra un pezzo e l'altro passa chi
/// aspettava.
pub fn rimetti_in_fila(app: &AppHandle, cartelle: &[String], rimasti: usize) {
    if rimasti == 0 {
        return;
    }
    let del_cloud: Vec<String> = cartelle
        .iter()
        .filter(|cartella| rymembra_domain::nuvola::e_nuvola(cartella))
        .cloned()
        .collect();
    crate::sorveglia::riguarda(app, del_cloud);
}

/// Le sorgenti per l'analisi: il disco, e del cloud solo le copie complete.
#[must_use]
pub fn archivi_per_l_analisi(app: &AppHandle) -> Arc<dyn MusicFiles> {
    let nuvola = app
        .try_state::<StatoMusicaNuvola>()
        .and_then(|stato| stato.archivio.clone())
        .map(|archivio| Arc::new(CopieComplete::di(archivio)) as Arc<dyn MusicFiles>);
    Arc::new(Archivi {
        locali: LocalFiles,
        nuvola,
    })
}

/// Il percorso del file di una copia della cache, dallo specchio.
#[must_use]
pub fn percorso_della_copia(app: &AppHandle, chiave: &Chiave) -> Option<String> {
    let nuvola = app.try_state::<StatoMusicaNuvola>()?;
    let specchio = nuvola
        .specchio
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    specchio.percorso(&chiave.conto, &chiave.id)
}

/// La copia è stata misurata: la cache può lasciarla andare come le altre.
pub fn copia_misurata(app: &AppHandle, chiave: &Chiave) {
    if let Some(archivio) = archivio(app) {
        archivio.cache().segna_misurata(chiave);
    }
}

/// L'archivio del cloud, per chi apre un brano intero (la riproduzione, gli
/// attacchi dei testi, «Ascolta insieme»).
#[must_use]
pub fn archivio(app: &AppHandle) -> Option<Arc<ArchivioNuvola>> {
    app.try_state::<StatoMusicaNuvola>()
        .and_then(|stato| stato.archivio.clone())
}

// ── l'avvio ─────────────────────────────────────────────────────────────────

/// Registra lo stato, ricarica specchio e conti, riapre i depositi dai segreti
/// del portachiavi e avvia un filo per conto. Da chiamare nel `setup`, dopo la
/// libreria e **prima** della sorveglianza: la prima scansione automatica deve
/// già trovare il cloud.
pub fn avvia(app: &AppHandle) {
    let Some(stato) = app.try_state::<Stato>() else {
        return;
    };
    let (specchio_caricato, conti, cache_attiva, tetto) = con_libreria(&stato, |l| {
        let specchio_caricato = Specchio::carica(&l.connection)?;
        let conti = specchio::conti(&l.connection)?;
        let attiva = settings::read(&l.connection, CHIAVE_CACHE_ATTIVA)?.as_deref() != Some("0");
        let tetto = settings::read(&l.connection, CHIAVE_CACHE_TETTO)?
            .and_then(|mb| mb.trim().parse::<u64>().ok())
            .map_or(TETTO_DI_SERIE, |mb| mb.saturating_mul(1024 * 1024));
        Ok((specchio_caricato, conti, attiva, tetto))
    })
    .unwrap_or_else(|guasto| {
        nota!("[nuvola] lo specchio non si legge, si riparte vuoti: {guasto}");
        (Specchio::default(), Vec::new(), true, TETTO_DI_SERIE)
    });
    let specchio_condiviso: SpecchioCondiviso = Arc::new(RwLock::new(specchio_caricato));
    let depositi: Depositi = Arc::default();
    let archivio = match Cache::apri(cartella_cache(app, &stato), tetto, cache_attiva) {
        Ok(cache) => Some(Arc::new(ArchivioNuvola::nuovo(
            {
                // Una copia completa è la misura «mentre suonano»: il volume e
                // l'impronta di quel brano, dalla copia locale. Vedi `analisi`.
                let mano = app.clone();
                cache.al_completamento(move |chiave| {
                    crate::analisi::misura_dal_cloud(&mano, chiave.clone());
                    // Il numero della scheda in Impostazioni cresce mentre si
                    // ascolta: prima lo cambiavano solo «Svuota» e il tetto.
                    if let Some(nuvola) = mano.try_state::<StatoMusicaNuvola>() {
                        mano.emetti("nuvola:cache", stato_cache(&nuvola));
                    }
                });
                Arc::clone(&specchio_condiviso)
            },
            Arc::clone(&depositi),
            cache,
        ))),
        Err(guasto) => {
            nota!("[nuvola] la cache dei brani non si apre, cloud spento: {guasto}");
            None
        }
    };
    let nuvola = StatoMusicaNuvola {
        specchio: specchio_condiviso,
        depositi,
        archivio,
        andamenti: Mutex::new(HashMap::new()),
        sveglie: Mutex::new(HashMap::new()),
        annulla: AtomicBool::new(false),
        collegando: AtomicBool::new(false),
    };
    let mut da_avviare: Vec<String> = Vec::new();
    for conto in &conti {
        match riapri(&stato, conto) {
            Ok(deposito) => {
                nuvola
                    .depositi
                    .write()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(conto.conto.clone(), deposito);
                da_avviare.push(conto.conto.clone());
            }
            // Il motivo vero, non un «scaduto» per tutti: un conto senza app
            // si rimedia registrandola, uno con l'app cambiata ricollegandolo.
            Err(guasto) => nuvola.andamento(&conto.conto, |a| {
                a.guasto = Some(guasto);
                a.ultimo_ms = conto.ultimo_ms;
            }),
        }
    }
    app.manage(nuvola);
    // Le copie che l'ultima chiusura ha lasciato da misurare: l'avviso di
    // `al_completamento` per loro non ripartirà. Dopo `manage` e non prima,
    // perché la misura cerca la voce nello specchio attraverso lo stato
    // registrato (`percorso_della_copia`): senza, la troverebbe assente per
    // tutte e le lascerebbe andare senza misurarle.
    if let Some(registrato) = self::archivio(app) {
        crate::analisi::riproponi_dal_cloud(app, registrato.cache().da_misurare());
    }
    for conto in da_avviare {
        avvia_filo(app, &conto);
    }
}

/// Dove sta la cache dei brani.
///
/// Fuori dalla cartella dei dati, che sta in `%APPDATA%` e quindi va in giro
/// coi profili roaming: duecento megabyte di brani da ricopiare a ogni accesso
/// a un altro PC del dominio non li vuole nessuno. Con `RYMEMBRA_DATI`
/// impostata (le istanze di prova) sta invece **dentro** quella cartella, così
/// due istanze non si dividono la stessa cache.
fn cartella_cache(app: &AppHandle, stato: &Stato) -> PathBuf {
    let dentro_i_dati = std::env::var_os("RYMEMBRA_DATI").is_some();
    match (
        dentro_i_dati,
        stato.cartella_dati(),
        app.path().app_local_data_dir(),
    ) {
        (true, Some(dati), _) | (false, Some(dati), Err(_)) => dati.join("nuvola-cache"),
        (_, _, Ok(locale)) => locale.join("nuvola-cache"),
        (_, None, Err(_)) => std::env::temp_dir().join("rymembra-nuvola-cache"),
    }
}

/// Il deposito di un conto salvato: la sua app, e il suo segreto nel
/// portachiavi.
///
/// # Errori
///
/// Il motivo per cui il conto resta «da ricollegare» (le sue righe restano):
/// `cloud.clientMissing` se la sua app non c'è più, `cloud.appChanged` se ha
/// cambiato identificativo, `cloud.appSecretMissing`, `cloud.authExpired` se
/// il refresh token (o la password WebDAV) non c'è, `settings.secretUnavailable`
/// se il portachiavi non risponde.
fn riapri(stato: &Stato, conto: &ContoSalvato) -> Result<Arc<dyn Deposito>, AppError> {
    let nome = || conto.servizio.nome().to_owned();
    if conto.servizio == Servizio::WebDav {
        return riapri_webdav(conto, &DiSistema);
    }
    let fornitore = fornitore_della_musica(conto.servizio).ok_or_else(|| {
        AppError::new(ErrorCode::CloudClientMissing { servizio: nome() })
            .with_cause("servizio non ancora disponibile in questa versione")
    })?;
    let app = app_del_conto(stato, conto)?;
    if DiSistema
        .leggi(&portachiavi::nuvola_refresh(&conto.conto))?
        .is_none()
    {
        return Err(
            AppError::new(ErrorCode::CloudAuthExpired { servizio: nome() })
                .with_cause("nessun refresh token per questo conto"),
        );
    }
    let credenziali = crate::nuvola_app::credenziali_dell_app(&app)?;
    let accesso = Accesso::del_conto(
        Rete::nuova(conto.servizio.chiave(), oauth::SCADENZA),
        fornitore,
        credenziali,
        &conto.conto,
        Arc::new(DiSistema),
    );
    deposito_di(conto.servizio, Arc::new(accesso))
        .ok_or_else(|| AppError::new(ErrorCode::CloudClientMissing { servizio: nome() }))
}

/// L'app con cui riaprire un conto: vedi `nuvola_app::stato_del_conto`.
///
/// # Errori
///
/// `cloud.clientMissing` se non ce n'è una, `cloud.appChanged` se ha cambiato
/// identificativo da quando il conto ha dato il consenso.
fn app_del_conto(stato: &Stato, conto: &ContoSalvato) -> Result<AppSalvata, AppError> {
    let servizio = conto.servizio.nome().to_owned();
    match con_libreria(stato, |l| nuvola_app::stato_del_conto(&l.connection, conto))? {
        StatoDelConto::Pronta(app) => Ok(app),
        StatoDelConto::Cambiata(_) => Err(AppError::new(ErrorCode::CloudAppChanged { servizio })),
        StatoDelConto::Mancante => Err(AppError::new(ErrorCode::CloudClientMissing { servizio })),
    }
}

/// Un server WebDAV salvato: indirizzo e utente dai dettagli del conto, la
/// password dal portachiavi.
///
/// # Errori
///
/// `cloud.authExpired` se la password o l'indirizzo non ci sono: si rimedia
/// ricollegando. Un portachiavi che non risponde invece passa com'è
/// (`settings.secretUnavailable`): la password c'è ancora, e chiederla di
/// nuovo per un intoppo del sistema sarebbe chiedere quel che l'utente ha già
/// dato. Fino alla 3.2.0 di prova valeva «password mancante».
fn riapri_webdav(
    conto: &ContoSalvato,
    portachiavi: &dyn Portachiavi,
) -> Result<Arc<dyn Deposito>, AppError> {
    let manca = |cosa: &str| {
        AppError::new(ErrorCode::CloudAuthExpired {
            servizio: conto.servizio.nome().to_owned(),
        })
        .with_cause(format!("{cosa} del server non c'è"))
    };
    let password = portachiavi
        .leggi(&portachiavi::nuvola_password(&conto.conto))?
        .ok_or_else(|| manca("la password"))?;
    let dettagli: serde_json::Value = conto
        .dettagli
        .as_deref()
        .and_then(|testo| serde_json::from_str(testo).ok())
        .ok_or_else(|| manca("l'indirizzo"))?;
    let campo = |nome: &str| {
        dettagli
            .get(nome)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| manca("l'indirizzo"))
    };
    let server = WebDav::nuovo(&campo("indirizzo")?, &campo("utente")?, &password)?;
    Ok(Arc::new(server))
}

/// Il deposito di un servizio col consenso browser, sopra il suo permesso.
fn deposito_di(servizio: Servizio, accesso: Arc<Accesso>) -> Option<Arc<dyn Deposito>> {
    match servizio {
        Servizio::OneDrive => Some(Arc::new(OneDrive::nuovo(accesso))),
        Servizio::Dropbox => Some(Arc::new(Dropbox::nuovo(accesso))),
        Servizio::GoogleDrive => Some(Arc::new(GoogleDrive::nuovo(accesso))),
        _ => None,
    }
}

/// Fa `lavoro` coi collegamenti fermi: nessun consenso in corso, e nessuno
/// che ne cominci uno finché `lavoro` non ha finito.
///
/// Per `nuvola_app`: chi aspetta un consenso ha in mano le credenziali di
/// prima, e le userebbe fra tre minuti.
///
/// # Errori
///
/// `sync.busy` se un collegamento è in corso; quelli di `lavoro`.
pub(crate) fn con_i_collegamenti_fermi<T>(
    app: &AppHandle,
    lavoro: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let (_, nuvola) = stati(app)?;
    let _turno =
        Turno::prendi(&nuvola.collegando).ok_or_else(|| AppError::new(ErrorCode::SyncBusy))?;
    lavoro()
}

/// L'app di questi conti è cambiata, o se n'è andata: ciascuno si riapre
/// com'è adesso nel database.
///
/// Riaprire è la sola regola, e copre ogni caso:
///
/// - **identificativo cambiato** o **app tolta**: `riapri` risponde
///   `cloud.appChanged` o `cloud.clientMissing`. Il permesso si spegne (vedi
///   `Accesso::spegni`) e il deposito esce dalla mappa, così il filo si ferma
///   al giro dopo; il conto resta, «da ricollegare», con le sue voci, le sue
///   radici e i suoi brani. Il refresh token resta nel portachiavi;
/// - **identificativo rimesso com'era** (A → B → A): `riapri` riesce col
///   refresh token di sempre, e il conto torna com'era;
/// - **solo il segreto**: `riapri` riesce con le credenziali nuove, e il
///   deposito di prima lascia il posto a questo.
pub(crate) fn app_cambiata(app: &AppHandle, conti: &[String]) {
    let Ok((stato, nuvola)) = stati(app) else {
        return;
    };
    let Ok(salvati) = con_libreria(&stato, |l| specchio::conti(&l.connection)) else {
        return;
    };
    for conto in salvati.iter().filter(|c| conti.contains(&c.conto)) {
        if rimetti(&stato, &nuvola, conto) {
            avvia_filo(app, &conto.conto);
        }
    }
    emetti_conti(app);
}

/// Riapre un conto com'è adesso nel database e nel portachiavi, e il suo
/// deposito prende il posto di quello di prima. `true` se si è riaperto.
///
/// Se non si riapre, il deposito di prima si spegne ed esce dalla mappa, e il
/// conto resta col motivo: il filo si ferma al giro dopo.
fn rimetti(stato: &Stato, nuvola: &StatoMusicaNuvola, conto: &ContoSalvato) -> bool {
    match riapri(stato, conto) {
        Ok(deposito) => {
            nuvola
                .depositi
                .write()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(conto.conto.clone(), deposito);
            nuvola.segna_respinto(&conto.conto, false);
            nuvola.andamento(&conto.conto, |a| a.guasto = None);
            true
        }
        Err(guasto) => {
            let tolto = nuvola
                .depositi
                .write()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&conto.conto);
            if let Some(deposito) = tolto {
                deposito.spegni();
            }
            nuvola.andamento(&conto.conto, |a| a.guasto = Some(guasto));
            nuvola.sveglia(&conto.conto);
            false
        }
    }
}

// ── il filo di un conto ─────────────────────────────────────────────────────

/// Avvia il filo di un conto, se non ce n'è già uno.
fn avvia_filo(app: &AppHandle, conto: &str) {
    let Some(nuvola) = app.try_state::<StatoMusicaNuvola>() else {
        return;
    };
    let (sveglia, orecchio) = channel();
    {
        let mut sveglie = nuvola
            .sveglie
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if sveglie.contains_key(conto) {
            drop(sveglie);
            nuvola.sveglia(conto);
            return;
        }
        sveglie.insert(conto.to_owned(), sveglia);
    }
    let servizio = Servizio::del_conto(conto).map_or("?", Servizio::chiave);
    let mano = app.clone();
    let nome = conto.to_owned();
    let partito = std::thread::Builder::new()
        .name(format!("rymembra-nuvola-{servizio}"))
        .spawn(move || filo(&mano, &nome, &orecchio));
    if let Err(guasto) = partito {
        nota!("[nuvola] il filo di {conto} non parte: {guasto}");
        nuvola
            .sveglie
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(conto);
    }
}

/// Il filo: elenchi delle radici nuove, poi i cambiamenti, per sempre.
///
/// Esce quando l'applicazione si chiude o quando il conto non ha più un
/// deposito (scollegato).
fn filo(app: &AppHandle, conto: &str, orecchio: &Receiver<()>) {
    let mut guasti_di_fila = 0_u32;
    // Dopo il primo giro riuscito si chiede una scansione di tutte le radici
    // del conto: è ciò che riprende le letture dei tag rimaste a metà nella
    // sessione di prima (la passata B), che nessun cambiamento sul servizio
    // farebbe ripartire. Dopo il primo giro e non subito: a quel punto la
    // sorveglianza ha letto il suo elenco, e la richiesta non va persa.
    let mut ripreso = false;
    loop {
        if crate::spegnimento::in_uscita() {
            return;
        }
        let Ok((stato, nuvola)) = stati(app) else {
            return;
        };
        let Some(deposito) = nuvola.deposito(conto) else {
            nuvola
                .sveglie
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(conto);
            emetti_conti(app);
            return;
        };
        let giro = un_giro(app, &stato, &nuvola, conto, deposito.as_ref());
        // Mentre il giro andava, l'app del conto può essere cambiata e il
        // deposito uscito dalla mappa: il guasto di quel giro è figlio del
        // permesso spento, e non deve coprire il motivo vero.
        if nuvola.deposito(conto).is_none() {
            continue;
        }
        let attesa = match giro {
            Ok(()) => {
                guasti_di_fila = 0;
                if !ripreso {
                    ripreso = true;
                    let radici: Vec<String> = nuvola
                        .specchio
                        .read()
                        .unwrap_or_else(PoisonError::into_inner)
                        .radici(conto)
                        .into_iter()
                        .map(|radice| radice.percorso)
                        .collect();
                    crate::sorveglia::riguarda(app, radici);
                }
                nuvola.segna_respinto(conto, false);
                nuvola.andamento(conto, |a| {
                    a.guasto = None;
                    a.ultimo_ms = Some(adesso_ms());
                });
                // Chi sa aspettare i cambiamenti l'ha già fatto dentro il giro.
                if sa_aspettare(deposito.servizio()) {
                    Duration::ZERO
                } else {
                    OGNI
                }
            }
            Err(guasto) => {
                guasti_di_fila = guasti_di_fila.saturating_add(1);
                let respinto = respinto_per_sempre(&guasto);
                nuvola.segna_respinto(conto, respinto);
                if respinto {
                    // Le sue radici la scansione le salta finché dura: al
                    // primo giro riuscito si chiede di nuovo di riguardarle
                    // tutte, come dopo l'avvio.
                    ripreso = false;
                }
                let attesa = pausa_dopo(&guasto, guasti_di_fila);
                nota!(
                    "[nuvola] {conto}: {guasto} — si riprova fra {} s",
                    attesa.as_secs()
                );
                nuvola.andamento(conto, |a| {
                    a.elenco = false;
                    a.guasto = Some(guasto);
                });
                attesa
            }
        };
        emetti_conti(app);
        if aspetta(orecchio, attesa) {
            return;
        }
    }
}

/// Il servizio aspetta da sé i cambiamenti (long-poll)?
const fn sa_aspettare(servizio: Servizio) -> bool {
    matches!(
        servizio,
        Servizio::Dropbox | Servizio::Mega | Servizio::MegaLink
    )
}

/// Quanto aspettare dopo un guasto.
///
/// Un consenso che non vale più non passa riprovando: si aspetta che qualcuno
/// ricolleghi (la sveglia arriva col deposito nuovo), e intanto si riprova di
/// rado. Il resto raddoppia da [`PAUSA_GUASTO`] fino a [`PAUSA_MASSIMA`].
fn pausa_dopo(guasto: &AppError, di_fila: u32) -> Duration {
    if da_ricollegare(guasto) {
        return PAUSA_MASSIMA;
    }
    if let ErrorCode::NetRateLimited {
        retry_after_ms: Some(ms),
        ..
    } = guasto.code()
    {
        return Duration::from_millis(*ms).clamp(PAUSA_GUASTO, PAUSA_MASSIMA);
    }
    let volte = di_fila.saturating_sub(1).min(5);
    PAUSA_GUASTO
        .saturating_mul(2_u32.saturating_pow(volte))
        .min(PAUSA_MASSIMA)
}

/// Il servizio ha respinto per sempre il permesso del conto: il consenso
/// revocato o scaduto (`cloud.authExpired`), l'app sospesa o cancellata
/// (`cloud.appRejected`). Sono i due rifiuti che l'`Accesso` ricorda.
///
/// Finché vale, le radici del conto sono irraggiungibili come quelle di un
/// disco spento ([`ArchivioNuvola::segna_respinto`]): la scansione le salta e
/// non giudica nessuno dei loro brani, invece di provarli uno per uno e
/// contarli fra gli «illeggibili».
fn respinto_per_sempre(guasto: &AppError) -> bool {
    matches!(
        guasto.code().kind(),
        ErrorCodeKind::CloudAuthExpired | ErrorCodeKind::CloudAppRejected
    )
}

/// Un guasto che non passa riprovando: si rimedia ricollegando il conto, o
/// sistemando la sua app.
fn da_ricollegare(guasto: &AppError) -> bool {
    matches!(
        guasto.code().kind(),
        ErrorCodeKind::CloudAuthExpired
            | ErrorCodeKind::CloudClientMissing
            | ErrorCodeKind::CloudAppChanged
            | ErrorCodeKind::CloudAppRejected
            | ErrorCodeKind::CloudAppSecretMissing
    )
}

/// Aspetta `quanto`, o una sveglia. `true` se si deve uscire.
fn aspetta(orecchio: &Receiver<()>, quanto: Duration) -> bool {
    let fine = Instant::now() + quanto;
    loop {
        if crate::spegnimento::in_uscita() {
            return true;
        }
        let resta = fine.saturating_duration_since(Instant::now());
        if resta.is_zero() {
            return false;
        }
        // A fette di un secondo: l'uscita va vista presto anche senza sveglia.
        match orecchio.recv_timeout(resta.min(Duration::from_secs(1))) {
            Ok(()) => {
                // Più sveglie insieme valgono una.
                while orecchio.try_recv().is_ok() {}
                return false;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return true,
        }
    }
}

/// Il deposito di questo filo è ancora quello del conto: nessuno l'ha
/// scollegato, né sostituito con un ricollegamento, mentre il filo era in rete.
///
/// Si chiede **dentro** il lucchetto di quel che si sta per scrivere — lo
/// specchio o la libreria —, ed è ciò che rende la risposta buona fino alla
/// scrittura: lo scollegamento toglie il deposito dalla mappa prima di
/// prendere quei due lucchetti per svuotare il conto. Senza, il long-poll di
/// Dropbox — che non chiede l'accesso — tornava dopo lo scollegamento e
/// rimetteva al mondo il conto salvandone il cursore.
fn ancora_mio(nuvola: &StatoMusicaNuvola, conto: &str, deposito: &dyn Deposito) -> bool {
    nuvola.deposito(conto).is_some_and(|attuale| {
        std::ptr::addr_eq(Arc::as_ptr(&attuale), std::ptr::from_ref(deposito))
    })
}

/// Cambia lo specchio del conto, se il conto è ancora di questo filo.
fn nello_specchio<T>(
    nuvola: &StatoMusicaNuvola,
    conto: &str,
    deposito: &dyn Deposito,
    cambia: impl FnOnce(&mut Specchio) -> T,
) -> Option<T> {
    let mut specchio = nuvola
        .specchio
        .write()
        .unwrap_or_else(PoisonError::into_inner);
    ancora_mio(nuvola, conto, deposito).then(|| cambia(&mut specchio))
}

/// Scrive nel database quel che una chiamata dello specchio ha cambiato, se il
/// conto è ancora di questo filo: `Ok(false)` se non lo è, e niente si scrive.
///
/// # Una scrittura caduta
///
/// La memoria è già cambiata, e gli stessi cambiamenti riapplicati non
/// scriverebbero niente: una voce uguale non si riscrive, una tolta non si
/// ritoglie. Il cursore non avanza, ma il giro dopo non avrebbe niente da
/// salvare, e il cursore avanzerebbe allora — con le righe mai scritte. Al
/// riavvio i file nuovi mancherebbero, e la scansione toglierebbe i loro
/// brani. Per questo, su un errore, il conto si rilegge dal database: i
/// cambiamenti tornano a essere cambiamenti.
fn salva_toccate(
    stato: &Stato,
    nuvola: &StatoMusicaNuvola,
    conto: &str,
    deposito: &dyn Deposito,
    toccate: &specchio::Toccate,
) -> Result<bool, AppError> {
    let esito = con_libreria(stato, |l| {
        if !ancora_mio(nuvola, conto, deposito) {
            return Ok(false);
        }
        Specchio::salva(&l.connection, toccate).map(|()| true)
    });
    if esito.is_err() {
        rileggi_il_conto(stato, nuvola, conto);
    }
    esito
}

/// Rimette lo specchio di un conto com'è nel database. Se nemmeno la lettura
/// riesce, lo segna da rileggere all'inizio del giro dopo.
fn rileggi_il_conto(stato: &Stato, nuvola: &StatoMusicaNuvola, conto: &str) -> bool {
    match con_libreria(stato, |l| specchio::leggi_conto(&l.connection, conto)) {
        Ok(letto) => {
            nuvola
                .specchio
                .write()
                .unwrap_or_else(PoisonError::into_inner)
                .rimpiazza_conto(conto, letto);
            nuvola.andamento(conto, |a| a.da_rileggere = false);
            true
        }
        Err(guasto) => {
            nota!("[nuvola] {conto}: lo specchio non si rilegge dal database: {guasto}");
            nuvola.andamento(conto, |a| a.da_rileggere = true);
            false
        }
    }
}

/// Un giro: le radici non ancora complete, poi i cambiamenti.
///
/// Ogni scrittura passa da [`ancora_mio`]: un conto scollegato a metà giro
/// finisce il giro senza scrivere niente, e il filo si ferma.
fn un_giro(
    app: &AppHandle,
    stato: &Stato,
    nuvola: &StatoMusicaNuvola,
    conto: &str,
    deposito: &dyn Deposito,
) -> Result<(), AppError> {
    let da_rileggere = nuvola
        .andamenti
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(conto)
        .is_some_and(|a| a.da_rileggere);
    if da_rileggere && !rileggi_il_conto(stato, nuvola, conto) {
        return Err(AppError::new(ErrorCode::InternalAborted {
            what: Some("specchio del cloud".to_owned()),
        })
        .with_cause("lo specchio del conto non si rilegge dal database"));
    }
    // Il cursore **prima** di ogni elenco: quel che cambia mentre l'elenco
    // scorre arriva poi coi cambiamenti, al peggio due volte, mai zero.
    let mut salvato = conto_salvato(stato, conto)?;
    if salvato.cursore.is_none() {
        let primo = deposito.cursore_adesso()?;
        if !salva_cursore(stato, nuvola, conto, deposito, &primo, salvato.ultimo_ms)? {
            return Ok(());
        }
        salvato.cursore = Some(primo);
    }
    dimentica_le_radici_tolte(stato, nuvola, conto)?;
    let da_elencare: Vec<RadiceSalvata> = nuvola
        .specchio
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .radici(conto)
        .into_iter()
        .filter(|radice| !radice.completa)
        .collect();
    elenca_le_radici(app, stato, nuvola, conto, deposito, &da_elencare)?;

    let cursore = salvato.cursore.clone().unwrap_or_default();
    let attesa = if sa_aspettare(deposito.servizio()) {
        OGNI
    } else {
        Duration::ZERO
    };
    let (cambi, nuovo) = match deposito.cambiamenti(&cursore, attesa) {
        Ok(risposta) => risposta,
        Err(guasto) if cursore_scaduto(&guasto) => {
            // Il servizio non accetta più il cursore, o è l'ora del ripasso
            // (WebDAV): ogni radice si rielenca per intero e si chiude — è il
            // solo modo in cui il confronto può togliere quel che è sparito
            // nel frattempo. Mai svuotare prima: un elenco che cade a metà non
            // deve lasciare la libreria a metà.
            //
            // E l'ordine conta. Le radici si segnano incomplete e si salvano
            // **prima** del cursore nuovo: un elenco interrotto — la rete, un
            // limite di frequenza, l'uscita — resta da finire, e il giro dopo
            // lo riprende. Fino alla 3.2.0 di prova il cursore nuovo si
            // salvava per primo, le radici restavano complete, e quel che era
            // cambiato nel buco non arrivava più.
            nota!(
                "[nuvola] {conto}: si rielenca tutto ({})",
                guasto.cause().unwrap_or("cursore scaduto")
            );
            let fresco = deposito.cursore_adesso()?;
            let Some(riaperte) =
                nello_specchio(nuvola, conto, deposito, |s| s.riapri_radici(conto))
            else {
                return Ok(());
            };
            let scritto = con_libreria(stato, |l| {
                if !ancora_mio(nuvola, conto, deposito) {
                    return Ok(false);
                }
                for radice in &riaperte {
                    specchio::salva_radice(&l.connection, radice)?;
                }
                specchio::salva_cursore(&l.connection, conto, Some(&fresco), salvato.ultimo_ms)
            })?;
            if scritto {
                elenca_le_radici(app, stato, nuvola, conto, deposito, &riaperte)?;
                segui_le_radici_traslocate(app, stato, nuvola, conto, deposito);
            }
            return Ok(());
        }
        Err(guasto) => return Err(guasto),
    };
    let toccate = if cambi.is_empty() {
        specchio::Toccate::default()
    } else {
        let Some(toccate) = nello_specchio(nuvola, conto, deposito, |s| s.applica(conto, cambi))
        else {
            return Ok(());
        };
        toccate
    };
    if !salva_toccate(stato, nuvola, conto, deposito, &toccate)?
        || !salva_cursore(stato, nuvola, conto, deposito, &nuovo, Some(adesso_ms()))?
    {
        return Ok(());
    }
    if !toccate.cartelle.is_empty() {
        crate::sorveglia::riguarda(app, toccate.cartelle.into_iter().collect());
    }
    segui_le_radici_traslocate(app, stato, nuvola, conto, deposito);
    Ok(())
}

/// Salva il cursore di un conto che c'è ancora: `false` se è stato scollegato
/// (vedi `specchio::salva_cursore`).
fn salva_cursore(
    stato: &Stato,
    nuvola: &StatoMusicaNuvola,
    conto: &str,
    deposito: &dyn Deposito,
    cursore: &str,
    ultimo_ms: Option<i64>,
) -> Result<bool, AppError> {
    con_libreria(stato, |l| {
        if !ancora_mio(nuvola, conto, deposito) {
            return Ok(false);
        }
        specchio::salva_cursore(&l.connection, conto, Some(cursore), ultimo_ms)
    })
}

/// Elenca delle radici, una dopo l'altra.
///
/// # Una radice che non c'è più
///
/// La cartella scelta tolta sul servizio — o rinominata dove l'identificativo
/// è il percorso, Dropbox e WebDAV — risponde «non c'è». Non è un guasto del
/// conto: la radice resta incompleta, e quindi non raggiungibile (vedi
/// `Specchio::raggiungibile`), e le altre radici e i cambiamenti vanno avanti.
/// Prima un elenco caduto fermava il giro, e un conto con una radice persa non
/// sentiva più niente.
fn elenca_le_radici(
    app: &AppHandle,
    stato: &Stato,
    nuvola: &StatoMusicaNuvola,
    conto: &str,
    deposito: &dyn Deposito,
    radici: &[RadiceSalvata],
) -> Result<(), AppError> {
    for radice in radici {
        match elenca_radice(app, stato, nuvola, conto, deposito, radice) {
            Ok(()) => nuvola.andamento(conto, |a| {
                a.perse.remove(&radice.percorso);
            }),
            Err(guasto) if guasto.code().kind() == ErrorCodeKind::CloudNotFound => {
                let mut nuova = false;
                nuvola.andamento(conto, |a| nuova = a.perse.insert(radice.percorso.clone()));
                if nuova {
                    nota!(
                        "[nuvola] {conto}: la cartella {} non c'è più sul servizio, resta da scegliere di nuovo",
                        radice.percorso
                    );
                }
            }
            Err(guasto) => return Err(guasto),
        }
    }
    Ok(())
}

/// Le radici che l'utente ha tolto dalla libreria escono anche dallo
/// specchio delle radici: non si elencano più, e una radice ripresa domani
/// riparte da un elenco completo. Le voci restano (si tolgono solo su prova,
/// e possono stare sotto un'altra radice).
///
/// Prima le radici, poi le cartelle sorvegliate: l'ordine inverso di
/// `aggiungi`, che scrive prima le cartelle e poi la radice. Letti nello
/// stesso ordine, una radice nata fra le due letture sembrava tolta, e si
/// dimenticava appena aggiunta.
fn dimentica_le_radici_tolte(
    stato: &Stato,
    nuvola: &StatoMusicaNuvola,
    conto: &str,
) -> Result<(), AppError> {
    let regole = rymembra_domain::paths::PathRules::for_current_platform();
    let radici = nuvola
        .specchio
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .radici(conto);
    let sorvegliate: HashSet<String> = crate::comandi::cartelle_sorvegliate(stato)?
        .iter()
        .map(|r| rymembra_domain::paths::path_key(r, regole))
        .collect();
    let tolte: Vec<String> = radici
        .into_iter()
        .map(|radice| radice.percorso)
        .filter(|percorso| {
            !sorvegliate.contains(&rymembra_domain::paths::path_key(percorso, regole))
        })
        .collect();
    for percorso in tolte {
        let _ = nuvola
            .specchio
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .dimentica_radice(&percorso);
        con_libreria(stato, |l| specchio::togli_radice(&l.connection, &percorso))?;
    }
    Ok(())
}

/// L'elenco completo di una radice, pagina per pagina.
///
/// Ogni pagina va nello specchio e nel database subito; la prima scansione
/// parte con la prima pagina, le altre al più ogni [`RITMO_SCANSIONI`]. In
/// fondo, `chiudi_elenco` toglie quel che sotto la radice non è arrivato — e
/// solo in fondo: un elenco interrotto non toglie niente.
fn elenca_radice(
    app: &AppHandle,
    stato: &Stato,
    nuvola: &StatoMusicaNuvola,
    conto: &str,
    deposito: &dyn Deposito,
    radice: &RadiceSalvata,
) -> Result<(), AppError> {
    nuvola.andamento(conto, |a| {
        a.elenco = true;
        a.voci = 0;
    });
    emetti_conti(app);
    let mut elencati: HashSet<String> = HashSet::new();
    let mut ultima_scansione: Option<Instant> = None;
    let mut guasto_di_scrittura: Option<AppError> = None;
    let completo = deposito.elenca(&radice.cima, &mut |pagina| {
        if crate::spegnimento::in_uscita() {
            return ControlFlow::Break(());
        }
        elencati.extend(pagina.iter().map(|voce| voce.id.clone()));
        let quante = u64::try_from(pagina.len()).unwrap_or(u64::MAX);
        let Some(toccate) = nello_specchio(nuvola, conto, deposito, |s| s.metti(conto, pagina))
        else {
            return ControlFlow::Break(());
        };
        match salva_toccate(stato, nuvola, conto, deposito, &toccate) {
            Ok(true) => {}
            Ok(false) => return ControlFlow::Break(()),
            Err(guasto) => {
                guasto_di_scrittura = Some(guasto);
                return ControlFlow::Break(());
            }
        }
        nuvola.andamento(conto, |a| a.voci = a.voci.saturating_add(quante));
        if ultima_scansione.is_none_or(|quando| quando.elapsed() >= RITMO_SCANSIONI) {
            ultima_scansione = Some(Instant::now());
            crate::sorveglia::riguarda(app, vec![radice.percorso.clone()]);
            emetti_conti(app);
        }
        ControlFlow::Continue(())
    });
    nuvola.andamento(conto, |a| a.elenco = false);
    if let Some(guasto) = guasto_di_scrittura {
        return Err(guasto);
    }
    if completo? {
        let Some((toccate, complete)) = nello_specchio(nuvola, conto, deposito, |s| {
            let toccate = s.chiudi_elenco(conto, &radice.cima, &elencati);
            let complete: Vec<RadiceSalvata> = s
                .radici(conto)
                .into_iter()
                .filter(|r| r.cima == radice.cima && r.completa)
                .collect();
            (toccate, complete)
        }) else {
            return Ok(());
        };
        let scritto = salva_toccate(stato, nuvola, conto, deposito, &toccate)?
            && con_libreria(stato, |l| {
                if !ancora_mio(nuvola, conto, deposito) {
                    return Ok(false);
                }
                complete
                    .iter()
                    .try_for_each(|r| specchio::salva_radice(&l.connection, r))
                    .map(|()| true)
            })?;
        if !scritto {
            return Ok(());
        }
    }
    // La scansione dopo l'ultima pagina c'è sempre: le pagine arrivate dopo
    // l'ultima richiesta non devono aspettare il prossimo cambiamento.
    crate::sorveglia::riguarda(app, vec![radice.percorso.clone()]);
    Ok(())
}

/// Quanto il trasloco di una cartella del cloud aspetta il turno delle
/// scansioni: poco. Gira sul filo del conto, e se una scansione lo tiene si
/// riprova al giro dopo — la radice intanto non si raggiunge, e niente si
/// toglie.
const ATTESA_DEL_TRASLOCO: Duration = Duration::from_secs(2);

/// Le cartelle scelte che l'utente ha rinominato o spostato sul servizio le
/// segue la libreria: le righe dei brani, le cartelle sorvegliate e la radice
/// dello specchio prendono il percorso nuovo, e ascolti, voti e playlist
/// restano dove sono.
///
/// Solo OneDrive e Google Drive: vedi `Specchio::radici_traslocate`. Su
/// Dropbox e WebDAV un rinomina toglie la cima, e la radice resta solo non
/// raggiungibile finché l'utente non la sceglie di nuovo.
fn segui_le_radici_traslocate(
    app: &AppHandle,
    stato: &Stato,
    nuvola: &StatoMusicaNuvola,
    conto: &str,
    deposito: &dyn Deposito,
) {
    let traslocate = nuvola
        .specchio
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .radici_traslocate(conto);
    let mut nuove = Vec::new();
    for (radice, nuovo) in traslocate {
        match trasloca_la_radice(stato, nuvola, conto, deposito, &radice, &nuovo) {
            Ok(Some(seguiti)) => {
                nota!(
                    "[nuvola] {conto}: la cartella {} ora è {nuovo}, {seguiti} brani seguiti",
                    radice.percorso
                );
                nuove.push(nuovo);
            }
            Ok(None) => return,
            Err(guasto) if guasto.code().kind() == ErrorCodeKind::LibraryScanBusy => {}
            Err(guasto) => nota!(
                "[nuvola] {conto}: il trasloco di {} non è riuscito, si riprova al giro dopo: {guasto}",
                radice.percorso
            ),
        }
    }
    if !nuove.is_empty() {
        crate::cartelle::cambiate(app);
        crate::sorveglia::riconfigura(app);
        crate::sorveglia::riguarda(app, nuove);
        emetti_conti(app);
    }
}

/// Trasloca una radice al suo percorso di adesso: `Ok(None)` se il conto non è
/// più di questo filo, altrimenti quanti brani hanno preso il percorso nuovo.
///
/// È la strada di «Trasloca» delle cartelle del disco (`comandi::trasloca_ora`),
/// col turno di manutenzione e in una transazione sola, con una differenza:
/// che un file ci sia sotto il nome nuovo lo dice lo specchio, non il disco.
/// Un brano che lì non c'è tiene la sua riga com'era.
fn trasloca_la_radice(
    stato: &Stato,
    nuvola: &StatoMusicaNuvola,
    conto: &str,
    deposito: &dyn Deposito,
    radice: &RadiceSalvata,
    nuovo: &str,
) -> Result<Option<usize>, AppError> {
    use rymembra_app::profilo::radici;
    use rymembra_app::volumi::Esistenza;

    let _turno = crate::comandi::aspetta_il_turno_di_manutenzione(
        stato,
        ATTESA_DEL_TRASLOCO,
        "il trasloco di una cartella del cloud aspetta la scansione in corso",
    )?;
    let rimappature = [rymembra_app::profilo::Rimappatura {
        da: radice.percorso.clone(),
        a: nuovo.to_owned(),
        brani: 0,
    }];
    let candidati = con_libreria(stato, |l| radici::candidati(&l.connection, &rimappature))?;
    let presenti: HashSet<String> = {
        let specchio = nuvola
            .specchio
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        candidati
            .into_iter()
            .map(|candidato| candidato.percorso)
            .filter(|percorso| specchio.voce(percorso).is_some())
            .collect()
    };
    let spostata = RadiceSalvata {
        percorso: nuovo.to_owned(),
        ..radice.clone()
    };
    let fatto = con_libreria(stato, |l| {
        if !ancora_mio(nuvola, conto, deposito) {
            return Ok(None);
        }
        let fallita = |err: rusqlite::Error| {
            rymembra_app::db::codice_da_sqlite("trasloco di una cartella del cloud", &err)
        };
        let tx = l
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(fallita)?;
        let fatto = radici::applica_verificati(&tx, &rimappature, &|percorso| {
            if presenti.contains(percorso) {
                Esistenza::Presente
            } else {
                Esistenza::Assente
            }
        })?;
        crate::comandi::riscrivi_le_cartelle(&tx, &rimappature)?;
        specchio::togli_radice(&tx, &radice.percorso)?;
        specchio::salva_radice(&tx, &spostata)?;
        tx.commit().map_err(fallita)?;
        Ok(Some(fatto.percorsi))
    })?;
    if fatto.is_some() {
        let mut specchio = nuvola
            .specchio
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        specchio.dimentica_radice(&radice.percorso);
        specchio.aggiungi_radice(spostata);
    }
    Ok(fatto)
}

fn conto_salvato(stato: &Stato, conto: &str) -> Result<ContoSalvato, AppError> {
    con_libreria(stato, |l| specchio::conti(&l.connection))?
        .into_iter()
        .find(|c| c.conto == conto)
        .ok_or_else(|| {
            AppError::new(ErrorCode::CloudNotConnected {
                servizio: nome_del_conto(conto),
            })
        })
}

fn salva_conto(stato: &Stato, conto: &ContoSalvato) -> Result<(), AppError> {
    con_libreria(stato, |l| specchio::salva_conto(&l.connection, conto))
}

fn nome_del_conto(conto: &str) -> String {
    Servizio::del_conto(conto).map_or_else(String::new, |s| s.nome().to_owned())
}

// ── quel che la finestra vede ───────────────────────────────────────────────

/// Un servizio come lo offre questa build.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServizioOfferto {
    servizio: &'static str,
    nome: &'static str,
    disponibile: bool,
    modo: &'static str,
    pronto: bool,
}

/// Una cartella del cloud scelta.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RadiceNuvola {
    percorso: String,
    nome: String,
    /// Il conto è collegato e non sta elencando, ma la cartella non si
    /// raggiunge: tolta o rinominata sul servizio (vedi
    /// `Specchio::raggiungibile`). Si sceglie di nuovo.
    persa: bool,
}

/// Un conto, per la finestra.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContoNuvola {
    conto: String,
    servizio: &'static str,
    /// L'app con cui è collegato (`None` per WebDAV, o se è stata tolta): per
    /// ricollegarlo con la stessa.
    app: Option<String>,
    nome: String,
    stato: &'static str,
    errore: Option<ErroreIpc>,
    radici: Vec<RadiceNuvola>,
    voci_elencate: Option<u64>,
    ultimo_ms: Option<i64>,
}

/// Una cartella remota, nel selettore.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CartellaRemota {
    id: String,
    nome: String,
    /// Una cartella che il servizio non ha (vedi
    /// [`rymembra_domain::nuvola::e_virtuale`]): ci si entra, ma non si sceglie.
    virtuale: bool,
}

/// La cache dei brani.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoCacheNuvola {
    attiva: bool,
    tetto_mb: u64,
    usati_byte: u64,
}

/// Quel che si scrive per MEGA e WebDAV.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    dead_code,
    reason = "il codice a sei cifre e il link li legge MEGA, che arriva per ultimo: il contratto con la finestra c'è già, così la finestra non cambia"
)]
pub struct ModuloCollega {
    indirizzo: Option<String>,
    utente: Option<String>,
    password: Option<String>,
    codice: Option<String>,
    link: Option<String>,
}

/// Il nome di una radice da mostrare: «Musica › Rock». La cima del drive non
/// ha segmenti, e si mostra col nome del servizio.
fn nome_radice(percorso: &str) -> String {
    let nomi = segmenti(percorso);
    if nomi.is_empty() {
        return conto_del_percorso(percorso).map_or_else(String::new, nome_del_conto);
    }
    nomi.join(" › ")
}

/// Lo stato di un conto per la finestra: `"daRicollegare"`, `"elenco"`,
/// `"errore"`, `"collegato"`.
///
/// # Il portachiavi che non risponde
///
/// Un conto senza deposito si ricollega — tranne quando il deposito manca
/// perché all'avvio il portachiavi non ha risposto. Il refresh token è ancora
/// lì: è un intoppo del sistema, e il rimedio è «Aggiorna», che riprova a
/// riaprirlo (vedi [`nuvola_aggiorna`]), non un consenso da rifare nel
/// browser.
fn stato_del_conto(collegato: bool, andamento: &Andamento) -> &'static str {
    let intoppo = andamento
        .guasto
        .as_ref()
        .is_some_and(|g| g.code().kind() == ErrorCodeKind::SettingsSecretUnavailable);
    let da_ricollegare = if collegato {
        andamento.guasto.as_ref().is_some_and(da_ricollegare)
    } else {
        !intoppo
    };
    if da_ricollegare {
        "daRicollegare"
    } else if andamento.elenco {
        "elenco"
    } else if andamento.guasto.is_some() {
        "errore"
    } else {
        "collegato"
    }
}

/// I conti, per la finestra.
fn conti_ipc(stato: &Stato, nuvola: &StatoMusicaNuvola) -> Result<Vec<ContoNuvola>, AppError> {
    let salvati = con_libreria(stato, |l| specchio::conti(&l.connection))?;
    let radici_della_libreria = crate::comandi::cartelle_sorvegliate(stato)?;
    let andamenti = nuvola
        .andamenti
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    Ok(salvati
        .into_iter()
        .map(|salvato| {
            let andamento = andamenti.get(&salvato.conto).cloned().unwrap_or_default();
            let collegato = nuvola.deposito(&salvato.conto).is_some();
            let stato_conto = stato_del_conto(collegato, &andamento);
            let radici = radici_della_libreria
                .iter()
                .filter(|r| conto_del_percorso(r) == Some(salvato.conto.as_str()))
                .map(|r| RadiceNuvola {
                    percorso: r.clone(),
                    nome: nome_radice(r),
                    // Mentre elenca no: una radice ripresa dopo un
                    // ricollegamento ha la cima in arrivo con la prima pagina.
                    persa: collegato
                        && !andamento.elenco
                        && !nuvola
                            .specchio
                            .read()
                            .unwrap_or_else(PoisonError::into_inner)
                            .raggiungibile(r),
                })
                .collect();
            ContoNuvola {
                servizio: salvato.servizio.chiave(),
                app: salvato.app.clone(),
                nome: salvato.nome.clone(),
                stato: stato_conto,
                errore: andamento.guasto.clone().map(|g| *errore(g)),
                radici,
                voci_elencate: andamento.elenco.then_some(andamento.voci),
                ultimo_ms: andamento.ultimo_ms.or(salvato.ultimo_ms),
                conto: salvato.conto,
            }
        })
        .collect())
}

/// Manda alla finestra lo stato di tutti i conti.
fn emetti_conti(app: &AppHandle) {
    let Ok((stato, nuvola)) = stati(app) else {
        return;
    };
    match conti_ipc(&stato, &nuvola) {
        Ok(conti) => app.emetti("nuvola:conti", conti),
        Err(guasto) => nota!("[nuvola] lo stato dei conti non si legge: {guasto}"),
    }
}

fn stato_cache(nuvola: &StatoMusicaNuvola) -> StatoCacheNuvola {
    let (usati, tetto, attiva) = nuvola
        .archivio
        .as_ref()
        .map_or((0, TETTO_DI_SERIE, false), |a| a.cache().stato());
    StatoCacheNuvola {
        attiva,
        tetto_mb: tetto >> 20,
        usati_byte: usati,
    }
}

// ── i comandi ───────────────────────────────────────────────────────────────

/// I servizi che Rymembra offre, e se c'è già un'app per collegarli.
///
/// Solo il database: se all'app manca il segreto lo dice `nuvola_app`, e il
/// collegamento con `cloud.appSecretMissing`.
#[tauri::command]
pub async fn nuvola_servizi(app: AppHandle) -> Esito<Vec<ServizioOfferto>> {
    in_disparte("servizi del cloud", move || {
        let (stato, _) = stati(&app)?;
        let con_app = con_libreria(&stato, |l| nuvola_app::elenco(&l.connection))?;
        let ha_un_app = |servizio: Servizio| {
            servizio
                .piattaforma()
                .is_some_and(|p| con_app.iter().any(|app| app.piattaforma == p))
        };
        let (microsoft, google, dropbox) = (
            ha_un_app(Servizio::OneDrive),
            ha_un_app(Servizio::GoogleDrive),
            ha_un_app(Servizio::Dropbox),
        );
        let offerto = |servizio: Servizio, disponibile: bool, modo: &'static str, pronto: bool| {
            ServizioOfferto {
                servizio: servizio.chiave(),
                nome: servizio.nome(),
                disponibile,
                modo,
                pronto,
            }
        };
        Ok(vec![
            offerto(Servizio::OneDrive, microsoft, "browser", true),
            offerto(Servizio::GoogleDrive, google, "browser", true),
            offerto(Servizio::Dropbox, dropbox, "browser", true),
            offerto(Servizio::WebDav, true, "modulo", true),
            offerto(Servizio::Mega, false, "modulo", false),
        ])
    })
    .await
    .and_then(|esito| esito)
    .map_err(errore)
}

/// I conti collegati (o da ricollegare).
#[tauri::command]
pub async fn nuvola_conti(app: AppHandle) -> Esito<Vec<ContoNuvola>> {
    in_disparte("conti del cloud", move || {
        let (stato, nuvola) = stati(&app)?;
        conti_ipc(&stato, &nuvola)
    })
    .await
    .and_then(|esito| esito)
    .map_err(errore)
}

/// Collega un conto: il consenso nel browser per OneDrive, Drive e Dropbox,
/// con l'app `app_id` (o la predefinita della piattaforma); il modulo per MEGA
/// e WebDAV.
///
/// Può durare fino a tre minuti (l'attesa del consenso). Un collegamento
/// annullato con [`nuvola_conto_annulla`] non è un guasto: torna
/// `internal.aborted`, che la finestra ignora.
#[tauri::command]
pub async fn nuvola_conto_collega(
    app: AppHandle,
    servizio: String,
    app_id: Option<String>,
    modulo: Option<ModuloCollega>,
) -> Esito<ContoNuvola> {
    in_disparte("collegamento di un conto", move || {
        collega(&app, &servizio, app_id.as_deref(), modulo)
    })
    .await
    .and_then(|esito| esito)
    .map_err(errore)
}

fn collega(
    app: &AppHandle,
    servizio: &str,
    app_id: Option<&str>,
    modulo: Option<ModuloCollega>,
) -> Result<ContoNuvola, AppError> {
    let (stato, nuvola) = stati(app)?;
    let servizio = Servizio::da_chiave(servizio).ok_or_else(|| {
        AppError::new(ErrorCode::IpcPayloadInvalid {
            channel: "nuvola_conto_collega".to_owned(),
            detail: Some(format!("«{servizio}» non è un servizio")),
        })
    })?;
    // Un turno che si lascia da sé, anche cadendo: un `swap` fatto a mano
    // restava preso dopo un panico, e ogni «Collega» e ogni app salvata
    // rispondevano «occupato» fino al riavvio.
    let turno =
        Turno::prendi(&nuvola.collegando).ok_or_else(|| AppError::new(ErrorCode::SyncBusy))?;
    let esito = (|| -> Result<String, AppError> {
        nuvola.annulla.store(false, Ordering::Release);
        let collegato = match servizio {
            Servizio::OneDrive | Servizio::Dropbox | Servizio::GoogleDrive => {
                collega_col_browser(&stato, &nuvola, servizio, app_id)?
            }
            Servizio::WebDav => collega_webdav(modulo.unwrap_or_default())?,
            Servizio::Mega | Servizio::MegaLink => {
                return Err(AppError::new(ErrorCode::CloudClientMissing {
                    servizio: servizio.nome().to_owned(),
                })
                .with_cause("servizio non ancora disponibile in questa build"));
            }
        };
        let conto = collegato.conto;
        // Un conto che c'era già (ricollegato) tiene il suo cursore: quel che
        // è cambiato nel frattempo arriva coi cambiamenti.
        let cursore = conto_salvato(&stato, &conto).ok().and_then(|c| c.cursore);
        let (app_del_conto, client) = collegato.legame.unzip();
        salva_conto(
            &stato,
            &ContoSalvato {
                conto: conto.clone(),
                servizio,
                nome: collegato.nome,
                dettagli: collegato.dettagli,
                cursore,
                ultimo_ms: Some(adesso_ms()),
                app: app_del_conto.clone(),
                client: client.clone(),
            },
        )?;
        // Un conto che c'era già tiene il legame di prima (vedi
        // `specchio::lega_conto`): il consenso appena dato è di quest'app.
        con_libreria(&stato, |l| {
            specchio::lega_conto(
                &l.connection,
                &conto,
                app_del_conto.as_deref(),
                client.as_deref(),
            )
        })?;
        nuvola
            .depositi
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(conto.clone(), collegato.deposito);
        nuvola.segna_respinto(&conto, false);
        nuvola.andamento(&conto, |a| {
            a.guasto = None;
            a.ultimo_ms = Some(adesso_ms());
        });
        Ok(conto)
    })();
    drop(turno);
    let conto = esito?;
    avvia_filo(app, &conto);
    emetti_conti(app);
    conti_ipc(&stato, &nuvola)?
        .into_iter()
        .find(|c| c.conto == conto)
        .ok_or_else(|| AppError::new(ErrorCode::CloudNotFound))
}

/// Un conto appena collegato, prima di salvarlo.
struct Collegato {
    conto: String,
    nome: String,
    /// Quel che il servizio vuole ricordato e non è segreto (per WebDAV
    /// l'indirizzo e l'utente), in JSON.
    dettagli: Option<String>,
    /// L'app del consenso e il suo identificativo di adesso; `None` per WebDAV.
    legame: Option<(String, String)>,
    deposito: Arc<dyn Deposito>,
}

/// Il consenso nel browser di sistema (OneDrive, Google Drive, Dropbox), con
/// l'app chiesta o con la predefinita della piattaforma.
///
/// # Errori
///
/// `cloud.clientMissing` se la piattaforma non ha un'app (o quella chiesta non
/// c'è, o è di un'altra piattaforma); quelli del consenso.
fn collega_col_browser(
    stato: &Stato,
    nuvola: &StatoMusicaNuvola,
    servizio: Servizio,
    app_id: Option<&str>,
) -> Result<Collegato, AppError> {
    let manca = || {
        AppError::new(ErrorCode::CloudClientMissing {
            servizio: servizio.nome().to_owned(),
        })
    };
    let piattaforma = servizio.piattaforma().ok_or_else(manca)?;
    let fornitore = fornitore_della_musica(servizio).ok_or_else(manca)?;
    let scelta = con_libreria(stato, |l| match app_id {
        Some(id) => nuvola_app::una(&l.connection, id),
        None => nuvola_app::predefinita(&l.connection, piattaforma),
    })?
    .filter(|app| app.piattaforma == piattaforma)
    .ok_or_else(manca)?;
    let credenziali = crate::nuvola_app::credenziali_dell_app(&scelta)?;
    let rete = Rete::nuova(servizio.chiave(), oauth::SCADENZA);
    let fermare = || nuvola.annulla.load(Ordering::Acquire) || crate::spegnimento::in_uscita();
    let token = oauth::collega_presso(
        &rete,
        &fornitore,
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
    )?;
    // Prima di toccare il deposito di prima: un consenso senza refresh token
    // funzionerebbe un'ora, e un conto che andava non deve spegnersi per lui.
    if token.refresh_token.is_none() {
        return Err(AppError::new(ErrorCode::CloudAuthExpired {
            servizio: servizio.nome().to_owned(),
        })
        .with_cause("il servizio non ha mandato un refresh token"));
    }
    let accesso = Arc::new(Accesso::appena_collegato(
        rete,
        fornitore,
        credenziali,
        token,
        Arc::new(DiSistema),
    ));
    let deposito = deposito_di(servizio, Arc::clone(&accesso)).ok_or_else(|| {
        AppError::new(ErrorCode::CloudClientMissing {
            servizio: servizio.nome().to_owned(),
        })
    })?;
    let chi = deposito.chi()?;
    let conto = conto_di(servizio, &chi.id_stabile);
    // Un conto che si ricollega: il permesso di prima si spegne **prima** che
    // il nuovo refresh token vada nel portachiavi. Un suo rinfresco in volo,
    // finito dopo, ci riscriverebbe sopra quello del consenso di prima.
    let di_prima = nuvola.deposito(&conto);
    if let Some(di_prima) = &di_prima {
        di_prima.spegni();
    }
    if let Err(guasto) = accesso.battezza(&conto) {
        // Il portachiavi non ha preso il token nuovo: il conto di prima, col
        // token di prima, torna com'era invece di restare spento fino al
        // riavvio.
        if di_prima.is_some()
            && let Ok(salvato) = conto_salvato(stato, &conto)
        {
            rimetti(stato, nuvola, &salvato);
        }
        return Err(guasto);
    }
    Ok(Collegato {
        conto,
        nome: chi.nome,
        dettagli: None,
        legame: Some((scelta.app, scelta.client_id)),
        deposito,
    })
}

/// Il modulo di WebDAV: indirizzo, utente, password per app. La password va
/// nel portachiavi e solo lì; indirizzo e utente nei dettagli del conto.
fn collega_webdav(modulo: ModuloCollega) -> Result<Collegato, AppError> {
    let indirizzo = modulo.indirizzo.unwrap_or_default();
    let utente = modulo.utente.unwrap_or_default();
    let password = modulo.password.unwrap_or_default();
    if indirizzo.trim().is_empty() || utente.trim().is_empty() || password.is_empty() {
        return Err(AppError::new(ErrorCode::CloudWrongCredentials));
    }
    let server = WebDav::nuovo(&indirizzo, utente.trim(), &password)?;
    let chi = server.chi()?;
    let conto = conto_di(Servizio::WebDav, &chi.id_stabile);
    DiSistema.scrivi(&portachiavi::nuvola_password(&conto), &password)?;
    Ok(Collegato {
        conto,
        nome: chi.nome,
        dettagli: Some(
            serde_json::json!({ "indirizzo": indirizzo.trim(), "utente": utente.trim() })
                .to_string(),
        ),
        legame: None,
        deposito: Arc::new(server),
    })
}

/// Ferma l'attesa del consenso. Torna subito.
#[tauri::command]
pub fn nuvola_conto_annulla(nuvola: State<'_, StatoMusicaNuvola>) -> Esito<()> {
    nuvola.annulla.store(true, Ordering::Release);
    Ok(())
}

/// Scollega un conto: segreti, specchio, cache. Le sue cartelle restano
/// attenuate («da ricollegare»), a meno che `togli_cartelle`.
///
/// La cache perde i brani del conto (`Cache::togli_conto`), come promette
/// `PRIVACY.md`, e la scheda in Impostazioni lo vede con `nuvola:cache`: lo
/// stato si legge qui in disparte, perché il lucchetto della cache lo tiene
/// anche chi ricompone una copia intera.
#[tauri::command]
pub async fn nuvola_conto_scollega(
    app: AppHandle,
    conto: String,
    togli_cartelle: bool,
) -> Esito<()> {
    let mano = app.clone();
    let cache = in_disparte("scollegamento di un conto", move || {
        let (stato, nuvola) = stati(&mano)?;
        let tolto = nuvola
            .depositi
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&conto);
        // Spento prima di cancellare: un brano che sta suonando da questo
        // conto non deve rimettere nel portachiavi il refresh token che si
        // sta togliendo (vedi `Accesso::spegni`).
        if let Some(deposito) = tolto {
            deposito.spegni();
        }
        nuvola.segna_respinto(&conto, false);
        nuvola.sveglia(&conto);
        DiSistema.cancella(&portachiavi::nuvola_refresh(&conto))?;
        DiSistema.cancella(&portachiavi::nuvola_password(&conto))?;
        // Le cartelle tenute tengono anche la loro radice, incompleta: al
        // ricollegamento il filo la rielenca. Vedi `Specchio::dimentica`.
        let tieni_radici = !togli_cartelle;
        let _ = nuvola
            .specchio
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .dimentica(&conto, tieni_radici);
        // I suoi brani in cache se ne vanno con lui, anche quello che sta
        // suonando: vedi `Cache::togli_conto`.
        if let Some(archivio) = &nuvola.archivio {
            archivio.cache().togli_conto(&conto);
        }
        con_libreria(&stato, |l| {
            specchio::togli_conto(&l.connection, &conto, tieni_radici)
        })?;
        nuvola
            .andamenti
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&conto);
        if togli_cartelle {
            let restano: Vec<String> = crate::comandi::cartelle_sorvegliate(&stato)?
                .into_iter()
                .filter(|r| conto_del_percorso(r) != Some(conto.as_str()))
                .collect();
            crate::comandi::scrivi_cartelle(&stato, &restano)?;
        }
        Ok::<_, AppError>(stato_cache(&nuvola))
    })
    .await
    .and_then(|esito| esito)
    .map_err(errore)?;
    app.emetti("nuvola:cache", cache);
    crate::sorveglia::riconfigura(&app);
    emetti_conti(&app);
    Ok(())
}

/// Le sottocartelle di una cartella remota (`None` = la cima del drive).
#[tauri::command]
pub async fn nuvola_cartelle(
    app: AppHandle,
    conto: String,
    dentro: Option<String>,
) -> Esito<Vec<CartellaRemota>> {
    in_disparte("cartelle remote", move || {
        let (_, nuvola) = stati(&app)?;
        let deposito = nuvola.deposito(&conto).ok_or_else(|| {
            AppError::new(ErrorCode::CloudNotConnected {
                servizio: nome_del_conto(&conto),
            })
        })?;
        let mut cartelle: Vec<CartellaRemota> = deposito
            .cartelle(dentro.as_deref())?
            .into_iter()
            .filter(|voce| voce.cartella)
            .map(|voce| CartellaRemota {
                virtuale: e_virtuale(&voce.id),
                id: voce.id,
                nome: voce.nome,
            })
            .collect();
        // Le virtuali in testa: non sono cartelle dell'utente fra le altre.
        cartelle.sort_by_cached_key(|c| (!c.virtuale, c.nome.to_lowercase()));
        Ok(cartelle)
    })
    .await
    .and_then(|esito| esito)
    .map_err(errore)
}

/// Aggiunge una cartella remota alle cartelle sorvegliate.
///
/// Chiede al servizio la prima pagina dell'elenco — che comincia con la
/// catena degli antenati — per sapere il percorso della cartella; poi la
/// scrive fra le sorvegliate con le regole di sempre e sveglia il filo del
/// conto, che continua l'elenco. La prima scansione parte subito con quella
/// pagina.
#[tauri::command]
pub async fn nuvola_aggiungi(
    app: AppHandle,
    conto: String,
    id: Option<String>,
    nomi: Vec<String>,
) -> Esito<CartelleOrdinate> {
    let _ = nomi;
    let mano = app.clone();
    let (ordinate, percorso) = in_disparte("cartella dal cloud", move || {
        aggiungi(&mano, &conto, id.as_deref())
    })
    .await
    .and_then(|esito| esito)
    .map_err(errore)?;
    crate::sorveglia::riconfigura(&app);
    crate::sorveglia::riguarda(&app, vec![percorso]);
    emetti_conti(&app);
    crate::nuvola::se_riuscito(&app, Ok::<_, Box<ErroreIpc>>(ordinate))
}

fn aggiungi(
    app: &AppHandle,
    conto: &str,
    id: Option<&str>,
) -> Result<(CartelleOrdinate, String), AppError> {
    let (stato, nuvola) = stati(app)?;
    let deposito = nuvola.deposito(conto).ok_or_else(|| {
        AppError::new(ErrorCode::CloudNotConnected {
            servizio: nome_del_conto(conto),
        })
    })?;
    // `None` è la cima del drive: per Graph `root` è un nome valido di voce.
    let cima = id.unwrap_or("root").to_owned();
    let mut prima_pagina = Vec::new();
    let esito = deposito.elenca(&cima, &mut |pagina| {
        prima_pagina = pagina;
        ControlFlow::Break(())
    });
    if prima_pagina.is_empty() {
        // L'errore vero, se c'è: un limite di frequenza o un consenso scaduto
        // non sono una cartella sparita.
        esito?;
        return Err(AppError::new(ErrorCode::CloudNotFound)
            .with_cause("il servizio non ha restituito la cartella"));
    }
    // La cima vera: per `root` il servizio risponde col suo identificativo.
    let cima_vera = if id.is_none() {
        prima_pagina
            .iter()
            .find(|voce| voce.genitore.is_none())
            .map_or_else(|| cima.clone(), |voce| voce.id.clone())
    } else {
        cima
    };
    let toccate = nuvola
        .specchio
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .metti(conto, prima_pagina);
    let percorso = nuvola
        .specchio
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .percorso(conto, &cima_vera)
        .ok_or_else(|| {
            AppError::new(ErrorCode::CloudNotFound)
                .with_cause("la catena degli antenati non è arrivata")
        })?;
    // Prima l'elenco delle cartelle, poi la radice nello specchio: il filo del
    // conto dimentica le radici che fra le sorvegliate non ci sono, e una
    // radice registrata un attimo prima di essere scritta lì verrebbe
    // dimenticata appena nata.
    let mut cartelle = crate::comandi::cartelle_sorvegliate(&stato)?;
    cartelle.push(percorso.clone());
    let ordinate = crate::comandi::scrivi_cartelle(&stato, &cartelle)?;
    let radice = RadiceSalvata {
        percorso: percorso.clone(),
        conto: conto.to_owned(),
        cima: cima_vera,
        completa: false,
    };
    nuvola
        .specchio
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .aggiungi_radice(radice.clone());
    // Una scrittura caduta rilegge il conto (vedi `salva_toccate`): la radice
    // resta in memoria, incompleta, e il filo la elenca da capo.
    salva_toccate(&stato, &nuvola, conto, deposito.as_ref(), &toccate)?;
    con_libreria(&stato, |l| specchio::salva_radice(&l.connection, &radice))?;
    nuvola.sveglia(conto);
    Ok((ordinate, percorso))
}

/// «Aggiorna ora»: sveglia il filo del conto.
///
/// Un conto rimasto senza deposito — il portachiavi non ha risposto
/// all'avvio — si riapre, e il suo filo riparte. Prima «Aggiorna» svegliava un
/// filo che non c'era, e il conto restava fermo fino al riavvio.
///
/// Lo stesso per un conto «da ricollegare» che il deposito ce l'ha ancora:
/// vedi [`aggiorna_riapre`].
#[tauri::command]
pub async fn nuvola_aggiorna(app: AppHandle, conto: String) -> Esito<()> {
    let mano = app.clone();
    in_disparte("aggiornamento di un conto", move || {
        let (_, nuvola) = stati(&mano)?;
        let guasto = nuvola.guasto(&conto);
        if !aggiorna_riapre(nuvola.deposito(&conto).is_some(), guasto.as_ref()) {
            nuvola.sveglia(&conto);
            return Ok(());
        }
        con_i_collegamenti_fermi(&mano, || {
            app_cambiata(&mano, std::slice::from_ref(&conto));
            Ok(())
        })
    })
    .await
    .and_then(|esito| esito)
    .map_err(errore)
}

/// «Aggiorna» riapre il conto invece di svegliarne il filo?
///
/// Sì se il conto non ha un deposito (non si è riaperto all'avvio), e sì se
/// il suo ultimo guasto è da ricollegare. L'`Accesso` del deposito ricorda un
/// rifiuto definitivo per dieci minuti (`RICORDO_DEL_RIFIUTO`, in
/// `rymembra_cloud::musica::accesso`), e svegliare il filo darebbe lo stesso
/// rifiuto senza chiedere niente al servizio: chi preme «Aggiorna» dopo aver
/// rimediato dal lato del servizio (un client ripristinato nella console, un
/// consenso ridato) vuole un tentativo vero adesso. `rimetti` crea un
/// deposito nuovo, con un `Accesso` senza ricordi.
fn aggiorna_riapre(deposito: bool, guasto: Option<&AppError>) -> bool {
    !deposito || guasto.is_some_and(da_ricollegare)
}

/// Com'è la cache dei brani.
///
/// In disparte: il lucchetto della cache lo tiene anche chi scrive un pezzo
/// sul disco o ricompone una copia intera, e sul filo della finestra
/// l'attesa era un'interfaccia ferma.
#[tauri::command]
pub async fn nuvola_cache_stato(app: AppHandle) -> Esito<StatoCacheNuvola> {
    in_disparte("cache del cloud", move || {
        let (_, nuvola) = stati(&app)?;
        Ok::<_, AppError>(stato_cache(&nuvola))
    })
    .await
    .and_then(|esito| esito)
    .map_err(errore)
}

/// Accende, spegne o cambia il tetto della cache.
#[tauri::command]
pub async fn nuvola_cache_imposta(
    app: AppHandle,
    attiva: bool,
    tetto_mb: u64,
) -> Esito<StatoCacheNuvola> {
    let mano = app.clone();
    let adesso = in_disparte("cache del cloud", move || {
        let (stato, nuvola) = stati(&mano)?;
        // Un tetto che la finestra non offre si porta al più vicino sotto:
        // nessuno ha scelto un numero a caso.
        let tetto_mb = TETTI_OFFERTI
            .iter()
            .rev()
            .find(|offerto| **offerto <= tetto_mb)
            .copied()
            .unwrap_or(200);
        con_libreria(&stato, |l| {
            settings::write(
                &l.connection,
                CHIAVE_CACHE_ATTIVA,
                if attiva { "1" } else { "0" },
            )?;
            settings::write(&l.connection, CHIAVE_CACHE_TETTO, &tetto_mb.to_string())
        })?;
        if let Some(archivio) = &nuvola.archivio {
            archivio
                .cache()
                .imposta(tetto_mb.saturating_mul(1024 * 1024), attiva);
        }
        Ok::<_, AppError>(stato_cache(&nuvola))
    })
    .await
    .and_then(|esito| esito)
    .map_err(errore)?;
    app.emetti("nuvola:cache", adesso.clone());
    Ok(adesso)
}

/// Svuota la cache dei brani.
#[tauri::command]
pub async fn nuvola_cache_svuota(app: AppHandle) -> Esito<StatoCacheNuvola> {
    let mano = app.clone();
    let adesso = in_disparte("cache del cloud", move || {
        let (_, nuvola) = stati(&mano)?;
        if let Some(archivio) = &nuvola.archivio {
            archivio.cache().svuota()?;
        }
        Ok::<_, AppError>(stato_cache(&nuvola))
    })
    .await
    .and_then(|esito| esito)
    .map_err(errore)?;
    app.emetti("nuvola:cache", adesso.clone());
    Ok(adesso)
}

/// Apre la pagina del file sul sito del servizio, nel browser di sistema.
///
/// Un identificativo e non un percorso, come «Mostra nella cartella»: il
/// percorso lo legge il nucleo, e quel che si apre è solo un file che la
/// libreria conosce. Per un brano d'immagine CD si apre l'immagine.
#[tauri::command]
pub async fn brano_apri_sul_web(app: AppHandle, id: i64) -> Esito<()> {
    in_disparte("apri sul web", move || {
        let (stato, _) = stati(&app)?;
        let id = crate::riproduzione::riga_di_libreria(id)?;
        let percorso = con_libreria(&stato, |l| {
            crate::comandi::file_da_mostrare(&l.connection, id)
        })?;
        // Il motivo vero se il conto non è collegato: «collega OneDrive», non
        // «file non trovato».
        let indirizzo = match archivio(&app) {
            Some(archivio) => archivio.indirizzo_web(&percorso)?,
            None => None,
        }
        .ok_or_else(|| AppError::new(ErrorCode::CloudNotFound))?;
        tauri_plugin_opener::open_url(indirizzo, None::<&str>).map_err(|err| {
            AppError::new(ErrorCode::InternalAborted {
                what: Some("apertura del browser".to_owned()),
            })
            .with_cause(err.to_string())
        })
    })
    .await
    .and_then(|esito| esito)
    .map_err(errore)
}

#[cfg(test)]
mod prove {
    use super::*;

    use rymembra_cloud::portachiavi::{Guasto, InMemoria};

    fn servizio() -> String {
        "OneDrive".to_owned()
    }

    #[test]
    fn aggiorna_riapre_un_conto_da_ricollegare() {
        // Con il deposito al suo posto e nessun guasto, «Aggiorna» sveglia il
        // filo e basta; un guasto di rete lo stesso.
        assert!(!aggiorna_riapre(true, None));
        let rete = AppError::new(ErrorCode::NetTimeout {
            url: None,
            timeout_ms: None,
        });
        assert!(!aggiorna_riapre(true, Some(&rete)));
        // Un rifiuto definitivo ricordato dall'`Accesso`: svegliare il filo
        // darebbe lo stesso rifiuto senza chiedere niente al servizio per
        // dieci minuti. Si riapre il conto, con un `Accesso` nuovo.
        for respinto in [
            ErrorCode::CloudAuthExpired {
                servizio: servizio(),
            },
            ErrorCode::CloudAppRejected {
                servizio: servizio(),
            },
        ] {
            assert!(aggiorna_riapre(true, Some(&AppError::new(respinto))));
        }
        // E un conto senza deposito si riapre sempre, com'era.
        assert!(aggiorna_riapre(false, None));
    }

    #[test]
    fn solo_un_rifiuto_definitivo_spegne_le_radici() {
        for (codice, atteso) in [
            (
                ErrorCode::CloudAuthExpired {
                    servizio: servizio(),
                },
                true,
            ),
            (
                ErrorCode::CloudAppRejected {
                    servizio: servizio(),
                },
                true,
            ),
            (
                ErrorCode::NetTimeout {
                    url: None,
                    timeout_ms: None,
                },
                false,
            ),
            (ErrorCode::CloudNotFound, false),
        ] {
            let guasto = AppError::new(codice);
            assert_eq!(respinto_per_sempre(&guasto), atteso, "{guasto}");
        }
    }

    fn con_guasto(codice: ErrorCode) -> Andamento {
        Andamento {
            guasto: Some(AppError::new(codice)),
            ..Andamento::default()
        }
    }

    #[test]
    fn un_portachiavi_muto_all_avvio_si_aggiorna_non_si_ricollega() {
        let muto = con_guasto(ErrorCode::SettingsSecretUnavailable {
            key: "nuvola.onedrive-abcdefgh.refresh".to_owned(),
        });
        assert_eq!(
            stato_del_conto(false, &muto),
            "errore",
            "«Aggiorna» lo riapre"
        );
        let scaduto = con_guasto(ErrorCode::CloudAuthExpired {
            servizio: "OneDrive".to_owned(),
        });
        assert_eq!(stato_del_conto(false, &scaduto), "daRicollegare");
        assert_eq!(stato_del_conto(true, &scaduto), "daRicollegare");
        assert_eq!(
            stato_del_conto(false, &Andamento::default()),
            "daRicollegare",
            "senza deposito e senza motivo: scollegato"
        );
        assert_eq!(stato_del_conto(true, &muto), "errore");
        assert_eq!(stato_del_conto(true, &Andamento::default()), "collegato");
    }

    #[test]
    fn webdav_un_portachiavi_muto_non_e_una_password_mancante() {
        let conto = ContoSalvato {
            conto: "webdav-abcdefgh".to_owned(),
            servizio: Servizio::WebDav,
            nome: "mario@cloud.it".to_owned(),
            dettagli: Some(r#"{"indirizzo":"https://cloud.it/dav","utente":"mario"}"#.to_owned()),
            cursore: None,
            ultimo_ms: None,
            app: None,
            client: None,
        };
        let codice = |esito: Result<Arc<dyn Deposito>, AppError>| {
            esito.err().map(|guasto| guasto.code().kind())
        };
        assert_eq!(
            codice(riapri_webdav(&conto, &Guasto)),
            Some(ErrorCodeKind::SettingsSecretUnavailable)
        );
        let vuoto = InMemoria::nuovo();
        assert_eq!(
            codice(riapri_webdav(&conto, &vuoto)),
            Some(ErrorCodeKind::CloudAuthExpired)
        );
        vuoto
            .scrivi(&portachiavi::nuvola_password(&conto.conto), "segreta")
            .expect("scritta");
        assert_eq!(codice(riapri_webdav(&conto, &vuoto)), None);
        let senza_indirizzo = ContoSalvato {
            dettagli: None,
            ..conto
        };
        assert_eq!(
            codice(riapri_webdav(&senza_indirizzo, &vuoto)),
            Some(ErrorCodeKind::CloudAuthExpired)
        );
    }

    #[test]
    fn il_nome_di_una_radice_e_la_catena_dei_nomi() {
        assert_eq!(
            nome_radice("nuvola://onedrive-abcdefgh/Musica/Rock%7CPop"),
            "Musica › Rock|Pop"
        );
        assert_eq!(nome_radice("nuvola://onedrive-abcdefgh"), "OneDrive");
    }

    #[test]
    fn le_pause_dopo_i_guasti_crescono_e_si_fermano() {
        let rete = AppError::new(ErrorCode::NetOffline { url: None });
        assert_eq!(pausa_dopo(&rete, 1), PAUSA_GUASTO);
        assert_eq!(pausa_dopo(&rete, 2), PAUSA_GUASTO * 2);
        assert_eq!(pausa_dopo(&rete, 30), PAUSA_MASSIMA);
        let consenso = AppError::new(ErrorCode::CloudAuthExpired {
            servizio: "OneDrive".into(),
        });
        assert_eq!(pausa_dopo(&consenso, 1), PAUSA_MASSIMA);
        // Quel che si rimedia sistemando l'app non passa riprovando.
        for guasto in [
            ErrorCode::CloudClientMissing {
                servizio: "OneDrive".into(),
            },
            ErrorCode::CloudAppChanged {
                servizio: "OneDrive".into(),
            },
            ErrorCode::CloudAppRejected {
                servizio: "OneDrive".into(),
            },
            ErrorCode::CloudAppSecretMissing,
        ] {
            assert_eq!(pausa_dopo(&AppError::new(guasto), 1), PAUSA_MASSIMA);
        }
        let piano = AppError::new(ErrorCode::NetRateLimited {
            service: None,
            retry_after_ms: Some(120_000),
        });
        assert_eq!(pausa_dopo(&piano, 1), Duration::from_secs(120));
    }

    #[test]
    fn solo_chi_sa_aspettare_aspetta() {
        assert!(!sa_aspettare(Servizio::OneDrive));
        assert!(!sa_aspettare(Servizio::GoogleDrive));
        assert!(sa_aspettare(Servizio::Dropbox));
    }

    #[test]
    fn una_sveglia_interrompe_l_attesa_e_un_canale_chiuso_fa_uscire() {
        let (sveglia, orecchio) = channel();
        let _ = sveglia.send(());
        let _ = sveglia.send(());
        assert!(!aspetta(&orecchio, Duration::from_secs(30)));
        drop(sveglia);
        assert!(aspetta(&orecchio, Duration::from_secs(30)));
    }

    #[test]
    fn i_tetti_offerti_cominciano_dai_duecento_mega() {
        assert_eq!(TETTI_OFFERTI.first(), Some(&200));
        assert_eq!(TETTO_DI_SERIE, 200 * 1024 * 1024);
    }

    /// Il corpo di una funzione di questo file, dalla sua firma alla prima
    /// graffa chiusa in colonna zero. La firma compare prima qui che nelle
    /// prove, quindi la prima occorrenza è quella vera.
    fn corpo_di(firma: &str) -> &'static str {
        let sorgente = include_str!("nuvola_musica.rs");
        sorgente
            .split(firma)
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default()
    }

    /// Le copie rimaste da misurare dall'ultima sessione si ripropongono
    /// all'avvio, e **dopo** `app.manage(nuvola)`: prima, la misura non
    /// troverebbe lo stato del cloud e le lascerebbe andare senza misurarle.
    #[test]
    fn le_copie_rimaste_da_misurare_si_ripropongono_dopo_lo_stato() {
        let avvia = corpo_di("pub fn avvia(app: &AppHandle) {");
        let registrato = avvia.find("app.manage(nuvola);");
        let riproposte = avvia.find(".da_misurare()");
        assert!(
            registrato.is_some() && riproposte.is_some() && riproposte > registrato,
            "le copie da misurare si ripropongono dopo aver registrato lo stato"
        );
    }

    /// «Scollega» toglie i brani in cache del conto (il commento del comando
    /// e `PRIVACY.md` lo promettono) e lo dice alla scheda della cache.
    #[test]
    fn scollegare_toglie_i_brani_in_cache_del_conto() {
        let scollega = corpo_di("pub async fn nuvola_conto_scollega(");
        assert!(
            scollega.contains(".togli_conto(&conto)"),
            "la cache non perde le copie del conto"
        );
        assert!(
            scollega.contains("\"nuvola:cache\""),
            "la scheda della cache non lo sa"
        );
    }
}
