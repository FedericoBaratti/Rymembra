/**
 * La notizia di un ripristino finito dopo che la sua finestrella è stata chiusa.
 *
 * # Perché serve
 *
 * Durante il ripristino Escape chiude ancora la finestrella, ed è voluto: un
 * download da Drive che s'impianta non deve chiudere dentro nessuno finché il
 * nucleo non risponde. Ma il lavoro continua, e la risposta cadeva su un
 * componente smontato: il rapporto — i file che non c'erano, l'invito a
 * scansionare le cartelle tornate — e l'errore non arrivavano a nessuno.
 * L'errore passa alla fascia della finestra; il rapporto diventa questa
 * notizia, con le sole cose che chi ha chiuso deve sapere.
 */

/** Una frase della notizia: una chiave del catalogo, col numero se ne ha uno. */
export interface PezzoNotizia {
  chiave: "restore.done.notice" | "restore.done.notice.folders" | "restore.done.notice.missing";
  n?: number;
}

export function notiziaDelRipristino(esito: {
  cartelle: number;
  mancanti: readonly string[];
}): PezzoNotizia[] {
  const pezzi: PezzoNotizia[] = [{ chiave: "restore.done.notice" }];
  if (esito.cartelle > 0) pezzi.push({ chiave: "restore.done.notice.folders" });
  if (esito.mancanti.length > 0) {
    pezzi.push({ chiave: "restore.done.notice.missing", n: esito.mancanti.length });
  }
  return pezzi;
}
