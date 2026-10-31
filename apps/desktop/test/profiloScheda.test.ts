import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import * as peso from "../src/parti/pesoProfilo";

/*
 * La scheda del profilo nelle Impostazioni: il peso del file esportato, e il
 * piano che si rifà a ogni rimappatura.
 */

const MIB = 1024 * 1024;

test("un profilo sotto il mezzo megabyte non pesa «0 MB»", () => {
  const { pesoInMegabyte } = peso;
  assert.equal(typeof pesoInMegabyte, "function");
  // `null` vuol dire «meno di 1 MB».
  assert.equal(pesoInMegabyte(40_000), null);
  assert.equal(pesoInMegabyte(MIB - 1), null);
  // Sotto i dieci, un decimale; sopra, megabyte interi come prima.
  assert.equal(pesoInMegabyte(1.5 * MIB), 1.5);
  assert.equal(pesoInMegabyte(9.94 * MIB), 9.9);
  assert.equal(pesoInMegabyte(123.4 * MIB), 123);
});

test("il ripiano adotta solo la risposta dell'ultima domanda", () => {
  const sorgente = readFileSync(new URL("../src/parti/Profilo.tsx", import.meta.url), "utf8");
  const inizio = sorgente.indexOf("const ripiana = async");
  assert.ok(inizio >= 0);
  const corpo = sorgente.slice(inizio, sorgente.indexOf("const applica = async", inizio));
  // Il turno del giro (applica, «Lascia stare») non basta: due piani nello
  // stesso giro tornano in ordine qualunque, e vinceva l'ultimo ad arrivare.
  assert.match(corpo, /turnoPiano\.current\(\)/, "nessun turno per richiesta");
  assert.match(corpo, /if \(mio === giro\.current && ultimo\(\)\)/);
});
