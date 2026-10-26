//! Il solo confine fra gli id della libreria e le voci di una sessione.
//!
//! # La regola
//!
//! **id > 0 è una riga della libreria di questo PC, id < 0 è una voce di
//! sessione** (una «maniglia»). La coda, il motore e la finestra continuano a
//! parlare di `i64`; chi deve sapere che cosa c'è dietro un id chiede a
//! [`chi`], e nessun altro fa il confronto col segno: le due sole funzioni che
//! lo guardano senza un registro in mano, [`e_di_libreria`] e
//! [`riga_di_libreria`], stanno qui accanto per questo.
//!
//! # Perché le maniglie e non gli `uid`
//!
//! Perché tutto quel che sta sopra il motore — la coda, l'ascolto, la
//! finestra, il preparatore del gapless — è scritto per un `i64` di brano, e
//! riscriverlo per un secondo tipo d'identificativo vorrebbe dire due strade
//! per ogni gesto. Un numero negativo non può essere una riga di SQLite (le
//! righe partono da uno), quindi la stessa casella dice due cose senza mai
//! confonderle. L'`uid` resta quel che è: il nome della voce **nella
//! sessione**, uguale su tutti i PC, e l'etichetta che il motore appiccica
//! alle sue ancore.
//!
//! # Il registro, e il suo lucchetto
//!
//! Le voci stanno in un [`Registro`] dietro un lucchetto **suo**, foglia:
//! chi ha il lettore in mano lo può prendere per un attimo (per tradurre una
//! maniglia in `uid` mentre fotografa la coda), ma chi lo tiene non chiede mai
//! il lettore. Così la finestra può chiedere la scheda di una voce senza
//! mettersi in fila dietro alla pausa, e la pausa non aspetta mai una
//! traduzione.
//!
//! Le maniglie **non si riusano**, nemmeno fra una sessione e l'altra: alla
//! fine di una sessione il registro si svuota ma il contatore resta dov'era.
//! Una maniglia vecchia rimasta in mano alla finestra — la coda disegnata un
//! istante prima dell'uscita — risponde così [`Chi::Sparita`], e non il titolo
//! di un'altra voce della sessione dopo.
//!
//! # Aprire
//!
//! [`apri`] fa per le voci quel che `brano_di` fa per le righe: si chiama
//! **fuori** da ogni lucchetto, e l'attesa ha addosso la scadenza della rete
//! (`APERTURA_FLUSSO`) quando i byte stanno su un altro PC o in un catalogo.
//! Il brano aperto porta come `track_id` la **maniglia** e come etichetta
//! l'`uid`: il motore annuncia la maniglia, che è quel che la coda contiene, e
//! le ancore portano l'`uid`, che è quel che l'altro PC confronta.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rymembra_app::library::TrackSummary;
use rymembra_domain::errors::{AppError, ErrorCode};
use tauri::Manager as _;

use super::StatoLettore;
use super::insieme::UID_CALIBRAZIONE;

/// Una voce di sessione, come la vede questo PC.
pub struct Voce {
    /// L'identità nella sessione, uguale su tutti i PC; è anche l'etichetta
    /// delle ancore del motore.
    pub uid: u64,
    /// Il nome del PC a cui appartiene; `None` se è di questo PC.
    pub proprietario: Option<String>,
    /// Quel che serve a disegnarla.
    pub scheda: SchedaVoce,
    /// Come si aprono i suoi byte.
    pub apertura: Apertura,
    /// La riga della libreria di questo PC da contare come ascolto, se ce
    /// n'è una: per chi ospita il suo brano, e in fase 3 una copia identica
    /// verificata. `None`: la voce non conta e non fa scrobble.
    pub da_contare: Option<i64>,
}

/// Quel che serve a disegnare una voce.
#[derive(Debug, Clone, PartialEq)]
pub struct SchedaVoce {
    /// Il titolo.
    pub titolo: String,
    /// L'interprete.
    pub artista: String,
    /// L'album.
    pub album: String,
    /// Quanto dura, in millisecondi.
    pub durata_ms: u64,
    /// Il numero di traccia.
    pub numero: Option<u32>,
    /// L'anno.
    pub anno: Option<i32>,
    /// La chiave della copertina per il protocollo `rymembra-cover`: l'impronta
    /// della libreria per un brano di questo PC, `insieme-<uid>` per una
    /// copertina arrivata dalla rete (vedi `crate::insieme::copertina`).
    pub copertina: Option<String>,
    /// La correzione ReplayGain dichiarata, in dB.
    pub replaygain_db: Option<f32>,
    /// Il picco dichiarato.
    pub picco: Option<f32>,
}

/// Chi apre i byte di una voce che sta sul disco di un altro PC.
///
/// Una chiusura e non il trasporto: questo modulo non sa niente della rete.
/// La costruisce la sessione, che sa a chi chiedere e con quale gettone, e la
/// si chiama **fuori** dal lucchetto del lettore, come ogni apertura.
pub type ApriInRete =
    Arc<dyn Fn() -> Result<Box<dyn rymembra_net::flusso::Sorgente>, AppError> + Send + Sync>;

/// Come si aprono i byte di una voce.
pub enum Apertura {
    /// Una riga della libreria di questo PC.
    Libreria {
        /// L'id della riga.
        track_id: i64,
    },
    /// Un file sul disco di un altro PC.
    Rete {
        /// Chi apre la sorgente.
        apri: ApriInRete,
        /// L'estensione, per il riconoscimento del formato.
        estensione: Option<String>,
        /// Il tratto, per un brano d'immagine CD: inizio e fine in ms.
        segmento: Option<(u64, Option<u64>)>,
        /// Di quale file è la voce, uguale per tutte le voci dello stesso file
        /// (il gettone in esadecimale): è quel che fa riconoscere contigue due
        /// voci della stessa immagine CD (`Segmento::immagine`). `None` per una
        /// voce che non viene da un file condiviso.
        immagine: Option<String>,
        /// Fase 3: quel che serve a cercare una copia identica qui.
        copia: Option<rymembra_insieme::messaggi::DescrittoreCopia>,
    },
    /// Un brano di catalogo, aperto da questo PC col suo riferimento.
    Catalogo {
        /// Il riferimento stabile.
        riferimento: String,
    },
}

/// Che cosa c'è dietro un id.
pub enum Chi {
    /// Una riga della libreria di questo PC.
    Libreria(i64),
    /// Una voce di sessione.
    Voce(Arc<Voce>),
    /// Una maniglia che non corrisponde più a niente: la sessione è finita, o
    /// la voce è stata tolta.
    Sparita,
}

impl Voce {
    /// La riga della libreria di questo PC da cui la voce si apre, se si apre
    /// da una riga: è una voce di chi ospita (o, in fase 2, del membro che
    /// l'ha aggiunta), suonata dal suo file.
    #[must_use]
    pub(super) const fn apertura_di_libreria(&self) -> Option<i64> {
        match self.apertura {
            Apertura::Libreria { track_id } => Some(track_id),
            Apertura::Rete { .. } | Apertura::Catalogo { .. } => None,
        }
    }
}

/// Che cosa c'è dietro un id: vedi il `//!`.
#[must_use]
pub fn chi(app: &tauri::AppHandle, id: i64) -> Chi {
    // Una riga, o la calibrazione, si riconoscono senza il registro: niente
    // lucchetto per la domanda che si fa più spesso.
    if e_di_libreria(id) || id == MANIGLIA_CALIBRAZIONE {
        return chi_nel(None, id);
    }
    con_registro(app, |registro| chi_nel(Some(registro), id)).unwrap_or(Chi::Sparita)
}

/// [`chi`], dato il registro: la regola intera, senza un'applicazione
/// intorno, ed è quella che le prove provano.
///
/// La voce di calibrazione non sta nel registro: è la stessa su ogni PC e in
/// ogni sessione, e un membro la deve saper aprire appena l'ancora dell'host la
/// nomina, senza che nessuno gliel'abbia mai mandata. Senza un registro — il
/// lettore che non c'è più — ogni maniglia è sparita.
fn chi_nel(registro: Option<&Registro>, id: i64) -> Chi {
    if e_di_libreria(id) {
        return Chi::Libreria(id);
    }
    if id == MANIGLIA_CALIBRAZIONE {
        return Chi::Voce(voce_di_calibrazione());
    }
    registro
        .and_then(|registro| registro.voce(id))
        .map_or(Chi::Sparita, Chi::Voce)
}

