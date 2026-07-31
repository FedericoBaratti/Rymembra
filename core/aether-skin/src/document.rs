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
//! esecuzione. Quindi i due blocchi grandi raccolgono, il resto si ferma al
//! primo problema.

use aether_domain::errors::{AppError, ErrorCode};
use serde_json::{Map, Value};

use crate::effects::{Corner, Effect, RadialShape, Stop};
use crate::parts::{
    PartAppearance, PartLayer, PartState, PartStates, PartStyle, TextTransform, nearest_parts, part,
};
use crate::tokens::{
    ColorValue, DynamicSource, ShadowLayer, ShadowValue, TOKENS, TokenDef, TokenKind, TokenSet,
    TokenValue, is_local_name, token,
};
use crate::values::{
    Duration, Easing, EasingKeyword, Length, LengthUnit, LengthValue, Rgba, StepPosition,
    is_font_family, parse_color, parse_duration, parse_length,
};
use crate::vicini::{forse, vicini};

/// La versione del formato. Un numero, non semver: cambia solo se rompiamo.
pub const SKIN_FORMAT_VERSION: u32 = 1;

/// Un problema in un punto preciso del documento.
#[derive(Debug, Clone, PartialEq)]
pub struct SkinIssue {
    /// Dove: `tokens.color.accent`.
    pub path: String,
    /// Cosa non va, e cosa scrivere al suo posto.
    pub message: String,
    /// Il codice del catalogo che descrive questo problema.
    pub code: ErrorCode,
}

/// Il tipo di risultato dei parser interni.
type Esito<T> = Result<T, SkinIssue>;

