/**
 * Le istruzioni che il modello riceve: il formato, il vocabolario chiuso, la
 * selezione, il documento, e come si scrive una modifica.
 *
 * # Perché si costruiscono qui e non nel nucleo
 *
 * Perché qui il registro, il documento e la selezione sono già in memoria — lo
 * Studio li ha per disegnare — e perché il pannello deve poter dire **quanto**
 * sta mandando. `rymembra-ia` resta ignorante di cosa sia una skin: queste righe
 * arrivano al client come un messaggio come gli altri.
 *
 * # Cosa viene dal registro, e cosa è scritto qui
 *
 * Dal registro viene ogni **nome**: token con il loro tipo, parti con quel che
 * ammettono (livelli, entrata e uscita, volo), stati nell'ordine della cascata,
 * inneschi, campi dei fotogrammi con i loro estremi, animazioni pronte,
 * effetti con un esemplare valido, parole chiuse, chiavi per contesto, budget.
 * Sono le stesse liste con cui il validatore rifiuta: un nome che il modello
 * legge qui è un nome che il salvataggio accetta.
 *
 * Qui è scritta la **grammatica**: che `background` è una pila, che uno stato
 * contiene solo i campi che cambiano, che un ciclo infinito sta solo su uno
 * stato. Sono proprietà del formato, e se il formato cambia cambia
 * `SKIN_FORMAT_VERSION` — un cambio maggiore, che si vede.
 *
 * **Non** ci vanno le descrizioni del registro: sono la metà pesante — una
 * frase per ognuna delle duecento voci — e sono scritte per una persona.
 *
 * # Il contesto leggero
 *
 * Il documento viaggia compatto (senza rientri: a un modello non servono) e,
 * oltre una soglia, **ridotto**: restano intere le sezioni piccole e quelle
 * che servono a questa domanda — la parte selezionata, le parti e le
 * animazioni nominate nella richiesta o negli errori, le animazioni che quelle
 * parti usano — e delle altre si elencano solo i nomi. La cronologia, oltre
 * una soglia sua, tiene intere le ultime battute e riassume le vecchie
 * (`storiaLeggera`). Tutte e due le soglie stanno sotto il contesto degli
 * otto-kilo-gettoni, che è il tetto dei modelli locali piccoli.
 *
 * # In inglese
 *
 * È l'unico testo dell'applicazione scritto per essere letto da un modello, e
 * i modelli piccoli seguono le istruzioni in inglese molto meglio — mentre la
 * **conversazione** resta nella lingua di chi scrive, perché quella la legge
 * una persona. Non passa dal catalogo delle lingue per la stessa ragione: e
 * per questo il modulo non importa `../lingue`, e si prova con `node --test`.
 */
import { LINGUAGGIO_PATCH_PRECEDENTE } from "../eredita";
import type { MessaggioIa, Problema, Registro, Validazione } from "../ipc";
import type { MessaggioChat } from "./conversazione";
import type { Selezione, Tema } from "./negozio";
import { accorciato } from "./taglio";

/** Quanti caratteri vale un gettone, per la stima che si mostra. */
const CARATTERI_PER_GETTONE = 4;

/** Quanti errori di validazione si rimandano indietro in un giro. */
const ERRORI_DA_MOSTRARE = 12;

/**
 * Oltre questa lunghezza, compatto, il documento si riduce.
 *
 * Plain intera ne pesa cinquemila, Vinile e Neon diecimila: sotto la soglia
 * passano intere, ed è il caso comune. Una skin cresciuta a trenta parti
 * ridipinte e venti animazioni ne pesa il doppio, e lì la riduzione comincia
 * a valere più di quel che toglie.
 */
export const SOGLIA_DOCUMENTO = 12_000;

/** Oltre questa lunghezza la cronologia si riassume. */
export const SOGLIA_STORIA = 12_000;

/** Quante battute in coda restano intere quando la cronologia si riassume. */
const BATTUTE_INTERE = 4;

/**
 * Il contratto d'uscita: come si scrive una modifica.
 *
 * Un blocco `rymembra-patch` di operazioni per percorso, e non il documento
 * riscritto: un modello che riscrive venti kilobyte per cambiare un raggio
 * sbaglia, prima o poi, uno dei diciannove che non doveva toccare.
 */
