/**
 * Il confronto fra due versioni dello stesso documento, riga per riga.
 *
 * # Perché riga per riga e non un confronto vero
 *
 * Un confronto con le mosse e i blocchi sarebbe una dipendenza in più per
 * rispondere a una domanda sola — «cosa cambia» — e il documento è testo
 * apposta perché quando la domanda diventa più grande ci sia già `git diff`.
 *
 * # Perché sta qui e non dentro `Documento`
 *
 * Perché adesso la stessa domanda se la fanno in due, e sono due domande
 * diverse fatte allo stesso modo: la vista Documento confronta quel che si sta
 * scrivendo con la skin installata — «cosa ho cambiato» — e la chat confronta
 * la sorgente con il **candidato**, cioè il testo che si otterrebbe applicando
 * quel che il modello propone — «cosa cambierebbe». Due implementazioni della
 * stessa funzione darebbero due letture diverse dello stesso confronto, e
 * quella della chat è l'unica delle due che si guarda **prima** di decidere.
 */

/** Una riga del confronto. */
export type Riga = {
  /** Uguale, aggiunta, tolta. */
  segno: "=" | "+" | "-";
  testo: string;
};

/**
 * Le righe del confronto fra `prima` e `dopo`.
 *
 * L'allineamento è posizionale: la riga *n* dell'uno si confronta con la riga
 * *n* dell'altro. Su un documento riscritto da `JSON.stringify` — che è quel
 * che fa `patch.ts` a ogni modifica — l'ordine delle chiavi è stabile e
 * l'allineamento tiene; su due testi riordinati a mano no, e allora il
 * confronto mostra più righe di quante ne siano davvero cambiate. È il limite
 * accettato di un confronto senza mosse, e si vede solo dove `git diff` sarebbe
 * comunque lo strumento giusto.
 */
export function differenze(prima: string, dopo: string): Riga[] {
  const a = prima.split("\n");
  const b = dopo.split("\n");
  const quante = Math.max(a.length, b.length);
  const righe: Riga[] = [];
  for (let i = 0; i < quante; i += 1) {
    const vecchia = a[i];
    const nuova = b[i];
    if (vecchia === nuova) {
      if (vecchia !== undefined) righe.push({ segno: "=", testo: vecchia });
      continue;
    }
    if (vecchia !== undefined) righe.push({ segno: "-", testo: vecchia });
    if (nuova !== undefined) righe.push({ segno: "+", testo: nuova });
  }
  return righe;
}

/** Solo le righe che cambiano, con quante ne sono. */
export function quanteCambiano(righe: readonly Riga[]): {
  aggiunte: number;
  tolte: number;
} {
  let aggiunte = 0;
  let tolte = 0;
  for (const riga of righe) {
    if (riga.segno === "+") aggiunte += 1;
    else if (riga.segno === "-") tolte += 1;
  }
  return { aggiunte, tolte };
}