/// L'id è una riga della libreria di questo PC.
///
/// Il confronto col segno, scritto una volta: chi deve solo sapere se un id
/// può toccare il database — un segno di «riprendi dov'eri», una guardia
/// davanti a un comando — chiede qui invece di ripetere `id > 0`, così il
/// giorno in cui la regola cambiasse cambierebbe in un posto solo.
#[must_use]
pub const fn e_di_libreria(id: i64) -> bool {
    id > 0
}

/// La guardia dei comandi che scrivono o leggono per id di brano: un id che
/// non è una riga della libreria è `library.trackNotFound`, lo stesso codice
/// che `rymembra_app` restituisce per una riga che non c'è più.
///
/// # Errori
///
/// `library.trackNotFound` per un id ≤ 0.
pub fn riga_di_libreria(id: i64) -> Result<i64, AppError> {
    if e_di_libreria(id) {
        return Ok(id);
    }
    Err(
        AppError::new(ErrorCode::LibraryTrackNotFound { track_id: Some(id) }).with_cause(format!(
            "id {id}: è una voce di sessione, non una riga della libreria"
        )),
    )
}

/// Come [`riga_di_libreria`], per un elenco: basta un id fuori perché il
/// comando non si faccia. Mescolare in silenzio metà gesto e metà rifiuto
/// vorrebbe dire una playlist con dentro solo una parte di quel che si è
/// trascinato, senza che nessuno lo dica.
///
/// # Errori
///
/// `library.trackNotFound` per il primo id ≤ 0.
pub fn righe_di_libreria(ids: &[i64]) -> Result<(), AppError> {
    ids.iter()
        .try_for_each(|id| riga_di_libreria(*id).map(|_| ()))
}

// ── il registro ─────────────────────────────────────────────────────────────

/// La maniglia della voce di calibrazione: fissa, su ogni PC e in ogni
/// sessione, e fuori dal registro. `i64::MIN` perché il registro conta le sue
/// a scendere da `-1` e non ci arriverà mai.
pub(super) const MANIGLIA_CALIBRAZIONE: i64 = i64::MIN;

/// Le voci della sessione di questo PC. Vedi «Il registro» nel `//!`.
#[derive(Default)]
pub(crate) struct Registro {
    /// `uid` → maniglia.
    per_uid: HashMap<u64, i64>,
    /// Maniglia → voce.
    voci: HashMap<i64, Arc<Voce>>,
    /// L'ultima maniglia data. Scende e basta, anche attraverso le sessioni:
    /// vedi il `//!`.
    ultima: i64,
    /// L'ultimo `uid` dato da questo PC quando ospita. Riparte da zero a ogni
    /// sessione: l'`uid` vive nella sessione, e i membri lo ricevono con lei.
    ultimo_uid: u64,
    /// Fase 3: la copia locale di una voce in rete, per `uid`, quando la si è
    /// già cercata. `Some(riga)`: trovata e verificata; `None`: cercata, e qui
    /// non c'è. Vedi «La copia locale» su [`apri`].
    copie: HashMap<u64, Option<i64>>,
    /// Quante sessioni sono finite: cresce a ogni [`Registro::svuota`].
    ///
    /// Serve alla ricerca della copia, che gira **senza** il lucchetto del
    /// registro (database e disco, fino a qualche secondo) e scrive l'esito
    /// dopo. Gli `uid` ripartono da uno a ogni sessione: un esito arrivato
    /// dopo l'uscita finirebbe nel registro della sessione **dopo**, dove lo
    /// stesso `uid` è un'altra voce — un altro brano suonato da un file di qui,
    /// e un ascolto contato alla riga sbagliata. L'esito si scrive solo se la
    /// generazione è ancora quella in cui la ricerca è partita.
    generazione: u64,
}

impl Registro {
    /// La voce dietro una maniglia.
    pub(super) fn voce(&self, maniglia: i64) -> Option<Arc<Voce>> {
        self.voci.get(&maniglia).cloned()
    }

    /// La maniglia di un `uid`, se la voce è registrata.
    pub(super) fn maniglia(&self, uid: u64) -> Option<i64> {
        if uid == UID_CALIBRAZIONE {
            return Some(MANIGLIA_CALIBRAZIONE);
        }
        self.per_uid.get(&uid).copied()
    }

    /// Aggiunge una voce, o aggiorna quella con lo stesso `uid`: lo stesso
    /// `uid` tiene la stessa maniglia, che è quel che la coda e il motore
    /// hanno già in mano. Restituisce la maniglia.
    ///
    /// La voce di calibrazione non si registra: ha la sua maniglia fissa.
    pub(super) fn registra(&mut self, voce: Voce) -> i64 {
        if voce.uid == UID_CALIBRAZIONE {
            return MANIGLIA_CALIBRAZIONE;
        }
        let maniglia = match self.per_uid.get(&voce.uid) {
            Some(&maniglia) => maniglia,
            None => {
                // A scendere, e mai fino a `MANIGLIA_CALIBRAZIONE`: ci vorrebbe
                // una voce al nanosecondo per tre secoli, ma un tetto scritto
                // costa una riga e toglie la domanda.
                self.ultima = self
                    .ultima
                    .saturating_sub(1)
                    .max(MANIGLIA_CALIBRAZIONE.saturating_add(1));
                self.per_uid.insert(voce.uid, self.ultima);
                self.ultima
            }
        };
        self.voci.insert(maniglia, Arc::new(voce));
        maniglia
    }

    /// Un `uid` nuovo per una voce di questo PC che ospita: da uno in su, per
    /// sessione.
    pub(super) fn nuovo_uid(&mut self) -> u64 {
        self.ultimo_uid = self.ultimo_uid.saturating_add(1);
        // Il tetto non si raggiunge, ma se si raggiungesse l'`uid` di
        // calibrazione non deve poter nominare una voce vera.
        if self.ultimo_uid == UID_CALIBRAZIONE {
            self.ultimo_uid = UID_CALIBRAZIONE.saturating_sub(1);
        }
        self.ultimo_uid
    }

    /// Le maniglie delle voci che aprono queste righe della libreria: servono
    /// a chi toglie dalla coda dei brani che la libreria sta per perdere, e
    /// in sessione la coda ha le maniglie, non le righe.
    pub(super) fn maniglie_di_righe(&self, righe: &[i64]) -> Vec<i64> {
        self.voci
            .iter()
            .filter(|(_, voce)| {
                matches!(voce.apertura, Apertura::Libreria { track_id } if righe.contains(&track_id))
            })
            .map(|(maniglia, _)| *maniglia)
            .collect()
    }

    /// La sessione è finita: ogni maniglia diventa [`Chi::Sparita`]. Il
    /// contatore delle maniglie resta dov'era, quello degli `uid` riparte.
    pub(super) fn svuota(&mut self) {
        self.per_uid.clear();
        self.voci.clear();
        self.copie.clear();
        self.ultimo_uid = 0;
        self.generazione = self.generazione.wrapping_add(1);
    }

    /// Toglie delle voci: le maniglie diventano [`Chi::Sparita`], e la copia
    /// cercata per loro si dimentica. Serve alle voci di un membro che esce,
    /// che non torneranno — e un registro che non le togliesse crescerebbe per
    /// tutta la sessione con voci che nessuno può più aprire.
    pub(super) fn togli(&mut self, maniglie: &[i64]) {
        for maniglia in maniglie {
            if let Some(voce) = self.voci.remove(maniglia) {
                self.per_uid.remove(&voce.uid);
                self.copie.remove(&voce.uid);
            }
        }
    }

    /// Fase 3: la copia locale già cercata per questo `uid`. Fuori `None` se
    /// non la si è ancora cercata; dentro, la riga trovata o `None`.
    pub(super) fn copia(&self, uid: u64) -> Option<Option<i64>> {
        self.copie.get(&uid).copied()
    }

    /// Fase 3: la copia trovata per questo `uid`, se ce n'è una verificata.
    pub(super) fn copia_trovata(&self, uid: u64) -> Option<i64> {
        self.copia(uid).flatten()
    }

    /// Fase 3: si ricorda com'è finita la ricerca per questo `uid`, se la
    /// sessione è ancora quella della `generazione` in cui la ricerca è
    /// partita. Vedi [`Registro::generazione`].
    pub(super) fn ricorda_copia(&mut self, uid: u64, riga: Option<i64>, generazione: u64) {
        if generazione == self.generazione {
            self.copie.insert(uid, riga);
        }
    }

    /// La generazione: vedi il campo.
    pub(super) const fn generazione(&self) -> u64 {
        self.generazione
    }

