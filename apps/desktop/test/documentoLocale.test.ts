import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { conCopiaLocale } from "../src/documentoLocale";

/*
 * D-10d: la licenza, gli avvisi delle terze parti, la privacy e le condizioni
 * si leggono anche senza rete. La copia viaggia con l'app — accanto
 * all'eseguibile sul computer, in `assets/legale/` nell'APK — e la finestra la
 * mostra in una finestrella; il sito resta a un tasto. Gli altri documenti
 * (il repository, le segnalazioni) non ne hanno una, e vanno al browser come
 * prima.
 */
const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

test("i quattro testi legali passano dalla copia locale, gli altri no", () => {
  assert.deepEqual(
    (["repository", "segnalazioni", "licenza", "terze", "privacy", "condizioni"] as const).filter(conCopiaLocale),
    ["licenza", "terze", "privacy", "condizioni"],
  );
});

test("le Impostazioni chiedono la copia locale prima di aprire il sito", () => {
  const aggiornamenti = sorgente("parti/Aggiornamenti.tsx");
  const apri = aggiornamenti.slice(aggiornamenti.indexOf("const apriDocumento = useCallback("));
  const corpo = apri.slice(0, apri.indexOf("[onErrore]"));
  assert.match(corpo, /if \(!conCopiaLocale\(quale\)\)/);
  assert.match(corpo, /ipc\s*\.documentoLocale\(quale\)/);
  // Senza copia, il sito come prima.
  assert.match(corpo, /testo === null \? ipc\.apriDocumento\(quale\)/);
  assert.match(aggiornamenti, /<DocumentoLocale/);
  const vista = sorgente("parti/DocumentoLocale.tsx");
  assert.match(vista, /<Dialogo/);
  assert.match(vista, /<pre className="testo-documento"/);
  assert.match(vista, /t\("settings\.docs\.online"\)/);
  const ipc = sorgente("ipc.ts");
  assert.match(ipc, /documentoLocale: \(quale: "licenza" \| "terze" \| "privacy" \| "condizioni"\) =>\s+invoke<string \| null>\("documento_locale", \{ quale \}\)/);
  const it = JSON.parse(sorgente("lingue/it.json")) as Record<string, string>;
  const en = JSON.parse(sorgente("lingue/en.json")) as Record<string, string>;
  assert.equal(it["settings.docs.online"], "Apri la versione aggiornata sul sito");
  assert.equal(en["settings.docs.online"], "Open the latest version online");
});

test("il testo si legge e si copia, e la finestrella è larga", () => {
  const foglio = sorgente("stile/finestrelle.css");
  const regola = foglio.slice(foglio.indexOf(".documento-locale .testo-documento {"));
  const corpo = regola.slice(0, regola.indexOf("}"));
  assert.match(corpo, /white-space: pre-wrap;/);
  assert.match(corpo, /user-select: text;/);
  assert.match(corpo, /overflow-y: auto;/);
  assert.match(sorgente("parti/DocumentoLocale.tsx"), /classe="finestrella larga documento-locale glass-modal"/);
});
