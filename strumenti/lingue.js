/*
 * Le lingue dicono tutte le stesse cose.
 *
 * # Il problema che risolve
 *
 * Aggiungere una lingua a Rymembra costa un file: si lascia cadere `de.json` in
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
 * # Le forme del plurale
 *
 * Dalla 3.0 una chiave può avere più forme: `brani.uno` per esattamente uno, e
 * le categorie del CLDR — `brani.one`, `brani.few`, `brani.many`, `brani.other`
 * — che `lingue/plurali.ts` sceglie con `Intl.PluralRules`. Le forme non si
 * confrontano chiave per chiave come le altre, perché le lingue non ne hanno lo
 * stesso numero: il russo ne vuole tre dove l'italiano ne vuole una, e il
 * giapponese nessuna. Le regole, una per una, stanno in [`controllaPlurali`].
 * In breve: una forma che la lingua non ha è un errore (non si leggerebbe mai);
 * una forma che l'italiano scrive e la lingua ha va scritta anche lì; e una
 * forma che una lingua usa da qualche parte va scritta dappertutto — usarla è
 * dichiararla. Le categorie che una lingua non scrive mai sono il ripiego
 * dichiarato: si stampano come nota, e quelle frasi usano la chiave nuda.
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

const CARTELLA = "apps/desktop/src/lingue";
/** La lingua di riferimento: è quella in cui il testo nasce. */
const RIFERIMENTO = "it";
/** Sta in ogni file e non è testo dell'interfaccia: è il nome nativo della lingua. */
const CHIAVE_NOME = "_nome";

/**
 * Le forme del plurale, come suffissi: `uno` (proprio uno) e le categorie del
 * CLDR. Gli stessi di `SUFFISSO_UNO` e `CATEGORIE` in `lingue/plurali.ts`:
 * questo strumento gira senza compilare TypeScript, e la copia è corta.
 */
const UNO = "uno";
const CATEGORIE = ["zero", "one", "two", "few", "many", "other"];
const FORME = [UNO, ...CATEGORIE];

/** Le categorie plurali di una lingua, come le dice il motore. */
function categorieDi(codice) {
  try {
    return new Intl.PluralRules(codice).resolvedOptions().pluralCategories;
  } catch {
    return ["other"];
  }
}

/**
 * In questa lingua `one` vuol dire «proprio uno», e nient'altro.
 *
 * Vero in italiano e in inglese, ed è il motivo per cui lì `.uno` basta: copre
 * tutta la categoria. Falso in russo (21 è `one`) e in francese (0 è `one`), dove
 * `.uno` dice soltanto la frase per l'uno e la categoria va scritta a parte.
 */
function unoEsatto(codice) {
  let regole;
  try {
    regole = new Intl.PluralRules(codice);
  } catch {
    return false;
  }
  if (!regole.resolvedOptions().pluralCategories.includes("one")) return false;
  for (let n = 0; n <= 1000; n += 1) {
    if ((regole.select(n) === "one") !== (n === 1)) return false;
  }
  return true;
}

/**
 * Se `chiave` è una forma del plurale, la sua base e la sua forma.
 *
 * Lo è solo se finisce con una forma **e** la base esiste come chiave: è la
 * chiave nuda che il motore usa per ultima, e una forma senza la sua base non è
 * una forma — `artists.index.other` è l'etichetta della voce «#» dell'indice, e
 * di `artists.index` non c'è traccia.
 */
function variante(chiave, baseEsiste) {
  const punto = chiave.lastIndexOf(".");
  if (punto < 0) return null;
  const forma = chiave.slice(punto + 1);
  const base = chiave.slice(0, punto);
  if (!FORME.includes(forma) || !baseEsiste(base)) return null;
  return { base, forma };
}

/** Le forme scritte in un dizionario, per base. */
function formeDi(dizionario, baseEsiste) {
  const forme = new Map();
  for (const chiave of Object.keys(dizionario)) {
    const v = variante(chiave, baseEsiste);
    if (v === null) continue;
    if (!forme.has(v.base)) forme.set(v.base, new Set());
    forme.get(v.base).add(v.forma);
  }
  return forme;
}

