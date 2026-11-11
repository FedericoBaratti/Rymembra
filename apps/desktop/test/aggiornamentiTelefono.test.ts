/**
 * L'APK che Android non installerebbe: la fascia non lo scarica, e dice come
 * passare alla versione pubblica.
 *
 * Le copie di Rymembra per telefono uscite finora sono firmate con la chiave di
 * prova, e l'APK pubblico con quella di rilascio: Android non installa l'uno
 * sopra l'altro. «Aggiorna» scaricava decine di megabyte, l'installatore li
 * rifiutava con «Riavvia Rymembra», e ogni tocco successivo li riscaricava
 * (R03-2, R11-2). Il nucleo adesso lo sa prima — o lo ricorda dopo il primo
 * rifiuto — e lo dice con `altraChiave` (D-5); qui si controlla che la fascia
 * ne tenga conto e che il testo dica il rimedio vero.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");
const it = JSON.parse(leggi("../src/lingue/it.json")) as Record<string, string>;
const en = JSON.parse(leggi("../src/lingue/en.json")) as Record<string, string>;

test("con un APK firmato con un'altra chiave la fascia non offre «Aggiorna»", () => {
  const fascia = leggi("../src/parti/Aggiornamenti.tsx");
  const tasto = fascia.indexOf('{t("update.install")}');
  assert.ok(tasto >= 0, "il tasto «Aggiorna» non c'è più");
  // Il tasto sta dentro il ramo che lo esclude per l'altra chiave.
  const prima = fascia.slice(Math.max(0, tasto - 300), tasto);
  assert.match(prima, /\{!altraChiave && \(/, "«Aggiorna» resta anche per un APK che Android rifiuterebbe");
  // E al suo posto c'è la spiegazione.
  assert.match(fascia, /t\("update\.otherKey\.steps"/);
});

test("il testo dell'altra chiave dice come passare alla versione pubblica, non «Riavvia»", () => {
  for (const [lingua, testi, esporta, leggiProfilo] of [
    ["it", it, it["profile.export"], it["profile.read"]],
    ["en", en, en["profile.export"], en["profile.read"]],
  ] as const) {
    const passi = testi["update.otherKey.steps"];
    assert.ok(passi, `${lingua}: manca update.otherKey.steps`);
    assert.ok(testi["update.otherKey"], `${lingua}: manca update.otherKey`);
    // I nomi dei due tasti, così come sono scritti nell'interfaccia.
    assert.ok(esporta && passi.includes(esporta), `${lingua}: non nomina «${esporta}»`);
    assert.ok(leggiProfilo && passi.includes(leggiProfilo), `${lingua}: non nomina «${leggiProfilo}»`);
    assert.doesNotMatch(passi, /riavvia|restart/i, `${lingua}: promette che un riavvio basti`);
    assert.match(testi["update.otherKey"], /\{v\}/, `${lingua}: non dice quale versione`);
  }
});
