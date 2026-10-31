import { test } from "node:test";
import assert from "node:assert/strict";

import { daFotografare, poiSeSalvato } from "../src/studio/salvataggio";

test("Ctrl+S senza modifiche non prende un'altra istantanea uguale", () => {
  // Il nucleo ne tiene venti e pota senza guardare la causa: venti Ctrl+S
  // identici spingevano fuori «salvata», «esportata» e «migrata».
  assert.equal(daFotografare("{}", "{}"), false);
  assert.equal(daFotografare('{"id":"a"}', "{}"), true);
  assert.equal(daFotografare("{}", null), true, "la prima della sessione si prende");
});

test("poiSeSalvato: con un salvataggio fallito non si esce", async () => {
  let uscito = false;
  let errore: unknown = null;
  await poiSeSalvato(
    () => Promise.reject(new Error("disco pieno")),
    () => {
      uscito = true;
    },
    (e) => {
      errore = e;
    },
  );
  assert.equal(uscito, false);
  assert.ok(errore instanceof Error);
  await poiSeSalvato(
    () => Promise.resolve(),
    () => {
      uscito = true;
    },
    () => {},
  );
  assert.equal(uscito, true);
});
