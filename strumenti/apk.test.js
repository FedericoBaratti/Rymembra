/*
 * I controlli dell'APK, sulle uscite vere degli strumenti.
 *
 * `apk.js` gira solo nella CI di rilascio, dopo la build del telefono: il posto
 * peggiore per scoprire che un'espressione regolare non riconosce più l'uscita
 * di `apksigner`. I testi qui sotto sono pezzi delle uscite vere (un APK di
 * prova firmato con una chiave usa e getta, il `mapping.txt` di R8 della
 * stessa build, l'APK debug), accorciati; zip ed ELF si costruiscono a mano.
 *
 *   node --test "strumenti/*.test.js"
 */
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const {
  ABI,
  nomeAsset,
  normalizzaImpronta,
  improntaInchiodata,
  voci,
  allineamentiLoad,
  problemiPagine,
  problemiBadging,
  problemiMapping,
  problemiFirma,
  problemiSig,
  problemiRisorse,
  iconeDellAuto,
} = require("./apk.js");
const { nuovaChiave, commentoDi } = require("./firma-di-prova.js");

const IMPRONTA = "24:85:94:2C:0F:5D:73:42:97:CD:96:A6:65:19:C7:2E:49:22:C7:05:37:E8:F3:9A:B2:D9:80:60:24:18:2A:C1";

/* ── il nome, e `rilasci.rs::apk_per` ─────────────────────────────────── */

/** La scelta di `apk_per` (rilasci.rs), riscritta: per un telefono aarch64. */
function sceltoDaApkPer(nomi) {
  const ARCHITETTURE = ["arm64", "aarch64", "armeabi", "armv7", "x86_64", "x86", "x64"];
  const apk = nomi.filter((n) => n.toLowerCase().endsWith(".apk"));
  const nomina = (n, parole) => parole.some((p) => n.toLowerCase().includes(p));
  return (
    apk.find((n) => nomina(n, ["arm64", "aarch64"])) ??
    apk.find((n) => nomina(n, ["universal"])) ??
    (apk.length === 1 && !nomina(apk[0], ARCHITETTURE) ? apk[0] : null)
  );
}

test("il nome dell'asset lo trova l'app del telefono, e il .sig no", () => {
  const nome = nomeAsset("3.2.1", "Rymembra");
  assert.equal(nome, "Rymembra_3.2.1_android-arm64.apk");
  const asset = ["Rymembra_3.2.1_x64-setup.exe", "Rymembra_3.2.1_x64-setup.exe.sig", "latest.json", nome, `${nome}.sig`, "LICENSE"];
  assert.equal(sceltoDaApkPer(asset), nome);
  assert.ok(!`${nome}.sig`.toLowerCase().endsWith(".apk"));
  // Nessun'altra architettura nel nome: `apk_per` cerca le parole dentro il nome.
  for (const altra of ["x86_64", "x86", "x64", "armeabi", "armv7", "universal"]) assert.ok(!nome.toLowerCase().includes(altra), altra);
  assert.equal(nomeAsset("3.2.1"), nome);
});

test("l'impronta inchiodata è quella del certificato di rilascio", () => {
  assert.equal(improntaInchiodata(), normalizzaImpronta(IMPRONTA));
  assert.equal(improntaInchiodata().length, 64);
  // apksigner la stampa in minuscolo e senza due punti.
  assert.equal(normalizzaImpronta("2485942c0f5d734297cd96a66519c72e4922c70537e8f39ab2d9806024182ac1"), improntaInchiodata());
});

/* ── apksigner ────────────────────────────────────────────────────────── */

/** L'uscita vera di `apksigner verify --verbose --print-certs` (build-tools 36.1.0), con un'impronta a scelta. */
function verifica({ dn = "CN=Rymembra, O=Federico Baratti", sha = "2485942c0f5d734297cd96a66519c72e4922c70537e8f39ab2d9806024182ac1", firmatari = 1, v2 = true } = {}) {
  return [
    "Verifies",
    "Verified using v1 scheme (JAR signing): false",
    `Verified using v2 scheme (APK Signature Scheme v2): ${v2}`,
    "Verified using v3 scheme (APK Signature Scheme v3): true",
    "Verified using v3.1 scheme (APK Signature Scheme v3.1): false",
    "Verified using v4 scheme (APK Signature Scheme v4): false",
    "Verified for SourceStamp: false",
    `Number of signers: ${firmatari}`,
    `Signer #1 certificate DN: ${dn}`,
    `Signer #1 certificate SHA-256 digest: ${sha}`,
    "Signer #1 certificate SHA-1 digest: 4c018aeb4e67dcb484c723c00dd0f7ed68202903",
    "Signer #1 key algorithm: RSA",
    "Signer #1 key size (bits): 4096",
    "",
  ].join("\r\n");
}

