/**
 * Due regole della finestrella «Nuovo tema» (`NuovoTema.tsx`), fuori da React
 * perché si possano provare.
 */

/**
 * Dove porta una freccia nel gruppo «Basato su», o `null` se il tasto non è
 * una freccia.
 *
 * È una griglia che va a capo, quindi non c'è un «sopra» che si possa dire a
 * colpo sicuro: le quattro frecce scorrono l'elenco, avanti e indietro, e in
 * fondo ricominciano. Il modulo con l'addizione: in JavaScript `-1 % 3` fa
 * `-1`, non `2`.
 */
export function indiceDopoFreccia(tasto: string, indice: number, quante: number): number | null {
  if (quante <= 0) return null;
  const passo =
    tasto === "ArrowRight" || tasto === "ArrowDown"
      ? 1
      : tasto === "ArrowLeft" || tasto === "ArrowUp"
        ? -1
        : 0;
  if (passo === 0) return null;
  return (indice + passo + quante) % quante;
}

/**
 * Una funzione che non riparte finché la volta prima non ha finito.
 *
 * «Crea» premuto due volte — un doppio clic, Invio battuto due volte — faceva
 * partire due creazioni con lo stesso id: la prima riusciva e apriva lo
 * Studio, la seconda trovava l'id preso e accendeva la fascia d'errore sopra un
 * tema appena creato. Finita la prima, un invio nuovo passa: se è fallita la
 * finestrella resta aperta e si deve poter riprovare.
 */
export function unaAllaVolta<A extends unknown[]>(
  fai: (...argomenti: A) => Promise<void> | void,
): (...argomenti: A) => Promise<void> {
  let inVolo = false;
  return async (...argomenti: A) => {
    if (inVolo) return;
    inVolo = true;
    try {
      await fai(...argomenti);
    } finally {
      inVolo = false;
    }
  };
}
