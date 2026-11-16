/**
 * La notizia di fine coda di «Scarica qui», e il segnaposto del relay (R13-12).
 *
 * - «Scaricati 0 brani, 2 non riusciti»: con tutti i file rifiutati la frase
 *   cominciava dal niente. Il secondo numero (`falliti`) non sceglie la forma del
 *   plurale — la sceglie solo `n` — e «{falliti} non riusciti» con falliti = 1
 *   non accordava; oggi il conto è in coda alla frase («non riusciti: {falliti}»).
 *   Con nessun brano arrivato c'è una frase sua, il cui `n` è proprio il numero
 *   dei non riusciti, così ha la sua forma per uno.
 * - Il segnaposto del campo del relay era un dominio italiano scritto nel
 *   componente (`https://relay.esempio.it`), anche in inglese.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import * as lontano from "../src/lontano";

const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");
const LINGUE = {
  it: JSON.parse(leggi("../src/lingue/it.json")) as Record<string, string>,
  en: JSON.parse(leggi("../src/lingue/en.json")) as Record<string, string>,
};

test("la frase di fine coda: tutto bene, qualcosa no, niente è arrivato (R13-12)", () => {
  assert.equal(typeof lontano.fraseScaricoFinito, "function", "manca fraseScaricoFinito");
  const frase = lontano.fraseScaricoFinito;
  assert.deepEqual(frase({ fatti: 3, falliti: 0 }), { chiave: "devices.download.done", valori: { n: 3 } });
  assert.deepEqual(frase({ fatti: 3, falliti: 1 }), {
    chiave: "devices.download.doneWithErrors",
    valori: { n: 3, falliti: 1 },
  });
  // Niente è arrivato: la frase non comincia da «Scaricati 0 brani», e il suo `n`
  // sono i non riusciti, così sceglie la forma giusta per uno.
  assert.deepEqual(frase({ fatti: 0, falliti: 2 }), {
    chiave: "devices.download.allFailed",
    valori: { n: 2 },
  });
  assert.deepEqual(frase({ fatti: 0, falliti: 1 }), {
    chiave: "devices.download.allFailed",
    valori: { n: 1 },
  });
});

test("le frasi di fine coda esistono nelle due lingue, con la forma per uno (R13-12)", () => {
  for (const [lingua, voci] of Object.entries(LINGUE)) {
    for (const chiave of ["devices.download.allFailed", "devices.download.allFailed.uno"]) {
      assert.ok(voci[chiave], `${lingua}: manca ${chiave}`);
    }
    // `n` è il numero dei non riusciti: la frase per niente arrivato non nomina
    // «0 brani», e non porta un secondo segnaposto da accordare.
    assert.match(voci["devices.download.allFailed"] ?? "", /\{n\}/, lingua);
    assert.doesNotMatch(voci["devices.download.allFailed"] ?? "", /\{falliti\}/, lingua);
  }
});

test("il segnaposto del relay sta nel catalogo e non è un dominio italiano nell'inglese (R13-12)", () => {
  const componente = leggi("../src/parti/Dispositivi.tsx");
  assert.doesNotMatch(componente, /placeholder="https?:\/\/relay\./, "il segnaposto del relay non è scritto nel componente");
  assert.match(componente, /placeholder=\{t\("settings\.devices\.relay\.placeholder"\)\}/);
  for (const [lingua, voci] of Object.entries(LINGUE)) {
    assert.ok(voci["settings.devices.relay.placeholder"], `${lingua}: manca il segnaposto`);
  }
  assert.doesNotMatch(LINGUE.en["settings.devices.relay.placeholder"] ?? "", /esempio/, "in inglese un dominio d'esempio inglese");
});