/**
 * Le forme del plurale di una lingua, contro quelle del riferimento.
 *
 * Restituisce:
 * - `mancanti`: le forme che il riferimento scrive, che la lingua ha, e che il
 *   suo file non ha. `brani.uno` in italiano chiede `brani.uno` in inglese — o
 *   `brani.one`, che lì vuol dire la stessa cosa —, e non lo chiede al
 *   giapponese, che il singolare non ce l'ha;
 * - `ammesse`: le forme che la lingua scrive in più del riferimento e che le
 *   spettano — `brani.few` in russo. Non sono chiavi avanzate;
 * - `errori`: una forma che la lingua non ha (`brani.few` in italiano: nessun
 *   numero la sceglierà mai), e una forma che la lingua usa altrove e qui
 *   manca;
 * - `note`: le categorie che la lingua ha e non scrive da nessuna parte. È il
 *   ripiego dichiarato — quelle frasi usano la chiave nuda — e si dice, ma non
 *   è un errore: il milione esatto dell'italiano è `many`, e nessuno vuole
 *   scrivere «1.000.000 di brani» a parte.
 *
 * Il riferimento si controlla passando sé stesso: niente mancanti, il resto sì.
 */
function controllaPlurali(codice, dizionario, riferimento) {
  const baseEsiste = (base) => base in riferimento || base in dizionario;
  const categorie = categorieDi(codice);
  const esatto = unoEsatto(codice);
  /** La forma `forma` della base, o una che in questa lingua vale lo stesso. */
  const copre = (insieme, forma) =>
    insieme.has(forma) ||
    (esatto && forma === "one" && insieme.has(UNO)) ||
    (esatto && forma === UNO && insieme.has("one"));
  const applicabile = (forma) =>
    forma === UNO ? categorie.includes("one") : categorie.includes(forma);

  const nostre = formeDi(dizionario, baseEsiste);
  const loro = formeDi(riferimento, (base) => base in riferimento);
  const mancanti = [];
  const ammesse = new Set();
  const errori = [];
  const note = [];

  // Una forma che la lingua non ha non si leggerà mai.
  for (const [base, forme] of nostre) {
    for (const forma of forme) {
      const chiave = `${base}.${forma}`;
      if (forma !== UNO && !categorie.includes(forma)) {
        errori.push(
          `${codice}.json: «${chiave}» è una forma che questa lingua non ha ` +
            `(le sue: ${categorie.join(", ")}): nessun numero la sceglierà.`,
        );
        continue;
      }
      if (!(chiave in riferimento)) ammesse.add(chiave);
    }
  }

  // Quel che il riferimento scrive e questa lingua distingue.
  if (dizionario !== riferimento) {
    for (const [base, forme] of loro) {
      const qui = nostre.get(base) ?? new Set();
      for (const forma of forme) {
        if (applicabile(forma) && !copre(qui, forma)) mancanti.push(`${base}.${forma}`);
      }
    }
  }

  // Usarla è dichiararla: una categoria scritta in una base va scritta in
  // tutte quelle che hanno delle forme, qui o nel riferimento.
  const usate = new Set();
  for (const forme of nostre.values()) {
    for (const forma of forme) {
      if (forma === UNO) {
        if (esatto) usate.add("one");
      } else if (forma !== "other" && categorie.includes(forma)) {
        usate.add(forma);
      }
    }
  }
  const famiglie = new Set([...loro.keys(), ...nostre.keys()]);
  for (const base of famiglie) {
    if (!(base in dizionario)) continue;
    const qui = nostre.get(base) ?? new Set();
    for (const forma of usate) {
      if (copre(qui, forma) || mancanti.includes(`${base}.${forma}`)) continue;
      errori.push(
        `${codice}.json: «${base}» non ha la forma «${forma}», che altrove questa ` +
          `lingua scrive: con un numero di quella categoria si leggerebbe la chiave nuda.`,
      );
    }
  }

  const ripiego = categorie.filter((c) => c !== "other" && !usate.has(c));
  if (ripiego.length > 0 && famiglie.size > 0) {
    note.push(
      `${codice}.json: le forme ${ripiego.map((c) => `«${c}»`).join(", ")} non sono ` +
        `scritte da nessuna parte: ripiegano sulla chiave nuda.`,
    );
  }

  return { mancanti, ammesse, errori, note };
}

