/**
 * L'albero del pannello «Cartelle»: foglie, ordine dei nomi, filtro, selezione.
 *
 * Solo `albero.ts`, che non tocca né il DOM né il nucleo: le risposte del
 * nucleo qui sono scritte a mano, nella forma in cui `cartelle.rs` le manda.
 */
import { test } from "node:test";
import assert from "node:assert/strict";

import type { CartelleFiltrate, FileCartella, NodoCartella } from "../src/ipc";
import {
  NESSUNA,
  VUOTO,
  appiattisci,
  apri,
  attraversoLente,
  bersagli,
  chiave,
  chiaveRiga,
  chiudi,
  codaDaFile,
  conFigli,
  fratelliDi,
  lente,
  ordinaFile,
  seleziona,
  type RigaAlbero,
  type StatoAlbero,
} from "../src/albero";

const cartella = (
  percorso: string,
  sottocartelle: number,
  file: number,
  radice = false,
): NodoCartella => ({
  percorso,
  // Come le scrive il nucleo: la cima col percorso intero, le altre col nome.
  nome: radice ? percorso : (percorso.split(/[\\/]/).pop() ?? percorso),
  brani: file + sottocartelle,
  sottocartelle,
  file,
  radice,
});

const foglia = (brano: number, percorso: string, titolo = `Brano ${brano}`): FileCartella => ({
  brano,
  percorso,
  nome: percorso.split(/[\\/]/).pop() ?? percorso,
  titolo,
  durataMs: 200_000 + brano,
});

/** Una libreria piccola: una radice, un album con un disco dentro e tre file. */
function libreria(): StatoAlbero {
  let stato = conFigli(VUOTO, null, [cartella("C:\\M", 1, 0, true)]);
  stato = conFigli(stato, "C:\\M", [cartella("C:\\M\\Album", 1, 3)]);
  stato = conFigli(
    stato,
    "C:\\M\\Album",
    [cartella("C:\\M\\Album\\CD2", 0, 1)],
    [
      foglia(10, "C:\\M\\Album\\10 - Dieci.mp3", "Dieci"),
      foglia(2, "C:\\M\\Album\\2 - Due.mp3", "Due"),
      foglia(1, "C:\\M\\Album\\1 - Uno.mp3", ""),
    ],
  );
  stato = conFigli(stato, "C:\\M\\Album\\CD2", [], [foglia(20, "C:\\M\\Album\\CD2\\a.mp3")]);
  return stato;
}

const nomi = (righe: readonly RigaAlbero[]) => righe.map((r) => r.nome);

test("le foglie vengono dopo le sottocartelle, allo stesso livello", () => {
  const stato = apri(apri(libreria(), "C:\\M"), "C:\\M\\Album");
  const righe = appiattisci(stato);
  // Il titolo vuoto si scrive col nome del file: è l'unica cosa che distingue
  // un file senza tag dai suoi vicini.
  assert.deepEqual(nomi(righe), ["C:\\M", "Album", "CD2", "1 - Uno.mp3", "Due", "Dieci"]);
  const [, album, cd2, uno] = righe;
  assert.ok(album && cd2 && uno);
  assert.equal(album.figli, true, "una cartella d'album si apre");
  assert.equal(cd2.brano, null);
  assert.equal(uno.brano, 1);
  assert.equal(uno.figli, false, "una foglia non si apre");
  // Le foglie contano fra i fratelli: «4 di 4» per l'ultima, cartella compresa.
  assert.deepEqual(
    righe.slice(2).map((r) => [r.livello, r.posizione, r.fratelli]),
    [
      [3, 1, 4],
      [3, 2, 4],
      [3, 3, 4],
      [3, 4, 4],
    ],
  );
  // Sotto una radice vera: «Mostra nella cartella» si può offrire.
  assert.ok(righe.every((r) => r.sorvegliata));
});

test("una cartella chiusa nasconde le sue foglie senza dimenticarle", () => {
  const aperto = apri(apri(libreria(), "C:\\M"), "C:\\M\\Album");
  const chiuso = chiudi(aperto, "C:\\M\\Album");
  assert.equal(appiattisci(chiuso).length, 2);
  assert.equal(chiuso.file.get(chiave("C:\\M\\Album"))?.length, 3, "la cache resta");
});

test("i file in ordine di nome naturale: 2 prima di 10", () => {
  const ordinati = ordinaFile([
    foglia(1, "C:\\A\\track10.flac"),
    foglia(2, "C:\\A\\track2.flac"),
    foglia(3, "C:\\A\\Track1.flac"),
    foglia(4, "C:\\A\\track1.flac"),
  ]);
  assert.deepEqual(
    ordinati.map((f) => f.nome),
    // Maiuscole uguali per la collazione: a parità decide il percorso, così
    // l'ordine non dipende da quello d'arrivo.
    ["Track1.flac", "track1.flac", "track2.flac", "track10.flac"],
  );
});

test("le cartelle sorelle non si mescolano con i file per `*`", () => {
  const righe = appiattisci(apri(apri(libreria(), "C:\\M"), "C:\\M\\Album"));
  // Dal primo file: le sorelle da aprire sono le sole cartelle.
  assert.deepEqual(fratelliDi(righe, 3), ["C:\\M\\Album\\CD2"]);
});

