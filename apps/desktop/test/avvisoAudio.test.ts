import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * Il motore audio mai aperto (R10-2).
 *
 * Quando l'uscita non si apre all'avvio il nucleo manda `codice:
 * "playback.engineUnavailable"`, la causa tecnica **italiana** e
 * `causaCodice: null`. La finestra stampava la causa: «There is no audio.
 * l'apertura del dispositivo non è finita entro 20 secondi.», e la frase che
 * dice cosa controllare (`errors.playback.engineUnavailable`, con la sua
 * variante `.telefono`, che `tSe` sceglie da sé) non compariva mai. La causa
 * resta nel diario, dove il nucleo la scrive già.
 */

const sorgente = readFileSync(new URL("../src/parti/AvvisoAudio.tsx", import.meta.url), "utf8");

test("il motore mai aperto dice la frase del catalogo, non la causa del motore", () => {
  assert.ok(
    !/causaCodice === null\s*\?\s*guasto\.causa\b/.test(sorgente),
    "senza codice della causa si stampa ancora la frase italiana del motore",
  );
  assert.match(
    sorgente,
    /causaCodice === null\s*\?\s*tSe\(`errors\.\$\{guasto\.codice\}`, guasto\.causa\)/,
  );
  // E una volta sola il punto: la frase del catalogo lo ha, la fascia lo
  // aggiunge.
  assert.match(
    sorgente,
    /tSe\(`errors\.\$\{guasto\.codice\}`, guasto\.causa\)\.replace\(\/\\\.\+\$\/, ""\)/,
  );
});

test("la frase del catalogo c'è nelle due lingue, anche per il telefono", () => {
  for (const lingua of ["it", "en"]) {
    const testi = JSON.parse(
      readFileSync(new URL(`../src/lingue/${lingua}.json`, import.meta.url), "utf8"),
    ) as Record<string, string>;
    assert.ok(testi["errors.playback.engineUnavailable"], lingua);
    assert.ok(testi["errors.playback.engineUnavailable.telefono"], lingua);
  }
});
