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

test("le ragioni degli scarti si traducono: la frase italiana è il modello del catalogo (D-68)", async () => {
  // L'elenco «scartate» sotto una proposta della chat era sempre in italiano.
  // Adesso ogni ragione porta la chiave `studio3.reject.*` e i suoi valori; la
  // frase italiana — quella che torna al modello — è la riga di `it.json` coi
  // valori al loro posto, e l'inglese ha gli stessi segnaposto.
  const { readFileSync } = await import("node:fs");
  const catalogo = (lingua: string) =>
    JSON.parse(readFileSync(new URL(`../src/lingue/${lingua}.json`, import.meta.url), "utf8")) as Record<string, string>;
  const it = catalogo("it");
  const en = catalogo("en");
  type M = { chiave: string; testo: string; valori: Record<string, string | M> };
  const segnaposto = (frase: string) => [...frase.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
  const controlla = (m: M) => {
    const modello = it[m.chiave];
    assert.ok(modello !== undefined, `it.json: manca ${m.chiave}`);
    assert.ok(en[m.chiave] !== undefined, `en.json: manca ${m.chiave}`);
    assert.deepEqual(segnaposto(en[m.chiave] ?? ""), segnaposto(modello ?? ""), m.chiave);
    const pieno = (modello ?? "").replace(/\{(\w+)\}/g, (_, n: string) => {
      const v = m.valori[n];
      if (typeof v === "string") return v;
      assert.ok(v !== undefined, `${m.chiave}: manca {${n}}`);
      controlla(v);
      return v.testo;
    });
    assert.equal(pieno, m.testo, m.chiave);
  };
  const doc = JSON.stringify({ parts: { card: { background: [{ $pattern: "x" }] } }, tokens: { colore: "#fff" } });
  const { motivi, scartate } = applicaOperazioni(doc, [
    { togli: false, percorso: [], valore: 1 },
    { togli: false, percorso: ["tokens", "colore", "dentro"], valore: 1 },
    { togli: false, percorso: ["parts", "card", "background", "x"], valore: 1 },
    { togli: false, percorso: ["parts", "card", "background", "5"], valore: 1 },
    { togli: true, percorso: ["parts", "nessuna", "background"], valore: undefined },
    { togli: true, percorso: ["parts", "card", "background", "x"], valore: undefined },
    { togli: true, percorso: ["parts", "card", "background", "7"], valore: undefined },
    { togli: true, percorso: ["parts", "card", "manca"], valore: undefined },
  ]);
  assert.equal(motivi.length, 8);
  assert.deepEqual(motivi.map((m) => m.testo), scartate, "le due forme dicono la stessa cosa");
  for (const m of motivi) controlla(m as M);
  for (const m of applicaOperazioni("{ rotto", [{ togli: true, percorso: ["x"], valore: undefined }]).motivi) controlla(m as M);
});
