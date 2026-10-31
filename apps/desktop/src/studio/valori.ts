/**
 * I valori del documento, letti per essere mostrati.
 *
 * # Perché serve un modulo apposta
 *
 * Il formato accetta tre forme per un colore — un letterale, un riferimento a un
 * token, un riferimento alla tavolozza con un'opacità — e lo Studio le deve
 * saper fare tre cose: leggerle, scriverle e **dipingerle**. Dipingerle è la
 * parte che non è ovvia: la pastiglia accanto a un livello deve mostrare il
 * colore vero, e il colore vero di `{ "$token": "color.accent" }` è quel che la
 * variabile CSS contiene in quel momento, non un valore che si può calcolare
 * qui.
 *
 * La risposta è non calcolarlo: si restituisce `var(--accent)` e si lascia
 * decidere al browser, che è l'unico che sa quale skin è applicata all'anteprima.
 *
 * # Niente di quel che sta qui valida
 *
 * A dire se un valore è buono è `parse_skin_json`, e lo dice sull'intero
 * documento centoventi millisecondi dopo. Queste funzioni servono a disegnare, e
 * su un valore che non capiscono restituiscono `null` invece di indovinare.
 */
import type { EffettoRegistro, TokenRegistro } from "../ipc";

/** Un livello di sfondo, come sta nel documento. */
export type Livello = Record<string, unknown> & { effect?: unknown };

/**
 * I motivi dichiarati dal documento: nome → l'effetto per esteso.
 *
 * Serve a leggere un livello scritto come `{ "$pattern": "lampada" }`. Senza,
 * un riferimento a un motivo non ha né un nome né un costo — e si vedeva: la
 * riga mostrava `?` e il peso `0`, cioè diceva che un motivo è gratis proprio
 * mentre lo si stava impilando su una superficie col budget contato.
 */
export type Motivi = Readonly<Record<string, unknown>>;

/** Un livello scritto per esteso: se è un riferimento, il motivo che indica. */
export function sciolto(livello: unknown, motivi: Motivi = {}): unknown {
  if (livello === null || typeof livello !== "object") return livello;
  const riferimento = (livello as Record<string, unknown>)["$pattern"];
  if (typeof riferimento !== "string") return livello;
  return motivi[riferimento] ?? null;
}

/** Il nome dell'effetto di un livello, se ce l'ha. Segue i riferimenti. */
export function nomeEffetto(
  livello: unknown,
  motivi: Motivi = {},
): string | null {
  const dentro = sciolto(livello, motivi);
  if (dentro === null || typeof dentro !== "object") return null;
  const quale = (dentro as Record<string, unknown>)["effect"];
  return typeof quale === "string" ? quale : null;
}

/**
 * Un colore del documento, in una forma che il CSS sa dipingere.
 *
 * `null` quando la forma non si riconosce — un documento a metà, o un
 * riferimento a un token che non esiste. Chi chiama disegna un vuoto, che è
 * meglio di un nero finto: un nero finto si scambia per il colore scelto.
 */
export function tinta(
  valore: unknown,
  tokens: readonly TokenRegistro[],
  tavolozza: Readonly<Record<string, string>>,
): string | null {
  if (typeof valore === "string") return valore;
  if (valore === null || typeof valore !== "object") return null;
  const dentro = valore as Record<string, unknown>;

  const token = dentro["$token"];
  if (typeof token === "string") {
    const def = tokens.find((t) => t.id === token);
    // La variabile, non il valore: qual è il valore lo sa il browser, che è
    // l'unico a sapere quale skin sta dipingendo l'anteprima in questo istante.
    return def ? `var(${def.css})` : null;
  }

  const dalla = dentro["$palette"];
  if (typeof dalla === "string") {
    const base = tavolozza[dalla];
    if (base === undefined) return null;
    const alpha = dentro["alpha"];
    if (typeof alpha !== "number") return base;
    // `color-mix` invece di ricomporre l'esadecimale a mano: è così che le
    // varianti *soft* e *glow* restano lo **stesso** colore invece di essere un
    // secondo valore da tenere allineato.
    return `color-mix(in srgb, ${base} ${Math.round(alpha * 100)}%, transparent)`;
  }
  return null;
}

