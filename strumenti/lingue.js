/*
 * Le lingue dicono tutte le stesse cose.
 *
 * # Il problema che risolve
 *
 * Aggiungere una lingua ad Aether costa un file: si lascia cadere `de.json` in
 * `apps/desktop/src/lingue/` e il programma lo trova da sé, perché l'elenco
 * delle lingue disponibili *è* il contenuto di quella cartella
 * (`import.meta.glob`, in `lingue/index.ts`). Non c'è nessun registro da
 * aggiornare, ed è il punto.
 *
 * Ma un elenco che si costruisce da sé non sa dire se un file è **completo**.
 * Una chiave che manca da `de.json` non è un errore: il motore ripiega
 * sull'inglese e disegna lo stesso. È il comportamento giusto — meglio una
 * parola inglese in mezzo al tedesco che una schermata bianca — e insieme è
 * proprio ciò che rende una traduzione a metà invisibile a chi la scrive. Si
 * scopre mesi dopo, da uno screenshot di qualcun altro.
 *
 * Questo confronta le chiavi di ogni `lingue/*.json` con quelle di `it.json`,
 * che è la lingua in cui si scrive per prima, ed elenca le differenze nei due
 * versi: quel che manca e quel che avanza. Le seconde contano quanto le prime —
 * una chiave che sta solo in `de.json` è una chiave rinominata altrove e non
 * qui, cioè testo morto che nessuno disegnerà mai.
 *
 * # Due modi
 *
 *   node strumenti/lingue.js            controlla, esce con 1 se qualcosa manca
 *   node strumenti/lingue.js --scrivi   aggiunge le mancanti col testo italiano
 *
 * Il secondo serve a chi comincia una lingua nuova: mette in fila tutte le
 * chiavi con dentro l'italiano, così restano da tradurre invece che da
 * cercare. Non tocca mai una chiave che c'è già.
 */
const fs = require("fs");
const path = require("path");

// I percorsi si risolvono rispetto a **questo file**, non alla cartella da cui
// si è lanciato: la stessa ragione per cui lo fa `versione.js`.
const REPO = path.dirname(__dirname);
process.chdir(REPO);

const CARTELLA = "apps/desktop/src/lingue";
/** La lingua di riferimento: è quella in cui il testo nasce. */
const RIFERIMENTO = "it";
/** Sta in ogni file e non è testo dell'interfaccia: è il nome nativo della lingua. */
const CHIAVE_NOME = "_nome";

/** Legge un dizionario, o muore dicendo quale non si leggeva. */
function leggi(codice) {
  const dove = path.join(CARTELLA, `${codice}.json`);
  try {
    const letto = JSON.parse(fs.readFileSync(dove, "utf8"));
    if (letto === null || typeof letto !== "object" || Array.isArray(letto)) {
      throw new Error("non è un oggetto");
    }
    return letto;
  } catch (err) {
    console.error(`${dove}: ${err.message}`);
    process.exit(1);
  }
}

/** I codici presenti, dedotti dai nomi dei file. Nessun elenco scritto a mano. */
function lingue() {
  return fs
    .readdirSync(CARTELLA)
    .filter((f) => f.endsWith(".json"))
    .map((f) => f.slice(0, -".json".length))
    .sort();
}

/** Al più `quante` voci, e poi quante ne restano. */
function primeVoci(elenco, quante = 12) {
  const righe = elenco.slice(0, quante).map((k) => `      ${k}`);
  if (elenco.length > quante) {
    righe.push(`      … e altre ${elenco.length - quante}`);
  }
  return righe.join("\n");
}

const codici = lingue();
if (!codici.includes(RIFERIMENTO)) {
  console.error(`manca ${CARTELLA}/${RIFERIMENTO}.json, che è il riferimento`);
  process.exit(1);
}

const riferimento = leggi(RIFERIMENTO);
const chiaviRif = Object.keys(riferimento);
const scrivi = process.argv.includes("--scrivi");
let storto = false;

