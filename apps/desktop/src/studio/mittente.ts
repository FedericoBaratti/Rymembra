/**
 * Chi manda il testo a validare: l'ultimo vince.
 *
 * # Il problema
 *
 * Trascinando un cursore il testo cambia trenta volte in mezzo secondo, e
 * ognuna di quelle versioni è un giro di `parse` + `compile` nel nucleo. Lo
 * Studio 2.x mandava tutto con un respiro fisso: le risposte si
 * accavallavano, e quella di una versione vecchia poteva arrivare dopo quella
 * nuova e rimettere sulla tela un foglio di due passi fa.
 *
 * # La regola
 *
 * Un respiro (120 ms) dopo l'ultima modifica, poi si parte. Una richiesta sola
 * in volo; se intanto il testo cambia ancora, la versione più recente aspetta
 * in coda — una, non tutte — e parte appena quella in volo torna. Le versioni
 * di mezzo non si validano mai: nessuno le guarderebbe.
 *
 * Il negozio fa il resto: una risposta che non è del testo di adesso non
 * tocca niente (`riduci`, azione `validato`).
 */

export interface Orologio {
  imposta: (fai: () => void, ms: number) => number;
  togli: (quale: number) => void;
}

const OROLOGIO_VERO: Orologio = {
  imposta: (fai, ms) => window.setTimeout(fai, ms),
  togli: (quale) => window.clearTimeout(quale),
};

export interface Mittente {
  /** Un testo nuovo da validare. */
  manda: (testo: string) => void;
  /** Smette: il timer si toglie, le risposte in volo si ignorano. */
  ferma: () => void;
}

export function creaMittente<Esito>(opzioni: {
  valida: (testo: string) => Promise<Esito>;
  arrivato: (testo: string, esito: Esito) => void;
  guasto: (errore: unknown) => void;
  respiro?: number;
  orologio?: Orologio;
}): Mittente {
  const { valida, arrivato, guasto, respiro = 120, orologio = OROLOGIO_VERO } = opzioni;
  let prossimo: string | null = null;
  let timer: number | null = null;
  let inVolo = false;
  let fermo = false;

  const parti = () => {
    if (fermo || inVolo || prossimo === null) return;
    const testo = prossimo;
    prossimo = null;
    inVolo = true;
    valida(testo)
      .then((esito) => {
        if (!fermo) arrivato(testo, esito);
      })
      .catch((errore: unknown) => {
        if (!fermo) guasto(errore);
      })
      .finally(() => {
        inVolo = false;
        // Quel che è arrivato mentre si aspettava parte adesso, se il suo
        // respiro è già finito; altrimenti parte quando finisce.
        if (timer === null) parti();
      });
  };

  return {
    manda(testo) {
      if (fermo) return;
      prossimo = testo;
      if (timer !== null) orologio.togli(timer);
      timer = orologio.imposta(() => {
        timer = null;
        parti();
      }, respiro);
    },
    ferma() {
      fermo = true;
      if (timer !== null) orologio.togli(timer);
      timer = null;
    },
  };
}
