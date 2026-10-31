//! I comandi che la finestra può chiamare.
//!
//! Sono involucri e basta. Ogni riga di questo file o traduce un argomento, o
//! chiama `rymembra_app`, o traduce un errore: **niente decisioni**. Il momento in
//! cui un comando comincia a scegliere cosa inserire o cosa togliere è il
//! momento in cui quella scelta smette di essere provabile senza aprire una
//! finestra, e ricomincia a poter divergere da quella di Android.

use crate::spegnimento::Emette as _;
use rymembra_app::import_legacy;
use rymembra_app::library::{
    AlbumSummary, ArtistSummary, Avanzamento as ScanAvanzamento, Counts, RimozioneInAttesa,
    RimozioniSottoUnaRadice, Scadenze, Scan, ScanReport, TrackOrder, TrackSummary, album_tracks,
    applica_rimozioni, counts, list_albums, list_artists, list_tracks, recently_added_albums,
    riassumi_rimozioni, search, summaries_by_id,
};
use rymembra_app::settings::CHIAVE_CARTELLE;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::paths::PathRules;
use serde::Serialize;
use tauri::{Manager as _, State};

use crate::disparte::{in_disparte, leggi_la_libreria};
use crate::errore::{Esito, errore};
use crate::nota;
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
    /// nucleo. Vedi `rymembra_app::preferenze`.
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
    Ok(rymembra_app::settings::read_json(connection, CHIAVE_CARTELLE)?.unwrap_or_default())
}

/// Lo stato all'avvio.
///
/// In disparte, come ogni lettura che conta tutta la libreria: vedi
/// [`crate::disparte`].
#[tauri::command]
pub async fn avvio(app: tauri::AppHandle) -> Esito<Avvio> {
    leggi_la_libreria(app, "avvio", |libreria| {
        Ok(Avvio {
            data_dir: libreria.data_dir.display().to_string(),
            migrazioni: libreria.migrazioni,
            fts5: libreria.fts5,
            cartelle: leggi_cartelle(&libreria.connection)?,
            // Il `filter` regge le righe vuote scritte prima che
            // `imposta_cartella_download` cancellasse invece di svuotare.
            cartella_download: rymembra_app::settings::read(
                &libreria.connection,
                rymembra_app::settings::CHIAVE_CARTELLA_DOWNLOAD,
            )?
            .filter(|scelta| !scelta.trim().is_empty()),
            numeri: counts(&libreria.connection)?,
            tema: rymembra_app::preferenze::tema(&libreria.connection)?
                .map(|t| t.nome().to_owned()),
            lingua: rymembra_app::preferenze::lingua(&libreria.connection)?,
            scorciatoie: rymembra_app::preferenze::scorciatoie(&libreria.connection)?,
        })
    })
    .await
    .map_err(errore)
}

/// Cambia le cartelle sorvegliate, e dice cosa ne ha fatto.
///
/// # I doppioni si tolgono qui
///
/// La finestra confrontava le cartelle con `includes`, cioè stringa per
/// stringa: `D:\Musica`, `d:\musica` e `D:\Musica\` — il dialogo, un
/// trascinamento e un percorso incollato — erano tre cartelle sorvegliate, e la
/// stessa musica si scansionava tre volte. Il confronto giusto è
/// [`rymembra_domain::paths::path_key`], che è quello della scansione, e sta qui
/// perché le strade che arrivano a questo comando sono tre: resta la prima
/// grafia, quella con cui la cartella era già in elenco.
///
/// # E le annidate, e le gemelle
///
/// `D:\M` e `D:\M\Rock` insieme sono la stessa musica letta due volte: resta la
/// madre, e la risposta lo dice fra le `assorbite`. `Z:\` e `\\nas\musica`
/// possono essere la stessa cartella con due nomi, e aggiungere la seconda
/// voleva dire ogni brano rientrato col nome nuovo e la storia vuota: la
/// seconda non entra, e la risposta la mette fra le `gemelle`, per cui la
/// finestra propone [`cartelle_trasloca`]. La decisione è
/// [`rymembra_app::profilo::radici::ordina_cartelle`], che si prova senza una
/// share; qui si chiede soltanto al disco il nome vero di ogni cartella.
///
/// # Dove gira
///
/// In disparte, e il nome vero si chiede **fuori** dal lucchetto: risolvere una
/// lettera mappata tocca la rete, e su una share che non risponde è un'attesa.
/// Solo quando c'è una cartella nuova — togliere una cartella non ha bisogno di
/// sapere dove stanno le altre.
///
/// # Errori
///
/// `internal.aborted` se la libreria non c'è più, `db.*` se la lettura o la
/// scrittura falliscono.
#[tauri::command]
pub async fn imposta_cartelle(
    app: tauri::AppHandle,
    cartelle: Vec<String>,
) -> Esito<rymembra_app::profilo::radici::CartelleOrdinate> {
    let mano = app.clone();
    let esito = in_disparte("cartelle sorvegliate", move || {
        let stato = libreria_di(&mano, "cartelle sorvegliate")?;
        scrivi_cartelle(&stato, &cartelle)
    })
    .await
    .map_err(errore)?
    .map_err(errore);
    // La sorveglianza segue l'elenco: una cartella nuova si comincia a
    // guardare adesso, una tolta smette.
    if esito.is_ok() {
        crate::sorveglia::riconfigura(&app);
    }
    crate::nuvola::se_riuscito(&app, esito)
}

/// Scrive l'elenco delle cartelle sorvegliate con le regole di sempre:
/// annidate raccolte nella madre, gemelle trattenute per il trasloco.
///
/// Lo usano [`imposta_cartelle`] e l'aggiunta di una cartella del cloud
/// (`nuvola_musica::nuvola_aggiungi`): una sola strada per scrivere l'elenco,
/// così una cartella di OneDrive dentro un'altra già scelta si raccoglie come
/// una del disco. La sorveglianza non la avvisa: lo fa chi chiama, quando ha
/// finito anche il resto.
///
/// # Errori
///
/// `db.*` se la lettura o la scrittura falliscono.
pub(crate) fn scrivi_cartelle(
    stato: &Stato,
    cartelle: &[String],
) -> Result<rymembra_app::profilo::radici::CartelleOrdinate, AppError> {
    let prima = con_libreria(stato, |libreria| leggi_cartelle(&libreria.connection))?;
    let nomi = nomi_veri(&prima, cartelle);
    let ordinate = rymembra_app::profilo::radici::ordina_cartelle(&prima, cartelle, &|c| {
        nomi.get(c).cloned().flatten()
    });
    con_libreria(stato, |libreria| {
        rymembra_app::settings::write_json(
            &libreria.connection,
            CHIAVE_CARTELLE,
            &ordinate.cartelle,
        )
    })?;
    Ok(ordinate)
}

/// Le cartelle sorvegliate, come stanno in `settings`.
///
/// # Errori
///
/// `db.*` se la lettura fallisce.
pub(crate) fn cartelle_sorvegliate(stato: &Stato) -> Result<Vec<String>, AppError> {
    con_libreria(stato, |libreria| leggi_cartelle(&libreria.connection))
}

/// La libreria, o l'errore di chi arriva mentre la finestra si chiude.
///
/// `try_state` e non `state`, come in `cancellazione`: questi corpi girano sul
/// pool bloccante, e possono ritrovarsi vivi quando gli stati gestiti sono già
/// stati lasciati cadere.
pub(crate) fn libreria_di<'a>(
    app: &'a tauri::AppHandle,
    cosa: &'static str,
) -> Result<State<'a, Stato>, AppError> {
    app.try_state::<Stato>().ok_or_else(|| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some(cosa.to_owned()),
        })
        .with_cause("la libreria non è più fra gli stati gestiti")
    })
}

/// Il nome vero di ogni cartella chiesta, se fra loro ce n'è una nuova.
///
/// Vuoto quando non ce n'è: le regole che lo usano confrontano una cartella
/// **nuova** con le altre, e senza nuove non c'è niente da confrontare.
fn nomi_veri(
    prima: &[String],
    chieste: &[String],
) -> std::collections::HashMap<String, Option<String>> {
    let regole = PathRules::for_current_platform();
    let chiave = |p: &str| rymembra_domain::paths::path_key(p, regole);
    let c_e_una_nuova = chieste
        .iter()
        .any(|c| !prima.iter().any(|p| chiave(p) == chiave(c)));
    if !c_e_una_nuova {
        return std::collections::HashMap::new();
    }
    let elenco: Vec<String> = chieste
        .iter()
        .map(|c| c.trim().to_owned())
        .filter(|c| !c.is_empty())
        .collect();
    let risolti =
        rymembra_app::volumi::risolvi_tutti(&elenco, rymembra_app::volumi::SCADENZA_RISOLUZIONE);
    elenco.into_iter().zip(risolti).collect()
}

/// Sposta la libreria da una cartella sorvegliata alla stessa cartella scritta
/// in un altro modo, senza perdere niente.
///
/// È la risposta a una [`rymembra_app::profilo::radici::Gemella`]: `Z:\` e
/// `\\nas\musica` sono lo stesso posto, e chi aggiunge la seconda vuole che la
/// libreria la usi **al posto** della prima. Si riscrivono i percorsi dei brani
/// — ascolti, voti e playlist restano attaccati alle loro righe — poi la
/// cartella sorvegliata e, se ci stava sotto, quella dei download.
///
/// # Le tre fasi
///
/// Le stesse del profilo: i percorsi nuovi si leggono con il lucchetto preso
/// per un attimo, si verificano **fuori** (in parallelo, con la scadenza: sono
/// file su una share) e si scrivono dentro una transazione sola, con le
/// risposte già in mano. Nessuna domanda al disco parte con il lucchetto preso.
///
/// # Tutto o niente
///
/// Se anche un solo file non si è potuto verificare — la share ha smesso di
/// rispondere a metà — non si scrive niente. Un trasloco a metà lascerebbe una
/// parte dei brani sotto la cartella vecchia, che non è più sorvegliata: la
/// scansione dopo li rimetterebbe dentro col nome nuovo e la storia vuota, che
/// è il guasto che il trasloco esiste per evitare.
///
/// # Errori
///
/// `fs.networkUnavailable` se qualche file non si è potuto verificare;
/// `internal.aborted` se la libreria non c'è più; `db.*` per il resto.
#[tauri::command]
pub async fn cartelle_trasloca(
    app: tauri::AppHandle,
    da: String,
    a: String,
) -> Esito<rymembra_app::profilo::radici::Riscritti> {
    let mano = app.clone();
    let esito = in_disparte("trasloco di una cartella sorvegliata", move || {
        trasloca_ora(&mano, &da, &a)
    })
    .await
    .map_err(errore)?
    .map_err(errore);
    // L'albero delle cartelle e ogni vista che nomina un percorso vanno
    // richiesti: le radici sono cambiate come dopo una scansione.
    if esito.is_ok() {
        crate::cartelle::cambiate(&app);
        crate::sorveglia::riconfigura(&app);
    }
    crate::nuvola::se_riuscito(&app, esito)
}

/// Quanto il trasloco aspetta il turno delle scansioni prima di dire
/// «occupato». Abbastanza per una pulizia delle copertine, non per una
/// scansione: quella la si lascia finire, e il trasloco si chiede dopo.
const ATTESA_DEL_TURNO_DI_TRASLOCO: std::time::Duration = std::time::Duration::from_secs(15);

