//! Le cartelle sorvegliate: la libreria si accorge da sé di quel che cambia sul
//! disco.
//!
//! # Il difetto che questo modulo toglie
//!
//! Fino alla 2.4 la libreria guardava il disco solo quando qualcuno premeva
//! «Scansiona», o quando la coda dei download finiva. Un album copiato nella
//! cartella della musica non compariva; uno cancellato da Esplora risorse
//! restava in elenco, e al primo clic diceva «il file non c'è». La risposta
//! che tutti i lettori danno — riscansionare tutto ogni tanto — su una
//! libreria da cinquantamila brani è una camminata intera per scoprire che è
//! cambiato un file.
//!
//! # Come
//!
//! Sui **dischi di questo computer** il sistema avvisa da sé: `notify` con
//! `ReadDirectoryChangesW`, un osservatore per cartella sorvegliata. Gli avvisi
//! arrivano a raffiche — copiare un album sono venti file, e per ognuno due o
//! tre eventi — e si **accorpano**: si aspetta che la raffica taccia per
//! [`ACCORPAMENTO`], e poi si riscansionano soltanto le cartelle in cui è
//! successo qualcosa (`rymembra_app::library::cartelle_da_riguardare`). Un album
//! copiato è una scansione di un album, non della libreria.
//!
//! Sulle **condivisioni di rete** l'avviso può non arrivare: dipende dal
//! server, e quando la rete cade cade in silenzio. Lì, solo se chi usa Rymembra
//! lo chiede, un **sondaggio** leggero ogni [`SONDAGGIO`]: le date di modifica
//! delle cartelle, non i file e meno che mai i tag. Una cartella in cui è
//! comparso, sparito o stato rinominato qualcosa cambia data, e si riscansiona
//! quella. Spento di serie perché un NAS interrogato ogni cinque minuti è un
//! NAS i cui dischi non si addormentano mai.
//!
//! # Le scansioni che ne escono sono prudenti
//!
//! Nessuno le sta guardando: vale tutto quel che vale per la scansione
//! automatica di fine download — una radice che non risponde non giudica
//! nessuno, una sottocartella illeggibile non fa sparire niente, e una passata
//! che toglierebbe troppo in una volta non toglie niente da sola (vedi
//! `rymembra_app::library::Scan::prudente`). In più un sottoalbero è **solo**
//! quello: il resto della radice è fuori competenza e non può sparire.
//!
//! # Una scansione alla volta, e nessuna persa
//!
//! Il turno è quello di sempre (`Stato::turno_di_scansione`). Se lo tiene
//! qualcun altro — «Scansiona», la coda dei download — le cartelle da
//! riguardare restano in attesa e si riprova poco dopo: un avviso arrivato
//! durante una scansione a mano non va perso solo perché quella scansione
//! guardava altrove. La coda dei download fa lo stesso con
//! [`aspetta_il_turno`]: aspetta invece di lasciar perdere.
//!
//! # Dove si accende e si spegne
//!
//! Nelle Impostazioni, sezione Libreria: [`imposta_sorveglianza`]. Spegnerla
//! ferma gli osservatori subito, senza riavviare; la sorveglianza segue anche
//! l'elenco delle cartelle — aggiunta, tolta, traslocata — perché chi le cambia
//! la avvisa ([`riconfigura`]) e perché comunque ogni [`CONTROLLO`] rilegge
//! l'elenco da sé: un profilo importato o una sincronia le cambiano senza
//! passare dai comandi.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use notify::Watcher as _;
use rymembra_app::files::MusicFiles as _;
use rymembra_app::library::{Scadenze, Scan, cartelle_da_riguardare, piu_esterne};
use rymembra_app::volumi::{Collocazione, Esistenza};
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::paths::{PathRules, is_under};
use serde::Serialize;
use tauri::{AppHandle, Manager as _};

use crate::errore::{Esito, errore};
use crate::nota;
use crate::spegnimento::Emette as _;
use crate::stato::{DepositoStato, Stato, TurnoDiScansione, con_libreria};

/// Dove sta la scelta nelle impostazioni del database.
///
/// Fra le impostazioni della libreria e non nel `localStorage` della finestra:
/// la legge il filo di questo modulo, che parte prima che la finestra esista.
pub const CHIAVE_SORVEGLIANZA: &str = "library.watch";

/// Quanto silenzio dopo l'ultimo avviso prima di scansionare.
///
/// Due secondi: una copia di un album da un disco all'altro manda i suoi
/// eventi a pochi millisecondi l'uno dall'altro, e due secondi di silenzio
/// vogliono dire che la copia è finita — o che ne è finito un pezzo grosso.
/// Abbastanza corto da vedere il disco comparire mentre si guarda la finestra.
pub const ACCORPAMENTO: Duration = Duration::from_secs(2);

/// Quanto al massimo si rimanda una scansione mentre gli avvisi continuano.
///
/// Un minuto: chi copia cento gigabyte manda avvisi per mezz'ora senza un
/// silenzio di due secondi, e la libreria deve crescere mentre arriva la
/// musica, non dopo. Un file preso a metà copia si rilegge da sé: finita la
/// copia arriva un altro avviso, e la data cambiata lo fa rileggere.
pub const ATTESA_MASSIMA: Duration = Duration::from_secs(60);

/// Ogni quanto si sondano le cartelle di rete, se lo si è chiesto.
///
/// Cinque minuti: un disco copiato sul NAS da un altro computer compare in
/// libreria entro una pausa caffè, e il NAS riceve una manciata di domande
/// sulle cartelle ogni tanto invece di un filo che lo interroga di continuo.
pub const SONDAGGIO: Duration = Duration::from_secs(5 * 60);

/// Ogni quanto si rilegge da sé l'elenco delle cartelle e la scelta.
///
/// Un minuto. È la rete di sicurezza per chi cambia le cartelle senza passare
/// dai comandi che avvisano — l'importazione di un profilo, la sincronia — e
/// per ritentare un osservatore che non era partito perché il disco esterno
/// non era attaccato.
pub const CONTROLLO: Duration = Duration::from_secs(60);

/// Quanto può durare al massimo la camminata di un sondaggio.
///
/// Due minuti: un NAS con cinquemila cartelle risponde in qualche decina di
/// secondi su una rete sana. Oltre, si lascia stare il giro — la fotografia di
/// prima resta buona — e se ne riparla al prossimo.
const SONDAGGIO_SCADENZA: Duration = Duration::from_secs(2 * 60);

/// Ogni quanto chi aspetta il turno riprova a prenderlo.
const PASSO_DEL_TURNO: Duration = Duration::from_millis(500);

// ── la scelta ───────────────────────────────────────────────────────────────

/// Cosa si sorveglia.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Modo {
    /// Niente: la libreria cambia solo con «Scansiona» e con i download.
    Spenta,
    /// I dischi di questo computer, con gli avvisi del sistema. Di serie.
    #[default]
    Locali,
    /// Anche le condivisioni di rete, con un sondaggio ogni [`SONDAGGIO`].
    Tutte,
}

impl Modo {
    /// Il nome stabile, quello che viaggia verso la finestra e sta nel
    /// database.
    #[must_use]
    pub const fn come_testo(self) -> &'static str {
        match self {
            Self::Spenta => "spenta",
            Self::Locali => "locali",
            Self::Tutte => "tutte",
        }
    }

    /// Il contrario. Un nome sconosciuto non è una scelta.
    #[must_use]
    pub fn da_testo(testo: &str) -> Option<Self> {
        match testo {
            "spenta" => Some(Self::Spenta),
            "locali" => Some(Self::Locali),
            "tutte" => Some(Self::Tutte),
            _ => None,
        }
    }

    /// La scelta salvata, o quella di serie se non ce n'è una — o se quella
    /// salvata non si riconosce: una riga scritta da una versione futura non
    /// deve spegnere la sorveglianza a nessuno.
    #[must_use]
    pub fn dal_salvato(salvato: Option<&str>) -> Self {
        salvato.and_then(Self::da_testo).unwrap_or_default()
    }
}

