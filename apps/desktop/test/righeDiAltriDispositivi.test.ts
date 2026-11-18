/**
 * Le righe di un altro dispositivo negli elenchi della libreria: l'icona, il
 * grigio, e il costo di tutto questo per chi non ha dispositivi collegati.
 *
 * Prima ci pensava la pagina «Sfoglia», con i suoi elenchi; adesso sono le righe
 * della libreria (`schermate/Elenco.tsx`), e queste prove guardano il
 * cablaggio: l'icona con il nome, `aria-disabled` sulla riga di un dispositivo
 * che non risponde, e che una riga di qui non iscriva niente al pannello.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

test("la riga dice da quale dispositivo viene, con il nome per chi non vede l'icona", () => {
  const elenco = sorgente("schermate/Elenco.tsx");
  assert.match(elenco, /const lontana = useStatoDellaRiga\(brano\.path\);/);
  const da = elenco.indexOf('className="dispositivo-riga"');
  assert.ok(da >= 0, "manca l'icona del dispositivo");
  const icona = elenco.slice(da - 80, elenco.indexOf("</span>", da));
  assert.match(icona, /role="img"/);
  assert.match(icona, /aria-label=\{etichettaDellaRiga\(lontana\)\}/);
  assert.match(icona, /i-telefono/);
  assert.match(icona, /i-computer/);
});

test("una riga di un dispositivo che non risponde è `aria-disabled`, e tiene il fuoco e il menù", () => {
  const elenco = sorgente("schermate/Elenco.tsx");
  assert.match(elenco, /aria-disabled=\{lontana\?\.spenta === true \? true : undefined\}/);
  assert.match(elenco, /data-spenta=\{lontana\?\.spenta === true \? true : undefined\}/);
  // Non `disabled` sui comandi: il cuore e le stelle partono lo stesso (restano nella posta), e il menù si apre.
  const riga = elenco.slice(elenco.indexOf('className="riga list-row"'), elenco.indexOf('className="indice track-index"'));
  assert.doesNotMatch(riga, /tabIndex=\{-1\}\s*disabled=\{lontana/);
});

test("una riga di qui non iscrive niente al pannello (R13-11)", () => {
  const hook = sorgente("dispositivoLontano.ts");
  const da = hook.indexOf("export function useStatoDellaRiga(");
  assert.ok(da >= 0, "manca useStatoDellaRiga");
  const corpo = hook.slice(da, hook.indexOf("\n}\n", da));
  assert.match(corpo, /chiave === null \? null : statoDellaRiga\(percorso, stato\)/, "senza dispositivo la scelta è `null` senza leggere");
  assert.match(corpo, /ugualiInSuperficie/, "un pannello nuovo con gli stessi campi non ridisegna la riga");
  assert.doesNotMatch(corpo, /useArchivio\(archivioLontano\)/, "l'archivio intero ridisegnava ogni riga a ogni evento");
});

test("un tocco su una riga grigia dice chi non risponde, e non è un errore", () => {
  const app = sorgente("App.tsx");
  // La notizia, non la fascia rossa.
  const da = app.indexOf("const avvisaNonRaggiungibile = useCallback(");
  assert.ok(da >= 0, "manca avvisaNonRaggiungibile");
  const corpo = app.slice(da, app.indexOf("[setNotizia],", da));
  assert.match(corpo, /setNotizia\(/);
  assert.doesNotMatch(corpo, /segnalaErrore/);
  // Riproduci e il tocco passano dallo stesso posto.
  assert.match(app, /statoDellaRiga\(toccata\.path, pannello\)/);
  assert.match(app, /suonabili\(elenco, indice, pannello\)/);
  assert.match(app, /statoDellaRiga\(premuto\?\.path, archivioLontano\.leggi\(\)\)/, "anche nelle liste rade");
});
