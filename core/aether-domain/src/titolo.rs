//! Che cosa dicono, di un brano, il titolo di un risultato e il nome di chi lo
//! pubblica.
//!
//! I cataloghi liberi non hanno un campo «interprete» e uno «titolo»: hanno una
//! stringa sola, scritta da chi ha caricato, e un nome di autore o di
//! collezione. Da quella stringa bisogna ricavare le due cose che poi finiscono
//! nei tag del file e nella chiave con cui il brano si riconosce in libreria.
//!
//! Stava in `yt_titolo` e valeva per un servizio solo. Vale identico per l'altro
//! genere di titolo che ci si trova davanti oggi — quello di un *item*
//! dell'Internet Archive, dove `creator` è la band e il titolo del file è tutto
//! quel che resta.
//!
//! # Perché è dominio
//!
//! Perché si può sbagliare **in silenzio**, e sbagliando scrive nei tag
//! l'interprete di qualcun altro. Provarlo deve costare una chiamata di
//! funzione, non una rete.
//!
//! # Perché non basta [`crate::abbinamento::senza_decorazioni`]
//!
//! Perché quella funzione toglie **tutte** le parentesi, e nei titoli veri le
//! parentesi dicono due cose opposte:
//!
//! - `(Official Video)`, `(Lyric Video)`, `(Visualizer)` dicono «questo è un
//!   video», cioè niente sul brano: sono da togliere;
//! - `(Live at Wembley)`, `(Remix)`, `(Acoustic)` dicono **quale
//!   registrazione** è, e sono precisamente ciò su cui
//!   [`crate::scelta::scegli_candidato`] decide se un candidato è il brano
//!   chiesto o un altro. Toglierle qui vorrebbe dire scrivere in libreria un
//!   live col nome della versione in studio.
//!
//! Quindi qui si toglie solo il primo gruppo. `senza_decorazioni` resta dov'è e
//! continua a fare il suo mestiere più avanti, nell'abbinamento.

use crate::abbinamento::primo_artista;
use crate::text::{collapse_whitespace, fold_text};

/// Le parole che, da sole dentro una parentesi, dicono «questo è un video».
///
/// Ognuna è stata vista in un titolo vero. Non sono parole *brutte*: sono parole
/// che non dicono niente sul brano. Il confronto avviene sul gruppo **intero**
/// ripulito, non per sottostringa, ed è la condizione che salva
/// `(Audio Adrenaline)` — un gruppo musicale — dal finire tritato perché
/// contiene `audio`.
const CIARPAME: &[&str] = &[
    "official video",
    "official music video",
    "official audio",
    "official lyric video",
    "official lyrics video",
    "official visualizer",
    "official visual",
    "official video hd",
    "official",
    "video ufficiale",
    "audio ufficiale",
    "lyric video",
    "lyrics video",
    "lyrics",
    "testo",
    "visualizer",
    "visual",
    "music video",
    "videoclip",
    "hd",
    "hq",
    "4k",
    "full hd",
    "audio",
];

/// I trattini che possono separare interprete e titolo.
///
/// Tre e non uno: chi scrive un titolo copia e incolla, e il trattino lungo e
/// quello medio arrivano dai correttori automatici senza che nessuno li abbia
/// digitati.
const TRATTINI: [char; 3] = ['-', '\u{2013}', '\u{2014}'];

/// Da un titolo e da chi lo pubblica, l'interprete e il titolo del brano.
///
/// # Perché il titolo prima dell'autore
///
/// Perché il titolo è scritto da una persona per quel brano preciso; l'autore è
/// chi lo ha caricato, che nel caso migliore è l'interprete e nel caso normale è
/// una raccolta. Su una collezione «Netlabel Sampler» che contiene
/// «Tiësto - Beautiful Places», l'autore direbbe che l'interprete è il sampler.
///
/// L'autore però è **l'unico** indizio che funziona sui caricamenti in cui il
/// titolo è la sola canzone — un concerto dell'Internet Archive, dove i file si
/// chiamano `gd1977-05-08d1t04.flac` e la band sta solo nel `creator` dell'item.
/// Quelli sono anche i più numerosi, quindi il ripiego non è un caso raro.
#[must_use]
pub fn spezza(titolo: &str, autore: Option<&str>) -> (Option<String>, String) {
    let pulito = senza_ciarpame(titolo);

    if let Some((sinistra, destra)) = taglia_al_trattino(&pulito) {
        return (Some(sinistra), destra);
    }

    let dallautore = autore
        .map(str::trim)
        .map(collapse_whitespace)
        .filter(|c| !c.is_empty());
    (dallautore, pulito)
}

