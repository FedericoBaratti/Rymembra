/*
 * Ogni classe scritta nel markup ha una regola che la disegna, e ogni regola del
 * foglio veste qualcosa.
 *
 * # Il problema che risolve
 *
 * Una classe sbagliata non è un errore. `className="in-fondo"` compila, passa
 * `tsc`, passa `vite build`, e disegna: disegna col flusso di serie, cioè due
 * bottoni impilati a sinistra invece della riga in fondo alla finestrella. Non
 * c'è niente che si accenda di rosso — l'unico modo di accorgersene è aprire
 * quella schermata e guardarla, e sono ottanta schermate.
 *
 * È successo in tre modi, e questo strumento cerca tutti e tre.
 *
 * **Un nome che il foglio non conosce.** `in-fondo` dove sei modali su otto
 * dicono `tasti-finestrella`; `campo-ispettore` dove il gemello dice
 * `campo-identita`. Il nome è ragionevole, è solo che non l'ha mai disegnato
 * nessuno.
 *
 * **Una parte del registro che l'app non emette.** `aether-skin::parts` dichiara
 * il vocabolario che una skin può ridipingere, e `studio/scene.tsx` lo usa nel
 * mock dell'anteprima. Se la parte sta nel mock e non nell'app, chi disegna una
 * skin la vede funzionare mentre la scrive e non funzionare quando la installa:
 * il modo peggiore di scoprire una cosa. È il caso di `viz-scene`, scritto
 * nell'app dove il registro dice `viz-screen` — cioè esattamente l'errore per
 * assonanza che `aether-skin::vicini` esiste per correggere a chi fa le skin, e
 * che qui l'aveva fatto l'app.
 *
 * **Una regola che non veste più niente.** Il rovescio del primo, e si nota
 * ancora meno, perché quel che sbaglia non si vede: non si vede *niente*. Il
 * foglio disegnava un lettore in miniatura intero — `.mini`, `.mini-copertona`,
 * `.mini-trasporto`, duecento righe — sopravvissuto alla schermata che lo
 * montava. Chi apre il foglio per capire come si disegna una miniatura lo legge
 * come se fosse vivo, e chi cerca di capire quanto pesa il foglio conta anche
 * quello.
 *
 * # Cosa non sa fare
 *
 * Le classi che si compongono lontano dal `className` — `zona-${nodo.name}`,
 * che in `Impaginazione.tsx` nasce in una funzione a parte — non le vede, e va
 * bene: meglio qualche difetto non trovato che una lista di falsi allarmi che
 * si impara a saltare.
 *
 * Nel verso inverso quello stesso limite cambia di segno: la regola che disegna
 * un nome composto a distanza esiste e serve, ma il markup che la giustifica
 * questo strumento non lo sa leggere, e senza un elenco a parte la
 * dichiarerebbe morta. È per quello che c'è `ATTESE_FOGLIO`.
 *
 * Legge però `className={a ? "x" : "y"}` e `classe: "…"`, perché una prima
 * versione che leggeva solo `className="…"` dichiarava morte `bottom-nav`,
 * `np-transport` e `nav-pill`, che sono vive. E legge i commenti come commenti:
 * qui dentro il markup si cita per spiegarlo, e una citazione non è un elemento.
 *
 * # Le attese dichiarate
 *
 * In fondo ai tre elenchi ci sono `ATTESE`, `ATTESE_CLASSI` e `ATTESE_FOGLIO`:
 * quel che manca **e va bene**, ognuno col suo motivo. Senza quelle liste lo
 * strumento avrebbe ragione cinque volte su dieci, che è il modo di non averla
 * mai.
 *
 *   node strumenti/classi.js     controlla, esce con 1 se qualcosa non torna
 */
const fs = require("fs");
const path = require("path");

// I percorsi si risolvono rispetto a **questo file**, non alla cartella da cui
// si è lanciato: la stessa ragione per cui lo fa `versione.js`.
const REPO = path.dirname(__dirname);
process.chdir(REPO);

const SORGENTI = "apps/desktop/src";
const FOGLIO = path.join(SORGENTI, "stile.css");
const REGISTRO = "core/aether-skin/src/parts.rs";
// Il mock dell'anteprima dello Studio. Non è l'app: quel che vive solo qui è
// una promessa che l'anteprima mantiene e la finestra no.
const MOCK = path.join(SORGENTI, "studio", "scene.tsx");

