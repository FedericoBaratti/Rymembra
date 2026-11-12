/**
 * Il browser dentro la finestra, spento: il menù di Edge e le sue scorciatoie.
 *
 * # Perché qui e non nel nucleo
 *
 * WebView2 nasce con il menù contestuale del browser e con le sue scorciatoie
 * accesi, wry li lascia così e Tauri non li espone. Spegnerli dal nucleo
 * vorrebbe dire `ICoreWebView2Settings` attraverso `with_webview`, cioè COM, cioè
 * `unsafe`, che il workspace vieta. La pagina invece li può prendere prima del
 * browser: un `preventDefault()` sul `contextmenu` non apre il menù, e uno sul
 * `keydown` di un tasto del browser non lo fa arrivare al browser.
 *
 * # Che cosa si perdeva
 *
 * Un clic destro su una parte della finestra senza un menù suo — lo sfondo
 * dell'intestazione, uno spazio vuoto della colonna — apriva il menù di Edge:
 * Indietro, **Aggiorna**, **Salva con nome**, **Stampa**. Aggiorna, come F5 e
 * Ctrl+R, ricaricava tutta l'interfaccia: i dialoghi aperti e il testo non
 * salvato dello Studio se ne andavano. Salva con nome scriveva su disco
 * un'istantanea della pagina con i titoli e le cartelle della libreria. Ctrl+F
 * apriva la barra di ricerca di Edge dentro Rymembra, Ctrl+P la stampa.
 *
 * # La regola (decisione D-43)
 *
 * Il menù di sistema resta **solo nei campi di testo**, dove Taglia, Copia e
 * Incolla servono. Le scorciatoie del browser sono spente ovunque, campi
 * compresi: nessuna di loro modifica il testo, e quelle che lo fanno — Ctrl+C,
 * Ctrl+V, Ctrl+A, Ctrl+Z — non sono del browser e restano. Le scorciatoie
 * dell'app non cambiano: `preventDefault` toglie al browser il suo gesto, non
 * l'evento a chi lo ascolta (`tastiera.ts` non guarda `defaultPrevented`), e
 * `Ctrl+F` continua ad aprire la ricerca di Rymembra.
 *
 * Restano fuori lo zoom (lo fa l'app, `zoom.rs`, e WebView2 l'ha già spento) e
 * gli strumenti di sviluppo (F12, Ctrl+Maiusc+I): in rilascio non ci sono, e in
 * sviluppo servono.
 *
 * Solo sul computer: sul telefono il menù di Edge non c'è, e `vibra.ts` vibra a
 * ogni `contextmenu` preso — spegnerlo lì farebbe vibrare ogni tocco lungo sul
 * vuoto. Il documento della tela dello Studio è un `<iframe>` con i suoi
 * ascolti (`studio/Tela.tsx`, che il `contextmenu` lo ferma già da sé).
 */

/** Un tasto premuto, quel che serve a riconoscerlo: un `KeyboardEvent` lo è. */
export interface TastoPremuto {
  key: string;
  code: string;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  metaKey: boolean;
}

/**
 * Le lettere che con `Ctrl` sono del browser: trova (F), trova il successivo
 * (G), stampa (P), ricarica (R), salva con nome (S), sorgente della pagina (U).
 * Con o senza `Maiusc`: `Ctrl+Maiusc+R` ricarica scavalcando la cache,
 * `Ctrl+Maiusc+G` è il precedente.
 */
const LETTERE_DEL_BROWSER = new Set(["f", "g", "p", "r", "s", "u"]);

/**
 * I tasti funzione del browser, con qualunque modificatore: F3 trova il
 * successivo (Maiusc+F3 il precedente), F5 ricarica (anche con Ctrl o Maiusc),
 * F7 accende la navigazione col cursore.
 */
const FUNZIONI_DEL_BROWSER = new Set(["F3", "F5", "F7"]);

/** I tasti dedicati delle tastiere multimediali che parlano al browser. */
const TASTI_DEL_BROWSER = new Set([
  "BrowserBack",
  "BrowserForward",
  "BrowserRefresh",
  "BrowserSearch",
  "BrowserHome",
  "BrowserFavorites",
  "BrowserStop",
]);

/**
 * La lettera di un tasto per il browser: il carattere, oppure il tasto su cui
 * sta. Il browser riconosce le sue scorciatoie dal tasto, non dal carattere: su
 * una tastiera russa `Ctrl+F` arriva come `Ctrl+а`, e apre lo stesso la barra di
 * ricerca.
 */
