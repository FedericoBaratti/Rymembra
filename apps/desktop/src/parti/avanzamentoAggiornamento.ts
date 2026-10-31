/**
 * L'avanzamento dello scaricamento di un aggiornamento, come lo tiene la
 * fascia: un riduttore puro, che `test/avanzamentoAggiornamento.test.ts` prova.
 *
 * Si scriveva solo da `aggiornamenti:avanzamento` e non si azzerava mai: dopo
 * «Annulla» e un nuovo «Aggiorna» la fascia mostrava subito i megabyte e la
 * quota del tentativo di prima, e li teneva finché il nucleo non mandava il
 * primo pezzo nuovo — o per tutto `SILENZIO_MASSIMO`, un minuto, se la rete
 * taceva: un avanzamento che non stava succedendo. Un tentativo nuovo, o uno
 * annullato, riparte da `null`, cioè da «Comincio…».
 */
import type { AvanzamentoAggiornamento } from "../ipc";

export type EventoAvanzamento =
  | { tipo: "avanzamento"; valore: AvanzamentoAggiornamento }
  | { tipo: "installa" }
  | { tipo: "annulla" };

export function avanzamentoDopo(
  _prima: AvanzamentoAggiornamento | null,
  evento: EventoAvanzamento,
): AvanzamentoAggiornamento | null {
  return evento.tipo === "avanzamento" ? evento.valore : null;
}
