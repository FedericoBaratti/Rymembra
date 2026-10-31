import { test } from "node:test";
import assert from "node:assert/strict";

import {
  CURSORE_PESO_VARIABILE,
  FERMATE_MASSIME,
  LIVELLI_OMBRA_MASSIMI,
  angoliDopo,
  angoliTagliati,
  conUnita,
  latoCambiato,
  puntoDaCasella,
  puoAggiungere,
} from "../src/studio/valori";

test("smusso: senza corners sono tagliati in alto a destra e in basso a sinistra", () => {
  // `document.rs`, ramo "chamfer": `None => vec![Corner::TopRight, Corner::BottomLeft]`.
  // I tasti li mostravano tutti spenti mentre la tela ne tagliava due.
  assert.deepEqual(angoliTagliati(undefined), ["topRight", "bottomLeft"]);
  assert.deepEqual(angoliTagliati(null), ["topRight", "bottomLeft"]);
  assert.deepEqual(angoliTagliati(["topLeft"]), ["topLeft"]);
});

test("smusso: premere un angolo parte da quelli tagliati davvero", () => {
  // Accendere «in alto a sinistra» su un campo assente scriveva ["topLeft"]:
  // i due di serie sparivano.
  assert.deepEqual(angoliDopo(undefined, "topLeft"), ["topRight", "bottomLeft", "topLeft"]);
  assert.deepEqual(angoliDopo(undefined, "topRight"), ["bottomLeft"]);
  assert.deepEqual(angoliDopo(["topRight", "bottomLeft"], "topRight"), ["bottomLeft"]);
});

test("smusso: l'ultimo angolo acceso non si spegne", () => {
  // Il formato vuole da uno a quattro angoli; togliere il campo faceva
  // tornare i due di serie, con tutti i tasti spenti.
  assert.equal(angoliDopo(["bottomLeft"], "bottomLeft"), null);
});

test("una lunghezza toccata da un cursore tiene la sua unità", () => {
  // L'ombra scriveva sempre «px»: «0.5rem» diventava «1px», altro valore e
  // altra unità.
  assert.equal(conUnita("0.5rem", 2), "2rem");
  assert.equal(conUnita("8px", "12"), "12px");
  assert.equal(conUnita(undefined, 3), "3px", "un campo vuoto prende l'unità di serie");
  assert.equal(conUnita("4", 6, "%"), "6%", "un numero senza unità prende quella di serie");
});

test("ritaglio: toccare un lato non riscrive gli altri in percentuale", () => {
  assert.deepEqual(latoCambiato(["8px", "8px", "8px", "8px"], 0, 10), ["10px", "8px", "8px", "8px"]);
  assert.deepEqual(latoCambiato(["5%", "1rem", "0", "2%"], 2, 4), ["5%", "1rem", "4%", "2%"]);
  assert.deepEqual(latoCambiato([], 1, 3), ["0%", "3%", "0%", "0%"]);
});

test("la nona fermata e il settimo livello d'ombra non si aggiungono", () => {
  // Il formato vuole da 2 a 8 fermate e da 0 a 6 livelli d'ombra: oltre, il
  // documento non si valida e la tela si ferma.
  assert.equal(FERMATE_MASSIME, 8);
  assert.equal(LIVELLI_OMBRA_MASSIMI, 6);
  assert.equal(puoAggiungere(7, FERMATE_MASSIME), true);
  assert.equal(puoAggiungere(8, FERMATE_MASSIME), false);
  assert.equal(puoAggiungere(5, LIVELLI_OMBRA_MASSIMI), true);
  assert.equal(puoAggiungere(6, LIVELLI_OMBRA_MASSIMI), false);
});

test("cubic-bezier: una casella vuota o fuori dagli estremi non si scrive", () => {
  // Svuotare x1 per riscriverlo scriveva NaN — null nel testo — e la casella
  // tornava a 0 sotto le dita.
  assert.equal(puntoDaCasella("", 0), null);
  assert.equal(puntoDaCasella("-", 1), null);
  assert.equal(puntoDaCasella("05", 0), null, "x sta fra 0 e 1");
  assert.equal(puntoDaCasella("1.5", 2), null);
  assert.equal(puntoDaCasella("0.3", 0), 0.3);
  assert.equal(puntoDaCasella("-2", 1), -2, "le y possono uscire da 0..1: è il rimbalzo");
  assert.equal(puntoDaCasella("6", 3), null, "ma non da -5..5");
});

test("peso variabile: il cursore arriva a 100, 400, 700, 900 e 1000", () => {
  // Un range ammette solo min + k·passo: con min 1 e passo 50 i pesi erano
  // 1, 51, 101… e 100–900 si scriveva solo nel codice.
  const { min, max, passo } = CURSORE_PESO_VARIABILE;
  for (const peso of [100, 400, 700, 900, 1000]) {
    assert.ok(peso >= min && peso <= max && (peso - min) % passo === 0, String(peso));
  }
});
