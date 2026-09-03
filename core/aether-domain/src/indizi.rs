//! Quel che dicono la cartella e il nome del file, quando i tag non dicono niente.
//!
//! # Perché la cartella è una fonte, e non un ripiego qualsiasi
//!
//! Perché è **la stessa** che [`crate::organize`] scrive. Il riordino di Aether
//! mette i file in `Artista/Album`, e mezzo mondo — Picard, EAC, dBpoweramp,
//! foobar2000 — usa la stessa forma con l'anno davanti al titolo del disco.
//! Un file rippato senza tag sta quasi sempre dentro
//! `Pink Floyd/1973 - The Dark Side of the Moon/01 - Speak to Me.flac`, e lì
//! dentro c'è tutto quello che al brano manca.
//!
//! Fin qui la scansione guardava solo il **nome** del file, e solo per il titolo
//! ([`crate::paths::file_stem`], che è il ripiego di `read_track`). L'artista e
//! l'album diventavano «sconosciuto» con la risposta scritta due cartelle sopra.
//!
//! # Le due regole che tengono in piedi il resto
//!
//! **Un indizio viene solo da dentro una radice sorvegliata.** La radice è dove
//! l'utente tiene la musica, non un artista: se le radici sono `D:\Musica`,
//! nessun brano deve prendere «Musica» come interprete. Si guarda quindi solo la
//! parte di percorso *sotto* la radice che lo contiene, e mai un segmento più su.
//!
//! **Un contenitore generico non è un nome.** `Downloads`, `iTunes`,
//! `Nuova cartella`, `Vari` sono cartelle che qualcuno ha creato per metterci
//! roba dentro. Prenderle per un artista è il modo più veloce di riempire la
//! libreria di schede che non esistono.
//!
//! # Cosa questo modulo NON fa
//!
//! Non decide. Restituisce [`Indizi`], cioè delle ipotesi con un nome; è
//! [`crate::ricostruzione`] a stabilire quando un'ipotesi vince su un tag e
//! quando no. La separazione conta: qui si può sbagliare senza fare danni,
//! perché il confronto con quel che il file dichiara avviene dopo.

use crate::album::numero_disco;
use crate::paths::{PathRules, extension_of, file_stem, is_under};
use crate::text::{collapse_whitespace, fold_text, is_js_whitespace};
use crate::titolo::{concorda_con_autore, senza_ciarpame, spezza};

/// Il primo anno che si accetta da un nome di cartella.
///
/// Prima di questo non ci sono registrazioni, e un numero di quattro cifre più
/// piccolo è quasi sempre un titolo — `1000 Forms of Fear` è un disco vero.
const ANNO_MINIMO: i32 = 1900;
/// L'ultimo anno che si accetta. Largo di proposito: le uscite si datano avanti.
const ANNO_MASSIMO: i32 = 2100;

/// Il numero di traccia più grande che si accetta da un nome di file.
///
/// Tre cifre: esistono raccolte da duecento tracce. Quattro sarebbero un anno.
const TRACCIA_MASSIMA: u32 = 999;

/// I nomi di cartella che sono contenitori, non nomi di qualcuno.
///
/// Confronto sulla parola intera piegata, mai per sottostringa: `Musicals` è un
/// genere e non è `music`, e una libreria che perde le colonne sonore perché
/// contengono «music» è peggio di una che non deduce niente.
const CONTENITORI: &[&str] = &[
    "music",
    "musica",
    "musique",
    "musik",
    "media",
    "audio",
    "mp3",
    "mp3s",
    "flac",
    "download",
    "downloads",
    "scaricati",
    "itunes",
    "itunes media",
    "my music",
    "la mia musica",
    "documents",
    "documenti",
    "desktop",
    "new folder",
    "nuova cartella",
    "cartella nuova",
    "rip",
    "rips",
    "cd",
    "cds",
    "album",
    "albums",
    "compilation",
    "compilations",
    "raccolte",
    "varie",
    "vari",
    "singles",
    "singoli",
    "temp",
    "tmp",
    "shared",
    "condivisi",
    "library",
    "libreria",
    "collection",
    "collezione",
    "torrent",
    "torrents",
    "unsorted",
    "da ordinare",
    "incoming",
    "nuovi",
];