/// Come si tiene d'occhio una cartella sorvegliata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Come {
    /// Con gli avvisi del sistema: disco locale.
    Avvisi,
    /// Con il sondaggio periodico: condivisione di rete, e lo si è chiesto.
    Sondaggio,
    /// Non la si guarda: sorveglianza spenta, o rete senza sondaggio.
    Ferma,
    /// La si vorrebbe guardare e non si può: il disco non c'è, o il sistema
    /// ha rifiutato l'osservatore. Si ritenta a ogni [`CONTROLLO`].
    NonRaggiungibile,
}

/// La decisione, per una cartella: pura, perché è la parte che si prova.
///
/// Una cartella di cui non si sa dove stia ([`Collocazione::Ignota`]) è una
/// lettera che in quel momento non risponde: un'unità mappata con il NAS
/// spento, o un disco esterno nel cassetto. Con il sondaggio acceso la si
/// sonda, che è la strada che funziona per tutte e due; senza, resta da
/// riprovare a ogni [`CONTROLLO`] — il disco esterno, riattaccato, torna a
/// essere locale e merita gli avvisi.
#[must_use]
pub const fn come_sorvegliare(modo: Modo, dove: Collocazione) -> Come {
    match (modo, dove) {
        (Modo::Spenta, _) => Come::Ferma,
        (_, Collocazione::Locale) => Come::Avvisi,
        (Modo::Tutte, Collocazione::DiRete | Collocazione::Ignota) => Come::Sondaggio,
        (Modo::Locali, Collocazione::DiRete) => Come::Ferma,
        (Modo::Locali, Collocazione::Ignota) => Come::NonRaggiungibile,
    }
}

// ── l'accorpamento ──────────────────────────────────────────────────────────

/// Gli avvisi arrivati e non ancora scansionati.
///
/// Puro, con l'orologio passato da fuori: le prove gli danno degli istanti
/// inventati invece di aspettare due secondi veri.
#[derive(Debug, Default)]
pub struct Accorpatore {
    /// I percorsi che il sistema ha nominato: file, cartelle, vecchi nomi.
    cambiati: Vec<String>,
    /// Cartelle da riguardare già decise — il sondaggio le trova così.
    cartelle: Vec<String>,
    /// Il primo avviso della raffica in corso.
    primo: Option<Instant>,
    /// L'ultimo.
    ultimo: Option<Instant>,
}

impl Accorpatore {
    /// Percorsi nominati da un avviso.
    pub fn avvisati(&mut self, percorsi: impl IntoIterator<Item = String>, adesso: Instant) {
        let prima = self.cambiati.len();
        self.cambiati.extend(percorsi);
        if self.cambiati.len() > prima {
            self.segna(adesso);
        }
    }

    /// Cartelle da riguardare, già decise.
    pub fn da_riguardare(&mut self, cartelle: impl IntoIterator<Item = String>, adesso: Instant) {
        let prima = self.cartelle.len();
        self.cartelle.extend(cartelle);
        if self.cartelle.len() > prima {
            self.segna(adesso);
        }
    }

    fn segna(&mut self, adesso: Instant) {
        self.primo.get_or_insert(adesso);
        self.ultimo = Some(adesso);
    }

    /// È il momento di scansionare: la raffica tace da [`ACCORPAMENTO`], o
    /// dura da [`ATTESA_MASSIMA`].
    #[must_use]
    pub fn pronto(&self, adesso: Instant) -> bool {
        match (self.primo, self.ultimo) {
            (Some(primo), Some(ultimo)) => {
                adesso.saturating_duration_since(ultimo) >= ACCORPAMENTO
                    || adesso.saturating_duration_since(primo) >= ATTESA_MASSIMA
            }
            _ => false,
        }
    }

    /// Quanto manca a [`Self::pronto`]. `None` se non c'è niente in attesa.
    #[must_use]
    pub fn manca(&self, adesso: Instant) -> Option<Duration> {
        let (primo, ultimo) = (self.primo?, self.ultimo?);
        let per_silenzio = (ultimo + ACCORPAMENTO).saturating_duration_since(adesso);
        let per_tetto = (primo + ATTESA_MASSIMA).saturating_duration_since(adesso);
        Some(per_silenzio.min(per_tetto))
    }

    /// Tutto quel che è in attesa, e l'attesa ricomincia da capo.
    pub fn prendi(&mut self) -> (Vec<String>, Vec<String>) {
        self.primo = None;
        self.ultimo = None;
        (
            std::mem::take(&mut self.cambiati),
            std::mem::take(&mut self.cartelle),
        )
    }

    /// Rimette in attesa quel che non si è potuto scansionare — il turno era
    /// di qualcun altro — e ci si riprova fra [`ACCORPAMENTO`].
    pub fn rimetti(&mut self, cambiati: Vec<String>, cartelle: Vec<String>, adesso: Instant) {
        self.cambiati.extend(cambiati);
        self.cartelle.extend(cartelle);
        if !self.cambiati.is_empty() || !self.cartelle.is_empty() {
            // Da capo tutte e due: con il primo vecchio il tetto sarebbe già
            // superato, e si girerebbe a vuoto contro un turno occupato.
            self.primo = Some(adesso);
            self.ultimo = Some(adesso);
        }
    }
}

/// I file che non sono musica e cambiano spesso accanto alla musica: le
/// copertine, i testi che Rymembra stesso può scrivere accanto ai brani, le
/// liste, i file temporanei di una copia o di un download.
///
/// Un avviso su uno di questi non vale una scansione. Un nome sconosciuto
/// invece sì: è meglio una camminata in più che un album che non compare.
const NON_MUSICA: &[&str] = &[
    "jpg",
    "jpeg",
    "png",
    "gif",
    "bmp",
    "webp",
    "lrc",
    "txt",
    "nfo",
    "cue",
    "log",
    "m3u",
    "m3u8",
    "pls",
    "xspf",
    "ini",
    "db",
    "tmp",
    "part",
    "crdownload",
    "partial",
    "sfv",
    "md5",
    "accurip",
];

/// Un avviso su questo percorso vale una scansione?
#[must_use]
pub fn conta(percorso: &Path) -> bool {
    let Some(estensione) = percorso.extension().and_then(|e| e.to_str()) else {
        return true;
    };
    !NON_MUSICA
        .iter()
        .any(|non| estensione.eq_ignore_ascii_case(non))
}

// ── il sondaggio ────────────────────────────────────────────────────────────

/// Le cartelle sotto una radice, con la loro data di modifica in millisecondi.
pub type Fotografia = BTreeMap<String, i64>;

/// Fotografa le cartelle sotto `radice`: solo cartelle, solo date.
///
/// Senza seguire i collegamenti, come la scansione. Una cartella di cui non si
/// legge la data entra con `-1`, che non coincide con nessuna data vera: se la
/// volta dopo si legge, quella cartella si riguarda.
#[must_use]
pub fn fotografa(radice: &str) -> Fotografia {
    let mut foto = Fotografia::new();
    let cartelle = walkdir_cartelle(radice);
    for (percorso, data) in cartelle {
        foto.insert(percorso, data);
    }
    foto
}

