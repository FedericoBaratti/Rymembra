//! Il registro delle parti: le superfici che una skin può ridisegnare.
//!
//! Le classi elencate qui sotto **esistono già** e le skin le usano: `nothing`
//! ne aggancia 72, `cyberpunk` 90. Il problema è che la maggior parte **non ha
//! alcuna definizione di base**: esistono solo come agganci, messe nei
//! componenti perché una skin potesse afferrarle, e non sono documentate da
//! nessuna parte — si ricavano leggendo i selettori di diciannove file CSS.
//!
//! Le conseguenze di quel silenzio, entrambe reali:
//!
//! 1. rinominare una classe in un componente rompe in silenzio due skin, e il
//!    tipo di rottura è «un pannello non ha più il bordo giusto», che si nota
//!    settimane dopo;
//! 2. chi crea una skin non ha modo di sapere quali agganci esistono, quindi ne
//!    scopre alcuni leggendo il CSS di quelle già fatte, e altri mai.
//!
//! Il vocabolario di ciò che una skin può cambiare per parte è deliberatamente
//! corto. Non è CSS: sono sfondo, bordo, ritaglio, colore del testo, spaziatura,
//! più gli stati. Basta a riprodurre le skin esistenti e non basta a rompere il
//! layout — che è precisamente il confine giusto.
//!
//! # Qui non si scrive CSS
//!
//! Nel vecchio albero questo file emetteva selettori e dichiarazioni, cioè era
//! il secondo autore di CSS del sistema accanto al compilatore — e per non
//! dipendere da lui riceveva tre funzioni di resa con l'argomento tipizzato
//! `unknown`, che poi ricastava. Tre buchi nella disciplina dei tipi aperti per
//! evitare un ciclo fra moduli che in Rust non è un problema. Qui il registro è
//! dati e basta: ogni carattere di CSS esce da [`crate::compile`].

use crate::effects::{Effect, Paint, paint_effects};
use crate::movimento::PartAnimations;
use crate::tokens::ColorValue;
use crate::values::Length;

/// Dove sta una parte, per raggrupparla nell'editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartGroup {
    /// La cornice della finestra.
    Shell,
    /// La navigazione.
    Nav,
    /// Il corpo di una pagina.
    Page,
    /// Pulsanti e campi.
    Controls,
    /// Griglie ed elenchi.
    Lists,
    /// La barra del player.
    Player,
    /// La schermata In riproduzione.
    NowPlaying,
    /// Modali, menu, notifiche.
    Overlays,
}

/// Una parte del registro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartDef {
    /// Il nome, che è anche la classe CSS senza il punto.
    ///
    /// Non si rinomina: sta nel markup dei componenti e nelle skin già scritte.
    pub name: &'static str,
    /// Dove sta.
    pub group: PartGroup,
    /// A cosa serve.
    pub description: &'static str,
    /// Ha uno pseudo-elemento libero per un livello aggiuntivo.
    ///
    /// Conta perché è così che le skin esistenti fanno gli effetti sovrapposti —
    /// le scanline CRT di cyberpunk sono un `body::after`, la griglia a punti di
    /// nothing un `::before` su `.ambient-backdrop`. Dove lo pseudo-elemento è
    /// già usato dal componente per altro, qui è falso e il livello si rifiuta.
    pub layers: bool,
}

macro_rules! parte {
    ($name:literal, $group:ident, $layers:literal, $desc:literal) => {
        PartDef {
            name: $name,
            group: PartGroup::$group,
            description: $desc,
            layers: $layers,
        }
    };
}

