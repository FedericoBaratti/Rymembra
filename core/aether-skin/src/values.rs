//! I valori che una skin può scrivere.
//!
//! Questo modulo è la sicurezza dell'intero formato, e vale spiegare perché non
//! è un dettaglio di validazione.
//!
//! La decisione è: **zero CSS arbitrario**. Una skin non contiene stringhe CSS,
//! contiene dati; il compilatore è l'unico autore di CSS. La conseguenza è che
//! tutta la classe di attacchi via CSS — `url()` che chiama a casa, `@import`
//! che carica un foglio remoto, selettori che esfiltrano il contenuto degli
//! attributi — non si applica, perché non esiste un canale in cui infilarli. Ma
//! vale **solo** se ogni valore è tipizzato: basta un campo che accetti una
//! stringa e la copi nell'output, e la garanzia salta tutta insieme.
//!
//! Quindi: un colore viene scomposto nei suoi canali, una lunghezza è un numero
//! più un'unità da lista chiusa, un easing è una curva con quattro numeri.
//! Niente di ciò che entra viene mai copiato tale e quale.
//!
//! # Perché nessuna espressione regolare
//!
//! Il vecchio albero riconosceva colori, lunghezze e durate con tre espressioni
//! regolari. Qui sono tre funzioni scritte a mano, e non è pignoleria: una
//! `regex` è una dipendenza in più su un percorso che elabora dati ostili, e
//! soprattutto è illeggibile nel punto in cui deve essere ovvia. `RGB_FN_RE`
//! nell'originale è lunga centosessanta caratteri e accetta separatori misti che
//! nessuno ha deciso di accettare: sono emersi dalla scrittura.

use std::fmt::Write as _;

/// Un colore, sempre nei suoi canali.
///
/// Scomposto e non una stringa perché il contratto con i componenti richiede
/// **anche** la tripla `r g b` — i canvas del visualizer e dello scrubber la
/// leggono per costruire `rgba()` a runtime. Nel vecchio albero `--accent` e
/// `--accent-rgb` erano due dichiarazioni da tenere allineate a mano, e
/// `--cyber-fog-rgb` aveva perfino un commento in maiuscolo accanto: «DEVE
/// combaciare con surface-0». Con i canali la tripla si deriva, e non può
/// divergere.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    /// Canale rosso.
    pub r: u8,
    /// Canale verde.
    pub g: u8,
    /// Canale blu.
    pub b: u8,
    /// Opacità, fra 0 e 1.
    pub a: f64,
}

impl Rgba {
    /// Lo stesso colore con un'altra opacità. Serve ai token `*-soft` e `*-glow`.
    #[must_use]
    pub fn with_alpha(self, alpha: f64) -> Self {
        Self {
            a: alpha.clamp(0.0, 1.0),
            ..self
        }
    }
}

/// Il numero, come lo scrive il CSS: al massimo `decimali` cifre, senza zeri in
/// coda.
///
/// Serve perché `format!("{:.4}", 1.0)` dà `1.0000`, e un foglio pieno di zeri
/// inutili è un foglio più difficile da leggere e da confrontare con quello di
/// ieri.
pub(crate) fn number(value: f64, decimali: usize) -> String {
    if !value.is_finite() {
        // Un valore non finito non arriva mai qui da un documento validato. Se
        // ci arriva è un guasto nostro, e zero è l'unica risposta che non
        // produce CSS non valido.
        return "0".to_owned();
    }
    let mut text = format!("{value:.decimali$}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    if text == "-0" { "0".to_owned() } else { text }
}

/// Un numero in una posizione qualunque: quattro decimali bastano a tutto.
pub(crate) fn num(value: f64) -> String {
    number(value, 4)
}

// ── Colori ──────────────────────────────────────────────────────────────────

/// Un canale da un numero, senza troncamenti impliciti.
fn channel(value: f64) -> u8 {
    let rounded = value.round();
    if rounded <= 0.0 {
        return 0;
    }
    if rounded >= 255.0 {
        return 255;
    }
    // Qui `rounded` sta in (0, 255) ed è intero: la conversione non perde nulla.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "i due rami sopra hanno già tolto di mezzo tutto ciò che sta fuori da (0, 255), \
                  e `round` ha reso intero il resto"
    )]
    {
        rounded as u8
    }
}