test("un APK firmato con la chiave dei rilasci passa", () => {
  assert.deepEqual(problemiFirma(verifica(), IMPRONTA), []);
});

test("un'altra chiave, la chiave di debug, due firmatari o niente v2 fermano la release", () => {
  // La chiave usa e getta della prova del coordinatore.
  const altra = problemiFirma(verifica({ sha: "7ee9185edb9d76f86adef90fce1976d95a1e0bc4e039612241aee4d181c5e34b" }), IMPRONTA);
  assert.equal(altra.length, 1);
  assert.match(altra[0], /non quello dei rilasci/);
  assert.ok(problemiFirma(verifica({ dn: "C=US, O=Android, CN=Android Debug" }), IMPRONTA).some((p) => /debug/.test(p)));
  assert.ok(problemiFirma(verifica({ firmatari: 2 }), IMPRONTA).some((p) => /firmatari/.test(p)));
  assert.ok(problemiFirma(verifica({ v2: false }), IMPRONTA).some((p) => /v2/.test(p)));
  assert.ok(problemiFirma("DOES NOT VERIFY\nERROR: …", IMPRONTA).length >= 3);
});

/* ── aapt2 ────────────────────────────────────────────────────────────── */

function badging({ nome = "io.github.federicobaratti.rymembra", codice = "3002001", versione = "3.2.1", debug = false, native = "arm64-v8a" } = {}) {
  return [
    `package: name='${nome}' versionCode='${codice}' versionName='${versione}' platformBuildVersionName='17' platformBuildVersionCode='37' compileSdkVersion='37' compileSdkVersionCodename='17'`,
    "minSdkVersion:'26'",
    "targetSdkVersion:'37'",
    "uses-permission: name='android.permission.INTERNET'",
    "application: label='Rymembra' icon='res/mipmap-anydpi-v26/ic_launcher.xml'",
    ...(debug ? ["application-debuggable"] : []),
    "launchable-activity: name='io.github.federicobaratti.rymembra.MainActivity'  label='' icon=''",
    `native-code: '${native}'`,
    "",
  ].join("\n");
}

const ATTESO = { pacchetto: "io.github.federicobaratti.rymembra", versione: "3.2.1" };

test("il manifesto dell'APK dice il pacchetto, la versione e il versionCode giusti", () => {
  assert.deepEqual(problemiBadging(badging(), ATTESO), []);
  assert.ok(problemiBadging(badging({ debug: true }), ATTESO).some((p) => /debuggable/.test(p)));
  assert.ok(problemiBadging(badging({ codice: "3002000" }), ATTESO).some((p) => /versionCode/.test(p)));
  assert.ok(problemiBadging(badging({ versione: "3.2.0" }), ATTESO).some((p) => /versionName/.test(p)));
  assert.ok(problemiBadging(badging({ nome: "io.github.federicobaratti.rymembra.debug" }), ATTESO).some((p) => /pacchetto/.test(p)));
  assert.ok(problemiBadging(badging({ native: "arm64-v8a' 'x86_64" }), ATTESO).some((p) => /native-code/.test(p)));
});

/* ── R8 ───────────────────────────────────────────────────────────────── */

/** Pezzi del `mapping.txt` vero (R8 9.3.16, AGP 9.3.1), con i membri che `TENUTI` cerca. */
const MAPPING = [
  "# compiler: R8",
  "# compiler_version: 9.3.16",
  "io.github.federicobaratti.rymembra.Ipc -> io.github.federicobaratti.rymembra.Ipc:",
  "    7:14:void <init>(io.github.federicobaratti.rymembra.RustWebView,io.github.federicobaratti.rymembra.RustWebViewClient):13:13 -> <init>",
  "io.github.federicobaratti.rymembra.Ponte -> io.github.federicobaratti.rymembra.Ponte:",
  '# {"id":"sourceFile","fileName":"Ponte.kt"}',
  "    android.content.Context contesto -> b",
  "    1:12:android.content.Context contesto():80:80 -> c",
  "    7:14:java.lang.String chiama(java.lang.String,java.lang.String):113:113 -> chiama",
  "    15:21:boolean kotlin.jvm.internal.Intrinsics.areEqual(java.lang.Object,java.lang.Object):169:169 -> chiama",
  "    15:21:java.lang.String chiama(java.lang.String,java.lang.String):113 -> chiama",
  "io.github.federicobaratti.rymembra.RustWebView -> io.github.federicobaratti.rymembra.RustWebView:",
  "    10:16:void <init>(android.content.Context,java.lang.String[],java.lang.String):19:19 -> <init>",
  "io.github.federicobaratti.rymembra.WryActivity -> io.github.federicobaratti.rymembra.WryActivity:",
  "    int id -> o0",
  "    4:8:java.lang.Class getAppClass(java.lang.String):186:186 -> getAppClass",
  "    4:5:void setWebView(io.github.federicobaratti.rymembra.RustWebView):56:56 -> setWebView",
  "    17:24:androidx.activity.OnBackPressedDispatcher androidx.activity.ComponentActivity.getOnBackPressedDispatcher():597:597 -> setWebView",
  "",
].join("\r\n");

