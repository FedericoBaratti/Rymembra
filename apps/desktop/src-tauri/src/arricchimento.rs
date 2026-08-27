//! L'arricchimento dei metadati, dal lato della finestra.
//!
//! Tre comandi, due eventi e un filo di sottofondo. Le decisioni stanno in
//! `aether-domain`, le richieste in `aether-meta`, l'orchestrazione in
//! `aether_app::enrich`: qui si tiene il tempo e si prende il lucchetto — le due
//! cose che nessuno di quei tre sa fare, e le due che possono far impuntare la
//! riproduzione.
//!
//! # L'ordine dei lucchetti
//!
//! Come `nuvola` e `riproduzione` dichiarano il proprio, questo modulo dichiara
//! il suo:
//!
//! > **Il filo dell'arricchimento non prende mai il lucchetto del lettore, e
//! > prende quello della libreria solo in finestre brevi, una per gruppo
//! > d'album.**
//!
//! Le finestre sono: leggere un lotto di gruppi candidati, e riscrivere le righe
//! di un gruppo appena deciso. Tutto quel che sta in mezzo — le ricerche su
//! MusicBrainz, le copertine che scendono dal Cover Art Archive, i tag riscritti
//! sui file — dura da secondi a minuti e avviene **senza nessun lucchetto**.
//!
//! Che sia vero non è affidato all'attenzione di chi legge: `aether-meta` non
//! riceve mai una `rusqlite::Connection`, quindi il codice che terrebbe il
//! lucchetto durante una richiesta di rete non si può nemmeno scrivere.
//!
//! # Perché l'interruttore e l'annullamento sono qui
//!
//! Questo è l'unico filo di Aether che **scrive nei file dell'utente senza che
//! nessuno guardi prima**. È una scelta deliberata — la revisione a mano
//! trasformerebbe millequattrocento brani in millequattrocento decisioni, che
//! nessuno prende — e regge su due garanzie, non su una:
//!
//! * si scrive solo su verdetto `Applica`, e astenersi è il comportamento
//!   normale (la ragione per esteso sta in testa a `aether_domain::enrich`);
//! * ogni scrittura è annullabile, perché i tag di prima finiscono in
//!   `enrich_undo` la prima volta che si tocca un file, e una volta sola.
//!
//! Da qui i due comandi che sembrano accessori e non lo sono: [`arricchimento_attiva`]
//! per spegnerlo e [`arricchimento_annulla`] per disfare. Un sistema che lavora
//! da solo sui file di qualcun altro deve essere spegnibile e reversibile da un
//! posto che quel qualcuno trova — e il posto sono le Impostazioni, accanto alle
//! cartelle sorvegliate.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use aether_app::covers::CoverStore;
use aether_app::enrich::{self, DepositoSqlite, Gruppo};
use aether_app::settings;
use aether_domain::enrich::Verdetto;
use aether_domain::errors::{AppError, ErrorCode};
use aether_meta::Fornitori;
use serde::Serialize;
use tauri::{AppHandle, Emitter as _, Manager as _, State};

use crate::errore::{ErroreIpc, Esito, errore};
use crate::nota;
use crate::stato::{NOME_DATABASE, Stato, Turno, adesso_ms, con_libreria};

// ── le chiavi in `settings` ─────────────────────────────────────────────────

/// L'arricchimento automatico è acceso.
const CHIAVE_ATTIVO: &str = "enrich.auto";
/// Quando è finita l'ultima passata.
const CHIAVE_ULTIMO: &str = "enrich.ultimo_ms";

// ── i tempi del filo ────────────────────────────────────────────────────────

/// Ogni quanto si riprova, quando non succede niente.
///
/// Mezz'ora, il doppio dell'intervallo della nuvola, e per una ragione che non è
/// la prudenza: una passata **costa** — dodici gruppi sono qualche minuto di
/// richieste a una al secondo — mentre un backup che non ha niente da caricare
/// costa una `files.list`. Un intervallo corto qui vorrebbe dire tenere occupata
/// la quota di MusicBrainz per riscoprire ogni volta che non c'è niente da fare.
const INTERVALLO: Duration = Duration::from_secs(30 * 60);

