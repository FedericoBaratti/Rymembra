//! `skin.json`: la forma del documento, e la porta d'ingresso che lo valida.
//!
//! Ogni blocco è **chiuso**: una chiave che non conosciamo è un errore, mai
//! silenzio. È la scelta opposta a quella comoda, e il motivo è che nel vecchio
//! albero una skin era CSS — un token scritto male non dava errore, dava una
//! skin visivamente rotta in un punto solo, e trovarlo richiedeva di
//! accorgersene guardando.
//!
//! # Perché il messaggio conta più del solito
//!
//! Chi crea una skin deve sapere **cosa** non va — quale token, quale valore —
//! non solo che il pacchetto è stato rifiutato. Ed è per questo che qui non c'è
//! una `derive(Deserialize)`: serde riporterebbe «invalid type: string,
//! expected struct Length» con un percorso, mentre il messaggio utile è «`14pt`
//! non è una lunghezza: le unità ammesse sono px, rem, em, %, …». La
//! deserializzazione a mano costa righe e le restituisce tutte in messaggi.
//!
//! # Fail-fast dove è una svista, elenco dove è un lavoro
//!
//! Un `format` sbagliato è una cosa sola e la si corregge subito. I token e le
//! parti no: sono decine di voci scritte in una sessione, e chi le sta scrivendo
//! vuole vedere **tutti** gli errori insieme invece di scoprirli uno per
//! esecuzione. Quindi i blocchi grandi raccolgono, il resto si ferma al primo
//! problema.
//!
//! # Il formato 2
//!
//! Il formato 1 si legge ancora: [`MIGRAZIONI`] lo porta al 2 riscrivendo il
//! JSON prima della validazione (vedi [`crate::migrazione`]). Quel che il 2
//! aggiunge sta nei moduli che lo possiedono — gli stati in [`crate::stati`], il
//! movimento in [`crate::movimento`], le parti in [`crate::parts`] — e qui c'è
//! la lettura. Le differenze che contano per chi legge questo file:
//!
//! - **i temi sono simmetrici**: `scheme` dice se la skin è scura o chiara di
//!   base, e `variant` porta il tema opposto con i suoi token, la sua
//!   tavolozza, i suoi motivi e le sue parti. Il formato 1 aveva solo
//!   `themes.light`, e solo per i token;
//! - **una skin porta i suoi caratteri e le sue immagini**: `fonts` e
//!   l'effetto `image` nominano voci del pacchetto, mai indirizzi;
//! - **il movimento è per parte e per innesco**: `parts.<p>.motion`.

use rymembra_domain::errors::{AppError, ErrorCode};
use serde_json::{Map, Value};

use crate::effects::Effect;
use crate::layout::{Align, GapStep, LayoutZone, Spread, ZoneKind, shell_di_serie};
use crate::movimento::{AnimDirection, Animation, Curva, Riempimento};
use crate::parts::{BorderStyle, PartStyle, TextTransform};
use crate::tokens::TokenSet;
use crate::values::{Duration, Easing, EasingKeyword, Rgba};
use crate::vicini::{forse, vicini};

mod avvisi;
mod blocchi;
mod lampeggio;
mod parti;
mod scafale;
mod valori;

pub use avvisi::{
    AnelloMisurato, CONTRASTO_ANELLO, CONTRASTO_MINIMO, Ciclo, ContrastPair, Esame, Misura,
    OLTRE_IL_BUDGET, SkinWarning, WarningKind, check_skin, contrast_pairs, controlla_installazione,
    esamina, focus_ring_pairs, fuori_misura, palette_usage,
};
pub use lampeggio::{MEZZO_CICLO_MINIMO_MS, RIPETIZIONI_SENZA_LAMPEGGIO};

pub(crate) use avvisi::{DIETRO, colore_di};
pub(crate) use blocchi::famiglia_ridotta;

use blocchi::{
    capacita, caratteri, meta, motivi, movimento, tavolozza, tavolozza_mancante,
    tinte_che_cambiano, variante,
};
use lampeggio::lampeggi;
use parti::{Richiamabili, superfici};
use scafale::impaginazione;
use valori::insieme_token;

/// La versione del formato. Un numero, non semver: cambia solo se rompiamo.
///
/// È 2 dalla 3.0.0. Un documento del formato 1 si legge ancora — lo migra
/// [`crate::migrazione`] — e uno del formato 2 non si legge in una versione
/// che conosce solo l'1: i suoi blocchi sono chiusi, e una chiave nuova è un
/// errore. È esattamente quel che il cambio di numero dice.
pub const SKIN_FORMAT_VERSION: u32 = 2;

/// Un problema in un punto preciso del documento.
#[derive(Debug, Clone, PartialEq)]
pub struct SkinIssue {
    /// Dove: `tokens.color.accent`.
    pub path: String,
    /// Cosa non va, e cosa scrivere al suo posto.
    pub message: String,
    /// Il codice del catalogo che descrive questo problema.
    pub code: ErrorCode,
    /// Riga e colonna, quando si sanno **senza** il percorso.
    ///
    /// `None` per tutti i problemi di schema, e non è una mancanza: il loro
    /// posto lo dice `path`, e chi ha il testo sottomano lo traduce in una riga
    /// con [`crate::posizioni`]. L'unico che lo porta scritto è l'errore di
    /// sintassi — un documento che non è JSON non ha percorsi, ha solo un
    /// carattere in cui si è rotto, e quello lo sa soltanto `serde_json`.
    pub punto: Option<crate::posizioni::Punto>,
}

