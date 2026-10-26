//! Il ponte fra la sessione e il lettore: l'unico punto che tocca `Lettore`
//! per conto di `crate::insieme`.
//!
//! # Il lucchetto
//!
//! Ogni funzione prende il lucchetto del lettore da sé, per il tempo di una
//! fotografia o di un comando al motore, e **mai** mentre apre un brano: aprire
//! una voce che sta sul disco di un altro PC può aspettare la rete, e la
//! finestra non deve aspettare con lei. Nessuna si chiama col lucchetto del
//! lettore già in mano.
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
//! [`ANTICIPO`] più in là, con `Motore::riprendi_a`, e l'ancora annuncia il
//! punto e l'istante **prima** che succedano. Gli altri hanno così il tempo di
//! aprire la stessa voce e partono nello stesso istante, invece di arrivare
//! dopo e rincorrere a salti. La pausa invece è immediata: i membri si fermano
//! quando arriva l'ancora, e i pochi millisecondi li toglie la ripresa dopo.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use rymembra_app::library::TrackSummary;
use rymembra_app::playback::{Collocazione, SchedaSorgente};
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::queue::{Queue, QueueSnapshot};
use rymembra_insieme::messaggi::MASSIMO_VOCI;
use rymembra_play::sincronia::regolatore::{DaMe, Mossa};
use rymembra_play::{RifiutoAvvio, tempo};
use tauri::{Manager as _, State};

use crate::nota;
use crate::spegnimento::Emette as _;
use crate::stato::{adesso_ms, con_libreria};

use super::coda::{gesto_di_coda, salva_coda};
use super::fili::prepara_prossimo;
use super::ospiti::{self, Chi, MANIGLIA_CALIBRAZIONE};
use super::{Lettore, StatoLettore, chiudi_ascolto, con_lettore, manda_stato, riapertura};

pub use super::ospiti::{Apertura, ApriInRete, SchedaVoce, Voce};

/// L'`uid` riservato alla voce di calibrazione: clic identici, generati uguali
/// su ogni PC, nessun byte in rete. Non sta mai in coda.
pub const UID_CALIBRAZIONE: u64 = u64::MAX;

/// Un brano di **questo** PC, con quel che serve per mandarlo in rete come
/// voce di sessione: chi ospita per i suoi, chi segue (fase 2) per quelli che
/// aggiunge.
#[derive(Debug, Clone)]
pub struct BranoDiQuesto {
    /// La riga della libreria.
    pub track_id: i64,
    /// Quel che serve a disegnarla.
    pub scheda: SchedaVoce,
    /// Il file, se è un file. `None` per un brano di catalogo.
    pub file: Option<FileDellaVoce>,
    /// Il riferimento, se è un brano di catalogo.
    ///
    /// La forma è `<fonte>:<indirizzo>`, con il nome stabile della fonte
    /// (`internet-archive:https://archive.org/download/…`): la scrive e la
    /// rilegge `riproduzione::flusso`, e chi la riceve la ripassa dalla propria
    /// lista dei cataloghi ammessi. Per la sessione è opaca: va sul filo così
    /// com'è.
    pub riferimento: Option<String>,
    /// L'impronta della copertina nella libreria, se ce n'è una.
    pub copertina: Option<String>,
}

/// Perché una voce non si può mandare: la risposta di [`voci_di_questo`].
///
/// Quattro casi perché sono quattro decisioni diverse per chi la manda: le
/// prime tre non cambiano riprovando, l'ultima sì.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mancanza {
    /// La voce non è un brano della libreria di questo PC: è di un altro, o
    /// è un brano di catalogo di qualcun altro, o il registro non la conosce.
    NonDiQuesto,
    /// La riga non c'è più in libreria: tolta o cancellata da quando è
    /// entrata in coda.
    RigaSparita,
    /// Il file non c'è più dove la riga dice (`NotFound`, che comprende la
    /// cartella sparita): riprovare non serve finché qualcuno non lo rimette.
    FileAssente,
    /// Il disco non ha risposto come si deve — un altro errore, la cartella di
    /// rete che non risponde — o la scadenza del blocco è arrivata prima di
    /// questa voce: riprovare più tardi può bastare.
    Irraggiungibile,
}

/// Il file di un [`BranoDiQuesto`].
#[derive(Debug, Clone)]
pub struct FileDellaVoce {
    /// Il percorso **del file da aprire**: per un brano d'immagine CD
    /// l'immagine, non il `…\Album.cue|03` che lo nomina in tabella.
    pub percorso: PathBuf,
    /// Quanti byte.
    pub dimensione: u64,
    /// L'ultima modifica, ms dall'epoca Unix.
    pub modificato_ms: i64,
    /// L'estensione.
    pub estensione: Option<String>,
    /// Il tratto, per un brano d'immagine CD: inizio e fine in ms.
    pub segmento: Option<(u64, Option<u64>)>,
}

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

// ── le regole ───────────────────────────────────────────────────────────────

/// La dissolvenza da dare al motore, in millisecondi: zero in sessione, la
/// scelta dell'utente fuori. Pura, per poterla provare: vedi `prove`.
const fn dissolvenza_per(in_sessione: bool, secondi: u64) -> u64 {
    if in_sessione {
        0
    } else {
        secondi.saturating_mul(1000)
    }
}

/// L'autoplay che vale: mai in sessione.
const fn autoplay_per(in_sessione: bool, voluto: bool) -> bool {
    voluto && !in_sessione
}

/// La coda si scrive sul disco: mai in sessione.
const fn coda_si_scrive_per(in_sessione: bool) -> bool {
    !in_sessione
}

/// La dissolvenza da dare al motore adesso, in millisecondi.
///
/// È l'unico posto da cui il motore la riceve: il comando, la coda di ieri,
/// la riapertura del dispositivo, l'ingresso e l'uscita. `Lettore::dissolvenza_s`
/// resta la scelta dell'utente — la mostra il cursore, e la rilegge l'uscita
/// dalla sessione — e la sessione decide solo quella **effettiva**.
pub(super) fn dissolvenza_ms(lettore: &Lettore) -> u64 {
    dissolvenza_per(lettore.sessione.is_some(), lettore.dissolvenza_s)
}

/// L'autoplay che vale adesso: vedi [`autoplay_per`].
pub(super) fn autoplay(lettore: &Lettore) -> bool {
    autoplay_per(lettore.sessione.is_some(), lettore.autoplay)
}

/// La coda si scrive sul disco adesso: vedi `coda::scrivi_coda`.
pub(super) fn coda_si_scrive(lettore: &Lettore) -> bool {
    coda_si_scrive_per(lettore.sessione.is_some())
}

/// Questo lettore ospita una sessione.
pub(super) fn ospita(lettore: &Lettore) -> bool {
    lettore
        .sessione
        .as_ref()
        .is_some_and(|sessione| sessione.ruolo == Ruolo::Ospita)
}

/// Questo lettore segue la sessione di un altro.
pub(super) fn segue(lettore: &Lettore) -> bool {
    lettore
        .sessione
        .as_ref()
        .is_some_and(|sessione| sessione.ruolo == Ruolo::Segue)
}

/// La calibrazione è accesa.
pub(super) fn in_calibrazione(lettore: &Lettore) -> bool {
    lettore
        .sessione
        .as_ref()
        .is_some_and(|sessione| sessione.calibrazione.is_some())
}

/// L'id è la voce di calibrazione.
pub(super) const fn e_la_calibrazione(id: i64) -> bool {
    id == MANIGLIA_CALIBRAZIONE
}

/// Il brano da mostrare nella barra: quello della coda, o la voce di
/// calibrazione mentre è lei a suonare.
pub(super) fn corrente_da_mostrare(lettore: &Lettore) -> Option<i64> {
    if lettore.motore.posizione().track_id == Some(MANIGLIA_CALIBRAZIONE) {
        return Some(MANIGLIA_CALIBRAZIONE);
    }
    lettore.coda.current()
}

/// Il punto da mostrare alla finestra al posto di quello del motore, in
/// sessione; `None` per quello del motore.
///
/// # Perché
///
/// Perché in sessione una voce nuova non parte subito: parte fra un quarto di
/// secondo (chi ospita) o quando il regolatore l'ha riaperta (chi segue), e
/// fino al primo campione il motore dice ancora il punto della voce di prima.
/// La coda invece è già sulla nuova, e la finestra disegnava il titolo nuovo
/// col cursore a 63 secondi su un brano appena cominciato. Fuori sessione la
/// stessa cosa dura qualche millisecondo, e la copre `manda_stato_con_posizione`;
/// qui dura abbastanza perché si veda, e passa anche dal tempo dell'orologio.
///
/// Due regole, nell'ordine:
///
/// - una partenza promessa per la voce che si mostra, finché il motore non
///   suona lei, finché l'istante non è passato, o finché il motore non è
///   davvero arrivato lì: il punto promesso (il salto, o l'inizio). Con le
///   cuffie Bluetooth il primo campione del salto arriva all'orecchio parecchio
///   dopo l'istante, e nel frattempo il motore dice ancora il punto di prima:
///   la barra tornava indietro per mezzo secondo e poi saltava avanti (visto
///   nelle prove a tre PC, a ogni salto chiesto da un membro);
/// - il motore su un'altra voce di quella che si mostra — lo specchio di chi
///   segue è già sulla voce dopo, il regolatore non l'ha ancora riaperta: zero,
///   l'inizio della voce che si vedrà.
pub(super) fn posizione_da_mostrare(
    lettore: &Lettore,
    posizione: &rymembra_play::Posizione,
) -> Option<u64> {
    let sessione = lettore.sessione.as_ref()?;
    let corrente = corrente_da_mostrare(lettore);
    punto_da_mostrare(
        sessione.programmata.as_ref(),
        corrente,
        posizione.track_id,
        posizione.ms,
        tempo::adesso_ns(),
    )
}

