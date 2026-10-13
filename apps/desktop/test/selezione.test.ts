import { test } from "node:test";
import assert from "node:assert/strict";

import type { Album, Artista, Playlist } from "../src/ipc";
import {
  chiaveDellaSelezione,
  gestoDelClic,
  nellOrdineDellaVista,
  type ClicSullaRiga,
} from "../src/selezione";

const semplice = (indice: number): ClicSullaRiga => ({
  indice,
  maiuscolo: false,
  controllo: false,
});
const conMaiuscolo = (indice: number): ClicSullaRiga => ({
  indice,
  maiuscolo: true,
  controllo: false,
});
const conControllo = (indice: number): ClicSullaRiga => ({
  indice,
  maiuscolo: false,
  controllo: true,
});

/** Una sequenza di clic, con l'àncora che passa da uno all'altro. */
function sequenza(clic: ClicSullaRiga[], ancora: number | null = null) {
  const gesti = [];
  let adesso = ancora;
  for (const c of clic) {
    const esito = gestoDelClic(c, adesso);
    gesti.push(esito.gesto);
    adesso = esito.ancora;
  }
  return { gesti, ancora: adesso };
}

test("il clic semplice sposta l'àncora: Maiusc+clic dopo parte da lì", () => {
  // Il difetto: Ctrl sulla riga 3, clic semplice sulla 40, Maiusc+clic sulla
  // 45. L'intervallo partiva dalla 3 — trentotto righe in più, quasi tutte
  // fuori schermo — invece che dalla 40.
  const { gesti } = sequenza([conControllo(3), semplice(40), conMaiuscolo(45)]);
  assert.deepEqual(gesti[2], { tipo: "intervallo", da: 40, a: 45 });
});

test("il primo clic semplice in una vista basta a fare da àncora", () => {
  const { gesti } = sequenza([semplice(10), conMaiuscolo(4)]);
  assert.deepEqual(gesti, [
    { tipo: "semplice" },
    { tipo: "intervallo", da: 4, a: 10 },
  ]);
});

test("Maiusc+clic non sposta l'àncora: il secondo corregge il primo", () => {
  const { gesti, ancora } = sequenza([semplice(20), conMaiuscolo(30), conMaiuscolo(25)]);
  assert.deepEqual(gesti[1], { tipo: "intervallo", da: 20, a: 30 });
  assert.deepEqual(gesti[2], { tipo: "intervallo", da: 20, a: 25 });
  assert.equal(ancora, 20);
});

test("Ctrl+clic alterna la riga e diventa la nuova àncora", () => {
  const { gesti, ancora } = sequenza([semplice(1), conControllo(7)]);
  assert.deepEqual(gesti[1], { tipo: "alterna" });
  assert.equal(ancora, 7);
});

test("Maiusc senza àncora vale come un clic semplice, e la mette", () => {
  const esito = gestoDelClic(conMaiuscolo(12), null);
  assert.deepEqual(esito, { gesto: { tipo: "semplice" }, ancora: 12 });
});

test("Maiusc vince su Ctrl quando c'è un'àncora", () => {
  const esito = gestoDelClic({ indice: 9, maiuscolo: true, controllo: true }, 2);
  assert.deepEqual(esito, { gesto: { tipo: "intervallo", da: 2, a: 9 }, ancora: 2 });
});

test("le posizioni sono quelle della vista, anche lontane dalle righe in mano", () => {
  // Una lista rada: la riga 48.000 non è caricata, ma la sua posizione è
  // quella, e l'intervallo si conta su quella.
  const { gesti } = sequenza([semplice(48_000), conMaiuscolo(47_990)]);
  assert.deepEqual(gesti[1], { tipo: "intervallo", da: 47_990, a: 48_000 });
});

test("la chiave dice di quale elenco è la selezione", () => {
  const playlist = { id: 3, updatedAt: 17 } as Playlist;
  const album = { albumKey: "a|b" } as Album;
  const artista = { name: "Nina" } as Artista;
  // Le viste tenute montate: i Brani con l'ordinamento, le altre per nome.
  assert.equal(chiaveDellaSelezione({ tipo: "libreria", vista: "brani" }, "brani", "titolo", ""), "brani:titolo");
  assert.equal(chiaveDellaSelezione({ tipo: "libreria", vista: "album" }, "album", "titolo", ""), "album");
  // Le altre pagine, ciascuna con quel che la distingue.
  assert.equal(chiaveDellaSelezione({ tipo: "ricerca" }, null, "titolo", "sub"), "ricerca:sub");
  assert.equal(chiaveDellaSelezione({ tipo: "playlist", playlist }, null, "titolo", ""), "playlist:3:17");
  assert.equal(chiaveDellaSelezione({ tipo: "album", album }, null, "titolo", ""), "album:a|b");
  assert.equal(chiaveDellaSelezione({ tipo: "artista", artista }, null, "titolo", ""), "artista:Nina");
  assert.equal(chiaveDellaSelezione({ tipo: "libreria", vista: "cartelle" }, null, "titolo", ""), "cartelle");
  assert.equal(chiaveDellaSelezione({ tipo: "home" }, null, "titolo", ""), "home");
});

test("i selezionati nell'ordine della vista, piena o rada", () => {
  const scelti = new Map([
    [30, 2],
    [10, 0],
    [20, 1],
  ]);
  // Rada: per la posizione ricordata.
  assert.deepEqual(nellOrdineDellaVista(scelti, null), [10, 20, 30]);
  // Piena: nell'ordine delle righe, doppioni compresi.
  assert.deepEqual(
    nellOrdineDellaVista(scelti, [{ id: 20 }, { id: 99 }, { id: 30 }, { id: 20 }]),
    [20, 30, 20],
  );
});
