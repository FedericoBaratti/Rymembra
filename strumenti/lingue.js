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
 * # I testi, non solo le chiavi
 *
 * Fino alla 3.2 questo strumento confrontava insiemi di chiavi e basta, e tutti
 * questi difetti di testo sono passati con `npm run verify` verde: virgolette
 * di un'altra lingua dentro l'inglese, due grafie dell'inglese nello stesso
 * file, una frase che cita un tasto con parole che sullo schermo non ci sono,
 * «1 righe aggiunte», un codice d'errore tolto dal catalogo che si portava
 * dietro la sua frase (R17-24). Adesso guarda anche i valori — le funzioni
 * `controlla…` più sotto, ognuna pura e con la sua prova in `lingue.test.js` —
 * e tiene le eccezioni in `lingue-eccezioni.json`, una riga e un motivo per voce:
 * un'eccezione senza motivo è un controllo spento.
 *
 * Il controllo dei segnaposto **al punto di chiamata** (`t("k", { … })` che li
 * riempie davvero) non sta qui: legge il TypeScript, e per leggerlo senza
 * falsi allarmi serve il compilatore, che c'è solo dopo `npm ci`. È in
 * `apps/desktop/test/segnaposto.test.ts`.
 *
 * # Due modi
 *
 *   node strumenti/lingue.js            controlla, esce con 1 se qualcosa manca
 *   node strumenti/lingue.js --scrivi   aggiunge le mancanti col testo italiano
 *
 * Il secondo serve a chi comincia una lingua nuova: mette in fila tutte le
 * chiavi con dentro l'italiano, così restano da tradurre invece che da
 * cercare. Non tocca mai una chiave che c'è già.
 *
 * Con `--radice=<cartella>` i controlli girano su un altro albero (un'estrazione
 * di un commit vecchio, per vedere cosa avrebbero detto): le eccezioni restano
 * quelle di questo file.
 */
const fs = require("fs");
const path = require("path");

const { leggiJson } = require("./json.js");

// I percorsi si risolvono rispetto a **questo file**, non alla cartella da cui
// si è lanciato: la stessa ragione per cui lo fa `versione.js`.
const RADICE_ARG = process.argv.find((a) => a.startsWith("--radice="));
const REPO = RADICE_ARG ? path.resolve(RADICE_ARG.slice("--radice=".length)) : path.dirname(__dirname);
/** Le eccezioni stanno accanto allo strumento, non nell'albero che si controlla. */
const FILE_ECCEZIONI = path.join(__dirname, "lingue-eccezioni.json");

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
    // `leggiJson` rifiuta una chiave ripetuta: `JSON.parse` terrebbe l'ultima, e la
    // prima traduzione non arriverebbe mai a schermo.
    const letto = leggiJson(fs.readFileSync(dove, "utf8"), dove);
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

/*
 * I testi, non solo le chiavi.
 *
 * Ogni controllo qui sotto è una funzione pura: riceve dizionari, sorgenti ed
 * eccezioni, e restituisce l'elenco di quel che non va, una riga ciascuno. Non
 * legge niente dal disco e non esce: così ognuno si prova in `lingue.test.js`
 * con tre righe di fixture, e `main` li chiama sull'albero vero.
 *
 * Valgono solo i controlli che non danno falsi allarmi: dove una regola avrebbe
 * bisogno di capire la lingua (un sostantivo plurale, un tasto che «dovrebbe»
 * esistere) c'è un elenco chiuso, e quel che ci sta fuori è un'eccezione scritta
 * con il suo motivo in `lingue-eccezioni.json`.
 */

/** I segnaposto `{nome}` di una frase, ordinati e senza doppioni. */
function segnaposti(testo) {
  return [...new Set([...String(testo).matchAll(/\{(\w+)\}/g)].map((m) => m[1]))].sort();
}

/**
 * 1. Gli stessi segnaposto in ogni lingua.
 *
 * Oggi non trova niente — 0 differenze su 3 272 chiavi — ed è il controllo che
 * serve il giorno in cui arriva `de.json`: una traduzione che traduce anche
 * `{nome}` si vede a schermo come «{name}» scritto così, e lì nessuno cerca.
 */
