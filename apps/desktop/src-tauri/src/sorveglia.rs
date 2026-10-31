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
//! # Gli avvisi
//!
//! Su ogni cartella sorvegliata il sistema avvisa da sé: `notify` con
//! `ReadDirectoryChangesW`, un osservatore ricorsivo per cartella. Gli avvisi
//! arrivano a raffiche — copiare un album sono venti file, e per ognuno due o
//! tre eventi — e si **accorpano**: si aspetta che la raffica taccia per
//! [`ACCORPAMENTO`], e poi si riscansionano soltanto le cartelle in cui è
//! successo qualcosa (`rymembra_app::library::cartelle_da_riguardare`). Un album
//! copiato è una scansione di un album, non della libreria.
//!
//! Dalla 3.0.2 anche sulle **condivisioni di rete**, e di serie. Prima lì c'era
//! al più un sondaggio delle date delle cartelle, spento di serie: con la
//! libreria tutta sul NAS, un CD copiato non compariva finché qualcuno non
//! premeva «Scansiona». E gli avvisi SMB arrivano davvero: provato sul NAS
//! dell'utente con lo stesso `ReadDirectoryChangesW`, una cartella creata, un
//! file copiato e una rinomina arrivano in 10-70 ms.
//!
//! # Il riallineamento
//!
//! Un avviso può non arrivare mai. Mentre Rymembra è chiuso o il computer dorme
//! nessuno ascolta; e un osservatore muore **in silenzio**: `notify` 8.2 su
//! Windows, quando un completamento fallisce — la rete che cade, il buffer che
//! trabocca — toglie la sorveglianza senza mandare né un errore né un
//! `need_rescan`. Nessun avviso, e nessun avviso che ne manchino. Può capitare
//! anche su un disco di questo computer, col buffer che trabocca durante una
//! copia di migliaia di file (vedi [`RIALLINEAMENTO_LOCALE`]).
//!
//! Quindi ogni tanto una cartella si **riallinea**: una scansione prudente della
//! cartella intera, cioè la camminata e il piano, e si leggono soltanto i file
//! cambiati. Sulla libreria dell'utente — due cartelle sul NAS — riallinearle
//! tutte e due sono circa otto secondi e nessuna lettura. Quando, lo decide il
//! [`Pianificatore`]: all'avvio,
//! al risveglio dal sospeso, quando una cartella torna a rispondere, dopo ogni
//! (ri)armo di un osservatore, ogni [`RIALLINEAMENTO`] per quelle di rete e ogni
//! [`RIALLINEAMENTO_LOCALE`] per quelle di questo computer, rifacendo prima
//! l'osservatore — così una sordità che nessuno ha annunciato dura al più un
//! giro.
//!
//! Una cartella su cui l'osservatore non si arma resta col **solo
//! riallineamento**, ogni [`SONDAGGIO`]. Prende il posto del vecchio sondaggio a
//! fotografie, che camminava comunque ogni cartella, non vedeva un file
//! riscritto sul posto, e perdeva quel che non riusciva a guardare.
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
//! riguardare restano in attesa e si riprova poco dopo; e fra una scansione
//! automatica e la successiva passa un [`RESPIRO`], perché chi aspetta il turno
//! lo trovi libero. Quel che una scansione non ha potuto guardare — una cartella
//! che non ha risposto, una sottocartella che non si è letta, una scansione
//! fallita — torna in attesa con un rinvio, ognuno col suo: niente si perde,
//! niente gira a vuoto contro un disco spento, e un disco spento non trattiene
//! quel che aspetta sugli altri. La coda dei
//! download fa lo stesso con [`aspetta_il_turno`]: aspetta invece di lasciar
//! perdere.
//!
//! # Dove si accende e si spegne
//!
//! Nelle Impostazioni, sezione «Cartelle e scansione»:
//! [`imposta_sorveglianza`]. Di serie è accesa per tutte le cartelle, di rete
//! comprese. Spegnerla ferma gli osservatori subito, senza riavviare, e lascia
//! cadere quel che aspettava un rinvio: spenta, niente si riprova da sé. La
//! sorveglianza segue anche l'elenco delle cartelle — aggiunta, tolta,
//! traslocata — perché chi le cambia la avvisa ([`riconfigura`]) e perché
//! comunque ogni [`CONTROLLO`] rilegge l'elenco da sé: un profilo importato o
//! una sincronia le cambiano senza passare dai comandi.

