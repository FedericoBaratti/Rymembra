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

pub mod autoplay;
pub mod backup;
pub mod covers;
pub mod db;
pub mod desiderati;
pub mod enrich;
pub mod files;
pub mod import_account;
pub mod import_esterno;
pub mod import_legacy;
pub mod import_playlist;
pub mod incerti;
pub mod library;
pub mod metadata;
pub mod organize;
pub mod playback;
pub mod playlists;
pub mod preferenze;
pub mod primo;
pub mod profilo;
pub mod provenienza;
pub mod scadenza;
pub mod scrobble;
pub mod settimana;
pub mod settings;
pub mod sincronia;
pub mod smart;
pub mod sonora;
pub mod tag_scrittura;
pub mod testi;
pub mod tinta;
pub mod vicinanza;
