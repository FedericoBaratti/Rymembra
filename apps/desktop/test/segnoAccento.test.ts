import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { segnoAccento } from "../src/segnoAccento";

const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

/** Tre skin come oggetti: conta l'identità, come per lo stato di React. */
const reale = { id: "plain" };
const bozza = { id: "plain" };
const scelta = { id: "mia" };

test("mentre la finestra indossa la bozza l'accento della copertina non si chiede", () => {
  // Il nucleo taglia l'accento su capacità e superfici della skin *scelta*:
  // sulla bozza in prova sarebbe il colore di un'altra skin, e una bozza con
  // `dynamicAccent: false` lo riceverebbe lo stesso.
  assert.equal(segnoAccento(bozza, bozza, 3), null);
});

test("tornati dalla prova il segno torna, e l'accento con lui", () => {
  assert.equal(segnoAccento(reale, bozza, 3), 3, "«Torna»: si rimette la skin di prima");
  // «Tienila»: la prova si chiude prima che la scelta risponda, e la finestra
  // indossa ancora la bozza. Il segno torna solo quando arriva la skin scelta:
  // prima, l'accento si calcolerebbe sulla skin scelta di prima e finirebbe
  // sopra quella nuova.
  assert.equal(segnoAccento(bozza, bozza, 3), null, "«Tienila», scelta in volo");
  assert.equal(segnoAccento(scelta, bozza, 3), 3, "«Tienila», scelta arrivata");
});

test("senza una prova il segno è il contatore, anche prima che arrivi la skin", () => {
  assert.equal(segnoAccento(reale, null, 0), 0);
  assert.equal(segnoAccento(null, null, 0), 0);
  assert.equal(segnoAccento(null, bozza, 2), 2);
});

test("la finestra passa il segno all'accento, e senza segno non lo chiede", () => {
  const skinAttiva = sorgente("skinAttiva.ts");
  assert.match(skinAttiva, /accentoDaRimettere: segnoAccento\(skinAttiva, bozzaIndossata, accentoDaRimettere\)/);
  const inizia = skinAttiva.slice(skinAttiva.indexOf("const iniziaProva"), skinAttiva.indexOf("const finisciProva"));
  assert.match(inizia, /setBozzaIndossata\(bozza\.skin\)/);

  const aspetto = sorgente("aspettoFinestra.ts");
  const effetto = aspetto.slice(aspetto.indexOf("const cambiaAccentoDinamico"), aspetto.indexOf("const mostrata"));
  const domanda = effetto.indexOf(".accentoCopertina(");
  const sospeso = effetto.indexOf("if (accentoDaRimettere === null)");
  assert.ok(domanda >= 0, "manca la domanda dell'accento");
  assert.ok(sospeso >= 0, "l'effetto dell'accento non guarda il segno");
  assert.ok(sospeso < domanda, "il segno si guarda prima di chiedere");
  assert.match(effetto.slice(sospeso, domanda), /applicaAccento\(null\);\s*return;/);
  assert.match(effetto, /\}, \[lettore, chiaro, skinAttiva, accentoDinamico, accentoDaRimettere\]\);/);
});
