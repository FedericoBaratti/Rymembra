import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * File «solo online» di OneDrive, Dropbox, Google Drive per desktop (R25-2,
 * D-101): la scansione e l'analisi non li aprono — aprirli li farebbe
 * scaricare — e lo dicono. Il nucleo lo prova in `files.rs` e in
 * `library/tests.rs`; qui il cablaggio: chi legge per conto suo passa dalle
 * sorgenti che non scaricano, e l'esito della scansione lo dice.
 */

const sorgente = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");

test("la scansione e l'analisi leggono dalle sorgenti che non scaricano", () => {
  for (const file of ["comandi.rs", "sorveglia/filo.rs", "procura.rs"]) {
    const testo = sorgente(`../src-tauri/src/${file}`);
    assert.match(testo, /files: crate::nuvola_musica::archivi_per_la_scansione\(app\)/, file);
    assert.doesNotMatch(testo, /files: crate::nuvola_musica::archivi\(app\)/, file);
  }
  const nuvola = sorgente("../src-tauri/src/nuvola_musica.rs");
  const analisi = nuvola.split("pub fn archivi_per_l_analisi(")[1]?.split("\n}\n")[0] ?? "";
  assert.match(analisi, /SenzaScaricare\(/, "l'analisi");
  // Chi suona invece scarica: `archivi` resta senza.
  const suona = nuvola.split("pub fn archivi(app: &AppHandle)")[1]?.split("\n}\n")[0] ?? "";
  assert.doesNotMatch(suona, /SenzaScaricare/);
});

test("l'esito della scansione dice quanti file solo online non ha scaricato", () => {
  assert.match(sorgente("../src-tauri/src/comandi.rs"), /solo_in_rete: report\.solo_in_rete,/);
  assert.match(sorgente("../src/ipc/libreria.ts"), /soloInRete: number;/);
  assert.match(
    sorgente("../src/App.tsx"),
    /esito\.soloInRete > 0[\s\S]{0,120}settings\.scan\.onlineOnly\.strong/,
  );
  for (const lingua of ["it", "en"]) {
    const testi = JSON.parse(sorgente(`../src/lingue/${lingua}.json`));
    assert.match(testi["settings.scan.onlineOnly.strong"] ?? "", /\{n\}/, lingua);
    assert.ok(testi["settings.scan.onlineOnly.strong.uno"], lingua);
    assert.ok(testi["errors.cloud.onlineOnly"], lingua);
  }
});
