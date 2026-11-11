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

const { testiDi, MASSIMO } = require("./licenze.js");

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
