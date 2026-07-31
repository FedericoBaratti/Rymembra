//! Il compilatore: da documento validato a CSS.
//!
//! È l'**unico** autore di CSS di tutto il sistema. Non esiste un percorso per
//! cui una stringa scritta da chi crea la skin arrivi nell'output: i colori
//! entrano come canali, le lunghezze come numero più unità, e i nomi delle
//! proprietà vengono dal registro dei token e dal registro delle parti, non dal
//! documento. Quella proprietà è ciò che rende l'intera classe di attacchi via
//! CSS — `url()` che chiama a casa, `@import`, selettori che esfiltrano
//! attributi — non applicabile, e i test la verificano sull'output, non
//! sull'intenzione.
//!
//! # Perché non restituisce un `Result`
//!
//! Nel vecchio albero il compilatore era avvolto in un `try/catch` che
//! trasformava un'eccezione in un errore «la skin ha superato la validazione ma
//! non compila», con il commento che spiegava che sarebbe stato un buco fra
//! schema e compilatore. Qui quel buco non esiste: i tipi che entrano sono già
//! i tipi che escono — un colore è un `ColorValue`, un effetto è un `Effect`
//! chiuso — e ogni `match` è esaustivo. Non c'è un ramo in cui fallire, quindi
//! non c'è un `Result` da controllare e nessuno può ignorarlo.

use std::fmt::Write as _;

use crate::document::{RouteFrame, SkinDocument, SkinMotion};
use crate::effects::{Effect, EffectTarget, stack_cost};
use crate::parts::{PartAppearance, PartState, PartStyle};
use crate::tokens::{
    ColorValue, DynamicSource, ShadowValue, TokenDef, TokenKind, TokenSet, TokenValue,
};
use crate::values::{
    FontKind, Length, Rgba, format_color, format_duration, format_easing, format_font_stack,
    format_length, format_length_value, format_rgb_triple, num,
};

/// Il risultato della compilazione.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledSkin {
    /// L'identificatore della skin.
    pub id: String,
    /// Il foglio, pronto da adottare.
    pub css: String,
    /// Somma dei costi di motivi e parti, per il budget prestazionale.
    pub cost: u32,
    /// I token che seguono la copertina: il runtime deve aggiornarli.
    pub dynamic_tokens: Vec<&'static str>,
}

/// Il prefisso dei motivi. Evita collisioni fra skin e col registro dei token.
const PREFISSO_MOTIVO: &str = "--skin-";
/// Il prefisso dei colori della tavolozza locale.
const PREFISSO_TAVOLOZZA: &str = "--skin-color-";

/// La variante di selettore.
#[derive(Debug, Clone, Copy)]
enum Variante {
    Base,
    Chiaro,
    Telefono,
}

fn selettore(id: &str, variante: Variante) -> String {
    let radice = format!(":root[data-skin='{id}']");
    match variante {
        Variante::Base => radice,
        Variante::Chiaro => format!("{radice}[data-theme='light']"),
        Variante::Telefono => format!("{radice}[data-mobile]"),
    }
}

/// Una dichiarazione: proprietà e valore, entrambi già CSS.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Dichiarazione {
    property: String,
    value: String,
}

fn dichiara(property: impl Into<String>, value: impl Into<String>) -> Dichiarazione {
    Dichiarazione {
        property: property.into(),
        value: value.into(),
    }
}

fn blocco(selector: &str, dichiarazioni: &[Dichiarazione]) -> String {
    if dichiarazioni.is_empty() {
        return String::new();
    }
    let mut testo = format!("{selector} {{\n");
    for voce in dichiarazioni {
        let _ = writeln!(testo, "  {}: {};", voce.property, voce.value);
    }
    testo.push_str("}\n");
    testo
}

// ── I colori ────────────────────────────────────────────────────────────────

/// I due nomi con cui la copertina arriva nel foglio.
///
/// Nel vecchio albero questa scelta era un'espressione ternaria con i due rami
/// identici — `source === 'albumArt.vibrant' ? '--accent' : '--accent'` — che è
/// il modo in cui si scrive «qui andrà una distinzione» e poi non ci si torna.
/// Il `match` la rende una decisione dichiarata: oggi il runtime estrae **una**
/// tinta dalla copertina e la scrive in `--accent`, quindi tutte e quattro le
/// sorgenti puntano lì. Il giorno in cui ne estrarrà quattro, questo è l'unico
/// punto da cambiare, e il compilatore non lascia dimenticarne una.
const fn nomi_della_copertina(source: DynamicSource) -> (&'static str, &'static str) {
    match source {
        DynamicSource::Vibrant
        | DynamicSource::Muted
        | DynamicSource::DarkVibrant
        | DynamicSource::LightVibrant => ("--accent", "--accent-rgb"),
    }
}