const CONTRATTO = `You edit a Rymembra skin manifest: a JSON document that paints a music player.

To change it, emit ONE fenced block with the language tag \`rymembra-patch\`,
containing a JSON array of operations. Nothing else in the block.

\`\`\`rymembra-patch
[{"op": "scrivi", "percorso": ["tokens", "color.surface.0"], "valore": "#050508"},
 {"op": "scrivi", "percorso": ["parts", "card", "states", "hover", "shadow"], "valore": {"$token": "shadow.2"}},
 {"op": "togli", "percorso": ["parts", "section-card", "radius"]}]
\`\`\`

- \`op\` is "scrivi" (set a value, creating what is missing) or "togli" (remove).
- \`percorso\` is an ARRAY of steps from the document root — never a dotted
  string. A step is an object key, or a list position counted from 0.
    right: ["parts", "card", "background", "0", "stops", "0", "color"]
    wrong: "parts.card.background[0].stops[0].color"
- \`valore\` is required for "scrivi" and must be absent for "togli".

Lists, and how to address them:
- These are lists: every \`background\` (a STACK of effect layers, bottom
  first — also inside \`before\`, \`after\` and states), \`filter\`, \`backdrop\`,
  \`shadow.layers\`, \`frames\`, gradient \`stops\` and \`at\`, \`points\`,
  \`meta.preview\`, \`layout.shell.children\`.
- There is no "add" operation. To ADD an item, "scrivi" at the position equal
  to the list's current length — count it in the document. A position further
  out is refused, because it would leave a hole.
- To REMOVE one, "togli" that position: the ones after it shift down.
- To REORDER, or to insert elsewhere, write the whole list in one "scrivi".
- Otherwise change one item at a time by its position. Rewriting a whole stack
  to change one colour is how you lose the other layers.`;

/**
 * La grammatica del formato 2, in una pagina.
 *
 * Tutto quel che qui è un elenco di nomi viene dal registro; le frasi sono
 * quel che il registro non può dire.
 */
function grammatica(registro: Registro): string {
  const chiavi = (contesto: string) => (registro.chiavi[contesto] ?? []).join(", ");
  const stati = registro.stati.map((s) => s.name).join(" ");
  const inneschi = registro.inneschi
    .filter((i) => !i.stato)
    .map((i) => i.name)
    .join(", ");
  return `FORMAT ${registro.format} — the shape of a skin

Top level: ${chiavi("documento")}.
- scheme: the base theme, "dark" or "light". \`variant\` is the OPPOSITE theme:
  it may override ${chiavi("variante")}; whatever it does not override is
  inherited. When the user says "the light theme" or "the dark theme", write to
  the base or to \`variant\` according to \`scheme\`.
- tokens: the design variables, by id (listed below). palette: the skin's own
  named colours. patterns: named background layers, used as
  {"$pattern": "name"} inside a background stack.
- Colour values: "#rrggbb", "#rrggbbaa", "rgba(…)", {"$token": "color.accent"},
  {"$palette": "name", "alpha": 0.4}, or a colour that follows the playing cover:
  {"$source": "<source>", "alpha"?: 0.5}. Lengths and durations are strings
  with a unit: "12px", "180ms".

parts.<name> paints one surface of the app. Appearance fields:
  ${chiavi("aspetto")}
  - radius: "8px", or per corner {${chiavi("angoli")}}.
  - shadow: {"$token": "shadow.2"} or {"layers": [{${chiavi("livelloOmbra")}}]}.
  - focusRing: {${chiavi("anello")}}. font: sans | mono | display.
  - filter (on the element) and backdrop (behind it) are stacks of filter
    effects; clip is a clip effect.
  - before / after: one extra layer {${chiavi("livello")}} behind the content
    (before) or above the background (after) — only on parts marked b / a.
  - states.<state>: only the appearance fields that CHANGE in that state.
    States, in cascade order (a later one wins): ${stati}.
  - motion: which animation plays when. See below.

motion (top level): ${chiavi("movimento")}.
- motion.animations.<name>: {${chiavi("animazione")}}.
  frames: at least ${registro.minFotogrammi}, at most ${registro.maxFotogrammi}, each {"at": 0..100, <frame fields>}.
  iterations: 1..${registro.maxIterazioni}, or "infinite".
- motion.easings.<name>: {"kind": "cubicBezier", "points": [x1, y1, x2, y2]}
  or {"kind": "steps", "count": 4, "position": "end"}. An animation's easing is
  a keyword (listed below) or the name of one of these.
- motion.route: the page change, {${chiavi("rotta")}}; out / in hold ${chiavi("fotogrammaRotta")}.
- parts.<name>.motion maps a trigger to an animation NAME, plus ${chiavi("movimentoParte")}:
    {"enter": "rise", "hover": "pulse", "stagger": "30ms",
     "transition": {"duration": "160ms", "easing": "ease-out"}}
  Triggers: ${inneschi}, and every state name.
  enter / exit only on parts marked p (they come and go); share only on parts
  marked s (they fly between screens); inView when scrolled into view; change
  when the content changes. "infinite" iterations ONLY on a state trigger
  (e.g. "playing": "spin"). transition smooths appearance changes between
  states. Reduced motion is handled by the compiler: never add it yourself.
- Cost: each part has a surface budget (${registro.budget}) and a motion budget
  (${registro.motionBudget}); the whole window ${registro.windowMotionBudget}. Going over is a warning.

fonts: families from files already in the package. You cannot add files: if a
font or an image is missing, say so.`;
}

