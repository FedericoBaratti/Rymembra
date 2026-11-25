//! Il confine di fiducia: tutto quel che arriva da un altro dispositivo, prima
//! di toccare `tracks`.
//!
//! Un dispositivo collegato è fidato per il **codice** a quattro cifre che ha
//! superato, non per i **byte** che manda: il suo catalogo finisce nella
//! libreria, nell'indice di ricerca e sulla finestra. Qui ogni riga si
//! ripulisce e si controlla, e quel che non sta nei tetti si scarta — contato,
//! non in silenzio: il riconciliatore lo dice nel diario — invece di entrare:
//!
//! - **un tetto di righe per dispositivo** ([`MAX_RIGHE_PER_DISPOSITIVO`]);
//! - **lunghezze massime** per ogni testo. Sono anche quelle con cui chi serve
//!   taglia le sue righe (`condivisa::catalogo_da_riga`), perché cinquecento
//!   righe stiano nella cornice del protocollo;
//! - **testo puro**: niente caratteri di controllo né comandi di direzione, che
//!   farebbero leggere a schermo un testo diverso;
//! - **impronte** che passano [`crate::covers::e_un_impronta`], chiavi
//!   d'album di trentadue cifre, identificativi MusicBrainz nella forma di un
//!   UUID;
//! - **id > 0** e dentro il secchio chiesto, ciascuno una volta sola;
//! - **estensioni** fra quelle che il decodificatore conosce
//!   ([`SUPPORTED_EXTENSIONS`]);
//! - numeri nei loro intervalli, e nessun orologio nel futuro.
//!
//! Una chiave d'identità (`track_key`, `content_key`) non si taglia: tagliata
//! direbbe un altro brano. Se non sta nel tetto, è la riga a non entrare.

use rymembra_domain::album::{UNKNOWN_ALBUM, UNKNOWN_ARTIST};
use rymembra_domain::paths::SUPPORTED_EXTENSIONS;

use crate::library::condivisa::{BranoCatalogo, ID_PER_SECCHIO, Segmento};

/// Quante righe, al più, si tengono di un dispositivo: oltre, le righe nuove
/// non entrano. Una libreria vera sta ben sotto (cinquantamila brani).
pub const MAX_RIGHE_PER_DISPOSITIVO: usize = 200_000;

/// Quanti byte, al più, ha un titolo, un interprete, un album, un artista
/// dell'album.
pub const MAX_NOME: usize = 200;

/// Quanti byte, al più, ha un genere.
pub const MAX_GENERE: usize = 64;

/// Quanti byte, al più, ha il nome di un formato.
pub const MAX_CODEC: usize = 24;

/// Quanti byte, al più, ha una chiave d'identità.
pub const MAX_CHIAVE: usize = 300;

/// Quanti byte ha un identificativo MusicBrainz: un UUID.
pub const MAX_ID_MB: usize = 36;

/// Una durata oltre le quarantotto ore non è un brano.
const MAX_DURATA_MS: i64 = 48 * 3600 * 1000;

/// Quanto un orologio altrui può stare avanti al nostro, ms: un giorno, per un
/// fuso o un telefono con l'ora storta.
const MARGINE_OROLOGIO_MS: i64 = 24 * 3600 * 1000;

/// Perché una riga non è entrata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scarto {
    /// L'id non è positivo.
    IdNonValido,
    /// L'id sta fuori dal secchio chiesto.
    FuoriDalSecchio,
    /// Lo stesso id già visto in questo secchio.
    Ripetuta,
    /// Il titolo è vuoto.
    SenzaTitolo,
    /// La chiave d'identità manca, è troppo lunga o ha dei controlli.
    ChiaveNonValida,
    /// L'estensione non è fra quelle che il decodificatore conosce.
    EstensioneNonAmmessa,
    /// La dimensione è negativa.
    DimensioneNegativa,
    /// Il pezzo di un'immagine CD finisce prima di cominciare.
    SegmentoNonValido,
}

/// Quel che di un secchio è entrato, e quanto no.
#[derive(Debug, Clone, PartialEq)]
pub struct Convalidate {
    /// Le righe buone, per id crescente.
    pub righe: Vec<BranoCatalogo>,
    /// Quante si sono scartate.
    pub scartate: usize,
}

