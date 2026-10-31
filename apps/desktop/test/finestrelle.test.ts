import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * Prove sui sorgenti delle finestrelle, dove la regola sta nel cablaggio e non
 * in una funzione: chi chiude e quando, dove va il fuoco, a chi arriva un
 * esito. Montarle vorrebbe dire un DOM e il nucleo finto; qui si legge il
 * sorgente e si cerca il punto in cui la regola è scritta, come fanno
 * `impostazioni.test.ts` e `insiemeGesti.test.ts`.
 */
const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

test("Account: mentre importa, Escape, il velo e «Annulla» non chiudono", () => {
  const account = sorgente("Account.tsx");
  assert.match(account, /useFinestrella<HTMLDivElement>\(scrivendo \? restaAperta : onChiudi\)/);
  assert.match(account, /gesto\.clic\(e\.target, e\.currentTarget\) && !scrivendo\) onChiudi\(\)/);
  assert.match(account, /onClick=\{onChiudi\}\s+disabled=\{scrivendo\}/);
  // Si alza solo per la scrittura, non per l'apertura dell'archivio o il piano.
  const importa = account.slice(account.indexOf("const importa = async"), account.indexOf("const dimentica = async"));
  assert.match(importa, /setScrivendo\(true\)/);
});

test("Importa e ImportaLink: mentre scrivono il dialogo non si chiude", () => {
  const importa = sorgente("Importa.tsx");
  const dialoghi = [...importa.matchAll(/<Dialogo\b[^>]*>/g)].map((m) => m[0]);
  assert.equal(dialoghi.length, 2);
  for (const dialogo of dialoghi) assert.match(dialogo, /chiudibile=\{!scrivendo\}/);
  assert.equal([...importa.matchAll(/onClick=\{onChiudi\} disabled=\{scrivendo\}/g)].length, 2);
});

test("Ripristino: chiuso a metà, l'esito arriva alla finestra", () => {
  const ripristino = sorgente("Ripristino.tsx");
  assert.match(ripristino, /if \(aperta\.current\) setErrore\(testoErrore\(e\)\);\s+else onErrore\(e\);/);
  assert.match(ripristino, /if \(!aperta\.current\) \{\s+onNotizia\(/);
  const app = sorgente("App.tsx");
  const montato = app.slice(app.indexOf("<Ripristino"), app.indexOf("/>", app.indexOf("onNotizia=", app.indexOf("<Ripristino"))));
  assert.match(montato, /onErrore=\{segnalaErrore\}/);
  assert.match(montato, /onNotizia=\{setNotizia\}/);
});

test("Ripristino: l'avanzamento del backup automatico non si mostra come ripristino", () => {
  const ripristino = sorgente("Ripristino.tsx");
  assert.doesNotMatch(ripristino, /useAscolto<AvanzamentoNuvola>\("nuvola:avanzamento", setAvanzamento\)/);
  assert.match(ripristino, /if \(eseguendo\.current\) setAvanzamento\(p\)/);
});

test("NuovoTema: le frecce spostano il fuoco, non solo la scelta", () => {
  const nuovo = sorgente("NuovoTema.tsx");
  const daTastiera = nuovo.slice(nuovo.indexOf("const daTastiera"), nuovo.indexOf("};", nuovo.indexOf("const daTastiera")));
  // Il fuoco prima della scelta, sul bottone d'arrivo: come `Segmentato`.
  assert.match(daTastiera, /bottoni\.current\[dove\]\?\.focus\(\);\s+setBase\(prossima\.id\);/);
  assert.match(nuovo, /bottoni\.current\[indice\] = nodo;/);
});

test("ConfermaGesto: il fuoco aprendo va su «Annulla», anche con la casella", () => {
  const conferma = sorgente("ConfermaGesto.tsx");
  const annulla = conferma.slice(conferma.lastIndexOf("<button", conferma.indexOf('t("common.cancel")')), conferma.indexOf('t("common.cancel")'));
  assert.match(annulla, /data-fuoco-iniziale/);
});

test("Account: «Tolte N righe» resta a schermo quando la scheda degli importati sparisce", () => {
  const account = sorgente("Account.tsx");
  const inizio = account.indexOf("stato.ascoltiImportati > 0 && (");
  const scheda = account.slice(inizio, account.indexOf("</section>", inizio));
  // Dimenticati gli ascolti, `ascoltiImportati` torna a zero e la scheda si
  // smonta: il numero dentro di lei durava un giro di IPC.
  assert.doesNotMatch(scheda, /account\.forgotten/);
  assert.match(account, /t\("account\.forgotten", \{ n: dimenticati \}\)/);
});
