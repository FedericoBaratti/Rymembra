import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { notiziaDelProfilo, vociPortate } from "../src/parti/notiziaProfilo";

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
