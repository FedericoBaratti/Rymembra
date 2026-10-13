import { test } from "node:test";
import assert from "node:assert/strict";

import { differenze, differenzeFoglia, differenzePerPercorso, quanteCambiano } from "../src/studio/differenze";

test("una riga aggiunta in cima è una riga, non tutto il resto", () => {
  const prima = ["a", "b", "c", "d", "e"].join("\n");
  const dopo = ["nuova", "a", "b", "c", "d", "e"].join("\n");
  const righe = differenze(prima, dopo);
  assert.deepEqual(quanteCambiano(righe), { aggiunte: 1, tolte: 0 });
  assert.deepEqual(righe[0], { segno: "+", testo: "nuova" });
});

test("sostituzioni e rimozioni in mezzo", () => {
  const prima = ["uno", "due", "tre", "quattro"].join("\n");
  const dopo = ["uno", "DUE", "quattro"].join("\n");
  const righe = differenze(prima, dopo);
  assert.deepEqual(quanteCambiano(righe), { aggiunte: 1, tolte: 2 });
  // Le righe uguali restano in ordine, con i cambi al loro posto.
  assert.deepEqual(
    righe.filter((r) => r.segno === "=").map((r) => r.testo),
    ["uno", "quattro"],
  );
});

test("due testi uguali non hanno differenze", () => {
  const testo = ["x", "y", "z"].join("\n");
  assert.deepEqual(quanteCambiano(differenze(testo, testo)), { aggiunte: 0, tolte: 0 });
});

test("per percorso: quel che cambia, quel che arriva, quel che se ne va", () => {
  const cambi = differenzePerPercorso(
    { parts: { card: { radius: "12px", opacity: 1 } }, id: "x" },
    { parts: { card: { radius: "14px" }, heart: { textColor: "#fff" } }, id: "x" },
  );
  assert.deepEqual(cambi, [
    { percorso: "parts.card.radius", prima: "12px", dopo: "14px" },
    { percorso: "parts.card.opacity", prima: 1, dopo: undefined },
    { percorso: "parts.heart", prima: undefined, dopo: { textColor: "#fff" } },
  ]);
});

test("per foglia: un oggetto che arriva o se ne va si apre fino ai valori", () => {
  const cambi = differenzeFoglia(
    { parts: { heart: { textColor: "#fff", states: { hover: { opacity: 1 } } } }, id: "x" },
    { parts: { card: { radius: "18px", states: { hover: { opacity: 0.92 } } } }, id: "x" },
  );
  assert.deepEqual(cambi, [
    { percorso: "parts.heart.textColor", prima: "#fff", dopo: undefined },
    { percorso: "parts.heart.states.hover.opacity", prima: 1, dopo: undefined },
    { percorso: "parts.card.radius", prima: undefined, dopo: "18px" },
    { percorso: "parts.card.states.hover.opacity", prima: undefined, dopo: 0.92 },
  ]);
  // Un valore che cambia forma resta un cambio solo.
  assert.deepEqual(differenzeFoglia({ radius: "12px" }, { radius: { tl: "4px" } }), [
    { percorso: "radius", prima: "12px", dopo: { tl: "4px" } },
  ]);
});
