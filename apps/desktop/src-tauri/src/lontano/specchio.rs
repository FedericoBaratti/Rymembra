//! La libreria unica, lato rete: chiede il catalogo degli altri dispositivi e lo
//! porta in `tracks`, manda la posta (un voto, un mi piace) ai proprietari, e
//! prende le copertine pigramente.
//!
//! Le regole — cosa è una riga specchio, come si riabbina, quando si toglie —
//! sono in `rymembra_app::specchio`; qui c'è quel che ha una rete e un filo.
//!
//! # La disciplina dei lucchetti
//!
//! La rete sta **fuori** dal lucchetto della libreria, in tre tempi come
//! `sincronia` e `nuvola`: si legge lo stato (a parte, senza il lucchetto);
//! si chiede il riassunto e un secchio per volta all'altro dispositivo, senza
//! lucchetti; e ogni secchio si scrive in una presa sua, breve, aspettando il
//! turno come ogni comando. Un dispositivo lento o che sparisce a metà non
//! tiene fermo niente: il giro finisce `incompleto` e si riprende al prossimo.
//!
//! # Quando
//!
//! Appena un dispositivo risponde ([`presente`]), poi ogni [`OGNI`] finché
//! risponde, e a richiesta ([`lontano_specchio_ora`]). Ogni dispositivo ha il
//! suo filo e il suo posto: uno lento non fa aspettare gli altri. Un giro che
//! non trova niente di cambiato è una domanda sola (il riassunto).
//!
//! # Le versioni miste
//!
//! A un dispositivo che non dichiara `catalogo` non si chiede niente
//! ([`Dati::dichiara_catalogo`], e comunque `Nodo::chiedi` rifiuta): per la
//! finestra è `nonSupportato`, e il suo Sfoglia continua com'è.
//!
//! # Cosa dice alla finestra
//!
//! L'evento `lontano:specchio` ([`AvanzamentoSpecchio`]): «Leggo la libreria
//! di <PC>… 12.000/50.000» e, a giro finito, quante righe ci sono e quante
//! aspettano una conferma di sparire. Il comando `lontano_specchio_stato`
//! dà l'ultimo di ogni dispositivo a chi arriva dopo.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, Mutex, Once, PoisonError};
use std::time::{Duration, Instant};

use rymembra_app::dispositivi::{self, POSTA_VALUTA, SCADENZA_POSTA_MS};
use rymembra_app::library::condivisa::{PaginaCatalogo, RiassuntoCatalogo};
use rymembra_app::specchio::archivio::{self, Anteprima, Giro};
use rymembra_app::specchio::convalida::convalida_secchio;
use rymembra_app::specchio::valutazioni::Valutazione;
use rymembra_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use rymembra_lontano::protocollo::CAPACITA_CATALOGO;
use rymembra_lontano::{Domanda, Risposta};
use serde::Serialize;
use tauri::{AppHandle, Manager as _};

use super::filo::{Comune, leggi_a_parte, scrivi_in_libreria};
use super::{comune, in_disparte};
use crate::errore::Esito;
use crate::nota;
use crate::spegnimento::Emette as _;
use crate::stato::{Stato, adesso_ms};

/// Ogni quanto si riguarda il catalogo di un dispositivo che risponde.
const OGNI: Duration = Duration::from_secs(5 * 60);

/// Quanto si aspetta il riassunto.
const ATTESA_RIASSUNTO: Duration = Duration::from_secs(30);

/// Quanto si aspetta un secchio: cinquecento righe, compresse, via relay.
const ATTESA_SECCHIO: Duration = Duration::from_secs(30);

/// Quanto si aspetta l'altro per una voce di posta.
const ATTESA_POSTA: Duration = Duration::from_secs(10);

/// Quanto si aspetta una copertina.
const ATTESA_COPERTINA: Duration = Duration::from_secs(15);

/// Quanto si ricorda che l'altro non ha una copertina, prima di richiederla.
const MEMORIA_ASSENTI: Duration = Duration::from_secs(10 * 60);

/// Quante copertine si chiedono insieme a un dispositivo: il suo posto, che
/// non toglie niente a un altro.
const COPERTINE_PER_DISPOSITIVO: usize = 3;

/// In che punto è un giro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Fase {
    /// Niente in corso.
    #[default]
    Inattivo,
    /// Si legge il catalogo: `fatti` su `totali`.
    Leggo,
    /// Finito: `righe` ci sono, `inAttesa` aspettano di sparire.
    Fine,
    /// Non è riuscito: `errore` dice perché. Si riprova.
    Errore,
    /// Il dispositivo non sa servire il catalogo (una versione di prima).
    NonSupportato,
}

/// Quel che la finestra sa del catalogo di un dispositivo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AvanzamentoSpecchio {
    /// La chiave del dispositivo.
    pub dispositivo: String,
    /// In che punto è.
    pub fase: Fase,
    /// Righe lette in questo giro.
    pub fatti: i64,
    /// Righe da leggere in questo giro.
    pub totali: i64,
    /// Righe di questo dispositivo in libreria, a giro finito.
    pub righe: usize,
    /// Righe che il dispositivo non manda più e aspettano una conferma per
    /// sparire.
    pub in_attesa: usize,
    /// Quando è finito l'ultimo giro riuscito, ms.
    pub ultimo_ms: Option<i64>,
    /// Perché non è riuscito: il codice dell'errore.
    pub errore: Option<String>,
}

/// L'ultimo avanzamento di ogni dispositivo, per chi arriva dopo.
static AVANZAMENTI: Mutex<BTreeMap<String, AvanzamentoSpecchio>> = Mutex::new(BTreeMap::new());

fn avanzamenti() -> std::sync::MutexGuard<'static, BTreeMap<String, AvanzamentoSpecchio>> {
    AVANZAMENTI.lock().unwrap_or_else(PoisonError::into_inner)
}

// ── quel che un giro chiede all'app ─────────────────────────────────────────

