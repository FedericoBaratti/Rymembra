//! Il registro dei token: il contratto fra le skin e i componenti.
//!
//! Questo contratto **esiste già**. `global.css` dichiara una sessantina di
//! proprietà personalizzate, i componenti le leggono, e le tre skin le
//! sovrascrivono sotto `:root[data-skin='<id>']`. Il problema non è che manchi:
//! è che non è scritto da nessuna parte. Per sapere quali token esistono si
//! devono leggere 3.341 righe di CSS in diciannove file, e per sapere quali sono
//! **obbligatori** non c'è modo — un token dimenticato non dà errore, dà una
//! skin visivamente rotta in un punto che può volerci un mese per notare.
//!
//! Qui il contratto è dati. Da qui si derivano la validazione, il controllo di
//! contrasto e il CSS.
//!
//! Due cose che il registro fa e i file CSS non facevano.
//!
//! **I token derivati.** `--accent` e `--accent-rgb` sono lo stesso colore in
//! due forme, perché i canvas del visualizer leggono la tripla per costruire
//! `rgba()` a runtime. Erano due dichiarazioni da tenere allineate a mano, e
//! `--cyber-fog-rgb` aveva perfino un commento in maiuscolo: «DEVE combaciare
//! con surface-0». Qui la tripla si deriva dal colore, e non può divergere.
//!
//! **I token calcolati.** `--shell-left`, `--player-clearance` e
//! `--transition-fast` non sono scelte di stile, sono conseguenze: la prima è la
//! larghezza del rail, la seconda l'altezza del player più due volte il suo
//! margine, la terza una durata più una curva. Una skin non deve poterle
//! contraddire, quindi non sono token — le emette il compilatore.

use crate::values::{Duration, Easing, Length, LengthValue, Rgba};

/// A cosa serve un token. Determina i raggruppamenti nell'editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenGroup {
    /// Caratteri.
    Typography,
    /// Fondi e pannelli.
    Surface,
    /// Colori del testo.
    Text,
    /// L'accento e le sue varianti.
    Accent,
    /// Errore, conferma, avviso.
    Status,
    /// La cornice dell'applicazione.
    Chrome,
    /// Misure della shell.
    Layout,
    /// La scala della spaziatura.
    Rhythm,
    /// Raggi e forme.
    Geometry,
    /// Quanto rilievo hanno le superfici.
    Depth,
    /// Ombre e aloni.
    Elevation,
    /// Durate e curve.
    Motion,
    /// Quel che leggono i canvas.
    Canvas,
}

/// Che tipo di valore accetta un token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// Un colore, un riferimento o una sorgente.
    Color,
    /// Una lunghezza, anche adattiva.
    Length,
    /// Una durata.
    Duration,
    /// Una curva.
    Easing,
    /// Un numero nudo, che legge il JavaScript.
    Number,
    /// Una pila di caratteri.
    FontStack,
    /// Un'ombra a livelli.
    Shadow,
}

/// Un token del registro.
///
/// Non è più `Eq`: [`Self::limiti`] contiene due `f64`, e `f64` non lo è. Il
/// confronto fra token non ne ha mai avuto bisogno — si confrontano per `id`,
/// che è la loro identità.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TokenDef {
    /// Il nome nel documento, es. `color.accent`.
    pub id: &'static str,
    /// Il nome della proprietà CSS. Viene dal vecchio albero e **non** si cambia
    /// a piacere: è quello che i componenti leggono.
    pub css: &'static str,
    /// Che valore accetta.
    pub kind: TokenKind,
    /// Dove sta nell'editor.
    pub group: TokenGroup,
    /// Indispensabile: una skin che non lo dichiara eredita quello di base.
    /// Serve a distinguere «non l'ho scelto» da «l'ho scelto uguale».
    pub required: bool,
    /// Emette anche la tripla `r g b` sotto questo nome. Solo per i colori che i
    /// canvas leggono a runtime.
    pub rgb_triple: Option<&'static str>,
    /// Gli estremi ammessi, per i token la cui libertà è limitata di proposito.
    ///
    /// Serve a due lettori diversi, ed è la ragione per cui sta nel registro
    /// invece che in un `match` del validatore: il validatore rifiuta quel che
    /// esce dagli estremi, e l'editor ne fa i capi del cursore. Un solo posto da
    /// cambiare per spostare un limite, e nessun modo di spostarne uno e
    /// dimenticare l'altro.
    ///
    /// `None` non vuol dire «senza limiti» in senso lato: vuol dire che il tipo
    /// del valore è già tutto il vincolo che serve. Un colore non ha estremi, e
    /// un raggio esagerato produce un pulsante buffo, non un'interfaccia rotta.
    pub limiti: Option<(f64, f64)>,
    /// A cosa serve, per chi lo legge nell'editor.
    pub description: &'static str,
}

