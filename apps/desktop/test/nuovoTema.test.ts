import { test } from "node:test";
import assert from "node:assert/strict";

import { indiceDopoFreccia, unaAllaVolta } from "../src/temaNuovo";

test("le frecce nel gruppo «Basato su» girano e arrivano oltre la vicina", () => {
  // Sei skin, fuoco sulla terza: due frecce a destra portano alla quinta,
  // perché la seconda parte da dove ha portato la prima.
  let dove = 2;
  for (const tasto of ["ArrowRight", "ArrowRight"]) {
    const dopo = indiceDopoFreccia(tasto, dove, 6);
    assert.notEqual(dopo, null);
    dove = dopo ?? dove;
  }
  assert.equal(dove, 4);
  assert.equal(indiceDopoFreccia("ArrowLeft", 0, 6), 5);
  assert.equal(indiceDopoFreccia("ArrowDown", 5, 6), 0);
  assert.equal(indiceDopoFreccia("ArrowUp", 3, 6), 2);
  assert.equal(indiceDopoFreccia("Enter", 3, 6), null);
  assert.equal(indiceDopoFreccia("ArrowRight", 0, 0), null);
});

test("«Crea» premuto due volte crea una volta sola", async () => {
  let chiamate = 0;
  let finisci: () => void = () => undefined;
  const crea = unaAllaVolta(
    () =>
      new Promise<void>((risolvi) => {
        chiamate += 1;
        finisci = risolvi;
      }),
  );
  const prima = crea();
  const seconda = crea();
  assert.equal(chiamate, 1);
  finisci();
  await Promise.all([prima, seconda]);
  // Finita la prima, un nuovo invio — dopo un errore, per esempio — passa.
  const terza = crea();
  assert.equal(chiamate, 2);
  finisci();
  await terza;
});
