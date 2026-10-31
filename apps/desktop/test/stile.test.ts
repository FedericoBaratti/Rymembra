import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { contrasto, leggiColore, type Rgba } from "../src/studio/colore";

/*
 * Prove sul foglio, lette dal testo di `stile.css` e dei fogli che `main.tsx`
 * importa dopo di lui.
 *
 * Il foglio non si esegue qui: non c'è un motore di impaginazione. Quel che si
 * può provare è quel che il foglio **dice** — i token del blocco generato, le
 * regole che esistono o non esistono, i blocchi dove stanno — e i conti che ne
 * seguono, come il contrasto di WCAG fatto coi token veri.
 */
// I fogli nell'ordine in cui `main.tsx` li importa, che è l'ordine della cascata.
const MAIN = readFileSync(new URL("../src/main.tsx", import.meta.url), "utf8");
const FOGLIO = [...MAIN.matchAll(/^import "\.\/(.+\.css)";\r?$/gm)]
  .map((m) => readFileSync(new URL(`../src/${m[1] ?? ""}`, import.meta.url), "utf8"))
  .join("\n")
  .replace(/\r\n/g, "\n");

/** Il corpo del primo blocco che si apre con `apertura` (graffe bilanciate). */
function corpo(testo: string, apertura: string, da = 0): string {
  const inizio = testo.indexOf(apertura, da);
  assert.ok(inizio >= 0, `non trovo «${apertura}»`);
  let i = testo.indexOf("{", inizio + apertura.length - 1);
  const primo = i + 1;
  let profondita = 0;
  for (; i < testo.length; i++) {
    if (testo[i] === "{") profondita++;
    else if (testo[i] === "}") {
      profondita--;
      if (profondita === 0) return testo.slice(primo, i);
    }
  }
  throw new Error(`blocco non chiuso: «${apertura}»`);
}

/** Il blocco generato in testa al foglio: i token della skin di serie. */
const GENERATO = FOGLIO.slice(
  FOGLIO.indexOf("inizio blocco generato"),
  FOGLIO.indexOf("fine blocco generato"),
);

function tokenDi(blocco: string, nome: string): string {
  const riga = new RegExp(`^\\s*${nome.replace(/-/g, "\\-")}:\\s*([^;]+);`, "m").exec(blocco);
  assert.ok(riga, `manca ${nome}`);
  return (riga[1] ?? "").trim();
}

/** I due temi della skin di serie, scuro di base e chiaro sopra. */
function tema(quale: "scuro" | "chiaro") {
  const scuro = corpo(GENERATO, ":root {");
  const chiaro = corpo(GENERATO, ":root[data-theme='light'] {");
  return (nome: string): Rgba => {
    const testo = quale === "chiaro" && chiaro.includes(`${nome}:`) ? tokenDi(chiaro, nome) : tokenDi(scuro, nome);
    const colore = leggiColore(testo);
    assert.ok(colore, `${nome} non è un colore: ${testo}`);
    return colore;
  };
}

/** `sopra` composto su `sotto`, con l'alfa di `sopra` moltiplicata per `opacita`. */
function componi(sopra: Rgba, sotto: Rgba, opacita = 1): Rgba {
  const a = sopra.a * opacita;
  return {
    r: sopra.r * a + sotto.r * (1 - a),
    g: sopra.g * a + sotto.g * (1 - a),
    b: sopra.b * a + sotto.b * (1 - a),
    a: 1,
  };
}

/** Le dichiarazioni della prima regola con esattamente questo selettore. */
function regola(selettore: string): string {
  return corpo(FOGLIO, `\n${selettore} {`);
}

/** L'ultimo blocco `forced-colors`, quello di `stile/contrasto.css`: il contrasto elevato. */
function bloccoForzato(): string {
  const apertura = "@media (forced-colors: active) {";
  return corpo(FOGLIO, apertura, FOGLIO.lastIndexOf(apertura));
}

