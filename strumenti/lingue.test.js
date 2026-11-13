/*
 * Le forme del plurale, come le guarda `lingue.js`.
 *
 * Il resto dello strumento confronta chiavi, e si prova da sé girando su
 * `it.json` e `en.json` in `npm run verify`. Le forme del plurale no: le lingue
 * con più forme dell'italiano non ci sono ancora, e il giorno in cui arriva
 * `ru.json` è il giorno sbagliato per scoprire che il controllo le scambia per
 * chiavi avanzate.
 *
 *   node --test "strumenti/*.test.js"
 */
const test = require("node:test");
const assert = require("node:assert/strict");

const { controllaPlurali, semplici, unoEsatto, variante } = require("./lingue.js");

const IT = {
  "format.tracks": "{n} brani",
  "format.tracks.uno": "1 brano",
  "folders.offline": "{n} cartelle non rispondono.",
  "folders.offline.uno": "Una cartella non risponde.",
  "artists.index.other": "Altri",
  "nav.albums": "Album",
};

test("una forma è tale solo se la sua base esiste", () => {
  const base = (b) => b in IT;
  assert.deepEqual(variante("format.tracks.uno", base), { base: "format.tracks", forma: "uno" });
  // L'etichetta della voce «#» dell'indice: finisce con `other`, ma non è un plurale.
  assert.equal(variante("artists.index.other", base), null);
  assert.deepEqual(semplici(IT, IT).sort(), [
    "artists.index.other",
    "folders.offline",
    "format.tracks",
    "nav.albums",
  ]);
});

test("in italiano e in inglese `one` è proprio uno; in russo e in francese no", () => {
  assert.equal(unoEsatto("it"), true);
  assert.equal(unoEsatto("en"), true);
  assert.equal(unoEsatto("ru"), false);
  assert.equal(unoEsatto("fr"), false);
  assert.equal(unoEsatto("ja"), false);
});

test("l'inglese con le stesse forme dell'italiano è completo", () => {
  const EN = {
    "format.tracks": "{n} tracks",
    "format.tracks.uno": "1 track",
    "folders.offline": "{n} folders are not answering.",
    "folders.offline.one": "A folder is not answering.",
    "artists.index.other": "Other",
    "nav.albums": "Albums",
  };
  const esito = controllaPlurali("en", EN, IT);
  assert.deepEqual(esito.mancanti, []);
  assert.deepEqual(esito.errori, []);
  // `.one` al posto di `.uno` vale lo stesso in inglese, e non è una chiave in più.
  assert.ok(esito.ammesse.has("folders.offline.one"));
});

test("l'inglese senza il singolare manca di una forma", () => {
  const EN = {
    "format.tracks": "{n} tracks",
    "folders.offline": "{n} folders",
    "folders.offline.uno": "A folder",
    "artists.index.other": "Other",
    "nav.albums": "Albums",
  };
  const esito = controllaPlurali("en", EN, IT);
  assert.deepEqual(esito.mancanti, ["format.tracks.uno"]);
});

test("una forma che la lingua non ha è un errore", () => {
  const conFew = { ...IT, "format.tracks.few": "{n} brani (pochi)" };
  const esito = controllaPlurali("it", conFew, conFew);
  assert.equal(esito.errori.length, 1);
  assert.match(esito.errori[0], /format\.tracks\.few/);
});

test("il giapponese non deve scrivere il singolare", () => {
  const JA = {
    "format.tracks": "{n}曲",
    "folders.offline": "{n}個のフォルダ",
    "artists.index.other": "その他",
    "nav.albums": "アルバム",
  };
  const esito = controllaPlurali("ja", JA, IT);
  assert.deepEqual(esito.mancanti, []);
  assert.deepEqual(esito.errori, []);
});

