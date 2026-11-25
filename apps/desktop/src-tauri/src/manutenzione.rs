//! La manutenzione della libreria: il controllo del database e la pulizia
//! delle copertine.
//!
//! # Cosa fa, e quando
//!
//! Un filo che si sveglia da solo, come quelli di nuvola, sincronia e
//! arricchimento, e che fa tre cose:
//!
//! 1. **una volta per sessione**, un minuto dopo l'avvio, il controllo rapido
//!    del database (`PRAGMA quick_check`) su una connessione sua, con un tempo
//!    massimo. Non all'apertura: su cinquantamila brani sono due decimi di
//!    secondo a cache calda e molto di più a cache fredda, e l'apertura gira
//!    sul filo della finestra. I numeri stanno in `rymembra_app::db`;
//! 2. **ogni sei ore**, `PRAGMA optimize`, che la documentazione di SQLite
//!    chiede per le connessioni che restano aperte a lungo. Quella di chiusura
//!    la fa [`crate::stato::ottimizza_uscendo`], quella di apertura il nucleo;
//! 3. la **pulizia delle copertine** che nessun brano, album, artista o
//!    playlist nomina più — dopo ogni scansione che ha cambiato qualcosa (la
//!    sveglia la dà il turno della scansione quando cade, se la scansione l'ha
//!    chiesta: vedi [`crate::stato::TurnoDiScansione`]) e al battito
//!    periodico, se ce ne sono. Una scansione su un disco invariato non
//!    sveglia nessuno; dopo una che non ha tolto niente il conto delle orfane
//!    dice zero, e la pulizia non parte.
//!
//! E due comandi per le Impostazioni: quanto occupa la cache delle copertine
//! e com'è andato il controllo ([`manutenzione_stato`]), e la pulizia a mano
//! ([`manutenzione_pulisci_copertine`]). Più uno per la finestra intera: il
//! solo esito del controllo ([`manutenzione_salute`]), che l'avviso di database
//! danneggiato chiede all'apertura.
//!
//! # Un database danneggiato
//!
//! Il controllo che trova un danno produce [`ErrorCode::DbCorrupt`] — lo stesso
//! codice, la stessa frase e la stessa gravità di una query che inciampa su una
//! pagina rotta — e da lì questo filo **smette di scrivere**: niente
//! ottimizzazione, niente pulizie. Continuare a scrivere su un file malformato
//! è il modo di trasformare un backup ancora buono in due file rotti. L'esito
//! va nel diario, nello stato che le Impostazioni mostrano, e nell'evento
//! `manutenzione:salute` per chi lo vuole dire subito.

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use rymembra_app::covers::{self, Liberato, Occupazione};
use rymembra_app::db::{self, Controllo};
use rymembra_domain::errors::{AppError, ErrorCode};
use serde::Serialize;
use tauri::{AppHandle, Manager as _};

use crate::arricchimento::StatoArricchimento;
use crate::disparte::{comanda, in_disparte};
use crate::errore::{ErroreIpc, Esito, errore};
use crate::nota;
use crate::spegnimento::Emette as _;
use crate::stato::{NOME_DATABASE, Stato, adesso_ms, con_libreria};

// ── i tempi del filo ────────────────────────────────────────────────────────

/// Quanto aspetta il filo prima della prima passata: un minuto.
///
/// Dopo la sincronia (venti secondi) e il backup (mezzo minuto), prima
/// dell'arricchimento (un minuto e mezzo) e dell'analisi sonora (due minuti):
/// il controllo legge tutto il database, e farlo mentre gli altri fili fanno la
/// loro prima passata vorrebbe dire contendersi il disco proprio quando chi ha
/// appena aperto Rymembra sta scegliendo cosa ascoltare.
const ATTESA_AVVIO: Duration = Duration::from_secs(60);

/// Quanto aspettare dopo una scansione prima di pulire: dieci secondi.
///
/// Due scansioni a poca distanza — quella a mano e quella automatica della coda
/// dei download — sono una pulizia, non due.
const RAFFICA: Duration = Duration::from_secs(10);

/// Ogni quanto il filo si sveglia da solo: sei ore.
///
/// La documentazione di SQLite suggerisce `optimize` «ogni qualche ora» su una
/// connessione che resta aperta; chi tiene Rymembra nel vassoio per giorni lo
/// riceve quattro volte al giorno, e a ogni battito costa meno di un
/// millisecondo quando non c'è niente da fare.
const INTERVALLO: Duration = Duration::from_secs(6 * 60 * 60);

/// Il tempo massimo del controllo rapido: trenta secondi.
///
/// Sul database da cinquantamila brani ne servono due decimi a cache calda. Il
/// tetto è per il disco meccanico addormentato e per la cartella dati su una
/// chiavetta: oltre, il controllo si ferma e la sessione resta con un «non
/// finito», che è meglio di un lettore che tiene aperta una transazione per
/// minuti.
const TEMPO_DEL_CONTROLLO: Duration = Duration::from_secs(30);

/// Quanto deve essere vecchio un avanzo nello store perché lo si tolga: un
/// giorno.
///
/// Vedi `CoverStore::spazza_avanzi`: un avanzo giovane può essere la scrittura
/// in corso di un altro filo, o una copertina che una scansione sta per
/// registrare. Un giorno è molto più di qualunque scansione.
const ETA_DEGLI_AVANZI: Duration = Duration::from_secs(24 * 60 * 60);

