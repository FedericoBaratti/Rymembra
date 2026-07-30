//! I comandi che la finestra può chiamare.
//!
//! Sono involucri e basta. Ogni riga di questo file o traduce un argomento, o
//! chiama `aether_app`, o traduce un errore: **niente decisioni**. Il momento in
//! cui un comando comincia a scegliere cosa inserire o cosa togliere è il
//! momento in cui quella scelta smette di essere provabile senza aprire una
//! finestra, e ricomincia a poter divergere da quella di Android.

use aether_app::import_legacy;
use aether_app::library::{
    AlbumSummary, Counts, Scan, ScanReport, TrackOrder, TrackSummary, album_tracks, counts,
    list_albums, list_tracks, search,
};
use aether_domain::errors::AppError;
use aether_domain::paths::PathRules;
use serde::Serialize;
use tauri::{Emitter as _, State};

use crate::errore::{Esito, errore};
use crate::stato::{Stato, con_libreria};

/// Quel che la finestra deve sapere appena si apre.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Avvio {
    /// Dove stanno database e copertine.
    pub data_dir: String,
    /// Quante migrazioni ha applicato questa apertura.
    pub migrazioni: usize,
    /// FTS5 è disponibile: senza, la ricerca non funzionerebbe.
    pub fts5: bool,
    /// Le cartelle sorvegliate.
    pub cartelle: Vec<String>,
    /// I numeri della libreria.
    pub numeri: Counts,
}

/// La chiave con cui le cartelle sorvegliate stanno in `settings`.
const CHIAVE_CARTELLE: &str = "library.roots";

fn leggi_cartelle(connection: &rusqlite::Connection) -> Result<Vec<String>, AppError> {
    let raw: Option<String> = connection
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [CHIAVE_CARTELLE],
            |row| row.get(0),
        )
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
        .map_err(|err| {
            AppError::new(aether_domain::errors::ErrorCode::DbQueryFailed {
                detail: Some("lettura delle cartelle sorvegliate".into()),
            })
            .with_cause(err.to_string())
        })?;
    // Una lista malformata vale come nessuna cartella, non come un errore: al
    // massimo l'utente le riseleziona, mentre un avvio che fallisce per un
    // valore di impostazione corrotto non gli lascia modo di correggerlo.
    Ok(raw
        .and_then(|value: String| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_default())
}

/// Lo stato all'avvio.
#[tauri::command]
pub fn avvio(stato: State<'_, Stato>) -> Esito<Avvio> {
    con_libreria(&stato, |libreria| {
        Ok(Avvio {
            data_dir: libreria.data_dir.display().to_string(),
            migrazioni: libreria.migrazioni,
            fts5: libreria.fts5,
            cartelle: leggi_cartelle(&libreria.connection)?,
            numeri: counts(&libreria.connection)?,
        })
    })
    .map_err(errore)
}

/// Cambia le cartelle sorvegliate.
#[tauri::command]
pub fn imposta_cartelle(stato: State<'_, Stato>, cartelle: Vec<String>) -> Esito<()> {
    con_libreria(&stato, |libreria| {
        let value = serde_json::to_string(&cartelle).unwrap_or_else(|_| "[]".to_owned());
        libreria
            .connection
            .execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                rusqlite::params![CHIAVE_CARTELLE, value],
            )
            .map(|_| ())
            .map_err(|err| {
                AppError::new(aether_domain::errors::ErrorCode::DbQueryFailed {
                    detail: Some("scrittura delle cartelle sorvegliate".into()),
                })
                .with_cause(err.to_string())
            })
    })
    .map_err(errore)
}

/// L'avanzamento di una scansione, mandato alla finestra mentre procede.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Avanzamento {
    /// File letti finora.
    pub fatti: usize,
    /// File da leggere in tutto.
    pub totale: usize,
}

/// Cosa ha fatto una scansione, nella forma che la finestra riceve.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EsitoScansione {
    /// Righe inserite.
    pub inseriti: usize,
    /// Righe riscritte.
    pub aggiornati: usize,
    /// Righe conservate perché il file si era solo spostato.
    pub spostati: usize,
    /// Righe tolte.
    pub tolti: usize,
    /// File saltati perché illeggibili.
    pub illeggibili: usize,
    /// Copertine ricodificate ora.
    pub copertine_nuove: usize,
    /// Quanto è durata, in millisecondi.
    pub durata_ms: u128,
    /// I numeri della libreria dopo.
    pub numeri: Counts,
}

impl EsitoScansione {
    fn da(report: &ScanReport, numeri: Counts) -> Self {
        Self {
            inseriti: report.inserted,
            aggiornati: report.updated,
            spostati: report.moved,
            tolti: report.removed,
            illeggibili: report.unreadable.len(),
            copertine_nuove: report.covers_stored,
            durata_ms: report.elapsed_ms,
            numeri,
        }
    }
}