/**
 * Il vocabolario chiuso, compatto.
 *
 * I token per tipo e non per gruppo: quel che il modello deve sapere di
 * `color.surface.0` è che accetta un colore, non che appartiene a «Superfici».
 * Le parti con una sigla per quel che ammettono: una parte senza `a` rifiuta
 * un `after`, e senza la sigla il modello lo scriverebbe su tutte.
 */
function vocabolario(registro: Registro): string {
  const perTipo = new Map<string, string[]>();
  for (const token of registro.tokens) {
    const elenco = perTipo.get(token.kind) ?? [];
    elenco.push(token.id);
    perTipo.set(token.kind, elenco);
  }
  const sigla = (p: Registro["parts"][number]) => {
    const segni = `${p.before ? "b" : ""}${p.after ? "a" : ""}${p.presenza ? "p" : ""}${p.condivisa ? "s" : ""}`;
    return segni === "" ? p.name : `${p.name}:${segni}`;
  };
  const estremi = (c: Registro["fotogramma"][number]) =>
    c.min !== null && c.max !== null ? `${c.name} ${c.min}..${c.max}` : `${c.name} (${c.kind})`;
  const parole = Object.entries(registro.parole)
    .filter(([campo]) => campo !== "iterations")
    .map(([campo, nomi]) => `  ${campo}: ${nomi.join(" | ")}`);

  return [
    `VOCABULARY — use ONLY these names`,
    "",
    "tokens, by value type:",
    ...[...perTipo.entries()].map(([tipo, nomi]) => `  ${tipo}: ${nomi.join(" ")}`),
    "",
    "parts (b = before layer, a = after layer, p = enter/exit, s = share):",
    `  ${registro.parts.map(sigla).join(" ")}`,
    "",
    "frame fields (numbers are plain numbers; translate in px, rotate/skew/hueRotate in degrees):",
    `  ${registro.fotogramma.map(estremi).join(", ")}`,
    "",
    "ready-made animations (copy the definition into motion.animations first, then name it):",
    ...registro.animazioniPronte.map((a) => `  ${a.id} (for ${a.innesco}): ${compatto(a.valore)}`),
    "",
    // L'esemplare viene dal nucleo, che lo costruisce facendolo passare dal
    // parser vero: un esempio che non compila non esce.
    "effects (name, cost, and a minimal valid example):",
    ...registro.effects.map((e) => `  ${e.name} (cost ${e.cost}, ${e.target}): ${e.esempio}`),
    "",
    "closed words:",
    ...parole,
    `  $source: ${registro.sorgenti.join(" ")}`,
    "",
    "shell widgets:",
    `  ${registro.widgets.map((w) => w.name).join(" ")}`,
    `  zone: ${registro.vocabolario.zones.join(" ")}; gap: ${registro.vocabolario.gaps.join(" ")}; align: ${registro.vocabolario.aligns.join(" ")}; spread: ${registro.vocabolario.spreads.join(" ")}`,
  ].join("\n");
}

/** Un frammento JSON senza rientri; il testo com'è se non si legge. */
function compatto(testo: string): string {
  try {
    return JSON.stringify(JSON.parse(testo));
  } catch {
    return testo;
  }
}

/**
 * Il contratto d'uscita, ripetuto **dopo** il documento.
 *
 * # Perché è una ripetizione, e perché va tenuta
 *
 * Perché le stesse frasi in cima non bastavano, e non è un'ipotesi: con un
 * modello locale da ventisette miliardi di parametri, le istruzioni scritte
 * solo in testa producevano un blocco ```json con dentro il pezzo di documento
 * riscritto. Con queste righe in fondo, lo stesso modello e la stessa domanda
 * producono un blocco `rymembra-patch`. L'ultima forma vista è quella che un
 * modello imita: qui è il contratto e non il documento.
 *
 * Va **dopo gli errori**: in modalità agent quello è l'elenco che cresce, e
 * finirebbe lui a fare da ultima cosa letta.
 */
const CHIUSURA = `REMEMBER — the answer has exactly two parts, in this order:
1. one short sentence saying what you changed, in the user's language;
2. ONE fenced block tagged \`rymembra-patch\` holding the JSON array of operations.

Never a \`json\` block, and never the rewritten document: only the operations.
\`percorso\` is an array of steps — ["parts", "card", "background", "0"] — never
a dotted string. Use only names from the vocabulary; change only what was
asked. If the request cannot be done with this vocabulary, or nothing needs
changing, say so and emit no block.`;