/// La rete, la libreria e la finestra, viste da un giro: l'app vera, o — nelle
/// prove — due nodi in memoria e un database.
pub(super) trait Dati {
    /// Il dispositivo è ancora un dispositivo collegato.
    fn associato(&self, chiave: &str) -> bool;
    /// Il dispositivo ha detto di saper servire il catalogo.
    fn dichiara_catalogo(&self, chiave: &str) -> bool;
    /// Una domanda al dispositivo.
    fn chiedi(
        &self,
        chiave: &str,
        domanda: Domanda,
        scadenza: Duration,
    ) -> Result<Risposta, AppError>;
    /// Una lettura della libreria, senza il suo lucchetto.
    fn leggi<T>(
        &self,
        azione: impl FnOnce(&rusqlite::Connection) -> Result<T, AppError>,
    ) -> Result<T, AppError>;
    /// Una scrittura della libreria, che aspetta il suo turno.
    fn scrivi<T>(
        &self,
        azione: impl FnOnce(&rusqlite::Connection) -> Result<T, AppError>,
    ) -> Result<T, AppError>;
    /// Dice alla finestra a che punto si è.
    fn emetti(&self, avanzamento: &AvanzamentoSpecchio);
}

/// L'app vera.
struct DallApp {
    app: AppHandle,
    comune: Arc<Comune>,
}

impl DallApp {
    fn nuovo(app: &AppHandle) -> Result<Self, AppError> {
        Ok(Self {
            app: app.clone(),
            comune: comune(app)?,
        })
    }
}

impl Dati for DallApp {
    fn associato(&self, chiave: &str) -> bool {
        self.comune.associato(chiave)
    }

    fn dichiara_catalogo(&self, chiave: &str) -> bool {
        self.comune
            .nodo()
            .is_some_and(|nodo| nodo.dichiara(chiave, CAPACITA_CATALOGO))
    }

    fn chiedi(
        &self,
        chiave: &str,
        domanda: Domanda,
        scadenza: Duration,
    ) -> Result<Risposta, AppError> {
        // Il nodo si prende a ogni domanda e si lascia subito: spento
        // «Raggiungibile da internet», il filo lo deve poter fermare davvero.
        let nodo = self
            .comune
            .nodo()
            .ok_or_else(|| AppError::new(ErrorCode::LontanoSpento))?;
        nodo.chiedi(chiave, domanda, scadenza)
    }

    fn leggi<T>(
        &self,
        azione: impl FnOnce(&rusqlite::Connection) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        leggi_a_parte(&self.app, azione)
    }

    fn scrivi<T>(
        &self,
        azione: impl FnOnce(&rusqlite::Connection) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        scrivi_in_libreria(&self.app, azione)
    }

    fn emetti(&self, avanzamento: &AvanzamentoSpecchio) {
        self.app.emetti("lontano:specchio", avanzamento.clone());
    }
}

/// Ricorda e dice a che punto si è.
fn avanza<D: Dati>(dati: &D, avanzamento: AvanzamentoSpecchio) {
    avanzamenti().insert(avanzamento.dispositivo.clone(), avanzamento.clone());
    dati.emetti(&avanzamento);
}

// ── il giro ─────────────────────────────────────────────────────────────────

/// Com'è andato un giro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct EsitoGiro {
    /// C'era qualcosa da leggere.
    pub lavorato: bool,
    /// Tutto il catalogo è stato letto.
    pub completo: bool,
    /// Righe di questo dispositivo, dopo il giro.
    pub righe: usize,
    /// Righe che aspettano la conferma di sparire.
    pub in_attesa: usize,
}

/// Il codice con cui si dice un guasto alla finestra.
fn codice(err: &AppError) -> String {
    err.code().kind().code().to_owned()
}

/// Un giro con `chiave`: il riassunto, i secchi cambiati, la chiusura.
///
/// # Errori
///
/// `lontano.nonAssociato` se non è più un dispositivo collegato; quelli della
/// rete e della libreria. Un secchio che non arriva non è un errore: il giro
/// si chiude `incompleto`, e riprende dal punto in cui era.
pub(super) fn giro<D: Dati>(dati: &D, chiave: &str) -> Result<EsitoGiro, AppError> {
    if !dati.associato(chiave) {
        return Err(AppError::new(ErrorCode::LontanoNonAssociato));
    }
    let base = AvanzamentoSpecchio {
        dispositivo: chiave.to_owned(),
        ..AvanzamentoSpecchio::default()
    };
    if !dati.dichiara_catalogo(chiave) {
        // Una versione di prima, o un dispositivo che non si è ancora
        // presentato: niente da chiedere, e niente di rotto.
        avanza(
            dati,
            AvanzamentoSpecchio {
                fase: Fase::NonSupportato,
                ..base
            },
        );
        return Ok(EsitoGiro::default());
    }
    let stato = dati.leggi(|c| archivio::stato(c, chiave))?;
    let riassunto = chiedi_il_riassunto(dati, chiave)?;
    let (riassunto, scartati) = archivio::convalida_riassunto(riassunto);
    if scartati > 0 {
        nota!("[lontano] il riassunto di un dispositivo ha {scartati} secchi fuori dai tetti");
    }
    let lavoro = archivio::lavoro(&riassunto, &stato);
    if lavoro.niente {
        avanza(
            dati,
            AvanzamentoSpecchio {
                fase: Fase::Fine,
                righe: stato.righe,
                in_attesa: dati
                    .leggi(|c| archivio::sparite_in_attesa(c, chiave))
                    .unwrap_or(0),
                ultimo_ms: Some(stato.ultimo_ms),
                ..base
            },
        );
        return Ok(EsitoGiro {
            lavorato: false,
            completo: true,
            righe: stato.righe,
            in_attesa: 0,
        });
    }
    let totali: i64 = lavoro.da_leggere.iter().map(|s| s.righe).sum();
    let mut in_corso = Giro::nuovo(chiave, stato.righe);
    let mut fatti = 0_i64;
    let mut completo = true;
    let mut guasto: Option<AppError> = None;
    avanza(
        dati,
        AvanzamentoSpecchio {
            fase: Fase::Leggo,
            fatti,
            totali,
            righe: stato.righe,
            ..base.clone()
        },
    );
    for secchio in &lavoro.da_leggere {
        // Il cancello a ogni secchio: un dispositivo dimenticato nel frattempo
        // non si legge più.
        if !dati.associato(chiave) {
            completo = false;
            guasto = Some(AppError::new(ErrorCode::LontanoNonAssociato));
            break;
        }
        let pagina = match chiedi_il_secchio(dati, chiave, secchio.da_id, secchio.a_id) {
            Ok(pagina) => pagina,
            Err(err) => {
                nota!(
                    "[lontano] un secchio del catalogo non arriva codice={}: si riprende al giro dopo",
                    err.code().kind().code()
                );
                completo = false;
                guasto = Some(err);
                break;
            }
        };
        let buone = convalida_secchio(secchio.da_id, secchio.a_id, pagina.brani, adesso_ms());
        if buone.scartate > 0 {
            nota!(
                "[lontano] {} righe di un secchio fuori dai tetti non entrano",
                buone.scartate
            );
        }
        let quando = adesso_ms();
        let scritto = dati.scrivi(|c| in_corso.applica_secchio(c, secchio, buone.righe, quando));
        if let Err(err) = scritto {
            completo = false;
            guasto = Some(err);
            break;
        }
        fatti = fatti.saturating_add(secchio.righe);
        avanza(
            dati,
            AvanzamentoSpecchio {
                fase: Fase::Leggo,
                fatti,
                totali,
                righe: in_corso.righe_adesso(),
                ..base.clone()
            },
        );
    }
    let quando = adesso_ms();
    let generazione = riassunto.generazione;
    let da_svuotare = lavoro.da_svuotare.clone();
    let chiusura = dati.scrivi(|c| in_corso.chiudi(c, &da_svuotare, completo, generazione, quando));
    let chiusura = match chiusura {
        Ok(chiusura) => chiusura,
        Err(err) => {
            avanza(
                dati,
                AvanzamentoSpecchio {
                    fase: Fase::Errore,
                    errore: Some(codice(&err)),
                    ..base
                },
            );
            return Err(err);
        }
    };
    let righe = dati
        .leggi(|c| archivio::stato(c, chiave))
        .map_or(0, |s| s.righe);
    avanza(
        dati,
        AvanzamentoSpecchio {
            fase: if completo { Fase::Fine } else { Fase::Errore },
            fatti,
            totali,
            righe,
            in_attesa: chiusura.in_attesa,
            ultimo_ms: completo.then_some(quando),
            errore: guasto.as_ref().map(codice),
            ..base
        },
    );
    Ok(EsitoGiro {
        lavorato: true,
        completo,
        righe,
        in_attesa: chiusura.in_attesa,
    })
}

