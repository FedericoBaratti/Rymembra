/*
 * Il pacchetto MSIX per il Microsoft Store.
 *
 *   node strumenti/msix.js                       costruisce il .msix
 *   node strumenti/msix.js --solo-cartella       prepara la cartella e basta
 *   node strumenti/msix.js --exe percorso\rymembra.exe
 *
 * # Perché esiste
 *
 * L'installer NSIS non può andare nel Store come pacchetto MSIX, e come EXE
 * richiederebbe una firma di codice con un certificato di una CA fidata. Un
 * pacchetto MSIX invece **lo firma il Store** dopo la certificazione: niente
 * certificato, niente SmartScreen. Il prezzo è che il pacchetto va costruito, e
 * Tauri non sa farlo — questo script è quel pezzo.
 *
 * # Cosa mette dentro
 *
 *   rymembra.exe            l'eseguibile di `tauri build --no-bundle`
 *   rymembra-store.txt      il marcatore: dice al programma che a tenerlo
 *                           aggiornato è il Store (vedi `aggiornamenti.rs`)
 *   LICENSE.txt, ...        le stesse risorse che mette l'installer NSIS, lette
 *                           da `bundle.resources` e non ricopiate qui
 *   Assets\                 i loghi (`genera-icone.ps1`)
 *   AppxManifest.xml        il modello in `msix/`, con l'identità riempita
 *   resources.pri           l'indice dei loghi, perché il sistema li trovi
 *                           alle scale e alle misure che chiede
 *
 * # L'eseguibile non è quello dell'installer
 *
 * `tauri bundle` e `tauri build` con un bersaglio NSIS **scrivono dentro
 * l'eseguibile** di che tipo di installer è, e l'updater lo legge per sapere
 * come aggiornarsi. In un pacchetto MSIX quel dato sarebbe falso. Per questo
 * l'eseguibile qui viene da `tauri build --no-bundle`, che non lo tocca.
 *
 * # L'identità non è un segreto
 *
 * Nome, editore e nome dell'editore stanno in Partner Center → Gestione
 * prodotto → Identità del prodotto, e finiscono in chiaro nel manifesto di ogni
 * pacchetto che si scarica. Stanno quindi in `msix/identita.json`, nel
 * repository, e si possono passare anche da riga di comando (`--nome`,
 * `--editore`, `--nome-editore`) o con `MSIX_NOME`, `MSIX_EDITORE`,
 * `MSIX_NOME_EDITORE`. Devono coincidere **esattamente** con quelli del Store: un
 * pacchetto con un editore diverso viene rifiutato al caricamento.
 *
 * # La versione
 *
 * Quella di `tauri.conf.json` con uno zero in coda: `3.0.2` → `3.0.2.0`. Il
 * Store riserva a sé la quarta cifra, e vuole che quella dei pacchetti
 * sottoposti sia zero. Ogni invio deve avere una versione più alta del
 * precedente.
 */
const fs = require("fs");
const path = require("path");
const { spawnSync } = require("child_process");

const REPO = path.dirname(__dirname);
const TAURI_DIR = path.join(REPO, "apps/desktop/src-tauri");
const TAURI_CONF = path.join(TAURI_DIR, "tauri.conf.json");
const MSIX_DIR = path.join(TAURI_DIR, "msix");

/**
 * Il file che dice al programma di essere dentro il Store.
 *
 * Lo stesso nome sta in `apps/desktop/src-tauri/src/aggiornamenti.rs`
 * (`MARCATORE_STORE`), e una prova in quel file legge **questo** per
 * confrontarli: nessun compilatore guarda dentro uno script.
 */
const MARCATORE = "rymembra-store.txt";

const TESTO_MARCATORE =
  "Questa copia di Rymembra è stata installata dal Microsoft Store.\n" +
  "Il Store la tiene aggiornata: Rymembra non controlla gli aggiornamenti da sé.\n" +
  "\n" +
  "This copy of Rymembra was installed from the Microsoft Store.\n" +
  "The Store keeps it up to date: Rymembra does not check for updates by itself.\n";

/** I segnaposto del manifesto: né uno in più né uno in meno di questi. */
const SEGNAPOSTI = ["NOME", "EDITORE", "VERSIONE", "NOME_EDITORE", "DESCRIZIONE"];

