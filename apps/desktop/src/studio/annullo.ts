/**
 * Chi risponde a un tasto dello Studio: il campo col fuoco o lo Studio.
 *
 * # Due regole, e non una
 *
 * Fino alla 3.2 ce n'era una sola (`siScrive`, in `Guscio.tsx`): ogni
 * `<input>`, `<select>` e `<textarea>` si teneva Ctrl+Z, con la ragione che
 * «il campo ha il suo annullo». Vero solo per i campi in cui si scrive. Un
 * cursore, una casella, un elenco o un selettore di colore non hanno nessuna
 * pila d'annullo, e il fuoco ci resta dopo il gesto: trascinare l'opacità e
 * premere Ctrl+Z non faceva niente — dopo il gesto più frequente dello Studio.
 *
 * - [`haAnnulloProprio`] decide Ctrl+Z e Ctrl+Y: se li tiene solo chi ha
 *   davvero un annullo di testo.
 * - [`prendeITasti`] decide i tasti singoli («f», «i», «1»–«4», Esc): li
 *   tiene ogni controllo che li usa da sé. In un `<select>` le lettere cercano
 *   fra le opzioni, e rubarle con `preventDefault` toglierebbe la tastiera a
 *   chi la usa.
 *
 * Funzioni pure su un [`Bersaglio`] e non sul DOM: si provano con
 * `node --test` (`test/annullo.test.ts`).
 */

/** Il bersaglio di un tasto, in quel che serve a decidere. */
export interface Bersaglio {
  /** `tagName`, in maiuscolo come lo dà il DOM. */
  tag: string;
  /** `type` di un `<input>`; `null` per gli altri. */
  tipo: string | null;
  /** `isContentEditable`. */
  modificabile: boolean;
}

/** I tipi di `<input>` in cui si scrive, e che quindi annullano da sé. */
const INPUT_DI_TESTO = new Set(["text", "search", "number", "email", "url", "password", "tel"]);

/** Il campo ha un annullo suo: Ctrl+Z e Ctrl+Y sono del campo. */
export function haAnnulloProprio(b: Bersaglio | null): boolean {
  if (b === null) return false;
  if (b.modificabile || b.tag === "TEXTAREA") return true;
  // `type` assente si legge "text" dal DOM; qui si accetta anche il vuoto.
  return b.tag === "INPUT" && INPUT_DI_TESTO.has((b.tipo ?? "text").toLowerCase() || "text");
}

/** Il controllo usa da sé i tasti senza modificatori: lettere, cifre, Esc. */
export function prendeITasti(b: Bersaglio | null): boolean {
  if (b === null) return false;
  return b.tag === "INPUT" || b.tag === "TEXTAREA" || b.tag === "SELECT" || b.modificabile;
}

/**
 * Chi disfa un Ctrl+Z dato nell'editor del codice di una parte.
 *
 * Quel che si batte lì arriva al documento mezzo secondo dopo l'ultima
 * battuta (`CodiceParte`, in `IspezioneParte.tsx`): finché è in attesa vive
 * solo nell'editor, e lo Studio non lo sa. Fino alla 3.2 Ctrl+Z andava
 * comunque allo Studio, che disfaceva il passo **prima** — un'altra parte,
 * magari — e allo scadere del timer la battuta si scriveva sul documento
 * annullato, svuotando il «ripeti». Con una battuta in attesa l'annullo è
 * suo: si butta, e la storia non si tocca.
 */
export function chiAnnulla(testoLocale: string, scritto: string): "locale" | "studio" {
  return testoLocale !== scritto ? "locale" : "studio";
}

/**
 * Chi rifà un Ctrl+Y dato nell'editor del codice di una parte.
 *
 * Nessuno, finché c'è una battuta in attesa: rifare un passo sotto di lei la
 * farebbe scrivere su un documento diverso da quello su cui è stata battuta,
 * e il suo passo svuoterebbe comunque il «ripeti». Buttarla, come fa
 * l'annullo, sarebbe perdere quel che nessuno ha chiesto di perdere.
 */
export function chiRipete(testoLocale: string, scritto: string): "nessuno" | "studio" {
  return testoLocale !== scritto ? "nessuno" : "studio";
}
