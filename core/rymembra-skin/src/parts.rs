//! Il registro delle parti: le superfici che una skin può ridisegnare.
//!
//! Le classi elencate qui sotto **esistono** nel markup dei componenti: ogni
//! elemento porta la sua classe semantica italiana e, accanto, il nome della
//! parte — `className="riga list-row"`. Il nome della parte è il contratto: una
//! skin lo scrive, il compilatore ne fa un selettore, e `strumenti/classi.js`
//! controlla che il markup lo emetta davvero.
//!
//! # Da cinquantuno a cento
//!
//! Il formato 1 aveva cinquantuno parti contro trecentocinquanta classi del
//! foglio, e le superfici più visibili della grafica di oggi — la terza colonna,
//! la barra laterale, le schede della Home, i tasti della finestra — non erano
//! nel registro: una skin le poteva solo guardare. Il formato 2 le aggiunge, e
//! **divide** le classi che il formato 1 prestava: `list-row` stava sulle righe
//! e sulle schede, `glass-modal` sulle finestrelle e sulla coda, `menu-pop` sui
//! menù e sull'equalizzatore, `hero-art` sull'hero e sulla copertina della
//! colonna. Ridipingere l'una cambiava l'altra. La migrazione copia lo stile del
//! nome vecchio sul nome nuovo, quindi una skin del formato 1 resta com'era.
//!
//! # Cosa una skin può cambiare, e cosa no
//!
//! Sfondo, bordo, raggio, ombra, anello del fuoco, ritaglio, filtri, colore e
//! misura del testo, più gli stati e il movimento. Non c'è `width`, `height`,
//! `margin`, `padding`, `position`, `display`: una skin che può spostare le
//! cose può anche sovrapporle, nasconderle o portarle fuori schermo, e il
//! risultato non sarebbe una skin brutta ma un'app inservibile che sembra un bug
//! dell'app.
//!
//! # Qui non si scrive CSS
//!
//! Il registro è dati e basta: ogni carattere di CSS esce da [`crate::compile`].

use crate::effects::{Effect, FiltroElemento, Paint, paint_effects};
use crate::movimento::PartMotion;
use crate::stati::{Fuoco, PartState};
use crate::tokens::{ColorValue, ShadowValue, TokenDef};
use crate::values::{FontKind, Length};

/// Dove sta una parte, per raggrupparla nell'editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartGroup {
    /// La cornice della finestra.
    Shell,
    /// La navigazione.
    Nav,
    /// Il corpo di una pagina.
    Page,
    /// Pulsanti, campi e piccoli comandi.
    Controls,
    /// Elenchi e righe.
    Lists,
    /// Schede: album, brani, raccolte, artisti.
    Cards,
    /// Il lettore, la coda, l'equalizzatore.
    Player,
    /// La terza colonna.
    Column,
    /// La schermata In riproduzione, il testo, lo spettro.
    NowPlaying,
    /// Modali, menù, notifiche, il giro guidato.
    Overlays,
}

impl PartGroup {
    /// Tutti, nell'ordine in cui l'editor li mostra.
    pub const ALL: &'static [Self] = &[
        Self::Shell,
        Self::Nav,
        Self::Page,
        Self::Controls,
        Self::Lists,
        Self::Cards,
        Self::Player,
        Self::Column,
        Self::NowPlaying,
        Self::Overlays,
    ];
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
    /// `::before` è libero per un livello della skin.
    pub before: bool,
    /// `::after` è libero per un livello della skin.
    ///
    /// Dove lo pseudo-elemento è già usato dal componente — i tasti della
    /// finestra disegnano il loro fondo su `::before` — o non esiste affatto —
    /// una copertina è un `<img>`, e un'immagine non ha pseudo-elementi — qui è
    /// falso, e il livello si rifiuta invece di non comparire.
    pub after: bool,
    /// Entra ed esce dal documento come un'unità: si può animare l'uscita.
    ///
    /// Un'uscita in CSS puro non esiste — per animarla qualcuno deve tenere
    /// l'elemento montato finché l'animazione finisce — e chi lo fa è
    /// `<ViewTransition>` nel renderer, attorno a queste parti e a nessun'altra.
    pub presenza: bool,
    /// Può volare da un posto all'altro fra due schermate: la copertina della
    /// griglia che diventa quella della pagina dell'album.
    pub condivisa: bool,
    /// Dove sta il fuoco che accende lo stato `focus`.
    pub fuoco: Fuoco,
    /// Vive in un elenco virtualizzato, dove una riga si rimonta a ogni
    /// scorrimento: la sua entrata non deve ripartire mentre si scorre.
    pub riciclata: bool,
}

