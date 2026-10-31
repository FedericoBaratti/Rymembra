import { test } from "node:test";
import assert from "node:assert/strict";

import {
  creaProva,
  durataDaCampo,
  durataInCampo,
  limiteDaCampo,
  numeroDaCampo,
} from "../src/editorRegole";

/** Una domanda finta, che risponde quando la si risolve a mano. */
function domande<R>() {
  const aperte: { risolvi: (r: R) => void; rifiuta: (e: unknown) => void }[] = [];
  const chiedi = () =>
    new Promise<R>((risolvi, rifiuta) => {
      aperte.push({ risolvi, rifiuta });
    });
  return { chiedi, aperte };
}

const giro = () => new Promise((fatto) => setTimeout(fatto, 0));

test("vince l'ultima anteprima, non l'ultima arrivata", async () => {
  const { chiedi, aperte } = domande<string>();
  const scritte: string[] = [];
  const prova = creaProva(chiedi, { risposta: (r) => scritte.push(r), errore: () => undefined });
  prova("beatl");
  prova("beatles");
  aperte[1]?.risolvi("beatles");
  await giro();
  aperte[0]?.risolvi("beatl");
  await giro();
  assert.deepEqual(scritte, ["beatles"]);
});

test("un'anteprima riuscita toglie l'errore dell'anteprima, e un errore vecchio non torna", async () => {
  const { chiedi, aperte } = domande<string>();
  let errore: unknown = null;
  let anteprima: string | null = null;
  const prova = creaProva(chiedi, {
    risposta: (r) => {
      anteprima = r;
      errore = null;
    },
    errore: (e) => {
      errore = e;
    },
  });
  prova("a");
  aperte[0]?.rifiuta("db.locked");
  await giro();
  assert.equal(errore, "db.locked");
  prova("ab");
  prova("abc");
  aperte[2]?.risolvi("abc");
  await giro();
  aperte[1]?.rifiuta("db.locked");
  await giro();
  assert.equal(anteprima, "abc");
  assert.equal(errore, null);
});

test("un numero in una regola è un intero, e un campo vuoto è incompleto", () => {
  assert.equal(numeroDaCampo("3.5"), 3);
  assert.equal(numeroDaCampo("-2.7"), -2);
  assert.equal(numeroDaCampo("12"), 12);
  assert.equal(numeroDaCampo(""), null);
  assert.equal(numeroDaCampo("  "), null);
  assert.equal(numeroDaCampo("1e20"), Number.MAX_SAFE_INTEGER);
  assert.equal(limiteDaCampo("2.5"), 2);
  assert.equal(limiteDaCampo("1e10"), 4294967295);
  assert.equal(limiteDaCampo("-4"), 0);
  assert.equal(limiteDaCampo(""), null);
});

test("la durata si scrive in minuti e secondi, e si salva in millisecondi", () => {
  assert.equal(durataDaCampo("5:30"), 330_000);
  assert.equal(durataDaCampo("5"), 300_000);
  assert.equal(durataDaCampo("0:45"), 45_000);
  assert.equal(durataDaCampo(" 12:05 "), 725_000);
  assert.equal(durataDaCampo(""), null);
  assert.equal(durataDaCampo("5:"), null);
  assert.equal(durataDaCampo("5:75"), null);
  assert.equal(durataDaCampo("cinque"), null);
  assert.equal(durataInCampo(330_000), "5:30");
  assert.equal(durataInCampo(0), "0:00");
  assert.equal(durataInCampo(null), "");
});

test("una durata salvata che non è a secondi interi torna identica", () => {
  // Regole scritte quando il campo era in millisecondi: mostrate e risalvate
  // senza toccarle, non devono cambiare.
  for (const ms of [5, 330_500, 299_999, 61_010]) {
    const testo = durataInCampo(ms);
    assert.equal(durataDaCampo(testo), ms, `${ms} → «${testo}»`);
  }
  assert.equal(durataInCampo(330_500), "5:30.5");
});
