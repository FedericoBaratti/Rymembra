//! I comandi del profilo: esportare, guardare, applicare, tornare indietro.
//!
//! Involucri, come tutto il resto di questo crate: la decisione di cosa entra
//! nel profilo, come si scrive l'archivio e cosa vince quando si fonde sta in
//! `aether_app::profilo`. Qui c'è quel che quel modulo non può avere — il
//! lucchetto della libreria, la cartella dei dati, la casualità del sistema, e
//! la finestra a cui raccontare a che punto è.
//!
//! # Perché un modulo suo e non quattro funzioni in `comandi.rs`
//!
//! Perché ognuno dei quattro ha una disciplina di lucchetto che va scritta
//! accanto a lui. In `comandi.rs` sarebbero quattro funzioni lunghe in mezzo a
//! novanta comandi che una query la fanno e basta, e la parte che conta —
//! *dove* il lucchetto si prende e *dove* si lascia — si leggerebbe solo
//! cercandola.
//!
//! # Le tre fasi, e perché sono tre
//!
//! Un profilo con le copertine di una libreria vera pesa centinaia di megabyte,
//! e leggerli dal disco vale minuti. Tenere il lucchetto della libreria per
//! tutto quel tempo vorrebbe dire una finestra ferma e la riproduzione che si
//! interrompe al cambio di brano, perché anche il lettore passa da lì.
//!
//! 1. **Sotto lucchetto**: `raccogli`, cioè `sincronia::{allinea, contenuto}`
//!    più la biblioteca. Cinque interrogazioni, millisecondi. Devono stare
//!    nella stessa finestra o le due metà del profilo descriverebbero due
//!    istanti diversi della stessa libreria.
//! 2. **Fuori da ogni lucchetto**: la lettura dei jpeg e la scrittura dello
//!    zip. È possibile perché [`crate::stato::Stato::copertine`] non chiede il
//!    lucchetto — lo store è un percorso, e l'ha detto per iscritto molto prima
//!    che servisse a questo.
//! 3. **Sotto lucchetto, una transazione sola**: `applica_in` e la biblioteca.
//!    O la libreria è quella di prima o è quella dopo.
//!
//! In importazione le fasi sono le stesse, nell'ordine inverso: si legge
//! l'archivio (2), si estraggono copertine e pacchetti (2), e solo alla fine si
//! scrive nel database (3). Le copertine **prima** delle righe, perché una riga
//! che nomina un file che non c'è è una copertina rotta, mentre un file senza
//! riga è un file che nessuno guarda.
//!
//! # Perché tutti e quattro sono `(async)` e in disparte
//!
//! Perché aprono un file che l'utente ha scelto, e le due destinazioni naturali
//! di un profilo sono una chiavetta e una cartella di rete. Su una share che
//! non risponde, una lettura sul filo principale vale i quaranta secondi di
//! timeout di Windows — vedi [`crate::disparte`], che spiega anche perché
//! `(async)` da solo non basta.
//!
//! Ne segue la forma di ogni comando: prende un [`AppHandle`] e non uno
//! `State`, perché quel che si muove sul pool dev'essere `'static` mentre uno
//! `State` è un prestito che vive quanto la chiamata. È la stessa forma di
//! `comandi::scansiona`, e per la stessa ragione.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

use aether_app::profilo::radici;
use aether_app::profilo::{self, Ambiente, Esportazione, Passo, Piano, Rimappatura, Sorgenti};
use aether_app::volumi::{self, Esistenza};
use aether_domain::errors::{AppError, ErrorCode};
use serde::Serialize;
use tauri::{AppHandle, Manager as _};

use crate::disparte::in_disparte;
use crate::errore::{Esito, errore};
use crate::spegnimento::Emette as _;
use crate::stato::{Stato, adesso_ms, con_libreria};

