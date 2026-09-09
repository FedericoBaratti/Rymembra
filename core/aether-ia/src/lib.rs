//! I modelli di linguaggio: un client OpenAI-compatibile, e niente altro.
//!
//! # Cosa questo crate non sa
//!
//! Cos'è una skin. È la regola che decide se questo codice va qui o dentro
//! `src-tauri`, ed è scritta in cima apposta: il giorno in cui qui dentro
//! comparisse la parola «token» con il significato che ha nello Skin Studio,
//! questa crate avrebbe smesso di essere un client di modelli e sarebbe
//! diventata metà dello Studio in un posto sbagliato.
//!
//! Quel che sa fare è mandare una conversazione a un punto
//! OpenAI-compatibile e restituire quel che risponde, mentre risponde; e
//! riconoscere, dentro un testo qualunque, un blocco di operazioni su un
//! documento JSON qualunque ([`operazioni`]). Le istruzioni che descrivono il
//! vocabolario di una skin gliele passa chi lo usa, come un messaggio come gli
//! altri.
//!
//! # Perché non conosce il database, né la finestra
//!
//! Stessa regola di `aether-net`, `aether-catalogo` e `aether-scrobble`: chi
//! parla col mondo non vede `rusqlite`. I profili vivono in `settings` e le
//! chiavi nel portachiavi, entrambe cose di `src-tauri`; qui arriva un
//! [`Profilo`] già letto e una chiave già presa, per il tempo di una richiesta.
//!
//! # Le due cose che si provano senza rete, e sono quelle che sbagliano
//!
//! Il decodificatore del flusso ([`sse`]) e l'estrattore delle operazioni
//! ([`operazioni`]). Il primo perché i confini dei blocchi non coincidono con
//! quelli degli eventi, e il difetto che ne nasce compare solo con le risposte
//! lunghe; il secondo perché riceve testo scritto da un modello, cioè
//! l'ingresso meno prevedibile dell'applicazione. Sono due tipi puri con il
//! loro `mod prove`, e non hanno bisogno che nessun servizio sia acceso.
//!
//! # Cosa non finisce mai in un errore
//!
//! La chiave. [`Cliente`] ha un `Debug` scritto a mano perché non esca
//! nemmeno da lì, e [`Profilo`] non la porta affatto: porta l'informazione che
//! **esiste**, e il nome della voce di portachiavi dove trovarla.

pub mod cliente;
pub mod fornitore;
pub mod messaggi;
pub mod operazioni;
pub mod profilo;
pub mod sse;

pub use cliente::{Cliente, Modello, Prezzo};
pub use fornitore::Fornitore;
pub use messaggi::{Fine, Messaggio, Motivo, Ruolo, Voce};
pub use operazioni::{Op, Operazione};
pub use profilo::{Profilo, controlla_indirizzo, id_da_nome, voce_portachiavi};
pub use sse::Sse;
