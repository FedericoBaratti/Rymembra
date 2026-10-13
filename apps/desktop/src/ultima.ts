/**
 * Vince l'ultima: fra due risposte alla stessa domanda, conta quella chiesta
 * per ultima — non quella arrivata per ultima.
 *
 * # Il difetto
 *
 * La libreria si rilegge in risposta agli eventi: `cartelle:cambiate` alla fine
 * di ogni scansione, `scarico:in_libreria` quando entrano dei brani scaricati,
 * e a mano dopo ogni gesto che cambia le cartelle. Due di questi a pochi
 * istanti l'uno dall'altro fanno due richieste, e il nucleo non promette di
 * rispondere nell'ordine in cui sono partite: la prima può leggere mentre una
 * scansione scrive ancora, e arrivare **dopo** la seconda. Chi scrive la
 * risposta nello stato così com'è arriva, rimette a schermo la libreria di un
 * istante prima — conteggi vecchi, una cartella appena aggiunta che sparisce —
 * e ce la lascia fino al prossimo evento, che può non venire.
 *
 * # La regola
 *
 * Ogni domanda prende un turno; la sua risposta si scrive solo se nel frattempo
 * non ne è partita un'altra. Vale anche per l'errore: un guasto della domanda
 * vecchia, arrivato dopo che la nuova è andata bene, non deve accendere il
 * rosso sopra una libreria che si è letta benissimo.
 *
 * È la stessa regola che `App.tsx` scriveva a mano per la skin (`giroSkin`) e
 * che `rada.ts` applica alle pagine; qui è in una forma che si prova da sola
 * (`test/ultima.test.ts`).
 */

/**
 * Un contatore di turni.
 *
 * Chi chiede chiama la funzione restituita **prima** di mandare la domanda, e
 * ne riceve un'altra da chiamare quando la risposta arriva: `true` se è ancora
 * la domanda più recente, `false` se ne è partita una dopo.
 *
 * ```ts
 * const turno = vinceLUltima();
 * const mio = turno();
 * const arrivato = await ipc.avvio();
 * if (mio()) setAvvio(arrivato);
 * ```
 */
export function vinceLUltima(): () => () => boolean {
  let ultimo = 0;
  return () => {
    ultimo += 1;
    const mio = ultimo;
    return () => mio === ultimo;
  };
}