// ── lo stato ────────────────────────────────────────────────────────────────

/// Come è finito il controllo del database.
///
/// Sul filo è una parola sola — `daFare`, `sano`, `interrotto`, `saltato`,
/// `danneggiato`, `nonRiuscito`, le stesse dell'unione in `ipc/servizi.ts` —
/// ma qui è un `enum`: fino alla 3.2 era una stringa scritta in due posti, e la
/// guardia che ferma le scritture su un file malformato (`Salute::danneggiato`)
/// confrontava a mano la parola scritta da `salute_da`. Un refuso in uno solo
/// dei due la spegneva senza che niente fallisse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EsitoDelControllo {
    /// Il controllo non è ancora stato fatto.
    DaFare,
    /// Sano.
    Sano,
    /// Fermato dalla scadenza prima della fine.
    Interrotto,
    /// Non fatto, con il giornale che non è WAL o senza la cartella dati.
    Saltato,
    /// Il database è malformato: da qui il filo non scrive più.
    Danneggiato,
    /// Il controllo stesso non è riuscito.
    NonRiuscito,
}

impl EsitoDelControllo {
    /// La parola del filo, per il diario: la stessa che la finestra riceve, e
    /// che il diario scriveva quando l'esito era una stringa.
    const fn parola(self) -> &'static str {
        match self {
            Self::DaFare => "daFare",
            Self::Sano => "sano",
            Self::Interrotto => "interrotto",
            Self::Saltato => "saltato",
            Self::Danneggiato => "danneggiato",
            Self::NonRiuscito => "nonRiuscito",
        }
    }
}

/// Com'è andato il controllo del database in questa sessione, come lo vede la
/// finestra.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Salute {
    /// Una parola e non un `enum` con dei dati: la finestra ne legge una sola,
    /// e i campi qui sotto sono vuoti dove non servono. Vedi [`EsitoDelControllo`].
    pub esito: EsitoDelControllo,
    /// Quando è finito, in millisecondi dall'epoca.
    pub quando_ms: Option<i64>,
    /// Quanto è durato.
    pub millisecondi: Option<u64>,
    /// Il guasto, per `danneggiato` e `nonRiuscito`: il primo è
    /// `db.corrupt`, con la sua frase.
    pub errore: Option<ErroreIpc>,
}

impl Salute {
    /// Il controllo non è ancora stato fatto.
    fn da_fare() -> Self {
        Self {
            esito: EsitoDelControllo::DaFare,
            quando_ms: None,
            millisecondi: None,
            errore: None,
        }
    }

    /// Un esito senza guasto.
    fn fatto(esito: EsitoDelControllo, millisecondi: Option<u64>) -> Self {
        Self {
            esito,
            quando_ms: Some(adesso_ms()),
            millisecondi,
            errore: None,
        }
    }

    /// Un esito con un guasto.
    fn guasto(esito: EsitoDelControllo, err: AppError) -> Self {
        Self {
            esito,
            quando_ms: Some(adesso_ms()),
            millisecondi: None,
            errore: Some(ErroreIpc::from(err)),
        }
    }

    /// Il database è stato trovato danneggiato: da qui non si scrive più.
    fn danneggiato(&self) -> bool {
        self.esito == EsitoDelControllo::Danneggiato
    }
}

/// Quel che la manutenzione tiene aperto mentre l'applicazione gira.
pub struct StatoManutenzione {
    /// L'esito del controllo, per le Impostazioni.
    ///
    /// Dietro un mutex suo e non in quello della libreria: lo si legge da un
    /// comando che non ha bisogno del database, e chiedere quel lucchetto per
    /// sapere com'è andato un controllo vorrebbe dire mettersi in coda dietro
    /// una scansione per leggere tre campi.
    salute: Mutex<Salute>,
}

