//! Quando un disco in libreria è una pubblicazione conosciuta.
//!
//! L'arricchimento va a cercare in rete il titolo giusto, l'anno giusto e la
//! copertina di brani che i loro tag descrivono male o non descrivono affatto.
//! Questo modulo è la parte che **decide**, e sta nel dominio per la ragione che
//! [`crate`] scrive in testa: è una decisione che si può sbagliare in silenzio, e
//! sbagliandola si riscrivono i tag dei file dell'utente con quelli di un'altra
//! canzone. Provarla deve costare una chiamata di funzione.
//!
//! # L'album prima del brano
//!
//! Il vecchio albero arricchiva un brano per volta: un titolo contro un titolo,
//! una prova sola e debole. Qui l'unità è il **gruppo d'album** — quello che
//! [`crate::album::album_group_key`] già forma da titolo e cartella — e le prove
//! arrivano insieme: il numero di tracce coincide, l'ordine coincide, dodici
//! durate coincidono.
//!
//! La differenza non è di grado. Dodici corrispondenze indipendenti al 70% sono
//! dodici occasioni di sbagliare; un album le lega, e un disco che concorda su
//! dodici tracce non è «probabilmente quello». Costa anche molto meno: due o tre
//! richieste invece di dodici, che con il limite di **una al secondo** di
//! MusicBrainz è la differenza fra una libreria arricchita in dieci minuti e una
//! in mezz'ora.
//!
//! # Astenersi è il comportamento normale
//!
//! Nessuno guarda prima che si scriva. Quindi ogni funzione di decisione qui
//! dentro ha tre uscite e non due: [`Verdetto::Applica`] quando le prove ci sono,
//! [`Verdetto::DaRivedere`] quando c'è un candidato plausibile ma non provato, e
//! [`Verdetto::Nessuno`] quando non c'è niente. **`DaRivedere` non scrive nulla**:
//! né un tag, né una copertina, né un identificativo. È la regola che
//! `decision.ts` del vecchio albero aveva già capito, e vale la pena ripeterne il
//! motivo — una corrispondenza plausibile applicata è un errore che nessuno
//! troverà mai, perché il brano avrà l'aria di essere a posto.
//!
//! # Un asse ignoto non è un accordo
//!
//! È l'altra metà della stessa idea, e il punto in cui un porto distratto
//! romperebbe tutto. Nel **punteggio** una durata sconosciuta vale 0.5, cioè
//! neutro: serve a ordinare i candidati fra loro. Nella **decisione** vale
//! `None`, e `None` non conta mai verso l'applicazione. «Non lo so» e «va bene»
//! si scrivono con due valori diversi perché sono due cose diverse.
//!
//! # Le varianti si confrontano, non si tolgono
//!
//! Qui questo modulo si stacca dal vecchio albero di proposito. Là la
//! normalizzazione toglieva `live`, `remix`, `edit` e `version` prima di
//! confrontare, e il risultato è che una base karaoke — che dura esattamente
//! quanto l'originale e si chiama esattamente come l'originale — passava per il
//! brano giusto e ne portava dentro la copertina. Là a coprire quel buco c'era
//! l'impronta acustica; senza chiavi API non c'è.
//!
//! [`varianti`] costruisce invece una maschera di quel che un titolo **dichiara
//! di essere**, e due maschere diverse sono un veto. È la stessa «condizione
//! doppia» di [`crate::yt_match`]: chi ha davvero un live in libreria deve poter
//! trovare il suo live, e un brano che si chiama davvero «Remix» non deve
//! diventare introvabile.

use std::collections::HashMap;

use crate::abbinamento::senza_decorazioni;
use crate::album::{UNKNOWN_ALBUM, UNKNOWN_ARTIST, strip_edition_suffix};
use crate::keys::normalize_key;
use crate::text::{collapse_whitespace, fold_text, is_js_whitespace};

// ── le soglie ───────────────────────────────────────────────────────────────

/// Quanto due titoli devono somigliarsi per contare come lo stesso titolo.
pub const TITOLO_FORTE: f64 = 0.85;

/// Quanto due interpreti devono somigliarsi per contare come lo stesso.
///
/// Più bassa di quella dei titoli, e non per generosità: gli interpreti si
/// scrivono in modi legittimamente diversi («Beatles» / «The Beatles», «A & B» /
/// «A and B», l'ospite dentro o fuori), mentre un titolo che differisce di un
/// sesto è quasi sempre un altro pezzo.
pub const ARTISTA_FORTE: f64 = 0.80;

/// Sotto questa somiglianza un asse **noto** non è incerto: smentisce.
///
/// Porto di `CONTRADICTION` (`decision.ts:44`). Un interprete o un album così
/// dissimili descrivono un'altra cosa — una raccolta tributo, un disco di
/// karaoke, un omonimo — e nessun accordo sugli altri assi lo riscatta.
pub const SMENTITA: f64 = 0.35;

/// Lo scarto di durata entro cui due registrazioni sono la stessa.
///
/// Dieci secondi, come [`crate::abbinamento::TOLLERANZA_MS`] e per la stessa
/// ragione già scritta lì: sono i silenzi di coda e gli stacchi che cambiano fra
/// una codifica e l'altra dello stesso master.
pub const GRAZIA_MS: u64 = 10_000;

/// Oltre questo scarto la penalità di durata è al massimo.
pub const SCARTO_MASSIMO_MS: u64 = 30_000;

/// Lo scarto oltre il quale un brano assegnato **impedisce** di applicare.
///
/// Quindici secondi su una traccia sola bastano a fermare tutto il disco. Non è
/// pignoleria: la versione allungata di un pezzo dentro un'edizione deluxe è
/// esattamente questo, e prenderla per l'originale vuol dire scrivere il numero
/// di traccia sbagliato su tutto il resto del disco.
pub const VETO_DURATA_SEC: f64 = 15.0;

/// Quanto vicine devono essere due durate perché ancorino un candidato al file.
pub const ANCORA_DURATA_SEC: f64 = 4.0;

/// La distanza sotto cui un album si applica.
pub const DISTANZA_APPLICA: f64 = 0.12;

/// La distanza sotto cui un album è plausibile, ma non provato.
pub const DISTANZA_PLAUSIBILE: f64 = 0.35;

/// Il punteggio minimo perché un candidato per brano sia preso in
/// considerazione.
///
/// `MATCH_THRESHOLD` del vecchio albero (`match.ts:149`), invariato: è la soglia
/// sotto la quale non vale nemmeno la pena guardare le prove.
pub const SOGLIA_CANDIDATO: f64 = 0.65;

// ── i pesi della distanza d'album ───────────────────────────────────────────
//
// Sono quelli di beets (`match.distance_weights`), e sono presi da lì invece che
// inventati perché sono taratura fatta su librerie vere per anni. Il rapporto
// che conta è che titolo dell'album, interprete e titolo della traccia pesano
// il triplo dell'indice e dell'anno: i primi tre dicono *quale disco è*, gli
// altri due sono dettagli su cui una taggatura sciatta sbaglia di continuo.

/// Peso del titolo dell'album.
const P_ALBUM: f64 = 3.0;
/// Peso dell'interprete dell'album.
const P_ARTISTA: f64 = 3.0;
/// Peso della differenza fra il numero di tracce.
const P_CONTEGGIO: f64 = 2.0;
/// Peso delle tracce della pubblicazione che nessun file occupa.
const P_MANCANTI: f64 = 0.9;
/// Peso dei file che nessuna traccia della pubblicazione accoglie.
const P_SPAIATI: f64 = 0.6;
/// Peso dell'anno.
const P_ANNO: f64 = 1.0;
/// Peso del titolo di una traccia.
const P_TITOLO: f64 = 3.0;
/// Peso della durata di una traccia.
const P_DURATA: f64 = 2.0;
/// Peso del numero di traccia.
const P_INDICE: f64 = 1.0;

// ── normalizzazione ─────────────────────────────────────────────────────────

/// Il testo nella forma in cui due scritture della stessa cosa coincidono.
///
/// # Perché è una composizione e non una funzione nuova
///
/// È [`senza_decorazioni`] seguito da [`normalize_key`], cioè due funzioni che
/// esistono già e che i vettori dorati in `tests/golden/` provano contro il
/// JavaScript originale. Riscriverne una terza versione «per il confronto»
/// vorrebbe dire tre idee di cosa sia lo stesso titolo, e la terza divergerebbe
/// dalle altre due su un carattere che nessuno andrebbe a cercare.
///
/// **Non** sostituisce nessuna delle due: [`normalize_key`] costruisce le chiavi
/// che attraversano la sincronizzazione e [`crate::album::normalize_key_text`]
/// costruisce `album_key`. Quelle due sono già scritte nei file su Drive e nel
/// database, e cambiarle vorrebbe dire perdere l'aggancio a ciò che c'è. Questa
/// non è salvata da nessuna parte: si ricalcola a ogni confronto, quindi si può
/// cambiare quando serve.
///
/// ```
/// use aether_domain::enrich::normalize_for_match;
/// assert_eq!(
///     normalize_for_match("Poetica (Radio Edit)"),
///     normalize_for_match("poetica")
/// );
/// ```
#[must_use]
pub fn normalize_for_match(input: &str) -> String {
    normalize_key(Some(&senza_decorazioni(input)))
}

/// Le parole con cui un caricamento su YouTube annuncia sé stesso.
///
/// Sono rumore di **pubblicazione**, non di registrazione: dicono come il video
/// è fatto, non quale brano contiene. Per questo si tolgono dalla stringa di
/// ricerca, mentre `live` e `remaster` restano — cercare la versione dal vivo di
/// un file dal vivo è la cosa giusta, e toglierla dalla ricerca farebbe trovare
/// lo studio.
const RUMORE_DI_CARICAMENTO: &[&str] = &[
    "official",
    "ufficiale",
    "video",
    "videoclip",
    "audio",
    "lyric",
    "lyrics",
    "testo",
    "hd",
    "hq",
    "4k",
    "mv",
    "visualizer",
    "visualiser",
    "topic",
];

