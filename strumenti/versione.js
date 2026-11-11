/*
 * La versione, in un posto solo.
 *
 * # Il problema che risolve
 *
 * La versione finisce in cinque file che nessuno tiene allineati a mano a
 * lungo: `Cargo.toml` in radice (da cui i diciotto membri del workspace la
 * ereditano), `apps/desktop/package.json`, `apps/desktop/src-tauri/tauri.conf.json`,
 * e i due lockfile che la ripetono: `Cargo.lock` (una voce per membro) e
 * `apps/desktop/package-lock.json` (la radice e `packages[""]`).
 *
 * Cinque numeri indipendenti sono cinque occasioni di divergere, e il sintomo
 * non è un errore: è un installer che si chiama in un modo e un programma che
 * ne dichiara un altro nel registro di avvio. Nella cartella `release/` del
 * vecchio albero ci sono ancora `Rymembra Setup 0.9.13.7.26.2.exe` e
 * `0.9.14.7.26.2.exe` accanto a `0.9.14.7.26.3.exe`: è questo problema in forma
 * di artefatti.
 *
 * # I lockfile, e perché li riscrive lui
 *
 * Fino alla 3.2 questo script toccava tre file e diceva «resta `Cargo.lock`: lo
 * riallinea `cargo check`». In locale andava, perché `npm run verify` non
 * passava `--locked` e cargo riscriveva il lockfile in silenzio; in CI no:
 * `cargo clippy --locked` si fermava minuti dopo con «the lock file needs to be
 * updated», e il passo che costa tre secondi («Le versioni coincidono») aveva
 * detto «Allineate». Le righe da cambiare sono note — la `version` delle voci
 * senza `source` del `Cargo.lock`, cioè i membri, e due righe del lockfile di
 * npm — e cambiarle qui costa meno che spiegare a ogni rilascio come farle
 * cambiare a un altro strumento.
 *
 * # I numeri che non si possono usare
 *
 * Tre numeri, niente suffissi (NSIS e l'MSIX non ne vogliono), e minor e patch
 * fino a 999: il `versionCode` dell'APK è `major·10⁶ + minor·10³ + patch` (lo
 * scrive la CLI di Tauri in `tauri.properties`), e una 3.1000.0 avrebbe lo
 * stesso codice della 4.0.0 — Android rifiuterebbe l'aggiornamento come un
 * ritorno indietro. Il codice non può superare 2 100 000 000, il massimo che
 * Android documenta. Il tetto dell'MSIX (65535 per parte) sta largo dentro
 * questi.
 *
 * # Due modi
 *
 *   node strumenti/versione.js            controlla che siano allineate
 *   node strumenti/versione.js 0.2.0      le riscrive tutte
 *
 * Il primo esce con 1 se qualcosa non torna, ed è quel che gira in CI.
 */
const fs = require("fs");
const path = require("path");

// I percorsi si risolvono rispetto a **questo file**, non alla cartella da cui
// si è lanciato: gli script npm che lo chiamano girano da `apps/desktop`, e un
// percorso relativo alla CWD funzionerebbe solo dalla radice.
const REPO = path.dirname(__dirname);

const FILE = {
  cargoToml: "Cargo.toml",
  packageJson: "apps/desktop/package.json",
  tauriConf: "apps/desktop/src-tauri/tauri.conf.json",
  cargoLock: "Cargo.lock",
  packageLock: "apps/desktop/package-lock.json",
};

/** SemVer stretto: tre numeri, niente prerelease. Il bundler NSIS non ne vuole. */
const FORMA = /^\d+\.\d+\.\d+$/;

/** Il `versionCode` più alto che Android ammette. */
const CODICE_MASSIMO = 2100000000;

/** Il `versionCode` dell'APK per una versione: la regola di `tauri.properties`. */
function codiceAndroid(v) {
  const [major, minor, patch] = v.split(".").map(Number);
  return major * 1000000 + minor * 1000 + patch;
}

/** `null` se la versione si può usare, altrimenti il perché. */
function numeroAmmesso(v) {
  if (!FORMA.test(v)) return `«${v}» non è tre numeri separati da punti.`;
  const [, minor, patch] = v.split(".").map(Number);
  if (minor > 999 || patch > 999) {
    return (
      `«${v}»: minor e patch arrivano a 999, o il versionCode dell'APK ` +
      "(major·10⁶ + minor·10³ + patch) si scontra con quello della versione dopo."
    );
  }
  if (codiceAndroid(v) > CODICE_MASSIMO) return `«${v}»: il versionCode dell'APK supererebbe ${CODICE_MASSIMO}.`;
  return null;
}

/* ── Cargo.toml ───────────────────────────────────────────────────────── */

