import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import type { VocePreset } from "../src/ipc";
import * as eq from "../src/eq";

/*
 * L'equalizzatore: la tendina delle curve e i decibel scritti accanto ai
 * cursori. Le regole stanno in `eq.ts`, senza lingue e senza nucleo.
 */

const voce = (nome: string, diSerie: boolean, primo = 0): VocePreset => ({
  nome,
  diSerie,
  guadagni: [primo, 0, 0, 0, 0, 0, 0, 0, 0, 0],
});

test("una curva salvata col nome di una di serie si sceglie per quella che è", () => {
  assert.equal(typeof eq.trovaPreset, "function");
  const elenco = [voce("Rock", true, 5), voce("Rock", false, 2)];
  // La chiave della voce dell'utente porta a lei, non a quella di serie.
  const mia = eq.trovaPreset(elenco, eq.chiavePreset(elenco[1]!));
  assert.equal(mia?.diSerie, false);
  assert.equal(mia?.guadagni[0], 2);
  // E le due voci hanno valori diversi nella tendina.
  assert.notEqual(eq.chiavePreset(elenco[0]!), eq.chiavePreset(elenco[1]!));
  assert.equal(eq.trovaPreset(elenco, "u:Pop"), undefined);
});

test("la tendina usa la chiave e non il nome", () => {
  const sorgente = readFileSync(
    new URL("../src/parti/Equalizzatore.tsx", import.meta.url),
    "utf8",
  );
  assert.ok(!sorgente.includes("value={p.nome}"), "le opzioni hanno ancora il nome come valore");
  assert.ok(!sorgente.includes("p.nome === nome"), "la scelta cerca ancora per nome");
});

/** `numero()` in piccolo, con la virgola italiana. */
const italiano = (n: number, decimali: number) =>
  n.toLocaleString("it-IT", { minimumFractionDigits: decimali, maximumFractionDigits: decimali });

test("i decibel si scrivono con la virgola della lingua e il segno meno vero", () => {
  assert.equal(typeof eq.decibel, "function");
  assert.equal(eq.decibel(1.5, italiano), "+1,5");
  assert.equal(eq.decibel(-2.5, italiano), "−2,5");
  assert.equal(eq.decibel(3, italiano), "+3");
  assert.equal(eq.decibel(-12, italiano), "−12");
  assert.equal(eq.decibel(0, italiano), "0");
});

test("ogni curva di serie ha il suo nome in tutte e due le lingue", () => {
  const rust = readFileSync(
    new URL("../../../core/rymembra-play/src/equalizzatore.rs", import.meta.url),
    "utf8",
  );
  const tabella = rust.slice(rust.indexOf("pub const PRESET_DI_SERIE"));
  const nomi = [...tabella.slice(0, tabella.indexOf("];")).matchAll(/\(\s*"([^"]+)"/g)].map(
    (m) => m[1]!,
  );
  assert.ok(nomi.length >= 9, `curve trovate: ${nomi.join(", ")}`);
  for (const lingua of ["it", "en"]) {
    const testi = JSON.parse(
      readFileSync(new URL(`../src/lingue/${lingua}.json`, import.meta.url), "utf8"),
    ) as Record<string, string>;
    for (const nome of nomi) {
      assert.ok(eq.chiaveNomePreset(nome) in testi, `${lingua}: manca ${eq.chiaveNomePreset(nome)}`);
    }
  }
});