/// Un testo come testo puro, come quelli dei nomi che arrivano da fuori
/// (`insieme::ripulisci_nome`, `protocollo::testo_puro`): i caratteri di
/// controllo e gli spazi di ogni sorta diventano uno spazio solo, e mai in
/// testa né in coda; spariscono gli spazi di larghezza zero e i comandi di
/// direzione.
#[must_use]
pub fn ripulisci(grezzo: &str) -> String {
    let mut pulito = String::with_capacity(grezzo.len());
    let mut spazio_in_sospeso = false;
    for carattere in grezzo.chars() {
        if carattere.is_control() || carattere.is_whitespace() {
            spazio_in_sospeso = !pulito.is_empty();
            continue;
        }
        if matches!(
            carattere,
            '\u{200B}' | '\u{FEFF}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
        ) {
            continue;
        }
        if spazio_in_sospeso {
            pulito.push(' ');
            spazio_in_sospeso = false;
        }
        pulito.push(carattere);
    }
    pulito
}

/// Il testo tagliato a `massimo` byte, su un confine fra caratteri.
#[must_use]
pub fn tronca(testo: &str, massimo: usize) -> String {
    if testo.len() <= massimo {
        return testo.to_owned();
    }
    let mut fine = massimo;
    while fine > 0 && !testo.is_char_boundary(fine) {
        fine -= 1;
    }
    testo.get(..fine).unwrap_or_default().to_owned()
}

/// [`tronca`] per un testo che può mancare.
#[must_use]
pub fn tronca_opz(testo: Option<String>, massimo: usize) -> Option<String> {
    testo.map(|t| tronca(&t, massimo))
}

/// Un testo da mostrare: ripulito e nei tetti, vuoto se non resta niente.
fn testo(grezzo: &str, massimo: usize) -> String {
    let tagliato = tronca(&ripulisci(grezzo), massimo);
    // `ripulisci` non lascia mai uno spazio in coda, ma il taglio può fermarsi proprio dopo uno: senza
    // questo il testo usciva con lo spazio, e una seconda convalida lo toglieva (la convalida non
    // era idempotente). Dopo `ripulisci` l'unico spazio rimasto è U+0020.
    tagliato.trim_end().to_owned()
}

/// Un testo facoltativo: `None` se non resta niente.
fn testo_opz(grezzo: Option<&str>, massimo: usize) -> Option<String> {
    grezzo.map(|g| testo(g, massimo)).filter(|t| !t.is_empty())
}

/// Una chiave d'identità: com'è, o `None`. Mai tagliata, mai ripulita.
fn chiave_d_identita(grezza: &str) -> Option<String> {
    (!grezza.is_empty() && grezza.len() <= MAX_CHIAVE && !grezza.chars().any(char::is_control))
        .then(|| grezza.to_owned())
}

/// Un identificativo MusicBrainz: un UUID, esadecimale minuscolo con i
/// trattini. Il resto non è un identificativo.
fn uuid(grezzo: Option<&str>) -> Option<String> {
    let g = grezzo?;
    let forma_giusta = g.len() == MAX_ID_MB
        && g.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
            }
        });
    forma_giusta.then(|| g.to_owned())
}

/// Un intero dentro `[0, massimo]`, o `None`.
fn nel_tetto(valore: Option<i64>, massimo: i64) -> Option<i64> {
    valore.filter(|v| (0..=massimo).contains(v))
}

/// Un orologio: non negativo, e non oltre un giorno avanti all'orologio di qui.
fn orologio(valore: i64, adesso_ms: i64) -> i64 {
    valore.clamp(0, adesso_ms.saturating_add(MARGINE_OROLOGIO_MS))
}

