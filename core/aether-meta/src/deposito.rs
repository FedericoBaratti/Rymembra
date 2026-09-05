//! Dove si ricorda quel che un servizio ha già risposto.
//!
//! # Perché è un tratto e non una tabella
//!
//! Il deposito vive in SQLite, e questo crate **non vede `rusqlite`** — è la
//! regola di dipendenza che [`crate`] dichiara, e che rende impossibile invece
//! che sconsigliato tenere preso il lucchetto della libreria mentre si aspetta
//! una risposta dalla rete. Il tratto è la stessa indirezione che
//! `aether_cloud::portachiavi::Portachiavi` usa per il portachiavi di sistema e
//! che `aether_app::files::MusicFiles` usa per il filesystem, per la stessa
//! ragione: chi implementa sta dall'altra parte del confine.
//!
//! # La risposta negativa è quella che conta di più
//!
//! Ricordare che MusicBrainz *ha* «Abbey Road» risparmia una richiesta. Ricordare
//! che **non ha** i trecento file che nessuno riconoscerà mai risparmia trecento
//! richieste a ogni passata, per sempre — e su un servizio che ne concede una al
//! secondo è la differenza fra una passata di sottofondo e un filo che non fa
//! altro.
//!
//! Per questo [`Voce`] ha tre stati e non due: «non l'ho mai chiesto», «l'ho
//! chiesto e c'è», «l'ho chiesto e non c'è». Un deposito che collassasse gli
//! ultimi due in `None` richiederebbe per sempre le stesse cose.

/// Quel che il deposito sa di una chiave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Voce {
    /// Il servizio ha risposto, e questo è quel che ha detto.
    Corpo(Vec<u8>),
    /// Il servizio ha risposto che non ha niente. Non lo si richiede.
    Niente,
}

/// Un posto dove ricordare le risposte dei servizi.
///
/// Tutte le operazioni sono senza guasti: un deposito che non funziona fa
/// perdere una cache, non una passata. Chi implementa inghiotte i propri errori
/// e li registra, invece di propagarli fin qui — perché non c'è niente che chi
/// chiama possa fare, se non richiedere alla rete, che è già quel che succede.
pub trait Deposito: Send + Sync {
    /// Quel che si sa di questa chiave, se se ne sa qualcosa e non è scaduto.
    fn leggi(&self, servizio: &str, chiave: &str) -> Option<Voce>;

    /// Ricorda una risposta per `vive_ms` millisecondi.
    fn scrivi(&self, servizio: &str, chiave: &str, voce: &Voce, vive_ms: i64);
}

/// Per quanto si ricorda una ricerca.
///
/// Una settimana. I cataloghi cambiano — una pubblicazione viene corretta, una
/// copertina caricata — ma non abbastanza in fretta da giustificare di
/// richiedere ogni giorno. E chi vuole rifare una ricerca subito ha comunque la
/// via del ritentativo esplicito, che invalida.
pub const VIVE_RICERCA_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// Per quanto si ricorda una pubblicazione letta per intero.
///
/// Un mese: una pubblicazione già in catalogo, con le sue tracce e le sue
/// durate, è la cosa più stabile che MusicBrainz abbia.
pub const VIVE_PUBBLICAZIONE_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// Per quanto si ricorda un «non ce l'ho».
///
/// Tre giorni, cioè molto meno di una risposta positiva. Asimmetria voluta: una
/// registrazione che oggi manca può essere aggiunta domani da un collaboratore,
/// e ricordare per una settimana che non c'è vorrebbe dire non accorgersene.
pub const VIVE_NIENTE_MS: i64 = 3 * 24 * 60 * 60 * 1000;

/// Per quanto si ricordano i vicini di un brano o di un artista.
///
/// Tre mesi, che è molto più di una pubblicazione letta per intero, e non è una
/// distrazione. Un'affinità non è un fatto sul brano: è il residuo di **anni**
/// di ascolti aggregati di tutta ListenBrainz, e non cambia perché qualcuno
/// oggi ha ascoltato qualcosa. Cambia quando la fondazione rigenera l'intera
/// base dati, cioè su una scala di mesi.
///
/// Richiederla ogni settimana vorrebbe dire pagare cinquantasei richieste per
/// riavere lo stesso numero identico. La cosa che la fa scadere davvero non è
/// il tempo, è il cambio dell'algoritmo: e quello si vede nel codice, non
/// nell'orologio (vedi la nota sul nome del servizio in
/// [`crate::listenbrainz`]).
pub const VIVE_AFFINITA_MS: i64 = 90 * 24 * 60 * 60 * 1000;

/// Per quanto si ricorda che di un brano non si sa niente.
///
/// Un mese, cioè dieci volte [`VIVE_NIENTE_MS`], e l'asimmetria è rovesciata di
/// proposito rispetto a quella di MusicBrainz.
///
/// Là un «non ce l'ho» dura poco perché **un collaboratore può riempirlo
/// stanotte**: una registrazione che oggi manca domani c'è, e ricordarla assente
/// per una settimana vuol dire non accorgersene. Qui quel meccanismo non
/// esiste. Un brano assente dalla base dati delle somiglianze non ci entra
/// perché qualcuno lo aggiunge: ci entra quando la base dati viene ricalcolata
/// da capo, insieme a tutto il resto.
///
/// Tenere tre giorni sarebbe quindi il difetto peggiore possibile — la maggior
/// parte di una libreria vera **non** sta in quella base dati, e la metà
/// negativa è quella che risparmia di più. Vorrebbe dire rifare ogni tre giorni
/// tutte le richieste che questa cache esiste per non fare.
pub const VIVE_AFFINITA_NIENTE_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// Un deposito che non ricorda niente.
///
/// Serve alle prove e a chi vuole una passata che parli davvero con la rete —
/// per esempio l'esempio a riga di comando con cui si tara la decisione, dove
/// una cache renderebbe la seconda esecuzione una prova di sé stessa.
#[derive(Debug, Clone, Copy, Default)]
pub struct Senza;

impl Deposito for Senza {
    fn leggi(&self, _servizio: &str, _chiave: &str) -> Option<Voce> {
        None
    }

    fn scrivi(&self, _servizio: &str, _chiave: &str, _voce: &Voce, _vive_ms: i64) {}
}
