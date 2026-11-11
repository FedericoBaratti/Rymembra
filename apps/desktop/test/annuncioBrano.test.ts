import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { branoDaDire, branoVisto, type BranoDaDire } from "../src/annuncioBrano";

/*
 * Il brano che parte, a chi ascolta lo schermo (R16-5, D-71): ogni passaggio,
 * anche quello automatico, tranne il brano ritrovato in pausa all'avvio.
 */

const brano = (id: number, inPausa = false): BranoDaDire => ({
  id,
  titolo: `Brano ${id}`,
  artista: "Qualcuno",
  inPausa,
});

/** La sequenza di stati vista dalla finestra, e quel che si sarebbe detto. */
function detti(sequenza: (BranoDaDire | null)[]): (number | null)[] {
  let prima: BranoDaDire | null | undefined;
  return sequenza.map((adesso) => {
    const daDire = branoDaDire(prima, adesso);
    prima = branoVisto(prima, adesso);
    return daDire?.id ?? null;
  });
}

test("«Brano successivo» e il passaggio da solo si dicono, la pausa no", () => {
  // Lo stato vuoto, il brano ritrovato in pausa, play, poi «successivo»
  // (Opus → Parte 2), poi la fine di Parte 2 che passa da sé al terzo.
  assert.deepEqual(
    detti([null, brano(1, true), brano(1), brano(2), brano(2, true), brano(2), brano(3)]),
    [null, null, null, 2, null, null, 3],
  );
});

test("il primo brano fatto partire da chi ascolta si dice", () => {
  assert.deepEqual(detti([null, null, brano(7)]), [null, null, 7]);
});

test("dopo una coda finita il brano nuovo si dice", () => {
  assert.deepEqual(detti([brano(1), null, brano(1)]), [1, null, 1]);
});

test("la regione del brano sta sotto #radice, sempre montata, e si dice gentile", () => {
  const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
  // Accanto alle due regioni delle notizie, fuori da `.finestra-app`.
  const dopo = app.slice(app.indexOf("<Annuncio testo={notizia} />"));
  assert.match(dopo.slice(0, dopo.indexOf("{Studio !== null")), /<AnnuncioDelBrano \/>/);
  const annuncio = readFileSync(new URL("../src/parti/Annuncio.tsx", import.meta.url), "utf8");
  const regione = annuncio.slice(annuncio.indexOf("export function AnnuncioDelBrano"));
  assert.match(regione, /branoDaDire\(/);
  assert.match(regione, /<Annuncio testo=\{detto\} \/>/);
  // `role="status"` è `aria-live="polite"`: aspetta una pausa.
  assert.match(annuncio, /className="solo-lettori" role="status" data-annuncio=""/);
});
