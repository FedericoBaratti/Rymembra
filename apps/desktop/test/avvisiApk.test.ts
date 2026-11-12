/**
 * Gli avvisi di licenza viaggiano dentro l'APK.
 *
 * L'installer del computer li porta accanto all'eseguibile
 * (`tauri.conf.json`, `bundle.resources`); l'APK no, e sul telefono c'era solo
 * un link a GitHub, che senza rete non si apre e porta al file di `main`, non a
 * quello della versione installata (R19-3). La MIT chiede l'avviso «in all
 * copies», l'Apache-2.0 una copia della licenza, la OFL la licenza accanto ai
 * font. `app/build.gradle.kts` li mette fra gli asset di ogni variante, in
 * `legale/`, dagli stessi file del computer.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";

const gradle = readFileSync(
  new URL("../src-tauri/gen/android/app/build.gradle.kts", import.meta.url),
  "utf8",
).replace(/\/\/.*$/gm, "");

test("l'APK porta la licenza, gli avvisi di terzi, la privacy, i termini e la licenza dei font", () => {
  assert.match(gradle, /addGeneratedSourceDirectory\(/, "nessuna cartella di asset generata");
  for (const [sorgente, nome] of [
    ["../../../../../LICENSE", "LICENSE.txt"],
    ["../../../../../THIRD-PARTY-NOTICES.md", "THIRD-PARTY-NOTICES.md"],
    ["../../../../../PRIVACY.md", "PRIVACY.md"],
    ["../../../../../TERMS.md", "TERMS.md"],
    ["../../../src/font/OFL.txt", "font-OFL.txt"],
  ]) {
    assert.ok(gradle.includes(`"${sorgente}"`), `build.gradle.kts non prende ${sorgente}`);
    assert.ok(gradle.includes(`"${nome}"`), `build.gradle.kts non scrive ${nome}`);
    // Lo stesso file che il computer impacchetta, e che c'è davvero.
    const vero = new URL(`../src-tauri/gen/android/${sorgente}`, import.meta.url);
    assert.ok(existsSync(vero), `${sorgente} non esiste dalla cartella del progetto Gradle`);
  }
});
