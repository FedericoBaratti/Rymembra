import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import type { EsitoAccount, EsitoImport } from "../src/ipc/importazioni";
import { accantoAccount, riscritteAccount } from "../src/pianoAccount";
import { importabile } from "../src/pianoFilePlaylist";

/*
 * Le playlist omonime nelle importazioni (D-11, R06-3, R13c-1, R13c-2).
 *
 * Il nucleo non sostituisce più una playlist fatta qui: quella importata le
 * entra accanto, «Chill (Spotify)». Il piano lo deve dire per nome, e dire
 * anche quali playlist importate la volta prima verranno riscritte. Per i file
 * di playlist il piano è quello del nome scritto nel campo.
 */
const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");

function elenco(nome: string, campi: Partial<EsitoImport> = {}): EsitoImport {
  return {
    kind: "playlist",
    source: "archivio",
    sourceId: `id-${nome}`,
    title: nome,
    resolved: 1,
    matched: 1,
    missing: 0,
    matchedIsrc: 0,
    matchedExact: 1,
    matchedByTitle: 0,
    matchedStripped: 0,
    missingTracks: [],
    playlistId: 1,
    playlistName: nome,
    playlistCreated: true,
    playlistReplaced: false,
    playlistBeside: null,
    playlistEntries: 1,
    spotifyAlbumIdsWritten: 0,
    isrcWritten: 0,
    wantedRows: 0,
    playlistRestored: 0,
    wantedClosed: 0,
    truncated: null,
    ...campi,
  };
}

function esito(playlists: EsitoImport[]): EsitoAccount {
  return {
    source: "archivio",
    profile: null,
    spotifyUserId: null,
    fullHistory: true,
    playlists,
    rejectedPlaylists: [],
    liked: elenco("preferiti", { kind: "preferiti", playlistId: null, playlistName: null }),
    likedMarked: 0,
    likedKeptOff: 0,
    albums: [],
    albumsSeen: 0,
    albumIdsWritten: 0,
    artistsSeen: 0,
    artistsLinked: 0,
    historyRows: 0,
    historySkipped: { duplicates: 0, tooShort: 0, notInLibrary: 0 },
    statsUpdated: 0,
    playlistRestored: 0,
    wantedClosed: 0,
    matchedDistinct: 0,
  };
}

test("il piano dell'account dice quali playlist entrano accanto e quali si riscrivono", () => {
  const piano = esito([
    elenco("Chill (Spotify)", { playlistBeside: "Chill" }),
    elenco("Corsa", { playlistCreated: false, playlistReplaced: true }),
    elenco("Nuova"),
  ]);
  assert.deepEqual(accantoAccount(piano), [{ qui: "Chill", nuova: "Chill (Spotify)" }]);
  assert.deepEqual(riscritteAccount(piano), ["Corsa"]);
  // Un rapporto salvato prima non porta il campo: niente da dire.
  assert.deepEqual(accantoAccount(esito([elenco("Vecchia", { playlistBeside: undefined })])), []);
});

test("la finestrella dell'account mostra le playlist accanto e quelle riscritte", () => {
  const account = leggi("../src/Account.tsx");
  assert.match(account, /accantoAccount\(piano\)/);
  assert.match(account, /riscritteAccount\(piano\)/);
  assert.match(account, /t\("account\.beside"/);
  assert.match(account, /t\("account\.rewritten"/);
});

test("l'importazione da un link e il vecchio database dicono la playlist accanto", () => {
  const importa = leggi("../src/Importa.tsx");
  assert.match(importa, /piano\?\.playlistBeside != null/);
  assert.match(importa, /rapporto\.playlistsBeside\.length > 0/);
  assert.match(importa, /rapporto\.playlistsSkipped\.length > 0/);
});

test("il piano dell'account conta i ritrovati distinti e riparte pulito", () => {
  // R13c-12.
  const account = leggi("../src/Account.tsx");
  assert.match(account, /etichetta=\{t\("account\.found"\)\}\s+valore=\{piano\.matchedDistinct\}/);
  const effetto = account.slice(account.indexOf(".accountPiano(scelte)") - 600, account.indexOf(".accountPiano(scelte)"));
  assert.match(effetto, /setPiano\(null\);\s+setErrore\(null\);/);
});

test("un file di playlist si importa solo con il piano del nome scritto", () => {
  const base = { inCorso: false, nome: "Rock", nomeDelPiano: "Rock", ritrovati: 3 };
  assert.equal(importabile(base), true);
  // Il nome è cambiato e il piano nuovo non è arrivato: «Importa» aspetta.
  assert.equal(importabile({ ...base, nome: "Rock 2" }), false);
  assert.equal(importabile({ ...base, nomeDelPiano: null }), false);
  assert.equal(importabile({ ...base, inCorso: true }), false);
  assert.equal(importabile({ ...base, nome: "  ", nomeDelPiano: "  " }), false);
  assert.equal(importabile({ ...base, ritrovati: 0 }), false);
});

test("ImportaPlaylist rifà il piano per il nome del campo e lo usa negli avvisi", () => {
  const finestrella = leggi("../src/ImportaPlaylist.tsx");
  assert.match(finestrella, /\.playlistFilePiano\(percorso, nome\)/);
  assert.match(finestrella, /importabile\(\{/);
  // L'avviso nomina la playlist del piano, non quel che si sta scrivendo.
  const avviso = finestrella.slice(finestrella.indexOf('k="plfile.replaced"'));
  assert.match(avviso.slice(0, 200), /nome: <strong>\{rapporto\.name\}<\/strong>/);
  assert.match(finestrella, /k="plfile\.beside"/);
});
