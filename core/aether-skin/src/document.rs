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

use aether_domain::errors::{AppError, ErrorCode};
use serde_json::{Map, Value};

use crate::effects::{
    Corner, Effect, EffectTarget, FiltroElemento, ImageSize, Paint, RadialShape, Stop,
};
use crate::layout::{
    Align, GapStep, LayoutNode, LayoutZone, MAX_SHELL_NODES, OptionKind, OptionValue, Spread,
    TrackSize, WidgetDef, WidgetInstance, WidgetOption, ZoneKind, shell_di_serie,
};
use crate::movimento::{
    AnimDirection, AnimFrame, AnimTrigger, Animation, AnimationRef, CASCATA_VOCI, Curva,
    Iterazioni, MAX_ANIMAZIONI, MAX_CASCATA_MS, MAX_DURATA_MS, MAX_FOTOGRAMMI, MAX_ITERAZIONI,
    MAX_RITARDO_MS, MIN_FOTOGRAMMI, PartMotion, Riempimento, Ritaglio, Transizione,
};
use crate::parts::{
    AnelloFuoco, BorderStyle, FONT_SIZE_MAX, FONT_SIZE_MIN, Ombra, PartAppearance, PartDef,
    PartLayer, PartStates, PartStyle, Raggio, TextTransform, nearest_parts, part, ritirata,
};
use crate::stati::PartState;
use crate::tokens::{
    ColorValue, DynamicSource, ShadowLayer, ShadowValue, TOKENS, TokenDef, TokenKind, TokenSet,
    TokenValue, is_local_name, token,
};
use crate::values::{
    Duration, Easing, EasingKeyword, FontKind, Length, LengthUnit, LengthValue, Rgba, StepPosition,
    is_font_family, parse_color, parse_duration, parse_length,
};
use crate::vicini::{forse, vicini};

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

// ── I valori ────────────────────────────────────────────────────────────────

fn colore_letterale(value: &Value, path: &str) -> Esito<Rgba> {
    let scritto = testo(value, path)?;
    parse_color(scritto).ok_or_else(|| {
        problema(
            path,
            format!(
                "«{scritto}» non è un colore. Ammessi #rgb, #rgba, #rrggbb, #rrggbbaa, rgb() e rgba()"
            ),
        )
    })
}

fn opacita(map: &Map<String, Value>, path: &str) -> Esito<Option<f64>> {
    campo(map, "alpha")
        .map(|value| numero_fra(value, &giu(path, "alpha"), 0.0, 1.0))
        .transpose()
}

/// Un colore: letterale, riferito a un token, alla tavolozza, o alla copertina.
///
/// Riconosce prima la forma e valida poi, invece di provare le quattro
/// alternative e riportare l'ultimo errore: chi scrive `blu` deve leggere «`blu`
/// non è un colore», non «input non valido».
fn colore(value: &Value, path: &str) -> Esito<ColorValue> {
    let Some(map) = value.as_object() else {
        return colore_letterale(value, path).map(ColorValue::Literal);
    };

    if let Some(grezzo) = campo(map, "$palette") {
        solo_chiavi(map, path, &["$palette", "alpha"])?;
        let nome = testo(grezzo, &giu(path, "$palette"))?;
        if !is_local_name(nome) {
            return Err(problema(
                &giu(path, "$palette"),
                "il nome di un colore della tavolozza ammette minuscole, cifre e trattini",
            ));
        }
        return Ok(ColorValue::Palette {
            name: nome.to_owned(),
            alpha: opacita(map, path)?,
        });
    }

    if let Some(grezzo) = campo(map, "$token") {
        solo_chiavi(map, path, &["$token"])?;
        let percorso = giu(path, "$token");
        let nome = testo(grezzo, &percorso)?;
        return token(nome).map(ColorValue::Token).ok_or_else(|| {
            let candidati = vicini(nome, TOKENS.iter().map(|def| def.id));
            problema(
                &percorso,
                format!("«{nome}» non è un token del registro.{}", forse(&candidati)),
            )
        });
    }

    if let Some(grezzo) = campo(map, "$source") {
        solo_chiavi(map, path, &["$source", "alpha"])?;
        let percorso = giu(path, "$source");
        let nome = testo(grezzo, &percorso)?;
        let source = DynamicSource::parse(nome).ok_or_else(|| {
            let nomi: Vec<&str> = DynamicSource::ALL.iter().map(|s| s.as_str()).collect();
            problema(
                &percorso,
                format!("sorgente sconosciuta. Ammesse: {}", nomi.join(", ")),
            )
        })?;
        return Ok(ColorValue::Source {
            source,
            alpha: opacita(map, path)?,
        });
    }

    Err(problema(
        path,
        "un colore va scritto come stringa (#rrggbb, rgba(…)), o come { \"$token\": … }, { \"$palette\": … }, { \"$source\": … }",
    ))
}

fn lunghezza(value: &Value, path: &str) -> Esito<Length> {
    let scritto = testo(value, path)?;
    parse_length(scritto).ok_or_else(|| {
        problema(
            path,
            format!(
                "«{scritto}» non è una lunghezza: ci vuole un numero più un'unità fra {}",
                LengthUnit::elenco()
            ),
        )
    })
}

/// Una lunghezza in una delle unità date.
fn lunghezza_in(value: &Value, path: &str, unita: &[LengthUnit]) -> Esito<Length> {
    let letta = lunghezza(value, path)?;
    if !unita.contains(&letta.unit) {
        let nomi: Vec<&str> = unita.iter().map(|u| u.as_str()).collect();
        return Err(problema(
            path,
            format!(
                "qui l'unità va scelta fra {}, non {}",
                nomi.join(", "),
                letta.unit.as_str()
            ),
        ));
    }
    Ok(letta)
}

/// Una misura assoluta fra due estremi in pixel; `rem` vale sedici.
fn misura_fra(value: &Value, path: &str, min: f64, max: f64) -> Esito<Length> {
    let letta = lunghezza_in(value, path, &[LengthUnit::Px, LengthUnit::Rem])?;
    let px = match letta.unit {
        LengthUnit::Rem => letta.value * 16.0,
        _ => letta.value,
    };
    if px < min || px > max {
        return Err(problema(
            path,
            format!("va fra {min}px e {max}px, non {px}px"),
        ));
    }
    Ok(letta)
}

/// Uno spostamento: un numero è in pixel, una stringa porta la sua unità.
///
/// I numeri nudi sono la forma del formato 1 — `"translateY": 8` — e restano
/// validi. Le percentuali servono a far uscire una parte dal suo bordo senza
/// sapere quanto è grande: `-100%` è «tutta fuori», qualunque sia la misura.
fn spostamento(value: &Value, path: &str) -> Esito<Length> {
    let letta = if value.is_number() {
        Length::px(numero(value, path)?)
    } else {
        lunghezza_in(
            value,
            path,
            &[
                LengthUnit::Px,
                LengthUnit::Percent,
                LengthUnit::Rem,
                LengthUnit::Em,
            ],
        )?
    };
    let (estremo, unita) = match letta.unit {
        LengthUnit::Percent => (200.0, "%"),
        LengthUnit::Rem | LengthUnit::Em => (25.0, letta.unit.as_str()),
        _ => (400.0, "px"),
    };
    if letta.value.abs() > estremo {
        return Err(problema(
            path,
            format!("va fra -{estremo}{unita} e {estremo}{unita}"),
        ));
    }
    Ok(letta)
}

fn lunghezza_valore(value: &Value, path: &str) -> Esito<LengthValue> {
    let Some(map) = value.as_object() else {
        return lunghezza(value, path).map(LengthValue::Fixed);
    };
    solo_chiavi(map, path, &["min", "preferred", "max"])?;
    Ok(LengthValue::Adaptive {
        min: lunghezza(richiesto(map, path, "min")?, &giu(path, "min"))?,
        preferred: lunghezza(richiesto(map, path, "preferred")?, &giu(path, "preferred"))?,
        max: lunghezza(richiesto(map, path, "max")?, &giu(path, "max"))?,
    })
}

fn durata(value: &Value, path: &str) -> Esito<Duration> {
    let scritto = testo(value, path)?;
    parse_duration(scritto).ok_or_else(|| {
        problema(
            path,
            format!(
                "«{scritto}» non è una durata: un numero in ms o s, al massimo {}ms",
                crate::values::MAX_DURATION_MS
            ),
        )
    })
}

fn easing(value: &Value, path: &str) -> Esito<Easing> {
    let map = oggetto(value, path)?;
    let genere = testo(richiesto(map, path, "kind")?, &giu(path, "kind"))?;
    match genere {
        "keyword" => {
            solo_chiavi(map, path, &["kind", "keyword"])?;
            let percorso = giu(path, "keyword");
            let scritto = testo(richiesto(map, path, "keyword")?, &percorso)?;
            EasingKeyword::parse(scritto)
                .map(Easing::Keyword)
                .ok_or_else(|| {
                    let nomi: Vec<&str> = EasingKeyword::ALL.iter().map(|k| k.as_str()).collect();
                    problema(&percorso, format!("ammesse soltanto: {}", nomi.join(", ")))
                })
        }
        "cubicBezier" => {
            solo_chiavi(map, path, &["kind", "points"])?;
            let percorso = giu(path, "points");
            let punti = lista_fra(richiesto(map, path, "points")?, &percorso, 4, 4)?;
            let mut valori = [0.0_f64; 4];
            for (indice, atteso) in valori.iter_mut().enumerate() {
                let Some(grezzo) = punti.get(indice) else {
                    return Err(problema(&percorso, "ci vogliono quattro numeri"));
                };
                let dentro = giu(&percorso, &indice.to_string());
                // La x sta in [0,1] per definizione della curva; la y può
                // uscirne, ed è così che si ottiene il rimbalzo di --ease-spring.
                *atteso = if indice % 2 == 0 {
                    numero_fra(grezzo, &dentro, 0.0, 1.0)?
                } else {
                    numero_fra(grezzo, &dentro, -5.0, 5.0)?
                };
            }
            Ok(Easing::CubicBezier(valori))
        }
        "steps" => {
            solo_chiavi(map, path, &["kind", "count", "position"])?;
            let conta = numero_fra(
                richiesto(map, path, "count")?,
                &giu(path, "count"),
                1.0,
                60.0,
            )?;
            let position = parola(
                richiesto(map, path, "position")?,
                &giu(path, "position"),
                &[("start", StepPosition::Start), ("end", StepPosition::End)],
            )?;
            Ok(Easing::Steps {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "`numero_fra` ha già rifiutato tutto ciò che non sta fra 1 e 60, e \
                              `round` toglie i decimali: qui non c'è niente da troncare né un \
                              segno da perdere"
                )]
                count: conta.round() as u32,
                position,
            })
        }
        altro => Err(problema(
            &giu(path, "kind"),
            format!("«{altro}» non è una curva: ammessi keyword, cubicBezier, steps"),
        )),
    }
}

/// Una curva di un'animazione: per esteso, una parola chiave, o il nome di una
/// curva della skin.
///
/// La stringa è il caso nuovo: `"easing": "molla"` richiama `motion.easings.molla`,
/// e `"easing": "linear"` è la parola chiave del CSS. Le due non si confondono
/// perché una curva della skin non può chiamarsi come una parola chiave —
/// [`movimento`] lo rifiuta alla dichiarazione.
fn curva(value: &Value, path: &str, dichiarate: &[(String, Easing)]) -> Esito<Curva> {
    let Some(scritto) = value.as_str() else {
        return easing(value, path).map(Curva::Valore);
    };
    if let Some(parola) = EasingKeyword::parse(scritto) {
        return Ok(Curva::Valore(Easing::Keyword(parola)));
    }
    if dichiarate.iter().any(|(nome, _)| nome == scritto) {
        return Ok(Curva::Nome(scritto.to_owned()));
    }
    let candidati = vicini(scritto, dichiarate.iter().map(|(n, _)| n.as_str()));
    Err(problema(
        path,
        format!(
            "«{scritto}» non è una curva: né una parola chiave ({}), né una curva dichiarata in \
             `motion.easings`.{}",
            EasingKeyword::ALL
                .iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            forse(&candidati)
        ),
    ))
}

fn ombra(value: &Value, path: &str) -> Esito<ShadowValue> {
    let map = oggetto(value, path)?;
    solo_chiavi(map, path, &["layers"])?;
    let percorso = giu(path, "layers");
    // Zero livelli è legittimo e vale `none`: è così che una skin piatta spegne
    // un'ombra senza doverne inventare una trasparente.
    let livelli = lista_fra(richiesto(map, path, "layers")?, &percorso, 0, 6)?;
    let mut layers = Vec::with_capacity(livelli.len());
    for (indice, grezzo) in livelli.iter().enumerate() {
        let dentro = giu(&percorso, &indice.to_string());
        let livello = oggetto(grezzo, &dentro)?;
        solo_chiavi(livello, &dentro, CHIAVI_LIVELLO_OMBRA)?;
        layers.push(ShadowLayer {
            inset: campo(livello, "inset")
                .map(|v| booleano(v, &giu(&dentro, "inset")))
                .transpose()?
                .unwrap_or(false),
            x: lunghezza(richiesto(livello, &dentro, "x")?, &giu(&dentro, "x"))?,
            y: lunghezza(richiesto(livello, &dentro, "y")?, &giu(&dentro, "y"))?,
            blur: lunghezza(richiesto(livello, &dentro, "blur")?, &giu(&dentro, "blur"))?,
            spread: campo(livello, "spread")
                .map(|v| lunghezza(v, &giu(&dentro, "spread")))
                .transpose()?,
            color: colore(
                richiesto(livello, &dentro, "color")?,
                &giu(&dentro, "color"),
            )?,
        });
    }
    Ok(ShadowValue { layers })
}

/// Un'ombra di parte: per esteso, o un token d'ombra richiamato.
fn ombra_parte(value: &Value, path: &str) -> Esito<Ombra> {
    let map = oggetto(value, path)?;
    let Some(grezzo) = campo(map, "$token") else {
        return ombra(value, path).map(Ombra::Valore);
    };
    solo_chiavi(map, path, &["$token"])?;
    let percorso = giu(path, "$token");
    let nome = testo(grezzo, &percorso)?;
    match token(nome) {
        Some(def) if def.kind == TokenKind::Shadow => Ok(Ombra::Token(def)),
        Some(_) => Err(problema(
            &percorso,
            format!("«{nome}» non è un token d'ombra: qui ci va uno fra shadow.* e glow.*"),
        )),
        None => {
            let candidati = vicini(
                nome,
                TOKENS
                    .iter()
                    .filter(|d| d.kind == TokenKind::Shadow)
                    .map(|d| d.id),
            );
            Err(problema(
                &percorso,
                format!("«{nome}» non è un token del registro.{}", forse(&candidati)),
            ))
        }
    }
}

fn pila_caratteri(value: &Value, path: &str) -> Esito<Vec<String>> {
    let voci = lista_fra(value, path, 1, 8)?;
    let mut famiglie = Vec::with_capacity(voci.len());
    for (indice, grezzo) in voci.iter().enumerate() {
        let dentro = giu(path, &indice.to_string());
        let nome = testo(grezzo, &dentro)?;
        if !is_font_family(nome) {
            return Err(problema(
                &dentro,
                "nome del carattere non valido: ammessi lettere, cifre, spazi, - e _",
            ));
        }
        famiglie.push(nome.to_owned());
    }
    Ok(famiglie)
}

/// Il nome di una voce del pacchetto, controllato con la regola del pacchetto.
fn risorsa(value: &Value, path: &str, mime_ammessi: &[&str]) -> Esito<String> {
    let nome = testo(value, path)?;
    let Some(mime) = crate::package::tipo_risorsa(nome) else {
        return Err(problema(
            path,
            format!(
                "«{nome}» non è il nome di una risorsa del pacchetto: minuscole, cifre, `_` e \
                 `-`, poi un'estensione fra png, jpg, webp e woff2 — senza cartelle"
            ),
        ));
    };
    if !mime_ammessi.contains(&mime) {
        return Err(problema(
            path,
            format!(
                "«{nome}» non è del tipo che serve qui ({})",
                mime_ammessi.join(", ")
            ),
        ));
    }
    Ok(nome.to_owned())
}

// ── Gli effetti ─────────────────────────────────────────────────────────────

fn fermate(value: &Value, path: &str) -> Esito<Vec<Stop>> {
    let voci = lista_fra(value, path, 2, 8)?;
    let mut stops = Vec::with_capacity(voci.len());
    for (indice, grezzo) in voci.iter().enumerate() {
        let dentro = giu(path, &indice.to_string());
        let map = oggetto(grezzo, &dentro)?;
        solo_chiavi(map, &dentro, CHIAVI_FERMATA)?;
        stops.push(Stop {
            color: colore(richiesto(map, &dentro, "color")?, &giu(&dentro, "color"))?,
            at: campo(map, "at")
                .map(|v| lunghezza(v, &giu(&dentro, "at")))
                .transpose()?,
        });
    }
    Ok(stops)
}

fn coppia(value: &Value, path: &str) -> Esito<(Length, Length)> {
    let voci = lista_fra(value, path, 2, 2)?;
    let (Some(x), Some(y)) = (voci.first(), voci.get(1)) else {
        return Err(problema(path, "ci vogliono due lunghezze"));
    };
    Ok((
        lunghezza(x, &giu(path, "0"))?,
        lunghezza(y, &giu(path, "1"))?,
    ))
}

