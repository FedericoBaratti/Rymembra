import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { notiziaDelRipristino } from "../src/esitoRipristino";

test("un ripristino finito a finestrella chiusa si racconta lo stesso", () => {
  assert.deepEqual(notiziaDelRipristino({ cartelle: 0, mancanti: [] }), [{ chiave: "restore.done.notice" }]);
  // Quel che il rapporto avrebbe mostrato e che chi ha chiuso deve sapere: le
  // cartelle tornate da scansionare e i file che non c'erano.
  assert.deepEqual(notiziaDelRipristino({ cartelle: 2, mancanti: ["a", "b", "c"] }), [
    { chiave: "restore.done.notice" },
    { chiave: "restore.done.notice.folders" },
    { chiave: "restore.done.notice.missing", n: 3 },
  ]);
});

/** Le due lingue, come le legge la finestra. */
function testi(lingua: "it" | "en"): Record<string, string> {
  return JSON.parse(
    readFileSync(new URL(`../src/lingue/${lingua}.json`, import.meta.url), "utf8"),
  ) as Record<string, string>;
}

test("prima della scansione il piano dice di scansionare o di ripetere il ripristino", () => {
  // Decisione 14 (R06-1): i brani senza file non ritrovano le statistiche da
  // soli dopo la scansione; serve un secondo ripristino, e la frase lo dice.
  const it = testi("it");
  const en = testi("en");
  for (const chiave of ["restore.absent", "restore.absent.uno"]) {
    assert.match(it[chiave] ?? "", /ripristina di nuovo/, chiave);
    assert.match(en[chiave] ?? "", /restore again/, chiave);
  }
  assert.match(it["restore.foldersBack"] ?? "", /ripeti il ripristino/);
  assert.match(en["restore.foldersBack"] ?? "", /restore again/);
});

test("«nessun backup» ricorda che il backup è legato all'app Google", () => {
  // R13c-13: con un'altra app Google il backup di prima non si vede, e il
  // consiglio di «lasciar passare un salvataggio» portava a crederlo perso.
  assert.match(testi("it")["restore.noBackup"] ?? "", /un'altra app Google/);
  assert.match(testi("en")["restore.noBackup"] ?? "", /another Google app/);
});