/// Scorciatoia per tenere il registro leggibile: una riga per token.
macro_rules! token {
    ($id:literal, $css:literal, $kind:ident, $group:ident, $required:literal, $rgb:expr, $desc:literal) => {
        TokenDef {
            id: $id,
            css: $css,
            kind: TokenKind::$kind,
            group: TokenGroup::$group,
            required: $required,
            rgb_triple: $rgb,
            limiti: None,
            description: $desc,
        }
    };
}

/// Come [`token!`], per i pochi token che hanno estremi.
///
/// È una macro a parte e non un ottavo argomento di `token!` perché gli estremi
/// riguardano undici voci su cinquantotto: aggiungere un `None` alle altre
/// quarantasette metterebbe due `None` di fila su ogni riga della tabella, e da
/// lì in poi nessuno saprebbe più a vista quale dei due è la tripla.
macro_rules! token_limitato {
    ($id:literal, $css:literal, $kind:ident, $group:ident, $min:literal, $max:literal, $desc:literal) => {
        TokenDef {
            id: $id,
            css: $css,
            kind: TokenKind::$kind,
            group: TokenGroup::$group,
            required: false,
            rgb_triple: None,
            limiti: Some(($min, $max)),
            description: $desc,
        }
    };
}

/// Ogni token, col suo nome CSS.
///
/// L'ordine è quello dei pannelli dell'editor, non alfabetico: si sceglie un
/// colore di superficie prima di scegliere un'ombra. Ed è anche l'ordine in cui
/// il compilatore emette le dichiarazioni, così il foglio prodotto è
/// deterministico — che è ciò che rende utili gli snapshot e la cache.
///
/// Il registro è una tabella e si legge come tale: una riga per token. È il
/// motivo del `rustfmt::skip` — espanso a sei righe per voce diventerebbe
/// trecento righe in cui non si trova più niente, cioè l'opposto di ciò per cui
/// esiste.
#[rustfmt::skip]
pub static TOKENS: &[TokenDef] = &[
    // ── Tipografia ──────────────────────────────────────────────────────────
    token!("font.sans", "--font-sans", FontStack, Typography, true, None,
        "Carattere del corpo del testo."),
    token!("font.mono", "--font-mono", FontStack, Typography, false, None,
        "Carattere a spaziatura fissa: durate, etichette tecniche."),
    // Si chiama `--font-dot` per ragioni storiche: è nato nella skin Nothing per
    // il suo carattere a matrice di punti. I componenti lo leggono con quel
    // nome, quindi resta quello.
    token!("font.display", "--font-dot", FontStack, Typography, false, None,
        "Carattere da display: titoli grandi, numeri, eyebrow."),

    // ── Superfici ───────────────────────────────────────────────────────────
    token!("color.surface.0", "--color-surface-0", Color, Surface, true, None,
        "Il fondo dell'applicazione. È anche il colore dell'avvio a freddo."),
    token!("color.surface.1", "--color-surface-1", Color, Surface, true, None,
        "Pannelli e barre."),
    token!("color.surface.2", "--color-surface-2", Color, Surface, true, None,
        "Schede e righe."),
    token!("color.surface.3", "--color-surface-3", Color, Surface, true, None,
        "Elementi sollevati, stati attivi."),

    // ── Testo ───────────────────────────────────────────────────────────────
    token!("color.text.1", "--color-text-1", Color, Text, true, None,
        "Testo primario."),
    token!("color.text.2", "--color-text-2", Color, Text, true, None,
        "Testo secondario. Il controllo di contrasto guarda soprattutto questo."),
    token!("color.text.3", "--color-text-3", Color, Text, true, None,
        "Testo terziario, al limite della leggibilità: va verificato."),

    // ── Accento ─────────────────────────────────────────────────────────────
    token!("color.accent", "--accent", Color, Accent, true, Some("--accent-rgb"),
        "Accento primario. Può seguire la copertina in riproduzione."),
    token!("color.accent.soft", "--accent-soft", Color, Accent, false, None,
        "Accento a bassa opacità: sfondi di stato attivo."),
    token!("color.accent.glow", "--accent-glow", Color, Accent, false, None,
        "Accento per gli aloni."),
    token!("color.accent.like", "--accent-like", Color, Accent, false, None,
        "Riempimento del cuore. Nothing lo porta al rosso, il suo unico rosso."),
    // Solo tripla: i gradienti degli hero la usano dentro `rgba()` calcolate a
    // runtime dalla copertina, quindi emettere `--hero-rgb: rgb(...)` le
    // romperebbe tutte.
    token!("color.hero", "--hero-rgb", Color, Accent, false, Some("--hero-rgb"),
        "Tinta degli hero, di norma derivata dalla copertina."),

    // ── Semantici ───────────────────────────────────────────────────────────
    token!("color.danger", "--danger", Color, Status, true, None,
        "Errori e azioni distruttive."),
    token!("color.danger.soft", "--danger-soft", Color, Status, false, None,
        "Sfondo degli errori."),
    token!("color.success", "--success", Color, Status, true, None,
        "Conferme e operazioni riuscite."),
    token!("color.success.soft", "--success-soft", Color, Status, false, None,
        "Sfondo delle conferme."),
    token!("color.warning", "--warning", Color, Status, true, None,
        "Avvisi che non bloccano l'operazione."),
    token!("color.warning.soft", "--warning-soft", Color, Status, false, None,
        "Sfondo degli avvisi."),

    // ── Chrome ──────────────────────────────────────────────────────────────
    token!("color.sidebar", "--sidebar-bg", Color, Chrome, false, None,
        "Fondo della barra laterale."),
    token!("color.hairline", "--hairline", Color, Chrome, true, None,
        "Le linee da un pixel che separano le superfici."),
    token!("color.ambient.1", "--ambient-1", Color, Chrome, false, None,
        "Primo alone dello sfondo ambientale."),
    token!("color.ambient.2", "--ambient-2", Color, Chrome, false, None,
        "Secondo alone dello sfondo ambientale."),

    // ── Layout ──────────────────────────────────────────────────────────────
    token!("layout.rail", "--rail-w", Length, Layout, false, None,
        "Larghezza della barra laterale chiusa."),
    token!("layout.railExpanded", "--rail-w-expanded", Length, Layout, false, None,
        "Larghezza della barra laterale aperta."),
    token!("layout.playerHeight", "--player-h", Length, Layout, false, None,
        "Altezza della barra del player."),
    token!("layout.playerGap", "--player-gap", Length, Layout, false, None,
        "Margine attorno al player flottante."),
    token!("layout.contentX", "--content-x", Length, Layout, false, None,
        "Margine orizzontale del contenuto."),

    // ── Ritmo ───────────────────────────────────────────────────────────────
    // I cinque gradini della spaziatura. Erano costanti del motore, e non per
    // caso: una skin che potesse scrivere una spaziatura qualunque potrebbe far
    // uscire le cose dallo schermo. Gli estremi sono quella garanzia, scritta
    // dove si può verificare invece che ottenuta togliendo la manopola — sotto
    // zero non si scende e sopra i 64px non si sale, e la scala resta una scala.
    //
    // Restano **il ritmo di base**: `density` li moltiplica per `--densita-k`,
    // quindi le due manopole non si contraddicono e chi sceglie «compatto»
    // continua ad avere il compatto della propria scala.
    token_limitato!("space.1", "--ritmo-1", Length, Rhythm, 0.0, 64.0,
        "Il gradino più stretto: dentro un chip, fra un'icona e la sua etichetta."),
    token_limitato!("space.2", "--ritmo-2", Length, Rhythm, 0.0, 64.0,
        "Fra elementi affiancati della stessa riga."),
    token_limitato!("space.3", "--ritmo-3", Length, Rhythm, 0.0, 64.0,
        "Il gradino di base: dentro le schede, fra le righe di un elenco."),
    token_limitato!("space.4", "--ritmo-4", Length, Rhythm, 0.0, 64.0,
        "Fra i blocchi di una pagina."),
    token_limitato!("space.5", "--ritmo-5", Length, Rhythm, 0.0, 64.0,
        "Fra le sezioni grandi, e ai bordi delle schermate."),

    // ── Geometria ───────────────────────────────────────────────────────────
    token!("radius.panel", "--radius-panel", Length, Geometry, true, None,
        "Raggio dei pannelli. Cyberpunk lo porta a 4px, ed è metà della sua identità."),
    token!("radius.card", "--radius-card", Length, Geometry, true, None,
        "Raggio delle schede."),
    // Fattori e non misure: erano `calc(var(--radius-card) * 0.6)` scritti nel
    // foglio, cioè già derivati. Esporre il fattore invece della misura è quel
    // che tiene la promessa che `radius.card: 0` squadri **tutto** — con due
    // misure indipendenti, una skin che azzera le schede si ritroverebbe i
    // contenitori interni ancora tondi.
    token_limitato!("radius.inner", "--raggio-fattore-interno", Number, Geometry, 0.0, 1.0,
        "Quanto del raggio delle schede prendono i contenitori interni."),
    token_limitato!("radius.tiny", "--raggio-fattore-minuto", Number, Geometry, 0.0, 1.0,
        "Quanto ne prendono i dettagli minuti: pastiglie, caselle, maniglie."),

    // ── Profondità ──────────────────────────────────────────────────────────
    // Il modello di luce, come quantità e mai come colore.
    //
    // Le tre variabili sono `color-mix` fra token della skin dentro
    // `light-dark()`, e la composizione resta nel foglio: qui si dice **quanto**
    // rilievo, non **di che colore**. È la ragione per cui questi possono essere
    // token mentre `--spigolo` non poteva — una skin che scrivesse il colore
    // potrebbe accendere un filo bianco su una superficie bianca; una che ne
    // sceglie l'intensità, no. Zero appiattisce tutto, uno è quel che si vede
    // oggi, tre è il massimo prima che il rilievo diventi una fascia.
    token_limitato!("depth.edge", "--profondita-spigolo", Number, Depth, 0.0, 3.0,
        "Quanto risalta il filo di luce sul bordo alto delle superfici."),
    token_limitato!("depth.inset", "--profondita-incavo", Number, Depth, 0.0, 3.0,
        "Quanto sprofondano piste, solchi e campi."),
    token_limitato!("depth.lift", "--profondita-alzata", Number, Depth, 0.0, 3.0,
        "Quanto schiarisce la cima di una superficie."),
    token_limitato!("depth.sheen", "--profondita-luce", Length, Depth, 0.0, 240.0,
        "Fin dove scende la sfumatura di luce prima di esaurirsi."),

    // ── Elevazione ──────────────────────────────────────────────────────────
    token!("shadow.1", "--shadow-1", Shadow, Elevation, true, None,
        "Elevazione bassa."),
    token!("shadow.2", "--shadow-2", Shadow, Elevation, true, None,
        "Elevazione media."),
    token!("shadow.3", "--shadow-3", Shadow, Elevation, true, None,
        "Elevazione alta: overlay e finestre."),
    token!("shadow.player", "--shadow-player", Shadow, Elevation, false, None,
        "Elevazione del player flottante, con la sua hairline interna."),
    token!("glow.accent", "--glow-accent", Shadow, Elevation, false, None,
        "Alone d'accento riusabile."),

    // ── Movimento ───────────────────────────────────────────────────────────
    token!("motion.ease.outExpo", "--ease-out-expo", Easing, Motion, true, None,
        "Curva di uscita principale. La usano anche le transizioni di rotta."),
    token!("motion.ease.spring", "--ease-spring", Easing, Motion, false, None,
        "Curva con rimbalzo."),
    token!("motion.dur.1", "--dur-1", Duration, Motion, true, None,
        "Durata breve: hover, stati."),
    token!("motion.dur.2", "--dur-2", Duration, Motion, true, None,
        "Durata media: pannelli, entrate."),
    token!("motion.dur.3", "--dur-3", Duration, Motion, false, None,
        "Durata lunga: overlay a schermo intero."),

    // ── Canvas ──────────────────────────────────────────────────────────────
    // Non sono decorazione: sono il contratto che permette al visualizer e allo
    // scrubber, che disegnano su canvas in JavaScript, di seguire la skin senza
    // una riga di codice che sappia quale skin è attiva.
    token!("canvas.viz.primary", "--viz-primary", Color, Canvas, false, Some("--viz-primary-rgb"),
        "Barre dello spettro nel visualizer."),
    token!("canvas.viz.secondary", "--viz-secondary", Color, Canvas, false, Some("--viz-secondary-rgb"),
        "Anello dei bassi nel visualizer."),
    token!("canvas.viz.glow", "--viz-glow", Number, Canvas, false, None,
        "Raggio dell'alone del visualizer, in pixel. Numero nudo: lo legge il JavaScript."),
    token!("canvas.scrubber.glow", "--scrubber-glow", Number, Canvas, false, None,
        "Alone del playhead. Le skin piatte lo azzerano."),
    token!("canvas.scrubber.rest", "--scrubber-rest", Color, Canvas, false, None,
        "Onda non ancora riprodotta nello scrubber."),
];

