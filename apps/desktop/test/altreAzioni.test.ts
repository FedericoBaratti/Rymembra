/**
 * Il tasto ⋮ si annuncia col titolo del brano, non col segnaposto.
 *
 * `row.more` è «Altre azioni per «{titolo}»»: chiamata senza il titolo, `t()`
 * lascia il segnaposto com'è, e TalkBack leggeva «Altre azioni per graffa
 * titolo graffa» sul tasto ⋮ di «In riproduzione» del telefono (R17-4, R16-8).
 * Qui ogni chiamata della chiave, in ogni sorgente, deve passarglielo.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const SRC = join(import.meta.dirname, "..", "src");

function sorgenti(cartella: string): string[] {
  return readdirSync(cartella, { withFileTypes: true }).flatMap((voce) => {
    const percorso = join(cartella, voce.name);
    if (voce.isDirectory()) return sorgenti(percorso);
    return /\.tsx?$/.test(voce.name) ? [percorso] : [];
  });
}

test("ogni «Altre azioni per…» dice per quale brano", () => {
  const senza: string[] = [];
  let chiamate = 0;
  for (const file of sorgenti(SRC)) {
    const testo = readFileSync(file, "utf8");
    for (const chiamata of testo.matchAll(/\bt\(\s*"row\.more"\s*(\)|,[^)]*\))/g)) {
      chiamate++;
      if (!/titolo\s*:/.test(chiamata[0] ?? "")) {
        const riga = testo.slice(0, chiamata.index).split("\n").length;
        senza.push(`${file.slice(SRC.length + 1)}:${riga}`);
      }
    }
  }
  assert.ok(chiamate >= 4, "le chiamate di row.more non si trovano più");
  assert.deepEqual(senza, [], "row.more senza il titolo del brano");
});