/// Il titolo nella forma in cui vale la pena cercarlo.
///
/// «Cesare Cremonini - Poetica (Official Video)» non è un titolo: è la
/// trascrizione di come qualcuno ha chiamato un caricamento. Cercare quella
/// stringa su MusicBrainz non dà niente, e il brano resta senza metadati per
/// sempre — è il caso preciso per cui questo modulo esiste.
///
/// Si toglie solo il rumore di caricamento: un gruppo fra parentesi che ne è
/// **fatto interamente**, la coda `- Topic`, e le frasi che nominano il tipo di
/// video. Il resto del titolo resta, comprese le parentesi che dicono qualcosa
/// sul brano.
///
/// ```
/// use aether_domain::enrich::titolo_da_cercare;
/// assert_eq!(titolo_da_cercare("Poetica (Official Video)"), "Poetica");
/// assert_eq!(titolo_da_cercare("Poetica (Live)"), "Poetica (Live)");
/// ```
#[must_use]
pub fn titolo_da_cercare(input: &str) -> String {
    let senza_gruppi = togli_gruppi(input, |dentro| {
        let piegato = fold_text(dentro);
        let mut parole = parole_di(&piegato).into_iter();
        // Un gruppo si toglie solo se **ogni** sua parola è rumore: `(Official
        // Video)` se ne va, `(Live at Wembley)` no — e nemmeno `(Video Games)`,
        // che è un titolo vero.
        parole.next().is_some_and(|prima| {
            RUMORE_DI_CARICAMENTO.contains(&prima.as_str())
                && parole.all(|p| RUMORE_DI_CARICAMENTO.contains(&p.as_str()))
        })
    });
    let senza_topic = togli_coda_topic(&senza_gruppi);
    collapse_whitespace(&senza_topic)
}

/// Toglie la coda `- Topic`, che YouTube attacca ai canali che genera da sé.
fn togli_coda_topic(input: &str) -> String {
    let rifilato = input.trim_end_matches(is_js_whitespace);
    for coda in [" - topic", " – topic", " topic"] {
        if let Some(taglio) = rifilato.len().checked_sub(coda.len())
            && rifilato
                .get(taglio..)
                .is_some_and(|fine| fold_text(fine) == coda)
            && let Some(testa) = rifilato.get(..taglio)
        {
            return testa.to_owned();
        }
    }
    rifilato.to_owned()
}

/// Toglie i gruppi fra parentesi il cui contenuto soddisfa `scarta`.
///
/// Un solo livello, come la regex originale (`[^()\[\]]*`): un gruppo annidato
/// non è un caso che capiti nei titoli, e trattarlo richiederebbe un analizzatore
/// vero per non guadagnare niente.
///
/// Se una parentesi non si chiude, il resto della stringa torna com'era: un
/// titolo con una parentesi aperta per sbaglio è comunque un titolo, e mangiarne
/// la metà finale sarebbe peggio che lasciarlo intero — la stessa scelta che
/// [`crate::abbinamento`] fa in `togli_parentesi`.
fn togli_gruppi(input: &str, scarta: impl Fn(&str) -> bool) -> String {
    let mut fuori = String::with_capacity(input.len());
    let mut resto = input;
    loop {
        let Some(apertura) = resto.find(['(', '[']) else {
            fuori.push_str(resto);
            return fuori;
        };
        let Some(dopo) = resto.get(apertura.saturating_add(1)..) else {
            fuori.push_str(resto);
            return fuori;
        };
        let Some(chiusura) = dopo.find([')', ']']) else {
            fuori.push_str(resto);
            return fuori;
        };
        let dentro = dopo.get(..chiusura).unwrap_or("");
        fuori.push_str(resto.get(..apertura).unwrap_or(""));
        if scarta(dentro) {
            fuori.push(' ');
        } else {
            let fine = apertura.saturating_add(chiusura).saturating_add(2);
            fuori.push_str(resto.get(apertura..fine).unwrap_or(""));
        }
        resto = dopo.get(chiusura.saturating_add(1)..).unwrap_or("");
    }
}

/// Le parole di un testo già piegato, senza punteggiatura.
fn parole_di(piegato: &str) -> Vec<String> {
    piegato
        .split(|c: char| !c.is_alphanumeric())
        .filter(|p| !p.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// Le parole che dichiarano una registrazione **diversa** dall'originale.
///
/// Parente stretta di `RUMORE` in [`crate::yt_match`], e con la stessa logica:
/// non sono parole brutte, sono parole che significano qualcosa **quando stanno
/// da una parte sola**. La differenza è dove si guardano — là nel titolo di un
/// video, qui in quello di una pubblicazione.
///
/// L'elenco è chiuso e corto di proposito. «Single» non c'è: dice come è stato
/// pubblicato, non cosa si sente. «Deluxe» e «Remaster» nemmeno: sono lo stesso
/// nastro.
const VARIANTI: &[&str] = &[
    "live",
    "dal vivo",
    "unplugged",
    "acoustic",
    "acustico",
    "demo",
    "karaoke",
    "instrumental",
    "strumentale",
    "remix",
    "mashup",
    "cover",
    "tribute",
    "tributo",
    "nightcore",
    "sped up",
    "slowed",
    "reprise",
];

/// La maschera delle varianti che un testo dichiara.
///
/// Un bit per voce di [`VARIANTI`]. Due maschere diverse fra il brano locale e
/// il candidato sono un veto: la base karaoke di una canzone dura quanto la
/// canzone e si chiama come la canzone, e senza questo confronto è
/// indistinguibile da lei su ogni asse che il punteggio sappia misurare.
///
/// Chi chiama unisce le maschere di **titolo e album** prima di confrontarle,
/// perché un disco dal vivo lo dichiara spesso solo nel titolo del disco:
/// `varianti(titolo) | varianti(album)`. Senza quell'unione ogni traccia di un
/// live si vedrebbe negare la corrispondenza che il disco intero conferma.
///
/// ```
/// use aether_domain::enrich::varianti;
/// assert_eq!(varianti("Poetica"), varianti("Poetica (Remastered)"));
/// assert_ne!(varianti("Poetica"), varianti("Poetica (Karaoke Version)"));
/// ```
#[must_use]
pub fn varianti(testo: &str) -> u32 {
    let piegato = fold_text(testo);
    let mut maschera = 0_u32;
    for (posizione, parola) in VARIANTI.iter().enumerate() {
        let presente = if parola.contains(' ') {
            // «sped up» e «dal vivo» sono due parole: il confine si controlla a
            // mano, perché la divisione per parole le spezzerebbe.
            piegato.contains(parola)
        } else {
            contiene_parola(&piegato, parola)
        };
        if presente {
            maschera |= 1_u32 << (posizione % 32);
        }
    }
    maschera
}

/// Il testo piegato contiene la parola **intera**.
///
/// Per parola intera e non per sottostringa, per la stessa ragione che
/// [`crate::album`] documenta a proposito di `(Deep Cuts)`: cercare `live`
/// dentro `Delivery` o `cover` dentro `Discover` toglierebbe la corrispondenza
/// a due titoli che non hanno niente a che fare con una variante.
fn contiene_parola(piegato: &str, parola: &str) -> bool {
    piegato
        .split(|c: char| !c.is_alphanumeric())
        .any(|token| token == parola)
}

// ── somiglianza ─────────────────────────────────────────────────────────────

/// Quanto due testi si somigliano, da 0 a 1.
///
/// Coefficiente di Sørensen–Dice sui bigrammi dei testi normalizzati. Si
/// preferisce alla distanza di edizione per un motivo pratico: è insensibile
/// all'ordine delle parole, e «Bowie, David» contro «David Bowie» è una forma
/// che i tag producono davvero.
#[must_use]
pub fn similarity(a: &str, b: &str) -> f64 {
    dice(&normalize_for_match(a), &normalize_for_match(b))
}

/// Come [`similarity`], su testi **già** normalizzati.
///
/// Esiste perché l'abbinamento di un album fa `n × m` confronti — dodici file
/// contro dodici tracce, per tre candidati — e normalizzare dentro il ciclo
/// vorrebbe dire rifare centoquarantaquattro volte un lavoro che si fa
/// ventiquattro.
#[must_use]
pub fn dice(a: &str, b: &str) -> f64 {
    if a == b {
        return if a.is_empty() { 0.0 } else { 1.0 };
    }
    let (ca, cb) = (a.chars().count(), b.chars().count());
    if ca < 2 || cb < 2 {
        return 0.0;
    }
    let ba = bigrammi(a);
    let bb = bigrammi(b);
    let mut comuni = 0_usize;
    for (bigramma, quante) in &ba {
        if let Some(altre) = bb.get(bigramma) {
            comuni = comuni.saturating_add(*quante.min(altre));
        }
    }
    let totale = ca.saturating_sub(1).saturating_add(cb.saturating_sub(1));
    if totale == 0 {
        return 0.0;
    }
    2.0 * comuni as f64 / totale as f64
}

/// I bigrammi di caratteri di un testo, con quante volte compaiono.
///
/// Di caratteri e non di byte: su un titolo giapponese i byte darebbero bigrammi
/// che tagliano i caratteri a metà, e la somiglianza fra due scritture dello
/// stesso nome diventerebbe rumore.
fn bigrammi(testo: &str) -> HashMap<(char, char), usize> {
    let mut mappa = HashMap::new();
    let mut precedente: Option<char> = None;
    for c in testo.chars() {
        if let Some(prima) = precedente {
            *mappa.entry((prima, c)).or_insert(0_usize) += 1;
        }
        precedente = Some(c);
    }
    mappa
}

/// Quanto due durate concordano, da 0 a 1; 0.5 quando una non si sa.
///
/// Il neutro a metà è per il **punteggio**, dove serve a non premiare né punire
/// un candidato per un dato che manca. Nella decisione la stessa assenza si
/// scrive `None` e non conta verso niente: vedi la nota in testa al modulo.
///
/// Pubblica perché la usa anche [`crate::testo::scegli`], che sceglie fra le
/// risposte di un catalogo di testi con la stessa domanda — quanto ci credo che
/// questi due siano lo stesso brano — e una seconda curva scritta là darebbe due
/// idee diverse di «la durata concorda» nello stesso programma.
#[must_use]
pub fn punteggio_durata(a: Option<u64>, b: Option<u64>) -> f64 {
    let (Some(a), Some(b)) = (a.filter(|d| *d > 0), b.filter(|d| *d > 0)) else {
        return 0.5;
    };
    1.0 - penalita_durata(a.abs_diff(b))
}

/// Quanto pesa uno scarto di durata, da 0 (nullo) a 1 (massimo).
///
/// Piatta fino a [`GRAZIA_MS`], poi lineare fino a [`SCARTO_MASSIMO_MS`]. La
/// zona piatta non è tolleranza generosa: è il rumore vero fra due codifiche
/// dello stesso master, e senza di lei ogni traccia di ogni disco porterebbe una
/// penalità che non dice niente.
fn penalita_durata(scarto_ms: u64) -> f64 {
    if scarto_ms <= GRAZIA_MS {
        return 0.0;
    }
    if scarto_ms >= SCARTO_MASSIMO_MS {
        return 1.0;
    }
    let oltre = scarto_ms.saturating_sub(GRAZIA_MS) as f64;
    let campo = SCARTO_MASSIMO_MS.saturating_sub(GRAZIA_MS) as f64;
    if campo <= 0.0 { 1.0 } else { oltre / campo }
}

/// Lo scarto fra due durate in secondi; `None` se una delle due non si sa.
#[must_use]
pub fn scarto_secondi(a: Option<u64>, b: Option<u64>) -> Option<f64> {
    let (a, b) = (a.filter(|d| *d > 0)?, b.filter(|d| *d > 0)?);
    Some(a.abs_diff(b) as f64 / 1000.0)
}

// ── quel che si confronta ───────────────────────────────────────────────────

/// Un brano della libreria, ridotto a quel che serve per riconoscerlo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalTrack {
    /// L'identificativo della riga, per ritrovarla dopo la decisione.
    pub id: i64,
    /// Il titolo, dal tag.
    pub title: String,
    /// L'interprete, dal tag.
    pub artist: String,
    /// L'album, dal tag.
    pub album: Option<String>,
    /// L'interprete dell'album.
    pub album_artist: Option<String>,
    /// La durata in millisecondi. Zero o assente significa «non la so».
    pub duration_ms: Option<u64>,
    /// Il numero di traccia dichiarato.
    pub track_number: Option<u32>,
    /// Il numero di disco dichiarato.
    pub disc_number: Option<u32>,
    /// L'anno.
    pub year: Option<i32>,
    /// Il genere.
    pub genre: Option<String>,
    /// La radice del nome del file, per riconoscere il titolo di ripiego.
    ///
    /// `read_track` usa il nome del file quando il tag non ha un titolo: un
    /// titolo uguale a questa stringa non è un titolo, è quel ripiego.
    pub file_stem: Option<String>,
    /// Ha già una copertina.
    pub has_cover: bool,
    /// Porta già un identificativo MusicBrainz della registrazione.
    pub has_mb_id: bool,
}

/// Un gruppo d'album della libreria, da abbinare a una pubblicazione.
#[derive(Debug, Clone, Copy)]
pub struct LocalAlbum<'a> {
    /// Il titolo dell'album, come lo dicono i tag.
    pub title: &'a str,
    /// L'interprete canonico del gruppo.
    pub artist: &'a str,
    /// I brani, nell'ordine in cui stanno in libreria.
    pub tracks: &'a [LocalTrack],
}

