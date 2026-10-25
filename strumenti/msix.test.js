/*
 * Il pacchetto MSIX, provato prima di scoprire che il Store lo rifiuta.
 *
 * Un pacchetto sbagliato si scopre in due modi: `makeappx` che dice di no
 * (bene, subito) o il Store che lo rifiuta dopo il caricamento (male, dopo
 * minuti e con un messaggio che non nomina il campo). Queste prove coprono la
 * parte che non richiede il Windows SDK: l'identità, la versione, il modello
 * del manifesto e i loghi che il manifesto nomina.
 *
 *   node --test "strumenti/*.test.js"
 */
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const {
  MARCATORE,
  SEGNAPOSTI,
  controllaIdentita,
  dallaMaggiore,
  escapiXml,
  riempi,
  segnapostiDel,
  senzaPacchettiDiRisorse,
  versioneMsix,
} = require("./msix.js");

const MSIX = path.join(__dirname, "..", "apps/desktop/src-tauri/msix");
const MODELLO = fs.readFileSync(path.join(MSIX, "AppxManifest.xml"), "utf8");

test("la versione del pacchetto è quella di Tauri con uno zero in coda", () => {
  assert.equal(versioneMsix("3.0.2"), "3.0.2.0");
  assert.equal(versioneMsix("10.20.30"), "10.20.30.0");
});

test("una versione che non è di tre numeri non diventa un pacchetto", () => {
  assert.throws(() => versioneMsix("3.0"), /tre numeri/);
  assert.throws(() => versioneMsix("3.0.2-beta"), /tre numeri/);
  assert.throws(() => versioneMsix("3.0.70000"), /65535/);
});

test("un'identità vera passa", () => {
  const guasti = controllaIdentita({
    nome: "12345FedericoBaratti.Rymembra",
    editore: "CN=A1B2C3D4-0000-1111-2222-ABCDEFABCDEF",
    nomeEditore: "Federico Baratti",
  });
  assert.deepEqual(guasti, []);
});

test("i valori dell'esempio non passano: sono segnaposto, non l'identità", () => {
  const guasti = controllaIdentita({
    nome: "12345NomeEditore.Rymembra",
    editore: "CN=00000000-0000-0000-0000-000000000000",
    nomeEditore: "Federico Baratti",
  });
  assert.equal(guasti.length, 2);
  assert.match(guasti[0], /esempio/);
  assert.match(guasti[1], /esempio/);
});

test("ogni campo dell'identità guasto dà il suo avviso", () => {
  const guasti = controllaIdentita({ nome: "no", editore: "Federico", nomeEditore: " " });
  assert.equal(guasti.length, 3);
  assert.match(guasti[0], /^nome:/);
  assert.match(guasti[1], /^editore:/);
  assert.match(guasti[2], /^nomeEditore:/);
  // Senza niente, lo stesso: non un'eccezione sul `.test` di un `undefined`.
  assert.equal(controllaIdentita({}).length, 3);
});

test("i segnaposto si riempiono, e quello che manca ferma tutto", () => {
  assert.equal(riempi("a {{X}} b {{X}}", { X: "1" }), "a 1 b 1");
  assert.throws(() => riempi("{{X}} {{Y}}", { X: "1" }), /\{\{Y\}\}/);
  // Un valore che porta con sé un segnaposto non deve passare inosservato.
  assert.throws(() => riempi("{{X}}", { X: "{{Y}}" }), /rimasto \{\{Y\}\}/);
});

test("nei valori il testo XML è escapato", () => {
  assert.equal(escapiXml(`Rossi & "Figli" <srl>`), "Rossi &amp; &quot;Figli&quot; &lt;srl&gt;");
});

test("il modello del manifesto usa i segnaposto che lo script riempie", () => {
  // Né uno in più — il manifesto uscirebbe con `{{...}}` dentro — né uno in
  // meno: lo script riempirebbe un valore che nessuno legge.
  assert.deepEqual(segnapostiDel(MODELLO).sort(), [...SEGNAPOSTI].sort());
});

