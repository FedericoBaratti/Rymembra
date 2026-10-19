//! Il compilatore: da documento validato a CSS.
//!
//! È l'**unico** autore di CSS di tutto il sistema. Non esiste un percorso per
//! cui una stringa scritta da chi crea la skin arrivi nell'output: i colori
//! entrano come canali, le lunghezze come numero più unità, e i nomi delle
//! proprietà vengono dal registro dei token e dal registro delle parti, non dal
//! documento. Quella proprietà è ciò che rende l'intera classe di attacchi via
//! CSS — `@import`, selettori che esfiltrano attributi, indirizzi che chiamano a
//! casa — non applicabile, e i test la verificano sull'output.
//!
//! L'unico `url()` che esce di qui punta allo schema locale che serve le voci
//! dei pacchetti, ed è costruito dal compilatore con due pezzi già validati:
//! l'id della skin e il nome della voce. Vedi [`Effect::Image`].
//!
//! # I nomi che arrivano nel foglio, e perché non possono chiuderlo
//!
//! «Nessuna stringa dell'autore» va letto con precisione: i **valori** non ci
//! arrivano mai, i **nomi** sì, perché un nome è il modo in cui due punti del
//! foglio si ritrovano. Sono questi, e nessun altro:
//!
//! - l'id della skin, nei selettori (`[data-skin='<id>']`), nei nomi dei
//!   `@keyframes`, delle famiglie e negli indirizzi delle risorse;
//! - la versione, nell'intestazione;
//! - i nomi della tavolozza, dei motivi e delle curve, dentro i nomi delle
//!   variabili (`--skin-color-<nome>`, `--skin-<nome>`, `--skin-ease-<nome>`);
//! - i nomi delle animazioni, dentro quelli dei `@keyframes`;
//! - le famiglie dei caratteri, fra apici nelle pile di `font.*` e ridotte a
//!   `[a-z0-9-]` nel nome proprio che il `@font-face` dichiara;
//! - i nomi delle voci del pacchetto, dentro `url("…")`.
//!
//! Ognuno ha un alfabeto chiuso che la validazione impone — lettere, cifre,
//! trattini, al più lo spazio in una famiglia e il punto in una versione o in
//! un nome di file — e in nessuno ci stanno apici, graffe, punti e virgola,
//! barre inverse o `*/`. Il compilatore però **non si fida** della validazione:
//! ogni nome passa da `nel_suo_alfabeto` nel punto esatto in cui entra nel
//! testo. Su un documento validato non cambia un carattere — la prova sulle
//! skin di serie lo dice — e un [`SkinDocument`] costruito a mano, che ha i
//! campi pubblici e nessuno obbliga a passare da `parse_skin`, non ha comunque
//! un canale per uscire da una stringa.
//!
//! Il testo libero — nome, autore, descrizione, licenza — **non entra**, in
//! nessuna forma. Il nome stava nell'intestazione fino alla 3.0, dentro un
//! commento: una skin condivisa che si chiamasse `x */ :root{display:none} /*`
//! chiudeva il commento, e la finestra di chi la indossava restava vuota a ogni
//! avvio. Ora l'intestazione porta id e versione, e la prova che mette `*/`,
//! graffe, apici e a capo in **ogni** campo di stringa di un documento vero
//! verifica che il foglio non cambi di un byte.
//!
//! # Perché non restituisce un `Result`
//!
//! I tipi che entrano sono già i tipi che escono — un colore è un
//! `ColorValue`, un effetto è un `Effect` chiuso — e ogni `match` è esaustivo.
//! Non c'è un ramo in cui fallire.
//!
//! # Due ambiti
//!
//! Lo stesso documento si compila per la finestra e per l'anteprima dello
//! Studio, e le due cose non sono la stessa. Nella finestra le regole partono da
//! `:root[data-skin='<id>']`; nell'anteprima da `.anteprima[data-skin='<id>']`,
//! i nomi di `@keyframes` e dei caratteri hanno un suffisso che li tiene
//! separati da quelli della skin che la finestra indossa, e i token che la
//! bozza non dichiara si colmano con quelli di serie — nella finestra li colma
//! `stile.css`, nel riquadro li erediterebbe dalla skin attiva. È [`Ambito`] a
//! tenere queste differenze in un posto solo, invece di una sostituzione di
//! testo a valle.
//!
//! # Le animazioni di una parte sono **una** lista
//!
//! Lo shorthand `animation` non aggiunge: sostituisce l'intera lista. Una
//! regola di stato che scrivesse soltanto la sua animazione spegnerebbe
//! l'entrata e il ciclo che la regola base aveva acceso — il vinile che si
//! ferma e riparte da capo a ogni passaggio del mouse. Il compilatore quindi
//! scrive, in **ogni** regola che tocca `animation`, la lista completa di quel
//! che deve girare in quella condizione. Un'animazione che resta nella lista
//! allo stesso posto il motore la riconosce e la lascia andare avanti.

use std::fmt::Write as _;
use std::sync::OnceLock;

use crate::document::{RouteFrame, Scheme, SkinDocument, SkinMotion};
use crate::effects::{Effect, EffectTarget, FiltroElemento, ImageSize, Paint, stack_cost};
use crate::movimento::{
    AnimFrame, AnimTrigger, Animation, AnimationRef, CASCATA_VOCI, Curva, Ritaglio,
};
use crate::parts::{AnelloFuoco, Ombra, PartAppearance, PartLayer, PartStyle, Raggio};
use crate::stati::{Fuoco, PartState};
use crate::tokens::{
    ColorValue, DIFETTI_DEL_MOTORE, DynamicSource, ShadowValue, TokenDef, TokenKind, TokenSet,
    TokenValue,
};
use crate::values::{
    Duration, FontKind, Length, Rgba, format_color, format_duration, format_easing,
    format_font_stack, format_length, format_length_value, format_rgb_triple, num,
};

/// Una parte che il renderer avvolge in `<ViewTransition>`, e cosa la skin ci
/// dichiara sopra.
///
/// Il renderer lo deve sapere per una ragione precisa: una transizione di vista
/// **senza** regole della skin non è ferma — il motore mette di serie una
/// dissolvenza incrociata — e sopra l'entrata di `@starting-style` che il
/// foglio del motore già scrive ne farebbe due. Dove la skin non dichiara
/// niente, il renderer passa `"none"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presenza {
    /// La parte.
    pub parte: &'static str,
    /// La skin anima l'entrata.
    pub enter: bool,
    /// La skin anima l'uscita.
    pub exit: bool,
    /// La skin anima il volo fra due schermate.
    pub share: bool,
}

/// Il risultato della compilazione.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledSkin {
    /// L'identificatore della skin.
    pub id: String,
    /// Il foglio, pronto da adottare.
    pub css: String,
    /// Somma dei costi di motivi e parti, per il budget prestazionale.
    pub cost: u32,
    /// Quanti pezzi dell'app lo scafale monta, sul budget dello scafale.
    pub shell_cost: u32,
    /// Quanto costa il movimento, sommato su tutte le parti animate.
    pub motion_cost: u32,
    /// I token che seguono la copertina: il runtime deve aggiornarli.
    pub dynamic_tokens: Vec<&'static str>,
    /// Le parti che entrano, escono o volano con una transizione di vista.
    pub presenze: Vec<Presenza>,
}

/// Il prefisso dei motivi. Evita collisioni fra skin e col registro dei token.
const PREFISSO_MOTIVO: &str = "--skin-";
/// Il prefisso dei colori della tavolozza locale.
const PREFISSO_TAVOLOZZA: &str = "--skin-color-";
/// L'indirizzo da cui il protocollo locale serve le voci dei pacchetti.
///
/// È la forma con cui WebView2 raggiunge uno schema personalizzato — la
/// stessa di `http://rymembra-cover.localhost` per le copertine.
pub const ORIGINE_RISORSE: &str = "http://rymembra-skin.localhost";

/// Per chi si compila: la finestra, o il riquadro dell'anteprima.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ambito {
    id: String,
    anteprima: bool,
    /// Le risorse vengono dalla bozza dello Studio e non dalla skin installata.
    bozza: bool,
    /// Ogni stato si può anche forzare con `data-forza`.
    forza: bool,
}

