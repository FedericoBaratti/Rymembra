import { test } from "node:test";
import assert from "node:assert/strict";

import { ALTRO, indiceAlfabetico, letteraDi } from "../src/alfabeto";

test("accenti e maiuscole non cambiano la lettera, per chi legge in italiano", () => {
  const it = new Intl.Collator("it", { sensitivity: "base" });
  assert.equal(letteraDi("édith piaf", it), "e");
  assert.equal(letteraDi("Ørjan", it), "o");
  assert.equal(letteraDi("Ölo", it), "o");
  assert.equal(letteraDi("łona", it), "l");
});

test("numeri, altri alfabeti e chiavi vuote stanno sotto «#»", () => {
  const it = new Intl.Collator("it", { sensitivity: "base" });
  assert.equal(letteraDi("2pac", it), ALTRO);
  assert.equal(letteraDi("Земфира", it), ALTRO);
  assert.equal(letteraDi("", it), ALTRO);
});

test("per uno svedese «ö» è un'altra lettera", () => {
  const sv = new Intl.Collator("sv", { sensitivity: "base" });
  assert.equal(letteraDi("öst", sv), ALTRO);
  assert.equal(letteraDi("ost", sv), "o");
});

test("l'indice: lettere, voci piene e dove comincia ognuna", () => {
  const chiavi = ["abba", "ácido", "beatles", "édith", "zz top", "1975"];
  const { lettere, piene, ancore } = indiceAlfabetico(chiavi, "it");
  assert.deepEqual(lettere, ["a", "a", "b", "e", "z", ALTRO]);
  assert.deepEqual([...piene].sort(), [ALTRO, "a", "b", "e", "z"].sort());
  assert.equal(ancore.get("a"), 0);
  assert.equal(ancore.get("b"), 2);
  assert.equal(ancore.get("e"), 3);
  assert.equal(ancore.get(ALTRO), 5);
  assert.equal(ancore.get("c"), undefined);
});

test("una lingua malformata ripiega su quella di sistema", () => {
  assert.deepEqual(indiceAlfabetico(["abba"], "").lettere, ["a"]);
});