impl StatoManutenzione {
    /// La salute com'è adesso.
    fn salute(&self) -> Salute {
        self.salute
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Scrive la salute nuova.
    fn scrivi_salute(&self, salute: Salute) {
        *self
            .salute
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = salute;
    }
}

/// Registra lo stato e avvia il filo. Va chiamata dopo che la libreria è fra
/// gli stati gestiti.
pub fn avvia(app: &AppHandle) {
    let (sveglia, orecchio) = channel();
    app.manage(StatoManutenzione {
        salute: Mutex::new(Salute::da_fare()),
    });
    // La sveglia la tiene la libreria, e la suona il turno della scansione
    // quando cade. Senza libreria non c'è niente da scansionare, e il filo
    // parte lo stesso: il battito periodico non ha bisogno di sveglie.
    if let Some(stato) = app.try_state::<Stato>() {
        stato.a_fine_scansione(sveglia);
    }
    avvia_filo(app.clone(), orecchio);
}

/// Quel che va fatto all'uscita: l'ultimo `PRAGMA optimize`.
///
/// Una riga nella sequenza di spegnimento di `main`, e `try_state` perché lì
/// si sta smontando tutto. Il resto — il lucchetto a scadenza, l'ottimizzazione
/// «una volta sola» e il giornale svuotato a ogni chiamata — sta in
/// [`crate::stato::ottimizza_uscendo`].
pub fn uscendo(app: &AppHandle) {
    if let Some(stato) = app.try_state::<Stato>() {
        crate::stato::ottimizza_uscendo(&stato);
    }
}

// ── il filo ─────────────────────────────────────────────────────────────────

/// Avvia il filo della manutenzione. Il ciclo è quello di
/// [`crate::stato::avvia_filo_periodico`], comune agli altri tre.
fn avvia_filo(app: AppHandle, orecchio: Receiver<()>) {
    let avviato = crate::stato::avvia_filo_periodico(
        "rymembra-manutenzione",
        orecchio,
        ATTESA_AVVIO,
        RAFFICA,
        INTERVALLO,
        // Ogni sveglia viene da una scansione: si aspetta la raffica.
        |_| true,
        {
            let mut controllato = false;
            move || passata(&app, &mut controllato)
        },
    );
    if let Err(err) = avviato {
        nota!("[avvio] il filo della manutenzione non è partito: {err}");
    }
}

/// Una passata: il controllo (una volta), l'ottimizzazione, la pulizia.
///
/// Non fallisce mai rumorosamente: ogni guasto è una riga di diario, e quello
/// che conta per l'utente — un database danneggiato — anche un evento.
fn passata(app: &AppHandle, controllato: &mut bool) {
    if crate::spegnimento::in_uscita() {
        return;
    }
    let (Some(stato), Some(manutenzione)) = (
        app.try_state::<Stato>(),
        app.try_state::<StatoManutenzione>(),
    ) else {
        return;
    };

    passata_su(
        controllato,
        &manutenzione,
        || controlla(&stato),
        // Sempre, anche quando è sano: le Impostazioni aperte in quel momento
        // mostrano «non ancora controllato» finché qualcuno non glielo dice.
        |salute| app.emetti("manutenzione:salute", salute.clone()),
        || scrivi(app, &stato),
    );
}

/// Il controllo, una volta per sessione, e poi le scritture — solo se il
/// database non è stato trovato danneggiato.
///
/// Fuori da [`passata`] perché è l'unica guardia fra un file malformato e
/// `PRAGMA optimize` o la pulizia delle copertine (vedi il `//!`), e una
/// guardia così si prova: senza un'applicazione attorno, con il controllo, il
/// suo annuncio e le scritture passati da fuori.
fn passata_su(
    controllato: &mut bool,
    manutenzione: &StatoManutenzione,
    controlla: impl FnOnce() -> Salute,
    annuncia: impl FnOnce(&Salute),
    scrivi: impl FnOnce(),
) {
    if !*controllato {
        *controllato = true;
        let salute = controlla();
        manutenzione.scrivi_salute(salute.clone());
        annuncia(&salute);
    }
    if manutenzione.salute().danneggiato() {
        // Vedi il `//!`: su un file malformato non si scrive.
        return;
    }
    scrivi();
}

/// Le scritture di una passata: l'ottimizzazione e, se ci sono orfane, la
/// pulizia delle copertine.
fn scrivi(app: &AppHandle, stato: &Stato) {
    if let Err(err) = con_libreria(stato, |libreria| db::ottimizza(&libreria.connection)) {
        nota!(
            "[manutenzione] ottimizzazione non riuscita codice={} causa={}",
            err.code().kind().code(),
            err.cause().unwrap_or("—")
        );
    }

    match con_libreria(stato, |libreria| {
        covers::quante_orfane(&libreria.connection)
    }) {
        Ok(0) => {}
        Ok(_) => match pulisci(app, stato) {
            Ok(pulizia) => nota!(
                "[manutenzione] copertine pulite righe={} file={} byte={}",
                pulizia.righe,
                pulizia.liberato.file,
                pulizia.liberato.byte
            ),
            // Una scansione o un arricchimento in corso: la pulizia tocca alla
            // prossima sveglia, e la scansione, se toglie qualcosa, la manda lei.
            Err(err)
                if matches!(
                    err.code(),
                    ErrorCode::LibraryScanBusy | ErrorCode::MetadataEnrichBusy
                ) => {}
            Err(err) => nota!(
                "[manutenzione] pulizia delle copertine non riuscita codice={} causa={}",
                err.code().kind().code(),
                err.cause().unwrap_or("—")
            ),
        },
        Err(err) => nota!(
            "[manutenzione] conto delle copertine orfane non riuscito codice={} causa={}",
            err.code().kind().code(),
            err.cause().unwrap_or("—")
        ),
    }
}

/// Il controllo rapido del database, con la sua riga di diario.
///
/// Solo con il giornale in WAL: con il ripiego su `TRUNCATE` un lettore lungo
/// blocca il commit di chiunque scriva, e dopo cinque secondi di attesa ogni
/// cuoricino risponderebbe «occupato». Lì il controllo si salta, e si dice.
fn controlla(stato: &Stato) -> Salute {
    let giornale = con_libreria(stato, |libreria| Ok(libreria.giornale.clone()));
    let Some(cartella) = stato.cartella_dati() else {
        return Salute::fatto(EsitoDelControllo::Saltato, None);
    };
    match giornale {
        Ok(giornale) if giornale == "wal" => {}
        Ok(giornale) => {
            nota!("[manutenzione] controllo del database saltato: giornale={giornale}");
            return Salute::fatto(EsitoDelControllo::Saltato, None);
        }
        Err(err) => return Salute::guasto(EsitoDelControllo::NonRiuscito, err),
    }
    let salute = salute_da(db::controlla_il_file(
        &cartella.join(NOME_DATABASE),
        TEMPO_DEL_CONTROLLO,
    ));
    match (&salute.esito, &salute.millisecondi, &salute.errore) {
        (EsitoDelControllo::Sano, Some(millisecondi), _) => {
            nota!("[manutenzione] controllo del database: sano in {millisecondi} ms");
        }
        (EsitoDelControllo::Interrotto, Some(millisecondi), _) => {
            nota!("[manutenzione] controllo del database interrotto dopo {millisecondi} ms");
        }
        (esito, _, Some(err)) => nota!(
            "[manutenzione] controllo del database: {} codice={} causa={}",
            esito.parola(),
            err.code,
            err.cause.as_deref().unwrap_or("—")
        ),
        _ => {}
    }
    salute
}

/// La salute da quel che `controlla_il_file` ha risposto.
///
/// Pura, perché è metà della guardia del `//!`: `db.corrupt` — e solo lui —
/// vuol dire [`EsitoDelControllo::Danneggiato`], cioè niente più scritture. Ogni altro
/// guasto del controllo è [`EsitoDelControllo::NonRiuscito`], che le scritture non le
/// ferma: un controllo che non ha potuto leggere non ha trovato un danno.
fn salute_da(esito: Result<Controllo, AppError>) -> Salute {
    match esito {
        Ok(Controllo::Sano { millisecondi }) => {
            Salute::fatto(EsitoDelControllo::Sano, Some(millisecondi))
        }
        Ok(Controllo::Interrotto { millisecondi }) => {
            Salute::fatto(EsitoDelControllo::Interrotto, Some(millisecondi))
        }
        Err(err) if matches!(err.code(), ErrorCode::DbCorrupt { .. }) => {
            Salute::guasto(EsitoDelControllo::Danneggiato, err)
        }
        Err(err) => Salute::guasto(EsitoDelControllo::NonRiuscito, err),
    }
}

/// Com'è andata una pulizia.
#[derive(Debug, Clone, Copy, Default)]
struct Pulizia {
    /// Righe di `cover_art` tolte.
    righe: usize,
    /// File e byte tolti dal disco.
    liberato: Liberato,
}

/// La pulizia delle copertine: le righe col lucchetto, i file senza.
///
/// # Il turno della scansione
///
/// La pulizia prende il turno della scansione — dalla porta che non sveglia
/// nessuno, [`Stato::turno_di_manutenzione`] — e non per prudenza generica: la
/// scansione salva le copertine sul disco **fuori** dal lucchetto e le registra
/// dopo, e una pulizia che passasse in mezzo toglierebbe il file di una
/// copertina che sta per essere registrata — «già presente» quando la
/// scansione l'ha guardata, sparita quando la riga arriva. Con il turno, le due
/// cose non si incontrano mai.
///
/// L'arricchimento ha la stessa forma — salva fuori dal lucchetto, registra un
/// gruppo d'album dopo — e prende anche il **suo** turno, per la stessa
/// ragione: una copertina scaricata uguale a un'orfana trova il file già lì,
/// la pulizia lo toglie, e la riga arriva a nominare un file che non c'è più.
/// Il costo è piccolo nei due sensi: dopo una scansione la pulizia parte dopo
/// dieci secondi e l'arricchimento dopo due minuti, e chi dei due trova il
/// turno preso rimanda alla sveglia dopo.
///
/// # Errori
///
/// `library.scanBusy` se una scansione è in corso, `metadata.enrichBusy` se è
/// in corso una passata di arricchimento; `db.*` se il database non risponde, e
/// allora non si tocca nessun file.
fn pulisci(app: &AppHandle, stato: &Stato) -> Result<Pulizia, AppError> {
    let Some(_turno) = stato.turno_di_manutenzione() else {
        return Err(AppError::new(ErrorCode::LibraryScanBusy));
    };
    let arricchimento = app.try_state::<StatoArricchimento>();
    let _passata = match &arricchimento {
        Some(arricchimento) => Some(
            arricchimento
                .turno()
                .ok_or_else(|| AppError::new(ErrorCode::MetadataEnrichBusy))?,
        ),
        None => None,
    };
    let store = stato.copertine()?;
    let (tolte, registrate) = con_libreria(stato, |libreria| {
        let tolte = covers::dimentica_orfane(&libreria.connection)?;
        let registrate = covers::registrate(&libreria.connection)?;
        Ok((tolte, registrate))
    })?;
    // Da qui in giù senza lucchetto: sono migliaia di cancellazioni di file, e
    // la finestra deve poter leggere la libreria mentre avvengono.
    let liberato = store
        .togli_tutte(&tolte)
        .piu(store.spazza_avanzi(&registrate, ETA_DEGLI_AVANZI));
    Ok(Pulizia {
        righe: tolte.len(),
        liberato,
    })
}

// ── i comandi ───────────────────────────────────────────────────────────────

/// Quel che le Impostazioni mostrano della manutenzione.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoManutenzioneIpc {
    /// Quanti file conta la cache delle copertine.
    pub file: u64,
    /// Quanti byte occupa.
    pub byte: u64,
    /// Quante copertine nessuno nomina più: quel che «Pulisci» toglierebbe,
    /// avanzi a parte.
    pub orfane: u64,
    /// Com'è andato il controllo del database.
    pub salute: Salute,
}

