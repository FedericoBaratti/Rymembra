//! Android Auto: la libreria vista dallo schermo dell'auto, e i gesti che ne
//! partono.
//!
//! # Chi fa cosa
//!
//! L'auto sfoglia la libreria chiedendola al servizio della sessione, in
//! Kotlin. Kotlin costruisce da sé il primo livello — le quattro schede:
//! recenti, playlist, album, artisti — con i nomi tradotti dalle risorse di
//! Android, perché l'auto lo chiede appena si collega e a quel punto il nucleo
//! può non essere ancora partito. Da lì in giù chiede a Rust, e Rust risponde
//! con le [`Voce`] di questo modulo. Quando dallo schermo dell'auto si sceglie
//! qualcosa da suonare, arriva un gesto ([`GestoAuto`]) sulla stessa strada dei
//! tasti delle cuffie, e lo si esegue con i comandi che usa la finestra.
//!
//! # Gli identificativi
//!
//! Ogni voce ha un identificativo di testo ([`Nodo::testo`]) che torna indietro
//! tale e quale quando l'auto chiede i figli o chiede di suonarla. Un brano si
//! porta dietro **da dove** lo si è scelto — `brano:42@album:…` — perché
//! scegliere la terza traccia di un album vuol dire sentire l'album dalla
//! terza traccia in poi, non un brano solo.
//!
//! # Perché i tetti
//!
//! Le voci viaggiano verso l'auto dentro una transazione Binder, che sta sotto
//! il megabyte: un elenco di cinquemila album non arriva, e il sistema lo dice
//! con un'eccezione nel processo dell'auto, non nel nostro. Per questo le
//! pagine hanno un tetto ([`TETTO_PAGINA`]), e gli elenchi lunghi — album e
//! artisti — diventano lettere: in macchina si scorre poco, e si salta alla
//! lettera.
//!
//! # Fuori da Android
//!
//! Il modulo si compila anche nelle prove sul computer, dove gira tutto quel
//! che non vuole un telefono: gli identificativi, le pagine, le lettere, la
//! scelta di che cosa suonare per una richiesta a voce.

use rusqlite::Connection;
use rymembra_app::covers::CoverStore;
use rymembra_app::library::{
    AlbumSummary, ArtistSummary, TrackSummary, album_tracks, albums_by_artist,
    ascoltati_di_recente, list_albums, list_artists, list_liked, read_summary,
    recently_added_albums, search,
};
use rymembra_app::playlists::{self, PlaylistSummary};
use rymembra_app::specchio::Origine;
use rymembra_domain::keys::sort_name;
use rymembra_domain::{AppError, ErrorCode};
use serde::Serialize;
#[cfg(mobile)]
use tauri::Manager as _;

#[cfg(mobile)]
use crate::errore::{Esito, errore};
#[cfg(mobile)]
use crate::stato::{Stato, con_libreria};

/// Quante voci al massimo in una pagina, qualunque cosa chieda l'auto.
///
/// Un controllore Media1 — l'auto lo è — non chiede pagine: chiede «tutto», e
/// Media3 traduce «tutto» in una pagina grande quanto un `int`. Duecento voci
/// sono qualche decina di kilobyte: lontane dal tetto di Binder, e più di
/// quante se ne scorrano guidando.
pub const TETTO_PAGINA: usize = 200;

/// Oltre quanti album, o artisti, l'elenco diventa un elenco di lettere.
const SOGLIA_LETTERE: usize = 150;

/// Quanti ascoltati di recente nella prima scheda.
const RECENTI: i64 = 30;

/// Quanti album aggiunti di recente nella prima scheda.
const AGGIUNTI: i64 = 12;

/// Quanti brani al massimo in una coda che parte dall'auto: i preferiti, una
/// ricerca, un artista. Gli stessi cinquemila delle playlist intelligenti.
const TETTO_CODA: i64 = 5_000;

/// Quanti album di un artista si mettono in coda quando lo si suona intero.
const TETTO_ALBUM_ARTISTA: i64 = 200;

/// L'identificativo che Kotlin usa per «quel che sta già suonando», quando
/// l'auto chiede di riprendere: non è un nodo, e non fa partire niente.
pub const ATTUALE: &str = "attuale";

/// Un posto dell'albero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Nodo {
    /// La prima scheda: ascoltati e aggiunti di recente.
    Recenti,
    /// Tutti gli album.
    Album,
    /// Gli album sotto una lettera, quando sono troppi per un elenco solo.
    AlbumLettera(String),
    /// Tutti gli artisti.
    Artisti,
    /// Gli artisti sotto una lettera.
    ArtistiLettera(String),
    /// Le playlist, con i preferiti in cima.
    Playlist,
    /// I preferiti.
    Preferiti,
    /// Un album, per chiave.
    AlbumDi(String),
    /// Un artista, per nome.
    ArtistaDi(String),
    /// Una playlist, per riga.
    PlaylistDi(i64),
    /// I risultati di una ricerca.
    Cerca(String),
    /// «Suona tutto»: la prima voce dentro un album, un artista, una playlist.
    ///
    /// Una voce a sé e non l'album stesso reso suonabile, perché l'auto non
    /// tratta tutte allo stesso modo una voce che si apre **e** suona: c'è chi la
    /// apre e chi la suona al primo tocco, e allora dall'auto non si potrebbe più
    /// scegliere la terza traccia. Così una voce si apre sempre, e dentro, in
    /// cima, c'è il pulsante per sentirla intera.
    Tutto(Box<Nodo>),
    /// Un brano, e la lista da cui lo si è scelto.
    Brano {
        /// La riga del brano.
        id: i64,
        /// Dove stava: è la coda che parte se lo si suona.
        dentro: Box<Nodo>,
    },
}