mod accorpamento;
mod filo;
mod pianificatore;

use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use rymembra_app::library::Cambiamento;
use rymembra_app::volumi::Collocazione;
use rymembra_domain::errors::{AppError, ErrorCode};
use serde::Serialize;
use tauri::{AppHandle, Manager as _};

use crate::errore::{Esito, errore};
use crate::stato::{Stato, TurnoDiScansione};

pub use accorpamento::{Accorpatore, cambiamenti_dell_avviso};
pub use filo::{avvia, forse_in_rete};
pub use pianificatore::{Pianificatore, Riallineamento};

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

/// Ogni quanto si riallinea una cartella che ha il **solo** riallineamento:
/// quella su cui l'osservatore non si è potuto armare.
///
/// Cinque minuti: un disco copiato sul NAS da un altro computer compare in
/// libreria entro una pausa caffè, e il NAS si fa elencare le cartelle una
/// volta ogni tanto invece che di continuo. Su una libreria invariata è una
/// camminata senza letture.
///
/// È anche il rinvio di quel che non ha risposto — una cartella saltata da una
/// scansione automatica, un riallineamento che ha trovato il disco spento — e
/// che si riprova: abbastanza lungo da non girare a vuoto contro un NAS spento,
/// abbastanza corto da non aspettare il giro di [`RIALLINEAMENTO`].
///
/// Il nome è quello di prima della 3.0.2, quando era il passo del sondaggio a
/// fotografie: la finestra chiama ancora `sondaggio` le cartelle così.
pub const SONDAGGIO: Duration = Duration::from_secs(5 * 60);

/// Ogni quanto si riallinea una cartella di rete che ha gli avvisi, rifacendo
/// prima il suo osservatore.
///
/// Mezz'ora. Gli avvisi bastano finché l'osservatore è vivo, e quando muore lo
/// fa senza dirlo: questo è il tetto alla sordità. Il prezzo, sulla libreria
/// dell'utente, è qualche secondo di camminata per cartella ogni mezz'ora,
/// senza letture: otto, le due cartelle sul NAS insieme.
pub const RIALLINEAMENTO: Duration = Duration::from_secs(30 * 60);

/// Ogni quanto si riallinea una cartella di questo computer che ha gli avvisi,
/// rifacendo prima il suo osservatore.
///
/// Anche un osservatore locale può morire in silenzio. In `notify` 8.2 su
/// Windows ogni completamento di `ReadDirectoryChangesW` con un codice che non
/// sia «fatto», «annullato» o «accesso negato» toglie la sorveglianza e lo
/// scrive in un registro che Rymembra non ascolta, senza chiamare il gestore;
/// e il gestore, comunque vada, non riceve mai né un errore né un
/// `need_rescan`. Quando il sistema dice che il buffer è traboccato — e per
/// farlo traboccare basta una copia di migliaia di file sul disco di casa —
/// con `ERROR_NOTIFY_ENUM_DIR`, è uno di quei codici. I rami `Guasto` e
/// `need_rescan` di [`osserva`] lì non scattano mai.
///
/// Un'ora, il doppio della rete: qui la sordità ha una causa sola, e rara —
/// niente rete che cade. La camminata di un disco locale costa poco, qualche
/// secondo senza letture; quel che costa è svegliare un disco esterno che si
/// era fermato (Windows li ferma dopo venti minuti di serie), e una volta l'ora
/// è la metà dei risvegli della mezz'ora.
///
/// [`osserva`]: filo::osserva
pub const RIALLINEAMENTO_LOCALE: Duration = Duration::from_secs(60 * 60);

/// Fra quanto si riguarda un ramo perso la prima volta: una cartella che una
/// scansione non ha potuto leggere, dentro una cartella sorvegliata che invece
/// rispondeva (`rymembra_app::library::ScanReport::rami_persi`).
///
/// Un minuto: il caso tipico è un singhiozzo — il `.cue` di un CD appena
/// copiato che non si è letto in tempo, un pezzo di share che ha tardato — e un
/// minuto dopo si legge. Senza, quel CD compariva al giro di [`RIALLINEAMENTO`]
/// sul NAS, e su un disco locale mai. Ogni volta che il ramo si riperde
/// l'attesa raddoppia: uno, due, quattro, otto, sedici minuti.
const RAMO_PERSO: Duration = Duration::from_secs(60);

