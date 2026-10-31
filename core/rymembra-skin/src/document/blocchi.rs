//! I blocchi del documento letti uno per uno — `meta`, `capabilities`, `fonts`,
//! `palette`, `motion`, `patterns`, `variant` — e i controlli che li incrociano.

use serde_json::Value;

use super::parti::{Richiamabili, nome_destinazione, superfici};
use super::valori::{
    colore, colore_letterale, coppia, curva, durata, easing, effetto, insieme_token, lunghezza,
    misura_fra, ombra_parte, risorsa, spostamento,
};
use super::{
    CHIAVI_ANIMAZIONE, CHIAVI_CAPACITA, CHIAVI_FONT, CHIAVI_FOTOGRAMMA, CHIAVI_FOTOGRAMMA_ROTTA,
    CHIAVI_META, CHIAVI_MOVIMENTO, CHIAVI_ROTTA, CHIAVI_VARIANTE, Esito, FontFace, MotionIntensity,
    Rotta, RouteFrame, SkinCapabilities, SkinIssue, SkinMeta, SkinMotion, Variante, booleano,
    campo, giu, identificatore, lista_fra, numero_fra, oggetto, parola, problema, richiesto,
    solo_chiavi, testo,
};
use crate::effects::{Effect, Paint};
use crate::movimento::{
    AnimDirection, AnimFrame, Animation, Iterazioni, MAX_ANIMAZIONI, MAX_DURATA_MS, MAX_FOTOGRAMMI,
    MAX_ITERAZIONI, MAX_RITARDO_MS, MIN_FOTOGRAMMI, Riempimento, Ritaglio,
};
use crate::parts::PartStyle;
use crate::tokens::{TokenSet, is_local_name};
use crate::values::{Duration, Easing, EasingKeyword, Length, LengthUnit, Rgba, is_font_family};
use crate::vicini::{forse, vicini};

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

