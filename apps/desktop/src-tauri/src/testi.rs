//! I testi, dal lato della finestra.
//!
//! Due comandi e una forma da serializzare. Le decisioni stanno in
//! `aether_domain::testo`, la catena delle fonti in `aether_app::testi`: qui si
//! prende il lucchetto della libreria per il tempo di una lettura, e si traduce
//! quel che ne esce in qualcosa che attraversi l'IPC.
//!
//! # Perché una forma a parte e non `Testo` così com'è
//!
//! Perché `aether-domain` non conosce serde — è la sua regola, e la ragione è
//! che il dominio non deve avere un'opinione su come lo si trasmette. La stessa
//! indirezione di [`crate::errore::ErroreIpc`], per lo stesso motivo.
//!
//! # L'LRC non arriva mai alla finestra
//!
//! Quel che passa di qui sono righe già interpretate e già in ordine. La
//! finestra non sa cosa sia un timestamp, non sa cosa sia `[offset:]`, e non ha
//! nessuna espressione regolare: se ne avesse una, sarebbe il secondo lettore
//! dello stesso formato, e i due divergerebbero su quel che il formato non dice
//! — che è quasi tutto.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::spegnimento::Emette as _;
use aether_app::enrich::DepositoSqlite;
use aether_app::settings;
use aether_app::testi::{self, Copertura, Fonte};
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::testo::{Aderenza, Parola, Riga, Testo, parole_combaciano};
use aether_meta::Fornitori;
use serde::Serialize;
use tauri::{AppHandle, Manager as _, State};

use crate::disparte::in_disparte;
use crate::errore::{Esito, errore};
use crate::nota;
use crate::stato::{NOME_DATABASE, Stato, Turno, adesso_ms, con_libreria};

/// La chiave in `settings` per l'interruttore della rete.
const CHIAVE_RETE: &str = "testi.rete";

/// Una parola con il suo tempo, sul filo.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParolaIpc {
    /// Quando comincia, in millisecondi.
    pub ms: u32,
    /// Il pezzo di riga che le appartiene, spazi compresi.
    pub testo: String,
}

/// Una riga di testo con il suo tempo, sul filo.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RigaIpc {
    /// Quando comincia, in millisecondi.
    pub ms: u32,
    /// La riga.
    pub testo: String,
    /// I tempi delle parole, quando il file li porta. Quasi sempre vuoto.
    pub parole: Vec<ParolaIpc>,
}

/// Il testo di un brano, nella forma che la finestra riceve.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestoIpc {
    /// Le righe con i tempi, in ordine. Vuoto se il testo non è sincronizzato.
    pub righe: Vec<RigaIpc>,
    /// Il testo senza tempi, da mostrare quando le righe non ci sono.
    pub piatto: Option<String>,
    /// Il brano non ha parole.
    pub strumentale: bool,
    /// Da dove viene: `nessuna`, `sidecar`, `tag`, `lrclib`, `mano`.
    pub fonte: Fonte,
    /// Lo scarto che il file dichiara, nel verso dello standard.
    ///
    /// Arriva separato da [`scarto_ms`](Self::scarto_ms) e non già sommato,
    /// perché i due si correggono da due posti diversi: questo è di chi ha
    /// scritto il file, l'altro è di chi sta ascoltando. La finestra li somma
    /// alla posizione — è quel che fa `aether_domain::testo::posizione_corretta`
    /// — e ne mostra uno solo.
    pub offset_ms: i32,
    /// La correzione di chi ascolta.
    pub scarto_ms: i32,
    /// `buona`, `sospetta` o `fuori`.
    pub aderenza: &'static str,
    /// Si è già chiesto al catalogo per questo brano.
    pub cercato: bool,
    /// Vale la pena chiedere al catalogo: quel che si ha non scorre.
    ///
    /// La finestra non ricalcola la condizione — guarda questo. Vedi
    /// `aether_app::testi::TestoBrano::da_chiedere` per la regola e per il
    /// difetto che c'era prima che fosse una regola sola.
    pub da_chiedere: bool,
}

/// Il nome stabile dell'aderenza, per la finestra.
const fn nome_aderenza(aderenza: Aderenza) -> &'static str {
    match aderenza {
        Aderenza::Buona => "buona",
        Aderenza::Sospetta => "sospetta",
        Aderenza::Fuori => "fuori",
    }
}

impl From<testi::TestoBrano> for TestoIpc {
    fn from(trovato: testi::TestoBrano) -> Self {
        Self {
            righe: trovato
                .testo
                .righe
                .into_iter()
                .map(|riga| RigaIpc {
                    ms: riga.ms,
                    testo: riga.testo,
                    parole: riga
                        .parole
                        .into_iter()
                        .map(|p| ParolaIpc {
                            ms: p.ms,
                            testo: p.testo,
                        })
                        .collect(),
                })
                .collect(),
            piatto: trovato.testo.piatto,
            strumentale: trovato.testo.strumentale,
            fonte: trovato.fonte,
            offset_ms: trovato.testo.offset_ms,
            scarto_ms: trovato.scarto_ms,
            aderenza: nome_aderenza(trovato.aderenza),
            cercato: trovato.cercato,
            da_chiedere: trovato.da_chiedere,
        }
    }
}

/// Il testo di un brano, da quel che si ha già sul disco.
///
/// Non tocca la rete: quel che c'è si mostra subito, e chiedere altrove è un
/// gesto separato. Se al catalogo valga la pena chiederlo lo dice
/// `da_chiedere`, che la finestra legge senza ricostruirsi la regola.
///
/// # Errori
///
/// `library.trackNotFound` se il brano non c'è più, `db.queryFailed` se la
/// lettura fallisce.
///
/// # Dove gira
///
/// Prima del database questa lettura guarda se accanto al file c'è un `.lrc`, e
/// «accanto al file» può voler dire su una cartella di rete. Un comando normale
/// gira sul filo principale, e lì una condivisione che non risponde vale quaranta
/// secondi di finestra ferma per aprire il pannello del testo.
///
/// Passa quindi da [`in_disparte`], cioè dal pool bloccante, e non dal semplice
/// `(async)`: quello manderebbe il corpo su un worker del runtime, che sono tanti
/// quanti i processori, e tre pannelli aperti su una share morta li
/// occuperebbero tutti — fermando ogni altro comando asincrono, anche quelli che
/// il disco non lo toccano.
///
/// Prende un `AppHandle` e non uno `State`: il lavoro va mosso dentro una
/// chiusura `'static`, e un prestito dello stato lì dentro non entra.
#[tauri::command]
pub async fn testo_brano(app: AppHandle, id: i64) -> Esito<TestoIpc> {
    let mano = app.clone();
    in_disparte("testo del brano", move || {
        let Some(stato) = mano.try_state::<Stato>() else {
            return Err(AppError::new(ErrorCode::InternalAborted {
                what: Some("lettura del testo".to_owned()),
            })
            .with_cause("la libreria non è più fra gli stati gestiti"));
        };
        testo_gia_saputo(&stato, id)
    })
    .await
    .map_err(errore)?
    .map_err(errore)
}

/// Quel che si sa già del testo di un brano, senza toccare la rete.
///
/// Separata dal comando perché la chiamano in due: [`testo_brano`], che la manda
/// in disparte, e [`testo_cerca`] quando la rete è spenta — e quella è già su un
/// filo suo, con il brano appena letto sotto lo stesso lucchetto. Due copie della
/// stessa lettura sarebbero due copie che divergono il giorno in cui qualcuno
/// cambia cosa vuol dire «il testo che si ha».
fn testo_gia_saputo(stato: &Stato, id: i64) -> Result<TestoIpc, AppError> {
    con_libreria(stato, |libreria| {
        testi::per_brano(&libreria.connection, id).map(TestoIpc::from)
    })
}