/// Quante volte si riguarda da sé un ramo perso prima di lasciarlo stare.
///
/// Cinque, cioè una mezz'ora in tutto (vedi [`RAMO_PERSO`]): quel che non si
/// legge da mezz'ora non è un singhiozzo, è una cartella a permessi negati, e
/// riprovarla per sempre sarebbe un giro senza fine. Lasciata stare non vuol
/// dire persa: i suoi brani restano — non verificato non è sparito — e la
/// riguarda ogni riallineamento della sua cartella sorvegliata. Il primo che
/// la trova leggibile la rimette in regola.
const RAMO_PERSO_TENTATIVI: u32 = 5;

/// Ogni quanto si rilegge da sé l'elenco delle cartelle e la scelta.
///
/// Un minuto. È la rete di sicurezza per chi cambia le cartelle senza passare
/// dai comandi che avvisano — l'importazione di un profilo, la sincronia — e
/// per ritentare un osservatore che non era partito perché il disco esterno
/// non era attaccato.
pub const CONTROLLO: Duration = Duration::from_secs(60);

/// Quanto si lascia fra una scansione automatica e la successiva, e quanto si
/// aspetta prima di riprovare un turno occupato.
///
/// Due secondi, cioè quattro passi di [`aspetta_il_turno`]: senza, il filo che
/// finisce un riallineamento ne comincerebbe un altro dopo un decimo di
/// secondo, e «Scansiona» o la coda dei download, che riprovano il turno ogni
/// mezzo secondo, non lo troverebbero mai libero finché la sorveglianza ha
/// qualcosa da fare.
const RESPIRO: Duration = Duration::from_secs(2);

/// Di quanto l'orologio di sistema deve superare il doppio dell'attesa chiesta
/// perché quell'attesa valga un sospeso.
///
/// Un minuto: un'attesa a vuoto di un minuto che sull'orologio del muro ne dura
/// più di tre non è un filo lento, è un computer che ha dormito. Una sospensione
/// più breve non si vede, e non serve: gli osservatori di rete che non
/// sopravvivono a due minuti di sonno li rifà comunque il giro di
/// [`RIALLINEAMENTO`].
const SONNO_MARGINE: Duration = Duration::from_secs(60);

/// Quanto si concede al disco per dire cosa sono i percorsi di una raffica.
///
/// Dieci secondi per tutta la raffica, non per percorso: i file di musica si
/// riconoscono dal nome, e il disco si interroga per le cartelle, una volta
/// per cartella. Su un NAS vivo è qualche millisecondo ciascuna; scaduto il
/// tempo — la condivisione è morta a metà — si riguardano le cartelle
/// sorvegliate intere, che è la risposta prudente.
const NATURA_SCADENZA: Duration = Duration::from_secs(10);

/// Quanto [`imposta_sorveglianza`] aspetta che il filo abbia rimesso in riga
/// gli osservatori, per restituire il quadro nuovo.
///
/// Tre secondi: l'interruttore nella finestra cambia quando arriva la risposta,
/// e di solito il filo ci mette qualche millisecondo. Se è in mezzo a una
/// scansione risponde dopo, e allora si restituisce il quadro com'è — la scelta
/// nuova, le cartelle ancora come prima — invece di tenere fermo il tasto.
const ATTESA_DEL_QUADRO: Duration = Duration::from_secs(3);

/// Ogni quanto chi aspetta il turno riprova a prenderlo.
const PASSO_DEL_TURNO: Duration = Duration::from_millis(500);

/// Quante domande «c'è ancora?» di fila devono restare senza risposta perché il
/// ritorno di una cartella di rete valga un riarmo e un riallineamento.
///
/// Due, cioè due minuti: una risposta sola che arriva tardi è un NAS che fa
/// girare i dischi, e costerebbe una camminata intera per niente.
const MANCATE_PER_TORNARE: u32 = 2;

// ── la scelta ───────────────────────────────────────────────────────────────

