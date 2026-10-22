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
//! Nelle Impostazioni, sezione Libreria: [`imposta_sorveglianza`]. Di serie è
//! accesa per tutte le cartelle, di rete comprese. Spegnerla ferma gli
//! osservatori subito, senza riavviare, e lascia cadere quel che aspettava un
//! rinvio: spenta, niente si riprova da sé. La sorveglianza segue anche l'elenco
//! delle cartelle — aggiunta, tolta, traslocata — perché chi le cambia la avvisa
//! ([`riconfigura`]) e perché comunque ogni [`CONTROLLO`] rilegge l'elenco da
//! sé: un profilo importato o una sincronia le cambiano senza passare dai
//! comandi.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use notify::Watcher as _;
use rymembra_app::library::{
    Cambiamento, Deposito, Natura, Scadenze, Scan, cartelle_da_riguardare, piu_esterne,
};
use rymembra_app::volumi::{Collocazione, Esistenza};
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::paths::{PathRules, extension_of, is_supported_audio_path, is_under};
use serde::Serialize;
use tauri::{AppHandle, Manager as _};

use crate::errore::{Esito, errore};
use crate::nota;
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
/// armare (vedi [`arma`]).
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
    /// se ne va invece lo si vede da sé (vedi [`lascia_cadere_le_morte`]).
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

// ── l'accorpamento ──────────────────────────────────────────────────────────

/// Gli avvisi arrivati e non ancora scansionati.
///
/// Puro, con l'orologio passato da fuori: le prove gli danno degli istanti
/// inventati invece di aspettare due secondi veri.
#[derive(Debug, Default)]
pub struct Accorpatore {
    /// La raffica in corso: i percorsi che il sistema ha nominato — file,
    /// cartelle, vecchi nomi — e le cartelle che qualcuno ha chiesto di
    /// riguardare.
    cambiati: Vec<Cambiamento>,
    /// Il primo avviso della raffica in corso.
    primo: Option<Instant>,
    /// L'ultimo.
    ultimo: Option<Instant>,
    /// Quel che una scansione non ha potuto guardare perché il disco non ha
    /// risposto, perché una sottocartella non si è letta, o perché è fallita;
    /// ognuno con l'istante in cui riprovarlo.
    ///
    /// Aspetta il suo rinvio e non la raffica: riprovarlo a ogni raffica
    /// vorrebbe dire bussare a un NAS spento ogni volta che si copia un album
    /// sul disco di casa. E il **suo**, non uno per tutti: con un rinvio
    /// comune, il NAS spento che torna indietro ogni cinque minuti spostava
    /// avanti anche l'album rinviato sul disco di casa — per sempre.
    rinviati: Vec<(Cambiamento, Instant)>,
    /// I rami persi che si stanno riguardando da sé, e quante volte di fila si
    /// sono persi: vedi [`Self::rami`].
    rami: Vec<(String, u32)>,
}

impl Accorpatore {
    /// Cambiamenti nominati da un avviso.
    pub fn avvisati(&mut self, cambiati: impl IntoIterator<Item = Cambiamento>, adesso: Instant) {
        let prima = self.cambiati.len();
        self.cambiati.extend(cambiati);
        if self.cambiati.len() > prima {
            self.primo.get_or_insert(adesso);
            self.ultimo = Some(adesso);
        }
    }

    /// Cartelle da riguardare per intero.
    ///
    /// Entrano come cambiamenti **nuovi**, cioè «se è una cartella, guardala da
    /// sé»: è esattamente quel che si chiede, e se nel frattempo la cartella è
    /// sparita si risale a quella che la conteneva invece di saltarla.
    pub fn da_riguardare(&mut self, cartelle: impl IntoIterator<Item = String>, adesso: Instant) {
        self.avvisati(
            cartelle.into_iter().map(|percorso| Cambiamento {
                percorso,
                nuovo: true,
            }),
            adesso,
        );
    }

    /// La raffica tace da [`ACCORPAMENTO`], o dura da [`ATTESA_MASSIMA`].
    fn raffica_pronta(&self, adesso: Instant) -> bool {
        match (self.primo, self.ultimo) {
            (Some(primo), Some(ultimo)) => {
                adesso.saturating_duration_since(ultimo) >= ACCORPAMENTO
                    || adesso.saturating_duration_since(primo) >= ATTESA_MASSIMA
            }
            _ => false,
        }
    }

    /// Il rinvio di almeno un rinviato è passato.
    fn rinvio_scaduto(&self, adesso: Instant) -> bool {
        self.rinviati.iter().any(|(_, quando)| *quando <= adesso)
    }

    /// È il momento di scansionare: la raffica è finita, o il rinvio è passato.
    #[must_use]
    pub fn pronto(&self, adesso: Instant) -> bool {
        self.raffica_pronta(adesso) || self.rinvio_scaduto(adesso)
    }

    /// Quanto manca a [`Self::pronto`]. `None` se non c'è niente in attesa.
    #[must_use]
    pub fn manca(&self, adesso: Instant) -> Option<Duration> {
        let per_la_raffica = match (self.primo, self.ultimo) {
            (Some(primo), Some(ultimo)) => {
                let per_silenzio = (ultimo + ACCORPAMENTO).saturating_duration_since(adesso);
                let per_tetto = (primo + ATTESA_MASSIMA).saturating_duration_since(adesso);
                Some(per_silenzio.min(per_tetto))
            }
            _ => None,
        };
        let per_il_rinvio = self
            .rinviati
            .iter()
            .map(|(_, quando)| quando.saturating_duration_since(adesso))
            .min();
        match (per_la_raffica, per_il_rinvio) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Quel che è pronto, e da lì l'attesa ricomincia.
    ///
    /// Solo quel che è pronto: una raffica ancora in corso aspetta la sua fine
    /// anche se intanto è scaduto un rinvio, e viceversa; e dei rinviati, solo
    /// quelli il cui rinvio è passato.
    pub fn prendi(&mut self, adesso: Instant) -> Vec<Cambiamento> {
        let mut presi = Vec::new();
        if self.raffica_pronta(adesso) {
            presi.append(&mut self.cambiati);
            self.primo = None;
            self.ultimo = None;
        }
        if self.rinvio_scaduto(adesso) {
            let (scaduti, restano): (Vec<_>, Vec<_>) = std::mem::take(&mut self.rinviati)
                .into_iter()
                .partition(|(_, quando)| *quando <= adesso);
            self.rinviati = restano;
            presi.extend(scaduti.into_iter().map(|(cambiamento, _)| cambiamento));
        }
        presi
    }

    /// Rimette in attesa quel che non si è potuto scansionare per una ragione
    /// che passa presto — una scansione fermata a metà — e ci si riprova fra
    /// [`ACCORPAMENTO`]. Il turno di qualcun altro invece non tocca l'attesa:
    /// vedi [`Motore::prepara_la_riscansione`].
    pub fn rimetti(&mut self, cambiati: Vec<Cambiamento>, adesso: Instant) {
        if cambiati.is_empty() {
            return;
        }
        self.cambiati.extend(cambiati);
        // Da capo tutte e due: con il primo vecchio il tetto sarebbe già
        // superato, e si girerebbe a vuoto contro un turno occupato.
        self.primo = Some(adesso);
        self.ultimo = Some(adesso);
    }

    /// Rimanda di `quanto` quel che non si è potuto scansionare perché il disco
    /// non ha risposto, o perché la scansione è fallita.
    ///
    /// Ognuno col suo rinvio: quel che sta su un disco spento non trattiene
    /// quel che sta su un altro. Lo stesso percorso già in attesa resta una
    /// voce sola, e vince il rinvio più lontano: è fermo sullo stesso disco di
    /// quel che è appena tornato indietro, e riprovarlo prima sarebbe bussare
    /// due volte.
    pub fn rinvia(
        &mut self,
        cambiati: Vec<Cambiamento>,
        adesso: Instant,
        quanto: Duration,
        regole: PathRules,
    ) {
        let quando = adesso + quanto;
        for cambiamento in cambiati {
            let stesso = |gia: &Cambiamento| {
                gia.nuovo == cambiamento.nuovo
                    && is_under(&gia.percorso, &cambiamento.percorso, regole)
                    && is_under(&cambiamento.percorso, &gia.percorso, regole)
            };
            match self.rinviati.iter_mut().find(|(gia, _)| stesso(gia)) {
                Some((_, gia_quando)) => *gia_quando = (*gia_quando).max(quando),
                None => self.rinviati.push((cambiamento, quando)),
            }
        }
    }

    /// Lascia cadere tutti i rinviati, e il conto dei rami persi, e dice quanti
    /// rinviati erano: la sorveglianza è stata spenta, e da spenta niente si
    /// riprova da sé. La raffica resta: può portare una richiesta di
    /// [`riguarda`], che vale anche da spenta.
    pub fn svuota_rinviati(&mut self) -> usize {
        self.rami.clear();
        std::mem::take(&mut self.rinviati).len()
    }

    /// Com'è andata coi rami persi una passata che ha guardato `guardate` — le
    /// sue cartelle che hanno risposto — e non ha potuto leggere `persi`.
    ///
    /// Un ramo perso torna in attesa fra [`RAMO_PERSO`], e fra il doppio ogni
    /// volta che si riperde, per [`RAMO_PERSO_TENTATIVI`] volte; poi lo si
    /// lascia stare, e lo si restituisce — una volta sola — perché finisca nel
    /// diario. Lasciato stare resta ricordato: ogni riallineamento della sua
    /// cartella sorvegliata lo riperderebbe, e senza memoria ricomincerebbe il
    /// giro da un minuto, per sempre. Si dimentica quando una passata lo guarda
    /// e lo legge: sta sotto una cartella guardata, e non sotto un ramo perso
    /// adesso.
    pub fn rami(
        &mut self,
        guardate: &[String],
        persi: &[String],
        adesso: Instant,
        regole: PathRules,
    ) -> Vec<String> {
        self.rami.retain(|(ramo, _)| {
            !guardate
                .iter()
                .any(|cartella| is_under(ramo, cartella, regole))
                || persi.iter().any(|perso| is_under(ramo, perso, regole))
        });
        let mut lasciati = Vec::new();
        for perso in persi {
            let posto = self.rami.iter().position(|(ramo, _)| {
                is_under(ramo, perso, regole) && is_under(perso, ramo, regole)
            });
            let volte = match posto.and_then(|posto| self.rami.get_mut(posto)) {
                Some((_, volte)) => {
                    *volte = volte.saturating_add(1);
                    *volte
                }
                None => {
                    self.rami.push((perso.clone(), 1));
                    1
                }
            };
            if volte > RAMO_PERSO_TENTATIVI {
                if volte == RAMO_PERSO_TENTATIVI.saturating_add(1) {
                    lasciati.push(perso.clone());
                }
                continue;
            }
            // Uno, due, quattro... minuti: `volte` non supera il tetto, e
            // il doppio del doppio resta piccolo.
            let attesa = RAMO_PERSO.saturating_mul(2_u32.saturating_pow(volte.saturating_sub(1)));
            self.rinvia(
                vec![Cambiamento {
                    percorso: perso.clone(),
                    nuovo: true,
                }],
                adesso,
                attesa,
                regole,
            );
        }
        lasciati
    }

    /// Toglie dall'attesa e restituisce tutto quel che sta sotto `radice`, in
    /// raffica o in rinvio: un riallineamento della radice lo guarda già.
    ///
    /// Chi lo chiama lo rimette se il riallineamento non va in porto.
    pub fn togli_sotto(&mut self, radice: &str, regole: PathRules) -> Vec<Cambiamento> {
        let sotto = |cambiamento: &Cambiamento| is_under(&cambiamento.percorso, radice, regole);
        let (mut tolti, restano): (Vec<_>, Vec<_>) = std::mem::take(&mut self.cambiati)
            .into_iter()
            .partition(&sotto);
        self.cambiati = restano;
        if self.cambiati.is_empty() {
            self.primo = None;
            self.ultimo = None;
        }
        let (rinviati_sotto, restano): (Vec<_>, Vec<_>) = std::mem::take(&mut self.rinviati)
            .into_iter()
            .partition(|(cambiamento, _)| sotto(cambiamento));
        self.rinviati = restano;
        tolti.extend(
            rinviati_sotto
                .into_iter()
                .map(|(cambiamento, _)| cambiamento),
        );
        tolti
    }
}

/// I file che non sono musica e cambiano spesso accanto alla musica: le
/// copertine, i testi che Rymembra stesso può scrivere accanto ai brani, le
/// liste, i file temporanei di una copia o di un download.
///
/// Un avviso su uno di questi non vale una scansione. Un nome sconosciuto
/// invece sì: è meglio una camminata in più che un album che non compare.
///
/// Il `.cue` non c'è più dalla 3.0.2: è il foglio di un'immagine CD, e
/// cambiarlo cambia i brani che l'immagine diventa in libreria.
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

/// I cambiamenti che un avviso del sistema racconta.
///
/// - Creato, o rinominato **verso** questo nome: nuovo. Se è una cartella la si
///   guarderà da sé — un album copiato è la sua cartella, non quella
///   dell'artista.
/// - Una modifica di contenuto, di metadati o generica su una **cartella** si
///   salta. Lo stato di una cartella cambia perché è cambiato un figlio, e il
///   figlio ha già il suo avviso: tenerla vorrebbe dire riguardare il genitore
///   per un file che sta un piano più giù. Sul NAS dell'utente arriva a ogni
///   file copiato.
/// - Tutto il resto — tolto, rinominato **da**, una modifica di un file — non
///   è nuovo: si guarda dalla cartella che lo contiene.
///
/// `natura` è chiesta solo per le modifiche, cioè dove serve a decidere.
pub fn cambiamenti_dell_avviso(
    evento: &notify::Event,
    natura: &dyn Fn(&Path) -> Natura,
) -> Vec<Cambiamento> {
    use notify::EventKind;
    use notify::event::{ModifyKind, RenameMode};
    let cambiamento = |percorso: &Path, nuovo: bool| Cambiamento {
        percorso: percorso.to_string_lossy().into_owned(),
        nuovo,
    };
    let tutti = |nuovo: bool| -> Vec<Cambiamento> {
        evento
            .paths
            .iter()
            .filter(|percorso| conta(percorso.as_path()))
            .map(|percorso| cambiamento(percorso.as_path(), nuovo))
            .collect()
    };
    match evento.kind {
        EventKind::Access(_) => Vec::new(),
        EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(RenameMode::To)) => tutti(true),
        // Un nome solo per due percorsi: il primo è quello vecchio.
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => evento
            .paths
            .iter()
            .enumerate()
            .filter(|(_, percorso)| conta(percorso.as_path()))
            .map(|(posto, percorso)| cambiamento(percorso.as_path(), posto > 0))
            .collect(),
        EventKind::Modify(
            ModifyKind::Any | ModifyKind::Data(_) | ModifyKind::Metadata(_) | ModifyKind::Other,
        ) => evento
            .paths
            .iter()
            .filter(|percorso| {
                conta(percorso.as_path()) && natura(percorso.as_path()) != Natura::Cartella
            })
            .map(|percorso| cambiamento(percorso.as_path(), false))
            .collect(),
        _ => tutti(false),
    }
}

