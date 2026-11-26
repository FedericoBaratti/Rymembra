/**
 * Gli artisti in mano e la lettura di un catalogo che il filtro non vede
 * (S9-3, D-4 del collaudo AVD-2): `comeRileggereGliArtisti` in `src/artisti.ts`.
 *
 * Il test non ha un DOM: la decisione è una funzione pura, e il cablaggio
 * dell'hook si prova sul testo sorgente.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { comeRileggereGliArtisti } from "../src/origine";

const TUTTI = "tutti";
const LOCALE = "locale";
const PC = "d:" + "aa".repeat(32);

test("D-4: sotto lo stesso filtro, la lettura di un catalogo fuori filtro non rilegge gli artisti", () => {
  // La vista Artisti aperta con «Questo dispositivo»: ogni 3 s `ipc.artisti`.
  assert.equal(comeRileggereGliArtisti(LOCALE, LOCALE, true), "mai");
  assert.equal(comeRileggereGliArtisti(PC, PC, true), "mai");
});

test("con una causa che conta (scansione, scarico, «Tutti», quel dispositivo) si rilegge subito", () => {
  assert.equal(comeRileggereGliArtisti(TUTTI, TUTTI, false), "subito");
  assert.equal(comeRileggereGliArtisti(LOCALE, LOCALE, false), "subito");
  assert.equal(comeRileggereGliArtisti(PC, PC, false), "subito");
});

test("gli artisti di un altro filtro, rimasti in mano, invecchiano: si rileggono entrando", () => {
  // Gli artisti in mano sono quelli di «Tutti» (vista Artisti aperta, poi Album),
  // il filtro è diventato «Questo dispositivo» — uno solo per la finestra — e il
  // catalogo di A entra: la rilettura è fuori filtro, ma quelli di «Tutti» no.
  // Tornando a «Tutti» (con `lette === chiaveOrigine` e nessun segno) la vista
  // non li richiederebbe più, e mostrerebbe un elenco senza gli artisti di A.
  assert.equal(comeRileggereGliArtisti(TUTTI, LOCALE, true), "entrando");
  assert.equal(comeRileggereGliArtisti(TUTTI, PC, true), "entrando");
  assert.equal(comeRileggereGliArtisti(PC, LOCALE, true), "entrando");
});

/**
 * Il modello dell'hook, con le sue tre regole (`useArtisti`): le chiavi in mano,
 * il segno «da rileggere», e quel che fa entrare nella vista. Le richieste a
 * `ipc.artisti` sono quel che si conta.
 */
function vista() {
  let lette = TUTTI;
  let daRileggere = true;
  let filtro = TUTTI;
  let dentro = false;
  let richieste: string[] = [];
  const effetto = () => {
    if (!dentro) return;
    if (lette !== filtro) {
      lette = filtro;
      daRileggere = true;
    }
    if (!daRileggere) return;
    daRileggere = false;
    richieste.push(filtro);
  };
  return {
    entra() {
      dentro = true;
      effetto();
    },
    esce() {
      dentro = false;
    },
    filtra(nuovo: string) {
      filtro = nuovo;
      effetto();
    },
    rilettura(fuoriFiltro: boolean) {
      const come = comeRileggereGliArtisti(lette, filtro, fuoriFiltro);
      if (come === "mai") return;
      daRileggere = true;
      if (come === "subito") effetto();
    },
    prendi() {
      const fatte = richieste;
      richieste = [];
      return fatte;
    },
  };
}

test("D-4: la sequenza del collaudo non fa una chiamata, e tornando a «Tutti» gli artisti ci sono", () => {
  const v = vista();
  // 1. La vista Artisti sotto «Tutti»: una lettura.
  v.entra();
  assert.deepEqual(v.prendi(), [TUTTI]);
  // 2. Album, e il filtro diventa «Questo dispositivo».
  v.esce();
  v.filtra(LOCALE);
  // 3. Il catalogo di A entra, tre riletture accorpate fuori filtro.
  v.rilettura(true);
  v.rilettura(true);
  v.rilettura(true);
  assert.deepEqual(v.prendi(), [], "fuori filtro, con la vista chiusa, niente");
  // 4. «Tutti» di nuovo, e si entra in Artisti: una lettura, con gli artisti di A.
  v.filtra(TUTTI);
  v.entra();
  assert.deepEqual(v.prendi(), [TUTTI], "gli artisti di «Tutti» in mano erano vecchi");
});

test("D-4: con la vista aperta sotto «Questo dispositivo» la lettura di A non chiama niente", () => {
  const v = vista();
  v.filtra(LOCALE);
  v.entra();
  assert.deepEqual(v.prendi(), [LOCALE], "l'entrata sotto il nuovo filtro");
  for (let giro = 0; giro < 4; giro++) v.rilettura(true);
  assert.deepEqual(v.prendi(), [], "nessuna chiamata durante la lettura");
  // Una scansione nella stessa raffica, o «Tutti»: lì la vista cambia davvero.
  v.rilettura(false);
  assert.deepEqual(v.prendi(), [LOCALE]);
});

test("`useArtisti` decide con `comeRileggereGliArtisti`, e `ricaricaContorno` gliela lascia sempre", () => {
  const artisti = readFileSync(new URL("../src/artisti.ts", import.meta.url), "utf8");
  const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
  assert.match(artisti, /comeRileggereGliArtisti\(lette\.current, chiaveOrigine, opzioni\?\.fuoriFiltro === true\)/);
  assert.match(artisti, /if \(come === "subito"\) setGiroArtisti\(/);
  // Non sta dentro la guardia di `ricaricaContorno`: la guardia non sa di chi sono
  // gli artisti in mano, l'hook sì.
  const inizio = app.indexOf("const ricaricaContorno = useCallback(");
  const contorno = app.slice(inizio, app.indexOf("}, [ricaricaAlbum", inizio));
  const fuori = contorno.replace(/if \(!opzioni\?\.fuoriFiltro\) \{[^}]*\}/, "");
  assert.match(fuori, /invalidaArtisti\(opzioni\);/);
});