fn centro(map: &Map<String, Value>, path: &str) -> Esito<Option<(Length, Length)>> {
    campo(map, "at")
        .map(|grezzo| coppia(grezzo, &giu(path, "at")))
        .transpose()
}

fn angolo(map: &Map<String, Value>, path: &str, name: &str, difetto: f64) -> Esito<f64> {
    campo(map, name)
        .map(|v| numero(v, &giu(path, name)))
        .transpose()
        .map(|v| v.unwrap_or(difetto))
}

fn lunghezza_facoltativa(
    map: &Map<String, Value>,
    path: &str,
    name: &str,
) -> Esito<Option<Length>> {
    campo(map, name)
        .map(|v| lunghezza(v, &giu(path, name)))
        .transpose()
}

/// Un effetto. Il campo `effect` è il discriminante, e l'unione è chiusa.
pub(crate) fn effetto(value: &Value, path: &str) -> Esito<Effect> {
    let map = oggetto(value, path)?;
    let percorso = giu(path, "effect");
    let nome = testo(richiesto(map, path, "effect")?, &percorso)?;

    let colore_di =
        |name: &str| -> Esito<ColorValue> { colore(richiesto(map, path, name)?, &giu(path, name)) };
    let lunghezza_di =
        |name: &str| -> Esito<Length> { lunghezza(richiesto(map, path, name)?, &giu(path, name)) };
    let fermate_di =
        || -> Esito<Vec<Stop>> { fermate(richiesto(map, path, "stops")?, &giu(path, "stops")) };

    if Effect::NAMES.contains(&nome) {
        solo_chiavi(map, path, chiavi_effetto(nome))?;
    }

    match nome {
        "solid" => Ok(Effect::Solid {
            color: colore_di("color")?,
        }),
        "linearGradient" => Ok(Effect::LinearGradient {
            angle: angolo(map, path, "angle", 180.0)?,
            stops: fermate_di()?,
        }),
        "radialGradient" => Ok(Effect::RadialGradient {
            shape: campo(map, "shape")
                .map(|v| {
                    parola(
                        v,
                        &giu(path, "shape"),
                        &[
                            ("circle", RadialShape::Circle),
                            ("ellipse", RadialShape::Ellipse),
                        ],
                    )
                })
                .transpose()?
                .unwrap_or(RadialShape::Ellipse),
            at: centro(map, path)?,
            size: lunghezza_facoltativa(map, path, "size")?,
            stops: fermate_di()?,
        }),
        "conicGradient" => Ok(Effect::ConicGradient {
            from: angolo(map, path, "from", 0.0)?,
            at: centro(map, path)?,
            stops: fermate_di()?,
        }),
        "hairlineGrid" => Ok(Effect::HairlineGrid {
            color: colore_di("color")?,
            cell: lunghezza_di("cell")?,
            cell_y: lunghezza_facoltativa(map, path, "cellY")?,
            thickness: lunghezza_facoltativa(map, path, "thickness")?,
        }),
        "scanlines" => Ok(Effect::Scanlines {
            color: colore_di("color")?,
            line: lunghezza_di("line")?,
            gap: lunghezza_di("gap")?,
        }),
        "stripes" => Ok(Effect::Stripes {
            angle: angolo(map, path, "angle", -45.0)?,
            color: colore_di("color")?,
            background: colore_di("background")?,
            width: lunghezza_di("width")?,
        }),
        "dotGrid" => Ok(Effect::DotGrid {
            color: colore_di("color")?,
            spacing: lunghezza_di("spacing")?,
            dot: lunghezza_di("dot")?,
        }),
        "vignette" => Ok(Effect::Vignette {
            color: colore_di("color")?,
            start: campo(map, "start")
                .map(|v| numero_fra(v, &giu(path, "start"), 0.0, 100.0))
                .transpose()?
                .unwrap_or(60.0),
        }),
        "chamfer" => {
            let corners = match campo(map, "corners") {
                None => vec![Corner::TopRight, Corner::BottomLeft],
                Some(grezzo) => {
                    let dentro = giu(path, "corners");
                    let voci = lista_fra(grezzo, &dentro, 1, 4)?;
                    let mut angoli = Vec::with_capacity(voci.len());
                    for (indice, voce) in voci.iter().enumerate() {
                        let nomi: Vec<(&str, Corner)> =
                            Corner::ALL.iter().map(|c| (c.as_str(), *c)).collect();
                        angoli.push(parola(voce, &giu(&dentro, &indice.to_string()), &nomi)?);
                    }
                    angoli
                }
            };
            Ok(Effect::Chamfer {
                size: lunghezza_di("size")?,
                corners,
            })
        }
        "blurBehind" => Ok(Effect::BlurBehind {
            radius: lunghezza_di("radius")?,
            saturate: campo(map, "saturate")
                .map(|v| numero_fra(v, &giu(path, "saturate"), 0.0, 400.0))
                .transpose()?,
        }),
        "image" => Ok(Effect::Image {
            src: risorsa(
                richiesto(map, path, "src")?,
                &giu(path, "src"),
                &["image/png", "image/jpeg", "image/webp"],
            )?,
            size: match campo(map, "size") {
                None => ImageSize::Cover,
                Some(grezzo) => match grezzo.as_str() {
                    Some("cover") => ImageSize::Cover,
                    Some("contain") => ImageSize::Contain,
                    Some("auto") => ImageSize::Auto,
                    _ => {
                        ImageSize::Misura(lunghezza(grezzo, &giu(path, "size")).map_err(|_| {
                            problema(
                                &giu(path, "size"),
                                "ammessi cover, contain, auto, o una lunghezza per le mattonelle",
                            )
                        })?)
                    }
                },
            },
            repeat: campo(map, "repeat")
                .map(|v| booleano(v, &giu(path, "repeat")))
                .transpose()?
                .unwrap_or(false),
            position: campo(map, "position")
                .map(|v| coppia(v, &giu(path, "position")))
                .transpose()?,
        }),
        altro => {
            let candidati = vicini(altro, Effect::NAMES.iter().copied());
            Err(SkinIssue {
                code: ErrorCode::SkinUnknownEffect {
                    effect_type: altro.to_owned(),
                },
                path: percorso.clone(),
                message: format!("effetto sconosciuto: «{altro}».{}", forse(&candidati)),
                punto: None,
            })
        }
    }
}

// ── I token ─────────────────────────────────────────────────────────────────

fn valore_token(def: &TokenDef, value: &Value, path: &str) -> Esito<TokenValue> {
    let esito = match def.kind {
        TokenKind::Color => colore(value, path).map(TokenValue::Color),
        TokenKind::Length => lunghezza_valore(value, path).map(TokenValue::Length),
        TokenKind::Duration => durata(value, path).map(TokenValue::Duration),
        TokenKind::Easing => easing(value, path).map(TokenValue::Easing),
        TokenKind::Number => numero(value, path).map(TokenValue::Number),
        TokenKind::FontStack => pila_caratteri(value, path).map(TokenValue::FontStack),
        TokenKind::Shadow => ombra(value, path).map(TokenValue::Shadow),
    };
    let esito = esito.and_then(|valore| entro_gli_estremi(def, &valore, path).map(|()| valore));
    // Il messaggio preciso viene da sotto; qui si aggiunge soltanto il codice
    // che nomina il token, che è ciò che serve a chi legge un log invece del
    // documento.
    esito.map_err(|dentro| token_non_valido(&dentro.path, def.id, value, dentro.message))
}

/// Gli estremi di [`TokenDef::limiti`], quando ci sono.
///
/// Dove c'è un estremo l'unità è assoluta: `64%` non è «più piccolo» di `64px`,
/// e le unità relative alla finestra sono esattamente il modo in cui la
/// garanzia salterebbe. `rem` si converte alla sua radice di 16px.
fn entro_gli_estremi(def: &TokenDef, valore: &TokenValue, path: &str) -> Esito<()> {
    let Some((min, max)) = def.limiti else {
        return Ok(());
    };

    let fuori = |n: f64| problema(path, format!("va fra {min} e {max}, non {n}"));

    match *valore {
        TokenValue::Number(n) => {
            if n < min || n > max {
                return Err(fuori(n));
            }
        }
        TokenValue::Length(lunghezza) => {
            let misure = match lunghezza {
                LengthValue::Fixed(l) => vec![l],
                LengthValue::Adaptive {
                    min: a,
                    preferred: b,
                    max: c,
                } => vec![a, b, c],
            };
            for misura in misure {
                let px = match misura.unit {
                    LengthUnit::Px => misura.value,
                    LengthUnit::Rem => misura.value * 16.0,
                    altra => {
                        return Err(problema(
                            path,
                            format!(
                                "qui ci va una misura assoluta, in px o rem, non in {}",
                                altra.as_str()
                            ),
                        ));
                    }
                };
                if px < min || px > max {
                    return Err(fuori(px));
                }
            }
        }
        _ => {}
    }

    Ok(())
}

fn insieme_token(value: &Value, path: &str, problemi: &mut Vec<SkinIssue>) -> Esito<TokenSet> {
    let map = oggetto(value, path)?;
    let mut voci = Vec::with_capacity(map.len());
    for (chiave, grezzo) in map {
        let percorso = giu(path, chiave);
        let Some(def) = token(chiave) else {
            let candidati = vicini(chiave, TOKENS.iter().map(|d| d.id));
            problemi.push(problema(
                &percorso,
                format!("token inesistente: «{chiave}».{}", forse(&candidati)),
            ));
            continue;
        };
        match valore_token(def, grezzo, &percorso) {
            Ok(valore) => voci.push((def, valore)),
            Err(guasto) => problemi.push(guasto),
        }
    }
    Ok(TokenSet::new(voci))
}

// ── Le parti ────────────────────────────────────────────────────────────────

/// Quel che una parte può richiamare per nome, letto prima delle parti.
///
/// Un richiamo si risolve solo se quel che richiama esiste già: motivi,
/// animazioni e curve si leggono prima, e le parti li trovano qui.
struct Richiamabili<'a> {
    motivi: &'a [(String, Effect)],
    animazioni: &'a [(String, Animation)],
    curve: &'a [(String, Easing)],
}

/// Uno strato di pittura: `{"$pattern": "nome"}`, oppure un effetto per esteso.
///
/// Un **motivo** nel posto sbagliato produce un nome di variabile che non
/// esiste — `--skin-x-filter` richiesto come `var(--skin-x)` — cioè un
/// riferimento che risolve nel nulla senza che nessuno lo dica. È l'unico caso
/// in cui l'errore è invisibile, ed è per questo che è l'unico controllato.
fn pittura(
    value: &Value,
    path: &str,
    motivi: &[(String, Effect)],
    dove: EffectTarget,
) -> Esito<Paint> {
    let map = oggetto(value, path)?;
    let Some(riferimento) = campo(map, "$pattern") else {
        return effetto(value, path).map(Paint::Inline);
    };

    solo_chiavi(map, path, &["$pattern"])?;
    let percorso = giu(path, "$pattern");
    let nome = testo(riferimento, &percorso)?;
    let Some((_, def)) = motivi.iter().find(|(n, _)| n == nome) else {
        let candidati = vicini(nome, motivi.iter().map(|(n, _)| n.as_str()));
        return Err(problema(
            &percorso,
            format!("motivo inesistente: «{nome}».{}", forse(&candidati)),
        ));
    };

    if def.target() != dove {
        return Err(problema(
            &percorso,
            format!(
                "il motivo «{nome}» è un {}, e qui ci va un {}",
                nome_destinazione(def.target()),
                nome_destinazione(dove)
            ),
        ));
    }

    Ok(Paint::Pattern {
        name: nome.to_owned(),
        def: def.clone(),
    })
}

/// Come si chiama una destinazione nel messaggio d'errore.
const fn nome_destinazione(target: EffectTarget) -> &'static str {
    match target {
        EffectTarget::Background => "sfondo",
        EffectTarget::ClipPath => "ritaglio",
        EffectTarget::Filter => "filtro di quel che sta dietro",
    }
}

fn livelli(
    value: &Value,
    path: &str,
    min: usize,
    max: usize,
    motivi: &[(String, Effect)],
) -> Esito<Vec<Paint>> {
    let voci = lista_fra(value, path, min, max)?;
    let mut strati = Vec::with_capacity(voci.len());
    for (indice, grezzo) in voci.iter().enumerate() {
        strati.push(pittura(
            grezzo,
            &giu(path, &indice.to_string()),
            motivi,
            EffectTarget::Background,
        )?);
    }
    Ok(strati)
}

/// Il raggio: una lunghezza, o un oggetto con gli angoli.
fn raggio(value: &Value, path: &str) -> Esito<Raggio> {
    let Some(map) = value.as_object() else {
        return lunghezza(value, path).map(Raggio::Uguale);
    };
    solo_chiavi(map, path, CHIAVI_ANGOLI)?;
    if map.is_empty() {
        return Err(problema(
            path,
            "un raggio per angolo deve nominare almeno un angolo",
        ));
    }
    Ok(Raggio::Angoli {
        top_left: lunghezza_facoltativa(map, path, "topLeft")?,
        top_right: lunghezza_facoltativa(map, path, "topRight")?,
        bottom_right: lunghezza_facoltativa(map, path, "bottomRight")?,
        bottom_left: lunghezza_facoltativa(map, path, "bottomLeft")?,
    })
}

/// I filtri sull'elemento: una lista di oggetti a una chiave sola.
///
/// La forma è `[{ "blur": "2px" }, { "saturate": 1.4 }]`, e non un oggetto con
/// tante chiavi, perché l'ordine dei filtri cambia il risultato — una
/// sfocatura prima della saturazione non è una saturazione prima della
/// sfocatura — e l'ordine delle chiavi di un oggetto non è qualcosa su cui
/// fare affidamento.
fn filtri_elemento(value: &Value, path: &str) -> Esito<Vec<FiltroElemento>> {
    if value.is_object() {
        return Err(problema(
            path,
            "qui ci va una lista di filtri sull'elemento, come [{ \"blur\": \"2px\" }]. Il filtro \
             di quel che sta dietro — la sfocatura del vetro — si chiama `backdrop`",
        ));
    }
    let voci = lista_fra(value, path, 1, 6)?;
    let mut filtri = Vec::with_capacity(voci.len());
    for (indice, grezzo) in voci.iter().enumerate() {
        let dentro = giu(path, &indice.to_string());
        let map = oggetto(grezzo, &dentro)?;
        let mut chiavi = map.keys();
        let (Some(nome), None) = (chiavi.next(), chiavi.next()) else {
            return Err(problema(
                &dentro,
                "ogni filtro è un oggetto con una chiave sola: il nome del filtro",
            ));
        };
        let valore = map.get(nome).unwrap_or(&Value::Null);
        let qui = giu(&dentro, nome);
        filtri.push(match nome.as_str() {
            "blur" => FiltroElemento::Blur(misura_fra(valore, &qui, 0.0, 40.0)?),
            "brightness" => FiltroElemento::Brightness(numero_fra(valore, &qui, 0.0, 3.0)?),
            "contrast" => FiltroElemento::Contrast(numero_fra(valore, &qui, 0.0, 3.0)?),
            "saturate" => FiltroElemento::Saturate(numero_fra(valore, &qui, 0.0, 3.0)?),
            "grayscale" => FiltroElemento::Grayscale(numero_fra(valore, &qui, 0.0, 1.0)?),
            "hueRotate" => FiltroElemento::HueRotate(numero_fra(valore, &qui, -360.0, 360.0)?),
            "dropShadow" => {
                let ombra = oggetto(valore, &qui)?;
                solo_chiavi(ombra, &qui, &["x", "y", "blur", "color"])?;
                FiltroElemento::DropShadow {
                    x: lunghezza(richiesto(ombra, &qui, "x")?, &giu(&qui, "x"))?,
                    y: lunghezza(richiesto(ombra, &qui, "y")?, &giu(&qui, "y"))?,
                    blur: lunghezza(richiesto(ombra, &qui, "blur")?, &giu(&qui, "blur"))?,
                    color: colore(richiesto(ombra, &qui, "color")?, &giu(&qui, "color"))?,
                }
            }
            altro => {
                let candidati = vicini(altro, FiltroElemento::NAMES.iter().copied());
                return Err(problema(
                    &qui,
                    format!("filtro sconosciuto: «{altro}».{}", forse(&candidati)),
                ));
            }
        });
    }
    Ok(filtri)
}

