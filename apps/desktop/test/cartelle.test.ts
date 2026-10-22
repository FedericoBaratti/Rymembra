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
  branoDImmagine,
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

// ── i brani delle immagini CD ──────────────────────────────────────────────

/**
 * Un brano d'immagine come lo manda il nucleo: il percorso è quello del `.cue`
 * più `|NN`, e il nome è l'ultimo segmento — che di un file non è il nome.
 */
const traccia = (brano: number, cue: string, numero: string, titolo = `Brano ${brano}`) =>
  foglia(brano, `${cue}|${numero}`, titolo);

test("un brano d'immagine si riconosce dal percorso, come nel nucleo", () => {
  assert.deepEqual(branoDImmagine("C:\\M\\Disco\\Album.cue|03"), {
    cue: "C:\\M\\Disco\\Album.cue",
    numero: 3,
  });
  assert.equal(branoDImmagine("\\\\nas\\cd\\ALBUM.CUE|12")?.numero, 12, "maiuscole");
  // Due cifre, esatte; e un `.cue` prima del separatore.
  assert.equal(branoDImmagine("C:\\M\\Album.cue|3"), null);
  assert.equal(branoDImmagine("C:\\M\\Album.cue|003"), null);
  assert.equal(branoDImmagine("C:\\M\\Album.flac|03"), null);
  assert.equal(branoDImmagine("C:\\M\\03 - Brano.flac"), null);
});

test("i brani dello stesso .cue stanno in fila, per numero", () => {
  const cue = "C:\\M\\Disco\\Album.cue";
  const ordinati = ordinaFile([
    traccia(10, cue, "10", "Decimo"),
    traccia(2, cue, "02", "Zeta"),
    traccia(1, cue, "01", "Mezzo"),
  ]);
  // Per numero e non per titolo: è l'ordine del disco.
  assert.deepEqual(
    ordinati.map((f) => f.brano),
    [1, 2, 10],
  );
  // Dal percorso, non da `nome`: anche se il nucleo ci scrivesse il titolo,
  // l'ordine resterebbe quello del disco.
  const coiTitoli = ordinaFile([
    { ...traccia(2, cue, "02"), nome: "Alfa" },
    { ...traccia(1, cue, "01"), nome: "Zeta" },
  ]);
  assert.deepEqual(
    coiTitoli.map((f) => f.brano),
    [1, 2],
  );
});

test("due .cue nella stessa cartella non si mescolano, e i file veri stanno al loro posto", () => {
  const ordinati = ordinaFile([
    traccia(21, "C:\\M\\Box\\CD2.cue", "01"),
    foglia(99, "C:\\M\\Box\\Bonus.flac"),
    traccia(12, "C:\\M\\Box\\CD1.cue", "02"),
    traccia(22, "C:\\M\\Box\\CD2.cue", "02"),
    traccia(11, "C:\\M\\Box\\CD1.cue", "01"),
    foglia(98, "C:\\M\\Box\\Zeta.flac"),
  ]);
  assert.deepEqual(
    ordinati.map((f) => f.brano),
    // «Bonus.flac», poi CD1 per intero, poi CD2, poi «Zeta.flac»: il nome del
    // `.cue` sta fra gli altri nomi come se fosse un file.
    [99, 11, 12, 21, 22, 98],
  );
});

test("un brano d'immagine senza titolo si chiama col numero e il disco, mai «Album.cue|03»", () => {
  let stato = conFigli(VUOTO, null, [cartella("C:\\M", 1, 0, true)]);
  stato = conFigli(stato, "C:\\M", [cartella("C:\\M\\Disco", 0, 3)]);
  stato = conFigli(
    stato,
    "C:\\M\\Disco",
    [],
    [
      traccia(3, "C:\\M\\Disco\\Album.cue", "03", "  "),
      traccia(1, "C:\\M\\Disco\\Album.cue", "01", "Il primo"),
      traccia(2, "C:\\M\\Disco\\Album.cue", "02", ""),
    ],
  );
  stato = apri(apri(stato, "C:\\M"), "C:\\M\\Disco");
  // Senza lingue: il numero e il disco.
  assert.deepEqual(nomi(appiattisci(stato)), [
    "C:\\M",
    "Disco",
    "Il primo",
    "02 · Album",
    "03 · Album",
  ]);
  // Con la frase della finestra.
  const righe = appiattisci(stato, (numero, album) => `Traccia ${numero} · ${album}`);
  assert.deepEqual(nomi(righe).slice(2), ["Il primo", "Traccia 02 · Album", "Traccia 03 · Album"]);
  // Percorsi diversi, chiavi diverse: la selezione non confonde due brani dello
  // stesso `.cue`, e il doppio clic suona il disco da lì, in ordine.
  assert.equal(new Set(righe.map(chiaveRiga)).size, righe.length);
  assert.deepEqual(codaDaFile(righe, 3), { brani: [1, 2, 3], partenza: 1 });
});

test("sotto il filtro i brani d'immagine tengono lo stesso ordine", () => {
  const filtro = lente({
    rami: [
      { percorso: null, cartelle: [cartella("C:\\M", 1, 0, true)], file: [] },
      {
        percorso: "C:\\M",
        cartelle: [],
        file: [traccia(10, "C:\\M\\Album.cue", "10"), traccia(9, "C:\\M\\Album.cue", "09")],
      },
    ],
    trovate: [],
    corrispondenze: 2,
    troncato: false,
  });
  assert.deepEqual(
    filtro.file.get(chiave("C:\\M"))?.map((f) => f.brano),
    [9, 10],
  );
});