fn hex_byte(hex: &str, from: usize) -> Option<u8> {
    u8::from_str_radix(hex.get(from..from.checked_add(2)?)?, 16).ok()
}

fn parse_hex(hex: &str) -> Option<Rgba> {
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    // `#abc` → `#aabbcc`, `#abcd` → `#aabbccdd`: ogni cifra si raddoppia.
    let esteso = match hex.len() {
        3 | 4 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 | 8 => hex.to_owned(),
        _ => return None,
    };
    let a = match esteso.len() {
        8 => f64::from(hex_byte(&esteso, 6)?) / 255.0,
        _ => 1.0,
    };
    Some(Rgba {
        r: hex_byte(&esteso, 0)?,
        g: hex_byte(&esteso, 2)?,
        b: hex_byte(&esteso, 4)?,
        a,
    })
}

fn parse_rgb_function(text: &str) -> Option<Rgba> {
    let lower = text.to_ascii_lowercase();
    let inner = lower
        .strip_prefix("rgba(")
        .or_else(|| lower.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    // Virgole e barra sono separatori quanto lo spazio: `rgb(1,2,3)`,
    // `rgb(1 2 3 / 0.5)` e `rgba(1, 2, 3, 0.5)` sono la stessa cosa scritta in
    // tre epoche diverse del CSS, e le skin del vecchio albero le usano tutte.
    let campi: Vec<&str> = inner
        .split([',', '/', ' ', '\t'])
        .filter(|part| !part.is_empty())
        .collect();
    let (r, g, b) = match campi.as_slice() {
        [r, g, b] | [r, g, b, _] => (r, g, b),
        _ => return None,
    };
    let a = match campi.as_slice() {
        [_, _, _, a] => a.parse::<f64>().ok()?.clamp(0.0, 1.0),
        _ => 1.0,
    };
    Some(Rgba {
        r: channel(r.parse::<f64>().ok()?),
        g: channel(g.parse::<f64>().ok()?),
        b: channel(b.parse::<f64>().ok()?),
        a,
    })
}

/// Riconosce un colore, o niente.
///
/// Restituisce `None` invece di un errore: chi chiama sa dove si trovava il
/// valore nel documento, e solo lui può scrivere un messaggio che lo nomina.
///
/// Fuori dalla lista restano di proposito i nomi di colore CSS, `currentColor`,
/// `color-mix()` e `transparent`. Non è severità fine a sé stessa: sono i casi
/// in cui «non si sa bene cos'è» diventerebbe una stringa copiata nell'output.
#[must_use]
pub fn parse_color(input: &str) -> Option<Rgba> {
    let text = input.trim();
    match text.strip_prefix('#') {
        Some(hex) => parse_hex(hex),
        None => parse_rgb_function(text),
    }
}

/// `rgb(r g b)` oppure `rgb(r g b / a)`: la forma che il compilatore emette.
#[must_use]
pub fn format_color(color: Rgba) -> String {
    let Rgba { r, g, b, a } = color;
    if a >= 1.0 {
        return format!("rgb({r} {g} {b})");
    }
    format!("rgb({r} {g} {b} / {})", number(a, 3))
}

/// La tripla senza opacità, per i token `*-rgb` che i canvas leggono.
#[must_use]
pub fn format_rgb_triple(color: Rgba) -> String {
    format!("{} {} {}", color.r, color.g, color.b)
}

fn luminanza(r: f64, g: f64, b: f64) -> f64 {
    let canale = |grezzo: f64| {
        let c = grezzo.clamp(0.0, 255.0) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126_f64.mul_add(canale(r), 0.7152_f64.mul_add(canale(g), 0.0722 * canale(b)))
}

/// Luminanza relativa secondo WCAG 2.1.
///
/// Sta qui perché serve al controllo di contrasto, e perché è la ragione per cui
/// i colori vanno tenuti nei canali: sull'albero mobile qualcuno ha dovuto
/// alzare a mano `--color-text-2/3` da 0.38 a 0.5 per renderli leggibili su un
/// telefono, e l'ha scoperto sul dispositivo. Con i canali il controllo si fa
/// prima.
#[must_use]
pub fn relative_luminance(color: Rgba) -> f64 {
    luminanza(f64::from(color.r), f64::from(color.g), f64::from(color.b))
}

/// Rapporto di contrasto fra due colori.
///
/// Il primo viene appiattito sul secondo quando è semitrasparente, che qui è il
/// caso normale: i token del testo sono `rgba(255 255 255 / 0.6)` sopra una
/// superficie, e misurarne il contrasto ignorando l'opacità darebbe sempre il
/// massimo.
#[must_use]
pub fn contrast_ratio(foreground: Rgba, background: Rgba) -> f64 {
    let piatto = |sopra: u8, sotto: u8| {
        f64::from(sopra).mul_add(foreground.a, f64::from(sotto) * (1.0 - foreground.a))
    };
    let davanti = if foreground.a >= 1.0 {
        relative_luminance(foreground)
    } else {
        luminanza(
            piatto(foreground.r, background.r),
            piatto(foreground.g, background.g),
            piatto(foreground.b, background.b),
        )
    };
    let dietro = relative_luminance(background);
    let chiaro = davanti.max(dietro);
    let scuro = davanti.min(dietro);
    (chiaro + 0.05) / (scuro + 0.05)
}

// ── Lunghezze ───────────────────────────────────────────────────────────────

/// Le unità ammesse. Lista chiusa, e corta di proposito.
///
/// Fuori restano `calc()` e le stringhe libere: una lunghezza è un numero più
/// un'unità, e i `calc()` che servono li compone il compilatore — per esempio
/// `--player-clearance`, che è `calc(var(--player-h) + var(--player-gap) * 2)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthUnit {
    /// Pixel.
    Px,
    /// Multipli della dimensione di base.
    Rem,
    /// Multipli della dimensione locale.
    Em,
    /// Percentuale del contenitore.
    Percent,
    /// Percentuale dell'altezza della finestra.
    Vh,
    /// Percentuale della larghezza della finestra.
    Vw,
    /// Percentuale della larghezza del contenitore di query.
    Cqw,
    /// Percentuale dell'altezza del contenitore di query.
    Cqh,
    /// Larghezza dello zero nel carattere corrente.
    Ch,
}

impl LengthUnit {
    /// Come si scrive in CSS.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Px => "px",
            Self::Rem => "rem",
            Self::Em => "em",
            Self::Percent => "%",
            Self::Vh => "vh",
            Self::Vw => "vw",
            Self::Cqw => "cqw",
            Self::Cqh => "cqh",
            Self::Ch => "ch",
        }
    }

    /// Tutte, nell'ordine in cui compaiono nei messaggi d'errore.
    pub const ALL: &'static [Self] = &[
        Self::Px,
        Self::Rem,
        Self::Em,
        Self::Percent,
        Self::Vh,
        Self::Vw,
        Self::Cqw,
        Self::Cqh,
        Self::Ch,
    ];

    fn parse(raw: &str) -> Option<Self> {
        let lower = raw.to_ascii_lowercase();
        Self::ALL.iter().copied().find(|u| u.as_str() == lower)
    }

    /// L'elenco per un messaggio d'errore: `px, rem, em, %, …`.
    pub(crate) fn elenco() -> String {
        let mut testo = String::new();
        for (indice, unita) in Self::ALL.iter().enumerate() {
            if indice > 0 {
                testo.push_str(", ");
            }
            testo.push_str(unita.as_str());
        }
        testo
    }
}

