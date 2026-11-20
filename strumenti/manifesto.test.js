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
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const { note, noteDa, versione, versioneFirmata, verificaManifesto } = require("./manifesto.js");
const { nuovaChiave, commentoDi } = require("./firma-di-prova.js");

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

/* ── S5-2: la firma contro i byte ─────────────────────────────────────── */

const NOME = "Rymembra_9.9.9_x64-setup.exe";

/**
 * Un albero finto con `manifesto.js` dentro: lo script risolve i percorsi
 * rispetto a sé stesso (`REPO = dirname(__dirname)`), quindi una copia in una
 * cartella di prova lavora su quella e non sul repository vero.
 */
function albero({ chiave, installer, firma, uscita = {} }) {
  const radice = fs.mkdtempSync(path.join(os.tmpdir(), "rymembra-manifesto-"));
  const scrivi = (rel, dati) => {
    fs.mkdirSync(path.dirname(path.join(radice, rel)), { recursive: true });
    fs.writeFileSync(path.join(radice, rel), dati);
  };
  for (const f of ["manifesto.js", "minisign.js"]) {
    if (fs.existsSync(path.join(__dirname, f))) scrivi(`strumenti/${f}`, fs.readFileSync(path.join(__dirname, f)));
  }
  scrivi("apps/desktop/package.json", JSON.stringify({ version: "9.9.9" }));
  scrivi("apps/desktop/src-tauri/tauri.conf.json", JSON.stringify({ plugins: { updater: { pubkey: chiave.pubkey } } }));
  scrivi("CHANGELOG.md", "## [9.9.9] — 2030-01-01\n### New — una cosa\n");
  scrivi(`target/release/bundle/nsis/${NOME}`, installer);
  scrivi(`target/release/bundle/nsis/${NOME}.sig`, firma);
  for (const [nome, dati] of Object.entries(uscita)) scrivi(`uscita/${nome}`, dati);
  return radice;
}

/** Lancia il `manifesto.js` dell'albero finto, con un ambiente che non è quello della CI. */
function lancia(radice) {
  const env = { ...process.env, GITHUB_REPOSITORY: "prova/Rymembra" };
  for (const nome of Object.keys(env)) {
    if (/^(TAURI_|GITHUB_(?!REPOSITORY$))/i.test(nome)) delete env[nome];
  }
  const esito = spawnSync(process.execPath, [path.join(radice, "strumenti", "manifesto.js"), "uscita"], { env, encoding: "utf8" });
  return { codice: esito.status, uscita: `${esito.stdout}${esito.stderr}`, manifesto: path.join(radice, "uscita", "latest.json") };
}

const BYTE = Buffer.from("l'installer vero");

test("un installer firmato davvero dà un latest.json con quella firma", () => {
  const chiave = nuovaChiave();
  const firma = chiave.firma(BYTE, commentoDi(NOME, "9.9.9"));
  const { codice, uscita, manifesto } = lancia(albero({ chiave, installer: BYTE, firma, uscita: { [NOME]: BYTE } }));
  assert.equal(codice, 0, uscita);
  assert.equal(JSON.parse(fs.readFileSync(manifesto, "utf8")).platforms["windows-x86_64"].signature, firma);
});

test("una firma con i byte cambiati non passa, anche se id e versione tornano (S5-2)", () => {
  const chiave = nuovaChiave();
  const firma = chiave.firma(BYTE, commentoDi(NOME, "9.9.9"));
  // Un file ricostruito o sostituito dopo la firma: stesso nome, stessa chiave,
  // stessa versione nel commento. I cancelli di testo dicevano «ok».
  const cambiato = Buffer.from(BYTE);
  cambiato[0] ^= 1;
  const { codice, uscita, manifesto } = lancia(albero({ chiave, installer: cambiato, firma }));
  assert.equal(codice, 1, uscita);
  assert.match(uscita, /firma non è valida per i byte/);
  assert.ok(!fs.existsSync(manifesto), "il manifesto non deve nascere");
});

test("il file che si carica non è quello firmato: si ferma (S5-2)", () => {
  const chiave = nuovaChiave();
  const firma = chiave.firma(BYTE, commentoDi(NOME, "9.9.9"));
  // `target/…/bundle/nsis` ha il file firmato, `uscita/` — l'artefatto che
  // `pubblica` consegna — ne ha uno diverso: un passo in mezzo l'ha toccato.
  const { codice, uscita } = lancia(albero({ chiave, installer: BYTE, firma, uscita: { [NOME]: Buffer.concat([BYTE, Buffer.from("x")]) } }));
  assert.equal(codice, 1, uscita);
  assert.match(uscita, /il file controllato: uscita[\\/]/);
});

test("un commento fidato alterato non passa: la firma globale lo copre (S5-2)", () => {
  const chiave = nuovaChiave();
  const firma = chiave.firma(BYTE, commentoDi(NOME, "9.9.9", "1790000000"));
  // Il testo è lo stesso quanto a id e versione: cambia solo la marca di tempo.
  const alterata = Buffer.from(Buffer.from(firma, "base64").toString("utf8").replace("1790000000", "1790000001"), "utf8").toString("base64");
  const { codice, uscita } = lancia(albero({ chiave, installer: BYTE, firma: alterata }));
  assert.equal(codice, 1, uscita);
  assert.match(uscita, /firma globale non copre il commento/);
});

test("il latest.json si ricontrolla com'è scritto: la firma dentro è dei byte dell'installer (S5-2)", () => {
  const chiave = nuovaChiave();
  const firma = chiave.firma(BYTE, commentoDi(NOME, "9.9.9"));
  const buono = { version: "9.9.9", platforms: { "windows-x86_64": { signature: firma, url: `https://x/${NOME}` } } };
  assert.deepEqual(verificaManifesto(buono, BYTE, chiave.pubkey), []);
  const altra = nuovaChiave().firma(BYTE, commentoDi(NOME, "9.9.9"));
  const sbagliato = { ...buono, platforms: { "windows-x86_64": { signature: altra, url: `https://x/${NOME}` } } };
  assert.match(verificaManifesto(sbagliato, BYTE, chiave.pubkey).join("\n"), /latest\.json/);
  assert.match(verificaManifesto({ version: "9.9.9", platforms: {} }, BYTE, chiave.pubkey).join("\n"), /windows-x86_64/);
});
