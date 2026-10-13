import { test } from "node:test";
import assert from "node:assert/strict";

import { chiUsaAnimazione, chiUsaColore, chiUsaCurva, rinomina } from "../src/studio/riferimenti";

const DOC = {
  palette: { etichetta: "#a3301c", ottone: "#b98a3e" },
  patterns: {
    luce: { effect: "radialGradient", stops: [{ color: { $palette: "ottone", alpha: 0.1 } }] },
  },
  tokens: { "color.accent": { $palette: "etichetta" } },
  motion: {
    easings: { puntina: { kind: "cubicBezier", points: [0.2, 0.8, 0.2, 1] } },
    route: { duration: "300ms", easing: "puntina" },
    animations: { sale: { duration: "300ms", easing: "puntina", frames: [] } },
  },
  parts: {
    card: {
      background: [{ $pattern: "luce" }, { effect: "solid", color: { $palette: "etichetta" } }],
      motion: { enter: "sale", hover: "sale" },
    },
  },
  variant: { palette: { etichetta: "#ff6b4a" } },
};

test("chi usa un colore, un'animazione, una curva", () => {
  assert.deepEqual(chiUsaColore(DOC, "etichetta").sort(), [
    "parts.card.background.1.color",
    "tokens.color.accent",
  ]);
  assert.deepEqual(chiUsaAnimazione(DOC, "sale"), ["parts.card.motion.enter", "parts.card.motion.hover"]);
  assert.deepEqual(chiUsaCurva(DOC, "puntina").sort(), [
    "motion.animations.sale.easing",
    "motion.route.easing",
  ]);
});

test("rinominare un colore porta con sé i richiami e la variante", () => {
  const dopo = rinomina(DOC, "colore", "etichetta", "rosso") as typeof DOC | null;
  assert.ok(dopo);
  assert.deepEqual(Object.keys(dopo.palette), ["rosso", "ottone"], "l'ordine resta");
  assert.deepEqual(dopo.tokens["color.accent"], { $palette: "rosso" });
  assert.equal(JSON.stringify(dopo).includes('"etichetta"'), false);
  assert.deepEqual(Object.keys(dopo.variant.palette), ["rosso"]);
  // Il documento di partenza non si tocca.
  assert.ok("etichetta" in DOC.palette);
});

test("rinominare un'animazione e una curva", () => {
  const conAnimazione = rinomina(DOC, "animazione", "sale", "cresce") as typeof DOC;
  assert.deepEqual(conAnimazione.parts.card.motion, { enter: "cresce", hover: "cresce" });
  const conCurva = rinomina(DOC, "curva", "puntina", "morbida") as typeof DOC;
  assert.equal(conCurva.motion.route.easing, "morbida");
  assert.equal(conCurva.motion.animations.sale.easing, "morbida");
  assert.ok("morbida" in conCurva.motion.easings);
});

test("un nome già preso non si rinomina", () => {
  assert.equal(rinomina(DOC, "colore", "etichetta", "ottone"), null);
  assert.equal(rinomina(DOC, "colore", "manca", "altro"), null);
});
