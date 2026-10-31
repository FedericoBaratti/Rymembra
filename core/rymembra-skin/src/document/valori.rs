//! I valori di `skin.json` letti uno per uno: colori, lunghezze, durate,
//! curve, ombre, gli effetti e i token.

use rymembra_domain::errors::ErrorCode;
use serde_json::{Map, Value};

use super::{
    CHIAVI_FERMATA, CHIAVI_LIVELLO_OMBRA, Esito, SkinIssue, booleano, campo, chiavi_effetto, giu,
    lista_fra, numero, numero_fra, oggetto, parola, problema, richiesto, solo_chiavi, testo,
    token_non_valido,
};
use crate::effects::{Corner, Effect, ImageSize, RadialShape, Stop};
use crate::movimento::Curva;
use crate::parts::Ombra;
use crate::tokens::{
    ColorValue, DynamicSource, ShadowLayer, ShadowValue, TOKENS, TokenDef, TokenKind, TokenSet,
    TokenValue, is_local_name, token,
};
use crate::values::{
    Duration, Easing, EasingKeyword, Length, LengthUnit, LengthValue, Rgba, StepPosition,
    is_font_family, parse_color, parse_duration, parse_length,
};
use crate::vicini::{forse, vicini};

// ── I valori ────────────────────────────────────────────────────────────────