/// I valori che un taggatore scrive per dire «non lo so».
///
/// Le due sentinelle italiane di [`crate::album`] ci sono perché un file può
/// già essere passato da un riordino di Aether che gliele ha scritte dentro.
const SEGNAPOSTO: &[&str] = &[
    "unknown",
    "unknown artist",
    "unknown album",
    "unknown title",
    "unknown artist/band",
    "<unknown>",
    "(unknown)",
    "[unknown]",
    "unbekannt",
    "desconocido",
    "inconnu",
    "sconosciuto",
    "sconosciuta",
    "artista sconosciuto",
    "album sconosciuto",
    "various",
    "various artists",
    "va",
    "v/a",
    "no artist",
    "no album",
    "no title",
    "notitle",
    "untitled",
    "senza titolo",
    "senza nome",
    "n/a",
    "none",
    "null",
    "audio track",
    "audio cd",
    "track",
    "traccia",
    "pista",
];

/// Le parole che, seguite da un numero, restano un segnaposto.
///
/// `Track 03` e `Traccia 7` sono quel che un lettore CD scrive quando il disco
/// non è nel suo archivio: sono la stessa non-informazione di `Unknown`, ma
/// numerata, e senza questa riga entrerebbero in libreria come titoli veri.
///
/// **`song` e `brano` non ci sono, e non è una dimenticanza**: «Song 2» dei Blur
/// è un titolo vero, e nessun programma che rippa scrive «Song 01». La regola
/// vale solo per le parole che i lettori CD usano davvero come riempitivo.
const SEGNAPOSTO_NUMERATI: &[&str] = &[
    "track",
    "traccia",
    "pista",
    "titolo",
    "title",
    "audio track",
    "cd track",
    "untitled",
    "unknown",
];

/// Quel che il percorso di un file lascia intendere del brano.
///
/// Ogni campo è un'ipotesi, non un dato: nessuno di questi valori ha titolo per
/// sostituire un tag che c'è.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Indizi {
    /// L'interprete, dalla cartella dell'artista o dal nome del file.
    pub artista: Option<String>,
    /// L'album, dalla cartella che contiene il file.
    pub album: Option<String>,
    /// Il titolo, dal nome del file senza numero di traccia né interprete.
    pub titolo: Option<String>,
    /// L'anno, quando la cartella dell'album lo dichiara.
    pub anno: Option<i32>,
    /// Il numero di traccia, quando il nome del file lo dichiara.
    pub traccia: Option<u32>,
    /// Il numero di disco, dalla cartella `CD1` o dal prefisso del nome.
    pub disco: Option<u32>,
    /// L'interprete viene da una cartella, non dal nome del file.
    ///
    /// Serve a [`crate::ricostruzione`] per una domanda sola: se dedurre anche
    /// `album_artist`. Una cartella d'artista **è** l'artista dell'album — è la
    /// forma che [`crate::organize`] scrive — mentre un interprete ricavato dal
    /// nome del file è quello di quel brano soltanto, e su una raccolta
    /// scriverlo come artista del disco lo spezzerebbe in una uscita per traccia.
    pub artista_da_cartella: bool,
    /// L'interprete del nome del file concorda con quello della cartella.
    ///
    /// Quando è vero le due fonti dicono la stessa cosa e l'ipotesi è forte.
    /// Quando è falso non vuol dire «discordano»: quasi sempre vuol dire che a
    /// parlare è stata una fonte sola.
    pub artista_confermato: bool,
}

/// Il valore è vuoto, o è quel che si scrive per dire «non lo so»?
///
/// Estende [`crate::enrich::e_segnaposto`], che confronta con **una** sentinella
/// italiana alla volta, al vocabolario che i taggatori usano davvero. Senza
/// questa estensione un file con `artist = "Unknown Artist"` prende una scheda
/// artista tutta sua e l'arricchimento non lo guarda nemmeno, perché per lui
/// quel campo è pieno.
///
/// ```
/// use aether_domain::indizi::e_segnaposto;
/// assert!(e_segnaposto(Some("Unknown Artist")));
/// assert!(e_segnaposto(Some("<unknown>")));
/// assert!(e_segnaposto(Some("Various Artists")));
/// assert!(e_segnaposto(Some("Track 03")));
/// assert!(e_segnaposto(Some("   ")));
/// assert!(e_segnaposto(None));
/// // E i nomi veri restano nomi.
/// assert!(!e_segnaposto(Some("Unknown Mortal Orchestra")));
/// assert!(!e_segnaposto(Some("Various Cruelties")));
/// assert!(!e_segnaposto(Some("Track of Time")));
/// ```
#[must_use]
pub fn e_segnaposto(valore: Option<&str>) -> bool {
    let Some(grezzo) = valore else { return true };
    let piegato = collapse_whitespace(&fold_text(grezzo));
    if piegato.is_empty() {
        return true;
    }
    if SEGNAPOSTO.contains(&piegato.as_str()) {
        return true;
    }
    // `Track 03`: la coda numerica si stacca e si guarda cosa resta davanti.
    let Some((testa, coda)) = piegato.rsplit_once(' ') else {
        return false;
    };
    !coda.is_empty()
        && coda.bytes().all(|b| b.is_ascii_digit())
        && SEGNAPOSTO_NUMERATI.contains(&testa)
}