/// Il turno di manutenzione, aspettato al più `entro`: chi lo chiede tocca le
/// righe dei brani, e una scansione che scrive a lotti nel frattempo
/// riscriverebbe sopra quel che ha appena fatto.
///
/// Si aspetta un poco invece di rinunciare subito: il turno lo prende anche la
/// pulizia delle copertine, una decina di secondi dopo ogni scansione.
///
/// # Errori
///
/// `library.scanBusy`, con `perche` come causa, se il turno non si libera in
/// tempo o l'applicazione si chiude.
pub(crate) fn aspetta_il_turno_di_manutenzione<'a>(
    stato: &'a Stato,
    entro: std::time::Duration,
    perche: &str,
) -> Result<crate::stato::Turno<'a>, AppError> {
    let scadenza = std::time::Instant::now() + entro;
    loop {
        if let Some(turno) = stato.turno_di_manutenzione() {
            return Ok(turno);
        }
        if crate::spegnimento::in_uscita() || std::time::Instant::now() >= scadenza {
            return Err(AppError::new(ErrorCode::LibraryScanBusy).with_cause(perche.to_owned()));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Le cartelle sorvegliate e quella dei download, dopo un trasloco: dentro la
/// transazione di chi trasloca i brani.
///
/// Lo usa anche il filo di un conto del cloud, quando la cartella scelta è
/// stata rinominata sul servizio (`nuvola_musica`).
///
/// # Errori
///
/// Quelli del database.
pub(crate) fn riscrivi_le_cartelle(
    tx: &rusqlite::Connection,
    rimappature: &[rymembra_app::profilo::Rimappatura],
) -> Result<(), AppError> {
    use rymembra_app::profilo::radici;
    let cartelle = leggi_cartelle(tx)?;
    let rimappate: Vec<String> = cartelle
        .iter()
        .map(|c| radici::rimappa_percorso(c, rimappature))
        .collect();
    // Rimesse in ordine con le regole di sempre: la cartella traslocata può
    // essere finita dentro un'altra, o sopra un doppione.
    let ordinate = radici::ordina_cartelle(&rimappate, &rimappate, &|_| None);
    rymembra_app::settings::write_json(tx, CHIAVE_CARTELLE, &ordinate.cartelle)?;

    let chiave_download = rymembra_app::settings::CHIAVE_CARTELLA_DOWNLOAD;
    if let Some(scarichi) = rymembra_app::settings::read(tx, chiave_download)? {
        let spostata = radici::rimappa_percorso(&scarichi, rimappature);
        if spostata != scarichi {
            rymembra_app::settings::write(tx, chiave_download, &spostata)?;
        }
    }
    Ok(())
}

/// Il corpo di [`cartelle_trasloca`], sul filo che l'ha presa in disparte.
fn trasloca_ora(
    app: &tauri::AppHandle,
    da: &str,
    a: &str,
) -> Result<rymembra_app::profilo::radici::Riscritti, AppError> {
    use rymembra_app::profilo::radici;
    use rymembra_app::volumi::{self, Esistenza};

    let stato = libreria_di(app, "trasloco di una cartella sorvegliata")?;
    // Il turno delle scansioni, per tutta la durata. Una scansione scrive a
    // lotti e lascia il lucchetto fra l'uno e l'altro: se stesse leggendo la
    // cartella vecchia, i lotti dopo il trasloco rimetterebbero i suoi brani
    // sotto il nome di prima — una cartella non più sorvegliata, con la storia
    // vuota alla scansione dopo — che è il guasto che il trasloco esiste per
    // evitare. Il turno di manutenzione e non quello di scansione: escludono
    // allo stesso modo, ma questo non sveglia la manutenzione quando cade.
    //
    // Lo si aspetta un poco invece di rinunciare subito: il turno lo prende
    // anche la pulizia delle copertine, una decina di secondi dopo ogni
    // scansione, e un trasloco rifiutato per quella sarebbe una domanda da
    // rifare per niente. Si gira in disparte, quindi l'attesa non ferma
    // nessuno; oltre la scadenza c'è davvero una scansione, e lo si dice.
    let _turno = aspetta_il_turno_di_manutenzione(
        &stato,
        ATTESA_DEL_TURNO_DI_TRASLOCO,
        "il trasloco aspetta che la scansione in corso finisca",
    )?;
    let rimappature = [rymembra_app::profilo::Rimappatura {
        da: da.to_owned(),
        a: a.to_owned(),
        brani: 0,
    }];

    // ── con il lucchetto, per un attimo: quali percorsi cambierebbero ──
    let candidati = con_libreria(&stato, |libreria| {
        radici::candidati(&libreria.connection, &rimappature)
    })?;
    // ── fuori da ogni lucchetto: ci sono davvero? ──
    let verifiche = radici::verifica(candidati, None, &|percorsi| {
        volumi::esistenze(percorsi, volumi::SCADENZA_RISOLUZIONE)
    });

    // ── con il lucchetto, una transazione sola ──
    con_libreria(&stato, |libreria| {
        let fallita = |err: rusqlite::Error| rymembra_app::db::codice_da_sqlite("trasloco", &err);
        let tx = libreria
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(fallita)?;
        let fatto = radici::applica_verificati(&tx, &rimappature, &|percorso| {
            verifiche.esito(percorso).unwrap_or(Esistenza::Ignota)
        })?;
        if fatto.non_verificati > 0 {
            // La transazione cade qui, e con lei le righe già riscritte.
            return Err(AppError::new(ErrorCode::FsNetworkUnavailable {
                path: Some(a.to_owned()),
            })
            .with_cause(format!(
                "{} brani non si sono potuti verificare sotto la cartella nuova: \
                 il trasloco non si fa a metà",
                fatto.non_verificati
            )));
        }

        riscrivi_le_cartelle(&tx, &rimappature)?;
        tx.commit().map_err(fallita)?;
        Ok(fatto)
    })
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
///
/// Una scelta non vuota passa le regole di forma di
/// `procura::cartella_download_ammessa`, senza toccare il disco: vedi lì.
#[tauri::command]
pub fn imposta_cartella_download(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    percorso: String,
) -> Esito<()> {
    if !percorso.trim().is_empty() {
        crate::procura::cartella_download_ammessa(&percorso).map_err(errore)?;
    }
    crate::nuvola::se_riuscito(
        &app,
        con_libreria(&stato, |libreria| {
            let scelta = percorso.trim();
            if scelta.is_empty() {
                rymembra_app::settings::forget(
                    &libreria.connection,
                    rymembra_app::settings::CHIAVE_CARTELLA_DOWNLOAD,
                )
                .map(|_| ())
            } else {
                rymembra_app::settings::write(
                    &libreria.connection,
                    rymembra_app::settings::CHIAVE_CARTELLA_DOWNLOAD,
                    scelta,
                )
            }
        })
        .map_err(errore),
    )
}

/// I due numeri di una pagina, resi innocui.
///
/// # Il difetto che chiude
///
/// `LIMIT` e `OFFSET` arrivano dalla finestra e finiscono in SQLite, che i
/// numeri negativi non li rifiuta: **li reinterpreta**. Un `OFFSET` negativo
/// vale zero — sgradevole ma innocuo — e un `LIMIT` negativo vuol dire *«senza
/// limite»*. Cioè il numero che serve a chiedere duecento righe, se per una
/// sottrazione sbagliata diventa `-1`, chiede invece la libreria intera, e lo
/// fa in silenzio: nessun errore, solo una finestra che si ferma un secondo e
/// mezzo su una libreria grande.
///
/// Si tosano e non si rifiutano: una pagina è una domanda di sola lettura, e
/// rispondere «zero righe» a chi ne ha chieste meno di zero è più utile di un
/// errore rosso in una lista che stava scorrendo.
///
/// Nessun tetto massimo, invece. Un tetto taglierebbe in silenzio le risposte
/// di chi ha davvero bisogno di tutto — l'esportazione, la nuvola — e
/// sostituirebbe un difetto con uno più difficile da vedere.
pub(crate) const fn pagina(offset: i64, limite: i64) -> (i64, i64) {
    // `i64::max` non è ancora utilizzabile in un contesto costante, e questa
    // resta `const` perché è una funzione di due numeri e niente altro.
    (
        if offset < 0 { 0 } else { offset },
        if limite < 0 { 0 } else { limite },
    )
}

/// Una pagina di cronologia d'ascolto, dal più recente.
#[tauri::command]
pub async fn cronologia(
    app: tauri::AppHandle,
    offset: i64,
    limite: i64,
) -> Esito<Vec<rymembra_app::library::VoceCronologia>> {
    let (offset, limite) = pagina(offset, limite);
    leggi_la_libreria(app, "cronologia", move |libreria| {
        rymembra_app::library::list_history(&libreria.connection, offset, limite)
    })
    .await
    .map_err(errore)
}

/// Quanti ascolti ci sono in tutto.
///
/// Separato dalla pagina per la stessa ragione di [`cerca_conteggio`]: la pagina
/// si richiede a ogni scorrimento, il totale una volta sola.
#[tauri::command]
pub async fn cronologia_conteggio(app: tauri::AppHandle) -> Esito<i64> {
    leggi_la_libreria(app, "conteggio della cronologia", |libreria| {
        rymembra_app::library::count_history(&libreria.connection)
    })
    .await
    .map_err(errore)
}

/// L'avanzamento di una scansione, mandato alla finestra mentre procede.
///
/// Una forma sola per le tre fasi, con i campi che non c'entrano a zero: la
/// finestra la riceve come evento (`scansione:avanzamento`) e come risposta a
/// [`scansione_avanzamento`], e due forme vorrebbero dire due tipi da tenere
/// d'accordo dall'altra parte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Avanzamento {
    /// A che punto è.
    pub fase: FaseScansione,
    /// File letti finora, nella fase `legge`.
    pub fatti: usize,
    /// File da leggere in tutto, nella fase `legge`; zero finché non si sa.
    pub totale: usize,
    /// Cartelle elencate finora, dalla fase `cerca` in poi.
    pub cartelle: usize,
    /// File trovati finora, musica o no, dalla fase `cerca` in poi.
    pub trovati: usize,
}

/// La fase di una scansione a mano, come la vede la finestra.
///
/// Tre e non due perché l'attesa è un momento a sé: «Scansiona» premuto mentre
/// gira una scansione partita da sola aspetta che finisca, e chi guarda deve
/// leggere che sta aspettando — non una barra ferma a zero, e non «occupato».
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FaseScansione {
    /// Aspetta il turno: c'è una scansione automatica in corso.
    Attende,
    /// La camminata sul disco: si contano cartelle e file.
    Cerca,
    /// La lettura dei file: `fatti` su `totale`.
    Legge,
}

impl Avanzamento {
    /// L'avanzamento di una fase, con i numeri a zero.
    const fn di(fase: FaseScansione) -> Self {
        Self {
            fase,
            fatti: 0,
            totale: 0,
            cartelle: 0,
            trovati: 0,
        }
    }
}

/// Una riga nel diario per ogni scansione.
///
/// `origine`: `"a mano"`, `"sorveglianza"`, `"riallineamento"`, `"download"`.
/// È la riga che risponde a «quanto è durata, dove è andato il tempo, che cosa
/// ha cambiato» senza dover ricostruire niente: una scansione invariata sul NAS
/// si riconosce da `letti=0`, una lenta dalla fase che si mangia il totale.
///
/// Sta qui e la chiamano tutti quelli che scansionano — anche
/// `crate::sorveglia` e `crate::procura` — perché il formato è uno: chi cerca
/// nel diario `[scansione]` deve trovare gli stessi campi qualunque strada
/// abbia fatto partire la passata.
pub fn racconta_scansione(origine: &str, esito: &ScanReport) {
    crate::nota!("{}", riga_della_scansione(origine, esito));
}

/// Il testo di [`racconta_scansione`]. Pura, per provarne il formato.
///
/// Manca il numero delle radici guardate, che il rapporto non porta: ci sono
/// quelle che **non** hanno risposto (`saltate=`) e i rami persi dentro quelle
/// che rispondevano (`rami=`).
fn riga_della_scansione(origine: &str, esito: &ScanReport) -> String {
    let tempi = &esito.tempi;
    format!(
        "[scansione] origine={origine} cartelle={} file={} letti={} illeggibili={}(+{} noti) \
         immagine={} inseriti={} aggiornati={} spostati={} tolti={} saltate={} rami={} \
         rinviate={}{} cammino={}ms fogli={}ms piano={}ms lettura={}ms scrittura={}ms \
         album={}ms totale={}ms",
        esito.cartelle_viste,
        esito.file_visti,
        esito.letti,
        esito.unreadable.len(),
        esito.illeggibili_noti,
        esito.brani_d_immagine,
        esito.inserted,
        esito.updated,
        esito.moved,
        esito.removed,
        esito.radici_saltate.len(),
        esito.rami_persi.len(),
        esito.rimozioni_rinviate,
        if esito.cancelled { " annullata" } else { "" },
        tempi.camminata_ms,
        tempi.fogli_ms,
        tempi.piano_ms,
        tempi.lettura_ms,
        tempi.scrittura_ms,
        tempi.aggregati_ms,
        esito.elapsed_ms,
    )
}

/// La scansione ha cambiato qualcosa in libreria.
///
/// È la domanda che decide chi svegliare dopo — manutenzione, analisi,
/// arricchimento, backup, l'albero delle cartelle — e la regola dell'accordo
/// sul turno ([`crate::stato::TurnoDiScansione::segna_cambiamenti`]).
pub(crate) const fn ha_cambiato(esito: &ScanReport) -> bool {
    esito.inserted > 0 || esito.updated > 0 || esito.moved > 0 || esito.removed > 0
}

/// La scansione può aver cambiato qualcosa: [`ha_cambiato`] se è finita,
/// sempre se è caduta.
///
/// Una scansione caduta a metà può aver già scritto dei lotti, o tolto delle
/// righe, e l'errore non porta i conti: non si sa. Nel dubbio si fa come se
/// avesse cambiato — il turno sveglia la manutenzione, la finestra rilegge —
/// perché il prezzo di una rilettura per niente è niente, e quello di una
/// libreria mostrata com'era prima sono brani che non si trovano o righe che
/// non suonano. Vale per chi scansiona prendendo il turno senza un deposito
/// che si accorga delle scritture: la scansione a mano e quella dei download.
pub(crate) const fn forse_cambiata(esito: &Result<ScanReport, AppError>) -> bool {
    match esito {
        Ok(report) => ha_cambiato(report),
        Err(_) => true,
    }
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
    /// File che non si sono potuti leggere in questa passata.
    ///
    /// Qui c'è il numero, e in [`EsitoScansione::illeggibili_quali`] ci sono i
    /// percorsi. Dalla 3.0.2 non ci sono quelli già rifiutati così com'erano
    /// in una passata di prima (migrazione 032): non si riaprono, e una
    /// riscansione invariata dice zero. Un formato che il motore non sa
    /// suonare — un WMA, un APE più vecchio della 3.99 — arriva con
    /// `playback.formatUnsupported`.
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
    /// Anche in una scansione chiesta a mano, dalla 3.0: sopra la soglia la
    /// passata non toglie niente e chiede. Quel che chiede sta in
    /// [`EsitoScansione::da_confermare`].
    pub rimozioni_rinviate: usize,
    /// Le rimozioni trattenute che aspettano un sì, in breve.
    ///
    /// `None` quando non c'è niente da chiedere. Quando c'è, la finestra mostra
    /// quante e da dove, e un sì passa il `gettone` a
    /// [`rimozioni_conferma`], che toglie **esattamente** quelle righe — tenute
    /// qui, dal lato del nucleo, e non rimandate dalla finestra.
    pub da_confermare: Option<RimozioniDaConfermare>,
    /// Le cartelle che non si sono potute leggere dentro cartelle sorvegliate
    /// che invece rispondevano: una sottocartella a permessi negati, un ramo di
    /// share che singhiozza. I brani lì sotto non sono stati né tolti né
    /// riscritti.
    pub cartelle_illeggibili: Vec<String>,
    /// Quante righe stanno sotto quelle cartelle, e sono state lasciate stare.
    pub non_verificati: usize,
    /// I numeri della libreria dopo.
    pub numeri: Counts,
}

/// Le rimozioni che una scansione a mano ha trattenuto, come le vede chi deve
/// decidere.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RimozioniDaConfermare {
    /// Il segno di **questo** elenco: la conferma lo rimanda indietro, e un
    /// elenco nel frattempo superato da un'altra scansione non si applica.
    pub gettone: u64,
    /// Quante righe se ne andrebbero in tutto.
    pub quante: usize,
    /// Per cartella sorvegliata: quante, e qualche percorso per riconoscerle.
    pub radici: Vec<RimozioniSottoUnaRadice>,
}

/// Quanti percorsi d'esempio si mostrano per cartella, nella domanda.
///
/// Cinque: abbastanza per riconoscere «ah, è l'album che ho spostato» o «ma
/// quella è tutta la cartella del NAS», pochi abbastanza da stare in una
/// finestrella senza scorrere.
const ESEMPI_PER_RADICE: usize = 5;

/// Le rimozioni trattenute dall'ultima scansione a mano, in attesa di un sì.
///
/// # Perché qui, e non rimandate dalla finestra
///
/// Perché la conferma deve togliere **esattamente** quel che la scansione ha
/// trattenuto, riga per riga, e la finestra di quell'elenco vede solo un
/// riassunto — su una share caduta sono decine di migliaia di percorsi, che
/// non ha senso spedire avanti e indietro per il gusto di rileggerli. Qui si
/// tiene l'elenco vero, e la finestra tiene un gettone.
///
/// Una statica e non uno stato gestito da Tauri per la stessa ragione di
/// `riapertura::ULTIMA_RIPRESA`: è un posto solo, per un processo solo, che
/// nessuno deve registrare all'avvio per poterlo usare.
///
/// Una scansione nuova la svuota appena prende il turno: l'elenco della
/// passata prima descriveva un disco che nel frattempo è stato riguardato.
static IN_ATTESA: std::sync::Mutex<Option<(u64, Vec<RimozioneInAttesa>)>> =
    std::sync::Mutex::new(None);

/// Il prossimo gettone. Parte da uno: lo zero non lo riceve nessun elenco.
static GETTONI: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Il posto delle rimozioni in attesa, anche se un filo è caduto tenendolo.
///
/// Un mutex avvelenato qui vuol dire soltanto che un filo è caduto mentre ci
/// scriveva un `Option`: il valore dentro è comunque intero, e rinunciare a
/// leggerlo vorrebbe dire rinunciare a una conferma per un guasto che non
/// l'ha toccata.
fn in_attesa() -> std::sync::MutexGuard<'static, Option<(u64, Vec<RimozioneInAttesa>)>> {
    IN_ATTESA
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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
    fn da(
        report: &ScanReport,
        numeri: Counts,
        da_confermare: Option<RimozioniDaConfermare>,
    ) -> Self {
        let quali = |elenco: &[rymembra_app::library::Unreadable]| -> Vec<FileSaltato> {
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
            da_confermare,
            // Per intero come le radici, e per la stessa ragione: sono
            // cartelle, non file, e di solito una o due.
            cartelle_illeggibili: report.rami_persi.clone(),
            non_verificati: report.non_verificati,
            numeri,
        }
    }

    /// Una scansione a mano lasciata mentre aspettava il turno: non ha
    /// guardato niente e non ha cambiato niente, e la finestra lo legge come
    /// ogni scansione fermata — `annullata`.
    fn lasciata(numeri: Counts) -> Self {
        Self::da(
            &ScanReport {
                cancelled: true,
                ..ScanReport::default()
            },
            numeri,
            None,
        )
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
/// # L'attesa del turno
///
/// Se una scansione partita da sola — la sorveglianza delle cartelle, la coda
/// dei download — tiene il turno, questa aspetta che finisca, **senza limite**:
/// la finestra riceve un avanzamento in fase `attende` e lo mostra, e chi non
/// vuole aspettare preme Annulla. Fino alla 3.0.1 l'attesa durava trenta
/// secondi e poi rispondeva `library.scanBusy`; con la sorveglianza di serie un
/// riallineamento di una radice di rete ne dura di più, e «occupato» a chi ha
/// premuto un tasto e sta guardando era la risposta sbagliata a una domanda
/// che si risolveva da sola.
///
/// Annulla, o l'uscita, durante l'attesa non sono errori: l'esito torna con
/// `annullata` e tutti i numeri a zero. E non fermano la scansione automatica
/// che si stava aspettando: quella non l'ha chiesta chi ha premuto Annulla.
///
/// # Errori
///
/// `library.scanBusy` se c'è già una scansione chiesta a mano — un secondo
/// clic, un'altra finestra: l'attesa è per quelle automatiche.
/// `internal.aborted` se lo stato non c'è più. Poi quel che riporta il nucleo:
/// `db.*` per il database, `fs.*` per il disco. Una radice che non risponde
/// **non** è un errore: finisce in `radiciSaltate`, e i suoi brani restano
/// dove sono.
#[tauri::command]
pub async fn scansiona(app: tauri::AppHandle) -> Esito<EsitoScansione> {
    let mano = app.clone();
    let esito = in_disparte("scansione", move || scansiona_ora(&mano))
        .await
        .map_err(errore)?;
    // Chi svegliare dopo dipende da che cosa la scansione ha cambiato, e dalla
    // 3.0.2 solo da quello: una riscansione su un disco invariato — il caso di
    // quasi tutte, con la sorveglianza accesa — non ha brani nuovi da
    // arricchire o da misurare, né statistiche nuove da salvare, né cartelle
    // nuove da disegnare. Svegliarli tutti a vuoto era lavoro e disco per
    // niente, proprio nei minuti in cui chi ha premuto vuole ascoltare.
    let cambiato = esito.as_ref().is_ok_and(|fatto| {
        fatto.inseriti > 0 || fatto.aggiornati > 0 || fatto.spostati > 0 || fatto.tolti > 0
    });
    if !cambiato {
        return esito;
    }
    // Una scansione porta dentro brani che nessuno ha mai tentato di
    // arricchire, ed è il momento in cui hanno più bisogno: appena importati
    // sono precisamente quelli con «Album sconosciuto» e nessuna copertina.
    crate::arricchimento::sporca(&app);
    crate::analisi::sporca(&app);
    // Una scansione cambia quali cartelle esistono e cosa c'è dentro:
    // l'albero del pannello «Cartelle» è derivato da `tracks`, quindi non
    // si aggiorna, si butta. Costa un contatore atomico e un evento, e non
    // ricostruisce niente — se il pannello non è aperto non c'è nemmeno un
    // albero.
    crate::cartelle::cambiate(&app);
    // Una scansione cambia quali brani esistono, quindi quali statistiche il
    // backup può ancorare: un brano ritrovato dopo una reinstallazione va
    // salvato subito, non al prossimo cuoricino.
    crate::nuvola::se_riuscito(&app, esito)
}

/// Ogni quanto «Scansiona» riprova a prendere il turno mentre aspetta.
///
/// Un decimo di secondo, come il trasloco: Annulla e l'uscita si sentono
/// subito, e il turno, quando si libera, si prende prima che lo riprenda
/// un'altra passata automatica.
const PASSO_DEL_TURNO: std::time::Duration = std::time::Duration::from_millis(100);

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
    // Da qui la scansione c'è anche per una finestra che arriva a metà —
    // anche mentre aspetta il turno — e la sua fine si dice a tutte quando la
    // guardia cade: vedi `IN_CORSO`. Una sola a mano alla volta: l'attesa è
    // per le scansioni automatiche, e un secondo «Scansiona» mentre la prima
    // lavora risponde subito, come sempre.
    let Some(segno) = ScansioneInCorso::comincia(app) else {
        return Err(errore(
            AppError::new(ErrorCode::LibraryScanBusy)
                .with_cause("c'è già una scansione chiesta a mano"),
        ));
    };
    // Il turno prima di tutto: due scansioni intrecciate si contendono
    // l'inserimento della stessa riga in `tracks`, dove `path` è UNIQUE. Si
    // libera da sé quando la guardia cade, anche uscendo per un `?`. Se lo
    // tiene una scansione automatica si aspetta, quanto serve: vedi
    // «L'attesa del turno» su [`scansiona`]. Preso il turno, la bandiera
    // d'annullamento è già azzerata: vedi [`aspetta_il_turno_a_mano`].
    let Some(turno) =
        aspetta_il_turno_a_mano(&stato, &crate::spegnimento::in_uscita, &mut |attende| {
            app.emetti("scansione:avanzamento", attende)
        })
    else {
        nota!("[scansione] origine=a mano lasciata mentre aspettava il turno");
        // I numeri di adesso, che la scansione automatica aspettata può aver
        // cambiato. Senza, la finestra resta con quelli di prima: non è un
        // guasto da dire a chi ha appena premuto Annulla.
        let numeri =
            con_libreria(&stato, |libreria| counts(&libreria.connection)).unwrap_or_default();
        return Ok(EsitoScansione::lasciata(numeri));
    };
    // Le rimozioni trattenute dalla passata prima non valgono più: questa
    // riguarda il disco da capo, e se c'è ancora qualcosa da chiedere lo
    // chiederà lei, con un gettone suo.
    *in_attesa() = None;

    // Lo store delle copertine si copia **fuori** dal lucchetto: è un percorso, e
    // il nucleo se lo porta dietro su fili che possono sopravvivere alla scadenza
    // del file che stavano leggendo.
    let copertine = stato.copertine().map_err(errore)?;
    let roots =
        con_libreria(&stato, |libreria| leggi_cartelle(&libreria.connection)).map_err(errore)?;

    let fermare = &*stato;
    // La stessa bandiera che legge il callback dell'avanzamento, più l'uscita.
    // Il nucleo la guarda durante la camminata e la lettura dei fogli, e poi fra
    // un file e l'altro. L'uscita entra qui e non solo nella bandiera perché
    // una chiusura arrivata a scansione già partita deve fermarla anche se
    // nessuno ha premuto «Annulla».
    let fermati = || fermare.scansione_fermata() || crate::spegnimento::in_uscita();
    let scan = Scan {
        // Il disco e il cloud: le radici di OneDrive camminano sullo specchio.
        files: crate::nuvola_musica::archivi(app),
        covers: copertine,
        roots: &roots,
        // Qui le cartelle guardate sono quelle sorvegliate.
        radici_della_libreria: &roots,
        rules: PathRules::for_current_platform(),
        // Questa scansione l'ha chiesta qualcuno che è davanti alla finestra:
        // se toglierebbe troppo in una volta non toglie, e chiede. Spento qui
        // vuol dire «c'è chi può dire sì», e il nucleo restituisce le righe
        // trattenute una per una invece di un numero solo.
        prudente: false,
        scadenze: Scadenze::default(),
        fermati: Some(&fermati),
    };
    let mut deposito = DepositoStato::nuovo(&stato);
    let mut ultimo = 0usize;
    // I conti della camminata restano nell'avanzamento anche durante la
    // lettura: chi guarda li ha appena letti, e non devono tornare a zero.
    let mut camminata = (0usize, 0usize);
    let esito = scan.run_con(&mut deposito, |passo| {
        let avanzamento = match passo {
            // Il nucleo la racconta già al più quattro volte al secondo: ogni
            // racconto è un evento.
            ScanAvanzamento::Camminata { cartelle, file } => {
                camminata = (cartelle, file);
                Some(Avanzamento {
                    fase: FaseScansione::Cerca,
                    fatti: 0,
                    totale: 0,
                    cartelle,
                    trovati: file,
                })
            }
            // Non a ogni file: mandare un evento per ognuno di 1421 file
            // inonderebbe il canale IPC per disegnare una barra che si muove di
            // meno di un pixel per volta.
            ScanAvanzamento::Lettura { fatti, totale } => {
                (fatti == totale || fatti.saturating_sub(ultimo) >= 25).then(|| {
                    ultimo = fatti;
                    Avanzamento {
                        fase: FaseScansione::Legge,
                        fatti,
                        totale,
                        cartelle: camminata.0,
                        trovati: camminata.1,
                    }
                })
            }
        };
        if let Some(avanzamento) = avanzamento {
            segno.avanza(avanzamento);
            app.emetti("scansione:avanzamento", avanzamento);
        }
        if fermare.scansione_fermata() {
            std::ops::ControlFlow::Break(())
        } else {
            std::ops::ControlFlow::Continue(())
        }
    });
    // Anche se è caduta: vedi `forse_cambiata`. La manutenzione svegliata per
    // niente costa meno di copertine orfane che nessuno pulisce.
    if forse_cambiata(&esito) {
        turno.segna_cambiamenti();
    }
    let report = match esito {
        Ok(report) => report,
        Err(guasto) => {
            // Caduta a metà, può aver già scritto dei lotti o tolto delle
            // righe: la finestra rilegge qui, perché `scansiona` dopo un
            // errore non sveglia nessuno. Senza, mostrerebbe la libreria di
            // prima fino alla prossima scansione riuscita.
            crate::cartelle::cambiate(app);
            nota!(
                "[scansione] origine=a mano fallita codice={} causa={}",
                guasto.code().kind().code(),
                guasto.cause().unwrap_or("—")
            );
            return Err(errore(guasto));
        }
    };
    racconta_scansione("a mano", &report);
    crate::nuvola_musica::rimetti_in_fila(app, &roots, report.rimasti);
    // I numeri in una presa a parte, dopo: il deposito è appena stato
    // restituito, e chiederli da dentro la scansione vorrebbe dire tenerlo per
    // una `COUNT(*)` che con la scrittura non c'entra niente.
    let numeri = con_libreria(&stato, |libreria| counts(&libreria.connection)).map_err(errore)?;
    let da_confermare = if report.rimozioni_in_attesa.is_empty() {
        None
    } else {
        let gettone = GETTONI.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let riassunto = RimozioniDaConfermare {
            gettone,
            quante: report.rimozioni_in_attesa.len(),
            radici: riassumi_rimozioni(
                &report.rimozioni_in_attesa,
                &roots,
                PathRules::for_current_platform(),
                ESEMPI_PER_RADICE,
            ),
        };
        *in_attesa() = Some((gettone, report.rimozioni_in_attesa.clone()));
        Some(riassunto)
    };
    Ok(EsitoScansione::da(&report, numeri, da_confermare))
}

/// Il turno per la scansione a mano: subito se è libero, altrimenti quando la
/// scansione automatica che lo tiene ha finito.
///
/// `None` se nel frattempo qualcuno ha premuto Annulla
/// ([`annulla_scansione`]) o il programma si sta chiudendo: chi chiama
/// risponde `annullata`, e non comincia niente.
///
/// # L'attesa si dice una volta
///
/// Al primo tentativo andato a vuoto parte un avanzamento in fase `attende`, e
/// lo stato di [`IN_CORSO`] lo ripete a chi arriva dopo. Non uno per giro:
/// dieci eventi al secondo per dire la stessa cosa sarebbero solo rumore.
///
/// # Il passaggio dall'attesa al lavoro, sotto il lucchetto di [`IN_CORSO`]
///
/// Annulla fa due cose diverse secondo la fase: durante l'attesa lascia
/// l'attesa ([`ATTESA_LASCIATA`]), durante il lavoro ferma la scansione
/// (`Stato::ferma_scansione`). Se la fase cambiasse fra la sua domanda e il suo
/// gesto, un Annulla andrebbe perso — lasciata un'attesa già finita, o alzata
/// una bandiera che il passaggio azzera subito dopo. Per questo il passaggio
/// — guardare se l'attesa è stata lasciata, azzerare la bandiera
/// d'annullamento, segnare la fase `cerca` — avviene tutto dentro quel
/// lucchetto, e Annulla guarda e agisce dentro lo stesso.
///
/// `in_uscita` e `annuncia` arrivano da fuori — l'uscita del programma e
/// l'evento per la finestra — solo perché le prove non hanno né l'una né
/// l'altra: la bandiera d'uscita è di tutto il binario di prova.
fn aspetta_il_turno_a_mano<'a>(
    stato: &'a Stato,
    in_uscita: &dyn Fn() -> bool,
    annuncia: &mut dyn FnMut(Avanzamento),
) -> Option<crate::stato::TurnoDiScansione<'a>> {
    let mut detta = false;
    loop {
        let turno = stato.turno_di_scansione();
        {
            let mut posto = in_corso();
            if ATTESA_LASCIATA.load(std::sync::atomic::Ordering::Relaxed) || in_uscita() {
                // Il turno, se preso, cade qui senza aver segnato niente: non
                // sveglia nessuno.
                return None;
            }
            if let Some(turno) = turno {
                // Prima di leggere: un annullamento arrivato dopo la fine della
                // scansione precedente fermerebbe questa al primo file. Non se
                // si sta uscendo — lo esclude il controllo qui sopra — perché
                // lì la bandiera l'ha alzata l'uscita, e azzerarla farebbe
                // ripartire il filo che tutto il resto aspetta che se ne vada.
                stato.riprendi_scansioni();
                *posto = Some(Avanzamento::di(FaseScansione::Cerca));
                return Some(turno);
            }
        }
        if !detta {
            detta = true;
            let attende = Avanzamento::di(FaseScansione::Attende);
            *in_corso() = Some(attende);
            annuncia(attende);
        }
        std::thread::sleep(PASSO_DEL_TURNO);
    }
}

