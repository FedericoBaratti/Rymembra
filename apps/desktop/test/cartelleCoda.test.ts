import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { vociCartella } from "../src/menuBrani";

const t = (chiave: string) => chiave;
const fai = { riproduci: () => undefined, dopo: () => undefined, accoda: () => undefined };

test("pannello Cartelle: senza la coda il menù non offre «Riproduci», «Dopo» né «In coda»", () => {
  assert.deepEqual(
    vociCartella(fai, t).map((v) => v.etichetta),
    ["action.play", "action.playNext", "action.enqueue"],
  );
  // In una sessione con diritto «Ascolta»: niente gesti sulla coda che il
  // nucleo rifiuta, come in `vociBrani` — e far suonare è uno di loro
  // (R13b-9).
  assert.deepEqual(vociCartella(fai, t, true).map((v) => v.etichetta), []);
});

test("pannello Cartelle: la striscia e il trascinamento guardano il permesso della coda", () => {
  const cartelle = readFileSync(new URL("../src/schermate/Cartelle.tsx", import.meta.url), "utf8");
  assert.match(cartelle, /useLettore\(\(s\) => gestoPermesso\(s\.insieme, "coda"\)\)/);
  assert.match(cartelle, /vociCartella\(/);
  // La striscia della selezione: «Riproduci» e «In coda» spenti con la
  // ragione (R13b-9: far suonare è un gesto di coda).
  const striscia = cartelle.slice(cartelle.indexOf('className="scelta-cartelle"'), cartelle.indexOf("folders.selection.clear"));
  assert.equal([...striscia.matchAll(/disabled=\{!codaLibera\}/g)].length, 2);
  assert.equal([...striscia.matchAll(/title=\{spentoInSessione\}/g)].length, 2);
  // Il trascinamento verso la coda non parte.
  assert.match(cartelle, /if \(!codaLiberaOra\.current\) return;/);
});

test("pannello Cartelle: senza la coda nemmeno la pillola, Invio e il doppio clic fanno suonare", () => {
  const cartelle = readFileSync(new URL("../src/schermate/Cartelle.tsx", import.meta.url), "utf8");
  const invio = cartelle.slice(cartelle.indexOf("const invioIndice"), cartelle.indexOf("const scegli = useCallback"));
  assert.match(invio, /if \(!codaLiberaOra\.current\) return;/);
  const pillola = cartelle.slice(cartelle.indexOf('className="pillola btn-accent"') - 50, cartelle.indexOf("onClick={suonaTuttoIlPannello}"));
  assert.match(pillola, /!codaLibera/);
  assert.match(cartelle, /suonabile=\{\(mani\?\.suonabile \?\? true\) && codaLibera && !pannelloMuto\}/);
});

// ── R18-11: una radice che non risponde ────────────────────────────────────

test("pannello Cartelle: sotto una radice muta «Riproduci» è spento e dice perché", () => {
  const cartelle = readFileSync(new URL("../src/schermate/Cartelle.tsx", import.meta.url), "utf8");
  assert.match(cartelle, /const pannelloMuto =\s*cartellaScelta !== null && radiceSpenta\(cartellaScelta, radici, vive\)/);
  const pillola = cartelle.slice(cartelle.indexOf('className="pillola btn-accent"') - 50, cartelle.indexOf("onClick={suonaTuttoIlPannello}"));
  assert.match(pillola, /pannelloMuto/);
  assert.match(pillola, /folders\.blocked/);
});

// ── R13b-12: nello Studio niente disco vero ────────────────────────────────

test("pannello Cartelle: nello Studio né la sonda né il ricordo vanno al nucleo", () => {
  const cartelle = readFileSync(new URL("../src/schermate/Cartelle.tsx", import.meta.url), "utf8");
  const sonda = cartelle.slice(cartelle.indexOf("const sonda = useCallback"), cartelle.indexOf(".cartelleRadiciVive"));
  assert.match(sonda, /anteprima\) return;/);
  const ricordo = cartelle.slice(cartelle.indexOf("sonda(false);"), cartelle.indexOf(".cartelleUi()"));
  assert.match(ricordo, /if \(anteprima\)/);
});

// ── R13b-3: il menù del pannello conosce le sue righe ──────────────────────

test("pannello Cartelle: il menù dei brani passa ad App le righe del pannello", () => {
  const cartelle = readFileSync(new URL("../src/schermate/Cartelle.tsx", import.meta.url), "utf8");
  const menu = cartelle.slice(cartelle.indexOf("const menuBrani = useCallback"), cartelle.indexOf("const preferito = useCallback"));
  assert.match(menu, /archivio\.cerca\(/);
  const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
  assert.match(app, /typeof conosciuto === "function"/);
});

// ── R13b-2: togliere dei brani rifà l'albero e il pannello ─────────────────

test("pannello Cartelle: una fetta più corta fa rileggere gli identificativi", () => {
  const cartelle = readFileSync(new URL("../src/schermate/Cartelle.tsx", import.meta.url), "utf8");
  const chiedi = cartelle.slice(cartelle.indexOf("const archivio = useRada<Brano>"), cartelle.indexOf("onErrore,\n  });", cartelle.indexOf("const archivio = useRada<Brano>")));
  assert.match(chiedi, /righe\.length < pezzo\.length/);
  assert.match(chiedi, /setGiroIds\(/);
  assert.match(cartelle, /archivio\.rinnova\(ids\.length\)/);
});

test("Togli dalla libreria ed Elimina dal disco rifanno l'indice delle cartelle", () => {
  const nucleo = readFileSync(new URL("../src-tauri/src/cancellazione.rs", import.meta.url), "utf8");
  for (const comando of ["pub async fn brani_togli", "pub async fn brani_elimina"]) {
    const inizio = nucleo.indexOf(comando);
    assert.ok(inizio > 0, comando);
    const corpo = nucleo.slice(inizio, nucleo.indexOf("\n}\n", inizio));
    assert.match(corpo, /if fatto\.tolti > 0 \{\s*crate::cartelle::cambiate\(&mano\);/, comando);
  }
});
