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

use crate::effects::{Corner, Effect, EffectTarget, Paint, RadialShape, Stop};
use crate::layout::{
    Align, GapStep, LayoutNode, LayoutZone, OptionKind, OptionValue, Spread, TrackSize, WidgetDef,
    WidgetInstance, WidgetOption, ZoneKind, default_shell,
};
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

/// Uno strato di pittura: `{"$pattern": "nome"}`, oppure un effetto per esteso.
///
/// # Perché la destinazione si controlla solo per i motivi
///
/// Un effetto scritto per esteso nel posto sbagliato — un `blurBehind` dentro
/// `background` — produce una dichiarazione CSS che il browser scarta: si vede
/// subito che non fa niente. Un **motivo** nel posto sbagliato produce invece un
/// nome di variabile che non esiste: `compila_motivi` scrive `--skin-x-filter` e
/// la parte chiederebbe `var(--skin-x)`, cioè un riferimento a un motivo
/// dichiarato che risolve nel nulla senza che nessuno lo dica. È l'unico dei due
/// casi in cui l'errore è invisibile, ed è per questo che è l'unico controllato.
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
        EffectTarget::Filter => "filtro",
    }
}

fn aspetto(value: &Value, path: &str, motivi: &[(String, Effect)]) -> Esito<PartAppearance> {
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
            "filter",
            "opacity",
            "letterSpacing",
            "textTransform",
            "fontWeight",
        ],
    )?;
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
        radius: lunghezza_facoltativa(map, path, "radius")?,
        clip: campo(map, "clip")
            .map(|v| pittura(v, &giu(path, "clip"), motivi, EffectTarget::ClipPath))
            .transpose()?,
        // Come `clip`, con l'altra destinazione: `pittura` controlla che un
        // `$pattern` messo qui sia dichiarato per il filtro e non per lo
        // sfondo, che è la stessa guardia e la stessa ragione.
        filter: campo(map, "filter")
            .map(|v| pittura(v, &giu(path, "filter"), motivi, EffectTarget::Filter))
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

