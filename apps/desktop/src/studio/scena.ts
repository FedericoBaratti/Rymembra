/**
 * Che cosa c'è sulla tela: una pagina, e quel che le sta sopra.
 *
 * La tela disegna **le pagine vere** con dati finti (`mondo.ts`), e una scena
 * è la scelta di quale: la griglia degli album, la pagina di un disco, lo
 * schermo intero, le Cartelle, Esplora, e sopra un menù, una finestrella, una
 * notifica, la coda, l'equalizzatore, la barra della selezione, il giro
 * guidato, il primo avvio.
 *
 * `DOVE` dice, per ogni parte che non sta sempre sulla finestra, in che scena
 * compare. È quel che fa funzionare «Mostrami» nell'ispettore: chi seleziona
 * `explore-row` dall'elenco e non la vede sulla tela ci viene portato, invece
 * di chiedersi se la skin la stia ridipingendo o no. Una parte che manca da
 * qui e non si vede sulla scena di serie è un difetto, e il test dello Studio
 * (`test/scena.test.ts`) lo dice.
 */

/** Le pagine che la tela sa disegnare. */
export type Pagina =
  | "home"
  | "album"
  | "disco"
  | "brani"
  | "artisti"
  | "cartelle"
  | "esplora"
  | "impostazioni"
  | "schermo"
  | "testo"
  | "caricamento"
  | "vuoto";

/** Quel che può stare sopra una pagina. */
export type Sovrapposizione =
  | "menu"
  | "finestrella"
  | "notifica"
  | "coda"
  | "eq"
  | "selezione"
  | "giro"
  | "primo";

/** Le tre larghezze della finestra finta. */
export type Larghezza = "tre" | "due" | "stretta";

/** In pixel CSS: la finestra a tre colonne, a due, e il telefono di traverso. */
export const LARGHEZZE: Readonly<Record<Larghezza, number>> = {
  tre: 1280,
  due: 1000,
  stretta: 720,
};

/** L'altezza della finestra finta: è quella dell'app appena aperta. */
export const ALTEZZA = 820;

export interface Scena {
  pagina: Pagina;
  sovrapposizioni: readonly Sovrapposizione[];
}

export const SCENA_DI_SERIE: Scena = { pagina: "album", sovrapposizioni: [] };

/** Tutte, nell'ordine in cui si scelgono. */
export const PAGINE: readonly Pagina[] = [
  "home",
  "album",
  "disco",
  "brani",
  "artisti",
  "cartelle",
  "esplora",
  "impostazioni",
  "schermo",
  "testo",
  "caricamento",
  "vuoto",
];

export const SOVRAPPOSIZIONI: readonly Sovrapposizione[] = [
  "menu",
  "finestrella",
  "notifica",
  "coda",
  "eq",
  "selezione",
  "giro",
  "primo",
];

/** Dove si vede una parte: la pagina, le sovrapposizioni, la larghezza. */
export interface Dove {
  pagina?: Pagina;
  sovrapposizioni?: readonly Sovrapposizione[];
  larghezza?: Larghezza;
}

/**
 * Dove si vede ogni parte che non sta sempre sulla finestra.
 *
 * Quelle che mancano stanno sulla cornice — la barra laterale, la colonna, la
 * testata — e si vedono su qualunque pagina a tre colonne.
 */
