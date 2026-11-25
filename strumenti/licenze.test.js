/*
 * I testi di licenza, letti da una cartella finta.
 *
 * `licenze.js` scartava in silenzio ogni file di licenza sopra i 32 KiB: il
 * `LICENSE-THIRD-PARTY` di `crossbeam-channel` (33 447 byte, con il CC BY 3.0 e
 * la BSD di Go) mancava da `THIRD-PARTY-NOTICES.md` senza che nessuno lo
 * dicesse. Qui si prova la lettura di una cartella, senza `cargo` e senza
 * scrivere il file vero.
 *
 *   node --test "strumenti/*.test.js"
 */
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const { testiDi, MASSIMO, PARAGRAFO_ANDROID } = require("./licenze.js");

function crateFinto(file) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rymembra-licenze-"));
  fs.writeFileSync(path.join(dir, "Cargo.toml"), '[package]\nname = "finto"\n');
  for (const [nome, testo] of Object.entries(file)) fs.writeFileSync(path.join(dir, nome), testo);
  return dir;
}

test("un testo di licenza lungo non sparisce in silenzio", () => {
  const dir = crateFinto({
    "LICENSE-MIT": "MIT License\n\nCopyright (c) finto\n",
    // Come quello di crossbeam-channel: più di 32 KiB di licenze vere.
    "LICENSE-THIRD-PARTY": "Creative Commons Attribution 3.0\n" + "x".repeat(40 * 1024) + "\n",
    "README.md": "non è una licenza",
  });
  try {
    const { trovati, scartati } = testiDi(path.join(dir, "Cargo.toml"));
    assert.deepEqual(
      trovati.map((t) => t.nome),
      ["LICENSE-MIT", "LICENSE-THIRD-PARTY"],
    );
    assert.deepEqual(scartati, []);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("un file oltre il tetto si dice, con nome e peso", () => {
  const grande = "y".repeat(MASSIMO + 1);
  const dir = crateFinto({ "LICENSE-MIT": "MIT\n", LICENSE: grande });
  try {
    const { trovati, scartati } = testiDi(path.join(dir, "Cargo.toml"));
    assert.deepEqual(
      trovati.map((t) => t.nome),
      ["LICENSE-MIT"],
    );
    assert.deepEqual(scartati, [{ nome: "LICENSE", byte: Buffer.byteLength(grande) }]);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("il tetto copre ogni testo di licenza vero", () => {
  // Il testo Apache-2.0 sta in undici kilobyte, il più lungo del registro
  // (crossbeam-channel) in trentatré: il tetto serve contro i file di dati
  // chiamati LICENSE, non contro le licenze lunghe.
  assert.ok(MASSIMO >= 256 * 1024);
});

// ── le librerie dell'APK (S10-7) ────────────────────────────────────────────
//
// Il paragrafo sulle librerie Kotlin e Java è testo fisso: Gradle non passa da
// `cargo` né da `npm`, e nessuno lo confrontava con `build.gradle.kts`.

const GRADLE = path.join(__dirname, "..", "apps", "desktop", "src-tauri", "gen", "android", "app", "build.gradle.kts");

/** Le dipendenze `implementation("gruppo:artefatto:versione")` di Gradle. */
function dipendenzeDirette() {
  const testo = fs.readFileSync(GRADLE, "utf8");
  return [...testo.matchAll(/^\s*implementation\("([^":]+):([^":]+):[^"]+"\)/gm)].map((m) => ({ gruppo: m[1], artefatto: m[2] }));
}

/** Come il paragrafo chiama le librerie il cui artefatto non è scritto fra apici. */
const COME_SI_CHIAMA = {
  "com.google.android.material": /Material Components/,
};

test("ogni dipendenza gradle diretta è nominata nel paragrafo Android", () => {
  const dipendenze = dipendenzeDirette();
  assert.ok(dipendenze.length >= 10, `build.gradle.kts ha ${dipendenze.length} dipendenze dirette: la lettura è rotta?`);
  const testo = PARAGRAFO_ANDROID.join("\n");
  const mancano = dipendenze
    .filter((d) => !(COME_SI_CHIAMA[d.gruppo] ?? new RegExp("`" + d.artefatto + "`")).test(testo))
    .map((d) => `${d.gruppo}:${d.artefatto}`);
  assert.deepEqual(mancano, [], `dipendenze di build.gradle.kts che il paragrafo Android di licenze.js non nomina: ${mancano.join(", ")}`);
});

test("quel che le dipendenze portano con sé è nominato, e la MIT di checker-qual è scritta", () => {
  const testo = PARAGRAFO_ANDROID.join("\n");
  for (const nome of ["Guava", "jspecify", "Jackson", "Checker Framework", "`checker-qual`"]) {
    assert.ok(testo.includes(nome), `il paragrafo Android non nomina ${nome}`);
  }
  assert.ok(!/All of them are under \*\*Apache-2\.0\*\*, whose text is below\.(\s|$)/.test(testo), "non sono tutte Apache-2.0");
  assert.match(testo, /MIT License:/);
  assert.match(testo, /Copyright 2004-present by the Checker Framework developers/);
  assert.match(testo, /THE SOFTWARE IS PROVIDED "AS IS"/);
});
