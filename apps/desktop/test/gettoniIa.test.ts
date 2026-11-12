import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import {
  GETTONI_DI_SERIE,
  GETTONI_MASSIMI,
  GETTONI_MINIMI,
  campoDeiGettoni,
  gettoniDalCampo,
} from "../src/parti/gettoniIa";

/*
 * Il tetto dei gettoni di un profilo IA (R08-3, D-64): 4096 di serie,
 * modificabile nel profilo. Il campo vuoto vale il tetto di serie; quel che
 * non è un intero dentro i bordi del nucleo non si salva.
 */

test("il campo vuoto vale il tetto di serie, un numero dentro i bordi passa", () => {
  assert.equal(gettoniDalCampo(""), null);
  assert.equal(gettoniDalCampo("  "), null);
  assert.equal(gettoniDalCampo("8192"), 8192);
  assert.equal(gettoniDalCampo(` ${GETTONI_MINIMI} `), GETTONI_MINIMI);
  assert.equal(gettoniDalCampo(String(GETTONI_MASSIMI)), GETTONI_MASSIMI);
});

test("quel che non è un intero dentro i bordi non si salva", () => {
  for (const sbagliato of ["63", String(GETTONI_MASSIMI + 1), "4k", "-5", "1.5", "1e3"]) {
    assert.equal(gettoniDalCampo(sbagliato), undefined, sbagliato);
  }
});

test("un profilo salvato col tetto di serie riapre il campo vuoto", () => {
  assert.equal(campoDeiGettoni(null), "");
  assert.equal(campoDeiGettoni(undefined), "");
  assert.equal(campoDeiGettoni(2048), "2048");
});

test("i bordi sono quelli del nucleo", () => {
  const profilo = readFileSync(
    new URL("../../../core/rymembra-ia/src/profilo.rs", import.meta.url),
    "utf8",
  );
  assert.match(profilo, new RegExp(`GETTONI_DI_SERIE: u32 = ${GETTONI_DI_SERIE};`));
  assert.match(
    profilo,
    new RegExp(`GETTONI_AMMESSI: std::ops::RangeInclusive<u32> = ${GETTONI_MINIMI}..=262_144;`),
  );
  assert.equal(GETTONI_MASSIMI, 262_144);
});

test("il modulo dei modelli manda il tetto al nucleo", () => {
  const modulo = readFileSync(new URL("../src/parti/ModelliIA.tsx", import.meta.url), "utf8");
  assert.match(modulo, /gettoniMassimi: gettoniDalCampo\(bozza\.gettoni\)/);
  assert.match(modulo, /campoDeiGettoni\(profilo\.gettoniMassimi\)/);
});