/// La camminata delle sole cartelle.
fn walkdir_cartelle(radice: &str) -> Vec<(String, i64)> {
    let mut fuori = Vec::new();
    let mut pila = vec![PathBuf::from(radice)];
    while let Some(cartella) = pila.pop() {
        let data = std::fs::metadata(&cartella)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|quando| quando.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| i64::try_from(d.as_millis()).ok())
            .unwrap_or(-1);
        fuori.push((cartella.to_string_lossy().into_owned(), data));
        let Ok(voci) = std::fs::read_dir(&cartella) else {
            continue;
        };
        for voce in voci.flatten() {
            // `file_type` della voce e non `metadata`: non segue i
            // collegamenti, e su Windows non costa una domanda in più al disco.
            if voce.file_type().is_ok_and(|tipo| tipo.is_dir()) {
                pila.push(voce.path());
            }
        }
    }
    fuori
}

/// Le cartelle da riguardare fra due fotografie della stessa radice.
///
/// Una cartella che ha cambiato data è una cartella in cui qualcosa è
/// comparso, sparito o stato rinominato; una cartella nuova si riguarda per
/// intero; una sparita si vede da quella che la conteneva, che ha cambiato
/// data anche lei — e per non fidarsi di un server che non lo fa, si aggiunge
/// comunque il genitore.
#[must_use]
pub fn cartelle_cambiate(prima: &Fotografia, dopo: &Fotografia) -> Vec<String> {
    let mut fuori: Vec<String> = dopo
        .iter()
        .filter(|(cartella, data)| prima.get(*cartella) != Some(data))
        .map(|(cartella, _)| cartella.clone())
        .collect();
    for sparita in prima.keys().filter(|c| !dopo.contains_key(*c)) {
        if let Some(genitore) = Path::new(sparita).parent() {
            fuori.push(genitore.to_string_lossy().into_owned());
        }
    }
    fuori
}

// ── lo stato, dalla finestra ────────────────────────────────────────────────

/// Una cartella sorvegliata, come la vede la finestra.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CartellaSorvegliata {
    /// La cartella.
    pub percorso: String,
    /// Come la si tiene d'occhio.
    pub come: Come,
}

/// L'ultima scansione partita da sola.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UltimaPassata {
    /// Quando è finita, in millisecondi dall'epoca.
    pub quando_ms: i64,
    /// Quante cartelle ha guardato.
    pub cartelle: usize,
    /// Brani entrati.
    pub inseriti: usize,
    /// Brani riletti o spostati.
    pub aggiornati: usize,
    /// Brani tolti.
    pub tolti: usize,
    /// Rimozioni che la guardia ha trattenuto: niente è stato tolto, e la
    /// prossima «Scansiona» chiederà.
    pub rinviate: usize,
}

/// Quel che la finestra sa della sorveglianza.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QuadroSorveglianza {
    /// La scelta: `spenta`, `locali`, `tutte`.
    pub modo: String,
    /// Le cartelle e come le si guarda. Vuoto finché il filo non ha
    /// guardato, cioè nei primi secondi dopo l'avvio.
    pub cartelle: Vec<CartellaSorvegliata>,
    /// L'ultima scansione partita da sola, se c'è stata.
    pub ultima: Option<UltimaPassata>,
}

/// Lo stato gestito: dove mandare i messaggi al filo, e il quadro per la
/// finestra.
pub struct StatoSorveglianza {
    /// La posta del filo. `None` prima dell'avvio e dopo lo spegnimento.
    posta: Mutex<Option<Sender<Messaggio>>>,
    /// Quel che la finestra legge. Lo scrive il filo.
    quadro: Mutex<QuadroSorveglianza>,
    /// Dove sta ogni cartella sorvegliata, l'ultima volta che lo si è chiesto.
    ///
    /// Lo legge [`forse_in_rete`], che deve rispondere in microsecondi dal filo
    /// che apre un brano: chiedere al disco, lì, vorrebbe dire aspettare
    /// proprio la share che non risponde. Lo scrivono il risveglio all'avvio e
    /// ogni [`controlla`]. Un lucchetto foglia: nessuno lo tiene mentre ne
    /// chiede un altro.
    dove: Mutex<Vec<(String, Collocazione)>>,
}

impl StatoSorveglianza {
    fn quadro(&self) -> MutexGuard<'_, QuadroSorveglianza> {
        self.quadro.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn dove(&self) -> MutexGuard<'_, Vec<(String, Collocazione)>> {
        self.dove.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn posta(&self) -> MutexGuard<'_, Option<Sender<Messaggio>>> {
        self.posta.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn manda(&self, messaggio: Messaggio) {
        if let Some(posta) = self.posta().as_ref() {
            // Un filo che non c'è più non è un guasto di chi lo avvisa.
            let _ = posta.send(messaggio);
        }
    }
}

/// Quel che arriva al filo.
#[derive(Debug)]
enum Messaggio {
    /// Il sistema ha nominato dei percorsi.
    Avvisi(Vec<String>),
    /// Il sistema ha perso degli avvisi e lo dice: la radice va riguardata
    /// tutta, e l'osservatore resta.
    Riguarda(String),
    /// Un osservatore si è guastato: la radice va riguardata, e
    /// l'osservatore rifatto al prossimo controllo.
    Guasto(String),
    /// Le cartelle o la scelta sono cambiate: si rilegge tutto.
    Riconfigura,
    /// Si esce.
    Fermati,
}

// ── i comandi ───────────────────────────────────────────────────────────────

/// Com'è la sorveglianza adesso.
///
/// Istantaneo: legge un quadro in memoria, non tocca né disco né database.
#[tauri::command]
pub fn sorveglianza_stato(app: AppHandle) -> Esito<QuadroSorveglianza> {
    Ok(app
        .try_state::<StatoSorveglianza>()
        .map(|stato| stato.quadro().clone())
        .unwrap_or_default())
}

/// Cambia cosa si sorveglia, senza riavviare.
///
/// # Errori
///
/// `ipc.payloadInvalid` per un nome che non è una delle tre scelte;
/// `internal.aborted` se la libreria non c'è più; `db.*` se la scrittura
/// fallisce.
#[tauri::command]
pub async fn imposta_sorveglianza(app: AppHandle, modo: String) -> Esito<QuadroSorveglianza> {
    let Some(scelto) = Modo::da_testo(&modo) else {
        return Err(errore(AppError::new(ErrorCode::IpcPayloadInvalid {
            channel: "imposta_sorveglianza".to_owned(),
            detail: Some(format!("«{modo}» non è una delle tre scelte")),
        })));
    };
    crate::disparte::leggi_la_libreria(app.clone(), "sorveglianza delle cartelle", move |l| {
        rymembra_app::settings::write(&l.connection, CHIAVE_SORVEGLIANZA, scelto.come_testo())
    })
    .await
    .map_err(errore)?;
    let Some(stato) = app.try_state::<StatoSorveglianza>() else {
        return Ok(QuadroSorveglianza::default());
    };
    stato.quadro().modo = scelto.come_testo().to_owned();
    stato.manda(Messaggio::Riconfigura);
    Ok(stato.quadro().clone())
}

/// Le cartelle sorvegliate sono cambiate: il filo rilegga l'elenco adesso
/// invece che al prossimo [`CONTROLLO`]. Da chiamare dopo ogni comando che le
/// cambia.
pub fn riconfigura(app: &AppHandle) {
    if let Some(stato) = app.try_state::<StatoSorveglianza>() {
        stato.manda(Messaggio::Riconfigura);
    }
}

/// Ferma il filo: una riga della sequenza di spegnimento.
///
/// Gli osservatori se ne vanno con lui. Ripetibile, come tutta la sequenza: la
/// seconda volta non c'è più nessuno a cui scrivere.
pub fn ferma(app: &AppHandle) {
    if let Some(stato) = app.try_state::<StatoSorveglianza>()
        && let Some(posta) = stato.posta().take()
    {
        let _ = posta.send(Messaggio::Fermati);
    }
}

