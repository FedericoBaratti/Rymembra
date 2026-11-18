/**
 * L'avanzamento del catalogo di un dispositivo (`lontano:specchio`): chi sta
 * leggendo, chi è di una versione di prima, chi ha righe da confermare, e
 * quando la libreria si rilegge.
 */
import { test } from "node:test";
import assert from "node:assert/strict";

import type { AvanzamentoSpecchio, FaseSpecchio } from "../src/ipc";
import {
  RILEGGI_DURANTE_LA_LETTURA_MS,
  conAvanzamento,
  daConfermare,
  daFotografia,
  inLettura,
  nonSupportati,
  rileggereDopo,
} from "../src/lontano";

const A = "aa".repeat(32);
const B = "bb".repeat(32);

function avanzamento(
  dispositivo: string,
  fase: FaseSpecchio,
  resto: Partial<AvanzamentoSpecchio> = {},
): AvanzamentoSpecchio {
  return {
    dispositivo,
    fase,
    fatti: 0,
    totali: 0,
    righe: 0,
    inAttesa: 0,
    ultimoMs: null,
    errore: null,
    ...resto,
  };
}

test("l'ultimo avanzamento di un dispositivo prende il posto di quello di prima", () => {
  const uno = conAvanzamento({}, avanzamento(A, "leggo", { fatti: 10, totali: 100 }));
  const due = conAvanzamento(uno, avanzamento(A, "leggo", { fatti: 50, totali: 100 }));
  const tre = conAvanzamento(due, avanzamento(B, "fine", { righe: 3 }));
  assert.equal(Object.keys(tre).length, 2);
  assert.equal(tre[A]?.fatti, 50);
  assert.equal(uno[A]?.fatti, 10, "quello di prima non si tocca: lo stato è nuovo a ogni passo");
});

test("chi arriva dopo parte dalla fotografia", () => {
  const stato = daFotografia([avanzamento(A, "leggo"), avanzamento(B, "nonSupportato")]);
  assert.deepEqual(inLettura(stato).map((a) => a.dispositivo), [A]);
  assert.deepEqual(nonSupportati(stato).map((a) => a.dispositivo), [B]);
});

test("le righe che un dispositivo non manda più aspettano un sì, a giro finito", () => {
  const stato = daFotografia([
    avanzamento(A, "fine", { righe: 900, inAttesa: 120 }),
    avanzamento(B, "fine", { righe: 40, inAttesa: 0 }),
  ]);
  assert.deepEqual(daConfermare(stato).map((a) => a.dispositivo), [A]);
  // In lettura il conto non è ancora quello vero: la domanda aspetta la fine.
  assert.deepEqual(
    daConfermare(daFotografia([avanzamento(A, "leggo", { inAttesa: 120 })])),
    [],
  );
});

test("la libreria si rilegge a giro finito se è cambiato qualcosa", () => {
  const leggendo = avanzamento(A, "leggo", { fatti: 500, totali: 1000 });
  const finito = avanzamento(A, "fine", { righe: 1000 });
  assert.equal(rileggereDopo(leggendo, finito, 0, 0), true, "c'era una lettura in corso");
  assert.equal(
    rileggereDopo(avanzamento(A, "fine", { righe: 1000 }), avanzamento(A, "fine", { righe: 1000 }), 0, 0),
    false,
    "un giro che non trova niente di nuovo non rifà gli elenchi"
  );
  assert.equal(
    rileggereDopo(avanzamento(A, "fine", { righe: 1000 }), avanzamento(A, "fine", { righe: 1010 }), 0, 0),
    true,
    "le righe sono cambiate"
  );
  assert.equal(rileggereDopo(undefined, finito, 0, 0), true, "la prima volta");
});

test("durante la lettura la libreria si rilegge ogni tanto, perché si usa mentre arriva", () => {
  const prima = avanzamento(A, "leggo", { fatti: 1000, totali: 50_000 });
  const dopo = avanzamento(A, "leggo", { fatti: 3000, totali: 50_000 });
  assert.equal(rileggereDopo(prima, dopo, RILEGGI_DURANTE_LA_LETTURA_MS, 0), true);
  assert.equal(
    rileggereDopo(prima, dopo, RILEGGI_DURANTE_LA_LETTURA_MS - 1, 0),
    false,
    "non a ogni secchio"
  );
  assert.equal(
    rileggereDopo(dopo, dopo, RILEGGI_DURANTE_LA_LETTURA_MS * 10, 0),
    false,
    "senza righe nuove non c'è niente da rileggere"
  );
});

test("un errore o l'attesa non rifanno gli elenchi", () => {
  assert.equal(rileggereDopo(undefined, avanzamento(A, "errore", { errore: "net.timeout" }), 9e9, 0), false);
  assert.equal(rileggereDopo(undefined, avanzamento(A, "nonSupportato"), 9e9, 0), false);
  assert.equal(rileggereDopo(undefined, avanzamento(A, "inattivo"), 9e9, 0), false);
});
