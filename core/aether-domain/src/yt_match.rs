//! Quale video di YouTube è il brano che Spotify nomina.
//!
//! Spotify dice *cosa* si vuole — titolo, interprete, album, durata — e non dà
//! nessun modo di sentirlo. YouTube ha l'audio ma non sa cosa gli si sta
//! chiedendo: risponde a una stringa di ricerca con otto video, e fra quegli
//! otto ci sono la canzone giusta, un live, un remix, una cover di un ragazzo
//! con la chitarra e un'ora di «lofi beats to study to».
//!
//! Questo modulo è la decisione in mezzo, e sta qui — nel dominio — perché è
//! precisamente il genere di decisione che [`crate`] descrive: si può sbagliare
//! in silenzio, e sbagliando mette in libreria la canzone di qualcun altro col
//! nome giusto nei tag. Provarla deve costare una chiamata di funzione, non una
//! rete e un binario esterno.
//!
//! # La durata è un cancello, il canale è la scelta
//!
//! L'ordine fra i due criteri è **la** decisione di progetto di questo file, e
//! va nell'una direzione e non nell'altra:
//!
//! - un canale ufficiale con la durata sbagliata è la **registrazione**
//!   sbagliata — il live, la versione allungata, l'album intero in un video solo;
//! - la durata giusta su un canale qualunque è almeno la canzone giusta.
//!
//! Quindi la durata elimina, e solo fra i sopravvissuti il canale sceglie. Il
//! vecchio albero (`legacy/.../download/spotifyMatch.ts`) si fermava al primo
//! criterio: sceglieva **solo** per vicinanza di durata, e la ricerca non
//! chiedeva nemmeno il nome del canale a yt-dlp. Con otto risultati di cui
//! quattro entro trenta secondi, quello equivaleva a prendere quel che capitava.
//!
//! # E perché due bande e non una tolleranza sola
//!
//! Perché «entro trenta secondi» e «entro un minuto» non vogliono dire la stessa
//! cosa. Trenta secondi è il rumore vero fra due codifiche dello stesso master;
//! un minuto è già un'altra cosa, e la si accetta solo quando non c'è nient'altro.
//! Se le bande fossero una sola, un canale «- Topic» con cinquanta secondi di
//! scarto batterebbe il video giusto sul canale sbagliato — che è di nuovo il
//! difetto da cui si parte.

use crate::spotify::SpotifyTrack;
use crate::spotify_plan::{primo_artista, senza_decorazioni};
use crate::text::{collapse_whitespace, fold_text};

/// Lo scarto di durata entro cui due registrazioni sono la stessa.
///
/// Trenta secondi come nel vecchio albero, e per la ragione già scritta in
/// [`crate::spotify_plan::TOLLERANZA_MS`]: sono i silenzi di coda e gli stacchi
/// che cambiano fra un rip e l'altro. Qui è più larga che nell'abbinamento in
/// libreria perché YouTube ci mette del suo — un secondo di nero in testa, una
/// coda che sfuma.
pub const BANDA_STRETTA_MS: u64 = 30_000;

/// Oltre questo scarto non è la stessa registrazione, e nessun canale lo salva.
pub const BANDA_LARGA_MS: u64 = 60_000;

/// Un risultato di ricerca di YouTube, ridotto a quel che serve per sceglierlo.
///
/// `canale` è il campo che il vecchio albero **buttava via** (`spotifyEngine.ts`
/// teneva `url`, `duration` e `title` e scartava il resto), ed è la ragione per
/// cui là una preferenza per i canali ufficiali non era scrivibile: non c'era
/// niente su cui esprimerla.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Candidato {
    /// L'indirizzo del video.
    pub url: String,
    /// Il titolo del video, come lo scrive chi lo ha caricato.
    pub titolo: String,
    /// Il nome del canale.
    pub canale: Option<String>,
    /// YouTube segna il canale come verificato.
    pub canale_verificato: bool,
    /// La durata dichiarata, in secondi. `None` quando la ricerca non la dà.
    pub durata_sec: Option<u32>,
}

