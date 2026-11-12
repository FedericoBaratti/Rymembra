/*
 * I fogli che `classi.js` legge sono quelli che l'app carica: gli import `.css`
 * di `main.tsx`, tutti e nel loro ordine.
 *
 *   node --test "strumenti/*.test.js"
 */
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const { fogliDi, classiDelFoglio, testiDeiFogli, stratiDellaScala, consiglioDegliStrati } = require("./classi.js");

const REPO = path.dirname(__dirname);
const SRC = path.join(REPO, "apps/desktop/src");

test("i fogli sono gli import .css di main.tsx, nel loro ordine", () => {
  const finto = 'import { App } from "./App";\nimport "./stile.css";\nimport "./stile/b.css";\n';
  assert.deepEqual(fogliDi(finto), [path.join("apps/desktop/src", "stile.css"), path.join("apps/desktop/src", "stile/b.css")]);
});

test("ogni .css sotto src è importato da main.tsx una volta sola, e stile.css è il primo", () => {
  const fogli = fogliDi(fs.readFileSync(path.join(SRC, "main.tsx"), "utf8"));
  assert.equal(fogli[0], path.join("apps/desktop/src", "stile.css"));
  assert.equal(new Set(fogli).size, fogli.length);
  const suDisco = fs
    .readdirSync(SRC, { recursive: true, encoding: "utf8" })
    .filter((p) => p.endsWith(".css"))
    .map((p) => path.join("apps/desktop/src", p));
  assert.deepEqual([...fogli].sort(), suDisco.sort());
});

test("le classi del foglio sono quelle di tutti i fogli, non solo di stile.css", () => {
  const classi = classiDelFoglio(testiDeiFogli(REPO).map((t) => t.crudo).join("\n"));
  assert.ok(classi.has("strato-studio")); // sta in stile/studio.css
  assert.ok(classi.has("pannello-coda")); // sta in stile/lettore.css
});

test("il consiglio sugli z-index dice quanti strati ha la scala vera", () => {
  // report-f12b: il messaggio diceva ancora «dodici token» quando la scala ne
  // aveva quindici, e mandava a cercare un elenco che non c'era più.
  const stile = testiDeiFogli(REPO).find((t) => t.nome === "stile.css");
  const pulito = stile.crudo.replace(/\/\*[\s\S]*?\*\//g, " ");
  const scala = stratiDellaScala(pulito);
  assert.ok(scala.length >= 15, scala.join(", "));
  assert.equal(scala[0], "--strato-decoro");
  assert.equal(scala[scala.length - 1], "--strato-finestra");
  assert.equal(new Set(scala).size, scala.length);
  const consiglio = consiglioDegliStrati(scala);
  assert.ok(consiglio.includes(`: ${scala.length} token --strato-*`), consiglio);
  assert.doesNotMatch(consiglio, /dodici|undici/);
  // Un `var(--strato-…)` non è una dichiarazione della scala.
  assert.deepEqual(stratiDellaScala(":root { --strato-a: 1; } .x { z-index: var(--strato-a); }"), ["--strato-a"]);
});
