//! Una connessione aperta: come nasce, i suoi due fili, come muore.
//!
//! # Due fili per connessione
//!
//! **Chi legge** aspetta la prossima cornice e la tratta: risponde ai ping,
//! fa i passi dell'associazione, serve le letture, consegna i messaggi al
//! desktop. **Chi scrive** ha una coda ([`Uscita`]) e la svuota sul socket;
//! quando la coda tace manda il battito e, se richiesti, i ping d'orologio.
//!
//! Due e non uno perché i socket della libreria standard non sanno aspettare
//! «dati in arrivo **o** qualcosa da mandare»: un filo solo dovrebbe girare a
//! scadenze brevi, e su Windows una lettura scaduta lascia il socket in uno
//! stato indeterminato (vedi `canale`). E perché `Rete::manda` non deve
//! bloccare mai: mette in coda e torna, anche se l'altro PC legge piano.
//!
//! # Gli istanti dell'orologio
//!
//! `t2` (l'arrivo di un ping) e `t4` (l'arrivo di un pong) li prende chi
//! legge, appena la cornice è intera; `t1` e `t3` li prende chi scrive,
//! subito prima di cifrare il messaggio che li porta, cioè dopo l'attesa in
//! coda e non prima. Così il tempo che un pong passa in coda dietro a uno
//! stato di sessione sta fra `t2` e `t3`, dove la formula dell'orologio lo
//! toglie, e non dentro il tempo di rete, dove la falserebbe.
//!
//! Resta un'attesa che da qui non si toglie: quella nel buffer d'invio del
//! sistema, se lo stato è già stato scritto sul socket e non è ancora
//! partito tutto. Quel tempo cade fra `t3` e `t4`, cioè si vede: allunga il
//! giro (`t4 − t1 − (t3 − t2)`), e lo stimatore del desktop scarta ogni
//! scambio con un giro oltre il doppio del minimo (vedi
//! `rymembra_play::sincronia::orologio`). Non serve un'altra corsia per i
//! pong: a un campione al secondo, e con gli stati che partono a gesti, se ne
//! perde uno ogni tanto. Toglierla del tutto vorrebbe dire sapere quanti byte
//! aspettano nel buffer del sistema, e la libreria standard non lo dice.
//!
//! # La coda d'uscita, in byte
//!
//! La coda ha un tetto in voci ([`CODA`]) e uno in **byte** ([`TETTO_USCITA`]).
//! Il secondo c'è dalla revisione del 26 ottobre: con le sole voci, un capo
//! che chiede letture da due mega senza mai leggere le risposte faceva
//! preparare all'host duecentocinquantasei risposte, mezzo gigabyte, per
//! connessione. Adesso chi serve una lettura aspetta che ci sia posto prima
//! di leggere dal disco ([`Collegamento::aspetta_posto`]): intanto non legge
//! il socket, e la contropressione la fa il TCP, fino a chi chiede.
//! Le voci che non possono aspettare — quelle di `Rete::manda`, che non
//! blocca mai — oltre il tetto chiudono la connessione: un capo che lascia
//! otto mega di stato non letti non sta seguendo la sessione.
//!
//! # Gli stati in coda
//!
//! Lo stato della sessione (`Messaggio::Stato`) è una fotografia intera, col
//! suo numero di versione, e un membro scarta quelli più vecchi dell'ultimo
//! applicato. Quindi in coda ne serve uno solo, l'ultimo: chi scrive salta
//! uno stato se dietro di lui ce n'è già uno più nuovo ([`Uscita::App`]), e
//! le voci che stavano fra i due — un `Esito`, un pong, un'ancora — partono
//! senza aspettare una fotografia che nessuno guarderebbe. L'ultimo parte al
//! suo posto, dopo quel che era in coda prima di lui: l'ordine fra gli altri
//! messaggi e lo stato che vale non cambia. Con un collegamento lento e
//! l'host che prepara le voci, prima ne aspettavano anche due o tre in fila,
//! mezzo mega l'uno.
//!
//! # Chi può dire che cosa
//!
//! Da una chiave non fidata si accettano solo il saluto, il battito e
//! l'associazione: messaggi JSON di pochi byte, e quindi un tetto di
//! [`TETTO_ESTRANEO`] **prima** di leggerli (vedi `cornice`). Sull'host una
//! chiave non fidata ha solo connessioni di controllo, al più
//! `condiviso::ESTRANEI`, e ciascuna vive al più `Tempi::vita_estraneo` se
//! non si sta associando: il battito da solo non basta a tenerla.
//!
//! # Come muore
//!
//! Una connessione si chiude in un solo modo: qualcuno scrive il **perché**
//! ([`Collegamento::chiudi`]; il primo perché vince), e in quel momento il
//! desktop riceve l'unico `Evento::Chiusa` e un'associazione in corso si
//! conta come fallita. Poi chi scrive manda quel che era già in coda,
//! `Chiudo` col perché, e chiude la sua metà del socket; l'altro capo,
//! leggendo `Chiudo` e la fine, chiude la sua, e questo sveglia il nostro
//! filo che legge, che nel frattempo ha letto e buttato tutto. Uscendo, chi
//! legge toglie la connessione dall'elenco.
//!
//! La `Chiusa` parte subito e non all'uscita di chi legge perché chi legge
//! può restare bloccato: su Windows una lettura bloccata la sveglia solo
//! l'altro capo, e un capo che non chiude più la sua metà (un PC appeso, un
//! cavo staccato) la tiene ferma fino alla scadenza del silenzio. Il desktop
//! non deve aspettare quindici secondi per sapere una cosa che si sa già.
//!
//! # L'ordine degli eventi
//!
//! Tutti gli eventi di una connessione passano dal suo cancello
//! ([`Collegamento::annuncia`]): `Aperta`, poi quel che succede, poi
//! `Chiusa`, e dopo niente. Senza, un messaggio letto un istante prima della
//! chiusura potrebbe arrivare al desktop un istante dopo la sua `Chiusa`.

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

use rymembra_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use serde::Serialize;

use crate::ascolto::Stretta;
use crate::canale::{self, Cifratore, Decifratore, Guasto};
use crate::codice::{self, Rifiuto};
use crate::condiviso::{Condiviso, nome_valido};
use crate::cornice::{MASSIMO_MESSAGGIO, Tipo};
use crate::errori::{chiave, da_testo, errore};
use crate::identita::{ChiavePubblica, Impronta};
use crate::messaggi::{Gettone, Messaggio, VERSIONE};
use crate::rete::{CampioneOrologio, Evento, IdConnessione};
use crate::servente::{
    COPERTINA_MASSIMA, Chiesto, LETTURA_MASSIMA, Risposta, Servito, leggi_copertina, leggi_file,
};
use crate::trasporto::{Byte32, Trasporto, Uso};

/// Quante voci aspettano chi scrive, al più.
///
/// Duecentocinquantasei sono un minuto di ancore a quattro al secondo: una
/// coda piena vuol dire un altro capo che non legge da quasi un minuto, e la
/// connessione si chiude — il silenzio dall'altra parte l'avrebbe chiusa
/// comunque. Il tetto che conta per la memoria è l'altro, [`TETTO_USCITA`].
const CODA: usize = 256;

/// I byte non ancora scritti che una connessione può tenere in coda: otto
/// mebibyte. Vedi «La coda d'uscita, in byte».
///
/// Quattro letture da due mega: abbastanza perché una connessione di lettura
/// non resti mai a secco aspettando il disco, e su quarantotto connessioni
/// quattrocento mega nel caso peggiore, invece di ventiquattro gigabyte.
pub(crate) const TETTO_USCITA: usize = 8 * 1024 * 1024;

/// Il tetto dei messaggi JSON di una chiave non fidata: quattro kibibyte.
///
/// Il messaggio più lungo che un estraneo può mandare per davvero è il
/// saluto, con un nome di sessantaquattro caratteri: duecento byte. Quattro
/// kibibyte lasciano posto a un saluto di una versione futura, e gonfiati da
/// `serde` restano sotto i cento kibibyte.
pub(crate) const TETTO_ESTRANEO: usize = 4 * 1024;

/// Quanto pesa in coda una voce senza carico, o la testa di una con carico.
const PESO_TESTA: usize = 128;

/// Quanti ping mandati si ricordano, per riconoscere i pong.
const PING_RICORDATI: usize = 16;

/// Ogni quanto chi aspetta una risposta su un canale guarda se deve smettere
/// (l'annullo di una lettura, la chiusura di chi l'ha chiesta).
///
/// Dieci millisecondi: l'annullo di una lettura deve tornare «subito», ed è
/// un'attesa su un canale, non su un socket — la si può interrompere senza
/// il guaio delle letture scadute.
pub(crate) const RESPIRO_LETTURA: Duration = Duration::from_millis(10);

/// Una voce per chi scrive.
#[derive(Debug)]
pub(crate) enum Uscita {
    /// Un messaggio JSON già serializzato.
    Json(Vec<u8>),
    /// Un messaggio dell'applicazione (`Rete::manda`), forse compresso: vedi
    /// «La compressione» in `cornice`.
    App {
        /// Il JSON, o il JSON compresso.
        byte: Vec<u8>,
        /// Se è compresso.
        compresso: bool,
        /// Per uno stato di sessione, il suo numero fra gli stati messi in
        /// coda su questa connessione: vedi «Gli stati in coda».
        stato: Option<u64>,
    },
    /// Il pong di un ping: `t3` lo mette chi scrive.
    Pong {
        /// Il numero del ping.
        seq: u32,
        /// La partenza del ping.
        t1: u64,
        /// L'arrivo del ping.
        t2: u64,
    },
    /// La risposta a una lettura, coi suoi byte.
    Dati(Dati),
    /// Accende o spegne i ping d'orologio.
    Sincronizza(bool),
    /// Niente da mandare: solo un colpetto, perché chi scrive guardi se la
    /// connessione si sta chiudendo.
    Sveglia,
}

/// La risposta a una lettura, in coda.
///
/// Porta il gettone e la sua generazione perché chi scrive li ricontrolli
/// prima di cifrare: una revoca arrivata mentre la risposta aspettava in coda
/// la trasforma in un rifiuto. Fino alla revisione del 26 ottobre il
/// controllo si faceva solo prima di metterla in coda, e i byte già in coda
/// partivano lo stesso.
#[derive(Debug)]
pub(crate) struct Dati {
    /// Il numero della lettura.
    lettura: u32,
    /// `Some(totale)` per un `Pezzo`, `None` per un'`Immagine`.
    totale: Option<u64>,
    /// I byte.
    byte: Vec<u8>,
    /// Il gettone letto.
    gettone: Gettone,
    /// La sua generazione quando lo si è letto.
    generazione: u64,
}

