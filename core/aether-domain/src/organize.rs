//! Riordinare i file in cartelle: la decisione, separata dallo spostamento.
//!
//! # Perché serve
//!
//! Il raggruppamento degli album si fonda su «una cartella = un album» (vedi
//! [`crate::album`]). È l'assunzione giusta per una libreria ordinata, e non
//! dice niente su una libreria che è un unico deposito piatto: lì la cartella è
//! costante, la chiave si riduce al solo titolo, e due dischi omonimi di artisti
//! diversi diventano uno.
//!
//! Riordinare i file rende vera la premessa, invece di aggirarla con una regola
//! che indovina.
//!
//! # Tre garanzie, e perché ognuna
//!
//! **Il piano si vede prima.** Questo modulo non sposta niente: produce un
//! elenco di spostamenti che si può leggere, contare e rifiutare. Un'operazione
//! che tocca migliaia di file dell'utente non deve poter partire da un clic su
//! un pulsante che dice solo «Riordina».
//!
//! **Il nome del file non cambia.** Si cambia solo la cartella. Rinominare
//! aggiungerebbe una classe intera di guasti — troncamenti, caratteri
//! sostituiti, due brani che collassano sullo stesso nome — per un beneficio
//! estetico. La cartella basta a risolvere il problema vero.
//!
//! **L'incertezza si dichiara.** Dove i tag non concordano abbastanza, il piano
//! non indovina: marca il gruppo come da rivedere e lo lascia fuori. Un
//! riordino che sbaglia in silenzio è peggio del disordine, perché il disordine
//! almeno si vede.

use std::collections::HashMap;

use crate::album::{UNKNOWN_ARTIST, normalize_key_text, strip_edition_suffix};
use crate::paths::{PathRules, base_name, is_under, path_key};
use crate::text::is_js_whitespace;

/// Quanto deve essere d'accordo un gruppo perché il piano si fidi.
///
/// Sotto questa quota di consenso sull'artista, il gruppo finisce fra quelli da
/// rivedere. Il valore non è ottimizzato: è scelto perché una maggioranza netta
/// distingue «un album con molti ospiti» — dove l'artista dell'album ricorre —
/// da «brani scollegati che condividono un titolo», dove nessuno ricorre.
pub const CONSENSO_MINIMO: f64 = 0.6;

/// Lunghezza massima di un segmento di percorso.
///
/// Windows accetta 255 caratteri per componente, ma il limite che morde davvero
/// è quello del percorso intero (260 senza percorsi lunghi abilitati). Tagliare
/// corto qui lascia margine alla radice e al nome del file, che non tocchiamo.
pub const MAX_SEGMENTO: usize = 60;

/// I nomi che Windows riserva ai dispositivi, a qualunque estensione.
///
/// Una cartella chiamata `CON` non si può creare, e il guasto arriva come un
/// errore di permessi che manda a cercare nel posto sbagliato. Un artista
/// chiamato `AUX` o un album `NUL` sono rari ma esistono.
const NOMI_RISERVATI: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Rende un testo utilizzabile come nome di cartella su qualunque sistema.
///
/// Si applicano le regole di Windows anche altrove: una libreria deve poter
/// essere copiata su una chiavetta o sincronizzata con un telefono senza che
/// metà dei nomi diventi illegale a destinazione.
#[must_use]
pub fn sanitize_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            // Riservati dal filesystem. Sostituiti e non tolti: togliere
            // `AC/DC` darebbe `ACDC`, e due artisti diversi potrebbero
            // collassare sullo stesso nome.
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => out.push('-'),
            // I caratteri di controllo non hanno una sostituzione sensata.
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }

    // Spazi e punti finali: Windows li toglie da solo alla creazione, e il
    // percorso che poi si prova a riaprire non è quello che si è chiesto.
    let trimmed = out.trim_end_matches(|c: char| c == '.' || is_js_whitespace(c));
    let trimmed = trimmed.trim_start_matches(is_js_whitespace);

    // Il taglio è per CARATTERI e non per byte: tagliare a metà di una lettera
    // accentata produrrebbe un nome non valido in UTF-8.
    let mut result: String = trimmed.chars().take(MAX_SEGMENTO).collect();
    let result_trimmed = result.trim_end_matches(|c: char| c == '.' || is_js_whitespace(c));
    if result_trimmed.len() != result.len() {
        result = result_trimmed.to_owned();
    }

    if result.is_empty() {
        return "_".to_owned();
    }
    // Un nome riservato si disinnesca con un suffisso, non cancellandolo:
    // l'utente deve poter riconoscere la cartella.
    let stem = result.split('.').next().unwrap_or(&result).to_lowercase();
    if NOMI_RISERVATI.contains(&stem.as_str()) {
        result.push('_');
    }
    result
}

