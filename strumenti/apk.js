/*
 * L'APK per i telefoni, guardato prima e dopo la firma.
 *
 * # Perché uno script e non dieci righe di `bash` in `release.yml`
 *
 * Perché quel che va controllato su un APK è fatto di uscite di strumenti
 * diversi (`zipalign`, `aapt2`, `apksigner`, il `mapping.txt` di R8, l'ELF
 * delle librerie), e un `grep` scritto in un workflow non lo prova nessuno
 * finché la sera del tag non dice «ok» a un APK sbagliato. Qui ogni controllo
 * è una funzione che legge un testo o dei byte, provata in `apk.test.js` sulle
 * uscite vere degli strumenti; il workflow chiama i due comandi qui sotto.
 *
 * # Che cosa si guarda, prima della firma (lavoro `apk`, senza segreti)
 *
 *   - **Le pagine da 16 KB.** Ogni `LOAD` di ogni `lib/<abi>/*.so` allineato a
 *     16 KB, e lo zip allineato (`zipalign -c -P 16 -v 4`). Con l'NDK 27 —
 *     quello di serie sull'immagine `ubuntu-24.04` — la `.so` esce a 4 KB senza
 *     nessun errore, e su un telefono a 16 KB l'app gira in «compatibilità»
 *     con un avviso al primo avvio. Solo `arm64-v8a`: il nome dell'asset dice
 *     «arm64», e `rilasci.rs::apk_per` lo sceglie per quello.
 *   - **Il manifesto** (`aapt2 dump badging`): il pacchetto di
 *     `tauri.conf.json`, `versionName` uguale alla versione, `versionCode`
 *     come lo calcola la CLI di Tauri (`versione.js::codiceAndroid`), e
 *     **niente** `application-debuggable`.
 *   - **R8** (`mapping.txt`): i nomi che Rust e tao/wry chiamano per nome
 *     attraverso JNI devono esserci, col loro nome. Senza `Ponte.chiama` ogni
 *     ponte verso Kotlin (portachiavi, sessione multimediale, selettore delle
 *     cartelle) fallisce; senza i costruttori di wry la finestra non nasce.
 *   - **Le icone di Android Auto** (`aapt2 dump resources`): ogni
 *     `res/drawable/auto_*.xml` del progetto c'è ancora nella tabella delle
 *     risorse. Arrivano all'auto per nome, come stringa, e la riduzione delle
 *     risorse di R8 non le vede: nella release di prova le aveva tolte tutte, e
 *     l'auto mostrava le schede senza icona (R11-5). `res/raw/keep.xml` le
 *     tiene; questo controllo si accorge del giorno in cui non le tiene più.
 *
 * # E dopo la firma (lavoro `firma-apk`, nell'ambiente `release`)
 *
 *   - lo stesso allineamento, **sul file firmato**: è quello che la gente
 *     scarica, e `apksigner` può riallineare le voci;
 *   - `apksigner verify --print-certs`: schema v2 verificato, un firmatario
 *     solo, non la chiave di debug, e l'impronta SHA-256 del certificato
 *     uguale a quella inchiodata in `android-certificato.sha256`. È il gemello
 *     del confronto di `manifesto.js` per il computer: una chiave sbagliata qui
 *     vuol dire che nessun telefono con l'app installata potrà aggiornarsi, e
 *     se ne accorgerebbe solo chi prova;
 *   - il `.apk.sig` (minisign, la chiave dell'updater): firmato con la chiave
 *     che gli eseguibili conoscono, per quella versione e per quel file.
 *
 * # Tre modi
 *
 *   node strumenti/apk.js nome                     il nome dell'asset, dalla versione
 *   node strumenti/apk.js controlla <apk> --mapping <mapping.txt>
 *   node strumenti/apk.js verifica <apk-firmato>   (il .sig accanto, <apk>.sig)
 *
 * Gli strumenti di Android si cercano in `ANDROID_BUILD_TOOLS` (la cartella
 * `build-tools/<versione>` dell'SDK).
 */
const { spawnSync } = require("child_process");
const fs = require("fs");
const path = require("path");
const zlib = require("zlib");

const { codiceAndroid } = require("./versione.js");

