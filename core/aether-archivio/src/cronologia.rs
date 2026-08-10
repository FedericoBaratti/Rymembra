//! Gli ascolti, nei due formati in cui Spotify li manda.
//!
//! # Due formati, e non è una variante
//!
//! L'archivio dei dati dell'account porta la cronologia **breve** — l'ultimo
//! anno scarso — in file che si chiamano `StreamingHistory0.json` o
//! `StreamingHistory_music_0.json`, con quattro campi in camelCase e i minuti
//! come unità più fine:
//!
//! ```json
//! [{"endTime": "2023-01-01 12:00", "artistName": "…", "trackName": "…", "msPlayed": 197000}]
//! ```
//!
//! L'archivio della cronologia **estesa**, che arriva separato e settimane
//! dopo, porta tutto dall'apertura dell'account, in `Streaming_History_Audio_*`,
//! con i campi in snake_case e altri nomi:
//!
//! ```json
//! [{"ts": "2020-01-01T12:00:00Z", "ms_played": 197000,
//!   "master_metadata_track_name": "…", "master_metadata_album_artist_name": "…",
//!   "master_metadata_album_album_name": "…", "spotify_track_uri": "spotify:track:…"}]
//! ```
//!
//! La differenza che conta non sono i nomi: è che il breve **non ha l'album**.
//! Senza album la chiave d'identità ha un segmento vuoto, quindi quelle righe si
//! abbinano al secondo gradino e non al primo. Funziona, ed è il motivo per cui
//! la scala esiste — ma spiega perché due archivi dello stesso account danno
//! numeri diversi, e vale la pena saperlo prima di cercare il guasto.
//!
//! # I podcast
//!
//! Ci sono, in entrambi, e si riconoscono da `episode_name` non nullo (esteso) o
//! da un titolo di brano assente (breve). Si scartano **contandoli**: sono
//! legittimamente in un archivio di Spotify e legittimamente fuori da una
//! libreria musicale, e la differenza fra «ne ho ignorati 3.400» e il silenzio è
//! che nel secondo caso qualcuno passa una sera a cercare gli ascolti mancanti.

use aether_domain::spotify::SpotifyTrack;
use aether_domain::spotify_account::AscoltoSpotify;
use serde_json::Value;

use crate::tempo::istante_ms;

/// Quel che una passata su un file di cronologia ha prodotto.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Letti {
    /// Gli ascolti riconosciuti.
    pub ascolti: Vec<AscoltoSpotify>,
    /// Quante righe erano di podcast.
    pub podcast: usize,
    /// Quante righe non si sono potute leggere: senza data, senza titolo, o
    /// con una data che non è una data.
    pub illeggibili: usize,
}

impl Letti {
    /// Assorbe il risultato di un altro file.
    pub fn assorbi(&mut self, altro: Self) {
        self.ascolti.extend(altro.ascolti);
        self.podcast = self.podcast.saturating_add(altro.podcast);
        self.illeggibili = self.illeggibili.saturating_add(altro.illeggibili);
    }
}

/// Legge un file di cronologia, in qualunque dei due formati sia.
///
/// Il formato si riconosce **dalla riga**, non dal nome del file: Spotify ha
/// rinominato questi file almeno tre volte, e un lettore che si fida del nome è
/// un lettore che si rompe da solo al prossimo cambio. Una riga con `ts` è
/// estesa, una con `endTime` è breve, e chiederglielo costa un confronto.
#[must_use]
pub fn leggi(corpo: &Value) -> Letti {
    let Some(righe) = corpo.as_array() else {
        // Non è un elenco: non è un file di cronologia. Chi chiama lo conta fra
        // gli illeggibili con il suo nome.
        return Letti {
            illeggibili: 1,
            ..Letti::default()
        };
    };

    let mut letti = Letti::default();
    for riga in righe {
        match riga_ad_ascolto(riga) {
            Esito::Ascolto(ascolto) => letti.ascolti.push(ascolto),
            Esito::Podcast => letti.podcast = letti.podcast.saturating_add(1),
            Esito::Illeggibile => letti.illeggibili = letti.illeggibili.saturating_add(1),
        }
    }
    letti
}