/// Sposta il testo di questo brano avanti o indietro, e se lo ricorda.
///
/// Positivo anticipa, come dice lo standard LRC. Si applica al brano — cioè a
/// `track_key` — e non al file: chi ha lo stesso brano in due formati ha fatto
/// la correzione una volta sola.
///
/// # Errori
///
/// `library.trackNotFound`, `db.queryFailed`.
#[tauri::command(async)]
pub fn testo_scarto(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    id: i64,
    scarto_ms: i32,
) -> Esito<()> {
    let esito = con_libreria(&stato, |libreria| {
        let brano = testi::brano(&libreria.connection, id)?;
        testi::imposta_scarto(&libreria.connection, &brano.track_key, scarto_ms)
    })
    .map_err(errore);
    // Come `preferito` e `valutazione`: è una decisione di chi ascolta, e va
    // dove vanno le altre.
    crate::nuvola::se_riuscito(&app, esito)
}

// ── lo stato che vive quanto l'applicazione ─────────────────────────────────

/// Quel che i testi tengono aperto mentre l'applicazione gira.
///
/// # Perché i fornitori stanno qui
///
/// Per la stessa ragione per cui ci stanno i cataloghi di [`crate::procura`]:
/// dentro c'è la riserva di connessioni di `ureq`, ed è quel che rende una
/// passata di cinquanta richieste **un** saluto TLS invece di cinquanta. Contro
/// un servizio pubblico che ci ospita gratis, la differenza non è la velocità.
///
/// Dietro un `Arc` e non tenuti sotto il lucchetto per tutta la richiesta: chi
/// apre il pannello del testo mentre una passata sta girando non deve aspettare
/// la passata. A mettere in fila le richieste vere ci pensa già la `Cadenza`,
/// che è il posto giusto — là il ritmo è dichiarato, qui sarebbe un effetto
/// collaterale di come si è preso un lucchetto.
pub struct StatoTesti {
    /// I fornitori, creati alla prima richiesta: prima non si sa dove sia il
    /// database, e aprirne il deposito è metà del lavoro.
    fornitori: Mutex<Option<Arc<Fornitori>>>,
    /// Una passata sta girando.
    in_corso: AtomicBool,
    /// Qualcuno ha chiesto di fermarla.
    da_fermare: AtomicBool,
    /// Quanti brani ha guardato questa passata.
    fatti: AtomicU32,
    /// Quanti ne restano, per chi chiede lo stato senza aspettare un evento.
    rimasti: AtomicU32,
    /// Un precaricamento sta girando.
    ///
    /// Uno alla volta, e non per il traffico — a quello pensa la `Cadenza` —
    /// ma per i fili: chi scorre la coda a colpi di «avanti» manda un
    /// `Iniziato` al secondo, e senza questa bandiera sarebbero dieci fili
    /// fermi su dieci richieste che nessuno leggerà.
    precarica: AtomicBool,
    /// L'ultimo brano per cui si è precaricato il testo.
    ///
    /// `Iniziato` arriva a ogni cambio di traccia, e il **successivo** quasi
    /// sempre non cambia da un brano all'altro: senza questa memoria si
    /// rifarebbe la stessa domanda a ogni canzone. Zero vuol dire «nessuno»:
    /// gli identificativi di `tracks` partono da uno.
    precaricato: AtomicI64,
}

