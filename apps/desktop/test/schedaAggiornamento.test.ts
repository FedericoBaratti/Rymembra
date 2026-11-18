/**
 * La scheda dell'aggiornamento sta nella pila delle notifiche, e racconta i
 * passi: qui si guarda che il cablaggio sia quello (i test della logica sono in
 * `avanzamentoAggiornamento.test.ts`; il resto si vede dal vivo).
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");
const app = leggi("../src/App.tsx");
const scheda = leggi("../src/parti/Aggiornamenti.tsx");
const it = JSON.parse(leggi("../src/lingue/it.json")) as Record<string, string>;
const en = JSON.parse(leggi("../src/lingue/en.json")) as Record<string, string>;

test("la scheda sta una volta sola, dentro la pila delle notifiche e non nel flusso della pagina", () => {
  const montaggi = app.match(/<AvvisoAggiornamento\b/g) ?? [];
  assert.equal(montaggi.length, 1, "la scheda è montata in più posti, o in nessuno");
  const apre = app.indexOf("<PilaToast");
  const chiude = app.indexOf("</PilaToast>");
  const scheda_ = app.indexOf("<AvvisoAggiornamento");
  assert.ok(apre >= 0 && chiude > apre, "la pila non si trova");
  assert.ok(scheda_ > apre && scheda_ < chiude, "la scheda non è dentro la pila");
});

test("la scheda è una toast-card con la sua presenza, come la scansione", () => {
  assert.match(scheda, /<Presenza parte="toast-card">/);
  assert.match(scheda, /className="notizia aggiornamento toast-card"/);
});

test("«Alla chiusura» non c'è sul telefono, e ogni installazione dice quando", () => {
  const tasto = scheda.indexOf('{t("update.atClose")}');
  assert.ok(tasto >= 0, "il tasto «Alla chiusura» non c'è");
  const prima = scheda.slice(Math.max(0, tasto - 400), tasto);
  assert.match(prima, /!MOBILE/, "«Alla chiusura» compare anche sul telefono, dove non esiste");
  assert.match(prima, /installa\("allaChiusura"\)/);
  for (const chiamata of scheda.matchAll(/installa\("([^"]+)"\)/g)) {
    assert.ok(["subito", "allaChiusura"].includes(chiamata[1] ?? ""), `quando sconosciuto: ${chiamata[1]}`);
  }
});

test("la fase si legge dallo stato, non solo dagli eventi: la finestra può rinascere nel mezzo", () => {
  assert.match(scheda, /stato\?\.fase/);
  assert.match(scheda, /stato\?\.prontoDalMs/);
});

test("il sollecito dei tre giorni non si zittisce con «Non ora»", () => {
  assert.match(scheda, /!prontaDaTempo/);
  assert.match(scheda, /attesaLunga\(prontoDalMs, adesso\)/);
});

test("i testi dei passi e del pacchetto pronto ci sono in italiano e in inglese", () => {
  for (const chiave of [
    "update.atClose",
    "update.step.download",
    "update.step.verify",
    "update.step.install",
    "update.step.restart",
    "update.step.confirm",
    "update.status.ready.close",
    "update.status.ready.late",
    "update.status.ready.phone",
    "update.ready.now",
    "update.ready.install",
    "settings.update.play",
  ]) {
    assert.ok(it[chiave], `manca in italiano: ${chiave}`);
    assert.ok(en[chiave], `manca in inglese: ${chiave}`);
  }
  // Il sollecito dice «più di tre giorni» a parole: un numero qui vorrebbe un plurale.
  assert.doesNotMatch(it["update.status.ready.late"] ?? "", /\{/);
  assert.doesNotMatch(en["update.status.ready.late"] ?? "", /\{/);
});

test("alla chiusura il testo dice che non si riapre da sola, e dov'è «Esci» con il vassoio", () => {
  assert.match(it["update.status.ready.close"] ?? "", /non si riapre/);
  assert.match(en["update.status.ready.close"] ?? "", /doesn’t reopen/);
  assert.match(it["update.ready.tray"] ?? "", /Esci/);
  assert.match(en["update.ready.tray"] ?? "", /Exit/);
});
