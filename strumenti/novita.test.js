/*
 * Il controllo delle Novità: la versione che si rilascia ha le sue, e quelle che
 * ci sono sono scritte bene.
 *
 *   node --test "strumenti/novita.test.js"
 */
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const { controlla, passiDi, REPO } = require("./novita.js");

/** Un albero finto: la versione, `novita.ts` e i due file di lingua. */
function albero({ versione = "3.3.0", registro, it = {}, en = {} }) {
  const radice = fs.mkdtempSync(path.join(os.tmpdir(), "rymembra-novita-"));
  fs.mkdirSync(path.join(radice, "apps", "desktop", "src", "lingue"), { recursive: true });
  fs.writeFileSync(
    path.join(radice, "apps", "desktop", "package.json"),
    JSON.stringify({ version: versione }),
  );
  fs.writeFileSync(path.join(radice, "apps", "desktop", "src", "novita.ts"), registro);
  fs.writeFileSync(path.join(radice, "apps", "desktop", "src", "lingue", "it.json"), JSON.stringify(it));
  fs.writeFileSync(path.join(radice, "apps", "desktop", "src", "lingue", "en.json"), JSON.stringify(en));
  return radice;
}

/** Il registro di `novita.ts` con `n` passi per una versione. */
function registro(versione, n) {
  const passi = Array.from(
    { length: n },
    (_, i) => `    {
      illustrazione: "libreria",
      titoloKey: "whatsNew.${versione}.${i + 1}.title",
      testoKey: "whatsNew.${versione}.${i + 1}.text",
    },`,
  ).join("\n");
  return `export const NOVITA: Readonly<Record<string, readonly PassoNovita[]>> = {
  "${versione}": [
${passi}
  ],
};
`;
}

/** I testi di `n` passi, in una lingua. */
function testi(versione, n) {
  const fuori = {};
  for (let i = 1; i <= n; i++) {
    fuori[`whatsNew.${versione}.${i}.title`] = `Titolo ${i}`;
    fuori[`whatsNew.${versione}.${i}.text`] = `Testo ${i}`;
  }
  return fuori;
}

test("una versione con le sue novità in ordine non ha problemi né avvisi", () => {
  const radice = albero({ registro: registro("3.3.0", 3), it: testi("3.3.0", 3), en: testi("3.3.0", 3) });
  assert.deepEqual(controlla(radice), { problemi: [], avvisi: [] });
});

test("la versione che si rilascia senza novità è un avviso, non un errore", () => {
  const radice = albero({
    versione: "3.4.0",
    registro: registro("3.3.0", 3),
    it: testi("3.3.0", 3),
    en: testi("3.3.0", 3),
  });
  const { problemi, avvisi } = controlla(radice);
  assert.deepEqual(problemi, []);
  assert.equal(avvisi.length, 1);
  assert.match(avvisi[0], /3\.4\.0/);
  assert.match(avvisi[0], /novita\.ts/);
});

test("con --richiedi la versione senza novità è un problema", () => {
  const radice = albero({
    versione: "3.4.0",
    registro: registro("3.3.0", 3),
    it: testi("3.3.0", 3),
    en: testi("3.3.0", 3),
  });
  const { problemi } = controlla(radice, { richiedi: true });
  assert.equal(problemi.length, 1);
  assert.match(problemi[0], /3\.4\.0/);
});

test("da due a quattro passi", () => {
  for (const n of [1, 5]) {
    const radice = albero({ registro: registro("3.3.0", n), it: testi("3.3.0", n), en: testi("3.3.0", n) });
    const { problemi } = controlla(radice);
    assert.ok(
      problemi.some((p) => /da 2 a 4 passi/.test(p)),
      `${n} passi non segnalati: ${problemi.join(" | ")}`,
    );
  }
  for (const n of [2, 4]) {
    const radice = albero({ registro: registro("3.3.0", n), it: testi("3.3.0", n), en: testi("3.3.0", n) });
    assert.deepEqual(controlla(radice).problemi, []);
  }
});

test("un testo che manca in una lingua è un problema, e dice quale", () => {
  const en = testi("3.3.0", 2);
  delete en["whatsNew.3.3.0.2.text"];
  const radice = albero({ registro: registro("3.3.0", 2), it: testi("3.3.0", 2), en });
  const { problemi } = controlla(radice);
  assert.equal(problemi.length, 1);
  assert.match(problemi[0], /en/);
  assert.match(problemi[0], /whatsNew\.3\.3\.0\.2\.text/);
});

test("un testo vuoto vale come mancante", () => {
  const it = testi("3.3.0", 2);
  it["whatsNew.3.3.0.1.title"] = "  ";
  const radice = albero({ registro: registro("3.3.0", 2), it, en: testi("3.3.0", 2) });
  assert.equal(controlla(radice).problemi.length, 1);
});

