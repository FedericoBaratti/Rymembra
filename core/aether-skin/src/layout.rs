//! Lo scafale: **dove** stanno le cose, non di che colore sono.
//!
//! [`crate::parts`] dice cosa una skin può ridipingere e si ferma lì: la sua
//! lista di proprietà esclude `width`, `height`, `margin`, `position` e
//! `display` con una motivazione scritta — *«una skin che può spostare le cose
//! può anche sovrapporle, nasconderle o portarle fuori schermo»*. Quella
//! motivazione resta vera di **CSS libero**, e non è vera di un vocabolario
//! chiuso. Questo modulo è quel vocabolario.
//!
//! # Le due garanzie, che sono due argomenti diversi
//!
//! **Non si sovrappone: è strutturale.** Qui non ci sono coordinate, né
//! `position`, né `z-index`, né distanze negative, né una `ZoneKind::Overlay`.
//! Un albero di contenitori le cui uniche relazioni esprimibili sono *contiene*
//! e *precede* non può produrre una sovrapposizione: non esiste un termine che
//! la denoti. Tutto ciò che davvero si sovrappone — modali, menù, notifiche — è
//! del motore e sta fuori da questo albero, ed è **per questo** che la garanzia
//! è una proprietà dell'albero invece di un divieto imposto su di esso.
//!
//! **Non esce dallo schermo: è aritmetica, e decidibile *perché* il vocabolario
//! è chiuso.** Ogni misura è un numero che si può sommare prima di disegnare:
//! [`TrackSize::Fixed`] accetta solo `px` e `rem` — niente `%`, `vw`, `cqw`,
//! che al momento di validare non esistono ancora — ogni contenitore ha
//! esattamente un figlio [`TrackSize::Fill`] che assorbe il resto, e la somma
//! dei fissi fratelli è confrontata con [`SHELL_FIXED_BUDGET`].
//!
//! # Flex e non grid, deliberatamente
//!
//! `grid-template-columns` accoppia il template al numero e all'ordine dei
//! figli: appena un widget si nasconde — la terza colonna chiusa, la barra della
//! selezione al posto del lettore — la corrispondenza tracce↔figli si rompe, e
//! la visibilità di questi widget è **stato dell'app**, non dato della skin.
//! Flex non ha quell'accoppiamento: un figlio che non c'è semplicemente non è
//! nel flusso. L'unico vantaggio vero di grid — le aree nominate, che possono
//! sovrapporsi — è esattamente la capacità che qui si vuole rendere
//! inesprimibile.
//!
//! # Qui non si scrive CSS
//!
//! Come in [`crate::parts`]: questo modulo è vocabolario, registro e regole.
//! Ogni carattere di CSS esce da [`crate::compile`], e la lettura del JSON sta
//! in [`crate::document`] con tutti gli altri parser.

use crate::document::{PlayerLayout, SidebarLayout};
use crate::parts::{PartDef, part};
use crate::values::{Length, LengthUnit, num};

// ── I tetti ─────────────────────────────────────────────────────────────────

/// Quanto può essere profondo l'albero.
///
/// Lo scafale vero è profondo 4. Sei lascia spazio a una skin che ha idee, e
/// non abbastanza perché la ricorsione del renderer diventi una domanda.
pub const MAX_SHELL_DEPTH: usize = 6;

/// Quanti nodi può avere l'albero.
///
/// Circa quattro volte l'albero di serie. Non è una difesa dalla lentezza — 64
/// `<div>` non sono niente — è una difesa dal fatto che ogni nodo è una regola
/// CSS in più in un foglio che viene rigenerato a ogni battuta nello Studio.
pub const MAX_SHELL_NODES: usize = 64;

/// Il costo massimo dello scafale, oltre il quale è un avviso.
///
/// È un budget separato da quello delle superfici ([`crate::SURFACE_COST_BUDGET`]):
/// lì si conta quanto costa **disegnare** una superficie, qui quanti pezzi
/// dell'app sono montati insieme. L'albero di serie costa 3.
pub const SHELL_COST_BUDGET: u32 = 24;

/// La somma massima delle misure fisse fra fratelli, in pixel.
///
/// Scelto contro il minimo di finestra di 880px perché al `Fill` restino almeno
/// 240px, che è la larghezza sotto la quale un elenco di brani smette di essere
/// un elenco di brani.
pub const SHELL_FIXED_BUDGET: f64 = 640.0;

/// La misura fissa più piccola che ha senso, in pixel.
pub const FIXED_MIN: f64 = 24.0;

/// La misura fissa più grande ammessa, in pixel.
pub const FIXED_MAX: f64 = 480.0;

/// Quanti pixel vale un `rem`, per poter sommare prima di disegnare.
///
/// È la misura del guscio, non una preferenza dell'utente: `html` non cambia
/// dimensione carattere in questa finestra. Serve solo alla validazione — il
/// CSS emesso conserva il `rem`, che è il motivo per cui accettarlo ha senso.
const REM_IN_PX: f64 = 16.0;

// ── Il vocabolario ──────────────────────────────────────────────────────────

/// Come una zona dispone i suoi figli.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneKind {
    /// Uno accanto all'altro.
    Row,
    /// Uno sotto l'altro.
    Column,
    /// Uno sotto l'altro, e quel che avanza scorre.
    Scroll,
}

impl ZoneKind {
    /// Come si scrive nel documento.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Row => "row",
            Self::Column => "column",
            Self::Scroll => "scroll",
        }
    }

    /// Dispone in orizzontale.
    #[must_use]
    pub const fn is_row(self) -> bool {
        matches!(self, Self::Row)
    }

    /// Tutte, nell'ordine in cui compaiono nell'editor.
    pub const ALL: &'static [Self] = &[Self::Row, Self::Column, Self::Scroll];
}

/// Quanto spazio prende un nodo lungo l'asse della zona che lo contiene.
///
/// Tre casi e non un numero libero: `Hug` è «quanto gli serve», `Fill` è «tutto
/// quel che resta», `Fixed` è una misura che si può sommare. Non c'è un quarto
/// caso, e in particolare non c'è la percentuale — che è precisamente il valore
/// che rende indecidibile la somma.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackSize {
    /// Quanto gli serve.
    Hug,
    /// Tutto quel che resta. Esattamente uno per contenitore.
    Fill,
    /// Una misura fissa, in `px` o `rem`.
    Fixed(Length),
}

impl TrackSize {
    /// È la traccia elastica.
    #[must_use]
    pub const fn is_fill(self) -> bool {
        matches!(self, Self::Fill)
    }

    /// La misura in pixel, quando è fissa.
    ///
    /// `None` per `Hug` e `Fill`, che non sono numeri, e per un'unità che al
    /// momento di validare non ha ancora un valore — il che è il modo in cui
    /// questa funzione dice «questa misura non si può sommare».
    #[must_use]
    pub fn in_px(self) -> Option<f64> {
        match self {
            Self::Hug | Self::Fill => None,
            Self::Fixed(length) => match length.unit {
                LengthUnit::Px => Some(length.value),
                LengthUnit::Rem => Some(length.value * REM_IN_PX),
                _ => None,
            },
        }
    }
}

/// L'aria fra i figli di una zona.
///
/// Un **gradino**, non una lunghezza. `--spazio-1..5` sta in `stile.css` e non
/// è un token *apposta*: la sua nota dice che una skin che potesse cambiare la
/// spaziatura potrebbe far uscire le cose dallo schermo. La lettura onesta di
/// quella regola non è «l'aria non si tocca», è «l'aria non si scrive come
/// numero»: qui una skin sceglie una parola fra sei, e il compilatore emette
/// `var(--spazio-N)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapStep {
    /// Attaccati.
    None,
    /// `--spazio-1`.
    Xs,
    /// `--spazio-2`.
    S,
    /// `--spazio-3`.
    M,
    /// `--spazio-4`.
    L,
    /// `--spazio-5`.
    Xl,
}