function versioneRadice(cargoToml) {
  // Dentro `[workspace.package]` e non altrove: nel manifesto di radice ci sono
  // decine di righe `version = "…"` sotto `[workspace.dependencies]`, e la
  // prima che si incontra non è quella giusta.
  const blocco = cargoToml.split("[workspace.package]")[1];
  if (!blocco) throw new Error(`manca [workspace.package] in ${FILE.cargoToml}`);
  const m = blocco.match(/^version\s*=\s*"([^"]+)"/m);
  if (!m) throw new Error("manca version in [workspace.package]");
  return m[1];
}

/** I percorsi dei membri, dall'elenco `members` di `[workspace]`. */
function membri(cargoToml) {
  const m = /^members\s*=\s*\[([\s\S]*?)\]/m.exec(cargoToml);
  if (!m) throw new Error(`manca members in [workspace] di ${FILE.cargoToml}`);
  return [...m[1].matchAll(/"([^"]+)"/g)].map((x) => x[1]);
}

/** Il nome del pacchetto di un membro, e se la versione la eredita. */
function membro(manifesto) {
  const pacchetto = manifesto.split(/^\[package\]\s*$/m)[1]?.split(/^\[/m)[0] ?? "";
  const nome = /^name\s*=\s*"([^"]+)"/m.exec(pacchetto);
  return { nome: nome ? nome[1] : null, eredita: /^version\.workspace\s*=\s*true\s*$/m.test(pacchetto) };
}

/* ── Cargo.lock ───────────────────────────────────────────────────────── */

/**
 * Le voci `[[package]]` del lockfile, ciascuna col suo testo (il primo pezzo è
 * l'intestazione). Quelle senza `source` sono del workspace: i pacchetti del
 * registro e di git la dicono sempre.
 */
function vociDelLock(cargoLock) {
  return cargoLock.split(/(?=^\[\[package\]\]\s*$)/m).map((testo) => {
    const nome = /^name = "([^"]+)"$/m.exec(testo);
    const versione = /^version = "([^"]+)"$/m.exec(testo);
    return {
      testo,
      pacchetto: testo.startsWith("[[package]]"),
      nome: nome ? nome[1] : null,
      versione: versione ? versione[1] : null,
      locale: !/^source = /m.test(testo),
    };
  });
}

/* ── i JSON ───────────────────────────────────────────────────────────── */

/**
 * Riscrive un campo `"version"` di un JSON nel posto esatto, senza riformattare
 * il resto: `JSON.stringify` riscriverebbe file che qualcuno legge, per
 * cambiare tre cifre. Il posto lo dice il rientro (`rientro` spazi) dopo il
 * tratto `dopo`; il risultato si confronta con l'oggetto atteso, e se fosse
 * cambiato qualcos'altro ci si ferma invece di scrivere.
 */
function riscriviCampo(testo, nuova, { rientro, dopo = "" }, atteso, file) {
  const inizio = dopo === "" ? 0 : testo.indexOf(dopo);
  if (inizio < 0) throw new Error(`${file}: non trovo ${JSON.stringify(dopo)}`);
  const m = new RegExp(`^ {${rientro}}"version": "[^"]*"`, "m").exec(testo.slice(inizio));
  if (!m) throw new Error(`${file}: non trovo il campo version`);
  const da = inizio + m.index;
  const fuori = testo.slice(0, da) + `${" ".repeat(rientro)}"version": "${nuova}"` + testo.slice(da + m[0].length);
  if (JSON.stringify(JSON.parse(fuori)) !== JSON.stringify(atteso)) {
    throw new Error(`${file}: riscrivere la versione cambierebbe altro; va fatto a mano`);
  }
  return fuori;
}

/* ── il controllo e la riscrittura, su testi già letti ───────────────── */

/**
 * Che cosa non torna, file per file. Vuoto se è tutto allineato.
 *
 * `r` ha i cinque testi (`cargoToml`, `packageJson`, `tauriConf`, `cargoLock`,
 * `packageLock`) e `manifestiDeiMembri`, il `Cargo.toml` di ogni membro per
 * percorso.
 */
function discordanze(r) {
  const fuori = [];
  const v = versioneRadice(r.cargoToml);
  const numero = numeroAmmesso(v);
  if (numero) fuori.push(`${FILE.cargoToml}: ${numero}`);

  const confronta = (file, trovata) => {
    if (trovata !== v) fuori.push(`${file}: ${trovata ?? "(nessuna)"} invece di ${v}`);
  };
  confronta(FILE.packageJson, JSON.parse(r.packageJson).version);
  confronta(FILE.tauriConf, JSON.parse(r.tauriConf).version);

  const lockNpm = JSON.parse(r.packageLock);
  confronta(`${FILE.packageLock} (radice)`, lockNpm.version);
  confronta(`${FILE.packageLock} (packages[""])`, lockNpm.packages?.[""]?.version);

  const nomi = [];
  for (const dove of membri(r.cargoToml)) {
    const manifesto = r.manifestiDeiMembri[`${dove}/Cargo.toml`];
    if (manifesto === undefined) {
      fuori.push(`${dove}/Cargo.toml: non c'è`);
      continue;
    }
    const { nome, eredita } = membro(manifesto);
    if (!nome) fuori.push(`${dove}/Cargo.toml: manca name in [package]`);
    else nomi.push(nome);
    // La regola che rende vero «in un posto solo»: un membro con un numero
    // suo è un sesto file da tenere allineato a mano.
    if (!eredita) fuori.push(`${dove}/Cargo.toml: la versione va ereditata (version.workspace = true)`);
  }
  const locali = new Map(
    vociDelLock(r.cargoLock)
      .filter((x) => x.pacchetto && x.locale)
      .map((x) => [x.nome, x.versione]),
  );
  for (const nome of nomi) {
    if (!locali.has(nome)) fuori.push(`${FILE.cargoLock}: manca il membro ${nome}`);
    else confronta(`${FILE.cargoLock} (${nome})`, locali.get(nome));
  }
  return fuori;
}