// ── la selezione ────────────────────────────────────────────────────────────

/** Quel che lo Studio sta guardando, per le istruzioni. */
export interface Sguardo {
  selezione: Selezione | null;
  /** Lo stato della parte che l'ispettore modifica. */
  stato: string | null;
  tema: Tema;
}

/**
 * La riga che accompagna una richiesta: cosa era selezionato quando è stata
 * scritta.
 *
 * Sta **nel messaggio** e non solo nelle istruzioni: la cronologia deve
 * ricordare che «rendila più tonda», tre battute fa, parlava della scheda e
 * non del bottone selezionato adesso. `null` quando non c'è niente da dire.
 */
export function rigaSelezione({ selezione, stato, tema }: Sguardo): string | null {
  if (selezione === null || selezione.tipo === "documento") return null;
  const inTema = tema === "variante" ? ", in the variant theme" : "";
  switch (selezione.tipo) {
    case "parte":
      return `[Selected in the Studio: part "${selezione.nome}"${stato === null ? "" : `, state "${stato}"`}${inTema}]`;
    case "token":
      return `[Selected in the Studio: token "${selezione.id}"${inTema}]`;
    case "colore":
      return `[Selected in the Studio: palette colour "${selezione.nome}"${inTema}]`;
    case "motivo":
      return `[Selected in the Studio: pattern "${selezione.nome}"${inTema}]`;
    case "curva":
      return `[Selected in the Studio: easing "${selezione.nome}"]`;
    case "animazione":
      return `[Selected in the Studio: animation "${selezione.nome}"]`;
    case "carattere":
      return `[Selected in the Studio: font family "${selezione.nome}"]`;
    case "risorsa":
      return `[Selected in the Studio: asset file "${selezione.nome}"]`;
    case "nodo":
      return `[Selected in the Studio: shell node at ${JSON.stringify(percorsoNodo(selezione.via))}]`;
  }
}

/** Il percorso di un nodo dello scafale, dalla radice del documento. */
function percorsoNodo(via: readonly number[]): string[] {
  return ["layout", "shell", ...via.flatMap((i) => ["children", String(i)])];
}

type Oggetto = Record<string, unknown>;
const eOggetto = (v: unknown): v is Oggetto =>
  v !== null && typeof v === "object" && !Array.isArray(v);

/** Il valore a un percorso, o `undefined`. */
function a(documento: unknown, percorso: readonly string[]): unknown {
  let qui: unknown = documento;
  for (const passo of percorso) {
    if (Array.isArray(qui)) qui = qui[Number(passo)];
    else if (eOggetto(qui)) qui = qui[passo];
    else return undefined;
  }
  return qui;
}

/**
 * La selezione com'è scritta adesso, con il suo percorso.
 *
 * È la metà che la riga del messaggio non porta: il modello riceve la
 * definizione **di adesso** a ogni giro — anche quando il documento è ridotto
 * e la parte non ci sarebbe — e può scrivere il percorso giusto senza
 * cercarlo. Nel tema variante si mostrano tutte e due le metà: quel che la
 * variante sovrascrive, e quel che eredita.
 */
function dettaglioSelezione(documento: unknown, sguardo: Sguardo): string | null {
  const { selezione, tema } = sguardo;
  if (selezione === null || selezione.tipo === "documento" || selezione.tipo === "risorsa") {
    return null;
  }
  const percorsi: string[][] = [];
  const conVariante = (base: string[], variante: string[]) => {
    percorsi.push(base);
    if (tema === "variante") percorsi.push(variante);
  };
  switch (selezione.tipo) {
    case "parte":
      conVariante(["parts", selezione.nome], ["variant", "parts", selezione.nome]);
      break;
    case "token":
      conVariante(["tokens", selezione.id], ["variant", "tokens", selezione.id]);
      break;
    case "colore":
      conVariante(["palette", selezione.nome], ["variant", "palette", selezione.nome]);
      break;
    case "motivo":
      conVariante(["patterns", selezione.nome], ["variant", "patterns", selezione.nome]);
      break;
    case "curva":
      percorsi.push(["motion", "easings", selezione.nome]);
      break;
    case "animazione":
      percorsi.push(["motion", "animations", selezione.nome]);
      break;
    case "carattere":
      percorsi.push(["fonts", selezione.nome]);
      break;
    case "nodo":
      percorsi.push(percorsoNodo(selezione.via));
      break;
  }
  const righe = percorsi.map((p) => {
    const valore = a(documento, p);
    return `  ${JSON.stringify(p)}: ${valore === undefined ? "(not written yet)" : JSON.stringify(valore)}`;
  });
  return ["THE SELECTION, as it is written now:", ...righe].join("\n");
}