/// Com'è andata una conferma di rimozioni.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EsitoRimozioni {
    /// Quante righe sono state tolte davvero.
    ///
    /// Può essere meno di quante se ne erano chieste: una riga cambiata fra la
    /// domanda e la risposta non si riconosce e non si tocca. Zero se il
    /// gettone non era più quello buono — un'altra scansione nel frattempo.
    pub tolti: usize,
    /// I numeri della libreria dopo.
    pub numeri: Counts,
}

/// Toglie le rimozioni che l'ultima scansione a mano aveva trattenuto.
///
/// Non cammina il disco: toglie **esattamente** le righe trattenute, per
/// identificativo e percorso — vedi `rymembra_app::library::applica_rimozioni`.
/// Quel che quelle righe portavano con sé — ascolti, voto, preferito, posto
/// nelle playlist — resta messo da parte per novanta giorni (migrazione 028) e
/// torna se il file ricompare.
///
/// Prende il turno delle scansioni: togliere mentre un'altra passata sta
/// scrivendo vorrebbe dire intrecciare due decisioni sullo stesso disco.
///
/// # Errori
///
/// `library.scanBusy` se una scansione è in corso; `internal.aborted` se lo
/// stato non c'è più; `db.*` se la scrittura fallisce — e allora l'elenco
/// resta in attesa, e un secondo sì ci riprova. Un gettone superato non è un
/// errore: non toglie niente, e lo dice con `tolti: 0`.
#[tauri::command]
pub async fn rimozioni_conferma(app: tauri::AppHandle, gettone: u64) -> Esito<EsitoRimozioni> {
    let mano = app.clone();
    let esito = in_disparte("conferma delle rimozioni", move || {
        let Some(stato) = mano.try_state::<Stato>() else {
            return Err(AppError::new(ErrorCode::InternalAborted {
                what: Some("conferma delle rimozioni".to_owned()),
            })
            .with_cause("la libreria non è più fra gli stati gestiti"));
        };
        let Some(turno) = stato.turno_di_scansione() else {
            return Err(AppError::new(ErrorCode::LibraryScanBusy));
        };
        // Si prende l'elenco solo se è quello confermato; resta in mano fino a
        // scrittura riuscita, e se la scrittura fallisce torna al suo posto.
        let elenco = in_attesa().take_if(|(atteso, _)| *atteso == gettone);
        let tolti = match elenco {
            None => 0,
            Some((segno, righe)) => {
                let fatto = con_libreria(&stato, |libreria| {
                    applica_rimozioni(&mut libreria.connection, &righe)
                });
                match fatto {
                    Ok(tolti) => tolti,
                    Err(guasto) => {
                        *in_attesa() = Some((segno, righe));
                        return Err(guasto);
                    }
                }
            }
        };
        // Righe tolte sono copertine forse rimaste senza nessuno: la
        // manutenzione va svegliata come dopo una scansione che toglie.
        if tolti > 0 {
            turno.segna_cambiamenti();
        }
        let numeri = con_libreria(&stato, |libreria| counts(&libreria.connection))?;
        Ok(EsitoRimozioni { tolti, numeri })
    })
    .await
    .map_err(errore)?
    .map_err(errore);
    if esito.as_ref().is_ok_and(|fatto| fatto.tolti > 0) {
        // Come dopo una scansione: l'albero delle cartelle è derivato da
        // `tracks`, e il backup deve sapere che la libreria è cambiata.
        crate::cartelle::cambiate(&app);
    }
    crate::nuvola::se_riuscito(&app, esito)
}

