/**
 * La notizia di «Dimentica l'arricchimento», da quel che il nucleo ha fatto.
 *
 * # Perché serve
 *
 * Il gesto rilegge un file per ogni brano arricchito, e un file che non
 * risponde — un NAS addormentato, un disco staccato — non si riporta: resta
 * com'era, con la sua annotazione, e il gesto si può ripetere. Il nucleo lo
 * conta (`falliti`) accanto ai brani tornati davvero (`riportati`), ma la
 * finestra prendeva soltanto lo stato: con la cartella spenta l'automatico si
 * spegneva, il tasto tornava a dire «(300 brani)» e nessuna riga diceva che
 * non era successo niente, né perché.
 *
 * # Una frase per numero
 *
 * `t` sceglie il plurale su `n` soltanto: una frase con due numeri scriverebbe
 * «1 brani» per l'uno o per l'altro. Qui due pezzi, ognuno col suo `n`. Senza
 * lingue e senza React, perché `node --test` la carichi da sola.
 */

/** Un pezzo della notizia: una chiave del catalogo, col suo numero. */
export interface PezzoAnnullamento {
  chiave: "settings.enrich.undone" | "settings.enrich.undoFailed";
  n: number;
}

export function notiziaDellAnnullamento(esito: {
  riportati: number;
  falliti: number;
}): PezzoAnnullamento[] {
  const pezzi: PezzoAnnullamento[] = [];
  if (esito.riportati > 0) pezzi.push({ chiave: "settings.enrich.undone", n: esito.riportati });
  if (esito.falliti > 0) pezzi.push({ chiave: "settings.enrich.undoFailed", n: esito.falliti });
  return pezzi;
}