/// Cosa si sorveglia.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Modo {
    /// Niente: la libreria cambia solo con «Scansiona» e con i download.
    Spenta,
    /// I soli dischi di questo computer, con gli avvisi del sistema.
    Locali,
    /// Anche le condivisioni di rete: avvisi più riallineamento. Di serie
    /// dalla 3.0.2; chi aveva scelto `locali` o `spenta` resta com'era.
    #[default]
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
///
/// I nomi serializzati sono quelli che la finestra conosce, e restano.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Come {
    /// Con gli avvisi del sistema; sulla rete, più il riallineamento.
    Avvisi,
    /// Col **solo** riallineamento, ogni [`SONDAGGIO`]: l'osservatore non si è
    /// potuto armare, e lo si ritenta a ogni [`CONTROLLO`]. Il nome è quello
    /// del sondaggio a fotografie che c'era prima della 3.0.2.
    Sondaggio,
    /// Non la si guarda: sorveglianza spenta, o una cartella di rete con «solo
    /// i dischi di questo computer».
    Ferma,
    /// La si vorrebbe guardare e non si può: il disco non c'è, o il sistema
    /// ha rifiutato l'osservatore. Si ritenta a ogni [`CONTROLLO`].
    NonRaggiungibile,
}

/// La decisione, per una cartella: pura, perché è la parte che si prova.
///
/// È quel che si **vuole**: [`Come::Sondaggio`] non esce mai da qui, è il
/// ripiego di una cartella su cui gli avvisi si volevano e non si sono potuti
/// armare (vedi `arma`).
///
/// Una cartella di cui non si sa dove stia ([`Collocazione::Ignota`]) è una
/// lettera che in quel momento non risponde: un'unità mappata con il NAS
/// spento, o un disco esterno nel cassetto. Con «tutte» la si prova con gli
/// avvisi come le altre: se non si armano resta il solo riallineamento, e
/// l'osservatore si ritenta a ogni [`CONTROLLO`]. Fino alla 3.0.1 andava dritta
/// al sondaggio e lì restava per tutta la sessione, anche quando la lettera
/// tornava a rispondere. Con «locali» resta da riprovare a ogni [`CONTROLLO`]:
/// il disco esterno, riattaccato, torna a essere locale e merita gli avvisi.
#[must_use]
pub const fn come_sorvegliare(modo: Modo, dove: Collocazione) -> Come {
    match (modo, dove) {
        (Modo::Spenta, _) => Come::Ferma,
        (_, Collocazione::Locale) | (Modo::Tutte, _) => Come::Avvisi,
        (Modo::Locali, Collocazione::DiRete) => Come::Ferma,
        (Modo::Locali, Collocazione::Ignota) => Come::NonRaggiungibile,
    }
}

/// Con che passo si riallinea una cartella.
///
/// Lo decidono insieme come la si guarda e dove sta: è quel che il
/// [`Pianificatore`] sa di ogni cartella, e tutto quel che gli serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ritmo {
    /// Non la si guarda: non si riallinea.
    Nessuno,
    /// Gli avvisi di un disco di questo computer, più un riallineamento ogni
    /// [`RIALLINEAMENTO_LOCALE`] con l'osservatore rifatto prima: anche uno
    /// locale può morire in silenzio, quando il buffer trabocca. Il disco che
    /// se ne va invece lo si vede da sé (vedi `lascia_cadere_le_morte`).
    Locale,
    /// Gli avvisi di una condivisione di rete — o di una lettera che non si sa
    /// dove porti: più un riallineamento ogni [`RIALLINEAMENTO`], con
    /// l'osservatore rifatto prima.
    Rete,
    /// Il solo riallineamento, ogni [`SONDAGGIO`].
    SoloRiallineamento,
}

impl Ritmo {
    /// Il ritmo di una cartella guardata `come`, che sta `dove`.
    #[must_use]
    pub const fn di(come: Come, dove: Collocazione) -> Self {
        match (come, dove) {
            (Come::Avvisi, Collocazione::Locale) => Self::Locale,
            (Come::Avvisi, Collocazione::DiRete | Collocazione::Ignota) => Self::Rete,
            (Come::Sondaggio, _) => Self::SoloRiallineamento,
            (Come::Ferma | Come::NonRaggiungibile, _) => Self::Nessuno,
        }
    }

    /// Ogni quanto si riallinea da sé, se lo fa.
    const fn periodo(self) -> Option<Duration> {
        match self {
            Self::Nessuno => None,
            Self::Locale => Some(RIALLINEAMENTO_LOCALE),
            Self::Rete => Some(RIALLINEAMENTO),
            Self::SoloRiallineamento => Some(SONDAGGIO),
        }
    }

    /// La si guarda, e quindi la si riallinea quando serve.
    const fn guardata(self) -> bool {
        !matches!(self, Self::Nessuno)
    }