impl PartDef {
    const fn nuova(name: &'static str, group: PartGroup, description: &'static str) -> Self {
        Self {
            name,
            group,
            description,
            before: false,
            after: false,
            presenza: false,
            condivisa: false,
            fuoco: Fuoco::Proprio,
            riciclata: false,
        }
    }

    /// I due pseudo-elementi liberi.
    const fn livelli(mut self) -> Self {
        self.before = true;
        self.after = true;
        self
    }

    /// Il solo `::after` libero: `::before` lo usa il componente.
    const fn solo_dopo(mut self) -> Self {
        self.after = true;
        self
    }

    const fn presenza(mut self) -> Self {
        self.presenza = true;
        self
    }

    const fn condivisa(mut self) -> Self {
        self.condivisa = true;
        self
    }

    const fn fuoco_dentro(mut self) -> Self {
        self.fuoco = Fuoco::Dentro;
        self
    }

    const fn riciclata(mut self) -> Self {
        self.riciclata = true;
        self
    }

    /// Ha almeno un pseudo-elemento libero.
    #[must_use]
    pub const fn ha_livelli(&self) -> bool {
        self.before || self.after
    }
}

use PartGroup::{Cards, Column, Controls, Lists, Nav, NowPlaying, Overlays, Page, Player, Shell};

