//! L'arricchimento dei metadati, dal lato della finestra.
//!
//! Quattro comandi, due eventi e un filo di sottofondo. Le decisioni stanno in
//! `rymembra-domain`, le richieste in `rymembra-meta`, l'orchestrazione in
//! `rymembra_app::enrich`: qui si tiene il tempo e si prende il lucchetto — le due
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
//! MusicBrainz, le copertine che scendono dal Cover Art Archive e finiscono
//! nello store — dura da secondi a minuti e avviene **senza nessun
//! lucchetto**.
//!
//! Che sia vero non è affidato all'attenzione di chi legge: `rymembra-meta` non
//! riceve mai una `rusqlite::Connection`, quindi il codice che terrebbe il
//! lucchetto durante una richiesta di rete non si può nemmeno scrivere.
//!
//! # Perché l'interruttore e il ritorno indietro sono qui
//!
//! Fino alla 2.3.0 questo era l'unico filo di Rymembra che **scriveva nei file
//! dell'utente senza che nessuno guardasse prima**, e questa nota argomentava
//! perché fosse accettabile. Dalla 2.3.1 la domanda non si pone: quel che
//! l'arricchimento decide finisce nelle righe di `tracks` e in
//! `track_meta_arricchita`, e i file restano quelli che erano — nemmeno
//! aperti. Il valore di serie dell'interruttore, acceso, è finalmente
//! difendibile per la ragione più semplice che ci sia: **non c'è niente di
//! irreversibile da difendere.**
//!
//! Restano due garanzie, e sono le stesse di prima lette in un'altra luce:
//!
//! * si scrive solo su verdetto `Applica`, e astenersi è il comportamento
//!   normale (la ragione per esteso sta in testa a `rymembra_domain::enrich`);
//! * quel che si scrive si dimentica con un clic, perché sta in una tabella e
//!   non dentro migliaia di file: `rymembra_app::enrich::dimentica` la svuota e
//!   rilegge i tag dai file, che sono intatti.
//!
//! Da qui i due comandi che sembrano accessori e non lo sono:
//! [`arricchimento_attiva`] per spegnerlo e [`arricchimento_annulla`] per
//! dimenticare. Un sistema che lavora da solo sulla libreria di qualcun altro
//! deve essere spegnibile e reversibile da un posto che quel qualcuno trova —
//! e il posto sono le Impostazioni, accanto alle cartelle sorvegliate.
//!
//! # Il terzo comando, che è un'uscita a termine
//!
//! [`arricchimento_riporta_nei_file`] è l'unica cosa in tutta Rymembra che
//! riscriva ancora i tag dentro i file dell'utente, ed è lì solo per **disfare
//! quel che hanno fatto le versioni passate**: le righe di `enrich_undo` che
//! la 2.3.0 e prima di lei hanno lasciato in tabella. Non si può chiamare per
//! sbaglio — è un comando a parte, con la sua etichetta che dice che scrive nei
//! file — non ha un valore di serie, e il CHANGELOG dichiara che sparirà in una
//! release futura, quando quelle righe non serviranno più a nessuno. È una via
//! d'uscita a termine, non una funzione del programma.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use crate::spegnimento::Emette as _;
use rymembra_app::covers::CoverStore;
use rymembra_app::enrich::{self, DepositoSqlite, Gruppo};
use rymembra_app::library::{Toccati, ricostruisci_aggregati};
use rymembra_app::settings;
use rymembra_app::vicinanza;
use rymembra_domain::enrich::Verdetto;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_meta::Fornitori;
use serde::Serialize;
use tauri::{AppHandle, Manager as _, State};

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
/// si registra nella sua transazione, e fuori dalle transazioni non si scrive
/// più niente da nessuna parte — quindi al massimo si perde il lavoro di rete
/// già fatto sul gruppo in corso, che la passata dopo rifà.
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

    /// Il turno delle passate, per chi deve lavorare quando nessuna è in corso.
    ///
    /// Lo prende la pulizia delle copertine della manutenzione: una passata
    /// salva le copertine fuori dal lucchetto e le registra dopo, e una pulizia
    /// nel mezzo toglierebbe il file di una copertina che sta per essere
    /// registrata. Chi lo tiene fa saltare la passata che si sveglia in quel
    /// momento, come fanno «Dimentica» e «Riporta nei file».
    pub fn turno(&self) -> Option<Turno<'_>> {
        Turno::prendi(&self.in_corso)
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
    /// Quanti brani portano metadati messi dall'arricchimento.
    ///
    /// È il numero che decide se il pulsante «annulla» abbia senso: a zero non
    /// c'è niente da dimenticare, e mostrarlo attivo prometterebbe qualcosa che
    /// non succede.
    pub annullabili: i64,
    /// Quanti file le versioni passate hanno riscritto e non hanno mai disfatto.
    ///
    /// Conta le righe di `enrich_undo`, che dalla 2.3.1 nessuno scrive più: può
    /// solo calare, e a zero l'uscita a termine di
    /// [`arricchimento_riporta_nei_file`] non ha più niente da riportare. È un
    /// campo a parte da `annullabili` perché sono due gesti diversi — uno
    /// dimentica una tabella, l'altro riapre migliaia di file — e mostrarli
    /// sotto lo stesso numero farebbe premere il secondo a chi voleva il primo.
    pub nei_file: i64,
    /// Quando è finita l'ultima passata.
    pub ultimo_ms: Option<i64>,
    /// Com'è andata l'ultima passata automatica.
    pub errore: Option<ErroreIpc>,
}

