import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * Le Impostazioni non tacciono un guasto che spegne un comando (R12-5).
 *
 * - Se `testiStato()` falliva, la scheda dei testi restava senza stato: «Cerca
 *   i testi» spento per sempre, i numeri a zero, nessuna frase.
 * - «Accetta registrazioni diverse» mette il guasto della scrittura in
 *   `importazioni.errore`, che le Impostazioni non disegnavano: l'interruttore,
 *   che segue il nucleo, non si muoveva e non diceva perché. E prima che lo
 *   stato arrivasse si disegnava acceso (`?? true`), anche con la preferenza
 *   salvata spenta.
 */
const SORGENTE = readFileSync(
  new URL("../src/schermate/Impostazioni.tsx", import.meta.url),
  "utf8",
);

/** Il corpo della funzione di primo livello `nome`, fino alla successiva. */
function funzione(nome: string): string {
  const da = SORGENTE.indexOf(`\nfunction ${nome}(`);
  assert.ok(da >= 0, `non trovo ${nome}: la prova va aggiornata`);
  const dopo = SORGENTE.slice(da + 1).search(/\n(?:export )?function [A-Za-z]+\(/);
  return SORGENTE.slice(da, dopo < 0 ? undefined : da + 1 + dopo);
}

test("la scheda dei testi tiene il guasto della lettura e offre «Riprova»", () => {
  const scheda = funzione("SchedaTesti");
  const lettura = scheda.slice(scheda.indexOf("const aggiorna = useCallback("));
  const presa = lettura.slice(lettura.indexOf(".catch("), lettura.indexOf("}, []);"));
  assert.match(presa, /setGuasto\(/, "il guasto di testiStato() si butta");
  assert.match(scheda, /testoErrore\(guasto\)/);
  assert.match(scheda, /t\("common\.retry"\)/);
  assert.match(scheda, /onClick=\{aggiorna\}/);
});

test("la scheda delle importazioni dice il guasto dell'interruttore, e non lo dipinge acceso prima del tempo", () => {
  const dove = SORGENTE.indexOf('etichetta={t("settings.import.alt")}');
  assert.ok(dove > 0, "non trovo l'interruttore: la prova va aggiornata");
  const intorno = SORGENTE.slice(dove - 600, dove + 900);
  assert.match(intorno, /importazioni\.errore/);
  assert.doesNotMatch(intorno, /alternative \?\? true/);
});