/// Il tipo di risultato dei parser interni.
pub(crate) type Esito<T> = Result<T, SkinIssue>;

pub(crate) fn problema(path: &str, message: impl Into<String>) -> SkinIssue {
    let message = message.into();
    SkinIssue {
        code: ErrorCode::SkinManifestInvalid {
            detail: Some(format!("{path}: {message}")),
        },
        path: path.to_owned(),
        message,
        punto: None,
    }
}

/// Un valore che non va per un token. Ha un codice suo perché è il caso in cui
/// il posto esatto conta di più: un token è una riga in un file di duecento.
fn token_non_valido(
    path: &str,
    token: &str,
    grezzo: &Value,
    message: impl Into<String>,
) -> SkinIssue {
    let message = message.into();
    SkinIssue {
        code: ErrorCode::SkinTokenInvalid {
            token: token.to_owned(),
            value: compatto(grezzo),
        },
        path: path.to_owned(),
        message,
        punto: None,
    }
}

/// Il valore com'era scritto, accorciato: serve nel messaggio, non nel foglio.
fn compatto(value: &Value) -> String {
    let testo = value.to_string();
    match testo.char_indices().nth(80) {
        Some((taglio, _)) => match testo.get(..taglio) {
            Some(inizio) => format!("{inizio}…"),
            None => testo,
        },
        None => testo,
    }
}

pub(crate) fn giu(path: &str, name: &str) -> String {
    if path.is_empty() {
        return name.to_owned();
    }
    format!("{path}.{name}")
}

// ── Le forme elementari ─────────────────────────────────────────────────────

pub(crate) fn oggetto<'a>(value: &'a Value, path: &str) -> Esito<&'a Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| problema(path, "qui ci va un oggetto"))
}

fn testo<'a>(value: &'a Value, path: &str) -> Esito<&'a str> {
    value
        .as_str()
        .ok_or_else(|| problema(path, "qui ci va una stringa"))
}

fn numero(value: &Value, path: &str) -> Esito<f64> {
    value
        .as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| problema(path, "qui ci va un numero"))
}

fn numero_fra(value: &Value, path: &str, min: f64, max: f64) -> Esito<f64> {
    let n = numero(value, path)?;
    if n < min || n > max {
        return Err(problema(path, format!("va fra {min} e {max}, non {n}")));
    }
    Ok(n)
}

fn booleano(value: &Value, path: &str) -> Esito<bool> {
    value
        .as_bool()
        .ok_or_else(|| problema(path, "qui ci va vero o falso"))
}

fn lista<'a>(value: &'a Value, path: &str) -> Esito<&'a Vec<Value>> {
    value
        .as_array()
        .ok_or_else(|| problema(path, "qui ci va una lista"))
}

fn lista_fra<'a>(value: &'a Value, path: &str, min: usize, max: usize) -> Esito<&'a Vec<Value>> {
    let voci = lista(value, path)?;
    if voci.len() < min || voci.len() > max {
        return Err(problema(
            path,
            format!("la lista va da {min} a {max} voci, non {}", voci.len()),
        ));
    }
    Ok(voci)
}

/// Le sole chiavi ammesse. È lo `strict` del vecchio schema.
fn solo_chiavi(map: &Map<String, Value>, path: &str, ammesse: &[&str]) -> Esito<()> {
    for chiave in map.keys() {
        if ammesse.contains(&chiave.as_str()) {
            continue;
        }
        let candidati = vicini(chiave, ammesse.iter().copied());
        return Err(problema(
            &giu(path, chiave),
            format!("chiave sconosciuta.{}", forse(&candidati)),
        ));
    }
    Ok(())
}

fn campo<'a>(map: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    map.get(name).filter(|value| !value.is_null())
}

fn richiesto<'a>(map: &'a Map<String, Value>, path: &str, name: &str) -> Esito<&'a Value> {
    campo(map, name).ok_or_else(|| problema(&giu(path, name), "campo obbligatorio, manca"))
}

fn parola<T: Copy>(value: &Value, path: &str, ammesse: &[(&str, T)]) -> Esito<T> {
    let scritto = testo(value, path)?;
    ammesse
        .iter()
        .find(|(nome, _)| *nome == scritto)
        .map(|(_, valore)| *valore)
        .ok_or_else(|| {
            let nomi: Vec<&str> = ammesse.iter().map(|(nome, _)| *nome).collect();
            problema(path, format!("ammessi soltanto: {}", nomi.join(", ")))
        })
}

// ── Le chiavi, in ordine ────────────────────────────────────────────────────
//
// Gli elenchi delle chiavi ammesse, con un nome. Servono due volte: alla
// validazione, che rifiuta quel che non c'è, e alla forma canonica di
// `crate::migrazione`, che le riscrive **in quest'ordine**. Un elenco solo per
// due lettori è la ragione per cui non sono più scritti in linea.

/// Le chiavi della radice, nell'ordine della forma canonica.
pub(crate) const CHIAVI_DOCUMENTO: &[&str] = &[
    "format",
    "id",
    "meta",
    "scheme",
    "capabilities",
    "fonts",
    "palette",
    "patterns",
    "tokens",
    "variant",
    "platforms",
    "motion",
    "layout",
    "parts",
];

/// Le chiavi di `meta`.
pub(crate) const CHIAVI_META: &[&str] = &[
    "name",
    "author",
    "version",
    "description",
    "license",
    "basedOn",
    "preview",
];

