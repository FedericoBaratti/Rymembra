/**
 * Il permesso di leggere la musica, sul telefono, anche dopo il primo avvio.
 *
 * Il primo avvio lo chiede (`Primo.tsx`), ma «Lo faccio dopo» resta: e chi lo
 * sceglie, o dice di no al dialogo di Android, trovava una libreria che si
 * scansionava vuota per sempre, senza un posto nell'app dove concederlo. Adesso
 * lo si chiede da Impostazioni › Cartelle e scansione e dagli stati vuoti della
 * libreria (D-35). Qui la regola di che cosa chiedere dove, e il cablaggio dei
 * due posti.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { cosaChiedere } from "../src/parti/permessoMusica";

const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");

test("nello stato vuoto si chiede soltanto il permesso che svuota la libreria", () => {
  assert.equal(cosaChiedere("manca", "vuoto"), "musica");
  // Con i soli brani la libreria si riempie: lo stato vuoto non è colpa del
  // permesso, e la frase non va detta lì.
  assert.equal(cosaChiedere("solo-media", "vuoto"), "niente");
  assert.equal(cosaChiedere("pieno", "vuoto"), "niente");
  // Finché non si sa, niente: una frase che compare e sparisce a ogni avvio.
  assert.equal(cosaChiedere("verifica", "vuoto"), "niente");
});

test("nelle Impostazioni si passa anche ad «Accesso a tutti i file»", () => {
  assert.equal(cosaChiedere("manca", "impostazioni"), "musica");
  assert.equal(cosaChiedere("solo-media", "impostazioni"), "tuttiIFile");
  assert.equal(cosaChiedere("pieno", "impostazioni"), "niente");
  assert.equal(cosaChiedere("verifica", "impostazioni"), "niente");
});

test("la spiegazione di «Accesso a tutti i file» non promette di riscrivere i tag", () => {
  // Rymembra scrive i tag soltanto dei brani che scarica (`procura.rs`): la
  // libreria che c'è non la tocca, e PRIVACY lo promette. La frase del primo
  // avvio diceva «corregge i tag», proprio dove si concede il permesso più
  // ampio (R17-2, D-95).
  for (const lingua of ["it", "en"]) {
    const testi = JSON.parse(leggi(`../src/lingue/${lingua}.json`)) as Record<string, string>;
    const livelli = testi["telefono.accesso.livelli"] ?? "";
    assert.ok(livelli, `${lingua}: manca telefono.accesso.livelli`);
    assert.doesNotMatch(livelli, /\btag/i, `${lingua}: promette di toccare i tag: ${livelli}`);
    // E dice quel che fa davvero: i .lrc, i .cue, le copertine, le eliminazioni.
    for (const cosa of [".lrc", ".cue"]) assert.ok(livelli.includes(cosa), `${lingua}: non nomina ${cosa}`);
  }
});

/** Il pezzo di sorgente da `inizio` alla prima `fine` che lo segue. */
function tratto(sorgente: string, inizio: string, fine: string): string {
  const da = sorgente.indexOf(inizio);
  assert.ok(da >= 0, `manca ${inizio}`);
  const a = sorgente.indexOf(fine, da);
  assert.ok(a > da, `dopo ${inizio} manca ${fine}`);
  return sorgente.slice(da, a);
}

test("il permesso si chiede anche fuori dal primo avvio: negli stati vuoti della libreria", () => {
  const app = leggi("../src/App.tsx");
  assert.match(app, /from "\.\/parti\/AccessoTelefono"/);
  for (const titolo of ['titolo={t("empty.noFolders.title")}', 'titolo={t("empty.library.title")}']) {
    assert.match(
      tratto(app, titolo, "/>"),
      /nota=\{notaDelPermesso\(/,
      `lo stato vuoto ${titolo} non offre il permesso`,
    );
  }
});

test("il permesso si chiede anche fuori dal primo avvio: in Impostazioni › Cartelle e scansione", () => {
  const impostazioni = leggi("../src/schermate/Impostazioni.tsx");
  assert.match(impostazioni, /from "\.\.\/parti\/AccessoTelefono"/);
  assert.match(
    tratto(impostazioni, 'titolo={t("settings.roots.title")}', "</Scheda>"),
    /<PermessoMusica \/>/,
    "la scheda delle cartelle sorvegliate non offre il permesso",
  );
});
