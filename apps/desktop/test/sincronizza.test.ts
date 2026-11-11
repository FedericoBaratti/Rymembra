import { test } from "node:test";
import assert from "node:assert/strict";

import { chiusuraLibera, gestoDelTasto } from "../src/tastiSincronizza";

test("con battute fatte Escape non chiude senza chiedere", () => {
  assert.equal(chiusuraLibera("battute", [1200], []), false);
  assert.equal(chiusuraLibera("rivedi", [1200, 3400], []), false);
  assert.equal(chiusuraLibera("parole", [1200], [1300]), false);
  // Niente da perdere: le righe non battute, una battuta non ancora data, il
  // lavoro già salvato.
  assert.equal(chiusuraLibera("righe", [], []), true);
  assert.equal(chiusuraLibera("battute", [], []), true);
  assert.equal(chiusuraLibera("dono", [1200], [1300]), true);
});

test("i tasti delle battute", () => {
  const normale = { raddrizzo: false, tutteBattute: false };
  assert.equal(gestoDelTasto(" ", normale), "batti");
  assert.equal(gestoDelTasto("Backspace", normale), "rifai");
  assert.equal(gestoDelTasto("Enter", normale), null);
  assert.equal(gestoDelTasto("Enter", { raddrizzo: false, tutteBattute: true }), "finisci");
  assert.equal(gestoDelTasto("a", normale), null);
});

test("durante il raddrizzamento Invio, Backspace e Spazio si prendono e non fanno niente", () => {
  const raddrizza = { raddrizzo: true, tutteBattute: true };
  assert.equal(gestoDelTasto("Enter", raddrizza), "niente");
  assert.equal(gestoDelTasto("Backspace", raddrizza), "niente");
  assert.equal(gestoDelTasto(" ", raddrizza), "niente");
  assert.equal(gestoDelTasto("a", raddrizza), null);
});

test("finite le battute, Spazio e Invio su un tasto sono del tasto (R13c-3)", () => {
  // Chi usa solo la tastiera batte tutto, poi va con Tab su «Salva» e preme
  // Invio: partiva «Raddrizza», a ogni Invio, e Spazio diventava una battuta
  // oltre la fine. Su un tasto, finite le battute, il gesto è il suo.
  const finite = { raddrizzo: false, tutteBattute: true };
  assert.equal(gestoDelTasto("Enter", { ...finite, su: "tasto" }), null);
  assert.equal(gestoDelTasto(" ", { ...finite, su: "tasto" }), null);
  // Fuori dai tasti Invio resta «ho finito».
  assert.equal(gestoDelTasto("Enter", { ...finite, su: "radice" }), "finisci");
});

test("mentre si batte Spazio resta la battuta anche sul tasto premuto col mouse", () => {
  // Un clic su «Suona» lascia il fuoco su quel tasto: se Spazio fosse suo, la
  // battuta dopo metterebbe in pausa invece di segnare la riga.
  const battendo = { raddrizzo: false, tutteBattute: false };
  assert.equal(gestoDelTasto(" ", { ...battendo, su: "tasto" }), "batti");
  // Invio su un tasto lo attiva, come prima.
  assert.equal(gestoDelTasto("Enter", { ...battendo, su: "tasto" }), null);
  // Backspace resta «rifai» su un tasto; dentro un campo è del campo.
  assert.equal(gestoDelTasto("Backspace", { ...battendo, su: "tasto" }), "rifai");
  assert.equal(gestoDelTasto("Backspace", { ...battendo, su: "campo" }), null);
  assert.equal(gestoDelTasto(" ", { ...battendo, su: "campo" }), null);
});

test("l'editor passa dove sta il fuoco, e finite le battute lo porta su «Raddrizza»", async () => {
  const { readFileSync } = await import("node:fs");
  const sorgente = readFileSync(new URL("../src/Sincronizza.tsx", import.meta.url), "utf8");
  assert.match(sorgente, /su: bersaglioDi\(evento\.target\)/);
  assert.match(sorgente, /if \(tutteBattute\) tastoRaddrizza\.current\?\.focus\(\);/);
  assert.equal(sorgente.split("ref={tastoRaddrizza}").length - 1, 2, "i due «Raddrizza»");
});