export const DOVE: Readonly<Record<string, Dove>> = {
  // La libreria.
  "track-grid": { pagina: "album" },
  card: { pagina: "album" },
  "card-cover": { pagina: "album" },
  "card-title": { pagina: "album" },
  "card-subtitle": { pagina: "album" },
  "list-row": { pagina: "brani" },
  "list-header": { pagina: "brani" },
  "track-index": { pagina: "brani" },
  badge: { pagina: "brani" },
  stars: { pagina: "brani" },
  star: { pagina: "brani" },
  heart: { pagina: "brani" },
  skeleton: { pagina: "caricamento" },
  "empty-state": { pagina: "vuoto" },
  "empty-icon": { pagina: "vuoto" },
  // La Home.
  "home-shelf": { pagina: "home" },
  "home-shelf-title": { pagina: "home" },
  "track-card": { pagina: "home" },
  "collection-card": { pagina: "home" },
  "home-resume": { pagina: "home" },
  // Un disco aperto.
  "hero-art": { pagina: "disco" },
  "hero-eyebrow": { pagina: "disco" },
  "back-button": { pagina: "disco" },
  "btn-accent": { pagina: "disco" },
  // Gli artisti.
  "artist-card": { pagina: "artisti" },
  "artist-mosaic": { pagina: "artisti" },
  "alpha-index": { pagina: "artisti" },
  "alpha-letter": { pagina: "artisti" },
  // Le cartelle ed Esplora.
  "folder-tree": { pagina: "cartelle" },
  "folder-row": { pagina: "cartelle" },
  "explore-bar": { pagina: "esplora" },
  "explore-row": { pagina: "esplora" },
  "field-input": { pagina: "esplora" },
  // Le impostazioni.
  "settings-nav": { pagina: "impostazioni" },
  "section-card": { pagina: "impostazioni" },
  "section-heading": { pagina: "impostazioni" },
  "section-icon": { pagina: "impostazioni" },
  switch: { pagina: "impostazioni" },
  "switch-track": { pagina: "impostazioni" },
  segmented: { pagina: "impostazioni" },
  "tooltip-pill": { pagina: "impostazioni" },
  "stat-number": { pagina: "impostazioni" },
  // Lo schermo intero e il testo.
  "np-screen": { pagina: "schermo" },
  "np-art": { pagina: "schermo" },
  "np-scrim": { pagina: "schermo" },
  "np-transport": { pagina: "schermo" },
  "np-formato": { pagina: "schermo" },
  "viz-screen": { pagina: "schermo" },
  "lyrics-screen": { pagina: "testo" },
  "lyric-line": { pagina: "testo" },
  "lyric-word": { pagina: "testo" },
  "lyric-breath": { pagina: "testo" },
  // Le sovrapposizioni.
  "menu-pop": { sovrapposizioni: ["menu"] },
  "menu-item": { sovrapposizioni: ["menu"] },
  "glass-modal": { sovrapposizioni: ["finestrella"] },
  "modal-veil": { sovrapposizioni: ["finestrella"] },
  "toast-card": { sovrapposizioni: ["notifica"] },
  "toast-progress": { sovrapposizioni: ["notifica"] },
  "queue-panel": { sovrapposizioni: ["coda"], larghezza: "due" },
  "eq-panel": { sovrapposizioni: ["eq"], larghezza: "due" },
  "eq-bars": { sovrapposizioni: ["eq"], larghezza: "due" },
  "eq-slider": { sovrapposizioni: ["eq"], larghezza: "due" },
  "selection-bar": { pagina: "brani", sovrapposizioni: ["selezione"] },
  "tour-tooltip": { sovrapposizioni: ["giro"] },
  "tour-veil": { sovrapposizioni: ["giro"] },
  "first-run": { sovrapposizioni: ["primo"] },
  "first-run-card": { sovrapposizioni: ["primo"] },
  // La cornice, quando la colonna è chiusa o la finestra è stretta.
  "player-shell": { larghezza: "due" },
  "player-progress": { larghezza: "due" },
  "progress-sheen": { larghezza: "due" },
  "bottom-nav": { larghezza: "stretta" },
};

/** La scena che mostra una parte, partendo da quella che c'è. */
export function scenaPer(parte: string, adesso: Scena): Scena {
  const dove = DOVE[parte];
  if (dove === undefined) return adesso;
  return {
    pagina: dove.pagina ?? adesso.pagina,
    sovrapposizioni: dove.sovrapposizioni ?? [],
  };
}