/*
 * Le parti che il registro dichiara e l'app non disegna, con il perché.
 *
 * Una voce qui è una decisione, non un rinvio: se il motivo non si riesce a
 * scrivere in una riga, molto probabilmente è un difetto e non un'attesa.
 */
const ATTESE = {
  "app-shell":
    "Voluta: il contenitore di tutta la finestra non porta la classe — vedi il commento in parti/Colonna.tsx e parti/Navigazione.tsx.",
  "home-shortcuts": "Non c'è ancora una schermata iniziale con le scorciatoie.",
  "tour-tooltip": "Non c'è ancora un giro guidato.",
  "tooltip-pill":
    "Nessun bottone spento monta più la pastiglia — i testi adesso si leggono da soli — e il contenitore che la scopriva al passaggio non c'è più. Il foglio tiene `.tooltip-pill`: è una parte, e una skin deve poterla ridipingere.",
};

/*
 * Le classi scritte apposta senza regola, con il perché.
 *
 * Sono rare, e ognuna è una scelta: un nome che serve a dire che cosa è una
 * riga senza doverla dipingere in modo diverso.
 */
const ATTESE_CLASSI = {
  "d-uguale":
    "La riga immutata del confronto non prende colore: è il fondo su cui si leggono `.d-piu` e `.d-meno`, e dipingerla toglierebbe il contrasto che serve.",
  playlist:
    "Dice che cosa è la riga nella navigazione — `voce nav-pill playlist`, in `parti/Navigazione.tsx` — e non la dipinge: a dipingerla è `.voce`, e a distinguerla dalle destinazioni fisse è l'icona. Come `.dona` e `.trovata`, ma senza il ritocco che quelle due hanno.",
};

/*
 * Le regole del foglio che nessun markup **visibile da qui** indossa, col perché.
 *
 * Sono l'altra faccia di `ATTESE_CLASSI`, e la stessa disciplina: una voce qui
 * è una decisione, non un rinvio. Tutte dicono la stessa cosa da angoli diversi
 * — il nome si compone lontano dal `className`, e la sua metà che cambia arriva
 * da un campo o da Rust — cioè sono esattamente il limite dichiarato là sopra,
 * in «Cosa non sa fare», riletto dal lato del foglio.
 *
 * Se un giorno capita una voce che non si riesce a scrivere così, quella non è
 * un'attesa: è una regola morta, e la strada è toglierla da `stile.css`, non
 * iscriverla qui. Ci sono già passate `.blocco` e `.intestazione.playlist`,
 * scritte qui per una revisione e poi tolte dal foglio, che era il punto.
 */
const ATTESE_FOGLIO = {
  zona: "La metà fissa di `zona zona-${nodo.name}`, che `Impaginazione.tsx` compone in `classi()` — una funzione a parte, lontano da qualunque `className`.",
  "posto-content":
    "`Impaginazione.tsx` compone `posto posto-${nodo.name}` in `classi()`, e `content` è un nome di posto dello scafale: viene dalla skin, cioè da Rust, non da un letterale del markup.",
  "posto-player": "Come `posto-content`: il posto `player` dello scafale.",
  "posto-queue": "Come `posto-content`: il posto `queue` dello scafale.",
  "posto-selection-bar":
    "Come `posto-content`: il posto `selection-bar` dello scafale.",
  "t-chiave":
    "`studio/Documento.tsx` scrive `` `t-${pezzo.genere}` ``, e il genere è un campo — non un ternario di letterali, che è l'unica forma di buco che si sa leggere. I cinque generi li elenca `studio/evidenzia.ts`.",
  "t-stringa": "Come `t-chiave`: un genere di `studio/evidenzia.ts`.",
  "t-numero": "Come `t-chiave`: un genere di `studio/evidenzia.ts`.",
  "t-letterale": "Come `t-chiave`: un genere di `studio/evidenzia.ts`.",
  "t-segno": "Come `t-chiave`: un genere di `studio/evidenzia.ts`.",
};

/** I nomi di stato e i valori d'enumerazione, che non sono parti. */
const NON_PARTI = new Set([
  "active",
  "hover",
  "focus",
  "disabled",
  "none",
  "uppercase",
  "lowercase",
  "capitalize",
]);

