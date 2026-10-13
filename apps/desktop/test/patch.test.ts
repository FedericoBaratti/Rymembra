import { test } from "node:test";
import assert from "node:assert/strict";

import {
  applicaOperazioni,
  leggi,
  rinominaChiave,
  scriviIn,
  togliDa,
  valoreIn,
} from "../src/studio/patch";

const DOC = `${JSON.stringify(
  {
    format: 2,
    id: "prova",
    tokens: { "color.accent": "#fff", "color.surface.0": "#000" },
    parts: { "section-card": { background: [{ effect: "solid", color: "#111" }] } },
  },
  null,
  2,
)}\n`;

test("scrivere crea quel che manca e lascia il resto dov'era", () => {
  const dopo = scriviIn(DOC, ["parts", "card", "radius"], "12px");
  const letto = leggi(dopo);
  assert.equal(valoreIn(letto!, ["parts", "card", "radius"]), "12px");
  assert.deepEqual(Object.keys(letto!.parts as object), ["section-card", "card"]);
});

test("una posizione dentro un elenco è una posizione", () => {
  const dopo = scriviIn(DOC, ["parts", "section-card", "background", "1"], {
    effect: "vignette",
    color: "#000",
  });
  const pila = valoreIn(leggi(dopo)!, ["parts", "section-card", "background"]);
  assert.ok(Array.isArray(pila));
  assert.equal((pila as unknown[]).length, 2);
});

test("togliere l'ultima voce toglie anche i contenitori vuoti", () => {
  const dopo = togliDa(DOC, ["parts", "section-card", "background", "0"]);
  assert.equal(valoreIn(leggi(dopo)!, ["parts"]), undefined);
});

test("__proto__ è una chiave come le altre", () => {
  const dopo = scriviIn(DOC, ["palette", "__proto__"], "#123");
  assert.match(dopo, /"__proto__": "#123"/);
});

test("un token col punto si rinomina al suo posto", () => {
  const dopo = rinominaChiave(DOC, "tokens.color.accent", "color.accent.soft");
  assert.deepEqual(Object.keys(leggi(dopo)!.tokens as object), [
    "color.accent.soft",
    "color.surface.0",
  ]);
});

test("quel che non si applica si nomina, e il resto passa", () => {
  const { testo, scartate } = applicaOperazioni(DOC, [
    { percorso: ["tokens", "color.accent"], valore: "#f00", togli: false },
    { percorso: ["parts", "section-card", "background", "5"], valore: {}, togli: false },
  ] as never);
  assert.equal(valoreIn(leggi(testo)!, ["tokens", "color.accent"]), "#f00");
  assert.equal(scartate.length, 1);
  assert.match(scartate[0]!, /buco/);
});
