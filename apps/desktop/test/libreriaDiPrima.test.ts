import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * La copia del Microsoft Store e la libreria che c'era (R20-3, D-56/D-97).
 *
 * La cartella separata la prova il nucleo (`lib.rs`,
 * `la_copia_dello_store_non_apre_la_libreria_della_copia_nsis`), la copia della
 * libreria pure (`trasloco::copia_la_libreria_di`). Qui la metà che un DOM
 * nelle prove non c'è per vedere: la proposta è montata nella finestra, fuori
 * dal telefono; chiede prima di copiare, e la sua X (Escape, il velo) vale
 * «Comincia da capo», così la domanda non torna a ogni avvio; e arriva a
 * comandi registrati.
 */

const leggi = (percorso: string) =>
  readFileSync(new URL(percorso, import.meta.url), "utf8");

const APP = leggi("../src/App.tsx");
const PROPOSTA = leggi("../src/parti/LibreriaDiPrima.tsx");
const IPC = leggi("../src/ipc.ts");
const LIB = leggi("../src-tauri/src/lib.rs");

test("la proposta è montata, e non sul telefono", () => {
  assert.match(APP, /\{!MOBILE && <LibreriaDiPrima onErrore=\{segnalaErrore\} \/>\}/);
});

test("copiare si chiede, e la X vale «Comincia da capo»", () => {
  assert.match(PROPOSTA, /onChiudi=\{rifiuta\}/);
  const rifiuta = PROPOSTA.slice(PROPOSTA.indexOf("const rifiuta"), PROPOSTA.indexOf("const copia"));
  assert.ok(rifiuta.includes("ipc.storeRifiutaLibreriaDiPrima()"));
  const copia = PROPOSTA.slice(PROPOSTA.indexOf("const copia"));
  assert.ok(copia.includes(".storeCopiaLibreriaDiPrima()"));
  // Mentre copia non si chiude: chiusa a metà, nessuno racconterebbe l'esito.
  assert.match(PROPOSTA, /chiudibile=\{!copiando\}/);
});

test("i comandi ci sono", () => {
  for (const comando of [
    "store_libreria_di_prima",
    "store_copia_libreria_di_prima",
    "store_rifiuta_libreria_di_prima",
  ]) {
    assert.ok(IPC.includes(`("${comando}")`), `ipc.ts non chiama ${comando}`);
    assert.ok(LIB.includes(`manutenzione::${comando},`), `lib.rs non registra ${comando}`);
  }
});
