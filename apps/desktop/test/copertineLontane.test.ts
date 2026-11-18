/**
 * La cartella `copertine-lontane` si ripulisce all'avvio (FR2).
 *
 * Aveva una cache con la sua potatura, per le copertine di «Sfoglia»; la potatura è
 * andata con loro, e nessuno toglieva più i file piatti della 3.2.1, le cartelle per
 * dispositivo né la cartella di zeri di una sessione di un processo morto. Una volta
 * per avvio, mai con una sessione in corso, e solo dentro quella cartella.
 *
 * Quel che toglie e quel che lascia lo prova `lontano/sfoglia.rs` con una cartella
 * temporanea; qui si guarda dove è collegata.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const leggi = (percorso: string) =>
  readFileSync(new URL(percorso, import.meta.url), "utf8").replace(/\r\n/g, "\n");
const LIB = leggi("../src-tauri/src/lib.rs");
const SFOGLIA = leggi("../src-tauri/src/lontano/sfoglia.rs");

/** Il corpo di una funzione, fino alla prima graffa chiusa a capo di riga. */
function corpo(testo: string, firma: string): string {
  const inizio = testo.indexOf(firma);
  assert.ok(inizio >= 0, `manca ${firma}`);
  return testo.slice(inizio, testo.indexOf("\n}\n", inizio));
}

test("la cartella delle copertine lontane si ripulisce una volta all'avvio, dopo «Ascolta insieme», e solo lei", () => {
  const avvio = LIB.slice(LIB.indexOf("fn avvia_dopo_la_libreria("));
  const insieme = avvio.indexOf("insieme::avvia_filo(");
  const pulizia = avvio.indexOf("lontano::ripulisci_le_copertine_di_prima(app)");
  assert.ok(insieme > 0 && pulizia > insieme, "la pulizia viene dopo che la sessione c'è, per poterne chiedere");
  const fn = corpo(SFOGLIA, "pub(crate) fn ripulisci_le_copertine_di_prima(");
  assert.match(fn, /crate::insieme::ruolo\(app\) != crate::insieme::Ruolo::Fuori/);
  assert.match(fn, /si_ripulisce\(sessione_in_corso, &RIPULITA\)/);
  // Tocca `copertine-lontane` e nient'altro: la cartella viene da `cartella_copertine`.
  assert.match(fn, /cartella_copertine\(app\)/);
  assert.match(SFOGLIA, /crate::cartella_di_cache\(app, "copertine-lontane"\)/);
});
