import { test } from "node:test";
import assert from "node:assert/strict";

import type { NodoScafale } from "../src/ipc";
import { conNodo, prefabCambiati } from "../src/studio/albero";

const widget = (
  name: string,
  options: Record<string, boolean | string | number> = {},
): NodoScafale => ({
  kind: "widget",
  at: "",
  name,
  size: "hug",
  gap: null,
  align: null,
  spread: null,
  part: null,
  fromPrefab: null,
  slot: null,
  options,
  children: [],
});

const zona = (children: NodoScafale[], fromPrefab: string | null = null): NodoScafale => ({
  kind: "zone",
  at: "",
  name: "row",
  size: "fill",
  gap: "none",
  align: "stretch",
  spread: "start",
  part: null,
  fromPrefab,
  slot: null,
  options: {},
  children,
});

test("una modifica dentro un'istanza di prefab cambia il prefab", () => {
  const prima = zona([zona([widget("transport")], "comandi"), zona([widget("transport")], "comandi")]);
  assert.equal(prefabCambiati(prima, prima).size, 0, "niente di cambiato, niente da scrivere");
  const istanza = prima.children[1];
  assert.ok(istanza);
  const dopo = conNodo(prima, [1], { ...istanza, children: [widget("transport", { shuffle: true })] });
  const cambiati = prefabCambiati(prima, dopo);
  assert.deepEqual([...cambiati.keys()], ["comandi"]);
  const corpo = cambiati.get("comandi") as { children: { options: unknown }[] };
  assert.deepEqual(corpo.children[0]?.options, { shuffle: true });
});