/// Quanto un canale somiglia alla fonte ufficiale del brano.
///
/// L'ordine delle varianti **è** l'ordine di preferenza, e
/// [`Ufficialita::peso`] lo rende esplicito invece di lasciarlo dedurre da un
/// `derive(Ord)`: un giorno qualcuno riordinerà l'enum per leggibilità, e senza
/// il peso scritto a mano cambierebbe il comportamento senza cambiare nessuna
/// riga che sembri decidere qualcosa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ufficialita {
    /// `"<Artista> - Topic"`: i canali che YouTube genera da sé dalla
    /// distribuzione ufficiale. Audio senza video, senza intro del canale,
    /// senza applausi, con la durata dell'album. È il caso migliore che esista,
    /// e per questo sta sopra al video ufficiale vero e proprio.
    Topic,
    /// Un canale VEVO, cioè la distribuzione ufficiale dei video.
    Vevo,
    /// Il canale si chiama come l'interprete.
    NomeArtista,
    /// YouTube lo segna verificato, ma non è nessuno dei tre di sopra.
    Verificato,
    /// Tutto il resto: potrebbe essere chiunque.
    Ignoto,
}

impl Ufficialita {
    /// Quanto vale, dal più al meno ufficiale.
    #[must_use]
    pub const fn peso(self) -> u8 {
        match self {
            Self::Topic => 4,
            Self::Vevo => 3,
            Self::NomeArtista => 2,
            Self::Verificato => 1,
            Self::Ignoto => 0,
        }
    }
}

/// Le parole che rivelano un'altra registrazione.
///
/// Non sono «parole brutte»: sono parole che, se stanno nel titolo del video e
/// **non** in quello di Spotify, dicono che quel video non è il brano chiesto.
/// La condizione doppia è essenziale — chi importa una playlist di concerti
/// deve poter avere i suoi live, e un brano che si chiama davvero «Remix» non
/// deve diventare introvabile.
const RUMORE: &[&str] = &[
    "live",
    "cover",
    "remix",
    "mashup",
    "karaoke",
    "reaction",
    "reazione",
    "nightcore",
    "sped up",
    "slowed",
    "8d",
    "instrumental",
    "strumentale",
    "tutorial",
    "concert",
    "concerto",
];

/// La stringa di ricerca stretta: interprete e titolo come Spotify li scrive.
#[must_use]
pub fn query_stretta(brano: &SpotifyTrack) -> String {
    let artista = brano.artist.as_deref().unwrap_or("");
    collapse_whitespace(&format!("{artista} {}", brano.title))
}

/// La stringa di ricerca larga, per quando la stretta non trova niente.
///
/// Solo il primo interprete e il titolo senza decorazioni, riusando le due
/// funzioni con cui [`crate::spotify_plan`] ripulisce le stesse stringhe per
/// l'abbinamento in libreria. Riusarle e non riscriverle non è economia di
/// righe: è ciò che impedisce che «cercato su YouTube» e «cercato in libreria»
/// vogliano dire due cose diverse.
///
/// `None` quando non aggiunge niente — coincide con la stretta o è vuota — così
/// chi chiama non paga una seconda richiesta identica alla prima.
#[must_use]
pub fn query_larga(brano: &SpotifyTrack) -> Option<String> {
    let artista = brano.artist.as_deref().map(primo_artista).unwrap_or("");
    let titolo = senza_decorazioni(&brano.title);
    let larga = collapse_whitespace(&format!("{artista} {titolo}"));
    if larga.is_empty() || larga == query_stretta(brano) {
        return None;
    }
    Some(larga)
}

/// I caratteri che nessun filesystem che ci interessa accetta in un nome.
const VIETATI: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Quanti caratteri al massimo può essere lungo un segmento di percorso.
///
/// Centoventi e non 255: i segmenti sono tre — interprete, album, file — e su
/// Windows il percorso completo ha comunque un tetto. Tagliare qui è meglio che
/// scoprire a metà scaricamento che il file non si può creare.
const LUNGHEZZA_MASSIMA: usize = 120;

