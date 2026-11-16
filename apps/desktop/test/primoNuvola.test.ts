/**
 * Il primo avvio con una cartella del cloud che sta ancora arrivando (R12-9).
 *
 * Una cartella del cloud aggiunta da «Scegli un'altra cartella…» entra fra le
 * sorvegliate spuntata, con la notizia «i brani compaiono mentre arrivano», e
 * la scansione cammina sullo specchio del servizio: la prima lettura di un drive
 * grande dura un minuto, e se la camminata finisce prima che lo specchio sia
 * pieno l'esito è «0 brani». `Primo` diceva allora «Rymembra non ha trovato
 * brani… Controlla di aver scelto quelle giuste» mentre i brani stavano
 * arrivando. Non riprodotto dal vivo (serve un drive grande); la correzione
 * guarda il conto del servizio — `stato: "elenco"` — fra le cartelle scritte.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import * as primo from "../src/tastoPrimo";

const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

const CARTELLA = "nuvola://onedrive-k3x7q2ab/Musica";
const conto = (stato: string, id = "onedrive-k3x7q2ab") => ({
  conto: id,
  servizio: id.split("-")[0] ?? "",
  stato,
});

test("un conto che sta ancora elencando una cartella scritta è «in arrivo» (R12-9)", () => {
  assert.equal(typeof primo.servizioInArrivo, "function", "manca servizioInArrivo");
  const arriva = primo.servizioInArrivo;
  assert.equal(arriva([CARTELLA], [conto("elenco")]), "OneDrive");
  // Finito di elencare: non è più in arrivo, e una lettura vuota è vuota davvero.
  assert.equal(arriva([CARTELLA], [conto("collegato")]), null);
  assert.equal(arriva([CARTELLA], [conto("errore")]), null);
  // Un altro conto che elenca non c'entra con le cartelle scritte.
  assert.equal(arriva([CARTELLA], [conto("elenco", "gdrive-zz11")]), null);
  // Una cartella sul disco non aspetta nessun servizio.
  assert.equal(arriva(["C:\\Users\\X\\Music"], [conto("elenco")]), null);
  // I conti non ancora arrivati dal nucleo: niente da dire.
  assert.equal(arriva([CARTELLA], null), null);
});

test("la lettura vuota non si dice finché una cartella del cloud sta arrivando (R12-9)", () => {
  assert.equal(typeof primo.lettaVuota, "function", "manca lettaVuota");
  const base = { scansionando: false, inCorso: false, haEsito: true, brani: 0, nuvolaInArrivo: null };
  assert.equal(primo.lettaVuota(base), true, "letto tutto e non c'era niente: vuota");
  assert.equal(primo.lettaVuota({ ...base, nuvolaInArrivo: "OneDrive" }), false, "i brani stanno arrivando");
  // Le altre condizioni di prima non cambiano.
  assert.equal(primo.lettaVuota({ ...base, scansionando: true }), false);
  assert.equal(primo.lettaVuota({ ...base, inCorso: true }), false);
  assert.equal(primo.lettaVuota({ ...base, haEsito: false }), false);
  assert.equal(primo.lettaVuota({ ...base, brani: 3 }), false);
});

test("`Primo` dice «sta ancora leggendo», e lo dice in tutte e due le lingue (R12-9)", () => {
  const componente = sorgente("Primo.tsx");
  assert.match(componente, /servizioInArrivo\(/, "chiede alla funzione pura quale servizio sta arrivando");
  assert.match(componente, /lettaVuota\(\{/, "la lettura vuota passa dalla funzione pura");
  assert.doesNotMatch(
    componente,
    /const lettaVuota = !scansionando/,
    "la condizione non è più scritta nel componente",
  );
  assert.match(componente, /t\("primo\.nuvola"/);
  for (const lingua of ["it", "en"]) {
    const voci = JSON.parse(sorgente(`lingue/${lingua}.json`)) as Record<string, string>;
    assert.match(voci["primo.nuvola"] ?? "", /\{servizio\}/, lingua);
  }
});