function lettere(tasto: TastoPremuto): string[] {
  const dalTasto = /^Key([A-Z])$/.exec(tasto.code)?.[1]?.toLowerCase();
  return dalTasto === undefined ? [tasto.key.toLowerCase()] : [tasto.key.toLowerCase(), dalTasto];
}

/** È una scorciatoia del browser, che non deve arrivare al browser? */
export function eTastoDelBrowser(tasto: TastoPremuto): boolean {
  if (FUNZIONI_DEL_BROWSER.has(tasto.key) || TASTI_DEL_BROWSER.has(tasto.key)) return true;
  // `Alt+←` e `Alt+→`: la cronologia della pagina, che qui è una sola.
  // `tastiera.ts` usa `Alt+←` per tornare indietro di un livello, e continua a
  // farlo: qui si spegne soltanto il gesto del browser.
  if (
    tasto.altKey &&
    !tasto.ctrlKey &&
    !tasto.metaKey &&
    (tasto.key === "ArrowLeft" || tasto.key === "ArrowRight")
  ) {
    return true;
  }
  return (
    tasto.ctrlKey &&
    !tasto.altKey &&
    !tasto.metaKey &&
    lettere(tasto).some((lettera) => LETTERE_DEL_BROWSER.has(lettera))
  );
}

/** Dove è stato chiesto il menù, quel che serve a dire se è un campo di testo. */
export interface Bersaglio {
  tagName: string;
  type?: string | undefined;
  isContentEditable?: boolean | undefined;
}

/**
 * Gli `<input>` in cui si scrive: di serie è «text», e un tipo vuoto vale
 * quello. Una casella, un cursore o un pallino sono `input` ma non hanno niente
 * da tagliare o incollare.
 */
const INPUT_DI_TESTO = new Set(["", "text", "search", "email", "url", "tel", "password", "number"]);

/** Il menù di sistema si può aprire qui? Solo dentro un campo di testo. */
export function menuDelSistemaAmmesso(bersaglio: Bersaglio | null): boolean {
  if (bersaglio === null) return false;
  if (bersaglio.isContentEditable === true) return true;
  if (bersaglio.tagName === "TEXTAREA") return true;
  return bersaglio.tagName === "INPUT" && INPUT_DI_TESTO.has((bersaglio.type ?? "").toLowerCase());
}

/** Il bersaglio di un evento, letto senza supporre che sia un elemento HTML. */
function bersaglioDi(nodo: EventTarget | null): Bersaglio | null {
  if (nodo === null || typeof nodo !== "object" || !("tagName" in nodo)) return null;
  const elemento = nodo as { tagName: unknown; type?: unknown; isContentEditable?: unknown };
  if (typeof elemento.tagName !== "string") return null;
  return {
    tagName: elemento.tagName,
    type: typeof elemento.type === "string" ? elemento.type : undefined,
    isContentEditable: elemento.isContentEditable === true,
  };
}

/**
 * Spegne menù e scorciatoie del browser nella finestra.
 *
 * # Il menù in cattura, i tasti in risalita
 *
 * Il menù **in cattura**: arriva prima di tutti, e chi apre un menù suo con il
 * tasto destro lo apre lo stesso (nessuno guarda `defaultPrevented` sul
 * `contextmenu`, se non `vibra.ts`, che qui non c'è). Dentro un campo non si
 * tocca, quindi l'editor di codice dello Studio — un `contenteditable` — tiene
 * il suo.
 *
 * I tasti **in risalita**, cioè per ultimi: l'editor di codice dello Studio
 * (CodeMirror) ignora un tasto già preso (`event.defaultPrevented`), e Ctrl+F,
 * F3, Ctrl+G, Ctrl+U e Alt+←/→ lì dentro sono suoi — la sua ricerca, il
 * prossimo, l'annullo della selezione, il cursore per sintassi. Presi in
 * cattura, sarebbero spariti. Chi ferma la propagazione di un tasto (`fuoco.ts`,
 * `finestrella.ts`, `Scorciatoie.tsx`) lo prende anche con `preventDefault`, e il
 * browser non lo vede comunque.
 */
export function spegniIlBrowser(finestra: Window): void {
  finestra.addEventListener(
    "contextmenu",
    (e) => {
      if (!menuDelSistemaAmmesso(bersaglioDi(e.target))) e.preventDefault();
    },
    true,
  );
  finestra.addEventListener("keydown", (e) => {
    if (eTastoDelBrowser(e)) e.preventDefault();
  });
}