/// Il riassunto del catalogo di `chiave`.
fn chiedi_il_riassunto<D: Dati>(dati: &D, chiave: &str) -> Result<RiassuntoCatalogo, AppError> {
    match dati.chiedi(chiave, Domanda::CatalogoRiassunto, ATTESA_RIASSUNTO)? {
        Risposta::Dati(valore) => serde_json::from_value(valore)
            .map_err(|err| AppError::new(ErrorCode::LontanoProtocollo).with_cause(err.to_string())),
        _ => Err(AppError::new(ErrorCode::LontanoProtocollo)
            .with_cause("a un riassunto non si risponde con dei dati")),
    }
}

/// Le righe di un secchio di `chiave`, col secchio giusto.
fn chiedi_il_secchio<D: Dati>(
    dati: &D,
    chiave: &str,
    da_id: i64,
    a_id: i64,
) -> Result<PaginaCatalogo, AppError> {
    let pagina: PaginaCatalogo = match dati.chiedi(
        chiave,
        Domanda::CatalogoSecchio { da_id, a_id },
        ATTESA_SECCHIO,
    )? {
        Risposta::Dati(valore) => serde_json::from_value(valore).map_err(|err| {
            AppError::new(ErrorCode::LontanoProtocollo).with_cause(err.to_string())
        })?,
        _ => {
            return Err(AppError::new(ErrorCode::LontanoProtocollo)
                .with_cause("a un secchio non si risponde con dei dati"));
        }
    };
    if pagina.da_id != da_id || pagina.a_id != a_id {
        return Err(AppError::new(ErrorCode::LontanoProtocollo)
            .with_cause("un secchio che non è quello chiesto"));
    }
    Ok(pagina)
}

// ── la posta ────────────────────────────────────────────────────────────────

/// Manda a `chiave` la posta che aspetta, dalla più vecchia: un voto o un mi
/// piace (`valuta`) per volta. Si ferma alla prima che non parte — il
/// dispositivo non risponde — e la lascia lì per il giro dopo. Una voce che il
/// dispositivo rifiuta per sempre (il brano non c'è più, o una versione che
/// non la capisce da oltre [`SCADENZA_POSTA_MS`]) si butta. Quante ne ha
/// consegnate.
///
/// # Errori
///
/// Quelli della libreria; una voce che non parte non è un errore.
pub(super) fn manda_la_posta<D: Dati>(dati: &D, chiave: &str) -> Result<usize, AppError> {
    let voci = dati.leggi(|c| dispositivi::posta_da_mandare(c, chiave))?;
    let mut consegnate = 0;
    for voce in voci {
        if !dati.associato(chiave) {
            break;
        }
        let toglila =
            |dati: &D| dati.scrivi(|c| dispositivi::posta_consegnata(c, voce.id, voce.quando_ms));
        if voce.tipo != POSTA_VALUTA {
            // Il testo e il resto che arriveranno: li manda chi li conosce.
            continue;
        }
        let Some(valutazione) = Valutazione::da_corpo(&voce.corpo) else {
            // Una voce che non si legge non diventerà leggibile.
            toglila(dati)?;
            continue;
        };
        let domanda = Domanda::Valuta {
            brano: valutazione.brano,
            liked: valutazione.liked,
            rating: valutazione.rating,
            quando_ms: voce.quando_ms,
            chiave: format!("{}-{}", valutazione.brano, voce.quando_ms),
        };
        match dati.chiedi(chiave, domanda, ATTESA_POSTA) {
            Ok(_) => {
                toglila(dati)?;
                consegnate += 1;
            }
            Err(err) => match err.code().kind() {
                // Il brano non c'è più dall'altra parte: non c'è più niente da
                // dirgli.
                ErrorCodeKind::LontanoFileCambiato => {
                    toglila(dati)?;
                }
                // Una versione che non la capisce (o non lo dichiara ancora):
                // si tiene un po', poi si lascia.
                ErrorCodeKind::LontanoProtocollo => {
                    if adesso_ms().saturating_sub(voce.quando_ms) > SCADENZA_POSTA_MS {
                        toglila(dati)?;
                    }
                    break;
                }
                _ => break,
            },
        }
    }
    Ok(consegnate)
}

