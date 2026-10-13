import { test } from "node:test";
import assert from "node:assert/strict";

import { json } from "@codemirror/lang-json";
import { ensureSyntaxTree } from "@codemirror/language";
import { EditorState } from "@codemirror/state";

import type { Registro } from "../src/ipc";
import { CompletionContext } from "@codemirror/autocomplete";

import { chiaviPer, completamento, postoA, valoriPer, type Fonti } from "../src/studio/codice";

const REGISTRO = {
  tokens: [
    { id: "color.accent", kind: "color" },
    { id: "shadow.2", kind: "shadow" },
  ],
  parts: [{ name: "card" }, { name: "heart" }],
  stati: [{ name: "hover" }, { name: "current" }],
  inneschi: [{ name: "enter" }, { name: "hover" }],
  effects: [{ name: "solid", target: "background", params: [{ name: "color" }] }],
  widgets: [],
  sorgenti: ["albumArt.vibrant"],
  chiavi: {
    documento: ["format", "id", "parts"],
    aspetto: ["background", "textColor", "radius"],
    strutturaParte: ["before", "after", "states", "motion"],
    movimentoParte: ["stagger", "transition"],
    angoli: ["topLeft", "topRight"],
  },
  parole: { easing: ["linear", "ease-out"], borderStyle: ["solid", "dashed"] },
} as unknown as Registro;

const DOC = { motion: { animations: { sale: {} }, easings: { puntina: {} } }, palette: { etichetta: "#a3301c" } };

const fonti: Fonti = {
  registro: REGISTRO,
  documento: () => DOC,
  file: () => ["fraunces.woff2"],
  radice: [],
  descrizioneParte: (_n, r) => r,
  descrizioneToken: (_n, r) => r,
};

/** Lo stato di un testo, col cursore dove sta la `|`. */
function con(testo: string): { state: EditorState; pos: number } {
  const pos = testo.indexOf("|");
  const state = EditorState.create({ doc: testo.replace("|", ""), extensions: [json()] });
  ensureSyntaxTree(state, state.doc.length, 5000);
  return { state, pos };
}

test("dentro una chiave: il percorso è quello dell'oggetto", () => {
  const { state, pos } = con('{ "parts": { "card": { "rad|": 1 } } }');
  const posto = postoA(state, pos);
  assert.equal(posto?.ruolo, "chiave");
  assert.deepEqual(posto?.percorso, ["parts", "card"]);
  assert.ok(chiaviPer(posto?.percorso ?? [], fonti, null).includes("radius"));
});

test("una chiave appena aperta, prima che l'analizzatore la chiuda", () => {
  const { state, pos } = con('{ "parts": { "card": { "states": { "|');
  const posto = postoA(state, pos);
  assert.equal(posto?.ruolo, "chiave");
  assert.deepEqual(posto?.percorso, ["parts", "card", "states"]);
  assert.deepEqual(chiaviPer(posto?.percorso ?? [], fonti, null), ["hover", "current"]);
});

test("dentro un valore: la chiave decide che cosa ci sta", () => {
  const { state, pos } = con('{ "parts": { "card": { "motion": { "enter": "s|" } } } }');
  const posto = postoA(state, pos);
  assert.equal(posto?.ruolo, "valore");
  assert.equal(posto?.chiave, "enter");
  assert.deepEqual(valoriPer(posto?.percorso ?? [], posto?.chiave ?? null, fonti), ["sale"]);
});

test("un riferimento propone i nomi giusti", () => {
  const { state, pos } = con('{ "parts": { "card": { "textColor": { "$palette": "|" } } } }');
  const posto = postoA(state, pos);
  assert.deepEqual(valoriPer(posto?.percorso ?? [], posto?.chiave ?? null, fonti), ["etichetta"]);
  assert.deepEqual(valoriPer(["parts", "card", "shadow"], "$token", fonti), ["shadow.2"]);
  assert.deepEqual(valoriPer(["motion", "animations", "sale"], "easing", fonti), ["linear", "ease-out", "puntina"]);
});

test("un livello di sfondo propone i parametri del suo effetto", () => {
  assert.deepEqual(chiaviPer(["parts", "card", "background", "0"], fonti, "solid"), ["effect", "color"]);
  assert.deepEqual(chiaviPer(["parts", "card", "background", "0"], fonti, null), ["effect", "$pattern"]);
  assert.deepEqual(chiaviPer(["parts", "card", "radius"], fonti, null), ["topLeft", "topRight"]);
});

test("un documento a metà: l'albero si perde, il percorso no", () => {
  // Una chiave nuova senza la virgola dopo: l'analizzatore butta via
  // l'oggetto, la lettura in avanti no.
  const { state, pos } = con(
    ["{", '  "parts": {', '    "he|"', '    "card": { "radius": "4px" }', "  }", "}"].join("\n"),
  );
  const posto = postoA(state, pos);
  assert.equal(posto?.ruolo, "chiave");
  assert.deepEqual(posto?.percorso, ["parts"]);
});

test("una voce di un elenco prende la chiave dell'elenco", () => {
  const { state, pos } = con('{ "fonts": {}, "parts": { "card": { "background": [ { "effect": "so|" } ] } } }');
  const posto = postoA(state, pos);
  assert.equal(posto?.ruolo, "valore");
  assert.equal(posto?.chiave, "effect");
  assert.deepEqual(posto?.percorso, ["parts", "card", "background", "0", "effect"]);
  assert.deepEqual(valoriPer(posto?.percorso ?? [], posto?.chiave ?? null, fonti), ["solid"]);
});

test("dentro una stringa si completa dopo la virgoletta", () => {
  // CodeMirror filtra le voci con quel che si è battuto: con la virgoletta
  // dentro, nessun nome combaciava e l'elenco restava vuoto.
  const { state, pos } = con('{ "parts": { "he|" } }');
  const esito = completamento(() => fonti)(new CompletionContext(state, pos, false));
  assert.ok(esito);
  assert.equal(state.sliceDoc(esito.from, pos), "he");
  assert.ok(esito.options.some((o) => o.label === "heart"));
});