impl Uscita {
    /// Quanto pesa in coda, per [`TETTO_USCITA`].
    fn peso(&self) -> usize {
        match self {
            Self::Json(json) | Self::App { byte: json, .. } => {
                json.len().saturating_add(PESO_TESTA)
            }
            Self::Dati(dati) => dati.byte.len().saturating_add(PESO_TESTA),
            Self::Pong { .. } | Self::Sincronizza(_) | Self::Sveglia => PESO_TESTA,
        }
    }
}

/// Come si chiude.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Modo {
    /// Si manda quel che era in coda, poi `Chiudo` col perché.
    Cortese,
    /// Si manda quel che era in coda, senza `Chiudo`: l'altro capo se n'è
    /// già andato, o ha già detto lui perché.
    Muto,
    /// Si taglia: niente coda, niente `Chiudo`. È l'annullo di una lettura,
    /// in cui la connessione si butta.
    Taglio,
}

/// Il perché di una chiusura, e il modo.
#[derive(Debug, Clone, Copy)]
struct Chiusura {
    codice: Option<ErrorCodeKind>,
    modo: Modo,
}

/// Che cosa il desktop sa di una connessione. Vedi «L'ordine degli eventi».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Annunci {
    /// Niente ancora.
    Nessuno,
    /// Ha ricevuto `Aperta`.
    Aperta,
    /// Ha ricevuto `Chiusa`, o non la riceverà mai: da qui, niente.
    Chiusa,
}

/// A che punto è l'associazione su questa connessione. Vedi `codice`.
///
/// Ogni stato in corso porta il numero della sua prenotazione fra i
/// tentativi (`codice::Tentativi`): finisce con la riuscita, che la toglie
/// dal conto, o con la chiusura della connessione, che la lascia lì come
/// fallita — tranne in [`Associazione::Impegnata`], prima del nonce di chi
/// ospita, dove la chiusura la toglie senza contarla (vedi
/// `Collegamento::chiudi`).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Associazione {
    /// Nessuna in corso.
    Nessuna,
    /// Chi ha chiesto (il membro): impegno mandato, aspetta il nonce.
    Impegnata {
        nonce: [u8; 32],
        dal: Instant,
        prenotazione: u64,
    },
    /// Chi risponde (l'host): impegno ricevuto, nonce mandato, aspetta la
    /// rivelazione.
    AttesaRivela {
        impegno: [u8; 32],
        nonce: [u8; 32],
        dal: Instant,
        prenotazione: u64,
    },
    /// Il codice è sugli schermi: chi ha già confermato.
    Codice {
        qui: bool,
        la: bool,
        dal: Instant,
        prenotazione: u64,
    },
}

impl Associazione {
    /// Da quando è in corso, se lo è.
    const fn dal(&self) -> Option<Instant> {
        match self {
            Self::Nessuna => None,
            Self::Impegnata { dal, .. }
            | Self::AttesaRivela { dal, .. }
            | Self::Codice { dal, .. } => Some(*dal),
        }
    }

    /// La sua prenotazione, se è in corso.
    const fn prenotazione(&self) -> Option<u64> {
        match self {
            Self::Nessuna => None,
            Self::Impegnata { prenotazione, .. }
            | Self::AttesaRivela { prenotazione, .. }
            | Self::Codice { prenotazione, .. } => Some(*prenotazione),
        }
    }
}

/// Chi ci si aspetta dall'altra parte di una connessione aperta da qui.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Atteso {
    /// Quella chiave: l'host di una sessione, per le letture e il servizio.
    Chiave(ChiavePubblica),
    /// Una chiave con quell'impronta: un host trovato dalla scoperta, che
    /// l'impronta l'ha detta in chiaro. Vedi `Rete::collega_atteso`.
    Impronta(Impronta),
}

impl Atteso {
    /// La chiave dimostrata dalla stretta è quella attesa?
    fn corrisponde(&self, pari: &ChiavePubblica) -> bool {
        match self {
            Self::Chiave(chiave) => chiave == pari,
            Self::Impronta(impronta) => pari.impronta() == *impronta,
        }
    }
}

/// Una connessione aperta: stretta e saluto fatti.
pub(crate) struct Collegamento {
    /// Il suo numero.
    pub(crate) id: IdConnessione,
    /// A che cosa serve.
    pub(crate) uso: Uso,
    /// L'ha aperta l'altro capo.
    pub(crate) in_entrata: bool,
    /// La chiave dell'altro capo.
    pub(crate) pari: ChiavePubblica,
    /// L'indirizzo dell'altro capo, per i tetti. Non finisce mai in un
    /// errore né in una stampa.
    pub(crate) ip: IpAddr,
    /// Quando si è aperta: un estraneo ha un tempo di vita, e fra due
    /// connessioni di lettura ugualmente ferme lascia il posto la più vecchia
    /// (vedi `Condiviso::registra_entrante`).
    pub(crate) aperta: Instant,
    /// L'hash della stretta.
    hash: [u8; 32],
    /// La rete, per la chiusura (gli eventi, i tentativi). Debole: la rete
    /// tiene le connessioni nel suo elenco, e un riferimento forte all'indietro
    /// sarebbe un giro che non si libera mai.
    rete: Weak<Condiviso>,
    /// Il cancello degli eventi.
    annunci: Mutex<Annunci>,
    /// La coda di chi scrive.
    uscita: SyncSender<Uscita>,
    /// Quanti byte aspettano in coda. Vedi «La coda d'uscita, in byte».
    in_uscita: Mutex<usize>,
    /// Suona quando chi scrive ne ha mandati.
    posto_libero: Condvar,
    /// Una maniglia del socket, per tagliarlo.
    presa: TcpStream,
    /// Il perché della chiusura; `None` finché è aperta.
    chiusura: Mutex<Option<Chiusura>>,
    /// Chi legge è uscito.
    lettore_finito: AtomicBool,
    /// Il desktop vuole gli `Evento::Orologio` di questa connessione.
    sincronizza: AtomicBool,
    /// I ping mandati e non ancora tornati: numero e `t1`.
    ping: Mutex<VecDeque<(u32, u64)>>,
    /// Chi legge sta servendo una lettura: dalla richiesta alla risposta
    /// messa in coda. Vedi [`Collegamento::al_lavoro`].
    servendo: AtomicBool,
    /// L'ultima volta che una lettura è cominciata o finita qui; all'inizio,
    /// l'apertura.
    ultimo_lavoro: Mutex<Instant>,
    /// Il numero dell'ultimo stato di sessione messo in coda: vedi «Gli stati
    /// in coda».
    ultimo_stato: AtomicU64,
    /// L'associazione.
    associazione: Mutex<Associazione>,
    /// Le letture chieste su questa connessione e non ancora risposte: chi le
    /// aspetta, per numero di lettura.
    pub(crate) attese: Mutex<HashMap<u32, SyncSender<Risposta>>>,
}

impl std::fmt::Debug for Collegamento {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Né la chiave né l'indirizzo dell'altro capo: una stampa di prova
        // finisce prima o poi in un diario.
        f.debug_struct("Collegamento")
            .field("id", &self.id)
            .field("uso", &self.uso)
            .field("in_entrata", &self.in_entrata)
            .finish_non_exhaustive()
    }
}

