//! iTunes Search: il secondo parere, e l'unico che dà un genere.
//!
//! Il punto di ricerca di Apple è pubblico, documentato e senza chiave. Serve a
//! tre cose che MusicBrainz non fa altrettanto bene:
//!
//! - **il consenso**, che senza impronta acustica è la prova che
//!   `decide_track` pretende. Il catalogo di Apple è costruito dai distributori
//!   e non dai collaboratori, quindi sbaglia in modi diversi da MusicBrainz: è
//!   proprio questo che rende il loro accordo un'informazione invece di un'eco;
//! - **il genere**, che MusicBrainz non modella affatto — là è una proprietà
//!   dell'artista, non del brano;
//! - **una copertina quadrata ad alta risoluzione**, legata alla pubblicazione
//!   esatta invece che al gruppo di pubblicazione.

use aether_domain::enrich::{Candidate, FonteMeta, normalize_for_match, titolo_da_cercare};
use aether_domain::errors::AppError;
use aether_net::percento;
use serde_json::Value;

use crate::Fornitori;
use crate::deposito::VIVE_RICERCA_MS;

/// Il lato che si chiede per la copertina.
///
/// Seicento e non milleduecento: lo store delle copertine ricodifica comunque a
/// 640 (`covers::MAX_LATO`), quindi ogni pixel oltre quella soglia è banda spesa
/// per essere buttata dal ridimensionamento.
const LATO_COPERTINA: u32 = 600;

/// Interpreta la risposta di una ricerca. Funzione pura.
#[must_use]
pub fn interpreta(corpo: &[u8]) -> Vec<Candidate> {
    let Ok(letto) = serde_json::from_slice::<Value>(corpo) else {
        return Vec::new();
    };
    let Some(elenco) = letto.get("results").and_then(Value::as_array) else {
        return Vec::new();
    };
    elenco
        .iter()
        .filter_map(|voce| {
            let titolo = voce.get("trackName").and_then(Value::as_str)?;
            let artista = voce.get("artistName").and_then(Value::as_str)?;
            Some(Candidate {
                fonte: Some(FonteMeta::Itunes),
                title: titolo.to_owned(),
                artist: artista.to_owned(),
                album: voce
                    .get("collectionName")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                year: voce
                    .get("releaseDate")
                    .and_then(Value::as_str)
                    .and_then(|d| d.get(..4))
                    .and_then(|a| a.parse::<i32>().ok())
                    .filter(|a| *a > 0),
                genre: voce
                    .get("primaryGenreName")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                duration_ms: voce
                    .get("trackTimeMillis")
                    .and_then(Value::as_u64)
                    .filter(|d| *d > 0),
                cover_url: voce
                    .get("artworkUrl100")
                    .and_then(Value::as_str)
                    .map(ingrandisci),
                mb_recording_id: None,
                mb_release_id: None,
                mb_release_group_id: None,
            })
        })
        .collect()
}

/// Chiede la copertina più grande, riscrivendo l'ultimo segmento dell'indirizzo.
///
/// Gli indirizzi delle immagini di Apple finiscono con `<larghezza>x<altezza>bb.
/// <estensione>`, e la dimensione è un parametro travestito da nome di file: si
/// riscrive e si ottiene l'immagine grande, senza chiedere niente a nessuno.
///
/// Se il nome non ha quella forma l'indirizzo torna com'era. Un indirizzo
/// riscritto a caso non darebbe un'immagine più grande: darebbe un `403`, cioè
/// nessuna copertina dove ce n'era una piccola.
#[must_use]
pub fn ingrandisci(url: &str) -> String {
    let Some(taglio) = url.rfind('/') else {
        return url.to_owned();
    };
    let (testa, nome) = (
        url.get(..taglio).unwrap_or(""),
        url.get(taglio.saturating_add(1)..).unwrap_or(""),
    );
    let Some((dimensione, estensione)) = nome.rsplit_once('.') else {
        return url.to_owned();
    };
    let Some(lati) = dimensione.strip_suffix("bb") else {
        return url.to_owned();
    };
    let forma_giusta = lati.split_once('x').is_some_and(|(l, a)| {
        !l.is_empty() && !a.is_empty() && l.bytes().chain(a.bytes()).all(|b| b.is_ascii_digit())
    });
    if !forma_giusta {
        return url.to_owned();
    }
    format!("{testa}/{LATO_COPERTINA}x{LATO_COPERTINA}bb.{estensione}")
}

