import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { eTastoDelBrowser, menuDelSistemaAmmesso } from "../src/browserSpento";

/*
 * Il menù contestuale e le scorciatoie del browser di WebView2 (R10-4, D-43).
 *
 * wry li lascia accesi, Tauri non li espone, e spegnerli dal nucleo vorrebbe
 * dire una chiamata COM `unsafe`, che il workspace vieta: li spegne la pagina.
 * Un clic destro sullo sfondo apriva il menù di Edge — Indietro, Aggiorna,
 * Salva con nome, Stampa — e F5 ricaricava tutta l'interfaccia, perdendo i
 * dialoghi aperti e il testo non salvato dello Studio.
 */

const tasto = (
  key: string,
  modificatori: Partial<{ ctrlKey: boolean; shiftKey: boolean; altKey: boolean; code: string }> = {},
) => ({
  key,
  code: modificatori.code ?? "",
  ctrlKey: false,
  shiftKey: false,
  altKey: false,
  metaKey: false,
  ...modificatori,
});

test("i tasti del browser non arrivano al browser", () => {
  const delBrowser = [
    tasto("F5"),
    tasto("F5", { ctrlKey: true }),
    tasto("F5", { shiftKey: true }),
    tasto("r", { ctrlKey: true }),
    tasto("R", { ctrlKey: true, shiftKey: true }),
    tasto("p", { ctrlKey: true }),
    tasto("s", { ctrlKey: true }),
    tasto("S", { ctrlKey: true, shiftKey: true }),
    tasto("f", { ctrlKey: true }),
    tasto("g", { ctrlKey: true }),
    tasto("G", { ctrlKey: true, shiftKey: true }),
    tasto("u", { ctrlKey: true }),
    tasto("F3"),
    tasto("F3", { shiftKey: true }),
    tasto("F7"),
    tasto("ArrowLeft", { altKey: true }),
    tasto("ArrowRight", { altKey: true }),
    tasto("BrowserBack"),
    tasto("BrowserForward"),
    tasto("BrowserRefresh"),
    tasto("BrowserSearch"),
    // `Ctrl+F` su una tastiera russa: il carattere è un altro, il tasto è lo
    // stesso, e il browser guarda il tasto.
    tasto("а", { ctrlKey: true, code: "KeyF" }),
  ];
  for (const t of delBrowser) assert.ok(eTastoDelBrowser(t), JSON.stringify(t));
});

test("i tasti che scrivono, copiano, ingrandiscono o aprono gli strumenti restano a chi li usa", () => {
  const nostri = [
    tasto("f"),
    tasto("r"),
    tasto("F11"),
    // Gli strumenti di sviluppo: in rilascio non ci sono, in sviluppo servono.
    tasto("F12"),
    tasto("I", { ctrlKey: true, shiftKey: true }),
    tasto("c", { ctrlKey: true }),
    tasto("v", { ctrlKey: true }),
    tasto("x", { ctrlKey: true }),
    tasto("a", { ctrlKey: true }),
    tasto("z", { ctrlKey: true }),
    // Lo zoom è dell'app (`zoom.rs`), e WebView2 lo ha già spento.
    tasto("+", { ctrlKey: true }),
    tasto("-", { ctrlKey: true }),
    tasto("0", { ctrlKey: true }),
    tasto("ArrowLeft", { ctrlKey: true }),
    tasto(" "),
    tasto("Escape"),
  ];
  for (const t of nostri) assert.ok(!eTastoDelBrowser(t), JSON.stringify(t));
});

test("il menù del sistema si apre solo nei campi di testo", () => {
  const campo = (tagName: string, type?: string) => ({ tagName, type, isContentEditable: false });
  for (const sì of [
    campo("INPUT", "text"),
    campo("INPUT", "search"),
    campo("INPUT", "url"),
    campo("INPUT", "password"),
    campo("INPUT", "number"),
    campo("INPUT", ""),
    campo("TEXTAREA"),
    { tagName: "DIV", isContentEditable: true },
  ]) {
    assert.ok(menuDelSistemaAmmesso(sì), JSON.stringify(sì));
  }
  for (const no of [
    null,
    campo("DIV"),
    campo("BUTTON"),
    campo("IMG"),
    campo("svg"),
    campo("use"),
    campo("INPUT", "range"),
    campo("INPUT", "checkbox"),
    campo("INPUT", "radio"),
    campo("SELECT"),
  ]) {
    assert.ok(!menuDelSistemaAmmesso(no), JSON.stringify(no));
  }
});

test("la finestra del computer spegne il browser prima di disegnare, il telefono no", () => {
  const main = readFileSync(new URL("../src/main.tsx", import.meta.url), "utf8");
  assert.match(main, /import \{ spegniIlBrowser \} from "\.\/browserSpento";/);
  // Sul telefono non c'è il menù di Edge, e `vibra.ts` vibra a ogni
  // `contextmenu` preso: spegnerlo lì farebbe vibrare ogni tocco lungo.
  assert.match(main, /if \(!MOBILE\) spegniIlBrowser\(window\);/);
  const modulo = readFileSync(new URL("../src/browserSpento.ts", import.meta.url), "utf8");
  // Il menù in cattura, prima di chiunque.
  assert.match(modulo, /addEventListener\(\s*"contextmenu",[\s\S]*?\},\s*true,?\s*\)/);
  // I tasti in risalita: l'editor di codice dello Studio (CodeMirror) ignora
  // un tasto già preso, e Ctrl+F, F3, Ctrl+G lì dentro sono la sua ricerca.
  const tasti = /addEventListener\("keydown", \(e\) => \{[\s\S]*?\}(,\s*true)?\);/.exec(modulo);
  assert.ok(tasti, "l'ascolto dei tasti non c'è");
  assert.equal(tasti[1], undefined, "i tasti in cattura tolgono all'editor dello Studio la sua ricerca");
});

test("l'editor di codice dello Studio ignora davvero un tasto già preso", () => {
  // La ragione dei tasti in risalita sta in CodeMirror: se un giorno smette di
  // guardare `defaultPrevented`, la scelta si può rivedere.
  const vista = readFileSync(
    new URL("../node_modules/@codemirror/view/dist/index.js", import.meta.url),
    "utf8",
  );
  assert.match(vista, /if \(event\.defaultPrevented\)\s*return false;/);
});
