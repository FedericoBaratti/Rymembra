import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { sorgentiPer } from "../src/studio/sorgentiColore";

const COPERTINA = ["albumArt.vibrant", "albumArt.muted"] as const;

test("l'accento non offre il modo «sorgente»: segue la copertina con dynamicAccent", () => {
  // `color.accent: { "$source": … }` è un riferimento a sé stesso, e il
  // validatore della skin lo rifiuta: offrirlo vuol dire offrire un errore.
  assert.deepEqual(sorgentiPer("color.accent", COPERTINA), []);
  assert.deepEqual(sorgentiPer("color.surface.1", COPERTINA), COPERTINA);
  assert.deepEqual(sorgentiPer(undefined, COPERTINA), COPERTINA);
});

test("la voce del token nello Studio e il controllo del colore passano dal filtro", () => {
  const leggi = (f: string) => readFileSync(new URL(`../src/studio/${f}`, import.meta.url), "utf8");
  assert.match(leggi("IspezioneVoci.tsx"), /sorgentiPer\(id, registro\.sorgenti\)/);
  assert.match(leggi("controlli.tsx"), /const ammesse = sorgentiPer\(escluso, sorgenti\);/);
});