function basta(messaggio) {
  console.error(`msix: ${messaggio}`);
  process.exit(1);
}

// ── le parti pure ───────────────────────────────────────────────────────────

/** `3.0.2` → `3.0.2.0`. Il Store vuole la quarta cifra a zero. */
function versioneMsix(versione) {
  const m = /^(\d+)\.(\d+)\.(\d+)$/.exec(versione);
  if (!m) throw new Error(`«${versione}» non è una versione di tre numeri`);
  for (const parte of m.slice(1)) {
    // Ogni cifra di un `Version` di pacchetto sta in sedici bit.
    if (Number(parte) > 65535) throw new Error(`«${versione}»: ${parte} supera 65535`);
  }
  return `${versione}.0`;
}

/** Il testo di un valore dentro un attributo o un elemento XML. */
function escapiXml(testo) {
  return String(testo)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/**
 * Quel che non va nell'identità, una frase per difetto; vuoto se va bene.
 *
 * Le regole sono quelle di Windows per `Identity/@Name` e `@Publisher`, più una:
 * i valori di `identita.esempio.json` non passano, perché un pacchetto costruito
 * con quelli verrebbe rifiutato dal Store e non c'è modo di accorgersene prima
 * di caricarlo.
 */
function controllaIdentita({ nome, editore, nomeEditore }) {
  const guasti = [];
  if (typeof nome !== "string" || !/^[A-Za-z0-9][-.A-Za-z0-9]{2,49}$/.test(nome)) {
    guasti.push(
      "nome: da 3 a 50 caratteri fra lettere, cifre, punto e trattino " +
        "(Package/Identity/Name)",
    );
  } else if (nome === "12345NomeEditore.Rymembra") {
    guasti.push("nome: è quello dell'esempio, non quello di Partner Center");
  }
  if (typeof editore !== "string" || !/^CN=.+/.test(editore)) {
    guasti.push("editore: deve cominciare con «CN=» (Package/Identity/Publisher)");
  } else if (editore.includes("00000000-0000-0000-0000-000000000000")) {
    guasti.push("editore: è quello dell'esempio, non quello di Partner Center");
  }
  if (typeof nomeEditore !== "string" || nomeEditore.trim() === "") {
    guasti.push("nomeEditore: vuoto (Package/Properties/PublisherDisplayName)");
  }
  return guasti;
}

/**
 * Riempie i segnaposto `{{NOME}}` del modello. I valori vanno già escapati.
 *
 * Fallisce se un segnaposto del modello non ha un valore, **e** se resta una
 * parola fra doppie graffe dopo la sostituzione: un manifesto con `{{VERSIONE}}`
 * dentro non si installa, e l'errore che ne esce non dice quale.
 */
function riempi(modello, valori) {
  const fuori = modello.replace(/\{\{([A-Z_]+)\}\}/g, (_, chiave) => {
    if (!(chiave in valori)) throw new Error(`il modello chiede {{${chiave}}} e nessuno la dà`);
    return valori[chiave];
  });
  const resta = /\{\{([A-Z_]+)\}\}/.exec(fuori);
  if (resta) throw new Error(`nel manifesto è rimasto {{${resta[1]}}}`);
  return fuori;
}

/**
 * La configurazione di `makepri` senza i «pacchetti di risorse».
 *
 * Di serie `makepri createconfig` vi mette un blocco `<packaging>` che spezza
 * le risorse per lingua, scala e livello DirectX in file a parte
 * (`resources.scale-200.pri`, ...), pensati per diventare pacchetti separati. In
 * un `.msix` solo quei file sono pesi morti, e peggio: i loghi alle scale 200 e
 * 400 finirebbero là dentro e il sistema non li troverebbe più.
 */
function senzaPacchettiDiRisorse(xml) {
  return xml.replace(/[ \t]*<packaging>[\s\S]*?<\/packaging>\s*\n?/, "");
}

/** I segnaposto che un modello usa, senza ripetizioni. */
function segnapostiDel(modello) {
  return [...new Set([...modello.matchAll(/\{\{([A-Z_]+)\}\}/g)].map((m) => m[1]))];
}

/** Confronta due versioni di Windows SDK come `10.0.26100.0`, dalla maggiore. */
function dallaMaggiore(a, b) {
  const x = a.split(".").map(Number);
  const y = b.split(".").map(Number);
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    const d = (y[i] ?? 0) - (x[i] ?? 0);
    if (d !== 0) return d;
  }
  return 0;
}