impl std::fmt::Debug for StatoTesti {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatoTesti")
            .field("in_corso", &self.in_corso.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl Default for StatoTesti {
    fn default() -> Self {
        Self::nuovo()
    }
}

impl StatoTesti {
    /// Lo stato, prima che qualcuno chieda qualcosa.
    #[must_use]
    pub const fn nuovo() -> Self {
        Self {
            fornitori: Mutex::new(None),
            in_corso: AtomicBool::new(false),
            da_fermare: AtomicBool::new(false),
            fatti: AtomicU32::new(0),
            rimasti: AtomicU32::new(0),
            precarica: AtomicBool::new(false),
            precaricato: AtomicI64::new(0),
        }
    }

    /// Chiede alla passata in corso di fermarsi.
    ///
    /// Non chiede il lucchetto della libreria — e non potrebbe: quel lucchetto
    /// ce l'ha la passata, a intermittenza, per tutta la sua durata. Alza un
    /// bit, e la passata lo legge fra un brano e l'altro.
    pub fn ferma(&self) {
        self.da_fermare.store(true, Ordering::Relaxed);
    }
}

/// I fornitori, aprendoli se è la prima volta.
///
/// # Errori
///
/// `db.openFailed` se il deposito non si apre.
fn servizi(app: &AppHandle, testi: &StatoTesti) -> Result<Arc<Fornitori>, AppError> {
    let mut guardia = testi
        .fornitori
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(gia) = guardia.as_ref() {
        return Ok(Arc::clone(gia));
    }
    let dati = app.path().app_data_dir().map_err(|err| {
        AppError::new(aether_domain::errors::ErrorCode::FsNotFound {
            path: "cartella dati".to_owned(),
        })
        .with_cause(err.to_string())
    })?;
    let deposito = DepositoSqlite::apri(&dati.join(NOME_DATABASE), adesso_ms())?;
    let nuovi = Arc::new(Fornitori::nuovo(Box::new(deposito)));
    *guardia = Some(Arc::clone(&nuovi));
    Ok(nuovi)
}

/// La rete per i testi è accesa.
///
/// Accesa di serie. Chiedere un testo dice al catalogo cosa si sta ascoltando,
/// ed è scritto in `PRIVACY.md`: la scelta di partire accesi si regge su due
/// cose, che la richiesta parte **solo** quando il pannello del testo è aperto
/// — cioè quando qualcuno lo ha chiesto — e che l'interruttore sta nelle
/// Impostazioni accanto agli altri, non in fondo a un menu.
fn rete_attiva(connection: &rusqlite::Connection) -> bool {
    settings::read(connection, CHIAVE_RETE)
        .ok()
        .flatten()
        .is_none_or(|valore| valore != "0")
}

// ── quel che la finestra vede della passata ─────────────────────────────────

/// Lo stato dei testi sulla libreria intera.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoTestiIpc {
    /// Si può chiedere al catalogo.
    pub rete: bool,
    /// Una passata sta girando adesso.
    pub in_corso: bool,
    /// Quanti brani ha guardato la passata in corso.
    pub fatti: u32,
    /// Quanti gliene restano.
    pub rimasti: u32,
    /// I quattro numeri della copertura.
    pub copertura: Copertura,
}

fn stato_adesso(stato: &Stato, testi: &StatoTesti) -> Result<StatoTestiIpc, AppError> {
    // Le due letture sotto lo **stesso** lucchetto: prenderlo due volte
    // lascerebbe passare in mezzo un brano che si conclude, e l'interruttore
    // direbbe una cosa mentre i numeri ne dicono un'altra.
    let (rete, copertura) = con_libreria(stato, |libreria| {
        Ok((
            rete_attiva(&libreria.connection),
            testi::copertura(&libreria.connection)?,
        ))
    })?;
    Ok(StatoTestiIpc {
        rete,
        in_corso: testi.in_corso.load(Ordering::Acquire),
        fatti: testi.fatti.load(Ordering::Relaxed),
        rimasti: testi.rimasti.load(Ordering::Relaxed),
        copertura,
    })
}

// ── i comandi ───────────────────────────────────────────────────────────────

/// Chiede il testo di questo brano al catalogo, adesso.
///
/// # I tre tempi, e perché sono tre
///
/// Si legge la riga del brano sotto il lucchetto, **si lascia il lucchetto**, si
/// va in rete, si riprende il lucchetto per scrivere. Tenerlo per tutta la
/// durata sarebbe una riga in meno e bloccherebbe la riproduzione per il tempo
/// di una richiesta HTTP — che con una rete lenta è la scadenza intera, venti
/// secondi. Che non si possa fare per sbaglio è garantito dal fatto che
/// `aether-meta` non riceve mai una `Connection`.
///
/// # Errori
///
/// `library.trackNotFound`, l'errore di rete quando il catalogo non risponde,
/// `db.queryFailed`.
///
/// `(async)`: la richiesta al catalogo ha una scadenza di venti secondi, e un
/// comando normale la consumerebbe sul filo principale della finestra.
#[tauri::command(async)]
pub fn testo_cerca(
    app: AppHandle,
    stato: State<'_, Stato>,
    testi: State<'_, StatoTesti>,
    id: i64,
) -> Esito<TestoIpc> {
    // ── sotto lucchetto: chi è questo brano, e si può chiedere ──────────────
    let (brano, rete) = con_libreria(&stato, |libreria| {
        Ok((
            testi::brano(&libreria.connection, id)?,
            rete_attiva(&libreria.connection),
        ))
    })
    .map_err(errore)?;

    if !rete {
        // Non è un errore: è una scelta di chi usa il programma, e la risposta
        // giusta è quel che si sa senza rete.
        return testo_gia_saputo(&stato, id).map_err(errore);
    }

    // ── senza nessun lucchetto ──────────────────────────────────────────────
    let fornitori = servizi(&app, &testi).map_err(errore)?;
    let voce = testi::cerca_in_rete(&fornitori, &brano).map_err(errore)?;

    // ── di nuovo sotto lucchetto: scrivere, e rileggere quel che ne esce ────
    con_libreria(&stato, |libreria| {
        testi::ricorda_esito(&libreria.connection, &brano, voce.as_ref())?;
        Ok(TestoIpc::from(testi::per_questo_brano(
            &libreria.connection,
            &brano,
        )))
    })
    .map_err(errore)
}

/// Richiede il testo al catalogo **ignorando quel che si ricordava**.
///
/// # Perché non basta [`testo_cerca`]
///
/// Perché fra il pannello e la rete ci sono due memorie, e tutt'e due
/// risponderebbero prima che parta una richiesta:
///
/// * il **deposito** di `aether-meta` tiene un «non ce l'ho» per tre giorni e
///   una risposta per una settimana;
/// * la colonna `checked_at` tiene «a questo brano si è già chiesto» per
///   quattordici giorni, ed è quel che decide `da_chiedere`.
///
/// Chi ha davanti un pannello vuoto e preme «Cerca di nuovo» avrebbe quindi
/// rivisto lo stesso vuoto, senza nemmeno una richiesta partita, e avrebbe
/// concluso — con ragione — che il pulsante è finto. Questo comando toglie di
/// mezzo tutt'e due: azzera `checked_at` e chiede senza rileggere il deposito.
///
/// Il ritentativo automatico invece **non** è questo: i due tentativi in più a
/// mezzo secondo e a un secondo e mezzo stanno in `aether_app::testi`, valgono
/// per ogni richiesta e servono all'inciampo di rete. Questo qui è un gesto, e
/// costa una richiesta a un servizio pubblico: per questo lo fa solo chi lo
/// chiede, un brano alla volta.
///
/// # Errori
///
/// `library.trackNotFound`, l'errore di rete quando il catalogo non risponde,
/// `db.queryFailed`. Con l'interruttore spento **non** è un errore: si
/// risponde quel che si sa dal disco, come [`testo_cerca`].
///
/// `(async)`: come [`testo_cerca`], e per la stessa ragione — la scadenza è di
/// venti secondi, più le due attese del ritentativo.
#[tauri::command(async)]
pub fn testo_cerca_di_nuovo(
    app: AppHandle,
    stato: State<'_, Stato>,
    testi: State<'_, StatoTesti>,
    id: i64,
) -> Esito<TestoIpc> {
    // ── sotto lucchetto: chi è, si può chiedere, e si dimentica di averlo
    // già chiesto ──────────────────────────────────────────────────────────
    let (brano, rete) = con_libreria(&stato, |libreria| {
        let brano = testi::brano(&libreria.connection, id)?;
        testi::dimentica_esito(&libreria.connection, &brano.track_key)?;
        let rete = rete_attiva(&libreria.connection);
        Ok((brano, rete))
    })
    .map_err(errore)?;

    if !rete {
        return testo_gia_saputo(&stato, id).map_err(errore);
    }

    // ── senza nessun lucchetto ──────────────────────────────────────────────
    let fornitori = servizi(&app, &testi).map_err(errore)?;
    let voce = testi::cerca_di_nuovo_in_rete(&fornitori, &brano).map_err(errore)?;

    // ── di nuovo sotto lucchetto: scrivere, e rileggere quel che ne esce ────
    con_libreria(&stato, |libreria| {
        testi::ricorda_esito(&libreria.connection, &brano, voce.as_ref())?;
        Ok(TestoIpc::from(testi::per_questo_brano(
            &libreria.connection,
            &brano,
        )))
    })
    .map_err(errore)
}

/// Come stanno i testi sulla libreria intera.
///
/// **Non tocca la rete**, come `scarico_stato` e per la stessa ragione: lo
/// chiama ogni pannello che si apre, e farne una sonda vorrebbe dire
/// un'interfaccia che aspetta un servizio esterno per disegnare quattro numeri.
///
/// # Errori
///
/// `db.queryFailed`.
#[tauri::command]
pub fn testi_stato(stato: State<'_, Stato>, testi: State<'_, StatoTesti>) -> Esito<StatoTestiIpc> {
    stato_adesso(&stato, &testi).map_err(errore)
}

/// Accende o spegne le richieste al catalogo.
///
/// # Errori
///
/// `db.queryFailed`.
#[tauri::command(async)]
pub fn testi_rete(
    stato: State<'_, Stato>,
    testi: State<'_, StatoTesti>,
    attivo: bool,
) -> Esito<StatoTestiIpc> {
    con_libreria(&stato, |libreria| {
        settings::write(
            &libreria.connection,
            CHIAVE_RETE,
            if attivo { "1" } else { "0" },
        )
    })
    .map_err(errore)?;
    // Spegnendo si ferma anche quel che sta girando: lasciar finire una passata
    // dopo aver spento l'interruttore vorrebbe dire che l'interruttore non
    // spegne, e chi l'ha premuto guarderebbe le richieste continuare.
    if !attivo {
        testi.ferma();
    }
    stato_adesso(&stato, &testi).map_err(errore)
}

/// Cerca il testo di tutti i brani che ne sono senza.
///
/// Torna subito: il lavoro va su un filo suo, e l'avanzamento arriva con
/// l'evento `testi:avanzamento`.
///
/// # Errori
///
/// `db.queryFailed`.
#[tauri::command]
pub fn testi_riempi(
    app: AppHandle,
    stato: State<'_, Stato>,
    testi: State<'_, StatoTesti>,
) -> Esito<StatoTestiIpc> {
    avvia(&app);
    stato_adesso(&stato, &testi).map_err(errore)
}

/// Chiede alla passata in corso di fermarsi.
#[tauri::command]
pub fn testi_ferma(testi: State<'_, StatoTesti>) -> Esito<()> {
    testi.ferma();
    Ok(())
}

// ── la passata ──────────────────────────────────────────────────────────────

/// Fa partire il filo, se non ce n'è già uno.
fn avvia(app: &AppHandle) {
    let Some(testi) = app.try_state::<StatoTesti>() else {
        return;
    };
    if testi.in_corso.load(Ordering::Acquire) {
        // Ce n'è già una, e rilegge la tabella a ogni lotto: i brani entrati
        // nel frattempo li prende lei. Non serve accodare una seconda passata.
        return;
    }
    // Prima di partire: un «fermati» arrivato dopo la fine della passata
    // precedente fermerebbe questa al primo brano.
    testi.da_fermare.store(false, Ordering::Relaxed);
    testi.fatti.store(0, Ordering::Relaxed);

    let manico = app.clone();
    let avviato = std::thread::Builder::new()
        .name("aether-testi".to_owned())
        .spawn(move || {
            passata(&manico);
            manico.emetti("testi:finito", ());
        });
    if let Err(err) = avviato {
        nota!("[testi] il filo della passata non è partito: {err}");
    }
}

/// Una passata completa: legge lotti finché ce n'è, o finché non si ferma.
///
/// # L'ordine dei lucchetti
///
/// Come l'arricchimento, e vale la pena ripeterlo perché è la regola che tiene
/// in piedi la riproduzione:
///
/// > **Il filo dei testi non prende mai il lucchetto del lettore, e prende
/// > quello della libreria solo in finestre brevi: una per leggere il lotto,
/// > una per ogni esito da scrivere.**
///
/// Tutto quel che sta in mezzo — le richieste al catalogo, che sono la parte
/// lenta — avviene **senza nessun lucchetto**.
///
/// # Un guasto solo non ferma la passata
///
/// Prima sì: il primo errore di rete usciva dal filo, e su una libreria di
/// ventimila brani bastava un `503` isolato — o un titolo che faceva rispondere
/// male il catalogo — perché la passata si chiudesse dopo quattro brani. Chi
/// guardava vedeva la barra fermarsi senza motivo e ricominciava da capo, per
/// rifare le stesse quattro richieste.
///
/// Adesso si contano i guasti **di fila**: uno solo si salta, cinque
/// consecutivi fermano tutto — a quel punto non è un inciampo, è la rete che
/// non c'è o l'interruttore della `Cadenza` che si è aperto, e insistere
/// vorrebbe dire duecento richieste rifiutate in mezzo minuto. Il contatore si
/// azzera a ogni successo, perché quel che conta è la sequenza, non il totale.
///
/// Alla finestra si segnala **solo il primo** guasto di ogni sequenza: cinque
/// fasce d'errore identiche una dietro l'altra dicono la stessa cosa cinque
/// volte e coprono metà schermo.
fn passata(app: &AppHandle) {
    let (Some(stato), Some(testi)) = (app.try_state::<Stato>(), app.try_state::<StatoTesti>())
    else {
        return;
    };
    let Some(_turno) = Turno::prendi(&testi.in_corso) else {
        return;
    };

    let fornitori = match servizi(app, &testi) {
        Ok(fornitori) => fornitori,
        Err(err) => {
            segnala(app, &err);
            return;
        }
    };

    // I guasti di rete uno dietro l'altro, senza un successo in mezzo. Vive
    // fuori dal ciclo dei lotti: una sequenza non ricomincia perché è finito
    // un lotto di cinquanta.
    let mut sequenza = Sequenza::default();

    loop {
        if testi.da_fermare.load(Ordering::Relaxed) {
            break;
        }

        // ── finestra 1: il lotto, e quanti ne restano dopo ──────────────────
        let letto = con_libreria(&stato, |libreria| {
            if !rete_attiva(&libreria.connection) {
                return Ok(None);
            }
            let adesso = adesso_ms();
            let lotto = testi::da_cercare(&libreria.connection, adesso, testi::LOTTO)?;
            // Quanti ne restano lo dice la **coda**, non la copertura: un brano
            // di cui si ha il solo testo piatto non è fra i «mancanti» e la
            // passata però ci passa, e contarlo con l'altro numero farebbe
            // arrivare la barra a zero con ancora mezza libreria da guardare.
            let restano = testi::quanti_da_cercare(&libreria.connection, adesso)?;
            Ok(Some((lotto, restano)))
        });
        let Ok(Some((lotto, restano))) = letto else {
            if let Err(err) = letto {
                segnala(app, &err);
            }
            break;
        };
        if lotto.is_empty() {
            break;
        }
        testi.rimasti.store(
            u32::try_from(restano).unwrap_or(u32::MAX),
            Ordering::Relaxed,
        );

        // Quanti brani di questo lotto si sono chiusi con un esito scritto.
        // Serve a garantire che la passata **avanzi**: la coda si legge da
        // capo a ogni giro, quindi un lotto in cui nessuno riesce tornerebbe
        // identico al giro dopo, per sempre. Zero successi vuol dire che non
        // c'è niente da guadagnare a rileggere la stessa coda.
        let mut riusciti = 0_usize;

        for brano in lotto {
            if testi.da_fermare.load(Ordering::Relaxed) {
                return;
            }

            // ── senza lucchetto: la parte lenta ─────────────────────────────
            let voce = match testi::cerca_in_rete(&fornitori, &brano) {
                Ok(voce) => {
                    sequenza.riuscito();
                    voce
                }
                Err(err) => {
                    // Il catalogo non ha risposto per **questo** brano. Non si
                    // registra niente — «non si sa» non è «non c'è» — e non si
                    // esce per forza: il prossimo può andare benissimo, e un
                    // titolo che fa inciampare il catalogo non è una ragione
                    // per lasciare ventimila brani senza testo. Quando fermarsi
                    // lo dice `Sequenza`, che è provata.
                    match sequenza.guasto() {
                        Reazione::ProseguiSegnalando => {
                            segnala(app, &err);
                            continue;
                        }
                        Reazione::Prosegui => continue,
                        Reazione::Ferma => return,
                    }
                }
            };

            // ── finestra 2: scrivere l'esito ────────────────────────────────
            if let Err(err) = con_libreria(&stato, |libreria| {
                testi::ricorda_esito(&libreria.connection, &brano, voce.as_ref())
            }) {
                segnala(app, &err);
                return;
            }

            riusciti = riusciti.saturating_add(1);
            let fatti = testi
                .fatti
                .fetch_add(1, Ordering::Relaxed)
                .saturating_add(1);
            // `fetch_update` restituisce il valore PRECEDENTE, come il
            // `fetch_add` qui sopra: senza la sottrazione l'evento direbbe
            // sempre un rimasto di troppo, e la barra non arriverebbe mai a
            // zero prima di `testi:finito`.
            let rimasti = testi
                .rimasti
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |r| {
                    Some(r.saturating_sub(1))
                })
                .map(|precedente| precedente.saturating_sub(1))
                .unwrap_or(0);
            app.emetti("testi:avanzamento", Avanzamento { fatti, rimasti });
        }

        if riusciti == 0 {
            break;
        }
    }
}