impl GapStep {
    /// Come si scrive nel documento.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Xs => "xs",
            Self::S => "s",
            Self::M => "m",
            Self::L => "l",
            Self::Xl => "xl",
        }
    }

    /// Il numero della variabile `--spazio-N`, quando non è zero.
    #[must_use]
    pub const fn gradino(self) -> Option<u8> {
        match self {
            Self::None => None,
            Self::Xs => Some(1),
            Self::S => Some(2),
            Self::M => Some(3),
            Self::L => Some(4),
            Self::Xl => Some(5),
        }
    }

    /// Tutti, dal più stretto.
    pub const ALL: &'static [Self] = &[Self::None, Self::Xs, Self::S, Self::M, Self::L, Self::Xl];
}

/// Come i figli si allineano sull'asse **trasverso** a quello della zona.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// In cima, o a sinistra.
    Start,
    /// In mezzo.
    Center,
    /// In fondo, o a destra.
    End,
    /// Tirati per tutta la misura.
    Stretch,
}

impl Align {
    /// Come si scrive nel documento, che è anche il valore CSS.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Center => "center",
            Self::End => "end",
            Self::Stretch => "stretch",
        }
    }

    /// Tutti.
    pub const ALL: &'static [Self] = &[Self::Start, Self::Center, Self::End, Self::Stretch];
}

/// Come i figli si distribuiscono lungo l'asse della zona.
///
/// `Between` c'è e `around`/`evenly` no: sono tre modi di dire la stessa cosa
/// con differenze che nessuno riesce a prevedere guardando il nome, e un
/// vocabolario in cui tre voci su quattro si scelgono a tentativi è un
/// vocabolario più lungo, non più espressivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spread {
    /// Tutti all'inizio.
    Start,
    /// Tutti in mezzo.
    Center,
    /// Tutti alla fine.
    End,
    /// Il primo all'inizio, l'ultimo alla fine, l'aria in mezzo.
    Between,
}

impl Spread {
    /// Come si scrive nel documento.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Center => "center",
            Self::End => "end",
            Self::Between => "between",
        }
    }

    /// Il valore di `justify-content`.
    #[must_use]
    pub const fn css(self) -> &'static str {
        match self {
            Self::Start => "flex-start",
            Self::Center => "center",
            Self::End => "flex-end",
            Self::Between => "space-between",
        }
    }

    /// Tutti.
    pub const ALL: &'static [Self] = &[Self::Start, Self::Center, Self::End, Self::Between];
}

// ── Il registro dei widget ──────────────────────────────────────────────────

/// Dove sta un widget, per raggrupparlo nella tavolozza dell'editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetGroup {
    /// La cornice della finestra.
    Shell,
    /// La navigazione.
    Nav,
    /// Il contenuto di una pagina.
    Page,
    /// I comandi di riproduzione.
    Player,
    /// La coda e la selezione.
    Queue,
    /// Cosa sta suonando.
    Meta,
}

impl WidgetGroup {
    /// Come si chiama nell'editor.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Nav => "nav",
            Self::Page => "page",
            Self::Player => "player",
            Self::Queue => "queue",
            Self::Meta => "meta",
        }
    }

    /// Tutti, nell'ordine della tavolozza.
    pub const ALL: &'static [Self] = &[
        Self::Shell,
        Self::Nav,
        Self::Page,
        Self::Player,
        Self::Queue,
        Self::Meta,
    ];
}

/// Se togliere questo widget toglie l'applicazione.
///
/// È la forma della decisione «una skin non può togliere i comandi
/// essenziali»: non un avviso, un errore. Una skin che nasconde il tasto pausa
/// non è una skin brutta, è un'app rotta che sembra un bug dell'app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Essential {
    /// Si può togliere.
    No,
    /// Deve esserci.
    Yes,
    /// Almeno uno del gruppo deve esserci.
    ///
    /// Serve dove la stessa funzione ha più modi di stare in una finestra: la
    /// navigazione può essere una barra laterale **o** una barra in fondo, e
    /// pretenderle entrambe sarebbe pretendere una finestra sola.
    OneOf(&'static str),
}

/// Cosa può valere una manopola di widget.
///
/// **Non ci sono lunghezze**, e non è una dimenticanza: la geometria è del
/// motore, e una manopola che accettasse una misura sarebbe la porta di
/// servizio da cui rientra tutto ciò che il vocabolario delle zone tiene fuori.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionKind {
    /// Acceso o spento.
    Flag {
        /// Il valore quando non è dichiarato.
        default: bool,
    },
    /// Una parola fra quelle ammesse.
    Word {
        /// Le parole ammesse.
        allowed: &'static [&'static str],
        /// La parola di serie, che è sempre una di `allowed`.
        default: &'static str,
    },
    /// Un intero in un intervallo chiuso.
    Count {
        /// Il minimo.
        min: u32,
        /// Il massimo.
        max: u32,
        /// Il valore di serie.
        default: u32,
    },
}

/// Una manopola di un widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidgetOption {
    /// Come si chiama nel documento.
    pub name: &'static str,
    /// Cosa può valere.
    pub kind: OptionKind,
    /// A cosa serve, per l'editor.
    pub description: &'static str,
}

/// Il valore di una manopola, già validato.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionValue {
    /// Acceso o spento.
    Flag(bool),
    /// Una delle parole ammesse. È `&'static` perché viene dalla tabella.
    Word(&'static str),
    /// Un intero nell'intervallo.
    Count(u32),
}

/// Un widget del registro.
///
/// Non è `Eq`: `natural` può essere una misura, e una misura è un `f64`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WidgetDef {
    /// Il nome nel documento.
    ///
    /// Non si rinomina: sta nelle skin già scritte, come i nomi delle parti.
    pub name: &'static str,
    /// Dove sta, nella tavolozza.
    pub group: WidgetGroup,
    /// A cosa serve.
    pub description: &'static str,
    /// La classe del registro delle parti che questo widget porta, se ne porta una.
    ///
    /// È il ponte fra i due registri: chi ridipinge `player-shell` in `parts`
    /// ridipinge il widget `player`, e l'editor può dirlo invece di lasciarlo
    /// scoprire.
    pub part: Option<&'static str>,
    /// Il buco in cui la finestra infila il suo contenuto.
    ///
    /// I due widget che ce l'hanno rendono quel che l'**app** decide — quale
    /// pagina si sta guardando — e non quel che decide la skin. L'albero dice
    /// dove va la pagina; quale pagina sia non è cosa di cui una skin abbia
    /// titolo.
    pub slot: Option<&'static str>,
    /// Togliendolo, cosa si rompe.
    pub essential: Essential,
    /// Ne esiste al massimo uno.
    pub singleton: bool,
    /// In quali zone ci sta.
    pub fits: &'static [ZoneKind],
    /// La misura che prende quando il documento non ne dichiara una.
    pub natural: TrackSize,
    /// Quanto pesa nel budget dello scafale.
    pub cost: u32,
    /// Le manopole.
    pub options: &'static [WidgetOption],
}

macro_rules! widget {
    (
        $name:literal, $group:ident, $part:expr, $slot:expr,
        $essential:expr, $singleton:literal, $fits:expr, $natural:expr, $cost:literal,
        $options:expr, $desc:literal
    ) => {
        WidgetDef {
            name: $name,
            group: WidgetGroup::$group,
            description: $desc,
            part: $part,
            slot: $slot,
            essential: $essential,
            singleton: $singleton,
            fits: $fits,
            natural: $natural,
            cost: $cost,
            options: $options,
        }
    };
}

