/**
 * Il testo che un campo tiene in mano, scritto prima che lo Studio chiuda.
 *
 * # Il difetto (L-PC 8)
 *
 * Un campo che si conferma solo all'uscita — Tab, Invio, un clic altrove — tiene
 * il testo dentro di sé finché non esce. Scritto il nome e subito Alt+F4, la
 * bozza restava quella della creazione: l'uscita dal campo non avveniva mai,
 * perché la finestra si chiudeva sotto di lui. Con un Tab prima, il nome
 * arrivava alla bozza sette millisecondi prima dell'uscita.
 *
 * # Il gancio, che c'era già
 *
 * `registraSospeso` (vedi `contesto.ts`): chi salva per un gesto — «Esci»,
 * Ctrl+S, «Prova», «Esporta» — e la chiusura che aspetta lo Studio
 * (`primaDiChiudere`, R14-7) chiamano `svuotaSospesi` dentro un `flushSync`,
 * prima di leggere il documento. L'editor del codice di una parte lo usa già
 * (R14-3); questo lo dà ai campi di testo, che si registrano con la stessa
 * funzione e scrivono quello che hanno in mano **senza** togliere il fuoco: un
 * Ctrl+S non deve portar via il cursore a chi sta scrivendo.
 */
import { useEffect, useRef } from "react";

import { useAzioniStudio } from "./contesto";

/**
 * Registra `applica` fra le cose da scrivere prima di salvare o chiudere.
 *
 * `applica` è la **stessa** funzione che il campo chiama uscendo, e deve essere
 * idempotente: chiamata due volte di fila — una da qui, una dall'uscita vera
 * dal campo — scrive una volta sola, perché confronta il testo in mano con
 * quello del documento e il secondo giro li trova uguali.
 *
 * Si legge sempre l'ultima versione di `applica`, quella dell'ultimo disegno:
 * il testo in mano cambia a ogni battuta, e un riferimento preso al montaggio
 * scriverebbe quello di quando il campo è nato.
 */
export function useScriviAllaChiusura(applica: () => void): void {
  // Le sole azioni: `registraSospeso` non cambia, e un campo che si registra non deve
  // ridisegnarsi a ogni battuta dello Studio (`useStudio` dà anche lo stato).
  const { registraSospeso } = useAzioniStudio();
  const ultima = useRef(applica);
  ultima.current = applica;
  useEffect(() => registraSospeso(() => ultima.current()), [registraSospeso]);
}