/** Tutti i `.tsx` e `.ts` sotto una cartella. */
function sorgenti(dove) {
  const trovati = [];
  for (const voce of fs.readdirSync(dove, { withFileTypes: true })) {
    const via = path.join(dove, voce.name);
    if (voce.isDirectory()) trovati.push(...sorgenti(via));
    else if (/\.tsx?$/.test(voce.name)) trovati.push(via);
  }
  return trovati;
}

const APRE_BUCO = "$" + "{";

/**
 * Le classi scritte in un sorgente.
 *
 * Tre forme: `className="…"`, `className={…}` con dentro dei letterali, e
 * `classe` — la prop italiana con cui i componenti di `parti/` si fanno
 * passare una classe da fuori.
 */
function classiDi(sorgente) {
  // I commenti prima di tutto: qui dentro si cita il markup per spiegarlo —
  // «la regola cinque del markup: className="semantica parte"» — e una
  // citazione non è un elemento da disegnare.
  const testo = sorgente
    .replace(/\/\*[\s\S]*?\*\//g, " ")
    .replace(/^[ \t]*\/\/.*$/gm, " ");

  const trovate = new Set();

  const aggiungi = (elenco) => {
    for (const nome of elenco.split(/\s+/)) {
      if (/^[a-zA-Z][a-zA-Z0-9_-]*$/.test(nome)) trovate.add(nome);
    }
  };

  // Un letterale confrontato non è una classe: `nodo.kind === "zone"` dice che
  // cosa si sta guardando, non come si disegna.
  const senzaConfronti = (espressione) =>
    espressione
      .replace(/[=!]==?\s*"[^"]*"/g, "")
      .replace(/"[^"]*"\s*[=!]==?/g, "");

  /*
   * Un template, letto a pezzi separati dagli spazi.
   *
   * `zona zona-${nodo.name}` dà «zona» e basta: del secondo pezzo non si sa
   * niente, e inventarne il nome sarebbe peggio che tacere. `d-${x ? "piu" :
   * "meno"}` invece dà «d-piu» e «d-meno», perché il buco dice quali sono tutti
   * i suoi esiti — ed è la forma con cui in questo albero si scrive una classe
   * che cambia.
   */
  const daTemplate = (dentro) => {
    for (const pezzo of dentro.split(/\s+/)) {
      const buco = pezzo.indexOf(APRE_BUCO);
      if (buco === -1) {
        aggiungi(pezzo);
        continue;
      }
      if (!pezzo.endsWith("}")) continue;
      const radice = pezzo.slice(0, buco);
      if (!/^[a-zA-Z0-9_-]*$/.test(radice)) continue;
      for (const esito of senzaConfronti(pezzo.slice(buco)).matchAll(/"([^"]*)"/g)) {
        aggiungi(radice + esito[1]);
      }
    }
  };

  // La forma diretta, che è la stragrande maggioranza.
  for (const trovato of testo.matchAll(/(?:className|classe)\s*[=:]\s*"([^"]*)"/g)) {
    aggiungi(trovato[1]);
  }

  // La forma fra graffe. Si prende l'espressione fino alla graffa che chiude,
  // contando le annidate, e da lì si raccolgono i letterali.
  for (const trovato of testo.matchAll(/(?:className|classe)\s*=\s*\{/g)) {
    let i = trovato.index + trovato[0].length;
    let profondita = 1;
    const da = i;
    while (i < testo.length && profondita > 0) {
      if (testo[i] === "{") profondita++;
      else if (testo[i] === "}") profondita--;
      i++;
    }
    const espressione = testo.slice(da, i - 1);
    // I template si leggono a parte, e i loro letterali non si raccolgono due
    // volte: quel che sta dentro un buco è un esito, non una classe a sé.
    const templates = [...espressione.matchAll(/`([^`]*)`/g)];
    for (const pezzo of templates) daTemplate(pezzo[1]);
    let sciolto = espressione;
    for (const pezzo of templates) sciolto = sciolto.split(pezzo[0]).join(" ");
    for (const pezzo of senzaConfronti(sciolto).matchAll(/"([^"]*)"/g)) {
      aggiungi(pezzo[1]);
    }
  }

  return trovate;
}

/**
 * Le classi che il foglio disegna.
 *
 * I commenti si tolgono prima: questo foglio ne ha più righe che di regole, e
 * dentro ci sono nomi di file (`stile.css`, `plain.json`) che passerebbero per
 * selettori e coprirebbero proprio i difetti che si cercano.
 */
function classiDelFoglio(css) {
  const senzaCommenti = css.replace(/\/\*[\s\S]*?\*\//g, " ");
  // Anche le `url(...)`: `url('./font/geist-latin.woff2')` non è `.woff2`.
  const pulito = senzaCommenti.replace(/url\([^)]*\)/g, " ");
  const trovate = new Set();
  for (const pezzo of pulito.matchAll(/\.(-?[a-zA-Z][a-zA-Z0-9_-]*)/g)) {
    trovate.add(pezzo[1]);
  }
  return trovate;
}

/** Le parti dichiarate dal registro delle skin. */
function partiDelRegistro(rust) {
  const trovate = new Set();
  for (const pezzo of rust.matchAll(/parte!\(\s*"([a-z0-9-]+)"/g)) {
    trovate.add(pezzo[1]);
  }
  return trovate;
}

// ── il controllo ────────────────────────────────────────────────────────────

const file = sorgenti(SORGENTI);
const foglio = classiDelFoglio(fs.readFileSync(FOGLIO, "utf8"));
const parti = partiDelRegistro(fs.readFileSync(REGISTRO, "utf8"));

/** Dove compare ogni classe: serve a dire la riga, non solo il nome. */
const dove = new Map();
/** Le classi viste fuori dal mock dell'anteprima. */
const nellApp = new Set();

for (const via of file) {
  const testo = fs.readFileSync(via, "utf8");
  const righe = testo.split("\n");
  for (const nome of classiDi(testo)) {
    if (!dove.has(nome)) dove.set(nome, []);
    const riga = righe.findIndex((r) => r.includes(nome)) + 1;
    dove.get(nome).push(`${via.replace(/\\/g, "/")}:${riga || "?"}`);
    if (path.resolve(via) !== path.resolve(MOCK)) nellApp.add(nome);
  }
}

let guai = 0;

// 1. Una classe scritta che nessuno disegna, e che il registro non conosce.
const senzaRegola = [...dove.keys()]
  .filter((nome) => !foglio.has(nome) && !parti.has(nome))
  .filter((nome) => !(nome in ATTESE_CLASSI))
  .sort();

if (senzaRegola.length > 0) {
  guai += senzaRegola.length;
  console.log(`\n${senzaRegola.length} classi non hanno nessuna regola:\n`);
  for (const nome of senzaRegola) {
    console.log(`  .${nome}`);
    for (const posto of dove.get(nome).slice(0, 4)) console.log(`      ${posto}`);
  }
  console.log(
    "\n  Ognuna si disegna col flusso di serie. O le si dà una regola in\n" +
      "  stile.css, o è il nome a essere sbagliato: cerca il gemello che fa la\n" +
      "  stessa cosa in una schermata vicina e usa il suo.",
  );
}

// 2. Una parte del registro che l'app non emette.
const nonEmesse = [...parti]
  .filter((nome) => !NON_PARTI.has(nome))
  .filter((nome) => !nellApp.has(nome))
  .filter((nome) => !(nome in ATTESE))
  .sort();

if (nonEmesse.length > 0) {
  guai += nonEmesse.length;
  console.log(`\n${nonEmesse.length} parti del registro non le disegna nessuno:\n`);
  for (const nome of nonEmesse) {
    const soloNelMock = dove.has(nome);
    console.log(
      `  ${nome}${soloNelMock ? "   (c'è nel mock dell'anteprima, non nell'app)" : ""}`,
    );
  }
  console.log(
    "\n  Una skin può ridipingerle e non succede niente. O l'app le emette —\n" +
      "  di solito accanto alla classe italiana che già c'è — o vanno messe in\n" +
      "  ATTESE, qui dentro, con il motivo scritto.",
  );
}

// 3. Lo Studio dichiara le stesse attese, o no.
//
// `scene.tsx` ha un `NON_ANCORA` che serve a rispondere «questa parte non la
// disegna ancora nessuno» invece di «sta nella cornice, aspetta». Sono gli
// stessi nomi di `ATTESE`, e se i due elenchi divergono lo Studio torna a dare
// una delle due risposte sbagliate: quella che promette un'attesa senza fine.
const nonAncora = new Set(
  [
    ...fs
      .readFileSync(MOCK, "utf8")
      .matchAll(/"([a-z0-9-]+)":\s*"studio\.notYet\./g),
  ].map((t) => t[1]),
);
const soloAttese = Object.keys(ATTESE).filter((nome) => !nonAncora.has(nome));
const soloStudio = [...nonAncora].filter((nome) => !(nome in ATTESE));

if (soloAttese.length > 0 || soloStudio.length > 0) {
  guai += soloAttese.length + soloStudio.length;
  console.log(
    `\nLe attese e NON_ANCORA dello Studio non dicono la stessa cosa:\n`,
  );
  for (const nome of soloAttese) {
    console.log(`  ${nome} — sta in ATTESE e non in studio/scene.tsx`);
  }
  for (const nome of soloStudio) {
    console.log(`  ${nome} — sta in studio/scene.tsx e non in ATTESE`);
  }
  console.log(
    `\n  Chi ridipinge chiede allo Studio perché una parte non si vede, e la` +
      `\n  risposta viene da là: i due elenchi vanno tenuti uguali.`,
  );
}

// 4. Una regola del foglio che non veste niente.
//
// Il verso opposto del primo controllo, e vale la pena dire perché le fonti da
// cui si guarda sono tre e non una. Il markup è l'ovvia. Il registro conta
// perché una parte è un bersaglio legittimo di selettore anche prima che l'app
// la emetta — è tutto il senso di `ATTESE` qui sopra, e contarla morta sarebbe
// chiedere di cancellare proprio le regole che tengono in piedi la promessa
// alle skin. I nomi di stato contano perché non sono classi di nessuno.
const senzaMarkup = [...foglio]
  .filter((nome) => !dove.has(nome))
  .filter((nome) => !parti.has(nome))
  .filter((nome) => !NON_PARTI.has(nome))
  .filter((nome) => !(nome in ATTESE_FOGLIO))
  .sort();

if (senzaMarkup.length > 0) {
  guai += senzaMarkup.length;
  console.log(`\n${senzaMarkup.length} regole del foglio non vestono niente:\n`);
  for (const nome of senzaMarkup) console.log(`  .${nome}`);
  console.log(
    "\n  Nessun elemento porta questi nomi, quindi queste regole non si vedono:\n" +
      "  o sono sopravvissute alla schermata che le montava e vanno tolte da\n" +
      "  stile.css, o il nome si compone lontano dal `className` e la voce va in\n" +
      "  ATTESE_FOGLIO, qui dentro, col motivo scritto.",
  );
}

// 5. Un'attesa che non è più tale: quel che aspettava è arrivato.
const atteseScadute = [
  ...Object.keys(ATTESE)
    .filter((nome) => nellApp.has(nome))
    .map((nome) => `${nome} — adesso l'app la emette, togliela da ATTESE.`),
  ...Object.keys(ATTESE_CLASSI)
    .filter((nome) => foglio.has(nome))
    .map((nome) => `${nome} — adesso il foglio la disegna, togliela da ATTESE_CLASSI.`),
  // Una voce del foglio scade in due modi opposti: la regola è stata tolta, o
  // il markup ha smesso di nascondere il nome. In tutti e due i casi la riga
  // qui dentro racconta un albero che non c'è più.
  ...Object.keys(ATTESE_FOGLIO)
    .filter((nome) => !foglio.has(nome) || dove.has(nome))
    .map((nome) =>
      foglio.has(nome)
        ? `${nome} — adesso il markup la scrive per intero, togliela da ATTESE_FOGLIO.`
        : `${nome} — il foglio non la disegna più, togliela da ATTESE_FOGLIO.`,
    ),
];
if (atteseScadute.length > 0) {
  guai += atteseScadute.length;
  console.log(`\n${atteseScadute.length} attese non servono più:\n`);
  for (const riga of atteseScadute) console.log(`  ${riga}`);
}

if (guai === 0) {
  const attese =
    Object.keys(ATTESE).length +
    Object.keys(ATTESE_CLASSI).length +
    Object.keys(ATTESE_FOGLIO).length;
  console.log(
    `Tutto a posto: ${dove.size} classi nel markup, ${foglio.size} nomi ` +
      `disegnati dal foglio, ${parti.size} parti nel registro, ` +
      `${attese} attese dichiarate.`,
  );
}

process.exit(guai > 0 ? 1 : 0);
