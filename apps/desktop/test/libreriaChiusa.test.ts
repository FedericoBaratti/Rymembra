import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * Una libreria che non si apre lascia una finestra che dice perché, e una
 * strada per la cartella dei dati.
 *
 * Il nucleo ripiega sulla skin di serie quando la libreria non c'è
 * (`skin.rs`, `senza_libreria_si_indossa_quella_di_serie`): così la finestra si
 * disegna, e la fascia mostra l'errore di `avvio`. Qui si controlla l'altra
 * metà, quella che un DOM nelle prove non c'è per vedere: che la fascia, per
 * i guasti del database, offra di aprire la cartella dei dati — dove sta la
 * copia di prima dell'aggiornamento — e che il gesto arrivi a un comando che
 * esiste davvero. Sul telefono la cartella è privata, e il gesto non c'è.
 */

const leggi = (percorso: string) =>
  readFileSync(new URL(percorso, import.meta.url), "utf8");

const APP = leggi("../src/App.tsx");
const IPC = leggi("../src/ipc.ts");
const LIB = leggi("../src-tauri/src/lib.rs");

/** Il corpo di `azioneDelGuasto`, fino alla fascia che lo usa. */
function azioneDelGuasto(): string {
  const inizio = APP.indexOf("const azioneDelGuasto");
  const fine = APP.indexOf("const notizieDiFascia", inizio);
  assert.ok(inizio >= 0 && fine > inizio, "azioneDelGuasto non si trova in App.tsx");
  return APP.slice(inizio, fine);
}

test("i guasti del database offrono la cartella dei dati, non sul telefono", () => {
  const corpo = azioneDelGuasto();
  for (const codice of [
    "db.migrationFailed",
    "db.versionAhead",
    "db.openFailed",
    "db.corrupt",
  ]) {
    assert.ok(corpo.includes(`case "${codice}":`), `manca il caso ${codice}`);
  }
  const ramo = corpo.slice(corpo.indexOf('case "db.migrationFailed":'));
  const gesto = ramo.slice(0, ramo.indexOf("default:"));
  assert.match(gesto, /MOBILE\s*\?\s*undefined/, "sul telefono il gesto non c'è");
  assert.ok(gesto.includes("ipc.manutenzioneApriCartellaDati()"));
  assert.ok(gesto.includes('t("errors.action.dataFolder")'));
});

test("il gesto arriva a un comando registrato", () => {
  assert.match(
    IPC,
    /manutenzioneApriCartellaDati:\s*\(\)\s*=>\s*invoke<void>\("manutenzione_apri_cartella_dati"\)/,
  );
  assert.ok(LIB.includes("manutenzione::manutenzione_apri_cartella_dati,"));
});
