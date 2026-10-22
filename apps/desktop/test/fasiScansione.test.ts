/**
 * Che cosa si racconta di una scansione: le fasi della 3.0.2, il ripiego per
 * il nucleo di prima, e quel che l'annuncio dice e non dice.
 */
import { test } from "node:test";
import assert from "node:assert/strict";

import {
  annuncioFra,
  contiDellaCamminata,
  momentoDi,
  statoAnnunciato,
} from "../src/fasiScansione";
import type { Avanzamento } from "../src/ipc";

test("senza fase si fa come prima: i numeri se il totale c'è, il confronto se no", () => {
  // Il nucleo di prima, e il segnaposto che `App` mette al clic.
  assert.equal(momentoDi({ fatti: 0, totale: 0 }), "confronta");
  assert.equal(momentoDi({ fatti: 25, totale: 1464 }), "legge");
});

test("le tre fasi del nucleo nuovo", () => {
  assert.equal(
    momentoDi({ fase: "cerca", fatti: 0, totale: 0, cartelle: 120, trovati: 1500 }),
    "cerca",
  );
  assert.equal(momentoDi({ fase: "attende", fatti: 0, totale: 0, cartelle: 0, trovati: 0 }), "attende");
  assert.equal(momentoDi({ fase: "legge", fatti: 3, totale: 10, cartelle: 9, trovati: 10 }), "legge");
  // Fra la camminata e la lettura il totale è ancora zero: niente «0 / 0».
  assert.equal(momentoDi({ fase: "legge", fatti: 0, totale: 0, cartelle: 9, trovati: 10 }), "confronta");
});

test("i conti della camminata passano dal plurale di ciascuno", () => {
  const chieste: [string, unknown][] = [];
  const t = (chiave: string, valori?: Record<string, string | number>) => {
    chieste.push([chiave, valori?.["n"]]);
    return `${chiave}:${String(valori?.["n"])}`;
  };
  const scansione: Avanzamento = { fase: "cerca", fatti: 0, totale: 0, cartelle: 1, trovati: 29503 };
  assert.deepEqual(contiDellaCamminata(scansione, t), {
    cartelle: "scan.folders:1",
    file: "scan.files:29503",
  });
  // `n` numerico, così `t` sceglie «1 cartella» e non «1 cartelle».
  assert.deepEqual(chieste, [
    ["scan.folders", 1],
    ["scan.files", 29503],
  ]);
  // Senza i campi — non dovrebbe succedere, ma un nucleo a metà non deve
  // scrivere «undefined cartelle».
  assert.deepEqual(contiDellaCamminata({ fase: "cerca", fatti: 0, totale: 0 }, t), {
    cartelle: "scan.folders:0",
    file: "scan.files:0",
  });
});

test("l'annuncio dice l'inizio, l'attesa e la fine, mai i battiti", () => {
  const cerca: Avanzamento = { fase: "cerca", fatti: 0, totale: 0, cartelle: 5, trovati: 50 };
  const legge: Avanzamento = { fase: "legge", fatti: 5, totale: 50, cartelle: 5, trovati: 50 };
  const attende: Avanzamento = { fase: "attende", fatti: 0, totale: 0, cartelle: 0, trovati: 0 };
  assert.equal(statoAnnunciato(null), "ferma");
  assert.equal(statoAnnunciato(cerca), "lavora");
  assert.equal(statoAnnunciato(legge), "lavora", "dalla camminata alla lettura non cambia niente");
  assert.equal(statoAnnunciato(attende), "attende");

  // Il giro di «Scansiona» premuto durante una scansione automatica: il clic,
  // l'attesa, la scansione vera, la fine. Tre frasi, non quattro.
  const giro = [null, { fatti: 0, totale: 0 }, attende, cerca, legge, null].map(statoAnnunciato);
  const dette = giro.slice(1).map((adesso, i) => annuncioFra(giro[i] ?? "ferma", adesso));
  assert.deepEqual(dette, ["inizio", "attesa", null, null, "fine"]);

  // Una scansione che aspetta e basta, e poi finisce.
  assert.equal(annuncioFra("ferma", "attende"), "attesa");
  assert.equal(annuncioFra("attende", "ferma"), "fine");
  assert.equal(annuncioFra("lavora", "lavora"), null);
});
