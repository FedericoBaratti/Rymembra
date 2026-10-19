/**
 * L'editor di codice dello Studio: CodeMirror 6, istruito dal registro.
 *
 * # Perché un editor vero
 *
 * Lo Studio 2.x aveva una `<textarea>` con sotto un livello colorato allineato
 * a mano (`evidenzia.ts`): niente completamento, niente segni degli errori
 * sulla riga, e un annullo del campo che non sapeva niente della storia dello
 * Studio. Qui c'è CodeMirror, e quel che sa del formato viene tutto dal nucleo:
 *
 * - **Completamento guidato dal percorso.** L'albero sintattico dice dove si
 *   sta scrivendo — `parts.card.states.hover` — e il registro dice che cosa ci
 *   sta: le chiavi d'aspetto, i nomi degli stati, i token, le parti, gli
 *   inneschi, gli effetti coi loro parametri, le animazioni e le curve del
 *   documento, i file del pacchetto, le parole chiuse di ogni campo.
 * - **Gli errori sulla riga.** La riga e la colonna le calcola il nucleo
 *   (`rymembra_skin::posizioni`), e il segno cade sul valore sbagliato.
 * - **Le descrizioni al passaggio**, sulle parti e sui token.
 * - **Una storia sola.** CodeMirror non tiene la sua: Ctrl+Z e Ctrl+Y vanno
 *   allo Studio, così il codice e i controlli annullano la stessa cosa.
 *
 * I colori vengono dai token della skin indossata, come il resto dello Studio.
 */
import { autocompletion, closeBrackets, type Completion, type CompletionContext, type CompletionResult } from "@codemirror/autocomplete";
import { json } from "@codemirror/lang-json";
import { bracketMatching, foldGutter, HighlightStyle, indentOnInput, syntaxHighlighting, syntaxTree } from "@codemirror/language";
import { lintGutter, type Diagnostic } from "@codemirror/lint";
import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";
import { Annotation, EditorState, type Extension } from "@codemirror/state";
import {
  drawSelection,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  hoverTooltip,
  keymap,
  lineNumbers,
} from "@codemirror/view";
import { defaultKeymap } from "@codemirror/commands";
import { tags } from "@lezer/highlight";
import type { SyntaxNode } from "@lezer/common";

import type { Registro } from "../ipc";

/** Una modifica arrivata da fuori — un controllo, un annullo — e non da chi scrive. */
export const daFuori = Annotation.define<boolean>();

/** Quel che il completamento sa oltre al registro: il documento e il pacchetto. */
export interface Fonti {
  registro: Registro;
  /** L'ultimo documento che si è riusciti a leggere: mentre si scrive, spesso non è JSON. */
  documento: () => Record<string, unknown> | null;
  /** I file del pacchetto, per `src`. */
  file: () => readonly string[];
  /** Il percorso a cui corrisponde la radice del testo: `[]` per il documento, `["parts", "card"]` per un frammento. */
  radice: readonly string[];
  /** Le descrizioni, già nella lingua dell'interfaccia. */
  descrizioneParte: (nome: string, ripiego: string) => string;
  descrizioneToken: (id: string, ripiego: string) => string;
}

// ── dove si sta scrivendo ────────────────────────────────────────────────────

const PUNTEGGIATURA = new Set(["[", "]", "{", "}", ",", ":"]);

/** Il testo di una chiave, senza virgolette. */
function testoDi(state: EditorState, nodo: SyntaxNode): string {
  const scritto = state.sliceDoc(nodo.from, nodo.to);
  try {
    const letto: unknown = JSON.parse(scritto);
    return typeof letto === "string" ? letto : scritto;
  } catch {
    return scritto.replace(/^"|"$/g, "");
  }
}

/** Il posto di un figlio fra i valori di un elenco. */
function indiceIn(elenco: SyntaxNode, figlio: SyntaxNode): number {
  let i = 0;
  for (let x = elenco.firstChild; x !== null; x = x.nextSibling) {
    if (PUNTEGGIATURA.has(x.name)) continue;
    if (x.from === figlio.from) return i;
    i += 1;
  }
  return i;
}

/** Il percorso di un nodo nel documento: le chiavi e gli indici che ci portano. */
export function percorsoDi(state: EditorState, nodo: SyntaxNode): string[] {
  const passi: string[] = [];
  let figlio = nodo;
  for (let padre = nodo.parent; padre !== null; figlio = padre, padre = padre.parent) {
    if (padre.name === "Property" && figlio.name !== "PropertyName") {
      const nome = padre.getChild("PropertyName");
      if (nome !== null) passi.unshift(testoDi(state, nome));
    } else if (padre.name === "Array") {
      passi.unshift(String(indiceIn(padre, figlio)));
    }
  }
  return passi;
}

