import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const sorgente = readFileSync(new URL("../src/playlistAperta.ts", import.meta.url), "utf8");

// R13b-13: il brano preso si legge alla presa, come nella coda.
test("riordino col puntatore: il brano preso si ricorda alla presa, non al rilascio", () => {
  const presa = sorgente.slice(sorgente.indexOf("const presa = useCallback"), sorgente.indexOf("const lascia = useCallback"));
  assert.match(presa, /preso\.current = /);
  const lascia = sorgente.slice(sorgente.indexOf("const lascia = useCallback"), sorgente.indexOf("return {\n    braniPlaylist"));
  assert.doesNotMatch(lascia, /braniPlaylist\[trascinata\]/);
  assert.match(lascia, /preso\.current/);
});