/// Quel che la pulizia a mano restituisce.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EsitoPuliziaIpc {
    /// Righe di `cover_art` tolte.
    pub righe: usize,
    /// File tolti dal disco, copertine, miniature e avanzi.
    pub file: u64,
    /// Byte liberati.
    pub byte: u64,
    /// Lo stato dopo, per non doverlo richiedere.
    pub stato: StatoManutenzioneIpc,
}

/// Lo stato della manutenzione: cache delle copertine e salute del database.
///
/// In disparte: misurare la cache vuol dire leggere fino a 256 cartelle, e
/// contare le orfane un'unione su tutti i brani — qualche decina di
/// millisecondi ciascuno su una libreria grande, che sul filo della finestra
/// sarebbero fotogrammi persi.
///
/// # Errori
///
/// Quel che riporta il nucleo: `db.*` se il conto delle orfane non riesce.
#[tauri::command]
pub async fn manutenzione_stato(app: AppHandle) -> Esito<StatoManutenzioneIpc> {
    in_disparte("stato della manutenzione", move || stato_ipc(&app))
        .await
        .map_err(errore)?
        .map_err(errore)
}

/// Com'è andato il controllo del database in questa sessione, e nient'altro.
///
/// La metà di [`manutenzione_stato`] che serve **sempre**: la finestra la chiede
/// all'apertura per l'avviso di database danneggiato, che deve comparire anche
/// in una finestra nata dopo `manutenzione:salute` — ricaricata a sessione
/// avviata, quando l'evento è già passato e non torna. `manutenzione_stato`
/// misurerebbe anche la cache e conterebbe le orfane, cioè centinaia di
/// cartelle lette e un'unione su tutti i brani per una risposta che sta in un
/// `Mutex`; e lo farebbe all'apertura, quando il disco ha già altro da fare.
///
/// Sincrono: non legge né il disco né la libreria.
#[tauri::command]
pub fn manutenzione_salute(app: AppHandle) -> Salute {
    app.try_state::<StatoManutenzione>()
        .map_or_else(Salute::da_fare, |manutenzione| manutenzione.salute())
}

