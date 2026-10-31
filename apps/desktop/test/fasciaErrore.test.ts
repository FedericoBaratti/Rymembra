import { test } from "node:test";
import assert from "node:assert/strict";

import type { Avvio, Playlist } from "../src/ipc";
import * as modulo from "../src/libreria";

/*
 * La fascia d'errore e le riletture di `avvio`.
 *
 * Una rilettura riuscita toglieva **qualunque** guasto a schermo, anche uno
 * che con `avvio` non c'entrava: una scansione a mano che cade a metà manda
 * `cartelle:cambiate` e poi l'errore, l'evento fa rileggere `avvio` cinquanta
 * millesimi dopo, e la fascia rossa spariva prima che la si leggesse. Qui si
 * finge `App`: il guasto a schermo, quello messo dalla libreria, e la regola
 * `dopoLaLettura` che decide chi si toglie.
 */

function sospesa<T>() {
  let risolvi!: (valore: T) => void;
  let rifiuta!: (errore: unknown) => void;
  const promessa = new Promise<T>((r, f) => {
    risolvi = r;
    rifiuta = f;
  });
  return { promessa, risolvi, rifiuta };
}

const AVVIO = { numeri: { tracks: 1, albums: 1, artists: 1, liked: 0 } } as unknown as Avvio;

/** `App` in piccolo: la fascia, e il guasto che ci ha messo la libreria. */
function finestra() {
  const avvii: ReturnType<typeof sospesa<Avvio>>[] = [];
  const fascia: { errore: unknown } = { errore: null };
  let dellaLibreria: unknown = null;
  const libreria = modulo.creaLibreria(
    {
      avvio: () => {
        const s = sospesa<Avvio>();
        avvii.push(s);
        return s.promessa;
      },
      playlistElenco: () => Promise.resolve([] as Playlist[]),
    },
    {
      onErrore: (e) => {
        dellaLibreria = e;
        fascia.errore = e;
      },
      onLetta: (tutto) => {
        fascia.errore = modulo.dopoLaLettura(fascia.errore, dellaLibreria, tutto);
      },
    },
  );
  return { libreria, avvii, fascia };
}

test("una fascia d'errore estranea sopravvive alla rilettura nata da un evento", async () => {
  assert.equal(typeof modulo.dopoLaLettura, "function");
  const { libreria, avvii, fascia } = finestra();
  const scansione = new Error("db.locked durante la scansione");
  fascia.errore = scansione;
  // `cartelle:cambiate` → `accorpa` → la rilettura dei soli eventi.
  const letta = libreria.rileggi();
  avvii[0]?.risolvi(AVVIO);
  await letta;
  assert.equal(fascia.errore, scansione);
});

test("una seconda scansione riuscita toglie il guasto della prima", async () => {
  const { libreria, avvii, fascia } = finestra();
  fascia.errore = new Error("la prima scansione");
  // `scansiona` riletta a mano dopo il gesto: un tentativo nuovo riuscito.
  const letta = libreria.ricarica();
  avvii[0]?.risolvi(AVVIO);
  await letta;
  assert.equal(fascia.errore, null);
});

test("il guasto di `avvio` si toglie anche dalla rilettura di un evento", async () => {
  const { libreria, avvii, fascia } = finestra();
  const fallita = libreria.ricarica();
  const guasto = new Error("avvio non risponde");
  avvii[0]?.rifiuta(guasto);
  await fallita;
  assert.equal(fascia.errore, guasto);
  const letta = libreria.rileggi();
  avvii[1]?.risolvi(AVVIO);
  await letta;
  assert.equal(fascia.errore, null);
});

test("la regola, da sola", () => {
  const { dopoLaLettura } = modulo;
  const a = { codice: "a" };
  const b = { codice: "b" };
  assert.equal(dopoLaLettura(a, a, false), null);
  assert.equal(dopoLaLettura(b, a, false), b);
  assert.equal(dopoLaLettura(b, a, true), null);
  assert.equal(dopoLaLettura(null, a, false), null);
});