/// Una riga del catalogo di un altro dispositivo, nei tetti — o il perché non
/// entra. `adesso_ms` è l'orologio di qui.
///
/// # Errori
///
/// [`Scarto`], per la prima ragione che la esclude.
pub fn convalida_riga(grezza: BranoCatalogo, adesso_ms: i64) -> Result<BranoCatalogo, Scarto> {
    if grezza.id <= 0 {
        return Err(Scarto::IdNonValido);
    }
    let title = testo(&grezza.title, MAX_NOME);
    if title.is_empty() {
        return Err(Scarto::SenzaTitolo);
    }
    let track_key = chiave_d_identita(&grezza.track_key).ok_or(Scarto::ChiaveNonValida)?;
    // Una chiave di contenuto che non sta nei tetti non è un errore: si fa a
    // meno di lei, e il brano ripiega su `track_key`.
    let content_key = grezza.content_key.as_deref().and_then(chiave_d_identita);
    let estensione = grezza.estensione.trim().to_ascii_lowercase();
    if !SUPPORTED_EXTENSIONS.contains(&estensione.as_str()) {
        return Err(Scarto::EstensioneNonAmmessa);
    }
    if grezza.dimensione < 0 {
        return Err(Scarto::DimensioneNegativa);
    }
    let segmento = match grezza.segmento {
        Some(Segmento { inizio_cd, fine_cd }) => {
            if inizio_cd < 0 || fine_cd.is_some_and(|fine| fine <= inizio_cd) {
                return Err(Scarto::SegmentoNonValido);
            }
            Some(Segmento { inizio_cd, fine_cd })
        }
        None => None,
    };
    let versione: String = grezza
        .versione
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '-')
        .take(48)
        .collect();
    let album_key = grezza.album_key.filter(|k| {
        k.len() == 32
            && k.bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    });
    let cover_art_hash = grezza
        .cover_art_hash
        .filter(|h| (16..=128).contains(&h.len()) && crate::covers::e_un_impronta(h));
    Ok(BranoCatalogo {
        id: grezza.id,
        title,
        artist: Some(testo(&grezza.artist, MAX_NOME))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| UNKNOWN_ARTIST.to_owned()),
        album: Some(testo(&grezza.album, MAX_NOME))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| UNKNOWN_ALBUM.to_owned()),
        album_artist: testo_opz(grezza.album_artist.as_deref(), MAX_NOME),
        genre: testo_opz(grezza.genre.as_deref(), MAX_GENERE),
        album_key,
        track_number: nel_tetto(grezza.track_number, 9_999),
        disc_number: nel_tetto(grezza.disc_number, 9_999),
        year: nel_tetto(grezza.year, 9_999),
        duration_ms: grezza.duration_ms.clamp(0, MAX_DURATA_MS),
        cover_art_hash,
        play_count: grezza.play_count.max(0),
        liked: grezza.liked,
        rating: grezza.rating.clamp(0, 5),
        liked_at: grezza.liked_at.map(|v| orologio(v, adesso_ms)),
        rating_at: grezza.rating_at.map(|v| orologio(v, adesso_ms)),
        date_added: orologio(grezza.date_added, adesso_ms),
        last_played_at: grezza.last_played_at.map(|v| orologio(v, adesso_ms)),
        stats_updated_at: orologio(grezza.stats_updated_at, adesso_ms),
        bitrate: nel_tetto(grezza.bitrate, 100_000_000),
        sample_rate: nel_tetto(grezza.sample_rate, 2_000_000),
        channels: nel_tetto(grezza.channels, 64),
        codec: testo_opz(grezza.codec.as_deref(), MAX_CODEC),
        track_key,
        content_key,
        mb_recording_id: uuid(grezza.mb_recording_id.as_deref()),
        mb_release_group_id: uuid(grezza.mb_release_group_id.as_deref()),
        mb_release_id: uuid(grezza.mb_release_id.as_deref()),
        estensione,
        dimensione: grezza.dimensione,
        versione,
        segmento,
    })
}

/// Le righe di un secchio, nei tetti: quelle fuori dall'intervallo chiesto, le
/// ripetute e le oltre [`ID_PER_SECCHIO`] non entrano.
#[must_use]
pub fn convalida_secchio(
    da_id: i64,
    a_id: i64,
    grezze: Vec<BranoCatalogo>,
    adesso_ms: i64,
) -> Convalidate {
    let mut viste = std::collections::HashSet::new();
    let mut righe = Vec::with_capacity(
        grezze
            .len()
            .min(usize::try_from(ID_PER_SECCHIO).unwrap_or(0)),
    );
    let mut scartate = 0_usize;
    for grezza in grezze {
        let id = grezza.id;
        let esito = if id < da_id || id > a_id {
            Err(Scarto::FuoriDalSecchio)
        } else if !viste.insert(id) {
            Err(Scarto::Ripetuta)
        } else {
            convalida_riga(grezza, adesso_ms)
        };
        match esito {
            Ok(riga) => righe.push(riga),
            Err(_) => scartate += 1,
        }
    }
    // Un secchio ha al più tanti id quanti ne dice [`ID_PER_SECCHIO`].
    let tetto = usize::try_from(ID_PER_SECCHIO).unwrap_or(usize::MAX);
    if righe.len() > tetto {
        scartate += righe.len() - tetto;
        righe.truncate(tetto);
    }
    righe.sort_by_key(|r| r.id);
    Convalidate { righe, scartate }
}

#[cfg(test)]
mod prove {
    use super::*;

    use crate::specchio::comuni::{ADESSO, riga};

