import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { rigaDelRifiuto } from "../src/diarioDellaFinestra";

/*
 * La riga del diario per una promessa rifiutata senza `catch` (R13d-5).
 *
 * `invoke` rifiuta con l'oggetto serializzato del nucleo, e `String(...)` ne
 * faceva «[object Object]»: né il codice, né il messaggio, né dove. Proprio i
 * guasti inattesi che il gestore esiste per registrare.
 */

test("un errore del nucleo dice il suo codice, il messaggio e la causa", () => {
  const riga = rigaDelRifiuto({
    code: "net.timeout",
    domain: "net",
    severity: "warning",
    retryable: true,
    i18nKey: "errors.net.timeout",
    message: "lrclib non ha risposto",
    cause: "operation timed out",
  });
  assert.ok(!riga.includes("[object Object]"), riga);
  assert.ok(riga.startsWith("net.timeout"), riga);
  assert.ok(riga.includes("lrclib non ha risposto"), riga);
  assert.ok(riga.includes("operation timed out"), riga);
});

test("senza messaggio né causa resta il codice", () => {
  const riga = rigaDelRifiuto({
    code: "fs.noRecycleBin",
    domain: "fs",
    severity: "error",
    retryable: false,
    i18nKey: "errors.fs.noRecycleBin",
    message: null,
    cause: null,
  });
  assert.equal(riga, "fs.noRecycleBin");
});

test("un Error dice tipo, messaggio e dove", () => {
  const errore = new TypeError("Cannot read properties of undefined (reading 'length')");
  const riga = rigaDelRifiuto(errore);
  assert.ok(riga.startsWith("TypeError: Cannot read properties"), riga);
  assert.ok(riga.includes(" ← ") || riga.includes("at "), `senza il punto: ${riga}`);
});

test("qualunque altra cosa si legge senza cadere", () => {
  assert.equal(rigaDelRifiuto("rifiutata a mano"), "rifiutata a mano");
  assert.equal(rigaDelRifiuto(undefined), "undefined");
  assert.equal(rigaDelRifiuto({ dove: "qui" }), '{"dove":"qui"}');
  // Un oggetto senza prototipo fa cadere `String(...)`.
  const nudo = Object.create(null) as Record<string, unknown>;
  nudo["x"] = 1;
  assert.equal(typeof rigaDelRifiuto(nudo), "string");
  const circolare: Record<string, unknown> = {};
  circolare["io"] = circolare;
  assert.equal(typeof rigaDelRifiuto(circolare), "string");
});

test("il gestore delle promesse rifiutate usa la riga, non String(...)", () => {
  const main = readFileSync(new URL("../src/main.tsx", import.meta.url), "utf8");
  assert.ok(!main.includes("String(evento.reason)"), "il diario scrive ancora [object Object]");
  assert.match(main, /ipc\.diarioAnnota\("promessa", rigaDelRifiuto\(evento\.reason\)\)/);
});