    /// La voce `maniglia` è ancora quella registrata nella `generazione` in
    /// cui chi chiede l'ha trovata: la sessione non è finita nel frattempo, e
    /// la voce non è stata tolta. Serve a chi apre fuori dai lucchetti e
    /// consegna dopo — un'apertura in rete può durare secondi, e finire in una
    /// sessione successiva dove lo stesso `uid` è un'altra voce.
    pub(super) fn ancora_viva(&self, maniglia: i64, generazione: u64) -> bool {
        self.generazione == generazione && self.voci.contains_key(&maniglia)
    }

    /// Quante voci, per il diario.
    pub(super) fn quante(&self) -> usize {
        self.voci.len()
    }
}

/// Fa `azione` sul registro di questo PC, col suo lucchetto e nient'altro.
/// `None` se il lettore non è più fra gli stati gestiti: l'applicazione si
/// sta chiudendo, e ogni maniglia è sparita con lei.
pub(super) fn con_registro<T>(
    app: &tauri::AppHandle,
    azione: impl FnOnce(&mut Registro) -> T,
) -> Option<T> {
    let stato = app.try_state::<StatoLettore>()?;
    let mut registro = stato
        .ospiti
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Some(azione(&mut registro))
}

/// Il registro vuoto con cui nasce il lettore.
pub(super) fn registro_nuovo() -> Mutex<Registro> {
    Mutex::new(Registro::default())
}

// ── le domande sulle voci ───────────────────────────────────────────────────

/// La riga da contare come ascolto quando `id` comincia a suonare: la riga
/// stessa per un brano della libreria, quella dichiarata dalla voce per una
/// voce di sessione, nessuna per una maniglia sparita.
///
/// È il filtro davanti a `ListenTracker`, a `record_play` e allo scrobble:
/// `rymembra_app` rifiuta un id ≤ 0 con un errore, e senza questo filtro il
/// diario avrebbe una riga di rifiuto per ogni voce ascoltata.
#[must_use]
///
/// In fase 3 una voce in rete conta anche quando suona da una copia identica
/// trovata qui (vedi «La copia locale» su [`apri`]): è un file di questo PC,
/// e l'ascolto è suo.
pub fn da_contare(app: &tauri::AppHandle, id: i64) -> Option<i64> {
    match chi(app, id) {
        Chi::Libreria(riga) => Some(riga),
        Chi::Voce(voce) => {
            let copia = con_registro(app, |registro| registro.copia_trovata(voce.uid)).flatten();
            conta_della_voce(&voce, copia)
        }
        Chi::Sparita => None,
    }
}

/// La riga che una voce conta: quella che dichiara, o la copia verificata
/// trovata qui. Mai una maniglia, e mai una copia soltanto candidata: una riga
/// contata è un ascolto scritto nella cronologia, e un ascolto inventato non si
/// toglie più.
fn conta_della_voce(voce: &Voce, copia: Option<i64>) -> Option<i64> {
    voce.da_contare
        .or(copia)
        .filter(|riga| e_di_libreria(*riga))
}

/// La riga della libreria di questo PC che sta **dietro** l'id, per chi deve
/// leggerne qualcosa nel database — il formato del file, i vicini di disco
/// per la normalizzazione, se è un flusso. Diversa da [`da_contare`] solo in
/// fase 3, dove una copia verificata conta come ascolto senza che la voce si
/// apra da lì; oggi coincidono quasi sempre.
#[must_use]
pub fn riga_dietro(app: &tauri::AppHandle, id: i64) -> Option<i64> {
    match chi(app, id) {
        Chi::Libreria(riga) => Some(riga),
        Chi::Voce(voce) => match voce.apertura {
            Apertura::Libreria { track_id } => Some(track_id),
            Apertura::Rete { .. } | Apertura::Catalogo { .. } => {
                let copia =
                    con_registro(app, |registro| registro.copia_trovata(voce.uid)).flatten();
                conta_della_voce(&voce, copia)
            }
        },
        Chi::Sparita => None,
    }
}

/// Quanto dura una voce, dalla sua scheda. `None` per una riga della
/// libreria (la durata la sa il database) e per una maniglia sparita.
#[must_use]
pub fn durata_ms(app: &tauri::AppHandle, id: i64) -> Option<u64> {
    match chi(app, id) {
        Chi::Voce(voce) => Some(voce.scheda.durata_ms),
        Chi::Libreria(_) | Chi::Sparita => None,
    }
}

/// I byte di questa voce arrivano dalla rete — da un altro PC o da un
/// catalogo — e non da un disco di questo PC. `None` quando la risposta la
/// sa solo il database: una riga della libreria, o una voce che ne apre una.
#[must_use]
pub fn e_un_flusso(voce: &Voce) -> Option<bool> {
    match voce.apertura {
        Apertura::Libreria { .. } => None,
        Apertura::Rete { .. } | Apertura::Catalogo { .. } => Some(true),
    }
}

/// La scheda di una voce, nella forma che la finestra disegna già per ogni
/// riga della libreria.
///
/// Senza `path`, `fonte` e `album_key`: sono le chiavi con cui la finestra
/// accende i gesti di libreria — mostra nella cartella, apri l'album, la
/// pastiglia «in streaming» — e nessuno di quelli vale per una voce. L'id è
/// la maniglia, negativa: è il segno che la finestra legge (`idDiLibreria`)
/// per nascondere preferito, voto e playlist. La copertina è la chiave del
/// protocollo `rymembra-cover`, che per una voce arrivata dalla rete è
/// `insieme-<uid>` e la serve `crate::insieme::copertina`.
#[must_use]
pub fn scheda(maniglia: i64, voce: &Voce) -> TrackSummary {
    let s = &voce.scheda;
    TrackSummary {
        id: maniglia,
        path: None,
        title: s.titolo.clone(),
        artist: s.artista.clone(),
        album: s.album.clone(),
        album_key: None,
        track_number: s.numero.map(i64::from),
        disc_number: None,
        duration_ms: i64::try_from(s.durata_ms).unwrap_or(i64::MAX),
        year: s.anno.map(i64::from),
        cover_art_hash: s.copertina.clone(),
        play_count: 0,
        liked: false,
        rating: 0,
        fonte: None,
        fonte_pagina: None,
        licenza: None,
    }
}

/// Le maniglie delle voci che aprono queste righe della libreria. Vuoto
/// fuori sessione. Vedi [`Registro::maniglie_di_righe`].
#[must_use]
pub fn maniglie_di_righe(app: &tauri::AppHandle, righe: &[i64]) -> Vec<i64> {
    con_registro(app, |registro| registro.maniglie_di_righe(righe)).unwrap_or_default()
}

// ── aprire ──────────────────────────────────────────────────────────────────

