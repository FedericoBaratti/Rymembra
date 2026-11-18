/**
 * «Lasciali» si ricorda anche dopo un riavvio (FR2, punto 4).
 *
 * F3a lo teneva solo in memoria (`ConfermaSpecchio`): dopo un riavvio la domanda
 * «il PC non condivide più 1.340 brani» tornava alla stessa cifra, e a ogni
 * riavvio ancora. Adesso il nucleo ricorda, per dispositivo, il numero di righe
 * che la persona ha lasciato (`dispositivi::ricorda_lasciati`, una chiave
 * `settings` che sta fuori dal profilo). La domanda torna quando il numero cambia.
 *
 * Senza un DOM: la regola — chi si chiede, e quando non ancora — è una funzione
 * pura di `lontano.ts`; il resto si guarda nei sorgenti.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import type { AvanzamentoSpecchio, FaseSpecchio } from "../src/ipc";
import { daFotografia, domandaSullePerdite } from "../src/lontano";

const leggi = (percorso: string) =>
  readFileSync(new URL(percorso, import.meta.url), "utf8").replace(/\r\n/g, "\n");

const A = "aa".repeat(32);
const B = "bb".repeat(32);
const NOMI = new Map([
  [A, "PC di casa"],
  [B, "Portatile"],
]);

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

test("finché il nucleo non ha detto cosa si era lasciato, non si chiede niente", () => {
  // Altrimenti a ogni avvio la domanda lampeggerebbe per il tempo di una
  // risposta, anche per un «Lasciali» già dato: meglio tardi che due volte.
  const stato = daFotografia([avanzamento(A, "fine", { righe: 900, inAttesa: 120 })]);
  assert.equal(domandaSullePerdite(stato, NOMI, null), undefined);
});

test("la stessa cifra già lasciata non torna, una cifra nuova sì", () => {
  const stato = daFotografia([avanzamento(A, "fine", { righe: 900, inAttesa: 120 })]);
  assert.equal(domandaSullePerdite(stato, NOMI, new Map())?.dispositivo, A, "mai lasciata: si chiede");
  assert.equal(domandaSullePerdite(stato, NOMI, new Map([[A, 120]])), undefined, "lasciata a 120");
  assert.equal(
    domandaSullePerdite(stato, NOMI, new Map([[A, 90]]))?.inAttesa,
    120,
    "lasciata a 90, adesso sono 120: è un'altra situazione",
  );
});

test("ogni dispositivo ha il suo ricordo", () => {
  const stato = daFotografia([
    avanzamento(A, "fine", { righe: 900, inAttesa: 120 }),
    avanzamento(B, "fine", { righe: 400, inAttesa: 60 }),
  ]);
  assert.equal(domandaSullePerdite(stato, NOMI, new Map([[A, 120]]))?.dispositivo, B);
  assert.equal(
    domandaSullePerdite(stato, NOMI, new Map([[A, 120], [B, 60]])),
    undefined,
    "tutti e due lasciati",
  );
  // Il numero lasciato per B non vale per A.
  assert.equal(domandaSullePerdite(stato, NOMI, new Map([[B, 120]]))?.dispositivo, A);
});

test("non si chiede di un dispositivo che non c'è più, né di uno che non ha niente in attesa", () => {
  const stato = daFotografia([
    avanzamento(A, "fine", { righe: 900, inAttesa: 0 }),
    avanzamento("cc".repeat(32), "fine", { righe: 50, inAttesa: 70 }),
    avanzamento(B, "leggo", { inAttesa: 60 }),
  ]);
  assert.equal(domandaSullePerdite(stato, NOMI, new Map()), undefined);
});

test("il ricordo passa dal nucleo: si legge all'apertura, si scrive con «Lasciali», si toglie con «Togli»", () => {
  const conferma = leggi("../src/ConfermaSpecchio.tsx");
  const dom = conferma.slice(conferma.indexOf("export function DomandaSpecchio"));
  // Parte da «non lo so ancora», non da «non si è lasciato niente».
  assert.match(dom, /useState<ReadonlyMap<string, number> \| null>\(null\)/);
  assert.match(dom, /ipc\s*\.lontanoSpecchioLasciati\(\)/);
  assert.match(dom, /domandaSullePerdite\(/);
  // «Lasciali» lo scrive col numero, «Togli» lo cancella.
  assert.match(dom, /ipc\.lontanoSpecchioLascia\(domanda\.dispositivo, domanda\.inAttesa\)/);
  assert.match(dom, /ipc\.lontanoSpecchioLascia\(domanda\.dispositivo, null\)/);
  // E **solo** lì: un giro finito con zero righe in attesa non cancella il ricordo,
  // perché quello zero non è affidabile (un guasto nel giro lo dà lo stesso:
  // `togli_le_scadute` in `lontano/specchio.rs` parte da zero).
  assert.equal(
    (dom.match(/lontanoSpecchioLascia\(/g) ?? []).length,
    2,
    "un ricordo si scrive con «Lasciali» e si toglie con «Togli», e basta",
  );
  assert.doesNotMatch(dom, /ricordiDaDimenticare|inAttesa === 0/);

  const ipc = leggi("../src/ipc.ts");
  assert.match(ipc, /invoke<Record<string, number>>\("lontano_specchio_lasciati"\)/);
  assert.match(ipc, /invoke<void>\("lontano_specchio_lascia", \{ chiave, quante \}\)/);
});

test("i due comandi del nucleo sono registrati, e il ricordo sta fuori dal profilo", () => {
  const lib = leggi("../src-tauri/src/lib.rs");
  assert.match(lib, /lontano::lontano_specchio_lasciati,/);
  assert.match(lib, /lontano::lontano_specchio_lascia,/);
  // La chiave è sotto `lontano.`, che nessuna chiave del catalogo del profilo
  // può prendere (`niente_di_insieme_viaggia_nel_profilo`), e il manifesto lo dice.
  const dispositivi = leggi("../../../core/rymembra-app/src/dispositivi.rs");
  assert.match(dispositivi, /pub const CHIAVE_LASCIATI: &str = "lontano\.lasciati";/);
  assert.match(leggi("../../../core/rymembra-app/src/profilo/mod.rs"), /«Lasciali»/);
});