pub(super) fn colore_letterale(value: &Value, path: &str) -> Esito<Rgba> {
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
pub(super) fn colore(value: &Value, path: &str) -> Esito<ColorValue> {
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
        // Solo un colore, come fa `ombra_parte` con le ombre: fino alla 3.2.0 di prova un
        // `textColor` che richiamava `layout.rail` diventava `color: 68px`, non
        // valido al calcolo, e il testo ereditava il colore del genitore senza
        // che nessuno lo dicesse.
        let colori = || {
            TOKENS
                .iter()
                .filter(|def| def.kind == TokenKind::Color)
                .map(|def| def.id)
        };
        return match token(nome) {
            Some(def) if def.kind == TokenKind::Color => Ok(ColorValue::Token(def)),
            Some(_) => Err(problema(
                &percorso,
                format!(
                    "«{nome}» non è un token di colore: qui ci va un colore.{}",
                    forse(&vicini(nome, colori()))
                ),
            )),
            None => Err(problema(
                &percorso,
                format!(
                    "«{nome}» non è un token del registro.{}",
                    forse(&vicini(nome, colori()))
                ),
            )),
        };
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

pub(super) fn lunghezza(value: &Value, path: &str) -> Esito<Length> {
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

/// Una lunghezza che il CSS vuole positiva o zero: raggi, sfocature, spessori
/// del bordo, misure dei motivi.
///
/// `parse_length` legge il segno, e fino alla 3.2.0 di prova un `"radius": "-8px"` o una
/// sfocatura d'ombra di `-4px` passavano: il motore scarta la dichiarazione
/// intera, e la parte restava col raggio, l'ombra o il bordo del componente
/// senza che chi crea la skin sapesse quale valore non andava. Spostamenti,
/// espansione dell'ombra, distanza dell'anello e spaziatura delle lettere
/// restano liberi: lì il negativo è CSS valido, e le skin di serie lo usano.
pub(super) fn lunghezza_non_negativa(value: &Value, path: &str) -> Esito<Length> {
    let letta = lunghezza(value, path)?;
    if letta.value < 0.0 {
        return Err(problema(
            path,
            format!(
                "«{}» non può essere negativa: il motore scarterebbe la dichiarazione, e la \
                 parte resterebbe com'era",
                crate::values::format_length(letta)
            ),
        ));
    }
    Ok(letta)
}

/// Come [`lunghezza_non_negativa`], per un campo facoltativo.
pub(super) fn non_negativa_facoltativa(
    map: &Map<String, Value>,
    path: &str,
    name: &str,
) -> Esito<Option<Length>> {
    campo(map, name)
        .map(|v| lunghezza_non_negativa(v, &giu(path, name)))
        .transpose()
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
pub(super) fn misura_fra(value: &Value, path: &str, min: f64, max: f64) -> Esito<Length> {
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
pub(super) fn spostamento(value: &Value, path: &str) -> Esito<Length> {
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

pub(super) fn durata(value: &Value, path: &str) -> Esito<Duration> {
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

pub(super) fn easing(value: &Value, path: &str) -> Esito<Easing> {
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
pub(super) fn curva(value: &Value, path: &str, dichiarate: &[(String, Easing)]) -> Esito<Curva> {
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
            blur: lunghezza_non_negativa(
                richiesto(livello, &dentro, "blur")?,
                &giu(&dentro, "blur"),
            )?,
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
pub(super) fn ombra_parte(value: &Value, path: &str) -> Esito<Ombra> {
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
pub(super) fn risorsa(value: &Value, path: &str, mime_ammessi: &[&str]) -> Esito<String> {
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

pub(super) fn coppia(value: &Value, path: &str) -> Esito<(Length, Length)> {
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

pub(super) fn lunghezza_facoltativa(
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
    // Le misure di un motivo e il raggio del vetro: negative, il CSS le scarta.
    let misura_di = |name: &str| -> Esito<Length> {
        lunghezza_non_negativa(richiesto(map, path, name)?, &giu(path, name))
    };
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
            cell: misura_di("cell")?,
            cell_y: non_negativa_facoltativa(map, path, "cellY")?,
            thickness: non_negativa_facoltativa(map, path, "thickness")?,
        }),
        "scanlines" => Ok(Effect::Scanlines {
            color: colore_di("color")?,
            line: misura_di("line")?,
            gap: misura_di("gap")?,
        }),
        "stripes" => Ok(Effect::Stripes {
            angle: angolo(map, path, "angle", -45.0)?,
            color: colore_di("color")?,
            background: colore_di("background")?,
            width: misura_di("width")?,
        }),
        "dotGrid" => Ok(Effect::DotGrid {
            color: colore_di("color")?,
            spacing: misura_di("spacing")?,
            dot: misura_di("dot")?,
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
            radius: misura_di("radius")?,
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

/// Una sorgente della copertina scritta sul token che **è** la copertina.
///
/// Le sorgenti puntano tutte a `--accent` ([`DynamicSource::variabili`]):
/// `color.accent: { "$source": … }` diventava `--accent: var(--accent)`, un
/// ciclo che al valore calcolato non vale niente — e il runtime non lo
/// riscriveva mai, perché l'accento dinamico salta i token che seguono la
/// copertina. Bottone primario, cursori e anello del fuoco perdevano il colore
/// senza un avviso. L'accento segue la copertina per un'altra strada, e il
/// messaggio la nomina.
fn sorgente_su_se_stessa(def: &TokenDef, colore: &ColorValue, path: &str) -> Esito<()> {
    let ColorValue::Source { source, .. } = colore else {
        return Ok(());
    };
    let (pieno, tripla) = source.variabili();
    if def.css != pieno && def.rgb_triple != Some(tripla) {
        return Ok(());
    }
    Err(problema(
        &giu(path, "$source"),
        format!(
            "«{}» non può seguire la copertina con `$source`: la copertina arriva proprio \
             in «{}», e l'accento che rimanda a sé stesso resta senza colore. Scrivi qui il \
             colore di partenza, e lascia `capabilities.dynamicAccent` a true: è lui a far \
             seguire la copertina all'accento.",
            def.id, def.id
        ),
    ))
}

fn valore_token(def: &TokenDef, value: &Value, path: &str) -> Esito<TokenValue> {
    let esito = match def.kind {
        TokenKind::Color => colore(value, path)
            .and_then(|c| sorgente_su_se_stessa(def, &c, path).map(|()| c))
            .map(TokenValue::Color),
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

pub(super) fn insieme_token(
    value: &Value,
    path: &str,
    problemi: &mut Vec<SkinIssue>,
) -> Esito<TokenSet> {
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
