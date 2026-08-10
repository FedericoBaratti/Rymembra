//! Il ponte verso yt-dlp: dal nome di un brano ai byte del brano.
//!
//! Spotify dice *cosa* si vuole e non dà nessun modo di sentirlo. Questo crate è
//! l'altra metà: cerca su YouTube, sceglie, scarica. Fratello di
//! `aether-spotify`, con la sua stessa disciplina.
//!
//! # Cosa fa, e cosa deliberatamente non fa
//!
//! **Fa**: risolve il binario, costruisce l'argv, esegue yt-dlp, legge il suo
//! avanzamento, traduce i suoi guasti in codici del catalogo.
//!
//! **Non fa**: non decide *quale* video — quello è
//! [`aether_domain::yt_match`], perché è una decisione e va provata senza rete
//! né binari esterni. Non tocca il database: qui dentro non passa mai una
//! `rusqlite::Connection`, come in `aether-spotify` e `aether-cloud`, così
//! «nessun lucchetto della libreria resta preso durante uno scaricamento»
//! diventa una proprietà che il compilatore verifica invece di un commento. E la
//! differenza qui pesa di più che altrove: una richiesta di rete dura un
//! secondo, uno scaricamento dura un minuto.
//!
//! # La regola di dipendenza
//!
//! ```text
//! aether-yt  ──►  aether-domain
//! ```
//!
//! Nemmeno `aether-net`: yt-dlp fa da sé le sue richieste HTTP, e noi gli
//! parliamo attraverso un processo figlio.
//!
//! # Tutto bloccante, come il resto
//!
//! `std::process::Command` e fili, nessun runtime asincrono — la ragione sta in
//! `Cargo.toml:38-44` della radice. L'annullamento non è un `AbortSignal` ma una
//! chiusura `Fn() -> bool` che chi chiama collega al proprio `AtomicBool`:
//! [`processo`] la interroga fra una riga e l'altra e uccide il figlio quando
//! dice di sì.
//!
//! # Perché tre tentativi e non uno
//!
//! YouTube risponde `403 Forbidden` ai client predefiniti di yt-dlp a
//! intermittenza. [`argomenti::TENTATIVI`] cammina tre profili di client, e la
//! differenza fra «ritentare col profilo dopo» e «arrendersi» la decide
//! [`argomenti::ritentabile`] leggendo lo stderr. Senza quel giro, un 403
//! passeggero verrebbe riportato all'utente come «brano non trovato su
//! YouTube», che è falso e lo manda a cercare una copia che c'è già.

pub mod argomenti;
pub mod binario;
pub mod errori;
pub mod processo;
pub mod progresso;
pub mod ricerca;
pub mod scarica;

pub use binario::Binari;
pub use progresso::{Evento, analizza};
pub use ricerca::cerca;
pub use scarica::{Richiesta, scarica};