// ── il documento, leggero ───────────────────────────────────────────────────

/** I nomi di cui questa domanda ha bisogno per intero. */
export interface Pertinenti {
  parti: Set<string>;
  animazioni: Set<string>;
  scafale: boolean;
}

/**
 * `nome` compare in `testo` come parola intera — i nomi hanno trattini.
 *
 * Un nome vuoto non nomina niente, e va escluso prima del ciclo: con `""`
 * `indexOf` risponde sempre (arrivato in fondo ridà la lunghezza), e il `for`
 * non finiva più. Una chiave `""` in `parts` — scritta nel codice o da una
 * patch del modello — congelava la finestra mentre si scriveva nella chat.
 */
function nomina(testo: string, nome: string): boolean {
  if (nome === "") return false;
  const i = testo.indexOf(nome);
  if (i < 0) return false;
  const confine = /[a-z0-9.-]/i;
  for (let da = i; da >= 0; da = testo.indexOf(nome, da + 1)) {
    const prima = testo[da - 1] ?? "";
    const dopo = testo[da + nome.length] ?? "";
    if (!confine.test(prima) && !confine.test(dopo)) return true;
  }
  return false;
}

/** Le parole che fanno pensare allo scafale, in italiano e in inglese. */
const PAROLE_SCAFALE =
  /\b(layout|shell|scaffal|widget|zon[ae]|colonn|column|sidebar|barra laterale|impagina)/i;

/**
 * Quel che serve per intero a questa domanda.
 *
 * La selezione; le parti e le animazioni che la richiesta o gli errori
 * nominano; le animazioni che quelle parti usano. Lo scafale solo se la
 * domanda ne parla o un suo nodo è selezionato: è la sezione più lunga, e la
 * più lontana da «cambia questo colore».
 */
export function pertinenti(
  documento: unknown,
  sguardo: Sguardo,
  richiesta: string,
  problemi: readonly Problema[],
): Pertinenti {
  const parti = new Set<string>();
  const animazioni = new Set<string>();
  const { selezione } = sguardo;
  if (selezione?.tipo === "parte") parti.add(selezione.nome);
  if (selezione?.tipo === "animazione") animazioni.add(selezione.nome);

  const nomiParti = new Set([
    ...Object.keys(eOggetto(a(documento, ["parts"])) ? (a(documento, ["parts"]) as Oggetto) : {}),
    ...Object.keys(
      eOggetto(a(documento, ["variant", "parts"])) ? (a(documento, ["variant", "parts"]) as Oggetto) : {},
    ),
  ]);
  const tutteAnimazioni = a(documento, ["motion", "animations"]);
  const nomiAnimazioni = eOggetto(tutteAnimazioni) ? Object.keys(tutteAnimazioni) : [];

  const sorgenti = [richiesta, ...problemi.map((p) => p.path.split(".").join(" "))].join("\n");
  // Una chiave vuota non è un nome che una domanda possa fare: vedi `nomina`.
  for (const nome of nomiParti) if (nome !== "" && nomina(sorgenti, nome)) parti.add(nome);
  for (const nome of nomiAnimazioni) if (nome !== "" && nomina(sorgenti, nome)) animazioni.add(nome);

  // Le animazioni che le parti scelte usano: chi cambia l'entrata di una
  // scheda deve vedere l'animazione che la scheda chiama.
  for (const parte of parti) {
    for (const radice of [["parts"], ["variant", "parts"]]) {
      const movimento = a(documento, [...radice, parte, "motion"]);
      if (!eOggetto(movimento)) continue;
      for (const valore of Object.values(movimento)) {
        if (typeof valore === "string" && nomiAnimazioni.includes(valore)) animazioni.add(valore);
      }
    }
  }
  const scafale =
    selezione?.tipo === "nodo" ||
    PAROLE_SCAFALE.test(richiesta) ||
    problemi.some((p) => p.path.startsWith("layout"));
  return { parti, animazioni, scafale };
}

/**
 * Il documento come lo legge il modello.
 *
 * Compatto sempre; ridotto oltre [`SOGLIA_DOCUMENTO`]. Un testo che non è
 * JSON passa com'è: è rotto, e per aggiustarlo va visto tutto.
 *
 * Restituisce anche i nomi tolti, che le istruzioni elencano: un modello che
 * non sa che `card` esiste la dichiarerebbe da capo sopra quella che c'è.
 */