/// Quanto si aspetta prima della prima passata.
///
/// Novanta secondi, tre volte i trenta della nuvola. All'avvio può esserci una
/// scansione davanti, e la nuvola può permettersi di salvare uno stato
/// intermedio — sarebbe corretto, solo inutile. Qui no: arricchire mentre la
/// libreria cambia sotto vuol dire decidere su righe che la scansione sta per
/// riscrivere dai tag, e la riscrittura vincerebbe.
const ATTESA_AVVIO: Duration = Duration::from_secs(90);

/// Quanto si aspetta che una raffica di modifiche finisca.
///
/// Chi scarica una playlist da venti brani fa finire venti volte una coda. Senza
/// questa attesa produrrebbe venti passate, ognuna su un lotto di dodici gruppi
/// quasi tutti uguali a quelli di prima.
const RAFFICA: Duration = Duration::from_secs(2 * 60);

// ── lo stato condiviso ──────────────────────────────────────────────────────

/// Perché il filo si è svegliato.
///
/// Pubblico solo perché compare nel tipo del canale che `main.rs` passa a
/// [`avvia_filo`]. Nessuno fuori da questo modulo ne costruisce uno.
///
/// # Non c'è un «chiudi»
///
/// Il filo si ferma quando il canale si chiude, cioè quando [`StatoArricchimento`]
/// viene lasciato cadere insieme al resto dello stato dell'applicazione. Una
/// passata interrotta a metà non lascia niente di scritto per metà: ogni gruppo
/// si registra nella sua transazione, e i file si scrivono prima delle righe che
/// li descrivono — quindi al massimo si perde il lavoro di rete già fatto sul
/// gruppo in corso, che la passata dopo rifà.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sveglia {
    /// Sono entrati brani nuovi: aspetta che la raffica finisca.
    Sporca,
    /// Qualcuno ha chiesto una passata adesso.
    Subito,
}

/// Quel che l'arricchimento tiene aperto mentre l'applicazione gira.
pub struct StatoArricchimento {
    /// Come si sveglia il filo.
    sveglia: Sender<Sveglia>,

    /// C'è già una passata in corso.
    ///
    /// Non sta dietro un mutex per la stessa ragione di `scansione_da_fermare` in
    /// `stato.rs`: una passata occupa il filo per minuti, e un comando
    /// `arricchimento_stato` che dovesse chiedere lo stesso lucchetto per sapere
    /// se c'è una passata in corso resterebbe in coda dietro la passata di cui
    /// vuole parlare.
    in_corso: AtomicBool,

    /// L'esito dell'ultima passata automatica.
    ///
    /// `None` vuol dire riuscita. Vive qui e non in `settings` perché è una cosa
    /// di questa sessione: «MusicBrainz non rispondeva ieri sera» non è
    /// un'informazione utile stamattina, e scriverla su disco la farebbe
    /// sopravvivere al riavvio che quasi sempre la risolve.
    ultimo_errore: Mutex<Option<ErroreIpc>>,
}

impl StatoArricchimento {
    /// Costruisce lo stato e il capo del canale che il filo dovrà ascoltare.
    #[must_use]
    pub fn nuovo() -> (Self, Receiver<Sveglia>) {
        let (sveglia, orecchio) = channel();
        (
            Self {
                sveglia,
                in_corso: AtomicBool::new(false),
                ultimo_errore: Mutex::new(None),
            },
            orecchio,
        )
    }
}

/// Una passata è già in corso.
fn occupato() -> AppError {
    AppError::new(ErrorCode::MetadataEnrichBusy)
}

// ── quel che la finestra vede ───────────────────────────────────────────────

