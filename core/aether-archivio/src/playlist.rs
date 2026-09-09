//! Le playlist, da `Playlist1.json`.
//!
//! Il file è uno solo per molte playlist, e quando sono tante Spotify lo spezza
//! numerandolo: `Playlist1.json`, `Playlist2.json`. La forma:
//!
//! ```json
//! {"playlists": [
//!   {"name": "Corsa",
//!    "lastModifiedDate": "2024-03-01",
//!    "description": "…",
//!    "items": [
//!      {"track": {"trackName": "…", "artistName": "…", "albumName": "…",
//!                 "trackUri": "spotify:track:…"},
//!       "episode": null, "localTrack": null, "addedDate": "2023-11-04"}
//!    ]}
//! ]}
//! ```
//!
//! # Quel che non è un brano
//!
//! Ogni voce ha tre caselle e due sono quasi sempre nulle. `episode` è un
//! podcast; `localTrack` è un file che l'utente aveva aggiunto a Spotify dal
//! proprio disco — e quello è il caso interessante, perché **è già suo**: molto
//! probabilmente è ancora lì. Non porta però né un percorso utilizzabile né un
//! identificativo, quindi l'unica cosa onesta è trattarlo come un brano
//! qualunque e lasciare che la scala d'abbinamento lo ritrovi per artista e
//! titolo, che è precisamente quel che sa fare.
//!
//! # Nessuna durata
//!
//! L'archivio non la dice, per nessun brano. Non è un dettaglio: la durata è
//! quel che sorveglia il terzo e il quarto gradino dell'abbinamento, e senza di
//! lei quei gradini abbinano solo quando il candidato è **unico**. Il risultato
//! è che un archivio produce qualche mancante in più di quanti ne produrrebbe la
//! Web API sullo stesso account. È il comportamento voluto — nel dubbio si
//! dichiara mancante — ma spiega la differenza fra i due numeri.

use aether_domain::esterno::BranoEsterno;
use aether_domain::spotify_account::PlaylistSpotify;
use serde_json::Value;

use crate::cronologia::id_da_uri;

/// Quel che una passata su un file di playlist ha prodotto.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lette {
    /// Le playlist riconosciute, con i loro brani.
    pub playlist: Vec<PlaylistSpotify>,
    /// Quante voci erano podcast.
    pub podcast: usize,
    /// Quante voci non avevano né un brano né un episodio riconoscibile.
    pub illeggibili: usize,
}

impl Lette {
    /// Assorbe il risultato di un altro file.
    pub fn assorbi(&mut self, altro: Self) {
        self.playlist.extend(altro.playlist);
        self.podcast = self.podcast.saturating_add(altro.podcast);
        self.illeggibili = self.illeggibili.saturating_add(altro.illeggibili);
    }
}

/// Legge un file di playlist.
#[must_use]
pub fn leggi(corpo: &Value) -> Lette {
    let Some(elenco) = corpo.get("playlists").and_then(Value::as_array) else {
        return Lette {
            illeggibili: 1,
            ..Lette::default()
        };
    };

    let mut lette = Lette::default();
    for voce in elenco {
        let Some(nome) = testo(voce, "name") else {
            // Una playlist senza nome non è importabile: `PlaylistKey` nasce dal
            // nome, e una chiave vuota si scontrerebbe con ogni altra playlist
            // senza nome invece di crearne una seconda.
            lette.illeggibili = lette.illeggibili.saturating_add(1);
            continue;
        };

        let mut brani = Vec::new();
        for item in voce
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            match voce_a_brano(item) {
                Esito::Brano(brano) => brani.push(brano),
                Esito::Podcast => lette.podcast = lette.podcast.saturating_add(1),
                Esito::Illeggibile => {
                    lette.illeggibili = lette.illeggibili.saturating_add(1);
                }
            }
        }

        lette.playlist.push(PlaylistSpotify {
            nome,
            descrizione: testo(voce, "description"),
            // L'archivio non dà l'identificativo della playlist: `Playlist1.json`
            // non lo contiene. Chi si sincronizza dall'API ce l'ha, chi passa di
            // qui no, e la conseguenza è scritta in `005_account.sql`.
            spotify_id: None,
            // Nessun totale dichiarato, quindi nessuna troncatura possibile: il
            // file è un file, o c'è tutto o non si apre. È l'unico posto in cui
            // questa importazione è più semplice di quella da un link.
            dichiarati: None,
            brani,
        });
    }
    lette
}

/// Cosa era una voce di playlist.
///
/// Sbilanciato di proposito, per la stessa ragione scritta accanto all'omonimo
/// in [`crate::cronologia`]: incassare il brano costerebbe un'allocazione per
/// voce di playlist, e a guadagnarci sarebbe soltanto la dimensione di un valore
/// che vive un'iterazione.
#[expect(
    clippy::large_enum_variant,
    reason = "incassare il brano costerebbe un'allocazione per voce di playlist, e a guadagnarci \
              sarebbe soltanto la dimensione di un valore che vive un'iterazione"
)]
enum Esito {
    Brano(BranoEsterno),
    Podcast,
    Illeggibile,
}