/// I candidati che iTunes propone per questo brano.
///
/// # Errori
///
/// L'errore di rete così com'è: chi chiama lo conta come «una fonte in meno»,
/// non come un guasto. iTunes è il secondo parere, e senza si perde la
/// possibilità di applicare — non la ricerca.
pub fn cerca(
    fornitori: &Fornitori,
    titolo: &str,
    artista: &str,
) -> Result<Vec<Candidate>, AppError> {
    let Some(termine) = termine(titolo, artista) else {
        return Ok(Vec::new());
    };
    let url = format!(
        "https://itunes.apple.com/search?term={}&entity=song&media=music&limit=10",
        percento(&termine)
    );
    let corpo = fornitori.json(
        &fornitori.itunes,
        "itunes-ricerca",
        &normalize_for_match(&termine),
        &url,
        VIVE_RICERCA_MS,
    )?;
    Ok(corpo.as_deref().map(interpreta).unwrap_or_default())
}

/// La stringa da cercare: interprete e titolo, ripuliti dal rumore.
///
/// Un solo campo di testo libero e non due: iTunes non ha una sintassi di
/// campi, e le sue risposte migliorano molto quando l'interprete precede il
/// titolo — che è anche come la gente scriverebbe la ricerca.
#[must_use]
pub fn termine(titolo: &str, artista: &str) -> Option<String> {
    let titolo = titolo_da_cercare(titolo);
    let noto = !artista.trim().is_empty() && artista != aether_domain::album::UNKNOWN_ARTIST;
    let artista = if noto {
        titolo_da_cercare(artista)
    } else {
        String::new()
    };
    let termine = format!("{artista} {titolo}");
    let termine = aether_domain::text::collapse_whitespace(&termine);
    (!termine.is_empty()).then_some(termine)
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn la_copertina_si_chiede_grande() {
        assert_eq!(
            ingrandisci("https://is1.mzstatic.com/image/a/b/100x100bb.jpg"),
            "https://is1.mzstatic.com/image/a/b/600x600bb.jpg"
        );
        assert_eq!(
            ingrandisci("https://is1.mzstatic.com/x/60x60bb.png"),
            "https://is1.mzstatic.com/x/600x600bb.png"
        );
    }

    #[test]
    fn un_indirizzo_di_altra_forma_resta_com_e() {
        // Riscriverlo a caso darebbe un 403, cioè nessuna copertina dove ce
        // n'era una piccola.
        for url in [
            "https://esempio.invalido/copertina.jpg",
            "https://esempio.invalido/100x100.jpg",
            "https://esempio.invalido/axbbb.jpg",
            "senza-barre",
        ] {
            assert_eq!(ingrandisci(url), url, "{url}");
        }
    }

    #[test]
    fn una_risposta_diventa_candidati() {
        let corpo = br#"{"resultCount":1,"results":[{
            "trackName":"Poetica","artistName":"Cesare Cremonini",
            "collectionName":"Possibili scenari","primaryGenreName":"Pop",
            "releaseDate":"2017-11-24T08:00:00Z","trackTimeMillis":297000,
            "artworkUrl100":"https://is1.mzstatic.com/a/100x100bb.jpg"}]}"#;
        let candidati = interpreta(corpo);
        assert_eq!(candidati.len(), 1);
        let primo = candidati.first().expect("un candidato");
        assert_eq!(primo.fonte, Some(FonteMeta::Itunes));
        assert_eq!(primo.genre.as_deref(), Some("Pop"));
        assert_eq!(primo.year, Some(2017));
        assert_eq!(primo.duration_ms, Some(297_000));
        assert_eq!(
            primo.cover_url.as_deref(),
            Some("https://is1.mzstatic.com/a/600x600bb.jpg")
        );
    }

    #[test]
    fn una_voce_senza_titolo_o_interprete_si_salta() {
        // Non è un candidato: senza uno dei due non c'è niente da confrontare.
        let corpo = br#"{"results":[
            {"artistName":"Solo l'artista"},
            {"trackName":"Solo il titolo"},
            {"trackName":"Buono","artistName":"Buono"}
        ]}"#;
        assert_eq!(interpreta(corpo).len(), 1);
    }

    #[test]
    fn un_corpo_storto_non_fa_cadere_niente() {
        assert!(interpreta(b"non json").is_empty());
        assert!(interpreta(b"{}").is_empty());
        assert!(interpreta(br#"{"results":"non un elenco"}"#).is_empty());
    }

    #[test]
    fn il_termine_mette_l_interprete_davanti_e_toglie_il_rumore() {
        assert_eq!(
            termine("Poetica (Official Video)", "Cesare Cremonini"),
            Some("Cesare Cremonini Poetica".to_owned())
        );
        assert_eq!(
            termine("Poetica", aether_domain::album::UNKNOWN_ARTIST),
            Some("Poetica".to_owned())
        );
        assert_eq!(termine("   ", "   "), None);
    }
}
