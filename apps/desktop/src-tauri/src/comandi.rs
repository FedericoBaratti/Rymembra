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
    counts, list_albums, list_artists, list_tracks, recently_added_albums, search,
};
use aether_app::settings::CHIAVE_CARTELLE;
use aether_domain::errors::{AppError, ErrorCode};
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
    /// Dove finiscono i brani scaricati, se l'utente l'ha scelto.
    ///
    /// `None` vuol dire «la prima cartella sorvegliata», che è quel che
    /// `scarica::cartella_download` fa davvero. Non si risolve qui il valore di
    /// serie: la finestra deve poter distinguere una scelta esplicita da un
    /// ripiego, se non altro per sapere se offrire «rimetti quella di serie».
    pub cartella_download: Option<String>,
    /// I numeri della libreria.
    pub numeri: Counts,
    /// Il tema scelto: `scuro`, `chiaro`, `sistema`.
    ///
    /// `None` vuol dire **mai scelto**, ed è diverso da «sistema»: la finestra
    /// deve poter distinguere le due cose per sapere se ripiegare sulla
    /// preferenza rimasta in `localStorage` prima che il tema tornasse nel
    /// nucleo. Vedi `aether_app::preferenze`.
    pub tema: Option<String>,
    /// La lingua scelta, come codice ISO. `None` vuol dire **mai scelta**.
    ///
    /// E «mai scelta» è ciò che fa rilevare la lingua dal sistema operativo:
    /// risolverla qui darebbe alla finestra una lingua senza modo di sapere se
    /// qualcuno l'ha voluta. Il codice non è controllato contro un elenco perché
    /// l'elenco è la cartella `src/lingue/`, che il nucleo non conosce e non
    /// deve conoscere.
    pub lingua: Option<String>,
    /// Le scorciatoie riscritte dall'utente, come JSON. `None` = quelle di serie.
    ///
    /// Grezze e non interpretate: i nomi dei comandi appartengono alla finestra,
    /// e un nucleo che li capisse andrebbe ricompilato per aggiungere una
    /// scorciatoia.
    pub scorciatoie: Option<String>,
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
            // Il `filter` regge le righe vuote scritte prima che
            // `imposta_cartella_download` cancellasse invece di svuotare.
            cartella_download: aether_app::settings::read(
                &libreria.connection,
                aether_app::settings::CHIAVE_CARTELLA_DOWNLOAD,
            )?
            .filter(|scelta| !scelta.trim().is_empty()),
            numeri: counts(&libreria.connection)?,
            tema: aether_app::preferenze::tema(&libreria.connection)?.map(|t| t.nome().to_owned()),
            lingua: aether_app::preferenze::lingua(&libreria.connection)?,
            scorciatoie: aether_app::preferenze::scorciatoie(&libreria.connection)?,
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

/// Sceglie dove finiscono i brani scaricati. Una stringa vuota rimette il
/// valore di serie, cioè la prima cartella sorvegliata.
///
/// # La cartella si può scegliere fuori dalle sorvegliate, e la finestra lo dice
///
/// Sarebbe stato più semplice rifiutarla. Ma «sorvegliata» è uno stato che
/// cambia — si toglie una cartella e la scelta di ieri diventa illegale — e un
/// comando che fallisce su un valore già scritto è un comando che si rompe da
/// solo. Qui si scrive quel che l'utente chiede; è la scheda delle impostazioni
/// che, accanto al percorso scelto, avvisa quando nessuna scansione passerà mai
/// di lì. Vale anche il caso opposto e più comune: la cartella la si sceglie
/// *prima* di sorvegliarla.
///
/// Il valore di serie non si scrive: si **toglie** la riga. Una stringa vuota
/// in tabella è un terzo stato oltre «scelta» e «mai scelta», e
/// `scarica::cartella_download` dovrebbe ricordarsi di filtrarlo — cosa che fa,
/// ma che nessuno dovrebbe dover fare.
#[tauri::command]
pub fn imposta_cartella_download(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    percorso: String,
) -> Esito<()> {
    crate::nuvola::se_riuscito(
        &app,
        con_libreria(&stato, |libreria| {
            let scelta = percorso.trim();
            if scelta.is_empty() {
                aether_app::settings::forget(
                    &libreria.connection,
                    aether_app::settings::CHIAVE_CARTELLA_DOWNLOAD,
                )
                .map(|_| ())
            } else {
                aether_app::settings::write(
                    &libreria.connection,
                    aether_app::settings::CHIAVE_CARTELLA_DOWNLOAD,
                    scelta,
                )
            }
        })
        .map_err(errore),
    )
}

