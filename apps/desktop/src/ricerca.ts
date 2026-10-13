/**
 * La ricerca in libreria: la frase che si chiede, quanti risultati ha, e le
 * loro righe.
 *
 * Il passaggio in modalità ricerca è **immediato** — lo decide `App` appena
 * c'è qualcosa nella casella, e la pagina cambia (`pagina.ts`) — la richiesta
 * no: quel che si chiede al nucleo segue con un ritardo, e qui c'è tutto quel
 * che vive dentro quel ritardo. Tre cose che si tengono per mano: la frase
 * pigra, il conteggio di quella frase, e la lista rada lunga quanto il
 * conteggio. Separate nel corpo di `App` erano tre posti da tenere d'accordo
 * a mano.
 *
 * L'hook si chiama dove stava la lista rada della ricerca, dopo quelle dei
 * Brani e dei Preferiti: la frase pigra, che stava qualche riga più su, parte
 * adesso un poco dopo di loro, e sono tre domande indipendenti.
 */
import { useEffect, useState } from "react";

import { ipc, type Brano } from "./ipc";
import { usePigro, useRada } from "./pagine";
import type { ArchivioRado } from "./rada";

/** La ricerca, vista da chi la disegna. */
export interface Ricerca {
  /** La frase che si è chiesta al nucleo: quella della casella, 140 ms dopo. */
  queryPigra: string;
  /** Il campo e la richiesta non dicono ancora la stessa cosa. */
  inRitardo: boolean;
  /** Quanti risultati ha la frase di adesso, o `null` finché non si sa. */
  risultatiAdesso: number | null;
  /** Si aspetta ancora il conteggio: vedi `contandoRisultati` qui sotto. */
  contandoRisultati: boolean;
  /** Le righe dei risultati, a pagine. */
  rada: ArchivioRado<Brano>;
}

/** La ricerca della casella `query`. */
export function useRicerca(
  query: string,
  segnalaErrore: (e: unknown) => void,
): Ricerca {
  const queryPigra = usePigro(query.trim(), 140);
  const inRitardo = query.trim().length > 0 && queryPigra !== query.trim();

  /**
   * Quanti risultati ha la ricerca **in tutto**, e per quale frase.
   *
   * Chiesto a parte e una volta per query: l'intestazione scriveva «60
   * risultati» per una ricerca che ne aveva trecento, perché il numero era la
   * lunghezza della prima pagina. Un conteggio che si ferma al limite non è un
   * conteggio, è il limite scritto in lettere.
   *
   * Con la frase accanto, perché adesso il numero è anche la **lunghezza**
   * dell'elenco: il conteggio di «sub» mentre si cerca già «subs» darebbe
   * all'elenco nuovo la barra di scorrimento del vecchio. Finché quello della
   * frase di adesso non arriva, non c'è (`risultatiAdesso`).
   */
  const [risultati, setRisultati] = useState<{ query: string; n: number } | null>(null);
  useEffect(() => {
    if (queryPigra.length === 0) {
      setRisultati(null);
      return;
    }
    let annullato = false;
    ipc
      .cercaConteggio(queryPigra)
      .then((quanti) => {
        if (!annullato) setRisultati({ query: queryPigra, n: quanti });
      })
      // Anche l'errore di una frase già superata si tace, come la risposta:
      // la fascia rossa per «sub» mentre si legge l'elenco di «subs» parla di
      // una ricerca che nessuno sta più guardando.
      .catch((e: unknown) => {
        if (!annullato) segnalaErrore(e);
      });
    return () => {
      annullato = true;
    };
  }, [queryPigra, segnalaErrore]);
  const risultatiAdesso =
    risultati !== null && risultati.query === queryPigra ? risultati.n : null;

  const rada = useRada<Brano>({
    chiave: `cerca:${queryPigra}`,
    totale: risultatiAdesso ?? 0,
    chiedi: (offset, limite) =>
      queryPigra.length > 0
        ? ipc.cerca(queryPigra, offset, limite)
        : Promise.resolve([]),
    onErrore: segnalaErrore,
  });

  /**
   * La ricerca aspetta ancora il suo conteggio.
   *
   * Le liste rade non aspettano la prima pagina: il totale c'è prima delle
   * righe, e l'elenco si disegna subito, con gli scheletri dove le righe
   * mancano. Resta da aspettare il totale stesso, che per la ricerca arriva con
   * una chiamata sua — e prima ancora c'è il ritardo dei 140 ms fra un tasto e
   * la richiesta.
   */
  const contandoRisultati =
    inRitardo || (queryPigra.length > 0 && risultatiAdesso === null);

  return { queryPigra, inRitardo, risultatiAdesso, contandoRisultati, rada };
}
