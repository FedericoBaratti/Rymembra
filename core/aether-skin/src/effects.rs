//! La libreria degli effetti: parametrica, e con un costo dichiarato.
//!
//! Gli effetti non sono inventati. Sono l'inventario di ciò che le skin
//! esistenti fanno davvero, riscritto come parametri invece che come CSS. Per
//! esempio `--cyber-grid` nel vecchio albero è:
//!
//! ```text
//! linear-gradient(rgba(0,240,255,0.07) 1px, transparent 1px) 0 0 / 100% 36px,
//! linear-gradient(90deg, rgba(0,240,255,0.07) 1px, transparent 1px) 0 0 / 36px 100%
//! ```
//!
//! Due gradienti con lo stesso colore, la stessa opacità e lo stesso passo,
//! scritti due volte. Qui è `hairlineGrid` con un colore e una cella, e la
//! variante chiara della stessa skin — che nell'originale ripete le due righe
//! con un altro colore, e per una svista usa `36px 36px` invece di `100% 36px` —
//! diventa un colore diverso e nient'altro.
//!
//! # Il costo
//!
//! Ogni effetto dichiara quanto costa disegnarlo, e non è documentazione:
//! `global.css` porta la nota scritta a mano «al massimo ~4 superfici con
//! `backdrop-filter` composte insieme», e qui quel limite è un numero che si può
//! controllare.
//!
//! # Cosa cambia rispetto all'originale
//!
//! Nel vecchio albero costo e proprietà di destinazione erano due mappe
//! letterali con una voce per effetto, e la seconda portava una voce di troppo:
//! `filter: 'filter'`, che non è il nome di nessun effetto. Non dava errore
//! perché la mappa era chiusa da `as Readonly<Record<EffectName, …>>`, cioè da
//! un'asserzione che spegneva il controllo proprio dove serviva. Qui sono due
//! `match` sull'enum: la voce fantasma non è esprimibile, e un effetto nuovo
//! senza costo non compila.

use crate::tokens::ColorValue;
use crate::values::Length;

/// Quanto costa un effetto, per fotogramma.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostClass {
    /// Una tinta o un gradiente: il compositore lo assorbe.
    Cheap,
    /// Un motivo ripetuto: ridisegna l'area a ogni cambio.
    Paint,
    /// Richiede un livello proprio.
    Composited,
    /// `backdrop-filter` o sfocatura: costoso, e su WebView molto costoso.
    Gpu,
}

impl CostClass {
    /// Peso relativo, per sommare il costo di una superficie.
    #[must_use]
    pub const fn weight(self) -> u32 {
        match self {
            Self::Cheap => 1,
            Self::Paint => 3,
            Self::Composited => 4,
            Self::Gpu => 10,
        }
    }
}

/// Budget di una singola superficie.
///
/// Dieci è **un** `backdrop-filter` da solo: è il numero che rende esplicita la
/// nota di `global.css`. Quattro superfici sfocate composte insieme fanno
/// quaranta, cioè quattro volte il budget — che è precisamente il limite che
/// quel commento avvertiva di non superare.
pub const SURFACE_COST_BUDGET: u32 = 10;

/// In quale proprietà CSS finisce un effetto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectTarget {
    /// `background`.
    Background,
    /// `clip-path`.
    ClipPath,
    /// `backdrop-filter`.
    Filter,
}

/// La forma di un gradiente radiale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadialShape {
    /// Cerchio.
    Circle,
    /// Ellisse.
    Ellipse,
}

impl RadialShape {
    /// Come si scrive in CSS.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Circle => "circle",
            Self::Ellipse => "ellipse",
        }
    }
}

/// Un angolo da tagliare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    /// In alto a sinistra.
    TopLeft,
    /// In alto a destra.
    TopRight,
    /// In basso a destra.
    BottomRight,
    /// In basso a sinistra.
    BottomLeft,
}

impl Corner {
    /// Come si scrive nel documento.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TopLeft => "topLeft",
            Self::TopRight => "topRight",
            Self::BottomRight => "bottomRight",
            Self::BottomLeft => "bottomLeft",
        }
    }

    /// Tutti.
    pub const ALL: &'static [Self] = &[
        Self::TopLeft,
        Self::TopRight,
        Self::BottomRight,
        Self::BottomLeft,
    ];

    /// Dal nome scritto nel documento.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.as_str() == raw)
    }
}

/// Una fermata di un gradiente.
#[derive(Debug, Clone, PartialEq)]
pub struct Stop {
    /// Il colore.
    pub color: ColorValue,
    /// Dove si trova. Assente: distribuita uniformemente.
    pub at: Option<Length>,
}