export function documentoLeggero(
  testo: string,
  quali: Pertinenti,
  soglia = SOGLIA_DOCUMENTO,
): { testo: string; tolti: string[] } {
  let documento: unknown;
  try {
    documento = JSON.parse(testo);
  } catch {
    return { testo, tolti: [] };
  }
  const intero = JSON.stringify(documento);
  if (intero.length <= soglia || !eOggetto(documento)) return { testo: intero, tolti: [] };

  const tolti: string[] = [];
  const filtra = (dove: string, mappa: unknown, tieni: Set<string>): unknown => {
    if (!eOggetto(mappa)) return mappa;
    const fuori = Object.keys(mappa).filter((k) => !tieni.has(k));
    if (fuori.length > 0) tolti.push(`${dove}: ${fuori.join(" ")}`);
    return Object.fromEntries(Object.entries(mappa).filter(([k]) => tieni.has(k)));
  };

  const ridotto: Oggetto = { ...documento };
  ridotto.parts = filtra("parts", documento.parts, quali.parti);
  if (eOggetto(documento.variant)) {
    ridotto.variant = {
      ...documento.variant,
      parts: filtra("variant.parts", documento.variant.parts, quali.parti),
    };
  }
  if (eOggetto(documento.motion)) {
    ridotto.motion = {
      ...documento.motion,
      animations: filtra("motion.animations", documento.motion.animations, quali.animazioni),
    };
  }
  if (!quali.scafale && eOggetto(documento.layout)) {
    const { shell, prefabs, ...resto } = documento.layout;
    if (shell !== undefined || prefabs !== undefined) {
      tolti.push("layout: shell and prefabs (the window's arrangement)");
    }
    ridotto.layout = resto;
  }
  return { testo: JSON.stringify(ridotto), tolti };
}

// ── le istruzioni ───────────────────────────────────────────────────────────

/** Tutto quel che le istruzioni devono sapere di questo giro. */
export interface Contesto {
  registro: Registro;
  /** Il testo del documento di adesso. */
  documento: string;
  esito: Validazione | null;
  sguardo: Sguardo;
  /** L'ultima richiesta di chi scrive: decide cosa del documento serve. */
  richiesta: string;
}

/** Le istruzioni complete, per questo registro, questo documento, questa domanda. */
export function istruzioni(c: Contesto): string {
  let letto: unknown = null;
  try {
    letto = JSON.parse(c.documento);
  } catch {
    // Il documento è rotto: niente dettaglio della selezione, e il testo
    // intero sotto.
  }
  const problemi = c.esito?.errori ?? [];
  const quali = pertinenti(letto, c.sguardo, c.richiesta, problemi);
  const leggero = documentoLeggero(c.documento, quali);

  const pezzi = [CONTRATTO, grammatica(c.registro), vocabolario(c.registro)];
  const dettaglio = letto === null ? null : dettaglioSelezione(letto, c.sguardo);
  if (dettaglio !== null) pezzi.push(dettaglio);
  pezzi.push(`CURRENT DOCUMENT (compact JSON):\n${leggero.testo}`);
  if (leggero.tolti.length > 0) {
    pezzi.push(
      [
        "Left out above to save space — these EXIST in the document; do not declare them again:",
        ...leggero.tolti.map((riga) => `  ${riga}`),
      ].join("\n"),
    );
  }
  const aperti = errori(problemi);
  if (aperti !== null) pezzi.push(aperti);
  pezzi.push(CHIUSURA);
  return pezzi.join("\n\n");
}

/** Il messaggio di sistema, pronto da mettere in testa alla conversazione. */
export function sistema(c: Contesto): MessaggioIa {
  return { ruolo: "system", testo: istruzioni(c) };
}

/**
 * Gli errori aperti, nelle stesse parole che il bottone «forse volevi dire»
 * già usa. `null` quando non ce ne sono: una riga «no errors» sarebbe
 * contesto speso per dire che non c'è niente da dire.
 */
export function errori(problemi: readonly Problema[]): string | null {
  if (problemi.length === 0) return null;
  const righe = problemi.slice(0, ERRORI_DA_MOSTRARE).map((p) => {
    const forse = p.forse.length > 0 ? ` (did you mean: ${p.forse.join(", ")})` : "";
    return `  ${p.code} at ${p.path}: ${p.message}${forse}`;
  });
  const coda =
    problemi.length > ERRORI_DA_MOSTRARE
      ? [`  … and ${problemi.length - ERRORI_DA_MOSTRARE} more`]
      : [];
  return ["The document currently has these errors:", ...righe, ...coda].join("\n");
}

/**
 * Il messaggio che apre un giro di correzione, in modalità agent.
 *
 * Porta gli errori del **candidato** e le ragioni di quel che si è scartato.
 * `applicato` distingue i due giri: quando il documento non è cambiato
 * affatto, «l'ho applicata e non è venuta pulita» è una frase falsa, e manda
 * il modello a cercare cosa ha rotto invece che a riscrivere quel che non è
 * nemmeno partito.
 */
