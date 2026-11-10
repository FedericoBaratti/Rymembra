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

const pila: Gestore[] = [];
let sopra: Gestore = () => false;
let scala: Gestore = () => false;

/**
 * Mette un gestore in cima, davanti alla scala, finché resta aperto quel che
 * chiude. Restituisce la funzione che lo toglie.
 */
export function suIndietro(gestore: Gestore): () => void {
  pila.push(gestore);
  return () => {
    const dove = pila.lastIndexOf(gestore);
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
    if (pila[i]?.()) return true;
  }
  return scala();
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
