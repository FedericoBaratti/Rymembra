/*
 * La versione, in un posto solo.
 *
 * # Il problema che risolve
 *
 * La versione finisce in quattro file che nessuno tiene allineati a mano a
 * lungo: `Cargo.toml` in radice (da cui i tredici crate la ereditano),
 * `apps/desktop/package.json`, `apps/desktop/src-tauri/tauri.conf.json`, e
 * `Cargo.lock`, che la rigenera `cargo`.
 *
 * Quattro numeri indipendenti sono quattro occasioni di divergere, e il sintomo
 * non è un errore: è un installer che si chiama in un modo e un programma che
 * ne dichiara un altro nel registro di avvio. Nella cartella `release/` del
 * vecchio albero ci sono ancora `Aether Setup 0.9.13.7.26.2.exe` e
 * `0.9.14.7.26.2.exe` accanto a `0.9.14.7.26.3.exe`: è questo problema in forma
 * di artefatti.
 *
 * # Due modi
 *
 *   node strumenti/versione.js            controlla che siano allineate
 *   node strumenti/versione.js 0.2.0      le riscrive tutte
 *
 * Il primo esce con 1 se qualcosa non torna, ed è quel che gira in CI. Il
 * `CHANGELOG.md` dava per esistenti `check:version` e `version:set`, che non
 * c'erano: adesso ci sono, e sono questo.
 */
const fs = require("fs");
const path = require("path");

// I percorsi si risolvono rispetto a **questo file**, non alla cartella da cui
// si è lanciato: gli script npm che lo chiamano girano da `apps/desktop`, e un
// percorso relativo alla CWD funzionerebbe solo dalla radice.
const REPO = path.dirname(__dirname);
process.chdir(REPO);

const RADICE = "Cargo.toml";
const PACCHETTO = "apps/desktop/package.json";
const TAURI = "apps/desktop/src-tauri/tauri.conf.json";

/** SemVer stretto: tre numeri, niente prerelease. Il bundler NSIS non ne vuole. */
const FORMA = /^\d+\.\d+\.\d+$/;

function leggiRadice() {
  const s = fs.readFileSync(RADICE, "utf8");
  // Dentro `[workspace.package]` e non altrove: nel manifesto di radice ci sono
  // decine di righe `version = "…"` sotto `[workspace.dependencies]`, e la
  // prima che si incontra non è quella giusta.
  const blocco = s.split("[workspace.package]")[1];
  if (!blocco) throw new Error("manca [workspace.package] in " + RADICE);
  const m = blocco.match(/^version\s*=\s*"([^"]+)"/m);
  if (!m) throw new Error("manca version in [workspace.package]");
  return m[1];
}

function scriviRadice(nuova) {
  const s = fs.readFileSync(RADICE, "utf8");
  const i = s.indexOf("[workspace.package]");
  const testa = s.slice(0, i);
  const coda = s.slice(i).replace(/^version\s*=\s*"[^"]+"/m, `version = "${nuova}"`);
  fs.writeFileSync(RADICE, testa + coda);
}

function leggiJson(p) {
  return JSON.parse(fs.readFileSync(p, "utf8")).version;
}

function scriviJson(p, nuova) {
  // A mano e non con `JSON.stringify`: riscrivere l'intero documento
  // riformatterebbe file che qualcuno legge, per cambiare tre cifre.
  const s = fs.readFileSync(p, "utf8");
  const fuori = s.replace(/"version":\s*"[^"]+"/, `"version": "${nuova}"`);
  if (fuori === s) throw new Error("nessun campo version in " + p);
  fs.writeFileSync(p, fuori);
}

function main() {
  const chiesta = process.argv[2];

  if (chiesta === undefined) {
    const trovate = {
      [RADICE]: leggiRadice(),
      [PACCHETTO]: leggiJson(PACCHETTO),
      [TAURI]: leggiJson(TAURI),
    };
    const distinte = [...new Set(Object.values(trovate))];
    for (const [file, v] of Object.entries(trovate)) {
      console.log(`${v.padEnd(12)} ${file}`);
    }
    if (distinte.length !== 1) {
      console.error(
        "\nLe versioni non coincidono. Allinearle con:\n" +
          `  node strumenti/versione.js ${trovate[RADICE]}`,
      );
      process.exit(1);
    }
    if (!FORMA.test(distinte[0])) {
      console.error(`\n«${distinte[0]}» non è tre numeri separati da punti.`);
      process.exit(1);
    }
    console.log("\nAllineate.");
    return;
  }

  if (!FORMA.test(chiesta)) {
    console.error(`«${chiesta}» non è tre numeri separati da punti.`);
    process.exit(1);
  }
  scriviRadice(chiesta);
  scriviJson(PACCHETTO, chiesta);
  scriviJson(TAURI, chiesta);
  console.log(
    `Versione ${chiesta} scritta in tre file.\n` +
      "Resta `Cargo.lock`: lo riallinea `cargo check`, e va nel commit.",
  );
}

main();
