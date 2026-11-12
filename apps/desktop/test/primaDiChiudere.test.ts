import { test } from "node:test";
import assert from "node:assert/strict";

import { primaDiChiudere, tuttoScritto } from "../src/studio/primaDiChiudere";

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