impl Nodo {
    /// Il nodo di un identificativo, se lo è.
    ///
    /// Un brano dentro un brano non lo è: un identificativo così non lo scrive
    /// [`Nodo::testo`], quindi arriva da qualcun altro, e non lo si indovina.
    #[must_use]
    pub fn da_testo(testo: &str) -> Option<Self> {
        match testo {
            "recenti" => return Some(Self::Recenti),
            "album" => return Some(Self::Album),
            "artisti" => return Some(Self::Artisti),
            "playlist" => return Some(Self::Playlist),
            "preferiti" => return Some(Self::Preferiti),
            _ => {}
        }
        let (tipo, resto) = testo.split_once(':')?;
        match tipo {
            "lettera-album" => Some(Self::AlbumLettera(resto.to_owned())),
            "lettera-artisti" => Some(Self::ArtistiLettera(resto.to_owned())),
            "album" => Some(Self::AlbumDi(resto.to_owned())),
            "artista" => Some(Self::ArtistaDi(resto.to_owned())),
            "playlist" => resto.parse().ok().map(Self::PlaylistDi),
            "cerca" => Some(Self::Cerca(resto.to_owned())),
            "tutto" => {
                // Il resto è il nodo intero, `:` compresi: `tutto:album:…`.
                let dentro = Self::da_testo(resto)?;
                if matches!(dentro, Self::Brano { .. } | Self::Tutto(_)) {
                    return None;
                }
                Some(Self::Tutto(Box::new(dentro)))
            }
            "brano" => {
                // Il primo `@` e non l'ultimo: l'id è un numero, e dopo viene
                // un nodo che può contenere qualunque carattere — una chiave
                // d'album, il nome di un artista.
                let (id, dentro) = resto.split_once('@')?;
                let dentro = Self::da_testo(dentro)?;
                if matches!(dentro, Self::Brano { .. } | Self::Tutto(_)) {
                    return None;
                }
                Some(Self::Brano {
                    id: id.parse().ok()?,
                    dentro: Box::new(dentro),
                })
            }
            _ => None,
        }
    }

    /// L'identificativo di testo, quello che l'auto rimanda indietro.
    #[must_use]
    pub fn testo(&self) -> String {
        match self {
            Self::Recenti => "recenti".to_owned(),
            Self::Album => "album".to_owned(),
            Self::Artisti => "artisti".to_owned(),
            Self::Playlist => "playlist".to_owned(),
            Self::Preferiti => "preferiti".to_owned(),
            Self::AlbumLettera(lettera) => format!("lettera-album:{lettera}"),
            Self::ArtistiLettera(lettera) => format!("lettera-artisti:{lettera}"),
            Self::AlbumDi(chiave) => format!("album:{chiave}"),
            Self::ArtistaDi(nome) => format!("artista:{nome}"),
            Self::PlaylistDi(id) => format!("playlist:{id}"),
            Self::Cerca(testo) => format!("cerca:{testo}"),
            Self::Brano { id, dentro } => format!("brano:{id}@{}", dentro.testo()),
            Self::Tutto(dentro) => format!("tutto:{}", dentro.testo()),
        }
    }
}

/// Che cosa è una voce: decide come Kotlin la mostra, se si apre, se suona.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Tipo {
    /// Un brano: suona, non si apre.
    Brano,
    /// Un album: si apre.
    Album,
    /// Un artista: si apre sui suoi album.
    Artista,
    /// Una playlist: si apre.
    Playlist,
    /// I preferiti: come una playlist, col nome che mette Kotlin.
    Preferiti,
    /// «Suona tutto»: suona, non si apre. Il nome lo mette Kotlin.
    Tutto,
    /// Una lettera dell'alfabeto: si apre e basta.
    Lettera,
}

/// Una voce dell'albero, come la riceve Kotlin.
///
/// Niente frasi fatte qui: «12 brani» o «Ascoltati di recente» li scrive
/// Kotlin, con le risorse di Android e nella lingua del telefono, che è quella
/// dell'auto. Da qui partono i dati: [`Voce::quanti`], [`Voce::gruppo`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Voce {
    /// L'identificativo: vedi [`Nodo::testo`].
    pub id: String,
    /// Che cosa è.
    pub tipo: Tipo,
    /// Il titolo; vuoto per i preferiti, che il nome lo prendono da Kotlin.
    pub titolo: String,
    /// L'artista, per un brano o un album.
    pub artista: Option<String>,
    /// L'album, per un brano.
    pub album: Option<String>,
    /// Quanti brani (playlist), o quanti album (artista, lettera).
    pub quanti: Option<i64>,
    /// L'anno, per un album.
    pub anno: Option<i64>,
    /// La copertina, come percorso dentro la cartella delle copertine.
    pub copertina: Option<String>,
    /// Quanto dura, per un brano.
    pub durata_ms: Option<u64>,
    /// Sotto quale titoletto sta, dentro la sua scheda: `ascoltati` o
    /// `aggiunti`. Lo traduce Kotlin.
    pub gruppo: Option<&'static str>,
}

/// Una richiesta a voce, come l'ha capita l'assistente.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RicercaVocale {
    /// La frase intera.
    pub testo: String,
    /// L'artista, se l'assistente l'ha separato.
    pub artista: Option<String>,
    /// L'album.
    pub album: Option<String>,
    /// Il titolo di un brano.
    pub titolo: Option<String>,
}

impl RicercaVocale {
    /// «Metti un po' di musica»: niente da cercare.
    #[must_use]
    pub fn vuota(&self) -> bool {
        [
            Some(self.testo.as_str()),
            self.artista.as_deref(),
            self.album.as_deref(),
            self.titolo.as_deref(),
        ]
        .into_iter()
        .flatten()
        .all(|parte| parte.trim().is_empty())
    }
}

/// Un gesto partito dallo schermo dell'auto, o dall'assistente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GestoAuto {
    /// Suona una voce dell'albero.
    Suona(String),
    /// Suona quel che una ricerca a voce ha chiesto.
    Cerca(RicercaVocale),
    /// Salta a una posizione della coda.
    Coda(usize),
    /// Accende o spegne lo shuffle.
    Mescola,
    /// Il modo di ripetizione dopo.
    Ripeti,
    /// Mette (`true`) o toglie un brano dai preferiti.
    Piace {
        /// La riga del brano.
        id: i64,
        /// Il valore nuovo.
        valore: bool,
    },
}

// ── le voci ─────────────────────────────────────────────────────────────────

