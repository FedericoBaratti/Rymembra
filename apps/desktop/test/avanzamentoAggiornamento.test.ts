import { test } from "node:test";
import assert from "node:assert/strict";

import type { AvanzamentoAggiornamento } from "../src/ipc";
import * as modulo from "../src/parti/avanzamentoAggiornamento";

/*
 * La barra dello scaricamento dell'aggiornamento: dopo «Annulla» e un nuovo
 * «Aggiorna» ripartiva dai megabyte del tentativo di prima, e restava lì
 * finché non arrivava il primo pezzo nuovo — o per un minuto, a rete muta.
 * Adesso il riduttore calcola anche la quota, la velocità e il tempo rimasto.
 */

const MB = 1024 * 1024;
const pezzo = (scaricati: number, totale: number | null): AvanzamentoAggiornamento => ({
  scaricati,
  totale,
});

/** Una sequenza di pezzi, uno ogni `passoMs`. */
function scorri(
  pezzi: Array<[number, number | null]>,
  passoMs = 200,
  daMs = 1_000,
) {
  const { avanzamentoDopo } = modulo;
  let a: ReturnType<typeof avanzamentoDopo> = null;
  pezzi.forEach(([scaricati, totale], i) => {
    a = avanzamentoDopo(a, {
      tipo: "avanzamento",
      valore: pezzo(scaricati, totale),
      adesso: daMs + i * passoMs,
    });
  });
  return a;
}

/** Cinque pezzi da 1 MB ogni 200 ms: 5 MB/s, su un totale di 70. */
const CINQUE: Array<[number, number | null]> = [
  [1 * MB, 70 * MB],
  [2 * MB, 70 * MB],
  [3 * MB, 70 * MB],
  [4 * MB, 70 * MB],
  [5 * MB, 70 * MB],
];

test("dopo annulla e un nuovo aggiorna la barra riparte da zero", () => {
  const { avanzamentoDopo } = modulo;
  assert.equal(typeof avanzamentoDopo, "function");
  const quarantacinque = avanzamentoDopo(null, {
    tipo: "avanzamento",
    valore: pezzo(45 * MB, 70 * MB),
    adesso: 1_000,
  });
  assert.equal(quarantacinque?.scaricati, 45 * MB);
  assert.equal(avanzamentoDopo(quarantacinque, { tipo: "annulla" }), null);
  assert.equal(avanzamentoDopo(quarantacinque, { tipo: "installa" }), null);
});

test("la quota è la percentuale dei byte scesi, e non supera mai cento", () => {
  assert.equal(scorri([[35 * MB, 70 * MB]])?.quota, 50);
  assert.equal(
    scorri([[71 * MB, 70 * MB]])?.quota,
    100,
    "un pezzo in più del totale non fa una barra più lunga della pista",
  );
});

test("senza il totale la quota non c'è: una barra che non sa dove arriva mente", () => {
  const a = scorri([
    [1 * MB, null],
    [2 * MB, null],
    [3 * MB, null],
    [4 * MB, null],
    [5 * MB, null],
  ]);
  assert.equal(a?.quota, null);
  assert.equal(a?.restanti, null);
  // La velocità si sa lo stesso: dipende dal tempo, non dal totale.
  assert.ok((a?.velocita ?? 0) > 0);
});

test("il primo pezzo non ha una velocità: un solo punto non fa una retta", () => {
  const a = scorri([[1 * MB, 70 * MB]]);
  assert.equal(a?.velocita, null);
  assert.equal(a?.restanti, null);
});

test("la velocità è byte al secondo fra due pezzi", () => {
  // 1 MB in 200 ms sono 5 MB/s.
  const a = scorri([
    [1 * MB, 70 * MB],
    [2 * MB, 70 * MB],
  ]);
  assert.equal(Math.round((a?.velocita ?? 0) / MB), 5);
});

test("la velocità è una media che segue, e non salta a ogni pezzo", () => {
  const { avanzamentoDopo } = modulo;
  // Cinque pezzi a 5 MB/s, poi uno lentissimo.
  let a = scorri(CINQUE);
  const prima = a?.velocita ?? 0;
  a = avanzamentoDopo(a, {
    tipo: "avanzamento",
    valore: pezzo(5 * MB + 1024, 70 * MB),
    adesso: 1_000 + 5 * 200,
  });
  const dopo = a?.velocita ?? 0;
  assert.ok(dopo < prima, "un pezzo lento deve abbassare la media");
  assert.ok(dopo > prima * 0.5, "ma una raffica sola non la deve azzerare");
});

test("il tempo rimasto non si dice finché la stima non regge", () => {
  // Tre pezzi — due intervalli — non bastano: il numero oscillerebbe.
  const poco = scorri(CINQUE.slice(0, 3));
  assert.equal(poco?.restanti, null);
  const molti = scorri(CINQUE);
  // Restano 65 MB a 5 MB/s: tredici secondi.
  assert.equal(Math.round(molti?.restanti ?? -1), 13);
});