/** Il primo colore di un livello, per la pastiglia. */
function coloreDelLivello(
  livello: Livello,
  tokens: readonly TokenRegistro[],
  tavolozza: Readonly<Record<string, string>>,
): string {
  const diretto = tinta(livello["color"], tokens, tavolozza);
  if (diretto !== null) return diretto;
  const fermate = livello["stops"];
  if (Array.isArray(fermate)) {
    const prima = fermate[0];
    if (prima !== null && typeof prima === "object") {
      const colore = tinta((prima as Record<string, unknown>)["color"], tokens, tavolozza);
      if (colore !== null) return colore;
    }
  }
  return "var(--color-surface-3)";
}

/**
 * Come si dipinge la pastiglia di un livello.
 *
 * Non è l'effetto compilato — quello lo fa il crate, ed è nell'anteprima al
 * centro. È il suo ritratto in quattordici pixel: quel tanto che basta a
 * distinguere una tinta piena da una griglia di punti senza leggere il nome.
 */
export function ritrattoEffetto(
  livello: Livello,
  tokens: readonly TokenRegistro[],
  tavolozza: Readonly<Record<string, string>>,
  motivi: Motivi = {},
): string {
  const dentro = sciolto(livello, motivi);
  if (dentro === null || typeof dentro !== "object")
    return "var(--color-surface-3)";
  const disteso = dentro as Livello;
  const colore = coloreDelLivello(disteso, tokens, tavolozza);
  switch (nomeEffetto(disteso)) {
    case "dotGrid":
      return `radial-gradient(circle at 1.5px 1.5px, ${colore} 0.9px, transparent 1.1px) 0 0 / 4px 4px`;
    case "hairlineGrid":
      return `repeating-linear-gradient(0deg, ${colore} 0 1px, transparent 1px 5px), repeating-linear-gradient(90deg, ${colore} 0 1px, transparent 1px 5px)`;
    case "scanlines":
      return `repeating-linear-gradient(0deg, ${colore} 0 1px, transparent 1px 3px)`;
    case "stripes":
      return `repeating-linear-gradient(45deg, ${colore} 0 3px, transparent 3px 6px)`;
    case "linearGradient":
      return `linear-gradient(145deg, ${colore}, var(--color-surface-3))`;
    case "radialGradient":
    case "vignette":
      return `radial-gradient(circle, ${colore}, var(--color-surface-3))`;
    case "conicGradient":
      return `conic-gradient(${colore}, var(--color-surface-3), ${colore})`;
    case "blurBehind":
      // Una sfocatura non ha un colore suo: mostra quel che sta sotto, e il
      // ritratto onesto è una superficie che si vede attraverso.
      return "linear-gradient(145deg, var(--color-surface-3), transparent)";
    default:
      return colore;
  }
}

/**
 * Il costo di un livello, dal registro. Zero se l'effetto non si riconosce.
 *
 * Un riferimento a un motivo costa quanto il motivo che indica: il compilatore
 * lo conta così, e contarlo zero qui vorrebbe dire mostrare una superficie
 * dentro il budget mentre non lo è.
 */
export function costoDi(
  livello: unknown,
  effetti: readonly EffettoRegistro[],
  motivi: Motivi = {},
): number {
  const quale = nomeEffetto(livello, motivi);
  return effetti.find((e) => e.name === quale)?.cost ?? 0;
}

/**
 * Il colore di un costo, per classe.
 *
 * Le tre classi del crate — `Cheap`, `Paint`/`Composited`, `Gpu` — si leggono
 * dal peso invece che chiederle: il peso è già nel registro, e una quarta classe
 * qui sarebbe una tabella da tenere allineata a mano con `effects.rs`.
 */
export function coloreCosto(costo: number, budget: number): string {
  if (costo >= budget) return "var(--danger)";
  if (costo > 1) return "var(--warning)";
  return "var(--success)";
}

/**
 * Il colore che un valore dipinge **nella bozza**, per un campione.
 *
 * `tinta` risolve un `$token` con la variabile CSS, e nello Studio quella
 * variabile la definisce la skin **indossata**, non la bozza: il campione di
 * `color.accent` mostrava l'accento di Plain accanto al valore appena scelto.
 * Qui i riferimenti si seguono nel documento — prima la variante, se è quella
 * che si guarda, poi la base — fino a un letterale. Un token che la bozza non
 * dichiara ripiega sulla variabile, che è il meglio che si sappia dire.
 */