const IN_RIGA: &[ZoneKind] = &[ZoneKind::Row];
const IN_COLONNA: &[ZoneKind] = &[ZoneKind::Column, ZoneKind::Scroll];
const OVUNQUE: &[ZoneKind] = &[ZoneKind::Row, ZoneKind::Column, ZoneKind::Scroll];
const NESSUNA: &[WidgetOption] = &[];

const OPZIONI_NAVIGAZIONE: &[WidgetOption] = &[WidgetOption {
    name: "wide",
    kind: OptionKind::Flag { default: false },
    description: "Parte larga, con le etichette accanto alle icone.",
}];

/// Le manopole del trasporto.
///
/// Sono la duplicazione documentata di `parti/Trasporto.tsx` diventata una
/// manopola: la barra del lettore mostra cinque tasti, la colonna cinque, la
/// schermata grande cinque con le icone più grandi. Quel che davvero cambiava
/// fra i tre era la **taglia**, e mescolare o ripetere si potevano già
/// nascondere — solo che la scelta era scritta in TypeScript invece che qui.
const OPZIONI_TRASPORTO: &[WidgetOption] = &[
    WidgetOption {
        name: "shuffle",
        kind: OptionKind::Flag { default: true },
        description: "Il tasto che mescola la coda.",
    },
    WidgetOption {
        name: "repeat",
        kind: OptionKind::Flag { default: true },
        description: "Il tasto che ripete, nei suoi tre stati.",
    },
    WidgetOption {
        name: "size",
        kind: OptionKind::Word {
            allowed: &["bar", "column", "large"],
            default: "bar",
        },
        description: "Quanto sono grandi le icone, e se hanno un suggerimento.",
    },
];

const OPZIONI_CURSORE: &[WidgetOption] = &[WidgetOption {
    name: "times",
    kind: OptionKind::Flag { default: true },
    description: "I due tempi ai lati della barra.",
}];

const OPZIONI_ORA: &[WidgetOption] = &[
    WidgetOption {
        name: "cover",
        kind: OptionKind::Flag { default: true },
        description: "La copertina in miniatura.",
    },
    WidgetOption {
        name: "heart",
        kind: OptionKind::Flag { default: true },
        description: "Il cuore dei preferiti.",
    },
];

const OPZIONI_GIUDIZIO: &[WidgetOption] = &[
    WidgetOption {
        name: "stars",
        kind: OptionKind::Flag { default: true },
        description: "Le cinque stelle.",
    },
    WidgetOption {
        name: "volume",
        kind: OptionKind::Flag { default: true },
        description: "Il cursore del volume.",
    },
];

/// Il gruppo che tiene in piedi la navigazione.
const G_NAV: Essential = Essential::OneOf("navigation");

/// Il gruppo che tiene in piedi i comandi di riproduzione.
const G_COMANDI: Essential = Essential::OneOf("playback");

/// I widget che uno scafale può montare.
///
/// Una riga per widget, come [`crate::parts::PARTS`] e [`crate::tokens::TOKENS`].
/// Ogni riga corrisponde a un componente che **esiste**: un registro che
/// promette widget che React non sa rendere è un registro che mente, e lo
/// scopre chi scrive la skin invece di chi la scrive qui.
#[rustfmt::skip]
pub static WIDGETS: &[WidgetDef] = &[
    // ── Cornice ─────────────────────────────────────────────────────────────
    widget!("ambient", Shell, Some("ambient-backdrop"), None,
        Essential::No, true, OVUNQUE, TrackSize::Hug, 0, NESSUNA,
        "Il fondo dietro al contenuto, che prende il colore dalla copertina."),

    // ── Navigazione ─────────────────────────────────────────────────────────
    widget!("navigation", Nav, None, None,
        G_NAV, true, IN_RIGA, TrackSize::Hug, 1, OPZIONI_NAVIGAZIONE,
        "La barra laterale, con le viste e le playlist."),
    widget!("bottom-nav", Nav, Some("bottom-nav"), None,
        G_NAV, true, IN_COLONNA, TrackSize::Hug, 1, NESSUNA,
        "La navigazione in fondo, per finestre strette."),

    // ── Pagina ──────────────────────────────────────────────────────────────
    widget!("page-header", Page, Some("page-header"), Some("intestazione"),
        Essential::No, true, IN_COLONNA, TrackSize::Hug, 0, NESSUNA,
        "L'intestazione della pagina aperta: titolo, ricerca, azioni."),
    widget!("content", Page, None, Some("contenuto"),
        Essential::Yes, true, OVUNQUE, TrackSize::Fill, 0, NESSUNA,
        "La pagina aperta. Toglierlo toglie l'applicazione."),

    // ── Riproduzione ────────────────────────────────────────────────────────
    widget!("player", Player, Some("player-shell"), None,
        G_COMANDI, true, OVUNQUE, TrackSize::Hug, 2, NESSUNA,
        "La barra del lettore: cosa suona, i comandi, l'avanzamento."),
    widget!("column", Player, None, None,
        G_COMANDI, true, IN_RIGA, TrackSize::Fixed(Length::px(348.0)), 2, NESSUNA,
        "La terza colonna: copertina grande, comandi e coda, tutto insieme."),
    widget!("transport", Player, Some("np-transport"), None,
        G_COMANDI, false, OVUNQUE, TrackSize::Hug, 1, OPZIONI_TRASPORTO,
        "Precedente, play/pausa, successivo, e le due modalità."),
    widget!("scrubber", Player, Some("player-progress"), None,
        Essential::No, false, OVUNQUE, TrackSize::Fill, 1, OPZIONI_CURSORE,
        "La barra di avanzamento, con i due tempi."),

    // ── Coda e selezione ────────────────────────────────────────────────────
    widget!("queue", Queue, Some("queue-list"), None,
        Essential::No, true, OVUNQUE, TrackSize::Fixed(Length::px(320.0)), 1, NESSUNA,
        "La coda di riproduzione, riordinabile."),
    widget!("selection-bar", Queue, Some("selection-bar"), None,
        Essential::No, true, OVUNQUE, TrackSize::Hug, 1, NESSUNA,
        "La barra che appare quando ci sono più brani selezionati."),

    // ── Cosa suona ──────────────────────────────────────────────────────────
    widget!("now-playing", Meta, None, None,
        Essential::No, false, OVUNQUE, TrackSize::Hug, 1, OPZIONI_ORA,
        "Copertina piccola, titolo e artista di quel che suona."),
    widget!("cover-large", Meta, Some("np-art"), None,
        Essential::No, false, OVUNQUE, TrackSize::Fill, 1, NESSUNA,
        "La copertina in grande."),
    widget!("rating", Meta, None, None,
        Essential::No, false, OVUNQUE, TrackSize::Hug, 0, OPZIONI_GIUDIZIO,
        "Le stelle e il volume."),
];

/// Il widget con questo nome, se esiste.
#[must_use]
pub fn widget(name: &str) -> Option<&'static WidgetDef> {
    WIDGETS.iter().find(|def| def.name == name)
}

/// I widget di un gruppo.
#[must_use]
pub fn widgets_in_group(group: WidgetGroup) -> Vec<&'static WidgetDef> {
    WIDGETS.iter().filter(|def| def.group == group).collect()
}

/// Nomi vicini a quello scritto, per il messaggio d'errore.
#[must_use]
pub fn nearest_widgets(unknown: &str) -> Vec<&'static str> {
    crate::vicini::vicini(unknown, WIDGETS.iter().map(|def| def.name))
}

