import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { etichettaBanda, etichettaCortaBanda } from "../src/eq";

/*
 * Sul telefono le frequenze dell'equalizzatore non si accavallano (R18-13).
 *
 * A 360 dp «125 Hz», «250 Hz» e «500 Hz» si sovrapponevano di qualche pixel:
 * dieci etichette lunghe in trecento pixel. Sul telefono si scrivono corte
 * («125», «1k»), come su ogni equalizzatore stretto; il nome del cursore resta
 * quello intero, per chi ascolta.
 */
// I centri di `CENTRI_EQ` (`ipc/libreria.ts`), scritti qui: quel modulo porta
// con sé le lingue, che `node --test` non carica.
const CENTRI_EQ = [31.25, 62.5, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];

test("le etichette corte sono quelle di sempre, senza l'unità", () => {
  assert.deepEqual(
    CENTRI_EQ.map(etichettaCortaBanda),
    ["31", "63", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"],
  );
  assert.deepEqual(
    CENTRI_EQ.map(etichettaBanda),
    ["31 Hz", "63 Hz", "125 Hz", "250 Hz", "500 Hz", "1 kHz", "2 kHz", "4 kHz", "8 kHz", "16 kHz"],
  );
});

test("sul telefono la banda si scrive corta, e il cursore si chiama per intero", () => {
  const sorgente = readFileSync(new URL("../src/parti/Equalizzatore.tsx", import.meta.url), "utf8");
  assert.match(sorgente, /MOBILE \? etichettaCortaBanda\(hz\) : etichettaBanda\(hz\)/);
  assert.match(sorgente, /aria-label=\{etichettaBanda\(hz\)\}/);
});
