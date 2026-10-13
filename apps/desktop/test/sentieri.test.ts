import { test } from "node:test";
import assert from "node:assert/strict";

import { destinazioneDi, sentieroParte, sentieroToken } from "../src/studio/sentieri";

const TOKEN = ["color.surface.0", "color.accent", "color.accent.soft", "shadow.1"];

test("i percorsi seguono lo stato e il tema scelti", () => {
  assert.deepEqual(sentieroParte("card", null, "radius", "base"), ["parts", "card", "radius"]);
  assert.deepEqual(sentieroParte("card", "hover", "radius", "base"), [
    "parts",
    "card",
    "states",
    "hover",
    "radius",
  ]);
  assert.deepEqual(sentieroParte("card", null, "radius", "variante"), [
    "variant",
    "parts",
    "card",
    "radius",
  ]);
  assert.deepEqual(sentieroToken("color.accent", "variante"), ["variant", "tokens", "color.accent"]);
});

test("un percorso del nucleo porta alla cosa, col suo stato e il suo tema", () => {
  assert.deepEqual(destinazioneDi("parts.card.states.hover.textColor", TOKEN), {
    selezione: { tipo: "parte", nome: "card" },
    stato: "hover",
    ispettore: "stile",
  });
  assert.equal(destinazioneDi("parts.card.motion.enter", TOKEN)?.ispettore, "movimento");
  assert.deepEqual(destinazioneDi("variant.parts.card.radius", TOKEN)?.tema, "variante");
  // I token hanno i punti nel nome: vince il più lungo che c'è.
  assert.deepEqual(destinazioneDi("tokens.color.accent.soft", TOKEN)?.selezione, {
    tipo: "token",
    id: "color.accent.soft",
  });
  assert.deepEqual(destinazioneDi("tokens.shadow.1.layers.0.color", TOKEN)?.selezione, {
    tipo: "token",
    id: "shadow.1",
  });
  assert.deepEqual(destinazioneDi("motion.animations.sale.frames.1.at", TOKEN)?.selezione, {
    tipo: "animazione",
    nome: "sale",
  });
  assert.deepEqual(destinazioneDi("layout.shell.children.2.children.0", TOKEN)?.selezione, {
    tipo: "nodo",
    via: [2, 0],
  });
  assert.equal(destinazioneDi("tokens.inesistente", TOKEN), null);
});