function segnapostiDiversi(dizionari, riferimento = RIFERIMENTO) {
  const ref = dizionari[riferimento] ?? {};
  const trovati = [];
  for (const [codice, d] of Object.entries(dizionari)) {
    if (codice === riferimento) continue;
    for (const k of Object.keys(ref)) {
      if (!(k in d)) continue;
      const a = segnaposti(ref[k]);
      const b = segnaposti(d[k]);
      if (a.join(",") !== b.join(",")) {
        trovati.push(
          `${codice}.json: «${k}» ha {${b.join(", ")}} dove ${riferimento}.json ha {${a.join(", ")}}`,
        );
      }
    }
  }
  return trovati;
}

/**
 * 5. Le virgolette e gli apostrofi della lingua.
 *
 * L'italiano scrive «così», l'inglese “così” e l'apostrofo tipografico ’ (D-78):
 * un testo con le virgolette di un'altra lingua si legge storto, e passa tutti i
 * controlli sulle chiavi. Le virgolette dritte `"` restano ammesse ovunque: sono
 * quelle del codice che le frasi citano (`"infinite"`, `{"$token": …}`).
 */
function virgoletteSbagliate(codice, dizionario, eccezioni = {}) {
  const altrui = { it: /[“”]/, en: /[«»]/ }[codice];
  const trovati = [];
  for (const [k, v] of Object.entries(dizionario)) {
    if (k === CHIAVE_NOME || typeof v !== "string") continue;
    if (altrui?.test(v)) {
      trovati.push(`${codice}.json: «${k}» ha le virgolette di un'altra lingua (${v.match(altrui)[0]})`);
    }
    if (codice === "en" && !k.endsWith(".syn") && v.includes("'") && !(k in (eccezioni.apostrofiDritti ?? {}))) {
      trovati.push(`${codice}.json: «${k}» ha un apostrofo dritto ': in inglese si scrive ’`);
    }
  }
  return trovati;
}

/**
 * 6. Una grafia sola dell'inglese: l'americano (D-78).
 *
 * Un elenco chiuso di forme britanniche che il progetto ha già incontrato; fuori
 * dai `.syn`, che tengono volutamente tutte e due le grafie perché la ricerca
 * trovi chi scrive come gli pare.
 */
const GRAFIE_BRITANNICHE = [
  [/\bcatalogues?\b/i, "catalog"],
  [/\bfavourites?\b/i, "favorite"],
  [/\bcolours?\b|\bcoloured\b|\bcolouring\b/i, "color"],
  [/\brecognis(e|es|ed|ing)\b/i, "recognize"],
  [/\b(un)?authoris(e|es|ed|ing|ation)\b/i, "authorize"],
  [/\blicences?\b/i, "license"],
  [/\bcentres?\b/i, "center"],
  [/\banalysers?\b/i, "analyzer"],
  [/\bequalisers?\b/i, "equalizer"],
  [/\bbehaviours?\b/i, "behavior"],
  [/\bcancell(ed|ing|ation)\b/i, "canceled"],
  [/\blabell(ed|ing)\b/i, "labeled"],
  [/\bmodell(ed|ing)\b/i, "modeled"],
  [/\btravell(ed|ing|er)\b/i, "traveled"],
  [/\bgrey\b/i, "gray"],
  [/\b(organis|synchronis|normalis|customis|initialis|visualis|minimis|maximis|summaris|optimis|prioritis|categoris|finalis|serialis|randomis|localis)(e|es|ed|er|ers|ing|ation)\b/i, "-ize"],
  [/\banalys(e|es|ed|ing)\b/i, "analyze"],
  [/\bprogrammes?\b/i, "program"],
  [/\bjudgements?\b/i, "judgment"],
  [/\bwhilst\b/i, "while"],
  [/\bmetres?\b/i, "meter"],
  [/\bhonour|neighbour|\bdefence\b|\bartefacts?\b|\bageing\b/i, "la forma americana"],
];

function grafiaBritannica(dizionario) {
  const trovati = [];
  for (const [k, v] of Object.entries(dizionario)) {
    if (k === CHIAVE_NOME || k.endsWith(".syn") || typeof v !== "string") continue;
    for (const [forma, americana] of GRAFIE_BRITANNICHE) {
      const m = v.match(forma);
      if (m) trovati.push(`en.json: «${k}» scrive «${m[0]}», in americano ${americana}`);
    }
  }
  return trovati;
}

/** Il testo di una frase come lo confronta [`etichetteCitate`]: senza segnaposto né finale. */
function senzaFinale(testo) {
  return testo
    .replace(/\{[^}]*\}/g, "")
    .replace(/[….\s]+$/u, "")
    .trim()
    .toLowerCase();
}