/// Un titolo reso adatto a essere un nome di cartella o di file.
///
/// Porto di `sanitizeSegment` (`legacy/.../download/spotifyMatch.ts:13`), e come
/// là **senza** classi Unicode: l'insieme dei caratteri vietati è scritto per
/// esteso, così è lo stesso su ogni piattaforma e si legge senza eseguirlo.
#[must_use]
pub fn segmento_sicuro(grezzo: &str) -> String {
    let sostituito: String = grezzo
        .chars()
        .map(|c| {
            if VIETATI.contains(&c) || c.is_control() {
                ' '
            } else {
                c
            }
        })
        .collect();
    let pulito: String = collapse_whitespace(&sostituito)
        .chars()
        .take(LUNGHEZZA_MASSIMA)
        .collect();
    // Il taglio può lasciare uno spazio in coda, e un nome di cartella che
    // finisce con uno spazio su Windows si crea ma non si riapre.
    let pulito = pulito.trim_end().to_owned();
    if pulito.is_empty() {
        "Senza titolo".to_owned()
    } else {
        pulito
    }
}

/// Dove va a finire un brano scaricato, relativamente alla cartella dei download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destinazione {
    /// La cartella dell'interprete.
    pub cartella_artista: String,
    /// La cartella dell'album, dentro quella dell'interprete.
    pub cartella_album: String,
    /// Il nome del file, senza estensione.
    pub nome_base: String,
}

impl Destinazione {
    /// Il percorso relativo con le barre in avanti, come lo vuole yt-dlp.
    #[must_use]
    pub fn relativo(&self) -> String {
        format!(
            "{}/{}/{}",
            self.cartella_artista, self.cartella_album, self.nome_base
        )
    }
}

/// Come si chiamerà il file, e in che cartelle starà.
///
/// Si usa l'interprete **dell'album** e non quello del brano: un album con un
/// ospite in tre pezzi resta una cartella sola invece di spargersi in quattro.
/// È lo stesso motivo per cui `album_artist` è la chiave con cui
/// `rebuild_aggregates` raggruppa un album in una sola uscita.
///
/// `numero` è la posizione da cui ricavare il prefisso quando Spotify non dà un
/// numero di traccia, e si conta da 1.
#[must_use]
pub fn destinazione(brano: &SpotifyTrack, numero: u32) -> Destinazione {
    let artista = brano
        .album_artist
        .as_deref()
        .or(brano.artist.as_deref())
        .unwrap_or("Sconosciuto");
    // Senza album è un singolo, e «Singoli» è una cartella vera in cui cercarlo,
    // mentre «Album sconosciuto» è un buco travestito da nome.
    let album = brano.album.as_deref().unwrap_or("Singoli");
    let posizione = brano.track_number.unwrap_or(numero);
    Destinazione {
        cartella_artista: segmento_sicuro(artista),
        cartella_album: segmento_sicuro(album),
        nome_base: segmento_sicuro(&format!("{posizione:02} - {}", brano.title)),
    }
}

/// Toglie da un nome di canale i suffissi che dicono «canale» e non «artista».
///
/// Porto di `cleanYoutubeArtist` (`legacy/.../download/youtubeClean.ts:55`).
///
/// Il confronto è ASCII e sull'originale, **non** sulla stringa piegata: la
/// piegatura scompone in NFD e cambia la lunghezza in byte, quindi una posizione
/// trovata là non vale qui. Con un nome accentato il taglio cadrebbe in mezzo a
/// un carattere — e `get` restituirebbe `None` invece di tagliare, cioè il
/// suffisso resterebbe attaccato senza che nessuno se ne accorga.
fn nome_canale_pulito(canale: &str) -> String {
    let mut s = canale.trim();
    for suffisso in [
        " - topic",
        " topic",
        "vevo",
        " official channel",
        " official",
    ] {
        let taglio = s.len().checked_sub(suffisso.len());
        if let Some(taglio) = taglio
            && s.get(taglio..)
                .is_some_and(|coda| coda.eq_ignore_ascii_case(suffisso))
            && let Some(tagliato) = s.get(..taglio)
        {
            s = tagliato.trim();
        }
    }
    s.to_owned()
}