/// Il titolo senza i gruppi che dicono soltanto «questo è un video».
///
/// Un gruppo si toglie solo se, ripulito, **è** una delle voci di [`CIARPAME`].
/// `(Live at Wembley)` resta, `(Official Video)` sparisce, e
/// `(Audio Adrenaline)` resta perché il confronto è sul gruppo intero.
#[must_use]
pub fn senza_ciarpame(titolo: &str) -> String {
    let mut fuori = String::with_capacity(titolo.len());
    let mut gruppo = String::new();
    let mut profondita = 0_usize;

    for c in titolo.chars() {
        match c {
            '(' | '[' => {
                profondita = profondita.saturating_add(1);
                if profondita == 1 {
                    gruppo.clear();
                } else {
                    gruppo.push(c);
                }
            }
            ')' | ']' if profondita > 0 => {
                profondita = profondita.saturating_sub(1);
                if profondita == 0 {
                    // Il gruppo è finito: si tiene solo se dice qualcosa.
                    if e_ciarpame(&gruppo) {
                        fuori.push(' ');
                    } else {
                        fuori.push(if c == ')' { '(' } else { '[' });
                        fuori.push_str(&gruppo);
                        fuori.push(c);
                    }
                } else {
                    gruppo.push(c);
                }
            }
            altro if profondita > 0 => gruppo.push(altro),
            altro => fuori.push(altro),
        }
    }

    // Una parentesi che non si chiude: il titolo torna com'era. Un titolo con
    // una parentesi aperta per sbaglio è comunque un titolo, e mangiarne la
    // metà finale sarebbe peggio che lasciarlo intero — è la stessa scelta di
    // `abbinamento::togli_parentesi`.
    if profondita > 0 {
        return collapse_whitespace(titolo);
    }
    collapse_whitespace(&fuori)
}

/// Il gruppo, ripulito, è una delle voci di [`CIARPAME`].
fn e_ciarpame(gruppo: &str) -> bool {
    let piegato = fold_text(gruppo);
    let nudo = piegato.trim_matches(|c: char| !c.is_alphanumeric());
    !nudo.is_empty() && CIARPAME.contains(&nudo)
}

/// Divide al **primo** trattino isolato, se ce n'è uno.
///
/// # Perché il primo e non l'ultimo
///
/// Perché l'interprete sta davanti. «Radiohead - Karma Police - Remastered»
/// tagliato all'ultimo darebbe l'interprete «Radiohead - Karma Police»; tagliato
/// al primo dà «Radiohead» e il titolo «Karma Police - Remastered», e della coda
/// si occupa [`crate::abbinamento::senza_decorazioni`] quando sarà il momento.
///
/// # Perché isolato
///
/// Perché «Jean-Michel Jarre» e «Post-Punk» contengono un trattino che non
/// separa niente. Deve avere uno spazio da tutte e due le parti — la stessa
/// regola, e per la stessa ragione, di `abbinamento::togli_coda_edizione`.
///
/// # Perché due metà non vuote
///
/// Perché «- Song» e «Artist -» non dividono niente: la metà mancante
/// diventerebbe un interprete vuoto o un titolo vuoto, e un brano senza titolo
/// non è un brano.
fn taglia_al_trattino(titolo: &str) -> Option<(String, String)> {
    let caratteri: Vec<(usize, char)> = titolo.char_indices().collect();
    for (n, (posizione, c)) in caratteri.iter().enumerate() {
        if !TRATTINI.contains(c) {
            continue;
        }
        let prima = n
            .checked_sub(1)
            .and_then(|p| caratteri.get(p))
            .map(|(_, c)| *c);
        let dopo = caratteri.get(n.saturating_add(1)).map(|(_, c)| *c);
        if !prima.is_some_and(char::is_whitespace) || !dopo.is_some_and(char::is_whitespace) {
            continue;
        }

        let sinistra = titolo.get(..*posizione)?.trim();
        let destra = titolo.get(posizione.saturating_add(c.len_utf8())..)?.trim();
        if sinistra.is_empty() || destra.is_empty() {
            return None;
        }
        return Some((sinistra.to_owned(), destra.to_owned()));
    }
    None
}