/// I widget di un gruppo di essenzialità, per dire *quali* mancano.
#[must_use]
pub(crate) fn widgets_essenziali(gruppo: &str) -> Vec<&'static str> {
    WIDGETS
        .iter()
        .filter(|def| matches!(def.essential, Essential::OneOf(g) if g == gruppo))
        .map(|def| def.name)
        .collect()
}

impl WidgetDef {
    /// La manopola con questo nome, se questo widget ce l'ha.
    #[must_use]
    pub fn option(&self, name: &str) -> Option<&'static WidgetOption> {
        self.options.iter().find(|o| o.name == name)
    }

    /// Nomi di manopola vicini a quello scritto.
    #[must_use]
    pub fn nearest_options(&self, unknown: &str) -> Vec<&'static str> {
        crate::vicini::vicini(unknown, self.options.iter().map(|o| o.name))
    }
}

// ── L'albero ────────────────────────────────────────────────────────────────

/// Un widget montato, con le sue manopole.
#[derive(Debug, Clone, PartialEq)]
pub struct WidgetInstance {
    /// Quale widget.
    pub def: &'static WidgetDef,
    /// Quanto spazio prende.
    pub size: TrackSize,
    /// Le manopole, **tutte**, coi difetti già applicati.
    ///
    /// Già applicati e non lasciati impliciti: chi legge questa struttura — il
    /// compilatore, l'IPC, l'editor — non deve conoscere la tabella dei difetti
    /// per sapere cosa sta guardando.
    pub options: Vec<(&'static WidgetOption, OptionValue)>,
}

impl WidgetInstance {
    /// Il widget col suo aspetto di serie.
    #[must_use]
    pub fn nuovo(def: &'static WidgetDef) -> Self {
        Self {
            def,
            size: def.natural,
            options: def
                .options
                .iter()
                .map(|o| {
                    let valore = match o.kind {
                        OptionKind::Flag { default } => OptionValue::Flag(default),
                        OptionKind::Word { default, .. } => OptionValue::Word(default),
                        OptionKind::Count { default, .. } => OptionValue::Count(default),
                    };
                    (o, valore)
                })
                .collect(),
        }
    }

    /// Lo stesso widget con un'altra misura.
    ///
    /// Serve a chi lo monta sopra il contenuto invece che accanto: `natural` è
    /// la misura del widget quando prende una traccia sua, e per un widget che
    /// galleggia quella traccia non esiste. Dichiararla lo stesso la farebbe
    /// entrare nella somma dei fissi fratelli, cioè far fallire la validazione
    /// per uno spazio che nessuno occupa.
    #[must_use]
    const fn a_misura(mut self, size: TrackSize) -> Self {
        self.size = size;
        self
    }

    /// Lo stesso widget con una manopola cambiata. Per costruire i difetti.
    #[must_use]
    fn con(mut self, nome: &str, valore: OptionValue) -> Self {
        if let Some(voce) = self.options.iter_mut().find(|(o, _)| o.name == nome) {
            voce.1 = valore;
        }
        self
    }

    /// Il valore di una manopola.
    #[must_use]
    pub fn option(&self, name: &str) -> Option<OptionValue> {
        self.options
            .iter()
            .find(|(o, _)| o.name == name)
            .map(|(_, v)| *v)
    }
}

/// Un nodo dell'albero: una zona che contiene, o un widget che è.
#[derive(Debug, Clone, PartialEq)]
pub enum LayoutNode {
    /// Un contenitore.
    Zone(LayoutZone),
    /// Una foglia.
    Widget(WidgetInstance),
}

impl LayoutNode {
    /// Quanto spazio prende questo nodo.
    #[must_use]
    pub const fn size(&self) -> TrackSize {
        match self {
            Self::Zone(zona) => zona.size,
            Self::Widget(istanza) => istanza.size,
        }
    }
}

/// Un contenitore.
///
/// Non ha coordinate, e la loro assenza è il punto: vedi la nota del modulo.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutZone {
    /// In che verso dispone.
    pub kind: ZoneKind,
    /// Quanto spazio prende nella zona che la contiene.
    pub size: TrackSize,
    /// L'aria fra i figli.
    pub gap: GapStep,
    /// Come allinea sull'asse trasverso.
    pub align: Align,
    /// Come distribuisce sull'asse principale.
    pub spread: Spread,
    /// La parte ridipingibile che questa zona porta, se ne porta una.
    ///
    /// È un `&'static PartDef` e non una stringa: quel che è già stato validato
    /// non si valida due volte, e a valle non c'è un nome che possa non esistere.
    pub part: Option<&'static PartDef>,
    /// Da quale prefab viene questa zona, quando viene da un prefab.
    ///
    /// L'espansione avviene durante la validazione: da qui in giù un albero è
    /// fatto solo di zone e widget, e nessuno deve conoscere i prefab per
    /// leggerlo. Questo campo è l'unica traccia che resta, e serve a due cose
    /// che non si potrebbero fare senza: dire quali prefab sono dichiarati e mai
    /// usati, e riportare un errore *dentro* un prefab al sito di
    /// **dichiarazione** invece che a ogni sito d'uso — così chi lo corregge lo
    /// corregge una volta.
    pub from_prefab: Option<String>,
    /// Cosa contiene.
    pub children: Vec<LayoutNode>,
}

impl LayoutZone {
    /// Una zona vuota coi valori di serie.
    #[must_use]
    pub const fn nuova(kind: ZoneKind, size: TrackSize) -> Self {
        Self {
            kind,
            size,
            gap: GapStep::None,
            align: Align::Stretch,
            spread: Spread::Start,
            part: None,
            from_prefab: None,
            children: Vec::new(),
        }
    }

    /// La stessa zona con questi figli.
    #[must_use]
    fn con(mut self, children: Vec<LayoutNode>) -> Self {
        self.children = children;
        self
    }

    /// La stessa zona che porta questa parte.
    ///
    /// Il nome viene dal registro delle parti; se non c'è, la zona semplicemente
    /// non porta niente. Serve solo a costruire l'albero di serie, dove i nomi
    /// sono letterali scritti qui e il caso «non c'è» non capita.
    #[must_use]
    fn parte(mut self, nome: &str) -> Self {
        self.part = part(nome);
        self
    }

    /// Quanti nodi ci sono, contando questo.
    #[must_use]
    pub fn conta_nodi(&self) -> usize {
        1 + self
            .children
            .iter()
            .map(|figlio| match figlio {
                LayoutNode::Zone(zona) => zona.conta_nodi(),
                LayoutNode::Widget(_) => 1,
            })
            .sum::<usize>()
    }

    /// Il costo sommato dei widget montati.
    #[must_use]
    pub fn costo(&self) -> u32 {
        self.children
            .iter()
            .map(|figlio| match figlio {
                LayoutNode::Zone(zona) => zona.costo(),
                LayoutNode::Widget(istanza) => istanza.def.cost,
            })
            .sum()
    }

    /// Tutti i widget montati, in ordine di lettura.
    #[must_use]
    pub fn widgets(&self) -> Vec<&WidgetInstance> {
        let mut trovati = Vec::new();
        self.raccogli(&mut trovati);
        trovati
    }

    fn raccogli<'a>(&'a self, dentro: &mut Vec<&'a WidgetInstance>) {
        for figlio in &self.children {
            match figlio {
                LayoutNode::Zone(zona) => zona.raccogli(dentro),
                LayoutNode::Widget(istanza) => dentro.push(istanza),
            }
        }
    }

    /// C'è un widget con questo nome, da qualche parte.
    #[must_use]
    pub fn monta(&self, nome: &str) -> bool {
        self.widgets().iter().any(|w| w.def.name == nome)
    }

    /// I prefab da cui viene qualcosa di questo albero.
    #[must_use]
    pub fn prefabs_used(&self) -> Vec<&str> {
        let mut nomi: Vec<&str> = Vec::new();
        if let Some(nome) = self.from_prefab.as_ref() {
            nomi.push(nome);
        }
        for figlio in &self.children {
            if let LayoutNode::Zone(zona) = figlio {
                nomi.extend(zona.prefabs_used());
            }
        }
        nomi
    }
}

