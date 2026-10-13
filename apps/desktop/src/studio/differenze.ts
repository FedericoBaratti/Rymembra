/**
 * Il confronto fra due versioni dello stesso documento: per riga, e per
 * percorso.
 *
 * # Per riga, con un confronto vero
 *
 * La prima versione di questo file confrontava la riga `i` con la riga `i`, e
 * lo diceva: niente mosse, niente blocchi. Bastava una riga aggiunta in cima —
 * un colore nuovo nella tavolozza — perché tutto quel che veniva dopo si
 * leggesse cambiato, e la chat mostrasse cento righe rosse per una modifica di
 * una. Qui c'è l'algoritmo di Myers (1986), che trova il percorso più corto di
 * aggiunte e rimozioni: sono trenta righe e nessuna dipendenza, e il risultato
 * è quello di `git diff`.
 *
 * # Per percorso
 *
 * Una riga di JSON dice poco: `"radius": "14px",` non dice di chi. Il
 * confronto per percorso dice `parts.card.radius: 12px → 14px`, che è la
 * domanda vera — «che cosa cambia nella skin» — e resta leggibile anche quando
 * la stampa del documento sposta mezzo file.
 *
 * Tutti e due servono a due lettori: il codice dello Studio («che cosa ho
 * cambiato dall'apertura») e la chat («che cosa cambierebbe se accetto»).
 */

/** Una riga del confronto. */
export type Riga = {
  /** Uguale, aggiunta, tolta. */
  segno: "=" | "+" | "-";
  testo: string;
};

/**
 * Oltre questa distanza il confronto smette di cercare il minimo.
 *
 * Ogni passo tiene una copia del fronte: milleduecento passi sono una decina di
 * megabyte nel caso peggiore, e due documenti che differiscono di più di
 * milleduecento righe non hanno un confronto da leggere — hanno due testi.
 */
const DISTANZA_MASSIMA = 1200;

/**
 * Le righe dei due testi, con quel che è rimasto, quel che è sparito e quel che
 * è arrivato, nell'ordine in cui si leggono.
 */
export function differenze(prima: string, dopo: string): Riga[] {
  const a = prima.split("\n");
  const b = dopo.split("\n");
  // Il capo e la coda uguali si tolgono subito: quasi sempre sono il
  // novantanove per cento del documento, e il confronto vero lavora sul resto.
  let inizio = 0;
  while (inizio < a.length && inizio < b.length && a[inizio] === b[inizio]) inizio += 1;
  let fineA = a.length;
  let fineB = b.length;
  while (fineA > inizio && fineB > inizio && a[fineA - 1] === b[fineB - 1]) {
    fineA -= 1;
    fineB -= 1;
  }
  const mezzo = myers(a.slice(inizio, fineA), b.slice(inizio, fineB));
  return [
    ...a.slice(0, inizio).map((testo) => ({ segno: "=" as const, testo })),
    ...mezzo,
    ...a.slice(fineA).map((testo) => ({ segno: "=" as const, testo })),
  ];
}

