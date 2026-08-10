//! Il backup di Aether su Google Drive.
//!
//! Una libreria di Aether si ricostruisce scansionando il disco. Quel che una
//! scansione **non** ricostruisce — conteggi d'ascolto, voti, preferiti,
//! playlist, cartelle sorvegliate, skin — esiste in un posto solo, e se il
//! computer si formatta è finita. Questo crate lo mette al sicuro nella cartella
//! privata dell'applicazione su Drive, e lo riporta indietro quando serve.
//!
//! # La regola di dipendenza
//!
//! ```text
//! aether-cloud  ──►  aether-app  ──►  aether-domain
//! ```
//!
//! Verso il basso, mai il contrario. Questo crate non compare fra le dipendenze
//! di nessun altro membro del workspace, e non deve cominciare adesso.
//!
//! # Perché non dentro `aether-app`, che pure «parla col mondo»
//!
//! Tre ragioni, tutte concrete:
//!
//! 1. `aether-app` è il crate che diventerà l'`.so` di Android, e il suo
//!    `Cargo.toml` argomenta a lungo su SQLite compilato dentro *proprio* per
//!    l'APK. Metterci `ureq` più `rustls` significherebbe portarsi uno stack TLS
//!    su ogni ABI per una funzione che su Android passerà dai Play Services.
//! 2. `cargo test` su `aether-app` oggi non compila crittografia. Deve restare
//!    vero: è la differenza fra provare la libreria in cinque secondi e in due
//!    minuti.
//! 3. **Nessuna funzione di questo crate riceve una `rusqlite::Connection`.**
//!    Riceve un `Contenuto` già costruito e ne restituisce uno. Così «nessun
//!    lucchetto della libreria resta preso durante una richiesta di rete» non è
//!    un commento che qualcuno prima o poi violerà per comodità: è una proprietà
//!    che il compilatore verifica, perché il tipo che servirebbe a violarla non
//!    attraversa il confine.
//!
//! # I segreti
//!
//! Il token di aggiornamento sta nel portachiavi del sistema operativo, mai
//! nella tabella `settings` — vedi [`portachiavi`], dove sta anche la ragione
//! per cui non si ripiega mai in silenzio.

pub mod drive;
pub mod oauth;
pub mod pacchetti;
pub mod portachiavi;
pub mod servizio;

/// Il client HTTP, che ora vive in `aether-net`.
///
/// Re-esportato con lo stesso nome che aveva quando era un modulo di questo
/// crate: `crate::http::Rete` continua a risolvere in `oauth` e in `drive`, e lo
/// spostamento non si vede da qui. Chi scrive codice nuovo può importarlo dalla
/// sua casa vera.
pub use aether_net::http;

/// Il servitore di loopback, che ora vive in `aether-oauth`.
///
/// Stessa storia dell'HTTP qui sopra, e per la stessa ragione: raccogliere la
/// risposta di un browser su `127.0.0.1` non è una faccenda di Drive, ed è
/// diventato evidente quando anche Spotify ha avuto bisogno di farlo.
pub use aether_oauth::loopback;
pub use drive::{Drive, FileRemoto};
pub use oauth::{Credenziali, Token};
pub use portachiavi::Portachiavi;
pub use servizio::{Avanzamento, Cosa, DaRipristinare, Passata, Scritti};