/// Apre nel gestore file la cartella dei dati: quella della libreria, **anche
/// quando la libreria non si è aperta**.
///
/// # Perché esiste
///
/// Perché è il gesto che serve quando il database non si apre — una
/// migrazione fermata a metà, un database scritto da una versione più nuova, un
/// file rotto — e la fascia d'errore lo offre accanto alla frase. Lì dentro
/// c'è `rymembra.db`, e accanto la copia di prima dell'aggiornamento
/// (`rymembra.db.schema-NNN.bak`); e `%APPDATA%` in Esplora risorse è nascosta
/// di serie, quindi un percorso scritto in una frase non porta nessuno da
/// nessuna parte. È la stessa ragione di `diario_apri`.
///
/// Sul telefono la cartella dei dati è privata dell'app, e nessun gestore file
/// ci entra: la finestra lì non offre il gesto, e il comando lo rifiuta.
///
/// # Errori
///
/// `internal.aborted` se la cartella non c'è da aprire — dopo un rifiuto del
/// trasloco, o sul telefono — o se il gestore file rifiuta di aprirsi.
#[tauri::command]
pub async fn manutenzione_apri_cartella_dati(app: AppHandle) -> Esito<()> {
    // Chiede al sistema di aprire una cartella — un processo — e la traduce se
    // sta dentro il pacchetto del Store: aspetta.
    comanda("apertura della cartella dei dati", move || {
        let guasto = |causa: String| {
            errore(
                AppError::new(ErrorCode::InternalAborted {
                    what: Some("apertura della cartella dei dati".to_owned()),
                })
                .with_cause(causa),
            )
        };
        let Some(dove) = app.try_state::<Stato>().and_then(|stato| {
            stato
                .cartella_della_libreria()
                .map(std::path::Path::to_path_buf)
        }) else {
            return Err(guasto("nessuna cartella dei dati scelta".to_owned()));
        };
        #[cfg(target_os = "android")]
        {
            let _ = dove;
            Err(guasto(
                "sul telefono la cartella dei dati è privata".to_owned(),
            ))
        }
        #[cfg(not(target_os = "android"))]
        {
            // Esplora risorse gira fuori dal pacchetto: nella copia del Store la
            // cartella dei dati sta nella cartella privata del pacchetto, e il
            // percorso visto da dentro lì non c'è (R20-4). Vedi `pacchetto`.
            let dove = crate::pacchetto::per_esplora(&dove);
            tauri_plugin_opener::open_path(&dove, None::<&str>)
                .map_err(|err| guasto(err.to_string()))
        }
    })
    .await
}

/// La copia di prima dell'aggiornamento che «Torna alla copia» rimetterebbe.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopiaDiPrimaIpc {
    /// Il nome del file, accanto al database: `rymembra.db.schema-038.bak`.
    pub nome: String,
    /// Lo schema che contiene.
    pub versione: u32,
    /// Quando è stata fatta, in millisecondi dall'epoca.
    pub quando_ms: i64,
    /// Lo schema di questa versione. Una copia più vecchia vuole, dopo il
    /// ritorno, la versione di Rymembra di prima: questa la riaggiornerebbe.
    pub versione_di_questa: u32,
    /// Il nome che prenderà la libreria di adesso, messa da parte.
    pub messa_da_parte: String,
}