/// Un brano, per quel che serve a decidere dove va.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackToOrganize {
    /// L'identificativo della riga, per aggiornarla dopo lo spostamento.
    pub id: i64,
    /// Dove sta ora.
    pub path: String,
    /// Il titolo dell'album dai tag, vuoto se assente.
    pub album: String,
    /// L'artista dell'album, se dichiarato.
    pub album_artist: Option<String>,
    /// L'artista del brano.
    pub artist: Option<String>,
}

impl TrackToOrganize {
    /// L'artista che conta per raggruppare: quello dell'album se c'è.
    fn effective_artist(&self) -> &str {
        self.album_artist
            .as_deref()
            .or(self.artist.as_deref())
            .unwrap_or("")
            .trim_matches(is_js_whitespace)
    }

    fn has_album(&self) -> bool {
        !self.album.trim_matches(is_js_whitespace).is_empty()
    }
}

/// Uno spostamento proposto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    /// La riga da aggiornare.
    pub track_id: i64,
    /// Da dove.
    pub from: String,
    /// A dove.
    pub to: String,
}

/// Perché un brano è stato lasciato dov'è.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// È già al posto giusto.
    AlreadyInPlace,
    /// Sta fuori dalla radice che si sta riordinando.
    OutsideRoot,
    /// I tag del suo gruppo non concordano abbastanza per decidere.
    NeedsReview,
    /// La destinazione è già occupata da un altro brano di questo piano.
    DestinationTaken,
}

impl SkipReason {
    /// Il nome stabile, per l'interfaccia e la diagnostica.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AlreadyInPlace => "alreadyInPlace",
            Self::OutsideRoot => "outsideRoot",
            Self::NeedsReview => "needsReview",
            Self::DestinationTaken => "destinationTaken",
        }
    }
}

/// Un brano lasciato dov'è, col motivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// La riga.
    pub track_id: i64,
    /// Dove sta.
    pub path: String,
    /// Perché non si muove.
    pub reason: SkipReason,
}

/// Un gruppo su cui il piano non se la sente di decidere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeedsReview {
    /// Il titolo dell'album, come sta nei tag.
    pub album: String,
    /// Quanti brani.
    pub tracks: usize,
    /// Gli artisti trovati, dal più frequente.
    pub artists: Vec<String>,
}

/// Cosa il riordino propone di fare.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrganizePlan {
    /// Gli spostamenti, ordinati per destinazione così l'elenco si legge.
    pub moves: Vec<Move>,
    /// I brani lasciati dove sono, col motivo.
    pub skipped: Vec<Skipped>,
    /// I gruppi su cui i tag non bastano a decidere.
    ///
    /// Non è un errore: è la parte del lavoro che richiede una persona. Mostrarla
    /// separata è ciò che permette di sistemare dieci album a mano invece di
    /// accorgersi dopo che millesettecento file sono finiti nel posto sbagliato.
    pub needs_review: Vec<NeedsReview>,
}

impl OrganizePlan {
    /// Il piano non sposta niente?
    #[must_use]
    pub fn is_no_op(&self) -> bool {
        self.moves.is_empty()
    }
}

