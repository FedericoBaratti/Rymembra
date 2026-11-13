import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { notiziaDelProfilo, vociNonAttaccate, vociPortate } from "../src/parti/notiziaProfilo";

/*
 * Il profilo, dopo «Applica» e accanto a «Torna com'era prima»: dire quel che
 * è successo davvero (R13c-5) e quel che il tasto non disfa (R13c-4).
 */

const NIENTE = {
  ascolti: 0,
  voti: 0,
  preferiti: 0,
  posizioni: 0,
  playlist: 0,
  cartelle: 0,
  cronologia: 0,
  correzioni: 0,
  testi: 0,
  desiderati: 0,
  copertine: 0,
  fileCopertine: 0,
};

const sorgente = readFileSync(new URL("../src/parti/Profilo.tsx", import.meta.url), "utf8");

test("con le stesse impostazioni la notizia racconta la libreria, non «niente»", () => {
  const pezzi = notiziaDelProfilo({
    cambi: [],
    portati: { ...NIENTE, cronologia: 312, playlist: 4 },
  });
  assert.deepEqual(pezzi, [
    {
      chiave: "profile.applied.library",
      voci: [
        ["profile.brings.history", 312],
        ["profile.brings.playlists", 4],
      ],
    },
  ]);
});

test("impostazioni e libreria si dicono tutte e due; «niente» solo se è niente", () => {
  assert.deepEqual(
    notiziaDelProfilo({ cambi: [1, 2, 3], portati: { ...NIENTE, voti: 2 } }).map((p) => p.chiave),
    ["profile.applied", "profile.applied.library"],
  );
  assert.deepEqual(notiziaDelProfilo({ cambi: [], portati: NIENTE }), [
    { chiave: "profile.nothingChanged" },
  ]);
  assert.deepEqual(vociPortate(NIENTE), []);
});

test("la notizia dopo «Applica» passa da notiziaDelProfilo", () => {
  const inizio = sorgente.indexOf("const applica = async");
  assert.ok(inizio >= 0);
  const corpo = sorgente.slice(inizio, sorgente.indexOf("const annulla = async", inizio));
  assert.match(corpo, /notiziaDelProfilo\(fatto\)/, "la notizia conta solo le impostazioni");
  assert.doesNotMatch(corpo, /fatto\.cambi\.length === 0/);
});

test("il piano dice «non cambierebbe niente» solo senza niente per la libreria", () => {
  // La frase stava sopra l'elenco dei portati: «non cambierebbe niente» e
  // subito sotto «312 ascolti nella cronologia».
  assert.match(sorgente, /portati\.length === 0\s*\?\s*t\("profile\.noChanges"\)/);
  assert.match(sorgente, /t\("profile\.sameSettings"\)/);
});

test("accanto a «Torna com'era prima» c'è la nota su quel che resta", () => {
  const tasto = sorgente.indexOf('t("profile.undo")');
  assert.ok(tasto >= 0, "il tasto");
  const nota = sorgente.indexOf('t("profile.undo.note")');
  assert.ok(nota >= 0, "la nota promessa da profilo.rs non c'è");
  assert.ok(Math.abs(nota - tasto) < 1200, "la nota sta accanto al tasto");
  for (const lingua of ["it", "en"]) {
    const testi = JSON.parse(
      readFileSync(new URL(`../src/lingue/${lingua}.json`, import.meta.url), "utf8"),
    ) as Record<string, string>;
    assert.ok(testi["profile.undo.note"], `profile.undo.note in ${lingua}`);
    assert.ok(testi["profile.applied.library"], `profile.applied.library in ${lingua}`);
    assert.ok(testi["profile.sameSettings"], `profile.sameSettings in ${lingua}`);
  }
});

test("una skin del profilo che non passa l'esame si dice, nel piano e dopo", () => {
  // Decisione 21: arriva installata ma non indossata, e chi importa lo legge.
  assert.deepEqual(
    notiziaDelProfilo({ cambi: [], portati: NIENTE, skinNonIndossata: "notte" }),
    [{ chiave: "profile.skinNotWorn", nome: "notte" }],
  );
  assert.match(sorgente, /piano\.skinNonIndossata && \(/);
  assert.match(sorgente, /t\("profile\.skinNotWorn", \{ nome: piano\.skinNonIndossata \}\)/);
});

const SENZA_BRANI = {
  ascolti: 1,
  voti: 1,
  preferiti: 1,
  cronologia: 1,
  correzioni: 1,
  playlist: 5,
};

test("un profilo letto a libreria vuota dice cosa non ha potuto attaccare (T3-3)", () => {
  // Il piano lo dice prima di «Applica», la notizia dopo: stesse voci, e solo
  // quelle diverse da zero.
  assert.deepEqual(vociNonAttaccate({ ...SENZA_BRANI, ascolti: 0, correzioni: 0 }), [
    ["profile.unattached.ratings", 1],
    ["profile.unattached.loved", 1],
    ["profile.unattached.history", 1],
    ["profile.unattached.playlists", 5],
  ]);
  assert.deepEqual(vociNonAttaccate(undefined), []);
  assert.deepEqual(
    notiziaDelProfilo({ cambi: [1, 2, 3], portati: NIENTE, nonAttaccati: SENZA_BRANI }).map(
      (p) => p.chiave,
    ),
    ["profile.applied", "profile.applied.unattached"],
  );
});

test("la parte di libreria rifiutata si dice, e non si scambia per «niente» (T3-1)", () => {
  // «3 impostazioni applicate» e basta nascondeva il rifiuto: il piano, senza
  // copia di sicurezza, aveva promesso tutto.
  assert.deepEqual(
    notiziaDelProfilo({ cambi: [1, 2, 3], portati: NIENTE, libreriaRifiutata: true }),
    [{ chiave: "profile.applied", n: 3 }, { chiave: "profile.applied.refused" }],
  );
  assert.deepEqual(notiziaDelProfilo({ cambi: [], portati: NIENTE, libreriaRifiutata: true }), [
    { chiave: "profile.applied.refused" },
  ]);
});

test("il piano mostra quel che non si attacca prima di «Applica», in tutte e due le lingue", () => {
  const avviso = sorgente.indexOf('t("profile.unattached"');
  const applica = sorgente.indexOf('t("profile.apply")');
  assert.ok(avviso >= 0, "l'avviso nel piano");
  assert.ok(avviso < applica, "prima del tasto che applica");
  assert.match(sorgente, /vociNonAttaccate\(piano\.nonAttaccati\)/);
  for (const lingua of ["it", "en"]) {
    const testi = JSON.parse(
      readFileSync(new URL(`../src/lingue/${lingua}.json`, import.meta.url), "utf8"),
    ) as Record<string, string>;
    for (const chiave of [
      "profile.unattached",
      "profile.unattached.telefono",
      "profile.applied.unattached",
      "profile.applied.refused",
    ]) {
      assert.ok(testi[chiave], `${chiave} in ${lingua}`);
    }
    // Dice cosa fare: scansionare, poi leggere di nuovo.
    assert.match(testi["profile.unattached"] ?? "", lingua === "it" ? /leggi di nuovo il profilo/ : /read the profile again/);
    assert.match(testi["profile.unattached.telefono"] ?? "", lingua === "it" ? /accesso alla musica/ : /access to your music/);
  }
});