test("il manifesto riempito è XML con le parti che il Store si aspetta", () => {
  const fuori = riempi(MODELLO, {
    NOME: "12345FedericoBaratti.Rymembra",
    EDITORE: "CN=A1B2C3D4-0000-1111-2222-ABCDEFABCDEF",
    VERSIONE: "3.0.2.0",
    NOME_EDITORE: "Federico Baratti",
    DESCRIZIONE: "Un lettore musicale",
  });
  assert.match(fuori, /<Identity\s+Name="12345FedericoBaratti\.Rymembra"/);
  assert.match(fuori, /Publisher="CN=A1B2C3D4-0000-1111-2222-ABCDEFABCDEF"/);
  assert.match(fuori, /Version="3\.0\.2\.0"/);
  assert.match(fuori, /ProcessorArchitecture="x64"/);
  assert.match(fuori, /EntryPoint="Windows\.FullTrustApplication"/);
  assert.match(fuori, /<rescap:Capability Name="runFullTrust"\s*\/>/);
  // Il programma che il pacchetto avvia è quello che lo script copia.
  assert.match(fuori, /Executable="rymembra\.exe"/);
  // Un commento XML non può contenere due trattini di fila: il sistema rifiuta
  // l'intero manifesto, e il messaggio non dice dove.
  for (const commento of fuori.matchAll(/<!--([\s\S]*?)-->/g)) {
    assert.ok(!commento[1].includes("--"), "un commento contiene «--»");
  }
});

test("ogni logo che il manifesto nomina esiste, almeno alla scala 100", () => {
  // `makeappx` lo controllerebbe, ma ha bisogno del Windows SDK; qui basta il
  // file system. Il nome nel manifesto è senza scala: i file sono
  // `Nome.scale-100.png` e fratelli, e `resources.pri` li lega.
  const nominati = [...MODELLO.matchAll(/Assets\\([A-Za-z0-9]+)\.png/g)].map((m) => m[1]);
  assert.ok(nominati.length >= 3, "il manifesto dovrebbe nominare almeno tre loghi");
  for (const nome of new Set(nominati)) {
    for (const scala of [100, 200, 400]) {
      const file = path.join(MSIX, "Assets", `${nome}.scale-${scala}.png`);
      assert.ok(fs.existsSync(file), `manca ${path.relative(MSIX, file)}`);
    }
  }
});

test("la configurazione di makepri perde i pacchetti di risorse e tiene l'indice", () => {
  const xml = [
    '<resources targetOsVersion="10.0.0" majorVersion="1">',
    "\t<packaging>",
    '\t\t<autoResourcePackage qualifier="Scale"/>',
    "\t</packaging>",
    '\t<index root="\\" startIndexAt="\\">',
    "\t</index>",
    "</resources>",
    "",
  ].join("\n");
  const fuori = senzaPacchettiDiRisorse(xml);
  assert.ok(!fuori.includes("packaging"));
  assert.ok(!fuori.includes("autoResourcePackage"));
  assert.ok(fuori.includes('<index root="\\" startIndexAt="\\">'));
  // Senza il blocco, non cambia niente.
  assert.equal(senzaPacchettiDiRisorse(fuori), fuori);
});

test("le versioni del Windows SDK si ordinano dalla maggiore", () => {
  const versioni = ["10.0.19041.0", "10.0.26100.0", "10.0.9200.0", "10.0.22621.0"];
  assert.deepEqual(versioni.sort(dallaMaggiore), [
    "10.0.26100.0",
    "10.0.22621.0",
    "10.0.19041.0",
    "10.0.9200.0",
  ]);
});

test("il marcatore del Store ha il nome che il programma cerca", () => {
  // Il confronto inverso — `aggiornamenti.rs` che legge questo file — sta
  // nelle prove di quel modulo. Qui basta che il nome non sia vuoto e non abbia
  // percorso: il programma lo cerca accanto all'eseguibile.
  assert.match(MARCATORE, /^[a-z0-9.-]+$/);
});
