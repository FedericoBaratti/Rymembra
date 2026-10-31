/**
 * Un'animazione del documento, suonata dalla Web Animations API.
 *
 * La timeline e l'anteprima dell'ispettore non aspettano il compilatore: il
 * fotogramma che si sta trascinando deve muoversi mentre lo si trascina, e un
 * giro di validazione per ogni pixel sarebbe un'animazione a scatti di
 * un'animazione. Qui un fotogramma diventa un `Keyframe` con le stesse regole
 * di `compile.rs::fermata` — l'ordine fisso delle trasformazioni (traslazione,
 * rotazione, inclinazione, scala), i gradi, i filtri in fila — così quel che
 * suona qui è quel che suonerà nell'app.
 *
 * I colori e le ombre li risolve chi chiama: sono riferimenti al documento, e
 * il documento lo conosce lo Studio.
 */
import type { TokenRegistro } from "../ipc";
import { tintaNel } from "./valori";

/** Un fotogramma come lo scrive il documento. */
export type Fotogramma = Record<string, unknown> & { at?: unknown };

/** Come risolvere i valori che richiamano il documento. */
export interface Risolutori {
  colore: (valore: unknown) => string | null;
  ombra: (valore: unknown) => string | null;
  /** Una curva per nome, dalle curve della skin. */
  curva: (nome: string) => unknown;
}

const numero = (v: unknown): number | null => {
  if (typeof v === "number" && Number.isFinite(v)) return v;
  if (typeof v === "string") {
    const n = Number.parseFloat(v);
    return Number.isFinite(n) ? n : null;
  }
  return null;
};

/** Una lunghezza del documento in CSS: un numero nudo vale pixel. */
export function lunghezzaCss(v: unknown): string | null {
  if (typeof v === "number" && Number.isFinite(v)) return `${v}px`;
  if (typeof v === "string" && v.trim() !== "") return v.trim();
  return null;
}

/** Un ritaglio del documento in `clip-path`. */
export function ritaglioCss(v: unknown): string | null {
  if (v === null || typeof v !== "object") return null;
  const dentro = v as Record<string, unknown>;
  const lati = dentro["inset"];
  if (Array.isArray(lati) && lati.length === 4) {
    return `inset(${lati.map((l) => lunghezzaCss(l) ?? "0px").join(" ")})`;
  }
  const cerchio = dentro["circle"];
  if (cerchio !== null && typeof cerchio === "object") {
    const c = cerchio as Record<string, unknown>;
    const at = Array.isArray(c["at"]) ? (c["at"] as unknown[]) : ["50%", "50%"];
    return `circle(${lunghezzaCss(c["radius"]) ?? "50%"} at ${lunghezzaCss(at[0]) ?? "50%"} ${lunghezzaCss(at[1]) ?? "50%"})`;
  }
  const poligono = dentro["polygon"];
  if (Array.isArray(poligono)) {
    return `polygon(${poligono
      .map((p) => (Array.isArray(p) ? `${lunghezzaCss(p[0]) ?? "0"} ${lunghezzaCss(p[1]) ?? "0"}` : "0 0"))
      .join(", ")})`;
  }
  return null;
}

/** Le parole dell'origine, come le legge `document.rs::origine`. */
const ORIGINI: Readonly<Record<string, string>> = {
  center: "50% 50%",
  top: "50% 0%",
  bottom: "50% 100%",
  left: "0% 50%",
  right: "100% 50%",
  "top left": "0% 0%",
  "top right": "100% 0%",
  "bottom left": "0% 100%",
  "bottom right": "100% 100%",
};

/** Un'origine del documento in `transform-origin`, o `null`. */
export function origineCss(v: unknown): string | null {
  if (typeof v === "string") return ORIGINI[v] ?? null;
  if (Array.isArray(v) && v.length === 2) {
    const x = lunghezzaCss(v[0]);
    const y = lunghezzaCss(v[1]);
    return x !== null && y !== null ? `${x} ${y}` : null;
  }
  return null;
}

