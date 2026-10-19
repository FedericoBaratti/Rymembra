/*
 * La documentazione tecnica dice il vero sul repository.
 *
 * Il testo sta in `site/docs/contenuto/<capitolo>.js`, un file per capitolo,
 * caricati da `site/docs/index.html` nell'ordine in cui si leggono. Una pagina
 * a cui manca un capitolo si disegna lo stesso, e un percorso sbagliato in una
 * voce si legge benissimo: per questo si scoprono tardi. Qui si controlla che:
 *
 *   - ogni file di `contenuto/` sia caricato dalla pagina, e ogni file caricato
 *     esista;
 *   - capitoli e voci abbiano un `id` unico, un titolo e un corpo;
 *   - ogni blocco abbia una forma nota (paragrafo, `h`, `note`, `ul`, `ol`,
 *     `code`, `table`), e ogni riga di una tabella tante celle quante la testa;
 *   - ogni `**` e ogni apice inverso sia chiuso (fuori dai blocchi di codice);
 *   - ogni rimando — `see`, `[testo](#voce)`, e gli `href="#…"` della pagina —
 *     porti a una voce o a un capitolo che esiste;
 *   - ogni percorso in `src` esista nel repository: un file rinominato rompe
 *     questo controllo invece di lasciare una voce che indica il vuoto;
 *   - ogni ancora ammessa dal nucleo (`ANCORE_GUIDA` in `comandi.rs`) esista
 *     come voce: il «?» di una scheda non deve mai aprire una pagina senza il
 *     suo punto d'arrivo.
 *
 *   node strumenti/docs.js
 */
const fs = require("fs");
const path = require("path");
const vm = require("vm");

const REPO = path.dirname(__dirname);
const CARTELLA = path.join(REPO, "site", "docs");
const COMANDI = path.join(REPO, "apps/desktop/src-tauri/src/comandi.rs");

/** I file di contenuto che la pagina carica, nell'ordine in cui li carica. */
function caricati(pagina) {
  return [...pagina.matchAll(/<script src="\.\/(contenuto\/[^"]+\.js)"><\/script>/g)].map((m) => m[1]);
}

/** Carica i capitoli come lo farebbe il browser: stessi file, stesso ordine. */
function carica(cartella = CARTELLA) {
  const pagina = fs.readFileSync(path.join(cartella, "index.html"), "utf8");
  const elenco = caricati(pagina);
  const finestra = {};
  const contesto = vm.createContext({ window: finestra });
  const mancanti = [];
  for (const rel of elenco) {
    const file = path.join(cartella, rel);
    if (!fs.existsSync(file)) {
      mancanti.push(rel);
      continue;
    }
    vm.runInContext(fs.readFileSync(file, "utf8"), contesto, { filename: file });
  }
  const suDisco = fs.existsSync(path.join(cartella, "contenuto"))
    ? fs
        .readdirSync(path.join(cartella, "contenuto"))
        .filter((f) => f.endsWith(".js"))
        .map((f) => `contenuto/${f}`)
    : [];
  return {
    capitoli: finestra.RYMEMBRA_DOCS || [],
    pagina,
    mancanti,
    orfani: suDisco.filter((f) => !elenco.includes(f)),
  };
}

/** Le ancore ammesse dal nucleo, lette dal sorgente Rust. */
function ancoreDelNucleo(sorgente = fs.readFileSync(COMANDI, "utf8")) {
  const m = /const ANCORE_GUIDA: &\[&str\] = &\[([\s\S]*?)\];/.exec(sorgente);
  if (!m) return null;
  return [...m[1].matchAll(/"([^"]+)"/g)].map((x) => x[1]);
}

/** La forma di un blocco, o `null` se non è una forma nota. */
function forma(b) {
  if (typeof b === "string") return "p";
  if (b === null || typeof b !== "object") return null;
  const chiavi = Object.keys(b);
  if (chiavi.length !== 1) return null;
  const [k] = chiavi;
  if (k === "h" || k === "note" || k === "code") return typeof b[k] === "string" ? k : null;
  if (k === "ul" || k === "ol") return Array.isArray(b[k]) && b[k].length > 0 ? k : null;
  if (k === "table") {
    const t = b.table;
    const ok =
      t &&
      Array.isArray(t.head) &&
      t.head.length > 0 &&
      Array.isArray(t.rows) &&
      t.rows.length > 0 &&
      Object.keys(t).every((x) => x === "head" || x === "rows");
    return ok ? "table" : null;
  }
  return null;
}

/** Le stringhe di un blocco che passano dal marcatore in linea. */
function testiInLinea(b) {
  if (typeof b === "string") return [b];
  if (b.h !== undefined) return [b.h];
  if (b.note !== undefined) return [b.note];
  if (b.code !== undefined) return [];
  if (b.table !== undefined) return [...b.table.head, ...b.table.rows.flat()];
  return b.ol || b.ul || [];
}

