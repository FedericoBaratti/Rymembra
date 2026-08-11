//! I comandi delle playlist.
//!
//! Involucri, come quelli di [`crate::comandi`]: ogni funzione traduce gli
//! argomenti, chiama `aether_app::playlists` e traduce l'errore. Nessuna
//! decisione — né su cosa sia un nome valido, né su cosa succeda a una playlist
//! automatica, né su come si riscrivano le posizioni. Sono tutte cose che
//! Android dovrà fare identiche, e una regola scritta qui sarebbe una regola da
//! riscrivere di là.
//!
//! Sta in un file suo e non in `comandi.rs` perché quello è già il più lungo, e
//! perché queste otto funzioni hanno un solo argomento in comune: la playlist.

use std::path::{Path, PathBuf};

use aether_app::import_playlist::{self, PlaylistFileReport};
use aether_app::library::TrackSummary;
use aether_app::playlists::{self, PlaylistSummary};
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::playlist_file::{FormatoPlaylist, PlaylistLetta};
use aether_domain::regole::{Campo, Combinazione, Insieme, Operatore, Ordinamento, Regola, Valore};

use crate::errore::{Esito, errore};
use crate::stato::{Stato, adesso_ms, con_libreria};
use tauri::State;

/// Tutte le playlist, con i loro numeri.
#[tauri::command]
pub fn playlist_elenco(stato: State<'_, Stato>) -> Esito<Vec<PlaylistSummary>> {
    con_libreria(&stato, |libreria| playlists::list(&libreria.connection)).map_err(errore)
}

/// I brani di una playlist, nell'ordine in cui stanno.
#[tauri::command]
pub fn playlist_brani(stato: State<'_, Stato>, id: i64) -> Esito<Vec<TrackSummary>> {
    con_libreria(&stato, |libreria| {
        playlists::tracks(&libreria.connection, id)
    })
    .map_err(errore)
}

