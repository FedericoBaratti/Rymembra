import { test } from "node:test";
import assert from "node:assert/strict";

import { chiAnnulla, chiRipete, haAnnulloProprio, prendeITasti, type Bersaglio } from "../src/studio/annullo";

const campo = (tag: string, tipo: string | null = null, modificabile = false): Bersaglio => ({
  tag,
  tipo,
  modificabile,
});

test("haAnnulloProprio: range, checkbox, color e select non hanno annullo proprio", () => {
  // Il gesto più frequente dello Studio è trascinare un cursore o scegliere
  // in un elenco: il fuoco resta lì, e Ctrl+Z deve arrivare allo Studio.
  assert.equal(haAnnulloProprio(campo("INPUT", "range")), false);
  assert.equal(haAnnulloProprio(campo("INPUT", "checkbox")), false);
  assert.equal(haAnnulloProprio(campo("INPUT", "radio")), false);
  assert.equal(haAnnulloProprio(campo("INPUT", "color")), false);
  assert.equal(haAnnulloProprio(campo("INPUT", "button")), false);
  assert.equal(haAnnulloProprio(campo("SELECT")), false);
  assert.equal(haAnnulloProprio(campo("BUTTON")), false);
  assert.equal(haAnnulloProprio(null), false);
});

test("haAnnulloProprio: i campi di testo tengono il loro Ctrl+Z", () => {
  for (const tipo of ["text", "search", "number", "email", "url", "password", "tel"]) {
    assert.equal(haAnnulloProprio(campo("INPUT", tipo)), true, tipo);
  }
  assert.equal(haAnnulloProprio(campo("TEXTAREA")), true);
  assert.equal(haAnnulloProprio(campo("DIV", null, true)), true);
});

test("prendeITasti: i tasti singoli restano a ogni controllo, select compresa", () => {
  // In una <select> le lettere cercano fra le opzioni: «f» o «1» non devono
  // diventare scorciatoie dello Studio.
  assert.equal(prendeITasti(campo("SELECT")), true);
  assert.equal(prendeITasti(campo("INPUT", "range")), true);
  assert.equal(prendeITasti(campo("INPUT", "text")), true);
  assert.equal(prendeITasti(campo("TEXTAREA")), true);
  assert.equal(prendeITasti(campo("DIV", null, true)), true);
  assert.equal(prendeITasti(campo("BUTTON")), false);
  assert.equal(prendeITasti(null), false);
});

test("codice di una parte: Ctrl+Z con una battuta in attesa la butta, e non tocca la storia", () => {
  // La battuta vive mezzo secondo solo nell'editor: annullare lo Studio
  // disfaceva un altro passo, e poi il timer la scriveva sul documento
  // annullato, svuotando il «ripeti».
  assert.equal(chiAnnulla('{ "radius": "12px" }', "{}\n"), "locale");
  assert.equal(chiAnnulla("{}\n", "{}\n"), "studio");
});

test("codice di una parte: Ctrl+Y con una battuta in attesa non fa niente", () => {
  // Rifare un passo sotto una battuta che sta per essere scritta la
  // scriverebbe su un altro documento; buttarla sarebbe una perdita.
  assert.equal(chiRipete('{ "radius": "12px" }', "{}\n"), "nessuno");
  assert.equal(chiRipete("{}\n", "{}\n"), "studio");
});
