/*
 * Il sito si costruisce da un sorgente solo, e le sue pagine tornano.
 *
 *   node --test "strumenti/*.test.js"
 */
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const {
  esc,
  inline,
  piano,
  riempi,
  confrontaLingue,
  nonUsate,
  dataNelChangelog,
  dataInLingua,
  pesoInLingua,
  percorsoCapitolo,
  mappaDocs,
  controllaUscita,
  argomenti,
  costruisci,
} = require("./sito.js");

test("il marcatore in linea diventa HTML, e il resto si escapa", () => {
  assert.equal(inline("**a** *b* `c` [d](https://e.f)"), '<strong>a</strong> <em>b</em> <code>c</code> <a href="https://e.f">d</a>');
  assert.equal(inline('<b> & "x"'), "&lt;b&gt; &amp; &quot;x&quot;");
  // Dentro il codice niente marcatori, e niente HTML.
  assert.equal(inline("`**<x>**`"), "<code>**&lt;x&gt;**</code>");
  assert.equal(inline("[docs](../docs/audio/)"), '<a href="../docs/audio/">docs</a>');
  assert.throws(() => inline("[x](javascript:alert)"), /non ammesso/);
});

test("il testo semplice perde i marcatori", () => {
  assert.equal(piano("**a** e [b](https://c.d) e `e`"), "a e b e e");
  assert.equal(esc(piano('«a» "b"')), "«a» &quot;b&quot;");
});

test("il modello riempie testi, elenchi e valori del generatore", () => {
  const problemi = [];
  const usate = new Set();
  const fuori = riempi(
    '<h1>{{a.t}}</h1><p title="{{=a.t}}">{{a.v}}</p>{{#l}}<i>{{@n}} {{.x}}</i>{{/l}}{{&$h}}',
    { a: { t: "**T**", v: "versione {versione}" }, l: [{ x: "uno" }, { x: "due" }] },
    { $versione: "1.2.3", $h: "<br>" },
    problemi,
    usate,
  );
  assert.deepEqual(problemi, []);
  assert.equal(fuori, '<h1><strong>T</strong></h1><p title="T">versione 1.2.3</p><i>1 uno</i><i>2 due</i><br>');
  assert.deepEqual([...usate].sort(), ["a.t", "a.v", "l", "l[].x"]);
});

test("una chiave che manca è un problema, non una stringa vuota in silenzio", () => {
  const problemi = [];
  riempi("{{manca}} {{#no}}x{{/no}} {{a}} {{&a}}", { a: "{ignoto}" }, {}, problemi);
  assert.equal(problemi.length, 4);
  assert.ok(problemi.some((p) => /manca/.test(p)));
  assert.ok(problemi.some((p) => /ignoto/.test(p)));
  assert.ok(problemi.some((p) => /HTML grezzo/.test(p)));
});