/// L'interprete ricavato somiglia a chi ha pubblicato.
///
/// Serve a chi vuole sapere quanto fidarsi di quel che [`spezza`] ha ricavato:
/// quando le due cose coincidono, il titolo e l'autore dicono la stessa cosa e
/// l'interprete è quasi certamente giusto. Il confronto passa dalla piegatura di
/// [`crate::text`] e da [`primo_artista`], così «Gorillaz, De La Soul» e
/// l'autore «Gorillaz» si riconoscono, e «Bjork» e «Björk» pure.
#[must_use]
pub fn concorda_con_autore(artista: Option<&str>, autore: Option<&str>) -> bool {
    let (Some(artista), Some(autore)) = (artista, autore) else {
        return false;
    };
    let atteso = fold_text(autore.trim());
    !atteso.is_empty() && fold_text(primo_artista(artista)) == atteso
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn il_caso_normale_e_artista_trattino_titolo() {
        assert_eq!(
            spezza(
                "Bebe Rexha & Faithless - New Religion (Official Visual)",
                None
            ),
            (
                Some("Bebe Rexha & Faithless".to_owned()),
                "New Religion".to_owned()
            )
        );
        assert_eq!(
            spezza("Tiësto - Beautiful Places (Official Visualizer)", None),
            (Some("Tiësto".to_owned()), "Beautiful Places".to_owned())
        );
    }

    #[test]
    fn senza_trattino_linterprete_lo_da_lautore() {
        // Il caso dei concerti dell'Internet Archive: il titolo è la sola
        // canzone, la band sta nel `creator` dell'item.
        assert_eq!(
            spezza("Scarlet Begonias", Some("Grateful Dead")),
            (
                Some("Grateful Dead".to_owned()),
                "Scarlet Begonias".to_owned()
            )
        );
    }

    #[test]
    fn il_ciarpame_sparisce_e_la_registrazione_resta() {
        assert_eq!(senza_ciarpame("Song (Official Video)"), "Song");
        assert_eq!(senza_ciarpame("Song [HD]"), "Song");
        // Questa dice *quale* registrazione è: toglierla farebbe entrare in
        // libreria un live col nome della versione in studio.
        assert_eq!(
            senza_ciarpame("Song (Live at Wembley)"),
            "Song (Live at Wembley)"
        );
        assert_eq!(senza_ciarpame("Song (Remix)"), "Song (Remix)");
    }

    #[test]
    fn un_gruppo_musicale_che_contiene_una_parola_di_ciarpame_resta() {
        // Il confronto è sul gruppo intero: «audio» da solo è ciarpame,
        // «Audio Adrenaline» è una band.
        assert_eq!(
            senza_ciarpame("Big House (Audio Adrenaline)"),
            "Big House (Audio Adrenaline)"
        );
    }

    #[test]
    fn una_parentesi_aperta_per_sbaglio_lascia_il_titolo_intero() {
        assert_eq!(
            senza_ciarpame("Song (Official Video"),
            "Song (Official Video"
        );
    }

    #[test]
    fn il_trattino_dentro_un_nome_non_separa() {
        assert_eq!(
            spezza("Jean-Michel Jarre", None),
            (None, "Jean-Michel Jarre".to_owned())
        );
    }

    #[test]
    fn si_taglia_al_primo_trattino_non_allultimo() {
        assert_eq!(
            spezza("Radiohead - Karma Police - Remastered", None),
            (
                Some("Radiohead".to_owned()),
                "Karma Police - Remastered".to_owned()
            )
        );
    }

    #[test]
    fn una_meta_vuota_non_divide() {
        assert_eq!(spezza("- Song", None), (None, "- Song".to_owned()));
        assert_eq!(spezza("Artist -", None), (None, "Artist -".to_owned()));
    }

    #[test]
    fn i_trattini_lunghi_valgono_quanto_quello_corto() {
        assert_eq!(
            spezza("Artist \u{2013} Song", None),
            (Some("Artist".to_owned()), "Song".to_owned())
        );
    }

    #[test]
    fn laccordo_con_lautore_passa_dalla_piegatura() {
        assert!(concorda_con_autore(
            Some("Gorillaz, De La Soul"),
            Some("Gorillaz")
        ));
        assert!(concorda_con_autore(Some("Björk"), Some("Bjork")));
        assert!(!concorda_con_autore(Some("Tiësto"), Some("Chill Vibes")));
        assert!(!concorda_con_autore(None, Some("Gorillaz")));
    }
}
