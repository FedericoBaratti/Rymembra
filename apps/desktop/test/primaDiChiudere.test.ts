import { test } from "node:test";
import assert from "node:assert/strict";

import { readFileSync } from "node:fs";

import { primaDiChiudere, quandoCambiaDaScrivere, tuttoScritto } from "../src/studio/primaDiChiudere";

test("la X aspetta che lo Studio abbia scritto, ma non per sempre (R14-7)", async () => {
  // Il salvataggio dello Studio parte 800 ms dopo l'ultima modifica: chiudendo
  // Rymembra prima, l'ultimo respiro di modifiche andava perso.
  let scritto = false;
  const togli = primaDiChiudere(async () => {
    await new Promise((r) => setTimeout(r, 20));
    scritto = true;
  });
  await tuttoScritto(1000);
  assert.ok(scritto, "la chiusura non ha aspettato il salvataggio");
  togli();

  // Un salvataggio che non torna non tiene aperta la finestra.
  const appeso = primaDiChiudere(() => new Promise(() => undefined));
  const prima = Date.now();
  await tuttoScritto(50);
  assert.ok(Date.now() - prima < 1000);
  appeso();
  // E un guasto non ferma la chiusura.
  const rotto = primaDiChiudere(() => Promise.reject(new Error("disco pieno")));
  await tuttoScritto(50);
  rotto();
});

test("il nucleo sa quando lo Studio ha qualcosa da scrivere, e quando non più (R14-7)", () => {
  // Alt+F4, la barra delle applicazioni ed «Esci» dal vassoio chiudono dal
  // nucleo: per aspettare lo Studio come la X, il nucleo deve sapere che c'è.
  const detti: boolean[] = [];
  const smetti = quandoCambiaDaScrivere((si) => detti.push(si));
  // Chi si iscrive sente subito com'è adesso.
  assert.deepEqual(detti, [false]);
  const uno = primaDiChiudere(async () => undefined);
  const due = primaDiChiudere(async () => undefined);
  assert.deepEqual(detti, [false, true], "solo al primo che si registra");
  uno();
  assert.deepEqual(detti, [false, true], "ce n'è ancora uno");
  due();
  assert.deepEqual(detti, [false, true, false]);
  // Togliere due volte non dice niente di nuovo.
  due();
  assert.deepEqual(detti, [false, true, false]);
  smetti();
  const tre = primaDiChiudere(async () => undefined);
  tre();
  assert.deepEqual(detti, [false, true, false], "chi ha smesso non sente più");
});

test("la barra del titolo tiene il nucleo al corrente e scrive quando il nucleo lo chiede", () => {
  const barra = readFileSync(new URL("../src/parti/BarraTitolo.tsx", import.meta.url), "utf8");
  assert.match(barra, /quandoCambiaDaScrivere\(\(si\) =>/);
  assert.match(barra, /ipc\.chiusuraDaScrivere\(si\)/);
  assert.match(barra, /listen\(EVENTO_SCRIVI, \(\) =>/);
  assert.match(barra, /tuttoScritto\(\)\s*\.then\(\(\) => ipc\.chiusuraScritto\(\)\)/);
  const ipc = readFileSync(new URL("../src/ipc.ts", import.meta.url), "utf8");
  assert.match(ipc, /chiusuraDaScrivere: \(si: boolean\) => invoke<void>\("chiusura_da_scrivere", \{ si \}\)/);
  assert.match(ipc, /chiusuraScritto: \(\) => invoke<void>\("chiusura_scritto"\)/);
});
