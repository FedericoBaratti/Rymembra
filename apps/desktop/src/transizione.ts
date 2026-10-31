/**
 * Il passaggio da una vista all'altra.
 *
 * # Chi anima
 *
 * La skin, e React. `motion.route` è un campo del documento — `plain.json`
 * dichiara «esci sfumando e rimpicciolendo di otto millesimi, entra sfumando e
 * salendo di otto pixel» — e il compilatore lo scrive sulle due fotografie della
 * pagina: `::view-transition-old(.vt-page-content-exit)` e
 * `::view-transition-new(.vt-page-content-enter)`. Chi mette quelle due classi
 * sul contenitore della pagina è `PresenzaPagina`, in `presenza.tsx`, e lo fa
 * solo sulle transizioni di tipo `rotta`: quelle che partono da qui.
 *
 * Fino alla 2.x questo modulo chiamava `document.startViewTransition` a mano,
 * con un `flushSync` dentro, e animava la radice intera: la navigazione, il
 * lettore e la colonna dovevano chiamarsi fuori uno per uno con un
 * `view-transition-name` nel foglio, altrimenti salivano di otto pixel insieme
 * alla pagina. Con `<ViewTransition>` si fotografa solo la pagina, e tutto il
 * resto sta fermo senza doverlo dire.
 *
 * # Perché `startTransition`
 *
 * Perché è l'unico aggiornamento su cui React accende le transizioni di vista:
 * fotografa prima, applica il cambio, fotografa dopo, e anima la differenza.
 * Un `setState` qualunque cambia la pagina di colpo, come prima.
 *
 * Quel che non deve aspettare resta fuori. Il campo di ricerca è il caso: la
 * sua `value` è controllata, e un aggiornamento in transizione può arrivare un
 * disegno dopo il tasto — il cursore salterebbe in fondo mentre si scrive.
 * Chi svuota la ricerca cambiando vista lo fa prima, fuori da qui.
 */

import { addTransitionType, startTransition } from "react";

/**
 * Il tipo delle transizioni di cambio di vista.
 *
 * È la chiave con cui `PresenzaPagina` decide di animare: un aggiornamento
 * della pagina che non porta questo tipo — un filtro, una pagina di risultati
 * in più — non è un cambio di pagina.
 */
export const ROTTA = "rotta";

/**
 * Niente movimento: lo dice il sistema, o lo dice la skin.
 *
 * Esportata perché non serve solo alle transizioni di vista: il pannello del
 * testo insegue la riga accesa scorrendo, e scorrere è movimento quanto una
 * dissolvenza. Chiedere due volte la stessa cosa in due modi diversi vorrebbe
 * dire che un giorno una delle due dimentica `--motion-scale`.
 */
export function fermoRestando(): boolean {
  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return true;
  const scala = getComputedStyle(document.documentElement)
    .getPropertyValue("--motion-scale")
    .trim();
  // Assente vuol dire «la skin non si è espressa», che non è «ferma».
  return scala !== "" && Number(scala) === 0;
}

/**
 * Come scorrere, quando lo scorrimento lo fa il programma.
 *
 * `scrollTo` e `scrollIntoView` con `behavior: "smooth"` sono movimento quanto
 * una transizione, e il foglio non li ferma: la media query di `stile/movimento.css`
 * azzera le animazioni, non gli scorrimenti chiesti da JavaScript. Chi ha
 * chiesto meno movimento — al sistema, o nelle impostazioni, che arrivano qui
 * attraverso `--motion-scale` — vede quindi un salto invece di una scivolata.
 *
 * `fermo` si legge adesso di serie; è un argomento perché la scelta si provi
 * senza un documento.
 */
export function comeScorrere(
  voluto: ScrollBehavior,
  fermo: boolean = fermoRestando(),
): ScrollBehavior {
  return fermo && voluto === "smooth" ? "auto" : voluto;
}

/**
 * Cambia vista, animando il passaggio se la skin lo chiede.
 *
 * Il ripiego è il cambio istantaneo, e non è un caso d'errore: è quel che
 * succede con il movimento spento, dove una fotografia che non si anima
 * sarebbe lavoro fatto per niente. Saltare una transizione perché ne è
 * cominciata un'altra — due voci della barra premute in fretta — lo gestisce
 * React, e non arriva al diario.
 */
export function cambiandoVista(cambia: () => void): void {
  if (fermoRestando()) {
    cambia();
    return;
  }
  startTransition(() => {
    addTransitionType(ROTTA);
    cambia();
  });
}