impl Collegamento {
    /// Mette una voce in coda per chi scrive, senza aspettare. `false` se la
    /// connessione si sta chiudendo, o se la coda è piena — in voci o in byte —
    /// e allora la si chiude: vedi «La coda d'uscita, in byte».
    pub(crate) fn metti(&self, voce: Uscita) -> bool {
        if self.chiusa() {
            return false;
        }
        let peso = voce.peso();
        {
            let mut in_uscita = chiave(&self.in_uscita);
            if in_uscita.saturating_add(peso) > TETTO_USCITA {
                drop(in_uscita);
                self.chiudi(Some(ErrorCodeKind::InsiemeConnessioneCaduta), Modo::Taglio);
                return false;
            }
            *in_uscita = in_uscita.saturating_add(peso);
        }
        match self.uscita.try_send(voce) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.scritto(peso);
                self.chiudi(Some(ErrorCodeKind::InsiemeConnessioneCaduta), Modo::Taglio);
                false
            }
            Err(TrySendError::Disconnected(_)) => {
                self.scritto(peso);
                false
            }
        }
    }

    /// Mette in coda un messaggio dell'applicazione, già serializzato e forse
    /// compresso. Uno `stato` di sessione prende il numero dopo l'ultimo, e
    /// quelli prima di lui ancora in coda non partiranno: vedi «Gli stati in
    /// coda». Come [`Collegamento::metti`], `false` se non entra.
    pub(crate) fn metti_app(&self, byte: Vec<u8>, compresso: bool, stato: bool) -> bool {
        let stato = stato.then(|| {
            self.ultimo_stato
                .fetch_add(1, Ordering::SeqCst)
                .wrapping_add(1)
        });
        self.metti(Uscita::App {
            byte,
            compresso,
            stato,
        })
    }

    /// Aspetta che in coda ci sia posto per `quanto` byte. `false` se intanto
    /// la connessione si chiude. Vedi «La coda d'uscita, in byte».
    fn aspetta_posto(&self, quanto: usize) -> bool {
        let mut in_uscita = chiave(&self.in_uscita);
        loop {
            if self.chiusa() {
                return false;
            }
            if in_uscita.saturating_add(quanto) <= TETTO_USCITA {
                return true;
            }
            // A giri di cinquanta millisecondi, per vedere anche una chiusura
            // decisa da un altro filo senza che nessuno suoni.
            in_uscita = self
                .posto_libero
                .wait_timeout(in_uscita, Duration::from_millis(50))
                .map_or_else(|e| e.into_inner().0, |(guardia, _)| guardia);
        }
    }

    /// Chi scrive ha mandato (o buttato) una voce di quel peso.
    fn scritto(&self, peso: usize) {
        {
            let mut in_uscita = chiave(&self.in_uscita);
            *in_uscita = in_uscita.saturating_sub(peso);
        }
        self.posto_libero.notify_all();
    }

    /// Chiude, col suo perché. Il primo perché vince: chi arriva dopo non lo
    /// cambia, e il desktop riceve quello vero. Vedi «Come muore».
    pub(crate) fn chiudi(&self, codice: Option<ErrorCodeKind>, modo: Modo) {
        {
            let mut chiusura = chiave(&self.chiusura);
            if chiusura.is_some() {
                return;
            }
            *chiusura = Some(Chiusura { codice, modo });
        }
        if modo == Modo::Taglio {
            let _ = self.presa.shutdown(Shutdown::Both);
        }
        // Un colpetto a chi scrive, che sta aspettando sulla coda, e a chi
        // legge, se sta aspettando posto in coda. Se la coda è piena chi scrive
        // è sveglio comunque, e guarda la chiusura prima della voce dopo.
        let _ = self.uscita.try_send(Uscita::Sveglia);
        self.posto_libero.notify_all();
        let Some(rete) = self.rete.upgrade() else {
            return;
        };
        // Un'associazione ancora in corso quando la connessione si chiude è
        // un'associazione fallita, qualunque sia il perché: un no, una
        // rivelazione falsa, una scadenza, un cavo staccato. La sua
        // prenotazione resta fra i tentativi; qui si dice solo che è finita.
        //
        // Tranne una, dal lato di chi chiede, chiusa prima del nonce di chi
        // ospita (`Impegnata`): un codice non c'è mai stato, e si toglie dal
        // conto come per `Occupato` — vedi `Tentativi::annulla`. Fino alla
        // revisione del 30 ottobre contava anche lei: un host al tetto dei
        // tentativi per colpa di altri PC rifiuta l'impegno con
        // `troppiTentativi` prima del nonce, e al terzo rifiuto il membro si
        // fermava da solo per dieci minuti.
        let (prenotazione, prima_del_nonce) = {
            let mut associazione = chiave(&self.associazione);
            let prenotazione = associazione.prenotazione();
            let prima_del_nonce = matches!(*associazione, Associazione::Impegnata { .. });
            *associazione = Associazione::Nessuna;
            (prenotazione, prima_del_nonce)
        };
        if let Some(numero) = prenotazione {
            let mut tentativi = chiave(&rete.tentativi);
            if prima_del_nonce {
                tentativi.annulla(numero);
            } else {
                tentativi.finito(numero, false);
            }
        }
        let mut annunci = chiave(&self.annunci);
        // La `Chiusa` si deve a chi ha avuto l'`Aperta`, e sempre a una
        // connessione di `collega`: il desktop ne ha l'id da prima.
        let dovuta = self.uso == Uso::Controllo
            && match *annunci {
                Annunci::Aperta => true,
                Annunci::Nessuno => !self.in_entrata,
                Annunci::Chiusa => false,
            };
        *annunci = Annunci::Chiusa;
        if dovuta {
            rete.emetti(Evento::Chiusa {
                id: self.id,
                codice,
            });
        }
    }

    /// L'`Evento::Aperta`, se la connessione non si è già chiusa.
    fn annuncia_apertura(&self, rete: &Condiviso, evento: Evento) {
        let mut annunci = chiave(&self.annunci);
        if *annunci == Annunci::Nessuno {
            rete.emetti(evento);
            *annunci = Annunci::Aperta;
        }
    }

    /// Un evento di questa connessione: passa solo fra `Aperta` e `Chiusa`.
    fn annuncia(&self, rete: &Condiviso, evento: Evento) {
        se_aperta(&self.annunci, || rete.emetti(evento));
    }

    /// La connessione si sta chiudendo (o è chiusa).
    pub(crate) fn chiusa(&self) -> bool {
        chiave(&self.chiusura).is_some()
    }

    /// La connessione sta servendo una lettura, o ha ancora byte di una
    /// risposta da mandare: mandarla via adesso vuol dire buttare una lettura
    /// a metà. Vedi `Condiviso::registra_entrante`, che manda via prima quelle
    /// ferme.
    pub(crate) fn al_lavoro(&self) -> bool {
        self.servendo.load(Ordering::SeqCst) || *chiave(&self.in_uscita) > 0
    }

    /// L'ultima volta che una lettura è cominciata o finita qui.
    pub(crate) fn ultimo_lavoro(&self) -> Instant {
        *chiave(&self.ultimo_lavoro)
    }

    /// Chi legge comincia (`true`) o finisce di servire una lettura.
    fn lavora(&self, comincia: bool) {
        self.servendo.store(comincia, Ordering::SeqCst);
        *chiave(&self.ultimo_lavoro) = Instant::now();
    }

    /// Su questa connessione c'è un'associazione in corso.
    pub(crate) fn associando(&self) -> bool {
        chiave(&self.associazione).dal().is_some()
    }

    /// Accende o spegne gli `Evento::Orologio`, e i ping che li producono.
    pub(crate) fn sincronizza(&self, attivo: bool) {
        self.sincronizza.store(attivo, Ordering::SeqCst);
        let _ = self.metti(Uscita::Sincronizza(attivo));
    }

    /// Ricorda un ping appena mandato.
    fn ricorda_ping(&self, seq: u32, t1: u64) {
        let mut ping = chiave(&self.ping);
        ping.push_back((seq, t1));
        while ping.len() > PING_RICORDATI {
            ping.pop_front();
        }
    }

    /// Il `t1` di un ping mandato, togliendolo dall'elenco.
    fn ping_tornato(&self, seq: u32) -> Option<u64> {
        let mut ping = chiave(&self.ping);
        let posto = ping.iter().position(|(numero, _)| *numero == seq)?;
        ping.remove(posto).map(|(_, t1)| t1)
    }
}

/// Il corpo di [`Collegamento::annuncia`]: fa `emetti` se il desktop ha avuto
/// l'`Aperta` e non ancora la `Chiusa`, **tenendo il cancello** finché
/// l'evento non è partito.
///
/// Fino alla revisione del 26 ottobre il controllo era
/// `if *chiave(&annunci) == Annunci::Aperta { … }`: il lucchetto preso nello
/// scrutinio di un `if` è un temporaneo, e se ne va prima del ramo. Fra il
/// controllo e l'invio una `chiudi` da un altro filo poteva quindi mandare la
/// sua `Chiusa`, e l'evento arrivava dopo — un `Associato` per una connessione
/// già chiusa. Con la guardia in una variabile, `chiudi` aspetta che l'evento
/// sia partito.
fn se_aperta(annunci: &Mutex<Annunci>, emetti: impl FnOnce()) {
    let annunci = chiave(annunci);
    if *annunci == Annunci::Aperta {
        emetti();
    }
}

/// Da che parte nasce una connessione.
#[derive(Debug)]
pub(crate) enum Verso {
    /// Accettata dall'ascolto di questa generazione, col primo messaggio
    /// della stretta già letto (vedi `ascolto`).
    Entrante {
        /// La generazione dell'ascolto.
        generazione: u64,
        /// Il primo messaggio della stretta.
        primo: Vec<u8>,
        /// Il posto fra le strette preso dall'ascolto. Lo lascia la stretta,
        /// appena scritto il secondo messaggio e prima di aspettare il terzo:
        /// vedi «Le strette, e chi non parla» in `ascolto`. `None` una volta
        /// passato alla stretta.
        stretta: Option<Stretta>,
    },
    /// Aperta da questo PC.
    Uscente {
        /// Il numero già dato al desktop (per `collega`) o nuovo.
        id: IdConnessione,
        /// A che cosa serve.
        uso: Uso,
        /// Chi ci si aspetta dall'altra parte, se lo si sa.
        atteso: Option<Atteso>,
    },
}

/// Una connessione pronta a vivere: vedi [`vivi`].
pub(crate) struct Pronto {
    /// La connessione, già registrata.
    pub(crate) collegamento: Arc<Collegamento>,
    lettura: TcpStream,
    scrittura: TcpStream,
    decifratore: Decifratore,
    cifratore: Cifratore,
    coda: Receiver<Uscita>,
}

/// Il saluto dell'altro capo.
struct Saluto {
    versione: u32,
    nome: String,
    ospita: bool,
    uso: Uso,
}

/// Il primo messaggio dopo la stretta: un saluto, o un rifiuto al suo posto.
enum PrimaParola {
    Saluto(Saluto),
    Rifiuto(Option<ErrorCodeKind>),
}

/// Legge il primo messaggio dopo la stretta.
fn prima_parola(
    presa: &mut TcpStream,
    decifratore: &mut Decifratore,
    scadenza: Instant,
) -> Result<PrimaParola, Guasto> {
    let (tipo, carico) = decifratore.prossimo_entro(presa, scadenza)?;
    if tipo != Tipo::Json {
        return Err(Guasto::Protocollo);
    }
    match serde_json::from_slice::<Trasporto>(&carico) {
        Ok(Trasporto::Ciao {
            versione,
            nome,
            ospita,
            uso,
        }) => Ok(PrimaParola::Saluto(Saluto {
            versione,
            nome,
            ospita,
            uso,
        })),
        Ok(Trasporto::Chiudo { codice }) => {
            Ok(PrimaParola::Rifiuto(codice.as_deref().and_then(da_testo)))
        }
        _ => Err(Guasto::Protocollo),
    }
}

/// Manda un messaggio subito, prima che esista chi scrive: il saluto, o il
/// rifiuto che prende il suo posto.
fn manda_subito(
    presa: &mut TcpStream,
    cifratore: &mut Cifratore,
    parola: &Trasporto,
    scadenza: Instant,
) -> Result<(), Guasto> {
    cifratore.aggiungi(Tipo::Json, &json(parola))?;
    let resta = scadenza.saturating_duration_since(Instant::now());
    presa.set_write_timeout(Some(resta.max(Duration::from_millis(1))))?;
    cifratore.manda(presa)
}

