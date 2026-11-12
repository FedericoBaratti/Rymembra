/**
 * Il giro guidato non c'è sul telefono, per questo rilascio (D-37).
 *
 * Il `Giro` si monta solo fuori dal telefono, ma la regola che lo apre non lo
 * sapeva: sul telefono partiva senza vedersi, riempiva la coda con cinquanta
 * brani a ogni avvio con la coda vuota (`coda_prepara`), e non si chiudeva mai,
 * perché l'unico `onChiudi` è quello del `Giro` (R12-2). E «Rifai il giro»
 * nelle Impostazioni non mostrava niente. Qui il cablaggio dei tre punti; la
 * regola stessa sta in `guida.test.ts`.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");

test("App dice alla regola del giro se è il telefono", () => {
  const app = leggi("../src/App.tsx");
  const chiamata = app.slice(app.indexOf("comeParteIlGiro({"), app.indexOf("comeParteIlGiro({") + 500);
  assert.match(chiamata, /telefono: MOBILE/);
});

test("sul telefono le Impostazioni non offrono il giro", () => {
  const impostazioni = leggi("../src/schermate/Impostazioni.tsx");
  // La voce dell'indice: si trova cercando «giro» solo dove il giro c'è.
  assert.match(
    impostazioni,
    /\.\.\.\(MOBILE \? \[\] : \[v\("aspetto", "tour", "settings\.tour\.title"\)\]\)/,
    "la voce «Giro guidato» dell'indice compare anche sul telefono",
  );
  // La scheda con «Rifai il giro».
  const scheda = impostazioni.indexOf('titolo={t("settings.tour.title")}');
  assert.ok(scheda > 0, "la scheda del giro non c'è più");
  const prima = impostazioni.slice(Math.max(0, scheda - 900), scheda);
  assert.match(prima, /\{!MOBILE && \(/, "la scheda «Giro guidato» compare anche sul telefono");
});
