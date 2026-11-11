/**
 * Il manifesto e le risorse dell'APK: le righe che non danno errori quando
 * mancano, e si scoprono soltanto sul telefono.
 *
 * - **Chi entra nella sessione** (R03-1, D-33, D-34). Un orologio o un
 *   telecomando con l'accesso alle notifiche entra se Media3 lo dice fidato, e
 *   Media3 per dirlo deve vedere il suo pacchetto: con targetSdk ≥ 30 lo vede
 *   solo se `<queries>` lo nomina. L'Assistente entra per pacchetto e
 *   certificato, come Android Auto: anche lui va nominato, o `getPackageUid`
 *   non lo trova.
 * - **Le icone delle schede dell'auto** (R11-5). Si prendono per nome
 *   (`android.resource://…/drawable/auto_*`), e la riduzione delle risorse
 *   della build di rilascio le toglie: `res/raw/keep.xml` le tiene.
 * - **Le metriche della WebView** (R11-7): Rymembra non manda niente a nessuno,
 *   e la WebView di sistema lo farebbe per lei senza l'opt-out del manifesto.
 * - **Il verso** (R14b-13, D-77): il telefono in verticale, per questo rilascio.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const APP = "../src-tauri/gen/android/app/";
const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");
const manifesto = leggi(`${APP}src/main/AndroidManifest.xml`).replace(/<!--[\s\S]*?-->/g, "");
const KOTLIN = `${APP}src/main/java/io/github/federicobaratti/rymembra/`;

/** Il blocco `<queries>` del manifesto, senza commenti. */
function interrogazioni(): string {
  const blocco = /<queries>([\s\S]*?)<\/queries>/.exec(manifesto);
  assert.ok(blocco, "il manifesto non ha <queries>");
  return blocco[1] ?? "";
}

test("gli ascoltatori delle notifiche si vedono: gli orologi e i telecomandi entrano (D-33)", () => {
  assert.match(
    interrogazioni(),
    /<intent>\s*<action\s+android:name="android\.service\.notification\.NotificationListenerService"\s*\/>\s*<\/intent>/,
  );
});

test("l'Assistente entra per pacchetto e certificato, come Android Auto (D-34)", () => {
  const controllori = leggi(`${KOTLIN}Controllori.kt`);
  for (const [pacchetto, certificato] of [
    // Dall'elenco dei chiamanti ammessi di UAMP, le chiavi di rilascio.
    [
      "com.google.android.googlequicksearchbox",
      "f0fd6c5b410f25cb25c3b53346c8972fae30f8ee7411df910480ad6b2d60db83",
    ],
    [
      "com.google.android.carassistant",
      "74b6fbf710e8d90d44d340125889b42306a62c4379d0e5a66220e3a68abf90e2",
    ],
  ]) {
    assert.match(interrogazioni(), new RegExp(`<package\\s+android:name="${pacchetto.replace(/\./g, "\\.")}"`));
    assert.ok(controllori.includes(`"${pacchetto}"`), `Controllori non nomina ${pacchetto}`);
    assert.ok(controllori.includes(`"${certificato}"`), `Controllori non conosce il certificato di ${pacchetto}`);
  }
});

test("le icone delle schede dell'auto, prese per nome, restano nella build di rilascio", () => {
  const biblioteca = leggi(`${KOTLIN}Biblioteca.kt`);
  const perNome = [...biblioteca.matchAll(/"(auto_[a-z_]+)"/g)].map((m) => m[1] ?? "");
  assert.ok(perNome.length >= 4, "Biblioteca non prende più icone per nome?");
  const tieni = leggi(`${APP}src/main/res/raw/keep.xml`);
  const regola = /tools:keep="([^"]*)"/.exec(tieni)?.[1] ?? "";
  const voci = regola.split(",").map((v) => v.trim());
  for (const nome of perNome) {
    const tenuta = voci.some((voce) => {
      const [tipo, schema] = voce.replace(/^@/, "").split("/");
      if (tipo !== "drawable" || schema === undefined) return false;
      return new RegExp(`^${schema.replace(/\*/g, ".*")}$`).test(nome);
    });
    assert.ok(tenuta, `drawable/${nome} non è in res/raw/keep.xml: la riduzione delle risorse la toglie`);
  }
});

test("i metodi che wry chiama per nome e nessuna regola teneva restano", () => {
  const regole = leggi(`${APP}proguard-rules.pro`).replace(/#.*$/gm, "");
  assert.match(regole, /void clearAllBrowsingData\(\);/);
  assert.match(regole, /java\.lang\.String getCookies\(java\.lang\.String\);/);
  assert.match(regole, /-keepattributes[^\n]*RuntimeVisibleAnnotations/);
});

test("la WebView di sistema non manda metriche d'uso a nome di Rymembra", () => {
  assert.match(
    manifesto,
    /<meta-data\s+android:name="android\.webkit\.WebView\.MetricsOptOut"\s+android:value="true"\s*\/>/,
  );
});

test("il telefono resta in verticale (D-77)", () => {
  const attivita = /<activity\b[^>]*android:name="\.MainActivity"[^>]*>/.exec(manifesto)?.[0] ?? "";
  assert.ok(attivita, "MainActivity non c'è");
  assert.match(attivita, /android:screenOrientation="portrait"/);
});