/// Il token con questo nome, se esiste.
#[must_use]
pub fn token(id: &str) -> Option<&'static TokenDef> {
    TOKENS.iter().find(|def| def.id == id)
}

/// I token di un gruppo, per costruire i pannelli dell'editor dai dati.
#[must_use]
pub fn tokens_in_group(group: TokenGroup) -> Vec<&'static TokenDef> {
    TOKENS.iter().filter(|def| def.group == group).collect()
}

/// I token obbligatori.
#[must_use]
pub fn required_tokens() -> Vec<&'static TokenDef> {
    TOKENS.iter().filter(|def| def.required).collect()
}

// ── Riferimenti e sorgenti dinamiche ────────────────────────────────────────

/// Da dove un token può prendere il colore a runtime.
///
/// Sostituisce il flag `supportsDynamicAccent`, che nel vecchio albero era un
/// booleano sull'oggetto della skin e accendeva o spegneva il meccanismo in
/// blocco. Con una sorgente per token si può dire **quale** token segue la
/// copertina.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynamicSource {
    /// La tinta più viva della copertina.
    Vibrant,
    /// La più spenta.
    Muted,
    /// La viva, versione scura.
    DarkVibrant,
    /// La viva, versione chiara.
    LightVibrant,
}

impl DynamicSource {
    /// Come si scrive nel documento.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Vibrant => "albumArt.vibrant",
            Self::Muted => "albumArt.muted",
            Self::DarkVibrant => "albumArt.darkVibrant",
            Self::LightVibrant => "albumArt.lightVibrant",
        }
    }

    /// Tutte.
    pub const ALL: &'static [Self] = &[
        Self::Vibrant,
        Self::Muted,
        Self::DarkVibrant,
        Self::LightVibrant,
    ];

    /// Dal nome scritto nel documento.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|s| s.as_str() == raw)
    }
}