/// Le parti ridisegnabili.
///
/// Come il registro dei token, è una tabella: una riga per parte, e il
/// `rustfmt::skip` serve a tenerla leggibile.
#[rustfmt::skip]
pub static PARTS: &[PartDef] = &[
    // ── Cornice e navigazione ───────────────────────────────────────────────
    parte!("app-shell", Shell, false, "Il contenitore di tutta la finestra."),
    parte!("ambient-backdrop", Shell, true,
        "Il fondo dietro al contenuto. È qui che vivono griglie, pavimenti prospettici e campi di punti."),
    parte!("player-shell", Player, true, "La barra del player flottante."),
    parte!("bottom-nav", Nav, true, "La navigazione inferiore, su schermi stretti."),
    parte!("nav-pill", Nav, false, "La voce di navigazione, compreso lo stato attivo."),

    // ── Pagina ──────────────────────────────────────────────────────────────
    parte!("page-header", Page, false, "L'intestazione di una pagina."),
    parte!("page-title", Page, false, "Il titolo grande di una pagina."),
    parte!("page-subtitle", Page, false, "Il sottotitolo sotto il titolo di pagina."),
    parte!("hero-eyebrow", Page, false,
        "L'etichetta piccola sopra un titolo hero, di norma in maiuscole."),
    parte!("hero-art", Page, true, "L'immagine grande di un hero."),
    parte!("section-card", Page, true,
        "La scheda che contiene una sezione. La superficie più riusata dell'app."),
    parte!("section-heading", Page, false, "Il titolo di una sezione dentro una pagina."),
    parte!("section-icon", Page, false, "L'icona accanto al titolo di sezione."),
    parte!("stat-number", Page, false, "Un numero grande nelle statistiche."),

    // ── Controlli ───────────────────────────────────────────────────────────
    parte!("icon-btn", Controls, false, "Il pulsante con la sola icona."),
    parte!("play-btn-primary", Controls, true, "Il pulsante di riproduzione principale."),
    parte!("btn-accent", Controls, false, "Il pulsante d'azione primaria."),
    parte!("btn-ghost", Controls, false, "Il pulsante secondario, senza fondo."),
    parte!("switch", Controls, false, "L'interruttore, nel suo insieme."),
    parte!("switch-track", Controls, false, "La pista dell'interruttore."),
    parte!("field-input", Controls, false, "Il campo di testo."),
    parte!("range-accent", Controls, false, "Il cursore a scorrimento."),
    parte!("tooltip-pill", Controls, false, "Il suggerimento al passaggio."),
    parte!("segmented", Controls, false,
        "Il gruppo di linguette che sceglie fra due o tre viste dello stesso contenuto."),

    // ── Elenchi ─────────────────────────────────────────────────────────────
    parte!("track-grid", Lists, false, "La griglia delle tracce o degli album."),
    parte!("list-row", Lists, false,
        "La singola riga di un elenco, compresi lo stato attivo e quello selezionato."),
    parte!("queue-list", Lists, false, "La coda di riproduzione."),
    parte!("empty-state", Lists, true, "Il riquadro mostrato quando non c'è niente."),
    parte!("empty-icon", Lists, false, "L'icona dello stato vuoto."),
    parte!("skeleton", Lists, true,
        "Il segnaposto durante il caricamento. La sua animazione è parte dell'identità della skin."),

    // ── Player ──────────────────────────────────────────────────────────────
    parte!("player-progress", Player, true, "La barra di avanzamento del player."),
    parte!("progress-sheen", Player, false,
        "Il riflesso che scorre sulla barra di avanzamento."),
    parte!("selection-bar", Player, true,
        "La barra che prende il posto del player quando ci sono più brani selezionati."),

    // ── In riproduzione ─────────────────────────────────────────────────────
    parte!("np-screen", NowPlaying, true, "La schermata In riproduzione."),
    parte!("np-art", NowPlaying, true, "La copertina in grande."),
    parte!("np-title", NowPlaying, false, "Il titolo del brano in riproduzione."),
    parte!("np-meta", NowPlaying, false, "Artista e album sotto il titolo."),
    parte!("np-formato", NowPlaying, false,
        "I dati tecnici del file sotto i comandi: formato, frequenza, canali, bitrate."),
    parte!("np-transport", NowPlaying, false, "I comandi di riproduzione."),
    parte!("np-scrim", NowPlaying, false, "Il velo sopra la copertina, per leggere il testo."),
    parte!("lyrics-screen", NowPlaying, true, "La schermata del testo."),
    parte!("lyric-line", NowPlaying, false, "La riga di testo, attiva e non."),
    parte!("lyric-word", NowPlaying, false, "La parola del testo, quando i tempi ci sono."),
    parte!("viz-screen", NowPlaying, true, "La schermata del visualizer."),
    parte!("eq-bars", NowPlaying, false, "Le barre dell'equalizzatore."),
    parte!("eq-slider", NowPlaying, false, "Il cursore di una banda dell'equalizzatore."),

    // ── Sovrapposizioni ─────────────────────────────────────────────────────
    parte!("glass-modal", Overlays, true, "La finestra modale."),
    parte!("menu-pop", Overlays, true, "Il menu contestuale."),
    parte!("toast-card", Overlays, true, "La notifica temporanea."),
    parte!("toast-progress", Overlays, false, "La barra di durata di una notifica."),
    parte!("tour-tooltip", Overlays, true, "Il fumetto della presentazione guidata."),
];