export function tintaNel(
  valore: unknown,
  documento: Readonly<Record<string, unknown>> | null,
  variante: boolean,
  tokens: readonly TokenRegistro[],
  profondita = 0,
): string | null {
  if (typeof valore === "string") return valore;
  if (valore === null || typeof valore !== "object" || profondita > 8) return null;
  const dentro = valore as Record<string, unknown>;
  const radice = (documento ?? {}) as Record<string, unknown>;
  const altra = ((radice["variant"] ?? {}) as Record<string, unknown>);
  const cerca = (sezione: string, nome: string): unknown => {
    const inVariante = variante
      ? ((altra[sezione] ?? {}) as Record<string, unknown>)[nome]
      : undefined;
    return inVariante ?? ((radice[sezione] ?? {}) as Record<string, unknown>)[nome];
  };
  const conAlfa = (colore: string | null) => {
    const alfa = dentro["alpha"];
    if (colore === null || typeof alfa !== "number") return colore;
    return `color-mix(in srgb, ${colore} ${Math.round(alfa * 100)}%, transparent)`;
  };

  const token = dentro["$token"];
  if (typeof token === "string") {
    const dichiarato = cerca("tokens", token);
    if (dichiarato !== undefined) {
      return tintaNel(dichiarato, documento, variante, tokens, profondita + 1);
    }
    const def = tokens.find((t) => t.id === token);
    return def ? `var(${def.css})` : null;
  }
  const dalla = dentro["$palette"];
  if (typeof dalla === "string") {
    const scritto = cerca("palette", dalla);
    return conAlfa(typeof scritto === "string" ? scritto : null);
  }
  // Una sorgente dinamica non ha un colore finché non c'è una copertina: il
  // campione lo dice con l'arcobaleno, invece di inventarne uno.
  if (typeof dentro["$source"] === "string") {
    return "conic-gradient(from 90deg, #e8664f, #e9c46a, #58b37f, #4e8fd8, #a36bd6, #e8664f)";
  }
  return null;
}

/**
 * Gli angoli che uno smusso (`chamfer`) taglia quando `corners` non c'è.
 *
 * Gli stessi del nucleo (`document.rs`, ramo `"chamfer"`: `None =>
 * vec![Corner::TopRight, Corner::BottomLeft]`) — il registro non li espone, e
 * il registro lo dice a parole: «Senza, in alto a destra e in basso a
 * sinistra». Fino alla 3.2 qui si leggeva «nessun angolo dichiarato vuol dire
 * tutti», che era falso: i tasti si vedevano tutti spenti mentre la tela ne
 * tagliava due, e il primo tocco li perdeva.
 */
export const ANGOLI_DI_SERIE: readonly string[] = ["topRight", "bottomLeft"];

/** Gli angoli che uno smusso taglia davvero, dichiarati o di serie. */
export function angoliTagliati(valore: unknown): string[] {
  return Array.isArray(valore) ? valore.map(String) : [...ANGOLI_DI_SERIE];
}

/**
 * Gli angoli dopo aver premuto `angolo`, partendo da quelli tagliati davvero.
 *
 * `null` se il gesto non si può fare: spegnere l'ultimo angolo acceso. Il
 * formato vuole da uno a quattro angoli (`lista_fra(1, 4)`), e togliere il
 * campo — quel che si faceva prima — riaccendeva i due di serie: il tasto
 * dell'ultimo angolo si spegne invece.
 */
export function angoliDopo(valore: unknown, angolo: string): string[] | null {
  const tagliati = angoliTagliati(valore);
  const nuovi = tagliati.includes(angolo) ? tagliati.filter((a) => a !== angolo) : [...tagliati, angolo];
  return nuovi.length === 0 ? null : nuovi;
}

/** Le unità che il formato accetta per una lunghezza. Vengono da `LengthUnit`. */
export const UNITA = ["px", "rem", "em", "%", "vh", "vw", "cqw", "cqh", "ch"] as const;

/**
 * L'unità di una lunghezza già scritta, o `null` se è un numero puro.
 *
 * Si legge dalla coda e non con una regex sull'intero: `-0.5rem` comincia con un
 * segno e contiene un punto, e ogni tentativo di riconoscere «la parte numerica»
 * in avanti finisce per rifare `parse_length` in TypeScript. Il suffisso invece
 * è chiuso — sono le nove unità di `LengthUnit` — quindi si cerca quello.
 */