const REPO = path.dirname(__dirname);
const PACCHETTO = path.join(REPO, "apps/desktop/package.json");
const TAURI = path.join(REPO, "apps/desktop/src-tauri/tauri.conf.json");
/** Dove stanno le icone del progetto Android: vedi [`iconeDellAuto`]. */
const DISEGNI = path.join(REPO, "apps/desktop/src-tauri/gen/android/app/src/main/res/drawable");

/**
 * L'impronta SHA-256 del certificato di firma dei rilasci Android, nella forma
 * di `keytool` (coppie esadecimali maiuscole separate da due punti).
 *
 * Sta in un file suo, una riga sola, perché la leggano anche l'app (per
 * accorgersi, prima di scaricare, di essere una copia firmata con un'altra
 * chiave) e chi verifica a mano. La chiave non si può cambiare: chi ha l'app
 * installata accetta aggiornamenti solo dallo stesso certificato.
 */
const FILE_IMPRONTA = path.join(__dirname, "android-certificato.sha256");

/** L'unica ABI che esce: i telefoni a 64 bit (README, «Android»). */
const ABI = "arm64-v8a";

/** Sedici kilobyte: la pagina più grande che Android ammette. */
const PAGINA = 0x4000;

/**
 * I nomi che passano il confine con Rust per nome, e che R8 deve lasciare
 * dove sono. Classe, e il membro come lo scrive `mapping.txt` (tipo di
 * ritorno, nome, parametri). Vedi l'inventario di R11 e `proguard-*.pro`.
 */
const TENUTI = [
  // `rymembra-android/src/jvm.rs`: tutto il ponte verso Kotlin passa di qui.
  ["io.github.federicobaratti.rymembra.Ponte", "java.lang.String chiama(java.lang.String,java.lang.String)"],
  // wry crea la finestra con questi costruttori, da Rust.
  ["io.github.federicobaratti.rymembra.RustWebView", "void <init>(android.content.Context,java.lang.String[],java.lang.String)"],
  ["io.github.federicobaratti.rymembra.Ipc", "void <init>(io.github.federicobaratti.rymembra.RustWebView,io.github.federicobaratti.rymembra.RustWebViewClient)"],
  // tao chiede la classe dell'app all'attività, e wry le consegna la finestra.
  ["io.github.federicobaratti.rymembra.WryActivity", "java.lang.Class getAppClass(java.lang.String)"],
  ["io.github.federicobaratti.rymembra.WryActivity", "void setWebView(io.github.federicobaratti.rymembra.RustWebView)"],
];

/* ── i nomi ───────────────────────────────────────────────────────────── */

function versione() {
  return JSON.parse(fs.readFileSync(PACCHETTO, "utf8")).version;
}

function configurazione() {
  return JSON.parse(fs.readFileSync(TAURI, "utf8"));
}

/**
 * Il nome dell'asset sulla release: `Rymembra_3.2.1_android-arm64.apk`.
 *
 * Deve far trovare l'APK a `rilasci.rs::apk_per` su un telefono `aarch64`:
 * finisce in `.apk`, dice «arm64» e nessun'altra architettura. Il `.sig`
 * accanto (`….apk.sig`) non finisce in `.apk`, e quindi non lo sceglie mai.
 */
function nomeAsset(v, prodotto = configurazione().productName) {
  return `${prodotto}_${v}_android-arm64.apk`;
}

/** L'impronta in una forma sola: esadecimale minuscolo, senza separatori. */
function normalizzaImpronta(s) {
  return String(s).replace(/[^0-9a-f]/gi, "").toLowerCase();
}

function improntaInchiodata() {
  const riga = fs.readFileSync(FILE_IMPRONTA, "utf8").trim();
  const n = normalizzaImpronta(riga);
  if (n.length !== 64) throw new Error(`${FILE_IMPRONTA}: non è un'impronta SHA-256`);
  return n;
}

/* ── lo zip e l'ELF ───────────────────────────────────────────────────── */

/**
 * Le voci di uno zip, dalla directory centrale. Un APK è uno zip; il blocco
 * della firma v2 sta fra le voci e la directory, che la fine dell'archivio
 * indica comunque. Niente zip64: un APK non arriva a 4 GB.
 */
