//! Il ponte fra la sessione e il lettore: l'unico punto che tocca `Lettore`
//! per conto di `crate::insieme`.
//!
//! # Il lucchetto
//!
//! Le funzioni `pub` di qui, quelle che chiama `crate::insieme`, il lucchetto
//! del lettore, quando serve, lo prendono da sé, per il tempo di una fotografia
//! o di un comando al motore, e **mai** mentre aprono un brano: aprire una voce
//! che sta sul disco di un altro PC può aspettare la rete, e la finestra non
//! deve aspettare con lei. Nessuna di loro si chiama col lucchetto del lettore
//! già in mano, e quelle di [`brani`] il lettore non lo toccano affatto.
//!
//! Quelle che `riproduzione` chiama dentro i suoi gesti — le regole di
//! [`regole`], [`tenute_dal_lettore`], [`dimentica_le_uscite`] — ricevono
//! invece il `Lettore` che il gesto ha già in mano. Il suo lucchetto non lo
//! prendono, non aprono brani e non aspettano né il disco né la rete: i soli
//! lucchetti che toccano vengono dopo quello del lettore, il registro delle
//! voci (`ospiti`, una foglia) e quello di `ripresa`.
//!
//! Le aperture seguono quindi i tre tempi di `avvia_corrente`: si decide sotto
//! lucchetto, si apre **fuori**, si consegna sotto lucchetto controllando che
//! nel frattempo niente sia cambiato. Il database lo stesso: le voci di chi
//! ospita si costruiscono dalla libreria a lettore lasciato.
//!
//! # Le regole di sessione, e perché stanno in memoria
//!
//! In sessione il lettore si comporta diversamente: dissolvenza a zero (due
//! PC che sfumano in modo diverso non sono più a tempo), niente autoplay né
//! radio (sceglierebbero per tutti dalla cronologia di uno), niente shuffle
//! (l'ordine è quello della coda, uguale per tutti), niente timer (fermerebbe
//! tutti), e la coda non si scrive sul disco. Tutto questo vive in
//! [`Sessione`], dentro il lettore, e si legge da [`dissolvenza_ms`],
//! [`autoplay`] e [`coda_si_scrive`]: le preferenze scritte nelle impostazioni
//! **non si toccano mai**. Se l'app cade a metà sessione, al riavvio ritrova la
//! dissolvenza, l'autoplay e la coda dell'utente esattamente com'erano; uscendo
//! dalla sessione, il lettore torna a leggerle da dove le aveva sempre lette.
//!
//! # Chi ospita parte fra un quarto di secondo
//!
//! La ripresa, il salto e il brano nuovo di chi ospita non partono subito ma
//! [`ANTICIPO_NS`] più in là, con `Motore::riprendi_a`, e l'ancora annuncia il
//! punto e l'istante **prima** che succedano. Gli altri hanno così il tempo di
//! aprire la stessa voce e partono nello stesso istante, invece di arrivare
//! dopo e rincorrere a salti. La pausa invece è immediata: i membri si fermano
//! quando arriva l'ancora, e i pochi millisecondi li toglie la ripresa dopo.

mod brani;
mod regole;

use std::sync::atomic::Ordering;

use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::queue::{Queue, QueueSnapshot};
use rymembra_insieme::messaggi::MASSIMO_VOCI;
use rymembra_play::sincronia::regolatore::{DaMe, Mossa};
use rymembra_play::{RifiutoAvvio, tempo};
use tauri::{Manager as _, State};

use crate::nota;
use crate::stato::{adesso_ms, con_libreria};

use super::coda::{gesto_di_coda, salva_coda};
use super::fili::prepara_prossimo;
use super::ospiti::{self, Chi, MANIGLIA_CALIBRAZIONE};
use super::{APERTURA_FLUSSO, flusso, in_ordine, libreria_di};
use super::{Lettore, StatoLettore, chiudi_ascolto, con_lettore, manda_stato, riapertura};

use brani::voci_allineate;
use regole::{ARRIVATO_ENTRO_MS, finestra, quante_entrano, ripulita};

pub use super::ospiti::{Apertura, ApriInRete, SchedaVoce, Voce};

/// Il confronto col segno degli id, per chi sta fuori da `riproduzione`: vedi
/// il `//!` di `ospiti`. `ospiti` non si vede da fuori, il ponte sì.
pub use super::ospiti::e_di_libreria;

pub use brani::{BranoDiQuesto, FileDellaVoce, Mancanza};
pub use brani::{brani_di_questo, voci_da_libreria, voci_di_questo};

pub(super) use regole::{autoplay, brani_da_mettere, cambiato, coda_si_scrive};
pub(super) use regole::{corrente_da_mostrare, cursore_sulla_voce, dimentica_voci};
pub(super) use regole::{dissolvenza_ms, dopo_un_avvio, dopo_una_pausa, e_la_calibrazione};
pub(super) use regole::{entrano, finestra_di_sessione, in_calibrazione, latenza_cambiata};
pub(super) use regole::{lista_da_suonare, motore_sostituito, ospita, parti};
pub(super) use regole::{posizione_da_mostrare, righe_delle_voci, riprendi, salta, segue};

/// L'`uid` riservato alla voce di calibrazione: clic identici, generati uguali
/// su ogni PC, nessun byte in rete. Non sta mai in coda.
pub const UID_CALIBRAZIONE: u64 = u64::MAX;

/// La coda della sessione com'è adesso nel lettore di questo PC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Istantanea {
    /// Gli `uid`, nell'ordine in cui suoneranno.
    pub coda: Vec<u64>,
    /// La voce corrente.
    pub corrente: Option<u64>,
    /// In pausa.
    pub in_pausa: bool,
    /// Come si ripete.
    pub ripeti: rymembra_domain::queue::RepeatMode,
}

/// L'ancora di chi ospita, sul suo orologio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AncoraDiQuesto {
    /// L'`uid` della voce.
    pub etichetta: u64,
    /// Il punto, in µs dall'inizio della voce.
    pub posizione_us: u64,
    /// L'istante in cui si sente, ns su `rymembra_play::tempo`.
    pub udito_ns: u64,
    /// Sta suonando (o partirà a `udito_ns`, se è nel futuro).
    pub in_corso: bool,
    /// È l'avvio programmato di una ripresa, un salto o una voce nuova,
    /// annunciato prima di sentirsi: vedi `AncoraSulFilo::programmata`.
    pub programmata: bool,
}

// ── la sessione dentro il lettore ───────────────────────────────────────────

/// Quanto prima chi ospita annuncia una ripresa, un salto o un brano nuovo.
///
/// Un quarto di secondo, lo stesso anticipo con cui il regolatore dei membri
/// programma le sue riaperture (`Taratura::anticipo_ns`): è il tempo che serve
/// ad aprire una voce in rete e far partire il primo blocco. Di più si
/// sentirebbe come un tasto che risponde tardi; di meno e i membri
/// arriverebbero già in ritardo, cioè con un salto. Se la misura della
/// riapertura in rete dice che non basta, è questo il numero da alzare.
pub(super) const ANTICIPO_NS: u64 = 250_000_000;

/// Fino a quando un avvio programmato vale come ancora, se il motore non ha
/// ancora detto niente di meglio: due secondi dopo l'istante. Oltre, qualcosa
/// non è partito — il flusso, il file — e l'ancora vera del motore dice il
/// vero meglio di una promessa scaduta.
const SCADENZA_PROGRAMMATA_NS: u64 = 2_000_000_000;

/// Il ruolo di questo lettore nella sessione.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Ruolo {
    /// Ospita: la sua coda è la sessione.
    Ospita,
    /// Segue: la sua coda è lo specchio di quella dell'host.
    Segue,
}

/// La sessione, dentro il lettore. Vedi `Lettore::sessione`.
#[derive(Debug)]
pub(super) struct Sessione {
    ruolo: Ruolo,
    /// La coda dell'utente com'era all'ingresso, da rimettere all'uscita.
    prima: CodaDiPrima,
    /// Chi ospita: l'avvio programmato annunciato e non ancora arrivato.
    programmata: Option<Programmata>,
    /// Chi ospita: la calibrazione accesa, e quel che suonava prima.
    calibrazione: Option<PrimaDeiClic>,
    /// Chi segue: la pausa della sessione com'era nell'ultimo stato
    /// dell'host. È quella che decide il tasto play/pausa: il motore di qui
    /// può essere fermo per altre ragioni (una voce che si sta aprendo, un
    /// regolatore che riapre), e un «riprendi» deciso dal motore di qui
    /// arriverebbe a un host che suona già.
    pausa_dell_host: Option<bool>,
}

