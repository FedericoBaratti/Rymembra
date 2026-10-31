import { test } from "node:test";
import assert from "node:assert/strict";

import { vinceLUltima } from "../src/ultima";

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

/**
 * Una `ricarica` come quella di `App.tsx`: chiede, e scrive la risposta — o
 * l'errore — solo se è ancora la domanda più recente.
 */
function ricaricatore() {
  const turno = vinceLUltima();
  const scritti: string[] = [];
  const errori: unknown[] = [];
  const ricarica = async (domanda: Promise<string>) => {
    const mio = turno();
    try {
      const arrivato = await domanda;
      if (mio()) scritti.push(arrivato);
    } catch (e) {
      if (mio()) errori.push(e);
    }
  };
  return { ricarica, scritti, errori };
}

test("una risposta vecchia arrivata dopo una nuova si scarta", async () => {
  const { ricarica, scritti } = ricaricatore();
  const vecchia = sospesa<string>();
  const nuova = sospesa<string>();
  const prima = ricarica(vecchia.promessa);
  const seconda = ricarica(nuova.promessa);
  nuova.risolvi("dopo la scansione");
  await seconda;
  vecchia.risolvi("durante la scansione");
  await prima;
  assert.deepEqual(scritti, ["dopo la scansione"]);
});

test("nell'ordine giusto si scrive l'ultima, e la prima no", async () => {
  const { ricarica, scritti } = ricaricatore();
  const vecchia = sospesa<string>();
  const nuova = sospesa<string>();
  const prima = ricarica(vecchia.promessa);
  const seconda = ricarica(nuova.promessa);
  // La vecchia arriva per prima ma è già superata: scriverla vorrebbe dire un
  // fotogramma di libreria vecchia prima della nuova.
  vecchia.risolvi("vecchia");
  await prima;
  nuova.risolvi("nuova");
  await seconda;
  assert.deepEqual(scritti, ["nuova"]);
});

test("una domanda sola si scrive sempre", async () => {
  const { ricarica, scritti } = ricaricatore();
  await ricarica(Promise.resolve("unica"));
  await ricarica(Promise.resolve("e poi un'altra"));
  assert.deepEqual(scritti, ["unica", "e poi un'altra"]);
});

test("l'errore di una domanda superata non si mostra", async () => {
  const { ricarica, scritti, errori } = ricaricatore();
  const vecchia = sospesa<string>();
  const nuova = sospesa<string>();
  const prima = ricarica(vecchia.promessa);
  const seconda = ricarica(nuova.promessa);
  nuova.risolvi("letta bene");
  await seconda;
  vecchia.rifiuta(new Error("db.locked"));
  await prima;
  assert.deepEqual(scritti, ["letta bene"]);
  assert.deepEqual(errori, []);
});

test("l'errore dell'ultima domanda invece sì", async () => {
  const { ricarica, errori } = ricaricatore();
  const guasto = new Error("db.queryFailed");
  await ricarica(Promise.reject(guasto));
  assert.deepEqual(errori, [guasto]);
});

test("due contatori non si disturbano", () => {
  const avvio = vinceLUltima();
  const playlist = vinceLUltima();
  const mioAvvio = avvio();
  playlist();
  playlist();
  assert.equal(mioAvvio(), true);
});

test("una fotografia più vecchia non sovrascrive un evento arrivato prima di lei", async () => {
  const { eventiEFotografia } = await import("../src/ultima");
  assert.equal(typeof eventiEFotografia, "function");
  const conto = eventiEFotografia();
  // La fotografia chiesta, poi un evento più nuovo, poi la risposta: si butta.
  const prima = conto.chiedi();
  conto.evento();
  assert.equal(prima(), false);
  // Chiesta dopo l'ultimo evento, e nessun evento nel mezzo: si adotta.
  const seconda = conto.chiedi();
  assert.equal(seconda(), true);
  // Due contatori non si disturbano.
  const altro = eventiEFotografia();
  altro.evento();
  assert.equal(seconda(), true);
});