/// Un colore: letterale, riferito, locale, o legato alla copertina.
#[derive(Debug, Clone, PartialEq)]
pub enum ColorValue {
    /// Un colore scritto per esteso.
    Literal(Rgba),
    /// Un altro token del registro.
    ///
    /// Si conserva la **definizione**, non il nome scritto nel documento: da qui
    /// in poi il nome della variabile CSS viene dal registro, ed è la ragione
    /// per cui un riferimento non può diventare un canale di iniezione.
    Token(&'static TokenDef),
    /// Un colore della tavolozza locale della skin.
    Palette {
        /// Il nome, già validato.
        name: String,
        /// Opacità da applicare, per le varianti soft e glow dello stesso colore.
        alpha: Option<f64>,
    },
    /// Una tinta estratta dalla copertina.
    Source {
        /// Quale.
        source: DynamicSource,
        /// Opacità da applicare.
        alpha: Option<f64>,
    },
}

/// Il nome di un colore della tavolozza, o di un motivo, è ammesso?
///
/// Minuscole, cifre e trattini, e comincia con una lettera. Finisce in una
/// proprietà personalizzata con un prefisso, quindi vale la stessa regola dell'id.
#[must_use]
pub fn is_local_name(value: &str) -> bool {
    let mut caratteri = value.bytes();
    let primo = caratteri.next();
    !value.is_empty()
        && value.len() <= 40
        && primo.is_some_and(|b| b.is_ascii_lowercase())
        && caratteri.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

// ── Ombre ───────────────────────────────────────────────────────────────────

/// Un livello di un'ombra.
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowLayer {
    /// Verso l'interno.
    pub inset: bool,
    /// Spostamento orizzontale.
    pub x: Length,
    /// Spostamento verticale.
    pub y: Length,
    /// Sfocatura.
    pub blur: Length,
    /// Allargamento.
    pub spread: Option<Length>,
    /// Colore.
    pub color: ColorValue,
}

/// Un'ombra come lista di livelli, non come stringa.
///
/// Le ombre reali del progetto arrivano a tre livelli e mescolano `inset` con
/// ombre esterne: `--shadow-player` di `plain` è una hairline interna in alto
/// più una caduta profonda. Con la struttura, l'editor può mostrare un controllo
/// per livello invece di un campo di testo in cui si può scrivere qualunque cosa.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShadowValue {
    /// I livelli, dal primo disegnato.
    ///
    /// Zero livelli è legittimo e significa `none`: è così che una skin piatta
    /// spegne un'ombra senza doverne inventare una trasparente.
    pub layers: Vec<ShadowLayer>,
}

// ── Il valore di un token ───────────────────────────────────────────────────

/// Quel che un token può valere, già validato.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenValue {
    /// Un colore.
    Color(ColorValue),
    /// Una lunghezza.
    Length(LengthValue),
    /// Una durata.
    Duration(Duration),
    /// Una curva.
    Easing(Easing),
    /// Un numero nudo.
    Number(f64),
    /// Una pila di caratteri.
    FontStack(Vec<String>),
    /// Un'ombra.
    Shadow(ShadowValue),
}

