import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { CODA_VUOTA, laCodaChiesta, laCodaDelloStato, statoConLaCoda } from "../src/codaTenuta";
import type { StatoRiproduzioneEvento } from "../src/ipc";

/*
 * S7-5: il nucleo non manda più la coda intera a ogni `riproduzione:stato`. La
 * finestra tiene l'ultima ricevuta con la sua versione (`codaTenuta.ts`), la
 * rimette nello stato che disegna, e se manca la chiede.
 */

const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");

/** Un evento di stato, col solo che serve alle prove: la coda e la sua versione. */
function evento(versioneCoda: number, coda?: number[]): StatoRiproduzioneEvento {
  return { versioneCoda, ...(coda === undefined ? {} : { coda }) } as unknown as StatoRiproduzioneEvento;
}

test("un evento con la coda nuova la sostituisce, e si tiene la sua versione", () => {
  const esito = laCodaDelloStato(CODA_VUOTA, evento(5, [1, 2, 3]));
  assert.deepEqual(esito.coda, [1, 2, 3]);
  assert.deepEqual(esito.tenuta, { versione: 5, coda: [1, 2, 3] });
  assert.equal(esito.manca, false);
});

test("un evento senza la coda, con la stessa versione, lascia lo stesso array", () => {
  // La lista della coda confronta per identità: un array nuovo a ogni stato la
  // farebbe ridisegnare a ogni pausa.
  const tenuta = { versione: 5, coda: [1, 2, 3] };
  const esito = laCodaDelloStato(tenuta, evento(5));
  assert.equal(esito.coda, tenuta.coda, "stesso array, non una copia");
  assert.equal(esito.tenuta, tenuta);
  assert.equal(esito.manca, false);
});

test("una versione più nuova senza la coda vuol dire che quella che la portava è andata persa", () => {
  const tenuta = { versione: 5, coda: [1, 2, 3] };
  const esito = laCodaDelloStato(tenuta, evento(6));
  assert.equal(esito.manca, true, "la finestra deve chiedere la coda");
  assert.equal(esito.coda, tenuta.coda, "intanto resta quella che ha");
  assert.equal(esito.tenuta, tenuta);
});

test("un evento in ritardo non fa tornare indietro la coda", () => {
  const tenuta = { versione: 7, coda: [9, 8] };
  const con = laCodaDelloStato(tenuta, evento(6, [1, 2, 3]));
  assert.equal(con.coda, tenuta.coda, "una coda più vecchia non sostituisce quella in mano");
  assert.equal(con.manca, false);
  const senza = laCodaDelloStato(tenuta, evento(6));
  assert.equal(senza.coda, tenuta.coda);
  assert.equal(senza.manca, false, "una versione più vecchia non fa chiedere niente");
});

test("la coda chiesta si tiene solo se è più nuova di quella in mano", () => {
  const tenuta = { versione: 5, coda: [1] };
  assert.equal(laCodaChiesta(tenuta, { versione: 5, coda: [1] }), null);
  assert.equal(laCodaChiesta(tenuta, { versione: 4, coda: [0] }), null);
  assert.deepEqual(laCodaChiesta(tenuta, { versione: 6, coda: [1, 2] }), {
    versione: 6,
    coda: [1, 2],
  });
});

test("lo stato che si disegna porta sempre la coda, anche se l'evento no", () => {
  const stato = statoConLaCoda(evento(5), [4, 5]);
  assert.deepEqual(stato.coda, [4, 5]);
  assert.equal(stato.versioneCoda, 5);
});

test("S7-5: la finestra rimette la coda in un posto solo, e la chiede se le manca", () => {
  const riproduzione = leggi("../src/riproduzione.ts");
  // L'evento e la prima fotografia passano tutti e due da `conLaCoda`.
  assert.match(riproduzione, /const stato = conLaCoda\(carico\);\s*setStato\(stato\);/);
  assert.match(riproduzione, /const stato = conLaCoda\(iniziale\);\s*setStato\(stato\);/);
  assert.match(riproduzione, /useAscolto<StatoRiproduzioneEvento>\(\s*"riproduzione:stato"/);
  assert.ok(riproduzione.includes("ipc\n        .codaDelLettore()"), "la coda mancante si chiede");
  // Nessun altro posto ascolta lo stato intero come se la coda ci fosse sempre.
  for (const file of ["../src/parti/UscitaAudio.tsx"]) {
    assert.doesNotMatch(leggi(file), /useAscolto<StatoRiproduzione>\("riproduzione:stato"/);
  }
  const ipc = leggi("../src/ipc.ts");
  assert.ok(ipc.includes('invoke<CodaDelLettore>("riproduzione_coda")'));
});