/** Un contenitore aperto prima del cursore. */
type Cornice =
  | {
      tipo: "oggetto";
      /** La chiave di cui si sta scrivendo il valore, o l'ultima letta. */
      chiave: string | null;
      attesa: "chiave" | "valore";
      /** Le chiavi già scritte in questo oggetto. */
      presenti: string[];
      /** I valori di testo già scritti: l'`effect` di un livello. */
      stringhe: Record<string, string>;
    }
  | { tipo: "elenco"; indice: number };

/**
 * Il testo prima del cursore, letto in avanti: i contenitori aperti, e se si è
 * dentro una stringa.
 *
 * Non l'albero sintattico: mentre si scrive il documento è quasi sempre a metà
 * — una virgola che manca, una stringa aperta — e il recupero degli errori
 * dell'analizzatore può buttare via proprio l'oggetto in cui si sta scrivendo.
 * Una lettura in avanti fino al cursore non ha bisogno che il resto sia
 * giusto: conta graffe, quadre, due punti e virgole, fuori dalle stringhe.
 */
function scandisci(testo: string): { pila: Cornice[]; stringa: number | null } {
  const pila: Cornice[] = [];
  let i = 0;
  while (i < testo.length) {
    const c = testo[i];
    if (c === '"') {
      let j = i + 1;
      let dentro = "";
      while (j < testo.length && testo[j] !== '"') {
        if (testo[j] === "\\") {
          dentro += testo[j + 1] ?? "";
          j += 2;
        } else {
          dentro += testo[j];
          j += 1;
        }
      }
      if (j >= testo.length) return { pila, stringa: i };
      const cima = pila[pila.length - 1];
      if (cima?.tipo === "oggetto") {
        if (cima.attesa === "chiave") {
          cima.chiave = dentro;
          cima.presenti.push(dentro);
        } else if (cima.chiave !== null) {
          cima.stringhe[cima.chiave] = dentro;
        }
      }
      i = j + 1;
      continue;
    }
    const cima = pila[pila.length - 1];
    if (c === "{") pila.push({ tipo: "oggetto", chiave: null, attesa: "chiave", presenti: [], stringhe: {} });
    else if (c === "[") pila.push({ tipo: "elenco", indice: 0 });
    else if (c === "}" || c === "]") pila.pop();
    else if (c === ":" && cima?.tipo === "oggetto") cima.attesa = "valore";
    else if (c === ",") {
      if (cima?.tipo === "oggetto") {
        cima.attesa = "chiave";
        cima.chiave = null;
      } else if (cima?.tipo === "elenco") cima.indice += 1;
    }
    i += 1;
  }
  return { pila, stringa: null };
}

/** Il percorso di una pila di contenitori, fino a quello in cima escluso. */
function percorsoDiPila(pila: readonly Cornice[]): string[] {
  const passi: string[] = [];
  for (const cornice of pila.slice(0, -1)) {
    if (cornice.tipo === "oggetto") {
      if (cornice.chiave !== null) passi.push(cornice.chiave);
    } else passi.push(String(cornice.indice));
  }
  return passi;
}

/** Dove sta il cursore, per il completamento. */
export interface Posto {
  ruolo: "chiave" | "valore";
  /** Il percorso dell'oggetto (per una chiave) o del valore (per un valore). */
  percorso: string[];
  /** La chiave di cui si scrive il valore. */
  chiave: string | null;
  /** Le chiavi già scritte nell'oggetto, prima del cursore. */
  presenti: string[];
  /** I valori di testo già scritti nell'oggetto: l'`effect` di un livello. */
  stringhe: Record<string, string>;
  /** Da dove si sostituisce. */
  da: number;
}