/// Lo stato dell'arricchimento, come lo mostrano le Impostazioni.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoArricchimentoIpc {
    /// L'arricchimento automatico è acceso.
    pub attivo: bool,
    /// C'è una passata in corso adesso.
    pub in_corso: bool,
    /// Quanti brani hanno ricevuto una corrispondenza applicata.
    pub completati: i64,
    /// Quanti brani nessun catalogo ha riconosciuto.
    pub senza_corrispondenza: i64,
    /// Quanti brani aspettano ancora il loro turno.
    pub da_fare: i64,
    /// Quante scritture si possono ancora riportare indietro.
    ///
    /// È il numero che decide se il pulsante «annulla» abbia senso: a zero non
    /// c'è niente da disfare, e mostrarlo attivo prometterebbe qualcosa che non
    /// succede.
    pub annullabili: i64,
    /// Quando è finita l'ultima passata.
    pub ultimo_ms: Option<i64>,
    /// Com'è andata l'ultima passata automatica.
    pub errore: Option<ErroreIpc>,
}

/// Com'è andato un annullamento.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EsitoAnnullamentoIpc {
    /// Quanti brani sono tornati ai tag di prima.
    pub riportati: usize,
    /// Quanti file non si sono potuti riscrivere.
    pub falliti: usize,
    /// Lo stato dopo, per non doverlo richiedere.
    pub stato: StatoArricchimentoIpc,
}

// ── i comandi ───────────────────────────────────────────────────────────────

/// Lo stato dell'arricchimento. Istantaneo: non tocca la rete.
#[tauri::command]
pub fn arricchimento_stato(
    stato: State<'_, Stato>,
    arricchimento: State<'_, StatoArricchimento>,
) -> Esito<StatoArricchimentoIpc> {
    stato_ipc(&stato, &arricchimento).map_err(errore)
}

/// Accende o spegne l'arricchimento automatico.
///
/// Accenderlo non aspetta la mezz'ora: chi tocca l'interruttore vuole vedere che
/// ha funzionato, e mezz'ora di niente si legge come un interruttore rotto.
#[tauri::command]
pub fn arricchimento_attiva(
    app: AppHandle,
    stato: State<'_, Stato>,
    arricchimento: State<'_, StatoArricchimento>,
    attivo: bool,
) -> Esito<StatoArricchimentoIpc> {
    let esito = (|| {
        con_libreria(&stato, |libreria| {
            settings::write(
                &libreria.connection,
                CHIAVE_ATTIVO,
                if attivo { "1" } else { "0" },
            )
        })?;
        if attivo {
            subito(&arricchimento);
        }
        scorda_errore(&arricchimento);
        Ok(())
    })();

    concludi(&app, &stato, &arricchimento, esito)
}