/// Una traccia di una pubblicazione trovata in rete.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteTrack {
    /// Il titolo.
    pub title: String,
    /// L'interprete della traccia, quando la pubblicazione lo distingue.
    pub artist: Option<String>,
    /// La durata in millisecondi.
    pub duration_ms: Option<u64>,
    /// Il numero di traccia dentro il suo disco.
    pub track_number: Option<u32>,
    /// Il numero di disco.
    pub disc_number: Option<u32>,
    /// L'identificativo MusicBrainz della registrazione.
    pub mb_recording_id: Option<String>,
}

/// Una pubblicazione trovata in rete.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteRelease {
    /// Il titolo.
    pub title: String,
    /// L'interprete.
    pub artist: String,
    /// L'anno di pubblicazione.
    pub year: Option<i32>,
    /// Le tracce, appiattite su tutti i dischi.
    pub tracks: Vec<RemoteTrack>,
    /// L'identificativo MusicBrainz della pubblicazione.
    pub mb_release_id: Option<String>,
    /// L'identificativo MusicBrainz del gruppo di pubblicazione.
    pub mb_release_group_id: Option<String>,
}

/// Da quale servizio arriva un candidato.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FonteMeta {
    /// MusicBrainz.
    MusicBrainz,
    /// iTunes Search.
    Itunes,
    /// Deezer.
    Deezer,
}

impl FonteMeta {
    /// Il nome che finisce nel database e nei registri.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MusicBrainz => "musicbrainz",
            Self::Itunes => "itunes",
            Self::Deezer => "deezer",
        }
    }
}

/// Un candidato per un singolo brano.
///
/// `fonte` è un'opzione e non un valore: il consenso si misura fra fonti
/// **diverse**, e un candidato senza fonte dichiarata non può fare consenso con
/// nessuno — che è il comportamento giusto per un candidato costruito a mano in
/// una prova.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Candidate {
    /// Chi lo propone.
    pub fonte: Option<FonteMeta>,
    /// Il titolo.
    pub title: String,
    /// L'interprete.
    pub artist: String,
    /// L'album.
    pub album: Option<String>,
    /// L'anno.
    pub year: Option<i32>,
    /// Il genere, quando la fonte ne dà uno.
    pub genre: Option<String>,
    /// La durata in millisecondi.
    pub duration_ms: Option<u64>,
    /// Un indirizzo diretto della copertina, quando la fonte ne dà uno.
    pub cover_url: Option<String>,
    /// L'identificativo MusicBrainz della registrazione.
    pub mb_recording_id: Option<String>,
    /// L'identificativo MusicBrainz della pubblicazione.
    pub mb_release_id: Option<String>,
    /// L'identificativo MusicBrainz del gruppo di pubblicazione.
    pub mb_release_group_id: Option<String>,
}

// ── l'abbinamento di un album ───────────────────────────────────────────────

/// I fatti su cui si decide se un album corrisponde.
///
/// Separati dalla distanza di proposito: la distanza è un numero che ordina i
/// candidati fra loro, questi sono le prove. Una decisione presa sul solo numero
/// applicherebbe il migliore di tre candidati tutti sbagliati.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AlbumEvidence {
    /// Quanti brani ha il gruppo in libreria.
    pub local_tracks: usize,
    /// Quante tracce ha la pubblicazione.
    pub release_tracks: usize,
    /// Quanti brani hanno trovato una traccia.
    pub assigned: usize,
    /// Quanti l'hanno trovata con un titolo che concorda fortemente.
    pub strong_titles: usize,
    /// Il peggior scarto di durata fra i brani assegnati, in secondi.
    pub worst_duration_delta_sec: Option<f64>,
    /// Quanto concordano i titoli dei due album.
    pub album_sim: f64,
    /// Quanto concordano gli interpreti; `None` se quello locale è ignoto.
    pub artist_sim: Option<f64>,
    /// Le varianti dichiarate dai due lati discordano.
    pub variant_mismatch: bool,
}

/// Il risultato dell'abbinamento fra un gruppo e una pubblicazione.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AlbumMatch {
    /// Per ogni brano locale, la traccia della pubblicazione che gli tocca.
    pub assignment: Vec<Option<usize>>,
    /// Quanto i due si somigliano poco, da 0 (identici) a 1.
    pub distance: f64,
    /// I fatti.
    pub evidence: AlbumEvidence,
}

/// Un asse della distanza: quanto pesa e quanto ha penalizzato.
struct Asse {
    peso: f64,
    penalita: f64,
}

/// La distanza fra un gruppo d'album e una pubblicazione candidata.
///
/// # Come si assegnano le tracce
///
/// Due strade, e si tiene quella che produce la distanza minore.
///
/// La prima è **per numero**: quando ogni brano e ogni traccia dichiarano il
/// proprio posto, il posto è l'abbinamento. È quella giusta sui dischi taggati
/// bene, e in particolare è l'unica che sa distinguere due tracce che si
/// chiamano quasi uguale (una `Intro` e una `Outro` di trenta secondi l'una).
///
/// La seconda è **avida per somiglianza**: si ordinano tutte le coppie per
/// quanto si somigliano e si prendono dall'alto finché entrambi i lati sono
/// liberi. Serve ai dischi senza numeri, che sono la maggioranza di quelli
/// scaricati.
///
/// Provarle entrambe costa il doppio di un lavoro che su dodici tracce è
/// microsecondi, e in cambio toglie il caso peggiore di ognuna delle due: un
/// disco numerato male non manda a monte l'abbinamento, e un disco con due
/// tracce omonime non si scambia le durate.
///
/// # Perché la distanza si normalizza sui pesi applicati
///
/// Un asse che non si può misurare — l'anno che nessuno dei due dichiara,
/// l'interprete ignoto — esce dalla somma **e** dal divisore. Sommarlo come zero
/// premierebbe i dischi di cui si sa poco, che è il contrario di quel che serve.
#[must_use]
pub fn album_distance(locale: &LocalAlbum<'_>, remota: &RemoteRelease) -> AlbumMatch {
    let titoli_locali: Vec<String> = locale
        .tracks
        .iter()
        .map(|t| normalize_for_match(&t.title))
        .collect();
    let titoli_remoti: Vec<String> = remota
        .tracks
        .iter()
        .map(|t| normalize_for_match(&t.title))
        .collect();

    let per_numero = assegna_per_numero(locale.tracks, &remota.tracks);
    let avida = assegna_avida(
        locale.tracks,
        &remota.tracks,
        &titoli_locali,
        &titoli_remoti,
    );

    let mut migliore: Option<AlbumMatch> = None;
    for assegnamento in [per_numero, avida].into_iter().flatten() {
        let candidato = valuta(locale, remota, &titoli_locali, &titoli_remoti, assegnamento);
        let sostituisci = migliore
            .as_ref()
            .is_none_or(|attuale| candidato.distance < attuale.distance);
        if sostituisci {
            migliore = Some(candidato);
        }
    }

    migliore.unwrap_or_else(|| {
        // Nessun assegnamento possibile: il gruppo è vuoto. Distanza massima e
        // nessuna prova, così chi decide non ha niente su cui applicare.
        AlbumMatch {
            assignment: vec![None; locale.tracks.len()],
            distance: 1.0,
            evidence: AlbumEvidence {
                local_tracks: locale.tracks.len(),
                release_tracks: remota.tracks.len(),
                ..AlbumEvidence::default()
            },
        }
    })
}