/// Aspetta il turno delle scansioni invece di rinunciarvi.
///
/// È la coda della scansione dopo i download: fino alla 2.4, se il turno era
/// di qualcun altro, quella scansione si saltava, e i brani appena scaricati
/// aspettavano la «Scansiona» successiva — o un download successivo. Adesso si
/// aspetta che l'altra finisca. `fermati` risponde `true` quando non vale più
/// la pena aspettare (l'uscita), e allora torna `None`.
pub fn aspetta_il_turno<'a>(
    stato: &'a Stato,
    fermati: &dyn Fn() -> bool,
) -> Option<TurnoDiScansione<'a>> {
    loop {
        if fermati() {
            return None;
        }
        if let Some(turno) = stato.turno_di_scansione() {
            return Some(turno);
        }
        std::thread::sleep(PASSO_DEL_TURNO);
    }
}

// ── il filo ─────────────────────────────────────────────────────────────────

/// Registra lo stato e avvia il filo. Da chiamare nel `setup`, dopo la
/// libreria.
pub fn avvia(app: &AppHandle) {
    // La scelta si legge subito, così la finestra che chiede il quadro nei
    // primi secondi vede quella vera e non quella di serie.
    let modo = app
        .try_state::<Stato>()
        .and_then(|stato| {
            con_libreria(&stato, |l| {
                rymembra_app::settings::read(&l.connection, CHIAVE_SORVEGLIANZA)
            })
            .ok()
        })
        .flatten();
    let modo = Modo::dal_salvato(modo.as_deref());
    let (posta, orecchio) = channel();
    app.manage(StatoSorveglianza {
        posta: Mutex::new(Some(posta.clone())),
        quadro: Mutex::new(QuadroSorveglianza {
            modo: modo.come_testo().to_owned(),
            ..QuadroSorveglianza::default()
        }),
        dove: Mutex::new(Vec::new()),
    });
    let mano = app.clone();
    let avviato = std::thread::Builder::new()
        .name("rymembra-sorveglia".to_owned())
        .spawn(move || filo(&mano, &posta, &orecchio));
    if let Err(err) = avviato {
        nota!("[sorveglia] il filo non è partito: {err}");
    }
    let mano = app.clone();
    let avviato = std::thread::Builder::new()
        .name("rymembra-risveglio".to_owned())
        .spawn(move || risveglia(&mano));
    if let Err(err) = avviato {
        nota!("[sorveglia] il risveglio delle cartelle di rete non è partito: {err}");
    }
}

/// Quanto si concede al risveglio di una cartella di rete, all'avvio.
///
/// Mezzo minuto: un NAS addormentato fa girare i dischi in otto, quindici
/// secondi, e Windows ricollega una lettera mappata quando qualcuno ci bussa —
/// non prima. Nessuno aspetta questa risposta: serve a bussare.
const RISVEGLIO_SCADENZA: Duration = Duration::from_secs(30);

/// Bussa alle cartelle di rete appena Rymembra si apre, perché siano sveglie
/// quando qualcuno preme play.
///
/// # Perché
///
/// Windows ricollega le lettere mappate **in modo pigro**: all'accesso, non al
/// login. E un NAS che dorme fa girare i dischi solo quando gli si chiede un
/// file. Il primo a chiederlo era il primo play: cinque secondi di scadenza
/// contro un minuto di riconnessione, e la finestra diceva «la cartella di rete
/// non risponde» a chi aveva appena acceso il computer — tre volte, una per
/// ogni clic. Bussare all'avvio fa partire il risveglio mentre la finestra si
/// sta ancora disegnando.
///
/// Riempie anche la mappa di [`StatoSorveglianza::dove`]: subito con «non si
/// sa» per tutte, così [`forse_in_rete`] risponde «forse» invece di «no» nei
/// primi secondi; poi con la risposta vera.
///
/// Solo il diario: niente avvisi, niente scritture. Una cartella che non si
/// sveglia lo dirà chi la usa, quando la usa.
fn risveglia(app: &AppHandle) {
    let Some(stato) = app.try_state::<Stato>() else {
        return;
    };
    let radici = con_libreria(&stato, |l| {
        rymembra_app::settings::read_json::<Vec<String>>(
            &l.connection,
            rymembra_app::settings::CHIAVE_CARTELLE,
        )
    });
    let radici = match radici {
        Ok(Some(radici)) if !radici.is_empty() => radici,
        _ => return,
    };
    for radice in &radici {
        ricorda_dove(app, radice, Collocazione::Ignota);
    }
    let dove =
        rymembra_app::volumi::collocazioni(&radici, rymembra_app::volumi::SCADENZA_RISOLUZIONE);
    for (radice, dove) in radici.iter().zip(&dove) {
        ricorda_dove(app, radice, *dove);
    }
    // Anche le UNC: `collocazioni` le riconosce dalla forma senza toccarle, e
    // da sole non si sveglierebbero.
    let da_svegliare: Vec<String> = radici
        .iter()
        .zip(&dove)
        .filter(|(_, dove)| **dove != Collocazione::Locale)
        .map(|(radice, _)| radice.clone())
        .collect();
    if da_svegliare.is_empty() {
        return;
    }
    let inizio = Instant::now();
    let risposte = rymembra_app::volumi::esistenze(da_svegliare.clone(), RISVEGLIO_SCADENZA);
    let sveglie = risposte
        .iter()
        .filter(|risposta| **risposta == Esistenza::Presente)
        .count();
    nota!(
        "[sorveglia] risveglio delle cartelle di rete: {sveglie} su {} rispondono, in {} ms",
        da_svegliare.len(),
        inizio.elapsed().as_millis()
    );
}

/// Ricorda dove sta una cartella sorvegliata. Vedi [`StatoSorveglianza::dove`].
fn ricorda_dove(app: &AppHandle, radice: &str, dove: Collocazione) {
    let Some(stato) = app.try_state::<StatoSorveglianza>() else {
        return;
    };
    let mut elenco = stato.dove();
    match elenco.iter_mut().find(|(gia, _)| gia == radice) {
        Some(voce) => voce.1 = dove,
        None => elenco.push((radice.to_owned(), dove)),
    }
}

/// Questo percorso potrebbe stare su una cartella di rete?
///
/// La risposta che serve a chi apre un brano per decidere quanto aspettarlo:
/// un disco locale risponde in microsecondi, un NAS che si sveglia in quindici
/// secondi, e cinque erano pochi per il secondo. Dalla forma quando basta — un
/// UNC è di rete — e altrimenti dalla cartella sorvegliata più lunga che lo
/// contiene, com'era l'ultima volta che lo si è chiesto al disco. «Non si sa»
/// vale «forse»: è il caso della lettera mappata che non risponde ancora, cioè
/// proprio quello da aspettare di più.
///
/// Non tocca il disco, mai: la chiama un filo che sta per aprire un file, e un
/// controllo che si bloccasse sulla share sarebbe la stessa attesa che si sta
/// cercando di misurare.
pub fn forse_in_rete(app: &AppHandle, percorso: &str) -> bool {
    if rymembra_app::volumi::dalla_forma(Path::new(percorso)).is_some() {
        return true;
    }
    let Some(stato) = app.try_state::<StatoSorveglianza>() else {
        return false;
    };
    let regole = PathRules::for_current_platform();
    let elenco = stato.dove();
    elenco
        .iter()
        .filter(|(radice, _)| is_under(percorso, radice, regole))
        .max_by_key(|(radice, _)| radice.len())
        .is_some_and(|(_, dove)| *dove != Collocazione::Locale)
}