fn aspetto(value: &Value, path: &str, motivi: &[(String, Effect)]) -> Esito<PartAppearance> {
    let map = oggetto(value, path)?;
    solo_chiavi(map, path, CHIAVI_ASPETTO)?;
    Ok(PartAppearance {
        background: match campo(map, "background") {
            None => Vec::new(),
            Some(grezzo) => livelli(grezzo, &giu(path, "background"), 0, 4, motivi)?,
        },
        text_color: campo(map, "textColor")
            .map(|v| colore(v, &giu(path, "textColor")))
            .transpose()?,
        border_color: campo(map, "borderColor")
            .map(|v| colore(v, &giu(path, "borderColor")))
            .transpose()?,
        border_width: lunghezza_facoltativa(map, path, "borderWidth")?,
        border_style: campo(map, "borderStyle")
            .map(|v| {
                let nomi: Vec<(&str, BorderStyle)> =
                    BorderStyle::ALL.iter().map(|s| (s.as_str(), *s)).collect();
                parola(v, &giu(path, "borderStyle"), &nomi)
            })
            .transpose()?,
        radius: campo(map, "radius")
            .map(|v| raggio(v, &giu(path, "radius")))
            .transpose()?,
        shadow: campo(map, "shadow")
            .map(|v| ombra_parte(v, &giu(path, "shadow")))
            .transpose()?,
        focus_ring: campo(map, "focusRing")
            .map(|v| {
                let dentro = giu(path, "focusRing");
                let anello = oggetto(v, &dentro)?;
                solo_chiavi(anello, &dentro, CHIAVI_ANELLO)?;
                Ok(AnelloFuoco {
                    color: colore(richiesto(anello, &dentro, "color")?, &giu(&dentro, "color"))?,
                    width: misura_fra(
                        richiesto(anello, &dentro, "width")?,
                        &giu(&dentro, "width"),
                        0.0,
                        8.0,
                    )?,
                    offset: campo(anello, "offset")
                        .map(|o| {
                            let letta = lunghezza_in(
                                o,
                                &giu(&dentro, "offset"),
                                &[LengthUnit::Px, LengthUnit::Rem],
                            )?;
                            if letta.value.abs() > 12.0 {
                                return Err(problema(
                                    &giu(&dentro, "offset"),
                                    "va fra -12px e 12px",
                                ));
                            }
                            Ok(letta)
                        })
                        .transpose()?,
                })
            })
            .transpose()?,
        clip: campo(map, "clip")
            .map(|v| pittura(v, &giu(path, "clip"), motivi, EffectTarget::ClipPath))
            .transpose()?,
        filter: match campo(map, "filter") {
            None => Vec::new(),
            Some(grezzo) => filtri_elemento(grezzo, &giu(path, "filter"))?,
        },
        backdrop: campo(map, "backdrop")
            .map(|v| pittura(v, &giu(path, "backdrop"), motivi, EffectTarget::Filter))
            .transpose()?,
        opacity: campo(map, "opacity")
            .map(|v| numero_fra(v, &giu(path, "opacity"), 0.0, 1.0))
            .transpose()?,
        font: campo(map, "font")
            .map(|v| {
                parola(
                    v,
                    &giu(path, "font"),
                    &[
                        ("sans", FontKind::Sans),
                        ("mono", FontKind::Mono),
                        ("display", FontKind::Display),
                    ],
                )
            })
            .transpose()?,
        font_size: campo(map, "fontSize")
            .map(|v| misura_fra(v, &giu(path, "fontSize"), FONT_SIZE_MIN, FONT_SIZE_MAX))
            .transpose()?,
        line_height: campo(map, "lineHeight")
            .map(|v| numero_fra(v, &giu(path, "lineHeight"), 0.8, 3.0))
            .transpose()?,
        letter_spacing: lunghezza_facoltativa(map, path, "letterSpacing")?,
        text_transform: campo(map, "textTransform")
            .map(|v| {
                let nomi: Vec<(&str, TextTransform)> = TextTransform::ALL
                    .iter()
                    .map(|t| (t.as_str(), *t))
                    .collect();
                parola(v, &giu(path, "textTransform"), &nomi)
            })
            .transpose()?,
        font_weight: campo(map, "fontWeight")
            .map(|v| numero_fra(v, &giu(path, "fontWeight"), 1.0, 1000.0))
            .transpose()?,
    })
}

/// Un livello su `::before` o `::after`, se la parte ne ha uno libero.
fn livello(
    def: &PartDef,
    quale: &str,
    libero: bool,
    value: &Value,
    path: &str,
    motivi: &[(String, Effect)],
) -> Esito<PartLayer> {
    if !libero {
        // Non è un capriccio: lo pseudo-elemento di questa parte è già usato
        // dal componente, o la parte è un'immagine e non ne ha. Una skin che ci
        // scrivesse sopra non otterrebbe il suo livello — e toglierebbe quel che
        // c'era, o non disegnerebbe niente.
        return Err(problema(
            path,
            format!(
                "la parte «{}» non ha `::{quale}` libero per un livello",
                def.name
            ),
        ));
    }
    let map = oggetto(value, path)?;
    solo_chiavi(map, path, CHIAVI_LIVELLO)?;
    Ok(PartLayer {
        background: livelli(
            richiesto(map, path, "background")?,
            &giu(path, "background"),
            1,
            3,
            motivi,
        )?,
        opacity: campo(map, "opacity")
            .map(|v| numero_fra(v, &giu(path, "opacity"), 0.0, 1.0))
            .transpose()?,
    })
}

/// Il movimento di una parte: `parts.<p>.motion`.
fn movimento_parte(
    def: &'static PartDef,
    value: &Value,
    path: &str,
    richiamabili: &Richiamabili<'_>,
) -> Esito<PartMotion> {
    let map = oggetto(value, path)?;
    let mut motion = PartMotion::default();

    for (chiave, grezzo) in map {
        let percorso = giu(path, chiave);
        match chiave.as_str() {
            "stagger" => {
                let passo = durata(grezzo, &percorso)?;
                if passo.ms > MAX_CASCATA_MS {
                    return Err(problema(
                        &percorso,
                        format!("il passo della cascata va fino a {MAX_CASCATA_MS}ms"),
                    ));
                }
                motion.stagger = Some(passo);
            }
            "transition" => {
                let blocco = oggetto(grezzo, &percorso)?;
                solo_chiavi(blocco, &percorso, CHIAVI_TRANSIZIONE)?;
                let tempo = durata(
                    richiesto(blocco, &percorso, "duration")?,
                    &giu(&percorso, "duration"),
                )?;
                if tempo.ms > MAX_DURATA_MS {
                    return Err(problema(
                        &giu(&percorso, "duration"),
                        format!("al massimo {MAX_DURATA_MS}ms"),
                    ));
                }
                motion.transition = Some(Transizione {
                    duration: tempo,
                    easing: campo(blocco, "easing")
                        .map(|v| curva(v, &giu(&percorso, "easing"), richiamabili.curve))
                        .transpose()?
                        .unwrap_or_default(),
                });
            }
            altro => {
                let Some(trigger) = AnimTrigger::parse(altro) else {
                    let nomi: Vec<&str> = AnimTrigger::nomi()
                        .into_iter()
                        .chain(CHIAVI_MOVIMENTO_PARTE.iter().copied())
                        .collect();
                    let candidati = vicini(altro, nomi.iter().copied());
                    let dopo = if altro == "active" {
                        " Nel formato 2 lo stato che si chiamava «active» si chiama «current»."
                    } else {
                        ""
                    };
                    return Err(problema(
                        &percorso,
                        format!(
                            "innesco sconosciuto: «{altro}».{}{dopo} Ammessi: {}.",
                            forse(&candidati),
                            nomi.join(", ")
                        ),
                    ));
                };
                if trigger == AnimTrigger::Exit && !def.presenza {
                    return Err(problema(
                        &percorso,
                        format!(
                            "«{}» non entra ed esce dal documento come un'unità, quindi non ha \
                             un'uscita da animare: l'uscita esiste solo sulle presenze — menù, \
                             modali, notifiche, la colonna, la coda, il contenuto della pagina",
                            def.name
                        ),
                    ));
                }
                if trigger == AnimTrigger::Share && !def.condivisa {
                    return Err(problema(
                        &percorso,
                        format!(
                            "«{}» non vola fra due schermate: il volo condiviso esiste solo sulle \
                             copertine",
                            def.name
                        ),
                    ));
                }
                let richiamata = testo(grezzo, &percorso)?;
                let Some((_, animazione)) = richiamabili
                    .animazioni
                    .iter()
                    .find(|(nome, _)| nome == richiamata)
                else {
                    let candidati = vicini(
                        richiamata,
                        richiamabili
                            .animazioni
                            .iter()
                            .map(|(nome, _)| nome.as_str()),
                    );
                    return Err(problema(
                        &percorso,
                        format!(
                            "animazione inesistente: «{richiamata}». \
                             Le animazioni si dichiarano in `motion.animations`.{}",
                            forse(&candidati)
                        ),
                    ));
                };
                if animazione.iterations == Iterazioni::Infinite && !trigger.ammette_ciclo() {
                    return Err(problema(
                        &percorso,
                        format!(
                            "«{richiamata}» si ripete all'infinito, e «{altro}» non è uno stato: \
                             un ciclo dura quanto lo stato che lo accende, e un'entrata, \
                             un'uscita o un cambio non finirebbero mai. Mettilo su uno stato — \
                             «playing», «hover», «current» — o dagli un numero di ripetizioni"
                        ),
                    ));
                }
                motion.set(
                    trigger,
                    AnimationRef {
                        name: richiamata.to_owned(),
                        def: animazione.clone(),
                    },
                );
            }
        }
    }

    Ok(motion)
}

/// Una parte, nella base o nella variante.
///
/// Nella variante il movimento non c'è: un tema cambia i colori e le superfici,
/// non come le cose si muovono, e una parte che si animasse in modo diverso a
/// seconda del tema sarebbe una seconda skin nascosta dentro la prima.
fn stile_parte(
    nome: &str,
    value: &Value,
    path: &str,
    richiamabili: &Richiamabili<'_>,
    nella_variante: bool,
) -> Esito<Option<PartStyle>> {
    let Some(def) = part(nome) else {
        // Una parte ritirata si accetta e non fa niente: il nome è pubblico, e
        // un nome sconosciuto qui **ferma il caricamento** dell'intera skin.
        if ritirata(nome).is_some() {
            return Ok(None);
        }
        let candidati = nearest_parts(nome);
        return Err(problema(
            path,
            format!("parte inesistente: «{nome}».{}", forse(&candidati)),
        ));
    };
    let map = oggetto(value, path)?;

    let ammesse: Vec<&str> = CHIAVI_ASPETTO
        .iter()
        .chain(CHIAVI_STRUTTURA_PARTE.iter())
        .copied()
        .collect();
    solo_chiavi(map, path, &ammesse)?;
    if nella_variante && campo(map, "motion").is_some() {
        return Err(problema(
            &giu(path, "motion"),
            "il movimento non cambia col tema: dichiaralo sulla parte della skin, fuori da `variant`",
        ));
    }

    // L'aspetto e i blocchi strutturali stanno nello stesso oggetto: si toglie
    // quel che non è aspetto e si valida il resto con la stessa funzione degli
    // stati, così base e stati non possono divergere.
    let mut solo_aspetto = map.clone();
    for chiave in CHIAVI_STRUTTURA_PARTE {
        solo_aspetto.remove(*chiave);
    }
    let appearance = aspetto(&Value::Object(solo_aspetto), path, richiamabili.motivi)?;

    let before = campo(map, "before")
        .map(|v| {
            livello(
                def,
                "before",
                def.before,
                v,
                &giu(path, "before"),
                richiamabili.motivi,
            )
        })
        .transpose()?;
    let after = campo(map, "after")
        .map(|v| {
            livello(
                def,
                "after",
                def.after,
                v,
                &giu(path, "after"),
                richiamabili.motivi,
            )
        })
        .transpose()?;

    let mut stati = Vec::new();
    if let Some(grezzo) = campo(map, "states") {
        let dentro = giu(path, "states");
        let blocco = oggetto(grezzo, &dentro)?;
        for (chiave, valore) in blocco {
            let Some(stato) = PartState::parse(chiave) else {
                let candidati = vicini(chiave, PartState::ALL.iter().map(|s| s.as_str()));
                let dopo = if chiave == "active" {
                    " Nel formato 2 lo stato che si chiamava «active» si chiama «current»."
                } else {
                    ""
                };
                return Err(problema(
                    &giu(&dentro, chiave),
                    format!(
                        "stato sconosciuto: «{chiave}».{}{dopo} Ammessi: {}.",
                        forse(&candidati),
                        PartState::nomi().join(", ")
                    ),
                ));
            };
            stati.push((
                stato,
                aspetto(valore, &giu(&dentro, chiave), richiamabili.motivi)?,
            ));
        }
    }

    let motion = match campo(map, "motion") {
        None => PartMotion::default(),
        Some(grezzo) => movimento_parte(def, grezzo, &giu(path, "motion"), richiamabili)?,
    };

    Ok(Some(PartStyle {
        def,
        appearance,
        before,
        after,
        states: PartStates::new(stati),
        motion,
    }))
}

fn superfici(
    value: &Value,
    path: &str,
    richiamabili: &Richiamabili<'_>,
    nella_variante: bool,
    problemi: &mut Vec<SkinIssue>,
) -> Esito<Vec<PartStyle>> {
    let map = oggetto(value, path)?;
    let mut stili = Vec::with_capacity(map.len());
    for (nome, grezzo) in map {
        match stile_parte(nome, grezzo, &giu(path, nome), richiamabili, nella_variante) {
            Ok(Some(stile)) => stili.push(stile),
            // Una parte ritirata: letta, accettata, e lasciata cadere qui.
            Ok(None) => {}
            Err(guasto) => problemi.push(guasto),
        }
    }
    // Ordine di registro e non alfabetico: nel CSS l'ordine decide la cascata a
    // parità di specificità, e il registro va dal generale al particolare.
    stili.sort_by_key(|stile| crate::parts::posto(stile.def.name));
    Ok(stili)
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

fn stringa_lunga(value: &Value, path: &str, min: usize, max: usize) -> Esito<String> {
    let scritto = testo(value, path)?.trim();
    if scritto.chars().count() < min || scritto.chars().count() > max {
        return Err(problema(path, format!("va da {min} a {max} caratteri")));
    }
    Ok(scritto.to_owned())
}

fn versione(value: &Value, path: &str) -> Esito<String> {
    let scritto = testo(value, path)?;
    let pezzi: Vec<&str> = scritto.split('.').collect();
    let ammessa = pezzi.len() == 3
        && pezzi
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    if !ammessa {
        return Err(problema(path, "la versione va scritta come 1.0.0"));
    }
    Ok(scritto.to_owned())
}

/// I tre colori della scheda. Esattamente tre: la scheda ha tre fasce.
fn anteprima(value: &Value, path: &str) -> Esito<[Rgba; 3]> {
    let voci = lista_fra(value, path, 3, 3)?;
    let mut colori = [Rgba {
        r: 0,
        g: 0,
        b: 0,
        a: 1.0,
    }; 3];
    for (indice, atteso) in colori.iter_mut().enumerate() {
        let Some(grezzo) = voci.get(indice) else {
            return Err(problema(path, "ci vogliono tre colori"));
        };
        *atteso = colore_letterale(grezzo, &giu(path, &indice.to_string()))?;
    }
    Ok(colori)
}

fn meta(value: &Value, path: &str) -> Esito<SkinMeta> {
    let map = oggetto(value, path)?;
    solo_chiavi(map, path, CHIAVI_META)?;
    Ok(SkinMeta {
        name: stringa_lunga(richiesto(map, path, "name")?, &giu(path, "name"), 1, 64)?,
        author: stringa_lunga(richiesto(map, path, "author")?, &giu(path, "author"), 1, 64)?,
        version: versione(richiesto(map, path, "version")?, &giu(path, "version"))?,
        description: campo(map, "description")
            .map(|v| stringa_lunga(v, &giu(path, "description"), 0, 280))
            .transpose()?,
        license: campo(map, "license")
            .map(|v| stringa_lunga(v, &giu(path, "license"), 0, 64))
            .transpose()?,
        based_on: campo(map, "basedOn")
            .map(|v| identificatore(v, &giu(path, "basedOn")))
            .transpose()?,
        preview: campo(map, "preview")
            .map(|v| anteprima(v, &giu(path, "preview")))
            .transpose()?,
    })
}

fn capacita(value: &Value, path: &str) -> Esito<SkinCapabilities> {
    let map = oggetto(value, path)?;
    for chiave in ["light", "mobile"] {
        if map.contains_key(chiave) {
            return Err(problema(
                &giu(path, chiave),
                if chiave == "light" {
                    "nel formato 2 il tema opposto si dichiara scrivendolo, in `variant`: una \
                     capacità vuota non può più promettere quel che non c'è"
                } else {
                    "nel formato 2 non c'è più: le sovrascritture per telefono stanno in \
                     `platforms.mobile`, e la loro presenza è la dichiarazione"
                },
            ));
        }
    }
    solo_chiavi(map, path, CHIAVI_CAPACITA)?;
    Ok(SkinCapabilities {
        dynamic_accent: campo(map, "dynamicAccent")
            .map(|v| booleano(v, &giu(path, "dynamicAccent")))
            .transpose()?
            .unwrap_or(SkinCapabilities::default().dynamic_accent),
    })
}

/// I caratteri del pacchetto: al più otto famiglie.
fn caratteri(value: &Value, path: &str) -> Esito<Vec<FontFace>> {
    let map = oggetto(value, path)?;
    if map.len() > 8 {
        return Err(problema(path, "al massimo otto caratteri per skin"));
    }
    let mut elenco = Vec::with_capacity(map.len());
    for (famiglia, grezzo) in map {
        let dentro = giu(path, famiglia);
        if !is_font_family(famiglia) {
            return Err(problema(
                &dentro,
                "il nome di una famiglia ammette lettere, cifre, spazi, - e _",
            ));
        }
        let voce = oggetto(grezzo, &dentro)?;
        solo_chiavi(voce, &dentro, CHIAVI_FONT)?;
        let weight = match campo(voce, "weight") {
            None => (400, 400),
            Some(peso) if peso.is_array() => {
                let coppia = lista_fra(peso, &giu(&dentro, "weight"), 2, 2)?;
                let a = numero_fra(
                    coppia.first().unwrap_or(&Value::Null),
                    &giu(&dentro, "weight.0"),
                    1.0,
                    1000.0,
                )?;
                let b = numero_fra(
                    coppia.get(1).unwrap_or(&Value::Null),
                    &giu(&dentro, "weight.1"),
                    1.0,
                    1000.0,
                )?;
                if a > b {
                    return Err(problema(
                        &giu(&dentro, "weight"),
                        "l'intervallo dei pesi va dal più leggero al più pesante",
                    ));
                }
                (peso_intero(a), peso_intero(b))
            }
            Some(peso) => {
                let unico = numero_fra(peso, &giu(&dentro, "weight"), 1.0, 1000.0)?;
                (peso_intero(unico), peso_intero(unico))
            }
        };
        elenco.push(FontFace {
            family: famiglia.clone(),
            src: risorsa(
                richiesto(voce, &dentro, "src")?,
                &giu(&dentro, "src"),
                &["font/woff2"],
            )?,
            weight,
            italic: campo(voce, "style")
                .map(|v| {
                    parola(
                        v,
                        &giu(&dentro, "style"),
                        &[("normal", false), ("italic", true)],
                    )
                })
                .transpose()?
                .unwrap_or(false),
        });
    }
    // Due famiglie che nel foglio diventano lo stesso nome.
    //
    // Il compilatore dà a ogni famiglia un nome suo, ridotto a minuscole,
    // cifre e trattini ([`famiglia_ridotta`]): «Titoli A» e «titoli-a» sono
    // due chiavi diverse qui e un nome solo là. Fino alla 3.0 passavano
    // tutte e due, e il foglio dichiarava due `@font-face` con la stessa
    // famiglia e due file diversi: quale dei due disegnasse un titolo lo
    // decideva il motore, a caso fra due pesi uguali.
    let mut viste: Vec<(String, &str)> = Vec::with_capacity(elenco.len());
    for faccia in &elenco {
        let ridotta = famiglia_ridotta(&faccia.family);
        if let Some((_, prima)) = viste.iter().find(|(r, _)| *r == ridotta) {
            return Err(problema(
                &giu(path, &faccia.family),
                format!(
                    "«{}» e «{prima}» nel foglio diventano lo stesso nome, «{ridotta}»: due \
                     caratteri diversi si prenderebbero a vicenda il file. Cambia uno dei due nomi.",
                    faccia.family
                ),
            ));
        }
        viste.push((ridotta, faccia.family.as_str()));
    }
    elenco.sort_by(|a, b| a.family.cmp(&b.family));
    Ok(elenco)
}

/// Il nome di una famiglia com'è nel foglio, prima del prefisso della skin:
/// minuscole e cifre, e un trattino al posto di tutto il resto.
///
/// Una funzione sola per il compilatore, che la usa per scrivere il nome, e
/// per la validazione, che la usa per rifiutare due famiglie che finirebbero
/// sotto lo stesso: se fossero due, un giorno direbbero due cose diverse.
pub(crate) fn famiglia_ridotta(nome: &str) -> String {
    nome.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "chi chiama ha già controllato che il peso stia fra 1 e 1000, e `round` toglie i \
              decimali: un u16 lo contiene tutto"
)]
fn peso_intero(peso: f64) -> u16 {
    peso.round() as u16
}