function voci(zip) {
  let fine = -1;
  for (let i = zip.length - 22; i >= Math.max(0, zip.length - 22 - 65535); i--) {
    if (zip.readUInt32LE(i) === 0x06054b50) {
      fine = i;
      break;
    }
  }
  if (fine < 0) throw new Error("non è uno zip: manca la fine della directory centrale");
  const quante = zip.readUInt16LE(fine + 10);
  let p = zip.readUInt32LE(fine + 16);
  const fuori = [];
  for (let n = 0; n < quante; n++) {
    if (zip.readUInt32LE(p) !== 0x02014b50) throw new Error("directory centrale rotta");
    const lunghezzaNome = zip.readUInt16LE(p + 28);
    fuori.push({
      nome: zip.toString("utf8", p + 46, p + 46 + lunghezzaNome),
      metodo: zip.readUInt16LE(p + 10),
      compressa: zip.readUInt32LE(p + 20),
      locale: zip.readUInt32LE(p + 42),
    });
    p += 46 + lunghezzaNome + zip.readUInt16LE(p + 30) + zip.readUInt16LE(p + 32);
  }
  return fuori;
}

/** I byte di una voce: memorizzata così com'è (0) o compressa (8). */
function estrai(zip, voce) {
  const p = voce.locale;
  if (zip.readUInt32LE(p) !== 0x04034b50) throw new Error(`${voce.nome}: intestazione locale rotta`);
  const inizio = p + 30 + zip.readUInt16LE(p + 26) + zip.readUInt16LE(p + 28);
  const dati = zip.subarray(inizio, inizio + voce.compressa);
  if (voce.metodo === 0) return dati;
  if (voce.metodo === 8) return zlib.inflateRawSync(dati);
  throw new Error(`${voce.nome}: compressione ${voce.metodo} sconosciuta`);
}

/** L'allineamento di ogni segmento `LOAD` di un ELF a 64 bit little-endian. */
function allineamentiLoad(elf) {
  if (elf.readUInt32BE(0) !== 0x7f454c46) throw new Error("non è un ELF");
  if (elf[4] !== 2 || elf[5] !== 1) throw new Error("non è un ELF a 64 bit little-endian");
  const inizio = Number(elf.readBigUInt64LE(0x20));
  const misura = elf.readUInt16LE(0x36);
  const quanti = elf.readUInt16LE(0x38);
  const fuori = [];
  for (let i = 0; i < quanti; i++) {
    const p = inizio + i * misura;
    if (elf.readUInt32LE(p) === 1) fuori.push(Number(elf.readBigUInt64LE(p + 48)));
  }
  return fuori;
}

/** Le librerie native: solo `arm64-v8a`, e ogni `LOAD` a 16 KB. */
function problemiPagine(apk) {
  const problemi = [];
  const so = voci(apk).filter((v) => v.nome.startsWith("lib/") && v.nome.endsWith(".so"));
  if (so.length === 0) problemi.push("nessuna libreria nativa in lib/: l'APK è senza Rymembra");
  for (const v of so) {
    const abi = v.nome.split("/")[1];
    if (abi !== ABI) problemi.push(`${v.nome}: l'ABI ${abi} non è quella che il nome dell'asset promette (${ABI})`);
    let allineamenti;
    try {
      allineamenti = allineamentiLoad(estrai(apk, v));
    } catch (e) {
      problemi.push(`${v.nome}: ${e.message}`);
      continue;
    }
    if (allineamenti.length === 0) problemi.push(`${v.nome}: nessun segmento LOAD`);
    for (const a of allineamenti) {
      if (a < PAGINA || a % PAGINA !== 0) {
        problemi.push(`${v.nome}: un segmento LOAD allineato a 0x${a.toString(16)} invece di 0x4000 (16 KB): l'NDK non è quello giusto`);
      }
    }
  }
  return problemi;
}

/* ── le uscite degli strumenti ────────────────────────────────────────── */