/// Quanto aspettare dopo l'avvio prima di guardare le cartelle.
///
/// Dieci secondi: l'avvio è il momento in cui la finestra, la riproduzione e
/// il database vogliono tutti il disco, e risolvere le lettere mappate di una
/// libreria su un NAS può chiederne qualcuno. Una sorveglianza che arriva
/// dieci secondi dopo non perde niente che una scansione non trovi.
const ATTESA_AVVIO: Duration = Duration::from_secs(10);

/// Una cartella sorvegliata, dal lato del filo.
struct Sorvegliata {
    /// Come la si guarda.
    come: Come,
    /// L'osservatore, quando la si guarda con gli avvisi. Cadendo smette.
    osservatore: Option<notify::RecommendedWatcher>,
    /// L'ultima fotografia, quando la si sonda.
    foto: Option<Fotografia>,
    /// Quando sondarla la prossima volta.
    prossimo_sondaggio: Instant,
    /// La si sonda perché gli avvisi non sono partiti, non perché sia di rete.
    ///
    /// Si ritenta l'osservatore a ogni [`CONTROLLO`]: senza, una cartella che
    /// all'avvio non rispondeva — un disco che girava ancora a vuoto — restava
    /// a sondaggio per tutta la sessione anche dopo essere tornata normale.
    ripiego: bool,
    /// Aveva un osservatore, e lo si è perso.
    ///
    /// Quando si riarma, la cartella si riguarda intera: nel frattempo nessuno
    /// guardava, e quel che è cambiato non ha mandato avvisi a nessuno.
    persa: bool,
}

/// Quanto si aspetta la risposta a «la cartella c'è ancora?» per una cartella
/// con gli avvisi, a ogni [`CONTROLLO`].
///
/// Due secondi: sono dischi di questo computer, e rispondono in microsecondi
/// quando ci sono.
const VIVA_SCADENZA: Duration = Duration::from_secs(2);

/// Lo stato del filo fra un giro e l'altro.
struct Motore {
    /// La scelta in vigore.
    modo: Modo,
    /// Le cartelle, con la grafia con cui stanno nell'elenco.
    cartelle: BTreeMap<String, Sorvegliata>,
    /// Gli avvisi in attesa.
    attesa: Accorpatore,
    /// Il prossimo controllo dell'elenco.
    prossimo_controllo: Instant,
}

fn filo(app: &AppHandle, posta: &Sender<Messaggio>, orecchio: &Receiver<Messaggio>) {
    match orecchio.recv_timeout(ATTESA_AVVIO) {
        Ok(Messaggio::Fermati) | Err(RecvTimeoutError::Disconnected) => return,
        // Un avviso arrivato prima della configurazione non può esserci: non
        // c'è ancora nessun osservatore. Una riconfigurazione invece sì, e la
        // si fa adesso comunque.
        Ok(_) | Err(RecvTimeoutError::Timeout) => {}
    }
    let mut motore = Motore {
        modo: Modo::Locali,
        cartelle: BTreeMap::new(),
        attesa: Accorpatore::default(),
        prossimo_controllo: Instant::now(),
    };
    let mut cadute_di_fila = 0_u32;
    loop {
        if crate::spegnimento::in_uscita() {
            return;
        }
        let adesso = Instant::now();
        let attesa = prossima_attesa(&motore, adesso);
        let messaggio = match orecchio.recv_timeout(attesa) {
            Ok(Messaggio::Fermati) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(messaggio) => Some(messaggio),
            Err(RecvTimeoutError::Timeout) => None,
        };
        // Ogni giro dentro `sorvegliato`: un panico — un percorso che il
        // sistema consegna storto, un parser sotto la scansione — costa un
        // giro, non la sorveglianza per il resto della sessione.
        let giro =
            crate::stato::sorvegliato("sorveglia", || giro(app, posta, &mut motore, messaggio));
        if giro.is_some() {
            cadute_di_fila = 0;
        } else {
            cadute_di_fila = cadute_di_fila.saturating_add(1);
            let pausa = crate::stato::attesa_dopo_la_caduta(cadute_di_fila).min(CONTROLLO);
            if matches!(orecchio.recv_timeout(pausa), Ok(Messaggio::Fermati)) {
                return;
            }
        }
    }
}

/// Fino a quando dormire: il primo fra gli avvisi pronti, il prossimo
/// sondaggio e il prossimo controllo.
fn prossima_attesa(motore: &Motore, adesso: Instant) -> Duration {
    let mut attesa = motore.prossimo_controllo.saturating_duration_since(adesso);
    if let Some(manca) = motore.attesa.manca(adesso) {
        attesa = attesa.min(manca);
    }
    for sorvegliata in motore.cartelle.values() {
        if sorvegliata.come == Come::Sondaggio {
            attesa = attesa.min(
                sorvegliata
                    .prossimo_sondaggio
                    .saturating_duration_since(adesso),
            );
        }
    }
    // Mai un'attesa nulla in un ciclo: un decimo di secondo è il passo minimo.
    attesa.max(Duration::from_millis(100))
}

/// Un giro: il messaggio arrivato, e poi quel che è scaduto.
fn giro(
    app: &AppHandle,
    posta: &Sender<Messaggio>,
    motore: &mut Motore,
    messaggio: Option<Messaggio>,
) {
    let adesso = Instant::now();
    match messaggio {
        Some(Messaggio::Avvisi(percorsi)) => motore.attesa.avvisati(percorsi, adesso),
        Some(Messaggio::Riguarda(radice)) => motore.attesa.da_riguardare([radice], adesso),
        Some(Messaggio::Guasto(radice)) => {
            // L'osservatore se ne va subito, e si rifà al prossimo controllo e
            // non adesso: un osservatore che si guasta di continuo — una radice
            // che il sistema rifiuta — costa così una scansione della radice al
            // minuto, invece di un giro senza fine fra guasto e riavvio.
            nota!("[sorveglia] l'osservatore di {radice} si è guastato: la si riguarda");
            if let Some(sorvegliata) = motore.cartelle.get_mut(&radice) {
                sorvegliata.osservatore = None;
                sorvegliata.come = Come::NonRaggiungibile;
                // La si riguarda qui sotto, adesso: quando si riarma non serve
                // un secondo giro.
                sorvegliata.persa = false;
            }
            motore.attesa.da_riguardare([radice], adesso);
        }
        Some(Messaggio::Riconfigura) => motore.prossimo_controllo = adesso,
        Some(Messaggio::Fermati) | None => {}
    }
    if adesso >= motore.prossimo_controllo {
        controlla(app, posta, motore);
        motore.prossimo_controllo = Instant::now() + CONTROLLO;
    }
    sonda(app, motore);
    if motore.attesa.pronto(Instant::now()) {
        riscansiona(app, motore);
    }
}

