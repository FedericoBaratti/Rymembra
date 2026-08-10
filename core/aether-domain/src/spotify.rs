//! Quel che Spotify racconta di un brano, prima che diventi qualcosa di nostro.
//!
//! Sono tipi di trasporto: li riempie chi parla con la rete (`aether-spotify`),
//! li legge chi decide (`spotify_plan`) e chi scrive (`aether-app`). Stanno qui,
//! e non nel crate che fa le richieste, per la stessa ragione per cui ci sta
//! [`crate::scan_plan`]: il crate che decide non deve dipendere da quello che
//! parla col mondo, o la decisione diventa improvabile senza una rete.
//!
//! # Perché niente `serde`
//!
//! Il dominio non serializza nulla da sé — `Cargo.toml` lo dice, e `serde` ci
//! sta solo fra le dipendenze di sviluppo per i vettori dorati. I tipi che
//! attraversano il confine con l'interfaccia sono altri, e vivono in
//! `aether-app`: tenerli separati è ciò che permette di cambiare la forma di un
//! messaggio IPC senza toccare la definizione di cos'è un brano.
//!
//! # Cosa NON c'è, e non è una dimenticanza
//!
//! Non c'è la popolarità, non c'è l'anteprima da trenta secondi, non ci sono i
//! mercati. Da febbraio 2026 Spotify ha tolto parecchi di quei campi anche a chi
//! ha una chiave, ma il motivo per cui non stanno qui è un altro: nessuno dei
//! tre finisce mai in un tag, in una decisione o davanti all'utente. Un campo
//! che si porta dietro senza usarlo è un campo che qualcuno prima o poi comincia
//! a usare per sbaglio.

/// Che cosa nomina un link di Spotify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpotifyKind {
    /// Un brano singolo.
    Track,
    /// Un album, con i suoi brani in ordine.
    Album,
    /// Una playlist.
    Playlist,
    /// Un artista, di cui si prende la discografia.
    Artist,
}

impl SpotifyKind {
    /// La parola che Spotify usa nei percorsi e negli URI.
    ///
    /// In inglese perché è il protocollo, non il nostro vocabolario: questa
    /// stringa va dentro un indirizzo che deve arrivare a Spotify così com'è.
    #[must_use]
    pub const fn path_word(self) -> &'static str {
        match self {
            Self::Track => "track",
            Self::Album => "album",
            Self::Playlist => "playlist",
            Self::Artist => "artist",
        }
    }

    /// Il nome stabile che attraversa l'IPC, in italiano come il resto della UI.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Track => "brano",
            Self::Album => "album",
            Self::Playlist => "playlist",
            Self::Artist => "artista",
        }
    }

    /// Riconosce la parola del protocollo.
    #[must_use]
    pub fn from_path_word(word: &str) -> Option<Self> {
        match word.to_ascii_lowercase().as_str() {
            "track" => Some(Self::Track),
            "album" => Some(Self::Album),
            "playlist" => Some(Self::Playlist),
            "artist" => Some(Self::Artist),
            _ => None,
        }
    }
}

/// Da quale livello del lettore keyless è arrivata una risposta.
///
/// Non è statistica: è la differenza fra un elenco completo e uno che potrebbe
/// non esserlo, e va portata fino all'utente. Vedi `spotify.tracklistTruncated`
/// nel catalogo degli errori.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpotifySource {
    /// L'API interna del lettore web. Completa e paginata.
    Pathfinder,
    /// La pagina del riquadro incorporabile. Può troncare gli elenchi lunghi.
    Embed,
    /// oEmbed: solo titolo e copertina, nessun brano.
    OEmbed,
}

impl SpotifySource {
    /// Il nome stabile che attraversa l'IPC.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Pathfinder => "pathfinder",
            Self::Embed => "embed",
            Self::OEmbed => "oembed",
        }
    }
}

