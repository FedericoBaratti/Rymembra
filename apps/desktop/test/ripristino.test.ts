import { test } from "node:test";
import assert from "node:assert/strict";

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