/// Quanto il motore può stare lontano dal punto promesso, già fatto avanzare
/// col tempo, per dirsi arrivato: un secondo e mezzo, il ritardo di un primo
/// campione lento più l'arrotondamento al millisecondo.
const ARRIVATO_ENTRO_MS: u64 = 1_500;

/// La regola di [`posizione_da_mostrare`], senza motore e senza orologio.
fn punto_da_mostrare(
    promessa: Option<&Programmata>,
    corrente: Option<i64>,
    nel_motore: Option<i64>,
    motore_ms: u64,
    adesso_ns: u64,
) -> Option<u64> {
    // Una promessa vale al più fino alla sua scadenza: chi segue la fissa
    // riaprendo e non la toglie mai (la toglie l'ancora di chi ospita), e una
    // promessa vecchia di minuti teneva la barra di chi segue ferma sul punto
    // di quando era entrato, con la musica che andava.
    if let Some(promessa) = promessa
        && promessa.maniglia.is_some()
        && promessa.maniglia == corrente
        && adesso_ns <= promessa.quando_ns.saturating_add(SCADENZA_PROGRAMMATA_NS)
    {
        let promesso_ms = promessa.posizione_us.checked_div(1000).unwrap_or(0);
        let passati_ms = adesso_ns
            .saturating_sub(promessa.quando_ns)
            .checked_div(1_000_000)
            .unwrap_or(0);
        let arrivato = nel_motore == promessa.maniglia
            && adesso_ns >= promessa.quando_ns
            && motore_ms.abs_diff(promesso_ms.saturating_add(passati_ms)) <= ARRIVATO_ENTRO_MS;
        if !arrivato {
            return Some(promesso_ms);
        }
    }
    if corrente.is_some() && nel_motore.is_some() && nel_motore != corrente {
        return Some(0);
    }
    None
}

/// Chi segue: il cursore della coda sulla voce che il motore ha cominciato,
/// se è nello specchio. Vedi il ramo di chi segue in `iniziato`.
pub(super) fn cursore_sulla_voce(lettore: &mut Lettore, maniglia: i64) {
    if lettore.coda.current() == Some(maniglia) {
        return;
    }
    if let Some(posto) = lettore
        .coda
        .in_play_order()
        .iter()
        .position(|id| *id == maniglia)
    {
        lettore.coda.play_at(posto);
    }
}

/// Quanti dei brani da accodare entrano nella coda di sessione, che ha un
/// tetto: [`MASSIMO_VOCI`]. Fuori sessione tutti.
///
/// # Perché il tetto si fa rispettare qui
///
/// Perché è un tetto **del protocollo**: lo stato della sessione con più voci
/// di così, ogni membro lo rifiuta con `insieme.protocollo` e chiude la
/// connessione. Un brano accodato in più costerebbe a tutti gli altri la
/// sessione. Quelli che entrano entrano, il gesto non fallisce, e per quelli
/// che restano fuori la finestra riceve un avviso: vedi [`avvisa_coda_piena`].
pub(super) fn entrano<'a>(
    app: &tauri::AppHandle,
    lettore: &Lettore,
    brani: &'a [i64],
) -> &'a [i64] {
    if lettore.sessione.is_none() {
        return brani;
    }
    let posto = MASSIMO_VOCI.saturating_sub(lettore.coda.len());
    if brani.len() > posto {
        // Sul filo del seguito: qui il lucchetto del lettore è in mano, e il
        // diario scrive sul disco.
        let fuori = brani.len().saturating_sub(posto);
        super::in_ordine(app, move |app| avvisa_coda_piena(app, fuori));
    }
    brani.get(..posto.min(brani.len())).unwrap_or_default()
}

/// Dei brani sono rimasti fuori dalla coda di sessione: una riga nel diario
/// (il conto, non i brani) e un avviso alla finestra, con la chiave di
/// `insieme.codaPiena`. Senza lucchetti in mano.
fn avvisa_coda_piena(app: &tauri::AppHandle, fuori: usize) {
    nota!("[insieme] coda di sessione piena: {fuori} brani lasciati fuori");
    app.emetti(
        "insieme:avviso",
        crate::insieme::AvvisoIpc {
            chiave: "errors.insieme.codaPiena".to_owned(),
            nome: None,
        },
    );
}

/// Quel che entra in coda per un gesto che accoda dei brani, deciso **sotto il
/// lucchetto del lettore**: la lista, e le voci da dimenticare.
///
/// # La gara che chiude
///
/// La strada del gesto si chiede prima del lucchetto, e dal ruolo della
/// sessione: nell'istante fra il ruolo che diventa «ospita» e il lettore che
/// entra davvero in sessione, la strada crea già le voci e dà le maniglie
/// (`ConVoci`), ma la coda è ancora quella dell'utente. Messe lì, le maniglie
/// sarebbero tornate all'uscita come voci sparite, e una coda con una maniglia
/// non si salva più.
///
/// Qui si decide guardando il lettore, che è la verità: se ospita, le voci; se
/// no, i brani chiesti — il gesto era dell'utente, e fuori sessione è
/// esattamente quel che avrebbe fatto — e le voci create per niente si
/// dimenticano. Un errore ritentabile sarebbe stato l'altra scelta: avrebbe
/// fatto ripetere all'utente un gesto per una gara che non può vedere.
pub(super) fn brani_da_mettere(
    ospita: bool,
    righe: Vec<i64>,
    voci: Option<Vec<i64>>,
) -> (Vec<i64>, Vec<i64>) {
    match voci {
        None => (righe, Vec::new()),
        Some(maniglie) if ospita => (maniglie, Vec::new()),
        Some(maniglie) => (righe, maniglie),
    }
}

/// Come [`brani_da_mettere`], per «suona», che porta anche il brano da cui
/// partire: la lista, il punto di partenza, e le voci da dimenticare.
///
/// # L'indice
///
/// Le voci non sono per forza tante quante le righe: [`voci_da_libreria`]
/// salta quelle che la libreria non ha più. L'indice si ricalcola quindi sulla
/// voce del brano scelto — la stessa occorrenza, se il brano è in lista due
/// volte — e se quella manca, o se non c'è nessuna voce, il gesto si rifiuta
/// con `library.trackNotFound` **senza toccare** la coda di tutti: suonare un
/// altro brano, o svuotare la coda, sarebbe un gesto che nessuno ha fatto.
///
/// `righe_delle_voci` è la riga dietro ogni maniglia, dal registro.
pub(super) fn lista_da_suonare(
    ospita: bool,
    righe: Vec<i64>,
    indice: usize,
    voci: Option<VociDaSuonare>,
) -> (Partenza, Vec<i64>) {
    match voci {
        None => (Ok((righe, indice)), Vec::new()),
        Some((maniglie, _)) if !ospita => (Ok((righe, indice)), maniglie),
        Some((maniglie, righe_delle_voci)) => {
            match indice_fra_le_voci(&righe, indice, &righe_delle_voci) {
                Some(da) => (Ok((maniglie, da)), Vec::new()),
                None => (
                    Err(AppError::new(ErrorCode::LibraryTrackNotFound {
                        track_id: righe.get(indice).copied(),
                    })
                    .with_cause("il brano scelto non è diventato una voce della sessione")),
                    maniglie,
                ),
            }
        }
    }
}

/// Le maniglie di un «suona» e, per ciascuna, la riga che apre.
pub(super) type VociDaSuonare = (Vec<i64>, Vec<Option<i64>>);

/// La lista da suonare e il punto da cui partire, o il rifiuto.
pub(super) type Partenza = Result<(Vec<i64>, usize), AppError>;

/// Dove sta, fra le voci, il brano che stava a `indice` fra le righe: la
/// stessa occorrenza della stessa riga. `None` se non è diventato una voce.
fn indice_fra_le_voci(
    righe: &[i64],
    indice: usize,
    righe_delle_voci: &[Option<i64>],
) -> Option<usize> {
    let scelta = *righe.get(indice)?;
    let occorrenza = righe
        .get(..=indice)?
        .iter()
        .filter(|riga| **riga == scelta)
        .count();
    righe_delle_voci
        .iter()
        .enumerate()
        .filter(|(_, riga)| **riga == Some(scelta))
        .nth(occorrenza.checked_sub(1)?)
        .map(|(posto, _)| posto)
}

