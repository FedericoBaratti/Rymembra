//! I comandi che la finestra può chiamare.
//!
//! Sono involucri e basta. Ogni riga di questo file o traduce un argomento, o
//! chiama `aether_app`, o traduce un errore: **niente decisioni**. Il momento in
//! cui un comando comincia a scegliere cosa inserire o cosa togliere è il
//! momento in cui quella scelta smette di essere provabile senza aprire una
//! finestra, e ricomincia a poter divergere da quella di Android.

use aether_app::import_legacy;
use aether_app::library::{
    AlbumSummary, ArtistSummary, Counts, Scan, ScanReport, TrackOrder, TrackSummary, album_tracks,
    counts, list_albums, list_artists, list_tracks, search,
};
use aether_app::settings::CHIAVE_CARTELLE;
use aether_domain::errors::AppError;
use aether_domain::paths::PathRules;
use serde::Serialize;
use tauri::{Emitter as _, State};

use crate::errore::{Esito, errore};
use crate::stato::{Stato, adesso_ms, con_libreria};

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

/// Le cartelle sorvegliate.
///
/// Una lista malformata vale come nessuna cartella, non come un errore — al
/// massimo l'utente le riseleziona, mentre un avvio che fallisce per un valore
/// di impostazione corrotto non gli lascia modo di correggerlo. È la regola di
/// `settings::read_json`, dove sta ora insieme alla sua ragione.
fn leggi_cartelle(connection: &rusqlite::Connection) -> Result<Vec<String>, AppError> {
    Ok(aether_app::settings::read_json(connection, CHIAVE_CARTELLE)?.unwrap_or_default())
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
pub fn imposta_cartelle(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    cartelle: Vec<String>,
) -> Esito<()> {
    crate::nuvola::se_riuscito(
        &app,
        con_libreria(&stato, |libreria| {
            aether_app::settings::write_json(&libreria.connection, CHIAVE_CARTELLE, &cartelle)
        })
        .map_err(errore),
    )
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
    /// È stata fermata a metà.
    pub annullata: bool,
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
            annullata: report.cancelled,
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
    // Prima di prendere il lucchetto: un annullamento arrivato dopo la fine
    // della scansione precedente fermerebbe questa al primo file.
    stato.riprendi_scansioni();
    let fermare = &*stato;
    let esito = con_libreria(&stato, |libreria| {
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
            if fermare.scansione_fermata() {
                std::ops::ControlFlow::Break(())
            } else {
                std::ops::ControlFlow::Continue(())
            }
        })?;
        let numeri = counts(&libreria.connection)?;
        Ok(EsitoScansione::da(&report, numeri))
    })
    .map_err(errore);
    // Una scansione porta dentro brani che nessuno ha mai tentato di
    // arricchire, ed è il momento in cui hanno più bisogno: appena importati
    // sono precisamente quelli con «Album sconosciuto» e nessuna copertina.
    if esito.is_ok() {
        crate::arricchimento::sporca(&app);
    }
    // Una scansione cambia quali brani esistono, quindi quali statistiche il
    // backup può ancorare: un brano ritrovato dopo una reinstallazione va
    // salvato subito, non al prossimo cuoricino.
    crate::nuvola::se_riuscito(&app, esito)
}

/// Chiede alla scansione in corso di fermarsi.
///
/// Non chiede il lucchetto della libreria — e non può: quel lucchetto ce l'ha
/// la scansione che deve fermare, per tutta la sua durata. Alza un bit, e la
/// scansione lo legge fra un file e l'altro.
///
/// Torna subito: fermarsi vuol dire «alla fine del lotto in corso», non
/// «adesso». Chi la mostra lo sa dall'esito, che dirà `annullata`.
#[tauri::command]
pub fn annulla_scansione(stato: State<'_, Stato>) -> Esito<()> {
    stato.ferma_scansione();
    Ok(())
}