    #[test]
    fn una_riga_vera_passa_com_e() {
        let buona = riga(7);
        assert_eq!(convalida_riga(buona.clone(), ADESSO), Ok(buona));
    }

    #[test]
    fn i_testi_diventano_testo_puro_e_stanno_nei_tetti() {
        let mut r = riga(1);
        r.title = format!("  Ti\u{0}tolo\u{202E} \t lungo {}", "x".repeat(1_000));
        r.artist = "\u{200B}\u{FEFF}".to_owned();
        r.album = String::new();
        r.genre = Some("\n".to_owned());
        r.album_artist = Some("é".repeat(500));
        let v = convalida_riga(r, ADESSO).unwrap();
        assert!(v.title.starts_with("Ti tolo lungo xxx"), "{:?}", v.title);
        assert!(!v.title.chars().any(char::is_control));
        assert!(!v.title.contains('\u{202E}'));
        assert!(v.title.len() <= MAX_NOME);
        assert_eq!(v.artist, UNKNOWN_ARTIST, "un interprete vuoto ripiega");
        assert_eq!(v.album, UNKNOWN_ALBUM);
        assert_eq!(v.genre, None);
        let artista_album = v.album_artist.unwrap();
        assert!(artista_album.len() <= MAX_NOME);
        assert!(
            artista_album.chars().all(|c| c == 'é'),
            "taglio su un carattere intero"
        );
    }

    #[test]
    fn le_righe_che_non_stanno_nei_tetti_non_entrano() {
        let casi: Vec<(BranoCatalogo, Scarto)> = vec![
            (BranoCatalogo { id: 0, ..riga(1) }, Scarto::IdNonValido),
            (BranoCatalogo { id: -4, ..riga(1) }, Scarto::IdNonValido),
            (
                BranoCatalogo {
                    title: " \u{200B} ".to_owned(),
                    ..riga(1)
                },
                Scarto::SenzaTitolo,
            ),
            (
                BranoCatalogo {
                    track_key: String::new(),
                    ..riga(1)
                },
                Scarto::ChiaveNonValida,
            ),
            (
                BranoCatalogo {
                    track_key: "k".repeat(MAX_CHIAVE + 1),
                    ..riga(1)
                },
                Scarto::ChiaveNonValida,
            ),
            (
                BranoCatalogo {
                    track_key: "a|b\u{0}|c".to_owned(),
                    ..riga(1)
                },
                Scarto::ChiaveNonValida,
            ),
            (
                BranoCatalogo {
                    estensione: "exe".to_owned(),
                    ..riga(1)
                },
                Scarto::EstensioneNonAmmessa,
            ),
            (
                BranoCatalogo {
                    estensione: String::new(),
                    ..riga(1)
                },
                Scarto::EstensioneNonAmmessa,
            ),
            (
                BranoCatalogo {
                    estensione: "../mp3".to_owned(),
                    ..riga(1)
                },
                Scarto::EstensioneNonAmmessa,
            ),
            (
                BranoCatalogo {
                    dimensione: -1,
                    ..riga(1)
                },
                Scarto::DimensioneNegativa,
            ),
            (
                BranoCatalogo {
                    segmento: Some(Segmento {
                        inizio_cd: 10,
                        fine_cd: Some(10),
                    }),
                    ..riga(1)
                },
                Scarto::SegmentoNonValido,
            ),
        ];
        for (grezza, atteso) in casi {
            assert_eq!(
                convalida_riga(grezza.clone(), ADESSO),
                Err(atteso),
                "{grezza:?}"
            );
        }
        // Un'estensione in maiuscolo è la stessa.
        let v = convalida_riga(
            BranoCatalogo {
                estensione: " FLAC ".to_owned(),
                ..riga(1)
            },
            ADESSO,
        )
        .unwrap();
        assert_eq!(v.estensione, "flac");
    }

