/*
 * Il `latest.json` che l'updater interroga.
 *
 * # Cos'è
 *
 * Trecento byte su GitHub, e l'unica cosa che decide se una copia di Aether
 * installata da qualche parte si aggiornerà o no. `tauri-plugin-updater` lo
 * scarica ogni mezz'ora, confronta `version` con la propria, e se è più nuova
 * prende `platforms[…].url`, ne verifica la firma con la chiave compilata
 * dentro l'eseguibile, e installa.
 *
 * # Perché uno script e non tre righe di `yaml`
 *
 * Perché due delle quattro cose che ci vanno dentro non si possono scrivere a
 * mano senza sbagliarle prima o poi:
 *
 *   - `signature` è il **contenuto** del file `.sig`, non il suo percorso. È
 *     l'errore che si fa una volta sola, e il sintomo è un updater che scarica
 *     settanta megabyte e li butta via con un errore di base64.
 *   - `url` deve puntare all'asset di *questa* release, col nome esatto che il
 *     bundler ha dato all'installer — nome che contiene la versione, e che
 *     quindi cambia a ogni volta.
 *
 * E soprattutto c'è una cosa che nessuno può verificare a occhio, ed è quella
 * per cui vale la pena avere uno script: che la chiave privata con cui la CI ha
 * appena firmato sia **la stessa** di cui gli eseguibili già installati hanno la
 * metà pubblica. Se non lo è, `latest.json` è perfetto, l'installer si scarica,
 * la firma non torna e nessuno si aggiorna — e non lo scopre nessuno, perché il
 * guasto succede sul computer di altri. Qui si confrontano gli otto byte di
 * identificativo che minisign mette sia nella chiave sia nella firma, e la
 * release non parte se non coincidono.
 *
 * # Un modo
 *
 *   node strumenti/manifesto.js [cartella]     scrive <cartella>/latest.json
 *
 * Senza argomenti scrive in `uscita/`, che è dove `release.yml` raccoglie.
 */
const fs = require("fs");
const path = require("path");

// I percorsi si risolvono rispetto a **questo file**: la stessa ragione per cui
// lo fanno `versione.js` e `lingue.js`.
const REPO = path.dirname(__dirname);
process.chdir(REPO);

const PACCHETTO = "apps/desktop/package.json";
const TAURI = "apps/desktop/src-tauri/tauri.conf.json";
const CHANGELOG = "CHANGELOG.md";
const BUNDLE = "target/release/bundle/nsis";

/**
 * Il bersaglio che il plugin cerca dentro `platforms`.
 *
 * Uno solo, e non è una semplificazione: `bundle.targets` è `["nsis"]`, cioè
 * Windows e basta. Il giorno in cui ci sarà un `.dmg` questa riga diventerà un
 * elenco, e sarà un cambiamento visibile invece di un `platforms` vuoto che
 * nessuno nota.
 */
const BERSAGLIO = "windows-x86_64";

/**
 * Quanti titoli di changelog stanno in un avviso.
 *
 * L'avviso nella finestra è alto quattro righe con un `<details>` che se ne
 * apre altre poche. La sezione `[Non rilasciato]` di questo repo ne ha
 * ventisette: incollarli tutti darebbe una tendina che copre la schermata per
 * dire cose successe otto mesi fa. Chi vuole l'elenco intero ha il link alla
 * release, che è a un click e ce l'ha per intero.
 */
const QUANTI_TITOLI = 8;

/** Muore dicendo cosa, invece di scrivere un manifesto sbagliato. */
function basta(messaggio) {
  console.error(messaggio);
  process.exit(1);
}

/**
 * Gli otto byte con cui minisign dice «questa è la chiave numero tale».
 *
 * Sia la chiave pubblica sia la firma sono, dentro Tauri, il **testo** del file
 * minisign codificato in base64. Il testo ha una riga di commento e una riga di
 * base64 vero; quella riga, decodificata, comincia con due byte di algoritmo e
 * otto di identificativo. Sono quegli otto che si confrontano.
 */
