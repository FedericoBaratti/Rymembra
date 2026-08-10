//! I brani salvati, gli album e gli artisti seguiti, da `YourLibrary.json`.
//!
//! ```json
//! {"tracks":  [{"artist": "Blur", "album": "Blur", "track": "Song 2",
//!               "uri": "spotify:track:…"}],
//!  "albums":  [{"artist": "Blur", "album": "Blur", "uri": "spotify:album:…"}],
//!  "artists": [{"name": "Blur", "uri": "spotify:artist:…"}],
//!  "shows": [], "episodes": [], "bannedTracks": [], "other": []}
//! ```
//!
//! # I nomi dei campi cambiano fra le sezioni
//!
//! `tracks` chiama il titolo `track`, `artists` chiama il nome `name`. Non è un
//! refuso di questa documentazione: è come il file è fatto, ed è la ragione per
//! cui le tre sezioni si leggono con tre funzioni invece che con una
//! parametrizzata — che avrebbe tre parametri, cioè sarebbe le tre funzioni con
//! in più la possibilità di passarli in ordine sbagliato.
//!
//! # Cosa non si legge, e non è una dimenticanza
//!
//! `shows` ed `episodes` sono podcast. `bannedTracks` sono i brani che l'utente
//! ha detto di non voler più sentire: è un'informazione vera e Aether non ha
//! nessun posto dove metterla — nessuna colonna, nessuna schermata — e inventare
//! una tabella per un dato che nessuno leggerà è peggio che non importarlo.
//! `other` è quel che Spotify ci mette quando non sa dove metterlo.
//!
//! Tutte e quattro vengono **contate** e dichiarate nel rapporto, perché «ho
//! ignorato 340 podcast» è un'informazione e il silenzio no.

use aether_domain::spotify::SpotifyTrack;
use aether_domain::spotify_account::{AlbumSpotify, ArtistaSpotify};
use serde_json::Value;

use crate::cronologia::id_da_uri;

/// Quel che `YourLibrary.json` conteneva.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Letta {
    /// I «Brani che ti piacciono».
    pub preferiti: Vec<SpotifyTrack>,
    /// Gli album salvati.
    pub album: Vec<AlbumSpotify>,
    /// Gli artisti seguiti.
    pub artisti: Vec<ArtistaSpotify>,
    /// Quante voci si sono lasciate stare, e di che tipo.
    pub non_musica: NonMusica,
}

/// Quel che c'era nel file e non entra in una libreria musicale.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NonMusica {
    /// Podcast seguiti e loro puntate salvate.
    pub podcast: usize,
    /// Brani messi al bando: veri, e senza un posto dove andare.
    pub al_bando: usize,
    /// Quel che Spotify non sapeva dove mettere.
    pub altro: usize,
}

impl NonMusica {
    /// Quante voci in tutto.
    #[must_use]
    pub const fn totale(&self) -> usize {
        self.podcast
            .saturating_add(self.al_bando)
            .saturating_add(self.altro)
    }
}

/// Legge `YourLibrary.json`.
///
/// Non fallisce mai: un file con le sezioni assenti dà semplicemente elenchi
/// vuoti. Chi chiama distingue «non c'era niente da leggere» da «non c'era il
/// file» guardando se il nome era fra quelli trovati.
#[must_use]
pub fn leggi(corpo: &Value) -> Letta {
    Letta {
        preferiti: sezione(corpo, "tracks").filter_map(brano).collect(),
        album: sezione(corpo, "albums").filter_map(album).collect(),
        artisti: sezione(corpo, "artists").filter_map(artista).collect(),
        non_musica: NonMusica {
            podcast: sezione(corpo, "shows").count() + sezione(corpo, "episodes").count(),
            al_bando: sezione(corpo, "bannedTracks").count()
                + sezione(corpo, "bannedArtists").count(),
            altro: sezione(corpo, "other").count(),
        },
    }
}

/// Le voci di una sezione, o niente se la sezione non c'è.
fn sezione<'a>(corpo: &'a Value, nome: &str) -> impl Iterator<Item = &'a Value> {
    corpo
        .get(nome)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn brano(voce: &Value) -> Option<SpotifyTrack> {
    Some(SpotifyTrack {
        // Qui il titolo si chiama `track`. Nella cronologia si chiama
        // `trackName`. Nelle playlist si chiama `trackName`. Sono tre file
        // scritti in momenti diversi da persone diverse.
        title: testo(voce, "track")?,
        artist: testo(voce, "artist"),
        album: testo(voce, "album"),
        spotify_track_id: testo(voce, "uri").as_deref().and_then(id_da_uri),
        ..SpotifyTrack::default()
    })
}