/// Crea una playlist vuota.
#[tauri::command]
pub fn playlist_crea(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    nome: String,
) -> Esito<PlaylistSummary> {
    let esito = con_libreria(&stato, |libreria| {
        playlists::create(&libreria.connection, &nome)
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Rinomina una playlist.
#[tauri::command]
pub fn playlist_rinomina(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    id: i64,
    nome: String,
) -> Esito<PlaylistSummary> {
    let esito = con_libreria(&stato, |libreria| {
        playlists::rename(&libreria.connection, id, &nome)
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Cancella una playlist.
#[tauri::command]
pub fn playlist_cancella(app: tauri::AppHandle, stato: State<'_, Stato>, id: i64) -> Esito<()> {
    let esito = con_libreria(&stato, |libreria| {
        playlists::delete(&mut libreria.connection, id)
    })
    .map_err(errore);
    // La lapide che `delete` lascia deve arrivare su Drive: senza, l'altro
    // dispositivo vedrebbe solo «a me manca una playlist» e la rimanderebbe
    // indietro alla prima passata.
    crate::nuvola::se_riuscito(&app, esito)
}

/// Aggiunge brani in fondo a una playlist.
#[tauri::command]
pub fn playlist_aggiungi(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    id: i64,
    brani: Vec<i64>,
) -> Esito<PlaylistSummary> {
    let esito = con_libreria(&stato, |libreria| {
        playlists::add_tracks(&mut libreria.connection, id, &brani)
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Toglie il brano che sta in una posizione.
#[tauri::command]
pub fn playlist_togli(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    id: i64,
    posizione: i64,
) -> Esito<PlaylistSummary> {
    let esito = con_libreria(&stato, |libreria| {
        playlists::remove_at(&mut libreria.connection, id, posizione)
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Sposta un brano dentro una playlist.
#[tauri::command]
pub fn playlist_riordina(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    id: i64,
    da: i64,
    a: i64,
) -> Esito<PlaylistSummary> {
    let esito = con_libreria(&stato, |libreria| {
        playlists::reorder(&mut libreria.connection, id, da, a)
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

// ── le regole, dal lato della finestra ──────────────────────────────────────

/// Una regola, come arriva dalla finestra.
///
/// Un tipo IPC a parte e non `aether_domain::regole::Regola` direttamente: là
/// il valore è un `enum` a tre varianti, e un `enum` con dati attraverso serde
/// diventa un oggetto etichettato che il TypeScript deve costruire a mano. Qui
/// è quel che una schermata produce davvero — tre campi, il terzo di due tipi
/// possibili — e la traduzione sta in un posto solo.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegolaIpc {
    /// Il nome del campo: `artista`, `anno`, `riproduzioni`…
    pub campo: String,
    /// Il nome dell'operatore: `contiene`, `maggiore`, `negliUltimi`…
    pub operatore: String,
    /// Il valore, quando l'operatore ne vuole uno di testo.
    #[serde(default)]
    pub testo: Option<String>,
    /// Il valore, quando l'operatore ne vuole uno numerico.
    #[serde(default)]
    pub numero: Option<i64>,
}

/// L'insieme delle regole, come arriva dalla finestra.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InsiemeIpc {
    /// `tutte` o `qualsiasi`.
    pub combinazione: String,
    /// Le condizioni.
    pub regole: Vec<RegolaIpc>,
    /// Quanti brani al massimo.
    #[serde(default)]
    pub limite: Option<u32>,
    /// `scaffale`, `piuAscoltati`, `menoAscoltati`, `recenti`, `casuale`.
    pub ordinamento: String,
}

impl InsiemeIpc {
    /// Traduce nell'insieme del dominio.
    ///
    /// Quel che non si riconosce **si salta**, e non è indulgenza: le regole
    /// arrivano da menù a tendina, e cambiare il primo lascia il secondo
    /// incoerente per un istante. `Regola::valida` scarterebbe comunque quelle
    /// storte; qui si scartano anche i nomi sconosciuti, che vogliono dire una
    /// finestra più nuova del nucleo — cosa che in sviluppo capita a ogni
    /// ricarica a caldo.
    fn nel_dominio(&self) -> Insieme {
        Insieme {
            combinazione: Combinazione::da_testo(&self.combinazione).unwrap_or_default(),
            regole: self
                .regole
                .iter()
                .filter_map(|r| {
                    Some(Regola {
                        campo: Campo::da_testo(&r.campo)?,
                        operatore: Operatore::da_testo(&r.operatore)?,
                        valore: match (&r.testo, r.numero) {
                            (Some(t), _) => Valore::Testo(t.clone()),
                            (None, Some(n)) => Valore::Numero(n),
                            (None, None) => Valore::Nessuno,
                        },
                    })
                })
                .collect(),
            limite: self.limite,
            ordinamento: Ordinamento::da_testo(&self.ordinamento).unwrap_or_default(),
        }
    }

    /// Il contrario, per riaprire nell'editor delle regole già salvate.
    fn dal_dominio(insieme: &Insieme) -> Self {
        Self {
            combinazione: insieme.combinazione.come_testo().to_owned(),
            regole: insieme
                .regole
                .iter()
                .map(|r| RegolaIpc {
                    campo: r.campo.come_testo().to_owned(),
                    operatore: r.operatore.come_testo().to_owned(),
                    testo: match &r.valore {
                        Valore::Testo(t) => Some(t.clone()),
                        _ => None,
                    },
                    numero: match &r.valore {
                        Valore::Numero(n) => Some(*n),
                        _ => None,
                    },
                })
                .collect(),
            limite: insieme.limite,
            ordinamento: insieme.ordinamento.come_testo().to_owned(),
        }
    }
}

/// Crea una playlist intelligente.
#[tauri::command]
pub fn playlist_crea_smart(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    nome: String,
    regole: InsiemeIpc,
) -> Esito<PlaylistSummary> {
    let insieme = regole.nel_dominio();
    let esito = con_libreria(&stato, |libreria| {
        playlists::create_smart(&libreria.connection, &nome, &insieme)
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Riscrive le regole di una playlist intelligente.
#[tauri::command]
pub fn playlist_regole_scrivi(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    id: i64,
    regole: InsiemeIpc,
) -> Esito<PlaylistSummary> {
    let insieme = regole.nel_dominio();
    let esito = con_libreria(&stato, |libreria| {
        playlists::set_rules(&libreria.connection, id, &insieme)
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Le regole di una playlist, per riaprirle nell'editor.
///
/// `None` se la playlist non è intelligente.
#[tauri::command]
pub fn playlist_regole(stato: State<'_, Stato>, id: i64) -> Esito<Option<InsiemeIpc>> {
    con_libreria(&stato, |libreria| {
        Ok(playlists::regole_di(&libreria.connection, id)?
            .as_ref()
            .map(InsiemeIpc::dal_dominio))
    })
    .map_err(errore)
}

/// Quanti brani mostra l'anteprima dell'editor delle regole.
const ANTEPRIMA: u32 = 20;

/// Cosa prenderebbero delle regole non ancora salvate.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnteprimaRegole {
    /// Quanti brani in tutto.
    pub quanti: i64,
    /// Quante regole non stanno in piedi e sono state saltate.
    pub scartate: usize,
    /// Nessuna condizione: prende tutta la libreria.
    pub prende_tutto: bool,
    /// Un «oppure» senza condizioni: non prende niente, mai.
    pub prende_niente: bool,
    /// I primi, per far vedere che sono quelli giusti.
    pub primi: Vec<TrackSummary>,
}

/// Quanti brani prenderebbero queste regole, senza salvarle.
///
/// È l'anteprima dell'editor: dice «142 brani» mentre si scrive la condizione,
/// che è l'unico modo di accorgersi di aver scritto «anno maggiore di 2050»
/// prima di salvare una playlist vuota sotto un nome che promette il contrario.
#[tauri::command]
pub fn playlist_regole_prova(
    stato: State<'_, Stato>,
    regole: InsiemeIpc,
) -> Esito<AnteprimaRegole> {
    let insieme = regole.nel_dominio();
    con_libreria(&stato, |libreria| {
        let adesso = adesso_ms();
        Ok(AnteprimaRegole {
            quanti: aether_app::smart::quanti(&libreria.connection, &insieme, adesso)?,
            scartate: insieme.scartate(),
            prende_tutto: insieme.prende_tutto(),
            prende_niente: insieme.prende_niente(),
            primi: aether_app::smart::brani(
                &libreria.connection,
                &Insieme {
                    limite: Some(ANTEPRIMA),
                    ..insieme.clone()
                },
                adesso,
            )?,
        })
    })
    .map_err(errore)
}

// ── i file di playlist ──────────────────────────────────────────────────────

/// Legge un file di playlist e dice cosa porterebbe dentro, senza scrivere.
///
/// `(async)`: legge dal disco. Un M3U è piccolo, ma la regola vale per tutti i
/// comandi che toccano il filesystem — e la finestra non deve fermarsi perché
/// qualcuno ha scelto un file su una chiavetta lenta.
#[tauri::command(async)]
pub fn playlist_file_piano(stato: State<'_, Stato>, percorso: String) -> Esito<PianoFile> {
    let (letta, nome, cartella) = leggi_file(&percorso).map_err(errore)?;
    let rapporto = con_libreria(&stato, |libreria| {
        import_playlist::plan(&mut libreria.connection, &letta, &nome, cartella.as_deref())
    })
    .map_err(errore)?;
    Ok(PianoFile { nome, rapporto })
}

/// Il piano, più il nome di serie che la finestra propone nel campo.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PianoFile {
    /// Come si chiamerebbe: il titolo dentro il file, o il nome del file.
    pub nome: String,
    /// Cosa porterebbe dentro.
    pub rapporto: PlaylistFileReport,
}

/// Importa davvero un file di playlist.
#[tauri::command(async)]
pub fn playlist_file_importa(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    percorso: String,
    nome: String,
) -> Esito<PlaylistFileReport> {
    let (letta, predefinito, cartella) = leggi_file(&percorso).map_err(errore)?;
    let nome = if nome.trim().is_empty() {
        predefinito
    } else {
        nome
    };
    let esito = con_libreria(&stato, |libreria| {
        import_playlist::import(&mut libreria.connection, &letta, &nome, cartella.as_deref())
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Apre un file di playlist: quel che conteneva, il nome di serie, e dove sta.
///
/// Il nome di serie è quello dichiarato **dentro** il file se c'è — solo XSPF
/// ne ha uno vero — altrimenti quello del file senza estensione. In
/// quest'ordine perché un titolo scritto da chi ha esportato la playlist dice
/// di più di «playlist_export_final_2».
fn leggi_file(percorso: &str) -> Result<(PlaylistLetta, String, Option<PathBuf>), AppError> {
    let path = PathBuf::from(percorso);
    let estensione = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let formato = FormatoPlaylist::da_estensione(&estensione).ok_or_else(|| {
        AppError::new(ErrorCode::LibraryPlaylistNameInvalid {
            name: percorso.to_owned(),
        })
        .with_cause("non è un file di playlist: servono .m3u, .m3u8, .pls o .xspf")
    })?;
    let byte = std::fs::read(&path)
        .map_err(|err| aether_app::files::io_error(&path.display().to_string(), &err))?;
    let letta = aether_domain::playlist_file::leggi(&byte, formato);
    let dal_file = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Playlist".to_owned());
    let nome = letta.nome.clone().unwrap_or(dal_file);
    Ok((letta, nome, path.parent().map(Path::to_path_buf)))
}

/// Scrive una playlist in un file.
///
/// Il formato lo decide **l'estensione del file scelto**: chi salva come
/// `serata.pls` vuole un PLS, e chiederglielo una seconda volta in una tendina
/// accanto sarebbe la stessa domanda due volte con due risposte possibili.
/// Un'estensione che non riconosciamo vale M3U, che è quello che ogni lettore
/// al mondo apre.
#[tauri::command(async)]
pub fn playlist_esporta(stato: State<'_, Stato>, id: i64, percorso: String) -> Esito<usize> {
    let path = PathBuf::from(&percorso);
    let estensione = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let formato = FormatoPlaylist::da_estensione(&estensione).unwrap_or(FormatoPlaylist::M3u);
    let (testo, quanti) = con_libreria(&stato, |libreria| {
        let testo = import_playlist::esporta(&libreria.connection, id, formato)?;
        let quanti = playlists::tracks(&libreria.connection, id)?.len();
        Ok((testo, quanti))
    })
    .map_err(errore)?;
    std::fs::write(&path, testo.as_bytes())
        .map_err(|err| errore(aether_app::files::io_error(&percorso, &err)))?;
    Ok(quanti)
}