/// Un numero più un'unità.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Length {
    /// Quanto.
    pub value: f64,
    /// Di cosa.
    pub unit: LengthUnit,
}

impl Length {
    /// Una lunghezza in pixel, per i valori di ripiego del compilatore.
    #[must_use]
    pub const fn px(value: f64) -> Self {
        Self {
            value,
            unit: LengthUnit::Px,
        }
    }
}

/// Riconosce una lunghezza, o niente.
#[must_use]
pub fn parse_length(input: &str) -> Option<Length> {
    let text = input.trim();
    let taglio = text.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))?;
    let value: f64 = text.get(..taglio)?.parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some(Length {
        value,
        unit: LengthUnit::parse(text.get(taglio..)?)?,
    })
}

/// Una lunghezza in CSS.
#[must_use]
pub fn format_length(length: Length) -> String {
    format!("{}{}", num(length.value), length.unit.as_str())
}

/// Una lunghezza semplice, o una che si adatta.
///
/// L'adattiva è emersa convertendo la skin `plain`: `--content-x` è
/// `clamp(16px, 3cqw, 48px)`, e una lunghezza semplice non lo esprime. La
/// risposta **non** è ammettere `clamp()` come stringa — sarebbe il primo campo
/// che copia testo nell'output, e da lì la garanzia del formato salta tutta
/// insieme. La risposta è la struttura: tre lunghezze, e la funzione la compone
/// il compilatore.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LengthValue {
    /// Una misura fissa.
    Fixed(Length),
    /// Minimo, preferito, massimo.
    Adaptive {
        /// Sotto questa non scende.
        min: Length,
        /// Quella che vorrebbe.
        preferred: Length,
        /// Sopra questa non sale.
        max: Length,
    },
}