/// Dove sta il pacchetto di una skin installata.
///
/// La stessa forma di `skin::cartella_skin` e di quella di
/// `aether_cloud::pacchetti`, e questa è la terza copia: le altre due
/// restituiscono **byte**, e qui serve il **percorso** perché l'archivio ci
/// copia dentro in streaming invece di caricare venti megabyte per skin. Tre
/// righe con questa nota costano meno di un accessore pubblico in più su due
/// moduli che non sono di questo pacchetto.
fn file_skin(data_dir: &Path, id: &str) -> PathBuf {
    data_dir.join("skin").join(format!("{id}.aeskin"))
}

/// Dove sta il manifest di una bozza dello Studio.
fn file_bozza(data_dir: &Path, id: &str) -> PathBuf {
    data_dir
        .join("skin")
        .join("bozze")
        .join(id)
        .join("skin.json")
}

/// Manda alla finestra un passo di avanzamento.
///
/// Stessa forma di `nuvola:avanzamento`, perché la finestra ha già un modo di
/// disegnarla e due forme diverse per la stessa cosa sono due componenti.
fn avanza(app: &AppHandle, cosa: &str, passo: Passo) {
    app.emetti(
        "profilo:avanzamento",
        serde_json::json!({
            "fatti": passo.fatti,
            "totale": passo.totale,
            "cosa": cosa,
        }),
    );
}

/// Un identificativo nuovo per questa libreria.
///
/// Dalla casualità del sistema operativo, la stessa di `nuvola::dispositivo`:
/// due librerie che si dichiarassero la stessa fonderebbero le proprie
/// cronologie, che è il guasto da cui l'identità esiste per difendere.
fn genera_identita() -> Result<String, AppError> {
    aether_cloud::oauth::identificativo()
}

/// Lo stato della libreria, o l'errore di chi è arrivato mentre si chiudeva.
///
/// `try_state` e non `state`: `state` panica se lo stato non c'è, e questi
/// corpi girano su un filo del pool bloccante, che può ritrovarsi vivo mentre
/// la finestra si chiude e gli stati gestiti vengono lasciati cadere.
fn libreria<'a>(
    app: &'a AppHandle,
    cosa: &'static str,
) -> Result<tauri::State<'a, Stato>, AppError> {
    app.try_state::<Stato>().ok_or_else(|| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some(cosa.to_owned()),
        })
        .with_cause("la libreria non è più fra gli stati gestiti")
    })
}

/// Quel che serve prima di prendere qualunque altro lucchetto.
fn preludio(stato: &tauri::State<'_, Stato>) -> Result<(PathBuf, String), AppError> {
    con_libreria(stato, |libreria| {
        Ok((
            libreria.data_dir.clone(),
            crate::nuvola::dispositivo(&libreria.connection)?,
        ))
    })
}

/// I pacchetti su disco: le impronte per il documento, i percorsi per l'archivio.
///
/// Si calcola **fuori** dal lucchetto: sono blake3 su file da qualche megabyte,
/// cioè decine di millisecondi che non hanno niente a che fare con il database.
type Pacchetti = (
    BTreeMap<String, String>,
    BTreeMap<String, String>,
    BTreeMap<String, PathBuf>,
    BTreeMap<String, PathBuf>,
);

/// Le impronte e i percorsi di skin e bozze.
fn pacchetti(data_dir: &Path) -> Pacchetti {
    let skin = aether_cloud::pacchetti::elenca_skin(data_dir);
    let bozze = aether_cloud::pacchetti::elenca_bozze(data_dir);
    let percorsi_skin = skin
        .keys()
        .map(|id| (id.clone(), file_skin(data_dir, id)))
        .collect();
    let percorsi_bozze = bozze
        .keys()
        .map(|id| (id.clone(), file_bozza(data_dir, id)))
        .collect();
    (skin, bozze, percorsi_skin, percorsi_bozze)
}

