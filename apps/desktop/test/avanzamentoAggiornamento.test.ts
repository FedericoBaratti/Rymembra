import { test } from "node:test";
import assert from "node:assert/strict";

import type { AvanzamentoAggiornamento } from "../src/ipc";
import * as modulo from "../src/parti/avanzamentoAggiornamento";

/*
 * La barra dello scaricamento dell'aggiornamento: dopo «Annulla» e un nuovo
 * «Aggiorna» ripartiva dai megabyte del tentativo di prima, e restava lì
 * finché non arrivava il primo pezzo nuovo — o per un minuto, a rete muta.
 */

const quarantacinque = { scaricati: 45 * 1024 * 1024, totale: 70 * 1024 * 1024 } as AvanzamentoAggiornamento;

test("dopo annulla e un nuovo aggiorna la barra riparte da zero", () => {
  const { avanzamentoDopo } = modulo;
  assert.equal(typeof avanzamentoDopo, "function");
  let a = avanzamentoDopo(null, { tipo: "avanzamento", valore: quarantacinque });
  assert.equal(a, quarantacinque);
  a = avanzamentoDopo(a, { tipo: "annulla" });
  assert.equal(a, null);
  a = avanzamentoDopo(quarantacinque, { tipo: "installa" });
  assert.equal(a, null);
});
