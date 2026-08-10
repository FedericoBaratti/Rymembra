//! L'ultima spiaggia: titolo e copertina, nient'altro.
//!
//! `open.spotify.com/oembed` è il punto pubblico e documentato che Spotify tiene
//! in piedi perché i siti possano incorporare un riquadro. Non dà l'elenco dei
//! brani e non lo darà mai: serve a far vedere all'utente **che cosa** ha
//! incollato quando tutto il resto ha fallito, cioè a distinguere «il link è
//! sbagliato» da «Spotify oggi non si fa leggere».
//!
//! Un contenuto che arriva da qui non si può importare, e chi lo riceve deve
//! trattarlo come un'anteprima, non come un risultato: `tracks` è vuoto e
//! [`aether_domain::spotify::SpotifySource::OEmbed`] lo dichiara.

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::spotify::{SpotifyContent, SpotifySource};
use aether_net::http::{Corpo, Metodo, Rete, Richiesta, percento};
use serde_json::Value;

use crate::url::Riferimento;

/// Chiede a oEmbed titolo e copertina.
///
/// # Errori
///
/// `spotify.notPublic` su un 404 — qui vuol dire davvero che il contenuto non
/// è pubblico, perché questo punto risponde a tutto ciò che lo è.
pub fn risolvi(rete: &Rete, riferimento: &Riferimento) -> Result<SpotifyContent, AppError> {
    let url = format!(
        "https://open.spotify.com/oembed?url={}",
        percento(&riferimento.url_pubblico())
    );
    let risposta = rete.esegui(Richiesta {
        metodo: Metodo::Get,
        url: &url,
        intestazioni: &[("Accept", "application/json")],
        corpo: Corpo::Niente,
    })?;
    if risposta.stato == 404 {
        return Err(AppError::new(ErrorCode::SpotifyNotPublic));
    }
    if !risposta.e_andata() {
        return Err(rete.stato_a_errore(&risposta, &url));
    }
    interpreta(&risposta.corpo, riferimento).ok_or_else(|| {
        AppError::new(ErrorCode::SpotifyResolveFailed)
            .with_message("oEmbed non ha nemmeno restituito un titolo")
    })
}

/// Interpreta la risposta. Funzione pura: si prova senza rete.
#[must_use]
pub fn interpreta(corpo: &[u8], riferimento: &Riferimento) -> Option<SpotifyContent> {
    let letto: Value = serde_json::from_slice(corpo).ok()?;
    let titolo = letto
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    Some(SpotifyContent {
        kind: riferimento.genere,
        id: riferimento.id.clone(),
        title: titolo.to_owned(),
        // Nessun autore: l'unico campo che ci somiglia è `provider_name`, che
        // vale sempre «Spotify» e come firma di una playlist sarebbe una bugia.
        author: None,
        cover_url: letto
            .get("thumbnail_url")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        tracks: Vec::new(),
        // Non si dichiara nessun totale: qui non si è letto nemmeno un brano, e
        // un totale farebbe apparire un troncamento dove c'è invece un'assenza
        // completa di elenco.
        declared_total: None,
        source: SpotifySource::OEmbed,
    })
}

#[cfg(test)]
mod prove {
    use super::*;
    use aether_domain::spotify::SpotifyKind;

    fn rif() -> Riferimento {
        Riferimento {
            genere: SpotifyKind::Playlist,
            id: "37i9dQZF1DXcBWIGoYBM5M".to_owned(),
        }
    }

    #[test]
    fn titolo_e_copertina_bastano_a_far_vedere_cosa_si_e_incollato() {
        let corpo = br#"{"title":"Discover Weekly","thumbnail_url":"https://c/x.jpg",
            "provider_name":"Spotify","html":"<iframe...>"}"#;
        let Some(c) = interpreta(corpo, &rif()) else {
            panic!("si deve leggere");
        };
        assert_eq!(c.title, "Discover Weekly");
        assert_eq!(c.cover_url.as_deref(), Some("https://c/x.jpg"));
        assert!(c.tracks.is_empty(), "oEmbed non dà mai i brani");
        assert_eq!(c.source, SpotifySource::OEmbed);
        assert_eq!(
            c.truncation(),
            None,
            "nessun elenco non è un elenco troncato"
        );
    }

    #[test]
    fn senza_titolo_non_c_e_niente_da_mostrare() {
        assert!(interpreta(br#"{"thumbnail_url":"https://c"}"#, &rif()).is_none());
        assert!(interpreta(b"non json", &rif()).is_none());
        assert!(interpreta(br#"{"title":"   "}"#, &rif()).is_none());
    }
}
