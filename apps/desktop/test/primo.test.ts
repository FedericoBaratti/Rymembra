import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { conAMano } from "../src/cartellePrimo";
import { tastoDelPrimo } from "../src/tastoPrimo";

const MUSICA = "C:\\Users\\X\\Music";
const NUVOLA = "nuvola://onedrive/conto/Musica";

test("una cartella del cloud aggiunta non toglie «Continua»", () => {
  // Music spuntata e mai scritta, la cartella del cloud scritta dal nucleo e
  // già letta: i suoi brani arrivano, ma «Continua» deve restare.
  assert.equal(
    tastoDelPrimo({ brani: 120, spuntate: [MUSICA, NUVOLA], scritte: [NUVOLA], confermato: false }),
    "continua",
  );
  // Dopo «Continua» riuscito, i brani bastano.
  assert.equal(
    tastoDelPrimo({ brani: 120, spuntate: [MUSICA, NUVOLA], scritte: [MUSICA, NUVOLA], confermato: true }),
    "ascolta",
  );
});

test("con tutte le spuntate già scritte i brani bastano a dire «Ascolta»", () => {
  assert.equal(tastoDelPrimo({ brani: 40, spuntate: [NUVOLA], scritte: [NUVOLA], confermato: false }), "ascolta");
  assert.equal(tastoDelPrimo({ brani: 40, spuntate: [], scritte: [NUVOLA], confermato: false }), "ascolta");
});

test("senza brani resta «Continua», confermato o no", () => {
  assert.equal(tastoDelPrimo({ brani: 0, spuntate: [MUSICA], scritte: [], confermato: false }), "continua");
  assert.equal(tastoDelPrimo({ brani: 0, spuntate: [MUSICA], scritte: [MUSICA], confermato: true }), "continua");
});

test("una cartella indicata a mano non è contata, e non dice «0 brani»", () => {
  // R18-6. Dal vivo: «Scegli un'altra cartella…» → la riga diceva «0 brani»,
  // e lo diceva ancora dopo «13 brani pronti».
  const [aMano] = conAMano([], ["D:\\musica"]);
  assert.equal(aMano?.percorso, "D:\\musica");
  assert.equal(aMano?.brani, null);
  // Una cartella già proposta dal sistema resta com'era, col suo conto.
  const proposta = { percorso: MUSICA, brani: 12, troncato: false, parziale: false };
  assert.deepEqual(conAMano([proposta], [MUSICA]), [proposta]);
  // E la riga disegna il conto solo per le cartelle contate.
  const primo = readFileSync(new URL("../src/Primo.tsx", import.meta.url), "utf8");
  assert.match(primo, /!eNuvola\(cartella\.percorso\) && cartella\.brani !== null &&/);
});

test("il primo avvio parla da una regione sempre montata, e dice i passaggi, non il conto", () => {
  // R16-12. Le regioni `role="status"` nascevano insieme al loro testo, dentro
  // rami condizionali (`Annuncio.tsx` spiega perché spesso tacciono), e «{n}
  // brani pronti» stava dentro la regione: cambiava a ogni lotto.
  const primo = readFileSync(new URL("../src/Primo.tsx", import.meta.url), "utf8");
  assert.doesNotMatch(primo, /role="status"/);
  const annuncio = primo.slice(primo.indexOf("const daDire"));
  const regola = annuncio.slice(0, annuncio.indexOf(";\n"));
  assert.match(regola, /primo\.pronto\.annuncio/);
  assert.match(regola, /primo\.vuota\.title/);
  assert.doesNotMatch(regola, /\{ n:/, "il conto non si annuncia");
  assert.match(primo, /<Annuncio testo=\{daDire\} \/>/);
});

test("con i brani già pronti l'uscita del primo avvio dice «Chiudi», non «Lo faccio dopo»", () => {
  // R18-18 (la parte del primo avvio): dopo «13 brani pronti» l'uscita
  // secondaria diceva ancora «Lo faccio dopo», come se si saltasse la lettura.
  const primo = readFileSync(new URL("../src/Primo.tsx", import.meta.url), "utf8");
  assert.match(primo, /\{brani > 0 \? t\("common\.close"\) : t\("primo\.salta"\)\}/);
});
