import { test } from "node:test";
import assert from "node:assert/strict";

import { mescolato } from "../src/mescolato";

test("mescolato: gli stessi elementi, una volta ciascuno, e l'originale intatto", () => {
  const prima = [1, 2, 3, 4, 5, 6, 7, 8];
  const dopo = mescolato(prima);
  assert.deepEqual([...dopo].sort((a, b) => a - b), prima);
  assert.deepEqual(prima, [1, 2, 3, 4, 5, 6, 7, 8]);
});

test("mescolato: col caso fissato l'ordine è quello di Fisher-Yates", () => {
  // Il caso a zero scambia ogni posizione con la prima: [1,2,3,4] → [2,3,4,1].
  assert.deepEqual(
    mescolato([1, 2, 3, 4], () => 0),
    [2, 3, 4, 1],
  );
  // Appena sotto l'uno non scambia niente.
  assert.deepEqual(
    mescolato([1, 2, 3, 4], () => 0.999),
    [1, 2, 3, 4],
  );
});

test("mescolato: vuoto e uno solo restano come sono", () => {
  assert.deepEqual(mescolato([]), []);
  assert.deepEqual(mescolato(["a"]), ["a"]);
});
