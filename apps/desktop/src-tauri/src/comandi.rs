//! I comandi che la finestra può chiamare.
//!
//! Sono involucri e basta. Ogni riga di questo file o traduce un argomento, o
//! chiama `aether_app`, o traduce un errore: **niente decisioni**. Il momento in
//! cui un comando comincia a scegliere cosa inserire o cosa togliere è il
//! momento in cui quella scelta smette di essere provabile senza aprire una
//! finestra, e ricomincia a poter divergere da quella di Android.

use crate::spegnimento::Emette as _;
use aether_app::import_legacy;
use aether_app::library::{
    AlbumSummary, ArtistSummary, Counts, Scadenze, Scan, ScanReport, TrackOrder, TrackSummary,
    album_tracks, counts, list_albums, list_artists, list_tracks, recently_added_albums, search,
    summaries_by_id,
};
use aether_app::settings::CHIAVE_CARTELLE;
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::paths::PathRules;
use serde::Serialize;
use tauri::{Manager as _, State};

use crate::disparte::in_disparte;
use crate::errore::{Esito, errore};
use crate::stato::{DepositoStato, Stato, adesso_ms, con_libreria};

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
/// # Dove gira
///
/// Sul pool bloccante, via [`in_disparte`]. Non sul filo principale, dove venti
/// secondi congelano la webview; e nemmeno su un worker del runtime, che è quel
/// che `#[tauri::command(async)]` farebbe da solo: i worker sono tanti quanti i
/// processori, e una scansione ferma su una condivisione di rete se ne
/// prenderebbe uno per tutto il tempo del timeout di sistema, insieme a ogni
/// altro comando asincrono che nel frattempo si è messo in coda.
///
/// # Cosa non tiene mentre legge
///
/// Il lucchetto della libreria. Il nucleo lo chiede attraverso [`DepositoStato`]
/// una transazione alla volta e lo lascia subito: fra un lotto e l'altro la
/// finestra può leggere, `annulla_scansione` risponde, la chiusura si chiude. La
/// camminata sulle cartelle — la parte che su una share lenta è quasi tutta la
/// durata — non lo chiede affatto.
///
/// Il prezzo è che due scansioni insieme si intreccerebbero davvero, e
/// `tracks.path` è UNIQUE: per questo la prima cosa che si prende è il turno.
///
/// # Errori
///
/// `library.scanBusy` se una scansione è già in corso — a mano, o quella
/// automatica che parte quando la coda dei download ha finito. `internal.aborted`
/// se lo stato non c'è più. Poi quel che riporta il nucleo: `db.*` per il
/// database, `fs.*` per il disco. Una radice che non risponde **non** è un
/// errore: finisce in `radiciSaltate`, e i suoi brani restano dove sono.
#[tauri::command]
pub async fn scansiona(app: tauri::AppHandle) -> Esito<EsitoScansione> {
    let mano = app.clone();
    let esito = in_disparte("scansione", move || scansiona_ora(&mano))
        .await
        .map_err(errore)?;
    // Una scansione porta dentro brani che nessuno ha mai tentato di
    // arricchire, ed è il momento in cui hanno più bisogno: appena importati
    // sono precisamente quelli con «Album sconosciuto» e nessuna copertina.
    if esito.is_ok() {
        crate::arricchimento::sporca(&app);
        crate::analisi::sporca(&app);
        // Una scansione cambia quali cartelle esistono e cosa c'è dentro:
        // l'albero del pannello «Cartelle» è derivato da `tracks`, quindi non
        // si aggiorna, si butta. Costa un contatore atomico, e non ricostruisce
        // niente — se il pannello non è aperto non c'è nemmeno un albero.
        if let Some(indice) = app.try_state::<std::sync::Arc<crate::cartelle::IndiceCartelle>>() {
            indice.invalida();
        }
    }
    // Una scansione cambia quali brani esistono, quindi quali statistiche il
    // backup può ancorare: un brano ritrovato dopo una reinstallazione va
    // salvato subito, non al prossimo cuoricino.
    crate::nuvola::se_riuscito(&app, esito)
}

