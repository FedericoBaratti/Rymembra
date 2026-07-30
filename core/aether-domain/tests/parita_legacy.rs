//! Il port si prova contro l'originale, non contro chi l'ha riscritto.
//!
//! `golden/text-keys.json` non è stato scritto a mano: è l'uscita delle funzioni
//! TypeScript del vecchio albero, eseguite su input scelti per romperle. Se
//! questo file passa, le chiavi calcolate in Rust sono le stesse che stanno nei
//! file di sincronizzazione già presenti su Drive — che è l'unica definizione
//! utile di «il port è corretto».
//!
//! Tre comportamenti che una riscrittura ragionevole avrebbe cambiato, e che
//! questi vettori hanno colto:
//!
//! 1. si tolgono solo i combinanti U+0300–U+036F, non tutti;
//! 2. `\s` di JavaScript e `char::is_whitespace` differiscono su U+0085 e U+FEFF;
//! 3. il backslash sopravvive alla rimozione della punteggiatura.
//!
//! Quando la fase 8 porterà il nucleo su Android, lo stesso file sarà letto dai
//! test Kotlin attraverso UniFFI: è così che le due estremità non possono
//! divergere in silenzio, che è il difetto per cui questa riscrittura esiste.

use aether_domain::keys::{
    PlaylistKey, TrackKey, TrackKeyInput, normalize_key, upgrade_legacy_track_key,
};
use aether_domain::text::fold_text;
use serde::Deserialize;

const GOLDEN: &str = include_str!("golden/text-keys.json");

#[derive(Deserialize)]
struct Vettori {
    #[serde(rename = "foldText")]
    fold_text: Vec<CasoTesto>,
    #[serde(rename = "normalizeKey")]
    normalize_key: Vec<CasoTesto>,
    #[serde(rename = "trackKey")]
    track_key: Vec<CasoBrano>,
    #[serde(rename = "playlistKey")]
    playlist_key: Vec<CasoTesto>,
    #[serde(rename = "upgradeLegacyTrackKey")]
    upgrade_legacy_track_key: Vec<CasoTesto>,
}

#[derive(Deserialize)]
struct CasoTesto {
    #[serde(default)]
    nota: String,
    input: String,
    atteso: String,
}

#[derive(Deserialize)]
struct CasoBrano {
    #[serde(default)]
    nota: String,
    input: TagBrano,
    atteso: String,
}

#[derive(Deserialize)]
struct TagBrano {
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    album: Option<String>,
}

/// I test restituiscono `Result` e propagano con `?` invece di sbloccare con
/// `expect`: il divieto di `expect` vale anche qui, e un JSON dorato illeggibile
/// è un guasto da riportare, non da far esplodere.
fn vettori() -> Result<Vettori, serde_json::Error> {
    serde_json::from_str(GOLDEN)
}

/// Abbreviazione per il tipo di ritorno dei test.
type Esito = Result<(), serde_json::Error>;

/// Riporta *tutte* le differenze, non solo la prima.
///
/// Con `assert_eq!` dentro il ciclo si vedrebbe un caso per esecuzione, e un
/// port sbagliato in tre punti costerebbe tre giri. Qui una sola esecuzione dice
/// quanti e quali.
fn confronta(nome: &str, esiti: Vec<(String, String, String, String)>) {
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
                "  {input:?}\n      atteso   {atteso:?}\n      ottenuto {ottenuto:?}\n      ({nota})"
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn fold_text_combacia_con_l_originale() -> Esito {
    let casi = vettori()?.fold_text;
    assert!(
        casi.len() >= 50,
        "i vettori sono stati svuotati per sbaglio?"
    );
    confronta(
        "fold_text",
        casi.into_iter()
            .map(|c| {
                let ottenuto = fold_text(&c.input);
                (c.nota, c.input, c.atteso, ottenuto)
            })
            .collect(),
    );
    Ok(())
}

#[test]
fn normalize_key_combacia_con_l_originale() -> Esito {
    confronta(
        "normalize_key",
        vettori()?
            .normalize_key
            .into_iter()
            .map(|c| {
                let ottenuto = normalize_key(Some(&c.input));
                (c.nota, c.input, c.atteso, ottenuto)
            })
            .collect(),
    );
    Ok(())
}

#[test]
fn track_key_combacia_con_l_originale() -> Esito {
    confronta(
        "track_key",
        vettori()?
            .track_key
            .into_iter()
            .map(|c| {
                let ottenuto = TrackKey::compute(TrackKeyInput {
                    artist: c.input.artist.as_deref(),
                    title: c.input.title.as_deref(),
                    album: c.input.album.as_deref(),
                })
                .into_string();
                let mostrato = format!(
                    "{:?} / {:?} / {:?}",
                    c.input.artist, c.input.title, c.input.album
                );
                (c.nota, mostrato, c.atteso, ottenuto)
            })
            .collect(),
    );
    Ok(())
}

#[test]
fn playlist_key_combacia_con_l_originale() -> Esito {
    confronta(
        "playlist_key",
        vettori()?
            .playlist_key
            .into_iter()
            .map(|c| {
                let ottenuto = PlaylistKey::compute(Some(&c.input)).into_string();
                (c.nota, c.input, c.atteso, ottenuto)
            })
            .collect(),
    );
    Ok(())
}

#[test]
fn upgrade_legacy_track_key_combacia_con_l_originale() -> Esito {
    confronta(
        "upgrade_legacy_track_key",
        vettori()?
            .upgrade_legacy_track_key
            .into_iter()
            .map(|c| {
                let ottenuto = upgrade_legacy_track_key(&c.input).to_owned();
                (c.nota, c.input, c.atteso, ottenuto)
            })
            .collect(),
    );
    Ok(())
}
