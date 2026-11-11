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
const { leggiJson } = require("./json.js");

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

// R27-4: i segnaposto e i collegamenti di un testo sono gli stessi in ogni
// lingua. Un `de.json` che perde `[PRIVACY.md]({privacy})` passava.
test("i segnaposto e i collegamenti sono gli stessi in ogni lingua", () => {
  const en = { a: "See [PRIVACY.md]({privacy}), version {versione}.", b: "[x](#su)" };
  assert.deepEqual(confrontaLingue({ en, it: { a: "Vedi [PRIVACY.md]({privacy}), versione {versione}.", b: "[y](#su)" } }), []);
  const p = confrontaLingue({ en, de: { a: "Siehe PRIVACY.md, Version {versione}.", b: "[y](#unten)" } });
  assert.ok(p.some((x) => /de: «a»/.test(x) && x.includes("{privacy}")), p.join("\n"));
  assert.ok(p.some((x) => /de: «b»/.test(x) && x.includes("#su")), p.join("\n"));
});

// R27-4: `[x](//docs/audio/)` scritto per sbaglio è un link a un host di nome «docs».
test("un collegamento senza schema non passa", () => {
  assert.throws(() => inline("[x](//a.b/c)"), /non ammesso/);
  assert.throws(() => inline("[x](//docs/audio/)"), /non ammesso/);
  assert.equal(inline("[x](docs/audio/)"), '<a href="docs/audio/">x</a>');
});

