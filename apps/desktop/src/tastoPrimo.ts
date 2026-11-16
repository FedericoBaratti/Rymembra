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

import { contoDi, NOMI_SERVIZI, servizioDelConto } from "./cartelleNuvola";

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

/**
 * Il servizio del cloud che sta ancora elencando una delle cartelle scritte, o
 * `null`: il suo nome («OneDrive»), per dire «sta ancora leggendo».
 *
 * Una cartella del cloud si legge dallo specchio dei file del servizio, e la
 * prima lettura di un drive grande dura un minuto: finché il conto è in
 * `"elenco"` i brani stanno arrivando, e una scansione finita a zero non
 * dice «vuota». Contano i conti delle cartelle **scritte**: un altro conto che
 * elenca non riguarda questa libreria.
 */
export function servizioInArrivo(
  scritte: readonly string[],
  conti: readonly { conto: string; stato: string }[] | null,
): string | null {
  if (conti === null) return null;
  for (const percorso of scritte) {
    const delConto = contoDi(percorso);
    if (delConto === null) continue;
    if (!conti.some((c) => c.conto === delConto && c.stato === "elenco")) continue;
    const servizio = servizioDelConto(delConto);
    return servizio === null ? null : NOMI_SERVIZI[servizio];
  }
  return null;
}

/**
 * La lettura è finita senza brani: «Rymembra non ha trovato brani».
 *
 * Non vale mentre un servizio del cloud sta ancora arrivando (`nuvolaInArrivo`,
 * da [`servizioInArrivo`]): la camminata sullo specchio ancora vuoto torna con
 * zero brani, e dirlo vuoto, con «controlla di aver scelto le cartelle giuste»,
 * sarebbe falso (R12-9).
 */
export function lettaVuota(stato: {
  scansionando: boolean;
  inCorso: boolean;
  /** Una lettura è finita, e ha un esito. */
  haEsito: boolean;
  brani: number;
  nuvolaInArrivo: string | null;
}): boolean {
  return (
    !stato.scansionando &&
    !stato.inCorso &&
    stato.haEsito &&
    stato.brani === 0 &&
    stato.nuvolaInArrivo === null
  );
}