/// Riporta indietro tutto quel che l'arricchimento ha scritto.
///
/// # Perché spegne anche l'interruttore
///
/// Perché altrimenti non servirebbe a niente. L'annullamento rimette
/// `enrich_status` a `NULL` su ogni brano che tocca — deve, o la riga resterebbe
/// a dire «già arricchito» mentre il file è tornato com'era — e un brano con
/// `enrich_status` nullo è di nuovo un candidato. Con l'automatico acceso, la
/// passata successiva riscriverebbe entro mezz'ora esattamente quel che l'utente
/// ha appena chiesto di disfare.
///
/// Spegnere è quindi l'unica lettura onesta del gesto: chi annulla sta dicendo
/// «non era quello che volevo», non «rifallo fra poco». Riaccenderlo è un clic
/// nello stesso pannello.
///
/// # Perché `(async)` su una funzione che non è `async`
///
/// Perché un `#[tauri::command]` normale gira sul filo dell'anello degli eventi
/// della finestra. Questo riapre e riscrive un file per ogni brano arricchito —
/// su una libreria vera possono essere centinaia, cioè decine di secondi in cui
/// l'applicazione sarebbe congelata. `(async)` su una funzione sincrona la manda
/// sulla riserva di fili di Tauri: il corpo resta bloccante e ordinario, cambia
/// solo **dove** gira.
#[tauri::command(async)]
pub fn arricchimento_annulla(
    app: AppHandle,
    stato: State<'_, Stato>,
    arricchimento: State<'_, StatoArricchimento>,
) -> Esito<EsitoAnnullamentoIpc> {
    let annullati = (|| {
        // Il turno prima di tutto: una passata in corso sta riscrivendo tag
        // proprio mentre questo li riporterebbe indietro, e quale dei due vinca
        // dipenderebbe dall'ordine in cui i due fili arrivano al file.
        let _turno = Turno::prendi(&arricchimento.in_corso).ok_or_else(occupato)?;

        con_libreria(&stato, |libreria| {
            settings::write(&libreria.connection, CHIAVE_ATTIVO, "0")
        })?;
        con_libreria(&stato, |libreria| {
            enrich::annulla(&mut libreria.connection, true)
        })
    })();
    // Fuori dalla chiusura, cioè con il turno ormai lasciato cadere: qui dentro
    // lo stato direbbe `inCorso: true` per un'operazione appena finita, e il
    // pulsante resterebbe disabilitato fino al comando successivo.
    scorda_errore(&arricchimento);

    let esito = annullati.and_then(|annullati| {
        Ok(EsitoAnnullamentoIpc {
            riportati: annullati.riportati,
            falliti: annullati.falliti,
            stato: stato_ipc(&stato, &arricchimento)?,
        })
    });
    riferisci(&app);
    esito.map_err(errore)
}

/// Chiude un comando: rilascia, riferisce, e restituisce lo stato di **adesso**.
///
/// Lo stato si calcola qui e non dentro il comando, per la stessa ragione scritta
/// in `nuvola::concludi`: la finestra riceve due cose — la risposta al comando e
/// l'evento `arricchimento:stato` — senza nessuna garanzia su quale arrivi per
/// ultima, e le due devono dire la stessa cosa.
fn concludi(
    app: &AppHandle,
    stato: &Stato,
    arricchimento: &StatoArricchimento,
    esito: Result<(), AppError>,
) -> Esito<StatoArricchimentoIpc> {
    let adesso = esito
        .and_then(|()| stato_ipc(stato, arricchimento))
        .map_err(errore);
    riferisci(app);
    adesso
}

// ── il filo di sottofondo ───────────────────────────────────────────────────

/// Segnala che ci sono brani nuovi da guardare.
///
/// Da chiamare dopo una scansione e dopo una coda di scaricamento: sono i due
/// momenti in cui entrano in libreria righe che nessuno ha mai tentato di
/// arricchire. Non fa niente e non fallisce se il filo non c'è.
pub fn sporca(app: &AppHandle) {
    if let Some(arricchimento) = app.try_state::<StatoArricchimento>() {
        // Un canale chiuso vuol dire che il filo non c'è più: non è un guasto di
        // cui la scansione che ha appena finito debba occuparsi.
        let _ = arricchimento.sveglia.send(Sveglia::Sporca);
    }
}

/// Chiede una passata senza aspettare la raffica.
fn subito(arricchimento: &StatoArricchimento) {
    let _ = arricchimento.sveglia.send(Sveglia::Subito);
}

