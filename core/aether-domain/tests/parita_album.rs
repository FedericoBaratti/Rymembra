//! Parità con l'originale sul raggruppamento degli album.
//!
//! Un errore qui non produce un guasto: produce una libreria che sembra a posto
//! e ha lo stesso disco in cinque schede, o due dischi diversi fusi in uno. Non
//! si nota provando, si nota dopo mesi guardando la griglia degli album.

use aether_domain::album::{
    AlbumMember, album_folder, album_group_key, build_album_groups, normalize_key_text,
    pick_album_cover, pick_canonical_artist, strip_edition_suffix,
};
use serde::Deserialize;

const GOLDEN: &str = include_str!("golden/album.json");

type Esito = Result<(), serde_json::Error>;

fn vettori() -> Result<Vettori, serde_json::Error> {
    serde_json::from_str(GOLDEN)
}

#[derive(Deserialize)]
struct Vettori {
    #[serde(rename = "stripEditionSuffix")]
    strip_edition_suffix: Vec<CasoTesto>,
    #[serde(rename = "normalizeKeyText")]
    normalize_key_text: Vec<CasoTesto>,
    #[serde(rename = "albumFolder")]
    album_folder: Vec<CasoTesto>,
    #[serde(rename = "albumGroupKey")]
    album_group_key: Vec<CasoChiave>,
    #[serde(rename = "pickCanonical")]
    pick_canonical: Vec<CasoArtisti>,
    #[serde(rename = "pickAlbumCover")]
    pick_album_cover: Vec<CasoCopertine>,
    #[serde(rename = "buildAlbumGroups")]
    build_album_groups: Vec<CasoGruppi>,
}

#[derive(Deserialize)]
struct CasoTesto {
    nota: String,
    input: String,
    atteso: String,
}

#[derive(Deserialize)]
struct CasoChiave {
    nota: String,
    album: String,
    path: String,
    atteso: String,
}

#[derive(Deserialize)]
struct CasoArtisti {
    nota: String,
    rows: Vec<RigaJson>,
    atteso: String,
}

#[derive(Deserialize)]
struct CasoCopertine {
    nota: String,
    members: Vec<RigaJson>,
    atteso: Option<String>,
}

#[derive(Deserialize)]
struct CasoGruppi {
    nota: String,
    rows: Vec<RigaJson>,
    atteso: GruppiAttesi,
}

#[derive(Deserialize, PartialEq, Eq, Debug)]
struct GruppiAttesi {
    albums: Vec<AlbumAtteso>,
    /// Coppie `[chiave di base, chiave canonica]`, già ordinate dal generatore.
    remap: Vec<(String, String)>,
}

#[derive(Deserialize, PartialEq, Eq, Debug)]
struct AlbumAtteso {
    album_key: String,
    title: String,
    artist: String,
    year: Option<i32>,
    total_tracks: usize,
    cover_art_hash: Option<String>,
    mb_album_id: Option<String>,
    spotify_id: Option<String>,
}

/// La forma delle righe nel JSON: nomi come li scriveva il TypeScript.
#[derive(Deserialize, Default)]
struct RigaJson {
    #[serde(default)]
    album_key: String,
    #[serde(default)]
    album: String,
    #[serde(default)]
    album_artist: Option<String>,
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    year: Option<i32>,
    #[serde(default)]
    cover_art_hash: Option<String>,
    #[serde(default)]
    cover_source: Option<String>,
    #[serde(default)]
    cover_w: Option<u32>,
    #[serde(default)]
    cover_h: Option<u32>,
    #[serde(default)]
    mb_release_group_id: Option<String>,
    #[serde(default)]
    mb_release_id: Option<String>,
    #[serde(default)]
    spotify_album_id: Option<String>,
}

impl RigaJson {
    fn to_member(&self) -> AlbumMember {
        AlbumMember {
            album_key: self.album_key.clone(),
            album: self.album.clone(),
            album_artist: self.album_artist.clone(),
            artist: self.artist.clone(),
            year: self.year,
            // I casi di parità vengono dal vecchio albero, che il genere
            // dell'album non lo aggregava: qui non c'è niente da confrontare.
            genre: None,
            cover_art_hash: self.cover_art_hash.clone(),
            cover_source: self.cover_source.clone(),
            cover_width: self.cover_w,
            cover_height: self.cover_h,
            mb_release_group_id: self.mb_release_group_id.clone(),
            mb_release_id: self.mb_release_id.clone(),
            spotify_album_id: self.spotify_album_id.clone(),
        }
    }
}

