import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import {
  SCHEDA_FERMA,
  fuocoDiRitorno,
  fuocoSullaTestata,
  type Contenitore,
  type Fuocabile,
} from "../src/fuocoPagina";

/*
 * Il fuoco aprendo e chiudendo un album, un artista, «In riproduzione»
 * (R16-1, D-73): prima restava sul `<body>`.
 */

/** Un documento finto: chi ha il fuoco, e nodi che lo prendono se possono. */
function documento() {
  let attivo: unknown = "body";
  const nodo = (nome: string, montato = true, visibile = true) => {
    const attributi = new Set<string>();
    const n: Fuocabile & { nome: string } = {
      nome,
      isConnected: montato,
      tabIndex: 0,
      hasAttribute: (a) => attributi.has(a),
      focus: () => {
        if (montato && visibile) attivo = n;
      },
    };
    return n;
  };
  return { nodo, attivo: () => attivo };
}

function pagina(nodi: Record<string, Fuocabile | null>): Contenitore {
  return { querySelector: (selettore) => nodi[selettore] ?? null };
}

test("aprendo un album il fuoco va sulla testata, che non diventa una fermata del Tab", () => {
  const d = documento();
  const h1 = d.nodo("titolo del disco");
  assert.equal(fuocoSullaTestata(pagina({ h1 }), d.attivo), true);
  assert.equal(d.attivo(), h1);
  assert.equal(h1.tabIndex, -1);
});

test("chiudendo torna a chi aveva aperto, se c'è ancora", () => {
  const d = documento();
  const scheda = d.nodo("scheda premuta");
  const h1 = d.nodo("Album");
  fuocoDiRitorno([scheda, d.nodo("ferma")], pagina({ h1 }), d.attivo);
  assert.equal(d.attivo(), scheda);
});

test("se chi aveva aperto è sparito, la scheda dove la griglia si era fermata, poi la testata", () => {
  const d = documento();
  const sparita = d.nodo("scheda smontata", false);
  const ferma = d.nodo("scheda ferma");
  const h1 = d.nodo("Album");
  fuocoDiRitorno([sparita, ferma], pagina({ h1 }), d.attivo);
  assert.equal(d.attivo(), ferma);

  const e = documento();
  const nascosta = e.nodo("montata ma nascosta", true, false);
  const testata = e.nodo("Album");
  fuocoDiRitorno([nascosta, null], pagina({ h1: testata }), e.attivo);
  assert.equal(e.attivo(), testata, "un nodo che non prende il fuoco non conta");
});

test("il cablaggio: App chiede il fuoco aprendo e chiudendo, «In riproduzione» ritrova il tasto rinato", () => {
  const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
  for (const apri of ["const apriAlbum = useCallback(", "const apriArtista = useCallback("]) {
    const corpo = app.slice(app.indexOf(apri), app.indexOf("}, []);", app.indexOf(apri)));
    assert.match(corpo, /verso: "testata"/, apri);
    assert.match(corpo, /verso: "ritorno"/, apri);
  }
  // La scala di Escape chiede il ritorno a ogni livello che chiude.
  const scala = app.slice(app.indexOf("if (aperto) {"), app.indexOf("if (MOBILE) {", app.indexOf("if (aperto) {")));
  assert.equal([...scala.matchAll(/verso: "ritorno"/g)].length, 2);
  assert.match(app, /fuocoDiRitorno\(\[chi, pagina\.querySelector\(SCHEDA_FERMA\)\], pagina, attivo\)/);
  assert.equal(SCHEDA_FERMA, '.scheda[tabindex="0"]');

  const colonna = readFileSync(new URL("../src/parti/Colonna.tsx", import.meta.url), "utf8");
  assert.equal([...colonna.matchAll(/data-apri-grande="/g)].length, 2);
  const grande = readFileSync(new URL("../src/schermate/InRiproduzione.tsx", import.meta.url), "utf8");
  assert.match(grande, /fuocoDiRitorno\(\[chiAveva, rinato\]/);
});
