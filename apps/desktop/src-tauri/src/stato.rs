//! Quel che l'applicazione tiene aperto mentre gira.
//!
//! Una connessione al database e uno store di copertine, dietro un mutex. Non
//! un pool: SQLite in WAL regge un solo scrittore comunque, e le letture di
//! questa applicazione durano microsecondi. Un pool aggiungerebbe la possibilità
//! che due comandi vedano due transazioni diverse senza toglierne nessuna reale.
//!
//! # Dove stanno i dati
//!
//! Nella cartella dati dell'applicazione, chiesta a Tauri invece che composta a
//! mano: su Windows è `%APPDATA%\<identificatore>`, e comporla a mano
//! significherebbe sbagliarla su un profilo mobile o su un utente con la
//! cartella spostata.
//!
//! La cartella è **diversa** da quella dell'applicazione rilasciata
//! (`%APPDATA%\Aether`). Deve esserlo: finché questa non è finita, la vecchia
//! deve restare apribile, e l'importatore la legge in sola lettura.

use std::path::PathBuf;
use std::sync::Mutex;

use aether_app::covers::CoverStore;
use aether_domain::errors::AppError;
use rusqlite::Connection;

/// La libreria aperta.
pub struct Libreria {
    /// La connessione al database.
    pub connection: Connection,
    /// Lo store delle copertine.
    pub covers: CoverStore,
    /// Dove stanno i dati, per poterlo mostrare all'utente.
    pub data_dir: PathBuf,
    /// Quante migrazioni ha applicato l'apertura, per la riga di avvio.
    pub migrazioni: usize,
    /// FTS5 è disponibile.
    pub fts5: bool,
}

/// Lo stato condiviso fra i comandi.
pub struct Stato {
    /// La libreria, o l'errore che ne ha impedito l'apertura.
    ///
    /// L'errore si conserva invece di far fallire l'avvio: una finestra che si
    /// apre e spiega perché il database non si è aperto è utilizzabile — ci si
    /// può leggere il percorso, cambiare cartella, ripristinare un backup. Un
    /// processo che esce prima di disegnare qualcosa non lo è.
    pub libreria: Mutex<Result<Libreria, AppError>>,
}

impl Stato {
    /// Apre la libreria nella cartella dati data.
    pub fn apri(data_dir: PathBuf) -> Self {
        Self {
            libreria: Mutex::new(apri_libreria(data_dir)),
        }
    }
}

fn apri_libreria(data_dir: PathBuf) -> Result<Libreria, AppError> {
    std::fs::create_dir_all(&data_dir)
        .map_err(|err| aether_app::files::io_error(&data_dir.display().to_string(), &err))?;
    let covers = CoverStore::open(data_dir.join("copertine"))?;
    let aperto = aether_app::db::open(&data_dir.join("aether.db"))?;
    Ok(Libreria {
        connection: aperto.connection,
        covers,
        data_dir,
        migrazioni: aperto.applied,
        fts5: aperto.fts5,
    })
}

/// Stampa cosa è successo all'apertura.
///
/// Una riga sola, e c'è **sempre**: nel vecchio albero i messaggi di avvio non
/// arrivavano su disco, e quando qualcosa andava storto prima della finestra non
/// restava niente da leggere.
pub fn riga_di_avvio(stato: &Stato) {
    let guardia = stato
        .libreria
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match guardia.as_ref() {
        Ok(libreria) => println!(
            "[avvio] libreria aperta dati={} migrazioni={} fts5={}",
            libreria.data_dir.display(),
            libreria.migrazioni,
            libreria.fts5
        ),
        Err(errore) => eprintln!(
            "[avvio] libreria NON aperta codice={} causa={}",
            errore.code().kind().code(),
            errore.cause().unwrap_or("—")
        ),
    }
}

/// Esegue `azione` sulla libreria aperta.
///
/// Il mutex avvelenato — un panico dentro un altro comando mentre teneva il
/// lucchetto — si recupera invece di propagare: i dati dietro sono una
/// connessione SQLite, che un panico in codice Rust non lascia a metà di
/// niente, e rifiutare ogni comando successivo trasformerebbe un guasto isolato
/// in un'applicazione morta fino al riavvio.
pub fn con_libreria<T>(
    stato: &Stato,
    azione: impl FnOnce(&mut Libreria) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let mut guardia = stato
        .libreria
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match guardia.as_mut() {
        Ok(libreria) => azione(libreria),
        Err(errore) => Err(errore.clone()),
    }
}
