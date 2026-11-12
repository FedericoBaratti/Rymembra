import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { contiDaNominare } from "../src/altroConto";

/*
 * «Ricollega» con un altro account (R24-5, D-92): il nucleo rifiuta il
 * consenso e nomina l'account con cui si è entrati; la finestrella nomina
 * anche quello atteso. E perché il nucleo possa confrontare, il dialogo deve
 * dirgli quale conto si stava ricollegando.
 */

const altro = {
  code: "cloud.otherAccount",
  domain: "cloud",
  severity: "warning",
  retryable: false,
  i18nKey: "errors.cloud.otherAccount",
  message: "casa@esempio.it",
  cause: null,
};

test("un altro account si nomina accanto a quello atteso", () => {
  assert.deepEqual(contiDaNominare(altro, { nome: "lavoro@esempio.it" }), {
    ottenuto: "casa@esempio.it",
    atteso: "lavoro@esempio.it",
  });
});

test("fuori da un «Ricollega», o per un altro guasto, la frase è quella del catalogo", () => {
  assert.equal(contiDaNominare(altro, null), null);
  assert.equal(contiDaNominare({ ...altro, code: "cloud.authExpired" }, { nome: "x" }), null);
  assert.equal(contiDaNominare({ ...altro, message: "  " }, { nome: "x" }), null);
  assert.equal(contiDaNominare(new Error("rete"), { nome: "x" }), null);
});

const sorgente = (percorso: string) =>
  readFileSync(new URL(percorso, import.meta.url), "utf8");

test("il dialogo dice al nucleo quale conto si ricollega", () => {
  const finestrella = sorgente("../src/AggiungiCartella.tsx");
  assert.match(finestrella, /daRicollegare\?\.conto \?\? null/);
  assert.match(finestrella, /useCollegamento\(onCollegato, daRicollegare\)/);
  assert.match(finestrella, /fraseDelGuasto\(guasto, daRicollegare\)/);
  const ipc = sorgente("../src/ipc.ts");
  assert.match(ipc, /"nuvola_conto_collega", \{[^}]*\batteso,/s);
  const nucleo = sorgente("../src-tauri/src/nuvola_musica.rs");
  assert.match(nucleo, /pub async fn nuvola_conto_collega\([^)]*atteso: Option<String>/s);
});

test("la frase nomina tutti e due in tutte e due le lingue", () => {
  for (const lingua of ["it", "en"]) {
    const testi = JSON.parse(sorgente(`../src/lingue/${lingua}.json`));
    const frase: string = testi["folders.reconnect.otherAccount"] ?? "";
    assert.match(frase, /\{ottenuto\}/, lingua);
    assert.match(frase, /\{atteso\}/, lingua);
  }
});
