import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { regioneDelleDate } from "../src/lingue/regione";

/*
 * Le date e le ore inglesi seguono la regione del sistema quando la lingua
 * coincide (D-42): `en` da solo dava le date americane anche a chi ha il
 * sistema in inglese britannico.
 */

test("con la stessa lingua le date seguono la regione del sistema", () => {
  assert.equal(regioneDelleDate("en", ["en-GB", "it-IT"]), "en-GB");
  assert.equal(regioneDelleDate("en", ["it-IT", "en-AU"]), "en-AU");
  assert.equal(regioneDelleDate("it", ["it-CH"]), "it-CH");
  // E una data del 10 novembre si legge come tale.
  const dieciNovembre = Date.UTC(2026, 10, 10, 12);
  assert.equal(
    new Date(dieciNovembre).toLocaleDateString(regioneDelleDate("en", ["en-GB"]), { timeZone: "UTC" }),
    "10/11/2026",
  );
});

test("con una lingua diversa, o senza regione, resta quella dell'interfaccia", () => {
  assert.equal(regioneDelleDate("it", ["de-DE", "en-US"]), "it");
  assert.equal(regioneDelleDate("en", ["en"]), "en");
  assert.equal(regioneDelleDate("en", []), "en");
});

test("le date e le ore passano dalla regione, i numeri no", () => {
  const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");
  const formato = sorgente("formato.ts");
  for (const funzione of ["export function dataOra(", "export function dataOraBreve(", "export function data("]) {
    const corpo = formato.slice(formato.indexOf(funzione), formato.indexOf("\n}\n", formato.indexOf(funzione)));
    assert.match(corpo, /localeDate\(\)/, funzione);
  }
  const numero = formato.slice(formato.indexOf("export function numero("));
  assert.match(numero.slice(0, numero.indexOf("\n}\n")), /\blocale\(\)/);
  const cronologia = sorgente("parti/Cronologia.tsx");
  assert.doesNotMatch(cronologia, /toLocale(Date|Time)String\(locale\(\)/);
  const lingue = sorgente("lingue/index.ts");
  assert.match(lingue, /regioneDelleDate\(attiva, /);
});
