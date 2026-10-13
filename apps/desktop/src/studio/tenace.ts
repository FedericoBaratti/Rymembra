/**
 * Ascolti e pulizie che sopravvivono a `<Activity>` nascosta.
 *
 * # Il problema
 *
 * Lo Studio vive sotto `<Activity>`: «Prova sull'app» lo nasconde senza
 * smontarlo, e lo stato resta. Gli **effetti** no: React li smonta quando
 * nasconde e li rimonta quando mostra. Per quasi tutto lo Studio va bene, ma
 * non per la chat, dove fino alla 3.0 voleva dire tre cose sbagliate:
 *
 * - la pulizia «chiudendo il pannello ferma la generazione» girava anche
 *   nascondendo lo Studio, quindi provare la skin sull'app mentre il modello
 *   rispondeva buttava via la risposta — a pagamento, su un modello remoto;
 * - gli ascoltatori di `ia:pezzo` e `ia:fine` si scioglievano, quindi anche
 *   senza la pulizia i pezzi arrivati mentre lo Studio era nascosto andavano
 *   persi, e al ritorno il turno restava aperto per sempre;
 * - la lettura della conversazione salvata ripartiva al ritorno, e scriveva
 *   quella del disco sopra quella in memoria, più nuova.
 *
 * # La soluzione
 *
 * Distinguere «nascosto» da «smontato» guardando il documento: nascondendo,
 * React lascia i nodi al loro posto (`display: none`); smontando li stacca
 * prima di chiamare le pulizie. Un nodo ancora `isConnected` vuol dire
 * nascosto. Gli ascoltatori si aprono una volta e si chiudono solo allo
 * smontaggio vero; mentre il componente è nascosto quel che arriva si mette in
 * coda, e si consegna nell'ordine d'arrivo appena torna visibile.
 */
import { useEffect, useRef, type RefObject } from "react";

import { useDati } from "../dati";

/** Gli ascoltatori di un componente, per evento. */
export type Gestori = Readonly<Record<string, (carico: unknown) => void>>;

interface Sportello {
  /** Il componente è visibile: quel che arriva si consegna subito. */
  aperto: boolean;
  /** Smontato davvero: niente più arriva, e gli ascolti si sciolgono. */
  chiuso: boolean;
  /** Quel che è arrivato mentre il componente era nascosto. */
  coda: [string, unknown][];
  sciogli: (() => void)[];
}

/** Il nodo è stato staccato dal documento: è uno smontaggio, non `<Activity>`. */
export function smontatoDavvero(nodo: Element | null): boolean {
  return nodo === null || !nodo.isConnected;
}

/**
 * Ascolta gli eventi del nucleo anche mentre il componente è nascosto.
 *
 * `radice` è un elemento del componente: serve a riconoscere lo smontaggio
 * vero (vedi il `//` di questo modulo). `gestori` può cambiare a ogni disegno:
 * si consegna sempre all'ultimo.
 */
export function useAscoltiTenaci(radice: RefObject<Element | null>, gestori: Gestori): void {
  const { ascolta } = useDati();
  const ultimi = useRef(gestori);
  // Senza dipendenze, e prima dell'effetto sotto: quando lo Studio torna
  // visibile la coda si consegna ai gestori dell'ultimo disegno.
  useEffect(() => {
    ultimi.current = gestori;
  });
  const sportello = useRef<Sportello | null>(null);
  const eventi = Object.keys(gestori).sort().join("|");

  useEffect(() => {
    let aperto = sportello.current;
    if (aperto === null || aperto.chiuso) {
      const nuovo: Sportello = { aperto: false, chiuso: false, coda: [], sciogli: [] };
      for (const evento of eventi.split("|")) {
        void ascolta<unknown>(evento, (carico) => {
          if (nuovo.chiuso) return;
          if (nuovo.aperto) ultimi.current[evento]?.(carico);
          else nuovo.coda.push([evento, carico]);
        }).then((stop) => {
          if (nuovo.chiuso) stop();
          else nuovo.sciogli.push(stop);
        });
      }
      sportello.current = nuovo;
      aperto = nuovo;
    }
    aperto.aperto = true;
    for (const [evento, carico] of aperto.coda.splice(0)) ultimi.current[evento]?.(carico);
    const nodo = radice.current;
    const questo = aperto;
    return () => {
      questo.aperto = false;
      if (!smontatoDavvero(nodo)) return;
      questo.chiuso = true;
      questo.coda = [];
      for (const stop of questo.sciogli.splice(0)) stop();
    };
  }, [ascolta, eventi, radice]);
}

/**
 * Fa `azione` solo quando il componente si smonta davvero, non quando
 * `<Activity>` lo nasconde. `azione` può cambiare a ogni disegno.
 */
export function useAlloSmontaggio(radice: RefObject<Element | null>, azione: () => void): void {
  const ultima = useRef(azione);
  useEffect(() => {
    ultima.current = azione;
  });
  useEffect(() => {
    const nodo = radice.current;
    return () => {
      if (smontatoDavvero(nodo)) ultima.current();
    };
  }, [radice]);
}
