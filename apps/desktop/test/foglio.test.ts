/**
 * La durata di una transizione letta dal foglio di stile: quella che dice
 * quanto aspettare un foglio che scende prima di smontarlo (`foglio.ts`). E
 * le aperture: un foglio per ciascuna, che chiude solo sé stesso.
 */
import { test } from "node:test";
import assert from "node:assert/strict";

import { chiaveDellApertura, chiusuraDi, durataInMs } from "../src/foglio";

test("secondi e millisecondi, e vince la più lunga", () => {
  assert.equal(durataInMs("0.3s"), 300);
  assert.equal(durataInMs("120ms, 0.25s"), 250);
  assert.equal(durataInMs("0s, 0s"), 0);
});

test("quel che non si legge vale zero", () => {
  assert.equal(durataInMs(""), 0);
  assert.equal(durataInMs("auto"), 0);
});

test("ogni apertura è un foglio suo", () => {
  // Il ⋮ di una riga, chiuso toccando il velo, e subito dopo il ⋮ di
  // un'altra: il menù nuovo riusava il foglio di prima, ancora in discesa, e
  // a discesa finita si chiudeva anche lui senza essersi mai visto.
  const a = { voci: ["a"] };
  const b = { voci: ["b"] };
  assert.notEqual(chiaveDellApertura(a), chiaveDellApertura(b));
  assert.equal(chiaveDellApertura(a), chiaveDellApertura(a), "la stessa apertura, la stessa chiave");
});

test("la chiusura di un foglio non chiude quello aperto dopo", () => {
  const a = { voci: ["a"] };
  const b = { voci: ["b"] };
  assert.equal(chiusuraDi(a)(a), null, "chiude sé stesso");
  assert.equal(chiusuraDi(a)(b), b, "e lascia stare quello che c'è adesso");
  assert.equal(chiusuraDi(a)(null), null);
});
