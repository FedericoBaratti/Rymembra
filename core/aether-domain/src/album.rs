//! Quando due cartelle sono lo stesso disco.
//!
//! L'identità di un album **non** è il testo `(album, album_artist)`. Basandosi
//! su quello, una sola pubblicazione si spezza in più schede a ogni
//! incoerenza — una maiuscola, uno spazio doppio, un `(Deluxe Edition)` su
//! metà dei brani, un `album_artist` che su una traccia riporta l'ospite.
//!
//! La chiave si deriva invece dal **titolo normalizzato più la cartella che
//! contiene il file**, che è come trattano una libreria locale Jellyfin, Plex e
//! Navidrome: i brani della stessa pubblicazione stanno nella stessa cartella,
//! quindi la cartella assorbe le incoerenze dei tag; due dischi diversi che si
//! chiamano uguale stanno in cartelle diverse e restano separati.
//!
//! Sopra questo, gli identificativi autorevoli (MusicBrainz, Spotify) **fondono**
//! gruppi, e non li dividono mai. È la differenza che conta: se solo metà dei
//! brani porta l'identificativo, una regola che divide rimetterebbe il disco in
//! due — una che fonde no.
//!
//! Sbagliare qui non produce un errore: produce una libreria che sembra a posto
//! e ha lo stesso disco in cinque schede. Non si nota su una libreria piccola.

use std::collections::HashMap;

use crate::text::{collapse_whitespace, fold_text, is_js_whitespace};

/// Parole che rendono una coda fra parentesi un'indicazione di edizione o di
/// disco, invece che una parte del titolo.
const EDITION_KEYWORDS: [&str; 21] = [
    "deluxe",
    "remaster",
    "remastered",
    "bonus",
    "expanded",
    "anniversary",
    "edition",
    "version",
    "reissue",
    "mono",
    "stereo",
    "explicit",
    "clean",
    "single",
    "ep",
    "disc",
    "disco",
    "disk",
    "cd",
    "vol",
    "volume",
];

/// Le cifre che un indicatore di disco porta in coda, già piegato in minuscolo.
///
/// `cd 1`, `disc2`, `disco 3`… e `None` per tutto il resto.
fn cifre_del_disco(folded: &str) -> Option<&str> {
    let rest = ["cd", "disc", "disco", "disk"]
        .into_iter()
        // Il più lungo per primo: con `disc` davanti, `disco 1` lascerebbe
        // `o 1` e non verrebbe riconosciuto.
        .filter(|prefix| folded.starts_with(prefix))
        .max_by_key(|prefix| prefix.len())
        .and_then(|prefix| folded.get(prefix.len()..))?;
    let digits = rest.trim_start_matches(is_js_whitespace);
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then_some(digits)
}

/// `cd 1`, `disc2`, `disco 3`… già piegato in minuscolo.
fn looks_like_disc_marker(folded: &str) -> bool {
    cifre_del_disco(folded).is_some()
}

/// Il numero che un indicatore di disco porta con sé: `CD1` → 1, `Disc 2` → 2.
///
/// È la stessa domanda che [`album_folder`] si fa per far collassare una
/// cartella foglia nella superiore, con il numero al posto del sì o del no:
/// [`crate::indizi`] deve poterlo scrivere nel campo «disco» di un brano che i
/// tag non numerano, e un secondo elenco di parole scritto là sarebbe un elenco
/// che un giorno dirà una cosa diversa da questo.
///
/// A differenza di quella domanda, qui il nome arriva **come sta sul disco**: i
/// due punti di chiamata di `indizi` gli passano il contenuto di una parentesi e
/// il nome di una cartella, e piegarlo prima toccherebbe a tutti e due.
///
/// `None` quando non è un indicatore di disco, e anche quando le cifre ci sono
/// ma non stanno in un `u32`: una cartella `CD99999999999` non è l'undicesimo
/// disco di niente, è una cartella che si chiama così.
#[must_use]
pub fn numero_disco(segmento: &str) -> Option<u32> {
    let folded = fold_text(segmento.trim_matches(is_js_whitespace));
    cifre_del_disco(&folded).and_then(|cifre| cifre.parse().ok())
}

