import { test } from "node:test";
import assert from "node:assert/strict";

import {
  carattereDellaPosizione,
  eLaLettera,
  nomiDi,
  tastoDallaPosizione,
  tastoDi,
  type Pressione,
} from "../src/tasti";

/** Una pressione come la scrive il motore, con i modificatori spenti di serie. */
function premi(key: string, code?: string, mod: Partial<Pressione> = {}): Pressione {
  return {
    key,
    code,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    metaKey: false,
    ...mod,
  };
}

test("il carattere, con i modificatori nell'ordine di sempre", () => {
  assert.equal(tastoDi(premi("f", "KeyF", { ctrlKey: true })), "Ctrl+f");
  assert.equal(tastoDi(premi("F", "KeyF", { shiftKey: true })), "Shift+f");
  assert.equal(tastoDi(premi(" ", "Space")), "Space");
  assert.equal(tastoDi(premi("Control", "ControlLeft", { ctrlKey: true })), null);
  assert.equal(tastoDi(premi("f", "KeyF", { metaKey: true })), null);
});

test("Ctrl+F su una tastiera russa si riconosce dalla posizione", () => {
  const russa = premi("а", "KeyF", { ctrlKey: true });
  assert.equal(tastoDi(russa), "Ctrl+а");
  assert.equal(tastoDallaPosizione(russa), "Ctrl+f");
  assert.deepEqual(nomiDi(russa), ["Ctrl+а", "Ctrl+f"]);
});

test("Ctrl+0 su una tastiera francese: la cifra sta sotto Maiusc", () => {
  assert.deepEqual(nomiDi(premi("à", "Digit0", { ctrlKey: true })), ["Ctrl+à", "Ctrl+0"]);
});

test("un carattere ASCII si prende per quel che è", () => {
  // L'apostrofo italiano sta dove l'americana ha il trattino: niente zoom.
  assert.equal(tastoDallaPosizione(premi("'", "Minus", { ctrlKey: true })), null);
  assert.deepEqual(nomiDi(premi("'", "Minus", { ctrlKey: true })), ["Ctrl+'"]);
  // La Z tedesca sta dove l'americana ha la Y: vale la Z.
  assert.deepEqual(nomiDi(premi("z", "KeyY", { ctrlKey: true })), ["Ctrl+z"]);
});

test("una lettera accentata su un tasto che non è una lettera non ha posizione", () => {
  // `ò` sulla tastiera italiana sta su `Semicolon`.
  assert.equal(tastoDallaPosizione(premi("ò", "Semicolon")), null);
  assert.equal(carattereDellaPosizione("Semicolon"), null);
  assert.equal(carattereDellaPosizione(undefined), null);
});

test("la lettera di una scorciatoia fissa, su qualunque tastiera", () => {
  assert.equal(eLaLettera(premi("a", "KeyA", { ctrlKey: true }), "a"), true);
  assert.equal(eLaLettera(premi("A", "KeyA", { ctrlKey: true, shiftKey: true }), "a"), true);
  assert.equal(eLaLettera(premi("ф", "KeyA", { ctrlKey: true }), "a"), true);
  // AZERTY: la A sta dove l'americana ha la Q, e vale la A.
  assert.equal(eLaLettera(premi("a", "KeyQ", { ctrlKey: true }), "a"), true);
  assert.equal(eLaLettera(premi("q", "KeyA", { ctrlKey: true }), "a"), false);
});
