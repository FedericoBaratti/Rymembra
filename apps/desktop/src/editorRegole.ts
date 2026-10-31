/**
 * Le parti dell'editor delle regole (`Regole.tsx`) che non hanno bisogno di
 * React: l'anteprima col suo turno, e i valori dei campi numerici.
 */
import { vinceLUltima } from "./ultima";

/**
 * L'anteprima, chiesta a ogni modifica, con il turno.
 *
 * Senza turno ogni risposta si scriveva com'era arrivata: su una libreria
 * grande, scrivendo «beatles», sette domande sono in volo insieme, e il nucleo
 * non promette l'ordine (`ultima.ts`) — se «beatl» tornava dopo «beatles»
 * restavano a schermo il numero e i brani di «beatl», cioè proprio il numero
 * che la finestrella esiste per mostrare giusto. Con il turno conta solo
 * l'ultima chiesta, anche per gli errori: un guasto di una domanda superata
 * non accende il rosso sopra un'anteprima giusta.
 *
 * `risposta` riceve l'anteprima ed è anche il posto dove togliere l'errore
 * **dell'anteprima** (e solo quello: la lettura delle regole e il salvataggio
 * hanno il loro, che un'anteprima riuscita non deve cancellare).
 */
export function creaProva<I, R>(
  chiedi: (quali: I) => Promise<R>,
  scrivi: { risposta: (r: R) => void; errore: (e: unknown) => void },
): (quali: I) => void {
  const turno = vinceLUltima();
  return (quali) => {
    const mio = turno();
    chiedi(quali).then(
      (r) => {
        if (mio()) scrivi.risposta(r);
      },
      (e: unknown) => {
        if (mio()) scrivi.errore(e);
      },
    );
  };
}

/**
 * Il valore di un campo numerico di una regola: un intero, o `null` se il
 * campo è vuoto.
 *
 * Il nucleo vuole un `i64`: un «3.5» arrivava com'era e Tauri rifiutava gli
 * argomenti con una frase di deserializzazione grezza («invalid type: floating
 * point `3.5`, expected i64»). Si tronca, e si resta dentro gli interi che un
 * `number` rappresenta esatti. Vuoto è `null` e non zero: è una regola ancora
 * da scrivere (`completa`), non «uguale a 0».
 */
export function numeroDaCampo(testo: string): number | null {
  const intero = interoDa(testo);
  if (intero === null) return null;
  return Math.min(Number.MAX_SAFE_INTEGER, Math.max(-Number.MAX_SAFE_INTEGER, intero));
}

/** Il più grande `u32`: «Al massimo» arriva al nucleo come `Option<u32>`. */
const LIMITE_MASSIMO = 4_294_967_295;

/** «Al massimo N brani»: un intero fra 0 e il più grande `u32`, o `null` se vuoto. */
export function limiteDaCampo(testo: string): number | null {
  const intero = interoDa(testo);
  if (intero === null) return null;
  return Math.min(LIMITE_MASSIMO, Math.max(0, intero));
}

function interoDa(testo: string): number | null {
  const pulito = testo.trim();
  if (pulito === "") return null;
  const n = Number(pulito);
  if (Number.isNaN(n)) return null;
  // «1e400» è infinito: vale quanto il più grande, e chi lo chiama lo taglia.
  if (!Number.isFinite(n)) return n > 0 ? Number.MAX_SAFE_INTEGER : -Number.MAX_SAFE_INTEGER;
  // `+ 0` perché `Math.trunc(-0.5)` è `-0`, che si scriverebbe «-0».
  return Math.trunc(n) + 0;
}

/**
 * La durata di una regola, scritta in minuti e secondi: «5:30», o «5» per
 * cinque minuti. `null` se il campo è vuoto o non si legge.
 *
 * Si chiedeva in millisecondi, e per un pubblico generale il millisecondo non
 * è un'unità con cui si ragiona: «più lunghi di 5» erano cinque millesimi di
 * secondo, cioè tutta la libreria. Il nucleo continua a ricevere millisecondi
 * (`t.duration_ms`); la conversione sta qui, nei due versi. I decimali dopo i
 * secondi («5:30.5») servono solo alle regole salvate quando il campo era in
 * millisecondi, perché tornino identiche.
 */
export function durataDaCampo(testo: string): number | null {
  const forma = /^(\d+)(?::(\d{1,2})(?:[.,](\d{1,3}))?)?$/.exec(testo.trim());
  if (!forma) return null;
  const minuti = Number(forma[1]);
  const secondi = forma[2] === undefined ? 0 : Number(forma[2]);
  if (secondi >= 60) return null;
  const millesimi = forma[3] === undefined ? 0 : Number(forma[3].padEnd(3, "0"));
  return Math.min(Number.MAX_SAFE_INTEGER, minuti * 60_000 + secondi * 1000 + millesimi);
}

/** I millisecondi di una regola, come si scrivono nel campo: «5:30». */
export function durataInCampo(ms: number | null | undefined): string {
  if (ms == null) return "";
  const tutto = Math.max(0, Math.trunc(ms));
  const minuti = Math.floor(tutto / 60_000);
  const resto = tutto - minuti * 60_000;
  const secondi = Math.floor(resto / 1000);
  const millesimi = resto - secondi * 1000;
  const base = `${minuti}:${String(secondi).padStart(2, "0")}`;
  if (millesimi === 0) return base;
  return `${base}.${String(millesimi).padStart(3, "0").replace(/0+$/, "")}`;
}