// ── i fili ──────────────────────────────────────────────────────────────────

/// I dispositivi con un giro in corso, e quelli che ne vogliono un altro
/// appena finisce.
static IN_CORSO: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
static RIPETI: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

fn presi<T>(m: &'static Mutex<T>) -> std::sync::MutexGuard<'static, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Fa partire un giro con `chiave` su un filo suo, se non ce n'è già uno (e se
/// c'è, ne vuole un altro appena finisce): la posta e il catalogo. Non
/// aspetta.
pub(super) fn richiedi(app: &AppHandle, chiave: &str) {
    if !presi(&IN_CORSO).insert(chiave.to_owned()) {
        presi(&RIPETI).insert(chiave.to_owned());
        return;
    }
    let app = app.clone();
    let suo = chiave.to_owned();
    let partito = std::thread::Builder::new()
        .name("rymembra-lontano-specchio".to_owned())
        .spawn(move || {
            loop {
                lavora(&app, &suo);
                if !presi(&RIPETI).remove(&suo) {
                    break;
                }
            }
            presi(&IN_CORSO).remove(&suo);
        });
    if let Err(err) = partito {
        presi(&IN_CORSO).remove(chiave);
        nota!("[lontano] il filo dello specchio non parte: {err}");
    }
}

/// Un giro intero con `chiave`: la posta prima — un voto dato qui non deve
/// perdere contro il catalogo di prima — poi il catalogo.
fn lavora(app: &AppHandle, chiave: &str) {
    let dati = match DallApp::nuovo(app) {
        Ok(dati) => dati,
        Err(err) => {
            nota!(
                "[lontano] lo specchio non parte codice={}",
                err.code().kind().code()
            );
            return;
        }
    };
    let consegnate = match manda_la_posta(&dati, chiave) {
        Ok(n) => n,
        Err(err) => {
            nota!(
                "[lontano] la posta in uscita non si legge codice={}",
                err.code().kind().code()
            );
            0
        }
    };
    if let Err(err) = giro(&dati, chiave) {
        if err.code().kind() != ErrorCodeKind::LontanoNonAssociato {
            nota!(
                "[lontano] un giro del catalogo non è riuscito codice={}",
                err.code().kind().code()
            );
            avanza(
                &dati,
                AvanzamentoSpecchio {
                    dispositivo: chiave.to_owned(),
                    fase: Fase::Errore,
                    errore: Some(codice(&err)),
                    righe: avanzamenti().get(chiave).map_or(0, |a| a.righe),
                    ..AvanzamentoSpecchio::default()
                },
            );
        }
    } else if consegnate > 0 {
        // Quel che si è mandato ha cambiato le righe del proprietario: si
        // rilegge il catalogo, perché i nostri voti tornino col suo orologio.
        presi(&RIPETI).insert(chiave.to_owned());
    }
}

/// Un dispositivo ha cominciato a rispondere: il suo catalogo, la sua posta, e
/// da qui un giro ogni [`OGNI`] finché risponde. Non aspetta.
pub(super) fn presente(app: &AppHandle, chiave: &str) {
    parte_il_ciclo(app);
    richiedi(app, chiave);
}

/// C'è posta nuova per questi dispositivi: si manda subito, se rispondono.
/// Non aspetta.
pub(crate) fn posta_nuova(app: &AppHandle, dispositivi: &[String]) {
    let Ok(comune) = comune(app) else {
        return;
    };
    for chiave in dispositivi {
        if comune.nodo().is_some() && super::filo::prendi(&comune.presenze).contains_key(chiave) {
            richiedi(app, chiave);
        }
    }
}

static CICLO: Once = Once::new();

/// Il filo che, ogni [`OGNI`], riguarda il catalogo di chi risponde.
fn parte_il_ciclo(app: &AppHandle) {
    CICLO.call_once(|| {
        let app = app.clone();
        let partito = std::thread::Builder::new()
            .name("rymembra-lontano-ciclo".to_owned())
            .spawn(move || {
                loop {
                    std::thread::sleep(OGNI);
                    let Ok(comune) = comune(&app) else {
                        continue;
                    };
                    if comune.nodo().is_none() {
                        continue;
                    }
                    let presenti: Vec<String> = super::filo::prendi(&comune.presenze)
                        .keys()
                        .cloned()
                        .collect();
                    for chiave in presenti {
                        richiedi(&app, &chiave);
                    }
                }
            });
        if let Err(err) = partito {
            nota!("[lontano] il ciclo dello specchio non parte: {err}");
        }
    });
}

// ── le copertine ────────────────────────────────────────────────────────────

/// Quanti posti ha preso ogni dispositivo.
static POSTI: Mutex<BTreeMap<String, usize>> = Mutex::new(BTreeMap::new());
static UN_POSTO: std::sync::Condvar = std::sync::Condvar::new();

/// Un posto fra i [`COPERTINE_PER_DISPOSITIVO`] di un dispositivo: lasciato,
/// sveglia chi aspetta.
struct Posto(String);

impl Posto {
    fn prendi(dispositivo: &str, entro: Duration) -> Option<Self> {
        let posti = POSTI.lock().unwrap_or_else(PoisonError::into_inner);
        let (mut posti, _) = UN_POSTO
            .wait_timeout_while(posti, entro, |p| {
                p.get(dispositivo).copied().unwrap_or(0) >= COPERTINE_PER_DISPOSITIVO
            })
            .unwrap_or_else(PoisonError::into_inner);
        let presi = posti.entry(dispositivo.to_owned()).or_insert(0);
        if *presi >= COPERTINE_PER_DISPOSITIVO {
            return None;
        }
        *presi += 1;
        Some(Self(dispositivo.to_owned()))
    }
}

impl Drop for Posto {
    fn drop(&mut self) {
        let mut posti = POSTI.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(n) = posti.get_mut(&self.0) {
            *n = n.saturating_sub(1);
        }
        UN_POSTO.notify_one();
    }
}

/// Le copertine che un dispositivo ha detto di non avere, e da quando.
static ASSENTI: Mutex<BTreeMap<String, Instant>> = Mutex::new(BTreeMap::new());

/// La copertina di una riga specchio che qui non c'è ancora: la si chiede a un
/// dispositivo che risponde e ha una riga con quell'impronta, e si salva nello
/// store **sotto quell'impronta** (`CoverStore::salva_estranea`). Da lì il
/// provider, la notifica, Android Auto e l'albero di Android Auto la trovano
/// come ogni altra. `Ok(None)`: nessun dispositivo che risponde la ha. Si
/// chiama su un filo del pool bloccante.
///
/// # Errori
///
/// `lontano.*` dalla rete; `metadata.tagReadFailed` per byte che non sono una
/// copertina nei limiti.
pub(crate) fn copertina_di_una_riga(
    app: &AppHandle,
    impronta: &str,
) -> Result<Option<Vec<u8>>, AppError> {
    if !rymembra_app::covers::e_un_impronta(impronta) || impronta.len() < 16 {
        return Ok(None);
    }
    if presi(&ASSENTI)
        .get(impronta)
        .is_some_and(|quando| quando.elapsed() < MEMORIA_ASSENTI)
    {
        return Ok(None);
    }
    let comune = comune(app)?;
    let presenti: HashSet<String> = super::filo::prendi(&comune.presenze)
        .keys()
        .cloned()
        .collect();
    let Some(dispositivo) = leggi_a_parte(app, |c| {
        archivio::dispositivo_della_copertina(c, impronta, &|d| presenti.contains(d))
    })?
    else {
        return Ok(None);
    };
    let _posto = Posto::prendi(&dispositivo, ATTESA_COPERTINA).ok_or_else(|| {
        AppError::new(ErrorCode::NetTimeout {
            url: None,
            timeout_ms: u64::try_from(ATTESA_COPERTINA.as_millis()).ok(),
        })
    })?;
    let stato = app.try_state::<Stato>().ok_or_else(|| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some("lontano".to_owned()),
        })
    })?;
    let copertine = stato.copertine()?;
    // Mentre si aspettava il posto, un'altra richiesta può averla portata.
    if copertine.path_for(impronta).exists() {
        return copertine.media(impronta);
    }
    let dati = DallApp::nuovo(app)?;
    let risposta = dati.chiedi(
        &dispositivo,
        Domanda::Copertina {
            impronta: impronta.to_owned(),
        },
        ATTESA_COPERTINA,
    )?;
    let Risposta::Byte { byte, .. } = risposta else {
        presi(&ASSENTI).insert(impronta.to_owned(), Instant::now());
        return Ok(None);
    };
    if byte.is_empty() {
        presi(&ASSENTI).insert(impronta.to_owned(), Instant::now());
        return Ok(None);
    }
    // Solo un'immagine, riconosciuta dai suoi byte, e nei limiti dello store.
    if crate::copertine::tipo_immagine(&byte).is_none() {
        nota!("[lontano] una copertina di un altro dispositivo non è un'immagine: non si tiene");
        return Err(AppError::new(ErrorCode::LontanoProtocollo)
            .with_cause("una copertina che non è un'immagine"));
    }
    let salvata = copertine.salva_estranea(impronta, &byte)?;
    scrivi_in_libreria(app, |c| {
        archivio::copertina_arrivata(
            c,
            impronta,
            salvata.width,
            salvata.height,
            salvata.byte_size,
        )
        .map(|_| ())
    })?;
    copertine.media(impronta)
}