/// Stretta, saluto, ammissione, registrazione e `Evento::Aperta`.
///
/// # Chi saluta per primo
///
/// Chi apre saluta per primo; chi ospita legge il saluto, decide se far
/// entrare la connessione (i tetti di `Condiviso::registra_entrante`, che
/// dipendono dalla chiave e dall'uso) e risponde col suo saluto o, al suo
/// posto, con un `Chiudo` col perché. Così chi viene rifiutato lo sa subito,
/// con un codice, invece di vedersi aprire e chiudere una connessione.
///
/// # Errori
///
/// Il perché, da dare al desktop in un `Evento::Chiusa` se la connessione era
/// sua: `None` se si è fermata la rete o se l'ha chiusa lui prima che si
/// aprisse.
pub(crate) fn prepara(
    rete: &Arc<Condiviso>,
    mut presa: TcpStream,
    indirizzo: SocketAddr,
    mut verso: Verso,
) -> Result<Pronto, Option<ErrorCodeKind>> {
    let in_entrata = matches!(verso, Verso::Entrante { .. });
    // Dal lato di chi apre, una stretta che non va a buon fine vuol dire che
    // quel PC non si raggiunge (o che non è Rymembra); dal lato di chi ospita è
    // solo una connessione che è caduta.
    let caduta = if in_entrata {
        ErrorCodeKind::InsiemeConnessioneCaduta
    } else {
        ErrorCodeKind::InsiemeIrraggiungibile
    };
    let guasto = |g: Guasto| {
        Some(match g {
            Guasto::Protocollo => ErrorCodeKind::InsiemeProtocollo,
            Guasto::Silenzio | Guasto::Rete => caduta,
        })
    };
    let _ = presa.set_nodelay(true);
    let scadenza = Instant::now() + rete.tempi.stretta;
    let (primo, stretta) = match &mut verso {
        Verso::Entrante { primo, stretta, .. } => (Some(primo.as_slice()), stretta.take()),
        Verso::Uscente { .. } => (None, None),
    };
    // Il posto fra le strette se ne va appena finito il lavoro sulla curva, e
    // non a stretta finita: vedi «Le strette, e chi non parla» in `ascolto`.
    let canale = canale::stringi(
        &mut presa,
        &rete.identita,
        !in_entrata,
        primo,
        move || drop(stretta),
        scadenza,
    )
    .map_err(guasto)?;
    let (pari, hash) = (canale.pari, canale.hash);
    let (mut cifratore, mut decifratore) = canale.dividi();
    // Il saluto è piccolo per tutti: i tetti di un estraneo valgono qui per
    // chiunque, e niente byte grezzi.
    decifratore.imposta_tetti(TETTO_ESTRANEO, 0, 0);
    if let Verso::Uscente {
        atteso: Some(atteso),
        ..
    } = &verso
        && !atteso.corrisponde(&pari)
    {
        // All'indirizzo c'è un Rymembra, ma non quello che si cercava: per chi
        // lo cercava, quello giusto non si raggiunge.
        return Err(Some(ErrorCodeKind::InsiemeIrraggiungibile));
    }
    if rete.bloccato(&pari) {
        return Err(Some(ErrorCodeKind::InsiemeNonAssociato));
    }
    let uso_mio = match &verso {
        Verso::Uscente { uso, .. } => *uso,
        // Chi accetta non sceglie l'uso: lo dichiara chi apre.
        Verso::Entrante { .. } => Uso::Controllo,
    };
    let mio_saluto = Trasporto::Ciao {
        versione: VERSIONE,
        nome: rete.nome(),
        ospita: rete.ospita_ora(),
        uso: uso_mio,
    };
    if !in_entrata {
        manda_subito(&mut presa, &mut cifratore, &mio_saluto, scadenza).map_err(guasto)?;
    }
    let saluto = match prima_parola(&mut presa, &mut decifratore, scadenza).map_err(guasto)? {
        PrimaParola::Saluto(saluto) => saluto,
        // Chi ospita ha detto di no al posto del suo saluto: vale il suo
        // perché. Chi bussa non ha diritto a rifiutare così.
        PrimaParola::Rifiuto(codice) if !in_entrata => return Err(codice),
        PrimaParola::Rifiuto(_) => return Err(Some(ErrorCodeKind::InsiemeProtocollo)),
    };
    if saluto.versione != VERSIONE {
        // L'altro capo se ne accorge da sé leggendo il nostro saluto (che chi
        // ospita manda lo stesso, perché la versione la dice lui); il
        // `Chiudo` c'è perché il suo perché sia quello giusto anche se il
        // nostro saluto si fosse perso.
        let chiudo = Trasporto::Chiudo {
            codice: Some(ErrorCodeKind::InsiemeVersione.code().to_owned()),
        };
        if in_entrata {
            let _ = manda_subito(&mut presa, &mut cifratore, &mio_saluto, scadenza);
        }
        let _ = manda_subito(&mut presa, &mut cifratore, &chiudo, scadenza);
        return Err(Some(ErrorCodeKind::InsiemeVersione));
    }
    if !nome_valido(&saluto.nome) {
        return Err(Some(ErrorCodeKind::InsiemeProtocollo));
    }
    let uso = if in_entrata { saluto.uso } else { uso_mio };

    let scrittura = presa.try_clone().map_err(|_| Some(caduta))?;
    let maniglia = presa.try_clone().map_err(|_| Some(caduta))?;
    let (uscita, coda) = sync_channel(CODA);
    let id = match &verso {
        Verso::Uscente { id, .. } => *id,
        Verso::Entrante { .. } => rete.nuovo_id(),
    };
    let collegamento = Arc::new(Collegamento {
        id,
        uso,
        in_entrata,
        pari,
        ip: indirizzo.ip(),
        aperta: Instant::now(),
        hash,
        rete: Arc::downgrade(rete),
        annunci: Mutex::new(Annunci::Nessuno),
        uscita,
        in_uscita: Mutex::new(0),
        posto_libero: Condvar::new(),
        presa: maniglia,
        chiusura: Mutex::new(None),
        lettore_finito: AtomicBool::new(false),
        sincronizza: AtomicBool::new(false),
        ping: Mutex::new(VecDeque::new()),
        servendo: AtomicBool::new(false),
        ultimo_lavoro: Mutex::new(Instant::now()),
        ultimo_stato: AtomicU64::new(0),
        associazione: Mutex::new(Associazione::Nessuna),
        attese: Mutex::new(HashMap::new()),
    });
    match verso {
        Verso::Entrante { generazione, .. } => {
            let via = match rete.registra_entrante(Arc::clone(&collegamento), generazione) {
                Ok(via) => via,
                Err(codice) => {
                    let chiudo = Trasporto::Chiudo {
                        codice: codice.map(|k| k.code().to_owned()),
                    };
                    let _ = manda_subito(&mut presa, &mut cifratore, &chiudo, scadenza);
                    return Err(codice);
                }
            };
            // Senza un perché: è lo stesso PC che si è ricollegato, o una
            // connessione di lettura o di servizio che il membro ha già
            // sostituito. Vedi `None` in `Evento::Chiusa`.
            for vecchia in via {
                vecchia.chiudi(None, Modo::Cortese);
            }
            if manda_subito(&mut presa, &mut cifratore, &mio_saluto, scadenza).is_err() {
                // Registrata e mai partita: si toglie, e non se ne parla.
                collegamento.chiudi(Some(caduta), Modo::Taglio);
                rete.togli(id);
                return Err(Some(caduta));
            }
        }
        Verso::Uscente { .. } => {
            if !rete.registra(Arc::clone(&collegamento), None) {
                return Err(None);
            }
        }
    }
    // Una connessione di `collega` chiusa dal desktop mentre si apriva: si
    // registra lo stesso — così vive e muore per la strada di tutte — ma
    // senza `Aperta`, e chiusa subito col perché del desktop. Vedi
    // `Rete::chiudi` per l'ordine dei lucchetti che rende questo controllo
    // sicuro.
    let annullata = if in_entrata {
        None
    } else {
        chiave(&rete.in_apertura).remove(&id).flatten()
    };
    if let Some(codice) = annullata {
        collegamento.chiudi(codice, Modo::Cortese);
    } else if collegamento.uso == Uso::Controllo {
        collegamento.annuncia_apertura(
            rete,
            Evento::Aperta {
                id,
                pari,
                nome: saluto.nome,
                ospita: saluto.ospita,
                in_entrata,
                indirizzo,
            },
        );
    }
    Ok(Pronto {
        collegamento,
        lettura: presa,
        scrittura,
        decifratore,
        cifratore,
        coda,
    })
}

/// La vita di una connessione: lancia chi scrive, legge in questo filo fino
/// alla fine, fa le pulizie.
pub(crate) fn vivi(rete: &Arc<Condiviso>, pronto: Pronto) {
    let Pronto {
        collegamento,
        lettura,
        scrittura,
        decifratore,
        cifratore,
        coda,
    } = pronto;
    let per_chi_scrive = Arc::clone(&collegamento);
    let partito = rete.filo("scrive", move |rete| {
        scrivi_finche(&rete, &per_chi_scrive, scrittura, cifratore, &coda);
    });
    if !partito {
        collegamento.chiudi(Some(ErrorCodeKind::InsiemeConnessioneCaduta), Modo::Taglio);
    }
    leggi_finche(rete, &collegamento, lettura, decifratore);

    // Le pulizie, nell'ordine in cui servono: prima il segnale a chi scrive
    // (che smette di aspettare la nostra fine), poi la chiusura se nessuno
    // l'aveva ancora decisa (l'altro capo è sparito: è qui che lo si
    // scopre), poi chi aspetta una risposta (che si sveglia col canale
    // chiuso), poi l'elenco.
    collegamento.lettore_finito.store(true, Ordering::SeqCst);
    collegamento.chiudi(Some(ErrorCodeKind::InsiemeConnessioneCaduta), Modo::Muto);
    chiave(&collegamento.attese).clear();
    rete.togli(collegamento.id);
}

/// Perché una connessione va chiusa dopo un messaggio.
#[derive(Debug, Clone, Copy)]
struct Fine {
    codice: Option<ErrorCodeKind>,
    modo: Modo,
}

impl Fine {
    /// Si chiude dicendo perché.
    const fn con(codice: ErrorCodeKind) -> Self {
        Self {
            codice: Some(codice),
            modo: Modo::Cortese,
        }
    }

    /// Il caso più comune: l'altro capo non parla questo protocollo.
    const fn protocollo() -> Self {
        Self::con(ErrorCodeKind::InsiemeProtocollo)
    }
}

/// Un `Pezzo` o un'`Immagine` di cui stanno arrivando i byte.
struct InArrivo {
    lettura: u32,
    totale: u64,
    attesi: usize,
    byte: Vec<u8>,
}

/// I tetti dei messaggi in arrivo da questo capo, adesso. Vedi «Chi può dire
/// che cosa».
///
/// JSON, byte grezzi, JSON compressi. Da un estraneo, compressi niente: vedi
/// «La compressione» in `cornice`.
fn tetti(rete: &Condiviso, collegamento: &Collegamento) -> (usize, usize, usize) {
    if rete.fidato(&collegamento.pari) {
        return (MASSIMO_MESSAGGIO, MASSIMO_MESSAGGIO, MASSIMO_MESSAGGIO);
    }
    // Da un estraneo i byte grezzi arrivano solo come risposta a una lettura
    // che questo PC ha chiesto: chi legge ha scelto la chiave da cui leggere.
    let byte = if collegamento.uso == Uso::Lettura && !collegamento.in_entrata {
        MASSIMO_MESSAGGIO
    } else {
        0
    };
    (TETTO_ESTRANEO, byte, 0)
}

