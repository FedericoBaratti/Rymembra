/**
 * «Salta al contenuto»: il fuoco sul posto della pagina, oltre la navigazione.
 *
 * Il posto del contenuto è il punto di riferimento `main` della finestra
 * (`Impaginazione.tsx`), e chi usa un lettore di schermo ci salta da sé. Chi
 * usa solo la tastiera no: dal primo Tab c'erano tredici fermate nella barra,
 * più una per playlist — con quaranta playlist, oltre cinquanta Tab a ogni
 * ritorno sulla pagina (R16-10).
 *
 * Un gesto e non un indirizzo: un `<a href="#contenuto">` sarebbe una
 * navigazione, e la guardia della finestra (`src-tauri/src/pagina.rs`) non
 * deve doversi chiedere se un frammento è dei nostri. Qui si sposta il fuoco e
 * basta.
 *
 * # Il `tabindex` solo per il salto
 *
 * Un `main` non prende il fuoco. Con un `tabIndex={-1}` fisso lo prenderebbe
 * anche a ogni clic su un punto vuoto della pagina, e più di un posto
 * dell'app legge «il fuoco è sul `<body>`» come «nessuno ha il fuoco» (le
 * finestrelle che lo restituiscono, l'Escape del pannello di «Ascolta
 * insieme», lo schermo intero). Quindi il `tabindex` si mette per il salto e
 * se ne va col fuoco: il Tab dopo parte da lì, e il resto non cambia.
 *
 * Senza un `main` non fa niente: una skin il cui scafale non ha il posto del
 * contenuto non ha dove saltare, e un fuoco mandato altrove sarebbe peggio.
 */

/** Del posto del contenuto, quel che serve al salto: `HTMLElement` va bene. */
export interface PostoDelContenuto {
  hasAttribute(nome: string): boolean;
  setAttribute(nome: string, valore: string): void;
  removeAttribute(nome: string): void;
  addEventListener(evento: "blur", fai: () => void, opzioni: { once: true }): void;
  focus(opzioni?: FocusOptions): void;
}

export function saltaAlContenuto(pagina: {
  querySelector(selettore: "main"): PostoDelContenuto | null;
}): boolean {
  const posto = pagina.querySelector("main");
  if (posto === null) return false;
  // Un `tabindex` che c'era già resta com'era, e non se ne va col fuoco.
  if (!posto.hasAttribute("tabindex")) {
    posto.setAttribute("tabindex", "-1");
    posto.addEventListener("blur", () => posto.removeAttribute("tabindex"), { once: true });
  }
  // Senza scorrere: il posto del contenuto sta già nella finestra, ed è lui
  // che scorre dentro di sé.
  posto.focus({ preventScroll: true });
  return true;
}