// ── L'albero di serie ───────────────────────────────────────────────────────

/// L'albero che una skin riceve quando non ne dichiara uno.
///
/// È scritto qui una volta sola e mai duplicato in TypeScript: `SkinIpc` porta
/// sempre uno scafale popolato, e il renderer non ha un caso «manca».
///
/// Riproduce la finestra vera: la navigazione a sinistra, il contenuto in
/// mezzo con la sua intestazione, la terza colonna a destra, il lettore in
/// fondo. La terza colonna e il lettore sono **entrambi** nell'albero anche se
/// non si vedono mai insieme — quale dei due si veda è stato dell'app, e
/// l'albero dice dove andrebbero, non se ci sono adesso.
///
/// # I due che galleggiano accanto al lettore
///
/// `queue` e `selection-bar` stanno qui per la stessa ragione, ed erano gli
/// unici due widget del registro che l'albero di serie non montava. Non era una
/// scelta: era un buco, e si vedeva. Il tasto «Coda» della barra si accendeva e
/// non apriva niente; scegliere delle righe faceva **sparire** il lettore —
/// perché `player` si nasconde quando c'è una selezione, per lasciare il posto
/// a una barra che nessuno aveva montato — e al suo posto restava il vuoto.
/// Sono entrambi `position: fixed`, quindi il loro posto nel flusso è un
/// riquadro alto zero e metterli qui non sposta niente.
///
/// # I due argomenti
///
/// `player` e `sidebar` sono le vecchie manopole di `layout`, che il formato
/// dichiarava e nessuno leggeva. Non spariscono e non diventano geometria:
/// diventano **selettori di una variante** di quest'albero, e per costruzione
/// non possono mai contraddirlo — si applicano soltanto quando `layout.shell`
/// non c'è.
#[must_use]
pub fn default_shell(player: PlayerLayout, sidebar: SidebarLayout) -> LayoutZone {
    let contenuto = LayoutZone::nuova(ZoneKind::Column, TrackSize::Fill).con(
        [monta("page-header"), monta("content")]
            .into_iter()
            .flatten()
            .collect(),
    );

    let laterale = (sidebar != SidebarLayout::Hidden)
        .then(|| {
            monta_con(
                "navigation",
                "wide",
                OptionValue::Flag(sidebar == SidebarLayout::Expanded),
            )
        })
        .flatten();

    let riga: Vec<LayoutNode> = [laterale, Some(LayoutNode::Zone(contenuto)), monta("column")]
        .into_iter()
        .flatten()
        .collect();

    // Con la navigazione nascosta il posto della barra in fondo è la radice, e
    // la radice deve quindi essere una colonna: è la ragione per cui
    // `SidebarLayout::Hidden` non ha bisogno di un caso speciale altrove —
    // `Essential::OneOf("navigation")` è soddisfatto da `bottom-nav`.
    let in_colonna = sidebar == SidebarLayout::Hidden || player == PlayerLayout::BottomBar;

    // I tre che galleggiano viaggiano insieme: sono le tre forme che prende la
    // stessa domanda — «cosa sta suonando, cosa sto per fare» — e quale delle
    // tre si veda lo decide lo stato dell'app, non l'albero.
    //
    // `queue` va montata a `Hug` e non alla sua misura naturale: quei 320px
    // sono la larghezza che le serve **quando ha una colonna sua**, e qui non
    // ce l'ha. Lasciandoli, la somma dei fissi fratelli — barra 240, colonna
    // 348, coda 320 — sfonderebbe `SHELL_FIXED_BUDGET` per uno spazio che
    // nessuno occupa.
    let galleggianti = || {
        [
            monta("player"),
            monta_a_misura("queue", TrackSize::Hug),
            monta("selection-bar"),
        ]
        .into_iter()
        .flatten()
    };

    if !in_colonna {
        let figli: Vec<LayoutNode> = riga.into_iter().chain(galleggianti()).collect();
        return LayoutZone::nuova(ZoneKind::Row, TrackSize::Fill)
            .parte("app-shell")
            .con(figli);
    }

    let dentro = LayoutZone::nuova(ZoneKind::Row, TrackSize::Fill).con(riga);
    let barra = (sidebar == SidebarLayout::Hidden)
        .then(|| monta("bottom-nav"))
        .flatten();
    let figli: Vec<LayoutNode> = std::iter::once(LayoutNode::Zone(dentro))
        .chain(galleggianti())
        .chain(barra)
        .collect();
    LayoutZone::nuova(ZoneKind::Column, TrackSize::Fill)
        .parte("app-shell")
        .con(figli)
}

/// Un widget del registro, montato coi suoi difetti.
///
/// `None` quando il nome non è nel registro, che vuol dire che l'albero di
/// serie e la tabella sono andati in disaccordo. **Non è un panico**: è la
/// stessa scelta di `esempio()` nello Studio — una tabella che non conosce più
/// una voce perde la riga invece di mentire — e il buco che ne risulta lo
/// trova `verifica()`, perché un albero di serie senza `content` è
/// esattamente il caso che le regole essenziali rifiutano. Il test
/// `l_albero_di_serie_e_valido` lo vede prima di chiunque altro.
fn monta(nome: &str) -> Option<LayoutNode> {
    widget(nome).map(|def| LayoutNode::Widget(WidgetInstance::nuovo(def)))
}

/// Come [`monta`], con un'altra misura. Per i widget che galleggiano.
fn monta_a_misura(nome: &str, size: TrackSize) -> Option<LayoutNode> {
    widget(nome).map(|def| LayoutNode::Widget(WidgetInstance::nuovo(def).a_misura(size)))
}

/// Come [`monta`], con una manopola già girata.
fn monta_con(nome: &str, opzione: &str, valore: OptionValue) -> Option<LayoutNode> {
    widget(nome).map(|def| LayoutNode::Widget(WidgetInstance::nuovo(def).con(opzione, valore)))
}

// ── Le regole ───────────────────────────────────────────────────────────────

/// Un problema dello scafale: dove, e cosa.
///
/// Non è un [`crate::SkinIssue`]: questo modulo non conosce il catalogo degli
/// errori né i percorsi JSON del documento. Restituisce il percorso relativo
/// alla radice dello scafale, e [`crate::document`] ci mette il prefisso e il
/// codice — così la stessa regola serve sia al documento sia, un giorno, a un
/// albero costruito dall'editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellIssue {
    /// Il percorso dentro lo scafale: `children.0.children.1`, o vuoto per la radice.
    ///
    /// Quando [`Self::prefab`] è popolato, è relativo alla **dichiarazione** del
    /// prefab e non al punto in cui è usato.
    pub path: String,
    /// Cosa non va.
    pub message: String,
    /// Quale widget essenziale manca, quando è quello il problema.
    ///
    /// Serve a [`crate::document`] per scegliere il codice del catalogo: un
    /// widget essenziale mancante ha il suo, tutto il resto è un manifest non
    /// valido come gli altri.
    pub missing: Option<String>,
    /// Il problema è dentro un prefab, e questo è il suo nome.
    ///
    /// Chi lo corregge lo corregge **una volta**: un prefab montato in tre punti
    /// produrrebbe altrimenti tre errori identici, e correggerne uno ne
    /// lascerebbe due che sono lo stesso.
    pub prefab: Option<String>,
}

