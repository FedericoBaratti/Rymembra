//! I comandi delle playlist.
//!
//! Involucri, come quelli di [`crate::comandi`]: ogni funzione traduce gli
//! argomenti, chiama `rymembra_app::playlists` e traduce l'errore. Nessuna
//! decisione — né su cosa sia un nome valido, né su cosa succeda a una playlist
//! automatica, né su come si riscrivano le posizioni. Sono tutte cose che
//! Android dovrà fare identiche, e una regola scritta qui sarebbe una regola da
//! riscrivere di là.
//!
//! Sta in un file suo e non in `comandi.rs` perché quello è già il più lungo, e
//! perché queste otto funzioni hanno un solo argomento in comune: la playlist.

use std::path::{Path, PathBuf};

use rymembra_app::import_playlist::{self, PlaylistFileReport};
use rymembra_app::library::TrackSummary;
use rymembra_app::playlists::{self, PlaylistSummary};
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::playlist_file::{FormatoPlaylist, PlaylistLetta};
use rymembra_domain::regole::{
    Campo, Combinazione, Insieme, Operatore, Ordinamento, Regola, Valore,
};

use crate::destinazioni;
use crate::disparte::{in_disparte, leggi_la_libreria};
use crate::errore::{Esito, errore};
use crate::importa::gestito;
use crate::stato::{Stato, adesso_ms, con_libreria};
use tauri::State;

/// Tutte le playlist, con i loro numeri.
///
/// In disparte, come le altre letture lunghe — vedi [`crate::disparte`]: i
/// numeri delle intelligenti sono un'interrogazione ciascuna su tutta la
/// libreria, la prima volta dopo ogni scrittura dei brani. Le volte dopo li
/// ricorda `smart::conteggi_ricordati`.
#[tauri::command]
pub async fn playlist_elenco(app: tauri::AppHandle) -> Esito<Vec<PlaylistSummary>> {
    leggi_la_libreria(app, "elenco delle playlist", |libreria| {
        playlists::list(&libreria.connection)
    })
    .await
    .map_err(errore)
}

