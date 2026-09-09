/**
 * Le istruzioni che il modello riceve: il vocabolario chiuso, e come si scrive
 * una modifica.
 *
 * # Perché si costruiscono qui e non nel nucleo
 *
 * Perché qui il registro e il documento sono già in memoria — lo Studio li ha
 * per disegnare — e perché il pannello deve poter dire **quanto** sta mandando.
 * Costruirle in Rust vorrebbe dire una seconda lettura del registro, una
 * seconda idea di cosa mettere dentro, e nessun modo di mostrare la misura
 * accanto al campo in cui si scrive.
 *
 * `aether-ia` resta ignorante di cosa sia una skin: queste righe arrivano al
 * client come un messaggio come gli altri.
 *
 * # Cosa ci va, e cosa no
 *
 * Ci vanno i **nomi**: i token con il loro tipo, le parti, gli effetti con il
 * loro esemplare valido, le parole ammesse per zone, spazi e allineamenti. Un
 * modello che lavora su un vocabolario chiuso ha bisogno di sapere quali nomi
 * esistono, non di leggerne la descrizione.
 *
 * **Non** ci vanno le descrizioni del registro. Sono la metà pesante — una
 * frase per ognuna delle centoventi voci — e sono scritte per una persona che
 * legge l'albero. Senza, l'elenco compatto sta in cinque o sei kilobyte; con il
 * documento e le regole si resta sotto i venticinque, cioè sotto il contesto
 * degli otto-kilo-gettoni che è il tetto dei modelli locali piccoli. È la
 * differenza fra una chat che funziona su Ollama e una che risponde monca senza
 * dire perché.
 *
 * # Perché l'elenco degli elenchi sta scritto a mano in `REGOLE`
 *
 * Perché il registro non lo sa. `studio_registro` dice quali token esistono e
 * di che tipo sono, quali parti si possono ridipingere, quali effetti si
 * possono impilare — non dice che `background` è una pila e `radius` no. Quella
 * è una proprietà del **formato**, e vive in `document.rs`.
 *
 * Dedurla dal documento aperto sembrerebbe più onesto e sarebbe peggio: una
 * skin che non dichiara nessuno sfondo non insegnerebbe al modello che gli
 * sfondi sono pile, e il primo che ne aggiunge uno lo scriverebbe come oggetto.
 * Un elenco fisso di otto righe si sbaglia solo se cambia il formato — e se
 * cambia il formato cambia `SKIN_FORMAT_VERSION`, che è un cambio maggiore e si
 * vede.
 *
 * # Il conto dei caratteri, e perché si mostra
 *
 * Perché è l'unico modo in cui chi sceglie un modello con quattromila gettoni
 * di contesto capisce da dove viene una risposta tagliata a metà, invece di
 * dare la colpa alla chat. La stima è quattro caratteri per gettone: è
 * grossolana e sta dalla parte giusta della grossolanità — sovrastima per
 * l'italiano, che è quel che serve a un avviso.
 */
import type { MessaggioIa, Problema, Registro, Validazione } from "../ipc";
import { t } from "../lingue";

/** Quanti caratteri vale un gettone, per la stima che si mostra. */
const CARATTERI_PER_GETTONE = 4;

/** Quanti errori di validazione si rimandano indietro in un giro. */
const ERRORI_DA_MOSTRARE = 12;

/**
 * Le regole del formato: la parte delle istruzioni che non dipende dal
 * registro.
 *
 * In inglese, e non è una svista né un cedimento. È l'unico testo
 * dell'applicazione scritto per essere letto da un modello, e i modelli piccoli
 * seguono le istruzioni in inglese molto meglio di quanto le seguano in
 * italiano — mentre la **conversazione** resta nella lingua di chi scrive,
 * perché quella la legge una persona. Non passa dal catalogo delle lingue per
 * la stessa ragione: tradurlo lo peggiorerebbe.
 */
