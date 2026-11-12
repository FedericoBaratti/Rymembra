import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import {
  allaFine,
  conAvanzamento,
  conRitentativi,
  daRileggere,
  RITENTATIVI_AVVIO,
} from "../src/statoScarico";
import type { StatoScarico } from "../src/ipc";

/*
 * Lo stato della coda nelle importazioni (R13c-11): la lettura d'avvio si
 * ritenta, un avanzamento senza stato lo fa chiedere, la fine dice «ferma».
 */
const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");

const stato = (attiva: boolean): StatoScarico =>
  ({
    attiva,
    fatti: 0,
    rimasti: 3,
    conteggi: { attesa: 3, fatto: 0, fallito: 0, introvabile: 0 },
    sorgenti: [],
    cataloghi: [],
    alternative: true,
  }) as unknown as StatoScarico;

test("la lettura d'avvio si ritenta, e alla fine si arrende", async () => {
  let chiamate = 0;
  const letto = await conRitentativi(
    async () => {
      chiamate += 1;
      if (chiamate < 3) throw new Error("db.locked");
      return "ok";
    },
    RITENTATIVI_AVVIO,
    async () => {},
  );
  assert.equal(letto, "ok");
  assert.equal(chiamate, 3);

  chiamate = 0;
  await assert.rejects(
    conRitentativi(
      async () => {
        chiamate += 1;
        throw new Error("giù");
      },
      RITENTATIVI_AVVIO,
      async () => {},
    ),
  );
  assert.equal(chiamate, RITENTATIVI_AVVIO + 1);

  // Smontato: non si ritenta.
  chiamate = 0;
  await assert.rejects(
    conRitentativi(
      async () => {
        chiamate += 1;
        throw new Error("giù");
      },
      RITENTATIVI_AVVIO,
      async () => {},
      () => true,
    ),
  );
  assert.equal(chiamate, 1);
});

test("la fine della coda la dice ferma anche senza rilettura", () => {
  assert.equal(allaFine(stato(true))?.attiva, false);
  assert.equal(allaFine(null), null);
});

test("un avanzamento senza stato lo fa chiedere", () => {
  assert.equal(conAvanzamento(null, { fatti: 1, totale: 3, sorgenti: [] }), null);
  assert.equal(daRileggere(null), true);
  const dopo = conAvanzamento(stato(false), { fatti: 1, totale: 3, sorgenti: [] });
  assert.equal(dopo?.attiva, true);
  assert.equal(dopo?.rimasti, 2);
  assert.equal(daRileggere(dopo), false);
});

test("le importazioni usano le tre regole", () => {
  const sorgente = leggi("../src/parti/Importazioni.tsx");
  assert.match(sorgente, /conRitentativi\(/, "la lettura d'avvio non si ritenta");
  assert.match(sorgente, /allaFine\)/, "a `scarico:finito` la coda non si dice ferma");
  assert.match(sorgente, /daRileggere\(/, "un avanzamento senza stato si butta");
});