/// Una pagina di cronologia d'ascolto, dal più recente.
#[tauri::command]
pub fn cronologia(
    stato: State<'_, Stato>,
    offset: i64,
    limite: i64,
) -> Esito<Vec<aether_app::library::VoceCronologia>> {
    con_libreria(&stato, |libreria| {
        aether_app::library::list_history(&libreria.connection, offset, limite)
    })
    .map_err(errore)
}

/// Quanti ascolti ci sono in tutto.
///
/// Separato dalla pagina per la stessa ragione di [`cerca_conteggio`]: la pagina
/// si richiede a ogni scorrimento, il totale una volta sola.
#[tauri::command]
pub fn cronologia_conteggio(stato: State<'_, Stato>) -> Esito<i64> {
    con_libreria(&stato, |libreria| {
        aether_app::library::count_history(&libreria.connection)
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
    /// File che non si sono potuti leggere.
    ///
    /// Entrano comunque in libreria, marcati «degradato»: qui c'è il numero, e
    /// in [`EsitoScansione::illeggibili_quali`] ci sono i percorsi.
    pub illeggibili: usize,
    /// Quali, fino a [`QUANTI_DETTAGLI`].
    ///
    /// # Perché adesso e non prima
    ///
    /// Perché fin qui questa struttura portava **solo** il numero: l'utente
    /// leggeva «40 illeggibili» e non aveva modo, da dentro l'applicazione, di
    /// sapere quali. Il nucleo li aveva tutti — `ScanReport::unreadable` porta
    /// percorso e codice d'errore per ognuno — e si fermavano qui.
    pub illeggibili_quali: Vec<FileSaltato>,
    /// Copertine che non si sono potute salvare.
    ///
    /// A parte dagli illeggibili perché la reazione è diversa: un'immagine rotta
    /// riguarda quel brano, un disco pieno riguarda tutta la scansione.
    pub copertine_fallite: Vec<FileSaltato>,
    /// File che il piano ha lasciato fuori, e perché.
    ///
    /// `SkipReason::as_str` è documentato «per l'interfaccia» dal giorno in cui
    /// è stato scritto, e nessuna interfaccia lo leggeva.
    pub saltati: Vec<FileSaltato>,
    /// Copertine ricodificate ora.
    pub copertine_nuove: usize,
    /// Quanto è durata, in millisecondi.
    pub durata_ms: u128,
    /// È stata fermata a metà.
    pub annullata: bool,
    /// Le cartelle che non hanno risposto, e che quindi non sono state guardate.
    ///
    /// # Perché arriva fino a qui
    ///
    /// Perché senza, una scansione fatta con il NAS spento è indistinguibile da
    /// una fatta con il NAS acceso e la cartella davvero vuota: in tutti e due i
    /// casi la finestra scrive «completata». Il nucleo la differenza la sa —
    /// `ScanReport::radici_saltate` è documentato «va mostrato» dal giorno in cui
    /// è stato scritto — e si fermava qui.
    pub radici_saltate: Vec<String>,
    /// Righe che il piano toglierebbe e che la guardia ha lasciato stare.
    ///
    /// Zero nelle scansioni chieste a mano, che non sono prudenti apposta: chi
    /// le ha chieste è davanti alla finestra e legge l'esito.
    pub rimozioni_rinviate: usize,
    /// I numeri della libreria dopo.
    pub numeri: Counts,
}

/// Un file che non è entrato come ci si aspettava, e il perché.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSaltato {
    /// Il percorso.
    pub percorso: String,
    /// Il motivo: un codice del catalogo, o un nome di `SkipReason`.
    pub motivo: String,
}

/// Quanti percorsi si mandano alla finestra, per elenco.
///
/// Cinquanta. Una prima scansione su una cartella di scaricati può produrne
/// migliaia, e mandarli tutti vorrebbe dire spedire qualche megabyte di stringhe
/// per riempire una lista che nessuno scorre fino in fondo. Il numero intero
/// resta in `illeggibili`, quindi l'interfaccia può dire «e altri 1.312».
pub const QUANTI_DETTAGLI: usize = 50;

impl EsitoScansione {
    fn da(report: &ScanReport, numeri: Counts) -> Self {
        let quali = |elenco: &[aether_app::library::Unreadable]| -> Vec<FileSaltato> {
            elenco
                .iter()
                .take(QUANTI_DETTAGLI)
                .map(|guasto| FileSaltato {
                    percorso: guasto.path.clone(),
                    motivo: guasto.error.code().kind().code().to_owned(),
                })
                .collect()
        };
        Self {
            inseriti: report.inserted,
            aggiornati: report.updated,
            spostati: report.moved,
            tolti: report.removed,
            illeggibili: report.unreadable.len(),
            illeggibili_quali: quali(&report.unreadable),
            copertine_fallite: quali(&report.cover_failures),
            saltati: report
                .plan
                .skipped
                .iter()
                .take(QUANTI_DETTAGLI)
                .map(|salto| FileSaltato {
                    percorso: salto.file.path.clone(),
                    motivo: salto.reason.as_str().to_owned(),
                })
                .collect(),
            copertine_nuove: report.covers_stored,
            durata_ms: report.elapsed_ms,
            annullata: report.cancelled,
            // Per intero e non i primi `QUANTI_DETTAGLI`: le cartelle sorvegliate
            // sono una manciata, non un file per brano, e troncarle vorrebbe dire
            // non dire quale ricollegare.
            radici_saltate: report.radici_saltate.clone(),
            rimozioni_rinviate: report.rimozioni_rinviate,
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
///
/// `(async)`, e non per eleganza: un comando normale gira sul filo principale
/// (vedi la nota in `nuvola`), e venti secondi lì sopra congelano la webview —
/// gli eventi di avanzamento partono ma nessuno li disegna, e il comando
/// `annulla_scansione` non viene nemmeno ricevuto finché questo non ritorna.
#[tauri::command(async)]
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
            // Questa scansione l'ha chiesta qualcuno che è davanti alla finestra
            // e ne legge l'esito riga per riga: se togliesse troppo, se ne
            // accorgerebbe subito. La guardia serve all'altra — quella che parte
            // da sola quando la coda dei download ha finito — e accenderla anche
            // qui vorrebbe dire rifiutarsi di fare quel che è stato chiesto.
            prudente: false,
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
                // `rating_at` accanto a `stats_updated_at`, e non al suo posto:
                // il primo data **questo** voto, il secondo continua a datare
                // l'ultima notizia qualunque sul brano. Solo il primo permette a
                // uno zero di viaggiare come una decisione invece che come
                // un'assenza — vedi `aether_app::sincronia`.
                "UPDATE tracks SET rating = ?2, stats_updated_at = ?3, rating_at = ?3
                  WHERE id = ?1",
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
///
/// `(async)`: apre e legge un intero database dal disco, e la regola dei
/// comandi che toccano il filesystem vale anche per lui.
#[tauri::command(async)]
pub fn piano_importazione(
    stato: State<'_, Stato>,
    percorso: String,
) -> Esito<import_legacy::ImportReport> {
    importa_interno(&stato, &percorso, false)
}

/// Importa dal vecchio database.
///
/// `(async)` per la stessa ragione di [`piano_importazione`].
#[tauri::command(async)]
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

// ── le preferenze della finestra, e il profilo ──────────────────────────────

/// Scrive il tema.
///
/// Passa da `nuvola::se_riuscito` come ogni altra scrittura di preferenza: il
/// tema adesso sta nel database, quindi finisce nel backup — che è metà della
/// ragione per cui ci è tornato.
#[tauri::command]
pub fn imposta_tema(app: tauri::AppHandle, stato: State<'_, Stato>, tema: String) -> Esito<()> {
    let Some(scelto) = aether_app::preferenze::Tema::dal_nome(&tema) else {
        return Err(errore(AppError::new(
            aether_domain::errors::ErrorCode::IpcPayloadInvalid {
                channel: "imposta_tema".to_owned(),
                detail: Some(format!("«{tema}» non è uno dei tre temi")),
            },
        )));
    };
    crate::nuvola::se_riuscito(
        &app,
        con_libreria(&stato, |libreria| {
            aether_app::preferenze::imposta_tema(&libreria.connection, scelto)
        })
        .map_err(errore),
    )
}

/// Scrive la lingua. Una stringa vuota rimette il rilevamento dal sistema.
///
/// Non si controlla che esista un file per quel codice: l'elenco delle lingue
/// sta nella finestra (`src/lingue/`), e un nucleo che lo conoscesse andrebbe
/// ricompilato per aggiungere `de.json` — cioè proprio quel che l'impianto
/// esiste per evitare. Qui si controlla soltanto che sia un codice di lingua, e
/// lo fa `preferenze::imposta_lingua`.
#[tauri::command]
pub fn imposta_lingua(app: tauri::AppHandle, stato: State<'_, Stato>, lingua: String) -> Esito<()> {
    crate::nuvola::se_riuscito(
        &app,
        con_libreria(&stato, |libreria| {
            aether_app::preferenze::imposta_lingua(&libreria.connection, &lingua)
        })
        .map_err(errore),
    )
}

/// Scrive le scorciatoie. Una stringa vuota rimette quelle di serie.
#[tauri::command]
pub fn imposta_scorciatoie(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    scorciatoie: String,
) -> Esito<()> {
    crate::nuvola::se_riuscito(
        &app,
        con_libreria(&stato, |libreria| {
            aether_app::preferenze::imposta_scorciatoie(&libreria.connection, &scorciatoie)
        })
        .map_err(errore),
    )
}

/// Scrive il profilo su un file.
///
/// `(async)`: il percorso lo sceglie chi esporta, e una chiavetta o una
/// cartella di rete sono i due posti più naturali in cui mettere un profilo da
/// portarsi altrove. Sul filo principale una scrittura là sopra sarebbe la
/// finestra ferma per tutto il tempo del salvataggio.
#[tauri::command(async)]
pub fn profilo_esporta(
    stato: State<'_, Stato>,
    percorso: String,
) -> Esito<aether_app::profilo::Esportazione> {
    let esito = (|| {
        let profilo = con_libreria(&stato, |libreria| {
            aether_app::profilo::esporta(&libreria.connection, adesso_ms())
        })?;
        std::fs::write(&percorso, profilo.json.as_bytes()).map_err(|err| {
            AppError::new(aether_domain::errors::ErrorCode::FsWriteFailed {
                path: percorso.clone(),
                detail: Some(err.kind().to_string()),
            })
            .with_cause(err.to_string())
        })?;
        Ok(profilo)
    })();
    esito.map_err(errore)
}

/// Cosa cambierebbe importare questo profilo. Non scrive niente.
///
/// `(async)`: legge il file dal percorso scelto, con la stessa ragione di
/// [`profilo_esporta`].
#[tauri::command(async)]
pub fn profilo_piano(
    stato: State<'_, Stato>,
    percorso: String,
) -> Esito<aether_app::profilo::Piano> {
    let esito = (|| {
        let json = leggi_profilo(&percorso)?;
        con_libreria(&stato, |libreria| {
            aether_app::profilo::piano(&libreria.connection, &json, &esiste)
        })
    })();
    esito.map_err(errore)
}

/// Applica il profilo.
///
/// `(async)`: rilegge il file, come [`profilo_piano`].
#[tauri::command(async)]
pub fn profilo_importa(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    percorso: String,
) -> Esito<aether_app::profilo::Piano> {
    let esito = (|| {
        let json = leggi_profilo(&percorso)?;
        con_libreria(&stato, |libreria| {
            aether_app::profilo::importa(&mut libreria.connection, &json, &esiste)
        })
    })();
    crate::nuvola::se_riuscito(&app, esito.map_err(errore))
}

/// Legge il file del profilo, nominandolo se non si apre.
fn leggi_profilo(percorso: &str) -> Result<String, AppError> {
    std::fs::read_to_string(percorso).map_err(|err| {
        AppError::new(aether_domain::errors::ErrorCode::FsReadFailed {
            path: percorso.to_owned(),
            detail: Some(err.kind().to_string()),
        })
        .with_cause(err.to_string())
    })
}

/// Questo percorso esiste su questo computer?
///
/// Il modulo del profilo non guarda il disco da sé — la si passa come funzione,
/// così le sue prove girano senza avere le cartelle di nessuno. Qui il disco
/// c'è, ed è questa riga.
fn esiste(percorso: &str) -> bool {
    std::path::Path::new(percorso).exists()
}

/// Quanti brani sta in un ripiano della Home.
///
/// Dodici e non duecento: un ripiano si guarda, non si scorre. Chi vuole
/// l'elenco intero ha le quattro destinazioni della libreria, che sono
/// impaginate apposta.
const RIPIANO: i64 = 12;

/// Da quanti giorni un brano dev'essere fermo per contare come «trascurato».
///
/// Sei mesi. Trenta giorni sarebbero «non di questo mese», che su una libreria
/// vera comprende quasi tutto e non racconta niente.
const GIORNI_TRASCURATO: i64 = 180;

/// Quel che la Home mostra all'apertura.
///
/// # Perché un comando solo e non cinque
///
/// Perché sono cinque domande che si fanno **insieme**, all'apertura della
/// finestra, e cinque `invoke` separati vorrebbero dire cinque attraversamenti
/// dell'IPC e cinque prese del lucchetto della libreria per disegnare una
/// schermata sola. È lo stesso ragionamento di [`Avvio`], che raccoglie tutto
/// quel che serve al primo disegno.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Casa {
    /// Il brano su cui ci si era fermati, se c'è.
    pub riprendi: Option<TrackSummary>,
    /// A che punto era, in millisecondi.
    pub riprendi_ms: u64,
    /// Gli ultimi ascoltati, senza ripetizioni.
    pub recenti: Vec<TrackSummary>,
    /// I dischi entrati in libreria per ultimi.
    ///
    /// Dischi e non brani: la musica entra una cartella alla volta, e dodici
    /// brani ordinati per data d'ingresso sono dodici tracce dello stesso
    /// album. Vedi [`recently_added_albums`].
    pub aggiunti: Vec<AlbumSummary>,
    /// Quel che non si ascolta da mesi.
    pub trascurati: Vec<TrackSummary>,
}

/// Compone la Home.
#[tauri::command]
pub fn casa(stato: State<'_, Stato>) -> Esito<Casa> {
    use aether_domain::regole::{
        Campo, Combinazione, Insieme, Operatore, Ordinamento, Regola, Valore,
    };

    con_libreria(&stato, |libreria| {
        let connection = &libreria.connection;

        // Dove ci si era fermati: la coda di ieri sa quale brano, e la
        // posizione sta in una chiave sua.
        let istantanea = aether_app::playback::load_queue(connection)?;
        let corrente = aether_domain::queue::Queue::restore(istantanea).current();
        let riprendi = match corrente {
            Some(id) => aether_app::library::read_summary(connection, id)?,
            None => None,
        };
        // La posizione si legge solo se c'è un brano a cui appartiene: da sola
        // sarebbe un numero senza significato, e mostrarla accanto al brano
        // sbagliato è peggio che non mostrarla.
        let riprendi_ms = if riprendi.is_some() {
            aether_app::playback::load_posizione(connection)?
        } else {
            0
        };

        let recenti = list_tracks(connection, TrackOrder::RecentlyPlayed, 0, RIPIANO)?
            .into_iter()
            // I mai ascoltati stanno in fondo a quell'ordinamento: qui non ci
            // devono proprio essere, o il ripiano «ascoltati di recente» di una
            // libreria appena scansionata si riempirebbe di brani che nessuno
            // ha mai sentito.
            .filter(|brano| brano.play_count > 0)
            .collect();

        let aggiunti = recently_added_albums(connection, RIPIANO)?;

        // I trascurati passano dal motore delle regole invece che da una query
        // scritta a mano: è la stessa domanda che una playlist intelligente sa
        // già fare, e scriverla due volte vorrebbe dire due idee di cosa
        // significhi «non di recente».
        let trascurati = aether_app::smart::brani(
            connection,
            &Insieme {
                combinazione: Combinazione::Tutte,
                regole: vec![Regola {
                    campo: Campo::UltimoAscolto,
                    operatore: Operatore::NonNegliUltimi,
                    valore: Valore::Numero(GIORNI_TRASCURATO),
                }],
                limite: Some(12),
                ordinamento: Ordinamento::Casuale,
            },
            adesso_ms(),
        )?;

        Ok(Casa {
            riprendi,
            riprendi_ms,
            recenti,
            aggiunti,
            trascurati,
        })
    })
    .map_err(errore)
}
/// I documenti pubblici, e dove stanno.
///
/// # Perché un elenco chiuso e non un indirizzo qualunque
///
/// Perché un comando che apre l'indirizzo che gli si passa è un comando che
/// apre **qualunque** indirizzo, e dall'altra parte c'è il browser di sistema.
/// Una pagina compromessa dentro la finestra — una skin con un `<script>` che
/// non doveva passare, un giorno storto — potrebbe mandare chiunque ovunque,
/// dall'interno di un programma di cui ci si fida. Con un elenco chiuso il
/// peggio che può fare è aprire la licenza.
///
/// # Perché sul sito e non i file impacchettati
///
/// I file ci sono — `bundle.resources` mette `LICENSE.txt`,
/// `THIRD-PARTY-NOTICES.md`, `PRIVACY.md`, `TERMS.md` e `font-OFL.txt` accanto
/// all'eseguibile — ma un `.md` aperto col gestore file finisce in un editor di
/// testo o in niente, a seconda di cosa è associato su quella macchina. Il
/// documento sul repository è lo stesso testo, impaginato, e soprattutto è
/// quello **aggiornato**: chi apre la licenza da una versione di un anno fa non
/// ha motivo di leggere la licenza di un anno fa.
#[derive(Debug, Clone, Copy)]
enum Documento {
    /// Il repository.
    Repository,
    /// Dove si segnala un problema.
    Segnalazioni,
    /// La licenza di Aether.
    Licenza,
    /// Gli avvisi sulle dipendenze.
    Terze,
    /// Cosa viaggia in rete.
    Privacy,
    /// Cosa si può fare della musica.
    Condizioni,
    /// Dove sostenere il lavoro.
    ///
    /// Non è un documento e sta fra i documenti: quel che questo elenco tiene
    /// davvero non è «i testi legali», sono **gli indirizzi che la finestra ha
    /// il permesso di far aprire**. Una seconda porta accanto a questa, con la
    /// stessa cautela e un nome diverso, sarebbe stata la stessa serratura
    /// montata due volte.
    Donazioni,
}

impl Documento {
    /// Il nome stabile che attraversa l'IPC.
    fn da_nome(grezzo: &str) -> Option<Self> {
        match grezzo {
            "repository" => Some(Self::Repository),
            "segnalazioni" => Some(Self::Segnalazioni),
            "licenza" => Some(Self::Licenza),
            "terze" => Some(Self::Terze),
            "privacy" => Some(Self::Privacy),
            "condizioni" => Some(Self::Condizioni),
            "donazioni" => Some(Self::Donazioni),
            _ => None,
        }
    }

    /// Dove sta.
    fn indirizzo(self) -> String {
        // La radice viene da `Cargo.toml`, che `strumenti/versione.js` tiene
        // allineato al resto: un indirizzo scritto a mano qui sarebbe il quarto
        // posto in cui la stessa cosa può divergere.
        let radice = env!("CARGO_PKG_REPOSITORY");
        match self {
            Self::Repository => radice.to_owned(),
            Self::Segnalazioni => format!("{radice}/issues"),
            Self::Licenza => format!("{radice}/blob/main/LICENSE"),
            Self::Terze => format!("{radice}/blob/main/THIRD-PARTY-NOTICES.md"),
            Self::Privacy => format!("{radice}/blob/main/PRIVACY.md"),
            Self::Condizioni => format!("{radice}/blob/main/TERMS.md"),
            // Le donazioni non stanno **dentro** il repository: stanno accanto,
            // sotto il profilo di chi lo tiene — `github.com/OWNER/REPO` diventa
            // `github.com/sponsors/OWNER`. Il proprietario si ricava da quella
            // stessa riga invece di riscriverlo qui, per la ragione di sopra: un
            // indirizzo scritto a mano è un posto in più da cui divergere.
            //
            // Il ripiego è il repository, non una pagina inventata: se un domani
            // quella riga non avesse più la forma attesa, chi clicca finisce
            // dove il progetto sta davvero invece che su un 404.
            Self::Donazioni => radice
                .rsplit_once('/')
                .and_then(|(fino_al_proprietario, _repo)| fino_al_proprietario.rsplit_once('/'))
                .map_or_else(
                    || radice.to_owned(),
                    |(host, proprietario)| format!("{host}/sponsors/{proprietario}"),
                ),
        }
    }
}

/// Apre uno dei documenti pubblici nel browser di sistema.
///
/// Mai nella webview: una finestra dell'applicazione che sa disegnare pagine
/// altrui è una superficie di phishing, ed è la stessa ragione per cui il
/// consenso di Google si apre di là. Il CSP di `tauri.conf.json` resta identico.
///
/// # Errori
///
/// `internal.aborted` per un nome che non è nell'elenco — non può succedere
/// dalla finestra, che li scrive tutti a mano, e succede subito se qualcuno ne
/// aggiunge uno di là e si dimentica di qua — o se il browser rifiuta di
/// aprirsi.
#[tauri::command]
pub fn apri_documento(quale: String) -> Esito<()> {
    let Some(documento) = Documento::da_nome(&quale) else {
        return Err(errore(
            AppError::new(ErrorCode::InternalAborted {
                what: Some("apertura di un documento".to_owned()),
            })
            .with_cause(format!("«{quale}» non è uno dei documenti pubblici")),
        ));
    };
    let indirizzo = documento.indirizzo();
    tauri_plugin_opener::open_url(&indirizzo, None::<&str>).map_err(|err| {
        errore(
            AppError::new(ErrorCode::InternalAborted {
                what: Some("apertura del browser".to_owned()),
            })
            .with_cause(err.to_string()),
        )
    })
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Tutti i nomi che la finestra scrive a mano, in un posto solo.
    const TUTTI: &[&str] = &[
        "repository",
        "segnalazioni",
        "licenza",
        "terze",
        "privacy",
        "condizioni",
        "donazioni",
    ];

    /// La prova che avrebbe visto il difetto che c'era qui.
    ///
    /// `CARGO_PKG_REPOSITORY` esiste sempre, e quando il pacchetto non dichiara
    /// `repository` vale la **stringa vuota**: non è un errore di compilazione,
    /// è un indirizzo che diventa `/issues` e un browser che si apre su
    /// niente. Il crate della finestra non la ereditava, quindi ogni tasto dei
    /// documenti pubblici era rotto senza che niente lo dicesse.
    #[test]
    fn ogni_documento_ha_un_indirizzo_vero() {
        for nome in TUTTI {
            let documento = Documento::da_nome(nome).expect("un nome dell'elenco");
            let indirizzo = documento.indirizzo();
            assert!(
                indirizzo.starts_with("https://"),
                "«{nome}» apre «{indirizzo}», che non è un indirizzo"
            );
        }
    }

    /// Un nome fuori elenco non ha un indirizzo, e non lo inventa.
    #[test]
    fn un_nome_inventato_non_apre_niente() {
        assert!(Documento::da_nome("qualunque-cosa").is_none());
        assert!(Documento::da_nome("").is_none());
    }

    /// Le donazioni stanno sul profilo, non dentro il repository.
    #[test]
    fn le_donazioni_vanno_agli_sponsor() {
        let indirizzo = Documento::Donazioni.indirizzo();
        assert!(
            indirizzo.contains("/sponsors/"),
            "le donazioni aprono «{indirizzo}»"
        );
        assert!(!indirizzo.ends_with("/sponsors/"), "manca il proprietario");
    }
}
