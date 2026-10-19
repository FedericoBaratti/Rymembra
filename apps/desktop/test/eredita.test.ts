import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";

import {
  CARTELLA_DELLA_1_0,
  ESTENSIONE_PROFILO_PRECEDENTE,
  ESTENSIONE_SKIN_PRECEDENTE,
  LINGUAGGIO_PATCH_PRECEDENTE,
} from "../src/eredita";
import { LINGUAGGIO_PATCH } from "../src/studio/prompt";

/** Il sorgente di un file della finestra, come testo. */
function sorgente(percorso: string): string {
  return readFileSync(new URL(`../src/${percorso}`, import.meta.url), "utf8");
}

/** Tutti i `.ts` e `.tsx` sotto `src/`, col percorso relativo a `src/`. */
function sorgenti(): string[] {
  const radice = new URL("../src/", import.meta.url);
  return readdirSync(radice, { recursive: true, encoding: "utf8" })
    .filter((p) => /\.tsx?$/.test(p))
    .map((p) => p.replaceAll("\\", "/"));
}

// Questo è l'unico posto, con i test di compatibilità, in cui i nomi di prima
// si riscrivono per esteso: se una costante cambia per sbaglio, i file sul
// disco di chi ha aggiornato smettono di leggersi, e deve saltare qui.
test("le costanti dell'eredità sono i nomi di prima, e nessuna è quella di adesso", () => {
  assert.equal(LINGUAGGIO_PATCH_PRECEDENTE, "aether-patch");
  assert.equal(ESTENSIONE_SKIN_PRECEDENTE, "aeskin");
  assert.equal(ESTENSIONE_PROFILO_PRECEDENTE, "aeprofile");
  assert.equal(CARTELLA_DELLA_1_0, "Aether");

  assert.equal(LINGUAGGIO_PATCH, "rymembra-patch");
  assert.notEqual(LINGUAGGIO_PATCH_PRECEDENTE, LINGUAGGIO_PATCH);
  // Le estensioni sono senza punto, come le vuole il dialogo di scelta file.
  assert.ok(!ESTENSIONE_SKIN_PRECEDENTE.startsWith("."));
  assert.ok(!ESTENSIONE_PROFILO_PRECEDENTE.startsWith("."));
});

test("i nomi di prima stanno solo in eredita.ts, tutto il resto usa le costanti", () => {
  const vecchi = /aether|aeskin|aeprofile/i;
  const trovati = sorgenti()
    .filter((p) => p !== "eredita.ts")
    .filter((p) => vecchi.test(sorgente(p)));
  assert.deepEqual(trovati, []);
});

test("l'importazione accetta le estensioni di prima, il salvataggio scrive solo le nuove", () => {
  const skinAttiva = sorgente("skinAttiva.ts");
  assert.match(skinAttiva, /extensions: \["ryskin", ESTENSIONE_SKIN_PRECEDENTE\]/);

  const profilo = sorgente("parti/Profilo.tsx");
  assert.match(profilo, /extensions: \["ryprofile", ESTENSIONE_PROFILO_PRECEDENTE, "json"\]/);
  // Il dialogo di salvataggio, accanto, ha la sola estensione nuova.
  assert.match(profilo, /defaultPath: "rymembra-profilo\.ryprofile"/);
  assert.match(profilo, /filters: \[\{ name: t\("profile\.file"\), extensions: \["ryprofile"\] \}\]/);

  const guscio = sorgente("studio/Guscio.tsx");
  assert.match(guscio, /defaultPath: `\$\{idBozza\.current\}\.ryskin`/);
  assert.match(guscio, /extensions: \["ryskin"\]/);
});

test("la cartella della 1.0 viene dalla costante, e non è scritta nel dialogo", () => {
  const app = sorgente("App.tsx");
  assert.ok(app.includes("`${roaming}\\\\${CARTELLA_DELLA_1_0}`"));
  assert.match(app, /import \{ CARTELLA_DELLA_1_0 \} from "\.\/eredita"/);
});
