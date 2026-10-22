/**
 * La pagina in una stringa: quella che rialza il recinto del corpo
 * (`Recinto.tsx`) quando si cambia pagina.
 */
import { test } from "node:test";
import assert from "node:assert/strict";

import type { Album, Artista, Playlist } from "../src/ipc";
import { chiaveDiPagina } from "../src/pagina";

test("due album diversi sono due pagine diverse", () => {
  const album = (albumKey: string) => ({ tipo: "album", album: { albumKey } as Album }) as const;
  assert.notEqual(chiaveDiPagina(album("a")), chiaveDiPagina(album("b")));
  assert.equal(chiaveDiPagina(album("a")), chiaveDiPagina(album("a")));
});

test("playlist, artisti e viste della libreria si distinguono fra loro", () => {
  const chiavi = [
    chiaveDiPagina({ tipo: "playlist", playlist: { id: 1 } as Playlist }),
    chiaveDiPagina({ tipo: "playlist", playlist: { id: 2 } as Playlist }),
    chiaveDiPagina({ tipo: "artista", artista: { name: "1" } as Artista }),
    chiaveDiPagina({ tipo: "libreria", vista: "brani" }),
    chiaveDiPagina({ tipo: "libreria", vista: "preferiti" }),
    chiaveDiPagina({ tipo: "home" }),
    chiaveDiPagina({ tipo: "ricerca" }),
    chiaveDiPagina({ tipo: "impostazioni" }),
  ];
  assert.equal(new Set(chiavi).size, chiavi.length);
});
