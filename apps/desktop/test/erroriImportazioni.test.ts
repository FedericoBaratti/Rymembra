import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * Le finestrelle d'importazione dicono i loro errori a chi usa un lettore di
 * schermo (R13c-7).
 *
 * Il nodo dell'errore nasce già pieno: senza un ruolo vivo nessuno lo legge, e
 * con NVDA «Importa» sembra non aver fatto niente. Ogni `className="errore"`
 * di questi tre file porta `role="alert"`, come in `AggiungiCartella.tsx` e
 * `AggiungiAPlaylist.tsx`.
 */
const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");

const FILE = ["../src/Account.tsx", "../src/Importa.tsx", "../src/ImportaPlaylist.tsx"];

test("gli errori delle finestrelle d'importazione si annunciano", () => {
  for (const file of FILE) {
    const sorgente = leggi(file);
    const nodi = [...sorgente.matchAll(/<div\b[^>]*className="errore"[^>]*>/g)].map((m) => m[0]);
    assert.ok(nodi.length > 0, `${file}: non trovo l'errore, la prova va aggiornata`);
    for (const nodo of nodi) {
      assert.match(nodo, /role="alert"/, `${file}: ${nodo}`);
    }
  }
});

test("mentre l'account scrive, la × della finestra non resta senza effetto", () => {
  // `useFinestrella` di serie dice «domanda chiudibile»: la × allora chiama la
  // chiusura della finestrella, che mentre scrive non fa niente. Detto che non
  // si chiude, la × chiude la finestra come Alt+F4 (D-41, `cosaFaLaX`).
  const sorgente = leggi("../src/Account.tsx");
  assert.match(
    sorgente,
    /useFinestrella<HTMLDivElement>\([^;]*\{\s*chiudibile:\s*!scrivendo,?\s*\}\s*\)/,
  );
});
