import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import * as camera from "../src/parti/cameraSpettro";

/*
 * La camera della scena dello spettro: dove sta l'occhio, fotogramma per
 * fotogramma. Pura, perché `Spettro3D` porta con sé WebGL e l'IPC.
 */

const manopole = { oscillazione: 1, pulsazione: 1, occhio: 0.9 };

test("con il moto ridotto la camera non dipende dal colpo", () => {
  assert.equal(typeof camera.posaCamera, "function");
  const ferma = camera.posaCamera(true, 0, 12.3, manopole);
  const colpita = camera.posaCamera(true, 1, 12.3, manopole);
  assert.deepEqual(colpita, ferma);
  // Senza il moto ridotto il colpo alza l'occhio, com'è giusto.
  assert.notDeepEqual(
    camera.posaCamera(false, 1, 12.3, manopole),
    camera.posaCamera(false, 0, 12.3, manopole),
  );
});

test("la camera non salta al risveglio", () => {
  // Cento fotogrammi a sessanta hertz, poi un minuto di sonno: il risveglio
  // azzera `precedente`, e il primo fotogramma dopo vale 16,7 ms di scena.
  let adesso = 1000;
  let precedente = 0;
  let tempoScena = 0;
  let x = 0;
  for (let i = 0; i < 100; i += 1) {
    adesso += 16.7;
    const dt = camera.passoTempo(adesso, precedente);
    precedente = adesso;
    tempoScena += dt;
    x = camera.posaCamera(false, 0, tempoScena / 1000, manopole).occhioX;
  }
  adesso += 60_000;
  precedente = 0;
  const dt = camera.passoTempo(adesso, precedente);
  tempoScena += dt;
  const dopo = camera.posaCamera(false, 0, tempoScena / 1000, manopole).occhioX;
  assert.ok(Math.abs(dopo - x) < 0.01, `salto ${Math.abs(dopo - x)}`);
});

test("la densità della tela segue devicePixelRatio e non il fattore del monitor", () => {
  const sorgente = readFileSync(new URL("../src/parti/Spettro3D.tsx", import.meta.url), "utf8");
  // `onScaleChanged` porta il fattore del **monitor**, senza lo zoom della
  // finestra: scritto nella densità, a zoom 125% su un monitor al 100% dava
  // una tela all'80% dei pixel.
  assert.ok(!sorgente.includes("payload.scaleFactor"), "la densità viene ancora da scaleFactor");
  assert.ok(sorgente.includes("matchMedia(`(resolution: "), "manca l'ascolto della risoluzione");
});