fn tavolozza(value: &Value, path: &str) -> Esito<Vec<(String, Rgba)>> {
    let map = oggetto(value, path)?;
    let mut colori = Vec::with_capacity(map.len());
    for (nome, grezzo) in map {
        let percorso = giu(path, nome);
        if !is_local_name(nome) {
            return Err(problema(
                &percorso,
                "il nome di un colore della tavolozza ammette minuscole, cifre e trattini",
            ));
        }
        // Solo colori letterali: una tavolozza che può riferire sé stessa può
        // anche fare un ciclo.
        colori.push((nome.clone(), colore_letterale(grezzo, &percorso)?));
    }
    colori.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(colori)
}

fn is_easing_name(nome: &str) -> bool {
    let mut caratteri = nome.bytes();
    !nome.is_empty()
        && nome.len() <= 40
        && caratteri.next().is_some_and(|b| b.is_ascii_lowercase())
        && caratteri.all(|b| b.is_ascii_alphanumeric())
}

fn fotogramma_rotta(value: &Value, path: &str) -> Esito<RouteFrame> {
    let map = oggetto(value, path)?;
    solo_chiavi(map, path, CHIAVI_FOTOGRAMMA_ROTTA)?;
    Ok(RouteFrame {
        opacity: campo(map, "opacity")
            .map(|v| numero_fra(v, &giu(path, "opacity"), 0.0, 1.0))
            .transpose()?,
        scale: campo(map, "scale")
            .map(|v| numero_fra(v, &giu(path, "scale"), 0.5, 1.5))
            .transpose()?,
        translate_y: campo(map, "translateY")
            .map(|v| numero_fra(v, &giu(path, "translateY"), -100.0, 100.0))
            .transpose()?,
    })
}

/// Le iterazioni: da 1 a [`MAX_ITERAZIONI`], o `"infinite"`.
///
/// Dove l'infinito si possa usare lo decide l'innesco, non l'animazione: la
/// stessa animazione può girare per sempre su `playing` e non su `enter`, e il
/// controllo sta in [`movimento_parte`].
fn iterazioni(value: &Value, path: &str) -> Esito<Iterazioni> {
    if value.as_str() == Some("infinite") {
        return Ok(Iterazioni::Infinite);
    }
    let quante = value
        .as_u64()
        .filter(|n| *n >= 1 && *n <= u64::from(MAX_ITERAZIONI))
        .ok_or_else(|| {
            problema(
                path,
                format!("va da 1 a {MAX_ITERAZIONI}, o \"infinite\" per un ciclo"),
            )
        })?;
    u32::try_from(quante)
        .map(Iterazioni::Volte)
        .map_err(|_| problema(path, "iterazioni fuori scala"))
}

/// Una durata con un tetto suo.
fn durata_fino_a(value: &Value, path: &str, massimo: f64) -> Esito<Duration> {
    let letta = durata(value, path)?;
    if letta.ms > massimo {
        return Err(problema(
            path,
            format!("al massimo {massimo}ms, non {}ms", letta.ms),
        ));
    }
    Ok(letta)
}

/// Il punto attorno a cui ruota e scala: una parola, o due lunghezze.
fn origine(value: &Value, path: &str) -> Esito<(Length, Length)> {
    let Some(parola) = value.as_str() else {
        return coppia(value, path);
    };
    let pc = |v: f64| Length {
        value: v,
        unit: LengthUnit::Percent,
    };
    let (x, y) = match parola {
        "center" => (50.0, 50.0),
        "top" => (50.0, 0.0),
        "bottom" => (50.0, 100.0),
        "left" => (0.0, 50.0),
        "right" => (100.0, 50.0),
        "top left" => (0.0, 0.0),
        "top right" => (100.0, 0.0),
        "bottom left" => (0.0, 100.0),
        "bottom right" => (100.0, 100.0),
        _ => {
            return Err(problema(
                path,
                "ammessi center, top, bottom, left, right, i quattro angoli come «top left», o due lunghezze",
            ));
        }
    };
    Ok((pc(x), pc(y)))
}

fn ritaglio(value: &Value, path: &str) -> Esito<Ritaglio> {
    let map = oggetto(value, path)?;
    let mut chiavi = map.keys();
    let (Some(forma), None) = (chiavi.next(), chiavi.next()) else {
        return Err(problema(
            path,
            "un ritaglio è un oggetto con una chiave sola: inset, circle o polygon",
        ));
    };
    let valore = map.get(forma).unwrap_or(&Value::Null);
    let qui = giu(path, forma);
    match forma.as_str() {
        "inset" => {
            let lati = lista_fra(valore, &qui, 4, 4)?;
            let lato = |i: usize| -> Esito<Length> {
                lunghezza(
                    lati.get(i).unwrap_or(&Value::Null),
                    &giu(&qui, &i.to_string()),
                )
            };
            Ok(Ritaglio::Inset {
                top: lato(0)?,
                right: lato(1)?,
                bottom: lato(2)?,
                left: lato(3)?,
            })
        }
        "circle" => {
            let cerchio = oggetto(valore, &qui)?;
            solo_chiavi(cerchio, &qui, &["radius", "at"])?;
            Ok(Ritaglio::Circle {
                radius: lunghezza(richiesto(cerchio, &qui, "radius")?, &giu(&qui, "radius"))?,
                at: match campo(cerchio, "at") {
                    None => (
                        Length {
                            value: 50.0,
                            unit: LengthUnit::Percent,
                        },
                        Length {
                            value: 50.0,
                            unit: LengthUnit::Percent,
                        },
                    ),
                    Some(centro) => coppia(centro, &giu(&qui, "at"))?,
                },
            })
        }
        "polygon" => {
            let punti = lista_fra(valore, &qui, 3, 8)?;
            let mut letti = Vec::with_capacity(punti.len());
            for (indice, punto) in punti.iter().enumerate() {
                letti.push(coppia(punto, &giu(&qui, &indice.to_string()))?);
            }
            Ok(Ritaglio::Polygon(letti))
        }
        altro => Err(problema(
            &qui,
            format!("«{altro}» non è una forma di ritaglio: ammessi inset, circle, polygon"),
        )),
    }
}

/// I fotogrammi di un'animazione: da due a dodici, in ordine, il primo a 0.
///
/// L'ordine lo deve scrivere **chi dichiara**, e la lettura non lo raddrizza:
/// rifiutare costa un messaggio; riordinare costa la fiducia nell'anteprima.
fn fotogrammi_animazione(value: &Value, path: &str) -> Esito<Vec<AnimFrame>> {
    let voci = lista_fra(value, path, MIN_FOTOGRAMMI, MAX_FOTOGRAMMI)?;
    let mut fermate: Vec<AnimFrame> = Vec::with_capacity(voci.len());
    for (indice, grezzo) in voci.iter().enumerate() {
        let dentro = giu(path, &indice.to_string());
        let map = oggetto(grezzo, &dentro)?;
        solo_chiavi(map, &dentro, CHIAVI_FOTOGRAMMA)?;
        let fra = |nome: &str, min: f64, max: f64| -> Esito<Option<f64>> {
            campo(map, nome)
                .map(|v| numero_fra(v, &giu(&dentro, nome), min, max))
                .transpose()
        };
        let fermata = AnimFrame {
            at: numero_fra(
                richiesto(map, &dentro, "at")?,
                &giu(&dentro, "at"),
                0.0,
                100.0,
            )?,
            opacity: fra("opacity", 0.0, 1.0)?,
            translate_x: campo(map, "translateX")
                .map(|v| spostamento(v, &giu(&dentro, "translateX")))
                .transpose()?,
            translate_y: campo(map, "translateY")
                .map(|v| spostamento(v, &giu(&dentro, "translateY")))
                .transpose()?,
            scale: fra("scale", 0.0, 3.0)?,
            scale_x: fra("scaleX", 0.0, 3.0)?,
            scale_y: fra("scaleY", 0.0, 3.0)?,
            rotate: fra("rotate", -720.0, 720.0)?,
            skew_x: fra("skewX", -45.0, 45.0)?,
            skew_y: fra("skewY", -45.0, 45.0)?,
            blur: campo(map, "blur")
                .map(|v| misura_fra(v, &giu(&dentro, "blur"), 0.0, 40.0))
                .transpose()?,
            brightness: fra("brightness", 0.0, 3.0)?,
            saturate: fra("saturate", 0.0, 3.0)?,
            hue_rotate: fra("hueRotate", -360.0, 360.0)?,
            background_color: campo(map, "backgroundColor")
                .map(|v| colore(v, &giu(&dentro, "backgroundColor")))
                .transpose()?,
            color: campo(map, "color")
                .map(|v| colore(v, &giu(&dentro, "color")))
                .transpose()?,
            border_color: campo(map, "borderColor")
                .map(|v| colore(v, &giu(&dentro, "borderColor")))
                .transpose()?,
            shadow: campo(map, "shadow")
                .map(|v| ombra_parte(v, &giu(&dentro, "shadow")))
                .transpose()?,
            clip: campo(map, "clip")
                .map(|v| ritaglio(v, &giu(&dentro, "clip")))
                .transpose()?,
        };
        if fermata.is_empty() {
            return Err(problema(
                &dentro,
                "un fotogramma che non dichiara niente non è una fermata: \
                 togli la voce, o scrivi che cosa cambia qui",
            ));
        }
        if indice == 0 && fermata.at != 0.0 {
            return Err(problema(
                &giu(&dentro, "at"),
                "il primo fotogramma sta a 0: senza, il motore lo ricava dallo stato in cui \
                 la parte si trova, e l'animazione parte da un punto diverso ogni volta",
            ));
        }
        if let Some(precedente) = fermate.last().filter(|p| p.at >= fermata.at) {
            return Err(problema(
                &giu(&dentro, "at"),
                format!(
                    "i fotogrammi vanno in ordine crescente: {} non viene dopo {}",
                    fermata.at, precedente.at
                ),
            ));
        }
        fermate.push(fermata);
    }
    Ok(fermate)
}

fn animazione(value: &Value, path: &str, curve: &[(String, Easing)]) -> Esito<Animation> {
    let map = oggetto(value, path)?;
    solo_chiavi(map, path, CHIAVI_ANIMAZIONE)?;
    Ok(Animation {
        duration: durata_fino_a(
            richiesto(map, path, "duration")?,
            &giu(path, "duration"),
            MAX_DURATA_MS,
        )?,
        easing: campo(map, "easing")
            .map(|v| curva(v, &giu(path, "easing"), curve))
            .transpose()?
            .unwrap_or_default(),
        delay: campo(map, "delay")
            .map(|v| durata_fino_a(v, &giu(path, "delay"), MAX_RITARDO_MS))
            .transpose()?,
        iterations: campo(map, "iterations")
            .map(|v| iterazioni(v, &giu(path, "iterations")))
            .transpose()?
            .unwrap_or(Iterazioni::Volte(1)),
        direction: campo(map, "direction")
            .map(|v| {
                let nomi: Vec<(&str, AnimDirection)> = AnimDirection::ALL
                    .iter()
                    .map(|d| (d.as_str(), *d))
                    .collect();
                parola(v, &giu(path, "direction"), &nomi)
            })
            .transpose()?
            .unwrap_or_default(),
        fill: campo(map, "fill")
            .map(|v| {
                let nomi: Vec<(&str, Riempimento)> =
                    Riempimento::ALL.iter().map(|f| (f.as_str(), *f)).collect();
                parola(v, &giu(path, "fill"), &nomi)
            })
            .transpose()?
            .unwrap_or_default(),
        origin: campo(map, "origin")
            .map(|v| origine(v, &giu(path, "origin")))
            .transpose()?,
        frames: fotogrammi_animazione(richiesto(map, path, "frames")?, &giu(path, "frames"))?,
    })
}

fn animazioni(
    value: &Value,
    path: &str,
    curve: &[(String, Easing)],
) -> Esito<Vec<(String, Animation)>> {
    let map = oggetto(value, path)?;
    if map.len() > MAX_ANIMAZIONI {
        return Err(problema(
            path,
            format!(
                "al massimo {MAX_ANIMAZIONI} animazioni dichiarate, non {}",
                map.len()
            ),
        ));
    }
    let mut elenco = Vec::with_capacity(map.len());
    for (nome, grezzo) in map {
        let percorso = giu(path, nome);
        if !is_local_name(nome) {
            return Err(problema(
                &percorso,
                "il nome di un'animazione ammette minuscole, cifre e trattini",
            ));
        }
        elenco.push((nome.clone(), animazione(grezzo, &percorso, curve)?));
    }
    elenco.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(elenco)
}

// ── Il lampeggio ────────────────────────────────────────────────────────────
//
// Una skin si scambia, e chi la indossa non l'ha scritta. Un'animazione che
// accende e spegne una luce dieci volte al secondo, per sempre, non è uno stile
// brutto: in chi soffre di epilessia fotosensibile può scatenare una crisi, ed
// è il motivo per cui WCAG 2.3.1 ne fa un criterio di livello A. Le regole
// d'arresto del compilatore non bastano, perché scattano solo per chi ha già
// chiesto meno movimento — e chi non sa di essere sensibile non l'ha chiesto.
//
// Quindi è un **errore** e non un avviso: una skin che lampeggia non si
// installa, non si prova sull'app, e lo Studio lo dice sulla durata
// dell'animazione.
//
// # Cosa si misura
//
// Il tempo di un lampo: due cambi di luce opposti uno dopo l'altro — sale e
// scende, si accende e si spegne — dall'inizio del primo alla fine del secondo.
// La metà di quel tempo è il **mezzo ciclo**, e sotto [`MEZZO_CICLO_MINIMO_MS`]
// è un lampeggio. Per ogni proprietà che si vede come luce, separatamente,
// perché il motore anima ogni proprietà sulla sua traccia:
//
// - un fotogramma che non dichiara la proprietà non conta; se non la dichiara
//   il primo o l'ultimo, lì vale quel che la parte ha di suo, che non si
//   conosce e quindi è diverso da tutto;
// - due passi di fila nello stesso verso — 0 → 0,5 → 1 — sono un cambio solo:
//   una salita scritta in più fotogrammi non è un tremolio;
// - con `normal` e `reverse`, alla fine di ogni giro la luce **salta** al
//   primo fotogramma: è un cambio istantaneo, e conta;
// - con `alternate` il giro dura due volte, e al ritorno ogni cambio si rifà
//   al contrario;
// - la durata è quella che il foglio userà: con l'intensità `essential` il
//   compilatore la dimezza, e il conto si fa sulla metà.
//
// La soglia è più severa di quella di WCAG — tre lampi al secondo vorrebbero
// dire un mezzo ciclo di 166 ms — e di proposito: il criterio misura anche
// l'area e la luminanza, e qui non si sa quanto grande sarà la parte né su
// cosa starà. Un colore che pulsa in due secondi resta lontanissimo dalla
// soglia, un respiro di luminosità anche; un fondo che scatta cento volte al
// minuto no.