/// Scrive il profilo su un file.
///
/// Le tre fasi in ordine: una finestra di lucchetto corta per raccogliere, poi
/// tutto il resto fuori. Vedi il `//!`.
///
/// # Errori
///
/// `fs.extensionRefused` o `fs.pathInvalid` se il percorso non è quello di un
/// `.aeprofile` — si controlla prima di tutto il resto, perché non costa niente
/// e non tocca il disco: vedi [`crate::destinazioni`];
/// `internal.aborted` se la libreria non c'è più; `db.queryFailed` se la
/// raccolta fallisce; `fs.writeFailed` se l'archivio non si scrive.
#[tauri::command(async)]
pub async fn profilo_esporta(app: AppHandle, percorso: String) -> Esito<Esportazione> {
    let file =
        crate::destinazioni::file_da_scrivere(&percorso, crate::destinazioni::ESTENSIONI_PROFILO)
            .map_err(errore)?;
    let mano = app.clone();
    in_disparte("esportazione del profilo", move || {
        esporta_ora(&mano, &file.percorso)
    })
    .await
    .map_err(errore)?
    .map_err(errore)
}

/// Il corpo di [`profilo_esporta`], sul filo che l'ha presa in disparte.
fn esporta_ora(app: &AppHandle, percorso: &Path) -> Result<Esportazione, AppError> {
    let stato = libreria(app, "esportazione del profilo")?;
    let (data_dir, dispositivo) = preludio(&stato)?;
    let (skin, bozze, percorsi_skin, percorsi_bozze) = pacchetti(&data_dir);

    // ── fase 1: sotto lucchetto, e per pochi millisecondi. ─────────────────
    let raccolto = con_libreria(&stato, |libreria| {
        profilo::raccogli(
            &mut libreria.connection,
            &dispositivo,
            skin,
            bozze,
            adesso_ms(),
            &genera_identita,
        )
    })?;

    // ── fase 2: fuori da ogni lucchetto. ───────────────────────────────────
    let store = stato.copertine()?;
    profilo::scrivi(
        &raccolto,
        &Sorgenti {
            copertine: &store,
            skin: percorsi_skin,
            bozze: percorsi_bozze,
        },
        percorso,
        &|passo| avanza(app, "archivio", passo),
    )
}

/// Il piano del profilo, con quel che il nucleo non sa contare da sé.
///
/// # Perché un involucro
///
/// Perché le due cose in più nascono **qui**, dove il disco si guarda: il
/// nucleo del profilo riceve una domanda a due risposte («il file c'è?») e
/// quel che non si è potuto sapere lo conterebbe fra gli irrintracciabili.
/// Qui lo si toglie da lì e lo si dice a parte. Sul filo la forma è quella di
/// [`Piano`] con due campi in più, e chi non li conosce li ignora.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PianoProfilo {
    /// Il piano del nucleo, così com'è — con gli irrintracciabili già al netto
    /// dei non verificati.
    #[serde(flatten)]
    pub piano: Piano,
    /// Brani il cui file, sotto il prefisso nuovo, non si è potuto verificare:
    /// la share non rispondeva, o il tempo è scaduto. La loro riga resta
    /// com'era.
    pub brani_non_verificati: usize,
    /// I numeri della rimappatura vengono da un campione: è il piano, e sul
    /// piano non si guarda ogni file. L'importazione li guarda tutti.
    pub stima: bool,
}

/// Quanti file per rimappatura guarda il **piano**.
///
/// Sessantaquattro: abbastanza da distinguere i tre casi che contano — la
/// destinazione giusta, quella sbagliata, la share spenta — e abbastanza pochi
/// da restare sotto il secondo anche su un NAS in Wi-Fi. L'importazione vera
/// non campiona.
const CAMPIONE_DEL_PIANO: usize = 64;

/// Quanto si aspetta una risposta su un percorso che non è un brano — una
/// radice del profilo, la cartella dei download.
///
/// Più corta della scadenza dei brani perché queste domande partono con il
/// lucchetto della libreria in mano: sono poche, ma ognuna, su una share morta,
/// sarebbe il tempo in cui il lettore non passa al brano dopo.
const SCADENZA_PERCORSO: Duration = Duration::from_secs(3);

