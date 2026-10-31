import { test } from "node:test";
import assert from "node:assert/strict";

import { chiusuraLibera, gestoDelTasto } from "../src/tastiSincronizza";

test("con battute fatte Escape non chiude senza chiedere", () => {
  assert.equal(chiusuraLibera("battute", [1200], []), false);
  assert.equal(chiusuraLibera("rivedi", [1200, 3400], []), false);
  assert.equal(chiusuraLibera("parole", [1200], [1300]), false);
  // Niente da perdere: le righe non battute, una battuta non ancora data, il
  // lavoro già salvato.
  assert.equal(chiusuraLibera("righe", [], []), true);
  assert.equal(chiusuraLibera("battute", [], []), true);
  assert.equal(chiusuraLibera("dono", [1200], [1300]), true);
});

test("i tasti delle battute", () => {
  const normale = { raddrizzo: false, tutteBattute: false };
  assert.equal(gestoDelTasto(" ", normale), "batti");
  assert.equal(gestoDelTasto("Backspace", normale), "rifai");
  assert.equal(gestoDelTasto("Enter", normale), null);
  assert.equal(gestoDelTasto("Enter", { raddrizzo: false, tutteBattute: true }), "finisci");
  assert.equal(gestoDelTasto("a", normale), null);
});

test("durante il raddrizzamento Invio, Backspace e Spazio si prendono e non fanno niente", () => {
  const raddrizza = { raddrizzo: true, tutteBattute: true };
  assert.equal(gestoDelTasto("Enter", raddrizza), "niente");
  assert.equal(gestoDelTasto("Backspace", raddrizza), "niente");
  assert.equal(gestoDelTasto(" ", raddrizza), "niente");
  assert.equal(gestoDelTasto("a", raddrizza), null);
});