/** `aapt2 dump badging`: pacchetto, versione, codice, e niente debug. */
function problemiBadging(testo, { pacchetto, versione: v }) {
  const problemi = [];
  const riga = /^package: (.*)$/m.exec(testo);
  if (!riga) return ["aapt2 non ha descritto il pacchetto"];
  const campo = (nome) => new RegExp(`\\b${nome}='([^']*)'`).exec(riga[1])?.[1];
  if (campo("name") !== pacchetto) problemi.push(`il pacchetto è «${campo("name")}» invece di «${pacchetto}»`);
  if (campo("versionName") !== v) problemi.push(`versionName è «${campo("versionName")}» invece di «${v}»`);
  const codice = String(codiceAndroid(v));
  if (campo("versionCode") !== codice) problemi.push(`versionCode è «${campo("versionCode")}» invece di ${codice}`);
  // Un APK debuggable dà la WebView ispezionabile e i dati dell'app a `run-as`.
  if (/^application-debuggable\b/m.test(testo)) problemi.push("l'APK è debuggable: è una build di prova, non di rilascio");
  const native = /^native-code: (.*)$/m.exec(testo);
  if (native && native[1].trim() !== `'${ABI}'`) problemi.push(`native-code ${native[1].trim()} invece di '${ABI}'`);
  return problemi;
}

/**
 * `mapping.txt` di R8: ogni membro di `TENUTI` nella sua classe, col suo nome.
 * Una riga di membro è `    [a:b:]<ritorno> <nome>(<parametri>)[:c[:d]] -> <nome dopo R8>`;
 * le righe con un nome qualificato davanti sono quelle dei metodi incorporati.
 */
function problemiMapping(testo, tenuti = TENUTI) {
  const classi = new Map();
  let corrente = null;
  for (const riga of testo.split(/\r?\n/)) {
    const c = /^(\S+) -> (\S+):$/.exec(riga);
    if (c) {
      corrente = { dopo: c[2], membri: new Map() };
      classi.set(c[1], corrente);
      continue;
    }
    const m = /^\s+(?:\d+:\d+:)?(\S+ [^\s(]+\([^)]*\))(?::\d+(?::\d+)?)? -> (\S+)$/.exec(riga);
    if (m && corrente) {
      const [ritorno, firma] = m[1].split(" ");
      // Un nome con la classe davanti è un metodo incorporato in questo, non un suo membro.
      if (firma.slice(0, firma.indexOf("(")).includes(".")) continue;
      const chiave = `${ritorno} ${firma}`;
      if (!corrente.membri.has(chiave)) corrente.membri.set(chiave, m[2]);
    }
  }
  const problemi = [];
  for (const [classe, membro] of tenuti) {
    const c = classi.get(classe);
    if (!c) {
      problemi.push(`R8 ha tolto la classe ${classe}`);
      continue;
    }
    if (c.dopo !== classe) problemi.push(`R8 ha rinominato ${classe} in ${c.dopo}`);
    const nome = membro.slice(membro.indexOf(" ") + 1, membro.indexOf("("));
    const dopo = c.membri.get(membro);
    if (dopo === undefined) problemi.push(`R8 ha tolto ${classe}.${nome} (${membro})`);
    else if (dopo !== nome) problemi.push(`R8 ha rinominato ${classe}.${nome} in ${dopo}`);
  }
  return problemi;
}

/**
 * Le icone che Android Auto chiede per nome: ogni `auto_*.xml` di
 * `res/drawable`, in ordine. Lette dal progetto e non scritte qui, così
 * un'icona nuova presa per nome (che `keep.xml` tiene se comincia con
 * `auto_`) entra nel controllo da sé.
 */
function iconeDellAuto(cartella = DISEGNI) {
  return fs
    .readdirSync(cartella)
    .filter((f) => /^auto_[a-z0-9_]+\.xml$/.test(f))
    .map((f) => f.slice(0, -".xml".length))
    .sort();
}

/**
 * `aapt2 dump resources`: ogni icona di `nomi` è ancora nella tabella, come
 * `drawable/<nome>`. Una stringa con lo stesso nome (`string/auto_album`)
 * non conta: all'auto arriva l'indirizzo del disegno.
 */
