import { test } from "node:test";
import assert from "node:assert/strict";

import { comeScorrere } from "../src/transizione";

test("col movimento ridotto si salta invece di scivolare", () => {
  assert.equal(comeScorrere("smooth", true), "auto");
});

test("senza, si scivola come chiesto", () => {
  assert.equal(comeScorrere("smooth", false), "smooth");
});

test("chi non chiedeva di scivolare resta com'era", () => {
  assert.equal(comeScorrere("auto", true), "auto");
  assert.equal(comeScorrere("instant", false), "instant");
});