impl Sessione {
    fn nuova(ruolo: Ruolo, prima: CodaDiPrima) -> Self {
        Self {
            ruolo,
            prima,
            programmata: None,
            calibrazione: None,
            pausa_dell_host: None,
        }
    }
}

/// La coda dell'utente all'ingresso: rimessa all'uscita, ferma, al punto in
/// cui era. In memoria e basta: sul disco c'è già, perché in sessione la coda
/// non si scrive.
#[derive(Debug)]
struct CodaDiPrima {
    istantanea: QueueSnapshot,
    curata: bool,
    /// A che punto del brano si era.
    ms: u64,
    /// Il motore aveva un brano in mano: se no, all'uscita non c'è un punto
    /// da ritrovare.
    avviata: bool,
}

/// Un avvio programmato: questo punto di questa voce si sentirà a
/// quell'istante.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Programmata {
    /// La voce, come la nomina la coda di questo PC: serve a mostrare il punto
    /// promesso sulla voce giusta. Vedi [`posizione_da_mostrare`].
    maniglia: Option<i64>,
    /// L'`uid`, per l'ancora: senza, l'annuncio non si può fare, e l'ancora
    /// aspetta quella vera del motore.
    etichetta: Option<u64>,
    posizione_us: u64,
    quando_ns: u64,
}

/// Quel che suonava quando la calibrazione si è accesa.
#[derive(Debug, Clone, Copy)]
struct PrimaDeiClic {
    maniglia: Option<i64>,
    ms: u64,
    suonava: bool,
}

// ── ingresso e uscita ───────────────────────────────────────────────────────

/// Il lettore, o `internal.aborted`.
fn stato_lettore(app: &tauri::AppHandle) -> Result<State<'_, StatoLettore>, AppError> {
    app.try_state::<StatoLettore>().ok_or_else(|| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some("lettore in sessione".to_owned()),
        })
        .with_cause("il lettore non è più fra gli stati gestiti")
    })
}

fn gia_in_sessione() -> AppError {
    AppError::new(ErrorCode::InsiemeGiaInSessione)
}

fn non_disponibile(gesto: &str) -> AppError {
    AppError::new(ErrorCode::InsiemeGestoNonDisponibile {
        gesto: gesto.to_owned(),
    })
}

/// Le regole di sessione sul motore, e i segni di prima che non valgono più.
/// Si chiama col lucchetto del lettore in mano, entrando.
fn accendi_le_regole(stato: &StatoLettore, lettore: &mut Lettore) {
    lettore.motore.dissolvenza(dissolvenza_ms(lettore));
    // Il flusso invece riposa come sempre, anche in sessione: un'uscita
    // Bluetooth in pausa deve tornare libera. Chi programma una ripresa lo
    // sveglia prima: vedi [`programma`] e `Motore::riprendi_a`.
    lettore.motore.deriva(0);
    // Il timer si spegne entrando, e la strada lo rifiuta durante: un timer
    // che scade sull'host fermerebbe tutti.
    stato.spegnimento.store(0, Ordering::Relaxed);
    if stato.sfumando_fino.swap(0, Ordering::Relaxed) != 0 {
        lettore.motore.annulla_sfumatura();
    }
    // Quel che parlava della coda di prima, e che in una coda di voci non
    // vuol dire più niente.
    lettore.coda_di_prima = None;
    lettore.coda_del_giro = None;
    lettore.motivo_prossimo = None;
    lettore.puntina_a_vuoto = None;
    lettore.prepara(None);
    riapertura::dimentica_l_ascolto_da_continuare();
}