/// Le chiavi di `capabilities`.
pub(crate) const CHIAVI_CAPACITA: &[&str] = &["dynamicAccent"];

/// Le chiavi di un carattere in `fonts`.
pub(crate) const CHIAVI_FONT: &[&str] = &["src", "weight", "style"];

/// Le chiavi di `variant`.
pub(crate) const CHIAVI_VARIANTE: &[&str] = &["tokens", "palette", "patterns", "parts"];

/// Le chiavi di `motion`.
pub(crate) const CHIAVI_MOVIMENTO: &[&str] = &["intensity", "easings", "route", "animations"];

/// Le chiavi di `motion.route`.
pub(crate) const CHIAVI_ROTTA: &[&str] = &["duration", "easing", "out", "in"];

/// Le chiavi di un fotogramma di rotta.
pub(crate) const CHIAVI_FOTOGRAMMA_ROTTA: &[&str] = &["opacity", "scale", "translateY"];

/// Le chiavi di un'animazione.
pub(crate) const CHIAVI_ANIMAZIONE: &[&str] = &[
    "duration",
    "delay",
    "easing",
    "iterations",
    "direction",
    "fill",
    "origin",
    "frames",
];

/// Le chiavi di un fotogramma di animazione.
pub(crate) const CHIAVI_FOTOGRAMMA: &[&str] = &[
    "at",
    "opacity",
    "translateX",
    "translateY",
    "scale",
    "scaleX",
    "scaleY",
    "rotate",
    "skewX",
    "skewY",
    "blur",
    "brightness",
    "saturate",
    "hueRotate",
    "backgroundColor",
    "color",
    "borderColor",
    "shadow",
    "clip",
];

/// Le chiavi di `layout`.
pub(crate) const CHIAVI_IMPAGINAZIONE: &[&str] = &["density", "prefabs", "shell"];

/// Le chiavi di una zona dello scafale.
pub(crate) const CHIAVI_ZONA: &[&str] =
    &["zone", "size", "gap", "align", "spread", "part", "children"];

/// Le chiavi di un widget dello scafale.
pub(crate) const CHIAVI_WIDGET: &[&str] = &["widget", "size", "options"];

/// Le chiavi di un prefab richiamato.
pub(crate) const CHIAVI_RICHIAMO: &[&str] = &["prefab", "size"];

/// Le chiavi dell'aspetto di una parte, base o stato.
pub(crate) const CHIAVI_ASPETTO: &[&str] = &[
    "background",
    "textColor",
    "borderColor",
    "borderWidth",
    "borderStyle",
    "radius",
    "shadow",
    "focusRing",
    "clip",
    "filter",
    "backdrop",
    "opacity",
    "font",
    "fontSize",
    "lineHeight",
    "letterSpacing",
    "textTransform",
    "fontWeight",
];

/// Le chiavi strutturali di una parte, dopo quelle d'aspetto.
pub(crate) const CHIAVI_STRUTTURA_PARTE: &[&str] = &["before", "after", "states", "motion"];

/// Le chiavi di un livello.
pub(crate) const CHIAVI_LIVELLO: &[&str] = &["background", "opacity"];

/// Le chiavi del movimento di una parte oltre agli inneschi.
pub(crate) const CHIAVI_MOVIMENTO_PARTE: &[&str] = &["stagger", "transition"];

/// Le chiavi di una transizione di parte.
pub(crate) const CHIAVI_TRANSIZIONE: &[&str] = &["duration", "easing"];

/// Le chiavi di un anello del fuoco.
pub(crate) const CHIAVI_ANELLO: &[&str] = &["color", "width", "offset"];

/// Le chiavi di un raggio per angolo.
pub(crate) const CHIAVI_ANGOLI: &[&str] = &["topLeft", "topRight", "bottomRight", "bottomLeft"];

/// Le chiavi di un livello d'ombra.
pub(crate) const CHIAVI_LIVELLO_OMBRA: &[&str] = &["inset", "x", "y", "blur", "spread", "color"];

/// Le chiavi di una fermata di gradiente.
pub(crate) const CHIAVI_FERMATA: &[&str] = &["color", "at"];

