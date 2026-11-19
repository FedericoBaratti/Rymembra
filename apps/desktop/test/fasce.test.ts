import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * R18-17: le fasce di notizia e d'errore entravano nel flusso della pagina e
 * la spingevano giù — 77 px di titolo, indice e schede dopo «Scansiona», il
 * filtro e l'albero delle Cartelle sotto la fascia rossa, i tasti del primo
 * avvio spostati di 60 px mentre si stava per premere «Ascolta». Adesso stanno
 * nella pila dei toast, che galleggia sopra il lettore — in basso a destra sul
 * computer, a tutta larghezza sul telefono —; nel primo avvio galleggiano sopra
 * la scheda.
 *
 * Sul telefono la fascia in cima alla pagina era la forma di sempre, e nel
 * collaudo del 18/11 (D7) copriva la barra di stato nelle pagine d'album — la
 * tinta sale fin sotto di lei e il contenuto parte dal bordo dello schermo — e
 * nelle liste spingeva giù il contenuto. Non c'è più: una pila sola, che
 * rispetta il riquadro di sotto come le altre notifiche.
 */
const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

test("le fasce stanno nella pila dei toast, sul computer e sul telefono, non nel flusso della pagina", () => {
  const app = sorgente("App.tsx");
  // Nella pagina mai, nemmeno sul telefono (D7).
  assert.doesNotMatch(app, /\{!primoVisibile && MOBILE && notizieDiFascia\}/);
  assert.doesNotMatch(app, /\{!primoVisibile && notizieDiFascia\}/);
  // Nella pila, e non quando c'è già un'altra copia viva: il primo avvio le
  // mostra da sé, e così lo schermo intero (`avvisiGrande`).
  const pila = app.slice(app.indexOf("<PilaToast"), app.indexOf("</PilaToast>"));
  assert.match(pila, /\{!primoVisibile && !\(grande && conBrano\) && notizieDiFascia\}/);
  assert.doesNotMatch(pila, /\{!MOBILE && !primoVisibile/);
});

test("sul telefono la fascia nella pila riempie la pila invece di restare larga 360 px", () => {
  const foglio = sorgente("stile/telefono.css");
  const regola = foglio.slice(foglio.indexOf(':root[data-forma="telefono"] .pila-toast > .notizia,'));
  const corpo = regola.slice(0, regola.indexOf("}"));
  assert.match(corpo, /:root\[data-forma="telefono"\] \.pila-toast > \.errore \{/);
  assert.match(corpo, /width: auto;/);
  assert.match(corpo, /max-width: none;/);
});

test("nella pila le fasce sono toast: piene, ombreggiate e cliccabili", () => {
  const foglio = sorgente("stile/artisti-cartelle.css");
  const regola = foglio.slice(foglio.indexOf(".pila-toast > .notizia,"));
  const corpo = regola.slice(0, regola.indexOf("}"));
  assert.match(corpo, /\.pila-toast > \.errore \{/);
  assert.match(corpo, /pointer-events: auto;/);
  assert.match(corpo, /margin-bottom: 0;/);
  assert.match(foglio, /\.pila-toast > \.errore \{[^}]*background: linear-gradient\(var\(--danger-soft\), var\(--danger-soft\)\) var\(--color-surface-1\);/);
});

test("nel primo avvio le fasce galleggiano sopra la scheda e non spostano i tasti", () => {
  const foglio = sorgente("stile/libreria.css");
  const regola = foglio.slice(foglio.indexOf(':root[data-forma="computer"] .primo-avvisi {'));
  const corpo = regola.slice(0, regola.indexOf("}"));
  assert.match(corpo, /position: absolute;/);
  // Sotto la fascia del titolo, da cui si trascina la finestra.
  assert.match(corpo, /top: calc\(var\(--barra-titolo-h\) \+ var\(--spazio-2\)\);/);
  // Il contenitore è `.primo` (fisso), e la scheda in mezzo non deve esserlo.
  const scheda = foglio.slice(foglio.indexOf(".primo-scheda {"));
  assert.doesNotMatch(scheda.slice(0, scheda.indexOf("}")), /position:/);
  assert.match(sorgente("Primo.tsx"), /<div className="primo-avvisi">\{avvisi\}<\/div>/);
});