/// Il corpo di [`scansiona`], sul filo che l'ha presa in disparte.
///
/// Una funzione a parte e non una chiusura dentro il comando perché quel che si
/// muove nel pool dev'essere `'static`, mentre qui dentro si prendono prestiti —
/// dello stato, delle cartelle, della bandiera d'annullamento — che vivono per la
/// durata di questa chiamata e non oltre.
fn scansiona_ora(app: &tauri::AppHandle) -> Esito<EsitoScansione> {
    // `try_state` e non `state`: `state` panica se lo stato non c'è, e questo
    // corpo gira su un filo suo, che può ritrovarsi vivo mentre la finestra si
    // chiude e gli stati gestiti vengono lasciati cadere.
    let Some(stato) = app.try_state::<Stato>() else {
        return Err(errore(
            AppError::new(ErrorCode::InternalAborted {
                what: Some("scansione".to_owned()),
            })
            .with_cause("la libreria non è più fra gli stati gestiti"),
        ));
    };
    // Il turno prima di tutto: due scansioni intrecciate si contendono
    // l'inserimento della stessa riga in `tracks`, dove `path` è UNIQUE. Si
    // libera da sé quando la guardia cade, anche uscendo per un `?`.
    let Some(_turno) = stato.turno_di_scansione() else {
        return Err(errore(AppError::new(ErrorCode::LibraryScanBusy)));
    };
    // Ma se il programma si sta chiudendo, non si comincia affatto: una
    // scansione chiesta mentre si esce non deve azzerare la bandiera che
    // l'uscita ha appena alzato, cioè far ripartire proprio il filo che tutto il
    // resto sta aspettando che se ne vada.
    if crate::spegnimento::in_uscita() {
        return Err(errore(
            AppError::new(ErrorCode::InternalAborted {
                what: Some("scansione".to_owned()),
            })
            .with_cause("il programma si sta chiudendo"),
        ));
    }
    // Dopo il turno e il controllo d'uscita, e prima di leggere: un annullamento
    // arrivato dopo la fine della scansione precedente fermerebbe questa al
    // primo file.
    stato.riprendi_scansioni();

    // Lo store delle copertine si copia **fuori** dal lucchetto: è un percorso, e
    // il nucleo se lo porta dietro su fili che possono sopravvivere alla scadenza
    // del file che stavano leggendo.
    let copertine = stato.copertine().map_err(errore)?;
    let roots =
        con_libreria(&stato, |libreria| leggi_cartelle(&libreria.connection)).map_err(errore)?;

    let fermare = &*stato;
    // La stessa bandiera che legge il callback dell'avanzamento, più l'uscita.
    // Il nucleo la guarda durante la camminata, dove di avanzamento non ce n'è
    // perché non c'è ancora niente da contare — ed è la parte che su una share
    // lenta dura di più — e poi fra un file e l'altro. L'uscita entra qui e non
    // solo nella bandiera perché una chiusura arrivata a scansione già partita
    // deve fermarla anche se nessuno ha premuto «Annulla».
    let fermati = || fermare.scansione_fermata() || crate::spegnimento::in_uscita();
    let scan = Scan {
        files: std::sync::Arc::new(aether_app::files::LocalFiles),
        covers: copertine,
        roots: &roots,
        rules: PathRules::for_current_platform(),
        // Questa scansione l'ha chiesta qualcuno che è davanti alla finestra
        // e ne legge l'esito riga per riga: se togliesse troppo, se ne
        // accorgerebbe subito. La guardia serve all'altra — quella che parte
        // da sola quando la coda dei download ha finito — e accenderla anche
        // qui vorrebbe dire rifiutarsi di fare quel che è stato chiesto.
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: Some(&fermati),
    };
    let mut deposito = DepositoStato::nuovo(&stato);
    let mut ultimo = 0usize;
    let report = scan
        .run_su(&mut deposito, |fatti, totale| {
            // Non a ogni file: mandare un evento per ognuno di 1421 file
            // inonderebbe il canale IPC per disegnare una barra che si muove di
            // meno di un pixel per volta.
            if fatti == totale || fatti.saturating_sub(ultimo) >= 25 {
                ultimo = fatti;
                app.emetti("scansione:avanzamento", Avanzamento { fatti, totale });
            }
            if fermare.scansione_fermata() {
                std::ops::ControlFlow::Break(())
            } else {
                std::ops::ControlFlow::Continue(())
            }
        })
        .map_err(errore)?;
    // I numeri in una presa a parte, dopo: il deposito è appena stato
    // restituito, e chiederli da dentro la scansione vorrebbe dire tenerlo per
    // una `COUNT(*)` che con la scrittura non c'entra niente.
    let numeri = con_libreria(&stato, |libreria| counts(&libreria.connection)).map_err(errore)?;
    Ok(EsitoScansione::da(&report, numeri))
}