/// Quanti guasti di rete uno dietro l'altro fermano una passata.
///
/// Cinque. Meno vorrebbe dire fermarsi su una raffica che passa; molti di più
/// vorrebbe dire bussare per minuti a un servizio pubblico che ci ha appena
/// detto di no cinque volte di fila.
const GUASTI_DI_FILA: usize = 5;

/// Cosa fa la passata dopo un guasto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reazione {
    /// Si passa al brano dopo, in silenzio.
    Prosegui,
    /// Si passa al brano dopo, ma questo guasto si mostra: è il primo.
    ProseguiSegnalando,
    /// Basta: non è più un inciampo.
    Ferma,
}

/// La memoria dei guasti consecutivi di una passata.
///
/// Un tipo suo, e non due righe dentro il ciclo, perché è la sola parte di
/// [`passata`] che si può provare senza rete e senza una finestra — e la regola
/// che decide se ventimila brani restano senza testo merita una prova.
#[derive(Debug, Clone, Copy, Default)]
struct Sequenza {
    /// Quanti guasti dall'ultimo successo.
    di_fila: usize,
}

impl Sequenza {
    /// Un brano si è chiuso con un esito scritto: la sequenza ricomincia.
    const fn riuscito(&mut self) {
        self.di_fila = 0;
    }

