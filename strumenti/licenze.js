/*
 * Genera `THIRD-PARTY-NOTICES.md` dalle licenze vere dei crate e dei pacchetti
 * npm che Rymembra usa.
 *
 * # Perché uno script e non un file scritto a mano
 *
 * Perché un elenco di seicento dipendenze scritto a mano è un elenco sbagliato
 * dal giorno dopo. Le licenze cambiano fra una versione e l'altra — è successo
 * con `unicode-ident`, passato a `Unicode-3.0` — e un obbligo di attribuzione
 * che si aggiorna a mano è un obbligo che a un certo punto smette di essere
 * rispettato senza che nessuno se ne accorga.
 *
 * # Perché non `cargo about`
 *
 * Perché fa la stessa cosa e chiede di installare un binario in più su ogni
 * macchina che vuole ricostruire il file. Questo legge `cargo metadata`, va a
 * prendere i testi dove `cargo` li ha già scaricati, e non ha dipendenze.
 *
 * # Cosa produce, e perché è un sovrainsieme
 *
 * L'elenco copre **tutto** il grafo delle dipendenze per la piattaforma
 * indicata, compreso quel che serve solo a compilare o a provare. È più di
 * quanto finisca nel binario, ed è deliberato: distinguere il sottoinsieme
 * esatto richiede il grafo risolto per profilo, e attribuire in più non fa male
 * a nessuno mentre attribuire in meno è la sola cosa che si paga.
 *
 * # I pacchetti npm
 *
 * L'interfaccia è un pacchetto Vite: React, CodeMirror, Lezer e l'API di Tauri
 * finiscono dentro il binario come JavaScript. Si leggono da
 * `apps/desktop/package-lock.json`, solo le voci che non sono `dev` — gli
 * strumenti con cui si costruisce (Vite, TypeScript) non viaggiano col
 * programma — e i testi dalle cartelle di `node_modules` dove `npm` li ha già
 * messi. Senza `npm ci` fatto prima, i pacchetti si elencano lo stesso e
 * finiscono fra quelli «senza testo»: è il segnale di andarlo a fare.
 *
 * # Le piattaforme
 *
 * Di serie l'unione dei grafi dei due bersagli che escono: l'installer per
 * Windows (`x86_64-pc-windows-msvc`) e l'APK per i telefoni
 * (`aarch64-linux-android`, che ha le stesse dipendenze dell'emulatore
 * `x86_64`). Un file per due binari, perché i due elenchi sono quasi uguali e
 * uno che ne attribuisce uno solo lascia scoperto l'altro.
 *
 * Le librerie Kotlin e Java che Gradle mette nell'APK — AndroidX, CameraX,
 * Media3, la Car App Library, ZXing — e le icone delle schede di Android Auto non passano da
 * `cargo` né da `npm`: le dice un paragrafo scritto qui sotto, da aggiornare
 * quando cambia `gen/android/app/build.gradle.kts`.
 *
 * Uso:
 *   node strumenti/licenze.js [piattaforma…]
 */
const { execFileSync } = require("child_process");
const crypto = require("crypto");
const fs = require("fs");
const path = require("path");

const PIATTAFORME =
  require.main === module && process.argv.length > 2
    ? process.argv.slice(2)
    : ["x86_64-pc-windows-msvc", "aarch64-linux-android"];
const USCITA = "THIRD-PARTY-NOTICES.md";

/** I nomi con cui un crate chiama il proprio testo di licenza. */
const NOMI = /^(LICEN[SC]E|COPYING|NOTICE|UNLICENSE)([-._].*)?$/i;

/**
 * Quanto testo si accetta da un singolo file.
 *
 * Serve contro il caso in cui un crate chiami `LICENSE` qualcosa che non lo è.
 * Era trentadue kilobyte, «tre volte il testo Apache-2.0», ed era stretto: il
 * `LICENSE-THIRD-PARTY` di `crossbeam-channel` (33 447 byte: il CC BY 3.0 e la
 * BSD di Go che il crate riporta) restava fuori dal file senza che nessuno lo
 * dicesse. Duecentocinquantasei coprono ogni testo di licenza vero; e quel che
 * resta sopra non si scarta più in silenzio (vedi `main`).
 */
const MASSIMO = 256 * 1024;

/** Dove sta l'interfaccia, e il suo lockfile. */
const INTERFACCIA = path.join("apps", "desktop");