/// Le chiavi che il formato accetta, per contesto.
///
/// È quel che l'editor di codice dello Studio propone mentre si scrive: le
/// stesse liste con cui [`solo_chiavi`] rifiuta una chiave, così il
/// completamento non può suggerire una chiave che il validatore poi respinge,
/// né dimenticarne una che accetta.
#[must_use]
pub fn chiavi_schema() -> Vec<(&'static str, &'static [&'static str])> {
    vec![
        ("documento", CHIAVI_DOCUMENTO),
        ("meta", CHIAVI_META),
        ("capacita", CHIAVI_CAPACITA),
        ("font", CHIAVI_FONT),
        ("variante", CHIAVI_VARIANTE),
        ("movimento", CHIAVI_MOVIMENTO),
        ("rotta", CHIAVI_ROTTA),
        ("fotogrammaRotta", CHIAVI_FOTOGRAMMA_ROTTA),
        ("animazione", CHIAVI_ANIMAZIONE),
        ("fotogramma", CHIAVI_FOTOGRAMMA),
        ("impaginazione", CHIAVI_IMPAGINAZIONE),
        ("zona", CHIAVI_ZONA),
        ("widget", CHIAVI_WIDGET),
        ("richiamo", CHIAVI_RICHIAMO),
        ("aspetto", CHIAVI_ASPETTO),
        ("strutturaParte", CHIAVI_STRUTTURA_PARTE),
        ("livello", CHIAVI_LIVELLO),
        ("movimentoParte", CHIAVI_MOVIMENTO_PARTE),
        ("transizione", CHIAVI_TRANSIZIONE),
        ("anello", CHIAVI_ANELLO),
        ("angoli", CHIAVI_ANGOLI),
        ("livelloOmbra", CHIAVI_LIVELLO_OMBRA),
        ("fermata", CHIAVI_FERMATA),
    ]
}

/// Le parole chiuse che un campo accetta, per nome del campo.
///
/// Il gemello di [`chiavi_schema`] per i valori: `"scheme": "…"`,
/// `"borderStyle": "…"`, `"easing": "…"`. Vengono dagli stessi `ALL` con cui
/// la lettura riconosce la parola.
#[must_use]
pub fn parole_schema() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "scheme",
            vec![Scheme::Dark.as_str(), Scheme::Light.as_str()],
        ),
        (
            "density",
            [Density::Compact, Density::Comfortable, Density::Spacious]
                .iter()
                .map(|d| d.as_str())
                .collect(),
        ),
        (
            "intensity",
            MotionIntensity::ALL.iter().map(|m| m.as_str()).collect(),
        ),
        ("font", vec!["sans", "mono", "display"]),
        (
            "borderStyle",
            BorderStyle::ALL.iter().map(|b| b.as_str()).collect(),
        ),
        (
            "textTransform",
            TextTransform::ALL.iter().map(|t| t.as_str()).collect(),
        ),
        (
            "easing",
            EasingKeyword::ALL.iter().map(|k| k.as_str()).collect(),
        ),
        (
            "direction",
            AnimDirection::ALL.iter().map(|d| d.as_str()).collect(),
        ),
        (
            "fill",
            Riempimento::ALL.iter().map(|r| r.as_str()).collect(),
        ),
        ("iterations", vec!["infinite"]),
        (
            "origin",
            vec![
                "center",
                "top",
                "bottom",
                "left",
                "right",
                "top left",
                "top right",
                "bottom left",
                "bottom right",
            ],
        ),
        ("zone", ZoneKind::ALL.iter().map(|z| z.as_str()).collect()),
        ("gap", GapStep::ALL.iter().map(|g| g.as_str()).collect()),
        ("align", Align::ALL.iter().map(|a| a.as_str()).collect()),
        ("spread", Spread::ALL.iter().map(|s| s.as_str()).collect()),
        ("size", vec!["hug", "fill"]),
        ("position", vec!["start", "end"]),
        ("style", vec!["normal", "italic"]),
    ]
}

/// Le chiavi di ogni effetto, in ordine, dopo `effect`.
///
/// Stanno qui e non accanto a [`Effect`] perché sono la forma **scritta**
/// dell'effetto, che è cosa di questo modulo.
pub(crate) fn chiavi_effetto(nome: &str) -> &'static [&'static str] {
    match nome {
        "solid" => &["effect", "color"],
        "linearGradient" => &["effect", "angle", "stops"],
        "radialGradient" => &["effect", "shape", "at", "size", "stops"],
        "conicGradient" => &["effect", "from", "at", "stops"],
        "hairlineGrid" => &["effect", "color", "cell", "cellY", "thickness"],
        "scanlines" => &["effect", "color", "line", "gap"],
        "stripes" => &["effect", "angle", "color", "background", "width"],
        "dotGrid" => &["effect", "color", "spacing", "dot"],
        "vignette" => &["effect", "color", "start"],
        "chamfer" => &["effect", "size", "corners"],
        "blurBehind" => &["effect", "radius", "saturate"],
        "image" => &["effect", "src", "size", "repeat", "position"],
        _ => &["effect"],
    }
}

// ── I blocchi del documento ─────────────────────────────────────────────────

/// Chi ha fatto la skin, e come si chiama.
///
/// Non è `Eq`: `preview` sono colori, e l'opacità di un colore è un `f64`.
#[derive(Debug, Clone, PartialEq)]
pub struct SkinMeta {
    /// Il nome mostrato nel selettore.
    pub name: String,
    /// L'autore.
    pub author: String,
    /// La versione della skin, `1.0.0`.
    pub version: String,
    /// A cosa somiglia.
    pub description: Option<String>,
    /// La licenza.
    pub license: Option<String>,
    /// Da quale skin è stata derivata, quando è un fork.
    pub based_on: Option<String>,
    /// I tre colori della scheda nel selettore, scelti dall'autore.
    ///
    /// Sono **letterali** e non riferimenti a token: servono a mostrare la
    /// skin a chi non l'ha ancora scelta, cioè quando i suoi token non sono
    /// applicati a niente.
    pub preview: Option<[Rgba; 3]>,
}

/// Il tema di base di una skin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scheme {
    /// Scura di base: la variante, se c'è, è chiara.
    #[default]
    Dark,
    /// Chiara di base: la variante, se c'è, è scura.
    Light,
}

impl Scheme {
    /// Come si scrive nel documento e in `color-scheme`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    /// Il tema opposto: quello della variante.
    #[must_use]
    pub const fn opposto(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }

    /// Come si chiama in un messaggio: «scuro», «chiaro».
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Dark => "scuro",
            Self::Light => "chiaro",
        }
    }
}

/// Cosa la skin dichiara di saper fare.
///
/// Nel formato 1 c'erano anche `light` e `mobile`: il primo è diventato la
/// presenza di [`SkinDocument::variant`], che non può più mentire — una
/// variante c'è o non c'è — e il secondo non lo leggeva nessuno.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkinCapabilities {
    /// Va bene che i colori seguano la copertina. Una skin con una tavolozza
    /// fissa e voluta dice falso.
    pub dynamic_accent: bool,
}