/// Il mezzo ciclo più corto che una luce ripetuta può avere, in millisecondi.
///
/// Un terzo di secondo: un lampo intero dura almeno due terzi, cioè non più di
/// un lampo e mezzo al secondo. Vedi «Il lampeggio» in questo file.
pub const MEZZO_CICLO_MINIMO_MS: f64 = 333.0;

/// Quante ripetizioni una luce può fare prima di contare come lampeggio.
///
/// Tre come i tre lampi che WCAG 2.3.1 ammette: un'animazione che si ripete
/// al più tre volte e si ferma non può farne di più, a qualunque velocità.
pub const RIPETIZIONI_SENZA_LAMPEGGIO: u32 = 3;

/// Un valore che si vede come luce, su un fotogramma.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Luce<'a> {
    /// Quel che la parte ha di suo, dove la traccia non dichiara la proprietà.
    DellaParte,
    /// Un numero: opacità, sfocatura, luminosità, saturazione, tinta.
    Numero(f64),
    /// Un colore.
    Colore(&'a ColorValue),
    /// Un'ombra.
    Ombra(&'a Ombra),
}

impl Luce<'_> {
    /// In che verso va il cambio da qui a lì, quando lo si può dire.
    ///
    /// Solo fra due numeri: un colore che diventa un altro colore non sale e
    /// non scende, e due cambi di colore di fila si contano per due.
    fn verso(self, poi: Self) -> Option<std::cmp::Ordering> {
        match (self, poi) {
            (Self::Numero(da), Self::Numero(a)) => da.partial_cmp(&a),
            _ => None,
        }
    }
}

