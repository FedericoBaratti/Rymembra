/*
 * Il `latest.json` che l'updater interroga.
 *
 * # Cos'è
 *
 * Trecento byte su GitHub, e l'unica cosa che decide se una copia di Rymembra
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
 * Non basta, e non da solo: un id uguale non dice che la firma sia **di quei
 * byte**. Un `.sig` copiato da un'altra release, o un installer riconfezionato
 * dopo la firma, ha lo stesso id e la stessa versione e fa fallire ogni copia
 * installata. Per questo `main` verifica anche la firma contro i byte
 * (`minisign.js`: BLAKE2b-512 e Ed25519, come il client) — sull'installer che
 * il bundler ha lasciato, su quello che si sta per caricare in `<cartella>` e,
 * riletto da disco, sul `latest.json` appena scritto.
 *
 * # Un modo
 *
 *   node strumenti/manifesto.js [cartella]     scrive <cartella>/latest.json
 *
 * Senza argomenti scrive in `uscita/`, che è dove `release.yml` raccoglie.
 */
const fs = require("fs");
const path = require("path");

const { verificaFirma } = require("./minisign.js");

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

/**
 * I titoli che ogni sezione ha e che non dicono niente a chi aggiorna.
 *
 * «The number» spiega perché la versione è major, minor o patch, «Database»
 * quali migrazioni ci sono e cosa costa tornare indietro: stanno in fondo a
 * ogni sezione per chi legge il changelog, e in un avviso di quattro righe
 * prenderebbero due posti su otto per dire «qui c'è una regola di
 * numerazione». La 3.0.0 li aveva **solo** loro, e le note sarebbero state
 * quelle due righe.
 *
 * «Known limits» per la stessa ragione: dice che cosa **non** c'è, ed è giusto
 * che lo dica il changelog, non l'avviso che annuncia che cosa c'è di nuovo.
 */
const DI_SERVIZIO = /^(The number|Database|Known limits)\b/;

/**
 * I titoli che vanno in testa, qualunque posto abbiano nel changelog.
 *
 * Le sezioni di sicurezza sono quelle per cui una persona deve sapere che
 * conviene aggiornare, e con otto posti e il changelog in ordine di argomento
 * finivano oltre il taglio: nella sezione `[Unreleased]` del novembre 2026,
 * prima che si chiamassero «Security — your devices over the internet» e
 * «Security — the reviews of November 2026» e stessero davanti, «Security» e
 * «Security — the review of 2026-11-10» erano le voci undici e dodici, cioè
 * fuori dalle note. Il nome non conta (basta che cominci per `Security`) e il
 * posto nemmeno: l'ordine del file resta quello per chi lo legge per intero.
 */
const IN_TESTA = /^Security\b/;

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

/**
 * La versione che la firma dice di coprire, o `null` se non ne dice nessuna.
 *
 * Sta nel *commento fidato* del file minisign — la riga `trusted comment:`,
 * coperta dalla firma — come coppie `chiave:valore` separate da tabulazioni:
 * `timestamp:…\tfile:…\tversion:3.0.0`. La scrive `tauri signer sign
 * --app-version`, e dalla 3.0 Rymembra (`requireSignedVersion` in
 * `tauri.conf.json`) rifiuta un aggiornamento la cui firma non la porti o ne
 * porti un'altra: va controllata qui, prima che lo scopra ogni copia
 * installata.
 */
function versioneFirmata(base64DelFile) {
  const testo = Buffer.from(base64DelFile, "base64").toString("utf8");
  const commento = testo
    .split("\n")
    .map((r) => r.trim())
    .find((r) => r.startsWith("trusted comment:"));
  if (commento === undefined) return null;
  for (const coppia of commento.slice("trusted comment:".length).trim().split("\t")) {
    const [chiave, ...resto] = coppia.split(":");
    if (chiave === "version") return resto.join(":");
  }
  return null;
}

/**
 * La firma dentro un `latest.json` è valida per i byte dell'installer?
 *
 * Sull'oggetto riletto da disco e non su quello costruito in memoria: è il file
 * che si carica, e un errore nello scriverlo (o un campo preso dalla riga
 * sbagliata) lo vede solo chi lo rilegge. Vuota se va bene.
 *
 * @param {object} manifesto il contenuto di `latest.json`
 * @param {Buffer} byte l'installer
 * @param {string} chiave `plugins.updater.pubkey`
 * @returns {string[]} i problemi
 */
function verificaManifesto(manifesto, byte, chiave) {
  const voce = manifesto?.platforms?.[BERSAGLIO];
  if (!voce || typeof voce.signature !== "string") return [`latest.json: manca platforms.${BERSAGLIO}.signature`];
  const esito = verificaFirma(byte, voce.signature, chiave);
  return esito.ok ? [] : [`latest.json: la firma di platforms.${BERSAGLIO} non vale per l'installer: ${esito.motivo}`];
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
        "  cd apps/desktop && npm run tauri signer generate -- -w ../../.chiavi/rymembra.key",
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
  return noteDa(fs.readFileSync(CHANGELOG, "utf8"), v);
}

/**
 * Le note ricavate da un testo di changelog già letto.
 *
 * Separata da `note` perché la si possa provare su un changelog finto
 * (`manifesto.test.js`) senza scrivere file accanto a quello vero.
 */