impl Default for SkinCapabilities {
    fn default() -> Self {
        Self {
            dynamic_accent: true,
        }
    }
}

/// Un carattere portato dal pacchetto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFace {
    /// Il nome della famiglia, come lo scrivono i token `font.*`.
    pub family: String,
    /// La voce `woff2` del pacchetto.
    pub src: String,
    /// Il peso, o l'intervallo dei pesi di un carattere variabile.
    pub weight: (u16, u16),
    /// Corsivo.
    pub italic: bool,
}

/// Il tema opposto a quello di base.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Variante {
    /// I token che cambiano.
    pub tokens: TokenSet,
    /// I colori della tavolozza che cambiano. Solo nomi già dichiarati.
    pub palette: Vec<(String, Rgba)>,
    /// I motivi che cambiano. Solo nomi già dichiarati, con la stessa destinazione.
    pub patterns: Vec<(String, Effect)>,
    /// Le parti ridipinte in questo tema.
    pub parts: Vec<PartStyle>,
}

impl Variante {
    /// Non cambia niente: una promessa vuota.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
            && self.palette.is_empty()
            && self.patterns.is_empty()
            && self.parts.is_empty()
    }
}

/// Quanto si muove.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionIntensity {
    /// Ferma.
    None,
    /// Solo ciò che serve a capire cosa succede.
    Essential,
    /// Normale.
    Full,
    /// Di più.
    Maximum,
}

impl MotionIntensity {
    /// Come si scrive nel documento, ed è anche il valore di `--motion-intensity`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Essential => "essential",
            Self::Full => "full",
            Self::Maximum => "maximum",
        }
    }

    /// Il moltiplicatore che finisce in `--motion-scale`.
    #[must_use]
    pub const fn scale(self) -> f64 {
        match self {
            Self::None => 0.0,
            Self::Essential => 0.5,
            Self::Full => 1.0,
            Self::Maximum => 1.25,
        }
    }

    /// Tutte.
    pub const ALL: &'static [Self] = &[Self::None, Self::Essential, Self::Full, Self::Maximum];
}

/// Un fotogramma di una transizione di rotta.
///
/// Solo opacità e trasformazione: queste regole agiscono su un'istantanea
/// dell'intera pagina, e una rotazione o un colore lì non hanno un significato
/// che si possa prevedere.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RouteFrame {
    /// Opacità.
    pub opacity: Option<f64>,
    /// Scala.
    pub scale: Option<f64>,
    /// Spostamento verticale, in pixel.
    pub translate_y: Option<f64>,
}

impl RouteFrame {
    /// Non dichiara niente: non c'è nessun `@keyframes` da emettere.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Come si sostituiscono due pagine.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Rotta {
    /// Quanto dura. Assente: `--dur-2`, come nel formato 1.
    pub duration: Option<Duration>,
    /// Con che curva. Assente: `--ease-out-expo`.
    pub easing: Option<Curva>,
    /// Come esce quella vecchia.
    pub out: Option<RouteFrame>,
    /// Come entra quella nuova.
    pub enter: Option<RouteFrame>,
}

/// Il movimento della skin.
#[derive(Debug, Clone, PartialEq)]
pub struct SkinMotion {
    /// Quanto si muove. Si **compone** con `prefers-reduced-motion`.
    pub intensity: MotionIntensity,
    /// Le curve della skin, in ordine di nome.
    pub easings: Vec<(String, Easing)>,
    /// Il cambio di pagina.
    pub route: Option<Rotta>,
    /// Le animazioni nominate, in ordine di nome.
    pub animations: Vec<(String, Animation)>,
}

/// Quanto respira il contenuto.
///
/// I tre gradini sono **parole**, non misure, e i numeri che ci stanno dietro
/// sono del motore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Density {
    /// Stretto.
    Compact,
    /// Normale.
    Comfortable,
    /// Largo.
    Spacious,
}

impl Density {
    /// Come si scrive nel documento e in `data-density`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Comfortable => "comfortable",
            Self::Spacious => "spacious",
        }
    }
}

/// Le scelte di impaginazione.
#[derive(Debug, Clone, PartialEq)]
pub struct SkinLayout {
    /// La densità: la scala della spaziatura, in una parola.
    pub density: Density,
    /// L'albero: dove stanno le cose. Sempre popolato.
    pub shell: LayoutZone,
    /// I nomi dei prefab dichiarati, in ordine.
    pub prefabs: Vec<String>,
}

impl Default for SkinLayout {
    fn default() -> Self {
        Self {
            density: Density::Comfortable,
            shell: shell_di_serie(),
            prefabs: Vec::new(),
        }
    }
}