/** Le chiavi di un dizionario che non sono forme del plurale. */
function semplici(dizionario, riferimento) {
  const baseEsiste = (base) => base in riferimento || base in dizionario;
  return Object.keys(dizionario).filter((k) => variante(k, baseEsiste) === null);
}

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

/*
 * E poi il confronto che le lingue fra loro non possono fare.
 *
 * Il controllo delle chiavi mette `de.json` accanto a `it.json` e dice cosa
 * manca all'uno rispetto all'altro. Per costruzione non può accorgersi di una
 * chiave che manca a **tutti e due**: una chiave che non esiste da nessuna parte
 * è, per quel confronto, una chiave che non esiste e basta.
 *
 * È precisamente il buco da cui è passato `errors.fs.networkUnavailable`. Il
 * catalogo del nucleo genera per ogni codice la chiave `errors.<codice>`
 * (`rymembra-domain/src/errors/catalog.rs`), la finestra la cerca, e quando non la
 * trova ripiega sul `message` — che è il dettaglio tecnico scritto per chi legge
 * i registri, non per chi ascolta musica. Il guasto non si vede: non c'è nessun
 * errore, c'è una frase sbagliata, e la si scopre solo capitandoci sopra.
 *
 * Quindi il catalogo diventa la seconda sorgente della verità, accanto a
 * `it.json`. Si legge con un'espressione regolare invece che compilando Rust
 * perché questo strumento gira anche dove `cargo` non c'è, e perché la forma di
 * quelle righe è dichiarativa apposta.
 */
const CATALOGO = "core/rymembra-domain/src/errors/catalog.rs";

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

function main() {
  process.chdir(REPO);

  const codici = lingue();
  if (!codici.includes(RIFERIMENTO)) {
    console.error(`manca ${CARTELLA}/${RIFERIMENTO}.json, che è il riferimento`);
    process.exit(1);
  }

  const riferimento = leggi(RIFERIMENTO);
  const chiaviRif = Object.keys(riferimento);
  const sempliciRif = semplici(riferimento, riferimento);
  const scrivi = process.argv.includes("--scrivi");
  let storto = false;

  // Le forme del riferimento si guardano anche da sole: una forma che
  // l'italiano non ha, o una usata a metà, è un errore anche senza un'altra
  // lingua accanto.
  {
    const { errori, note } = controllaPlurali(RIFERIMENTO, riferimento, riferimento);
    for (const e of errori) console.error(e);
    for (const n of note) console.log(n);
    if (errori.length > 0) storto = true;
  }

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

    const plurali = controllaPlurali(codice, dizionario, riferimento);
    const mancanti = [
      ...sempliciRif.filter((k) => !(k in dizionario)),
      ...plurali.mancanti,
    ];
    const avanzate = Object.keys(dizionario).filter(
      (k) => !(k in riferimento) && !plurali.ammesse.has(k),
    );

    if (scrivi && mancanti.length > 0) {
      // Nell'ordine di `it.json`, non in coda: un dizionario in ordine sparso è
      // un dizionario che si rilegge male, e chi traduce lo rilegge tutto. Le
      // forme del plurale che questa lingua non chiede restano fuori.
      const daScrivere = new Set(mancanti);
      const dopo = {};
      for (const k of [CHIAVE_NOME, ...chiaviRif]) {
        if (k === CHIAVE_NOME) {
          dopo[k] = dizionario[k] ?? codice;
        } else if (k in dizionario) {
          dopo[k] = dizionario[k];
        } else if (daScrivere.has(k)) {
          dopo[k] = riferimento[k];
        }
      }
      for (const k of Object.keys(dizionario)) if (!(k in dopo)) dopo[k] = dizionario[k];
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
    for (const e of plurali.errori) console.error(e);
    for (const n of plurali.note) console.log(n);
    if (plurali.errori.length > 0) storto = true;
    if (mancanti.length === 0 && avanzate.length === 0 && plurali.errori.length === 0) {
      console.log(`${codice}.json: ${Object.keys(dizionario).length} chiavi, complete`);
    }
  }

  if (codici.length === 1) {
    console.log(`solo ${RIFERIMENTO}.json: niente da confrontare`);
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
}

if (require.main === module) main();

module.exports = { controllaPlurali, semplici, unoEsatto, variante };