/** Il cammino più corto di Myers, ricostruito all'indietro. */
function myers(a: readonly string[], b: readonly string[]): Riga[] {
  const n = a.length;
  const m = b.length;
  if (n === 0) return b.map((testo) => ({ segno: "+", testo }));
  if (m === 0) return a.map((testo) => ({ segno: "-", testo }));
  const massimo = Math.min(n + m, DISTANZA_MASSIMA);
  const scarto = massimo;
  let v = new Int32Array(2 * massimo + 2);
  const tracce: Int32Array[] = [];
  let trovato = false;
  for (let d = 0; d <= massimo && !trovato; d += 1) {
    tracce.push(v.slice());
    const nuovo = v.slice();
    for (let k = -d; k <= d; k += 2) {
      let x =
        k === -d || (k !== d && (v[scarto + k - 1] ?? 0) < (v[scarto + k + 1] ?? 0))
          ? (v[scarto + k + 1] ?? 0)
          : (v[scarto + k - 1] ?? 0) + 1;
      let y = x - k;
      while (x < n && y < m && a[x] === b[y]) {
        x += 1;
        y += 1;
      }
      nuovo[scarto + k] = x;
      if (x >= n && y >= m) {
        trovato = true;
        break;
      }
    }
    v = nuovo;
  }
  if (!trovato) {
    // Troppo diversi per cercare il minimo: tutto via e tutto nuovo, che è
    // vero e si legge.
    return [
      ...a.map((testo) => ({ segno: "-" as const, testo })),
      ...b.map((testo) => ({ segno: "+" as const, testo })),
    ];
  }
  const righe: Riga[] = [];
  let x = n;
  let y = m;
  for (let d = tracce.length - 1; d >= 0; d -= 1) {
    const traccia = tracce[d];
    if (traccia === undefined) break;
    const k = x - y;
    const giu =
      k === -d || (k !== d && (traccia[scarto + k - 1] ?? 0) < (traccia[scarto + k + 1] ?? 0));
    const kPrima = giu ? k + 1 : k - 1;
    const xPrima = traccia[scarto + kPrima] ?? 0;
    const yPrima = xPrima - kPrima;
    while (x > xPrima && y > yPrima) {
      x -= 1;
      y -= 1;
      righe.push({ segno: "=", testo: a[x] ?? "" });
    }
    if (d > 0) {
      if (giu) {
        y -= 1;
        righe.push({ segno: "+", testo: b[y] ?? "" });
      } else {
        x -= 1;
        righe.push({ segno: "-", testo: a[x] ?? "" });
      }
    }
  }
  return righe.reverse();
}

/** Solo le righe che cambiano, con quante ne sono. */
export function quanteCambiano(righe: readonly Riga[]): {
  aggiunte: number;
  tolte: number;
} {
  let aggiunte = 0;
  let tolte = 0;
  for (const riga of righe) {
    if (riga.segno === "+") aggiunte += 1;
    else if (riga.segno === "-") tolte += 1;
  }
  return { aggiunte, tolte };
}

/** Un valore cambiato, con dov'era. */
export interface Cambio {
  percorso: string;
  /** `undefined` se prima non c'era. */
  prima: unknown;
  /** `undefined` se dopo non c'è più. */
  dopo: unknown;
}

const eOggetto = (v: unknown): v is Record<string, unknown> =>
  v !== null && typeof v === "object" && !Array.isArray(v);

/**
 * Quel che cambia fra due documenti, per percorso.
 *
 * Gli oggetti si attraversano; tutto il resto — numeri, testi, e anche gli
 * elenchi — si confronta intero: un livello di sfondo spostato in cima è un
 * cambio di `background`, non tre cambi di indici.
 */
export function differenzePerPercorso(prima: unknown, dopo: unknown, via: readonly string[] = []): Cambio[] {
  if (eOggetto(prima) && eOggetto(dopo)) {
    const chiavi = [...new Set([...Object.keys(prima), ...Object.keys(dopo)])];
    return chiavi.flatMap((k) => differenzePerPercorso(prima[k], dopo[k], [...via, k]));
  }
  if (JSON.stringify(prima) === JSON.stringify(dopo)) return [];
  return [{ percorso: via.join("."), prima, dopo }];
}

/**
 * Come [`differenzePerPercorso`], ma dentro gli oggetti che arrivano o se ne
 * vanno interi: «`parts.card.radius`: 18px» e «`parts.card.states.hover.opacity`:
 * 0.92» invece di un solo `parts.card` con tutto dentro. È la forma della
 * chat, dove ogni riga porta alla cosa che nomina.
 */
export function differenzeFoglia(prima: unknown, dopo: unknown, via: readonly string[] = []): Cambio[] {
  const p = prima === undefined && eOggetto(dopo) ? {} : prima;
  const d = dopo === undefined && eOggetto(prima) ? {} : dopo;
  if (eOggetto(p) && eOggetto(d)) {
    const chiavi = [...new Set([...Object.keys(p), ...Object.keys(d)])];
    return chiavi.flatMap((k) => differenzeFoglia(p[k], d[k], [...via, k]));
  }
  if (JSON.stringify(p) === JSON.stringify(d)) return [];
  return [{ percorso: via.join("."), prima: p, dopo: d }];
}
