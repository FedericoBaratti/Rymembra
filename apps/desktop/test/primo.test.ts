import { test } from "node:test";
import assert from "node:assert/strict";

import { tastoDelPrimo } from "../src/tastoPrimo";

const MUSICA = "C:\\Users\\X\\Music";
const NUVOLA = "nuvola://onedrive/conto/Musica";

test("una cartella del cloud aggiunta non toglie «Continua»", () => {
  // Music spuntata e mai scritta, la cartella del cloud scritta dal nucleo e
  // già letta: i suoi brani arrivano, ma «Continua» deve restare.
  assert.equal(
    tastoDelPrimo({ brani: 120, spuntate: [MUSICA, NUVOLA], scritte: [NUVOLA], confermato: false }),
    "continua",
  );
  // Dopo «Continua» riuscito, i brani bastano.
  assert.equal(
    tastoDelPrimo({ brani: 120, spuntate: [MUSICA, NUVOLA], scritte: [MUSICA, NUVOLA], confermato: true }),
    "ascolta",
  );
});

test("con tutte le spuntate già scritte i brani bastano a dire «Ascolta»", () => {
  assert.equal(tastoDelPrimo({ brani: 40, spuntate: [NUVOLA], scritte: [NUVOLA], confermato: false }), "ascolta");
  assert.equal(tastoDelPrimo({ brani: 40, spuntate: [], scritte: [NUVOLA], confermato: false }), "ascolta");
});

test("senza brani resta «Continua», confermato o no", () => {
  assert.equal(tastoDelPrimo({ brani: 0, spuntate: [MUSICA], scritte: [], confermato: false }), "continua");
  assert.equal(tastoDelPrimo({ brani: 0, spuntate: [MUSICA], scritte: [MUSICA], confermato: true }), "continua");
});