test("le lingue devono dire le stesse cose", () => {
  const en = { a: "x", l: [{ d: "1", r: "2" }] };
  assert.deepEqual(confrontaLingue({ en, it: { a: "y", l: [{ d: "1", r: "2" }] } }), []);
  const p = confrontaLingue({ en, it: { l: [{ d: "1" }, { d: "2" }] }, de: { a: "x", b: "in più", l: [{ d: "1", r: "2" }] } });
  assert.ok(p.some((x) => /it: manca «a»/.test(x)));
  assert.ok(p.some((x) => /it: «l\[\]» ha 2 elementi/.test(x)));
  assert.ok(p.some((x) => /de: «b» non c'è/.test(x)));
});

test("un testo che nessuno mostra si vede", () => {
  const testi = { a: "x", b: "y", l: [{ d: "1", r: "2" }], s: ["p"] };
  assert.deepEqual(nonUsate(testi, new Set(["a", "l[].d", "s"])), ["b", "l[].r"]);
});

test("la data viene dal changelog, e si scrive nella lingua della pagina", () => {
  const c = "# Changelog\n\n## [3.0.1] — 2026-10-15\n\ntesto\n\n## [3.0.0] — 2026-10-13\n";
  assert.equal(dataNelChangelog(c, "3.0.1"), "2026-10-15");
  assert.equal(dataNelChangelog(c, "3.0.0"), "2026-10-13");
  assert.equal(dataNelChangelog(c, "3.0.2"), null);
  // Il punto non vale «qualunque carattere».
  assert.equal(dataNelChangelog("## [3x0x1] — 2026-01-01", "3.0.1"), null);
  assert.equal(dataInLingua("2026-10-15", "it_IT"), "15 ottobre 2026");
  assert.equal(dataInLingua("2026-10-15", "de_DE"), "15. Oktober 2026");
});

test("le voci delle docs sanno in che pagina stanno", () => {
  assert.equal(percorsoCapitolo("data-chapter"), "data");
  assert.equal(percorsoCapitolo("library"), "library");
  const m = mappaDocs([{ id: "library", entries: [{ id: "roots" }] }, { id: "studio-chapter", entries: [{ id: "studio" }] }]);
  assert.deepEqual(m, { "c-library": "library", roots: "library", "c-studio-chapter": "studio", studio: "studio" });
});

test("la versione arriva dal tag, con o senza la v", () => {
  assert.deepEqual(argomenti(["--versione", "v3.0.1"]), { controlla: false, versione: "3.0.1", installer: null });
  assert.deepEqual(argomenti(["--controlla"]), { controlla: true, versione: null, installer: null });
  assert.deepEqual(argomenti(["--versione", "3.0.1", "--installer", "cartella"]), { controlla: false, versione: "3.0.1", installer: "cartella" });
  assert.throws(() => argomenti(["--boh"]));
});

test("il peso si scrive come in Esplora risorse, nei numeri della lingua", () => {
  assert.equal(pesoInLingua(10066329, "it_IT"), "9,6");
  assert.equal(pesoInLingua(10066329, "en_US"), "9.6");
  assert.equal(pesoInLingua(1048576, "de_DE"), "1,0");
});

test("i collegamenti rotti, le risorse esterne e le ancore perse si vedono", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rymembra-uscita-"));
  try {
    const scrivi = (rel, s) => {
      fs.mkdirSync(path.dirname(path.join(dir, rel)), { recursive: true });
      fs.writeFileSync(path.join(dir, rel), s);
    };
    scrivi("index.html", '<a href="docs/">d</a><a href="#su" id="su">s</a><a href="docs/a/#voce">v</a><a href="manca/">m</a><a href="#via">x</a><div data-src="audio/manca.mp3"></div>');
    scrivi("docs/index.html", '<a href="../">r</a>');
    scrivi("docs/a/index.html", '<article id="voce"></article><script src="https://cdn.example/x.js"></script>');
    scrivi("404.html", '<a href="/">ok</a><a href="docs/">relativo</a>');
    scrivi("stile.css", "a { b: url(font/x.woff2) } c {");
    const p = controllaUscita(dir, { voce: "a", altra: "b" }, ["voce", "altra"]);
    assert.ok(p.some((x) => /manca\/index\.html/.test(x)));
    assert.ok(p.some((x) => /«#via»/.test(x)));
    assert.ok(p.some((x) => /cdn\.example/.test(x)));
    assert.ok(p.some((x) => /404\.html: «docs\/»/.test(x)));
    assert.ok(p.some((x) => /stile\.css: «font\/x\.woff2» non esiste/.test(x)));
    assert.ok(p.some((x) => /stile\.css: le graffe/.test(x)));
    assert.ok(p.some((x) => /«#altra» non arriva/.test(x)));
    assert.ok(p.some((x) => x.includes("«audio/manca.mp3»")));
    assert.equal(p.length, 8);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("il sito vero si costruisce e torna", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rymembra-sito-prova-"));
  try {
    const versione = JSON.parse(fs.readFileSync(path.join(__dirname, "../apps/desktop/package.json"), "utf8")).version;
    const esito = costruisci(dir, versione);
    assert.deepEqual(esito.problemi, []);
    for (const f of ["index.html", "it/index.html", "fr/index.html", "de/index.html", "docs/index.html", "docs/library/index.html", "sitemap.xml", "404.html", "audio/machina.mp3", "audio/passaggio.mp3"]) {
      assert.ok(fs.existsSync(path.join(dir, f)), f);
    }
    const it = fs.readFileSync(path.join(dir, "it/index.html"), "utf8");
    assert.match(it, /<html lang="it">/);
    assert.match(it, /hreflang="x-default" href="https:\/\/rymembra\.com\/"/);
    assert.ok(it.includes(`/releases/download/v${versione}/Rymembra_${versione}_x64-setup.exe`));
    // La colonna sonora si scarica solo quando la si chiede, e ha i suoi crediti.
    assert.ok(it.includes('<audio data-colonna preload="none" loop src="../audio/machina.mp3">'));
    assert.ok(it.includes("https://creativecommons.org/licenses/by/4.0/"));
    // Senza l'installer, niente peso e niente impronta: non si inventano.
    assert.ok(!it.includes('class="impronta"'));
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("con l'installer la pagina ne dice peso e impronta, misurati sul file", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rymembra-sito-impronta-"));
  const installer = fs.mkdtempSync(path.join(os.tmpdir(), "rymembra-installer-"));
  try {
    const versione = JSON.parse(fs.readFileSync(path.join(__dirname, "../apps/desktop/package.json"), "utf8")).version;
    const dati = Buffer.alloc(3 * 1048576 + 400000, 7);
    fs.writeFileSync(path.join(installer, `Rymembra_${versione}_x64-setup.exe`), dati);
    const esito = costruisci(dir, versione, installer);
    assert.deepEqual(esito.problemi, []);
    const impronta = require("node:crypto").createHash("sha256").update(dati).digest("hex").toUpperCase();
    const it = fs.readFileSync(path.join(dir, "it/index.html"), "utf8");
    assert.ok(it.includes(`<code>${impronta}</code>`));
    assert.ok(it.includes("3,4 MB"));
    assert.ok(fs.readFileSync(path.join(dir, "fr/index.html"), "utf8").includes("3,4 Mo"));
    // Una cartella senza quel file è un errore, non una riga che sparisce in silenzio.
    assert.throws(() => costruisci(dir, versione, os.tmpdir()), /non c'è/);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
    fs.rmSync(installer, { recursive: true, force: true });
  }
});
