/**
 * Le cartelle sorvegliate come elenco: che cosa diventa dopo un gesto, e la
 * fila in cui i gesti aspettano il loro turno.
 *
 * # Il difetto
 *
 * Togliere una cartella voleva dire scrivere al nucleo «tutte quelle di prima
 * meno questa», e «quelle di prima» erano le cartelle di `avvio` del disegno in
 * cui si era premuto. Togliendone due in fila, la seconda pressione partiva
 * mentre la prima era ancora in volo, leggeva lo stesso elenco della prima — in
 * cui la prima cartella c'era ancora — e la riscriveva: la cartella appena tolta
 * tornava. Lo stesso per due cartelle trascinate sulla finestra una dopo
 * l'altra, o per un trascinamento mentre si toglieva.
 *
 * # La regola
 *
 * Due cose insieme, e nessuna basta da sola. Ogni gesto calcola l'elenco nuovo
 * da quello **vero** al momento in cui parte, non da quello del disegno —
 * `cartelleSorvegliate.ts` lo legge dall'archivio della libreria, che la
 * risposta del nucleo aggiorna subito. E i gesti partono uno alla volta, in
 * fila: senza la fila, il secondo partirebbe comunque prima che la risposta
 * del primo esista, e leggerebbe il vero di un istante prima.
 *
 * Qui non c'è né React né il nucleo, e si prova da solo
 * (`test/elencoCartelle.test.ts`).
 */
import type { CartellaGemella } from "./ipc";

/** L'elenco con dei percorsi in più, senza doppioni, nell'ordine in cui arrivano. */
export function conCartelle(
  cartelle: readonly string[],
  nuove: readonly string[],
): string[] {
  return [...new Set([...cartelle, ...nuove])];
}

/** L'elenco senza quel percorso. */
export function senzaCartella(cartelle: readonly string[], percorso: string): string[] {
  return cartelle.filter((c) => c !== percorso);
}

/**
 * L'elenco dopo il «sì» alla domanda sul trasloco: la cartella che c'era
 * prende la grafia nuova, e quella scelta si aggiunge in fondo.
 *
 * Vedi `trasloca` in `cartelleSorvegliate.ts` per quando serve.
 */
export function traslocate(cartelle: readonly string[], quale: CartellaGemella): string[] {
  return conCartelle(
    cartelle.map((c) => (c === quale.esistente ? quale.a : c)),
    [quale.nuova],
  );
}

/** Mette in fila un'operazione, e restituisce il suo esito quando tocca a lei. */
export type Fila = <T>(operazione: () => Promise<T>) => Promise<T>;

/**
 * Una fila di operazioni: ognuna parte quando la precedente è finita.
 *
 * Finita bene **o male**: un'operazione fallita non deve tenere ferme quelle
 * dietro di lei, e il suo errore torna a chi l'aveva chiesta, non alla fila.
 *
 * `avvisa` riceve quante ce ne sono, in corso e in attesa, a ogni cambio: è
 * quel che spegne i tasti mentre si lavora.
 */
export function creaFila(avvisa: (quante: number) => void = () => undefined): Fila {
  let coda: Promise<unknown> = Promise.resolve();
  let quante = 0;
  return <T>(operazione: () => Promise<T>): Promise<T> => {
    quante += 1;
    avvisa(quante);
    const fatto = coda.then(operazione);
    coda = fatto.catch(() => undefined);
    return fatto.finally(() => {
      quante -= 1;
      avvisa(quante);
    });
  };
}
