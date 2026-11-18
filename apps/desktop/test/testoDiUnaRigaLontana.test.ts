/**
 * Il testo della canzone di un brano di un altro dispositivo.
 *
 * Una riga della libreria unica che sta su un altro dispositivo è una riga
 * specchio, con un id **maggiore di zero** come ogni brano di questa libreria:
 * il tasto e il pannello del testo — che si nascondono per un id ≤ 0, cioè per
 * una voce di «Ascolta insieme» — la lasciano passare, e il nucleo risolve da sé
 * dove sta il testo (lo chiede al proprietario, e se non risponde continua con
 * la catena di qui: mai un errore alla finestra). Se un giorno qualcuno
 * aggiunge un cancello sull'origine del brano, questo test lo dice.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import type { DispositivoLontano } from "../src/ipc";
import { idDiLibreria } from "../src/insieme";
import { NESSUN_DISPOSITIVO, proprietarioDellAudio } from "../src/lontano";

/** Un sorgente senza commenti: quel che conta è il codice, non cosa racconta. */
function sorgente(percorso: string): string {
  return readFileSync(new URL(percorso, import.meta.url), "utf8")
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/(^|[^:])\/\/.*$/gm, "$1");
}

test("una riga specchio passa il cancello del testo e una voce di sessione no", () => {
  // Gli id delle righe specchio sono quelli di `tracks`: positivi.
  assert.equal(idDiLibreria(1), true);
  assert.equal(idDiLibreria(48_213), true);
  // Le voci di sessione stanno sotto zero (`riproduzione/ospiti.rs`).
  assert.equal(idDiLibreria(0), false);
  assert.equal(idDiLibreria(-7), false);
});

test("i cancelli del testo guardano l'id e non da dove viene il brano", () => {
  const cancelli: Array<[string, RegExp]> = [
    // `InRiproduzione.tsx`: il tasto e il pannello sul computer.
    ["../src/schermate/InRiproduzione.tsx", /const conTesto\s*=\s*brano\s*!==\s*null\s*&&\s*idDiLibreria\(brano\.id\)\s*;/],
    // `InRiproduzioneTelefono.tsx`: il foglio del telefono.
    ["../src/schermate/InRiproduzioneTelefono.tsx", /const diLibreria\s*=\s*idDiLibreria\(brano\.id\)\s*;/],
  ];
  for (const [percorso, atteso] of cancelli) {
    const codice = sorgente(percorso);
    assert.match(codice, atteso, `${percorso}: il cancello del testo è cambiato`);
    // E nessuno dei due, né il pannello né la scelta del catalogo, ha un
    // ramo per le righe di un altro dispositivo: il nucleo lo sa già.
    assert.doesNotMatch(codice, /dispositivoDellaRiga|lontano:\/\//, percorso);
  }
  for (const percorso of ["../src/parti/Testo.tsx", "../src/parti/SceltaTesto.tsx"]) {
    assert.doesNotMatch(sorgente(percorso), /dispositivoDellaRiga|lontano:\/\//, percorso);
  }
});

// ── il «Raddrizza» di un brano di un altro dispositivo ──────────────────────

const CHIAVE = "ab".repeat(32);

function dispositivo(chiave: string, nome: string): DispositivoLontano {
  return {
    chiave,
    nome,
    piattaforma: "windows",
    dispositivo: "d-1",
    online: true,
    vistoMs: null,
    associatoMs: 1,
    revocato: false,
    versione: null,
    insiemeAcceso: false,
    insiemeOspita: false,
    insieme: null,
    via: null,
  };
}

test("di chi è l'audio lo dice il percorso, e il nome il pannello dei dispositivi", () => {
  const dispositivi = [dispositivo(CHIAVE, "PC di casa")];
  // Un brano di questa macchina: l'audio è qui, e «Raddrizza» lavora.
  assert.equal(proprietarioDellAudio("C:/Musica/uno.flac", dispositivi), null);
  assert.equal(proprietarioDellAudio(null, dispositivi), null);
  assert.equal(proprietarioDellAudio(`lontano://${CHIAVE}/0`, dispositivi), null, "id storto");
  // Una riga specchio: l'audio sta sul dispositivo, che ha un nome.
  assert.deepEqual(proprietarioDellAudio(`lontano://${CHIAVE}/4120`, dispositivi), {
    chiave: CHIAVE,
    nome: "PC di casa",
  });
  // Un dispositivo che il pannello non ha più: la nota dice «un altro dispositivo».
  assert.deepEqual(proprietarioDellAudio(`lontano://${CHIAVE}/4120`, NESSUN_DISPOSITIVO), {
    chiave: CHIAVE,
    nome: null,
  });
});

test("l'editor non raddrizza un brano di un altro dispositivo e lo dice, senza il rosso", () => {
  const codice = sorgente("../src/Sincronizza.tsx");
  // Il comando che cerca gli attacchi si chiama una volta sola, e solo se
  // l'audio è di questa macchina.
  const chiamate = codice.match(/ipc\.testoAggancia\(/g) ?? [];
  assert.equal(chiamate.length, 1);
  assert.match(
    codice,
    /audioAltrove\s*\?\s*Promise\.resolve\(battute\)\s*:\s*ipc\.testoAggancia\(brano\.id,\s*battute\)/,
  );
  assert.match(codice, /const audioAltrove\s*=\s*remoto\s*!==\s*null/);
  // Il tasto cambia nome (non promette di raddrizzare), e la nota con di chi è
  // l'audio compare nei due momenti in cui il tasto c'è: le battute e le parole.
  assert.match(codice, /remoto\s*!==\s*null\s*\?\s*t\("lyrics\.remote\.sync\.finish"\)/);
  assert.equal((codice.match(/notaDellAudio\(remoto\.nome\)/g) ?? []).length, 2);
  assert.match(codice, /"lyrics\.remote\.sync\.tapDone"/);
  assert.match(codice, /"lyrics\.remote\.sync\.wordsDone"/);
  // La nota è una nota, non un errore: la classe è quella delle altre note
  // dell'editor, e il brano che non si raddrizza non passa da `onErrore`.
  assert.equal(
    (codice.match(/<p className="nota" data-nota="audio-remoto">/g) ?? []).length,
    2,
  );
});

test("i testi della nota stanno in italiano e in inglese, con il nome del dispositivo", () => {
  for (const lingua of ["it", "en"]) {
    const testi = JSON.parse(
      readFileSync(new URL(`../src/lingue/${lingua}.json`, import.meta.url), "utf8"),
    ) as Record<string, string>;
    for (const chiave of [
      "lyrics.remote.sync.finish",
      "lyrics.remote.sync.tapDone",
      "lyrics.remote.sync.wordsDone",
      "lyrics.remote.sync.note",
      "lyrics.remote.sync.altroDispositivo",
    ]) {
      assert.ok(testi[chiave], `${lingua}: manca ${chiave}`);
    }
    assert.match(testi["lyrics.remote.sync.note"] ?? "", /\{dispositivo\}/, lingua);
  }
});