/**
 * 7. Ogni etichetta citata esiste.
 *
 * Una frase che dice «premi «Riprova»» manda a un tasto, e quel tasto deve
 * chiamarsi così **nella lingua della frase**: la traduzione di un'etichetta si
 * rinomina da una parte e si dimentica dall'altra. Un testo fra virgolette lungo
 * meno di 45 caratteri deve essere il valore di qualche chiave — a meno del
 * finale («…», «.») e dei segnaposto. Quel che non è un'etichetta di Rymembra —
 * la console di Google, un nome di file, una parola del formato delle skin —
 * sta in `etichetteEsterne` (per testo) o in `etichetteInChiavi` (per prefisso
 * della chiave), con il motivo.
 */
function etichetteCitate(codice, dizionario, eccezioni = {}) {
  const re = codice === "it" ? /«([^»]+)»/g : codice === "en" ? /“([^”]+)”/g : null;
  if (re === null) return [];
  const esterne = new Set(Object.keys(eccezioni.etichetteEsterne ?? {}));
  const prefissi = Object.keys(eccezioni.etichetteInChiavi ?? {});
  const valori = new Set(
    Object.values(dizionario)
      .filter((v) => typeof v === "string")
      .map(senzaFinale),
  );
  const trovati = [];
  for (const [k, v] of Object.entries(dizionario)) {
    if (k === CHIAVE_NOME || typeof v !== "string") continue;
    if (prefissi.some((p) => k.startsWith(p))) continue;
    for (const m of v.matchAll(re)) {
      const citato = m[1];
      if (/\{/.test(citato) || citato.length > 45 || esterne.has(citato)) continue;
      if (!valori.has(senzaFinale(citato))) {
        trovati.push(`${codice}.json: «${k}» cita «${citato}», che non è il testo di nessuna chiave`);
      }
    }
  }
  return trovati;
}

/**
 * 8. Solo `n` sceglie il plurale.
 *
 * `lingue/index.ts` sceglie la forma (`.uno`, `.one`…) dal solo valore `n`. Una
 * frase con un altro contatore davanti a un sostantivo plurale — «{piu} righe
 * aggiunte», «{bozze} bozze» — dice «1 righe» quando vale uno, e nessuna forma
 * può correggerla. Si scrive con `n` (e le sue forme), o in una forma che non
 * concorda col numero («aggiunte: {piu} · tolte: {meno}»).
 *
 * L'elenco dei sostantivi è chiuso apposta: capire se «{nome} chiede» è un verbo
 * o un plurale vorrebbe una grammatica, e un falso allarme costa più di un
 * sostantivo che manca (si aggiunge qui quando se ne scopre uno).
 */
const SOSTANTIVI_PLURALI = {
  it: "righe|brani|bozze|colori|motivi|parti|token|elementi|dischi|gettoni|voci|caratteri|pezzi|cartelle|animazioni|doppioni|playlist|preferiti|ascolti|file|secondi|minuti|passi|punti|modifiche|errori|risultati|livelli|modelli",
  en: "lines|drafts|tracks|colors|patterns|parts|tokens|items|skins|discs|records|steps|seconds|minutes|files|folders|playlists|changes|errors|results|levels|models|animations|keyframes|rows",
};

function contatoriFuoriDaN(codice, dizionario, eccezioni = {}) {
  const nomi = SOSTANTIVI_PLURALI[codice];
  if (nomi === undefined) return [];
  const re = new RegExp(`\\{(\\w+)\\}\\s+(${nomi})\\b`, "giu");
  const ammessi = eccezioni.contatori ?? {};
  const prefissi = Object.keys(ammessi).filter((k) => k.endsWith("."));
  const trovati = [];
  for (const [k, v] of Object.entries(dizionario)) {
    if (k === CHIAVE_NOME || typeof v !== "string") continue;
    if (k in ammessi || prefissi.some((p) => k.startsWith(p))) continue;
    for (const m of v.matchAll(re)) {
      if (m[1] === "n") continue;
      trovati.push(
        `${codice}.json: «${k}» scrive «${m[0]}»: solo \`n\` sceglie il plurale, e con 1 si leggerebbe storto`,
      );
    }
  }
  return trovati;
}

/**
 * 9. I valori uguali in italiano e in inglese sono dichiarati.
 *
 * Quasi tutti sono giusti — unità, marchi, «Album», «Playlist» — e uno è la
 * prova che una traduzione non è stata fatta (`studio.chat.mode.agent` era
 * «Agent» in italiano). Un elenco di quelli accettati, per gruppo e con un
 * motivo, li fa passare e fa saltare fuori il prossimo.
 */
function valoriUguali(it, en, eccezioni = {}) {
  const accettati = new Set(Object.values(eccezioni.valoriUguali ?? {}).flat());
  const trovati = [];
  for (const k of Object.keys(it)) {
    if (k === CHIAVE_NOME || !(k in en) || it[k] !== en[k] || accettati.has(k)) continue;
    // Né lettere né parole: «{n} kB», «5.1», «…».
    if (!/\p{L}{3,}/u.test(it[k].replace(/\{[^}]*\}/g, ""))) continue;
    trovati.push(`«${k}» è identico in it.json e en.json («${it[k]}»): tradotto, o da dichiarare in lingue-eccezioni.json`);
  }
  return trovati;
}