/// Cosa era una riga.
///
/// Le varianti sono sbilanciate — una porta un ascolto intero, le altre due
/// niente — e clippy lo fa notare. Incassare l'ascolto lo pareggerebbe, e
/// costerebbe **un'allocazione per riga**: su un archivio da quarantamila
/// ascolti sono quarantamila `malloc` per risparmiare duecento byte di pila su
/// un valore che vive lo spazio di un'iterazione e finisce comunque in un `Vec`.
#[allow(clippy::large_enum_variant)]
enum Esito {
    Ascolto(AscoltoSpotify),
    Podcast,
    Illeggibile,
}

fn riga_ad_ascolto(riga: &Value) -> Esito {
    // Un episodio nominato è un podcast, e lo si riconosce prima di tutto il
    // resto: nella cronologia estesa un episodio ha i campi della musica tutti
    // a `null`, quindi senza questo controllo finirebbe fra gli illeggibili — e
    // «3.400 righe illeggibili» manda a cercare un guasto che non c'è.
    if testo(riga, "episode_name").is_some() || testo(riga, "spotify_episode_uri").is_some() {
        return Esito::Podcast;
    }

    let (quando, ms, titolo, artista, album, uri) = if let Some(ts) = testo(riga, "ts") {
        (
            ts,
            numero(riga, "ms_played"),
            testo(riga, "master_metadata_track_name"),
            testo(riga, "master_metadata_album_artist_name"),
            testo(riga, "master_metadata_album_album_name"),
            testo(riga, "spotify_track_uri"),
        )
    } else if let Some(fine) = testo(riga, "endTime") {
        (
            fine,
            numero(riga, "msPlayed"),
            testo(riga, "trackName"),
            testo(riga, "artistName"),
            // La cronologia breve l'album non ce l'ha. Vedi la nota in testa al
            // modulo su cosa comporta per l'abbinamento.
            None,
            None,
        )
    } else {
        return Esito::Illeggibile;
    };

    let (Some(finito_ms), Some(titolo)) = (istante_ms(&quando), titolo) else {
        return Esito::Illeggibile;
    };

    Esito::Ascolto(AscoltoSpotify {
        finito_ms,
        ms_ascoltati: ms.unwrap_or(0),
        brano: SpotifyTrack {
            title: titolo,
            artist: artista,
            album,
            spotify_track_id: uri.as_deref().and_then(id_da_uri),
            ..SpotifyTrack::default()
        },
    })
}

/// L'identificativo dentro un URI `spotify:track:4iV5W9uYEdYUVa79Axb7Rh`.
///
/// `None` se non è un URI di brano: nella cronologia ci finiscono anche
/// `spotify:local:…` — i file che l'utente aveva aggiunto a Spotify dal proprio
/// disco — e quelli un identificativo di catalogo non ce l'hanno.
#[must_use]
pub fn id_da_uri(uri: &str) -> Option<String> {
    let resto = uri.strip_prefix("spotify:track:")?;
    (!resto.is_empty()).then(|| resto.to_owned())
}

/// Il campo, se c'è ed è una stringa non vuota.
///
/// Vuoto vale come assente: nell'archivio i campi mancanti sono a volte `null` e
/// a volte `""`, e distinguerli non serve a nessuno.
fn testo(riga: &Value, nome: &str) -> Option<String> {
    riga.get(nome)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(ToOwned::to_owned)
}

/// Il campo, se c'è ed è un numero non negativo.
fn numero(riga: &Value, nome: &str) -> Option<u64> {
    riga.get(nome).and_then(Value::as_u64)
}

#[cfg(test)]
mod prove {
    use super::*;
    use serde_json::json;

    #[test]
    fn la_cronologia_estesa_si_legge() {
        let corpo = json!([{
            "ts": "2020-01-01T12:00:00Z",
            "ms_played": 197_000,
            "master_metadata_track_name": "Song 2",
            "master_metadata_album_artist_name": "Blur",
            "master_metadata_album_album_name": "Blur",
            "spotify_track_uri": "spotify:track:4iV5W9uYEdYUVa79Axb7Rh"
        }]);
        let letti = leggi(&corpo);
        assert_eq!(letti.ascolti.len(), 1);
        let a = letti.ascolti.first().expect("un ascolto");
        assert_eq!(a.finito_ms, 1_577_880_000_000);
        assert_eq!(a.ms_ascoltati, 197_000);
        assert_eq!(a.brano.title, "Song 2");
        assert_eq!(a.brano.artist.as_deref(), Some("Blur"));
        assert_eq!(a.brano.album.as_deref(), Some("Blur"));
        assert_eq!(
            a.brano.spotify_track_id.as_deref(),
            Some("4iV5W9uYEdYUVa79Axb7Rh")
        );
    }