fn stile_parte(
    nome: &str,
    value: &Value,
    path: &str,
    motivi: &[(String, Effect)],
) -> Esito<PartStyle> {
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
    let appearance = aspetto(&Value::Object(solo_aspetto), path, motivi)?;

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
                    motivi,
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
            let aspetto_stato = aspetto(valore, &giu(&dentro, stato.as_str()), motivi)?;
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
///
/// Non è `Eq`: `preview` sono colori, e l'opacità di un colore è un `f64`.
#[derive(Debug, Clone, PartialEq)]
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
    /// I tre colori della scheda nel selettore, scelti dall'autore.
    ///
    /// Senza, chi disegna il selettore deve indovinarli, e il modo ovvio di
    /// indovinare — fondo, superficie e accento — dà tre grigi quasi uguali per
    /// una skin sobria e sbaglia del tutto per una che tiene la sua identità in
    /// un colore della tavolozza. Sono **letterali** e non riferimenti a token:
    /// servono a mostrare la skin a chi non l'ha ancora scelta, cioè quando i
    /// suoi token non sono applicati a niente.
    pub preview: Option<[Rgba; 3]>,
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
///
/// I tre gradini sono **parole**, non misure, e i numeri che ci stanno dietro —
/// la scala `--spazio-1..5` — sono del motore e stanno in `stile.css`, non qui e
/// non nel compilatore. È la stessa ragione per cui la spaziatura non è un
/// token: una skin che potesse scriverne il valore potrebbe ridurre l'aria a
/// zero o gonfiarla finché qualcosa esce dallo schermo.
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
///
/// Non è più `Copy`: [`Self::shell`] è un albero, e un albero ha figli. Il
/// prezzo è una riga di `.clone()` dove prima non serviva; quel che si compra è
/// che una skin possa dire **dove** stanno le cose invece che solo di che
/// colore sono.
#[derive(Debug, Clone, PartialEq)]
pub struct SkinLayout {
    /// Il player.
    ///
    /// Non è geometria: sceglie una variante dell'albero di serie, e conta
    /// **solo** quando `shell` non è dichiarato.
    ///
    /// Da considerarsi **in uscita**. Ora che l'albero si dichiara, questa è una
    /// seconda maniera di dire la stessa cosa in modo meno preciso, e due
    /// maniere di dire la stessa cosa prima o poi si contraddicono. Toglierla è
    /// una rottura di formato, quindi è la prima voce vera di
    /// [`MIGRAZIONI`](crate::document): al formato 2 diventa uno `shell`
    /// scritto per esteso, e la tabella di migrazione esiste già vuota apposta.
    pub player: PlayerLayout,
    /// La barra laterale. Come `player`, in uscita per la stessa ragione.
    pub sidebar: SidebarLayout,
    /// La densità.
    ///
    /// Sopravvive alle altre due: non è geometria da mettere nell'albero, è la
    /// scala della spaziatura, e resta esprimibile in una parola sola.
    pub density: Density,
    /// L'albero: dove stanno le cose.
    ///
    /// Sempre popolato. Un documento che non lo dichiara riceve
    /// [`crate::layout::default_shell`] applicato alle due manopole qui sopra —
    /// così a valle non esiste il caso «manca lo scafale», né in Rust né in
    /// TypeScript.
    pub shell: LayoutZone,
    /// I nomi dei prefab dichiarati, in ordine.
    ///
    /// Solo i **nomi**: i corpi sono già espansi dentro `shell`, e tenerli
    /// anche qui vorrebbe dire avere due copie dello stesso sottoalbero che
    /// possono divergere. Servono a una cosa sola — dire quali sono dichiarati
    /// e mai usati — e per quella il nome basta.
    pub prefabs: Vec<String>,
}

impl Default for SkinLayout {
    fn default() -> Self {
        let player = PlayerLayout::Floating;
        let sidebar = SidebarLayout::Rail;
        Self {
            player,
            sidebar,
            density: Density::Comfortable,
            shell: default_shell(player, sidebar),
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
            "preview",
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
        preview: campo(map, "preview")
            .map(|v| anteprima(v, &giu(path, "preview")))
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

fn impaginazione(value: &Value, path: &str, problemi: &mut Vec<SkinIssue>) -> Esito<SkinLayout> {
    let map = oggetto(value, path)?;
    solo_chiavi(
        map,
        path,
        &["player", "sidebar", "density", "prefabs", "shell"],
    )?;
    let difetto = SkinLayout::default();
    let base = SkinLayout {
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
        // Provvisori: lo scafale vero e i prefab si leggono subito sotto,
        // quando si sa quali sono le due manopole da cui dipende la variante di
        // serie.
        shell: difetto.shell,
        prefabs: Vec::new(),
    };

    // I prefab si leggono **prima** dello scafale, come i motivi prima delle
    // parti: un riferimento si risolve solo se quel che riferisce esiste già.
    let prefabs = match campo(map, "prefabs") {
        None => Vec::new(),
        Some(grezzo) => modelli(grezzo, &giu(path, "prefabs"), problemi)?,
    };

    let Some(grezzo) = campo(map, "shell") else {
        return Ok(SkinLayout {
            shell: default_shell(base.player, base.sidebar),
            prefabs: prefabs.iter().map(|(n, _)| n.clone()).collect(),
            ..base
        });
    };

    let dentro = giu(path, "shell");
    let prima = problemi.len();
    let albero = zona_scafale(grezzo, &dentro, &prefabs, problemi)?;

    // Le regole si applicano soltanto se l'albero è arrivato intero. Un ramo
    // che non si è letto lascia un buco, e un buco produce «manca il fill», che
    // è vero dell'albero mutilato e non di quello scritto: manderebbe a
    // correggere una cosa che non c'è.
    if problemi.len() == prima {
        for guasto in crate::layout::verifica(&albero) {
            problemi.push(problema_scafale(&dentro, &guasto));
        }
    }

    Ok(SkinLayout {
        shell: albero,
        prefabs: prefabs.into_iter().map(|(n, _)| n).collect(),
        ..base
    })
}

/// I sottoalberi nominati che lo scafale può richiamare.
///
/// Un prefab è **un sottoalbero di widget già registrati**, e nient'altro:
/// niente parametri, niente condizioni, niente prefab dentro prefab. Ognuna
/// delle tre lo trasformerebbe in un linguaggio di programmazione, e la terza
/// renderebbe possibili i cicli — che senza una visita del grafo non si
/// riconoscono. Il corpo si legge con la tabella **vuota**, quindi
/// l'annidamento non è una regola da controllare: è inesprimibile.
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
        let mut corpo = zona_scafale(grezzo, &percorso, &[], problemi)?;
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
/// lo Studio consuma per portare il cursore sull'errore, e un indirizzo di
/// nodo non si sa dove sia in un file di testo.
fn problema_scafale(prefisso: &str, guasto: &crate::layout::ShellIssue) -> SkinIssue {
    // Un problema dentro un prefab si riporta al sito di **dichiarazione**: chi
    // lo corregge lo corregge una volta, invece che a ogni uso.
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
    // Un essenziale mancante ha un codice suo, perché è l'unico caso in cui chi
    // legge un log deve poter contare quante skin sono state rifiutate per
    // questo — e non è «il manifest è sbagliato», è «questa skin toglie l'app».
    SkinIssue {
        code: ErrorCode::SkinLayoutIncomplete {
            widget: widget.clone(),
        },
        path: percorso,
        message: guasto.message.clone(),
    }
}

fn misura_traccia(value: &Value, path: &str, difetto: TrackSize) -> Esito<TrackSize> {
    let scritto = testo(value, path)?;
    match scritto {
        "hug" => Ok(TrackSize::Hug),
        "fill" => Ok(TrackSize::Fill),
        _ => parse_length(scritto).map(TrackSize::Fixed).ok_or_else(|| {
            let _ = difetto;
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
            // Un conteggio si legge come intero e non come numero arrotondato:
            // «2.5 tasti» non è un valore che qualcuno abbia scritto per sbaglio
            // di battitura, è un valore che il formato non ha.
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
    solo_chiavi(map, path, &["widget", "size", "options"])?;
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
            .map(|v| misura_traccia(v, &giu(path, "size"), def.natural))
            .transpose()?
            .unwrap_or(def.natural),
        options: match campo(map, "options") {
            None => WidgetInstance::nuovo(def).options,
            Some(grezzo) => opzioni_widget(def, grezzo, &giu(path, "options"))?,
        },
    })
}

fn zona_scafale(
    value: &Value,
    path: &str,
    prefabs: &[(String, LayoutZone)],
    problemi: &mut Vec<SkinIssue>,
) -> Esito<LayoutZone> {
    let map = oggetto(value, path)?;
    solo_chiavi(
        map,
        path,
        &["zone", "size", "gap", "align", "spread", "part", "children"],
    )?;

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
            let Some(def) = crate::parts::part(nome) else {
                let candidati = crate::parts::nearest_parts(nome);
                return Err(problema(
                    &percorso,
                    format!("parte inesistente: «{nome}».{}", forse(&candidati)),
                ));
            };
            Some(def)
        }
    };

    let dentro = giu(path, "children");
    let voci = lista(richiesto(map, path, "children")?, &dentro)?;
    let mut figli = Vec::with_capacity(voci.len());
    for (indice, grezzo) in voci.iter().enumerate() {
        let percorso = giu(&dentro, &indice.to_string());
        // Un figlio malformato smette di scendere in **quel** ramo; i fratelli
        // proseguono. È la forma di `superfici()`, per la stessa ragione: chi
        // costruisce un layout corregge in una passata sola.
        match nodo_scafale(grezzo, &percorso, prefabs, problemi) {
            Ok(figlio) => figli.push(figlio),
            Err(guasto) => problemi.push(guasto),
        }
    }

    Ok(LayoutZone {
        kind,
        size: campo(map, "size")
            .map(|v| misura_traccia(v, &giu(path, "size"), TrackSize::Fill))
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
        // I prefab si espandono qui sotto, non qui: una zona scritta per esteso
        // non viene da nessun prefab, e `modelli()` marca la sua.
        from_prefab: None,
        children: figli,
    })
}

fn nodo_scafale(
    value: &Value,
    path: &str,
    prefabs: &[(String, LayoutZone)],
    problemi: &mut Vec<SkinIssue>,
) -> Esito<LayoutNode> {
    let map = oggetto(value, path)?;
    if campo(map, "widget").is_some() {
        return foglia_scafale(map, path).map(LayoutNode::Widget);
    }
    if campo(map, "zone").is_some() {
        return zona_scafale(value, path, prefabs, problemi).map(LayoutNode::Zone);
    }
    if campo(map, "prefab").is_some() {
        return modello_richiamato(map, path, prefabs).map(LayoutNode::Zone);
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
) -> Esito<LayoutZone> {
    solo_chiavi(map, path, &["prefab", "size"])?;
    let percorso = giu(path, "prefab");
    let nome = testo(richiesto(map, path, "prefab")?, &percorso)?;

    let Some((_, corpo)) = prefabs.iter().find(|(n, _)| n == nome) else {
        // La tabella vuota è il caso «dentro un prefab»: lì i prefab non
        // esistono affatto, e il messaggio deve dirlo invece di suggerire nomi
        // che in quel punto non sarebbero comunque richiamabili.
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

    Ok(LayoutZone {
        // La misura è l'unica cosa che il sito d'uso può dire: è **dove** sta il
        // sottoalbero, non cosa contiene. Tutto il resto verrebbe da un
        // parametro, e un prefab con parametri è una funzione.
        size: campo(map, "size")
            .map(|v| misura_traccia(v, &giu(path, "size"), corpo.size))
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

fn superfici(
    value: &Value,
    path: &str,
    motivi: &[(String, Effect)],
    problemi: &mut Vec<SkinIssue>,
) -> Esito<Vec<PartStyle>> {
    let map = oggetto(value, path)?;
    let mut stili = Vec::with_capacity(map.len());
    for (nome, grezzo) in map {
        match stile_parte(nome, grezzo, &giu(path, nome), motivi) {
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
        .map(|grezzo| impaginazione(grezzo, "layout", &mut problemi))
        .transpose()
        .map_err(solo)?;
    let patterns = match campo(map, "patterns") {
        None => Vec::new(),
        Some(grezzo) => motivi(grezzo, "patterns").map_err(solo)?,
    };
    let parts = match campo(map, "parts") {
        None => Vec::new(),
        Some(grezzo) => superfici(grezzo, "parts", &patterns, &mut problemi).map_err(solo)?,
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
    let Some(trovata) = versione_formato(raw) else {
        return Err(AppError::new(ErrorCode::SkinManifestInvalid {
            detail: Some("manca il campo «format»".to_owned()),
        }));
    };
    let nostra = u64::from(SKIN_FORMAT_VERSION);

    // Tre vie e non due. Più nuova: si rifiuta, ed è l'unica risposta onesta —
    // un documento che usa chiavi che non conosciamo non si legge a metà. Uguale:
    // passa. **Più vecchia: si migra e passa**, che è il ramo che oggi non ha
    // niente da fare e che esiste perché il giorno in cui ne avrà, ci sia già.
    if trovata > nostra {
        let found = u32::try_from(trovata).unwrap_or(u32::MAX);
        return Err(AppError::new(ErrorCode::SkinFormatUnsupported {
            found,
            supported: SKIN_FORMAT_VERSION,
        }));
    }
    if trovata < nostra {
        let migrato = migra(raw, trovata).map_err(|problemi| in_errore(&problemi))?;
        return documento(&migrato).map_err(|problemi| in_errore(&problemi));
    }

    documento(raw).map_err(|problemi| in_errore(&problemi))
}

// ── Le migrazioni ───────────────────────────────────────────────────────────

/// Un passo di migrazione: da una versione alla successiva.
///
/// Riscrive **il JSON**, non la forma interna, ed è l'unico posto possibile:
/// [`SkinDocument`] non è riserializzabile, quindi non esiste un modo di
/// tornare indietro da lì al testo. Una migrazione è quindi una funzione da
/// documento a documento, e la validazione gira una volta sola, alla fine, sul
/// risultato — così un difetto introdotto da una migrazione viene rifiutato
/// dalle stesse regole di tutto il resto invece che da nessuno.
struct Migrazione {
    /// La versione da cui parte. Produce sempre `da + 1`.
    da: u32,
    /// Cosa cambia.
    applica: fn(&mut Map<String, Value>) -> Esito<()>,
}

/// I passi di migrazione, in ordine di versione.
///
/// **Vuota, e non per pigrizia**: il formato 1 è il primo, e non esiste un
/// documento più vecchio da migrare. Questa tabella esiste perché il giorno in
/// cui arriva il secondo formato ci sia già un posto dove metterlo, un ordine in
/// cui applicarlo e un test che lo esercita — invece che una decisione da
/// prendere di fretta mentre si rompe qualcosa.
///
/// La prima voce vera è già prevedibile: la deprecazione di `layout.player` e
/// `layout.sidebar`, che al formato 2 diventeranno un `layout.shell` scritto per
/// esteso da [`crate::layout::default_shell`].
#[rustfmt::skip]
static MIGRAZIONI: &[Migrazione] = &[];

/// Porta un documento dalla sua versione alla nostra.
fn migra(raw: &Value, da: u64) -> Result<Value, Vec<SkinIssue>> {
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
    /// Un motivo dichiarato e mai riferito da nessuna parte.
    ///
    /// # Perché per un po' non era producibile
    ///
    /// Un motivo si compila in una proprietà custom (`--skin-<nome>`), e per
    /// diverse versioni il formato non aveva una forma per riferirla: una parte
    /// scriveva i suoi effetti per esteso e non poteva dire «usa il motivo
    /// `griglia`». Ogni motivo era quindi tecnicamente inutilizzato, e un avviso
    /// che scatta su tutti è rumore che insegna a ignorare gli avvisi — così la
    /// variante restava dichiarata e nessun ramo la produceva.
    ///
    /// Ora `{"$pattern": "griglia"}` è quella forma, e l'avviso dice la cosa che
    /// voleva dire dall'inizio: questo motivo si compila in una variabile CSS che
    /// nessuno legge.
    UnusedPattern,
    /// Un prefab dichiarato e mai richiamato.
    ///
    /// Stessa idea di [`Self::UnusedPattern`], sull'altro registro: un
    /// sottoalbero nominato che nessuno monta non fa niente e non si vede, e
    /// quasi sempre è il pezzo che si è dimenticato di collegare.
    UnusedPrefab,
    /// Una superficie che sfora il budget di costo.
    CostBudget,
    /// Una coppia testo/superficie sotto la soglia di leggibilità.
    Contrast,
}

/// La soglia di WCAG 2.1 per il testo normale.
pub const CONTRASTO_MINIMO: f64 = 4.5;

/// Una coppia misurata: quanto si legge questo colore su quello.
#[derive(Debug, Clone, PartialEq)]
pub struct ContrastPair {
    /// Il token davanti.
    pub foreground: &'static str,
    /// Il token dietro.
    pub background: &'static str,
    /// Il rapporto nel tema base.
    pub dark: f64,
    /// Il rapporto nel tema chiaro, quando la skin ne ha uno.
    pub light: Option<f64>,
}

impl ContrastPair {
    /// Passa in tutti i temi in cui è stata misurata.
    #[must_use]
    pub fn passa(&self) -> bool {
        self.dark >= CONTRASTO_MINIMO && self.light.is_none_or(|l| l >= CONTRASTO_MINIMO)
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
/// `surface.3` è deliberatamente fuori: è un riempimento di sopraelevazione —
/// il fondo di un elemento di menù al passaggio del mouse — non un letto di
/// testo semantico. Includerlo produrrebbe avvisi su coppie che nell'interfaccia
/// non si incontrano.
pub(crate) const DIETRO: &[&str] = &["color.surface.0", "color.surface.1", "color.surface.2"];

/// Il colore vero di un token, quando si può saperlo.
///
/// `None` quando dipende dalla copertina: `{ "$source": "albumArt.vibrant" }` è
/// un colore che al momento della validazione non esiste ancora, e misurarne il
/// contrasto vorrebbe dire inventarsi un disco.
///
/// I riferimenti fra token si seguono, con un tetto: `a → b → a` è un ciclo che
/// il formato non vieta, e senza il tetto questa funzione non tornerebbe.
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
            // Prima nell'insieme di questo tema, poi in quello base: è la stessa
            // cascata del CSS, e cercarne uno solo darebbe un contrasto giusto
            // per il tema sbagliato.
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

/// Misura ogni coppia testo/superficie, nei temi che la skin dichiara.
///
/// # Perché è pubblica e separata dagli avvisi
///
/// Perché lo Studio mostra **la tabella**, non solo le righe rosse: sapere che
/// `text.2` sta a 7.3 e `text.3` a 4.6 dice quanto margine c'è prima che un
/// ritocco al fondo rompa qualcosa. `check_skin` ne prende le righe che non
/// passano e le trasforma in avvisi; qui ci sono tutte.
///
/// Le coppie che non si possono misurare — un colore che segue la copertina, un
/// token non dichiarato — semplicemente non compaiono: un rapporto inventato
/// sarebbe peggio di un rapporto assente.
#[must_use]
pub fn contrast_pairs(skin: &SkinDocument) -> Vec<ContrastPair> {
    let mut coppie = Vec::new();
    for davanti in DAVANTI {
        for dietro in DIETRO {
            let Some(f) = colore_di(davanti, &skin.tokens, None, &skin.palette) else {
                continue;
            };
            let Some(b) = colore_di(dietro, &skin.tokens, None, &skin.palette) else {
                continue;
            };
            let chiaro = skin.light.as_ref().and_then(|light| {
                let f = colore_di(davanti, light, Some(&skin.tokens), &skin.palette)?;
                let b = colore_di(dietro, light, Some(&skin.tokens), &skin.palette)?;
                Some(crate::values::contrast_ratio(f, b))
            });
            coppie.push(ContrastPair {
                foreground: davanti,
                background: dietro,
                dark: crate::values::contrast_ratio(f, b),
                light: chiaro,
            });
        }
    }

    // E la regola inversa, che nessun'altra coppia copre: il testo che sta
    // **sopra** l'accento è `--color-surface-0`. Se l'accento si schiarisce,
    // l'etichetta di un bottone primario sparisce, e la tabella di sopra non se
    // ne accorgerebbe: lì `surface.0` è sempre un fondo, mai un davanti.
    if let (Some(f), Some(b)) = (
        colore_di("color.surface.0", &skin.tokens, None, &skin.palette),
        colore_di("color.accent", &skin.tokens, None, &skin.palette),
    ) {
        let chiaro = skin.light.as_ref().and_then(|light| {
            let f = colore_di("color.surface.0", light, Some(&skin.tokens), &skin.palette)?;
            let b = colore_di("color.accent", light, Some(&skin.tokens), &skin.palette)?;
            Some(crate::values::contrast_ratio(f, b))
        });
        coppie.push(ContrastPair {
            foreground: "color.surface.0",
            background: "color.accent",
            dark: crate::values::contrast_ratio(f, b),
            light: chiaro,
        });
    }

    coppie
}

/// Quante volte ogni colore della tavolozza è riferito.
///
/// # Perché un conteggio e non un avviso
///
/// È il numero che trasforma una tavolozza in un sistema: un colore usato una
/// volta sola non è un colore della skin, è un letterale con un nome. Ma non è
/// un errore, e trattarlo come tale insegnerebbe a ignorare gli avvisi. Lo
/// Studio lo mostra accanto al colore e lascia decidere.
#[must_use]
pub fn palette_usage(skin: &SkinDocument) -> Vec<(String, usize)> {
    let mut conteggi: Vec<(String, usize)> =
        skin.palette.iter().map(|(n, _)| (n.clone(), 0)).collect();

    let mut conta = |colore: &ColorValue| {
        if let ColorValue::Palette { name, .. } = colore {
            if let Some(voce) = conteggi.iter_mut().find(|(n, _)| n == name) {
                voce.1 += 1;
            }
        }
    };

    let mut in_token = |insieme: &TokenSet| {
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
    };
    in_token(&skin.tokens);
    if let Some(chiaro) = skin.light.as_ref() {
        in_token(chiaro);
    }
    if let Some(telefono) = skin.mobile.as_ref() {
        in_token(telefono);
    }

    for (_, effetto) in &skin.patterns {
        for colore in effetto.colors() {
            conta(colore);
        }
    }
    for stile in &skin.parts {
        for effetto in &stile.effects() {
            for colore in effetto.colors() {
                conta(colore);
            }
        }
        for aspetto in stile.appearances() {
            for colore in [aspetto.text_color.as_ref(), aspetto.border_color.as_ref()]
                .into_iter()
                .flatten()
            {
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

    // Il contrasto. `contrast_ratio()` esisteva da prima di questo ramo, con un
    // test che provava che `color.text.3` della skin di riferimento stava sotto
    // la soglia — e nessuno la chiamava, quindi quel test provava un difetto
    // invece di impedirlo. Misurare qui è ciò che trasforma un numero scritto a
    // occhio in un numero che qualcuno controlla.
    //
    // È un avviso e non un errore: una skin poco leggibile funziona, e chi la
    // scrive potrebbe averlo voluto per un'etichetta secondaria. Ma deve
    // saperlo, e deve saperlo **prima** di pubblicarla invece che dopo, da chi
    // la usa.
    for coppia in contrast_pairs(skin) {
        if coppia.passa() {
            continue;
        }
        let dove = if coppia.dark < CONTRASTO_MINIMO {
            format!("{:.2}:1 nel tema scuro", coppia.dark)
        } else {
            format!(
                "{:.2}:1 nel tema chiaro",
                coppia.light.unwrap_or(CONTRASTO_MINIMO)
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

    // Uno scafale che monta mezza applicazione non è illegale — ogni singolo
    // widget ci sta — ma è una finestra in cui non si trova più niente. Riusa
    // `CostBudget` invece di inventarsi una variante: è la stessa idea di
    // «troppa roba insieme», misurata su un altro budget.
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

        let costo = layout.shell.costo();
        if costo > crate::layout::SHELL_COST_BUDGET {
            avvisi.push(SkinWarning {
                kind: WarningKind::CostBudget,
                path: "layout.shell".to_owned(),
                message: format!(
                    "lo scafale monta {costo} sul budget di {}: \
                     tanti pezzi insieme sono una finestra in cui non si trova niente.",
                    crate::layout::SHELL_COST_BUDGET
                ),
            });
        }
    }

    // Un motivo che nessuna parte richiama si compila comunque: resta una
    // proprietà custom in `:root` che nessuno legge. Non rompe niente — ed è
    // esattamente il tipo di cosa che chi scrive la skin ha dimenticato di
    // collegare, non deciso di lasciare.
    let usati: Vec<&str> = skin
        .parts
        .iter()
        .flat_map(crate::parts::PartStyle::patterns_used)
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

    /// Un motivo dichiarato, più quel che il chiamante ci mette attorno.
    fn con_motivo(parti: &str) -> String {
        minima(&format!(
            r##", "patterns": {{
                "griglia": {{ "effect": "hairlineGrid", "color": "#fff", "cell": "36px" }},
                "taglio": {{ "effect": "chamfer", "size": "12px", "corners": ["topLeft"] }}
            }}, "parts": {{ {parti} }}"##
        ))
    }

    #[test]
    fn un_motivo_si_richiama_per_nome() {
        let skin = parse_skin_json(&con_motivo(
            r##""app-shell": { "background": [{ "$pattern": "griglia" }], "clip": { "$pattern": "taglio" } }"##,
        ))
        .expect("valida");
        let stile = &skin.parts[0];
        assert_eq!(
            stile.appearance.background[0].pattern_name(),
            Some("griglia")
        );
        assert_eq!(
            stile.appearance.clip.as_ref().and_then(Paint::pattern_name),
            Some("taglio")
        );
        // La definizione viaggia col nome: chi conta il costo non deve tenersi
        // la tabella dei motivi per sapere cosa sta contando.
        assert_eq!(stile.effects().len(), 2);
    }

    #[test]
    fn un_motivo_inesistente_riceve_un_suggerimento() {
        // La `s` di troppo, che è la stessa forma di sbaglio di `color.accents`:
        // `vicini()` riconosce la coda sbagliata, non la lettera cambiata in
        // mezzo. Vale per i motivi come per tutti gli altri registri.
        let err = rifiuta(&con_motivo(
            r##""app-shell": { "background": [{ "$pattern": "griglias" }] }"##,
        ));
        let messaggio = err.message().unwrap_or_default();
        assert!(messaggio.contains("motivo inesistente"), "{messaggio}");
        assert!(messaggio.contains("«griglia»"), "{messaggio}");
    }

    #[test]
    fn un_motivo_nel_posto_sbagliato_e_un_errore() {
        // È il caso che vale la pena controllare: un ritaglio dentro `background`
        // scriverebbe `var(--skin-taglio)` mentre il motivo si compila in
        // `--skin-taglio-clip`, cioè un riferimento a niente che non dà segno.
        for (dove, motivo) in [
            (r##""background": [{ "$pattern": "taglio" }]"##, "taglio"),
            (r##""clip": { "$pattern": "griglia" }"##, "griglia"),
        ] {
            let err = rifiuta(&con_motivo(&format!(r##""app-shell": {{ {dove} }}"##)));
            let messaggio = err.message().unwrap_or_default();
            assert!(messaggio.contains(motivo), "{messaggio}");
            assert!(messaggio.contains("qui ci va"), "{messaggio}");
        }
    }

    #[test]
    fn un_riferimento_a_motivo_non_porta_altre_chiavi() {
        // Metà riferimento e metà effetto non è una cosa: o si nomina un motivo,
        // o si scrive l'effetto per esteso.
        let err = rifiuta(&con_motivo(
            r##""app-shell": { "background": [{ "$pattern": "griglia", "color": "#fff" }] }"##,
        ));
        let messaggio = err.message().unwrap_or_default();
        assert!(messaggio.contains("color"), "{messaggio}");
    }

    #[test]
    fn un_motivo_mai_richiamato_e_un_avviso() {
        let skin = parse_skin_json(&con_motivo(
            r##""app-shell": { "background": [{ "$pattern": "griglia" }] }"##,
        ))
        .expect("valida");
        let avvisi = check_skin(&skin);
        let motivi: Vec<&str> = avvisi
            .iter()
            .filter(|a| a.kind == WarningKind::UnusedPattern)
            .map(|a| a.path.as_str())
            .collect();
        // `griglia` è richiamata, `taglio` no.
        assert_eq!(motivi, ["patterns.taglio"]);
    }

    #[test]
    fn un_motivo_richiamato_solo_in_uno_stato_e_richiamato() {
        // Stessa regola dei colori in `appearances()`: quel che si vede al
        // passaggio del mouse si vede eccome.
        let skin = parse_skin_json(&con_motivo(
            r##""app-shell": { "clip": { "$pattern": "taglio" },
                "states": { "hover": { "background": [{ "$pattern": "griglia" }] } } }"##,
        ))
        .expect("valida");
        assert!(
            !check_skin(&skin)
                .iter()
                .any(|a| a.kind == WarningKind::UnusedPattern)
        );
    }

    // ── Lo scafale ──────────────────────────────────────────────────────────

    /// Uno scafale non di serie, scritto per esteso. Non è spedita: sta in
    /// `tests/` e non in `skins/`, quindi resta fuori dall'elenco chiuso di
    /// `package.rs` e non può finire in un pacchetto per sbaglio.
    const SCAFALATURA: &str = include_str!("../tests/scafalatura.json");

    /// Una skin minima con questo scafale.
    fn con_scafale(shell: &str) -> String {
        minima(&format!(r##", "layout": {{ "shell": {shell} }}"##))
    }

    /// Uno scafale che passa, da guastare in un punto solo.
    const SANO: &str = r##"{
        "zone": "row",
        "children": [
            { "widget": "navigation" },
            { "widget": "content" },
            { "widget": "player" }
        ]
    }"##;

    #[test]
    fn chi_non_dichiara_uno_scafale_riceve_quello_di_serie() {
        // La skin di riferimento continua a esercitare il difetto: se l'albero
        // di serie cambia senza che nessuno lo decida, questo test lo dice.
        let skin = parse_skin_json(crate::PLAIN_SOURCE).expect("plain è valida");
        let layout = skin.layout.expect("plain dichiara layout");
        // `expanded` e non `rail`: la barra è sempre partita larga, e la skin di
        // riferimento diceva «rail» solo perché nessuno leggeva quel campo.
        // Adesso che lo legge qualcuno, il valore scritto deve essere quello
        // vero — è precisamente il tipo di bugia che questo lavoro chiude.
        assert_eq!(
            layout.shell,
            default_shell(PlayerLayout::Floating, SidebarLayout::Expanded)
        );
    }

    #[test]
    fn uno_scafale_scritto_per_esteso_si_legge() {
        let skin = parse_skin_json(SCAFALATURA).expect("valida");
        let layout = skin.layout.expect("dichiara layout");
        assert_eq!(layout.shell.kind, ZoneKind::Column);
        assert_eq!(layout.shell.part.map(|p| p.name), Some("app-shell"));
        assert_eq!(layout.density, Density::Compact);
        assert!(layout.shell.monta("queue"));
        // Le manopole dichiarate valgono, e le altre restano ai difetti.
        let trasporto = layout
            .shell
            .widgets()
            .into_iter()
            .find(|w| w.def.name == "transport")
            .expect("il trasporto");
        assert_eq!(trasporto.option("size"), Some(OptionValue::Word("column")));
        assert_eq!(trasporto.option("shuffle"), Some(OptionValue::Flag(false)));
        assert_eq!(trasporto.option("repeat"), Some(OptionValue::Flag(true)));
    }

    #[test]
    fn togliere_un_widget_essenziale_e_un_errore_col_suo_codice() {
        let err = rifiuta(&con_scafale(
            r##"{ "zone": "row", "children": [
                { "widget": "navigation" },
                { "widget": "player", "size": "fill" }
            ] }"##,
        ));
        assert_eq!(
            *err.code(),
            ErrorCode::SkinLayoutIncomplete {
                widget: "content".to_owned()
            }
        );
    }

    #[test]
    fn togliere_i_comandi_e_un_errore_che_elenca_le_alternative() {
        let err = rifiuta(&con_scafale(
            r##"{ "zone": "row", "children": [
                { "widget": "navigation" },
                { "widget": "content" }
            ] }"##,
        ));
        assert_eq!(
            *err.code(),
            ErrorCode::SkinLayoutIncomplete {
                widget: "playback".to_owned()
            }
        );
        let messaggio = err.message().unwrap_or_default();
        assert!(messaggio.contains("«player»"), "{messaggio}");
        assert!(messaggio.contains("«column»"), "{messaggio}");
    }

    #[test]
    fn un_widget_sconosciuto_riceve_un_suggerimento() {
        let err = rifiuta(&con_scafale(&SANO.replace("\"content\"", "\"contents\"")));
        let messaggio = err.message().unwrap_or_default();
        assert!(messaggio.contains("widget inesistente"), "{messaggio}");
        assert!(messaggio.contains("«content»"), "{messaggio}");
    }

    #[test]
    fn una_manopola_sconosciuta_riceve_un_suggerimento() {
        let err = rifiuta(&con_scafale(&SANO.replace(
            r##"{ "widget": "player" }"##,
            r##"{ "widget": "transport", "options": { "shuffles": true } }"##,
        )));
        let messaggio = err.message().unwrap_or_default();
        assert!(messaggio.contains("«shuffle»"), "{messaggio}");
    }

    #[test]
    fn una_parola_fuori_vocabolario_elenca_le_ammesse() {
        let err = rifiuta(&con_scafale(&SANO.replace(
            r##"{ "widget": "player" }"##,
            r##"{ "widget": "transport", "options": { "size": "gigante" } }"##,
        )));
        let messaggio = err.message().unwrap_or_default();
        assert!(messaggio.contains("large"), "{messaggio}");
    }

    #[test]
    fn una_misura_relativa_e_un_errore_col_percorso_giusto() {
        let err = rifiuta(&con_scafale(&SANO.replace(
            r##"{ "widget": "navigation" }"##,
            r##"{ "widget": "navigation", "size": "20%" }"##,
        )));
        let messaggio = err.message().unwrap_or_default();
        assert!(messaggio.contains("non esiste ancora"), "{messaggio}");
        // Il percorso è quello del JSON, non un indirizzo di nodo: è ciò che
        // porta il cursore dell'editor sulla riga giusta.
        assert!(messaggio.contains("layout.shell.children.0"), "{messaggio}");
    }

    #[test]
    fn un_nodo_che_non_e_ne_zona_ne_widget_e_un_errore() {
        let err = rifiuta(&con_scafale(
            r##"{ "zone": "row", "children": [{ "size": "fill" }] }"##,
        ));
        let messaggio = err.message().unwrap_or_default();
        assert!(messaggio.contains("nessuno dei tre"), "{messaggio}");
    }

    #[test]
    fn gli_errori_dello_scafale_arrivano_tutti_insieme() {
        // Due rami rotti, non uno: chi costruisce un layout corregge in una
        // passata sola, come per i token e per le parti.
        let err = rifiuta(&con_scafale(
            r##"{ "zone": "row", "children": [
                { "widget": "navigazione" },
                { "widget": "contenuto" },
                { "widget": "player" }
            ] }"##,
        ));
        let messaggio = err.message().unwrap_or_default();
        assert!(messaggio.contains("navigazione"), "{messaggio}");
        assert!(messaggio.contains("contenuto"), "{messaggio}");
    }

    /// Uno scafale che richiama un prefab, col prefab dichiarato.
    fn con_prefab(corpo: &str, uso: &str) -> String {
        minima(&format!(
            r##", "layout": {{
                "prefabs": {{ "barretta": {corpo} }},
                "shell": {{ "zone": "row", "children": [
                    {{ "widget": "navigation" }},
                    {{ "widget": "content" }},
                    {uso}
                ] }}
            }}"##
        ))
    }

    const BARRETTA: &str = r##"{
        "zone": "row",
        "size": "hug",
        "gap": "s",
        "part": "player-shell",
        "children": [
            { "widget": "now-playing" },
            { "widget": "scrubber", "size": "fill" },
            { "widget": "transport" }
        ]
    }"##;

    #[test]
    fn un_prefab_si_espande_dove_e_richiamato() {
        let skin = parse_skin_json(&con_prefab(BARRETTA, r##"{ "prefab": "barretta" }"##))
            .expect("valida");
        let shell = skin.layout.expect("dichiara layout").shell;
        // Da qui in giù l'albero è fatto solo di zone e widget: chi lo legge non
        // deve conoscere i prefab.
        assert!(shell.monta("transport"));
        assert!(shell.monta("scrubber"));
        assert_eq!(shell.prefabs_used(), ["barretta"]);
        let LayoutNode::Zone(espansa) = &shell.children[2] else {
            panic!("il prefab non si è espanso in una zona");
        };
        assert_eq!(espansa.part.map(|p| p.name), Some("player-shell"));
        assert_eq!(espansa.gap, GapStep::S);
    }

    #[test]
    fn il_sito_d_uso_puo_dire_solo_la_misura() {
        let skin = parse_skin_json(&con_prefab(
            BARRETTA,
            r##"{ "prefab": "barretta", "size": "200px" }"##,
        ))
        .expect("valida");
        let shell = skin.layout.expect("dichiara layout").shell;
        assert_eq!(
            shell.children[2].size(),
            TrackSize::Fixed(Length::px(200.0))
        );

        // Qualunque altra chiave sarebbe un parametro, e un prefab con parametri
        // è una funzione.
        let err = rifiuta(&con_prefab(
            BARRETTA,
            r##"{ "prefab": "barretta", "gap": "l" }"##,
        ));
        assert!(err.message().unwrap_or_default().contains("gap"));
    }

    #[test]
    fn un_prefab_non_puo_contenerne_un_altro() {
        let err = rifiuta(&con_prefab(
            r##"{ "zone": "row", "children": [{ "prefab": "barretta" }] }"##,
            r##"{ "widget": "player" }"##,
        ));
        let messaggio = err.message().unwrap_or_default();
        assert!(
            messaggio.contains("non può contenerne un altro"),
            "{messaggio}"
        );
    }

    #[test]
    fn un_prefab_inesistente_riceve_un_suggerimento() {
        let err = rifiuta(&con_prefab(BARRETTA, r##"{ "prefab": "barrettas" }"##));
        let messaggio = err.message().unwrap_or_default();
        assert!(messaggio.contains("prefab inesistente"), "{messaggio}");
        assert!(messaggio.contains("«barretta»"), "{messaggio}");
    }

    #[test]
    fn l_espansione_conserva_gli_essenziali() {
        // Il trasporto sta **dentro** il prefab: se l'espansione lo perdesse,
        // il gruppo «playback» resterebbe vuoto e la skin verrebbe rifiutata.
        parse_skin_json(&minima(&format!(
            r##", "layout": {{
                "prefabs": {{ "barretta": {BARRETTA} }},
                "shell": {{ "zone": "row", "children": [
                    {{ "widget": "navigation" }},
                    {{ "widget": "content" }},
                    {{ "prefab": "barretta" }}
                ] }}
            }}"##
        )))
        .expect("il trasporto dentro il prefab conta come montato");
    }

    #[test]
    fn un_errore_dentro_un_prefab_si_riporta_alla_dichiarazione() {
        // Due usi, un errore solo: chi lo corregge lo corregge una volta.
        let rotto = r##"{ "zone": "row", "size": "hug", "children": [
            { "widget": "now-playing" },
            { "widget": "transport" }
        ] }"##;
        let err = rifiuta(&minima(&format!(
            r##", "layout": {{
                "prefabs": {{ "barretta": {rotto} }},
                "shell": {{ "zone": "row", "children": [
                    {{ "widget": "navigation" }},
                    {{ "widget": "content" }},
                    {{ "prefab": "barretta" }},
                    {{ "prefab": "barretta" }}
                ] }}
            }}"##
        )));
        let messaggio = err.message().unwrap_or_default();
        assert!(messaggio.contains("layout.prefabs.barretta"), "{messaggio}");
        assert!(!messaggio.contains("layout.shell.children"), "{messaggio}");
        assert_eq!(messaggio.matches("nessun figlio").count(), 1, "{messaggio}");
    }

    #[test]
    fn un_prefab_mai_richiamato_e_un_avviso() {
        let skin = parse_skin_json(&con_prefab(BARRETTA, r##"{ "widget": "player" }"##))
            .expect("valida: è un avviso");
        let avvisi = check_skin(&skin);
        assert!(
            avvisi.iter().any(|a| a.kind == WarningKind::UnusedPrefab
                && a.path == "layout.prefabs.barretta"),
            "{avvisi:?}"
        );
    }

    #[test]
    fn uno_scafale_troppo_carico_e_un_avviso_non_un_errore() {
        let mut figli = vec![
            r##"{ "widget": "content" }"##.to_owned(),
            r##"{ "widget": "navigation" }"##.to_owned(),
        ];
        for _ in 0..24 {
            figli.push(r##"{ "widget": "transport" }"##.to_owned());
        }
        let skin = parse_skin_json(&con_scafale(&format!(
            r##"{{ "zone": "row", "children": [{}] }}"##,
            figli.join(",")
        )))
        .expect("valida: è un avviso");
        let avvisi = check_skin(&skin);
        assert!(
            avvisi
                .iter()
                .any(|a| a.kind == WarningKind::CostBudget && a.path == "layout.shell"),
            "{avvisi:?}"
        );
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