test("R8 ha lasciato Ponte.chiama e i nomi che Rust chiama attraverso JNI", () => {
  assert.deepEqual(problemiMapping(MAPPING), []);
});

test("un Ponte.chiama rinominato o tolto da R8 ferma la release", () => {
  const rinominato = MAPPING.replace(/\):113:113 -> chiama/, "):113:113 -> a").replace(/\):113 -> chiama/, "):113 -> a");
  assert.deepEqual(problemiMapping(rinominato), ["R8 ha rinominato io.github.federicobaratti.rymembra.Ponte.chiama in a"]);
  const tolto = MAPPING.split("\r\n").filter((r) => !/ chiama\(java/.test(r)).join("\r\n");
  assert.match(problemiMapping(tolto).join("\n"), /R8 ha tolto io\.github\.federicobaratti\.rymembra\.Ponte\.chiama/);
  // Un metodo incorporato con lo stesso nome non vale come il membro.
  const soloIncorporato = MAPPING.split("\r\n").filter((r) => !/:java\.lang\.String chiama\(/.test(r)).join("\r\n");
  assert.ok(soloIncorporato.includes("areEqual(java.lang.Object,java.lang.Object):169:169 -> chiama"));
  assert.match(problemiMapping(soloIncorporato).join("\n"), /tolto .*Ponte\.chiama/);
  const senzaClasse = MAPPING.replace("io.github.federicobaratti.rymembra.Ponte -> io.github.federicobaratti.rymembra.Ponte:", "a.b -> a.b:");
  assert.match(problemiMapping(senzaClasse).join("\n"), /tolto la classe io\.github\.federicobaratti\.rymembra\.Ponte/);
  const classeRinominata = MAPPING.replace("rymembra.Ponte -> io.github.federicobaratti.rymembra.Ponte:", "rymembra.Ponte -> d2:");
  assert.match(problemiMapping(classeRinominata).join("\n"), /rinominato io\.github\.federicobaratti\.rymembra\.Ponte in d2/);
});

/* ── zip ed ELF, a mano ───────────────────────────────────────────────── */

/** Un ELF a 64 bit con solo le intestazioni dei programmi: un `LOAD` per allineamento. */
function elf(allineamenti) {
  const testa = 64;
  const b = Buffer.alloc(testa + 56 * (allineamenti.length + 1));
  b.writeUInt32BE(0x7f454c46, 0);
  b[4] = 2;
  b[5] = 1;
  b.writeBigUInt64LE(BigInt(testa), 0x20);
  b.writeUInt16LE(56, 0x36);
  b.writeUInt16LE(allineamenti.length + 1, 0x38);
  // Un PT_PHDR davanti, che non conta.
  b.writeUInt32LE(6, testa);
  b.writeBigUInt64LE(8n, testa + 48);
  allineamenti.forEach((a, i) => {
    const p = testa + 56 * (i + 1);
    b.writeUInt32LE(1, p);
    b.writeBigUInt64LE(BigInt(a), p + 48);
  });
  return b;
}

/** Uno zip con voci memorizzate (metodo 0), come le `.so` di un APK con `extractNativeLibs=false`. */
function zip(file) {
  const locali = [];
  const centrali = [];
  let posto = 0;
  for (const [nome, dati] of Object.entries(file)) {
    const n = Buffer.from(nome, "utf8");
    const l = Buffer.alloc(30);
    l.writeUInt32LE(0x04034b50, 0);
    l.writeUInt32LE(dati.length, 18);
    l.writeUInt32LE(dati.length, 22);
    l.writeUInt16LE(n.length, 26);
    const c = Buffer.alloc(46);
    c.writeUInt32LE(0x02014b50, 0);
    c.writeUInt32LE(dati.length, 20);
    c.writeUInt32LE(dati.length, 24);
    c.writeUInt16LE(n.length, 28);
    c.writeUInt32LE(posto, 42);
    locali.push(l, n, dati);
    centrali.push(c, n);
    posto += 30 + n.length + dati.length;
  }
  const dir = Buffer.concat(centrali);
  const fine = Buffer.alloc(22);
  fine.writeUInt32LE(0x06054b50, 0);
  fine.writeUInt16LE(Object.keys(file).length, 8);
  fine.writeUInt16LE(Object.keys(file).length, 10);
  fine.writeUInt32LE(dir.length, 12);
  fine.writeUInt32LE(posto, 16);
  return Buffer.concat([...locali, dir, fine]);
}

