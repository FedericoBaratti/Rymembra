import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import {
  ATTESA_ANCORA_MS,
  DISTACCO,
  codaDaSvuotare,
  comeParteIlGiro,
  posaFumetto,
  propostaPuoComparire,
  type CondizioniDelGiro,
  type CondizioniDellaProposta,
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
  telefono: false,
};

test("con tutte le condizioni il giro parte", () => {
  assert.equal(comeParteIlGiro(PRONTO), "parti");
});

test("sul telefono il giro non parte (D-37)", () => {
  // Il giro lì non si disegna: partiva lo stesso, senza vedersi, riempiva la
  // coda con cinquanta brani a ogni avvio con la coda vuota e non si chiudeva
  // mai (R12-2).
  assert.equal(comeParteIlGiro({ ...PRONTO, telefono: true }), "no");
  assert.equal(comeParteIlGiro({ ...PRONTO, telefono: true, modaleAperto: true }), "no");
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

test("a schermo intero il fumetto non copre i tasti che il passo descrive (R18-12)", () => {
  // Il passo «A tutto schermo» a 1280×820, dal vivo: il bersaglio è il posto
  // del contenuto, il fumetto stava a x 570–950 e y 12–227, e sotto c'erano
  // proprio «testo» (x 881) e «spettro» (x 917).
  const schermo = { larghezza: 1280, altezza: 820 };
  const contenuto = { top: 0, left: 240, width: 1040, height: 820 };
  const fumetto = { width: 380, height: 215 };
  const tasti = [
    { top: 16, left: 867, width: 28, height: 28 },
    { top: 16, left: 903, width: 28, height: 28 },
  ];
  const posto = posaFumetto(contenuto, fumetto, schermo, tasti);
  for (const tasto of tasti) {
    const fuori =
      posto.left + fumetto.width <= tasto.left ||
      tasto.left + tasto.width <= posto.left ||
      posto.top + fumetto.height <= tasto.top ||
      tasto.top + tasto.height <= posto.top;
    assert.ok(fuori, `il fumetto in ${JSON.stringify(posto)} copre il tasto in ${tasto.left}`);
  }
  // Dentro la finestra, come sempre.
  assert.ok(posto.top >= DISTACCO && posto.top + fumetto.height <= schermo.altezza - DISTACCO);
  assert.ok(posto.left >= DISTACCO && posto.left + fumetto.width <= schermo.larghezza - DISTACCO);
  // Senza punti da evitare, il posto di sempre.
  assert.deepEqual(posaFumetto(contenuto, fumetto, schermo), { top: DISTACCO, left: 760 - 190 });
  // E un punto che non dà fastidio non sposta niente.
  assert.deepEqual(posaFumetto(contenuto, fumetto, schermo, [{ top: 700, left: 300, width: 20, height: 20 }]), {
    top: DISTACCO,
    left: 760 - 190,
  });
});

test("il giro passa a posaFumetto i punti del passo, e «A tutto schermo» ne ha due", () => {
  const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");
  const giro = sorgente("Giro.tsx");
  assert.match(giro, /bersaglio\.querySelectorAll<HTMLElement>\("\[data-giro-interesse\]"\)/);
  assert.match(giro, /schermo\(\),\s+evita,\s+\)/);
  const grande = sorgente("schermate/InRiproduzione.tsx");
  assert.equal(grande.match(/data-giro-interesse=""/g)?.length, 2);
  for (const chiave of ["np.lyrics.toggle", "np.spectrum.toggle"]) {
    const prima = grande.slice(0, grande.indexOf(`aria-label={t("${chiave}")}`));
    const tasto = prima.slice(prima.lastIndexOf("<button"));
    assert.match(tasto, /data-giro-interesse=""/, chiave);
  }
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

test("il benvenuto conta i passi veri: il numero non è scritto a parole (R17-7)", () => {
  const it = JSON.parse(readFileSync(new URL("../src/lingue/it.json", import.meta.url), "utf8")) as Record<string, string>;
  const en = JSON.parse(readFileSync(new URL("../src/lingue/en.json", import.meta.url), "utf8")) as Record<string, string>;
  // «In tredici passi» restava così quando i passi erano quattordici.
  assert.match(it["tour.step.benvenuto.body"] ?? "", /In \{n\} passi/);
  assert.match(en["tour.step.benvenuto.body"] ?? "", /In \{n\} steps/);
  for (const [chiave, testo] of [...Object.entries(it), ...Object.entries(en)]) {
    if (chiave.startsWith("tour.")) {
      assert.doesNotMatch(testo, /\b(tredici|quattordici|quindici|thirteen|fourteen|fifteen)\b/i, chiave);
    }
  }
  // E il numero è lo stesso del contatore («3 di 14»): una sola espressione, `totale`.
  const giro = readFileSync(new URL("../src/Giro.tsx", import.meta.url), "utf8");
  assert.match(giro, /const totale = COPIONE\.length - saltati\.size;/);
  assert.match(giro, /t\("tour\.progress", \{[^}]*\btotale,/s);
  assert.match(giro, /t\(`tour\.step\.\$\{passo\[0\]\}\.body`, \{ n: totale \}\)/);
});

// ── la proposta del sottofondo (FC2) ─────────────────────────────────────────

/** Tutto a posto: la proposta può comparire. */
const PROPOSTA_LIBERA: CondizioniDellaProposta = {
  primoVisibile: false,
  giroDaFare: false,
  giroAperto: false,
  novitaInAttesa: false,
  telefono: false,
};

test("la proposta compare quando niente la trattiene", () => {
  assert.equal(propostaPuoComparire(PROPOSTA_LIBERA), true);
  assert.equal(propostaPuoComparire({ ...PROPOSTA_LIBERA, telefono: true }), true);
});

test("sul computer la proposta aspetta Primo, il giro e le Novità", () => {
  for (const trattiene of [
    { primoVisibile: true },
    { giroDaFare: true },
    { giroAperto: true },
    { novitaInAttesa: true },
  ]) {
    assert.equal(
      propostaPuoComparire({ ...PROPOSTA_LIBERA, ...trattiene }),
      false,
      JSON.stringify(trattiene),
    );
  }
});

test("sul telefono il giro non esiste, e la proposta non lo aspetta (collaudo sugli emulatori)", () => {
  // `giroDaFare` resta vero per sempre, perché il giro lì non parte mai
  // (`comeParteIlGiro`, D-37): era il motivo per cui la scheda non compariva.
  const telefono = { ...PROPOSTA_LIBERA, telefono: true };
  assert.equal(propostaPuoComparire({ ...telefono, giroDaFare: true }), true);
  assert.equal(propostaPuoComparire({ ...telefono, giroDaFare: true, giroAperto: true }), true);
  // Quel che c'è anche sul telefono la trattiene: Primo e le Novità.
  assert.equal(propostaPuoComparire({ ...telefono, giroDaFare: true, primoVisibile: true }), false);
  assert.equal(propostaPuoComparire({ ...telefono, giroDaFare: true, novitaInAttesa: true }), false);
});