/// Il contenuto di una parentesi finale è un'indicazione di edizione?
///
/// Il confronto è **per parola intera**, e non è un dettaglio: cercare la
/// sottostringa `ep` toglierebbe `(Deep Cuts)`, perché «deep» contiene «ep».
fn looks_like_edition(inner: &str) -> bool {
    let folded = fold_text(inner.trim_matches(is_js_whitespace));
    if folded.is_empty() {
        return false;
    }
    if looks_like_disc_marker(&folded) {
        return true;
    }
    folded
        .split(|c: char| !c.is_ascii_lowercase() && !c.is_ascii_digit())
        .any(|token| !token.is_empty() && EDITION_KEYWORDS.contains(&token))
}

/// L'ultima parentesi tonda o quadra in coda: posizione di apertura e contenuto.
///
/// Replica `/[([]([^()[\]]*)[)\]]\s*$/`, comprese le sue asimmetrie: le due
/// classi di caratteri sono indipendenti, quindi l'originale accetta anche una
/// tonda chiusa da una quadra. Il contenuto non può contenere parentesi.
fn trailing_group(s: &str) -> Option<(usize, &str)> {
    let trimmed = s.trim_end_matches(is_js_whitespace);
    let close = trimmed.char_indices().next_back()?;
    if close.1 != ')' && close.1 != ']' {
        return None;
    }
    let before = trimmed.get(..close.0)?;
    for (index, ch) in before.char_indices().rev() {
        match ch {
            '(' | '[' => {
                let inner = before.get(index + ch.len_utf8()..)?;
                return Some((index, inner));
            }
            // Una parentesi chiusa prima dell'apertura: il contenuto ne
            // conterrebbe una, e l'originale non lo permette.
            ')' | ']' => return None,
            _ => {}
        }
    }
    None
}

/// Toglie le code di edizione o di disco: `(Deluxe Edition)`, `[2019 Remaster]`,
/// `(CD1)`, `(Disc 2)`.
///
/// Una parentesi che **non** contiene una parola di edizione resta, perché fa
/// parte del titolo: `Songs (For Drella)` non è `Songs`.
///
/// Non restituisce mai una stringa vuota: se togliere svuoterebbe il titolo, si
/// tiene l'originale. Un album che si chiama solo `(Deluxe Edition)` ha un
/// titolo strano, ma perderlo del tutto sarebbe peggio.
///
/// ```
/// use aether_domain::album::strip_edition_suffix;
/// assert_eq!(strip_edition_suffix("Abbey Road (Deluxe Edition)"), "Abbey Road");
/// assert_eq!(strip_edition_suffix("Songs (For Drella)"), "Songs (For Drella)");
/// // «deep» contiene «ep», ma il confronto è per parola intera.
/// assert_eq!(strip_edition_suffix("Rarities (Deep Cuts)"), "Rarities (Deep Cuts)");
/// ```
#[must_use]
pub fn strip_edition_suffix(album: &str) -> String {
    let original = album.trim_matches(is_js_whitespace);
    let mut current = original;
    // Ripetuto, non fatto una volta: `Album (Bonus Tracks) (Remastered)` ha due
    // code da togliere, e fermarsi alla prima lascerebbe una chiave diversa da
    // quella dei brani dello stesso disco taggati con una sola coda.
    while let Some((at, inner)) = trailing_group(current) {
        if !looks_like_edition(inner) {
            break;
        }
        let Some(head) = current.get(..at) else { break };
        current = head.trim_matches(is_js_whitespace);
    }
    if current.is_empty() {
        original.to_owned()
    } else {
        current.to_owned()
    }
}

