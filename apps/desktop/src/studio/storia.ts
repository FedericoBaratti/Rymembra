/**
 * Annulla e ripeti, sul testo del documento.
 *
 * # Perché serve, e perché non basta quello del browser
 *
 * Lo Studio non aveva nessun annulla. Nemmeno quello nativo della `textarea`,
 * che pure ci sarebbe stato: la sorgente è uno stato controllato, e ogni
 * controllo — un cursore, un elenco, una casella — chiama `setSorgente` con un
 * testo riscritto da capo. React lo riscrive nel campo, e la pila di annullo del
 * browser, che tiene traccia di quel che l'utente ha *digitato*, non sa cosa
 * farsene di un valore comparso dall'esterno: si svuota. Quindi non solo
 * trascinare un cursore non si poteva annullare — trascinare un cursore
 * cancellava anche la possibilità di annullare quel che si era scritto a mano un
 * momento prima.
 *
 * La sorgente di verità è il testo, e il testo intero costa poco da conservare:
 * un documento di skin sta in qualche decina di kilobyte, e cento passi sono
 * qualche megabyte nel caso peggiore. Un annullo strutturato — quale campo, quale
 * valore prima — sarebbe un secondo modello da tenere allineato al primo, cioè
 * esattamente quel che `patch.ts` esiste per non fare.
 *
 * # L'accorpamento è a tempo, e non per provenienza
 *
 * Un trascinamento di cursore produce trenta `setSorgente` in mezzo secondo, e
 * trenta passi di annullo per un gesto solo sono peggio di nessuno: per tornare
 * indietro di un gesto si preme trenta volte. Marcare la provenienza — «questo
 * viene da un cursore» — vorrebbe dire passare un'etichetta attraverso ogni
 * controllo, e ricordarsene al prossimo.
 *
 * A tempo funziona senza etichette e vale per tutti allo stesso modo: le
 * modifiche che si susseguono senza una pausa sono lo stesso gesto. Una pausa —
 * mezzo secondo, quel tanto che separa due intenzioni — apre un passo nuovo.
 */
import { useCallback, useRef, useState } from "react";

/** Quanto silenzio separa due gesti. */
const PAUSA_MS = 500;

/** Quanti passi si conservano. Oltre, i più vecchi cadono. */
const PROFONDITA = 100;

export type Storia = {
  /** Il testo adesso. */
  sorgente: string;
  /** Scrive, e apre un passo di annullo (o allunga quello aperto). */
  scriviSorgente: (prossimo: string | ((prima: string) => string)) => void;
  /**
   * Riparte da un testo, senza passato.
   *
   * È il caricamento del documento: annullare fino a «prima che il file
   * esistesse» non vuol dire niente, e lasciarlo fare mostrerebbe una stringa
   * vuota al posto della skin.
   */
  riparti: (testo: string) => void;
  annulla: () => void;
  ripeti: () => void;
  puoAnnullare: boolean;
  puoRipetere: boolean;
};

export function useStoria(): Storia {
  const [passato, setPassato] = useState<readonly string[]>([]);
  const [sorgente, setSorgente] = useState("");
  const [futuro, setFuturo] = useState<readonly string[]>([]);
  /** Quando è stato aperto il passo corrente. */
  const ultimoTocco = useRef(0);

  const scriviSorgente = useCallback(
    (prossimo: string | ((prima: string) => string)) => {
      const adesso = Date.now();
      const nellaStessaRaffica = adesso - ultimoTocco.current < PAUSA_MS;
      ultimoTocco.current = adesso;

      setSorgente((prima) => {
        const dopo =
          typeof prossimo === "function" ? prossimo(prima) : prossimo;
        // Una scrittura che non cambia niente non è un passo. Capita più di
        // quanto sembri: `scriviIn` restituisce la sorgente intatta quando il
        // testo non è JSON, e senza questa guardia il documento rotto
        // riempirebbe la pila di copie identiche.
        if (dopo === prima) return prima;

        setPassato((prece) =>
          // Dentro la raffica il passo resta uno: si sposta solo il punto
          // d'arrivo, e il punto di partenza è ancora quello di prima.
          nellaStessaRaffica && prece.length > 0
            ? prece
            : [...prece, prima].slice(-PROFONDITA),
        );
        setFuturo([]);
        return dopo;
      });
    },
    [],
  );

  const riparti = useCallback((testo: string) => {
    setPassato([]);
    setFuturo([]);
    setSorgente(testo);
    ultimoTocco.current = 0;
  }, []);

  const annulla = useCallback(() => {
    setPassato((prece) => {
      if (prece.length === 0) return prece;
      const indietro = prece[prece.length - 1];
      if (indietro === undefined) return prece;
      setSorgente((ora) => {
        setFuturo((avanti) => [ora, ...avanti].slice(0, PROFONDITA));
        return indietro;
      });
      // Il prossimo tocco apre un passo nuovo: senza, scrivere subito dopo un
      // annullo si accorperebbe al passo che si è appena disfatto.
      ultimoTocco.current = 0;
      return prece.slice(0, -1);
    });
  }, []);

  const ripeti = useCallback(() => {
    setFuturo((avanti) => {
      if (avanti.length === 0) return avanti;
      const prossimo = avanti[0];
      if (prossimo === undefined) return avanti;
      setSorgente((ora) => {
        setPassato((prece) => [...prece, ora].slice(-PROFONDITA));
        return prossimo;
      });
      ultimoTocco.current = 0;
      return avanti.slice(1);
    });
  }, []);

  return {
    sorgente,
    scriviSorgente,
    riparti,
    annulla,
    ripeti,
    puoAnnullare: passato.length > 0,
    puoRipetere: futuro.length > 0,
  };
}