fn voce_a_brano(item: &Value) -> Esito {
    if item.get("episode").is_some_and(|e| !e.is_null()) {
        return Esito::Podcast;
    }

    // `track` per i brani del catalogo, `localTrack` per i file che l'utente
    // aveva aggiunto a Spotify dal proprio disco. Hanno gli stessi campi, e
    // quello locale ha la miglior probabilità di essere già in libreria.
    let Some(brano) = item
        .get("track")
        .filter(|v| !v.is_null())
        .or_else(|| item.get("localTrack").filter(|v| !v.is_null()))
    else {
        return Esito::Illeggibile;
    };

    let Some(titolo) = testo(brano, "trackName") else {
        return Esito::Illeggibile;
    };

    Esito::Brano(BranoEsterno {
        title: titolo,
        artist: testo(brano, "artistName"),
        album: testo(brano, "albumName"),
        spotify_track_id: testo(brano, "trackUri").as_deref().and_then(id_da_uri),
        // La durata l'archivio non la dice. Vedi la nota in testa al modulo su
        // cosa comporta per i gradini larghi dell'abbinamento.
        ..BranoEsterno::default()
    })
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

    #[test]
    fn una_playlist_con_i_suoi_brani_in_ordine() {
        let corpo = json!({"playlists": [{
            "name": "Corsa",
            "description": "Per correre",
            "items": [
                {"track": {"trackName": "Song 2", "artistName": "Blur",
                           "albumName": "Blur", "trackUri": "spotify:track:abc"},
                 "episode": null, "localTrack": null, "addedDate": "2023-11-04"},
                {"track": {"trackName": "Feel Good Inc", "artistName": "Gorillaz",
                           "albumName": "Demon Days", "trackUri": "spotify:track:def"},
                 "episode": null, "localTrack": null, "addedDate": "2023-11-05"}
            ]
        }]});

        let lette = leggi(&corpo);
        assert_eq!(lette.playlist.len(), 1);
        let p = lette.playlist.first().expect("una playlist");
        assert_eq!(p.nome, "Corsa");
        assert_eq!(p.descrizione.as_deref(), Some("Per correre"));
        assert_eq!(p.brani.len(), 2);
        assert_eq!(p.brani.first().map(|b| b.title.as_str()), Some("Song 2"));
        assert_eq!(
            p.brani.get(1).map(|b| b.title.as_str()),
            Some("Feel Good Inc"),
            "l'ordine è quello del file"
        );
        assert_eq!(p.troncatura(), None, "un file o c'è tutto o non si apre");
    }

    #[test]
    fn un_brano_locale_vale_come_un_brano() {
        // È già suo: molto probabilmente è ancora sul disco, e la scala
        // d'abbinamento lo ritrova per artista e titolo.
        let corpo = json!({"playlists": [{
            "name": "Mista",
            "items": [
                {"track": null, "episode": null,
                 "localTrack": {"trackName": "Un mio file", "artistName": "Io",
                                "albumName": "Casa"}}
            ]
        }]});
        let lette = leggi(&corpo);
        let p = lette.playlist.first().expect("una playlist");
        assert_eq!(p.brani.len(), 1);
        assert_eq!(
            p.brani.first().map(|b| b.title.as_str()),
            Some("Un mio file")
        );
        assert_eq!(lette.illeggibili, 0);
    }

    #[test]
    fn i_podcast_in_playlist_si_contano_e_non_entrano() {
        let corpo = json!({"playlists": [{
            "name": "Mista",
            "items": [
                {"track": null, "localTrack": null,
                 "episode": {"episodeName": "Una puntata"}},
                {"track": {"trackName": "Song 2", "artistName": "Blur"},
                 "episode": null, "localTrack": null}
            ]
        }]});
        let lette = leggi(&corpo);
        assert_eq!(lette.podcast, 1);
        assert_eq!(
            lette.playlist.first().map(|p| p.brani.len()),
            Some(1),
            "la playlist esiste comunque, con quel che era musica"
        );
    }

    #[test]
    fn una_playlist_senza_nome_non_si_importa() {
        // `PlaylistKey` nasce dal nome: una chiave vuota si scontrerebbe con
        // ogni altra playlist senza nome invece di crearne una seconda.
        let corpo = json!({"playlists": [
            {"name": "", "items": []},
            {"items": []},
            {"name": "Vera", "items": []}
        ]});
        let lette = leggi(&corpo);
        assert_eq!(lette.playlist.len(), 1);
        assert_eq!(lette.illeggibili, 2);
    }

    #[test]
    fn una_playlist_vuota_resta_una_playlist() {
        // Su Spotify esiste, e chi importa il proprio account se l'aspetta.
        let lette = leggi(&json!({"playlists": [{"name": "Ancora vuota", "items": []}]}));
        assert_eq!(lette.playlist.len(), 1);
        assert_eq!(lette.playlist.first().map(|p| p.brani.len()), Some(0));
    }

    #[test]
    fn una_voce_senza_niente_dentro_si_conta() {
        let corpo = json!({"playlists": [{
            "name": "Strana",
            "items": [{"track": null, "episode": null, "localTrack": null}, {}]
        }]});
        assert_eq!(leggi(&corpo).illeggibili, 2);
    }

    #[test]
    fn un_file_che_non_e_un_file_di_playlist_e_illeggibile() {
        assert_eq!(leggi(&json!({"altro": []})).illeggibili, 1);
        assert_eq!(leggi(&json!([])).illeggibili, 1);
    }

    #[test]
    fn i_file_numerati_si_sommano() {
        // Quando le playlist sono tante Spotify spezza il file numerandolo.
        let mut uno = leggi(&json!({"playlists": [{"name": "A", "items": []}]}));
        let due = leggi(&json!({"playlists": [{"name": "B", "items": []}]}));
        uno.assorbi(due);
        assert_eq!(uno.playlist.len(), 2);
    }
}