    /// Un brano è caduto, e questo è quel che si fa.
    const fn guasto(&mut self) -> Reazione {
        let primo = self.di_fila == 0;
        self.di_fila = self.di_fila.saturating_add(1);
        if self.di_fila >= GUASTI_DI_FILA {
            Reazione::Ferma
        } else if primo {
            Reazione::ProseguiSegnalando
        } else {
            Reazione::Prosegui
        }
    }
}

#[cfg(test)]
mod prove {
    use super::{
        GUASTI_DI_FILA, ParolaSalvata, Reazione, RigaSalvata, Sequenza, componi, gemello_esteso,
    };

    #[test]
    fn cinque_guasti_di_fila_fermano_la_passata_uno_no() {
        let mut sequenza = Sequenza::default();

        // Uno solo si salta, e lo si dice una volta: era il difetto — il primo
        // errore di rete usciva dal filo, e la passata si chiudeva dopo quattro
        // brani su ventimila.
        assert_eq!(sequenza.guasto(), Reazione::ProseguiSegnalando);
        sequenza.riuscito();

        // E il primo della sequenza **dopo** un successo si segnala di nuovo:
        // è un guasto nuovo, non la coda di quello di prima.
        assert_eq!(sequenza.guasto(), Reazione::ProseguiSegnalando);
        // I successivi no: cinque fasce identiche dicono la stessa cosa cinque
        // volte.
        for numero in 2..GUASTI_DI_FILA {
            assert_eq!(
                sequenza.guasto(),
                Reazione::Prosegui,
                "il guasto numero {numero} non si mostra"
            );
        }
        // Il quinto ferma tutto: a quel punto non è un titolo storto, è la rete
        // che non c'è.
        assert_eq!(sequenza.guasto(), Reazione::Ferma);

        // Quattro guasti alternati a successi non fermano niente, e sono
        // quattro fasce: quattro guasti veri, ognuno il primo della sua
        // sequenza.
        let mut alternata = Sequenza::default();
        for _ in 0..10 {
            assert_eq!(alternata.guasto(), Reazione::ProseguiSegnalando);
            alternata.riuscito();
        }
    }

    /// Una riga con le sue parole, come la manda la finestra: gli spazi in coda
    /// stanno attaccati alla parola che li precede.
    fn riga(ms: u32, testo: &str, parole: &[(u32, &str)]) -> RigaSalvata {
        RigaSalvata {
            ms,
            testo: testo.to_owned(),
            parole: parole
                .iter()
                .map(|(quando, pezzo)| ParolaSalvata {
                    ms: *quando,
                    testo: (*pezzo).to_owned(),
                })
                .collect(),
        }
    }

    #[test]
    fn le_parole_arrivano_dall_ipc_come_le_manda_la_finestra() {
        // Il viaggio vero: il payload che `ipc.testoSalva` costruisce, letto
        // con lo stesso serde del comando. Se un giorno il nome di un campo
        // cambia da una parte sola, è qui che si vede.
        let arrivato: Vec<RigaSalvata> = serde_json::from_str(
            r#"[{"ms":10000,"testo":"prima riga","parole":[
                 {"ms":10000,"testo":"prima "},
                 {"ms":10400,"testo":"riga"}]}]"#,
        )
        .expect("il payload dell'editor");
        assert_eq!(arrivato.len(), 1);
        let prima = arrivato.first().expect("una riga");
        assert_eq!(prima.parole.len(), 2);
        assert_eq!(prima.parole.first().map(|p| p.ms), Some(10_000));
        assert_eq!(
            prima.parole.first().map(|p| p.testo.as_str()),
            Some("prima ")
        );
    }

    #[test]
    fn le_parole_sopravvivono_al_viaggio_e_finiscono_nell_esteso() {
        let gemelli = componi(vec![
            riga(
                10_000,
                "prima riga",
                &[(10_000, "prima "), (10_400, "riga")],
            ),
            riga(20_000, "seconda", &[(20_000, "seconda")]),
        ]);
        assert_eq!(
            gemelli.esteso.as_deref(),
            Some(concat!(
                "[00:10.00]<00:10.00>prima <00:10.40>riga\n",
                "[00:20.00]<00:20.00>seconda\n",
            ))
        );
        // E si rileggono: è il giro completo, con la stessa coppia di funzioni
        // che il resto dell'applicazione usa.
        let riletto = aether_domain::testo::leggi(gemelli.esteso.as_deref().expect("l'esteso"));
        assert_eq!(
            riletto
                .righe
                .first()
                .map(|r| r.parole.iter().map(|p| p.ms).collect::<Vec<_>>()),
            Some(vec![10_000, 10_400])
        );
    }

    #[test]
    fn il_gemello_semplice_e_lo_stesso_lrc_di_sempre() {
        // Quel che leggono gli altri lettori: nessun `<…>`, gli stessi tempi di
        // riga. È la metà del lavoro che non deve regredire mai.
        let gemelli = componi(vec![
            riga(
                10_000,
                "prima riga",
                &[(10_000, "prima "), (10_400, "riga")],
            ),
            riga(20_000, "seconda", &[]),
        ]);
        assert_eq!(
            gemelli.semplice,
            "[00:10.00]prima riga\n[00:20.00]seconda\n"
        );
        assert!(!gemelli.semplice.contains('<'));
    }

    #[test]
    fn senza_parole_non_si_scrive_nessun_esteso() {
        let gemelli = componi(vec![riga(10_000, "prima riga", &[])]);
        assert_eq!(gemelli.esteso, None);
        assert_eq!(gemelli.semplice, "[00:10.00]prima riga\n");
    }

    #[test]
    fn le_parole_che_non_ricompongono_la_riga_si_buttano() {
        // Il testo è cambiato dopo che le parole erano state battute: i tempi
        // non si riassegnano a caso, la riga torna sincronizzata al verso.
        let gemelli = componi(vec![riga(
            10_000,
            "prima strofa",
            &[(10_000, "prima "), (10_400, "riga")],
        )]);
        assert_eq!(gemelli.esteso, None);
        assert_eq!(gemelli.semplice, "[00:10.00]prima strofa\n");
    }

    #[test]
    fn il_gemello_esteso_si_scrive_accanto_al_brano_e_si_toglie() {
        let cartella = tempfile::tempdir().expect("una cartella");
        let brano = cartella.path().join("una canzone.mp3");
        let esteso = cartella.path().join("una canzone.a2.lrc");

        gemello_esteso(&brano, Some("[00:10.00]<00:10.00>parola\n")).expect("scrittura");
        assert_eq!(
            std::fs::read_to_string(&esteso).expect("l'esteso"),
            "[00:10.00]<00:10.00>parola\n"
        );

        // Risincronizzato senza battere le parole: l'esteso di ieri se ne va,
        // altrimenti scavalcherebbe per sempre il `.lrc` di oggi.
        gemello_esteso(&brano, None).expect("rimozione");
        assert!(!esteso.exists());

        // E toglierlo quando non c'è non è un guasto.
        gemello_esteso(&brano, None).expect("niente da togliere");
    }
}

