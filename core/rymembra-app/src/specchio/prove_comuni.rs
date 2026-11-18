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

// ── le playlist e la sincronia, per le prove dei gesti ──────────────────────

use rusqlite::params;
use rymembra_sync::{Contenuto, Documento, fondi};

use crate::sincronia;

/// Un brano di questa macchina con le chiavi di [`riga`]`(numero)`: lo stesso
/// brano che quel dispositivo manda con quell'id.
pub(crate) fn locale(c: &Connection, id: i64, numero: i64) {
    let k = riga(numero).track_key;
    c.execute(
        "INSERT INTO tracks (id, path, track_key, content_key, title, artist, album, file_size,
                             date_added, date_modified)
         VALUES (?1, ?2, ?3, ?3, ?4, 'Artista', 'Album', 1000, 1, 1)",
        params![id, format!("/musica/{numero}.mp3"), k, format!("Titolo {numero}")],
    )
    .expect("brano di qui");
}

/// L'id della riga specchio del brano `numero` del dispositivo `chi`.
pub(crate) fn id_specchio(c: &Connection, chi: char, numero: i64) -> i64 {
    c.query_row(
        "SELECT track_id FROM lontano_brani WHERE dispositivo = ?1 AND id_remoto = ?2",
        params![chiave(chi), numero],
        |r| r.get(0),
    )
    .expect("riga specchio")
}

/// Crea la playlist `nome` con questi brani, nell'ordine. Restituisce il suo id.
pub(crate) fn playlist_con(c: &Connection, nome: &str, brani: &[i64]) -> i64 {
    c.execute(
        "INSERT INTO playlists (playlist_key, name, created_at, updated_at)
         VALUES (?1, ?1, 100, 100)",
        [nome],
    )
    .expect("playlist");
    let id: i64 = c
        .query_row(
            "SELECT id FROM playlists WHERE playlist_key = ?1",
            [nome],
            |r| r.get(0),
        )
        .expect("id");
    for (posizione, brano) in brani.iter().enumerate() {
        c.execute(
            "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)",
            params![id, brano, i64::try_from(posizione).expect("posizione")],
        )
        .expect("membro");
    }
    id
}

/// Le chiavi dei brani di una playlist, nell'ordine, come stanno in `playlist_tracks`.
pub(crate) fn in_playlist(c: &Connection, nome: &str) -> Vec<String> {
    let mut st = c
        .prepare(
            "SELECT t.track_key FROM playlist_tracks pt
               JOIN playlists p ON p.id = pt.playlist_id
               JOIN tracks t ON t.id = pt.track_id
              WHERE p.playlist_key = ?1 ORDER BY pt.position",
        )
        .expect("ordine");
    st.query_map([nome], |r| r.get::<_, String>(0))
        .expect("ordine")
        .map(|r| r.expect("riga"))
        .collect()
}

/// Il documento che `chi` pubblicherebbe adesso.
pub(crate) fn documento(c: &Connection, chi: &str) -> Contenuto {
    sincronia::contenuto(c, chi, BTreeMap::new(), BTreeMap::new()).expect("contenuto")
}

/// La playlist come la pubblica la sincronia: le chiavi vive nell'ordine, e
/// quante voci tolte (lapidi) porta.
pub(crate) fn pubblicata(c: &Connection, nome: &str) -> (Vec<String>, usize) {
    let contenuto = documento(c, "qui");
    let sequenza = &contenuto
        .playlist
        .get(nome)
        .expect("la playlist viaggia")
        .sequenza;
    let vive = sequenza.ordine().into_iter().map(str::to_owned).collect();
    let tolte = sequenza
        .elementi()
        .filter(|(_, e)| e.tolto.is_some())
        .count();
    (vive, tolte)
}

/// Una passata vera di sincronia di `qui` con il documento di un altro: allinea,
/// legge, fonde, applica.
pub(crate) fn passata(qui: &mut Connection, io: &str, altro: (&str, Contenuto), adesso_ms: i64) {
    sincronia::allinea(qui, io, adesso_ms).expect("allineamento");
    let mio = documento(qui, io);
    let fuso = fondi(&[
        Documento::nuovo(io, mio.clone(), adesso_ms),
        Documento::nuovo(altro.0, altro.1, adesso_ms),
    ]);
    sincronia::applica_se_ferma(qui, &mio, &fuso, adesso_ms)
        .expect("applicazione")
        .expect("la libreria era ferma");
}

/// Il PC e il telefono si scambiano i documenti, nei due versi.
pub(crate) fn scambio(pc: &mut Connection, telefono: &mut Connection, adesso_ms: i64) {
    passata(
        telefono,
        "telefono",
        ("pc", documento(pc, "pc")),
        adesso_ms,
    );
    passata(
        pc,
        "pc",
        ("telefono", documento(telefono, "telefono")),
        adesso_ms + 1,
    );
}