/// Apre una voce per il motore, con la maniglia come `track_id` e l'`uid`
/// come etichetta. Vedi «Aprire» nel `//!`.
///
/// **Fuori da ogni lucchetto**: una voce in rete aspetta la rete, una voce di
/// libreria la libreria e il disco, e la regola del modulo padre vale anche
/// qui.
///
/// # La copia locale
///
/// Una voce in rete con un descrittore (il campo `copia` di [`Apertura::Rete`]) si cerca
/// prima su questo PC: se la libreria ha lo stesso file — lo stesso percorso
/// su una condivisione, o una copia con la stessa impronta — si suona da lì,
/// e la rete non serve. È la stessa musica al byte, quindi la sincronia non ne
/// sa niente; l'ascolto invece sì: una copia verificata è un file di questo PC,
/// e conta (vedi [`da_contare`]).
///
/// - I candidati vengono dal database (`rymembra_app::insieme::candidati_copia`),
///   che sa già le regole: una copia certa per percorso, dimensione e data, o
///   da verificare per chiave di contenuto; per un brano d'immagine CD solo
///   quella certa.
/// - Una copia da verificare si verifica leggendo 128 KiB dal disco
///   (`e_la_stessa`), con la scadenza di un'apertura: un disco che non
///   risponde non è una copia, e non deve fermare la voce.
/// - L'esito si **ricorda per `uid`**, trovata o no: la stessa voce si riapre
///   a ogni salto grande del regolatore e a ogni riapertura, e rileggere
///   l'impronta ogni volta costerebbe proprio i millisecondi dell'anticipo. Un
///   esito che non è un esito — il database occupato, la scadenza — non si
///   ricorda, e la volta dopo si riprova.
/// - Una copia trovata che poi non si apre si dimentica, e si suona dalla rete.
///
/// La ricerca gira dove gira l'apertura: per la voce dopo, nel preparatore,
/// cioè mentre la voce di prima suona ancora.
///
/// # Errori
///
/// Quelli di `brano_di` per una voce di libreria, quelli del trasporto
/// (`insieme.*`) e del flusso per una voce in rete, `net.timeout` quando la
/// rete non risponde entro `APERTURA_FLUSSO`, e quelli dei cataloghi.
pub(super) fn apri(
    app: &tauri::AppHandle,
    maniglia: i64,
    voce: &Voce,
    formato: rymembra_play::FormatoUscita,
) -> Result<rymembra_play::BranoAperto, AppError> {
    let brano = match &voce.apertura {
        Apertura::Libreria { track_id } => {
            super::brano_di_libreria(app, *track_id, maniglia, formato)?
        }
        Apertura::Rete {
            apri,
            estensione,
            segmento,
            immagine,
            copia,
        } => {
            // Prima la copia locale, se l'altro PC ha descritto il file: vedi
            // «La copia locale» qui sopra.
            if let Some(brano) = copia
                .as_ref()
                .and_then(|descrittore| apri_la_copia(app, maniglia, voce, descrittore, formato))
            {
                return Ok(brano.con_etichetta(voce.uid));
            }
            // L'identità del file per i brani d'immagine: due voci dello
            // stesso disco devono portare la stessa stringa perché il motore
            // le riconosca contigue (`Segmento::precede`). La dà la sessione,
            // dal gettone del file; senza, l'impronta della copia se c'è, e
            // come ultimo ripiego una stringa per voce — due tracce dello
            // stesso CD passano allora l'una all'altra col gapless di sempre
            // invece che come un segnale solo, e la differenza si sentirebbe
            // solo con una dissolvenza, che in sessione è spenta.
            let immagine = immagine.clone().unwrap_or_else(|| {
                copia.as_ref().map_or_else(
                    || format!("insieme-voce-{}", voce.uid),
                    |copia| format!("insieme-file-{}", copia.impronta_rapida),
                )
            });
            super::flusso::apri_da_pari(
                Arc::clone(apri),
                super::flusso::DaPari {
                    maniglia,
                    estensione: estensione.clone(),
                    durata_ms: voce.scheda.durata_ms,
                    replaygain_db: voce.scheda.replaygain_db,
                    picco: voce.scheda.picco,
                    segmento: segmento.map(|(inizio, fine)| tratto(immagine, inizio, fine)),
                },
                formato,
            )?
        }
        Apertura::Catalogo { riferimento } => {
            let rif = super::flusso::riferimento_da_voce(riferimento).ok_or_else(|| {
                // Un riferimento che la lista di questo PC non riconosce non si
                // apre: è la stessa regola di una riga della libreria, e un
                // altro PC non può farla scavalcare.
                AppError::new(ErrorCode::PlaybackSourceUnavailable {
                    track_id: None,
                    path: None,
                })
                .with_cause("il riferimento di catalogo della voce non è ammesso qui".to_owned())
            })?;
            super::flusso::apri_da_catalogo(app, maniglia, &rif, &voce.scheda, formato)?
        }
    };
    Ok(brano.con_etichetta(voce.uid))
}

/// La copia locale di una voce, aperta come la sua riga della libreria con
/// la maniglia della voce. `None` se qui non c'è, o se c'era e non si apre
/// più: allora si dimentica, e la voce si apre dalla rete.
fn apri_la_copia(
    app: &tauri::AppHandle,
    maniglia: i64,
    voce: &Voce,
    descrittore: &rymembra_insieme::messaggi::DescrittoreCopia,
    formato: rymembra_play::FormatoUscita,
) -> Option<rymembra_play::BranoAperto> {
    let trovata = copia_locale(app, voce.uid, descrittore, voce.scheda.durata_ms)?;
    match super::brano_di_libreria(app, trovata.riga, maniglia, formato) {
        Ok(brano) => Some(brano),
        Err(err) => {
            crate::nota!(
                "[insieme] la copia locale della voce {} non si apre ({}): si suona dalla rete",
                voce.uid,
                err.code().kind().code()
            );
            // Per il resto della sessione questa voce va dalla rete: una copia
            // certa non si riverifica, e riprovarla a ogni riapertura vorrebbe
            // dire, su una cartella di rete che dorme, aspettare ogni volta la
            // stessa scadenza per lo stesso no.
            con_registro(app, |registro| {
                registro.ricorda_copia(voce.uid, None, trovata.generazione);
            });
            None
        }
    }
}

/// Una copia trovata, con la generazione del registro in cui la si è trovata:
/// vedi [`Registro::generazione`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Trovata {
    riga: i64,
    generazione: u64,
}

/// La riga della copia locale di una voce, cercata una volta per `uid`.
fn copia_locale(
    app: &tauri::AppHandle,
    uid: u64,
    descrittore: &rymembra_insieme::messaggi::DescrittoreCopia,
    durata_ms: u64,
) -> Option<Trovata> {
    let stato = app.try_state::<StatoLettore>()?;
    copia_ricordata(&stato.ospiti, uid, || {
        cerca_copia(app, descrittore, durata_ms)
    })
}

/// L'esito ricordato per `uid`, o quello di `cerca`, che si ricorda se è un
/// esito. `cerca` gira **senza** il lucchetto del registro in mano: legge il
/// database e il disco, e il registro lo legge chi disegna la coda.
///
/// Due aperture della stessa voce nello stesso istante — il preparatore e il
/// regolatore — possono cercare tutte e due: costa una ricerca in più, una
/// volta, ed è meglio che far aspettare l'una dietro l'altra.
///
/// L'esito si scrive solo se la sessione è ancora quella in cui la ricerca è
/// partita: vedi [`Registro::generazione`].
fn copia_ricordata(
    registro: &Mutex<Registro>,
    uid: u64,
    cerca: impl FnOnce() -> Option<Option<i64>>,
) -> Option<Trovata> {
    let leggi = || {
        registro
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    };
    let (ricordata, generazione) = {
        let registro = leggi();
        (registro.copia(uid), registro.generazione)
    };
    let riga = match ricordata {
        Some(ricordata) => ricordata,
        None => {
            let trovata = cerca()?;
            leggi().ricorda_copia(uid, trovata, generazione);
            trovata
        }
    };
    riga.map(|riga| Trovata { riga, generazione })
}

/// Cerca la copia: i candidati dal database, poi la verifica sul disco con la
/// scadenza di un'apertura. Fuori `None` quando non è un esito — il database
/// non ha risposto, il disco nemmeno — e allora non si ricorda niente.
fn cerca_copia(
    app: &tauri::AppHandle,
    descrittore: &rymembra_insieme::messaggi::DescrittoreCopia,
    durata_ms: u64,
) -> Option<Option<i64>> {
    let descrittore = descrittore_da(descrittore);
    let stato = super::libreria_di(app, "copia locale").ok()?;
    // Con un'attesa corta: la riapertura del regolatore passa di qui, e una
    // scansione che tiene la libreria non deve fermare una voce che dalla rete
    // si aprirebbe subito. Prima della fase 3 una voce in rete il database di
    // chi ascolta non lo toccava affatto.
    let candidati = match crate::stato::con_libreria_entro(&stato, ATTESA_COPIA, |libreria| {
        rymembra_app::insieme::candidati_copia(&libreria.connection, &descrittore, durata_ms)
    }) {
        Some(Ok(candidati)) => candidati,
        Some(Err(err)) => {
            crate::nota!(
                "[insieme] copia locale non cercata: {}",
                err.code().kind().code()
            );
            return None;
        }
        None => return None,
    };
    if candidati.is_empty() {
        return Some(None);
    }
    let esito = rymembra_app::scadenza::con_scadenza_o_caduta(
        "insieme-copia",
        super::APERTURA_BRANO,
        move || {
            scegli_copia(candidati, |percorso| {
                rymembra_app::insieme::e_la_stessa(percorso, &descrittore)
            })
        },
    );
    match esito {
        Ok(trovata) => Some(trovata),
        Err(_) => {
            crate::nota!("[insieme] la verifica della copia locale non ha risposto: dalla rete");
            None
        }
    }
}

/// Quanto la ricerca della copia aspetta la libreria: un quinto di secondo,
/// dentro l'anticipo di 250 ms con cui partono le voci. Oltre, la voce si apre
/// dalla rete, e la ricerca si rifà alla prossima apertura.
const ATTESA_COPIA: std::time::Duration = std::time::Duration::from_millis(200);