/// Il nome di cartella è un contenitore e non il nome di qualcuno?
fn e_contenitore(nome: &str) -> bool {
    let piegato = collapse_whitespace(&fold_text(nome));
    CONTENITORI.contains(&piegato.as_str())
}

/// Un segmento di cartella vale come nome?
///
/// Deve dire qualcosa: non vuoto, non un contenitore, non un segnaposto.
fn segmento_utile(nome: &str) -> Option<String> {
    let pulito = collapse_whitespace(nome);
    if pulito.is_empty() || e_contenitore(&pulito) || e_segnaposto(Some(&pulito)) {
        return None;
    }
    Some(pulito)
}

/// I segni che separano un numero, o un anno, da quel che segue.
const SEPARATORI: [char; 5] = ['-', '.', '_', '\u{2013}', '\u{2014}'];

/// Il testo è un anno plausibile di quattro cifre?
fn anno_di(cifre: &str) -> Option<i32> {
    if cifre.len() != 4 || !cifre.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let anno = cifre.parse::<i32>().ok()?;
    (ANNO_MINIMO..=ANNO_MASSIMO).contains(&anno).then_some(anno)
}

/// Rifila separatori, spazi e parentesi da un bordo.
fn rifila(testo: &str) -> &str {
    testo.trim_matches(|c: char| {
        is_js_whitespace(c) || SEPARATORI.contains(&c) || matches!(c, '[' | ']' | '(' | ')')
    })
}

/// L'anno che il nome di una cartella dichiara, e il nome senza.
///
/// Riconosce le due forme che i taggatori scrivono davvero: l'anno in testa
/// (`1973 - The Dark Side of the Moon`, con o senza parentesi) e in coda
/// (`The Dark Side of the Moon (1973)`).
///
/// **Non svuota mai il nome.** `1984` è un disco dei Van Halen, e un anno che si
/// porta via tutto il titolo non era un anno: in quel caso si restituisce il
/// nome intero e nessun anno.
fn stacca_anno(nome: &str) -> (Option<i32>, String) {
    let pulito = collapse_whitespace(nome);

    // In testa.
    let senza_apertura = pulito.trim_start_matches(['[', '(']);
    let cifre: String = senza_apertura
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    if let Some(anno) = anno_di(&cifre)
        && let Some(resto) = senza_apertura.get(cifre.len()..)
    {
        let resto = rifila(resto);
        if !resto.is_empty() {
            return (Some(anno), resto.to_owned());
        }
    }

    // In coda.
    let senza_chiusura = pulito.trim_end_matches([']', ')']);
    let rovesciate: String = senza_chiusura
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect();
    let cifre: String = rovesciate.chars().rev().collect();
    if let Some(anno) = anno_di(&cifre)
        && let Some(taglio) = senza_chiusura.len().checked_sub(cifre.len())
        && let Some(prima) = senza_chiusura.get(..taglio)
    {
        let prima = rifila(prima);
        if !prima.is_empty() {
            return (Some(anno), prima.to_owned());
        }
    }

    (None, pulito)
}

/// Il numero di disco che il nome di una cartella d'album porta in coda, e il
/// nome senza.
///
/// `The Wall (CD1)` e `The Wall - Disc 2` sono lo stesso disco scritto da due
/// programmi diversi. Toglierlo dal titolo non è cosmesi: senza, la libreria
/// mostra due album che si chiamano quasi uguale.
///
/// Le altre code fra parentesi restano dove sono. `(Deluxe Edition)` **fa parte**
/// del titolo che l'utente si aspetta di leggere — è
/// [`crate::album::album_group_key`] a toglierla per raggruppare, e la
/// distinzione fra «come si chiama» e «con chi sta insieme» è tutto il punto di
/// quel modulo.
fn stacca_disco(nome: &str) -> (Option<u32>, String) {
    let pulito = collapse_whitespace(nome);

    // Una coda fra parentesi: `(CD1)`, `[Disc 2]`.
    if let Some(chiusura) = pulito.chars().next_back()
        && matches!(chiusura, ')' | ']')
        && let Some(senza) = pulito.get(..pulito.len().saturating_sub(chiusura.len_utf8()))
        && let Some(apertura) = senza.rfind(['(', '['])
        && let Some(dentro) = senza.get(apertura.saturating_add(1)..)
        && let Some(numero) = numero_disco(dentro)
        && let Some(testa) = senza.get(..apertura)
    {
        let testa = rifila(testa);
        if !testa.is_empty() {
            return (Some(numero), testa.to_owned());
        }
    }

    // Una coda dopo un trattino isolato: `The Wall - CD1`.
    if let Some((testa, coda)) = pulito.rsplit_once(" - ")
        && let Some(numero) = numero_disco(coda)
    {
        let testa = rifila(testa);
        if !testa.is_empty() {
            return (Some(numero), testa.to_owned());
        }
    }

    (None, pulito)
}

