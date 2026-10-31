import { test } from "node:test";
import assert from "node:assert/strict";

import { avvioProvino, curvaCss, durataMs, gestoRombo, versoAnimazione, versoKeyframe } from "../src/studio/fotogrammi";

const RISOLVI = {
  colore: (v: unknown) => (typeof v === "string" ? v : null),
  ombra: () => null,
  curva: (nome: string) =>
    nome === "puntina" ? { kind: "cubicBezier", points: [0.2, 0.8, 0.2, 1] } : undefined,
};

test("le trasformazioni vanno nell'ordine del compilatore, non in quello scritto", () => {
  const k = versoKeyframe({ at: 50, scale: 1.2, rotate: 90, translateY: 4, skewX: 5 }, RISOLVI, null);
  assert.equal(k.transform, "translateY(4px) rotate(90deg) skewX(5deg) scale(1.2)");
  assert.equal(k.offset, 0.5);
});

test("filtri, colori e ritagli", () => {
  const k = versoKeyframe(
    {
      at: 0,
      blur: "2px",
      hueRotate: 30,
      backgroundColor: "#fff",
      clip: { inset: ["0%", "10%", "0%", "10%"] },
    },
    RISOLVI,
    "50% 50%",
  );
  assert.equal(k.filter, "blur(2px) hue-rotate(30deg)");
  assert.equal(k.backgroundColor, "#fff");
  assert.equal(k.clipPath, "inset(0% 10% 0% 10%)");
  assert.equal(k.transformOrigin, undefined, "l'origine vale solo con una trasformazione");
});

test("la curva va su ogni fotogramma, come in CSS", () => {
  const { fotogrammi, opzioni } = versoAnimazione(
    {
      duration: "0.3s",
      easing: "puntina",
      iterations: "infinite",
      frames: [
        { at: 0, opacity: 0 },
        { at: 100, opacity: 1 },
      ],
    },
    RISOLVI,
  );
  assert.equal(opzioni.duration, 300);
  assert.equal(opzioni.easing, "linear");
  assert.equal(opzioni.iterations, Number.POSITIVE_INFINITY);
  assert.equal(fotogrammi[0]?.easing, "cubic-bezier(0.2, 0.8, 0.2, 1)");
});

test("durate e curve", () => {
  assert.equal(durataMs("250ms"), 250);
  assert.equal(durataMs("1.5s"), 1500);
  assert.equal(curvaCss({ kind: "steps", count: 4, position: "start" }, RISOLVI), "steps(4, start)");
  assert.equal(curvaCss("ease-out", RISOLVI), "ease-out");
});

test("un rombo riportato al punto di partenza ci torna davvero", () => {
  // Il confronto era col fotogramma del pointerdown: tornando a 40 la
  // scrittura si saltava, e il rombo restava all'ultima posizione scritta.
  const scrivere = gestoRombo(40);
  const scritte = [50, 45, 45, 40].filter((at) => scrivere(at));
  assert.deepEqual(scritte, [50, 45, 40]);
});

test("con «Riduci movimento» il provino non parte da solo, e al tasto suona una volta", () => {
  // La Web Animations API non legge --motion-scale: il provino pulsava senza
  // fine anche per chi aveva chiesto di non muoversi.
  assert.equal(avvioProvino(true, false), "fermo");
  assert.equal(avvioProvino(true, true), "unaVolta");
  assert.equal(avvioProvino(false, false), "suona");
  assert.equal(avvioProvino(false, true), "suona");
});