/// Rilegge cartelle e scelta, e rimette in riga gli osservatori.
fn controlla(app: &AppHandle, posta: &Sender<Messaggio>, motore: &mut Motore) {
    let Some(stato) = app.try_state::<Stato>() else {
        return;
    };
    let letto = con_libreria(&stato, |l| {
        let radici: Vec<String> = rymembra_app::settings::read_json(
            &l.connection,
            rymembra_app::settings::CHIAVE_CARTELLE,
        )?
        .unwrap_or_default();
        let modo = rymembra_app::settings::read(&l.connection, CHIAVE_SORVEGLIANZA)?;
        Ok((radici, Modo::dal_salvato(modo.as_deref())))
    });
    let (radici, modo) = match letto {
        Ok(letto) => letto,
        Err(guasto) => {
            nota!("[sorveglia] cartelle non lette: {guasto}");
            return;
        }
    };
    let modo_cambiato = modo != motore.modo;
    motore.modo = modo;
    // Via quel che non è più nell'elenco: l'osservatore cade con la voce.
    motore
        .cartelle
        .retain(|cartella, _| radici.contains(cartella));
    if let Some(stato) = app.try_state::<StatoSorveglianza>() {
        stato.dove().retain(|(radice, _)| radici.contains(radice));
    }
    lascia_cadere_le_morte(motore);
    let mut da_riguardare: Vec<String> = Vec::new();
    for radice in &radici {
        let (rifare, persa) = match motore.cartelle.get(radice) {
            None => (true, false),
            Some(sorvegliata) => (
                modo_cambiato || sorvegliata.come == Come::NonRaggiungibile || sorvegliata.ripiego,
                sorvegliata.persa,
            ),
        };
        if !rifare {
            continue;
        }
        // Dove sta, chiesto fuori da ogni lucchetto e con la scadenza: su una
        // lettera mappata che non risponde è un'attesa.
        let dove =
            rymembra_app::volumi::collocazione(radice, rymembra_app::volumi::SCADENZA_RISOLUZIONE);
        ricorda_dove(app, radice, dove);
        let voluto = come_sorvegliare(modo, dove);
        let sorvegliata = match voluto {
            Come::Avvisi => match osserva_entro(radice, posta) {
                Ok(osservatore) => {
                    if persa {
                        da_riguardare.push(radice.clone());
                    }
                    Sorvegliata {
                        come: Come::Avvisi,
                        osservatore: Some(osservatore),
                        foto: None,
                        prossimo_sondaggio: Instant::now(),
                        ripiego: false,
                        persa: false,
                    }
                }
                Err(guasto) => {
                    nota!("[sorveglia] {radice} non si può osservare: {guasto}");
                    // Con il sondaggio acceso, una cartella che non accetta gli
                    // avvisi si sonda: è la strada che funziona anche dove gli
                    // avvisi no. Senza, si riprova al prossimo controllo.
                    let ripiego = modo == Modo::Tutte;
                    // Il primo sondaggio fa solo la fotografia di riferimento:
                    // quel che è cambiato mentre nessuno guardava va riguardato
                    // a parte.
                    let persa = if ripiego && persa {
                        da_riguardare.push(radice.clone());
                        false
                    } else {
                        persa
                    };
                    Sorvegliata {
                        come: if ripiego {
                            Come::Sondaggio
                        } else {
                            Come::NonRaggiungibile
                        },
                        osservatore: None,
                        foto: None,
                        prossimo_sondaggio: Instant::now(),
                        ripiego,
                        persa,
                    }
                }
            },
            altro => Sorvegliata {
                come: altro,
                osservatore: None,
                // La prima fotografia la fa il primo sondaggio, subito: è il
                // punto di riferimento, non un cambiamento.
                foto: None,
                prossimo_sondaggio: Instant::now(),
                ripiego: false,
                persa: false,
            },
        };
        motore.cartelle.insert(radice.clone(), sorvegliata);
    }
    if !da_riguardare.is_empty() {
        nota!(
            "[sorveglia] riarmate dopo averle perse, si riguardano: {}",
            da_riguardare.join(", ")
        );
        motore.attesa.da_riguardare(da_riguardare, Instant::now());
    }
    if let Some(stato) = app.try_state::<StatoSorveglianza>() {
        let mut quadro = stato.quadro();
        quadro.modo = modo.come_testo().to_owned();
        quadro.cartelle = radici
            .iter()
            .map(|radice| CartellaSorvegliata {
                percorso: radice.clone(),
                come: motore
                    .cartelle
                    .get(radice)
                    .map_or(Come::Ferma, |sorvegliata| sorvegliata.come),
            })
            .collect();
    }
}

/// Le cartelle con gli avvisi che non ci sono più.
///
/// # Perché bisogna andarle a cercare
///
/// Perché il loro osservatore muore **in silenzio**. In `notify` 8.2 su Windows
/// un completamento fallito di `ReadDirectoryChangesW` — il disco USB staccato,
/// la lettera che sparisce — toglie la sorveglianza e lo scrive in un registro
/// che Rymembra non ascolta: il gestore qui sotto non riceve nessun errore, il
/// ramo `Guasto` non scatta, e la cartella resta «con gli avvisi» per tutta la
/// sessione senza che nessun avviso arrivi più. Riattaccato il disco, quel che
/// ci si copiava sopra non entrava in libreria.
///
/// Una domanda a ogni [`CONTROLLO`] — «c'è ancora?» — con [`VIVA_SCADENZA`].
/// Solo un «non c'è» detto dal sistema fa cadere l'osservatore: un «non si sa»
/// (un disco che si sta svegliando) lo lascia stare, perché farlo cadere e
/// rifarlo costerebbe una riscansione della cartella per un disco lento.
/// Quando la cartella torna, [`controlla`] la riarma e la riguarda intera.
fn lascia_cadere_le_morte(motore: &mut Motore) {
    let con_avvisi: Vec<String> = motore
        .cartelle
        .iter()
        .filter(|(_, sorvegliata)| sorvegliata.come == Come::Avvisi)
        .map(|(radice, _)| radice.clone())
        .collect();
    if con_avvisi.is_empty() {
        return;
    }
    let risposte = rymembra_app::volumi::esistenze(con_avvisi.clone(), VIVA_SCADENZA);
    for (radice, risposta) in con_avvisi.iter().zip(risposte) {
        if risposta != Esistenza::Assente {
            continue;
        }
        if let Some(sorvegliata) = motore.cartelle.get_mut(radice) {
            nota!("[sorveglia] {radice} non c'è più: la si riarma quando torna");
            sorvegliata.osservatore = None;
            sorvegliata.come = Come::NonRaggiungibile;
            sorvegliata.persa = true;
        }
    }
}

/// [`osserva`], con una scadenza.
///
/// `notify` chiede al disco se la cartella è un file o una cartella prima di
/// osservarla, e lo chiede senza scadenza: su una lettera mappata che Windows
/// sta ricollegando la risposta arrivava dopo un minuto — e per quel minuto il
/// filo della sorveglianza non faceva nient'altro, una volta al minuto finché il
/// NAS restava giù. Scaduto il tempo, la risposta che arriva dopo trova il
/// canale chiuso: l'osservatore nato tardi cade col filo che l'aspettava, e il
/// suo `Drop` chiude la sorveglianza. Niente resta appeso.
fn osserva_entro(
    radice: &str,
    posta: &Sender<Messaggio>,
) -> Result<notify::RecommendedWatcher, String> {
    let percorso = radice.to_owned();
    let posta = posta.clone();
    rymembra_app::scadenza::con_scadenza(
        "osserva",
        rymembra_app::volumi::SCADENZA_RISOLUZIONE,
        move || osserva(&percorso, &posta).map_err(|guasto| guasto.to_string()),
    )
    .unwrap_or_else(|| {
        Err(format!(
            "la cartella non ha risposto entro {} secondi",
            rymembra_app::volumi::SCADENZA_RISOLUZIONE.as_secs()
        ))
    })
}

/// Mette un osservatore ricorsivo su una cartella, che scrive al filo.
fn osserva(radice: &str, posta: &Sender<Messaggio>) -> notify::Result<notify::RecommendedWatcher> {
    let posta = posta.clone();
    let nome = radice.to_owned();
    let mut osservatore =
        notify::recommended_watcher(move |esito: notify::Result<notify::Event>| {
            let messaggio = match esito {
                // Il sistema ha perso degli avvisi — il suo buffer si è
                // riempito durante una copia enorme — e lo dice: non si sa più
                // cosa è cambiato, quindi si riguarda la radice intera.
                // L'osservatore funziona, e resta: quel che arriva dopo lo
                // vede.
                Ok(evento) if evento.need_rescan() => Messaggio::Riguarda(nome.clone()),
                Ok(evento) => {
                    if matches!(evento.kind, notify::EventKind::Access(_)) {
                        return;
                    }
                    let percorsi: Vec<String> = evento
                        .paths
                        .iter()
                        .filter(|p| conta(p))
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect();
                    if percorsi.is_empty() {
                        return;
                    }
                    Messaggio::Avvisi(percorsi)
                }
                Err(_) => Messaggio::Guasto(nome.clone()),
            };
            let _ = posta.send(messaggio);
        })?;
    osservatore.watch(Path::new(radice), notify::RecursiveMode::Recursive)?;
    Ok(osservatore)
}

