//! Quel che le prove del riconciliatore si scambiano: una riga di catalogo, un
//! orologio e una libreria in memoria.

use std::collections::BTreeMap;

use rusqlite::Connection;

use crate::library::condivisa::{
    BranoCatalogo, ID_PER_SECCHIO, RiassuntoCatalogo, SecchioCatalogo,
};
use crate::specchio::archivio::{Chiusura, Conti, Giro, lavoro, stato};
use crate::specchio::convalida::convalida_secchio;

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

/// Un giro intero con `righe` come catalogo del dispositivo: i secchi
/// dell'id, ognuno scritto, poi la chiusura. Come lo farebbe chi ha la
/// rete, senza la rete.
pub(crate) fn giro(
    c: &Connection,
    dispositivo: &str,
    righe: &[BranoCatalogo],
    adesso: i64,
) -> Chiusura {
    giro_con(c, dispositivo, righe, adesso).0
}

pub(crate) fn giro_con(
    c: &Connection,
    dispositivo: &str,
    righe: &[BranoCatalogo],
    adesso: i64,
) -> (Chiusura, Conti) {
    let mut per_secchio: BTreeMap<i64, Vec<BranoCatalogo>> = BTreeMap::new();
    for r in righe {
        per_secchio
            .entry(r.id.div_euclid(ID_PER_SECCHIO))
            .or_default()
            .push(r.clone());
    }
    let secchi: Vec<SecchioCatalogo> = per_secchio
        .iter()
        .map(|(k, v)| SecchioCatalogo {
            da_id: k * ID_PER_SECCHIO,
            a_id: k * ID_PER_SECCHIO + ID_PER_SECCHIO - 1,
            impronta: {
                let mut h = blake3::Hasher::new();
                for r in v {
                    h.update(&serde_json::to_vec(r).unwrap());
                }
                h.finalize().to_hex().chars().take(32).collect()
            },
            righe: i64::try_from(v.len()).unwrap(),
        })
        .collect();
    let riassunto = RiassuntoCatalogo {
        generazione: 1,
        secchi,
    };
    let stato = stato(c, dispositivo).expect("stato");
    let lavoro = lavoro(&riassunto, &stato);
    let mut giro = Giro::nuovo(dispositivo, stato.righe);
    for secchio in &lavoro.da_leggere {
        let letto = per_secchio
            .get(&secchio.da_id.div_euclid(ID_PER_SECCHIO))
            .cloned()
            .unwrap_or_default();
        let buone = convalida_secchio(secchio.da_id, secchio.a_id, letto, adesso);
        giro.applica_secchio(c, secchio, buone.righe, adesso)
            .expect("secchio");
    }
    let conti = giro.conti;
    let chiusura = giro
        .chiudi(c, &lavoro.da_svuotare, true, 1, adesso)
        .expect("chiusura");
    (chiusura, conti)
}