/// Che cos'è un percorso, chiesto al disco — ma non per i file di musica.
///
/// Un nome con l'estensione di un brano o di un foglio `.cue` si prende per un
/// file senza chiedere: è la quasi totalità degli avvisi di una copia, e sul NAS
/// ogni domanda è un giro di rete. Una cartella che si chiamasse `Disco.flac`
/// verrebbe presa per un file, e si guarderebbe dalla cartella che la contiene:
/// una scansione un po' più larga, non un brano perso.
///
/// Senza seguire i collegamenti, come la scansione: un collegamento non è una
/// cartella da camminare.
fn natura_di(percorso: &Path) -> Natura {
    let testo = percorso.to_string_lossy();
    if is_supported_audio_path(&testo) || extension_of(&testo) == "cue" {
        return Natura::File;
    }
    match std::fs::symlink_metadata(rymembra_app::files::percorso_lungo(percorso)) {
        Ok(metadati) if metadati.is_dir() => Natura::Cartella,
        Ok(_) => Natura::File,
        Err(_) => Natura::Assente,
    }
}

/// Le cartelle da scansionare per dei cambiamenti, col disco interrogato
/// sotto una scadenza.
///
/// Fuori dal filo della sorveglianza, su un filo che si può abbandonare: una
/// condivisione che muore fra l'avviso e la domanda risponderebbe dopo il
/// timeout di SMB, per ogni percorso. Scaduta [`NATURA_SCADENZA`], ogni
/// cambiamento diventa la cartella sorvegliata che lo contiene — la scelta
/// prudente: una camminata più larga, e se il disco è davvero spento la
/// radice risulterà saltata e tornerà in attesa.
///
/// `natura` è [`natura_di`]; le prove ne passano una che conta le domande.
fn cartelle_da_guardare(
    cambiati: Vec<Cambiamento>,
    radici: Vec<String>,
    regole: PathRules,
    natura: fn(&Path) -> Natura,
) -> Vec<String> {
    if cambiati.is_empty() {
        return Vec::new();
    }
    let (per_il_filo, radici_del_filo) = (cambiati.clone(), radici.clone());
    let decise = rymembra_app::scadenza::con_scadenza("natura", NATURA_SCADENZA, move || {
        // Una domanda per cartella e non per avviso: i venti file di un album
        // chiedono tutti della stessa.
        let memoria: RefCell<HashMap<String, Natura>> = RefCell::default();
        let ricordata = |percorso: &str| {
            *memoria
                .borrow_mut()
                .entry(percorso.to_owned())
                .or_insert_with(|| natura(Path::new(percorso)))
        };
        cartelle_da_riguardare(&per_il_filo, &radici_del_filo, regole, &ricordata)
    });
    decise.unwrap_or_else(|| {
        nota!(
            "[sorveglia] il disco non ha detto in tempo cosa è cambiato: si riguardano le cartelle sorvegliate intere"
        );
        cartelle_da_riguardare(&cambiati, &radici, regole, &|_| Natura::Assente)
    })
}

// ── il pianificatore dei riallineamenti ─────────────────────────────────────

/// Un riallineamento da fare adesso.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Riallineamento {
    /// La cartella sorvegliata.
    pub radice: String,
    /// Prima di camminarla si rifà l'osservatore.
    ///
    /// Prima e non dopo: quel che cambia mentre si cammina lo vede già quello
    /// nuovo. E rifarlo è l'unico modo di sapere che quello vecchio non era
    /// sordo, perché un osservatore di rete sordo non lo dice (vedi il
    /// preambolo).
    pub riarma: bool,
}

/// Quel che il pianificatore sa di una cartella.
#[derive(Debug, Clone)]
struct Programma {
    /// Il passo, come l'ha detto l'ultima configurazione.
    ritmo: Ritmo,
    /// Va riallineata appena si può.
    dovuta: bool,
    /// E prima si rifà l'osservatore.
    riarma: bool,
    /// Il prossimo riallineamento periodico, o il rinvio di uno che non ha
    /// risposto.
    prossimo: Option<Instant>,
}

/// Quando riallineare quale cartella.
///
/// Puro, come l'[`Accorpatore`]: l'orologio arriva da fuori, e le prove gli
/// raccontano mezz'ore e sospensioni senza aspettarle. Il filo lo informa dei
/// fatti — com'è stata armata una cartella, il risveglio finito, un'attesa
/// durata troppo — e gli chiede ogni volta [`Self::prossima`].
///
/// # Le cause
///
/// - **L'avvio**, dopo il risveglio delle cartelle di rete: per le cartelle
///   che erano nell'elenco quando il filo è partito ([`Self::fotografa`]).
///   Quello che si è perso mentre Rymembra era chiuso non ha mandato avvisi a
///   nessuno. Il primo riallineamento aspetta [`Self::avvio`] perché un NAS
///   addormentato, interrogato prima di essersi svegliato, risponderebbe «non
///   ci sono» e la cartella risulterebbe saltata.
/// - **Il risveglio dal sospeso**, visto da [`Self::sveglio`]: tutte, e con
///   l'osservatore rifatto prima — locali comprese.
/// - **Il ritorno** di una cartella di rete che per un po' non ha risposto
///   ([`Self::tornata`]).
/// - **Ogni cambio di passo** ([`Self::imposta`]): una cartella che passa da
///   non guardata a guardata è stata un po' senza nessuno a guardarla — un
///   disco staccato e riattaccato, un osservatore guastato e rifatto, la
///   sorveglianza riaccesa.
/// - **Il periodico**, con l'osservatore rifatto prima: ogni [`RIALLINEAMENTO`]
///   per le cartelle di rete con gli avvisi, ogni [`RIALLINEAMENTO_LOCALE`] per
///   quelle di questo computer; ogni [`SONDAGGIO`] per quelle col solo
///   riallineamento, che un osservatore non ce l'hanno.
///
/// Più cause insieme fanno un riallineamento solo. Una cartella **aggiunta**
/// mentre Rymembra è aperto non si riallinea al primo armo: chi la aggiunge la
/// scansiona — il primo avvio lo fa da sé — e una lettura intera partita da
/// sola terrebbe il turno proprio a quella «Scansiona». Vale anche per quella
/// scelta nei primi secondi, prima che il filo abbia letto l'elenco: è il primo
/// avvio, e la sua «Scansiona» parte subito.
#[derive(Debug, Default)]
pub struct Pianificatore {
    /// Le cartelle, per nome.
    radici: BTreeMap<String, Programma>,
    /// Il risveglio delle cartelle di rete all'avvio è finito: da qui si
    /// riallinea.
    avviato: bool,
    /// L'elenco delle cartelle è stato letto almeno una volta: quelle che
    /// arrivano dopo sono aggiunte, non ritrovate.
    elenco_letto: bool,
    /// Le cartelle dell'elenco quando il filo è partito: solo loro hanno il
    /// riallineamento dell'avvio. `None` se non si sono potute leggere, e
    /// allora vale il primo elenco letto. Vedi [`Self::fotografa`].
    dell_avvio: Option<Vec<String>>,
    /// Niente prima di questo istante: il turno era occupato, o una scansione
    /// automatica è appena finita.
    non_prima: Option<Instant>,
    /// L'attesa a vuoto in corso: l'orologio di sistema quando è cominciata, e
    /// quanto si è chiesto di aspettare.
    sonno: Option<(SystemTime, Duration)>,
}

impl Pianificatore {
    /// Il filo ha appena (ri)configurato una cartella, e questo è il suo passo.
    ///
    /// Una cartella nuova si riallinea se arriva con il primo elenco letto ed
    /// era già nell'elenco quando il filo è partito; una già nota se il passo
    /// cambia verso uno guardato. Un passo uguale — un osservatore rifatto per
    /// il periodico, un ripiego che riprova e non ce la fa — non è una causa: è
    /// già dovuta, o non è cambiato niente.
    pub fn imposta(&mut self, radice: &str, ritmo: Ritmo, adesso: Instant) {
        let prossimo = ritmo.periodo().map(|periodo| adesso + periodo);
        let Some(programma) = self.radici.get_mut(radice) else {
            let dell_avvio = !self.elenco_letto
                && self
                    .dell_avvio
                    .as_ref()
                    .is_none_or(|elenco| elenco.iter().any(|gia| gia == radice));
            self.radici.insert(
                radice.to_owned(),
                Programma {
                    ritmo,
                    dovuta: dell_avvio && ritmo.guardata(),
                    riarma: false,
                    prossimo,
                },
            );
            return;
        };
        // È appena stata rifatta: un riarmo in sospeso è fatto.
        programma.riarma = false;
        if programma.ritmo == ritmo {
            return;
        }
        programma.ritmo = ritmo;
        programma.dovuta = ritmo.guardata();
        programma.prossimo = prossimo;
    }

    /// Dimentica le cartelle che non sono più nell'elenco.
    pub fn tieni_solo(&mut self, radici: &[String]) {
        self.radici.retain(|radice, _| radici.contains(radice));
    }

    /// L'elenco com'era quando il filo è partito: solo queste cartelle hanno
    /// il riallineamento dell'avvio.
    ///
    /// Si fotografa prima che chiunque possa aggiungerne. Il filo legge
    /// l'elenco la prima volta dopo [`ATTESA_AVVIO`], o prima se qualcuno lo
    /// cambia; e al primo avvio è proprio quello che succede — la cartella
    /// scelta nei primi secondi arrivava con il primo elenco letto, si
    /// riallineava come se fosse stata lì da sempre, cioè si importava da
    /// sola, senza avanzamento, e la «Scansiona» partita insieme aspettava la
    /// sua fine e ricamminava tutto.
    pub fn fotografa(&mut self, radici: Vec<String>) {
        self.dell_avvio = Some(radici);
    }

    /// L'elenco è stato letto: da qui una cartella nuova è un'aggiunta.
    pub fn elenco_letto(&mut self) {
        self.elenco_letto = true;
        self.dell_avvio = None;
    }

    /// Il risveglio delle cartelle di rete è finito: i riallineamenti possono
    /// cominciare.
    pub fn avvio(&mut self) {
        self.avviato = true;
    }

    /// Il filo sta per aspettare a vuoto `chiesta`; `muro` è l'orologio di
    /// sistema adesso.
    pub fn dormo(&mut self, chiesta: Duration, muro: SystemTime) {
        self.sonno = Some((muro, chiesta));
    }

    /// L'attesa è finita; `muro` è l'orologio di sistema adesso. `true` se è
    /// durata tanto da essere un sospeso, e allora si riallinea tutto.
    ///
    /// # Perché solo l'attesa
    ///
    /// Perché è l'unico tratto di cui si sa quanto doveva durare. Una
    /// scansione può durare dieci minuti su un NAS lento, e misurata col muro
    /// sembrerebbe un sonno; un'attesa di un minuto che ne dura tre no. Per
    /// questo la misura comincia a [`Self::dormo`] e qui si consuma: quel che
    /// succede fra un'attesa e l'altra non entra mai nel conto.
    ///
    /// E l'orologio di sistema, non `Instant`: su Windows non è garantito che
    /// `Instant` conti il tempo passato in sospensione, e confrontare i due
    /// sarebbe misurare la differenza fra due orologi che non si sa come
    /// contano. Un orologio portato indietro durante l'attesa non è un sonno; uno
    /// portato avanti di ore sì, e costa un riallineamento in più.
    pub fn sveglio(&mut self, muro: SystemTime) -> bool {
        let Some((inizio, chiesta)) = self.sonno.take() else {
            return false;
        };
        let Ok(trascorso) = muro.duration_since(inizio) else {
            return false;
        };
        if trascorso <= chiesta.saturating_mul(2).saturating_add(SONNO_MARGINE) {
            return false;
        }
        for programma in self.radici.values_mut() {
            if programma.ritmo.guardata() {
                programma.dovuta = true;
                // Un osservatore di rete non sopravvive a un sonno lungo, e se
                // è morto non lo dice. Uno locale di solito sopravvive, ma se
                // non ce l'ha fatta non lo dice nemmeno lui: rifarlo costa un
                // attimo, tenerne uno sordo fino al giro dopo costa un'ora.
                programma.riarma |= programma.ritmo.con_osservatore();
            }
        }
        true
    }

    /// Una cartella di rete torna a rispondere dopo un po' di silenzio: il suo
    /// osservatore è probabilmente morto con la rete, e quel che è cambiato
    /// intanto non l'ha visto nessuno.
    pub fn tornata(&mut self, radice: &str) {
        if let Some(programma) = self.radici.get_mut(radice)
            && programma.ritmo.guardata()
        {
            programma.dovuta = true;
            programma.riarma |= programma.ritmo == Ritmo::Rete;
        }
    }

    /// Il prossimo riallineamento da fare adesso, se ce n'è uno e se si può.
    ///
    /// Uno alla volta, nell'ordine delle cartelle: fra uno e l'altro il filo
    /// legge la sua posta e lascia un [`RESPIRO`] a chi aspetta il turno.
    pub fn prossima(&mut self, adesso: Instant) -> Option<Riallineamento> {
        // I periodici scaduti diventano dovuti. Il passo si azzera qui e non
        // alla fine: se la cartella non si può riallineare subito — il turno è
        // occupato — non deve maturare di nuovo, né chiedere un altro riarmo.
        for programma in self.radici.values_mut() {
            if programma.ritmo.guardata()
                && programma.prossimo.is_some_and(|quando| quando <= adesso)
            {
                programma.dovuta = true;
                programma.riarma |= programma.ritmo.con_osservatore();
                programma.prossimo = None;
            }
        }
        if !self.libero(adesso) {
            return None;
        }
        self.radici
            .iter()
            .find(|(_, programma)| programma.dovuta && programma.ritmo.guardata())
            .map(|(radice, programma)| Riallineamento {
                radice: radice.clone(),
                riarma: programma.riarma,
            })
    }

    /// Si può cominciare una scansione automatica: l'avvio è fatto, e il
    /// [`RESPIRO`] è passato.
    #[must_use]
    pub fn libero(&self, adesso: Instant) -> bool {
        self.avviato && self.fra_quanto_libero(adesso).is_zero()
    }