/// Lascia cadere le rimozioni in attesa: la risposta è stata no.
///
/// Sincrono e senza lucchetto della libreria: svuota un posto in memoria e
/// basta. Le righe restano dove sono, e la prossima scansione — se i file
/// mancano ancora — chiederà di nuovo.
#[tauri::command]
pub fn rimozioni_scarta() {
    *in_attesa() = None;
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
///
/// # Durante l'attesa del turno
///
/// Se la scansione a mano sta ancora aspettando che finisca una automatica, il
/// bit da alzare è un altro: [`ATTESA_LASCIATA`], che fa smettere di aspettare
/// e basta. Quello di sempre lo leggono anche le scansioni automatiche, e
/// alzarlo lì fermerebbe la passata della sorveglianza che nessuno ha chiesto
/// di fermare. La domanda sulla fase e il gesto stanno dentro lo stesso
/// lucchetto di [`IN_CORSO`], che si tiene per il tempo di una copia: vedi
/// [`aspetta_il_turno_a_mano`] per perché.
#[tauri::command]
pub fn annulla_scansione(stato: State<'_, Stato>) -> Esito<()> {
    annulla(&stato);
    Ok(())
}

/// Il corpo di [`annulla_scansione`], senza Tauri intorno.
fn annulla(stato: &Stato) {
    let posto = in_corso();
    if posto.is_some_and(|adesso| adesso.fase == FaseScansione::Attende) {
        ATTESA_LASCIATA.store(true, std::sync::atomic::Ordering::Relaxed);
    } else {
        stato.ferma_scansione();
    }
}

/// Chi ha premuto Annulla mentre la scansione a mano aspettava il turno.
///
/// Si scrive e si azzera solo col lucchetto di [`IN_CORSO`] in mano: vedi
/// [`aspetta_il_turno_a_mano`]. Un atomico e non un campo dentro quel posto
/// perché [`scansione_avanzamento`] restituisce il posto così com'è, e questo
/// bit alla finestra non serve.
static ATTESA_LASCIATA: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// ── la scansione vista da una finestra che arriva a metà ────────────────────

/// L'ultimo avanzamento della scansione a mano in corso, o `None` se non ce
/// n'è una.
///
/// # Perché serve
///
/// L'avanzamento la finestra lo riceve come evento, e la fine come **risposta**
/// a [`scansiona`]. Una finestra ricaricata a metà scansione riceve ancora gli
/// eventi ma non la risposta, che spettava alla finestra di prima e non arriva
/// più a nessuno: la barra restava «in corso» per sempre, e con lei «Annulla»
/// su una scansione finita da un pezzo. Con questo posto la finestra nuova
/// chiede all'apertura com'è davvero ([`scansione_avanzamento`]), e la fine la
/// sente da `scansione:finita`, che va a tutti e non dipende da chi l'ha
/// chiesta.
///
/// # Solo quella a mano
///
/// È l'unica che manda l'avanzamento: quelle della sorveglianza e della coda
/// dei download una barra non l'hanno mai avuta. Il turno che condividono
/// ([`Stato::scansione_in_corso`]) non andava bene per questa domanda: lo prende
/// anche la pulizia delle copertine, e leggerlo avrebbe acceso una barra per
/// lavori che non ne hanno una.
///
/// Una statica come [`IN_ATTESA`], e per la stessa ragione: un posto solo, per
/// un processo solo.
static IN_CORSO: std::sync::Mutex<Option<Avanzamento>> = std::sync::Mutex::new(None);

/// Il posto della scansione in corso, anche se un filo è caduto tenendolo:
/// dentro c'è un `Option` di qualche numero, intero comunque.
fn in_corso() -> std::sync::MutexGuard<'static, Option<Avanzamento>> {
    IN_CORSO
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// La scansione a mano finché dura: la segna in [`IN_CORSO`] e, cadendo, la
/// toglie e manda `scansione:finita`.
///
/// Una guardia e non due righe in cima e in fondo a [`scansiona_ora`]: quel
/// corpo esce in cinque punti con un `?`, e la fine va detta in tutti — una
/// fine detta solo quando va bene è la barra eterna di prima, spostata sul
/// caso dell'errore.
struct ScansioneInCorso<'a> {
    /// A chi dire che è finita.
    app: &'a tauri::AppHandle,
}