/// Un brano, come Spotify lo descrive.
///
/// Quasi tutto è opzionale perché quasi tutto lo è davvero: la pagina
/// incorporabile non dà l'anno né il numero di traccia, oEmbed non dà nemmeno
/// l'artista. Il titolo no — un brano senza titolo non è un brano, ed è l'unica
/// cosa che tutti e tre i livelli restituiscono sempre.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpotifyTrack {
    /// Il titolo.
    pub title: String,
    /// Gli interpreti, uniti da virgole come li dà Spotify.
    pub artist: Option<String>,
    /// L'album di provenienza.
    pub album: Option<String>,
    /// L'interprete dell'album, che raggruppa le compilation sotto un nome solo.
    pub album_artist: Option<String>,
    /// Il numero del disco, per gli album multipli.
    pub disc_number: Option<u32>,
    /// Il numero di traccia.
    pub track_number: Option<u32>,
    /// L'anno, ricavato dalla data di pubblicazione.
    pub year: Option<i32>,
    /// La durata in millisecondi.
    pub duration_ms: Option<u64>,
    /// Il codice ISRC.
    ///
    /// È l'unica identità davvero affidabile fra servizi diversi, e da febbraio
    /// 2026 la REST pubblica non lo dà più (`external_ids` è stato rimosso).
    /// Pathfinder sì, ed è una delle ragioni per cui vale la pena passare di lì.
    pub isrc: Option<String>,
    /// L'identificativo Spotify del brano.
    pub spotify_track_id: Option<String>,
    /// L'identificativo Spotify dell'album.
    ///
    /// Vale la pena portarlo fino al database: [`crate::album`] lo usa già come
    /// aggancio autorevole per fondere le edizioni dello stesso album, ed è
    /// finora un innesto senza nessuno che ci scriva dentro.
    pub spotify_album_id: Option<String>,
    /// L'indirizzo della copertina.
    pub cover_url: Option<String>,
}

/// Un contenuto risolto: la playlist, l'album o il brano, con i suoi brani.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpotifyContent {
    /// Cosa è.
    pub kind: SpotifyKind,
    /// L'identificativo Spotify del contenitore.
    pub id: String,
    /// Il nome della playlist, dell'album o del brano.
    pub title: String,
    /// Chi lo firma: l'interprete dell'album, o chi possiede la playlist.
    pub author: Option<String>,
    /// La copertina del contenitore.
    pub cover_url: Option<String>,
    /// I brani, nell'ordine in cui stanno su Spotify.
    pub tracks: Vec<SpotifyTrack>,
    /// Quanti brani Spotify **dichiara** che ce ne siano.
    ///
    /// Separato da `tracks.len()` di proposito: la differenza fra i due è
    /// l'unico modo di accorgersi che un elenco è arrivato monco, ed è
    /// precisamente il guasto che non deve poter passare in silenzio. `None`
    /// quando il livello che ha risposto non dichiara un totale.
    pub declared_total: Option<u32>,
    /// Da dove è arrivato.
    pub source: SpotifySource,
}

impl SpotifyContent {
    /// Quanti brani mancano all'appello, se ne mancano.
    ///
    /// Restituisce `(letti, attesi)` — i due numeri che servono a dire «142 su
    /// 300» invece di «alcuni brani». `None` quando l'elenco è completo, quando
    /// Spotify non ha dichiarato un totale, o quando ne sono arrivati **più** di
    /// quanti dichiarati: succede sulle playlist modificate fra una pagina e
    /// l'altra, e non è un ammanco.
    #[must_use]
    pub fn truncation(&self) -> Option<(u32, u32)> {
        let attesi = self.declared_total?;
        let letti = u32::try_from(self.tracks.len()).unwrap_or(u32::MAX);
        (letti < attesi).then_some((letti, attesi))
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    fn contenuto(quanti: usize, dichiarati: Option<u32>) -> SpotifyContent {
        SpotifyContent {
            kind: SpotifyKind::Playlist,
            id: "37i9dQZF1DXcBWIGoYBM5M".to_owned(),
            title: "Prova".to_owned(),
            author: None,
            cover_url: None,
            tracks: vec![SpotifyTrack::default(); quanti],
            declared_total: dichiarati,
            source: SpotifySource::Embed,
        }
    }

    #[test]
    fn un_elenco_monco_si_vede() {
        assert_eq!(contenuto(142, Some(300)).truncation(), Some((142, 300)));
    }

    #[test]
    fn un_elenco_completo_non_si_lamenta() {
        assert_eq!(contenuto(300, Some(300)).truncation(), None);
        assert_eq!(
            contenuto(12, None).truncation(),
            None,
            "senza un totale dichiarato non si può dire che manchi qualcosa"
        );
        assert_eq!(
            contenuto(301, Some(300)).truncation(),
            None,
            "più del dichiarato non è un ammanco: la playlist è cambiata mentre la leggevamo"
        );
    }

    #[test]
    fn le_parole_del_protocollo_fanno_il_giro() {
        for genere in [
            SpotifyKind::Track,
            SpotifyKind::Album,
            SpotifyKind::Playlist,
            SpotifyKind::Artist,
        ] {
            assert_eq!(
                SpotifyKind::from_path_word(genere.path_word()),
                Some(genere)
            );
        }
        assert_eq!(
            SpotifyKind::from_path_word("TRACK"),
            Some(SpotifyKind::Track)
        );
        assert_eq!(
            SpotifyKind::from_path_word("episode"),
            None,
            "i podcast non sono musica in libreria"
        );
    }
}
