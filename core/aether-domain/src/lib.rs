//! Il dominio di Aether: le decisioni, separate da chi le esegue.
//!
//! Qui non si legge un file, non si apre una connessione, non si guarda
//! l'orologio. Ogni funzione di questo crate è una funzione: stessi argomenti,
//! stesso risultato, sempre. Chi legge il disco e chi parla con la rete sta in
//! `aether-app`; chi disegna sta nelle due applicazioni.
//!
//! # Perché questa separazione, in questo progetto
//!
//! Aether è esistito in due alberi paralleli — desktop ed Android — con la
//! stessa logica scritta due volte. Non è divergita per sciatteria: è divergita
//! perché *poteva*. `classifyDownloadFailure` decideva `permanent` da una parte
//! e `transient` dall'altra per lo stesso identico guasto, e nessuno se n'è
//! accorto finché un download non si è smesso di ritentare solo sul telefono.
//!
//! Un solo crate, usato da entrambe le piattaforme, rende quella classe di
//! difetti impossibile invece che improbabile.
//!
//! # E perché "puro" non è un vezzo
//!
//! Le funzioni che stanno qui sono quelle che possono **perdere dati**: decidere
//! che una riga di libreria va cancellata, che due brani sono lo stesso brano,
//! che una skin è sicura da scompattare. Nel vecchio albero la decisione della
//! scansione era intrecciata alla camminata sul disco, e per provarla serviva
//! costruire un albero di file veri: il risultato è che il test era 56 righe
//! contro le 368 del modulo, e i casi che costavano dati non erano fra quelle 56.
//!
//! Separata, la stessa decisione si prova come una chiamata di funzione.

pub mod album;
pub mod errors;
pub mod keys;
pub mod paths;
pub mod scan_plan;
pub mod text;

pub use errors::{AppError, Domain, ErrorCode, ErrorCodeKind, Severity};
pub use keys::{PlaylistKey, TrackKey, TrackKeyInput};
pub use paths::PathRules;
pub use scan_plan::{DiscoveredFile, KnownTrack, ScanInput, ScanPlan, plan_scan};
pub use text::fold_text;