/// La copertina di un'impronta, come percorso dentro lo store: la media se c'è
/// già, la piena altrimenti, niente se il file non c'è.
///
/// La media (320 pixel) è quella giusta per una griglia sullo schermo
/// dell'auto; la piena arriva a 640, e va bene lo stesso. Che il file esista lo
/// si guarda qui e non lo si lascia scoprire all'auto, che di un indirizzo che
/// non si apre disegna un riquadro grigio senza dire perché.
#[must_use]
pub fn copertina(store: &CoverStore, impronta: Option<&str>) -> Option<String> {
    let impronta = impronta?;
    [store.percorso_media(impronta), store.path_for(impronta)]
        .into_iter()
        .find(|percorso| percorso.is_file())
        .and_then(|percorso| relativa(store, &percorso))
}

/// Come [`copertina`], ma la piena prima della media: per il brano che suona,
/// che l'auto e la schermata di blocco mostrano grande.
#[must_use]
pub fn copertina_grande(store: &CoverStore, impronta: Option<&str>) -> Option<String> {
    let impronta = impronta?;
    [store.path_for(impronta), store.percorso_media(impronta)]
        .into_iter()
        .find(|percorso| percorso.is_file())
        .and_then(|percorso| relativa(store, &percorso))
}

/// Il percorso dentro lo store, con le barre di Android anche sul computer.
fn relativa(store: &CoverStore, percorso: &std::path::Path) -> Option<String> {
    let dentro = percorso.strip_prefix(store.radice()).ok()?;
    let parti: Option<Vec<&str>> = dentro
        .components()
        .map(|parte| parte.as_os_str().to_str())
        .collect();
    Some(parti?.join("/"))
}

fn voce_brano(brano: &TrackSummary, dentro: &Nodo, store: &CoverStore) -> Voce {
    Voce {
        id: Nodo::Brano {
            id: brano.id,
            dentro: Box::new(dentro.clone()),
        }
        .testo(),
        tipo: Tipo::Brano,
        titolo: brano.title.clone(),
        artista: Some(brano.artist.clone()),
        album: Some(brano.album.clone()),
        quanti: None,
        anno: brano.year,
        copertina: copertina(store, brano.cover_art_hash.as_deref()),
        durata_ms: u64::try_from(brano.duration_ms).ok().filter(|ms| *ms > 0),
        gruppo: None,
    }
}

fn voce_album(album: &AlbumSummary, store: &CoverStore) -> Voce {
    Voce {
        id: Nodo::AlbumDi(album.album_key.clone()).testo(),
        tipo: Tipo::Album,
        titolo: album.title.clone(),
        artista: Some(album.artist.clone()),
        album: None,
        quanti: Some(album.total_tracks),
        anno: album.year,
        copertina: copertina(store, album.cover_art_hash.as_deref()),
        durata_ms: None,
        gruppo: None,
    }
}

fn voce_artista(artista: &ArtistSummary, store: &CoverStore) -> Voce {
    Voce {
        id: Nodo::ArtistaDi(artista.name.clone()).testo(),
        tipo: Tipo::Artista,
        titolo: artista.name.clone(),
        artista: None,
        album: None,
        quanti: Some(artista.albums),
        anno: None,
        // La più recente delle quattro: è quella che chi ascolta riconosce.
        copertina: artista
            .covers
            .iter()
            .rev()
            .find_map(|impronta| copertina(store, Some(impronta))),
        durata_ms: None,
        gruppo: None,
    }
}

fn voce_playlist(playlist: &PlaylistSummary) -> Voce {
    Voce {
        id: Nodo::PlaylistDi(playlist.id).testo(),
        tipo: Tipo::Playlist,
        titolo: playlist.name.clone(),
        artista: None,
        album: None,
        quanti: Some(playlist.tracks),
        anno: None,
        copertina: None,
        durata_ms: None,
        gruppo: None,
    }
}

fn voce_preferiti() -> Voce {
    Voce {
        id: Nodo::Preferiti.testo(),
        tipo: Tipo::Preferiti,
        titolo: String::new(),
        artista: None,
        album: None,
        quanti: None,
        anno: None,
        copertina: None,
        durata_ms: None,
        gruppo: None,
    }
}

fn voce_lettera(nodo: Nodo, lettera: &str, quanti: usize) -> Voce {
    Voce {
        id: nodo.testo(),
        tipo: Tipo::Lettera,
        titolo: lettera.to_owned(),
        artista: None,
        album: None,
        quanti: i64::try_from(quanti).ok(),
        anno: None,
        copertina: None,
        durata_ms: None,
        gruppo: None,
    }
}

/// «Suona tutto» per `dentro`, con quanti brani, se si sanno.
fn voce_tutto(dentro: &Nodo, quanti: Option<usize>) -> Voce {
    Voce {
        id: Nodo::Tutto(Box::new(dentro.clone())).testo(),
        tipo: Tipo::Tutto,
        titolo: String::new(),
        artista: None,
        album: None,
        quanti: quanti.and_then(|quanti| i64::try_from(quanti).ok()),
        anno: None,
        copertina: None,
        durata_ms: None,
        gruppo: None,
    }
}

/// Le voci di una pagina, con «Suona tutto» in cima alla prima.
///
/// Solo alla prima: le pagine dopo continuano l'elenco, e un secondo «Suona
/// tutto» a metà lista sembrerebbe un'altra cosa.
fn con_tutto(dentro: &Nodo, numero: u32, quanti: Option<usize>, voci: Vec<Voce>) -> Vec<Voce> {
    if numero > 0 || voci.is_empty() {
        return voci;
    }
    std::iter::once(voce_tutto(dentro, quanti))
        .chain(voci)
        .collect()
}

// ── le pagine e le lettere ──────────────────────────────────────────────────

/// La pagina `pagina` di `quante` voci, con il tetto di [`TETTO_PAGINA`].
#[must_use]
pub fn pagina<T>(tutte: Vec<T>, pagina: u32, quante: u32) -> Vec<T> {
    let quante = usize::try_from(quante)
        .unwrap_or(usize::MAX)
        .clamp(1, TETTO_PAGINA);
    let salta = usize::try_from(pagina)
        .unwrap_or(usize::MAX)
        .saturating_mul(quante);
    tutte.into_iter().skip(salta).take(quante).collect()
}

