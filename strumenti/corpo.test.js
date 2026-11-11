/*
 * Il corpo della release: per chi ha una build di prova, per Windows, per Android.
 *
 *   node --test "strumenti/*.test.js"
 */
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const { ETICHETTE, corpoDelRepository } = require("./corpo.js");
const { nomeAsset } = require("./apk.js");

const REPO = path.dirname(__dirname);
const versione = JSON.parse(fs.readFileSync(path.join(REPO, "apps/desktop/package.json"), "utf8")).version;
const testo = corpoDelRepository();
const sezioni = [...testo.matchAll(/^### (.+)$/gm)].map((m) => m[1]);

test("la prima sezione è per chi ha una build di prova del telefono", () => {
  // Le copie di prova mostrano il corpo della release come note
  // dell'aggiornamento, ed è l'unico testo che arriva a loro (D-5).
  assert.match(sezioni[0], /test build/);
  assert.ok(sezioni.indexOf("Windows") > 0 && sezioni.indexOf("Android") > 0, sezioni.join(" | "));
  // L'avviso di SmartScreen è per Windows, non la prima cosa che legge chi scarica un APK.
  assert.ok(testo.indexOf("This installer is not signed") > testo.indexOf("### Windows"));
  assert.ok(testo.indexOf("This installer is not signed") < testo.indexOf("### Android"));
});

test("la procedura è esporta, disinstalla, installa, importa, e dice che cosa non passa", () => {
  const prova = testo.slice(testo.indexOf("### If you have a test build"), testo.indexOf("### Windows"));
  const passi = [...prova.matchAll(/^\d\. (.+)$/gm)].map((m) => m[1]);
  assert.equal(passi.length, 4);
  assert.match(passi[0], /Export the profile/);
  assert.match(passi[1], /Uninstall/);
  assert.match(passi[2], new RegExp(nomeAsset(versione).replace(/\./g, "\\.")));
  assert.match(passi[3], /Read a profile/);
  assert.match(prova, /connected accounts/);
  assert.match(prova, /linked devices/);
});

test("i nomi dei menù sono quelli dell'app in inglese", () => {
  const en = JSON.parse(fs.readFileSync(path.join(REPO, "apps/desktop/src/lingue/en.json"), "utf8"));
  for (const chiave of Object.values(ETICHETTE)) assert.ok(testo.includes(en[chiave]), `${chiave}: «${en[chiave]}» non è nel corpo`);
});

test("i file e l'impronta sono quelli della release", () => {
  assert.ok(testo.startsWith(`## Rymembra ${versione}\n`));
  assert.ok(testo.includes(`\`Rymembra_${versione}_x64-setup.exe\``));
  assert.ok(testo.includes(`\`${nomeAsset(versione)}\``));
  const impronta = fs.readFileSync(path.join(__dirname, "android-certificato.sha256"), "utf8").trim();
  assert.ok(testo.includes(impronta));
  assert.ok(testo.includes(`/blob/v${versione}/CHANGELOG.md`));
});

test("il corpo è in inglese americano, con le virgolette tipografiche", () => {
  assert.ok(!/[«»]/.test(testo), "virgolette italiane nel corpo inglese");
  assert.ok(!/favourite|colour|licence\b/i.test(testo), "inglese britannico");
  assert.ok(!/\b(?:isn|doesn|won|aren|hasn|it)'(?:t|s)\b/.test(testo), "apostrofo dritto");
});
