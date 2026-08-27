//! Il client HTTP di Aether.
//!
//! Stava in `aether-cloud`, quando l'unica cosa che usciva in rete era il backup
//! su Google Drive. Con l'arrivo dell'importazione da Spotify i clienti sono
//! due, e un importatore che dipende dal crate «la nuvola» per prendere in
//! prestito un `Agent` sarebbe una bugia strutturale: direbbe che l'importazione
//! ha qualcosa a che vedere con Drive, e chi legge il grafo delle dipendenze fra
//! sei mesi ci crederebbe.
//!
//! Non è una previsione: `Cargo.toml` alla radice del workspace dice già, a
//! proposito di `ureq` e compagni, che stanno lì «perché prima o poi le vorrà
//! anche qualcun altro (il recupero delle copertine, l'arricchimento da
//! MusicBrainz)». Questo è il qualcun altro.
//!
//! # La regola di dipendenza
//!
//! ```text
//! aether-cloud    ──►  aether-net  ──►  aether-domain
//! aether-catalogo ──►  aether-net  ──►  aether-domain
//! ```
//!
//! Questo crate vede **solo** il catalogo degli errori. Non conosce la libreria,
//! non conosce il database, e soprattutto non conosce `rusqlite`: è quel che
//! rende impossibile, e non solo sconsigliato, tenere preso un lucchetto della
//! libreria mentre si aspetta una risposta dalla rete.

pub mod flusso;
pub mod http;
pub mod immagine;

pub use flusso::{FlussoHttp, Sorgente};
pub use http::{Corpo, Metodo, Pezzo, Rete, Richiesta, Risposta, percento};
