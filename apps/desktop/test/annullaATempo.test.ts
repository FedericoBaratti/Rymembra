import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import {
  annullaATempo,
  contoAnnulla,
  pilaAnnulli,
  type Orologio,
} from "../src/annullaATempo";

/*
 * Gli «Annulla» a tempo (R13d-2, D-40): il conto che si ferma col fuoco o il
 * puntatore sopra, `Ctrl+Z` per l'ultimo, e la conferma prima di eliminare
 * una playlist.
 */

/** Un orologio che va avanti solo quando lo dice la prova. */
function orologioFinto() {
  let adesso = 0;
  let prossimo = 1;
  const attese = new Map<number, { quando: number; fai: () => void }>();
  const orologio: Orologio = {
    imposta: (fai, ms) => {
      const id = prossimo++;
      attese.set(id, { quando: adesso + ms, fai });
      return id;
    },
    togli: (id) => {
      attese.delete(id);
    },
  };
  const avanza = (ms: number) => {
    adesso += ms;
    for (const [id, attesa] of [...attese]) {
      if (attesa.quando <= adesso) {
        attese.delete(id);
        attesa.fai();
      }
    }
  };
  return { orologio, avanza };
}

const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

test("l_annulla_non_scade_col_fuoco_sopra", () => {
  const { orologio, avanza } = orologioFinto();
  let scaduto = false;
  const conto = contoAnnulla(() => (scaduto = true), 10_000, orologio);
  conto.parti();
  avanza(9_000);
  conto.tieni("fuoco");
  avanza(60_000);
  assert.equal(scaduto, false, "col fuoco sopra non scade");
  // Riparte da capo quando il fuoco se ne va: dieci secondi interi.
  conto.lascia("fuoco");
  avanza(9_999);
  assert.equal(scaduto, false);
  avanza(1);
  assert.equal(scaduto, true);
});

test("fuoco e puntatore: il conto riparte solo quando se ne sono andati tutti e due", () => {
  const { orologio, avanza } = orologioFinto();
  let scaduto = false;
  const conto = contoAnnulla(() => (scaduto = true), 10_000, orologio);
  conto.parti();
  conto.tieni("puntatore");
  conto.tieni("fuoco");
  conto.lascia("puntatore");
  avanza(30_000);
  assert.equal(scaduto, false, "il fuoco lo tiene ancora");
  conto.lascia("fuoco");
  avanza(10_000);
  assert.equal(scaduto, true);
});

test("un riquadro tolto dimentica le prese, e un avviso nuovo riparte da capo", () => {
  const { orologio, avanza } = orologioFinto();
  let scadenze = 0;
  const conto = contoAnnulla(() => scadenze++, 10_000, orologio);
  conto.parti();
  conto.tieni("puntatore");
  conto.ferma();
  conto.parti();
  avanza(10_000);
  assert.equal(scadenze, 1, "la presa del riquadro tolto non tiene quello nuovo");
});

test("Ctrl+Z fa l'ultimo «Annulla» ancora a schermo, una volta sola", () => {
  const pila = pilaAnnulli();
  const fatti: string[] = [];
  const { orologio, avanza } = orologioFinto();
  const coda = annullaATempo<string>((a) => fatti.push(`coda ${a}`), { orologio, pila });
  const playlist = annullaATempo<string>((a) => fatti.push(`playlist ${a}`), { orologio, pila });
  coda.mostra("200 brani");
  playlist.mostra("Notte");
  assert.equal(pila.annullaLUltimo(), true);
  assert.deepEqual(fatti, ["playlist Notte"]);
  assert.equal(playlist.avviso.leggi(), null, "il riquadro se ne va, in tutte le copie");
  assert.equal(pila.annullaLUltimo(), true, "poi quello sotto");
  assert.deepEqual(fatti, ["playlist Notte", "coda 200 brani"]);
  assert.equal(pila.annullaLUltimo(), false, "niente più da annullare");
  // Un avviso scaduto si ritira anche da Ctrl+Z.
  coda.mostra("12 brani");
  avanza(10_000);
  assert.equal(coda.avviso.leggi(), null);
  assert.equal(pila.annullaLUltimo(), false);
  // E il tasto dopo Ctrl+Z non rifà il gesto.
  coda.mostra("5 brani");
  pila.annullaLUltimo();
  coda.annulla();
  assert.deepEqual(fatti, ["playlist Notte", "coda 200 brani", "coda 5 brani"]);
});

test("il cablaggio: i due riquadri, Ctrl+Z e la conferma prima di eliminare una playlist", () => {
  // La coda: lo stato è uno per le due copie, e il riquadro ha la presa.
  const coda = sorgente("parti/AvvisoCoda.tsx");
  assert.match(coda, /const ANNULLO = annullaATempo<CodaSostituita>\(/);
  assert.match(coda, /<div className="notizia toast-card" \{\.\.\.ANNULLO\.presa\}>/);
  assert.doesNotMatch(coda, /setTimeout/);
  // La playlist: lo stesso in `App`, e una domanda prima.
  const app = sorgente("App.tsx");
  assert.match(app, /useState\(\(\) =>\s+annullaATempo<PlaylistCancellata>\(/);
  assert.match(app, /<div className="notizia toast-card" \{\.\.\.annulloPlaylist\.presa\}>/);
  assert.match(app, /cancella: \(\) => setDaCancellare\(\{ playlist: p, risposta: unaVolta\(\) \}\)/);
  const conferma = app.slice(app.indexOf("{daCancellare && ("));
  assert.match(conferma.slice(0, conferma.indexOf("\n        )}")), /<ConfermaGesto/);
  // Ctrl+Z: un comando come gli altri, di serie, e mai dentro un campo.
  const tastiera = sorgente("tastiera.ts");
  assert.match(tastiera, /annulla: \["Ctrl\+z"\]/);
  assert.match(tastiera, /if \(comando === "annulla" && siStaScrivendo\(e\.target\)\) return;/);
  assert.match(app, /annulla: \(\) => \{\s+ANNULLI\.annullaLUltimo\(\);/);
});
