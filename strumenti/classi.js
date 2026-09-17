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
 * Una parte però l'app la può emettere in **due** modi, e per un po' questo
 * strumento ne conosceva uno solo. Il primo è il `className` del markup. Il
 * secondo è l'albero di serie dello scafale, in `core/aether-skin/src/layout.rs`:
 * una zona dichiara `.parte("…")`, `Impaginazione.tsx` mette in classe
 * `nodo.part` senza sapere cosa ci sia scritto, e la parte finisce nel DOM
 * esattamente come le altre. `app-shell` è così, su tutti e due i rami di
 * `default_shell`. Finché si leggeva il solo TypeScript quella parte risultava
 * non emessa, e l'unico modo di far tacere il controllo era una voce in
 * `ATTESE` che diceva il falso — «il contenitore non porta la classe» — cioè
 * proprio la bugia contro cui questi controlli esistono. Leggere i letterali di
 * `layout.rs` chiude il caso **per costruzione**: il giorno in cui qualcuno
 * togliesse quel `.parte("app-shell")`, il controllo tornerebbe a parlare da
 * solo, che è quel che una deroga scritta a mano non avrebbe mai fatto.
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
 * # Quel che il foglio deve a sé stesso
 *
 * Gli ultimi controlli non confrontano il foglio col markup: confrontano il
 * foglio con una regola che il foglio stesso si è data, e che vale solo finché
 * vale **ovunque**. Stanno qui e non in una prova Rust perché il materiale è il
 * CSS, e qui il CSS lo si legge già.
 *
 * **Gli strati.** Undici `z-index` scelti uno alla volta non sono una scala: sono
 * undici decisioni locali, e la prova che non si componevano è che il menù
 * contestuale si disegnava dietro la barra della selezione che lo aveva fatto
 * aprire. Ora i numeri stanno in un posto solo, in cima a `stile.css`, e ogni
 * `z-index` del foglio deve essere uno di quei token. L'elenco delle eccezioni è
 * **vuoto**, e non per caso: un numero letterale fra i token non sarebbe uno
 * strato in più, sarebbe il ritorno del problema.
 *
 * **Le durate.** Il foglio dichiara che `prefers-reduced-motion` si rispetta
 * azzerando `--motion-scale`, e quella dichiarazione è vera soltanto se **ogni**
 * durata passa da lì. Non lo era: sei barre di avanzamento portavano `120ms`,
 * `160ms` e `200ms` scritti a mano e continuavano a scorrere con l'interruttore
 * di sistema acceso. Un commento non può garantirlo — questo controllo sì, ed è
 * la ragione per cui quel commento adesso si può scrivere al presente. Le
 * eccezioni motivate stanno in `ATTESE_MOVIMENTO`, e sono tutte dello stesso
 * tipo: animazioni che ripetono all'infinito, che una scala a zero non
 * fermerebbe — una durata nulla su un ciclo infinito è un ciclo infinito
 * istantaneo — e che quindi hanno una regola `prefers-reduced-motion` dedicata,
 * scritta a mano una per una.
 *
 * **Il puntatore.** Una superficie grande quanto il suo contenitore — `inset: 0`
 * su un elemento posizionato — o è decorazione, e allora il clic la attraversa,
 * o è un bersaglio, e allora se lo prende. La terza possibilità è non decidere,
 * ed è quella che si paga: lo scrim di «In riproduzione» portava la classe
 * `.velo`, ha ereditato lo strato che i veli avevano appena preso, e quella
 * schermata è diventata slavata e sorda a ogni clic **senza una riga nei log** —
 * un `aria-hidden` senza gestori che intercetta tutto non ha niente da
 * raccontare. Il controllo non sceglie al posto di nessuno: pretende che la
 * scelta sia scritta, nel blocco o in `ATTESE_PUNTATORE`.
 *
 * # Le attese dichiarate
 *
 * In fondo stanno gli elenchi — `ATTESE`, `ATTESE_CLASSI`, `ATTESE_FOGLIO`,
 * `ATTESE_MOVIMENTO`, `ATTESE_PUNTATORE`: quel che manca **e va bene**, ognuno col
 * suo motivo. Senza quelle liste lo strumento avrebbe ragione cinque volte su
 * dieci, che è il modo di non averla mai.
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
// L'albero di serie dello scafale. Le parti che dichiara finiscono in classe
// per mano di `Impaginazione.tsx`, che non sa quali siano: è markup emesso, e
// il solo motivo per cui va letto qui è che è scritto in Rust.
const IMPAGINAZIONE = "core/aether-skin/src/layout.rs";
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
  // Ed è **vuoto**, per la prima volta da quando esiste. Le quattro voci se ne
  // sono andate una alla volta, e ogni volta la stessa cosa: un'attesa dura
  // finché qualcuno la prende in carico o finché si ammette che nessuno lo
  // farà. Un elenco vuoto qui non è una svista da riempire — è il caso normale,
  // e il posto giusto per una parte nuova resta questo se e solo se il motivo
  // sta in una riga.
  //
  // `tour-tooltip` stava qui, e la sua riga diceva «non c'è ancora un giro
  // guidato». Adesso c'è: `Giro.tsx` monta il fumetto come
  // `<div className="giro-fumetto tour-tooltip">`, e una parte che l'app emette
  // non è un'attesa. Esce di qui e da `NON_ANCORA` di `studio/scene.tsx` nello
  // stesso passo, perché il controllo 3 vuole i due elenchi uguali — e allo
  // Studio non basta toglierla: `finto.ts` ha guadagnato l'interruttore «giro»
  // e `scene.tsx` disegna il velo, il buco e il fumetto, altrimenti la risposta
  // «sta in un'altra scena» rimanderebbe a una scena che non la mostra.
  //
  // `app-shell` stava qui, e la sua riga diceva il falso: la radice dello
  // scafale di serie porta `.parte("app-shell")` su tutti e due i rami di
  // `default_shell`, e `Impaginazione.tsx` la mette in classe. Non è uscita per
  // deroga — spostarla in `ATTESE_FOGLIO` non sarebbe nemmeno servito, perché
  // il controllo 4 filtra via i nomi che sono parti e la voce sarebbe scaduta
  // subito — ma perché il controllo 2 ha imparato a leggere anche `layout.rs`.
  //
  // `home-shortcuts` stava qui, e adesso sta in `parts::RITIRATE`: era l'unica
  // delle quattro attese senza un proprietario, e un'attesa che nessuno ha
  // preso in carico è una promessa che lo Studio non può mantenere. Come
  // `app-shell`, esce anche da `NON_ANCORA` di `studio/scene.tsx` nello stesso
  // passo, perché il controllo 3 vuole i due elenchi uguali.
  // `tooltip-pill` stava qui, e non ci sta più: il markup è arrivato. Le
  // linguette spente di `parti/Segmentato.tsx` sono `aria-disabled` e non
  // `disabled`, quindi restano focalizzabili e possono dire perché sono spente:
  // ognuna sta dentro il guscio `.con-ragione`, con la pastiglia appesa. La
  // voce esce di qui e da `NON_ANCORA` di `studio/scene.tsx` nello stesso
  // passo, perché il controllo 3 vuole i due elenchi uguali.
};