fn confronta<T: PartialEq + std::fmt::Debug>(nome: &str, esiti: Vec<(String, String, T, T)>) {
    let differenze: Vec<_> = esiti
        .into_iter()
        .filter(|(_, _, atteso, ottenuto)| atteso != ottenuto)
        .collect();
    assert!(
        differenze.is_empty(),
        "{} vettori di {nome} non combaciano con l'originale TypeScript:\n{}",
        differenze.len(),
        differenze
            .iter()
            .map(|(nota, input, atteso, ottenuto)| format!(
                "  {input}\n      atteso   {atteso:?}\n      ottenuto {ottenuto:?}\n      ({nota})"
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn strip_edition_suffix_combacia_con_l_originale() -> Esito {
    confronta(
        "strip_edition_suffix",
        vettori()?
            .strip_edition_suffix
            .into_iter()
            .map(|c| {
                let ottenuto = strip_edition_suffix(&c.input);
                (c.nota, format!("{:?}", c.input), c.atteso, ottenuto)
            })
            .collect(),
    );
    Ok(())
}

#[test]
fn normalize_key_text_combacia_con_l_originale() -> Esito {
    confronta(
        "normalize_key_text",
        vettori()?
            .normalize_key_text
            .into_iter()
            .map(|c| {
                let ottenuto = normalize_key_text(&c.input);
                (c.nota, format!("{:?}", c.input), c.atteso, ottenuto)
            })
            .collect(),
    );
    Ok(())
}

#[test]
fn album_folder_combacia_con_l_originale() -> Esito {
    confronta(
        "album_folder",
        vettori()?
            .album_folder
            .into_iter()
            .map(|c| {
                let ottenuto = album_folder(&c.input);
                (c.nota, format!("{:?}", c.input), c.atteso, ottenuto)
            })
            .collect(),
    );
    Ok(())
}

#[test]
fn album_group_key_combacia_con_l_originale() -> Esito {
    confronta(
        "album_group_key",
        vettori()?
            .album_group_key
            .into_iter()
            .map(|c| {
                let ottenuto = album_group_key(&c.album, &c.path);
                (c.nota.clone(), c.nota, c.atteso, ottenuto)
            })
            .collect(),
    );
    Ok(())
}

#[test]
fn pick_canonical_artist_combacia_con_l_originale() -> Esito {
    confronta(
        "pick_canonical_artist",
        vettori()?
            .pick_canonical
            .into_iter()
            .map(|c| {
                let members: Vec<AlbumMember> = c.rows.iter().map(RigaJson::to_member).collect();
                let ottenuto = pick_canonical_artist(&members);
                (c.nota.clone(), c.nota, c.atteso, ottenuto)
            })
            .collect(),
    );
    Ok(())
}

#[test]
fn pick_album_cover_combacia_con_l_originale() -> Esito {
    confronta(
        "pick_album_cover",
        vettori()?
            .pick_album_cover
            .into_iter()
            .map(|c| {
                let members: Vec<AlbumMember> = c.members.iter().map(RigaJson::to_member).collect();
                let ottenuto = pick_album_cover(&members);
                (c.nota.clone(), c.nota, c.atteso, ottenuto)
            })
            .collect(),
    );
    Ok(())
}

#[test]
fn build_album_groups_combacia_con_l_originale() -> Esito {
    let casi = vettori()?.build_album_groups;
    assert!(
        casi.len() >= 10,
        "i vettori sono stati svuotati per sbaglio?"
    );
    confronta(
        "build_album_groups",
        casi.into_iter()
            .map(|c| {
                let members: Vec<AlbumMember> = c.rows.iter().map(RigaJson::to_member).collect();
                let out = build_album_groups(&members);
                let ottenuto = GruppiAttesi {
                    albums: out
                        .albums
                        .into_iter()
                        .map(|a| AlbumAtteso {
                            album_key: a.album_key,
                            title: a.title,
                            artist: a.artist,
                            year: a.year,
                            total_tracks: a.total_tracks,
                            cover_art_hash: a.cover_art_hash,
                            mb_album_id: a.mb_album_id,
                            spotify_id: a.spotify_id,
                        })
                        .collect(),
                    remap: out.remap,
                };
                (c.nota.clone(), c.nota, c.atteso, ottenuto)
            })
            .collect(),
    );
    Ok(())
}
