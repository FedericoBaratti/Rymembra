/**
 * Lo schermo intero «In riproduzione» e la fine di quel che suonava: quando
 * non resta niente da mostrare si chiude, e quando la scheda del brano tarda
 * no (`schermoIntero.ts`).
 */
import { test } from "node:test";
import assert from "node:assert/strict";

import { nienteInCorso } from "../src/schermoIntero";

const brano = { id: 7 };

test("con la coda svuotata non c'è più niente in corso", () => {
  // «Svuota la coda» con lo schermo intero aperto: il nucleo pulisce e ferma,
  // e lo stato arriva senza brano e senza posizione. Lo schermo intero
  // restava «aperto» invisibile: si mangiava il primo Indietro, e si riapriva
  // da solo col brano fatto partire dopo.
  assert.equal(nienteInCorso({ brano: null, posizioneCoda: null }), true);
});

test("una scheda che tarda non è la fine", () => {
  // Un brano nuovo mentre una scansione tiene la libreria: il nucleo manda lo
  // stato senza la scheda (non la ricorda, è un altro brano), ma la coda è
  // sul brano. Chiudere qui chiuderebbe lo schermo intero a ogni cambio di
  // brano durante una scansione.
  assert.equal(nienteInCorso({ brano: null, posizioneCoda: 3 }), false);
  assert.equal(nienteInCorso({ brano: null, posizioneCoda: 0 }), false);
});

test("un brano che suona è in corso", () => {
  assert.equal(nienteInCorso({ brano, posizioneCoda: 0 }), false);
  // Una voce che non sta in coda — la calibrazione di «Ascolta insieme» — ha
  // la sua scheda anche senza posizione.
  assert.equal(nienteInCorso({ brano, posizioneCoda: null }), false);
});
