/**
 * Dove va il fuoco quando la pagina cambia sotto di lui: aprendo un album o
 * un artista, chiudendoli, e lasciando «In riproduzione».
 *
 * # Il difetto
 *
 * Il cambio di vista smontava la scheda premuta, e nessuno decideva dove
 * dovesse andare il fuoco: cadeva sul `<body>`. Aprendo un disco un lettore di
 * schermo non diceva niente — né il titolo, né un cambio di pagina — e dopo
 * Escape il Tab saltava la griglia per finire nella terza colonna. Chiudendo
 * «In riproduzione» il fuoco doveva tornare al tasto che l'aveva aperta, ma
 * quel tasto stava nella terza colonna, che lo schermo intero aveva smontato.
 *
 * # La regola
 *
 * - **Aprendo**, il fuoco va sulla testata della pagina nuova (D-73): il suo
 *   `h1`, che lo prende senza diventare una fermata del Tab (`tabIndex = -1`).
 *   Si sente il titolo del disco, e il primo Tab porta ai suoi tasti.
 * - **Chiudendo**, torna a chi aveva aperto, se c'è ancora e lo prende
 *   davvero; altrimenti alla scheda dove la griglia si era fermata (quella col
 *   `tabindex="0"`, la sua fermata del Tab); altrimenti alla testata della
 *   pagina che resta.
 *
 * Con nodi finti nelle prove: qui c'è solo quel che serve di un elemento, e
 * niente che `node --test` non sappia caricare.
 */

/** Quel che serve di un elemento per dargli il fuoco. */
export interface Fuocabile {
  focus(opzioni?: { preventScroll?: boolean }): void;
  readonly isConnected: boolean;
  tabIndex: number;
  hasAttribute(nome: string): boolean;
}

/** Dove cercare: il contenitore della pagina. */
export interface Contenitore {
  querySelector(selettore: string): Fuocabile | null;
}

/** La scheda dove la griglia si è fermata: la sua sola fermata del Tab. */
export const SCHEDA_FERMA = '.scheda[tabindex="0"]';

/** Prova a dare il fuoco a `chi`: `true` se l'ha preso davvero. */
function prova(chi: Fuocabile | null, attivo: () => unknown): boolean {
  if (chi === null || !chi.isConnected) return false;
  chi.focus({ preventScroll: true });
  // Un nodo montato ma nascosto (una pagina tenuta da parte) non lo prende,
  // e `focus()` non lo dice.
  return attivo() === chi;
}

/** Il fuoco sulla testata della pagina: il suo `h1`. `true` se c'era. */
export function fuocoSullaTestata(
  contenitore: Contenitore | null,
  attivo: () => unknown,
): boolean {
  const testata = contenitore?.querySelector("h1") ?? null;
  if (testata === null) return false;
  // Prende il fuoco senza diventare una fermata del Tab.
  if (!testata.hasAttribute("tabindex")) testata.tabIndex = -1;
  return prova(testata, attivo);
}

/**
 * Il fuoco che torna chiudendo: al primo dei `candidati` che c'è ancora e lo
 * prende davvero, e se nessuno lo prende alla testata della pagina. Vedi la
 * nota del modulo: chiudendo un album i candidati sono chi l'aveva aperto e
 * la scheda dove la griglia si era fermata (`SCHEDA_FERMA`).
 */
export function fuocoDiRitorno(
  candidati: readonly (Fuocabile | null)[],
  contenitore: Contenitore | null,
  attivo: () => unknown,
): void {
  for (const chi of candidati) if (prova(chi, attivo)) return;
  fuocoSullaTestata(contenitore, attivo);
}
