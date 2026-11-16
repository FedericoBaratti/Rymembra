/**
 * Sfoglia e il gesto «indietro» (R13-5, R13-6).
 *
 * - R13-5: sul computer Escape, `Alt+←` e il tasto indietro del mouse non
 *   chiudevano il disco, l'artista o la ricerca aperti: `suIndietro` lo leggeva
 *   solo il tasto Indietro del telefono, e la scala di `App` non conosceva i
 *   livelli di Sfoglia. L'utente ha accettato che salgano come nella libreria
 *   (DECISIONI-2026-11-11, «Escape risale i livelli di Sfoglia anche sul computer»).
 * - R13-6: sul telefono, con il dispositivo che non risponde più, la pagina è lo
 *   stato vuoto «non risponde» e i livelli non si disegnano, ma Indietro li
 *   chiudeva lo stesso uno per volta, senza che a schermo cambiasse niente.
 *
 * I componenti non si montano con `node --test`: la scelta del passo è una
 * funzione pura (`passoIndietroSfoglia`), la pila di `indietro.ts` si prova
 * com'è, e il cablaggio (chi si iscrive, chi la scende) si legge dai sorgenti.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import * as indietro from "../src/indietro";
import * as lontano from "../src/lontano";

const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

const APERTO = { online: true, album: false, artista: false, ricerca: false, altraScheda: false };

// ── R13-6: il passo, scelto da quel che si vede ─────────────────────────────

test("con il dispositivo che non risponde Indietro non chiude livelli che non si vedono (R13-6)", () => {
  const passo = lontano.passoIndietroSfoglia;
  assert.equal(typeof passo, "function", "manca passoIndietroSfoglia");
  for (const sulComputer of [false, true]) {
    assert.equal(
      passo({ ...APERTO, online: false, album: true, artista: true, ricerca: true, altraScheda: true, sulComputer }),
      null,
      "offline la pagina è lo stato vuoto: non c'è niente da chiudere, Indietro esce",
    );
  }
});

test("sul telefono Indietro scende un livello alla volta: disco, artista, ricerca, scheda", () => {
  const passo = lontano.passoIndietroSfoglia;
  const telefono = { ...APERTO, sulComputer: false };
  assert.equal(passo({ ...telefono, album: true, artista: true, ricerca: true, altraScheda: true }), "album");
  assert.equal(passo({ ...telefono, artista: true, ricerca: true, altraScheda: true }), "artista");
  assert.equal(passo({ ...telefono, ricerca: true, altraScheda: true }), "ricerca");
  assert.equal(passo({ ...telefono, altraScheda: true }), "scheda");
  assert.equal(passo(telefono), null, "su «In evidenza», senza niente aperto, Indietro esce");
});

test("sul computer Escape fa come nella libreria: chiude disco, artista, ricerca, e non cambia scheda", () => {
  const passo = lontano.passoIndietroSfoglia;
  const computer = { ...APERTO, sulComputer: true };
  assert.equal(passo({ ...computer, album: true, artista: true, ricerca: true, altraScheda: true }), "album");
  assert.equal(passo({ ...computer, artista: true, ricerca: true, altraScheda: true }), "artista");
  assert.equal(passo({ ...computer, ricerca: true, altraScheda: true }), "ricerca");
  assert.equal(
    passo({ ...computer, altraScheda: true }),
    null,
    "in libreria Escape non cambia la vista: qui non torna a «In evidenza»",
  );
});

// ── R13-5: la pila, anche per il computer ───────────────────────────────────

test("Escape sul computer scende i gestori di Sfoglia, e non quelli del solo telefono (R13-5)", () => {
  assert.equal(typeof indietro.indietroSulComputer, "function", "manca indietroSulComputer");
  const chiamati: string[] = [];
  // La coda o la terza colonna, che sul telefono coprono la pagina e sul
  // computer stanno accanto: Escape non le deve chiudere.
  const togliFoglio = indietro.suIndietro(() => {
    chiamati.push("foglio");
    return true;
  });
  assert.equal(indietro.indietroSulComputer(), false, "nessun gestore per il computer: la scala di App decide");
  assert.deepEqual(chiamati, []);

  const togliSfoglia = indietro.suIndietro(
    () => {
      chiamati.push("sfoglia");
      return true;
    },
    { sulComputer: true },
  );
  assert.equal(indietro.indietroSulComputer(), true);
  assert.deepEqual(chiamati, ["sfoglia"], "il gestore di Sfoglia, e non quello del foglio che gli sta sotto");

  // Il telefono continua a scendere tutta la pila, dal più recente.
  chiamati.length = 0;
  assert.equal(indietro.indietro(), true);
  assert.deepEqual(chiamati, ["sfoglia"]);

  togliSfoglia();
  assert.equal(indietro.indietroSulComputer(), false, "tolto il gestore, Escape non lo trova più");
  togliFoglio();
});

test("un gestore di Sfoglia che non chiude niente lascia passare il gradino dopo", () => {
  const togli = indietro.suIndietro(() => false, { sulComputer: true });
  assert.equal(indietro.indietroSulComputer(), false);
  togli();
});

// ── il cablaggio ────────────────────────────────────────────────────────────

test("Sfoglia si iscrive per il computer solo quando c'è qualcosa che si vede da chiudere", () => {
  const pagina = sorgente("schermate/Dispositivo.tsx");
  assert.match(pagina, /passoIndietroSfoglia\(/, "la pagina sceglie il passo dalla funzione pura");
  assert.match(pagina, /sulComputer:\s*!MOBILE/, "la funzione sa se è il computer");
  assert.match(pagina, /\{\s*sulComputer:\s*true\s*\}/, "il gestore è anche per Escape");
  assert.match(pagina, /online[^;]*,\s*album:\s*album !== null/s, "l'online entra nella scelta");
});

test("la scala di Escape di App scende la pila di Sfoglia sul computer, dopo il menù (R13-5)", () => {
  const app = sorgente("App.tsx");
  const inizio = app.indexOf("chiudi: () => {");
  assert.ok(inizio >= 0, "manca la scala `chiudi` di App");
  const scala = app.slice(inizio, app.indexOf("\n      },\n    },\n    scorciatoie,", inizio));
  assert.match(scala, /!MOBILE && indietroSulComputer\(\)/, "il computer scende i livelli di Sfoglia");
  assert.ok(
    scala.indexOf("setMenu(null)") < scala.indexOf("indietroSulComputer()"),
    "un menù ⋮ aperto si chiude prima del livello di sotto",
  );
  assert.ok(
    scala.indexOf("annullaSelezione()") < scala.indexOf("indietroSulComputer()"),
    "la selezione e la ricerca di App stanno sopra la pagina",
  );
});