/// Quel che il nome del file dichiara, prima ancora di guardare le cartelle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct DalNome {
    disco: Option<u32>,
    traccia: Option<u32>,
    artista: Option<String>,
    titolo: Option<String>,
}

/// Le cifre in testa, se non sono più di `massimo`, e quel che resta.
fn cifre_iniziali(testo: &str, massimo: usize) -> Option<(&str, &str)> {
    let quante = testo.chars().take_while(char::is_ascii_digit).count();
    if quante == 0 || quante > massimo {
        return None;
    }
    // Le cifre sono ASCII: contarle in caratteri o in byte è lo stesso.
    Some((testo.get(..quante)?, testo.get(quante..)?))
}

/// Il numero di disco e di traccia in testa al nome, e il titolo che resta.
///
/// # La regola che salva «99 Luftballons»
///
/// Un numero seguito da uno spazio conta come traccia **solo se è imbottito di
/// zeri**. È la convenzione di ogni programma che rippa — `01 Speak to Me` — e
/// distingue il numero dal titolo che comincia per numero: `99 Luftballons`,
/// `7 Nation Army`, `21 Guns` restano interi. Con un separatore vero davanti al
/// titolo (`10 - Titolo`, `03. Titolo`) l'imbottitura non serve, perché il
/// separatore dice già che il numero è staccato.
///
/// # Perché non si riconoscono i lati del vinile
///
/// Perché `A1 - Titolo` e `U2 - One` hanno la stessa forma, e sbagliare il
/// secondo — buttando via l'interprete e tenendo un numero di traccia inventato
/// — costa molto più di quanto valga riconoscere il primo.
fn stacca_numeri(nome: &str) -> (Option<u32>, Option<u32>, String) {
    // Disco e traccia insieme: `1-01 Titolo`, `2.05 - Titolo`.
    if let Some((cifre_disco, resto)) = cifre_iniziali(nome, 2)
        && let Some(segno) = resto.chars().next()
        && matches!(segno, '-' | '.')
        && let Some(dopo) = resto.get(segno.len_utf8()..)
        && let Some((cifre_traccia, coda)) = cifre_iniziali(dopo, 3)
        && let Some(stacco) = coda.chars().next()
        && (is_js_whitespace(stacco) || SEPARATORI.contains(&stacco))
    {
        let titolo = rifila(coda);
        if !titolo.is_empty()
            && let (Ok(disco), Ok(traccia)) =
                (cifre_disco.parse::<u32>(), cifre_traccia.parse::<u32>())
            && traccia <= TRACCIA_MASSIMA
        {
            return (Some(disco), Some(traccia), titolo.to_owned());
        }
    }

    // La sola traccia.
    if let Some((cifre, resto)) = cifre_iniziali(nome, 3) {
        let separata = resto
            .trim_start_matches(is_js_whitespace)
            .starts_with(|c: char| SEPARATORI.contains(&c) || matches!(c, ')' | ']'));
        let imbottita = cifre.starts_with('0') && resto.starts_with(is_js_whitespace);
        if separata || imbottita {
            let titolo = rifila(resto);
            if !titolo.is_empty()
                && let Ok(traccia) = cifre.parse::<u32>()
                && traccia <= TRACCIA_MASSIMA
            {
                return (None, Some(traccia), titolo.to_owned());
            }
        }
    }

    (None, None, collapse_whitespace(nome))
}

