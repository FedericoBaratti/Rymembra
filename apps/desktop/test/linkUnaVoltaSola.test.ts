/**
 * Due link di collegamento di fila (S4-4): il primo vince.
 *
 * Il nucleo non si può eseguire da qui: le prove di `src-tauri/src/lontano/mod.rs`
 * fanno la parte loro, e qui c'è quel che sta dalla parte della pagina — la
 * regola pura del dialogo e il cablaggio che la tiene unita al nucleo. La verifica
 * dal vivo è nel report della correzione.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import type { LinkLontano } from "../src/ipc";
import { linkDaMostrare } from "../src/lontano";

const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

const link = (impronta: string): LinkLontano => ({
  link: `rymembra://collega/${impronta}`,
  nome: "PC di casa",
  impronta,
});

/*
 * S4-4. Una pagina web che lancia `rymembra://collega/…` in sequenza non deve
 * cambiare sotto il dito di chi sta leggendo il dialogo: stesso nome dichiarato,
 * impronta diversa. Il primo link vince, e quelli dopo si scartano.
 */
test("S4-4: un link che arriva a dialogo aperto non lo sostituisce", () => {
  const a = link("AAAA");
  const b = link("BBBB");
  assert.equal(linkDaMostrare(a, b), a, "il primo vince");
  assert.equal(linkDaMostrare(null, b), b, "senza dialogo si apre");
  assert.equal(linkDaMostrare(a, null), a, "niente da sostituire");
  assert.equal(linkDaMostrare(null, null), null);
});

test("S4-4: la pagina dice al nucleo se il dialogo è aperto, e al montaggio", () => {
  const dispositivi = sorgente("parti/Dispositivi.tsx");
  const ipc = sorgente("ipc.ts");
  assert.match(ipc, /lontanoLinkDialogo: \(aperto: boolean\) =>\s*invoke<void>\("lontano_link_dialogo", \{ aperto \}\)/);
  // L'effetto gira al montaggio e a ogni cambio di presenza del dialogo.
  assert.match(dispositivi, /const dialogoDelLink = link !== null;/);
  assert.match(
    dispositivi,
    /useEffect\(\(\) => \{\s*ipc\.lontanoLinkDialogo\(dialogoDelLink\)\.catch\(\(\) => undefined\);\s*\}, \[dialogoDelLink\]\);/,
  );
  // Il link preso dal nucleo passa dalla regola, e non scrive e basta.
  assert.match(dispositivi, /archivioLink\.scrivi\(linkDaMostrare\(archivioLink\.leggi\(\), visto\)\)/);
});
