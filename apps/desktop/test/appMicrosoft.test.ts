import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * Microsoft: prima del modulo si dice che serve un account Azure (D-47).
 *
 * La frase d'apertura prometteva «gratis e una volta sola, in una decina di
 * minuti», e solo il primo passo diceva che un account Microsoft personale
 * da solo non basta e che serve un account Azure, che di solito chiede una
 * carta per la verifica.
 */
const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");
const LINGUE = {
  it: JSON.parse(leggi("../src/lingue/it.json")) as Record<string, string>,
  en: JSON.parse(leggi("../src/lingue/en.json")) as Record<string, string>,
};

test("la frase prima del modulo Microsoft nomina Azure e la carta", () => {
  for (const [lingua, voci] of Object.entries(LINGUE)) {
    const frase = voci["folders.add.needApp.microsoft"] ?? "";
    assert.match(frase, /Azure/, lingua);
    assert.match(frase, /carta|card/, lingua);
    assert.doesNotMatch(frase, /dieci minuti|decina di minuti|ten minutes/, lingua);
    assert.match(voci["settings.cloudApps.body"] ?? "", /Azure/, lingua);
  }
});

test("«Aggiungi una cartella» usa la frase di Microsoft per Microsoft", () => {
  const sorgente = leggi("../src/AggiungiCartella.tsx");
  assert.match(
    sorgente,
    /piattaforma\.chiave === "microsoft"\s*\?\s*"folders\.add\.needApp\.microsoft"/,
  );
});