fn risolvi_colore(value: &ColorValue) -> String {
    match *value {
        ColorValue::Literal(colore) => format_color(colore),
        // Il nome della variabile viene dal registro, non dal documento: è la
        // ragione per cui un riferimento non può diventare un canale di
        // iniezione.
        ColorValue::Token(def) => format!("var({})", def.css),
        ColorValue::Palette { ref name, alpha } => match alpha {
            None => format!("var({PREFISSO_TAVOLOZZA}{name})"),
            // Con un'opacità serve la tripla: `rgb(var(--x) / a)` funziona solo
            // se `--x` è una tripla di canali. Il compilatore emette entrambe le
            // forme per ogni colore della tavolozza proprio perché questo caso è
            // frequente — è come `cyberpunk` costruisce le varianti soft e glow
            // del suo teal.
            Some(a) => format!(
                "rgb(var({PREFISSO_TAVOLOZZA}{name}-rgb) / {})",
                crate::values::number(a, 3)
            ),
        },
        ColorValue::Source { source, alpha } => {
            let (pieno, tripla) = nomi_della_copertina(source);
            match alpha {
                None => format!("var({pieno})"),
                Some(a) => format!("rgb(var({tripla}) / {})", crate::values::number(a, 3)),
            }
        }
    }
}

const fn colore_letterale(value: &ColorValue) -> Option<Rgba> {
    match *value {
        ColorValue::Literal(colore) => Some(colore),
        _ => None,
    }
}

/// La tavolozza locale, con la tripla di ogni colore.
///
/// Entrambe le forme, sempre. Nel vecchio albero erano due dichiarazioni scritte
/// a mano per ogni colore locale — `--cyber-teal` e `--cyber-teal-rgb` — con la
/// stessa possibilità di divergere che avevano `--accent` e `--accent-rgb`.
fn compila_tavolozza(palette: &[(String, Rgba)]) -> Vec<Dichiarazione> {
    let mut dichiarazioni = Vec::with_capacity(palette.len() * 2);
    for (nome, colore) in palette {
        dichiarazioni.push(dichiara(
            format!("{PREFISSO_TAVOLOZZA}{nome}"),
            format_color(*colore),
        ));
        dichiarazioni.push(dichiara(
            format!("{PREFISSO_TAVOLOZZA}{nome}-rgb"),
            format_rgb_triple(*colore),
        ));
    }
    dichiarazioni
}

fn risolvi_ombra(value: &ShadowValue) -> String {
    if value.layers.is_empty() {
        return "none".to_owned();
    }
    let mut pezzi: Vec<String> = Vec::with_capacity(value.layers.len());
    for livello in &value.layers {
        let mut parti: Vec<String> = Vec::new();
        if livello.inset {
            parti.push("inset".to_owned());
        }
        parti.push(format_length(livello.x));
        parti.push(format_length(livello.y));
        parti.push(format_length(livello.blur));
        if let Some(spread) = livello.spread {
            parti.push(format_length(spread));
        }
        parti.push(risolvi_colore(&livello.color));
        pezzi.push(parti.join(" "));
    }
    pezzi.join(", ")
}

// ── Gli effetti ─────────────────────────────────────────────────────────────

