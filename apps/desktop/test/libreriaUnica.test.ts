import { test } from "node:test";
import assert from "node:assert/strict";

import { dispositivoDellaRiga } from "../src/ipc/libreria.ts";

/*
 * La libreria unica (§4): un brano di un altro dispositivo è una riga di
 * `tracks` con il percorso `lontano://<chiave del dispositivo>/<id>`. La
 * finestra lo riconosce dal percorso — la stessa forma che il nucleo scrive
 * (`rymembra_app::specchio::parti`) — per mostrare la riga come «di un altro
 * dispositivo» e, se non risponde, in grigio.
 */

const CHIAVE = "ab".repeat(32);

test("il dispositivo di una riga si legge dal suo percorso", () => {
  assert.equal(dispositivoDellaRiga(`lontano://${CHIAVE}/4120`), CHIAVE);
});

test("quel che non è la forma vera non è una riga della libreria unica", () => {
  for (const storto of [
    null,
    undefined,
    "",
    "C:\\Musica\\a.flac",
    "nuvola://onedrive-abcdefgh/Musica/a.flac",
    "lontano://",
    `lontano://${CHIAVE}`,
    `lontano://${CHIAVE}/`,
    `lontano://${CHIAVE}/0`,
    `lontano://${CHIAVE}/007`,
    `lontano://${CHIAVE}/-3`,
    `lontano://${CHIAVE}/12/13`,
    `lontano://${"AB".repeat(32)}/12`,
    `lontano://${"ab".repeat(31)}/12`,
  ]) {
    assert.equal(dispositivoDellaRiga(storto), null, String(storto));
  }
});