/// Avvia il filo che arricchisce da solo.
///
/// Modellato su `nuvola::avvia_filo`: un thread nominato, `recv_timeout` che fa
/// da periodicità **e** da antirimbalzo, nessun timer e nessun runtime
/// asincrono. Con tokio arriverebbe una seconda idea di cos'è un errore e di chi
/// possiede un thread, per fare qualche richiesta HTTP ogni mezz'ora.
///
/// # Un filo, non tre
///
/// Il vecchio albero ne usava tre, per sovrapporre il lavoro di CPU di `fpcalc`
/// alle richieste di rete. Senza impronta acustica non c'è lavoro di CPU da
/// sovrapporre, e tre fili si metterebbero soltanto in coda sullo stesso cancello
/// da una richiesta al secondo — pagando tre lucchetti per la velocità di uno.
pub fn avvia_filo(app: AppHandle, orecchio: Receiver<Sveglia>) {
    let avviato = std::thread::Builder::new()
        .name("aether-arricchimento".to_owned())
        .spawn(move || {
            // I fornitori vivono qui, sullo stack del filo, e non nello stato
            // condiviso: dentro ci sono la riserva di connessioni di `ureq` e lo
            // stato degli interruttori, che devono sopravvivere fra una passata
            // e l'altra — ma nessun altro filo li guarda, e metterli dietro un
            // mutex vorrebbe dire un lucchetto tenuto per minuti che nessuno
            // aspetta.
            let mut fornitori: Option<Fornitori> = None;

            let mut motivo = match orecchio.recv_timeout(ATTESA_AVVIO) {
                // Canale chiuso: l'applicazione sta uscendo.
                Err(RecvTimeoutError::Disconnected) => return,
                Ok(sveglia) => sveglia,
                Err(RecvTimeoutError::Timeout) => Sveglia::Subito,
            };
            loop {
                if motivo == Sveglia::Sporca {
                    // Si aspetta che la raffica finisca: venti brani scaricati
                    // sono una passata, non venti. Si esce da qui quando per due
                    // minuti non arriva più niente — oppure subito, se nel
                    // frattempo il canale si è chiuso.
                    //
                    // Distinguere i due esiti è ciò che impedisce a una chiusura
                    // dell'applicazione di far cominciare **adesso** una passata
                    // intera: minuti di richieste di rete e, soprattutto, tag
                    // riscritti sui file dell'utente mentre il processo esce.
                    if crate::stato::aspetta_la_raffica(&orecchio, RAFFICA).is_break() {
                        return;
                    }
                }
                passata(&app, &mut fornitori);
                motivo = match orecchio.recv_timeout(INTERVALLO) {
                    Err(RecvTimeoutError::Disconnected) => return,
                    Ok(sveglia) => sveglia,
                    // Il timeout **è** il battito periodico: nessun timer.
                    Err(RecvTimeoutError::Timeout) => Sveglia::Subito,
                };
            }
        });
    if let Err(err) = avviato {
        nota!("[avvio] il filo dell'arricchimento non è partito: {err}");
    }
}

/// Una passata. Non fallisce mai rumorosamente.
///
/// Ogni guasto diventa un evento per la finestra: questo filo gira per conto
/// suo, e un `Err` propagato non avrebbe nessuno a cui arrivare — mentre l'utente
/// ha il diritto di sapere che l'arricchimento non sta funzionando, soprattutto
/// se ha spento la scansione automatica aspettandosi che ci pensasse questo.
fn passata(app: &AppHandle, fornitori: &mut Option<Fornitori>) {
    let esito = passata_vera(app, fornitori);
    if let Some(arricchimento) = app.try_state::<StatoArricchimento>() {
        let mut ultimo = arricchimento
            .ultimo_errore
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *ultimo = match &esito {
            Ok(_) => None,
            Err(err) => Some(ErroreIpc::from(err.clone())),
        };
    }
    match &esito {
        Ok(Some(bilancio)) => {
            let _ = app.emit("arricchimento:esito", *bilancio);
        }
        // Niente da fare, o interruttore spento: nessun evento. Una riga di
        // «zero applicati» ogni mezz'ora addestrerebbe chi guarda a ignorarle.
        Ok(None) => {}
        Err(err) => nota!(
            "[arricchimento] passata non riuscita codice={} causa={}",
            err.code().kind().code(),
            err.cause().unwrap_or("—")
        ),
    }
    riferisci(app);
}

