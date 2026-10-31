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
