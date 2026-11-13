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

test("la procedura è esporta, disinstalla, installa, scansiona, importa, e dice che cosa non passa", () => {
  const prova = testo.slice(testo.indexOf("### If you have a test build"), testo.indexOf("### Windows"));
  const passi = [...prova.matchAll(/^\d\. (.+)$/gm)].map((m) => m[1]);
  assert.equal(passi.length, 5);
  assert.match(passi[0], /Export the profile/);
  assert.match(passi[1], /Uninstall/);
  assert.match(passi[2], new RegExp(nomeAsset(versione).replace(/\./g, "\\.")));
  // T3-3: la musica prima, il profilo dopo. Il permesso alla musica sta nel
  // passo della scansione, prima di leggere il profilo.
  assert.match(passi[3], /Allow access/);
  assert.match(passi[3], /Continue/);
  assert.match(passi[3], /Add a folder/);
  assert.match(passi[3], /scan has finished/);
  assert.match(passi[4], /Read a profile/);
  assert.match(prova, /connected accounts/);
  assert.match(prova, /linked devices/);
});

test("il profilo si legge dopo la scansione, e il testo dice perché (T3-3)", () => {
  const prova = testo.slice(testo.indexOf("### If you have a test build"), testo.indexOf("### Windows"));
  assert.match(prova, /Read the profile after the scan, not before/);
  assert.match(prova, /scan, then read the same file again/);
  // Il testo di prima mandava a scansionare **dopo** il profilo.
  assert.ok(!/after reading the profile, scan/i.test(prova), "l'ordine sbagliato è ancora nel corpo");
});

test("il corpo promette solo quel che il profilo porta davvero (T3-4)", () => {
  const prova = testo.slice(testo.indexOf("### If you have a test build"), testo.indexOf("### Windows"));
  // `sincronia::segna_posizione` non ha chiamanti e `player.position` resta
  // fuori dal catalogo: il profilo non porta posizioni di ripresa.
  assert.ok(!/resume position/i.test(prova), "«resume positions» non sono nel profilo");
  assert.match(prova, /playback queue/);
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
