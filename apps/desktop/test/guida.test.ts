import { test } from "node:test";
import assert from "node:assert/strict";

import {
  ATTESA_ANCORA_MS,
  DISTACCO,
  codaDaSvuotare,
  comeParteIlGiro,
  posaFumetto,
  type CondizioniDelGiro,
} from "../src/guida";

/** Le condizioni in cui il giro parte: ogni prova ne cambia una. */
const PRONTO: CondizioniDelGiro = {
  daFare: true,
  aperto: false,
  avviato: true,
  primoVisibile: false,
  brani: 120,
  inEsplora: false,
  studioVisibile: false,
  modaleAperto: false,
};

test("con tutte le condizioni il giro parte", () => {
  assert.equal(comeParteIlGiro(PRONTO), "parti");
});

test("sopra un modale aperto il giro aspetta, non rinuncia", () => {
  assert.equal(comeParteIlGiro({ ...PRONTO, modaleAperto: true }), "aspetta");
});

test("ogni condizione che manca lo ferma, modale o no", () => {
  const mancanze: Partial<CondizioniDelGiro>[] = [
    { daFare: false },
    { aperto: true },
    { avviato: false },
    { primoVisibile: true },
    { brani: 0 },
    { inEsplora: true },
    { studioVisibile: true },
  ];
  for (const manca of mancanze) {
    assert.equal(comeParteIlGiro({ ...PRONTO, ...manca }), "no", JSON.stringify(manca));
    // Un modale aperto non trasforma un «no» in un'attesa: aspettare che si
    // chiuda non servirebbe, la condizione che manca resterebbe.
    assert.equal(
      comeParteIlGiro({ ...PRONTO, ...manca, modaleAperto: true }),
      "no",
      JSON.stringify(manca),
    );
  }
});

test("lo Studio aperto conta come una condizione che manca, non come un modale", () => {
  // Lo Studio sta sopra il velo del giro: si aspetta che si chiuda, e la
  // domanda si rifà da sé perché `studioVisibile` è uno stato di `App`.
  assert.equal(comeParteIlGiro({ ...PRONTO, studioVisibile: true }), "no");
});

test("l'attesa di un'ancora è un secondo e mezzo", () => {
  assert.equal(ATTESA_ANCORA_MS, 1_500);
});

const SCHERMO = { larghezza: 1280, altezza: 800 };
const FUMETTO = { width: 380, height: 180 };

test("il fumetto si posa sotto il buco quando ci sta, centrato", () => {
  const buco = { top: 100, left: 400, width: 200, height: 40 };
  assert.deepEqual(posaFumetto(buco, FUMETTO, SCHERMO), {
    top: 100 + 40 + DISTACCO,
    left: 400 + 100 - 190,
  });
});

test("sopra quando sotto non c'è posto", () => {
  const buco = { top: 700, left: 400, width: 200, height: 60 };
  assert.deepEqual(posaFumetto(buco, FUMETTO, SCHERMO), {
    top: 700 - DISTACCO - 180,
    left: 310,
  });
});

test("un bersaglio alto quanto la finestra lo affianca, prima a destra", () => {
  // La barra laterale: alta tutta la finestra, stretta, a sinistra.
  const barra = { top: 0, left: 0, width: 240, height: 800 };
  assert.deepEqual(posaFumetto(barra, FUMETTO, SCHERMO), {
    top: 400 - 90,
    left: 240 + DISTACCO,
  });
});

test("e a sinistra quando a destra non ci sta", () => {
  // La colonna di destra: alta tutta la finestra, contro il bordo destro.
  const colonna = { top: 0, left: 932, width: 348, height: 800 };
  assert.deepEqual(posaFumetto(colonna, FUMETTO, SCHERMO), {
    top: 310,
    left: 932 - DISTACCO - 380,
  });
});

test("grande quanto tutto lo schermo: in alto, sovrapposto, dentro la finestra", () => {
  const tutto = { top: 0, left: 0, width: 1280, height: 800 };
  const posto = posaFumetto(tutto, FUMETTO, SCHERMO);
  assert.equal(posto.top, DISTACCO);
  assert.equal(posto.left, 640 - 190);
});

test("il fumetto non esce mai dalla finestra", () => {
  // Un buco piccolo nell'angolo in basso a destra: sotto non ci sta, sopra sì,
  // e centrato sul buco uscirebbe a destra.
  const angolo = { top: 760, left: 1250, width: 20, height: 20 };
  const posto = posaFumetto(angolo, FUMETTO, SCHERMO);
  assert.ok(posto.left + FUMETTO.width <= SCHERMO.larghezza - DISTACCO);
  assert.ok(posto.top >= DISTACCO);
  assert.ok(posto.top + FUMETTO.height <= SCHERMO.altezza - DISTACCO);
});

test("la coda del giro non toccata si svuota chiudendo", () => {
  const messa = [5, 9, 2];
  assert.equal(
    codaDaSvuotare(messa, { coda: [5, 9, 2], posizioneCoda: 0, posizioneMs: 0 }),
    true,
  );
  // Con lo shuffle il nucleo la mescola: gli stessi brani in un altro ordine
  // sono ancora la coda del giro.
  assert.equal(
    codaDaSvuotare(messa, { coda: [2, 5, 9], posizioneCoda: 0, posizioneMs: 0 }),
    true,
  );
});

test("una coda usata durante il giro resta", () => {
  const messa = [5, 9, 2];
  // Partita: la puntina si è mossa.
  assert.equal(
    codaDaSvuotare(messa, { coda: [5, 9, 2], posizioneCoda: 0, posizioneMs: 1200 }),
    false,
  );
  // Saltato al brano dopo.
  assert.equal(
    codaDaSvuotare(messa, { coda: [5, 9, 2], posizioneCoda: 1, posizioneMs: 0 }),
    false,
  );
  // Un brano in più in coda.
  assert.equal(
    codaDaSvuotare(messa, { coda: [5, 9, 2, 7], posizioneCoda: 0, posizioneMs: 0 }),
    false,
  );
  // Sostituita da un'altra lista lunga uguale.
  assert.equal(
    codaDaSvuotare(messa, { coda: [5, 9, 3], posizioneCoda: 0, posizioneMs: 0 }),
    false,
  );
});

test("se il giro non ha messo niente in coda, non si svuota niente", () => {
  const ferma = { coda: [1, 2], posizioneCoda: 0, posizioneMs: 0 };
  assert.equal(codaDaSvuotare(null, ferma), false);
  assert.equal(codaDaSvuotare([], { ...ferma, coda: [] }), false);
});
