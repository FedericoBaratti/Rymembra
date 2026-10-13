import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { DOVE, PAGINE, SOVRAPPOSIZIONI } from "../src/studio/scena";

const REGISTRO = readFileSync(new URL("../../../core/aether-skin/src/parts.rs", import.meta.url), "utf8");
const parti = [...REGISTRO.matchAll(/PartDef::nuova\(\s*"([a-z0-9-]+)"/g)].map((m) => m[1]!);

test("ogni luogo di DOVE è una parte del registro", () => {
  const note = new Set(parti);
  for (const parte of Object.keys(DOVE)) assert.ok(note.has(parte), `«${parte}» non è nel registro`);
});

test("ogni luogo di DOVE è una scena che la tela sa disegnare", () => {
  for (const [parte, dove] of Object.entries(DOVE)) {
    if (dove.pagina !== undefined) assert.ok(PAGINE.includes(dove.pagina), parte);
    for (const s of dove.sovrapposizioni ?? []) assert.ok(SOVRAPPOSIZIONI.includes(s), parte);
  }
});
