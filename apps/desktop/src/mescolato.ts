/**
 * Un elenco in ordine casuale, per il tasto «Casuale» delle pagine del
 * telefono (`parti/TestataDettaglio.tsx`).
 *
 * Una copia mescolata, e non il casuale del lettore acceso: «Casuale» su un
 * disco vuol dire «questo disco, in un ordine qualunque, adesso», e accendere
 * il casuale del lettore lo lascerebbe acceso anche per la coda che verrà
 * dopo, che nessuno ha chiesto di mescolare. La coda parte dal primo
 * dell'elenco mescolato, come «Riproduci» parte dal primo dell'elenco vero.
 *
 * Fisher-Yates, e il caso si passa: le prove lo fissano.
 */
export function mescolato<T>(elenco: readonly T[], caso: () => number = Math.random): T[] {
  const copia = [...elenco];
  for (let i = copia.length - 1; i > 0; i -= 1) {
    const j = Math.floor(caso() * (i + 1));
    const qui = copia[i] as T;
    copia[i] = copia[j] as T;
    copia[j] = qui;
  }
  return copia;
}
