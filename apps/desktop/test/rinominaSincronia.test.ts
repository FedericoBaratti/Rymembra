import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * R13-3 (decisione 27): «Rinomina» su un dispositivo della sincronia non dà
 * fiducia. Il campo del nome passava da `onAccoppia`, cioè da «Accetta».
 */

const sorgente = readFileSync(new URL("../src/parti/Dispositivi.tsx", import.meta.url), "utf8");

test("il nome di un dispositivo della sincronia non passa da «Accetta»", () => {
  const inizio = sorgente.indexOf("function RigaSincronia(");
  assert.ok(inizio >= 0);
  const corpo = sorgente.slice(inizio, sorgente.indexOf("export function SchedaDispositivi", inizio));
  const campo = corpo.slice(corpo.indexOf("onFine="), corpo.indexOf("<Etichette"));
  assert.doesNotMatch(campo, /onAccoppia/, "rinominare accetterebbe il dispositivo");
  assert.match(campo, /ipc\.sincroniaRinomina\(/);
});

test("il comando di rinomina esiste da tutte e due le parti", () => {
  const ipc = readFileSync(new URL("../src/ipc.ts", import.meta.url), "utf8");
  assert.match(ipc, /sincroniaRinomina: \(id: string, nome: string \| null\) =>/);
  assert.match(ipc, /"sincronia_rinomina"/);
  const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
  assert.match(lib, /sincronia::sincronia_rinomina,/);
});