/// I sondaggi scaduti, uno per cartella di rete, mai durante una scansione.
fn sonda(app: &AppHandle, motore: &mut Motore) {
    let Some(stato) = app.try_state::<Stato>() else {
        return;
    };
    let adesso = Instant::now();
    let mut trovate: Vec<String> = Vec::new();
    for (radice, sorvegliata) in &mut motore.cartelle {
        if sorvegliata.come != Come::Sondaggio || adesso < sorvegliata.prossimo_sondaggio {
            continue;
        }
        // Una scansione in corso guarda già il disco, e un sondaggio in mezzo
        // sarebbe una seconda camminata sulla stessa share: si riprova fra un
        // minuto.
        if stato.scansione_in_corso() || crate::spegnimento::in_uscita() {
            sorvegliata.prossimo_sondaggio = adesso + CONTROLLO;
            continue;
        }
        sorvegliata.prossimo_sondaggio = adesso + SONDAGGIO;
        let percorso = radice.clone();
        // Con la scadenza: una share che smette di rispondere a metà non deve
        // tenere fermo il filo. Scaduta, la fotografia di prima resta buona.
        let Some(foto) =
            rymembra_app::scadenza::con_scadenza("sondaggio", SONDAGGIO_SCADENZA, move || {
                rymembra_app::files::LocalFiles
                    .radice_raggiungibile(&percorso)
                    .then(|| fotografa(&percorso))
            })
            .flatten()
        else {
            continue;
        };
        if let Some(prima) = sorvegliata.foto.as_ref() {
            trovate.extend(cartelle_cambiate(prima, &foto));
        }
        sorvegliata.foto = Some(foto);
    }
    if !trovate.is_empty() {
        motore.attesa.da_riguardare(trovate, Instant::now());
    }
}

/// La scansione delle cartelle in attesa, se il turno è libero.
fn riscansiona(app: &AppHandle, motore: &mut Motore) {
    let Some(stato) = app.try_state::<Stato>() else {
        return;
    };
    let (cambiati, cartelle) = motore.attesa.prendi();
    let regole = PathRules::for_current_platform();
    let radici: Vec<String> = motore
        .cartelle
        .iter()
        .filter(|(_, s)| {
            matches!(
                s.come,
                Come::Avvisi | Come::Sondaggio | Come::NonRaggiungibile
            )
        })
        .map(|(radice, _)| radice.clone())
        .collect();
    // Su un disco locale chiedere se una cartella esiste non costa niente; i
    // percorsi del sondaggio sono già cartelle che esistevano un attimo fa.
    let mut da_guardare =
        cartelle_da_riguardare(&cambiati, &radici, regole, &|c| Path::new(c).exists());
    da_guardare.extend(
        cartelle
            .into_iter()
            .filter(|c| radici.iter().any(|r| is_under(c, r, regole))),
    );
    let da_guardare = piu_esterne(da_guardare, regole);
    if da_guardare.is_empty() {
        return;
    }
    let Some(turno) = stato.turno_di_scansione() else {
        // Il turno è di qualcun altro: si rimette tutto in attesa, e si
        // riprova fra poco. Le cartelle e non i percorsi, ormai sono decise.
        motore
            .attesa
            .rimetti(Vec::new(), da_guardare, Instant::now());
        return;
    };
    if crate::spegnimento::in_uscita() {
        return;
    }
    stato.riprendi_scansioni();
    esegui(app, &stato, &da_guardare, turno);
}

