import { test } from "node:test";
import assert from "node:assert/strict";

import { creaMittente, type Orologio } from "../src/studio/mittente";

/** Un orologio a mano: il tempo avanza quando lo dice il test. */
function orologioFinto() {
  let adesso = 0;
  let prossimo = 1;
  const attese = new Map<number, { quando: number; fai: () => void }>();
  const orologio: Orologio = {
    imposta: (fai, ms) => {
      attese.set(prossimo, { quando: adesso + ms, fai });
      return prossimo++;
    },
    togli: (quale) => void attese.delete(quale),
  };
  const avanza = (ms: number) => {
    adesso += ms;
    for (const [id, a] of [...attese]) {
      if (a.quando <= adesso) {
        attese.delete(id);
        a.fai();
      }
    }
  };
  return { orologio, avanza };
}

const giro = () => new Promise((fatto) => setImmediate(fatto));

test("le versioni di mezzo non si validano, e l'ultima vince", async () => {
  const { orologio, avanza } = orologioFinto();
  const chieste: string[] = [];
  const arrivate: string[] = [];
  const risposte: ((v: string) => void)[] = [];
  const m = creaMittente<string>({
    valida: (t) => {
      chieste.push(t);
      return new Promise((fatto) => risposte.push(fatto));
    },
    arrivato: (t) => arrivate.push(t),
    guasto: () => assert.fail("nessun guasto"),
    orologio,
  });

  m.manda("a");
  m.manda("ab");
  avanza(120);
  assert.deepEqual(chieste, ["ab"], "un respiro dopo l'ultima, una volta");

  m.manda("abc");
  m.manda("abcd");
  avanza(120);
  assert.deepEqual(chieste, ["ab"], "una sola in volo");

  risposte[0]!("ok");
  await giro();
  assert.deepEqual(arrivate, ["ab"]);
  assert.deepEqual(chieste, ["ab", "abcd"], "parte la più recente, non quella di mezzo");

  m.ferma();
  risposte[1]!("ok");
  await giro();
  assert.deepEqual(arrivate, ["ab"], "fermo, non consegna più");
});
