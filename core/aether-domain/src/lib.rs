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
pub mod enrich;
pub mod errors;
pub mod keys;
pub mod listen;
pub mod merge;
pub mod organize;
pub mod paths;
pub mod queue;
pub mod restore;
pub mod scan_plan;
pub mod spotify;
pub mod spotify_account;
pub mod spotify_plan;
pub mod text;
pub mod yt_match;

pub use enrich::{
    AlbumMatch, Candidate, Fields, Fonte, LocalAlbum, LocalTrack, RemoteRelease, RemoteTrack,
    TrackMatch, Verdetto, album_distance, decide_album, plan_write, resolve_track,
};
pub use errors::{AppError, Domain, ErrorCode, ErrorCodeKind, Severity};
pub use keys::{PlaylistKey, TrackKey, TrackKeyInput};
pub use listen::{Listen, ListenTracker, counts_as_play};
pub use merge::{TrackStats, merge_stats};
pub use paths::PathRules;
pub use queue::{Queue, QueueSnapshot, RepeatMode, Step};
pub use restore::{
    PlaylistChange, PlaylistState, RestoreInput, RestorePlan, RootToAdd, TrackChange, plan_restore,
};
pub use scan_plan::{
    DiscoveredFile, KnownTrack, Rematch, RemovedIdentity, ScanInput, ScanPlan, match_moved_tracks,
    plan_scan,
};
pub use spotify::{SpotifyContent, SpotifyKind, SpotifySource, SpotifyTrack};
pub use spotify_account::{
    AccountPlan, AccountSnapshot, AlbumSpotify, ArtistaSpotify, AscoltoAbbinato, AscoltoSpotify,
    PianoPlaylist, PlaylistSpotify, Provenienza, ScartiCronologia, Scelte, plan_account_import,
};
pub use spotify_plan::{
    Abbinato, Gradino, Indice, LibraryTrack, SpotifyPlan, plan_spotify_import,
};
pub use text::fold_text;
