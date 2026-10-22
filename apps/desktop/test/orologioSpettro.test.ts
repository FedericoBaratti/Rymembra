import { test } from "node:test";
import assert from "node:assert/strict";

import { OrologioSpettro } from "../src/parti/orologioSpettro";

const PASSO = 33;
const FOTOGRAMMA = 1000 / 60;

/**
 * Fa girare l'orologio come lo fa girare la scena: arrivi e fotogrammi in ordine
 * di tempo, la coda contata qui. Restituisce, per ogni fotogramma, il tempo e la
 * posizione della scena in file — file entrate meno una, più la frazione, che è
 * la stessa grandezza che misura il banco.
 */
function simula(arrivi: number[], fine: number) {
  const orologio = new OrologioSpettro(PASSO);
  orologio.azzera();
  let inCoda = 0;
  let entrate = 0;
  let a = 0;
  const posizioni: Array<[number, number]> = [];
  for (let t = 0; t <= fine; t += FOTOGRAMMA) {
    while (a < arrivi.length && (arrivi[a] ?? Infinity) <= t) {
      orologio.arrivo(arrivi[a] ?? t, inCoda);
      inCoda += 1;
      a += 1;
    }
    const { prendi, frazione } = orologio.avanza(t, inCoda);
    assert.ok(prendi <= inCoda, `prende ${prendi} file da una coda di ${inCoda}`);
    assert.ok(frazione >= 0 && frazione <= 1, `frazione ${frazione}`);
    inCoda -= prendi;
    entrate += prendi;
    posizioni.push([t, entrate - 1 + frazione]);
  }
  return { posizioni, inCoda };
}

/** Avanzamento per fotogramma, in multipli del dovuto, da `da` in poi. */
function rapporti(posizioni: Array<[number, number]>, da: number): number[] {
  const fuori: number[] = [];
  for (let i = 1; i < posizioni.length; i += 1) {
    const [t0, p0] = posizioni[i - 1] ?? [0, 0];
    const [t1, p1] = posizioni[i] ?? [0, 0];
    if (t0 < da) continue;
    fuori.push((p1 - p0) / ((t1 - t0) / PASSO));
  }
  return fuori;
}

/** Un generatore ripetibile in [0, 1). */
function caso(seme: number) {
  let s = seme >>> 0;
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

test("con arrivi regolari la scena avanza a velocità costante", () => {
  const arrivi = Array.from({ length: 300 }, (_, i) => 5 + i * PASSO);
  const { posizioni } = simula(arrivi, 9_000);
  const r = rapporti(posizioni, 2_000);
  for (const x of r) assert.ok(x > 0.93 && x < 1.07, `avanzamento ${x} con arrivi regolari`);
});

test("il ritmo irregolare del filo della finestra non diventa scatto", () => {
  // Il caso misurato: ±2 ms su ogni evento, e ogni tanto uno in ritardo di
  // dodici. Prima la scena avanzava fra 0,89 e 1,10 del dovuto per fotogramma.
  const r0 = caso(7);
  const arrivi = Array.from({ length: 600 }, (_, i) => {
    const tremolio = (r0() - 0.5) * 4;
    const inciampo = i % 47 === 0 ? 12 : 0;
    return 5 + i * PASSO + tremolio + inciampo;
  });
  const { posizioni } = simula(arrivi, 19_000);
  const r = rapporti(posizioni, 3_000);
  for (const x of r) assert.ok(x > 0.85 && x < 1.25, `avanzamento ${x} con il tremolio`);
  const medio = r.reduce((s, x) => s + x, 0) / r.length;
  assert.ok(Math.abs(medio - 1) < 0.02, `avanzamento medio ${medio}`);
});

test("uno stallo con raffica non fa saltare la scena e non la manda indietro", () => {
  // Il filo della finestra resta fermo 300 ms, poi consegna tutto insieme.
  const arrivi: number[] = [];
  for (let i = 0; i < 300; i += 1) {
    const t = 5 + i * PASSO;
    arrivi.push(t >= 4_000 && t < 4_300 ? 4_300 : t);
  }
  const { posizioni } = simula(arrivi, 9_500);
  const r = rapporti(posizioni, 1_000);
  for (const x of r) {
    assert.ok(x >= 0, `la scena è tornata indietro (${x})`);
    assert.ok(x <= 2.01, `un salto di ${x} volte il dovuto`);
  }
  // E il ritardo accumulato si recupera: a fine prova la scena è di nuovo a
  // ridosso dell'ultima fila arrivata.
  const ultima = posizioni[posizioni.length - 1];
  const arrivate = arrivi.filter((t) => t <= 9_500).length;
  assert.ok(ultima && arrivate - 1 - ultima[1] < 2.5, `indietro di ${arrivate - 1 - (ultima?.[1] ?? 0)} file`);
});

test("una riserva assurda si scarta invece di recuperarla per secondi", () => {
  // Un secondo e mezzo di stallo: quarantacinque file in un colpo. Recuperarle
  // al doppio vorrebbe dire un secondo e mezzo di avanti veloce.
  const arrivi: number[] = [];
  for (let i = 0; i < 300; i += 1) {
    const t = 5 + i * PASSO;
    arrivi.push(t >= 3_000 && t < 4_500 ? 4_500 : t);
  }
  const { posizioni } = simula(arrivi, 4_700);
  const ultima = posizioni[posizioni.length - 1];
  const arrivate = arrivi.filter((t) => t <= 4_700).length;
  assert.ok(ultima && arrivate - 1 - ultima[1] < 15, `indietro di ${arrivate - 1 - (ultima?.[1] ?? 0)} file`);
});

test("ferma, riparte con la fila davanti piccola", () => {
  // Dopo un azzeramento — il ciclo che riparte, una risoluzione nuova — la
  // scena aspetta la prima fila e la fa crescere da zero, invece di
  // mostrarla già alta.
  const orologio = new OrologioSpettro(PASSO);
  orologio.azzera();
  assert.deepEqual(orologio.avanza(0, 0), { prendi: 0, frazione: 1 });
  assert.deepEqual(orologio.avanza(FOTOGRAMMA, 0), { prendi: 0, frazione: 1 });
  orologio.arrivo(20, 0);
  const passo = orologio.avanza(2 * FOTOGRAMMA, 1);
  assert.equal(passo.prendi, 1);
  assert.equal(passo.frazione, 0);
  const dopo = orologio.avanza(3 * FOTOGRAMMA, 0);
  assert.ok(dopo.frazione > 0.3 && dopo.frazione < 0.7, `frazione ${dopo.frazione}`);
});
