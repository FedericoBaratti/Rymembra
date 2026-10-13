import { test } from "node:test";
import assert from "node:assert/strict";

import {
  apriModale,
  daRendereInerti,
  fuocoDopoTab,
  fuocoIniziale,
  modaleAperto,
  nuovoVelo,
  passaLaScorciatoia,
  type NodoDiAlbero,
} from "../src/modale";

// ── il velo ────────────────────────────────────────────────────────────────

test("il velo chiude un clic nato e finito su di lui", () => {
  const velo = {};
  const v = nuovoVelo();
  v.premuto(velo, velo);
  assert.equal(v.clic(velo, velo), true);
});

test("una selezione trascinata da dentro a fuori non chiude", () => {
  // Il caso che ha fatto nascere la regola: il tasto si abbassa nel campo, si
  // alza sul velo, e il browser manda il `click` al velo.
  const velo = {};
  const campo = {};
  const v = nuovoVelo();
  v.premuto(campo, velo);
  assert.equal(v.clic(velo, velo), false);
});

test("una pressione sul velo e un clic dentro non chiudono", () => {
  const velo = {};
  const tasto = {};
  const v = nuovoVelo();
  v.premuto(velo, velo);
  assert.equal(v.clic(tasto, velo), false);
});

test("il ricordo della pressione si consuma a ogni clic", () => {
  // Una pressione sul velo seguita da un clic altrove non deve armare il
  // clic dopo, che è nato dentro.
  const velo = {};
  const v = nuovoVelo();
  v.premuto(velo, velo);
  v.clic({}, velo);
  assert.equal(v.clic(velo, velo), false, "senza una pressione nuova non chiude");
});

// ── il fuoco ───────────────────────────────────────────────────────────────

const marcati = (...chi: string[]) => (nodo: string) => chi.includes(nodo);

test("all'apertura il fuoco va sul primo comando", () => {
  assert.equal(fuocoIniziale(["campo", "annulla", "crea"], "radice", marcati()), "campo");
});

test("un comando marcato vince sul primo", () => {
  assert.equal(
    fuocoIniziale(["togli", "lasciali"], "radice", marcati("lasciali")),
    "lasciali",
  );
});

test("il marchio sulla radice tiene il fuoco sulla radice", () => {
  assert.equal(fuocoIniziale(["a", "b"], "radice", marcati("radice", "b")), "radice");
});

test("con tutti i comandi spenti il fuoco va sulla radice", () => {
  assert.equal(fuocoIniziale([], "radice", marcati()), "radice");
});

test("il Tab dall'ultimo torna al primo, lo Shift+Tab dal primo all'ultimo", () => {
  const fuochi = ["a", "b", "c"];
  assert.equal(fuocoDopoTab(fuochi, "c", true, false), "a");
  assert.equal(fuocoDopoTab(fuochi, "a", true, true), "c");
});

test("in mezzo all'anello il Tab lo fa il browser", () => {
  const fuochi = ["a", "b", "c"];
  assert.equal(fuocoDopoTab(fuochi, "a", true, false), null);
  assert.equal(fuocoDopoTab(fuochi, "b", true, true), null);
});

test("dalla radice o da fuori il giro ricomincia da un capo", () => {
  const fuochi = ["a", "b", "c"];
  // Fuori, o sulla radice: `dentro` è falso.
  assert.equal(fuocoDopoTab(fuochi, "radice", false, false), "a");
  assert.equal(fuocoDopoTab(fuochi, "radice", false, true), "c");
  assert.equal(fuocoDopoTab(fuochi, null, true, false), "a");
});

test("senza comandi accesi il Tab non ha dove andare", () => {
  assert.equal(fuocoDopoTab([], "radice", false, false), null);
});

// ── le scorciatoie sotto un modale ─────────────────────────────────────────

test("con un modale aperto passano solo zoom e schermo intero", () => {
  for (const comando of ["ingrandisci", "rimpicciolisci", "zoomNormale", "schermoIntero"]) {
    assert.equal(passaLaScorciatoia(comando, true), true, comando);
  }
  for (const comando of [
    "alterna",
    "cerca",
    "avanti",
    "indietro",
    "inRiproduzione",
    "importazioni",
    "incollaLink",
  ]) {
    assert.equal(passaLaScorciatoia(comando, true), false, comando);
  }
});

test("senza modali passano tutte", () => {
  for (const comando of ["incollaLink", "alterna", "ingrandisci"]) {
    assert.equal(passaLaScorciatoia(comando, false), true, comando);
  }
});

test("il conto dei modali regge una chiusura chiamata due volte", () => {
  assert.equal(modaleAperto(), false);
  const chiudiUno = apriModale();
  const chiudiDue = apriModale();
  assert.equal(modaleAperto(), true);
  chiudiUno();
  chiudiUno();
  assert.equal(modaleAperto(), true, "il secondo è ancora aperto");
  chiudiDue();
  assert.equal(modaleAperto(), false);
});

// ── chi diventa inerte ─────────────────────────────────────────────────────

/** Un nodo finto, abbastanza per salire l'albero. */
interface Finto extends NodoDiAlbero {
  readonly nome: string;
  parentElement: Finto | null;
  children: Finto[];
}

function nodo(nome: string, tagName = "DIV", ...figli: Finto[]): Finto {
  const n: Finto = { nome, tagName, parentElement: null, children: figli };
  for (const figlio of figli) figlio.parentElement = n;
  return n;
}

test("diventano inerti i fratelli di ogni gradino fra il velo e la cima", () => {
  // html > (head, body > (#root > (app > (pagina, velo, menu), studio), script))
  const velo = nodo("velo");
  const pagina = nodo("pagina");
  const menu = nodo("menu");
  const app = nodo("app", "DIV", pagina, velo, menu);
  const studio = nodo("studio");
  const root = nodo("root", "DIV", app, studio);
  const script = nodo("script", "SCRIPT");
  const body = nodo("body", "BODY", root, script);
  const head = nodo("head", "HEAD");
  nodo("html", "HTML", head, body);

  const inerti = daRendereInerti(velo).map((n) => n.nome);
  assert.deepEqual(inerti, ["pagina", "menu", "studio"]);
});

test("un velo dentro una pagina rende inerte anche il resto della pagina", () => {
  // La conferma di Cartelle nasce dentro la schermata, non in fondo ad `App`.
  const velo = nodo("velo");
  const albero = nodo("albero");
  const cartelle = nodo("cartelle", "DIV", albero, velo);
  const fascia = nodo("fascia");
  const dentro = nodo("dentro", "DIV", fascia, cartelle);
  const barra = nodo("barra");
  nodo("app", "DIV", barra, dentro);

  const inerti = daRendereInerti(velo).map((n) => n.nome);
  assert.deepEqual(inerti, ["albero", "fascia", "barra"]);
  assert.ok(!inerti.includes("dentro"), "un antenato del velo non si spegne");
});