/// Le parti ridisegnabili, nell'ordine in cui il compilatore le emette.
///
/// L'ordine conta: a parità di specificità vince chi viene dopo, e il registro
/// va dalla cornice alle sovrapposizioni, cioè dal generale al particolare. Le
/// parti del formato 1 tengono il loro ordine relativo — una skin migrata vince
/// e perde gli stessi conflitti — e ogni parte nata da una divisione sta subito
/// dopo quella da cui viene.
#[rustfmt::skip]
pub static PARTS: &[PartDef] = &[
    // ── Cornice ─────────────────────────────────────────────────────────────
    PartDef::nuova("app-shell", Shell, "Il contenitore di tutta la finestra."),
    PartDef::nuova("ambient-backdrop", Shell,
        "Il fondo dietro al contenuto. È qui che vivono griglie, pavimenti prospettici e campi di punti.").livelli(),
    PartDef::nuova("window-controls", Shell, "I tre tasti della finestra, in alto a destra.").livelli(),
    PartDef::nuova("window-button", Shell,
        "Uno dei tre tasti della finestra: riduci, ingrandisci, chiudi.").solo_dopo(),

    // ── Il lettore flottante, che il formato 1 metteva qui ─────────────────────
    PartDef::nuova("player-shell", Player, "La barra del lettore, quando la terza colonna è chiusa.").livelli(),

    // ── Navigazione ─────────────────────────────────────────────────────────
    PartDef::nuova("bottom-nav", Nav, "La navigazione inferiore, su finestre strette.").livelli(),
    PartDef::nuova("sidebar", Nav, "La barra laterale: destinazioni e playlist.").livelli(),
    PartDef::nuova("brand", Nav, "Il marchio in cima alla barra laterale.").livelli(),
    PartDef::nuova("nav-pill", Nav, "La voce di navigazione, compresa quella della pagina aperta."),
    PartDef::nuova("nav-section-title", Nav, "Il titoletto di un gruppo di voci, come «Playlist»."),
    PartDef::nuova("nav-count", Nav, "Il numero accanto a una voce della navigazione."),

    // ── Pagina ──────────────────────────────────────────────────────────────
    PartDef::nuova("page-header", Page, "L'intestazione di una pagina."),
    PartDef::nuova("page-title", Page, "Il titolo grande di una pagina."),
    PartDef::nuova("page-subtitle", Page, "Il sottotitolo sotto il titolo di pagina."),
    PartDef::nuova("hero-eyebrow", Page,
        "L'etichetta piccola sopra un titolo hero, di norma in maiuscole."),
    PartDef::nuova("hero-art", Page, "L'immagine grande in cima alla pagina di un album o di una playlist.").condivisa(),
    PartDef::nuova("section-card", Page,
        "La scheda che contiene una sezione. La superficie più riusata dell'app.").livelli(),
    PartDef::nuova("section-heading", Page, "Il titolo di una sezione dentro una pagina."),
    PartDef::nuova("section-icon", Page, "L'icona accanto al titolo di sezione."),
    PartDef::nuova("stat-number", Page, "Un numero grande nelle statistiche."),
    PartDef::nuova("page-content", Page,
        "Il posto della pagina aperta. Il cambio di pagina si anima su questa parte.").presenza(),
    PartDef::nuova("search-field", Page, "Il campo di ricerca, lente e tasto compresi.").livelli().fuoco_dentro(),
    PartDef::nuova("back-button", Page, "Il tasto che torna all'elenco da una pagina di dettaglio."),
    PartDef::nuova("settings-nav", Page, "L'indice delle sezioni delle Impostazioni.").livelli(),
    PartDef::nuova("home-shelf", Page, "Uno scaffale della Home: un titolo e una fila di schede.").livelli(),
    PartDef::nuova("home-shelf-title", Page, "Il titolo di uno scaffale della Home."),
    PartDef::nuova("explore-bar", Page, "La barra di ricerca di Esplora, coi suoi comandi.").livelli(),

    // ── Controlli ───────────────────────────────────────────────────────────
    PartDef::nuova("icon-btn", Controls, "Il pulsante con la sola icona."),
    PartDef::nuova("play-btn-primary", Controls, "Il pulsante di riproduzione principale.").livelli(),
    PartDef::nuova("btn-accent", Controls, "Il pulsante d'azione primaria."),
    PartDef::nuova("btn-ghost", Controls, "Il pulsante secondario, senza fondo."),
    PartDef::nuova("switch", Controls, "L'interruttore, nel suo insieme.").solo_dopo(),
    PartDef::nuova("switch-track", Controls, "La pista dell'interruttore."),
    PartDef::nuova("field-input", Controls, "Il campo di testo."),
    PartDef::nuova("range-accent", Controls, "Il cursore a scorrimento."),
    PartDef::nuova("tooltip-pill", Controls, "Il suggerimento al passaggio."),
    PartDef::nuova("segmented", Controls,
        "Il gruppo di linguette che sceglie fra due o tre viste dello stesso contenuto."),
    PartDef::nuova("badge", Controls, "La pastiglia: una licenza, un'origine, un'etichetta breve."),
    PartDef::nuova("heart", Controls, "Il cuore dei preferiti, acceso e spento."),
    PartDef::nuova("rating", Controls, "Il blocco del giudizio: cuore, stelle e volume.").livelli().fuoco_dentro(),
    PartDef::nuova("stars", Controls, "Le cinque stelle, insieme."),
    PartDef::nuova("star", Controls, "Una stella, accesa e spenta."),
    PartDef::nuova("volume", Controls, "Il volume: il tasto muto e il suo cursore.").fuoco_dentro(),

    // ── Elenchi ─────────────────────────────────────────────────────────────
    PartDef::nuova("track-grid", Lists, "La griglia delle tracce o degli album."),
    PartDef::nuova("list-row", Lists,
        "La riga di un elenco di brani, compresa quella selezionata e quella che suona.").livelli().riciclata(),
    PartDef::nuova("list-header", Lists, "L'intestazione delle colonne di un elenco di brani.").livelli(),
    PartDef::nuova("track-index", Lists, "Il numero di una riga, che diventa un'onda quando suona."),
    PartDef::nuova("queue-list", Lists, "La coda di riproduzione."),
    PartDef::nuova("queue-row", Lists, "Una riga della coda.").solo_dopo().riciclata(),
    PartDef::nuova("folder-tree", Lists, "L'albero delle cartelle.").livelli(),
    PartDef::nuova("folder-row", Lists, "Una riga dell'albero delle cartelle.").solo_dopo().riciclata(),
    PartDef::nuova("explore-row", Lists, "Un risultato di Esplora.").solo_dopo(),
    PartDef::nuova("alpha-index", Lists, "L'indice alfabetico accanto agli artisti.").livelli(),
    PartDef::nuova("alpha-letter", Lists, "Una lettera dell'indice alfabetico."),
    PartDef::nuova("empty-state", Lists, "Il riquadro mostrato quando non c'è niente.").livelli(),
    PartDef::nuova("empty-icon", Lists, "L'icona dello stato vuoto."),
    PartDef::nuova("skeleton", Lists,
        "Il segnaposto durante il caricamento. La sua animazione è parte dell'identità della skin.").livelli(),

    // ── Schede ──────────────────────────────────────────────────────────────
    PartDef::nuova("card", Cards, "La scheda di un album o di una playlist.").livelli().riciclata(),
    PartDef::nuova("card-cover", Cards, "La copertina dentro una scheda.").condivisa(),
    PartDef::nuova("card-title", Cards, "Il titolo di una scheda."),
    PartDef::nuova("card-subtitle", Cards, "La riga sotto il titolo di una scheda."),
    PartDef::nuova("track-card", Cards, "La scheda di un brano, negli scaffali della Home.").livelli(),
    PartDef::nuova("collection-card", Cards, "La scheda di una raccolta della settimana.").livelli(),
    PartDef::nuova("home-resume", Cards, "Il riquadro «riprendi da qui» in cima alla Home.").livelli(),
    PartDef::nuova("artist-card", Cards, "La scheda di un artista.").livelli().riciclata(),
    PartDef::nuova("artist-mosaic", Cards, "Il mosaico di copertine di un artista.").livelli(),

    // ── Lettore ─────────────────────────────────────────────────────────────
    PartDef::nuova("player-progress", Player, "La barra di avanzamento.").livelli(),
    PartDef::nuova("progress-sheen", Player,
        "Il riflesso che scorre sulla barra di avanzamento."),
    PartDef::nuova("selection-bar", Player,
        "La barra che prende il posto del lettore quando ci sono più brani selezionati.").livelli().presenza(),
    PartDef::nuova("now-playing", Player, "Copertina piccola, titolo e artista di quel che suona.").livelli(),
    PartDef::nuova("np-thumb", Player, "La copertina in miniatura di quel che suona.").condivisa(),
    PartDef::nuova("queue-panel", Player, "Il pannello della coda, quando si apre sopra il contenuto.").livelli().presenza(),
    PartDef::nuova("eq-panel", Player, "Il pannello dell'equalizzatore.").livelli().presenza(),

    // ── Terza colonna ───────────────────────────────────────────────────────
    PartDef::nuova("column", Column, "La terza colonna: cosa suona, i comandi e la coda.").livelli().presenza(),
    PartDef::nuova("column-header", Column, "L'intestazione della colonna, coi suoi due tasti.").livelli(),
    PartDef::nuova("column-art", Column, "La copertina grande della colonna.").condivisa(),

    // ── In riproduzione ─────────────────────────────────────────────────────
    PartDef::nuova("np-screen", NowPlaying, "La schermata In riproduzione.").livelli(),
    PartDef::nuova("np-art", NowPlaying, "La copertina in grande.").condivisa(),
    PartDef::nuova("np-title", NowPlaying, "Il titolo del brano in riproduzione."),
    PartDef::nuova("np-meta", NowPlaying, "Artista e album sotto il titolo."),
    PartDef::nuova("np-formato", NowPlaying,
        "I dati tecnici del file sotto i comandi: formato, frequenza, canali, bitrate."),
    PartDef::nuova("np-transport", NowPlaying, "I comandi di riproduzione."),
    PartDef::nuova("np-scrim", NowPlaying, "Il velo sopra la copertina, per leggere il testo."),
    PartDef::nuova("lyrics-screen", NowPlaying, "La schermata del testo.").livelli(),
    PartDef::nuova("lyric-line", NowPlaying, "La riga di testo, accesa e non."),
    PartDef::nuova("lyric-word", NowPlaying, "La parola del testo, quando i tempi ci sono."),
    PartDef::nuova("lyric-breath", NowPlaying, "La pausa fra due righe del testo."),
    PartDef::nuova("viz-screen", NowPlaying, "La schermata del visualizer.").livelli(),
    PartDef::nuova("eq-bars", NowPlaying, "Le barre dell'equalizzatore."),
    PartDef::nuova("eq-slider", NowPlaying, "Il cursore di una banda dell'equalizzatore."),

    // ── Sovrapposizioni ─────────────────────────────────────────────────────
    PartDef::nuova("glass-modal", Overlays, "La finestra modale.").livelli().presenza(),
    PartDef::nuova("modal-veil", Overlays, "Il velo scuro dietro una finestra modale.").presenza(),
    PartDef::nuova("menu-pop", Overlays, "Il menu contestuale.").livelli().presenza(),
    PartDef::nuova("menu-item", Overlays, "Una voce del menu contestuale."),
    PartDef::nuova("toast-stack", Overlays, "La pila delle notifiche, in basso."),
    PartDef::nuova("toast-card", Overlays, "La notifica temporanea.").livelli().presenza(),
    PartDef::nuova("toast-progress", Overlays, "La barra di durata di una notifica."),
    PartDef::nuova("tour-tooltip", Overlays, "Il fumetto della presentazione guidata.").livelli().presenza(),
    PartDef::nuova("tour-veil", Overlays, "Il velo col buco della presentazione guidata.").presenza(),
    PartDef::nuova("first-run", Overlays, "La schermata del primo avvio.").livelli().presenza(),
    PartDef::nuova("first-run-card", Overlays, "La scheda al centro del primo avvio.").livelli(),
];

