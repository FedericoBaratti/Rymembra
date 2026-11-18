/**
 * Le schede del telefono: quale si accende per una pagina, da dove riparte, e
 * dove porta Indietro quando la scala comune non ha più niente da chiudere.
 */
import { test } from "node:test";
import assert from "node:assert/strict";

import type { Album, Artista, Playlist } from "../src/ipc";
import { paginaCorrente } from "../src/pagina";
import {
  PILLOLE_LIBRERIA,
  eDellaLibreria,
  passoIndietroTelefono,
  radiceDellaScheda,
  schedaDi,
} from "../src/schedeTelefono";

const album = { albumKey: "a" } as Album;
const artista = { name: "x" } as Artista;
const playlist = { id: 1 } as Playlist;

test("un album resta nella scheda da cui lo si è aperto", () => {
  assert.equal(schedaDi({ tipo: "album", album }, "home"), "home");
  assert.equal(schedaDi({ tipo: "album", album }, "album"), "libreria");
  assert.equal(schedaDi({ tipo: "album", album }, "cerca"), "cerca");
  assert.equal(schedaDi({ tipo: "artista", artista }, "artisti"), "libreria");
  assert.equal(schedaDi({ tipo: "playlist", playlist }, "playlist"), "libreria");
});

test("ricerca, Esplora e il campo vuoto sono tutti di Cerca", () => {
  assert.equal(schedaDi({ tipo: "ricerca" }, "home"), "cerca");
  assert.equal(schedaDi({ tipo: "cerca" }, "cerca"), "cerca");
  assert.equal(schedaDi({ tipo: "esplora" }, "esplora"), "cerca");
});

test("Impostazioni e Importazioni stanno sotto Altro", () => {
  assert.equal(schedaDi({ tipo: "impostazioni" }, "impostazioni"), "altro");
  assert.equal(schedaDi({ tipo: "importazioni" }, "importazioni"), "altro");
  assert.equal(schedaDi({ tipo: "altro" }, "altro"), "altro");
});

test("la pagina di una vista del telefono", () => {
  const base = { cercando: false, playlist: null, album: null, artista: null };
  assert.deepEqual(paginaCorrente({ ...base, vista: "altro" }), { tipo: "altro" });
  // Di servizio: la casella della libreria non la copre, e un album aperto
  // prima resta sotto.
  assert.deepEqual(paginaCorrente({ ...base, vista: "impostazioni", cercando: true }), {
    tipo: "impostazioni",
  });
  assert.deepEqual(paginaCorrente({ ...base, vista: "cerca" }), { tipo: "cerca" });
  assert.deepEqual(paginaCorrente({ ...base, vista: "playlist" }), {
    tipo: "libreria",
    vista: "playlist",
  });
  // Scrivere nel campo di Cerca dà i risultati; un album aperto da lì gli sta sopra.
  assert.deepEqual(paginaCorrente({ ...base, vista: "cerca", cercando: true }), {
    tipo: "ricerca",
  });
  assert.equal(paginaCorrente({ ...base, vista: "cerca", album }).tipo, "album");
});

test("una scheda riparte da dove la si era lasciata", () => {
  assert.equal(radiceDellaScheda("libreria", "artisti", "cerca"), "artisti");
  assert.equal(radiceDellaScheda("cerca", "album", "esplora"), "esplora");
  assert.equal(radiceDellaScheda("home", "album", "cerca"), "home");
  assert.equal(radiceDellaScheda("altro", "album", "cerca"), "altro");
});

test("le pillole sono le viste della libreria, e solo loro", () => {
  for (const p of PILLOLE_LIBRERIA) assert.ok(eDellaLibreria(p));
  assert.ok(!eDellaLibreria("home"));
  assert.ok(!eDellaLibreria("cerca"));
  assert.ok(!eDellaLibreria("impostazioni"));
});

test("Indietro risale nella scheda, poi alla Home, poi esce", () => {
  const passo = (vista: Parameters<typeof passoIndietroTelefono>[0]["vista"], extra = {}) =>
    passoIndietroTelefono({ vista, playlistAperta: false, dentroLaSezione: false, ...extra });
  assert.equal(passo("brani", { playlistAperta: true }), "playlist");
  assert.equal(passo("impostazioni", { dentroLaSezione: true }), "sezioni");
  // Aperta da Altro, la sezione torna ad Altro, non all'elenco delle sezioni.
  assert.deepEqual(passo("impostazioni", { dentroLaSezione: true, sezioneDaFuori: true }), {
    vai: "altro",
  });
  assert.deepEqual(passo("impostazioni"), { vai: "altro" });
  assert.deepEqual(passo("importazioni"), { vai: "altro" });
  assert.deepEqual(passo("altro"), { vai: "home" });
  assert.deepEqual(passo("artisti"), { vai: "home" });
  assert.deepEqual(passo("esplora"), { vai: "home" });
  assert.equal(passo("home"), null);
});
