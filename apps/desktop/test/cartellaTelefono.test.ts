/**
 * Una cartella scelta fuori dalla memoria del telefono non è un guasto.
 *
 * Nel selettore di Android si può toccare «Download» o un'app cloud che offre
 * cartelle: non hanno un percorso che la scansione possa camminare. `Documenti`
 * lo segnalava con un'eccezione, e Rust la trattava come ogni guasto del ponte:
 * «Qualcosa è andato storto… Riavvia Rymembra», con il motivo vero solo nel
 * diario, in italiano (R17-3). Adesso è un esito, e la finestra dice di
 * sceglierne una nella memoria del telefono o sulla scheda SD.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");

test("il selettore di cartelle non lancia per una cartella fuori dalla memoria", () => {
  const documenti = leggi(
    "../src-tauri/gen/android/app/src/main/java/io/github/federicobaratti/rymembra/Documenti.kt",
  );
  assert.doesNotMatch(documenti, /require\(albero\.authority == MEMORIA\)/);
  assert.match(documenti, /"fuori"/);
});

test("sul telefono la frase dice dove scegliere la cartella", () => {
  for (const [lingua, sd] of [
    ["it", /sched[ae] SD/],
    ["en", /SD cards?/],
  ] as const) {
    const testi = JSON.parse(leggi(`../src/lingue/${lingua}.json`)) as Record<string, string>;
    const frase = testi["errors.fs.pathInvalid.telefono"];
    assert.ok(frase, `${lingua}: manca errors.fs.pathInvalid.telefono`);
    assert.match(frase, sd, `${lingua}: non dice della scheda SD`);
    assert.doesNotMatch(frase, /riavvia|restart/i);
  }
});
