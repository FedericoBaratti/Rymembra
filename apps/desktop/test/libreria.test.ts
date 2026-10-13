import { test } from "node:test";
import assert from "node:assert/strict";

import type { Avvio, Playlist } from "../src/ipc";
import { conteggiDi, creaLibreria, type Domande } from "../src/libreria";

/** Una promessa che si risolve o fallisce quando lo dice il test. */
function sospesa<T>() {
  let risolvi!: (valore: T) => void;
  let rifiuta!: (errore: unknown) => void;
  const promessa = new Promise<T>((r, f) => {
    risolvi = r;
    rifiuta = f;
  });
  return { promessa, risolvi, rifiuta };
}

/** Un `avvio` con i numeri che servono e niente altro di vero. */
function avvio(brani: number, cartelle: string[] = ["C:/Musica"]): Avvio {
  return {
    dataDir: "C:/dati",
    migrazioni: 30,
    fts5: true,
    cartelle,
    cartellaDownload: null,
    tema: null,
    lingua: null,
    scorciatoie: null,
    numeri: { tracks: brani, albums: 2, artists: 3, liked: 4 },
  } as unknown as Avvio;
}

/** Una libreria con le domande in mano al test, una coda per domanda. */
function libreriaFinta() {
  const avvii: ReturnType<typeof sospesa<Avvio>>[] = [];
  const elenchi: ReturnType<typeof sospesa<Playlist[]>>[] = [];
  const domande: Domande = {
    avvio: () => {
      const s = sospesa<Avvio>();
      avvii.push(s);
      return s.promessa;
    },
    playlistElenco: () => {
      const s = sospesa<Playlist[]>();
      elenchi.push(s);
      return s.promessa;
    },
  };
  const errori: unknown[] = [];
  let lette = 0;
  const libreria = creaLibreria(domande, {
    onErrore: (e) => errori.push(e),
    onLetta: () => (lette += 1),
  });
  return { libreria, avvii, elenchi, errori, lette: () => lette };
}

test("vince l'ultima: la risposta vecchia arrivata dopo si butta", async () => {
  const { libreria, avvii, lette } = libreriaFinta();
  const prima = libreria.ricarica();
  const seconda = libreria.ricarica();
  avvii[1]?.risolvi(avvio(10));
  await seconda;
  avvii[0]?.risolvi(avvio(5));
  await prima;
  assert.equal(libreria.avvio.leggi()?.numeri.tracks, 10);
  assert.equal(lette(), 1);
});

test("l'errore di una lettura superata non si mostra, quello dell'ultima sì", async () => {
  const { libreria, avvii, errori } = libreriaFinta();
  const prima = libreria.ricarica();
  const seconda = libreria.ricarica();
  avvii[1]?.risolvi(avvio(1));
  await seconda;
  avvii[0]?.rifiuta(new Error("vecchio"));
  await prima;
  assert.deepEqual(errori, []);
  const terza = libreria.ricarica();
  avvii[2]?.rifiuta(new Error("nuovo"));
  await terza;
  assert.equal(errori.length, 1);
  assert.equal(libreria.avvio.leggi()?.numeri.tracks, 1);
});

test("le playlist hanno il loro turno, e non disturbano quello di avvio", async () => {
  const { libreria, avvii, elenchi } = libreriaFinta();
  const lettura = libreria.ricarica();
  const vecchie = libreria.ricaricaPlaylist();
  const nuove = libreria.ricaricaPlaylist();
  const una = { id: 1 } as Playlist;
  const due = { id: 2 } as Playlist;
  elenchi[1]?.risolvi([una, due]);
  await nuove;
  elenchi[0]?.risolvi([una]);
  await vecchie;
  avvii[0]?.risolvi(avvio(7));
  await lettura;
  assert.deepEqual(
    libreria.playlist.leggi().map((p) => p.id),
    [1, 2],
  );
  assert.equal(libreria.avvio.leggi()?.numeri.tracks, 7);
});

test("chi legge le playlist non è svegliato da avvio, e viceversa", async () => {
  const { libreria, avvii, elenchi } = libreriaFinta();
  let perAvvio = 0;
  let perPlaylist = 0;
  libreria.avvio.iscrivi(() => (perAvvio += 1));
  libreria.playlist.iscrivi(() => (perPlaylist += 1));
  const lettura = libreria.ricarica();
  avvii[0]?.risolvi(avvio(3));
  await lettura;
  const elenco = libreria.ricaricaPlaylist();
  elenchi[0]?.risolvi([]);
  await elenco;
  assert.equal(perAvvio, 1);
  assert.equal(perPlaylist, 1);
});

test("i conteggi: niente prima di avvio, e le radici per le cartelle", () => {
  assert.deepEqual(conteggiDi(null), {});
  assert.deepEqual(conteggiDi(avvio(9, ["C:/A", "D:/B"])), {
    album: 2,
    artisti: 3,
    brani: 9,
    cartelle: 2,
    preferiti: 4,
  });
});
