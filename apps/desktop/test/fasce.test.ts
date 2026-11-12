import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * R18-17: le fasce di notizia e d'errore entravano nel flusso della pagina e
 * la spingevano giù — 77 px di titolo, indice e schede dopo «Scansiona», il
 * filtro e l'albero delle Cartelle sotto la fascia rossa, i tasti del primo
 * avvio spostati di 60 px mentre si stava per premere «Ascolta». Sul computer
 * adesso stanno nella pila dei toast, che galleggia in basso a destra sopra il
 * lettore; nel primo avvio galleggiano sopra la scheda. Il telefono resta
 * com'era: la sua pila è larga quanto lo schermo, e lì la fascia in cima alla
 * pagina è la forma di sempre.
 */
const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

test("sul computer le fasce stanno nella pila dei toast, non nel flusso della pagina", () => {
  const app = sorgente("App.tsx");
  // Nella pagina solo sul telefono.
  assert.match(app, /\{!primoVisibile && MOBILE && notizieDiFascia\}/);
  assert.doesNotMatch(app, /\{!primoVisibile && notizieDiFascia\}/);
  // Nella pila sul computer, e non quando c'è già un'altra copia viva: il
  // primo avvio le mostra da sé, e così lo schermo intero (`avvisiGrande`).
  const pila = app.slice(app.indexOf("<PilaToast"), app.indexOf("</PilaToast>"));
  assert.match(pila, /\{!MOBILE && !primoVisibile && !\(grande && conBrano\) && notizieDiFascia\}/);
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
