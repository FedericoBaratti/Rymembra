import { test } from "node:test";
import assert from "node:assert/strict";

import { faseLettura, sostituibileDaQui } from "../src/parti/ramoTesto";

/*
 * Il pannello del testo prima che il testo ci sia.
 *
 * Finché `testoBrano` non rispondeva il pannello non disegnava niente — un
 * riquadro vuoto con l'occhiello, per decine di secondi su una condivisione
 * lenta — e dopo una lettura fallita restava vuoto per sempre. Qui la scelta
 * di cosa dire, senza React.
 */

test("mentre il testo si legge dal disco il pannello lo dice", () => {
  assert.equal(faseLettura({ testo: null, leggendo: true, illeggibile: false }), "leggo");
});

test("una lettura fallita si dice, invece di un pannello vuoto", () => {
  assert.equal(faseLettura({ testo: null, leggendo: false, illeggibile: true }), "illeggibile");
});

test("col testo arrivato si disegna il testo", () => {
  const testo = { strumentale: false };
  assert.equal(faseLettura({ testo, leggendo: false, illeggibile: false }), "pronto");
});

/*
 * «Non è questo» e «Altri testi» sul testo del proprietario del brano.
 *
 * Il pannello li offriva solo per `lrclib`: la copia del testo che ha il
 * proprietario (`proprietario`) non si poteva scartare, perché alla prossima
 * apertura il nucleo la rimetteva. Adesso un rifiuto vale finché lui manda lo
 * stesso testo (`testi::catalogo::rifiuta`), quindi il gesto si offre.
 */

test("il testo del catalogo e la copia di quello del proprietario si sostituiscono da qui", () => {
  assert.equal(sostituibileDaQui("lrclib"), true);
  assert.equal(sostituibileDaQui("proprietario"), true);
});

test("il testo di chi ascolta, il sidecar e il tag non si sostituiscono da qui", () => {
  for (const fonte of ["mano", "sidecar", "tag", "nessuna", ""]) {
    assert.equal(sostituibileDaQui(fonte), false, fonte);
  }
});