/// Chi legge: fino alla fine della connessione.
fn leggi_finche(
    rete: &Arc<Condiviso>,
    collegamento: &Arc<Collegamento>,
    mut presa: TcpStream,
    mut decifratore: Decifratore,
) {
    let mut in_arrivo: Option<InArrivo> = None;
    // I tetti si decidono a ogni messaggio, quando ne arriva la testa e non
    // prima di aspettarla: un estraneo che si associa diventa fidato a metà
    // connessione — magari mentre questo filo aspetta — e uno dimenticato il
    // contrario. Vedi `Decifratore::prossimo_coi_tetti`.
    let tetti_adesso = || tetti(rete, collegamento);
    loop {
        let (tipo, carico, arrivo) = match decifratore.prossimo_coi_tetti(
            &mut presa,
            rete.tempi.silenzio,
            rete.adesso_ns,
            &tetti_adesso,
        ) {
            Ok(messaggio) => messaggio,
            Err(guasto) => {
                let fine = match guasto {
                    Guasto::Protocollo => Fine::protocollo(),
                    Guasto::Silenzio | Guasto::Rete => Fine {
                        codice: Some(ErrorCodeKind::InsiemeConnessioneCaduta),
                        modo: Modo::Muto,
                    },
                };
                collegamento.chiudi(fine.codice, fine.modo);
                return;
            }
        };
        // Durante il congedo si legge e si butta: vedi «Come muore».
        if collegamento.chiusa() {
            continue;
        }
        let esito = match tipo {
            Tipo::Byte => allegato(collegamento, &mut in_arrivo, carico),
            // I byte di un `Pezzo` arrivano di seguito, senza niente in mezzo:
            // un JSON prima della fine è un capo che non sa contare.
            Tipo::Json if in_arrivo.is_some() => Err(Fine::protocollo()),
            // Arrivano già decompressi (vedi `Decifratore::prossimo_coi_tetti`):
            // uno compresso qui è un errore di questo crate, non dell'altro capo,
            // ma non si legge lo stesso.
            Tipo::JsonCompresso => Err(Fine::protocollo()),
            Tipo::Json => match serde_json::from_slice::<Trasporto>(&carico) {
                Ok(parola) => tratta(rete, collegamento, parola, arrivo, &mut in_arrivo),
                Err(_) => Err(Fine::protocollo()),
            },
        };
        match esito {
            Err(fine) => collegamento.chiudi(fine.codice, fine.modo),
            Ok(()) => scadenze(rete, collegamento),
        }
    }
}

/// Un messaggio del trasporto.
fn tratta(
    rete: &Arc<Condiviso>,
    collegamento: &Arc<Collegamento>,
    parola: Trasporto,
    arrivo: u64,
    in_arrivo: &mut Option<InArrivo>,
) -> Result<(), Fine> {
    let controllo = || {
        if collegamento.uso == Uso::Controllo {
            Ok(())
        } else {
            Err(Fine::protocollo())
        }
    };
    // Chi serve le letture: l'host sulle connessioni di lettura che gli
    // arrivano, il membro su quella di servizio che ha aperto lui.
    let servente = || match (collegamento.uso, collegamento.in_entrata) {
        (Uso::Lettura, true) | (Uso::Servizio, false) => Ok(()),
        _ => Err(Fine::protocollo()),
    };
    // E chi le riceve: il contrario.
    let cliente = || match (collegamento.uso, collegamento.in_entrata) {
        (Uso::Lettura, false) | (Uso::Servizio, true) => Ok(()),
        _ => Err(Fine::protocollo()),
    };
    match parola {
        Trasporto::Ciao { .. } => Err(Fine::protocollo()),
        Trasporto::Battito | Trasporto::Sconosciuto => Ok(()),
        Trasporto::Chiudo { codice } => Err(Fine {
            codice: codice.as_deref().and_then(da_testo),
            modo: Modo::Muto,
        }),
        Trasporto::Ping { seq, t1 } => {
            controllo()?;
            // L'orologio è per chi è fidato: a uno sconosciuto non si dice
            // nemmeno che ore sono.
            if rete.fidato(&collegamento.pari) {
                let _ = collegamento.metti(Uscita::Pong {
                    seq,
                    t1,
                    t2: arrivo,
                });
            }
            Ok(())
        }
        Trasporto::Pong { seq, t1, t2, t3 } => {
            controllo()?;
            pong(rete, collegamento, seq, (t1, t2, t3, arrivo));
            Ok(())
        }
        Trasporto::Impegno { impegno } => {
            controllo()?;
            impegno_ricevuto(rete, collegamento, impegno.0)
        }
        Trasporto::Nonce { nonce } => {
            controllo()?;
            nonce_ricevuto(rete, collegamento, nonce.0)
        }
        Trasporto::Occupato => {
            controllo()?;
            occupato_ricevuto(rete, collegamento)
        }
        Trasporto::Rivela { nonce } => {
            controllo()?;
            rivelazione_ricevuta(rete, collegamento, nonce.0)
        }
        Trasporto::Conferma { uguale } => {
            controllo()?;
            conferma_ricevuta(rete, collegamento, uguale)
        }
        Trasporto::Leggi {
            lettura,
            gettone,
            da,
            quanti,
        } => {
            servente()?;
            if quanti > LETTURA_MASSIMA {
                return Err(Fine::protocollo());
            }
            servi(
                rete,
                collegamento,
                lettura,
                &gettone,
                Chiesto::Byte { da, quanti },
            );
            Ok(())
        }
        Trasporto::Copertina { lettura, gettone } => {
            servente()?;
            servi(rete, collegamento, lettura, &gettone, Chiesto::Copertina);
            Ok(())
        }
        Trasporto::Pezzo {
            lettura,
            totale,
            byte,
        } => {
            cliente()?;
            comincia(
                collegamento,
                in_arrivo,
                lettura,
                totale,
                byte,
                LETTURA_MASSIMA,
            )
        }
        Trasporto::Immagine { lettura, byte } => {
            cliente()?;
            let tetto = u64::try_from(COPERTINA_MASSIMA).unwrap_or(u64::MAX);
            comincia(collegamento, in_arrivo, lettura, byte, byte, tetto)
        }
        Trasporto::LetturaRifiutata { lettura, codice } => {
            cliente()?;
            let codice = da_testo(&codice).unwrap_or(ErrorCodeKind::InsiemeProtocollo);
            consegna(collegamento, lettura, Risposta::Rifiutata(codice));
            Ok(())
        }
        Trasporto::App { messaggio } => {
            controllo()?;
            app(rete, collegamento, messaggio)
        }
    }
}

/// Un messaggio dell'applicazione: solo da chi è fidato, e solo nei tetti.
fn app(rete: &Condiviso, collegamento: &Collegamento, messaggio: Messaggio) -> Result<(), Fine> {
    if !rete.fidato(&collegamento.pari) {
        return Err(Fine::con(ErrorCodeKind::InsiemeNonAssociato));
    }
    if messaggio.valida().is_err() {
        return Err(Fine::protocollo());
    }
    if matches!(messaggio, Messaggio::Sconosciuto) {
        return Ok(());
    }
    collegamento.annuncia(
        rete,
        Evento::Ricevuto {
            id: collegamento.id,
            messaggio,
        },
    );
    Ok(())
}

/// Un pong: se torna da un ping nostro e i suoi istanti stanno in piedi,
/// diventa un campione.
fn pong(rete: &Condiviso, collegamento: &Collegamento, seq: u32, istanti: (u64, u64, u64, u64)) {
    let (t1, t2, t3, t4) = istanti;
    // Il `t1` di riferimento è il nostro, non quello ricopiato: un pong che
    // non corrisponde a un ping mandato da qui non è un campione.
    let Some(nostro) = collegamento.ping_tornato(seq) else {
        return;
    };
    if nostro != t1 || t1 > t4 || t2 > t3 {
        return;
    }
    if !collegamento.sincronizza.load(Ordering::SeqCst) {
        return;
    }
    collegamento.annuncia(
        rete,
        Evento::Orologio {
            id: collegamento.id,
            campione: CampioneOrologio { t1, t2, t3, t4 },
        },
    );
}

/// Chi legge sta chiedendo i gettoni a buon diritto?
///
/// - Sull'host, sulle connessioni di lettura: deve essere fidato **e**
///   ammesso, cioè nella sessione (`Rete::ammessi`). Fidato e basta non
///   bastava: un PC associato mesi fa, fuori dalla sessione, che conosce un
///   gettone (l'ha visto in uno stato quando era dentro) poteva leggerlo.
/// - Sul membro, sulla connessione di servizio che ha aperto lui con
///   `Rete::offri`: chi chiede è l'host a cui l'ha offerta — la stretta l'ha
///   dimostrato — e deve essere fidato. Gli ammessi qui non contano: il
///   membro non ospita, e la sua sessione è quella dell'host.
fn ha_diritto(rete: &Condiviso, collegamento: &Collegamento) -> bool {
    match (collegamento.uso, collegamento.in_entrata) {
        (Uso::Lettura, true) => rete.fidato(&collegamento.pari) && rete.ammesso(&collegamento.pari),
        (Uso::Servizio, false) => rete.fidato(&collegamento.pari),
        _ => false,
    }
}

