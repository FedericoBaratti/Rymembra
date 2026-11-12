import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { provenienzaDi } from "../src/provenienzaEsplora";

test("«Tieni» e «Nella lista» registrano la frase dei risultati, non quella della casella", () => {
  // Cercato «debussy», poi scritto «coltr» senza Invio: i risultati a schermo
  // sono ancora di Debussy, e la provenienza è la loro.
  assert.equal(provenienzaDi({ tipo: "esito", frase: "debussy" }, "coltr"), "debussy");
  assert.equal(provenienzaDi({ tipo: "esito", frase: "debussy" }, ""), "debussy");
  // Senza un esito a schermo i tasti non ci sono; il ripiego è la casella.
  assert.equal(provenienzaDi({ tipo: "ferma" }, "  satie "), "satie");
});

test("i tasti di riga di Esplora dicono di quale brano sono", () => {
  // «Ascolta, In coda, Ascolta, In coda…» su cinquanta risultati non dice
  // niente a chi naviga per pulsanti: ogni tasto della riga è descritto dal
  // titolo della sua riga.
  const esplora = readFileSync(new URL("../src/schermate/Esplora.tsx", import.meta.url), "utf8");
  const riga = esplora.slice(esplora.indexOf('className="list-row explore-row"'), esplora.indexOf("</li>"));
  assert.match(riga, /<span className="titolo" id=\{idTitolo\}>/);
  const tasti = [...riga.matchAll(/<button\b[\s\S]*?>/g)].map((m) => m[0]);
  assert.ok(tasti.length >= 6, `${tasti.length} tasti`);
  for (const tasto of tasti) assert.match(tasto, /aria-describedby=\{idTitolo\}/, tasto);
});

// ── R13b-10: una riga «in libreria» tolta nel frattempo ────────────────────

test("Ascolta e In coda su una riga tolta dalla libreria la rimettono, o lo dicono", () => {
  const esplora = readFileSync(new URL("../src/schermate/Esplora.tsx", import.meta.url), "utf8");
  // Un'unica strada per avere l'identificativo, usata da tutti e due i tasti.
  const ascolta = esplora.slice(esplora.indexOf("const ascolta = useCallback"), esplora.indexOf("const accoda = useCallback"));
  const accoda = esplora.slice(esplora.indexOf("const accoda = useCallback"), esplora.indexOf("Tieni una copia"));
  for (const corpo of [ascolta, accoda]) {
    assert.match(corpo, /await idInLibreria\(riga\)/);
    assert.match(corpo, /explore\.gone/);
  }
  const strada = esplora.slice(esplora.indexOf("const idInLibreria = useCallback"), esplora.indexOf("const ascolta = useCallback"));
  // Se l'identificativo non c'è per una riga che diceva «in libreria», la si
  // rimette e si chiede di nuovo, una volta.
  assert.match(strada, /riga\.inLibreria/);
  assert.equal([...strada.matchAll(/ipc\.esploraIdentificativo\(/g)].length, 2);
});

// ── R13b-11: l'esito della ricerca arriva al lettore di schermo ────────────

test("Esplora ha una sola regione viva, sempre montata, che dice cerca/esito/vuoto/guasto", () => {
  const esplora = readFileSync(new URL("../src/schermate/Esplora.tsx", import.meta.url), "utf8");
  assert.equal([...esplora.matchAll(/role="status"/g)].length, 1);
  const regione = esplora.slice(esplora.indexOf('role="status"') - 200, esplora.indexOf('role="status"') + 200);
  assert.match(regione, /solo-lettori/);
  assert.match(regione, /\{annuncio\}/);
  const annuncio = esplora.slice(esplora.indexOf("const annuncio ="), esplora.indexOf("const annuncio =") + 900);
  for (const chiave of ["explore.searching", "explore.results", "explore.empty.title", "explore.down.title", "explore.cancelled"]) {
    assert.ok(annuncio.includes(chiave), chiave);
  }
});
