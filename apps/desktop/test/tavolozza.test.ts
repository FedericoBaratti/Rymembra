import { test } from "node:test";
import assert from "node:assert/strict";

import { operazioniTavolozza, propostaPer, type PropostaTavolozza } from "../src/studio/tavolozza";

const scura: PropostaTavolozza = {
  voci: [{ token: "color.surface", valore: "#101014" }],
  tema: "base",
  scura: true,
  daCopertina: false,
};

test("una tavolozza chiesta per il tema scuro non vale guardando la variante chiara", () => {
  // Il generatore restava a schermo dopo «Guarda la variante», e «Applica»
  // scriveva superfici scure in variant.tokens con l'etichetta «chiaro».
  assert.equal(propostaPer(scura, "variante", false), null);
  assert.equal(propostaPer(scura, "base", false), null, "scheme cambiato sotto la proposta");
  assert.equal(propostaPer(scura, "base", true), scura);
  assert.equal(propostaPer(null, "base", true), null);
});

test("le scritture vanno al tema per cui la tavolozza è stata chiesta", () => {
  assert.deepEqual(operazioniTavolozza(scura), [
    { togli: false, percorso: ["tokens", "color.surface"], valore: "#101014" },
  ]);
  assert.deepEqual(operazioniTavolozza({ ...scura, tema: "variante", scura: false })[0]?.percorso, [
    "variant",
    "tokens",
    "color.surface",
  ]);
});
