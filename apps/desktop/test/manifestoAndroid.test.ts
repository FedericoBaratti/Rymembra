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

// ── il fornitore delle copertine (F1, #1) ───────────────────────────────────

const COPERTINE_KT = leggi(`${KOTLIN}Copertine.kt`);
const SFOGLIA_RS = leggi("../src-tauri/src/lontano/sfoglia.rs");

/**
 * Un `Regex("…")` di `Copertine.kt`, come espressione di JavaScript che deve
 * combaciare per intero (come `Regex.matches` in Kotlin). Il sorgente porta le
 * barre rovesce raddoppiate (`\\.`): in memoria ne resta una.
 */
function regolaKotlin(nome: string): RegExp {
  const riga = new RegExp(`^\\s*private val ${nome} = Regex\\("(.*)"\\)\\s*$`, "m");
  const letterale = riga.exec(COPERTINE_KT)?.[1];
  assert.ok(letterale, `Copertine.kt non ha più ${nome}`);
  return new RegExp(`^(?:${letterale.replace(/\\\\/g, "\\")})$`);
}

const DISPOSITIVO = "a".repeat(64);

test("il fornitore apre le copertine degli altri dispositivi solo nella forma lontane/<64 hex>/<hex 2-128>.jpg", () => {
  // Rust scrive `lontane/<dispositivo>/<impronta>.jpg` (`percorso_per_android`),
  // e Kotlin riconosce lo stesso nome: i due non si parlano, e un disaccordo
  // si scopre solo sul telefono, con un'auto senza immagine.
  assert.ok(
    SFOGLIA_RS.includes('format!("lontane/{dispositivo}/{impronta}.jpg")'),
    "Rust non scrive più lontane/<dispositivo>/<impronta>.jpg",
  );
  assert.match(COPERTINE_KT, /private const val LONTANE = "lontane"/);
  const dispositivo = regolaKotlin("DISPOSITIVO");
  const impronta = regolaKotlin("IMPRONTA_LONTANA");

  assert.ok(dispositivo.test(DISPOSITIVO));
  for (const buona of ["0123456789abcdef.jpg", "ab.jpg", `${"f".repeat(128)}.jpg`]) {
    assert.ok(impronta.test(buona), buona);
  }
  // Il dispositivo: 64 cifre esadecimali minuscole, né una di più né una di meno.
  for (const cattivo of ["a".repeat(63), "a".repeat(65), "A".repeat(64), "g".repeat(64), `${"a".repeat(63)}/`, ".."]) {
    assert.ok(!dispositivo.test(cattivo), cattivo);
  }
  // L'impronta: esadecimale minuscolo, da 2 a 128, e `.jpg` e basta (niente
  // `.m`/`.t` dello store, niente trattini del vecchio nome della cache).
  for (const cattiva of [
    "a.jpg",
    `${"f".repeat(129)}.jpg`,
    "ABCDEF.jpg",
    "0123456789abcdeg.jpg",
    "0123456789abcdef.m.jpg",
    "0123456789abcdef.png",
    "0123456789abcdef.jpg/",
    `${DISPOSITIVO}-0123456789abcdef.jpg`,
    "../0123456789abcdef.jpg",
    "..%2F..%2Fx.jpg",
    "..jpg",
    ".jpg",
    "",
  ]) {
    assert.ok(!impronta.test(cattiva), cattiva);
  }
});

test("il fornitore tiene i due controlli: la forma del percorso e il percorso canonico dentro la radice", () => {
  // Due radici, tre segmenti per la seconda: la forma si guarda prima di
  // costruire un File, e il percorso canonico dopo.
  assert.match(COPERTINE_KT, /parti\.size == 3 && parti\[0\] == LONTANE && DISPOSITIVO\.matches\(parti\[1\]\)/);
  assert.match(COPERTINE_KT, /IMPRONTA_LONTANA\.matches\(parti\[2\]\)/);
  assert.match(COPERTINE_KT, /parti\.size == 2 && SOTTOCARTELLA\.matches\(parti\[0\]\) && NOME\.matches\(parti\[1\]\)/);
  // Il controllo è su `canonicalFile`, con la radice canonicalizzata ogni
  // volta (`/data/user/0` e `/data/data` sono lo stesso posto), e col
  // separatore: `…/copertine-lontane-altra` non è dentro `…/copertine-lontane`.
  assert.match(COPERTINE_KT, /file\.canonicalFile/);
  assert.match(COPERTINE_KT, /canonico\.path\.startsWith\(radice\.canonicalPath \+ File\.separator\)/);
  // Una seconda radice che Rust non ha detto non apre niente.
  assert.match(COPERTINE_KT, /cartellaLontane\(contesto\)\s*\?: throw FileNotFoundException/);
  // E sempre in sola lettura.
  assert.match(COPERTINE_KT, /mode != "r"\) throw SecurityException/);
});

test("Rust dice a Kotlin la seconda radice in sessione.pronta, e Kotlin la ricorda o la toglie", () => {
  const ponte = leggi(`${KOTLIN}Ponte.kt`);
  assert.match(
    ponte,
    /"sessione\.pronta"\s*->\s*Nucleo\.pronta\(contesto\(\), a\.getString\("copertine"\), a\.optString\("lontane", ""\)\.ifEmpty \{ null \}\)/,
  );
  const nucleo = leggi(`${KOTLIN}Nucleo.kt`);
  assert.match(nucleo, /fun pronta\(contesto: Context, copertine: String, lontane: String\?\)/);
  assert.match(nucleo, /Copertine\.ricorda\(contesto, copertine, lontane\)/);
  // `putString(…, null)` toglie la voce: una cache che non c'è più non resta.
  assert.match(COPERTINE_KT, /\.putString\("lontane", lontane\)/);
});

test("il ricordo dell'ultimo brano si riscrive anche quando cambia solo la copertina", () => {
  // La copertina di un brano di un altro dispositivo arriva dopo il brano: con
  // l'id uguale e la copertina nuova il ricordo restava senza immagine.
  const nucleo = leggi(`${KOTLIN}Nucleo.kt`);
  assert.match(nucleo, /if \(scheda\.id != scritto \|\| scheda\.copertina != copertinaScritta\)/);
  assert.match(nucleo, /copertinaScritta = scheda\.copertina/);
});