export function unitaDi(valore: unknown): string | null {
  if (typeof valore !== "string") return null;
  const testo = valore.trim();
  // Dalla più lunga: `cqw` prima di `w`… e soprattutto `rem` prima di `em`, che
  // ne è un suffisso e ruberebbe ogni `rem` se venisse prima.
  const ordinate = [...UNITA].sort((a, b) => b.length - a.length);
  return ordinate.find((u) => testo.endsWith(u)) ?? null;
}

/**
 * Una lunghezza col numero nuovo e l'unità che aveva; `predefinita` se non ne
 * aveva una.
 *
 * I cursori dell'ombra e del ritaglio scrivevano sempre la loro unità — `px`
 * l'una, `%` l'altro — su campi che il formato accetta in qualunque lunghezza:
 * «0.5rem» toccato diventava «1px», un inset in px diventava in %. È il difetto
 * che `Cursore` e `ValoreLunghezza` avevano già corretto (`controlli.tsx`).
 */
export function conUnita(valore: unknown, numero: number | string, predefinita = "px"): string {
  return `${numero}${unitaDi(valore) ?? predefinita}`;
}

/**
 * I quattro lati di un `inset` con il lato `indice` portato a `numero`: gli
 * altri restano scritti com'erano, e quello toccato tiene la sua unità.
 */
export function latoCambiato(lati: readonly unknown[], indice: number, numero: number): string[] {
  return [0, 1, 2, 3].map((j) => {
    const lato = lati[j];
    if (j === indice) return conUnita(lato, numero, "%");
    return typeof lato === "string" ? lato : `${Number(lato) || 0}%`;
  });
}

/**
 * Quante fermate al massimo in un gradiente: `document.rs`, `lista_fra(…, 2, 8)`.
 *
 * Il registro non porta questi due tetti (porta quelli del movimento, come
 * `maxFotogrammi`): sono scritti qui, col rimando al nucleo. Fino alla 3.2 i
 * tasti che aggiungono non avevano tetto, mentre quello che toglie le fermate
 * si spegneva a due: la nona fermata o il settimo livello rompevano il
 * documento, contro la prima regola dello Studio — non si può scrivere quel
 * che il formato non accetta.
 */
export const FERMATE_MASSIME = 8;
/** Quanti livelli al massimo in un'ombra: `document.rs`, `lista_fra(…, 0, 6)`. */
export const LIVELLI_OMBRA_MASSIMI = 6;

/** Se ne può aggiungere un altro, con `quanti` già scritti e quel tetto. */
export function puoAggiungere(quanti: number, tetto: number): boolean {
  return quanti < tetto;
}

/**
 * Il numero di una casella della `cubicBezier` (x1, y1, x2, y2), o `null` per
 * non scriverlo.
 *
 * `null` per quel che non è un numero finito — una casella svuotata per
 * riscriverla, un segno meno da solo — e per quel che esce dagli estremi che
 * `document.rs::easing` verifica: le x fra 0 e 1, le y fra -5 e 5. Fino alla
 * 3.2 si scriveva `Number.parseFloat` così com'era: una casella vuota
 * diventava `NaN`, cioè `null` nel testo, e un errore di validazione a ogni
 * battuta.
 */
export function puntoDaCasella(testo: string, indice: number): number | null {
  if (testo.trim() === "") return null;
  const numero = Number(testo);
  if (!Number.isFinite(numero)) return null;
  const [min, max] = indice % 2 === 0 ? [0, 1] : [-5, 5];
  return numero >= min && numero <= max ? numero : null;
}

/**
 * Il cursore del peso di un carattere variabile.
 *
 * Un `<input type="range">` ammette solo `min + k·passo`: con min 1 e passo 50
 * — com'era fino alla 3.2 — i pesi raggiungibili erano 1, 51, 101… 951, e il
 * [100, 900] scritto dal selettore si spostava a [101, 901] al primo tocco.
 * Da 100 a passi di 50 si arriva a ogni peso con un nome. Un peso dichiarato
 * sotto 100 resta scrivibile nel codice, che per un carattere vero basta.
 */
export const CURSORE_PESO_VARIABILE = { min: 100, max: 1000, passo: 50 } as const;
