/*
 * PRIVACY.md dice quel che il manifesto di Android chiede, e la data di PRIVACY
 * e di TERMS non è più vecchia dell'ultima volta che il file è cambiato (S10-5,
 * S10-6).
 *
 * # I permessi (S10-5)
 *
 * La tabella «What the app asks Android for, and why» è scritta a mano, e un
 * permesso nuovo nel manifesto non la faceva cambiare: `RECEIVE_BOOT_COMPLETED`
 * stava dichiarato dal manifesto e spiegato solo dentro un'altra sezione.
 * La prova guarda **la tabella**, non il file intero: una voce citata di passaggio
 * in un altro paragrafo non conta, perché chi cerca «che cosa chiede l'app?»
 * legge la tabella.
 *
 * Ogni `uses-permission` del manifesto deve avere una riga che lo nomina (o la
 * famiglia a cui appartiene: «Music and audio» è tre permessi). Un permesso nuovo
 * fa fallire la prova finché non ha la sua riga e la sua voce in `RIGA_DI`.
 *
 * # La data (S10-6)
 *
 * «Last updated: 18 November 2026» stava su un file cambiato il 19. La prova
 * confronta la data scritta con quella dell'**ultimo commit** del file, e solo
 * se il repository ha la storia: in un clone superficiale (`fetch-depth: 1`,
 * quello di CI) ogni file risulta cambiato dall'unico commit che c'è, e la prova
 * si fermerebbe sul commit di rilascio per niente. Si guarda la data dell'autore
 * (`%as`), che un innesto o un rebase della storia non riscrive, e non quella del
 * committente. Non si confronta con la data del CHANGELOG: sceglierla è del
 * rilascio, e non deve costringere a toccare un testo che non cambia.
 */
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");

const REPO = process.env.RYMEMBRA_RADICE || path.dirname(__dirname);
const MANIFESTO = "apps/desktop/src-tauri/gen/android/app/src/main/AndroidManifest.xml";

/**
 * Per ogni permesso, cosa deve comparire nella sua riga della tabella. I nomi
 * della tabella sono quelli che legge chi usa il telefono («Notifications»), non
 * le costanti di Android; solo `RECEIVE_BOOT_COMPLETED` ha il suo nome scritto.
 */
const RIGA_DI = {
  INTERNET: /Internet, network state/,
  ACCESS_NETWORK_STATE: /Internet, network state/,
  ACCESS_WIFI_STATE: /Wi-Fi state, Wi-Fi multicast/,
  CHANGE_WIFI_MULTICAST_STATE: /Wi-Fi state, Wi-Fi multicast/,
  WAKE_LOCK: /Keep the device awake/,
  ACCESS_LOCAL_NETWORK: /Nearby devices/,
  FOREGROUND_SERVICE: /Foreground service/,
  FOREGROUND_SERVICE_MEDIA_PLAYBACK: /Foreground service \(media playback\)/,
  FOREGROUND_SERVICE_DATA_SYNC: /Foreground service \(data sync\)/,
  FOREGROUND_SERVICE_CONNECTED_DEVICE: /Foreground service \(connected device\)/,
  RECEIVE_BOOT_COMPLETED: /RECEIVE_BOOT_COMPLETED/,
  POST_NOTIFICATIONS: /^\| Notifications/,
  READ_MEDIA_AUDIO: /Music and audio/,
  READ_EXTERNAL_STORAGE: /Music and audio/,
  WRITE_EXTERNAL_STORAGE: /Music and audio/,
  MANAGE_EXTERNAL_STORAGE: /All files access/,
  REQUEST_INSTALL_PACKAGES: /Install apps/,
  CAMERA: /^\| Camera/,
};

/** Le righe della tabella dei permessi di PRIVACY. */
function tabellaDeiPermessi() {
  const righe = fs.readFileSync(path.join(REPO, "PRIVACY.md"), "utf8").split("\n");
  const inizio = righe.findIndex((r) => /^\| Permission \| Why \|/.test(r));
  assert.ok(inizio >= 0, "PRIVACY.md non ha più la tabella dei permessi di Android («| Permission | Why |»)");
  const tabella = [];
  for (let i = inizio + 2; i < righe.length && righe[i].startsWith("|"); i++) tabella.push(righe[i]);
  return tabella;
}

/** I permessi che il manifesto dichiara. */
function permessiDelManifesto() {
  const xml = fs.readFileSync(path.join(REPO, MANIFESTO), "utf8");
  return [...xml.matchAll(/<uses-permission[^>]*?android:name="android\.permission\.(\w+)"/g)].map((m) => m[1]);
}