function problemiRisorse(testo, nomi) {
  if (!/^Package name=/m.test(testo)) return ["aapt2 non ha descritto le risorse dell'APK"];
  const disegni = new Set([...testo.matchAll(/^\s*resource 0x[0-9a-f]+ drawable\/(\S+)\s*$/gm)].map((m) => m[1]));
  return nomi
    .filter((nome) => !disegni.has(nome))
    .map((nome) => `drawable/${nome} non c'è più: la riduzione delle risorse l'ha tolta, e Android Auto mostrerebbe la scheda senza icona (res/raw/keep.xml)`);
}

/** `apksigner verify --verbose --print-certs`: chi ha firmato, e come. */
function problemiFirma(testo, impronta) {
  const problemi = [];
  if (!/^Verifies\s*$/m.test(testo)) problemi.push("apksigner non dice «Verifies»");
  if (!/^Verified using v2 scheme \(APK Signature Scheme v2\): true$/m.test(testo)) {
    problemi.push("lo schema v2 non è verificato (minSdk 26: serve v2 o v3)");
  }
  const firmatari = /^Number of signers: (\d+)$/m.exec(testo);
  if (!firmatari || firmatari[1] !== "1") problemi.push(`i firmatari sono ${firmatari ? firmatari[1] : "(non detti)"} invece di uno`);
  const dn = /^Signer #1 certificate DN: (.*)$/m.exec(testo);
  if (!dn) problemi.push("apksigner non dice il certificato");
  else if (/CN=Android Debug/i.test(dn[1])) problemi.push("l'APK è firmato con la chiave di debug");
  const sha = /^Signer #1 certificate SHA-256 digest: ([0-9a-fA-F:]+)$/m.exec(testo);
  if (!sha) problemi.push("apksigner non dice l'impronta SHA-256 del certificato");
  else if (normalizzaImpronta(sha[1]) !== normalizzaImpronta(impronta)) {
    problemi.push(
      `il certificato è ${normalizzaImpronta(sha[1])}, non quello dei rilasci (${normalizzaImpronta(impronta)}): ` +
        "nessuna copia installata potrebbe aggiornarsi con questo APK",
    );
  }
  return problemi;
}

/**
 * Il `.apk.sig` di minisign: la stessa chiave dell'updater del computer, la
 * versione e il nome del file nel commento fidato. Gli identificativi e il
 * commento si leggono con le funzioni di `manifesto.js`.
 */
function problemiSig(sig, { pubkey, versione: v, file }) {
  const { identificativo, versioneFirmata } = require("./manifesto.js");
  const problemi = [];
  const dellaChiave = identificativo(pubkey, "plugins.updater.pubkey");
  const dellaFirma = identificativo(sig, `${file}.sig`);
  if (dellaChiave !== dellaFirma) {
    problemi.push(`${file}.sig è firmato con la chiave ${dellaFirma}, non con quella dell'updater (${dellaChiave})`);
  }
  const firmata = versioneFirmata(sig);
  if (firmata !== v) problemi.push(`${file}.sig copre la versione ${firmata ?? "(nessuna)"} invece della ${v}`);
  const testo = Buffer.from(sig, "base64").toString("utf8");
  const commento = /^trusted comment:(.*)$/m.exec(testo)?.[1] ?? "";
  const nome = commento.split("\t").find((c) => c.trim().startsWith("file:"));
  if (!nome || nome.trim().slice("file:".length) !== file) problemi.push(`${file}.sig è stato fatto per un altro file (${nome ? nome.trim() : "nessuno"})`);
  return problemi;
}

/* ── il comando ───────────────────────────────────────────────────────── */

