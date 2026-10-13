import { test } from "node:test";
import assert from "node:assert/strict";

import {
  colonneDa,
  finestraDa,
  larghezzaColonnaDa,
  passoGriglia,
  righeDiGriglia,
} from "../src/finestra";

test("la finestra in cima disegna lo schermo più il margine", () => {
  const f = finestraDa({
    scorrimento: 0,
    cima: 30,
    altezzaVista: 600,
    passo: 42,
    totale: 50_000,
    margine: 8,
  });
  assert.equal(f.primo, 0);
  assert.equal(f.visibileDa, 0);
  assert.equal(f.ultimo, Math.ceil(600 / 42) + 16);
});

test("in fondo a cinquantamila righe la finestra è l'ultima e non sfora", () => {
  const passo = 41.9;
  const totale = 50_000;
  const altezzaVista = 700;
  const cima = 30;
  // Lo scorrimento massimo: tutto l'elenco meno quel che si vede.
  const scorrimento = cima + totale * passo - altezzaVista;
  const f = finestraDa({ scorrimento, cima, altezzaVista, passo, totale, margine: 8 });
  assert.equal(f.ultimo, totale, "l'ultima riga è disegnata");
  assert.ok(f.primo > totale - 40, `e non molte prima: ${f.primo}`);
  assert.ok(f.visibileDa >= f.primo && f.visibileDa < f.ultimo);
});

test("uno scorrimento oltre i bordi resta dentro l'elenco", () => {
  const sopra = finestraDa({
    scorrimento: -500,
    cima: 0,
    altezzaVista: 400,
    passo: 40,
    totale: 100,
    margine: 8,
  });
  assert.equal(sopra.primo, 0);
  const sotto = finestraDa({
    scorrimento: 1e9,
    cima: 0,
    altezzaVista: 400,
    passo: 40,
    totale: 100,
    margine: 8,
  });
  assert.equal(sotto.ultimo, 100);
  assert.ok(sotto.primo <= 99);
  const vuoto = finestraDa({
    scorrimento: 0,
    cima: 0,
    altezzaVista: 400,
    passo: 40,
    totale: 0,
    margine: 8,
  });
  assert.deepEqual([vuoto.primo, vuoto.ultimo], [0, 0]);
});

test("un passo non misurato non divide per zero", () => {
  const f = finestraDa({
    scorrimento: 100,
    cima: 0,
    altezzaVista: 10,
    passo: 0,
    totale: 50,
    margine: 2,
  });
  assert.ok(Number.isFinite(f.primo) && Number.isFinite(f.ultimo));
  assert.ok(f.ultimo <= 50);
});

test("le colonne si contano dalle tracce calcolate", () => {
  assert.equal(colonneDa("171.5px 171.5px 171.5px"), 3);
  assert.equal(colonneDa("200px"), 1);
  assert.equal(colonneDa("[inizio] 180px [mezzo] 180px [fine]"), 2);
  // Una griglia non disegnata non ha tracce risolte: si tiene il numero di prima.
  assert.equal(colonneDa("repeat(auto-fill, minmax(164px, 1fr))"), null);
  assert.equal(colonneDa("none"), 1);
});

test("la larghezza della colonna è la prima traccia", () => {
  assert.equal(larghezzaColonnaDa("171.5px 171.5px"), 171.5);
  assert.equal(larghezzaColonnaDa("none"), null);
  assert.equal(larghezzaColonnaDa("repeat(3, 1fr)"), null);
});

test("le righe di griglia non contano una riga vuota in fondo", () => {
  assert.equal(righeDiGriglia(12, 4), 3);
  assert.equal(righeDiGriglia(13, 4), 4);
  assert.equal(righeDiGriglia(0, 4), 0);
  assert.equal(righeDiGriglia(5, 0), 5);
});

test("il fuoco nella griglia: frecce, bordi, Inizio e Fine", () => {
  // Quattro colonne, dieci celle: l'ultima riga ha due celle (8 e 9).
  const passo = (i: number, tasto: string) => passoGriglia(i, tasto, 4, 10);
  assert.equal(passo(0, "ArrowRight"), 1);
  assert.equal(passo(9, "ArrowRight"), null, "contro il bordo si resta");
  assert.equal(passo(0, "ArrowLeft"), null);
  assert.equal(passo(5, "ArrowLeft"), 4, "a sinistra si va anche a capo di riga");
  assert.equal(passo(1, "ArrowDown"), 5);
  assert.equal(passo(5, "ArrowUp"), 1);
  assert.equal(passo(2, "ArrowUp"), null);
  // Sotto la 6 non c'è niente in verticale, ma una riga sotto sì: l'ultima cella.
  assert.equal(passo(6, "ArrowDown"), 9);
  // Dall'ultima riga non si scende.
  assert.equal(passo(9, "ArrowDown"), null);
  assert.equal(passo(5, "Home"), 0);
  assert.equal(passo(5, "End"), 9);
  assert.equal(passo(5, "Enter"), null);
  assert.equal(passoGriglia(0, "End", 4, 0), null, "una griglia vuota non ha dove");
});