/// Com'è andato un ritorno indietro, dell'uno o dell'altro tipo.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EsitoAnnullamentoIpc {
    /// Quanti brani sono tornati a quel che dice il loro file.
    pub riportati: usize,
    /// Quanti brani non si sono potuti riportare.
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

/// Dimentica quel che l'arricchimento ha deciso, e rimette le righe sui tag dei file.
///
/// # Perché spegne anche l'interruttore
///
/// Perché chi dimentica sta dicendo «non era quello che volevo», non «rifallo
/// fra poco», e con l'automatico acceso la passata successiva ricomincerebbe da
/// quei brani entro mezz'ora. Non basterebbe lo stato terminale a fermarla per
/// sempre: `enrich_status` torna a `undone`, che la clausola dei candidati
/// esclude, ma il primo brano che una scansione tocca davvero — o una riga
/// aggiunta a mano — la rimetterebbe in moto sul resto della libreria. Spegnere
/// è quindi l'unica lettura onesta del gesto; riaccenderlo è un clic nello
/// stesso pannello.
///
/// # Perché `(async)` su una funzione che non è `async`
///
/// Perché un `#[tauri::command]` normale gira sul filo dell'anello degli eventi
/// della finestra. Questo **rilegge** un file per ogni brano arricchito — su
/// una libreria vera possono essere centinaia, e su una radice di rete lenta
/// sono secondi. `(async)` su una funzione sincrona la manda sulla riserva di
/// fili di Tauri: il corpo resta bloccante e ordinario, cambia solo **dove**
/// gira.
#[tauri::command(async)]
pub fn arricchimento_annulla(
    app: AppHandle,
    stato: State<'_, Stato>,
    arricchimento: State<'_, StatoArricchimento>,
) -> Esito<EsitoAnnullamentoIpc> {
    let annullati = (|| {
        // Il turno prima di tutto: una passata in corso sta scrivendo sulle
        // stesse righe che questo riporta indietro, e quale dei due vinca
        // dipenderebbe dall'ordine in cui i due fili arrivano alla transazione.
        let _turno = Turno::prendi(&arricchimento.in_corso).ok_or_else(occupato)?;

        // Poi tre tempi, come [`arricchimento_riporta_nei_file`]: il lucchetto
        // della libreria nel primo e nel terzo, e in mezzo la rilettura di un
        // file per ogni brano arricchito — che su un NAS addormentato sono
        // secondi l'uno, con scansione, backup e play fermi ad aspettare.
        //
        // Nel primo tempo anche le cartelle dei `.cue` dei brani d'immagine:
        // dopo, l'annotazione che le nomina non c'è più. Vedi
        // [`cartelle_dei_fogli_arricchiti`].
        let (da_fare, cartelle) = con_libreria(&stato, |libreria| {
            settings::write(&libreria.connection, CHIAVE_ATTIVO, "0")?;
            Ok((
                enrich::da_dimenticare(&libreria.connection)?,
                cartelle_dei_fogli_arricchiti(&libreria.connection)?,
            ))
        })?;
        let riletti = enrich::rileggi_i_file(da_fare);
        let annullati = con_libreria(&stato, |libreria| {
            enrich::registra_dimenticati(&mut libreria.connection, riletti)
        })?;
        Ok::<_, AppError>((annullati, cartelle))
    })();
    // Fuori dalla chiusura, cioè con il turno ormai lasciato cadere: qui dentro
    // lo stato direbbe `inCorso: true` per un'operazione appena finita, e il
    // pulsante resterebbe disabilitato fino al comando successivo.
    scorda_errore(&arricchimento);
    // I brani d'immagine dimenticati aspettano la scansione che li ricostruisce
    // dal loro `.cue`, e sul disco non è cambiato niente che la faccia partire:
    // la si chiede alla sorveglianza, per quelle cartelle.
    let annullati = annullati.map(|(annullati, cartelle)| {
        crate::sorveglia::riguarda(&app, cartelle);
        annullati
    });

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

/// Riscrive nei file i tag di prima, per quel che le versioni passate hanno già toccato.
///
/// # Che cosa fa che nessun altro comando fa
///
/// **Apre e riscrive i file dell'utente.** È l'unico che lo faccia in tutta
/// Rymembra, esiste solo per disfare le scritture della 2.3.0 e di prima, e lo
/// dice l'etichetta del bottone che lo chiama: senza quella scritta, un utente
/// che ha appena letto «Rymembra non modifica i tuoi file» premerebbe un pulsante
/// che glieli modifica. Vedi la nota in testa al modulo: è una via d'uscita a
/// termine, e il CHANGELOG la dà per rimossa in una release futura.
///
/// Non spegne l'interruttore, e non deve: non sta dicendo «l'arricchimento ha
/// sbagliato», sta dicendo «togli dai miei file quel che una vecchia versione
/// ci ha messo». Chi vuole anche l'altra cosa preme anche l'altro pulsante.
///
/// # Perché `(async)`
///
/// La stessa ragione di [`arricchimento_annulla`], moltiplicata: qui i file si
/// riaprono **in scrittura**, uno per uno, e su una libreria vera sono decine
/// di secondi in cui la finestra sarebbe congelata.
#[tauri::command(async)]
pub fn arricchimento_riporta_nei_file(
    app: AppHandle,
    stato: State<'_, Stato>,
    arricchimento: State<'_, StatoArricchimento>,
) -> Esito<EsitoAnnullamentoIpc> {
    let riportati = (|| {
        // Come sopra: una passata in corso non deve incrociare una riscrittura
        // di file a metà.
        let _turno = Turno::prendi(&arricchimento.in_corso).ok_or_else(occupato)?;
        // Tre tempi, e il lucchetto della libreria solo nel primo e nel terzo:
        // in mezzo si riaprono in scrittura migliaia di file, e su una share
        // sono minuti in cui ogni altro comando restava fermo ad aspettare.
        // Il turno resta preso per tutti e tre, quindi nessuna passata
        // dell'arricchimento può scrivere nelle stesse righe nel frattempo.
        let da_fare = con_libreria(&stato, |libreria| {
            enrich::da_riportare(&libreria.connection)
        })?;
        // Fra un file e l'altro si guarda se l'applicazione sta uscendo: sono
        // minuti su una share, e chi chiude non deve aspettare né trovarsi il
        // processo trattenuto. Quel che è già riscritto si registra lo stesso
        // qui sotto; il resto resta in `enrich_undo` per la volta dopo. Un
        // «Annulla» dalla finestra non c'è ancora: il predicato è il posto
        // dove attaccarlo.
        let riscritti = enrich::riscrivi_nei_file(da_fare, &crate::spegnimento::in_uscita);
        // Il solo esito che chiede a qualcuno di guardare il disco: accanto a
        // questi brani è rimasta la copia intera di com'erano. Il numero e il
        // nome con cui riconoscerle, non i percorsi: sono i titoli della
        // libreria di qualcuno, e il diario si spedisce.
        if riscritti.riserve_rimaste() > 0 {
            nota!(
                "[arricchimento] {} brani non si sono potuti rimettere com'erano: accanto a ognuno è rimasta la sua copia intera, col nome del brano più «.{}-<numero>.tmp»",
                riscritti.riserve_rimaste(),
                std::process::id()
            );
        }
        con_libreria(&stato, |libreria| {
            enrich::registra_riportati(&mut libreria.connection, riscritti)
        })
    })();
    scorda_errore(&arricchimento);

    let esito = riportati.and_then(|riportati| {
        Ok(EsitoAnnullamentoIpc {
            riportati: riportati.riportati,
            falliti: riportati.falliti,
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

/// Una passata lasciata a una scansione: si riprova una raffica dopo.
///
/// Il filo, da solo, tornerebbe fra mezz'ora ([`INTERVALLO`]), e una scansione
/// che non ha cambiato niente non lo sveglia. Una sveglia `Sporca` mandata a
/// sé stessi lo fa riprovare dopo [`RAFFICA`]: se la scansione è ancora lì si
/// rimanda di nuovo, altrimenti si lavora.
fn rimanda_per_la_scansione(app: &AppHandle) {
    sporca(app);
}

/// Avvia il filo che arricchisce da solo.
///
/// Il ciclo sta in [`crate::stato::avvia_filo_periodico`], insieme a quello
/// della nuvola e della sincronia: un thread nominato, `recv_timeout` che fa da
/// periodicità **e** da antirimbalzo, nessun timer e nessun runtime asincrono.
/// Con tokio arriverebbe una seconda idea di cos'è un errore e di chi possiede un
/// thread, per fare qualche richiesta HTTP ogni mezz'ora.
///
/// # Un filo, non tre
///
/// Il vecchio albero ne usava tre, per sovrapporre il lavoro di CPU di `fpcalc`
/// alle richieste di rete. Senza impronta acustica non c'è lavoro di CPU da
/// sovrapporre, e tre fili si metterebbero soltanto in coda sullo stesso cancello
/// da una richiesta al secondo — pagando tre lucchetti per la velocità di uno.
pub fn avvia_filo(app: AppHandle, orecchio: Receiver<Sveglia>) {
    let avviato = crate::stato::avvia_filo_periodico(
        "rymembra-arricchimento",
        orecchio,
        ATTESA_AVVIO,
        RAFFICA,
        INTERVALLO,
        // Solo `Sporca` aspetta la raffica: venti brani scaricati sono una
        // passata, non venti — vedi [`RAFFICA`]. `Subito` è una richiesta a
        // mano, e chi l'ha fatta sta guardando la finestra.
        |sveglia| *sveglia == Sveglia::Sporca,
        {
            // I fornitori vivono qui, catturati dalla passata e quindi sul filo,
            // e non nello stato condiviso: dentro ci sono la riserva di
            // connessioni di `ureq` e lo stato degli interruttori, che devono
            // sopravvivere fra una passata e l'altra — ma nessun altro filo li
            // guarda, e metterli dietro un mutex vorrebbe dire un lucchetto
            // tenuto per minuti che nessuno aspetta.
            let mut fornitori: Option<Fornitori> = None;
            move || passata(&app, &mut fornitori)
        },
    );
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
            app.emetti("arricchimento:esito", *bilancio);
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
    /// Righe di somiglianza culturale raccolte.
    vicini: usize,
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
        // Qualcuno sta dimenticando l'arricchimento a mano, o riportando i tag
        // nei file. La passata successiva arriva fra mezz'ora: non c'è niente
        // da segnalare.
        return Ok(None);
    };

    // Una scansione in corso ha la precedenza: l'arricchimento non parte
    // mentre gira. Le due scrivono le stesse righe — una dai tag, l'altra dai
    // cataloghi — e la scansione ha qualcuno che la aspetta. Ci si risveglia da
    // soli, una raffica dopo: vedi [`rimanda_per_la_scansione`].
    if stato.scansione_in_corso() {
        rimanda_per_la_scansione(app);
        return Ok(None);
    }

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
    // Le chiavi d'album e gli artisti che le scritture hanno toccato, per la
    // finestra 3: vedi [`Toccati`].
    let mut toccati = Toccati::nessuno();
    let mut interrotta = false;
    // Il guasto che ha fermato il ciclo, se ce n'è uno. Si tiene da parte invece
    // di uscire con un `?`: i gruppi registrati prima sono già scritti, ognuno
    // nella sua transazione, e uscire da qui saltava la finestra 3 — gli album
    // di quei brani restavano quelli di prima, e la libreria segnata in regola
    // non lo avrebbe saputo. Si ferma il ciclo, si chiude la passata, e solo
    // dopo si restituisce l'errore.
    let mut guasto: Option<AppError> = None;
    for (fatti, gruppo) in da_fare.gruppi.iter().enumerate() {
        // Prima di ogni gruppo, e non solo all'inizio della passata: dodici
        // gruppi sono minuti di richieste a una al secondo, e in quei minuti si
        // può chiudere l'applicazione o spegnere l'interruttore. Prima nessuno
        // dei due fermava niente: la chiusura aspettava la fine del lotto, e
        // chi spegneva in Impostazioni guardava le richieste continuare — cioè
        // un interruttore che non spegne, e la promessa di `PRIVACY.md` mancata
        // per qualche minuto. L'interruttore si rilegge dal database, che è
        // dove sta: un lucchetto breve per gruppo, contro un gruppo che ne dura
        // decine di secondi. Quel che è già registrato resta, ed è intero:
        // ogni gruppo ha la sua transazione. Un interruttore che non si legge
        // ferma il ciclo come uno spento: vedi `guasto`.
        // E una scansione cominciata nel frattempo la si lascia passare, fra
        // un gruppo e l'altro: non è un guasto, i gruppi già registrati
        // restano, gli album si rifanno qui sotto come dopo ogni
        // interruzione, e il resto lo fa la passata dopo.
        if stato.scansione_in_corso() {
            interrotta = true;
            rimanda_per_la_scansione(app);
            break;
        }
        match da_interrompere(&stato) {
            Ok(false) => {}
            Ok(true) => {
                interrotta = true;
                break;
            }
            Err(err) => {
                interrotta = true;
                guasto = Some(err);
                break;
            }
        }
        avanza(app, fatti, totale);

        let decisione = enrich::decidi(servizi, gruppo);
        if decisione.non_raggiungibile {
            // Nessuna fonte ha risposto. **Non si registra niente**: marchiare
            // come introvabili i brani incontrati mentre il portatile era
            // staccato dal wifi vorrebbe dire non riprovarli per un mese. Ci si
            // ferma anche per i gruppi dopo, che troverebbero la stessa rete.
            break;
        }

        // I file dell'utente non si toccano: qui si salvano soltanto le
        // copertine nello store di Rymembra. Vedi la nota in testa al modulo.
        let (esiti, guasti) = enrich::applica(&da_fare.covers, &decisione.scritture);
        for guasto in &guasti {
            // Un'immagine che non si salva — disco pieno, byte illeggibili —
            // non ferma la passata e non butta via titolo e anno del brano a
            // cui apparteneva: si dice e si va avanti.
            nota!(
                "[arricchimento] copertina non salvata codice={} causa={}",
                guasto.code().kind().code(),
                guasto.cause().unwrap_or("—")
            );
        }

        // ── finestra 2: registrare questo gruppo. ───────────────────────────
        //
        // Un gruppo che non si registra ferma il ciclo come l'interruttore che
        // non si legge, e per la stessa ragione: i gruppi di prima sono già
        // scritti, e la finestra 3 deve arrivare anche per loro.
        let registrato = con_libreria(&stato, |libreria| {
            // `Immediate`, come le altre due di questo file: la transazione
            // legge e poi scrive, e il `DepositoSqlite` scrive la sua cache
            // sullo stesso file da un'altra connessione. Una transazione
            // normale che a metà diventa di scrittura riceve `SQLITE_BUSY`
            // senza attesa; chiedere il lucchetto all'inizio la fa aspettare.
            let tx = libreria
                .connection
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(|err| db_errore("apertura della transazione di arricchimento", &err))?;
            // Ogni scrittura ha il suo esito, sempre: la fase tre non può più
            // fallire su un brano, perché non apre più nessun file. Fin qui
            // c'era un secondo giro che segnava `enrich_status = 'error'` sulle
            // scritture rimaste senza esito — non ne resta nessuna, e tenere
            // quel giro vorrebbe dire tenere un rimedio a un guasto che non
            // esiste più.
            let ids: Vec<i64> = decisione.scritture.iter().map(|s| s.track_id).collect();
            toccati.segna_brani(&tx, &ids)?;
            let conto =
                enrich::registra(&tx, &decisione, &decisione.scritture, &esiti, adesso_ms())?;
            toccati.segna_brani(&tx, &ids)?;
            tx.commit()
                .map_err(|err| db_errore("chiusura della transazione di arricchimento", &err))?;
            Ok(conto)
        });
        let conto = match registrato {
            Ok(conto) => conto,
            Err(err) => {
                interrotta = true;
                guasto = Some(err);
                break;
            }
        };

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

    // ── le somiglianze culturali, in coda alla stessa passata. ──────────────
    //
    // Qui e non su un filo suo per tre ragioni che si sommano. La prima è che
    // dipende da quel che l'arricchimento ha appena fatto: senza un
    // `mb_recording_id` non c'è niente da chiedere, e chiederlo un istante dopo
    // che è stato scritto è il momento migliore possibile. La seconda è che ha
    // già il `Fornitori` in mano, con la sua cadenza e il suo interruttore. La
    // terza è che così eredita l'interruttore dell'utente: chi spegne
    // l'arricchimento in Impostazioni › Metadati spegne anche questo, che è
    // esattamente quel che `PRIVACY.md` gli promette.
    //
    // Un guasto non fa cadere la passata: i metadati sono già scritti, e
    // perderli per una domanda sulle somiglianze sarebbe il torto sbagliato.
    //
    // Una passata interrotta le salta: sono altre richieste in rete, e sia chi
    // esce sia chi ha spento l'interruttore ha appena detto di non volerne.
    bilancio.vicini = if interrotta {
        0
    } else {
        match vicini(&stato, servizi) {
            Ok(quanti) => quanti,
            Err(err) => {
                nota!(
                    "[arricchimento] vicini non raccolti codice={} causa={}",
                    err.code().kind().code(),
                    err.cause().unwrap_or("—")
                );
                0
            }
        }
    };

    // ── finestra 3: gli aggregati, una volta sola. ──────────────────────────
    //
    // Anche dopo un'interruzione: è solo database, per chiavi, e i gruppi già
    // registrati hanno bisogno dei loro album rifatti quanto quelli di una
    // passata intera.
    //
    // Qui e non dentro il ciclo: riscrivere gli stessi album dodici volte di
    // fila costerebbe dodici volte per un risultato che conta solo alla fine.
    //
    // Per chiavi, come la scansione, e non per intero: su una libreria di
    // cinquantamila brani la ricostruzione intera passa i quattro secondi che
    // la finestra aspetta il lucchetto, e cuore, voto e playlist rinunciavano
    // con `db.locked` mentre girava. Il risultato è lo stesso: vedi
    // [`ricostruisci_aggregati`], che rifà anche gli album **fusi** con quelli
    // toccati attraverso un identificativo condiviso.
    let chiusura = con_libreria(&stato, |libreria| {
        chiudi_la_passata(&mut libreria.connection, bilancio.applicati > 0, &toccati)
    });

    // Il guasto che ha fermato il ciclo viene prima di quello della chiusura:
    // è lui che dice perché la passata non è finita.
    if let Some(err) = guasto {
        return Err(err);
    }
    chiusura?;
    Ok(Some(bilancio))
}

/// La finestra 3 di una passata: gli aggregati per le chiavi toccate, e l'ora
/// dell'ultima passata.
///
/// `rifare` dice se qualcosa è stato applicato: senza, non c'è niente da
/// ricostruire, ma l'ora si scrive lo stesso.
///
/// # Errori
///
/// Quello della ricostruzione se cade, e allora prima di tornare si chiede la
/// ricostruzione intera; altrimenti quello della scrittura dell'ora.
fn chiudi_la_passata(
    connection: &mut rusqlite::Connection,
    rifare: bool,
    toccati: &Toccati,
) -> Result<(), AppError> {
    let esito = (|| -> Result<(), AppError> {
        if rifare && !toccati.e_vuoto() {
            // L'arricchimento scrive `mb_release_group_id` su brani che
            // prima non l'avevano, ed è esattamente l'identificativo con
            // cui i gruppi d'album si **fondono**. Senza la ricostruzione,
            // il disco resterebbe spezzato nelle due schede che
            // l'arricchimento ha appena dimostrato essere una sola.
            let tx = connection
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(|err| db_errore("apertura della ricostruzione", &err))?;
            ricostruisci_aggregati(&tx, toccati)?;
            tx.commit()
                .map_err(|err| db_errore("chiusura della ricostruzione", &err))?;
        }
        Ok(())
    })();
    // I brani dei gruppi sono già scritti, ognuno nella sua transazione
    // chiusa un passo fa: se la ricostruzione cade — un `db.locked`
    // passeggero, o un guasto come quello delle chiavi d'album doppie della
    // 3.0 — gli album restano quelli di prima mentre i brani sono già quelli
    // nuovi, e la libreria segnata in regola non lo saprebbe mai. Le chiavi
    // toccate si perdono con questa passata, e l'unico modo di ritrovarle è
    // la ricostruzione intera alla prossima scansione: la si chiede, come
    // fa la scansione stessa al suo settimo passo. Sforzo minimo: se non
    // riesce nemmeno questa scrittura, l'errore da dire resta quello vero.
    if esito.is_err() {
        let _ = rymembra_app::library::chiedi_ricostruzione_intera(connection);
    }
    // L'ora dell'ultima passata si scrive **anche** quando sono caduti solo
    // gli aggregati. Dice quando la libreria ha ricevuto metadati, e quelli
    // sono arrivati: i conteggi accanto nel pannello — completati, senza
    // corrispondenza — sono già cambiati. Lasciarla indietro farebbe dire
    // al pannello «nessuna passata da ieri» sopra numeri di adesso; il
    // guasto degli album lo racconta l'errore della passata, che arriva lo
    // stesso. Ed è il primo errore a tornare, non quello di questa riga.
    let scritto = settings::write(connection, CHIAVE_ULTIMO, &adesso_ms().to_string());
    esito.and(scritto)
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
    app.emetti(
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
        app.emetti("arricchimento:stato", ipc);
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
/// Quel che rende accettabile il valore di serie non è questa riga, ed è
/// cambiato con la 2.3.1: prima era che si scrivesse solo su verdetto `Applica`
/// e che ogni scrittura fosse annullabile riaprendo i file. Adesso è molto più
/// semplice — **l'arricchimento non tocca i file dell'utente**, quel che decide
/// sta in due tabelle, e dimenticarlo è una `DELETE` più una rilettura. Un
/// interruttore acceso di serie che non produce niente di irreversibile non ha
/// bisogno di essere difeso: ha bisogno di essere spegnibile, e lo è. Vedi la
/// nota in testa al modulo.
fn attivo(connection: &rusqlite::Connection) -> Result<bool, AppError> {
    Ok(settings::read(connection, CHIAVE_ATTIVO)?.as_deref() != Some("0"))
}

/// Se la passata in corso deve smettere prima del prossimo gruppo: si sta
/// uscendo, o qualcuno ha spento l'interruttore mentre lavorava.
///
/// # Errori
///
/// Quelli di [`attivo`]: un database che non risponde qui non risponderebbe
/// nemmeno alla registrazione del gruppo, un passo dopo.
fn da_interrompere(stato: &Stato) -> Result<bool, AppError> {
    if crate::spegnimento::in_uscita() {
        return Ok(true);
    }
    con_libreria(stato, |libreria| attivo(&libreria.connection)).map(|acceso| !acceso)
}

/// Dimentica l'errore dell'ultima passata.
fn scorda_errore(arricchimento: &StatoArricchimento) {
    let mut ultimo = arricchimento
        .ultimo_errore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *ultimo = None;
}

/// Chiede a ListenBrainz i vicini di un lotto di brani, e li scrive.
///
/// Le tre finestre di sempre, nell'ordine di sempre: si legge chi manca, si
/// rilascia il lucchetto, si chiede alla rete, lo si riprende per scrivere.
/// Restituisce quante righe di somiglianza sono entrate — che è molto meno dei
/// vicini ricevuti, perché `vicinanza` tiene solo i brani che sono già in
/// libreria.
fn vicini(stato: &State<'_, Stato>, servizi: &Fornitori) -> Result<usize, AppError> {
    let lotto = con_libreria(stato, |libreria| {
        vicinanza::candidati(&libreria.connection, adesso_ms(), vicinanza::LOTTO)
    })?;
    if lotto.is_empty() {
        return Ok(0);
    }

    // ── senza lucchetto: al più quattro richieste, una al secondo. ──────────
    let trovati = vicinanza::chiedi(servizi, &lotto)?;

    let scritte = con_libreria(stato, |libreria| {
        let tx = libreria
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|err| db_errore("apertura della transazione dei vicini", &err))?;
        let scritte = vicinanza::registra(&tx, &trovati, adesso_ms())?;
        tx.commit()
            .map_err(|err| db_errore("chiusura della transazione dei vicini", &err))?;
        Ok(scritte)
    })?;
    Ok(scritte)
}

/// Un guasto del database, nella forma del catalogo.
///
/// Delega a [`rymembra_app::db::codice_da_sqlite`]: è la seconda connessione allo
/// stesso file, quindi è anche il punto da cui arriva il `SQLITE_BUSY` più
/// frequente di tutta l'applicazione — l'arricchimento che legge mentre la
/// scansione scrive. Raccontarlo come «il database ha rifiutato una richiesta»
/// era dire all'utente di riavviare per una coda di mezzo secondo.
fn db_errore(cosa: &str, err: &rusqlite::Error) -> AppError {
    rymembra_app::db::codice_da_sqlite(cosa, err)
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
            annullabili: quanti(
                &libreria.connection,
                "SELECT COUNT(*) FROM track_meta_arricchita",
            )?,
            nei_file: quanti(&libreria.connection, CONTA_DA_RIPORTARE)?,
            ultimo_ms: settings::read(&libreria.connection, CHIAVE_ULTIMO)?
                .and_then(|quando| quando.parse().ok()),
            errore,
        })
    })
}

/// Quante fotografie di `enrich_undo` «Riporta nei file» riporterebbe.
///
/// Lo stesso filtro di `rymembra_app::enrich::da_riportare`: le righe che sono
/// brani d'immagine CD restano fuori, perché nei file non si riportano mai —
/// il file è l'immagine di tutto il disco. Contarle lo stesso lasciava il
/// numero sopra zero per sempre, e il pulsante acceso su un lavoro che non
/// avrebbe fatto niente. Le fotografie senza più la riga contano, come là.
const CONTA_DA_RIPORTARE: &str = "SELECT COUNT(*) FROM enrich_undo u
                                    LEFT JOIN tracks t ON t.id = u.track_id
                                   WHERE t.immagine IS NULL";

/// Le cartelle dei `.cue` dei brani d'immagine CD arricchiti.
///
/// Servono a «Dimentica» ([`arricchimento_annulla`]): un brano d'immagine non
/// si rilegge dal suo file — il file è l'immagine di tutto il disco, e il
/// titolo vero sta nel `.cue` — e il nucleo lo segna da rileggere alla
/// prossima scansione (`date_modified = -1`). Ma i file sul disco non sono
/// cambiati, quindi nessun avviso farebbe partire quella scansione da sé: va
/// chiesta, per queste cartelle. Si leggono **prima** di dimenticare, perché
/// dopo l'annotazione che le nomina non c'è più.
fn cartelle_dei_fogli_arricchiti(
    connection: &rusqlite::Connection,
) -> Result<Vec<String>, AppError> {
    let fallita = |err: rusqlite::Error| db_errore("cartelle dei fogli arricchiti", &err);
    let mut statement = connection
        .prepare(
            "SELECT t.path FROM track_meta_arricchita m
               JOIN tracks t ON t.id = m.track_id
              WHERE t.immagine IS NOT NULL AND t.path IS NOT NULL",
        )
        .map_err(fallita)?;
    let percorsi = statement
        .query_map([], |riga| riga.get::<_, String>(0))
        .map_err(fallita)?;
    let mut cartelle: Vec<String> = Vec::new();
    for percorso in percorsi {
        let percorso = percorso.map_err(fallita)?;
        let cartella = rymembra_domain::cue::brano_d_immagine(&percorso)
            .and_then(|(cue, _)| std::path::Path::new(cue).parent())
            .map(|cartella| cartella.to_string_lossy().into_owned());
        if let Some(cartella) = cartella
            && !cartelle.contains(&cartella)
        {
            cartelle.push(cartella);
        }
    }
    Ok(cartelle)
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

#[cfg(test)]
mod prove {
    use super::*;

    /// L'interruttore spento a passata in corso si legge prima del gruppo dopo.
    ///
    /// È quel che il ciclo della passata chiede a ogni giro: prima non lo
    /// chiedeva nessuno, e chi spegneva in Impostazioni guardava le richieste
    /// continuare fino alla fine del lotto.
    #[test]
    fn l_interruttore_spento_ferma_la_passata_al_gruppo_dopo() {
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        assert_eq!(da_interrompere(&stato), Ok(false), "acceso di serie");

        con_libreria(&stato, |libreria| {
            settings::write(&libreria.connection, CHIAVE_ATTIVO, "0")
        })
        .expect("spento");
        assert_eq!(da_interrompere(&stato), Ok(true));

        con_libreria(&stato, |libreria| {
            settings::write(&libreria.connection, CHIAVE_ATTIVO, "1")
        })
        .expect("riacceso");
        assert_eq!(da_interrompere(&stato), Ok(false));
    }

    /// Se la ricostruzione degli aggregati cade dopo che i gruppi sono già
    /// scritti, la prossima scansione li rifà per intero — le chiavi toccate
    /// si perdono con la passata — e l'ora dell'ultima passata si scrive lo
    /// stesso. L'errore che torna è quello della ricostruzione.
    #[test]
    fn aggregati_caduti_chiedono_la_ricostruzione_intera() {
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        let mut guardia = stato.libreria.lock().expect("lucchetto");
        let libreria = guardia.as_mut().expect("libreria aperta");
        let connection = &mut libreria.connection;
        // Un brano scritto da un gruppo, gli aggregati segnati in regola, e un
        // guasto che fa cadere ogni scrittura sugli album: è il `db.locked` o
        // la chiave doppia, senza doverli fabbricare.
        connection
            .execute_batch(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, album_key,
                                     duration_ms, file_size, date_added, date_modified)
                 VALUES (1, 'C:\\M\\a.mp3', 'k', 'T', 'A', 'Al', 'gruppo', 1000, 1, 0, 0);
                 UPDATE libreria_stato SET aggregati = 99 WHERE id = 1;
                 CREATE TEMP TRIGGER guasto_i BEFORE INSERT ON albums
                   BEGIN SELECT RAISE(ABORT, 'guasto di prova'); END;
                 CREATE TEMP TRIGGER guasto_d BEFORE DELETE ON albums
                   BEGIN SELECT RAISE(ABORT, 'guasto di prova'); END;
                 CREATE TEMP TRIGGER guasto_u BEFORE UPDATE ON albums
                   BEGIN SELECT RAISE(ABORT, 'guasto di prova'); END;",
            )
            .expect("banco");
        let mut toccati = Toccati::nessuno();
        toccati.segna_brani(connection, &[1]).expect("toccati");
        assert!(!toccati.e_vuoto());

        let esito = chiudi_la_passata(connection, true, &toccati);
        assert!(esito.is_err(), "la ricostruzione doveva cadere");

        let aggregati: i64 = connection
            .query_row(
                "SELECT aggregati FROM libreria_stato WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .expect("versione");
        assert_eq!(aggregati, 0, "la prossima ricostruzione dev'essere intera");
        assert!(
            settings::read(connection, CHIAVE_ULTIMO)
                .expect("lettura")
                .is_some(),
            "l'ora dell'ultima passata si scrive anche così"
        );
    }

    /// Un file intero e due brani di un'immagine CD, arricchiti tutti e tre,
    /// con le fotografie di `enrich_undo` di una versione passata.
    fn banco_d_immagine(connection: &rusqlite::Connection) {
        connection
            .execute_batch(
                r"INSERT INTO tracks (id, path, track_key, title, artist, album, album_key,
                                     duration_ms, file_size, date_added, date_modified)
                 VALUES (1, 'C:\M\a.mp3', 'k1', 'T', 'A', 'Al', 'g', 1000, 1, 0, 0);
                 INSERT INTO tracks (id, path, track_key, title, artist, album, album_key,
                                     duration_ms, file_size, date_added, date_modified,
                                     immagine, inizio_cd, fine_cd)
                 VALUES (2, 'C:\M\Disco\Album.cue|01', 'k2', 'Uno', 'A', 'D', 'd',
                         1000, 9, 0, 0, 'C:\M\Disco\Album.flac', 0, 7500),
                        (3, 'C:\M\Disco\Album.cue|02', 'k3', 'Due', 'A', 'D', 'd',
                         1000, 9, 0, 0, 'C:\M\Disco\Album.flac', 7500, NULL);
                 INSERT INTO track_meta_arricchita (track_id, campi, set_at)
                 VALUES (1, '{}', 0), (2, '{}', 0), (3, '{}', 0);
                 INSERT INTO enrich_undo (track_id, path, tags, written_at)
                 VALUES (1, 'C:\M\a.mp3', '{}', 0),
                        (2, 'C:\M\Disco\Album.flac', '{}', 0);",
            )
            .expect("banco");
    }

    /// Le fotografie dei brani d'immagine non si riportano mai nei file, e
    /// non devono tenere acceso il numero che dice quante ce ne sono.
    #[test]
    fn i_brani_d_immagine_non_contano_fra_quelli_da_riportare_nei_file() {
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        let guardia = stato.libreria.lock().expect("lucchetto");
        let connection = &guardia.as_ref().expect("libreria aperta").connection;
        banco_d_immagine(connection);
        assert_eq!(quanti(connection, CONTA_DA_RIPORTARE), Ok(1));
    }

    /// Dopo «Dimentica» i brani d'immagine si ricostruiscono dal loro `.cue`
    /// alla scansione dopo, che va chiesta per la sua cartella: una volta
    /// sola, anche se i brani sono due.
    #[test]
    fn dimenticare_un_immagine_fa_riguardare_la_cartella_del_suo_foglio() {
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        let guardia = stato.libreria.lock().expect("lucchetto");
        let connection = &guardia.as_ref().expect("libreria aperta").connection;
        banco_d_immagine(connection);
        assert_eq!(
            cartelle_dei_fogli_arricchiti(connection),
            Ok(vec![r"C:\M\Disco".to_owned()])
        );
    }
}