test("le librerie native allineate a 16 KB passano", () => {
  assert.deepEqual(allineamentiLoad(elf([0x4000, 0x4000])), [0x4000, 0x4000]);
  const apk = zip({ "AndroidManifest.xml": Buffer.from("x"), [`lib/${ABI}/librymembra_lib.so`]: elf([0x4000, 0x4000]), [`lib/${ABI}/libsurface_util_jni.so`]: elf([0x10000]) });
  assert.equal(voci(apk).length, 3);
  assert.deepEqual(problemiPagine(apk), []);
});

test("una libreria a 4 KB, un'altra ABI o nessuna libreria fermano la release", () => {
  // Quel che esce con l'NDK 27 di serie sull'immagine di GitHub: niente errori, pagine da 4 KB.
  const quattro = problemiPagine(zip({ [`lib/${ABI}/librymembra_lib.so`]: elf([0x4000, 0x1000]) }));
  assert.equal(quattro.length, 1);
  assert.match(quattro[0], /0x1000 invece di 0x4000/);
  assert.match(problemiPagine(zip({ "lib/x86_64/librymembra_lib.so": elf([0x4000]) })).join("\n"), /ABI x86_64/);
  assert.match(problemiPagine(zip({ "classes.dex": Buffer.from("dex") })).join("\n"), /nessuna libreria nativa/);
  assert.match(problemiPagine(zip({ [`lib/${ABI}/rotta.so`]: Buffer.from("non un elf") })).join("\n"), /non è un ELF/);
});

/* ── il .sig di minisign ──────────────────────────────────────────────── */

/**
 * Una firma vera (chiave usa e getta, `firma-di-prova.js`) dei byte `APK`, con
 * un commento fidato a scelta. `altraChiave`: la firma è di una chiave che non è
 * quella della `pubkey` data.
 */
const APK = Buffer.from("un apk firmato");
function sig(commento, altraChiave = false) {
  const chiave = nuovaChiave();
  return { sig: (altraChiave ? nuovaChiave() : chiave).firma(APK, commento), pubkey: chiave.pubkey };
}