/// Offset e limite di SQL per la stessa pagina di [`pagina`].
fn finestra(pagina: u32, quante: u32) -> (i64, i64) {
    let quante = i64::from(quante).clamp(1, i64::try_from(TETTO_PAGINA).unwrap_or(i64::MAX));
    (i64::from(pagina).saturating_mul(quante), quante)
}

/// La lettera sotto cui sta un nome: la prima della sua chiave d'ordinamento,
/// maiuscola, o `#` per quel che non comincia con una lettera.
///
/// La chiave è [`sort_name`]: «The Cure» sta sotto C, come nella finestra.
#[must_use]
pub fn iniziale(nome: &str) -> String {
    let chiave = sort_name(nome);
    match chiave.chars().next() {
        Some(prima) if prima.is_alphabetic() => prima.to_uppercase().collect(),
        _ => "#".to_owned(),
    }
}

/// Le lettere di un elenco, nell'ordine in cui compaiono, con quanti ce ne
/// sono sotto ciascuna. L'ordine è quello dell'elenco, che è già alfabetico.
fn lettere<T>(elenco: &[T], nome: impl Fn(&T) -> &str) -> Vec<(String, usize)> {
    let mut lettere: Vec<(String, usize)> = Vec::new();
    for voce in elenco {
        let lettera = iniziale(nome(voce));
        match lettere.iter_mut().find(|(gia, _)| *gia == lettera) {
            Some((_, quanti)) => *quanti = quanti.saturating_add(1),
            None => lettere.push((lettera, 1)),
        }
    }
    lettere
}

/// Le righe di una playlist che l'auto può suonare adesso.
///
/// Una playlist nomina le righe una per una, e una riga di un dispositivo che
/// non risponde — il PC spento — non parte: nell'auto sarebbe un brano che
/// si salta al volante. Lo stesso per una riga che il proprietario non manda
/// più e che resta in libreria per la grazia, nascosta: la sua voce sta ancora
/// nella playlist, ma non si suona. Con le altre origini la lista resta com'è.
fn raggiungibili(
    connessione: &Connection,
    origine: &Origine,
    brani: Vec<TrackSummary>,
) -> Result<Vec<TrackSummary>, AppError> {
    if !matches!(origine, Origine::Raggiungibili(_)) {
        return Ok(brani);
    }
    let ids: Vec<i64> = brani.iter().map(|brano| brano.id).collect();
    let in_grazia = rymembra_app::specchio::archivio::in_grazia(connessione, &ids)?;
    Ok(brani
        .into_iter()
        .filter(|brano| !in_grazia.contains(&brano.id) && origine.raggiunge(brano.path.as_deref()))
        .collect())
}

/// Gli artisti ordinati come nella finestra: per chiave d'ordinamento.
fn artisti_in_ordine(
    connessione: &Connection,
    origine: &Origine,
) -> Result<Vec<ArtistSummary>, AppError> {
    list_artists(connessione, origine)
}

/// Tutti gli album, nell'ordine della griglia della finestra.
fn tutti_gli_album(
    connessione: &Connection,
    origine: &Origine,
) -> Result<Vec<AlbumSummary>, AppError> {
    list_albums(connessione, origine, 0, i64::MAX)
}

// ── i figli ─────────────────────────────────────────────────────────────────

/// I figli di un nodo, una pagina alla volta.
///
/// # Errori
///
/// Quelli della libreria; `library.trackNotFound` per un nodo che non ha
/// figli — un brano — o una playlist che non c'è più.
pub fn figli(
    connessione: &Connection,
    origine: &Origine,
    store: &CoverStore,
    genitore: &Nodo,
    numero: u32,
    quante: u32,
) -> Result<Vec<Voce>, AppError> {
    let (offset, limite) = finestra(numero, quante);
    Ok(match genitore {
        Nodo::Recenti => {
            if numero > 0 {
                return Ok(Vec::new());
            }
            let ascoltati = ascoltati_di_recente(connessione, origine, RECENTI)?;
            let aggiunti = recently_added_albums(connessione, origine, AGGIUNTI)?;
            ascoltati
                .iter()
                .map(|brano| Voce {
                    gruppo: Some("ascoltati"),
                    ..voce_brano(brano, genitore, store)
                })
                .chain(aggiunti.iter().map(|album| Voce {
                    gruppo: Some("aggiunti"),
                    ..voce_album(album, store)
                }))
                .collect()
        }
        Nodo::Album => {
            let tutti = tutti_gli_album(connessione, origine)?;
            if tutti.len() > SOGLIA_LETTERE {
                let lettere = lettere(&tutti, |album| album.artist.as_str());
                pagina(lettere, numero, quante)
                    .into_iter()
                    .map(|(lettera, quanti)| {
                        voce_lettera(Nodo::AlbumLettera(lettera.clone()), &lettera, quanti)
                    })
                    .collect()
            } else {
                pagina(tutti, numero, quante)
                    .iter()
                    .map(|album| voce_album(album, store))
                    .collect()
            }
        }
        Nodo::AlbumLettera(lettera) => {
            let sotto: Vec<AlbumSummary> = tutti_gli_album(connessione, origine)?
                .into_iter()
                .filter(|album| iniziale(&album.artist) == *lettera)
                .collect();
            pagina(sotto, numero, quante)
                .iter()
                .map(|album| voce_album(album, store))
                .collect()
        }
        Nodo::Artisti => {
            let tutti = artisti_in_ordine(connessione, origine)?;
            if tutti.len() > SOGLIA_LETTERE {
                let lettere = lettere(&tutti, |artista| artista.name.as_str());
                pagina(lettere, numero, quante)
                    .into_iter()
                    .map(|(lettera, quanti)| {
                        voce_lettera(Nodo::ArtistiLettera(lettera.clone()), &lettera, quanti)
                    })
                    .collect()
            } else {
                pagina(tutti, numero, quante)
                    .iter()
                    .map(|artista| voce_artista(artista, store))
                    .collect()
            }
        }
        Nodo::ArtistiLettera(lettera) => {
            let sotto: Vec<ArtistSummary> = artisti_in_ordine(connessione, origine)?
                .into_iter()
                .filter(|artista| iniziale(&artista.name) == *lettera)
                .collect();
            pagina(sotto, numero, quante)
                .iter()
                .map(|artista| voce_artista(artista, store))
                .collect()
        }
        Nodo::Playlist => {
            let mut tutte = vec![voce_preferiti()];
            tutte.extend(playlists::list(connessione)?.iter().map(voce_playlist));
            pagina(tutte, numero, quante)
        }
        Nodo::Preferiti => {
            let voci = list_liked(connessione, origine, offset, limite)?
                .iter()
                .map(|brano| voce_brano(brano, genitore, store))
                .collect();
            con_tutto(genitore, numero, None, voci)
        }
        Nodo::AlbumDi(chiave) => {
            let brani = album_tracks(connessione, origine, chiave)?;
            let quanti = brani.len();
            let voci = pagina(brani, numero, quante)
                .iter()
                .map(|brano| voce_brano(brano, genitore, store))
                .collect();
            con_tutto(genitore, numero, Some(quanti), voci)
        }
        Nodo::ArtistaDi(nome) => {
            let voci = albums_by_artist(connessione, origine, nome, offset, limite)?
                .iter()
                .map(|album| voce_album(album, store))
                .collect();
            con_tutto(genitore, numero, None, voci)
        }
        Nodo::PlaylistDi(id) => {
            let brani = raggiungibili(connessione, origine, playlists::tracks(connessione, *id)?)?;
            let quanti = brani.len();
            let voci = pagina(brani, numero, quante)
                .iter()
                .map(|brano| voce_brano(brano, genitore, store))
                .collect();
            con_tutto(genitore, numero, Some(quanti), voci)
        }
        Nodo::Cerca(testo) => search(connessione, origine, testo, offset, limite)?
            .iter()
            .map(|brano| voce_brano(brano, genitore, store))
            .collect(),
        Nodo::Tutto(_) => return Err(niente("tutto: non ha figli")),
        Nodo::Brano { id, .. } => {
            return Err(AppError::new(ErrorCode::LibraryTrackNotFound {
                track_id: Some(*id),
            })
            .with_cause("un brano non ha figli"));
        }
    })
}

