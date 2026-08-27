//! Che cosa nomina il link che qualcuno ha incollato.
//!
//! Si legge senza rete, perché è una decisione su una stringa e le decisioni su
//! stringhe si provano senza rete. Il crate che parla col mondo la usa; se
//! l'indirizzo non si riconosce, nessuna richiesta parte.
//!
//! # Perché non si accetta qualunque cosa
//!
//! Perché «non riconosco questo link» è una risposta utile, mentre «l'ho chiesto
//! e ha detto di no» non lo è: la seconda manda a controllare la connessione per
//! un indirizzo di un servizio che non c'entra niente. E soprattutto: un link
//! che non si riconosce è un link a cui **non si va**, ed è la garanzia che
//! Aether non vada mai a bussare dove non è invitato.

use aether_domain::esterno::{Fonte, GenereContenuto};

/// Che cosa un indirizzo nomina, e presso chi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Riferimento {
    /// Quale catalogo.
    pub fonte: Fonte,
    /// Che genere di cosa.
    pub genere: GenereContenuto,
    /// L'identificativo presso quel catalogo.
    pub id: String,
}

impl Riferimento {
    /// L'indirizzo pubblico da cui questo riferimento si è ricavato.
    ///
    /// Ricostruito e non conservato: quel che si è incollato può avere
    /// parametri di tracciamento, un dominio abbreviato, un `?si=` di troppo.
    /// Quel che si mostra e si salva dev'essere la forma canonica.
    #[must_use]
    pub fn url_pubblico(&self) -> String {
        match self.fonte {
            Fonte::InternetArchive => format!("https://archive.org/details/{}", self.id),
            Fonte::Jamendo => format!(
                "https://www.jamendo.com/{}/{}",
                self.genere.path_word(),
                self.id
            ),
            Fonte::Audius => format!("https://audius.co/{}", self.id),
            Fonte::ArchivioSpotify | Fonte::FilePlaylist => self.id.clone(),
        }
    }
}

/// Riconosce un indirizzo di un catalogo che Aether sa leggere.
///
/// `None` per tutto il resto, compresi i servizi che Aether non interroga: non
/// è una lacuna da colmare, è il confine.
#[must_use]
pub fn riconosci(input: &str) -> Option<Riferimento> {
    let pulito = input.trim();
    if pulito.is_empty() {
        return None;
    }
    let (dominio, percorso) = spezza_indirizzo(pulito)?;

    if dominio.ends_with("archive.org") {
        return archivio_org(&percorso);
    }
    if dominio.ends_with("jamendo.com") {
        return jamendo(&percorso);
    }
    if dominio.ends_with("audius.co") || dominio.ends_with("audius.org") {
        return audius(&percorso);
    }
    None
}

/// Dominio e percorso, senza schema, senza `www.`, senza query.
fn spezza_indirizzo(input: &str) -> Option<(String, Vec<String>)> {
    let senza_schema = input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))
        .unwrap_or(input);
    let senza_query = senza_schema.split(['?', '#']).next()?;
    let (host, resto) = senza_query.split_once('/').unwrap_or((senza_query, ""));
    let dominio = host.trim_start_matches("www.").to_ascii_lowercase();
    if dominio.is_empty() || !dominio.contains('.') {
        return None;
    }
    let pezzi: Vec<String> = resto
        .split('/')
        .filter(|p| !p.is_empty())
        .map(decodifica)
        .collect();
    Some((dominio, pezzi))
}

/// `archive.org/details/<id>` e `archive.org/download/<id>/<file>`.
///
/// Il secondo si riduce al primo di proposito: chi incolla il link diretto a un
/// file vuole quel concerto, non quel file soltanto — e se volesse solo quello,
/// prenderlo insieme agli altri non gli toglie niente.
fn archivio_org(percorso: &[String]) -> Option<Riferimento> {
    let primo = percorso.first()?.as_str();
    if !matches!(primo, "details" | "download" | "metadata" | "embed") {
        return None;
    }
    let id = percorso.get(1)?.trim();
    (!id.is_empty()).then(|| Riferimento {
        fonte: Fonte::InternetArchive,
        genere: GenereContenuto::Collezione,
        id: id.to_owned(),
    })
}

/// `jamendo.com/track/<id>[/<slug>]`, `/album/<id>`, `/artist/<id>`.
fn jamendo(percorso: &[String]) -> Option<Riferimento> {
    // Le pagine di Jamendo hanno una lingua davanti su certi mercati:
    // `jamendo.com/it/track/…`. Saltarla quando c'è costa due righe; non
    // saltarla vuol dire non riconoscere i link di metà Europa.
    let inizio = percorso
        .first()
        .filter(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic()))
        .map_or(0, |_| 1);
    let genere = match percorso.get(inizio)?.as_str() {
        "track" => GenereContenuto::Track,
        "album" => GenereContenuto::Album,
        "artist" => GenereContenuto::Artist,
        "playlist" => GenereContenuto::Playlist,
        _ => return None,
    };
    let id = percorso.get(inizio.saturating_add(1))?.trim();
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_digit())).then(|| Riferimento {
        fonte: Fonte::Jamendo,
        genere,
        id: id.to_owned(),
    })
}

