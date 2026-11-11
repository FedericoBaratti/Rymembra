/**
 * I fogli del telefono non toccano il computer.
 *
 * `stile/telefono*.css` sono importati da `main.tsx` per tutte e due le
 * piattaforme — un foglio non si carica a metà — e la promessa che sul
 * computer non cambino un pixel sta tutta in un prefisso: ogni regola comincia
 * con `:root[data-forma="telefono"]`, che `main.tsx` mette solo sul telefono.
 * Una regola che lo dimentica non dà nessun errore: disegna sul computer, in
 * silenzio, fino a che qualcuno se ne accorge da una schermata. Qui lo si
 * controlla regola per regola, anche dentro `@media`, `@supports` e
 * `@starting-style`.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const CARTELLA = join(import.meta.dirname, "..", "src", "stile");
const PREFISSO = ':root[data-forma="telefono"]';

/** I selettori delle regole di un foglio, con le at-rule attraversate. */
function selettori(css: string): string[] {
  const pulito = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const trovati: string[] = [];
  // Ogni livello dice se dentro ci sono regole (`true`) o dichiarazioni.
  const pila: boolean[] = [true];
  let testa = "";
  for (const c of pulito) {
    if (c === "{") {
      const intestazione = testa.trim();
      testa = "";
      const dentroRegole = pila.at(-1) ?? true;
      if (!dentroRegole) {
        pila.push(false);
        continue;
      }
      if (intestazione.startsWith("@")) {
        // Dentro `@keyframes` ci sono fotogrammi, non selettori.
        pila.push(!/^@(-webkit-)?keyframes\b/.test(intestazione) ? true : false);
        continue;
      }
      trovati.push(intestazione);
      pila.push(false);
    } else if (c === "}") {
      pila.pop();
      testa = "";
    } else if (c === ";" && (pila.at(-1) ?? true)) {
      // Un `@import` o un `@layer x;` a livello di regole.
      testa = "";
    } else {
      testa += c;
    }
  }
  return trovati;
}

/**
 * I selettori di una lista, divisi alle virgole di primo livello: quelle dentro
 * `:not(a, b)` o `:is(a, b)` sono dello stesso selettore, e il pezzo dopo non
 * comincia col prefisso perché non è un selettore.
 */
function dividi(lista: string): string[] {
  const pezzi: string[] = [];
  let profondita = 0;
  let pezzo = "";
  for (const c of lista) {
    if (c === "(") profondita++;
    else if (c === ")") profondita--;
    if (c === "," && profondita === 0) {
      pezzi.push(pezzo);
      pezzo = "";
    } else {
      pezzo += c;
    }
  }
  pezzi.push(pezzo);
  return pezzi;
}

const fogli = readdirSync(CARTELLA).filter((f) => /^telefono.*\.css$/.test(f));

test("ci sono i fogli del telefono", () => {
  assert.ok(fogli.includes("telefono.css"));
  assert.ok(fogli.includes("telefono-pagine.css"));
});

for (const foglio of fogli) {
  test(`${foglio}: ogni regola vale solo sul telefono`, () => {
    const css = readFileSync(join(CARTELLA, foglio), "utf8");
    const scoperti = selettori(css)
      .flatMap(dividi)
      .map((s) => s.trim())
      .filter((s) => s.length > 0 && !s.startsWith(PREFISSO));
    assert.deepEqual(scoperti, [], `selettori senza ${PREFISSO}`);
  });
}

test("le virgole dentro :not() non dividono il selettore", () => {
  assert.deepEqual(dividi(`${PREFISSO} a:not(.x, .y), ${PREFISSO} b`), [
    `${PREFISSO} a:not(.x, .y)`,
    ` ${PREFISSO} b`,
  ]);
});

test("il controllo vede anche dentro le at-rule", () => {
  const css = `
    @media (hover: hover) { .x { color: red; } }
    @keyframes k { from { opacity: 0; } to { opacity: 1; } }
    ${PREFISSO} .y { color: blue; }
  `;
  assert.deepEqual(selettori(css), [".x", `${PREFISSO} .y`]);
});