/// I segni fuori dal lettore che entrando e uscendo non valgono più: il
/// punto di «Riprova», la scheda del brano corrente. Si chiama a lucchetto del
/// lettore lasciato: quei lucchetti sono foglie, ma non c'è ragione di
/// tenerli insieme.
fn azzera_i_segni(stato: &StatoLettore) {
    *stato
        .ripresa
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    *stato
        .scheda_corrente
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

/// Quel che [`entra_come_host`] fotografa prima di costruire le voci.
struct Fotografia {
    /// Il salto chiesto a motore vuoto, se c'era: vedi `Lettore::puntina_a_vuoto`.
    puntina: Option<(i64, u64)>,
    istantanea: QueueSnapshot,
    ordine: Vec<i64>,
    posto: Option<usize>,
    curata: bool,
    posizione: rymembra_play::Posizione,
    formato: rymembra_play::FormatoUscita,
}

// ── chi ospita ──────────────────────────────────────────────────────────────

/// Entra come host: la coda diventa di voci di sessione e la voce corrente si
/// riapre sotto la sua maniglia, allo stesso punto e nello stesso stato. Si
/// accendono le regole di sessione (dissolvenza a zero, niente autoplay,
/// shuffle o timer, coda non salvata) — in memoria, mai nelle impostazioni.
///
/// # I tre tempi
///
/// Si fotografa la coda; a lettore lasciato se ne costruiscono le voci dalla
/// libreria e si riapre il brano che suona; sotto lucchetto si consegna tutto
/// insieme, se nel frattempo la coda è rimasta quella. Se è cambiata — un
/// «prossimo» premuto proprio adesso — si ricomincia una volta.
///
/// La coda entra al più per [`MASSIMO_VOCI`] voci attorno a quella che suona:
/// vedi [`finestra`]. Lo shuffle si spegne tenendo l'ordine in cui si stava
/// ascoltando, che diventa l'ordine della sessione.
///
/// # Errori
///
/// `internal.aborted` se il lettore non c'è, o se la coda ha continuato a
/// cambiare; `insieme.giaInSessione` se ci si è già; quelli della libreria.
pub fn entra_come_host(app: &tauri::AppHandle) -> Result<(), AppError> {
    let stato = stato_lettore(app)?;
    super::sveglia_fuori_dal_lucchetto(&stato);
    for _ in 0..2 {
        // ── 1. la fotografia ──
        let foto = con_lettore(&stato, |lettore| {
            if lettore.sessione.is_some() {
                return Err(gia_in_sessione());
            }
            Ok(Fotografia {
                puntina: lettore.puntina_a_vuoto,
                istantanea: lettore.coda.snapshot(),
                ordine: lettore.coda.in_play_order(),
                posto: lettore.coda.position(),
                curata: lettore.coda_curata,
                posizione: lettore.motore.posizione(),
                formato: lettore.motore.formato(),
            })
        })?;

        // ── 2. le voci e il brano che suona, a lettore lasciato ──
        let (finestra, posto) = finestra(&foto.ordine, foto.posto, MASSIMO_VOCI);
        let voci = voci_allineate(app, &finestra)?;
        let corrente = posto.and_then(|posto| voci.get(posto).copied().flatten());
        let in_motore = foto.posizione.track_id;
        // Il brano si riapre solo se è quello che la coda indica: un motore
        // che suona altro — un successivo appena attaccato — lo fermerà la
        // consegna, e la coda ripartirà dalla sua voce.
        let riaperto = match corrente {
            Some(maniglia) if in_motore.is_some() && in_motore == foto.istantanea_corrente() => {
                match super::brano_di(app, maniglia, foto.formato) {
                    Ok(brano) => Some((maniglia, brano)),
                    Err(err) => {
                        nota!(
                            "[insieme] il brano corrente non si riapre come voce: {}",
                            err.code().kind().code()
                        );
                        None
                    }
                }
            }
            _ => None,
        };
        // Il punto da cui la voce corrente ripartirà al primo play, se il motore
        // non la sta suonando: quello da cui l'utente l'avrebbe ripresa — il
        // salto chiesto a motore vuoto, o il segno di «riprendi dov'eri» della
        // sua riga. Senza, la sessione cominciava il brano lasciato a cinquanta
        // secondi dall'inizio. Il segno si legge qui, a lettore lasciato.
        let segno = match (corrente, riaperto.is_some()) {
            (Some(maniglia), false) => foto
                .istantanea_corrente()
                .and_then(|riga| segno_dell_utente(app, foto.puntina, riga))
                .map(|ms| (maniglia, ms)),
            _ => None,
        };
        let maniglie: Vec<i64> = voci.iter().flatten().copied().collect();
        let nuovo_posto =
            corrente.and_then(|maniglia| maniglie.iter().position(|id| *id == maniglia));

        // ── 3. la consegna ──
        let consegna = con_lettore(&stato, |lettore| {
            if lettore.sessione.is_some() {
                return Err(gia_in_sessione());
            }
            if lettore.coda.snapshot() != foto.istantanea {
                return Ok(false);
            }
            // Il motore è stato sostituito mentre si apriva il brano: è stato
            // aperto per l'uscita di prima, e consegnato a questa suonerebbe
            // alla velocità o coi canali sbagliati. Si riprova, come fanno gli
            // altri percorsi a tre tempi.
            if lettore.motore.formato() != foto.formato {
                return Ok(false);
            }
            let adesso = lettore.motore.posizione();
            lettore.sessione = Some(Sessione::nuova(
                Ruolo::Ospita,
                CodaDiPrima {
                    // Senza maniglie: una voce rimasta nella coda dell'utente
                    // da una gara d'ingresso tornerebbe all'uscita come una
                    // maniglia sparita, e una coda con una maniglia non si
                    // salva più. Vedi [`ripulita`].
                    istantanea: ripulita(foto.istantanea.clone()),
                    curata: foto.curata,
                    ms: adesso.ms,
                    avviata: adesso.track_id.is_some(),
                },
            ));
            let quante = maniglie.len();
            lettore.coda = Queue::restore(QueueSnapshot {
                tracks: maniglie.clone(),
                order: (0..quante).collect(),
                order_pos: nuovo_posto,
                shuffle: false,
                repeat: foto.istantanea.repeat,
            });
            lettore.coda_curata = false;
            accendi_le_regole(&stato, lettore);
            match riaperto {
                // Il brano di prima, sotto la sua maniglia, dal punto di
                // **adesso**: fra la fotografia e qui la musica è andata avanti.
                // L'ascolto è lo stesso — chi ascolta non ha cambiato brano —
                // e l'annuncio del motore lo continua invece di chiuderlo.
                Some((maniglia, brano)) if adesso.track_id == in_motore => {
                    if adesso.in_pausa {
                        lettore.apri_fermo(brano, adesso.ms);
                    } else {
                        lettore.suona_da(brano, adesso.ms);
                    }
                    riapertura::continua_l_ascolto(maniglia);
                }
                // Il motore suonava una riga della libreria che non è
                // diventata una voce: non può restare lì, sotto un id che la
                // coda non ha. Si ferma, e la coda resta sulla sua voce.
                _ if adesso.track_id.is_some() => {
                    chiudi_ascolto(app, lettore);
                    lettore.ferma();
                }
                _ => {}
            }
            // Dopo `ferma`, che non tocca la puntina, e dopo le regole, che la
            // azzerano: è il punto della coda nuova. C'è solo se la voce non è
            // stata riaperta al volo — quella suona già dal suo punto.
            if segno.is_some() {
                lettore.puntina_a_vuoto = segno;
            }
            prepara_prossimo(app, lettore);
            manda_stato(app, lettore);
            Ok(true)
        });
        // Un tentativo che non consegna non lascia voci nel registro: sono
        // voci che nessuna coda nomina, e il loro `uid` non tornerà.
        let fatto = match consegna {
            Ok(fatto) => fatto,
            Err(err) => {
                dimentica_voci(app, &maniglie);
                return Err(err);
            }
        };
        if !fatto {
            dimentica_voci(app, &maniglie);
        }
        if fatto {
            azzera_i_segni(&stato);
            let voci = ospiti::con_registro(app, |registro| registro.quante()).unwrap_or(0);
            nota!("[insieme] si ospita: {voci} voci in coda");
            return Ok(());
        }
    }
    Err(AppError::new(ErrorCode::InternalAborted {
        what: Some("ingresso nella sessione".to_owned()),
    })
    .with_cause("la coda ha continuato a cambiare mentre se ne costruivano le voci"))
}

/// Il punto da cui l'utente avrebbe ripreso la riga `riga`: il salto chiesto
/// a motore vuoto se era di lei, altrimenti il segno di «riprendi dov'eri».
/// `None` per l'inizio. Legge il database: a lettore lasciato.
fn segno_dell_utente(
    app: &tauri::AppHandle,
    puntina: Option<(i64, u64)>,
    riga: i64,
) -> Option<u64> {
    if let Some((di, ms)) = puntina
        && di == riga
    {
        return Some(ms);
    }
    let stato = super::libreria_di(app, "segno di ripresa").ok()?;
    con_libreria(&stato, |libreria| {
        rymembra_app::playback::load_posizione(&libreria.connection, riga)
    })
    .ok()
    .filter(|ms| *ms > 0)
}

impl Fotografia {
    /// Il brano che la coda indicava nella fotografia.
    fn istantanea_corrente(&self) -> Option<i64> {
        self.posto.and_then(|posto| self.ordine.get(posto).copied())
    }
}

/// Fase 2, chi ospita: mette in coda voci già registrate (quelle di un
/// membro), in fondo o subito dopo la corrente.
///
/// Restituisce quante ne sono entrate, dalla prima, accanto all'esito: anche
/// quando l'esito è un errore — l'apertura della prima, a coda vuota, arriva
/// **dopo** l'accoda — quelle contate sono in coda, e solo le altre sono
/// rimaste fuori.
///
/// # La coda piena
///
/// Il tetto è quello di [`entrano`], ma senza il suo avviso: il gesto è del
/// membro, e una fascia rossa sulla finestra di chi ospita parlerebbe di un
/// gesto che lì nessuno ha fatto, mentre il membro non saprebbe niente. Chi
/// chiama manda il «no» al membro (vedi `insieme::filo::ospita::esito_dell_aggiunta`).
/// Le voci rimaste fuori escono dal registro qui, sotto lo stesso lucchetto
/// che le ha lasciate fuori: in coda non sono mai arrivate, e nessuno le
/// aprirà più.
///
/// # Errori
///
/// `internal.aborted` se il lettore non c'è; `insieme.gestoNonDisponibile`
/// fuori dalla sessione o per chi segue; quelli dell'apertura se la coda era
/// vuota e la voce parte.
pub fn accoda_maniglie(
    app: &tauri::AppHandle,
    maniglie: &[i64],
    dopo_la_corrente: bool,
) -> (usize, Result<(), AppError>) {
    let stato = match stato_lettore(app) {
        Ok(stato) => stato,
        Err(err) => return (0, Err(err)),
    };
    let mut entrate = 0;
    let esito = gesto_di_coda(app, &stato, |lettore| {
        if !ospita(lettore) {
            return Err(non_disponibile("accoda"));
        }
        let era_vuota = lettore.coda.is_empty();
        let quante = quante_entrano(true, lettore.coda.len(), maniglie.len());
        let dentro = maniglie.get(..quante).unwrap_or_default();
        dimentica_voci(app, maniglie.get(quante..).unwrap_or_default());
        entrate = quante;
        if dopo_la_corrente {
            lettore.coda.play_next(dentro);
        } else {
            lettore.coda.enqueue(dentro);
        }
        if era_vuota {
            return Ok(true);
        }
        prepara_prossimo(app, lettore);
        salva_coda(app, lettore);
        manda_stato(app, lettore);
        Ok(false)
    });
    (entrate, esito)
}

/// La coda della sessione, adesso. `None` fuori sessione.
///
/// Anche per chi segue, sulla coda specchio: la finestra del membro manda
/// gesti per posizione (`coda_vai`, `coda_togli`, `coda_riordina`), e la
/// sessione li traduce in `uid` da qui prima di mandarli all'host.
#[must_use]
pub fn istantanea(app: &tauri::AppHandle) -> Option<Istantanea> {
    let stato = app.try_state::<StatoLettore>()?;
    con_lettore(&stato, |lettore| {
        if lettore.sessione.is_none() {
            return Ok(None);
        }
        let ordine = lettore.coda.in_play_order();
        let corrente = lettore.coda.current();
        // Le maniglie diventano `uid` col registro in mano per un attimo: è un
        // lucchetto foglia, e prenderlo qui dentro tiene la coda e la sua
        // traduzione nello stesso istante.
        let (coda, corrente) = ospiti::con_registro(app, |registro| {
            let uid = |maniglia: i64| registro.voce(maniglia).map(|voce| voce.uid);
            (
                ordine.iter().filter_map(|m| uid(*m)).collect(),
                corrente.and_then(uid),
            )
        })
        .unwrap_or_default();
        let in_pausa = lettore
            .sessione
            .as_ref()
            .and_then(|sessione| sessione.pausa_dell_host)
            .unwrap_or_else(|| lettore.motore.posizione().in_pausa);
        Ok(Some(Istantanea {
            coda,
            corrente,
            in_pausa,
            ripeti: lettore.coda.repeat(),
        }))
    })
    .ok()
    .flatten()
}

/// Chi ospita: accende o spegne la calibrazione. Accesa, il motore suona la
/// voce di calibrazione al posto della corrente; spenta, la corrente riparte
/// dal punto e nello stato in cui era.
///
/// Accesa, i clic partono programmati come ogni voce nuova di chi ospita, e
/// l'ancora li annuncia con l'`uid` di calibrazione: i membri li riaprono come
/// qualunque voce, dalla stessa generazione in memoria. L'ascolto del brano di
/// prima si ferma e riparte quando la calibrazione si spegne, senza contare il
/// tempo dei clic.
///
/// # Errori
///
/// `internal.aborted` se il lettore non c'è o l'uscita è cambiata a metà;
/// `insieme.gestoNonDisponibile` se non si ospita; quelli dell'apertura.
pub fn calibra(app: &tauri::AppHandle, attiva: bool) -> Result<(), AppError> {
    let stato = stato_lettore(app)?;
    if attiva {
        accendi_la_calibrazione(app, &stato)
    } else {
        spegni_la_calibrazione(app, &stato)
    }
}

fn accendi_la_calibrazione(app: &tauri::AppHandle, stato: &StatoLettore) -> Result<(), AppError> {
    let formato = con_lettore(stato, |lettore| {
        if !ospita(lettore) {
            return Err(non_disponibile("calibra"));
        }
        Ok((!in_calibrazione(lettore)).then(|| lettore.motore.formato()))
    })?;
    let Some(formato) = formato else {
        return Ok(());
    };
    let voce = ospiti::voce_di_calibrazione();
    let brano = ospiti::apri(app, MANIGLIA_CALIBRAZIONE, &voce, formato)?;
    con_lettore(stato, |lettore| {
        if !ospita(lettore) || in_calibrazione(lettore) {
            return Ok(());
        }
        if lettore.motore.formato() != formato {
            return Err(AppError::new(ErrorCode::InternalAborted {
                what: Some("calibrazione".to_owned()),
            })
            .with_cause("l'uscita è cambiata mentre si preparavano i clic"));
        }
        // La voce a cui tornare è quella della coda, e il punto quello del
        // motore solo se il motore sta già suonando lei: una voce appena
        // scelta e non ancora uscita dalle casse riparte dall'inizio.
        let adesso = lettore.motore.posizione();
        let corrente = lettore.coda.current();
        let prima = PrimaDeiClic {
            maniglia: corrente,
            ms: if adesso.track_id == corrente {
                adesso.ms
            } else {
                0
            },
            suonava: adesso.track_id.is_some() && !adesso.in_pausa,
        };
        if let Some(ascolto) = lettore.ascolto.as_mut() {
            ascolto.pause(adesso_ms());
        }
        lettore.prepara(None);
        parti(lettore, brano, 0);
        // Dopo `parti`: la partenza dimentica la calibrazione di prima (vedi
        // [`dopo_un_avvio`]), e questa è quella nuova.
        if let Some(sessione) = lettore.sessione.as_mut() {
            sessione.calibrazione = Some(prima);
        }
        manda_stato(app, lettore);
        Ok(())
    })
}

fn spegni_la_calibrazione(app: &tauri::AppHandle, stato: &StatoLettore) -> Result<(), AppError> {
    // Una copia, non un `take`: fra qui e la consegna qualcuno può scegliere
    // un'altra voce, e allora la calibrazione è già finita da sé (vedi
    // [`dopo_un_avvio`]) e non c'è niente da rimettere.
    let prima = con_lettore(stato, |lettore| {
        let formato = lettore.motore.formato();
        Ok(lettore
            .sessione
            .as_ref()
            .and_then(|sessione| sessione.calibrazione)
            .map(|prima| (prima, formato)))
    })?;
    let Some((prima, formato)) = prima else {
        return Ok(());
    };
    let brano = prima
        .maniglia
        .filter(|maniglia| !e_la_calibrazione(*maniglia))
        .and_then(|maniglia| match super::brano_di(app, maniglia, formato) {
            Ok(brano) => Some((maniglia, brano)),
            Err(err) => {
                nota!(
                    "[insieme] dopo la calibrazione la voce di prima non si riapre: {}",
                    err.code().kind().code()
                );
                None
            }
        });
    con_lettore(stato, |lettore| {
        // La calibrazione la dice la sessione e non la posizione del motore:
        // nel quarto di secondo prima che i clic partano, il motore direbbe
        // ancora la voce di prima.
        if !ospita(lettore) || !in_calibrazione(lettore) {
            return Ok(());
        }
        if lettore.motore.formato() != formato {
            return Err(AppError::new(ErrorCode::InternalAborted {
                what: Some("calibrazione".to_owned()),
            })
            .with_cause("l'uscita è cambiata mentre si riapriva la voce di prima"));
        }
        if let Some(sessione) = lettore.sessione.as_mut() {
            sessione.calibrazione = None;
        }
        match brano {
            Some((maniglia, brano)) => {
                if prima.suonava {
                    parti(lettore, brano, prima.ms);
                } else {
                    lettore.apri_fermo(brano, prima.ms);
                }
                riapertura::continua_l_ascolto(maniglia);
            }
            None => lettore.ferma(),
        }
        prepara_prossimo(app, lettore);
        manda_stato(app, lettore);
        Ok(())
    })
}

/// Chi ospita: il motore non ha nessun brano in mano (un'apertura fallita, la
/// coda finita). Non lo stesso di «nessuna ancora», che capita anche per un
/// attimo fra due brani di fila.
#[must_use]
pub fn motore_senza_brano(app: &tauri::AppHandle) -> bool {
    app.try_state::<StatoLettore>().is_some_and(|stato| {
        con_lettore(&stato, |lettore| {
            Ok(lettore.motore.posizione().track_id.is_none())
        })
        .unwrap_or(false)
    })
}

/// L'ancora del motore di chi ospita: dove è adesso, o dove partirà se una
/// ripresa è programmata.
///
/// # Prima l'annuncio, poi il motore
///
/// Un avvio programmato il motore non lo racconta: fino all'istante la
/// callback tace, e la sua ancora non dice ancora il punto da cui partirà. Lo
/// dice l'annuncio scritto da chi l'ha chiesto, finché l'ancora vera non
/// arriva — la stessa voce, in corso, a quell'istante o dopo — o finché non è
/// chiaro che non arriverà ([`SCADENZA_PROGRAMMATA_NS`]).
#[must_use]
pub fn ancora_di_questo(app: &tauri::AppHandle) -> Option<AncoraDiQuesto> {
    let stato = app.try_state::<StatoLettore>()?;
    con_lettore(&stato, |lettore| Ok(ancora_del_motore(lettore)))
        .ok()
        .flatten()
}

fn ancora_del_motore(lettore: &mut Lettore) -> Option<AncoraDiQuesto> {
    if !ospita(lettore) {
        return None;
    }
    let adesso = tempo::adesso_ns();
    let in_pausa = lettore.motore.posizione().in_pausa;
    let vera = if in_pausa {
        lettore
            .motore
            .in_attesa()
            .and_then(|(etichetta, posizione_us)| {
                Some(AncoraDiQuesto {
                    etichetta: etichetta?,
                    posizione_us,
                    udito_ns: adesso,
                    in_corso: false,
                    programmata: false,
                })
            })
    } else {
        lettore
            .motore
            .ancora()
            .filter(|ancora| ancora.in_corso)
            .and_then(|ancora| {
                Some(AncoraDiQuesto {
                    etichetta: ancora.etichetta?,
                    posizione_us: ancora.posizione_us,
                    udito_ns: ancora.udito_ns,
                    in_corso: true,
                    programmata: false,
                })
            })
    };
    let sessione = lettore.sessione.as_mut()?;
    if let Some(promessa) = sessione.programmata
        && let Some(etichetta) = promessa.etichetta
    {
        // Arrivata vuol dire: la voce promessa, suonata dall'istante in poi,
        // **e** al punto promesso. Con una latenza d'uscita lunga (Bluetooth)
        // l'ancora del punto di prima ha già un istante d'ascolto oltre quello
        // promesso, e senza il punto passava per arrivata.
        let arrivata = vera.is_some_and(|v| {
            let atteso_us = promessa.posizione_us.saturating_add(
                v.udito_ns
                    .saturating_sub(promessa.quando_ns)
                    .checked_div(1000)
                    .unwrap_or(0),
            );
            v.in_corso
                && v.etichetta == etichetta
                && v.udito_ns >= promessa.quando_ns
                && v.posizione_us.abs_diff(atteso_us) <= ARRIVATO_ENTRO_MS.saturating_mul(1000)
        });
        let scaduta = adesso > promessa.quando_ns.saturating_add(SCADENZA_PROGRAMMATA_NS);
        if !arrivata && !scaduta && !in_pausa {
            return Some(AncoraDiQuesto {
                etichetta,
                posizione_us: promessa.posizione_us,
                udito_ns: promessa.quando_ns,
                in_corso: true,
                programmata: true,
            });
        }
        sessione.programmata = None;
    }
    vera
}

// ── chi segue ───────────────────────────────────────────────────────────────

/// Entra come membro: si mette da parte la coda di questo PC (in memoria; la
/// coda salvata sul disco non si tocca), si ferma il motore, si accendono le
/// regole di sessione.
///
/// L'ascolto in corso si chiude e conta per quel che si è sentito: il brano
/// di prima finisce qui. La coda resta vuota finché il primo stato dell'host
/// non la riempie con [`specchia`].
///
/// # Errori
///
/// `internal.aborted` se il lettore non c'è; `insieme.giaInSessione` se ci si
/// è già.
pub fn entra_come_membro(app: &tauri::AppHandle) -> Result<(), AppError> {
    let stato = stato_lettore(app)?;
    super::sveglia_fuori_dal_lucchetto(&stato);
    con_lettore(&stato, |lettore| {
        if lettore.sessione.is_some() {
            return Err(gia_in_sessione());
        }
        let adesso = lettore.motore.posizione();
        let prima = CodaDiPrima {
            istantanea: lettore.coda.snapshot(),
            curata: lettore.coda_curata,
            ms: adesso.ms,
            avviata: adesso.track_id.is_some(),
        };
        chiudi_ascolto(app, lettore);
        lettore.ferma();
        lettore.sessione = Some(Sessione::nuova(Ruolo::Segue, prima));
        lettore.coda = Queue::new();
        lettore.coda_curata = false;
        accendi_le_regole(&stato, lettore);
        manda_stato(app, lettore);
        Ok(())
    })?;
    azzera_i_segni(&stato);
    nota!("[insieme] si segue una sessione");
    Ok(())
}

/// Aggiunge o aggiorna voci per `uid`, e restituisce le loro maniglie. La usa
/// chi segue per lo specchio, e chi ospita (fase 2) per le voci dei membri.
pub fn registra(app: &tauri::AppHandle, voci: Vec<Voce>) -> Vec<i64> {
    ospiti::con_registro(app, |registro| {
        voci.into_iter()
            .map(|voce| registro.registra(voce))
            .collect()
    })
    .unwrap_or_default()
}

/// Chi segue: la coda di questo PC diventa quella della sessione. La voce
/// corrente del motore resta finché c'è; la sceglie il regolatore, non questa
/// funzione.
///
/// Gli `uid` che il registro non conosce si saltano: la sessione registra le
/// voci prima di specchiarle, e una voce che manca è una voce che non si
/// saprebbe aprire. Uno stato identico a quello che c'è già non cambia niente,
/// nemmeno la finestra: l'host lo manda intero a ogni gesto.
pub fn specchia(
    app: &tauri::AppHandle,
    coda: &[u64],
    corrente: Option<u64>,
    in_pausa: bool,
    ripeti: rymembra_domain::queue::RepeatMode,
) {
    let Some(stato) = app.try_state::<StatoLettore>() else {
        return;
    };
    // Il registro prima e da solo: è un lucchetto foglia, e col lettore in mano
    // qui non serve.
    let (maniglie, corrente) = ospiti::con_registro(app, |registro| {
        (
            coda.iter()
                .filter_map(|uid| registro.maniglia(*uid))
                .collect::<Vec<i64>>(),
            corrente.and_then(|uid| registro.maniglia(uid)),
        )
    })
    .unwrap_or_default();
    let _ = con_lettore(&stato, |lettore| {
        if !segue(lettore) {
            return Ok(());
        }
        if let Some(sessione) = lettore.sessione.as_mut() {
            sessione.pausa_dell_host = Some(in_pausa);
        }
        let posto = corrente.and_then(|m| maniglie.iter().position(|id| *id == m));
        let quante = maniglie.len();
        let nuova = QueueSnapshot {
            tracks: maniglie,
            order: (0..quante).collect(),
            order_pos: posto,
            shuffle: false,
            repeat: ripeti,
        };
        if lettore.coda.snapshot() == nuova {
            return Ok(());
        }
        let prima = lettore.coda.in_play_order();
        let tenute = tenute_dal_lettore(&stato, lettore);
        lettore.coda = Queue::restore(nuova);
        prepara_prossimo(app, lettore);
        dimentica_le_uscite(app, &stato, lettore, &prima, tenute);
        manda_stato(app, lettore);
        Ok(())
    });
}

/// Le maniglie che il lettore tiene fuori dalla coda: quella che il motore ha
/// in mano, il successivo in canna, quello che non si è aperto, l'annuncio
/// atteso, il punto chiesto a motore vuoto, e il brano di «Riprova». Una voce
/// tolta dalla coda può essere ancora una di queste, e il registro la deve
/// saper rispondere finché lo è. Col lucchetto del lettore in mano: quello di
/// `ripresa` viene dopo, come in `fermato`.
pub(super) fn tenute_dal_lettore(stato: &StatoLettore, lettore: &Lettore) -> Vec<i64> {
    let ripresa = stato
        .ripresa
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .map(|r| r.track_id);
    [
        lettore.motore.posizione().track_id,
        lettore.in_canna,
        lettore.non_aperto,
        lettore.annuncio_atteso,
        lettore.puntina_a_vuoto.map(|(id, _)| id),
        ripresa,
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Dopo un gesto che ha sostituito o accorciato la coda di sessione: le voci
/// che ne sono uscite, e che il lettore non tiene più (né prima del gesto —
/// `tenute`, da [`tenute_dal_lettore`] — né adesso), escono dal registro.
/// Col lucchetto del lettore in mano: il registro è una foglia.
///
/// # Perché solo quelle uscite in questo gesto
///
/// Un registro che non toglie le voci uscite dalla coda cresce per tutta la
/// sessione: mille voci a ogni stato dell'host con una coda nuova, su ogni PC,
/// fino a «Termina». Tenere solo le voci della coda di adesso non si può: le
/// voci si registrano **prima** del lucchetto del lettore (`instrada`, le
/// aggiunte di un membro, lo specchio), e un gesto che arrivasse in mezzo
/// toglierebbe quelle appena create e non ancora in coda. Quelle non sono mai
/// nella coda di prima, e così non si toccano.
///
/// La chiamano chi segue in [`specchia`] e, su chi ospita, i gesti che
/// sostituiscono o accorciano la coda: `suona`, `coda_svuota`, `coda_togli`,
/// `sgombra_dalla_coda`. Una o due voci per gesto restano lo stesso — quella
/// che il motore suonava e il successivo in canna, tenute prima del gesto —
/// ma non mille.
pub(super) fn dimentica_le_uscite(
    app: &tauri::AppHandle,
    stato: &StatoLettore,
    lettore: &Lettore,
    prima: &[i64],
    mut tenute: Vec<i64>,
) {
    // Fuori sessione la coda di prima ha solo righe della libreria: niente da
    // togliere, e niente insiemi da costruire su una coda di ventimila brani.
    if prima
        .iter()
        .all(|maniglia| ospiti::e_di_libreria(*maniglia))
    {
        return;
    }
    tenute.extend(tenute_dal_lettore(stato, lettore));
    dimentica_voci(
        app,
        &uscite_dalla_coda(prima, &lettore.coda.in_play_order(), &tenute),
    );
}

/// Le maniglie della coda di `prima` che non sono in quella di `dopo` né fra
/// le `tenute`: solo voci di sessione, mai la calibrazione, ognuna una volta.
/// Vedi [`dimentica_le_uscite`].
fn uscite_dalla_coda(prima: &[i64], dopo: &[i64], tenute: &[i64]) -> Vec<i64> {
    let restano: std::collections::HashSet<i64> = dopo.iter().chain(tenute).copied().collect();
    let mut viste = std::collections::HashSet::new();
    prima
        .iter()
        .copied()
        .filter(|maniglia| {
            !ospiti::e_di_libreria(*maniglia)
                && !e_la_calibrazione(*maniglia)
                && !restano.contains(maniglia)
                && viste.insert(*maniglia)
        })
        .collect()
}

/// Chi segue: quel che il regolatore deve sapere del motore di questo PC.
///
/// Fermo, il punto da cui ripartirà (`Motore::in_attesa`); suonando,
/// l'ultima ancora in corso. Senza niente in mano, un «fermo su nessuna
/// voce»: il regolatore riapre subito invece di aspettare un secondo che
/// arrivi un'ancora che non arriverà. `None` quando il motore sta cambiando
/// punto — un salto, una voce appena data — e la risposta giusta arriva al
/// passo dopo.
#[must_use]
pub fn da_me(app: &tauri::AppHandle) -> Option<DaMe> {
    let stato = app.try_state::<StatoLettore>()?;
    con_lettore(&stato, |lettore| Ok(da_me_nel(lettore)))
        .ok()
        .flatten()
}

fn da_me_nel(lettore: &Lettore) -> Option<DaMe> {
    if !segue(lettore) {
        return None;
    }
    let motore = &lettore.motore;
    let in_correzione = motore.in_correzione();
    let riserva_us = motore.riserva_us();
    let posizione = motore.posizione();
    if posizione.track_id.is_none() {
        return Some(DaMe {
            etichetta: None,
            posizione_us: 0,
            udito_ns: 0,
            in_corso: false,
            in_correzione,
            riserva_us,
        });
    }
    if posizione.in_pausa {
        let (etichetta, posizione_us) = motore.in_attesa()?;
        return Some(DaMe {
            etichetta,
            posizione_us,
            udito_ns: 0,
            in_corso: false,
            in_correzione,
            riserva_us,
        });
    }
    let ancora = motore.ancora().filter(|ancora| ancora.in_corso)?;
    Some(DaMe {
        etichetta: ancora.etichetta,
        posizione_us: ancora.posizione_us,
        udito_ns: ancora.udito_ns,
        in_corso: true,
        in_correzione,
        riserva_us,
    })
}

/// Chi segue: esegue una mossa del regolatore. `Riapri` apre la voce fuori
/// dal lucchetto.
///
/// Le mosse si eseguono come le esegue il banco della fase 0
/// (`rymembra-play/examples/insieme_locale.rs`), con le stesse convenzioni di
/// segno — sono quelle del motore — e la stessa cura dei microsecondi che
/// `apri_fermo` non prende.
///
/// # Errori
///
/// Quelli dell'apertura della voce; `playback.sourceUnavailable` per una voce
/// che il registro non ha.
pub fn applica(app: &tauri::AppHandle, mossa: Mossa, frequenza: u32) -> Result<(), AppError> {
    let stato = stato_lettore(app)?;
    match mossa {
        Mossa::Niente => Ok(()),
        Mossa::Gradino(fotogrammi) => con_lettore(&stato, |lettore| {
            if segue(lettore) {
                lettore.motore.gradino(fotogrammi);
            }
            Ok(())
        }),
        Mossa::Deriva(ppm) => con_lettore(&stato, |lettore| {
            if segue(lettore) {
                lettore.motore.deriva(ppm);
            }
            Ok(())
        }),
        Mossa::Pausa => con_lettore(&stato, |lettore| {
            if !segue(lettore) {
                return Ok(());
            }
            lettore.motore.pausa();
            if let Some(ascolto) = lettore.ascolto.as_mut() {
                ascolto.pause(adesso_ms());
            }
            manda_stato(app, lettore);
            Ok(())
        }),
        Mossa::AvviaA { quando_ns, salta } => {
            // Il flusso in pausa riposa, e `riprendi_a` lo sveglierebbe col
            // lucchetto in mano: qui lo si sveglia prima, a lucchetto libero.
            super::sveglia_fuori_dal_lucchetto(&stato);
            let da_riaprire = con_lettore(&stato, |lettore| {
                if !segue(lettore) {
                    return Ok(None);
                }
                Ok(avvia_a(lettore, quando_ns, salta, frequenza))
            })?;
            match da_riaprire {
                Some((etichetta, posizione_us)) => {
                    riapri(app, &stato, etichetta, posizione_us, quando_ns)
                }
                None => {
                    let _ = con_lettore(&stato, |lettore| {
                        if let Some(ascolto) = lettore.ascolto.as_mut() {
                            ascolto.resume(adesso_ms());
                        }
                        manda_stato(app, lettore);
                        Ok(())
                    });
                    Ok(())
                }
            }
        }
        Mossa::Riapri {
            etichetta,
            posizione_us,
            quando_ns,
        } => riapri(app, &stato, etichetta, posizione_us, quando_ns),
        Mossa::Posiziona {
            etichetta,
            posizione_us,
        } => posiziona(app, &stato, etichetta, posizione_us),
    }
}

/// `Posiziona`: l'host è fermo a un altro punto (ha saltato mentre era in
/// pausa) e questo PC si mette lì, **fermo**. Nella stessa voce basta un salto
/// del motore; un'altra voce si apre fuori dal lucchetto, come in [`riapri`],
/// e si consegna ferma con `apri_fermo`. Niente promesse e niente avvio: alla
/// ripresa l'host annuncia l'istante, e il regolatore fa un `AvviaA`.
///
/// # Errori
///
/// Quelli dell'apertura della voce; `playback.sourceUnavailable` per una voce
/// che il registro non ha.
fn posiziona(
    app: &tauri::AppHandle,
    stato: &StatoLettore,
    etichetta: u64,
    posizione_us: u64,
) -> Result<(), AppError> {
    let trovata = ospiti::con_registro(app, |registro| {
        registro
            .maniglia(etichetta)
            .map(|maniglia| (maniglia, registro.generazione()))
    })
    .flatten();
    let voce =
        trovata.map(|(maniglia, generazione)| (maniglia, generazione, ospiti::chi(app, maniglia)));
    let Some((maniglia, generazione, Chi::Voce(voce))) = voce else {
        return Err(AppError::new(ErrorCode::PlaybackSourceUnavailable {
            track_id: None,
            path: None,
        })
        .with_cause(format!(
            "la voce {etichetta} da raggiungere in pausa non è nel registro di questo PC"
        )));
    };
    let da_ms = posizione_us.checked_div(1000).unwrap_or(0);
    // La stessa voce, già nel motore: un salto da fermi, e la barra lo dice.
    let fatto = con_lettore(stato, |lettore| {
        if !segue(lettore) {
            return Ok(true);
        }
        let adesso = lettore.motore.posizione();
        if adesso.track_id != Some(maniglia) {
            return Ok(false);
        }
        // Se suona, non è più il momento: l'host è ripartito, e la mossa
        // giusta la decide il regolatore al passo dopo.
        if adesso.in_pausa {
            lettore.motore.vai_a(da_ms);
            manda_stato(app, lettore);
        }
        Ok(true)
    })?;
    if fatto {
        return Ok(());
    }
    let formato = con_lettore(stato, |lettore| {
        Ok(segue(lettore).then(|| lettore.motore.formato()))
    })?;
    let Some(formato) = formato else {
        return Ok(());
    };
    let brano = ospiti::apri(app, maniglia, &voce, formato)?;
    con_lettore(stato, |lettore| {
        let viva =
            ospiti::con_registro(app, |registro| registro.ancora_viva(maniglia, generazione))
                .unwrap_or(false);
        // Nel frattempo la sessione può essere finita, o diventata un'altra
        // (lo stesso `uid` è un'altra voce), o l'uscita audio cambiata. Se
        // invece l'host è ripartito, la voce aperta ferma va bene lo stesso:
        // il regolatore la fa partire.
        if !segue(lettore) || lettore.motore.formato() != formato || !viva {
            return Ok(());
        }
        lettore.apri_fermo(brano, da_ms);
        cursore_sulla_voce(lettore, maniglia);
        manda_stato(app, lettore);
        Ok(())
    })
}

/// `AvviaA`, come la vuole `Mossa::AvviaA`: il fotogramma in attesa va
/// sentito `salta` fotogrammi prima di `quando`, cioè si chiede
/// `riprendi_a(quando − salta·1e9/f)` e se quell'istante è passato il motore
/// salta da sé. **Mai** un gradino seguito dalla ripresa: in pausa la callback
/// i gradini li lascia cadere.
///
/// Restituisce la voce e il punto da riaprire quando il motore rifiuta
/// perché l'istante è passato da più di un secondo: lì non è più una ripresa,
/// è una riapertura dove l'host sarà a `quando`.
fn avvia_a(
    lettore: &mut Lettore,
    quando_ns: u64,
    salta: i64,
    frequenza: u32,
) -> Option<(u64, u64)> {
    let frequenza = frequenza.max(1);
    let anticipo = i128::from(salta)
        .saturating_mul(1_000_000_000)
        .checked_div(i128::from(frequenza))
        .unwrap_or(0);
    let avvio = u64::try_from((i128::from(quando_ns) - anticipo).max(1)).unwrap_or(1);
    match lettore.motore.riprendi_a(avvio) {
        Ok(()) => None,
        Err(RifiutoAvvio::NelPassato) => {
            let (etichetta, posizione_us) = lettore.motore.in_attesa()?;
            let salto_us = i128::from(salta)
                .saturating_mul(1_000_000)
                .checked_div(i128::from(frequenza))
                .unwrap_or(0);
            let punto = u64::try_from(i128::from(posizione_us) + salto_us).unwrap_or(0);
            Some((etichetta?, punto))
        }
    }
}

/// `Riapri`: la voce `etichetta` si apre **fuori** dal lucchetto, poi
/// `apri_fermo` al millisecondo e `riprendi_a` all'istante, coi microsecondi
/// che `apri_fermo` non prende tolti dall'istante — lo schema di
/// `insieme_locale::riapri`.
///
/// # Quando l'apertura è stata più lunga dell'anticipo
///
/// Se aprire la voce ha preso più di un secondo oltre l'istante, il motore
/// rifiuta l'avvio come «nel passato». Invece di lasciare il motore fermo su un
/// punto ormai vecchio — e far ripartire da capo il regolatore con un'altra
/// apertura lunga uguale — si salta, nel file già aperto, dove l'host sarà fra
/// un altro anticipo, e si riparte lì: un salto in un file aperto costa molto
/// meno di un'apertura.
fn riapri(
    app: &tauri::AppHandle,
    stato: &StatoLettore,
    etichetta: u64,
    posizione_us: u64,
    quando_ns: u64,
) -> Result<(), AppError> {
    // La maniglia e la generazione del registro in cui la si è trovata: la
    // consegna, dopo un'apertura che può durare secondi, controlla che la voce
    // sia ancora quella. Vedi `Registro::ancora_viva`.
    let trovata = ospiti::con_registro(app, |registro| {
        registro
            .maniglia(etichetta)
            .map(|maniglia| (maniglia, registro.generazione()))
    })
    .flatten();
    let voce =
        trovata.map(|(maniglia, generazione)| (maniglia, generazione, ospiti::chi(app, maniglia)));
    let Some((maniglia, generazione, Chi::Voce(voce))) = voce else {
        return Err(AppError::new(ErrorCode::PlaybackSourceUnavailable {
            track_id: None,
            path: None,
        })
        .with_cause(format!(
            "la voce {etichetta} dell'ancora non è nel registro di questo PC"
        )));
    };
    let formato = con_lettore(stato, |lettore| {
        Ok(segue(lettore).then(|| lettore.motore.formato()))
    })?;
    let Some(formato) = formato else {
        return Ok(());
    };
    let inizio = std::time::Instant::now();
    let brano = ospiti::apri(app, maniglia, &voce, formato)?;
    let aperta_in = inizio.elapsed();
    con_lettore(stato, |lettore| {
        // La voce è ancora quella: la calibrazione ha la sua maniglia fissa e
        // non sta nel registro, ogni altra deve esserci ancora, nella stessa
        // generazione. Il registro è un lucchetto foglia: si chiede qui.
        let viva = e_la_calibrazione(maniglia)
            || ospiti::con_registro(app, |registro| registro.ancora_viva(maniglia, generazione))
                .unwrap_or(false);
        if !segue(lettore) || lettore.motore.formato() != formato || !viva {
            // Uscita dalla sessione — o una sessione nuova, dove lo stesso
            // `uid` è un'altra voce — o un'uscita audio nuova mentre la voce si
            // apriva: il brano non serve più, o è per il formato di prima. Il
            // regolatore lo chiederà di nuovo al passo dopo, se serve.
            return Ok(());
        }
        // La stessa voce riaperta per correggere — un salto grande, una
        // riapertura del regolatore — è lo stesso ascolto: si ferma adesso, e
        // l'annuncio d'inizio del motore lo fa ripartire invece di chiuderlo e
        // aprirne un altro. Conta per le voci che contano: le proprie di chi
        // segue (fase 2) e le copie trovate qui (fase 3).
        let continua = lettore.motore.posizione().track_id == Some(maniglia);
        if continua && let Some(ascolto) = lettore.ascolto.as_mut() {
            ascolto.pause(adesso_ms());
        }
        let da_ms = posizione_us.checked_div(1000).unwrap_or(0);
        let avanzo_ns = posizione_us
            .saturating_sub(da_ms.saturating_mul(1000))
            .saturating_mul(1000);
        lettore.apri_fermo(brano, da_ms);
        if continua {
            riapertura::continua_l_ascolto(maniglia);
        }
        // Il punto e l'istante promessi, per la finestra: fino al primo
        // campione il motore direbbe il punto della voce di prima. Vedi
        // [`posizione_da_mostrare`]. Dopo `apri_fermo`, che dimentica quelli
        // di prima.
        let mut promessa = Programmata {
            maniglia: Some(maniglia),
            etichetta: Some(etichetta),
            posizione_us,
            quando_ns,
        };
        if let Err(RifiutoAvvio::NelPassato) = lettore
            .motore
            .riprendi_a(quando_ns.saturating_sub(avanzo_ns))
        {
            // Vedi «Quando l'apertura è stata più lunga dell'anticipo».
            let adesso = tempo::adesso_ns();
            let nuovo_quando = adesso.saturating_add(ANTICIPO_NS);
            let avanti_us = nuovo_quando
                .saturating_sub(quando_ns)
                .checked_div(1000)
                .unwrap_or(0);
            let punto_us = posizione_us.saturating_add(avanti_us);
            let punto_ms = punto_us.checked_div(1000).unwrap_or(0);
            let avanzo = punto_us
                .saturating_sub(punto_ms.saturating_mul(1000))
                .saturating_mul(1000);
            nota!(
                "[insieme] voce aperta in {} ms, oltre l'anticipo: si salta avanti",
                aperta_in.as_millis()
            );
            lettore.motore.vai_a(punto_ms);
            if lettore
                .motore
                .riprendi_a(nuovo_quando.saturating_sub(avanzo))
                .is_err()
            {
                lettore.motore.riprendi();
            }
            promessa.posizione_us = punto_us;
            promessa.quando_ns = nuovo_quando;
        }
        if let Some(sessione) = lettore.sessione.as_mut() {
            sessione.programmata = Some(promessa);
        }
        cursore_sulla_voce(lettore, maniglia);
        manda_stato(app, lettore);
        Ok(())
    })
}

/// La frequenza d'uscita del motore, per le mosse in fotogrammi.
#[must_use]
pub fn frequenza(app: &tauri::AppHandle) -> Option<u32> {
    let stato = app.try_state::<StatoLettore>()?;
    con_lettore(&stato, |lettore| Ok(lettore.motore.formato().frequenza)).ok()
}

// ── tutti e due ─────────────────────────────────────────────────────────────

/// Esce dalla sessione: torna la coda di prima — ferma, al punto in cui era —
/// si spengono le regole di sessione e si dimenticano le voci.
///
/// «Al punto in cui era»: il brano corrente della coda di prima riparte, al
/// primo play, dal millisecondo in cui si era entrati (`puntina_a_vuoto`).
/// Le regole tornano quelle delle impostazioni senza rileggerle: non sono mai
/// state toccate. Le voci si dimenticano **dopo** aver rimesso la coda, così
/// nessuno vede mai una coda di maniglie sparite.
pub fn esci(app: &tauri::AppHandle) {
    let Some(stato) = app.try_state::<StatoLettore>() else {
        return;
    };
    super::sveglia_fuori_dal_lucchetto(&stato);
    let uscito = con_lettore(&stato, |lettore| {
        let Some(sessione) = lettore.sessione.take() else {
            return Ok(false);
        };
        chiudi_ascolto(app, lettore);
        lettore.ferma();
        // Il `Fermato` di questo stop arriverà quando la coda è già quella
        // dell'utente: non deve toccarla. Vedi `Lettore::fermato_della_sessione`.
        lettore.fermato_della_sessione = true;
        lettore.motore.deriva(0);
        lettore.motore.dissolvenza(dissolvenza_ms(lettore));
        let prima = sessione.prima;
        lettore.coda = Queue::restore(prima.istantanea);
        lettore.coda_curata = prima.curata;
        lettore.puntina_a_vuoto = prima
            .avviata
            .then(|| lettore.coda.current().map(|id| (id, prima.ms)))
            .flatten();
        lettore.coda_di_prima = None;
        lettore.coda_del_giro = None;
        lettore.motivo_prossimo = None;
        riapertura::dimentica_l_ascolto_da_continuare();
        // La coda sul disco è già questa — in sessione non si è mai scritta —
        // ma la bandiera costa niente e toglie ogni dubbio su un'uscita a metà.
        salva_coda(app, lettore);
        manda_stato(app, lettore);
        Ok(true)
    })
    .unwrap_or(false);
    // Solo se si era dentro: fuori sessione il punto di «Riprova» e la scheda
    // del brano sono dell'utente, e un'uscita chiamata per scrupolo — da una
    // sessione che non è mai entrata nel lettore — non li deve toccare. Il
    // registro invece si svuota sempre: è della sessione, anche quando
    // l'ingresso non è arrivato al lettore.
    if uscito {
        azzera_i_segni(&stato);
    }
    let dimenticate = ospiti::con_registro(app, |registro| {
        let quante = registro.quante();
        registro.svuota();
        quante
    })
    .unwrap_or(0);
    if uscito {
        nota!("[insieme] fuori dalla sessione: {dimenticate} voci dimenticate");
    }
}

// ── ondata 2: le fasi 2 e 3 ─────────────────────────────────────────────────

/// Fase 2, chi ospita: toglie dalla coda le voci con questi `uid` — quelle di
/// un membro che esce o che viene allontanato. La voce che suona, se è fra
/// queste, passa alla successiva come per un «togli» della finestra.
///
/// Per `uid` e non per posizione: fra la decisione della sessione e questa
/// chiamata la coda può essere cambiata, e una posizione vecchia toglierebbe
/// la voce di un altro. Per lo stesso motivo serve anche a un `Togli{uid}`
/// di un membro: è idempotente — una voce che non c'è più non fa niente.
///
/// La strada è quella di `sgombra_dalla_coda`, cioè quella di «togli»: se
/// suonava una di loro parte la voce dopo, nello stesso stato (ferma se era
/// ferma); se dopo non c'è niente, ci si ferma. Poi le voci escono dal
/// registro: chi le apriva non le potrà più aprire — i gettoni di chi esce
/// si revocano — e tenerle vorrebbe dire un registro che cresce per tutta la
/// sessione.
///
/// # Errori
///
/// `internal.aborted` se il lettore non c'è; quelli dell'apertura della voce
/// dopo, se la corrente era fra quelle tolte.
pub fn togli_voci(app: &tauri::AppHandle, uids: &[u64]) -> Result<(), AppError> {
    let stato = stato_lettore(app)?;
    let maniglie: Vec<i64> = ospiti::con_registro(app, |registro| {
        uids.iter()
            .filter_map(|uid| registro.maniglia(*uid))
            .filter(|maniglia| !e_la_calibrazione(*maniglia))
            .collect()
    })
    .unwrap_or_default();
    if maniglie.is_empty() {
        return Ok(());
    }
    if !con_lettore(&stato, |lettore| Ok(ospita(lettore)))? {
        return Ok(());
    }
    let esito = super::coda::sgombra_dalla_coda(app, &stato, &maniglie, false);
    ospiti::con_registro(app, |registro| registro.togli(&maniglie));
    esito
}

/// Ridisegna la finestra con lo stato del lettore com'è: una copertina di
/// sessione appena arrivata, una voce che ha trovato la sua copia locale.
/// Non blocca a lungo: prende il lucchetto per una fotografia.
///
/// Non avvisa la sessione (vedi `manda_stato_alla_finestra`): la chiama lei,
/// e un avviso che le facesse ripubblicare lo stato la farebbe richiamare.
/// Serve perché [`specchia`] con uno stato identico non ridisegna, giustamente,
/// e una copertina arrivata dopo lo specchio non cambia la coda.
pub fn aggiorna_finestra(app: &tauri::AppHandle) {
    let Some(stato) = app.try_state::<StatoLettore>() else {
        return;
    };
    let _ = con_lettore(&stato, |lettore| {
        super::manda_stato_alla_finestra(app, lettore);
        Ok(())
    });
}

/// Fase 3: la voce che suona adesso si legge da un file di questo PC — sua, o
/// una copia identica trovata qui — e non dalla rete. Fuori sessione `false`.
///
/// «Che suona» è quella del motore; a motore vuoto quella della coda, che è
/// quella che partirà. Una copia «trovata» vuol dire aperta almeno una volta:
/// la ricerca la fa l'apertura (vedi `ospiti::apri`), e prima di allora non si
/// sa. Prende il lucchetto del lettore per leggere la voce, e il registro dopo
/// averlo lasciato.
#[must_use]
pub fn voce_corrente_locale(app: &tauri::AppHandle) -> bool {
    let Some(stato) = app.try_state::<StatoLettore>() else {
        return false;
    };
    let voce = con_lettore(&stato, |lettore| {
        if lettore.sessione.is_none() {
            return Ok(None);
        }
        Ok(lettore
            .motore
            .posizione()
            .track_id
            .or_else(|| lettore.coda.current()))
    })
    .ok()
    .flatten();
    voce.is_some_and(|id| ospiti::si_apre_da_qui(app, id))
}

/// Fase 2, chi ospita: fa suonare la voce `uid`, come «vai» della coda ma per
/// `uid`. `false` se la voce non è più in coda: il gesto era su una voce che
/// nel frattempo qualcuno ha tolto, e non c'è niente da fare.
///
/// La posizione si cerca **sotto lo stesso lucchetto** che la usa: cercata
/// prima e usata poi con `coda_vai`, fra le due la coda può cambiare, e la
/// posizione vecchia farebbe suonare la voce di un altro. Poi la strada di
/// sempre: l'ascolto di prima si chiude, e la voce parte programmata come ogni
/// voce nuova di chi ospita.
///
/// # Errori
///
/// `internal.aborted` se il lettore non c'è; `insieme.gestoNonDisponibile`
/// se questo PC non ospita; quelli dell'apertura della voce.
pub fn suona_voce(app: &tauri::AppHandle, uid: u64) -> Result<bool, AppError> {
    let stato = stato_lettore(app)?;
    let Some(maniglia) = ospiti::con_registro(app, |registro| registro.maniglia(uid)).flatten()
    else {
        return Ok(false);
    };
    let mut trovata = false;
    gesto_di_coda(app, &stato, |lettore| {
        if !ospita(lettore) {
            return Err(non_disponibile("vai"));
        }
        let Some(posto) = lettore
            .coda
            .in_play_order()
            .iter()
            .position(|id| *id == maniglia)
        else {
            return Ok(false);
        };
        if lettore.coda.play_at(posto).is_none() {
            return Ok(false);
        }
        trovata = true;
        chiudi_ascolto(app, lettore);
        Ok(true)
    })?;
    Ok(trovata)
}

/// La calibrazione è accesa adesso, nel lettore di questo PC.
///
/// Letta dal lettore e non tenuta in una copia: la calibrazione la spegne
/// anche un gesto qualunque — un «prossimo», una voce scelta — perché ogni
/// voce nuova data al motore la chiude (vedi [`dopo_un_avvio`]), e una copia
/// tenuta altrove resterebbe accesa sui clic che non suonano più.
#[must_use]
pub fn calibrando(app: &tauri::AppHandle) -> bool {
    let Some(stato) = app.try_state::<StatoLettore>() else {
        return false;
    };
    con_lettore(&stato, |lettore| Ok(in_calibrazione(lettore))).unwrap_or(false)
}

#[cfg(test)]
mod prove;