impl<'a> ScansioneInCorso<'a> {
    /// La scansione comincia, se non ce n'è già una a mano: `None` vuol dire
    /// che c'è.
    ///
    /// Comincia in fase `attende`, senza dirlo a nessuno: finché non ha il
    /// turno sta aspettando, anche se per un istante solo, e un Annulla in
    /// quell'istante deve lasciare l'attesa e non fermare la scansione
    /// automatica che magari tiene il turno. L'evento `attende` parte solo se
    /// l'attesa c'è davvero: vedi [`aspetta_il_turno_a_mano`].
    fn comincia(app: &'a tauri::AppHandle) -> Option<Self> {
        prendi_il_posto().then_some(Self { app })
    }

    /// L'ultimo avanzamento mandato, per chi arriverà dopo.
    fn avanza(&self, avanzamento: Avanzamento) {
        *in_corso() = Some(avanzamento);
    }
}

/// Segna in [`IN_CORSO`] una scansione a mano che comincia, in fase
/// `attende`; `false` se ce n'è già una. Il corpo di
/// [`ScansioneInCorso::comincia`], senza Tauri intorno.
fn prendi_il_posto() -> bool {
    let mut posto = in_corso();
    if posto.is_some() {
        return false;
    }
    *posto = Some(Avanzamento::di(FaseScansione::Attende));
    ATTESA_LASCIATA.store(false, std::sync::atomic::Ordering::Relaxed);
    true
}

impl Drop for ScansioneInCorso<'_> {
    fn drop(&mut self) {
        *in_corso() = None;
        // Anche alla finestra che la scansione l'ha chiesta, che però si regola
        // sulla risposta e questo lo lascia cadere: vedi `scansioneMia` in
        // `App.tsx`.
        self.app.emetti("scansione:finita", ());
    }
}

/// Com'è la scansione a mano adesso: l'ultimo avanzamento, o `None` se non ce
/// n'è una in corso.
///
/// La chiede la finestra all'apertura: vedi [`IN_CORSO`]. Sincrono e senza il
/// lucchetto della libreria, come [`annulla_scansione`]: legge qualche numero
/// in memoria. Ha la forma dell'evento `scansione:avanzamento`, fase compresa:
/// una finestra che arriva mentre «Scansiona» aspetta il turno legge
/// `attende`.
#[tauri::command]
pub fn scansione_avanzamento() -> Option<Avanzamento> {
    *in_corso()
}

/// Una pagina di risultati.
///
/// In disparte: una ricerca per prefisso su una libreria grande vale decine di
/// millisecondi, che sul filo della finestra si vedono mentre si scrive.
#[tauri::command]
pub async fn cerca(
    app: tauri::AppHandle,
    query: String,
    offset: i64,
    limite: i64,
) -> Esito<Vec<TrackSummary>> {
    let (offset, limite) = pagina(offset, limite);
    leggi_la_libreria(app, "ricerca", move |libreria| {
        search(&libreria.connection, &query, offset, limite)
    })
    .await
    .map_err(errore)
}

/// Quanti risultati ha questa ricerca in tutto.
///
/// Separato dalla pagina e non un campo del risultato: la pagina si chiede a
/// ogni scorrimento, il conteggio una volta per query. Metterli insieme
/// vorrebbe dire rifare la `COUNT(*)` a ogni fetta.
#[tauri::command]
pub async fn cerca_conteggio(app: tauri::AppHandle, query: String) -> Esito<i64> {
    leggi_la_libreria(app, "conteggio della ricerca", move |libreria| {
        rymembra_app::library::search_count(&libreria.connection, &query)
    })
    .await
    .map_err(errore)
}

/// Una pagina di preferiti.
#[tauri::command]
pub async fn preferiti(
    app: tauri::AppHandle,
    offset: i64,
    limite: i64,
) -> Esito<Vec<TrackSummary>> {
    let (offset, limite) = pagina(offset, limite);
    leggi_la_libreria(app, "preferiti", move |libreria| {
        rymembra_app::library::list_liked(&libreria.connection, offset, limite)
    })
    .await
    .map_err(errore)
}

