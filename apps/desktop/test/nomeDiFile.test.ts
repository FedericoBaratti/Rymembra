import { test } from "node:test";
import assert from "node:assert/strict";

import { nomeDiFile } from "../src/nomeDiFile";

/*
 * Il nome che «Esporta» propone nel dialogo di salvataggio.
 *
 * Il nome della playlist passava intero come `defaultPath`: «AC/DC» diventava
 * la cartella relativa «AC» e il nome «DC.m3u8», e i caratteri che Windows
 * vieta rendevano il nome proposto non salvabile.
 */

test("il nome proposto non ha caratteri vietati da Windows", () => {
  assert.equal(nomeDiFile("AC/DC"), "AC-DC");
  assert.equal(nomeDiFile("Live: 2024?"), "Live- 2024-");
  const brutto = nomeDiFile('a\b/c:d*e?f"g<h>i|j');
  for (const vietato of ["\\", "/", ":", "*", "?", '"', "<", ">", "|"]) {
    assert.ok(!brutto.includes(vietato), vietato);
  }
  assert.ok(brutto.includes("a") && brutto.includes("j"));
});

test("niente caratteri di controllo, punti e spazi in coda", () => {
  assert.equal(nomeDiFile("Serata\t\n"), "Serata");
  assert.equal(nomeDiFile("Fine..."), "Fine");
  assert.equal(nomeDiFile("  Viaggio . "), "Viaggio");
});

test("un nome vuoto o riservato ripiega su «playlist»", () => {
  assert.equal(nomeDiFile(""), "playlist");
  assert.equal(nomeDiFile("///"), "---");
  assert.equal(nomeDiFile("..."), "playlist");
  assert.equal(nomeDiFile("CON"), "playlist");
  assert.equal(nomeDiFile("nul"), "playlist");
  assert.equal(nomeDiFile("COM1"), "playlist");
  assert.equal(nomeDiFile("Concerto"), "Concerto");
});
