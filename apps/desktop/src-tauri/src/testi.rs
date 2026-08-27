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

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use aether_app::enrich::DepositoSqlite;
use aether_app::settings;
use aether_app::testi::{self, Copertura, Fonte};
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::testo::{Aderenza, Riga, Testo};
use aether_meta::Fornitori;
use serde::Serialize;
use tauri::{AppHandle, Emitter as _, Manager as _, State};

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
#[tauri::command]
pub fn testo_brano(stato: State<'_, Stato>, id: i64) -> Esito<TestoIpc> {
    con_libreria(&stato, |libreria| {
        testi::per_brano(&libreria.connection, id).map(TestoIpc::from)
    })
    .map_err(errore)
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
#[tauri::command]
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
#[tauri::command]
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
        return testo_brano(stato, id);
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
#[tauri::command]
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
            let _ = manico.emit("testi:finito", ());
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

        for brano in lotto {
            if testi.da_fermare.load(Ordering::Relaxed) {
                return;
            }

            // ── senza lucchetto: la parte lenta ─────────────────────────────
            let voce = match testi::cerca_in_rete(&fornitori, &brano) {
                Ok(voce) => voce,
                Err(err) => {
                    // Il catalogo non risponde. Non si registra niente — «non
                    // si sa» non è «non c'è» — e non si insiste: l'interruttore
                    // della `Cadenza` è già scattato, e i brani rimasti li
                    // prenderà la passata di domani.
                    segnala(app, &err);
                    return;
                }
            };

            // ── finestra 2: scrivere l'esito ────────────────────────────────
            if let Err(err) = con_libreria(&stato, |libreria| {
                testi::ricorda_esito(&libreria.connection, &brano, voce.as_ref())
            }) {
                segnala(app, &err);
                return;
            }

            let fatti = testi
                .fatti
                .fetch_add(1, Ordering::Relaxed)
                .saturating_add(1);
            let rimasti = testi
                .rimasti
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |r| {
                    Some(r.saturating_sub(1))
                })
                .unwrap_or(0);
            let _ = app.emit("testi:avanzamento", Avanzamento { fatti, rimasti });
        }
    }
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
    let _ = app.emit("testi:guasto", crate::errore::ErroreIpc::from(err.clone()));
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
/// Decodifica il brano per intero: uno o due secondi su quattro minuti. È un
/// comando sincrono come `scansiona`, che ne blocca venti, e per la stessa
/// ragione: Tauri li serve su un filo suo, e la finestra intanto disegna.
///
/// # Errori
///
/// `playback.sourceUnavailable` se il file non si apre,
/// `playback.formatUnsupported` per un formato che il motore non sa leggere.
/// Un brano di cui non si trova nessun attacco **non** è un errore: le battute
/// tornano com'erano, ed è quel che `aggancia` fa senza candidati.
#[tauri::command]
pub fn testo_aggancia(stato: State<'_, Stato>, id: i64, battute: Vec<u32>) -> Esito<Vec<u32>> {
    // La sorgente si costruisce sotto lucchetto — è una lettura di una riga —
    // e la decodifica avviene fuori: il file lo si è già aperto, e il
    // `Decodificatore` non sa niente del database.
    let sorgente = con_libreria(&stato, |libreria| {
        aether_app::playback::sorgente(&libreria.connection, &aether_app::files::LocalFiles, id)
    })
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
#[tauri::command]
pub fn testo_salva(
    app: AppHandle,
    stato: State<'_, Stato>,
    id: i64,
    righe: Vec<RigaSalvata>,
) -> Esito<TestoIpc> {
    let testo = Testo {
        righe: righe
            .into_iter()
            .map(|riga| Riga {
                ms: riga.ms,
                testo: riga.testo.trim().to_owned(),
                parole: Vec::new(),
            })
            .collect(),
        piatto: None,
        strumentale: false,
        // Lo scarto si azzera: un testo appena sincronizzato **su questo file**
        // non ha niente da correggere, e portarsi dietro la correzione di un
        // testo di prima vorrebbe dire spostare tutto quel che si è appena
        // messo a posto.
        offset_ms: 0,
    };
    let lrc = aether_domain::testo::scrivi(&testo);

    let esito = con_libreria(&stato, |libreria| {
        let brano = testi::brano(&libreria.connection, id)?;
        testi::salva_a_mano(&libreria.connection, &brano, &lrc)?;
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
/// lavoro, cioè del calcolo, non dell'attesa di rete.
///
/// # Errori
///
/// `metadata.lyricsPublishRefused` se non c'è niente da mandare o se il catalogo
/// dice di no; l'errore di trasporto se la rete non risponde; `db.queryFailed`
/// se il brano non si legge.
#[tauri::command]
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

/// Una riga come la manda l'editor: un tempo e delle parole.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigaSalvata {
    /// Quando comincia, in millisecondi.
    pub ms: u32,
    /// La riga.
    pub testo: String,
}
