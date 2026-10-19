import { test } from "node:test";
import assert from "node:assert/strict";

import { LINGUAGGIO_PATCH_PRECEDENTE } from "../src/eredita";
import type { MessaggioIa, Problema } from "../src/ipc";
import {
  BLOCCO,
  LINGUAGGIO_PATCH,
  NON_APPLICATA,
  conTagNuovo,
  correzione,
  documentoLeggero,
  haBloccoDiModifiche,
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
  const blocco = "Ecco.\n```rymembra-patch\n" + "x".repeat(400) + "\n```";
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
  assert.equal(leggera[1]?.testo, "Ecco.\n[a rymembra-patch block, already applied]");
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
  const blocco = "Ecco.\n```rymembra-patch\n" + "x".repeat(400) + "\n```";
  const storia = [
    { ruolo: "user" as const, testo: "u".repeat(900) },
    { ruolo: "assistant" as const, testo: blocco, applicata: false },
    { ruolo: "user" as const, testo: "due" },
    { ruolo: "assistant" as const, testo: blocco, applicata: true },
    { ruolo: "user" as const, testo: "tre" },
    { ruolo: "assistant" as const, testo: blocco, applicata: false },
  ];
  const leggera = storiaLeggera(storia, 100);
  assert.equal(leggera[1]?.testo, "Ecco.\n[a rymembra-patch block, NOT applied: the user discarded it]");
  // Anche intera, fra le ultime: il blocco resta, e una riga dice com'è finita.
  assert.equal(leggera[5]?.testo, `${blocco}\n${NON_APPLICATA}`);
  assert.equal(leggera[3]?.testo, blocco);
  // Il segno non parte verso il servizio.
  assert.ok(leggera.every((m) => !("applicata" in m)));
});

// ── il tag di prima ─────────────────────────────────────────────────────────
//
// Le chat salvate sul disco, e la cronologia rimandata al modello, contengono
// ancora il linguaggio dei blocchi di prima: un blocco di allora, applicato o
// scartato, si riconosce come uno di adesso, e al modello si parla solo con
// quello di adesso.

/** Un blocco di modifiche col tag che si passa, lungo abbastanza da pesare. */
function bloccoCon(tag: string): string {
  return "Ecco.\n```" + tag + "\n" + "x".repeat(400) + "\n```";
}

test("haBloccoDiModifiche: il tag di adesso e quello di prima, e nient'altro", () => {
  assert.equal(haBloccoDiModifiche(bloccoCon(LINGUAGGIO_PATCH)), true);
  assert.equal(haBloccoDiModifiche(bloccoCon(LINGUAGGIO_PATCH_PRECEDENTE)), true);
  // Ancora aperto, come arriva mentre il modello scrive.
  assert.equal(haBloccoDiModifiche("Ecco.\n```" + LINGUAGGIO_PATCH_PRECEDENTE + "\n[{"), true);
  assert.equal(haBloccoDiModifiche("solo parole"), false);
  assert.equal(haBloccoDiModifiche("```json\n[]\n```"), false);
  // Il nome senza i tre apici non è un blocco.
  assert.equal(haBloccoDiModifiche("il " + LINGUAGGIO_PATCH + " non è un blocco"), false);
  // Senza stato: chiederlo due volte sullo stesso testo dà la stessa risposta.
  const testo = bloccoCon(LINGUAGGIO_PATCH_PRECEDENTE);
  assert.equal(haBloccoDiModifiche(testo), true);
  assert.equal(haBloccoDiModifiche(testo), true);
});

test("BLOCCO trova i blocchi di tutti e due i tag, anche quello rimasto aperto", () => {
  const testo =
    "uno ```" + LINGUAGGIO_PATCH + "\n[]\n``` due ```" + LINGUAGGIO_PATCH_PRECEDENTE + "\n[]\n``` tre ```" +
    LINGUAGGIO_PATCH_PRECEDENTE + "\n[{";
  const trovati = testo.match(BLOCCO);
  assert.equal(trovati?.length, 3);
  assert.equal(trovati?.[0], "```" + LINGUAGGIO_PATCH + "\n[]\n```");
  assert.equal(trovati?.[1], "```" + LINGUAGGIO_PATCH_PRECEDENTE + "\n[]\n```");
  assert.equal(trovati?.[2], "```" + LINGUAGGIO_PATCH_PRECEDENTE + "\n[{");
  assert.equal(testo.replace(BLOCCO, "").replace(/\s+/g, " ").trim(), "uno due tre");
});