/// Le parti ritirate: si accettano e non fanno niente.
///
/// Una parte è un nome pubblico. Sta nelle skin che qualcuno ha già scritto e
/// spedito, e il lettore di documenti tratta un nome sconosciuto come un
/// **errore duro**, non come un avviso. Il patto è: il nome continua a essere
/// accettato dal lettore, non compare nell'editor, non genera CSS e non
/// suggerisce nulla ai refusi.
///
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
/// Una parte ritirata non esiste: questa funzione dice `None`. Chi deve
/// *accettarla* è il solo lettore di documenti, che per questo chiede a
/// [`ritirata`] prima di dichiarare un errore.
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

/// Il posto della parte nel registro, che è l'ordine di emissione.
#[must_use]
pub fn posto(name: &str) -> usize {
    PARTS
        .iter()
        .position(|def| def.name == name)
        .unwrap_or(usize::MAX)
}

// ── Cosa una skin può cambiare, per parte ───────────────────────────────────

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

/// Lo stile del bordo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderStyle {
    /// Pieno.
    Solid,
    /// A trattini.
    Dashed,
    /// A punti.
    Dotted,
    /// Doppio.
    Double,
    /// Nessun bordo, anche se il componente ne ha uno.
    None,
}

impl BorderStyle {
    /// Come si scrive in CSS. È anche il nome nel documento.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Solid => "solid",
            Self::Dashed => "dashed",
            Self::Dotted => "dotted",
            Self::Double => "double",
            Self::None => "none",
        }
    }

    /// Tutti.
    pub const ALL: &'static [Self] = &[
        Self::Solid,
        Self::Dashed,
        Self::Dotted,
        Self::Double,
        Self::None,
    ];
}