/// Una proprietà che si vede come luce: come si chiama in un messaggio, e come
/// si legge da un fotogramma.
type Tracciato = (&'static str, for<'a> fn(&'a AnimFrame) -> Option<Luce<'a>>);

/// Le proprietà che, cambiando in fretta, lampeggiano. Le trasformazioni e il
/// ritaglio restano fuori: spostano la parte, non ne cambiano la luce.
const LUCI: &[Tracciato] = &[
    ("l'opacità", |f| f.opacity.map(Luce::Numero)),
    ("la sfocatura", |f| f.blur.map(|l| Luce::Numero(l.value))),
    ("la luminosità", |f| f.brightness.map(Luce::Numero)),
    ("la saturazione", |f| f.saturate.map(Luce::Numero)),
    ("la tinta", |f| f.hue_rotate.map(Luce::Numero)),
    ("il colore di fondo", |f| {
        f.background_color.as_ref().map(Luce::Colore)
    }),
    ("il colore del testo", |f| {
        f.color.as_ref().map(Luce::Colore)
    }),
    ("il colore del bordo", |f| {
        f.border_color.as_ref().map(Luce::Colore)
    }),
    ("l'ombra", |f| f.shadow.as_ref().map(Luce::Ombra)),
];

/// Un cambio di luce dentro un giro, in millisecondi dall'inizio del giro.
#[derive(Debug, Clone, Copy)]
struct Cambio {
    inizio: f64,
    fine: f64,
    verso: Option<std::cmp::Ordering>,
}

/// Il mezzo ciclo più corto di una proprietà, o `None` se non cambia mai.
///
/// `durata` è quella di un giro, già scalata; `alterna` vale per `alternate` e
/// `alternate-reverse`. `reverse` è `normal` guardato allo specchio: gli
/// intervalli fra un cambio e l'altro sono gli stessi.
fn mezzo_ciclo(
    fotogrammi: &[AnimFrame],
    durata: f64,
    alterna: bool,
    leggi: for<'a> fn(&'a AnimFrame) -> Option<Luce<'a>>,
) -> Option<f64> {
    let mut punti: Vec<(f64, Luce<'_>)> = fotogrammi
        .iter()
        .filter_map(|f| leggi(f).map(|luce| (f.at, luce)))
        .collect();
    let (&(primo, _), &(ultimo, _)) = (punti.first()?, punti.last()?);
    if primo > 0.0 {
        punti.insert(0, (0.0, Luce::DellaParte));
    }
    if ultimo < 100.0 {
        punti.push((100.0, Luce::DellaParte));
    }
    let tempo = |at: f64| durata * at / 100.0;

    let mut giro: Vec<Cambio> = punti
        .windows(2)
        .filter_map(|coppia| match coppia {
            [(da, prima), (a, dopo)] if prima != dopo => Some(Cambio {
                inizio: tempo(*da),
                fine: tempo(*a),
                verso: prima.verso(*dopo),
            }),
            _ => None,
        })
        .collect();
    let periodo = if alterna {
        // Il ritorno: gli stessi cambi, al contrario e nel verso opposto.
        let periodo = durata * 2.0;
        let ritorno: Vec<Cambio> = giro
            .iter()
            .rev()
            .map(|c| Cambio {
                inizio: periodo - c.fine,
                fine: periodo - c.inizio,
                verso: c.verso.map(std::cmp::Ordering::reverse),
            })
            .collect();
        giro.extend(ritorno);
        periodo
    } else {
        // Il salto dall'ultimo fotogramma al primo, fra un giro e l'altro.
        if let (Some(&(_, fine)), Some(&(_, inizio))) = (punti.last(), punti.first())
            && fine != inizio
        {
            giro.push(Cambio {
                inizio: durata,
                fine: durata,
                verso: fine.verso(inizio),
            });
        }
        durata
    };

    // Due cambi di fila nello stesso verso sono uno solo, anche a cavallo fra
    // la fine di un giro e l'inizio del successivo.
    let mut cambi: Vec<Cambio> = Vec::with_capacity(giro.len());
    for cambio in giro {
        match cambi.last_mut() {
            Some(prima) if prima.verso.is_some() && prima.verso == cambio.verso => {
                prima.fine = cambio.fine;
            }
            _ => cambi.push(cambio),
        }
    }
    if cambi.len() > 1
        && let (Some(&primo), Some(&ultimo)) = (cambi.first(), cambi.last())
        && primo.verso.is_some()
        && primo.verso == ultimo.verso
    {
        cambi.pop();
        if let Some(testa) = cambi.first_mut() {
            testa.inizio = ultimo.inizio - periodo;
        }
    }

    // Ogni cambio col successivo — l'ultimo col primo del giro dopo — è un
    // lampo. Il più breve decide.
    let quanti = cambi.len();
    (0..quanti)
        .filter_map(|indice| {
            let questo = cambi.get(indice)?;
            let (poi, oltre) = match cambi.get(indice + 1) {
                Some(poi) => (poi, 0.0),
                None => (cambi.first()?, periodo),
            };
            Some((poi.fine + oltre - questo.inizio) / 2.0)
        })
        .reduce(f64::min)
}

/// Le animazioni che lampeggiano, come errori sulla loro durata.
fn lampeggi(motion: &SkinMotion) -> Vec<SkinIssue> {
    // La durata che il foglio userà: `essential` la dimezza. `maximum` la
    // allunga, e non si conta — lo Studio fa scorrere le animazioni alla loro
    // durata scritta, e il conto deve valere anche lì. `none` le ferma tutte,
    // e nemmeno questo si conta: una scala a zero non ferma un ciclo, lo rende
    // infinitamente veloce, e a fermarlo è una regola del foglio che non deve
    // essere l'unica cosa fra una skin e un lampeggio.
    let scala = match motion.intensity.scale() {
        s if s > 0.0 => s.min(1.0),
        _ => 1.0,
    };
    let mut errori = Vec::new();
    for (nome, animazione) in &motion.animations {
        let ripete = match animazione.iterations {
            Iterazioni::Infinite => "si ripete all'infinito".to_owned(),
            Iterazioni::Volte(n) if n > RIPETIZIONI_SENZA_LAMPEGGIO => {
                format!("si ripete {n} volte")
            }
            Iterazioni::Volte(_) => continue,
        };
        let alterna = matches!(
            animazione.direction,
            AnimDirection::Alternate | AnimDirection::AlternateReverse
        );
        let durata = animazione.duration.ms * scala;
        let peggiore = LUCI
            .iter()
            .filter_map(|(cosa, leggi)| {
                mezzo_ciclo(&animazione.frames, durata, alterna, *leggi).map(|m| (*cosa, m))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((cosa, mezzo)) = peggiore.filter(|(_, m)| *m < MEZZO_CICLO_MINIMO_MS) else {
            continue;
        };
        let dimezzata = if scala < 1.0 {
            format!(
                " Con l'intensità «{}» il foglio moltiplica ogni durata per {scala}, e il conto \
                 si fa sulla durata che il foglio userà.",
                motion.intensity.as_str()
            )
        } else {
            String::new()
        };
        errori.push(problema(
            &giu(&giu(&giu("motion", "animations"), nome), "duration"),
            format!(
                "«{nome}» lampeggia: {cosa} cambia ogni {mezzo:.0} ms e {ripete}, sotto i \
                 {MEZZO_CICLO_MINIMO_MS:.0} ms per mezzo ciclo. Una luce che cambia così in fretta \
                 e si ripete può scatenare una crisi in chi soffre di epilessia fotosensibile \
                 (WCAG 2.3.1). Allunga la durata, togli i fotogrammi che vanno e vengono, anima \
                 una trasformazione invece della luce, o fermala a {RIPETIZIONI_SENZA_LAMPEGGIO} \
                 ripetizioni.{dimezzata}"
            ),
        ));
    }
    errori
}

fn movimento(value: &Value, path: &str) -> Esito<SkinMotion> {
    let map = oggetto(value, path)?;
    if map.contains_key("routeTransition") {
        return Err(problema(
            &giu(path, "routeTransition"),
            "nel formato 2 si chiama `route`, e oltre a `out` e `in` accetta `duration` ed `easing`",
        ));
    }
    solo_chiavi(map, path, CHIAVI_MOVIMENTO)?;

    let intensity = match campo(map, "intensity") {
        None => MotionIntensity::Full,
        Some(grezzo) => {
            let nomi: Vec<(&str, MotionIntensity)> = MotionIntensity::ALL
                .iter()
                .map(|i| (i.as_str(), *i))
                .collect();
            parola(grezzo, &giu(path, "intensity"), &nomi)?
        }
    };

    let mut easings = Vec::new();
    if let Some(grezzo) = campo(map, "easings") {
        let dentro = giu(path, "easings");
        for (nome, valore) in oggetto(grezzo, &dentro)? {
            let percorso = giu(&dentro, nome);
            if !is_easing_name(nome) {
                return Err(problema(
                    &percorso,
                    "il nome di una curva ammette minuscole, cifre e lettere, e comincia con una minuscola",
                ));
            }
            if EasingKeyword::parse(nome).is_some() {
                return Err(problema(
                    &percorso,
                    format!(
                        "«{nome}» è una parola chiave del CSS: una curva della skin non può chiamarsi così"
                    ),
                ));
            }
            easings.push((nome.clone(), easing(valore, &percorso)?));
        }
        easings.sort_by(|a, b| a.0.cmp(&b.0));
    }

    let route = match campo(map, "route") {
        None => None,
        Some(grezzo) => {
            let dentro = giu(path, "route");
            let blocco = oggetto(grezzo, &dentro)?;
            solo_chiavi(blocco, &dentro, CHIAVI_ROTTA)?;
            Some(Rotta {
                duration: campo(blocco, "duration")
                    .map(|v| durata_fino_a(v, &giu(&dentro, "duration"), 2_000.0))
                    .transpose()?,
                easing: campo(blocco, "easing")
                    .map(|v| curva(v, &giu(&dentro, "easing"), &easings))
                    .transpose()?,
                out: campo(blocco, "out")
                    .map(|v| fotogramma_rotta(v, &giu(&dentro, "out")))
                    .transpose()?,
                enter: campo(blocco, "in")
                    .map(|v| fotogramma_rotta(v, &giu(&dentro, "in")))
                    .transpose()?,
            })
        }
    };

    let animations = match campo(map, "animations") {
        None => Vec::new(),
        Some(grezzo) => animazioni(grezzo, &giu(path, "animations"), &easings)?,
    };

    Ok(SkinMotion {
        intensity,
        easings,
        route,
        animations,
    })
}

fn impaginazione(value: &Value, path: &str, problemi: &mut Vec<SkinIssue>) -> Esito<SkinLayout> {
    let map = oggetto(value, path)?;
    for chiave in ["player", "sidebar"] {
        if map.contains_key(chiave) {
            return Err(problema(
                &giu(path, chiave),
                "nel formato 2 non c'è più: dove stanno il lettore e la barra lo dice lo \
                 scafale, `layout.shell`, scritto per esteso",
            ));
        }
    }
    solo_chiavi(map, path, CHIAVI_IMPAGINAZIONE)?;
    let density = campo(map, "density")
        .map(|v| {
            parola(
                v,
                &giu(path, "density"),
                &[
                    ("compact", Density::Compact),
                    ("comfortable", Density::Comfortable),
                    ("spacious", Density::Spacious),
                ],
            )
        })
        .transpose()?
        .unwrap_or(Density::Comfortable);

    // I prefab si leggono **prima** dello scafale, come i motivi prima delle
    // parti: un riferimento si risolve solo se quel che riferisce esiste già.
    let prefabs = match campo(map, "prefabs") {
        None => Vec::new(),
        Some(grezzo) => modelli(grezzo, &giu(path, "prefabs"), problemi)?,
    };

    let Some(grezzo) = campo(map, "shell") else {
        return Ok(SkinLayout {
            density,
            shell: shell_di_serie(),
            prefabs: prefabs.iter().map(|(n, _)| n.clone()).collect(),
        });
    };

    let dentro = giu(path, "shell");
    let prima = problemi.len();
    // La radice è già un nodo: il budget parte da uno in meno.
    let mut restanti = MAX_SHELL_NODES.saturating_sub(1);
    let albero = zona_scafale(grezzo, &dentro, &prefabs, &mut restanti, problemi)?;

    // Le regole si applicano soltanto se l'albero è arrivato intero: un ramo
    // che non si è letto lascia un buco, e un buco produce errori veri
    // dell'albero mutilato e falsi di quello scritto.
    if problemi.len() == prima {
        for guasto in crate::layout::verifica(&albero) {
            problemi.push(problema_scafale(&dentro, &guasto));
        }
    }

    Ok(SkinLayout {
        density,
        shell: albero,
        prefabs: prefabs.into_iter().map(|(n, _)| n).collect(),
    })
}

/// I sottoalberi nominati che lo scafale può richiamare.
///
/// Un prefab è **un sottoalbero di widget già registrati**, e nient'altro:
/// niente parametri, niente condizioni, niente prefab dentro prefab.
fn modelli(
    value: &Value,
    path: &str,
    problemi: &mut Vec<SkinIssue>,
) -> Esito<Vec<(String, LayoutZone)>> {
    let map = oggetto(value, path)?;
    let mut elenco = Vec::with_capacity(map.len());
    for (nome, grezzo) in map {
        let percorso = giu(path, nome);
        if !is_local_name(nome) {
            return Err(problema(
                &percorso,
                "il nome di un prefab ammette minuscole, cifre e trattini",
            ));
        }
        let mut restanti = MAX_SHELL_NODES.saturating_sub(1);
        let mut corpo = zona_scafale(grezzo, &percorso, &[], &mut restanti, problemi)?;
        corpo.from_prefab = Some(nome.clone());
        elenco.push((nome.clone(), corpo));
    }
    elenco.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(elenco)
}

// ── Lo scafale ──────────────────────────────────────────────────────────────

/// Un problema dello scafale, col prefisso e il codice del catalogo.
///
/// Il percorso è **quello del JSON** e mai un indirizzo `data-nodo`: è ciò che
/// lo Studio consuma per portare il cursore sull'errore.
fn problema_scafale(prefisso: &str, guasto: &crate::layout::ShellIssue) -> SkinIssue {
    // Un problema dentro un prefab si riporta al sito di **dichiarazione**.
    let base = match guasto.prefab.as_ref() {
        None => prefisso.to_owned(),
        Some(nome) => format!(
            "{}.prefabs.{nome}",
            prefisso.strip_suffix(".shell").unwrap_or(prefisso)
        ),
    };
    let percorso = if guasto.path.is_empty() {
        base
    } else {
        format!("{base}.{}", guasto.path)
    };
    let Some(widget) = guasto.missing.as_ref() else {
        return problema(&percorso, guasto.message.clone());
    };
    SkinIssue {
        code: ErrorCode::SkinLayoutIncomplete {
            widget: widget.clone(),
        },
        path: percorso,
        message: guasto.message.clone(),
        punto: None,
    }
}

fn misura_traccia(value: &Value, path: &str) -> Esito<TrackSize> {
    let scritto = testo(value, path)?;
    match scritto {
        "hug" => Ok(TrackSize::Hug),
        "fill" => Ok(TrackSize::Fill),
        _ => parse_length(scritto).map(TrackSize::Fixed).ok_or_else(|| {
            problema(
                path,
                format!("«{scritto}» non è una misura: «hug», «fill», o una lunghezza in px o rem"),
            )
        }),
    }
}

fn opzioni_widget(
    def: &'static WidgetDef,
    value: &Value,
    path: &str,
) -> Esito<Vec<(&'static WidgetOption, OptionValue)>> {
    let map = oggetto(value, path)?;
    // Si parte dai difetti e si sovrascrive: chi legge la struttura a valle
    // trova sempre tutte le manopole, dichiarate o no.
    let mut valori = WidgetInstance::nuovo(def).options;

    for (nome, grezzo) in map {
        let percorso = giu(path, nome);
        let Some(opzione) = def.option(nome) else {
            let candidati = def.nearest_options(nome);
            return Err(problema(
                &percorso,
                format!(
                    "«{}» non ha una manopola «{nome}».{}",
                    def.name,
                    forse(&candidati)
                ),
            ));
        };
        let valore = match opzione.kind {
            OptionKind::Flag { .. } => OptionValue::Flag(booleano(grezzo, &percorso)?),
            OptionKind::Word { allowed, .. } => {
                let ammesse: Vec<(&str, &'static str)> =
                    allowed.iter().map(|parola| (*parola, *parola)).collect();
                OptionValue::Word(parola(grezzo, &percorso, &ammesse)?)
            }
            OptionKind::Count { min, max, .. } => {
                let quanti = grezzo
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| (min..=max).contains(n))
                    .ok_or_else(|| {
                        problema(&percorso, format!("qui ci va un intero da {min} a {max}"))
                    })?;
                OptionValue::Count(quanti)
            }
        };
        if let Some(voce) = valori.iter_mut().find(|(o, _)| o.name == opzione.name) {
            voce.1 = valore;
        }
    }

    Ok(valori)
}

fn foglia_scafale(map: &Map<String, Value>, path: &str) -> Esito<WidgetInstance> {
    solo_chiavi(map, path, CHIAVI_WIDGET)?;
    let percorso = giu(path, "widget");
    let nome = testo(richiesto(map, path, "widget")?, &percorso)?;
    let Some(def) = crate::layout::widget(nome) else {
        let candidati = crate::layout::nearest_widgets(nome);
        return Err(problema(
            &percorso,
            format!("widget inesistente: «{nome}».{}", forse(&candidati)),
        ));
    };

    Ok(WidgetInstance {
        def,
        size: campo(map, "size")
            .map(|v| misura_traccia(v, &giu(path, "size")))
            .transpose()?
            .unwrap_or(def.natural),
        options: match campo(map, "options") {
            None => WidgetInstance::nuovo(def).options,
            Some(grezzo) => opzioni_widget(def, grezzo, &giu(path, "options"))?,
        },
    })
}

fn troppi_nodi(path: &str) -> SkinIssue {
    problema(
        path,
        format!("lo scafale supera i {MAX_SHELL_NODES} nodi, che è il massimo"),
    )
}

/// Prende dei nodi dal budget, o dice che l'albero non ci sta.
///
/// Il tetto si spende invece di contarlo alla fine perché un prefab si espande
/// dove è richiamato: l'albero cresce come il **prodotto** fra la larghezza di
/// un corpo e il numero dei richiami, e contare alla fine vuol dire contare
/// qualcosa che è già stato allocato.
fn prendi(restanti: &mut usize, quanti: usize, path: &str) -> Esito<()> {
    match restanti.checked_sub(quanti) {
        Some(rimasti) => {
            *restanti = rimasti;
            Ok(())
        }
        None => {
            *restanti = 0;
            Err(troppi_nodi(path))
        }
    }
}

fn zona_scafale(
    value: &Value,
    path: &str,
    prefabs: &[(String, LayoutZone)],
    restanti: &mut usize,
    problemi: &mut Vec<SkinIssue>,
) -> Esito<LayoutZone> {
    let map = oggetto(value, path)?;
    solo_chiavi(map, path, CHIAVI_ZONA)?;

    let kind = {
        let ammesse: Vec<(&str, ZoneKind)> =
            ZoneKind::ALL.iter().map(|k| (k.as_str(), *k)).collect();
        parola(richiesto(map, path, "zone")?, &giu(path, "zone"), &ammesse)?
    };

    let parte = match campo(map, "part") {
        None => None,
        Some(grezzo) => {
            let percorso = giu(path, "part");
            let nome = testo(grezzo, &percorso)?;
            match crate::parts::part(nome) {
                Some(def) => Some(def),
                // La stessa decisione di `stile_parte`, sulla seconda porta: una
                // parte ritirata su un nodo si accetta, e il nodo resta senza.
                None if crate::parts::ritirata(nome).is_some() => None,
                None => {
                    let candidati = crate::parts::nearest_parts(nome);
                    return Err(problema(
                        &percorso,
                        format!("parte inesistente: «{nome}».{}", forse(&candidati)),
                    ));
                }
            }
        }
    };

    let dentro = giu(path, "children");
    let voci = lista(richiesto(map, path, "children")?, &dentro)?;
    let mut figli = Vec::with_capacity(voci.len());
    for (indice, grezzo) in voci.iter().enumerate() {
        let percorso = giu(&dentro, &indice.to_string());
        if *restanti == 0 {
            return Err(troppi_nodi(&dentro));
        }
        // Un figlio malformato smette di scendere in **quel** ramo; i fratelli
        // proseguono.
        match nodo_scafale(grezzo, &percorso, prefabs, restanti, problemi) {
            Ok(figlio) => figli.push(figlio),
            Err(guasto) => problemi.push(guasto),
        }
    }

    Ok(LayoutZone {
        kind,
        size: campo(map, "size")
            .map(|v| misura_traccia(v, &giu(path, "size")))
            .transpose()?
            .unwrap_or(TrackSize::Fill),
        gap: campo(map, "gap")
            .map(|v| {
                let ammesse: Vec<(&str, GapStep)> =
                    GapStep::ALL.iter().map(|g| (g.as_str(), *g)).collect();
                parola(v, &giu(path, "gap"), &ammesse)
            })
            .transpose()?
            .unwrap_or(GapStep::None),
        align: campo(map, "align")
            .map(|v| {
                let ammesse: Vec<(&str, Align)> =
                    Align::ALL.iter().map(|a| (a.as_str(), *a)).collect();
                parola(v, &giu(path, "align"), &ammesse)
            })
            .transpose()?
            .unwrap_or(Align::Stretch),
        spread: campo(map, "spread")
            .map(|v| {
                let ammesse: Vec<(&str, Spread)> =
                    Spread::ALL.iter().map(|s| (s.as_str(), *s)).collect();
                parola(v, &giu(path, "spread"), &ammesse)
            })
            .transpose()?
            .unwrap_or(Spread::Start),
        part: parte,
        from_prefab: None,
        children: figli,
    })
}

fn nodo_scafale(
    value: &Value,
    path: &str,
    prefabs: &[(String, LayoutZone)],
    restanti: &mut usize,
    problemi: &mut Vec<SkinIssue>,
) -> Esito<LayoutNode> {
    // Un nodo costa uno prima ancora che si sappia cos'è.
    prendi(restanti, 1, path)?;

    let map = oggetto(value, path)?;
    if campo(map, "widget").is_some() {
        return foglia_scafale(map, path).map(LayoutNode::Widget);
    }
    if campo(map, "zone").is_some() {
        return zona_scafale(value, path, prefabs, restanti, problemi).map(LayoutNode::Zone);
    }
    if campo(map, "prefab").is_some() {
        return modello_richiamato(map, path, prefabs, restanti).map(LayoutNode::Zone);
    }
    Err(problema(
        path,
        "un nodo dello scafale è una zona («zone»), un widget («widget») o un \
         prefab («prefab»), e questo non dichiara nessuno dei tre",
    ))
}

/// Un prefab richiamato: si espande qui, e da qui in giù non esiste più.
fn modello_richiamato(
    map: &Map<String, Value>,
    path: &str,
    prefabs: &[(String, LayoutZone)],
    restanti: &mut usize,
) -> Esito<LayoutZone> {
    solo_chiavi(map, path, CHIAVI_RICHIAMO)?;
    let percorso = giu(path, "prefab");
    let nome = testo(richiesto(map, path, "prefab")?, &percorso)?;

    let Some((_, corpo)) = prefabs.iter().find(|(n, _)| n == nome) else {
        if prefabs.is_empty() {
            return Err(problema(
                &percorso,
                "un prefab non può contenerne un altro: sarebbe un linguaggio, \
                 e i cicli non si riconoscerebbero senza visitare il grafo",
            ));
        }
        let candidati = vicini(nome, prefabs.iter().map(|(n, _)| n.as_str()));
        return Err(problema(
            &percorso,
            format!("prefab inesistente: «{nome}».{}", forse(&candidati)),
        ));
    };

    // Il corpo intero, meno il nodo che `nodo_scafale` ha già pagato.
    prendi(restanti, corpo.conta_nodi().saturating_sub(1), path)?;

    Ok(LayoutZone {
        // La misura è l'unica cosa che il sito d'uso può dire.
        size: campo(map, "size")
            .map(|v| misura_traccia(v, &giu(path, "size")))
            .transpose()?
            .unwrap_or(corpo.size),
        ..corpo.clone()
    })
}

fn motivi(value: &Value, path: &str) -> Esito<Vec<(String, Effect)>> {
    let map = oggetto(value, path)?;
    let mut elenco = Vec::with_capacity(map.len());
    for (nome, grezzo) in map {
        let percorso = giu(path, nome);
        if !is_local_name(nome) {
            return Err(problema(
                &percorso,
                "il nome di un motivo ammette minuscole, cifre e trattini",
            ));
        }
        elenco.push((nome.clone(), effetto(grezzo, &percorso)?));
    }
    elenco.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(elenco)
}

/// Il tema opposto.
///
/// La tavolozza e i motivi della variante **ridefiniscono** nomi della base e
/// non ne aggiungono: un riferimento scritto in una parte deve risolversi in
/// tutti e due i temi, e un nome che esistesse solo nella variante sarebbe un
/// riferimento a niente nell'altro.
/// La tavolozza e i motivi della base, a cui la variante deve rispondere.
type Richiami<'a> = (&'a [(String, Rgba)], &'a [(String, Effect)]);

fn variante(
    value: &Value,
    path: &str,
    base: Richiami<'_>,
    richiamabili: &Richiamabili<'_>,
    problemi: &mut Vec<SkinIssue>,
) -> Esito<Variante> {
    let (tavolozza_base, motivi_base) = base;
    let map = oggetto(value, path)?;
    solo_chiavi(map, path, CHIAVI_VARIANTE)?;

    let tokens = match campo(map, "tokens") {
        None => TokenSet::default(),
        Some(grezzo) => insieme_token(grezzo, &giu(path, "tokens"), problemi)?,
    };

    let palette = match campo(map, "palette") {
        None => Vec::new(),
        Some(grezzo) => {
            let letta = tavolozza(grezzo, &giu(path, "palette"))?;
            for (nome, _) in &letta {
                if !tavolozza_base.iter().any(|(n, _)| n == nome) {
                    return Err(problema(
                        &giu(&giu(path, "palette"), nome),
                        format!(
                            "«{nome}» non è nella tavolozza della skin: la variante ridefinisce \
                             i colori che ci sono, non ne aggiunge"
                        ),
                    ));
                }
            }
            letta
        }
    };

    let patterns = match campo(map, "patterns") {
        None => Vec::new(),
        Some(grezzo) => {
            let letti = motivi(grezzo, &giu(path, "patterns"))?;
            for (nome, effetto) in &letti {
                let dove = giu(&giu(path, "patterns"), nome);
                let Some((_, originale)) = motivi_base.iter().find(|(n, _)| n == nome) else {
                    return Err(problema(
                        &dove,
                        format!(
                            "«{nome}» non è fra i motivi della skin: la variante ridefinisce i \
                             motivi che ci sono, non ne aggiunge"
                        ),
                    ));
                };
                if originale.target() != effetto.target() {
                    return Err(problema(
                        &dove,
                        format!(
                            "«{nome}» nella skin è un {}, e nella variante deve restarlo",
                            nome_destinazione(originale.target())
                        ),
                    ));
                }
            }
            letti
        }
    };

    let parts = match campo(map, "parts") {
        None => Vec::new(),
        Some(grezzo) => superfici(grezzo, &giu(path, "parts"), richiamabili, true, problemi)?,
    };

    Ok(Variante {
        tokens,
        palette,
        patterns,
        parts,
    })
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
    // Studio la timeline la fa scorrere lo stesso. Vedi «Il lampeggio».
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
                "è del formato {found}, e questa versione di Aether legge fino al {SKIN_FORMAT_VERSION}"
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

// ── Gli avvisi ──────────────────────────────────────────────────────────────

/// Perché un avviso è stato emesso.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningKind {
    /// Un token obbligatorio non è dichiarato.
    MissingRequiredToken,
    /// Una promessa non è mantenuta: una variante dichiarata e vuota.
    UnkeptCapability,
    /// Un motivo dichiarato e mai riferito da nessuna parte.
    UnusedPattern,
    /// Un prefab dichiarato e mai richiamato.
    UnusedPrefab,
    /// Un'animazione dichiarata e mai richiamata da nessuna parte.
    UnusedAnimation,
    /// Una superficie che sfora il budget di costo, una parte che sfora quello
    /// del movimento, o la finestra intera che sfora il suo.
    CostBudget,
    /// Una coppia testo/superficie sotto la soglia di leggibilità.
    Contrast,
    /// Un ciclo il cui primo fotogramma non è la posa di riposo.
    ///
    /// Un ciclo su `playing` si ferma in pausa **sul fotogramma in cui è**, e
    /// prima di suonare la prima volta sta sul primo. Se il primo non è la
    /// parte com'è — un vinile che parte ruotato di trenta gradi — la parte si
    /// vede storta finché nessuno preme play.
    RestingLoop,
}

/// La soglia di WCAG 2.1 per il testo normale.
pub const CONTRASTO_MINIMO: f64 = 4.5;

/// La soglia di WCAG 2.1 (criterio 1.4.11) per quel che non è testo: qui,
/// l'anello del fuoco contro la superficie su cui si disegna.
///
/// Tre e non quattro e mezzo perché l'anello non si legge, si **vede**: è un
/// tratto di due pixel attorno a un comando, e la regola per le forme è più
/// larga di quella per le lettere. Sotto, chi naviga con la tastiera non trova
/// più dove sta il fuoco — ed è la sola cosa che l'anello esiste per dire.
pub const CONTRASTO_ANELLO: f64 = 3.0;

/// Una coppia misurata: quanto si legge questo colore su quello.
#[derive(Debug, Clone, PartialEq)]
pub struct ContrastPair {
    /// Il token davanti.
    pub foreground: &'static str,
    /// Il token dietro.
    pub background: &'static str,
    /// Il rapporto nel tema di base.
    pub base: f64,
    /// Il rapporto nella variante, quando la skin ne ha una.
    pub variante: Option<f64>,
}

impl ContrastPair {
    /// Passa in tutti i temi in cui è stata misurata.
    #[must_use]
    pub fn passa(&self) -> bool {
        self.base >= CONTRASTO_MINIMO && self.variante.is_none_or(|l| l >= CONTRASTO_MINIMO)
    }
}

/// I colori che finiscono **sul testo**, e vanno misurati.
const DAVANTI: &[&str] = &[
    "color.text.1",
    "color.text.2",
    "color.text.3",
    "color.accent",
    "color.danger",
    "color.success",
    "color.warning",
];

/// Le superfici su cui il testo sta davvero.
///
/// `surface.3` è deliberatamente fuori: è un riempimento di sopraelevazione,
/// non un letto di testo semantico.
pub(crate) const DIETRO: &[&str] = &["color.surface.0", "color.surface.1", "color.surface.2"];

/// Il colore vero di un colore, quando si può saperlo.
///
/// `None` quando dipende dalla copertina. I riferimenti fra token si seguono,
/// con un tetto: `a → b → a` è un ciclo che il formato non vieta.
fn risolvi(
    colore: &ColorValue,
    tokens: &TokenSet,
    base: Option<&TokenSet>,
    palette: &[(String, Rgba)],
    passi: u8,
) -> Option<Rgba> {
    if passi == 0 {
        return None;
    }
    match colore {
        ColorValue::Literal(rgba) => Some(*rgba),
        ColorValue::Palette { name, alpha } => palette
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, rgba)| alpha.map_or(*rgba, |a| rgba.with_alpha(a))),
        ColorValue::Source { .. } => None,
        ColorValue::Token(def) => {
            let valore = tokens
                .get(def.id)
                .or_else(|| base.and_then(|b| b.get(def.id)))?;
            match valore {
                TokenValue::Color(dentro) => risolvi(dentro, tokens, base, palette, passi - 1),
                _ => None,
            }
        }
    }
}

/// Il colore di un token per nome, nel tema dato.
pub(crate) fn colore_di(
    id: &str,
    tokens: &TokenSet,
    base: Option<&TokenSet>,
    palette: &[(String, Rgba)],
) -> Option<Rgba> {
    let valore = tokens.get(id).or_else(|| base.and_then(|b| b.get(id)))?;
    match valore {
        TokenValue::Color(colore) => risolvi(colore, tokens, base, palette, 8),
        _ => None,
    }
}

/// Un tema da misurare: il nome, i suoi token, quelli su cui ricade, la sua
/// tavolozza.
struct Tema<'a> {
    nome: &'static str,
    tokens: &'a TokenSet,
    base: Option<&'a TokenSet>,
    palette: Vec<(String, Rgba)>,
}