/// Le parti ritirate: si accettano e non fanno niente.
///
/// Una parte è un nome pubblico. Sta nelle skin che qualcuno ha già scritto e
/// spedito, e il lettore di documenti tratta un nome sconosciuto come un
/// **errore duro**, non come un avviso: chi ha stilato `viz-title` nel 2.2.0 si
/// troverebbe una skin che non si apre più, per una riga che non disegnava
/// niente neanche prima. Togliere la voce dal registro senza questo elenco
/// sarebbe quindi una rottura di compatibilità travestita da pulizia.
///
/// Il patto è: il nome continua a essere accettato dal lettore, non compare
/// nell'editor, non genera CSS e non suggerisce nulla ai refusi. Sparisce dal
/// futuro senza rompere il passato — che è la sola forma di ritiro che si possa
/// fare su un formato spedito.
///
/// La seconda voce della coppia è il perché, in una riga, ed è ciò che si
/// mostra a chi chiede conto del nome. Se il perché non si riesce a scrivere,
/// molto probabilmente la parte non va ritirata: va disegnata.
/// È una tabella come [`PARTS`], e per la stessa ragione porta il
/// `rustfmt::skip`: una riga per nome ritirato, col perché accanto.
#[rustfmt::skip]
pub static RITIRATE: &[(&str, &str)] = &[
    ("viz-title", "il visualizer non ha un titolo, e non l'avrà: vedi DettaglioSpettro."),
    ("home-shortcuts", "la schermata iniziale non ha scorciatoie, e nessun pacchetto le prevede: un'attesa senza proprietario è una promessa che lo Studio non può mantenere."),
];

/// Perché questa parte è stata ritirata, se lo è.
#[must_use]
pub fn ritirata(name: &str) -> Option<&'static str> {
    RITIRATE
        .iter()
        .find(|(nome, _)| *nome == name)
        .map(|(_, perche)| *perche)
}

/// La parte con questo nome, se esiste.
///
/// Una parte ritirata non esiste: questa funzione dice `None`, ed è giusto così
/// — l'editor, il compilatore e i suggerimenti sui refusi devono ignorarla. Chi
/// deve invece *accettarla* è il solo lettore di documenti, che per questo
/// chiede a [`ritirata`] prima di dichiarare un errore.
#[must_use]
pub fn part(name: &str) -> Option<&'static PartDef> {
    PARTS.iter().find(|def| def.name == name)
}

/// Le parti di un gruppo.
#[must_use]
pub fn parts_in_group(group: PartGroup) -> Vec<&'static PartDef> {
    PARTS.iter().filter(|def| def.group == group).collect()
}

/// Nomi vicini a quello scritto, per il messaggio d'errore.
#[must_use]
pub fn nearest_parts(unknown: &str) -> Vec<&'static str> {
    crate::vicini::vicini(unknown, PARTS.iter().map(|def| def.name))
}

// ── Cosa una skin può cambiare, per parte ───────────────────────────────────

/// Gli stati.
///
/// Solo questi quattro, e sono quelli che le skin esistenti usano davvero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartState {
    /// Il puntatore è sopra.
    Hover,
    /// È l'elemento attivo.
    Active,
    /// Ha il fuoco da tastiera.
    Focus,
    /// È disabilitato.
    Disabled,
}

