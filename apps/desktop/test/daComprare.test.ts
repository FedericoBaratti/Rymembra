import { test } from "node:test";
import assert from "node:assert/strict";

import { daRichiedere } from "../src/elencoDaComprare";

test("«Da comprare» aperta si rilegge quando il conto cresce", () => {
  // Letta con 3 brani, il titolo ora dice 23: l'elenco va richiesto.
  assert.equal(daRichiedere({ aperta: true, letta: true, quanti: 23, quantiLetti: 3 }), true);
  // Stesso conto: niente da rileggere.
  assert.equal(daRichiedere({ aperta: true, letta: true, quanti: 3, quantiLetti: 3 }), false);
});

test("chiusa non si chiede, e alla prima apertura sì", () => {
  assert.equal(daRichiedere({ aperta: false, letta: false, quanti: 3, quantiLetti: null }), false);
  assert.equal(daRichiedere({ aperta: false, letta: true, quanti: 23, quantiLetti: 3 }), false);
  assert.equal(daRichiedere({ aperta: true, letta: false, quanti: 3, quantiLetti: null }), true);
});