fn temi(skin: &SkinDocument) -> Vec<Tema<'_>> {
    let mut temi = vec![Tema {
        nome: skin.scheme.nome(),
        tokens: &skin.tokens,
        base: None,
        palette: skin.palette.clone(),
    }];
    if let Some(variante) = skin.variant.as_ref() {
        temi.push(Tema {
            nome: skin.scheme.opposto().nome(),
            tokens: &variante.tokens,
            base: Some(&skin.tokens),
            palette: skin.tavolozza_variante(),
        });
    }
    temi
}

/// Misura ogni coppia testo/superficie, nei temi che la skin dichiara.
///
/// Le coppie che non si possono misurare — un colore che segue la copertina,
/// un token non dichiarato — semplicemente non compaiono.
#[must_use]
pub fn contrast_pairs(skin: &SkinDocument) -> Vec<ContrastPair> {
    let temi = temi(skin);
    let misura = |davanti: &str, dietro: &str, tema: &Tema<'_>| -> Option<f64> {
        let f = colore_di(davanti, tema.tokens, tema.base, &tema.palette)?;
        let b = colore_di(dietro, tema.tokens, tema.base, &tema.palette)?;
        Some(crate::values::contrast_ratio(f, b))
    };
    let mut coppie = Vec::new();
    let mut aggiungi = |davanti: &'static str, dietro: &'static str| {
        let Some(tema_base) = temi.first() else {
            return;
        };
        let Some(base) = misura(davanti, dietro, tema_base) else {
            return;
        };
        coppie.push(ContrastPair {
            foreground: davanti,
            background: dietro,
            base,
            variante: temi.get(1).and_then(|t| misura(davanti, dietro, t)),
        });
    };
    for davanti in DAVANTI {
        for dietro in DIETRO {
            aggiungi(davanti, dietro);
        }
    }
    // E la regola inversa: il testo che sta **sopra** l'accento è
    // `--color-surface-0`, l'etichetta di un bottone primario.
    aggiungi("color.surface.0", "color.accent");
    coppie
}

/// Il colore di fondo che una pila di livelli dipinge, se se ne può dire uno.
///
/// Si guarda lo strato più basso, che in CSS è l'**ultimo** della lista: una
/// tinta vale sé stessa, un gradiente la media delle sue fermate. Tutto il
/// resto — un motivo, un'immagine, una tinta che segue la copertina — non si
/// misura. Uno strato trasparente si guarda sopra il fondo dell'app, perché è
/// quello che ci si vede attraverso.
fn fondo_dipinto(
    livelli: &[Paint],
    tokens: &TokenSet,
    base: Option<&TokenSet>,
    palette: &[(String, Rgba)],
) -> Option<Rgba> {
    let media = |stops: &[Stop]| -> Option<Rgba> {
        let risolti: Vec<Rgba> = stops
            .iter()
            .filter_map(|s| risolvi(&s.color, tokens, base, palette, 8))
            .collect();
        if risolti.len() != stops.len() || risolti.is_empty() {
            return None;
        }
        let quanti = risolti.len();
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "la media di valori fra 0 e 255 sta in un u8"
        )]
        let canale = |prendi: fn(&Rgba) -> u8| -> u8 {
            (risolti.iter().map(|c| f64::from(prendi(c))).sum::<f64>() / quanti as f64).round()
                as u8
        };
        Some(Rgba {
            r: canale(|c| c.r),
            g: canale(|c| c.g),
            b: canale(|c| c.b),
            a: risolti.iter().map(|c| c.a).sum::<f64>() / quanti as f64,
        })
    };
    let dipinto = match livelli.last()?.effect() {
        Effect::Solid { color } => risolvi(color, tokens, base, palette, 8),
        Effect::LinearGradient { stops, .. }
        | Effect::RadialGradient { stops, .. }
        | Effect::ConicGradient { stops, .. } => media(stops),
        _ => None,
    }?;
    if dipinto.a >= 1.0 {
        return Some(dipinto);
    }
    let sotto = colore_di("color.surface.0", tokens, base, palette)?;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "una media pesata di due canali fra 0 e 255 sta in un u8"
    )]
    let piatto = |sopra: u8, dietro: u8| -> u8 {
        f64::from(sopra)
            .mul_add(dipinto.a, f64::from(dietro) * (1.0 - dipinto.a))
            .round() as u8
    };
    Some(Rgba {
        r: piatto(dipinto.r, sotto.r),
        g: piatto(dipinto.g, sotto.g),
        b: piatto(dipinto.b, sotto.b),
        a: 1.0,
    })
}

/// Le superfici che una skin si ridipinge, misurate contro il testo che ci va
/// sopra, in ogni tema che dichiara.
///
/// Nel tema della variante una parte si guarda com'è **in quel tema**: se la
/// variante la ridipinge, conta la sua pittura; se no, quella della base — che
/// è esattamente il caso in cui un colore scritto per esteso resta uguale in
/// due temi che hanno testi opposti.
fn avvisi_superfici(skin: &SkinDocument) -> Vec<SkinWarning> {
    let mut avvisi = Vec::new();
    for (indice, tema) in temi(skin).iter().enumerate() {
        for base in &skin.parts {
            let sovrascritta = (indice > 0)
                .then(|| {
                    skin.variant
                        .as_ref()
                        .and_then(|v| v.parts.iter().find(|p| p.def.name == base.def.name))
                })
                .flatten();
            let fondo_scritto = sovrascritta
                .map(|p| &p.appearance.background)
                .filter(|b| !b.is_empty())
                .unwrap_or(&base.appearance.background);
            if fondo_scritto.is_empty() {
                continue;
            }
            let Some(fondo) = fondo_dipinto(fondo_scritto, tema.tokens, tema.base, &tema.palette)
            else {
                continue;
            };
            let testo_scritto = sovrascritta
                .and_then(|p| p.appearance.text_color.as_ref())
                .or(base.appearance.text_color.as_ref());
            let testo = testo_scritto
                .and_then(|c| risolvi(c, tema.tokens, tema.base, &tema.palette, 8))
                .or_else(|| colore_di("color.text.1", tema.tokens, tema.base, &tema.palette));
            let Some(testo) = testo else { continue };
            let rapporto = crate::values::contrast_ratio(testo, fondo);
            if rapporto >= CONTRASTO_MINIMO {
                continue;
            }
            avvisi.push(SkinWarning {
                kind: WarningKind::Contrast,
                path: format!("parts.{}.background", base.def.name),
                message: format!(
                    "«{}» si dipinge un fondo su cui il testo fa {rapporto:.2}:1 nel tema {}, \
                     sotto {CONTRASTO_MINIMO}:1. Un colore scritto per esteso resta quello in \
                     tutti e due i temi: usa un token (`color.surface.1` e simili), ridipingi \
                     la parte nella variante, o dichiara `textColor` su questa parte.",
                    base.def.name, tema.nome
                ),
            });
        }
    }
    avvisi
}

/// Un anello del fuoco, misurato contro le superfici in un tema.
#[derive(Debug, Clone, PartialEq)]
pub struct AnelloMisurato {
    /// Dove sta scritto il suo colore: `tokens.color.accent` per l'anello di
    /// serie, `parts.<parte>.focusRing` o `parts.<parte>.states.<stato>.focusRing`
    /// per quello che una parte si disegna da sé.
    pub path: String,
    /// Il tema in cui si è misurato: `scuro` o `chiaro`.
    pub tema: &'static str,
    /// La superficie contro cui si vede peggio.
    pub background: &'static str,
    /// Il rapporto contro quella superficie: il più basso.
    pub rapporto: f64,
}

impl AnelloMisurato {
    /// Si vede abbastanza: almeno [`CONTRASTO_ANELLO`].
    #[must_use]
    pub fn passa(&self) -> bool {
        self.rapporto >= CONTRASTO_ANELLO
    }
}

/// Ogni anello del fuoco di una skin, misurato contro le superfici, in ogni
/// tema che dichiara.
///
/// # Quali anelli
///
/// L'anello di serie è `outline: 2px solid var(--accent)` su `:focus-visible`
/// (`stile.css`): il suo colore è il token `color.accent`, e si misura per
/// primo. Poi quelli che una parte si disegna da sé con `focusRing`, alla base o
/// in uno stato: nel tema della variante conta la parte com'è **in quel tema**,
/// come per le superfici ridipinte (`avvisi_superfici`).
///
/// # Contro cosa
///
/// Contro le tre superfici su cui stanno i comandi (`color.surface.0`…`2`), e
/// vale la peggiore: l'anello sta **fuori** dal comando, sopra quel che c'è
/// attorno, e un anello che si vede su una superficie e sparisce su un'altra
/// sparisce proprio dove il fuoco ci passa. Un colore che segue la copertina
/// non si misura — lo tiene a bada `dinamico.rs` — e nemmeno una superficie che
/// non si può risolvere.
#[must_use]
pub fn focus_ring_pairs(skin: &SkinDocument) -> Vec<AnelloMisurato> {
    let mut anelli = Vec::new();
    for (indice, tema) in temi(skin).iter().enumerate() {
        let superfici: Vec<(&'static str, Rgba)> = DIETRO
            .iter()
            .filter_map(|id| colore_di(id, tema.tokens, tema.base, &tema.palette).map(|c| (*id, c)))
            .collect();
        if superfici.is_empty() {
            continue;
        }
        let mut misura = |path: String, colore: Rgba| {
            let peggiore = superfici
                .iter()
                .map(|(id, dietro)| (*id, crate::values::contrast_ratio(colore, *dietro)))
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((background, rapporto)) = peggiore {
                anelli.push(AnelloMisurato {
                    path,
                    tema: tema.nome,
                    background,
                    rapporto,
                });
            }
        };

        if let Some(accento) = colore_di("color.accent", tema.tokens, tema.base, &tema.palette) {
            misura("tokens.color.accent".to_owned(), accento);
        }

        for base in &skin.parts {
            let parte = (indice > 0)
                .then(|| {
                    skin.variant
                        .as_ref()
                        .and_then(|v| v.parts.iter().find(|p| p.def.name == base.def.name))
                })
                .flatten()
                .unwrap_or(base);
            let nome = base.def.name;
            let mut scritti: Vec<(String, &ColorValue)> = Vec::new();
            if let Some(anello) = parte.appearance.focus_ring.as_ref() {
                scritti.push((format!("parts.{nome}.focusRing"), &anello.color));
            }
            for (stato, aspetto) in parte.states.iter() {
                if let Some(anello) = aspetto.focus_ring.as_ref() {
                    scritti.push((
                        format!("parts.{nome}.states.{}.focusRing", stato.as_str()),
                        &anello.color,
                    ));
                }
            }
            for (path, colore) in scritti {
                if let Some(colore) = risolvi(colore, tema.tokens, tema.base, &tema.palette, 8) {
                    misura(path, colore);
                }
            }
        }
    }
    anelli
}

/// Gli anelli del fuoco che non si vedono abbastanza, come avvisi.
///
/// Un avviso e non un errore, come per il testo: la skin si installa e si
/// indossa, ma chi la scrive lo sa nello Studio e chi la importa lo legge
/// nell'esame prima di indossarla.
fn avvisi_anelli(skin: &SkinDocument) -> Vec<SkinWarning> {
    focus_ring_pairs(skin)
        .into_iter()
        .filter(|a| !a.passa())
        .map(|a| SkinWarning {
            kind: WarningKind::Contrast,
            message: format!(
                "L'anello del fuoco scritto in «{}» fa {:.2}:1 contro «{}» nel tema {}, sotto \
                 {CONTRASTO_ANELLO}:1: chi naviga con la tastiera non vede dove si trova. \
                 L'anello di serie è `color.accent`; una parte può dichiarare un suo \
                 `focusRing` più acceso.",
                a.path, a.rapporto, a.background, a.tema
            ),
            path: a.path,
        })
        .collect()
}

/// Quante volte ogni colore della tavolozza è riferito.
///
/// Un conteggio e non un avviso: un colore usato una volta sola non è un
/// errore, è un letterale con un nome. Lo Studio lo mostra e lascia decidere.
#[must_use]
pub fn palette_usage(skin: &SkinDocument) -> Vec<(String, usize)> {
    let mut conteggi: Vec<(String, usize)> =
        skin.palette.iter().map(|(n, _)| (n.clone(), 0)).collect();

    let mut conta = |colore: &ColorValue| {
        if let ColorValue::Palette { name, .. } = colore
            && let Some(voce) = conteggi.iter_mut().find(|(n, _)| n == name)
        {
            voce.1 += 1;
        }
    };

    let mut insiemi: Vec<&TokenSet> = vec![&skin.tokens];
    insiemi.extend(skin.variante_tokens());
    insiemi.extend(skin.mobile.as_ref());
    for insieme in insiemi {
        for (_, valore) in insieme.iter() {
            match valore {
                TokenValue::Color(colore) => conta(colore),
                TokenValue::Shadow(ombra) => {
                    for livello in &ombra.layers {
                        conta(&livello.color);
                    }
                }
                _ => {}
            }
        }
    }

    let motivi = skin
        .patterns
        .iter()
        .chain(skin.variant.iter().flat_map(|v| v.patterns.iter()));
    for (_, effetto) in motivi {
        for colore in effetto.colors() {
            conta(colore);
        }
    }
    let parti = skin
        .parts
        .iter()
        .chain(skin.variant.iter().flat_map(|v| v.parts.iter()));
    for stile in parti {
        for effetto in &stile.effects() {
            for colore in effetto.colors() {
                conta(colore);
            }
        }
        for aspetto in stile.appearances() {
            for colore in aspetto.colori_propri() {
                conta(colore);
            }
        }
    }
    for (_, animazione) in skin
        .motion
        .as_ref()
        .map(|m| m.animations.as_slice())
        .unwrap_or_default()
    {
        for fotogramma in &animazione.frames {
            for colore in fotogramma.colori() {
                conta(colore);
            }
        }
    }

    conteggi
}

/// Una cosa che non impedisce di usare la skin, ma che chi la crea deve vedere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinWarning {
    /// Perché.
    pub kind: WarningKind,
    /// Dove.
    pub path: String,
    /// Cosa.
    pub message: String,
}

/// Il primo fotogramma di un ciclo è la parte com'è?
fn a_riposo(fotogramma: &AnimFrame) -> bool {
    let zero = |v: Option<f64>| v.is_none_or(|n| n == 0.0);
    let uno = |v: Option<f64>| v.is_none_or(|n| (n - 1.0).abs() < f64::EPSILON);
    let fermo = |l: Option<Length>| l.is_none_or(|l| l.value == 0.0);
    uno(fotogramma.opacity)
        && fermo(fotogramma.translate_x)
        && fermo(fotogramma.translate_y)
        && uno(fotogramma.scale)
        && uno(fotogramma.scale_x)
        && uno(fotogramma.scale_y)
        && zero(fotogramma.rotate.map(|r| r % 360.0))
        && zero(fotogramma.skew_x)
        && zero(fotogramma.skew_y)
        && fermo(fotogramma.blur)
        && uno(fotogramma.brightness)
        && uno(fotogramma.saturate)
        && zero(fotogramma.hue_rotate.map(|h| h % 360.0))
}