// R27-4: JSON.parse tiene l'ultima di due chiavi uguali senza dirlo.
test("una chiave ripetuta in un JSON di testi è un errore", () => {
  assert.deepEqual(leggiJson('{"a":"x","b":{"c":1,"d":[{"e":1},{"e":2}]}}', "t.json"), { a: "x", b: { c: 1, d: [{ e: 1 }, { e: 2 }] } });
  assert.throws(() => leggiJson('{"a":"x","a":"y"}', "t.json"), /t\.json.*«a»/);
  // Una stringa che contiene una chiave fra virgolette non è una chiave.
  assert.throws(() => leggiJson('{"s":{"b":1,"c":"\\"b\\"","b":2}}', "t.json"), /«s\.b»/);
  // Una chiave uguale in due oggetti diversi non è una ripetizione.
  assert.doesNotThrow(() => leggiJson('{"x":{"a":1},"y":{"a":2},"l":[{"a":1},{"a":2}]}', "t.json"));
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
    // La 404 lavora con una `<base>`: un percorso dalla radice la scavalca, e
    // sotto `/Rymembra/` porterebbe fuori dal sito.
    scrivi("404.html", '<base href="/"><a href="./">ok</a><a href="docs/">relativo</a><a href="/docs/">dalla radice</a>');
    scrivi("stile.css", "a { b: url(font/x.woff2) } c {");
    const p = controllaUscita(dir, { voce: "a", altra: "b" }, ["voce", "altra"]);
    assert.ok(p.some((x) => /manca\/index\.html/.test(x)));
    assert.ok(p.some((x) => /«#via»/.test(x)));
    assert.ok(p.some((x) => /cdn\.example/.test(x)));
    assert.ok(p.some((x) => /404\.html: «\/docs\/»/.test(x)), p.join("\n"));
    assert.ok(p.some((x) => /stile\.css: «font\/x\.woff2» non esiste/.test(x)));
    assert.ok(p.some((x) => /stile\.css: le graffe/.test(x)));
    assert.ok(p.some((x) => /«#altra» non arriva/.test(x)));
    assert.ok(p.some((x) => x.includes("«audio/manca.mp3»")));
    assert.equal(p.length, 8);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

// R27-4: il controllo delle risorse esterne guardava solo `script|img|source|iframe`
// e i `<link>`; non i `.js` copiati, non `@import`, non l'indice della ricerca.
test("ogni risorsa da un altro dominio si vede, e l'indice della ricerca deve esistere", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rymembra-uscita-esterne-"));
  try {
    const scrivi = (rel, s) => {
      fs.mkdirSync(path.dirname(path.join(dir, rel)), { recursive: true });
      fs.writeFileSync(path.join(dir, rel), s);
    };
    scrivi(
      "index.html",
      '<audio src="https://cdn.example/a.mp3"></audio><video poster="//cdn.example/p.jpg"></video>' +
        '<img srcset="immagine.webp 1x, https://cdn.example/b.webp 2x" src="immagine.webp" alt="">' +
        '<object data="https://cdn.example/c.svg"></object><a href="https://github.com/x">un clic va bene</a>',
    );
    scrivi("immagine.webp", "x");
    scrivi("docs/index.html", '<form data-ricerca="manca.json"></form><form data-ricerca="c.json"></form>');
    scrivi("docs/c.json", "[]");
    scrivi("stile.css", '@import "https://cdn.example/d.css";\na { b: c }');
    scrivi("sito.js", 'fetch("https://cdn.example/e.json"); var a = new Audio(\'//cdn.example/f.mp3\');');
    const p = controllaUscita(dir, {}, []);
    const tutti = p.join("\n");
    for (const atteso of ["a.mp3", "p.jpg", "b.webp", "c.svg", "d.css", "e.json", "f.mp3"]) {
      assert.ok(p.some((x) => x.includes(`cdn.example/${atteso}`)), `${atteso}: ${tutti}`);
    }
    assert.ok(p.some((x) => x.includes("«manca.json»")), tutti);
    assert.ok(!p.some((x) => /github\.com|c\.json|immagine\.webp 1x/.test(x)), tutti);
    assert.equal(p.length, 8, tutti);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

// R27-1, la parte tecnica: Pages serve la 404 a qualunque indirizzo che non
// esiste, alla radice di un dominio personalizzato o sotto `/Rymembra/` senza.
// Con percorsi dalla radice, sotto `/Rymembra/` usciva senza stile e «Go to the
// home page» portava a `federicobaratti.github.io/`, un altro sito.
test("la pagina 404 funziona alla radice e sotto /Rymembra/", () => {
  const vm = require("node:vm");
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rymembra-sito-404-"));
  try {
    const versione = JSON.parse(fs.readFileSync(path.join(__dirname, "../apps/desktop/package.json"), "utf8")).version;
    assert.deepEqual(costruisci(dir, versione).problemi, []);
    const pagina = fs.readFileSync(path.join(dir, "404.html"), "utf8");
    // Nessun indirizzo dalla radice: tutto passa dalla `<base>`.
    assert.match(pagina, /<base href="\/">/);
    const indirizzi = [...pagina.replace('<base href="/">', "").matchAll(/\s(?:href|src)="([^"]*)"/g)].map((m) => m[1]);
    assert.ok(indirizzi.length > 5);
    assert.deepEqual(indirizzi.filter((u) => u.startsWith("/")), []);
    const script = /<script>([\s\S]*?)<\/script>/.exec(pagina);
    assert.ok(script, "manca lo script che sceglie la base");
    assert.ok(pagina.indexOf("<script>") < pagina.indexOf('rel="stylesheet"'), "la base va scelta prima dei fogli di stile");
    const base = (percorso) => {
      const elemento = { href: "/" };
      vm.runInNewContext(script[1], {
        location: { pathname: percorso },
        document: { querySelector: (s) => (s === "base" ? elemento : null) },
      });
      return elemento.href;
    };
    assert.equal(base("/qualcosa/che/manca"), "/");
    assert.equal(base("/Rymembra/qualcosa/che/manca"), "/Rymembra/");
    assert.equal(base("/rymembra/docs/vecchia/"), "/rymembra/");
    assert.equal(base("/Rymembrax/manca"), "/");
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
    // La scheda del Store, in tutte le lingue: un collegamento sotto il pulsante
    // in alto e una seconda riga nella sezione «Scarica», e basta.
    for (const f of ["index.html", "it/index.html", "fr/index.html", "de/index.html"]) {
      const pagina = fs.readFileSync(path.join(dir, f), "utf8");
      assert.equal(pagina.split('href="https://apps.microsoft.com/detail/9NW2F2PW2B91"').length - 1, 2, f);
    }
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
