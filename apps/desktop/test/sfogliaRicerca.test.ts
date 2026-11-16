/**
 * La ricerca in «Sfoglia» (R13-7).
 *
 * - Il conteggio che non arriva (un dispositivo raggiunto via relay, una risposta
 *   che scade) lasciava sopra i risultati un «Caricamento…» per sempre, annunciato
 *   dal lettore di schermo: il `.catch(() => undefined)` non scriveva niente e
 *   `quanti` restava `null`.
 * - I risultati si smontavano aprendo un disco col menù «Vai all'album» (`!livello`),
 *   contro il commento della pagina («Quel che è coperto resta montato,
 *   nascosto»): tornando indietro la ricerca ripartiva da zero, con una domanda in
 *   più all'altro dispositivo e le pagine caricate perse.
 *
 * I componenti non si montano con `node --test`: si legge il sorgente.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const pagina = readFileSync(new URL("../src/schermate/Dispositivo.tsx", import.meta.url), "utf8");

/** Il corpo di `Risultati`, fino al componente che segue. */
function risultati(): string {
  const da = pagina.indexOf("function Risultati(");
  const a = pagina.indexOf("\n/** Una scheda o un livello", da);
  assert.ok(da >= 0 && a > da, "non trovo Risultati");
  return pagina.slice(da, a);
}

test("un conteggio che non arriva non lascia «Caricamento…» per sempre (R13-7)", () => {
  const corpo = risultati();
  assert.doesNotMatch(
    corpo,
    /lontanoCercaConteggio[\s\S]{0,300}\.catch\(\(\) => undefined\)/,
    "il guasto del conteggio non si ingoia lasciando `quanti` com'era",
  );
  assert.match(corpo, /\.catch\(\(\) => \{[\s\S]*?setQuanti\("mancante"\)/, "il guasto è uno stato, e non è l'attesa");
  // Il testo: «Caricamento…» solo nell'attesa, niente per il guasto.
  assert.match(corpo, /quanti === "attesa"\s*\?\s*t\("common\.loading"\)/);
  assert.match(corpo, /quanti === "mancante"\s*\?\s*""/);
});

test("la regione di stato del conteggio resta montata: cambia il testo, non il nodo (R13-7)", () => {
  const corpo = risultati();
  const stato = corpo.indexOf('role="status"');
  assert.ok(stato >= 0, "manca la regione di stato");
  const prima = corpo.slice(0, stato);
  assert.doesNotMatch(
    prima.slice(prima.lastIndexOf("return (")),
    /quanti[^<]*&&|\?\s*\(\s*<p/,
    "la regione non è dietro una condizione: nasce con la pagina e cambia solo il testo",
  );
});

test("i risultati restano montati sotto un disco aperto, nascosti, come le schede (R13-7)", () => {
  assert.doesNotMatch(
    pagina,
    /\{cercato !== "" && !livello && \(/,
    "con un livello aperto i risultati non si smontano",
  );
  assert.match(
    pagina,
    /\{cercato !== "" && \(\s*<Piano nascosto=\{livello\}>\s*<Risultati testo=\{cercato\} \/>/,
    "i risultati stanno in un Piano nascosto dal livello",
  );
});
