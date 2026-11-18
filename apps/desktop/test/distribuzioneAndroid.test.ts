/**
 * Le due distribuzioni di Android: `github`, con l'aggiornatore di sempre, e
 * `play`, senza aggiornatore e senza il permesso `REQUEST_INSTALL_PACKAGES`.
 *
 * Il Kotlin non ha test unitari e Gradle non gira qui: si guardano i sorgenti
 * dove un guasto costerebbe una build rotta o una policy violata. La prova vera —
 * il manifesto fuso e le costanti di build delle due distribuzioni — la fa
 * Gradle (`processArm64DebugMainManifest`, vedi la procedura nel report).
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";

const base = "../src-tauri/gen/android/app/";
const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");
const kotlin = (nome: string) => leggi(`${base}src/main/java/io/github/federicobaratti/rymembra/${nome}`);
const GRADLE = leggi(`${base}build.gradle.kts`);
/** Il Gradle senza commenti: le ragioni citano parole che il codice non deve usare. */
const CODICE = GRADLE.split("\n")
  .filter((riga) => !riga.trim().startsWith("//") && !riga.trim().startsWith("*") && !riga.trim().startsWith("/*"))
  .join("\n");

test("la distribuzione è una proprietà Gradle, di serie github, e non una flavor", () => {
  assert.match(CODICE, /findProperty\("distribuzione"\) as\? String\) \?: "github"/);
  assert.match(CODICE, /require\(distribuzione == "github" \|\| distribuzione == "play"\)/);
  // Una seconda dimensione di flavor rompe il plugin `rust` della CLI di Tauri
  // già alla configurazione («Task with name 'mergeUniversalDebugJniLibFolders'
  // not found»), e cambierebbe i nomi dei compiti che la CI e la CLI chiamano.
  assert.doesNotMatch(CODICE, /flavorDimensions/);
  assert.doesNotMatch(CODICE, /productFlavors/);
  // E la ragione, e il modo di usarla con la CLI di Tauri, stanno scritti.
  assert.match(GRADLE, /ORG_GRADLE_PROJECT_distribuzione/);
  assert.match(GRADLE, /mergeUniversalDebugJniLibFolders/);
});

test("le costanti di build dicono la distribuzione e se c'è l'aggiornatore", () => {
  assert.match(CODICE, /buildConfigField\("String", "DISTRIBUZIONE", "\\"\$distribuzione\\""\)/);
  assert.match(CODICE, /buildConfigField\("boolean", "AGGIORNATORE_APK", \(!distribuzionePlay\)\.toString\(\)\)/);
  assert.match(CODICE, /val distribuzionePlay: Boolean = distribuzione == "play"/);
});

test("il manifesto per il Play toglie REQUEST_INSTALL_PACKAGES, da una cartella che Gradle non conosce", () => {
  // Il manifesto di `main` lo dichiara ancora: è la distribuzione di serie.
  assert.match(leggi(`${base}src/main/AndroidManifest.xml`), /android\.permission\.REQUEST_INSTALL_PACKAGES/);
  const percorso = `${base}src/distribuzione-play/AndroidManifest.xml`;
  assert.ok(existsSync(new URL(percorso, import.meta.url)), "manca il manifesto della distribuzione play");
  const sovrapposto = leggi(percorso);
  assert.match(
    sovrapposto,
    /<uses-permission\s+android:name="android\.permission\.REQUEST_INSTALL_PACKAGES"\s+tools:node="remove"\s*\/>/,
  );
  // Si attacca ai tipi di build solo con la proprietà; mai a una cartella che
  // Android Gradle prenderebbe da sola (`src/debug`, `src/release`, o per nome
  // di distribuzione), che toglierebbe il permesso anche a `github`.
  const attacco = CODICE.slice(CODICE.indexOf("if (distribuzionePlay) {"));
  assert.match(attacco, /sourceSets\.getByName\("debug"\)\.manifest\.srcFile\("src\/distribuzione-play\/AndroidManifest\.xml"\)/);
  assert.match(attacco, /sourceSets\.getByName\("release"\)\.manifest\.srcFile\("src\/distribuzione-play\/AndroidManifest\.xml"\)/);
  for (const automatica of ["debug", "release", "play", "github"]) {
    assert.ok(
      !existsSync(new URL(`${base}src/${automatica}/AndroidManifest.xml`, import.meta.url)),
      `src/${automatica}/AndroidManifest.xml è una cartella che Gradle prende da sola`,
    );
  }
});

test("sul Play «Parti con l'auto» è spenta: la costante di F7 va a false, e solo lì", () => {
  const play = CODICE.slice(CODICE.indexOf("if (distribuzionePlay) {"));
  assert.match(play, /buildConfigField\("boolean", "PARTENZA_CON_AUTO", "false"\)/);
  // E il motivo: la regola di qualità di Google per le app delle auto.
  assert.match(GRADLE, /MA-1/);
});

test("Kotlin dice la distribuzione a Rust, e l'installatore non parte sul Play", () => {
  assert.match(kotlin("Ponte.kt"), /"sistema\.distribuzione" -> BuildConfig\.DISTRIBUZIONE/);
  const installatore = kotlin("Installatore.kt");
  assert.match(installatore, /check\(BuildConfig\.AGGIORNATORE_APK\)/);
  for (const funzione of ["fun controlla(", "fun installa("]) {
    const da = installatore.indexOf(funzione);
    assert.ok(da >= 0, `${funzione} non c'è più`);
    const corpo = installatore.slice(da, da + 400);
    assert.match(corpo, /soloFuoriDalPlay\(\)/, `${funzione} non controlla la distribuzione`);
  }
});

test("Rust spegne l'aggiornatore del telefono sul Play: il controllo, il comando e lo stato", () => {
  const sistema = leggi("../rymembra-android/src/sistema.rs");
  assert.ok(sistema.includes('chiama("sistema.distribuzione", &())'));
  const mobile = leggi("../src-tauri/src/mobile/aggiornamenti.rs");
  assert.match(mobile, /fn senza_aggiornatore\(\) -> bool/);
  // La passata non fa richieste.
  const passata = mobile.slice(mobile.indexOf("fn passata("), mobile.indexOf("fn passata(") + 400);
  assert.match(passata, /if senza_aggiornatore\(\) \{\s*return Giro::Saltato;/);
  // «Aggiorna» e «Installa» rifiutano.
  const installa = mobile.slice(mobile.indexOf("pub fn aggiornamenti_installa("));
  assert.match(installa.slice(0, 1200), /senza_aggiornatore\(\)/);
  // E la finestra lo sa: non configurato, e «ci pensa il negozio».
  assert.match(mobile, /configurato: !senza_aggiornatore\(\)/);
  assert.match(mobile, /gestito_dal_store: senza_aggiornatore\(\)/);
  // Prudenza: se il ponte non risponde si sta senza aggiornatore, non con.
  const funzione = mobile.slice(mobile.indexOf("fn senza_aggiornatore("));
  assert.match(funzione.slice(0, 1200), /true\s*\}\s*\}/);
});

test("la finestra dice «Google Play» sul telefono quando il Play la aggiorna", () => {
  const it = JSON.parse(leggi("../src/lingue/it.json")) as Record<string, string>;
  const en = JSON.parse(leggi("../src/lingue/en.json")) as Record<string, string>;
  assert.match(it["settings.update.play"] ?? "", /Google Play/);
  assert.match(en["settings.update.play"] ?? "", /Google Play/);
  assert.match(leggi("../src/parti/Aggiornamenti.tsx"), /MOBILE \? "settings\.update\.play" : "settings\.update\.store"/);
});
