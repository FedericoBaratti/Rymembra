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
  // `.foglio-np` è «In riproduzione» sul telefono (`InRiproduzioneTelefono`).
  const ancore = [".lettore", ".colonna", ".in-riproduzione", ".foglio-np", ".player-progress"];
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

test("la pista di una barra di avanzamento è un blocco anche dentro uno <span>", () => {
  // `parti/Aggiornamenti.tsx` la scrive come `<span>`, perché sta dentro un
  // paragrafo dove un `<div>` non è valido: in linea, `height` e `overflow`
  // non valgono, e la barra dello scaricamento non si vedeva mai.
  assert.match(regola(".toast-progress"), /display:\s*block/);
});

test("le regole nude dello Studio non vestono il markup del resto dell'app", () => {
  // `.mancanti` e `.avviso` di `studio.css` disegnavano elenchi e avvisi che lo
  // Studio non ha più: la prima metteva in fila come parole i brani mancanti del
  // rapporto d'importazione (`parti/Rapporto.tsx`).
  const nude = regole(senzaCommenti(FOGLIO))
    .flatMap((r) => r.selettori)
    .filter((s) => s === ".mancanti" || s === ".avviso" || s === ".avviso .cosa");
  assert.deepEqual(nude, [".avviso"], "resta solo l'.avviso di importazioni.css");
});