/** I permessi che nessuna riga della tabella nomina, e quelli che `RIGA_DI` non conosce. */
function permessiSenzaRiga(tabella, permessi) {
  const senzaRiga = [];
  const senzaNome = [];
  for (const p of permessi) {
    const cerca = RIGA_DI[p];
    if (!cerca) senzaNome.push(p);
    else if (!tabella.some((riga) => cerca.test(riga))) senzaRiga.push(p);
  }
  return { senzaRiga, senzaNome };
}

test("ogni permesso del manifesto ha la sua riga nella tabella dei permessi di PRIVACY", () => {
  const permessi = permessiDelManifesto();
  assert.ok(permessi.length >= 15, `il manifesto ha ${permessi.length} permessi: la lettura è rotta?`);
  const { senzaRiga, senzaNome } = permessiSenzaRiga(tabellaDeiPermessi(), permessi);
  assert.deepEqual(senzaNome, [], `permessi nuovi nel manifesto: aggiungere la riga a PRIVACY.md e la voce a RIGA_DI (${senzaNome.join(", ")})`);
  assert.deepEqual(senzaRiga, [], `permessi del manifesto che la tabella di PRIVACY.md non nomina: ${senzaRiga.join(", ")}`);
});

test("la prova guarda la tabella: un permesso citato solo in un altro paragrafo non basta", () => {
  // `RECEIVE_BOOT_COMPLETED` compare anche nel paragrafo «The phone», fuori dalla
  // tabella. Senza la sua riga, la prova deve nominarlo lo stesso.
  const tabella = tabellaDeiPermessi().filter((r) => !/RECEIVE_BOOT_COMPLETED/.test(r));
  const { senzaRiga } = permessiSenzaRiga(tabella, permessiDelManifesto());
  assert.deepEqual(senzaRiga, ["RECEIVE_BOOT_COMPLETED"]);
});

test("le app che il manifesto nomina in `<queries>` sono dette in PRIVACY", () => {
  const xml = fs.readFileSync(path.join(REPO, MANIFESTO), "utf8");
  const privacy = fs.readFileSync(path.join(REPO, "PRIVACY.md"), "utf8");
  const nomi = [...xml.matchAll(/<queries>([\s\S]*?)<\/queries>/g)]
    .flatMap((q) => [...q[1].matchAll(/<package android:name="([^"]+)"/g)])
    .map((m) => m[1]);
  assert.ok(nomi.length >= 5, `<queries> nomina ${nomi.length} pacchetti: la lettura è rotta?`);
  for (const nome of nomi) {
    assert.ok(privacy.includes(`\`${nome}\``), `PRIVACY.md non nomina il pacchetto ${nome}, che <queries> chiede di vedere`);
  }
});

// ── la data ─────────────────────────────────────────────────────────────────

const MESI = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/** `Last updated: 25 November 2026.` → `2026-11-25`. */
function dataScritta(file) {
  const testo = fs.readFileSync(path.join(REPO, file), "utf8");
  const m = /^Last updated: (\d{1,2}) ([A-Z][a-z]+) (\d{4})\.$/m.exec(testo);
  assert.ok(m, `${file} non ha la riga «Last updated: <giorno> <mese> <anno>.»`);
  const mese = MESI.indexOf(m[2]) + 1;
  assert.ok(mese > 0, `${file}: «${m[2]}» non è un mese`);
  return `${m[3]}-${String(mese).padStart(2, "0")}-${m[1].padStart(2, "0")}`;
}

/** La data (AAAA-MM-GG) dell'autore dell'ultimo commit del file, o `null` se la storia non c'è. */
function ultimoCommitDi(file) {
  const git = (...args) => execFileSync("git", args, { cwd: REPO, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
  try {
    if (git("rev-parse", "--is-shallow-repository") === "true") return null;
    return git("log", "-1", "--format=%as", "--", file) || null;
  } catch {
    return null;
  }
}

test("la data di PRIVACY e di TERMS è scritta in modo che si legga, e non nel futuro", () => {
  for (const f of ["PRIVACY.md", "TERMS.md"]) {
    const data = dataScritta(f);
    assert.ok(!Number.isNaN(Date.parse(data)), `${f}: ${data} non è una data`);
    assert.ok(data <= new Date(Date.now() + 36 * 3600 * 1000).toISOString().slice(0, 10), `${f}: la data scritta (${data}) è nel futuro`);
  }
});

for (const f of ["PRIVACY.md", "TERMS.md"]) {
  test(`${f}: la data «Last updated» non è più vecchia dell'ultima modifica del file`, (t) => {
    const scritta = dataScritta(f);
    const commit = ultimoCommitDi(f);
    if (commit === null) {
      t.skip("senza la storia di git (clone superficiale, o niente commit) la data non si può confrontare");
      return;
    }
    assert.ok(
      scritta >= commit,
      `${f} dice «Last updated» ${scritta}, ma l'ultimo commit che lo tocca è del ${commit}: se il testo è cambiato, la data si aggiorna`,
    );
  });
}
