import { test } from "node:test";
import assert from "node:assert/strict";

import { categoria, formaPlurale } from "../src/lingue/plurali";

/** Un dizionario finto, letto come lo legge il motore delle lingue. */
function dizionario(voci: Record<string, string>) {
  return (chiave: string): string | null => voci[chiave] ?? null;
}

const IT = dizionario({
  "format.tracks": "{n} brani",
  "format.tracks.uno": "1 brano",
  "folders.offline": "{n} cartelle non rispondono.",
  "folders.offline.uno": "Una cartella non risponde.",
  "page.library.sub": "{n} in libreria",
});

const EN = dizionario({
  "format.tracks": "{n} tracks",
  "format.tracks.uno": "1 track",
});

test("italiano: zero, uno e molti", () => {
  assert.equal(formaPlurale(IT, "it", "format.tracks", 0), "{n} brani");
  assert.equal(formaPlurale(IT, "it", "format.tracks", 1), "1 brano");
  assert.equal(formaPlurale(IT, "it", "format.tracks", 2), "{n} brani");
  assert.equal(formaPlurale(IT, "it", "format.tracks", 18_534), "{n} brani");
  assert.equal(formaPlurale(IT, "it", "folders.offline", 1), "Una cartella non risponde.");
});

test("inglese: zero, uno e molti", () => {
  assert.equal(formaPlurale(EN, "en", "format.tracks", 0), "{n} tracks");
  assert.equal(formaPlurale(EN, "en", "format.tracks", 1), "1 track");
  assert.equal(formaPlurale(EN, "en", "format.tracks", 2), "{n} tracks");
});

test("il milione italiano è «many» per il CLDR, e senza la forma ripiega", () => {
  // Il CLDR dà all'italiano una forma «many» per i milioni esatti; nessuna
  // chiave la scrive, e la frase resta quella generica.
  assert.equal(categoria("it", 1_000_000), "many");
  assert.equal(formaPlurale(IT, "it", "format.tracks", 1_000_000), "{n} brani");
});

test("una chiave senza forme resta quella di sempre", () => {
  assert.equal(formaPlurale(IT, "it", "page.library.sub", 1), "{n} in libreria");
  assert.equal(formaPlurale(IT, "it", "page.library.sub", 7), "{n} in libreria");
});

test("una chiave che il dizionario non ha in nessuna forma è null", () => {
  assert.equal(formaPlurale(IT, "it", "non.esiste", 1), null);
  assert.equal(formaPlurale(EN, "en", "folders.offline", 3), null);
});

test("le categorie del russo, e «.uno» che vince su «.one»", () => {
  const RU = dizionario({
    brani: "{n} трека",
    "brani.uno": "один трек",
    "brani.one": "{n} трек",
    "brani.few": "{n} трека",
    "brani.many": "{n} треков",
  });
  assert.equal(formaPlurale(RU, "ru", "brani", 1), "один трек");
  assert.equal(formaPlurale(RU, "ru", "brani", 21), "{n} трек");
  assert.equal(formaPlurale(RU, "ru", "brani", 3), "{n} трека");
  assert.equal(formaPlurale(RU, "ru", "brani", 5), "{n} треков");
  assert.equal(formaPlurale(RU, "ru", "brani", 0), "{n} треков");
  // Un decimale è «other»: la chiave nuda.
  assert.equal(formaPlurale(RU, "ru", "brani", 1.5), "{n} трека");
});

test("ripiego su «.other» prima della chiave nuda", () => {
  const XX = dizionario({ voce: "nuda", "voce.other": "altra", "voce.one": "una" });
  assert.equal(formaPlurale(XX, "en", "voce", 1), "una");
  assert.equal(formaPlurale(XX, "en", "voce", 2), "altra");
});

test("una lingua con una forma sola non chiede «.one»", () => {
  const JA = dizionario({ brani: "{n}曲", "brani.one": "mai usata" });
  assert.equal(categoria("ja", 1), "other");
  assert.equal(formaPlurale(JA, "ja", "brani", 1), "{n}曲");
});

test("un numero che non è un numero prende la forma generica", () => {
  assert.equal(formaPlurale(IT, "it", "format.tracks", Number.NaN), "{n} brani");
});

test("un codice di lingua malformato non lancia", () => {
  assert.equal(typeof categoria("", 1), "string");
});
