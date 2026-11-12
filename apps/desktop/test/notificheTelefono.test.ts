/**
 * Le notifiche si chiedono al primo bisogno vero, non al primo avvio (D-36).
 *
 * «Consenti l'accesso» del primo avvio apriva due dialoghi di sistema, la
 * musica e poi «Inviare notifiche?», e la schermata parlava solo del primo
 * (R11-6). La notifica del lettore non ne ha bisogno — Android esenta quelle
 * delle sessioni multimediali —; ne hanno bisogno «Scarica qui» e l'attesa del
 * consenso di un servizio da collegare, ed è lì che adesso si chiedono. Il
 * Kotlin non ha prove unitarie: qui il cablaggio.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const KOTLIN = "../src-tauri/gen/android/app/src/main/java/io/github/federicobaratti/rymembra/";
const leggi = (percorso: string) =>
  readFileSync(new URL(percorso, import.meta.url), "utf8").replace(/\/\/.*$|\/\*[\s\S]*?\*\//gm, "");

/** Il corpo della funzione Kotlin `nome`, dalla firma alla graffa che la chiude. */
function corpo(sorgente: string, nome: string): string {
  const inizio = sorgente.indexOf(`fun ${nome}(`);
  assert.ok(inizio >= 0, `manca fun ${nome}`);
  let profondita = 0;
  for (let i = sorgente.indexOf("{", inizio); i < sorgente.length; i++) {
    if (sorgente[i] === "{") profondita++;
    else if (sorgente[i] === "}" && --profondita === 0) return sorgente.slice(inizio, i + 1);
  }
  return sorgente.slice(inizio);
}

test("il dialogo dei permessi del primo avvio non chiede le notifiche", () => {
  const permessi = leggi(`${KOTLIN}Permessi.kt`);
  assert.doesNotMatch(corpo(permessi, "chiedi"), /POST_NOTIFICATIONS/);
});

test("le notifiche si chiedono quando partono «Scarica qui» e l'attesa del consenso", () => {
  const ponte = leggi(`${KOTLIN}Ponte.kt`);
  for (const comando of ['"sistema.scarico"', '"sistema.attesaConsenso"']) {
    const da = ponte.indexOf(comando);
    assert.ok(da >= 0, `manca ${comando}`);
    const ramo = ponte.slice(da, ponte.indexOf("\n      \"", da + comando.length));
    assert.match(ramo, /Permessi\.notificheAlBisogno\(/, `${comando} non chiede le notifiche`);
  }
});
