/*
 * L'installatore NSIS, letto come testo.
 *
 * Il modello di Tauri e `makensis` non girano nelle prove: quel che si può
 * guardare senza costruire è la configurazione (`tauri.conf.json`) e il gancio
 * (`installer/precedente.nsh`), ed è lì che stavano i due difetti di questo
 * file: un installer solo in inglese, e un gancio che disinstallava qualunque
 * programma registrato come «Aether», di chiunque fosse. Che il tutto compili
 * lo dice `tauri build` (in `release.yml`, e a mano con `--bundles nsis`).
 *
 *   node --test "strumenti/*.test.js"
 */
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const REPO = path.dirname(__dirname);
const CONF = JSON.parse(fs.readFileSync(path.join(REPO, "apps/desktop/src-tauri/tauri.conf.json"), "utf8"));
const GANCIO = fs.readFileSync(path.join(REPO, "apps/desktop/src-tauri/installer/precedente.nsh"), "utf8");

/** Il gancio senza i commenti (`;` a inizio riga, dopo gli spazi). */
const CODICE = GANCIO.split(/\r?\n/)
  .filter((r) => !/^\s*;/.test(r))
  .join("\n");

/** Il nome che NSIS dà alla lingua di ogni file di `apps/desktop/src/lingue/`. */
const NSIS = { en: "English", it: "Italian" };

/** Gli id di lingua di Windows: le `LangString` del gancio li scrivono in cifre (vedi lì perché). */
const ID = { English: "1033", Italian: "1040" };

test("l'installer parla le lingue dell'app, l'inglese per primo", () => {
  const lingue = fs
    .readdirSync(path.join(REPO, "apps/desktop/src/lingue"))
    .filter((f) => f.endsWith(".json"))
    .map((f) => f.slice(0, -5));
  const dichiarate = CONF.bundle?.windows?.nsis?.languages;
  assert.ok(Array.isArray(dichiarate), "bundle.windows.nsis.languages manca: l'installer parla solo inglese");
  // Senza il selettore NSIS sceglie la lingua di Windows, e se non c'è la prima.
  assert.equal(dichiarate[0], "English");
  for (const l of lingue) {
    assert.ok(NSIS[l], `lingua dell'app «${l}» senza un nome NSIS in questa prova`);
    assert.ok(dichiarate.includes(NSIS[l]), `l'installer non parla ${NSIS[l]}`);
  }
  assert.equal(CONF.bundle.windows.nsis.displayLanguageSelector ?? false, false);
});

