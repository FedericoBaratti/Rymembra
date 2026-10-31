import { test } from "node:test";
import assert from "node:assert/strict";

import type { ProfiloIa, StatoIa } from "../src/ipc";
import * as modelli from "../src/parti/profiliIa";

/*
 * «Modelli di linguaggio» nelle Impostazioni: quale profilo si prova da solo
 * dopo il salvataggio, e che cosa è in volo mentre succede.
 */

const profilo = (id: string, nome: string): ProfiloIa =>
  ({ id, nome, fornitore: "openrouter", urlBase: "", modello: "m", conChiave: true }) as ProfiloIa;

const stato = (profili: ProfiloIa[], attivo: string | null): StatoIa => ({
  profili,
  attivo,
  occupato: false,
  chiaveDimenticata: null,
});

test("dopo il salvataggio si prova il profilo appena salvato, non un omonimo", () => {
  assert.equal(typeof modelli.profiloSalvato, "function");
  // Due «OpenRouter»: il nucleo dà id diversi, e rende attivo quello salvato.
  const s = stato([profilo("openrouter", "OpenRouter"), profilo("openrouter-2", "OpenRouter")], "openrouter-2");
  assert.equal(modelli.profiloSalvato(s)?.id, "openrouter-2");
});

test("un nome vuoto o con gli spazi non impedisce la prova", () => {
  // Nome vuoto: il nucleo usa il modello come nome. Cercando per il nome
  // scritto nel modulo («» o «Mio ») non si trovava niente.
  const s = stato([profilo("openai-gpt-4o", "openai/gpt-4o")], "openai-gpt-4o");
  assert.equal(modelli.profiloSalvato(s)?.id, "openai-gpt-4o");
  assert.equal(modelli.profiloSalvato(stato([], null)), undefined);
});

test("la prova automatica resta in volo dopo che il salvataggio è finito", () => {
  assert.equal(typeof modelli.inVoloDopo, "function");
  let v = modelli.NIENTE_IN_VOLO;
  v = modelli.inVoloDopo(v, { gesto: "salva", fase: "inizio" });
  v = modelli.inVoloDopo(v, { gesto: "prova", fase: "inizio" });
  v = modelli.inVoloDopo(v, { gesto: "salva", fase: "fine" });
  assert.equal(v.prova, true);
  assert.equal(v.salva, false);
  assert.equal(modelli.qualcosaInVolo(v), true);
  v = modelli.inVoloDopo(v, { gesto: "prova", fase: "fine" });
  assert.equal(modelli.qualcosaInVolo(v), false);
});