/// Una voce sola: quel che l'auto chiede quando riapre un punto dell'albero
/// che si era ricordata. `None` se non c'è più.
///
/// # Errori
///
/// Quelli della libreria.
pub fn voce(
    connessione: &Connection,
    origine: &Origine,
    store: &CoverStore,
    nodo: &Nodo,
) -> Result<Option<Voce>, AppError> {
    Ok(match nodo {
        Nodo::Brano { id, dentro } => {
            read_summary(connessione, *id)?.map(|brano| voce_brano(&brano, dentro, store))
        }
        Nodo::AlbumDi(chiave) => album_tracks(connessione, origine, chiave)?
            .first()
            .map(|brano| Voce {
                id: nodo.testo(),
                tipo: Tipo::Album,
                titolo: brano.album.clone(),
                artista: Some(brano.artist.clone()),
                album: None,
                quanti: None,
                anno: brano.year,
                copertina: copertina(store, brano.cover_art_hash.as_deref()),
                durata_ms: None,
                gruppo: None,
            }),
        Nodo::ArtistaDi(nome) => artisti_in_ordine(connessione, origine)?
            .iter()
            .find(|artista| artista.name == *nome)
            .map(|artista| voce_artista(artista, store)),
        Nodo::PlaylistDi(id) => playlists::list(connessione)?
            .iter()
            .find(|playlist| playlist.id == *id)
            .map(voce_playlist),
        Nodo::Preferiti => Some(voce_preferiti()),
        Nodo::Tutto(dentro) => Some(voce_tutto(dentro, None)),
        Nodo::AlbumLettera(lettera) | Nodo::ArtistiLettera(lettera) => {
            Some(voce_lettera(nodo.clone(), lettera, 0))
        }
        // Le schede le fa Kotlin, e una ricerca non è una voce da ritrovare.
        Nodo::Recenti | Nodo::Album | Nodo::Artisti | Nodo::Playlist | Nodo::Cerca(_) => None,
    })
}

// ── che cosa suonare ────────────────────────────────────────────────────────

/// I brani di un nodo che si suona per intero, nell'ordine in cui suonano.
fn elenco(connessione: &Connection, origine: &Origine, nodo: &Nodo) -> Result<Vec<i64>, AppError> {
    let righe = |brani: Vec<TrackSummary>| brani.into_iter().map(|brano| brano.id).collect();
    Ok(match nodo {
        Nodo::Recenti => righe(ascoltati_di_recente(connessione, origine, RECENTI)?),
        Nodo::Preferiti => righe(list_liked(connessione, origine, 0, TETTO_CODA)?),
        Nodo::AlbumDi(chiave) => righe(album_tracks(connessione, origine, chiave)?),
        Nodo::PlaylistDi(id) => righe(raggiungibili(
            connessione,
            origine,
            playlists::tracks(connessione, *id)?,
        )?),
        Nodo::Cerca(testo) => righe(search(connessione, origine, testo, 0, TETTO_CODA)?),
        Nodo::ArtistaDi(nome) => brani_di_artista(connessione, origine, nome)?,
        Nodo::Brano { id, .. } => vec![*id],
        Nodo::Tutto(dentro) => elenco(connessione, origine, dentro)?,
        Nodo::Album
        | Nodo::AlbumLettera(_)
        | Nodo::Artisti
        | Nodo::ArtistiLettera(_)
        | Nodo::Playlist => Vec::new(),
    })
}

/// Tutti i brani di un artista: i suoi album, uno dopo l'altro, nell'ordine
/// della sua pagina nella finestra.
fn brani_di_artista(
    connessione: &Connection,
    origine: &Origine,
    nome: &str,
) -> Result<Vec<i64>, AppError> {
    let mut ids = Vec::new();
    for album in albums_by_artist(connessione, origine, nome, 0, TETTO_ALBUM_ARTISTA)? {
        ids.extend(
            album_tracks(connessione, origine, &album.album_key)?
                .into_iter()
                .map(|brano| brano.id),
        );
    }
    Ok(ids)
}