/// Una lunghezza, semplice o adattiva, in CSS.
#[must_use]
pub fn format_length_value(value: LengthValue) -> String {
    match value {
        LengthValue::Fixed(length) => format_length(length),
        LengthValue::Adaptive {
            min,
            preferred,
            max,
        } => format!(
            "clamp({}, {}, {})",
            format_length(min),
            format_length(preferred),
            format_length(max)
        ),
    }
}

// ── Durate ──────────────────────────────────────────────────────────────────

/// Tetto sulle durate: un'animazione di dieci secondi è un blocco, non uno stile.
pub const MAX_DURATION_MS: f64 = 10_000.0;

/// Una durata, sempre in millisecondi.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Duration {
    /// Millisecondi.
    pub ms: f64,
}

/// Riconosce una durata, o niente.
#[must_use]
pub fn parse_duration(input: &str) -> Option<Duration> {
    let text = input.trim();
    let taglio = text.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let value: f64 = text.get(..taglio)?.parse().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let ms = match text.get(taglio..)?.to_ascii_lowercase().as_str() {
        "ms" => value,
        "s" => value * 1000.0,
        _ => return None,
    };
    if ms > MAX_DURATION_MS {
        return None;
    }
    Some(Duration { ms })
}

/// Una durata in CSS.
#[must_use]
pub fn format_duration(duration: Duration) -> String {
    format!("{}ms", number(duration.ms, 2))
}

// ── Easing ──────────────────────────────────────────────────────────────────

/// Le curve con un nome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EasingKeyword {
    /// Costante.
    Linear,
    /// Il default del CSS.
    Ease,
    /// Parte piano.
    EaseIn,
    /// Finisce piano.
    EaseOut,
    /// Entrambe.
    EaseInOut,
}

impl EasingKeyword {
    /// Come si scrive in CSS. È anche il nome nel documento.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Ease => "ease",
            Self::EaseIn => "ease-in",
            Self::EaseOut => "ease-out",
            Self::EaseInOut => "ease-in-out",
        }
    }

    /// Tutte.
    pub const ALL: &'static [Self] = &[
        Self::Linear,
        Self::Ease,
        Self::EaseIn,
        Self::EaseOut,
        Self::EaseInOut,
    ];

    /// Dal nome scritto nel documento.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.as_str() == raw)
    }
}

/// Dove sta il salto in una curva a gradini.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepPosition {
    /// All'inizio di ogni gradino.
    Start,
    /// Alla fine.
    End,
}

impl StepPosition {
    /// Come si scrive in CSS.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::End => "end",
        }
    }
}

/// Una curva di accelerazione.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Easing {
    /// Una delle curve con un nome.
    Keyword(EasingKeyword),
    /// I due punti di controllo: `[x1, y1, x2, y2]`.
    ///
    /// La x sta in `[0,1]`; la y può uscirne, ed è così che si ottiene il
    /// rimbalzo di `--ease-spring`, `cubic-bezier(0.34, 1.56, 0.64, 1)`.
    CubicBezier([f64; 4]),
    /// A gradini.
    Steps {
        /// Quanti.
        count: u32,
        /// Dove salta.
        position: StepPosition,
    },
}

/// Una curva in CSS.
#[must_use]
pub fn format_easing(easing: Easing) -> String {
    match easing {
        Easing::Keyword(keyword) => keyword.as_str().to_owned(),
        Easing::CubicBezier(points) => {
            let mut testo = String::from("cubic-bezier(");
            for (indice, punto) in points.iter().enumerate() {
                if indice > 0 {
                    testo.push_str(", ");
                }
                testo.push_str(&num(*punto));
            }
            testo.push(')');
            testo
        }
        Easing::Steps { count, position } => {
            format!("steps({count}, {})", position.as_str())
        }
    }
}

// ── Caratteri ───────────────────────────────────────────────────────────────