/// Quanto ha prodotto una passata.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Bilancio {
    /// Brani con i campi riscritti.
    applicati: usize,
    /// Brani su cui ci si è astenuti, per qualunque ragione.
    astenuti: usize,
    /// Brani che nessun catalogo ha riconosciuto.
    senza_corrispondenza: usize,
    /// Copertine nuove salvate.
    copertine: usize,
}

/// Quel che serve a una passata, letto sotto il lucchetto e portato fuori.
///
/// Una struttura e non quattro valori sciolti perché il punto è che escano
/// **insieme** dalla finestra di lucchetto: `CoverStore` è clonabile (è un
/// percorso) proprio per poter salvare le copertine senza tenerlo preso.
struct DaFare {
    /// La cartella dati, per aprire il deposito della cache.
    dati: PathBuf,
    /// Lo store delle copertine, staccato dalla libreria.
    covers: CoverStore,
    /// I gruppi d'album di questo lotto.
    gruppi: Vec<Gruppo>,
}

/// Il corpo della passata, con le sue finestre di lucchetto.
///
/// `Ok(None)` vuol dire «non c'era niente da fare»: interruttore spento, lotto
/// vuoto, o applicazione senza libreria aperta. Non è un guasto e non si mostra.
fn passata_vera(
    app: &AppHandle,
    fornitori: &mut Option<Fornitori>,
) -> Result<Option<Bilancio>, AppError> {
    let (Some(stato), Some(arricchimento)) = (
        app.try_state::<Stato>(),
        app.try_state::<StatoArricchimento>(),
    ) else {
        return Ok(None);
    };
    let Some(_turno) = Turno::prendi(&arricchimento.in_corso) else {
        // Un annullamento a mano sta riscrivendo i file. La passata successiva
        // arriva fra mezz'ora: non c'è niente da segnalare.
        return Ok(None);
    };

    // ── finestra 1: leggere il lotto. ───────────────────────────────────────
    let Some(da_fare) = apri_passata(&stato)? else {
        return Ok(None);
    };

    // ── senza nessun lucchetto: da qui in poi si va in rete. ────────────────
    if fornitori.is_none() {
        let deposito = DepositoSqlite::apri(&da_fare.dati.join(NOME_DATABASE), adesso_ms())?;
        *fornitori = Some(Fornitori::nuovo(Box::new(deposito)));
    }
    let Some(servizi) = fornitori.as_ref() else {
        return Ok(None);
    };
    if !servizi.in_piedi() {
        // L'interruttore di MusicBrainz è aperto: le altre due fonti possono al
        // massimo correggere un titolo, e senza il consenso non applicherebbero
        // comunque niente. Meglio dirlo che spendere il traffico.
        return Err(AppError::new(ErrorCode::MetadataMusicbrainzUnavailable)
            .with_cause("l'interruttore di MusicBrainz è aperto"));
    }

    // Da qui la passata comincia davvero, e la finestra deve saperlo: senza
    // questa riga il primo `arricchimento:stato` con `inCorso` vero sarebbe
    // quello di **fine** passata, cioè quello che dice già di no — e per tutti i
    // minuti in cui il filo lavora il pannello direbbe che è fermo.
    riferisci(app);

    let totale = da_fare.gruppi.len();
    let mut bilancio = Bilancio::default();
    for (fatti, gruppo) in da_fare.gruppi.iter().enumerate() {
        avanza(app, fatti, totale);

        let decisione = enrich::decidi(servizi, gruppo);
        if decisione.non_raggiungibile {
            // Nessuna fonte ha risposto. **Non si registra niente**: marchiare
            // come introvabili i brani incontrati mentre il portatile era
            // staccato dal wifi vorrebbe dire non riprovarli per un mese. Ci si
            // ferma anche per i gruppi dopo, che troverebbero la stessa rete.
            break;
        }

        let (esiti, guasti) = enrich::scrivi_file(&da_fare.covers, &decisione.scritture, true);
        for guasto in &guasti {
            // Un file bloccato — su Windows basta che sia in riproduzione — non
            // ferma la passata: si dice e si va avanti. La riga resta candidata,
            // e la si ritenta fra una settimana.
            nota!(
                "[arricchimento] file non scritto codice={} causa={}",
                guasto.code().kind().code(),
                guasto.cause().unwrap_or("—")
            );
        }

        // ── finestra 2: registrare questo gruppo. ───────────────────────────
        let conto = con_libreria(&stato, |libreria| {
            let tx = libreria
                .connection
                .transaction()
                .map_err(|err| db_errore("apertura della transazione di arricchimento", &err))?;
            let conto =
                enrich::registra(&tx, &decisione, &decisione.scritture, &esiti, adesso_ms())?;
            // Le scritture senza un esito sono quelle che non si sono potute
            // fare. Vanno segnate, o resterebbero con `enrich_status` nullo e
            // la passata dopo le ritenterebbe subito, per sempre.
            for scrittura in &decisione.scritture {
                if !esiti.iter().any(|e| e.track_id == scrittura.track_id) {
                    enrich::segna_errore(&tx, scrittura.track_id, adesso_ms())?;
                }
            }
            tx.commit()
                .map_err(|err| db_errore("chiusura della transazione di arricchimento", &err))?;
            Ok(conto)
        })?;

        bilancio.applicati = bilancio.applicati.saturating_add(conto.applicati);
        bilancio.astenuti = bilancio.astenuti.saturating_add(conto.astenuti);
        bilancio.copertine = bilancio.copertine.saturating_add(conto.copertine);
        bilancio.senza_corrispondenza = bilancio.senza_corrispondenza.saturating_add(
            decisione
                .astensioni
                .iter()
                .filter(|(_, verdetto)| *verdetto == Verdetto::Nessuno)
                .count(),
        );
    }
    avanza(app, totale, totale);

    // ── finestra 3: gli aggregati, una volta sola. ──────────────────────────
    //
    // Qui e non dentro il ciclo: `rebuild_aggregates` riscrive `albums` e
    // `artists` per intero, e farlo dodici volte di fila costerebbe dodici volte
    // per un risultato che conta solo alla fine.
    con_libreria(&stato, |libreria| {
        if bilancio.applicati > 0 {
            // L'arricchimento scrive `mb_release_group_id` su brani che prima non
            // l'avevano, ed è esattamente l'identificativo con cui i gruppi
            // d'album si **fondono**. Senza la ricostruzione, il disco resterebbe
            // spezzato nelle due schede che l'arricchimento ha appena dimostrato
            // essere una sola.
            let tx = libreria
                .connection
                .transaction()
                .map_err(|err| db_errore("apertura della ricostruzione", &err))?;
            enrich::ricostruisci(&tx)?;
            tx.commit()
                .map_err(|err| db_errore("chiusura della ricostruzione", &err))?;
        }
        settings::write(
            &libreria.connection,
            CHIAVE_ULTIMO,
            &adesso_ms().to_string(),
        )
    })?;

    Ok(Some(bilancio))
}

