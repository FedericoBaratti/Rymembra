//! Il lettore keyless di Spotify: da un link pubblico a un elenco di brani,
//! senza chiavi, senza account, senza che l'utente registri un'applicazione.
//!
//! # Cosa fa, e cosa deliberatamente non fa
//!
//! **Fa**: risolve un link a un brano, un album, una playlist o un artista in
//! metadati normalizzati ([`aether_domain::spotify::SpotifyContent`]).
//!
//! **Non fa**: non scarica audio, non tocca il database, non decide niente.
//! L'abbinamento con la libreria sta in
//! [`aether_domain::spotify_plan`] perché è una decisione e va provata senza
//! rete; la scrittura sta in `aether-app` perché tocca SQLite. Qui dentro non
//! passa mai una `rusqlite::Connection`, ed è la stessa regola che `aether-cloud`
//! si è data: così «nessun lucchetto della libreria resta preso durante una
//! richiesta di rete» è una proprietà che il compilatore verifica, non un
//! commento che qualcuno prima o poi violerà per comodità.
//!
//! # La regola di dipendenza
//!
//! ```text
//! aether-spotify  ──►  aether-net  ──►  aether-domain
//! ```
//!
//! # Come è fatto
//!
//! [`risolvi::Lettore`] è la porta d'ingresso e prova tre livelli in ordine
//! ([`pathfinder`], [`embed`], [`oembed`]), tenendosi il primo che risponde. Le
//! costanti che Spotify ruota — cifrari del TOTP, impronte delle interrogazioni
//! persistite — stanno in [`config`] e si correggono con un file, senza
//! ricompilare.
//!
//! Prima ancora c'è il riconoscimento di quel che l'utente ha incollato: [`url`]
//! per tutte le forme che si leggono da sole, [`scorciatoia`] per i link corti
//! del telefono, che vanno chiesti alla rete perché non dicono niente.
//!
//! # Una nota di onestà
//!
//! Questo crate parla con punti interni non documentati. Funzionerà finché
//! Spotify non li cambia, e prima o poi li cambierà: a febbraio 2026 ne ha
//! cambiati parecchi in una volta, ed è il motivo per cui il livello che il
//! vecchio albero usava per primo — la Web API pubblica con un gettone anonimo —
//! oggi non legge più nemmeno una playlist pubblica. Tutta l'architettura di qui
//! dentro è costruita attorno a quel fatto: livelli, configurazione esterna,
//! diagnostica.

pub mod account;
pub mod config;
pub mod copertina;
pub mod embed;
pub mod oembed;
pub mod pathfinder;
pub mod risolvi;
pub mod scorciatoia;
pub mod sessione;
pub mod totp;
pub mod url;

pub use account::{Avanzamento, Lettura};
pub use config::Configurazione;
pub use risolvi::{Diagnostica, Fallito, Lettore, Risultato};
pub use url::{Riferimento, riconosci};
