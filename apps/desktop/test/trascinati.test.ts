import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { ESTENSIONE_SKIN_PRECEDENTE } from "../src/eredita";
import type { Trascinati } from "../src/ipc";
import { pianoDeiTrascinati, type SituazioneTrascinati } from "../src/trascinati";

/** Niente di niente: si aggiunge quel che la prova lascia cadere. */
function lasciati(parte: Partial<Trascinati>): Trascinati {
  return {
    cartelle: [],
    skin: [],
    playlist: [],
    brani: [],
    fuoriLibreria: 0,
    ignorati: 0,
    ...parte,
  };
}

const normale: SituazioneTrascinati = { primoInAttesa: false, avviato: true, finestrellaSopra: false };

test("una skin vince su tutto il resto", () => {
  const piano = pianoDeiTrascinati(
    lasciati({ skin: ["a.ryskin"], playlist: ["b.m3u8"], brani: [1], cartelle: ["C:/M"] }),
    normale,
  );
  assert.deepEqual(piano, { tipo: "skin", percorso: "a.ryskin" });
});

test("una playlist apre la sua finestrella, e non si fa altro", () => {
  const piano = pianoDeiTrascinati(
    lasciati({ playlist: ["b.m3u8", "c.pls"], brani: [1] }),
    normale,
  );
  assert.deepEqual(piano, { tipo: "playlist", percorso: "b.m3u8" });
});

test("brani e cartelle insieme: tutti e due, e nessuna notizia di troppo", () => {
  const piano = pianoDeiTrascinati(
    lasciati({ brani: [3, 1], cartelle: ["C:/M"], fuoriLibreria: 2 }),
    normale,
  );
  assert.deepEqual(piano, {
    tipo: "resto",
    brani: [3, 1],
    cartelle: { dove: "sorvegliate", percorsi: ["C:/M"] },
    notizia: null,
  });
});

test("col primo avvio che chiede, le cartelle vanno a lui", () => {
  const piano = pianoDeiTrascinati(lasciati({ cartelle: ["C:/M"] }), {
    primoInAttesa: true,
    finestrellaSopra: false,
    avviato: true,
  });
  assert.equal(piano.tipo, "resto");
  if (piano.tipo === "resto") assert.equal(piano.cartelle?.dove, "primo");
});

test("prima di `avvio` le cartelle non si sistemano, e si dice quel che resta", () => {
  const piano = pianoDeiTrascinati(lasciati({ cartelle: ["C:/M"], ignorati: 1 }), {
    primoInAttesa: false,
    finestrellaSopra: false,
    avviato: false,
  });
  assert.deepEqual(piano, { tipo: "resto", brani: [], cartelle: null, notizia: "niente" });
});

test("file che la libreria non conosce: si dice, e prima dei file inutili", () => {
  const fuori = pianoDeiTrascinati(lasciati({ fuoriLibreria: 1, ignorati: 3 }), normale);
  assert.equal(fuori.tipo === "resto" && fuori.notizia, "fuoriLibreria");
  const niente = pianoDeiTrascinati(lasciati({}), normale);
  assert.equal(niente.tipo === "resto" && niente.notizia, null);
});

test("una skin di prima si installa come una di adesso", () => {
  // A dire cos'è una skin è il nucleo, che riconosce anche l'estensione di
  // prima (`trascinati.rs`): qui basta che quel che arriva in `skin` vinca
  // su tutto il resto, qualunque sia l'estensione del file.
  const vecchia = `C:/Skin/notte.${ESTENSIONE_SKIN_PRECEDENTE}`;
  const piano = pianoDeiTrascinati(
    lasciati({ skin: [vecchia, "b.ryskin"], playlist: ["b.m3u8"], brani: [1], cartelle: ["C:/M"] }),
    normale,
  );
  assert.deepEqual(piano, { tipo: "skin", percorso: vecchia });
});

test("con una finestrella aperta non si fa niente dietro il velo, e lo si dice", () => {
  // R13d-8. Il trascinamento è un evento nativo, e `inert` non lo ferma: con
  // «Elimina dal disco» aperto, una cartella lasciata cadere portava dietro il
  // velo alle Impostazioni e partiva la scansione; dei brani cambiavano la coda.
  const sopra = { ...normale, finestrellaSopra: true };
  for (const parte of [
    { cartelle: ["C:/M"] },
    { brani: [1, 2] },
    { skin: ["a.ryskin"] },
    { playlist: ["b.m3u8"] },
  ]) {
    assert.deepEqual(pianoDeiTrascinati(lasciati(parte), sopra), { tipo: "fermo" }, JSON.stringify(parte));
  }
  // Il primo avvio non conta come finestrella sopra: le cartelle sono per lui.
  const alPrimo = pianoDeiTrascinati(lasciati({ cartelle: ["C:/M"] }), {
    primoInAttesa: true,
    avviato: true,
    finestrellaSopra: false,
  });
  assert.equal(alPrimo.tipo, "resto");
  // E `App` lo dice, invece di eseguire.
  const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
  assert.match(app, /finestrellaSopra: modaliAperti\(\) > \(primoVisibile \? 1 : 0\)/);
  assert.match(app, /if \(piano\.tipo === "fermo"\) \{\s+setNotizia\(t\("drop\.closeFirst"\)\);/);
});
