//! «Ascolta insieme»: il trasporto fra due o più Rymembra della stessa rete.
//!
//! # Cosa sta qui e cosa no
//!
//! Qui c'è tutto quel che riguarda la **rete**: trovare chi ospita, aprire un
//! canale cifrato, associare due PC con un codice di quattro cifre, mandare e
//! ricevere i [`messaggi`], leggere i byte di un brano sul disco di un altro
//! PC, e rispondere all'orologio. Non c'è niente che sappia cos'è un lettore,
//! una coda o un database: quelle cose le sa il desktop, che usa questo crate
//! attraverso [`Rete`] e i suoi [`Evento`].
//!
//! Per questo le sole dipendenze del progetto sono `rymembra-domain`, per gli
//! errori, e nient'altro: niente `rymembra-play`, niente `rusqlite`, niente
//! Internet. Il tempo arriva da fuori, come `fn() -> u64` in nanosecondi su
//! una base unica di processo (vedi [`Impostazioni::adesso_ns`]): è la stessa
//! su cui il motore audio scrive le sue ancore, e con due basi diverse la stima
//! dello scarto fra due PC non varrebbe niente.
//!
//! # Chi ascolta
//!
//! Solo chi **ospita** apre porte: TCP e UDP sulla [`PORTA`], o sulle nove
//! dopo, o su una qualunque. Gli altri si collegano in uscita e basta. Così la
//! richiesta del firewall di Windows compare su un PC solo, e non c'è mai un
//! «membro irraggiungibile».
//!
//! # Le connessioni
//!
//! Tutte cifrate con Noise `XX_25519_ChaChaPoly_SHA256`, e dopo la stretta
//! ciascun capo conosce la chiave pubblica dell'altro. Tre usi:
//!
//! - **controllo**: una per PC collegato. Saluto, battito ogni 5 s (15 s di
//!   silenzio la chiudono), orologio, associazione, e i [`Messaggio`];
//! - **lettura**: fino a due per membro verso l'host, riusabili, solo per i
//!   byte dei brani e delle copertine. Separate perché due megabyte in coda
//!   davanti a un'ancora la farebbero arrivare tardi;
//! - **servizio** (fase 2): un membro che ha brani suoi in coda apre una
//!   connessione verso l'host da cui l'host gli chiede i byte, e li rilancia
//!   agli altri. Il membro resta senza porte aperte.
//!
//! # Prove
//!
//! Non legano mai `0.0.0.0`: su Windows farebbe comparire la richiesta del
//! firewall a chi lancia `cargo test`. L'indirizzo da legare è un argomento
//! ([`Impostazioni::lega`]), e le prove usano `127.0.0.1`.

mod ascolto;
mod canale;
mod cliente;
mod codice;
mod collegamento;
mod condiviso;
mod cornice;
mod errori;
mod esadecimale;
pub mod identita;
pub mod messaggi;
#[cfg(test)]
mod prove_grezze;
#[cfg(test)]
mod prove_revisione;
mod rete;
mod scoperta;
mod servente;
mod tempi;
mod trasporto;

pub use identita::{ChiavePubblica, Identita, Impronta};
pub use messaggi::{Gettone, Messaggio};
pub use rete::{
    CampioneOrologio, Evento, FileServito, IdConnessione, Impostazioni, LettoreRemoto, Rete,
};
pub use scoperta::{IndirizziDiQui, indirizzi_di_qui};
pub use tempi::Tempi;

/// La porta di chi ospita, TCP e UDP. Se è occupata si provano le nove dopo,
/// poi una qualunque: la scoperta dice a chi cerca quale.
pub const PORTA: u16 = 47811;