/// Gli effetti.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Una tinta piatta.
    Solid {
        /// Il colore.
        color: ColorValue,
    },
    /// Un gradiente lineare.
    LinearGradient {
        /// Gradi. 180 = dall'alto in basso, come il default CSS.
        angle: f64,
        /// Le fermate.
        stops: Vec<Stop>,
    },
    /// Un gradiente radiale.
    RadialGradient {
        /// Cerchio o ellisse.
        shape: RadialShape,
        /// Il centro.
        at: Option<(Length, Length)>,
        /// Il raggio.
        size: Option<Length>,
        /// Le fermate.
        stops: Vec<Stop>,
    },
    /// Un gradiente conico.
    ConicGradient {
        /// Da quale angolo parte.
        from: f64,
        /// Il centro.
        at: Option<(Length, Length)>,
        /// Le fermate.
        stops: Vec<Stop>,
    },
    /// La griglia a linee sottili sui due assi: il `--cyber-grid`, con il passo
    /// che diventa un parametro invece di essere ripetuto due volte.
    HairlineGrid {
        /// Il colore delle linee.
        color: ColorValue,
        /// Passo orizzontale.
        cell: Length,
        /// Passo verticale, se diverso.
        cell_y: Option<Length>,
        /// Spessore della linea.
        thickness: Option<Length>,
    },
    /// Le scanline CRT: `--cyber-scanline`.
    Scanlines {
        /// Il colore.
        color: ColorValue,
        /// Spessore della linea.
        line: Length,
        /// Passo fra due linee.
        gap: Length,
    },
    /// Le strisce diagonali: `--cyber-hazard`.
    Stripes {
        /// Inclinazione.
        angle: f64,
        /// Il colore della striscia.
        color: ColorValue,
        /// Il colore fra una striscia e l'altra.
        background: ColorValue,
        /// Larghezza della striscia.
        width: Length,
    },
    /// La matrice di punti della skin Nothing.
    DotGrid {
        /// Il colore dei punti.
        color: ColorValue,
        /// Distanza fra i centri.
        spacing: Length,
        /// Raggio del punto.
        dot: Length,
    },
    /// Oscuramento ai bordi.
    Vignette {
        /// Il colore verso cui scurisce.
        color: ColorValue,
        /// Da dove comincia, in percentuale del raggio.
        start: f64,
    },
    /// Gli angoli tagliati: `--cyber-chamfer`.
    ///
    /// Produce un `clip-path`, non uno sfondo. Nel vecchio albero erano due
    /// token distinti — `--cyber-cut` per la misura, `--cyber-chamfer` per il
    /// poligono — da tenere coerenti a mano.
    Chamfer {
        /// Quanto taglia.
        size: Length,
        /// Quali angoli.
        corners: Vec<Corner>,
    },
    /// Sfocatura di ciò che sta sotto. L'effetto più costoso che esista qui.
    BlurBehind {
        /// Raggio della sfocatura.
        radius: Length,
        /// Saturazione in percentuale. 100 = invariata.
        saturate: Option<f64>,
    },
}

impl Effect {
    /// Il nome nel documento.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match *self {
            Self::Solid { .. } => "solid",
            Self::LinearGradient { .. } => "linearGradient",
            Self::RadialGradient { .. } => "radialGradient",
            Self::ConicGradient { .. } => "conicGradient",
            Self::HairlineGrid { .. } => "hairlineGrid",
            Self::Scanlines { .. } => "scanlines",
            Self::Stripes { .. } => "stripes",
            Self::DotGrid { .. } => "dotGrid",
            Self::Vignette { .. } => "vignette",
            Self::Chamfer { .. } => "chamfer",
            Self::BlurBehind { .. } => "blurBehind",
        }
    }

    /// Quanto costa disegnarlo.
    #[must_use]
    pub const fn cost(&self) -> CostClass {
        match *self {
            Self::Solid { .. }
            | Self::LinearGradient { .. }
            | Self::RadialGradient { .. }
            | Self::Vignette { .. } => CostClass::Cheap,
            Self::ConicGradient { .. }
            | Self::HairlineGrid { .. }
            | Self::Scanlines { .. }
            | Self::Stripes { .. }
            | Self::DotGrid { .. } => CostClass::Paint,
            Self::Chamfer { .. } => CostClass::Composited,
            Self::BlurBehind { .. } => CostClass::Gpu,
        }
    }

    /// In quale proprietà va.
    ///
    /// Serve al compilatore per non mettere un `clip-path` dentro un
    /// `background`, e a chi scrive una skin per sapere quali effetti si possono
    /// comporre a livelli — solo quelli che producono uno sfondo.
    #[must_use]
    pub const fn target(&self) -> EffectTarget {
        match *self {
            Self::Solid { .. }
            | Self::LinearGradient { .. }
            | Self::RadialGradient { .. }
            | Self::ConicGradient { .. }
            | Self::HairlineGrid { .. }
            | Self::Scanlines { .. }
            | Self::Stripes { .. }
            | Self::DotGrid { .. }
            | Self::Vignette { .. } => EffectTarget::Background,
            Self::Chamfer { .. } => EffectTarget::ClipPath,
            Self::BlurBehind { .. } => EffectTarget::Filter,
        }
    }

    /// I nomi ammessi, per il messaggio di un effetto sconosciuto.
    pub const NAMES: &'static [&'static str] = &[
        "solid",
        "linearGradient",
        "radialGradient",
        "conicGradient",
        "hairlineGrid",
        "scanlines",
        "stripes",
        "dotGrid",
        "vignette",
        "chamfer",
        "blurBehind",
    ];
}

