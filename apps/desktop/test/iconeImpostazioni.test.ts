import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * Le icone delle Impostazioni non dicono il contrario di quel che c'è
 * (R18-9, la parte delle Impostazioni).
 *
 * «Chiusura e secondo piano» aveva la croce che, nelle righe accanto, vuol dire
 * «togli»; «Riproduzione», «Uscita audio», «Dissolvenza incrociata» e «Timer di
 * spegnimento» avevano tutte il ▶; «Equalizzatore», «Spettro», «Scorciatoie da
 * tastiera», «Movimento e accesso» e la voce «Scrobbling» tutte le barre.
 */
const SORGENTE = readFileSync(new URL("../src/schermate/Impostazioni.tsx", import.meta.url), "utf8");

/** L'icona della scheda col titolo `chiave`. */
function iconaDi(chiave: string): string {
  const dove = SORGENTE.indexOf(`titolo={t("${chiave}")}`);
  assert.ok(dove > 0, `non trovo ${chiave}`);
  const prima = SORGENTE.slice(SORGENTE.lastIndexOf("<Scheda", dove), dove);
  return /icona="([a-z0-9-]+)"/.exec(prima)?.[1] ?? "";
}

test("la croce non sta su una scheda: è il glifo di «togli»", () => {
  assert.notEqual(iconaDi("settings.background.title"), "i-x");
  assert.doesNotMatch(SORGENTE, /\["chiusura", t\("settings\.section\.chiusura"\), "i-x"\]/);
});

test("le schede della riproduzione non hanno tutte la stessa icona", () => {
  const play = ["settings.playback.title", "settings.output.title", "settings.crossfade.title", "settings.sleep.title"];
  const barre = ["settings.eq.title", "settings.shortcuts.title", "settings.motion.title"];
  for (const gruppo of [play, barre]) {
    const icone = gruppo.map(iconaDi);
    assert.equal(new Set(icone).size, icone.length, `${gruppo.join(", ")}: ${icone.join(", ")}`);
  }
  assert.doesNotMatch(SORGENTE, /\["scrobbling", t\("settings\.section\.scrobbling"\), "i-eq"\]/);
});
