/**
 * Collegare due o più PC (F4, richiesta #2). Il collegamento col link
 * copiato e incollato c'era già; mancava quel che lo rende trovabile e
 * sopportabile con più di un computer:
 *
 * - i testi non parlavano che del telefono, e il campo «Incolla il link» sul PC
 *   non aveva nemmeno un titoletto;
 * - la finestrella mostrava `richieste[0]` e basta: chi aspettava dietro non
 *   si vedeva;
 * - due PC si presentano come «Computer» e si distinguono solo per il nome, che
 *   è quello del computer: al collegamento si propone di dargliene uno migliore;
 * - «un link nuovo annulla il precedente» non stava scritto da nessuna parte.
 *
 * Senza un DOM: le regole sono funzioni pure di `lontano.ts`, e il resto si
 * guarda nel sorgente dei componenti e nei file delle lingue.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import type { DispositivoLontano, RichiestaLontano } from "../src/ipc";
import {
  FINESTRA_COLLEGATO_MS,
  appenaCollegato,
  proponiNome,
  richiesteInFila,
} from "../src/lontano";

const leggi = (percorso: string) =>
  readFileSync(new URL(percorso, import.meta.url), "utf8").replace(/\r\n/g, "\n");
const DISPOSITIVI = leggi("../src/parti/Dispositivi.tsx");
const IT = JSON.parse(leggi("../src/lingue/it.json")) as Record<string, string>;
const EN = JSON.parse(leggi("../src/lingue/en.json")) as Record<string, string>;

function richiesta(id: number, nome: string): RichiestaLontano {
  return { id, nome, piattaforma: "windows", impronta: `K7QX 3MPA ${id}` };
}

function dispositivo(nome: string, campi: Partial<DispositivoLontano> = {}): DispositivoLontano {
  return {
    chiave: nome.toLowerCase(),
    nome,
    piattaforma: "windows",
    dispositivo: nome,
    online: true,
    vistoMs: null,
    associatoMs: 0,
    revocato: false,
    versione: null,
    insiemeAcceso: false,
    insiemeOspita: false,
    insieme: null,
    via: null,
    ...campi,
  };
}

// ── le richieste in fila ────────────────────────────────────────────────────

test("le richieste di più PC si mettono in fila: una in testa, e quante aspettano dietro", () => {
  assert.deepEqual(richiesteInFila([]), { adesso: null, dietro: 0 });
  const una = richiesteInFila([richiesta(7, "STUDIO")]);
  assert.equal(una.adesso?.id, 7);
  assert.equal(una.dietro, 0);
  // Nell'ordine in cui sono arrivate: la più vecchia per prima, e le altre
  // aspettano il loro turno (non si perdono, e non si saltano).
  const tre = richiesteInFila([richiesta(7, "STUDIO"), richiesta(9, "SALOTTO"), richiesta(12, "PORTATILE")]);
  assert.equal(tre.adesso?.id, 7);
  assert.equal(tre.dietro, 2);
});

test("la finestrella di una richiesta dice quante altre aspettano, e non guarda più richieste[0]", () => {
  assert.doesNotMatch(DISPOSITIVI, /richieste\[0\]/, "si guardava solo la prima richiesta, senza dirlo");
  assert.match(DISPOSITIVI, /richiesteInFila\(/);
  assert.match(DISPOSITIVI, /t\("devices\.link\.queue", \{ n: dietro \}\)/);
  // Il plurale: una sola altra, o più d'una.
  for (const [lingua, testi] of [["it", IT], ["en", EN]] as const) {
    assert.ok(testi["devices.link.queue"], `${lingua}: manca devices.link.queue`);
    assert.ok(testi["devices.link.queue.uno"], `${lingua}: manca devices.link.queue.uno`);
    assert.match(testi["devices.link.queue"] ?? "", /\{n\}/, `${lingua}: il plurale dice il numero`);
  }
});

// ── il nome al collegamento ─────────────────────────────────────────────────

test("il dispositivo appena collegato è il più recente con quel nome, dentro la finestra", () => {
  const adesso = 10_000_000;
  const vecchio = dispositivo("STUDIO", { associatoMs: adesso - FINESTRA_COLLEGATO_MS - 1 });
  const nuovo = dispositivo("STUDIO-2", { chiave: "nuovo", nome: "STUDIO", associatoMs: adesso - 2_000 });
  const altro = dispositivo("SALOTTO", { associatoMs: adesso - 1_000 });

  // Per nome: quello che chi mostrava il QR ha sentito dichiarare.
  assert.equal(appenaCollegato([vecchio, altro, nuovo], "STUDIO", adesso)?.chiave, "nuovo");
  // Senza nome (il link non lo porta): l'ultimo collegato.
  assert.equal(appenaCollegato([vecchio, nuovo, altro], null, adesso)?.chiave, "salotto");
  // Fuori dalla finestra non è «appena»: un dispositivo di settimane fa non si
  // ripropone a ogni collegamento.
  assert.equal(appenaCollegato([vecchio], "STUDIO", adesso), null);
  assert.equal(appenaCollegato([vecchio], null, adesso), null);
  // Un nome che non c'è, e un elenco vuoto.
  assert.equal(appenaCollegato([nuovo], "ALTRO", adesso), null);
  assert.equal(appenaCollegato([], null, adesso), null);
  // Uno revocato dall'altra parte non è un collegamento riuscito.
  assert.equal(
    appenaCollegato([dispositivo("STUDIO", { revocato: true, associatoMs: adesso - 100 })], "STUDIO", adesso),
    null,
  );
  // Un orologio che va indietro non rende «appena» un collegamento del futuro.
  assert.equal(
    appenaCollegato([dispositivo("STUDIO", { associatoMs: adesso + 3_600_000 })], "STUDIO", adesso),
    null,
  );
});

test("il nome si propone per i computer, che hanno quello del sistema; i telefoni hanno già il loro", () => {
  assert.equal(proponiNome(dispositivo("DESKTOP-7K2L9QF")), true);
  assert.equal(proponiNome(dispositivo("Nothing Phone 2a", { piattaforma: "android" })), false);
  assert.equal(proponiNome(dispositivo("STUDIO", { revocato: true })), false);
});

test("al collegamento si propone di rinominare, da tutte e due le parti, col comando che c'è già", () => {
  // Da chi incolla il link: la risposta di «Collega» porta l'elenco aggiornato.
  const collega = DISPOSITIVI.slice(DISPOSITIVI.indexOf("const collega = (daCollegare: LinkLontano)"));
  const finoAlLink = collega.slice(0, collega.indexOf("/** «Collega»: col collegamento spento"));
  assert.match(finoAlLink, /appenaCollegato\(/);
  assert.match(finoAlLink, /proponiNome\(/);
  // Da chi mostrava il QR: l'evento dice il nome, e il dispositivo arriva
  // con l'elenco.
  assert.match(DISPOSITIVI, /useAscolto<string>\("lontano:collegato"/);
  assert.match(DISPOSITIVI, /setAttesaNome\(/);
  // La finestrella è la domanda con una risposta sola che c'è già (`Chiedi`),
  // col nome com'è e la frase che dice perché; il comando è quello della riga
  // dell'elenco. Non sta davanti a un sì: aspetta che le domande di consenso
  // siano chiuse.
  const domanda = DISPOSITIVI.slice(DISPOSITIVI.indexOf("<Chiedi"), DISPOSITIVI.indexOf("<Chiedi") + 1_200);
  assert.match(domanda, /titolo=\{t\("devices\.link\.rename\.title"\)\}/);
  assert.match(domanda, /iniziale=\{daNominare\.nome\}/);
  assert.match(domanda, /ipc\.lontanoRinomina\(daNominare\.chiave, nome\)/);
  assert.match(DISPOSITIVI, /daNominare !== null && richiesta === null && link === null && riprova === null/);
  for (const chiave of [
    "devices.link.rename.title",
    "devices.link.rename.sub",
    "devices.link.rename.field",
  ]) {
    assert.ok(DISPOSITIVI.includes(`"${chiave}"`), `${chiave} non è usata`);
    assert.ok(IT[chiave], `it: manca ${chiave}`);
    assert.ok(EN[chiave], `en: manca ${chiave}`);
  }
  // Il nome com'è si dice come dichiarato dall'altro: non è fidato.
  assert.match(IT["devices.link.rename.sub"] ?? "", /\{nome\}/);
  assert.match(EN["devices.link.rename.sub"] ?? "", /\{nome\}/);
});

// ── i testi che parlavano solo del telefono ─────────────────────────────────

test("i testi del collegamento dicono anche «copia il link e incollalo»", () => {
  for (const chiave of ["settings.devices.qr.sub", "settings.devices.none", "more.devices.link.sub"]) {
    assert.match(IT[chiave] ?? "", /link/i, `it: ${chiave} non parla del link`);
    assert.match(EN[chiave] ?? "", /link/i, `en: ${chiave} non parla del link`);
  }
  // Sul PC: dove si prende il link e dove si incolla, coi nomi dei tasti che si
  // vedono (`settings.devices.qr.copy`, `settings.devices.paste`).
  assert.ok((IT["settings.devices.qr.sub"] ?? "").includes(`«${IT["settings.devices.qr.copy"]}»`));
  assert.ok((IT["settings.devices.qr.sub"] ?? "").includes(`«${IT["settings.devices.paste"]}»`));
  assert.ok((EN["settings.devices.qr.sub"] ?? "").includes(`“${EN["settings.devices.qr.copy"]}”`));
  assert.ok((EN["settings.devices.qr.sub"] ?? "").includes(`“${EN["settings.devices.paste"]}”`));
  // L'elenco vuoto non manda solo al QR «qui sopra».
  assert.match(IT["settings.devices.none"] ?? "", /incolla/i);
  assert.match(EN["settings.devices.none"] ?? "", /paste/i);
});

test("il campo «Incolla il link» ha un titoletto e una riga di spiegazione, e il campo li legge", () => {
  const scheda = DISPOSITIVI.slice(DISPOSITIVI.indexOf("export function SchedaDispositivi"));
  const form = scheda.slice(scheda.indexOf("devices.link.paste.title") - 200, scheda.indexOf("settings.devices.paste.cta"));
  assert.match(form, /<h3 className="titoletto">\{t\("devices\.link\.paste\.title"\)\}<\/h3>/);
  assert.match(form, /t\("devices\.link\.paste\.sub"\)/);
  assert.match(form, /aria-describedby=\{idSpiegazione\}/);
  for (const [lingua, testi] of [["it", IT], ["en", EN]] as const) {
    assert.ok(testi["devices.link.paste.title"], `${lingua}: manca il titoletto`);
    assert.ok(testi["devices.link.paste.sub"], `${lingua}: manca la spiegazione`);
  }
  // Il nome del campo è quello che i testi citano: «Incolla il link» / “Paste link”.
  assert.equal(IT["settings.devices.paste"], "Incolla il link");
  assert.equal(EN["settings.devices.paste"], "Paste link");
});

test("«un link nuovo annulla il precedente» sta scritto dove si mostra il QR", () => {
  const scheda = DISPOSITIVI.slice(DISPOSITIVI.indexOf("export function SchedaDispositivi"));
  const pannello = scheda.slice(scheda.indexOf('<div className="collega-qr">'), scheda.indexOf("settings.devices.paste") );
  assert.match(pannello, /t\("devices\.link\.replaces"\)/);
  assert.match(IT["devices.link.replaces"] ?? "", /nuovo.*annulla/i);
  assert.match(EN["devices.link.replaces"] ?? "", /new.*(replaces|cancels)/i);
});

// ── il documento ────────────────────────────────────────────────────────────

test("il documento dice dove viaggia la chiave di «Ascolta insieme»: nella stretta di mano, non nel link", () => {
  const doc = leggi("../../../site/docs/contenuto/devices.js");
  assert.doesNotMatch(doc, /X25519 key inside the link/);
  assert.match(doc, /X25519 key in the handshake/);
  assert.match(doc, /Presentazione\.chiave_insieme/);
});
