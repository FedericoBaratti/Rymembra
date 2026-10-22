/**
 * Le cartelle sorvegliate come elenco, e la fila dei gesti che lo riscrivono.
 *
 * Il caso che ha fatto nascere `elencoCartelle.ts` è qui scritto per intero:
 * due cartelle tolte una dopo l'altra, la seconda partita mentre la prima è
 * ancora in volo. Il «nucleo» è una variabile, e le sue risposte arrivano
 * quando lo dice il test.
 */
import { test } from "node:test";
import assert from "node:assert/strict";

import type { CartellaGemella } from "../src/ipc";
import {
  conCartelle,
  creaFila,
  senzaCartella,
  traslocate,
} from "../src/elencoCartelle";

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

/** Lascia girare le promesse già pronte. */
const unGiro = () => new Promise<void>((r) => setTimeout(r, 0));

test("aggiungere non fa doppioni e tiene l'ordine", () => {
  assert.deepEqual(conCartelle(["A", "B"], ["B", "C", "C"]), ["A", "B", "C"]);
  assert.deepEqual(conCartelle([], ["A"]), ["A"]);
});

test("togliere toglie quella e basta", () => {
  assert.deepEqual(senzaCartella(["A", "B", "C"], "B"), ["A", "C"]);
  assert.deepEqual(senzaCartella(["A"], "Z"), ["A"]);
});

test("il trasloco rinomina la vecchia e aggiunge la scelta in fondo", () => {
  const quale: CartellaGemella = { nuova: "\\\\nas\\m", esistente: "Z:\\", a: "\\\\nas\\" };
  assert.deepEqual(traslocate(["C:\\x", "Z:\\"], quale), ["C:\\x", "\\\\nas\\", "\\\\nas\\m"]);
});

test("due cartelle tolte in fila restano tolte tutte e due", async () => {
  // Il nucleo: quel che c'è scritto, e la risposta che il test lascia andare.
  let scritte = ["A", "B", "C"];
  const risposte: ReturnType<typeof sospesa<void>>[] = [];
  const imposta = async (cartelle: string[]) => {
    const risposta = sospesa<void>();
    risposte.push(risposta);
    await risposta.promessa;
    scritte = cartelle;
  };

  const fila = creaFila();
  // Ogni gesto legge l'elenco **quando parte**, come `scriviCartelle`.
  const togli = (percorso: string) =>
    fila(() => imposta(senzaCartella(scritte, percorso)));

  const prima = togli("A");
  const seconda = togli("B");
  await unGiro();
  // La seconda aspetta: al nucleo è arrivata una domanda sola.
  assert.equal(risposte.length, 1);
  risposte[0]?.risolvi();
  await prima;
  await unGiro();
  assert.equal(risposte.length, 2);
  risposte[1]?.risolvi();
  await seconda;

  // Senza la fila la seconda avrebbe scritto ["A", "C"]: la prima, di nuovo.
  assert.deepEqual(scritte, ["C"]);
});

test("un gesto fallito non ferma quelli dietro, e il suo errore torna a lui", async () => {
  const fila = creaFila();
  const fatti: string[] = [];
  const guasto = new Error("disco staccato");
  const prima = fila(() => Promise.reject(guasto));
  const seconda = fila(async () => {
    fatti.push("seconda");
    return 2;
  });
  await assert.rejects(prima, (e) => e === guasto);
  assert.equal(await seconda, 2);
  assert.deepEqual(fatti, ["seconda"]);
});

test("la fila dice quanti gesti ha, in corso e in attesa", async () => {
  const conti: number[] = [];
  const fila = creaFila((n) => conti.push(n));
  const a = sospesa<void>();
  const uno = fila(() => a.promessa);
  const due = fila(async () => undefined);
  assert.deepEqual(conti, [1, 2]);
  a.risolvi();
  await uno;
  await due;
  // Torna a zero solo quando l'ultimo ha finito: è quel che riaccende i tasti.
  assert.equal(conti.at(-1), 0);
  assert.ok(conti.every((n) => n >= 0));
});
