/**
 * Una raffica di chiamate, un lavoro solo.
 *
 * # Il caso
 *
 * Quando dei brani scaricati entrano in libreria il nucleo manda due eventi di
 * fila, `scarico:in_libreria` e `cartelle:cambiate`, a pochi millisecondi l'uno
 * dall'altro, e la finestra rileggeva la libreria due volte: due `avvio`, due
 * giri di pagine per ogni elenco aperto, per una sola scansione.
 *
 * # La regola
 *
 * La prima chiamata apre una finestra di `ms` millisecondi; quelle che arrivano
 * dentro ci si aggiungono, e alla chiusura il lavoro si fa una volta. In
 * **coda** e non in testa: fatto alla prima chiamata, leggerebbe lo stato di
 * prima della seconda, e se la seconda portava un cambiamento suo — una
 * scansione a mano finita subito dopo una automatica — quel cambiamento
 * resterebbe fuori. La finestra non si allunga a ogni chiamata: il ritardo ha
 * un tetto, e una pioggia di eventi non rimanda il lavoro per sempre.
 *
 * Una chiamata che arriva **dopo** che il lavoro è partito ne apre un'altra:
 * il lavoro fatto non poteva vederla.
 */

/** Quel che `accorpa` restituisce. */
export interface Accorpato {
  /** Chiede il lavoro: entro `ms` millisecondi, una volta per raffica. */
  chiama: () => void;
  /** Butta il lavoro in sospeso, se c'è: allo smontaggio. */
  butta: () => void;
}

/** Vedi il commento in testa. */
export function accorpa(fai: () => void, ms: number): Accorpato {
  let timer: ReturnType<typeof setTimeout> | undefined;
  return {
    chiama() {
      if (timer !== undefined) return;
      timer = setTimeout(() => {
        timer = undefined;
        fai();
      }, ms);
    },
    butta() {
      if (timer === undefined) return;
      clearTimeout(timer);
      timer = undefined;
    },
  };
}
