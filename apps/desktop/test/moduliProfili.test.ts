import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * I moduli dei profili IA e di Last.fm non promettono gesti che non vanno
 * (R13-14).
 *
 * - «Modifica» su un profilo, poi «Elimina» sullo stesso: il modulo restava
 *   su «Modifica profilo» con l'id di un profilo che non c'è più, e «Prova» e
 *   «Salva» rispondevano con un errore del nucleo.
 * - «Usa queste credenziali» era premibile a campi vuoti, e rispondeva con un
 *   errore invece di non esserlo.
 */
const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");

test("eliminare il profilo aperto nel modulo lo chiude", () => {
  const sorgente = leggi("../src/parti/ModelliIA.tsx");
  const da = sorgente.indexOf("ipc.iaEliminaProfilo(profilo.id)");
  assert.ok(da > 0, "non trovo l'eliminazione: la prova va aggiornata");
  const dopo = sorgente.slice(da, sorgente.indexOf("})", da) + 2);
  assert.match(dopo, /dimenticaSeAperto\(profilo\.id\)/);
  const corpo = sorgente.slice(sorgente.indexOf("const dimenticaSeAperto = useStabile("));
  const funzione = corpo.slice(0, corpo.indexOf("});") + 3);
  assert.match(funzione, /if \(bozza\.id !== id\) return;/);
  assert.match(funzione, /setBozza\(vuota\(bozza\.fornitore\)\)/);
  assert.match(funzione, /setChiave\(""\)/);
});

test("«Usa queste credenziali» è spento a campi vuoti", () => {
  const sorgente = leggi("../src/parti/Scrobbling.tsx");
  const dove = sorgente.indexOf('t("scrobble.lfm.use")');
  const tasto = sorgente.slice(sorgente.lastIndexOf("<button", dove), dove);
  const spento = /disabled=\{([^}]*)\}/.exec(tasto)?.[1] ?? "";
  assert.match(spento, /apiKey\.trim\(\) === ""/);
  assert.match(spento, /segreto\.trim\(\) === ""/);
});