/// La coda che parte da un nodo, e da quale suo brano.
///
/// Per un brano è la lista da cui lo si è scelto, dal brano in poi; se nel
/// frattempo la lista è cambiata e il brano non c'è più, il brano da solo.
///
/// # Errori
///
/// Quelli della libreria; `library.trackNotFound` per un nodo che non si
/// suona — una scheda, una lettera — o che è vuoto.
pub fn da_suonare(
    connessione: &Connection,
    origine: &Origine,
    nodo: &Nodo,
) -> Result<(Vec<i64>, usize), AppError> {
    let (ids, da) = match nodo {
        Nodo::Brano { id, dentro } => {
            let lista = elenco(connessione, origine, dentro)?;
            match lista.iter().position(|riga| riga == id) {
                Some(da) => (lista, da),
                None => (vec![*id], 0),
            }
        }
        altro => (elenco(connessione, origine, altro)?, 0),
    };
    if ids.is_empty() {
        return Err(niente(&nodo.testo()));
    }
    Ok((ids, da))
}

/// I brani che una richiesta a voce chiede, in ordine. Vuoto se non c'è
/// niente che le somigli.
///
/// L'ordine delle prove è quello di chi parla: se l'assistente ha capito un
/// titolo, si cerca il brano; un album, l'album; solo un artista, l'artista.
/// Una frase che non ha scomposto si prova prima come artista, poi come
/// album, poi come ricerca libera — «metti i Queen» vuol dire i Queen, non
/// tutti i brani che hanno «Queen» da qualche parte.
///
/// # Errori
///
/// Quelli della libreria.
pub fn per_la_voce(
    connessione: &Connection,
    origine: &Origine,
    ricerca: &RicercaVocale,
) -> Result<Vec<i64>, AppError> {
    let righe = |brani: Vec<TrackSummary>| -> Vec<i64> {
        brani.into_iter().map(|brano| brano.id).collect()
    };
    let pulito = |parte: &Option<String>| {
        parte
            .as_deref()
            .map(str::trim)
            .filter(|parte| !parte.is_empty())
            .map(ToOwned::to_owned)
    };
    let (artista, album, titolo) = (
        pulito(&ricerca.artista),
        pulito(&ricerca.album),
        pulito(&ricerca.titolo),
    );
    if let Some(titolo) = titolo {
        let domanda = artista
            .as_ref()
            .map_or_else(|| titolo.clone(), |artista| format!("{titolo} {artista}"));
        let trovati = righe(search(connessione, origine, &domanda, 0, TETTO_CODA)?);
        if !trovati.is_empty() {
            return Ok(trovati);
        }
        return Ok(righe(search(connessione, origine, &titolo, 0, TETTO_CODA)?));
    }
    if let Some(album) = album {
        if let Some(trovato) = scegli_album(
            &tutti_gli_album(connessione, origine)?,
            &album,
            artista.as_deref(),
        ) {
            return Ok(righe(album_tracks(
                connessione,
                origine,
                &trovato.album_key,
            )?));
        }
        return Ok(righe(search(connessione, origine, &album, 0, TETTO_CODA)?));
    }
    if let Some(artista) = artista {
        if let Some(trovato) = scegli_artista(&artisti_in_ordine(connessione, origine)?, &artista) {
            return brani_di_artista(connessione, origine, &trovato.name);
        }
        return Ok(righe(search(
            connessione,
            origine,
            &artista,
            0,
            TETTO_CODA,
        )?));
    }
    let testo = ricerca.testo.trim();
    if testo.is_empty() {
        return Ok(Vec::new());
    }
    if let Some(trovato) = scegli_artista(&artisti_in_ordine(connessione, origine)?, testo) {
        return brani_di_artista(connessione, origine, &trovato.name);
    }
    if let Some(trovato) = scegli_album(&tutti_gli_album(connessione, origine)?, testo, None) {
        return Ok(righe(album_tracks(
            connessione,
            origine,
            &trovato.album_key,
        )?));
    }
    Ok(righe(search(connessione, origine, testo, 0, TETTO_CODA)?))
}

/// Due nomi «uguali» per chi parla: stessa chiave d'ordinamento, che ignora
/// maiuscole, articoli e punteggiatura. «the beatles» è The Beatles.
fn stesso_nome(a: &str, b: &str) -> bool {
    let (a, b) = (sort_name(a), sort_name(b));
    !a.is_empty() && a == b
}

/// L'artista di cui parla una richiesta: quello col nome uguale; se nessuno,
/// l'unico il cui nome contiene la richiesta per intero. Due candidati a pari
/// merito non sono una risposta: meglio la ricerca libera che un artista a
/// caso.
fn scegli_artista<'a>(artisti: &'a [ArtistSummary], nome: &str) -> Option<&'a ArtistSummary> {
    if let Some(uguale) = artisti
        .iter()
        .find(|artista| stesso_nome(&artista.name, nome))
    {
        return Some(uguale);
    }
    let cercato = sort_name(nome);
    if cercato.chars().count() < 3 {
        return None;
    }
    let mut candidati = artisti
        .iter()
        .filter(|artista| sort_name(&artista.name).contains(&cercato));
    match (candidati.next(), candidati.next()) {
        (Some(unico), None) => Some(unico),
        _ => None,
    }
}

/// L'album di cui parla una richiesta: titolo uguale e, se l'assistente l'ha
/// detto, anche l'artista.
fn scegli_album<'a>(
    album: &'a [AlbumSummary],
    titolo: &str,
    artista: Option<&str>,
) -> Option<&'a AlbumSummary> {
    album.iter().find(|candidato| {
        stesso_nome(&candidato.title, titolo)
            && artista.is_none_or(|artista| stesso_nome(&candidato.artist, artista))
    })
}

fn niente(dove: &str) -> AppError {
    AppError::new(ErrorCode::LibraryTrackNotFound { track_id: None })
        .with_cause(format!("niente da suonare in «{dove}»"))
}

// ── con l'applicazione ──────────────────────────────────────────────────────