for (const codice of codici) {
  if (codice === RIFERIMENTO) continue;
  const dizionario = leggi(codice);

  if (typeof dizionario[CHIAVE_NOME] !== "string" || dizionario[CHIAVE_NOME] === "") {
    console.error(
      `${codice}.json: manca «${CHIAVE_NOME}», il nome della lingua nella lingua stessa.\n` +
        `      È quel che il selettore delle impostazioni mostra: senza, la voce non ha etichetta.`,
    );
    storto = true;
  }

  const mancanti = chiaviRif.filter((k) => !(k in dizionario));
  const avanzate = Object.keys(dizionario).filter((k) => !(k in riferimento));

  if (scrivi && mancanti.length > 0) {
    // Nell'ordine di `it.json`, non in coda: un dizionario in ordine sparso è
    // un dizionario che si rilegge male, e chi traduce lo rilegge tutto.
    const dopo = {};
    for (const k of [CHIAVE_NOME, ...chiaviRif]) {
      if (k === CHIAVE_NOME) {
        dopo[k] = dizionario[k] ?? codice;
      } else {
        dopo[k] = k in dizionario ? dizionario[k] : riferimento[k];
      }
    }
    for (const k of avanzate) dopo[k] = dizionario[k];
    fs.writeFileSync(
      path.join(CARTELLA, `${codice}.json`),
      `${JSON.stringify(dopo, null, 2)}\n`,
      "utf8",
    );
    console.log(`${codice}.json: aggiunte ${mancanti.length} chiavi, da tradurre`);
    continue;
  }

  if (mancanti.length > 0) {
    console.error(
      `${codice}.json: ${mancanti.length} chiavi mancano rispetto a ${RIFERIMENTO}.json\n` +
        `${primeVoci(mancanti)}\n` +
        `      «node strumenti/lingue.js --scrivi» le aggiunge col testo italiano.`,
    );
    storto = true;
  }
  if (avanzate.length > 0) {
    console.error(
      `${codice}.json: ${avanzate.length} chiavi che ${RIFERIMENTO}.json non ha\n` +
        `${primeVoci(avanzate)}\n` +
        `      Sono chiavi rinominate o tolte altrove: qui non le disegna nessuno.`,
    );
    storto = true;
  }
  if (mancanti.length === 0 && avanzate.length === 0) {
    console.log(`${codice}.json: ${chiaviRif.length} chiavi, complete`);
  }
}

if (codici.length === 1) {
  console.log(`solo ${RIFERIMENTO}.json: niente da confrontare`);
}

/*
 * E poi il confronto che le lingue fra loro non possono fare.
 *
 * Il controllo qui sopra mette `de.json` accanto a `it.json` e dice cosa manca
 * all'uno rispetto all'altro. Per costruzione non può accorgersi di una chiave
 * che manca a **tutti e due**: una chiave che non esiste da nessuna parte è, per
 * quel confronto, una chiave che non esiste e basta.
 *
 * È precisamente il buco da cui è passato `errors.fs.networkUnavailable`. Il
 * catalogo del nucleo genera per ogni codice la chiave `errors.<codice>`
 * (`aether-domain/src/errors/catalog.rs`), la finestra la cerca, e quando non la
 * trova ripiega sul `message` — che è il dettaglio tecnico scritto per chi legge
 * i registri, non per chi ascolta musica. Il guasto non si vede: non c'è nessun
 * errore, c'è una frase sbagliata, e la si scopre solo capitandoci sopra.
 *
 * Quindi il catalogo diventa la seconda sorgente della verità, accanto a
 * `it.json`. Si legge con un'espressione regolare invece che compilando Rust
 * perché questo strumento gira anche dove `cargo` non c'è, e perché la forma di
 * quelle righe è dichiarativa apposta.
 */
const CATALOGO = "core/aether-domain/src/errors/catalog.rs";

/** I codici dichiarati dal nucleo, nell'ordine in cui stanno scritti. */
function codiciDelNucleo() {
  let sorgente;
  try {
    sorgente = fs.readFileSync(CATALOGO, "utf8");
  } catch (err) {
    console.error(`${CATALOGO}: ${err.message}`);
    process.exit(1);
  }
  const trovati = [...sorgente.matchAll(/=\s*"([a-z]+\.[A-Za-z]+)"/g)].map((m) => m[1]);
  if (trovati.length === 0) {
    console.error(
      `${CATALOGO}: nessun codice riconosciuto.
` +
        `      La forma delle righe del catalogo è cambiata: aggiorna l'espressione qui sopra,` +
        ` o questo controllo passerà per sempre senza guardare niente.`,
    );
    process.exit(1);
  }
  return [...new Set(trovati)].sort();
}

const senzaFrase = codiciDelNucleo().filter((c) => !(`errors.${c}` in riferimento));
if (senzaFrase.length > 0) {
  console.error(
    `${RIFERIMENTO}.json: ${senzaFrase.length} codici del catalogo non hanno una frase
` +
      `${primeVoci(senzaFrase.map((c) => `errors.${c}`))}
` +
      `      Chi li incontra legge il dettaglio tecnico del nucleo al posto di una spiegazione.`,
  );
  storto = true;
} else {
  console.log(`${CATALOGO}: tutti i codici hanno la loro frase`);
}

process.exit(storto ? 1 : 0);