/**
 * Il paragrafo sulle librerie Kotlin e Java dell'APK: testo fisso, perché Gradle
 * non passa da `cargo` né da `npm`.
 *
 * Da aggiornare quando cambia `gen/android/app/build.gradle.kts`: la prova
 * `ogni dipendenza gradle diretta è nominata nel paragrafo Android`
 * (`licenze.test.js`) lo ricorda. Nomina anche quel che le dipendenze portano con
 * sé (Guava, jspecify, Jackson, le annotazioni del Checker Framework), perché la
 * lista di sola lettura dei crate non le vede, e dà il testo della licenza che
 * non è Apache-2.0 (`checker-qual` è MIT, e lo vuole scritto). Con R8 l'APK di
 * rilascio perde quel che non usa, quindi non tutto sta in ogni APK: si elenca
 * lo stesso, perché attribuire in più non costa niente e in meno sì.
 */
const PARAGRAFO_ANDROID = [
  "## The Android app",
  "",
  "The APK also carries Kotlin and Java libraries that Gradle adds and that",
  "this list does not read. The ones Rymembra asks for: **AndroidX** (`webkit`,",
  "`appcompat`, `activity-ktx`, `lifecycle-process`), **CameraX**",
  "(`camera-camera2`, `camera-lifecycle`, `camera-view`: the QR scanner that",
  "links a device), **Media3** (`media3-session` and `media3-common`: the player",
  "service, its notification and Android Auto), the",
  "**Car App Library** (`app`: only",
  "`CarConnection`, to know when the phone is connected to Android Auto),",
  "**ZXing** (`core`: reading the QR code), **Material Components for Android**",
  "and the **Kotlin standard library**. And what they bring with them:",
  "**Guava**, **jspecify**, the pieces of **AndroidX** they build on",
  "(`activity`, `annotation`, `core`, `lifecycle`, `media`), the annotations of",
  "the **Checker Framework** (`checker-qual`, which the Car App Library brings",
  "through Guava) and **Jackson**, which comes with Tauri's Android library. R8",
  "removes from the release APK what is not used, so not every one of them is in",
  "every build; they are all listed, because attributing too much costs nothing",
  "and attributing too little does.",
  "",
  "All of them are under **Apache-2.0**, whose text is below, except two. The",
  "annotations of the Checker Framework (`checker-qual`) are under the **MIT**",
  "license, whose text follows. And `jackson-core` carries two small parts of its",
  "own, whose notices travel in its `META-INF`: **FastDoubleParser** (MIT,",
  "copyright 2023 Werner Randelshofer) and a big-integer implementation by Tim",
  "Buktu (BSD-2-Clause). Sources:",
  "<https://android.googlesource.com/platform/frameworks/support>,",
  "<https://github.com/androidx/media>, <https://github.com/zxing/zxing>,",
  "<https://github.com/material-components/material-components-android>,",
  "<https://github.com/JetBrains/kotlin>, <https://github.com/google/guava>,",
  "<https://github.com/typetools/checker-framework> and",
  "<https://github.com/FasterXML/jackson-core>.",
  "",
  "```text",
  "Checker Framework qualifiers",
  "Copyright 2004-present by the Checker Framework developers",
  "",
  "MIT License:",
  "",
  "Permission is hereby granted, free of charge, to any person obtaining a copy",
  'of this software and associated documentation files (the "Software"), to deal',
  "in the Software without restriction, including without limitation the rights",
  "to use, copy, modify, merge, publish, distribute, sublicense, and/or sell",
  "copies of the Software, and to permit persons to whom the Software is",
  "furnished to do so, subject to the following conditions:",
  "",
  "The above copyright notice and this permission notice shall be included in",
  "all copies or substantial portions of the Software.",
  "",
  'THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR',
  "IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,",
  "FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE",
  "AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER",
  "LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,",
  "OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN",
  "THE SOFTWARE.",
  "```",
  "",
  "The six icons of the Android Auto tabs (`res/drawable/auto_*.xml`) are",
  "drawn from Google's **Material Icons** — `history`, `queue_music`,",
  "`album`, `person`, `favorite`, `play_circle` — under **Apache-2.0**:",
  "<https://github.com/google/material-design-icons>.",
];