const REGOLE = `You edit an Aether skin manifest: a JSON document.

To change it, emit ONE fenced block with the language tag \`aether-patch\`,
containing a JSON array of operations. Nothing else in the block.

\`\`\`aether-patch
[{"op": "scrivi", "percorso": ["tokens", "color.surface.0"], "valore": "#050508"},
 {"op": "scrivi", "percorso": ["parts", "section-card", "background", "0", "stops", "1", "color"], "valore": "#1a1512"},
 {"op": "togli", "percorso": ["parts", "section-card", "radius"]}]
\`\`\`

- \`op\` is "scrivi" (set a value, creating what is missing) or "togli" (remove).
- \`percorso\` is an ARRAY of steps, one per level from the document root — never
  a dotted string. A step is an object key, or a position counted from 0.
    right: ["parts", "section-card", "background", "0", "stops", "0", "color"]
    wrong: "parts.section-card.background[0].stops[0].color"
- \`valore\` is required for "scrivi" and must be absent for "togli".

Lists, and how to address them:
- \`parts.<name>.background\` is a STACK of effect layers, bottom first. So
  \`["parts","section-card","background","0","stops","1","color"]\` is the second
  colour stop of the bottom layer.
- Also lists: \`meta.preview\`, \`tokens.font.*\`, \`tokens.shadow.*.layers\`,
  \`tokens.motion.ease.*.points\`, \`patterns.*.at\`, \`patterns.*.stops\`, the
  \`at\` and \`stops\` of any gradient, \`layout.shell.children\`.
- There is no "add" operation. To ADD a layer, "scrivi" at the position equal to
  the list's current length — count it in the document below. If that stack has
  two layers, the third one is written like this:
    {"op": "scrivi", "percorso": ["parts", "section-card", "background", "2"],
     "valore": {"effect": "vignette", "color": "#000"}}
  A position further out is refused, because it would leave a hole.
- To REMOVE one, "togli" that position: the ones after it shift down.
- To REORDER, or to insert somewhere other than the end, write the whole list in
  a single "scrivi" on the list itself.
- Otherwise change one layer at a time by its position. Rewriting a whole stack
  to change one colour is how you lose the other layers.

Value syntax inside the document:
- colors: \`"#rrggbb"\` or \`"#rrggbbaa"\`
- a reference to a token: \`{"$token": "color.accent"}\`
- a palette entry with opacity: \`{"$palette": "viola", "alpha": 0.4}\`
- lengths and durations are strings with a unit: \`"12px"\`, \`"180ms"\`

Rules:
- Use ONLY names from the vocabulary below. Nothing else exists; a name that is
  not listed is an error, not an extension.
- Change only what was asked. Do not restate unchanged values.
- Write a short sentence explaining what you changed, then the block.
- If the request cannot be done with this vocabulary, say so and emit no block.`;

/**
 * Il contratto d'uscita, ripetuto **dopo** il documento.
 *
 * # Perché è una ripetizione, e perché va tenuta
 *
 * Perché le stesse frasi in cima non bastavano, e non è un'ipotesi: con un
 * modello locale da ventisette miliardi di parametri, le istruzioni scritte
 * solo in testa producevano un blocco ```json con dentro il pezzo di documento
 * riscritto — cioè zero operazioni, un pannello che non ha niente da mostrare
 * e nessuna spiegazione del perché. Con queste otto righe in fondo, lo stesso
 * modello e la stessa domanda producono un blocco `aether-patch` con dentro
 * un'operazione.
 *
 * La ragione è la distanza. Fra le regole e la risposta ci sono il vocabolario
 * e il documento intero — venti kilobyte in cui l'ultima cosa che il modello
 * legge è un manifest JSON, e imitare l'ultima forma vista è quel che un
 * modello fa quando le istruzioni sono lontane. Qui l'ultima forma vista è il
 * contratto.
 *
 * Va **dopo gli errori** e non prima: in modalità agent quello è l'elenco che
 * cresce, e finirebbe lui a fare da ultima cosa letta.
 */
const CHIUSURA = `REMEMBER — the answer has exactly two parts, in this order:
1. one short sentence saying what you changed;
2. ONE fenced block tagged \`aether-patch\` holding the JSON array of operations.

Never a \`json\` block, and never the rewritten document: only the operations.
\`percorso\` is an array of steps — ["parts", "section-card", "background", "0"] —
never a dotted string. If nothing needs changing, say so and emit no block.`;

/** Le istruzioni complete, per questo registro e questo documento. */
export function istruzioni(
  registro: Registro,
  documento: string,
  esito: Validazione | null,
): string {
  const pezzi = [REGOLE, vocabolario(registro), `Current document:\n${documento}`];
  const aperti = errori(esito?.errori ?? []);
  if (aperti !== null) pezzi.push(aperti);
  pezzi.push(CHIUSURA);
  return pezzi.join("\n\n");
}

/**
 * Il vocabolario chiuso, compatto.
 *
 * I token vanno raggruppati per tipo e non per gruppo: quel che il modello deve
 * sapere di `color.surface.0` è che accetta un colore, non che appartiene a
 * «Superfici» — il gruppo è per l'albero, che lo legge una persona.
 */