/// Scansiona le cartelle sorvegliate.
///
/// Manda `scansione:avanzamento` mentre legge. È l'unico comando che dura più di
/// un istante, ed è il motivo per cui l'avanzamento esiste: sulla libreria vera
/// la prima passata sono venti secondi, e venti secondi senza un segno di vita
/// sono venti secondi in cui l'applicazione sembra bloccata.
#[tauri::command]
pub fn scansiona(app: tauri::AppHandle, stato: State<'_, Stato>) -> Esito<EsitoScansione> {
    con_libreria(&stato, |libreria| {
        let roots = leggi_cartelle(&libreria.connection)?;
        let scan = Scan {
            files: &aether_app::files::LocalFiles,
            covers: &libreria.covers,
            roots: &roots,
            rules: PathRules::for_current_platform(),
        };
        let mut ultimo = 0usize;
        let report = scan.run(&mut libreria.connection, |fatti, totale| {
            // Non a ogni file: mandare un evento per ognuno di 1421 file
            // inonderebbe il canale IPC per disegnare una barra che si muove di
            // meno di un pixel per volta.
            if fatti == totale || fatti.saturating_sub(ultimo) >= 25 {
                ultimo = fatti;
                let _ = app.emit("scansione:avanzamento", Avanzamento { fatti, totale });
            }
        })?;
        let numeri = counts(&libreria.connection)?;
        Ok(EsitoScansione::da(&report, numeri))
    })
    .map_err(errore)
}

/// Cerca in libreria.
#[tauri::command]
pub fn cerca(stato: State<'_, Stato>, query: String, limite: usize) -> Esito<Vec<TrackSummary>> {
    con_libreria(&stato, |libreria| {
        search(&libreria.connection, &query, limite)
    })
    .map_err(errore)
}

/// Una pagina di brani.
#[tauri::command]
pub fn brani(
    stato: State<'_, Stato>,
    ordine: String,
    offset: i64,
    limite: i64,
) -> Esito<Vec<TrackSummary>> {
    // Il nome dell'ordinamento si traduce qui in un valore chiuso: quel che
    // arriva dalla finestra non deve poter raggiungere una clausola SQL.
    let ordine = match ordine.as_str() {
        "recenti" => TrackOrder::RecentlyAdded,
        "ascoltati" => TrackOrder::MostPlayed,
        "titolo" => TrackOrder::Title,
        _ => TrackOrder::Shelf,
    };
    con_libreria(&stato, |libreria| {
        list_tracks(&libreria.connection, ordine, offset, limite)
    })
    .map_err(errore)
}

/// Una pagina di album.
#[tauri::command]
pub fn album(stato: State<'_, Stato>, offset: i64, limite: i64) -> Esito<Vec<AlbumSummary>> {
    con_libreria(&stato, |libreria| {
        list_albums(&libreria.connection, offset, limite)
    })
    .map_err(errore)
}

/// I brani di un album.
#[tauri::command]
pub fn brani_album(stato: State<'_, Stato>, chiave: String) -> Esito<Vec<TrackSummary>> {
    con_libreria(&stato, |libreria| {
        album_tracks(&libreria.connection, &chiave)
    })
    .map_err(errore)
}

/// Mette o toglie un preferito.
///
/// Scrive anche `liked_at`: è il timestamp della **decisione**, ed è ciò che
/// permette a un «non mi piace più» di vincere su un «mi piace» più vecchio
/// quando due dispositivi si allineano. Senza, il cuoricino tolto qui
/// tornerebbe indietro dal telefono.
#[tauri::command]
pub fn preferito(stato: State<'_, Stato>, id: i64, valore: bool) -> Esito<()> {
    con_libreria(&stato, |libreria| {
        let now = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
        )
        .unwrap_or(0);
        libreria
            .connection
            .execute(
                "UPDATE tracks SET liked = ?2, liked_at = ?3, stats_updated_at = ?3 WHERE id = ?1",
                rusqlite::params![id, i64::from(valore), now],
            )
            .map(|_| ())
            .map_err(|err| {
                AppError::new(aether_domain::errors::ErrorCode::DbQueryFailed {
                    detail: Some("preferito".into()),
                })
                .with_cause(err.to_string())
            })
    })
    .map_err(errore)
}

/// Cosa porterebbe l'importazione dal vecchio database.
#[tauri::command]
pub fn piano_importazione(
    stato: State<'_, Stato>,
    percorso: String,
) -> Esito<import_legacy::ImportReport> {
    importa_interno(&stato, &percorso, false)
}

/// Importa dal vecchio database.
#[tauri::command]
pub fn importa(stato: State<'_, Stato>, percorso: String) -> Esito<import_legacy::ImportReport> {
    importa_interno(&stato, &percorso, true)
}

fn importa_interno(
    stato: &State<'_, Stato>,
    percorso: &str,
    esegui: bool,
) -> Esito<import_legacy::ImportReport> {
    con_libreria(stato, |libreria| {
        let legacy = import_legacy::open_legacy(std::path::Path::new(percorso))?;
        if esegui {
            import_legacy::import(&legacy, &mut libreria.connection)
        } else {
            import_legacy::plan_import(&legacy, &mut libreria.connection)
        }
    })
    .map_err(errore)
}