/// Legge sotto il lucchetto tutto ciò che serve a una passata, e lo rilascia.
///
/// `Ok(None)` quando non c'è niente da fare: l'interruttore è spento, o il lotto
/// è vuoto. Nessuno dei due è un guasto.
fn apri_passata(stato: &Stato) -> Result<Option<DaFare>, AppError> {
    con_libreria(stato, |libreria| {
        if !attivo(&libreria.connection)? {
            return Ok(None);
        }
        let gruppi = enrich::candidati(&libreria.connection, adesso_ms(), enrich::LOTTO)?;
        if gruppi.is_empty() {
            return Ok(None);
        }
        Ok(Some(DaFare {
            dati: libreria.data_dir.clone(),
            covers: libreria.covers.clone(),
            gruppi,
        }))
    })
}

/// Manda alla finestra un passo di avanzamento.
///
/// Un evento che non parte non è una ragione per far fallire una passata
/// riuscita: la finestra può essersi chiusa mentre il filo lavorava.
fn avanza(app: &AppHandle, fatti: usize, totale: usize) {
    let _ = app.emit(
        "arricchimento:avanzamento",
        serde_json::json!({ "fatti": fatti, "totale": totale }),
    );
}

/// Manda alla finestra lo stato aggiornato.
fn riferisci(app: &AppHandle) {
    let (Some(stato), Some(arricchimento)) = (
        app.try_state::<Stato>(),
        app.try_state::<StatoArricchimento>(),
    ) else {
        return;
    };
    if let Ok(ipc) = stato_ipc(&stato, &arricchimento) {
        drop(app.emit("arricchimento:stato", ipc));
    }
}

