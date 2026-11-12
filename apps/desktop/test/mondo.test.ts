import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import type { ArchivioPosizione } from "../src/posizione";
import { datiFinti } from "../src/studio/mondo";

/*
 * Il mondo finto della tela: le letture che le pagine fanno da sé devono
 * rispondere da qui, non dalla libreria vera.
 */

test("il testo del brano, nell'anteprima, viene dal mondo finto", async () => {
  // Il pannello del testo leggeva `ipc.testoBrano`: nello Studio chiedeva il
  // testo alla libreria vera, per un brano finto.
  const dati = datiFinti({} as ArchivioPosizione, () => ({ larghezza: 1200, altezza: 800 }));
  assert.equal(typeof dati.ipc.testoBrano, "function", "LETTURE non ha testoBrano");
  const testo = await dati.ipc.testoBrano(1);
  assert.ok(testo.righe.length > 0);
  assert.equal(testo.daChiedere, false, "l'anteprima non va in rete");
  assert.equal(testo.cercato, true);
});

test("il pannello del testo legge dalle letture del contesto, non dall'IPC", () => {
  const sorgente = readFileSync(new URL("../src/parti/Testo.tsx", import.meta.url), "utf8");
  assert.doesNotMatch(sorgente, /\bipc\s*\.\s*testoBrano\b/);
  assert.match(sorgente, /\bletture\s*\.\s*testoBrano\(/);
});

test("le Cartelle nello Studio non sondano il disco e non leggono il ricordo vero (R13b-12)", async () => {
  // La sonda chiedeva al nucleo se «D:/Musica» rispondeva — fino a otto
  // secondi su un disco che dorme — e il ricordo del pannello era quello vero:
  // la cartella scelta ieri, che nell'albero finto non c'è, e il pannello dei
  // brani restava su «Scegli una cartella».
  const dati = datiFinti({} as ArchivioPosizione, () => ({ larghezza: 1200, altezza: 800 }));
  assert.deepEqual(await dati.ipc.cartelleRadiciVive(["D:/Musica"]), [true]);
  assert.deepEqual(await dati.ipc.cartelleUi(), { aperte: [], scelta: null });
  const sorgente = readFileSync(new URL("../src/schermate/Cartelle.tsx", import.meta.url), "utf8");
  assert.doesNotMatch(sorgente, /\bipc\s*\.\s*cartelle(RadiciVive|Ui)\(/);
  assert.match(sorgente, /\bletture\s*\.\s*cartelleRadiciVive\(/);
  assert.match(sorgente, /\bletture\s*\.\s*cartelleUi\(/);
});