/// Il primo candidato che è una copia: certo, o verificato da `verifica`.
/// Nell'ordine del database, che mette prima i migliori; un candidato certo
/// non si verifica, perché è lo stesso file.
fn scegli_copia(
    candidati: Vec<rymembra_app::insieme::Candidato>,
    verifica: impl Fn(&std::path::Path) -> bool,
) -> Option<i64> {
    candidati
        .into_iter()
        .find(|candidato| candidato.certa || verifica(&candidato.percorso))
        .map(|candidato| candidato.track_id)
}

/// Il descrittore del trasporto nella forma della libreria: gli stessi campi,
/// in due crate che non si vedono.
fn descrittore_da(
    copia: &rymembra_insieme::messaggi::DescrittoreCopia,
) -> rymembra_app::insieme::Descrittore {
    rymembra_app::insieme::Descrittore {
        unc: copia.unc.clone(),
        dimensione: copia.dimensione,
        modificato_ms: copia.modificato_ms,
        impronta_rapida: copia.impronta_rapida.clone(),
        chiave_contenuto: copia.chiave_contenuto.clone(),
    }
}

/// La voce `id` si apre da un file di questo PC: una riga della libreria, una
/// voce che ne apre una, o una voce in rete di cui si è trovata la copia.
#[must_use]
pub(super) fn si_apre_da_qui(app: &tauri::AppHandle, id: i64) -> bool {
    match chi(app, id) {
        Chi::Libreria(_) => true,
        Chi::Voce(voce) => match voce.apertura {
            Apertura::Libreria { .. } => true,
            Apertura::Rete { .. } => con_registro(app, |registro| registro.copia_trovata(voce.uid))
                .flatten()
                .is_some(),
            Apertura::Catalogo { .. } => false,
        },
        Chi::Sparita => false,
    }
}

/// Il tratto di un'immagine CD nei fotogrammi del CD (1/75 s), dai
/// millisecondi della voce.
///
/// L'host li ha portati in millisecondi arrotondando un numero di
/// fotogrammi; qui si torna indietro arrotondando di nuovo, e il giro è
/// esatto: l'errore di andata è al più mezzo millisecondo, cioè meno di un
/// ventesimo di fotogramma, e il secondo arrotondamento lo assorbe.
fn tratto(immagine: String, inizio_ms: u64, fine_ms: Option<u64>) -> rymembra_play::Segmento {
    let a_cd = |ms: u64| -> u32 {
        let cd = ms
            .saturating_mul(3)
            .saturating_add(20)
            .checked_div(40)
            .unwrap_or(0);
        u32::try_from(cd).unwrap_or(u32::MAX)
    };
    rymembra_play::Segmento {
        immagine,
        inizio_cd: a_cd(inizio_ms),
        fine_cd: fine_ms.map(a_cd),
    }
}

// ── la voce di calibrazione ─────────────────────────────────────────────────

/// La voce di calibrazione, uguale su ogni PC: clic generati qui, nessun byte
/// in rete.
///
/// Un'[`Apertura::Rete`] con una chiusura che non va in rete: così la voce
/// passa per **la stessa strada** di ogni voce di un altro PC — `FlussoHttp`,
/// l'involucro senza salti grossolani, il decodificatore WAV, il
/// ricampionatore, l'avvio programmato — e quel che si calibra è il percorso
/// vero, non un percorso a parte che potrebbe avere un ritardo suo.
pub(super) fn voce_di_calibrazione() -> Arc<Voce> {
    static VOCE: std::sync::OnceLock<Arc<Voce>> = std::sync::OnceLock::new();
    Arc::clone(VOCE.get_or_init(|| {
        let clic = Arc::new(Clic::nuovo());
        let durata_ms = clic.durata_ms();
        let apri: ApriInRete = Arc::new(move || {
            Ok(Box::new(ClicCondivisi(Arc::clone(&clic)))
                as Box<dyn rymembra_net::flusso::Sorgente>)
        });
        Arc::new(Voce {
            uid: UID_CALIBRAZIONE,
            proprietario: None,
            scheda: SchedaVoce {
                // Il titolo resta in italiano, come il resto di quel che nasce
                // nel nucleo: la finestra riconosce la voce dalla maniglia (o
                // dall'ancora) e scrive la sua frase. Vedi il rapporto.
                titolo: "Calibrazione".to_owned(),
                artista: "Rymembra".to_owned(),
                album: String::new(),
                durata_ms,
                numero: None,
                anno: None,
                copertina: None,
                // Nessuna correzione: i clic sono già al livello voluto, e la
                // normalizzazione di ciascuno li porterebbe a livelli diversi
                // su PC diversi solo se la voce dichiarasse un guadagno.
                replaygain_db: None,
                picco: None,
            },
            apertura: Apertura::Rete {
                apri,
                estensione: Some("wav".to_owned()),
                segmento: None,
                immagine: None,
                copia: None,
            },
            da_contare: None,
        })
    }))
}

/// La frequenza dei clic. 48 kHz su ogni PC: chi ha l'uscita a 44,1 la
/// ricampiona, e il ritardo del ricampionatore il motore lo compensa già
/// (`ricampionatore.rs`), quindi il clic esce allo stesso istante.
const CLIC_FREQUENZA: u32 = 48_000;

/// Quanto dura la voce: un'ora, ben oltre una calibrazione. Non sta in
/// memoria — vedi [`Clic`] — quindi la durata non costa niente.
const CLIC_DURATA_S: u64 = 60 * 60;

/// Una raffica: 2 ms, come in `genera_clic`. Breve perché l'orecchio la
/// localizzi senza ambiguità: un suono lungo si fonde fra le due casse e il
/// «da dove viene» smette di avere una risposta.
const CLIC_RAFFICA: u64 = 96;

/// La portante della raffica: 4 kHz, come in `genera_clic`, perché passa da
/// qualunque cassa e l'orecchio la localizza bene.
const CLIC_PORTANTE_HZ: f64 = 4000.0;

/// −6 dBFS di picco: due PC nella stessa stanza si sommano senza saturare.
const CLIC_AMPIEZZA: f64 = 0.5;

/// Il seme del generatore, lo stesso di `genera_clic`: gli istanti sono
/// identici su ogni PC, ed è quel che fa sentire un clic solo quando due PC
/// sono a tempo.
const CLIC_SEME: u64 = 0x5EED_C11C;

/// L'intestazione di un WAV PCM: 44 byte.
const TESTA_WAV: u64 = 44;

/// I clic, come un file WAV mono a 16 bit che non esiste: i byte si
/// calcolano quando qualcuno li chiede.
///
/// Un'ora a 48 kHz sono 345 MB; scriverli in memoria per una calibrazione di
/// un minuto sarebbe assurdo. Si tengono solo gli istanti d'inizio delle
/// raffiche (qualche migliaio di numeri) e ogni campione si calcola dal più
/// vicino che lo precede, con la stessa formula di `genera_clic`: raffiche di
/// 2 ms con inviluppo di Hann a intervalli pseudo-casuali fra 400 e 600 ms.
/// Intervalli irregolari perché una fila regolare è ambigua: con un clic ogni
/// mezzo secondo esatto, due PC sfasati di mezzo secondo sembrerebbero a
/// tempo.
#[derive(Debug)]
struct Clic {
    /// Il fotogramma d'inizio di ogni raffica, in ordine.
    inizi: Vec<u64>,
    /// Quanti fotogrammi in tutto.
    fotogrammi: u64,
}

impl Clic {
    fn nuovo() -> Self {
        let fotogrammi = CLIC_DURATA_S.saturating_mul(u64::from(CLIC_FREQUENZA));
        let mut inizi = Vec::new();
        let mut caso = CLIC_SEME;
        // In microsecondi interi, come `genera_clic`: lo stesso istante è lo
        // stesso numero a ogni frequenza.
        let mut t_us: u64 = 500_000;
        loop {
            let fotogramma = t_us
                .saturating_mul(u64::from(CLIC_FREQUENZA))
                .checked_div(1_000_000)
                .unwrap_or(u64::MAX);
            if fotogramma.saturating_add(CLIC_RAFFICA) >= fotogrammi {
                break;
            }
            inizi.push(fotogramma);
            caso = caso
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let salto = (caso >> 33).checked_rem(200_001).unwrap_or(0);
            t_us = t_us.saturating_add(400_000).saturating_add(salto);
        }
        Self { inizi, fotogrammi }
    }

    /// Quanto dura, in millisecondi.
    fn durata_ms(&self) -> u64 {
        self.fotogrammi
            .saturating_mul(1000)
            .checked_div(u64::from(CLIC_FREQUENZA))
            .unwrap_or(0)
    }

