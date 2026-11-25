import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * «Torna alla copia di prima dell'aggiornamento» (D-55), e i testi che ci
 * mandano.
 *
 * Il ritorno vero lo provano le prove del nucleo (`db::manutenzione`:
 * `tornare_alla_copia_*`). Qui la metà che un DOM nelle prove non c'è per
 * vedere: che il gesto si disegni anche quando la libreria non si è aperta —
 * è il caso in cui `errors.db.migrationFailed` manda lì —, che non ci sia sul
 * telefono, che chieda conferma, e che arrivi a comandi registrati. E che le
 * frasi che nominano la copia `.bak` non dicano più di rimetterla senza
 * togliere il `-wal`, il difetto di R06b-2 e R17-6.
 */

const leggi = (percorso: string) =>
  readFileSync(new URL(percorso, import.meta.url), "utf8");

const IMPOSTAZIONI = leggi("../src/schermate/Impostazioni.tsx");
const COPIA = leggi("../src/parti/CopiaDiPrima.tsx");
const IPC = leggi("../src/ipc.ts");
const LIB = leggi("../src-tauri/src/lib.rs");
const IT = JSON.parse(leggi("../src/lingue/it.json")) as Record<string, string>;
const EN = JSON.parse(leggi("../src/lingue/en.json")) as Record<string, string>;

test("il gesto sta nella scheda della manutenzione, che si disegna anche senza libreria", () => {
  const scheda = IMPOSTAZIONI.slice(
    IMPOSTAZIONI.indexOf("function SchedaManutenzione("),
    IMPOSTAZIONI.indexOf("function fraseConto("),
  );
  assert.match(scheda, /\{!MOBILE && <CopiaDiPrima onErrore=\{onErrore\} \/>\}/);
  // La scheda non aspetta `avvio`: è quel che la rende raggiungibile con la
  // libreria chiusa.
  assert.match(IMPOSTAZIONI, /\{sezione === "dati" && <SchedaManutenzione onErrore=\{onErrore\} \/>\}/);
});

test("il gesto chiede conferma e arriva a comandi registrati", () => {
  assert.ok(COPIA.includes("ipc\n      .manutenzioneCopiaDiPrima()") || COPIA.includes("ipc.manutenzioneCopiaDiPrima()"));
  assert.ok(COPIA.includes(".manutenzioneTornaAllaCopia()"));
  assert.ok(COPIA.includes("<ConfermaGesto"), "un gesto che chiude Rymembra si conferma");
  // Il tasto che lo fa sta solo dentro la domanda.
  const fai = COPIA.slice(COPIA.indexOf("fai: () =>"));
  assert.ok(fai.includes("manutenzioneTornaAllaCopia"));
  assert.match(IPC, /invoke<CopiaDiPrima \| null>\("manutenzione_copia_di_prima"\)/);
  assert.match(IPC, /invoke<void>\("manutenzione_torna_alla_copia"\)/);
  assert.ok(LIB.includes("manutenzione::manutenzione_copia_di_prima,"));
  assert.ok(LIB.includes("manutenzione::manutenzione_torna_alla_copia,"));
});

test("le frasi che nominano la copia nominano anche il giornale, o il gesto", () => {
  for (const [lingua, testi] of [
    ["it", IT],
    ["en", EN],
  ] as const) {
    const titolo = testi["settings.restoreCopy.title"]!;
    for (const chiave of [
      "errors.db.migrationFailed",
      "errors.db.versionAhead",
      "db.damaged.how",
    ]) {
      const frase = testi[chiave]!;
      assert.ok(frase.includes(titolo), `${lingua} ${chiave}: non nomina il gesto`);
    }
    // L'unica che dice anche come rimettere la copia a mano dice di togliere
    // il giornale del database nuovo, e di copiare invece di rinominare.
    const aMano = testi["db.damaged.how"]!;
    assert.ok(aMano.includes("rymembra.db-wal"), `${lingua}: manca il -wal`);
    assert.ok(aMano.includes("rymembra.db-shm"), `${lingua}: manca lo -shm`);
    // Sul telefono il gesto non c'è, e la frase non lo nomina.
    for (const chiave of [
      "errors.db.migrationFailed.telefono",
      "errors.db.versionAhead.telefono",
      "db.damaged.how.telefono",
    ]) {
      const frase = testi[chiave];
      assert.ok(frase !== undefined, `${lingua}: manca ${chiave}`);
      assert.ok(!frase.includes(titolo), `${lingua} ${chiave}: nomina un gesto che lì non c'è`);
      assert.ok(!frase.includes(".bak"), `${lingua} ${chiave}: sul telefono la cartella è privata`);
    }
  }
  // La frase vecchia — «il database di prima è intatto» — non era vera dopo
  // un passo già fatto.
  assert.ok(!IT["errors.db.migrationFailed"]!.includes("intatto"));
  assert.ok(!EN["errors.db.migrationFailed"]!.includes("intact"));
});

test("la frase dell'aggiornamento fermo non afferma che la copia c'è (S3-3)", () => {
  // La copia si salta quando lo spazio libero non basta, e la scheda del gesto
  // compare solo se la copia c'è: la frase non può dire «ho copiato».
  const dice = {
    it: { prova: /prova a copiare/, salta: /la salta se non c'è abbastanza spazio/, vieta: /ha copiato/ },
    en: { prova: /tries to copy/, salta: /skips the copy if there isn’t enough free space/, vieta: /copied the library/ },
  } as const;
  for (const [lingua, testi] of [
    ["it", IT],
    ["en", EN],
  ] as const) {
    const frase = testi["errors.db.migrationFailed"]!;
    assert.match(frase, dice[lingua].prova, `${lingua}: non dice che è un tentativo`);
    assert.match(frase, dice[lingua].salta, `${lingua}: non dice che la copia si può saltare`);
    assert.doesNotMatch(frase, dice[lingua].vieta, `${lingua}: afferma la copia`);
    // Sul telefono la copia non è mai un'offerta, e non si promette.
    const telefono = testi["errors.db.migrationFailed.telefono"]!;
    assert.doesNotMatch(telefono, /copia|copy/i, `${lingua}: il telefono promette una copia`);
  }
});
