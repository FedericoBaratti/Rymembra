/**
 * Quando l'auto si stacca Rymembra si ferma (D-T2 del collaudo sul telefono vero,
 * FC1).
 *
 * Android, perso il flusso verso l'auto, ripiega sull'altoparlante e Rymembra,
 * che riapre da sola l'uscita e «riparte come stava», continuava a suonare dal
 * telefono mentre Spotify andava in pausa. La regola sta in `riapertura.rs`, con
 * le sue prove sulla cucitura (`riparte_fermo_dopo_il_cambio`); qui si guarda che
 * il dato che le serve — se l'uscita è dentro il telefono — arrivi fin lì da
 * Android.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const leggi = (percorso: string) =>
  readFileSync(new URL(percorso, import.meta.url), "utf8").replace(/\r\n/g, "\n");
const senzaCommenti = (testo: string) => testo.replace(/\/\*[\s\S]*?\*\/|\/\/.*$/gm, "");

const KOTLIN = "../src-tauri/gen/android/app/src/main/java/io/github/federicobaratti/rymembra";

test("Android dice se l'uscita è dentro il telefono: altoparlante e auricolare", () => {
  const uscita = senzaCommenti(leggi(`${KOTLIN}/Uscita.kt`));
  assert.match(uscita, /TYPE_BUILTIN_SPEAKER,\s*AudioDeviceInfo\.TYPE_BUILTIN_EARPIECE/);
  assert.match(uscita, /\.put\("integrata", uscita\.type in INTEGRATE\)/);
});

test("il dato attraversa il ponte, il motore e arriva alla riapertura", () => {
  const sistema = senzaCommenti(leggi("../rymembra-android/src/sistema.rs"));
  assert.match(sistema, /#\[serde\(default\)\]\s*pub integrata: bool,/);
  const telefono = senzaCommenti(leggi("../src-tauri/src/telefono.rs"));
  assert.match(telefono, /integrata: uscita\.integrata,/);
  const riapertura = senzaCommenti(leggi("../src-tauri/src/riproduzione/riapertura.rs"));
  assert.match(riapertura, /fn riparte_fermo_dopo_il_cambio\(/);
  assert.match(riapertura, /riparte_fermo_dopo_il_cambio\(/);
  // Solo la riapertura del sorvegliante: chi sceglie un'uscita nelle impostazioni
  // o preme play ha chiesto lui di sentire dove l'ha messa.
  assert.match(riapertura, /matches!\(modo, Modo::Automatica\)/);
});