/// Quanto questo canale somiglia alla fonte ufficiale del brano.
#[must_use]
pub fn ufficialita(canale: Option<&str>, verificato: bool, artista: Option<&str>) -> Ufficialita {
    let Some(canale) = canale.map(str::trim).filter(|c| !c.is_empty()) else {
        return if verificato {
            Ufficialita::Verificato
        } else {
            Ufficialita::Ignoto
        };
    };
    let piegato = fold_text(canale);

    if piegato.ends_with(" - topic") || piegato.ends_with(" topic") {
        return Ufficialita::Topic;
    }
    if piegato.ends_with("vevo") {
        return Ufficialita::Vevo;
    }

    // Il confronto col nome dell'interprete passa dalla piegatura di
    // `aether_domain::text`, la stessa che usa l'abbinamento in libreria: senza,
    // «Bjork» e «Björk» sarebbero due artisti diversi e il canale ufficiale
    // varrebbe come uno qualunque.
    if let Some(artista) = artista {
        let atteso = fold_text(primo_artista(artista));
        if !atteso.is_empty() && fold_text(&nome_canale_pulito(canale)) == atteso {
            return Ufficialita::NomeArtista;
        }
    }

    if verificato {
        Ufficialita::Verificato
    } else {
        Ufficialita::Ignoto
    }
}

/// La parola compare nel testo come parola intera.
///
/// Il confronto per sottostringa non basta e il caso che lo dimostra è in
/// classifica da cinquant'anni: «Stayin' Alive» contiene `live`, e con un
/// `contains` diventerebbe un concerto da scartare.
fn contiene_parola(piegato: &str, parola: &str) -> bool {
    piegato.match_indices(parola).any(|(inizio, _)| {
        let prima = piegato
            .get(..inizio)
            .and_then(|p| p.chars().next_back())
            .is_none_or(|c| !c.is_alphanumeric());
        let dopo = piegato
            .get(inizio.saturating_add(parola.len())..)
            .and_then(|p| p.chars().next())
            .is_none_or(|c| !c.is_alphanumeric());
        prima && dopo
    })
}

/// Il titolo del video promette una registrazione che non è quella chiesta.
fn rumoroso(titolo_video: &str, titolo_atteso_piegato: &str) -> bool {
    let piegato = fold_text(titolo_video);
    RUMORE.iter().any(|parola| {
        contiene_parola(&piegato, parola) && !contiene_parola(titolo_atteso_piegato, parola)
    })
}

/// La banda di durata in cui cade un candidato: 0 stretta, 1 larga.
///
/// Restituisce `None` quando la durata lo esclude del tutto.
fn banda(attesa_ms: Option<u64>, durata_sec: Option<u32>) -> Option<(u8, u64)> {
    let (Some(attesa), Some(sua_sec)) = (attesa_ms, durata_sec) else {
        // Una delle due durate non si sa: il candidato resta in gara, ma nella
        // banda larga e con lo scarto peggiore possibile, così perde ogni pari
        // merito contro uno di cui la durata si conosce e combacia.
        return Some((1, u64::MAX));
    };
    let scarto = attesa.abs_diff(u64::from(sua_sec).saturating_mul(1000));
    if scarto <= BANDA_STRETTA_MS {
        Some((0, scarto))
    } else if scarto <= BANDA_LARGA_MS {
        Some((1, scarto))
    } else {
        None
    }
}