/// I brani di una playlist, nell'ordine in cui stanno.
///
/// In disparte: per un'intelligente è il motore delle regole su tutta la
/// libreria, fino a cinquemila righe.
#[tauri::command]
pub async fn playlist_brani(app: tauri::AppHandle, id: i64) -> Esito<Vec<TrackSummary>> {
    leggi_la_libreria(app, "brani di una playlist", move |libreria| {
        playlists::tracks(&libreria.connection, id)
    })
    .await
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

/// Le playlist appena cancellate, finché un «Annulla» le può ancora rimettere.
///
/// # Perché qui, e perché poche
///
/// La cancellazione è già scritta quando l'avviso compare — vedi
/// `rymembra_app::playlists::Cancellata` per il perché — e quel che serve a
/// tornare indietro sta in memoria, con un gettone che la finestra ripassa. È
/// la forma delle rimozioni in attesa di `comandi.rs`: una statica e non uno
/// stato gestito, perché è un posto solo per un processo solo, che nessuno deve
/// registrare all'avvio per poterlo usare.
///
/// Se ne tengono poche e per poco: l'avviso dura dieci secondi, e una
/// fotografia più vecchia di [`DURATA_ANNULLO`] non la chiede più nessuno. Si
/// puliscono a ogni cancellazione, non con un orologio: un filo che si sveglia
/// per buttare via qualche chilobyte costerebbe più di quel che libera.
static CANCELLATE: std::sync::Mutex<Annullabili<playlists::Cancellata>> =
    std::sync::Mutex::new(Annullabili::nuovo());

/// Una cancellazione annullabile: il gettone, quando, e la fotografia.
type Tenuta<T> = (u64, std::time::Instant, T);

/// Le cancellazioni annullabili: scadenza, tetto, gettone e ritorno al posto.
///
/// Un tipo a parte, generico sulla fotografia e con l'ora data da chi chiama,
/// perché è l'unica via di ritorno da una cancellazione già scritta (e la cui
/// lapide è già partita per Drive): le prove lo esercitano con `T = ()` e un
/// orologio scelto da loro, invece di aspettare un minuto vero.
struct Annullabili<T> {
    tenute: Vec<Tenuta<T>>,
}

impl<T> Annullabili<T> {
    /// Nessuna cancellazione tenuta.
    const fn nuovo() -> Self {
        Self { tenute: Vec::new() }
    }

    /// Tiene la fotografia di una cancellazione appena fatta. Le scadute si
    /// buttano adesso, e oltre il tetto se ne va la più vecchia: mai quella
    /// appena entrata, che è l'«Annulla» che l'utente ha sotto gli occhi.
    fn tieni(&mut self, gettone: u64, adesso: std::time::Instant, fotografia: T) {
        self.tenute
            .retain(|(_, quando, _)| adesso.saturating_duration_since(*quando) < DURATA_ANNULLO);
        if self.tenute.len() >= CANCELLATE_MASSIME {
            self.tenute.remove(0);
        }
        self.tenute.push((gettone, adesso, fotografia));
    }

    /// Toglie e restituisce la tenuta del gettone, se c'è e non è scaduta.
    fn prendi(&mut self, gettone: u64, adesso: std::time::Instant) -> Option<Tenuta<T>> {
        let dove = self.tenute.iter().position(|(segno, quando, _)| {
            *segno == gettone && adesso.saturating_duration_since(*quando) < DURATA_ANNULLO
        })?;
        Some(self.tenute.remove(dove))
    }

    /// Rimette al suo posto una tenuta presa, quando il ripristino è caduto:
    /// un secondo «Annulla» deve poterci riprovare, con la scadenza di prima.
    fn rimetti(&mut self, tenuta: Tenuta<T>) {
        self.tenute.push(tenuta);
    }
}

/// Il prossimo gettone. Parte da uno: lo zero non lo riceve nessuna cancellazione.
static GETTONI: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Quanto resta possibile annullare una cancellazione.
///
/// Un minuto: sei volte l'avviso della finestra. Il margine non è per chi
/// guarda — l'«Annulla» sparisce prima — ma per chi l'ha premuto mentre il
/// nucleo era occupato in una scrittura lunga.
const DURATA_ANNULLO: std::time::Duration = std::time::Duration::from_secs(60);

/// Quante fotografie si tengono al massimo.
const CANCELLATE_MASSIME: usize = 8;

/// Il posto delle cancellazioni annullabili, anche se un filo è caduto tenendolo.
///
/// Un mutex avvelenato vuol dire soltanto che un filo è caduto mentre ci
/// scriveva: le fotografie dentro sono intere, e rinunciarci vorrebbe dire
/// rinunciare a un «Annulla» per un guasto che non le ha toccate.
fn cancellate() -> std::sync::MutexGuard<'static, Annullabili<playlists::Cancellata>> {
    CANCELLATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Cancella una playlist, e restituisce il gettone per annullare.
///
/// In disparte: la cancellazione legge tutte le righe della playlist per
/// fotografarle, e su una playlist di migliaia di brani non è una query di
/// microsecondi.
#[tauri::command]
pub async fn playlist_cancella(app: tauri::AppHandle, id: i64) -> Esito<u64> {
    let esito = leggi_la_libreria(
        app.clone(),
        "cancellazione di una playlist",
        move |libreria| playlists::cancella(&mut libreria.connection, id),
    )
    .await
    .map(|fotografia| {
        let gettone = GETTONI.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        cancellate().tieni(gettone, std::time::Instant::now(), fotografia);
        gettone
    })
    .map_err(errore);
    // La lapide che `cancella` lascia deve arrivare su Drive: senza, l'altro
    // dispositivo vedrebbe solo «a me manca una playlist» e la rimanderebbe
    // indietro alla prima passata.
    crate::nuvola::se_riuscito(&app, esito)
}

/// Rimette la playlist cancellata col gettone dato.
///
/// `None` se il gettone non vale più — scaduto, già usato, o di un'altra
/// esecuzione dell'applicazione: non è un errore, è un «Annulla» arrivato
/// tardi, e la finestra lo dice con parole sue.
///
/// La fotografia si prende prima di scrivere e torna al suo posto se la
/// scrittura fallisce: un secondo «Annulla» deve poterci riprovare.
#[tauri::command]
pub async fn playlist_ripristina(
    app: tauri::AppHandle,
    gettone: u64,
) -> Esito<Option<PlaylistSummary>> {
    let presa = cancellate().prendi(gettone, std::time::Instant::now());
    let Some(tenuta) = presa else {
        return Ok(None);
    };
    let esito = leggi_la_libreria(app.clone(), "ripristino di una playlist", move |libreria| {
        let tornata = playlists::ripristina(&mut libreria.connection, &tenuta.2);
        if tornata.is_err() {
            cancellate().rimetti(tenuta);
        }
        tornata
    })
    .await
    .map(Some)
    .map_err(errore);
    // La playlist che torna è una modifica come le altre: Drive e l'altro
    // dispositivo devono vederla, con la sua data nuova.
    crate::nuvola::se_riuscito(&app, esito)
}

/// Aggiunge brani in fondo a una playlist, e dice quanti non c'erano più.
///
/// L'`Aggiunta` intera e non la sola playlist: «2 non c'erano più» è la frase
/// che distingue un'aggiunta riuscita a metà per una scansione nel frattempo da
/// un guasto, e senza il numero la finestra non la può dire.
#[tauri::command]
pub async fn playlist_aggiungi(
    app: tauri::AppHandle,
    id: i64,
    brani: Vec<i64>,
) -> Esito<playlists::Aggiunta> {
    // Una voce di sessione non è una riga: si rifiuta il gesto intero, invece
    // di farne metà. Vedi `riproduzione::ospiti::righe_di_libreria`.
    crate::riproduzione::righe_di_libreria(&brani).map_err(errore)?;
    let esito = leggi_la_libreria(app.clone(), "aggiunta a una playlist", move |libreria| {
        playlists::aggiungi_brani(&mut libreria.connection, id, &brani)
    })
    .await
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Crea una playlist e ci mette dentro dei brani, in una transazione sola.
///
/// Erano due comandi chiamati di seguito dalla finestra, e un'aggiunta fallita
/// lasciava una playlist vuota col nome appena scelto: vedi
/// `rymembra_app::playlists::crea_e_aggiungi`.
#[tauri::command]
pub async fn playlist_crea_e_aggiungi(
    app: tauri::AppHandle,
    nome: String,
    brani: Vec<i64>,
) -> Esito<playlists::Aggiunta> {
    // Una voce di sessione non è una riga: si rifiuta il gesto intero, invece
    // di farne metà. Vedi `riproduzione::ospiti::righe_di_libreria`.
    crate::riproduzione::righe_di_libreria(&brani).map_err(errore)?;
    let esito = leggi_la_libreria(
        app.clone(),
        "playlist nuova con i suoi brani",
        move |libreria| playlists::crea_e_aggiungi(&mut libreria.connection, &nome, &brani),
    )
    .await
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
/// Un tipo IPC a parte e non `rymembra_domain::regole::Regola` direttamente: là
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
    ///
    /// Il `match` sul valore è esaustivo apposta, senza `_ =>`: un genere di
    /// valore nuovo nel dominio (una data, un intervallo) non deve diventare
    /// in silenzio «nessun valore», che `nel_dominio` ricostruirebbe come
    /// `Valore::Nessuno` e `Regola::valida` scarterebbe. Basterebbe riaprire e
    /// risalvare una playlist intelligente per perderne una condizione. Così
    /// non compila finché qualcuno non decide come tradurlo, e la prova
    /// `le_regole_salvate_tornano_uguali_dall_editor` fissa l'andata e ritorno.
    fn dal_dominio(insieme: &Insieme) -> Self {
        Self {
            combinazione: insieme.combinazione.come_testo().to_owned(),
            regole: insieme
                .regole
                .iter()
                .map(|r| {
                    let (testo, numero) = match &r.valore {
                        Valore::Testo(t) => (Some(t.clone()), None),
                        Valore::Numero(n) => (None, Some(*n)),
                        Valore::Nessuno => (None, None),
                    };
                    RegolaIpc {
                        campo: r.campo.come_testo().to_owned(),
                        operatore: r.operatore.come_testo().to_owned(),
                        testo,
                        numero,
                    }
                })
                .collect(),
            limite: insieme.limite,
            ordinamento: insieme.ordinamento.come_testo().to_owned(),
        }
    }
}

/// Crea una playlist intelligente.
///
/// In disparte: la risposta porta i suoi numeri, cioè le regole nuove contate
/// su tutta la libreria.
#[tauri::command]
pub async fn playlist_crea_smart(
    app: tauri::AppHandle,
    nome: String,
    regole: InsiemeIpc,
) -> Esito<PlaylistSummary> {
    let insieme = regole.nel_dominio();
    let esito = leggi_la_libreria(
        app.clone(),
        "playlist intelligente nuova",
        move |libreria| playlists::create_smart(&libreria.connection, &nome, &insieme),
    )
    .await
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Riscrive le regole di una playlist intelligente.
///
/// In disparte per la stessa ragione di [`playlist_crea_smart`].
#[tauri::command]
pub async fn playlist_regole_scrivi(
    app: tauri::AppHandle,
    id: i64,
    regole: InsiemeIpc,
) -> Esito<PlaylistSummary> {
    let insieme = regole.nel_dominio();
    let esito = leggi_la_libreria(app.clone(), "regole di una playlist", move |libreria| {
        playlists::set_rules(&libreria.connection, id, &insieme)
    })
    .await
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
///
/// In disparte: si chiede a ogni tasto premuto nell'editor, e ogni volta conta
/// tutta la libreria senza limite.
#[tauri::command]
pub async fn playlist_regole_prova(
    app: tauri::AppHandle,
    regole: InsiemeIpc,
) -> Esito<AnteprimaRegole> {
    let insieme = regole.nel_dominio();
    leggi_la_libreria(app, "anteprima delle regole", move |libreria| {
        let adesso = adesso_ms();
        Ok(AnteprimaRegole {
            quanti: rymembra_app::smart::quanti(&libreria.connection, &insieme, adesso)?,
            scartate: insieme.scartate(),
            prende_tutto: insieme.prende_tutto(),
            prende_niente: insieme.prende_niente(),
            primi: rymembra_app::smart::brani(
                &libreria.connection,
                &Insieme {
                    limite: Some(ANTEPRIMA),
                    ..insieme.clone()
                },
                adesso,
            )?,
        })
    })
    .await
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
///
/// In disparte, sul pool bloccante: legge un file che l'utente ha scelto — da
/// una chiavetta, da una share — e poi scrive una playlist intera. Un
/// `(async)` su una funzione sincrona faceva aspettare quel disco a un
/// *worker* del runtime, togliendo il posto a ogni altro comando asincrono.
#[tauri::command]
pub async fn playlist_file_importa(
    app: tauri::AppHandle,
    percorso: String,
    nome: String,
) -> Esito<PlaylistFileReport> {
    in_disparte("importazione di un file di playlist", move || {
        let stato =
            gestito::<Stato>(&app, "importazione di un file di playlist").map_err(errore)?;
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
    })
    .await
    .map_err(errore)?
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
        .map_err(|err| rymembra_app::files::io_error(&path.display().to_string(), &err))?;
    let letta = rymembra_domain::playlist_file::leggi(&byte, formato);
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
///
/// Un'estensione che non è di una playlist si **rifiuta**. Fino alla 2.4 valeva
/// M3U, che sembrava generoso e voleva dire che questo comando scriveva testo
/// sotto qualunque nome la pagina gli passasse — `serata.txt` come
/// `qualcosa.dll`. Le regole, e perché non si aggiusta il nome, stanno in
/// [`crate::destinazioni`].
///
/// # Errori
///
/// `fs.extensionRefused` o `fs.pathInvalid` per un percorso che la finestra non
/// può chiedere; quelli della libreria e della scrittura per il resto.
#[tauri::command(async)]
pub fn playlist_esporta(
    stato: State<'_, Stato>,
    id: i64,
    percorso: String,
) -> Esito<PlaylistEsportata> {
    let file = destinazioni::file_da_scrivere(&percorso, destinazioni::ESTENSIONI_PLAYLIST)
        .map_err(errore)?;
    // Non succede: `ESTENSIONI_PLAYLIST` e `da_estensione` sono tenute allineate
    // da un test in `destinazioni`. Ma se un giorno divergessero, il rifiuto è
    // la risposta giusta — non un ritorno silenzioso a M3U.
    let formato = FormatoPlaylist::da_estensione(file.estensione).ok_or_else(|| {
        errore(AppError::new(ErrorCode::FsExtensionRefused {
            path: percorso.clone(),
            allowed: destinazioni::ESTENSIONI_PLAYLIST
                .iter()
                .map(|&e| e.to_owned())
                .collect(),
        }))
    })?;
    let path = file.percorso;
    let fatta = con_libreria(&stato, |libreria| {
        import_playlist::esporta(&libreria.connection, id, formato)
    })
    .map_err(errore)?;
    std::fs::write(&path, fatta.testo.as_bytes())
        .map_err(|err| errore(rymembra_app::files::io_error(&percorso, &err)))?;
    Ok(PlaylistEsportata {
        scritti: fatta.scritti,
        lasciati_fuori: fatta.lasciati_fuori,
    })
}

/// Com'è andata un'esportazione: quanti brani sono nel file e quanti ne sono
/// rimasti fuori.
///
/// I due numeri vengono da chi scrive il file (`import_playlist::esporta`), e
/// non dalla playlist: prima la finestra diceva i brani della playlist, e una
/// playlist con dieci brani del cloud su trenta usciva come «30 brani scritti»
/// in un file che ne aveva venti. Una playlist tutta del cloud si scrive lo
/// stesso — un file vuoto è un'esportazione legittima — e lo dice: «0 brani».
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEsportata {
    /// I brani scritti nel file: i brani, non le voci (una fila di brani dello
    /// stesso `.cue` è una voce sola).
    pub scritti: usize,
    /// I brani del cloud o di un catalogo, che un file di playlist non può
    /// nominare.
    pub lasciati_fuori: usize,
}

#[cfg(test)]
mod prove {
    use super::*;
    use std::time::{Duration, Instant};

    // ── l'«Annulla» delle cancellazioni ─────────────────────────────────────

    #[test]
    fn il_nono_annullo_butta_il_piu_vecchio_e_tiene_l_ultimo() {
        let adesso = Instant::now();
        let mut tenute = Annullabili::nuovo();
        for gettone in 1..=9 {
            tenute.tieni(gettone, adesso, ());
        }
        assert!(
            tenute.prendi(1, adesso).is_none(),
            "oltre il tetto se ne va la più vecchia"
        );
        assert!(
            tenute.prendi(9, adesso).is_some(),
            "l'«Annulla» appena comparso vale sempre"
        );
        assert!(tenute.prendi(2, adesso).is_some());
    }

    #[test]
    fn un_annullo_dopo_un_minuto_non_vale() {
        let allora = Instant::now();
        let mut tenute = Annullabili::nuovo();
        tenute.tieni(7, allora, ());
        assert!(tenute.prendi(7, allora + Duration::from_secs(61)).is_none());
        // Arrivato tardi non la consuma: dentro il minuto si ritrova.
        assert!(tenute.prendi(7, allora + Duration::from_secs(59)).is_some());
        // E una cancellazione nuova butta via le scadute.
        tenute.tieni(8, allora, ());
        tenute.tieni(9, allora + Duration::from_secs(61), ());
        assert!(tenute.prendi(8, allora + Duration::from_secs(30)).is_none());
        assert!(tenute.prendi(9, allora + Duration::from_secs(61)).is_some());
    }

    #[test]
    fn un_ripristino_fallito_si_puo_ripetere() {
        let adesso = Instant::now();
        let mut tenute = Annullabili::nuovo();
        tenute.tieni(3, adesso, "fotografia");
        let presa = tenute.prendi(3, adesso).expect("la prima volta");
        // Il ripristino è caduto: la fotografia torna al suo posto.
        tenute.rimetti(presa);
        let ripresa = tenute.prendi(3, adesso).expect("il secondo «Annulla»");
        assert_eq!(ripresa.2, "fotografia");
        // Con la sua ora di prima, non con quella del guasto.
        assert_eq!(ripresa.1, adesso);
    }

    #[test]
    fn un_gettone_si_usa_una_volta() {
        let adesso = Instant::now();
        let mut tenute = Annullabili::nuovo();
        tenute.tieni(4, adesso, ());
        tenute.tieni(5, adesso, ());
        assert!(tenute.prendi(4, adesso).is_some());
        assert!(tenute.prendi(4, adesso).is_none());
        assert!(tenute.prendi(5, adesso).is_some(), "l'altra resta");
        assert!(tenute.prendi(0, adesso).is_none());
    }

    // ── le regole intelligenti, dall'editor e ritorno ───────────────────────

    /// Un valore che `Regola::valida` accetta per quel campo e operatore.
    fn valore_per(campo: Campo, operatore: Operatore) -> Valore {
        use rymembra_domain::regole::GenereCampo;
        if !operatore.vuole_valore() {
            return Valore::Nessuno;
        }
        match campo.genere() {
            GenereCampo::Testo => Valore::Testo(format!("{} di prova", campo.come_testo())),
            GenereCampo::Data => Valore::Numero(30),
            GenereCampo::Booleano => Valore::Numero(1),
            GenereCampo::Numero => Valore::Numero(-7),
        }
    }

    #[test]
    fn le_regole_salvate_tornano_uguali_dall_editor() {
        let mut regole = Vec::new();
        for &campo in Campo::tutti() {
            for &operatore in Operatore::per(campo.genere()) {
                let regola = Regola {
                    campo,
                    operatore,
                    valore: valore_per(campo, operatore),
                };
                assert!(regola.valida(), "{regola:?}");
                regole.push(regola);
            }
        }
        for (combinazione, limite, ordinamento) in [
            (Combinazione::Qualsiasi, Some(25), Ordinamento::PiuAscoltati),
            (Combinazione::Tutte, None, Ordinamento::Scaffale),
            (Combinazione::Tutte, Some(1), Ordinamento::MenoAscoltati),
            (Combinazione::Qualsiasi, None, Ordinamento::Recenti),
            (Combinazione::Tutte, Some(5000), Ordinamento::Casuale),
        ] {
            let insieme = Insieme {
                combinazione,
                regole: regole.clone(),
                limite,
                ordinamento,
            };
            let tornato = InsiemeIpc::dal_dominio(&insieme).nel_dominio();
            assert_eq!(tornato, insieme);
        }
    }
}