impl PartState {
    /// Come si scrive nel documento.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hover => "hover",
            Self::Active => "active",
            Self::Focus => "focus",
            Self::Disabled => "disabled",
        }
    }

    /// Il selettore che lo riconosce.
    ///
    /// `focus` è `:focus-visible` e non `:focus`: lo stato deve apparire a chi
    /// naviga da tastiera e non a ogni clic del mouse. Nel vecchio albero le
    /// skin lo scrivevano correttamente, e vale conservarlo per costruzione
    /// invece che per disciplina.
    #[must_use]
    pub const fn selector(self) -> &'static str {
        match self {
            Self::Hover => ":hover",
            Self::Active => "[data-active], .is-active",
            Self::Focus => ":focus-visible",
            Self::Disabled => ":disabled, [aria-disabled=\"true\"]",
        }
    }

    /// Tutti, nell'ordine in cui il compilatore li emette.
    pub const ALL: &'static [Self] = &[Self::Hover, Self::Active, Self::Focus, Self::Disabled];
}

/// Come si trasforma il testo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextTransform {
    /// Com'è scritto.
    None,
    /// Tutto maiuscolo.
    Uppercase,
    /// Tutto minuscolo.
    Lowercase,
    /// Iniziali maiuscole.
    Capitalize,
}

impl TextTransform {
    /// Come si scrive in CSS. È anche il nome nel documento.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Uppercase => "uppercase",
            Self::Lowercase => "lowercase",
            Self::Capitalize => "capitalize",
        }
    }

    /// Tutte.
    pub const ALL: &'static [Self] = &[
        Self::None,
        Self::Uppercase,
        Self::Lowercase,
        Self::Capitalize,
    ];

    /// Dal nome scritto nel documento.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.as_str() == raw)
    }
}

/// L'aspetto di una parte, o di uno dei suoi stati.
///
/// Cosa **non** c'è, e per scelta: `width`, `height`, `margin`, `padding`,
/// `position`, `display`. Una skin che può spostare le cose può anche
/// sovrapporle, nasconderle o portarle fuori schermo — e il risultato non
/// sarebbe una skin brutta, ma un'app inutilizzabile che sembra un bug dell'app.
///
/// Cosa non c'è più rispetto al vecchio albero: `shadow`. Era dichiarato come
/// una lista di effetti con lunghezza massima **zero**, cioè un campo che poteva
/// valere soltanto la lista vuota. Un campo che non accetta nessun valore utile
/// non è un'estensione futura, è una riga che chi legge lo schema deve capire e
/// poi scartare.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PartAppearance {
    /// Livelli di sfondo, dal più basso. Il costo sommato è soggetto al budget.
    pub background: Vec<Paint>,
    /// Colore del testo.
    pub text_color: Option<ColorValue>,
    /// Colore del bordo.
    pub border_color: Option<ColorValue>,
    /// Spessore del bordo.
    pub border_width: Option<Length>,
    /// Raggio degli angoli.
    pub radius: Option<Length>,
    /// Un ritaglio, di norma un `chamfer`.
    pub clip: Option<Paint>,
    /// Un filtro su quel che sta **dietro**, di norma un `blurBehind`.
    ///
    /// # Perché è arrivato dopo
    ///
    /// Non c'era, e la sua assenza era un buco silenzioso: `blurBehind` si
    /// poteva dichiarare come motivo, [`EffectTarget::Filter`] sapeva già che
    /// finisce in `backdrop-filter`, e poi **nessun campo poteva riferirlo**.
    /// Una skin che ci provava riceveva un messaggio d'errore giusto su una
    /// strada che non esisteva.
    ///
    /// Costa dieci, cioè il budget intero di una skin: è il modo del formato di
    /// dire che di sfocature dietro una superficie se ne può avere una sola.
    pub filter: Option<Paint>,
    /// Opacità.
    pub opacity: Option<f64>,
    /// Spaziatura fra le lettere.
    pub letter_spacing: Option<Length>,
    /// Trasformazione del testo.
    pub text_transform: Option<TextTransform>,
    /// Peso del carattere.
    pub font_weight: Option<f64>,
}