/// Legge il nome del file: numeri in testa, poi interprete e titolo.
fn dal_nome(radice: &str) -> DalNome {
    // `senza_ciarpame` toglie `(Official Video)` e simili — il rumore dei file
    // scaricati — e lascia stare `(Live at Wembley)`, che dice *quale*
    // registrazione è. La distinzione è scritta per esteso in `crate::titolo`.
    let pulito = senza_ciarpame(radice);
    let (disco, traccia, resto) = stacca_numeri(&pulito);
    let (artista, titolo) = spezza(&resto, None);
    DalNome {
        disco,
        traccia,
        artista: artista.filter(|a| !e_segnaposto(Some(a))),
        titolo: Some(titolo).filter(|t| !e_segnaposto(Some(t))),
    }
}

/// I segmenti di cartella che stanno **sotto** la radice sorvegliata.
///
/// La radice e tutto ciò che le sta sopra sono fuori: sono il posto in cui
/// l'utente tiene la musica, non qualcosa che riguarda questo brano. Senza
/// questo taglio, una libreria in `D:\Musica` darebbe a ogni traccia senza tag
/// l'interprete «Musica».
///
/// Quando nessuna radice contiene il percorso — un file guardato fuori da una
/// scansione, o un elenco di radici vuoto — si prende comunque la catena di
/// cartelle, tolta la lettera di unità: è meno sicuro, e per questo
/// [`segmento_utile`] resta il filtro che decide cosa vale come nome.
fn segmenti_sotto_radice<'a>(
    percorso: &'a str,
    radici: &[String],
    rules: PathRules,
) -> Vec<&'a str> {
    let pezzi = |testo: &'a str| -> Vec<&'a str> {
        testo
            .split(['/', '\\'])
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
    };
    let mut segmenti = pezzi(percorso);
    // Via il nome del file: qui si guardano solo le cartelle.
    segmenti.pop();

    // La radice più lunga fra quelle che lo contengono: con `D:\Musica` e
    // `D:\Musica\Live` registrate entrambe, la seconda è quella che dice di più.
    let quante = radici
        .iter()
        .filter(|radice| is_under(percorso, radice, rules))
        .map(|radice| radice.split(['/', '\\']).filter(|p| !p.is_empty()).count())
        .max();

    match quante {
        Some(quante) => segmenti.into_iter().skip(quante).collect(),
        // Una lettera di unità (`C:`) o un host UNC non è il nome di nessuno.
        None => segmenti
            .into_iter()
            .skip_while(|s| s.ends_with(':'))
            .collect(),
    }
}

/// Gli indizi che il percorso di un file porta con sé.
///
/// `radici` sono le cartelle sorvegliate, quelle che la scansione conosce:
/// servono a sapere dove smettere di salire. Passarne un elenco vuoto è lecito e
/// rende la deduzione più larga, non sbagliata — il filtro sui contenitori
/// generici resta.
///
/// ```
/// use aether_domain::indizi::dal_percorso;
/// use aether_domain::paths::PathRules;
///
/// let radici = vec!["D:/Musica".to_owned()];
/// let regole = PathRules { case_insensitive: true };
/// let i = dal_percorso(
///     "D:/Musica/Pink Floyd/1973 - The Dark Side of the Moon/01 - Speak to Me.flac",
///     &radici,
///     regole,
/// );
/// assert_eq!(i.artista.as_deref(), Some("Pink Floyd"));
/// assert_eq!(i.album.as_deref(), Some("The Dark Side of the Moon"));
/// assert_eq!(i.titolo.as_deref(), Some("Speak to Me"));
/// assert_eq!(i.anno, Some(1973));
/// assert_eq!(i.traccia, Some(1));
///
/// // La radice non è un artista.
/// let i = dal_percorso("D:/Musica/01 - Speak to Me.flac", &radici, regole);
/// assert_eq!(i.artista, None);
/// assert_eq!(i.album, None);
/// assert_eq!(i.titolo.as_deref(), Some("Speak to Me"));
/// ```
#[must_use]
pub fn dal_percorso(percorso: &str, radici: &[String], rules: PathRules) -> Indizi {
    let dal_file = dal_nome(file_stem(percorso));
    let mut segmenti = segmenti_sotto_radice(percorso, radici, rules);

    // Una cartella foglia che è un indicatore di disco si risolve nella
    // superiore, esattamente come fa `album::album_folder` per raggruppare: un
    // album su due dischi è una pubblicazione sola.
    let mut disco_cartella = None;
    if segmenti.len() >= 2
        && let Some(numero) = segmenti.last().and_then(|foglia| numero_disco(foglia))
    {
        disco_cartella = Some(numero);
        segmenti.pop();
    }

    let cartella_album = segmenti.last().copied().and_then(segmento_utile);
    let cartella_artista = segmenti
        .len()
        .checked_sub(2)
        .and_then(|posto| segmenti.get(posto))
        .copied()
        .and_then(segmento_utile);

    let mut anno = None;
    let mut album = None;
    let mut artista_dalla_cartella = None;

    if let Some(nome) = cartella_album {
        let (disco_in_coda, senza_disco) = stacca_disco(&nome);
        disco_cartella = disco_cartella.or(disco_in_coda);
        let (anno_trovato, senza_anno) = stacca_anno(&senza_disco);
        anno = anno_trovato;

        // Una sola cartella sotto la radice, e il nome del file dice che
        // l'interprete è proprio quella: allora quella cartella è l'artista, non
        // l'album. È il caso di `Musica/Radiohead/Radiohead - Creep.mp3`.
        let e_lartista = cartella_artista.is_none()
            && segmenti.len() <= 1
            && concorda_con_autore(dal_file.artista.as_deref(), Some(&senza_anno));
        if e_lartista {
            artista_dalla_cartella = Some(senza_anno);
        } else if cartella_artista.is_none() {
            // `Pink Floyd - The Wall` in una cartella sola: le due cose stanno
            // nello stesso nome, e il trattino isolato le divide.
            let (davanti, dietro) = spezza(&senza_anno, None);
            artista_dalla_cartella = davanti.filter(|a| !e_segnaposto(Some(a)));
            album = Some(dietro).filter(|a| !e_segnaposto(Some(a)));
        } else {
            album = Some(senza_anno).filter(|a| !e_segnaposto(Some(a)));
        }
    }

    let da_cartella = cartella_artista.clone().or(artista_dalla_cartella);
    let artista_da_cartella = da_cartella.is_some();
    let artista = da_cartella.or_else(|| dal_file.artista.clone());

    Indizi {
        artista_confermato: concorda_con_autore(
            dal_file.artista.as_deref(),
            cartella_artista.as_deref(),
        ),
        artista_da_cartella,
        artista,
        album,
        titolo: dal_file.titolo,
        anno,
        traccia: dal_file.traccia,
        disco: dal_file.disco.or(disco_cartella),
    }
}