/// Il raggio degli angoli: uno per tutti, o uno per angolo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Raggio {
    /// Lo stesso ai quattro angoli.
    Uguale(Length),
    /// Angolo per angolo; quel che manca resta del componente.
    Angoli {
        /// In alto a sinistra.
        top_left: Option<Length>,
        /// In alto a destra.
        top_right: Option<Length>,
        /// In basso a destra.
        bottom_right: Option<Length>,
        /// In basso a sinistra.
        bottom_left: Option<Length>,
    },
}

/// L'ombra di una parte: scritta per esteso, o presa da un token d'elevazione.
#[derive(Debug, Clone, PartialEq)]
pub enum Ombra {
    /// A livelli, come un token `shadow.*`.
    Valore(ShadowValue),
    /// Un token d'ombra del registro.
    Token(&'static TokenDef),
}

/// L'anello che mostra il fuoco da tastiera.
///
/// È un `outline`, non un bordo: non occupa spazio e non sposta niente, ed è
/// quel che un lettore di schermo si aspetta di vedere attorno all'elemento
/// col fuoco.
#[derive(Debug, Clone, PartialEq)]
pub struct AnelloFuoco {
    /// Il colore.
    pub color: ColorValue,
    /// Lo spessore.
    pub width: Length,
    /// La distanza dal bordo; negativa lo porta dentro.
    pub offset: Option<Length>,
}

/// L'aspetto di una parte, o di uno dei suoi stati.
///
/// È la stessa struttura per la base e per i dieci stati: base e stati non
/// possono divergere, e un campo che valesse solo alla base non esiste.
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
    /// Stile del bordo.
    pub border_style: Option<BorderStyle>,
    /// Raggio degli angoli.
    pub radius: Option<Raggio>,
    /// Ombra.
    pub shadow: Option<Ombra>,
    /// L'anello del fuoco da tastiera.
    pub focus_ring: Option<AnelloFuoco>,
    /// Un ritaglio, di norma un `chamfer`.
    pub clip: Option<Paint>,
    /// I filtri sull'elemento stesso: sfocatura, luminosità, saturazione…
    pub filter: Vec<FiltroElemento>,
    /// Un filtro su quel che sta **dietro**, di norma un `blurBehind`.
    ///
    /// Nel formato 1 si chiamava `filter`, e il nome mentiva: era
    /// `backdrop-filter`. Il formato 2 dà `filter` al filtro vero e questo lo
    /// chiama per quel che fa; la migrazione sposta la chiave.
    pub backdrop: Option<Paint>,
    /// Opacità.
    pub opacity: Option<f64>,
    /// La famiglia di carattere, fra le tre dei token `font.*`.
    pub font: Option<FontKind>,
    /// La misura del testo, in px o rem entro [`FONT_SIZE_MIN`] e [`FONT_SIZE_MAX`].
    pub font_size: Option<Length>,
    /// L'interlinea, come moltiplicatore.
    pub line_height: Option<f64>,
    /// Spaziatura fra le lettere.
    pub letter_spacing: Option<Length>,
    /// Trasformazione del testo.
    pub text_transform: Option<TextTransform>,
    /// Peso del carattere.
    pub font_weight: Option<f64>,
}