// ── la riproduzione ─────────────────────────────────────────────────────────

/// La riga da suonare al posto di `track_id`: la copia migliore del brano fra
/// quelle che adesso si possono usare — un dispositivo che risponde, e «Dati
/// mobili» che lo consente. Vedi `rymembra_app::specchio::scelta`. Una riga
/// senza dispositivi di mezzo torna com'è, senza leggere altro.
///
/// # Errori
///
/// `lontano.irraggiungibile`, `lontano.reteAConsumo`; quelli della libreria.
pub(crate) fn scegli_la_copia(
    app: &AppHandle,
    stato: &Stato,
    track_id: i64,
) -> Result<i64, AppError> {
    let comune = comune(app)?;
    let presenti: HashSet<String> = super::filo::prendi(&comune.presenze)
        .keys()
        .cloned()
        .collect();
    let rete = super::sfoglia::rete_di_adesso(&comune);
    crate::stato::con_libreria(stato, |libreria| {
        rymembra_app::specchio::scelta::scegli_con_la_rete(
            &libreria.connection,
            track_id,
            &presenti,
            rete,
        )
    })
}

/// I byte di una riga specchio: la sorgente a blocchi sul nodo del suo
/// dispositivo, come per Sfoglia, con il cancello di «Dati mobili» dentro.
pub(crate) fn apri_riga(
    app: &AppHandle,
    riga: &rymembra_app::specchio::scelta::RigaRemota,
) -> Result<crate::riproduzione::insieme::ApriInRete, AppError> {
    let comune = comune(app)?;
    if !comune.associato(&riga.dispositivo) {
        return Err(AppError::new(ErrorCode::LontanoNonAssociato));
    }
    Ok(super::sfoglia::apri_in_rete(
        &comune,
        &riga.dispositivo,
        riga.id_remoto,
    ))
}

// ── i comandi ───────────────────────────────────────────────────────────────

/// L'ultimo avanzamento di ogni dispositivo, per la finestra che arriva dopo.
#[tauri::command]
pub fn lontano_specchio_stato() -> Esito<Vec<AvanzamentoSpecchio>> {
    Ok(avanzamenti().values().cloned().collect())
}

/// Rilegge adesso il catalogo di un dispositivo (e gli manda la posta). Non
/// aspetta il giro.
#[tauri::command]
pub fn lontano_specchio_ora(app: AppHandle, chiave: String) -> Esito<()> {
    let comune = comune(&app).map_err(crate::errore::errore)?;
    if !comune.associato(&chiave) {
        return Err(crate::errore::errore(AppError::new(
            ErrorCode::LontanoNonAssociato,
        )));
    }
    richiedi(&app, &chiave);
    Ok(())
}

