/**
 * Due decisioni dell'editor a battute (`Sincronizza.tsx`), fuori da React
 * perché si possano provare.
 */

/** In quale dei cinque momenti si è. */
export type Momento = "righe" | "battute" | "rivedi" | "parole" | "dono";

/**
 * Chiudere adesso non butta via niente?
 *
 * Tre minuti di battute, e poi i ritocchi della revisione, vivono solo qui
 * finché non si salva. Un Escape premuto per riflesso — o «Annulla» in
 * revisione — smontava l'editor con tutto dentro, senza chiedere: il velo era
 * stato spento apposta («il velo qui non chiude»), Escape no. Con almeno una
 * battuta, di riga o di parola, prima si chiede. Nelle righe non c'è ancora
 * niente di battuto, e nel dono il lavoro è già salvato.
 */
export function chiusuraLibera(
  momento: Momento,
  tempi: readonly number[],
  tempiParola: readonly number[],
): boolean {
  if (momento === "righe" || momento === "dono") return true;
  return tempi.length === 0 && tempiParola.length === 0;
}

/**
 * Dove sta il fuoco quando arriva il tasto: su un tasto (o un altro
 * controllo che Spazio e Invio attivano), in un campo di testo, o altrove —
 * la finestrella, il documento.
 */
export type Bersaglio = "tasto" | "campo" | "radice";

/** Il bersaglio di un evento della tastiera, per [`gestoDelTasto`]. */
export function bersaglioDi(elemento: EventTarget | null): Bersaglio {
  if (!(elemento instanceof Element)) return "radice";
  if (elemento.closest("input, textarea, select, [contenteditable='true']")) return "campo";
  if (elemento.closest("button, a[href], [role='button'], [role='switch'], [role='radio']")) {
    return "tasto";
  }
  return "radice";
}

/**
 * Che cosa fa un tasto mentre si batte: `null` se non è dei nostri (passa
 * oltre), `"niente"` se lo si prende e lo si ignora.
 *
 * Durante il raddrizzamento — la pausa e la decodifica di tutto il brano, che
 * su un FLAC a 24 bit o su una share sono secondi — il tasto «Raddrizza» è
 * spento, e la tastiera deve esserlo con lui: un secondo Invio rilanciava una
 * decodifica intera, e un Backspace toglieva una battuta che la risposta,
 * calcolata sulle battute di prima, rimetteva al suo posto. Spazio si prende
 * lo stesso, perché le scorciatoie globali non lo leggano come «pausa».
 *
 * # Dove sta il fuoco (`su`)
 *
 * L'ascoltatore sta su `window`, in cattura, e decideva dal solo tasto: finite
 * le battute, Invio sul tasto «Salva» faceva «Raddrizza», a ogni Invio, e
 * Spazio diventava una battuta oltre la fine — «Salva», «Torna» e i ritocchi
 * non si raggiungevano da tastiera. Adesso:
 *
 * - finché ci sono battute da dare, Spazio batte **anche** su un tasto: un
 *   clic su «Suona» lascia il fuoco lì, e la battuta dopo deve segnare la
 *   riga, non mettere in pausa;
 * - finite le battute, Spazio e Invio su un tasto sono del tasto;
 * - Invio come «ho finito» vale solo fuori dai controlli;
 * - in un campo di testo i tre tasti sono del campo.
 */
export function gestoDelTasto(
  tasto: string,
  {
    raddrizzo,
    tutteBattute,
    su = "radice",
  }: { raddrizzo: boolean; tutteBattute: boolean; su?: Bersaglio },
): "batti" | "rifai" | "finisci" | "niente" | null {
  const nostro = tasto === " " || tasto === "Backspace" || tasto === "Enter";
  if (!nostro) return null;
  if (su === "campo") return null;
  if (raddrizzo) return "niente";
  if (tasto === "Backspace") return "rifai";
  if (tasto === " ") return tutteBattute && su === "tasto" ? null : "batti";
  return tutteBattute && su !== "tasto" ? "finisci" : null;
}