/** Uno strumento di `ANDROID_BUILD_TOOLS`, con il suo esito. */
function lancia(nome, argomenti) {
  const cartella = process.env.ANDROID_BUILD_TOOLS;
  if (!cartella) throw new Error("ANDROID_BUILD_TOOLS non dice dove sono zipalign, aapt2 e apksigner");
  const windows = process.platform === "win32";
  const eseguibile = path.join(cartella, windows ? (nome === "apksigner" ? "apksigner.bat" : `${nome}.exe`) : nome);
  if (!fs.existsSync(eseguibile)) throw new Error(`manca ${eseguibile}`);
  // Un `.bat` su Windows passa da cmd.
  const esito = eseguibile.endsWith(".bat")
    ? spawnSync("cmd.exe", ["/d", "/c", eseguibile, ...argomenti], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 })
    : spawnSync(eseguibile, argomenti, { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  return { codice: esito.status, uscita: `${esito.stdout ?? ""}${esito.stderr ?? ""}` };
}

function zipallineato(apk) {
  const { codice, uscita } = lancia("zipalign", ["-c", "-P", "16", "-v", "4", apk]);
  return codice === 0 ? [] : [`zipalign -c -P 16: non allineato\n${uscita.split("\n").filter((r) => /BAD|fail/i.test(r)).slice(0, 10).join("\n")}`];
}

function argomenti(argv) {
  const [comando, ...resto] = argv;
  const fuori = { comando, file: null, mapping: null };
  for (let i = 0; i < resto.length; i++) {
    if (resto[i] === "--mapping") fuori.mapping = resto[++i] ?? null;
    else if (fuori.file === null) fuori.file = resto[i];
    else throw new Error(`argomento sconosciuto: ${resto[i]}`);
  }
  return fuori;
}

function main() {
  const a = argomenti(process.argv.slice(2));
  const v = versione();
  if (a.comando === "nome") {
    console.log(nomeAsset(v));
    return;
  }
  if ((a.comando !== "controlla" && a.comando !== "verifica") || !a.file) {
    console.error("uso: apk.js nome | controlla <apk> --mapping <mapping.txt> | verifica <apk-firmato>");
    process.exit(2);
  }
  const apk = fs.readFileSync(a.file);
  const problemi = [...zipallineato(a.file), ...problemiPagine(apk)];
  if (a.comando === "controlla") {
    if (!a.mapping) {
      console.error("controlla vuole --mapping: senza, R8 potrebbe aver tolto il ponte senza che nessuno lo veda");
      process.exit(2);
    }
    const badging = lancia("aapt2", ["dump", "badging", a.file]);
    if (badging.codice !== 0) problemi.push(`aapt2 dump badging non è riuscito:\n${badging.uscita}`);
    else problemi.push(...problemiBadging(badging.uscita, { pacchetto: configurazione().identifier, versione: v }));
    const risorse = lancia("aapt2", ["dump", "resources", a.file]);
    if (risorse.codice !== 0) problemi.push(`aapt2 dump resources non è riuscito:\n${risorse.uscita}`);
    else problemi.push(...problemiRisorse(risorse.uscita, iconeDellAuto()));
    problemi.push(...problemiMapping(fs.readFileSync(a.mapping, "utf8")));
  } else {
    const firma = lancia("apksigner", ["verify", "--verbose", "--print-certs", a.file]);
    if (firma.codice !== 0) problemi.push(`apksigner verify non è riuscito:\n${firma.uscita}`);
    else problemi.push(...problemiFirma(firma.uscita, improntaInchiodata()));
    const sig = `${a.file}.sig`;
    if (!fs.existsSync(sig)) problemi.push(`manca ${path.basename(sig)}: senza, l'app non può verificare l'APK come fa il computer`);
    else {
      const pubkey = configurazione().plugins?.updater?.pubkey ?? "";
      problemi.push(...problemiSig(fs.readFileSync(sig, "utf8").trim(), { pubkey, versione: v, file: path.basename(a.file) }));
    }
  }
  if (problemi.length) {
    console.error(`${path.basename(a.file)}: ${problemi.length} problemi\n` + problemi.map((p) => `  - ${p}`).join("\n"));
    process.exit(1);
  }
  console.log(`${path.basename(a.file)}: ${a.comando === "controlla" ? "pagine, manifesto, icone di Android Auto e R8" : "allineamento, certificato e .sig"} in ordine.`);
}

if (require.main === module) main();

module.exports = {
  ABI,
  TENUTI,
  nomeAsset,
  normalizzaImpronta,
  improntaInchiodata,
  voci,
  estrai,
  allineamentiLoad,
  problemiPagine,
  problemiBadging,
  problemiMapping,
  problemiRisorse,
  iconeDellAuto,
  problemiFirma,
  problemiSig,
  argomenti,
};
