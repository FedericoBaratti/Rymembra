//! Quel che si dice di una skin già valida: gli avvisi e il contrasto, i budget
//! e quando fermano l'installazione, l'esame prima di indossarla.

use rymembra_domain::errors::{AppError, ErrorCode};

use super::{MotionIntensity, SkinDocument, SkinIssue, Variante, in_errore};
use crate::effects::{Effect, Paint, Stop};
use crate::layout::shell_di_serie;
use crate::movimento::{AnimFrame, AnimTrigger, CASCATA_VOCI, Iterazioni};
use crate::parts::PartStyle;
use crate::tokens::{ColorValue, TokenSet, TokenValue};
use crate::values::{Length, Rgba};

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