/** Le forme di una chiave senza il suo plurale né la variante del telefono. */
function baseDiChiave(chiave, esiste) {
  let base = chiave;
  for (;;) {
    const punto = base.lastIndexOf(".");
    if (punto < 0) return base;
    const coda = base.slice(punto + 1);
    const resto = base.slice(0, punto);
    if ((FORME.includes(coda) || coda === "telefono") && esiste(resto)) base = resto;
    else return base;
  }
}

/**
 * Le chiavi dei testi che il codice nomina, scritte per esteso fra virgolette
 * (`"queue.remove.aria"`) o come inizio di una chiave costruita
 * (`` `settings.entry.${id}` ``, `"pref." + x`).
 */
function chiaviNelCodice(testoCodice) {
  const letterali = new Set();
  for (const m of testoCodice.matchAll(/["'`]([A-Za-z0-9_][A-Za-z0-9_.\-]*)["'`]/g)) letterali.add(m[1]);
  const prefissi = new Set();
  for (const m of testoCodice.matchAll(/`([A-Za-z0-9_][A-Za-z0-9_.\-]*\.)\$\{/g)) prefissi.add(m[1]);
  for (const m of testoCodice.matchAll(/["']([A-Za-z0-9_][A-Za-z0-9_.\-]*\.)["']\s*\+/g)) prefissi.add(m[1]);
  // `skin.issue.*` nasce nel validatore, da `motivo!("chiave", …)`.
  const motivi = new Set();
  for (const m of testoCodice.matchAll(/motivo!\(\s*"([^"]+)"/g)) {
    motivi.add(m[1].startsWith("skin.") ? m[1] : `skin.issue.${m[1]}`);
  }
  return { letterali, prefissi, motivi };
}

/**
 * 3. Nessuna chiave è morta.
 *
 * Una chiave che nessun punto del codice nomina — né per esteso né come inizio
 * di una chiave costruita — è testo che nessuno disegnerà, e quasi sempre la
 * traccia di un tasto spostato altrove (`settings.sync.rename`) o di una
 * funzione tolta. Una frase `errors.<codice>` il cui codice non è più nel
 * catalogo del nucleo è lo stesso difetto, con la sua causa scritta: i nove
 * `transfer.*` sono restati per un anno dopo che nessuno li produceva (R17-23).
 *
 * `eccezioni.chiaviVive` dice, per chiave o per prefisso (se finisce con «.»),
 * quelle che si disegnano in un modo che il codice non mostra, con il motivo.
 */
function chiaviMorte({ riferimento, testoCodice, codici, eccezioni = {} }) {
  const { letterali, prefissi, motivi } = chiaviNelCodice(testoCodice);
  const nelCatalogo = new Set(codici.map((c) => `errors.${c}`));
  const vive = Object.keys(eccezioni.chiaviVive ?? {});
  const esiste = (k) => k in riferimento;
  const morte = [];
  const frasiSenzaCodice = [];
  for (const k of Object.keys(riferimento)) {
    if (k === CHIAVE_NOME) continue;
    const base = baseDiChiave(k, esiste);
    if (nelCatalogo.has(base)) continue;
    if (vive.some((v) => (v.endsWith(".") ? base.startsWith(v) : v === base))) continue;
    if (letterali.has(base) || motivi.has(base)) continue;
    if ([...prefissi].some((p) => base.startsWith(p))) continue;
    // Una frase d'errore di un codice che il catalogo non ha più.
    (base.startsWith("errors.") ? frasiSenzaCodice : morte).push(k);
  }
  return [
    ...frasiSenzaCodice.map(
      (k) => `«${k}» è la frase di un codice che il catalogo del nucleo non ha più: togli la frase`,
    ),
    ...morte.map((k) => `«${k}» non è usata da nessun punto del codice`),
  ];
}

/**
 * 4. Ogni voce dei registri ha la sua frase.
 *
 * Le descrizioni delle parti, dei token e dei parametri delle skin, i motivi
 * della coda automatica e i generi di avviso dello Studio vivono in Rust e si
 * traducono con una chiave **composta dal nome**. Una voce nuova senza la sua riga
 * nel catalogo delle lingue ripiega sull'italiano del nucleo: in inglese si
 * legge una frase italiana nello Studio (R17-5). Si leggono i registri con
 * espressioni regolari — come il catalogo degli errori —, e se una non trova
 * niente è la forma dei sorgenti che è cambiata.
 */
function vociDeiRegistri({ parti, token, autoplay, avvisi, parametri }) {
  const attese = [];
  const lette = (nome, trovate) => {
    if (trovate.length === 0) {
      throw new Error(`${nome}: nessuna voce riconosciuta. La forma del sorgente è cambiata: aggiorna l'espressione, o questo controllo passerà per sempre senza guardare niente.`);
    }
    return trovate;
  };
  for (const m of lette("parts.rs", [...parti.matchAll(/PartDef::nuova\(\s*"([^"]+)"/g)])) {
    attese.push({ chiave: `skin.part.${m[1]}`, registro: "parts.rs" });
  }
  for (const m of lette("tokens.rs", [...token.matchAll(/token!\(\s*"([^"]+)"/g)])) {
    attese.push({ chiave: `skin.token.${m[1]}`, registro: "tokens.rs" });
  }
  const dopoCodice = autoplay.slice(autoplay.indexOf("pub const fn codice"));
  const corpo = dopoCodice.slice(0, dopoCodice.indexOf("\n    }\n"));
  for (const m of lette("autoplay.rs", [...corpo.matchAll(/=>\s*"([a-z]+)"/g)])) {
    attese.push({ chiave: `queue.why.${m[1]}`, registro: "autoplay.rs" });
  }
  const enumAvvisi = avvisi.slice(avvisi.indexOf("pub enum WarningKind"));
  const variantiAvvisi = enumAvvisi.slice(0, enumAvvisi.indexOf("\n}\n"));
  for (const m of lette("avvisi.rs", [...variantiAvvisi.matchAll(/^\s{4}([A-Z]\w+),/gm)])) {
    const camel = m[1][0].toLowerCase() + m[1].slice(1);
    attese.push({ chiave: `studio3.warning.${camel}`, registro: "avvisi.rs (WarningKind)" });
  }
  // I parametri stanno sotto l'effetto che li dichiara: qui basta che il nome
  // abbia una riga per qualcuno (`skin.par.<effetto>.<nome>`).
  const nomiParametri = new Set(lette("registro.rs", [...parametri.matchAll(/par!\(\s*"([^"]+)"/g)]).map((m) => m[1]));
  return { attese, nomiParametri };
}

/** Le voci dei registri che un dizionario non ha. */
function vociMancanti(codice, dizionario, registri) {
  const trovati = [];
  for (const { chiave, registro } of registri.attese) {
    if (!(chiave in dizionario)) trovati.push(`${codice}.json: manca «${chiave}» (${registro})`);
  }
  for (const nome of registri.nomiParametri) {
    const c = Object.keys(dizionario).some((k) => k.startsWith("skin.par.") && k.endsWith(`.${nome}`));
    if (!c) trovati.push(`${codice}.json: manca «skin.par.<effetto>.${nome}» (registro.rs)`);
  }
  return trovati;
}

/** Le eccezioni scritte accanto allo strumento. */
function leggiEccezioni() {
  try {
    return leggiJson(fs.readFileSync(FILE_ECCEZIONI, "utf8"), FILE_ECCEZIONI);
  } catch (err) {
    console.error(`${FILE_ECCEZIONI}: ${err.message}`);
    process.exit(1);
  }
}

/** Tutti i file con una di queste estensioni sotto `cartella`, senza `node_modules` né `target`. */
function filesSotto(cartella, estensioni, tranne = []) {
  const fuori = [];
  const visita = (dir) => {
    let voci;
    try {
      voci = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const voce of voci) {
      const p = path.join(dir, voce.name);
      if (voce.isDirectory()) {
        if (!["node_modules", "target", "gen", "dist", ".git", ...tranne].includes(voce.name)) visita(p);
      } else if (estensioni.some((e) => voce.name.endsWith(e))) {
        fuori.push(p);
      }
    }
  };
  visita(cartella);
  return fuori;
}

/**
 * Il testo di tutti i file di un elenco, senza i commenti: un commento che cita
 * ``settings.sync.rename`` non la disegna, e farebbe passare per viva una chiave morta.
 */
function leggiTutti(files) {
  return files
    .map((f) =>
      fs
        .readFileSync(f, "utf8")
        .replace(/\/\*[\s\S]*?\*\//g, "")
        .split("\n")
        .filter((riga) => !/^\s*(\/\/|\*)/.test(riga))
        .join("\n"),
    )
    .join("\n");
}

/**
 * I controlli sui testi, sull'albero vero (o su quello di `--radice`).
 *
 * Restituisce le righe di errore. Quelli che leggono i sorgenti (3 e 4) si
 * accorgono da soli se il sorgente non c'è più.
 */
function controlliSuiTesti(dizionari) {
  const eccezioni = leggiEccezioni();
  const errori = [];
  const it = dizionari[RIFERIMENTO];
  const aggiungi = (titolo, righe, aiuto) => {
    if (righe.length === 0) return;
    errori.push(`${titolo}: ${righe.length}\n${primeVoci(righe, 20)}${aiuto ? `\n      ${aiuto}` : ""}`);
  };
  const perOgniLingua = (titolo, controllo, aiuto) => {
    for (const [codice, d] of Object.entries(dizionari)) aggiungi(titolo, controllo(codice, d), aiuto);
  };

  aggiungi("Segnaposto diversi fra le lingue", segnapostiDiversi(dizionari));
  perOgniLingua("Virgolette o apostrofi di un'altra lingua", (c, d) => virgoletteSbagliate(c, d, eccezioni));
  if (dizionari.en) aggiungi("Grafie britanniche in en.json", grafiaBritannica(dizionari.en));
  perOgniLingua(
    "Etichette citate che non esistono",
    (c, d) => etichetteCitate(c, d, eccezioni),
    "Se non è un'etichetta di Rymembra (la console di un servizio, un nome di file), va in lingue-eccezioni.json con il motivo.",
  );
  const sorgentiTs = filesSotto(path.join(REPO, "apps/desktop/src"), [".ts", ".tsx"], ["lingue"]);
  const sorgentiRs = [
    ...filesSotto(path.join(REPO, "apps/desktop/src-tauri/src"), [".rs"]),
    ...filesSotto(path.join(REPO, "apps/desktop/rymembra-android"), [".rs"]),
    ...filesSotto(path.join(REPO, "core"), [".rs"]),
  ];
  const testoCodice = leggiTutti([...sorgentiTs, ...sorgentiRs]);
  aggiungi(
    "Chiavi morte in it.json",
    chiaviMorte({ riferimento: it, testoCodice, codici: codiciDelNucleo(), eccezioni }),
    "Si tolgono (da tutte le lingue), o se si disegnano in un modo che il codice non mostra vanno in lingue-eccezioni.json con il motivo.",
  );

  return errori;
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

  // I valori, non solo le chiavi (R17-24). Con `--scrivi` si sta traducendo: non si
  // rimprovera a chi comincia quel che ancora non ha scritto.
  if (!scrivi) {
    const dizionari = Object.fromEntries(
      codici.map((c) => [c, c === RIFERIMENTO ? riferimento : leggi(c)]),
    );
    const errori = controlliSuiTesti(dizionari);
    for (const e of errori) console.error(e);
    if (errori.length > 0) storto = true;
    else console.log("testi: virgolette, grafia, etichette, contatori, chiavi vive e registri: a posto");
  }

  process.exit(storto ? 1 : 0);
}

if (require.main === module) main();

module.exports = {
  GRAFIE_BRITANNICHE,
  baseDiChiave,
  chiaviMorte,
  contatoriFuoriDaN,
  controllaPlurali,
  etichetteCitate,
  grafiaBritannica,
  segnaposti,
  segnapostiDiversi,
  semplici,
  unoEsatto,
  valoriUguali,
  variante,
  vociDeiRegistri,
  vociMancanti,
  virgoletteSbagliate,
};
