/**
 * Una riga di un dispositivo spento che ha una copia di qui suona, e non è grigia
 * (D1 del collaudo del 18/11, FC1).
 *
 * Con A spento, il tocco su un doppione che la riga di A rappresenta dava «non
 * raggiungibile», anche se B — «questo» dispositivo — ha la copia. Per §1 la copia
 * migliore si suona sempre, e il nucleo ha il ripiego (`copia_da_suonare`): era la
 * finestra a bloccare il tocco prima di chiamarlo, perché giudicava la riga dal
 * solo dispositivo. Adesso la riga porta un fatto in più — `copiaQui`, che il nucleo
 * calcola — e la domanda «si suona adesso?» è una sola, `rigaSuonabile`, per il
 * tocco, per «Riproduci» su un album o un artista, e per Cerca.
 *
 * `spenta` resta quel che era — il dispositivo non risponde — perché «Scarica qui»
 * deve chiedergli qualcosa e non può.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import type { DispositivoLontano, StatoLontano } from "../src/ipc";
import {
  primoSpento,
  rigaSuonabile,
  statoDellaRiga,
  suonabili,
} from "../src/origine";

const PC = "aa".repeat(32);

function pc(online: boolean): StatoLontano {
  const dispositivo: DispositivoLontano = {
    chiave: PC,
    nome: "Casa",
    piattaforma: "windows",
    dispositivo: "",
    online,
    vistoMs: null,
    associatoMs: 0,
    revocato: false,
    versione: null,
    insiemeAcceso: false,
    insiemeOspita: false,
    insieme: null,
    via: online ? "diretto" : null,
  };
  return {
    attivo: true,
    acceso: true,
    chiave: null,
    impronta: null,
    relay: null,
    datiMobili: "chiedi",
    sottofondo: false,
    errore: null,
    dispositivi: [dispositivo],
    invito: null,
    richieste: [],
  };
}

const percorso = (id: number) => `lontano://${PC}/${id}`;
const riga = (id: number, copiaQui = false) => ({ path: percorso(id), copiaQui });

test("una riga di un PC spento con una copia di qui è suonabile, ma il PC resta spento", () => {
  const spento = pc(false);
  const senza = statoDellaRiga(percorso(1), spento);
  assert.equal(senza?.spenta, true);
  assert.equal(rigaSuonabile(senza), false, "senza copia di qui: «non raggiungibile»");

  const con = statoDellaRiga(percorso(1), spento, false, true);
  assert.equal(con?.spenta, true, "il dispositivo non risponde: «Scarica qui» non può chiedergli niente");
  assert.equal(con?.copiaQui, true);
  assert.equal(rigaSuonabile(con), true, "ma di qui c'è la copia, e suona");
});

test("una riga di qui, o di un PC acceso, è sempre suonabile; una in grazia mai", () => {
  assert.equal(rigaSuonabile(statoDellaRiga("C:/Musica/uno.mp3", pc(false))), true);
  assert.equal(rigaSuonabile(statoDellaRiga(percorso(1), pc(true))), true);
  // In grazia il dispositivo non manda più il brano: la copia di qui non c'entra.
  const grazia = statoDellaRiga(percorso(1), pc(true), true, true);
  assert.equal(grazia?.inGrazia, true);
  assert.equal(grazia?.copiaQui, undefined);
  assert.equal(rigaSuonabile(grazia), false);
});

test("Riproduci salta le righe senza copia di qui e tiene quelle con la copia", () => {
  const spento = pc(false);
  const lista = [riga(1), riga(2, true), riga(3), riga(4, true)];
  const { brani, indice } = suonabili(lista, 0, spento);
  assert.deepEqual(
    brani.map((b) => b.path),
    [percorso(2), percorso(4)],
  );
  assert.equal(indice, 0);
  // Da una riga che resta, l'indice segue il suo posto.
  assert.equal(suonabili(lista, 3, spento).indice, 1);
});

test("una lista fatta solo di righe con la copia di qui non è «non raggiungibile»", () => {
  const spento = pc(false);
  const lista = [riga(1, true), riga(2, true)];
  assert.deepEqual(suonabili(lista, 0, spento).brani, lista);
  // E «Scarica qui», che vuole il dispositivo, lo vede ancora spento.
  assert.equal(primoSpento(lista, spento)?.nome, "Casa");
});

test("la finestra passa copiaQui al giudizio del tocco e della riga, e il nucleo la calcola", () => {
  const leggi = (percorsoFile: string) =>
    readFileSync(new URL(percorsoFile, import.meta.url), "utf8").replace(/\r\n/g, "\n");
  const tipo = leggi("../src/ipc/libreria.ts");
  assert.match(tipo, /copiaQui\?: boolean;/);
  const nucleo = leggi("../../../core/rymembra-app/src/library/interrogazioni.rs");
  assert.match(nucleo, /pub copia_qui: bool,/);
});