export function postoA(state: EditorState, pos: number): Posto | null {
  const { pila, stringa } = scandisci(state.sliceDoc(0, pos));
  const cima = pila[pila.length - 1];
  if (cima === undefined) return null;
  const sopra = percorsoDiPila(pila);

  if (cima.tipo === "elenco") {
    if (stringa === null) return null;
    // Una voce di un elenco: la chiave è quella sotto cui l'elenco sta.
    const padre = pila[pila.length - 2];
    return {
      ruolo: "valore",
      percorso: [...sopra, String(cima.indice)],
      chiave: padre?.tipo === "oggetto" ? padre.chiave : null,
      presenti: [],
      stringhe: {},
      da: stringa,
    };
  }
  if (stringa !== null) {
    if (cima.attesa === "chiave") {
      // La chiave che si sta scrivendo non conta fra quelle già scritte.
      return { ruolo: "chiave", percorso: sopra, chiave: null, presenti: cima.presenti, stringhe: cima.stringhe, da: stringa };
    }
    return {
      ruolo: "valore",
      percorso: cima.chiave === null ? sopra : [...sopra, cima.chiave],
      chiave: cima.chiave,
      presenti: cima.presenti,
      stringhe: cima.stringhe,
      da: stringa,
    };
  }
  // Fuori da una stringa: solo il completamento a richiesta, dove va una chiave.
  if (cima.attesa === "chiave" && /[{,]\s*$/.test(state.sliceDoc(Math.max(0, pos - 200), pos))) {
    return { ruolo: "chiave", percorso: sopra, chiave: null, presenti: cima.presenti, stringhe: cima.stringhe, da: pos };
  }
  return null;
}

// ── che cosa ci sta ──────────────────────────────────────────────────────────

const chiaviDi = (v: unknown): string[] =>
  v !== null && typeof v === "object" && !Array.isArray(v) ? Object.keys(v) : [];

/** Una sezione del documento, nella base o nella variante. */
function nomiIn(documento: Record<string, unknown> | null, sezione: string): string[] {
  const base = chiaviDi(documento?.[sezione]);
  const variante = chiaviDi((documento?.["variant"] as Record<string, unknown> | undefined)?.[sezione]);
  return [...new Set([...base, ...variante])];
}

/** Le chiavi di un effetto: `effect`, i suoi parametri, o un motivo richiamato. */
function chiaviEffetto(fonti: Fonti, effetto: string | null): string[] {
  if (effetto === null) return ["effect", "$pattern"];
  const def = fonti.registro.effects.find((e) => e.name === effetto);
  return ["effect", ...(def?.params.map((p) => p.name) ?? [])];
}

/** Le chiavi che ci stanno in un oggetto a questo percorso. */
export function chiaviPer(percorso: readonly string[], fonti: Fonti, effetto: string | null): string[] {
  const k = fonti.registro.chiavi;
  const aspetto = k["aspetto"] ?? [];
  const p = percorso[0] === "variant" ? percorso.slice(1) : [...percorso];
  if (percorso.length === 0) return k["documento"] ?? [];
  if (percorso[0] === "variant" && percorso.length === 1) return k["variante"] ?? [];
  const [sezione, a, b, c] = p;
  switch (sezione) {
    case "meta":
      return p.length === 1 ? (k["meta"] ?? []) : [];
    case "capabilities":
      return k["capacita"] ?? [];
    case "fonts":
      return p.length === 2 ? (k["font"] ?? []) : [];
    case "patterns":
      return p.length === 2 ? chiaviEffetto(fonti, effetto) : p.includes("stops") ? (k["fermata"] ?? []) : [];
    case "tokens": {
      if (p.length === 1) return fonti.registro.tokens.map((t) => t.id);
      const def = fonti.registro.tokens.find((t) => t.id === a);
      if (p.length === 2 && def?.kind === "shadow") return ["layers"];
      if (p.length === 2 && def?.kind === "length") return ["min", "preferred", "max"];
      if (b === "layers") return k["livelloOmbra"] ?? [];
      return [];
    }
    case "motion": {
      if (p.length === 1) return k["movimento"] ?? [];
      if (a === "route") return p.length === 2 ? (k["rotta"] ?? []) : (k["fotogrammaRotta"] ?? []);
      if (a === "animations" && p.length === 3) return k["animazione"] ?? [];
      if (a === "animations" && c === "frames") return p.length === 5 ? (k["fotogramma"] ?? []) : [];
      if (a === "easings" && p.length === 3) return ["kind", "keyword", "points", "count", "position"];
      return [];
    }
    case "layout": {
      if (p.length === 1) return k["impaginazione"] ?? [];
      const ultimo = p[p.length - 1];
      if (ultimo === "options") {
        return [];
      }
      return [...new Set([...(k["zona"] ?? []), ...(k["widget"] ?? []), ...(k["richiamo"] ?? [])])];
    }
    case "parts": {
      if (p.length === 1) return fonti.registro.parts.map((x) => x.name);
      if (p.length === 2) return [...aspetto, ...(k["strutturaParte"] ?? [])];
      const resto = p.slice(2);
      if (resto[0] === "states") return resto.length === 1 ? fonti.registro.stati.map((s) => s.name) : chiaviAspetto(resto.slice(2), fonti, effetto);
      if (resto[0] === "motion") {
        if (resto.length === 1) return [...fonti.registro.inneschi.map((i) => i.name), ...(k["movimentoParte"] ?? [])];
        if (resto[1] === "transition") return k["transizione"] ?? [];
        return [];
      }
      if (resto[0] === "before" || resto[0] === "after") {
        return resto.length === 1 ? (k["livello"] ?? []) : chiaviEffetto(fonti, effetto);
      }
      return chiaviAspetto(resto, fonti, effetto);
    }
    default:
      return [];
  }
}

/** Le chiavi dentro un campo d'aspetto: il raggio, l'ombra, l'anello, un livello. */
function chiaviAspetto(resto: readonly string[], fonti: Fonti, effetto: string | null): string[] {
  const k = fonti.registro.chiavi;
  const [campo, , terzo] = resto;
  if (resto.length === 0) return k["aspetto"] ?? [];
  switch (campo) {
    case "radius":
      return k["angoli"] ?? [];
    case "focusRing":
      return k["anello"] ?? [];
    case "shadow":
      return resto.length === 1 ? ["layers", "$token"] : terzo !== undefined ? (k["livelloOmbra"] ?? []) : [];
    case "background":
      return resto.length === 2 ? chiaviEffetto(fonti, effetto) : resto.includes("stops") ? (k["fermata"] ?? []) : [];
    case "clip":
      return chiaviEffetto(fonti, effetto ?? "chamfer");
    case "backdrop":
      return chiaviEffetto(fonti, effetto ?? "blurBehind");
    case "filter":
      return resto.length === 2 ? ["blur", "brightness", "contrast", "saturate", "grayscale", "hueRotate", "dropShadow"] : ["x", "y", "blur", "color"];
    default:
      return [];
  }
}

/** I valori che ci stanno per una chiave, a un percorso. */
export function valoriPer(percorso: readonly string[], chiave: string | null, fonti: Fonti): string[] {
  const documento = fonti.documento();
  const r = fonti.registro;
  if (chiave === null) return [];
  if (chiave === "$token") {
    const ombra = percorso.includes("shadow");
    return r.tokens.filter((t) => (ombra ? t.kind === "shadow" : t.kind === "color")).map((t) => t.id);
  }
  if (chiave === "$palette") return nomiIn(documento, "palette");
  if (chiave === "$pattern") return nomiIn(documento, "patterns");
  if (chiave === "$source") return r.sorgenti;
  if (chiave === "effect") {
    if (percorso.includes("clip")) return ["chamfer"];
    if (percorso.includes("backdrop")) return ["blurBehind"];
    return r.effects.filter((e) => e.target === "background").map((e) => e.name);
  }
  if (chiave === "widget") return r.widgets.map((w) => w.name);
  if (chiave === "part") return r.parts.map((p) => p.name);
  if (chiave === "prefab") return chiaviDi((documento?.["layout"] as Record<string, unknown> | undefined)?.["prefabs"]);
  if (chiave === "src") return [...fonti.file()];
  const p = percorso[0] === "variant" ? percorso.slice(1) : percorso;
  if (p[0] === "parts" && p[2] === "motion" && p.length === 4 && chiave !== "stagger" && chiave !== "transition") {
    return chiaviDi((documento?.["motion"] as Record<string, unknown> | undefined)?.["animations"]);
  }
  if (chiave === "easing") {
    return [...(r.parole["easing"] ?? []), ...chiaviDi((documento?.["motion"] as Record<string, unknown> | undefined)?.["easings"])];
  }
  if (chiave === "keyword") return r.parole["easing"] ?? [];
  if (chiave === "kind" && p[0] === "motion") return ["keyword", "cubicBezier", "steps"];
  return r.parole[chiave] ?? [];
}

// ── le estensioni ────────────────────────────────────────────────────────────

export function completamento(fonti: () => Fonti) {
  return (contesto: CompletionContext): CompletionResult | null => {
    const posto = postoA(contesto.state, contesto.pos);
    if (posto === null) return null;
    const f = fonti();
    const percorso = [...f.radice, ...posto.percorso];
    const effetto = posto.stringhe["effect"] ?? null;
    const voci =
      posto.ruolo === "chiave"
        ? chiaviPer(percorso, f, effetto)
        : valoriPer(percorso, posto.chiave, f);
    if (voci.length === 0) return null;
    const giaScritte = new Set(posto.ruolo === "chiave" ? posto.presenti : []);
    const dentro = contesto.state.sliceDoc(posto.da, posto.da + 1) === '"';
    const opzioni: Completion[] = voci
      .filter((v) => !giaScritte.has(v))
      .map((v) => {
        const parte = percorso.length === 1 && percorso[0] === "parts" ? f.registro.parts.find((x) => x.name === v) : undefined;
        const token = percorso.length === 1 && percorso[0] === "tokens" ? f.registro.tokens.find((x) => x.id === v) : undefined;
        const info = parte
          ? f.descrizioneParte(parte.name, parte.description)
          : token
            ? f.descrizioneToken(token.id, token.description)
            : undefined;
        return {
          label: v,
          apply: dentro ? applica(v, posto.ruolo) : posto.ruolo === "chiave" ? `"${v}": ` : `"${v}"`,
          type: posto.ruolo === "chiave" ? "property" : "enum",
          ...(info === undefined ? {} : { info }),
        };
      });
    // Dentro una stringa si completa dopo la virgoletta: il filtro di
    // CodeMirror confronta le voci con quel che si è battuto, e una virgoletta
    // in testa non la contiene nessun nome — l'elenco restava vuoto.
    return {
      from: dentro ? posto.da + 1 : posto.da,
      to: contesto.pos,
      options: opzioni,
      validFor: /^[\w$.\- ]*$/,
    };
  };
}

/**
 * Scrive la voce scelta dentro la stringa, e la chiude.
 *
 * La virgoletta di chiusura può esserci già — `closeBrackets` la mette insieme
 * a quella d'apertura — o no: si sostituisce fino a lei, compresa. Una chiave
 * prende i suoi due punti, a meno che non li abbia già: la si stava
 * correggendo.
 */
function applica(etichetta: string, ruolo: "chiave" | "valore") {
  return (view: EditorView, _scelta: Completion, from: number, to: number) => {
    let fine = to;
    while (fine < view.state.doc.length && /[\w$.\- ]/.test(view.state.sliceDoc(fine, fine + 1))) fine += 1;
    if (view.state.sliceDoc(fine, fine + 1) === '"') fine += 1;
    const conDuePunti = view.state.sliceDoc(fine, fine + 4).trimStart().startsWith(":");
    const inserto = `${etichetta}"${ruolo === "chiave" && !conDuePunti ? ": " : ""}`;
    view.dispatch({
      changes: { from, to: fine, insert: inserto },
      selection: { anchor: from + inserto.length },
      userEvent: "input.complete",
    });
  };
}

function descrizioni(fonti: () => Fonti) {
  return hoverTooltip((view, pos) => {
    const nodo = syntaxTree(view.state).resolveInner(pos, 1);
    if (nodo.name !== "PropertyName" && nodo.name !== "String") return null;
    const f = fonti();
    const testo = testoDi(view.state, nodo);
    const percorso = [...f.radice, ...percorsoDi(view.state, nodo)];
    let spiegazione: string | null = null;
    if (nodo.name === "PropertyName") {
      const dove = percorso.length === 1 ? percorso[0] : percorso.length === 2 && percorso[0] === "variant" ? percorso[1] : null;
      if (dove === "parts") {
        const parte = f.registro.parts.find((p) => p.name === testo);
        if (parte) spiegazione = f.descrizioneParte(parte.name, parte.description);
      } else if (dove === "tokens") {
        const token = f.registro.tokens.find((t) => t.id === testo);
        if (token) spiegazione = `${token.css} — ${f.descrizioneToken(token.id, token.description)}`;
      }
    } else if (percorso[percorso.length - 1] === "$token") {
      const token = f.registro.tokens.find((t) => t.id === testo);
      if (token) spiegazione = `${token.css} — ${f.descrizioneToken(token.id, token.description)}`;
    }
    if (spiegazione === null) return null;
    const testoSpiegazione = spiegazione;
    return {
      pos: nodo.from,
      end: nodo.to,
      above: true,
      create: () => {
        const dom = document.createElement("div");
        dom.classList.add("cm-descrizione");
        dom.textContent = testoSpiegazione;
        return { dom };
      },
    };
  });
}

const colori = HighlightStyle.define([
  { tag: tags.propertyName, color: "var(--accent)" },
  { tag: tags.string, color: "var(--color-text-1)" },
  { tag: tags.number, color: "var(--success)" },
  { tag: [tags.bool, tags.null], color: "var(--warning)" },
  { tag: [tags.separator, tags.punctuation, tags.brace, tags.squareBracket], color: "var(--color-text-3)" },
]);

const aspetto = EditorView.theme({
  "&": {
    flex: "1",
    minHeight: "0",
    height: "100%",
    color: "var(--color-text-1)",
    backgroundColor: "var(--color-surface-2)",
    borderRadius: "8px",
    border: "1px solid var(--hairline)",
    fontSize: "12px",
  },
  "&.cm-focused": { outline: "2px solid var(--accent)", outlineOffset: "-1px" },
  ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "1.55" },
  ".cm-content": { caretColor: "var(--accent)" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--accent)" },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, ::selection":
    { backgroundColor: "var(--accent-soft)" },
  ".cm-gutters": {
    backgroundColor: "var(--color-surface-1)",
    color: "var(--color-text-3)",
    border: "none",
    borderRight: "1px solid var(--hairline)",
  },
  ".cm-activeLine": { backgroundColor: "color-mix(in srgb, var(--color-text-1) 4%, transparent)" },
  ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--color-text-1)" },
  ".cm-tooltip": {
    border: "1px solid var(--hairline)",
    backgroundColor: "var(--color-surface-2)",
    color: "var(--color-text-1)",
    borderRadius: "8px",
  },
  ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
    backgroundColor: "var(--accent-soft)",
    color: "var(--accent)",
  },
  ".cm-descrizione, .cm-completionInfo": { maxWidth: "320px", padding: "6px 8px", fontSize: "11.5px", lineHeight: "1.45" },
  ".cm-panels": { backgroundColor: "var(--color-surface-1)", color: "var(--color-text-1)" },
  ".cm-lintRange-error": { backgroundImage: "none", textDecoration: "underline wavy var(--danger)" },
  ".cm-lintRange-warning": { backgroundImage: "none", textDecoration: "underline wavy var(--warning)" },
});