/** `cargo metadata` per una piattaforma. */
function metadataPer(piattaforma) {
  const grezzo = execFileSync(
    "cargo",
    [
      "metadata",
      "--format-version",
      "1",
      "--filter-platform",
      piattaforma,
      "--all-features",
      // Come ogni altro `cargo` della CI: in `costruisci` questo gira prima
      // di `tauri build --locked`, e un lockfile riscritto qui in silenzio
      // lo renderebbe cieco.
      "--locked",
    ],
    { encoding: "utf8", maxBuffer: 256 * 1024 * 1024 },
  );
  return JSON.parse(grezzo);
}

/** L'unione dei pacchetti di tutte le piattaforme, ciascuno una volta. */
function metadata() {
  const pacchetti = new Map();
  const membri = new Set();
  for (const piattaforma of PIATTAFORME) {
    const m = metadataPer(piattaforma);
    for (const id of m.workspace_members) membri.add(id);
    for (const p of m.packages) pacchetti.set(p.id, p);
  }
  return { workspace_members: [...membri], packages: [...pacchetti.values()] };
}

/**
 * I testi di licenza che un crate porta con sé, dal suo `Cargo.toml`, e i file
 * che sembrano di licenza ma passano il tetto (`scartati`, con nome e peso).
 */
function testiDi(manifestPath) {
  const dir = path.dirname(manifestPath);
  let voci;
  try {
    voci = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return { trovati: [], scartati: [] };
  }
  const trovati = [];
  const scartati = [];
  for (const voce of voci) {
    if (!voce.isFile() || !NOMI.test(voce.name)) continue;
    try {
      const testo = fs.readFileSync(path.join(dir, voce.name), "utf8");
      if (testo.trim() === "") continue;
      if (testo.length > MASSIMO) {
        scartati.push({ nome: voce.name, byte: Buffer.byteLength(testo) });
        continue;
      }
      trovati.push({ nome: voce.name, testo: testo.replace(/\r\n/g, "\n").trimEnd() });
    } catch {
      // Un file illeggibile non ferma la generazione: si nota nel rapporto
      // finale come «senza testo», che è la cosa da andare a guardare.
    }
  }
  trovati.sort((a, b) => a.nome.localeCompare(b.nome));
  scartati.sort((a, b) => a.nome.localeCompare(b.nome));
  return { trovati, scartati };
}

/** Un indirizzo di repository come lo scrive un `package.json`, ripulito. */
function repositoryNpm(dir) {
  try {
    const manifesto = JSON.parse(fs.readFileSync(path.join(dir, "package.json"), "utf8"));
    const grezzo =
      typeof manifesto.repository === "string" ? manifesto.repository : manifesto.repository?.url;
    if (typeof grezzo !== "string") return null;
    return grezzo.replace(/^git\+/, "").replace(/\.git$/, "").replace(/^git:\/\//, "https://");
  } catch {
    return null;
  }
}

/**
 * I pacchetti npm che finiscono nel programma: le voci del lockfile che non
 * servono solo a costruire.
 */
function pacchettiNpm() {
  const lock = JSON.parse(fs.readFileSync(path.join(INTERFACCIA, "package-lock.json"), "utf8"));
  return Object.entries(lock.packages ?? {})
    .filter(([dove, voce]) => dove.startsWith("node_modules/") && !voce.dev && !voce.devOptional)
    .map(([dove, voce]) => {
      const dir = path.join(INTERFACCIA, dove);
      return {
        name: voce.name ?? dove.slice(dove.lastIndexOf("node_modules/") + "node_modules/".length),
        version: voce.version ?? "?",
        license: voce.license ?? null,
        repository: repositoryNpm(dir),
        manifest_path: path.join(dir, "package.json"),
      };
    })
    .sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));
}

function impronta(testo) {
  return crypto.createHash("sha256").update(testo).digest("hex").slice(0, 12);
}