/// L'assegnamento per numero di traccia e disco.
///
/// `None` quando anche un solo lato non dichiara i numeri: un assegnamento
/// parziale per numero sarebbe peggio di nessuno, perché lascerebbe fuori
/// proprio le tracce su cui l'altro metodo funziona.
fn assegna_per_numero(locali: &[LocalTrack], remote: &[RemoteTrack]) -> Option<Vec<Option<usize>>> {
    if locali.is_empty() || remote.is_empty() {
        return None;
    }
    let posto = |disco: Option<u32>, traccia: Option<u32>| -> Option<(u32, u32)> {
        // Il disco assente vale «disco 1»: la stragrande maggioranza dei dischi
        // è singola e non lo dichiara, e pretenderlo escluderebbe questa strada
        // proprio dove funziona meglio.
        Some((disco.unwrap_or(1), traccia?))
    };
    let mut per_posto: HashMap<(u32, u32), usize> = HashMap::new();
    for (indice, traccia) in remote.iter().enumerate() {
        let chiave = posto(traccia.disc_number, traccia.track_number)?;
        // Due tracce nello stesso posto: la pubblicazione è malformata e questa
        // strada non si può percorrere.
        if per_posto.insert(chiave, indice).is_some() {
            return None;
        }
    }
    let mut fuori = Vec::with_capacity(locali.len());
    let mut presi: Vec<bool> = vec![false; remote.len()];
    for brano in locali {
        let chiave = posto(brano.disc_number, brano.track_number)?;
        match per_posto.get(&chiave) {
            Some(&indice) if !presi.get(indice).copied().unwrap_or(true) => {
                if let Some(preso) = presi.get_mut(indice) {
                    *preso = true;
                }
                fuori.push(Some(indice));
            }
            _ => fuori.push(None),
        }
    }
    Some(fuori)
}

/// L'assegnamento avido per somiglianza di titolo e durata.
fn assegna_avida(
    locali: &[LocalTrack],
    remote: &[RemoteTrack],
    titoli_locali: &[String],
    titoli_remoti: &[String],
) -> Option<Vec<Option<usize>>> {
    if locali.is_empty() || remote.is_empty() {
        return None;
    }
    let mut coppie: Vec<(usize, usize, f64)> = Vec::with_capacity(locali.len() * remote.len());
    for (i, brano) in locali.iter().enumerate() {
        for (j, traccia) in remote.iter().enumerate() {
            let titolo = match (titoli_locali.get(i), titoli_remoti.get(j)) {
                (Some(a), Some(b)) => dice(a, b),
                _ => 0.0,
            };
            let durata = punteggio_durata(brano.duration_ms, traccia.duration_ms);
            coppie.push((i, j, 0.7 * titolo + 0.3 * durata));
        }
    }
    // Decrescente per punteggio; a pari merito prima l'indice locale e poi
    // quello remoto. Il criterio di pareggio non cambia quale abbinamento sia
    // giusto — cambia che sia sempre lo **stesso**, e un piano che cambia fra
    // due esecuzioni non si può confrontare né riprodurre in un test.
    coppie.sort_by(|a, b| {
        b.2.partial_cmp(&a.2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
            .then(a.1.cmp(&b.1))
    });

    let mut fuori: Vec<Option<usize>> = vec![None; locali.len()];
    let mut presi: Vec<bool> = vec![false; remote.len()];
    for (i, j, punteggio) in coppie {
        // Zero vuol dire «non si somigliano per niente»: assegnarle
        // riempirebbe le caselle senza portare nessuna prova.
        if punteggio <= 0.0 {
            continue;
        }
        if fuori.get(i).copied().flatten().is_some() || presi.get(j).copied().unwrap_or(true) {
            continue;
        }
        if let Some(cella) = fuori.get_mut(i) {
            *cella = Some(j);
        }
        if let Some(preso) = presi.get_mut(j) {
            *preso = true;
        }
    }
    Some(fuori)
}

/// Calcola distanza e prove per un assegnamento dato.
fn valuta(
    locale: &LocalAlbum<'_>,
    remota: &RemoteRelease,
    titoli_locali: &[String],
    titoli_remoti: &[String],
    assignment: Vec<Option<usize>>,
) -> AlbumMatch {
    let mut assi: Vec<Asse> = Vec::new();

    // ── gli assi dell'album ──
    // Il titolo si confronta senza la coda di edizione: `Abbey Road (Deluxe
    // Edition)` e `Abbey Road` sono lo stesso disco, ed è già ciò che
    // `album_key` assume per raggrupparli.
    let album_sim = dice(
        &normalize_for_match(&strip_edition_suffix(locale.title)),
        &normalize_for_match(&strip_edition_suffix(&remota.title)),
    );
    assi.push(Asse {
        peso: P_ALBUM,
        penalita: 1.0 - album_sim,
    });

    let artista_noto = !locale.artist.trim().is_empty() && locale.artist != UNKNOWN_ARTIST;
    let artist_sim = artista_noto.then(|| similarity(locale.artist, &remota.artist));
    if let Some(sim) = artist_sim {
        assi.push(Asse {
            peso: P_ARTISTA,
            penalita: 1.0 - sim,
        });
    }

    let quanti_locali = locale.tracks.len();
    let quanti_remoti = remota.tracks.len();
    let massimo = quanti_locali.max(quanti_remoti);
    if massimo > 0 {
        let scarto = quanti_locali.abs_diff(quanti_remoti) as f64 / massimo as f64;
        assi.push(Asse {
            peso: P_CONTEGGIO,
            penalita: scarto.min(1.0),
        });
    }

    let assegnati = assignment.iter().filter(|a| a.is_some()).count();
    if quanti_remoti > 0 {
        let mancanti = quanti_remoti.saturating_sub(assegnati) as f64 / quanti_remoti as f64;
        assi.push(Asse {
            peso: P_MANCANTI,
            penalita: mancanti.min(1.0),
        });
    }
    if quanti_locali > 0 {
        let spaiati = quanti_locali.saturating_sub(assegnati) as f64 / quanti_locali as f64;
        assi.push(Asse {
            peso: P_SPAIATI,
            penalita: spaiati.min(1.0),
        });
    }

    // L'anno di un solo lato non penalizza: metà dei tag non ce l'ha, e
    // pretenderlo escluderebbe i dischi taggati peggio, cioè quelli che
    // l'arricchimento serve di più.
    let anno_locale = locale.tracks.iter().filter_map(|t| t.year).max();
    if let (Some(a), Some(b)) = (anno_locale, remota.year) {
        let scarto = a.abs_diff(b);
        assi.push(Asse {
            peso: P_ANNO,
            // Un anno di scarto è normale: una ristampa europea esce l'anno
            // dopo di quella americana, e i tag riportano quella che capita.
            penalita: match scarto {
                0 => 0.0,
                1 => 0.4,
                _ => 1.0,
            },
        });
    }

    // ── gli assi delle tracce, mediati ──
    // Mediati e non sommati: sommandoli, un disco di venti tracce peserebbe il
    // doppio di uno di dieci sugli stessi assi, e la distanza smetterebbe di
    // essere confrontabile fra candidati con conteggi diversi.
    let mut pena_titoli = 0.0_f64;
    let mut pena_durate = 0.0_f64;
    let mut pena_indici = 0.0_f64;
    let mut quanti_indici = 0_usize;
    let mut titoli_forti = 0_usize;
    let mut peggior_scarto: Option<f64> = None;

    for (i, destinazione) in assignment.iter().enumerate() {
        let Some(j) = *destinazione else { continue };
        let (Some(brano), Some(traccia)) = (locale.tracks.get(i), remota.tracks.get(j)) else {
            continue;
        };
        let titolo = match (titoli_locali.get(i), titoli_remoti.get(j)) {
            (Some(a), Some(b)) => dice(a, b),
            _ => 0.0,
        };
        pena_titoli += 1.0 - titolo;
        if titolo >= TITOLO_FORTE {
            titoli_forti = titoli_forti.saturating_add(1);
        }

        match (brano.duration_ms.filter(|d| *d > 0), traccia.duration_ms) {
            (Some(a), Some(b)) => {
                pena_durate += penalita_durata(a.abs_diff(b));
                let scarto = a.abs_diff(b) as f64 / 1000.0;
                peggior_scarto = Some(peggior_scarto.map_or(scarto, |p: f64| p.max(scarto)));
            }
            // Una durata che manca non penalizza e non premia: entra come
            // metà, che è il neutro del punteggio.
            _ => pena_durate += 0.5,
        }

        if let (Some(a), Some(b)) = (brano.track_number, traccia.track_number) {
            quanti_indici = quanti_indici.saturating_add(1);
            pena_indici += if a == b { 0.0 } else { 1.0 };
        }
    }

    if assegnati > 0 {
        let quanti = assegnati as f64;
        assi.push(Asse {
            peso: P_TITOLO,
            penalita: pena_titoli / quanti,
        });
        assi.push(Asse {
            peso: P_DURATA,
            penalita: pena_durate / quanti,
        });
    }
    if quanti_indici > 0 {
        assi.push(Asse {
            peso: P_INDICE,
            penalita: pena_indici / quanti_indici as f64,
        });
    }

    let peso_totale: f64 = assi.iter().map(|a| a.peso).sum();
    let distance = if peso_totale > 0.0 {
        assi.iter()
            .map(|a| a.peso * a.penalita.clamp(0.0, 1.0))
            .sum::<f64>()
            / peso_totale
    } else {
        1.0
    };

    let maschera_locale = varianti(locale.title)
        | locale
            .tracks
            .iter()
            .fold(0_u32, |acc, t| acc | varianti(&t.title));
    let maschera_remota = varianti(&remota.title)
        | remota
            .tracks
            .iter()
            .fold(0_u32, |acc, t| acc | varianti(&t.title));

    AlbumMatch {
        assignment,
        distance,
        evidence: AlbumEvidence {
            local_tracks: quanti_locali,
            release_tracks: quanti_remoti,
            assigned: assegnati,
            strong_titles: titoli_forti,
            worst_duration_delta_sec: peggior_scarto,
            album_sim,
            artist_sim,
            variant_mismatch: maschera_locale != maschera_remota,
        },
    }
}

// ── la decisione ────────────────────────────────────────────────────────────

/// Cosa fare di una corrispondenza.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Verdetto {
    /// Le prove ci sono: si può scrivere.
    Applica,
    /// C'è un candidato plausibile e non provato. **Non si scrive niente.**
    DaRivedere,
    /// Non c'è niente.
    Nessuno,
}

