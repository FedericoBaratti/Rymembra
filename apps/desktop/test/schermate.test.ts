import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * Prove sui sorgenti delle schermate, dove la regola sta nel cablaggio: chi
 * riceve un errore, che cosa fa un tasto, che nome ha un comando. Come
 * `impostazioni.test.ts`, si legge il sorgente e si cerca il punto in cui la
 * regola è scritta.
 */
const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

test("Impostazioni, scheda dei testi: le scritture che falliscono si dicono", () => {
  const impostazioni = sorgente("schermate/Impostazioni.tsx");
  for (const comando of ["testiRete", "testiAccanto"]) {
    const dopo = impostazioni.slice(impostazioni.indexOf(`.${comando}(`));
    const catena = dopo.slice(0, dopo.indexOf(";"));
    assert.doesNotMatch(catena, /\.catch\(\(\) => \{\}\)/, comando);
    assert.match(catena, /\.catch\(onErrore\)/, comando);
  }
  const riempi = impostazioni.slice(impostazioni.indexOf("ipc.testiFerma() : ipc.testiRiempi()"));
  assert.match(riempi.slice(0, riempi.indexOf("}}")), /\.catch\(onErrore\)/);
  assert.match(impostazioni, /<SchedaTesti onErrore=\{onErrore\} \/>/);
});

test("Impostazioni, ricerca: Invio fa quel che fa il clic, scheda compresa", () => {
  const impostazioni = sorgente("schermate/Impostazioni.tsx");
  const invio = impostazioni.slice(impostazioni.indexOf('if (e.key === "Enter" && prima)'));
  assert.match(invio.slice(0, invio.indexOf("\n")), /apriVoce\(prima\)/);
  assert.match(impostazioni, /onClick=\{\(\) => apriVoce\(v\)\}/);
  const apri = impostazioni.slice(impostazioni.indexOf("const apriVoce"));
  assert.match(apri.slice(0, apri.indexOf("};")), /vaiAllaScheda\(t\(v\.scheda\)\)/);
});
