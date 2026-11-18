//! Quel che le prove del riconciliatore si scambiano: una riga di catalogo, un
//! orologio e una libreria in memoria.

use rusqlite::Connection;

use crate::library::condivisa::BranoCatalogo;

/// Martedì 10 novembre 2026, più o meno.
pub(crate) const ADESSO: i64 = 1_794_312_000_000;

/// La chiave di un dispositivo di prova.
pub(crate) fn chiave(c: char) -> String {
    c.to_string().repeat(64)
}

/// Una libreria vuota, con lo schema aggiornato.
pub(crate) fn libreria() -> Connection {
    crate::db::open_in_memory()
        .expect("database in memoria")
        .connection
}

pub(crate) fn riga(id: i64) -> BranoCatalogo {
    BranoCatalogo {
        id,
        title: format!("Titolo {id}"),
        artist: "Artista".to_owned(),
        album: "Album".to_owned(),
        album_artist: None,
        genre: Some("Rock".to_owned()),
        album_key: Some("a".repeat(32)),
        track_number: Some(1),
        disc_number: None,
        year: Some(1977),
        duration_ms: 200_000,
        cover_art_hash: Some("ab".repeat(32)),
        play_count: 3,
        liked: false,
        rating: 0,
        liked_at: None,
        rating_at: None,
        date_added: 1_700_000_000_000,
        last_played_at: None,
        stats_updated_at: 0,
        bitrate: Some(320),
        sample_rate: Some(44_100),
        channels: Some(2),
        codec: Some("mp3".to_owned()),
        track_key: format!("artista|titolo {id}|album"),
        content_key: Some(format!("artista|titolo {id}|album")),
        mb_recording_id: None,
        mb_release_group_id: None,
        mb_release_id: None,
        estensione: "mp3".to_owned(),
        dimensione: 5_000_000,
        versione: "5000000-1700000000000".to_owned(),
        segmento: None,
    }
}
