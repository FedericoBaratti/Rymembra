/**
 * Prestazioni di «Sfoglia» e del dispositivo aperto (R13-11).
 *
 * - `App` chiama `useDispositivo(dispositivoAperto)`, e `useDispositivo` seguiva
 *   l'archivio **intero**: anche con `dispositivoAperto === null`, ogni
 *   `lontano:stato` (una volta al minuto per dispositivo, a ogni cambio di via o
 *   di presenza) ridisegnava `App` con testata e corpo, fuori da «Sfoglia» — proprio
 *   quel che `archivio.ts` esiste per evitare. Adesso segue il solo dispositivo
 *   che le serve, per chiave, e con nessuna chiave non segue niente.
 * - In «Brani» dopo dieci «Mostra altri» le righe sono duemila, e ogni pagina
 *   nuova le ridisegnava tutte: `ElencoBrani` passava `elenco={brani}` (un array
 *   nuovo a ogni pagina) a righe senza `memo`, sotto un contesto ricreato a ogni
 *   disegno.
 *
 * Non fatta: la virtualizzazione delle liste (come `virtuale.ts` in libreria). Le
 * pagine restano a 200 voci con «Mostra altri»; qui si toglie il costo di
 * ridisegnare quel che non è cambiato.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import type { DispositivoLontano, StatoLontano } from "../src/ipc";
import * as lontano from "../src/lontano";

const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

function dispositivo(chiave: string, campi: Partial<DispositivoLontano> = {}): DispositivoLontano {
  return {
    chiave,
    nome: chiave.toUpperCase(),
    piattaforma: "windows",
    dispositivo: chiave,
    online: true,
    vistoMs: 1_000,
    associatoMs: 0,
    revocato: false,
    versione: null,
    insiemeAcceso: false,
    insiemeOspita: false,
    insieme: null,
    via: "diretto",
    ...campi,
  };
}

function pannello(...dispositivi: DispositivoLontano[]): StatoLontano {
  return {
    attivo: true,
    acceso: true,
    chiave: null,
    impronta: null,
    relay: null,
    datiMobili: "chiedi",
    errore: null,
    dispositivi,
    invito: null,
    richieste: [],
  };
}

test("senza dispositivo aperto la scelta è sempre `null`, qualunque sia il pannello (R13-11)", () => {
  assert.equal(typeof lontano.sceltaDelDispositivo, "function", "manca sceltaDelDispositivo");
  const scegli = lontano.sceltaDelDispositivo(null);
  assert.equal(scegli(null), null);
  assert.equal(scegli(pannello(dispositivo("pc"))), null);
  assert.equal(scegli(pannello(dispositivo("pc"), dispositivo("tel"))), null);
});

test("la scelta tiene la stessa identità finché il dispositivo non cambia (R13-11)", () => {
  const scegli = lontano.sceltaDelDispositivo("pc");
  const primo = scegli(pannello(dispositivo("pc"), dispositivo("tel")));
  assert.equal(primo?.chiave, "pc");

  // Un altro dispositivo cambia presenza: quello aperto è lo stesso, e chi lo
  // legge non si ridisegna.
  const dopoAltro = scegli(pannello(dispositivo("pc"), dispositivo("tel", { online: false })));
  assert.equal(dopoAltro, primo, "un altro dispositivo che cambia non ridisegna questa pagina");

  // Il pannello nuovo con un oggetto nuovo ma uguale in superficie: stessa identità.
  const rifatto = scegli(pannello(dispositivo("pc"), dispositivo("tel")));
  assert.equal(rifatto, primo);

  // Il suo stato cambia davvero: una scelta nuova.
  const offline = scegli(pannello(dispositivo("pc", { online: false }), dispositivo("tel")));
  assert.notEqual(offline, primo);
  assert.equal(offline?.online, false);
});

test("un dispositivo che sparisce dal pannello è `null`, e il pannello assente pure", () => {
  const scegli = lontano.sceltaDelDispositivo("pc");
  assert.equal(scegli(null), null);
  assert.equal(scegli(pannello(dispositivo("tel"))), null);
});

test("`useDispositivo` segue il solo dispositivo scelto, non l'archivio intero (cablaggio)", () => {
  const pagina = sorgente("schermate/Dispositivo.tsx");
  const da = pagina.indexOf("export function useDispositivo(");
  assert.ok(da >= 0, "manca useDispositivo");
  const corpo = pagina.slice(da, pagina.indexOf("\n}\n", da));
  assert.match(corpo, /sceltaDelDispositivo\(chiave\)/);
  assert.doesNotMatch(corpo, /useArchivio\(archivioLontano\)/, "l'archivio intero ridisegnava App a ogni evento");
});

test("le righe di Sfoglia non si ridisegnano a ogni pagina nuova (cablaggio)", () => {
  const pagina = sorgente("schermate/Dispositivo.tsx");
  assert.match(pagina, /const RigaBrano = memo\(function RigaBrano\(/, "RigaBrano sotto memo");
  const elenco = pagina.slice(pagina.indexOf("function ElencoBrani("), pagina.indexOf("/** Un brano in un ripiano"));
  assert.doesNotMatch(elenco, /elenco=\{brani\}/, "l'array di una pagina nuova non scende alle righe: sarebbe una prop nuova per tutte");
  assert.match(elenco, /useRef\(brani\)/, "l'elenco intero scende come riferimento, che non cambia identità");
  // Il contesto: senza un valore stabile, `memo` non serve a niente.
  assert.match(
    pagina,
    /const sfoglia = useMemo<Sfoglia>\(/,
    "il valore del contesto di Sfoglia si rifà solo quando cambia qualcosa",
  );
});