/** Gli stessi testi, con la versione `nuova` in tutti e cinque i file. Non scrive niente. */
function riscrivi(r, nuova) {
  const numero = numeroAmmesso(nuova);
  if (numero) throw new Error(numero);
  const i = r.cargoToml.indexOf("[workspace.package]");
  if (i < 0) throw new Error(`manca [workspace.package] in ${FILE.cargoToml}`);
  const cargoToml =
    r.cargoToml.slice(0, i) + r.cargoToml.slice(i).replace(/^version\s*=\s*"[^"]+"/m, `version = "${nuova}"`);

  const conVersione = (testo) => ({ ...JSON.parse(testo), version: nuova });
  const packageJson = riscriviCampo(r.packageJson, nuova, { rientro: 2 }, conVersione(r.packageJson), FILE.packageJson);
  const tauriConf = riscriviCampo(r.tauriConf, nuova, { rientro: 2 }, conVersione(r.tauriConf), FILE.tauriConf);

  // Le due righe del lockfile di npm, una alla volta, ciascuna confrontata con
  // quel che deve risultare: la radice, poi `packages[""]`.
  const lockNpm = JSON.parse(r.packageLock);
  const soloRadice = { ...lockNpm, version: nuova };
  const atteso = { ...soloRadice, packages: { ...lockNpm.packages, "": { ...lockNpm.packages[""], version: nuova } } };
  let packageLock = riscriviCampo(r.packageLock, nuova, { rientro: 2 }, soloRadice, FILE.packageLock);
  packageLock = riscriviCampo(packageLock, nuova, { rientro: 6, dopo: '\n    "": {' }, atteso, FILE.packageLock);

  const nomi = new Set(membri(r.cargoToml).map((d) => membro(r.manifestiDeiMembri[`${d}/Cargo.toml`] ?? "").nome));
  const cargoLock = vociDelLock(r.cargoLock)
    .map((x) =>
      x.pacchetto && x.locale && nomi.has(x.nome) ? x.testo.replace(/^version = "[^"]+"$/m, `version = "${nuova}"`) : x.testo,
    )
    .join("");

  return { ...r, cargoToml, packageJson, tauriConf, packageLock, cargoLock };
}

/* ── il repository ────────────────────────────────────────────────────── */

function leggi(rel) {
  return fs.readFileSync(path.join(REPO, rel), "utf8");
}

/** I cinque testi del repository vero, e i manifesti dei membri. */
function leggiTutto() {
  const r = Object.fromEntries(Object.entries(FILE).map(([k, f]) => [k, leggi(f)]));
  r.manifestiDeiMembri = {};
  for (const dove of membri(r.cargoToml)) {
    const p = `${dove}/Cargo.toml`;
    if (fs.existsSync(path.join(REPO, p))) r.manifestiDeiMembri[p] = leggi(p);
  }
  return r;
}

function main() {
  const chiesta = process.argv[2];
  const r = leggiTutto();

  if (chiesta === undefined) {
    const v = versioneRadice(r.cargoToml);
    const problemi = discordanze(r);
    if (problemi.length) {
      console.error(
        problemi.join("\n") + `\n\nLe versioni non coincidono. Allinearle con:\n  node strumenti/versione.js ${v}`,
      );
      process.exit(1);
    }
    console.log(`${v}: la stessa in ${Object.values(FILE).join(", ")} e nei ${membri(r.cargoToml).length} membri.`);
    return;
  }

  let nuovi;
  try {
    nuovi = riscrivi(r, chiesta);
  } catch (e) {
    console.error(e.message);
    process.exit(1);
  }
  for (const [k, f] of Object.entries(FILE)) {
    if (nuovi[k] !== r[k]) fs.writeFileSync(path.join(REPO, f), nuovi[k]);
  }
  console.log(`Versione ${chiesta} scritta in ${Object.values(FILE).join(", ")}: vanno nello stesso commit.`);
}

if (require.main === module) main();

module.exports = { FORMA, codiceAndroid, numeroAmmesso, discordanze, riscrivi, leggiTutto, membri, versioneRadice };