/** Tutto quel che serve a un editor dello Studio. */
export function estensioni(opzioni: {
  fonti: () => Fonti;
  onAnnulla: () => void;
  onRipeti: () => void;
  etichetta: string;
}): Extension[] {
  const storia = keymap.of([
    { key: "Mod-z", run: () => (opzioni.onAnnulla(), true), preventDefault: true },
    { key: "Mod-y", run: () => (opzioni.onRipeti(), true), preventDefault: true },
    { key: "Mod-Shift-z", run: () => (opzioni.onRipeti(), true), preventDefault: true },
  ]);
  return [
    storia,
    lineNumbers(),
    foldGutter(),
    lintGutter(),
    highlightActiveLineGutter(),
    highlightActiveLine(),
    drawSelection(),
    indentOnInput(),
    bracketMatching(),
    closeBrackets(),
    highlightSelectionMatches(),
    search({ top: true }),
    keymap.of([...searchKeymap, ...defaultKeymap]),
    json(),
    syntaxHighlighting(colori),
    aspetto,
    autocompletion({ override: [completamento(opzioni.fonti)], icons: false }),
    descrizioni(opzioni.fonti),
    EditorView.contentAttributes.of({ "aria-label": opzioni.etichetta, spellcheck: "false" }),
    EditorState.tabSize.of(2),
  ];
}

/** Un problema del nucleo, come segno sul testo. */
export interface Segno {
  riga: number;
  colonna: number;
  messaggio: string;
  gravita: "error" | "warning";
}

/** I segni in diagnostiche di CodeMirror: dal punto indicato alla fine del valore. */
export function diagnostiche(state: EditorState, segni: readonly Segno[]): Diagnostic[] {
  const fuori: Diagnostic[] = [];
  for (const s of segni) {
    if (s.riga < 1 || s.riga > state.doc.lines) continue;
    const riga = state.doc.line(s.riga);
    const da = Math.min(riga.to, riga.from + Math.max(0, s.colonna - 1));
    const nodo = syntaxTree(state).resolveInner(da, 1);
    const a = nodo.to > da && nodo.to <= riga.to ? nodo.to : riga.to;
    fuori.push({ from: da, to: Math.max(a, Math.min(riga.to, da + 1)), severity: s.gravita, message: s.messaggio });
  }
  return fuori;
}