/** Un fotogramma in `Keyframe`. */
export function versoKeyframe(
  f: Fotogramma,
  risolvi: Risolutori,
  origine: string | null,
): Keyframe {
  const k: Keyframe = { offset: Math.min(1, Math.max(0, (numero(f.at) ?? 0) / 100)) };
  const opacita = numero(f["opacity"]);
  if (opacita !== null) k["opacity"] = opacita;

  const trasformazioni: string[] = [];
  const tx = lunghezzaCss(f["translateX"]);
  if (tx !== null) trasformazioni.push(`translateX(${tx})`);
  const ty = lunghezzaCss(f["translateY"]);
  if (ty !== null) trasformazioni.push(`translateY(${ty})`);
  const r = numero(f["rotate"]);
  if (r !== null) trasformazioni.push(`rotate(${r}deg)`);
  const sx = numero(f["skewX"]);
  if (sx !== null) trasformazioni.push(`skewX(${sx}deg)`);
  const sy = numero(f["skewY"]);
  if (sy !== null) trasformazioni.push(`skewY(${sy}deg)`);
  const s = numero(f["scale"]);
  if (s !== null) trasformazioni.push(`scale(${s})`);
  const scx = numero(f["scaleX"]);
  if (scx !== null) trasformazioni.push(`scaleX(${scx})`);
  const scy = numero(f["scaleY"]);
  if (scy !== null) trasformazioni.push(`scaleY(${scy})`);
  if (trasformazioni.length > 0) {
    k["transform"] = trasformazioni.join(" ");
    if (origine !== null) k["transformOrigin"] = origine;
  }

  const filtri: string[] = [];
  const blur = lunghezzaCss(f["blur"]);
  if (blur !== null) filtri.push(`blur(${blur})`);
  const luce = numero(f["brightness"]);
  if (luce !== null) filtri.push(`brightness(${luce})`);
  const sat = numero(f["saturate"]);
  if (sat !== null) filtri.push(`saturate(${sat})`);
  const tinta = numero(f["hueRotate"]);
  if (tinta !== null) filtri.push(`hue-rotate(${tinta}deg)`);
  if (filtri.length > 0) k["filter"] = filtri.join(" ");

  const fondo = f["backgroundColor"] === undefined ? null : risolvi.colore(f["backgroundColor"]);
  if (fondo !== null) k["backgroundColor"] = fondo;
  const testo = f["color"] === undefined ? null : risolvi.colore(f["color"]);
  if (testo !== null) k["color"] = testo;
  const bordo = f["borderColor"] === undefined ? null : risolvi.colore(f["borderColor"]);
  if (bordo !== null) k["borderColor"] = bordo;
  const ombra = f["shadow"] === undefined ? null : risolvi.ombra(f["shadow"]);
  if (ombra !== null) k["boxShadow"] = ombra;
  const ritaglio = ritaglioCss(f["clip"]);
  if (ritaglio !== null) k["clipPath"] = ritaglio;
  return k;
}

/** Una curva del documento in `easing` CSS; `linear` se non la si legge. */
export function curvaCss(v: unknown, risolvi: Risolutori, profondita = 0): string {
  if (typeof v === "string") {
    if (["linear", "ease", "ease-in", "ease-out", "ease-in-out"].includes(v)) return v;
    return profondita > 2 ? "linear" : curvaCss(risolvi.curva(v), risolvi, profondita + 1);
  }
  if (v === null || typeof v !== "object") return "ease";
  const d = v as Record<string, unknown>;
  if (d["kind"] === "keyword" && typeof d["keyword"] === "string") return d["keyword"];
  if (d["kind"] === "cubicBezier" && Array.isArray(d["points"]) && d["points"].length === 4) {
    return `cubic-bezier(${d["points"].map(Number).join(", ")})`;
  }
  if (d["kind"] === "steps") {
    return `steps(${Number(d["count"] ?? 1)}, ${d["position"] === "start" ? "start" : "end"})`;
  }
  return "ease";
}

/** Una durata del documento in millisecondi. */
export function durataMs(v: unknown): number {
  if (typeof v === "number") return v;
  if (typeof v !== "string") return 0;
  const n = Number.parseFloat(v);
  if (!Number.isFinite(n)) return 0;
  return /ms\s*$/.test(v) ? n : /s\s*$/.test(v) ? n * 1000 : n;
}

