import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import {
  conAnnuncio,
  fasciaDaRitirare,
  quandoRispondonoLeRadici,
  radiciSpente,
} from "../src/radiciVive";

/*
 * R18-11, il resto: una radice che non risponde lo dice anche nelle
 * Impostazioni, e la fascia rossa «il file di questo brano non si raggiunge»
 * se ne va quando la sonda vede di nuovo tutte le radici — «Ricontrolla»
 * nelle Cartelle, o la scheda delle cartelle nelle Impostazioni.
 */
const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

test("la sonda annuncia quel che risponde a chi ascolta", async () => {
  const sentite: [readonly string[], readonly boolean[]][] = [];
  const smetti = quandoRispondonoLeRadici((radici, vive) => sentite.push([radici, vive]));
  const leggi = conAnnuncio(async (radici) => radici.map((r) => r !== "E:\\"));
  assert.deepEqual(await leggi(["D:\\Musica", "E:\\"]), [true, false]);
  assert.deepEqual(sentite, [[["D:\\Musica", "E:\\"], [true, false]]]);
  smetti();
  await leggi(["D:\\Musica"]);
  assert.equal(sentite.length, 1, "chi ha smesso non sente più");
  // Una sonda che fallisce non annuncia niente: non ha detto niente.
  const ascolto = quandoRispondonoLeRadici(() => assert.fail("annunciata"));
  await assert.rejects(conAnnuncio(() => Promise.reject(new Error("caduta")))(["D:\\"]));
  ascolto();
});

test("la fascia del file irraggiungibile se ne va solo con tutte le radici vive", () => {
  assert.equal(fasciaDaRitirare("playback.sourceUnavailable", [true, true]), true);
  // Una radice ancora spenta: il brano poteva stare proprio lì.
  assert.equal(fasciaDaRitirare("playback.sourceUnavailable", [true, false]), false);
  // Gli altri guasti non sono di una radice staccata.
  assert.equal(fasciaDaRitirare("net.offline", [true]), false);
  assert.equal(fasciaDaRitirare(null, [true]), false);
  // Una sonda di niente non dice niente.
  assert.equal(fasciaDaRitirare("playback.sourceUnavailable", []), false);
});

test("le radici spente sono quelle che hanno risposto di no", () => {
  assert.deepEqual([...radiciSpente(["A", "B", "C"], [true, false])], ["B"]);
  assert.deepEqual([...radiciSpente(["A"], [])], []);
});

test("cablaggio: chi legge la sonda annuncia, App ritira la fascia, le Impostazioni segnano la radice", () => {
  // Tutte le letture della finestra passano da `conAnnuncio`: anche il
  // «Ricontrolla» delle Cartelle, che legge da `useDati().ipc`.
  assert.match(sorgente("dati.tsx"), /cartelleRadiciVive: conAnnuncio\(ipc\.cartelleRadiciVive\)/);
  assert.match(sorgente("schermate/Cartelle.tsx"), /letture\s*\.cartelleRadiciVive\(radici\)/);
  const app = sorgente("App.tsx");
  assert.match(app, /quandoRispondonoLeRadici\(/);
  assert.match(app, /if \(fasciaDaRitirare\(guasto\?\.codice, vive\)\) chiudiErrore\(\);/);
  const impostazioni = sorgente("schermate/Impostazioni.tsx");
  assert.match(impostazioni, /letture\s*\.cartelleRadiciVive\(radici\)/);
  assert.match(impostazioni, /data-spenta=\{spente\.has\(c\) \|\| undefined\}/);
  assert.match(impostazioni, /t\("settings\.roots\.offline"\)/);
  const it = JSON.parse(sorgente("lingue/it.json")) as Record<string, string>;
  const en = JSON.parse(sorgente("lingue/en.json")) as Record<string, string>;
  assert.equal(it["settings.roots.offline"], "Non risponde");
  assert.equal(en["settings.roots.offline"], "Not responding");
});