/// Una lettura chiesta a questo PC: si risponde sempre, con i byte o con un
/// rifiuto.
fn servi(
    rete: &Arc<Condiviso>,
    collegamento: &Arc<Collegamento>,
    lettura: u32,
    gettone: &Gettone,
    chiesto: Chiesto,
) {
    // Prima il posto in coda, poi il disco: chi non legge le risposte non fa
    // leggere niente. Vedi «La coda d'uscita, in byte».
    let massimo = usize::try_from(LETTURA_MASSIMA)
        .unwrap_or(usize::MAX)
        .saturating_add(PESO_TESTA);
    collegamento.lavora(true);
    if !collegamento.aspetta_posto(massimo) {
        collegamento.lavora(false);
        return;
    }
    let (risposta, generazione) = if ha_diritto(rete, collegamento) {
        // Sull'host, una lettura che arriva su una connessione di lettura può
        // essere inoltrata al proprietario; una che arriva su quella di
        // servizio no: la chiede l'host a un membro, e un membro non inoltra a
        // nessuno. Senza questa regola due PC potrebbero rimandarsi la stessa
        // lettura all'infinito.
        let inoltrabile = collegamento.uso == Uso::Lettura;
        let chi = Arc::clone(collegamento);
        let per_chi = Arc::clone(rete);
        leggi_gettone(
            rete,
            gettone,
            chiesto,
            &move || !chi.chiusa() && !per_chi.fermo(),
            inoltrabile,
        )
    } else {
        // Lo stesso rifiuto di un gettone revocato: a chi non è nella
        // sessione non si dice se il gettone esiste.
        (
            Risposta::Rifiutata(ErrorCodeKind::InsiemeGettoneScaduto),
            None,
        )
    };
    let voce = match (risposta, generazione) {
        (Risposta::Byte { totale, byte }, Some(generazione)) => Uscita::Dati(Dati {
            lettura,
            totale: match chiesto {
                Chiesto::Copertina => None,
                Chiesto::Byte { .. } => Some(totale),
            },
            byte,
            gettone: *gettone,
            generazione,
        }),
        (Risposta::Byte { .. }, None) => rifiuto(lettura, ErrorCodeKind::InsiemeGettoneScaduto),
        (Risposta::Rifiutata(codice), _) => rifiuto(lettura, codice),
    };
    // Se non entra, la connessione si sta chiudendo: chi aspettava la
    // risposta lo saprà da lì.
    let _ = collegamento.metti(voce);
    collegamento.lavora(false);
}

/// Un rifiuto di lettura, pronto per la coda.
fn rifiuto(lettura: u32, codice: ErrorCodeKind) -> Uscita {
    Uscita::Json(json(&Trasporto::LetturaRifiutata {
        lettura,
        codice: codice.code().to_owned(),
    }))
}

/// I byte di un gettone di questo PC: dal disco, dalla memoria, o (fase 2,
/// sull'host) dal membro che li possiede. Con la generazione del gettone
/// quando lo si è letto, se c'era.
///
/// `ancora` dice se chi ha chiesto li vuole ancora: serve all'attesa di un
/// inoltro, che può durare.
pub(crate) fn leggi_gettone(
    rete: &Arc<Condiviso>,
    gettone: &Gettone,
    chiesto: Chiesto,
    ancora: &dyn Fn() -> bool,
    inoltrabile: bool,
) -> (Risposta, Option<u64>) {
    let Some((generazione, servito)) = chiave(&rete.servente).cerca(gettone) else {
        return (
            Risposta::Rifiutata(ErrorCodeKind::InsiemeGettoneScaduto),
            None,
        );
    };
    let risposta = match (servito, chiesto) {
        (Servito::File(file), Chiesto::Byte { da, quanti }) => {
            match leggi_file(&file, da, quanti) {
                Ok((byte, totale)) => Risposta::Byte { totale, byte },
                Err(codice) => Risposta::Rifiutata(codice),
            }
        }
        (Servito::Lettore(lettore), Chiesto::Byte { da, quanti }) => {
            match (lettore.0)(da, quanti.min(crate::servente::LETTURA_MASSIMA)) {
                Ok((byte, totale)) => Risposta::Byte { totale, byte },
                Err(codice) => Risposta::Rifiutata(codice),
            }
        }
        // Un gettone di un file non dà diritto a una copertina: non sono la
        // stessa cosa, e chi chiede così sta sbagliando richiesta.
        (Servito::File(_) | Servito::Lettore(_), Chiesto::Copertina) => {
            Risposta::Rifiutata(ErrorCodeKind::InsiemeProtocollo)
        }
        (Servito::Copertina(byte), chiesto) => leggi_copertina(&byte, chiesto),
        (Servito::Inoltrato(proprietario), chiesto) if inoltrabile => {
            inoltra(rete, &proprietario, gettone, chiesto, ancora)
        }
        (Servito::Inoltrato(_), _) => Risposta::Rifiutata(ErrorCodeKind::InsiemeGettoneScaduto),
    };
    // Vedi «La revoca» in `servente`.
    if chiave(&rete.servente).generazione(gettone) != Some(generazione) {
        return (
            Risposta::Rifiutata(ErrorCodeKind::InsiemeGettoneScaduto),
            None,
        );
    }
    (risposta, Some(generazione))
}

/// Fase 2, sull'host: chiede i byte al proprietario sulla sua connessione di
/// servizio, e aspetta.
fn inoltra(
    rete: &Arc<Condiviso>,
    proprietario: &ChiavePubblica,
    gettone: &Gettone,
    chiesto: Chiesto,
    ancora: &dyn Fn() -> bool,
) -> Risposta {
    let servizio = rete
        .connessioni_dove(|c| {
            c.uso == Uso::Servizio && c.in_entrata && c.pari == *proprietario && !c.chiusa()
        })
        .into_iter()
        .next();
    let Some(servizio) = servizio else {
        return Risposta::Rifiutata(ErrorCodeKind::InsiemeIrraggiungibile);
    };
    let lettura = rete.prossima_lettura.fetch_add(1, Ordering::Relaxed);
    let (dai, prendi) = sync_channel(1);
    chiave(&servizio.attese).insert(lettura, dai);
    let parola = match chiesto {
        Chiesto::Byte { da, quanti } => Trasporto::Leggi {
            lettura,
            gettone: *gettone,
            da,
            quanti,
        },
        Chiesto::Copertina => Trasporto::Copertina {
            lettura,
            gettone: *gettone,
        },
    };
    if !servizio.metti(Uscita::Json(json(&parola))) {
        chiave(&servizio.attese).remove(&lettura);
        return Risposta::Rifiutata(ErrorCodeKind::InsiemeConnessioneCaduta);
    }
    let inizio = Instant::now();
    loop {
        match prendi.recv_timeout(RESPIRO_LETTURA) {
            Ok(risposta) => return risposta,
            Err(RecvTimeoutError::Disconnected) => {
                return Risposta::Rifiutata(ErrorCodeKind::InsiemeConnessioneCaduta);
            }
            Err(RecvTimeoutError::Timeout) => {
                if !ancora() || inizio.elapsed() > rete.tempi.inoltro {
                    // La risposta, se arriva dopo, trova il posto vuoto e si
                    // butta: vedi `consegna`.
                    chiave(&servizio.attese).remove(&lettura);
                    return Risposta::Rifiutata(ErrorCodeKind::InsiemeIrraggiungibile);
                }
            }
        }
    }
}

/// Comincia a ricevere i byte di un `Pezzo` o di un'`Immagine`.
fn comincia(
    collegamento: &Collegamento,
    in_arrivo: &mut Option<InArrivo>,
    lettura: u32,
    totale: u64,
    byte: u64,
    tetto: u64,
) -> Result<(), Fine> {
    // Il tetto prima di tutto: vedi «Il controllo prima dell'allocazione» in
    // `cornice`. Qui la memoria la riserva il primo allegato.
    if byte > tetto {
        return Err(Fine::protocollo());
    }
    let attesi = usize::try_from(byte).map_err(|_| Fine::protocollo())?;
    if attesi == 0 {
        consegna(
            collegamento,
            lettura,
            Risposta::Byte {
                totale,
                byte: Vec::new(),
            },
        );
        return Ok(());
    }
    *in_arrivo = Some(InArrivo {
        lettura,
        totale,
        attesi,
        byte: Vec::new(),
    });
    Ok(())
}

/// Un messaggio di byte grezzi: il pezzo successivo di quel che sta arrivando.
fn allegato(
    collegamento: &Collegamento,
    in_arrivo: &mut Option<InArrivo>,
    carico: Vec<u8>,
) -> Result<(), Fine> {
    let Some(mut voce) = in_arrivo.take() else {
        return Err(Fine::protocollo());
    };
    if voce.byte.len().saturating_add(carico.len()) > voce.attesi {
        return Err(Fine::protocollo());
    }
    if voce.byte.is_empty() && carico.len() == voce.attesi {
        // Il caso comune, una copertina o una lettura sotto il mebibyte: il
        // messaggio è già la risposta, senza copiarlo.
        voce.byte = carico;
    } else {
        voce.byte
            .reserve_exact(voce.attesi.saturating_sub(voce.byte.len()));
        voce.byte.extend_from_slice(&carico);
    }
    if voce.byte.len() == voce.attesi {
        consegna(
            collegamento,
            voce.lettura,
            Risposta::Byte {
                totale: voce.totale,
                byte: voce.byte,
            },
        );
    } else {
        *in_arrivo = Some(voce);
    }
    Ok(())
}

/// Dà la risposta a chi la sta aspettando. Se non la aspetta più nessuno —
/// annullata, scaduta — si butta.
fn consegna(collegamento: &Collegamento, lettura: u32, risposta: Risposta) {
    if let Some(chi) = chiave(&collegamento.attese).remove(&lettura) {
        let _ = chi.try_send(risposta);
    }
}