/// Un nome del documento ridotto al suo alfabeto, nel punto in cui entra nel
/// foglio.
///
/// Restano le lettere e le cifre ASCII più i caratteri di `anche`, e cade tutto
/// il resto. Non è la validazione — quella rifiuta e dice perché, questa scarta
/// in silenzio — ed è proprio la differenza: qui non c'è più nessuno a cui
/// dirlo, e un carattere che arrivasse fin qui sarebbe già un guasto a monte.
/// Su un nome validato è l'identità. Vedi il `//!`, «I nomi che arrivano nel
/// foglio».
fn nel_suo_alfabeto(testo: &str, anche: &[char]) -> String {
    testo
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || anche.contains(c))
        .collect()
}

/// L'alfabeto di un id e dei nomi locali: tavolozza, motivi, curve, animazioni.
const TRATTINO: &[char] = &['-'];

impl Ambito {
    fn nuovo(id: &str, anteprima: bool, bozza: bool, forza: bool) -> Self {
        Self {
            // L'id finisce fra apici in ogni selettore: vedi `nel_suo_alfabeto`.
            id: nel_suo_alfabeto(id, TRATTINO),
            anteprima,
            bozza,
            forza,
        }
    }

    /// La finestra vera.
    #[must_use]
    pub fn app(id: &str) -> Self {
        Self::nuovo(id, false, false, false)
    }

    /// La finestra vera, con la bozza dello Studio indosso: «Prova sull'app».
    ///
    /// I selettori sono quelli della finestra, le risorse quelle della bozza —
    /// che è dove lo Studio mette un carattere appena aggiunto, prima che la
    /// skin si installi.
    #[must_use]
    pub fn prova(id: &str) -> Self {
        Self::nuovo(id, false, true, false)
    }

    /// La tela dello Studio: una finestra finta dentro un `<iframe>`.
    ///
    /// I selettori sono quelli della finestra — nel documento della tela
    /// `:root` è la sua radice, e la skin attiva dell'app lì non c'è — con le
    /// risorse della bozza e gli stati forzabili. È la forma più fedele: la
    /// tela misura `100vw`, `position: fixed` e le query sulla sua finestra,
    /// esattamente come le misurerà l'app.
    #[must_use]
    pub fn tela(id: &str) -> Self {
        Self::nuovo(id, false, true, true)
    }

    /// Il riquadro dell'anteprima dello Studio.
    #[must_use]
    pub fn anteprima(id: &str) -> Self {
        Self::nuovo(id, true, true, true)
    }

    /// È l'anteprima.
    #[must_use]
    pub const fn in_anteprima(&self) -> bool {
        self.anteprima
    }

