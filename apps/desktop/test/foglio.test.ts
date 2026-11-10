/**
 * La durata di una transizione letta dal foglio di stile: quella che dice
 * quanto aspettare un foglio che scende prima di smontarlo (`foglio.ts`).
 */
import { test } from "node:test";
import assert from "node:assert/strict";

import { durataInMs } from "../src/foglio";

test("secondi e millisecondi, e vince la più lunga", () => {
  assert.equal(durataInMs("0.3s"), 300);
  assert.equal(durataInMs("120ms, 0.25s"), 250);
  assert.equal(durataInMs("0s, 0s"), 0);
});

test("quel che non si legge vale zero", () => {
  assert.equal(durataInMs(""), 0);
  assert.equal(durataInMs("auto"), 0);
});