test("conTagNuovo: l'apertura di prima diventa quella di adesso, il resto non si tocca", () => {
  const vecchio = bloccoCon(LINGUAGGIO_PATCH_PRECEDENTE);
  assert.equal(conTagNuovo(vecchio), bloccoCon(LINGUAGGIO_PATCH));
  assert.equal(conTagNuovo(bloccoCon(LINGUAGGIO_PATCH)), bloccoCon(LINGUAGGIO_PATCH));
  assert.equal(conTagNuovo("niente blocchi"), "niente blocchi");
  // Anche più blocchi nella stessa risposta.
  assert.equal(conTagNuovo(vecchio + vecchio).includes(LINGUAGGIO_PATCH_PRECEDENTE), false);
});

test("storia leggera: un blocco di prima, oltre la soglia, si riassume come uno di adesso", () => {
  const vecchio = bloccoCon(LINGUAGGIO_PATCH_PRECEDENTE);
  const storia = [
    { ruolo: "user" as const, testo: "u".repeat(900) },
    { ruolo: "assistant" as const, testo: vecchio, applicata: false },
    { ruolo: "user" as const, testo: "due" },
    { ruolo: "assistant" as const, testo: vecchio, applicata: true },
    { ruolo: "user" as const, testo: "tre" },
    { ruolo: "assistant" as const, testo: vecchio },
  ];
  const leggera = storiaLeggera(storia, 100);
  // Riassunto: la riga che lo sostituisce parla solo col tag di adesso.
  assert.equal(
    leggera[1]?.testo,
    "Ecco.\n[a " + LINGUAGGIO_PATCH + " block, NOT applied: the user discarded it]",
  );
  // Un messaggio senza segno, da una chat più vecchia ancora, si racconta applicato.
  const senzaSegno = storiaLeggera([{ ruolo: "assistant", testo: vecchio }, ...storia.slice(2)], 100);
  assert.equal(senzaSegno[0]?.testo, "Ecco.\n[a " + LINGUAGGIO_PATCH + " block, already applied]");
  // Fra le ultime battute il blocco resta intero, ma col tag di adesso.
  assert.equal(leggera[3]?.testo, bloccoCon(LINGUAGGIO_PATCH));
  assert.equal(leggera[5]?.testo, bloccoCon(LINGUAGGIO_PATCH));
  // Del vecchio nome, al modello, non arriva niente.
  assert.ok(leggera.every((m) => !m.testo.includes(LINGUAGGIO_PATCH_PRECEDENTE)));
});

test("storia leggera: una proposta scartata di prima non si racconta come applicata", () => {
  const storia = [
    { ruolo: "user" as const, testo: "scurisci" },
    { ruolo: "assistant" as const, testo: bloccoCon(LINGUAGGIO_PATCH_PRECEDENTE), applicata: false },
    { ruolo: "user" as const, testo: "ancora" },
  ];
  // Sotto la soglia nessuno riassume: il blocco resta intero e una riga dice com'è finita.
  const leggera = storiaLeggera(storia);
  assert.equal(leggera[1]?.testo, `${bloccoCon(LINGUAGGIO_PATCH)}\n${NON_APPLICATA}`);
  assert.ok(leggera.every((m) => !m.testo.includes(LINGUAGGIO_PATCH_PRECEDENTE)));
  // Identica a quella che darebbe lo stesso blocco scritto col tag di adesso.
  const nuova = storiaLeggera([
    storia[0]!,
    { ruolo: "assistant", testo: bloccoCon(LINGUAGGIO_PATCH), applicata: false },
    storia[2]!,
  ]);
  assert.deepEqual(leggera, nuova);
  // Il segno non parte verso il servizio.
  assert.ok(leggera.every((m) => !("applicata" in m)));
});

test("storia leggera: una frase che nomina il tag senza blocco non cambia", () => {
  const storia = [
    { ruolo: "user" as const, testo: "usi " + LINGUAGGIO_PATCH_PRECEDENTE + "?" },
    { ruolo: "assistant" as const, testo: "no", applicata: false },
  ];
  // Non c'è un blocco (mancano i tre apici): niente riga di «scartata», niente riscrittura.
  assert.deepEqual(storiaLeggera(storia), [
    { ruolo: "user", testo: "usi " + LINGUAGGIO_PATCH_PRECEDENTE + "?" },
    { ruolo: "assistant", testo: "no" },
  ]);
});

test("al modello si chiede solo il tag di adesso", () => {
  for (const applicato of [true, false]) {
    const testo = correzione([problema("id")], ["una ragione"], applicato);
    assert.ok(!testo.includes(LINGUAGGIO_PATCH_PRECEDENTE));
  }
  assert.ok(correzione([], [], false).includes("`" + LINGUAGGIO_PATCH + "`"));
  assert.ok(NON_APPLICATA.includes(LINGUAGGIO_PATCH));
  assert.ok(!NON_APPLICATA.includes(LINGUAGGIO_PATCH_PRECEDENTE));
});