/// Una skin validata.
///
/// Nessun campo è una stringa di CSS, e nessun tipo di questo modulo deriva
/// `Serialize`: la forma interna **non è riserializzabile come sorgente** — un
/// colore validato è una quaterna di canali, e riscriverlo produrrebbe un
/// manifest che questa stessa validazione rifiuta. Chi deve conservare la
/// sorgente conserva il JSON di partenza.
#[derive(Debug, Clone, PartialEq)]
pub struct SkinDocument {
    /// L'identificatore, che finisce nel selettore e nel nome del file.
    pub id: String,
    /// Chi l'ha fatta.
    pub meta: SkinMeta,
    /// Il tema di base.
    pub scheme: Scheme,
    /// Cosa dichiara di saper fare.
    pub capabilities: SkinCapabilities,
    /// I caratteri che porta nel pacchetto.
    pub fonts: Vec<FontFace>,
    /// I colori locali, in ordine di nome.
    pub palette: Vec<(String, Rgba)>,
    /// I token del tema di base.
    pub tokens: TokenSet,
    /// Il tema opposto, se lo dichiara.
    pub variant: Option<Variante>,
    /// Le sovrascritture per `[data-mobile]`.
    pub mobile: Option<TokenSet>,
    /// Il movimento.
    pub motion: Option<SkinMotion>,
    /// L'impaginazione.
    pub layout: Option<SkinLayout>,
    /// I motivi riusabili, in ordine di nome.
    pub patterns: Vec<(String, Effect)>,
    /// Le superfici ridisegnate, in ordine di registro.
    pub parts: Vec<PartStyle>,
}

impl SkinDocument {
    /// I token della variante, se c'è.
    #[must_use]
    pub fn variante_tokens(&self) -> Option<&TokenSet> {
        self.variant.as_ref().map(|v| &v.tokens)
    }

    /// La tavolozza nel tema della variante: quella di base con i colori che
    /// la variante ridefinisce.
    #[must_use]
    pub fn tavolozza_variante(&self) -> Vec<(String, Rgba)> {
        let mut tutta = self.palette.clone();
        if let Some(variante) = self.variant.as_ref() {
            for (nome, colore) in &variante.palette {
                if let Some(voce) = tutta.iter_mut().find(|(n, _)| n == nome) {
                    voce.1 = *colore;
                }
            }
        }
        tutta
    }

    /// Le voci del pacchetto che il documento nomina — caratteri e immagini —
    /// in ordine di nome e senza doppioni.
    ///
    /// Serve al pacchetto, che le deve contenere, e allo Studio, che dice quali
    /// mancano nella bozza.
    #[must_use]
    pub fn assets(&self) -> Vec<String> {
        let mut nomi: Vec<String> = self.fonts.iter().map(|f| f.src.clone()).collect();
        let motivi = self
            .patterns
            .iter()
            .chain(self.variant.iter().flat_map(|v| v.patterns.iter()));
        for (_, effetto) in motivi {
            nomi.extend(effetto.asset().map(str::to_owned));
        }
        let parti = self
            .parts
            .iter()
            .chain(self.variant.iter().flat_map(|v| v.parts.iter()));
        for stile in parti {
            for effetto in stile.effects() {
                nomi.extend(effetto.asset().map(str::to_owned));
            }
        }
        nomi.sort();
        nomi.dedup();
        nomi
    }
}

fn identificatore(value: &Value, path: &str) -> Esito<String> {
    let scritto = testo(value, path)?;
    let mut caratteri = scritto.bytes();
    let ammesso = caratteri.next().is_some_and(|b| b.is_ascii_lowercase())
        && caratteri.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !ammesso || scritto.len() < 2 || scritto.len() > 48 {
        // Vincolato perché finisce in un selettore CSS, in un nome di file e in
        // un URL della rete locale: un id con una virgoletta o una barra
        // romperebbe uno dei tre.
        return Err(problema(
            path,
            "l'id ammette da 2 a 48 fra minuscole, cifre e trattini, e comincia con una lettera",
        ));
    }
    Ok(scritto.to_owned())
}

/// Un problema che ferma la lettura, invece di aggiungersi all'elenco.
fn solo(guasto: SkinIssue) -> Vec<SkinIssue> {
    vec![guasto]
}

