import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * Il testo di un errore non è un secondo canale di frasi (R10, R17).
 *
 * Il nucleo scriveva un messaggio italiano (`with_message`) che la finestra
 * mostrava sotto la frase tradotta: con l'interfaccia in inglese, una riga
 * italiana. Adesso il dettaglio di chi sviluppa resta nel diario, e alla
 * finestra arrivano la frase del codice, i dati che nomina (`values`) e, per i
 * servizi remoti, quel che il servizio ha scritto (`serviceText`).
 *
 * `errori.ts` si porta dietro il motore delle lingue (`import.meta.glob`), che
 * `node --test` non sa leggere: qui si prova il cablaggio sul sorgente, e il
 * comportamento delle funzioni pure sta in `altroConto.test.ts`.
 */

const sorgente = (percorso: string) =>
  readFileSync(new URL(percorso, import.meta.url), "utf8");

const errori = sorgente("../src/ipc/errori.ts");

/** Il corpo di una funzione esportata, fino alla graffa che la chiude in colonna 0. */
function corpoDi(nome: string): string {
  const inizio = errori.indexOf(`export function ${nome}`);
  assert.ok(inizio >= 0, nome);
  const fine = errori.indexOf("\n}\n", inizio);
  return errori.slice(inizio, fine + 3);
}

test("l'errore che arriva alla finestra non ha un campo `message` né `detail`", () => {
  const interfaccia = errori.slice(
    errori.indexOf("export interface ErroreIpc"),
    errori.indexOf("/** È un errore del nucleo"),
  );
  assert.doesNotMatch(interfaccia, /\bmessage\b\s*:/);
  assert.doesNotMatch(interfaccia, /\bdetail\b\s*:/);
  assert.match(interfaccia, /serviceText: string \| null/);
  assert.match(interfaccia, /values: Record<string, string>/);

  // E lo stesso dal lato di Rust: la struct che si serializza.
  const rust = sorgente("../src-tauri/src/errore.rs");
  const struttura = rust.slice(rust.indexOf("pub struct ErroreIpc"), rust.indexOf("impl From<AppError>"));
  assert.doesNotMatch(struttura, /pub message\b/);
  assert.doesNotMatch(struttura, /pub detail\b/);
  assert.match(struttura, /pub service_text: Option<String>/);
});

test("`dettaglioErrore` dà vuoto per un errore senza testo del servizio", () => {
  const corpo = corpoDi("dettaglioErrore");
  // Esce subito se `serviceText` non è una stringa: non c'è nessun altro campo
  // da cui pescare una riga.
  assert.match(corpo, /typeof value\.serviceText !== "string"\) return null/);
  assert.doesNotMatch(corpo, /\.message\b/);
  assert.doesNotMatch(corpo, /\.cause\b/);
});

test("la frase si riempie con i dati dell'errore", () => {
  const corpo = corpoDi("testoErrore");
  assert.match(corpo, /tSe\(value\.i18nKey, [^)]*valoriDi\(value\)\)/);
  // Un `Error` di JavaScript ha il suo `message`; un errore del nucleo non più.
  assert.doesNotMatch(corpo, /value\.message \?\?/);
});

test("nessuno legge più `message` da un errore del nucleo", () => {
  // I punti che lo leggevano in modo strutturale — il compilatore non li vede.
  for (const percorso of ["../src/altroConto.ts", "../src/diarioDellaFinestra.ts"]) {
    const testo = sorgente(percorso);
    assert.doesNotMatch(testo, /"message" in /, percorso);
    assert.doesNotMatch(testo, /motivo\.message/, percorso);
    assert.doesNotMatch(testo, /guasto\.message/, percorso);
  }
});
