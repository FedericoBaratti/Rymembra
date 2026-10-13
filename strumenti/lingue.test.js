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