/// La riga dietro ogni maniglia, dal registro: vedi [`lista_da_suonare`].
pub(super) fn righe_delle_voci(app: &tauri::AppHandle, maniglie: &[i64]) -> Vec<Option<i64>> {
    ospiti::con_registro(app, |registro| {
        maniglie
            .iter()
            .map(|maniglia| {
                registro
                    .voce(*maniglia)
                    .and_then(|voce| voce.apertura_di_libreria())
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Toglie dal registro voci che nessuna coda nominerà. Il registro è un
/// lucchetto foglia: si può chiamare col lettore in mano.
pub(super) fn dimentica_voci(app: &tauri::AppHandle, maniglie: &[i64]) {
    if maniglie.is_empty() {
        return;
    }
    ospiti::con_registro(app, |registro| registro.togli(maniglie));
}

/// La coda dell'utente senza maniglie: quelle che una gara d'ingresso ci
/// avesse lasciato. Il cursore resta dov'era, o scivola sul brano dopo come
/// per ogni brano tolto.
fn ripulita(istantanea: QueueSnapshot) -> QueueSnapshot {
    let maniglie: Vec<i64> = istantanea
        .tracks
        .iter()
        .copied()
        .filter(|id| !ospiti::e_di_libreria(*id))
        .collect();
    if maniglie.is_empty() {
        return istantanea;
    }
    let mut coda = Queue::restore(istantanea);
    coda.remove_tracks(&maniglie);
    coda.snapshot()
}

/// La finestra di brani attorno a `indice`, se si è in sessione, per un gesto
/// che sostituisce la coda: vedi [`finestra`]. Fuori sessione tutti.
///
/// Si legge il ruolo della sessione e non il lettore: si chiama prima della
/// strada del gesto, senza lucchetti, e la strada stessa decide dal ruolo.
pub(super) fn finestra_di_sessione(
    app: &tauri::AppHandle,
    brani: Vec<i64>,
    indice: usize,
) -> (Vec<i64>, usize) {
    let ruolo = crate::insieme::ruolo(app);
    if ruolo == crate::insieme::Ruolo::Fuori {
        return (brani, indice);
    }
    // L'avviso solo per chi ospita, che il gesto lo fa davvero: per chi segue
    // il gesto va all'host, o si rifiuta, e i suoi tetti sono altri.
    if ruolo == crate::insieme::Ruolo::Host && brani.len() > MASSIMO_VOCI {
        avvisa_coda_piena(app, brani.len().saturating_sub(MASSIMO_VOCI));
    }
    let (finestra, dove) = finestra(&brani, Some(indice), MASSIMO_VOCI);
    (finestra, dove.unwrap_or(0))
}

/// Al più `tetto` brani attorno al `posto`: qualcuno prima, perché «precedente»
/// abbia dove andare, e il resto dopo, che è quel che si ascolterà.
///
/// Un decimo del tetto prima, al più. Una coda da cinquantamila brani — «Tutti
/// i brani» — entra in una sessione con i mille che cominciano da quello che
/// suona, che è quel che chi la condivide si aspetta di sentire.
fn finestra(ordine: &[i64], posto: Option<usize>, tetto: usize) -> (Vec<i64>, Option<usize>) {
    if ordine.len() <= tetto {
        return (ordine.to_vec(), posto);
    }
    let qui = posto.unwrap_or(0).min(ordine.len().saturating_sub(1));
    let indietro = qui.min(tetto.checked_div(10).unwrap_or(0));
    let fine = qui
        .saturating_sub(indietro)
        .saturating_add(tetto)
        .min(ordine.len());
    let inizio = fine.saturating_sub(tetto);
    let pezzo = ordine.get(inizio..fine).unwrap_or_default().to_vec();
    (pezzo, posto.map(|_| qui.saturating_sub(inizio)))
}

// ── il motore in sessione ───────────────────────────────────────────────────

/// Fa partire `brano` dal millisecondo `da_ms`: chi ospita fra [`ANTICIPO_NS`]
/// e annunciandolo, tutti gli altri subito. Vedi «Chi ospita parte fra un
/// quarto di secondo» nel `//!`.
pub(super) fn parti(lettore: &mut Lettore, brano: rymembra_play::BranoAperto, da_ms: u64) {
    if !ospita(lettore) {
        lettore.suona_da(brano, da_ms);
        return;
    }
    let etichetta = brano.etichetta();
    let maniglia = brano.track_id();
    lettore.apri_fermo(brano, da_ms);
    programma(
        lettore,
        Some(maniglia),
        etichetta,
        Some(da_ms.saturating_mul(1000)),
    );
}

/// Riprende: chi ospita fra [`ANTICIPO_NS`], dal punto in cui il motore
/// aspetta; tutti gli altri subito.
///
/// Solo da fermi: un motore che sta già suonando non ha niente da
/// programmare, e un avvio programmato chiesto lì aprirebbe un buco di un
/// quarto di secondo dove non serviva.
pub(super) fn riprendi(lettore: &mut Lettore) {
    if !ospita(lettore) || !lettore.motore.posizione().in_pausa {
        lettore.motore.riprendi();
        return;
    }
    let maniglia = lettore.motore.posizione().track_id;
    let attesa = lettore.motore.in_attesa();
    programma(
        lettore,
        maniglia,
        attesa.and_then(|(etichetta, _)| etichetta),
        attesa.map(|(_, posizione_us)| posizione_us),
    );
}

/// Salta a `ms`: chi ospita, mentre suona, ferma, salta e riparte fra
/// [`ANTICIPO_NS`]; tutti gli altri — e chi ospita da fermo — saltano e
/// basta. Vedi «Chi ospita parte fra un quarto di secondo».
///
/// Pausa, salto e ripresa programmata, in quest'ordine, sono la forma che il
/// motore vuole: un avvio programmato chiesto mentre suona non ferma niente, e
/// un salto a musica in corso farebbe sentire il punto d'arrivo prima degli
/// altri.
pub(super) fn salta(lettore: &mut Lettore, ms: u64) {
    let suona = !lettore.motore.posizione().in_pausa;
    if !ospita(lettore) || !suona || lettore.motore.posizione().track_id.is_none() {
        lettore.motore.vai_a(ms);
        return;
    }
    // L'etichetta si legge prima della pausa, finché l'ancora la dice: dopo il
    // salto non la direbbe più nessuno fino al primo blocco nuovo.
    let etichetta = lettore
        .motore
        .ancora()
        .and_then(|ancora| ancora.etichetta)
        .or_else(|| {
            lettore
                .motore
                .in_attesa()
                .and_then(|(etichetta, _)| etichetta)
        });
    let maniglia = lettore.motore.posizione().track_id;
    lettore.motore.pausa();
    lettore.motore.vai_a(ms);
    programma(lettore, maniglia, etichetta, Some(ms.saturating_mul(1000)));
}

/// La ripresa programmata vera e propria, e l'annuncio per l'ancora.
///
/// Se il motore la rifiuta — il flusso riposava, cosa che in sessione non
/// dovrebbe succedere perché all'ingresso lo si tiene sveglio — si riparte
/// subito, come fuori sessione: meglio un brano che arriva agli altri un
/// quarto di secondo dopo che un brano che non parte.
fn programma(
    lettore: &mut Lettore,
    maniglia: Option<i64>,
    etichetta: Option<u64>,
    posizione_us: Option<u64>,
) {
    let quando_ns = tempo::adesso_ns().saturating_add(ANTICIPO_NS);
    match lettore.motore.riprendi_a(quando_ns) {
        Ok(()) => {
            if let Some(sessione) = lettore.sessione.as_mut() {
                sessione.programmata = posizione_us.map(|posizione_us| Programmata {
                    maniglia,
                    etichetta,
                    posizione_us,
                    quando_ns,
                });
            }
        }
        Err(rifiuto) => {
            nota!("[insieme] avvio programmato rifiutato ({rifiuto:?}): si parte subito");
            lettore.motore.riprendi();
        }
    }
}

/// La latenza dichiarata di quest'uscita è cambiata, in sessione: la musica
/// si sposta subito della stessa quantità, all'indietro.
///
/// # Perché qui e non dal regolatore
///
/// Perché la latenza dichiarata non sposta il suono, sposta l'istante in cui
/// il motore **crede** di essere sentito, e con lui le ancore. Lasciato al
/// regolatore, un millisecondo in più diventerebbe un errore di un
/// millisecondo, che la deriva toglie in una ventina di secondi (50 ppm per
/// ms): calibrare a orecchio, un millisecondo per volta, vorrebbe dire
/// aspettare venti secondi dopo ogni tacca del cursore per sentire che cosa ha
/// fatto. Un gradino uguale e contrario sposta il suono nell'istante, e lascia
/// l'errore misurato dov'era: posizione e istante si muovono insieme, il
/// regolatore non vede niente da correggere.
///
/// Il segno è quello del motore: dichiarare più latenza vuol dire che il
/// suono arriva più tardi di quel che si credeva, quindi si salta avanti
/// (gradino negativo). Da fermi la callback i gradini li lascia cadere, ed è
/// giusto: alla ripresa l'avvio programmato tiene già conto del numero nuovo.
pub(super) fn latenza_cambiata(lettore: &Lettore, prima_ms: i64, dopo_ms: i64) {
    if lettore.sessione.is_none() || lettore.motore.posizione().in_pausa {
        return;
    }
    let differenza_ms = i128::from(dopo_ms) - i128::from(prima_ms);
    let fotogrammi = differenza_ms
        .saturating_mul(i128::from(lettore.motore.formato().frequenza))
        .checked_div(1000)
        .unwrap_or(0);
    if let Ok(fotogrammi) = i64::try_from(-fotogrammi) {
        lettore.motore.gradino(fotogrammi);
    }
}

/// Una pausa butta l'avvio programmato: l'annuncio parlerebbe di una musica
/// che non partirà.
pub(super) fn dopo_una_pausa(lettore: &mut Lettore) {
    if let Some(sessione) = lettore.sessione.as_mut() {
        sessione.programmata = None;
    }
}

/// Un brano nuovo dato al motore, o il motore fermato: l'avvio programmato e
/// la calibrazione erano di quel che c'era prima. Lo chiama
/// `Lettore::dopo_un_avvio`, che è la strada di ogni partenza; chi programma
/// o calibra riscrive i suoi dopo.
pub(super) fn dopo_un_avvio(lettore: &mut Lettore) {
    if let Some(sessione) = lettore.sessione.as_mut() {
        sessione.programmata = None;
        sessione.calibrazione = None;
    }
}

/// Il motore è stato sostituito — un'uscita nuova, un dispositivo tornato —
/// e il nuovo nasce senza le regole della sessione: si rimettono. La
/// dissolvenza la rimette già chi lo sostituisce, con [`dissolvenza_ms`].
///
/// E la sessione lo deve sapere: l'uscita nuova è un'altra scheda con un
/// altro quarzo, e la deriva che il regolatore di chi segue aveva imparato
/// era di quella di prima (`Regolatore::azzera`). L'avviso non blocca, e si
/// può dare col lucchetto del lettore in mano: vedi
/// `crate::insieme::uscita_cambiata`.
pub(super) fn motore_sostituito(app: &tauri::AppHandle, lettore: &mut Lettore) {
    let Some(sessione) = lettore.sessione.as_mut() else {
        return;
    };
    sessione.programmata = None;
    lettore.motore.tieni_sveglio(true);
    crate::insieme::uscita_cambiata(app);
}

/// Chi ospita: avvisa la sessione che il lettore è cambiato. Lo chiama
/// `manda_stato`, che è dove passa ogni cambiamento. Non blocca.
pub(super) fn cambiato(app: &tauri::AppHandle, lettore: &Lettore) {
    if ospita(lettore) {
        crate::insieme::lettore_cambiato(app);
    }
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
    // Il flusso non riposa mai, in sessione: `riprendi_a` lo vuole sveglio, e
    // svegliarlo all'ultimo costerebbe proprio i millisecondi che l'avvio
    // programmato esiste per non perdere. Il risveglio vero l'ha già fatto chi
    // chiama, a lucchetto lasciato: qui `tieni_sveglio` lo trova sveglio.
    lettore.motore.tieni_sveglio(true);
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

/// I brani di libreria di questo PC diventano voci di sessione: restituisce le
/// loro maniglie, nell'ordine. È la strada di `Instradato::ConVoci`.
///
/// Al più [`MASSIMO_VOCI`]: oltre, la sessione non le potrebbe mandare.
/// Quelli che la libreria non ha più si saltano.
///
/// # Errori
///
/// Quelli della libreria.
pub fn voci_da_libreria(app: &tauri::AppHandle, ids: &[i64]) -> Result<Vec<i64>, AppError> {
    Ok(voci_allineate(app, ids)?.into_iter().flatten().collect())
}

/// Come [`voci_da_libreria`], ma allineate agli id chiesti: `None` dove la
/// riga non c'è. Serve a chi deve ritrovare la voce di un posto preciso — il
/// brano corrente, entrando.
fn voci_allineate(app: &tauri::AppHandle, ids: &[i64]) -> Result<Vec<Option<i64>>, AppError> {
    let ids = ids.get(..ids.len().min(MASSIMO_VOCI)).unwrap_or_default();
    let righe: Vec<i64> = ids
        .iter()
        .copied()
        .filter(|id| ospiti::e_di_libreria(*id))
        .collect();
    let schede = if righe.is_empty() {
        std::collections::HashMap::new()
    } else {
        let stato = super::libreria_di(app, "voci di sessione")?;
        con_libreria(&stato, |libreria| {
            leggi_schede(&libreria.connection, &righe)
        })?
    };
    ospiti::con_registro(app, |registro| {
        ids.iter()
            .map(|id| {
                let scheda = schede.get(id)?.clone();
                let uid = registro.nuovo_uid();
                Some(registro.registra(Voce {
                    uid,
                    proprietario: None,
                    scheda,
                    apertura: Apertura::Libreria { track_id: *id },
                    da_contare: Some(*id),
                }))
            })
            .collect()
    })
    .ok_or_else(|| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some("voci di sessione".to_owned()),
        })
    })
}

/// Le schede di alcune righe, dal database, con un'occhiata sola per tutte
/// le righe e una per brano per il livello.
///
/// Il livello è quello del brano (`scheda_sorgente`), non quello scelto dalla
/// normalizzazione di questo PC: viaggia agli altri, e ciascuno lo usa con la
/// sua normalizzazione. Il livello d'album non c'è perché dipende dai vicini
/// in coda, cioè da una scelta di chi ascolta.
fn leggi_schede(
    connection: &rusqlite::Connection,
    righe: &[i64],
) -> Result<std::collections::HashMap<i64, SchedaVoce>, AppError> {
    let lette = rymembra_app::library::summaries_by_id(connection, righe)?;
    Ok(lette
        .into_iter()
        .map(|riga| {
            let livello = rymembra_app::playback::scheda_sorgente(connection, riga.id).ok();
            (riga.id, scheda_da_riga(&riga, livello.as_ref()))
        })
        .collect())
}

/// La scheda di una voce, da una riga della libreria e dal suo livello.
#[expect(
    clippy::cast_possible_truncation,
    reason = "decibel e un picco lineare: la precisione di un f32 basta e avanza, ed è quella \
              che il motore usa"
)]
fn scheda_da_riga(riga: &TrackSummary, livello: Option<&SchedaSorgente>) -> SchedaVoce {
    SchedaVoce {
        titolo: riga.title.clone(),
        artista: riga.artist.clone(),
        album: riga.album.clone(),
        durata_ms: u64::try_from(riga.duration_ms).unwrap_or(0),
        numero: riga.track_number.and_then(|n| u32::try_from(n).ok()),
        anno: riga.year.and_then(|anno| i32::try_from(anno).ok()),
        copertina: riga.cover_art_hash.clone(),
        replaygain_db: livello.and_then(|l| l.replaygain_db).map(|db| db as f32),
        picco: livello.and_then(|l| l.picco).map(|picco| picco as f32),
    }
}

/// Fase 2, chi ospita: mette in coda voci già registrate (quelle di un
/// membro), in fondo o subito dopo la corrente.
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
) -> Result<(), AppError> {
    let stato = stato_lettore(app)?;
    gesto_di_coda(app, &stato, |lettore| {
        if !ospita(lettore) {
            return Err(non_disponibile("accoda"));
        }
        let era_vuota = lettore.coda.is_empty();
        let entrano = entrano(app, lettore, maniglie);
        if dopo_la_corrente {
            lettore.coda.play_next(entrano);
        } else {
            lettore.coda.enqueue(entrano);
        }
        if era_vuota {
            return Ok(true);
        }
        prepara_prossimo(app, lettore);
        salva_coda(app, lettore);
        manda_stato(app, lettore);
        Ok(false)
    })
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

/// I dettagli delle voci che sono brani di questo PC, per mandarle in rete, in
/// blocco e col perché: per ogni `uid`, nello stesso ordine e uno per uno, il
/// brano da mandare o la [`Mancanza`]. Legge il database e il disco, come
/// [`brani_di_questo`]: fuori dal lucchetto del lettore.
///
/// # Perché in blocco
///
/// Perché una voce per volta voleva dire un filo, una scadenza e una lettura
/// del disco per voce: con la coda di un NAS dove trentotto file non c'erano
/// più, trentotto attese, e alla fine un `None` che non diceva se riprovare o
/// rinunciare. Qui il registro si guarda una volta, il database una volta, e il
/// disco su un filo solo con **una** scadenza per tutto il blocco — quella dei
/// flussi. I brani di catalogo il disco non lo toccano.
///
/// # Come si classifica
///
/// - nel registro la voce non apre una riga di questo PC: [`Mancanza::NonDiQuesto`];
/// - la riga non c'è più: [`Mancanza::RigaSparita`];
/// - il file risponde `NotFound`: [`Mancanza::FileAssente`];
/// - ogni altro errore del disco, o la scadenza arrivata prima di quella voce:
///   [`Mancanza::Irraggiungibile`]. Anche il database che non risponde dà
///   questo, per tutte: è un «non adesso», non un «no».
#[must_use]
pub fn voci_di_questo(
    app: &tauri::AppHandle,
    uids: &[u64],
) -> Vec<(u64, Result<BranoDiQuesto, Mancanza>)> {
    let righe = ospiti::con_registro(app, |registro| righe_dei_uid(registro, uids))
        .unwrap_or_else(|| uids.iter().map(|uid| (*uid, None)).collect());
    let da_leggere: Vec<i64> = righe.iter().filter_map(|(_, riga)| *riga).collect();
    let lette = leggi_brani(app, &da_leggere).unwrap_or_else(|err| {
        nota!(
            "[insieme] voci da mandare non lette: {}",
            err.code().kind().code()
        );
        vec![Err(Mancanza::Irraggiungibile); da_leggere.len()]
    });
    let mut lette = lette.into_iter();
    righe
        .into_iter()
        .map(|(uid, riga)| {
            let esito = match riga {
                None => Err(Mancanza::NonDiQuesto),
                Some(_) => lette.next().unwrap_or(Err(Mancanza::Irraggiungibile)),
            };
            (uid, esito)
        })
        .collect()
}

/// La riga della libreria di questo PC dietro ogni `uid`, nell'ordine: solo
/// le voci di questo PC che si aprono da una riga. Una voce di un altro PC,
/// un `uid` che il registro non conosce, la calibrazione: `None`.
fn righe_dei_uid(registro: &ospiti::Registro, uids: &[u64]) -> Vec<(u64, Option<i64>)> {
    uids.iter()
        .map(|uid| {
            let riga = registro
                .maniglia(*uid)
                .and_then(|maniglia| registro.voce(maniglia))
                .filter(|voce| voce.proprietario.is_none())
                .and_then(|voce| voce.apertura_di_libreria())
                .filter(|riga| ospiti::e_di_libreria(*riga));
            (*uid, riga)
        })
        .collect()
}

/// I dettagli di brani della libreria di questo PC, nell'ordine; quelli che
/// non ci sono più si saltano. Legge il database e il disco (dimensione e
/// data del file): fuori dal lucchetto del lettore.
///
/// # Il disco, con una scadenza
///
/// Dimensione e data si chiedono al file system, e un file su una cartella di
/// rete che dorme può farsi aspettare quanto Windows vuole. Si chiedono tutte
/// su un filo solo con la scadenza dei flussi: se passa, quelli già letti si
/// tengono e quelli non letti si saltano, come quelli che non ci sono — la
/// sessione non li può servire, e il servente rileggerebbe comunque dimensione
/// e data a ogni lettura. I brani di catalogo il disco non lo toccano, e non
/// aspettano.
///
/// # Errori
///
/// Quelli della libreria.
pub fn brani_di_questo(
    app: &tauri::AppHandle,
    ids: &[i64],
) -> Result<Vec<BranoDiQuesto>, AppError> {
    let righe: Vec<i64> = ids
        .iter()
        .copied()
        .filter(|id| ospiti::e_di_libreria(*id))
        .collect();
    Ok(leggi_brani(app, &righe)?
        .into_iter()
        .filter_map(Result::ok)
        .collect())
}

/// Una riga letta dal database, pronta per il disco.
type Letta = (TrackSummary, SchedaSorgente);

/// Il cuore di [`brani_di_questo`] e [`voci_di_questo`]: per ogni riga,
/// nell'ordine e una per una, il brano o il perché no. Il database con una
/// presa sola, il disco con una scadenza sola.
///
/// # Errori
///
/// Quelli della libreria.
fn leggi_brani(
    app: &tauri::AppHandle,
    righe: &[i64],
) -> Result<Vec<Result<BranoDiQuesto, Mancanza>>, AppError> {
    if righe.is_empty() {
        return Ok(Vec::new());
    }
    let stato = super::libreria_di(app, "brani da mandare in sessione")?;
    let lette = con_libreria(&stato, |libreria| leggi_righe(&libreria.connection, righe))?;
    Ok(classifica(
        lette,
        super::APERTURA_FLUSSO,
        |percorso: &Path| std::fs::metadata(percorso),
    ))
}

/// Le righe dal database, nell'ordine chiesto e coi doppioni: `None` per una
/// riga che non c'è più (o che non si sa più aprire).
fn leggi_righe(
    connection: &rusqlite::Connection,
    righe: &[i64],
) -> Result<Vec<Option<Letta>>, AppError> {
    let schede = rymembra_app::library::summaries_by_id(connection, righe)?;
    let per_id: std::collections::HashMap<i64, TrackSummary> =
        schede.into_iter().map(|riga| (riga.id, riga)).collect();
    Ok(righe
        .iter()
        .map(|id| {
            let riga = per_id.get(id)?.clone();
            let sorgente = rymembra_app::playback::scheda_sorgente(connection, *id).ok()?;
            Some((riga, sorgente))
        })
        .collect())
}

/// Dalle righe lette ai brani, col disco su un filo solo e una scadenza sola
/// per tutto il blocco.
///
/// Il filo mette da parte quel che legge man mano: alla scadenza si tiene
/// quel che c'è, e le voci non ancora lette sono [`Mancanza::Irraggiungibile`]
/// — il filo, se è ancora appeso a una cartella, finisce da sé e quel che
/// aggiunge dopo non lo legge più nessuno. I brani di catalogo si fanno prima e
/// subito: il disco non lo toccano, e non aspettano dietro a un NAS che dorme.
///
/// `stat` è la lettura del disco: `std::fs::metadata`, o una finta nelle prove.
fn classifica<S>(
    lette: Vec<Option<Letta>>,
    scadenza: std::time::Duration,
    stat: S,
) -> Vec<Result<BranoDiQuesto, Mancanza>>
where
    S: Fn(&Path) -> std::io::Result<std::fs::Metadata> + Send + 'static,
{
    let mut esiti: Vec<Option<Result<BranoDiQuesto, Mancanza>>> = vec![None; lette.len()];
    let mut sul_disco = Vec::new();
    for (posto, letta) in lette.into_iter().enumerate() {
        let esito = match letta {
            None => Err(Mancanza::RigaSparita),
            Some((riga, sorgente)) => {
                if matches!(sorgente.collocazione, Collocazione::Catalogo { .. }) {
                    brano_di_questo(&riga, &sorgente, &stat)
                } else {
                    sul_disco.push((posto, riga, sorgente));
                    continue;
                }
            }
        };
        if let Some(casella) = esiti.get_mut(posto) {
            *casella = Some(esito);
        }
    }
    if !sul_disco.is_empty() {
        let quanti = sul_disco.len();
        let raccolti = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let nel_filo = std::sync::Arc::clone(&raccolti);
        let esito =
            rymembra_app::scadenza::con_scadenza_o_caduta("insieme-file", scadenza, move || {
                for (posto, riga, sorgente) in sul_disco {
                    let esito = brano_di_questo(&riga, &sorgente, &stat);
                    nel_filo
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push((posto, esito));
                }
            });
        let letti = std::mem::take(
            &mut *raccolti
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        match esito {
            Ok(()) => {}
            Err(rymembra_app::scadenza::Mancato::Scaduto) => nota!(
                "[insieme] file da servire: {} su {quanti} letti entro {} ms, gli altri \
                 irraggiungibili",
                letti.len(),
                scadenza.as_millis()
            ),
            Err(rymembra_app::scadenza::Mancato::Caduto { .. }) => nota!(
                "[insieme] file da servire: la lettura è caduta dopo {} su {quanti}, gli altri \
                 irraggiungibili",
                letti.len()
            ),
        }
        for (posto, esito) in letti {
            if let Some(casella) = esiti.get_mut(posto) {
                *casella = Some(esito);
            }
        }
    }
    esiti
        .into_iter()
        .map(|esito| esito.unwrap_or(Err(Mancanza::Irraggiungibile)))
        .collect()
}

/// Un [`BranoDiQuesto`] da una riga e dalla sua collocazione, o il perché
/// il suo file non si può servire.
fn brano_di_questo(
    riga: &TrackSummary,
    sorgente: &SchedaSorgente,
    stat: &dyn Fn(&Path) -> std::io::Result<std::fs::Metadata>,
) -> Result<BranoDiQuesto, Mancanza> {
    let scheda = scheda_da_riga(riga, Some(sorgente));
    let (file, riferimento) = match &sorgente.collocazione {
        Collocazione::File(percorso) => (Some(file_della_voce(percorso, None, stat)?), None),
        Collocazione::Segmento {
            immagine,
            inizio_cd,
            fine_cd,
        } => {
            let tratto = (ms_da_cd(*inizio_cd), fine_cd.map(ms_da_cd));
            (Some(file_della_voce(immagine, Some(tratto), stat)?), None)
        }
        Collocazione::Catalogo { fonte, url } => {
            (None, Some(super::flusso::riferimento_in_testo(*fonte, url)))
        }
    };
    Ok(BranoDiQuesto {
        track_id: riga.id,
        scheda,
        file,
        riferimento,
        copertina: riga.cover_art_hash.clone(),
    })
}

/// Il file di una voce, letto adesso dal disco. `NotFound` — il file o la sua
/// cartella non ci sono — è [`Mancanza::FileAssente`], e lo è anche un
/// percorso che non è un file; ogni altro errore è
/// [`Mancanza::Irraggiungibile`].
fn file_della_voce(
    percorso: &str,
    segmento: Option<(u64, Option<u64>)>,
    stat: &dyn Fn(&Path) -> std::io::Result<std::fs::Metadata>,
) -> Result<FileDellaVoce, Mancanza> {
    let percorso = PathBuf::from(percorso);
    let dati = stat(&percorso).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            Mancanza::FileAssente
        } else {
            Mancanza::Irraggiungibile
        }
    })?;
    if !dati.is_file() {
        return Err(Mancanza::FileAssente);
    }
    let modificato_ms = dati
        .modified()
        .ok()
        .and_then(|quando| quando.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0);
    Ok(FileDellaVoce {
        estensione: estensione_di(&percorso),
        dimensione: dati.len(),
        modificato_ms,
        segmento,
        percorso,
    })
}

