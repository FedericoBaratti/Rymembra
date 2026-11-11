/*
 * Il modulo delle segnalazioni su GitHub, letto come testo.
 *
 * Parlava solo di Windows: il percorso del diario della copia NSIS e un
 * campo «Which Windows». Chi usa l'APK non trovava come mandare il diario
 * (sul telefono si condivide), e chi usa la copia del Microsoft Store veniva
 * mandato in una cartella che lì non c'è: i file che un'app impacchettata
 * crea in `%APPDATA%` Windows li mette nella cartella privata del pacchetto
 * (R20-4). Le etichette dei menù sono quelle di `en.json`.
 *
 *   node --test "strumenti/*.test.js"
 */
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const REPO = path.dirname(__dirname);
const MODULO = fs.readFileSync(path.join(REPO, ".github/ISSUE_TEMPLATE/problema.yml"), "utf8");
const EN = JSON.parse(fs.readFileSync(path.join(REPO, "apps/desktop/src/lingue/en.json"), "utf8"));

test("il modulo chiede dove gira Rymembra: installer, Store o telefono", () => {
  const dove = /- type: dropdown\n\s+id: dove\n[\s\S]*?options:\n((?:\s+- .+\n)+)/.exec(MODULO);
  assert.ok(dove, "manca la scelta «dove»");
  const scelte = dove[1].split("\n").map((r) => r.trim().replace(/^- /, "")).filter(Boolean);
  assert.equal(scelte.length, 3, scelte.join(" | "));
  assert.match(scelte.join(" | "), /installer/);
  assert.match(scelte.join(" | "), /Microsoft Store/);
  assert.match(scelte.join(" | "), /Android/);
  assert.ok(!/label: Which Windows$/m.test(MODULO), "il campo dice ancora solo Windows");
});

test("per ognuno dei tre dice come si arriva al diario, con le parole dell'app", () => {
  const diario = /id: diario\n[\s\S]*?render: text/.exec(MODULO)[0];
  assert.ok(diario.includes(EN["settings.update.title"]));
  assert.ok(diario.includes(EN["settings.diary.open"]));
  // Sul telefono il diario non è una cartella: si condivide.
  assert.ok(diario.includes(EN["settings.diary.open.telefono"]), "manca la via del telefono");
  // La copia del Store: la cartella privata del pacchetto, non %APPDATA%.
  assert.match(diario, /%LOCALAPPDATA%\\Packages\\/);
  assert.match(diario, /FedericoBaratti\.Rymembra_/);
  assert.match(diario, /LocalCache\\Roaming/);
});