impl PartAppearance {
    /// Non dichiara niente: il compilatore non emette la regola.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Il livello aggiuntivo su `::after`.
#[derive(Debug, Clone, PartialEq)]
pub struct PartLayer {
    /// Gli sfondi del livello.
    pub background: Vec<Paint>,
    /// Opacità del livello.
    pub opacity: Option<f64>,
}

/// Gli stati dichiarati da una skin per una parte.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PartStates {
    /// Al passaggio del puntatore.
    pub hover: Option<PartAppearance>,
    /// Quando è attivo.
    pub active: Option<PartAppearance>,
    /// Col fuoco da tastiera.
    pub focus: Option<PartAppearance>,
    /// Quando è disabilitato.
    pub disabled: Option<PartAppearance>,
}

impl PartStates {
    /// L'aspetto di uno stato, se dichiarato.
    #[must_use]
    pub const fn get(&self, state: PartState) -> Option<&PartAppearance> {
        match state {
            PartState::Hover => self.hover.as_ref(),
            PartState::Active => self.active.as_ref(),
            PartState::Focus => self.focus.as_ref(),
            PartState::Disabled => self.disabled.as_ref(),
        }
    }
}

/// Una parte ridisegnata.
#[derive(Debug, Clone, PartialEq)]
pub struct PartStyle {
    /// Quale parte.
    pub def: &'static PartDef,
    /// L'aspetto di base.
    pub appearance: PartAppearance,
    /// Il livello su `::after`, ammesso solo dove `layers` è vero.
    pub layer: Option<PartLayer>,
    /// Gli stati.
    pub states: PartStates,
    /// Le animazioni assegnate, per trigger.
    ///
    /// Sta qui e non in [`PartAppearance`] perché un'animazione è un **legame**
    /// e non un valore d'aspetto, e perché `PartAppearance` è la stessa
    /// identica struttura per la base e per i quattro stati: un campo che lì
    /// valesse solo alla base comparirebbe anche in `states.hover`, si potrebbe
    /// scrivere, e non farebbe niente. L'argomento per esteso sta nel `//!` di
    /// [`crate::movimento`].
    pub animations: PartAnimations,
}

impl PartStyle {
    /// Tutti gli effetti che questa parte disegna, per contarne il costo.
    #[must_use]
    pub fn effects(&self) -> Vec<Effect> {
        let mut tutti = paint_effects(&self.appearance.background);
        if let Some(layer) = self.layer.as_ref() {
            tutti.extend(paint_effects(&layer.background));
        }
        if let Some(clip) = self.appearance.clip.as_ref() {
            tutti.push(clip.effect().clone());
        }
        // Il filtro **deve** entrare nel conto, ed è la ragione per cui il
        // budget esiste: un `blurBehind` costa dieci da solo, e senza questa
        // riga una skin potrebbe metterne uno su ogni superficie senza che
        // niente lo dica.
        if let Some(filter) = self.appearance.filter.as_ref() {
            tutti.push(filter.effect().clone());
        }
        tutti
    }

    /// Quanto costa il movimento di questa parte.
    ///
    /// Separato da [`Self::effects`] e mai sommato con quello: sono due budget
    /// che misurano due cose diverse — quanto costa **disegnare** una
    /// superficie e quanto costa **muoverla** — e una somma non risponderebbe a
    /// nessuna delle due. È la stessa distinzione che
    /// [`CompiledSkin`](crate::CompiledSkin) fa già fra `cost` e `shell_cost`.
    #[must_use]
    pub fn motion_cost(&self) -> u32 {
        self.animations.costo()
    }

    /// Le animazioni che questa parte richiama, per trigger.
    ///
    /// Gemella di [`Self::patterns_used`], e serve alla stessa domanda su un
    /// altro registro: quale animazione dichiarata non la usa nessuno.
    #[must_use]
    pub fn animations_used(&self) -> Vec<&str> {
        self.animations.nomi_usati()
    }

