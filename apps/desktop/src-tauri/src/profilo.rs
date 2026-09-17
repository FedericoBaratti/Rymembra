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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use aether_app::profilo::{self, Ambiente, Esportazione, Passo, Piano, Rimappatura, Sorgenti};
use aether_domain::errors::{AppError, ErrorCode};
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
/// `internal.aborted` se la libreria non c'è più; `db.queryFailed` se la
/// raccolta fallisce; `fs.writeFailed` se l'archivio non si scrive.
#[tauri::command(async)]
pub async fn profilo_esporta(app: AppHandle, percorso: String) -> Esito<Esportazione> {
    let mano = app.clone();
    in_disparte("esportazione del profilo", move || {
        esporta_ora(&mano, &percorso)
    })
    .await
    .map_err(errore)?
    .map_err(errore)
}

/// Il corpo di [`profilo_esporta`], sul filo che l'ha presa in disparte.
fn esporta_ora(app: &AppHandle, percorso: &str) -> Result<Esportazione, AppError> {
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
        Path::new(percorso),
        &|passo| avanza(app, "archivio", passo),
    )
}

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
) -> Esito<Piano> {
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
) -> Esito<Piano> {
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
) -> Result<Piano, AppError> {
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

    // ── fase 3: sotto lucchetto, una transazione sola. ─────────────────────
    con_libreria(&stato, |libreria| {
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

/// Questo percorso esiste su questo computer?
///
/// Il modulo del profilo non guarda il disco da sé — la si passa come funzione,
/// così le sue prove girano senza avere le cartelle di nessuno. Qui il disco
/// c'è, ed è questa riga.
fn esiste(percorso: &str) -> bool {
    Path::new(percorso).exists()
}