/// L'estensione di un file, in minuscolo: il suggerimento per il
/// riconoscitore del formato sull'altro PC.
fn estensione_di(percorso: &Path) -> Option<String> {
    percorso
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
}

/// Fotogrammi del CD (1/75 s) in millisecondi, arrotondati: il contrario di
/// `ospiti::tratto`, e il giro è esatto.
fn ms_da_cd(cd: u32) -> u64 {
    u64::from(cd)
        .saturating_mul(2000)
        .saturating_add(75)
        .checked_div(150)
        .unwrap_or(0)
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
        lettore.coda = Queue::restore(nuova);
        prepara_prossimo(app, lettore);
        manda_stato(app, lettore);
        Ok(())
    });
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
        Err(RifiutoAvvio::FlussoARiposo) => {
            // In sessione il flusso è tenuto sveglio: se riposa, il motore è
            // nuovo e non lo sa ancora. Si riparte subito, e il regolatore
            // misura e corregge.
            nota!("[insieme] il flusso riposava: si riparte senza programmare");
            lettore.motore.tieni_sveglio(true);
            lettore.motore.riprendi();
            None
        }
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
        match lettore
            .motore
            .riprendi_a(quando_ns.saturating_sub(avanzo_ns))
        {
            Ok(()) => {}
            Err(RifiutoAvvio::NelPassato) => {
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
            Err(RifiutoAvvio::FlussoARiposo) => {
                lettore.motore.tieni_sveglio(true);
                lettore.motore.riprendi();
                promessa.quando_ns = tempo::adesso_ns();
            }
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
        lettore.motore.tieni_sveglio(false);
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
mod prove {
    use super::*;

    #[test]
    fn in_sessione_la_dissolvenza_e_zero_e_fuori_e_quella_scelta() {
        assert_eq!(dissolvenza_per(true, 8), 0);
        assert_eq!(dissolvenza_per(false, 8), 8000);
        assert_eq!(dissolvenza_per(false, 0), 0);
    }

    #[test]
    fn in_sessione_niente_autoplay_e_la_coda_non_si_scrive() {
        assert!(!autoplay_per(true, true));
        assert!(autoplay_per(false, true));
        assert!(!autoplay_per(false, false));
        assert!(!coda_si_scrive_per(true), "la coda di sessione è sospesa");
        assert!(coda_si_scrive_per(false));
    }

    /// Le regole di sessione decidono quel che vale **adesso**, e non toccano
    /// quel che è scritto: si legge la libreria prima e dopo averle applicate
    /// e il contenuto di `settings` è identico, riga per riga.
    #[test]
    fn le_regole_di_sessione_non_scrivono_mai_nelle_impostazioni() {
        let libreria = rymembra_app::db::open_in_memory()
            .expect("database")
            .connection;
        rymembra_app::playback::save_crossfade(&libreria, 8).expect("dissolvenza");
        rymembra_app::playback::save_autoplay(&libreria, true).expect("autoplay");
        let coda = QueueSnapshot {
            tracks: vec![1, 2, 3],
            order: vec![0, 1, 2],
            order_pos: Some(1),
            shuffle: true,
            repeat: rymembra_domain::queue::RepeatMode::All,
        };
        rymembra_app::playback::save_queue(&libreria, &coda).expect("coda");
        let fotografia = |c: &rusqlite::Connection| -> Vec<(String, String)> {
            let mut sql = c
                .prepare("SELECT key, value FROM settings ORDER BY key")
                .expect("query");
            sql.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .expect("righe")
                .collect::<Result<_, _>>()
                .expect("lettura")
        };
        let prima = fotografia(&libreria);

        // Quel che il lettore legge dalle impostazioni, e quel che la sessione
        // ne fa: la dissolvenza a zero, l'autoplay spento, la coda non scritta.
        let secondi = rymembra_app::playback::load_crossfade(&libreria).expect("letta");
        let voluto = rymembra_app::playback::load_autoplay(&libreria).expect("letto");
        assert_eq!(dissolvenza_per(true, secondi), 0);
        assert!(!autoplay_per(true, voluto));
        assert!(!coda_si_scrive_per(true));
        // La sessione mette da parte la coda in memoria, così com'è.
        let sessione = Sessione::nuova(
            Ruolo::Ospita,
            CodaDiPrima {
                istantanea: rymembra_app::playback::load_queue(&libreria).expect("coda letta"),
                curata: true,
                ms: 0,
                avviata: false,
            },
        );
        assert_eq!(sessione.prima.istantanea, coda);

        assert_eq!(
            fotografia(&libreria),
            prima,
            "le impostazioni sono cambiate"
        );
        // E all'uscita le regole tornano quelle scritte, senza rileggere niente.
        assert_eq!(dissolvenza_per(false, secondi), 8000);
        assert!(autoplay_per(false, voluto));
        // Una coda di maniglie, se mai arrivasse al disco, il nucleo la
        // rifiuterebbe: la seconda difesa dietro alla sospensione.
        let di_maniglie = QueueSnapshot {
            tracks: vec![-1, -2],
            order: vec![0, 1],
            order_pos: Some(0),
            shuffle: false,
            repeat: rymembra_domain::queue::RepeatMode::Off,
        };
        assert!(rymembra_app::playback::save_queue(&libreria, &di_maniglie).is_err());
        assert_eq!(fotografia(&libreria), prima);
    }

    /// La posizione vecchia sul brano nuovo, vista nella prova: dopo
    /// «prossimo» il motore dice ancora 63 798 ms finché la voce nuova non
    /// parte, e la finestra deve mostrare l'inizio — o il punto del salto.
    #[test]
    fn fino_alla_partenza_si_mostra_il_punto_promesso() {
        let promessa = Programmata {
            maniglia: Some(-7),
            etichetta: Some(7),
            posizione_us: 42_000_000,
            quando_ns: 1_000,
        };
        // La voce nuova è in coda e il motore suona ancora la vecchia: il
        // punto promesso, anche dopo l'istante (il primo campione è in ritardo).
        assert_eq!(
            punto_da_mostrare(Some(&promessa), Some(-7), Some(-6), 63_798, 500),
            Some(42_000)
        );
        assert_eq!(
            punto_da_mostrare(Some(&promessa), Some(-7), Some(-6), 63_798, 5_000),
            Some(42_000)
        );
        // Un salto nella stessa voce: il punto d'arrivo fino all'istante, poi
        // quello vero del motore — ma solo quando il motore c'è arrivato.
        assert_eq!(
            punto_da_mostrare(Some(&promessa), Some(-7), Some(-7), 63_798, 500),
            Some(42_000)
        );
        assert_eq!(
            punto_da_mostrare(Some(&promessa), Some(-7), Some(-7), 42_004, 5_000),
            None
        );
        // L'istante è passato ma il motore dice ancora il punto di prima (il
        // primo campione del salto non è ancora all'orecchio): il promesso,
        // non il vecchio — la barra non torna indietro.
        assert_eq!(
            punto_da_mostrare(Some(&promessa), Some(-7), Some(-7), 63_798, 400_000_000),
            Some(42_000)
        );
        // Arrivato, e andato avanti di 400 ms come il tempo: il motore.
        assert_eq!(
            punto_da_mostrare(Some(&promessa), Some(-7), Some(-7), 42_390, 400_001_000),
            None
        );
        // Scaduta, anche se il motore non c'è: la promessa non vale più, e la
        // barra dice quel che il motore dice — non resta ferma per sempre.
        assert_eq!(
            punto_da_mostrare(Some(&promessa), Some(-7), Some(-7), 81_394, 600_000_000_000),
            None
        );
        // Lo specchio di chi segue già sulla voce dopo, senza promessa: zero.
        assert_eq!(punto_da_mostrare(None, Some(-8), Some(-7), 5, 0), Some(0));
        // Una promessa di un'altra voce non conta; motore e coda d'accordo, il
        // motore.
        assert_eq!(
            punto_da_mostrare(Some(&promessa), Some(-9), Some(-9), 5, 0),
            None
        );
        assert_eq!(punto_da_mostrare(None, Some(-9), None, 5, 0), None);
    }

    /// Una riga di `tracks` per le prove delle voci da mandare: un file, o un
    /// brano di catalogo se `percorso` è `None`.
    fn riga_in_tabella(c: &rusqlite::Connection, id: i64, percorso: Option<&str>) {
        // Un file ha percorso, dimensione e data; un brano di catalogo ha
        // servizio, indirizzo, licenza e disponibilità, e niente del file: è
        // il `CHECK` della migrazione 26.
        let esito = match percorso {
            Some(percorso) => c.execute(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, duration_ms,
                                     file_size, date_added, date_modified)
                 VALUES (?1, ?2, 'k' || ?1, 'T', 'A', 'B', 200000, 3, 0, 0)",
                rusqlite::params![id, percorso],
            ),
            None => c.execute(
                "INSERT INTO tracks (id, track_key, title, artist, album, duration_ms,
                                     date_added, source_service, fonte_url, licenza,
                                     disponibilita)
                 VALUES (?1, 'k' || ?1, 'T', 'A', 'B', 200000, 0, 'internet-archive',
                         'https://archive.org/download/gd77/t01.mp3', 'cc-by',
                         'scaricabile')",
                rusqlite::params![id],
            ),
        };
        esito.expect("riga");
    }

    /// Il disco delle prove: quello vero, con due eccezioni a nome. Un file
    /// che si chiama `negato` risponde «permesso negato», uno che si chiama
    /// `lento` fa aspettare due secondi. Conta le letture.
    fn disco_finto(
        letture: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) -> impl Fn(&Path) -> std::io::Result<std::fs::Metadata> + Send + 'static {
        move |percorso: &Path| {
            letture.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let nome = percorso.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if nome.starts_with("negato") {
                return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
            }
            if nome.starts_with("lento") {
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
            std::fs::metadata(percorso)
        }
    }

    fn esiti_in_breve(esiti: &[Result<BranoDiQuesto, Mancanza>]) -> Vec<Result<i64, Mancanza>> {
        esiti
            .iter()
            .map(|esito| esito.as_ref().map(|brano| brano.track_id).map_err(|m| *m))
            .collect()
    }

    /// I casi del disco e della libreria, con file veri, nell'ordine chiesto e
    /// coi doppioni: il file c'è, la riga non c'è più, il file non c'è più, il
    /// disco risponde un altro errore.
    #[test]
    fn ogni_voce_dice_perche_non_si_puo_mandare() {
        let cartella = tempfile::tempdir().expect("cartella");
        let presente = cartella.path().join("presente.flac");
        std::fs::write(&presente, b"abc").expect("file");
        let sparito = cartella
            .path()
            .join("sottocartella-sparita")
            .join("sparito.flac");
        let negato = cartella.path().join("negato.flac");
        let c = rymembra_app::db::open_in_memory()
            .expect("database")
            .connection;
        riga_in_tabella(&c, 1, Some(&presente.to_string_lossy()));
        riga_in_tabella(&c, 2, Some(&sparito.to_string_lossy()));
        riga_in_tabella(&c, 4, Some(&negato.to_string_lossy()));

        let lette = leggi_righe(&c, &[2, 1, 3, 4, 1]).expect("lette");
        let letture = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let esiti = classifica(
            lette,
            std::time::Duration::from_secs(5),
            disco_finto(std::sync::Arc::clone(&letture)),
        );
        assert_eq!(
            esiti_in_breve(&esiti),
            [
                Err(Mancanza::FileAssente),
                Ok(1),
                Err(Mancanza::RigaSparita),
                Err(Mancanza::Irraggiungibile),
                Ok(1),
            ]
        );
        // Il brano presente porta il suo file com'è sul disco.
        let file = esiti
            .get(1)
            .and_then(|esito| esito.as_ref().ok())
            .and_then(|brano| brano.file.clone())
            .expect("file");
        assert_eq!(file.dimensione, 3);
        assert_eq!(file.estensione.as_deref(), Some("flac"));
        assert_eq!(letture.load(std::sync::atomic::Ordering::SeqCst), 4);
    }

    /// La scadenza del blocco taglia a metà: quel che è stato letto prima
    /// resta, la voce su cui il disco è fermo e quelle dopo sono
    /// irraggiungibili — riprovarle più tardi può bastare.
    #[test]
    fn la_scadenza_taglia_il_blocco_a_meta() {
        let cartella = tempfile::tempdir().expect("cartella");
        let mut riga = 0;
        let c = rymembra_app::db::open_in_memory()
            .expect("database")
            .connection;
        for nome in ["prima.flac", "lento.flac", "dopo.flac"] {
            let percorso = cartella.path().join(nome);
            std::fs::write(&percorso, b"abc").expect("file");
            riga += 1;
            riga_in_tabella(&c, riga, Some(&percorso.to_string_lossy()));
        }
        let lette = leggi_righe(&c, &[1, 2, 3]).expect("lette");
        let letture = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let inizio = std::time::Instant::now();
        let esiti = classifica(
            lette,
            std::time::Duration::from_millis(300),
            disco_finto(letture),
        );
        assert!(
            inizio.elapsed() < std::time::Duration::from_millis(1500),
            "la scadenza è del blocco, non della voce lenta"
        );
        assert_eq!(
            esiti_in_breve(&esiti),
            [
                Ok(1),
                Err(Mancanza::Irraggiungibile),
                Err(Mancanza::Irraggiungibile),
            ]
        );
    }

    /// Un brano di catalogo non tocca il disco, nemmeno quando il disco è
    /// fermo: esce subito, col suo riferimento.
    #[test]
    fn un_brano_di_catalogo_non_tocca_il_disco() {
        let cartella = tempfile::tempdir().expect("cartella");
        let lento = cartella.path().join("lento.flac");
        std::fs::write(&lento, b"abc").expect("file");
        let c = rymembra_app::db::open_in_memory()
            .expect("database")
            .connection;
        riga_in_tabella(&c, 1, None);
        riga_in_tabella(&c, 2, Some(&lento.to_string_lossy()));
        let lette = leggi_righe(&c, &[1, 2]).expect("lette");
        let letture = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let esiti = classifica(
            lette,
            std::time::Duration::from_millis(100),
            disco_finto(std::sync::Arc::clone(&letture)),
        );
        let catalogo = esiti
            .first()
            .and_then(|esito| esito.as_ref().ok())
            .expect("il brano di catalogo");
        assert!(catalogo.file.is_none());
        assert_eq!(
            catalogo.riferimento.as_deref(),
            Some("internet-archive:https://archive.org/download/gd77/t01.mp3")
        );
        assert_eq!(
            esiti.get(1).cloned().map(|e| e.err()),
            Some(Some(Mancanza::Irraggiungibile))
        );
        assert_eq!(
            letture.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "solo il file ha letto il disco"
        );
    }

    /// Il registro dice quali voci sono di questo PC: solo quelle che si
    /// aprono da una riga della sua libreria, e nell'ordine chiesto.
    #[test]
    fn solo_le_voci_di_questo_pc_si_mandano() {
        let voce = |uid: u64, proprietario: Option<&str>, apertura: Apertura| Voce {
            uid,
            proprietario: proprietario.map(str::to_owned),
            scheda: SchedaVoce {
                titolo: String::new(),
                artista: String::new(),
                album: String::new(),
                durata_ms: 1,
                numero: None,
                anno: None,
                copertina: None,
                replaygain_db: None,
                picco: None,
            },
            apertura,
            da_contare: None,
        };
        let mut registro = ospiti::Registro::default();
        registro.registra(voce(1, None, Apertura::Libreria { track_id: 40 }));
        registro.registra(voce(
            2,
            Some("PC di Marco"),
            Apertura::Catalogo {
                riferimento: String::new(),
            },
        ));
        registro.registra(voce(
            3,
            None,
            Apertura::Catalogo {
                riferimento: String::new(),
            },
        ));
        assert_eq!(
            righe_dei_uid(&registro, &[2, 1, 99, UID_CALIBRAZIONE, 3, 1]),
            [
                (2, None),
                (1, Some(40)),
                (99, None),
                (UID_CALIBRAZIONE, None),
                (3, None),
                (1, Some(40)),
            ]
        );
    }

    /// La gara d'ingresso: la strada ha già creato le voci, ma il lettore non
    /// ospita ancora. Le maniglie non entrano nella coda dell'utente: entrano i
    /// brani chiesti, e le voci si dimenticano.
    #[test]
    fn le_maniglie_non_entrano_in_una_coda_che_non_e_di_sessione() {
        let righe = vec![10, 11];
        let voci = Some(vec![-1, -2]);
        assert_eq!(
            brani_da_mettere(false, righe.clone(), voci.clone()),
            (vec![10, 11], vec![-1, -2])
        );
        assert_eq!(
            brani_da_mettere(true, righe.clone(), voci),
            (vec![-1, -2], vec![])
        );
        assert_eq!(
            brani_da_mettere(false, righe.clone(), None),
            (righe, vec![])
        );
    }

    /// `suona` di chi ospita: l'indice si ricalcola sulla voce del brano
    /// scelto, e un brano che non è diventato una voce non tocca la coda.
    #[test]
    fn suona_ritrova_il_brano_scelto_fra_le_voci() {
        let ok = |esito: (Partenza, Vec<i64>)| (esito.0.ok(), esito.1);
        // La riga 11 non c'è più: le voci sono una di meno, e il brano scelto
        // — la riga 12 — è la seconda voce, non la terza.
        let voci = (vec![-1, -3], vec![Some(10), Some(12)]);
        assert_eq!(
            ok(lista_da_suonare(true, vec![10, 11, 12], 2, Some(voci))),
            (Some((vec![-1, -3], 1)), vec![])
        );
        // Lo stesso brano due volte: la stessa occorrenza.
        let voci = (vec![-1, -2, -3], vec![Some(10), Some(11), Some(10)]);
        assert_eq!(
            ok(lista_da_suonare(true, vec![10, 11, 10], 2, Some(voci))),
            (Some((vec![-1, -2, -3], 2)), vec![])
        );
        // Il brano scelto non è diventato una voce: rifiuto, e le voci create
        // si dimenticano.
        let voci = (vec![-1], vec![Some(10)]);
        let (esito, da_dimenticare) = lista_da_suonare(true, vec![10, 11], 1, Some(voci));
        assert_eq!(
            esito.err().map(|e| e.code().kind()),
            Some(rymembra_domain::errors::ErrorCodeKind::LibraryTrackNotFound)
        );
        assert_eq!(da_dimenticare, vec![-1]);
        // Nessuna voce: rifiuto, non una coda vuota.
        let (esito, _) = lista_da_suonare(true, vec![10], 0, Some((vec![], vec![])));
        assert!(esito.is_err());
        // Il lettore non ospita (la gara d'ingresso): le righe, e l'indice
        // chiesto.
        let voci = (vec![-1], vec![Some(10)]);
        assert_eq!(
            ok(lista_da_suonare(false, vec![10, 11], 1, Some(voci))),
            (Some((vec![10, 11], 1)), vec![-1])
        );
    }

    /// La coda dell'utente messa da parte entrando non porta maniglie: quelle
    /// di una gara d'ingresso tornerebbero all'uscita come voci sparite, e la
    /// coda non si salverebbe più.
    #[test]
    fn la_coda_di_prima_si_ripulisce_dalle_maniglie() {
        let sporca = QueueSnapshot {
            tracks: vec![10, -3, 11, -4],
            order: vec![0, 1, 2, 3],
            order_pos: Some(2),
            shuffle: false,
            repeat: rymembra_domain::queue::RepeatMode::All,
        };
        let pulita = ripulita(sporca);
        assert_eq!(pulita.tracks, vec![10, 11]);
        assert_eq!(
            Queue::restore(pulita.clone()).current(),
            Some(11),
            "il cursore resta sul brano dell'utente"
        );
        assert_eq!(pulita.repeat, rymembra_domain::queue::RepeatMode::All);
        // Una coda pulita resta com'è.
        let gia = QueueSnapshot {
            tracks: vec![1, 2],
            order: vec![1, 0],
            order_pos: Some(0),
            shuffle: true,
            repeat: rymembra_domain::queue::RepeatMode::Off,
        };
        assert_eq!(ripulita(gia.clone()), gia);
    }

    /// I percorsi del ponte che aprono fuori dai lucchetti ricontrollano tutto
    /// alla consegna. Le righe che lo fanno, cercate nel sorgente: senza una
    /// di loro le prove delle regole passerebbero lo stesso.
    #[test]
    fn le_consegne_del_ponte_ricontrollano_quel_che_e_cambiato() {
        let ponte = include_str!("insieme.rs");
        let ponte = ponte.split("\n#[cfg(test)]").next().unwrap_or(ponte);
        let corpo = |firma: &str| {
            let da = ponte.find(firma).expect(firma);
            let resto = &ponte[da..];
            &resto[..resto.find("\n}\n").unwrap_or(resto.len())]
        };
        let ingresso = corpo("pub fn entra_come_host(");
        assert!(ingresso.contains("lettore.motore.formato() != foto.formato"));
        assert!(ingresso.contains("istantanea: ripulita(foto.istantanea.clone())"));
        assert!(ingresso.contains("dimentica_voci(app, &maniglie);"));
        let riapri = corpo("fn riapri(");
        assert!(riapri.contains("registro.ancora_viva(maniglia, generazione)"));
        let apri = riapri
            .find("lettore.apri_fermo(brano, da_ms);")
            .expect("apri_fermo");
        let continua = riapri
            .find("riapertura::continua_l_ascolto(maniglia);")
            .expect("l'ascolto continua");
        assert!(apri < continua, "il segno dell'ascolto va dopo l'apertura");
        let esci = corpo("pub fn esci(");
        let se = esci.find("if uscito {").expect("solo se si era dentro");
        let segni = esci.find("azzera_i_segni(&stato);").expect("i segni");
        assert!(
            se < segni,
            "i segni dell'utente si toccano solo uscendo davvero"
        );
    }

    #[test]
    fn la_finestra_tiene_il_brano_che_suona_e_quel_che_viene_dopo() {
        let ordine: Vec<i64> = (1..=5000).collect();
        // Al centro: un decimo prima, il resto dopo.
        let (pezzo, posto) = finestra(&ordine, Some(2000), 1000);
        assert_eq!(pezzo.len(), 1000);
        assert_eq!(posto, Some(100));
        assert_eq!(pezzo.get(100).copied(), Some(2001));
        // In fondo: la finestra si appoggia alla fine e tiene tutto il tetto.
        let (pezzo, posto) = finestra(&ordine, Some(4990), 1000);
        assert_eq!(pezzo.len(), 1000);
        assert_eq!(pezzo.last().copied(), Some(5000));
        assert_eq!(posto.and_then(|p| pezzo.get(p).copied()), Some(4991));
        // In testa, e senza un corrente.
        let (pezzo, posto) = finestra(&ordine, Some(0), 1000);
        assert_eq!((pezzo.first().copied(), posto), (Some(1), Some(0)));
        let (pezzo, posto) = finestra(&ordine, None, 1000);
        assert_eq!((pezzo.len(), posto), (1000, None));
        // Sotto il tetto non si taglia niente.
        let corta = vec![7, 8, 9];
        assert_eq!(finestra(&corta, Some(2), 1000), (corta.clone(), Some(2)));
    }

    #[test]
    fn un_tratto_di_cd_va_e_torna_in_millisecondi() {
        for cd in [0_u32, 1, 74, 75, 13_337] {
            let ms = ms_da_cd(cd);
            // Un fotogramma CD è 13,3 ms: l'arrotondamento resta sotto il mezzo.
            let esatto = f64::from(cd) * 1000.0 / 75.0;
            assert!((ms as f64 - esatto).abs() <= 0.5, "{cd} → {ms}");
        }
    }

    #[test]
    fn la_scheda_di_una_riga_porta_il_livello_del_brano() {
        let riga = TrackSummary {
            id: 9,
            path: Some(r"C:\Musica\a.flac".to_owned()),
            title: "Titolo".to_owned(),
            artist: "Artista".to_owned(),
            album: "Album".to_owned(),
            album_key: Some("k".to_owned()),
            track_number: Some(4),
            disc_number: Some(1),
            duration_ms: 200_000,
            year: Some(1999),
            cover_art_hash: Some("ab12".to_owned()),
            play_count: 3,
            liked: true,
            rating: 5,
            fonte: None,
            fonte_pagina: None,
            licenza: None,
        };
        let livello = SchedaSorgente {
            track_id: 9,
            collocazione: Collocazione::File(r"C:\Musica\a.flac".to_owned()),
            durata_ms: 200_000,
            replaygain_db: Some(-7.25),
            picco: Some(0.98),
        };
        let scheda = scheda_da_riga(&riga, Some(&livello));
        assert_eq!(scheda.titolo, "Titolo");
        assert_eq!(scheda.durata_ms, 200_000);
        assert_eq!(scheda.numero, Some(4));
        assert_eq!(scheda.anno, Some(1999));
        assert_eq!(scheda.copertina.as_deref(), Some("ab12"));
        assert!(
            scheda
                .replaygain_db
                .is_some_and(|db| (db + 7.25).abs() < 1e-3)
        );
        assert!(scheda_da_riga(&riga, None).replaygain_db.is_none());
    }
}