test("le chiavi dei passi seguono whatsNew.<versione>.<n>.title|text, in ordine", () => {
  const storto = registro("3.3.0", 2).replace("whatsNew.3.3.0.2.title", "whatsNew.3.3.0.7.title");
  const radice = albero({
    registro: storto,
    it: { ...testi("3.3.0", 2), "whatsNew.3.3.0.7.title": "x" },
    en: { ...testi("3.3.0", 2), "whatsNew.3.3.0.7.title": "x" },
  });
  assert.ok(controlla(radice).problemi.some((p) => /whatsNew\.3\.3\.0\.7\.title/.test(p)));
});

test("le chiavi di una versione che nessun registro nomina sono un problema: restavano di una versione di prima", () => {
  const radice = albero({
    registro: registro("3.3.0", 2),
    it: { ...testi("3.3.0", 2), ...testi("3.2.0", 2) },
    en: { ...testi("3.3.0", 2), ...testi("3.2.0", 2) },
  });
  const { problemi } = controlla(radice);
  assert.ok(problemi.some((p) => /3\.2\.0/.test(p)), problemi.join(" | "));
});

test("i passi si leggono dal registro per versione", () => {
  assert.deepEqual(passiDi(registro("3.3.0", 2)), { "3.3.0": 2 });
});

test("il repository com'è: le novità che ci sono sono scritte bene", () => {
  const { problemi, avvisi } = controlla(REPO);
  assert.deepEqual(problemi, []);
  // Una versione senza novità è un avviso e non un errore (una correttiva non ne
  // ha): qui può esserci, ma non un avviso di un altro genere. Dopo un
  // `version:set` la prova non deve diventare rossa.
  for (const avviso of avvisi) assert.match(avviso, /non ha novità/);
});

/* ── S5-8: una minor o una major senza Novità ferma il tag ────────────── */

const { spawnSync } = require("node:child_process");

/** Lancia `novita.js` dentro un albero finto: lo script risolve `REPO` rispetto a sé stesso. */
function lancia(radice, ...argomenti) {
  for (const f of ["novita.js", "json.js"]) {
    fs.mkdirSync(path.join(radice, "strumenti"), { recursive: true });
    fs.copyFileSync(path.join(__dirname, f), path.join(radice, "strumenti", f));
  }
  const env = { ...process.env };
  delete env.GITHUB_ACTIONS;
  const esito = spawnSync(process.execPath, [path.join(radice, "strumenti", "novita.js"), ...argomenti], { env, encoding: "utf8" });
  return { codice: esito.status, uscita: `${esito.stdout}${esito.stderr}` };
}

/** Un albero con la versione `versione` e le novità solo della 3.3.0. */
function senzaNovita(versione) {
  return albero({ versione, registro: registro("3.3.0", 3), it: testi("3.3.0", 3), en: testi("3.3.0", 3) });
}

test("una versione con la patch a zero (minor o major) è una che deve portare le Novità", () => {
  const { richiedeLeNovita } = require("./novita.js");
  for (const v of ["3.3.0", "3.10.0", "4.0.0", "10.0.0"]) assert.equal(richiedeLeNovita(v), true, v);
  for (const v of ["3.3.1", "3.2.10", "4.0.7"]) assert.equal(richiedeLeNovita(v), false, v);
});

test("con --se-minor la minor senza novità ferma il tag, e la patch no (S5-8)", () => {
  // La 3.3.0 esce senza la finestra «Novità»: chi viene dalla 3.2.0 non la
  // vede, e non la vedrà mai (alla 3.3.1 la tabella dice «Niente»). Era un
  // avviso e la corsa andava avanti.
  const minor = lancia(senzaNovita("3.4.0"), "--se-minor");
  assert.equal(minor.codice, 1, minor.uscita);
  assert.match(minor.uscita, /3\.4\.0 non ha novità/);
  const major = lancia(senzaNovita("4.0.0"), "--se-minor");
  assert.equal(major.codice, 1, major.uscita);
  // Una correttiva non ne ha: resta libera, con l'avviso.
  const patch = lancia(senzaNovita("3.4.1"), "--se-minor");
  assert.equal(patch.codice, 0, patch.uscita);
  assert.match(patch.uscita, /Avviso: .*3\.4\.1 non ha novità/);
  // Una minor che le ha passa.
  const con = lancia(albero({ versione: "3.3.0", registro: registro("3.3.0", 3), it: testi("3.3.0", 3), en: testi("3.3.0", 3) }), "--se-minor");
  assert.equal(con.codice, 0, con.uscita);
});

test("senza opzioni una minor senza novità resta un avviso: `npm run verify` non si ferma a metà sviluppo", () => {
  const { codice, uscita } = lancia(senzaNovita("3.4.0"));
  assert.equal(codice, 0, uscita);
});
