/**
 * I colori, fatti di numeri: leggerli, scriverli, spostarli in OKLCH.
 *
 * # Perché OKLCH
 *
 * Un selettore in RGB o in HSL sposta la luminosità percepita mentre si cambia
 * la tinta: un giallo e un blu con la stessa «L» di HSL non hanno niente della
 * stessa luce, e una tavolozza costruita così ha un colore che grida e uno che
 * sparisce. In OKLCH la L è quella che l'occhio legge, quindi girare la tinta a
 * L ferma cambia colore senza cambiare contrasto — che è la cosa che una skin
 * deve poter fare senza rifare i conti col testo.
 *
 * # Perché si scrive in esadecimale
 *
 * Il formato accetta `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()` e `rgba()`
 * (`document.rs::colore_letterale`), non `oklch()`. Il selettore lavora in OKLCH
 * e scrive il risultato in esadecimale, col canale alfa solo quando serve.
 *
 * Le formule sono quelle di Björn Ottosson (2020), le stesse di `dinamico.rs`.
 */

/** Un colore in sRGB, canali da 0 a 1, alfa da 0 a 1. */
export interface Rgba {
  r: number;
  g: number;
  b: number;
  a: number;
}

/** Un colore in OKLCH: L da 0 a 1, C da 0 a ~0,37, H in gradi. */
export interface Oklch {
  l: number;
  c: number;
  h: number;
  a: number;
}

const fra = (x: number, min: number, max: number) => Math.min(max, Math.max(min, x));

/** Un letterale del documento, o `null` se non è una delle sei forme ammesse. */
export function leggiColore(testo: string): Rgba | null {
  const s = testo.trim().toLowerCase();
  const esa = /^#([0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/.exec(s);
  if (esa) {
    let cifre = esa[1] ?? "";
    if (cifre.length <= 4) cifre = [...cifre].map((c) => c + c).join("");
    const canale = (i: number) => Number.parseInt(cifre.slice(i, i + 2), 16) / 255;
    return { r: canale(0), g: canale(2), b: canale(4), a: cifre.length === 8 ? canale(6) : 1 };
  }
  const funzione = /^rgba?\(\s*([^)]*)\)$/.exec(s);
  if (funzione) {
    const parti = (funzione[1] ?? "").split(/[\s,/]+/).filter((p) => p !== "");
    if (parti.length < 3 || parti.length > 4) return null;
    const numeri = parti.map((p, i) => {
      const n = Number.parseFloat(p);
      if (!Number.isFinite(n)) return Number.NaN;
      if (p.endsWith("%")) return n / 100;
      return i < 3 ? n / 255 : n;
    });
    if (numeri.some((n) => !Number.isFinite(n))) return null;
    return {
      r: fra(numeri[0] ?? 0, 0, 1),
      g: fra(numeri[1] ?? 0, 0, 1),
      b: fra(numeri[2] ?? 0, 0, 1),
      a: fra(numeri[3] ?? 1, 0, 1),
    };
  }
  return null;
}

const due = (x: number) =>
  Math.round(fra(x, 0, 1) * 255)
    .toString(16)
    .padStart(2, "0");

/** In esadecimale: sei cifre se è opaco, otto se no. */
export function esadecimale(c: Rgba): string {
  const base = `#${due(c.r)}${due(c.g)}${due(c.b)}`;
  return c.a >= 0.9995 ? base : `${base}${due(c.a)}`;
}

/** Le sole sei cifre, per un `<input type="color">` che l'alfa non la conosce. */
export function esadecimaleOpaco(c: Rgba): string {
  return `#${due(c.r)}${due(c.g)}${due(c.b)}`;
}

const lineare = (x: number) => (x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4);
const gamma = (x: number) => (x <= 0.0031308 ? 12.92 * x : 1.055 * x ** (1 / 2.4) - 0.055);

export function versoOklch(c: Rgba): Oklch {
  const r = lineare(c.r);
  const g = lineare(c.g);
  const b = lineare(c.b);
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  const C = Math.hypot(A, B);
  // Un grigio non ha tinta: si tiene zero invece del rumore di arrotondamento,
  // così il cursore della tinta non salta quando si toglie tutto il croma.
  const H = C < 1e-4 ? 0 : ((Math.atan2(B, A) * 180) / Math.PI + 360) % 360;
  return { l: L, c: C, h: H, a: c.a };
}

/** Da OKLCH, senza correggere: i canali possono uscire da [0, 1]. */
function grezzo(o: Oklch): Rgba {
  const rad = (o.h * Math.PI) / 180;
  const A = o.c * Math.cos(rad);
  const B = o.c * Math.sin(rad);
  const l = (o.l + 0.3963377774 * A + 0.2158037573 * B) ** 3;
  const m = (o.l - 0.1055613458 * A - 0.0638541728 * B) ** 3;
  const s = (o.l - 0.0894841775 * A - 1.291485548 * B) ** 3;
  return {
    r: gamma(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
    g: gamma(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
    b: gamma(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s),
    a: o.a,
  };
}

const dentro = (c: Rgba) =>
  [c.r, c.g, c.b].every((x) => x >= -1e-4 && x <= 1 + 1e-4);

/**
 * Da OKLCH a sRGB, dentro lo schermo.
 *
 * Un croma alto a una tinta e una luce date può non esistere in sRGB. Tagliare
 * i canali cambierebbe la tinta; si riduce invece il croma finché il colore ci
 * sta, che è quel che fanno i selettori seri: la luce e la tinta restano quelle
 * scelte, e si perde solo un po' di saturazione.
 */
export function daOklch(o: Oklch): Rgba {
  const primo = grezzo(o);
  if (dentro(primo)) return pulito(primo);
  let basso = 0;
  let alto = o.c;
  for (let i = 0; i < 24; i += 1) {
    const mezzo = (basso + alto) / 2;
    if (dentro(grezzo({ ...o, c: mezzo }))) basso = mezzo;
    else alto = mezzo;
  }
  return pulito(grezzo({ ...o, c: basso }));
}

function pulito(c: Rgba): Rgba {
  return { r: fra(c.r, 0, 1), g: fra(c.g, 0, 1), b: fra(c.b, 0, 1), a: fra(c.a, 0, 1) };
}

/** La luminanza relativa di WCAG 2. */
export function luminanza(c: Rgba): number {
  return 0.2126 * lineare(c.r) + 0.7152 * lineare(c.g) + 0.0722 * lineare(c.b);
}

/**
 * Il contrasto fra due colori opachi, da 1 a 21.
 *
 * Un davanti trasparente si compone prima sul dietro: è quel che l'occhio
 * vede, ed è come lo misura `check_skin`.
 */
export function contrasto(davanti: Rgba, dietro: Rgba): number {
  const composto: Rgba = {
    r: davanti.r * davanti.a + dietro.r * (1 - davanti.a),
    g: davanti.g * davanti.a + dietro.g * (1 - davanti.a),
    b: davanti.b * davanti.a + dietro.b * (1 - davanti.a),
    a: 1,
  };
  const uno = luminanza(composto);
  const altro = luminanza(dietro);
  return (Math.max(uno, altro) + 0.05) / (Math.min(uno, altro) + 0.05);
}
