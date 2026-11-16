/**
 * Le risposte dei comandi del collegamento e gli eventi `lontano:stato` (R13-10).
 *
 * Un comando (accendi, rinomina, dimentica, relay, dati mobili…) risponde col
 * pannello **com'era quando ha risposto**. Se mentre la risposta era in volo il
 * filo ha pubblicato un cambio — un dispositivo torna online, cambia la via — e
 * il suo evento è arrivato per primo, la risposta lo ricopriva col passato fino
 * al cambio dopo (che può essere a un minuto: `SCRIVI_VISTO_OGNI`).
 *
 * L'evento è sempre più fresco della risposta chiesta prima di lui, o uguale: il
 * nucleo lo pubblica solo quando il pannello cambia (`lontano/filo.rs`,
 * `pubblica`: `self.pubblicata != json`), e ogni cambio fatto da un comando è
 * seguito dal suo evento. È la regola di `parti/Insieme.tsx` (`eventiArrivati`),
 * in una forma che si prova da sola (`ultima.ts`).
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import type { DispositivoLontano, StatoLontano } from "../src/ipc";
import * as lontano from "../src/lontano";

const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

function dispositivo(online: boolean): DispositivoLontano {
  return {
    chiave: "pc",
    nome: "PC",
    piattaforma: "windows",
    dispositivo: "PC",
    online,
    vistoMs: null,
    associatoMs: 0,
    revocato: false,
    versione: null,
    insiemeAcceso: false,
    insiemeOspita: false,
    insieme: null,
    via: null,
  };
}

function pannello(online: boolean): StatoLontano {
  return {
    attivo: true,
    acceso: true,
    chiave: null,
    impronta: null,
    relay: null,
    datiMobili: "chiedi",
    errore: null,
    dispositivi: [dispositivo(online)],
    invito: null,
    richieste: [],
  };
}

/** Una promessa che si risolve quando lo dice il test. */
function inSospeso<T>(): { promessa: Promise<T>; risolvi: (valore: T) => void } {
  let risolvi: (valore: T) => void = () => undefined;
  const promessa = new Promise<T>((ok) => {
    risolvi = ok;
  });
  return { promessa, risolvi };
}

test("una risposta partita prima di un evento arrivato non lo ricopre (R13-10)", async () => {
  assert.equal(typeof lontano.comandoDelPannello, "function", "manca comandoDelPannello");
  assert.equal(typeof lontano.eventoDelPannello, "function", "manca eventoDelPannello");

  const iniziale = pannello(false);
  lontano.eventoDelPannello(iniziale);
  assert.equal(lontano.archivioLontano.leggi(), iniziale);

  // «Rinomina»: la risposta è il pannello letto all'istante T, col dispositivo
  // ancora offline…
  const risposta = inSospeso<StatoLontano>();
  const finito = lontano.comandoDelPannello(() => risposta.promessa);

  // …ma fra T e l'arrivo della risposta il filo pubblica il ritorno online, e il
  // suo evento arriva prima.
  const piuNuovo = pannello(true);
  lontano.eventoDelPannello(piuNuovo);
  risposta.risolvi(pannello(false));
  await finito;

  assert.equal(
    lontano.archivioLontano.leggi(),
    piuNuovo,
    "la risposta chiesta prima dell'evento non rimette a schermo il passato",
  );
});

test("senza eventi in mezzo la risposta si adotta, e chi chiede la riceve comunque", async () => {
  lontano.eventoDelPannello(pannello(false));
  const nuovo = pannello(true);
  const ricevuto = await lontano.comandoDelPannello(() => Promise.resolve(nuovo));
  assert.equal(ricevuto, nuovo, "il comando restituisce sempre la risposta: chi la usa la trova");
  assert.equal(lontano.archivioLontano.leggi(), nuovo);
});

test("un evento arrivato prima del comando non lo blocca", async () => {
  lontano.eventoDelPannello(pannello(false));
  const nuovo = pannello(true);
  await lontano.comandoDelPannello(() => Promise.resolve(nuovo));
  assert.equal(lontano.archivioLontano.leggi(), nuovo);
});

test("anche chi la riceve la trova se la risposta è stata scartata", async () => {
  lontano.eventoDelPannello(pannello(false));
  const risposta = inSospeso<StatoLontano>();
  const finito = lontano.comandoDelPannello(() => risposta.promessa);
  lontano.eventoDelPannello(pannello(true));
  const vecchia = pannello(false);
  risposta.risolvi(vecchia);
  assert.equal(await finito, vecchia, "scartata dall'archivio, ma restituita a chi aveva chiesto");
});

test("«accendi poi collega» non ricopre un evento arrivato mentre si accendeva", async () => {
  lontano.eventoDelPannello(pannello(false));
  const attivazione = inSospeso<StatoLontano>();
  let collegato = false;
  const fine = lontano.accendiPoiCollega({
    attiva: () => attivazione.promessa,
    collega: () => {
      collegato = true;
    },
    annullato: () => false,
  });
  const piuNuovo = pannello(true);
  lontano.eventoDelPannello(piuNuovo);
  attivazione.risolvi(pannello(false));
  assert.equal(await fine, true);
  assert.equal(collegato, true, "il collegamento parte lo stesso: è il pannello che non torna indietro");
  assert.equal(lontano.archivioLontano.leggi(), piuNuovo);
});

test("ogni scrittura dell'archivio passa dal contatore degli eventi (cablaggio)", () => {
  const dispositivi = sorgente("parti/Dispositivi.tsx");
  assert.doesNotMatch(
    dispositivi,
    /archivioLontano\.scrivi\(/,
    "i comandi e l'ascolto scrivono con comandoDelPannello / eventoDelPannello, mai direttamente",
  );
  assert.match(dispositivi, /eventoDelPannello/, "l'evento `lontano:stato` conta");

  const modulo = sorgente("lontano.ts");
  // Le sole scritture dirette sono dentro i due aiutanti.
  const dirette = [...modulo.matchAll(/archivioLontano\.scrivi\(/g)].length;
  assert.equal(dirette, 2, "una nell'evento, una nella risposta: nessun'altra strada");
});
