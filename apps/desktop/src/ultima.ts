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

/**
 * Una fotografia e gli eventi: la fotografia si adotta solo se nel frattempo
 * non è arrivato nessun evento.
 *
 * È la stessa regola per chi segue uno stato che il nucleo manda a ogni
 * cambiamento e ne chiede anche una fotografia all'apertura (`poi` in
 * `useAscolto`, `pagine.ts`): la fotografia si chiede quando l'ascolto è
 * aperto, ma la sua risposta — calcolata prima — può arrivare **dopo** un
 * evento più nuovo, e posata sopra di lui rimetterebbe a schermo il passato
 * fino al cambiamento dopo. L'evento è sempre più fresco della fotografia
 * chiesta prima di lui, o uguale. È lo schema di `Insieme.tsx`
 * (`eventiArrivati`), in una forma che si prova da sola.
 *
 * ```ts
 * const conto = eventiEFotografia();
 * // nel gestore dell'evento:   conto.evento();
 * // nel `poi` di `useAscolto`: const valida = conto.chiedi();
 * //                            ipc.stato().then((s) => { if (valida()) adotta(s); });
 * ```
 */
export function eventiEFotografia(): {
  /** È arrivato un evento. */
  evento: () => void;
  /** Si chiede la fotografia: la funzione restituita dice se vale ancora. */
  chiedi: () => () => boolean;
} {
  let eventi = 0;
  return {
    evento: () => {
      eventi += 1;
    },
    chiedi: () => {
      const prima = eventi;
      return () => eventi === prima;
    },
  };
}