/** I fotogrammi e le opzioni di un'animazione, pronti per `element.animate`. */
export function versoAnimazione(
  animazione: Record<string, unknown>,
  risolvi: Risolutori,
): { fotogrammi: Keyframe[]; opzioni: KeyframeAnimationOptions } {
  const origine = origineCss(animazione["origin"]);
  const fermate = Array.isArray(animazione["frames"]) ? (animazione["frames"] as Fotogramma[]) : [];
  const iterazioni = animazione["iterations"];
  // La curva va su ogni fotogramma e non sull'animazione: in CSS
  // `animation-timing-function` vale per ogni tratto fra due fotogrammi, e
  // l'`easing` delle opzioni della Web Animations API vale invece per l'intera
  // iterazione. Con tre fotogrammi e una curva d'uscita le due letture danno
  // due movimenti diversi, e il provino mentirebbe.
  const curva = curvaCss(animazione["easing"], risolvi);
  return {
    fotogrammi: fermate.map((f) => ({ ...versoKeyframe(f, risolvi, origine), easing: curva })),
    opzioni: {
      duration: Math.max(1, durataMs(animazione["duration"])),
      delay: durataMs(animazione["delay"]),
      easing: "linear",
      iterations: iterazioni === "infinite" ? Number.POSITIVE_INFINITY : (numero(iterazioni) ?? 1),
      direction: (typeof animazione["direction"] === "string"
        ? animazione["direction"]
        : "normal") as PlaybackDirection,
      fill: (typeof animazione["fill"] === "string" ? animazione["fill"] : "both") as FillMode,
    },
  };
}

/** I campi animati di un'animazione, nell'ordine del registro. */
export function campiAnimati(frames: readonly Fotogramma[], ordine: readonly string[]): string[] {
  const presenti = new Set<string>();
  for (const f of frames) for (const k of Object.keys(f)) if (k !== "at") presenti.add(k);
  return ordine.filter((c) => presenti.has(c));
}

/**
 * I risolutori per una bozza: i colori e le ombre seguono i riferimenti nel
 * documento, le curve per nome si cercano in `motion.easings`.
 */
export function risolutoriDi(
  documento: Record<string, unknown> | null,
  variante: boolean,
  tokens: readonly TokenRegistro[],
): Risolutori {
  const colore = (v: unknown) => tintaNel(v, documento, variante, tokens);
  return {
    colore,
    ombra: (v) => {
      if (v === null || typeof v !== "object") return null;
      const d = v as Record<string, unknown>;
      if (typeof d["$token"] === "string") {
        const def = tokens.find((t) => t.id === d["$token"]);
        return def === undefined ? null : `var(${def.css})`;
      }
      const livelli = Array.isArray(d["layers"]) ? (d["layers"] as Record<string, unknown>[]) : [];
      if (livelli.length === 0) return "none";
      return livelli
        .map((l) =>
          [
            l["inset"] === true ? "inset" : "",
            lunghezzaCss(l["x"]) ?? "0px",
            lunghezzaCss(l["y"]) ?? "0px",
            lunghezzaCss(l["blur"]) ?? "0px",
            lunghezzaCss(l["spread"]) ?? "",
            colore(l["color"]) ?? "transparent",
          ]
            .filter((x) => x !== "")
            .join(" "),
        )
        .join(", ");
    },
    curva: (nome) => {
      const movimento = (documento?.["motion"] ?? {}) as Record<string, unknown>;
      return ((movimento["easings"] ?? {}) as Record<string, unknown>)[nome];
    },
  };
}

/**
 * Il gesto di un rombo trascinato: per ogni posizione, se va scritta.
 *
 * Si confronta con l'**ultima scritta**, non con quella di partenza. Il
 * gestore del puntatore vede i fotogrammi del `pointerdown`, e fino alla 3.2
 * confrontava con loro: riportato al punto di partenza, il rombo trovava
 * «uguale» e saltava la scrittura, e restava a 41 — o dov'era passato per
 * ultimo — con la testina ferma lì.
 */
export function gestoRombo(partenza: number): (at: number) => boolean {
  let ultima = partenza;
  return (at) => {
    if (at === ultima) return false;
    ultima = at;
    return true;
  };
}

/**
 * Come parte il provino dell'ispettore di un'animazione.
 *
 * `ridotto` è «Riduci movimento», di Rymembra o del sistema; `chiesto` vuol
 * dire che questa partenza l'ha chiesta il tasto ▶, e non una modifica del
 * documento. La Web Animations API non legge `--motion-scale`, che è l'unico
 * interruttore di `stile.css`: fino alla 3.2 il provino partiva da solo a ogni
 * modifica e, con `iterations: "infinite"`, pulsava senza fine anche per chi
 * aveva chiesto di non muoversi. Adesso con la preferenza ridotta resta fermo
 * sull'ultimo fotogramma, e il tasto lo fa suonare una volta sola. La tela fa
 * la sua parte da sé (`Tela.tsx`); il provino della timeline non parte mai da
 * solo — lo guida la testina — e non ne ha bisogno.
 */
export function avvioProvino(ridotto: boolean, chiesto: boolean): "suona" | "unaVolta" | "fermo" {
  if (!ridotto) return "suona";
  return chiesto ? "unaVolta" : "fermo";
}