test("il .apk.sig è della chiave dell'updater, per quella versione e quel file", () => {
  const file = "Rymembra_3.2.1_android-arm64.apk";
  const buono = sig(commentoDi(file, "3.2.1"));
  assert.deepEqual(problemiSig(buono.sig, { pubkey: buono.pubkey, versione: "3.2.1", file, byte: APK }), []);
  const altra = sig(commentoDi(file, "3.2.1"), true);
  assert.match(problemiSig(altra.sig, { pubkey: altra.pubkey, versione: "3.2.1", file, byte: APK }).join("\n"), /non con quella dell'updater/);
  const senzaVersione = sig(`timestamp:1790000000\tfile:${file}`);
  assert.match(problemiSig(senzaVersione.sig, { pubkey: senzaVersione.pubkey, versione: "3.2.1", file, byte: APK }).join("\n"), /\(nessuna\)/);
  const altroFile = sig(commentoDi("app-arm64-release-unsigned.apk", "3.2.1"));
  assert.match(problemiSig(altroFile.sig, { pubkey: altroFile.pubkey, versione: "3.2.1", file, byte: APK }).join("\n"), /un altro file/);
});

/* ── aapt2 dump resources ─────────────────────────────────────────────── */

/**
 * L'uscita vera di `aapt2 dump resources` (build-tools 36.1.0) sulla release di
 * prova di R11, quella senza `keep.xml`: dopo `abc_vector_test` (0x76) viene
 * `avd_hide_password` (0x7d), e i sei identificativi in mezzo sono le icone di
 * Android Auto che la riduzione delle risorse ha tolto. Accorciata.
 */
const RISORSE_SENZA = [
  "Binary APK",
  "Package name=io.github.federicobaratti.rymembra id=7f",
  "  type drawable id=07 entryCount=262",
  "    resource 0x7f070075 drawable/abc_textfield_search_material",
  "      () (file) res/4u.xml type=XML",
  "    resource 0x7f070076 drawable/abc_vector_test",
  "      () (file) res/hq.xml type=XML",
  "    resource 0x7f07007d drawable/avd_hide_password",
  "      () (file) res/Tf.xml type=XML",
  "  type string id=0f entryCount=121",
  "    resource 0x7f0f0010 string/auto_album",
  "      () \"Album\"",
  "",
].join("\n");

/** La stessa, con le sei icone tenute: i nomi restano, i file hanno il nome corto di R8. */
function risorseCon(nomi) {
  const righe = RISORSE_SENZA.split("\n");
  const dopo = righe.indexOf("      () (file) res/hq.xml type=XML") + 1;
  const icone = nomi.flatMap((nome, i) => [
    `    resource 0x7f07007${(7 + i).toString(16)} drawable/${nome}`,
    `      () (file) res/a${i}.xml type=XML`,
  ]);
  return [...righe.slice(0, dopo), ...icone, ...righe.slice(dopo)].join("\n");
}

test("le icone di Android Auto sono le sei auto_* del progetto", () => {
  assert.deepEqual(iconeDellAuto(), ["auto_album", "auto_artisti", "auto_playlist", "auto_preferiti", "auto_recenti", "auto_suona"]);
});

test("un APK con le sei icone di Android Auto passa", () => {
  assert.deepEqual(problemiRisorse(risorseCon(iconeDellAuto()), iconeDellAuto()), []);
});

test("un APK senza le icone di Android Auto ferma la release (R11-5)", () => {
  // Nella release di prova R8 le aveva tolte tutte: l'auto mostrava le
  // schede senza icona, e nessun controllo se ne accorgeva.
  const problemi = problemiRisorse(RISORSE_SENZA, iconeDellAuto());
  assert.equal(problemi.length, 6, problemi.join("\n"));
  assert.match(problemi[0], /drawable\/auto_album/);
  // Una stringa con lo stesso nome non è l'icona.
  assert.match(problemi.join("\n"), /auto_album/);
  // Ne basta una che manca.
  const cinque = iconeDellAuto().filter((n) => n !== "auto_suona");
  assert.deepEqual(problemiRisorse(risorseCon(cinque), iconeDellAuto()).length, 1);
  // Un'uscita che non è quella di aapt2 non passa per buona.
  assert.match(problemiRisorse("", iconeDellAuto()).join("\n"), /non ha descritto/);
});

/* ── S5-2: il .apk.sig contro i byte dell'APK ─────────────────────────── */

test("il .apk.sig con i byte cambiati non passa (S5-2)", () => {
  const file = "Rymembra_3.2.1_android-arm64.apk";
  const chiave = nuovaChiave();
  const firma = chiave.firma(APK, commentoDi(file, "3.2.1"));
  const dati = { pubkey: chiave.pubkey, versione: "3.2.1", file };
  assert.deepEqual(problemiSig(firma, { ...dati, byte: APK }), []);
  // Un `zipalign` o una ricompressione dopo la firma: stessa chiave, stessa
  // versione, stesso nome nel commento, altri byte.
  const cambiato = Buffer.from(APK);
  cambiato[0] ^= 1;
  assert.match(problemiSig(firma, { ...dati, byte: cambiato }).join("\n"), /non è valida per i byte/);
  // Senza i byte non c'è niente da verificare, e non passa per buono in silenzio.
  assert.throws(() => problemiSig(firma, dati), /byte/);
});

/* ── S5-7: l'impronta del certificato esiste in due copie ─────────────── */

test("l'impronta dell'app (rilasci.rs) è quella inchiodata per la CI (android-certificato.sha256)", () => {
  // Due copie, due lettori: la CI approva l'APK se il certificato è quello di
  // `android-certificato.sha256`; l'app sul telefono giudica «un'altra chiave»
  // ogni copia che non sia quella di `IMPRONTA_DEL_RILASCIO`. Se divergono, la
  // CI approva un APK che nessun telefono scarica più, senza un errore.
  const rust = fs.readFileSync(path.join(__dirname, "../apps/desktop/src-tauri/src/rilasci.rs"), "utf8");
  // La costante va a capo dopo `=`: `\s*` copre anche l'a capo.
  const m = /pub const IMPRONTA_DEL_RILASCIO\s*:\s*&str\s*=\s*"([^"]*)"/.exec(rust);
  assert.ok(m, "rilasci.rs non ha più `pub const IMPRONTA_DEL_RILASCIO: &str = \"…\"`");
  assert.equal(normalizzaImpronta(m[1]), improntaInchiodata());
  // La forma che l'app confronta è esadecimale minuscolo, senza due punti (`impronta_normale`).
  assert.match(m[1], /^[0-9a-f]{64}$/);
});