    /// Quanto manca alla fine del [`RESPIRO`].
    #[must_use]
    pub fn fra_quanto_libero(&self, adesso: Instant) -> Duration {
        self.non_prima.map_or(Duration::ZERO, |quando| {
            quando.saturating_duration_since(adesso)
        })
    }

    /// Niente per un [`RESPIRO`]: il turno era occupato, o una scansione
    /// automatica è appena finita.
    pub fn aspetta_un_respiro(&mut self, adesso: Instant) {
        self.non_prima = Some(adesso + RESPIRO);
    }

    /// Il riallineamento di `radice` è finito; `risposto` se la cartella ha
    /// risposto: guardata tutta, o trovata vuota dalla guardia per radice.
    ///
    /// Vuota vale risposto, e resta al suo passo: una cartella che risponde
    /// vuota risponderà vuota anche fra cinque minuti, e riprovarla a ogni
    /// [`SONDAGGIO`] era un riallineamento ogni cinque minuti per sempre su una
    /// cartella svuotata davvero. La riguardano il suo periodico, il risveglio,
    /// il ritorno — o una «Scansiona», dove c'è chi può confermare.
    ///
    /// Una cartella che non ha risposto resta in debito, e si riprova al
    /// prossimo motivo — il ritorno, il risveglio, un riarmo — o al più fra
    /// [`SONDAGGIO`]: mai subito, che su un NAS spento sarebbe un ciclo stretto
    /// di sonde che scadono. Anche una cartella locale, che di periodico ne ha
    /// uno lungo: un disco esterno che gira ancora a vuoto non deve aspettare
    /// un'ora.
    pub fn fatto(&mut self, radice: &str, risposto: bool, adesso: Instant) {
        self.aspetta_un_respiro(adesso);
        let Some(programma) = self.radici.get_mut(radice) else {
            return;
        };
        programma.dovuta = false;
        programma.riarma = false;
        programma.prossimo = if !programma.ritmo.guardata() {
            None
        } else if risposto {
            programma.ritmo.periodo().map(|periodo| adesso + periodo)
        } else {
            let rinvio = programma
                .ritmo
                .periodo()
                .map_or(SONDAGGIO, |periodo| periodo.min(SONDAGGIO));
            Some(adesso + rinvio)
        };
    }