/// Legge la libreria per l'auto, con lo store delle copertine accanto.
///
/// # Errori
///
/// `internal.aborted` se la libreria non è più fra gli stati gestiti — l'app
/// si chiude —, poi quelli della libreria.
#[cfg(mobile)]
pub fn leggi<T>(
    app: &tauri::AppHandle,
    lettura: impl FnOnce(&Connection, &Origine, &CoverStore) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let stato = app.try_state::<Stato>().ok_or_else(|| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some("auto".to_owned()),
        })
        .with_cause("la libreria non è più fra gli stati gestiti")
    })?;
    let store = stato.copertine()?;
    // Nell'auto non c'è una riga grigia: un brano che non parte è un guasto al
    // volante. Le righe dei dispositivi che adesso non rispondono non si mostrano.
    let origine = crate::lontano::origine_che_suona(app);
    con_libreria(&stato, |libreria| {
        lettura(&libreria.connection, &origine, &store)
    })
}

/// Esegue un gesto dell'auto con i comandi della finestra: la stessa coda,
/// gli stessi salvataggi, lo stesso stato a tutti quelli che guardano.
///
/// # Errori
///
/// Quelli dei comandi; `library.trackNotFound` per una voce o una ricerca che
/// non porta a niente da suonare.
#[cfg(mobile)]
pub fn esegui(app: &tauri::AppHandle, gesto: GestoAuto) -> Esito<()> {
    let Some(lettore) = app.try_state::<crate::riproduzione::StatoLettore>() else {
        return Ok(());
    };
    match gesto {
        GestoAuto::Suona(testo) => {
            if testo == ATTUALE {
                return crate::riproduzione::riprendi_ora(app.clone(), lettore);
            }
            let nodo = Nodo::da_testo(&testo).ok_or_else(|| errore(niente(&testo)))?;
            let (ids, da) = leggi(app, |connessione, origine, _| {
                da_suonare(connessione, origine, &nodo)
            })
            .map_err(errore)?;
            crate::riproduzione::suona_ora(app.clone(), lettore, ids, da)
        }
        GestoAuto::Cerca(ricerca) => {
            if ricerca.vuota() {
                // «Metti un po' di musica»: la coda che c'è, se c'è; se no gli
                // ascoltati di recente, che sono la musica di chi ascolta.
                if crate::riproduzione::c_e_un_brano(&lettore) {
                    return crate::riproduzione::riprendi_ora(app.clone(), lettore);
                }
                let (ids, da) = leggi(app, |connessione, origine, _| {
                    da_suonare(connessione, origine, &Nodo::Recenti)
                })
                .map_err(errore)?;
                return crate::riproduzione::suona_ora(app.clone(), lettore, ids, da);
            }
            let ids = leggi(app, |connessione, origine, _| {
                per_la_voce(connessione, origine, &ricerca)
            })
            .map_err(errore)?;
            if ids.is_empty() {
                return Err(errore(niente(&ricerca.testo)));
            }
            crate::riproduzione::suona_ora(app.clone(), lettore, ids, 0)
        }
        GestoAuto::Coda(indice) => {
            crate::riproduzione::coda_vai_ora(app.clone(), lettore, indice, None)
        }
        GestoAuto::Mescola => crate::riproduzione::mescola_ora(app.clone(), lettore),
        GestoAuto::Ripeti => crate::riproduzione::ripeti_ora(app.clone(), lettore),
        GestoAuto::Piace { id, valore } => {
            crate::comandi::preferito_ora(app, &app.state::<Stato>(), id, valore)?;
            // Il cuore va ridisegnato dappertutto — l'auto, la notifica, la
            // finestra se è aperta — e lo stato si rilegge dal database a ogni
            // invio: basta mandarlo.
            crate::riproduzione::primo_stato_fuori(app);
            Ok(())
        }
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn gli_identificativi_tornano_indietro_tali_e_quali() {
        let nodi = [
            Nodo::Recenti,
            Nodo::Album,
            Nodo::Artisti,
            Nodo::Playlist,
            Nodo::Preferiti,
            Nodo::AlbumLettera("Q".to_owned()),
            Nodo::ArtistiLettera("#".to_owned()),
            // Una chiave d'album può contenere qualunque cosa, anche i
            // separatori degli identificativi.
            Nodo::AlbumDi("queen|a night: at the @opera".to_owned()),
            Nodo::ArtistaDi("AC/DC".to_owned()),
            Nodo::PlaylistDi(17),
            Nodo::Cerca("bohemian rhapsody".to_owned()),
            Nodo::Brano {
                id: 42,
                dentro: Box::new(Nodo::AlbumDi("x@y:z".to_owned())),
            },
            Nodo::Brano {
                id: 7,
                dentro: Box::new(Nodo::Preferiti),
            },
            Nodo::Tutto(Box::new(Nodo::AlbumDi("queen:ii".to_owned()))),
            Nodo::Tutto(Box::new(Nodo::ArtistaDi("AC/DC".to_owned()))),
        ];
        for nodo in nodi {
            assert_eq!(
                Nodo::da_testo(&nodo.testo()),
                Some(nodo.clone()),
                "{nodo:?}"
            );
        }
    }

    #[test]
    fn un_identificativo_che_non_scriviamo_noi_non_si_indovina() {
        assert_eq!(Nodo::da_testo("radice"), None);
        assert_eq!(Nodo::da_testo("playlist:dodici"), None);
        assert_eq!(Nodo::da_testo("brano:x@album:a"), None);
        assert_eq!(Nodo::da_testo("brano:3"), None);
        assert_eq!(Nodo::da_testo("brano:3@brano:4@album:a"), None);
        assert_eq!(Nodo::da_testo(ATTUALE), None);
        // «Suona tutto» di un brano, o di un «suona tutto», non lo scriviamo mai.
        assert_eq!(Nodo::da_testo("tutto:brano:1@album:a"), None);
        assert_eq!(Nodo::da_testo("tutto:tutto:album:a"), None);
        assert_eq!(Nodo::da_testo("brano:1@tutto:album:a"), None);
    }

    #[test]
    fn le_pagine_hanno_un_tetto_anche_quando_l_auto_chiede_tutto() {
        let cento: Vec<u32> = (0..1_000).collect();
        // Media3 traduce il «tutto» di un controllore Media1 in `i32::MAX`.
        let tutto = pagina(
            cento.clone(),
            0,
            u32::try_from(i32::MAX).unwrap_or(u32::MAX),
        );
        assert_eq!(tutto.len(), TETTO_PAGINA);
        assert_eq!(pagina(cento.clone(), 1, 10), (10..20).collect::<Vec<_>>());
        assert!(pagina(cento.clone(), 500, 10).is_empty());
        // Una pagina da zero voci non esiste: almeno una.
        assert_eq!(pagina(cento, 0, 0), vec![0]);
        assert_eq!(finestra(2, 50), (100, 50));
        assert_eq!(finestra(0, u32::MAX).1, 200);
    }

    #[test]
    fn suona_tutto_sta_in_cima_alla_prima_pagina_e_basta() {
        let album = Nodo::AlbumDi("x".to_owned());
        let brano = |id| {
            voce_tutto(
                &Nodo::Brano {
                    id,
                    dentro: Box::new(album.clone()),
                },
                None,
            )
        };
        let prima = con_tutto(&album, 0, Some(12), vec![brano(1), brano(2)]);
        assert_eq!(prima.len(), 3);
        assert_eq!(prima.first().map(|voce| voce.tipo), Some(Tipo::Tutto));
        assert_eq!(prima.first().and_then(|voce| voce.quanti), Some(12));
        assert_eq!(
            prima.first().map(|voce| voce.id.as_str()),
            Some("tutto:album:x")
        );
        // Le pagine dopo continuano l'elenco.
        assert_eq!(con_tutto(&album, 1, Some(12), vec![brano(3)]).len(), 1);
        // Un album vuoto non ha niente da suonare tutto.
        assert!(con_tutto(&album, 0, Some(0), Vec::new()).is_empty());
    }

    #[test]
    fn le_lettere_seguono_la_chiave_d_ordinamento() {
        assert_eq!(iniziale("The Cure"), "C");
        assert_eq!(iniziale("queen"), "Q");
        assert_eq!(iniziale("2Pac"), "#");
        assert_eq!(iniziale(""), "#");
        let nomi = ["ABBA", "Air", "The Beatles", "Björk", "U2"];
        assert_eq!(
            lettere(&nomi, |nome| nome),
            vec![
                ("A".to_owned(), 2),
                ("B".to_owned(), 2),
                ("U".to_owned(), 1)
            ]
        );
    }

    fn artista(nome: &str) -> ArtistSummary {
        ArtistSummary {
            name: nome.to_owned(),
            sort_name: sort_name(nome),
            albums: 1,
            tracks: 10,
            covers: Vec::new(),
        }
    }

    fn album(titolo: &str, di: &str) -> AlbumSummary {
        AlbumSummary {
            album_key: format!("{di}|{titolo}"),
            title: titolo.to_owned(),
            artist: di.to_owned(),
            year: None,
            genre: None,
            total_tracks: 10,
            cover_art_hash: None,
        }
    }

    #[test]
    fn a_voce_un_artista_si_riconosce_senza_articoli_e_maiuscole() {
        let artisti = [
            artista("The Beatles"),
            artista("Queen"),
            artista("Queensrÿche"),
        ];
        assert_eq!(
            scegli_artista(&artisti, "beatles").map(|a| a.name.as_str()),
            Some("The Beatles")
        );
        // Uguale vince su «contiene»: Queen e non Queensrÿche.
        assert_eq!(
            scegli_artista(&artisti, "queen").map(|a| a.name.as_str()),
            Some("Queen")
        );
        // Due che contengono la richiesta non sono una risposta.
        let simili = [artista("Pink Floyd"), artista("Pink")];
        assert_eq!(
            scegli_artista(&simili, "pin").map(|a| a.name.as_str()),
            None
        );
        // Troppo corto per un «contiene».
        assert_eq!(scegli_artista(&artisti, "qu"), None);
    }

    #[test]
    fn a_voce_un_album_si_riconosce_anche_con_l_artista() {
        let dischi = [
            album("Greatest Hits", "Queen"),
            album("Greatest Hits", "ABBA"),
        ];
        assert_eq!(
            scegli_album(&dischi, "greatest hits", Some("abba")).map(|a| a.artist.as_str()),
            Some("ABBA")
        );
        assert_eq!(
            scegli_album(&dischi, "greatest hits", None).map(|a| a.artist.as_str()),
            Some("Queen")
        );
        assert_eq!(scegli_album(&dischi, "innuendo", None), None);
    }

    #[test]
    fn una_richiesta_senza_niente_dentro_e_vuota() {
        assert!(RicercaVocale::default().vuota());
        assert!(
            RicercaVocale {
                testo: "  ".to_owned(),
                artista: Some(String::new()),
                ..RicercaVocale::default()
            }
            .vuota()
        );
        assert!(
            !RicercaVocale {
                artista: Some("Queen".to_owned()),
                ..RicercaVocale::default()
            }
            .vuota()
        );
    }

    #[test]
    fn le_copertine_sono_percorsi_dentro_lo_store() {
        let cartella = tempfile::tempdir().expect("cartella di prova");
        let store = CoverStore::open(cartella.path()).expect("cartella di prova");
        let impronta = "abcdef0123";
        // Nessun file: nessuna copertina, invece di un indirizzo che non si apre.
        assert_eq!(copertina(&store, Some(impronta)), None);
        let piena = store.path_for(impronta);
        std::fs::create_dir_all(piena.parent().unwrap_or(cartella.path()))
            .expect("cartella di prova");
        std::fs::write(&piena, b"jpg").expect("cartella di prova");
        assert_eq!(
            copertina(&store, Some(impronta)).as_deref(),
            Some("ab/abcdef0123.jpg")
        );
        // La media, quando c'è, vince: è della misura giusta per una griglia.
        std::fs::write(store.percorso_media(impronta), b"jpg").expect("cartella di prova");
        assert_eq!(
            copertina(&store, Some(impronta)).as_deref(),
            Some("ab/abcdef0123.m.jpg")
        );
        assert_eq!(copertina(&store, None), None);
    }
}
