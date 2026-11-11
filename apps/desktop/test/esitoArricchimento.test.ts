import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { notiziaDellAnnullamento } from "../src/esitoArricchimento";

/*
 * «Dimentica l'arricchimento» dice com'è andata, e prima chiede.
 *
 * Il nucleo risponde `{riportati, falliti, stato}`, e la finestra prendeva
 * soltanto `stato`: con un NAS spento il gesto spegneva l'automatico, non
 * riportava niente e non diceva niente (R12-3). E partiva con un clic, senza
 * domanda, mentre «Rileggi tutti i tag», che costa meno, chiede (D-39).
 */
const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");

test("con tutti i file irraggiungibili la notizia lo dice, col numero", () => {
  assert.deepEqual(notiziaDellAnnullamento({ riportati: 0, falliti: 300 }), [
    { chiave: "settings.enrich.undoFailed", n: 300 },
  ]);
});

test("il caso misto dice tutte e due le cose, ognuna col suo numero", () => {
  // Una frase per numero, e non una con due: `t` sceglie il plurale su `n`
  // soltanto, e «1 brani» sarebbe la prima cosa che si legge.
  assert.deepEqual(notiziaDellAnnullamento({ riportati: 120, falliti: 180 }), [
    { chiave: "settings.enrich.undone", n: 120 },
    { chiave: "settings.enrich.undoFailed", n: 180 },
  ]);
  assert.deepEqual(notiziaDellAnnullamento({ riportati: 1, falliti: 0 }), [
    { chiave: "settings.enrich.undone", n: 1 },
  ]);
  // Niente riportato e niente fallito: non c'è niente da dire.
  assert.deepEqual(notiziaDellAnnullamento({ riportati: 0, falliti: 0 }), []);
});

test("le frasi della notizia ci sono in tutte e due le lingue, con la forma per uno", () => {
  for (const lingua of ["it", "en"]) {
    const voci = JSON.parse(leggi(`../src/lingue/${lingua}.json`)) as Record<string, string>;
    for (const chiave of [
      "settings.enrich.undone",
      "settings.enrich.undoFailed",
      "settings.enrich.undo.confirm.title",
      "settings.enrich.undo.confirm.body",
    ]) {
      assert.ok(voci[chiave], `${lingua}: manca ${chiave}`);
    }
    for (const chiave of [
      "settings.enrich.undone",
      "settings.enrich.undoFailed",
      "settings.enrich.undo.confirm.body",
    ]) {
      assert.ok(voci[`${chiave}.uno`], `${lingua}: manca ${chiave}.uno`);
    }
  }
});

test("la finestra legge l'esito dell'annullamento e lo dà alla notizia", () => {
  const finestra = leggi("../src/ImpostazioniFinestra.tsx");
  const gesto = finestra.slice(finestra.indexOf("onArricchimentoAnnulla={"));
  const corpo = gesto.slice(0, gesto.indexOf("nuvola={nuvola}"));
  assert.match(corpo, /notiziaDellAnnullamento\(esito\)/);
  assert.match(corpo, /onNotizia\(/);
});

test("«Dimentica l'arricchimento» chiede prima, come «Rileggi tutti i tag»", () => {
  const sorgente = leggi("../src/schermate/Impostazioni.tsx");
  const dove = sorgente.indexOf('t("settings.enrich.undo.count"');
  assert.ok(dove > 0, "non trovo il tasto: la prova va aggiornata");
  const tasto = sorgente.slice(sorgente.lastIndexOf("<button", dove), dove);
  // Il clic apre la domanda; il gesto vero parte solo dal suo «sì».
  assert.doesNotMatch(tasto, /onClick=\{onArricchimentoAnnulla\}/);
  assert.match(tasto, /setDomanda\(/);
  assert.match(tasto, /settings\.enrich\.undo\.confirm\.title/);
  assert.match(tasto, /fai: onArricchimentoAnnulla/);
});
