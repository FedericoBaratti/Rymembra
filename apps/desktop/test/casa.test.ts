import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { lunediLocale, prossimoLunedi } from "../src/lunedi";

/*
 * La Home e le raccolte del lunedì (R07-5, D-63, R12-5 parte Home).
 *
 * - «Il lunedì» non si rinnovava restando sulla Home: il lunedì si
 *   ricalcolava solo entrando nella vista, senza un timer né un ascolto della
 *   finestra che torna visibile.
 * - Una settimana vuota il nucleo la rifà quando arriva la cronologia o la
 *   scala sonora, ma la Home la segnava «chiesta» anche vuota, e non la
 *   richiedeva più fino al lunedì dopo.
 * - Se `ipc.casa()` falliva al primo ingresso la Home restava a scheletro per
 *   sempre (`CasaFinta`, «Caricamento…»), senza «Riprova».
 */
const CASA = readFileSync(new URL("../src/casa.ts", import.meta.url), "utf8");
const HOME = readFileSync(new URL("../src/schermate/Home.tsx", import.meta.url), "utf8");
const APP = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");

/** Il blocco dell'effetto che chiede le raccolte del lunedì. */
function effettoDellaSettimana(): string {
  const da = CASA.indexOf("const settimanaChiesta = useRef");
  assert.ok(da >= 0, "non trovo l'effetto delle raccolte: la prova va aggiornata");
  const fine = CASA.indexOf("E la Home, restando.", da);
  return CASA.slice(da, fine < 0 ? undefined : fine);
}

test("il lunedì locale e il prossimo, anche a cavallo della mezzanotte", () => {
  const domenicaSera = new Date(2026, 10, 15, 23, 59, 59);
  assert.equal(lunediLocale(domenicaSera), new Date(2026, 10, 9).getTime());
  assert.equal(prossimoLunedi(domenicaSera), new Date(2026, 10, 16).getTime());

  const lunediMezzanotte = new Date(2026, 10, 16, 0, 0, 0);
  assert.equal(lunediLocale(lunediMezzanotte), lunediMezzanotte.getTime());
  assert.equal(prossimoLunedi(lunediMezzanotte), new Date(2026, 10, 23).getTime());

  // La settimana del cambio d'ora (in Europa l'ultima domenica di ottobre):
  // il lunedì dopo resta la mezzanotte locale, non sette giorni di millisecondi.
  const mercoledi = new Date(2026, 9, 21, 12, 0, 0);
  assert.equal(prossimoLunedi(mercoledi), new Date(2026, 9, 26).getTime());
});

test("restando sulla Home le raccolte si richiedono a mezzanotte del lunedì e quando la finestra torna", () => {
  const effetto = effettoDellaSettimana();
  assert.match(effetto, /lunediLocale\(adesso\)/);
  assert.match(effetto, /setTimeout\([\s\S]*prossimoLunedi\(adesso\)/, "manca il timer al lunedì");
  assert.match(effetto, /addEventListener\("visibilitychange"/, "manca la finestra che torna");
  assert.match(effetto, /clearTimeout\(/);
  assert.match(effetto, /removeEventListener\("visibilitychange"/);
  assert.match(effetto, /\[vista, cercando, segnalaErrore, sveglia\]/, "l'effetto non rigira al colpo");
});

test("una settimana vuota non si segna chiesta, e una scansione la fa richiedere", () => {
  const effetto = effettoDellaSettimana();
  assert.match(
    effetto,
    /if \(arrivate\.length > 0\) settimanaChiesta\.current = lunedi;/,
    "una settimana vuota resta segnata fino al lunedì dopo",
  );
  const rileggi = CASA.slice(CASA.indexOf("const rileggiCasa = useCallback("));
  assert.match(rileggi.slice(0, rileggi.indexOf("}, [")), /setSveglia\(/);
});

test("la Home senza ripiani dice il guasto e offre «Riprova»", () => {
  assert.match(CASA, /guasto: unknown;/);
  assert.match(CASA, /return \{ casa, guasto, settimana, setSettimana, rileggiCasa \};/);
  // Ogni lettura dei ripiani che fallisce lo tiene, ogni lettura buona lo toglie.
  assert.equal((CASA.match(/setGuasto\(e\)/g) ?? []).length, 3);
  assert.equal((CASA.match(/setGuasto\(null\)/g) ?? []).length, 3);

  const corpo = HOME.slice(HOME.indexOf("export function Home("));
  const guasto = corpo.indexOf("casa === null && guasto !== null");
  const scheletro = corpo.indexOf("if (casa === null) return <CasaFinta />;");
  assert.ok(guasto >= 0 && guasto < scheletro, "lo scheletro viene prima del guasto");
  const stato = corpo.slice(guasto, scheletro);
  assert.match(stato, /<StatoVuoto/);
  assert.match(stato, /t\("home\.error\.title"\)/);
  assert.match(stato, /onRiprova && \{ etichetta: t\("common\.retry"\), fai: onRiprova \}/);

  assert.match(APP, /guasto: guastoCasa/);
  assert.match(APP, /guasto=\{guastoCasa\}\s*onRiprova=\{rileggiCasa\}/);
});