/** Il testo senza commenti, perché un selettore citato in un commento non vale. */
function senzaCommenti(testo: string): string {
  return testo.replace(/\/\*[\s\S]*?\*\//g, "");
}

/**
 * Un elenco di selettori, separato alle virgole di primo livello: quelle dentro
 * `:is(...)` restano dentro. Gli spazi si riducono a uno, e spariscono accanto
 * alle parentesi, così `:is(\n  .a,\n  .b\n)` si legge `:is(.a, .b)`.
 */
function separa(testa: string): string[] {
  const parti: string[] = [];
  let profondita = 0;
  let corrente = "";
  for (const c of testa) {
    if (c === "(") profondita++;
    else if (c === ")") profondita--;
    if (c === "," && profondita === 0) {
      parti.push(corrente);
      corrente = "";
    } else corrente += c;
  }
  parti.push(corrente);
  return parti.map((s) =>
    s
      .replace(/\s+/g, " ")
      .replace(/\( /g, "(")
      .replace(/ \)/g, ")")
      .trim(),
  );
}

/**
 * Le regole di un pezzo di foglio: per ognuna i selettori (separati) e le
 * dichiarazioni. Le at-rule (`@media`, `@container`…) non sono regole: si entra
 * dentro e si leggono le loro.
 */
function regole(testo: string): { selettori: string[]; dichiarazioni: string }[] {
  const pulito = senzaCommenti(testo);
  const fuori: { selettori: string[]; dichiarazioni: string }[] = [];
  const apertura = /([^{};]+)\{/g;
  let m: RegExpExecArray | null;
  while ((m = apertura.exec(pulito)) !== null) {
    const testa = (m[1] ?? "").trim();
    if (testa.startsWith("@")) continue;
    // Le dichiarazioni fino alla graffa che chiude: le regole qui non si annidano.
    const fine = pulito.indexOf("}", apertura.lastIndex);
    fuori.push({
      selettori: separa(testa),
      dichiarazioni: pulito.slice(apertura.lastIndex, fine),
    });
    apertura.lastIndex = fine + 1;
  }
  return fuori;
}

const SOGLIA_AA = 4.5;

test("tema chiaro: l'accento sul suo fondo tenue passa 4,5:1 (voce attiva, riga che suona, coda al sorvolo)", () => {
  for (const quale of ["scuro", "chiaro"] as const) {
    const t = tema(quale);
    const accento = t("--accent");
    const tenue = t("--accent-soft");
    // `.voce[data-active]`: accento su `--accent-soft` sopra `surface-1`.
    const voce = contrasto(accento, componi(tenue, t("--color-surface-1")));
    // `.riga[aria-current] .nome`: accento su `--accent-soft` sopra `surface-0`.
    const riga = contrasto(accento, componi(tenue, t("--color-surface-0")));
    // `.riga-coda:hover` con la riga che suona: accento su `surface-3`.
    const coda = contrasto(accento, t("--color-surface-3"));
    for (const [dove, valore] of [
      ["voce attiva", voce],
      ["riga che suona", riga],
      ["coda al sorvolo", coda],
    ] as const) {
      assert.ok(valore >= SOGLIA_AA, `${quale}, ${dove}: ${valore.toFixed(2)}:1`);
    }
  }
});

test("le righe del testo già cantate restano sopra 4,5:1, con e senza le barre dietro", () => {
  const passata = regola(".lyric-line[data-passata]");
  const opacita = /opacity:\s*([\d.]+)/.exec(passata);
  const quanta = opacita ? Number(opacita[1]) : 1;
  for (const quale of ["scuro", "chiaro"] as const) {
    const t = tema(quale);
    const fondoPannello = t("--color-surface-0");
    // Il pannello è `surface-0` all'82 % sopra la scena: senza barre la scena
    // è il fondo stesso, sopra le barre è il colore delle barre o l'accento.
    for (const [dove, scena] of [
      ["senza barre", t("--color-surface-0")],
      ["sopra le barre", t("--viz-secondary")],
      ["sopra l'accento", t("--accent")],
    ] as const) {
      const fondo = componi(fondoPannello, scena, 0.82);
      const testo = componi(t("--color-text-3"), fondo, quanta);
      const valore = contrasto(testo, fondo);
      assert.ok(valore >= SOGLIA_AA, `${quale}, ${dove}: ${valore.toFixed(2)}:1`);
    }
  }
});


test("le regole dello Studio non raggiungono il cursore del lettore", () => {
  // `parti/Scrubber.tsx` avvolge la barra di posizione in `.cursore`: una
  // regola scritta per i cursori dello Studio con quel nome nudo le dava 4 px
  // di altezza e la griglia a tre colonne del pannello.
  const ancore = [".lettore", ".colonna", ".in-riproduzione", ".player-progress"];
  const sconfinate: string[] = [];
  for (const { selettori } of regole(FOGLIO)) {
    for (const s of selettori) {
      if (!/\.cursore(?![-\w])/.test(s)) continue;
      if (ancore.some((a) => s.includes(a))) continue;
      // I due tempi della barra mentre si cerca: sono dello Scrubber.
      if (/^\.cursore \.tempo\.cercando/.test(s)) continue;
      sconfinate.push(s);
    }
  }
  assert.deepEqual(sconfinate, []);

  const scrubber = readFileSync(new URL("../src/parti/Scrubber.tsx", import.meta.url), "utf8");
  assert.match(scrubber, /className="cursore player-progress"/);
  for (const file of ["studio/controlli.tsx", "studio/IspezioneVoci.tsx", "studio/Pila.tsx"]) {
    const sorgente = readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");
    assert.doesNotMatch(sorgente, /className="cursore[ "]/, `${file} usa ancora la classe del lettore`);
  }
});

/**
 * La specificità di un selettore semplice, come terna (id, classi, elementi).
 * Basta per i selettori di questo foglio: `:is()` vale quanto il suo argomento
 * più pesante, gli pseudo-elementi contano come elementi.
 */
function specificita(selettore: string): [number, number, number] {
  let s = selettore;
  let base: [number, number, number] = [0, 0, 0];
  // `:is(...)` e `:where(...)` per primi: il primo pesa il suo argomento più forte, il secondo niente.
  s = s.replace(/:(is|where)\(([^()]*)\)/g, (_, quale: string, dentro: string) => {
    if (quale === "is") {
      for (const arg of dentro.split(",")) {
        const a = specificita(arg.trim());
        if (confronta(a, base) > 0) base = a;
      }
    }
    return " ";
  });
  const id = (s.match(/#[\w-]+/g) ?? []).length;
  const classi =
    (s.match(/\.[\w-]+/g) ?? []).length +
    (s.match(/\[[^\]]*\]/g) ?? []).length +
    (s.match(/(?<!:):(?!:)[\w-]+/g) ?? []).length;
  const elementi =
    (s.replace(/\[[^\]]*\]/g, "").match(/(^|[\s>+~])[a-z][\w-]*/gi) ?? []).length +
    (s.match(/::[\w-]+/g) ?? []).length;
  return [base[0] + id, base[1] + classi, base[2] + elementi];
}

function confronta(a: [number, number, number], b: [number, number, number]): number {
  return a[0] - b[0] || a[1] - b[1] || a[2] - b[2];
}

test("la specificità si conta come nel motore", () => {
  assert.deepEqual(specificita(".editor-regole select"), [0, 1, 1]);
  assert.deepEqual(specificita(".eq-cursore:focus-visible"), [0, 2, 0]);
  assert.deepEqual(specificita(".cursore input[type='range']"), [0, 2, 1]);
  assert.deepEqual(specificita(":is(.a b, .c):focus-visible"), [0, 2, 1]);
  assert.deepEqual(specificita(".scorrimento::-webkit-slider-thumb"), [0, 1, 1]);
});

test("in contrasto elevato i cursori hanno pista e pallina", () => {
  const forzato = regole(bloccoForzato());
  const dichiara = (selettore: string, proprieta: RegExp) =>
    forzato.some((r) => r.selettori.includes(selettore) && proprieta.test(r.dichiarazioni));
  // La pista: un bordo di sistema, e il riempimento in `Highlight` che il motore non tocca.
  for (const pista of [".scorrimento::-webkit-slider-runnable-track", ".eq-cursore::-webkit-slider-runnable-track"]) {
    assert.ok(dichiara(pista, /border:\s*1px solid CanvasText/), `${pista}: manca il bordo`);
    assert.ok(dichiara(pista, /forced-color-adjust:\s*none/), `${pista}: il gradiente verrebbe tolto`);
    assert.ok(dichiara(pista, /Highlight/), `${pista}: manca il riempimento`);
  }
  // La pallina: un colore di sistema pieno, e quella dello scrubber si vede.
  for (const pallina of [".scorrimento::-webkit-slider-thumb", ".eq-cursore::-webkit-slider-thumb"]) {
    assert.ok(dichiara(pallina, /background:\s*CanvasText/), `${pallina}: pallina del colore del fondo`);
  }
  assert.ok(dichiara(".scorrimento::-webkit-slider-thumb", /opacity:\s*1/), "la pallina dello scrubber resta invisibile");
  // Spento resta spento: la regola di fuori pesa di più (0,2,1) e vince ancora.
  const spenta = regola(".scorrimento:disabled::-webkit-slider-thumb");
  assert.match(spenta, /opacity:\s*0/);
  assert.ok(
    confronta(specificita(".scorrimento:disabled::-webkit-slider-thumb"), specificita(".scorrimento::-webkit-slider-thumb")) > 0,
  );
});

test("in contrasto elevato nessun outline:none batte l'anello Highlight", () => {
  // I comandi senza cursore di testo — menu a tendina e cursori — che fuori dal
  // blocco tolgono l'anello con un selettore più pesante di `:focus-visible`.
  const bersagli = [
    ".editor-regole input",
    ".editor-regole select",
    ".eq-cursore",
    ".proprieta-riga input",
    ".proprieta-riga select",
    ".manopole-livello input",
    ".manopole-livello select",
    ".scala-passi input",
    ".scala-passi select",
  ];
  const testoForzato = bloccoForzato();
  const forzato = regole(testoForzato);
  const fuori = regole(FOGLIO.replace(testoForzato, ""));
  for (const bersaglio of bersagli) {
    const spegne = fuori.flatMap((r) =>
      /outline:\s*none/.test(r.dichiarazioni)
        ? r.selettori.filter((s) => s === bersaglio || s === `${bersaglio}:focus-visible` || s === `${bersaglio}:focus`)
        : [],
    );
    assert.ok(spegne.length > 0, `${bersaglio}: nessuna regola lo spegne, la prova va aggiornata`);
    const anelli = forzato.flatMap((r) =>
      /outline:\s*2px solid Highlight/.test(r.dichiarazioni) ? r.selettori.filter((s) => s.includes(":focus-visible")) : [],
    );
    const copre = anelli.find(
      (a) =>
        (a === ":focus-visible" || a.includes(bersaglio)) &&
        spegne.every((s) => confronta(specificita(a), specificita(s)) >= 0),
    );
    assert.ok(copre, `${bersaglio}: in contrasto elevato vince «outline: none» (${spegne.join(", ")})`);
  }
});

/** I blocchi `@container (max-width: N)`, col loro N e le regole che contengono. */
function contenitoriStretti(): { larghezza: number; regole: ReturnType<typeof regole> }[] {
  const pulito = senzaCommenti(FOGLIO);
  const fuori: { larghezza: number; regole: ReturnType<typeof regole> }[] = [];
  for (const m of pulito.matchAll(/@container(?: [\w-]+)? \(max-width: (\d+)px\) \{/g)) {
    fuori.push({ larghezza: Number(m[1]), regole: regole(corpo(pulito, m[0], m.index)) });
  }
  return fuori;
}

test("le Impostazioni si impilano su un contenitore stretto, con l'indice in una striscia", () => {
  // A 880 px logici e ingrandimento 200 % restavano un centinaio di pixel alle
  // schede: l'indice da 186 px e lo spazio fra le colonne si prendevano il resto.
  const stretto = contenitoriStretti().find(({ regole: dentro }) =>
    dentro.some(
      (r) => r.selettori.includes(".impostazioni") && /grid-template-columns:\s*minmax\(0, 1fr\);/.test(r.dichiarazioni),
    ),
  );
  assert.ok(stretto, "nessuna @container porta le Impostazioni a una colonna");
  assert.ok(stretto.larghezza >= 560, `${stretto.larghezza}px`);
  const indice = stretto.regole.find((r) => r.selettori.includes(".impostazioni .indice"));
  assert.ok(indice, "l'indice resta una colonna fissa sopra le schede");
  assert.match(indice.dichiarazioni, /position:\s*static/);
  assert.match(indice.dichiarazioni, /flex-direction:\s*row/);
  assert.match(indice.dichiarazioni, /overflow-x:\s*auto/);
});

test("nessun :hover disegna l'anello del fuoco", () => {
  // Il fuoco globale è `outline: 2px solid var(--accent)`; un sorvolo con lo
  // stesso segno su un'altra scheda fa perdere a chi usa il tabulatore dove è.
  const anello = /outline:\s*2px solid var\(--accent\)/;
  const sorvoli = regole(FOGLIO)
    .filter((r) => r.selettori.some((s) => s.includes(":hover")) && anello.test(r.dichiarazioni))
    .flatMap((r) => r.selettori);
  assert.deepEqual(sorvoli, []);
});

test("il campo della chat dello Studio sembra occupato durante un turno", () => {
  const occupato = regola(".chat-scrivi textarea[aria-disabled='true']");
  assert.match(occupato, /opacity:\s*0\.6/);
  assert.match(occupato, /cursor:\s*progress/);
});
