/**
 * La WebView del telefono tiene chiuse le porte che la pagina non usa.
 *
 * Uno script che girasse nella pagina — un metadato, un testo, una skin — con
 * le impostazioni di serie poteva leggere un indirizzo `content://`, chiedere
 * la posizione e aprire finestre da sé; e i permessi che chiedeva (la
 * fotocamera, già concessa per il QR) Android glieli dava senza dialogo
 * (R11-3). I permessi li nega Rust (`telefono::crea_la_finestra`, provato in
 * `telefono.rs`); qui le impostazioni di `MainActivity`.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const attivita = readFileSync(
  new URL(
    "../src-tauri/gen/android/app/src/main/java/io/github/federicobaratti/rymembra/MainActivity.kt",
    import.meta.url,
  ),
  "utf8",
).replace(/\/\/.*$/gm, "");

test("la WebView del telefono non apre content://, la posizione né finestre da sé", () => {
  const inizio = attivita.indexOf("override fun onWebViewCreate(");
  assert.ok(inizio >= 0, "onWebViewCreate non c'è");
  const corpo = attivita.slice(inizio, attivita.indexOf("override fun onCreate(", inizio));
  assert.match(corpo, /settings\.allowContentAccess = false/);
  assert.match(corpo, /settings\.setGeolocationEnabled\(false\)/);
  assert.match(corpo, /settings\.javaScriptCanOpenWindowsAutomatically = false/);
});