/// Sceglie il video da scaricare, o nessuno.
///
/// L'ordine dei criteri, dal più forte al più debole:
///
/// 1. la banda di durata — stretta prima di larga, e fuori dalla larga si scarta;
/// 2. l'[`Ufficialita`] del canale;
/// 3. lo scarto di durata;
/// 4. l'ordine in cui YouTube li ha restituiti, che è la sua idea di pertinenza
///    e vale come ultimo spareggio invece di un `first()` arbitrario.
///
/// `None` quando non ne resta nessuno: è un esito legittimo e **terminale**, da
/// non confondere con «la ricerca è fallita». Chi chiama deve poter distinguere
/// il brano che su YouTube non c'è — inutile ritentarlo per sempre — da quello
/// per cui la richiesta è andata storta.
#[must_use]
pub fn scegli_candidato<'a>(
    candidati: &'a [Candidato],
    brano: &SpotifyTrack,
) -> Option<&'a Candidato> {
    let titolo_atteso = fold_text(&brano.title);
    let mut migliore: Option<(&Candidato, u8, u8, u64)> = None;

    for candidato in candidati {
        if candidato.url.trim().is_empty() {
            continue;
        }
        if rumoroso(&candidato.titolo, &titolo_atteso) {
            continue;
        }
        let Some((banda_sua, scarto)) = banda(brano.duration_ms, candidato.durata_sec) else {
            continue;
        };
        let peso = ufficialita(
            candidato.canale.as_deref(),
            candidato.canale_verificato,
            brano.artist.as_deref(),
        )
        .peso();

        let meglio = migliore.is_none_or(|(_, banda_ora, peso_ora, scarto_ora)| {
            (banda_sua, std::cmp::Reverse(peso), scarto)
                < (banda_ora, std::cmp::Reverse(peso_ora), scarto_ora)
        });
        if meglio {
            migliore = Some((candidato, banda_sua, peso, scarto));
        }
    }

    migliore.map(|(candidato, _, _, _)| candidato)
}

#[cfg(test)]
mod prove {
    use super::*;

    fn brano(artista: &str, titolo: &str, durata_ms: Option<u64>) -> SpotifyTrack {
        SpotifyTrack {
            title: titolo.to_owned(),
            artist: Some(artista.to_owned()),
            duration_ms: durata_ms,
            ..SpotifyTrack::default()
        }
    }

    fn cand(url: &str, titolo: &str, canale: &str, durata_sec: u32) -> Candidato {
        Candidato {
            url: url.to_owned(),
            titolo: titolo.to_owned(),
            canale: Some(canale.to_owned()),
            canale_verificato: false,
            durata_sec: Some(durata_sec),
        }
    }

    #[test]
    fn il_topic_vince_sul_video_ufficiale_a_parita_di_durata() {
        // Il caso normale: YouTube restituisce prima il videoclip, che ha
        // l'intro del regista e la durata leggermente diversa, e poi l'audio
        // del canale generato dalla distribuzione. Si vuole il secondo.
        let b = brano("Radiohead", "Karma Police", Some(264_000));
        let candidati = [
            cand(
                "https://y/clip",
                "Radiohead - Karma Police",
                "Radiohead",
                264,
            ),
            cand("https://y/topic", "Karma Police", "Radiohead - Topic", 264),
        ];
        assert_eq!(
            scegli_candidato(&candidati, &b).map(|c| c.url.as_str()),
            Some("https://y/topic")
        );
    }

    #[test]
    fn il_topic_con_la_durata_sbagliata_perde_contro_il_canale_ignoto() {
        // Quattro minuti di scarto: sul canale ufficiale c'è l'album intero, o
        // la versione estesa. Non è la registrazione chiesta, e nessun grado di
        // ufficialità la rende tale.
        let b = brano("Tizio", "Canzone", Some(200_000));
        let candidati = [
            cand("https://y/topic", "Canzone", "Tizio - Topic", 440),
            cand("https://y/tale", "Tizio - Canzone", "MusicaVaria", 201),
        ];
        assert_eq!(
            scegli_candidato(&candidati, &b).map(|c| c.url.as_str()),
            Some("https://y/tale")
        );
    }

    #[test]
    fn la_banda_stretta_batte_lufficialita() {
        // Cinquanta secondi di scarto stanno dentro il cancello ma fuori dalla
        // banda stretta: il canale ufficiale non basta a farli passare davanti
        // a un candidato che la durata conferma. È la ragione per cui le bande
        // sono due.
        let b = brano("Tizio", "Canzone", Some(200_000));
        let candidati = [
            cand("https://y/topic", "Canzone", "Tizio - Topic", 250),
            cand("https://y/tale", "Tizio - Canzone", "Chiunque", 202),
        ];
        assert_eq!(
            scegli_candidato(&candidati, &b).map(|c| c.url.as_str()),
            Some("https://y/tale")
        );
    }

