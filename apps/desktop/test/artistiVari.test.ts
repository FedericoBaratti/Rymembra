import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * L'artista di una raccolta senza artista del disco (R07-4, D-63).
 *
 * Il nucleo scrive il segnaposto `VARIOUS_ARTISTS` come artista dell'album, e
 * la finestra lo riconosce al disegno, come fa con «Artista sconosciuto»: se
 * i due testi divergessero, la griglia mostrerebbe il valore crudo in tutte e
 * due le lingue.
 */
const ALBUM = readFileSync(
  new URL("../../../core/rymembra-domain/src/album.rs", import.meta.url),
  "utf8",
);
const FORMATO = readFileSync(new URL("../src/formato.ts", import.meta.url), "utf8");

test("la finestra riconosce il segnaposto che il nucleo scrive", () => {
  const nucleo = /pub const VARIOUS_ARTISTS: &str = "([^"]+)";/.exec(ALBUM)?.[1];
  const finestra = /const ARTISTI_VARI = "([^"]+)";/.exec(FORMATO)?.[1];
  assert.ok(nucleo, "non trovo VARIOUS_ARTISTS in album.rs");
  assert.equal(finestra, nucleo);
  const nome = FORMATO.slice(FORMATO.indexOf("export function nomeArtista("));
  assert.match(
    nome.slice(0, nome.indexOf("\n}")),
    /nome === ARTISTI_VARI\) return t\("format\.variousArtists"\)/,
  );
});

test("e lo traduce in tutte e due le lingue", () => {
  for (const [lingua, atteso] of [
    ["it", "Artisti vari"],
    ["en", "Various artists"],
  ] as const) {
    const testi = JSON.parse(
      readFileSync(new URL(`../src/lingue/${lingua}.json`, import.meta.url), "utf8"),
    ) as Record<string, string>;
    assert.equal(testi["format.variousArtists"], atteso, lingua);
  }
});
