/*
 * Le Novità della versione che si rilascia.
 *
 * # Il problema che risolve
 *
 * La finestra delle Novità (`apps/desktop/src/novita.ts`) mostra, una volta dopo
 * un aggiornamento, due, tre o quattro schermate scritte a mano. Quel che nessun
 * compilatore vede è la dimenticanza: si alza la versione (`npm run version:set`),
 * si fa il tag, e le novità sono ancora quelle della versione di prima — cioè la
 * finestra non compare, e nessuno se ne accorge finché qualcuno non chiede
 * «perché non c'è niente?». Una versione senza novità è legittima — una
 * correttiva non ne ha — ma deve essere una scelta, non una svista: questo
 * strumento la dice ad alta voce, e non ferma niente.
 *
 * # Cosa controlla
 *
 * - **Avviso**, che non ferma: la versione di `apps/desktop/package.json` non ha
 *   novità in `novita.ts`. In GitHub Actions è un'annotazione `::warning::`.
 * - **Problema**, che ferma: una versione del registro con meno di due o più di
 *   quattro passi; chiavi che non sono \`whatsNew.<versione>.<n>.title|text\` in
 *   ordine; un testo mancante o vuoto in italiano o in inglese; chiavi
 *   \`whatsNew.<versione>.*\` nei file di lingua che nessun registro nomina (le
 *   novità di una versione di prima, rimaste lì).
 *
 * Con \`--richiedi\` anche l'avviso ferma: è il modo di pretendere le novità per
 * una release che le deve avere.
 *
 * Con \`--se-minor\` ferma solo se la versione è una **minor o una major** (la
 * patch è zero: 3.3.0, 4.0.0): è quella che porta cose nuove, e se esce senza
 * la finestra chi viene dalla versione prima non la vedrà mai (alla correttiva
 * dopo la tabella dice «Niente»). Una correttiva (3.3.1) resta libera, con
 * l'avviso. È quel che chiede il lavoro \`tag\` di \`release.yml\`; senza opzioni
 * lo strumento non ferma mai per questo, perché \`npm run verify\` lo lancia
 * anche a metà sviluppo, con la versione già alzata e le novità ancora da
 * scrivere.
 *
 *   node strumenti/novita.js              controlla, avvisa, esce con 1 se qualcosa non torna
 *   node strumenti/novita.js --richiedi   la versione di oggi DEVE avere le novità
 *   node strumenti/novita.js --se-minor   come --richiedi, ma solo per una minor o una major
 *
 * Il registro si legge come testo, senza compilare TypeScript: lo strumento gira
 * prima di \`npm ci\`, e la forma di \`novita.ts\` è abbastanza semplice da non
 * volerne un parser.
 */
const fs = require("fs");
const path = require("path");

const { leggiJson } = require("./json.js");

// I percorsi si risolvono rispetto a **questo file**, non alla cartella da cui
// si è lanciato: la stessa ragione per cui lo fa `versione.js`.
const REPO = path.dirname(__dirname);

const MIN_PASSI = 2;
const MAX_PASSI = 4;

/** Un file di lingua, con le chiavi ripetute che diventano un errore. */
function leggi(file) {
  return leggiJson(fs.readFileSync(file, "utf8"), path.basename(file));
}

/** Il numero di passi di ogni versione nel registro di `novita.ts`, dal testo. */
function passiDi(testo) {
  const conti = {};
  const registro = testo.slice(testo.indexOf("NOVITA"));
  for (const voce of registro.matchAll(/"(\d+\.\d+\.\d+)":\s*\[([\s\S]*?)\n\s*\],/g)) {
    conti[voce[1]] = (voce[2].match(/titoloKey:/g) ?? []).length;
  }
  return conti;
}

/** Le chiavi `titoloKey` e `testoKey` di una versione, nell'ordine in cui compaiono. */
function chiaviDi(testo, versione) {
  const voce = testo.match(
    new RegExp(`"${versione.replace(/\./g, "\\.")}":\\s*\\[([\\s\\S]*?)\\n\\s*\\],`),
  );
  if (!voce) return [];
  return [...voce[1].matchAll(/(?:titoloKey|testoKey):\s*"([^"]+)"/g)].map((m) => m[1]);
}

/**
 * Una minor o una major — la patch è zero: 3.3.0, 4.0.0 — porta cose nuove, e
 * deve portare le Novità; una correttiva (3.3.1) no.
 */
