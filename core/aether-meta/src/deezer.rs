//! Deezer: il terzo parere, e la copertina più grande di tutte.
//!
//! Anche questo punto di ricerca è pubblico e senza chiave. Rispetto a iTunes
//! porta due cose in più — `cover_xl` è a mille pixel, e il catalogo è europeo,
//! quindi copre meglio la musica che una libreria italiana contiene davvero —
//! e una in meno: non dà il genere del brano.
//!
//! # Perché tre fonti e non due
//!
//! Perché il consenso serve fra fonti **diverse**, e due sole vuol dire che una
//! qualunque delle due, cadendo, spegne la possibilità di applicare qualcosa.
//! Con tre, ne basta una che risponda insieme a MusicBrainz.

use aether_domain::enrich::{Candidate, Fonte, normalize_for_match};
use aether_domain::errors::AppError;
use aether_net::percento;
use serde_json::Value;

use crate::deposito::VIVE_RICERCA_MS;
use crate::{Fornitori, itunes};

/// Interpreta la risposta di una ricerca. Funzione pura.
#[must_use]
pub fn interpreta(corpo: &[u8]) -> Vec<Candidate> {
    let Ok(letto) = serde_json::from_slice::<Value>(corpo) else {
        return Vec::new();
    };
    let Some(elenco) = letto.get("data").and_then(Value::as_array) else {
        return Vec::new();
    };
    elenco
        .iter()
        .filter_map(|voce| {
            let titolo = voce.get("title").and_then(Value::as_str)?;
            let artista = voce
                .get("artist")
                .and_then(|a| a.get("name"))
                .and_then(Value::as_str)?;
            let album = voce.get("album");
            Some(Candidate {
                fonte: Some(Fonte::Deezer),
                title: titolo.to_owned(),
                artist: artista.to_owned(),
                album: album
                    .and_then(|a| a.get("title"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                // Deezer non dà l'anno sul brano: sta sull'album, che questa
                // ricerca non restituisce per esteso. Meglio `None` che l'anno
                // di qualcos'altro.
                year: None,
                genre: None,
                // I secondi, non i millisecondi: è l'unica fonte delle tre a
                // darli così, ed è un errore che si fa una volta sola perché
                // produce scarti di durata di tre ordini di grandezza — cioè
                // nessuna corrispondenza, mai, senza nessun messaggio.
                duration_ms: voce
                    .get("duration")
                    .and_then(Value::as_u64)
                    .filter(|d| *d > 0)
                    .map(|secondi| secondi.saturating_mul(1000)),
                cover_url: album.and_then(copertina),
                mb_recording_id: None,
                mb_release_id: None,
                mb_release_group_id: None,
            })
        })
        .collect()
}

/// La copertina più grande fra quelle che l'album dichiara.
fn copertina(album: &Value) -> Option<String> {
    for campo in ["cover_xl", "cover_big", "cover_medium", "cover"] {
        if let Some(url) = album
            .get(campo)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|u| u.starts_with("https://"))
        {
            return Some(url.to_owned());
        }
    }
    None
}

/// I candidati che Deezer propone per questo brano.
///
/// # Errori
///
/// L'errore di rete così com'è: chi chiama lo conta come «una fonte in meno».
pub fn cerca(
    fornitori: &Fornitori,
    titolo: &str,
    artista: &str,
) -> Result<Vec<Candidate>, AppError> {
    // Lo stesso termine di iTunes, e non uno costruito qui: due fonti
    // interrogate con due domande diverse darebbero un disaccordo che parla
    // delle domande invece che dei cataloghi — e il consenso fra loro è
    // esattamente la prova su cui si decide di scrivere.
    let Some(termine) = itunes::termine(titolo, artista) else {
        return Ok(Vec::new());
    };
    let url = format!(
        "https://api.deezer.com/search?q={}&limit=10",
        percento(&termine)
    );
    let corpo = fornitori.json(
        &fornitori.deezer,
        "deezer-ricerca",
        &normalize_for_match(&termine),
        &url,
        VIVE_RICERCA_MS,
    )?;
    Ok(corpo.as_deref().map(interpreta).unwrap_or_default())
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn i_secondi_diventano_millisecondi() {
        // L'errore che si fa una volta sola: senza la moltiplicazione, ogni
        // scarto di durata è di tre ordini di grandezza e nessun brano
        // corrisponde mai, senza nessun messaggio che dica perché.
        let corpo = br#"{"data":[{"title":"Poetica","duration":297,
            "artist":{"name":"Cesare Cremonini"},
            "album":{"title":"Possibili scenari","cover_xl":"https://c/xl.jpg",
                     "cover_big":"https://c/big.jpg"}}]}"#;
        let candidati = interpreta(corpo);
        let primo = candidati.first().expect("un candidato");
        assert_eq!(primo.duration_ms, Some(297_000));
        assert_eq!(primo.fonte, Some(Fonte::Deezer));
        assert_eq!(primo.cover_url.as_deref(), Some("https://c/xl.jpg"));
    }

    #[test]
    fn si_prende_la_copertina_piu_grande_disponibile() {
        let corpo = br#"{"data":[{"title":"X","artist":{"name":"Y"},
            "album":{"cover_medium":"https://c/m.jpg","cover":"https://c/s.jpg"}}]}"#;
        assert_eq!(
            interpreta(corpo)
                .first()
                .and_then(|c| c.cover_url.as_deref()),
            Some("https://c/m.jpg")
        );
    }

    #[test]
    fn una_copertina_non_cifrata_non_si_segue() {
        // L'indirizzo arriva da una risposta di rete e finisce in una
        // richiesta: `http:` non ha motivo di essere seguito.
        let corpo = br#"{"data":[{"title":"X","artist":{"name":"Y"},
            "album":{"cover_xl":"http://c/xl.jpg"}}]}"#;
        assert_eq!(
            interpreta(corpo)
                .first()
                .and_then(|c| c.cover_url.as_deref()),
            None
        );
    }

    #[test]
    fn una_voce_senza_interprete_si_salta() {
        let corpo = br#"{"data":[{"title":"Solo il titolo"},
            {"title":"Buono","artist":{"name":"Buono"}}]}"#;
        assert_eq!(interpreta(corpo).len(), 1);
    }

    #[test]
    fn un_corpo_storto_non_fa_cadere_niente() {
        assert!(interpreta(b"non json").is_empty());
        assert!(interpreta(b"{}").is_empty());
        assert!(interpreta(br#"{"data":{}}"#).is_empty());
    }
}
