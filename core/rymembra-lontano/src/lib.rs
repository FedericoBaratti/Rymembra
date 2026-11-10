//! Il collegamento fra i propri dispositivi via internet, senza un server di
//! Rymembra.
//!
//! # Cosa fa
//!
//! Il PC mostra un QR, il telefono lo legge, e da lì i due si ritrovano da
//! soli, in casa e fuori: il telefono sfoglia e ascolta la musica del PC
//! anche sui dati mobili, e il PC quella del telefono. Gli ascolti fatti
//! sull'uno si contano sull'altro, che ne è il proprietario.
//!
//! # Come, e chi vede cosa
//!
//! Il trasporto è [iroh](https://iroh.computer): QUIC fra due chiavi ed25519,
//! cifrato da un capo all'altro. Prima si prova la strada diretta (anche
//! bucando il NAT); se non c'è, si passa da un relay — quelli pubblici di n0,
//! o uno proprio — che vede soltanto byte cifrati e le due chiavi. Per
//! ritrovarsi senza un server nostro ognuno pubblica il proprio relay di casa
//! nel DNS di n0 (`dns.iroh.link`) e, se lo si vuole, nella DHT di
//! BitTorrent; in casa basta mDNS. Vedi `PRIVACY.md`.
//!
//! # I pezzi
//!
//! - [`collegamento`]: il link del QR e la sua matrice;
//! - [`protocollo`]: cosa si dicono due dispositivi, e come;
//! - [`nodo`]: questo dispositivo in rete, con una faccia bloccante;
//! - [`errori`]: i codici `lontano.*`, che soli attraversano la rete.
//!
//! Il crate non sa niente della libreria né del database: le risposte le dà
//! l'app, attraverso [`nodo::Ospite`].

#![forbid(unsafe_code)]

pub mod collegamento;
pub mod errori;
pub mod flusso;
pub mod nodo;
pub mod protocollo;

pub use collegamento::{Invito, Matrice, matrice};
pub use flusso::Flusso;
pub use nodo::{Evento, Identita, Impostazioni, Nodo, Ospite, PariNoto, Relay, Rete, Via};
pub use protocollo::{Domanda, Presentazione, Presenza, Risposta};