/// Decide dove va ogni brano.
///
/// Puro: nessun accesso al disco. Chi esegue prende il piano e lo applica.
///
/// # Come raggruppa
///
/// Per **titolo d'album normalizzato**, e non per cartella: in una libreria
/// piatta la cartella è costante e non direbbe nulla. Dentro ogni gruppo si
/// guarda l'accordo sugli artisti: se uno domina — perché è l'artista dell'album
/// e gli altri sono ospiti — quello dà il nome alla cartella. Se invece sono
/// sparsi, il gruppo non è un album: sono brani scollegati che condividono un
/// titolo, e finiscono fra quelli da rivedere.
///
/// I brani senza tag album non si raggruppano fra loro. Nella libreria reale
/// sono cinquantaquattro con trentatré artisti diversi: metterli insieme
/// produrrebbe una scheda d'album da cinquantaquattro brani che non c'entrano
/// niente l'uno con l'altro. Vanno sotto il loro artista, senza cartella d'album.
#[must_use]
pub fn plan_organize(tracks: &[TrackToOrganize], root: &str, rules: PathRules) -> OrganizePlan {
    let mut plan = OrganizePlan::default();

    // ── raggruppamento per titolo, in ordine di comparsa ──
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<&TrackToOrganize>> = HashMap::new();
    let mut loose: Vec<&TrackToOrganize> = Vec::new();

    for track in tracks {
        if !is_under(&track.path, root, rules) {
            plan.skipped.push(Skipped {
                track_id: track.id,
                path: track.path.clone(),
                reason: SkipReason::OutsideRoot,
            });
            continue;
        }
        if !track.has_album() {
            loose.push(track);
            continue;
        }
        let key = normalize_key_text(&strip_edition_suffix(&track.album));
        groups.entry(key.clone()).or_insert_with(|| {
            order.push(key);
            Vec::new()
        });
        // La chiave è appena stata inserita, quindi il gruppo c'è.
        if let Some(group) =
            groups.get_mut(&normalize_key_text(&strip_edition_suffix(&track.album)))
        {
            group.push(track);
        }
    }

    // ── una destinazione per brano ──
    let mut proposals: Vec<(i64, String, String)> = Vec::new();

    for key in &order {
        let Some(group) = groups.get(key) else {
            continue;
        };
        let (winner, share) = dominant_artist(group);

        if share < CONSENSO_MINIMO {
            // Non un album: brani scollegati che condividono un titolo.
            plan.needs_review.push(NeedsReview {
                album: group
                    .first()
                    .map_or_else(String::new, |t| t.album.trim().to_owned()),
                tracks: group.len(),
                artists: artists_by_frequency(group),
            });
            for track in group {
                plan.skipped.push(Skipped {
                    track_id: track.id,
                    path: track.path.clone(),
                    reason: SkipReason::NeedsReview,
                });
            }
            continue;
        }

        let artist_dir = sanitize_component(&winner);
        for track in group {
            let album_dir = sanitize_component(strip_edition_suffix(&track.album).trim());
            let destination = format!(
                "{}/{artist_dir}/{album_dir}/{}",
                root.trim_end_matches(['/', '\\']),
                base_name(&track.path)
            );
            proposals.push((track.id, track.path.clone(), destination));
        }
    }

    // I brani senza album: sotto il loro artista, senza cartella d'album.
    for track in loose {
        let artist = track.effective_artist();
        let artist_dir = sanitize_component(if artist.is_empty() {
            UNKNOWN_ARTIST
        } else {
            artist
        });
        let destination = format!(
            "{}/{artist_dir}/{}",
            root.trim_end_matches(['/', '\\']),
            base_name(&track.path)
        );
        proposals.push((track.id, track.path.clone(), destination));
    }

    // ── collisioni ──
    // Due brani sulla stessa destinazione: succede se due file omonimi finiscono
    // nella stessa cartella d'album. Eseguire lo spostamento ne sovrascriverebbe
    // uno, cioè perderebbe un brano. Si fermano ENTRAMBI: quale dei due sia «il
    // vero» non lo può sapere il piano.
    let mut occupancy: HashMap<String, usize> = HashMap::new();
    for (_, _, destination) in &proposals {
        *occupancy.entry(path_key(destination, rules)).or_insert(0) += 1;
    }

    for (track_id, from, to) in proposals {
        if path_key(&from, rules) == path_key(&to, rules) {
            plan.skipped.push(Skipped {
                track_id,
                path: from,
                reason: SkipReason::AlreadyInPlace,
            });
            continue;
        }
        if occupancy.get(&path_key(&to, rules)).copied().unwrap_or(0) > 1 {
            plan.skipped.push(Skipped {
                track_id,
                path: from,
                reason: SkipReason::DestinationTaken,
            });
            continue;
        }
        plan.moves.push(Move { track_id, from, to });
    }

    plan.moves.sort_by(|a, b| a.to.cmp(&b.to));
    plan.skipped.sort_by_key(|s| s.track_id);
    plan
}