function noteDa(testo, v) {
  const righe = testo.split("\n");
  // La sezione di questa versione, e **solo se non c'è** quella non ancora
  // rilasciata: al momento in cui si taglia un tag il changelog di solito ha
  // ancora l'intestazione vecchia, e pubblicare una release senza note perché
  // mancava una parentesi quadra sarebbe un modo curioso di essere rigorosi.
  //
  // # Perché due ricerche e non un `||` dentro una sola
  //
  // Perché `findIndex` restituisce la **prima** riga che soddisfa la
  // condizione, e «Non rilasciato» sta sopra a ogni versione: con un `||` la
  // fallback vinceva sempre sulla cosa che doveva sostituire. Finché il
  // changelog non aveva una sezione chiusa non si vedeva, ed è esattamente il
  // genere di difetto che si scopre la sera del primo rilascio — con in mano
  // una release le cui note sono la stringa vuota.
  let inizio = righe.findIndex((r) => r.startsWith(`## [${v}]`));
  if (inizio < 0) {
    // Le due grafie: il changelog è passato all'inglese con la 2.3, e
    // l'intestazione della sezione aperta con lui.
    inizio = righe.findIndex(
      (r) => r.startsWith("## [Non rilasciato]") || r.startsWith("## [Unreleased]"),
    );
  }
  if (inizio < 0) return "";
  const titoli = [];
  for (let i = inizio + 1; i < righe.length; i++) {
    if (righe[i].startsWith("## ")) break;
    if (righe[i].startsWith("### ")) {
      const titolo = righe[i].slice(4).trim();
      if (!DI_SERVIZIO.test(titolo)) titoli.push(titolo);
    }
  }
  // Prima la sicurezza, poi il resto nell'ordine del file: `sort` è stabile.
  titoli.sort((a, b) => Number(IN_TESTA.test(b)) - Number(IN_TESTA.test(a)));
  const mostrati = titoli.slice(0, QUANTI_TITOLI).map((t) => `• ${t}`);
  // In inglese come i titoli: la 3.2.0 è uscita con «… e altre 1 voci nel
  // changelog.», una riga italiana nelle note di chi usa Rymembra in inglese.
  // «N more» non chiede di scegliere fra singolare e plurale.
  if (titoli.length > QUANTI_TITOLI) {
    mostrati.push(`… and ${titoli.length - QUANTI_TITOLI} more in the changelog.`);
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

  const firmata = versioneFirmata(firma);
  if (firmata !== v && firmata !== `v${v}`) {
    basta(
      `${nome}.sig: la firma copre la versione ${firmata ?? "(nessuna)"}, il manifesto annuncia la ${v}.\n` +
        "Le copie 3.x rifiutano un aggiornamento così (`requireSignedVersion`).\n" +
        `Firmare con: npx tauri signer sign --app-version ${v} <installer>`,
    );
  }

  // La firma contro i byte, e non solo l'id e la versione che il testo dice:
  // vedi il preambolo e `minisign.js`. Sia sul file che ha lasciato il bundler
  // (quello che si è firmato) sia su quello in `<cartella>`, che è l'artefatto
  // che `pubblica` consegna: se un passo in mezzo l'avesse toccato, le due
  // firme sarebbero buone e l'installer pubblicato no.
  const percorsi = [path.join(BUNDLE, nome)];
  const caricato = path.join(dove, nome);
  if (fs.existsSync(caricato)) percorsi.push(caricato);
  for (const percorso of percorsi) {
    const esito = verificaFirma(fs.readFileSync(percorso), firma, chiave);
    if (!esito.ok) {
      basta(
        `${nome}.sig: ${esito.motivo}.\n` +
          `  il file controllato: ${percorso}\n\n` +
          "Pubblicare così vuol dire che ogni copia installata scaricherebbe l'installer\n" +
          "e lo scarterebbe con «The signature verification failed»: il guasto sta sul\n" +
          "computer di altri e non lo racconta nessuno. Controllare che il file firmato\n" +
          "sia quello che si carica, e la chiave in TAURI_SIGNING_PRIVATE_KEY.",
      );
    }
  }

  // Vuote non partono: l'avviso nella finestra si aprirebbe su una tendina
  // senza niente dentro, su ogni copia installata. La prova in
  // `manifesto.test.js` lo dice a ogni push; questo è l'ultimo cancello.
  const testo = note(v);
  if (testo === "") {
    basta(
      `${CHANGELOG}: la sezione della ${v} non ha titoli \`### \` da mostrare.\n` +
        "Le note dell'aggiornamento sarebbero vuote.",
    );
  }

  const manifesto = {
    version: v,
    notes: testo,
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
  // Riletto da disco: il file che si carica, non l'oggetto da cui nasce.
  const riletto = verificaManifesto(JSON.parse(fs.readFileSync(uscita, "utf8")), fs.readFileSync(path.join(BUNDLE, nome)), chiave);
  if (riletto.length > 0) {
    fs.rmSync(uscita);
    basta(riletto.join("\n"));
  }
  console.log(`${uscita}: Rymembra ${v}, firmato con ${dellaChiave}`);
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
  noteDa,
  verificaManifesto,
  versione,
  versioneFirmata,
};