/// I token dichiarati da una skin, o da uno dei suoi temi.
///
/// Le voci stanno in ordine di **registro**, non in ordine di scrittura nel
/// documento: l'ordinamento avviene una volta sola, qui, e da lì in poi il
/// compilatore itera e basta. Un output deterministico è ciò che rende utili gli
/// snapshot e la cache dell'anteprima.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TokenSet {
    voci: Vec<(&'static TokenDef, TokenValue)>,
}

impl TokenSet {
    /// Costruisce l'insieme mettendo le voci in ordine di registro.
    #[must_use]
    pub fn new(mut voci: Vec<(&'static TokenDef, TokenValue)>) -> Self {
        let posizione = |def: &TokenDef| TOKENS.iter().position(|altro| altro.id == def.id);
        voci.sort_by_key(|(def, _)| posizione(def).unwrap_or(usize::MAX));
        Self { voci }
    }

    /// Le voci, in ordine di registro.
    pub fn iter(&self) -> impl Iterator<Item = (&'static TokenDef, &TokenValue)> {
        self.voci.iter().map(|(def, value)| (*def, value))
    }

    /// Il valore di un token, se dichiarato.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&TokenValue> {
        self.voci
            .iter()
            .find(|(def, _)| def.id == id)
            .map(|(_, value)| value)
    }