    /// Quanti byte ha il file.
    fn totale(&self) -> u64 {
        TESTA_WAV.saturating_add(self.fotogrammi.saturating_mul(2))
    }

    /// L'intestazione: PCM intero, un canale, 16 bit.
    fn testa(&self) -> [u8; 44] {
        let dati = u32::try_from(self.fotogrammi.saturating_mul(2)).unwrap_or(u32::MAX - 36);
        let mut testa = [0_u8; 44];
        let pezzi: [&[u8]; 13] = [
            b"RIFF",
            &dati.saturating_add(36).to_le_bytes(),
            b"WAVE",
            b"fmt ",
            &16_u32.to_le_bytes(),
            &1_u16.to_le_bytes(),
            &1_u16.to_le_bytes(),
            &CLIC_FREQUENZA.to_le_bytes(),
            &CLIC_FREQUENZA.saturating_mul(2).to_le_bytes(),
            &2_u16.to_le_bytes(),
            &16_u16.to_le_bytes(),
            b"data",
            &dati.to_le_bytes(),
        ];
        let mut dove = testa.iter_mut();
        for byte in pezzi.iter().flat_map(|pezzo| pezzo.iter()) {
            if let Some(posto) = dove.next() {
                *posto = *byte;
            }
        }
        testa
    }

    /// Il campione del fotogramma `n`.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "il valore è arrotondato e tagliato all'intervallo di un i16 prima della \
                  conversione"
    )]
    fn campione(&self, n: u64) -> i16 {
        let dopo = self.inizi.partition_point(|&inizio| inizio <= n);
        let Some(&inizio) = dopo.checked_sub(1).and_then(|i| self.inizi.get(i)) else {
            return 0;
        };
        let dentro = n.saturating_sub(inizio);
        if dentro >= CLIC_RAFFICA {
            return 0;
        }
        let f = f64::from(CLIC_FREQUENZA);
        let tau = dentro as f64 / f;
        let lunga = CLIC_RAFFICA as f64 / f;
        let inviluppo = (std::f64::consts::PI * tau / lunga).sin().powi(2);
        let v = CLIC_AMPIEZZA * inviluppo * (std::f64::consts::TAU * CLIC_PORTANTE_HZ * tau).sin();
        (v * 32767.0).round().clamp(-32768.0, 32767.0) as i16
    }

    /// I byte da `da`, al più `quanti`, come li leggerebbe un file.
    fn byte(&self, da: u64, quanti: u64) -> Vec<u8> {
        let fine = da.saturating_add(quanti).min(self.totale());
        let lunghi = usize::try_from(fine.saturating_sub(da)).unwrap_or(0);
        let mut fuori = Vec::with_capacity(lunghi);
        let testa = self.testa();
        let mut posto = da;
        while posto < fine.min(TESTA_WAV) {
            let indice = usize::try_from(posto).unwrap_or(usize::MAX);
            fuori.push(testa.get(indice).copied().unwrap_or(0));
            posto = posto.saturating_add(1);
        }
        while posto < fine {
            let dentro = posto.saturating_sub(TESTA_WAV);
            let [basso, alto] = self.campione(dentro >> 1).to_le_bytes();
            if dentro & 1 == 0 {
                fuori.push(basso);
                if posto.saturating_add(1) < fine {
                    fuori.push(alto);
                }
                posto = posto.saturating_add(2);
            } else {
                fuori.push(alto);
                posto = posto.saturating_add(1);
            }
        }
        fuori
    }
}

/// I clic, condivisi fra tutte le aperture: gli istanti si calcolano una
/// volta per processo.
#[derive(Debug)]
struct ClicCondivisi(Arc<Clic>);

