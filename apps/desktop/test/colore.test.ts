import { test } from "node:test";
import assert from "node:assert/strict";

import { contrasto, daOklch, esadecimale, leggiColore, versoOklch } from "../src/studio/colore";

test("le sei forme del formato si leggono, le altre no", () => {
  assert.deepEqual(leggiColore("#fff"), { r: 1, g: 1, b: 1, a: 1 });
  assert.equal(leggiColore("#ff000080")?.a.toFixed(2), "0.50");
  assert.equal(leggiColore("rgba(255, 0, 0, 0.5)")?.a, 0.5);
  assert.equal(leggiColore("rgb(0 128 255)")?.g.toFixed(3), (128 / 255).toFixed(3));
  assert.equal(leggiColore("oklch(0.5 0.1 20)"), null);
  assert.equal(leggiColore("rosso"), null);
});

test("l'esadecimale tiene l'alfa solo quando serve", () => {
  assert.equal(esadecimale({ r: 1, g: 0, b: 0, a: 1 }), "#ff0000");
  assert.equal(esadecimale({ r: 1, g: 0, b: 0, a: 0.5 }), "#ff000080");
});

test("OKLCH andata e ritorno restituisce il colore", () => {
  for (const testo of ["#a3301c", "#f3ede2", "#1c1712", "#4e8fd8", "#808080"]) {
    const letto = leggiColore(testo);
    assert.ok(letto);
    assert.equal(esadecimale(daOklch(versoOklch(letto))), testo);
  }
});

test("un croma impossibile si riduce invece di cambiare tinta", () => {
  const dentro = daOklch({ l: 0.7, c: 0.37, h: 140, a: 1 });
  for (const canale of [dentro.r, dentro.g, dentro.b]) {
    assert.ok(canale >= 0 && canale <= 1);
  }
  const tinta = versoOklch(dentro).h;
  assert.ok(Math.abs(tinta - 140) < 3, `tinta ${tinta}`);
});

test("il contrasto è quello di WCAG", () => {
  const nero = { r: 0, g: 0, b: 0, a: 1 };
  const bianco = { r: 1, g: 1, b: 1, a: 1 };
  assert.equal(contrasto(nero, bianco).toFixed(0), "21");
  assert.equal(contrasto(bianco, bianco), 1);
});
