import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * R10-6: «nessun modello è configurato» copriva cinque casi diversi. I quattro
 * che non lo erano hanno un codice e una frase loro; quella del tetto dei
 * profili scrive il numero per esteso, perché l'errore non porta valori alla
 * finestra: deve restare lo stesso numero del nucleo.
 */

const lingua = (nome: string): Record<string, string> =>
  JSON.parse(readFileSync(new URL(`../src/lingue/${nome}.json`, import.meta.url), "utf8"));

test("i quattro casi hanno la loro frase in tutte e due le lingue", () => {
  for (const nome of ["it", "en"]) {
    const testi = lingua(nome);
    for (const codice of ["profileIncomplete", "keyMissing", "keyForOtherOrigin", "tooManyProfiles"]) {
      const frase = testi[`errors.ia.${codice}`];
      assert.ok(frase !== undefined && frase !== testi["errors.ia.notConfigured"], `${nome}: ${codice}`);
    }
  }
});

test("il tetto dei profili nella frase è quello del nucleo", () => {
  const ia = readFileSync(new URL("../src-tauri/src/ia.rs", import.meta.url), "utf8");
  const tetto = /const PROFILI_MASSIMI: usize = (\d+);/.exec(ia)?.[1];
  assert.ok(tetto !== undefined, "PROFILI_MASSIMI");
  for (const nome of ["it", "en"]) {
    assert.match(lingua(nome)["errors.ia.tooManyProfiles"] ?? "", new RegExp(`\\b${tetto}\\b`), nome);
  }
});
