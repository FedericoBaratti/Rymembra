import { test } from "node:test";
import assert from "node:assert/strict";

import { creaFreno } from "../src/eq";

/*
 * Il freno degli invii dell'equalizzatore (R13-9).
 *
 * Un cursore trascinato manda al nucleo al più una curva ogni `passo`
 * millisecondi: la prima subito, le altre tenute da parte, e solo l'ultima
 * parte allo scadere. Chiudere il pannello entro quel passo smontava il
 * componente con la curva più recente ancora in sospeso: la finestra l'aveva
 * disegnata, il nucleo aveva quella di prima, e riaprendo il cursore tornava
 * indietro di un passo.
 */

/** Un orologio finto: il tempo avanza solo quando lo si dice. */
function orologioFinto() {
  let adesso = 0;
  let prossimo = 1;
  const attese = new Map<number, { quando: number; fai: () => void }>();
  return {
    ora: () => adesso,
    dopo: (ms: number, fai: () => void) => {
      const id = prossimo++;
      attese.set(id, { quando: adesso + ms, fai });
      return id;
    },
    annulla: (id: number) => {
      attese.delete(id);
    },
    avanza(ms: number) {
      adesso += ms;
      for (const [id, attesa] of [...attese]) {
        if (attesa.quando <= adesso) {
          attese.delete(id);
          attesa.fai();
        }
      }
    },
    inAttesa: () => attese.size,
  };
}

test("la prima parte subito, le altre una per passo, e solo l'ultima", () => {
  const orologio = orologioFinto();
  const mandate: number[] = [];
  const freno = creaFreno<number>(60, (v) => mandate.push(v), orologio);
  orologio.avanza(1000);
  freno.metti(1);
  assert.deepEqual(mandate, [1]);
  orologio.avanza(10);
  freno.metti(2);
  orologio.avanza(10);
  freno.metti(3);
  assert.deepEqual(mandate, [1], "troppo presto: si tiene da parte");
  orologio.avanza(40);
  assert.deepEqual(mandate, [1, 3], "allo scadere parte l'ultima, non la seconda");
});

test("chiudere a metà attesa manda subito quel che era in sospeso", () => {
  const orologio = orologioFinto();
  const mandate: number[] = [];
  const freno = creaFreno<number>(60, (v) => mandate.push(v), orologio);
  orologio.avanza(1000);
  freno.metti(1);
  orologio.avanza(20);
  freno.metti(2);
  freno.svuota();
  assert.deepEqual(mandate, [1, 2]);
  assert.equal(orologio.inAttesa(), 0, "l'attesa si ferma");
  // E niente parte una seconda volta allo scadere.
  orologio.avanza(100);
  assert.deepEqual(mandate, [1, 2]);
  // Senza niente in sospeso, svuotare non manda niente.
  freno.svuota();
  assert.deepEqual(mandate, [1, 2]);
});

test("l'equalizzatore svuota il freno allo smontaggio", async () => {
  const { readFileSync } = await import("node:fs");
  const sorgente = readFileSync(new URL("../src/parti/Equalizzatore.tsx", import.meta.url), "utf8");
  assert.match(sorgente, /creaFreno\(PASSO_INVIO,/);
  assert.match(sorgente, /useEffect\(\(\) => \(\) => freno\.svuota\(\), \[freno\]\)/);
});