/// Normalizza un testo per usarlo dentro una chiave d'album.
///
/// Diversa da [`crate::keys::normalize_key`], che serve alle chiavi di
/// sincronizzazione, in due punti: qui i puntini di sospensione diventano **tre
/// punti** invece di uno, e la punteggiatura **non** viene rimossa. Sono chiavi
/// per scopi diversi e non vanno unificate «per pulizia»: quella di
/// sincronizzazione deve sopravvivere a taggature molto diverse fra dispositivi,
/// questa deve solo raggruppare file che stanno nella stessa cartella.
#[must_use]
pub fn normalize_key_text(input: &str) -> String {
    let mut replaced = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' | '\u{2032}' | '\u{2035}' => {
                replaced.push('\'');
            }
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' | '\u{2033}' | '\u{2036}' => {
                replaced.push('"');
            }
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
            | '\u{2212}' => replaced.push('-'),
            '\u{2026}' => replaced.push_str("..."),
            _ => replaced.push(c),
        }
    }
    collapse_whitespace(&fold_text(&replaced))
}

/// La cartella che contiene il brano, non solo il nome dell'ultima.
///
/// Il percorso intero e non il nome finale, perché due artisti possono avere
/// entrambi una cartella `Greatest Hits`. E una cartella finale che è un
/// indicatore di disco (`CD1`, `Disc 2`, `Disco 1`) si risolve nella cartella
/// superiore, così un album su più dischi resta una pubblicazione sola.
#[must_use]
pub fn album_folder(path: &str) -> String {
    let mut parts: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    parts.pop(); // via il nome del file
    let collapse = parts.len() >= 2
        && parts
            .last()
            .is_some_and(|leaf| looks_like_disc_marker(&fold_text(leaf)));
    if collapse {
        parts.pop();
    }
    parts.join("/")
}

/// La chiave di raggruppamento di un brano: titolo normalizzato più cartella.
#[must_use]
pub fn album_group_key(album: &str, path: &str) -> String {
    format!(
        "{} {}",
        normalize_key_text(&strip_edition_suffix(album)),
        normalize_key_text(&album_folder(path))
    )
}

/// Una riga di brano, per quel che serve ad aggregare un album.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AlbumMember {
    /// La chiave di raggruppamento del brano.
    pub album_key: String,
    /// Il titolo dell'album come sta nei tag.
    pub album: String,
    /// L'artista dell'album, se dichiarato.
    pub album_artist: Option<String>,
    /// L'artista del brano.
    pub artist: Option<String>,
    /// L'anno.
    pub year: Option<i32>,
    /// Il genere dichiarato dal brano.
    pub genre: Option<String>,
    /// L'impronta della copertina.
    pub cover_art_hash: Option<String>,
    /// La provenienza della copertina: `tag`, `provider`, `spotify`, `caa`.
    pub cover_source: Option<String>,
    /// Larghezza della copertina, in pixel.
    pub cover_width: Option<u32>,
    /// Altezza della copertina, in pixel.
    pub cover_height: Option<u32>,
    /// Identificativo MusicBrainz del gruppo di pubblicazione.
    pub mb_release_group_id: Option<String>,
    /// Identificativo MusicBrainz della pubblicazione.
    pub mb_release_id: Option<String>,
    /// Identificativo Spotify dell'album.
    pub spotify_album_id: Option<String>,
}

/// Una riga di album, ricostruita dai brani che la compongono.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumRow {
    /// La chiave canonica del gruppo.
    pub album_key: String,
    /// Il titolo da mostrare.
    pub title: String,
    /// L'artista da mostrare.
    pub artist: String,
    /// L'anno.
    pub year: Option<i32>,
    /// Il genere dominante fra i brani.
    pub genre: Option<String>,
    /// Quanti brani.
    pub total_tracks: usize,
    /// La copertina scelta.
    pub cover_art_hash: Option<String>,
    /// L'identificativo MusicBrainz, pubblicazione o gruppo.
    pub mb_album_id: Option<String>,
    /// L'identificativo Spotify.
    pub spotify_id: Option<String>,
}

/// Il ripiego quando il titolo manca del tutto.
pub const UNKNOWN_ALBUM: &str = "Album sconosciuto";
/// Il ripiego quando l'artista manca del tutto.
pub const UNKNOWN_ARTIST: &str = "Artista sconosciuto";

fn effective_artist(member: &AlbumMember) -> &str {
    member
        .album_artist
        .as_deref()
        .or(member.artist.as_deref())
        .unwrap_or("")
        .trim_matches(is_js_whitespace)
}

