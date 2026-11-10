/**
 * I fogli del telefono non toccano il computer.
 *
 * `stile/telefono*.css` sono importati da `main.tsx` per tutte e due le
 * piattaforme — un foglio non si carica a metà — e la promessa che sul
 * computer non cambino un pixel sta tutta in un prefisso: ogni regola comincia
 * con `:root[data-forma="telefono"]`, che `main.tsx` mette solo sul telefono.
 * Una regola che lo dimentica non dà nessun errore: disegna sul computer, in
 * silenzio, fino a che qualcuno se ne accorge da una schermata. Qui lo si
 * controlla regola per regola, anche dentro `@media`, `@supports` e
 * `@starting-style`.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const CARTELLA = join(import.meta.dirname, "..", "src", "stile");
const PREFISSO = ':root[data-forma="telefono"]';

/** I selettori delle regole di un foglio, con le at-rule attraversate. */
function selettori(css: string): string[] {
  const pulito = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const trovati: string[] = [];
  // Ogni livello dice se dentro ci sono regole (`true`) o dichiarazioni.
  const pila: boolean[] = [true];
  let testa = "";
  for (const c of pulito) {
    if (c === "{") {
      const intestazione = testa.trim();
      testa = "";
      const dentroRegole = pila.at(-1) ?? true;
      if (!dentroRegole) {
        pila.push(false);
        continue;
      }
      if (intestazione.startsWith("@")) {
        // Dentro `@keyframes` ci sono fotogrammi, non selettori.
        pila.push(!/^@(-webkit-)?keyframes\b/.test(intestazione) ? true : false);
        continue;
      }
      trovati.push(intestazione);
      pila.push(false);
    } else if (c === "}") {
      pila.pop();
      testa = "";
    } else if (c === ";" && (pila.at(-1) ?? true)) {
      // Un `@import` o un `@layer x;` a livello di regole.
      testa = "";
    } else {
      testa += c;
    }
  }
  return trovati;
}

/**
 * I selettori di una lista, divisi alle virgole di primo livello: quelle dentro
 * `:not(a, b)` o `:is(a, b)` sono dello stesso selettore, e il pezzo dopo non
 * comincia col prefisso perché non è un selettore.
 */
function dividi(lista: string): string[] {
  const pezzi: string[] = [];
  let profondita = 0;
  let pezzo = "";
  for (const c of lista) {
    if (c === "(") profondita++;
    else if (c === ")") profondita--;
    if (c === "," && profondita === 0) {
      pezzi.push(pezzo);
      pezzo = "";
    } else {
      pezzo += c;
    }
  }
  pezzi.push(pezzo);
  return pezzi;
}

const fogli = readdirSync(CARTELLA).filter((f) => /^telefono.*\.css$/.test(f));

test("ci sono i fogli del telefono", () => {
  assert.ok(fogli.includes("telefono.css"));
  assert.ok(fogli.includes("telefono-pagine.css"));
});

for (const foglio of fogli) {
  test(`${foglio}: ogni regola vale solo sul telefono`, () => {
    const css = readFileSync(join(CARTELLA, foglio), "utf8");
    const scoperti = selettori(css)
      .flatMap(dividi)
      .map((s) => s.trim())
      .filter((s) => s.length > 0 && !s.startsWith(PREFISSO));
    assert.deepEqual(scoperti, [], `selettori senza ${PREFISSO}`);
  });
}

test("le virgole dentro :not() non dividono il selettore", () => {
  assert.deepEqual(dividi(`${PREFISSO} a:not(.x, .y), ${PREFISSO} b`), [
    `${PREFISSO} a:not(.x, .y)`,
    ` ${PREFISSO} b`,
  ]);
});

test("il controllo vede anche dentro le at-rule", () => {
  const css = `
    @media (hover: hover) { .x { color: red; } }
    @keyframes k { from { opacity: 0; } to { opacity: 1; } }
    ${PREFISSO} .y { color: blue; }
  `;
  assert.deepEqual(selettori(css), [".x", `${PREFISSO} .y`]);
});
