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
    apriSulWeb: segna("apriSulWeb"),
    togli: segna("togli"),
    elimina: segna("elimina"),
  };
  return { fai, fatte };
}

const file = (id: number) => ({ id, fonte: null, fontePagina: null }) as unknown as Brano;
const catalogo = (id: number, pagina: string | null = "https://esempio") =>
  ({ id, fonte: "audius", fontePagina: pagina }) as unknown as Brano;
/** Un brano del cloud: un file, ma su un servizio e non sul disco. */
const nuvola = (id: number, servizio = "onedrive") =>
  ({
    id,
    path: `nuvola://${servizio}-abcdefgh/Musica/${id}.flac`,
    fonte: null,
    fontePagina: null,
  }) as unknown as Brano;
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

test("una voce di «Ascolta insieme» non ha gesti di libreria", () => {
  const { fai } = azioni();
  assert.deepEqual(vociBrani([-3], inMano(file(-3)), fai, t), []);
});

test("in una selezione mista i gesti valgono solo per le righe della libreria", () => {
  const { fai, fatte } = azioni();
  const voci = vociBrani([7, -3], inMano(file(7), file(-3)), fai, t);
  // Con una sola riga di libreria il menù è quello di un brano solo: la radio
  // e i «da qui» tornano, perché il seme adesso non è ambiguo.
  assert.ok(voci.some((v) => v.etichetta === "action.radio"));
  voci.find((v) => v.etichetta === "action.enqueue")?.azione();
  assert.deepEqual(fatte, [["accoda", [7]]]);
});

test("in sessione la radio sparisce dal menù di un brano, e il resto resta", () => {
  const { fai } = azioni();
  const fuori = vociBrani([7], inMano(file(7)), fai, t).map((v) => v.etichetta);
  const dentro = vociBrani([7], inMano(file(7)), fai, t, true).map((v) => v.etichetta);
  assert.ok(fuori.includes("action.radio"));
  assert.ok(!dentro.includes("action.radio"));
  assert.deepEqual(
    dentro,
    fuori.filter((v) => v !== "action.radio"),
  );
});

test("in sessione la radio sparisce anche dal menù di un album", () => {
  const segna = () => undefined;
  const fai = { radio: segna, dopo: segna, accoda: segna };
  assert.deepEqual(
    vociAlbum(fai, t, true).map((v) => v.etichetta),
    ["action.playNext", "action.enqueue"],
  );
  assert.ok(vociAlbum(fai, t).some((v) => v.etichetta === "action.radio"));
});

test("senza la coda concessa da chi ospita, «Dopo» e «In coda» spariscono, e il resto resta", () => {
  const { fai } = azioni();
  const tutto = vociBrani([7], inMano(file(7)), fai, t, true).map((v) => v.etichetta);
  const senza = vociBrani([7], inMano(file(7)), fai, t, true, true).map((v) => v.etichetta);
  assert.ok(tutto.includes("action.playNext") && tutto.includes("action.enqueue"));
  assert.deepEqual(
    senza,
    tutto.filter((v) => v !== "action.playNext" && v !== "action.enqueue"),
  );
  const segna = () => undefined;
  const album = { radio: segna, dopo: segna, accoda: segna };
  assert.deepEqual(vociAlbum(album, t, true, true), [], "un menù vuoto non si apre");
});

test("un brano del cloud: «Apri su» al posto della cartella, ed «Elimina» spenta con la sua ragione", () => {
  const { fai, fatte } = azioni();
  // La finta di `t` scrive anche i valori, per vedere quale servizio si nomina.
  const tv = (chiave: string, valori?: Record<string, string | number>) =>
    valori === undefined ? chiave : `${chiave}:${Object.values(valori).join(",")}`;
  const voci = vociBrani([7], inMano(nuvola(7, "dropbox")), fai, tv);
  const etichette = voci.map((v) => v.etichetta);
  assert.ok(!etichette.includes("menu.showInFolder"));
  assert.ok(etichette.includes("menu.openOnService:Dropbox"));
  const elimina = voci.at(-1);
  assert.equal(elimina?.etichetta, "menu.deleteFromDisk");
  assert.equal(elimina?.spenta, "errors.cloud.readOnly");
  // Spenta vuol dire che premerla non fa niente.
  elimina?.azione();
  voci.find((v) => v.etichetta === "menu.openOnService:Dropbox")?.azione();
  assert.deepEqual(fatte, [["apriSulWeb", 7]]);
});

test("cloud e disco insieme: «Elimina dal disco» resta accesa, e decide il nucleo", () => {
  const { fai } = azioni();
  const misti = vociBrani([1, 2], inMano(nuvola(1), file(2)), fai, t);
  assert.equal(misti.at(-1)?.etichetta, "menu.deleteFromDisk");
  assert.equal(misti.at(-1)?.spenta, undefined);
  const tuttiNuvola = vociBrani([1, 2], inMano(nuvola(1), nuvola(2, "gdrive")), fai, t);
  assert.equal(tuttiNuvola.at(-1)?.spenta, "errors.cloud.readOnly");
  // Uno che non si trova: non si sa che sia nel cloud, la voce resta accesa.
  const unoIgnoto = vociBrani([1, 3], inMano(nuvola(1)), fai, t);
  assert.equal(unoIgnoto.at(-1)?.spenta, undefined);
});