    #[test]
    fn la_cronologia_breve_si_legge_e_non_ha_l_album() {
        // Non è una mancanza da correggere: è il formato. Comporta che quelle
        // righe si abbinino al secondo gradino invece che al primo, ed è la
        // ragione per cui due archivi dello stesso account danno numeri diversi.
        let corpo = json!([{
            "endTime": "2023-01-01 12:00",
            "artistName": "Blur",
            "trackName": "Song 2",
            "msPlayed": 197_000
        }]);
        let letti = leggi(&corpo);
        let a = letti.ascolti.first().expect("un ascolto");
        assert_eq!(a.brano.title, "Song 2");
        assert_eq!(a.brano.album, None);
        assert_eq!(a.finito_ms, 1_672_574_400_000);
    }

    #[test]
    fn il_formato_si_riconosce_dalla_riga_non_dal_nome_del_file() {
        // Spotify ha rinominato questi file almeno tre volte. Un lettore che si
        // fida del nome si rompe da solo al prossimo cambio.
        let misto = json!([
            {"ts": "2020-01-01T12:00:00Z", "ms_played": 1, "master_metadata_track_name": "A"},
            {"endTime": "2023-01-01 12:00", "trackName": "B", "msPlayed": 2},
        ]);
        let letti = leggi(&misto);
        assert_eq!(letti.ascolti.len(), 2);
        assert_eq!(letti.illeggibili, 0);
    }

    #[test]
    fn i_podcast_si_scartano_contandoli() {
        // Nella cronologia estesa un episodio ha i campi della musica tutti a
        // null: senza il controllo su `episode_name` finirebbe fra gli
        // illeggibili, e «3.400 righe illeggibili» manda a cercare un guasto
        // che non c'è.
        let corpo = json!([{
            "ts": "2020-01-01T12:00:00Z",
            "ms_played": 1_800_000,
            "master_metadata_track_name": null,
            "master_metadata_album_artist_name": null,
            "episode_name": "Una puntata",
            "spotify_episode_uri": "spotify:episode:abc"
        }]);
        let letti = leggi(&corpo);
        assert!(letti.ascolti.is_empty());
        assert_eq!(letti.podcast, 1);
        assert_eq!(letti.illeggibili, 0);
    }

    #[test]
    fn una_riga_senza_data_o_senza_titolo_si_conta_e_si_lascia_stare() {
        let corpo = json!([
            {"ts": "non una data", "master_metadata_track_name": "A", "ms_played": 1},
            {"ts": "2020-01-01T12:00:00Z", "ms_played": 1},
            {"niente": "di utile"},
            {"ts": "2020-01-01T12:00:00Z", "master_metadata_track_name": "  ", "ms_played": 1},
        ]);
        let letti = leggi(&corpo);
        assert!(letti.ascolti.is_empty());
        assert_eq!(
            letti.illeggibili, 4,
            "una per riga, e nessuna ferma le altre"
        );
    }

    #[test]
    fn un_file_che_non_e_un_elenco_e_un_file_illeggibile() {
        assert_eq!(leggi(&json!({"non": "un elenco"})).illeggibili, 1);
    }

    #[test]
    fn i_brani_locali_non_hanno_un_identificativo_di_catalogo() {
        // `spotify:local:…` sono i file che l'utente aveva aggiunto a Spotify
        // dal proprio disco. L'ascolto vale, l'identificativo no.
        assert_eq!(id_da_uri("spotify:track:abc"), Some("abc".to_owned()));
        assert_eq!(id_da_uri("spotify:local:Blur+Song+2"), None);
        assert_eq!(id_da_uri("spotify:track:"), None);
        assert_eq!(id_da_uri(""), None);
    }

    #[test]
    fn assorbire_somma_tutto() {
        let mut a =
            leggi(&json!([{"endTime": "2023-01-01 12:00", "trackName": "A", "msPlayed": 1}]));
        let b = leggi(&json!([
            {"ts": "2020-01-01T12:00:00Z", "episode_name": "P"},
            {"non": "leggibile"},
        ]));
        a.assorbi(b);
        assert_eq!(a.ascolti.len(), 1);
        assert_eq!(a.podcast, 1);
        assert_eq!(a.illeggibili, 1);
    }
}