fn album(voce: &Value) -> Option<AlbumSpotify> {
    Some(AlbumSpotify {
        titolo: testo(voce, "album")?,
        artista: testo(voce, "artist"),
        spotify_id: testo(voce, "uri")
            .as_deref()
            .and_then(|uri| id_da_prefisso(uri, "spotify:album:")),
        // L'archivio elenca gli album salvati **senza** le loro tracce. Un album
        // senza brani porta comunque il proprio identificativo, che è quel che
        // serve a `albums.spotify_id`.
        brani: Vec::new(),
    })
}

fn artista(voce: &Value) -> Option<ArtistaSpotify> {
    Some(ArtistaSpotify {
        // E qui il nome si chiama `name`.
        nome: testo(voce, "name")?,
        spotify_id: testo(voce, "uri")
            .as_deref()
            .and_then(|uri| id_da_prefisso(uri, "spotify:artist:")),
    })
}

/// L'identificativo dopo un prefisso `spotify:qualcosa:`.
fn id_da_prefisso(uri: &str, prefisso: &str) -> Option<String> {
    let resto = uri.strip_prefix(prefisso)?;
    (!resto.is_empty()).then(|| resto.to_owned())
}

/// Il campo, se c'è ed è una stringa non vuota.
fn testo(voce: &Value, nome: &str) -> Option<String> {
    voce.get(nome)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod prove {
    use super::*;
    use serde_json::json;

    fn completo() -> Value {
        json!({
            "tracks": [
                {"artist": "Blur", "album": "Blur", "track": "Song 2",
                 "uri": "spotify:track:abc"},
                {"artist": "Gorillaz", "album": "Demon Days", "track": "Feel Good Inc",
                 "uri": "spotify:track:def"}
            ],
            "albums": [{"artist": "Blur", "album": "Parklife",
                        "uri": "spotify:album:ghi"}],
            "artists": [{"name": "Blur", "uri": "spotify:artist:jkl"}],
            "shows": [{"name": "Un podcast"}],
            "episodes": [{"name": "Una puntata"}, {"name": "Un'altra"}],
            "bannedTracks": [{"track": "Mai più"}],
            "other": [{"boh": 1}]
        })
    }

    #[test]
    fn le_tre_sezioni_che_contano_si_leggono() {
        let letta = leggi(&completo());
        assert_eq!(letta.preferiti.len(), 2);
        assert_eq!(
            letta.preferiti.first().map(|b| b.title.as_str()),
            Some("Song 2")
        );
        assert_eq!(
            letta
                .preferiti
                .first()
                .and_then(|b| b.spotify_track_id.as_deref()),
            Some("abc")
        );

        assert_eq!(letta.album.len(), 1);
        let a = letta.album.first().expect("un album");
        assert_eq!(a.titolo, "Parklife");
        assert_eq!(a.spotify_id.as_deref(), Some("ghi"));
        assert!(
            a.brani.is_empty(),
            "l'archivio non dà le tracce degli album"
        );

        assert_eq!(letta.artisti.len(), 1);
        assert_eq!(letta.artisti.first().map(|a| a.nome.as_str()), Some("Blur"));
        assert_eq!(
            letta.artisti.first().and_then(|a| a.spotify_id.as_deref()),
            Some("jkl")
        );
    }

    #[test]
    fn quel_che_non_e_musica_si_conta_invece_di_sparire() {
        // «Ho ignorato 340 podcast» è un'informazione; il silenzio no.
        let letta = leggi(&completo());
        assert_eq!(letta.non_musica.podcast, 3, "uno show più due puntate");
        assert_eq!(letta.non_musica.al_bando, 1);
        assert_eq!(letta.non_musica.altro, 1);
        assert_eq!(letta.non_musica.totale(), 5);
    }

    #[test]
    fn un_file_senza_sezioni_non_e_un_guasto() {
        // Chi ha un account nuovo ha un `YourLibrary.json` quasi vuoto.
        let letta = leggi(&json!({}));
        assert!(letta.preferiti.is_empty());
        assert!(letta.album.is_empty());
        assert!(letta.artisti.is_empty());
        assert_eq!(letta.non_musica.totale(), 0);
    }

    #[test]
    fn una_voce_senza_il_campo_che_la_nomina_si_lascia_stare() {
        let letta = leggi(&json!({
            "tracks":  [{"artist": "Solo artista"}, {"track": "Buono"}],
            "artists": [{"uri": "spotify:artist:x"}, {"name": "Buono"}]
        }));
        assert_eq!(letta.preferiti.len(), 1);
        assert_eq!(letta.artisti.len(), 1);
    }

    #[test]
    fn un_uri_del_tipo_sbagliato_non_diventa_un_identificativo() {
        let letta = leggi(&json!({
            "albums":  [{"album": "X", "uri": "spotify:track:abc"}],
            "artists": [{"name": "Y", "uri": "spotify:album:def"}]
        }));
        assert_eq!(
            letta.album.first().and_then(|a| a.spotify_id.as_deref()),
            None
        );
        assert_eq!(
            letta.artisti.first().and_then(|a| a.spotify_id.as_deref()),
            None
        );
    }
}
