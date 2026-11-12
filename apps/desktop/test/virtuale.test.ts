import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { riservaDiScorrimento } from "../src/virtuale";

// R14b-4, la parte di `virtuale.ts`: il salto a una riga non ancora disegnata
// scrive `scrollTop` da sé, e il browser non c'entra. Deve tenere conto della
// riserva che il foglio dichiara (`scroll-padding-top`), come fa il browser
// quando porta il fuoco su una riga già disegnata.
test("la riserva in cima si legge dal foglio, e senza riserva è zero", () => {
  assert.equal(riservaDiScorrimento("auto"), 0);
  assert.equal(riservaDiScorrimento(""), 0);
  assert.equal(riservaDiScorrimento("48px"), 48);
  assert.equal(riservaDiScorrimento("12.5px"), 12.5);
  assert.equal(riservaDiScorrimento("-4px"), 0);
});

test("scorriA porta la riga sotto la riserva in cima, non sotto gli avvisi aderenti", () => {
  const virtuale = readFileSync(new URL("../src/virtuale.ts", import.meta.url), "utf8");
  const scorri = virtuale.slice(virtuale.indexOf("const scorriA = useCallback"), virtuale.indexOf("[contenitore],", virtuale.indexOf("const scorriA = useCallback")));
  assert.match(scorri, /scrollPaddingTop/);
  assert.match(scorri, /riservaDiScorrimento\(/);
});

// ── R13b-7: la riga col fuoco resta montata anche fuori dalla finestra ─────

import { pezziDellaFinestra } from "../src/virtuale";

test("la riga col fuoco fuori dalla finestra si disegna al suo posto, fra due paglie", () => {
  // Dentro la finestra: le due paglie di sempre.
  assert.deepEqual(pezziDellaFinestra(10, 13, 100, 11), [
    { paglia: "sopra", da: 0, a: 10 },
    { riga: 10 },
    { riga: 11 },
    { riga: 12 },
    { paglia: "sotto", da: 13, a: 100 },
  ]);
  // Sopra: una rotellata ha portato la finestra in giù.
  assert.deepEqual(pezziDellaFinestra(10, 12, 100, 4), [
    { paglia: "sopra", da: 0, a: 4 },
    { riga: 4 },
    { paglia: "sopra-dopo", da: 5, a: 10 },
    { riga: 10 },
    { riga: 11 },
    { paglia: "sotto", da: 12, a: 100 },
  ]);
  // Sotto.
  assert.deepEqual(pezziDellaFinestra(10, 12, 100, 50), [
    { paglia: "sopra", da: 0, a: 10 },
    { riga: 10 },
    { riga: 11 },
    { paglia: "sotto", da: 12, a: 50 },
    { riga: 50 },
    { paglia: "sotto-dopo", da: 51, a: 100 },
  ]);
  // Un elenco vuoto non ha righe, e un fuoco oltre la fine non si disegna.
  assert.deepEqual(pezziDellaFinestra(0, 0, 0, 0), [
    { paglia: "sopra", da: 0, a: 0 },
    { paglia: "sotto", da: 0, a: 0 },
  ]);
});

test("la coda e l'albero delle Cartelle tengono montata la riga col fuoco", () => {
  for (const file of ["Coda.tsx", "schermate/Cartelle.tsx"]) {
    const sorgente = readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");
    assert.match(sorgente, /pezziDellaFinestra\(/, file);
  }
  // L'albero scrive l'indice sulle righe: disegnate fuori ordine, la posizione
  // nel DOM non dice più quale riga sia (`indiceDi`).
  const cartelle = readFileSync(new URL("../src/schermate/Cartelle.tsx", import.meta.url), "utf8");
  const riga = cartelle.slice(cartelle.indexOf('role="treeitem"') - 600, cartelle.indexOf('role="treeitem"') + 600);
  assert.match(riga, /data-indice=\{i\}/);
});
