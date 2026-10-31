import { test } from "node:test";
import assert from "node:assert/strict";

import { faseLettura } from "../src/parti/ramoTesto";

/*
 * Il pannello del testo prima che il testo ci sia.
 *
 * Finché `testoBrano` non rispondeva il pannello non disegnava niente — un
 * riquadro vuoto con l'occhiello, per decine di secondi su una condivisione
 * lenta — e dopo una lettura fallita restava vuoto per sempre. Qui la scelta
 * di cosa dire, senza React.
 */

test("mentre il testo si legge dal disco il pannello lo dice", () => {
  assert.equal(faseLettura({ testo: null, leggendo: true, illeggibile: false }), "leggo");
});

test("una lettura fallita si dice, invece di un pannello vuoto", () => {
  assert.equal(faseLettura({ testo: null, leggendo: false, illeggibile: true }), "illeggibile");
});

test("col testo arrivato si disegna il testo", () => {
  const testo = { strumentale: false };
  assert.equal(faseLettura({ testo, leggendo: false, illeggibile: false }), "pronto");
});
