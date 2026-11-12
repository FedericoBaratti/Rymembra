import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * D-11 per il ripristino da Drive e per il profilo: una playlist di qui con lo
 * stesso nome ma che è un'altra non si tocca, quella che arriva le entra
 * accanto, e il piano lo dice per nome. Il nome nuovo lo sceglie il nucleo;
 * qui si guarda che la finestra lo dica.
 */
const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");

test("il piano del ripristino dice quale playlist di qui resta, e accanto a chi entra la nuova", () => {
  const ripristino = leggi("../src/Ripristino.tsx");
  assert.match(ripristino, /p\.accantoA !== null &&\s+t\("restore\.playlist\.beside", \{ esistente: p\.accantoA \}\)/);
});

test("il piano del profilo elenca le playlist che entrano accanto", () => {
  const profilo = leggi("../src/parti/Profilo.tsx");
  assert.match(profilo, /piano\.playlistAccanto \?\? \[\]/);
  assert.match(profilo, /t\("profile\.playlistBeside"/);
});