/// Le estensioni che si accettano per una copertina di lato.
///
/// Le stesse che `image` sa decodificare nella configurazione di Aether. Un
/// `.bmp` da otto megabyte accanto ai file è raro ma esiste, e ricodificarlo
/// costa quanto ricodificare un JPEG.
const IMMAGINI: [&str; 5] = ["jpg", "jpeg", "png", "webp", "bmp"];

/// I nomi con cui si chiama una copertina, dal più esplicito al meno.
///
/// L'ordine è la regola: in una cartella che contiene `cover.jpg`, `back.jpg` e
/// `booklet-01.jpg` la copertina è la prima, e prendere «la prima immagine che
/// si trova» darebbe una griglia di retrocopertine — lo stesso guasto che
/// `metadata::pick_cover` evita dentro il file preferendo `CoverFront`.
const NOMI_COPERTINA: [&str; 6] = ["cover", "folder", "front", "album", "albumart", "artwork"];

/// Quanto un file accanto ai brani somiglia alla copertina del disco.
///
/// `None` quando non è un'immagine, o quando il nome dice che è **un'altra**
/// immagine: `back`, `booklet`, `inlay`, `disc`, `cd1` sono le scansioni del
/// resto della confezione, e nessuna di quelle è la copertina.
///
/// Un'immagine con un nome qualunque vale zero: si prende solo se in quella
/// cartella non c'è di meglio. È il caso della cartella che contiene una sola
/// immagine chiamata come l'album.
///
/// ```
/// use aether_domain::indizi::rango_copertina;
/// assert!(rango_copertina("D:/M/A/B/cover.jpg") > rango_copertina("D:/M/A/B/folder.png"));
/// assert!(rango_copertina("D:/M/A/B/folder.png") > rango_copertina("D:/M/A/B/scan.jpg"));
/// assert_eq!(rango_copertina("D:/M/A/B/back.jpg"), None);
/// assert_eq!(rango_copertina("D:/M/A/B/note.txt"), None);
/// ```
#[must_use]
pub fn rango_copertina(percorso: &str) -> Option<u8> {
    if !IMMAGINI.contains(&extension_of(percorso).as_str()) {
        return None;
    }
    let radice = fold_text(file_stem(percorso));
    let nudo = radice.trim_matches(|c: char| !c.is_alphanumeric());
    if ESCLUSE.iter().any(|escluso| nudo.starts_with(escluso)) {
        return None;
    }
    let posto = NOMI_COPERTINA
        .iter()
        .position(|nome| nudo.starts_with(nome))
        .and_then(|posto| NOMI_COPERTINA.len().checked_sub(posto));
    Some(u8::try_from(posto.unwrap_or(0)).unwrap_or(0))
}