test("il russo: le forme in più sono ammesse, e usarne una la chiede dappertutto", () => {
  const RU = {
    "format.tracks": "{n} трека",
    "format.tracks.uno": "1 трек",
    "format.tracks.one": "{n} трек",
    "format.tracks.few": "{n} трека",
    "format.tracks.many": "{n} треков",
    "folders.offline": "{n} папки",
    "folders.offline.uno": "Одна папка",
    "folders.offline.one": "{n} папка",
    "artists.index.other": "Другие",
    "nav.albums": "Альбомы",
  };
  const esito = controllaPlurali("ru", RU, IT);
  assert.deepEqual(esito.mancanti, []);
  for (const k of ["format.tracks.one", "format.tracks.few", "format.tracks.many"]) {
    assert.ok(esito.ammesse.has(k), k);
  }
  // `few` e `many` scritti per i brani e non per le cartelle.
  assert.equal(esito.errori.length, 2);
  assert.ok(esito.errori.every((e) => e.includes("folders.offline")));
});

test("le categorie mai scritte sono il ripiego dichiarato, e si dicono", () => {
  const esito = controllaPlurali("it", IT, IT);
  assert.deepEqual(esito.errori, []);
  assert.equal(esito.note.length, 1);
  assert.match(esito.note[0], /many/);
});

/*
 * I testi, non solo le chiavi (R17-24).
 *
 * Ogni controllo è una funzione pura: qui la si prova con tre righe di fixture,
 * al rosso e al verde. Sull'albero vero girano in `npm run check:lingue`.
 */
const {
  chiaviMorte,
  contatoriFuoriDaN,
  etichetteCitate,
  grafiaBritannica,
  segnaposti,
  segnapostiDiversi,
  valoriUguali,
  virgoletteSbagliate,
  vociDeiRegistri,
  vociMancanti,
} = require("./lingue.js");

test("i segnaposto sono gli stessi in ogni lingua", () => {
  const dizionari = {
    it: { a: "{nome} ha {n} brani", b: "ciao" },
    en: { a: "{nome} has {n} tracks", b: "hi" },
    de: { a: "{name} hat {n} Titel", b: "hallo" },
  };
  const trovati = segnapostiDiversi(dizionari);
  assert.equal(trovati.length, 1);
  assert.match(trovati[0], /de\.json: «a» ha \{n, name\} dove it\.json ha \{n, nome\}/);
  assert.deepEqual(segnapostiDiversi({ it: dizionari.it, en: dizionari.en }), []);
  assert.deepEqual(segnaposti("{b} {a} {b}"), ["a", "b"]);
});

test("le virgolette e gli apostrofi sono quelli della lingua", () => {
  // L'inglese con le «» dell'italiano e l'apostrofo dritto: due difetti, una chiave.
  const en = { "a.b": "Turn on «Listen together»? It isn't on.", ok: "Turn on “Listen together”? It isn’t on." };
  const trovati = virgoletteSbagliate("en", en);
  assert.equal(trovati.length, 2);
  assert.ok(trovati.every((t) => t.includes("«a.b»")));
  // Le virgolette dritte sono quelle del codice citato, e passano; i `.syn` anche.
  assert.deepEqual(virgoletteSbagliate("en", { c: "Written as {\"$token\": …}", "x.syn": "don't" }), []);
  // E l'italiano con le “” dell'inglese.
  assert.equal(virgoletteSbagliate("it", { a: "Accendi “Ascolta insieme”" }).length, 1);
  assert.deepEqual(virgoletteSbagliate("it", { a: "Accendi «Ascolta insieme» dell'altro PC" }), []);
  // Un'eccezione dichiarata (con il suo motivo, nel file accanto) passa.
  assert.deepEqual(virgoletteSbagliate("en", { a: "x's" }, { apostrofiDritti: { a: "motivo" } }), []);
});

test("l'inglese ha una grafia sola: l'americano", () => {
  const en = {
    a: "Ask the catalogue for your favourite colour",
    b: "We recognise it; the licence is the analyser's",
    "c.syn": "visualiser,colour,catalogue",
    ok: "Ask the catalog for your favorite color",
    // `color` e `centers` non sono britannici: nessun falso allarme.
    ok2: "The color of the center, the colors of the centers; Spectrum analyzer",
  };
  const trovati = grafiaBritannica(en);
  const parole = trovati.map((t) => t.match(/«(\w+)», in americano (\w+)/).slice(1).join(">"));
  assert.deepEqual(parole.sort(), [
    "analyser>analyzer",
    "catalogue>catalog",
    "colour>color",
    "favourite>favorite",
    "licence>license",
    "recognise>recognize",
  ]);
  assert.ok(trovati.every((t) => !t.includes("«c.syn»") && !t.includes("«ok")));
});