impl Verdetto {
    /// Il nome che finisce in `tracks.enrich_status`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Applica => "ok",
            Self::DaRivedere => "needs-review",
            Self::Nessuno => "no-match",
        }
    }
}

/// Se un album abbinato si può applicare.
///
/// Cinque condizioni **tutte insieme**, e ognuna esclude un modo concreto di
/// sbagliare:
///
/// - titolo e interprete dell'album concordano — altrimenti è un altro disco;
/// - il numero di tracce è **esatto** — un disco a cui ne manca una è
///   un'edizione diversa, e applicarla scrive i numeri di traccia sbagliati su
///   tutto il resto;
/// - almeno quattro brani su cinque hanno trovato la loro traccia con un titolo
///   che concorda — sotto, si sta abbinando una raccolta a un album vero;
/// - nessun brano scarta di più di [`VETO_DURATA_SEC`] — è la versione
///   allungata, o il disco dal vivo;
/// - le varianti dichiarate coincidono — vedi la nota in testa al modulo.
///
/// La distanza serve da ultimo controllo: sotto [`DISTANZA_APPLICA`] si scrive,
/// sotto [`DISTANZA_PLAUSIBILE`] si aspetta, oltre non c'è niente.
#[must_use]
pub fn decide_album(abbinamento: &AlbumMatch) -> Verdetto {
    let prove = &abbinamento.evidence;
    let titolo_forte = prove.album_sim >= TITOLO_FORTE;
    let artista_forte = prove.artist_sim.is_some_and(|s| s >= ARTISTA_FORTE);
    let conteggio_esatto = prove.local_tracks == prove.release_tracks && prove.release_tracks > 0;
    // Quattro su cinque, scritto come prodotto incrociato per non dividere fra
    // interi: `strong_titles / local_tracks >= 0.8` troncato direbbe di sì su
    // tre brani su quattro.
    let quasi_tutte = prove.local_tracks > 0
        && prove.strong_titles.saturating_mul(5) >= prove.local_tracks.saturating_mul(4);
    let durate_sane = prove
        .worst_duration_delta_sec
        .is_none_or(|scarto| scarto <= VETO_DURATA_SEC);

    if titolo_forte
        && artista_forte
        && conteggio_esatto
        && quasi_tutte
        && durate_sane
        && !prove.variant_mismatch
        && abbinamento.distance <= DISTANZA_APPLICA
    {
        return Verdetto::Applica;
    }
    if abbinamento.distance <= DISTANZA_PLAUSIBILE {
        Verdetto::DaRivedere
    } else {
        Verdetto::Nessuno
    }
}

/// I fatti su cui si decide se un singolo brano corrisponde.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TrackEvidence {
    /// Quanto concordano i titoli.
    pub title_sim: f64,
    /// Quanto concordano gli interpreti; `None` se quello locale è ignoto.
    pub artist_sim: Option<f64>,
    /// Quanto concordano gli album; `None` se uno dei due manca.
    pub album_sim: Option<f64>,
    /// Lo scarto di durata in secondi; `None` se una delle due manca.
    pub duration_delta_sec: Option<f64>,
    /// Un'altra fonte nomina la stessa registrazione.
    pub consensus: bool,
    /// Le varianti dichiarate discordano.
    pub variant_mismatch: bool,
}

/// Se un candidato per un singolo brano si può applicare.
///
/// # Perché il consenso è obbligatorio, e nel vecchio albero non lo era
///
/// Là bastavano due assi forti fra titolo, interprete e durata
/// (`decision.ts:76`). Quella regola reggeva perché sopra di lei c'era
/// un'impronta acustica a raccogliere i casi che sbagliava. Senza chiavi API
/// l'impronta non c'è, e due assi forti sono esattamente ciò che una base
/// karaoke — stesso titolo, stessa durata — soddisfa senza essere il brano.
///
/// Qui l'accordo fra **due fonti diverse** prende quel posto: un titolo che
/// MusicBrainz e Deezer nominano allo stesso modo, con lo stesso album o la
/// stessa durata, è una registrazione che esiste davvero e si chiama così.
///
/// Il consenso però parla dei candidati fra loro, non del **nostro file**.
/// Serve quindi anche un'àncora al file: l'interprete che concorda, oppure una
/// durata entro [`ANCORA_DURATA_SEC`]. Senza, «Yesterday» in libreria si
/// aggancerebbe a «Yesterday» di chiunque.
#[must_use]
pub fn decide_track(prove: &TrackEvidence, punteggio: f64) -> Verdetto {
    if punteggio < SOGLIA_CANDIDATO {
        return Verdetto::Nessuno;
    }
    let smentita = prove.artist_sim.is_some_and(|s| s < SMENTITA)
        || prove.album_sim.is_some_and(|s| s < SMENTITA);
    let titolo_forte = prove.title_sim >= TITOLO_FORTE;
    let ancorato = prove.artist_sim.is_some_and(|s| s >= ARTISTA_FORTE)
        || prove
            .duration_delta_sec
            .is_some_and(|scarto| scarto <= ANCORA_DURATA_SEC);

    if titolo_forte && ancorato && prove.consensus && !smentita && !prove.variant_mismatch {
        Verdetto::Applica
    } else {
        Verdetto::DaRivedere
    }
}

/// Il candidato scelto per un brano, con le sue prove.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackMatch {
    /// La posizione del candidato scelto nell'elenco ricevuto.
    pub best: usize,
    /// Il punteggio composito, da 0 a 1.
    pub score: f64,
    /// Le prove.
    pub evidence: TrackEvidence,
    /// Il verdetto.
    pub verdict: Verdetto,
}

/// Il punteggio composito di un candidato rispetto a un brano.
///
/// Titolo 0.4, interprete 0.3, durata 0.3 — gli stessi pesi del vecchio albero
/// (`match.ts:156`), e come là un interprete ignoto o una durata mancante valgono
/// il neutro 0.5. Serve a **ordinare**, non a decidere.
#[must_use]
pub fn score_candidate(brano: &LocalTrack, candidato: &Candidate) -> f64 {
    let titolo = similarity(&brano.title, &candidato.title);
    let artista_noto = !brano.artist.trim().is_empty() && brano.artist != UNKNOWN_ARTIST;
    let artista = if artista_noto && !candidato.artist.is_empty() {
        similarity(&brano.artist, &candidato.artist)
    } else {
        0.5
    };
    0.4 * titolo + 0.3 * artista + 0.3 * punteggio_durata(brano.duration_ms, candidato.duration_ms)
}

/// Due candidati nominano la stessa registrazione.
///
/// Stretto di proposito, come `agreesStrictly` (`resolve.ts:51`): titolo **e**
/// interprete normalizzati identici, più un accordo su album *oppure* durata.
/// Titolo e interprete da soli appaierebbero la versione in studio con quella di
/// una raccolta karaoke dello stesso interprete — e il consenso, che qui è la
/// prova principale, si fonderebbe su un accordo che non dice niente.
#[must_use]
pub fn concordano(a: &Candidate, b: &Candidate) -> bool {
    if normalize_for_match(&a.title) != normalize_for_match(&b.title) {
        return false;
    }
    if normalize_for_match(&a.artist) != normalize_for_match(&b.artist) {
        return false;
    }
    let album_concorda = match (a.album.as_deref(), b.album.as_deref()) {
        (Some(x), Some(y)) if !x.is_empty() && !y.is_empty() => {
            normalize_for_match(x) == normalize_for_match(y)
        }
        _ => false,
    };
    let durata_concorda = scarto_secondi(a.duration_ms, b.duration_ms)
        .is_some_and(|scarto| scarto <= ANCORA_DURATA_SEC);
    album_concorda || durata_concorda
}