    /// I motivi che questa parte richiama, base e stati.
    ///
    /// Serve al conteggio che rende `UnusedPattern` finalmente producibile: un
    /// motivo richiamato **solo** al passaggio del mouse è un motivo usato,
    /// esattamente come per i colori in `appearances()`.
    #[must_use]
    pub fn patterns_used(&self) -> Vec<&str> {
        let mut nomi: Vec<&str> = Vec::new();
        for aspetto in self.appearances() {
            nomi.extend(aspetto.background.iter().filter_map(Paint::pattern_name));
            nomi.extend(aspetto.clip.as_ref().and_then(Paint::pattern_name));
            nomi.extend(aspetto.filter.as_ref().and_then(Paint::pattern_name));
        }
        if let Some(layer) = self.layer.as_ref() {
            nomi.extend(layer.background.iter().filter_map(Paint::pattern_name));
        }
        nomi
    }

    /// L'aspetto di base e quelli degli stati dichiarati.
    ///
    /// Gli stati contano quanto la base: un colore usato **solo** al passaggio
    /// del mouse è un colore usato, e un conteggio che li saltasse direbbe «mai
    /// usato» di un colore che si vede eccome.
    #[must_use]
    pub fn appearances(&self) -> Vec<&PartAppearance> {
        let mut tutti = vec![&self.appearance];
        tutti.extend(PartState::ALL.iter().filter_map(|s| self.states.get(*s)));
        tutti
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn il_registro_non_ha_doppioni() {
        let mut visti = HashSet::new();
        for def in PARTS {
            assert!(visti.insert(def.name), "parte ripetuta: {}", def.name);
        }
    }

    #[test]
    fn una_parte_ritirata_non_e_anche_viva() {
        for (nome, perche) in RITIRATE {
            // Se un nome stesse in tutti e due gli elenchi il lettore
            // prenderebbe il ramo del ritiro e la parte smetterebbe di
            // disegnarsi, restando visibile nell'editor: la peggiore delle due
            // risposte sbagliate, perché sembra che la skin non funzioni.
            assert!(
                part(nome).is_none(),
                "«{nome}» è ritirata e insieme viva nel registro"
            );
            assert!(!perche.is_empty(), "«{nome}» è ritirata senza un perché");
            // Nemmeno come suggerimento: chi sbaglia a scrivere una parte viva
            // non va mandato su un nome che non fa niente.
            assert!(!nearest_parts(nome).contains(nome));
        }
        assert!(ritirata("viz-title").is_some_and(|perche| perche.contains("DettaglioSpettro")));
        assert_eq!(ritirata("viz-screen"), None);
    }

    #[test]
    fn un_nome_sbagliato_per_assonanza_riceve_un_suggerimento() {
        // I tre errori che si fanno davvero: il plurale, il sinonimo, il prefisso.
        assert!(nearest_parts("section-cards").contains(&"section-card"));
        assert!(nearest_parts("np-titles").contains(&"np-title"));
        assert!(nearest_parts("player-bar").contains(&"player-shell"));
        // E quando non c'è niente di simile, meglio nessun suggerimento che uno
        // a caso: un candidato sbagliato manda a leggere il registro comunque,
        // ma dopo aver provato.
        assert!(nearest_parts("zzzzzz").is_empty());
    }

    #[test]
    fn i_gruppi_coprono_tutto_il_registro() {
        let gruppi = [
            PartGroup::Shell,
            PartGroup::Nav,
            PartGroup::Page,
            PartGroup::Controls,
            PartGroup::Lists,
            PartGroup::Player,
            PartGroup::NowPlaying,
            PartGroup::Overlays,
        ];
        let contate: usize = gruppi.iter().map(|g| parts_in_group(*g).len()).sum();
        assert_eq!(contate, PARTS.len());
    }

    #[test]
    fn il_fuoco_e_quello_da_tastiera() {
        // `:focus` mostrerebbe lo stato a ogni clic del mouse, che è il difetto
        // che il vecchio albero evitava per disciplina e qui è per costruzione.
        assert_eq!(PartState::Focus.selector(), ":focus-visible");
    }
}