    #[test]
    fn oltre_il_cancello_non_passa_nessuno() {
        let b = brano("Tizio", "Canzone", Some(200_000));
        let candidati = [cand("https://y/uno", "Canzone", "Tizio - Topic", 900)];
        assert_eq!(scegli_candidato(&candidati, &b), None);
    }

    #[test]
    fn senza_la_durata_di_spotify_decide_solo_il_canale() {
        let b = brano("Tizio", "Canzone", None);
        let candidati = [
            cand("https://y/tale", "Tizio - Canzone", "Chiunque", 202),
            cand("https://y/vevo", "Tizio - Canzone", "TizioVEVO", 700),
        ];
        assert_eq!(
            scegli_candidato(&candidati, &b).map(|c| c.url.as_str()),
            Some("https://y/vevo")
        );
    }

    #[test]
    fn un_live_non_passa_se_spotify_non_dice_live() {
        let b = brano("Tizio", "Canzone", Some(200_000));
        let candidati = [
            cand(
                "https://y/live",
                "Canzone (Live at Wembley)",
                "Tizio - Topic",
                201,
            ),
            cand("https://y/studio", "Canzone", "Chiunque", 200),
        ];
        assert_eq!(
            scegli_candidato(&candidati, &b).map(|c| c.url.as_str()),
            Some("https://y/studio")
        );
    }

    #[test]
    fn un_live_passa_se_spotify_dice_live() {
        // Chi importa una playlist di concerti deve poter avere i suoi live.
        let b = brano("Tizio", "Canzone - Live at Wembley", Some(200_000));
        let candidati = [cand(
            "https://y/live",
            "Canzone (Live at Wembley)",
            "Tizio - Topic",
            201,
        )];
        assert_eq!(
            scegli_candidato(&candidati, &b).map(|c| c.url.as_str()),
            Some("https://y/live")
        );
    }

    #[test]
    fn live_dentro_unaltra_parola_non_e_un_live() {
        // «Stayin' Alive» contiene `live`. Con un confronto per sottostringa
        // questo brano sarebbe introvabile su YouTube, per sempre.
        let b = brano("Bee Gees", "Stayin' Alive", Some(285_000));
        let candidati = [cand(
            "https://y/uno",
            "Bee Gees - Stayin' Alive",
            "Bee Gees - Topic",
            285,
        )];
        assert_eq!(
            scegli_candidato(&candidati, &b).map(|c| c.url.as_str()),
            Some("https://y/uno")
        );
    }

    #[test]
    fn un_candidato_senza_indirizzo_non_e_un_candidato() {
        let b = brano("Tizio", "Canzone", Some(200_000));
        let candidati = [cand("", "Canzone", "Tizio - Topic", 200)];
        assert_eq!(scegli_candidato(&candidati, &b), None);
    }

    #[test]
    fn a_parita_di_tutto_vince_lordine_di_youtube() {
        let b = brano("Tizio", "Canzone", Some(200_000));
        let candidati = [
            cand("https://y/primo", "Canzone", "Chiunque", 200),
            cand("https://y/secondo", "Canzone", "Altri", 200),
        ];
        assert_eq!(
            scegli_candidato(&candidati, &b).map(|c| c.url.as_str()),
            Some("https://y/primo")
        );
    }

    #[test]
    fn lufficialita_riconosce_le_quattro_forme() {
        assert_eq!(
            ufficialita(Some("Radiohead - Topic"), false, Some("Radiohead")),
            Ufficialita::Topic
        );
        assert_eq!(
            ufficialita(Some("DuaLipaVEVO"), false, Some("Dua Lipa")),
            Ufficialita::Vevo
        );
        assert_eq!(
            ufficialita(Some("Radiohead"), false, Some("Radiohead")),
            Ufficialita::NomeArtista
        );
        assert_eq!(
            ufficialita(Some("Radiohead Official"), false, Some("Radiohead")),
            Ufficialita::NomeArtista,
            "il suffisso «Official» non fa di un canale un altro artista"
        );
        assert_eq!(
            ufficialita(Some("Qualcun altro"), true, Some("Radiohead")),
            Ufficialita::Verificato
        );
        assert_eq!(
            ufficialita(Some("Qualcun altro"), false, Some("Radiohead")),
            Ufficialita::Ignoto
        );
        assert_eq!(
            ufficialita(None, false, Some("Radiohead")),
            Ufficialita::Ignoto
        );
    }