/// Gli avvisi di una skin valida.
#[must_use]
pub fn check_skin(skin: &SkinDocument) -> Vec<SkinWarning> {
    let mut avvisi = Vec::new();

    for def in crate::tokens::required_tokens() {
        if skin.tokens.contains(def.id) {
            continue;
        }
        avvisi.push(SkinWarning {
            kind: WarningKind::MissingRequiredToken,
            path: format!("tokens.{}", def.id),
            message: format!(
                "{} Non dichiarato: erediterà il valore di base.",
                def.description
            ),
        });
    }

    // Una variante dichiarata e vuota è una promessa non mantenuta:
    // l'interruttore del tema apparirebbe e non farebbe niente.
    if skin.variant.as_ref().is_some_and(Variante::is_empty) {
        avvisi.push(SkinWarning {
            kind: WarningKind::UnkeptCapability,
            path: "variant".to_owned(),
            message: format!(
                "La skin dichiara un tema {} e non ci scrive niente.",
                skin.scheme.opposto().nome()
            ),
        });
    }

    // Il contrasto: un avviso e non un errore, ma da sapere **prima** di
    // pubblicare.
    for coppia in contrast_pairs(skin) {
        if coppia.passa() {
            continue;
        }
        let dove = if coppia.base < CONTRASTO_MINIMO {
            format!("{:.2}:1 nel tema {}", coppia.base, skin.scheme.nome())
        } else {
            format!(
                "{:.2}:1 nel tema {}",
                coppia.variante.unwrap_or(CONTRASTO_MINIMO),
                skin.scheme.opposto().nome()
            )
        };
        avvisi.push(SkinWarning {
            kind: WarningKind::Contrast,
            path: format!("tokens.{}", coppia.foreground),
            message: format!(
                "«{}» su «{}» fa {dove}, sotto {CONTRASTO_MINIMO}:1. L'opacità conta: \
                 un bianco al 38% sopra una superficie scura non è un bianco.",
                coppia.foreground, coppia.background
            ),
        });
    }

    avvisi.extend(avvisi_superfici(skin));
    avvisi.extend(avvisi_anelli(skin));

    if let Some(layout) = skin.layout.as_ref() {
        let usati = layout.shell.prefabs_used();
        for nome in &layout.prefabs {
            if usati.contains(&nome.as_str()) {
                continue;
            }
            avvisi.push(SkinWarning {
                kind: WarningKind::UnusedPrefab,
                path: format!("layout.prefabs.{nome}"),
                message: format!(
                    "il prefab «{nome}» non è richiamato da nessuna parte dello scafale: \
                     scrivi {{\"prefab\": \"{nome}\"}} dove serve, o toglilo."
                ),
            });
        }

        avvisi.extend(sfori_scafale(skin).into_iter().map(Sforo::avviso));
    }

    let usati: Vec<&str> = tutte_le_parti(skin)
        .flat_map(PartStyle::patterns_used)
        .collect();
    for (nome, _) in &skin.patterns {
        if usati.contains(&nome.as_str()) {
            continue;
        }
        avvisi.push(SkinWarning {
            kind: WarningKind::UnusedPattern,
            path: format!("patterns.{nome}"),
            message: format!(
                "il motivo «{nome}» non è richiamato da nessuna parte: \
                 scrivi {{\"$pattern\": \"{nome}\"}} dove serve, o toglilo."
            ),
        });
    }

    let animate: Vec<&str> = skin
        .parts
        .iter()
        .flat_map(PartStyle::animations_used)
        .collect();
    let dichiarate = skin
        .motion
        .as_ref()
        .map(|blocco| blocco.animations.as_slice())
        .unwrap_or_default();
    for (nome, _) in dichiarate {
        if animate.contains(&nome.as_str()) {
            continue;
        }
        avvisi.push(SkinWarning {
            kind: WarningKind::UnusedAnimation,
            path: format!("motion.animations.{nome}"),
            message: format!(
                "l'animazione «{nome}» non è assegnata a nessuna parte: \
                 scrivi \"motion\": {{\"enter\": \"{nome}\"}} sulla parte che deve \
                 muoversi, o toglila."
            ),
        });
    }
    for (nome, animazione) in dichiarate {
        if animazione.iterations != Iterazioni::Infinite {
            continue;
        }
        if animazione.frames.first().is_some_and(|f| !a_riposo(f)) {
            avvisi.push(SkinWarning {
                kind: WarningKind::RestingLoop,
                path: format!("motion.animations.{nome}.frames.0"),
                message: format!(
                    "«{nome}» è un ciclo, e il suo primo fotogramma non è la parte com'è: \
                     prima di partire e in pausa la parte resta lì, storta. Metti al primo \
                     fotogramma la posa di riposo."
                ),
            });
        }
    }

    // Il movimento: per parte, e per la finestra intera.
    avvisi.extend(sfori_movimento(skin).into_iter().map(Sforo::avviso));

    // La cascata ha senso solo se qualcosa entra — e entra a tempo.
    //
    // Il compilatore la scrive sul ritardo dell'entrata (`enter`), e solo
    // dove l'entrata è un'animazione della parte: su una presenza l'entrata è
    // una transizione di vista, e la vista non ha un indice. Su `inView` non
    // c'è niente da ritardare: ogni voce ha la sua linea di scorrimento, e
    // parte quando entra lei — un ritardo a tempo su una linea che non è il
    // tempo il motore lo ignora. Fino alla 3.0 questo avviso suggeriva proprio
    // `inView` come rimedio, e la cascata restava scritta senza fare niente.
    for stile in &skin.parts {
        let entra_a_tempo = !stile.def.presenza && stile.motion.get(AnimTrigger::Enter).is_some();
        if stile.motion.stagger.is_some() && !entra_a_tempo {
            let perche = if stile.def.presenza {
                "entra come una presenza, tutta insieme, e non ha voci da far partire una dopo l'altra"
            } else if stile.motion.get(AnimTrigger::InView).is_some() {
                "entra con lo scorrimento, e ogni voce parte già quando arriva in vista"
            } else {
                "non ha un'entrata"
            };
            avvisi.push(SkinWarning {
                kind: WarningKind::UnusedAnimation,
                path: format!("parts.{}.motion.stagger", stile.def.name),
                message: format!(
                    "la cascata ritarda l'entrata di ogni voce, e «{}» {perche}: assegna \
                     «enter», o togli la cascata. Parte al più per le prime {CASCATA_VOCI} voci.",
                    stile.def.name
                ),
            });
        }
    }

    // Una superficie che sfora il budget non è illegale: è una superficie che
    // salterà i fotogrammi su un telefono.
    avvisi.extend(sfori_superfici(skin).into_iter().map(Sforo::avviso));

    avvisi
}

// ── I budget, e quando smettono di essere un avviso ─────────────────────────
//
// Un budget sforato è un avviso: lo Studio lo mostra mentre si lavora, e chi
// lavora decide. Ma una skin **arriva** anche da altri, e chi la installa non
// ha lo Studio aperto né sa cosa vuol dire «costo 34 sul budget di 10». Oltre
// [`OLTRE_IL_BUDGET`] volte il budget non si tratta più di una scelta estetica
// cara: è una finestra che scatta a ogni fotogramma, e il posto per dirlo è il
// momento in cui la si installa, non quello in cui la si indossa.
//
// Per questo la soglia non sta nella validazione. Un documento valido resta
// valido — si apre, si modifica, si prova, si esporta per ripararlo — e
// [`controlla_installazione`] è una domanda in più che fa solo chi installa.

/// Oltre quante volte il suo budget un costo impedisce di installare la skin.
pub const OLTRE_IL_BUDGET: u32 = 2;

/// Un costo, e il budget su cui si misura.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Misura {
    /// Quanto costa.
    pub costo: u32,
    /// Quanto dovrebbe costare al più.
    pub budget: u32,
}

impl Misura {
    /// Sfora il budget: un avviso.
    #[must_use]
    pub const fn sfora(self) -> bool {
        self.costo > self.budget
    }

    /// Sfora più di [`OLTRE_IL_BUDGET`] volte: all'installazione è un errore.
    #[must_use]
    pub const fn fuori_misura(self) -> bool {
        self.costo > self.budget.saturating_mul(OLTRE_IL_BUDGET)
    }
}

/// Un budget sforato: dove, di quanto, e con quali parole.
struct Sforo {
    path: String,
    misura: Misura,
    message: String,
}

impl Sforo {
    fn nuovo(path: String, misura: Misura, message: String) -> Self {
        // Chi lavora nello Studio deve sapere **prima** che la skin non si
        // installerà: l'avviso lo dice nella stessa frase.
        let message = if misura.fuori_misura() {
            format!("{message} Oltre {OLTRE_IL_BUDGET} volte il budget la skin non si installa.")
        } else {
            message
        };
        Self {
            path,
            misura,
            message,
        }
    }

    fn avviso(self) -> SkinWarning {
        SkinWarning {
            kind: WarningKind::CostBudget,
            path: self.path,
            message: self.message,
        }
    }
}

/// Le parti della skin in tutti e due i temi.
fn tutte_le_parti(skin: &SkinDocument) -> impl Iterator<Item = &PartStyle> {
    skin.parts
        .iter()
        .chain(skin.variant.iter().flat_map(|v| v.parts.iter()))
}

/// Lo scafale, se sfora. Si misura solo quando la skin lo dichiara: quello di
/// serie costa 3.
fn sfori_scafale(skin: &SkinDocument) -> Option<Sforo> {
    let layout = skin.layout.as_ref()?;
    let misura = Misura {
        costo: layout.shell.costo(),
        budget: crate::layout::SHELL_COST_BUDGET,
    };
    misura.sfora().then(|| {
        Sforo::nuovo(
            "layout.shell".to_owned(),
            misura,
            format!(
                "lo scafale monta {} sul budget di {}: \
                 tanti pezzi insieme sono una finestra in cui non si trova niente.",
                misura.costo, misura.budget
            ),
        )
    })
}

/// Il movimento di ogni parte, e quello della finestra intera, quando sforano.
fn sfori_movimento(skin: &SkinDocument) -> Vec<Sforo> {
    let mut sfori = Vec::new();
    for stile in &skin.parts {
        let misura = Misura {
            costo: stile.motion_cost(),
            budget: crate::movimento::MOTION_COST_BUDGET,
        };
        if misura.sfora() {
            sfori.push(Sforo::nuovo(
                format!("parts.{}.motion", stile.def.name),
                misura,
                format!(
                    "il movimento costa {} sul budget di {}: \
                     togli una ripetizione, un innesco, o un colore animato.",
                    misura.costo, misura.budget
                ),
            ));
        }
    }
    let finestra = movimento_della_finestra(skin);
    if finestra.sfora() {
        sfori.push(Sforo::nuovo(
            "parts".to_owned(),
            finestra,
            format!(
                "tutte le parti insieme muovono {} sul budget di {} della finestra: \
                 ognuna sta nel suo, e insieme fanno una finestra che non sta ferma.",
                finestra.costo, finestra.budget
            ),
        ));
    }
    sfori
}

/// Il movimento che può essere in corso tutto insieme, sul budget della
/// finestra.
fn movimento_della_finestra(skin: &SkinDocument) -> Misura {
    Misura {
        costo: skin.parts.iter().map(|s| s.motion.costo_insieme()).sum(),
        budget: crate::movimento::WINDOW_MOTION_BUDGET,
    }
}

/// Le superfici che sforano, in tutti e due i temi.
fn sfori_superfici(skin: &SkinDocument) -> Vec<Sforo> {
    tutte_le_parti(skin)
        .filter_map(|stile| {
            let misura = Misura {
                costo: stile.costo_superficie(),
                budget: crate::effects::SURFACE_COST_BUDGET,
            };
            misura.sfora().then(|| {
                Sforo::nuovo(
                    format!("parts.{}", stile.def.name),
                    misura,
                    format!(
                        "costo {} sul budget di {}: su WebView questa superficie salterà fotogrammi.",
                        misura.costo, misura.budget
                    ),
                )
            })
        })
        .collect()
}

/// I costi che sforano più di [`OLTRE_IL_BUDGET`] volte il loro budget, come
/// problemi al loro posto nel documento.
///
/// Sono gli stessi avvisi che [`check_skin`] dà per lo Studio, con le stesse
/// parole: qui diventano il motivo per cui una skin non si installa.
#[must_use]
pub fn fuori_misura(skin: &SkinDocument) -> Vec<SkinIssue> {
    sfori_scafale(skin)
        .into_iter()
        .chain(sfori_movimento(skin))
        .chain(sfori_superfici(skin))
        .filter(|sforo| sforo.misura.fuori_misura())
        .map(|sforo| SkinIssue {
            // Un codice suo e non quello del manifest non valido: il manifest
            // è valido, e la frase di quel codice — «non si legge» — mandava a
            // cercare un errore che non c'era.
            code: ErrorCode::SkinOverBudget {
                costo: sforo.misura.costo,
                budget: sforo.misura.budget,
            },
            path: sforo.path,
            message: sforo.message,
            punto: None,
        })
        .collect()
}

/// La domanda che fa chi installa, e solo lui: la skin sta nei suoi budget
/// abbastanza da poterla indossare?
///
/// # Errori
///
/// `skin.overBudget`, con l'elenco dei costi fuori misura, se almeno uno
/// sfora più di [`OLTRE_IL_BUDGET`] volte il suo budget.
pub fn controlla_installazione(skin: &SkinDocument) -> Result<(), AppError> {
    let problemi = fuori_misura(skin);
    if problemi.is_empty() {
        Ok(())
    } else {
        Err(in_errore(&problemi))
    }
}

// ── L'esame prima di indossarla ─────────────────────────────────────────────
//
// Chi installa una skin arrivata da un altro non sa che cosa sta per mettersi
// addosso finché non l'ha addosso. L'esame è quel che gli serve per decidere
// prima: quanto pesa, se si muove e cosa gira per sempre, se il testo si
// legge. Sono tutte cose che il nucleo sa già — i budget, i cicli, le coppie di
// contrasto — raccolte in una forma che un'interfaccia può mostrare senza
// ricalcolarle, e senza decidere da sé che cosa vada chiesto: lo dice
// [`Esame::da_confermare`].

/// Un ciclo che non finisce da solo: dura quanto lo stato che lo accende.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ciclo {
    /// La parte che gira.
    pub parte: &'static str,
    /// Lo stato che lo accende: `playing`, `hover`, …
    pub stato: &'static str,
    /// L'animazione, col nome che ha in `motion.animations`.
    pub animazione: String,
}

/// Quel che chi sta per indossare una skin deve sapere, prima di indossarla.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Esame {
    /// La superficie più cara, sul budget di una superficie.
    pub superficie: Misura,
    /// La parte che si muove di più, sul budget del movimento di una parte.
    pub movimento_parte: Misura,
    /// Il movimento della finestra intera, sul suo budget.
    pub movimento: Misura,
    /// Lo scafale, sul suo budget.
    pub scafale: Misura,
    /// Si muove: almeno una parte ha un'animazione, o la rotta ne ha una, e
    /// l'intensità non è `none`.
    pub anima: bool,
    /// Quel che gira per sempre, finché dura lo stato che lo accende.
    pub cicli: Vec<Ciclo>,
    /// Quante coppie di testo e superficie stanno sotto [`CONTRASTO_MINIMO`],
    /// contando le superfici che la skin si ridipinge.
    pub contrasti: usize,
    /// Quanti anelli del fuoco stanno sotto [`CONTRASTO_ANELLO`], tema per
    /// tema: vedi [`focus_ring_pairs`].
    pub anelli: usize,
}

impl Esame {
    /// C'è qualcosa da chiedere prima di indossarla: un budget sforato, un
    /// testo che non si legge, o un anello del fuoco che non si vede.
    ///
    /// L'anello sta con il testo e non con i cicli: chi naviga con la tastiera e
    /// indossa una skin il cui fuoco non si vede resta senza sapere dove si
    /// trova, ed è una cosa da dire **prima** — non un rifiuto, una domanda.
    ///
    /// I cicli **non** bastano: sono ammessi, si fermano con «Riduci il
    /// movimento», e una skin che fa girare il disco mentre suona non merita una
    /// domanda. Se la domanda c'è per altro, l'esame li elenca.
    #[must_use]
    pub fn da_confermare(&self) -> bool {
        self.contrasti > 0
            || self.anelli > 0
            || [
                self.superficie,
                self.movimento_parte,
                self.movimento,
                self.scafale,
            ]
            .iter()
            .any(|m| m.sfora())
    }
}

/// Esamina una skin valida. Vedi [`Esame`].
#[must_use]
pub fn esamina(skin: &SkinDocument) -> Esame {
    let ferma = skin
        .motion
        .as_ref()
        .is_none_or(|m| m.intensity == MotionIntensity::None);
    let rotta = skin
        .motion
        .as_ref()
        .and_then(|m| m.route.as_ref())
        .is_some_and(|r| {
            r.out.is_some_and(|f| !f.is_empty()) || r.enter.is_some_and(|f| !f.is_empty())
        });
    let cicli: Vec<Ciclo> = if ferma {
        Vec::new()
    } else {
        skin.parts
            .iter()
            .flat_map(|stile| {
                stile
                    .motion
                    .assegnate()
                    .into_iter()
                    .filter(|(_, r)| r.def.iterations == Iterazioni::Infinite)
                    .map(|(innesco, r)| Ciclo {
                        parte: stile.def.name,
                        stato: innesco.as_str(),
                        animazione: r.name.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    Esame {
        superficie: Misura {
            costo: tutte_le_parti(skin)
                .map(PartStyle::costo_superficie)
                .max()
                .unwrap_or(0),
            budget: crate::effects::SURFACE_COST_BUDGET,
        },
        movimento_parte: Misura {
            costo: skin
                .parts
                .iter()
                .map(PartStyle::motion_cost)
                .max()
                .unwrap_or(0),
            budget: crate::movimento::MOTION_COST_BUDGET,
        },
        movimento: movimento_della_finestra(skin),
        scafale: Misura {
            costo: skin
                .layout
                .as_ref()
                .map_or_else(|| shell_di_serie().costo(), |l| l.shell.costo()),
            budget: crate::layout::SHELL_COST_BUDGET,
        },
        anima: !ferma
            && (rotta
                || skin
                    .parts
                    .iter()
                    .any(|stile| !stile.motion.senza_animazioni())),
        cicli,
        contrasti: contrast_pairs(skin).iter().filter(|c| !c.passa()).count()
            + avvisi_superfici(skin).len(),
        anelli: focus_ring_pairs(skin).iter().filter(|a| !a.passa()).count(),
    }
}

#[cfg(test)]
mod tests;