/// Chi scrive: fino alla fine della connessione.
fn scrivi_finche(
    rete: &Condiviso,
    collegamento: &Collegamento,
    mut presa: TcpStream,
    mut cifratore: Cifratore,
    coda: &Receiver<Uscita>,
) {
    let tempi = rete.tempi;
    // La scadenza di ogni scrittura: chi non legge per un silenzio intero non
    // c'è più. Vedi «Le scadenze» in `canale`: scaduta, si chiude.
    let _ = presa.set_write_timeout(Some(tempi.silenzio));
    let mut ultimo = Instant::now();
    // I ping, se accesi: da quando, e quando tocca al prossimo.
    let mut orologio: Option<(Instant, Instant)> = None;
    let mut seq = 0_u32;
    loop {
        // Copiata fuori prima dell'`if let`: il lucchetto di un temporaneo
        // nello scrutinio vivrebbe per tutto il ramo, cioè per tutto il
        // congedo, e chi chiama `metti` resterebbe fermo ad aspettarlo.
        let chiusura = *chiave(&collegamento.chiusura);
        if let Some(chiusura) = chiusura {
            congeda(
                rete,
                collegamento,
                &mut presa,
                &mut cifratore,
                coda,
                chiusura,
            );
            return;
        }
        let adesso = Instant::now();
        // Il peso della voce mandata, da togliere dalla coda dopo averla
        // scritta: chi aspetta posto lo riprende solo quando i byte sono
        // davvero partiti.
        let mut peso = 0;
        // I tempi scaduti vengono prima della coda: un ping non aspetta che la
        // coda si svuoti, e il suo `t1` è comunque preso ora.
        let esito = if let Some((dal, prossimo)) = orologio
            && adesso >= prossimo
        {
            seq = seq.wrapping_add(1);
            let passo = if adesso.saturating_duration_since(dal) < tempi.orologio_avvio {
                tempi.ping_svelto
            } else {
                tempi.ping_lento
            };
            orologio = Some((dal, adesso + passo));
            let t1 = (rete.adesso_ns)();
            collegamento.ricorda_ping(seq, t1);
            cifratore.aggiungi(Tipo::Json, &json(&Trasporto::Ping { seq, t1 }))
        } else if adesso.saturating_duration_since(ultimo) >= tempi.battito {
            cifratore.aggiungi(Tipo::Json, &json(&Trasporto::Battito))
        } else {
            let mut sveglia = ultimo + tempi.battito;
            if let Some((_, prossimo)) = orologio {
                sveglia = sveglia.min(prossimo);
            }
            match coda.recv_timeout(sveglia.saturating_duration_since(adesso)) {
                Ok(Uscita::Sincronizza(attivo)) => {
                    // Ogni accensione riparte con la cadenza svelta: è quando
                    // lo stimatore del desktop non sa ancora niente.
                    collegamento.scritto(PESO_TESTA);
                    orologio = attivo.then_some((adesso, adesso));
                    chiave(&collegamento.ping).clear();
                    continue;
                }
                Ok(Uscita::Sveglia) | Err(RecvTimeoutError::Timeout) => continue,
                Ok(voce) => {
                    peso = voce.peso();
                    cifra_voce(rete, collegamento, &mut cifratore, voce)
                }
                Err(RecvTimeoutError::Disconnected) => Err(Guasto::Rete),
            }
        };
        let scritta = esito.and_then(|()| cifratore.manda(&mut presa));
        collegamento.scritto(peso);
        if scritta.is_err() {
            collegamento.chiudi(Some(ErrorCodeKind::InsiemeConnessioneCaduta), Modo::Taglio);
            return;
        }
        ultimo = Instant::now();
    }
}

/// Cifra una voce della coda.
fn cifra_voce(
    rete: &Condiviso,
    collegamento: &Collegamento,
    cifratore: &mut Cifratore,
    voce: Uscita,
) -> Result<(), Guasto> {
    match voce {
        Uscita::Json(json) => cifratore.aggiungi(Tipo::Json, &json),
        // Uno stato con dietro uno più nuovo non parte: vedi «Gli stati in
        // coda».
        Uscita::App {
            stato: Some(numero),
            ..
        } if numero != collegamento.ultimo_stato.load(Ordering::SeqCst) => Ok(()),
        Uscita::App {
            byte, compresso, ..
        } => {
            let tipo = if compresso {
                Tipo::JsonCompresso
            } else {
                Tipo::Json
            };
            cifratore.aggiungi(tipo, &byte)
        }
        Uscita::Pong { seq, t1, t2 } => {
            // `t3` adesso: dopo l'attesa in coda, subito prima di cifrare e
            // scrivere.
            let t3 = (rete.adesso_ns)();
            cifratore.aggiungi(Tipo::Json, &json(&Trasporto::Pong { seq, t1, t2, t3 }))
        }
        Uscita::Dati(dati) => {
            // L'ultimo controllo della revoca, prima di cifrare: vedi `Dati`.
            if chiave(&rete.servente).generazione(&dati.gettone) != Some(dati.generazione) {
                let scaduto = Trasporto::LetturaRifiutata {
                    lettura: dati.lettura,
                    codice: ErrorCodeKind::InsiemeGettoneScaduto.code().to_owned(),
                };
                return cifratore.aggiungi(Tipo::Json, &json(&scaduto));
            }
            let quanti = u64::try_from(dati.byte.len()).unwrap_or(u64::MAX);
            let testa = match dati.totale {
                Some(totale) => Trasporto::Pezzo {
                    lettura: dati.lettura,
                    totale,
                    byte: quanti,
                },
                None => Trasporto::Immagine {
                    lettura: dati.lettura,
                    byte: quanti,
                },
            };
            cifratore.aggiungi(Tipo::Json, &json(&testa))?;
            for pezzo in dati.byte.chunks(MASSIMO_MESSAGGIO) {
                cifratore.aggiungi(Tipo::Byte, pezzo)?;
            }
            Ok(())
        }
        Uscita::Sincronizza(_) | Uscita::Sveglia => Ok(()),
    }
}