    /// L'id sotto cui si compila, già ridotto al suo alfabeto.
    ///
    /// È quel che la finestra deve scrivere in `data-skin` perché i selettori
    /// la trovino. Di solito è l'id del documento; nello Studio è quello della
    /// **bozza** — la cartella da cui vengono le risorse — che resta indietro
    /// rispetto al documento finché una rinomina non la raggiunge, e la tela e
    /// la prova sull'app devono seguire la cartella, non il testo.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    fn radice(&self) -> String {
        if self.anteprima {
            format!(".anteprima[data-skin='{}']", self.id)
        } else {
            format!(":root[data-skin='{}']", self.id)
        }
    }

    fn tema(&self, scheme: Scheme) -> String {
        format!("{}[data-theme='{}']", self.radice(), scheme.as_str())
    }

    fn telefono(&self) -> String {
        format!("{}[data-mobile]", self.radice())
    }

    /// La radice sotto cui la preferenza di Rymembra ferma il movimento.
    fn radice_ferma_utente(&self) -> String {
        if self.anteprima {
            format!(":root[data-motion-utente='ridotto'] {}", self.radice())
        } else {
            format!(
                ":root[data-skin='{}'][data-motion-utente='ridotto']",
                self.id
            )
        }
    }

    /// La radice sotto cui l'intensità della skin ferma il movimento.
    fn radice_ferma_intensita(&self) -> String {
        format!("{}[data-motion='none']", self.radice())
    }

    fn suffisso(&self) -> &'static str {
        if self.anteprima { "-anteprima" } else { "" }
    }

    fn keyframes(&self, nome: &str) -> String {
        format!(
            "skin-{}-anim-{}{}",
            self.id,
            nel_suo_alfabeto(nome, TRATTINO),
            self.suffisso()
        )
    }

    fn keyframes_rotta(&self, verso: &str) -> String {
        format!("skin-{}-{verso}{}", self.id, self.suffisso())
    }

    /// Il nome di famiglia di un carattere del pacchetto.
    ///
    /// Le famiglie sono globali come i `@keyframes`: senza un nome proprio,
    /// una skin che chiama «Fraunces» il suo carattere e una che chiama così un
    /// altro si prenderebbero a vicenda il file.
    ///
    /// La riduzione è quella della validazione
    /// ([`crate::document::famiglia_ridotta`]), che rifiuta due famiglie che
    /// finirebbero sotto lo stesso nome.
    fn famiglia(&self, nome: &str) -> String {
        let piccolo = crate::document::famiglia_ridotta(nome);
        format!("skin-{}{}-{piccolo}", self.id, self.suffisso())
    }

    /// L'indirizzo di una voce del pacchetto.
    ///
    /// Nella finestra serve la skin installata; nell'anteprima — e nella prova
    /// sull'app — la bozza, che è dove lo Studio mette le voci nuove prima che
    /// la skin si installi.
    fn url_risorsa(&self, nome: &str) -> String {
        // Il nome di una voce sta fra virgolette dentro `url("…")`: una
        // virgoletta o una barra inversa lo chiuderebbero, una barra lo
        // manderebbe in un'altra cartella. L'alfabeto del pacchetto non ha
        // nessuno dei tre.
        let nome = nel_suo_alfabeto(nome, &['-', '_', '.']);
        if self.bozza {
            format!("{ORIGINE_RISORSE}/bozza/{}/{nome}", self.id)
        } else {
            format!("{ORIGINE_RISORSE}/{}/{nome}", self.id)
        }
    }

    /// Il selettore di uno stato.
    ///
    /// Nell'anteprima ogni stato si può anche **forzare**: la tela scrive
    /// `data-forza="hover"` sulla parte scelta, e lo stato si vede senza
    /// portarci sopra il puntatore — che è l'unico modo di guardare com'è una
    /// superficie premuta mentre la si modifica.
    fn stato(&self, stato: PartState, fuoco: Fuoco) -> String {
        let nativo = stato.selector(fuoco);
        if self.forza {
            format!("{nativo}, [data-forza~='{}']", stato.as_str())
        } else {
            nativo.to_owned()
        }
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
/// Oggi il runtime estrae **una** tinta dalla copertina e la scrive in
/// `--accent`, quindi tutte e quattro le sorgenti puntano lì. Il `match` rende
/// la cosa una decisione dichiarata invece di un ramo ripetuto.
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
        // Il nome della variabile viene dal registro, non dal documento.
        ColorValue::Token(def) => format!("var({})", def.css),
        ColorValue::Palette { ref name, alpha } => {
            let name = nel_suo_alfabeto(name, TRATTINO);
            match alpha {
                None => format!("var({PREFISSO_TAVOLOZZA}{name})"),
                Some(a) => format!(
                    "rgb(var({PREFISSO_TAVOLOZZA}{name}-rgb) / {})",
                    crate::values::number(a, 3)
                ),
            }
        }
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
fn compila_tavolozza(palette: &[(String, Rgba)]) -> Vec<Dichiarazione> {
    let mut dichiarazioni = Vec::with_capacity(palette.len() * 2);
    for (nome, colore) in palette {
        let nome = nel_suo_alfabeto(nome, TRATTINO);
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

fn risolvi_ombra_parte(ombra: &Ombra) -> String {
    match ombra {
        Ombra::Valore(valore) => risolvi_ombra(valore),
        Ombra::Token(def) => format!("var({})", def.css),
    }
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
fn compile_effect(effect: &Effect, ambito: &Ambito) -> String {
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
            let misura = size
                .map(|s| format!(" {}", format_length(s)))
                .unwrap_or_default();
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

        Effect::Image {
            ref src,
            size,
            repeat,
            position,
        } => {
            let misura = match size {
                ImageSize::Cover => "cover".to_owned(),
                ImageSize::Contain => "contain".to_owned(),
                ImageSize::Auto => "auto".to_owned(),
                ImageSize::Misura(lunghezza) => format_length(lunghezza),
            };
            let dove = position.map_or_else(
                || "center".to_owned(),
                |(x, y)| format!("{} {}", format_length(x), format_length(y)),
            );
            format!(
                "url(\"{}\") {dove} / {misura} {}",
                ambito.url_risorsa(src),
                if repeat { "repeat" } else { "no-repeat" }
            )
        }
    }
}

/// Il suffisso dice in quale proprietà va usato un motivo.
const fn suffisso_motivo(target: EffectTarget) -> &'static str {
    match target {
        EffectTarget::Background => "",
        EffectTarget::ClipPath => "-clip",
        EffectTarget::Filter => "-filter",
    }
}

/// Uno strato di pittura, per esteso o come riferimento a un motivo.
///
/// Il nome del motivo finisce nel nome di una variabile: ci arriva già passato
/// da `is_local_name`, e ripassa comunque da [`nel_suo_alfabeto`].
fn compila_pittura(paint: &Paint, ambito: &Ambito) -> String {
    match paint {
        Paint::Inline(effect) => compile_effect(effect, ambito),
        Paint::Pattern { name, def } => {
            format!(
                "var({PREFISSO_MOTIVO}{}{})",
                nel_suo_alfabeto(name, TRATTINO),
                suffisso_motivo(def.target())
            )
        }
    }
}

/// Una pila di strati per `background`, il primo sopra gli altri come in CSS.
///
/// Un colore pieno è ammesso da CSS **solo** nell'ultimo strato: `rgb(…),
/// linear-gradient(…)` è una dichiarazione non valida, e il browser la butta
/// intera — con `var()` di mezzo senza nemmeno dirlo, perché l'errore arriva
/// al valore calcolato. Una tinta che non sta in fondo si scrive allora come un
/// gradiente di un colore solo, che si dipinge uguale e sta dove vuole.
fn compila_sfondo(strati: &[Paint], ambito: &Ambito) -> String {
    let ultimo = strati.len().saturating_sub(1);
    strati
        .iter()
        .enumerate()
        .map(|(posto, strato)| {
            let scritto = compila_pittura(strato, ambito);
            let tinta = matches!(strato.effect(), Effect::Solid { .. });
            if tinta && posto < ultimo {
                format!("linear-gradient({scritto}, {scritto})")
            } else {
                scritto
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn compila_motivi(patterns: &[(String, Effect)], ambito: &Ambito) -> Vec<Dichiarazione> {
    patterns
        .iter()
        .map(|(nome, effetto)| {
            dichiara(
                format!(
                    "{PREFISSO_MOTIVO}{}{}",
                    nel_suo_alfabeto(nome, TRATTINO),
                    suffisso_motivo(effetto.target())
                ),
                compile_effect(effetto, ambito),
            )
        })
        .collect()
}

// ── I token ─────────────────────────────────────────────────────────────────

/// I token di un insieme, più quelli che seguono la copertina.
///
/// `famiglia` traduce il nome di un carattere del pacchetto nel nome proprio
/// della skin: nei token si scrive «Fraunces», nel foglio va il nome che il
/// `@font-face` ha dichiarato.
fn compila_token(
    tokens: &TokenSet,
    famiglia: &dyn Fn(&str) -> Option<String>,
) -> (Vec<Dichiarazione>, Vec<&'static str>) {
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
                let nomi: Vec<String> = families
                    .iter()
                    .map(|f| famiglia(f).unwrap_or_else(|| f.clone()))
                    .collect();
                dichiarazioni.push(dichiara(def.css, format_font_stack(&nomi, genere)));
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

    // `color.hero` esiste **solo** come tripla.
    if def.rgb_triple == Some(def.css) {
        dichiarazioni.push(dichiara(def.css, tripla(color)));
        return;
    }

    dichiarazioni.push(dichiara(def.css, risolvi_colore(color)));
    if let Some(nome) = def.rgb_triple {
        dichiarazioni.push(dichiara(nome, tripla(color)));
    }
}

/// I token **calcolati**: conseguenze, non scelte.
///
/// `tutti` li emette anche quando i token da cui derivano non sono nell'insieme:
/// serve al blocco di serie dell'anteprima, che deve reggere da solo.
fn calcolati(tokens: &TokenSet, tutti: bool) -> Vec<Dichiarazione> {
    let mut dichiarazioni = Vec::new();

    if tutti || tokens.contains("layout.rail") {
        dichiarazioni.push(dichiara("--shell-left", "var(--rail-w)"));
    }
    if tutti || tokens.contains("layout.playerHeight") || tokens.contains("layout.playerGap") {
        dichiarazioni.push(dichiara(
            "--player-clearance",
            "calc(var(--player-h) + var(--player-gap) * 2)",
        ));
    }
    if let Some(TokenValue::Color(color)) = tokens.get("color.surface.0")
        && let Some(colore) = colore_letterale(color)
    {
        dichiarazioni.push(dichiara("--surface-0-rgb", format_rgb_triple(colore)));
    }
    if tutti || tokens.contains("motion.dur.1") {
        dichiarazioni.push(dichiara(
            "--transition-fast",
            "calc(var(--dur-1) * var(--motion-scale, 1)) var(--ease-out-expo)",
        ));
    }
    if tutti || tokens.contains("motion.dur.2") {
        dichiarazioni.push(dichiara(
            "--transition-med",
            "calc(var(--dur-2) * var(--motion-scale, 1)) var(--ease-out-expo)",
        ));
    }

    dichiarazioni
}

/// La skin di riferimento, letta una volta sola.
///
/// Il compilatore non fallisce, e la skin di riferimento è compilata dentro il
/// binario e provata dai test: se non si leggesse, l'anteprima semplicemente
/// non avrebbe il blocco di serie — che è il comportamento di prima.
fn di_serie() -> Option<&'static SkinDocument> {
    static PLAIN: OnceLock<Option<SkinDocument>> = OnceLock::new();
    PLAIN.get_or_init(|| crate::plain().ok()).as_ref()
}

/// Il blocco di serie dell'anteprima: tutti i token, come li avrebbe la
/// finestra se la bozza non ne dichiarasse nessuno.
///
/// Sta sotto un `:where()`, cioè a specificità zero: qualunque cosa la bozza
/// dichiari lo batte, e quel che non dichiara lo trova qui invece di
/// ereditarlo dalla skin che la finestra indossa.
fn blocchi_di_serie(ambito: &Ambito) -> String {
    let Some(plain) = di_serie() else {
        return String::new();
    };
    let nessuna = |_: &str| -> Option<String> { None };
    let (mut base, _) = compila_token(&plain.tokens, &nessuna);
    for (id, valore) in DIFETTI_DEL_MOTORE {
        if let Some(def) = crate::tokens::token(id)
            && !plain.tokens.contains(id)
        {
            base.push(dichiara(def.css, *valore));
        }
    }
    base.extend(calcolati(&plain.tokens, true));
    base.push(dichiara("--motion-intensity", "full"));
    base.push(dichiara("--motion-scale", "1"));
    base.push(dichiara("color-scheme", "dark"));
    let mut css = blocco(&format!(":where({})", ambito.radice()), &base);

    if let Some(chiari) = plain.variante_tokens() {
        let (mut luce, _) = compila_token(chiari, &nessuna);
        luce.extend(calcolati(chiari, false));
        luce.push(dichiara("color-scheme", "light"));
        css.push_str(&blocco(
            &format!(":where({})", ambito.tema(Scheme::Light)),
            &luce,
        ));
    }
    css
}

// ── Le parti ────────────────────────────────────────────────────────────────

fn compila_raggio(raggio: Raggio, dichiarazioni: &mut Vec<Dichiarazione>) {
    match raggio {
        Raggio::Uguale(lunghezza) => {
            dichiarazioni.push(dichiara("border-radius", format_length(lunghezza)));
        }
        Raggio::Angoli {
            top_left,
            top_right,
            bottom_right,
            bottom_left,
        } => {
            for (proprieta, valore) in [
                ("border-top-left-radius", top_left),
                ("border-top-right-radius", top_right),
                ("border-bottom-right-radius", bottom_right),
                ("border-bottom-left-radius", bottom_left),
            ] {
                if let Some(lunghezza) = valore {
                    dichiarazioni.push(dichiara(proprieta, format_length(lunghezza)));
                }
            }
        }
    }
}

fn compila_filtri(filtri: &[FiltroElemento]) -> String {
    filtri
        .iter()
        .map(|filtro| match *filtro {
            FiltroElemento::Blur(l) => format!("blur({})", format_length(l)),
            FiltroElemento::Brightness(n) => format!("brightness({})", num(n)),
            FiltroElemento::Contrast(n) => format!("contrast({})", num(n)),
            FiltroElemento::Saturate(n) => format!("saturate({})", num(n)),
            FiltroElemento::Grayscale(n) => format!("grayscale({})", num(n)),
            FiltroElemento::HueRotate(n) => format!("hue-rotate({}deg)", num(n)),
            FiltroElemento::DropShadow {
                x,
                y,
                blur,
                ref color,
            } => format!(
                "drop-shadow({} {} {} {})",
                format_length(x),
                format_length(y),
                format_length(blur),
                risolvi_colore(color)
            ),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn compila_anello(anello: &AnelloFuoco) -> Vec<Dichiarazione> {
    let mut dichiarazioni = vec![dichiara(
        "outline",
        format!(
            "{} solid {}",
            format_length(anello.width),
            risolvi_colore(&anello.color)
        ),
    )];
    if let Some(distanza) = anello.offset {
        dichiarazioni.push(dichiara("outline-offset", format_length(distanza)));
    }
    dichiarazioni
}

const fn variabile_carattere(genere: FontKind) -> &'static str {
    match genere {
        FontKind::Sans => "var(--font-sans)",
        FontKind::Mono => "var(--font-mono)",
        FontKind::Display => "var(--font-dot)",
    }
}

/// L'aspetto, senza l'anello del fuoco: quello va nella regola del fuoco.
fn compila_aspetto(source: &PartAppearance, ambito: &Ambito) -> Vec<Dichiarazione> {
    let mut dichiarazioni = Vec::new();

    if !source.background.is_empty() {
        dichiarazioni.push(dichiara(
            "background",
            compila_sfondo(&source.background, ambito),
        ));
    }
    if let Some(color) = source.text_color.as_ref() {
        dichiarazioni.push(dichiara("color", risolvi_colore(color)));
    }
    if source.border_color.is_some()
        || source.border_width.is_some()
        || source.border_style.is_some()
    {
        let spessore = source
            .border_width
            .map_or_else(|| "1px".to_owned(), format_length);
        let stile = source.border_style.map_or("solid", |s| s.as_str());
        let colore = source
            .border_color
            .as_ref()
            .map_or_else(|| "var(--hairline)".to_owned(), risolvi_colore);
        dichiarazioni.push(dichiara("border", format!("{spessore} {stile} {colore}")));
    }
    if let Some(raggio) = source.radius {
        compila_raggio(raggio, &mut dichiarazioni);
    }
    if let Some(ombra) = source.shadow.as_ref() {
        dichiarazioni.push(dichiara("box-shadow", risolvi_ombra_parte(ombra)));
    }
    if let Some(clip) = source.clip.as_ref() {
        dichiarazioni.push(dichiara("clip-path", compila_pittura(clip, ambito)));
    }
    if !source.filter.is_empty() {
        dichiarazioni.push(dichiara("filter", compila_filtri(&source.filter)));
    }
    if let Some(dietro) = source.backdrop.as_ref() {
        dichiarazioni.push(dichiara("backdrop-filter", compila_pittura(dietro, ambito)));
    }
    if let Some(opacity) = source.opacity {
        dichiarazioni.push(dichiara("opacity", num(opacity)));
    }
    if let Some(genere) = source.font {
        dichiarazioni.push(dichiara("font-family", variabile_carattere(genere)));
    }
    if let Some(misura) = source.font_size {
        dichiarazioni.push(dichiara("font-size", format_length(misura)));
    }
    if let Some(interlinea) = source.line_height {
        dichiarazioni.push(dichiara("line-height", num(interlinea)));
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

/// Un livello su `::before` o `::after`.
///
/// Posizione, riempimento e inerzia al puntatore li mette il compilatore,
/// sempre uguali: una skin non può spostare un livello, né fargli prendere i
/// clic della superficie sotto. Il raggio lo eredita dall'ospite, così un
/// livello non esce dagli angoli tondi della superficie che veste.
fn compila_livello(livello: &PartLayer, prima: bool, ambito: &Ambito) -> Vec<Dichiarazione> {
    let mut dichiarazioni = vec![
        dichiara("content", "\"\""),
        dichiara("position", "absolute"),
        dichiara("inset", "0"),
        dichiara("pointer-events", "none"),
        dichiara("border-radius", "inherit"),
    ];
    if prima {
        // Dietro al contenuto dell'ospite e davanti al suo fondo: l'ospite
        // isola il contesto, e il livello non può scivolare sotto la pagina.
        dichiarazioni.push(dichiara("z-index", "-1"));
    }
    dichiarazioni.push(dichiara(
        "background",
        compila_sfondo(&livello.background, ambito),
    ));
    if let Some(opacity) = livello.opacity {
        dichiarazioni.push(dichiara("opacity", num(opacity)));
    }
    dichiarazioni
}

// ── Il movimento ────────────────────────────────────────────────────────────

/// Una durata come esce di qui, e come esce **sempre**.
///
/// Nessun ramo del compilatore emette una durata nuda dentro un `animation:` o
/// un `transition:`. Il fallback `, 1` serve a una skin che dichiara
/// un'animazione e non l'intensità.
fn durata_scalata(tempo: &str) -> String {
    format!("calc({tempo} * var(--motion-scale, 1))")
}

fn curva(curva: &Curva) -> String {
    match curva {
        Curva::Valore(easing) => format_easing(*easing),
        Curva::Nome(nome) => format!(
            "var({PREFISSO_MOTIVO}ease-{})",
            nel_suo_alfabeto(nome, TRATTINO)
        ),
    }
}

/// Il ritardo di un'animazione, con la cascata se c'è.
///
/// La cascata legge `--indice`, che il componente scrive sulla voce: la
/// posizione dentro la pagina o la finestra visibile, mai quella assoluta —
/// la duecentesima scheda di una pagina non aspetta duecento passi. E anche
/// così il passo si ferma a [`CASCATA_VOCI`].
fn ritardo(def: &Animation, cascata: Option<Duration>) -> Option<String> {
    match (def.delay, cascata) {
        (None, None) => None,
        (Some(tempo), None) => Some(durata_scalata(&format_duration(tempo))),
        (fisso, Some(passo)) => {
            let base = fisso.map_or_else(|| "0ms".to_owned(), format_duration);
            Some(format!(
                "calc(({base} + min(var(--indice, 0), {CASCATA_VOCI}) * {}) * var(--motion-scale, 1))",
                format_duration(passo)
            ))
        }
    }
}

/// Una voce della lista `animation` di una parte.
#[derive(Debug, Clone)]
struct Voce<'a> {
    keyframes: String,
    def: &'a Animation,
    /// È l'entrata della parte: tace dove la parte si rimonta scorrendo.
    entrata: bool,
    /// Gira solo finché la parte suona.
    suona: bool,
    /// Scorre con la vista invece che col tempo.
    vista: bool,
    cascata: Option<Duration>,
}

impl<'a> Voce<'a> {
    fn di(ambito: &Ambito, riferimento: &'a AnimationRef) -> Self {
        Self {
            keyframes: ambito.keyframes(&riferimento.name),
            def: &riferimento.def,
            entrata: false,
            suona: false,
            vista: false,
            cascata: None,
        }
    }
}

impl Voce<'_> {
    fn scorciatoia(&self) -> String {
        let mut pezzi = vec![self.keyframes.clone()];
        pezzi.push(if self.vista {
            // Una durata a tempo su una linea di vista non vuol dire niente:
            // la misura la dà lo scorrimento. Si scrive `auto` qui, e la prova
            // sulle durate nude la accetta perché `auto` non è un tempo.
            "auto".to_owned()
        } else {
            durata_scalata(&format_duration(self.def.duration))
        });
        pezzi.push(curva(&self.def.easing));
        if let Some(tempo) = ritardo(self.def, self.cascata) {
            pezzi.push(tempo);
        }
        pezzi.push(self.def.iterations.css());
        pezzi.push(self.def.direction.as_str().to_owned());
        pezzi.push(self.def.fill.as_str().to_owned());
        pezzi.join(" ")
    }
}

/// Le dichiarazioni di una lista di animazioni.
fn dichiara_lista(voci: &[Voce<'_>]) -> Vec<Dichiarazione> {
    if voci.is_empty() {
        return vec![dichiara("animation", "none")];
    }
    let mut dichiarazioni = vec![dichiara(
        "animation",
        voci.iter()
            .map(Voce::scorciatoia)
            .collect::<Vec<_>>()
            .join(", "),
    )];
    if voci.iter().any(|v| v.suona) {
        dichiarazioni.push(dichiara(
            "animation-play-state",
            voci.iter()
                .map(|v| {
                    if v.suona {
                        "var(--skin-suona, paused)"
                    } else {
                        "running"
                    }
                })
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    if voci.iter().any(|v| v.vista) {
        // Dopo lo shorthand, che rimette la linea del tempo a `auto`.
        dichiarazioni.push(dichiara(
            "animation-timeline",
            voci.iter()
                .map(|v| if v.vista { "view()" } else { "auto" })
                .collect::<Vec<_>>()
                .join(", "),
        ));
        dichiarazioni.push(dichiara(
            "animation-range",
            voci.iter()
                .map(|v| {
                    if v.vista {
                        "entry 0% cover 30%"
                    } else {
                        "normal"
                    }
                })
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    dichiarazioni
}

/// Una fermata di un'animazione.
///
/// L'ordine delle trasformazioni è fisso — traslazione, rotazione,
/// inclinazione, scala — e non quello in cui i campi sono scritti:
/// `transform` non è commutativa.
fn fermata(frame: &AnimFrame, origine: Option<(Length, Length)>) -> Vec<String> {
    let mut righe: Vec<String> = Vec::new();
    let mut scrivi = |proprieta: &str, valore: String| {
        righe.push(format!("    {proprieta}: {valore};"));
    };
    if let Some(opacity) = frame.opacity {
        scrivi("opacity", num(opacity));
    }
    let mut trasformazioni: Vec<String> = Vec::new();
    if let Some(x) = frame.translate_x {
        trasformazioni.push(format!("translateX({})", format_length(x)));
    }
    if let Some(y) = frame.translate_y {
        trasformazioni.push(format!("translateY({})", format_length(y)));
    }
    if let Some(gradi) = frame.rotate {
        trasformazioni.push(format!("rotate({}deg)", num(gradi)));
    }
    if let Some(gradi) = frame.skew_x {
        trasformazioni.push(format!("skewX({}deg)", num(gradi)));
    }
    if let Some(gradi) = frame.skew_y {
        trasformazioni.push(format!("skewY({}deg)", num(gradi)));
    }
    if let Some(scala) = frame.scale {
        trasformazioni.push(format!("scale({})", num(scala)));
    }
    if let Some(scala) = frame.scale_x {
        trasformazioni.push(format!("scaleX({})", num(scala)));
    }
    if let Some(scala) = frame.scale_y {
        trasformazioni.push(format!("scaleY({})", num(scala)));
    }
    if !trasformazioni.is_empty() {
        scrivi("transform", trasformazioni.join(" "));
        if let Some((x, y)) = origine {
            scrivi(
                "transform-origin",
                format!("{} {}", format_length(x), format_length(y)),
            );
        }
    }
    let mut filtri: Vec<String> = Vec::new();
    if let Some(l) = frame.blur {
        filtri.push(format!("blur({})", format_length(l)));
    }
    if let Some(n) = frame.brightness {
        filtri.push(format!("brightness({})", num(n)));
    }
    if let Some(n) = frame.saturate {
        filtri.push(format!("saturate({})", num(n)));
    }
    if let Some(n) = frame.hue_rotate {
        filtri.push(format!("hue-rotate({}deg)", num(n)));
    }
    if !filtri.is_empty() {
        scrivi("filter", filtri.join(" "));
    }
    if let Some(c) = frame.background_color.as_ref() {
        scrivi("background-color", risolvi_colore(c));
    }
    if let Some(c) = frame.color.as_ref() {
        scrivi("color", risolvi_colore(c));
    }
    if let Some(c) = frame.border_color.as_ref() {
        scrivi("border-color", risolvi_colore(c));
    }
    if let Some(ombra) = frame.shadow.as_ref() {
        scrivi("box-shadow", risolvi_ombra_parte(ombra));
    }
    if let Some(ritaglio) = frame.clip.as_ref() {
        scrivi("clip-path", compila_ritaglio(ritaglio));
    }
    righe
}

fn compila_ritaglio(ritaglio: &Ritaglio) -> String {
    match ritaglio {
        Ritaglio::Inset {
            top,
            right,
            bottom,
            left,
        } => format!(
            "inset({} {} {} {})",
            format_length(*top),
            format_length(*right),
            format_length(*bottom),
            format_length(*left)
        ),
        Ritaglio::Circle { radius, at } => format!(
            "circle({} at {} {})",
            format_length(*radius),
            format_length(at.0),
            format_length(at.1)
        ),
        Ritaglio::Polygon(punti) => format!(
            "polygon({})",
            punti
                .iter()
                .map(|(x, y)| format!("{} {}", format_length(*x), format_length(*y)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// I `@keyframes` di un'animazione nominata.
fn keyframes(nome: &str, def: &Animation) -> String {
    let mut css = format!("@keyframes {nome} {{\n");
    for frame in &def.frames {
        let _ = write!(
            css,
            "  {}% {{\n{}\n  }}\n",
            num(frame.at),
            fermata(frame, def.origin).join("\n")
        );
    }
    css.push_str("}\n");
    css
}

fn fotogramma_rotta(nome: &str, quando: &str, frame: RouteFrame) -> String {
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

/// Le variabili del movimento, le curve, i `@keyframes` di tutte le animazioni.
///
/// Escono tutte, anche quelle che nessuna parte richiama: un `@keyframes`
/// inerte non costa niente, e lo Studio deve poter mostrare un'animazione
/// appena dichiarata e non ancora assegnata.
fn compila_movimento(ambito: &Ambito, motion: &SkinMotion) -> String {
    let mut dichiarazioni = vec![
        dichiara("--motion-intensity", motion.intensity.as_str()),
        dichiara("--motion-scale", num(motion.intensity.scale())),
    ];
    for (nome, easing) in &motion.easings {
        dichiarazioni.push(dichiara(
            format!("{PREFISSO_MOTIVO}ease-{}", nel_suo_alfabeto(nome, TRATTINO)),
            format_easing(*easing),
        ));
    }
    let mut css = blocco(&ambito.radice(), &dichiarazioni);

    for (nome, def) in &motion.animations {
        let chiave = ambito.keyframes(nome);
        css.push_str(&keyframes(&chiave, def));
    }
    css
}

/// Le animazioni di una parte, divise per condizione.
struct Liste<'a> {
    /// Quel che gira sempre: l'entrata, lo scorrimento, il ciclo «finché suona».
    base: Vec<Voce<'a>>,
    /// Il cambio di contenuto, nei suoi due gemelli.
    cambio: Option<(Voce<'a>, Voce<'a>)>,
    /// Gli stati.
    stati: Vec<(PartState, Voce<'a>)>,
    /// C'è un ciclo «finché suona».
    suona: bool,
}

fn liste<'a>(stile: &'a PartStyle, ambito: &Ambito) -> Liste<'a> {
    let voce = |riferimento: &'a AnimationRef| Voce::di(ambito, riferimento);
    let mut base = Vec::new();
    // Sulle presenze l'entrata è una transizione di vista, non un'animazione
    // della regola base: nell'anteprima, dove le transizioni di vista non ci
    // sono, la si guarda con la timeline.
    if !stile.def.presenza
        && let Some(entrata) = stile.motion.get(AnimTrigger::Enter)
    {
        base.push(Voce {
            entrata: true,
            cascata: stile.motion.stagger,
            ..voce(entrata)
        });
    }
    if let Some(scorre) = stile.motion.get(AnimTrigger::InView) {
        base.push(Voce {
            vista: true,
            ..voce(scorre)
        });
    }
    let suona = stile
        .motion
        .get(AnimTrigger::State(PartState::Playing))
        .map(|giro| Voce {
            suona: true,
            ..voce(giro)
        });
    let ha_suona = suona.is_some();
    base.extend(suona);

    let cambio = stile.motion.get(AnimTrigger::Change).map(|cambia| {
        (
            Voce {
                keyframes: format!("{}-a", ambito.keyframes(&cambia.name)),
                ..voce(cambia)
            },
            Voce {
                keyframes: format!("{}-b", ambito.keyframes(&cambia.name)),
                ..voce(cambia)
            },
        )
    });

    let stati = PartState::ALL
        .iter()
        .filter(|s| **s != PartState::Playing)
        .filter_map(|s| {
            stile
                .motion
                .get(AnimTrigger::State(*s))
                .map(|r| (*s, voce(r)))
        })
        .collect();

    Liste {
        base,
        cambio,
        stati,
        suona: ha_suona,
    }
}

/// Le regole di una parte.
///
/// Il selettore include sempre `[data-skin='<id>']`, quindi una parte
/// ridisegnata da una skin non può influenzare un'altra skin. `radice` è la
/// radice del tema: la base, o la variante.
fn compila_parte(ambito: &Ambito, radice: &str, stile: &PartStyle, con_movimento: bool) -> String {
    let nome = stile.def.name;
    let base = format!("{radice} .{nome}");
    let mut dichiarazioni = compila_aspetto(&stile.appearance, ambito);
    if stile.before.is_some() {
        dichiarazioni.push(dichiara("isolation", "isolate"));
    }

    let liste = liste(stile, ambito);
    if con_movimento {
        if !liste.base.is_empty() {
            dichiarazioni.extend(dichiara_lista(&liste.base));
        }
        // Il ciclo «finché suona» parte fermo, e lo dice la parte **a sé
        // stessa**. Le proprietà personalizzate si ereditano: fino alla 3.0
        // una parte col suo ciclo dentro una che suonava — la copertina di
        // una riga dentro la colonna in ascolto — riceveva il `running` del
        // genitore e girava anche lei, senza suonare. Dichiarata qui, la
        // pausa vince sull'eredità; lo stato `playing` della parte stessa,
        // che viene dopo con la stessa specificità, vince sulla pausa.
        if liste.suona {
            dichiarazioni.push(dichiara("--skin-suona", "paused"));
        }
        if let Some(transizione) = stile.motion.transition.as_ref() {
            dichiarazioni.push(dichiara(
                "transition-duration",
                durata_scalata(&format_duration(transizione.duration)),
            ));
            dichiarazioni.push(dichiara(
                "transition-timing-function",
                curva(&transizione.easing),
            ));
        } else if stile.def.presenza && stile.motion.get(AnimTrigger::Enter).is_some() {
            // La skin dichiara la sua entrata: quella di serie, scritta con
            // `@starting-style` nel foglio del motore, non deve sommarsi.
            dichiarazioni.push(dichiara("transition", "none"));
        }
    }
    let mut css = blocco(&base, &dichiarazioni);

    for (livello, prima) in [(stile.before.as_ref(), true), (stile.after.as_ref(), false)] {
        if let Some(livello) = livello {
            let pseudo = if prima { "::before" } else { "::after" };
            css.push_str(&blocco(
                &format!("{base}{pseudo}"),
                &compila_livello(livello, prima, ambito),
            ));
        }
    }

    // Il ciclo «finché suona»: lo stato accende la variabile, e la lista
    // della regola base se ne accorge.
    if con_movimento && liste.suona {
        css.push_str(&blocco(
            &format!(
                "{base}:where({})",
                ambito.stato(PartState::Playing, stile.def.fuoco)
            ),
            &[dichiara("--skin-suona", "running")],
        ));
    }

    // Il cambio di contenuto: due gemelli che si alternano, così il motore vede
    // un nome nuovo a ogni cambio e riparte.
    if con_movimento && let Some((a, b)) = liste.cambio.as_ref() {
        for (gemello, voce) in [("a", a), ("b", b)] {
            let mut lista = liste.base.clone();
            lista.push(voce.clone());
            css.push_str(&blocco(
                &format!("{base}:where([data-cambio='{gemello}'])"),
                &dichiara_lista(&lista),
            ));
        }
    }

    // L'anello del fuoco: sta sotto lo stato del fuoco, qualunque sia la
    // regola in cui è stato scritto.
    if let Some(anello) = stile.appearance.focus_ring.as_ref() {
        css.push_str(&blocco(
            &format!(
                "{base}:where({})",
                ambito.stato(PartState::Focus, stile.def.fuoco)
            ),
            &compila_anello(anello),
        ));
    }

    for stato in PartState::ALL {
        let aspetto = stile.states.get(*stato);
        let animazione = con_movimento
            .then(|| liste.stati.iter().find(|(s, _)| s == stato))
            .flatten();
        if aspetto.is_none() && animazione.is_none() {
            continue;
        }
        let mut dichiarazioni = aspetto
            .map(|a| compila_aspetto(a, ambito))
            .unwrap_or_default();
        if let Some(anello) = aspetto.and_then(|a| a.focus_ring.as_ref()) {
            dichiarazioni.extend(compila_anello(anello));
        }
        if let Some((_, voce)) = animazione {
            let mut lista = liste.base.clone();
            lista.push(voce.clone());
            dichiarazioni.extend(dichiara_lista(&lista));
        }
        // `:where()` mantiene la specificità dello stato uguale a quella della
        // parte: senza, uno stato dichiarato da una skin vincerebbe su una
        // regola che il componente considera più importante.
        css.push_str(&blocco(
            &format!("{base}:where({})", ambito.stato(*stato, stile.def.fuoco)),
            &dichiarazioni,
        ));
    }

    // Il cambio di contenuto **dentro** uno stato animato.
    //
    // La regola di uno stato scrive la sua lista — la base più l'animazione
    // dello stato — e vince su quella del cambio, che le sta prima. Fino alla
    // 3.0 quindi, sopra una parte appena cambiata, il passaggio del mouse
    // toglieva dalla lista il gemello del cambio, e alla fine del passaggio la
    // regola del cambio lo rimetteva: il motore vedeva un'animazione nuova e
    // la faceva ripartire. Un titolo che entra con un guizzo guizzava di nuovo
    // a ogni passaggio del puntatore. Qui, dove ci sono tutti e due, la lista
    // porta il gemello **allo stesso posto** — subito dopo la base, come nella
    // regola del cambio — e l'animazione dello stato in coda: entrando e
    // uscendo dallo stato il gemello resta dov'era, e il motore lo lascia
    // andare. Sta prima delle righe scorse qui sotto, che restano l'ultima
    // parola nei contenitori già scorsi.
    if con_movimento && let Some((a, b)) = liste.cambio.as_ref() {
        for (stato, voce_stato) in &liste.stati {
            for (gemello, voce) in [("a", a), ("b", b)] {
                let mut lista = liste.base.clone();
                lista.push(voce.clone());
                lista.push(voce_stato.clone());
                css.push_str(&blocco(
                    &format!(
                        "{base}:where([data-cambio='{gemello}']):where({})",
                        ambito.stato(*stato, stile.def.fuoco)
                    ),
                    &dichiara_lista(&lista),
                ));
            }
        }
    }

    // Le righe che si rimontano mentre si scorre: tutto quel che gira, meno
    // l'entrata e la cascata. Vengono dopo le loro gemelle e hanno la stessa
    // specificità, quindi vincono solo dove il contenitore è stato scorso.
    if con_movimento && stile.def.riciclata && liste.base.iter().any(|v| v.entrata) {
        let senza: Vec<Voce<'_>> = liste.base.iter().filter(|v| !v.entrata).cloned().collect();
        let scorso = format!("{radice} :where([data-scorso]) .{nome}");
        css.push_str(&blocco(&scorso, &dichiara_lista(&senza)));
        for (stato, voce) in &liste.stati {
            let mut lista = senza.clone();
            lista.push(voce.clone());
            css.push_str(&blocco(
                &format!("{scorso}:where({})", ambito.stato(*stato, stile.def.fuoco)),
                &dichiara_lista(&lista),
            ));
        }
    }

    css
}

/// Le regole che fermano le animazioni di una parte, scritte dal compilatore.
///
/// Tre porte, e tutte e tre chiudono: la preferenza del sistema, quella chiesta
/// da Rymembra, e una skin a intensità `none`. È ciò che rende ammissibile un
/// ciclo infinito: si ferma esattamente come le quattro animazioni infinite del
/// motore, e una skin non può ometterlo.
fn regole_di_arresto(ambito: &Ambito, nome: &str) -> String {
    let ferma = [dichiara("animation", "none !important")];
    let mut css = String::new();
    let _ = write!(
        css,
        "@media (prefers-reduced-motion: reduce) {{\n{}}}\n",
        blocco(&format!("{} .{nome}", ambito.radice()), &ferma)
    );
    css.push_str(&blocco(
        &format!("{} .{nome}", ambito.radice_ferma_utente()),
        &ferma,
    ));
    css.push_str(&blocco(
        &format!("{} .{nome}", ambito.radice_ferma_intensita()),
        &ferma,
    ));
    css
}

/// Le transizioni di vista: la rotta, le presenze, le condivise.
///
/// Solo nella finestra: gli pseudo-elementi delle transizioni di vista esistono
/// alla radice del documento, e nell'anteprima non ce n'è nessuna — lì le
/// entrate e le uscite si guardano sulla timeline.
///
/// Una classe per innesco — `vt-<parte>-enter`, `-exit`, `-share` — e non una
/// per parte: `<ViewTransition>` mette la classe dell'innesco che sta
/// accadendo, e una regola per classe evita di dover distinguere un'entrata da
/// un'uscita guardando quali pseudo-elementi esistono.
fn compila_viste(ambito: &Ambito, skin: &SkinDocument) -> (String, Vec<Presenza>) {
    let radice = ambito.radice();
    let mut regole: Vec<(String, Vec<Dichiarazione>)> = Vec::new();
    let mut fotogrammi = String::new();
    let mut presenze = Vec::new();

    // La rotta: `page-content` è la presenza della pagina, e se la skin non
    // gli assegna un'entrata e un'uscita sue le prende da `motion.route`.
    let rotta = skin.motion.as_ref().and_then(|m| m.route.as_ref());
    let contenuto = skin.parts.iter().find(|p| p.def.name == "page-content");
    let mut pagina = Presenza {
        parte: "page-content",
        enter: false,
        exit: false,
        share: false,
    };
    for (verso, trigger, pseudo, quando) in [
        ("out", AnimTrigger::Exit, "view-transition-old", "to"),
        ("in", AnimTrigger::Enter, "view-transition-new", "from"),
    ] {
        let selettore = format!("{radice}::{pseudo}(.vt-page-content-{})", trigger.as_str());
        if let Some(propria) = contenuto.and_then(|p| p.motion.get(trigger)) {
            regole.push((
                selettore,
                vec![dichiara(
                    "animation",
                    Voce::di(ambito, propria).scorciatoia(),
                )],
            ));
        } else if let Some(r) = rotta {
            let fotogramma = if verso == "out" { r.out } else { r.enter };
            let Some(fotogramma) = fotogramma.filter(|f| !f.is_empty()) else {
                continue;
            };
            let nome = ambito.keyframes_rotta(verso);
            let tempo = durata_scalata(
                &r.duration
                    .map_or_else(|| "var(--dur-2)".to_owned(), format_duration),
            );
            let easing = r
                .easing
                .as_ref()
                .map_or_else(|| "var(--ease-out-expo)".to_owned(), curva);
            regole.push((
                selettore,
                vec![dichiara(
                    "animation",
                    format!("{nome} {tempo} {easing} both"),
                )],
            ));
            fotogrammi.push_str(&fotogramma_rotta(&nome, quando, fotogramma));
        } else {
            continue;
        }
        if trigger == AnimTrigger::Exit {
            pagina.exit = true;
        } else {
            pagina.enter = true;
        }
    }
    if pagina.enter || pagina.exit {
        presenze.push(pagina);
    }

    for stile in &skin.parts {
        if stile.def.name == "page-content" || !(stile.def.presenza || stile.def.condivisa) {
            continue;
        }
        let mut presenza = Presenza {
            parte: stile.def.name,
            enter: false,
            exit: false,
            share: false,
        };
        for (trigger, pseudo) in [
            (AnimTrigger::Enter, "view-transition-new"),
            (AnimTrigger::Exit, "view-transition-old"),
            (AnimTrigger::Share, "view-transition-group"),
        ] {
            if trigger == AnimTrigger::Enter && !stile.def.presenza {
                continue;
            }
            let Some(riferimento) = stile.motion.get(trigger) else {
                continue;
            };
            let selettore = format!(
                "{radice}::{pseudo}(.vt-{}-{})",
                stile.def.name,
                trigger.as_str()
            );
            let dichiarazioni = if trigger == AnimTrigger::Share {
                // Un volo condiviso non ha fotogrammi suoi: il motore sposta e
                // ridimensiona il gruppo da solo. La skin ne sceglie il tempo.
                let mut tempo = vec![
                    dichiara(
                        "animation-duration",
                        durata_scalata(&format_duration(riferimento.def.duration)),
                    ),
                    dichiara("animation-timing-function", curva(&riferimento.def.easing)),
                ];
                if let Some(attesa) = riferimento.def.delay {
                    tempo.push(dichiara(
                        "animation-delay",
                        durata_scalata(&format_duration(attesa)),
                    ));
                }
                tempo
            } else {
                vec![dichiara(
                    "animation",
                    Voce::di(ambito, riferimento).scorciatoia(),
                )]
            };
            regole.push((selettore, dichiarazioni));
            match trigger {
                AnimTrigger::Enter => presenza.enter = true,
                AnimTrigger::Exit => presenza.exit = true,
                _ => presenza.share = true,
            }
        }
        if presenza.enter || presenza.exit || presenza.share {
            presenze.push(presenza);
        }
    }

    let mut css = String::new();
    for (selettore, dichiarazioni) in &regole {
        css.push_str(&blocco(selettore, dichiarazioni));
    }
    css.push_str(&fotogrammi);

    // E le loro regole d'arresto, come per le parti.
    if !regole.is_empty() {
        let ferma = [dichiara("animation", "none !important")];
        let selettori: Vec<&str> = regole.iter().map(|(s, _)| s.as_str()).collect();
        let _ = write!(
            css,
            "@media (prefers-reduced-motion: reduce) {{
{}}}
",
            blocco(
                &selettori.join(
                    ",
"
                ),
                &ferma
            )
        );
        let utente: Vec<String> = selettori
            .iter()
            .map(|s| {
                s.replacen(
                    &radice,
                    &format!("{radice}[data-motion-utente='ridotto']"),
                    1,
                )
            })
            .collect();
        css.push_str(&blocco(
            &utente.join(
                ",
",
            ),
            &ferma,
        ));
    }

    (css, presenze)
}

// ── Lo scafale ──────────────────────────────────────────────────────────────

/// L'indirizzo di un nodo: il percorso degli indici dei figli.
///
/// `0-1-2` è «il terzo figlio del secondo figlio del primo figlio della
/// radice». Lo calcola **il compilatore** camminando l'albero, e il renderer
/// React deriva lo stesso indirizzo dallo stesso albero.
fn indirizzo(via: &[usize]) -> String {
    if via.is_empty() {
        return "radice".to_owned();
    }
    via.iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join("-")
}

/// Come una misura diventa `flex`.
///
/// `min-width: 0` e `min-height: 0` li mette **sempre** il compilatore: senza,
/// un figlio flex si rifiuta di scendere sotto la larghezza del suo contenuto.
fn misura_flex(size: crate::layout::TrackSize, riga: bool) -> Vec<Dichiarazione> {
    use crate::layout::TrackSize;

    let mut dichiarazioni = vec![
        dichiara("min-width", "0"),
        dichiara("min-height", "0"),
        dichiara(
            "flex",
            match size {
                TrackSize::Hug => "0 0 auto".to_owned(),
                TrackSize::Fill => "1 1 0".to_owned(),
                TrackSize::Fixed(length) => format!(
                    "0 0 clamp({}px, {}, {}px)",
                    num(crate::layout::FIXED_MIN),
                    format_length(length),
                    num(crate::layout::FIXED_MAX)
                ),
            },
        ),
    ];
    if let TrackSize::Fixed(length) = size {
        let proprieta = if riga { "width" } else { "height" };
        dichiarazioni.push(dichiara(
            proprieta,
            format!(
                "clamp({}px, {}, {}px)",
                num(crate::layout::FIXED_MIN),
                format_length(length),
                num(crate::layout::FIXED_MAX)
            ),
        ));
    }
    dichiarazioni
}

fn compila_zona(
    ambito: &Ambito,
    zona: &crate::layout::LayoutZone,
    via: &mut Vec<usize>,
    dentro_una_riga: bool,
    css: &mut String,
) {
    use crate::layout::{LayoutNode, ZoneKind};

    let mut dichiarazioni = vec![
        dichiara("display", "flex"),
        dichiara(
            "flex-direction",
            if zona.kind.is_row() { "row" } else { "column" },
        ),
        dichiara("align-items", zona.align.as_str()),
        dichiara("justify-content", zona.spread.css()),
    ];
    if let Some(gradino) = zona.gap.gradino() {
        dichiarazioni.push(dichiara("gap", format!("var(--spazio-{gradino})")));
    }
    if matches!(zona.kind, ZoneKind::Scroll) {
        dichiarazioni.push(dichiara("overflow-y", "auto"));
        dichiarazioni.push(dichiara("overflow-x", "hidden"));
    }
    if zona
        .children
        .iter()
        .any(|figlio| matches!(figlio, LayoutNode::Widget(w) if w.def.name == "content"))
    {
        dichiarazioni.push(dichiara("container-type", "inline-size"));
    }
    dichiarazioni.extend(misura_flex(zona.size, dentro_una_riga));

    css.push_str(&blocco(
        &format!("{} [data-nodo='{}']", ambito.radice(), indirizzo(via)),
        &dichiarazioni,
    ));

    for (indice, figlio) in zona.children.iter().enumerate() {
        via.push(indice);
        match figlio {
            LayoutNode::Zone(sotto) => compila_zona(ambito, sotto, via, zona.kind.is_row(), css),
            LayoutNode::Widget(istanza) => {
                css.push_str(&blocco(
                    &format!("{} [data-nodo='{}']", ambito.radice(), indirizzo(via)),
                    &misura_flex(istanza.size, zona.kind.is_row()),
                ));
            }
        }
        via.pop();
    }
}

fn compila_impaginazione(ambito: &Ambito, shell: &crate::layout::LayoutZone) -> String {
    let mut css = String::new();
    let mut via = Vec::with_capacity(crate::layout::MAX_SHELL_DEPTH);
    compila_zona(ambito, shell, &mut via, false, &mut css);
    css
}

// ── L'insieme ───────────────────────────────────────────────────────────────

/// Compila una skin per la finestra.
#[must_use]
pub fn compile_skin(skin: &SkinDocument) -> CompiledSkin {
    compila(skin, &Ambito::app(&skin.id))
}

/// Compila una skin per l'anteprima dello Studio.
///
/// Stesso documento, stesso compilatore, un altro ambito: vedi il `//!`.
#[must_use]
pub fn compile_skin_anteprima(skin: &SkinDocument) -> CompiledSkin {
    compila(skin, &Ambito::anteprima(&skin.id))
}

/// Compila una skin per la tela dello Studio: vedi [`Ambito::tela`].
#[must_use]
pub fn compile_skin_tela(skin: &SkinDocument) -> CompiledSkin {
    compila(skin, &Ambito::tela(&skin.id))
}

/// Compila una skin nell'ambito dato.
#[must_use]
pub fn compila(skin: &SkinDocument, ambito: &Ambito) -> CompiledSkin {
    let famiglie: Vec<(&str, String)> = skin
        .fonts
        .iter()
        .map(|f| (f.family.as_str(), ambito.famiglia(&f.family)))
        .collect();
    let famiglia = |nome: &str| {
        famiglie
            .iter()
            .find(|(scritto, _)| *scritto == nome)
            .map(|(_, proprio)| proprio.clone())
    };

    // L'intestazione porta l'id e la versione, e **nient'altro**. Fino alla 3.0
    // portava il nome, che è testo libero: un `*/` dentro il nome chiudeva il
    // commento e il resto del nome diventava foglio. Id e versione hanno un
    // alfabeto chiuso — minuscole, cifre e trattini; cifre e punti — e passano
    // comunque dal loro alfabeto qui, dove il commento si apre. È anche
    // l'unico commento del foglio: la prova sul CSS emesso lo pretende.
    let versione: String = skin
        .meta
        .version
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let mut css = format!(
        "/* skin {} {versione} — generato, non modificare a mano */\n",
        ambito.id
    );

    for font in &skin.fonts {
        let mut dichiarazioni = vec![
            dichiara(
                "font-family",
                format!("'{}'", ambito.famiglia(&font.family)),
            ),
            dichiara(
                "src",
                format!("url(\"{}\") format('woff2')", ambito.url_risorsa(&font.src)),
            ),
            dichiara(
                "font-weight",
                if font.weight.0 == font.weight.1 {
                    font.weight.0.to_string()
                } else {
                    format!("{} {}", font.weight.0, font.weight.1)
                },
            ),
            dichiara("font-style", if font.italic { "italic" } else { "normal" }),
        ];
        dichiarazioni.push(dichiara("font-display", "swap"));
        css.push_str(&blocco("@font-face", &dichiarazioni));
    }

    if ambito.in_anteprima() {
        css.push_str(&blocchi_di_serie(ambito));
    }

    let (base, dinamici) = compila_token(&skin.tokens, &famiglia);
    let mut dichiarazioni = compila_tavolozza(&skin.palette);
    dichiarazioni.extend(base);
    dichiarazioni.extend(calcolati(&skin.tokens, false));
    dichiarazioni.extend(compila_motivi(&skin.patterns, ambito));
    // `color-scheme` non è un token: dice al motore come disegnare le barre di
    // scorrimento e i controlli nativi, e segue il tema di base.
    dichiarazioni.push(dichiara("color-scheme", skin.scheme.as_str()));
    css.push_str(&blocco(&ambito.radice(), &dichiarazioni));

    let tema_variante = ambito.tema(skin.scheme.opposto());
    if let Some(variante) = skin.variant.as_ref() {
        let (mut dichiarazioni, _) = compila_token(&variante.tokens, &famiglia);
        dichiarazioni.extend(calcolati(&variante.tokens, false));
        dichiarazioni.extend(compila_tavolozza(&variante.palette));
        dichiarazioni.extend(compila_motivi(&variante.patterns, ambito));
        dichiarazioni.push(dichiara("color-scheme", skin.scheme.opposto().as_str()));
        css.push_str(&blocco(&tema_variante, &dichiarazioni));
    }

    if let Some(telefono) = skin.mobile.as_ref() {
        let (mut dichiarazioni, _) = compila_token(telefono, &famiglia);
        dichiarazioni.extend(calcolati(telefono, false));
        css.push_str(&blocco(&ambito.telefono(), &dichiarazioni));
    }

    if let Some(motion) = skin.motion.as_ref() {
        css.push_str(&compila_movimento(ambito, motion));
        // I gemelli del cambio: stessi fotogrammi, nome diverso.
        for stile in &skin.parts {
            if let Some(cambia) = stile.motion.get(AnimTrigger::Change) {
                for gemello in ["a", "b"] {
                    css.push_str(&keyframes(
                        &format!("{}-{gemello}", ambito.keyframes(&cambia.name)),
                        &cambia.def,
                    ));
                }
            }
        }
    }

    let presenze = if ambito.in_anteprima() {
        Vec::new()
    } else {
        let (viste, presenze) = compila_viste(ambito, skin);
        css.push_str(&viste);
        presenze
    };

    let radice = ambito.radice();
    let mut costo_parti = 0;
    let mut costo_movimento = 0;
    for stile in &skin.parts {
        costo_parti += stile.costo_superficie();
        costo_movimento += stile.motion_cost();
        css.push_str(&compila_parte(ambito, &radice, stile, true));
        if !stile.motion.senza_animazioni() {
            css.push_str(&regole_di_arresto(ambito, stile.def.name));
        }
    }
    if let Some(variante) = skin.variant.as_ref() {
        for stile in &variante.parts {
            css.push_str(&compila_parte(ambito, &tema_variante, stile, false));
        }
    }

    // Lo scafale esce **dopo** le parti: a parità di specificità vince chi viene
    // dopo, e le regole di impaginazione devono poter dire l'ultima parola su
    // `display` e `flex`.
    let difetto = crate::document::SkinLayout::default();
    let layout = skin.layout.as_ref().unwrap_or(&difetto);
    css.push_str(&compila_impaginazione(ambito, &layout.shell));

    CompiledSkin {
        id: skin.id.clone(),
        css,
        cost: stack_cost(
            &skin
                .patterns
                .iter()
                .map(|(_, e)| e.clone())
                .collect::<Vec<_>>(),
        ) + costo_parti,
        shell_cost: layout.shell.costo(),
        motion_cost: costo_movimento,
        dynamic_tokens: dinamici,
        presenze,
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
mod tests;
