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
 * Che cosa fa un tasto mentre si batte: `null` se non è dei nostri (passa
 * oltre), `"niente"` se lo si prende e lo si ignora.
 *
 * Durante il raddrizzamento — la pausa e la decodifica di tutto il brano, che
 * su un FLAC a 24 bit o su una share sono secondi — il tasto «Raddrizza» è
 * spento, e la tastiera deve esserlo con lui: un secondo Invio rilanciava una
 * decodifica intera, e un Backspace toglieva una battuta che la risposta,
 * calcolata sulle battute di prima, rimetteva al suo posto. Spazio si prende
 * lo stesso, perché le scorciatoie globali non lo leggano come «pausa».
 */
export function gestoDelTasto(
  tasto: string,
  { raddrizzo, tutteBattute }: { raddrizzo: boolean; tutteBattute: boolean },
): "batti" | "rifai" | "finisci" | "niente" | null {
  const nostro = tasto === " " || tasto === "Backspace" || tasto === "Enter";
  if (!nostro) return null;
  if (raddrizzo) return "niente";
  if (tasto === " ") return "batti";
  if (tasto === "Backspace") return "rifai";
  return tutteBattute ? "finisci" : null;
}
