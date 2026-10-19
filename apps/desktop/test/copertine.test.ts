import { test } from "node:test";
import assert from "node:assert/strict";

import {
  indirizzoCopertina,
  LATO,
  misuraPer,
  sizesPer,
  srcsetCopertina,
} from "../src/copertine";

const IMPRONTA = "ab".repeat(32);

test("le tre misure hanno i loro indirizzi", () => {
  assert.equal(
    indirizzoCopertina(IMPRONTA, "miniatura"),
    `http://rymembra-cover.localhost/${IMPRONTA}.t`,
  );
  assert.equal(
    indirizzoCopertina(IMPRONTA, "media"),
    `http://rymembra-cover.localhost/${IMPRONTA}.m`,
  );
  assert.equal(
    indirizzoCopertina(IMPRONTA, "piena"),
    `http://rymembra-cover.localhost/${IMPRONTA}`,
  );
});

test("srcset dichiara le tre larghezze, dalla più piccola", () => {
  assert.equal(
    srcsetCopertina(IMPRONTA),
    [
      `http://rymembra-cover.localhost/${IMPRONTA}.t 160w`,
      `http://rymembra-cover.localhost/${IMPRONTA}.m 320w`,
      `http://rymembra-cover.localhost/${IMPRONTA} 640w`,
    ].join(", "),
  );
});

test("sizes è la larghezza della scheda, per eccesso", () => {
  assert.equal(sizesPer(171.5), "172px");
  assert.equal(sizesPer(null), "164px");
  assert.equal(sizesPer(0), "164px");
});

test("a 100% una scheda prende la media, a 200% la piena", () => {
  // Una scheda della griglia degli album: dai 164 pixel in su.
  assert.equal(misuraPer(171.5, 1), "media");
  assert.equal(misuraPer(171.5, 2), "piena");
  // A 150% la media basta ancora per una scheda stretta…
  assert.equal(misuraPer(164, 1.5), "media");
  assert.equal(misuraPer(220, 1.5), "piena");
  assert.equal(misuraPer(100, 1.5), "miniatura");
  // …e un tassello del mosaico degli artisti, largo metà, prende la miniatura a
  // 100% e la media a 200%.
  assert.equal(misuraPer(85, 1), "miniatura");
  assert.equal(misuraPer(85, 2), "media");
  assert.equal(misuraPer(80, 2), "miniatura");
  // Oltre la piena non c'è niente: si prende la piena.
  assert.equal(misuraPer(1000, 2), "piena");
  assert.equal(LATO.media, 320);
});