export function correzione(
  problemi: readonly Problema[],
  ragioni: readonly string[],
  applicato = true,
): string {
  const pezzi = [
    applicato
      ? "I applied your patch. It did not come out clean."
      : "None of that could be applied: the document is unchanged.",
  ];
  const aperti = errori(problemi);
  if (aperti !== null) pezzi.push(aperti);
  if (ragioni.length > 0) {
    pezzi.push(
      ["Some operations were rejected before being applied:", ...ragioni.map((r) => `  ${r}`)].join(
        "\n",
      ),
    );
  }
  // «Do not repeat what already works» dopo un giro in cui **non** ha
  // funzionato niente dice al modello che c'è qualcosa da conservare: visto
  // davvero, un secondo giro che rispondeva a parole e senza blocco.
  pezzi.push(
    applicato
      ? "Emit a new block that fixes only these. Do not repeat what already works."
      : "Emit the same change again as one `rymembra-patch` block, in the correct form.",
  );
  return pezzi.join("\n\n");
}

// ── la cronologia, leggera ──────────────────────────────────────────────────

/**
 * Il linguaggio, quello che segue i tre apici, del blocco con cui il modello
 * propone le modifiche. È l'unico che si insegna al modello e che si scrive.
 */
export const LINGUAGGIO_PATCH = "rymembra-patch";

/**
 * Un blocco di modifiche dentro una risposta, anche ancora aperto.
 *
 * Riconosce il tag di adesso e quello di prima (`LINGUAGGIO_PATCH_PRECEDENTE`):
 * le chat salvate sul disco, e la cronologia che torna al modello, contengono
 * ancora il vecchio, e un blocco di allora, applicato o scartato, deve
 * comportarsi come uno di adesso. È globale: si usa con `match`, `matchAll` e
 * `replace`, mai con `test` o `exec`, che si porterebbero dietro `lastIndex`.
 * Per sapere solo se c'è un blocco c'è [`haBloccoDiModifiche`].
 */
export const BLOCCO = new RegExp(
  "```(?:" + LINGUAGGIO_PATCH + "|" + LINGUAGGIO_PATCH_PRECEDENTE + ")[\\s\\S]*?(?:```|$)",
  "g",
);

/** L'apertura di un blocco di modifiche, senza stato: vedi [`BLOCCO`]. */
const APERTURA_BLOCCO = new RegExp("```(?:" + LINGUAGGIO_PATCH + "|" + LINGUAGGIO_PATCH_PRECEDENTE + ")");

/**
 * Il testo contiene un blocco di modifiche, col tag di adesso o con quello di
 * prima. Al posto di un `includes` sul tag, che dimenticherebbe le chat
 * salvate prima del cambio di nome.
 */
export function haBloccoDiModifiche(testo: string): boolean {
  return APERTURA_BLOCCO.test(testo);
}

/**
 * Il testo con i blocchi di modifiche aperti dal tag di adesso.
 *
 * Al modello si mostra un solo nome: una cronologia che gli facesse rileggere
 * il tag di prima gliene insegnerebbe due, e un modello imita l'ultima forma
 * che ha visto. Il contenuto del blocco non si tocca.
 */
export function conTagNuovo(testo: string): string {
  return testo.replaceAll("```" + LINGUAGGIO_PATCH_PRECEDENTE, "```" + LINGUAGGIO_PATCH);
}

/**
 * Il testo accorciato a `quanti` caratteri, con un segno che lo dice.
 *
 * Senza spezzare una coppia di surrogati: vedi `taglio.ts`.
 */
function accorcia(testo: string, quanti: number): string {
  return accorciato(testo, quanti);
}

/** La riga che segue una proposta che non è stata applicata. */
export const NON_APPLICATA = `[this ${LINGUAGGIO_PATCH} block was NOT applied: the user discarded it]`;

/** Il segno di un blocco vecchio, nella cronologia riassunta. */
function segnoDelBlocco(applicata: boolean | undefined): string {
  return applicata === false
    ? `[a ${LINGUAGGIO_PATCH} block, NOT applied: the user discarded it]`
    : `[a ${LINGUAGGIO_PATCH} block, already applied]`;
}

