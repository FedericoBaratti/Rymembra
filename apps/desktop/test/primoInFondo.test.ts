import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { mettiInPila } from "../src/modale.ts";

/*
 * Il primo avvio sta in fondo alla pila delle finestrelle e dei veli (D-97,
 * riprova in Windows Sandbox del 16/11/2026).
 *
 * La proposta della copia del Microsoft Store arrivava prima che il primo avvio
 * fosse montato; il primo avvio finiva in cima alle due pile, e la finestrella,
 * visibile sopra di lui per lo strato del CSS, diventava inerte: i clic cadevano
 * sulla scheda di sotto, Escape chiudeva la scheda, il lettore di schermo non la
 * vedeva. Qui la regola pura e il cablaggio; la prova vera è dal vivo.
 */

const leggi = (percorso: string) =>
  readFileSync(new URL(percorso, import.meta.url), "utf8");

test("in fondo vuol dire sotto anche chi era già aperto prima", () => {
  const pila = ["proposta"];
  mettiInPila(pila, "primo", true);
  assert.deepEqual(pila, ["primo", "proposta"], "la proposta resta in cima");
  mettiInPila(pila, "playlist", false);
  assert.deepEqual(pila, ["primo", "proposta", "playlist"], "quel che si apre dopo va in cima");
});

test("il primo avvio chiede il fondo a tutte e due le pile", () => {
  const primo = leggi("../src/Primo.tsx");
  assert.match(primo, /useFinestrella<HTMLDivElement>\(onSalta, "nonEUnaDomanda", \{\s*sotto: true,?\s*\}\)/);
  assert.match(primo, /useVeloInerte\(finestrella, undefined, \{ sotto: true \}\)/);
});

test("le due pile passano da mettiInPila, e il fondo non ruba il fuoco a chi sta sopra", () => {
  const finestrella = leggi("../src/finestrella.ts");
  assert.match(finestrella, /mettiInPila\(aperte, questa, sotto\)/);
  assert.match(finestrella, /mettiInPila\(veliAperti, aperto, sotto\)/);
  assert.doesNotMatch(finestrella, /aperte\.push\(/);
  assert.doesNotMatch(finestrella, /veliAperti\.push\(/);
  assert.match(finestrella, /const coperta = sotto && aperte\.length > 0;\s*if \(!coperta\) \{/);
});

test("«Rimettila»: il nome per il lettore di schermo contiene quel che si legge (WCAG 2.5.3)", () => {
  for (const lingua of ["it", "en"]) {
    const testi = JSON.parse(leggi(`../src/lingue/${lingua}.json`)) as Record<string, string>;
    for (const chiave of ["skin.shift.undo", "skin.replace.undo"]) {
      const visibile = testi[chiave];
      const nome = testi[`${chiave}.aria`];
      assert.ok(visibile && nome, `${lingua}: manca ${chiave}`);
      assert.ok(
        nome.toLowerCase().includes(visibile.toLowerCase()),
        `${lingua}: «${nome}» non contiene «${visibile}»`,
      );
    }
  }
});