/// L'artista dominante di un gruppo e la sua quota, fra 0 e 1.
fn dominant_artist(group: &[&TrackToOrganize]) -> (String, f64) {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    let mut total = 0usize;
    for track in group {
        let artist = track.effective_artist();
        if !artist.is_empty() {
            *counts.entry(artist).or_insert(0) += 1;
            total += 1;
        }
    }
    if total == 0 {
        return (UNKNOWN_ARTIST.to_owned(), 1.0);
    }
    counts
        .into_iter()
        .max_by_key(|(artist, count)| (*count, std::cmp::Reverse(*artist)))
        .map_or_else(
            || (UNKNOWN_ARTIST.to_owned(), 1.0),
            |(artist, count)| {
                // `as` su usize piccoli: i conteggi sono al più il numero di
                // brani di un album, molto sotto la precisione di f64.
                #[allow(clippy::cast_precision_loss)]
                let share = count as f64 / total as f64;
                (artist.to_owned(), share)
            },
        )
}

fn artists_by_frequency(group: &[&TrackToOrganize]) -> Vec<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for track in group {
        let artist = track.effective_artist();
        if !artist.is_empty() {
            *counts.entry(artist).or_insert(0) += 1;
        }
    }
    let mut sorted: Vec<_> = counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    sorted
        .into_iter()
        .map(|(artist, _)| artist.to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN: PathRules = PathRules {
        case_insensitive: true,
    };

    fn track(id: i64, name: &str, album: &str, artist: &str) -> TrackToOrganize {
        TrackToOrganize {
            id,
            path: format!("C:/Music/{name}"),
            album: album.to_owned(),
            album_artist: None,
            artist: Some(artist.to_owned()),
        }
    }

    #[test]
    fn i_caratteri_illegali_diventano_trattini_non_spariscono() {
        // Toglierli farebbe collassare due artisti diversi sullo stesso nome.
        assert_eq!(sanitize_component("AC/DC"), "AC-DC");
        assert_eq!(sanitize_component("Ke$ha: The Album"), "Ke$ha- The Album");
        assert_ne!(sanitize_component("AC/DC"), sanitize_component("ACDC"));
    }

    #[test]
    fn i_nomi_riservati_di_windows_si_disinnescano() {
        // Una cartella `CON` non si crea, e il guasto arriva come errore di
        // permessi — che manda a cercare nel posto sbagliato.
        assert_eq!(sanitize_component("CON"), "CON_");
        assert_eq!(sanitize_component("aux"), "aux_");
        assert_eq!(sanitize_component("Nul.Album"), "Nul.Album_");
        assert_eq!(sanitize_component("Concrete"), "Concrete");
    }

    #[test]
    fn punti_e_spazi_finali_non_sopravvivono() {
        // Windows li toglie da solo alla creazione, e il percorso che poi si
        // prova a riaprire non e' quello che si e' chiesto.
        assert_eq!(sanitize_component("Album."), "Album");
        assert_eq!(sanitize_component("Album "), "Album");
        assert_eq!(sanitize_component("  Album  "), "Album");
        assert_eq!(sanitize_component(""), "_");
        assert_eq!(sanitize_component("..."), "_");
    }

    #[test]
    fn il_taglio_e_per_caratteri_non_per_byte() {
        // Tagliare a meta' di una lettera accentata darebbe un nome non valido.
        let lungo = "à".repeat(200);
        let tagliato = sanitize_component(&lungo);
        assert_eq!(tagliato.chars().count(), MAX_SEGMENTO);
        assert!(tagliato.is_char_boundary(tagliato.len()));
    }

    #[test]
    fn un_album_con_ospiti_resta_un_album() {
        // Il caso reale: molti artisti diversi, ma uno domina perche' e'
        // l'artista del disco e gli altri sono ospiti.
        let tracks = vec![
            track(1, "a.mp3", "Realtà Aumentata", "Gemitaiz"),
            track(2, "b.mp3", "Realtà Aumentata", "Gemitaiz"),
            track(3, "c.mp3", "Realtà Aumentata", "Gemitaiz"),
            track(4, "d.mp3", "Realtà Aumentata", "Gemitaiz, Ospite"),
        ];
        let plan = plan_organize(&tracks, "C:/Music", WIN);
        assert_eq!(plan.moves.len(), 4);
        assert!(plan.needs_review.is_empty());
        assert!(
            plan.moves
                .iter()
                .all(|m| m.to.contains("/Gemitaiz/Realtà Aumentata/")),
            "tutti sotto lo stesso album: {:?}",
            plan.moves.first()
        );
    }

    #[test]
    fn brani_scollegati_con_lo_stesso_titolo_non_si_indovinano() {
        // Quattro «Greatest Hits» di quattro artisti: nessuno domina, quindi il
        // piano non sceglie. Restano dove sono e finiscono nell'elenco da
        // rivedere, dove una persona puo' sistemarli in dieci minuti.
        let tracks = vec![
            track(1, "a.mp3", "Greatest Hits", "A"),
            track(2, "b.mp3", "Greatest Hits", "B"),
            track(3, "c.mp3", "Greatest Hits", "C"),
            track(4, "d.mp3", "Greatest Hits", "D"),
        ];
        let plan = plan_organize(&tracks, "C:/Music", WIN);
        assert!(plan.moves.is_empty(), "nessuno spostamento indovinato");
        assert_eq!(plan.needs_review.len(), 1);
        assert_eq!(plan.needs_review.first().map(|r| r.tracks), Some(4));
        assert!(
            plan.skipped
                .iter()
                .all(|s| s.reason == SkipReason::NeedsReview)
        );
    }

    #[test]
    fn i_brani_senza_album_non_si_fondono_fra_loro() {
        // Nella libreria reale sono 54 con 33 artisti: insieme darebbero una
        // scheda da 54 brani che non c'entrano niente l'uno con l'altro.
        let tracks = vec![
            track(1, "a.mp3", "", "A"),
            track(2, "b.mp3", "", "B"),
            track(3, "c.mp3", "  ", "A"),
        ];
        let plan = plan_organize(&tracks, "C:/Music", WIN);
        assert_eq!(plan.moves.len(), 3);
        let destinazioni: Vec<_> = plan.moves.iter().map(|m| m.to.as_str()).collect();
        assert!(destinazioni.iter().any(|d| d.contains("/A/a.mp3")));
        assert!(destinazioni.iter().any(|d| d.contains("/B/b.mp3")));
        // Sotto l'artista, senza una cartella d'album inventata.
        assert!(!destinazioni.iter().any(|d| d.contains("sconosciuto/")));
    }

    #[test]
    fn due_brani_sulla_stessa_destinazione_si_fermano_entrambi() {
        // Eseguire ne sovrascriverebbe uno, cioe' perderebbe un brano. Quale sia
        // «il vero» non lo puo' sapere il piano.
        let mut a = track(1, "stesso.mp3", "Album", "Art");
        let mut b = track(2, "stesso.mp3", "Album", "Art");
        a.path = "C:/Music/uno/stesso.mp3".to_owned();
        b.path = "C:/Music/due/stesso.mp3".to_owned();
        let plan = plan_organize(&[a, b], "C:/Music", WIN);
        assert!(plan.moves.is_empty());
        assert_eq!(plan.skipped.len(), 2);
        assert!(
            plan.skipped
                .iter()
                .all(|s| s.reason == SkipReason::DestinationTaken)
        );
    }

    #[test]
    fn chi_e_gia_al_posto_giusto_non_si_muove() {
        let mut t = track(1, "a.mp3", "Album", "Art");
        t.path = "C:/Music/Art/Album/a.mp3".to_owned();
        let plan = plan_organize(&[t], "C:/Music", WIN);
        assert!(plan.is_no_op());
        assert_eq!(
            plan.skipped.first().map(|s| s.reason),
            Some(SkipReason::AlreadyInPlace)
        );
    }

    #[test]
    fn i_brani_fuori_dalla_radice_non_si_toccano() {
        let mut t = track(1, "a.mp3", "Album", "Art");
        t.path = "D:/Altro/a.mp3".to_owned();
        let plan = plan_organize(&[t], "C:/Music", WIN);
        assert!(plan.is_no_op());
        assert_eq!(
            plan.skipped.first().map(|s| s.reason),
            Some(SkipReason::OutsideRoot)
        );
    }

    #[test]
    fn le_edizioni_finiscono_nella_stessa_cartella() {
        let tracks = vec![
            track(1, "a.mp3", "Abbey Road", "Beatles"),
            track(2, "b.mp3", "Abbey Road (Deluxe Edition)", "Beatles"),
        ];
        let plan = plan_organize(&tracks, "C:/Music", WIN);
        assert_eq!(plan.moves.len(), 2);
        assert!(
            plan.moves.iter().all(|m| m.to.contains("/Abbey Road/")),
            "l'edizione non deve creare una seconda cartella"
        );
    }
}