    #[test]
    fn i_campi_facoltativi_storti_si_perdono_e_la_riga_resta() {
        let mut r = riga(1);
        r.album_key = Some("troppo corta".to_owned());
        r.cover_art_hash = Some("../../etc/passwd".to_owned());
        r.mb_recording_id = Some("non un uuid".to_owned());
        r.mb_release_id = Some("0123456789ABCDEF0123456789abcdef0123".to_owned());
        r.mb_release_group_id = Some("5b11f4ce-a62d-471e-81fc-a69a8278c7da".to_owned());
        r.content_key = Some("k".repeat(MAX_CHIAVE + 1));
        r.year = Some(123_456);
        r.track_number = Some(-1);
        r.bitrate = Some(i64::MAX);
        r.channels = Some(500);
        r.rating = 99;
        r.play_count = -5;
        r.duration_ms = i64::MAX;
        r.versione = "<script>1-2".to_owned();
        let v = convalida_riga(r, ADESSO).unwrap();
        assert_eq!(v.album_key, None);
        assert_eq!(v.cover_art_hash, None);
        assert_eq!(v.mb_recording_id, None);
        assert_eq!(v.mb_release_id, None, "maiuscolo non vale");
        assert_eq!(
            v.mb_release_group_id.as_deref(),
            Some("5b11f4ce-a62d-471e-81fc-a69a8278c7da")
        );
        assert_eq!(v.content_key, None);
        assert_eq!(
            (v.year, v.track_number, v.bitrate, v.channels),
            (None, None, None, None)
        );
        assert_eq!((v.rating, v.play_count), (5, 0));
        assert_eq!(v.duration_ms, MAX_DURATA_MS);
        assert_eq!(v.versione, "1-2", "solo cifre e trattini");
    }

    #[test]
    fn un_orologio_nel_futuro_non_vale_piu_di_un_giorno_avanti() {
        let mut r = riga(1);
        r.stats_updated_at = i64::MAX;
        r.liked_at = Some(ADESSO + 10 * 365 * 24 * 3600 * 1000);
        r.rating_at = Some(-5);
        r.last_played_at = Some(ADESSO - 1000);
        r.date_added = -1;
        let v = convalida_riga(r, ADESSO).unwrap();
        let tetto = ADESSO + MARGINE_OROLOGIO_MS;
        assert_eq!(v.stats_updated_at, tetto);
        assert_eq!(v.liked_at, Some(tetto));
        assert_eq!(v.rating_at, Some(0));
        assert_eq!(v.last_played_at, Some(ADESSO - 1000));
        assert_eq!(v.date_added, 0);
    }

    #[test]
    fn un_secchio_non_porta_id_di_un_altro_ne_ripetuti_ne_piu_del_dovuto() {
        let da = 1_000;
        let a = 1_499;
        let mut grezze: Vec<BranoCatalogo> = (da..=a).map(riga).collect();
        grezze.push(riga(999)); // fuori, prima
        grezze.push(riga(1_500)); // fuori, dopo
        grezze.push(riga(1_200)); // ripetuta
        grezze.push(BranoCatalogo {
            id: 1_300,
            estensione: "exe".to_owned(),
            ..riga(1_300)
        }); // ripetuta (già vista)
        let v = convalida_secchio(da, a, grezze, ADESSO);
        assert_eq!(v.righe.len(), 500);
        assert_eq!(v.scartate, 4);
        assert!(v.righe.windows(2).all(|c| c[0].id < c[1].id), "per id");
        assert_eq!(v.righe.first().map(|r| r.id), Some(1_000));
        assert_eq!(v.righe.last().map(|r| r.id), Some(1_499));
    }

    #[test]
    fn un_testo_tagliato_dopo_uno_spazio_non_resta_con_lo_spazio_in_coda() {
        // B2: `ripulisci` non lascia mai uno spazio in coda, ma il taglio a `MAX_NOME` byte può
        // fermarsi proprio dopo uno spazio, e la riga restava «xxx… » (con lo spazio). Una seconda
        // convalida lo toglieva: la convalida non era idempotente, e due righe dello stesso brano
        // — una convalidata una volta, una due — non avevano lo stesso titolo.
        let mut r = riga(1);
        r.title = format!("{} y z", "x".repeat(MAX_NOME - 1));
        r.album_artist = Some(format!("{}x y", "é".repeat(99)));
        r.genre = Some(format!("{} pop", "g".repeat(MAX_GENERE - 1)));
        let una = convalida_riga(r, ADESSO).unwrap();
        for testo in [
            Some(&una.title),
            una.album_artist.as_ref(),
            una.genre.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            assert!(!testo.ends_with(' '), "spazio in coda: {testo:?}");
        }
        assert_eq!(convalida_riga(una.clone(), ADESSO), Ok(una));
    }

    #[test]
    fn il_taglio_sta_su_un_confine_fra_caratteri() {
        assert_eq!(tronca("àèìòù", 3), "à");
        assert_eq!(tronca("abc", 10), "abc");
        assert_eq!(tronca("日本語", 4), "日");
        assert_eq!(tronca("日本語", 2), "");
    }
}
