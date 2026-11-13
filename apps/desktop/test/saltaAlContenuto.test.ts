import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { saltaAlContenuto } from "../src/saltaAlContenuto";

/*
 * R16-10, la seconda metà: il posto del contenuto è già un `main` (F12b), e
 * mancava la strada per arrivarci da tastiera senza passare da tutte le voci
 * della navigazione — una per playlist, oltre cinquanta Tab con quaranta
 * playlist. Un pulsante e non un `<a href="#…">`: la guardia di navigazione
 * della finestra (`src-tauri/src/pagina.rs`) non deve vedere niente.
 */
const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

/** Un `main` finto: gli attributi, chi ascolta il `blur`, e quante volte ha preso il fuoco. */
function unMain(attributi: Record<string, string> = {}) {
  const lasciato: (() => void)[] = [];
  const posto = {
    attributi,
    fuochi: 0,
    hasAttribute: (nome: string) => nome in attributi,
    setAttribute: (nome: string, valore: string) => {
      attributi[nome] = valore;
    },
    removeAttribute: (nome: string) => {
      delete attributi[nome];
    },
    addEventListener: (_evento: "blur", fai: () => void) => {
      lasciato.push(fai);
    },
    focus: () => {
      posto.fuochi += 1;
    },
    lascia: () => {
      for (const fai of lasciato.splice(0)) fai();
    },
  };
  return posto;
}

test("il salto porta il fuoco sul main, e senza un main non fa niente", () => {
  const main = unMain();
  const chiesti: string[] = [];
  const pagina = {
    querySelector: (selettore: "main") => {
      chiesti.push(selettore);
      return main;
    },
  };
  assert.equal(saltaAlContenuto(pagina), true);
  assert.deepEqual(chiesti, ["main"]);
  assert.equal(main.fuochi, 1);
  // Il `main` prende il fuoco dal programma per il salto, non dal Tab…
  assert.equal(main.attributi["tabindex"], "-1");
  // …e non dal clic dopo: il `tabindex` se ne va col fuoco. Fisso, un clic su
  // un punto vuoto della pagina toglierebbe il fuoco dal `<body>`, che più di
  // un posto dell'app legge come «nessuno ha il fuoco».
  main.lascia();
  assert.equal("tabindex" in main.attributi, false);
  // Una skin senza il posto del contenuto: nessun `main`, nessun salto.
  assert.equal(saltaAlContenuto({ querySelector: () => null }), false);
});

test("un tabindex che c'era già resta com'era", () => {
  const main = unMain({ tabindex: "0" });
  assert.equal(saltaAlContenuto({ querySelector: () => main }), true);
  main.lascia();
  assert.equal(main.attributi["tabindex"], "0");
});

test("il salto è il primo comando della finestra, e solo sul computer", () => {
  const app = sorgente("App.tsx");
  const apertura = '<div className="finestra-app" inert={studioVisibile}>';
  const dopo = app.slice(app.indexOf(apertura) + apertura.length);
  // Il primo elemento dopo l'apertura, saltati i commenti JSX.
  const senzaCommenti = dopo.replace(/\{\/\*[\s\S]*?\*\/\}/g, "").trimStart();
  assert.ok(
    senzaCommenti.startsWith("{!MOBILE && <SaltaAlContenuto />}"),
    senzaCommenti.slice(0, 80),
  );
  // Prima di `App` nel documento c'è solo `Simboli`, che non prende il fuoco;
  // i comandi della finestra vengono dopo.
  const main = sorgente("main.tsx");
  assert.ok(main.indexOf("<App />") < main.indexOf("<BarraTitolo />"));
  const salto = sorgente("parti/SaltaAlContenuto.tsx");
  assert.match(salto, /<button\s+type="button"\s+className="salta-al-contenuto"/);
  assert.match(salto, /saltaAlContenuto\(document\)/);
  assert.doesNotMatch(salto, /href=/);
});

test("il salto si vede solo col fuoco, su uno strato della scala, e in tutte e due le lingue", () => {
  const foglio = sorgente("stile/navigazione.css");
  const regola = foglio.slice(foglio.indexOf(".salta-al-contenuto {"));
  assert.match(regola.slice(0, regola.indexOf("}")), /z-index: var\(--strato-salto\);/);
  assert.match(foglio, /\.salta-al-contenuto:not\(:focus\) \{[^}]*clip-path: inset\(50%\);/);
  assert.match(sorgente("stile.css"), /--strato-salto: \d+;/);
  // L'anello attorno al contenuto intero direbbe soltanto che il salto è avvenuto.
  assert.match(sorgente("stile.css"), /main\.posto-content:focus-visible/);
  assert.match(sorgente("stile/contrasto.css"), /main\.posto-content:focus-visible/);
  const it = JSON.parse(sorgente("lingue/it.json")) as Record<string, string>;
  const en = JSON.parse(sorgente("lingue/en.json")) as Record<string, string>;
  assert.equal(it["nav.skipToContent"], "Salta al contenuto");
  assert.equal(en["nav.skipToContent"], "Skip to content");
});