fn guasto(path: &str, message: impl Into<String>) -> ShellIssue {
    ShellIssue {
        path: path.to_owned(),
        message: message.into(),
        missing: None,
        prefab: None,
    }
}

fn giu(path: &str, figlio: usize) -> String {
    if path.is_empty() {
        return format!("children.{figlio}");
    }
    format!("{path}.children.{figlio}")
}

/// Tutto quel che non va in uno scafale, in una passata sola.
///
/// **Accumula invece di fermarsi**, come `tokens` e `parts`: chi sta
/// costruendo un layout corregge in una passata sola invece di scoprire un
/// errore per esecuzione. Un ramo malformato smette di scendere, i fratelli
/// proseguono.
#[must_use]
pub fn verifica(radice: &LayoutZone) -> Vec<ShellIssue> {
    let mut problemi = Vec::new();

    let nodi = radice.conta_nodi();
    if nodi > MAX_SHELL_NODES {
        problemi.push(guasto(
            "",
            format!("lo scafale ha {nodi} nodi, e il massimo è {MAX_SHELL_NODES}"),
        ));
        // Oltre il tetto non si scende: un albero fuori misura produrrebbe
        // centinaia di messaggi che dicono tutti la stessa cosa.
        return problemi;
    }

    let mut visti: Vec<&'static str> = Vec::new();
    zona(radice, "", None, 1, &mut visti, &mut problemi);
    essenziali(&visti, &mut problemi);

    // Un prefab montato in tre punti produce tre volte gli stessi problemi:
    // sono lo stesso problema, e correggerne uno li corregge tutti.
    let mut visti_prefab: Vec<(String, String)> = Vec::new();
    problemi.retain(|p| {
        let Some(nome) = p.prefab.as_ref() else {
            return true;
        };
        let chiave = (nome.clone(), p.path.clone());
        if visti_prefab.contains(&chiave) {
            return false;
        }
        visti_prefab.push(chiave);
        true
    });

    problemi
}

fn zona(
    corrente: &LayoutZone,
    path: &str,
    prefab: Option<&str>,
    profondita: usize,
    visti: &mut Vec<&'static str>,
    problemi: &mut Vec<ShellIssue>,
) {
    // Entrando in un sottoalbero che viene da un prefab, il percorso riparte
    // dalla sua dichiarazione: è là che si corregge.
    let (path, prefab) = match corrente.from_prefab.as_deref() {
        Some(nome) => ("", Some(nome)),
        None => (path, prefab),
    };
    // I problemi di **questa** zona si raccolgono a parte e si marcano in un
    // colpo solo: i figli si marcano da sé scendendo, e mettere la marcatura in
    // ogni `push` sarebbe una riga da ricordarsi otto volte.
    let mut miei: Vec<ShellIssue> = Vec::new();
    let chiudi = |miei: Vec<ShellIssue>, problemi: &mut Vec<ShellIssue>| {
        problemi.extend(miei.into_iter().map(|mut g| {
            g.prefab = prefab.map(ToOwned::to_owned);
            g
        }));
    };

    if profondita > MAX_SHELL_DEPTH {
        miei.push(guasto(
            path,
            format!(
                "questa zona è annidata a {profondita}, e il massimo è {MAX_SHELL_DEPTH}: \
                 lo scafale vero è profondo 4"
            ),
        ));
        chiudi(miei, problemi);
        return;
    }

    if corrente.children.is_empty() {
        miei.push(guasto(
            path,
            "una zona senza figli è aria che nessuno ha chiesto: mettici qualcosa o toglila",
        ));
        chiudi(miei, problemi);
        return;
    }

    // Esattamente un `Fill`. Zero e due sono due errori diversi e meritano due
    // messaggi diversi: senza, il primo lascia una zona che non riempie niente
    // e il secondo una che si divide lo spazio in un modo che nessuno ha scelto.
    let elastici = corrente
        .children
        .iter()
        .filter(|figlio| figlio.size().is_fill())
        .count();
    match elastici {
        1 => {}
        0 => miei.push(guasto(
            path,
            "nessun figlio è «fill»: serve una traccia elastica che assorba \
             lo spazio che avanza, altrimenti la zona lascia un vuoto",
        )),
        quanti => miei.push(guasto(
            path,
            format!(
                "{quanti} figli sono «fill»: si dividono lo spazio in parti uguali, \
                 che non è una scelta che qualcuno abbia fatto. Uno solo"
            ),
        )),
    }

    // La somma dei fissi. Si può fare **perché** il vocabolario è chiuso: ogni
    // misura fissa è px o rem, cioè un numero che esiste già adesso.
    let somma: f64 = corrente
        .children
        .iter()
        .filter_map(|figlio| figlio.size().in_px())
        .sum();
    if somma > SHELL_FIXED_BUDGET {
        miei.push(guasto(
            path,
            format!(
                "le misure fisse di questa zona sommano {}px sul budget di {}px: \
                 al «fill» resterebbe meno di quanto serve a un elenco di brani",
                num(somma),
                num(SHELL_FIXED_BUDGET)
            ),
        ));
    }

    for (indice, figlio) in corrente.children.iter().enumerate() {
        let dentro = giu(path, indice);
        // La misura di un figlio è una scelta di **questa** zona, anche quando
        // il figlio viene da un prefab: è il sito d'uso a dichiararla.
        misura(figlio.size(), &dentro, &mut miei);
        if let LayoutNode::Widget(istanza) = figlio {
            foglia(istanza, corrente.kind, &dentro, visti, &mut miei);
        }
    }
    chiudi(miei, problemi);

    // Le zone figlie scendono **dopo**: si marcano da sé, e passano da
    // `problemi` invece che da `miei` perché il prefab da cui vengono potrebbe
    // essere un altro.
    for (indice, figlio) in corrente.children.iter().enumerate() {
        if let LayoutNode::Zone(sotto) = figlio {
            zona(
                sotto,
                &giu(path, indice),
                prefab,
                profondita + 1,
                visti,
                problemi,
            );
        }
    }
}

fn misura(size: TrackSize, path: &str, problemi: &mut Vec<ShellIssue>) {
    let TrackSize::Fixed(length) = size else {
        return;
    };
    let Some(px) = size.in_px() else {
        problemi.push(guasto(
            path,
            format!(
                "«{}» dipende da qualcosa che al momento di validare non esiste ancora: \
                 una misura fissa si scrive in px o in rem",
                crate::values::format_length(length)
            ),
        ));
        return;
    };
    if !(FIXED_MIN..=FIXED_MAX).contains(&px) {
        problemi.push(guasto(
            path,
            format!(
                "una misura fissa va da {}px a {}px, e questa è {}px",
                num(FIXED_MIN),
                num(FIXED_MAX),
                num(px)
            ),
        ));
    }
}

fn foglia(
    istanza: &WidgetInstance,
    dove: ZoneKind,
    path: &str,
    visti: &mut Vec<&'static str>,
    problemi: &mut Vec<ShellIssue>,
) {
    let def = istanza.def;

    if !def.fits.contains(&dove) {
        let ammesse: Vec<&str> = def.fits.iter().map(|k| k.as_str()).collect();
        problemi.push(guasto(
            path,
            format!(
                "«{}» non ci sta in una zona «{}»: ci sta in {}",
                def.name,
                dove.as_str(),
                ammesse.join(" o ")
            ),
        ));
    }

    // Il percorso è quello della **seconda** occorrenza: la prima è quella che
    // chi ha scritto la skin voleva, e mandarlo a correggere quella sarebbe
    // mandarlo nel posto sbagliato.
    if def.singleton && visti.contains(&def.name) {
        problemi.push(guasto(
            path,
            format!(
                "«{}» può stare in un posto solo, ed è già montato",
                def.name
            ),
        ));
    }
    visti.push(def.name);
}

