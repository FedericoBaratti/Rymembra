//! Orchestrazione: dove il dominio incontra il mondo.
//!
//! `aether-domain` decide; questo crate esegue. Qui stanno il database, la rete,
//! il filesystem — tutto ciò che ha un orologio, uno stato o una latenza.
//!
//! La separazione non è ordine per l'ordine: le funzioni che possono perdere
//! dati vivono di là, dove si provano come chiamate di funzione; qui restano
//! l'apertura del file, la transazione, la coda. Quando qualcosa va storto in
//! questo crate si perde un'operazione. Quando andava storto nel modulo
//! monolitico del vecchio albero si perdevano righe di libreria.

pub mod db;
pub mod files;
pub mod metadata;
pub mod organize;
