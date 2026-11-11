import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * L'editor delle scorciatoie si fa sentire (R16-7).
 *
 * - Il tasto che toglie una scorciatoia si chiamava solo «Spazio»: l'azione,
 *   «Togli Spazio», e il conflitto stavano nel `title`, che molti lettori non
 *   leggono. Invio lo toglieva senza che il nome l'avesse detto.
 * - «Assegna un tasto a …», premuto, aspettava un tasto senza dirlo: stesso
 *   nome, nessuno stato.
 * - Il rifiuto («Tab sposta il fuoco…») e i conflitti comparivano in un
 *   paragrafo senza ruolo, cioè muti.
 */
const SORGENTE = readFileSync(new URL("../src/parti/Scorciatoie.tsx", import.meta.url), "utf8");

test("il tasto che toglie dice che toglie, da quale comando, e il conflitto", () => {
  const chip = SORGENTE.slice(SORGENTE.indexOf('className="tasto-scorciatoia"'));
  const fine = chip.indexOf("data-conteso");
  const dove = chip.indexOf("aria-label={");
  assert.ok(dove >= 0 && dove < fine, "il tasto non ha un nome suo");
  const etichetta = chip.slice(chip.indexOf("aria-label={"), chip.indexOf("data-conteso"));
  assert.match(etichetta, /shortcuts\.removeKeyFrom/);
  assert.match(etichetta, /shortcuts\.removeKeyContested/);
  assert.match(etichetta, /comando: titolo/);
});

test("«Assegna» dice quando sta ascoltando", () => {
  assert.match(SORGENTE, /aria-pressed=\{inAscolto === chiave\}/);
});

test("rifiuto e conflitti stanno in una regione viva montata sempre", () => {
  const dove = SORGENTE.indexOf('role="status"');
  assert.ok(dove > 0, "nessuna regione viva");
  // La regione non sta dentro una condizione: nasce vuota, e si riempie.
  const riga = SORGENTE.slice(SORGENTE.lastIndexOf("\n", dove), dove);
  assert.doesNotMatch(riga, /&&/);
  const dopo = SORGENTE.slice(dove);
  const regione = dopo.slice(0, dopo.indexOf("</div>"));
  assert.match(regione, /rifiutato/);
  assert.match(regione, /shortcuts\.clash/);
});