/** Le regole di un foglio: i selettori (divisi) e le dichiarazioni, anche dentro le at-rule. */
function regoleDi(css: string): { selettori: string[]; dichiarazioni: string }[] {
  const pulito = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const fuori: { selettori: string[]; dichiarazioni: string }[] = [];
  const apertura = /([^{};]+)\{/g;
  let m: RegExpExecArray | null;
  while ((m = apertura.exec(pulito)) !== null) {
    const testa = (m[1] ?? "").trim();
    if (testa.startsWith("@")) continue;
    const fine = pulito.indexOf("}", apertura.lastIndex);
    fuori.push({
      selettori: dividi(testa).map((s) => s.replace(/\s+/g, " ").trim()),
      dichiarazioni: pulito.slice(apertura.lastIndex, fine),
    });
    apertura.lastIndex = fine + 1;
  }
  return fuori;
}

/** L'ultimo pezzo di un selettore, senza gli stati: quel che il selettore veste. */
function bersaglio(selettore: string): string {
  return (selettore.split(/\s*[\s>+~]\s*/).at(-1) ?? "").replace(
    /:(hover|focus-visible|focus-within|focus)\b/g,
    "",
  );
}

test("quel che sul computer compare al passaggio, sul telefono c'è sempre", () => {
  // Un dito non passa sopra niente: un comando a opacità zero che si accende al
  // passaggio, sul telefono, è una zona invisibile che prende i tocchi. Ogni
  // gemello `:hover` dei fogli del computer ha una regola del telefono che lo
  // rende visibile (`opacity: 1`) o lo toglie (`display: none`).
  const computer = [
    readFileSync(join(CARTELLA, "..", "stile.css"), "utf8"),
    ...readdirSync(CARTELLA)
      .filter((f) => f.endsWith(".css") && !/^telefono/.test(f))
      .map((f) => readFileSync(join(CARTELLA, f), "utf8")),
  ].flatMap(regoleDi);
  const telefono = fogli.flatMap((f) => regoleDi(readFileSync(join(CARTELLA, f), "utf8")));
  const ECCEZIONI = new Map([
    // Un suggerimento, non un comando: sul telefono compare al tocco, che dà
    // il fuoco alla linguetta spenta (`:focus-within`).
    [".tooltip-pill", "suggerimento"],
    // Sul telefono non c'è: la disinstallazione sta nel ⋮ della scheda (D-76),
    // vedi il controllo sul markup qui sotto.
    [".azioni-skin .via-skin", "nel ⋮"],
  ]);
  const scoperti: string[] = [];
  for (const nascosta of computer.filter((r) => /(^|[;\s])opacity:\s*0\s*;/.test(r.dichiarazioni))) {
    for (const selettore of nascosta.selettori) {
      const chi = bersaglio(selettore);
      const rivelata = computer.some(
        (r) =>
          /opacity:\s*1\b/.test(r.dichiarazioni) &&
          r.selettori.some((s) => s.includes(":hover") && bersaglio(s) === chi),
      );
      if (!rivelata || ECCEZIONI.has(selettore)) continue;
      const coperta = telefono.some(
        (r) =>
          /opacity:\s*1\b|display:\s*none/.test(r.dichiarazioni) &&
          r.selettori.some((s) => bersaglio(s) === chi),
      );
      if (!coperta) scoperti.push(selettore);
    }
  }
  assert.deepEqual(scoperti, []);

  // La X delle skin c'è solo sul computer, e sul telefono la scheda ha il suo ⋮
  // con «Disinstalla».
  const impostazioni = readFileSync(join(CARTELLA, "..", "schermate", "Impostazioni.tsx"), "utf8");
  assert.match(impostazioni, /\{!s\.diSerie && !MOBILE && \(\s*<button[^>]*?className="[^"]*\bvia-skin\b/);
  assert.match(impostazioni, /\{!s\.diSerie && MOBILE && \(\s*<button[^>]*?className="[^"]*\baltro-skin\b/);
  assert.match(impostazioni, /azione: \(\) => chiediDisinstalla\(s\)/);
});

test("i fogli del telefono non occupano i livelli che il registro promette alle skin", () => {
  // Una parte «con livelli» (`.livelli()` in `rymembra-skin/src/parts.rs`)
  // lascia alla skin il suo `::before` e il suo `::after`. Una regola del
  // telefono su quegli pseudo-elementi pesa più di quella compilata e la
  // sostituisce: la maniglia dei fogli, sul `::before` di `.finestrella`
  // (`glass-modal`), diventava il livello di ogni skin che ne disegna uno.
  const registro = readFileSync(
    join(CARTELLA, "..", "..", "..", "..", "core", "rymembra-skin", "src", "parts.rs"),
    "utf8",
  );
  const conLivelli = new Set(
    [...registro.matchAll(/PartDef::nuova\(\s*"([\w-]+)"[\s\S]*?\)(?:\.\w+\(\))*\s*,\s*\n/g)]
      .filter((m) => m[0].includes(".livelli()"))
      .map((m) => m[1] ?? ""),
  );
  assert.ok(conLivelli.has("glass-modal") && conLivelli.has("list-row"), "il registro non si legge più");
  // Le classi che nel markup stanno accanto a una di quelle parti, nello stesso letterale.
  const sorgenti = (cartella: string): string[] =>
    readdirSync(cartella, { withFileTypes: true }).flatMap((e) =>
      e.isDirectory()
        ? sorgenti(join(cartella, e.name))
        : /\.tsx?$/.test(e.name)
          ? [join(cartella, e.name)]
          : [],
    );
  const accanto = new Map<string, string>();
  for (const file of sorgenti(join(CARTELLA, ".."))) {
    const testo = readFileSync(file, "utf8").replace(/\/\*[\s\S]*?\*\//g, " ");
    for (const m of testo.matchAll(/["'`]([a-z][\w -]*)["'`]/g)) {
      const classi = (m[1] ?? "").split(/\s+/).filter((c) => c !== "");
      const parte = classi.find((c) => conLivelli.has(c));
      if (parte) for (const c of classi) accanto.set(c, parte);
    }
  }
  const occupati: string[] = [];
  for (const foglio of fogli) {
    for (const selettore of selettori(readFileSync(join(CARTELLA, foglio), "utf8")).flatMap(dividi)) {
      const pseudo = /([^\s>+~]+)::(before|after)\s*$/.exec(selettore.trim());
      if (!pseudo) continue;
      const classi = [...(pseudo[1] ?? "").matchAll(/\.([\w-]+)/g)].map((m) => m[1] ?? "");
      for (const c of classi.filter((x) => accanto.has(x))) {
        occupati.push(`${selettore.trim().replace(/\s+/g, " ")} (${c} → ${accanto.get(c)})`);
      }
    }
  }
  assert.deepEqual(occupati, []);
});
