/**
 * Il sottofondo del telefono, due difetti del collaudo del 18/11 sull'emulatore
 * (FC1): la notifica che non compariva se il permesso arrivava nello stesso
 * giro (D2), e la nota della batteria che non si aggiornava (D8).
 *
 * Il Kotlin non ha prove unitarie: qui si guarda nei sorgenti che il cablaggio
 * c'è, e la procedura dal vivo sta nel report della fase.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const leggi = (percorso: string) =>
  readFileSync(new URL(percorso, import.meta.url), "utf8").replace(/\r\n/g, "\n");
const KOTLIN = "../src-tauri/gen/android/app/src/main/java/io/github/federicobaratti/rymembra";
const PRESENZA = leggi(`${KOTLIN}/Presenza.kt`);
const PERMESSI = leggi(`${KOTLIN}/Permessi.kt`);
const ATTIVITA = leggi(`${KOTLIN}/MainActivity.kt`);
/** Il sorgente senza commenti: i commenti nominano quel che il codice non fa. */
const senzaCommenti = (testo: string) => testo.replace(/\/\*[\s\S]*?\*\/|\/\/.*$/gm, "");

/** Il corpo di una funzione Kotlin, fino alla graffa che la chiude. */
function corpo(testo: string, firma: string): string {
  const inizio = testo.indexOf(firma);
  assert.ok(inizio >= 0, `manca ${firma}`);
  const graffa = testo.indexOf("{", inizio);
  let livello = 0;
  for (let i = graffa; i < testo.length; i += 1) {
    if (testo[i] === "{") livello += 1;
    if (testo[i] === "}") {
      livello -= 1;
      if (livello === 0) return testo.slice(inizio, i + 1);
    }
  }
  assert.fail(`${firma} non si chiude`);
}

test("D2: la notifica del servizio si rifà quando il permesso delle notifiche arriva", () => {
  // Il servizio rifà la notifica senza il confronto con la frase di prima — è
  // quel confronto che la teneva nascosta — ma solo se è vivo e le notifiche
  // adesso sono permesse.
  const ripubblica = senzaCommenti(corpo(PRESENZA, "fun ripubblica("));
  assert.match(ripubblica, /if \(vivo == null\) return/);
  assert.match(ripubblica, /areNotificationsEnabled\(\)/);
  assert.match(ripubblica, /notify\(ID, notifica\(/);
  assert.doesNotMatch(ripubblica, /mostrata/, "non guarda la frase di prima: la riposta e basta");
  // La domanda che non aspetta la risposta ha un codice, e la risposta lo porta.
  assert.match(senzaCommenti(PERMESSI), /\bconst val CODICE_NOTIFICHE = 4_213/);
  assert.doesNotMatch(senzaCommenti(PERMESSI), /private const val CODICE_NOTIFICHE/);
  const risposta = senzaCommenti(corpo(ATTIVITA, "override fun onRequestPermissionsResult("));
  assert.match(risposta, /super\.onRequestPermissionsResult\(requestCode, permissions, grantResults\)/);
  assert.match(risposta, /requestCode == Permessi\.CODICE_NOTIFICHE/);
  assert.match(risposta, /Presenza\.ripubblica\(this\)/);
});

test("D2: anche il dialogo che aspetta (il consenso) rifà la notifica se il sì riguarda le notifiche", () => {
  const lanciatore = senzaCommenti(
    ATTIVITA.slice(
      ATTIVITA.indexOf("private val lanciatorePermessi"),
      ATTIVITA.indexOf("private var mittenteInAttesa"),
    ),
  );
  assert.match(lanciatore, /risultato\[Manifest\.permission\.POST_NOTIFICATIONS\] == true/);
  assert.match(lanciatore, /Presenza\.ripubblica\(this\)/);
  assert.match(ATTIVITA, /^import android\.Manifest$/m);
});

test("D8: la nota della batteria si rilegge quando l'app torna in vista o riprende il fuoco", () => {
  // Era già così dalla F6: l'esclusione dal risparmio batteria si cambia nella
  // pagina di sistema, e da lì si torna senza un risultato da aspettare. Il
  // collaudo la cambiava con `dumpsys deviceidle` restando nell'app, che non
  // genera nessun evento: la procedura dal vivo usa il pulsante.
  const scheda = senzaCommenti(leggi("../src/parti/SottofondoTelefono.tsx"));
  assert.match(scheda, /document\.addEventListener\("visibilitychange", torna\)/);
  assert.match(scheda, /window\.addEventListener\("focus", torna\)/);
  assert.match(scheda, /document\.visibilityState === "visible"/);
  assert.match(scheda, /document\.removeEventListener\("visibilitychange", torna\)/);
  assert.match(scheda, /window\.removeEventListener\("focus", torna\)/);
});