/// Il nome di un file, per la finestra.
fn nome_del_file(percorso: &std::path::Path) -> String {
    percorso
        .file_name()
        .map(|nome| nome.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// La copia di prima dell'aggiornamento da offrire, se ce n'è una.
///
/// Non apre il database, ed è il punto: la scheda la chiede anche quando la
/// libreria non si è aperta, che è il caso in cui il gesto serve di più. Legge
/// soltanto la cartella dei dati.
///
/// Sul telefono niente: Android non installa una versione più vecchia senza
/// disinstallare, e disinstallare porta via libreria e copia insieme.
#[tauri::command]
pub async fn manutenzione_copia_di_prima(app: AppHandle) -> Option<CopiaDiPrimaIpc> {
    if cfg!(mobile) {
        return None;
    }
    // Legge la cartella dei dati, che può stare su una share: sul pool
    // bloccante. Se il lavoro cade, la risposta è quella di «non c'è»: la
    // scheda non offre il ritorno, e il panico l'ha già scritto il diario.
    in_disparte("copia di prima dell'aggiornamento", move || {
        let stato = app.try_state::<Stato>()?;
        let percorso = stato.cartella_della_libreria()?.join(NOME_DATABASE);
        let copia = db::copia_di_prima(&percorso)?;
        Some(CopiaDiPrimaIpc {
            nome: nome_del_file(&copia.percorso),
            versione: copia.versione,
            quando_ms: copia.quando_ms,
            versione_di_questa: db::LATEST_VERSION,
            messa_da_parte: nome_del_file(&db::nome_messa_da_parte(&percorso)),
        })
    })
    .await
    .ok()
    .flatten()
}

/// «Torna alla copia di prima dell'aggiornamento» (D-55): rimette la copia,
/// mette da parte la libreria di adesso, e chiude Rymembra.
///
/// # Perché si chiude
///
/// Perché la copia rimessa è quasi sempre di uno schema più vecchio, e questa
/// versione, se la riaprisse, la riaggiornerebbe subito: il ritorno ha senso
/// solo con la versione di prima, da installare a programma chiuso. Il
/// dialogo prima del gesto lo dice. Chiudere è la sequenza di sempre, quella
/// di «Esci» dal vassoio: la libreria a quel punto è già chiusa
/// ([`crate::stato::torna_alla_copia_di_prima`]), e i passi che la vogliono lo
/// annotano e vanno avanti.
///
/// # Errori
///
/// Quelli del ritorno, e allora niente è cambiato e Rymembra resta aperto;
/// `internal.aborted` sul telefono.
#[tauri::command]
pub async fn manutenzione_torna_alla_copia(app: AppHandle) -> Esito<()> {
    if cfg!(mobile) {
        return Err(errore(
            AppError::new(ErrorCode::InternalAborted {
                what: Some("ritorno alla copia di prima dell'aggiornamento".to_owned()),
            })
            .with_cause("non sul telefono"),
        ));
    }
    in_disparte("ritorno alla copia di prima", move || {
        let Some(stato) = app.try_state::<Stato>() else {
            return Err(fuori_servizio());
        };
        match crate::stato::torna_alla_copia_di_prima(&stato) {
            Ok(ritorno) => {
                nota!(
                    "[manutenzione] rimessa la copia di prima dell'aggiornamento {} (schema {}); la libreria di prima è in {}",
                    ritorno.copia.percorso.display(),
                    ritorno.copia.versione,
                    ritorno.messa_da_parte.display()
                );
                // La copia usata si rinomina fuori dal giro delle copie: così il
                // prossimo aggiornamento ne fa una nuova (S3-5). Se non è
                // riuscita il ritorno è fatto lo stesso, ma al prossimo
                // aggiornamento la copia vecchia risulterebbe «già presente», e
                // il diario lo dice.
                match &ritorno.copia_consumata {
                    Ok(dove) => nota!(
                        "[manutenzione] la copia usata non è più una copia di prima dell'aggiornamento: ora è {}",
                        dove.display()
                    ),
                    Err(perche) => nota!(
                        "[manutenzione] la copia usata NON si è rinominata ({perche}): al prossimo aggiornamento risulterà già presente e non se ne farà una nuova"
                    ),
                }
                crate::spegnimento::chiedi();
                app.exit(0);
                Ok(())
            }
            Err(err) => {
                nota!(
                    "[manutenzione] ritorno alla copia di prima non riuscito codice={} causa={}",
                    err.code().kind().code(),
                    err.cause().unwrap_or("—")
                );
                Err(err)
            }
        }
    })
    .await
    .map_err(errore)?
    .map_err(errore)
}

// ── la libreria di prima, per la copia del Microsoft Store ─────────────────

/// Dove si ricorda che la proposta è stata fatta: `copiata` o `rifiutata`.
/// Si propone una volta sola.
const CHIAVE_LIBRERIA_DI_PRIMA: &str = "store.libreria_di_prima";

/// Quel che la proposta mostra.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibreriaDiPrimaIpc {
    /// Dove sta la libreria da prendere.
    pub cartella: String,
}

/// La libreria che la copia del Microsoft Store può prendere, se va proposta
/// adesso (D-97).
///
/// Dalla 3.2.1 la copia del Store tiene una libreria sua (vedi
/// `cartella_di_serie` in `lib.rs`). Quella che c'era — della copia installata
/// dal sito, o sua prima dell'aggiornamento — si propone al primo avvio, una
/// volta sola, e solo se la libreria di qui è ancora vuota: niente brani e
/// niente playlist. Una libreria che qualcuno ha già cominciato a riempire non
/// si sostituisce con una domanda.
///
/// # Errori
///
/// `db.*` se la libreria non risponde. Fuori dal Store, e senza una libreria
/// da prendere, `None`.
#[tauri::command]
pub async fn store_libreria_di_prima(app: AppHandle) -> Esito<Option<LibreriaDiPrimaIpc>> {
    let Some(cartella) = app
        .try_state::<crate::LibreriaDiPrima>()
        .and_then(|libreria| libreria.0.clone())
    else {
        return Ok(None);
    };
    in_disparte("libreria di prima", move || {
        let Some(stato) = app.try_state::<Stato>() else {
            return Ok(None);
        };
        let da_proporre = con_libreria(&stato, |libreria| {
            let chiesta =
                rymembra_app::settings::read(&libreria.connection, CHIAVE_LIBRERIA_DI_PRIMA)?
                    .is_some();
            let vuota: bool = libreria
                .connection
                .query_row(
                    "SELECT NOT EXISTS (SELECT 1 FROM tracks)
                        AND NOT EXISTS (SELECT 1 FROM playlists)",
                    [],
                    |riga| riga.get(0),
                )
                .map_err(|err| db::codice_da_sqlite("la libreria è vuota?", &err))?;
            Ok(!chiesta && vuota)
        })?;
        // Il posto si **mostra**: dove Esplora lo trova, che nella copia del
        // Store di prima dell'aggiornamento è la cartella privata del pacchetto
        // — ma solo se il database è lì: con il solo giornale accanto a un
        // database rimasto fuori, la libreria sta fuori.
        Ok(da_proporre.then(|| LibreriaDiPrimaIpc {
            cartella: crate::pacchetto::per_esplora_se_contiene(&cartella, NOME_DATABASE)
                .display()
                .to_string(),
        }))
    })
    .await
    .map_err(errore)?
    .map_err(errore)
}

/// Prende la [`LibreriaDiPrimaIpc`] proposta: la copia nella libreria di qui
/// (`trasloco::copia_la_libreria_di`, l'originale non si tocca) e chiude
/// Rymembra, che riaperto la trova al suo posto.
///
/// Si chiude perché quel che è partito all'avvio — le cartelle sorvegliate,
/// lo specchio del cloud, la coda — viene dalla libreria vuota di prima.
///
/// # Errori
///
/// `library.scanBusy` durante una scansione; quelli della copia, e allora la
/// libreria di qui è com'era e Rymembra resta aperto.
#[tauri::command]
pub async fn store_copia_libreria_di_prima(app: AppHandle) -> Esito<()> {
    let Some(da) = app
        .try_state::<crate::LibreriaDiPrima>()
        .and_then(|libreria| libreria.0.clone())
    else {
        return Err(errore(fuori_servizio()));
    };
    in_disparte("copia della libreria di prima", move || {
        let Some(stato) = app.try_state::<Stato>() else {
            return Err(fuori_servizio());
        };
        let Some(a) = stato.cartella_dati().map(std::path::Path::to_path_buf) else {
            return Err(fuori_servizio());
        };
        let Some(_turno) = stato.turno_di_manutenzione() else {
            return Err(AppError::new(ErrorCode::LibraryScanBusy));
        };
        let esito = con_libreria(&stato, |libreria| {
            let copiata =
                rymembra_app::trasloco::copia_la_libreria_di(&mut libreria.connection, &da, &a)?;
            rymembra_app::settings::write(
                &libreria.connection,
                CHIAVE_LIBRERIA_DI_PRIMA,
                "copiata",
            )?;
            Ok(copiata)
        });
        match esito {
            Ok(copiata) => {
                nota!(
                    "[manutenzione] libreria di prima copiata da {} file={} gia_presenti={}",
                    da.display(),
                    copiata.file,
                    copiata.gia_presenti
                );
                crate::spegnimento::chiedi();
                app.exit(0);
                Ok(())
            }
            Err(err) => {
                nota!(
                    "[manutenzione] copia della libreria di prima non riuscita codice={} causa={}",
                    err.code().kind().code(),
                    err.cause().unwrap_or("—")
                );
                Err(err)
            }
        }
    })
    .await
    .map_err(errore)?
    .map_err(errore)
}

/// «Comincia da capo»: la proposta non si rifà.
///
/// # Errori
///
/// `db.*` se la libreria non risponde.
#[tauri::command]
pub async fn store_rifiuta_libreria_di_prima(app: AppHandle) -> Esito<()> {
    in_disparte("libreria di prima rifiutata", move || {
        let Some(stato) = app.try_state::<Stato>() else {
            return Err(fuori_servizio());
        };
        con_libreria(&stato, |libreria| {
            rymembra_app::settings::write(
                &libreria.connection,
                CHIAVE_LIBRERIA_DI_PRIMA,
                "rifiutata",
            )
        })
    })
    .await
    .map_err(errore)?
    .map_err(errore)
}

/// Pulisce la cache delle copertine adesso, e dice quanto ha liberato.
///
/// # Errori
///
/// `library.scanBusy` se una scansione è in corso e `metadata.enrichBusy` se lo
/// è una passata di arricchimento — la pulizia le aspetta, e riprovare dopo
/// basta. `db.corrupt` se il controllo di questa sessione ha
/// trovato il database danneggiato: su un file malformato non si scrive.
#[tauri::command]
pub async fn manutenzione_pulisci_copertine(app: AppHandle) -> Esito<EsitoPuliziaIpc> {
    in_disparte("pulizia delle copertine", move || {
        let Some(stato) = app.try_state::<Stato>() else {
            return Err(fuori_servizio());
        };
        if let Some(manutenzione) = app.try_state::<StatoManutenzione>() {
            let salute = manutenzione.salute();
            if salute.danneggiato() {
                return Err(AppError::new(ErrorCode::DbCorrupt {
                    path: stato
                        .cartella_dati()
                        .map(|cartella| cartella.join(NOME_DATABASE).display().to_string()),
                    quarantined_as: None,
                })
                .with_cause("pulizia rifiutata: il controllo ha trovato il database danneggiato"));
            }
        }
        let pulizia = pulisci(&app, &stato)?;
        nota!(
            "[manutenzione] pulizia a mano righe={} file={} byte={}",
            pulizia.righe,
            pulizia.liberato.file,
            pulizia.liberato.byte
        );
        Ok(EsitoPuliziaIpc {
            righe: pulizia.righe,
            file: pulizia.liberato.file,
            byte: pulizia.liberato.byte,
            stato: stato_ipc(&app)?,
        })
    })
    .await
    .map_err(errore)?
    .map_err(errore)
}

/// Lo stato per la finestra, letto adesso.
fn stato_ipc(app: &AppHandle) -> Result<StatoManutenzioneIpc, AppError> {
    let Some(stato) = app.try_state::<Stato>() else {
        return Err(fuori_servizio());
    };
    let Occupazione { file, byte } = stato.copertine()?.occupazione();
    let orfane = con_libreria(&stato, |libreria| {
        covers::quante_orfane(&libreria.connection)
    })?;
    let salute = app
        .try_state::<StatoManutenzione>()
        .map_or_else(Salute::da_fare, |manutenzione| manutenzione.salute());
    Ok(StatoManutenzioneIpc {
        file,
        byte,
        orfane,
        salute,
    })
}

/// La libreria non è più fra gli stati gestiti: l'applicazione sta uscendo.
fn fuori_servizio() -> AppError {
    AppError::new(ErrorCode::InternalAborted {
        what: Some("manutenzione".to_owned()),
    })
    .with_cause("la libreria non è più fra gli stati gestiti")
}

#[cfg(test)]
mod prove {
    use super::*;
    use std::cell::Cell;

    fn corrotto() -> AppError {
        AppError::new(ErrorCode::DbCorrupt {
            path: None,
            quarantined_as: None,
        })
    }

    #[test]
    fn un_db_corrupt_e_danneggiato_e_gli_altri_guasti_no() {
        assert!(salute_da(Err(corrotto())).danneggiato());
        assert!(
            !salute_da(Err(AppError::new(ErrorCode::DbQueryFailed {
                detail: None
            })))
            .danneggiato()
        );
        assert!(
            !salute_da(Ok(Controllo::Interrotto {
                millisecondi: 30_000
            }))
            .danneggiato()
        );
        assert!(!salute_da(Ok(Controllo::Sano { millisecondi: 12 })).danneggiato());
        // Sul filo le parole di sempre, quelle dell'unione in `ipc/servizi.ts`.
        let scritto = serde_json::to_value(salute_da(Err(corrotto()))).expect("serializzata");
        assert_eq!(scritto["esito"], "danneggiato");
        let scritto = serde_json::to_value(Salute::da_fare()).expect("serializzata");
        assert_eq!(scritto["esito"], "daFare");
        let scritto =
            serde_json::to_value(Salute::guasto(EsitoDelControllo::NonRiuscito, corrotto()))
                .expect("ok");
        assert_eq!(scritto["esito"], "nonRiuscito");
        // E il diario scrive la stessa parola del filo.
        for esito in [
            EsitoDelControllo::DaFare,
            EsitoDelControllo::Sano,
            EsitoDelControllo::Interrotto,
            EsitoDelControllo::Saltato,
            EsitoDelControllo::Danneggiato,
            EsitoDelControllo::NonRiuscito,
        ] {
            assert_eq!(
                serde_json::to_value(esito).expect("serializzato"),
                esito.parola()
            );
        }
    }

    #[test]
    fn su_un_database_danneggiato_la_passata_non_scrive() {
        let manutenzione = StatoManutenzione {
            salute: Mutex::new(Salute::da_fare()),
        };
        let mut controllato = false;
        let scritto = Cell::new(false);
        let annunciato = Cell::new(false);
        passata_su(
            &mut controllato,
            &manutenzione,
            || salute_da(Err(corrotto())),
            |_| annunciato.set(true),
            || scritto.set(true),
        );
        assert!(annunciato.get(), "l'esito del controllo non è stato detto");
        assert!(!scritto.get(), "su un file malformato si è scritto");
        // Il controllo è uno per sessione: alla passata dopo non si rifà, e la
        // guardia tiene lo stesso.
        passata_su(
            &mut controllato,
            &manutenzione,
            || panic!("il controllo si è rifatto"),
            |_| {},
            || scritto.set(true),
        );
        assert!(!scritto.get(), "alla seconda passata si è scritto");

        // Su un database sano, invece, si scrive.
        let manutenzione = StatoManutenzione {
            salute: Mutex::new(Salute::da_fare()),
        };
        let mut controllato = false;
        passata_su(
            &mut controllato,
            &manutenzione,
            || salute_da(Ok(Controllo::Sano { millisecondi: 1 })),
            |_| {},
            || scritto.set(true),
        );
        assert!(scritto.get());
    }
}
