import { test } from "node:test";
import assert from "node:assert/strict";

import type { Motivo } from "../src/ipc";
import { testoMotivo } from "../src/studio/motivo";

/*
 * I problemi del validatore nella lingua dell'interfaccia (D-46): la chiave
 * del catalogo e i valori dei segnaposto arrivano dal nucleo, e qui si compone
 * la frase. Il catalogo è finto: `node --test` non importa i JSON delle lingue.
 */
const catalogo: Record<string, string> = {
  "skin.issue.colore": "“{scritto}” isn’t a color",
  "skin.issue.varianteVuota": "The skin declares a {tema} theme and writes nothing in it.",
  "skin.issue.tema.chiaro": "light",
  "skin.issue.parteInesistente": "No such part: “{nome}”.",
  "skin.issue.forse": "Did you mean {nomi}?",
  "skin.issue.forse.nome": "“{nome}”",
};
const traduci = (chiave: string, valori: Record<string, string>) => {
  const frase = catalogo[chiave];
  return frase === undefined ? null : frase.replace(/\{(\w+)\}/g, (tutto, n: string) => valori[n] ?? tutto);
};
const m = (parti: Partial<Motivo>): Motivo => ({ chiave: "", testo: "", valori: {}, forse: [], ...parti });

test("un motivo con la sua chiave si legge nella lingua del catalogo, coi valori al loro posto", () => {
  const motivo = m({ chiave: "skin.issue.colore", testo: "«#zz» non è un colore", valori: { scritto: "#zz" } });
  assert.equal(testoMotivo(motivo, traduci), "“#zz” isn’t a color");
});

test("i pezzi di frase si traducono prima di entrare", () => {
  const tema = m({ chiave: "skin.issue.tema.chiaro", testo: "chiaro" });
  const motivo = m({ chiave: "skin.issue.varianteVuota", testo: "La skin dichiara un tema chiaro…", valori: { tema } });
  assert.equal(testoMotivo(motivo, traduci), "The skin declares a light theme and writes nothing in it.");
});

test("«Forse intendevi» lo aggiunge la finestra, con le virgolette della lingua", () => {
  const motivo = m({
    chiave: "skin.issue.parteInesistente",
    testo: "parte inesistente: «crad». Forse intendevi «card»?",
    valori: { nome: "crad" },
    forse: ["card", "cards"],
  });
  assert.equal(testoMotivo(motivo, traduci), "No such part: “crad”. Did you mean “card”, “cards”?");
});

test("senza chiave, o con una chiave che il catalogo non ha, resta la frase del nucleo", () => {
  assert.equal(testoMotivo(m({ testo: "frase del nucleo" }), traduci), "frase del nucleo");
  const ignota = m({ chiave: "skin.issue.nata.dopo", testo: "nata dopo. Forse intendevi «x»?", forse: ["x"] });
  assert.equal(testoMotivo(ignota, traduci), "nata dopo. Forse intendevi «x»?", "il ripiego ha già il suo «forse»");
});
