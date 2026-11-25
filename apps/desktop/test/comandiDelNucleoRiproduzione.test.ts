import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * I comandi che la finestra chiede al nucleo per la pagina dell'artista, per
 * «Riproduci» / «Casuale» / «Scarica qui» e per la coda del lettore (S9-3, S9-8,
 * S9-9, S7-5): quel che `ipc.ts` chiama deve essere registrato in `lib.rs`.
 */

const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");

test("i comandi dell'artista e della coda esistono da tutte e due le parti", () => {
  const ipc = leggi("../src/ipc.ts");
  const lib = leggi("../src-tauri/src/lib.rs");
  const comandi = leggi("../src-tauri/src/comandi.rs");
  const riproduzione = leggi("../src-tauri/src/riproduzione/mod.rs");
  for (const [comando, registrazione, definizione, dove] of [
    ["artista", "comandi::artista,", "pub async fn artista(", comandi],
    ["artista_righe", "comandi::artista_righe,", "pub async fn artista_righe(", comandi],
    ["righe_scelte", "comandi::righe_scelte,", "pub async fn righe_scelte(", comandi],
    ["riproduzione_coda", "riproduzione::riproduzione_coda,", "pub async fn riproduzione_coda(", riproduzione],
  ] as const) {
    assert.ok(ipc.includes(`("${comando}"`), `ipc.ts non chiama ${comando}`);
    assert.ok(lib.includes(registrazione), `lib.rs non registra ${comando}`);
    assert.ok(dove.includes(`#[tauri::command]\n${definizione}`), `il nucleo non definisce ${comando}`);
  }
});

test("S9-8: la pagina dell'artista non rilegge più gli elenchi a pagine per suonare o scaricare", () => {
  const app = leggi("../src/App.tsx");
  assert.doesNotMatch(app, /tutteLePagine/);
  assert.doesNotMatch(app, /braniDellArtista/);
});