test("il foglio del motore non legge la tavolozza di una skin", () => {
  // `--skin-color-*` è lo spazio dei nomi dei colori che **una** skin dichiara:
  // letto qui, dava a Vinile e alle skin di terzi il ciano di Plain.
  const fuori = senzaCommenti(FOGLIO.replace(GENERATO, ""));
  assert.deepEqual(fuori.match(/var\(--skin-color-[\w-]+/g) ?? [], []);
});

test("chi galleggia in fondo riserva il suo spazio, anche al fuoco", () => {
  // Il lettore e la barra della selezione galleggiano sopra l'elenco: senza
  // un `padding-bottom` l'ultima riga resta sotto, e senza `scroll-padding` il
  // motore considera visibile una riga che il lettore copre, e non scorre
  // quando il fuoco ci arriva (WCAG 2.4.11).
  const fondo = regole(senzaCommenti(FOGLIO)).filter((r) =>
    r.selettori.some((s) => /^\[data-scafale\].*\.posto-content \.dentro$/.test(s)),
  );
  for (const attributo of ["data-lettore", "data-selezione"]) {
    const sua = fondo.find((r) => r.selettori.some((s) => s.includes(attributo)));
    assert.ok(sua, `nessuna riserva in fondo con [${attributo}]`);
    assert.match(sua.dichiarazioni, /padding-bottom:\s*var\(--player-clearance\)/, attributo);
    assert.match(sua.dichiarazioni, /scroll-padding-bottom:\s*var\(--player-clearance\)/, attributo);
  }
  // L'attributo nasce dallo stesso predicato che decide se la barra si vede.
  const impaginazione = readFileSync(new URL("../src/Impaginazione.tsx", import.meta.url), "utf8");
  assert.match(impaginazione, /data-selezione=\{WIDGET\["selection-bar"\]\?\.visibile\?\.\(contesto/);
});

/** Vero se `selettore` è `antenato` stesso, o un suo discendente scritto a partire da lui. */
function dentro(selettore: string, antenato: string): boolean {
  return selettore === antenato || selettore.startsWith(`${antenato} `) || selettore.startsWith(`${antenato}::`);
}

test("in contrasto elevato nessun testo HighlightText finisce sul fondo di sistema", () => {
  // Con `forced-color-adjust: auto` il motore disegna dietro a ogni riga di
  // testo un fondo `Canvas`, e in tutte le tavolozze di sistema `HighlightText`
  // è il colore di `Canvas`: la voce attiva della navigazione e la linguetta
  // scelta diventavano un rettangolo pieno senza nome (R16-2, visto dal vivo).
  // Chi scrive testo `HighlightText` deve togliere quel fondo, su di sé o su un
  // antenato dichiarato nello stesso blocco.
  const forzato = regole(bloccoForzato());
  const senzaFondo = forzato
    .filter((r) => /forced-color-adjust:\s*none/.test(r.dichiarazioni))
    .flatMap((r) => r.selettori);
  const scoperti = forzato
    .filter((r) => /(^|[^-])color:\s*HighlightText/.test(r.dichiarazioni))
    .flatMap((r) => r.selettori)
    .filter((s) => !senzaFondo.some((a) => dentro(s, a)));
  assert.deepEqual(scoperti, []);
});

test("in contrasto elevato la riga scelta e i tasti premuti hanno un segno", () => {
  const forzato = regole(bloccoForzato());
  /** Le dichiarazioni (unite) delle regole che nominano proprio questo selettore. */
  const di = (selettore: string) =>
    forzato
      .filter((r) => r.selettori.includes(selettore))
      .map((r) => r.dichiarazioni)
      .join("\n");
  // Riga scelta e tasti premuti: fondo `Highlight`, con `!important` perché il
  // foglio della skin arriva dopo e i suoi stati pesano (0,3,0) o più — Neon e
  // Vinile li ridipingono — e una regola d'autore più pesante, anche forzata,
  // toglie il colore di sistema di questo blocco.
  for (const scelto of [
    ".voce[data-active]",
    ".segmentato .linguetta[data-active]",
    ".riga[data-scelta]",
    ".tasto[aria-pressed='true']",
    ".tasto-icona[aria-pressed='true']",
    ".voce-livello[aria-selected='true']",
    ".riga-scafale[aria-selected='true']",
  ]) {
    const d = di(scelto);
    assert.match(d, /background:\s*Highlight !important/, `${scelto}: nessun fondo Highlight`);
    assert.match(d, /(^|[^-])color:\s*HighlightText !important/, `${scelto}: il testo non è HighlightText`);
    assert.match(d, /forced-color-adjust:\s*none/, `${scelto}: il testo finirebbe sul fondo di sistema`);
  }
  // La riga che suona e insieme è scelta: l'anello rientrato non può essere
  // `Highlight` sopra un fondo `Highlight`.
  assert.match(di(".riga[data-scelta][aria-current='true']"), /outline-color:\s*HighlightText/);
  assert.match(di(".riga[data-scelta][aria-current='true']"), /outline-offset:\s*-4px/);
  // Accesi (l'equalizzatore coi filtri, una sessione insieme): l'icona in
  // `Highlight`, un gradino sotto il premuto.
  for (const acceso of [
    ".con-pannello .tasto[data-acceso]:not([aria-pressed='true'])",
    ".tasto-insieme[data-acceso]:not([aria-pressed='true'])",
  ]) {
    assert.match(di(acceso), /(^|[^-])color:\s*Highlight !important/, acceso);
  }
  // La riga cantata, le tacche della fiducia e la linguetta dello Studio.
  assert.match(di(".lyric-line[data-attiva]"), /(^|[^-])color:\s*Highlight !important/);
  assert.match(di(".tacche > span"), /border:\s*1px solid CanvasText/);
  assert.match(di(".tacche > span[data-accesa]"), /background:\s*Highlight/);
  assert.match(di(".scheda-studio[aria-selected='true']::after"), /background:\s*Highlight/);
});

test("in contrasto elevato il guscio di un campo di ricerca segna il fuoco", () => {
  // Il campo toglie il suo anello e il fuoco lo dice il guscio, con un bordo e
  // un'ombra d'accento: due cose che il contrasto elevato appiattisce. Restava
  // il cursore lampeggiante, che sparisce col testo selezionato (D-74).
  const forzato = regole(bloccoForzato());
  const anelli = forzato
    .filter((r) => /outline:\s*2px solid Highlight/.test(r.dichiarazioni))
    .flatMap((r) => r.selettori);
  for (const guscio of [".cerca", ".filtro-impostazioni", ".cerca-livelli"]) {
    assert.ok(
      anelli.some((s) => s.endsWith(":focus-within") && s.includes(guscio)),
      `${guscio}: nessun anello Highlight al fuoco`,
    );
  }
});

test("in contrasto elevato i colori che sono il contenuto restano quelli", () => {
  // Il motore impone i colori di sistema anche agli stili in linea: le
  // anteprime delle skin diventavano tre riquadri uguali, i campioni dello
  // Studio quadratini vuoti.
  const tutte = regole(senzaCommenti(FOGLIO));
  for (const contenuto of [
    ".scena-spettro",
    ".scheda-skin .bande",
    ".basi-tema .bande",
    ".campione",
    ".valore-colore .pastiglia",
  ]) {
    assert.ok(
      tutte.some((r) => r.selettori.includes(contenuto) && /forced-color-adjust:\s*none/.test(r.dichiarazioni)),
      `${contenuto}: il motore lo ridipinge`,
    );
  }
});

/**
 * I temi delle tre skin di serie, base e variante, letti dai loro documenti:
 * cinque in tutto (Plain scuro e chiaro, Neon, Vinile chiaro e scuro). Il
 * blocco generato del foglio ha solo Plain, e una soglia che regge su Plain può
 * non reggere sulle altre due. I nomi sono quelli del foglio (`--accent`,
 * `--color-text-3`), tradotti in quelli del documento.
 */
function temiDiSerie(): { nome: string; colore: (nome: string) => Rgba }[] {
  type Valore = string | { $palette?: string; $token?: string; alpha?: number };
  const fuori: { nome: string; colore: (nome: string) => Rgba }[] = [];
  for (const id of ["plain", "neon", "vinile"]) {
    const doc = JSON.parse(
      readFileSync(new URL(`../../../core/rymembra-skin/skins/${id}.json`, import.meta.url), "utf8"),
    ) as {
      palette: Record<string, string>;
      tokens: Record<string, Valore>;
      variant?: { palette?: Record<string, string>; tokens?: Record<string, Valore> };
    };
    const varianti: [string, Record<string, string>, Record<string, Valore>][] = [
      [id, doc.palette, doc.tokens],
    ];
    if (doc.variant) {
      varianti.push([
        `${id}, variante`,
        { ...doc.palette, ...(doc.variant.palette ?? {}) },
        { ...doc.tokens, ...(doc.variant.tokens ?? {}) },
      ]);
    }
    for (const [nome, tavolozza, token] of varianti) {
      const risolvi = (v: Valore | undefined): Rgba => {
        if (typeof v === "string") {
          const c = leggiColore(v);
          assert.ok(c, `${nome}: ${v} non è un colore`);
          return c;
        }
        if (v?.$palette !== undefined) {
          const c = risolvi(tavolozza[v.$palette]);
          return { ...c, a: v.alpha ?? 1 };
        }
        if (v?.$token !== undefined) return risolvi(token[v.$token]);
        throw new Error(`${nome}: colore? ${JSON.stringify(v)}`);
      };
      const chiave = (css: string) =>
        css === "--accent"
          ? "color.accent"
          : css === "--accent-soft"
            ? "color.accent.soft"
            : css.replace(/^--/, "").replace(/-/g, ".");
      fuori.push({ nome, colore: (css) => risolvi(token[chiave(css)]) });
    }
  }
  assert.equal(fuori.length, 5);
  return fuori;
}

/** Il primo numero che segue `prima` nel testo, o `ripiego`. */
function numeroDopo(testo: string, prima: RegExp, ripiego?: number): number {
  const m = new RegExp(prima.source + String.raw`\s*([\d.]+)`).exec(testo);
  if (!m) {
    assert.ok(ripiego !== undefined, `non trovo ${prima.source}`);
    return ripiego;
  }
  return Number(m[1]);
}

test("le testate sopra la copertina sfocata reggono 4,5:1 con qualunque copertina", () => {
  // La copertina sfocata è la copertina stessa: bianca col tema scuro, o nera
  // col tema chiaro, sotto l'occhiello «In riproduzione» lasciava 2,1:1. Le tre
  // testate stanno in cima, e in cima il velo deve essere fitto (D-75). Il
  // caso peggiore è una copertina bianca o nera piena, con e senza l'alone
  // della skin (`--hero-rgb`, l'accento) sotto di lei.
  const bianco: Rgba = { r: 1, g: 1, b: 1, a: 1 };
  const nero: Rgba = { r: 0, g: 0, b: 0, a: 1 };
  const di = (selettore: string) => regola(selettore);
  /**
   * L'alfa del velo (o della maschera) in cima, e fin dove arriva prima di
   * diradarsi. `assente` è l'alfa di quando non c'è: zero per un velo, uno per
   * una maschera.
   */
  const fascia = (dichiarazioni: string, colore: RegExp, assente: number): { alfa: number; fin: string } => {
    const fermate = [...dichiarazioni.matchAll(new RegExp(colore.source + String.raw`\s*([^,\n]*)`, "g"))];
    if (fermate.length === 0) return { alfa: assente, fin: "" };
    const [prima, seconda] = fermate;
    // Una fascia è la seconda fermata con lo stesso alfa della prima.
    const fin = prima?.[1] === seconda?.[1] ? (seconda?.[2] ?? "").trim() : (prima?.[2] ?? "").trim();
    return { alfa: Number(prima?.[1]), fin };
  };

  // In riproduzione sul computer: superficie, alone, tinta, velo.
  const np = fascia(di(".in-riproduzione .np-scrim"), /rgb\(var\(--surface-0-rgb\) \/ ([\d.]+)\)/, 0);
  const tintaNp = numeroDopo(di(".in-riproduzione .ambiente .tinta"), /opacity:/);
  const aloneNp = Math.max(
    ...[...di(".in-riproduzione .ambiente").matchAll(/--hero-rgb\) \/ ([\d.]+)/g)].map((m) => Number(m[1])),
  );
  // La colonna: niente velo che possa starle sopra (la tinta è dentro
  // `ambient-backdrop`, una parte con livelli), quindi la maschera dello strato.
  const colonna = fascia(di(".colonna .ambiente"), /rgb\(0 0 0 \/ ([\d.]+)\)/, 1);
  const tintaColonna = numeroDopo(di(".colonna .ambiente .tinta"), /opacity:/);
  const aloneColonna = numeroDopo(di(".colonna .ambiente"), /--hero-rgb\) \/ /);
  // Il telefono.
  const telefono = fascia(
    di(':root[data-forma="telefono"] .sfondo-np::after'),
    /rgb\(var\(--surface-0-rgb\) \/ ([\d.]+)\)/,
    0,
  );
  const tintaTelefono = numeroDopo(di(':root[data-forma="telefono"] .tinta-np'), /opacity:/);

  const cadute: string[] = [];
  for (const { nome, colore } of temiDiSerie()) {
    const s0 = colore("--color-surface-0");
    const s1 = colore("--color-surface-1");
    const accento = colore("--accent");
    for (const copertina of [bianco, nero]) {
      for (const alone of [false, true]) {
        const misura = (dove: string, testo: string, fondo: Rgba, soglia: number) => {
          const valore = contrasto(colore(testo), fondo);
          if (valore < soglia) cadute.push(`${nome}, ${dove}: ${valore.toFixed(2)}:1`);
        };
        // In riproduzione: occhiello 10 px e «artista · album» col testo, icone a 3:1.
        let fondo = alone ? componi(accento, s0, aloneNp) : s0;
        fondo = componi(copertina, fondo, tintaNp);
        fondo = componi(s0, fondo, np.alfa);
        misura("in riproduzione, occhiello", "--color-text-3", fondo, SOGLIA_AA);
        misura("in riproduzione, artista · album", "--color-text-2", fondo, SOGLIA_AA);
        // La colonna: la maschera mescola lo strato alla superficie che c'è sotto.
        let strato = alone ? componi(accento, s1, aloneColonna) : s1;
        strato = componi(copertina, strato, tintaColonna);
        fondo = componi(strato, s1, colonna.alfa);
        misura("colonna, occhiello", "--color-text-3", fondo, SOGLIA_AA);
        misura("colonna, icone", "--color-text-2", fondo, 3);
        // Il telefono: occhiello in `text-2`, il nome del disco in `text-1`.
        fondo = componi(s0, componi(copertina, s0, tintaTelefono), telefono.alfa);
        misura("telefono, occhiello", "--color-text-2", fondo, SOGLIA_AA);
        misura("telefono, disco", "--color-text-1", fondo, SOGLIA_AA);
      }
    }
  }
  assert.deepEqual([...new Set(cadute)], []);
  // E il velo fitto arriva fin sotto quel che legge. Sul computer, il sommario:
  // con lo spettro acceso sale in cima con la sua riga «artista · album»
  // (copertina fino a 168 px, sotto la testata). Nella colonna, la testata.
  // Sul telefono, la barra di stato e la testata.
  assert.ok(np.fin.endsWith("px") && Number.parseFloat(np.fin) >= 240, `in riproduzione: fino a «${np.fin}»`);
  assert.ok(colonna.fin.endsWith("px") && Number.parseFloat(colonna.fin) >= 56, `colonna: fino a «${colonna.fin}»`);
  assert.match(telefono.fin, /--barra-titolo-h.*--barra-alta-h/, "telefono");
});

test("nessuna opacità porta un testo informativo sotto 4,5:1", () => {
  // I token passano tutti 4,5:1 sulle quattro superfici; il foglio però li
  // moltiplicava per un'opacità: il conteggio della voce attiva, i numeri di un
  // segmentato, una cartella che non risponde, un'uscita audio staccata, il
  // selettore delle barre a riposo, un valore ereditato nello Studio.
  const fuori = regole(senzaCommenti(FOGLIO.replace(bloccoForzato(), "")));
  const opacita = (selettore: string) => {
    const r = fuori.find((x) => x.selettori.includes(selettore) && /opacity:/.test(x.dichiarazioni));
    return r ? numeroDopo(r.dichiarazioni, /opacity:/) : 1;
  };
  const coppie: [string, string, (c: (n: string) => Rgba) => Rgba][] = [
    [".voce[data-active] .conteggio", "--accent", (c) => componi(c("--accent-soft"), c("--color-surface-1"))],
    [".segmentato .quante", "--color-text-3", (c) => c("--color-surface-2")],
    [".segmentato .quante", "--color-text-3", (c) => c("--color-surface-3")],
    [".nodo-spento", "--color-text-3", (c) => c("--color-surface-0")],
    [".nodo-spento", "--color-text-2", (c) => c("--color-surface-0")],
    [".uscite .assente label", "--color-text-2", (c) => c("--color-surface-1")],
    [".uscite .assente .nome", "--color-text-3", (c) => c("--color-surface-1")],
    [".dettaglio-spettro", "--color-text-2", (c) => c("--color-surface-0")],
    [".dettaglio-spettro", "--color-text-3", (c) => c("--color-surface-0")],
    ["[data-ereditato]", "--color-text-3", (c) => c("--color-surface-1")],
  ];
  const cadute: string[] = [];
  for (const { nome, colore } of temiDiSerie()) {
    for (const [selettore, testo, fondoDi] of coppie) {
      const fondo = fondoDi(colore);
      const valore = contrasto(componi(colore(testo), fondo, opacita(selettore)), fondo);
      if (valore < SOGLIA_AA) cadute.push(`${nome}, ${selettore} (${testo}): ${valore.toFixed(2)}:1`);
    }
  }
  assert.deepEqual(cadute, []);
});

test("S9-5: una riga grigia si attenua col colore e non con l'opacità, e il suo testo resta sopra 4,5:1", () => {
  // Il difetto: `.riga[data-spenta] { opacity: 0.55 }` su tutta la riga. Titolo,
  // artista, album e durata scendevano sotto 4,5:1 (nel tema chiaro anche il
  // titolo, 3,5:1; l'album 2,4:1) e con loro il cuore e le stelle, che nella riga
  // grigia restano comandi attivi. Qui si legge dal foglio di che colore sono, e
  // si fa il conto con i token di tutte le skin di serie, sopra ogni fondo su cui
  // sta una riga: le quattro superfici, il sorvolo e la riga scelta.
  const fuori = regole(senzaCommenti(FOGLIO.replace(bloccoForzato(), "")));
  const spente = fuori.filter((r) => r.selettori.some((s) => s.startsWith(".riga[data-spenta]")));
  assert.ok(spente.length > 0, "la riga grigia ha le sue regole");
  for (const r of spente) {
    // L'opacità sta bene solo sulla miniatura, che non è testo (come l'icona di
    // `.nodo-spento`): sul resto — la riga, i testi, i comandi — no.
    const soloMiniatura = r.selettori.every((s) => s.endsWith(".miniatura"));
    if (!soloMiniatura) assert.doesNotMatch(r.dichiarazioni, /opacity:/, r.selettori.join(", "));
  }
  const figli = [".nome", ".autore", ".disco", ".durata", ".indice"];
  const cadute: string[] = [];
  let minimo = Infinity;
  for (const figlio of figli) {
    const colorata = spente.find(
      (r) =>
        r.selettori.some((s) => s.startsWith(".riga[data-spenta]") && s.endsWith(` ${figlio}`)) &&
        /(^|[;\s])color:/.test(r.dichiarazioni),
    );
    assert.ok(colorata, `${figlio}: la riga grigia lo colora, con un token del tema`);
    const token = /(?:^|[;\s])color:\s*var\((--[a-z0-9-]+)\)/.exec(colorata.dichiarazioni)?.[1];
    assert.ok(token, `${figlio}: un token, non un colore scritto a mano`);
    for (const { nome, colore } of temiDiSerie()) {
      const testo = colore(token);
      const fondi: [string, Rgba][] = [];
      // Una riga di elenco sta sulla pagina o su un pannello: `surface-0…2`.
      // `surface-3` è dei menù e della coda al sorvolo, e le righe di brani non ci
      // stanno: lì vale solo il fondo nudo.
      for (const s of ["0", "1", "2", "3"]) {
        const superficie = colore(`--color-surface-${s}`);
        fondi.push([`surface-${s}`, superficie]);
        if (s === "3") continue;
        // `.riga:hover`: il 6 % del testo sopra la superficie.
        fondi.push([`sorvolo su surface-${s}`, componi(colore("--color-text-1"), superficie, 0.06)]);
        // `.riga[data-scelta]`: `--accent-soft` sopra la superficie.
        fondi.push([`scelta su surface-${s}`, componi(colore("--accent-soft"), superficie)]);
      }
      for (const [dove, fondo] of fondi) {
        const valore = contrasto(testo, fondo);
        minimo = Math.min(minimo, valore);
        if (valore < SOGLIA_AA) cadute.push(`${nome}, ${figlio} (${token}) su ${dove}: ${valore.toFixed(2)}:1`);
      }
    }
  }
  console.log(`S9-5: il rapporto più basso del testo di una riga grigia è ${minimo.toFixed(2)}:1`);
  assert.deepEqual(cadute, []);
  // Il titolo corrente resta d'accento anche se la riga è grigia, e il segno
  // «spenta» che non è un colore (il corsivo) sopravvive al contrasto elevato.
  const nome = spente.find((r) => r.selettori.some((s) => s.endsWith(" .nome")));
  assert.ok(nome && /font-style:\s*italic/.test(nome.dichiarazioni), "il titolo grigio è in corsivo");
  assert.ok(
    nome.selettori.every((s) => s.includes(':not([aria-current="true"])')),
    "la riga che suona tiene il titolo d'accento",
  );
  // In contrasto elevato la miniatura torna piena, come l'icona di `.nodo-spento`.
  assert.match(bloccoForzato(), /\.riga\[data-spenta\] \.miniatura\s*\{\s*opacity:\s*1;/);
});

test("con lo spettro acceso l'alone dei comandi è del colore del fondo del tema", () => {
  // Sopra le barre i comandi non hanno un fondo, solo un'ombra. Nera, reggeva il
  // tema scuro (testo chiaro su ombra scura) e non il chiaro: icone scure con
  // un'ombra scura sopra barre scure. L'alone deve venire da `surface-0`, che
  // nel tema chiaro è chiaro (R14b-11).
  for (const selettore of [
    ".in-riproduzione .centro[data-spettro] .comandi-np",
    ".in-riproduzione .centro[data-spettro] .comandi-np svg",
  ]) {
    const d = regola(selettore);
    assert.match(d, /--surface-0-rgb/, selettore);
    assert.doesNotMatch(d, /rgb\(0 0 0/, selettore);
  }
});

/** Un numero di pixel da una dichiarazione, o `ripiego`. */
function pixel(dichiarazioni: string, proprieta: string, ripiego = 0): number {
  const m = new RegExp(String.raw`(?:^|[;\s])` + proprieta + String.raw`:\s*(-?[\d.]+)px`).exec(dichiarazioni);
  return m ? Number(m[1]) : ripiego;
}

test("le stelle e il triangolo dell'albero sono bersagli da 24 pixel", () => {
  // WCAG 2.2 2.5.8: cinque stelle da 15 pixel a un pixel l'una dall'altra, e
  // nessun'altra strada per votare: chi punta con poca precisione prendeva la
  // stella accanto, cioè un voto sbagliato.
  const stella = regola(".stella");
  const stelle = regola(".stelle");
  const passo = pixel(stella, "width") + pixel(stelle, "gap");
  assert.ok(passo >= 24, `stelle a ${passo} px l'una dall'altra`);
  assert.ok(pixel(stella, "height") >= 24, "stelle basse");
  // La colonna del voto le contiene tutte e cinque, in ogni larghezza
  // dell'elenco che la mostra: è la traccia subito prima della durata (46 px).
  for (const m of senzaCommenti(FOGLIO).matchAll(/--colonne-riga:\s*([^;]+);/g)) {
    const tracce = (m[1] ?? "").trim().split(/\s+(?![^(]*\))/);
    const durata = tracce.indexOf("46px");
    const titolo = tracce.indexOf("minmax(0, 1fr)");
    if (durata - titolo < 2) continue; // la colonna del voto non c'è
    const voto = Number.parseFloat(tracce[durata - 1] ?? "0");
    assert.ok(voto >= passo * 5, `colonna del voto da ${voto} px in «${m[1]}»`);
  }
  // Il triangolo dell'albero resta di 16 pixel, perché l'altezza della riga la
  // misura la finestra virtuale: il bersaglio cresce con uno pseudo-elemento.
  const bersaglio = regola(".freccia-nodo:not(:empty)::after");
  const lato = pixel(regola(".freccia-nodo"), "width") - 2 * pixel(bersaglio, "inset");
  assert.ok(lato >= 24, `triangolo da ${lato} px`);
});

test("chi galleggia in cima alla pagina riserva il suo spazio al fuoco (R14b-4)", () => {
  // Gli avvisi aderenti (`.aderenti-pagina`) stanno in cima allo scorrevole
  // della pagina: senza `scroll-padding-top` il salto a una riga lontana
  // (`scorriA`, che lo legge dal foglio) la metteva sotto di loro.
  const cima = regole(senzaCommenti(FOGLIO)).filter((r) =>
    r.selettori.some((s) => s.startsWith(".posto-content .dentro:has(> .aderenti-pagina > :not(.solo-lettori)")),
  );
  assert.equal(cima.length, 3, "una riserva per uno, due e tre avvisi");
  assert.match(cima[0]?.dichiarazioni ?? "", /scroll-padding-top:\s*var\(--riserva-avviso\)/);
  assert.match(cima[1]?.dichiarazioni ?? "", /scroll-padding-top:\s*calc\(var\(--riserva-avviso\) \* 2\)/);
  const virtuale = readFileSync(new URL("../src/virtuale.ts", import.meta.url), "utf8");
  assert.match(virtuale, /getComputedStyle\(scorrevole\)\.scrollPaddingTop/);
});

test("le righe della coda in viaggio hanno la misura del testo che aspettano", () => {
  // R13b-5: al posto del titolo e dell'autore due barre finte, che senza una
  // regola loro erano due barre da 11 px attaccate.
  const tutte = regole(senzaCommenti(FOGLIO));
  const nome = tutte.find((r) => r.selettori.includes(".riga-coda .nome.riga-finta.skeleton"));
  const autore = tutte.find((r) => r.selettori.includes(".riga-coda .autore.riga-finta.skeleton"));
  assert.match(nome?.dichiarazioni ?? "", /margin-block:/);
  assert.match(autore?.dichiarazioni ?? "", /height:\s*9px/);
  const coda = readFileSync(new URL("../src/Coda.tsx", import.meta.url), "utf8");
  assert.match(coda, /<span className="nome riga-finta skeleton" \/>/);
  assert.match(coda, /<span className="autore riga-finta corta skeleton" \/>/);
});

test("«Ricontrolla» non sta attaccato al testo della nota (R18-11)", () => {
  const nota = regole(senzaCommenti(FOGLIO)).find((r) => r.selettori.includes(".colonna-albero > .nota"));
  assert.match(nota?.dichiarazioni ?? "", /display:\s*flex/);
  assert.match(nota?.dichiarazioni ?? "", /gap:\s*var\(--spazio-2\)/);
});