function identificativo(base64DelFile, cosa) {
  let testo;
  try {
    testo = Buffer.from(base64DelFile, "base64").toString("utf8");
  } catch {
    return basta(`${cosa}: non è base64.`);
  }
  const righe = testo
    .split("\n")
    .map((r) => r.trim())
    .filter((r) => r !== "" && !r.startsWith("untrusted comment:"));
  if (righe.length === 0) basta(`${cosa}: dentro non c'è nessuna riga di dati.`);
  const grezzo = Buffer.from(righe[0], "base64");
  if (grezzo.length < 10) basta(`${cosa}: la riga di dati è troppo corta.`);
  return grezzo.subarray(2, 10).toString("hex");
}

/** La versione, dal file che `versione.js` tiene allineato agli altri due. */
function versione() {
  return JSON.parse(fs.readFileSync(PACCHETTO, "utf8")).version;
}

/** La chiave pubblica compilata dentro l'eseguibile. */
function chiavePubblica() {
  const conf = JSON.parse(fs.readFileSync(TAURI, "utf8"));
  const chiave = conf.plugins?.updater?.pubkey ?? "";
  if (chiave.trim() === "") {
    basta(
      `${TAURI}: plugins.updater.pubkey è vuoto.\n` +
        "Senza, gli eseguibili non sanno verificare niente e l'aggiornamento\n" +
        "fallirebbe sul computer di chi lo scarica. Generare le chiavi con:\n" +
        "  cd apps/desktop && npm run tauri signer generate -- -w ../../.chiavi/aether.key",
    );
  }
  return chiave.trim();
}

/** Da dove si scarica: `owner/repo`, che in CI lo dice GitHub stesso. */
function deposito() {
  if (process.env.GITHUB_REPOSITORY) return process.env.GITHUB_REPOSITORY;
  const radice = fs.readFileSync("Cargo.toml", "utf8");
  const m = radice.match(/^repository\s*=\s*"https:\/\/github\.com\/([^"]+)"/m);
  if (!m) basta("Non so su quale repository sto pubblicando.");
  return m[1].replace(/\/$/, "");
}

/**
 * L'installer prodotto dal bundler, e la sua firma.
 *
 * Uno solo: se ce ne fossero due vorrebbe dire che `target/` porta dentro
 * l'installer di una compilazione precedente, e sceglierne uno a caso vuol dire
 * pubblicare la versione sbagliata sotto il nome giusto.
 */
function installer() {
  if (!fs.existsSync(BUNDLE)) {
    basta(`Non c'è ${BUNDLE}: manca un \`npm run build\` da apps/desktop.`);
  }
  const exe = fs
    .readdirSync(BUNDLE)
    .filter((f) => f.toLowerCase().endsWith("-setup.exe"));
  if (exe.length === 0) basta(`Nessun installer in ${BUNDLE}.`);
  if (exe.length > 1) {
    basta(
      `In ${BUNDLE} ce n'è più d'uno:\n  ${exe.join("\n  ")}\n` +
        "Sono compilazioni diverse: svuotare la cartella e ricostruire.",
    );
  }
  const nome = exe[0];
  const firma = path.join(BUNDLE, `${nome}.sig`);
  if (!fs.existsSync(firma)) {
    basta(
      `Manca ${firma}.\n` +
        "L'installer non è stato firmato: `bundle.createUpdaterArtifacts` in\n" +
        "tauri.conf.json, e TAURI_SIGNING_PRIVATE_KEY nell'ambiente di build.",
    );
  }
  return { nome, firma: fs.readFileSync(firma, "utf8").trim() };
}

/**
 * Le note di rilascio: i titoli delle sottosezioni del changelog.
 *
 * I titoli e non il corpo, ed è una scelta: il corpo di una sezione di
 * `CHANGELOG.md` qui dentro sono pagine — questo file spiega *perché* le cose
 * sono come sono, ed è il suo pregio — mentre l'avviso che compare nella
 * finestra è alto quattro righe. I titoli sono già scritti per essere letti da
 * soli («Aggiunto — l'equalizzatore»), e chi vuole il resto trova il link alla
 * release.
 */