// ── il precaricamento ───────────────────────────────────────────────────────

/// Cerca in disparte il testo del brano che verrà dopo.
///
/// # Il difetto che chiude
///
/// La rete partiva **solo aprendo il pannello**: chi lascia il pannello aperto
/// e ascolta un album vedeva, a ogni cambio di brano, qualche secondo di
/// «Cerco il testo…» prima della prima riga — cioè proprio l'introduzione, che
/// è la parte in cui il testo serve. E quei secondi non sono la rete: sono la
/// [`aether_meta::Cadenza`], che concede una richiesta ogni quarto di secondo,
/// più la richiesta esatta e quella generosa.
///
/// Chiedendolo mentre il brano corrente suona ancora, il testo del successivo è
/// già in tabella quando tocca a lui, e il pannello lo trova senza andare in
/// rete: `testo_brano` non la tocca mai.
///
/// # Quanto costa
///
/// **Una richiesta per brano**, e solo quando quel brano un testo che scorre
/// non ce l'ha — la condizione è `da_chiedere`, la stessa che usa il pannello,
/// letta dal nucleo e non ricostruita qui. Nessuna richiesta se l'interruttore
/// è spento, se il successivo è lo stesso di prima, o se un altro
/// precaricamento sta ancora girando. In fila dietro la stessa `Cadenza` di
/// tutto il resto, quindi non scavalca né una passata né il pannello.
///
/// # Perché un filo e non `in_disparte`
///
/// Perché chi chiama è l'osservatore del motore, che è sincrono e ha in mano il
/// lucchetto del lettore: da lì non si può aspettare un futuro, e non si deve
/// tenere fermo il filo della decodifica nemmeno per un microsecondo in più.
/// Questa funzione torna subito, e il lavoro comincia altrove — è la stessa
/// forma di `crate::scrobble::sta_suonando`, chiamata due righe più su.
pub fn precarica_prossimo(app: &AppHandle, prossimo: Option<i64>) {
    let Some(id) = prossimo.filter(|id| *id > 0) else {
        return;
    };
    let Some(testi) = app.try_state::<StatoTesti>() else {
        return;
    };
    // `Iniziato` arriva a ogni cambio di traccia; il successivo, quasi sempre,
    // è già quello che si era precaricato al brano prima.
    if testi.precaricato.swap(id, Ordering::Relaxed) == id {
        return;
    }
    let manico = app.clone();
    let avviato = std::thread::Builder::new()
        .name("aether-testo-dopo".to_owned())
        .spawn(move || precarica_adesso(&manico, id));
    if let Err(err) = avviato {
        // Senza il filo si perde l'anticipo, non il testo: il pannello lo
        // chiederà quando toccherà a quel brano. Vale una riga nel diario.
        nota!("[testi] il filo del precaricamento non è partito: {err}");
    }
}

/// Il precaricamento vero, sul suo filo. Gli stessi tre tempi di [`testo_cerca`].
fn precarica_adesso(app: &AppHandle, id: i64) {
    let (Some(stato), Some(testi)) = (app.try_state::<Stato>(), app.try_state::<StatoTesti>())
    else {
        return;
    };
    let Some(_turno) = Turno::prendi(&testi.precarica) else {
        return;
    };

    // ── sotto lucchetto: chi è, si può chiedere, e serve davvero ────────────
    let letto = con_libreria(&stato, |libreria| {
        if !rete_attiva(&libreria.connection) {
            return Ok(None);
        }
        let brano = testi::brano(&libreria.connection, id)?;
        // La condizione è quella del pannello, letta dal nucleo: un brano che
        // il testo ce l'ha già — o a cui si è già chiesto — non si chiede.
        let serve = testi::per_questo_brano(&libreria.connection, &brano).da_chiedere;
        Ok(serve.then_some(brano))
    });
    let Ok(Some(brano)) = letto else {
        return;
    };

    // ── senza nessun lucchetto: la parte lenta ──────────────────────────────
    let Ok(fornitori) = servizi(app, &testi) else {
        return;
    };
    let voce = match testi::cerca_in_rete(&fornitori, &brano) {
        Ok(voce) => voce,
        // In silenzio, e non è una svista: nessuno ha chiesto questo testo, e
        // una fascia d'errore per un brano che non si sta nemmeno ascoltando
        // sarebbe un guasto annunciato per un lavoro che nessuno aspettava.
        // Quando toccherà a quel brano, il pannello ci riproverà e allora
        // l'errore sarà una risposta a una domanda.
        Err(_) => return,
    };

    // ── di nuovo sotto lucchetto: scrivere ──────────────────────────────────
    let _ = con_libreria(&stato, |libreria| {
        testi::ricorda_esito(&libreria.connection, &brano, voce.as_ref())
    });
}

/// L'avanzamento di una passata, in brani.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct Avanzamento {
    /// Quanti se ne sono guardati.
    fatti: u32,
    /// Quanti ne restano.
    rimasti: u32,
}

/// Manda un guasto alla finestra, che decide se e come mostrarlo.
fn segnala(app: &AppHandle, err: &AppError) {
    app.emetti("testi:guasto", crate::errore::ErroreIpc::from(err.clone()));
}

// ── sincronizzare a mano ────────────────────────────────────────────────────

/// Raddrizza delle battute date a orecchio, usando gli attacchi del brano.
///
/// # Perché gli attacchi non attraversano l'IPC
///
/// Perché sono migliaia: un brano di quattro minuti ne ha qualche centinaio, e
/// mandarli alla finestra perché li rimandi indietro sarebbe traffico per un
/// calcolo che la finestra non fa comunque. Quel che serve là è il risultato —
/// i tempi raddrizzati — e la regola che li produce sta in
/// `aether_domain::testo::aggancia`, che è pura e provata senza aprire un file.
///
/// Decodifica il brano per intero: uno o due secondi su quattro minuti. Per
/// questo è `(async)`: un comando normale gira sul **filo principale** della
/// finestra — è la regola annotata in `nuvola`, e vale qui come per
/// `scansiona` — e due secondi lì sopra sono due secondi di editor congelato.
///
/// # Errori
///
/// `playback.sourceUnavailable` se il file non si apre,
/// `playback.formatUnsupported` per un formato che il motore non sa leggere.
/// Un brano di cui non si trova nessun attacco **non** è un errore: le battute
/// tornano com'erano, ed è quel che `aggancia` fa senza candidati.
#[tauri::command(async)]
pub fn testo_aggancia(stato: State<'_, Stato>, id: i64, battute: Vec<u32>) -> Esito<Vec<u32>> {
    // Sotto lucchetto ci va **solo** la riga di database. L'apertura del file no:
    // su una condivisione di rete morta dura quaranta secondi, e tenerci dentro
    // il lucchetto della libreria vorrebbe dire fermare la scansione,
    // l'arricchimento e il backup perché qualcuno ha chiesto di agganciare un
    // testo. La decodifica vera avviene poi ancora più fuori: il
    // `Decodificatore` non sa niente del database.
    let scheda = con_libreria(&stato, |libreria| {
        aether_app::playback::scheda_sorgente(&libreria.connection, id)
    })
    .map_err(errore)?;
    let sorgente =
        aether_app::playback::sorgente_da_scheda(&aether_app::files::LocalFiles, &scheda)
            .map_err(errore)?;
    let attacchi = aether_play::attacchi::attacchi(sorgente).map_err(errore)?;
    Ok(aether_domain::testo::aggancia(&battute, &attacchi))
}

