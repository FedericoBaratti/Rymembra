import { test } from "node:test";
import assert from "node:assert/strict";

import {
  annulla,
  cosaAnnulla,
  cosaRipete,
  DURATA_RAFFICA_MS,
  PAUSA_MS,
  PROFONDITA,
  ripeti,
  scrivi,
  storiaDa,
} from "../src/studio/storia";

const passo = (testo: string, gruppo: string, adesso: number) => ({
  testo,
  etichetta: `scrivo ${testo}`,
  gruppo,
  adesso,
});

test("una raffica dello stesso gruppo è un passo solo", () => {
  let s = storiaDa("a");
  s = scrivi(s, passo("b", "raggio", 1000));
  s = scrivi(s, passo("c", "raggio", 1100));
  s = scrivi(s, passo("d", "raggio", 1200));
  assert.equal(s.passato.length, 1);
  assert.equal(s.presente.testo, "d");
  assert.equal(cosaAnnulla(s), "scrivo d");
  assert.equal(annulla(s).presente.testo, "a");
});

test("una pausa o un gruppo diverso aprono un passo", () => {
  let s = storiaDa("a");
  s = scrivi(s, passo("b", "raggio", 1000));
  s = scrivi(s, passo("c", "raggio", 1000 + PAUSA_MS + 1));
  s = scrivi(s, passo("d", "colore", 1000 + PAUSA_MS + 2));
  assert.deepEqual(
    s.passato.map((p) => p.testo),
    ["a", "b", "c"],
  );
});

test("annullare e ripetere tornano dove si era, una volta sola", () => {
  let s = storiaDa("a");
  s = scrivi(s, passo("b", "x", 1));
  s = scrivi(s, passo("c", "y", 2));
  s = annulla(s);
  assert.equal(s.presente.testo, "b");
  assert.equal(cosaRipete(s), "scrivo c");
  s = ripeti(s);
  assert.equal(s.presente.testo, "c");
  assert.equal(ripeti(s), s, "niente da ripetere: la stessa storia");
});

test("dopo un annullo la scrittura non si accorpa al passo disfatto", () => {
  let s = storiaDa("a");
  s = scrivi(s, passo("b", "x", 1));
  s = annulla(s);
  s = scrivi(s, passo("c", "x", 2));
  assert.deepEqual(s.passato.map((p) => p.testo), ["a"]);
  assert.equal(s.futuro.length, 0, "scrivere dopo un annullo butta il futuro");
});

test("una scrittura che non cambia niente non è un passo", () => {
  const s = storiaDa("a");
  assert.equal(scrivi(s, passo("a", "x", 1)), s);
});

test("i passi più vecchi cadono oltre la profondità", () => {
  let s = storiaDa("0");
  for (let i = 1; i <= PROFONDITA + 20; i += 1) s = scrivi(s, passo(String(i), String(i), i));
  assert.equal(s.passato.length, PROFONDITA);
  assert.equal(s.passato[0]?.testo, "20");
});

test("una battitura senza pause si spezza in passi di qualche secondo", () => {
  // Un minuto di codice scritto di seguito non è un passo solo: annullare
  // toglie gli ultimi istanti, non tutto il minuto.
  let s = storiaDa("");
  let testo = "";
  for (let t = 0; t <= 6_000; t += 100) {
    testo += "x";
    s = scrivi(s, passo(testo, "codice", 1_000 + t));
  }
  assert.ok(s.passato.length >= 3, `passi: ${s.passato.length}`);
  const dopo = annulla(s);
  assert.ok(
    testo.length - dopo.presente.testo.length <= DURATA_RAFFICA_MS / 100 + 1,
    "un annullo toglie al più una raffica",
  );
});
