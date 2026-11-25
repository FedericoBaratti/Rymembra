/**
 * Il pannello del testo prima che il testo ci sia: che cosa dire.
 *
 * Un modulo a parte perché è puro e si prova con `node --test`: `Testo.tsx`
 * porta con sé `ipc.ts` e le lingue, che fuori da Vite non si caricano.
 */

/**
 * - `leggo`: `testoBrano` non ha ancora risposto. Legge il database e il
 *   `.lrc` accanto al brano, e su una condivisione che non risponde può
 *   volerci decine di secondi (`testi.rs`): il pannello lo dice — «Cerco il
 *   testo…» — invece di restare un riquadro vuoto con l'occhiello.
 * - `illeggibile`: la lettura è fallita. Il guasto va alla fascia, come prima,
 *   e qui si dice che il testo non si è potuto leggere; **senza** «Riprova»,
 *   per la scelta scritta accanto alla lettura in `Testo.tsx` — un database che
 *   non risponde non è una cosa da riprovare dal pannello.
 * - `pronto`: il testo c'è, e decidono i rami di sempre.
 * - `niente`: nessuna lettura in corso e nessun testo — una lettura superata
 *   da un cambio di brano, per l'istante prima che parta la nuova.
 */
export type FaseLettura = "leggo" | "illeggibile" | "pronto" | "niente";

export function faseLettura(s: {
  testo: object | null;
  leggendo: boolean;
  illeggibile: boolean;
}): FaseLettura {
  if (s.testo !== null) return "pronto";
  if (s.leggendo) return "leggo";
  return s.illeggibile ? "illeggibile" : "niente";
}

/**
 * Il testo che si ha è uno che «Non è questo» e «Altri testi» possono sostituire?
 *
 * Quello del catalogo (`lrclib`), che l'ha scelto il programma, e la copia del
 * testo del proprietario del brano (`proprietario`): per chi ascolta qui è una
 * copia, e un rifiuto — o una voce scelta al suo posto — vale finché lui manda
 * lo stesso testo (`testi::catalogo::rifiuta`). Il testo di chi ascolta (`mano`),
 * di un sidecar o di un tag non si sostituisce da qui: vincerebbe comunque.
 */
export function sostituibileDaQui(fonte: string): boolean {
  return fonte === "lrclib" || fonte === "proprietario";
}