/// Salva un testo sincronizzato a mano.
///
/// Riceve le righe già agganciate: l'aggancio agli attacchi lo fa
/// `aether_domain::testo::aggancia`, che è puro, e la finestra manda quel che ne
/// esce. Qui si compone l'LRC e si scrive — il `.lrc` accanto al brano prima, la
/// riga in tabella dopo.
///
/// # Perché l'LRC si scrive qui e non nella finestra
///
/// Perché comporre un LRC è l'esatto inverso di leggerlo, e i due devono essere
/// la stessa idea del formato: `aether_domain::testo::scrivi` è provato contro
/// `leggi` sugli stessi vettori. Un compositore scritto in TypeScript
/// produrrebbe file che solo Aether rilegge com'erano.
///
/// # Errori
///
/// `library.trackNotFound`, `fs.writeFailed` se la cartella è di sola lettura,
/// `db.queryFailed`.
///
/// # Due file accanto al brano, non uno
///
/// Quando almeno una riga porta i tempi delle sue parole si scrive anche
/// `nome.a2.lrc`, e il `nome.lrc` di sempre resta lì **senza** le parole. Il
/// perché sta su [`Gemelli`]; quel che conta qui è che la riga in tabella porta
/// il **semplice**, non l'esteso, e nemmeno questo è una dimenticanza:
///
/// * la riga è una copia veloce, e il file è la verità — la fonte che si
///   rilegge è il sidecar, dove l'esteso vince già da sé
///   (`aether_app::testi::CODE`);
/// * da quella riga esce quel che si offre a LRCLIB
///   ([`testi::da_restituire`]), e il catalogo si aspetta un LRC che tutti
///   sappiano leggere. Mandargli i `<mm:ss.xx>` vorrebbe dire far arrivare i
///   tempi delle parole a chi li mostrerà come testo.
///
/// Quando parole non ce ne sono l'esteso non si scrive, e se ce n'era uno **si
/// toglie**: vedi [`gemello_esteso`].
///
/// # I due scarti si azzerano entrambi, e non è una dimenticanza
///
/// «Offset» qui nomina due numeri diversi, e confonderli è il modo di rompere
/// questo comando. `aether_domain::testo::posizione_corretta` è l'unico posto in
/// cui si sommano, e dice quali sono:
///
/// * **`Testo::offset_ms`** è il `[offset:]` **del file**, quello che `scrivi`
///   emette in testa all'LRC. Lo azzera questa funzione, ed è la riga
///   `offset_ms: 0` qui sotto;
/// * **`lyrics.offset_ms`** — che in `aether_app::testi` si chiama `scarto_ms`,
///   ed è quello che muovono i due bottoni del pannello — è la correzione di chi
///   ascolta. Lo azzera `aether_app::testi::salva_a_mano`, sulla sua riga.
///
/// Tutti e due, e per la stessa ragione: **le battute nascono nello spazio della
/// posizione grezza.** `Sincronizza.tsx::batti` legge `posizioneAdesso()`, cioè
/// la posizione che il motore racconta, senza applicarci né l'uno né l'altro
/// scarto — il commento accanto a quella chiamata lo dichiara. Il disegno invece
/// accende la riga a `posizione + offset_ms + scarto_ms`, che è lo specchio di
/// `posizione_corretta`.
///
/// Mettendo insieme le due: una riga registrata alla posizione grezza `P` si
/// accende quando `posizione ≥ P − offset_ms − scarto_ms`. Conservare qui uno
/// dei due vorrebbe dire che **ogni** riga appena battuta si accende in anticipo
/// di quel tanto, per sempre, e senza che si veda perché — cioè rimettere a mano
/// lo sfasamento che si era appena finito di togliere a orecchio.
///
/// E non si perde niente, perché non c'è niente da conservare: l'`[offset:]`
/// vecchio descriveva i **tempi** vecchi, e dopo una risincronizzazione quei
/// tempi non esistono più. Dall'editor arrivano solo le parole — `iniziale`, in
/// `Testo.tsx` — e i tempi si ribattono tutti.
///
/// `(async)`: scrive un `.lrc` accanto al file musicale, che può stare su una
/// cartella di rete. Stessa ragione di [`testo_brano`], e qui è una scrittura:
/// costa di più di una lettura anche quando la rete c'è.
#[tauri::command(async)]
pub fn testo_salva(
    app: AppHandle,
    stato: State<'_, Stato>,
    id: i64,
    righe: Vec<RigaSalvata>,
) -> Esito<TestoIpc> {
    let gemelli = componi(righe);

    let esito = con_libreria(&stato, |libreria| {
        let brano = testi::brano(&libreria.connection, id)?;
        // L'esteso per primo, e non è indifferente: se la scrittura del `.lrc`
        // qui sotto fallisce, quel che resta su disco è comunque il file più
        // ricco, con dentro tutto — tempi delle righe compresi. Nell'ordine
        // opposto un guasto lascerebbe accanto al brano un esteso vecchio che
        // scavalca il `.lrc` appena scritto, cioè il caso peggiore.
        gemello_esteso(Path::new(&brano.path), gemelli.esteso.as_deref())?;
        testi::salva_a_mano(&libreria.connection, &brano, &gemelli.semplice)?;
        Ok(TestoIpc::from(testi::per_questo_brano(
            &libreria.connection,
            &brano,
        )))
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Restituisce al catalogo il testo che si è appena sincronizzato.
///
/// # Perché è un comando a sé, e non la coda di `testo_salva`
///
/// Perché mandare qualcosa su un servizio pubblico è un gesto, e un gesto ha
/// bisogno di un momento in cui non farlo. Attaccato al salvataggio sarebbe
/// automatico per definizione — si salva sempre — e chi non se ne fosse accorto
/// avrebbe pubblicato senza saperlo. Separato, il salvataggio resta locale e la
/// pubblicazione resta una cosa che si decide, un brano alla volta.
///
/// Cosa si rifiuta di mandare sta in [`testi::da_restituire`], e non qui: la
/// regola vale anche se un giorno questo comando avesse un secondo punto di
/// chiamata.
///
/// # Quanto ci mette
///
/// Qualche secondo, e la finestra deve dirlo prima: LRCLIB chiede una prova di
/// lavoro, cioè del calcolo, non dell'attesa di rete. Per questo è `(async)`:
/// quei secondi di calcolo più la richiesta non stanno sul filo principale
/// della finestra.
///
/// # Errori
///
/// `metadata.lyricsPublishRefused` se non c'è niente da mandare o se il catalogo
/// dice di no; l'errore di trasporto se la rete non risponde; `db.queryFailed`
/// se il brano non si legge.
#[tauri::command(async)]
pub fn testo_pubblica(
    app: AppHandle,
    stato: State<'_, Stato>,
    testi: State<'_, StatoTesti>,
    id: i64,
) -> Esito<()> {
    // ── sotto lucchetto: cosa si manderebbe, e si può ───────────────────────
    let (cosa, rete) = con_libreria(&stato, |libreria| {
        Ok((
            testi::da_restituire(&libreria.connection, id)?,
            rete_attiva(&libreria.connection),
        ))
    })
    .map_err(errore)?;

    // L'interruttore vale in tutte e due le direzioni. Chi l'ha spento ha detto
    // «niente traffico verso il catalogo», e pubblicare è traffico verso il
    // catalogo — anzi è quello che ne dice di più. Un rifiuto con la ragione
    // dentro, non un successo silenzioso che non ha mandato niente.
    if !rete {
        return Err(errore(AppError::new(
            ErrorCode::MetadataLyricsPublishRefused {
                detail: Some("le richieste al catalogo sono spente".to_owned()),
            },
        )));
    }

    // ── senza nessun lucchetto: la prova di lavoro dura secondi ─────────────
    let fornitori = servizi(&app, &testi).map_err(errore)?;
    testi::restituisci(&fornitori, &cosa).map_err(errore)
}

/// Una parola come la manda l'editor: un tempo e il pezzo di riga che le tocca.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParolaSalvata {
    /// Quando comincia, in millisecondi dall'inizio del brano.
    pub ms: u32,
    /// Il pezzo di riga che le appartiene, **spazi in coda compresi**.
    ///
    /// Gli spazi arrivano fin qui perché sono quelli che separano una parola
    /// dalla successiva quando la riga si ricompone: rifilarli in viaggio
    /// vorrebbe dire riattaccare le parole fra loro. È la stessa ragione
    /// scritta su `aether_domain::testo::Parola`, ed è anche quel che rende
    /// verificabile la corrispondenza fra le parole e la riga.
    pub testo: String,
}

/// Una riga come la manda l'editor: un tempo, delle parole, e i loro tempi.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigaSalvata {
    /// Quando comincia, in millisecondi.
    pub ms: u32,
    /// La riga.
    pub testo: String,
    /// I tempi delle singole parole, quando l'editor li ha battuti.
    ///
    /// Vuoto è la norma: il passo delle parole è facoltativo, e chi sincronizza
    /// al verso non lo attraversa nemmeno. Vuoto **anche** per le righe che
    /// quel passo ha lasciato a metà, e la finestra le manda già così: mezzi
    /// tempi non si completano indovinando.
    pub parole: Vec<ParolaSalvata>,
}

/// I due `.lrc` che una sincronizzazione a mano produce.
///
/// # Perché due file e non uno
///
/// Perché `.a2.lrc` — l'LRC con i `<mm:ss.xx>` dentro la riga — lo leggono in
/// pochi. Lo legge Aether, lo leggono alcuni lettori per telefono, e per tutti
/// gli altri quei `<…>` sono **testo**: aprire un `a2` con un lettore che non
/// lo conosce vuol dire vedere i tempi stampati in mezzo alle parole.
///
/// Scrivere solo l'esteso vorrebbe quindi dire barattare la compatibilità con
/// il resto del mondo contro una funzione che è nostra — e chi sincronizza per
/// sé, sul suo disco, accanto alla sua musica, non ha chiesto quel baratto. I
/// due file stanno accanto allo stesso brano, hanno gli stessi tempi di riga, e
/// chi non sa cosa sia il primo trova il secondo.
///
/// Quale dei due rilegge Aether lo decide `aether_app::testi::CODE`, che guarda
/// `a2.lrc` per primo: il più ricco vince, e il gemello resta lì per gli altri.
struct Gemelli {
    /// Il `.lrc` di sempre: i tempi delle righe e basta.
    semplice: String,
    /// Il `.a2.lrc`, con i tempi delle parole. `None` quando parole non ce ne
    /// sono, e allora il gemello non si scrive affatto.
    esteso: Option<String>,
}

/// Compone i due LRC da quel che l'editor ha mandato.
///
/// Pura: nessun file, nessun database, nessun lucchetto. È qui che si decide
/// cosa finisce su disco, ed è per questo che sta fuori dal comando — la
/// decisione si prova come una chiamata di funzione.
///
/// # Le parole che non combaciano si buttano
///
/// `aether_domain::testo::parole_combaciano` verifica che rimettere in fila le
/// parole ridia la riga. Quando non la ridà, i tempi delle parole di quella
/// riga si scartano e la riga resta sincronizzata al verso: il perché per
/// esteso sta su quella funzione, e in breve è che dei tempi riassegnati a caso
/// sarebbero plausibili a guardarli e sbagliati ad ascoltarli.
///
/// Il controllo qui è la seconda rete, non la prima: la finestra manda già
/// vuote le righe battute a metà. È la rete che vale anche il giorno in cui
/// l'editor cambia, perché il formato non deve dipendere da chi lo riempie.
fn componi(righe: Vec<RigaSalvata>) -> Gemelli {
    let righe: Vec<Riga> = righe
        .into_iter()
        .map(|riga| {
            let piena = Riga {
                ms: riga.ms,
                testo: riga.testo.trim().to_owned(),
                parole: riga
                    .parole
                    .into_iter()
                    .map(|parola| Parola {
                        ms: parola.ms,
                        testo: parola.testo,
                    })
                    .collect(),
            };
            if parole_combaciano(&piena) {
                piena
            } else {
                Riga {
                    parole: Vec::new(),
                    ..piena
                }
            }
        })
        .collect();

    let esteso = righe.iter().any(|riga| !riga.parole.is_empty()).then(|| {
        aether_domain::testo::scrivi(&Testo {
            righe: righe.clone(),
            piatto: None,
            strumentale: false,
            // Zero, e la ragione per esteso sta su `testo_salva`: questi tempi
            // sono misurati sulla posizione grezza.
            offset_ms: 0,
        })
    });

    let semplice = aether_domain::testo::scrivi(&Testo {
        righe: righe
            .into_iter()
            .map(|riga| Riga {
                parole: Vec::new(),
                ..riga
            })
            .collect(),
        piatto: None,
        strumentale: false,
        offset_ms: 0,
    });

    Gemelli { semplice, esteso }
}

/// Mette a posto il `.a2.lrc` accanto al brano: lo scrive, o lo toglie.
///
/// # Perché toglierlo è metà del lavoro
///
/// Perché `aether_app::testi::CODE` guarda `a2.lrc` **prima** di `lrc`. Un
/// esteso rimasto lì da una sincronizzazione di ieri scavalcherebbe il `.lrc`
/// scritto oggi: si salverebbe, non si vedrebbe cambiare niente, e la ragione
/// sarebbe un file che nessuno ha pensato di guardare. Chi rifà i tempi senza
/// battere le parole sta dicendo «quelle parole non valgono più», e il file che
/// le porta se ne va con loro.
///
/// # Errori
///
/// `fs.writeFailed`, con dentro il percorso — vale per la scrittura e per la
/// rimozione: in tutt'e due i casi quel che non è riuscito è mettere il disco
/// nello stato voluto. Un file che non c'era non è un guasto.
fn gemello_esteso(brano: &Path, esteso: Option<&str>) -> Result<(), AppError> {
    let percorso = testi::percorso_sidecar(brano, "a2.lrc");
    let guaio = |err: std::io::Error| {
        AppError::new(ErrorCode::FsWriteFailed {
            path: percorso.display().to_string(),
            detail: None,
        })
        .with_cause(err.to_string())
    };
    match esteso {
        Some(lrc) => std::fs::write(&percorso, lrc).map_err(guaio),
        None => match std::fs::remove_file(&percorso) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(guaio(err)),
        },
    }
}
