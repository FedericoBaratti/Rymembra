/**
 * L'avanzamento della scansione, dagli eventi del nucleo all'archivio della
 * libreria (`libreria.ts`).
 *
 * Stava nel corpo di `App` perché era uno stato di `App`; adesso che lo stato
 * è un archivio, quel che resta è la strada che ci arriva — lo strozzamento a
 * quattro aggiornamenti al secondo, la scansione già in corso trovata
 * all'apertura, la fine detta dal nucleo — e sta qui, accanto a nessun altro.
 * `App` ne riceve le due maniglie che le servono per la scansione che chiede
 * lei (`scansiona`): come posare un valore subito, e il conto di quelle in
 * volo.
 *
 * L'hook si chiama nel punto di `App` in cui stavano i suoi due ascolti e il
 * suo effetto, dopo la skin e l'aspetto: l'ordine in cui si registrano è
 * quello di prima.
 */
import { useCallback, useEffect, useRef, type RefObject } from "react";

import { ipc, type Avanzamento } from "./ipc";
import type { Libreria } from "./libreria";
import { useAscolto } from "./pagine";

/** Le due maniglie della scansione per chi ne chiede una. */
export interface AvanzamentoScansione {
  /** Scrive l'avanzamento subito, buttando quello in sospeso. */
  posaScansione: (valore: Avanzamento | null) => void;
  /** Quante scansioni chieste da questa finestra sono in volo. Vedi sotto. */
  scansioniMie: RefObject<number>;
}

/** Segue l'avanzamento della scansione per tutta la vita della finestra. */
export function useAvanzamentoScansione(libreria: Libreria): AvanzamentoScansione {
  /*
   * L'avanzamento della scansione, a non più di quattro disegni al secondo.
   *
   * `scansione:avanzamento` arriva ogni venticinque file, cioè decine di volte
   * al secondo su un disco veloce, e ogni arrivo ridisegnava l'applicazione
   * intera — elenchi, colonna, lettore — per spostare una barra di un pixel.
   * Adesso ridisegna soltanto chi la disegna (`ConScansione`, le
   * Impostazioni), ma resta che un numero che si legge non cambia più spesso
   * di così; l'ultimo valore arriva sempre, e la fine del lavoro passa subito. Il timer in sospeso si butta
   * quando la scansione finisce, o un arrivo in ritardo rimetterebbe la barra
   * dopo che `scansiona` l'ha tolta.
   */
  const strozzaScansione = useRef<{
    ultimo: number;
    timer: number | undefined;
    valore: Avanzamento | null;
  }>({ ultimo: 0, timer: undefined, valore: null });
  const posaScansione = useCallback(
    (valore: Avanzamento | null) => {
      const r = strozzaScansione.current;
      window.clearTimeout(r.timer);
      r.timer = undefined;
      r.ultimo = performance.now();
      r.valore = valore;
      libreria.scansione.scrivi(valore);
    },
    [libreria],
  );
  useAscolto<Avanzamento>("scansione:avanzamento", (carico) => {
    const r = strozzaScansione.current;
    r.valore = carico;
    const passato = performance.now() - r.ultimo;
    const finito = carico.totale > 0 && carico.fatti >= carico.totale;
    if (finito || passato >= 250) {
      posaScansione(carico);
      return;
    }
    if (r.timer === undefined)
      r.timer = window.setTimeout(() => posaScansione(r.valore), 250 - passato);
  });
  useEffect(() => () => window.clearTimeout(strozzaScansione.current.timer), []);

  /**
   * Quante scansioni chieste da **questa** finestra sono in volo.
   *
   * La barra la toglie la risposta di `scansiona`, e solo la finestra che l'ha
   * chiesta la riceve. Una finestra ricaricata a metà scansione riceve ancora
   * gli avanzamenti ma non quella risposta, e la barra restava «in corso» per
   * sempre. Adesso la fine la dice anche il nucleo a tutti
   * (`scansione:finita`), e all'apertura la finestra gli chiede com'è
   * (`scansioneAvanzamento`); questo conto serve a non ascoltarli quando la
   * scansione è nostra — lì la barra resta fino a libreria riletta, come prima.
   * Un conto e non un sì/no: «Scansiona» premuto due volte fa due richieste, e
   * la seconda, rifiutata subito perché il turno è preso, non deve far credere
   * che la prima non sia più di nessuno.
   */
  const scansioniMie = useRef(0);
  useEffect(() => {
    ipc
      .scansioneAvanzamento()
      .then((adesso) => {
        if (scansioniMie.current === 0) posaScansione(adesso);
      })
      // Senza risposta resta quel che dicono gli eventi: è com'era prima.
      .catch(() => undefined);
  }, [posaScansione]);
  useAscolto("scansione:finita", () => {
    if (scansioniMie.current === 0) posaScansione(null);
  });

  return { posaScansione, scansioniMie };
}
