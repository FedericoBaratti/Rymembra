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