test("i testi del gancio sono LangString in ogni lingua dell'installer, non frasi cablate", () => {
  // Ogni MessageBox e ogni DetailPrint mostra una LangString e basta.
  const mostrati = [...CODICE.matchAll(/^\s*(MessageBox\s+\S+|DetailPrint)\s+(.*)$/gm)];
  assert.ok(mostrati.length >= 6, "il gancio non mostra più niente?");
  for (const m of mostrati) {
    const testo = m[1].startsWith("MessageBox") ? m[2].split(/\s+ID(?:OK|CANCEL)\b/)[0].trim() : m[2].trim();
    assert.match(testo, /^"\$\(precedente\w+\)"$/, `testo cablato: ${m[0].trim()}`);
  }
  const usate = new Set([...CODICE.matchAll(/\$\((precedente\w+)\)/g)].map((m) => m[1]));
  const definite = [...CODICE.matchAll(/^\s*LangString\s+(\w+)\s+(\S+)\s+"/gm)];
  for (const nome of usate) {
    for (const lingua of CONF.bundle.windows.nsis.languages) {
      assert.ok(
        definite.some((d) => d[1] === nome && d[2] === ID[lingua]),
        `«${nome}» non ha la LangString per ${lingua} (${ID[lingua]})`,
      );
    }
  }
  for (const d of definite) assert.ok(usate.has(d[1]), `«${d[1]}» definita e mai mostrata`);
});

test("il primo nome di Aether in ogni testo dice che è il nome di prima di Rymembra", () => {
  // I testi che chi installa vede in una finestra: quelli dei MessageBox. I
  // DetailPrint stanno nei dettagli, dopo che la finestra l'ha già detto.
  const finestre = [...CODICE.matchAll(/^\s*MessageBox\s+\S+\s+"\$\((\w+)\)"/gm)].map((m) => m[1]);
  assert.ok(finestre.length > 0);
  const testi = [...CODICE.matchAll(/^\s*LangString\s+(\w+)\s+(\d+)\s+"(.*)"\s*$/gm)];
  for (const [, nome, id, testo] of testi.filter((t) => finestre.includes(t[1]))) {
    const spiega = id === "1040" ? "${PRECEDENTE_NOME} (il nome di prima di Rymembra)" : "${PRECEDENTE_NOME} (Rymembra’s previous name)";
    const primo = testo.indexOf("${PRECEDENTE_NOME}");
    assert.ok(primo >= 0 && testo.startsWith(spiega, primo), `${nome} ${id}: ${testo}`);
  }
});

test("l'aggancio tocca solo l'Aether di Federico Baratti, nella sua cartella", () => {
  assert.match(CODICE, /!define PRECEDENTE_EDITORE "Federico Baratti"/);
  const prima = CODICE.slice(0, CODICE.indexOf("ExecWait"));
  assert.ok(CODICE.includes("ExecWait"), "il gancio non disinstalla più niente?");
  // Editore e nome della voce, confrontati prima di qualunque esecuzione.
  assert.match(prima, /ReadRegStr \$R\d HKCU "\$\{PRECEDENTE_CHIAVE_VOCE\}" "Publisher"/);
  assert.match(prima, /ReadRegStr \$R\d HKCU "\$\{PRECEDENTE_CHIAVE_VOCE\}" "DisplayName"/);
  assert.match(prima, /\$\{If\} \$R\d != "\$\{PRECEDENTE_EDITORE\}"/);
  assert.match(prima, /!= "\$\{PRECEDENTE_NOME\}"/);
  // La cartella della voce deve essere quella della chiave produttore\prodotto,
  // e contenere il disinstallatore e l'eseguibile di Aether.
  assert.match(prima, /ReadRegStr \$R\d HKCU "\$\{PRECEDENTE_CHIAVE_CARTELLA\}" ""/);
  assert.match(prima, /\$\{FileExists\} "\$R1\\uninstall\.exe"/);
  assert.match(prima, /\$\{FileExists\} "\$R1\\\$\{PRECEDENTE_EXE\}"/);
  // Niente più ripiego su %LOCALAPPDATA%\Aether: è proprio lì che sta un
  // programma «Aether» di un altro editore installato con un modello NSIS.
  assert.ok(!CODICE.includes("$LOCALAPPDATA\\${PRECEDENTE_NOME}"), "il ripiego su %LOCALAPPDATA%\\Aether c'è ancora");
  // Il processo da aspettare è quello noto, non quello che dice la voce.
  assert.ok(!/"MainBinaryName"/.test(CODICE), "il nome del processo viene ancora dalla voce");
  // I collegamenti di Aether si guardano solo quando la voce è riconosciuta:
  // altrimenti in /UPDATE comparirebbero collegamenti di Rymembra mai chiesti.
  assert.ok(CODICE.indexOf('"Publisher"') < CODICE.indexOf("StrCpy $PrecedenteMenuStart 1"));
});

/** Il corpo di una macro del gancio, senza i commenti. */
function macro(nome) {
  const da = CODICE.indexOf(`!macro ${nome}`);
  assert.ok(da >= 0, `il gancio non ha la macro ${nome}`);
  return CODICE.slice(da, CODICE.indexOf("!macroend", da));
}

/** Il comando del protocollo, com'è scritto in NSIS: `"<exe>" "%1"`, con `$\"` per le virgolette. */
const COMANDO_NSIS = '"$\\"$INSTDIR\\${MAINBINARYNAME}.exe$\\" $\\"%1$\\""';

test("il protocollo rymembra:// si registra a ogni installazione, per l'utente corrente", () => {
  const dopo = macro("NSIS_HOOK_POSTINSTALL");
  // HKCU e non HKLM: l'installer è «per l'utente corrente», senza amministratore.
  assert.ok(!dopo.includes("HKLM"), "il protocollo non si scrive in HKLM");
  assert.ok(dopo.includes('WriteRegStr HKCU "Software\\Classes\\rymembra" "URL Protocol" ""'));
  // Il comando passa il link come **un** argomento, fra virgolette: `"<exe>" "%1"`.
  assert.ok(
    dopo.includes(`WriteRegStr HKCU "Software\\Classes\\rymembra\\shell\\open\\command" "" ${COMANDO_NSIS}`),
    "il comando del protocollo non è `\"<exe>\" \"%1\"`",
  );
  // Anche in modalità aggiornamento: un percorso cambiato deve riscriversi. La
  // scrittura sta fuori dal blocco dei collegamenti, che è solo di /UPDATE.
  const scrittura = dopo.indexOf('"URL Protocol"');
  assert.ok(scrittura >= 0 && scrittura < dopo.indexOf("${If} $UpdateMode = 1"));
});

test("la disinstallazione toglie il protocollo solo se è il nostro, e l'avvio, ma non in /UPDATE", () => {
  const dopo = macro("NSIS_HOOK_POSTUNINSTALL");
  assert.match(dopo, /^!macro NSIS_HOOK_POSTUNINSTALL\s+\$\{If\} \$UpdateMode <> 1/);
  // Il protocollo si toglie se il comando punta a questa cartella, come fa il
  // modello coi suoi (`deep_link_protocols`).
  assert.ok(dopo.includes('ReadRegStr $R7 HKCU "Software\\Classes\\rymembra\\shell\\open\\command" ""'));
  assert.ok(dopo.includes(`\${If} $R7 == ${COMANDO_NSIS}`));
  const lettura = dopo.indexOf("$R7 ==");
  const cancella = dopo.indexOf('DeleteRegKey HKCU "Software\\Classes\\rymembra"');
  assert.ok(lettura >= 0 && cancella > lettura, "il protocollo si toglie senza guardare di chi è");
  // L'avvio con Windows: il valore col nome del prodotto, e lo spento di Windows.
  assert.ok(
    dopo.includes('DeleteRegValue HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "${PRODUCTNAME}"'),
  );
  assert.ok(
    dopo.includes(
      'DeleteRegValue HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run" "${PRODUCTNAME}"',
    ),
  );
  assert.ok(!dopo.includes("HKLM"));
});

test("il valore Run dell'avvio si chiama come il prodotto: è così che lo toglie il modello di Tauri", () => {
  // Il modello toglie `Run\${PRODUCTNAME}` e nient'altro (installer.nsi del CLI
  // 2.12.0, «Removes the Autostart entry»): il nome che il programma scrive
  // deve essere `productName`, lettera per lettera.
  const avvio = fs.readFileSync(path.join(REPO, "apps/desktop/rymembra-barra/src/avvio.rs"), "utf8");
  const nome = /pub const NOME_VALORE: &str = "([^"]+)";/.exec(avvio);
  assert.ok(nome, "NOME_VALORE non si trova in rymembra-barra/src/avvio.rs");
  assert.equal(nome[1], CONF.productName);
  // E l'argomento che il registro passa è quello che il programma capisce.
  assert.ok(avvio.includes('pub const ARGOMENTO_AVVIO_AUTOMATICO: &str = "--avvio-automatico";'));
});
