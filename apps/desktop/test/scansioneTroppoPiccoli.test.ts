import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * I file audio troppo piccoli si dicono nell'esito della scansione (L-PC 5).
 *
 * `musica-rotta` ha cinque file audio rotti; la scansione ne dichiarava uno
 * («· 1 illeggibile») e taceva sugli altri quattro, che stanno sotto la soglia
 * di un brano e il piano lascia fuori prima di leggerli. L'esclusione è voluta
 * (un download interrotto non deve diventare un brano), ma deve avere un
 * numero. Il conteggio lo fa il nucleo (`troppoPiccoli`); qui il filo fino allo
 * schermo, letto nei sorgenti.
 */

const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");
const LINGUE = {
  it: JSON.parse(leggi("../src/lingue/it.json")) as Record<string, string>,
  en: JSON.parse(leggi("../src/lingue/en.json")) as Record<string, string>,
};

test("l'esito porta il numero dei troppo piccoli, e la scheda lo scrive accanto agli illeggibili", () => {
  assert.match(leggi("../src/ipc/libreria.ts"), /troppoPiccoli: number;/);
  const scheda = leggi("../src/schermate/Impostazioni.tsx");
  assert.match(
    scheda,
    /esito\.illeggibili > 0 &&\s*t\("settings\.scan\.unreadable"[^)]*\)\}\s*\{\/\*[\s\S]*?\*\/\}\s*\{esito\.troppoPiccoli > 0 &&\s*t\("settings\.scan\.tooSmall", \{ n: esito\.troppoPiccoli \}\)\}/,
  );
});

test("la frase c'è nelle due lingue, con la forma per uno e senza il numero della soglia scritto a mano", () => {
  for (const [lingua, voci] of Object.entries(LINGUE)) {
    const piu = voci["settings.scan.tooSmall"];
    const uno = voci["settings.scan.tooSmall.uno"];
    assert.ok(piu !== undefined && uno !== undefined, `${lingua}: manca la frase o la sua forma per uno`);
    assert.match(piu, /\{n\}/, `${lingua}: il numero non c'è`);
    assert.doesNotMatch(uno, /\{n\}/, `${lingua}: «${uno}» ha ancora il segnaposto`);
    assert.match(uno, /^ · 1 /, `${lingua}: «${uno}» non dice uno`);
    // La soglia è del nucleo (`MIN_TRACK_BYTES`): scritta qui andrebbe fuori sincrono.
    assert.doesNotMatch(piu + uno, /\b32\b/, `${lingua}: la soglia è scritta a mano`);
  }
});