/**
 * Dove sta `makeappx.exe` o `makepri.exe`.
 *
 * Prima la variabile d'ambiente (`MAKEAPPX`, `MAKEPRI`), poi il Windows SDK più
 * recente fra quelli installati. Il SDK sta su `windows-latest` di GitHub e, di
 * solito, su qualunque computer con Visual Studio o con il suo installer.
 */
function trovaStrumento(nome) {
  const daAmbiente = process.env[nome.toUpperCase()];
  if (daAmbiente) {
    if (!fs.existsSync(daAmbiente)) basta(`${nome.toUpperCase()}=${daAmbiente} non esiste`);
    return daAmbiente;
  }
  const radice = path.join(
    process.env["ProgramFiles(x86)"] ?? "C:\\Program Files (x86)",
    "Windows Kits",
    "10",
    "bin",
  );
  if (fs.existsSync(radice)) {
    const versioni = fs
      .readdirSync(radice)
      .filter((v) => /^\d+\.\d+\.\d+\.\d+$/.test(v))
      .sort(dallaMaggiore);
    for (const v of versioni) {
      const trovato = path.join(radice, v, "x64", `${nome}.exe`);
      if (fs.existsSync(trovato)) return trovato;
    }
  }
  return basta(
    `non trovo ${nome}.exe. Serve il Windows SDK (winget install Microsoft.WindowsSDK.10.0.26100), ` +
      `oppure ${nome.toUpperCase()}=<percorso> nell'ambiente.`,
  );
}

// ── il lavoro ───────────────────────────────────────────────────────────────

function leggiOpzioni(argv) {
  const opzioni = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--solo-cartella") opzioni.soloCartella = true;
    else if (["--exe", "--uscita", "--nome", "--editore", "--nome-editore"].includes(a)) {
      const valore = argv[++i];
      if (valore === undefined) basta(`${a} vuole un valore`);
      opzioni[a.slice(2).replace(/-(\w)/g, (_, c) => c.toUpperCase())] = valore;
    } else basta(`opzione sconosciuta: ${a}`);
  }
  return opzioni;
}

function leggiIdentita(opzioni) {
  let file = {};
  const percorso = path.join(MSIX_DIR, "identita.json");
  if (fs.existsSync(percorso)) file = JSON.parse(fs.readFileSync(percorso, "utf8"));
  return {
    nome: opzioni.nome ?? process.env.MSIX_NOME ?? file.nome,
    editore: opzioni.editore ?? process.env.MSIX_EDITORE ?? file.editore,
    nomeEditore: opzioni.nomeEditore ?? process.env.MSIX_NOME_EDITORE ?? file.nomeEditore,
  };
}

function esegui(programma, argomenti) {
  const r = spawnSync(programma, argomenti, { encoding: "utf8" });
  if (r.error) basta(`${path.basename(programma)}: ${r.error.message}`);
  if (r.status !== 0) {
    basta(
      `${path.basename(programma)} è uscito con ${r.status}\n${(r.stdout ?? "").trim()}\n${(r.stderr ?? "").trim()}`,
    );
  }
  return r.stdout ?? "";
}