/// I nomi che dicono «questa immagine è un'altra parte della confezione».
///
/// Il confronto è per prefisso perché queste immagini arrivano numerate —
/// `booklet-01`, `disc2` — e per prefisso, e non per sottostringa, perché
/// `frontal` comincia per `front` mentre `bandcamp-back` finirebbe escluso da
/// una regola che cerca `back` dovunque.
const ESCLUSE: [&str; 8] = [
    "back", "retro", "booklet", "libretto", "inlay", "inside", "matrix", "obi",
];

#[cfg(test)]
mod prove {
    use super::*;

    const REGOLE: PathRules = PathRules {
        case_insensitive: true,
    };

    fn radici() -> Vec<String> {
        vec!["D:/Musica".to_owned()]
    }

    fn indizi(percorso: &str) -> Indizi {
        dal_percorso(percorso, &radici(), REGOLE)
    }

    #[test]
    fn il_caso_normale_di_un_rip() {
        let i =
            indizi("D:/Musica/Pink Floyd/1973 - The Dark Side of the Moon/01 - Speak to Me.flac");
        assert_eq!(i.artista.as_deref(), Some("Pink Floyd"));
        assert_eq!(i.album.as_deref(), Some("The Dark Side of the Moon"));
        assert_eq!(i.titolo.as_deref(), Some("Speak to Me"));
        assert_eq!(i.anno, Some(1973));
        assert_eq!(i.traccia, Some(1));
        assert_eq!(i.disco, None);
    }

    #[test]
    fn lanno_si_riconosce_in_testa_e_in_coda() {
        for cartella in [
            "1973 - The Dark Side of the Moon",
            "[1973] The Dark Side of the Moon",
            "The Dark Side of the Moon (1973)",
            "The Dark Side of the Moon [1973]",
        ] {
            let i = indizi(&format!(
                "D:/Musica/Pink Floyd/{cartella}/01 - Speak to Me.flac"
            ));
            assert_eq!(i.anno, Some(1973), "{cartella}");
            assert_eq!(
                i.album.as_deref(),
                Some("The Dark Side of the Moon"),
                "{cartella}"
            );
        }
    }

    #[test]
    fn un_titolo_che_e_un_numero_resta_un_titolo() {
        // `1984` è un disco dei Van Halen: un anno che si porta via tutto il
        // nome non era un anno.
        let i = indizi("D:/Musica/Van Halen/1984/01 - 1984.flac");
        assert_eq!(i.album.as_deref(), Some("1984"));
        assert_eq!(i.anno, None);
    }

    #[test]
    fn i_dischi_multipli_danno_il_numero_e_restano_un_album() {
        for coda in ["CD2", "Disc 2", "Disco 2"] {
            let i = indizi(&format!(
                "D:/Musica/Pink Floyd/The Wall/{coda}/03 - Hey You.flac"
            ));
            assert_eq!(i.disco, Some(2), "{coda}");
            assert_eq!(i.album.as_deref(), Some("The Wall"), "{coda}");
            assert_eq!(i.artista.as_deref(), Some("Pink Floyd"), "{coda}");
        }
        // …e lo stesso quando il numero sta in coda al nome dell'album.
        let i = indizi("D:/Musica/Pink Floyd/The Wall (CD1)/03 - Hey You.flac");
        assert_eq!(i.disco, Some(1));
        assert_eq!(i.album.as_deref(), Some("The Wall"));
    }

    #[test]
    fn la_coda_di_edizione_resta_nel_titolo() {
        // Non è cosmesi al contrario: `(Deluxe Edition)` è come il disco si
        // chiama, ed è `album::album_group_key` a toglierla per raggruppare.
        let i = indizi("D:/Musica/Adele/25 (Deluxe Edition)/01 - Hello.flac");
        assert_eq!(i.album.as_deref(), Some("25 (Deluxe Edition)"));
    }

    #[test]
    fn la_radice_non_e_un_artista() {
        // Il guasto che questa prova impedisce: una libreria in `D:\Musica`
        // dove ogni brano senza tag prende l'interprete «Musica».
        let i = indizi("D:/Musica/01 - Speak to Me.flac");
        assert_eq!(i.artista, None);
        assert_eq!(i.album, None);
        assert_eq!(i.titolo.as_deref(), Some("Speak to Me"));
    }