pub(super) fn meta(value: &Value, path: &str) -> Esito<SkinMeta> {
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

pub(super) fn capacita(value: &Value, path: &str) -> Esito<SkinCapabilities> {
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
pub(super) fn caratteri(value: &Value, path: &str) -> Esito<Vec<FontFace>> {
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

pub(super) fn tavolozza(value: &Value, path: &str) -> Esito<Vec<(String, Rgba)>> {
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

pub(super) fn movimento(value: &Value, path: &str) -> Esito<SkinMotion> {
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

pub(super) fn motivi(value: &Value, path: &str) -> Esito<Vec<(String, Effect)>> {
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

pub(super) fn variante(
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

/// I motivi che nella variante passano da tinta a gradiente, o viceversa, e
/// stanno sopra un altro strato di un fondo.
///
/// In CSS una tinta piena sta **solo** nell'ultimo strato di `background`, e
/// il compilatore avvolge una tinta che non ci sta in `linear-gradient(c, c)`.
/// La scelta si fa una volta sola, sulla regola della parte, guardando il
/// motivo della base — e la variante ridefinisce soltanto la variabile del
/// motivo. [`variante`] controlla che la destinazione resti la stessa, e
/// `solid` e `linearGradient` sono tutti e due uno sfondo: fino alla 3.2.0 di prova un
/// motivo gradiente nella base e tinta nella variante dava, nel tema della
/// variante, un colore sopra un'immagine — una dichiarazione non valida, e il
/// fondo intero spariva senza un avviso; il caso inverso, un gradiente dentro
/// un gradiente. In fondo alla pila tutti e due vanno bene, ed è lì che si
/// lascia fare.
pub(super) fn tinte_che_cambiano(
    motivi: &[(String, Effect)],
    variante: &Variante,
    parti: &[PartStyle],
) -> Vec<SkinIssue> {
    let tinta = |effetto: &Effect| matches!(effetto, Effect::Solid { .. });
    let cambiati: Vec<(&str, bool)> = variante
        .patterns
        .iter()
        .filter_map(|(nome, nuovo)| {
            let (_, vecchio) = motivi.iter().find(|(n, _)| n == nome)?;
            (tinta(vecchio) != tinta(nuovo)).then_some((nome.as_str(), tinta(vecchio)))
        })
        .collect();
    if cambiati.is_empty() {
        return Vec::new();
    }

    let mut problemi = Vec::new();
    for (radice, elenco) in [
        ("parts", parti),
        ("variant.parts", variante.parts.as_slice()),
    ] {
        for stile in elenco {
            let parte = giu(radice, stile.def.name);
            let mut pile: Vec<(String, &[Paint])> =
                vec![(giu(&parte, "background"), &stile.appearance.background)];
            for (stato, aspetto) in stile.states.iter() {
                pile.push((
                    giu(&giu(&giu(&parte, "states"), stato.as_str()), "background"),
                    &aspetto.background,
                ));
            }
            for (nome, livello) in [("before", &stile.before), ("after", &stile.after)] {
                if let Some(livello) = livello {
                    pile.push((giu(&giu(&parte, nome), "background"), &livello.background));
                }
            }
            for (percorso, strati) in pile {
                let ultimo = strati.len().saturating_sub(1);
                for (posto, strato) in strati.iter().enumerate().take(ultimo) {
                    let Some(nome) = strato.pattern_name() else {
                        continue;
                    };
                    let Some((_, era_tinta)) = cambiati.iter().find(|(n, _)| *n == nome) else {
                        continue;
                    };
                    let (prima, dopo) = if *era_tinta {
                        ("una tinta", "un gradiente o un'immagine")
                    } else {
                        ("un gradiente o un'immagine", "una tinta")
                    };
                    problemi.push(problema(
                        &giu(&giu(&percorso, &posto.to_string()), "$pattern"),
                        format!(
                            "«{nome}» è {prima} nella skin e {dopo} nella variante, e qui sta \
                             sopra un altro strato: in CSS una tinta sta solo nell'ultimo \
                             strato di un fondo, e il foglio non può scrivere lo stesso strato \
                             nei due modi. Mettilo in fondo alla pila, o tienilo dello stesso \
                             genere nei due temi."
                        ),
                    ));
                }
            }
        }
    }
    problemi
}

/// I richiami a colori della tavolozza che la tavolozza non ha.
///
/// [`colore`] controlla l'alfabeto di un `$palette` e non può sapere se il
/// colore esiste: la tavolozza si legge prima, ma i colori stanno in una
/// ventina di posti — token della base, della variante e del telefono, livelli
/// d'ombra, motivi, parti con stati, livelli, anelli e filtri, fotogrammi — e
/// passarla a ognuno vorrebbe dire passarla dappertutto. Fino alla 3.2.0 di prova non lo
/// controllava nessuno: `var(--skin-color-cian)` non esiste, il testo ereditava
/// il colore del genitore, un fondo diventava trasparente e il contrasto
/// saltava la coppia, mentre motivi, animazioni e curve sbagliati si dicevano
/// con un suggerimento. Qui si scorre il documento scritto una volta sola, e
/// ogni `$palette` si confronta con la tavolozza della **base**: la variante
/// ridefinisce i colori che ci sono e non ne aggiunge. Un nome fuori
/// dall'alfabeto l'ha già detto [`colore`].
pub(super) fn tavolozza_mancante(
    value: &Value,
    path: &str,
    palette: &[(String, Rgba)],
    problemi: &mut Vec<SkinIssue>,
) {
    match value {
        Value::Object(map) => {
            if let Some(nome) = map.get("$palette").and_then(Value::as_str)
                && is_local_name(nome)
                && !palette.iter().any(|(n, _)| n == nome)
            {
                let candidati = vicini(nome, palette.iter().map(|(n, _)| n.as_str()));
                problemi.push(problema(
                    &giu(path, "$palette"),
                    format!(
                        "colore della tavolozza inesistente: «{nome}».{}",
                        forse(&candidati)
                    ),
                ));
            }
            for (chiave, dentro) in map {
                tavolozza_mancante(dentro, &giu(path, chiave), palette, problemi);
            }
        }
        Value::Array(voci) => {
            for (indice, dentro) in voci.iter().enumerate() {
                tavolozza_mancante(dentro, &giu(path, &indice.to_string()), palette, problemi);
            }
        }
        _ => {}
    }
}