/// Cosa cambierebbe importare questo profilo. Non applica niente.
///
/// `rimappature` sono quelle che chi importa ha scelto: vuote la prima volta,
/// e allora il piano ne **propone** una per ogni radice che qui non esiste.
///
/// # Errori
///
/// `internal.aborted` se la libreria non c'è più; `settings.corrupt` se il file
/// non è un profilo leggibile; `db.queryFailed` per il resto.
#[tauri::command(async)]
pub async fn profilo_piano(
    app: AppHandle,
    percorso: String,
    rimappature: Vec<Rimappatura>,
    // Facoltativo per chi chiama senza: vale «no», com'era prima.
    unisci_comunque: Option<bool>,
) -> Esito<PianoProfilo> {
    let mano = app.clone();
    let unisci = unisci_comunque.unwrap_or(false);
    in_disparte("piano del profilo", move || {
        applica_ora(&mano, &percorso, &rimappature, unisci, false)
    })
    .await
    .map_err(errore)?
    .map_err(errore)
}

/// Applica il profilo, dopo aver scritto una copia di sicurezza.
///
/// # Errori
///
/// Come [`profilo_piano`], più `fs.writeFailed` se la copia di sicurezza non si
/// scrive — e allora l'importazione **non parte**.
#[tauri::command(async)]
pub async fn profilo_importa(
    app: AppHandle,
    percorso: String,
    rimappature: Vec<Rimappatura>,
    unisci_comunque: Option<bool>,
) -> Esito<PianoProfilo> {
    let mano = app.clone();
    let unisci = unisci_comunque.unwrap_or(false);
    let esito = in_disparte("importazione del profilo", move || {
        // **Prima** di scrivere, non dopo. Una copia scritta dopo sarebbe la
        // fotografia della libreria già cambiata, cioè esattamente il
        // contrario di quel che serve a chi vuole tornare indietro.
        copia_di_sicurezza(&mano)?;
        applica_ora(&mano, &percorso, &rimappature, unisci, true)
    })
    .await
    .map_err(errore)?;
    // Le radici importate cambiano l'albero delle cartelle: il pannello aperto
    // deve richiederlo, come dopo una scansione.
    if esito.is_ok() {
        crate::cartelle::cambiate(&app);
    }
    // Un profilo importato cambia preferenze, playlist e statistiche tutte
    // insieme: è il momento in cui il backup ha più da salvare.
    crate::nuvola::se_riuscito(&app, esito.map_err(errore))
}

/// Rimette le preferenze com'erano prima dell'ultima importazione.
///
/// **Solo le preferenze e la skin attiva.** La parte di libreria è additiva —
/// ascolti sommati, playlist fuse, testi scritti — e disfarla vorrebbe dire un
/// giornale grande quanto l'importazione. Chi preme il bottone deve leggere che
/// torna indietro la configurazione e non la storia, ed è il compito della
/// stringa `profile.undo.note`.
///
/// # Errori
///
/// `fs.notFound` se non c'è nessuna copia di sicurezza; per il resto come
/// [`profilo_importa`].
#[tauri::command(async)]
pub async fn profilo_annulla(app: AppHandle) -> Esito<Piano> {
    let mano = app.clone();
    let esito = in_disparte("annullamento del profilo", move || annulla_ora(&mano))
        .await
        .map_err(errore)?;
    crate::nuvola::se_riuscito(&app, esito.map_err(errore))
}

/// Il corpo di [`profilo_annulla`].
fn annulla_ora(app: &AppHandle) -> Result<Piano, AppError> {
    let stato = libreria(app, "annullamento del profilo")?;
    let (data_dir, dispositivo) = preludio(&stato)?;
    let copia = profilo::ultima_copia(&data_dir).ok_or_else(|| {
        AppError::new(ErrorCode::FsNotFound {
            path: data_dir.join(profilo::CARTELLA_COPIE).display().to_string(),
        })
        .with_cause("non c'è nessuna copia di sicurezza da cui tornare indietro")
    })?;
    let gia_chiesti = RefCell::new(HashMap::new());
    let esiste = |percorso: &str| esiste_entro(percorso, &gia_chiesti);
    con_libreria(&stato, |libreria| {
        profilo::annulla(
            &mut libreria.connection,
            &copia,
            &Ambiente {
                esiste: &esiste,
                adesso_ms: adesso_ms(),
                dispositivo: &dispositivo,
                rimappature: &[],
                copertine: None,
                unisci_comunque: false,
            },
        )
    })
}

