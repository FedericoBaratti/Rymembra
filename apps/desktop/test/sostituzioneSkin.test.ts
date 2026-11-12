import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * Una skin con lo stesso id di una installata non si sostituisce senza
 * chiedere, da nessuna delle due strade: il selettore e il trascinamento.
 *
 * La regola sta nel nucleo (`skin.rs::installa_file`, con la sua prova): senza
 * `sostituisci` non scrive niente e risponde con le due skin. Qui si prova il
 * cablaggio — che tutte e due le strade passino da `installaDaFile`, e che il sì
 * arrivi al nucleo solo da una risposta — leggendo il sorgente, come fanno
 * `finestrelle.test.ts` e `insiemeGesti.test.ts`.
 */
const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

test("il trascinamento e il selettore passano tutti e due da installaDaFile", () => {
  const app = sorgente("App.tsx");
  const trascinato = app.slice(app.indexOf('piano.tipo === "skin"'), app.indexOf('piano.tipo === "playlist"'));
  assert.match(trascinato, /skinFinestra\.installaDaFile\(piano\.percorso\)/);
  assert.doesNotMatch(trascinato, /skinInstalla/);

  const skin = sorgente("skinAttiva.ts");
  const selettore = skin.slice(skin.indexOf("const installaSkin = useCallback"));
  assert.match(selettore, /await installaDaFile\(scelta\)/);
});

test("il sì alla sostituzione arriva al nucleo solo dalla risposta o da «Rimettila»", () => {
  const skin = sorgente("skinAttiva.ts");
  const chiamate = [...skin.matchAll(/ipc\.skinInstalla\(([^)]*)\)/g)].map((m) => m[1]);
  // Una senza il sì (la prima installazione) e due col sì: la conferma della
  // domanda e la copia tenuta da parte che torna al suo posto. Tutte e due col
  // sì portano l'impronta della skin che il gesto vuole togliere (R04b-4,
  // R04b-5).
  assert.deepEqual(chiamate, [
    "percorso",
    "quale.percorso, true, quale.domanda.impronta",
    "percorso, true, impronta",
  ]);
});

test("la domanda è un consenso, e il fuoco parte da «Annulla»", () => {
  const conferma = sorgente("ConfermaSkin.tsx");
  const domanda = conferma.slice(conferma.indexOf("export function ConfermaSostituzione"));
  const dialogo = domanda.slice(domanda.indexOf("<Dialogo"), domanda.indexOf(">\n", domanda.indexOf("<Dialogo")));
  assert.match(dialogo, /\bconsenso\b/);
  assert.match(dialogo, /ruolo="alertdialog"/);
  assert.match(domanda, /className="bottone primario"\s+onClick=\{chiudi\}\s+data-fuoco-iniziale/);
});