/// Sceglie fra i candidati e decide.
///
/// `None` quando l'elenco è vuoto. Un elenco pieno di candidati pessimi
/// restituisce comunque un [`TrackMatch`], con verdetto [`Verdetto::Nessuno`]:
/// chi chiama ha bisogno di distinguere «non ho chiesto» da «ho chiesto e non
/// c'è», perché la seconda si ricorda e la prima no.
#[must_use]
pub fn resolve_track(brano: &LocalTrack, candidati: &[Candidate]) -> Option<TrackMatch> {
    let mut migliore: Option<(usize, f64)> = None;
    for (indice, candidato) in candidati.iter().enumerate() {
        let punteggio = score_candidate(brano, candidato);
        // `>` e non `>=`: a pari punteggio vince il primo, e l'ordine in cui
        // arrivano è quello in cui chi chiama ha interrogato le fonti.
        if migliore.is_none_or(|(_, attuale)| punteggio > attuale) {
            migliore = Some((indice, punteggio));
        }
    }
    let (best, score) = migliore?;
    let candidato = candidati.get(best)?;

    let consensus = candidati
        .iter()
        .enumerate()
        .any(|(altro, c)| altro != best && c.fonte != candidato.fonte && concordano(c, candidato));

    let artista_noto = !brano.artist.trim().is_empty() && brano.artist != UNKNOWN_ARTIST;
    let album_noto = brano
        .album
        .as_deref()
        .is_some_and(|a| !a.trim().is_empty() && a != UNKNOWN_ALBUM);

    let maschera_locale = varianti(&brano.title) | brano.album.as_deref().map_or(0, varianti);
    let maschera_remota =
        varianti(&candidato.title) | candidato.album.as_deref().map_or(0, varianti);

    let evidence = TrackEvidence {
        title_sim: similarity(&brano.title, &candidato.title),
        artist_sim: (artista_noto && !candidato.artist.is_empty())
            .then(|| similarity(&brano.artist, &candidato.artist)),
        album_sim: match (album_noto, candidato.album.as_deref()) {
            (true, Some(remoto)) if !remoto.is_empty() => Some(similarity(
                brano.album.as_deref().unwrap_or_default(),
                remoto,
            )),
            _ => None,
        },
        duration_delta_sec: scarto_secondi(brano.duration_ms, candidato.duration_ms),
        consensus,
        variant_mismatch: maschera_locale != maschera_remota,
    };

    Some(TrackMatch {
        best,
        score,
        verdict: decide_track(&evidence, score),
        evidence,
    })
}

// ── il piano di scrittura ───────────────────────────────────────────────────

/// I campi che si possono scrivere su un brano.
///
/// Ogni `None` vuol dire «non lo so» in entrata e «non toccarlo» in uscita.
/// Non esiste un valore che significhi «svuotalo»: vedi [`plan_write`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fields {
    /// Il titolo.
    pub title: Option<String>,
    /// L'interprete.
    pub artist: Option<String>,
    /// L'album.
    pub album: Option<String>,
    /// L'interprete dell'album.
    pub album_artist: Option<String>,
    /// L'anno.
    pub year: Option<i32>,
    /// Il genere.
    pub genre: Option<String>,
    /// Il numero di traccia.
    pub track_number: Option<u32>,
    /// Il numero di disco.
    pub disc_number: Option<u32>,
    /// L'identificativo MusicBrainz della registrazione.
    pub mb_recording_id: Option<String>,
    /// L'identificativo MusicBrainz della pubblicazione.
    pub mb_release_id: Option<String>,
    /// L'identificativo MusicBrainz del gruppo di pubblicazione.
    pub mb_release_group_id: Option<String>,
}

impl Fields {
    /// Non c'è niente da scrivere.
    ///
    /// Chi chiama la usa per non aprire un file, non toccarne la data di
    /// modifica e non scrivere una riga di annullamento per una scrittura che
    /// non cambierebbe niente.
    #[must_use]
    pub fn e_vuoto(&self) -> bool {
        self.title.is_none()
            && self.artist.is_none()
            && self.album.is_none()
            && self.album_artist.is_none()
            && self.year.is_none()
            && self.genre.is_none()
            && self.track_number.is_none()
            && self.disc_number.is_none()
            && self.mb_recording_id.is_none()
            && self.mb_release_id.is_none()
            && self.mb_release_group_id.is_none()
    }
}

/// Il valore è vuoto o è un segnaposto messo dalla scansione.
fn e_segnaposto(valore: Option<&str>, segnaposto: &str) -> bool {
    match valore.map(str::trim) {
        None | Some("") => true,
        Some(v) => v == segnaposto,
    }
}

/// Il titolo di un brano è un ripiego e non un titolo.
///
/// Tre modi in cui può esserlo, e sono i tre che si trovano in libreria:
/// è vuoto; è la radice del nome del file, che è il ripiego di `read_track`
/// quando il tag non ha un titolo; oppure porta il rumore di un caricamento su
/// YouTube, che [`titolo_da_cercare`] sa togliere.
#[must_use]
pub fn titolo_di_ripiego(titolo: &str, radice_del_file: Option<&str>) -> bool {
    let rifilato = titolo.trim_matches(is_js_whitespace);
    if rifilato.is_empty() {
        return true;
    }
    if radice_del_file.is_some_and(|radice| fold_text(radice.trim()) == fold_text(rifilato)) {
        return true;
    }
    titolo_da_cercare(rifilato) != collapse_whitespace(rifilato)
}