fn essenziali(visti: &[&'static str], problemi: &mut Vec<ShellIssue>) {
    let mut gruppi: Vec<&'static str> = Vec::new();
    for def in WIDGETS {
        match def.essential {
            Essential::No => {}
            Essential::Yes => {
                if !visti.contains(&def.name) {
                    problemi.push(ShellIssue {
                        path: String::new(),
                        message: format!(
                            "«{}» deve esserci: {}",
                            def.name,
                            def.description.to_lowercase()
                        ),
                        missing: Some(def.name.to_owned()),
                        prefab: None,
                    });
                }
            }
            Essential::OneOf(gruppo) => {
                if !gruppi.contains(&gruppo) {
                    gruppi.push(gruppo);
                }
            }
        }
    }

    for gruppo in gruppi {
        let membri = widgets_essenziali(gruppo);
        if membri.iter().any(|nome| visti.contains(nome)) {
            continue;
        }
        let elenco: Vec<String> = membri.iter().map(|n| format!("«{n}»")).collect();
        problemi.push(ShellIssue {
            path: String::new(),
            message: format!(
                "lo scafale non monta nessuno di {}: senza, non c'è modo di \
                 comandare la riproduzione o di cambiare vista",
                elenco.join(", ")
            ),
            missing: Some(gruppo.to_owned()),
            prefab: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Un widget che *deve* stare nel registro, perché il nome è scritto qui
    /// sotto. Cade solo se il registro è stato cambiato senza guardare qui.
    fn atteso(nome: &str) -> &'static WidgetDef {
        widget(nome).expect("widget assente dal registro")
    }

    #[test]
    fn il_registro_non_ha_doppioni() {
        let mut visti = HashSet::new();
        for def in WIDGETS {
            assert!(visti.insert(def.name), "widget ripetuto: {}", def.name);
        }
    }

    #[test]
    fn i_gruppi_coprono_la_tabella() {
        for def in WIDGETS {
            assert!(
                WidgetGroup::ALL.contains(&def.group),
                "gruppo fuori elenco: {}",
                def.name
            );
        }
    }

    #[test]
    fn ogni_widget_ci_sta_da_qualche_parte() {
        for def in WIDGETS {
            assert!(
                !def.fits.is_empty(),
                "«{}» non ci sta in nessuna zona: è un widget che non si può montare",
                def.name
            );
        }
    }

    #[test]
    fn le_parti_dichiarate_esistono_nel_registro_delle_parti() {
        // È il ponte fra i due registri: se una parte viene rinominata, questo
        // test lo dice invece di lasciare un widget che punta al nulla.
        for def in WIDGETS {
            let Some(nome) = def.part else { continue };
            assert!(
                part(nome).is_some(),
                "«{}» dichiara la parte «{nome}», che non è nel registro",
                def.name
            );
        }
    }

    #[test]
    fn ogni_gruppo_essenziale_ha_almeno_un_membro() {
        // Un gruppo con zero membri sarebbe un requisito impossibile: nessuno
        // scafale potrebbe soddisfarlo, e ogni skin verrebbe rifiutata.
        for def in WIDGETS {
            let Essential::OneOf(gruppo) = def.essential else {
                continue;
            };
            assert!(
                !widgets_essenziali(gruppo).is_empty(),
                "il gruppo «{gruppo}» non ha membri"
            );
        }
    }

    #[test]
    fn i_difetti_delle_manopole_sono_ammessi() {
        for def in WIDGETS {
            for opzione in def.options {
                match opzione.kind {
                    OptionKind::Flag { .. } => {}
                    OptionKind::Word { allowed, default } => assert!(
                        allowed.contains(&default),
                        "«{}».{}: il difetto «{default}» non è fra gli ammessi",
                        def.name,
                        opzione.name
                    ),
                    OptionKind::Count { min, max, default } => {
                        assert!(min <= default && default <= max);
                    }
                }
            }
        }
    }

    #[test]
    fn l_albero_di_serie_e_valido() {
        // Ogni combinazione, non solo quella di serie: sono nove alberi che una
        // skin può ricevere senza scrivere una riga di `shell`, e nessuno dei
        // nove ha modo di essere provato a mano.
        for player in [
            PlayerLayout::Floating,
            PlayerLayout::BottomBar,
            PlayerLayout::Compact,
        ] {
            for sidebar in [
                SidebarLayout::Rail,
                SidebarLayout::Expanded,
                SidebarLayout::Hidden,
            ] {
                let albero = default_shell(player, sidebar);
                let problemi = verifica(&albero);
                assert!(problemi.is_empty(), "{player:?}/{sidebar:?}: {problemi:?}");
                assert!(albero.monta("content"));
            }
        }
    }

    #[test]
    fn l_albero_di_serie_monta_anche_quel_che_galleggia() {
        // La coda e la barra della selezione non si vedono quasi mai, e per
        // questo mancavano: nessuno se ne accorgeva guardando la finestra
        // ferma. Si accorgeva chi premeva «Coda» — il tasto si accendeva e non
        // apriva niente — e chi sceglieva delle righe, che vedeva sparire il
        // lettore senza niente al suo posto.
        for player in [
            PlayerLayout::Floating,
            PlayerLayout::BottomBar,
            PlayerLayout::Compact,
        ] {
            for sidebar in [
                SidebarLayout::Rail,
                SidebarLayout::Expanded,
                SidebarLayout::Hidden,
            ] {
                let albero = default_shell(player, sidebar);
                for nome in ["player", "queue", "selection-bar", "column"] {
                    assert!(albero.monta(nome), "{player:?}/{sidebar:?}: manca {nome}");
                }
            }
        }
    }

    #[test]
    fn l_albero_di_serie_sta_dentro_i_tetti() {
        let albero = default_shell(PlayerLayout::Floating, SidebarLayout::Rail);
        // Un quarto del tetto: l'albero di serie deve lasciare spazio a chi ne
        // scrive uno più ricco senza sbattere subito contro il massimo.
        assert!(albero.conta_nodi() <= 16, "{} nodi", albero.conta_nodi());
        assert!(
            albero.costo() <= SHELL_COST_BUDGET,
            "costo {}",
            albero.costo()
        );
    }

    /// Una radice minima che passa, da guastare in un punto solo.
    fn sana() -> LayoutZone {
        LayoutZone::nuova(ZoneKind::Row, TrackSize::Fill).con(vec![
            LayoutNode::Widget(WidgetInstance::nuovo(atteso("navigation"))),
            LayoutNode::Widget(WidgetInstance::nuovo(atteso("content"))),
            LayoutNode::Widget(WidgetInstance::nuovo(atteso("player"))),
        ])
    }

    fn messaggi(radice: &LayoutZone) -> String {
        verifica(radice)
            .iter()
            .map(|p| format!("{}: {}", p.path, p.message))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn la_radice_sana_passa() {
        assert!(verifica(&sana()).is_empty(), "{}", messaggi(&sana()));
    }

    #[test]
    fn togliere_il_contenuto_e_un_errore() {
        let mut radice = sana();
        radice.children.remove(1);
        // Senza `content` non resta nemmeno un `fill`: due errori, ed è giusto
        // che siano due.
        let problemi = verifica(&radice);
        assert!(
            problemi
                .iter()
                .any(|p| p.missing.as_deref() == Some("content")),
            "{problemi:?}"
        );
    }

    #[test]
    fn togliere_i_comandi_e_un_errore() {
        let mut radice = sana();
        radice.children.pop();
        let problemi = verifica(&radice);
        assert!(
            problemi
                .iter()
                .any(|p| p.missing.as_deref() == Some("playback")),
            "{problemi:?}"
        );
        // Il messaggio elenca i membri, perché «manca uno del gruppo» senza
        // dire quali è un messaggio che costringe ad aprire il registro.
        assert!(
            messaggi(&radice).contains("«player»"),
            "{}",
            messaggi(&radice)
        );
    }

    #[test]
    fn togliere_la_navigazione_e_un_errore() {
        let mut radice = sana();
        radice.children.remove(0);
        assert!(
            verifica(&radice)
                .iter()
                .any(|p| p.missing.as_deref() == Some("navigation"))
        );
    }

    #[test]
    fn la_barra_in_fondo_basta_alla_navigazione() {
        let radice = LayoutZone::nuova(ZoneKind::Column, TrackSize::Fill).con(vec![
            LayoutNode::Widget(WidgetInstance::nuovo(atteso("content"))),
            LayoutNode::Widget(WidgetInstance::nuovo(atteso("player"))),
            LayoutNode::Widget(WidgetInstance::nuovo(atteso("bottom-nav"))),
        ]);
        assert!(verifica(&radice).is_empty(), "{}", messaggi(&radice));
    }

    #[test]
    fn un_singleton_ripetuto_e_un_errore() {
        let mut radice = sana();
        radice
            .children
            .push(LayoutNode::Widget(WidgetInstance::nuovo(atteso("content"))));
        let problemi = verifica(&radice);
        // Il percorso è quello della seconda occorrenza, non della prima.
        assert!(
            problemi.iter().any(|p| p.path == "children.3"),
            "{problemi:?}"
        );
    }

    #[test]
    fn un_widget_nella_zona_sbagliata_dice_dove_ci_sta() {
        let radice = LayoutZone::nuova(ZoneKind::Column, TrackSize::Fill).con(vec![
            LayoutNode::Widget(WidgetInstance::nuovo(atteso("navigation"))),
            LayoutNode::Widget(WidgetInstance::nuovo(atteso("content"))),
            LayoutNode::Widget(WidgetInstance::nuovo(atteso("player"))),
        ]);
        let detto = messaggi(&radice);
        assert!(detto.contains("non ci sta"), "{detto}");
        assert!(detto.contains("row"), "{detto}");
    }

    #[test]
    fn zero_o_due_elastici_sono_due_errori_diversi() {
        let mut nessuno = sana();
        nessuno.children[1] = LayoutNode::Widget(WidgetInstance {
            size: TrackSize::Hug,
            ..WidgetInstance::nuovo(atteso("content"))
        });
        assert!(
            messaggi(&nessuno).contains("nessun figlio"),
            "{}",
            messaggi(&nessuno)
        );

        let mut due = sana();
        due.children.push(LayoutNode::Widget(WidgetInstance {
            size: TrackSize::Fill,
            ..WidgetInstance::nuovo(atteso("scrubber"))
        }));
        assert!(messaggi(&due).contains("2 figli"), "{}", messaggi(&due));
    }

    #[test]
    fn una_misura_relativa_non_si_puo_sommare() {
        for unita in [
            LengthUnit::Percent,
            LengthUnit::Vw,
            LengthUnit::Vh,
            LengthUnit::Cqw,
            LengthUnit::Em,
            LengthUnit::Ch,
        ] {
            let mut radice = sana();
            radice.children[0] = LayoutNode::Widget(WidgetInstance {
                size: TrackSize::Fixed(Length {
                    value: 40.0,
                    unit: unita,
                }),
                ..WidgetInstance::nuovo(atteso("navigation"))
            });
            let detto = messaggi(&radice);
            assert!(detto.contains("non esiste ancora"), "{unita:?}: {detto}");
        }
    }

    #[test]
    fn una_misura_fissa_ha_un_minimo_e_un_massimo() {
        for (quanto, atteso_nel_messaggio) in [(4.0, "4px"), (900.0, "900px")] {
            let mut radice = sana();
            radice.children[0] = LayoutNode::Widget(WidgetInstance {
                size: TrackSize::Fixed(Length::px(quanto)),
                ..WidgetInstance::nuovo(atteso("navigation"))
            });
            let detto = messaggi(&radice);
            assert!(detto.contains(atteso_nel_messaggio), "{detto}");
        }
    }

    #[test]
    fn il_rem_si_somma_perche_si_conosce() {
        // 30rem sono 480px: il massimo esatto, quindi passa. 31rem no.
        let mut radice = sana();
        radice.children[0] = LayoutNode::Widget(WidgetInstance {
            size: TrackSize::Fixed(Length {
                value: 30.0,
                unit: LengthUnit::Rem,
            }),
            ..WidgetInstance::nuovo(atteso("navigation"))
        });
        assert!(verifica(&radice).is_empty(), "{}", messaggi(&radice));
    }

    #[test]
    fn i_fissi_fratelli_hanno_un_budget() {
        let mut radice = sana();
        for _ in 0..2 {
            radice.children.push(LayoutNode::Widget(WidgetInstance {
                size: TrackSize::Fixed(Length::px(400.0)),
                ..WidgetInstance::nuovo(atteso("cover-large"))
            }));
        }
        let detto = messaggi(&radice);
        assert!(detto.contains("800px"), "{detto}");
        assert!(detto.contains("640px"), "{detto}");
    }

    #[test]
    fn una_zona_vuota_e_un_errore() {
        let mut radice = sana();
        radice.children.push(LayoutNode::Zone(LayoutZone::nuova(
            ZoneKind::Row,
            TrackSize::Hug,
        )));
        assert!(messaggi(&radice).contains("aria che nessuno ha chiesto"));
    }

    #[test]
    fn l_annidamento_ha_un_tetto() {
        let mut foglia =
            LayoutZone::nuova(ZoneKind::Column, TrackSize::Fill).con(vec![LayoutNode::Widget(
                WidgetInstance::nuovo(atteso("content")),
            )]);
        for _ in 0..MAX_SHELL_DEPTH {
            foglia = LayoutZone::nuova(ZoneKind::Column, TrackSize::Fill)
                .con(vec![LayoutNode::Zone(foglia)]);
        }
        assert!(
            messaggi(&foglia).contains("annidata"),
            "{}",
            messaggi(&foglia)
        );
    }

    #[test]
    fn il_numero_di_nodi_ha_un_tetto() {
        let mut radice = sana();
        for _ in 0..MAX_SHELL_NODES {
            radice
                .children
                .push(LayoutNode::Widget(WidgetInstance::nuovo(atteso("rating"))));
        }
        let problemi = verifica(&radice);
        // Uno solo: oltre il tetto non si scende, altrimenti ogni nodo di
        // troppo produrrebbe il suo messaggio e il primo si perderebbe.
        assert_eq!(problemi.len(), 1, "{problemi:?}");
        assert!(problemi[0].message.contains("nodi"));
    }

    #[test]
    fn le_manopole_partono_dai_difetti() {
        let trasporto = WidgetInstance::nuovo(atteso("transport"));
        assert_eq!(trasporto.option("shuffle"), Some(OptionValue::Flag(true)));
        assert_eq!(trasporto.option("size"), Some(OptionValue::Word("bar")));
        // Tutte, non solo quelle dichiarate: chi legge non deve conoscere la
        // tabella dei difetti per sapere cosa sta guardando.
        assert_eq!(trasporto.options.len(), atteso("transport").options.len());
    }

    #[test]
    fn un_nome_sbagliato_riceve_un_suggerimento() {
        assert!(nearest_widgets("contents").contains(&"content"));
    }
}
