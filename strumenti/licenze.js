/*
 * Genera `THIRD-PARTY-NOTICES.md` dalle licenze vere dei crate che Aether usa.
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
 * Uso:
 *   node strumenti/licenze.js [piattaforma]
 *
 * La piattaforma vale `x86_64-pc-windows-msvc` se non si dice altro: è il solo
 * bersaglio che l'installer produce oggi.
 */
const { execFileSync } = require("child_process");
const crypto = require("crypto");
const fs = require("fs");
const path = require("path");

// Come in `versione.js`: i percorsi valgono rispetto a questo file.
process.chdir(path.dirname(__dirname));

const PIATTAFORMA = process.argv[2] || "x86_64-pc-windows-msvc";
const USCITA = "THIRD-PARTY-NOTICES.md";

/** I nomi con cui un crate chiama il proprio testo di licenza. */
const NOMI = /^(LICEN[SC]E|COPYING|NOTICE|UNLICENSE)([-._].*)?$/i;

/**
 * Quanto testo si accetta da un singolo file.
 *
 * Serve contro il caso in cui un crate chiami `LICENSE` qualcosa che non lo è.
 * Il testo Apache-2.0 sta in undici kilobyte, quindi trentadue è largo tre
 * volte quel che serve senza lasciar passare un file di dati.
 */
const MASSIMO = 32 * 1024;

function metadata() {
  const grezzo = execFileSync(
    "cargo",
    [
      "metadata",
      "--format-version",
      "1",
      "--filter-platform",
      PIATTAFORMA,
      "--all-features",
    ],
    { encoding: "utf8", maxBuffer: 256 * 1024 * 1024 },
  );
  return JSON.parse(grezzo);
}

/** I testi di licenza che un crate porta con sé, dal suo `Cargo.toml`. */
function testiDi(manifestPath) {
  const dir = path.dirname(manifestPath);
  let voci;
  try {
    voci = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return [];
  }
  const fuori = [];
  for (const voce of voci) {
    if (!voce.isFile() || !NOMI.test(voce.name)) continue;
    try {
      const testo = fs.readFileSync(path.join(dir, voce.name), "utf8");
      if (testo.trim() === "" || testo.length > MASSIMO) continue;
      fuori.push({ nome: voce.name, testo: testo.replace(/\r\n/g, "\n").trimEnd() });
    } catch {
      // Un file illeggibile non ferma la generazione: si nota nel rapporto
      // finale come «senza testo», che è la cosa da andare a guardare.
    }
  }
  fuori.sort((a, b) => a.nome.localeCompare(b.nome));
  return fuori;
}

function impronta(testo) {
  return crypto.createHash("sha256").update(testo).digest("hex").slice(0, 12);
}

function main() {
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

  for (const p of esterni) {
    const trovati = testiDi(p.manifest_path);
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
    "Aether is distributed under the MIT license (see `LICENSE`). The program,",
    "however, is not made only of the code written here: it incorporates other",
    "people's free software, and their licenses ask for the text to travel",
    "together with the binary. This file is that text.",
  );
  righe.push("");
  righe.push(
    "**It is not written by hand.** `strumenti/licenze.js` generates it from",
    "`cargo metadata`, reading the texts where `cargo` has already downloaded",
    "them. It is remade with:",
  );
  righe.push("");
  righe.push("```");
  righe.push("node strumenti/licenze.js");
  righe.push("```");
  righe.push("");
  righe.push(
    `The list covers the whole dependency graph for \`${PIATTAFORMA}\`,`,
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
    "Aether modifies none of them — it uses them as crates from the registry, at",
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
    "**sale** of encoders and decoders: Aether is free and sells nothing. There",
    "is therefore nothing to pay and nothing to ask for.",
  );
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

  if (senzaTesto.length > 0) {
    righe.push("### Crates with no license file in the package");
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

  righe.push("---");
  righe.push("");
  righe.push("## The texts");
  righe.push("");

  for (const [chiave, gruppo] of ordinati) {
    righe.push(`### Text \`${chiave}\` — ${gruppo.crate.length} crates`);
    righe.push("");
    righe.push("<details><summary>Which crates</summary>");
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
    `${USCITA}: ${esterni.length} crates, ${testi.size} distinct texts, ` +
      `${senzaTesto.length} with no text attached.`,
  );
}

main();