/**
 * La cronologia da mandare: intera finché è corta, riassunta dopo.
 *
 * Riassunta vuol dire: le ultime [`BATTUTE_INTERE`] restano come sono, e nelle
 * vecchie i blocchi di modifiche diventano una riga — sono già applicati, e il
 * documento che il modello riceve li contiene — mentre le frasi si accorciano.
 * Le immagini restano solo sull'ultima richiesta in ogni caso: una fotografia
 * della tela di tre giri fa mostra una skin che non c'è più, e si pagherebbe
 * a ogni giro.
 *
 * # Le proposte scartate
 *
 * «Già applicati» era vero in modalità agent e falso in modalità proposta,
 * dove chi guarda può scartare: fino alla 3.0 la cronologia diceva al modello
 * che una proposta rifiutata era nel documento, e il modello costruiva il giro
 * dopo su una modifica che non c'era. Adesso la chat segna ogni risposta col
 * suo esito (`MessaggioChat.applicata`), e una scartata lo dice — riassunta o
 * no. Un messaggio senza segno, da una conversazione salvata prima, si
 * racconta come prima. Il segno non parte: si mandano solo `ruolo`, `testo` e,
 * sull'ultimo, le immagini.
 *
 * # Le conversazioni di prima del cambio di nome
 *
 * Le chat sul disco contengono ancora il tag di prima. Un loro blocco si
 * riconosce come uno di adesso — riassunto, o segnato come scartato — e le
 * risposte che restano intere arrivano al modello col tag di adesso
 * (`conTagNuovo`): del vecchio nome il modello non deve vedere traccia.
 */
export function storiaLeggera(
  conversazione: readonly MessaggioChat[],
  soglia = SOGLIA_STORIA,
): MessaggioIa[] {
  const ultima = conversazione.length - 1;
  const senzaVecchieImmagini: MessaggioIa[] = conversazione.map((m, i) => {
    const scartata =
      m.ruolo === "assistant" && m.applicata === false && haBloccoDiModifiche(m.testo);
    // Una risposta salvata col tag di prima arriva al modello col tag di adesso.
    const corrente = m.ruolo === "assistant" ? conTagNuovo(m.testo) : m.testo;
    const testo = scartata ? `${corrente}\n${NON_APPLICATA}` : corrente;
    return i === ultima && m.immagini !== undefined
      ? { ruolo: m.ruolo, testo, immagini: m.immagini }
      : { ruolo: m.ruolo, testo };
  });
  const peso = senzaVecchieImmagini.reduce((somma, m) => somma + m.testo.length, 0);
  if (peso <= soglia) return alternati(senzaVecchieImmagini);
  const confine = conversazione.length - BATTUTE_INTERE;
  const riassunti = senzaVecchieImmagini.map((m, i) => {
    if (i >= confine) return m;
    if (m.ruolo === "assistant") {
      const originale = conversazione[i]?.testo ?? m.testo;
      const blocchi = originale.match(BLOCCO)?.length ?? 0;
      const frasi = accorcia(originale.replace(BLOCCO, "").trim(), 300);
      return {
        ruolo: m.ruolo,
        testo:
          blocchi > 0 ? `${frasi}\n${segnoDelBlocco(conversazione[i]?.applicata)}` : frasi,
      };
    }
    return { ruolo: m.ruolo, testo: accorcia(m.testo, 500) };
  });
  return alternati(riassunti);
}

/**
 * I messaggi con i ruoli che si alternano: due dello stesso ruolo di fila
 * diventano uno, i testi separati da una riga vuota.
 *
 * Una domanda caduta per un guasto non resta più nella conversazione
 * (`senzaDomandaFallita`), ma le chat salvate prima della 3.2 possono averne:
 * certi modelli rifiutano due «user» di fila, e da lì ogni domanda di quella
 * conversazione falliva. Le immagini, se ci sono, restano quelle del secondo:
 * stanno solo sull'ultimo messaggio.
 */
function alternati(messaggi: readonly MessaggioIa[]): MessaggioIa[] {
  const fuori: MessaggioIa[] = [];
  for (const m of messaggi) {
    const prima = fuori[fuori.length - 1];
    if (prima === undefined || prima.ruolo !== m.ruolo) {
      fuori.push(m);
      continue;
    }
    const unito: MessaggioIa = { ruolo: m.ruolo, testo: `${prima.testo}

${m.testo}` };
    const immagini = m.immagini ?? prima.immagini;
    if (immagini !== undefined) unito.immagini = immagini;
    fuori[fuori.length - 1] = unito;
  }
  return fuori;
}

/**
 * Quanti gettoni peserebbe quel che si sta per mandare, a spanne.
 *
 * Quattro caratteri per gettone: grossolana, e dalla parte giusta della
 * grossolanità — sovrastima per l'italiano, che è quel che serve a un avviso.
 * Un'immagine si conta come il migliaio di gettoni che i modelli con la
 * visione di solito le attribuiscono.
 */
export function stimaGettoni(messaggi: readonly MessaggioIa[]): number {
  const caratteri = messaggi.reduce((somma, m) => somma + m.testo.length, 0);
  const immagini = messaggi.reduce((somma, m) => somma + (m.immagini?.length ?? 0), 0);
  return Math.round(caratteri / CARATTERI_PER_GETTONE) + immagini * 1000;
}
