import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { statoDellaCoda } from "../src/riepilogoCoda";

/*
 * La coda delle importazioni finita si dice finita (R13c-8).
 *
 * A coda finita la pagina mostrava «12 / 12», la barra ferma e grigia e «in
 * pausa · 0 rimasti», mentre il sottotitolo diceva «Nessuna coda in corso»:
 * due stati opposti per la stessa coda, e «in pausa» faceva cercare un
 * «Riprendi» che non c'è.
 */
test("ferma e senza rimasti è finita, non in pausa", () => {
  assert.equal(statoDellaCoda({ attiva: false, fatti: 12, inAttesa: 0 }), "finita");
});

test("ferma con dei rimasti è in pausa, e attiva è in corso", () => {
  assert.equal(statoDellaCoda({ attiva: false, fatti: 3, inAttesa: 9 }), "pausa");
  assert.equal(statoDellaCoda({ attiva: true, fatti: 3, inAttesa: 9 }), "corso");
  // Attiva all'ultimo brano: il nucleo non ha ancora detto che ha finito.
  assert.equal(statoDellaCoda({ attiva: true, fatti: 12, inAttesa: 0 }), "corso");
});

test("la pagina usa lo stato della coda per il testo e per la barra", () => {
  const sorgente = readFileSync(new URL("../src/schermate/Importazioni.tsx", import.meta.url), "utf8");
  assert.match(sorgente, /statoDellaCoda\(/);
  assert.match(sorgente, /t\("imports\.state\.done"\)/);
  assert.match(sorgente, /data-ferma=\{comeSta === "pausa" \|\| undefined\}/);
  for (const lingua of ["it", "en"]) {
    const voci = JSON.parse(
      readFileSync(new URL(`../src/lingue/${lingua}.json`, import.meta.url), "utf8"),
    ) as Record<string, string>;
    assert.ok(voci["imports.state.done"], lingua);
  }
});