test("una chiave che nessun punto del codice nomina è morta", () => {
  const riferimento = {
    "queue.remove": "Togli",
    "queue.remove.telefono": "Togli dalla coda",
    "settings.sync.rename": "Rinomina",
    "settings.entry.audio": "Audio",
    "skin.issue.oggetto": "qui ci va un oggetto",
    "errors.fs.notFound": "Il file non c'è più.",
    "errors.transfer.pinInvalid": "Questo codice non è quello giusto.",
    "errors.action.folders": "Apri le cartelle",
    "nav.close": "Chiudi il menu",
    "format.tracks": "{n} brani",
    "format.tracks.uno": "1 brano",
  };
  const testoCodice = [
    't("queue.remove")',
    // Costruita: `settings.entry.${id}`.
    "const chiave = `settings.entry.${id}`;",
    'motivo!("oggetto", "qui ci va un oggetto")',
    'tSe("errors.action.folders", "")',
    'const f = t("format.tracks", { n });',
  ].join("\n");
  const trovati = chiaviMorte({ riferimento, testoCodice, codici: ["fs.notFound"] });
  assert.deepEqual(trovati.sort(), [
    // Un codice che il catalogo ha tolto si porta dietro la sua frase: si dice com'è.
    "«errors.transfer.pinInvalid» è la frase di un codice che il catalogo del nucleo non ha più: togli la frase",
    "«nav.close» non è usata da nessun punto del codice",
    "«settings.sync.rename» non è usata da nessun punto del codice",
  ].sort());
  // Le forme del plurale e la variante del telefono non sono chiavi a sé.
  assert.ok(!trovati.some((t) => t.includes("format.tracks") || t.includes("queue.remove")));
});

test("un commento che cita la chiave non la disegna, e un'eccezione scritta sì", () => {
  const riferimento = { "settings.sync.rename": "Rinomina", "cloudApp.google.step1": "Vai in «API»" };
  // Il commento sta già fuori dal testo che `main` passa: `leggiTutti` lo toglie.
  const eccezioni = { chiaviVive: { "cloudApp.": "le passi della console di un servizio, composte per nome" } };
  const trovati = chiaviMorte({ riferimento, testoCodice: "", codici: [], eccezioni });
  assert.deepEqual(trovati, ["«settings.sync.rename» non è usata da nessun punto del codice"]);
  // Una chiave esatta, senza il punto finale, vale solo per sé.
  const esatta = chiaviMorte({ riferimento, testoCodice: "", codici: [], eccezioni: { chiaviVive: { "settings.sync.rename": "motivo" } } });
  assert.deepEqual(esatta, ["«cloudApp.google.step1» non è usata da nessun punto del codice"]);
});

test("una frase che cita un tasto lo cita con le parole che ha sullo schermo", () => {
  const en = {
    "action.addToPlaylist": "Add to a playlist…",
    "playlist.empty.body": "From a track: ⋮ and then “Add to playlist…”.",
    "ok.body": "Press “Add to a playlist” to start one.",
    "ext.body": "In the console choose “Desktop app”, and write {nome} as “{titolo}”.",
    "cloudApp.google.step4": "Press “Create client”.",
    "lungo": "He said “this quote is much longer than forty-five characters, so it is a sentence”.",
  };
  const trovati = etichetteCitate("en", en, {
    etichetteEsterne: { "Desktop app": "Il tipo di client della console di Google." },
    etichetteInChiavi: { "cloudApp.": "I passi della console di un servizio." },
  });
  // «Add to playlist…» non è «Add to a playlist…»: la frase manda a un tasto che non c'è.
  assert.deepEqual(trovati, [
    "en.json: «playlist.empty.body» cita «Add to playlist…», che non è il testo di nessuna chiave",
  ]);
  // Il finale («…», «.») e i segnaposto non contano; in italiano valgono le «».
  const it = { "a.b": "Aggiungi a playlist…", c: "Premi «Aggiungi a playlist» o “Annulla”" };
  assert.deepEqual(etichetteCitate("it", it), []);
  assert.equal(etichetteCitate("it", { c: "Premi «Aggiungi alla coda»" }).length, 1);
});
