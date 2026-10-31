import { test } from "node:test";
import assert from "node:assert/strict";

import { creaArchivioPosizione, passoDellaBarra } from "../src/posizione";

test("su un brano lungo in una barra stretta la barra si ridisegna di rado", () => {
  // Quattro minuti nella colonna: 300 px CSS a 125% sono 375 pixel veri, cioè
  // 640 ms per pixel. Il passo più lungo che divide il secondo e ci sta: 500.
  assert.equal(passoDellaBarra(240_000, 300 * 1.25), 500);
  // Un'ora: un pixel vale dodici secondi, ma il secondo scritto ai lati vuole
  // cambiare ogni secondo.
  assert.equal(passoDellaBarra(3_600_000, 300), 1000);
});

test("su un brano corto o una barra larga il passo scende, mai sotto 50", () => {
  assert.equal(passoDellaBarra(30_000, 300), 100);
  assert.equal(passoDellaBarra(240_000, 1200 * 1.5), 125);
  assert.equal(passoDellaBarra(20_000, 600), 50);
});

test("ogni passo divide il secondo: il tempo scritto cambia col secondo vero", () => {
  for (const durata of [5_000, 61_000, 240_000, 1_000_000, 7_200_000]) {
    for (const pixel of [80, 300, 375, 1024, 3840]) {
      const passo = passoDellaBarra(durata, pixel);
      assert.equal(1000 % passo, 0, `${durata} ms su ${pixel} px → ${passo}`);
      assert.ok(passo >= 50);
    }
  }
});

test("senza una misura vale il passo fitto di prima", () => {
  assert.equal(passoDellaBarra(0, 300), 50);
  assert.equal(passoDellaBarra(240_000, 0), 50);
  assert.equal(passoDellaBarra(Number.NaN, 300), 50);
  assert.equal(passoDellaBarra(240_000, Number.NaN), 50);
});

test("l'archivio avvisa solo quando la posizione cambia", () => {
  const archivio = creaArchivioPosizione();
  let avvisi = 0;
  const smetti = archivio.iscrivi(() => avvisi++);
  archivio.pubblica(50);
  archivio.pubblica(50);
  archivio.pubblica(100);
  smetti();
  archivio.pubblica(150);
  assert.equal(avvisi, 2);
  assert.equal(archivio.leggi(), 150);
});