    /// Il token è dichiarato?
    #[must_use]
    pub fn contains(&self, id: &str) -> bool {
        self.get(id).is_some()
    }

    /// Quanti token dichiara.
    #[must_use]
    pub fn len(&self) -> usize {
        self.voci.len()
    }

    /// Nessun token dichiarato.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.voci.is_empty()
    }
}

impl<'a> IntoIterator for &'a TokenSet {
    type Item = (&'static TokenDef, &'a TokenValue);
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter().collect::<Vec<_>>().into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn il_registro_non_ha_doppioni() {
        let mut id = HashSet::new();
        let mut css = HashSet::new();
        for def in TOKENS {
            assert!(id.insert(def.id), "id ripetuto: {}", def.id);
            // Il nome CSS è il contratto vero: due token che scrivono la stessa
            // proprietà sono due token di cui uno non ha effetto, e nessuno se
            // ne accorge finché non guarda lo schermo.
            assert!(css.insert(def.css), "proprietà ripetuta: {}", def.css);
        }
    }

    #[test]
    fn solo_i_colori_hanno_una_tripla() {
        for def in TOKENS {
            if def.rgb_triple.is_some() {
                assert_eq!(
                    def.kind,
                    TokenKind::Color,
                    "{} non è un colore ma dichiara una tripla",
                    def.id
                );
            }
        }
    }

