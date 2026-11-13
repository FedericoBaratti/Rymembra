/*
 * Le note dell'aggiornamento, provate prima del tag.
 *
 * `manifesto.js` gira una volta sola, nella CI di release, dopo venti minuti di
 * compilazione: è il posto peggiore per scoprire che la sezione del changelog
 * non ha titoli, perché lì la release si ferma — o, prima della 3.0, partiva
 * con una tendina vuota su ogni copia installata. Queste prove girano a ogni
 * push (`verify.yml`) e in `npm run verify`.
 *
 *   node --test "strumenti/*.test.js"
 */
const test = require("node:test");
const assert = require("node:assert/strict");

const { note, noteDa, versione, versioneFirmata } = require("./manifesto.js");

/** Un `.sig` come lo scrive Tauri: il testo minisign, in base64. */
function sig(commentoFidato) {
  const testo = [
    "untrusted comment: signature from tauri secret key",
    "RUTNDb6LIyIG/xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    `trusted comment: ${commentoFidato}`,
    "yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy",
    "",
  ].join("\n");
  return Buffer.from(testo, "utf8").toString("base64");
}

test("la versione firmata si legge dal commento fidato", () => {
  assert.equal(
    versioneFirmata(sig("timestamp:1790000000\tfile:Rymembra_3.0.0_x64-setup.exe\tversion:3.0.0")),
    "3.0.0",
  );
  // Le firme fatte prima che la CLI la scrivesse: niente versione, e dalla 3.0
  // non aggiornano più nessuno.
  assert.equal(versioneFirmata(sig("timestamp:1790000000\tfile:Rymembra_2.4.0_x64-setup.exe")), null);
});

test("la versione nei file ha delle note vere nel changelog", () => {
  const v = versione();
  const testo = note(v);
  const titoli = testo.split("\n").filter((r) => r.startsWith("• "));
  assert.ok(
    titoli.length >= 3,
    `CHANGELOG.md: la sezione della ${v} ha ${titoli.length} titoli \`### \` da mostrare ` +
      `nell'avviso d'aggiornamento; ne servono almeno tre.\n${testo}`,
  );
});

test("i titoli di servizio non finiscono nell'avviso", () => {
  const changelog = [
    "## [9.0.0] — 2030-01-01",
    "",
    "### Added — una cosa",
    "testo",
    "### The number",
    "### Database",
    "### Fixed — un'altra",
    "## [8.0.0] — 2029-01-01",
    "### Added — vecchia",
  ].join("\r\n");
  assert.equal(noteDa(changelog, "9.0.0"), "• Added — una cosa\n• Fixed — un'altra");
});

test("la sezione della versione vince su quella non rilasciata", () => {
  const changelog = [
    "## [Unreleased]",
    "### Added — ancora da uscire",
    "## [9.0.0] — 2030-01-01",
    "### Added — uscita",
  ].join("\n");
  assert.equal(noteDa(changelog, "9.0.0"), "• Added — uscita");
  assert.equal(noteDa(changelog, "9.1.0"), "• Added — ancora da uscire");
});

test("oltre otto titoli si rimanda al changelog, in inglese come le note", () => {
  const righe = ["## [9.0.0]"];
  for (let i = 1; i <= 11; i++) righe.push(`### Voce ${i}`);
  const testo = noteDa(righe.join("\n"), "9.0.0").split("\n");
  assert.equal(testo.length, 9);
  // La 3.2.0 è uscita con «… e altre 1 voci nel changelog.»: una riga italiana
  // (e sgrammaticata) nelle note di chi usa Rymembra in inglese.
  assert.equal(testo[8], "… and 3 more in the changelog.");
});

test("le sezioni Security vengono prima, ovunque siano nel changelog", () => {
  const changelog = [
    "## [9.0.0] — 2030-01-01",
    "### New — una",
    "### New — due",
    "### Changed — tre",
    "### Changed — quattro",
    "### Fixed — cinque",
    "### Fixed — sei",
    "### Changed — sette",
    "### Changed — otto",
    "### Security — la revisione",
    "### Fixed — nove",
    "### Security",
    "### Known limits",
    "### Database",
  ].join("\n");
  const titoli = noteDa(changelog, "9.0.0").split("\n");
  assert.deepEqual(titoli.slice(0, 3), ["• Security — la revisione", "• Security", "• New — una"]);
  assert.equal(titoli.length, 9);
  // «Known limits» dice che cosa non c'è: non è una novità da annunciare.
  assert.ok(!titoli.some((t) => /Known limits/.test(t)));
  assert.equal(titoli[8], "… and 3 more in the changelog.");
});

test("le note della prossima versione mostrano le sezioni Security del changelog vero", () => {
  const changelog = require("node:fs").readFileSync(require("node:path").join(__dirname, "../CHANGELOG.md"), "utf8");
  // Fra un rilascio e l'altro le note sono quelle di «[Unreleased]» (una
  // versione che non c'è ci ripiega); nel commit del numero «[Unreleased]»
  // diventa la voce della versione dei file, e sono le note che l'updater
  // mostrerà davvero.
  const aperta = /^## \[Unreleased\]/m.test(changelog);
  const versione = aperta ? "999.0.0" : require("../apps/desktop/package.json").version;
  const testo = noteDa(changelog, versione);
  const titoli = testo.split("\n");
  const security = titoli.filter((t) => t.startsWith("• Security"));
  assert.ok(security.length >= 1, testo);
  assert.ok(titoli.slice(0, security.length).every((t) => t.startsWith("• Security")), testo);
  assert.ok(!/Known limits/.test(testo), testo);
});

test("una sezione che non c'è dà note vuote, e non quelle di un'altra versione", () => {
  assert.equal(noteDa("## [8.0.0]\n### Added — vecchia\n", "9.0.0"), "");
});