/// Chiede alla scansione in corso di fermarsi.
///
/// Non chiede il lucchetto della libreria, e non perché non potrebbe: la
/// scansione ormai lo prende e lo lascia lotto per lotto, quindi lo otterrebbe.
/// È che questo comando è **sincrono**, cioè gira sul filo principale, e il filo
/// principale è quello che ridisegna la finestra: qualunque attesa lì è la
/// finestra ferma. Alza un bit, e la scansione lo legge fra un file e l'altro —
/// e anche durante la camminata sulle cartelle, dove un lucchetto non c'è.
///
/// Torna subito: fermarsi vuol dire «alla fine del file che si sta leggendo»,
/// non «adesso». Il lotto scrive quel che ha già letto e la scansione smette lì:
/// quel che è entrato in libreria resta, e la passata dopo finisce il lavoro.
/// L'attesa che resta, quindi, è quella di un'apertura sola — fino alla sua
/// scadenza, se il file è su una condivisione che non risponde. Chi lo mostra lo
/// sa dall'esito, che dirà `annullata`.
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

// ── le preferenze della finestra ────────────────────────────────────────────

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

/// Se chi guarda ha chiesto meno movimento di quanto la skin ne dichiari.
///
/// # Perché un comando suo e non un campo di [`Avvio`]
///
/// Il tema sta in [`Avvio`] perché ha un passato da riconciliare: la finestra
/// deve distinguere «mai scelto» da «scelto sistema» per sapere se ripiegare
/// sulla riga rimasta in `localStorage`, e quella decisione va presa nello
/// stesso istante in cui l'avvio risponde. Qui non c'è niente da riconciliare
/// — la chiave nasce adesso, e assente vuol dire spento — quindi non c'è
/// ragione di far crescere la struttura che ogni apertura riempie sempre. È la
/// stessa regola di [`cartelle_ui`], scritta per esteso là sopra.
///
/// La finestra la chiede una volta e la passa ad `applicaMovimento`, che
/// scrive `data-motion-utente` sulla radice: il rimedio è già in `stile.css`,
/// accanto a quello di `prefers-reduced-motion`, e non se ne scrive un
/// secondo.
#[tauri::command]
pub fn movimento_ridotto(stato: State<'_, Stato>) -> Esito<bool> {
    con_libreria(&stato, |libreria| {
        aether_app::preferenze::movimento_ridotto(&libreria.connection)
    })
    .map_err(errore)
}