function main() {
  // Come in `versione.js`: i percorsi valgono rispetto a questo file.
  process.chdir(path.dirname(__dirname));
  const m = metadata();
  const nostri = new Set(m.workspace_members);
  const esterni = m.packages
    .filter((p) => !nostri.has(p.id))
    .sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));

  // I testi si raccolgono una volta sola: la licenza MIT di `serde` e quella di
  // `syn` sono lo stesso file tranne il nome del titolare, ma centinaia di
  // copie identiche di Apache-2.0 sono centinaia di copie identiche.
  const testi = new Map(); // impronta → { testo, crate: [] }
  const senzaTesto = [];
  const fuoriTetto = []; // { pacchetto, nome, byte }
  const npm = pacchettiNpm();

  for (const p of [...esterni, ...npm]) {
    const { trovati, scartati } = testiDi(p.manifest_path);
    for (const s of scartati) fuoriTetto.push({ pacchetto: `${p.name} ${p.version}`, ...s });
    if (trovati.length === 0) {
      senzaTesto.push(p);
      continue;
    }
    for (const t of trovati) {
      const chiave = impronta(t.testo);
      if (!testi.has(chiave)) testi.set(chiave, { testo: t.testo, crate: [] });
      const gruppo = testi.get(chiave);
      const etichetta = `${p.name} ${p.version}`;
      if (!gruppo.crate.includes(etichetta)) gruppo.crate.push(etichetta);
    }
  }

  const ordinati = [...testi.entries()].sort(
    (a, b) => b[1].crate.length - a[1].crate.length,
  );

  const righe = [];
  righe.push("# Third-party component notices");
  righe.push("");
  righe.push(
    "Rymembra is distributed under the MIT license (see `LICENSE`). The program,",
    "however, is not made only of the code written here: it incorporates other",
    "people's free software, and their licenses ask for the text to travel",
    "together with the binary. This file is that text.",
  );
  righe.push("");
  righe.push(
    "**It is not written by hand.** `strumenti/licenze.js` generates it from",
    "`cargo metadata` and from the production entries of",
    "`apps/desktop/package-lock.json`, reading the texts where `cargo` and `npm`",
    "have already downloaded them. It is remade with:",
  );
  righe.push("");
  righe.push("```");
  righe.push("node strumenti/licenze.js");
  righe.push("```");
  righe.push("");
  righe.push(
    `The Rust list covers the whole dependency graph for ${PIATTAFORME.map((p) => `\`${p}\``).join(" and ")},`,
    "including what is needed only to build or to test. It is more than what",
    "ends up in the binary, and that is deliberate: over-attributing costs",
    "nobody anything, under-attributing is the only thing you pay for.",
  );
  righe.push("");
  righe.push("## The two licenses to look at first");
  righe.push("");
  righe.push(
    "**MPL-2.0** — the whole `symphonia` family, which is the audio decoding",
    "engine, plus `cssparser` and `selectors`. The MPL is *file-based*: it",
    "requires the source of the **covered files** you modify to be distributed.",
    "Rymembra modifies none of them — it uses them as crates from the registry, at",
    "the versions written in `Cargo.lock` — so the obligation is discharged by",
    "saying so and pointing at where the original is. If one were ever forked,",
    "that fork would have to be published.",
  );
  righe.push("");
  righe.push(
    "**Apache-2.0 with no alternative** — `cpal` (the audio device), `tao` (the",
    "window), `ring` (`Apache-2.0 AND ISC`), `zopfli`, `sync_wrapper`. Almost",
    "all the rest of the Rust ecosystem is `MIT OR Apache-2.0`, and for those",
    "crates you can choose; for these you cannot. Apache-2.0 § 4 asks for a copy",
    "of the license to be delivered to whoever receives the program and for the",
    "`NOTICE` notices to be kept: both are below.",
  );
  righe.push("");
  righe.push(
    "**Unicode-3.0** — the `icu4x` and `unicode-ident` crates. A permissive",
    "license with an attribution requirement, discharged by this file.",
  );
  righe.push("");
  righe.push("## The interface");
  righe.push("");
  righe.push(
    "The window is a web page bundled into the program: **React**, **CodeMirror**",
    "(the Skin Studio's code editor), **Lezer** and Tauri's JavaScript API travel",
    "inside it as JavaScript. They are listed after the crates; only the",
    "packages that ship count, not the tools used to build (Vite, TypeScript).",
  );
  righe.push("");
  righe.push("## The fonts");
  righe.push("");
  righe.push(
    "**Geist** and **Bricolage Grotesque** are packaged inside the application,",
    "under the **SIL Open Font License 1.1**, whose text is in",
    "`apps/desktop/src/font/OFL.txt`. The OFL requires the license to be",
    "distributed together with the fonts, and forbids selling them on their",
    "own: neither is a problem here, but the first has to be done and until",
    "recently it was not.",
  );
  righe.push("");
  righe.push("## The audio");
  righe.push("");
  righe.push(
    "**AAC** decoding happens through `symphonia-codec-aac`, which is an",
    "independent implementation. The fundamental patents on AAC-LC expired",
    "between 2017 and 2023, and the Via LA licensing program bills on the",
    "**sale** of encoders and decoders: Rymembra is free and sells nothing. There",
    "is therefore nothing to pay and nothing to ask for.",
  );
  righe.push("");
  righe.push(...PARAGRAFO_ANDROID);
  righe.push("");
  righe.push("---");
  righe.push("");
  righe.push(`## The list: ${esterni.length} crates`);
  righe.push("");
  righe.push(
    "Identical texts are collected once only, with the list of the crates that",
    "carry them underneath. Anyone looking for a specific crate finds it with",
    "their reader's search: it appears in its license's group.",
  );
  righe.push("");

  // La tabella dei crate, per chi cerca il singolo nome.
  righe.push("| Crate | Version | License | Where it lives |");
  righe.push("| --- | --- | --- | --- |");
  for (const p of esterni) {
    const repo = p.repository ? `<${p.repository}>` : "—";
    righe.push(
      `| \`${p.name}\` | ${p.version} | ${p.license || "(not declared)"} | ${repo} |`,
    );
  }
  righe.push("");

  righe.push(`## The interface: ${npm.length} npm packages`);
  righe.push("");
  righe.push("| Package | Version | License | Where it lives |");
  righe.push("| --- | --- | --- | --- |");
  for (const p of npm) {
    const repo = p.repository ? `<${p.repository}>` : "—";
    righe.push(`| \`${p.name}\` | ${p.version} | ${p.license || "(not declared)"} | ${repo} |`);
  }
  righe.push("");

  if (senzaTesto.length > 0) {
    righe.push("### Packages with no license file");
    righe.push("");
    righe.push(
      "They declare the license in the manifest but do not attach its text. The",
      "standard text of the declared license applies, reproduced further down",
      "for the other crates that use it.",
    );
    righe.push("");
    for (const p of senzaTesto) {
      righe.push(`- \`${p.name}\` ${p.version} — ${p.license || "(not declared)"}`);
    }
    righe.push("");
  }

  // Un file che sembra di licenza e passa il tetto: si dice qui, e lo script
  // esce con 1 perché la CI lo veda. Attribuire in meno è la sola cosa che si
  // paga, e scartarlo in silenzio era esattamente quello.
  if (fuoriTetto.length > 0) {
    righe.push("### Licence files not reproduced");
    righe.push("");
    righe.push(
      `Larger than ${MASSIMO / 1024} KiB, which no licence text is: read them in the`,
      "package itself.",
    );
    righe.push("");
    for (const f of fuoriTetto) righe.push(`- \`${f.pacchetto}\` — \`${f.nome}\`, ${f.byte} bytes`);
    righe.push("");
  }

  righe.push("---");
  righe.push("");
  righe.push("## The texts");
  righe.push("");

  for (const [chiave, gruppo] of ordinati) {
    righe.push(`### Text \`${chiave}\` — ${gruppo.crate.length} packages`);
    righe.push("");
    righe.push("<details><summary>Which packages</summary>");
    righe.push("");
    for (const c of gruppo.crate) righe.push(`- \`${c}\``);
    righe.push("");
    righe.push("</details>");
    righe.push("");
    righe.push("```");
    righe.push(gruppo.testo);
    righe.push("```");
    righe.push("");
  }

  fs.writeFileSync(USCITA, righe.join("\n") + "\n");
  console.log(
    `${USCITA}: ${esterni.length} crates, ${npm.length} npm packages, ${testi.size} distinct texts, ` +
      `${senzaTesto.length} with no text attached.`,
  );
  if (fuoriTetto.length > 0) {
    console.error(
      `${fuoriTetto.length} file di licenza oltre ${MASSIMO / 1024} KiB, non riprodotti:\n` +
        fuoriTetto.map((f) => `  ${f.pacchetto}: ${f.nome} (${f.byte} byte)`).join("\n") +
        "\nGuardarli: se sono testi di licenza veri, il tetto (MASSIMO) va alzato.",
    );
    process.exit(1);
  }
}

// Come script rigenera il file; come modulo presta la lettura dei testi a
// `licenze.test.js`, senza `cargo` e senza scrivere niente.
if (require.main === module) main();

module.exports = { testiDi, MASSIMO, NOMI, PARAGRAFO_ANDROID };
