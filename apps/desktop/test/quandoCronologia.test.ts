import { test } from "node:test";
import assert from "node:assert/strict";

import { fasciaQuando } from "../src/parti/quandoCronologia";

/*
 * Il «quando» della cronologia, per giorni di calendario e non per finestre
 * mobili. Le date si costruiscono nell'ora locale, come le legge chi guarda.
 */

const ADESSO = new Date(2026, 9, 30, 10, 0).getTime(); // venerdì 30 ottobre, 10:00

test("l'ascolto di ieri pomeriggio non si legge come un orario di oggi", () => {
  const ieri = new Date(2026, 9, 29, 14, 0).getTime(); // venti ore fa
  assert.equal(fasciaQuando(ieri, ADESSO).tipo, "ieri");
  const stamattina = new Date(2026, 9, 30, 8, 15).getTime();
  assert.equal(fasciaQuando(stamattina, ADESSO).tipo, "oggi");
});

test("venerdì scorso non si legge come il giorno di oggi", () => {
  // Sei giorni e sedici ore fa: in una finestra mobile di sette giorni, «ven».
  const venerdi = new Date(2026, 9, 23, 18, 0).getTime();
  assert.equal(fasciaQuando(venerdi, ADESSO).tipo, "data");
  const lunedi = new Date(2026, 9, 26, 9, 0).getTime();
  assert.equal(fasciaQuando(lunedi, ADESSO).tipo, "settimana");
});

test("le fasce corte restano quelle di prima", () => {
  assert.equal(fasciaQuando(ADESSO - 30_000, ADESSO).tipo, "ora");
  assert.deepEqual(fasciaQuando(ADESSO - 12 * 60_000, ADESSO), { tipo: "minuti", minuti: 12 });
  assert.equal(fasciaQuando(ADESSO + 60_000, ADESSO).tipo, "futuro");
  // Oltre dieci mesi la data porta l'anno.
  const anno = new Date(2025, 0, 2, 12, 0).getTime();
  assert.deepEqual(fasciaQuando(anno, ADESSO), { tipo: "data", anno: true });
});

test("a cavallo del cambio d'ora i giorni restano giorni", () => {
  // 25 ottobre 2026: in Europa l'ora legale finisce, e quel giorno dura 25 ore.
  const adesso = new Date(2026, 9, 26, 0, 30).getTime();
  const sabato = new Date(2026, 9, 24, 23, 45).getTime();
  assert.equal(fasciaQuando(sabato, adesso).tipo, "settimana");
  const domenica = new Date(2026, 9, 25, 0, 10).getTime();
  assert.equal(fasciaQuando(domenica, adesso).tipo, "ieri");
});