/// La misura minima del testo, in pixel.
///
/// Sotto i nove pixel un'etichetta non è più un'etichetta: è una riga grigia.
pub const FONT_SIZE_MIN: f64 = 9.0;
/// La misura massima del testo, in pixel.
///
/// Oltre i settantadue un titolo esce dalla sua zona su una finestra normale, e
/// le zone non hanno la geometria per farlo stare: è il confine fra una
/// tipografia e un'impaginazione.
pub const FONT_SIZE_MAX: f64 = 72.0;

impl PartAppearance {
    /// Non dichiara niente: il compilatore non emette la regola.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Tutti i colori che dichiara, fuori dagli effetti.
    #[must_use]
    pub fn colori_propri(&self) -> Vec<&ColorValue> {
        let mut colori: Vec<&ColorValue> = [self.text_color.as_ref(), self.border_color.as_ref()]
            .into_iter()
            .flatten()
            .collect();
        if let Some(anello) = self.focus_ring.as_ref() {
            colori.push(&anello.color);
        }
        if let Some(Ombra::Valore(ombra)) = self.shadow.as_ref() {
            colori.extend(ombra.layers.iter().map(|l| &l.color));
        }
        for filtro in &self.filter {
            if let FiltroElemento::DropShadow { color, .. } = filtro {
                colori.push(color);
            }
        }
        colori
    }
}

/// Un livello aggiuntivo su `::before` o `::after`.
#[derive(Debug, Clone, PartialEq)]
pub struct PartLayer {
    /// Gli sfondi del livello.
    pub background: Vec<Paint>,
    /// Opacità del livello.
    pub opacity: Option<f64>,
}

/// Gli stati dichiarati da una skin per una parte, in ordine di [`PartState::ALL`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PartStates {
    voci: Vec<(PartState, PartAppearance)>,
}

impl PartStates {
    /// Costruisce l'elenco mettendolo in ordine di emissione.
    #[must_use]
    pub fn new(mut voci: Vec<(PartState, PartAppearance)>) -> Self {
        voci.sort_by_key(|(stato, _)| stato.posto());
        Self { voci }
    }

