import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import * as apertura from "../src/apertura";
import type { FaseLibreria } from "../src/ipc";

/*
 * L'avvio fuori dal `setup` (R06b-5, R23-9), dal lato della finestra.
 *
 * La libreria si apre su un filo suo e la finestra compare subito: aspetta in
 * `AttesaDellaLibreria.tsx` e monta l'applicazione quando la libreria è aperta, o quando
 * si sa che non si aprirà. Qui le decisioni pure (`apertura.ts`) e il
 * cablaggio, che un DOM nelle prove non c'è per vedere: la finestra aspetta
 * davvero, ascolta prima di chiedere, e l'uscita audio che si sta aprendo non
 * diventa una fascia rossa.
 */

const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");

const PRONTA: FaseLibreria = { fase: "pronta" };
const GUASTO: FaseLibreria = { fase: "guasto" };
const APERTURA: FaseLibreria = { fase: "apertura" };
const AGGIORNAMENTO: FaseLibreria = { fase: "aggiornamento", da: 38 };

test("l'avvio è finito con la libreria aperta o col suo guasto, non prima", () => {
  assert.equal(typeof apertura.finita, "function");
  assert.equal(apertura.finita(null), false);
  assert.equal(apertura.finita(APERTURA), false);
  assert.equal(apertura.finita(AGGIORNAMENTO), false);
  assert.equal(apertura.finita(PRONTA), true);
  assert.equal(apertura.finita(GUASTO), true);
});

test("una fine non torna indietro, l'attesa segue l'ultima notizia", () => {
  // La domanda partita durante l'apertura torna dopo l'evento che l'ha chiusa.
  assert.deepEqual(apertura.prossima(PRONTA, APERTURA), PRONTA);
  assert.deepEqual(apertura.prossima(GUASTO, AGGIORNAMENTO), GUASTO);
  assert.deepEqual(apertura.prossima(null, AGGIORNAMENTO), AGGIORNAMENTO);
  assert.deepEqual(apertura.prossima(APERTURA, AGGIORNAMENTO), AGGIORNAMENTO);
  assert.deepEqual(apertura.prossima(AGGIORNAMENTO, PRONTA), PRONTA);
});

test("un nucleo che non è partito intero non monta l'applicazione, e non si aspetta", () => {
  const NON_PARTE: FaseLibreria = { fase: "nonParte" };
  assert.equal(typeof apertura.nonParte, "function");
  assert.equal(apertura.nonParte(NON_PARTE), true);
  for (const altra of [null, APERTURA, AGGIORNAMENTO, PRONTA, GUASTO]) {
    assert.equal(apertura.nonParte(altra), false);
  }
  // Non è una fine in cui montare: la pagina senza il lettore, lo scrobbling e il resto
  // risponderebbe `internal.aborted` a ogni comando.
  assert.equal(apertura.finita(NON_PARTE), false);
  // E non torna indietro per una domanda partita prima, né si perde per una dopo.
  assert.deepEqual(apertura.prossima(NON_PARTE, APERTURA), NON_PARTE);
  assert.deepEqual(apertura.prossima(NON_PARTE, PRONTA), NON_PARTE);
  assert.deepEqual(apertura.prossima(APERTURA, NON_PARTE), NON_PARTE);
  assert.deepEqual(apertura.prossima(null, NON_PARTE), NON_PARTE);
  // Il componente: i figli solo a fine avvio, e il «non parte» ha la sua frase invece.
  const componente = leggi("../src/AttesaDellaLibreria.tsx");
  const figli = componente.indexOf("if (finita(fase)) return <>{children}</>;");
  const rotta = componente.indexOf("if (rotta) {");
  assert.ok(figli > 0 && rotta > figli, "il nucleo rotto si controlla dopo i figli");
  const ramo = componente.slice(rotta, componente.indexOf("return (", rotta + 20));
  assert.ok(!ramo.includes("children"), "il ramo del nucleo rotto monta l'applicazione");
  assert.ok(componente.includes('t("startup.failed")') && componente.includes('t("startup.failed.detail")'));
  assert.ok(componente.includes("const aspetta = fase !== null && !finita(fase) && !rotta;"));
  // Il tipo che Rust scrive (`stato::FaseLibreria::NonParte`, `nonParte`).
  assert.ok(leggi("../src/ipc/libreria.ts").includes('| { fase: "nonParte" }'));
  assert.ok(leggi("../src-tauri/src/stato.rs").includes('serde_json::json!({ "fase": "nonParte" })'));
});

test("la copia dell'aggiornamento ha il nome di `db::nome_istantanea`", () => {
  assert.equal(apertura.copiaDi(38), "rymembra.db.schema-038.bak");
  assert.equal(apertura.copiaDi(7), "rymembra.db.schema-007.bak");
});

test("l'uscita audio che si apre non è un guasto, gli altri errori sì", () => {
  const errore = (code: string) => ({
    code,
    domain: "playback",
    severity: "info",
    retryable: true,
    i18nKey: `errors.${code}`,
    serviceText: null,
    values: {},
    cause: null,
  });
  assert.equal(apertura.apreAncora(errore("playback.engineOpening")), true);
  assert.equal(apertura.apreAncora(errore("playback.engineUnavailable")), false);
  assert.equal(apertura.apreAncora(new Error("x")), false);
});

test("l'applicazione si monta dentro l'attesa", () => {
  const main = leggi("../src/main.tsx");
  assert.match(main, /<AttesaDellaLibreria>\s*<App \/>\s*<\/AttesaDellaLibreria>/);
  const componente = leggi("../src/AttesaDellaLibreria.tsx");
  // Ascolto, poi domanda: la domanda sta nel `poi` di `useAscolto`.
  assert.match(componente, /useAscolto<FaseLibreria>\("libreria:stato", adotta, true, chiedi\)/);
  // I figli solo a fine avvio.
  assert.match(componente, /if \(finita\(fase\)\) return <>\{children\}<\/>;/);
  assert.ok(componente.includes("ipc.finestraMostra()"), "la finestra si mostra se c'è da aspettare");
});

test("il lettore che si apre dopo torna disponibile, e la sua attesa non è un guasto", () => {
  const riproduzione = leggi("../src/riproduzione.ts");
  const fotografia = riproduzione.slice(riproduzione.indexOf("const fotografia = () =>"));
  const prima = fotografia.slice(0, fotografia.indexOf("useAscolto<StatoRiproduzione>"));
  assert.ok(prima.includes("if (!apreAncora(e)) setErrore(e);"));
  const evento = fotografia.slice(fotografia.indexOf('"riproduzione:stato"'));
  const gestore = evento.slice(0, evento.indexOf("fotografia,"));
  assert.ok(gestore.includes("setDisponibile(true);"));
});

test("i comandi dell'attesa ci sono", () => {
  const ipc = leggi("../src/ipc.ts");
  const lib = leggi("../src-tauri/src/lib.rs");
  for (const comando of ["libreria_fase", "finestra_mostra"]) {
    assert.ok(ipc.includes(`("${comando}")`), `ipc.ts non chiama ${comando}`);
  }
  assert.ok(lib.includes("stato::libreria_fase,"));
  assert.ok(lib.includes("finestra_mostra,"));
});
