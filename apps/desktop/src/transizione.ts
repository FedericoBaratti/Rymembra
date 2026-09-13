/**
 * Il passaggio da una vista all'altra.
 *
 * # Perché non c'è nessuna animazione scritta qui
 *
 * Perché è già scritta, e non da noi. `motion.routeTransition` è un campo del
 * documento skin — `plain.json` dichiara «esci sfumando e rimpicciolendo di otto
 * millesimi, entra sfumando e salendo di otto pixel» — e il compilatore ne emette
 * da sempre le regole complete: due `@keyframes` e i due selettori
 * `::view-transition-old(root)` / `::view-transition-new(root)`
 * (`core/aether-skin/src/compile.rs`, `compila_movimento`).
 *
 * Mancava una riga sola: **nessuno chiamava `startViewTransition`**, quindi il
 * motore non produceva mai le due pseudo-elemento su cui quelle regole
 * agiscono. Era una funzione dichiarata, compilata, spedita nel foglio e mai
 * eseguita — lo stesso difetto di `--motion-scale` prima che qualcuno lo
 * leggesse, e con lo stesso rimedio: non aggiungere una funzione, collegare
 * quella che c'è.
 *
 * # `flushSync`, e perché non se ne può fare a meno
 *
 * `startViewTransition` fotografa la pagina, esegue il callback, rifotografa e
 * anima la differenza. Il callback deve quindi cambiare il DOM **prima di
 * ritornare**, e un `setState` di React non lo fa: mette in coda un rendering
 * per dopo. Senza `flushSync` la seconda fotografia sarebbe identica alla prima
 * e non si vedrebbe niente — o peggio, la transizione resterebbe aperta finché
 * non scade.
 *
 * # Chi vince sul movimento
 *
 * Due porte, e tutte e due chiudono:
 *
 * - `prefers-reduced-motion` del sistema, che è una condizione di chi guarda e
 *   non una preferenza estetica;
 * - `--motion-scale`, che è l'intensità dichiarata dalla skin — e con lei, da
 *   quando `applicaMovimento` scrive `data-motion-utente` sulla radice, quella
 *   chiesta da chi guarda: il foglio azzera la scala sotto quel selettore, e
 *   qui si legge il risultato senza sapere chi dei due l'ha deciso.
 *
 * # Perché `--motion-scale` si legge ancora a mano
 *
 * Non più perché il compilatore la dimenticava. Le due regole
 * `::view-transition-*` scrivevano davvero `var(--dur-2)` nudo, ed era il
 * motivo originale di questa lettura; adesso passano da `durata_scalata` come
 * ogni altra durata del foglio (`compile.rs`, `compila_movimento`), quindi una
 * skin a `none` non anima più nemmeno senza di noi.
 *
 * Restano due ragioni, e sono quelle vere:
 *
 * - **una durata a zero non è una transizione che non parte.**
 *   `startViewTransition` fotografa la finestra, esegue il callback, la
 *   rifotografa e tiene i due pseudo-elementi finché l'animazione non finisce.
 *   A scala zero quel lavoro si fa tutto per non mostrare niente: chi ha
 *   chiesto meno movimento merita il cambio istantaneo, non lo stesso costo
 *   senza l'immagine.
 * - **questa funzione non serve solo alle transizioni di vista.** Il pannello
 *   del testo insegue la riga accesa scorrendo, e uno scorrimento lo decide
 *   JavaScript: nessun `calc()` del foglio lo può fermare. È scritto anche
 *   sopra `fermoRestando`, ed è la ragione per cui è esportata.
 */

import { flushSync } from "react-dom";

/** Il motore sa fare le transizioni di vista? */
interface ConTransizione {
  startViewTransition?: (aggiorna: () => void) => unknown;
}

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
 * Cambia vista, animando il passaggio se si può.
 *
 * Il ripiego non è un caso d'errore: è il comportamento di prima, cioè il
 * cambio istantaneo. Chi non ha l'API — o non la vuole — vede l'applicazione
 * esattamente come la vedeva.
 */
export function cambiandoVista(cambia: () => void): void {
  const documento = document as Document & ConTransizione;
  if (typeof documento.startViewTransition !== "function" || fermoRestando()) {
    cambia();
    return;
  }
  documento.startViewTransition(() => {
    flushSync(cambia);
  });
}