function chiuso(testo) {
  return (testo.match(/\*\*/g) || []).length % 2 === 0 && (testo.match(/`/g) || []).length % 2 === 0;
}

/** I rimandi interni scritti dentro un testo: `[x](#id)`, fuori dal codice. */
function rimandi(testo) {
  const senzaCodice = testo.replace(/`[^`]*`/g, "");
  return [...senzaCodice.matchAll(/\]\(#([^)\s]+)\)/g)].map((m) => m[1]);
}

/**
 * Controlla i capitoli; ritorna l'elenco dei problemi.
 *
 * `esiste` dice se un percorso del repository c'è: si passa da fuori perché le
 * prove possano farne a meno del disco.
 */
function controlla(capitoli, ancore, esiste, pagina = "") {
  const problemi = [];
  const dica = (m) => problemi.push(m);

  const voci = new Set();
  const capitoliVisti = new Set();
  for (const c of capitoli) {
    if (!c || !c.id) {
      dica("un capitolo senza id");
      continue;
    }
    if (capitoliVisti.has(c.id)) dica(`capitolo «${c.id}» ripetuto`);
    capitoliVisti.add(c.id);
    for (const v of c.entries || []) {
      if (voci.has(v.id)) dica(`voce «${v.id}» ripetuta`);
      voci.add(v.id);
    }
  }
  const bersagli = new Set([...voci, ...[...capitoliVisti].map((id) => `c-${id}`)]);
  for (const id of voci) if (capitoliVisti.has(id)) dica(`«${id}» è sia un capitolo sia una voce`);

  for (const c of capitoli) {
    if (!c || !c.id) continue;
    if (!c.title) dica(`capitolo ${c.id} senza titolo`);
    if (!Array.isArray(c.entries) || c.entries.length === 0) dica(`capitolo ${c.id} senza voci`);
    if (c.lead !== undefined && !chiuso(c.lead)) dica(`capitolo ${c.id}: lead con grassetto o codice non chiuso`);
    for (const v of c.entries || []) {
      const nome = v.id || "(senza id)";
      if (!v.id || !/^[a-z0-9-]+$/.test(v.id)) dica(`${nome}: l'id deve essere minuscolo, cifre e trattini`);
      if (!v.title) dica(`${nome}: titolo vuoto`);
      if (!Array.isArray(v.src)) dica(`${nome}: manca "src"`);
      else {
        if (v.src.length === 0) dica(`${nome}: "src" vuoto`);
        for (const p of v.src) {
          if (typeof p !== "string" || p === "" || path.isAbsolute(p) || p.includes("\\")) {
            dica(`${nome}: percorso «${p}» non relativo alla radice del repository`);
          } else if (!esiste(p)) dica(`${nome}: «${p}» non esiste nel repository`);
        }
      }
      if (!Array.isArray(v.body) || v.body.length === 0) {
        dica(`${nome}: corpo vuoto`);
        continue;
      }
      for (const s of v.see || []) if (!voci.has(s)) dica(`${nome}: rimanda a «${s}», che non esiste`);
      for (const b of v.body) {
        const f = forma(b);
        if (f === null) {
          dica(`${nome}: blocco di forma sconosciuta ${JSON.stringify(b).slice(0, 60)}`);
          continue;
        }
        if (f === "code" && b.code.trim() === "") dica(`${nome}: blocco di codice vuoto`);
        if (f === "table") {
          b.table.rows.forEach((r, i) => {
            if (!Array.isArray(r) || r.length !== b.table.head.length) {
              dica(`${nome}: la riga ${i + 1} della tabella non ha ${b.table.head.length} celle`);
            }
          });
        }
        for (const t of testiInLinea(b)) {
          if (typeof t !== "string" || t.trim() === "") dica(`${nome}: testo vuoto`);
          else {
            if (!chiuso(t)) dica(`${nome}: grassetto o codice non chiuso in «${t.slice(0, 40)}…»`);
            for (const r of rimandi(t)) if (!bersagli.has(r)) dica(`${nome}: «#${r}» non porta a niente`);
          }
        }
      }
    }
  }

  for (const m of pagina.matchAll(/href="#([^"]+)"/g)) {
    if (!bersagli.has(m[1])) dica(`index.html: «#${m[1]}» non porta a niente`);
  }
  if (ancore) {
    for (const x of ancore) if (!voci.has(x)) dica(`manca la voce «${x}» ammessa da ANCORE_GUIDA`);
  }
  return problemi;
}

function main() {
  const { capitoli, pagina, mancanti, orfani } = carica();
  const ancore = ancoreDelNucleo();
  const problemi = [];
  for (const f of mancanti) problemi.push(`index.html carica ${f}, che non esiste`);
  for (const f of orfani) problemi.push(`${f} non è caricato da index.html`);
  if (!ancore || ancore.length === 0) problemi.push("non trovo ANCORE_GUIDA in comandi.rs");
  problemi.push(...controlla(capitoli, ancore, (p) => fs.existsSync(path.join(REPO, p)), pagina));
  if (problemi.length) {
    console.error(`docs: ${problemi.length} problemi\n` + problemi.map((p) => "  - " + p).join("\n"));
    process.exit(1);
  }
  const voci = capitoli.reduce((n, c) => n + c.entries.length, 0);
  console.log(`docs: ${capitoli.length} capitoli, ${voci} voci, ${ancore.length} ancore dell'app: tutto torna.`);
}

if (require.main === module) main();

module.exports = { carica, caricati, ancoreDelNucleo, controlla, forma, chiuso, rimandi };
