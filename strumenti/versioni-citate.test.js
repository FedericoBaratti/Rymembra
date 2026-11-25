/*
 * I testi pubblici non citano una versione che non è mai uscita (S10-1).
 *
 * # Perché esiste
 *
 * La 3.2.1 era la versione interna su cui si lavorava, e non è mai stata
 * pubblicata: chi aggiorna arriva dalla 3.2.0 e trova la 3.3.0. Una frase come
 * «ogni dispositivo deve essere sulla 3.2.1» o «QR (3.2.1 o successiva)» è
 * vera solo per chi ha visto quella versione, cioè per nessuno, e nessun
 * controllo se ne accorgeva: `docs.js` guarda i percorsi e i rimandi, non i
 * numeri.
 *
 * # Cosa controlla
 *
 * Ogni numero `X.Y.Z` con X uguale a 2 o a 3 — le due righe di Rymembra che hanno
 * ancora lettori — in README, PRIVACY, SECURITY, TERMS, nelle pagine del sito
 * (`site/testi`, `site/docs/contenuto`) e nella sezione più recente del
 * CHANGELOG deve avere la sua intestazione `## [X.Y.Z]` nel CHANGELOG: è
 * l'unico elenco delle versioni uscite.
 *
 * **La versione di `package.json` non basta come lasciapassare**: finché la
 * 3.2.1 era il numero del codice, un testo che la citava passava da sé, che
 * era proprio il difetto. Vale solo quel che ha un'intestazione.
 *
 * # Cosa non guarda
 *
 * - I numeri di una libreria o di una norma. Non li prende il lookbehind (`tauri-plugin-updater/2.13.0`,
 *   `ndk-context-0.1.1`: li precede un `/` o un `-`) né due eccezioni dichiarate qui sotto, che non
 *   dipendono dal numero: così un aggiornamento di dipendenza non rompe la prova. Un numero nuovo di
 *   questo tipo si aggiunge a `ESTRANEI`, con la ragione.
 * - Le versioni precedenti alla 2.0.0: sono le righe vecchie, e i numeri di libreria
 *   cominciano per 0 e per 1.
 *
 * Con `RYMEMBRA_RADICE=<cartella>` si prova un altro albero (per vedere la prova
 * fallire su com'era prima).
 */
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const REPO = process.env.RYMEMBRA_RADICE || path.dirname(__dirname);

/** I testi pubblici, relativi alla radice. */
function fileDaControllare() {
  const elenco = ["README.md", "PRIVACY.md", "SECURITY.md", "TERMS.md"];
  for (const cartella of ["site/testi", "site/docs/contenuto"]) {
    const dir = path.join(REPO, cartella);
    if (!fs.existsSync(dir)) continue;
    for (const f of fs.readdirSync(dir).sort()) {
      if (/\.(json|js)$/.test(f)) elenco.push(`${cartella}/${f}`);
    }
  }
  return elenco;
}

/**
 * Numeri che non sono una versione di Rymembra ma passano il lookbehind: cosa li
 * precede, scritto in modo che non dipenda dal numero.
 */
const ESTRANEI = [
  // La norma di accessibilità: «WCAG 2.5.3».
  /WCAG\s*$/,
  // Il pavimento di SQLite, nelle pagine del sito: «SQLite version floor (3.53.2)».
  /SQLite[^\n]{0,60}$/,
];

/** Le versioni uscite e la sezione più recente del CHANGELOG. */
function changelog() {
  const testo = fs.readFileSync(path.join(REPO, "CHANGELOG.md"), "utf8");
  const uscite = new Set([...testo.matchAll(/^## \[(\d+\.\d+\.\d+)\]/gm)].map((m) => m[1]));
  const righe = testo.split("\n");
  const inizio = righe.findIndex((r) => r.startsWith("## ["));
  let fine = righe.findIndex((r, i) => i > inizio && r.startsWith("## ["));
  if (fine < 0) fine = righe.length;
  return { uscite, recente: righe.slice(inizio, fine).join("\n"), intestazione: righe[inizio] };
}

/** I numeri da 2.x.y e 3.x.y di un testo, con la riga in cui stanno. */
function numeriCitati(testo) {
  const trovati = [];
  const numero = /(?<![\w.\-/])([23]\.\d+\.\d+)(?![\w]|\.\d)/g;
  for (const m of testo.matchAll(numero)) {
    const prima = testo.slice(Math.max(0, m.index - 80), m.index);
    if (ESTRANEI.some((r) => r.test(prima))) continue;
    const riga = testo.slice(0, m.index).split("\n").length;
    trovati.push({ numero: m[1], riga });
  }
  return trovati;
}

/** I numeri citati che nessuna intestazione del CHANGELOG nomina. */
function nonUsciti(testo, uscite) {
  return numeriCitati(testo).filter((c) => !uscite.has(c.numero));
}

test("la prova guarda davvero: un numero senza intestazione si trova, uno con l'intestazione no", () => {
  const uscite = new Set(["3.2.0", "3.3.0"]);
  const trovati = nonUsciti("da 3.2.0 in poi\nma non la 3.2.1, né tauri-plugin-updater/2.13.0\nWCAG 2.5.3, SQLite floor (3.53.2)", uscite);
  assert.deepEqual(trovati, [{ numero: "3.2.1", riga: 2 }]);
});

test("i testi pubblici citano solo versioni uscite: ognuna ha la sua intestazione nel CHANGELOG", () => {
  const { uscite, recente, intestazione } = changelog();
  assert.ok(uscite.size > 0, "il CHANGELOG non ha nessuna intestazione `## [X.Y.Z]`");
  assert.ok(
    /^## \[\d+\.\d+\.\d+\]/.test(intestazione),
    `la sezione più recente del CHANGELOG è «${intestazione}»: una sola, con la sua versione, senza [Unreleased]`,
  );
  const problemi = [];
  for (const f of fileDaControllare()) {
    const testo = fs.readFileSync(path.join(REPO, f), "utf8");
    for (const c of nonUsciti(testo, uscite)) {
      problemi.push(`${f}:${c.riga}: cita la ${c.numero}, che non ha un'intestazione nel CHANGELOG`);
    }
  }
  for (const c of nonUsciti(recente, uscite)) {
    problemi.push(`CHANGELOG.md, sezione più recente, riga ${c.riga}: cita la ${c.numero}, che non ha un'intestazione`);
  }
  assert.deepEqual(problemi, [], "versioni mai uscite nei testi pubblici:\n  " + problemi.join("\n  "));
});
