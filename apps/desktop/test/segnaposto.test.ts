import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

/*
 * Ogni chiamata a un testo riempie i suoi segnaposto (R17-4, R17-24 controllo 2).
 *
 * `t()` controlla la **chiave** (il tipo `Chiave` viene da `it.json`), non i valori
 * che la frase vuole: `t("row.more")` senza il titolo disegnava «Altre azioni per
 * «{titolo}»», e TalkBack leggeva «graffa titolo graffa». `riempi` lascia il
 * segnaposto com'è quando manca il valore — è visibile, e giusto per un difetto di
 * traduzione —, ma il difetto era nel punto di chiamata, e nessun controllo lo
 * guardava.
 *
 * Si legge il TypeScript con il compilatore, non con espressioni regolari: una
 * prova fatta così sbagliava sui commenti dentro gli oggetti e sui valori che
 * arrivano da una variabile. Le chiamate che non si possono leggere (i valori
 * vengono da `...altro`, da una variabile, da una funzione) non si giudicano:
 * niente falsi allarmi.
 */
const CARTELLA = fileURLToPath(new URL("../src", import.meta.url));
const lingua = (nome: string) =>
  JSON.parse(readFileSync(new URL(`../src/lingue/${nome}.json`, import.meta.url), "utf8")) as Record<string, string>;
const IT = lingua("it");
const EN = lingua("en");

/** Le forme di una chiave: il plurale e la variante del telefono, in qualunque combinazione. */
const FORME = new Set(["uno", "one", "other", "few", "many", "zero", "two", "telefono"]);

/** I segnaposto `{nome}` di una chiave, di tutte le sue forme, in tutte e due le lingue. */
function segnapostiDi(chiave: string, dizionari: Record<string, string>[]): Set<string> {
  const trovati = new Set<string>();
  for (const d of dizionari) {
    for (const [k, v] of Object.entries(d)) {
      if (k !== chiave) {
        if (!k.startsWith(`${chiave}.`)) continue;
        if (!k.slice(chiave.length + 1).split(".").every((pezzo) => FORME.has(pezzo))) continue;
      }
      for (const m of v.matchAll(/\{(\w+)\}/g)) trovati.add(m[1] ?? "");
    }
  }
  return trovati;
}

/** I nomi di un oggetto letterale, o `null` se non si sanno tutti (spread, nome calcolato, non un oggetto). */
function nomiDi(espressione: ts.Expression | undefined): Set<string> | null {
  if (espressione === undefined) return new Set();
  let nodo = espressione;
  while (ts.isParenthesizedExpression(nodo) || ts.isAsExpression(nodo) || ts.isSatisfiesExpression(nodo)) {
    nodo = nodo.expression;
  }
  if (!ts.isObjectLiteralExpression(nodo)) return null;
  const nomi = new Set<string>();
  for (const proprieta of nodo.properties) {
    if (ts.isShorthandPropertyAssignment(proprieta)) nomi.add(proprieta.name.text);
    else if (ts.isPropertyAssignment(proprieta) || ts.isMethodDeclaration(proprieta)) {
      const nome = proprieta.name;
      if (ts.isIdentifier(nome) || ts.isStringLiteral(nome) || ts.isNumericLiteral(nome)) nomi.add(nome.text);
      else return null;
    } else return null;
  }
  return nomi;
}

/** La chiave di una chiamata, se è scritta per esteso. */
function chiaveLetterale(nodo: ts.Expression | undefined): string | null {
  if (nodo === undefined) return null;
  let n = nodo;
  while (ts.isParenthesizedExpression(n) || ts.isAsExpression(n)) n = n.expression;
  return ts.isStringLiteral(n) || ts.isNoSubstitutionTemplateLiteral(n) ? n.text : null;
}

/**
 * Le chiamate di un sorgente a cui mancano dei segnaposto: `[riga, chiave, mancanti]`.
 *
 * Cerca `t("k", {…})`, `tSe("k", ripiego, {…})` e `<Trans k="k" v={{…}} n={{…}} />`.
 */
