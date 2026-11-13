/**
 * Il piano del profilo non sborda a destra sul telefono (T3-2).
 *
 * Sull'emulatore, in una finestra di 412 px, il titoletto, il campo «La cartella
 * su questo telefono» e «0 brani di qui» arrivavano a 462 e restavano tagliati.
 * Il riquadro (`.scheda-anteprima:has(> .titoletto)`) è una griglia a una
 * colonna `1fr`, che non scende sotto la larghezza minima del contenuto, e il
 * contenuto più largo era il prefisso vecchio della rimappatura, `nowrap`.
 *
 * Qui non c'è un DOM: la prova è che le regole che lo impediscono ci siano, e
 * che stiano **solo** sul telefono — il computer resta com'era. La misura vera,
 * a 360 dp e a carattere 1,3×, la fa chi guarda l'emulatore.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");
const telefono = leggi("../src/stile/telefono.css");
const casa = leggi("../src/stile/casa.css");

/** Il corpo della prima regola il cui selettore contiene `selettore`. */
function regola(css: string, selettore: string): string {
  const da = css.indexOf(selettore);
  assert.ok(da >= 0, `nel foglio non c'è «${selettore}»`);
  const apre = css.indexOf("{", da);
  return css.slice(apre + 1, css.indexOf("}", apre));
}

test("sul telefono la colonna del piano può scendere sotto il suo contenuto", () => {
  const corpo = regola(
    telefono,
    ':root[data-forma="telefono"] .scheda-anteprima:has(> .titoletto)',
  );
  assert.match(corpo, /grid-template-columns:\s*minmax\(0,\s*1fr\)/);
  // E lo stesso per l'elenco delle rimappature, che è una griglia anche lui.
  assert.match(telefono, /\.scheda-anteprima \.rimappature\s*\{/);
});

test("sul telefono la riga della rimappatura va a capo e il percorso si spezza", () => {
  const prefisso = regola(telefono, ':root[data-forma="telefono"] .rimappatura-da');
  assert.match(prefisso, /white-space:\s*normal/);
  assert.match(prefisso, /overflow-wrap:\s*anywhere/);
  assert.match(prefisso, /flex-basis:\s*100%/);
  assert.match(prefisso, /overflow:\s*visible/);
  const campo = regola(telefono, ':root[data-forma="telefono"] .rimappatura .campo');
  assert.match(campo, /flex-basis:\s*100%/);
});

test("sul computer la riga resta com'era: ellissi, e nessuna regola in più nel foglio di base", () => {
  const prefisso = regola(casa, ".rimappatura-da {");
  assert.match(prefisso, /white-space:\s*nowrap/);
  assert.match(prefisso, /text-overflow:\s*ellipsis/);
  // Tutte le regole nuove portano il prefisso del telefono.
  const blocco = telefono.slice(telefono.indexOf("Il piano del profilo"));
  const selettori = [...blocco.matchAll(/^([^\n{}/*][^{}]*)\{/gm)]
    .map((m) => m[1] ?? "")
    .filter((s) => /rimappatur|scheda-anteprima/.test(s));
  assert.ok(selettori.length >= 3, "le regole del piano");
  for (const selettore of selettori) {
    assert.match(selettore, /data-forma="telefono"/, selettore);
  }
});
