import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * Una cartella aggiunta si legge (R18-3).
 *
 * Dalle Impostazioni, da un trascinamento o dalla Home vuota la cartella
 * entrava nell'elenco, la sorveglianza cominciava a guardarne i cambiamenti
 * futuri, e i brani che c'erano già non entravano finché qualcuno non premeva
 * «Scansiona». Il README promette il contrario. La regola sta nel comando che
 * scrive l'elenco: le radici nuove si chiedono alla sorveglianza, come fa il
 * cloud. La finestra lo dice, perché i brani arrivano qualche secondo dopo.
 */
const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");

/** Il corpo di una funzione Rust, dalla firma alla funzione dopo. */
function corpoRust(sorgente: string, nome: string): string {
  const da = sorgente.indexOf(`pub async fn ${nome}(`);
  assert.ok(da >= 0, `non trovo ${nome}: la prova va aggiornata`);
  const dopo = sorgente.slice(da + 1).search(/\n(?:pub(?:\(crate\))? )?(?:async )?fn /);
  return sorgente.slice(da, da + 1 + dopo);
}

test("imposta_cartelle chiede alla sorveglianza di leggere le radici nuove", () => {
  const comando = corpoRust(leggi("../src-tauri/src/comandi.rs"), "imposta_cartelle");
  assert.match(comando, /radici_nuove\(&prima, &ordinate\.cartelle\)/);
  assert.match(comando, /crate::sorveglia::riguarda\(&app, nuove\)/);
});

test("aggiungere una cartella dice che la si sta leggendo", () => {
  const sorgente = leggi("../src/cartelleSorvegliate.ts");
  const da = sorgente.indexOf("const aggiungiCartelle = async");
  const corpo = sorgente.slice(da, sorgente.indexOf("const scegliCartella = async", da));
  assert.match(corpo, /t\("folders\.add\.reading"/);
  for (const lingua of ["it", "en"]) {
    const voci = JSON.parse(leggi(`../src/lingue/${lingua}.json`)) as Record<string, string>;
    assert.match(voci["folders.add.reading"] ?? "", /\{nome\}/, lingua);
  }
});