/// Il corpo di [`profilo_piano`] e [`profilo_importa`].
///
/// Una funzione sola con un interruttore, come in `aether_app::profilo`: due
/// funzioni che devono fare la stessa cosa e differire in una riga sono due
/// funzioni che prima o poi differiscono in due.
fn applica_ora(
    app: &AppHandle,
    percorso: &str,
    rimappature: &[Rimappatura],
    unisci_comunque: bool,
    conferma: bool,
) -> Result<PianoProfilo, AppError> {
    let stato = libreria(app, "importazione del profilo")?;
    let (data_dir, dispositivo) = preludio(&stato)?;
    let store = stato.copertine()?;

    // ── fase 2: leggere. Fuori da ogni lucchetto. ──────────────────────────
    let letto = profilo::leggi(Path::new(percorso))?;

    // ── fase 2 bis: gli allegati, sempre fuori dal lucchetto. ──────────────
    //
    // Solo quando si conferma. Nel piano non si estrae niente: sarebbe una
    // scrittura dentro una funzione che promette di non farne, e le copertine
    // che mancherebbero il piano le conta guardando il disco.
    if conferma {
        profilo::estrai_copertine(Path::new(percorso), &store, &|passo| {
            avanza(app, "copertine", passo);
        })?;
        // I pacchetti passano dalle guardie di `aether_cloud::pacchetti`, che
        // sono le stesse dell'installazione a mano: identificatore sicuro,
        // archivio validato, e **non si sovrascrive mai** quel che c'è. Un
        // archivio arrivato su una chiavetta non è più fidato di uno scaricato.
        profilo::per_ogni_pacchetto(Path::new(percorso), |id, bozza, byte| {
            let scritto = if bozza {
                aether_cloud::pacchetti::scrivi_bozza(&data_dir, id, byte)
            } else {
                aether_cloud::pacchetti::scrivi_skin(&data_dir, id, byte)
            };
            // Un pacchetto illeggibile non ferma l'importazione: è una skin che
            // non si installa, non una libreria che non si importa. Il resto
            // del profilo vale comunque, e fermarsi qui vorrebbe dire perderlo
            // per colpa di un tema.
            Ok(scritto.unwrap_or(false))
        })?;
    }

    // ── fase 2 ter: i file della rimappatura, fuori da ogni lucchetto. ─────
    //
    // Fino alla 3.0 si chiedeva al disco se ogni file c'era **dentro** la
    // transazione della fase 3, cioè con il lucchetto della libreria in mano:
    // su una libreria di cinquantamila brani su un NAS erano cinquantamila
    // domande in fila, la finestra ferma, il lettore che non passava al brano
    // dopo e ogni altro comando che dopo quattro secondi diceva «database
    // occupato». Adesso i percorsi si leggono con il lucchetto preso per un
    // attimo, si verificano qui fuori in parallelo e con la scadenza, e la fase
    // 3 consulta soltanto le risposte. Nel piano si guarda un campione: vedi
    // `radici::Verifiche`.
    let candidati = con_libreria(&stato, |libreria| {
        radici::candidati(&libreria.connection, rimappature)
    })?;
    let verifiche = radici::verifica(
        candidati,
        (!conferma).then_some(CAMPIONE_DEL_PIANO),
        &|percorsi| volumi::esistenze(percorsi, volumi::SCADENZA_RISOLUZIONE),
    );

    // Le risposte, e il conto di quelle che non si sono sapute. Il nucleo del
    // profilo le riceve come «no» — la sua domanda ha due risposte — e le
    // conterebbe fra gli irrintracciabili: qui si contano a parte, e dopo le si
    // tolgono da lì.
    let non_verificati = Cell::new(0_usize);
    let gia_chiesti = RefCell::new(HashMap::new());
    let esiste = |percorso: &str| match verifiche.esito(percorso) {
        Some(Esistenza::Presente) => true,
        Some(Esistenza::Assente) => false,
        Some(Esistenza::Ignota) => {
            non_verificati.set(non_verificati.get().saturating_add(1));
            false
        }
        // Non è un brano: una radice del profilo, la cartella dei download.
        // Sono poche, e si chiedono qui con la loro scadenza.
        None => esiste_entro(percorso, &gia_chiesti),
    };

    // ── fase 3: sotto lucchetto, una transazione sola. ─────────────────────
    let mut piano = con_libreria(&stato, |libreria| {
        let ambiente = Ambiente {
            esiste: &esiste,
            adesso_ms: adesso_ms(),
            dispositivo: &dispositivo,
            rimappature,
            copertine: Some(&store),
            unisci_comunque,
        };
        if conferma {
            profilo::importa(&mut libreria.connection, &letto, &ambiente)
        } else {
            profilo::piano(&mut libreria.connection, &letto, &ambiente)
        }
    })?;
    let brani_non_verificati = non_verificati.get();
    piano.brani_irrintracciabili = piano
        .brani_irrintracciabili
        .saturating_sub(brani_non_verificati);
    Ok(PianoProfilo {
        piano,
        brani_non_verificati,
        stima: verifiche.stima(),
    })
}