    #[test]
    fn i_gruppi_coprono_tutto_il_registro() {
        let gruppi = [
            TokenGroup::Typography,
            TokenGroup::Surface,
            TokenGroup::Text,
            TokenGroup::Accent,
            TokenGroup::Status,
            TokenGroup::Chrome,
            TokenGroup::Layout,
            TokenGroup::Rhythm,
            TokenGroup::Geometry,
            TokenGroup::Depth,
            TokenGroup::Elevation,
            TokenGroup::Motion,
            TokenGroup::Canvas,
        ];
        let contati: usize = gruppi.iter().map(|g| tokens_in_group(*g).len()).sum();
        // Se un gruppo nuovo non finisce nei pannelli, i suoi token diventano
        // invisibili nell'editor pur essendo validi: è il tipo di silenzio che
        // il registro esiste per togliere.
        assert_eq!(contati, TOKENS.len());
    }

    #[test]
    fn gli_estremi_hanno_senso_e_solo_dove_servono() {
        for def in TOKENS {
            let Some((min, max)) = def.limiti else {
                continue;
            };
            assert!(min < max, "{}: estremi al contrario", def.id);
            // Un estremo su un colore o su una curva non avrebbe niente da
            // confrontare, e il validatore lo ignorerebbe in silenzio: meglio
            // che non compili la tabella.
            assert!(
                matches!(def.kind, TokenKind::Length | TokenKind::Number),
                "{} ha estremi ma è {:?}",
                def.id,
                def.kind
            );
        }
    }

    #[test]
    fn il_ritmo_e_la_profondita_non_sono_mai_obbligatori() {
        // Sono arrivati dopo il formato 1, e un token nuovo obbligatorio
        // trasformerebbe ogni skin già scritta in una skin con un avviso.
        for def in TOKENS {
            if matches!(def.group, TokenGroup::Rhythm | TokenGroup::Depth) {
                assert!(!def.required, "{} è obbligatorio", def.id);
            }
        }
    }

    #[test]
    fn le_voci_escono_in_ordine_di_registro() {
        let accento = token("color.accent").expect("nel registro");
        let superficie = token("color.surface.0").expect("nel registro");
        // Scritte al contrario dell'ordine del registro.
        let insieme = TokenSet::new(vec![
            (accento, TokenValue::Number(0.0)),
            (superficie, TokenValue::Number(0.0)),
        ]);
        let ordine: Vec<&str> = insieme.iter().map(|(def, _)| def.id).collect();
        assert_eq!(ordine, ["color.surface.0", "color.accent"]);
    }

    #[test]
    fn i_nomi_locali_stanno_in_un_alfabeto_chiuso() {
        assert!(is_local_name("teal"));
        assert!(is_local_name("cyber-red-2"));
        for nome in [
            "Teal",
            "2teal",
            "-teal",
            "teal_1",
            "teal red",
            "",
            &"a".repeat(41),
        ] {
            assert!(!is_local_name(nome), "accettato: {nome}");
        }
    }
}