fn documento(raw: &Value) -> Result<SkinDocument, Vec<SkinIssue>> {
    let mut problemi: Vec<SkinIssue> = Vec::new();

    let map = oggetto(raw, "").map_err(solo)?;
    for vecchia in ["themes"] {
        if map.contains_key(vecchia) {
            return Err(solo(problema(
                vecchia,
                "nel formato 2 il tema opposto si chiama `variant`, e oltre ai token porta \
                 tavolozza, motivi e parti",
            )));
        }
    }
    solo_chiavi(map, "", CHIAVI_DOCUMENTO).map_err(solo)?;

    let id = identificatore(richiesto(map, "", "id").map_err(solo)?, "id").map_err(solo)?;
    let meta = meta(richiesto(map, "", "meta").map_err(solo)?, "meta").map_err(solo)?;
    let scheme = match campo(map, "scheme") {
        None => Scheme::Dark,
        Some(grezzo) => parola(
            grezzo,
            "scheme",
            &[("dark", Scheme::Dark), ("light", Scheme::Light)],
        )
        .map_err(solo)?,
    };
    let capabilities = match campo(map, "capabilities") {
        None => SkinCapabilities::default(),
        Some(grezzo) => capacita(grezzo, "capabilities").map_err(solo)?,
    };
    let fonts = match campo(map, "fonts") {
        None => Vec::new(),
        Some(grezzo) => caratteri(grezzo, "fonts").map_err(solo)?,
    };
    let palette = match campo(map, "palette") {
        None => Vec::new(),
        Some(grezzo) => tavolozza(grezzo, "palette").map_err(solo)?,
    };

    let tokens = insieme_token(
        richiesto(map, "", "tokens").map_err(solo)?,
        "tokens",
        &mut problemi,
    )
    .map_err(solo)?;

    let mobile = match campo(map, "platforms") {
        None => None,
        Some(grezzo) => {
            let blocco = oggetto(grezzo, "platforms").map_err(solo)?;
            solo_chiavi(blocco, "platforms", &["mobile"]).map_err(solo)?;
            match campo(blocco, "mobile") {
                None => None,
                Some(telefono) => {
                    Some(insieme_token(telefono, "platforms.mobile", &mut problemi).map_err(solo)?)
                }
            }
        }
    };

    let motion = campo(map, "motion")
        .map(|grezzo| movimento(grezzo, "motion"))
        .transpose()
        .map_err(solo)?;
    // Un lampeggio è un errore di tutta la skin, non della parte che lo
    // richiama: si dice sull'animazione, anche se nessuno la usa ancora — nello
    // Studio la timeline la fa scorrere lo stesso. Vedi «Il lampeggio», in
    // `document/lampeggio.rs`.
    if let Some(blocco) = motion.as_ref() {
        problemi.extend(lampeggi(blocco));
    }
    let layout = campo(map, "layout")
        .map(|grezzo| impaginazione(grezzo, "layout", &mut problemi))
        .transpose()
        .map_err(solo)?;
    let patterns = match campo(map, "patterns") {
        None => Vec::new(),
        Some(grezzo) => motivi(grezzo, "patterns").map_err(solo)?,
    };

    // Motivi, animazioni e curve si leggono **prima** delle parti perché le
    // parti li richiamano per nome.
    let animazioni_dichiarate: &[(String, Animation)] =
        motion.as_ref().map_or(&[], |blocco| &blocco.animations);
    let curve_dichiarate: &[(String, Easing)] =
        motion.as_ref().map_or(&[], |blocco| &blocco.easings);
    let richiamabili = Richiamabili {
        motivi: &patterns,
        animazioni: animazioni_dichiarate,
        curve: curve_dichiarate,
    };
    let parts = match campo(map, "parts") {
        None => Vec::new(),
        Some(grezzo) => {
            superfici(grezzo, "parts", &richiamabili, false, &mut problemi).map_err(solo)?
        }
    };
    let variant = campo(map, "variant")
        .map(|grezzo| {
            variante(
                grezzo,
                "variant",
                (&palette, &patterns),
                &richiamabili,
                &mut problemi,
            )
        })
        .transpose()
        .map_err(solo)?;

    tavolozza_mancante(raw, "", &palette, &mut problemi);
    if let Some(variante) = variant.as_ref() {
        problemi.extend(tinte_che_cambiano(&patterns, variante, &parts));
    }

    if !problemi.is_empty() {
        return Err(problemi);
    }

    Ok(SkinDocument {
        id,
        meta,
        scheme,
        capabilities,
        fonts,
        palette,
        tokens,
        variant,
        mobile,
        motion,
        layout,
        patterns,
        parts,
    })
}

fn in_errore(problemi: &[SkinIssue]) -> AppError {
    let elenco: Vec<String> = problemi
        .iter()
        .take(20)
        .map(|guasto| format!("{}: {}", guasto.path, guasto.message))
        .collect();
    // Il codice è quello del primo problema, perché è quello che nomina il punto
    // esatto; il messaggio li elenca tutti.
    let codice = problemi
        .first()
        .map_or(ErrorCode::SkinManifestInvalid { detail: None }, |guasto| {
            guasto.code.clone()
        });
    AppError::new(codice).with_message(elenco.join("; "))
}

pub(crate) fn versione_formato(raw: &Value) -> Option<u64> {
    raw.as_object()?.get("format")?.as_u64()
}

/// Valida un documento skin.
///
/// La versione del formato si controlla **prima** dello schema e a parte: un
/// pacchetto scritto da una versione futura dell'app non è malformato, è solo
/// più nuovo, e i due casi vogliono messaggi diversi.
///
/// # Errori
///
/// `skin.formatUnsupported` se la versione è più nuova della nostra; altrimenti
/// il codice del primo problema trovato, col messaggio che li elenca tutti.
pub fn parse_skin(raw: &Value) -> Result<SkinDocument, AppError> {
    valida(raw).map_err(|problemi| in_errore(&problemi))
}

/// La sequenza vera: versione, migrazione, schema — e l'elenco dei problemi.
fn valida(raw: &Value) -> Result<SkinDocument, Vec<SkinIssue>> {
    let Some(trovata) = versione_formato(raw) else {
        return Err(solo(problema("format", "campo obbligatorio, manca")));
    };
    let nostra = u64::from(SKIN_FORMAT_VERSION);

    // Tre vie. Più nuova: si rifiuta, ed è l'unica risposta onesta. Uguale:
    // passa. **Più vecchia: si migra e passa.**
    if trovata > nostra {
        let found = u32::try_from(trovata).unwrap_or(u32::MAX);
        return Err(solo(SkinIssue {
            code: ErrorCode::SkinFormatUnsupported {
                found,
                supported: SKIN_FORMAT_VERSION,
            },
            path: "format".to_owned(),
            message: format!(
                "è del formato {found}, e questa versione di Rymembra legge fino al {SKIN_FORMAT_VERSION}"
            ),
            punto: None,
        }));
    }
    if trovata < nostra {
        let migrato = migra(raw, trovata)?;
        return documento(&migrato);
    }

    documento(raw)
}

// ── Le migrazioni ───────────────────────────────────────────────────────────

