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
  righe.push("# Avvisi sulle componenti di terze parti");
  righe.push("");
  righe.push(
    "Aether è distribuito sotto licenza MIT (vedi `LICENSE`). Il programma però",
    "non è fatto solo del codice scritto qui: incorpora software libero di altri,",
    "e le loro licenze chiedono che il testo viaggi insieme al binario. Questo",
    "file è quel testo.",
  );
  righe.push("");
  righe.push(
    "**Non è scritto a mano.** Lo genera `strumenti/licenze.js` da `cargo",
    "metadata`, andando a leggere i testi dove `cargo` li ha già scaricati. Si",
    "rifà con:",
  );
  righe.push("");
  righe.push("```");
  righe.push("node strumenti/licenze.js");
  righe.push("```");
  righe.push("");
  righe.push(
    `L'elenco copre l'intero grafo delle dipendenze per \`${PIATTAFORMA}\`,`,
    "compreso quel che serve solo a compilare o a provare. È più di quanto",
    "finisca nel binario, ed è deliberato: attribuire in più non costa niente a",
    "nessuno, attribuire in meno è la sola cosa che si paga.",
  );
  righe.push("");
  righe.push("## Le due licenze da guardare per prime");
  righe.push("");
  righe.push(
    "**MPL-2.0** — tutta la famiglia `symphonia`, che è il motore di decodifica",
    "audio, più `cssparser` e `selectors`. La MPL è *file-based*: obbliga a",
    "distribuire il sorgente dei **file coperti** che si modificano. Aether non",
    "ne modifica nessuno — li usa come crate dal registro, alle versioni scritte",
    "in `Cargo.lock` — quindi l'obbligo si assolve dicendolo e indicando dove sta",
    "l'originale. Se un giorno se ne forcasse uno, quel fork andrebbe pubblicato.",
  );
  righe.push("");
  righe.push(
    "**Apache-2.0 senza alternativa** — `cpal` (il dispositivo audio), `tao` (la",
    "finestra), `ring` (`Apache-2.0 AND ISC`), `zopfli`, `sync_wrapper`. Quasi",
    "tutto il resto dell'ecosistema Rust è `MIT OR Apache-2.0`, e per quei crate",
    "si può scegliere; per questi no. La § 4 della Apache-2.0 chiede di",
    "consegnare una copia della licenza a chi riceve il programma e di conservare",
    "gli avvisi `NOTICE`: entrambe le cose stanno qui sotto.",
  );
  righe.push("");
  righe.push(
    "**Unicode-3.0** — i crate `icu4x` e `unicode-ident`. Licenza permissiva con",
    "obbligo di attribuzione, assolto da questo file.",
  );
  righe.push("");
  righe.push("## I font");
  righe.push("");
  righe.push(
    "**Geist** e **Bricolage Grotesque** sono impacchettati dentro",
    "l'applicazione, sotto **SIL Open Font License 1.1**, il cui testo sta in",
    "`apps/desktop/src/font/OFL.txt`. La OFL obbliga a distribuire la licenza",
    "insieme ai font, e a non venderli da soli: nessuna delle due cose è un",
    "problema qui, ma la prima va fatta e fino a poco fa non si faceva.",
  );
  righe.push("");
  righe.push("## L'audio");
  righe.push("");
  righe.push(
    "La decodifica **AAC** avviene tramite `symphonia-codec-aac`, che è",
    "un'implementazione indipendente. I brevetti fondamentali su AAC-LC sono",
    "scaduti fra il 2017 e il 2023, e il programma di licenza Via LA fattura",
    "sulla **vendita** di codificatori e decodificatori: Aether è gratuito e non",
    "vende nulla. Non c'è quindi niente da pagare né da chiedere.",
  );
  righe.push("");
  righe.push("---");
  righe.push("");
  righe.push(`## L'elenco: ${esterni.length} crate`);
  righe.push("");
  righe.push(
    "I testi identici sono raccolti una volta sola, con sotto l'elenco dei crate",
    "che li portano. Chi cerca un crate preciso lo trova con la ricerca del",
    "proprio lettore: compare nel gruppo della sua licenza.",
  );
  righe.push("");

  // La tabella dei crate, per chi cerca il singolo nome.
  righe.push("| Crate | Versione | Licenza | Dove sta |");
  righe.push("| --- | --- | --- | --- |");
  for (const p of esterni) {
    const repo = p.repository ? `<${p.repository}>` : "—";
    righe.push(
      `| \`${p.name}\` | ${p.version} | ${p.license || "(non dichiarata)"} | ${repo} |`,
    );
  }
  righe.push("");

  if (senzaTesto.length > 0) {
    righe.push("### Crate senza un file di licenza nel pacchetto");
    righe.push("");
    righe.push(
      "Dichiarano la licenza nel manifesto ma non ne allegano il testo. Vale il",
      "testo standard della licenza dichiarata, riportato più sotto per gli altri",
      "crate che la usano.",
    );
    righe.push("");
    for (const p of senzaTesto) {
      righe.push(`- \`${p.name}\` ${p.version} — ${p.license || "(non dichiarata)"}`);
    }
    righe.push("");
  }

  righe.push("---");
  righe.push("");
  righe.push("## I testi");
  righe.push("");

  for (const [chiave, gruppo] of ordinati) {
    righe.push(`### Testo \`${chiave}\` — ${gruppo.crate.length} crate`);
    righe.push("");
    righe.push("<details><summary>Quali crate</summary>");
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
    `${USCITA}: ${esterni.length} crate, ${testi.size} testi distinti, ` +
      `${senzaTesto.length} senza testo allegato.`,
  );
}

main();
