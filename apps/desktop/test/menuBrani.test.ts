import { test } from "node:test";
import assert from "node:assert/strict";

import type { Brano, Playlist } from "../src/ipc";
import {
  vociAlbum,
  vociArtista,
  vociBrani,
  vociPlaylist,
  type AzioniBrani,
} from "../src/menuBrani";

/** La chiave al posto del testo: le prove guardano quali voci, non le parole. */
const t = (chiave: string) => chiave;

/** Le azioni, che si segnano chi è stato chiamato e con cosa. */
function azioni() {
  const fatte: [string, unknown][] = [];
  const segna =
    (nome: string) =>
    (arg: unknown): void => {
      fatte.push([nome, arg]);
    };
  const fai: AzioniBrani = {
    radio: segna("radio"),
    dopo: segna("dopo"),
    accoda: segna("accoda"),
    aggiungiAPlaylist: segna("aggiungiAPlaylist"),
    vaiAlDisco: segna("vaiAlDisco"),
    vaiAllArtista: segna("vaiAllArtista"),
    mostraNellaCartella: segna("mostraNellaCartella"),
    apriPagina: segna("apriPagina"),
    togli: segna("togli"),
    elimina: segna("elimina"),
  };
  return { fai, fatte };
}

const file = (id: number) => ({ id, fonte: null, fontePagina: null }) as unknown as Brano;
const catalogo = (id: number, pagina: string | null = "https://esempio") =>
  ({ id, fonte: "audius", fontePagina: pagina }) as unknown as Brano;
/** Un elenco in mano fatto di questi brani. */
const inMano = (...brani: Brano[]) => (id: number) => brani.find((b) => b.id === id);

test("un file solo: radio, i tre gesti, i tre «da qui», le due distruttive", () => {
  const { fai } = azioni();
  const voci = vociBrani([7], inMano(file(7)), fai, t).map((v) => v.etichetta);
  assert.deepEqual(voci, [
    "action.radio",
    "action.playNext",
    "action.enqueue",
    "action.addToPlaylist",
    "menu.goToAlbum",
    "menu.goToArtist",
    "menu.showInFolder",
    "menu.removeFromLibrary",
    "menu.deleteFromDisk",
  ]);
});

test("un brano di catalogo: la pagina e non la cartella, e niente «dal disco»", () => {
  const { fai } = azioni();
  const voci = vociBrani([7], inMano(catalogo(7)), fai, t).map((v) => v.etichetta);
  assert.ok(voci.includes("track.openPage"));
  assert.ok(!voci.includes("menu.showInFolder"));
  assert.ok(!voci.includes("menu.deleteFromDisk"));
  assert.equal(voci.at(-1), "menu.removeFromLibrary");
});

test("un catalogo senza pagina pubblica non promette la pagina", () => {
  const { fai } = azioni();
  const voci = vociBrani([7], inMano(catalogo(7, null)), fai, t).map((v) => v.etichetta);
  assert.ok(!voci.includes("track.openPage"));
  assert.ok(!voci.includes("menu.showInFolder"));
});

test("un brano che l'elenco non ha lascia le voci di prima", () => {
  const { fai } = azioni();
  const voci = vociBrani([7], inMano(), fai, t).map((v) => v.etichetta);
  assert.ok(voci.includes("menu.showInFolder"));
  assert.ok(!voci.includes("track.openPage"));
  assert.ok(voci.includes("menu.deleteFromDisk"));
});

test("più brani: niente radio né «da qui», e «dal disco» se uno solo è un file", () => {
  const { fai } = azioni();
  const misti = vociBrani([1, 2], inMano(catalogo(1), file(2)), fai, t).map(
    (v) => v.etichetta,
  );
  assert.deepEqual(misti, [
    "action.playNext",
    "action.enqueue",
    "action.addToPlaylist",
    "menu.removeFromLibrary",
    "menu.deleteFromDisk",
  ]);
  const tuttiCatalogo = vociBrani([1, 2], inMano(catalogo(1), catalogo(2)), fai, t);
  assert.equal(tuttiCatalogo.at(-1)?.etichetta, "menu.removeFromLibrary");
  // Uno che non si trova tiene la voce: non si sa che non sia un file.
  const unoIgnoto = vociBrani([1, 3], inMano(catalogo(1)), fai, t);
  assert.equal(unoIgnoto.at(-1)?.etichetta, "menu.deleteFromDisk");
});

test("ogni voce chiama la sua azione, con i brani giusti", () => {
  const { fai, fatte } = azioni();
  for (const voce of vociBrani([4], inMano(file(4)), fai, t)) voce.azione();
  assert.deepEqual(fatte, [
    ["radio", 4],
    ["dopo", [4]],
    ["accoda", [4]],
    ["aggiungiAPlaylist", [4]],
    ["vaiAlDisco", 4],
    ["vaiAllArtista", 4],
    ["mostraNellaCartella", 4],
    ["togli", [4]],
    ["elimina", [4]],
  ]);
});

test("una playlist a mano si rinomina, una automatica cambia le regole", () => {
  const fai = {
    suona: () => undefined,
    rinomina: () => undefined,
    regole: () => undefined,
    cancella: () => undefined,
    esporta: () => undefined,
  };
  const aMano = vociPlaylist({ isSmart: false } as Playlist, fai, t).map((v) => v.etichetta);
  assert.deepEqual(aMano, ["action.play", "menu.rename", "common.delete", "menu.exportFile"]);
  const automatica = vociPlaylist({ isSmart: true } as Playlist, fai, t).map(
    (v) => v.etichetta,
  );
  assert.deepEqual(automatica, [
    "action.play",
    "menu.editRules",
    "common.delete",
    "menu.exportFile",
  ]);
});

test("album e artista: le voci e il loro ordine", () => {
  const nulla = () => undefined;
  assert.deepEqual(
    vociAlbum({ radio: nulla, dopo: nulla, accoda: nulla }, t).map((v) => v.etichetta),
    ["action.radio", "action.playNext", "action.enqueue"],
  );
  assert.deepEqual(
    vociArtista({ apri: nulla, cercaPerNome: nulla }, t).map((v) => v.etichetta),
    ["menu.open", "menu.searchName"],
  );
});
