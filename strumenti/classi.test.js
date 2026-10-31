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

const { fogliDi, classiDelFoglio, testiDeiFogli } = require("./classi.js");

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