function main() {
  const opzioni = leggiOpzioni(process.argv.slice(2));

  const conf = JSON.parse(fs.readFileSync(TAURI_CONF, "utf8"));
  const versione = versioneMsix(conf.version);

  const identita = leggiIdentita(opzioni);
  const guasti = controllaIdentita(identita);
  if (guasti.length > 0) {
    basta(
      "l'identità del pacchetto non è a posto:\n  - " +
        guasti.join("\n  - ") +
        "\n\nI tre valori stanno in Partner Center → l'app MSIX → Gestione prodotto →\n" +
        "Identità del prodotto. Si scrivono in apps/desktop/src-tauri/msix/identita.json\n" +
        "(vedi identita.esempio.json) o si passano con --nome, --editore, --nome-editore.",
    );
  }

  const exe = path.resolve(opzioni.exe ?? path.join(REPO, "target/release/rymembra.exe"));
  if (!fs.existsSync(exe)) {
    basta(
      `manca ${exe}\nSi costruisce con: cd apps/desktop && npx tauri build --no-bundle -- --locked`,
    );
  }

  const uscita = path.resolve(opzioni.uscita ?? path.join(REPO, "target/msix"));
  const cartella = path.join(uscita, "cartella");
  fs.rmSync(uscita, { recursive: true, force: true });
  fs.mkdirSync(cartella, { recursive: true });

  // L'eseguibile, con il nome che il manifesto dichiara.
  fs.copyFileSync(exe, path.join(cartella, "rymembra.exe"));
  fs.writeFileSync(path.join(cartella, MARCATORE), TESTO_MARCATORE);

  // Le risorse dell'installer, dalla stessa tabella che usa NSIS: aggiungerne
  // una a `bundle.resources` la porta qui senza toccare questo file.
  for (const [origine, destinazione] of Object.entries(conf.bundle?.resources ?? {})) {
    const da = path.resolve(TAURI_DIR, origine);
    if (!fs.existsSync(da)) basta(`bundle.resources: manca ${da}`);
    fs.copyFileSync(da, path.join(cartella, destinazione));
  }

  fs.cpSync(path.join(MSIX_DIR, "Assets"), path.join(cartella, "Assets"), { recursive: true });

  const modello = fs.readFileSync(path.join(MSIX_DIR, "AppxManifest.xml"), "utf8");
  const attesi = [...SEGNAPOSTI].sort().join(",");
  const trovati = segnapostiDel(modello).sort().join(",");
  if (attesi !== trovati) {
    basta(`il modello usa {{${trovati}}}, lo script ne riempie {{${attesi}}}`);
  }
  const manifesto = riempi(modello, {
    NOME: escapiXml(identita.nome),
    EDITORE: escapiXml(identita.editore),
    VERSIONE: versione,
    NOME_EDITORE: escapiXml(identita.nomeEditore),
    DESCRIZIONE: escapiXml(conf.bundle?.shortDescription ?? "Rymembra"),
  });
  fs.writeFileSync(path.join(cartella, "AppxManifest.xml"), manifesto);

  // L'indice delle risorse. Senza, i loghi con la scala nel nome
  // (`Square44x44Logo.scale-200.png`) non sarebbero trovati dal sistema, e il
  // manifesto — che li chiama per il nome senza scala — non passerebbe la
  // validazione di `makeappx`.
  const makepri = trovaStrumento("makepri");
  const configurazione = path.join(uscita, "priconfig.xml");
  esegui(makepri, ["createconfig", "/cf", configurazione, "/dq", "en-US", "/o"]);
  fs.writeFileSync(
    configurazione,
    senzaPacchettiDiRisorse(fs.readFileSync(configurazione, "utf8")),
  );
  esegui(makepri, [
    "new",
    "/pr",
    cartella,
    "/cf",
    configurazione,
    "/mn",
    path.join(cartella, "AppxManifest.xml"),
    "/of",
    path.join(cartella, "resources.pri"),
    "/o",
  ]);
  const spezzati = fs.readdirSync(cartella).filter((f) => /^resources\..+\.pri$/.test(f));
  if (spezzati.length > 0) {
    basta(`makepri ha spezzato le risorse in ${spezzati.join(", ")}: nel .msix i loghi andrebbero persi`);
  }

  console.log(`msix: cartella pronta in ${cartella}`);
  console.log(`  ${identita.nome}  ${versione}  (${identita.editore})`);
  if (opzioni.soloCartella) return;

  const pacchetto = path.join(uscita, `Rymembra_${versione}_x64.msix`);
  esegui(trovaStrumento("makeappx"), ["pack", "/d", cartella, "/p", pacchetto, "/o"]);
  const dimensione = (fs.statSync(pacchetto).size / 1048576).toFixed(1);
  console.log(`msix: ${pacchetto} (${dimensione} MB)`);
  console.log("  non è firmato, ed è voluto: lo firma il Store dopo la certificazione.");
}

if (require.main === module) main();

module.exports = {
  MARCATORE,
  SEGNAPOSTI,
  controllaIdentita,
  dallaMaggiore,
  escapiXml,
  riempi,
  segnapostiDel,
  senzaPacchettiDiRisorse,
  versioneMsix,
};
