/**
 * Quale tasto grande mostra il primo avvio: «Continua» o «Ascolta».
 *
 * # Perché non bastano i brani
 *
 * «Ascolta» sostituisce «Continua» appena la libreria ha qualcosa dentro (vedi
 * `Primo.tsx`): è il momento per cui esiste. Ma i brani possono arrivare anche
 * **prima** di «Continua»: una cartella del cloud scelta con «Aggiungi
 * un'altra cartella» il nucleo la scrive e la legge subito
 * (`nuvolaAggiungi`), e finita quella lettura la libreria non è più vuota.
 * Con la sola regola `brani > 0` «Continua» spariva in quel momento, e le
 * cartelle del sistema mostrate spuntate — mai scritte — non entravano in
 * libreria senza che niente lo dicesse: `onCartelle` si raggiunge solo da
 * «Continua».
 *
 * Quindi «Ascolta» vuole i brani **e** le scelte scritte: o «Continua» è
 * andato a buon fine, o ogni cartella spuntata sta già fra le sorvegliate.
 */

export type TastoPrimo = "continua" | "ascolta";

export interface StatoPrimo {
  /** Quanti brani ci sono in libreria adesso. */
  brani: number;
  /** Le cartelle spuntate nell'elenco. */
  spuntate: readonly string[];
  /** Le cartelle già fra le sorvegliate, come le dice il nucleo. */
  scritte: readonly string[];
  /** «Continua» è andato a buon fine almeno una volta. */
  confermato: boolean;
}

export function tastoDelPrimo({ brani, spuntate, scritte, confermato }: StatoPrimo): TastoPrimo {
  if (brani <= 0) return "continua";
  if (confermato) return "ascolta";
  const daScrivere = spuntate.some((p) => !scritte.includes(p));
  return daScrivere ? "continua" : "ascolta";
}
