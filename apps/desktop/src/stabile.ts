/**
 * Una funzione con un'identità sola, che chiama sempre la sua ultima versione.
 *
 * Serve a dare a un componente sotto `memo` un gesto di `App` che legge il
 * suo stato — `scegliCartella` legge le cartelle di `avvio`, `scansiona` il
 * conto delle scansioni in volo — senza che ogni disegno di `App` ne faccia
 * una nuova, e con lei un disegno in più delle Impostazioni.
 *
 * Si aggiorna durante il disegno, come i riferimenti di `useStatoPresente` e
 * di `useAscolto`: chi la chiama lo fa a un clic, cioè dopo che il disegno è
 * finito, e trova l'ultima.
 */
import { useCallback, useRef } from "react";

/** Vedi il commento in testa. */
export function useStabile<A extends unknown[], R>(
  funzione: (...argomenti: A) => R,
): (...argomenti: A) => R {
  const ultima = useRef(funzione);
  ultima.current = funzione;
  return useCallback((...argomenti: A) => ultima.current(...argomenti), []);
}