fn problema(path: &str, message: impl Into<String>) -> SkinIssue {
    let message = message.into();
    SkinIssue {
        code: ErrorCode::SkinManifestInvalid {
            detail: Some(format!("{path}: {message}")),
        },
        path: path.to_owned(),
        message,
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

fn giu(path: &str, name: &str) -> String {
    if path.is_empty() {
        return name.to_owned();
    }
    format!("{path}.{name}")
}

// ── Le forme elementari ─────────────────────────────────────────────────────

fn oggetto<'a>(value: &'a Value, path: &str) -> Esito<&'a Map<String, Value>> {
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

/// Le sole chiavi ammesse. È il `strict` del vecchio schema.
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
/// alternative e riportare l'ultimo errore. È la stessa scelta che il vecchio
/// albero aveva dovuto fare a mano contro l'unione di zod: chi scrive `blu`
/// deve leggere «`blu` non è un colore», non «input non valido».
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
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
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
        solo_chiavi(
            livello,
            &dentro,
            &["inset", "x", "y", "blur", "spread", "color"],
        )?;
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

// ── Gli effetti ─────────────────────────────────────────────────────────────

fn fermate(value: &Value, path: &str) -> Esito<Vec<Stop>> {
    let voci = lista_fra(value, path, 2, 8)?;
    let mut stops = Vec::with_capacity(voci.len());
    for (indice, grezzo) in voci.iter().enumerate() {
        let dentro = giu(path, &indice.to_string());
        let map = oggetto(grezzo, &dentro)?;
        solo_chiavi(map, &dentro, &["color", "at"])?;
        stops.push(Stop {
            color: colore(richiesto(map, &dentro, "color")?, &giu(&dentro, "color"))?,
            at: campo(map, "at")
                .map(|v| lunghezza(v, &giu(&dentro, "at")))
                .transpose()?,
        });
    }
    Ok(stops)
}

fn centro(map: &Map<String, Value>, path: &str) -> Esito<Option<(Length, Length)>> {
    let Some(grezzo) = campo(map, "at") else {
        return Ok(None);
    };
    let percorso = giu(path, "at");
    let coppia = lista_fra(grezzo, &percorso, 2, 2)?;
    let (Some(x), Some(y)) = (coppia.first(), coppia.get(1)) else {
        return Err(problema(&percorso, "ci vogliono due lunghezze"));
    };
    Ok(Some((
        lunghezza(x, &giu(&percorso, "0"))?,
        lunghezza(y, &giu(&percorso, "1"))?,
    )))
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
fn effetto(value: &Value, path: &str) -> Esito<Effect> {
    let map = oggetto(value, path)?;
    let percorso = giu(path, "effect");
    let nome = testo(richiesto(map, path, "effect")?, &percorso)?;

    let colore_di =
        |name: &str| -> Esito<ColorValue> { colore(richiesto(map, path, name)?, &giu(path, name)) };
    let lunghezza_di =
        |name: &str| -> Esito<Length> { lunghezza(richiesto(map, path, name)?, &giu(path, name)) };
    let fermate_di =
        || -> Esito<Vec<Stop>> { fermate(richiesto(map, path, "stops")?, &giu(path, "stops")) };

    match nome {
        "solid" => {
            solo_chiavi(map, path, &["effect", "color"])?;
            Ok(Effect::Solid {
                color: colore_di("color")?,
            })
        }
        "linearGradient" => {
            solo_chiavi(map, path, &["effect", "angle", "stops"])?;
            Ok(Effect::LinearGradient {
                angle: angolo(map, path, "angle", 180.0)?,
                stops: fermate_di()?,
            })
        }
        "radialGradient" => {
            solo_chiavi(map, path, &["effect", "shape", "at", "size", "stops"])?;
            Ok(Effect::RadialGradient {
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
            })
        }
        "conicGradient" => {
            solo_chiavi(map, path, &["effect", "from", "at", "stops"])?;
            Ok(Effect::ConicGradient {
                from: angolo(map, path, "from", 0.0)?,
                at: centro(map, path)?,
                stops: fermate_di()?,
            })
        }
        "hairlineGrid" => {
            solo_chiavi(
                map,
                path,
                &["effect", "color", "cell", "cellY", "thickness"],
            )?;
            Ok(Effect::HairlineGrid {
                color: colore_di("color")?,
                cell: lunghezza_di("cell")?,
                cell_y: lunghezza_facoltativa(map, path, "cellY")?,
                thickness: lunghezza_facoltativa(map, path, "thickness")?,
            })
        }
        "scanlines" => {
            solo_chiavi(map, path, &["effect", "color", "line", "gap"])?;
            Ok(Effect::Scanlines {
                color: colore_di("color")?,
                line: lunghezza_di("line")?,
                gap: lunghezza_di("gap")?,
            })
        }
        "stripes" => {
            solo_chiavi(
                map,
                path,
                &["effect", "angle", "color", "background", "width"],
            )?;
            Ok(Effect::Stripes {
                angle: angolo(map, path, "angle", -45.0)?,
                color: colore_di("color")?,
                background: colore_di("background")?,
                width: lunghezza_di("width")?,
            })
        }
        "dotGrid" => {
            solo_chiavi(map, path, &["effect", "color", "spacing", "dot"])?;
            Ok(Effect::DotGrid {
                color: colore_di("color")?,
                spacing: lunghezza_di("spacing")?,
                dot: lunghezza_di("dot")?,
            })
        }
        "vignette" => {
            solo_chiavi(map, path, &["effect", "color", "start"])?;
            Ok(Effect::Vignette {
                color: colore_di("color")?,
                start: campo(map, "start")
                    .map(|v| numero_fra(v, &giu(path, "start"), 0.0, 100.0))
                    .transpose()?
                    .unwrap_or(60.0),
            })
        }
        "chamfer" => {
            solo_chiavi(map, path, &["effect", "size", "corners"])?;
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
        "blurBehind" => {
            solo_chiavi(map, path, &["effect", "radius", "saturate"])?;
            Ok(Effect::BlurBehind {
                radius: lunghezza_di("radius")?,
                saturate: campo(map, "saturate")
                    .map(|v| numero_fra(v, &giu(path, "saturate"), 0.0, 400.0))
                    .transpose()?,
            })
        }
        altro => {
            let candidati = vicini(altro, Effect::NAMES.iter().copied());
            Err(SkinIssue {
                code: ErrorCode::SkinUnknownEffect {
                    effect_type: altro.to_owned(),
                },
                path: percorso.clone(),
                message: format!("effetto sconosciuto: «{altro}».{}", forse(&candidati)),
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
    // Il messaggio preciso viene da sotto; qui si aggiunge soltanto il codice
    // che nomina il token, che è ciò che serve a chi legge un log invece del
    // documento.
    esito.map_err(|dentro| token_non_valido(&dentro.path, def.id, value, dentro.message))
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

fn aspetto(value: &Value, path: &str) -> Esito<PartAppearance> {
    let map = oggetto(value, path)?;
    solo_chiavi(
        map,
        path,
        &[
            "background",
            "textColor",
            "borderColor",
            "borderWidth",
            "radius",
            "clip",
            "opacity",
            "letterSpacing",
            "textTransform",
            "fontWeight",
        ],
    )?;
    Ok(PartAppearance {
        background: match campo(map, "background") {
            None => Vec::new(),
            Some(grezzo) => livelli(grezzo, &giu(path, "background"), 0, 4)?,
        },
        text_color: campo(map, "textColor")
            .map(|v| colore(v, &giu(path, "textColor")))
            .transpose()?,
        border_color: campo(map, "borderColor")
            .map(|v| colore(v, &giu(path, "borderColor")))
            .transpose()?,
        border_width: lunghezza_facoltativa(map, path, "borderWidth")?,
        radius: lunghezza_facoltativa(map, path, "radius")?,
        clip: campo(map, "clip")
            .map(|v| effetto(v, &giu(path, "clip")))
            .transpose()?,
        opacity: campo(map, "opacity")
            .map(|v| numero_fra(v, &giu(path, "opacity"), 0.0, 1.0))
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

fn livelli(value: &Value, path: &str, min: usize, max: usize) -> Esito<Vec<Effect>> {
    let voci = lista_fra(value, path, min, max)?;
    let mut effetti = Vec::with_capacity(voci.len());
    for (indice, grezzo) in voci.iter().enumerate() {
        effetti.push(effetto(grezzo, &giu(path, &indice.to_string()))?);
    }
    Ok(effetti)
}

fn stile_parte(nome: &str, value: &Value, path: &str) -> Esito<PartStyle> {
    let Some(def) = part(nome) else {
        let candidati = nearest_parts(nome);
        return Err(problema(
            path,
            format!("parte inesistente: «{nome}».{}", forse(&candidati)),
        ));
    };
    let map = oggetto(value, path)?;

    // L'aspetto e i due blocchi strutturali stanno nello stesso oggetto: si
    // toglie quel che non è aspetto e si valida il resto con la stessa
    // funzione degli stati, così base e stati non possono divergere.
    let mut solo_aspetto = map.clone();
    solo_aspetto.remove("layer");
    solo_aspetto.remove("states");
    let appearance = aspetto(&Value::Object(solo_aspetto), path)?;

    let layer = match campo(map, "layer") {
        None => None,
        Some(grezzo) => {
            let dentro = giu(path, "layer");
            if !def.layers {
                // Non è un capriccio: lo pseudo-elemento di questa parte è già
                // usato dal componente, e un secondo `::after` non esiste. Una
                // skin che ci scrive sopra non otterrebbe il suo livello — e
                // toglierebbe quello che c'era.
                return Err(problema(
                    &dentro,
                    format!("la parte «{nome}» non ha uno pseudo-elemento libero per un livello"),
                ));
            }
            let livello = oggetto(grezzo, &dentro)?;
            solo_chiavi(livello, &dentro, &["background", "opacity"])?;
            Some(PartLayer {
                background: livelli(
                    richiesto(livello, &dentro, "background")?,
                    &giu(&dentro, "background"),
                    1,
                    3,
                )?,
                opacity: campo(livello, "opacity")
                    .map(|v| numero_fra(v, &giu(&dentro, "opacity"), 0.0, 1.0))
                    .transpose()?,
            })
        }
    };

    let mut states = PartStates::default();
    if let Some(grezzo) = campo(map, "states") {
        let dentro = giu(path, "states");
        let blocco = oggetto(grezzo, &dentro)?;
        let nomi: Vec<&str> = PartState::ALL.iter().map(|s| s.as_str()).collect();
        solo_chiavi(blocco, &dentro, &nomi)?;
        for stato in PartState::ALL {
            let Some(valore) = campo(blocco, stato.as_str()) else {
                continue;
            };
            let aspetto_stato = aspetto(valore, &giu(&dentro, stato.as_str()))?;
            match *stato {
                PartState::Hover => states.hover = Some(aspetto_stato),
                PartState::Active => states.active = Some(aspetto_stato),
                PartState::Focus => states.focus = Some(aspetto_stato),
                PartState::Disabled => states.disabled = Some(aspetto_stato),
            }
        }
    }

    Ok(PartStyle {
        def,
        appearance,
        layer,
        states,
    })
}

// ── I blocchi del documento ─────────────────────────────────────────────────

/// Chi ha fatto la skin, e come si chiama.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinMeta {
    /// Il nome mostrato nel selettore.
    pub name: String,
    /// L'autore.
    pub author: String,
    /// La versione della skin, `1.0.0`. Serve all'allineamento fra PC e telefono.
    pub version: String,
    /// A cosa somiglia.
    pub description: Option<String>,
    /// La licenza.
    pub license: Option<String>,
    /// Da quale skin è stata derivata, quando è un fork.
    pub based_on: Option<String>,
}

/// Cosa la skin dichiara di saper fare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkinCapabilities {
    /// Ha una variante chiara. Se falsa, l'interruttore del tema non si mostra.
    pub light: bool,
    /// Ha sovrascritture pensate per lo schermo di un telefono.
    pub mobile: bool,
    /// Va bene che i colori seguano la copertina. Una skin con una tavolozza
    /// fissa e voluta — Nothing è bianco e nero per scelta — dice falso.
    pub dynamic_accent: bool,
}

impl Default for SkinCapabilities {
    fn default() -> Self {
        Self {
            light: false,
            mobile: false,
            dynamic_accent: true,
        }
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
/// Solo opacità e trasformazione: sono le due proprietà che il compositore
/// anima senza ridisegnare, e il vincolo è del compilatore, non una
/// raccomandazione scritta da qualche parte.
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

/// Come si sostituiscono due schermate.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RouteTransition {
    /// Come esce quella vecchia.
    pub out: Option<RouteFrame>,
    /// Come entra quella nuova.
    pub enter: Option<RouteFrame>,
}

/// Il movimento della skin.
#[derive(Debug, Clone, PartialEq)]
pub struct SkinMotion {
    /// Quanto si muove.
    ///
    /// Si **compone** con `prefers-reduced-motion`, non lo sovrascrive verso
    /// l'alto: la media query del sistema viene dopo, quindi vince.
    pub intensity: MotionIntensity,
    /// Curve aggiuntive della skin, oltre a quelle del registro. In ordine di
    /// nome, perché il foglio prodotto dev'essere deterministico.
    pub easings: Vec<(String, Easing)>,
    /// Le transizioni di rotta.
    pub route_transition: Option<RouteTransition>,
}

/// Dove sta il player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerLayout {
    /// Attaccato in basso.
    BottomBar,
    /// Flottante.
    Floating,
    /// Ridotto.
    Compact,
}

/// Come si presenta la barra laterale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarLayout {
    /// Solo icone.
    Rail,
    /// Aperta.
    Expanded,
    /// Nascosta.
    Hidden,
}

/// Quanto respira il contenuto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Density {
    /// Stretto.
    Compact,
    /// Normale.
    Comfortable,
    /// Largo.
    Spacious,
}

/// Le scelte di impaginazione.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkinLayout {
    /// Il player.
    pub player: PlayerLayout,
    /// La barra laterale.
    pub sidebar: SidebarLayout,
    /// La densità.
    pub density: Density,
}

impl Default for SkinLayout {
    fn default() -> Self {
        Self {
            player: PlayerLayout::Floating,
            sidebar: SidebarLayout::Rail,
            density: Density::Comfortable,
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
    /// Cosa dichiara di saper fare.
    pub capabilities: SkinCapabilities,
    /// I colori locali, in ordine di nome.
    ///
    /// Sono i token skin-locali del vecchio albero: `--cyber-teal`,
    /// `--nothing-red`. Nessun componente li legge — servono alla skin per non
    /// ripetere lo stesso valore in venti dichiarazioni.
    pub palette: Vec<(String, Rgba)>,
    /// I token del blocco base.
    pub tokens: TokenSet,
    /// Le sovrascritture per `[data-theme='light']`.
    pub light: Option<TokenSet>,
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

const CHIAVI_DOCUMENTO: &[&str] = &[
    "format",
    "id",
    "meta",
    "capabilities",
    "palette",
    "tokens",
    "themes",
    "platforms",
    "motion",
    "layout",
    "patterns",
    "parts",
];

fn identificatore(value: &Value, path: &str) -> Esito<String> {
    let scritto = testo(value, path)?;
    let mut caratteri = scritto.bytes();
    let ammesso = caratteri.next().is_some_and(|b| b.is_ascii_lowercase())
        && caratteri.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !ammesso || scritto.len() < 2 || scritto.len() > 48 {
        // Vincolato perché finisce in un selettore CSS, in un nome di file e in
        // un URL della rete locale: un id con una virgoletta o una barra
        // romperebbe uno dei tre, ed è proprio la fuga che il formato deve
        // rendere impossibile.
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

fn meta(value: &Value, path: &str) -> Esito<SkinMeta> {
    let map = oggetto(value, path)?;
    solo_chiavi(
        map,
        path,
        &[
            "name",
            "author",
            "version",
            "description",
            "license",
            "basedOn",
        ],
    )?;
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
    })
}

fn capacita(value: &Value, path: &str) -> Esito<SkinCapabilities> {
    let map = oggetto(value, path)?;
    solo_chiavi(map, path, &["light", "mobile", "dynamicAccent"])?;
    let difetto = SkinCapabilities::default();
    let leggi = |name: &str, difetto: bool| -> Esito<bool> {
        campo(map, name)
            .map(|v| booleano(v, &giu(path, name)))
            .transpose()
            .map(|v| v.unwrap_or(difetto))
    };
    Ok(SkinCapabilities {
        light: leggi("light", difetto.light)?,
        mobile: leggi("mobile", difetto.mobile)?,
        dynamic_accent: leggi("dynamicAccent", difetto.dynamic_accent)?,
    })
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
        // anche fare un ciclo, e un ciclo in un foglio di stile non è un errore
        // — è un valore che semplicemente non si applica.
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

fn fotogramma(value: &Value, path: &str) -> Esito<RouteFrame> {
    let map = oggetto(value, path)?;
    solo_chiavi(map, path, &["opacity", "scale", "translateY"])?;
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

fn movimento(value: &Value, path: &str) -> Esito<SkinMotion> {
    let map = oggetto(value, path)?;
    solo_chiavi(map, path, &["intensity", "easings", "routeTransition"])?;

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
        for (nome, curva) in oggetto(grezzo, &dentro)? {
            let percorso = giu(&dentro, nome);
            if !is_easing_name(nome) {
                return Err(problema(
                    &percorso,
                    "il nome di una curva ammette minuscole, cifre e lettere, e comincia con una minuscola",
                ));
            }
            easings.push((nome.clone(), easing(curva, &percorso)?));
        }
        easings.sort_by(|a, b| a.0.cmp(&b.0));
    }

    let route_transition = match campo(map, "routeTransition") {
        None => None,
        Some(grezzo) => {
            let dentro = giu(path, "routeTransition");
            let blocco = oggetto(grezzo, &dentro)?;
            solo_chiavi(blocco, &dentro, &["out", "in"])?;
            Some(RouteTransition {
                out: campo(blocco, "out")
                    .map(|v| fotogramma(v, &giu(&dentro, "out")))
                    .transpose()?,
                enter: campo(blocco, "in")
                    .map(|v| fotogramma(v, &giu(&dentro, "in")))
                    .transpose()?,
            })
        }
    };

    Ok(SkinMotion {
        intensity,
        easings,
        route_transition,
    })
}

fn impaginazione(value: &Value, path: &str) -> Esito<SkinLayout> {
    let map = oggetto(value, path)?;
    solo_chiavi(map, path, &["player", "sidebar", "density"])?;
    let difetto = SkinLayout::default();
    Ok(SkinLayout {
        player: campo(map, "player")
            .map(|v| {
                parola(
                    v,
                    &giu(path, "player"),
                    &[
                        ("bottom-bar", PlayerLayout::BottomBar),
                        ("floating", PlayerLayout::Floating),
                        ("compact", PlayerLayout::Compact),
                    ],
                )
            })
            .transpose()?
            .unwrap_or(difetto.player),
        sidebar: campo(map, "sidebar")
            .map(|v| {
                parola(
                    v,
                    &giu(path, "sidebar"),
                    &[
                        ("rail", SidebarLayout::Rail),
                        ("expanded", SidebarLayout::Expanded),
                        ("hidden", SidebarLayout::Hidden),
                    ],
                )
            })
            .transpose()?
            .unwrap_or(difetto.sidebar),
        density: campo(map, "density")
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
            .unwrap_or(difetto.density),
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

fn superfici(value: &Value, path: &str, problemi: &mut Vec<SkinIssue>) -> Esito<Vec<PartStyle>> {
    let map = oggetto(value, path)?;
    let mut stili = Vec::with_capacity(map.len());
    for (nome, grezzo) in map {
        match stile_parte(nome, grezzo, &giu(path, nome)) {
            Ok(stile) => stili.push(stile),
            Err(guasto) => problemi.push(guasto),
        }
    }
    // Ordine di registro e non alfabetico: nel CSS l'ordine decide la cascata a
    // parità di specificità, e il registro va dalla cornice alle
    // sovrapposizioni, cioè dal generale al particolare. Nel vecchio albero il
    // commento diceva «l'ordine è quello del registro» e il codice ordinava per
    // nome, il che metteva `toast-card` prima di `app-shell` senza che nessuno
    // l'avesse deciso.
    stili.sort_by_key(|stile| {
        crate::parts::PARTS
            .iter()
            .position(|def| def.name == stile.def.name)
            .unwrap_or(usize::MAX)
    });
    Ok(stili)
}

/// Un problema che ferma la lettura, invece di aggiungersi all'elenco.
fn solo(guasto: SkinIssue) -> Vec<SkinIssue> {
    vec![guasto]
}

fn documento(raw: &Value) -> Result<SkinDocument, Vec<SkinIssue>> {
    let mut problemi: Vec<SkinIssue> = Vec::new();

    let map = oggetto(raw, "").map_err(solo)?;
    solo_chiavi(map, "", CHIAVI_DOCUMENTO).map_err(solo)?;

    let id = identificatore(richiesto(map, "", "id").map_err(solo)?, "id").map_err(solo)?;
    let meta = meta(richiesto(map, "", "meta").map_err(solo)?, "meta").map_err(solo)?;
    let capabilities = match campo(map, "capabilities") {
        None => SkinCapabilities::default(),
        Some(grezzo) => capacita(grezzo, "capabilities").map_err(solo)?,
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

    let light = match campo(map, "themes") {
        None => None,
        Some(grezzo) => {
            let blocco = oggetto(grezzo, "themes").map_err(solo)?;
            solo_chiavi(blocco, "themes", &["light"]).map_err(solo)?;
            match campo(blocco, "light") {
                None => None,
                Some(chiaro) => {
                    Some(insieme_token(chiaro, "themes.light", &mut problemi).map_err(solo)?)
                }
            }
        }
    };

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
    let layout = campo(map, "layout")
        .map(|grezzo| impaginazione(grezzo, "layout"))
        .transpose()
        .map_err(solo)?;
    let patterns = match campo(map, "patterns") {
        None => Vec::new(),
        Some(grezzo) => motivi(grezzo, "patterns").map_err(solo)?,
    };
    let parts = match campo(map, "parts") {
        None => Vec::new(),
        Some(grezzo) => superfici(grezzo, "parts", &mut problemi).map_err(solo)?,
    };

    if !problemi.is_empty() {
        return Err(problemi);
    }

    Ok(SkinDocument {
        id,
        meta,
        capabilities,
        palette,
        tokens,
        light,
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
    // esatto; il messaggio li elenca tutti, perché chi sta scrivendo la skin
    // vuole correggerli in una passata sola.
    let codice = problemi
        .first()
        .map_or(ErrorCode::SkinManifestInvalid { detail: None }, |guasto| {
            guasto.code.clone()
        });
    AppError::new(codice).with_message(elenco.join("; "))
}

fn versione_formato(raw: &Value) -> Option<u64> {
    raw.as_object()?.get("format")?.as_u64()
}

/// Valida un documento skin.
///
/// La versione del formato si controlla **prima** dello schema e a parte: un
/// pacchetto scritto da una versione futura dell'app non è malformato, è solo
/// più nuovo, e i due casi vogliono messaggi diversi. È la stessa distinzione
/// che nel database separa una migrazione fallita da un database più avanti
/// del codice.
///
/// # Errori
///
/// `skin.formatUnsupported` se la versione non è la nostra; altrimenti il codice
/// del primo problema trovato, col messaggio che li elenca tutti.
pub fn parse_skin(raw: &Value) -> Result<SkinDocument, AppError> {
    let trovata = versione_formato(raw);
    match trovata {
        None => {
            return Err(AppError::new(ErrorCode::SkinManifestInvalid {
                detail: Some("manca il campo «format»".to_owned()),
            }));
        }
        Some(versione) if versione != u64::from(SKIN_FORMAT_VERSION) => {
            let found = u32::try_from(versione).unwrap_or(u32::MAX);
            return Err(AppError::new(ErrorCode::SkinFormatUnsupported {
                found,
                supported: SKIN_FORMAT_VERSION,
            }));
        }
        Some(_) => {}
    }

    documento(raw).map_err(|problemi| in_errore(&problemi))
}

/// Valida un documento skin scritto come testo JSON.
///
/// # Errori
///
/// `skin.manifestInvalid` se il testo non è JSON; per il resto come
/// [`parse_skin`].
pub fn parse_skin_json(text: &str) -> Result<SkinDocument, AppError> {
    let raw: Value = serde_json::from_str(text).map_err(|err| {
        AppError::new(ErrorCode::SkinManifestInvalid {
            detail: Some("il manifest non è JSON valido".to_owned()),
        })
        .with_cause(err.to_string())
    })?;
    parse_skin(&raw)
}

// ── Gli avvisi ──────────────────────────────────────────────────────────────

/// Perché un avviso è stato emesso.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningKind {
    /// Un token obbligatorio non è dichiarato.
    MissingRequiredToken,
    /// Una promessa delle capacità non è mantenuta.
    UnkeptCapability,
    /// Un motivo dichiarato e mai usato.
    UnusedPattern,
    /// Una superficie che sfora il budget di costo.
    CostBudget,
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

/// Gli avvisi di una skin valida.
///
/// Il primo è quello che nel vecchio albero costava un mese di silenzio. Un
/// token obbligatorio non dichiarato non è un errore — la skin eredita quello di
/// base — ma quasi sempre è una dimenticanza, e il risultato è una superficie
/// che resta del colore sbagliato in una schermata che si visita raramente.
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

    // Un tema chiaro dichiarato e vuoto è una promessa non mantenuta:
    // l'interruttore del tema apparirebbe e non farebbe niente.
    if skin.capabilities.light && skin.light.as_ref().is_none_or(TokenSet::is_empty) {
        avvisi.push(SkinWarning {
            kind: WarningKind::UnkeptCapability,
            path: "themes.light".to_owned(),
            message: "La skin dichiara di avere una variante chiara ma non la definisce."
                .to_owned(),
        });
    }
    if skin.capabilities.mobile && skin.mobile.as_ref().is_none_or(TokenSet::is_empty) {
        avvisi.push(SkinWarning {
            kind: WarningKind::UnkeptCapability,
            path: "platforms.mobile".to_owned(),
            message: "La skin dichiara sovrascritture per telefono ma non le definisce.".to_owned(),
        });
    }

    // Una superficie che sfora il budget non è illegale: è una superficie che
    // salterà i fotogrammi su un telefono, e chi la sta costruendo su un PC non
    // ha modo di accorgersene.
    for stile in &skin.parts {
        let effetti = stile.effects();
        if crate::effects::exceeds_budget(&effetti) {
            avvisi.push(SkinWarning {
                kind: WarningKind::CostBudget,
                path: format!("parts.{}", stile.def.name),
                message: format!(
                    "costo {} sul budget di {}: su WebView questa superficie salterà fotogrammi.",
                    crate::effects::stack_cost(&effetti),
                    crate::effects::SURFACE_COST_BUDGET
                ),
            });
        }
    }

    avvisi
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minima(extra: &str) -> String {
        format!(
            r##"{{
              "format": 1,
              "id": "prova",
              "meta": {{ "name": "Prova", "author": "Aether", "version": "1.0.0" }},
              "tokens": {{ "color.accent": "#8b7cf6" }}
              {extra}
            }}"##
        )
    }

    fn rifiuta(json: &str) -> AppError {
        match parse_skin_json(json) {
            Ok(_) => panic!("accettato quel che andava rifiutato: {json}"),
            Err(err) => err,
        }
    }

    #[test]
    fn una_skin_minima_passa() {
        let skin = parse_skin_json(&minima("")).expect("valida");
        assert_eq!(skin.id, "prova");
        assert_eq!(skin.tokens.len(), 1);
        // I difetti sono quelli documentati, non l'assenza di scelta.
        assert!(skin.capabilities.dynamic_accent);
        assert!(!skin.capabilities.light);
    }

    #[test]
    fn una_chiave_sconosciuta_e_un_errore() {
        // È la differenza col vecchio albero: là una chiave in più era CSS che
        // il browser ignorava, e la skin restava rotta in un punto solo.
        let err = rifiuta(&minima(r##", "colours": {}"##));
        let messaggio = err.message().unwrap_or_default();
        assert!(messaggio.contains("colours"), "{messaggio}");
        assert!(messaggio.contains("sconosciuta"), "{messaggio}");
    }

    #[test]
    fn un_token_sbagliato_riceve_un_suggerimento() {
        let err = rifiuta(&minima("").replace("color.accent", "color.accents"));
        let messaggio = err.message().unwrap_or_default();
        assert!(messaggio.contains("color.accents"), "{messaggio}");
        assert!(messaggio.contains("«color.accent»"), "{messaggio}");
    }

    #[test]
    fn un_valore_di_token_sbagliato_nomina_il_token() {
        let err = rifiuta(&minima("").replace("#8b7cf6", "blu"));
        // Il codice del catalogo, non solo un messaggio: è ciò che permette di
        // contare quante skin vengono rifiutate e perché.
        assert_eq!(
            *err.code(),
            ErrorCode::SkinTokenInvalid {
                token: "color.accent".to_owned(),
                value: "\"blu\"".to_owned(),
            }
        );
        let messaggio = err.message().unwrap_or_default();
        assert!(messaggio.contains("non è un colore"), "{messaggio}");
    }

    #[test]
    fn gli_errori_dei_token_arrivano_tutti_insieme() {
        // Chi sta scrivendo una skin corregge in una passata sola invece di
        // scoprire un errore per esecuzione.
        let err = rifiuta(&minima("").replace(
            r##""color.accent": "#8b7cf6""##,
            r##""color.accent": "blu", "color.danger": "rosso", "color.text.9": "#fff""##,
        ));
        let messaggio = err.message().unwrap_or_default();
        assert!(messaggio.contains("color.accent"), "{messaggio}");
        assert!(messaggio.contains("color.danger"), "{messaggio}");
        assert!(messaggio.contains("color.text.9"), "{messaggio}");
    }

    #[test]
    fn una_versione_futura_non_e_un_manifest_rotto() {
        let err = rifiuta(&minima("").replace("\"format\": 1", "\"format\": 2"));
        assert_eq!(
            *err.code(),
            ErrorCode::SkinFormatUnsupported {
                found: 2,
                supported: 1
            }
        );
    }

    #[test]
    fn un_effetto_sconosciuto_ha_il_suo_codice() {
        let err = rifiuta(&minima(
            r##", "patterns": { "griglia": { "effect": "hairlineGrids", "color": "#fff", "cell": "36px" } }"##,
        ));
        assert_eq!(
            *err.code(),
            ErrorCode::SkinUnknownEffect {
                effect_type: "hairlineGrids".to_owned()
            }
        );
        let messaggio = err.message().unwrap_or_default();
        assert!(messaggio.contains("«hairlineGrid»"), "{messaggio}");
    }

    #[test]
    fn un_id_che_romperebbe_il_selettore_non_passa() {
        for cattivo in ["Prova", "pro va", "pro'va", "p", "pro/va", "1prova"] {
            let json = minima("").replace("\"id\": \"prova\"", &format!("\"id\": \"{cattivo}\""));
            rifiuta(&json);
        }
    }

    #[test]
    fn un_livello_su_una_parte_senza_pseudo_elemento_e_un_errore() {
        // `nav-pill` ha `layers: false`: il suo ::after è già usato dal
        // componente, e una skin che ci scrive sopra toglierebbe quel che c'è.
        let err = rifiuta(&minima(
            r##", "parts": { "nav-pill": { "layer": { "background": [{ "effect": "solid", "color": "#fff" }] } } }"##,
        ));
        let messaggio = err.message().unwrap_or_default();
        assert!(messaggio.contains("pseudo-elemento"), "{messaggio}");
    }

    #[test]
    fn una_parte_sbagliata_per_assonanza_riceve_un_suggerimento() {
        let err = rifiuta(&minima(
            r##", "parts": { "section-cards": { "opacity": 1 } }"##,
        ));
        let messaggio = err.message().unwrap_or_default();
        assert!(messaggio.contains("«section-card»"), "{messaggio}");
    }

    #[test]
    fn le_parti_escono_in_ordine_di_registro() {
        let skin = parse_skin_json(&minima(
            r##", "parts": {
                "toast-card": { "opacity": 1 },
                "app-shell": { "opacity": 1 },
                "nav-pill": { "opacity": 1 }
            }"##,
        ))
        .expect("valida");
        let ordine: Vec<&str> = skin.parts.iter().map(|s| s.def.name).collect();
        // Alfabeticamente sarebbe app-shell, nav-pill, toast-card. Qui è
        // l'ordine del registro: cornice, navigazione, sovrapposizioni.
        assert_eq!(ordine, ["app-shell", "nav-pill", "toast-card"]);
    }

    #[test]
    fn i_token_obbligatori_mancanti_sono_un_avviso_non_un_errore() {
        let skin = parse_skin_json(&minima("")).expect("valida");
        let avvisi = check_skin(&skin);
        assert!(avvisi.len() > 10, "{avvisi:?}");
        assert!(
            avvisi
                .iter()
                .all(|a| a.kind == WarningKind::MissingRequiredToken)
        );
        assert!(avvisi.iter().any(|a| a.path == "tokens.color.surface.0"));
    }

    #[test]
    fn un_tema_chiaro_promesso_e_non_scritto_e_un_avviso() {
        let skin =
            parse_skin_json(&minima(r##", "capabilities": { "light": true }"##)).expect("valida");
        let avvisi = check_skin(&skin);
        assert!(
            avvisi
                .iter()
                .any(|a| a.kind == WarningKind::UnkeptCapability && a.path == "themes.light")
        );
    }

    #[test]
    fn una_superficie_troppo_costosa_e_un_avviso() {
        let skin = parse_skin_json(&minima(
            r##", "parts": { "glass-modal": {
                "background": [
                    { "effect": "blurBehind", "radius": "24px" },
                    { "effect": "blurBehind", "radius": "12px" }
                ]
            } }"##,
        ))
        .expect("valida");
        let avvisi = check_skin(&skin);
        assert!(
            avvisi
                .iter()
                .any(|a| a.kind == WarningKind::CostBudget && a.path == "parts.glass-modal")
        );
    }
}
