/**
 * L'errore del lettore a finestra distrutta (S4-5): la pagina nuova lo chiede.
 *
 * Il nucleo tiene l'ultimo errore emesso mentre nessuna pagina ascoltava
 * (`src-tauri/src/perso.rs`, che prova la regola); qui c'è il cablaggio dalla
 * parte della pagina.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

/*
 * S4-5. `riproduzione:errore` è un evento senza stato dietro: emesso a finestra
 * distrutta si perdeva, e «Mostra» ricreava una pagina con il lettore fermo e
 * nessun perché. La pagina chiede l'errore perso **dopo** aver aperto l'ascolto
 * (`useAscolto`, quarto argomento): chiederlo prima lascerebbe una finestra in cui
 * un errore arriva, non trova nessuno e non è nemmeno nella risposta.
 */
test("S4-5: la pagina chiede l'errore perso dopo aver aperto l'ascolto", () => {
  const riproduzione = sorgente("riproduzione.ts");
  assert.match(
    riproduzione,
    /useAscolto<ErroreIpc>\("riproduzione:errore", setErrore, true, \(\) => \{[\s\S]*?ipc\s*\.riproduzioneErrorePerso\(\)[\s\S]*?setErrore\(perso\)/,
  );
  const ipc = sorgente("ipc.ts");
  assert.match(ipc, /riproduzioneErrorePerso: \(\) => invoke<ErroreIpc \| null>\("riproduzione_errore_perso"\)/);
});
