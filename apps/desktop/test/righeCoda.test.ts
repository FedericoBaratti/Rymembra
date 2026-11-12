import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import type { Brano } from "../src/ipc";
import {
  prossimoRiordino,
  sparizioniDaTogliere,
  statoRiga,
} from "../src/righeCoda";

const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");
const brano = (id: number) => ({ id, title: `T${id}` }) as unknown as Brano;

// ── R13b-5: in attesa non vuol dire sparita ────────────────────────────────

test("una riga non ancora arrivata è in attesa, non «non più in libreria»", () => {
  const righe = new Map<number, Brano | null>([
    [1, brano(1)],
    [2, null],
  ]);
  assert.equal(statoRiga(1, righe), "presente");
  assert.equal(statoRiga(2, righe), "sparita");
  assert.equal(statoRiga(3, righe), "attesa");
});

test("si tolgono solo le righe chieste e non trovate, dalla più in fondo", () => {
  // 2 è sparita (due volte in coda), 3 non è ancora arrivata, -4 è una voce di
  // «Ascolta insieme»: si toglie solo la 2, prima la copia più in fondo.
  const righe = new Map<number, Brano | null>([
    [1, brano(1)],
    [2, null],
    [-4, null],
  ]);
  assert.deepEqual(sparizioniDaTogliere([1, 2, 3, 2, -4], righe), [
    { indice: 3, id: 2 },
    { indice: 1, id: 2 },
  ]);
  assert.deepEqual(sparizioniDaTogliere([1, 3], righe), []);
});

test("Coda.tsx: niente «id 12345» sotto un titolo, e le righe in attesa sono scheletri", () => {
  const coda = sorgente("Coda.tsx");
  assert.doesNotMatch(coda, /`id \$\{id\}`/);
  assert.match(coda, /statoRiga\(/);
  assert.match(coda, /skeleton/);
  // Le sparite si tolgono da sole (decisione G), ma non dallo Studio e non
  // in una sessione dove la coda non è di questo PC.
  assert.match(coda, /sparizioniDaTogliere\(/);
  assert.match(coda, /anteprima \|\| !codaLibera/);
});

test("Coda.tsx: una lettura delle righe fallita si riprova", () => {
  const coda = sorgente("Coda.tsx");
  const gancio = coda.slice(coda.indexOf("export function useRigheCoda"), coda.indexOf("export function RigheCoda"));
  assert.match(gancio, /setTimeout\(/);
  assert.match(gancio, /RIPROVA_RIGHE_MS/);
});

// ── R13b-6: la posizione vera nella coda ───────────────────────────────────

test("Coda.tsx: ogni riga dice la sua posizione nella coda intera", () => {
  const coda = sorgente("Coda.tsx");
  const riga = coda.slice(coda.indexOf('className="riga-coda list-row queue-row"') - 400);
  assert.match(riga, /aria-posinset=\{indice \+ 1\}/);
  assert.match(riga, /aria-setsize=\{stato\.coda\.length\}/);
});

// ── R13b-4: Alt+↓ tenuto premuto porta sempre lo stesso brano ──────────────

test("col riordino in volo, il colpo dopo porta ancora il brano preso", () => {
  const coda = [10, 20, 30, 40];
  // Primo colpo: A (10) da 0 a 1; il fuoco è già sulla riga 1, che nella coda
  // in mano mostra ancora B (20).
  const primo = prossimoRiordino(coda, null, 0, "giu");
  assert.deepEqual(primo, { da: 0, a: 1, id: 10 });
  const secondo = prossimoRiordino(coda, { dove: 1, id: 10 }, 1, "giu");
  assert.deepEqual(secondo, { da: 1, a: 2, id: 10 });
  // Senza niente in volo vale la riga com'è.
  assert.deepEqual(prossimoRiordino(coda, null, 1, "giu"), { da: 1, a: 2, id: 20 });
  // Un riordino in volo su un'altra riga non c'entra.
  assert.deepEqual(prossimoRiordino(coda, { dove: 3, id: 10 }, 1, "su"), { da: 1, a: 0, id: 20 });
  // Ai bordi non si va da nessuna parte.
  assert.equal(prossimoRiordino(coda, null, 0, "su"), null);
  assert.equal(prossimoRiordino(coda, null, 3, "giu"), null);
});

test("Coda.tsx: il colpo da tastiera passa da prossimoRiordino, e l'attesa scade", () => {
  const coda = sorgente("Coda.tsx");
  const tastiera = coda.slice(coda.indexOf("const daTastiera"), coda.indexOf("if (stato.coda.length === 0)"));
  assert.match(tastiera, /prossimoRiordino\(/);
  assert.match(coda, /ATTESA_RIORDINO_MS/);
});

test("Coda.tsx: in una sessione le righe sparite non si tolgono da sole", () => {
  const coda = sorgente("Coda.tsx");
  assert.match(coda, /if \(anteprima \|\| !codaLibera \|\| inSessione\) return;/);
});

test("App: «Sposta in cima» nella coda porta subito dopo il brano in ascolto", () => {
  const app = sorgente("App.tsx");
  const menu = app.slice(app.indexOf("const menuDellaCoda"), app.indexOf("const menuDelBranoGrande"));
  assert.match(menu, /posizioneCoda \+ 1/);
  assert.match(menu, /inCima: indice - cima >= 2 \? vai\(cima\)/);
});