    /// Quanto manca al prossimo riallineamento. `None` se non ce n'è nessuno
    /// in vista — o se l'avvio non è ancora arrivato: lo porta un messaggio,
    /// che sveglia il filo da sé.
    #[must_use]
    pub fn manca(&self, adesso: Instant) -> Option<Duration> {
        if !self.avviato {
            return None;
        }
        let primo = self
            .radici
            .values()
            .filter(|programma| programma.ritmo.guardata())
            .filter_map(|programma| {
                if programma.dovuta {
                    Some(Duration::ZERO)
                } else {
                    programma
                        .prossimo
                        .map(|quando| quando.saturating_duration_since(adesso))
                }
            })
            .min()?;
        Some(primo.max(self.fra_quanto_libero(adesso)))
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
    /// ogni volta che una cartella si arma ([`arma`]). Un lucchetto foglia:
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

// ── il filo ─────────────────────────────────────────────────────────────────

/// Registra lo stato e avvia il filo. Da chiamare nel `setup`, dopo la
/// libreria.
pub fn avvia(app: &AppHandle) {
    // La scelta si legge subito, così la finestra che chiede il quadro nei
    // primi secondi vede quella vera e non quella di serie. E con lei l'elenco
    // delle cartelle com'è adesso, prima che la finestra possa cambiarlo: solo
    // queste hanno il riallineamento dell'avvio (vedi
    // `Pianificatore::fotografa`). Un elenco che non si legge non toglie la
    // scelta.
    let letto = app.try_state::<Stato>().and_then(|stato| {
        con_libreria(&stato, |l| {
            let modo = rymembra_app::settings::read(&l.connection, CHIAVE_SORVEGLIANZA)?;
            let radici = rymembra_app::settings::read_json::<Vec<String>>(
                &l.connection,
                rymembra_app::settings::CHIAVE_CARTELLE,
            )
            .ok()
            .map(Option::unwrap_or_default);
            Ok((modo, radici))
        })
        .ok()
    });
    let (modo, dell_avvio) = letto.unwrap_or_default();
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
    let del_filo = posta.clone();
    let avviato = std::thread::Builder::new()
        .name("rymembra-sorveglia".to_owned())
        .spawn(move || filo(&mano, &del_filo, &orecchio, dell_avvio));
    if let Err(err) = avviato {
        nota!("[sorveglia] il filo non è partito: {err}");
    }
    let mano = app.clone();
    let avviato = std::thread::Builder::new()
        .name("rymembra-risveglio".to_owned())
        .spawn(move || {
            let _fine = FineDelRisveglio(mano.clone());
            risveglia(&mano);
        });
    if let Err(err) = avviato {
        nota!("[sorveglia] il risveglio delle cartelle di rete non è partito: {err}");
        // Senza risveglio non c'è niente da aspettare: il primo riallineamento
        // proverà da sé.
        let _ = posta.send(Messaggio::Svegliate);
    }
}

/// Dice al filo che il risveglio è finito, comunque sia finito — anche
/// cadendo in panico: il primo riallineamento aspetta questo messaggio, e senza
/// non partirebbe mai.
struct FineDelRisveglio(AppHandle);

impl Drop for FineDelRisveglio {
    fn drop(&mut self) {
        if let Some(stato) = self.0.try_state::<StatoSorveglianza>() {
            stato.manda(Messaggio::Svegliate);
        }
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
/// sveglia lo dirà chi la usa, quando la usa. Quando ha finito, il filo della
/// sorveglianza comincia i riallineamenti dell'avvio (vedi
/// [`FineDelRisveglio`]): su un NAS che dorme, chi cammina prima di aver bussato
/// trova le cartelle «non raggiungibili».
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
/// dieci secondi dopo non perde niente: il riallineamento dell'avvio guarda
/// comunque tutto.
const ATTESA_AVVIO: Duration = Duration::from_secs(10);

/// Una cartella sorvegliata, dal lato del filo.
struct Sorvegliata {
    /// Come la si guarda.
    come: Come,
    /// Dove sta, quando la si è armata.
    dove: Collocazione,
    /// L'osservatore, quando la si guarda con gli avvisi. Cadendo smette.
    osservatore: Option<notify::RecommendedWatcher>,
    /// Le domande «c'è ancora?» di fila rimaste senza risposta. Conta solo per
    /// le cartelle di rete con gli avvisi: vedi [`lascia_cadere_le_morte`].
    senza_risposta: u32,
}

impl Sorvegliata {
    /// Il passo dei suoi riallineamenti.
    const fn ritmo(&self) -> Ritmo {
        Ritmo::di(self.come, self.dove)
    }
}

/// Quanto si aspetta la risposta a «la cartella c'è ancora?» per una cartella
/// con gli avvisi, a ogni [`CONTROLLO`].
///
/// Due secondi: un disco di questo computer risponde in microsecondi, e un NAS
/// vivo in millisecondi. Uno che non risponde in due secondi non è dato per
/// perso — vedi [`MANCATE_PER_TORNARE`].
const VIVA_SCADENZA: Duration = Duration::from_secs(2);

/// Lo stato del filo fra un giro e l'altro.
struct Motore {
    /// La scelta in vigore.
    modo: Modo,
    /// Le cartelle nell'ordine dell'elenco: è quello in cui la finestra le
    /// mostra.
    elenco: Vec<String>,
    /// Le cartelle, con la grafia con cui stanno nell'elenco.
    cartelle: BTreeMap<String, Sorvegliata>,
    /// Gli avvisi in attesa.
    attesa: Accorpatore,
    /// Quando riallineare.
    piano: Pianificatore,
    /// Il prossimo controllo dell'elenco.
    prossimo_controllo: Instant,
    /// L'elenco è stato letto almeno una volta. Prima le cartelle da
    /// riguardare non si possono giudicare: fuori dalle cartelle sorvegliate
    /// si scarterebbero tutte.
    configurata: bool,
    /// Chi aspetta la fine del prossimo controllo.
    chi_aspetta: Vec<Sender<()>>,
    /// Dopo un giro caduto in panico, fino a quando non si lavora. La posta
    /// intanto si legge e si conserva.
    pausa: Option<Instant>,
}

/// Le cartelle e non i percorsi, ormai sono decise; e da cartelle «nuove», che
/// si riguardano da sé — o da quella che le conteneva, se intanto sono sparite.
fn indietro(cartelle: &[String]) -> Vec<Cambiamento> {
    cartelle
        .iter()
        .map(|cartella| Cambiamento {
            percorso: cartella.clone(),
            nuovo: true,
        })
        .collect()
}

impl Motore {
    /// Un motore appena partito: niente letto, niente in attesa.
    fn nuovo(adesso: Instant) -> Self {
        Self {
            modo: Modo::default(),
            elenco: Vec::new(),
            cartelle: BTreeMap::new(),
            attesa: Accorpatore::default(),
            piano: Pianificatore::default(),
            // Il primo controllo dopo `ATTESA_AVVIO`; una riconfigurazione lo
            // anticipa.
            prossimo_controllo: adesso + ATTESA_AVVIO,
            configurata: false,
            chi_aspetta: Vec::new(),
            pausa: None,
        }
    }

    /// La scelta appena letta; `true` se è cambiata.
    ///
    /// Spenta, quel che aspettava un rinvio cade: è sorveglianza, e da spenta
    /// niente si riprova da sé. Senza, i rinviati continuavano a girare ogni
    /// [`SONDAGGIO`] anche a sorveglianza spenta — contro un NAS spento, per
    /// tutta la sessione. La raffica resta: vedi
    /// [`Accorpatore::svuota_rinviati`].
    fn imposta_modo(&mut self, modo: Modo) -> bool {
        let cambiato = modo != self.modo;
        self.modo = modo;
        if modo == Modo::Spenta {
            let caduti = self.attesa.svuota_rinviati();
            if caduti > 0 {
                nota!(
                    "[sorveglia] sorveglianza spenta: {caduti} cartelle in attesa di riprova lasciate stare"
                );
            }
        }
        cambiato
    }

    /// Rimanda `cartelle` di `quanto` — se la sorveglianza non è spenta.
    fn rinvia(&mut self, cartelle: Vec<Cambiamento>, adesso: Instant, quanto: Duration) {
        if self.modo == Modo::Spenta {
            return;
        }
        self.attesa
            .rinvia(cartelle, adesso, quanto, PathRules::for_current_platform());
    }

    /// I rami persi di una passata che ha guardato `guardate`: tornano in
    /// attesa fra poco, finché non sono troppe volte (vedi
    /// [`Accorpatore::rami`]). Spenta, niente.
    ///
    /// Non quelli sotto una delle `vuote`: la guardia per radice ha lasciato
    /// stare quella cartella sorvegliata per intero, e riguardarne un ramo da
    /// solo — una sottocartella, che la guardia per radice non protegge — la
    /// giudicherebbe a pezzi.
    fn rami_persi(
        &mut self,
        guardate: &[String],
        persi: &[String],
        vuote: &[String],
        adesso: Instant,
    ) {
        if self.modo == Modo::Spenta {
            return;
        }
        let regole = PathRules::for_current_platform();
        let persi: Vec<String> = persi
            .iter()
            .filter(|ramo| !vuote.iter().any(|vuota| is_under(ramo, vuota, regole)))
            .cloned()
            .collect();
        let lasciati = self.attesa.rami(guardate, &persi, adesso, regole);
        for ramo in lasciati {
            nota!(
                "[sorveglia] {ramo} non si legge da {RAMO_PERSO_TENTATIVI} tentativi: la si lascia stare, la riguarderanno i riallineamenti"
            );
        }
    }

    /// Il turno, e le cartelle da riguardare: in quest'ordine.
    ///
    /// Il turno prima: con «Scansiona» o la coda dei download in corso si
    /// riprova ogni [`RESPIRO`], e prima chiedere al disco cos'erano i percorsi
    /// voleva dire chiederlo ogni due secondi — sul NAS un giro di rete per
    /// cartella — per tutta la durata dell'altra scansione, e buttare la
    /// risposta. Il prezzo è il turno tenuto mentre si chiede: qualche
    /// millisecondo, al più [`NATURA_SCADENZA`] su una share che muore in quel
    /// momento, e chi lo vuole intanto aspetta.
    ///
    /// `None` col turno occupato — l'attesa resta com'è, e si riprova dopo un
    /// respiro — o con niente da guardare.
    fn prepara_la_riscansione<'a>(
        &mut self,
        stato: &'a Stato,
        adesso: Instant,
        natura: fn(&Path) -> Natura,
    ) -> Option<(TurnoDiScansione<'a>, Vec<String>)> {
        let Some(turno) = stato.turno_di_scansione() else {
            self.piano.aspetta_un_respiro(adesso);
            return None;
        };
        let cambiati = self.attesa.prendi(adesso);
        // Tutte le cartelle dell'elenco e non solo quelle guardate: un avviso
        // arriva solo da una cartella guardata, e una richiesta di [`riguarda`]
        // vale con qualunque scelta.
        let da_guardare = cartelle_da_guardare(
            cambiati,
            self.elenco.clone(),
            PathRules::for_current_platform(),
            natura,
        );
        if da_guardare.is_empty() {
            return None;
        }
        Some((turno, da_guardare))
    }

    /// Quel che resta da fare dopo il riallineamento di `radice`: dirlo al
    /// pianificatore, e rimettere quel che aspettava sotto di lei
    /// (`assorbiti`) se non l'ha guardato.
    ///
    /// - Fermato a metà: la cartella si riprova, gli assorbiti tornano in
    ///   raffica.
    /// - Non ha risposto: la cartella si riprova, gli assorbiti col rinvio.
    ///   Lo stesso `adesso` per tutti e due, così su un NAS spento il
    ///   riallineamento e i suoi rinviati scadono insieme, e il primo si porta
    ///   via i secondi: una sonda ogni [`SONDAGGIO`], non due.
    /// - Ha risposto vuota: per il pianificatore ha risposto (vedi
    ///   [`Pianificatore::fatto`]), e gli assorbiti non tornano: li riguarda il
    ///   suo prossimo riallineamento, che ogni cartella guardata ha.
    /// - Fallito: come non ha risposto, ma gli assorbiti fra [`CONTROLLO`].
    ///
    /// E in ogni caso i suoi rami persi tornano in attesa per conto loro.
    fn dopo_il_riallineamento(
        &mut self,
        radice: &str,
        assorbiti: Vec<Cambiamento>,
        passata: Passata,
        adesso: Instant,
    ) {
        match passata {
            Passata::Fatta { fermata: true, .. } => {
                nota!(
                    "[sorveglia] il riallineamento di {radice} si è fermato a metà: si riprova più tardi"
                );
                self.piano.fatto(radice, false, adesso);
                self.attesa.rimetti(assorbiti, adesso);
            }
            Passata::Fatta {
                saltate,
                vuote,
                rami_persi,
                ..
            } => {
                let risposto = saltate.iter().all(|saltata| vuote.contains(saltata));
                if !risposto {
                    nota!(
                        "[sorveglia] il riallineamento di {radice} non è andato in porto: si riprova più tardi"
                    );
                    self.rinvia(assorbiti, adesso, SONDAGGIO);
                } else if !vuote.is_empty() {
                    nota!(
                        "[sorveglia] {radice} risponde vuota: niente è stato tolto, la si riguarda al suo giro"
                    );
                }
                self.piano.fatto(radice, risposto, adesso);
                let guardate: Vec<String> = if saltate.is_empty() {
                    vec![radice.to_owned()]
                } else {
                    Vec::new()
                };
                self.rami_persi(&guardate, &rami_persi, &vuote, adesso);
            }
            Passata::Fallita => {
                nota!(
                    "[sorveglia] il riallineamento di {radice} non è andato in porto: si riprova più tardi"
                );
                self.piano.fatto(radice, false, adesso);
                self.rinvia(assorbiti, adesso, CONTROLLO);
            }
        }
    }

    /// Quel che resta da fare dopo la scansione di `da_guardare`.
    ///
    /// Quel che si è preso e non si è potuto guardare torna in attesa: con la
    /// scansione fermata a metà fra [`ACCORPAMENTO`]; con la scansione fallita
    /// fra [`CONTROLLO`]; con una cartella che non ha risposto fra
    /// [`SONDAGGIO`], allargata alla cartella sorvegliata che la contiene. Una
    /// sottocartella che non risponde è un disco spento, o una cartella sparita
    /// fra l'avviso e la camminata — un album spostato via per intero — e
    /// riprovarla identica, se è sparita, sarebbe un rinvio senza fine: la
    /// cartella sorvegliata intera è dove la si ritrova.
    ///
    /// Una cartella sorvegliata trovata vuota dalla guardia non torna: ha
    /// risposto, e la riguarda il suo riallineamento (vedi
    /// [`Pianificatore::fatto`]). I rami persi tornano per conto loro.
    fn dopo_la_riscansione(&mut self, da_guardare: &[String], passata: Passata, adesso: Instant) {
        let regole = PathRules::for_current_platform();
        self.piano.aspetta_un_respiro(adesso);
        match passata {
            Passata::Fatta { fermata: true, .. } => {
                self.attesa.rimetti(indietro(da_guardare), adesso);
            }
            Passata::Fatta {
                saltate,
                vuote,
                rami_persi,
                ..
            } => {
                for vuota in &vuote {
                    nota!(
                        "[sorveglia] {vuota} risponde vuota: niente è stato tolto, la si riguarda al suo giro"
                    );
                }
                let allargate = piu_esterne(
                    saltate
                        .iter()
                        .filter(|saltata| !vuote.contains(*saltata))
                        .filter_map(|saltata| {
                            self.elenco
                                .iter()
                                .find(|radice| is_under(saltata, radice, regole))
                                .cloned()
                        })
                        .collect(),
                    regole,
                );
                self.rinvia(indietro(&allargate), adesso, SONDAGGIO);
                let guardate: Vec<String> = da_guardare
                    .iter()
                    .filter(|cartella| !saltate.contains(*cartella))
                    .cloned()
                    .collect();
                self.rami_persi(&guardate, &rami_persi, &vuote, adesso);
            }
            Passata::Fallita => self.rinvia(indietro(da_guardare), adesso, CONTROLLO),
        }
    }
}

fn filo(
    app: &AppHandle,
    posta: &Sender<Messaggio>,
    orecchio: &Receiver<Messaggio>,
    dell_avvio: Option<Vec<String>>,
) {
    let mut motore = Motore::nuovo(Instant::now());
    if let Some(radici) = dell_avvio {
        motore.piano.fotografa(radici);
    }
    let mut cadute_di_fila = 0_u32;
    loop {
        if crate::spegnimento::in_uscita() {
            return;
        }
        let attesa = prossima_attesa(&motore, Instant::now());
        // Il sospeso si misura qui e solo qui: attorno all'attesa a vuoto, mai
        // attorno a una scansione. Vedi `Pianificatore::sveglio`.
        motore.piano.dormo(attesa, SystemTime::now());
        let ricevuto = orecchio.recv_timeout(attesa);
        if motore.piano.sveglio(SystemTime::now()) {
            nota!("[sorveglia] risveglio dal sospeso: le cartelle si riallineano");
        }
        match ricevuto {
            Ok(Messaggio::Fermati) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(messaggio) => ricevi(&mut motore, messaggio, Instant::now()),
            Err(RecvTimeoutError::Timeout) => {}
        }
        // Dopo una caduta la posta si legge lo stesso — il risveglio finito,
        // una richiesta di riguardare, chi aspetta un controllo: niente va
        // perso — e il lavoro aspetta la fine della pausa.
        if motore.pausa.is_some_and(|fino| Instant::now() < fino) {
            continue;
        }
        motore.pausa = None;
        // Ogni giro dentro `sorvegliato`: un panico — un percorso che il
        // sistema consegna storto, un parser sotto la scansione — costa un
        // giro, non la sorveglianza per il resto della sessione.
        let giro = crate::stato::sorvegliato("sorveglia", || lavora(app, posta, &mut motore));
        if giro.is_some() {
            cadute_di_fila = 0;
        } else {
            cadute_di_fila = cadute_di_fila.saturating_add(1);
            let pausa = crate::stato::attesa_dopo_la_caduta(cadute_di_fila).min(CONTROLLO);
            motore.pausa = Some(Instant::now() + pausa);
        }
    }
}

/// Fino a quando dormire: il primo fra il prossimo controllo, gli avvisi
/// pronti e il prossimo riallineamento.
fn prossima_attesa(motore: &Motore, adesso: Instant) -> Duration {
    // Mai un'attesa nulla in un ciclo: un decimo di secondo è il passo minimo.
    let minimo = Duration::from_millis(100);
    if let Some(fino) = motore.pausa {
        return fino.saturating_duration_since(adesso).max(minimo);
    }
    let mut attesa = motore.prossimo_controllo.saturating_duration_since(adesso);
    if motore.configurata
        && let Some(manca) = motore.attesa.manca(adesso)
    {
        // Gli avvisi pronti aspettano anche il respiro dopo l'ultima scansione.
        attesa = attesa.min(manca.max(motore.piano.fra_quanto_libero(adesso)));
    }
    if let Some(manca) = motore.piano.manca(adesso) {
        attesa = attesa.min(manca);
    }
    attesa.max(minimo)
}

/// Un messaggio arrivato: solo stato in memoria, niente disco.
fn ricevi(motore: &mut Motore, messaggio: Messaggio, adesso: Instant) {
    match messaggio {
        Messaggio::Avvisi(cambiati) => motore.attesa.avvisati(cambiati, adesso),
        Messaggio::Riguarda(cartelle) => motore.attesa.da_riguardare(cartelle, adesso),
        Messaggio::Guasto(radice) => {
            // Con `notify` 8.2 su Windows non arriva mai (vedi
            // `RIALLINEAMENTO_LOCALE`): quel che protegge davvero sono i
            // riallineamenti periodici. Se arriva, l'osservatore se ne va
            // subito, e si rifà al prossimo controllo e non adesso: un
            // osservatore che si guasta di continuo — una radice che il sistema
            // rifiuta — costa così un riallineamento al minuto, invece di un
            // giro senza fine fra guasto e riavvio. Rifatto, il cambio di passo
            // lo fa riallineare: quel che è successo intanto non l'ha visto
            // nessuno.
            nota!("[sorveglia] l'osservatore di {radice} si è guastato: si rifà e la si riallinea");
            if let Some(sorvegliata) = motore.cartelle.get_mut(&radice) {
                sorvegliata.osservatore = None;
                sorvegliata.come = Come::NonRaggiungibile;
                motore.piano.imposta(&radice, Ritmo::Nessuno, adesso);
            }
        }
        Messaggio::Riconfigura(chi) => {
            motore.chi_aspetta.extend(chi);
            motore.prossimo_controllo = adesso;
        }
        Messaggio::Svegliate => motore.piano.avvio(),
        Messaggio::Fermati => {}
    }
}

/// Un giro di lavoro: il controllo se è ora, e al più una scansione.
fn lavora(app: &AppHandle, posta: &Sender<Messaggio>, motore: &mut Motore) {
    if Instant::now() >= motore.prossimo_controllo {
        controlla(app, posta, motore);
        motore.prossimo_controllo = Instant::now() + CONTROLLO;
        for chi in motore.chi_aspetta.drain(..) {
            let _ = chi.send(());
        }
    }
    if !motore.configurata {
        return;
    }
    // Prima il riallineamento: guarda tutta la cartella, e quel che è in
    // attesa sotto di lei se lo porta via.
    if let Some(voce) = motore.piano.prossima(Instant::now()) {
        riallinea(app, posta, motore, voce);
        return;
    }
    // Gli avvisi aspettano il respiro ma non il risveglio delle cartelle di
    // rete: un album copiato sul disco di casa non ha motivo di aspettare che
    // il NAS si svegli, e quel che arriva da lì intanto se lo porterà via il
    // riallineamento dell'avvio. Con la stessa condizione di
    // `prossima_attesa`, se no il filo girerebbe a vuoto ogni decimo di
    // secondo fino al risveglio.
    let adesso = Instant::now();
    if motore.piano.fra_quanto_libero(adesso).is_zero() && motore.attesa.pronto(adesso) {
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
    let modo_cambiato = motore.imposta_modo(modo);
    // Via quel che non è più nell'elenco: l'osservatore cade con la voce.
    motore
        .cartelle
        .retain(|cartella, _| radici.contains(cartella));
    motore.piano.tieni_solo(&radici);
    if let Some(stato) = app.try_state::<StatoSorveglianza>() {
        stato.dove().retain(|(radice, _)| radici.contains(radice));
    }
    lascia_cadere_le_morte(motore);
    for radice in &radici {
        // Si rifà quel che è nuovo, quel che non si è potuto guardare, e — con
        // la scelta cambiata — tutto. Una cartella col solo riallineamento
        // ritenta l'osservatore a ogni controllo: senza, una cartella che
        // all'avvio non rispondeva — un disco che girava ancora a vuoto —
        // restava così per tutta la sessione anche dopo essere tornata normale.
        let rifare = motore.cartelle.get(radice).is_none_or(|sorvegliata| {
            modo_cambiato || matches!(sorvegliata.come, Come::NonRaggiungibile | Come::Sondaggio)
        });
        if !rifare {
            continue;
        }
        let prima = motore
            .cartelle
            .get(radice)
            .map(|sorvegliata| sorvegliata.come);
        let sorvegliata = arma(app, radice, modo, posta, prima);
        motore
            .piano
            .imposta(radice, sorvegliata.ritmo(), Instant::now());
        motore.cartelle.insert(radice.clone(), sorvegliata);
    }
    motore.elenco = radici;
    motore.configurata = true;
    motore.piano.elenco_letto();
    pubblica_quadro(app, motore);
}

/// Arma una cartella com'è giusto per la scelta: chiede dove sta, e se la si
/// vuole con gli avvisi prova l'osservatore.
///
/// Dove sta si chiede fuori da ogni lucchetto e con la scadenza: su una
/// lettera mappata che non risponde è un'attesa.
///
/// `prima` è come la si guardava finora: un osservatore che non si arma si
/// annota solo quando la cosa è nuova, se no un NAS spento per una giornata
/// scriverebbe la stessa riga ogni minuto.
fn arma(
    app: &AppHandle,
    radice: &str,
    modo: Modo,
    posta: &Sender<Messaggio>,
    prima: Option<Come>,
) -> Sorvegliata {
    let dove =
        rymembra_app::volumi::collocazione(radice, rymembra_app::volumi::SCADENZA_RISOLUZIONE);
    ricorda_dove(app, radice, dove);
    let (come, osservatore) = match come_sorvegliare(modo, dove) {
        Come::Avvisi => match osserva_entro(radice, posta) {
            Ok(osservatore) => (Come::Avvisi, Some(osservatore)),
            Err(guasto) => {
                // Con «tutte» resta il solo riallineamento: è la strada che
                // funziona anche dove gli avvisi no. Con «locali», dove questa
                // è per forza un disco di questo computer, si riprova al
                // prossimo controllo.
                let ripiego = if modo == Modo::Tutte {
                    Come::Sondaggio
                } else {
                    Come::NonRaggiungibile
                };
                if prima != Some(ripiego) {
                    nota!("[sorveglia] {radice} non si può osservare: {guasto}");
                }
                (ripiego, None)
            }
        },
        altro => (altro, None),
    };
    Sorvegliata {
        come,
        dove,
        osservatore,
        senza_risposta: 0,
    }
}

/// Scrive il quadro per la finestra.
fn pubblica_quadro(app: &AppHandle, motore: &Motore) {
    let Some(stato) = app.try_state::<StatoSorveglianza>() else {
        return;
    };
    let mut quadro = stato.quadro();
    quadro.modo = motore.modo.come_testo().to_owned();
    quadro.cartelle = motore
        .elenco
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

/// Le cartelle con gli avvisi che non ci sono più, e quelle di rete che
/// tornano a rispondere.
///
/// # Perché bisogna andarle a cercare
///
/// Perché il loro osservatore muore **in silenzio**. In `notify` 8.2 su Windows
/// un completamento fallito di `ReadDirectoryChangesW` — il disco USB staccato,
/// la lettera che sparisce, la rete che cade — toglie la sorveglianza e lo
/// scrive in un registro che Rymembra non ascolta: il gestore qui sotto non
/// riceve nessun errore, il ramo `Guasto` non scatta, e la cartella resta «con
/// gli avvisi» senza che nessun avviso arrivi più.
///
/// Una domanda a ogni [`CONTROLLO`] — «c'è ancora?» — con [`VIVA_SCADENZA`].
///
/// - Un «non c'è» detto dal sistema fa cadere l'osservatore. Quando la
///   cartella torna, [`controlla`] la riarma, e il cambio di passo la fa
///   riallineare.
/// - Un «non si sa» (un disco che si sta svegliando, una rete giù) lo lascia
///   stare: farlo cadere e rifarlo costerebbe un riallineamento per un disco
///   lento. Ma una cartella di rete che è rimasta muta per
///   [`MANCATE_PER_TORNARE`] domande di fila e poi risponde è tornata: il suo
///   osservatore è con ogni probabilità morto con la rete, e la si riarma e
///   riallinea subito invece che al prossimo giro di [`RIALLINEAMENTO`].
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
    let adesso = Instant::now();
    for (radice, risposta) in con_avvisi.iter().zip(risposte) {
        let Some(sorvegliata) = motore.cartelle.get_mut(radice) else {
            continue;
        };
        match risposta {
            Esistenza::Assente => {
                nota!("[sorveglia] {radice} non c'è più: la si riarma quando torna");
                sorvegliata.osservatore = None;
                sorvegliata.come = Come::NonRaggiungibile;
                sorvegliata.senza_risposta = 0;
                motore.piano.imposta(radice, Ritmo::Nessuno, adesso);
            }
            Esistenza::Ignota => {
                sorvegliata.senza_risposta = sorvegliata.senza_risposta.saturating_add(1);
            }
            Esistenza::Presente => {
                if sorvegliata.senza_risposta >= MANCATE_PER_TORNARE
                    && sorvegliata.ritmo() == Ritmo::Rete
                {
                    nota!("[sorveglia] {radice} risponde di nuovo: la si riarma e riallinea");
                    motore.piano.tornata(radice);
                }
                sorvegliata.senza_risposta = 0;
            }
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
                // vede. Questo e l'`Err` qui sotto, con `notify` 8.2 su
                // Windows, non arrivano mai: l'osservatore muore senza dirlo,
                // e lo rifanno i riallineamenti periodici (vedi
                // `RIALLINEAMENTO_LOCALE`). Restano per chi lo dicesse.
                Ok(evento) if evento.need_rescan() => Messaggio::Riguarda(vec![nome.clone()]),
                Ok(evento) => {
                    let cambiati = cambiamenti_dell_avviso(&evento, &natura_di);
                    if cambiati.is_empty() {
                        return;
                    }
                    Messaggio::Avvisi(cambiati)
                }
                Err(_) => Messaggio::Guasto(nome.clone()),
            };
            let _ = posta.send(messaggio);
        })?;
    osservatore.watch(Path::new(radice), notify::RecursiveMode::Recursive)?;
    Ok(osservatore)
}

/// Un riallineamento: l'osservatore rifatto se serve, e la cartella intera
/// scansionata, se il turno è libero.
fn riallinea(
    app: &AppHandle,
    posta: &Sender<Messaggio>,
    motore: &mut Motore,
    voce: Riallineamento,
) {
    let Some(stato) = app.try_state::<Stato>() else {
        return;
    };
    let radice = voce.radice;
    if voce.riarma && motore.cartelle.contains_key(&radice) {
        // Il nuovo prima della camminata — vedi `Riallineamento::riarma` — e
        // il vecchio cade quando il nuovo prende il suo posto. Se il nuovo non
        // si arma, la cartella passa al solo riallineamento, e si cammina lo
        // stesso.
        let prima = motore.cartelle.get(&radice).map(|vecchia| vecchia.come);
        let nuova = arma(app, &radice, motore.modo, posta, prima);
        let ritmo = nuova.ritmo();
        motore.piano.imposta(&radice, ritmo, Instant::now());
        let cambiata = motore
            .cartelle
            .get(&radice)
            .is_none_or(|vecchia| vecchia.come != nuova.come);
        motore.cartelle.insert(radice.clone(), nuova);
        if cambiata {
            pubblica_quadro(app, motore);
        }
        if !ritmo.guardata() {
            return;
        }
    }
    let Some(turno) = stato.turno_di_scansione() else {
        // Il turno è di qualcun altro: la cartella resta dovuta, e si riprova
        // dopo un respiro.
        motore.piano.aspetta_un_respiro(Instant::now());
        return;
    };
    if crate::spegnimento::in_uscita() {
        return;
    }
    stato.riprendi_scansioni();
    // Quel che aspettava sotto questa cartella lo guarda il riallineamento:
    // va via adesso, prima della camminata, e torna se la camminata non va in
    // porto (vedi `Motore::dopo_il_riallineamento`). Quel che arriva durante
    // resta in attesa per conto suo.
    let assorbiti = motore
        .attesa
        .togli_sotto(&radice, PathRules::for_current_platform());
    let passata = esegui(
        app,
        &stato,
        std::slice::from_ref(&radice),
        turno,
        "riallineamento",
    );
    motore.dopo_il_riallineamento(&radice, assorbiti, passata, Instant::now());
}

/// La scansione delle cartelle in attesa, se il turno è libero.
///
/// Il turno si chiede **prima** di decidere le cartelle, e col turno occupato
/// non si tocca né l'attesa né il disco: vedi
/// [`Motore::prepara_la_riscansione`]. Quel che si è preso e non si è potuto
/// guardare torna in attesa come dice [`Motore::dopo_la_riscansione`].
fn riscansiona(app: &AppHandle, motore: &mut Motore) {
    let Some(stato) = app.try_state::<Stato>() else {
        return;
    };
    let Some((turno, da_guardare)) =
        motore.prepara_la_riscansione(&stato, Instant::now(), natura_di)
    else {
        return;
    };
    if crate::spegnimento::in_uscita() {
        motore
            .attesa
            .rimetti(indietro(&da_guardare), Instant::now());
        return;
    }
    stato.riprendi_scansioni();
    let passata = esegui(app, &stato, &da_guardare, turno, "sorveglianza");
    motore.dopo_la_riscansione(&da_guardare, passata, Instant::now());
}

/// Com'è andata una scansione automatica.
enum Passata {
    /// È arrivata in fondo, o si è fermata a metà perché qualcuno l'ha chiesto.
    Fatta {
        /// Le cartelle che non hanno risposto: niente di loro è stato giudicato.
        /// Comprende le `vuote`.
        saltate: Vec<String>,
        /// Fra le `saltate`, le cartelle sorvegliate che hanno risposto vuote
        /// e la guardia per radice ha lasciato stare: hanno risposto, e non si
        /// riprovano come quelle che non l'hanno fatto.
        vuote: Vec<String>,
        /// Le sottocartelle che non si sono lette, dentro cartelle che
        /// rispondevano: il resto è stato giudicato, loro no.
        rami_persi: Vec<String>,
        /// Si è fermata a metà: quel che ha fatto è scritto, il resto no.
        fermata: bool,
    },
    /// Non è partita, o è fallita: niente di quel che doveva guardare si può
    /// dare per guardato.
    Fallita,
}

/// La scansione vera, con il turno già in mano. Il turno cade alla fine, e
/// sveglia la manutenzione se la scansione ha cambiato qualcosa.
fn esegui(
    app: &AppHandle,
    stato: &Stato,
    cartelle: &[String],
    turno: TurnoDiScansione<'_>,
    origine: &'static str,
) -> Passata {
    let copertine = match stato.copertine() {
        Ok(copertine) => copertine,
        Err(guasto) => {
            nota!("[sorveglia] la scansione non parte: {guasto}");
            return Passata::Fallita;
        }
    };
    let fermati = || stato.scansione_fermata() || crate::spegnimento::in_uscita();
    // Le cartelle sorvegliate, per la guardia per radice: vale su quelle e non
    // sulle sottocartelle che si riguardano qui. Se non si leggono, tutte le
    // cartelle di questa passata valgono sorvegliate — la guardia scatta di
    // più, e nel dubbio non si toglie.
    let radici_della_libreria: Vec<String> = con_libreria(stato, |l| {
        rymembra_app::settings::read_json::<Vec<String>>(
            &l.connection,
            rymembra_app::settings::CHIAVE_CARTELLE,
        )
    })
    .ok()
    .flatten()
    .unwrap_or_else(|| cartelle.to_vec());
    let scan = Scan {
        files: std::sync::Arc::new(rymembra_app::files::LocalFiles),
        covers: copertine,
        roots: cartelle,
        radici_della_libreria: &radici_della_libreria,
        rules: PathRules::for_current_platform(),
        // Nessuno la sta guardando: vedi il preambolo del modulo.
        prudente: true,
        scadenze: Scadenze::default(),
        fermati: Some(&fermati),
    };
    let mut deposito = DepositoCheGuarda {
        dentro: DepositoStato::nuovo(stato),
        scritto: false,
    };
    let report = match scan.run_su(&mut deposito, |_, _| std::ops::ControlFlow::Continue(())) {
        Ok(report) => report,
        Err(guasto) => {
            nota!(
                "[sorveglia] la scansione di {} cartelle ({origine}) è fallita: {guasto}",
                cartelle.len()
            );
            // Fallita dopo aver scritto: i lotti chiusi sono in libreria anche
            // se l'errore non porta i conti, e la finestra deve rileggerli.
            if deposito.scritto {
                libreria_cambiata(app, &turno);
            }
            return Passata::Fallita;
        }
    };
    crate::comandi::racconta_scansione(origine, &report);
    // Le vuote stanno anche fra le saltate, ma hanno risposto: le annuncia
    // chi decide se riprovarle.
    let mute = report
        .radici_saltate
        .len()
        .saturating_sub(report.radici_vuote.len());
    if mute > 0 {
        nota!(
            "[sorveglia] {mute} cartelle non hanno risposto, i loro brani non sono stati giudicati"
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
    let cambiato = report
        .inserted
        .saturating_add(aggiornati)
        .saturating_add(report.removed)
        > 0;
    if cambiato {
        libreria_cambiata(app, &turno);
    }
    Passata::Fatta {
        saltate: report.radici_saltate,
        vuote: report.radici_vuote,
        rami_persi: report.rami_persi,
        fermata: report.cancelled,
    }
}

/// Quel che si fa quando una scansione automatica ha cambiato la libreria.
///
/// Il turno lo sa — cadendo sveglierà la manutenzione — e la finestra riceve
/// **un** evento: `cartelle:cambiate`, che le fa rileggere viste e albero.
/// Fino alla 3.0.1 ne riceveva due, perché si mandava anche quello dei
/// download (`scarico:in_libreria`), che resta ai download. Poi i brani nuovi
/// vanno arricchiti, misurati e salvati. Una scansione che non ha cambiato
/// niente non sveglia nessuno di questi.
fn libreria_cambiata(app: &AppHandle, turno: &TurnoDiScansione<'_>) {
    turno.segna_cambiamenti();
    crate::cartelle::cambiate(app);
    crate::arricchimento::sporca(app);
    crate::analisi::sporca(app);
    crate::nuvola::sporca(app);
}

/// Il deposito della libreria, che in più si accorge se qualcosa è stato
/// scritto.
///
/// Serve al caso in cui la scansione fallisce **dopo** aver scritto: l'errore
/// non porta i conti, ma i lotti già chiusi sono in libreria. Si guarda il
/// contatore delle modifiche di SQLite prima e dopo ogni presa, con il
/// lucchetto in mano: conta solo quel che ha scritto la scansione, non gli
/// altri che usano la connessione fra una presa e l'altra.
struct DepositoCheGuarda<'a> {
    /// Il deposito vero.
    dentro: DepositoStato<'a>,
    /// Almeno una presa ha cambiato qualcosa.
    scritto: bool,
}

impl Deposito for DepositoCheGuarda<'_> {
    fn con_connessione<T, F>(&mut self, azione: F) -> Result<T, AppError>
    where
        F: FnOnce(&mut rusqlite::Connection) -> Result<T, AppError>,
    {
        let mut scritto = false;
        let esito = self.dentro.con_connessione(|connessione| {
            let prima = connessione.total_changes();
            let esito = azione(connessione);
            scritto = connessione.total_changes() != prima;
            esito
        });
        self.scritto |= scritto;
        esito
    }
}

#[cfg(test)]
mod prove {
    use super::*;
    use std::path::PathBuf;

    /// Un cambiamento nuovo: creato, o una cartella da riguardare.
    fn nuovo(percorso: &str) -> Cambiamento {
        Cambiamento {
            percorso: percorso.to_owned(),
            nuovo: true,
        }
    }

    /// Un cambiamento che si guarda dalla cartella che lo contiene.
    fn vecchio(percorso: &str) -> Cambiamento {
        Cambiamento {
            percorso: percorso.to_owned(),
            nuovo: false,
        }
    }

    #[test]
    fn molti_avvisi_fanno_una_scansione_sola() {
        let zero = Instant::now();
        let mut attesa = Accorpatore::default();
        assert_eq!(attesa.manca(zero), None);
        // Una copia: venti file, uno ogni cento millisecondi.
        for n in 0..20_u64 {
            let quando = zero + Duration::from_millis(100 * n);
            attesa.avvisati([nuovo(&format!(r"C:\M\A\{n:02}.flac"))], quando);
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
        let cambiati = attesa.prendi(ultimo + ACCORPAMENTO);
        assert_eq!(cambiati.len(), 20, "tutti in una volta sola");
        assert_eq!(cambiati.first(), Some(&nuovo(r"C:\M\A\00.flac")));
        assert_eq!(attesa.manca(ultimo), None, "e l'attesa ricomincia da vuota");
        assert!(!attesa.pronto(ultimo + ACCORPAMENTO * 10));
        assert!(attesa.prendi(ultimo + ACCORPAMENTO * 10).is_empty());
    }

    #[test]
    fn una_raffica_senza_fine_non_rimanda_per_sempre() {
        let zero = Instant::now();
        let mut attesa = Accorpatore::default();
        let mut quando = zero;
        while quando < zero + ATTESA_MASSIMA {
            attesa.avvisati([vecchio("x")], quando);
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
        attesa.avvisati([vecchio("a")], zero);
        let dopo = zero + ATTESA_MASSIMA + ACCORPAMENTO;
        assert!(attesa.pronto(dopo));
        let cambiati = attesa.prendi(dopo);
        attesa.rimetti(cambiati, dopo);
        assert!(
            !attesa.pronto(dopo),
            "non si gira a vuoto sul turno occupato"
        );
        assert_eq!(attesa.manca(dopo), Some(ACCORPAMENTO));
        assert!(attesa.pronto(dopo + ACCORPAMENTO));
        assert_eq!(attesa.prendi(dopo + ACCORPAMENTO), vec![vecchio("a")]);
        // Rimettere niente non accende nessuna attesa.
        let mut vuota = Accorpatore::default();
        vuota.rimetti(Vec::new(), dopo);
        vuota.rinvia(
            Vec::new(),
            dopo,
            SONDAGGIO,
            PathRules::for_current_platform(),
        );
        assert_eq!(vuota.manca(dopo), None);
    }

    #[test]
    fn quel_che_non_ha_risposto_torna_col_suo_rinvio_senza_fermare_il_resto() {
        let regole = PathRules {
            case_insensitive: true,
        };
        let zero = Instant::now();
        let mut attesa = Accorpatore::default();
        attesa.rinvia(vec![nuovo(r"\\nas\m")], zero, SONDAGGIO, regole);
        assert!(
            !attesa.pronto(zero + ACCORPAMENTO * 10),
            "un NAS spento non si riprova a ogni raffica"
        );
        assert_eq!(attesa.manca(zero), Some(SONDAGGIO));
        // Un album copiato sul disco di casa intanto non aspetta il rinvio, e
        // il rinvio non parte con lui.
        attesa.avvisati([vecchio(r"C:\M\A\01.flac")], zero);
        let silenzio = zero + ACCORPAMENTO;
        assert!(attesa.pronto(silenzio));
        assert_eq!(attesa.prendi(silenzio), vec![vecchio(r"C:\M\A\01.flac")]);
        assert_eq!(attesa.manca(silenzio), Some(SONDAGGIO - ACCORPAMENTO));
        // Un minuto dopo anche il disco di casa non risponde: torna indietro.
        let minuto = zero + Duration::from_secs(60);
        attesa.rinvia(vec![nuovo(r"C:\M")], minuto, SONDAGGIO, regole);
        // Il NAS scade per primo, da solo, e torna indietro ancora spento.
        assert_eq!(attesa.prendi(zero + SONDAGGIO), vec![nuovo(r"\\nas\m")]);
        let di_nuovo = zero + SONDAGGIO + Duration::from_secs(3);
        attesa.rinvia(vec![nuovo(r"\\nas\m")], di_nuovo, SONDAGGIO, regole);
        // Il disco di casa non si sposta: ognuno il suo rinvio. Con un rinvio
        // per tutti, qui avrebbe aspettato il NAS — e ogni cinque minuti di
        // nuovo, per sempre.
        assert_eq!(attesa.manca(di_nuovo), Some(minuto + SONDAGGIO - di_nuovo));
        assert_eq!(attesa.prendi(minuto + SONDAGGIO), vec![nuovo(r"C:\M")]);
        // Lo stesso percorso due volte è una voce sola, col rinvio più lontano.
        attesa.rinvia(vec![nuovo(r"\\NAS\m")], di_nuovo, CONTROLLO, regole);
        assert!(attesa.prendi(di_nuovo + CONTROLLO).is_empty());
        assert_eq!(attesa.prendi(di_nuovo + SONDAGGIO), vec![nuovo(r"\\nas\m")]);
        assert_eq!(attesa.manca(di_nuovo + SONDAGGIO), None);
    }

    #[test]
    fn un_riallineamento_si_porta_via_quel_che_aspetta_sotto_la_sua_radice() {
        let zero = Instant::now();
        let regole = PathRules {
            case_insensitive: true,
        };
        let mut attesa = Accorpatore::default();
        attesa.avvisati(
            [vecchio(r"\\nas\m\A\01.flac"), vecchio(r"C:\M\B\02.flac")],
            zero,
        );
        attesa.rinvia(vec![nuovo(r"\\NAS\m\C")], zero, SONDAGGIO, regole);
        assert_eq!(
            attesa.togli_sotto(r"\\nas\m", regole),
            vec![vecchio(r"\\nas\m\A\01.flac"), nuovo(r"\\NAS\m\C")]
        );
        assert_eq!(
            attesa.manca(zero),
            Some(ACCORPAMENTO),
            "il resto aspetta come prima"
        );
        assert_eq!(
            attesa.prendi(zero + ACCORPAMENTO),
            vec![vecchio(r"C:\M\B\02.flac")]
        );
        assert_eq!(
            attesa.manca(zero + ACCORPAMENTO),
            None,
            "e un rinvio vuoto non resta acceso"
        );
        // Una radice sotto cui non aspetta niente non porta via niente.
        attesa.avvisati([vecchio(r"C:\M\B\03.flac")], zero);
        assert!(attesa.togli_sotto(r"\\nas\m", regole).is_empty());
        assert_eq!(attesa.manca(zero), Some(ACCORPAMENTO));
    }

    #[test]
    fn le_copertine_e_i_testi_non_valgono_una_scansione() {
        assert!(conta(Path::new(r"C:\M\A\01.flac")));
        assert!(conta(Path::new(r"C:\M\A")), "una cartella sì");
        assert!(
            conta(Path::new(r"C:\M\Vol. 2")),
            "anche con un punto nel nome"
        );
        assert!(
            conta(Path::new(r"C:\M\A\Album.CUE")),
            "il foglio di un'immagine CD cambia i suoi brani"
        );
        assert!(conta(Path::new(r"C:\M\A\CDImage.ape")));
        assert!(!conta(Path::new(r"C:\M\A\cover.JPG")));
        assert!(!conta(Path::new(r"C:\M\A\01.lrc")));
        assert!(!conta(Path::new(r"C:\M\A\01.flac.part")));
    }

    #[test]
    fn gli_avvisi_diventano_cambiamenti() {
        use notify::event::{AccessKind, RenameMode};
        use notify::event::{CreateKind, DataChange, MetadataKind, ModifyKind, RemoveKind};
        use notify::{Event, EventKind};
        // Un disco finto: «Album» è una cartella, tutto il resto un file.
        let natura = |percorso: &Path| {
            if percorso.to_string_lossy().ends_with("Album") {
                Natura::Cartella
            } else {
                Natura::File
            }
        };
        let avviso = |genere: EventKind, percorsi: &[&str]| {
            let evento = percorsi
                .iter()
                .fold(Event::new(genere), |evento, percorso| {
                    evento.add_path(PathBuf::from(percorso))
                });
            cambiamenti_dell_avviso(&evento, &natura)
        };
        let album = r"\\nas\m\Guccini\Album";
        let brano = r"\\nas\m\Guccini\Album\01.flac";

        // Una cartella creata è nuova: si guarderà da sé, non dall'artista.
        assert_eq!(
            avviso(EventKind::Create(CreateKind::Any), &[album]),
            vec![nuovo(album)]
        );
        // Anche rinominata verso; il nome di prima no.
        assert_eq!(
            avviso(
                EventKind::Modify(ModifyKind::Name(RenameMode::To)),
                &[album]
            ),
            vec![nuovo(album)]
        );
        assert_eq!(
            avviso(
                EventKind::Modify(ModifyKind::Name(RenameMode::From)),
                &[r"\\nas\m\Guccini\Vecchio"]
            ),
            vec![vecchio(r"\\nas\m\Guccini\Vecchio")]
        );
        assert_eq!(
            avviso(
                EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                &[r"\\nas\m\Guccini\Vecchio", album]
            ),
            vec![vecchio(r"\\nas\m\Guccini\Vecchio"), nuovo(album)]
        );
        assert_eq!(
            avviso(EventKind::Remove(RemoveKind::Any), &[album]),
            vec![vecchio(album)]
        );
        // La modifica di una cartella si salta: l'ha cambiata un figlio, che
        // ha il suo avviso. È quel che il NAS manda a ogni file copiato.
        for modifica in [
            ModifyKind::Any,
            ModifyKind::Metadata(MetadataKind::WriteTime),
            ModifyKind::Data(DataChange::Any),
        ] {
            assert!(
                avviso(EventKind::Modify(modifica), &[album]).is_empty(),
                "{modifica:?}"
            );
        }
        // La modifica di un file no.
        assert_eq!(
            avviso(EventKind::Modify(ModifyKind::Any), &[brano]),
            vec![vecchio(brano)]
        );
        // Il foglio di un'immagine conta; la copertina e gli accessi no.
        let foglio = r"\\nas\m\Guccini\Album\Album.cue";
        assert_eq!(
            avviso(EventKind::Modify(ModifyKind::Any), &[foglio]),
            vec![vecchio(foglio)]
        );
        assert!(
            avviso(
                EventKind::Create(CreateKind::File),
                &[r"\\nas\m\Guccini\Album\cover.jpg"]
            )
            .is_empty()
        );
        assert!(avviso(EventKind::Access(AccessKind::Any), &[brano]).is_empty());
    }

    #[test]
    fn la_natura_di_un_percorso() {
        let cartella = tempfile::tempdir().expect("cartella");
        let dentro = |nome: &str| cartella.path().join(nome);
        std::fs::create_dir(dentro("Album")).expect("cartella");
        std::fs::write(dentro("note.txt"), b"x").expect("file");
        assert_eq!(natura_di(&dentro("Album")), Natura::Cartella);
        assert_eq!(natura_di(&dentro("note.txt")), Natura::File);
        assert_eq!(natura_di(&dentro("manca")), Natura::Assente);
        // Un brano o un foglio si riconoscono dal nome, senza chiedere: questo
        // non esiste, e il disco avrebbe risposto «assente».
        assert_eq!(natura_di(&dentro("manca.flac")), Natura::File);
        assert_eq!(natura_di(&dentro("manca.cue")), Natura::File);
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
        // Con «tutte» gli avvisi si vogliono ovunque, rete compresa: è il
        // filo a ripiegare sul solo riallineamento se non si armano.
        for dove in [Locale, DiRete, Ignota] {
            assert_eq!(come_sorvegliare(Modo::Tutte, dove), Come::Avvisi);
        }
        // Di serie, tutte; un nome sconosciuto non spegne niente; chi aveva
        // scelto resta com'era.
        assert_eq!(Modo::dal_salvato(None), Modo::Tutte);
        assert_eq!(Modo::dal_salvato(Some("domani")), Modo::Tutte);
        assert_eq!(Modo::dal_salvato(Some("locali")), Modo::Locali);
        assert_eq!(Modo::dal_salvato(Some("spenta")), Modo::Spenta);
        for modo in [Modo::Spenta, Modo::Locali, Modo::Tutte] {
            assert_eq!(Modo::da_testo(modo.come_testo()), Some(modo));
        }
        // E il passo che ne viene.
        assert_eq!(Ritmo::di(Come::Avvisi, Locale), Ritmo::Locale);
        assert_eq!(Ritmo::di(Come::Avvisi, DiRete), Ritmo::Rete);
        assert_eq!(
            Ritmo::di(Come::Avvisi, Ignota),
            Ritmo::Rete,
            "una lettera che non si sa dove porti si tratta da rete"
        );
        assert_eq!(
            Ritmo::di(Come::Sondaggio, DiRete),
            Ritmo::SoloRiallineamento
        );
        assert_eq!(Ritmo::di(Come::Ferma, DiRete), Ritmo::Nessuno);
        assert_eq!(Ritmo::di(Come::NonRaggiungibile, Locale), Ritmo::Nessuno);
    }

    // ── il pianificatore ──

    const NAS: &str = r"\\nas\musica";
    const CASA: &str = r"C:\Musica";
    const USB: &str = r"E:\Musica";

    /// Un pianificatore con l'elenco dell'avvio letto e il risveglio finito.
    fn piano_avviato(radici: &[(&str, Ritmo)], adesso: Instant) -> Pianificatore {
        let mut piano = Pianificatore::default();
        for (radice, ritmo) in radici {
            piano.imposta(radice, *ritmo, adesso);
        }
        piano.elenco_letto();
        piano.avvio();
        piano
    }

    /// Fa tutti i riallineamenti dovuti da `adesso`, uno per respiro, come il
    /// filo, e dice quali.
    fn fai_tutti(piano: &mut Pianificatore, adesso: Instant) -> Vec<Riallineamento> {
        let mut fatti = Vec::new();
        let mut quando = adesso;
        while let Some(voce) = piano.prossima(quando) {
            piano.fatto(&voce.radice, true, quando);
            fatti.push(voce);
            quando += RESPIRO;
            assert!(fatti.len() < 20, "un giro senza fine: {fatti:?}");
        }
        fatti
    }

    fn voce(radice: &str, riarma: bool) -> Riallineamento {
        Riallineamento {
            radice: radice.to_owned(),
            riarma,
        }
    }

    #[test]
    fn all_avvio_si_riallinea_dopo_il_risveglio_delle_cartelle() {
        let zero = Instant::now();
        let mut piano = Pianificatore::default();
        piano.imposta(NAS, Ritmo::Rete, zero);
        piano.imposta(CASA, Ritmo::Locale, zero);
        piano.imposta(r"\\nas\spenta", Ritmo::Nessuno, zero);
        piano.elenco_letto();
        assert_eq!(
            piano.prossima(zero),
            None,
            "il NAS non ha ancora finito di svegliarsi"
        );
        assert_eq!(piano.manca(zero), None);
        piano.avvio();
        assert_eq!(piano.manca(zero), Some(Duration::ZERO));
        assert_eq!(
            fai_tutti(&mut piano, zero),
            vec![voce(CASA, false), voce(NAS, false)],
            "una volta ciascuna, e non quella che non si guarda"
        );
        // Il risveglio può anche finire prima che l'elenco sia letto.
        let mut presto = Pianificatore::default();
        presto.avvio();
        presto.imposta(NAS, Ritmo::Rete, zero);
        presto.elenco_letto();
        assert_eq!(presto.prossima(zero), Some(voce(NAS, false)));
    }

    #[test]
    fn una_cartella_aggiunta_dopo_l_avvio_non_si_riallinea_al_primo_armo() {
        let zero = Instant::now();
        let mut piano = piano_avviato(&[(CASA, Ritmo::Locale)], zero);
        fai_tutti(&mut piano, zero);
        // Chi l'aggiunge la scansiona: una lettura intera partita da sola gli
        // terrebbe il turno.
        piano.imposta(NAS, Ritmo::Rete, zero);
        assert_eq!(piano.prossima(zero + RESPIRO), None);
        // Il suo periodico però sì.
        assert_eq!(piano.prossima(zero + RIALLINEAMENTO), Some(voce(NAS, true)));
    }

    #[test]
    fn un_salto_dell_orologio_conta_solo_sull_attesa() {
        let zero = Instant::now();
        let muro = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let minuto = Duration::from_secs(60);
        let mut piano = piano_avviato(&[(NAS, Ritmo::Rete), (CASA, Ritmo::Locale)], zero);
        fai_tutti(&mut piano, zero);

        // Un'attesa di un minuto che dura un minuto.
        piano.dormo(minuto, muro);
        assert!(!piano.sveglio(muro + minuto));
        // Poi una scansione di dieci minuti, che il muro vede passare ma che
        // non sta dentro nessuna attesa: la misura ricomincia dopo.
        let dopo_la_scansione = muro + minuto * 11;
        piano.dormo(minuto, dopo_la_scansione);
        assert!(
            !piano.sveglio(dopo_la_scansione + minuto),
            "una scansione lunga non è un sonno"
        );
        assert_eq!(piano.prossima(zero + RESPIRO * 2), None);
        // Fino al margine non è un sonno; oltre sì.
        piano.dormo(minuto, muro);
        assert!(!piano.sveglio(muro + minuto * 2 + SONNO_MARGINE));
        // Svegliarsi senza essersi addormentati non misura niente.
        assert!(!piano.sveglio(muro + minuto * 1_000));
        // Un orologio portato indietro non è un sonno.
        piano.dormo(minuto, muro);
        assert!(!piano.sveglio(muro - minuto * 60));

        // Un minuto d'attesa che sul muro dura due ore: il computer ha dormito.
        piano.dormo(minuto, muro);
        assert!(piano.sveglio(muro + minuto * 120));
        assert_eq!(
            fai_tutti(&mut piano, zero + RESPIRO * 2),
            vec![voce(CASA, true), voce(NAS, true)],
            "tutte riallineate, e ogni osservatore rifatto prima — anche il locale"
        );
    }

    #[test]
    fn una_cartella_di_rete_tornata_si_riarma_e_si_riallinea() {
        let zero = Instant::now();
        let mut piano = piano_avviato(
            &[
                (NAS, Ritmo::Rete),
                (CASA, Ritmo::Locale),
                (r"\\nas\ferma", Ritmo::Nessuno),
            ],
            zero,
        );
        fai_tutti(&mut piano, zero);
        let poi = zero + RESPIRO * 2;
        piano.tornata(NAS);
        piano.tornata(r"\\nas\ferma");
        piano.tornata(r"\\nas\sconosciuta");
        assert_eq!(piano.prossima(poi), Some(voce(NAS, true)));
        // Il riarmo è fatto: resta la camminata, senza un secondo riarmo.
        piano.imposta(NAS, Ritmo::Rete, poi);
        assert_eq!(piano.prossima(poi), Some(voce(NAS, false)));
        piano.fatto(NAS, true, poi);
        assert_eq!(piano.prossima(poi + RESPIRO), None);
    }

    #[test]
    fn le_cartelle_di_rete_si_riallineano_ogni_mezz_ora_riarmando_prima() {
        let zero = Instant::now();
        let mut piano = piano_avviato(&[(NAS, Ritmo::Rete)], zero);
        fai_tutti(&mut piano, zero);
        let un_attimo = Duration::from_secs(1);
        assert_eq!(piano.prossima(zero + RIALLINEAMENTO - un_attimo), None);
        assert_eq!(
            piano.manca(zero + RESPIRO * 2),
            Some(RIALLINEAMENTO - RESPIRO * 2)
        );
        let mezz_ora = zero + RIALLINEAMENTO;
        assert_eq!(piano.prossima(mezz_ora), Some(voce(NAS, true)));
        // Il turno è occupato: niente per un respiro, e niente riarmo doppio.
        piano.imposta(NAS, Ritmo::Rete, mezz_ora);
        piano.aspetta_un_respiro(mezz_ora);
        assert_eq!(piano.prossima(mezz_ora), None);
        assert_eq!(piano.prossima(mezz_ora + RESPIRO), Some(voce(NAS, false)));
        piano.fatto(NAS, true, mezz_ora + RESPIRO);
        assert_eq!(
            piano.prossima(mezz_ora + RESPIRO + RIALLINEAMENTO),
            Some(voce(NAS, true)),
            "e di nuovo fra mezz'ora"
        );
    }

    #[test]
    fn anche_le_cartelle_locali_hanno_una_rete_di_sicurezza() {
        // `notify` 8.2 non dice quando un osservatore locale muore — il buffer
        // che trabocca durante una copia enorme — e fino a qui il disco di
        // casa si riallineava solo all'avvio: un'ora, e l'osservatore rifatto.
        let zero = Instant::now();
        let un_attimo = Duration::from_secs(1);
        let mut casa = piano_avviato(&[(CASA, Ritmo::Locale)], zero);
        assert_eq!(fai_tutti(&mut casa, zero), vec![voce(CASA, false)]);
        assert_eq!(
            casa.manca(zero + RESPIRO),
            Some(RIALLINEAMENTO_LOCALE - RESPIRO)
        );
        assert_eq!(
            casa.prossima(zero + RIALLINEAMENTO_LOCALE - un_attimo),
            None
        );
        let un_ora = zero + RIALLINEAMENTO_LOCALE;
        assert_eq!(casa.prossima(un_ora), Some(voce(CASA, true)));
        casa.imposta(CASA, Ritmo::Locale, un_ora);
        casa.fatto(CASA, true, un_ora);
        assert_eq!(casa.prossima(un_ora + RESPIRO), None);
        // Al risveglio dal sospeso si rifà anche l'osservatore locale.
        let muro = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let minuto = Duration::from_secs(60);
        casa.dormo(minuto, muro);
        assert!(casa.sveglio(muro + minuto * 120));
        assert_eq!(casa.prossima(un_ora + RESPIRO), Some(voce(CASA, true)));
        // Quella col solo riallineamento non ha un osservatore da rifare: lo
        // ritenta già ogni controllo.
        let mut senza = piano_avviato(&[(NAS, Ritmo::SoloRiallineamento)], zero);
        fai_tutti(&mut senza, zero);
        senza.dormo(minuto, muro);
        assert!(senza.sveglio(muro + minuto * 120));
        assert_eq!(senza.prossima(zero + RESPIRO), Some(voce(NAS, false)));
    }

    #[test]
    fn il_solo_riallineamento_va_ogni_cinque_minuti() {
        let zero = Instant::now();
        let mut piano = piano_avviato(&[(NAS, Ritmo::SoloRiallineamento)], zero);
        assert_eq!(fai_tutti(&mut piano, zero), vec![voce(NAS, false)]);
        assert_eq!(
            piano.prossima(zero + SONDAGGIO - Duration::from_secs(1)),
            None
        );
        assert_eq!(
            piano.prossima(zero + SONDAGGIO),
            Some(voce(NAS, false)),
            "senza osservatore non c'è niente da riarmare"
        );
    }

    #[test]
    fn piu_cause_insieme_fanno_un_riallineamento_solo() {
        let zero = Instant::now();
        let muro = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        // L'avvio, mai fatto; il sospeso; il ritorno; il cambio di passo; il
        // periodico scaduto: tutto insieme.
        let mut piano = piano_avviato(&[(NAS, Ritmo::Rete), (CASA, Ritmo::Locale)], zero);
        piano.dormo(Duration::from_secs(60), muro);
        assert!(piano.sveglio(muro + Duration::from_secs(3 * 60 * 60)));
        piano.tornata(NAS);
        piano.imposta(CASA, Ritmo::Nessuno, zero);
        piano.imposta(CASA, Ritmo::Locale, zero);
        let tardi = zero + RIALLINEAMENTO_LOCALE;
        assert_eq!(
            fai_tutti(&mut piano, tardi),
            vec![voce(CASA, true), voce(NAS, true)]
        );
        assert_eq!(piano.prossima(tardi + RESPIRO * 3), None);
    }

    #[test]
    fn una_cartella_che_non_risponde_si_riprova_ma_non_in_un_ciclo_stretto() {
        let zero = Instant::now();
        let mut piano = piano_avviato(&[(NAS, Ritmo::Rete), (USB, Ritmo::Locale)], zero);
        assert_eq!(piano.prossima(zero), Some(voce(USB, false)));
        piano.fatto(USB, false, zero);
        assert_eq!(piano.prossima(zero + RESPIRO), Some(voce(NAS, false)));
        piano.fatto(NAS, false, zero + RESPIRO);
        let un_attimo = Duration::from_secs(1);
        assert_eq!(
            piano.prossima(zero + SONDAGGIO - un_attimo),
            None,
            "non subito: il disco è probabilmente ancora spento"
        );
        // Al più fra cinque minuti, anche il disco locale che di periodico ne
        // ha uno di un'ora; tutti e due con l'osservatore rifatto.
        assert_eq!(
            fai_tutti(&mut piano, zero + SONDAGGIO + RESPIRO),
            vec![voce(USB, true), voce(NAS, true)]
        );
        // Risposto, si torna al passo di sempre.
        let dopo = zero + SONDAGGIO * 2;
        assert_eq!(piano.prossima(dopo), None);
        assert_eq!(piano.prossima(zero + RIALLINEAMENTO), None);
        assert_eq!(
            piano.prossima(zero + SONDAGGIO + RESPIRO * 2 + RIALLINEAMENTO),
            Some(voce(NAS, true))
        );
    }

    #[test]
    fn col_turno_occupato_e_dopo_ogni_scansione_si_lascia_un_respiro() {
        let zero = Instant::now();
        let mut piano = piano_avviato(&[(NAS, Ritmo::Rete), (CASA, Ritmo::Locale)], zero);
        let primo = piano.prossima(zero).expect("il primo");
        // Il turno è di «Scansiona»: si riprova dopo un respiro, non al
        // prossimo decimo di secondo.
        piano.aspetta_un_respiro(zero);
        assert!(!piano.libero(zero));
        assert_eq!(piano.manca(zero), Some(RESPIRO));
        assert_eq!(piano.prossima(zero + RESPIRO / 2), None);
        assert_eq!(piano.prossima(zero + RESPIRO), Some(primo.clone()));
        // E un respiro anche dopo una scansione, prima della prossima.
        piano.fatto(&primo.radice, true, zero + RESPIRO);
        assert_eq!(piano.prossima(zero + RESPIRO), None);
        assert_eq!(piano.fra_quanto_libero(zero + RESPIRO), RESPIRO);
        assert!(piano.prossima(zero + RESPIRO * 2).is_some());
    }

    #[test]
    fn perdere_una_cartella_e_ritrovarla_non_perde_la_promessa() {
        let zero = Instant::now();
        let poi = |minuti: u64| zero + Duration::from_secs(60 * minuti);
        // Il disco USB: armato, staccato, e per due controlli «non si sa dove
        // sta» — la strada che fino alla 3.0.1 azzerava la promessa.
        let mut piano = piano_avviato(&[(USB, Ritmo::Locale)], zero);
        fai_tutti(&mut piano, zero);
        piano.imposta(USB, Ritmo::Nessuno, poi(1));
        piano.imposta(USB, Ritmo::Nessuno, poi(2));
        piano.imposta(USB, Ritmo::Nessuno, poi(3));
        assert_eq!(piano.prossima(poi(3)), None, "finché non c'è, niente");
        piano.imposta(USB, Ritmo::Locale, poi(4));
        assert_eq!(
            piano.prossima(poi(4)),
            Some(voce(USB, false)),
            "riattaccato, si riguarda"
        );
        // Il NAS senza avvisi: ripiega sul solo riallineamento, riprova
        // l'osservatore a ogni controllo senza farcela — niente di nuovo — e
        // quando ce la fa si riallinea.
        let mut piano = piano_avviato(&[(NAS, Ritmo::Rete)], zero);
        fai_tutti(&mut piano, zero);
        piano.imposta(NAS, Ritmo::SoloRiallineamento, poi(1));
        assert_eq!(piano.prossima(poi(1)), Some(voce(NAS, false)));
        piano.fatto(NAS, true, poi(1));
        piano.imposta(NAS, Ritmo::SoloRiallineamento, poi(2));
        piano.imposta(NAS, Ritmo::SoloRiallineamento, poi(3));
        assert_eq!(piano.prossima(poi(3)), None);
        piano.imposta(NAS, Ritmo::Rete, poi(4));
        assert_eq!(piano.prossima(poi(4)), Some(voce(NAS, false)));
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
        let mut visti: Vec<Cambiamento> = Vec::new();
        let fine = Instant::now() + Duration::from_secs(3);
        while Instant::now() < fine && !visti.iter().any(|c| c.percorso.ends_with("01.flac")) {
            if let Ok(Messaggio::Avvisi(cambiati)) =
                orecchio.recv_timeout(Duration::from_millis(200))
            {
                visti.extend(cambiati);
            }
        }
        assert!(
            visti.iter().any(|c| c.percorso.ends_with("01.flac")),
            "il file nuovo non è stato annunciato: {visti:?}"
        );
        // Quel che resta della raffica: la cartella cambiata dal file nuovo,
        // se il sistema la annuncia, non deve contare.
        while let Ok(Messaggio::Avvisi(cambiati)) =
            orecchio.recv_timeout(Duration::from_millis(300))
        {
            visti.extend(cambiati);
        }
        let dell_album: Vec<&Cambiamento> = visti
            .iter()
            .filter(|c| c.percorso.ends_with("Album"))
            .collect();
        assert!(
            !dell_album.is_empty() && dell_album.iter().all(|c| c.nuovo),
            "la cartella creata è nuova, e la sua modifica non conta: {visti:?}"
        );
        // E fino in fondo: un album copiato è una scansione dell'album. Se la
        // modifica della cartella passasse, qui ci sarebbe la radice; se la
        // grafia dei percorsi del sistema non fosse quella della radice, niente.
        assert_eq!(
            cartelle_da_guardare(
                visti.clone(),
                vec![radice.clone()],
                PathRules::for_current_platform(),
                natura_di
            ),
            vec![cartella.path().join("Album").to_string_lossy().into_owned()]
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

    // ── il motore, dopo le passate ──

    fn tutte(cartelle: &[&str]) -> Vec<String> {
        cartelle
            .iter()
            .map(|cartella| (*cartella).to_owned())
            .collect()
    }

    /// Una passata arrivata in fondo: `saltate` non hanno risposto, `vuote`
    /// hanno risposto vuote (e stanno anche fra le saltate, come nel
    /// rapporto), `persi` non si sono letti.
    fn passata(saltate: &[&str], vuote: &[&str], persi: &[&str]) -> Passata {
        let mut tutte_le_saltate = tutte(saltate);
        tutte_le_saltate.extend(tutte(vuote));
        Passata::Fatta {
            saltate: tutte_le_saltate,
            vuote: tutte(vuote),
            rami_persi: tutte(persi),
            fermata: false,
        }
    }

    /// Un motore che ha letto l'elenco e finito il risveglio, col prossimo
    /// controllo fuori dalla prova.
    fn motore_con(radici: &[(&str, Ritmo)], adesso: Instant) -> Motore {
        let mut motore = Motore::nuovo(adesso);
        motore.elenco = radici
            .iter()
            .map(|(radice, _)| (*radice).to_owned())
            .collect();
        motore.piano = piano_avviato(radici, adesso);
        motore.configurata = true;
        motore.prossimo_controllo = adesso + Duration::from_secs(7 * 24 * 60 * 60);
        motore
    }

    #[test]
    fn un_nas_spento_non_trattiene_le_cartelle_di_casa() {
        // La simulazione della revisione: con un rinvio solo per tutti, il NAS
        // spento — riallineato ogni cinque minuti, e ogni volta saltato — si
        // portava dietro l'album rinviato sul disco di casa. In ventiquattro
        // ore, 286 riallineamenti del NAS e nessuna scansione di casa.
        let zero = Instant::now();
        let regole = PathRules::for_current_platform();
        let mut motore = motore_con(
            &[(CASA, Ritmo::Locale), (NAS, Ritmo::SoloRiallineamento)],
            zero,
        );
        // I riallineamenti dell'avvio: casa risponde, il NAS no; e un album
        // sul NAS, saltato, aspetta la sua radice.
        motore.dopo_il_riallineamento(CASA, Vec::new(), passata(&[], &[], &[]), zero);
        motore.dopo_il_riallineamento(NAS, Vec::new(), passata(&[NAS], &[], &[]), zero);
        let sul_nas = format!(r"{NAS}\Album");
        motore.dopo_la_riscansione(
            std::slice::from_ref(&sul_nas),
            passata(&[&sul_nas], &[], &[]),
            zero,
        );
        // Un minuto dopo, un album copiato a casa trova il disco esterno che
        // girava ancora a vuoto.
        let minuto = zero + Duration::from_secs(60);
        let a_casa = format!(r"{CASA}\Album");
        motore.dopo_la_riscansione(
            std::slice::from_ref(&a_casa),
            passata(&[&a_casa], &[], &[]),
            minuto,
        );
        // Il filo, come `lavora`: prima il riallineamento, poi quel che è
        // pronto. Ogni passata sul NAS dura tre secondi e lo trova spento.
        let tre_secondi = Duration::from_secs(3);
        let mut adesso = minuto;
        let mut del_nas = 0_u32;
        let ripresa = loop {
            adesso += prossima_attesa(&motore, adesso);
            assert!(
                adesso < zero + Duration::from_secs(24 * 60 * 60),
                "il disco di casa non si è più riguardato ({del_nas} riallineamenti del NAS)"
            );
            if let Some(voce) = motore.piano.prossima(adesso) {
                assert_eq!(voce.radice, NAS);
                del_nas += 1;
                let assorbiti = motore.attesa.togli_sotto(NAS, regole);
                adesso += tre_secondi;
                motore.dopo_il_riallineamento(NAS, assorbiti, passata(&[NAS], &[], &[]), adesso);
                continue;
            }
            if motore.piano.fra_quanto_libero(adesso).is_zero() && motore.attesa.pronto(adesso) {
                let presi = motore.attesa.prendi(adesso);
                if presi.contains(&nuovo(CASA)) {
                    break adesso;
                }
                let cartelle: Vec<String> = presi.into_iter().map(|c| c.percorso).collect();
                let mute: Vec<&str> = cartelle.iter().map(String::as_str).collect();
                adesso += tre_secondi;
                motore.dopo_la_riscansione(&cartelle, passata(&mute, &[], &[]), adesso);
            }
        };
        assert_eq!(
            ripresa,
            minuto + SONDAGGIO,
            "al suo rinvio, non a quello del NAS"
        );
        assert_eq!(del_nas, 1, "intanto il NAS si è riprovato una volta sola");
    }

    #[test]
    fn una_cartella_svuotata_davvero_resta_al_suo_passo() {
        let zero = Instant::now();
        let album = nuovo(r"\\nas\musica\Album");
        // Risponde vuota: la guardia per radice non ha tolto niente. Non è «non
        // risponde»: riprovarla ogni cinque minuti era un riallineamento ogni
        // cinque minuti per sempre, su una cartella svuotata davvero.
        let mut motore = motore_con(&[(NAS, Ritmo::Rete)], zero);
        assert_eq!(motore.piano.prossima(zero), Some(voce(NAS, false)));
        motore.dopo_il_riallineamento(NAS, vec![album.clone()], passata(&[], &[NAS], &[]), zero);
        assert_eq!(
            motore.attesa.manca(zero),
            None,
            "quel che aspettava sotto lo guarda il suo prossimo riallineamento"
        );
        assert_eq!(motore.piano.prossima(zero + SONDAGGIO), None);
        assert_eq!(
            motore.piano.manca(zero + RESPIRO),
            Some(RIALLINEAMENTO - RESPIRO)
        );
        assert_eq!(
            motore.piano.prossima(zero + RIALLINEAMENTO),
            Some(voce(NAS, true)),
            "al suo giro"
        );
        // Una che non risponde invece si riprova fra cinque minuti, e con lei
        // quel che aspettava.
        let mut muta = motore_con(&[(NAS, Ritmo::Rete)], zero);
        muta.dopo_il_riallineamento(NAS, vec![album], passata(&[NAS], &[], &[]), zero);
        assert_eq!(muta.attesa.manca(zero), Some(SONDAGGIO));
        assert_eq!(muta.piano.prossima(zero + SONDAGGIO), Some(voce(NAS, true)));
        // In una scansione di più cartelle: la vuota non torna, la muta sì.
        let altra = r"\\nas\altra";
        let mut motore = motore_con(&[(NAS, Ritmo::Rete), (altra, Ritmo::Rete)], zero);
        motore.dopo_la_riscansione(&tutte(&[NAS, altra]), passata(&[altra], &[NAS], &[]), zero);
        assert_eq!(motore.attesa.manca(zero), Some(SONDAGGIO));
        assert_eq!(motore.attesa.prendi(zero + SONDAGGIO), vec![nuovo(altra)]);
        // E un ramo perso sotto la vuota non si riguarda da solo: una
        // sottocartella non ha la guardia per radice, e la giudicherebbe a
        // pezzi.
        let ramo = format!(r"{NAS}\System Volume Information");
        let mut motore = motore_con(&[(NAS, Ritmo::Rete)], zero);
        motore.dopo_il_riallineamento(NAS, Vec::new(), passata(&[], &[NAS], &[&ramo]), zero);
        assert_eq!(motore.attesa.manca(zero), None);
        motore.dopo_la_riscansione(&tutte(&[NAS]), passata(&[], &[NAS], &[&ramo]), zero);
        assert_eq!(motore.attesa.manca(zero), None);
    }

    #[test]
    fn col_turno_occupato_non_si_chiede_niente_al_disco() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static DOMANDE: AtomicUsize = AtomicUsize::new(0);
        fn natura_contata(_: &Path) -> Natura {
            DOMANDE.fetch_add(1, Ordering::SeqCst);
            Natura::Cartella
        }
        let dati = tempfile::tempdir().expect("cartella dati di prova");
        let stato = Stato::apri(dati.path().to_path_buf());
        let zero = Instant::now();
        let mut motore = motore_con(&[(CASA, Ritmo::Locale)], zero);
        let brano = format!(r"{CASA}\Album\01.flac");
        motore.attesa.avvisati([vecchio(&brano)], zero);
        let pronto = zero + ACCORPAMENTO;
        // «Scansiona» ha il turno: si riprova ogni respiro, e ogni volta solo il
        // turno — prima, ogni due secondi, una domanda al disco per cartella.
        let di_scansiona = stato.turno_di_scansione().expect("il turno di «Scansiona»");
        let mut quando = pronto;
        for _ in 0..5 {
            assert!(
                motore
                    .prepara_la_riscansione(&stato, quando, natura_contata)
                    .is_none()
            );
            assert_eq!(motore.piano.fra_quanto_libero(quando), RESPIRO);
            quando += RESPIRO;
        }
        assert_eq!(
            DOMANDE.load(Ordering::SeqCst),
            0,
            "il disco è stato interrogato"
        );
        assert!(motore.attesa.pronto(quando), "e niente è andato perso");
        drop(di_scansiona);
        let (turno, cartelle) = motore
            .prepara_la_riscansione(&stato, quando, natura_contata)
            .expect("il turno libero");
        assert_eq!(cartelle, vec![format!(r"{CASA}\Album")]);
        assert!(DOMANDE.load(Ordering::SeqCst) > 0);
        assert!(!motore.attesa.pronto(quando), "preso");
        drop(turno);
    }

    #[test]
    fn un_ramo_perso_si_riguarda_fra_poco_ma_non_per_sempre() {
        let regole = PathRules {
            case_insensitive: true,
        };
        let zero = Instant::now();
        let ramo = r"C:\Musica\CD nuovo";
        let casa = tutte(&[CASA]);
        let il_ramo = tutte(&[ramo]);
        let mut attesa = Accorpatore::default();
        // Il riallineamento di casa non ha letto il `.cue` del CD appena
        // copiato: lo si riguarda fra un minuto, non al giro dopo.
        assert!(attesa.rami(&casa, &il_ramo, zero, regole).is_empty());
        assert_eq!(attesa.manca(zero), Some(RAMO_PERSO));
        // Una cartella a permessi negati non si legge mai: ogni volta il
        // doppio, e poi basta.
        let mut adesso = zero;
        let mut attese = Vec::new();
        let mut lasciati = Vec::new();
        for _ in 0..10 {
            let Some(manca) = attesa.manca(adesso) else {
                break;
            };
            attese.push(manca);
            adesso += manca;
            assert_eq!(attesa.prendi(adesso), vec![nuovo(ramo)]);
            lasciati.extend(attesa.rami(&il_ramo, &il_ramo, adesso, regole));
        }
        let minuti = |quanti: u64| Duration::from_secs(60 * quanti);
        assert_eq!(
            attese,
            vec![minuti(1), minuti(2), minuti(4), minuti(8), minuti(16)]
        );
        assert_eq!(lasciati, il_ramo, "detto una volta sola");
        // Il riallineamento di casa lo riperde: niente giro da capo.
        assert!(attesa.rami(&casa, &il_ramo, adesso, regole).is_empty());
        assert_eq!(attesa.manca(adesso), None);
        // Una passata che non l'ha guardato — la radice non ha risposto — non
        // lo rimette in regola.
        assert!(attesa.rami(&[], &[], adesso, regole).is_empty());
        attesa.rami(&casa, &il_ramo, adesso, regole);
        assert_eq!(attesa.manca(adesso), None);
        // Una che lo legge sì: la prossima volta si riprova fra un minuto.
        attesa.rami(&casa, &[], adesso, regole);
        attesa.rami(&casa, &il_ramo, adesso, regole);
        assert_eq!(attesa.manca(adesso), Some(RAMO_PERSO));

        // E dalla sorveglianza: l'album appena copiato, il cui `.cue` non si è
        // letto. Il resto della passata è andato.
        let mut motore = motore_con(&[(CASA, Ritmo::Locale)], zero);
        let album = format!(r"{CASA}\CD nuovo");
        motore.dopo_la_riscansione(
            std::slice::from_ref(&album),
            passata(&[], &[], &[&album]),
            zero,
        );
        assert_eq!(motore.attesa.manca(zero), Some(RAMO_PERSO));
        assert_eq!(motore.attesa.prendi(zero + RAMO_PERSO), vec![nuovo(&album)]);
        // E dal riallineamento.
        motore.dopo_il_riallineamento(CASA, Vec::new(), passata(&[], &[], &[&album]), zero);
        assert_eq!(motore.attesa.manca(zero), Some(RAMO_PERSO * 2));
    }

    #[test]
    fn la_cartella_scelta_al_primo_avvio_non_si_riallinea_da_sola() {
        let zero = Instant::now();
        // Il primo avvio: all'apertura l'elenco è vuoto, nei primi secondi se
        // ne sceglie una, e la si scansiona a mano. Arriva col primo elenco
        // letto, ma non c'era quando il filo è partito.
        let mut piano = Pianificatore::default();
        piano.fotografa(Vec::new());
        piano.imposta(CASA, Ritmo::Locale, zero);
        piano.elenco_letto();
        piano.avvio();
        assert_eq!(piano.prossima(zero), None, "la scansiona chi l'ha scelta");
        assert_eq!(
            piano.prossima(zero + RIALLINEAMENTO_LOCALE),
            Some(voce(CASA, true)),
            "il suo periodico sì"
        );
        // Quelle che c'erano all'apertura si riallineano, e solo loro.
        let mut piano = Pianificatore::default();
        piano.fotografa(tutte(&[NAS]));
        piano.imposta(NAS, Ritmo::Rete, zero);
        piano.imposta(CASA, Ritmo::Locale, zero);
        piano.elenco_letto();
        piano.avvio();
        assert_eq!(fai_tutti(&mut piano, zero), vec![voce(NAS, false)]);
        // Una dell'apertura tolta e rimessa dopo è un'aggiunta come le altre.
        piano.tieni_solo(&tutte(&[CASA]));
        piano.imposta(NAS, Ritmo::Rete, zero + RESPIRO * 2);
        assert_eq!(piano.prossima(zero + RESPIRO * 3), None);
    }

    #[test]
    fn a_sorveglianza_spenta_niente_si_riprova_da_se() {
        let zero = Instant::now();
        let album = format!(r"{NAS}\Album");
        let l_album = std::slice::from_ref(&album);
        let mut motore = motore_con(&[(NAS, Ritmo::Rete)], zero);
        motore.dopo_la_riscansione(l_album, passata(&[&album], &[], &[]), zero);
        assert_eq!(motore.attesa.manca(zero), Some(SONDAGGIO));
        // Spenta: quel che aspettava il rinvio cade. Prima girava ogni cinque
        // minuti per tutta la sessione.
        assert!(motore.imposta_modo(Modo::Spenta));
        assert_eq!(motore.attesa.manca(zero), None);
        assert!(!motore.imposta_modo(Modo::Spenta));
        // Una richiesta di riguardare vale anche da spenta...
        motore.attesa.da_riguardare([album.clone()], zero);
        let pronto = zero + ACCORPAMENTO;
        assert_eq!(motore.attesa.prendi(pronto), vec![nuovo(&album)]);
        // ...ma se non va, non si riprova da sé: né il disco che non risponde,
        // né la scansione fallita, né la sottocartella che non si legge.
        motore.dopo_la_riscansione(l_album, passata(&[&album], &[], &[]), pronto);
        motore.dopo_la_riscansione(l_album, Passata::Fallita, pronto);
        motore.dopo_la_riscansione(l_album, passata(&[], &[], &[&album]), pronto);
        motore.dopo_il_riallineamento(NAS, vec![nuovo(&album)], passata(&[NAS], &[], &[]), pronto);
        assert_eq!(motore.attesa.manca(pronto), None);
        // Riaccesa, si torna a riprovare.
        assert!(motore.imposta_modo(Modo::Tutte));
        motore.dopo_la_riscansione(l_album, passata(&[&album], &[], &[]), pronto);
        assert_eq!(motore.attesa.manca(pronto), Some(SONDAGGIO));
    }
}