/// Una pagina di risultati.
#[tauri::command]
pub fn cerca(
    stato: State<'_, Stato>,
    query: String,
    offset: i64,
    limite: i64,
) -> Esito<Vec<TrackSummary>> {
    con_libreria(&stato, |libreria| {
        search(&libreria.connection, &query, offset, limite)
    })
    .map_err(errore)
}

/// Quanti risultati ha questa ricerca in tutto.
///
/// Separato dalla pagina e non un campo del risultato: la pagina si chiede a
/// ogni scorrimento, il conteggio una volta per query. Metterli insieme
/// vorrebbe dire rifare la `COUNT(*)` a ogni fetta.
#[tauri::command]
pub fn cerca_conteggio(stato: State<'_, Stato>, query: String) -> Esito<i64> {
    con_libreria(&stato, |libreria| {
        aether_app::library::search_count(&libreria.connection, &query)
    })
    .map_err(errore)
}

/// Una pagina di preferiti.
#[tauri::command]
pub fn preferiti(stato: State<'_, Stato>, offset: i64, limite: i64) -> Esito<Vec<TrackSummary>> {
    con_libreria(&stato, |libreria| {
        aether_app::library::list_liked(&libreria.connection, offset, limite)
    })
    .map_err(errore)
}

/// Una pagina degli album di un artista.
#[tauri::command]
pub fn album_artista(
    stato: State<'_, Stato>,
    nome: String,
    offset: i64,
    limite: i64,
) -> Esito<Vec<AlbumSummary>> {
    con_libreria(&stato, |libreria| {
        aether_app::library::albums_by_artist(&libreria.connection, &nome, offset, limite)
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

/// Tutti gli artisti, in ordine alfabetico.
///
/// Senza offset né limite, al contrario di `album` e `brani`: gli artisti sono
/// pochi e la vista li mostra tutti con un indice alfabetico laterale. Il
/// giorno in cui non fosse più vero, il posto in cui aggiungere la pagina è
/// `list_artists`, non qui.
#[tauri::command]
pub fn artisti(stato: State<'_, Stato>) -> Esito<Vec<ArtistSummary>> {
    con_libreria(&stato, |libreria| list_artists(&libreria.connection)).map_err(errore)
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
pub fn preferito(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    id: i64,
    valore: bool,
) -> Esito<()> {
    let esito = con_libreria(&stato, |libreria| {
        let now = adesso_ms();
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
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Cambia la valutazione di un brano.
///
/// Come `preferito`, scrive `stats_updated_at`: è il timestamp della decisione,
/// e senza, un voto tolto qui tornerebbe indietro dal telefono alla prima
/// sincronia. Il voto non ha una colonna «quando» tutta sua perché non ne ha
/// bisogno — `merge` confronta l'istante delle statistiche, non quello del
/// singolo campo.
#[tauri::command]
pub fn valutazione(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    id: i64,
    stelle: i64,
) -> Esito<()> {
    // Tagliato qui, non lasciato arrivare al database. Lo schema dichiara
    // `CHECK (rating BETWEEN 0 AND 5)`: un sei arriverebbe alla finestra come
    // un errore di vincolo SQL, cioè come un guasto del programma, quando è
    // solo un valore da riportare in scala.
    let stelle = stelle.clamp(0, 5);
    let esito = con_libreria(&stato, |libreria| {
        let now = adesso_ms();
        libreria
            .connection
            .execute(
                "UPDATE tracks SET rating = ?2, stats_updated_at = ?3 WHERE id = ?1",
                rusqlite::params![id, stelle, now],
            )
            .map(|_| ())
            .map_err(|err| {
                AppError::new(aether_domain::errors::ErrorCode::DbQueryFailed {
                    detail: Some("valutazione".into()),
                })
                .with_cause(err.to_string())
            })
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
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
pub fn importa(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    percorso: String,
) -> Esito<import_legacy::ImportReport> {
    crate::nuvola::se_riuscito(&app, importa_interno(&stato, &percorso, true))
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
