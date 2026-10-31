import { test } from "node:test";
import assert from "node:assert/strict";

import { passoPagina } from "../src/fuoco";
import * as fuoco from "../src/fuoco";

test("in un elenco, una schermata più in là e più in qua", () => {
  assert.equal(passoPagina(0, 1, 14, 1, 50_000), 14);
  assert.equal(passoPagina(100, -1, 14, 1, 50_000), 86);
});

test("contro i capi si ferma sulla prima e sull'ultima riga", () => {
  assert.equal(passoPagina(5, -1, 14, 1, 50_000), 0);
  assert.equal(passoPagina(49_990, 1, 14, 1, 50_000), 49_999);
});

test("in una griglia resta sulla stessa colonna", () => {
  // Cinque colonne, tre righe per schermata: da 7 (riga 1, colonna 2) a 22.
  assert.equal(passoPagina(7, 1, 3, 5, 100), 22);
  assert.equal(passoPagina(22, -1, 3, 5, 100), 7);
  // In cima: prima riga, stessa colonna.
  assert.equal(passoPagina(7, -1, 3, 5, 100), 2);
});

test("in fondo a una griglia con l'ultima riga corta", () => {
  // 23 schede su cinque colonne: l'ultima riga ha le colonne 0, 1 e 2.
  assert.equal(passoPagina(6, 1, 9, 5, 23), 21);
  // Colonna 4: nell'ultima riga non c'è, si va all'ultima scheda.
  assert.equal(passoPagina(9, 1, 9, 5, 23), 22);
});

test("almeno una riga, e niente da fare in un elenco vuoto", () => {
  assert.equal(passoPagina(3, 1, 0, 1, 10), 4);
  assert.equal(passoPagina(0, 1, 5, 1, 0), 0);
});

test("tornare alla riga attiva fuori dalla finestra scorre", () => {
  // Fuoco sulla riga 0, la rotella porta la finestra alla 5000, Inizio: la
  // riga d'arrivo è già quella attiva, `setAttivo` non ridisegna, e l'effetto
  // che scorre non girava. `vaiA` deve scorrere da sé.
  const { daScorrere } = fuoco;
  assert.equal(typeof daScorrere, "function");
  assert.equal(daScorrere(0, { primo: 4992, ultimo: 5030 }), true);
  assert.equal(daScorrere(5029, { primo: 4992, ultimo: 5030 }), false);
  assert.equal(daScorrere(5000, { primo: 4992, ultimo: 5030 }), false);
  assert.equal(daScorrere(5030, { primo: 4992, ultimo: 5030 }), true);
});