/// Il valore non vuoto più frequente, a pari merito il minore alfabeticamente.
fn dominant<'a>(
    members: &'a [AlbumMember],
    get: impl Fn(&'a AlbumMember) -> Option<&'a str>,
) -> Option<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for member in members {
        let value = get(member).unwrap_or("").trim_matches(is_js_whitespace);
        if !value.is_empty() {
            *counts.entry(value).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        // `max_by_key` restituisce l'ULTIMO massimo, quindi il criterio di
        // pareggio va invertito: `Reverse` sul valore fa vincere il minore.
        .max_by_key(|(value, count)| (*count, std::cmp::Reverse(*value)))
        .map(|(value, _)| value.to_owned())
}

/// L'artista canonico di un gruppo: il più frequente, a pari merito il più
/// corto — che è ciò che toglie un `feat. …` — poi alfabetico.
#[must_use]
pub fn pick_canonical_artist(members: &[AlbumMember]) -> String {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for member in members {
        let value = effective_artist(member);
        if !value.is_empty() {
            *counts.entry(value).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|(value, count)| {
            (
                *count,
                std::cmp::Reverse(value.len()),
                std::cmp::Reverse(*value),
            )
        })
        .map_or_else(String::new, |(value, _)| value.to_owned())
}

/// Quanto vale la provenienza di una copertina.
///
/// Un'immagine dentro i tag del file (o scelta a mano) batte quella di un
/// servizio, che batte una trovata per somiglianza dal Cover Art Archive.
fn cover_rank(source: Option<&str>) -> u8 {
    match source {
        Some("tag") => 3,
        Some("provider" | "spotify") => 2,
        Some("caa") => 1,
        _ => 0,
    }
}

/// La copertina dell'album: per ogni impronta distinta si guarda provenienza,
/// quanti brani la usano e l'area in pixel, in quest'ordine.
///
/// Ricalcolabile e deterministica. Sostituisce la vecchia scelta «l'impronta del
/// primo membro, e da lì in poi non cambia più», che lasciava a un brano
/// anomalo il potere di dettare la copertina di tutto l'album.
#[must_use]
pub fn pick_album_cover(members: &[AlbumMember]) -> Option<String> {
    struct Candidate {
        rank: u8,
        count: usize,
        pixels: u64,
    }
    let mut by_hash: HashMap<&str, Candidate> = HashMap::new();
    for member in members {
        let Some(hash) = member.cover_art_hash.as_deref() else {
            continue;
        };
        let rank = cover_rank(member.cover_source.as_deref());
        let pixels = u64::from(member.cover_width.unwrap_or(0))
            * u64::from(member.cover_height.unwrap_or(0));
        let entry = by_hash.entry(hash).or_insert(Candidate {
            rank: 0,
            count: 0,
            pixels: 0,
        });
        entry.count += 1;
        entry.rank = entry.rank.max(rank);
        entry.pixels = entry.pixels.max(pixels);
    }
    by_hash
        .into_iter()
        .max_by_key(|(hash, c)| (c.rank, c.count, c.pixels, std::cmp::Reverse(*hash)))
        .map(|(hash, _)| hash.to_owned())
}

fn aggregate_group(album_key: String, members: &[AlbumMember]) -> AlbumRow {
    // Il titolo più frequente; a pari merito il più corto, poi alfabetico.
    let mut title_counts: HashMap<&str, usize> = HashMap::new();
    for member in members {
        let title = member.album.trim_matches(is_js_whitespace);
        if !title.is_empty() {
            *title_counts.entry(title).or_insert(0) += 1;
        }
    }
    let title = title_counts
        .into_iter()
        .max_by_key(|(title, count)| {
            (
                *count,
                std::cmp::Reverse(title.len()),
                std::cmp::Reverse(*title),
            )
        })
        .map_or_else(|| UNKNOWN_ALBUM.to_owned(), |(title, _)| title.to_owned());

    let artist = pick_canonical_artist(members);

    AlbumRow {
        album_key,
        title,
        artist: if artist.is_empty() {
            UNKNOWN_ARTIST.to_owned()
        } else {
            artist
        },
        year: members.iter().filter_map(|m| m.year).max(),
        // Il genere dominante e non quello del primo brano: su un disco taggato
        // a mano capita che una traccia porti «Rock» e le altre «Alternative
        // Rock», e la scheda dell'album mostrerebbe il genere di quella sola.
        genre: dominant(members, |m| m.genre.as_deref()),
        total_tracks: members.len(),
        cover_art_hash: pick_album_cover(members),
        // L'identificativo della pubblicazione quando c'è, altrimenti quello del
        // gruppo: la colonna resta popolata in entrambi i casi.
        mb_album_id: dominant(members, |m| m.mb_release_id.as_deref())
            .or_else(|| dominant(members, |m| m.mb_release_group_id.as_deref())),
        spotify_id: dominant(members, |m| m.spotify_album_id.as_deref()),
    }
}

/// Il risultato della ricostruzione degli album.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumGroups {
    /// Un album per super-gruppo, ordinati per chiave.
    pub albums: Vec<AlbumRow>,
    /// Da ogni chiave di brano alla chiave canonica sotto cui è finito.
    ///
    /// Serve a riscrivere `tracks.album_key` sul valore canonico, così la
    /// giunzione brani⋈album e il conteggio degli album per artista restano
    /// coerenti.
    pub remap: Vec<(String, String)>,
}

/// Ricostruisce gli album dai brani, fondendo i gruppi che condividono un
/// identificativo autorevole.
///
/// Tre passi: raggruppare per chiave di brano; unire i gruppi che condividono un
/// identificativo — gruppo di pubblicazione MusicBrainz, poi pubblicazione, poi
/// album Spotify; dare a ogni super-gruppo una chiave canonica, che è
/// l'identificativo condiviso quando c'è (`mbrg:`, `mbr:`, `sp:`) e altrimenti
/// la più piccola fra le chiavi di base.
///
/// **Gli identificativi uniscono e non dividono mai.** È la proprietà che rende
/// innocua una taggatura parziale: se metà dei brani porta l'identificativo e
/// metà no, il disco resta uno.
#[must_use]
pub fn build_album_groups(members: &[AlbumMember]) -> AlbumGroups {
    // Chiavi di base in ordine di comparsa: l'ordine non cambia il risultato —
    // tutte le riduzioni qui sotto sono indipendenti dall'ordine — ma lo rende
    // riproducibile, e un piano riproducibile si può confrontare.
    let mut base_keys: Vec<&str> = Vec::new();
    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut groups: Vec<Vec<&AlbumMember>> = Vec::new();
    for member in members {
        let position = *index.entry(&member.album_key).or_insert_with(|| {
            base_keys.push(&member.album_key);
            groups.push(Vec::new());
            base_keys.len() - 1
        });
        if let Some(group) = groups.get_mut(position) {
            group.push(member);
        }
    }

    // ── insieme-unione sulle chiavi di base ──
    let mut parent: Vec<usize> = (0..base_keys.len()).collect();
    fn find(parent: &mut [usize], mut node: usize) -> usize {
        while parent.get(node).copied().unwrap_or(node) != node {
            let grandparent = parent
                .get(parent.get(node).copied().unwrap_or(node))
                .copied()
                .unwrap_or(node);
            if let Some(slot) = parent.get_mut(node) {
                *slot = grandparent;
            }
            node = grandparent;
        }
        node
    }
    let union = |parent: &mut Vec<usize>, a: usize, b: usize, keys: &[&str]| {
        let (ra, rb) = (find(parent, a), find(parent, b));
        if ra == rb {
            return;
        }
        // Si adotta come radice quella con la chiave minore: la scelta non
        // cambia la partizione, ma la rende indipendente dall'ordine di arrivo.
        let (keep, drop) = match (keys.get(ra), keys.get(rb)) {
            (Some(ka), Some(kb)) if kb < ka => (rb, ra),
            _ => (ra, rb),
        };
        if let Some(slot) = parent.get_mut(drop) {
            *slot = keep;
        }
    };

    let link_by = |parent: &mut Vec<usize>, get: fn(&AlbumMember) -> Option<&str>| {
        let mut first_for_id: HashMap<String, usize> = HashMap::new();
        for (position, group) in groups.iter().enumerate() {
            let owned: Vec<AlbumMember> = group.iter().map(|m| (*m).clone()).collect();
            let Some(id) = dominant(&owned, |m| get(m)) else {
                continue;
            };
            match first_for_id.get(&id) {
                Some(&seen) => union(parent, seen, position, &base_keys),
                None => {
                    first_for_id.insert(id, position);
                }
            }
        }
    };
    link_by(&mut parent, |m| m.mb_release_group_id.as_deref());
    link_by(&mut parent, |m| m.mb_release_id.as_deref());
    link_by(&mut parent, |m| m.spotify_album_id.as_deref());

    // ── raccolta dei super-gruppi ──
    let mut members_by_root: HashMap<usize, Vec<AlbumMember>> = HashMap::new();
    let mut keys_by_root: HashMap<usize, Vec<&str>> = HashMap::new();
    for (position, group) in groups.iter().enumerate() {
        let root = find(&mut parent, position);
        let entry = members_by_root.entry(root).or_default();
        entry.extend(group.iter().map(|m| (*m).clone()));
        if let Some(key) = base_keys.get(position) {
            keys_by_root.entry(root).or_default().push(key);
        }
    }

    let mut albums = Vec::with_capacity(members_by_root.len());
    let mut remap: Vec<(String, String)> = Vec::new();
    for (root, group_members) in members_by_root {
        let release_group = dominant(&group_members, |m| m.mb_release_group_id.as_deref());
        let release = dominant(&group_members, |m| m.mb_release_id.as_deref());
        let spotify = dominant(&group_members, |m| m.spotify_album_id.as_deref());
        let mut member_keys = keys_by_root.remove(&root).unwrap_or_default();
        member_keys.sort_unstable();

        let canonical = match (release_group, release, spotify) {
            (Some(id), _, _) => format!("mbrg:{id}"),
            (None, Some(id), _) => format!("mbr:{id}"),
            (None, None, Some(id)) => format!("sp:{id}"),
            (None, None, None) => member_keys
                .first()
                .map_or_else(String::new, |k| (*k).to_owned()),
        };

        albums.push(aggregate_group(canonical.clone(), &group_members));
        for key in member_keys {
            remap.push((key.to_owned(), canonical.clone()));
        }
    }

    albums.sort_by(|a, b| a.album_key.cmp(&b.album_key));
    remap.sort();
    AlbumGroups { albums, remap }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(album_key: &str, album: &str) -> AlbumMember {
        AlbumMember {
            album_key: album_key.to_owned(),
            album: album.to_owned(),
            artist: Some("Art".to_owned()),
            ..AlbumMember::default()
        }
    }

    #[test]
    fn la_trappola_di_deep_cuts() {
        // Un confronto per sottostringa toglierebbe «(Deep Cuts)» per via di
        // «ep» dentro «deep», e due album diversi si fonderebbero.
        assert_eq!(
            strip_edition_suffix("Rarities (Deep Cuts)"),
            "Rarities (Deep Cuts)"
        );
        assert_eq!(strip_edition_suffix("Deep Cuts"), "Deep Cuts");
        assert_eq!(strip_edition_suffix("Rarities (EP)"), "Rarities");
    }

    #[test]
    fn le_parentesi_che_fanno_parte_del_titolo_restano() {
        assert_eq!(
            strip_edition_suffix("Songs (For Drella)"),
            "Songs (For Drella)"
        );
        assert_eq!(
            strip_edition_suffix("Album (Live) (Deluxe)"),
            "Album (Live)"
        );
    }

    #[test]
    fn togliere_non_puo_svuotare_il_titolo() {
        assert_eq!(strip_edition_suffix("(Deluxe Edition)"), "(Deluxe Edition)");
    }

    #[test]
    fn i_dischi_multipli_sono_una_pubblicazione_sola() {
        let a = album_group_key("Abbey Road", "C:/Music/Beatles/Abbey Road/01.mp3");
        let b = album_group_key("Abbey Road", "C:/Music/Beatles/Abbey Road/CD1/05.mp3");
        let c = album_group_key(
            "Abbey Road (Deluxe Edition)",
            "C:/Music/Beatles/Abbey Road/02.mp3",
        );
        assert_eq!(a, b);
        assert_eq!(a, c);
    }

    #[test]
    fn due_dischi_omonimi_restano_separati() {
        // Senza la cartella nella chiave, queste due raccolte diventerebbero una.
        assert_ne!(
            album_group_key("Greatest Hits", "C:/Music/A/Greatest Hits/01.mp3"),
            album_group_key("Greatest Hits", "C:/Music/B/Greatest Hits/01.mp3")
        );
    }

    #[test]
    fn una_taggatura_parziale_non_spezza_un_album() {
        // Il caso che rende «unire» diverso da «dividere»: un solo brano su tre
        // porta l'identificativo, e il disco deve restare uno.
        let rows = vec![
            AlbumMember {
                mb_release_group_id: Some("RG1".to_owned()),
                ..member("k1", "A")
            },
            member("k1", "A"),
            AlbumMember {
                mb_release_group_id: Some("RG1".to_owned()),
                ..member("k2", "A")
            },
        ];
        let out = build_album_groups(&rows);
        assert_eq!(out.albums.len(), 1);
        assert_eq!(
            out.albums.first().map(|a| a.album_key.as_str()),
            Some("mbrg:RG1")
        );
        assert_eq!(out.albums.first().map(|a| a.total_tracks), Some(3));
    }

    #[test]
    fn la_fusione_e_transitiva() {
        // k1–k2 legati dal gruppo di pubblicazione, k2–k3 da Spotify: tutti e
        // tre finiscono insieme.
        let rows = vec![
            AlbumMember {
                mb_release_group_id: Some("RG1".to_owned()),
                ..member("k1", "A")
            },
            AlbumMember {
                mb_release_group_id: Some("RG1".to_owned()),
                spotify_album_id: Some("SP1".to_owned()),
                ..member("k2", "A")
            },
            AlbumMember {
                spotify_album_id: Some("SP1".to_owned()),
                ..member("k3", "A")
            },
        ];
        let out = build_album_groups(&rows);
        assert_eq!(out.albums.len(), 1);
        assert_eq!(out.remap.len(), 3);
    }

    #[test]
    fn la_copertina_non_la_detta_un_brano_anomalo() {
        // Provenienza prima di tutto: una copertina dentro i tag batte una
        // enorme trovata per somiglianza.
        let with_cover = |hash: &str, source: &str, w: u32, h: u32| AlbumMember {
            cover_art_hash: Some(hash.to_owned()),
            cover_source: Some(source.to_owned()),
            cover_width: Some(w),
            cover_height: Some(h),
            ..member("k", "A")
        };
        let scelta = pick_album_cover(&[
            with_cover("h1", "tag", 300, 300),
            with_cover("h2", "caa", 1000, 1000),
        ]);
        assert_eq!(scelta.as_deref(), Some("h1"));
    }

    #[test]
    fn il_genere_dell_album_e_quello_dominante() {
        // Una traccia taggata a mano diversamente dalle altre non deve dettare
        // il genere della scheda.
        let rows = vec![
            AlbumMember {
                genre: Some("Alternative Rock".to_owned()),
                ..member("k", "A")
            },
            AlbumMember {
                genre: Some("Alternative Rock".to_owned()),
                ..member("k", "A")
            },
            AlbumMember {
                genre: Some("Rock".to_owned()),
                ..member("k", "A")
            },
        ];
        let out = build_album_groups(&rows);
        assert_eq!(
            out.albums.first().and_then(|a| a.genre.as_deref()),
            Some("Alternative Rock")
        );
    }

    #[test]
    fn l_artista_canonico_perde_il_feat() {
        let rows = vec![
            AlbumMember {
                artist: Some("A feat. B".to_owned()),
                ..member("k", "X")
            },
            AlbumMember {
                artist: Some("A".to_owned()),
                ..member("k", "X")
            },
        ];
        assert_eq!(pick_canonical_artist(&rows), "A");
    }
}
