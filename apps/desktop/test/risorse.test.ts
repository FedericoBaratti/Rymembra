import { test } from "node:test";
import assert from "node:assert/strict";

import { iniziale, riduci, testo } from "../src/studio/negozio";
import { scrivi as stampa } from "../src/studio/patch";
import { elencoBreve, problemaNomeFamiglia, senzaCaratteriDi, usiDellaRisorsa } from "../src/studio/risorse";
import { cosaAnnulla, cosaRipete } from "../src/studio/storia";

const CON_CARATTERE = JSON.stringify({
  id: "prova",
  fonts: { Fraunces: { src: "fraunces.woff2" }, Inter: { src: "inter.woff2" } },
  parts: { card: { background: [{ effect: "image", src: "fraunces.woff2" }] } },
});

test("gli usi di un file: le dichiarazioni dei caratteri e i livelli image", () => {
  const documento = JSON.parse(CON_CARATTERE) as Record<string, unknown>;
  assert.deepEqual(usiDellaRisorsa(documento, "fraunces.woff2"), ["fonts.Fraunces", "parts.card.background.0"]);
  assert.deepEqual(usiDellaRisorsa(documento, "altro.png"), []);
  assert.equal(elencoBreve(["a", "b", "c"], 2), "a, b, … (+1)");
  assert.equal(elencoBreve(["a", "b"]), "a, b");
});

test("togliere un file non è un passo dell'annullo, e Ctrl+Z non rimette la dichiarazione", () => {
  // Il file sul disco non torna: un «Annulla: via fraunces.woff2» rimetteva la
  // dichiarazione e non il file, e annullare il passo di prima la rimetteva
  // sotto un'altra etichetta.
  let s = riduci(iniziale("prova"), { tipo: "apri", testo: CON_CARATTERE });
  const conRaggio = JSON.stringify({ ...(JSON.parse(CON_CARATTERE) as object), meta: { radius: 1 } });
  s = riduci(s, { tipo: "scrivi", testo: conRaggio, etichetta: "raggio", gruppo: "meta", adesso: 1 });
  s = riduci(s, { tipo: "risorsaTolta", nome: "fraunces.woff2" });
  assert.ok(!testo(s).includes("Fraunces"));
  assert.ok(testo(s).includes("Inter"));
  assert.equal(cosaAnnulla(s.storia), "raggio", "la cancellazione non deve comparire nell'annullo");
  s = riduci(s, { tipo: "annulla" });
  assert.ok(!testo(s).includes("Fraunces"), "annullare il passo di prima rimetteva la dichiarazione");
  assert.equal(cosaRipete(s.storia), "raggio");
  s = riduci(s, { tipo: "ripeti" });
  assert.ok(!testo(s).includes("Fraunces"));
  assert.ok(testo(s).includes("radius"));
});

test("un passo che diventa uguale al precedente si toglie dalla storia", () => {
  // Il passo che aveva aggiunto il carattere, senza il carattere, non cambia
  // più niente: un annullo che non annulla niente non deve restare. I testi
  // sono scritti come li scrive lo Studio (`patch.ts`, due spazi).
  const senza = stampa({ id: "prova" });
  const con = stampa({ id: "prova", fonts: { Fraunces: { src: "fraunces.woff2" } } });
  let s = riduci(iniziale("prova"), { tipo: "apri", testo: senza });
  s = riduci(s, { tipo: "scrivi", testo: con, etichetta: "carattere Fraunces", gruppo: "fonts", adesso: 1 });
  s = riduci(s, { tipo: "risorsaTolta", nome: "fraunces.woff2" });
  assert.equal(cosaAnnulla(s.storia), null);
  assert.ok(!testo(s).includes("Fraunces"));
});

test("un testo che non è JSON resta com'è", () => {
  assert.equal(senzaCaratteriDi("{ rotto", "fraunces.woff2"), "{ rotto");
});

test("rinominare una famiglia: un nome non ammesso o già preso si dice", () => {
  // Il campo restava col testo scritto, senza un segno, e sembrava rinominato.
  const presi = ["Inter", "Fraunces"];
  assert.equal(problemaNomeFamiglia("Inter (Bold)", "Inter", presi), "alfabeto");
  assert.equal(problemaNomeFamiglia("  ", "Inter", presi), "alfabeto");
  assert.equal(problemaNomeFamiglia("Fraunces", "Inter", presi), "preso");
  assert.equal(problemaNomeFamiglia("Inter", "Inter", presi), null, "il nome di adesso non è un problema");
  assert.equal(problemaNomeFamiglia("Inter Display", "Inter", presi), null);
});
