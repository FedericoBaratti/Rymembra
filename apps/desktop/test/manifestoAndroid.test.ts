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

// ── «Resta attiva in sottofondo» (F6, `Presenza.kt`) ─────────────────────────

/** Un sorgente Kotlin, senza commenti: i commenti nominano quel che il codice non usa. */
const kotlin = (nome: string) =>
  leggi(`${KOTLIN}${nome}`).replace(/\/\*[\s\S]*?\*\/|\/\/.*$/gm, "");

/** I permessi che il manifesto chiede, per nome corto. */
function permessiChiesti(): Set<string> {
  return new Set(
    [...manifesto.matchAll(/<uses-permission\s+android:name="android\.permission\.([A-Z_]+)"/g)].map(
      (m) => m[1] ?? "",
    ),
  );
}

test("il servizio del sottofondo è connectedDevice, interno, e ha tutto quel che Android 14 vuole", () => {
  const servizio = /<service\b[^>]*android:name="\.Presenza"[^>]*>/.exec(manifesto)?.[0] ?? "";
  assert.ok(servizio, "il servizio .Presenza non c'è");
  assert.match(servizio, /android:foregroundServiceType="connectedDevice"/);
  assert.match(servizio, /android:exported="false"/, "lo avvia solo l'app");

  const permessi = permessiChiesti();
  assert.ok(permessi.has("FOREGROUND_SERVICE"));
  assert.ok(
    permessi.has("FOREGROUND_SERVICE_CONNECTED_DEVICE"),
    "senza il permesso del tipo, da Android 14 startForeground dà SecurityException",
  );
  // Uno dei prerequisiti di `connectedDevice` (developer.android.com, «Foreground
  // service types»): senza, ancora SecurityException, e solo sul telefono.
  const prerequisiti = ["CHANGE_NETWORK_STATE", "CHANGE_WIFI_STATE", "CHANGE_WIFI_MULTICAST_STATE", "NFC", "TRANSMIT_IR"];
  assert.ok(
    prerequisiti.some((p) => permessi.has(p)),
    `connectedDevice vuole uno fra ${prerequisiti.join(", ")}`,
  );
});

test("Presenza passa il tipo giusto a startForeground e risorge da sola", () => {
  const presenza = kotlin("Presenza.kt");
  assert.match(presenza, /startForeground\(ID, notifica, ServiceInfo\.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE\)/);
  assert.match(presenza, /return START_STICKY/);
  // Il tasto «Spegni» e la notifica minima: canale a bassa importanza.
  assert.match(presenza, /IMPORTANCE_LOW/);
  assert.match(presenza, /addAction\(/);
});

test("il risparmio batteria si toglie dalla pagina di sistema, non con la richiesta diretta", () => {
  // La policy di Google Play vieta `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS` a chi
  // non rompe il proprio compito senza (developer.android.com, «Doze e App
  // Standby»): il pulsante apre l'elenco, e basta.
  assert.ok(!permessiChiesti().has("REQUEST_IGNORE_BATTERY_OPTIMIZATIONS"));
  assert.doesNotMatch(manifesto, /REQUEST_IGNORE_BATTERY_OPTIMIZATIONS/);
  for (const nome of ["Presenza.kt", "Ponte.kt", "Permessi.kt", "MainActivity.kt"]) {
    assert.doesNotMatch(kotlin(nome), /ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS/, nome);
  }
  assert.match(kotlin("Presenza.kt"), /Settings\.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS/);
});

test("tolta dalle recenti, con il sottofondo acceso, la musica ferma non si spegne", () => {
  const musica = kotlin("ServizioMusica.kt");
  const da = musica.indexOf("override fun onTaskRemoved");
  assert.ok(da >= 0, "manca onTaskRemoved");
  const corpo = musica.slice(da, musica.indexOf("override fun onDestroy"));
  assert.ok(
    corpo.indexOf("Presenza.voluto(this)") >= 0 &&
      corpo.indexOf("Presenza.voluto(this)") < corpo.indexOf("stopSelf()"),
    "onTaskRemoved deve guardare il sottofondo prima di fermarsi",
  );
});

test("la notifica del sottofondo ha le sue frasi in inglese e in italiano, e la sua icona", () => {
  const nomi = [
    "presenza_canale",
    "presenza_titolo",
    "presenza_titolo_uno",
    "presenza_titolo_piu",
    "presenza_titolo_riposo",
    "presenza_testo_riposo",
    "presenza_spegni",
  ];
  for (const cartella of ["values", "values-it"]) {
    const frasi = leggi(`${APP}src/main/res/${cartella}/strings.xml`);
    for (const nome of nomi) {
      assert.match(frasi, new RegExp(`<string name="${nome}">`), `${cartella}: manca ${nome}`);
    }
  }
  const icona = leggi(`${APP}src/main/res/drawable/notifica_presenza.xml`);
  assert.match(icona, /<vector\b/);
});
