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