impl rymembra_net::flusso::Sorgente for ClicCondivisi {
    fn pezzo(&self, da: u64, quanti: u64) -> Result<rymembra_net::Pezzo, AppError> {
        Ok(rymembra_net::Pezzo {
            byte: self.0.byte(da, quanti),
            totale: Some(self.0.totale()),
            url_finale: None,
        })
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    fn voce(uid: u64, titolo: &str) -> Voce {
        Voce {
            uid,
            proprietario: Some("PC di Marco".to_owned()),
            scheda: SchedaVoce {
                titolo: titolo.to_owned(),
                artista: "Interprete".to_owned(),
                album: "Disco".to_owned(),
                durata_ms: 180_000,
                numero: Some(3),
                anno: Some(1977),
                copertina: Some(format!("insieme-{uid}")),
                replaygain_db: Some(-6.5),
                picco: Some(0.9),
            },
            apertura: Apertura::Catalogo {
                riferimento: "internet-archive:https://archive.org/download/x/y.mp3".to_owned(),
            },
            da_contare: None,
        }
    }

    #[test]
    fn un_id_positivo_e_della_libreria_e_gli_altri_no() {
        assert!(e_di_libreria(1));
        assert!(e_di_libreria(i64::MAX));
        assert!(!e_di_libreria(0));
        assert!(!e_di_libreria(-1));
        assert!(!e_di_libreria(MANIGLIA_CALIBRAZIONE));
    }

    #[test]
    fn chi_distingue_i_tre_casi() {
        let mut registro = Registro::default();
        let maniglia = registro.registra(voce(7, "Dogs"));
        // I tre casi: la riga, la voce, la maniglia che non c'è più.
        let caso = |id: i64| chi_nel(Some(&registro), id);
        assert!(matches!(caso(42), Chi::Libreria(42)));
        assert!(matches!(caso(maniglia), Chi::Voce(v) if v.uid == 7));
        assert!(matches!(caso(maniglia - 1), Chi::Sparita));
        assert!(matches!(caso(0), Chi::Sparita));
        assert!(matches!(
            caso(MANIGLIA_CALIBRAZIONE),
            Chi::Voce(v) if v.uid == UID_CALIBRAZIONE
        ));
        // Senza registro — l'applicazione che si chiude — le righe restano
        // righe e ogni maniglia è sparita, tranne la calibrazione.
        assert!(matches!(chi_nel(None, 42), Chi::Libreria(42)));
        assert!(matches!(chi_nel(None, maniglia), Chi::Sparita));
        assert!(matches!(chi_nel(None, MANIGLIA_CALIBRAZIONE), Chi::Voce(_)));
    }

    #[test]
    fn lo_stesso_uid_tiene_la_stessa_maniglia() {
        let mut registro = Registro::default();
        let prima = registro.registra(voce(7, "Dogs"));
        let altra = registro.registra(voce(8, "Pigs"));
        assert!(prima < 0 && altra < 0 && prima != altra);
        // Una voce aggiornata — la scheda arrivata col nuovo stato — resta
        // sotto la maniglia che la coda e il motore hanno già in mano.
        let dopo = registro.registra(voce(7, "Dogs (live)"));
        assert_eq!(dopo, prima);
        assert_eq!(registro.maniglia(7), Some(prima));
        assert_eq!(
            registro.voce(prima).map(|v| v.scheda.titolo.clone()),
            Some("Dogs (live)".to_owned())
        );
        assert_eq!(registro.quante(), 2);
    }

    #[test]
    fn a_sessione_finita_le_maniglie_spariscono_e_non_tornano() {
        let mut registro = Registro::default();
        let vecchia = registro.registra(voce(1, "A"));
        registro.svuota();
        assert!(registro.voce(vecchia).is_none(), "sessione finita: sparita");
        assert_eq!(registro.maniglia(1), None);
        // La sessione dopo riparte dagli `uid`, non dalle maniglie: lo stesso
        // `uid` 1 ha una maniglia nuova, e quella vecchia resta sparita.
        assert_eq!(registro.nuovo_uid(), 1);
        let nuova = registro.registra(voce(1, "B"));
        assert_ne!(nuova, vecchia);
        assert!(registro.voce(vecchia).is_none());
    }

    #[test]
    fn la_calibrazione_ha_la_sua_maniglia_fissa_e_non_entra_nel_registro() {
        let mut registro = Registro::default();
        let mut finta = voce(UID_CALIBRAZIONE, "finta");
        finta.da_contare = Some(5);
        assert_eq!(registro.registra(finta), MANIGLIA_CALIBRAZIONE);
        assert_eq!(registro.quante(), 0);
        assert_eq!(
            registro.maniglia(UID_CALIBRAZIONE),
            Some(MANIGLIA_CALIBRAZIONE)
        );
    }

    #[test]
    fn gli_uid_di_chi_ospita_partono_da_uno() {
        let mut registro = Registro::default();
        assert_eq!(registro.nuovo_uid(), 1);
        assert_eq!(registro.nuovo_uid(), 2);
        registro.svuota();
        assert_eq!(registro.nuovo_uid(), 1);
    }

    #[test]
    fn le_maniglie_delle_righe_trovano_solo_le_voci_di_libreria() {
        let mut registro = Registro::default();
        let mut mia = voce(1, "mia");
        mia.apertura = Apertura::Libreria { track_id: 40 };
        let mia = registro.registra(mia);
        let _altrui = registro.registra(voce(2, "altrui"));
        assert_eq!(registro.maniglie_di_righe(&[40, 41]), vec![mia]);
        assert!(registro.maniglie_di_righe(&[41]).is_empty());
    }

    #[test]
    fn la_scheda_di_una_voce_non_accende_i_gesti_di_libreria() {
        let scheda = scheda(-12, &voce(7, "Dogs"));
        assert_eq!(scheda.id, -12, "l'id è la maniglia");
        assert_eq!(scheda.path, None);
        assert_eq!(scheda.fonte, None);
        assert_eq!(scheda.album_key, None);
        assert_eq!(scheda.title, "Dogs");
        assert_eq!(scheda.duration_ms, 180_000);
        assert_eq!(scheda.track_number, Some(3));
        assert_eq!(scheda.year, Some(1977));
        assert_eq!(scheda.cover_art_hash.as_deref(), Some("insieme-7"));
        assert!(!scheda.liked);
        assert_eq!(scheda.rating, 0);
    }

    #[test]
    fn la_guardia_rifiuta_le_voci_col_codice_della_libreria() {
        assert_eq!(riga_di_libreria(5).ok(), Some(5));
        let rifiuto = riga_di_libreria(-3).err().map(|e| e.code().kind());
        assert_eq!(
            rifiuto,
            Some(rymembra_domain::errors::ErrorCodeKind::LibraryTrackNotFound)
        );
        assert!(righe_di_libreria(&[1, 2, 3]).is_ok());
        assert!(righe_di_libreria(&[1, -2, 3]).is_err());
        assert!(righe_di_libreria(&[0]).is_err());
        assert!(righe_di_libreria(&[]).is_ok());
    }

    #[test]
    fn i_clic_sono_un_wav_valido_e_uguale_a_ogni_lettura() {
        let clic = Clic::nuovo();
        let testa = clic.byte(0, 44);
        assert_eq!(testa.get(0..4), Some(&b"RIFF"[..]));
        assert_eq!(testa.get(8..16), Some(&b"WAVEfmt "[..]));
        assert_eq!(testa.get(36..40), Some(&b"data"[..]));
        assert_eq!(clic.totale(), 44 + 48_000 * 3600 * 2);
        assert_eq!(clic.durata_ms(), 3_600_000);
        // Lo stesso tratto letto in un colpo o a pezzi dispari dà gli stessi
        // byte: il decodificatore salta e rilegge, e un byte spostato sarebbe
        // un campione con i due mezzi scambiati.
        let intero = clic.byte(40, 10_000);
        let mut a_pezzi = clic.byte(40, 3);
        a_pezzi.extend(clic.byte(43, 4_001));
        a_pezzi.extend(clic.byte(4_044, 5_996));
        assert_eq!(intero, a_pezzi);
        // Oltre la fine non c'è niente.
        assert!(clic.byte(clic.totale(), 10).is_empty());
        assert_eq!(clic.byte(clic.totale() - 3, 10).len(), 3);
    }

    #[test]
    fn i_clic_cadono_dove_li_mette_genera_clic() {
        let clic = Clic::nuovo();
        // La prima raffica a mezzo secondo, come nel file della misura.
        assert_eq!(clic.inizi.first().copied(), Some(24_000));
        // Silenzio prima, suono dentro, silenzio subito dopo.
        assert_eq!(clic.campione(23_999), 0);
        assert!((24_001..24_000 + CLIC_RAFFICA).any(|n| clic.campione(n) != 0));
        assert_eq!(clic.campione(24_000 + CLIC_RAFFICA), 0);
        // Gli intervalli fra 400 e 600 ms, e mai regolari per tutta la fila.
        let passi: Vec<u64> = clic.inizi.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(passi.iter().all(|p| (19_200..=28_800).contains(p)));
        assert!(passi.windows(2).any(|w| w[0] != w[1]));
        // E gli stessi a ogni costruzione: è quel che li rende uguali su
        // ogni PC.
        assert_eq!(Clic::nuovo().inizi, clic.inizi);
    }

    /// I clic si decodificano per la stessa strada di una voce in rete — la
    /// finestra scorrevole, l'involucro di sessione, il decodificatore WAV — e
    /// un salto li trova al fotogramma: dal punto 400 ms la prima raffica
    /// arriva esattamente 100 ms dopo, preceduta da silenzio.
    #[test]
    fn i_clic_si_decodificano_e_il_salto_li_trova_al_fotogramma() {
        let voce = voce_di_calibrazione();
        let Apertura::Rete { apri, .. } = &voce.apertura else {
            panic!("la calibrazione si apre come una voce in rete");
        };
        let sorgente = apri().expect("i clic non vanno in rete");
        let flusso = rymembra_net::FlussoHttp::da(sorgente).expect("finestra");
        let sorgente = rymembra_play::Sorgente {
            track_id: MANIGLIA_CALIBRAZIONE,
            media: Box::new(super::super::flusso::FlussoDiPari::per_prova(flusso)),
            estensione: Some("wav".to_owned()),
            durata_ms: voce.scheda.durata_ms,
            replaygain_db: None,
            picco: None,
            segmento: None,
        };
        let mut decodificatore =
            rymembra_play::decodifica::Decodificatore::apri(sorgente, 48_000, 2)
                .expect("un WAV si apre");
        decodificatore.cerca(400).expect("il salto riesce");
        let mut campioni = Vec::new();
        // Duecento millesimi di secondo, stereo: oltre il clic a 100 ms.
        while campioni.len() < 19_200 {
            let mut blocco = Vec::new();
            if !decodificatore.prossimo(&mut blocco).expect("si decodifica") {
                break;
            }
            campioni.extend(blocco);
        }
        // Stereo interlacciato: 100 ms sono 4 800 fotogrammi, 9 600 campioni.
        let prima = &campioni[..9_590];
        assert!(
            prima.iter().all(|c| c.abs() < 1e-3),
            "silenzio prima del clic"
        );
        let lunga = 2 * usize::try_from(CLIC_RAFFICA).unwrap_or(96);
        let raffica = &campioni[9_600..9_600 + lunga];
        assert!(raffica.iter().any(|c| c.abs() > 0.1), "il clic è lì");
    }

    /// Una riga della libreria per le prove della copia: le colonne che
    /// `candidati_copia` guarda.
    fn riga_in_tabella(
        c: &rusqlite::Connection,
        id: i64,
        percorso: &str,
        dimensione: i64,
        data: i64,
        chiave: Option<&str>,
    ) {
        c.execute(
            "INSERT INTO tracks (id, path, track_key, title, artist, album, duration_ms,
                                 file_size, date_added, date_modified, content_key)
             VALUES (?1, ?2, 'k' || ?1, 'T', 'A', 'B', 200000, ?3, 0, ?4, ?5)",
            rusqlite::params![id, percorso, dimensione, data, chiave],
        )
        .expect("riga");
    }

    fn file_con(cartella: &std::path::Path, nome: &str, byte: &[u8]) -> std::path::PathBuf {
        let percorso = cartella.join(nome);
        std::fs::write(&percorso, byte).expect("file");
        percorso
    }

    /// Lo stesso file sulla condivisione è una copia certa: si usa senza
    /// leggere un byte.
    #[test]
    fn una_copia_certa_si_usa_senza_verificarla() {
        let c = rymembra_app::db::open_in_memory()
            .expect("database")
            .connection;
        riga_in_tabella(&c, 1, r"\\nas\m\a.flac", 5_000_000, 1_000, None);
        let descrittore = rymembra_app::insieme::Descrittore {
            unc: Some(r"\\nas\m\a.flac".to_owned()),
            dimensione: 5_000_000,
            modificato_ms: 1_000,
            impronta_rapida: "0".repeat(64),
            chiave_contenuto: None,
        };
        let candidati =
            rymembra_app::insieme::candidati_copia(&c, &descrittore, 200_000).expect("candidati");
        let trovata = scegli_copia(candidati, |_| panic!("una copia certa non si verifica"));
        assert_eq!(trovata, Some(1));
    }

