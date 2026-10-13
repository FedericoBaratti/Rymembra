import { test } from "node:test";
import assert from "node:assert/strict";

import type { MessaggioIa, Problema } from "../src/ipc";
import {
  NON_APPLICATA,
  documentoLeggero,
  pertinenti,
  rigaSelezione,
  stimaGettoni,
  storiaLeggera,
  type Sguardo,
} from "../src/studio/prompt";

const niente: Sguardo = { selezione: null, stato: null, tema: "base" };

function problema(path: string): Problema {
  return { code: "x", path, message: "", forse: [], riga: null, colonna: null };
}

/** Un documento abbastanza grosso da superare la soglia che si passa. */
const DOCUMENTO = {
  format: 2,
  id: "prova",
  parts: {
    card: { radius: "12px", motion: { enter: "sale", hover: "pulsa" } },
    "card-cover": { radius: "8px" },
    heart: { textColor: "#f00" },
  },
  variant: { parts: { card: { opacity: 0.9 }, heart: { opacity: 1 } } },
  motion: {
    animations: {
      sale: { duration: "200ms", frames: [] },
      pulsa: { duration: "1s", frames: [] },
      gira: { duration: "4s", frames: [] },
    },
  },
  layout: { density: "comfortable", shell: { type: "row", children: [] }, prefabs: {} },
};

test("la selezione diventa una riga, col suo stato e il suo tema", () => {
  assert.equal(
    rigaSelezione({ selezione: { tipo: "parte", nome: "card" }, stato: "hover", tema: "variante" }),
    '[Selected in the Studio: part "card", state "hover", in the variant theme]',
  );
  assert.equal(
    rigaSelezione({ selezione: { tipo: "curva", nome: "molla" }, stato: null, tema: "variante" }),
    '[Selected in the Studio: easing "molla"]',
  );
  assert.equal(
    rigaSelezione({ selezione: { tipo: "nodo", via: [1, 0] }, stato: null, tema: "base" }),
    '[Selected in the Studio: shell node at ["layout","shell","children","1","children","0"]]',
  );
  assert.equal(rigaSelezione({ selezione: { tipo: "documento" }, stato: null, tema: "base" }), null);
  assert.equal(rigaSelezione(niente), null);
});

test("pertinenti: la selezione, i nomi interi della domanda, le animazioni che usano", () => {
  const quali = pertinenti(
    DOCUMENTO,
    { selezione: { tipo: "parte", nome: "heart" }, stato: null, tema: "base" },
    "arrotonda di più la card",
    [],
  );
  assert.deepEqual([...quali.parti].sort(), ["card", "heart"]);
  // `card` chiama `sale` e `pulsa`; `gira` non la nomina nessuno.
  assert.deepEqual([...quali.animazioni].sort(), ["pulsa", "sale"]);
  assert.equal(quali.scafale, false);
});

test("pertinenti: un nome dentro un altro nome non conta", () => {
  const quali = pertinenti(DOCUMENTO, niente, "la card-cover è troppo tonda", []);
  assert.deepEqual([...quali.parti], ["card-cover"]);
});

test("pertinenti: gli errori nominano, e lo scafale arriva quando se ne parla", () => {
  const quali = pertinenti(DOCUMENTO, niente, "sistema", [problema("motion.animations.gira.frames")]);
  assert.deepEqual([...quali.animazioni], ["gira"]);
  assert.equal(pertinenti(DOCUMENTO, niente, "sposta la colonna a sinistra", []).scafale, true);
  assert.equal(pertinenti(DOCUMENTO, niente, "ok", [problema("layout.shell.children.0")]).scafale, true);
  assert.equal(
    pertinenti(DOCUMENTO, { selezione: { tipo: "nodo", via: [] }, stato: null, tema: "base" }, "", [])
      .scafale,
    true,
  );
});

test("documento leggero: sotto la soglia passa intero, compatto", () => {
  const testo = JSON.stringify(DOCUMENTO, null, 2);
  const leggero = documentoLeggero(testo, { parti: new Set(), animazioni: new Set(), scafale: false });
  assert.equal(leggero.testo, JSON.stringify(DOCUMENTO));
  assert.deepEqual(leggero.tolti, []);
});