    /// L'aspetto di uno stato, se dichiarato.
    #[must_use]
    pub fn get(&self, state: PartState) -> Option<&PartAppearance> {
        self.voci.iter().find(|(s, _)| *s == state).map(|(_, a)| a)
    }

    /// Gli stati dichiarati, in ordine di emissione.
    pub fn iter(&self) -> impl Iterator<Item = (PartState, &PartAppearance)> {
        self.voci.iter().map(|(s, a)| (*s, a))
    }

    /// Nessuno stato dichiarato.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.voci.is_empty()
    }
}

/// Una parte ridisegnata.
#[derive(Debug, Clone, PartialEq)]
pub struct PartStyle {
    /// Quale parte.
    pub def: &'static PartDef,
    /// L'aspetto di base.
    pub appearance: PartAppearance,
    /// Il livello su `::before`, ammesso solo dove `before` è vero.
    pub before: Option<PartLayer>,
    /// Il livello su `::after`, ammesso solo dove `after` è vero.
    pub after: Option<PartLayer>,
    /// Gli stati.
    pub states: PartStates,
    /// Il movimento: le animazioni per innesco, la cascata, la transizione.
    ///
    /// Sta qui e non in [`PartAppearance`] perché un'animazione è un **legame**
    /// e non un valore d'aspetto: `PartAppearance` è la stessa per la base e per
    /// gli stati, e un campo che lì valesse solo alla base comparirebbe negli
    /// stati, si potrebbe scrivere, e non farebbe niente.
    pub motion: PartMotion,
}

impl PartStyle {
    /// Una parte senza niente di dichiarato, pronta da riempire.
    #[must_use]
    pub fn vuota(def: &'static PartDef) -> Self {
        Self {
            def,
            appearance: PartAppearance::default(),
            before: None,
            after: None,
            states: PartStates::default(),
            motion: PartMotion::default(),
        }
    }

    /// Tutti gli effetti che questa parte disegna, per i colori e il costo.
    #[must_use]
    pub fn effects(&self) -> Vec<Effect> {
        let mut tutti = Vec::new();
        for aspetto in self.appearances() {
            tutti.extend(paint_effects(&aspetto.background));
            if let Some(clip) = aspetto.clip.as_ref() {
                tutti.push(clip.effect().clone());
            }
            // Il filtro dietro **deve** entrare nel conto, ed è la ragione per
            // cui il budget esiste: un `blurBehind` costa dieci da solo.
            if let Some(dietro) = aspetto.backdrop.as_ref() {
                tutti.push(dietro.effect().clone());
            }
        }
        for livello in [self.before.as_ref(), self.after.as_ref()]
            .into_iter()
            .flatten()
        {
            tutti.extend(paint_effects(&livello.background));
        }
        tutti
    }

    /// Quanto costa disegnare questa superficie: effetti più filtri.
    ///
    /// Gli stati entrano nel conto come la base, e non è prudenza: uno stato
    /// è una superficie che si disegna davvero, e il `blurBehind` messo solo
    /// al passaggio del mouse costa quanto quello messo sempre, nel momento in
    /// cui si vede.
    #[must_use]
    pub fn costo_superficie(&self) -> u32 {
        let filtri: u32 = self
            .appearances()
            .iter()
            .flat_map(|a| a.filter.iter())
            .map(|f| f.costo().weight())
            .sum();
        crate::effects::stack_cost(&self.effects()) + filtri
    }

    /// Quanto costa il movimento di questa parte.
    ///
    /// Separato dal costo della superficie e mai sommato con quello: sono due
    /// budget che misurano due cose diverse.
    #[must_use]
    pub fn motion_cost(&self) -> u32 {
        self.motion.costo()
    }

    /// Le animazioni che questa parte richiama.
    #[must_use]
    pub fn animations_used(&self) -> Vec<&str> {
        self.motion.nomi_usati()
    }