    /// Una copia per chiave di contenuto si verifica sull'impronta: la stessa
    /// musica al byte passa, un file della stessa dimensione con un'altra
    /// impronta si scarta.
    #[test]
    fn una_copia_si_verifica_e_una_diversa_si_scarta() {
        let cartella = tempfile::tempdir().expect("cartella");
        let musica: Vec<u8> = (0..300_000_u32)
            .map(|n| u8::try_from(n % 251).unwrap_or(0))
            .collect();
        let mut altra = musica.clone();
        if let Some(byte) = altra.get_mut(1_000) {
            *byte ^= 0xFF;
        }
        // Il file dell'altro PC, e quello di qui: stessi byte.
        let di_la = file_con(cartella.path(), "di-la.flac", &musica);
        let qui = file_con(cartella.path(), "qui.flac", &musica);
        let diverso = file_con(cartella.path(), "diverso.flac", &altra);
        let c = rymembra_app::db::open_in_memory()
            .expect("database")
            .connection;
        let dimensione = i64::try_from(musica.len()).expect("dimensione");
        riga_in_tabella(&c, 1, &qui.to_string_lossy(), dimensione, 7, Some("K"));
        riga_in_tabella(&c, 2, &diverso.to_string_lossy(), dimensione, 7, Some("D"));

        let descrittore = rymembra_app::insieme::descrittore(&di_la, Some("K")).expect("letto");
        let candidati =
            rymembra_app::insieme::candidati_copia(&c, &descrittore, 200_000).expect("candidati");
        assert!(
            candidati.iter().all(|c| !c.certa),
            "per chiave: da verificare"
        );
        let verifica =
            |percorso: &std::path::Path| rymembra_app::insieme::e_la_stessa(percorso, &descrittore);
        assert_eq!(scegli_copia(candidati, verifica), Some(1));

        // La riga 2 ha un'altra chiave; con la stessa chiave, la sua impronta
        // la scarta lo stesso.
        let descrittore = rymembra_app::insieme::descrittore(&di_la, Some("D")).expect("letto");
        let candidati =
            rymembra_app::insieme::candidati_copia(&c, &descrittore, 200_000).expect("candidati");
        assert_eq!(
            candidati.iter().map(|c| c.track_id).collect::<Vec<_>>(),
            [2]
        );
        let verifica =
            |percorso: &std::path::Path| rymembra_app::insieme::e_la_stessa(percorso, &descrittore);
        assert_eq!(scegli_copia(candidati, verifica), None);
    }

    /// La ricerca si fa una volta per `uid`, trovata o no; un «non so» — il
    /// database o il disco che non rispondono — non si ricorda, e si riprova.
    #[test]
    fn la_copia_si_cerca_una_volta_per_uid() {
        let registro = Mutex::new(Registro::default());
        let cercate = std::cell::Cell::new(0);
        let cerca = |esito: Option<Option<i64>>| {
            let cercate = &cercate;
            move || {
                cercate.set(cercate.get() + 1);
                esito
            }
        };
        let riga = |trovata: Option<Trovata>| trovata.map(|t| t.riga);
        assert_eq!(
            riga(copia_ricordata(&registro, 7, cerca(Some(Some(40))))),
            Some(40)
        );
        assert_eq!(
            riga(copia_ricordata(&registro, 7, cerca(Some(None)))),
            Some(40)
        );
        assert_eq!(cercate.get(), 1, "la seconda apertura non cerca");
        // Non trovata: si ricorda anche quello.
        assert_eq!(riga(copia_ricordata(&registro, 8, cerca(Some(None)))), None);
        assert_eq!(
            riga(copia_ricordata(&registro, 8, cerca(Some(Some(41))))),
            None
        );
        assert_eq!(cercate.get(), 2);
        // Un «non so» non si ricorda.
        assert_eq!(riga(copia_ricordata(&registro, 9, cerca(None))), None);
        assert_eq!(
            riga(copia_ricordata(&registro, 9, cerca(Some(Some(42))))),
            Some(42)
        );
        assert_eq!(cercate.get(), 4);
        // Trovata ma non si apre: da lì in poi la rete, senza cercare più.
        let generazione = registro.lock().expect("registro").generazione;
        registro
            .lock()
            .expect("registro")
            .ricorda_copia(7, None, generazione);
        assert_eq!(
            riga(copia_ricordata(&registro, 7, cerca(Some(Some(40))))),
            None
        );
        assert_eq!(cercate.get(), 4);
    }

    /// Una ricerca ancora in corso quando la sessione finisce non scrive il
    /// suo esito nella sessione dopo, dove lo stesso `uid` è un'altra voce.
    #[test]
    fn una_ricerca_finita_dopo_l_uscita_non_entra_nella_sessione_dopo() {
        let registro = Mutex::new(Registro::default());
        let trovata = copia_ricordata(&registro, 3, || {
            // A metà ricerca la sessione finisce.
            registro.lock().expect("registro").svuota();
            Some(Some(40))
        });
        // Chi l'ha chiesta la usa — è per la voce che stava aprendo — ma il
        // registro della sessione nuova non ne sa niente.
        assert_eq!(trovata.map(|t| t.riga), Some(40));
        assert_eq!(registro.lock().expect("registro").copia(3), None);
        // E un guasto segnato con la generazione vecchia non scrive niente.
        let vecchia = trovata.map_or(0, |t| t.generazione);
        registro
            .lock()
            .expect("registro")
            .ricorda_copia(3, None, vecchia);
        assert_eq!(registro.lock().expect("registro").copia(3), None);
    }

    /// Una voce in rete conta solo con una copia verificata; una voce di
    /// libreria conta la sua riga; una maniglia non conta mai.
    #[test]
    fn da_contare_cambia_solo_con_una_copia_verificata() {
        let in_rete = voce(3, "Dogs");
        assert_eq!(conta_della_voce(&in_rete, None), None);
        assert_eq!(conta_della_voce(&in_rete, Some(40)), Some(40));
        let mut mia = voce(4, "Pigs");
        mia.apertura = Apertura::Libreria { track_id: 12 };
        mia.da_contare = Some(12);
        assert_eq!(conta_della_voce(&mia, None), Some(12));
        assert_eq!(conta_della_voce(&mia, Some(40)), Some(12));
        assert_eq!(conta_della_voce(&in_rete, Some(-2)), None);
    }

    /// Le voci di un membro che esce escono dal registro, con la loro copia.
    #[test]
    fn togliere_le_voci_le_fa_sparire_col_loro_ricordo() {
        let mut registro = Registro::default();
        let tolta = registro.registra(voce(5, "A"));
        let resta = registro.registra(voce(6, "B"));
        registro.ricorda_copia(5, Some(40), registro.generazione);
        registro.togli(&[tolta]);
        assert!(registro.voce(tolta).is_none());
        assert_eq!(registro.maniglia(5), None);
        assert_eq!(registro.copia(5), None);
        assert!(registro.voce(resta).is_some());
    }

    /// Una riapertura lunga che finisce dopo l'uscita, o in una sessione
    /// nuova, non consegna la voce vecchia: la maniglia non è più viva.
    #[test]
    fn una_voce_trovata_prima_dell_uscita_non_e_piu_viva_dopo() {
        let mut registro = Registro::default();
        let maniglia = registro.registra(voce(1, "A"));
        let generazione = registro.generazione();
        assert!(registro.ancora_viva(maniglia, generazione));
        // La sessione finisce e ne comincia un'altra con lo stesso `uid`.
        registro.svuota();
        let nuova = registro.registra(voce(1, "B"));
        assert!(!registro.ancora_viva(maniglia, generazione));
        assert!(
            !registro.ancora_viva(nuova, generazione),
            "un'altra generazione"
        );
        assert!(registro.ancora_viva(nuova, registro.generazione()));
        // Una voce tolta (un membro uscito) non è viva nemmeno nella stessa
        // generazione.
        let generazione = registro.generazione();
        registro.togli(&[nuova]);
        assert!(!registro.ancora_viva(nuova, generazione));
    }

    #[test]
    fn i_millisecondi_di_un_tratto_tornano_fotogrammi_del_cd_esatti() {
        for cd in [0_u32, 1, 2, 74, 75, 13_337, 330_000] {
            // Come li porta l'host: arrotondati al millisecondo.
            let ms = (u64::from(cd) * 1000 * 2 + 75)
                .checked_div(150)
                .unwrap_or(0);
            let segmento = tratto("x".to_owned(), ms, Some(ms));
            assert_eq!(segmento.inizio_cd, cd, "{cd} fotogrammi → {ms} ms");
            assert_eq!(segmento.fine_cd, Some(cd));
        }
    }
}
