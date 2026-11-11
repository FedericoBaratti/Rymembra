import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * WebDAV: durante «Provo ad accedere…» la finestrella non si chiude (R12-8).
 *
 * Chiuderla (Annulla, Escape, il velo, un'altra linguetta) chiamava
 * `nuvolaContoAnnulla`, ma l'annullo lo legge solo l'attesa del browser: il
 * nucleo finiva lo stesso, scriveva la password nel portachiavi e salvava il
 * conto, che compariva in Impostazioni anche se chi guardava aveva annullato.
 * Come già per «Aggiungo…», finché il server non risponde la finestrella
 * resta aperta (al più la scadenza delle domande WebDAV, 30 s).
 */
const SORGENTE = readFileSync(new URL("../src/AggiungiCartella.tsx", import.meta.url), "utf8");

/** Il corpo di una funzione di primo livello. */
function funzione(nome: string): string {
  const da = SORGENTE.search(new RegExp(`\\n(?:export )?function ${nome}\\(`));
  assert.ok(da >= 0, `non trovo ${nome}`);
  const dopo = SORGENTE.slice(da + 1).search(/\n(?:export )?function [A-Za-z]+\(/);
  return SORGENTE.slice(da, dopo < 0 ? undefined : da + 1 + dopo);
}

test("il pannello WebDAV dice quando sta provando, e smette smontandosi", () => {
  const corpo = funzione("PerWebdav");
  assert.match(corpo, /onAttesa\?\.\(inAttesa\)/);
  assert.match(corpo, /return \(\) => onAttesa\?\.\(false\)/);
});

test("le due finestrelle non si chiudono mentre WebDAV prova", () => {
  for (const nome of ["AggiungiCartella", "RicollegaConto"]) {
    const corpo = funzione(nome);
    assert.match(corpo, /onAttesa=\{setProvaWebdav\}/, nome);
    assert.match(corpo, /chiudibile=\{[^}]*!provaWebdav/, nome);
    const annulla = corpo.slice(corpo.lastIndexOf("<button", corpo.indexOf('t("common.cancel")')));
    assert.match(annulla.slice(0, annulla.indexOf(">")), /provaWebdav/, `${nome}: «Annulla» del piede`);
  }
  // Cambiare linguetta smonterebbe il pannello, con lo stesso annullo che non ferma.
  const scegli = SORGENTE.slice(SORGENTE.indexOf("const scegliSorgente = "));
  assert.match(scegli.slice(0, scegli.indexOf("};")), /if \(provaWebdav\) return;/);
});
