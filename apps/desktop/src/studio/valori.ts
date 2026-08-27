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

/** Il costo sommato di una pila di livelli. */
export function costoPila(
  livelli: readonly unknown[],
  effetti: readonly EffettoRegistro[],
  motivi: Motivi = {},
): number {
  return livelli.reduce<number>(
    (somma, livello) => somma + costoDi(livello, effetti, motivi),
    0,
  );
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