/// Il congedo di chi scrive. Vedi «Come muore».
fn congeda(
    rete: &Condiviso,
    collegamento: &Collegamento,
    presa: &mut TcpStream,
    cifratore: &mut Cifratore,
    coda: &Receiver<Uscita>,
    chiusura: Chiusura,
) {
    if chiusura.modo != Modo::Taglio {
        // Quel che era in coda parte: un `Fine` mandato dal desktop e subito
        // seguito da `chiudi` deve arrivare, o l'altro PC non saprebbe che la
        // sessione è finita invece che caduta.
        let mut esito = Ok(());
        while let Ok(voce) = coda.try_recv() {
            esito = esito.and_then(|()| cifra_voce(rete, collegamento, cifratore, voce));
        }
        if chiusura.modo == Modo::Cortese {
            let chiudo = Trasporto::Chiudo {
                codice: chiusura.codice.map(|k| k.code().to_owned()),
            };
            esito = esito.and_then(|()| cifratore.aggiungi(Tipo::Json, &json(&chiudo)));
        }
        let _ = esito.and_then(|()| cifratore.manda(presa));
    }
    let _ = presa.shutdown(Shutdown::Write);
    // Si aspetta che l'altro capo chiuda la sua metà, cioè che chi legge
    // esca: chiudere il socket prima, con dati non letti, manderebbe un reset
    // che può cancellare il `Chiudo` appena scritto.
    let fine = Instant::now() + rete.tempi.congedo;
    while !collegamento.lettore_finito.load(Ordering::SeqCst) && Instant::now() < fine {
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = presa.shutdown(Shutdown::Both);
}

/// Le scadenze di una connessione, guardate dopo ogni messaggio: i battiti ne
/// portano uno ogni cinque secondi anche quando non succede niente, e per
/// scadenze di minuti è precisione che avanza.
///
/// - Un'associazione oltre `Tempi::associazione` è fallita, e si chiude con
///   `insieme.associazioneScaduta` — da tutti e due i capi, perché tutti e
///   due tengono il tempo, e il primo che scade lo dice all'altro nel
///   `Chiudo`. Fino alla revisione del 26 ottobre si chiudeva con
///   `insieme.codiceRifiutato`, che dice a due persone che si erano solo
///   distratte che i codici non coincidevano.
/// - Un estraneo in entrata che non si sta associando vive al più
///   `Tempi::vita_estraneo`: il tempo di vedere chi ospita e decidere. Poi si
///   chiude con `insieme.estraneoScaduto`, e se vuole si ricollega — è quel
///   che fa il desktop con gli host che vede. Un'associazione in corso
///   (l'impegno arrivato, il codice sugli schermi) lo tiene in vita: ha il
///   suo tempo, quello qui sopra, e tagliarla a metà vorrebbe dire togliere
///   il codice dagli schermi mentre due persone lo stanno confrontando.
fn scadenze(rete: &Condiviso, collegamento: &Collegamento) {
    let dal = chiave(&collegamento.associazione).dal();
    if dal.is_some_and(|dal| dal.elapsed() > rete.tempi.associazione) {
        collegamento.chiudi(
            Some(ErrorCodeKind::InsiemeAssociazioneScaduta),
            Modo::Cortese,
        );
        return;
    }
    if dal.is_none()
        && collegamento.in_entrata
        && collegamento.aperta.elapsed() > rete.tempi.vita_estraneo
        && !rete.fidato(&collegamento.pari)
    {
        collegamento.chiudi(Some(ErrorCodeKind::InsiemeEstraneoScaduto), Modo::Cortese);
    }
}

/// Trentadue byte dal generatore del sistema.
fn casuale() -> Option<[u8; 32]> {
    let mut byte = [0_u8; 32];
    getrandom::fill(&mut byte).ok()?;
    Some(byte)
}

/// L'associazione, dal lato di chi la chiede: l'impegno. Vedi
/// `Rete::associa`.
///
/// La chiede solo chi si è collegato a un host, su una connessione **in
/// uscita**; chi ospita risponde e basta. Con i ruoli fissi, chi sta nel mezzo
/// non può far fare a un host la parte del membro — quella che sceglie per
/// prima il suo nonce.
///
/// # Errori
///
/// `insieme.codiceRifiutato` con la propria chiave, `insieme.troppiTentativi`
/// oltre i tetti, `insieme.connessioneCaduta` se la connessione non c'è più,
/// `insieme.protocollo` su una connessione in entrata.
pub(crate) fn associa(rete: &Condiviso, collegamento: &Collegamento) -> Result<(), AppError> {
    if collegamento.uso != Uso::Controllo || collegamento.chiusa() {
        return Err(errore(ErrorCodeKind::InsiemeConnessioneCaduta));
    }
    if collegamento.in_entrata {
        return Err(errore(ErrorCodeKind::InsiemeProtocollo)
            .with_cause("l'associazione la chiede chi si collega, non chi ospita"));
    }
    if collegamento.pari == rete.identita.pubblica() {
        // Due istanze con la stessa identità: associarle non vuol dire
        // niente, e il codice uscirebbe uguale per costruzione.
        return Err(errore(ErrorCodeKind::InsiemeCodiceRifiutato)
            .with_cause("l'altro capo ha la stessa chiave di questo PC"));
    }
    let mut stato = chiave(&collegamento.associazione);
    if stato.dal().is_some() {
        // Già in corso: il codice arriverà da quella.
        return Ok(());
    }
    let prenotazione = chiave(&rete.tentativi)
        .prenota(&collegamento.pari, Instant::now(), false)
        .map_err(|rifiuto| {
            let riprova = match rifiuto {
                Rifiuto::Troppi(attesa) => {
                    Some(u64::try_from(attesa.as_millis()).unwrap_or(u64::MAX))
                }
                Rifiuto::Occupato => None,
            };
            AppError::new(ErrorCode::InsiemeTroppiTentativi {
                riprova_fra_ms: riprova,
            })
        })?;
    let Some(nonce) = casuale() else {
        drop(stato);
        chiave(&rete.tentativi).finito(prenotazione, true);
        return Err(crate::errori::inatteso(
            "il generatore del sistema non risponde",
        ));
    };
    let impegno = codice::impegno(&nonce, &rete.identita.pubblica(), &collegamento.pari);
    *stato = Associazione::Impegnata {
        nonce,
        dal: Instant::now(),
        prenotazione,
    };
    drop(stato);
    if !collegamento.metti(Uscita::Json(json(&Trasporto::Impegno {
        impegno: Byte32(impegno),
    }))) {
        return Err(errore(ErrorCodeKind::InsiemeConnessioneCaduta));
    }
    Ok(())
}

/// L'associazione, dal lato di chi risponde: arriva l'impegno, si manda il
/// nonce. Solo su una connessione in entrata (vedi [`associa`]), e una sola
/// alla volta su tutto l'host (vedi `codice::Tentativi`).
///
/// Con un'altra già in corso si risponde `Occupato` e la connessione resta:
/// vedi [`Trasporto::Occupato`]. Oltre i tetti dei tentativi invece si chiude
/// con `insieme.troppiTentativi`, che è un no vero.
fn impegno_ricevuto(
    rete: &Condiviso,
    collegamento: &Collegamento,
    impegno: [u8; 32],
) -> Result<(), Fine> {
    if !collegamento.in_entrata {
        return Err(Fine::protocollo());
    }
    if collegamento.pari == rete.identita.pubblica() {
        return Err(Fine::con(ErrorCodeKind::InsiemeCodiceRifiutato));
    }
    let mut stato = chiave(&collegamento.associazione);
    if stato.dal().is_some() {
        return Err(Fine::protocollo());
    }
    let prenotata = chiave(&rete.tentativi).prenota(&collegamento.pari, Instant::now(), true);
    let prenotazione = match prenotata {
        Ok(numero) => numero,
        Err(Rifiuto::Occupato) => {
            drop(stato);
            let _ = collegamento.metti(Uscita::Json(json(&Trasporto::Occupato)));
            return Ok(());
        }
        Err(Rifiuto::Troppi(_)) => return Err(Fine::con(ErrorCodeKind::InsiemeTroppiTentativi)),
    };
    let Some(nonce) = casuale() else {
        drop(stato);
        chiave(&rete.tentativi).finito(prenotazione, true);
        return Err(Fine::protocollo());
    };
    *stato = Associazione::AttesaRivela {
        impegno,
        nonce,
        dal: Instant::now(),
        prenotazione,
    };
    drop(stato);
    let _ = collegamento.metti(Uscita::Json(json(&Trasporto::Nonce {
        nonce: Byte32(nonce),
    })));
    Ok(())
}

/// Chi ha chiesto: arriva il nonce dell'host, si rivela il proprio e si
/// mostra il codice.
fn nonce_ricevuto(
    rete: &Condiviso,
    collegamento: &Collegamento,
    nonce_host: [u8; 32],
) -> Result<(), Fine> {
    let mut stato = chiave(&collegamento.associazione);
    let Associazione::Impegnata {
        nonce: nonce_membro,
        dal,
        prenotazione,
    } = *stato
    else {
        return Err(Fine::protocollo());
    };
    let codice = codice::codice(
        &collegamento.hash,
        &nonce_membro,
        &nonce_host,
        &rete.identita.pubblica(),
        &collegamento.pari,
    );
    *stato = Associazione::Codice {
        qui: false,
        la: false,
        dal,
        prenotazione,
    };
    drop(stato);
    let _ = collegamento.metti(Uscita::Json(json(&Trasporto::Rivela {
        nonce: Byte32(nonce_membro),
    })));
    collegamento.annuncia(
        rete,
        Evento::Codice {
            id: collegamento.id,
            codice,
        },
    );
    Ok(())
}

/// Chi ha chiesto: chi ospita è occupato con un'altra associazione. Vedi
/// [`Trasporto::Occupato`]: la prenotazione si toglie senza contarla, la
/// connessione resta, e il desktop lo sa da `Evento::AssociazioneRimandata`.
fn occupato_ricevuto(rete: &Condiviso, collegamento: &Collegamento) -> Result<(), Fine> {
    let mut stato = chiave(&collegamento.associazione);
    let Associazione::Impegnata { prenotazione, .. } = *stato else {
        // Solo in risposta a un impegno nostro, e prima del nonce.
        return Err(Fine::protocollo());
    };
    *stato = Associazione::Nessuna;
    chiave(&rete.tentativi).annulla(prenotazione);
    drop(stato);
    collegamento.annuncia(
        rete,
        Evento::AssociazioneRimandata {
            id: collegamento.id,
            codice: ErrorCodeKind::InsiemeAssociazioneOccupata,
        },
    );
    Ok(())
}

/// Chi risponde: arriva la rivelazione, si verifica l'impegno e si mostra il
/// codice. Una rivelazione che non mantiene l'impegno chiude.
fn rivelazione_ricevuta(
    rete: &Condiviso,
    collegamento: &Collegamento,
    nonce_membro: [u8; 32],
) -> Result<(), Fine> {
    let mut stato = chiave(&collegamento.associazione);
    let Associazione::AttesaRivela {
        impegno,
        nonce: nonce_host,
        dal,
        prenotazione,
    } = *stato
    else {
        return Err(Fine::protocollo());
    };
    let mia = rete.identita.pubblica();
    if !codice::impegno_mantenuto(&impegno, &nonce_membro, &mia, &collegamento.pari) {
        return Err(Fine::con(ErrorCodeKind::InsiemeCodiceRifiutato));
    }
    let codice = codice::codice(
        &collegamento.hash,
        &nonce_membro,
        &nonce_host,
        &mia,
        &collegamento.pari,
    );
    *stato = Associazione::Codice {
        qui: false,
        la: false,
        dal,
        prenotazione,
    };
    drop(stato);
    collegamento.annuncia(
        rete,
        Evento::Codice {
            id: collegamento.id,
            codice,
        },
    );
    Ok(())
}

/// L'altra persona ha risposto al codice.
fn conferma_ricevuta(
    rete: &Condiviso,
    collegamento: &Collegamento,
    uguale: bool,
) -> Result<(), Fine> {
    if !uguale {
        // L'altro capo chiude da sé col suo `Chiudo`; qui si chiude lo stesso,
        // per non dipendere da lui.
        return Err(Fine {
            codice: Some(ErrorCodeKind::InsiemeCodiceRifiutato),
            modo: Modo::Muto,
        });
    }
    let mut stato = chiave(&collegamento.associazione);
    let Associazione::Codice {
        qui,
        dal,
        prenotazione,
        ..
    } = *stato
    else {
        return Err(Fine::protocollo());
    };
    if qui {
        // Fidato prima di uscire dal codice: vedi `riuscita`.
        rete.fida(collegamento.pari);
        *stato = Associazione::Nessuna;
        drop(stato);
        riuscita(rete, collegamento, prenotazione);
    } else {
        *stato = Associazione::Codice {
            qui,
            la: true,
            dal,
            prenotazione,
        };
    }
    Ok(())
}

/// La persona davanti a questo schermo ha risposto al codice. Vedi
/// `Rete::conferma_codice`.
pub(crate) fn conferma(rete: &Condiviso, collegamento: &Collegamento, uguale: bool) {
    let mut stato = chiave(&collegamento.associazione);
    let Associazione::Codice {
        la,
        dal,
        prenotazione,
        ..
    } = *stato
    else {
        return;
    };
    if !uguale {
        drop(stato);
        collegamento.chiudi(Some(ErrorCodeKind::InsiemeCodiceRifiutato), Modo::Cortese);
        return;
    }
    let finita = la;
    if finita {
        // Fidato prima di uscire dal codice: vedi `riuscita`.
        rete.fida(collegamento.pari);
    }
    *stato = if finita {
        Associazione::Nessuna
    } else {
        Associazione::Codice {
            qui: true,
            la,
            dal,
            prenotazione,
        }
    };
    drop(stato);
    let _ = collegamento.metti(Uscita::Json(json(&Trasporto::Conferma { uguale: true })));
    if finita {
        riuscita(rete, collegamento, prenotazione);
    }
}

/// Tutte e due le persone hanno detto sì.
///
/// La chiave a quel punto è già fidata: la fanno fidata i due chiamanti,
/// **prima** di lasciare lo stato del codice e sotto il suo lucchetto. Fidata
/// da subito, prima che il desktop la salvi e chiami `pari`, perché il primo
/// messaggio dell'altro capo (un `Chiedi`, magari) può arrivare prima, e senza
/// chiuderebbe la connessione con `nonAssociato`. E prima di lasciare il
/// codice perché fino alla revisione del 26 ottobre c'era un istante — fra lo
/// stato tornato `Nessuna` e la fiducia — in cui un battito letto dall'altro
/// filo faceva guardare le scadenze a una connessione né in associazione né
/// fidata: un estraneo oltre il suo tempo, chiuso mentre le due persone
/// avevano appena detto sì.
fn riuscita(rete: &Condiviso, collegamento: &Collegamento, prenotazione: u64) {
    // La prenotazione esce dal conto: un'associazione riuscita non è un
    // tentativo da contare.
    chiave(&rete.tentativi).finito(prenotazione, true);
    collegamento.annuncia(
        rete,
        Evento::Associato {
            id: collegamento.id,
            pari: collegamento.pari,
        },
    );
}

/// Un valore in JSON. I tipi di questo crate si serializzano sempre — non
/// hanno mappe con chiavi non testuali né numeri non finiti — e il ripiego
/// vuoto, se mai servisse, l'altro capo lo rifiuterebbe come protocollo.
pub(crate) fn json<T: Serialize>(valore: &T) -> Vec<u8> {
    serde_json::to_vec(valore).unwrap_or_default()
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn un_evento_parte_col_cancello_chiuso_alle_spalle() {
        // Mentre l'evento parte, `chiudi` non deve poter prendere il cancello:
        // è quel che le impedisce di mandare la sua `Chiusa` in mezzo.
        let annunci = Mutex::new(Annunci::Aperta);
        let mut partito = false;
        se_aperta(&annunci, || {
            partito = true;
            assert!(
                annunci.try_lock().is_err(),
                "il cancello è libero mentre l'evento parte"
            );
        });
        assert!(partito);
        // Prima dell'`Aperta` e dopo la `Chiusa` non parte niente.
        for stato in [Annunci::Nessuno, Annunci::Chiusa] {
            let annunci = Mutex::new(stato);
            se_aperta(&annunci, || panic!("{stato:?}: non doveva partire"));
        }
    }
}
