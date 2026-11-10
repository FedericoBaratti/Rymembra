/**
 * Le decisioni dei gesti del telefono: quando un trascinamento chiude, quando
 * una strisciata cambia brano, quando un tocco è ancora un tocco.
 */
import { test } from "node:test";
import assert from "node:assert/strict";

import {
  asseDelGesto,
  elastico,
  esitoStrisciata,
  esitoTrascinamento,
  velocitaDa,
} from "../src/gesti";

test("un tocco che trema non ha ancora un asse", () => {
  assert.equal(asseDelGesto(3, -4), null);
  assert.equal(asseDelGesto(9, 0), null);
});

test("un gesto deciso prende l'asse su cui va", () => {
  assert.equal(asseDelGesto(30, 4), "x");
  assert.equal(asseDelGesto(-2, 25), "y");
  // In diagonale ma più in giù che di lato: scende.
  assert.equal(asseDelGesto(6, 14), "y");
});

test("pari o quasi si aspetta, ma non per sempre", () => {
  assert.equal(asseDelGesto(12, 11), null);
  assert.equal(asseDelGesto(26, 22), "x");
  assert.equal(asseDelGesto(-22, -26), "y");
});

test("la velocità è quella del rilascio, non la media del gesto", () => {
  // Trascinato piano per mezzo secondo, poi lanciato negli ultimi 50 ms.
  const campioni = [
    { t: 0, x: 0, y: 0 },
    { t: 400, x: 0, y: 40 },
    { t: 450, x: 0, y: 45 },
    { t: 500, x: 0, y: 95 },
  ];
  const { vy } = velocitaDa(campioni);
  assert.ok(vy >= 1 - 1e-9, `vy = ${vy}`);
});

test("un dito che si è fermato non lancia niente", () => {
  assert.deepEqual(velocitaDa([{ t: 10, x: 5, y: 5 }]), { vx: 0, vy: 0 });
  assert.deepEqual(velocitaDa([]), { vx: 0, vy: 0 });
  const fermo = velocitaDa([
    { t: 0, x: 0, y: 0 },
    { t: 300, x: 0, y: 200 },
  ]);
  // Il primo campione è fuori dalla finestra: resta solo l'ultimo.
  assert.deepEqual(fermo, { vx: 0, vy: 0 });
});

test("un foglio si chiude oltre un quarto, o con un colpo verso il basso", () => {
  assert.equal(esitoTrascinamento({ dy: 100, vy: 0, altezza: 800 }), "torna");
  assert.equal(esitoTrascinamento({ dy: 220, vy: 0, altezza: 800 }), "chiudi");
  assert.equal(esitoTrascinamento({ dy: 40, vy: 0.8, altezza: 800 }), "chiudi");
});

test("un colpo verso l'alto rimette su il foglio anche da lontano", () => {
  assert.equal(esitoTrascinamento({ dy: 500, vy: -0.9, altezza: 800 }), "torna");
});

test("verso sinistra è il brano dopo, verso destra quello prima", () => {
  assert.equal(esitoStrisciata({ dx: -200, vx: 0, larghezza: 400 }), "prossimo");
  assert.equal(esitoStrisciata({ dx: 200, vx: 0, larghezza: 400 }), "precedente");
  assert.equal(esitoStrisciata({ dx: -40, vx: -0.9, larghezza: 400 }), "prossimo");
});

test("una strisciata corta, o un colpo contrario, non cambia niente", () => {
  assert.equal(esitoStrisciata({ dx: -60, vx: 0, larghezza: 400 }), null);
  assert.equal(esitoStrisciata({ dx: -60, vx: 0.9, larghezza: 400 }), null);
  assert.equal(esitoStrisciata({ dx: 0, vx: 0, larghezza: 400 }), null);
});

test("l'elastico cede sempre meno e non arriva al limite", () => {
  const poco = elastico(20, 60);
  const tanto = elastico(400, 60);
  assert.ok(poco > 0 && poco < 20);
  assert.ok(tanto > poco && tanto < 60);
  assert.equal(elastico(-400, 60), -tanto);
  assert.equal(elastico(10, 0), 0);
});
