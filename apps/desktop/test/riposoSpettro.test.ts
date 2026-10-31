import { test } from "node:test";
import assert from "node:assert/strict";

import * as riposo from "../src/parti/riposoSpettro";
import type { Passo, Riposo } from "../src/parti/riposoSpettro";

/*
 * La macchina del riposo della scena dello spettro, senza WebGL e senza React.
 *
 * `Spettro3D` esegue i passi che lei restituisce: il rubinetto della presa nel
 * motore, il ciclo da fermare, il risveglio. Qui si fanno passare le storie che
 * la lasciavano incastrata — un contesto perso a finestra nascosta — e si
 * guarda come finisce: lo stato, e l'ultima cosa fatta al rubinetto.
 */

/** Una storia: gli eventi in fila, con `risveglia` che fa quel che fa la scena. */
function storia(eventi: ((r: Riposo) => Passo)[]) {
  let r: Riposo = { stato: "vivo", nascosta: false };
  let rubinetto: boolean | null = null;
  let guasto = false;
  for (const evento of eventi) {
    const passo = evento(r);
    r = { stato: passo.stato, nascosta: passo.nascosta };
    if (passo.rubinetto !== null) rubinetto = passo.rubinetto;
    if (passo.guasto) guasto = true;
    // `risveglia` della scena: esce su perso e nascosto, altrimenti vivo.
    if (passo.risveglia && r.stato !== "perso" && r.stato !== "nascosto") {
      r = { ...r, stato: "vivo" };
    }
  }
  return { r, rubinetto, guasto };
}

test("contesto perso a finestra nascosta, tornato prima della finestra: la presa si riapre", () => {
  const { nascondi, perso, ritrovato, mostra } = riposo;
  const fine = storia([nascondi, perso, ritrovato, mostra]);
  assert.equal(fine.r.stato, "vivo");
  assert.equal(fine.rubinetto, true);
});

test("contesto perso a finestra nascosta, finestra tornata prima del contesto: la presa si riapre", () => {
  const { nascondi, perso, ritrovato, mostra } = riposo;
  const fine = storia([nascondi, perso, mostra, ritrovato]);
  assert.equal(fine.r.stato, "vivo");
  assert.equal(fine.rubinetto, true);
});

test("tornato a finestra ancora giù, il contesto non sveglia la scena", () => {
  const { nascondi, perso, ritrovato } = riposo;
  const fine = storia([nascondi, perso, ritrovato]);
  assert.equal(fine.r.stato, "nascosto");
  assert.equal(fine.rubinetto, false);
});

test("perso e poi nascosto: la presa si chiude anche da perso", () => {
  const { perso, nascondi, ritrovato } = riposo;
  const fine = storia([perso, nascondi]);
  assert.equal(fine.r.stato, "perso");
  assert.equal(fine.rubinetto, false);
  // E al ritorno del contesto, con la finestra ancora giù, resta tutto fermo.
  const dopo = storia([perso, nascondi, ritrovato]);
  assert.equal(dopo.r.stato, "nascosto");
  assert.equal(dopo.rubinetto, false);
});

test("perso senza ritorno chiude la presa e segnala il guasto", () => {
  const { perso, scaduto } = riposo;
  const fine = storia([perso, scaduto]);
  assert.equal(fine.rubinetto, false);
  assert.equal(fine.guasto, true);
});

test("mostrare una scena assopita non riapre la presa: la chiude e la apre chi l'ha chiusa", () => {
  const { mostra } = riposo;
  const passo = mostra({ stato: "assopito", nascosta: false });
  assert.equal(passo.rubinetto, null);
  assert.equal(passo.risveglia, false);
});

test("addormentandosi la scena disegna l'ultimo fotogramma e non riarma", () => {
  const { passoCiclo } = riposo;
  assert.deepEqual(passoCiclo({ silenzio: 1000, copertura: 1016, misuraValida: true }), {
    disegna: true,
    riarma: true,
    addormenta: false,
  });
  assert.deepEqual(passoCiclo({ silenzio: 1017, copertura: 1016, misuraValida: true }), {
    disegna: true,
    riarma: false,
    addormenta: true,
  });
  // Senza misura non si disegna niente, e la sveglia la dà il ridimensionamento.
  assert.deepEqual(passoCiclo({ silenzio: 0, copertura: 1016, misuraValida: false }), {
    disegna: false,
    riarma: false,
    addormenta: false,
  });
});

test("l'attesa del ritorno corre solo a finestra visibile", () => {
  const { perso, nascondi, mostra, ritrovato } = riposo;
  // Perso a finestra visibile: si aspetta il ritorno.
  assert.equal(perso({ stato: "vivo", nascosta: false }).attesa, "arma");
  // Perso a finestra nascosta: la presa è già chiusa, e un driver che si
  // aggiorna mentre la finestra è giù può metterci quanto vuole.
  assert.equal(perso({ stato: "nascosto", nascosta: true }).attesa, null);
  // Nascosta mentre si aspetta: l'attesa si toglie; mostrata: riparte.
  assert.equal(nascondi({ stato: "perso", nascosta: false }).attesa, "togli");
  assert.equal(mostra({ stato: "perso", nascosta: true }).attesa, "arma");
  // Tornato: non si aspetta più niente.
  assert.equal(ritrovato({ stato: "perso", nascosta: false }).attesa, "togli");
  assert.equal(ritrovato({ stato: "perso", nascosta: true }).attesa, "togli");
});
