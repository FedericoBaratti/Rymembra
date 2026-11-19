/**
 * Il protocollo `rymembra-cover` non aspetta mai la rete (FC1, la WebView
 * bloccata sul telefono vero): a una copertina che lo store non ha ancora
 * risponde «non c'è» subito, il nucleo la chiede in disparte, e la finestra —
 * `lontano:copertina` — la richiede quando arriva.
 *
 * Quel che il gestore risponde, e quanto in fretta, lo prova `media/lontane.rs`
 * con una rete finta ferma. Qui sta la parte della finestra: gli arrivi si
 * raccolgono, e solo i segnaposto si iscrivono.
 */
import { test, mock } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { RITARDO_COPERTINE_MS, archivioCopertine, copertinaArrivata } from "../src/lontano";

const leggi = (percorso: string) =>
  readFileSync(new URL(percorso, import.meta.url), "utf8").replace(/\r\n/g, "\n");

test("duecento copertine arrivate in fila cambiano il conto una volta sola", () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  try {
    const prima = archivioCopertine.leggi();
    let avvisi = 0;
    const smetti = archivioCopertine.iscrivi(() => {
      avvisi += 1;
    });
    for (let n = 0; n < 200; n += 1) copertinaArrivata();
    assert.equal(archivioCopertine.leggi(), prima, "non subito: si raccolgono");
    mock.timers.tick(RITARDO_COPERTINE_MS);
    assert.equal(archivioCopertine.leggi(), prima + 1);
    assert.equal(avvisi, 1, "i segnaposto sono avvisati una volta");
    // Una raccolta nuova, un cambio nuovo.
    copertinaArrivata();
    mock.timers.tick(RITARDO_COPERTINE_MS);
    assert.equal(archivioCopertine.leggi(), prima + 2);
    smetti();
  } finally {
    mock.timers.reset();
  }
});

test("la finestra ascolta lontano:copertina, e solo il segnaposto si iscrive", () => {
  const app = leggi("../src/App.tsx");
  assert.match(app, /useAscolto<string>\("lontano:copertina", \(\) => copertinaArrivata\(\)\)/);
  const copertina = leggi("../src/Copertina.tsx");
  // Il segnaposto ci riprova con un indirizzo nuovo, che il gestore ignora.
  assert.match(copertina, /<RiprovaAllArrivo onRiprova=\{riprova\} \/>/);
  assert.match(copertina, /`\$\{base\}\?r=\$\{tentativo\}`/);
  // L'abbonamento sta nel segnaposto e non nel componente di ogni riga: una
  // copertina che ha la sua immagine non aspetta niente.
  const corpo = copertina.slice(copertina.indexOf("export function Copertina("));
  assert.doesNotMatch(corpo, /useArchivio\(archivioCopertine\)/);
  const riprova = copertina.slice(
    copertina.indexOf("function RiprovaAllArrivo("),
    copertina.indexOf("export function Copertina("),
  );
  assert.match(riprova, /useArchivio\(archivioCopertine\)/);
});

test("il gestore del protocollo non chiede la rete e non la aspetta", () => {
  const gestore = leggi("../src-tauri/src/copertine.rs");
  const servi = gestore.slice(
    gestore.indexOf("pub fn servi("),
    gestore.indexOf("\n}\n", gestore.indexOf("pub fn servi(")),
  );
  // Risponde da `media::lontane::servi`, che risponde prima di chiedere.
  assert.match(servi, /crate::media::lontane::servi\(/);
  // E non va più a prendere la copertina dall'altro dispositivo da sé.
  assert.doesNotMatch(servi, /copertina_di_una_riga/);
  assert.doesNotMatch(gestore, /fn dal_dispositivo/);
  // Il «non c'è» non si ricorda: la copertina può arrivare fra un attimo.
  assert.match(servi, /risposta_con\(404, "text\/plain", Vec::new\(\), "no-store"\)/);
});
