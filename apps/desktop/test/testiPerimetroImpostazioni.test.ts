import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * Testi delle Impostazioni che dicevano cose sbagliate (R12-7).
 *
 * - «o trascinala qui» sul telefono, dove sulla finestra non si trascina
 *   niente (`AggiungiCartella` e `Primo` lo nascondono già);
 * - la nota di Google Drive senza account che cominciava con uno spazio, resto
 *   di una concatenazione;
 * - «1 aggiunti · 1 aggiornati · 1 tolti»: i numeri non si chiamano `n`, e il
 *   plurale non si sceglie. In italiano si scrive «aggiunti: 1», che non
 *   accorda; l'inglese («1 added») non ne ha bisogno.
 */
const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");
const LINGUE = {
  it: JSON.parse(leggi("../src/lingue/it.json")) as Record<string, string>,
  en: JSON.parse(leggi("../src/lingue/en.json")) as Record<string, string>,
};

test("«o trascinala qui» non si dice sul telefono", () => {
  const sorgente = leggi("../src/schermate/Impostazioni.tsx");
  for (const chiave of ["settings.roots.orDrop", "settings.skin.orDrop"]) {
    const dove = sorgente.indexOf(`t("${chiave}")`);
    assert.ok(dove > 0, `non trovo ${chiave}: la prova va aggiornata`);
    const riga = sorgente.slice(sorgente.lastIndexOf("\n", dove), dove);
    assert.match(riga, /!MOBILE &&/, chiave);
  }
});

test("la nota di Drive senza account non comincia con uno spazio", () => {
  for (const [lingua, voci] of Object.entries(LINGUE)) {
    const testo = voci["settings.sync.drive.needAccount"] ?? "";
    assert.ok(testo.length > 0, lingua);
    assert.equal(testo, testo.trimStart(), lingua);
  }
});

test("i conti della scansione e della sorveglianza non scrivono «1 aggiunti»", () => {
  for (const chiave of ["settings.scan.result", "settings.roots.watch.last"]) {
    const testo = LINGUE.it[chiave] ?? "";
    assert.ok(testo.length > 0, chiave);
    assert.doesNotMatch(testo, /\{\w+\} (aggiunti|entrati|aggiornati|spostati|tolti)\b/, chiave);
  }
});
