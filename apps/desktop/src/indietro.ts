/**
 * Il tasto (o il gesto) Indietro di Android.
 *
 * Su Android Indietro è il gesto più usato dopo il tocco, e vuol dire quel che
 * sul computer vuol dire Escape: chiudi quel che è aperto, un livello alla
 * volta. Per questo la sua strada è la stessa — la scala di `tastiera.ts` —
 * con davanti quel che solo il telefono ha, come il cassetto della
 * navigazione.
 *
 * L'attività Kotlin (`MainActivity.kt`) chiama `window.rymembraIndietro()` e
 * guarda la risposta: `true` se qualcosa si è chiuso; `false` se non c'era
 * niente da chiudere, e allora l'app va sullo sfondo come col tasto Home —
 * mai chiusa, perché la musica deve continuare.
 */

import { useEffect, useRef } from "react";

/** Chi sa chiudere qualcosa: `true` se l'ha chiuso. */
type Gestore = () => boolean;

/** Un gestore in pila, e se Escape sul computer lo scende anche lui. */
interface Voce {
  gestore: Gestore;
  sulComputer: boolean;
}

const pila: Voce[] = [];
let sopra: Gestore = () => false;
let scala: Gestore = () => false;

/**
 * Mette un gestore in cima, davanti alla scala, finché resta aperto quel che
 * chiude. Restituisce la funzione che lo toglie.
 *
 * `sulComputer`: chiudere quel gestore è anche quel che fa Escape (e `Alt+←`,
 * e il tasto indietro del mouse) sul computer — i livelli di una pagina che sul
 * telefono e sul computer si aprono nello stesso modo, come i dischi delle
 * pagine della libreria. Senza, il gestore è del solo telefono: sul computer la coda e la
 * terza colonna stanno accanto alla pagina, e Escape non deve chiuderle.
 */
export function suIndietro(
  gestore: Gestore,
  opzioni: { sulComputer?: boolean } = {},
): () => void {
  const voce: Voce = { gestore, sulComputer: opzioni.sulComputer === true };
  pila.push(voce);
  return () => {
    const dove = pila.lastIndexOf(voce);
    if (dove >= 0) pila.splice(dove, 1);
  };
}

/**
 * La scala di Escape, da `tastiera.ts`, in due metà: `prima` è quel che sta
 * sopra a tutto — il campo in cui si scrive, la finestrella in cima — e va
 * chiuso prima di qualunque cosa del telefono; `poi` sono i gradini della
 * pagina — la vista grande, il menu, la selezione, la ricerca, l'album aperto.
 */
export function impostaScala(prima: Gestore, poi: Gestore): void {
  sopra = prima;
  scala = poi;
}

/** Indietro: quel che sta sopra, il gestore più recente, o la scala. */
export function indietro(): boolean {
  if (sopra()) return true;
  for (let i = pila.length - 1; i >= 0; i--) {
    if (pila[i]?.gestore()) return true;
  }
  return scala();
}

/**
 * Escape, `Alt+←` e il tasto indietro del mouse, sul computer: i soli gestori
 * che si sono dichiarati anche per lui, dal più recente.
 *
 * La chiama la scala di `App` (`chiudi`), al suo posto fra i gradini — dopo il
 * menù e la selezione, che stanno sopra la pagina — e **non** chiama la scala:
 * `indietro()` la chiama lei, e il giro sarebbe senza fine.
 */
export function indietroSulComputer(): boolean {
  for (let i = pila.length - 1; i >= 0; i--) {
    const voce = pila[i];
    if (voce?.sulComputer === true && voce.gestore()) return true;
  }
  return false;
}

/**
 * Finché il componente è montato, Indietro chiama `chiudi` prima della scala.
 * Per quel che sul telefono copre la pagina e che Escape sul computer non
 * chiude, perché lì sta accanto: la coda, la terza colonna.
 */
export function useSuIndietro(chiudi: () => void): void {
  const ultima = useRef(chiudi);
  ultima.current = chiudi;
  useEffect(
    () =>
      suIndietro(() => {
        ultima.current();
        return true;
      }),
    [],
  );
}

declare global {
  interface Window {
    /** Vedi il preambolo: la chiama `MainActivity.kt`. */
    rymembraIndietro?: () => boolean;
  }
}

if (typeof window !== "undefined") window.rymembraIndietro = indietro;