fn fermate(stops: &[crate::effects::Stop]) -> String {
    stops
        .iter()
        .map(|stop| match stop.at {
            None => risolvi_colore(&stop.color),
            Some(at) => format!("{} {}", risolvi_colore(&stop.color), format_length(at)),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn posizione(at: Option<(Length, Length)>) -> String {
    match at {
        None => String::new(),
        Some((x, y)) => format!(" at {} {}", format_length(x), format_length(y)),
    }
}

/// Un effetto, come valore CSS. `target()` dice in quale proprietà va.
#[must_use]
pub fn compile_effect(effect: &Effect) -> String {
    match *effect {
        Effect::Solid { ref color } => risolvi_colore(color),

        Effect::LinearGradient { angle, ref stops } => {
            format!("linear-gradient({}deg, {})", num(angle), fermate(stops))
        }

        Effect::RadialGradient {
            shape,
            at,
            size,
            ref stops,
        } => {
            let misura = size.map(format_length).unwrap_or_default();
            let misura = if misura.is_empty() {
                String::new()
            } else {
                format!(" {misura}")
            };
            format!(
                "radial-gradient({}{misura}{}, {})",
                shape.as_str(),
                posizione(at),
                fermate(stops)
            )
        }

        Effect::ConicGradient {
            from,
            at,
            ref stops,
        } => format!(
            "conic-gradient(from {}deg{}, {})",
            num(from),
            posizione(at),
            fermate(stops)
        ),

        Effect::HairlineGrid {
            ref color,
            cell,
            cell_y,
            thickness,
        } => {
            // Le due righe che nel vecchio albero erano copiate a mano, col
            // passo ripetuto — ed è nella copia che la variante chiara di
            // cyberpunk scrive `36px 36px` dove le altre scrivono `100% 36px`.
            let colore = risolvi_colore(color);
            let spessore = format_length(thickness.unwrap_or(Length::px(1.0)));
            format!(
                "linear-gradient({colore} {spessore}, transparent {spessore}) 0 0 / 100% {}, \
                 linear-gradient(90deg, {colore} {spessore}, transparent {spessore}) 0 0 / {} 100%",
                format_length(cell_y.unwrap_or(cell)),
                format_length(cell)
            )
        }

        Effect::Scanlines {
            ref color,
            line,
            gap,
        } => format!(
            "repeating-linear-gradient(0deg, {} 0 {}, transparent {} {})",
            risolvi_colore(color),
            format_length(line),
            format_length(line),
            format_length(gap)
        ),

        Effect::Stripes {
            angle,
            ref color,
            ref background,
            width,
        } => {
            let doppia = Length {
                value: width.value * 2.0,
                unit: width.unit,
            };
            format!(
                "repeating-linear-gradient({}deg, {} 0 {}, {} {} {})",
                num(angle),
                risolvi_colore(color),
                format_length(width),
                risolvi_colore(background),
                format_length(width),
                format_length(doppia)
            )
        }

        Effect::DotGrid {
            ref color,
            spacing,
            dot,
        } => format!(
            "radial-gradient(circle at center, {} 0 {}, transparent {}) 0 0 / {} {}",
            risolvi_colore(color),
            format_length(dot),
            format_length(dot),
            format_length(spacing),
            format_length(spacing)
        ),

        Effect::Vignette { ref color, start } => format!(
            "radial-gradient(ellipse at center, transparent {}%, {} 100%)",
            num(start),
            risolvi_colore(color)
        ),

        Effect::Chamfer { size, ref corners } => {
            // Il poligono che nel vecchio albero erano due token da tenere
            // coerenti a mano: la misura in `--cyber-cut`, la forma in
            // `--cyber-chamfer`.
            use crate::effects::Corner;
            let misura = format_length(size);
            let taglia = |angolo: Corner| corners.contains(&angolo);
            let mut punti: Vec<String> = Vec::with_capacity(8);
            punti.push(if taglia(Corner::TopLeft) {
                format!("0 {misura}")
            } else {
                "0 0".to_owned()
            });
            if taglia(Corner::TopLeft) {
                punti.push(format!("{misura} 0"));
            }
            punti.push(if taglia(Corner::TopRight) {
                format!("calc(100% - {misura}) 0")
            } else {
                "100% 0".to_owned()
            });
            if taglia(Corner::TopRight) {
                punti.push(format!("100% {misura}"));
            }
            punti.push(if taglia(Corner::BottomRight) {
                format!("100% calc(100% - {misura})")
            } else {
                "100% 100%".to_owned()
            });
            if taglia(Corner::BottomRight) {
                punti.push(format!("calc(100% - {misura}) 100%"));
            }
            punti.push(if taglia(Corner::BottomLeft) {
                format!("{misura} 100%")
            } else {
                "0 100%".to_owned()
            });
            if taglia(Corner::BottomLeft) {
                punti.push(format!("0 calc(100% - {misura})"));
            }
            format!("polygon({})", punti.join(", "))
        }

        Effect::BlurBehind { radius, saturate } => match saturate {
            None => format!("blur({})", format_length(radius)),
            Some(percentuale) => format!(
                "blur({}) saturate({}%)",
                format_length(radius),
                num(percentuale)
            ),
        },
    }
}

// ── I token ─────────────────────────────────────────────────────────────────

fn compila_token(tokens: &TokenSet) -> (Vec<Dichiarazione>, Vec<&'static str>) {
    let mut dichiarazioni = Vec::with_capacity(tokens.len() * 2);
    let mut dinamici = Vec::new();

    for (def, value) in tokens.iter() {
        match *value {
            TokenValue::Color(ref color) => {
                if matches!(*color, ColorValue::Source { .. }) {
                    dinamici.push(def.id);
                }
                compila_colore(def, color, &mut dichiarazioni);
            }
            TokenValue::Length(length) => {
                dichiarazioni.push(dichiara(def.css, format_length_value(length)));
            }
            TokenValue::Duration(duration) => {
                dichiarazioni.push(dichiara(def.css, format_duration(duration)));
            }
            TokenValue::Easing(easing) => {
                dichiarazioni.push(dichiara(def.css, format_easing(easing)));
            }
            TokenValue::Number(number) => {
                dichiarazioni.push(dichiara(def.css, num(number)));
            }
            TokenValue::FontStack(ref families) => {
                let genere = match def.id {
                    "font.mono" => FontKind::Mono,
                    "font.display" => FontKind::Display,
                    _ => FontKind::Sans,
                };
                dichiarazioni.push(dichiara(def.css, format_font_stack(families, genere)));
            }
            TokenValue::Shadow(ref shadow) => {
                dichiarazioni.push(dichiara(def.css, risolvi_ombra(shadow)));
            }
        }
    }

    (dichiarazioni, dinamici)
}

fn compila_colore(def: &TokenDef, color: &ColorValue, dichiarazioni: &mut Vec<Dichiarazione>) {
    // La tripla derivata dal colore. Un riferimento o una sorgente non hanno un
    // valore qui: si rimanda alla tripla dell'accento, che è quella che il
    // runtime aggiorna.
    let tripla = |color: &ColorValue| {
        colore_letterale(color).map_or_else(|| "var(--accent-rgb)".to_owned(), format_rgb_triple)
    };

    // `color.hero` esiste **solo** come tripla: emettere `--hero-rgb: rgb(…)`
    // romperebbe tutte le `rgba()` che la usano.
    if def.rgb_triple == Some(def.css) {
        dichiarazioni.push(dichiara(def.css, tripla(color)));
        return;
    }

    dichiarazioni.push(dichiara(def.css, risolvi_colore(color)));
    if let Some(nome) = def.rgb_triple {
        // Non può divergere dal colore, perché è lo stesso dato. È la correzione
        // del commento «DEVE combaciare con surface-0».
        dichiarazioni.push(dichiara(nome, tripla(color)));
    }
}

/// I token **calcolati**.
///
/// Non sono scelte di stile, sono conseguenze, e una skin non deve poterle
/// contraddire: `--shell-left` è la larghezza del rail, `--player-clearance`
/// l'altezza del player più due volte il suo margine, le due `--transition-*`
/// una durata più una curva. Nel vecchio albero stavano in `:root` insieme a
/// tutto il resto, quindi una skin poteva sovrascriverle con valori incoerenti
/// e la shell si sfasava.
fn calcolati(tokens: &TokenSet) -> Vec<Dichiarazione> {
    let mut dichiarazioni = Vec::new();

    if tokens.contains("layout.rail") {
        dichiarazioni.push(dichiara("--shell-left", "var(--rail-w)"));
    }
    if tokens.contains("layout.playerHeight") || tokens.contains("layout.playerGap") {
        dichiarazioni.push(dichiara(
            "--player-clearance",
            "calc(var(--player-h) + var(--player-gap) * 2)",
        ));
    }

    // La tripla di surface-0. Nel vecchio albero `cyberpunk` dichiarava
    // `--cyber-fog-rgb: 6 6 8` con accanto un commento in maiuscolo: «DEVE
    // combaciare con surface-0». Un'invariante affidata a un commento è
    // un'invariante che prima o poi si rompe — basta ritoccare la superficie e
    // dimenticare la nebbia, e il pavimento prospettico sfuma verso un colore
    // che non è il fondo.
    if let Some(TokenValue::Color(color)) = tokens.get("color.surface.0")
        && let Some(colore) = colore_letterale(color)
    {
        dichiarazioni.push(dichiara("--surface-0-rgb", format_rgb_triple(colore)));
    }

    if tokens.contains("motion.dur.1") {
        dichiarazioni.push(dichiara(
            "--transition-fast",
            "var(--dur-1) var(--ease-out-expo)",
        ));
    }
    if tokens.contains("motion.dur.2") {
        dichiarazioni.push(dichiara(
            "--transition-med",
            "var(--dur-2) var(--ease-out-expo)",
        ));
    }

    dichiarazioni
}

// ── I motivi ────────────────────────────────────────────────────────────────

fn compila_motivi(patterns: &[(String, Effect)]) -> (Vec<Dichiarazione>, u32) {
    let mut dichiarazioni = Vec::with_capacity(patterns.len());
    let mut effetti = Vec::with_capacity(patterns.len());

    for (nome, effetto) in patterns {
        effetti.push(effetto.clone());
        // Il suffisso dice in quale proprietà va usato: un clip-path e uno
        // sfondo non si scambiano, e il nome lo rende evidente a chi scrive le
        // parti.
        let suffisso = match effetto.target() {
            EffectTarget::Background => "",
            EffectTarget::ClipPath => "-clip",
            EffectTarget::Filter => "-filter",
        };
        dichiarazioni.push(dichiara(
            format!("{PREFISSO_MOTIVO}{nome}{suffisso}"),
            compile_effect(effetto),
        ));
    }

    (dichiarazioni, stack_cost(&effetti))
}

// ── Le parti ────────────────────────────────────────────────────────────────

fn compila_aspetto(source: &PartAppearance) -> Vec<Dichiarazione> {
    let mut dichiarazioni = Vec::new();

    if !source.background.is_empty() {
        dichiarazioni.push(dichiara(
            "background",
            source
                .background
                .iter()
                .map(compile_effect)
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    if let Some(color) = source.text_color.as_ref() {
        dichiarazioni.push(dichiara("color", risolvi_colore(color)));
    }
    if source.border_color.is_some() || source.border_width.is_some() {
        let spessore = source
            .border_width
            .map_or_else(|| "1px".to_owned(), format_length);
        let colore = source
            .border_color
            .as_ref()
            .map_or_else(|| "var(--hairline)".to_owned(), risolvi_colore);
        dichiarazioni.push(dichiara("border", format!("{spessore} solid {colore}")));
    }
    if let Some(radius) = source.radius {
        dichiarazioni.push(dichiara("border-radius", format_length(radius)));
    }
    if let Some(clip) = source.clip.as_ref() {
        dichiarazioni.push(dichiara("clip-path", compile_effect(clip)));
    }
    if let Some(opacity) = source.opacity {
        dichiarazioni.push(dichiara("opacity", num(opacity)));
    }
    if let Some(spacing) = source.letter_spacing {
        dichiarazioni.push(dichiara("letter-spacing", format_length(spacing)));
    }
    if let Some(transform) = source.text_transform {
        dichiarazioni.push(dichiara("text-transform", transform.as_str()));
    }
    if let Some(weight) = source.font_weight {
        dichiarazioni.push(dichiara("font-weight", num(weight.round())));
    }

    dichiarazioni
}

/// Le regole CSS di una parte.
///
/// Il selettore include sempre `[data-skin='<id>']`, quindi una parte
/// ridisegnata da una skin non può influenzare un'altra skin: è la stessa
/// proprietà che il compilatore dei token garantisce, estesa alle parti.
fn compila_parte(id: &str, stile: &PartStyle) -> String {
    let base = format!(":root[data-skin='{id}'] .{}", stile.def.name);
    let mut css = blocco(&base, &compila_aspetto(&stile.appearance));

    if let Some(layer) = stile.layer.as_ref() {
        // Lo pseudo-elemento ha bisogno di `content` e di essere posizionato, ma
        // **non** di poter essere spostato dalla skin: quelle proprietà le mette
        // il compilatore, sempre uguali.
        let mut dichiarazioni = vec![
            dichiara("content", "\"\""),
            dichiara("position", "absolute"),
            dichiara("inset", "0"),
            dichiara("pointer-events", "none"),
            dichiara(
                "background",
                layer
                    .background
                    .iter()
                    .map(compile_effect)
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        ];
        if let Some(opacity) = layer.opacity {
            dichiarazioni.push(dichiara("opacity", num(opacity)));
        }
        css.push_str(&blocco(&format!("{base}::after"), &dichiarazioni));
    }

    for stato in PartState::ALL {
        let Some(aspetto) = stile.states.get(*stato) else {
            continue;
        };
        // `:where()` mantiene la specificità del selettore di stato uguale a
        // quella della parte: senza, uno stato dichiarato da una skin
        // vincerebbe su una regola che il componente considera più importante.
        css.push_str(&blocco(
            &format!("{base}:where({})", stato.selector()),
            &compila_aspetto(aspetto),
        ));
    }

    css
}

// ── Il movimento ────────────────────────────────────────────────────────────

fn fotogramma(nome: &str, quando: &str, frame: RouteFrame) -> String {
    let mut dichiarazioni: Vec<String> = Vec::new();
    if let Some(opacity) = frame.opacity {
        dichiarazioni.push(format!("    opacity: {};", num(opacity)));
    }
    let mut trasformazioni: Vec<String> = Vec::new();
    if let Some(y) = frame.translate_y {
        trasformazioni.push(format!("translateY({}px)", num(y)));
    }
    if let Some(scale) = frame.scale {
        trasformazioni.push(format!("scale({})", num(scale)));
    }
    if !trasformazioni.is_empty() {
        dichiarazioni.push(format!("    transform: {};", trasformazioni.join(" ")));
    }
    if dichiarazioni.is_empty() {
        return String::new();
    }
    format!(
        "@keyframes {nome} {{\n  {quando} {{\n{}\n  }}\n}}\n",
        dichiarazioni.join("\n")
    )
}

fn compila_movimento(id: &str, motion: &SkinMotion) -> String {
    let mut dichiarazioni = vec![
        dichiara("--motion-intensity", motion.intensity.as_str()),
        dichiara("--motion-scale", num(motion.intensity.scale())),
    ];
    for (nome, curva) in &motion.easings {
        dichiarazioni.push(dichiara(
            format!("{PREFISSO_MOTIVO}ease-{nome}"),
            format_easing(*curva),
        ));
    }

    let mut css = blocco(&selettore(id, Variante::Base), &dichiarazioni);

    let Some(transizione) = motion.route_transition else {
        return css;
    };
    // Solo `transform` e `opacity`: è il vincolo che il compilatore impone, e
    // non una raccomandazione scritta in un documento che nessuno rilegge.
    if let Some(uscita) = transizione.out.filter(|f| !f.is_empty()) {
        let _ = write!(
            css,
            ":root[data-skin='{id}']::view-transition-old(root) {{\n  animation: skin-{id}-out var(--dur-2) var(--ease-out-expo) both;\n}}\n"
        );
        css.push_str(&fotogramma(&format!("skin-{id}-out"), "to", uscita));
    }
    if let Some(entrata) = transizione.enter.filter(|f| !f.is_empty()) {
        let _ = write!(
            css,
            ":root[data-skin='{id}']::view-transition-new(root) {{\n  animation: skin-{id}-in var(--dur-2) var(--ease-out-expo) both;\n}}\n"
        );
        css.push_str(&fotogramma(&format!("skin-{id}-in"), "from", entrata));
    }

    css
}

// ── L'insieme ───────────────────────────────────────────────────────────────

/// Compila una skin validata.
#[must_use]
pub fn compile_skin(skin: &SkinDocument) -> CompiledSkin {
    let (base, dinamici) = compila_token(&skin.tokens);
    let (motivi, costo_motivi) = compila_motivi(&skin.patterns);

    let mut dichiarazioni = compila_tavolozza(&skin.palette);
    dichiarazioni.extend(base);
    dichiarazioni.extend(calcolati(&skin.tokens));
    dichiarazioni.extend(motivi);
    // `color-scheme` non è un token: è ciò che dice al motore di rendering come
    // disegnare le barre di scorrimento e i controlli nativi. Nel vecchio albero
    // stava scritto a mano in ogni skin, e dimenticarlo dava scrollbar chiare su
    // fondo nero.
    dichiarazioni.push(dichiara("color-scheme", "dark"));

    let mut css = format!(
        "/* {} {} — generato, non modificare a mano */\n",
        skin.meta.name, skin.meta.version
    );
    css.push_str(&blocco(
        &selettore(&skin.id, Variante::Base),
        &dichiarazioni,
    ));

    if let Some(chiaro) = skin.light.as_ref() {
        let (mut dichiarazioni, _) = compila_token(chiaro);
        dichiarazioni.extend(calcolati(chiaro));
        dichiarazioni.push(dichiara("color-scheme", "light"));
        css.push_str(&blocco(
            &selettore(&skin.id, Variante::Chiaro),
            &dichiarazioni,
        ));
    }

    if let Some(telefono) = skin.mobile.as_ref() {
        let (mut dichiarazioni, _) = compila_token(telefono);
        dichiarazioni.extend(calcolati(telefono));
        css.push_str(&blocco(
            &selettore(&skin.id, Variante::Telefono),
            &dichiarazioni,
        ));
    }

    if let Some(motion) = skin.motion.as_ref() {
        css.push_str(&compila_movimento(&skin.id, motion));
    }

    let mut costo_parti = 0;
    for stile in &skin.parts {
        costo_parti += stack_cost(&stile.effects());
        css.push_str(&compila_parte(&skin.id, stile));
    }

    CompiledSkin {
        id: skin.id.clone(),
        css,
        // Il costo somma motivi e parti: è la cifra che il budget confronta, e
        // sommarne solo una metà la renderebbe inutile.
        cost: costo_motivi + costo_parti,
        dynamic_tokens: dinamici,
    }
}

/// I token di una skin, per l'editor e per il confronto fra librerie.
#[must_use]
pub fn declared_tokens(skin: &SkinDocument) -> Vec<&'static str> {
    skin.tokens.iter().map(|(def, _)| def.id).collect()
}

/// Il tipo del token, per chi costruisce un pannello dai dati.
#[must_use]
pub const fn token_kind(def: &TokenDef) -> TokenKind {
    def.kind
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::parse_skin_json;

    const MINIMA: &str = r##"{
      "format": 1,
      "id": "prova",
      "meta": { "name": "Prova", "author": "Aether", "version": "1.0.0" },
      "tokens": {}
    }"##;

    fn compila(tokens: &str) -> String {
        let json = MINIMA.replace("\"tokens\": {}", &format!("\"tokens\": {tokens}"));
        let skin = parse_skin_json(&json).expect("valida");
        compile_skin(&skin).css
    }

    fn compila_documento(json: &str) -> CompiledSkin {
        let skin = parse_skin_json(json).expect("valida");
        compile_skin(&skin)
    }

    #[test]
    fn la_tripla_si_deriva_dal_colore() {
        let css = compila(r##"{ "color.accent": "#8b7cf6" }"##);
        assert!(css.contains("--accent: rgb(139 124 246);"), "{css}");
        assert!(css.contains("--accent-rgb: 139 124 246;"), "{css}");
    }

    #[test]
    fn la_nebbia_non_puo_piu_scollarsi_dal_fondo() {
        // Il commento in maiuscolo «DEVE combaciare con surface-0», sostituito
        // da una derivazione.
        let css = compila(r##"{ "color.surface.0": "#060608" }"##);
        assert!(css.contains("--color-surface-0: rgb(6 6 8);"), "{css}");
        assert!(css.contains("--surface-0-rgb: 6 6 8;"), "{css}");
    }

    #[test]
    fn hero_esce_solo_come_tripla() {
        // `--hero-rgb` finisce dentro `rgba()` calcolate a runtime: emetterlo
        // come `rgb(...)` le romperebbe tutte.
        let css = compila(r##"{ "color.hero": "#8b7cf6" }"##);
        assert!(css.contains("--hero-rgb: 139 124 246;"), "{css}");
        assert!(!css.contains("--hero-rgb: rgb("), "{css}");
    }

    #[test]
    fn i_calcolati_non_si_dichiarano() {
        let css = compila(
            r##"{ "layout.rail": "68px", "layout.playerHeight": "92px", "layout.playerGap": "14px", "motion.dur.1": "150ms" }"##,
        );
        assert!(css.contains("--shell-left: var(--rail-w);"), "{css}");
        assert!(
            css.contains("--player-clearance: calc(var(--player-h) + var(--player-gap) * 2);"),
            "{css}"
        );
        assert!(
            css.contains("--transition-fast: var(--dur-1) var(--ease-out-expo);"),
            "{css}"
        );
        // E non compaiono se la skin non dichiara ciò da cui derivano.
        let vuota = compila("{}");
        assert!(!vuota.contains("--shell-left"), "{vuota}");
    }

    #[test]
    fn il_color_scheme_non_si_dimentica() {
        // Dimenticarlo dava barre di scorrimento chiare su fondo nero, e nel
        // vecchio albero era scritto a mano in ogni skin.
        let css = compila("{}");
        assert!(css.contains("color-scheme: dark;"), "{css}");
    }

    #[test]
    fn niente_del_documento_finisce_nel_foglio_come_testo() {
        // La proprietà che tiene in piedi tutto il formato, verificata
        // sull'output: nessun canale in cui infilare CSS arbitrario.
        let compilata = compila_documento(
            r##"{
              "format": 1,
              "id": "prova",
              "meta": { "name": "Prova", "author": "Aether", "version": "1.0.0" },
              "palette": { "teal": "#00f0ff" },
              "tokens": {
                "color.accent": { "$palette": "teal", "alpha": 0.18 },
                "color.accent.like": { "$token": "color.accent" },
                "color.hero": { "$source": "albumArt.vibrant" },
                "font.sans": ["Space Grotesk Variable"]
              },
              "patterns": {
                "griglia": { "effect": "hairlineGrid", "color": { "$palette": "teal" }, "cell": "36px" },
                "taglio": { "effect": "chamfer", "size": "10px" }
              }
            }"##,
        );
        for vietato in ["url(", "@import", "expression(", "javascript:", "</style"] {
            assert!(
                !compilata.css.contains(vietato),
                "«{vietato}» nel foglio:\n{}",
                compilata.css
            );
        }
        // Il riferimento diventa una `var()` col nome preso dal registro.
        assert!(
            compilata.css.contains("--accent-like: var(--accent);"),
            "{}",
            compilata.css
        );
        // La tavolozza esce in due forme, sempre.
        assert!(
            compilata.css.contains("--skin-color-teal: rgb(0 240 255);"),
            "{}",
            compilata.css
        );
        assert!(
            compilata.css.contains("--skin-color-teal-rgb: 0 240 255;"),
            "{}",
            compilata.css
        );
        // Il motivo che produce un ritaglio ha il suffisso che lo dice.
        assert!(
            compilata.css.contains("--skin-taglio-clip: polygon("),
            "{}",
            compilata.css
        );
        // E il token legato alla copertina è dichiarato come tale.
        assert_eq!(compilata.dynamic_tokens, ["color.hero"]);
    }

    #[test]
    fn la_griglia_e_un_colore_e_un_passo() {
        // Nel vecchio albero erano due gradienti scritti due volte, e la
        // variante chiara ne sbagliava uno.
        let css = compila_documento(
            &MINIMA.replace(
                "\"tokens\": {}",
                r##""tokens": {}, "patterns": { "g": { "effect": "hairlineGrid", "color": "rgba(0,240,255,0.07)", "cell": "36px" } }"##,
            ),
        )
        .css;
        assert!(css.contains("0 0 / 100% 36px"), "{css}");
        assert!(css.contains("0 0 / 36px 100%"), "{css}");
        // Lo stesso colore in entrambe le righe, per costruzione.
        assert_eq!(css.matches("rgb(0 240 255 / 0.07)").count(), 2, "{css}");
    }

    #[test]
    fn gli_stati_non_scavalcano_i_componenti() {
        let compilata = compila_documento(
            &MINIMA.replace(
                "\"tokens\": {}",
                r##""tokens": {}, "parts": { "nav-pill": { "states": { "hover": { "opacity": 0.8 }, "focus": { "opacity": 1 } } } }"##,
            ),
        );
        assert!(
            compilata
                .css
                .contains(":root[data-skin='prova'] .nav-pill:where(:hover)"),
            "{}",
            compilata.css
        );
        // Il fuoco è quello da tastiera.
        assert!(
            compilata
                .css
                .contains(":root[data-skin='prova'] .nav-pill:where(:focus-visible)"),
            "{}",
            compilata.css
        );
    }

    #[test]
    fn il_livello_non_lo_puo_spostare_la_skin() {
        let css = compila_documento(
            &MINIMA.replace(
                "\"tokens\": {}",
                r##""tokens": {}, "parts": { "ambient-backdrop": { "layer": { "background": [{ "effect": "solid", "color": "#fff" }], "opacity": 0.2 } } }"##,
            ),
        )
        .css;
        assert!(css.contains(".ambient-backdrop::after"), "{css}");
        // Posizionamento e contenuto li mette il compilatore, sempre uguali.
        assert!(css.contains("position: absolute;"), "{css}");
        assert!(css.contains("inset: 0;"), "{css}");
        assert!(css.contains("pointer-events: none;"), "{css}");
    }

    #[test]
    fn il_costo_somma_motivi_e_parti() {
        let compilata = compila_documento(
            &MINIMA.replace(
                "\"tokens\": {}",
                r##""tokens": {},
                   "patterns": { "sfoca": { "effect": "blurBehind", "radius": "24px" } },
                   "parts": { "section-card": { "background": [{ "effect": "solid", "color": "#fff" }] } }"##,
            ),
        );
        // Sfocatura 10 più tinta 1: sommarne una metà renderebbe il budget
        // inutile.
        assert_eq!(compilata.cost, 11);
    }

    #[test]
    fn due_compilazioni_danno_lo_stesso_foglio() {
        // Determinismo: è ciò che rende utili gli snapshot e la cache
        // dell'anteprima dal vivo, e non lo si ottiene iterando un oggetto JSON.
        let json = MINIMA.replace(
            "\"tokens\": {}",
            r##""tokens": { "color.accent": "#8b7cf6", "color.surface.0": "#09090d", "radius.card": "14px" }"##,
        );
        let skin = parse_skin_json(&json).expect("valida");
        assert_eq!(compile_skin(&skin).css, compile_skin(&skin).css);
        // E l'ordine è quello del registro, non quello di scrittura.
        let css = compile_skin(&skin).css;
        let posizione = |ago: &str| css.find(ago).unwrap_or(usize::MAX);
        assert!(posizione("--color-surface-0") < posizione("--accent"));
        assert!(posizione("--accent") < posizione("--radius-card"));
    }

    #[test]
    fn il_tema_chiaro_sovrascrive_solo_quel_che_serve() {
        let css = compila_documento(&MINIMA.replace(
            "\"tokens\": {}",
            r##""tokens": { "color.hairline": "rgba(255,255,255,0.07)" },
                   "themes": { "light": { "color.hairline": "rgba(0,0,0,0.08)" } }"##,
        ))
        .css;
        assert!(
            css.contains(":root[data-skin='prova'][data-theme='light'] {"),
            "{css}"
        );
        assert!(css.contains("color-scheme: light;"), "{css}");
    }
}