    #[test]
    fn un_contenitore_generico_non_e_un_nome() {
        let radici = vec!["D:/".to_owned()];
        let i = dal_percorso("D:/Downloads/Nuova cartella/01 - X.mp3", &radici, REGOLE);
        assert_eq!(i.artista, None, "«Downloads» non è un interprete");
        assert_eq!(i.album, None, "«Nuova cartella» non è un album");
        assert_eq!(i.titolo.as_deref(), Some("X"));
    }

    #[test]
    fn un_titolo_che_comincia_per_numero_resta_intero() {
        // La regola dell'imbottitura: `01 Titolo` è una traccia, `99
        // Luftballons` è una canzone. Sbagliarlo vuol dire mettere in libreria
        // «Luftballons» di nessuno.
        for (nome, titolo, traccia) in [
            ("99 Luftballons", "99 Luftballons", None),
            ("7 Nation Army", "7 Nation Army", None),
            ("21 Guns", "21 Guns", None),
            ("01 Speak to Me", "Speak to Me", Some(1)),
            ("10 - Eclipse", "Eclipse", Some(10)),
            ("03. Time", "Time", Some(3)),
        ] {
            let i = indizi(&format!("D:/Musica/A/B/{nome}.flac"));
            assert_eq!(i.titolo.as_deref(), Some(titolo), "{nome}");
            assert_eq!(i.traccia, traccia, "{nome}");
        }
    }

    #[test]
    fn il_prefisso_disco_traccia_si_legge() {
        let i = indizi("D:/Musica/A/B/2-05 - Hey You.flac");
        assert_eq!(i.disco, Some(2));
        assert_eq!(i.traccia, Some(5));
        assert_eq!(i.titolo.as_deref(), Some("Hey You"));
    }

    #[test]
    fn una_cartella_sola_che_dice_due_cose_si_divide() {
        let i = indizi("D:/Musica/Pink Floyd - The Wall/03 - Hey You.flac");
        assert_eq!(i.artista.as_deref(), Some("Pink Floyd"));
        assert_eq!(i.album.as_deref(), Some("The Wall"));
    }

    #[test]
    fn su_una_raccolta_linterprete_lo_da_il_nome_del_file() {
        // La cartella dice «Various Artists», che è un segnaposto: l'unico
        // interprete vero sta nel nome del file, e va preso da lì.
        let i = indizi("D:/Musica/Various Artists/Best of 90s/04 - Blur - Song 2.mp3");
        assert_eq!(i.artista.as_deref(), Some("Blur"));
        assert_eq!(i.album.as_deref(), Some("Best of 90s"));
        assert_eq!(i.titolo.as_deref(), Some("Song 2"));
        assert!(!i.artista_confermato, "la cartella non conferma niente");
    }

    #[test]
    fn le_due_fonti_che_concordano_si_dichiarano() {
        let i = indizi("D:/Musica/Radiohead/The Bends/02 - Radiohead - Fake Plastic Trees.flac");
        assert_eq!(i.artista.as_deref(), Some("Radiohead"));
        assert!(i.artista_confermato);
    }

    #[test]
    fn un_titolo_segnaposto_non_e_un_titolo() {
        let i = indizi("D:/Musica/A/B/Track 03.mp3");
        assert_eq!(
            i.titolo, None,
            "«Track 03» è la non-informazione di un lettore CD"
        );
    }

    #[test]
    fn senza_radici_la_deduzione_e_larga_ma_non_sbagliata() {
        let i = dal_percorso("C:/Pink Floyd/The Wall/03 - Hey You.flac", &[], REGOLE);
        assert_eq!(i.artista.as_deref(), Some("Pink Floyd"));
        assert_eq!(i.album.as_deref(), Some("The Wall"));
        // La lettera di unità non entra mai fra i nomi.
        let i = dal_percorso("C:/The Wall/03 - Hey You.flac", &[], REGOLE);
        assert_eq!(i.artista, None);
        assert_eq!(i.album.as_deref(), Some("The Wall"));
    }

    #[test]
    fn il_rumore_dei_caricamenti_sparisce_dal_titolo() {
        let i = indizi("D:/Musica/A/B/Cesare Cremonini - Poetica (Official Video).mp3");
        assert_eq!(i.titolo.as_deref(), Some("Poetica"));
        // …e quel che dice *quale* registrazione è resta.
        let i = indizi("D:/Musica/A/B/Song (Live at Wembley).mp3");
        assert_eq!(i.titolo.as_deref(), Some("Song (Live at Wembley)"));
    }
}
