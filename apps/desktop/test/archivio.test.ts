import { test } from "node:test";
import assert from "node:assert/strict";

import { creaArchivio, sceltaRicordata, ugualiInSuperficie } from "../src/archivio";

test("una scrittura sveglia chi è iscritto, e solo se il valore cambia", () => {
  const archivio = creaArchivio(1);
  let sveglie = 0;
  const smetti = archivio.iscrivi(() => (sveglie += 1));
  archivio.scrivi(2);
  archivio.scrivi(2);
  archivio.scrivi((prima) => prima);
  assert.equal(archivio.leggi(), 2);
  assert.equal(sveglie, 1);
  smetti();
  archivio.scrivi(3);
  assert.equal(sveglie, 1);
  assert.equal(archivio.leggi(), 3);
});

test("la scrittura per funzione parte dal valore di adesso", () => {
  const archivio = creaArchivio({ n: 1 });
  archivio.scrivi((prima) => ({ n: prima.n + 1 }));
  archivio.scrivi((prima) => ({ n: prima.n + 1 }));
  assert.deepEqual(archivio.leggi(), { n: 3 });
});

test("chi smette mentre viene avvisato non ferma gli altri", () => {
  const archivio = creaArchivio(0);
  const visti: string[] = [];
  const smettiPrimo = archivio.iscrivi(() => {
    visti.push("primo");
    smettiPrimo();
  });
  archivio.iscrivi(() => visti.push("secondo"));
  archivio.scrivi(1);
  archivio.scrivi(2);
  assert.deepEqual(visti, ["primo", "secondo", "secondo"]);
});

test("una scelta si rifà solo quando cambia la sorgente", () => {
  let conti = 0;
  const scegli = sceltaRicordata((v: { a: number }) => {
    conti += 1;
    return v.a * 2;
  });
  const sorgente = { a: 2 };
  assert.equal(scegli(sorgente), 4);
  assert.equal(scegli(sorgente), 4);
  assert.equal(conti, 1);
  assert.equal(scegli({ a: 3 }), 6);
  assert.equal(conti, 2);
});

test("una scelta uguale tiene l'identità di prima", () => {
  const scegli = sceltaRicordata((v: { a: number; b: number }) => ({ a: v.a }), ugualiInSuperficie);
  const prima = scegli({ a: 1, b: 1 });
  const dopo = scegli({ a: 1, b: 2 });
  assert.equal(dopo, prima);
  const altra = scegli({ a: 2, b: 2 });
  assert.notEqual(altra, prima);
  assert.deepEqual(altra, { a: 2 });
});

test("uguali in superficie: stesse chiavi e stessi valori, non oltre", () => {
  const lista = [1];
  assert.equal(ugualiInSuperficie({ x: 1, y: lista }, { x: 1, y: lista }), true);
  assert.equal(ugualiInSuperficie({ x: 1, y: lista }, { x: 1, y: [1] }), false);
  assert.equal(ugualiInSuperficie({ x: 1 }, { x: 1, y: undefined }), false);
  assert.equal(ugualiInSuperficie({ x: Number.NaN }, { x: Number.NaN }), true);
});