function note(v) {
  if (!fs.existsSync(CHANGELOG)) return "";
  const testo = fs.readFileSync(CHANGELOG, "utf8");
  const righe = testo.split("\n");
  // La sezione di questa versione, o quella non ancora rilasciata: al momento
  // in cui si taglia un tag il changelog di solito ha ancora l'intestazione
  // vecchia, e pubblicare una release senza note perché mancava una parentesi
  // quadra sarebbe un modo curioso di essere rigorosi.
  const inizio = righe.findIndex(
    (r) => r.startsWith(`## [${v}]`) || r.startsWith("## [Non rilasciato]"),
  );
  if (inizio < 0) return "";
  const titoli = [];
  for (let i = inizio + 1; i < righe.length; i++) {
    if (righe[i].startsWith("## ")) break;
    if (righe[i].startsWith("### ")) {
      titoli.push(`• ${righe[i].slice(4).trim()}`);
    }
  }
  const mostrati = titoli.slice(0, QUANTI_TITOLI);
  if (titoli.length > QUANTI_TITOLI) {
    mostrati.push(`… e altre ${titoli.length - QUANTI_TITOLI} voci nel changelog.`);
  }
  return mostrati.join("\n");
}

function main() {
  const dove = process.argv[2] ?? "uscita";
  const v = versione();
  const chiave = chiavePubblica();
  const { nome, firma } = installer();

  const dellaChiave = identificativo(chiave, "plugins.updater.pubkey");
  const dellaFirma = identificativo(firma, `${nome}.sig`);
  if (dellaChiave !== dellaFirma) {
    basta(
      "La firma non è stata fatta con la chiave che gli eseguibili conoscono.\n" +
        `  la chiave in tauri.conf.json:  ${dellaChiave}\n` +
        `  la chiave che ha firmato:      ${dellaFirma}\n\n` +
        "Pubblicare così vuol dire che nessuna installazione esistente\n" +
        "riuscirà ad aggiornarsi, e che non se ne accorgerà nessuno finché\n" +
        "non lo racconta qualcuno. Controllare TAURI_SIGNING_PRIVATE_KEY nei\n" +
        "segreti del repository.",
    );
  }

  const manifesto = {
    version: v,
    notes: note(v),
    pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
    platforms: {
      [BERSAGLIO]: {
        signature: firma,
        // Al tag e non a `/releases/latest/download/`: `latest` cambia
        // significato quando esce la release dopo, e un manifesto che resta
        // scritto da qualche parte deve continuare a indicare il file che
        // dichiara di indicare.
        url: `https://github.com/${deposito()}/releases/download/v${v}/${nome}`,
      },
    },
  };

  fs.mkdirSync(dove, { recursive: true });
  const uscita = path.join(dove, "latest.json");
  fs.writeFileSync(uscita, `${JSON.stringify(manifesto, null, 2)}\n`);
  console.log(`${uscita}: Aether ${v}, firmato con ${dellaChiave}`);
  console.log(`  ${manifesto.platforms[BERSAGLIO].url}`);
}

// Come script fa il manifesto vero; come modulo presta i suoi pezzi.
//
// Il secondo caso è `finto-aggiornamento.js`, che deve fare esattamente le
// stesse quattro cose — trovare l'installer, leggerne la firma, ricavare le
// note, confrontare gli identificativi delle due metà della chiave — su un
// manifesto che però punta a `127.0.0.1` invece che a GitHub. Riscriverle di
// là vorrebbe dire due copie del confronto che esiste per non essere sbagliato,
// e la copia che si aggiorna sarebbe sempre l'altra.
if (require.main === module) main();

module.exports = {
  BERSAGLIO,
  chiavePubblica,
  identificativo,
  installer,
  note,
  versione,
};