/// Quali campi scrivere davvero su un brano.
///
/// # La scala di prudenza
///
/// Tre regole, e la terza è quella che si dimentica:
///
/// 1. un campo **vuoto o di ripiego** si riempie sempre, anche con una
///    corrispondenza appena sufficiente: peggio di un dato incerto c'è solo
///    «Artista sconosciuto»;
/// 2. un campo **già scritto** si sostituisce solo con `sostituisci`, cioè
///    quando il verdetto è [`Verdetto::Applica`] — chi ha taggato a mano i suoi
///    file non deve vederseli riscrivere da una corrispondenza plausibile;
/// 3. un campo che il candidato **non conosce** non si tocca **mai**. Non
///    esiste un modo di dire «svuotalo», e non deve esistere: perdere un genere
///    scritto a mano per averlo cercato e non trovato sarebbe il modo peggiore
///    di arricchire.
///
/// # Il genere non si sostituisce mai
///
/// Si riempie se manca e si lascia stare se c'è, qualunque sia il verdetto. È il
/// campo su cui le fonti sono più in disaccordo fra loro — lo stesso brano è
/// «Rock», «Alternative Rock» e «Indie» a seconda di chi risponde — e l'unico su
/// cui la scelta dell'utente è più informata di quella di un servizio.
///
/// # Gli identificativi si aggiungono e basta
///
/// Un `mb_recording_id` che prima non c'era non toglie niente a nessuno, e vale
/// molto: è ciò con cui [`crate::album::build_album_groups`] **fonde** due
/// gruppi che erano lo stesso disco. Si scrivono anche senza `sostituisci`, ma
/// mai sopra a uno diverso già presente — quello lo ha messo chi ha taggato il
/// file con Picard, e ne sa più di noi.
#[must_use]
pub fn plan_write(brano: &LocalTrack, trovati: &Fields, sostituisci: bool) -> Fields {
    let testo = |attuale: Option<&str>, nuovo: Option<&String>, rimpiazzabile: bool| {
        let nuovo = nuovo
            .map(String::as_str)
            .map(str::trim)
            .filter(|n| !n.is_empty())?;
        if rimpiazzabile || sostituisci {
            // Non si riscrive un valore identico: aprirebbe il file, ne
            // cambierebbe la data di modifica e scriverebbe una riga di
            // annullamento per non cambiare niente.
            if attuale.map(str::trim) == Some(nuovo) {
                return None;
            }
            return Some(nuovo.to_owned());
        }
        None
    };

    let titolo_ripiego = titolo_di_ripiego(&brano.title, brano.file_stem.as_deref());
    let identificativo = |attuale: bool, nuovo: Option<&String>| {
        if attuale {
            return None;
        }
        nuovo
            .map(String::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToOwned::to_owned)
    };

    Fields {
        title: testo(Some(&brano.title), trovati.title.as_ref(), titolo_ripiego),
        artist: testo(
            Some(&brano.artist),
            trovati.artist.as_ref(),
            e_segnaposto(Some(&brano.artist), UNKNOWN_ARTIST),
        ),
        album: testo(
            brano.album.as_deref(),
            trovati.album.as_ref(),
            e_segnaposto(brano.album.as_deref(), UNKNOWN_ALBUM),
        ),
        album_artist: testo(
            brano.album_artist.as_deref(),
            trovati.album_artist.as_ref(),
            e_segnaposto(brano.album_artist.as_deref(), UNKNOWN_ARTIST),
        ),
        year: match (brano.year, trovati.year) {
            (_, None) => None,
            (None, Some(nuovo)) => Some(nuovo),
            (Some(attuale), Some(nuovo)) if sostituisci && attuale != nuovo => Some(nuovo),
            _ => None,
        },
        // Vedi la nota qui sopra: si riempie, non si sostituisce.
        genre: match (
            brano.genre.as_deref().map(str::trim),
            trovati.genre.as_ref(),
        ) {
            (None | Some(""), Some(nuovo)) if !nuovo.trim().is_empty() => {
                Some(nuovo.trim().to_owned())
            }
            _ => None,
        },
        track_number: match (brano.track_number, trovati.track_number) {
            (_, None) => None,
            (None, Some(nuovo)) => Some(nuovo),
            (Some(attuale), Some(nuovo)) if sostituisci && attuale != nuovo => Some(nuovo),
            _ => None,
        },
        disc_number: match (brano.disc_number, trovati.disc_number) {
            (_, None) => None,
            (None, Some(nuovo)) => Some(nuovo),
            (Some(attuale), Some(nuovo)) if sostituisci && attuale != nuovo => Some(nuovo),
            _ => None,
        },
        mb_recording_id: identificativo(brano.has_mb_id, trovati.mb_recording_id.as_ref()),
        mb_release_id: identificativo(brano.has_mb_id, trovati.mb_release_id.as_ref()),
        mb_release_group_id: identificativo(brano.has_mb_id, trovati.mb_release_group_id.as_ref()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brano(titolo: &str, artista: &str, durata_ms: u64) -> LocalTrack {
        LocalTrack {
            title: titolo.to_owned(),
            artist: artista.to_owned(),
            duration_ms: Some(durata_ms),
            ..LocalTrack::default()
        }
    }

    fn traccia(titolo: &str, numero: u32, durata_ms: u64) -> RemoteTrack {
        RemoteTrack {
            title: titolo.to_owned(),
            duration_ms: Some(durata_ms),
            track_number: Some(numero),
            disc_number: Some(1),
            ..RemoteTrack::default()
        }
    }

    fn candidato(fonte: FonteMeta, titolo: &str, artista: &str, durata_ms: u64) -> Candidate {
        Candidate {
            fonte: Some(fonte),
            title: titolo.to_owned(),
            artist: artista.to_owned(),
            duration_ms: Some(durata_ms),
            ..Candidate::default()
        }
    }

    // ── normalizzazione ──

    #[test]
    fn le_decorazioni_non_cambiano_l_identita() {
        assert_eq!(
            normalize_for_match("Poetica (Radio Edit)"),
            normalize_for_match("poetica")
        );
        assert_eq!(
            normalize_for_match("(Don't Fear) The Reaper"),
            normalize_for_match("the reaper")
        );
    }

    #[test]
    fn il_rumore_di_caricamento_se_ne_va_e_il_resto_resta() {
        assert_eq!(titolo_da_cercare("Poetica (Official Video)"), "Poetica");
        assert_eq!(titolo_da_cercare("Poetica [HD]"), "Poetica");
        assert_eq!(titolo_da_cercare("Radiohead - Topic"), "Radiohead");
        // Una parentesi che dice qualcosa sul brano resta: cercare la versione
        // dal vivo di un file dal vivo è la cosa giusta.
        assert_eq!(titolo_da_cercare("Poetica (Live)"), "Poetica (Live)");
        // …e nemmeno un titolo vero che contiene una parola di rumore si tocca.
        assert_eq!(titolo_da_cercare("Video Games"), "Video Games");
        assert_eq!(
            titolo_da_cercare("Everything (Video Games Mix)"),
            "Everything (Video Games Mix)"
        );
    }

    #[test]
    fn una_parentesi_non_chiusa_non_mangia_il_titolo() {
        assert_eq!(titolo_da_cercare("Poetica (Official"), "Poetica (Official");
    }

    // ── varianti ──

    #[test]
    fn una_base_karaoke_non_e_il_brano() {
        // Il caso che il vecchio albero sbagliava: stesso titolo, stessa
        // durata, e senza impronta acustica niente lo distingue — tranne che
        // uno dei due lo dice.
        assert_ne!(varianti("Poetica"), varianti("Poetica (Karaoke Version)"));
        assert_ne!(varianti("Poetica"), varianti("Poetica - Live"));
        assert_ne!(varianti("Poetica"), varianti("Poetica (Acoustic)"));
    }

    #[test]
    fn una_rimasterizzazione_e_lo_stesso_nastro() {
        assert_eq!(varianti("Poetica"), varianti("Poetica (2019 Remaster)"));
        assert_eq!(varianti("Poetica"), varianti("Poetica (Deluxe Edition)"));
        assert_eq!(varianti("Poetica"), varianti("Poetica - Single Version"));
    }

    #[test]
    fn una_variante_dentro_un_altra_parola_non_conta() {
        // «Delivery» contiene «live», «Discover» contiene «cover»: il confronto
        // è per parola intera, come in `album::looks_like_edition`.
        assert_eq!(varianti("Delivery"), 0);
        assert_eq!(varianti("Discover"), 0);
        assert_eq!(varianti("Recovery"), 0);
        assert_ne!(varianti("Live at Wembley"), 0);
    }

    // ── somiglianza ──

    #[test]
    fn la_somiglianza_ignora_l_ordine_delle_parole() {
        // «Bowie, David» è una forma che i tag producono davvero, ed è il motivo
        // per cui si usa Dice e non una distanza di edizione: quella la
        // vedrebbe come una stringa quasi del tutto diversa.
        //
        // Vale **esattamente** 0.8, cioè appena `ARTISTA_FORTE`: otto bigrammi
        // in comune su venti. Un nome invertito passa, ma senza un briciolo di
        // margine — ed è una delle ragioni per cui `decide_track` non si
        // accontenta dell'interprete e pretende anche il consenso fra due fonti.
        let invertito = similarity("David Bowie", "Bowie, David");
        assert!(
            invertito >= ARTISTA_FORTE,
            "un nome invertito deve restare lo stesso interprete: {invertito}"
        );
        assert!((similarity("Poetica", "Poetica") - 1.0).abs() < f64::EPSILON);
        assert!(similarity("Poetica", "Nessuno vuole essere Robin") < 0.3);
    }

    #[test]
    fn due_testi_vuoti_non_si_somigliano() {
        // Uguali sì, ma «non lo so» contro «non lo so» non è una prova, ed è
        // quel che diventerebbe un 1.0 restituito qui.
        assert!(dice("", "") < f64::EPSILON);
    }

    #[test]
    fn i_bigrammi_sono_di_caratteri_non_di_byte() {
        // Su una scrittura non latina i byte darebbero bigrammi che tagliano i
        // caratteri a metà, e la somiglianza diventerebbe rumore.
        assert!((similarity("坂本龍一", "坂本龍一") - 1.0).abs() < f64::EPSILON);
        assert!(similarity("坂本龍一", "久石譲") < 0.3);
    }

    // ── abbinamento d'album ──

    /// Un disco di tre tracce, in libreria e su MusicBrainz.
    fn disco() -> (Vec<LocalTrack>, RemoteRelease) {
        let locali = vec![
            LocalTrack {
                track_number: Some(1),
                disc_number: Some(1),
                year: Some(2001),
                ..brano("Uno", "Artista", 180_000)
            },
            LocalTrack {
                track_number: Some(2),
                disc_number: Some(1),
                year: Some(2001),
                ..brano("Due", "Artista", 200_000)
            },
            LocalTrack {
                track_number: Some(3),
                disc_number: Some(1),
                year: Some(2001),
                ..brano("Tre", "Artista", 220_000)
            },
        ];
        let remota = RemoteRelease {
            title: "Il Disco".to_owned(),
            artist: "Artista".to_owned(),
            year: Some(2001),
            tracks: vec![
                traccia("Uno", 1, 180_000),
                traccia("Due", 2, 200_000),
                traccia("Tre", 3, 220_000),
            ],
            mb_release_id: Some("R1".to_owned()),
            mb_release_group_id: Some("RG1".to_owned()),
        };
        (locali, remota)
    }

    #[test]
    fn un_disco_che_coincide_si_applica() {
        let (locali, remota) = disco();
        let abbinamento = album_distance(
            &LocalAlbum {
                title: "Il Disco",
                artist: "Artista",
                tracks: &locali,
            },
            &remota,
        );
        assert_eq!(abbinamento.assignment, vec![Some(0), Some(1), Some(2)]);
        assert!(
            abbinamento.distance < DISTANZA_APPLICA,
            "distanza {}",
            abbinamento.distance
        );
        assert_eq!(decide_album(&abbinamento), Verdetto::Applica);
    }

    #[test]
    fn un_disco_a_cui_manca_una_traccia_non_si_applica() {
        // Un'edizione diversa. Applicarla scriverebbe i numeri di traccia
        // sbagliati su tutto il resto del disco.
        let (locali, mut remota) = disco();
        remota.tracks.push(traccia("Quattro", 4, 240_000));
        let abbinamento = album_distance(
            &LocalAlbum {
                title: "Il Disco",
                artist: "Artista",
                tracks: &locali,
            },
            &remota,
        );
        assert_ne!(decide_album(&abbinamento), Verdetto::Applica);
    }

    #[test]
    fn una_traccia_allungata_ferma_tutto_il_disco() {
        let (locali, mut remota) = disco();
        if let Some(seconda) = remota.tracks.get_mut(1) {
            seconda.duration_ms = Some(200_000 + 40_000);
        }
        let abbinamento = album_distance(
            &LocalAlbum {
                title: "Il Disco",
                artist: "Artista",
                tracks: &locali,
            },
            &remota,
        );
        assert!(
            abbinamento
                .evidence
                .worst_duration_delta_sec
                .is_some_and(|s| s > VETO_DURATA_SEC)
        );
        assert_ne!(decide_album(&abbinamento), Verdetto::Applica);
    }

    #[test]
    fn un_disco_dal_vivo_non_e_quello_in_studio() {
        let (locali, mut remota) = disco();
        remota.title = "Il Disco (Live at Wembley)".to_owned();
        let abbinamento = album_distance(
            &LocalAlbum {
                title: "Il Disco",
                artist: "Artista",
                tracks: &locali,
            },
            &remota,
        );
        assert!(abbinamento.evidence.variant_mismatch);
        assert_ne!(decide_album(&abbinamento), Verdetto::Applica);
    }

    #[test]
    fn i_numeri_battono_la_somiglianza_su_due_titoli_quasi_uguali() {
        // «Intro» e «Outro» si somigliano parecchio e durano quasi uguale:
        // l'assegnamento avido può scambiarle, quello per numero no.
        let locali = vec![
            LocalTrack {
                track_number: Some(1),
                disc_number: Some(1),
                ..brano("Intro", "Artista", 30_000)
            },
            LocalTrack {
                track_number: Some(2),
                disc_number: Some(1),
                ..brano("Outro", "Artista", 31_000)
            },
        ];
        let remota = RemoteRelease {
            title: "Il Disco".to_owned(),
            artist: "Artista".to_owned(),
            tracks: vec![traccia("Intro", 1, 30_000), traccia("Outro", 2, 31_000)],
            ..RemoteRelease::default()
        };
        let abbinamento = album_distance(
            &LocalAlbum {
                title: "Il Disco",
                artist: "Artista",
                tracks: &locali,
            },
            &remota,
        );
        assert_eq!(abbinamento.assignment, vec![Some(0), Some(1)]);
    }

    #[test]
    fn senza_numeri_si_abbina_per_somiglianza() {
        let locali = vec![
            brano("Tre", "Artista", 220_000),
            brano("Uno", "Artista", 180_000),
        ];
        let remota = RemoteRelease {
            title: "Il Disco".to_owned(),
            artist: "Artista".to_owned(),
            tracks: vec![traccia("Uno", 1, 180_000), traccia("Tre", 3, 220_000)],
            ..RemoteRelease::default()
        };
        let abbinamento = album_distance(
            &LocalAlbum {
                title: "Il Disco",
                artist: "Artista",
                tracks: &locali,
            },
            &remota,
        );
        assert_eq!(abbinamento.assignment, vec![Some(1), Some(0)]);
    }

    #[test]
    fn l_abbinamento_non_dipende_dall_ordine_di_arrivo() {
        // Un piano che cambia fra due esecuzioni non si può confrontare.
        let (locali, remota) = disco();
        let gruppo = LocalAlbum {
            title: "Il Disco",
            artist: "Artista",
            tracks: &locali,
        };
        let prima = album_distance(&gruppo, &remota);
        let dopo = album_distance(&gruppo, &remota);
        assert_eq!(prima, dopo);
    }

    #[test]
    fn un_asse_ignoto_non_diluisce_una_penalita() {
        // Il modo preciso in cui questo si romperebbe: se un asse che non si
        // può misurare entrasse nella somma come zero **restando** nel
        // divisore, saprebbe meno e sembrerebbe più vicino. Qui il titolo
        // dell'album non coincide, e la penalità che ne viene deve pesare di
        // più — non di meno — quando l'anno non si sa.
        let (locali, remota) = disco();
        let gruppo = LocalAlbum {
            title: "Un Altro Disco",
            artist: "Artista",
            tracks: &locali,
        };
        let con_anno = album_distance(&gruppo, &remota);
        let senza_anno = album_distance(
            &gruppo,
            &RemoteRelease {
                year: None,
                ..remota
            },
        );
        assert!(
            con_anno.distance > 0.0,
            "il titolo diverso deve penalizzare"
        );
        assert!(
            senza_anno.distance > con_anno.distance,
            "senza anno {} contro con anno {}",
            senza_anno.distance,
            con_anno.distance
        );
    }

    #[test]
    fn un_gruppo_vuoto_non_corrisponde_a_niente() {
        let (_, remota) = disco();
        let abbinamento = album_distance(
            &LocalAlbum {
                title: "Il Disco",
                artist: "Artista",
                tracks: &[],
            },
            &remota,
        );
        assert!(abbinamento.assignment.is_empty());
        assert_eq!(decide_album(&abbinamento), Verdetto::Nessuno);
    }

    // ── decisione per brano ──

    #[test]
    fn senza_consenso_non_si_applica() {
        // È la regola più severa del vecchio albero, e la ragione sta nel
        // commento di `decide_track`: due assi forti sono quel che una base
        // karaoke soddisfa.
        let locale = brano("Poetica", "Cesare Cremonini", 297_000);
        let uno = candidato(
            FonteMeta::MusicBrainz,
            "Poetica",
            "Cesare Cremonini",
            297_000,
        );
        let solo = resolve_track(&locale, std::slice::from_ref(&uno)).expect("un candidato");
        assert_eq!(solo.verdict, Verdetto::DaRivedere);

        let due = candidato(FonteMeta::Deezer, "Poetica", "Cesare Cremonini", 297_500);
        let insieme = resolve_track(&locale, &[uno, due]).expect("due candidati");
        assert!(insieme.evidence.consensus);
        assert_eq!(insieme.verdict, Verdetto::Applica);
    }

    #[test]
    fn due_candidati_della_stessa_fonte_non_fanno_consenso() {
        // Due risposte dello stesso servizio sono un'opinione sola: il consenso
        // vale perché due basi dati indipendenti dicono la stessa cosa.
        let locale = brano("Poetica", "Cesare Cremonini", 297_000);
        let candidati = [
            candidato(
                FonteMeta::MusicBrainz,
                "Poetica",
                "Cesare Cremonini",
                297_000,
            ),
            candidato(
                FonteMeta::MusicBrainz,
                "Poetica",
                "Cesare Cremonini",
                297_200,
            ),
        ];
        let esito = resolve_track(&locale, &candidati).expect("candidati");
        assert!(!esito.evidence.consensus);
        assert_ne!(esito.verdict, Verdetto::Applica);
    }

    #[test]
    fn un_interprete_smentito_veta_l_applicazione() {
        let locale = brano("Poetica", "Cesare Cremonini", 297_000);
        let candidati = [
            candidato(
                FonteMeta::MusicBrainz,
                "Poetica",
                "Coro dei Bambini",
                297_000,
            ),
            candidato(FonteMeta::Deezer, "Poetica", "Coro dei Bambini", 297_100),
        ];
        let esito = resolve_track(&locale, &candidati).expect("candidati");
        assert!(esito.evidence.artist_sim.is_some_and(|s| s < SMENTITA));
        assert_ne!(esito.verdict, Verdetto::Applica);
    }

    #[test]
    fn senza_ancora_al_file_il_consenso_non_basta() {
        // Due fonti concordi su «Yesterday» non dicono che il **nostro** file
        // sia quel «Yesterday»: senza interprete noto e senza durata, si aspetta.
        let locale = LocalTrack {
            title: "Yesterday".to_owned(),
            artist: UNKNOWN_ARTIST.to_owned(),
            duration_ms: None,
            ..LocalTrack::default()
        };
        let candidati = [
            Candidate {
                duration_ms: None,
                ..candidato(FonteMeta::MusicBrainz, "Yesterday", "The Beatles", 0)
            },
            Candidate {
                album: Some("Help!".to_owned()),
                ..candidato(FonteMeta::Deezer, "Yesterday", "The Beatles", 0)
            },
        ];
        let esito = resolve_track(&locale, &candidati).expect("candidati");
        assert_ne!(esito.verdict, Verdetto::Applica);
    }

    #[test]
    fn nessun_candidato_non_e_un_verdetto() {
        let locale = brano("Poetica", "Cesare Cremonini", 297_000);
        assert!(resolve_track(&locale, &[]).is_none());
    }

    // ── piano di scrittura ──

    #[test]
    fn un_campo_di_ripiego_si_riempie_anche_senza_certezza() {
        let locale = LocalTrack {
            title: "01 - traccia".to_owned(),
            artist: UNKNOWN_ARTIST.to_owned(),
            album: Some(UNKNOWN_ALBUM.to_owned()),
            file_stem: Some("01 - traccia".to_owned()),
            ..LocalTrack::default()
        };
        let trovati = Fields {
            title: Some("Poetica".to_owned()),
            artist: Some("Cesare Cremonini".to_owned()),
            album: Some("Possibili scenari".to_owned()),
            ..Fields::default()
        };
        let piano = plan_write(&locale, &trovati, false);
        assert_eq!(piano.title.as_deref(), Some("Poetica"));
        assert_eq!(piano.artist.as_deref(), Some("Cesare Cremonini"));
        assert_eq!(piano.album.as_deref(), Some("Possibili scenari"));
    }

    #[test]
    fn un_campo_gia_scritto_non_si_tocca_senza_certezza() {
        let locale = brano("Poetica", "Cesare Cremonini", 297_000);
        let trovati = Fields {
            title: Some("Poetica - Remastered".to_owned()),
            ..Fields::default()
        };
        assert_eq!(plan_write(&locale, &trovati, false).title, None);
        assert_eq!(
            plan_write(&locale, &trovati, true).title.as_deref(),
            Some("Poetica - Remastered")
        );
    }

    #[test]
    fn un_genere_scritto_a_mano_non_si_perde() {
        // Il campo su cui le fonti sono più in disaccordo, e l'unico su cui la
        // scelta dell'utente è più informata della loro.
        let locale = LocalTrack {
            genre: Some("Cantautorato".to_owned()),
            ..brano("Poetica", "Cesare Cremonini", 297_000)
        };
        let trovati = Fields {
            genre: Some("Pop".to_owned()),
            ..Fields::default()
        };
        assert_eq!(plan_write(&locale, &trovati, true).genre, None);
    }

    #[test]
    fn un_campo_che_la_fonte_non_conosce_non_si_svuota() {
        let locale = LocalTrack {
            genre: Some("Cantautorato".to_owned()),
            year: Some(2017),
            ..brano("Poetica", "Cesare Cremonini", 297_000)
        };
        let piano = plan_write(&locale, &Fields::default(), true);
        assert!(piano.e_vuoto(), "una fonte muta non cancella niente");
    }

    #[test]
    fn un_identificativo_gia_presente_non_si_sostituisce() {
        // Ce l'ha messo chi ha taggato il file con Picard, e ne sa più di noi.
        let locale = LocalTrack {
            has_mb_id: true,
            ..brano("Poetica", "Cesare Cremonini", 297_000)
        };
        let trovati = Fields {
            mb_recording_id: Some("altro".to_owned()),
            ..Fields::default()
        };
        assert_eq!(plan_write(&locale, &trovati, true).mb_recording_id, None);
    }

    #[test]
    fn riscrivere_lo_stesso_valore_non_e_una_scrittura() {
        // Aprirebbe il file, ne cambierebbe la data di modifica e scriverebbe
        // una riga di annullamento per non cambiare niente.
        let locale = brano("Poetica", "Cesare Cremonini", 297_000);
        let trovati = Fields {
            title: Some("Poetica".to_owned()),
            artist: Some("Cesare Cremonini".to_owned()),
            ..Fields::default()
        };
        assert!(plan_write(&locale, &trovati, true).e_vuoto());
    }

    #[test]
    fn un_titolo_uguale_al_nome_del_file_e_un_ripiego() {
        assert!(titolo_di_ripiego("01 - traccia", Some("01 - traccia")));
        assert!(titolo_di_ripiego("Poetica (Official Video)", None));
        assert!(titolo_di_ripiego("   ", None));
        assert!(!titolo_di_ripiego("Poetica", Some("01 - traccia")));
        assert!(!titolo_di_ripiego("Poetica (Live)", None));
    }
}
