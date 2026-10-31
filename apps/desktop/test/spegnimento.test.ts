import { test } from "node:test";
import assert from "node:assert/strict";

import { sceltaIniziale } from "../src/sceltaSpegnimento";

const MINUTO = 60_000;

test("rientrando nella scheda la linguetta parte dal nucleo, non da «spento»", () => {
  // Nessun timer: «spento», qualunque cosa si ricordi.
  assert.equal(sceltaIniziale(null, "30"), "spento");
  // Fine del brano: lo dice il nucleo da sé.
  assert.equal(sceltaIniziale(0, null), "brano");
  assert.equal(sceltaIniziale(0, "30"), "brano");
  // Un timer da mezz'ora che corre da tre minuti resta «30», non scivola a «15».
  assert.equal(sceltaIniziale(27 * MINUTO, "30"), "30");
  // Senza ricordo: la durata più piccola che contiene quel che manca.
  assert.equal(sceltaIniziale(10 * MINUTO, null), "15");
  assert.equal(sceltaIniziale(29 * MINUTO, null), "30");
  assert.equal(sceltaIniziale(45 * MINUTO, null), "60");
});

test("un ricordo che non torna coi minuti rimasti non si usa", () => {
  // Ricordato «15» ma ne mancano 40: il timer l'ha messo qualcun altro.
  assert.equal(sceltaIniziale(40 * MINUTO, "15"), "60");
  // Ricordato «brano» o «spento» con un timer a minuti che corre.
  assert.equal(sceltaIniziale(12 * MINUTO, "brano"), "15");
  assert.equal(sceltaIniziale(12 * MINUTO, "spento"), "15");
  // Ricordato «60» e ne mancano 10: compatibile, resta «60».
  assert.equal(sceltaIniziale(10 * MINUTO, "60"), "60");
});

test("oltre l'ora la linguetta è la più lunga che c'è", () => {
  assert.equal(sceltaIniziale(90 * MINUTO, null), "60");
});
