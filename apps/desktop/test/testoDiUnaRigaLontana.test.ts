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

import { idDiLibreria } from "../src/insieme";

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
