import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { vociCartella } from "../src/menuBrani";

const t = (chiave: string) => chiave;
const fai = { riproduci: () => undefined, dopo: () => undefined, accoda: () => undefined };

test("pannello Cartelle: senza la coda il menù non offre «Dopo» né «In coda»", () => {
  assert.deepEqual(
    vociCartella(fai, t).map((v) => v.etichetta),
    ["action.play", "action.playNext", "action.enqueue"],
  );
  // In una sessione con diritto «Ascolta»: niente gesti sulla coda che il
  // nucleo rifiuta, come in `vociBrani`.
  assert.deepEqual(vociCartella(fai, t, true).map((v) => v.etichetta), ["action.play"]);
});

test("pannello Cartelle: la striscia e il trascinamento guardano il permesso della coda", () => {
  const cartelle = readFileSync(new URL("../src/schermate/Cartelle.tsx", import.meta.url), "utf8");
  assert.match(cartelle, /useLettore\(\(s\) => gestoPermesso\(s\.insieme, "coda"\)\)/);
  assert.match(cartelle, /vociCartella\(/);
  // La striscia della selezione: «In coda» spento con la ragione.
  const striscia = cartelle.slice(cartelle.indexOf('className="scelta-cartelle"'), cartelle.indexOf("folders.selection.clear"));
  assert.equal([...striscia.matchAll(/disabled=\{!codaLibera\}/g)].length, 1);
  assert.equal([...striscia.matchAll(/title=\{spentoInSessione\}/g)].length, 1);
  // Il trascinamento verso la coda non parte.
  assert.match(cartelle, /if \(!codaLiberaOra\.current\) return;/);
});
