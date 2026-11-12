import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { stabile } from "../src/studio/stabile";

test("uno scafale con lo stesso contenuto resta lo stesso oggetto (R14-11)", () => {
  // Ogni validazione tornava dall'IPC con un oggetto nuovo: il `memo` della
  // pagina finta saltava, e la tela ridisegnava Home, Esplora e Impostazioni
  // a ogni battuta anche quando lo scafale non era cambiato.
  const prima = { kind: "zone", children: [{ kind: "widget", name: "transport" }] };
  const uguale = JSON.parse(JSON.stringify(prima)) as typeof prima;
  assert.equal(stabile(prima, uguale), prima);
  const diverso = { kind: "zone", children: [] };
  assert.equal(stabile(prima, diverso), diverso);
  assert.equal(stabile(null, prima), prima);
  assert.equal(stabile(prima, null), null);
});

test("la tela riceve lo scafale stabilizzato (R14-11)", () => {
  const centro = readFileSync(new URL("../src/studio/Centro.tsx", import.meta.url), "utf8");
  assert.match(centro, /shell=\{scafale\}/);
  assert.match(centro, /stabile\(ultimoScafale\.current, layout\?\.shell \?\? null\)/);
});