/// Scrive la copia di sicurezza di com'è questo computer adesso, e ruota.
///
/// Un guasto qui **ferma** l'importazione, e non è severità per principio: la
/// copia è l'unica via di ritorno, e importare senza averla scritta vorrebbe
/// dire togliere silenziosamente l'annullamento a chi crede di averlo.
fn copia_di_sicurezza(app: &AppHandle) -> Result<(), AppError> {
    let stato = libreria(app, "copia di sicurezza del profilo")?;
    let (data_dir, dispositivo) = preludio(&stato)?;
    let (skin, bozze, percorsi_skin, percorsi_bozze) = pacchetti(&data_dir);

    let adesso = adesso_ms();
    let raccolto = con_libreria(&stato, |libreria| {
        profilo::raccogli(
            &mut libreria.connection,
            &dispositivo,
            skin,
            bozze,
            adesso,
            &genera_identita,
        )
    })?;

    let store = stato.copertine()?;
    profilo::scrivi(
        &raccolto,
        &Sorgenti {
            copertine: &store,
            skin: percorsi_skin,
            bozze: percorsi_bozze,
        },
        &profilo::copia_di_sicurezza(&data_dir, adesso),
        &|passo| avanza(app, "copia", passo),
    )?;
    profilo::ruota_le_copie(&data_dir);
    Ok(())
}

/// Questo percorso esiste su questo computer? Con la scadenza, e una volta sola.
///
/// Il modulo del profilo non guarda il disco da sé — la si passa come funzione,
/// così le sue prove girano senza avere le cartelle di nessuno. Qui il disco
/// c'è, ed è questa riga.
///
/// Per i pochi percorsi che non sono brani: le radici del profilo e la
/// cartella dei download, che il nucleo chiede con il lucchetto in mano. Prima
/// era un `Path::exists` nudo, che su una share spenta aspettava i quaranta
/// secondi di SMB per rispondere «no»; adesso aspetta [`SCADENZA_PERCORSO`], e
/// la stessa radice chiesta due volte — dalle proposte e dai percorsi mancanti
/// — si chiede al disco una volta sola.
///
/// «Non si sa» vale «no», e per queste domande è la risposta giusta: una
/// radice che non risponde si propone da rimappare, e chi importa può lasciare
/// vuoto il campo.
fn esiste_entro(percorso: &str, gia_chiesti: &RefCell<HashMap<String, bool>>) -> bool {
    if let Some(risposta) = gia_chiesti.borrow().get(percorso) {
        return *risposta;
    }
    let risposta = volumi::esistenza_entro(percorso, SCADENZA_PERCORSO) == Esistenza::Presente;
    gia_chiesti
        .borrow_mut()
        .insert(percorso.to_owned(), risposta);
    risposta
}