/// Il costo sommato di una pila di livelli.
#[must_use]
pub fn stack_cost(effects: &[Effect]) -> u32 {
    effects
        .iter()
        .map(|effect| effect.cost().weight())
        .sum::<u32>()
}

/// La pila sfora il budget di una superficie?
#[must_use]
pub fn exceeds_budget(effects: &[Effect]) -> bool {
    stack_cost(effects) > SURFACE_COST_BUDGET
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::values::{Length, Rgba};

    fn colore() -> ColorValue {
        ColorValue::Literal(Rgba {
            r: 0,
            g: 240,
            b: 255,
            a: 0.07,
        })
    }

    fn sfocatura() -> Effect {
        Effect::BlurBehind {
            radius: Length::px(24.0),
            saturate: None,
        }
    }

    #[test]
    fn due_sfocature_sforano_il_budget() {
        // È la nota scritta a mano in `global.css` — «al massimo ~4 superfici
        // con backdrop-filter composte insieme» — trasformata in un numero.
        assert!(!exceeds_budget(&[sfocatura()]));
        assert!(exceeds_budget(&[sfocatura(), sfocatura()]));
        assert_eq!(stack_cost(&[sfocatura(), sfocatura()]), 20);
    }

    #[test]
    fn una_pila_di_motivi_sta_nel_budget() {
        let griglia = Effect::HairlineGrid {
            color: colore(),
            cell: Length::px(36.0),
            cell_y: None,
            thickness: None,
        };
        let tinta = Effect::Solid { color: colore() };
        // Tinta più tre motivi: 1 + 9 = 10, esattamente il budget.
        assert_eq!(
            stack_cost(&[tinta.clone(), griglia.clone(), griglia.clone(), griglia]),
            10
        );
        assert!(!exceeds_budget(&[tinta]));
    }

    #[test]
    fn ogni_effetto_ha_un_nome_e_uno_solo() {
        // La lista dei nomi serve ai messaggi d'errore, e una lista che si
        // scolla dall'enum è un messaggio che suggerisce un effetto inesistente.
        let mut visti: Vec<&str> = Vec::new();
        for effetto in [
            Effect::Solid { color: colore() },
            Effect::LinearGradient {
                angle: 180.0,
                stops: Vec::new(),
            },
            Effect::RadialGradient {
                shape: RadialShape::Ellipse,
                at: None,
                size: None,
                stops: Vec::new(),
            },
            Effect::ConicGradient {
                from: 0.0,
                at: None,
                stops: Vec::new(),
            },
            Effect::HairlineGrid {
                color: colore(),
                cell: Length::px(36.0),
                cell_y: None,
                thickness: None,
            },
            Effect::Scanlines {
                color: colore(),
                line: Length::px(1.0),
                gap: Length::px(3.0),
            },
            Effect::Stripes {
                angle: -45.0,
                color: colore(),
                background: colore(),
                width: Length::px(8.0),
            },
            Effect::DotGrid {
                color: colore(),
                spacing: Length::px(20.0),
                dot: Length::px(1.0),
            },
            Effect::Vignette {
                color: colore(),
                start: 60.0,
            },
            Effect::Chamfer {
                size: Length::px(10.0),
                corners: vec![Corner::TopRight],
            },
            sfocatura(),
        ] {
            visti.push(effetto.name());
        }
        assert_eq!(visti, Effect::NAMES);
    }

    #[test]
    fn il_taglio_e_la_sfocatura_non_vanno_nello_sfondo() {
        // La voce fantasma `filter: 'filter'` del vecchio albero stava proprio
        // qui, e non dava fastidio finché non si andava a leggere la mappa.
        assert_eq!(
            Effect::Chamfer {
                size: Length::px(10.0),
                corners: vec![Corner::TopRight],
            }
            .target(),
            EffectTarget::ClipPath
        );
        assert_eq!(sfocatura().target(), EffectTarget::Filter);
        assert_eq!(
            Effect::Solid { color: colore() }.target(),
            EffectTarget::Background
        );
    }
}
