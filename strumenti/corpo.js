/*
 * Il corpo della release su GitHub: la pagina che si legge prima di scaricare.
 *
 * # Perché uno script e non un blocco `body:` in `release.yml`
 *
 * Perché adesso la release porta due cose — l'installer per Windows e l'APK
 * per i telefoni — e il testo nomina file, versione e impronta del
 * certificato, che vanno presi dove stanno e non riscritti a mano a ogni tag.
 * E perché il corpo non lo legge solo chi apre la pagina: le copie di
 * Rymembra per Android installate prima di questa release lo mostrano come
 * «note» dell'aggiornamento (R11-1). Per le **build di prova** — firmate con
 * la chiave di debug, che non possono installare l'APK di rilascio sopra di
 * sé — è l'unico testo che arriva: per questo la loro sezione sta in testa,
 * prima di ogni istruzione per il computer (decisione D-5).
 *
 * # In che lingua
 *
 * Inglese americano, come tutto quel che sta su GitHub. I nomi dei menù sono
 * quelli di `apps/desktop/src/lingue/en.json`: `corpo.test.js` li confronta,
 * così un'etichetta cambiata nell'app non lascia qui istruzioni che portano
 * da nessuna parte.
 *
 *   node strumenti/corpo.js <file>      scrive il corpo della release della versione dei file
 */
const fs = require("fs");
const path = require("path");

const { nomeAsset } = require("./apk.js");

const REPO = path.dirname(__dirname);

/** Le etichette dell'app che il testo nomina, per chiave di `en.json`. */
const ETICHETTE = {
  sezione: "settings.section.dati",
  scheda: "settings.profile.title",
  esporta: "profile.export",
  leggi: "profile.read",
};

function leggi(rel) {
  return fs.readFileSync(path.join(REPO, rel), "utf8");
}

/** `owner/repo`: in CI lo dice GitHub, altrimenti `repository` di `Cargo.toml`. */
function deposito() {
  if (process.env.GITHUB_REPOSITORY) return process.env.GITHUB_REPOSITORY;
  const m = /^repository\s*=\s*"https:\/\/github\.com\/([^"]+)"/m.exec(leggi("Cargo.toml"));
  if (!m) throw new Error("Non so su quale repository sto pubblicando.");
  return m[1].replace(/\/$/, "");
}

/** Il testo, dati versione, repository, nomi dei file, impronta ed etichette. */
function corpo({ versione, repo, installer, apk, impronta, etichette }) {
  const menu = (azione) => `**Settings → ${etichette.sezione} → ${etichette.scheda} → ${etichette[azione]}**`;
  return `## Rymembra ${versione}

What changed is in [CHANGELOG.md](https://github.com/${repo}/blob/v${versione}/CHANGELOG.md).

### If you have a test build of the Android app

Test builds were signed with a different key: Android won’t install this version over them, and updating from inside the app fails every time. To move to this version without losing your library:

1. In Rymembra on the phone: ${menu("esporta")}, and save the file outside the app — in Downloads, or in a cloud folder.
2. Uninstall Rymembra.
3. Install \`${apk}\` from this page.
4. ${menu("leggi")}, and choose the file you saved.

The profile carries your settings, play counts, ratings, favorites, resume positions, playlists, listening history, metadata corrections, lyrics, the tracks to buy, covers, skins and watched folders. It does **not** carry:

- the **connected accounts** — Google Drive, OneDrive, Dropbox, Last.fm, ListenBrainz and the keys of AI models: sign in again;
- the **linked devices**: on each computer, forget the old phone and link the phone again; the same for the devices you listen together with;
- the permissions Android gave the old app: Rymembra asks for them again.

Your music stays where it is on the phone: after reading the profile, scan your folders again.

### Windows

Download \`${installer}\`. Copies already installed update by themselves: Rymembra shows the update and installs it when you say so.

**This installer is not signed.** Windows SmartScreen will show a warning: “Windows protected your PC”. To proceed: *More info* → *Run anyway*.

It isn’t a formality and shouldn’t be taken for granted. An unsigned installer is an installer whose provenance you cannot verify, and the only thing I can offer in exchange is that the source is here and builds itself. An OV certificate requires identity validation and an annual cost: when there is one, this warning will disappear from these notes.

**Coming from Aether**, Rymembra’s previous name: this installer removes it by itself and leaves its data alone. The first time Rymembra starts, the library, settings, skins, drafts, accounts and sync of Aether carry over automatically. If Aether is open, close it first (also from the icon next to the clock): the installer waits a few seconds, then leaves it installed rather than closing it under you.

### Android

Download \`${apk}\`: Android 8 or later, on a 64-bit phone — almost every phone of the last years. Devices that are only 32-bit aren’t supported.

- The first time, Android asks you to allow your browser or file manager to install unknown apps, and Play Protect may offer to scan an app it hasn’t seen before.
- Later updates arrive from inside the app; the first time, Android asks you to allow Rymembra to install apps.
- The APK is signed with the certificate whose SHA-256 fingerprint is \`${impronta}\`. To check it: \`apksigner verify --print-certs ${apk}\`. The \`.apk.sig\` next to it is the same kind of signature that Rymembra on the computer checks before installing an update.
- Android Auto shows an app installed outside the Play Store only after *Android Auto → developer settings → Unknown sources* is switched on.

### Where the music comes from

Rymembra plays your files, and looks for what’s missing in the **free catalogs**. It doesn’t download from YouTube and doesn’t scrape Spotify: the why is in \`README.md\`. What no free catalog has ends up in a shopping list, not in an error.

### Licenses

\`LICENSE\` (MIT), \`THIRD-PARTY-NOTICES.md\` (the dependencies, with their texts), \`PRIVACY.md\` (every network request the program can make), \`TERMS.md\` (what you can do with the music you download — including the Live Music Archive’s **non-commercial** constraint).
`;
}

/** Il corpo della release della versione dei file, letto dal repository. */
function corpoDelRepository() {
  const versione = JSON.parse(leggi("apps/desktop/package.json")).version;
  const prodotto = JSON.parse(leggi("apps/desktop/src-tauri/tauri.conf.json")).productName;
  const en = JSON.parse(leggi("apps/desktop/src/lingue/en.json"));
  const etichette = {};
  for (const [nome, chiave] of Object.entries(ETICHETTE)) {
    if (typeof en[chiave] !== "string") throw new Error(`en.json: manca «${chiave}», che il corpo della release nomina`);
    etichette[nome] = en[chiave];
  }
  return corpo({
    versione,
    repo: deposito(),
    // Il nome che il bundler NSIS dà all'installer (vedi `sito.js`, `rilascio`).
    installer: `${prodotto}_${versione}_x64-setup.exe`,
    apk: nomeAsset(versione, prodotto),
    impronta: fs.readFileSync(path.join(__dirname, "android-certificato.sha256"), "utf8").trim(),
    etichette,
  });
}

function main() {
  const file = process.argv[2];
  if (!file) {
    console.error("uso: node strumenti/corpo.js <file>");
    process.exit(2);
  }
  fs.mkdirSync(path.dirname(path.resolve(file)), { recursive: true });
  fs.writeFileSync(file, corpoDelRepository());
  console.log(`${file}: il corpo della release.`);
}

if (require.main === module) main();

module.exports = { ETICHETTE, corpo, corpoDelRepository };
