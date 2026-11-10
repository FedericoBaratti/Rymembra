/**
 * Quando lo schermo intero «In riproduzione» non ha più niente da mostrare.
 *
 * # Perché non bastava `brano === null`
 *
 * `App` monta lo schermo intero con `grande && conBrano`, e lo stato `grande`
 * non lo spegneva nessuno quando il brano spariva: dopo «Svuota la coda» lo
 * schermo intero restava «aperto» senza vedersi, il primo Indietro (Escape sul
 * computer) lo chiudeva senza che niente cambiasse a schermo, e il brano fatto
 * partire dopo lo riapriva a tutta pagina senza che nessuno l'avesse chiesto.
 * L'effetto dei due componenti che doveva chiuderlo — «il brano è finito
 * mentre il foglio era aperto» — non girava mai: `App` legge lo stesso archivio
 * e li smonta nello stesso disegno in cui il brano diventa `null`.
 *
 * Ma `brano: null` da solo non vuol dire «finito». Il nucleo manda lo stato
 * anche quando la libreria non risponde entro un secondo — una scansione su
 * una share lenta — e per un brano **nuovo** la scheda non ce l'ha nemmeno nel
 * ricordo (`StatoLettore::scheda_corrente`): la coda è sul brano, la scheda
 * arriva allo stato dopo. Chiudere lì vorrebbe dire chiudere lo schermo intero
 * a ogni cambio di brano durante una scansione, che è il difetto che quel
 * ricordo esiste per togliere. Per quell'istante basta che lo schermo intero
 * non si disegni, come fa già `grande && conBrano`.
 *
 * La fine è quando non c'è né la scheda né un posto nella coda: la coda è
 * vuota, o non è su nessun brano.
 *
 * Niente React e niente DOM: lo prova `test/schermoIntero.test.ts`.
 */

/** Della riproduzione, quel che dice se c'è ancora qualcosa in corso. */
export interface InCorso {
  brano: object | null;
  posizioneCoda: number | null;
}

/** Non c'è più niente in corso: lo schermo intero si chiude, e resta chiuso. */
export function nienteInCorso(stato: InCorso): boolean {
  return stato.brano === null && stato.posizioneCoda === null;
}