    /// Ha un osservatore, che può essere morto senza dirlo: prima di un
    /// riallineamento periodico o dopo un sospeso lo si rifà. Quella col solo
    /// riallineamento non ne ha, e il suo lo ritenta già ogni [`CONTROLLO`].
    const fn con_osservatore(self) -> bool {
        matches!(self, Self::Locale | Self::Rete)
    }
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
    /// ogni volta che una cartella si arma (`arma`). Un lucchetto foglia:
    /// nessuno lo tiene mentre ne chiede un altro.
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
    Avvisi(Vec<Cambiamento>),
    /// Cartelle da riguardare per intero: le cartelle che qualcuno chiede con
    /// [`riguarda`], o una radice di cui il sistema ha perso degli avvisi e lo
    /// dice — l'osservatore resta. Il secondo caso, con `notify` 8.2 su
    /// Windows, non arriva mai: vedi [`RIALLINEAMENTO_LOCALE`].
    Riguarda(Vec<String>),
    /// Un osservatore si è guastato e lo dice: lo si rifà al prossimo
    /// controllo, e lì la cartella si riallinea. Con `notify` 8.2 su Windows
    /// non succede mai — l'osservatore muore senza dirlo, e a quello pensano
    /// i riallineamenti periodici — ma il ramo resta per chi lo dicesse.
    Guasto(String),
    /// Le cartelle o la scelta sono cambiate: si rilegge tutto. Chi vuole
    /// sapere quando il controllo è finito manda di che rispondergli.
    Riconfigura(Option<Sender<()>>),
    /// Il risveglio delle cartelle di rete all'avvio è finito: si può
    /// riallineare.
    Svegliate,
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
/// Restituisce il quadro **dopo** che il filo ha rimesso in riga gli
/// osservatori, così le cartelle hanno già il loro modo nuovo. Lo si aspetta
/// al più [`ATTESA_DEL_QUADRO`]: un filo in mezzo a una scansione risponde
/// quando l'ha finita, e allora il quadro che torna ha la scelta nuova e le
/// cartelle ancora come prima.
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
    let (fatto, aspetta) = channel();
    {
        let Some(stato) = app.try_state::<StatoSorveglianza>() else {
            return Ok(QuadroSorveglianza::default());
        };
        stato.quadro().modo = scelto.come_testo().to_owned();
        stato.manda(Messaggio::Riconfigura(Some(fatto)));
    }
    let mano = app.clone();
    crate::disparte::in_disparte("sorveglianza delle cartelle", move || {
        // Scaduta o col filo che non c'è più — il canale si chiude e la
        // risposta è immediata — si restituisce quel che c'è.
        let _ = aspetta.recv_timeout(ATTESA_DEL_QUADRO);
        mano.try_state::<StatoSorveglianza>()
            .map(|stato| stato.quadro().clone())
            .unwrap_or_default()
    })
    .await
    .map_err(errore)
}

/// Le cartelle sorvegliate sono cambiate: il filo rilegga l'elenco adesso
/// invece che al prossimo [`CONTROLLO`]. Da chiamare dopo ogni comando che le
/// cambia.
pub fn riconfigura(app: &AppHandle) {
    if let Some(stato) = app.try_state::<StatoSorveglianza>() {
        stato.manda(Messaggio::Riconfigura(None));
    }
}

/// Chiede alla sorveglianza di riguardare queste cartelle: una scansione
/// prudente mirata, accorpata come gli avvisi.
///
/// Per chi ha cambiato quel che la libreria sa di dei file **senza** cambiare i
/// file — «dimentica l'arricchimento» sui brani d'immagine CD, che li rimette a
/// come li ricostruirebbe la scansione: nessun avviso arriverebbe, perché sul
/// disco non è successo niente.
///
/// Vale con qualunque scelta, anche spenta: è una richiesta, non una
/// sorveglianza. Le cartelle fuori dalle cartelle sorvegliate si ignorano,
/// come quelle di un avviso. Non aspetta niente: la scansione parte quando il
/// filo e il turno sono liberi, e se una cartella non risponde torna in attesa
/// come tutto il resto — tranne a sorveglianza spenta, dove niente si riprova
/// da sé: la richiesta si prova una volta, e se il disco non risponde la
/// rifarà la prossima «Scansiona».
pub fn riguarda(app: &AppHandle, cartelle: Vec<String>) {
    if cartelle.is_empty() {
        return;
    }
    if let Some(stato) = app.try_state::<StatoSorveglianza>() {
        stato.manda(Messaggio::Riguarda(cartelle));
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

#[cfg(test)]
mod prove;