/// A cosa serve una famiglia di caratteri. Decide i ripieghi di sistema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontKind {
    /// Il corpo del testo.
    Sans,
    /// Spaziatura fissa.
    Mono,
    /// Da display.
    Display,
}

impl FontKind {
    const fn fallbacks(self) -> &'static str {
        match self {
            Self::Sans => "system-ui, -apple-system, sans-serif",
            Self::Mono => "ui-monospace, SFMono-Regular, monospace",
            Self::Display => "system-ui, sans-serif",
        }
    }
}

/// Il nome di una famiglia è ammesso?
///
/// Solo lettere, cifre, spazi, trattino e underscore. Il compilatore lo mette
/// lui fra apici: un nome con una virgoletta o un punto e virgola potrebbe
/// chiudere la dichiarazione e aprirne un'altra, ed è esattamente la fuga che il
/// formato deve rendere impossibile.
#[must_use]
pub fn is_font_family(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b' ' || b == b'-' || b == b'_')
}

/// Una pila di caratteri, con i ripieghi di sistema in coda.
#[must_use]
pub fn format_font_stack(families: &[String], kind: FontKind) -> String {
    let mut testo = String::new();
    for family in families {
        // Gli apici li mette il compilatore, non l'autore: è la ragione per cui
        // il nome è vincolato a un alfabeto che non li contiene.
        let _ = write!(testo, "'{family}', ");
    }
    testo.push_str(kind.fallbacks());
    testo
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i_colori_entrano_come_canali() {
        assert_eq!(
            parse_color("#8b7cf6"),
            Some(Rgba {
                r: 139,
                g: 124,
                b: 246,
                a: 1.0
            })
        );
        // Le tre forme corte e lunghe sono lo stesso colore.
        assert_eq!(parse_color("#abc"), parse_color("#aabbcc"));
        assert_eq!(parse_color("#abcd"), parse_color("#aabbccdd"));
        // Le tre epoche del CSS che le skin del vecchio albero usano tutte.
        let mezzo = Rgba {
            r: 255,
            g: 255,
            b: 255,
            a: 0.6,
        };
        assert_eq!(parse_color("rgba(255, 255, 255, 0.6)"), Some(mezzo));
        assert_eq!(parse_color("rgb(255 255 255 / 0.6)"), Some(mezzo));
        assert_eq!(parse_color("RGBA(255,255,255,.6)"), Some(mezzo));
    }

    #[test]
    fn quel_che_non_e_un_colore_non_entra() {
        // Ognuno di questi, ammesso, sarebbe una stringa copiata nell'output.
        for scritto in [
            "blu",
            "rebeccapurple",
            "currentColor",
            "transparent",
            "color-mix(in srgb, red, blue)",
            "var(--accent)",
            "url(https://esempio.invalid/x.png)",
            "#12345",
            "#gg0000",
            "rgb(1,2)",
            "rgb(1,2,3,4,5)",
            "rgb(50%,0,0)",
            "",
        ] {
            assert_eq!(parse_color(scritto), None, "accettato: {scritto}");
        }
    }

    #[test]
    fn la_tripla_non_puo_divergere_dal_colore() {
        // Il difetto che il commento in maiuscolo «DEVE combaciare con
        // surface-0» sorvegliava a mano: qui sono lo stesso dato.
        let colore = parse_color("#060608").expect("colore valido");
        assert_eq!(format_color(colore), "rgb(6 6 8)");
        assert_eq!(format_rgb_triple(colore), "6 6 8");
        assert_eq!(format_rgb_triple(colore.with_alpha(0.2)), "6 6 8");
    }

    #[test]
    fn l_opacita_si_scrive_con_la_barra() {
        let colore = Rgba {
            r: 139,
            g: 124,
            b: 246,
            a: 0.35,
        };
        assert_eq!(format_color(colore), "rgb(139 124 246 / 0.35)");
        // Niente zeri in coda: un foglio con `0.3500` è più difficile da
        // confrontare con quello di ieri.
        assert_eq!(
            format_color(colore.with_alpha(0.5)),
            "rgb(139 124 246 / 0.5)"
        );
        assert_eq!(format_color(colore.with_alpha(1.0)), "rgb(139 124 246)");
    }

    #[test]
    fn il_contrasto_tiene_conto_dell_opacita() {
        let fondo = parse_color("#09090d").expect("colore valido");
        let testo_1 = parse_color("rgba(255,255,255,0.92)").expect("colore valido");
        let testo_3 = parse_color("rgba(255,255,255,0.38)").expect("colore valido");
        // Il testo primario passa AA; il terziario no, ed è precisamente ciò che
        // sul telefono si è scoperto guardando lo schermo.
        assert!(contrast_ratio(testo_1, fondo) > 4.5);
        assert!(contrast_ratio(testo_3, fondo) < 4.5);
        // Ignorando l'opacità darebbero lo stesso identico rapporto.
        assert!(
            (contrast_ratio(testo_1, fondo) - contrast_ratio(testo_3, fondo)).abs() > 1.0,
            "l'opacità non sta cambiando il risultato"
        );
    }

    #[test]
    fn una_lunghezza_e_un_numero_piu_un_unita() {
        assert_eq!(parse_length("14px"), Some(Length::px(14.0)));
        assert_eq!(
            parse_length("3cqw"),
            Some(Length {
                value: 3.0,
                unit: LengthUnit::Cqw
            })
        );
        assert_eq!(
            parse_length("-2.5rem"),
            Some(Length {
                value: -2.5,
                unit: LengthUnit::Rem
            })
        );
        assert_eq!(format_length(Length::px(14.0)), "14px");
    }

    #[test]
    fn quel_che_non_e_una_lunghezza_non_entra() {
        for scritto in [
            "calc(100% - 20px)",
            "clamp(16px, 3cqw, 48px)",
            "14",
            "px",
            "14pt",
            "14 px",
            "auto",
            "",
        ] {
            assert_eq!(parse_length(scritto), None, "accettato: {scritto}");
        }
    }

    #[test]
    fn il_clamp_lo_compone_il_compilatore() {
        // `--content-x` di `plain`, che nel vecchio albero è una stringa
        // `clamp(16px, 3cqw, 48px)` e qui è una struttura.
        let adattiva = LengthValue::Adaptive {
            min: Length::px(16.0),
            preferred: Length {
                value: 3.0,
                unit: LengthUnit::Cqw,
            },
            max: Length::px(48.0),
        };
        assert_eq!(format_length_value(adattiva), "clamp(16px, 3cqw, 48px)");
    }

    #[test]
    fn una_durata_ha_un_tetto() {
        assert_eq!(parse_duration("280ms"), Some(Duration { ms: 280.0 }));
        assert_eq!(parse_duration("0.5s"), Some(Duration { ms: 500.0 }));
        assert_eq!(format_duration(Duration { ms: 500.0 }), "500ms");
        // Dieci secondi passano, undici no: oltre quella soglia non è più uno
        // stile, è un blocco.
        assert!(parse_duration("10s").is_some());
        assert_eq!(parse_duration("11s"), None);
        assert_eq!(parse_duration("-1ms"), None);
        assert_eq!(parse_duration("280"), None);
    }

    #[test]
    fn la_curva_col_rimbalzo_e_ammessa() {
        // `--ease-spring`: la y oltre 1 è il rimbalzo, ed è il motivo per cui il
        // vincolo su y è più largo di quello su x.
        assert_eq!(
            format_easing(Easing::CubicBezier([0.34, 1.56, 0.64, 1.0])),
            "cubic-bezier(0.34, 1.56, 0.64, 1)"
        );
        assert_eq!(
            format_easing(Easing::Keyword(EasingKeyword::EaseOut)),
            "ease-out"
        );
        assert_eq!(
            format_easing(Easing::Steps {
                count: 4,
                position: StepPosition::End
            }),
            "steps(4, end)"
        );
    }

    #[test]
    fn un_carattere_non_puo_chiudere_la_dichiarazione() {
        assert!(is_font_family("Space Grotesk Variable"));
        assert!(is_font_family("Inter_Variable-2"));
        // Ognuno di questi, messo fra apici dal compilatore, li chiuderebbe.
        for nome in [
            "Inter', color: red; --x: '",
            "Inter\"",
            "Inter;",
            "Inter, sans-serif",
            "",
            &"a".repeat(65),
        ] {
            assert!(!is_font_family(nome), "accettato: {nome}");
        }
    }

    #[test]
    fn i_ripieghi_di_sistema_li_aggiunge_il_compilatore() {
        assert_eq!(
            format_font_stack(&["Inter Variable".to_owned()], FontKind::Sans),
            "'Inter Variable', system-ui, -apple-system, sans-serif"
        );
    }
}