// ── i pezzi condivisi ───────────────────────────────────────────────────────

/// L'arricchimento automatico è acceso?
///
/// # Perché l'assenza della chiave vale «acceso»
///
/// Perché è la scelta che l'utente ha già fatto scegliendo questo sistema: la
/// passata è automatica, senza schermata di revisione. Una libreria appena
/// importata è precisamente quella che ne ha bisogno — titoli presi da YouTube,
/// «Album sconosciuto», nessuna copertina — e un interruttore spento di serie
/// vorrebbe dire che il caso normale è quello in cui non succede niente finché
/// qualcuno non trova un pannello.
///
/// Quel che rende accettabile il valore di serie non è questa riga: è che si
/// scrive solo su verdetto `Applica`, e che ogni scrittura è annullabile. Vedi la
/// nota in testa al modulo.
fn attivo(connection: &rusqlite::Connection) -> Result<bool, AppError> {
    Ok(settings::read(connection, CHIAVE_ATTIVO)?.as_deref() != Some("0"))
}

/// Dimentica l'errore dell'ultima passata.
fn scorda_errore(arricchimento: &StatoArricchimento) {
    let mut ultimo = arricchimento
        .ultimo_errore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *ultimo = None;
}

/// Un guasto del database, nella forma del catalogo.
fn db_errore(cosa: &str, err: &rusqlite::Error) -> AppError {
    AppError::new(ErrorCode::DbQueryFailed {
        detail: Some(cosa.to_owned()),
    })
    .with_cause(err.to_string())
}

/// Lo stato, nella forma che la finestra riceve.
fn stato_ipc(
    stato: &Stato,
    arricchimento: &StatoArricchimento,
) -> Result<StatoArricchimentoIpc, AppError> {
    let errore = arricchimento
        .ultimo_errore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();

    con_libreria(stato, |libreria| {
        Ok(StatoArricchimentoIpc {
            attivo: attivo(&libreria.connection)?,
            in_corso: arricchimento.in_corso.load(Ordering::Acquire),
            completati: quanti(
                &libreria.connection,
                "SELECT COUNT(*) FROM tracks WHERE enrich_status = 'ok'",
            )?,
            senza_corrispondenza: quanti(
                &libreria.connection,
                "SELECT COUNT(*) FROM tracks WHERE enrich_status = 'no-match'",
            )?,
            da_fare: enrich::quanti_mancano(&libreria.connection, adesso_ms())?,
            annullabili: quanti(&libreria.connection, "SELECT COUNT(*) FROM enrich_undo")?,
            ultimo_ms: settings::read(&libreria.connection, CHIAVE_ULTIMO)?
                .and_then(|quando| quando.parse().ok()),
            errore,
        })
    })
}

/// Un conteggio, per le tre righe di stato.
///
/// Contati dal database e non tenuti in `settings`: un contatore incrementato a
/// ogni passata divergerebbe al primo brano cancellato, e il sintomo sarebbe un
/// pannello che dice «142 completati» su una libreria che ne ha novanta.
fn quanti(connection: &rusqlite::Connection, sql: &str) -> Result<i64, AppError> {
    connection
        .query_row(sql, [], |row| row.get(0))
        .map_err(|err| db_errore("conteggio dell'arricchimento", &err))
}
