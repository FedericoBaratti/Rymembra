import { test } from "node:test";
import assert from "node:assert/strict";

import { chiaveDelGuasto, notaDi } from "../src/notaBrano";

const brano = (esito: string, codice: string | null = null, frazione: number | null = null) => ({
  esito,
  codice,
  frazione,
});

test("un brano che non scende non mostra il codice nudo", () => {
  // «net.timeout» è un codice, non una frase: la nota dice «non riuscito», e
  // il perché lungo va nel suggerimento.
  assert.deepEqual(notaDi(brano("fallito", "net.timeout")), { chiave: "imports.track.failed" });
  assert.equal(chiaveDelGuasto(brano("fallito", "net.timeout")), "errors.net.timeout");
  assert.deepEqual(notaDi(brano("fallito")), { chiave: "imports.track.failed" });
  assert.equal(chiaveDelGuasto(brano("fallito")), null);
});

test("le altre note restano quelle di prima", () => {
  assert.deepEqual(notaDi(brano("cerco")), { chiave: "imports.track.searching" });
  assert.deepEqual(notaDi(brano("prendo")), { chiave: "imports.track.fetching" });
  assert.deepEqual(notaDi(brano("prendo", null, 0.426)), { percento: 43 });
  assert.deepEqual(notaDi(brano("fatto")), { chiave: "imports.track.done" });
  assert.deepEqual(notaDi(brano("introvabile", "download.notPermitted")), { chiave: "imports.track.notAllowed" });
  assert.deepEqual(notaDi(brano("introvabile")), { chiave: "imports.track.nowhere" });
  assert.equal(chiaveDelGuasto(brano("introvabile", "download.notPermitted")), null);
});