/*
 * Le durate che non passano da `--motion-scale`, con il perché.
 *
 * Una voce qui è un'animazione che **ripete all'infinito**, e per quelle la scala
 * non è un rimedio: `animation: x 0s linear infinite` è un ciclo infinito che
 * dura zero, cioè un difetto peggiore di quello che si voleva togliere. Si
 * fermano una per una, con una regola dedicata dentro
 * `@media (prefers-reduced-motion: reduce)`, e ogni motivo qui sotto nomina la
 * sua.
 *
 * Quella regola il controllo **non** la può verificare, e vale la pena dire
 * perché invece di lasciar credere che lo faccia: le quattro regole di
 * spegnimento non nominano l'animazione — spengono l'elemento, con `animation:
 * none` o con `display: none` — quindi non c'è niente da cercare. Quel che il
 * controllo verifica è il verso opposto, che è l'unico meccanizzabile: che il
 * foglio dichiari ancora `@keyframes <nome>`, così un'animazione ritirata non
 * lascia dietro di sé una scusa valida per sempre.
 *
 * Se un giorno arriva una voce che non ripete all'infinito, quella non è
 * un'eccezione: è una durata da avvolgere in `calc(… * var(--motion-scale, 1))`.
 */
const ATTESE_MOVIMENTO = {
  "aggiornamento-ignoto":
    "La barra di un aggiornamento senza `Content-Length`: non sa dove arriva, quindi scorre invece di riempirsi. Ferma sarebbe una banda parcheggiata a metà, cioè una bugia. Spenta da `.aggiornamento .toast-progress > span:not([style])`, che con meno movimento va al 100% e lascia dire la percentuale.",
  scorri:
    "Il riflesso che attraversa la barra della scansione. Dice che qualcosa sta succedendo anche quando la percentuale sta ferma per secondi interi — il nucleo annuncia ogni venticinque file. Spento da `.avanzamento-scansione .riflesso { display: none }`: sparisce il riflesso, non la barra.",
  scintilla:
    "Il luccichio dei segnaposto. È una posizione di sfondo che si muove, non uno pseudo-elemento, perché `skeleton` promette il suo `::after` alle skin. Spento da `.skeleton { animation: none }`: resta il riquadro — quello che dice «qui arriverà qualcosa» — e sparisce solo la banda.",
  "lettura-scorre":
    "La barra indeterminata della lettura di un collegamento: quante pagine saranno non si sa, e la barra non lo stima. Spenta da `.lettura-link .barra[data-indeterminata] .riempimento`, che resta ferma al 30% — cioè quel che quella barra vuol dire anche mentre si muove.",
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

/*
 * Le superfici a copertura piena che il puntatore lo prendono davvero, col perché.
 *
 * L'altra faccia del controllo qui sotto: `inset: 0` su un elemento posizionato
 * non vuol dire «decorazione», vuol dire «grande quanto il contenitore». Metà di
 * queste sono la schermata stessa, e una schermata i clic li prende perché sono
 * i suoi; l'altra metà sono veli, e un velo esiste **per** prendere il clic che
 * chiude.
 *
 * Una voce qui è una decisione, come in `ATTESE` e `ATTESE_FOGLIO`: se il motivo
 * non sta in una riga, quasi sempre quel che manca è un `pointer-events: none`.
 */
const ATTESE_PUNTATORE = {
  ".primo":
    "La schermata del primo avvio è la superficie, non quel che ci sta sopra: i clic sono i suoi.",
  ".in-riproduzione":
    "Come `.primo`: è la schermata. Quel che ci galleggia sopra — l'ambiente e lo scrim — il divieto ce l'ha scritto nel blocco suo.",
  ".editor-doppio .editor, .editor-doppio .sotto-editor":
    "La regola condivisa dai due strati incollati, che è l'unica cosa che hanno in comune: `.sotto-editor` si vieta il puntatore nel blocco suo, e `.editor` è la `<textarea>` su cui si scrive.",
  ".anteprima-documento .velo":
    "Copre l'anteprima ferma di proposito: è il velo che dice «in pausa» mentre il documento è rotto, e coprire è il suo mestiere.",
  ".velo":
    "Un velo è un bersaglio: prende il clic che chiude il menù o la finestrella. È la definizione, ed è il motivo per cui ha uno strato.",
  ".giro-velo":
    "Prende tutti i clic per progetto: il giro guidato avanza dai suoi tasti e non da un clic qualunque sulla finestra. La ragione lunga sta nel preambolo di `Giro.tsx`.",
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

/**
 * Le parti che l'albero di serie dello scafale mette in classe.
 *
 * Sono i letterali passati a `.parte("…")` in `layout.rs`. Si legge il
 * letterale e non il tipo perché è il letterale a finire nel DOM: la catena è
 * `LayoutZone::parte` → `LayoutNode` → `Impaginazione.tsx`, che scrive
 * `nodo.part` in classe senza guardarci dentro. Un nome composto a runtime qui
 * non si saprebbe leggere, e va bene: non ce ne sono, e il giorno in cui ce ne
 * fosse uno il controllo tornerebbe a lamentarsi invece di tacere.
 */
function partiDalloScafale(rust) {
  const trovate = new Set();
  for (const pezzo of rust.matchAll(/\.parte\(\s*"([a-z0-9-]+)"/g)) {
    trovate.add(pezzo[1]);
  }
  return trovate;
}

// ── il controllo ────────────────────────────────────────────────────────────

const file = sorgenti(SORGENTI);
const foglio = classiDelFoglio(fs.readFileSync(FOGLIO, "utf8"));
const parti = partiDelRegistro(fs.readFileSync(REGISTRO, "utf8"));
const dalloScafale = partiDalloScafale(fs.readFileSync(IMPAGINAZIONE, "utf8"));

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

/**
 * Le parti che l'applicazione emette davvero, da qualunque delle due sorgenti.
 *
 * È l'unione di quel che il markup scrive a mano e di quel che lo scafale di
 * serie dichiara in Rust. I due controlli che chiedono «l'app la disegna?» —
 * il secondo e il quinto — guardano qui e non solo il TypeScript: sono la
 * stessa domanda, e una risposta che dipende da quale dei due file l'ha
 * scritta sarebbe una risposta sbagliata la metà delle volte.
 */
const emesse = new Set([...nellApp, ...dalloScafale]);

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
  .filter((nome) => !emesse.has(nome))
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
      "  di solito accanto alla classe italiana che già c'è, oppure da\n" +
      "  layout.rs con .parte(\"…\") se è una zona dello scafale — o vanno messe\n" +
      "  in ATTESE, qui dentro, con il motivo scritto.",
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
    .filter((nome) => emesse.has(nome))
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

// ── il foglio contro sé stesso ──────────────────────────────────────────────

/*
 * Il testo del foglio senza commenti, ma con le righe al loro posto.
 *
 * I commenti si tolgono per la stessa ragione di `classiDelFoglio` — qui dentro
 * si cita il CSS per spiegarlo, e una citazione non è una dichiarazione — e si
 * sostituiscono con altrettanti ritorni a capo invece che con uno spazio: i due
 * controlli qui sotto dicono la riga, e una riga sbagliata di quattrocento manda
 * a cercare nel posto sbagliato.
 */
const cssCrudo = fs.readFileSync(FOGLIO, "utf8");
const cssPulito = cssCrudo.replace(/\/\*[\s\S]*?\*\//g, (pezzo) =>
  pezzo.replace(/[^\n]/g, " "),
);

/** La riga (da 1) a cui sta un indice dentro il foglio. */
function rigaDi(indice) {
  return cssPulito.slice(0, indice).split("\n").length;
}

/**
 * Il selettore che apre il blocco in cui sta un indice.
 *
 * Serve al messaggio: «z-index letterale a riga 3571» fa cercare, «`.barra-selezione`
 * a riga 3571» fa capire. Si risale all'ultima graffa aperta e si prende quel che
 * la precede fino al confine del blocco prima.
 */
function selettoreDi(indice) {
  const prima = cssPulito.lastIndexOf("{", indice);
  if (prima < 0) return "?";
  const confine = Math.max(
    cssPulito.lastIndexOf("}", prima),
    cssPulito.lastIndexOf("{", prima - 1),
    cssPulito.lastIndexOf(";", prima),
  );
  return cssPulito
    .slice(confine + 1, prima)
    .trim()
    .replace(/\s+/g, " ");
}

/** Il valore di una dichiarazione che comincia a `da`, fino al `;` o alla graffa. */
function valoreDa(da) {
  const fine = cssPulito.slice(da).search(/[;}]/);
  return cssPulito.slice(da, fine < 0 ? cssPulito.length : da + fine);
}

// 6. Ogni `z-index` è uno strato della scala.
//
// L'elenco delle eccezioni non esiste, e il suo non esistere è la regola: il
// difetto che la scala ha chiuso era proprio la convivenza fra numeri scelti
// altrove, quindi un'eccezione qui sarebbe la prima crepa.
const strati = [];
for (const trovato of cssPulito.matchAll(/(?<![-\w])z-index\s*:/g)) {
  const da = trovato.index + trovato[0].length;
  const valore = valoreDa(da);
  if (/var\(\s*--strato-[a-z]+\s*\)/.test(valore)) continue;
  strati.push(
    `  stile.css:${rigaDi(trovato.index)}  ${selettoreDi(trovato.index)}  →  z-index:${valore}`,
  );
}

if (strati.length > 0) {
  guai += strati.length;
  console.log(`\n${strati.length} z-index non vengono dalla scala degli strati:\n`);
  for (const riga of strati) console.log(riga);
  console.log(
    "\n  La scala sta in apps/desktop/src/stile.css, nel primo :root dopo la riga\n" +
      "  «fine blocco generato»: dodici token --strato-*, dal decoro alla finestra.\n" +
      "  Un numero scritto qui non si può confrontare con gli altri undici senza\n" +
      "  aprire il foglio, ed è così che il menù contestuale è finito dietro la\n" +
      "  barra della selezione. Scegli il token che dice il ruolo; se nessuno\n" +
      "  dei dodici lo dice, il posto dove aggiungerne uno è la scala, non qui.",
  );
}

// 7. Nessuna durata sfugge a `--motion-scale`.
//
// Il foglio promette che azzerare la scala ferma tutto. Vale se ogni durata è un
// `var(--transition-*)` (che la scala la porta dentro) o un `calc` che la nomina.
// Un `var(--dur-N)` nudo conta come letterale: è lo stesso numero con un nome.
const movimento = [];
for (const trovato of cssPulito.matchAll(
  /(?<![-\w])(?:transition|animation)(?:-duration|-delay)?\s*:/g,
)) {
  const valore = valoreDa(trovato.index + trovato[0].length);

  // Si toglie quel che è già scalato: i due token di transizione, e ogni `calc`
  // che nomina la scala — con le parentesi contate, perché dentro ce n'è un'altra.
  let resto = valore.replace(/var\(\s*--transition-[a-z]+\s*\)/g, " ");
  for (;;) {
    const apre = resto.indexOf("calc(");
    if (apre < 0) break;
    let i = apre + 5;
    let profondita = 1;
    while (i < resto.length && profondita > 0) {
      if (resto[i] === "(") profondita++;
      else if (resto[i] === ")") profondita--;
      i++;
    }
    const dentro = resto.slice(apre, i);
    if (!dentro.includes("--motion-scale")) break;
    resto = resto.slice(0, apre) + " " + resto.slice(i);
  }

  const letterali = [
    ...resto.matchAll(/(?<![\w.-])\d*\.?\d+m?s(?![\w-])/g),
    ...resto.matchAll(/var\(\s*--dur-\d+\s*\)/g),
  ].map((t) => t[0]);
  if (letterali.length === 0) continue;

  // Un'eccezione vale se il nome dell'animazione è dichiarato qui sopra **e** il
  // blocco del movimento ridotto la nomina: la riga di `ATTESE_MOVIMENTO` dice
  // perché, la regola dice che è stato fatto.
  const scusata = Object.keys(ATTESE_MOVIMENTO).find((nome) =>
    new RegExp(`(?<![\\w-])${nome}(?![\\w-])`).test(valore),
  );
  if (scusata) continue;

  movimento.push(
    `  stile.css:${rigaDi(trovato.index)}  ${selettoreDi(trovato.index)}  →  ${letterali.join(", ")}`,
  );
}

if (movimento.length > 0) {
  guai += movimento.length;
  console.log(
    `\n${movimento.length} durate non passano da --motion-scale:\n`,
  );
  for (const riga of movimento) console.log(riga);
  console.log(
    "\n  @media (prefers-reduced-motion) azzera --motion-scale, e quel blocco dice\n" +
      "  di fermare tutto il foglio: è vero solo per le durate che passano da lì.\n" +
      "  Le forme buone sono due — var(--transition-fast|med), oppure\n" +
      "  calc(<n>ms * var(--motion-scale, 1)) quando la curva deve restare la sua.\n" +
      "  Un'animazione che ripete all'infinito è l'unica eccezione legittima, e va\n" +
      "  in ATTESE_MOVIMENTO qui dentro **insieme** alla sua regola di spegnimento.",
  );
}

// 8. Un'eccezione del movimento che non serve più.
const movimentoScadute = Object.keys(ATTESE_MOVIMENTO)
  .filter((nome) => !new RegExp(`@keyframes\\s+${nome}(?![\\w-])`).test(cssPulito))
  .map((nome) => `  ${nome} — il foglio non la dichiara più, togliila da ATTESE_MOVIMENTO.`);
if (movimentoScadute.length > 0) {
  guai += movimentoScadute.length;
  console.log(`\n${movimentoScadute.length} eccezioni del movimento non servono più:\n`);
  for (const riga of movimentoScadute) console.log(riga);
}

// 9. Chi copre tutto dichiara se prende il puntatore.
//
// `inset: 0` su un elemento `absolute` o `fixed` copre il contenitore intero. Da
// lì in poi ci sono due comportamenti possibili e nessun valore di serie che li
// distingua: il clic passa, o si ferma. Il controllo pretende che il blocco lo
// dica — `pointer-events`, un valore qualunque — oppure che il selettore stia in
// `ATTESE_PUNTATORE` con la sua riga.
//
// Si guarda il blocco e non la cascata. Leggere la cascata vorrebbe dire
// scrivere un motore CSS per trovare una dimenticanza, e un divieto scritto in
// un'altra regola sullo stesso selettore è più raro del difetto che si cerca:
// se capita, la voce in `ATTESE_PUNTATORE` lo dice in una riga.
const puntatore = [];
/** I selettori a copertura piena che il divieto non ce l'hanno scritto. */
const scoperti = new Set();
for (const trovato of cssPulito.matchAll(/(?<![-\w])inset\s*:\s*0\s*(?=[;}])/g)) {
  const apre = cssPulito.lastIndexOf("{", trovato.index);
  const chiude = cssPulito.indexOf("}", trovato.index);
  if (apre < 0 || chiude < 0) continue;
  const blocco = cssPulito.slice(apre + 1, chiude);
  if (!/(?<![-\w])position\s*:\s*(?:absolute|fixed)/.test(blocco)) continue;
  if (/(?<![-\w])pointer-events\s*:/.test(blocco)) continue;
  const selettore = selettoreDi(trovato.index);
  scoperti.add(selettore);
  if (Object.hasOwn(ATTESE_PUNTATORE, selettore)) continue;
  puntatore.push(`  stile.css:${rigaDi(trovato.index)}  ${selettore}`);
}

if (puntatore.length > 0) {
  guai += puntatore.length;
  console.log(
    `\n${puntatore.length} superfic${puntatore.length === 1 ? "ie copre" : "i coprono"} tutto senza dire del puntatore:\n`,
  );
  for (const riga of puntatore) console.log(riga);
  console.log(
    "\n  Chi copre tutto decide per tutti: senza `pointer-events` dichiarato il\n" +
      "  clic si ferma lì, e quel che sta sotto smette di rispondere senza un\n" +
      "  errore da nessuna parte — è così che lo scrim di «In riproduzione» ha\n" +
      "  spento un'intera schermata restando invisibile ai log. Se è decorazione\n" +
      "  — un velo, un riflesso, una tela, un `aria-hidden` — scrivi `pointer-events: none`.\n" +
      "  Se i clic sono davvero suoi, la voce va in ATTESE_PUNTATORE qui dentro,\n" +
      "  con la riga che dice perché.",
  );
}

// 10. Un'eccezione del puntatore che non serve più.
const puntatoreScadute = Object.keys(ATTESE_PUNTATORE)
  .filter((selettore) => !scoperti.has(selettore))
  .map(
    (selettore) =>
      `  ${selettore} — o non copre più tutto, o il divieto adesso ce l'ha: togliila da ATTESE_PUNTATORE.`,
  );
if (puntatoreScadute.length > 0) {
  guai += puntatoreScadute.length;
  console.log(`\n${puntatoreScadute.length} eccezion${puntatoreScadute.length === 1 ? "e del puntatore non serve" : "i del puntatore non servono"} più:\n`);
  for (const riga of puntatoreScadute) console.log(riga);
}

if (guai === 0) {
  const attese =
    Object.keys(ATTESE).length +
    Object.keys(ATTESE_CLASSI).length +
    Object.keys(ATTESE_FOGLIO).length +
    Object.keys(ATTESE_MOVIMENTO).length +
    Object.keys(ATTESE_PUNTATORE).length;
  console.log(
    `Tutto a posto: ${dove.size} classi nel markup, ${foglio.size} nomi ` +
      `disegnati dal foglio, ${parti.size} parti nel registro, ` +
      `${attese} attese dichiarate.`,
  );
}

process.exit(guai > 0 ? 1 : 0);