/// `audius.co/<handle>/<slug>` per un brano, `audius.co/<handle>` per un artista.
fn audius(percorso: &[String]) -> Option<Riferimento> {
    let handle = percorso.first()?.trim();
    if handle.is_empty() {
        return None;
    }
    match percorso.get(1).map(String::as_str) {
        Some("album") | Some("playlist") => {
            let coda = percorso.get(2)?.trim();
            (!coda.is_empty()).then(|| Riferimento {
                fonte: Fonte::Audius,
                genere: GenereContenuto::Playlist,
                id: format!("{handle}/{}/{coda}", percorso.get(1).map_or("", |s| s)),
            })
        }
        Some(slug) if !slug.is_empty() => Some(Riferimento {
            fonte: Fonte::Audius,
            genere: GenereContenuto::Track,
            id: format!("{handle}/{slug}"),
        }),
        _ => Some(Riferimento {
            fonte: Fonte::Audius,
            genere: GenereContenuto::Artist,
            id: handle.to_owned(),
        }),
    }
}

/// La decodifica percentuale, per gli identificativi con caratteri speciali.
fn decodifica(pezzo: &str) -> String {
    let byte = pezzo.as_bytes();
    let mut fuori: Vec<u8> = Vec::with_capacity(byte.len());
    let mut n = 0_usize;
    while let Some(&b) = byte.get(n) {
        if b == b'%'
            && let (Some(&alto), Some(&basso)) = (byte.get(n + 1), byte.get(n + 2))
            && let (Some(a), Some(z)) = (cifra(alto), cifra(basso))
        {
            fuori.push(a.saturating_mul(16).saturating_add(z));
            n = n.saturating_add(3);
            continue;
        }
        fuori.push(b);
        n = n.saturating_add(1);
    }
    String::from_utf8(fuori).unwrap_or_else(|_| pezzo.to_owned())
}

/// Una cifra esadecimale, o niente.
const fn cifra(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn un_concerto_dellarchive_si_riconosce_in_tutte_le_sue_forme() {
        let atteso = Riferimento {
            fonte: Fonte::InternetArchive,
            genere: GenereContenuto::Collezione,
            id: "gd1977-05-08.sbd.hicks.4982.sbeok.shnf".to_owned(),
        };
        for url in [
            "https://archive.org/details/gd1977-05-08.sbd.hicks.4982.sbeok.shnf",
            "http://www.archive.org/details/gd1977-05-08.sbd.hicks.4982.sbeok.shnf/",
            "archive.org/download/gd1977-05-08.sbd.hicks.4982.sbeok.shnf/gd77t01.flac",
            "https://archive.org/metadata/gd1977-05-08.sbd.hicks.4982.sbeok.shnf?x=1",
        ] {
            assert_eq!(riconosci(url).as_ref(), Some(&atteso), "su «{url}»");
        }
    }

    #[test]
    fn lindirizzo_pubblico_e_la_forma_canonica() {
        let r = riconosci("archive.org/download/gd77/t01.flac?utm_source=x")
            .expect("è un link dell'Archive");
        assert_eq!(r.url_pubblico(), "https://archive.org/details/gd77");
    }

    #[test]
    fn quel_che_non_si_riconosce_non_si_va_a_chiedere() {
        // Il confine, e la ragione per cui è scritto: un link che non si
        // riconosce è un link a cui non si bussa.
        for url in [
            "https://www.youtube.com/watch?v=abcdefghijk",
            "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT",
            "https://music.apple.com/it/album/x/123",
            "https://archive.org/",
            "non un indirizzo",
            "",
            "   ",
        ] {
            assert_eq!(riconosci(url), None, "su «{url}»");
        }
    }

    #[test]
    fn jamendo_si_riconosce_anche_con_la_lingua_davanti() {
        let atteso = Riferimento {
            fonte: Fonte::Jamendo,
            genere: GenereContenuto::Track,
            id: "1884527".to_owned(),
        };
        assert_eq!(
            riconosci("https://www.jamendo.com/track/1884527/mon-ami").as_ref(),
            Some(&atteso)
        );
        assert_eq!(
            riconosci("https://www.jamendo.com/it/track/1884527/mon-ami").as_ref(),
            Some(&atteso)
        );
        // Un identificativo che non è un numero non è un identificativo Jamendo.
        assert_eq!(riconosci("https://www.jamendo.com/track/mon-ami"), None);
    }

    #[test]
    fn audius_distingue_un_brano_da_un_artista() {
        assert_eq!(
            riconosci("https://audius.co/deadmau5").map(|r| r.genere),
            Some(GenereContenuto::Artist)
        );
        assert_eq!(
            riconosci("https://audius.co/deadmau5/strobe-123").map(|r| r.genere),
            Some(GenereContenuto::Track)
        );
        assert_eq!(
            riconosci("https://audius.co/deadmau5/album/random-album-title-99").map(|r| r.genere),
            Some(GenereContenuto::Playlist)
        );
    }

    #[test]
    fn gli_identificativi_codificati_si_leggono() {
        assert_eq!(decodifica("gd77%20live"), "gd77 live");
        assert_eq!(decodifica("Bj%C3%B6rk"), "Björk");
        // Una percentuale che non introduce niente resta com'è.
        assert_eq!(decodifica("100%"), "100%");
    }
}