function richiedeLeNovita(versione) {
  const m = /^(\d+)\.(\d+)\.(\d+)$/.exec(versione);
  return m !== null && Number(m[3]) === 0;
}

/**
 * Controlla le novità di un albero.
 *
 * @param {string} radice la radice del repository
 * @param {{richiedi?: boolean, seMinor?: boolean}} [opzioni] `seMinor`: come
 *   `richiedi`, ma solo se la versione è una minor o una major
 * @returns {{problemi: string[], avvisi: string[]}}
 */
function controlla(radice, { richiedi = false, seMinor = false } = {}) {
  const problemi = [];
  const avvisi = [];
  const desktop = path.join(radice, "apps", "desktop");
  const versione = JSON.parse(fs.readFileSync(path.join(desktop, "package.json"), "utf8")).version;
  if (seMinor && richiedeLeNovita(versione)) richiedi = true;
  const registro = fs.readFileSync(path.join(desktop, "src", "novita.ts"), "utf8");
  const lingue = {
    it: leggi(path.join(desktop, "src", "lingue", "it.json")),
    en: leggi(path.join(desktop, "src", "lingue", "en.json")),
  };

  const conti = passiDi(registro);
  for (const [v, n] of Object.entries(conti)) {
    if (n < MIN_PASSI || n > MAX_PASSI) {
      problemi.push(
        `novita.ts: la ${v} ha ${n} passi, e ne servono da ${MIN_PASSI} a ${MAX_PASSI} (da 2 a 4 passi).`,
      );
    }
    const attese = [];
    for (let i = 1; i <= n; i++) attese.push(`whatsNew.${v}.${i}.title`, `whatsNew.${v}.${i}.text`);
    const trovate = chiaviDi(registro, v);
    for (const chiave of trovate) {
      if (!attese.includes(chiave)) {
        problemi.push(
          `novita.ts: la chiave «${chiave}» non è whatsNew.${v}.<n>.title o .text, con <n> da 1 a ${n} in ordine.`,
        );
      }
    }
    for (const [lingua, testi] of Object.entries(lingue)) {
      for (const chiave of trovate) {
        const valore = testi[chiave];
        if (typeof valore !== "string" || valore.trim() === "") {
          problemi.push(`${lingua}.json: manca il testo di «${chiave}» (o è vuoto).`);
        }
      }
    }
  }

  // Le chiavi di una versione che il registro non nomina più: novità di prima,
  // rimaste nei file di lingua dopo che si è spostata la chiave.
  for (const [lingua, testi] of Object.entries(lingue)) {
    const orfane = new Set();
    for (const chiave of Object.keys(testi)) {
      const m = chiave.match(/^whatsNew\.(\d+\.\d+\.\d+)\./);
      if (m && !(m[1] in conti)) orfane.add(m[1]);
    }
    for (const v of orfane) {
      problemi.push(
        `${lingua}.json: ci sono chiavi whatsNew.${v}.* ma novita.ts non ha la ${v}: ` +
          `o la versione è tornata nel registro, o le chiavi vanno tolte.`,
      );
    }
  }

  if (!(versione in conti)) {
    const frase =
      `La ${versione} non ha novità in apps/desktop/src/novita.ts: la finestra «Novità» non ` +
      `comparirà dopo l'aggiornamento. Se è voluto (una versione correttiva), niente da fare; ` +
      `se no, scrivi i passi e i testi whatsNew.${versione}.* in it.json e en.json.`;
    (richiedi ? problemi : avvisi).push(frase);
  }
  return { problemi, avvisi };
}

function principale() {
  const richiedi = process.argv.includes("--richiedi");
  const seMinor = process.argv.includes("--se-minor");
  const { problemi, avvisi } = controlla(REPO, { richiedi, seMinor });
  const inCi = process.env.GITHUB_ACTIONS === "true";
  for (const avviso of avvisi) console.log(inCi ? `::warning title=Novità::${avviso}` : `Avviso: ${avviso}`);
  if (problemi.length > 0) {
    console.log(`\n${problemi.length} problemi nelle Novità:\n`);
    for (const p of problemi) console.log(`  ${p}`);
    process.exit(1);
  }
  if (avvisi.length === 0) console.log("Novità: la versione di oggi ha le sue, scritte in italiano e in inglese.");
}

if (require.main === module) principale();

module.exports = { controlla, passiDi, richiedeLeNovita, REPO };