/// Un passo di migrazione: da una versione alla successiva.
///
/// Riscrive **il JSON**, non la forma interna, ed è l'unico posto possibile:
/// [`SkinDocument`] non è riserializzabile. La validazione gira una volta sola,
/// alla fine, sul risultato — così un difetto introdotto da una migrazione
/// viene rifiutato dalle stesse regole di tutto il resto.
pub(crate) struct Migrazione {
    /// La versione da cui parte. Produce sempre `da + 1`.
    pub(crate) da: u32,
    /// Cosa cambia.
    pub(crate) applica: fn(&mut Map<String, Value>) -> Esito<()>,
}

/// I passi di migrazione, in ordine di versione.
///
/// La prima voce è arrivata con la 3.0.0: il formato 1 diventa il 2. Cosa
/// cambia, e perché la skin migrata si disegna come prima, sta in
/// [`crate::migrazione`].
#[rustfmt::skip]
pub(crate) static MIGRAZIONI: &[Migrazione] = &[
    Migrazione { da: 1, applica: crate::migrazione::da_1_a_2 },
];

/// Porta un documento dalla sua versione alla nostra.
pub(crate) fn migra(raw: &Value, da: u64) -> Result<Value, Vec<SkinIssue>> {
    let mut map = oggetto(raw, "").map_err(solo)?.clone();
    let mut versione = u32::try_from(da).unwrap_or(u32::MAX);

    while versione < SKIN_FORMAT_VERSION {
        let Some(passo) = MIGRAZIONI.iter().find(|m| m.da == versione) else {
            // Un buco nella catena: c'è un documento di quella versione ma
            // nessuno sa portarlo avanti. Non si finge di riuscirci.
            return Err(solo(problema(
                "format",
                format!(
                    "il formato {versione} non si sa più leggere: \
                     nessuna migrazione porta da lì a {SKIN_FORMAT_VERSION}"
                ),
            )));
        };
        (passo.applica)(&mut map).map_err(solo)?;
        versione = passo.da.saturating_add(1);
        map.insert("format".to_owned(), Value::from(versione));
    }

    Ok(Value::Object(map))
}

/// Cosa dire di un testo che non è JSON, in italiano e senza la coda.
///
/// Il messaggio di serde è inglese e finisce con «at line 42 column 7», cioè
/// ripete dentro la frase i due numeri che qui viaggiano come numeri.
pub(crate) fn perche_non_e_json(err: &serde_json::Error) -> String {
    let inglese = err.to_string();
    let nudo = match inglese.find(" at line ") {
        Some(taglio) => inglese.get(..taglio).unwrap_or(&inglese),
        None => &inglese,
    };

    if err.classify() == serde_json::error::Category::Eof {
        return "il documento finisce prima di chiudersi: manca una parentesi, o una virgoletta"
            .to_owned();
    }
    for (segno, detto) in [
        (
            "expected `,` or `}`",
            "manca una virgola fra due campi, o la graffa che chiude l'oggetto",
        ),
        (
            "expected `,` or `]`",
            "manca una virgola fra due voci, o la quadra che chiude la lista",
        ),
        (
            "expected `:`",
            "dopo il nome di un campo ci va un due punti",
        ),
        (
            "key must be a string",
            "il nome di un campo va fra virgolette",
        ),
        (
            "trailing comma",
            "c'è una virgola di troppo prima della chiusura",
        ),
        (
            "trailing characters",
            "c'è altro dopo la fine del documento, o una parentesi che chiude due volte",
        ),
        (
            "control character while parsing a string",
            "una stringa non è chiusa: c'è un a capo dentro le virgolette",
        ),
        (
            "invalid escape",
            "dentro una stringa, una barra rovescia che non introduce niente",
        ),
        ("expected value", "qui ci vuole un valore, e non ce n'è uno"),
    ] {
        if nudo.contains(segno) {
            return detto.to_owned();
        }
    }
    format!("il documento non è JSON: {nudo}")
}

/// Il problema di sintassi di un testo che non è JSON.
pub(crate) fn non_e_json(err: &serde_json::Error) -> SkinIssue {
    let message = perche_non_e_json(err);
    SkinIssue {
        code: ErrorCode::SkinManifestInvalid {
            detail: Some(message.clone()),
        },
        path: String::new(),
        message,
        punto: Some(crate::posizioni::Punto {
            riga: u32::try_from(err.line()).unwrap_or(1).max(1),
            colonna: u32::try_from(err.column()).unwrap_or(1).max(1),
        }),
    }
}

/// Valida un documento skin scritto come testo JSON, elencando tutto.
///
/// Esiste accanto a [`parse_skin_json`] perché quello schiaccia fino a venti
/// problemi dentro **un** `AppError`, che è la forma giusta per un pacchetto
/// rifiutato e quella sbagliata per un editor: lo Studio vuole l'elenco, una
/// riga per problema, col suo percorso e il suo codice.
///
/// # Errori
///
/// Un problema solo — quello di sintassi, con la posizione — se il testo non è
/// JSON; l'elenco intero altrimenti.
pub fn leggi_skin(sorgente: &str) -> Result<SkinDocument, Vec<SkinIssue>> {
    let raw: Value = serde_json::from_str(sorgente).map_err(|err| solo(non_e_json(&err)))?;
    valida(&raw)
}

/// Valida un documento skin scritto come testo JSON.
///
/// # Errori
///
/// `skin.manifestInvalid` se il testo non è JSON; per il resto come
/// [`parse_skin`].
pub fn parse_skin_json(text: &str) -> Result<SkinDocument, AppError> {
    leggi_skin(text).map_err(|problemi| in_errore(&problemi))
}

#[cfg(test)]
mod tests;