/// La persona ha detto di sì: le righe che il dispositivo non manda più se ne
/// vanno. Quante.
#[tauri::command]
pub async fn lontano_specchio_conferma(app: AppHandle, chiave: String) -> Esito<usize> {
    in_disparte(move || {
        let tolte = scrivi_in_libreria(&app, |c| archivio::conferma_rimozioni(c, &chiave))?;
        let dati = DallApp::nuovo(&app)?;
        let mut a = avanzamenti().get(&chiave).cloned().unwrap_or_default();
        a.dispositivo = chiave.clone();
        a.in_attesa = 0;
        a.righe = leggi_a_parte(&app, |c| archivio::stato(c, &chiave)).map_or(0, |s| s.righe);
        avanza(&dati, a);
        Ok(tolte)
    })
    .await
}

/// Cosa succederebbe alle playlist dimenticando un dispositivo: per chiedere
/// conferma dicendo quante playlist perdono brani.
#[tauri::command]
pub async fn lontano_dimentica_anteprima(app: AppHandle, chiave: String) -> Esito<Anteprima> {
    in_disparte(move || leggi_a_parte(&app, |c| archivio::anteprima_dimentica(c, &chiave))).await
}

/// Il dispositivo è stato dimenticato: il suo avanzamento non c'è più.
pub(super) fn dimenticato(chiave: &str) {
    avanzamenti().remove(chiave);
    presi(&RIPETI).remove(chiave);
}

#[cfg(test)]
mod prove {
    //! Lo specchio fra due nodi veri in memoria (solo `127.0.0.1`, niente
    //! relay): il PC serve il suo catalogo con le stesse funzioni di
    //! `servizio`, il telefono lo porta nella sua libreria, i voti tornano
    //! indietro dalla posta, e col PC spento si ripiega sulla copia di qui.

    use std::collections::HashSet;
    use std::sync::Mutex;

    use rymembra_app::dispositivi::POSTA_VALUTA;
    use rymembra_app::library::condivisa;
    use rymembra_app::specchio::scelta::{Rete as ReteProva, scegli_con_la_rete};
    use rymembra_lontano::{
        Evento, Identita, Impostazioni, Nodo, Ospite, Presentazione, Presenza, Rete as ReteNodo,
    };

    use super::*;

    /// Il PC: un'app finta che risponde dal suo database come `servizio`.
    struct Pc {
        db: Mutex<rusqlite::Connection>,
        associati: Mutex<HashSet<String>>,
    }

    impl Ospite for Pc {
        fn associato(&self, chiave: &str) -> bool {
            self.associati.lock().unwrap().contains(chiave)
        }

        fn conferma(&self, _chiave: &str, _chi: &Presentazione) -> bool {
            true
        }

        fn nuovo_pari(
            &self,
            chiave: &str,
            _chi: &Presentazione,
            _indirizzi: &[std::net::SocketAddr],
        ) {
            self.associati.lock().unwrap().insert(chiave.to_owned());
        }

        fn rispondi(&self, _da: &str, domanda: Domanda) -> Result<Risposta, AppError> {
            let db = self.db.lock().unwrap();
            let dati = |v: serde_json::Result<serde_json::Value>| {
                v.map(Risposta::Dati).map_err(|e| {
                    AppError::new(ErrorCode::LontanoProtocollo).with_cause(e.to_string())
                })
            };
            match domanda {
                Domanda::CatalogoRiassunto => {
                    dati(serde_json::to_value(&*condivisa::riassunto_catalogo(&db)?))
                }
                Domanda::CatalogoSecchio { da_id, a_id } => dati(serde_json::to_value(
                    condivisa::pagina_catalogo(&db, da_id, a_id)?,
                )),
                Domanda::Valuta {
                    brano,
                    liked,
                    rating,
                    quando_ms,
                    ..
                } => {
                    condivisa::valuta(&db, brano, liked, rating, quando_ms, adesso_ms())?;
                    Ok(Risposta::Fatto)
                }
                _ => Err(AppError::new(ErrorCode::LontanoProtocollo)),
            }
        }

        fn dimenticato(&self, _da: &str) {}

        fn evento(&self, _evento: Evento) {}
    }

    /// Il telefono, senza l'app: lo stesso `Dati` con un nodo e un database.
    struct Telefono {
        nodo: Arc<Nodo>,
        pc: String,
        db: Mutex<rusqlite::Connection>,
        emessi: Mutex<Vec<AvanzamentoSpecchio>>,
    }

    impl Dati for Telefono {
        fn associato(&self, chiave: &str) -> bool {
            chiave == self.pc
        }

        fn dichiara_catalogo(&self, chiave: &str) -> bool {
            self.nodo.dichiara(chiave, CAPACITA_CATALOGO)
        }

        fn chiedi(
            &self,
            chiave: &str,
            domanda: Domanda,
            scadenza: Duration,
        ) -> Result<Risposta, AppError> {
            self.nodo.chiedi(chiave, domanda, scadenza)
        }

        fn leggi<T>(
            &self,
            azione: impl FnOnce(&rusqlite::Connection) -> Result<T, AppError>,
        ) -> Result<T, AppError> {
            azione(&self.db.lock().unwrap())
        }

        fn scrivi<T>(
            &self,
            azione: impl FnOnce(&rusqlite::Connection) -> Result<T, AppError>,
        ) -> Result<T, AppError> {
            azione(&self.db.lock().unwrap())
        }

        fn emetti(&self, avanzamento: &AvanzamentoSpecchio) {
            self.emessi.lock().unwrap().push(avanzamento.clone());
        }
    }

    fn nodo(nome: &str, capacita: &[&str], ospite: Arc<dyn Ospite>) -> Arc<Nodo> {
        Arc::new(
            Nodo::avvia(
                Impostazioni {
                    identita: Identita::nuova(),
                    rete: ReteNodo::SoloQuesta,
                    io: Presentazione {
                        nome: nome.to_owned(),
                        piattaforma: "prova".to_owned(),
                        dispositivo: format!("d-{nome}"),
                        chiave_insieme: None,
                    },
                    presenza: Presenza {
                        nome: nome.to_owned(),
                        capacita: capacita.iter().map(|c| (*c).to_owned()).collect(),
                        ..Presenza::default()
                    },
                },
                ospite,
            )
            .expect("il nodo parte"),
        )
    }

    fn libreria() -> rusqlite::Connection {
        rymembra_app::db::open_in_memory()
            .expect("database")
            .connection
    }

