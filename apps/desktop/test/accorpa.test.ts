/**
 * Una raffica di eventi, una ricarica sola: `scarico:in_libreria` e
 * `cartelle:cambiate` a pochi millisecondi rileggono la libreria una volta.
 */
import { mock, test } from "node:test";
import assert from "node:assert/strict";

import { accorpa } from "../src/accorpa";

test("due chiamate di fila fanno il lavoro una volta, alla chiusura della finestra", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const fai = mock.fn();
  const ricarica = accorpa(fai, 50);
  ricarica.chiama(); // scarico:in_libreria
  t.mock.timers.tick(3);
  ricarica.chiama(); // cartelle:cambiate
  assert.equal(fai.mock.callCount(), 0, "in coda, non in testa");
  t.mock.timers.tick(47);
  assert.equal(fai.mock.callCount(), 1);
  t.mock.timers.tick(500);
  assert.equal(fai.mock.callCount(), 1, "e poi più niente");
});

test("la finestra non si allunga: il ritardo ha un tetto", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const fai = mock.fn();
  const ricarica = accorpa(fai, 50);
  ricarica.chiama();
  t.mock.timers.tick(40);
  ricarica.chiama();
  t.mock.timers.tick(10);
  assert.equal(fai.mock.callCount(), 1, "cinquanta dalla prima, non dall'ultima");
});

test("una chiamata dopo il lavoro ne chiede un altro", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const fai = mock.fn();
  const ricarica = accorpa(fai, 50);
  ricarica.chiama();
  t.mock.timers.tick(50);
  ricarica.chiama();
  t.mock.timers.tick(50);
  assert.equal(fai.mock.callCount(), 2, "il primo lavoro non poteva vedere la seconda");
});

test("buttato allo smontaggio, il lavoro in sospeso non parte", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const fai = mock.fn();
  const ricarica = accorpa(fai, 50);
  ricarica.chiama();
  ricarica.butta();
  t.mock.timers.tick(100);
  assert.equal(fai.mock.callCount(), 0);
  // E resta usabile: lo smontaggio doppio della modalità rigorosa di React
  // butta e poi rimonta lo stesso oggetto.
  ricarica.chiama();
  t.mock.timers.tick(50);
  assert.equal(fai.mock.callCount(), 1);
});
