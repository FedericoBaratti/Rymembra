import { test } from "node:test";
import assert from "node:assert/strict";

import { daSalvare, iniziale, inValidazione, riduci, scafaleScrivibile, testo } from "../src/studio/negozio";
import type { Validazione } from "../src/ipc";

const esito = (errori: number) =>
  ({ errori: Array.from({ length: errori }, () => ({})), css: "x" }) as unknown as Validazione;

test("una validazione vecchia non sovrascrive quella del testo di adesso", () => {
  let s = riduci(iniziale("prova"), { tipo: "apri", testo: "a" });
  s = riduci(s, { tipo: "scrivi", testo: "b", etichetta: "b", gruppo: "g", adesso: 1 });
  s = riduci(s, { tipo: "validato", testo: "a", esito: esito(0) });
  assert.equal(s.esito, null);
  assert.ok(inValidazione(s));
  s = riduci(s, { tipo: "validato", testo: "b", esito: esito(1) });
  assert.equal(s.esito?.errori.length, 1);
  assert.equal(s.valido, null, "un esito con errori non diventa l'ultimo valido");
  assert.ok(!inValidazione(s));
});

test("cambiare selezione riporta l'ispettore all'aspetto di base", () => {
  let s = riduci(iniziale("prova"), { tipo: "scegli", selezione: { tipo: "parte", nome: "card" } });
  s = riduci(s, { tipo: "stato", stato: "hover" });
  s = riduci(s, { tipo: "scegli", selezione: { tipo: "parte", nome: "card" } });
  assert.equal(s.stato, "hover", "la stessa selezione non cambia niente");
  s = riduci(s, { tipo: "scegli", selezione: { tipo: "parte", nome: "heart" } });
  assert.equal(s.stato, null);
});

test("mostrare una parte porta la tela dove si vede", () => {
  let s = riduci(iniziale("prova"), { tipo: "mostra", parte: "explore-row" });
  assert.equal(s.scena.pagina, "esplora");
  s = riduci(s, { tipo: "mostra", parte: "menu-item" });
  assert.equal(s.scena.pagina, "esplora", "una sovrapposizione resta sulla pagina di prima");
  assert.deepEqual(s.scena.sovrapposizioni, ["menu"]);
  s = riduci(s, { tipo: "mostra", parte: "bottom-nav" });
  assert.equal(s.larghezza, "stretta");
});

test("salvare e annullare dicono se c'è da salvare", () => {
  let s = riduci(iniziale("prova"), { tipo: "apri", testo: "a" });
  assert.ok(!daSalvare(s));
  s = riduci(s, { tipo: "scrivi", testo: "b", etichetta: "b", gruppo: "g", adesso: 1 });
  assert.ok(daSalvare(s));
  s = riduci(s, { tipo: "salvato", testo: "b" });
  assert.ok(!daSalvare(s));
  s = riduci(s, { tipo: "annulla" });
  assert.equal(testo(s), "a");
  assert.ok(daSalvare(s), "tornare all'originale è una modifica da scrivere anche lei");
});

test("«vai alla riga» si mostra una volta, e una richiesta nuova non si perde", () => {
  let s = riduci(iniziale("prova"), { tipo: "apri", testo: "a" });
  s = riduci(s, { tipo: "vaiRiga", riga: 3, colonna: 1 });
  const prima = s.riga?.volta ?? -1;
  // Una richiesta nuova arrivata prima che l'editor dica «mostrata».
  s = riduci(s, { tipo: "vaiRiga", riga: 9, colonna: 2 });
  s = riduci(s, { tipo: "rigaMostrata", volta: prima });
  assert.equal(s.riga?.riga, 9, "la richiesta nuova resta");
  s = riduci(s, { tipo: "rigaMostrata", volta: s.riga?.volta ?? -1 });
  assert.equal(s.riga, null, "mostrata, si dimentica");
});

test("lo scafale non si riscrive finché l'albero che si vede è di un testo vecchio", () => {
  // Due gesti rapidi partivano dallo stesso albero vecchio: il secondo
  // riscriveva layout.shell intero e disfaceva il primo.
  let s = riduci(iniziale("prova"), { tipo: "apri", testo: "a" });
  s = riduci(s, { tipo: "validato", testo: "a", esito: esito(0) });
  assert.ok(scafaleScrivibile(s));
  s = riduci(s, { tipo: "scrivi", testo: "b", etichetta: "b", gruppo: "g", adesso: 1 });
  assert.ok(!scafaleScrivibile(s), "la validazione di «b» è ancora in volo");
  s = riduci(s, { tipo: "validato", testo: "b", esito: esito(0) });
  assert.ok(scafaleScrivibile(s));
  s = riduci(s, { tipo: "scrivi", testo: "c", etichetta: "c", gruppo: "g", adesso: 2 });
  s = riduci(s, { tipo: "validato", testo: "c", esito: esito(1) });
  assert.ok(!scafaleScrivibile(s), "con errori l'albero è quello dell'ultima versione buona");
});

test("un'istantanea presa fuori dal pannello fa crescere il contatore che il pannello guarda", () => {
  // L'elenco delle istantanee si rileggeva solo coi tasti del pannello: un
  // Ctrl+S, «Salva e usa», «Esporta» o la chat non lo aggiornavano.
  let s = riduci(iniziale("prova"), { tipo: "apri", testo: "a" });
  const prima = s.istantanee;
  s = riduci(s, { tipo: "fotografata" });
  assert.equal(s.istantanee, prima + 1);
});