test("il filtro apre gli antenati e mostra solo quel che porta alle corrispondenze", () => {
  const stato = apri(libreria(), "C:\\M");
  const risposta: CartelleFiltrate = {
    rami: [
      { percorso: null, cartelle: [cartella("C:\\M", 1, 0, true)], file: [] },
      { percorso: "C:\\M", cartelle: [cartella("C:\\M\\Album", 1, 3)], file: [] },
      {
        percorso: "C:\\M\\Album",
        cartelle: [],
        file: [foglia(10, "C:\\M\\Album\\10 - Dieci.mp3", "Dieci")],
      },
    ],
    trovate: [],
    corrispondenze: 1,
    troncato: false,
  };
  const filtro = lente(risposta);
  const sotto = appiattisci(attraversoLente(stato, filtro));
  // Gli antenati aperti, la sorella CD2 e gli altri file potati via.
  assert.deepEqual(nomi(sotto), ["C:\\M", "Album", "Dieci"]);
  assert.ok(sotto.slice(0, 2).every((r) => r.aperto));

  // Fuori dal filtro l'albero è com'era, aperti compresi.
  assert.deepEqual(nomi(appiattisci(stato)), ["C:\\M", "Album"]);
});

test("una cartella trovata, aperta sotto il filtro, si vede intera", () => {
  const risposta: CartelleFiltrate = {
    rami: [
      { percorso: null, cartelle: [cartella("C:\\M", 1, 0, true)], file: [] },
      { percorso: "C:\\M", cartelle: [cartella("C:\\M\\Album", 1, 3)], file: [] },
    ],
    trovate: ["C:\\M\\Album"],
    corrispondenze: 1,
    troncato: false,
  };
  const filtro = lente(risposta);
  assert.ok(filtro.trovate.has(chiave("C:\\M\\Album")));
  const chiusa = appiattisci(attraversoLente(libreria(), filtro));
  assert.deepEqual(nomi(chiusa), ["C:\\M", "Album"], "trovata ma chiusa");
  // Aperta a mano: il contenuto viene dalla cache, non dalla lente.
  const aperta = { ...filtro, aperti: [...filtro.aperti, chiave("C:\\M\\Album")] };
  assert.deepEqual(nomi(appiattisci(attraversoLente(libreria(), aperta))), [
    "C:\\M",
    "Album",
    "CD2",
    "1 - Uno.mp3",
    "Due",
    "Dieci",
  ]);
});

test("Maiusc prende l'intervallo dall'àncora, Ctrl aggiunge e toglie", () => {
  const righe = appiattisci(apri(apri(libreria(), "C:\\M"), "C:\\M\\Album"));
  // Il clic semplice sul primo file: niente scelto, àncora lì.
  let sel = seleziona(NESSUNA, righe, 3, "azzera");
  assert.equal(sel.chiavi.size, 0);
  assert.equal(sel.ancora, 3);
  // Maiusc sull'ultimo: i tre file.
  sel = seleziona(sel, righe, 5, "intervallo");
  assert.deepEqual(
    righe.filter((r) => sel.chiavi.has(chiaveRiga(r))).map((r) => r.brano),
    [1, 2, 10],
  );
  // Ctrl sul secondo lo toglie, e l'àncora si sposta lì.
  sel = seleziona(sel, righe, 4, "alterna");
  assert.deepEqual(
    righe.filter((r) => sel.chiavi.has(chiaveRiga(r))).map((r) => r.brano),
    [1, 10],
  );
  assert.equal(sel.ancora, 4);
  // Maiusc verso l'alto da lì: l'intervallo si rifà da capo, non si somma.
  sel = seleziona(sel, righe, 2, "intervallo");
  assert.deepEqual(
    righe.filter((r) => sel.chiavi.has(chiaveRiga(r))).map((r) => r.nome),
    ["CD2", "1 - Uno.mp3", "Due"],
  );
});

test("una cartella scelta copre i file scelti dentro di lei", () => {
  const stato = apri(apri(apri(libreria(), "C:\\M"), "C:\\M\\Album"), "C:\\M\\Album\\CD2");
  const righe = appiattisci(stato);
  assert.deepEqual(nomi(righe), ["C:\\M", "Album", "CD2", "Brano 20", "1 - Uno.mp3", "Due", "Dieci"]);
  let sel = seleziona(NESSUNA, righe, 2, "alterna"); // CD2
  sel = seleziona(sel, righe, 3, "alterna"); // il suo file
  sel = seleziona(sel, righe, 6, "alterna"); // Dieci, fuori da CD2
  // Il file dentro CD2 non conta due volte: CD2 lo porta già.
  assert.deepEqual(
    bersagli(righe, sel.chiavi).map((r) => r.nome),
    ["CD2", "Dieci"],
  );
});

test("il doppio clic su un file suona la cartella da lì", () => {
  const righe = appiattisci(apri(apri(libreria(), "C:\\M"), "C:\\M\\Album"));
  assert.deepEqual(codaDaFile(righe, 4), { brani: [1, 2, 10], partenza: 1 });
  assert.equal(codaDaFile(righe, 2), null, "una cartella non è un file");
});