/// La scansione vera, con il turno già in mano.
fn esegui(app: &AppHandle, stato: &Stato, cartelle: &[String], _turno: TurnoDiScansione<'_>) {
    let copertine = match stato.copertine() {
        Ok(copertine) => copertine,
        Err(guasto) => {
            nota!("[sorveglia] la scansione non parte: {guasto}");
            return;
        }
    };
    let fermati = || stato.scansione_fermata() || crate::spegnimento::in_uscita();
    let scan = Scan {
        files: std::sync::Arc::new(rymembra_app::files::LocalFiles),
        covers: copertine,
        roots: cartelle,
        rules: PathRules::for_current_platform(),
        // Nessuno la sta guardando: vedi il preambolo del modulo.
        prudente: true,
        scadenze: Scadenze::default(),
        fermati: Some(&fermati),
    };
    let mut deposito = DepositoStato::nuovo(stato);
    let report = match scan.run_su(&mut deposito, |_, _| std::ops::ControlFlow::Continue(())) {
        Ok(report) => report,
        Err(guasto) => {
            nota!(
                "[sorveglia] la scansione di {} cartelle è fallita: {guasto}",
                cartelle.len()
            );
            return;
        }
    };
    if !report.radici_saltate.is_empty() {
        nota!(
            "[sorveglia] {} cartelle non hanno risposto, i loro brani non sono stati giudicati",
            report.radici_saltate.len()
        );
    }
    if report.rimozioni_rinviate > 0 {
        nota!(
            "[sorveglia] {} rimozioni trattenute dalla guardia, niente è stato tolto",
            report.rimozioni_rinviate
        );
    }
    let aggiornati = report.updated.saturating_add(report.moved);
    if let Some(sorveglianza) = app.try_state::<StatoSorveglianza>() {
        sorveglianza.quadro().ultima = Some(UltimaPassata {
            quando_ms: crate::stato::adesso_ms(),
            cartelle: cartelle.len(),
            inseriti: report.inserted,
            aggiornati,
            tolti: report.removed,
            rinviate: report.rimozioni_rinviate,
        });
    }
    let cambiato = report.inserted > 0 || aggiornati > 0 || report.removed > 0;
    if !cambiato {
        return;
    }
    // Quel che fa la coda dei download quando i brani sono entrati: la
    // finestra rilegge le viste, l'albero delle cartelle si butta, e i brani
    // nuovi vanno arricchiti, misurati e salvati. L'evento è quello dei
    // download perché la finestra lo ascolta già e fa esattamente questo.
    app.emetti("scarico:in_libreria", report.inserted);
    crate::cartelle::cambiate(app);
    crate::arricchimento::sporca(app);
    crate::analisi::sporca(app);
    let _ = crate::nuvola::se_riuscito(app, Ok::<(), ()>(()));
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn molti_avvisi_fanno_una_scansione_sola() {
        let zero = Instant::now();
        let mut attesa = Accorpatore::default();
        assert_eq!(attesa.manca(zero), None);
        assert_eq!(attesa.manca(zero), None);
        // Una copia: venti file, uno ogni cento millisecondi.
        for n in 0..20_u64 {
            let quando = zero + Duration::from_millis(100 * n);
            attesa.avvisati([format!(r"C:\M\A\{n:02}.flac")], quando);
            assert!(
                !attesa.pronto(quando),
                "durante la raffica non si scansiona"
            );
        }
        let ultimo = zero + Duration::from_millis(1_900);
        assert!(!attesa.pronto(ultimo + Duration::from_millis(1_999)));
        assert!(
            attesa.pronto(ultimo + ACCORPAMENTO),
            "il silenzio è arrivato"
        );
        let (cambiati, cartelle) = attesa.prendi();
        assert_eq!(cambiati.len(), 20, "tutti in una volta sola");
        assert!(cartelle.is_empty());
        assert_eq!(attesa.manca(ultimo), None, "e l'attesa ricomincia da vuota");
        assert!(!attesa.pronto(ultimo + ACCORPAMENTO * 10));
    }

    #[test]
    fn una_raffica_senza_fine_non_rimanda_per_sempre() {
        let zero = Instant::now();
        let mut attesa = Accorpatore::default();
        let mut quando = zero;
        while quando < zero + ATTESA_MASSIMA {
            attesa.avvisati(["x".to_owned()], quando);
            assert!(!attesa.pronto(quando));
            quando += Duration::from_millis(500);
        }
        assert!(
            attesa.pronto(zero + ATTESA_MASSIMA),
            "al tetto si scansiona"
        );
    }

    #[test]
    fn con_il_turno_occupato_si_riprova_fra_poco() {
        let zero = Instant::now();
        let mut attesa = Accorpatore::default();
        attesa.avvisati(["a".to_owned()], zero);
        let dopo = zero + ATTESA_MASSIMA + ACCORPAMENTO;
        assert!(attesa.pronto(dopo));
        let (cambiati, cartelle) = attesa.prendi();
        attesa.rimetti(cambiati, cartelle, dopo);
        assert!(
            !attesa.pronto(dopo),
            "non si gira a vuoto sul turno occupato"
        );
        assert_eq!(attesa.manca(dopo), Some(ACCORPAMENTO));
        assert!(attesa.pronto(dopo + ACCORPAMENTO));
        // Rimettere niente non accende nessuna attesa.
        let mut vuota = Accorpatore::default();
        vuota.rimetti(Vec::new(), Vec::new(), dopo);
        assert_eq!(vuota.manca(dopo), None);
    }

    #[test]
    fn le_copertine_e_i_testi_non_valgono_una_scansione() {
        assert!(conta(Path::new(r"C:\M\A\01.flac")));
        assert!(conta(Path::new(r"C:\M\A")), "una cartella sì");
        assert!(
            conta(Path::new(r"C:\M\Vol. 2")),
            "anche con un punto nel nome"
        );
        assert!(!conta(Path::new(r"C:\M\A\cover.JPG")));
        assert!(!conta(Path::new(r"C:\M\A\01.lrc")));
        assert!(!conta(Path::new(r"C:\M\A\01.flac.part")));
    }

    #[test]
    fn l_interruttore_decide_chi_si_guarda() {
        use Collocazione::{DiRete, Ignota, Locale};
        for dove in [Locale, DiRete, Ignota] {
            assert_eq!(come_sorvegliare(Modo::Spenta, dove), Come::Ferma);
        }
        assert_eq!(come_sorvegliare(Modo::Locali, Locale), Come::Avvisi);
        assert_eq!(come_sorvegliare(Modo::Locali, DiRete), Come::Ferma);
        assert_eq!(
            come_sorvegliare(Modo::Locali, Ignota),
            Come::NonRaggiungibile,
            "un disco che non risponde si riprova"
        );
        assert_eq!(come_sorvegliare(Modo::Tutte, Locale), Come::Avvisi);
        assert_eq!(come_sorvegliare(Modo::Tutte, DiRete), Come::Sondaggio);
        assert_eq!(come_sorvegliare(Modo::Tutte, Ignota), Come::Sondaggio);
        // Di serie, i dischi locali; un nome sconosciuto non spegne niente.
        assert_eq!(Modo::dal_salvato(None), Modo::Locali);
        assert_eq!(Modo::dal_salvato(Some("domani")), Modo::Locali);
        for modo in [Modo::Spenta, Modo::Locali, Modo::Tutte] {
            assert_eq!(Modo::da_testo(modo.come_testo()), Some(modo));
        }
    }

    #[test]
    fn un_osservatore_vero_avvisa_e_spento_tace() {
        // Il sistema vero, su una cartella temporanea: l'unica prova che
        // aspetta il tempo reale, e al più un paio di secondi.
        let cartella = tempfile::tempdir().expect("cartella");
        let radice = cartella.path().to_string_lossy().into_owned();
        let (posta, orecchio) = channel();
        let osservatore = osserva(&radice, &posta).expect("osservatore");
        std::fs::create_dir(cartella.path().join("Album")).expect("cartella nuova");
        std::fs::write(cartella.path().join("Album").join("01.flac"), b"x").expect("file");
        let mut visti: Vec<String> = Vec::new();
        let fine = Instant::now() + Duration::from_secs(3);
        while Instant::now() < fine && !visti.iter().any(|p| p.ends_with("01.flac")) {
            if let Ok(Messaggio::Avvisi(percorsi)) =
                orecchio.recv_timeout(Duration::from_millis(200))
            {
                visti.extend(percorsi);
            }
        }
        assert!(
            visti.iter().any(|p| p.ends_with("01.flac")),
            "il file nuovo non è stato annunciato: {visti:?}"
        );
        // Spegnere è lasciar cadere l'osservatore: da lì in poi niente.
        drop(osservatore);
        while orecchio.recv_timeout(Duration::from_millis(100)).is_ok() {}
        std::fs::write(cartella.path().join("Album").join("02.flac"), b"y").expect("file");
        assert!(
            orecchio.recv_timeout(Duration::from_millis(500)).is_err(),
            "un osservatore spento ha parlato"
        );
    }

    #[test]
    fn il_sondaggio_vede_le_cartelle_cambiate() {
        let cartella = tempfile::tempdir().expect("cartella");
        let radice = cartella.path().to_string_lossy().into_owned();
        std::fs::create_dir(cartella.path().join("Ferma")).expect("cartella");
        std::fs::create_dir(cartella.path().join("Viva")).expect("cartella");
        let prima = fotografa(&radice);
        assert_eq!(prima.len(), 3, "la radice e le due cartelle");
        // Niente cambiato, niente da riguardare.
        assert!(cartelle_cambiate(&prima, &fotografa(&radice)).is_empty());
        // Un file nuovo in «Viva» cambia la data di «Viva», non di «Ferma».
        // Le date si confrontano finte, per non dipendere dalla grana
        // dell'orologio del filesystem.
        let mut dopo = prima.clone();
        let viva = cartella.path().join("Viva").to_string_lossy().into_owned();
        if let Some(data) = dopo.get_mut(&viva) {
            *data += 1;
        }
        assert_eq!(cartelle_cambiate(&prima, &dopo), vec![viva.clone()]);
        // Una cartella sparita fa riguardare chi la conteneva.
        dopo.remove(&viva);
        assert_eq!(cartelle_cambiate(&prima, &dopo), vec![radice.clone()]);
    }

    #[test]
    fn la_coda_dei_download_aspetta_il_turno_invece_di_saltarlo() {
        let dati = tempfile::tempdir().expect("cartella dati di prova");
        let stato = Stato::apri(dati.path().to_path_buf());
        let preso = stato.turno_di_scansione().expect("il primo turno");
        std::thread::scope(|filo| {
            let attesa = filo.spawn(|| {
                let inizio = Instant::now();
                let turno = aspetta_il_turno(&stato, &|| false);
                (turno.is_some(), inizio.elapsed())
            });
            std::thread::sleep(Duration::from_millis(700));
            drop(preso);
            let (preso_dopo, quanto) = attesa.join().expect("il filo che aspetta");
            assert!(preso_dopo, "il turno doveva arrivare");
            assert!(
                quanto >= Duration::from_millis(600),
                "doveva aspettare la fine dell'altra: {quanto:?}"
            );
        });
        // E chi deve smettere di aspettare smette.
        let _ancora = stato.turno_di_scansione().expect("di nuovo libero");
        assert!(aspetta_il_turno(&stato, &|| true).is_none());
    }
}