    #[test]
    fn il_canale_si_confronta_piegato() {
        // Senza la piegatura, il canale ufficiale di Björk varrebbe come uno
        // qualunque e vincerebbe il primo video che passa.
        assert_eq!(
            ufficialita(Some("Bjork"), false, Some("Björk")),
            Ufficialita::NomeArtista
        );
    }

    #[test]
    fn il_canale_dellartista_principale_conta_anche_con_gli_ospiti() {
        assert_eq!(
            ufficialita(Some("Gorillaz"), false, Some("Gorillaz, De La Soul")),
            Ufficialita::NomeArtista
        );
    }

    #[test]
    fn le_due_query_dicono_due_cose_diverse() {
        let b = SpotifyTrack {
            title: "Feel Good Inc. (feat. De La Soul)".to_owned(),
            artist: Some("Gorillaz, De La Soul".to_owned()),
            ..SpotifyTrack::default()
        };
        assert_eq!(
            query_stretta(&b),
            "Gorillaz, De La Soul Feel Good Inc. (feat. De La Soul)"
        );
        assert_eq!(query_larga(&b).as_deref(), Some("Gorillaz Feel Good Inc."));
    }

    #[test]
    fn la_query_larga_sparisce_quando_non_aggiunge_niente() {
        let b = brano("Queen", "Bohemian Rhapsody", None);
        assert_eq!(
            query_larga(&b),
            None,
            "una seconda ricerca identica alla prima è una richiesta sprecata"
        );
    }

    #[test]
    fn i_segmenti_sono_sicuri_su_ogni_filesystem() {
        assert_eq!(segmento_sicuro("AC/DC"), "AC DC");
        assert_eq!(segmento_sicuro("Chi? Cosa: Quando*"), "Chi Cosa Quando");
        assert_eq!(segmento_sicuro("   "), "Senza titolo");
        assert_eq!(segmento_sicuro(""), "Senza titolo");
        assert_eq!(
            segmento_sicuro("a\u{0000}b"),
            "a b",
            "i caratteri di controllo non arrivano al filesystem"
        );
        let lungo = segmento_sicuro(&"à".repeat(200));
        assert_eq!(
            lungo.chars().count(),
            LUNGHEZZA_MASSIMA,
            "il taglio conta caratteri, non byte"
        );
    }

    #[test]
    fn un_nome_non_finisce_mai_con_uno_spazio() {
        // Su Windows una cartella che finisce con uno spazio si crea e poi non
        // si riapre: il taglio a 120 non deve poterla produrre.
        let nome = segmento_sicuro(&format!("{} coda", "a".repeat(118)));
        assert!(!nome.ends_with(' '), "«{nome}»");
    }

    #[test]
    fn la_destinazione_usa_linterprete_dellalbum() {
        // Un album con un ospite in un pezzo resta una cartella sola.
        let b = SpotifyTrack {
            title: "Il Pezzo".to_owned(),
            artist: Some("Tizio, Ospite".to_owned()),
            album_artist: Some("Tizio".to_owned()),
            album: Some("L'Album".to_owned()),
            track_number: Some(3),
            ..SpotifyTrack::default()
        };
        assert_eq!(
            destinazione(&b, 99).relativo(),
            "Tizio/L'Album/03 - Il Pezzo"
        );
    }

    #[test]
    fn senza_album_e_un_singolo() {
        let b = brano("Tizio", "Il Pezzo", None);
        let d = destinazione(&b, 1);
        assert_eq!(d.cartella_album, "Singoli");
        assert_eq!(d.nome_base, "01 - Il Pezzo");
    }
}