function vocabolario(registro: Registro): string {
  const perTipo = new Map<string, string[]>();
  for (const token of registro.tokens) {
    const elenco = perTipo.get(token.kind) ?? [];
    elenco.push(token.id);
    perTipo.set(token.kind, elenco);
  }
  const righe = [...perTipo.entries()].map(
    ([tipo, nomi]) => `  ${tipo}: ${nomi.join(" ")}`,
  );

  return [
    `VOCABULARY (format ${registro.format})`,
    "",
    "tokens, by value type:",
    ...righe,
    "",
    // Gli strati liberi si segnano: una parte che non li ha rifiuta un
    // `layers`, e senza questa distinzione il modello lo scriverebbe su tutte.
    `parts (a "+" means it accepts an extra free layer):`,
    `  ${registro.parts.map((p) => (p.layers ? `${p.name}+` : p.name)).join(" ")}`,
    "",
    // L'esemplare viene dal nucleo, che lo costruisce facendolo passare dal
    // parser vero: un esempio che non compila non esce, e un effetto che la
    // tabella non conosce sparisce dall'elenco invece di mentire.
    "effects (name, cost, and a minimal valid example):",
    ...registro.effects.map(
      (e) => `  ${e.name} (cost ${e.cost}, ${e.target}): ${e.esempio}`,
    ),
    "",
    `surface cost budget: ${registro.budget}; shell budget: ${registro.shellBudget}`,
    "",
    "shell widgets:",
    `  ${registro.widgets.map((w) => w.name).join(" ")}`,
    "",
    "words allowed in a shell node:",
    `  zone: ${registro.vocabolario.zones.join(" ")}`,
    `  gap: ${registro.vocabolario.gaps.join(" ")}`,
    `  align: ${registro.vocabolario.aligns.join(" ")}`,
    `  spread: ${registro.vocabolario.spreads.join(" ")}`,
  ].join("\n");
}

/**
 * Gli errori aperti, nelle stesse parole che il bottone «forse volevi dire»
 * già usa.
 *
 * `null` quando non ce ne sono: una riga «no errors» sarebbe contesto speso per
 * dire che non c'è niente da dire.
 */
export function errori(problemi: readonly Problema[]): string | null {
  if (problemi.length === 0) return null;
  const righe = problemi.slice(0, ERRORI_DA_MOSTRARE).map((p) => {
    const forse = p.forse.length > 0 ? ` (did you mean: ${p.forse.join(", ")})` : "";
    // La riga serve al modello quanto serve a chi legge: gli errori che riceve
    // sono quelli del **suo** documento, e senza un numero deve ritrovare il
    // punto dal percorso. Fino a poco fa qui arrivava comunque un problema
    // solo, con dentro venti frasi unite da punti e virgola.
    const dove = p.riga === null ? p.path : `${p.path} (line ${p.riga})`;
    return `  ${p.code} at ${dove}: ${p.message}${forse}`;
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
 * Porta gli errori del **candidato** e le ragioni di quel che si è scartato:
 * sono le due cose che il modello può usare per correggersi, e sono già scritte
 * nelle sue parole.
 *
 * `applicato` distingue i due giri, e non è un dettaglio: quando il documento
 * non è cambiato affatto — tutte le operazioni rifiutate — «l'ho applicata e non
 * è venuta pulita» è una frase falsa, e manda il modello a cercare cosa ha
 * rotto invece che a riscrivere quel che non è nemmeno partito.
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
  // La chiusa cambia con `applicato`, e cambiarla non è cortesia. «Do not
  // repeat what already works» dopo un giro in cui **non** ha funzionato niente
  // dice al modello che c'è qualcosa da conservare, e la conclusione che ne
  // trae è che non resta niente da scrivere: visto davvero, un secondo giro che
  // rispondeva a parole e senza blocco. Quando il documento è intatto, quel che
  // serve è il contrario — rifallo tutto, nella forma giusta.
  pezzi.push(
    applicato
      ? "Emit a new block that fixes only these. Do not repeat what already works."
      : "Emit the same change again as one `aether-patch` block, in the correct form.",
  );
  return pezzi.join("\n\n");
}

/** Il messaggio di sistema, pronto da mettere in testa alla conversazione. */
export function sistema(
  registro: Registro,
  documento: string,
  esito: Validazione | null,
): MessaggioIa {
  return { ruolo: "system", testo: istruzioni(registro, documento, esito) };
}

/**
 * Quanto pesa quel che si sta per mandare, detto a chi guarda.
 *
 * La stima dei gettoni è deliberatamente grossolana — vedi il preambolo — e la
 * frase la nomina come stima.
 */
export function misura(messaggi: readonly MessaggioIa[]): string {
  const caratteri = messaggi.reduce((somma, m) => somma + m.testo.length, 0);
  return t("studio.chat.size", {
    k: Math.round(caratteri / 100) / 10,
    gettoni: Math.round(caratteri / CARATTERI_PER_GETTONE / 100) * 100,
  });
}