export function chiamateSenzaSegnaposti(
  nomeFile: string,
  testo: string,
  segnaposti: (chiave: string) => Set<string>,
): [number, string, string[]][] {
  const sorgente = ts.createSourceFile(
    nomeFile,
    testo,
    ts.ScriptTarget.Latest,
    true,
    nomeFile.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS,
  );
  const trovati: [number, string, string[]][] = [];
  const riga = (nodo: ts.Node) => sorgente.getLineAndCharacterOfPosition(nodo.getStart(sorgente)).line + 1;
  const controlla = (nodo: ts.Node, chiave: string | null, dati: Set<string> | null) => {
    if (chiave === null || dati === null) return;
    const attesi = segnaposti(chiave);
    const mancanti = [...attesi].filter((nome) => !dati.has(nome));
    if (mancanti.length > 0) trovati.push([riga(nodo), chiave, mancanti]);
  };
  const visita = (nodo: ts.Node): void => {
    if (ts.isCallExpression(nodo) && ts.isIdentifier(nodo.expression)) {
      const chiamata = nodo.expression.text;
      if (chiamata === "t") {
        controlla(nodo, chiaveLetterale(nodo.arguments[0]), nomiDi(nodo.arguments[1]));
      } else if (chiamata === "tSe") {
        controlla(nodo, chiaveLetterale(nodo.arguments[0]), nomiDi(nodo.arguments[2]));
      }
    }
    if (ts.isJsxSelfClosingElement(nodo) || ts.isJsxOpeningElement(nodo)) {
      if (ts.isIdentifier(nodo.tagName) && nodo.tagName.text === "Trans") {
        const attributo = (nome: string) =>
          nodo.attributes.properties.find(
            (a): a is ts.JsxAttribute => ts.isJsxAttribute(a) && a.name.getText(sorgente) === nome,
          );
        const espressione = (a: ts.JsxAttribute | undefined): ts.Expression | undefined => {
          const i = a?.initializer;
          return i !== undefined && ts.isJsxExpression(i) ? (i.expression ?? undefined) : undefined;
        };
        const k = attributo("k")?.initializer;
        const chiave = k === undefined ? null : ts.isStringLiteral(k) ? k.text : ts.isJsxExpression(k) ? chiaveLetterale(k.expression ?? undefined) : null;
        const v = nomiDi(espressione(attributo("v")));
        const n = nomiDi(espressione(attributo("n")));
        controlla(nodo, chiave, v === null || n === null ? null : new Set([...v, ...n]));
      }
    }
    ts.forEachChild(nodo, visita);
  };
  visita(sorgente);
  return trovati;
}

/** Tutti i `.ts` e `.tsx` sotto `src`, tranne il motore delle lingue. */
function sorgenti(cartella: string): string[] {
  const fuori: string[] = [];
  for (const voce of readdirSync(cartella, { withFileTypes: true })) {
    const percorso = join(cartella, voce.name);
    if (voce.isDirectory()) {
      if (voce.name !== "lingue") fuori.push(...sorgenti(percorso));
    } else if (/\.tsx?$/.test(voce.name)) fuori.push(percorso);
  }
  return fuori;
}

test("ogni chiamata riempie i segnaposto della sua frase", () => {
  const file = sorgenti(CARTELLA);
  assert.ok(file.length > 100, `pochi sorgenti trovati (${file.length})`);
  const segnaposti = (chiave: string) => segnapostiDi(chiave, [IT, EN]);
  let chiamate = 0;
  const trovati: string[] = [];
  for (const percorso of file) {
    const testo = readFileSync(percorso, "utf8");
    chiamate += (testo.match(/\b(t|tSe)\(|<Trans\b/g) ?? []).length;
    for (const [riga, chiave, mancanti] of chiamateSenzaSegnaposti(percorso, testo, segnaposti)) {
      trovati.push(`${relative(CARTELLA, percorso)}:${riga} «${chiave}» non riempie {${mancanti.join("}, {")}}`);
    }
  }
  assert.ok(chiamate > 500, `poche chiamate lette (${chiamate}): la prova non guarda niente`);
  assert.deepEqual(trovati, []);
});

test("la prova vede quel che deve: una chiamata senza i suoi valori", () => {
  const segnaposti = (chiave: string) =>
    new Set(chiave === "row.more" ? ["titolo"] : chiave === "tour.progress" ? ["n", "totale"] : []);
  const rosso = `
    const a = t("row.more");
    const b = t("row.more", { titolo: brano.title });
    const c = t("row.more", { ...altro });
    const d = tSe("row.more", "ripiego", { altro: 1 });
    const e = t("tour.progress", { n, totale });
    const f = t(chiaveCalcolata);
    const g = <Trans k="tour.progress" v={{ n: <b>1</b> }} />;
    const h = <Trans k="tour.progress" n={{ n: 1, totale: 2 }} />;
    const i = t("row.more", valori);
  `;
  const trovati = chiamateSenzaSegnaposti("prova.tsx", rosso, segnaposti);
  assert.deepEqual(
    trovati.map(([riga, chiave, mancanti]) => `${riga} ${chiave} ${mancanti.join(",")}`),
    ["2 row.more titolo", "5 row.more titolo", "8 tour.progress totale"],
  );
});
