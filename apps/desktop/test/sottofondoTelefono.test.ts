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

import { noteDelSottofondo, rigaDellAuto } from "../src/sottofondo";

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

// ── «Parti con l'auto» (F7) ──────────────────────────────────────────────────

test("la riga dell'auto non c'è dove la build non la offre, e dice che serve il sottofondo", () => {
  // La variante per Google Play toglie la funzione con la costante di build
  // `PARTENZA_CON_AUTO`: lì l'interruttore non compare, né spento né impedito.
  assert.equal(rigaDellAuto(false, true), "nascosta");
  assert.equal(rigaDellAuto(false, false), "nascosta");
  // Offerta ma col sottofondo spento: impedita, con la sua ragione.
  assert.equal(rigaDellAuto(true, false), "serveIlSottofondo");
  assert.equal(rigaDellAuto(true, true), "libera");
});

test("la riga dell'auto sta sotto il sottofondo, spenta di serie, e chiama il suo comando", () => {
  const riga = senzaCommenti(sorgente("../src/parti/SottofondoTelefono.tsx"));
  assert.match(riga, /rigaDellAuto\(stato\.partenzaConAuto, stato\.sottofondo\)/);
  assert.match(riga, /auto !== "nascosta" && \(/);
  assert.match(riga, /ipc\.lontanoPartiConAuto\(acceso\)/);
  assert.match(riga, /acceso=\{stato\.partiConAuto\}/);
  assert.match(riga, /impedito=\{auto === "serveIlSottofondo"/);
  // Dopo il sottofondo e le sue note, non prima.
  assert.ok(riga.indexOf("settings.devices.background.makers") < riga.indexOf("settings.devices.car\""));
});

test("«Parti con l'auto» dice la cosa com'è, in italiano e in inglese, nello spazio settings.devices.car", () => {
  for (const lingua of ["it", "en"]) {
    const frasi = JSON.parse(sorgente(`../src/lingue/${lingua}.json`)) as Record<string, string>;
    for (const chiave of [
      "settings.devices.car",
      "settings.devices.car.sub",
      "settings.devices.car.needsBackground",
    ]) {
      assert.ok(frasi[chiave], `${lingua}: manca ${chiave}`);
    }
  }
  const it = JSON.parse(sorgente("../src/lingue/it.json")) as Record<string, string>;
  // Il testo onesto della guida: riparte da sola, **al posto** dell'ultima app.
  assert.ok(
    (it["settings.devices.car.sub"] ?? "").startsWith(
      "Quando colleghi l'auto, Rymembra riparte da sola al posto dell'ultima app usata",
    ),
  );
});

test("il pannello porta alla finestra la scelta e se la build la offre, e il comando esiste", () => {
  const tipi = sorgente("../src/ipc/lontano.ts");
  assert.match(tipi, /partiConAuto: boolean;/);
  assert.match(tipi, /partenzaConAuto: boolean;/);
  const ipc = sorgente("../src/ipc.ts");
  assert.match(ipc, /lontanoPartiConAuto:[^]*?invoke<StatoLontano>\("lontano_parti_con_auto"/);
  assert.match(sorgente("../src-tauri/src/lib.rs"), /::lontano_parti_con_auto,/);
  // Il pannello Rust ha gli stessi campi, con gli stessi nomi (camelCase).
  const pannello = sorgente("../src-tauri/src/lontano/ipc.rs");
  assert.match(pannello, /pub parti_con_auto: bool,/);
  assert.match(pannello, /pub partenza_con_auto: bool,/);
});

// ── la proposta del sottofondo sul telefono (F7, §8.4) ──────────────────────

test("la proposta del telefono usa le sue parole, la stessa chiave, e lo stesso comando dell'interruttore", () => {
  const proposta = sorgente("../src/parti/PropostaSottofondo.tsx");
  const codice = senzaCommenti(proposta);
  // Le parole del telefono sono dietro `MOBILE`, quelle del computer no.
  for (const chiave of [
    "background.proposal.phone.text",
    "background.proposal.phone.yes",
    "background.proposal.phone.no",
    "background.proposal.phone.needsInternet",
  ]) {
    assert.ok(codice.includes(`"${chiave}"`), `il componente non usa «${chiave}»`);
  }
  assert.match(codice, /import \{ MOBILE \} from "\.\.\/piattaforma"/);
  // «Sì» ricorda la risposta e poi accende il sottofondo **con il comando
  // dell'interruttore** (che chiede anche le notifiche); «No» non accende niente.
  const risponde = codice.slice(codice.indexOf("const risponde"));
  assert.ok(risponde.indexOf("propostaSottofondoRispondi(accetta)") >= 0);
  assert.ok(
    risponde.indexOf("propostaSottofondoRispondi(accetta)") < risponde.indexOf("ipc.lontanoSottofondo(true)"),
    "la risposta si ricorda prima di accendere il servizio",
  );
  assert.match(risponde, /if \(!accetta\) return;\s*const stato = await ipc\.lontanoSottofondo\(true\)/);
  // Il sottofondo da solo non apre niente verso internet: lo si dice.
  assert.match(risponde, /if \(!stato\.attivo\) onNotizia\(t\("background\.proposal\.phone\.needsInternet"\)\)/);
  // Una chiave sola per tutti e due: niente `ui.proposta.sottofondo` nuovo.
  assert.doesNotMatch(codice, /proposta\.sottofondo\.(phone|telefono)/);
});

test("la proposta del telefono dice quel che la guida promette, in italiano e in inglese", () => {
  const it = JSON.parse(sorgente("../src/lingue/it.json")) as Record<string, string>;
  const en = JSON.parse(sorgente("../src/lingue/en.json")) as Record<string, string>;
  assert.equal(
    it["background.proposal.phone.text"],
    "Vuoi che Rymembra resti attiva in sottofondo, così i tuoi PC la raggiungono sempre?",
  );
  assert.equal(it["background.proposal.phone.no"], "No, grazie");
  for (const chiave of [
    "background.proposal.phone.text",
    "background.proposal.phone.yes",
    "background.proposal.phone.no",
    "background.proposal.phone.needsInternet",
  ]) {
    assert.ok(it[chiave] && en[chiave], `«${chiave}» manca in una delle lingue`);
    assert.ok(!(en[chiave] ?? "").includes("'"), `${chiave}: apostrofo dritto in inglese`);
  }
});

test("la proposta si monta anche sul telefono, e aspetta come sul computer", () => {
  const app = sorgente("../src/App.tsx");
  assert.match(app, /<PropostaSottofondo\s+pronta=\{propostaPuoMostrarsi\}/);
  assert.doesNotMatch(app, /\{!MOBILE && \(\s*<PropostaSottofondo/);
  // Il punto in cui si decide quando aspettare è uno solo, e non è cambiato.
  const riga = /const propostaPuoMostrarsi = ([^;]+);/.exec(app)?.[1] ?? "";
  for (const motivo of ["primoVisibile", "giroDaFare", "giroAperto"]) {
    assert.ok(riga.includes(`!${motivo}`), `la proposta non aspetta «${motivo}»`);
  }
});