/// Scrive se chi guarda ha chiesto meno movimento.
///
/// Passa da `nuvola::se_riuscito` come il tema, e con una ragione in più: una
/// preferenza di accessibilità è precisamente quella che deve ritrovarsi
/// identica su un altro computer, e l'unico meccanismo che la porta là è il
/// backup — più la riga nel catalogo del profilo.
#[tauri::command]
pub fn imposta_movimento_ridotto(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    ridotto: bool,
) -> Esito<()> {
    crate::nuvola::se_riuscito(
        &app,
        con_libreria(&stato, |libreria| {
            aether_app::preferenze::imposta_movimento_ridotto(&libreria.connection, ridotto)
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

/// Com'era rimasto il pannello «Cartelle».
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoUiCartelle {
    /// I nodi aperti, dal meno recente al più recente.
    pub aperte: Vec<String>,
    /// Il nodo su cui stava il fuoco, se ce n'era uno.
    pub scelta: Option<String>,
}

/// Com'era rimasto il pannello «Cartelle»: i nodi aperti e quello col fuoco.
///
/// # Perché un comando suo e non un campo di [`Avvio`]
///
/// Perché chi non apre mai quel pannello non deve pagarlo, ed è la stessa
/// regola con cui `cartelle::IndiceCartelle` costruisce l'albero alla prima
/// domanda invece che all'avvio. Due letture di `settings` sono poca cosa, ma
/// `Avvio` è la struttura che ogni avvio riempie **sempre**: quel che ci entra
/// smette di essere facoltativo, e il modo di tenerla parsimoniosa è non
/// mettercelo.
#[tauri::command]
pub fn cartelle_ui(stato: State<'_, Stato>) -> Esito<StatoUiCartelle> {
    con_libreria(&stato, |libreria| {
        Ok(StatoUiCartelle {
            aperte: aether_app::preferenze::cartelle_aperte(&libreria.connection)?,
            scelta: aether_app::preferenze::cartella_scelta(&libreria.connection)?,
        })
    })
    .map_err(errore)
}

/// Scrive com'è rimasto il pannello «Cartelle».
///
/// # Perché non passa da `nuvola::se_riuscito`
///
/// Perché sono percorsi di *questa* macchina — vedi il preambolo di
/// `aether_app::preferenze` — e una passata di backup vale quel che porta
/// altrove. Farla partire a ogni nodo aperto vorrebbe dire svegliare la rete
/// per un dato che sull'altro computer non si può nemmeno usare.
#[tauri::command]
pub fn imposta_cartelle_ui(
    stato: State<'_, Stato>,
    aperte: Vec<String>,
    scelta: String,
) -> Esito<()> {
    con_libreria(&stato, |libreria| {
        aether_app::preferenze::imposta_cartelle_aperte(&libreria.connection, &aperte)?;
        aether_app::preferenze::imposta_cartella_scelta(&libreria.connection, &scelta)
    })
    .map_err(errore)
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
/// Una raccolta del lunedì, coi suoi brani già risolti.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RaccoltaSettimana {
    /// La riga, per marcarla aperta.
    pub id: i64,
    /// `ripescati` o `ancora`.
    pub genere: String,
    /// Quale delle «Ancora» è. Zero per i ripescati.
    pub ordine: i64,
    /// Il materiale del nome: un genere, un artista, o niente.
    pub etichetta: Option<String>,
    /// Come leggerlo: `genere` o `artista`.
    pub etichetta_tipo: Option<String>,
    /// Se è già stata aperta.
    pub aperta: bool,
    /// I brani, nell'ordine deciso dal calcolo.
    pub brani: Vec<TrackSummary>,
}

/// Le raccolte di questa settimana, generandole se è lunedì di nuovo.
///
/// # Perché il fuso arriva dall'interfaccia
///
/// Perché il nucleo non ha un fuso: `aether-domain` non guarda l'orologio e
/// `aether-app` tiene il tempo in millisecondi universali, che è l'unica forma
/// che sopravvive a una sincronizzazione fra dispositivi in due paesi. Il
/// lunedì invece è un fatto locale — comincia sette ore prima a Roma che a Los
/// Angeles — e l'unico posto che sa dove si trova la finestra è la finestra.
///
/// `scostamento_minuti` sono i minuti da **aggiungere** all'universale per
/// avere l'ora locale: è il contrario del segno di `getTimezoneOffset()`, e il
/// verso si legge senza doverci pensare. Vedi
/// [`aether_domain::settimana::lunedi`].
///
/// # Perché `(async)` e non un comando qualunque
///
/// Perché «decine di millisecondi una volta a settimana» era vero del lavoro e
/// falso del costo. Un comando sincrono lo esegue **il filo principale**, che è
/// quello che disegna la finestra: per tutto il tempo della generazione la
/// finestra non risponde, e siccome la generazione tiene il lucchetto della
/// libreria, dietro ci si accoda anche la riproduzione — che quel lucchetto lo
/// vuole a ogni cambio di stato.
///
/// `(async)` non cambia una riga del corpo: dice a Tauri di eseguirlo su un
/// filo del suo pool invece che sul principale. Il lucchetto della libreria
/// resta, e resta giusto che resti — è la connessione, ed è una sola — ma
/// aspettarlo non ferma più il disegno.
///
/// Il filo in più che questo commento diceva di non volere non c'è comunque:
/// il pool esiste già, ed è dove girano di già tutti i comandi `(async)`.
#[tauri::command(async)]
pub fn settimana(
    stato: State<'_, Stato>,
    scostamento_minuti: i32,
) -> Esito<Vec<RaccoltaSettimana>> {
    let adesso = adesso_ms();
    let lunedi = aether_domain::settimana::lunedi(adesso, scostamento_minuti);

    con_libreria(&stato, |libreria| {
        let raccolte = aether_app::settimana::aggiorna(&mut libreria.connection, lunedi, adesso)?;
        let mut fuori = Vec::with_capacity(raccolte.len());
        for raccolta in raccolte {
            fuori.push(RaccoltaSettimana {
                id: raccolta.id,
                genere: raccolta.genere,
                ordine: raccolta.ordine,
                etichetta: raccolta.etichetta,
                etichetta_tipo: raccolta.etichetta_tipo,
                aperta: raccolta.aperta,
                brani: summaries_by_id(&libreria.connection, &raccolta.brani)?,
            });
        }
        Ok(fuori)
    })
    .map_err(errore)
}

/// Segna una raccolta come aperta, così l'annuncio smette di annunciarla.
#[tauri::command]
pub fn settimana_apri(stato: State<'_, Stato>, raccolta: i64) -> Esito<()> {
    con_libreria(&stato, |libreria| {
        aether_app::settimana::apri(&libreria.connection, raccolta, adesso_ms())
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

/// Le cartelle musicali del sistema che contengono davvero qualcosa.
///
/// Serve al primo avvio, e la decisione di cosa proporre sta in
/// `aether_app::primo`: qui si traduce soltanto, come ogni altra riga di questo
/// file. Quali siano le cartelle musicali di un sistema però lo sa il sistema, e
/// **questo** lo si può chiedere solo da qui: `aether-app` non conosce Tauri e
/// non deve conoscerlo.
///
/// # La scadenza dice quanto, la riserva dice dove
///
/// Attraversare la cartella Musica di qualcuno può durare, e su una cartella
/// sincronizzata che non risponde può durare i quaranta secondi di Windows. È
/// il primo avvio, cioè il momento in cui un'attesa senza spiegazione fa
/// chiudere il programma: alla scadenza si restituisce un elenco vuoto, che la
/// finestra sa già disegnare — è lo stato «nessuna cartella trovata», che deve
/// esistere comunque per chi la musica la tiene altrove.
///
/// Quella scadenza dice **quanto** si aspetta. [`in_disparte`] dice **dove**
/// l'attesa succede: sul pool bloccante e non su un worker del runtime, che sono
/// tanti quanti i processori e che qui verrebbero occupati da un'attesa di rete
/// mentre servono a ogni altro comando asincrono. Le due cose non si sostituiscono
/// a vicenda, e la documentazione di [`crate::disparte`] dice perché.
///
/// Prende un `AppHandle` e non uno `State`: il lavoro va mosso dentro una
/// chiusura `'static`, e un prestito dello stato lì dentro non entra.
#[tauri::command]
pub async fn cartelle_candidate(app: tauri::AppHandle) -> Esito<Vec<CartellaCandidata>> {
    let percorsi = percorsi_musicali(&app);
    let trovate = in_disparte("cartelle candidate", move || {
        aether_app::scadenza::con_scadenza(RICERCA_CARTELLE_NOME, RICERCA_CARTELLE, move || {
            aether_app::primo::esamina(&aether_app::files::LocalFiles, &percorsi)
        })
        .unwrap_or_default()
    })
    .await
    .map_err(errore)?;

    Ok(trovate
        .into_iter()
        .map(|c| CartellaCandidata {
            percorso: c.percorso,
            brani: u32::try_from(c.brani).unwrap_or(u32::MAX),
            troncato: c.troncato,
            parziale: c.parziale,
        })
        .collect())
}

/// Quanto si aspetta prima di rinunciare a cercare le cartelle musicali.
///
/// Sei secondi. Più della sonda delle radici, che deve solo dire se una cartella
/// esiste, e meno di quanto qualcuno resti a guardare una schermata di benvenuto
/// senza capire se il programma è vivo.
const RICERCA_CARTELLE: std::time::Duration = std::time::Duration::from_secs(6);
/// Il nome del filo che cerca, per il registro diagnostico.
const RICERCA_CARTELLE_NOME: &str = "cartelle-candidate";

/// Una cartella da proporre al primo avvio.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CartellaCandidata {
    /// Dove.
    pub percorso: String,
    /// Quanti brani ci si sono contati.
    pub brani: u32,
    /// Il conteggio si è fermato al tetto: ce n'erano altri.
    pub troncato: bool,
    /// La camminata ha perso dei rami: il numero è una sottostima.
    pub parziale: bool,
}

/// Dove un sistema tiene la musica, in ordine di probabilità.
///
/// La cartella dichiarata dal sistema per prima, poi la sua copia dentro
/// OneDrive — che su Windows è dove finisce davvero la musica di chi ha il
/// backup acceso, e che il sistema **non** dichiara come cartella musicale.
///
/// Non si aggiunge la cartella dei download: contiene musica per qualcuno e
/// tutto il resto per tutti, e una radice sorvegliata sbagliata si paga a ogni
/// scansione successiva.
fn percorsi_musicali(app: &tauri::AppHandle) -> Vec<String> {
    let mut fuori = Vec::new();
    let percorsi = app.path();
    if let Ok(musica) = percorsi.audio_dir() {
        fuori.push(musica.to_string_lossy().into_owned());
    }
    if let Ok(casa) = percorsi.home_dir() {
        for nome in ["Music", "Musica"] {
            let dentro_onedrive = casa.join("OneDrive").join(nome);
            fuori.push(dentro_onedrive.to_string_lossy().into_owned());
        }
    }
    // Due percorsi che puntano allo stesso posto capitano — la cartella
    // musicale *è* dentro OneDrive quando il backup è acceso — e proporli due
    // volte sarebbe una schermata che si contraddice. Si toglie il secondo e
    // non si ordina: l'ordine è quello della probabilità, e ordinarli
    // alfabeticamente lo butterebbe via.
    let mut visti = std::collections::HashSet::new();
    fuori.retain(|percorso| visti.insert(percorso.clone()));
    fuori
}