    /// I motivi che questa parte richiama, base, stati e livelli.
    #[must_use]
    pub fn patterns_used(&self) -> Vec<&str> {
        let mut nomi: Vec<&str> = Vec::new();
        for aspetto in self.appearances() {
            nomi.extend(aspetto.background.iter().filter_map(Paint::pattern_name));
            nomi.extend(aspetto.clip.as_ref().and_then(Paint::pattern_name));
            nomi.extend(aspetto.backdrop.as_ref().and_then(Paint::pattern_name));
        }
        for livello in [self.before.as_ref(), self.after.as_ref()]
            .into_iter()
            .flatten()
        {
            nomi.extend(livello.background.iter().filter_map(Paint::pattern_name));
        }
        nomi
    }

    /// L'aspetto di base e quelli degli stati dichiarati.
    #[must_use]
    pub fn appearances(&self) -> Vec<&PartAppearance> {
        let mut tutti = vec![&self.appearance];
        tutti.extend(self.states.iter().map(|(_, a)| a));
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
    fn il_registro_ha_le_parti_del_formato_1_e_le_nuove() {
        // Cento, non cinquantuno. Se questo numero scende, una skin del formato
        // 1 o 2 ha perso una superficie senza che nessuno l'abbia deciso.
        assert!(PARTS.len() >= 100, "le parti sono {}", PARTS.len());
        for nome in [
            "app-shell",
            "player-shell",
            "list-row",
            "glass-modal",
            "menu-pop",
            "hero-art",
            "tour-tooltip",
        ] {
            assert!(part(nome).is_some(), "manca «{nome}» del formato 1");
        }
    }

    #[test]
    fn ogni_parte_che_convive_con_list_row_viene_dopo() {
        // Queste righe portano **sia** `list-row` sia la loro parte: a parità
        // di specificità vince chi viene dopo, e una riga della coda deve
        // poter dire l'ultima parola su quel che la riga generica ha detto.
        for (vecchia, nuova) in [
            ("list-row", "queue-row"),
            ("list-row", "folder-row"),
            ("list-row", "explore-row"),
            ("list-row", "card"),
        ] {
            assert!(
                posto(vecchia) <= posto(nuova),
                "«{nuova}» viene prima di «{vecchia}»"
            );
        }
    }

    #[test]
    fn una_parte_ritirata_non_e_anche_viva() {
        for (nome, perche) in RITIRATE {
            assert!(
                part(nome).is_none(),
                "«{nome}» è ritirata e insieme viva nel registro"
            );
            assert!(!perche.is_empty(), "«{nome}» è ritirata senza un perché");
            assert!(!nearest_parts(nome).contains(nome));
        }
        assert!(ritirata("viz-title").is_some_and(|perche| perche.contains("DettaglioSpettro")));
        assert_eq!(ritirata("viz-screen"), None);
    }

    #[test]
    fn un_nome_sbagliato_per_assonanza_riceve_un_suggerimento() {
        assert!(nearest_parts("section-cards").contains(&"section-card"));
        assert!(nearest_parts("np-titles").contains(&"np-title"));
        assert!(nearest_parts("player-bar").contains(&"player-shell"));
        assert!(nearest_parts("zzzzzz").is_empty());
    }

    #[test]
    fn i_gruppi_coprono_tutto_il_registro() {
        let contate: usize = PartGroup::ALL
            .iter()
            .map(|g| parts_in_group(*g).len())
            .sum();
        assert_eq!(contate, PARTS.len());
    }

    #[test]
    fn le_copertine_non_hanno_livelli_perche_sono_immagini() {
        // Un `<img>` non ha pseudo-elementi: un livello dichiarato lì non
        // comparirebbe mai, e il formato 1 lo accettava in silenzio.
        for nome in ["hero-art", "np-art", "column-art", "card-cover", "np-thumb"] {
            let def = part(nome).expect("nel registro");
            assert!(!def.ha_livelli(), "«{nome}» dichiara dei livelli");
            assert!(def.condivisa, "«{nome}» non è condivisa");
        }
    }

    #[test]
    fn le_presenze_sono_le_parti_che_entrano_ed_escono_intere() {
        for nome in [
            "menu-pop",
            "glass-modal",
            "toast-card",
            "column",
            "queue-panel",
            "eq-panel",
            "page-content",
        ] {
            assert!(part(nome).is_some_and(|d| d.presenza), "«{nome}»");
        }
        assert!(part("list-row").is_some_and(|d| !d.presenza));
    }
}