test("a rete ferma il tempo rimasto non diventa infinito né negativo", () => {
  const { avanzamentoDopo } = modulo;
  let a = scorri(CINQUE);
  // Una lunga pausa con zero byte in più: la media scende, ma non a infinito.
  for (let i = 0; i < 80; i++) {
    a = avanzamentoDopo(a, {
      tipo: "avanzamento",
      valore: pezzo(5 * MB, 70 * MB),
      adesso: 3_000 + i * 1_000,
    });
  }
  assert.ok(a !== null);
  assert.ok(a.restanti === null || Number.isFinite(a.restanti), "un tempo rimasto infinito");
  assert.ok(a.velocita === null || a.velocita >= 0);
});

test("un orologio che non avanza non divide per zero", () => {
  const { avanzamentoDopo } = modulo;
  let a = avanzamentoDopo(null, {
    tipo: "avanzamento",
    valore: pezzo(1 * MB, 70 * MB),
    adesso: 1_000,
  });
  a = avanzamentoDopo(a, {
    tipo: "avanzamento",
    valore: pezzo(2 * MB, 70 * MB),
    adesso: 1_000,
  });
  assert.ok(a !== null);
  assert.ok(a.velocita === null || Number.isFinite(a.velocita));
  assert.equal(a.scaricati, 2 * MB, "i byte si segnano comunque");
});

test("byte che tornano indietro sono un tentativo nuovo: la velocità riparte", () => {
  const { avanzamentoDopo } = modulo;
  let a = scorri([
    [30 * MB, 70 * MB],
    [31 * MB, 70 * MB],
    [32 * MB, 70 * MB],
  ]);
  a = avanzamentoDopo(a, {
    tipo: "avanzamento",
    valore: pezzo(1 * MB, 70 * MB),
    adesso: 5_000,
  });
  assert.equal(a?.velocita, null);
  assert.equal(a?.restanti, null);
  assert.equal(a?.scaricati, 1 * MB);
});

// ── i passi ───────────────────────────────────────────────────────────────────

test("sul computer i passi sono quattro, e quel che è già fatto si segna fatto", () => {
  const { passiDi } = modulo;
  const scarico = passiDi("scarico", "computer");
  assert.deepEqual(
    scarico.map((p) => p.chiave),
    ["scarico", "verifico", "installo", "riavvio"],
  );
  assert.deepEqual(
    scarico.map((p) => p.stato),
    ["corrente", "attesa", "attesa", "attesa"],
  );
  assert.deepEqual(
    passiDi("installo", "computer").map((p) => p.stato),
    ["fatto", "fatto", "corrente", "attesa"],
  );
  assert.deepEqual(
    passiDi("riavvio", "computer").map((p) => p.stato),
    ["fatto", "fatto", "fatto", "corrente"],
  );
});

test("sul telefono i passi sono tre e l'ultimo è «Installa»", () => {
  const { passiDi } = modulo;
  assert.deepEqual(
    passiDi("verifico", "telefono").map((p) => p.chiave),
    ["scarico", "verifico", "installa"],
  );
  assert.deepEqual(
    passiDi("verifico", "telefono").map((p) => p.stato),
    ["fatto", "corrente", "attesa"],
  );
  // «Pronto» sul telefono è il passo di «Installa»: i primi due sono fatti.
  assert.deepEqual(
    passiDi("pronto", "telefono").map((p) => p.stato),
    ["fatto", "fatto", "corrente"],
  );
});

test("senza una fase non c'è nessun passo da disegnare", () => {
  const { passiDi } = modulo;
  assert.deepEqual(passiDi(null, "computer"), []);
  assert.deepEqual(passiDi(null, "telefono"), []);
});

test("sul computer «pronto» non è un passo: il pacchetto aspetta, e i passi non si mostrano", () => {
  const { passiDi } = modulo;
  assert.deepEqual(passiDi("pronto", "computer"), []);
});

// ── l'attesa del pacchetto ────────────────────────────────────────────────────

test("dopo tre giorni la scheda ripropone «Installa ora»", () => {
  const { attesaLunga, SOLLECITO_MS } = modulo;
  const giorno = 24 * 3600 * 1000;
  assert.equal(SOLLECITO_MS, 3 * giorno);
  const dal = 1_700_000_000_000;
  assert.equal(attesaLunga(dal, dal + 3 * giorno - 1), false);
  assert.equal(attesaLunga(dal, dal + 3 * giorno), true);
  assert.equal(attesaLunga(dal, dal + 10 * giorno), true);
  // Senza un momento, o con un orologio spostato indietro, non si sollecita.
  assert.equal(attesaLunga(null, dal), false);
  assert.equal(attesaLunga(dal, dal - giorno), false);
});

// ── il tempo che manca, a parole ─────────────────────────────────────────────

test("il tempo rimasto si dice in secondi sotto il minuto e in minuti sopra", () => {
  const { quantoManca } = modulo;
  assert.deepEqual(quantoManca(0), { unita: "s", n: 1 });
  assert.deepEqual(quantoManca(0.2), { unita: "s", n: 1 });
  assert.deepEqual(quantoManca(13.2), { unita: "s", n: 14 });
  assert.deepEqual(quantoManca(59), { unita: "s", n: 59 });
  assert.deepEqual(quantoManca(61), { unita: "min", n: 2 });
  assert.deepEqual(quantoManca(600), { unita: "min", n: 10 });
});
