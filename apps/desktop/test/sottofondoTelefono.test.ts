/**
 * «Resta attiva in sottofondo» nella scheda «Dispositivi» del telefono (F6):
 * quel che la finestra deve dire, e dove.
 *
 * Senza un DOM nelle prove: la regola delle note è una funzione pura
 * (`sottofondo.ts`), e il cablaggio — dove sta la riga, quali comandi chiama,
 * quali frasi non restano vere col servizio acceso — si guarda nei sorgenti.
 * La verifica dal vivo (la notifica, «Spegni», il Doze) sta nel report della
 * fase.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { noteDelSottofondo } from "../src/sottofondo";

const sorgente = (percorso: string) =>
  readFileSync(new URL(percorso, import.meta.url), "utf8").replace(/\r\n/g, "\n");
/** Il sorgente senza commenti: i commenti nominano quel che il codice non fa. */
const senzaCommenti = (testo: string) =>
  testo.replace(/\/\*[\s\S]*?\*\/|\/\/.*$/gm, "").replace(/\{\/\*[\s\S]*?\*\/\}/g, "");

test("spento, il sottofondo non ha note: non c'è niente da dire", () => {
  for (const senza of [true, false, null]) {
    assert.deepEqual(noteDelSottofondo(false, true, senza), { internet: false, batteria: null });
    assert.deepEqual(noteDelSottofondo(false, false, senza), { internet: false, batteria: null });
  }
});

test("acceso senza «Raggiungibile da internet», dice che serve anche quello", () => {
  assert.equal(noteDelSottofondo(true, false, null).internet, true);
  assert.equal(noteDelSottofondo(true, true, null).internet, false);
});

test("la nota della batteria segue lo stato di Android, e senza stato tace", () => {
  // Dentro il risparmio: la spiegazione col pulsante che apre la pagina.
  assert.equal(noteDelSottofondo(true, true, false).batteria, "dentro");
  // Fuori: basta dirlo.
  assert.equal(noteDelSottofondo(true, true, true).batteria, "fuori");
  // Il ponte non ha risposto: meglio niente di una nota che può essere falsa.
  assert.equal(noteDelSottofondo(true, true, null).batteria, null);
});

test("la riga sta nella scheda «Dispositivi», solo sul telefono, dopo «Raggiungibile da internet»", () => {
  const scheda = sorgente("../src/parti/Dispositivi.tsx");
  assert.match(scheda, /\{MOBILE && <SottofondoTelefono\b/);
  assert.ok(
    scheda.indexOf("settings.devices.reachable\"") < scheda.indexOf("<SottofondoTelefono"),
    "il sottofondo viene dopo «Raggiungibile da internet»",
  );
  assert.ok(
    scheda.indexOf("<SottofondoTelefono") < scheda.indexOf("settings.devices.mobileData\""),
    "e prima dei dati mobili: sono le righe di quel che il telefono fa da solo",
  );
});

test("con il sottofondo acceso non restano dette le frasi che diventano false", () => {
  const scheda = senzaCommenti(sorgente("../src/parti/Dispositivi.tsx"));
  // «Il collegamento riposa»: non riposa più.
  const riposa = scheda.slice(
    scheda.lastIndexOf("MOBILE &&", scheda.indexOf("settings.devices.asleep")),
    scheda.indexOf("settings.devices.asleep"),
  );
  assert.match(riposa, /!stato\.sottofondo/);
  // «…mentre Rymembra è aperto o sta suonando»: con il sottofondo, anche chiusa.
  assert.match(scheda, /stato\.sottofondo\s*\?\s*"settings\.devices\.background\.reachable\.sub"/);
});

test("il pulsante apre la pagina di sistema e non chiede l'esenzione diretta", () => {
  const riga = senzaCommenti(sorgente("../src/parti/SottofondoTelefono.tsx"));
  assert.match(riga, /ipc\.telefonoApriRisparmioBatteria\(\)/);
  assert.match(riga, /ipc\.lontanoSottofondo\(acceso\)/);
  // Lo stato si rilegge quando l'app torna in vista: lo si cambia fuori di qui.
  assert.match(riga, /visibilitychange/);
  assert.match(riga, /ipc\s*\.telefonoSenzaRisparmioBatteria\(\)/);
});

test("la voce dell'indice delle impostazioni c'è, solo sul telefono, e ha le sue frasi", () => {
  const impostazioni = sorgente("../src/schermate/Impostazioni.tsx");
  assert.match(
    impostazioni,
    /\.\.\.\(MOBILE \? \[v\("dispositivi", "daemon", "settings\.devices\.title"\)\] : \[\]\)/,
  );
  for (const lingua of ["it", "en"]) {
    const frasi = JSON.parse(sorgente(`../src/lingue/${lingua}.json`)) as Record<string, string>;
    assert.ok(frasi["settings.entry.daemon"], `${lingua}: manca settings.entry.daemon`);
    assert.ok(frasi["settings.entry.daemon.syn"], `${lingua}: mancano i sinonimi`);
    // Il costo in batteria sta scritto nella spiegazione, come dice la guida.
    assert.match(
      frasi["settings.devices.background.sub"] ?? "",
      lingua === "it" ? /batteria/ : /battery/,
    );
  }
});

test("i tre comandi nuovi della finestra esistono nel nucleo e hanno lo stesso nome", () => {
  const ipc = sorgente("../src/ipc.ts");
  const nucleo = sorgente("../src-tauri/src/lib.rs");
  for (const [lato, comando] of [
    ["lontanoSottofondo", "lontano_sottofondo"],
    ["telefonoSenzaRisparmioBatteria", "telefono_senza_risparmio_batteria"],
    ["telefonoApriRisparmioBatteria", "telefono_apri_risparmio_batteria"],
  ] as const) {
    assert.match(ipc, new RegExp(lato + ":[^]*?invoke<[^>]*>[(]\"" + comando + "\""));
    assert.match(nucleo, new RegExp(`::${comando},`), `lib.rs non registra ${comando}`);
  }
});