test("documento leggero: oltre la soglia tiene il pertinente e dice cosa ha tolto", () => {
  const leggero = documentoLeggero(
    JSON.stringify(DOCUMENTO),
    { parti: new Set(["card"]), animazioni: new Set(["sale"]), scafale: false },
    10,
  );
  const letto = JSON.parse(leggero.testo);
  assert.deepEqual(Object.keys(letto.parts), ["card"]);
  assert.deepEqual(Object.keys(letto.variant.parts), ["card"]);
  assert.deepEqual(Object.keys(letto.motion.animations), ["sale"]);
  assert.deepEqual(letto.layout, { density: "comfortable" });
  assert.equal(letto.id, "prova");
  assert.deepEqual(leggero.tolti, [
    "parts: card-cover heart",
    "variant.parts: heart",
    "motion.animations: pulsa gira",
    "layout: shell and prefabs (the window's arrangement)",
  ]);
});

test("documento leggero: lo scafale resta quando serve, e un testo rotto passa com'è", () => {
  const conScafale = documentoLeggero(
    JSON.stringify(DOCUMENTO),
    { parti: new Set(), animazioni: new Set(), scafale: true },
    10,
  );
  assert.ok(JSON.parse(conScafale.testo).layout.shell);
  const rotto = '{"id": "prova", ';
  assert.deepEqual(
    documentoLeggero(rotto, { parti: new Set(), animazioni: new Set(), scafale: false }, 1),
    { testo: rotto, tolti: [] },
  );
});

test("storia leggera: l'immagine va solo con l'ultimo messaggio", () => {
  const storia: MessaggioIa[] = [
    { ruolo: "user", testo: "prima", immagini: ["data:image/jpeg;base64,AAAA"] },
    { ruolo: "assistant", testo: "fatto" },
    { ruolo: "user", testo: "adesso", immagini: ["data:image/jpeg;base64,BBBB"] },
  ];
  const leggera = storiaLeggera(storia);
  assert.equal(leggera[0]?.immagini, undefined);
  assert.deepEqual(leggera[2]?.immagini, ["data:image/jpeg;base64,BBBB"]);
});

test("storia leggera: oltre la soglia i blocchi vecchi diventano un segno, le ultime battute restano", () => {
  const blocco = "Ecco.\n```aether-patch\n" + "x".repeat(400) + "\n```";
  const storia: MessaggioIa[] = [
    { ruolo: "user", testo: "u".repeat(900) },
    { ruolo: "assistant", testo: blocco },
    { ruolo: "user", testo: "due" },
    { ruolo: "assistant", testo: blocco },
    { ruolo: "user", testo: "tre" },
    { ruolo: "assistant", testo: blocco },
  ];
  const leggera = storiaLeggera(storia, 100);
  assert.equal(leggera.length, storia.length);
  assert.equal(leggera[0]?.testo, `${"u".repeat(500)}…`);
  assert.equal(leggera[1]?.testo, "Ecco.\n[an aether-patch block, already applied]");
  // Le ultime quattro non si toccano.
  assert.deepEqual(leggera.slice(2), storia.slice(2));
  // Sotto la soglia, niente cambia.
  assert.deepEqual(storiaLeggera(storia, 1_000_000), storia);
});

test("la stima dei gettoni: quattro caratteri l'uno, mille per immagine", () => {
  assert.equal(
    stimaGettoni([
      { ruolo: "system", testo: "x".repeat(400) },
      { ruolo: "user", testo: "y".repeat(40), immagini: [""] },
    ]),
    1110,
  );
});

test("storia leggera: una proposta scartata non si racconta come applicata", () => {
  const blocco = "Ecco.\n```aether-patch\n" + "x".repeat(400) + "\n```";
  const storia = [
    { ruolo: "user" as const, testo: "u".repeat(900) },
    { ruolo: "assistant" as const, testo: blocco, applicata: false },
    { ruolo: "user" as const, testo: "due" },
    { ruolo: "assistant" as const, testo: blocco, applicata: true },
    { ruolo: "user" as const, testo: "tre" },
    { ruolo: "assistant" as const, testo: blocco, applicata: false },
  ];
  const leggera = storiaLeggera(storia, 100);
  assert.equal(leggera[1]?.testo, "Ecco.\n[an aether-patch block, NOT applied: the user discarded it]");
  // Anche intera, fra le ultime: il blocco resta, e una riga dice com'è finita.
  assert.equal(leggera[5]?.testo, `${blocco}\n${NON_APPLICATA}`);
  assert.equal(leggera[3]?.testo, blocco);
  // Il segno non parte verso il servizio.
  assert.ok(leggera.every((m) => !("applicata" in m)));
});