/// Una pagina degli album di un artista.
#[tauri::command]
pub async fn album_artista(
    app: tauri::AppHandle,
    nome: String,
    offset: i64,
    limite: i64,
) -> Esito<Vec<AlbumSummary>> {
    let (offset, limite) = pagina(offset, limite);
    leggi_la_libreria(app, "album di un artista", move |libreria| {
        rymembra_app::library::albums_by_artist(&libreria.connection, &nome, offset, limite)
    })
    .await
    .map_err(errore)
}

/// Una pagina di brani.
#[tauri::command]
pub async fn brani(
    app: tauri::AppHandle,
    ordine: String,
    offset: i64,
    limite: i64,
) -> Esito<Vec<TrackSummary>> {
    // Il nome dell'ordinamento si traduce qui in un valore chiuso: quel che
    // arriva dalla finestra non deve poter raggiungere una clausola SQL.
    //
    // Un nome sconosciuto vale «scaffale» e non è un errore: l'insieme è chiuso
    // e l'unico che lo scrive è questa applicazione, quindi un valore fuori
    // elenco è un refuso nostro o una preferenza salvata da una versione che
    // chiamava le cose in un altro modo — e in tutti e due i casi un elenco che
    // si disegna nell'ordine di sempre è meglio di un elenco che non si
    // disegna. Ma se ne lascia traccia: prima spariva, e un refuso che non
    // lascia traccia è un refuso che resta.
    let ordine = match ordine.as_str() {
        "recenti" => TrackOrder::RecentlyAdded,
        "ascoltati" => TrackOrder::MostPlayed,
        "titolo" => TrackOrder::Title,
        "scaffale" | "shelf" => TrackOrder::Shelf,
        altro => {
            crate::nota!("[brani] ordinamento sconosciuto «{altro}»: vale «scaffale»");
            TrackOrder::Shelf
        }
    };
    let (offset, limite) = pagina(offset, limite);
    leggi_la_libreria(app, "elenco dei brani", move |libreria| {
        list_tracks(&libreria.connection, ordine, offset, limite)
    })
    .await
    .map_err(errore)
}

/// Una pagina di album.
#[tauri::command]
pub async fn album(app: tauri::AppHandle, offset: i64, limite: i64) -> Esito<Vec<AlbumSummary>> {
    let (offset, limite) = pagina(offset, limite);
    leggi_la_libreria(app, "elenco degli album", move |libreria| {
        list_albums(&libreria.connection, offset, limite)
    })
    .await
    .map_err(errore)
}

/// Tutti gli artisti, in ordine alfabetico.
///
/// Senza offset né limite, al contrario di `album` e `brani`: gli artisti sono
/// pochi e la vista li mostra tutti con un indice alfabetico laterale. Il
/// giorno in cui non fosse più vero, il posto in cui aggiungere la pagina è
/// `list_artists`, non qui.
///
/// In disparte: è il comando più lungo fra le letture, un raggruppamento su
/// tutta la tabella — sopra i cinquanta millisecondi su cinquantamila brani.
#[tauri::command]
pub async fn artisti(app: tauri::AppHandle) -> Esito<Vec<ArtistSummary>> {
    leggi_la_libreria(app, "elenco degli artisti", |libreria| {
        list_artists(&libreria.connection)
    })
    .await
    .map_err(errore)
}