    /// `quanti` brani nella libreria del PC, con gli id da 1.
    fn brani_del_pc(c: &rusqlite::Connection, quanti: i64) {
        let tx = c.unchecked_transaction().unwrap();
        for id in 1..=quanti {
            tx.execute(
                "INSERT INTO tracks (id, path, track_key, content_key, title, artist, album,
                                     album_key, duration_ms, file_size, date_added, date_modified,
                                     codec, bitrate, sample_rate)
                 VALUES (?1, ?2, ?3, ?3, ?4, 'Artista', 'Album', 'album-del-pc', 180000, 1000,
                         1, 1, 'Flac', 900, 96000)",
                rusqlite::params![
                    id,
                    format!("C:/Musica/{id}.flac"),
                    format!("artista|titolo {id}|album"),
                    format!("Titolo {id}")
                ],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }

    fn quante(c: &rusqlite::Connection, sql: &str) -> i64 {
        c.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    /// PC e telefono collegati, con la presenza scambiata.
    fn collegati(brani: i64) -> (Arc<Nodo>, Arc<Pc>, Telefono) {
        let pc = Arc::new(Pc {
            db: Mutex::new(libreria()),
            associati: Mutex::new(HashSet::new()),
        });
        brani_del_pc(&pc.db.lock().unwrap(), brani);
        let nodo_pc = nodo(
            "PC",
            &[CAPACITA_CATALOGO],
            Arc::clone(&pc) as Arc<dyn Ospite>,
        );
        let nodo_telefono = nodo(
            "Telefono",
            &[CAPACITA_CATALOGO],
            Arc::new(Pc {
                db: Mutex::new(libreria()),
                associati: Mutex::new(HashSet::new()),
            }),
        );
        let invito = nodo_pc.invita().unwrap();
        nodo_telefono.collega(&invito.link()).unwrap();
        // Si conoscono: il telefono chiede al PC chi è, e dunque cosa sa fare.
        let chiave_pc = nodo_pc.chiave();
        nodo_telefono
            .chiedi(&chiave_pc, Domanda::Presenza, Duration::from_secs(10))
            .unwrap();
        let telefono = Telefono {
            nodo: nodo_telefono,
            pc: chiave_pc,
            db: Mutex::new(libreria()),
            emessi: Mutex::new(Vec::new()),
        };
        (nodo_pc, pc, telefono)
    }

    #[test]
    fn il_catalogo_del_pc_entra_nella_libreria_del_telefono_e_poi_solo_le_differenze() {
        let (_nodo_pc, pc, telefono) = collegati(1_200);
        let chiave = telefono.pc.clone();

        // Il primo giro: tre secchi, 1.200 righe, con l'avanzamento.
        let esito = giro(&telefono, &chiave).expect("il giro");
        assert_eq!(
            esito,
            EsitoGiro {
                lavorato: true,
                completo: true,
                righe: 1_200,
                in_attesa: 0
            }
        );
        {
            let db = telefono.db.lock().unwrap();
            assert_eq!(quante(&db, "SELECT COUNT(*) FROM tracks"), 1_200);
            assert_eq!(
                quante(
                    &db,
                    "SELECT COUNT(*) FROM tracks WHERE path LIKE 'lontano://%'"
                ),
                1_200
            );
            assert_eq!(quante(&db, "SELECT COUNT(*) FROM lontano_secchi"), 3);
            assert_eq!(quante(&db, "SELECT COUNT(*) FROM albums"), 1);
        }
        let emessi = telefono.emessi.lock().unwrap().clone();
        assert_eq!(emessi.first().map(|a| a.fase), Some(Fase::Leggo));
        assert_eq!(
            emessi.last().map(|a| (a.fase, a.righe)),
            Some((Fase::Fine, 1_200))
        );
        assert!(
            emessi
                .iter()
                .any(|a| a.fase == Fase::Leggo && a.fatti > 0 && a.totali == 1_200),
            "«Leggo la libreria… 12.000/50.000»: {emessi:?}"
        );

        // Niente è cambiato: una domanda sola, niente da rileggere.
        let esito = giro(&telefono, &chiave).expect("il giro dopo");
        assert!(!esito.lavorato && esito.completo);

        // Un ascolto sul PC cambia un brano del secchio di mezzo: si rilegge
        // quel secchio e basta.
        pc.db
            .lock()
            .unwrap()
            .execute(
                "UPDATE tracks SET play_count = 7, last_played_at = 100, stats_updated_at = 100 WHERE id = 600",
                [],
            )
            .unwrap();
        let prima = telefono.emessi.lock().unwrap().len();
        let esito = giro(&telefono, &chiave).expect("il giro del cambiamento");
        assert!(esito.lavorato && esito.completo);
        let leggo: Vec<i64> = telefono.emessi.lock().unwrap()[prima..]
            .iter()
            .filter(|a| a.fase == Fase::Leggo)
            .map(|a| a.totali)
            .collect();
        assert_eq!(leggo.first(), Some(&500), "un secchio solo: {leggo:?}");
        let db = telefono.db.lock().unwrap();
        assert_eq!(
            quante(&db, "SELECT play_count FROM tracks WHERE path LIKE '%/600'"),
            7
        );
    }

    #[test]
    fn un_voto_dato_sul_telefono_arriva_al_pc_dalla_posta_e_torna_nel_catalogo() {
        let (_nodo_pc, pc, telefono) = collegati(40);
        let chiave = telefono.pc.clone();
        giro(&telefono, &chiave).expect("il primo giro");
        let id = {
            let db = telefono.db.lock().unwrap();
            quante(&db, "SELECT id FROM tracks WHERE path LIKE '%/7'")
        };
        // Il gesto: come `preferito` e `valutazione`.
        {
            let db = telefono.db.lock().unwrap();
            let adesso = adesso_ms();
            db.execute(
                "UPDATE tracks SET liked = 1, liked_at = ?2, stats_updated_at = ?2, rating = 4,
                                   rating_at = ?2 WHERE id = ?1",
                rusqlite::params![id, adesso],
            )
            .unwrap();
            let svegliati = rymembra_app::specchio::valutazioni::dai_al_gruppo(
                &db,
                id,
                Some(true),
                Some(4),
                adesso,
            )
            .unwrap();
            assert_eq!(svegliati, vec![chiave.clone()]);
            assert_eq!(
                dispositivi::posta_da_mandare(&db, &chiave).unwrap().len(),
                1,
                "in posta, finché il PC non l'ha"
            );
        }
        assert_eq!(manda_la_posta(&telefono, &chiave).unwrap(), 1);
        {
            let db = telefono.db.lock().unwrap();
            assert!(
                dispositivi::posta_da_mandare(&db, &chiave)
                    .unwrap()
                    .is_empty()
            );
        }
        let (liked, rating): (i64, i64) = pc
            .db
            .lock()
            .unwrap()
            .query_row("SELECT liked, rating FROM tracks WHERE id = 7", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((liked, rating), (1, 4), "scritto dal proprietario");
        // Una seconda consegna della stessa voce è innocua.
        dispositivi::metti_in_posta(
            &telefono.db.lock().unwrap(),
            &chiave,
            POSTA_VALUTA,
            "7",
            r#"{"brano":7,"liked":true,"rating":4}"#,
            1,
        )
        .unwrap();
        assert_eq!(manda_la_posta(&telefono, &chiave).unwrap(), 1);
        // E il catalogo dopo porta il suo orologio e le stesse stelle.
        giro(&telefono, &chiave).expect("il giro dopo");
        let db = telefono.db.lock().unwrap();
        assert_eq!(
            quante(&db, &format!("SELECT rating FROM tracks WHERE id = {id}")),
            4
        );
    }

    #[test]
    fn col_pc_spento_il_giro_non_rompe_niente_e_si_suona_la_copia_di_qui() {
        let (nodo_pc, _pc, telefono) = collegati(30);
        let chiave = telefono.pc.clone();
        giro(&telefono, &chiave).expect("il primo giro");
        // Sul telefono c'è anche la copia locale di un brano, in mp3.
        let (remota, locale) = {
            let db = telefono.db.lock().unwrap();
            db.execute(
                "INSERT INTO tracks (path, track_key, content_key, title, artist, album, file_size,
                                     date_added, date_modified, codec, bitrate, sample_rate)
                 VALUES ('/storage/Musica/5.mp3', 'artista|titolo 5|album', 'artista|titolo 5|album',
                         'Titolo 5', 'Artista', 'Album', 1000, 1, 1, 'Mpeg', 320, 44100)",
                [],
            )
            .unwrap();
            let locale = db.last_insert_rowid();
            rymembra_app::specchio::archivio::ricalcola_doppioni(&db).unwrap();
            let remota = quante(&db, "SELECT id FROM tracks WHERE path LIKE '%/5'");
            (remota, locale)
        };
        {
            let db = telefono.db.lock().unwrap();
            assert_eq!(
                quante(
                    &db,
                    &format!("SELECT copia_di FROM tracks WHERE id = {locale}")
                ),
                remota,
                "la locale sta dietro la flac del PC"
            );
            // Col PC che risponde si suona la sua copia, migliore.
            let presenti = HashSet::from([chiave.clone()]);
            assert_eq!(
                scegli_con_la_rete(&db, locale, &presenti, ReteProva::Libera).unwrap(),
                remota
            );
        }
        // Il PC si spegne.
        nodo_pc.chiudi();
        drop(nodo_pc);
        let errore = giro(&telefono, &chiave);
        assert!(errore.is_err(), "un PC spento non risponde");
        let db = telefono.db.lock().unwrap();
        assert_eq!(
            quante(
                &db,
                "SELECT COUNT(*) FROM tracks WHERE path LIKE 'lontano://%'"
            ),
            30,
            "le righe restano, per il grigio"
        );
        // Ripiego sulla copia locale.
        assert_eq!(
            scegli_con_la_rete(&db, remota, &HashSet::new(), ReteProva::Libera).unwrap(),
            locale
        );
        assert_eq!(
            scegli_con_la_rete(&db, locale, &HashSet::new(), ReteProva::Libera).unwrap(),
            locale
        );
    }

    #[test]
    fn a_un_pari_di_prima_non_si_chiede_il_catalogo() {
        // Sul telefono e sul PC dell'utente gira una build che non dichiara
        // `catalogo`: lo specchio lo dice `nonSupportato` e non rompe niente.
        let pc = Arc::new(Pc {
            db: Mutex::new(libreria()),
            associati: Mutex::new(HashSet::new()),
        });
        let nodo_pc = nodo("PC", &[], Arc::clone(&pc) as Arc<dyn Ospite>);
        let nodo_telefono = nodo(
            "Telefono",
            &[CAPACITA_CATALOGO],
            Arc::new(Pc {
                db: Mutex::new(libreria()),
                associati: Mutex::new(HashSet::new()),
            }),
        );
        nodo_telefono
            .collega(&nodo_pc.invita().unwrap().link())
            .unwrap();
        nodo_telefono
            .chiedi(
                &nodo_pc.chiave(),
                Domanda::Presenza,
                Duration::from_secs(10),
            )
            .unwrap();
        let telefono = Telefono {
            nodo: nodo_telefono,
            pc: nodo_pc.chiave(),
            db: Mutex::new(libreria()),
            emessi: Mutex::new(Vec::new()),
        };
        let chiave = telefono.pc.clone();
        assert_eq!(giro(&telefono, &chiave).unwrap(), EsitoGiro::default());
        assert_eq!(
            telefono.emessi.lock().unwrap().last().map(|a| a.fase),
            Some(Fase::NonSupportato)
        );
        assert_eq!(
            quante(&telefono.db.lock().unwrap(), "SELECT COUNT(*) FROM tracks"),
            0
        );
        // E la posta, che al PC di prima non si può mandare, aspetta.
        {
            let db = telefono.db.lock().unwrap();
            dispositivi::metti_in_posta(
                &db,
                &chiave,
                POSTA_VALUTA,
                "7",
                r#"{"brano":7,"liked":true}"#,
                adesso_ms(),
            )
            .unwrap();
        }
        assert_eq!(manda_la_posta(&telefono, &chiave).unwrap(), 0);
        assert_eq!(
            dispositivi::posta_da_mandare(&telefono.db.lock().unwrap(), &chiave)
                .unwrap()
                .len(),
            1
        );
    }
}
