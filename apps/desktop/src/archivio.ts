/**
 * Un archivio fuori da React: un valore, chi lo scrive e chi lo legge.
 *
 * # Perché
 *
 * Lo stato di un componente ridisegna il componente intero quando cambia, e
 * `App` è il componente che disegna tutto: una pagina, la colonna, la barra,
 * l'elenco, le Impostazioni. Finché la riproduzione, la libreria e la scansione
 * stavano nei suoi `useState`, un passo del volume o un quarto di secondo di
 * scansione rifaceva la finestra intera per cambiare un numero in un angolo.
 *
 * Un archivio tiene il valore fuori, e sveglia solo chi si è iscritto: chi lo
 * legge con `useArchivio` si ridisegna quando cambia **la parte che ha
 * scelto**, e nessun altro. È lo stesso schema della posizione di
 * riproduzione (`posizione.ts`) e della lingua (`lingue/index.ts`), scritto
 * una volta per chi viene dopo.
 *
 * # Cosa non sa fare
 *
 * Le transizioni di vista: un aggiornamento che arriva da
 * `useSyncExternalStore` è sempre sincrono, e `<ViewTransition>` si accende
 * solo dentro `startTransition`. Per quel che compare e scompare — la barra
 * della scansione — c'è `useSpecchioPresente` in `presenza.tsx`, che copia il
 * valore in uno stato suo e passa dalla presenza come `useStatoPresente`.
 *
 * Senza React a runtime oltre agli hook in fondo: le prove di `test/` lo
 * caricano con `node --test`.
 */
import { useRef, useSyncExternalStore } from "react";

/** Un valore che si legge, si scrive e si segue. */
export interface Archivio<T> {
  /** Il valore adesso. La stessa identità finché nessuno lo riscrive. */
  leggi(): T;
  /**
   * Lo riscrive, o lo ricava da quello di prima.
   *
   * Un valore identico a quello che c'è (`Object.is`) non sveglia nessuno: è
   * quel che fa `setState` con lo stesso valore, e i gestori che rispondono
   * «com'era» — un volume che non è cambiato — restano gratuiti.
   */
  scrivi(valore: T | ((prima: T) => T)): void;
  /** Si iscrive ai cambi. Restituisce come si smette. */
  iscrivi(avvisa: () => void): () => void;
}

/** Un archivio visto da chi lo legge soltanto. */
export type Leggibile<T> = Pick<Archivio<T>, "leggi" | "iscrivi">;

/** Un archivio nuovo, con il suo valore di partenza. */
export function creaArchivio<T>(iniziale: T): Archivio<T> {
  let valore = iniziale;
  const iscritti = new Set<() => void>();
  return {
    leggi: () => valore,
    scrivi: (nuovo) => {
      const dopo =
        typeof nuovo === "function" ? (nuovo as (prima: T) => T)(valore) : nuovo;
      if (Object.is(dopo, valore)) return;
      valore = dopo;
      // Una copia: chi viene avvisato può smettere, o iscriversi, mentre si
      // scorre l'elenco.
      for (const avvisa of [...iscritti]) avvisa();
    },
    iscrivi: (avvisa) => {
      iscritti.add(avvisa);
      return () => {
        iscritti.delete(avvisa);
      };
    },
  };
}

/**
 * Una scelta su un valore, ricordata finché il valore non cambia.
 *
 * `useSyncExternalStore` chiede la stessa risposta a ogni lettura dello stesso
 * istante, e una scelta che costruisce un oggetto ne darebbe uno nuovo ogni
 * volta: React lo prende per un cambio, e ridisegna senza fine. Qui la scelta
 * si rifà solo quando cambia la sorgente, e se `uguali` dice che il risultato
 * è lo stesso di prima si tiene l'identità di prima — così un elenco di
 * conteggi riletto con gli stessi numeri non sveglia chi li disegna.
 *
 * Pura, e per questo a parte dall'hook: si prova da sola.
 */
export function sceltaRicordata<T, S>(
  scegli: (valore: T) => S,
  uguali: (a: S, b: S) => boolean = Object.is,
): (valore: T) => S {
  let ultima: { da: T; scelta: S } | null = null;
  return (valore) => {
    if (ultima !== null && Object.is(ultima.da, valore)) return ultima.scelta;
    const nuova = scegli(valore);
    const scelta = ultima !== null && uguali(ultima.scelta, nuova) ? ultima.scelta : nuova;
    ultima = { da: valore, scelta };
    return scelta;
  };
}

/** Due oggetti con le stesse chiavi e gli stessi valori (`Object.is`), uno per uno. */
export function ugualiInSuperficie<S extends object>(a: S, b: S): boolean {
  if (Object.is(a, b)) return true;
  const chiaviA = Object.keys(a);
  if (chiaviA.length !== Object.keys(b).length) return false;
  return chiaviA.every((k) =>
    Object.is((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]),
  );
}

/** Nessuna iscrizione: per chi, in questo disegno, non legge da un archivio. */
export const NESSUNA_ISCRIZIONE = (): (() => void) => () => undefined;

/**
 * Segue un archivio, o la parte che se ne sceglie.
 *
 * Senza `scegli` è il valore intero. Con `scegli`, il componente si ridisegna
 * solo quando cambia quel che la scelta restituisce: una scelta che risponde
 * con un numero o un booleano non sveglia nessuno per i cambi che non lo
 * toccano. `scegli` e `uguali` si leggono al primo disegno e restano quelli:
 * sono una domanda fissa, non un valore.
 */
export function useArchivio<T>(archivio: Leggibile<T>): T;
export function useArchivio<T, S>(
  archivio: Leggibile<T>,
  scegli: (valore: T) => S,
  uguali?: (a: S, b: S) => boolean,
): S;
export function useArchivio<T, S>(
  archivio: Leggibile<T>,
  scegli?: (valore: T) => S,
  uguali?: (a: S, b: S) => boolean,
): T | S {
  const scelta = useRef<((valore: T) => S) | null>(null);
  if (scelta.current === null && scegli !== undefined) {
    scelta.current = sceltaRicordata(scegli, uguali);
  }
  const leggi = (): T | S => {
    const valore = archivio.leggi();
    return scelta.current === null ? valore : scelta.current(valore);
  };
  return useSyncExternalStore(archivio.iscrivi, leggi, leggi);
}