/// I brani di un album.
#[tauri::command]
pub async fn brani_album(app: tauri::AppHandle, chiave: String) -> Esito<Vec<TrackSummary>> {
    leggi_la_libreria(app, "brani di un album", move |libreria| {
        album_tracks(&libreria.connection, &chiave)
    })
    .await
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
    // Un id di sessione non è una riga: vedi `riproduzione::ospiti`.
    let id = crate::riproduzione::riga_di_libreria(id).map_err(errore)?;
    let esito = con_libreria(&stato, |libreria| {
        let now = adesso_ms();
        libreria
            .connection
            .execute(
                "UPDATE tracks SET liked = ?2, liked_at = ?3, stats_updated_at = ?3 WHERE id = ?1",
                rusqlite::params![id, i64::from(valore), now],
            )
            .map(|_| ())
            // Dal classificatore e non costruito a mano: un database occupato o
            // una share staccata devono dirlo, non «il database ha rifiutato
            // una richiesta».
            .map_err(|err| rymembra_app::db::codice_da_sqlite("preferito", &err))
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Apre la cartella di un brano in Esplora risorse, con il file selezionato.
///
/// # Perché prende un identificativo e non un percorso
///
/// Perché un comando che apre il gestore file su un percorso qualunque è un
/// comando che la finestra potrebbe usare per aprire qualunque cosa. Con un
/// identificativo il percorso lo legge il nucleo da `tracks`, e quel che si può
/// mostrare è soltanto un file che la libreria conosce già.
///
/// Sul pool bloccante ([`in_disparte`]): su una cartella di rete il gestore
/// file può metterci secondi a rispondere — su una share spenta, il timeout di
/// Windows — e non sul filo della finestra, né su un worker del runtime, che
/// è prestato a tutti i comandi asincroni: due o tre clic con il NAS spento li
/// fermavano tutti.
///
/// # Errori
///
/// `library.trackNotFound` se il brano non c'è più; `fs.notFound` se il file
/// non c'è o il gestore file rifiuta di aprirsi.
#[tauri::command]
pub async fn brano_mostra_nella_cartella(app: tauri::AppHandle, id: i64) -> Esito<()> {
    // Un id di sessione non è una riga: vedi `riproduzione::ospiti`.
    let id = crate::riproduzione::riga_di_libreria(id).map_err(errore)?;
    in_disparte("mostra nella cartella", move || {
        mostra_nella_cartella(&app, id)
    })
    .await
    .map_err(errore)?
}

/// Il corpo di [`brano_mostra_nella_cartella`], sul pool bloccante.
fn mostra_nella_cartella(app: &tauri::AppHandle, id: i64) -> Esito<()> {
    let stato = crate::importa::gestito::<Stato>(app, "mostra nella cartella").map_err(errore)?;
    let percorso = con_libreria(&stato, |libreria| {
        file_da_mostrare(&libreria.connection, id)
    })
    .map_err(errore)?;
    // Un brano del cloud non sta in nessuna cartella di questo PC: la finestra
    // al suo posto offre «Apri su OneDrive» (`brano_apri_sul_web`), e il
    // comando lo rifiuta lo stesso, perché una porta non si fida di chi bussa.
    if rymembra_domain::nuvola::e_nuvola(&percorso) {
        return Err(errore(AppError::new(ErrorCode::CloudReadOnly)));
    }
    tauri_plugin_opener::reveal_item_in_dir(&percorso).map_err(|err| {
        errore(
            AppError::new(ErrorCode::FsNotFound {
                path: percorso.clone(),
            })
            .with_cause(err.to_string()),
        )
    })
}

/// Il file da mostrare per un brano: il suo, o l'immagine CD di cui è un pezzo.
///
/// Un brano d'immagine ha in tabella un percorso `…\Album.cue|03`, che non è un
/// file: chiederlo a Esplora risorse vorrebbe dire una cartella che non si apre,
/// o un errore che dà la colpa al disco. Il file vero è l'immagine, nella
/// colonna `immagine`, ed è quello che ha senso vedere selezionato — accanto
/// al suo `.cue`.
///
/// # Errori
///
/// `library.trackNotFound` se il brano non c'è più;
/// `playback.sourceUnavailable` se è di catalogo; `db.*` se la lettura fallisce.
pub(crate) fn file_da_mostrare(
    connection: &rusqlite::Connection,
    id: i64,
) -> Result<String, AppError> {
    use rusqlite::OptionalExtension as _;
    let brano = rymembra_app::library::read_summary(connection, id)?
        .ok_or_else(|| AppError::new(ErrorCode::LibraryTrackNotFound { track_id: Some(id) }))?;
    // Un brano di catalogo non sta in nessuna cartella. La finestra nasconde
    // già questo gesto quando il brano è un flusso; il controllo resta
    // perché un comando è una porta, e una porta non si fida di chi bussa.
    let percorso = brano.path.ok_or_else(|| {
        AppError::new(ErrorCode::PlaybackSourceUnavailable {
            track_id: Some(id),
            path: None,
        })
        .with_cause("un brano di catalogo non sta in nessuna cartella".to_owned())
    })?;
    let immagine: Option<String> = connection
        .query_row("SELECT immagine FROM tracks WHERE id = ?1", [id], |riga| {
            riga.get(0)
        })
        .optional()
        .map_err(|err| rymembra_app::db::codice_da_sqlite("immagine di un brano", &err))?
        .flatten();
    Ok(immagine.unwrap_or(percorso))
}

/// Apre nel browser la pagina pubblica di un brano di catalogo.
///
/// # Perché esiste accanto a `esplora::esplora_apri_pagina`
///
/// Perché quella chiede l'indirizzo all'**ultima ricerca**, e un brano tenuto
/// in libreria l'ultima ricerca non se la ricorda: è entrato settimane fa, e
/// la finestra che vuole aprirne la pagina è l'elenco dei brani, non Esplora.
/// Qui la pagina la legge il nucleo da `tracks.fonte_pagina`.
///
/// # Perché prende un identificativo e non un indirizzo
///
/// Stessa regola di [`brano_mostra_nella_cartella`] e di
/// `procura::cerca_dove_comprare`: aprire un indirizzo nel browser di sistema è
/// l'unica capacità pericolosa che la finestra ha, e lasciarle scegliere quale
/// indirizzo vorrebbe dire dargliela intera. La finestra nomina un brano;
/// l'indirizzo è quello che il catalogo aveva dichiarato quando quel brano è
/// entrato, e prima di aprirlo passa comunque dall'allowlist di
/// `rymembra_catalogo::riconosci` — lo stesso cancello da cui passano i byte da
/// suonare.
///
/// `(async)`: aprire il browser di sistema può metterci un istante, e non sul
/// filo della finestra.
///
/// # Errori
///
/// `library.trackNotFound` se il brano non c'è più;
/// `download.unrecognizedUrl` se non dichiara una pagina pubblica, o se quella
/// pagina non è in un posto dove Rymembra sia invitato; `internal.aborted` se il
/// browser non si apre.
#[tauri::command(async)]
pub fn brano_apri_pagina(stato: State<'_, Stato>, id: i64) -> Esito<()> {
    // Un id di sessione non è una riga: vedi `riproduzione::ospiti`.
    let id = crate::riproduzione::riga_di_libreria(id).map_err(errore)?;
    let pagina = con_libreria(&stato, |libreria| {
        let brano = rymembra_app::library::read_summary(&libreria.connection, id)?
            .ok_or_else(|| AppError::new(ErrorCode::LibraryTrackNotFound { track_id: Some(id) }))?;
        brano.fonte_pagina.ok_or_else(|| {
            AppError::new(ErrorCode::DownloadUnrecognizedUrl)
                .with_cause("questo brano non dichiara una pagina pubblica".to_owned())
        })
    })
    .map_err(errore)?;
    if rymembra_catalogo::riconosci(&pagina).is_none() {
        return Err(errore(
            AppError::new(ErrorCode::DownloadUnrecognizedUrl).with_cause(
                "questa pagina non è in un posto dove Rymembra sia invitato".to_owned(),
            ),
        ));
    }
    tauri_plugin_opener::open_url(&pagina, None::<&str>).map_err(|err| {
        errore(
            AppError::new(ErrorCode::InternalAborted {
                what: Some("apertura del browser".to_owned()),
            })
            .with_cause(err.to_string()),
        )
    })
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
    // Un id di sessione non è una riga: vedi `riproduzione::ospiti`.
    let id = crate::riproduzione::riga_di_libreria(id).map_err(errore)?;
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
                // un'assenza — vedi `rymembra_app::sincronia`.
                "UPDATE tracks SET rating = ?2, stats_updated_at = ?3, rating_at = ?3
                  WHERE id = ?1",
                rusqlite::params![id, stelle, now],
            )
            .map(|_| ())
            .map_err(|err| rymembra_app::db::codice_da_sqlite("valutazione", &err))
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Cosa porterebbe l'importazione dal vecchio database.
///
/// Sul pool bloccante ([`in_disparte`]): apre e legge un intero database dal
/// disco — scelto dall'utente, e può stare su una share — e la regola dei
/// comandi che toccano il filesystem vale anche per lui.
#[tauri::command]
pub async fn piano_importazione(
    app: tauri::AppHandle,
    percorso: String,
) -> Esito<import_legacy::ImportReport> {
    in_disparte("piano d'importazione", move || {
        importa_interno(&app, &percorso, false)
    })
    .await
    .map_err(errore)?
}

/// Importa dal vecchio database.
///
/// Sul pool bloccante per la stessa ragione di [`piano_importazione`].
#[tauri::command]
pub async fn importa(
    app: tauri::AppHandle,
    percorso: String,
) -> Esito<import_legacy::ImportReport> {
    in_disparte("importazione", move || {
        crate::nuvola::se_riuscito(&app, importa_interno(&app, &percorso, true))
    })
    .await
    .map_err(errore)?
}

/// Il corpo di [`piano_importazione`] e di [`importa`], sul pool bloccante.
///
/// Il database vecchio si apre **prima** di prendere la libreria, e non
/// dentro: aprirlo su una share lenta con il lucchetto in mano fermava ogni
/// comando che lo chiede, anche quelli che di worker liberi ne trovavano.
fn importa_interno(
    app: &tauri::AppHandle,
    percorso: &str,
    esegui: bool,
) -> Esito<import_legacy::ImportReport> {
    let stato = crate::importa::gestito::<Stato>(app, "importazione").map_err(errore)?;
    let legacy = import_legacy::open_legacy(std::path::Path::new(percorso)).map_err(errore)?;
    con_libreria(&stato, |libreria| {
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
    let Some(scelto) = rymembra_app::preferenze::Tema::dal_nome(&tema) else {
        return Err(errore(AppError::new(
            rymembra_domain::errors::ErrorCode::IpcPayloadInvalid {
                channel: "imposta_tema".to_owned(),
                detail: Some(format!("«{tema}» non è uno dei tre temi")),
            },
        )));
    };
    crate::nuvola::se_riuscito(
        &app,
        con_libreria(&stato, |libreria| {
            rymembra_app::preferenze::imposta_tema(&libreria.connection, scelto)
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
/// scrive `data-motion-utente` sulla radice: il rimedio è già in
/// `stile/movimento.css`, accanto a quello di `prefers-reduced-motion`, e non
/// se ne scrive un secondo.
#[tauri::command]
pub fn movimento_ridotto(stato: State<'_, Stato>) -> Esito<bool> {
    con_libreria(&stato, |libreria| {
        rymembra_app::preferenze::movimento_ridotto(&libreria.connection)
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
            rymembra_app::preferenze::imposta_movimento_ridotto(&libreria.connection, ridotto)
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
            rymembra_app::preferenze::imposta_lingua(&libreria.connection, &lingua)
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
            rymembra_app::preferenze::imposta_scorciatoie(&libreria.connection, &scorciatoie)
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
            aperte: rymembra_app::preferenze::cartelle_aperte(&libreria.connection)?,
            scelta: rymembra_app::preferenze::cartella_scelta(&libreria.connection)?,
        })
    })
    .map_err(errore)
}

/// Scrive com'è rimasto il pannello «Cartelle».
///
/// # Perché non passa da `nuvola::se_riuscito`
///
/// Perché sono percorsi di *questa* macchina — vedi il preambolo di
/// `rymembra_app::preferenze` — e una passata di backup vale quel che porta
/// altrove. Farla partire a ogni nodo aperto vorrebbe dire svegliare la rete
/// per un dato che sull'altro computer non si può nemmeno usare.
#[tauri::command]
pub fn imposta_cartelle_ui(
    stato: State<'_, Stato>,
    aperte: Vec<String>,
    scelta: String,
) -> Esito<()> {
    con_libreria(&stato, |libreria| {
        rymembra_app::preferenze::imposta_cartelle_aperte(&libreria.connection, &aperte)?;
        rymembra_app::preferenze::imposta_cartella_scelta(&libreria.connection, &scelta)
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
///
/// In disparte: cinque letture insieme, e quella dei trascurati passa dal
/// motore delle regole su tutta la libreria.
#[tauri::command]
pub async fn casa(app: tauri::AppHandle) -> Esito<Casa> {
    use rymembra_domain::regole::{
        Campo, Combinazione, Insieme, Operatore, Ordinamento, Regola, Valore,
    };

    leggi_la_libreria(app, "Home", |libreria| {
        let connection = &libreria.connection;

        // Dove ci si era fermati: la coda di ieri sa quale brano, e la
        // posizione sta in una chiave sua.
        let istantanea = rymembra_app::playback::load_queue(connection)?;
        let corrente = rymembra_domain::queue::Queue::restore(istantanea).current();
        let riprendi = match corrente {
            Some(id) => rymembra_app::library::read_summary(connection, id)?,
            None => None,
        };
        // La posizione si legge solo se c'è un brano a cui appartiene: da sola
        // sarebbe un numero senza significato, e mostrarla accanto al brano
        // sbagliato è peggio che non mostrarla.
        let riprendi_ms = match &riprendi {
            Some(brano) => rymembra_app::playback::load_posizione(connection, brano.id)?,
            None => 0,
        };

        // Solo gli ascoltati, con una data, i dodici più recenti: la domanda
        // passa dall'indice delle date invece di ordinare tutta la libreria a
        // ogni cambio di brano con la Home aperta. Vedi
        // `rymembra_app::library::ascoltati_di_recente`.
        let recenti = rymembra_app::library::ascoltati_di_recente(connection, RIPIANO)?;

        let aggiunti = recently_added_albums(connection, RIPIANO)?;

        // I trascurati passano dal motore delle regole invece che da una query
        // scritta a mano: è la stessa domanda che una playlist intelligente sa
        // già fare, e scriverla due volte vorrebbe dire due idee di cosa
        // significhi «non di recente».
        let trascurati = rymembra_app::smart::brani(
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
    .await
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
/// Perché il nucleo non ha un fuso: `rymembra-domain` non guarda l'orologio e
/// `rymembra-app` tiene il tempo in millisecondi universali, che è l'unica forma
/// che sopravvive a una sincronizzazione fra dispositivi in due paesi. Il
/// lunedì invece è un fatto locale — comincia sette ore prima a Roma che a Los
/// Angeles — e l'unico posto che sa dove si trova la finestra è la finestra.
///
/// `scostamento_minuti` sono i minuti da **aggiungere** all'universale per
/// avere l'ora locale: è il contrario del segno di `getTimezoneOffset()`, e il
/// verso si legge senza doverci pensare. Vedi
/// [`rymembra_domain::settimana::lunedi`].
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
    let lunedi = rymembra_domain::settimana::lunedi(adesso, scostamento_minuti);

    con_libreria(&stato, |libreria| {
        let raccolte = rymembra_app::settimana::aggiorna(&mut libreria.connection, lunedi, adesso)?;
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
        rymembra_app::settimana::apri(&libreria.connection, raccolta, adesso_ms())
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
    /// La licenza di Rymembra.
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
    /// La console dove l'utente registra la sua app per una piattaforma
    /// (dalla 3.2). Stessa ragione di `Donazioni`: un indirizzo che la finestra
    /// ha il permesso di far aprire, e quale lo dice `rymembra-cloud`, che con
    /// quella piattaforma parla.
    Console(rymembra_domain::nuvola_app::Piattaforma),
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
            _ => grezzo
                .strip_prefix("console-")
                .and_then(rymembra_domain::nuvola_app::Piattaforma::da_chiave)
                .map(Self::Console),
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
            Self::Console(piattaforma) => rymembra_cloud::oauth::registrazione(piattaforma)
                .console
                .to_owned(),
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

/// Le voci della documentazione tecnica a cui le Impostazioni rimandano.
///
/// Una per scheda, più i principi che le schede hanno in comune: ognuna è la
/// voce che spiega come quella parte funziona nel codice. L'elenco è
/// chiuso per la ragione di [`Documento`]: l'ancora arriva dalla finestra, e un
/// comando che la incollasse in un indirizzo senza guardarla lascerebbe a una
/// pagina compromessa la scelta di cosa aggiungere dopo il `#`. Le ancore
/// stanno anche in `site/docs/` — `strumenti/docs.js` controlla che ci siano
/// tutte — e una nuova scheda delle Impostazioni ne chiede una nuova in tutti e
/// due i posti.
const ANCORE_GUIDA: &[&str] = &[
    "roots",
    "downloads",
    "lyrics",
    "enrich",
    "language",
    "theme",
    "skin",
    "tour",
    "eq",
    "playback",
    "output",
    "crossfade",
    "sleep",
    "spectrum",
    "shortcuts",
    "zoom",
    "motion",
    "cloud",
    "cloud-folders",
    "sync",
    "together",
    "legacy",
    "import",
    "scrobbling",
    "models",
    "background",
    "data",
    "maintenance",
    "profile",
    "update",
    "concepts",
];

/// Dove sta la documentazione, per un'ancora.
///
/// `None` per un'ancora fuori elenco. La lingua non c'entra: la documentazione
/// descrive il codice, ed è scritta in una lingua sola, l'inglese.
///
/// L'indirizzo si ricava dal repository, come quello delle donazioni:
/// `github.com/OWNER/REPO` diventa `OWNER.github.io/REPO/docs/`. Il ripiego è il
/// repository stesso, e non una pagina inventata.
fn indirizzo_guida(ancora: &str) -> Option<String> {
    if !ANCORE_GUIDA.contains(&ancora) {
        return None;
    }
    let radice = env!("CARGO_PKG_REPOSITORY");
    let base = radice
        .strip_prefix("https://github.com/")
        .and_then(|resto| resto.split_once('/'))
        .map_or_else(
            || radice.to_owned(),
            |(proprietario, repo)| {
                format!(
                    "https://{}.github.io/{repo}/docs/",
                    proprietario.to_lowercase()
                )
            },
        );
    Some(format!("{base}#{ancora}"))
}

/// Apre nel browser di sistema la voce della documentazione di una scheda.
///
/// Mai nella webview, per la ragione di [`apri_documento`]. L'ancora dev'essere
/// una di [`ANCORE_GUIDA`].
///
/// # Errori
///
/// `internal.aborted` per un'ancora fuori elenco, o se il browser rifiuta di
/// aprirsi.
#[tauri::command]
pub fn apri_guida(ancora: String) -> Esito<()> {
    let Some(indirizzo) = indirizzo_guida(&ancora) else {
        return Err(errore(
            AppError::new(ErrorCode::InternalAborted {
                what: Some("apertura della guida".to_owned()),
            })
            .with_cause(format!("«{ancora}» non è una pagina della guida")),
        ));
    };
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
        "console-google",
        "console-microsoft",
        "console-dropbox",
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
        assert!(Documento::da_nome("console-").is_none());
        assert!(Documento::da_nome("console-mega").is_none());
        assert!(Documento::da_nome("console-https://altrove").is_none());
    }

    /// Ogni ancora ha un indirizzo vero, sul sito e con l'ancora in fondo.
    #[test]
    fn ogni_ancora_della_guida_ha_un_indirizzo() {
        for ancora in ANCORE_GUIDA {
            let indirizzo = indirizzo_guida(ancora).expect("un'ancora dell'elenco");
            assert!(indirizzo.starts_with("https://"), "«{ancora}»: {indirizzo}");
            assert!(indirizzo.contains(".github.io/"), "«{ancora}»: {indirizzo}");
            assert!(indirizzo.ends_with(&format!("#{ancora}")), "{indirizzo}");
        }
    }

    /// Un'ancora inventata non apre niente: non finisce incollata dopo il `#`.
    #[test]
    fn un_ancora_inventata_non_apre_niente() {
        assert!(indirizzo_guida("../../x").is_none());
        assert!(indirizzo_guida("roots#x").is_none());
        assert!(indirizzo_guida("roots?lang=it").is_none());
        assert!(indirizzo_guida("").is_none());
    }

    /// L'indirizzo non porta parametri: la pagina è una, in una lingua sola.
    #[test]
    fn la_guida_non_ha_parametri() {
        let indirizzo = indirizzo_guida("roots").expect("un'ancora dell'elenco");
        assert!(indirizzo.ends_with("/docs/#roots"), "{indirizzo}");
        assert!(!indirizzo.contains('?'), "{indirizzo}");
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
/// `rymembra_app::primo`: qui si traduce soltanto, come ogni altra riga di questo
/// file. Quali siano le cartelle musicali di un sistema però lo sa il sistema, e
/// **questo** lo si può chiedere solo da qui: `rymembra-app` non conosce Tauri e
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
        rymembra_app::scadenza::con_scadenza(RICERCA_CARTELLE_NOME, RICERCA_CARTELLE, move || {
            rymembra_app::primo::esamina(&rymembra_app::files::LocalFiles, &percorsi)
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

#[cfg(test)]
mod prove_della_scansione {
    use super::*;
    use rymembra_app::library::TempiScansione;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    #[test]
    fn l_avanzamento_arriva_in_camel_case_con_la_fase() {
        // La forma che la finestra legge, dall'evento e da
        // `scansione_avanzamento`: una sola, con la fase in minuscolo.
        let cerca = Avanzamento {
            fase: FaseScansione::Cerca,
            fatti: 0,
            totale: 0,
            cartelle: 2561,
            trovati: 29_503,
        };
        assert_eq!(
            serde_json::to_value(cerca).expect("json"),
            serde_json::json!({
                "fase": "cerca",
                "fatti": 0,
                "totale": 0,
                "cartelle": 2561,
                "trovati": 29_503,
            })
        );
        for (fase, nome) in [
            (FaseScansione::Legge, "legge"),
            (FaseScansione::Attende, "attende"),
        ] {
            assert_eq!(
                serde_json::to_value(Avanzamento::di(fase)).expect("json")["fase"],
                nome
            );
        }
    }

    #[test]
    fn la_riga_del_diario_ha_un_formato_solo() {
        let esito = ScanReport {
            inserted: 3,
            updated: 1,
            removed: 2,
            cartelle_viste: 2561,
            file_visti: 29_503,
            letti: 4,
            illeggibili_noti: 61,
            brani_d_immagine: 4883,
            radici_saltate: vec![r"\\nas\spento".to_owned()],
            elapsed_ms: 8200,
            tempi: TempiScansione {
                camminata_ms: 8100,
                fogli_ms: 12,
                piano_ms: 30,
                lettura_ms: 5,
                scrittura_ms: 40,
                aggregati_ms: 60,
            },
            ..ScanReport::default()
        };
        assert_eq!(
            riga_della_scansione("a mano", &esito),
            "[scansione] origine=a mano cartelle=2561 file=29503 letti=4 illeggibili=0(+61 noti) \
             immagine=4883 inseriti=3 aggiornati=1 spostati=0 tolti=2 saltate=1 rami=0 \
             rinviate=0 cammino=8100ms fogli=12ms piano=30ms lettura=5ms scrittura=40ms \
             album=60ms totale=8200ms"
        );
        assert!(ha_cambiato(&esito));

        // Una fermata lo dice, e non ha cambiato niente.
        let fermata = ScanReport {
            cancelled: true,
            ..ScanReport::default()
        };
        assert!(
            riga_della_scansione("sorveglianza", &fermata)
                .contains("origine=sorveglianza cartelle=0")
        );
        assert!(
            riga_della_scansione("sorveglianza", &fermata)
                .contains("rinviate=0 annullata cammino=")
        );
        assert!(!ha_cambiato(&fermata));
        // Basta un numero solo dei quattro.
        assert!(ha_cambiato(&ScanReport {
            moved: 1,
            ..ScanReport::default()
        }));
    }

    #[test]
    fn una_scansione_caduta_vale_come_una_che_ha_cambiato() {
        // Il difetto: la scansione a mano e quella dei download, cadute dopo
        // aver scritto dei lotti, non avvisavano la finestra, che restava con
        // la libreria di prima. L'errore non porta i conti: nel dubbio sì.
        let caduta: Result<ScanReport, AppError> = Err(AppError::new(ErrorCode::InternalAborted {
            what: Some("scansione".to_owned()),
        }));
        assert!(forse_cambiata(&caduta));
        // Finita, decide quel che ha fatto: niente non avvisa nessuno.
        assert!(!forse_cambiata(&Ok(ScanReport::default())));
        assert!(forse_cambiata(&Ok(ScanReport {
            removed: 1,
            ..ScanReport::default()
        })));
    }

    #[test]
    fn mostra_nella_cartella_un_brano_d_immagine_mostra_l_immagine() {
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        let mut guardia = stato.libreria.lock().expect("lucchetto");
        let libreria = guardia.as_mut().expect("libreria aperta");
        libreria
            .connection
            .execute_batch(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, album_key,
                                     duration_ms, file_size, date_added, date_modified)
                 VALUES (1, 'C:\\M\\a.flac', 'k1', 'T', 'A', 'Al', 'g', 1000, 1, 0, 0);
                 INSERT INTO tracks (id, path, track_key, title, artist, album, album_key,
                                     duration_ms, file_size, date_added, date_modified,
                                     immagine, inizio_cd, fine_cd)
                 VALUES (2, 'C:\\M\\Disco\\Album.cue|03', 'k2', 'T3', 'A', 'Al', 'g', 1000, 1,
                         0, 0, 'C:\\M\\Disco\\Album.ape', 7500, 15000);",
            )
            .expect("banco");
        assert_eq!(
            file_da_mostrare(&libreria.connection, 1).expect("un file"),
            r"C:\M\a.flac"
        );
        assert_eq!(
            file_da_mostrare(&libreria.connection, 2).expect("un brano d'immagine"),
            r"C:\M\Disco\Album.ape",
            "non il percorso virtuale, che non si apre"
        );
        let manca = file_da_mostrare(&libreria.connection, 99).expect_err("non c'è");
        assert_eq!(manca.code().kind().code(), "library.trackNotFound");
    }

    /// Il ciclo intero dell'attesa del turno, in una prova sola: i due posti
    /// che tocca (`IN_CORSO`, `ATTESA_LASCIATA`) sono statici, e due prove che
    /// li usassero insieme si guarderebbero a vicenda.
    #[test]
    fn l_attesa_del_turno_si_lascia_con_annulla_senza_fermare_la_scansione_automatica() {
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        let mai = || false;

        // ── il turno libero: si prende subito, senza annunciare un'attesa ──
        assert!(prendi_il_posto());
        assert!(!prendi_il_posto(), "una sola scansione a mano alla volta");
        // Un annullamento rimasto da prima non deve fermare questa.
        stato.ferma_scansione();
        let mut annunci = 0_u32;
        let turno = aspetta_il_turno_a_mano(&stato, &mai, &mut |_| annunci += 1)
            .expect("il turno era libero");
        assert_eq!(annunci, 0, "nessuna attesa da dire");
        assert!(!stato.scansione_fermata(), "azzerato prima di leggere");
        assert_eq!(in_corso().map(|a| a.fase), Some(FaseScansione::Cerca));
        // Al lavoro, Annulla ferma la scansione come sempre.
        annulla(&stato);
        assert!(stato.scansione_fermata());
        drop(turno);
        *in_corso() = None;

        // ── il turno di una scansione automatica: si aspetta ──
        stato.riprendi_scansioni();
        let automatica = stato.turno_di_scansione().expect("turno libero");
        assert!(prendi_il_posto());
        let annunciata = AtomicBool::new(false);
        let preso = std::thread::scope(|fili| {
            let filo = fili.spawn(|| {
                aspetta_il_turno_a_mano(&stato, &mai, &mut |avanzamento| {
                    assert_eq!(avanzamento.fase, FaseScansione::Attende);
                    annunciata.store(true, Ordering::Release);
                })
                .is_some()
            });
            let limite = Instant::now() + Duration::from_secs(10);
            while !annunciata.load(Ordering::Acquire) {
                assert!(Instant::now() < limite, "l'attesa non si è annunciata");
                std::thread::sleep(Duration::from_millis(10));
            }
            // Chi arriva adesso legge l'attesa, come l'evento.
            assert_eq!(
                scansione_avanzamento().map(|a| a.fase),
                Some(FaseScansione::Attende)
            );
            annulla(&stato);
            filo.join().expect("il filo è finito")
        });
        assert!(!preso, "un'attesa lasciata non prende il turno");
        // E quel che `scansiona` risponde allora è una scansione annullata,
        // nella forma che la finestra legge — non `library.scanBusy`.
        let lasciata = EsitoScansione::lasciata(Counts::default());
        assert!(lasciata.annullata);
        assert_eq!(
            serde_json::to_value(&lasciata).expect("json")["annullata"],
            true
        );
        assert!(
            !stato.scansione_fermata(),
            "la scansione automatica non l'ha fermata nessuno"
        );
        assert!(
            stato.turno_di_scansione().is_none(),
            "il turno resta a chi lo teneva"
        );
        *in_corso() = None;

        // ── l'uscita lascia l'attesa allo stesso modo ──
        assert!(prendi_il_posto());
        assert!(aspetta_il_turno_a_mano(&stato, &|| true, &mut |_| ()).is_none());
        drop(automatica);
        // E con il turno libero, chi esce non lo prende comunque.
        assert!(aspetta_il_turno_a_mano(&stato, &|| true, &mut |_| ()).is_none());
        assert!(stato.turno_di_scansione().is_some(), "e il turno è libero");
        *in_corso() = None;
    }
}
