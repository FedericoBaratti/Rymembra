/*
 * GitHub, finto, su `127.0.0.1`.
 *
 * # A cosa serve
 *
 * A provare l'updater senza pubblicare una release. Serve le stesse due cose
 * che servirebbe GitHub — un `latest.json` e l'installer a cui punta — e
 * `apps/desktop/src-tauri/src/aggiornamenti.rs` ha il gancio che lo rende
 * possibile: `AETHER_AGGIORNAMENTI_ENDPOINT`, sotto `#[cfg(debug_assertions)]`,
 * sostituisce l'endpoint quando l'applicazione gira con `npm run dev`.
 *
 * L'endpoint è `http://` e non `https://`, e va bene: il vincolo sul protocollo
 * sta in `tauri-plugin-updater`, in `config.rs`, dentro un
 * `#[cfg(not(debug_assertions))]`. In sviluppo esce un avviso giallo, non un
 * errore. In rilascio non esiste né la variabile né la deroga.
 *
 * # Perché la versione non si passa da riga di comando
 *
 * Perché sarebbe l'unico modo di sbagliare questa prova senza accorgersene.
 * Il numero nel manifesto deve essere quello che l'installer **installa
 * davvero**: se il manifesto annuncia la 0.3.0 e l'installer dentro è la 0.2.0,
 * l'aggiornamento riesce, l'applicazione riparte, si ritrova più vecchia di
 * quel che le era stato promesso e al controllo dopo si offre di nuovo lo
 * stesso aggiornamento. Un ciclo che non si ferma, e che sembra un difetto
 * dell'updater mentre è un numero digitato male.
 *
 * Quindi si legge dal nome del file che il bundler ha scritto —
 * `Aether_0.3.0_x64-setup.exe` — che è l'unico posto che non può mentire,
 * perché lo ha scritto la stessa compilazione che ha prodotto i byte.
 *
 * # Un modo
 *
 *   node strumenti/finto-aggiornamento.js                 sulla porta 8787
 *   node strumenti/finto-aggiornamento.js --porta 9000
 *   node strumenti/finto-aggiornamento.js --guasta-firma  la prova negativa
 *
 * L'ultima è quella che conta più delle altre: vedere che un aggiornamento si
 * installa dice che la catena è collegata; vedere che una firma sbagliata viene
 * **rifiutata** dice che è anche buona a qualcosa.
 */
const fs = require("fs");
const http = require("http");
const path = require("path");

// `manifesto.js` fa `process.chdir` sulla radice del repo quando lo si carica,
// e presta i suoi pezzi invece di farseli riscrivere qui: il confronto fra gli
// identificativi delle due metà della chiave esiste per non essere sbagliato,
// e due copie di un controllo così sono una copia che prima o poi diverge.
const {
  BERSAGLIO,
  chiavePubblica,
  identificativo,
  installer,
  note,
  versione,
} = require("./manifesto.js");

const BUNDLE = "target/release/bundle/nsis";
const PORTA_DI_SERIE = 8787;

/** L'alfabeto di base64, per guastare una firma restando dentro il formato. */
const ALFABETO =
  "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

function basta(messaggio) {
  console.error(messaggio);
  process.exit(1);
}

/** Gli argomenti, che sono tre e non meritano una libreria. */
function argomenti() {
  const argv = process.argv.slice(2);
  let porta = PORTA_DI_SERIE;
  let guasta = false;
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--guasta-firma") {
      guasta = true;
    } else if (argv[i] === "--porta") {
      porta = Number.parseInt(argv[++i], 10);
      if (!Number.isInteger(porta) || porta < 1 || porta > 65535) {
        basta("--porta vuole un numero fra 1 e 65535.");
      }
    } else {
      basta(
        `Non capisco «${argv[i]}».\n` +
          "  node strumenti/finto-aggiornamento.js [--porta N] [--guasta-firma]",
      );
    }
  }
  return { porta, guasta };
}

/**
 * La versione che l'installer installa davvero, dal nome che il bundler gli ha
 * dato.
 */
function versioneDellInstaller(nome) {
  const m = nome.match(/_(\d+\.\d+\.\d+)_/);
  if (!m) {
    basta(
      `Dal nome «${nome}» non ricavo un numero di versione.\n` +
        "Atteso qualcosa come Aether_0.3.0_x64-setup.exe.",
    );
  }
  return m[1];
}

/** `a` è più recente di `b`? Tre numeri, che è la forma che `versione.js` impone. */
function piuRecente(a, b) {
  const [x, y] = [a.split(".").map(Number), b.split(".").map(Number)];
  for (let i = 0; i < 3; i++) {
    if (x[i] !== y[i]) return x[i] > y[i];
  }
  return false;
}

/**
 * Una firma ben formata che però non torna.
 *
 * Non si tocca il file su disco: un `.sig` guastato che resta lì è una mina che
 * salta alla release dopo. Si guasta la copia che finisce nel manifesto, e
 * basta rilanciare senza il flag per tornare a quella buona.
 *
 * Il carattere si cambia **in mezzo** e non in testa. I primi dieci byte della
 * riga di dati sono due di algoritmo e otto di identificativo di chiave:
 * cambiare quelli darebbe «questa firma è di un'altra chiave», che è un errore
 * diverso e che `manifesto.js` intercetta già prima di pubblicare. Quel che si
 * vuole provare qui è l'altro — la chiave è la nostra, i byte firmati no — che
 * è il caso in cui qualcuno ha messo le mani sull'installer per strada.
 */
function guastaFirma(base64DelFile) {
  const testo = Buffer.from(base64DelFile, "base64").toString("utf8");
  const righe = testo.split("\n");
  const i = righe.findIndex(
    (r) => r.trim() !== "" && !r.startsWith("untrusted comment:"),
  );
  if (i < 0) basta("Nella firma non c'è nessuna riga di dati da guastare.");
  const riga = righe[i].trim();

  // Dal centro in avanti, il primo carattere che appartiene all'alfabeto: il
  // riempimento finale `=` non ne fa parte, e sostituirlo darebbe un base64
  // rotto — cioè un errore di formato invece di una firma che non verifica.
  let dove = -1;
  for (let k = Math.floor(riga.length / 2); k < riga.length; k++) {
    if (ALFABETO.indexOf(riga[k]) >= 0) {
      dove = k;
      break;
    }
  }
  if (dove < 0) basta("Non ho trovato un carattere da cambiare nella firma.");

  const successivo = ALFABETO[(ALFABETO.indexOf(riga[dove]) + 1) % ALFABETO.length];
  righe[i] = riga.slice(0, dove) + successivo + riga.slice(dove + 1);
  return Buffer.from(righe.join("\n"), "utf8").toString("base64");
}

function main() {
  const { porta, guasta } = argomenti();

  // Gli stessi controlli della release vera, e nello stesso ordine: un
  // installer solo nella cartella, il suo `.sig` accanto, la chiave pubblica
  // non vuota, e le due metà che si corrispondono. Se qui passa, in CI passa.
  const { nome, firma } = installer();
  const chiave = chiavePubblica();
  const dellaChiave = identificativo(chiave, "plugins.updater.pubkey");
  const dellaFirma = identificativo(firma, `${nome}.sig`);
  if (dellaChiave !== dellaFirma) {
    basta(
      "La firma non è stata fatta con la chiave che gli eseguibili conoscono.\n" +
        `  la chiave in tauri.conf.json:  ${dellaChiave}\n` +
        `  la chiave che ha firmato:      ${dellaFirma}\n\n` +
        "Ricostruire con TAURI_SIGNING_PRIVATE_KEY nell'ambiente.",
    );
  }

  const offerta = versioneDellInstaller(nome);
  const corrente = versione();
  if (!piuRecente(offerta, corrente)) {
    basta(
      `L'installer è la ${offerta}, e l'albero dichiara la ${corrente}.\n\n` +
        "L'applicazione che parte con `npm run dev` si dichiarerà quindi almeno\n" +
        "altrettanto nuova, e non si offrirà nessun aggiornamento — il confronto\n" +
        "lo fa il plugin, e non c'è modo di convincerlo dall'esterno.\n\n" +
        `Serve un albero più vecchio dell'installer:\n` +
        `  node strumenti/versione.js <qualcosa di minore di ${offerta}>\n` +
        "  cargo check --workspace",
    );
  }

  const percorso = path.join(BUNDLE, nome);
  const quantoPesa = fs.statSync(percorso).size;

  const manifesto = {
    version: offerta,
    notes: note(offerta),
    pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
    platforms: {
      [BERSAGLIO]: {
        signature: guasta ? guastaFirma(firma) : firma,
        url: `http://127.0.0.1:${porta}/${nome}`,
      },
    },
  };
  const corpo = Buffer.from(`${JSON.stringify(manifesto, null, 2)}\n`, "utf8");

  const server = http.createServer((req, res) => {
    // Il percorso e basta: `req.url` porta anche l'eventuale query, e
    // `decodeURIComponent` perché il nome del file passa per l'URL.
    const chiesto = decodeURIComponent(new URL(req.url, "http://127.0.0.1").pathname);
    console.log(`  ${req.method} ${chiesto}`);

    if (chiesto === "/latest.json") {
      res.writeHead(200, {
        "content-type": "application/json",
        "content-length": corpo.length,
      });
      res.end(req.method === "HEAD" ? undefined : corpo);
      return;
    }

    if (chiesto === `/${nome}`) {
      // `content-length` non è una gentilezza: senza, `AvanzamentoIpc.totale`
      // resta `None` e la fascia mostra i megabyte scesi invece della barra —
      // cioè si proverebbe un percorso diverso da quello che vedrà chi scarica
      // da GitHub, che il `Content-Length` lo manda.
      res.writeHead(200, {
        "content-type": "application/octet-stream",
        "content-length": quantoPesa,
      });
      if (req.method === "HEAD") {
        res.end();
        return;
      }
      fs.createReadStream(percorso).pipe(res);
      return;
    }

    res.writeHead(404, { "content-type": "text/plain; charset=utf-8" });
    res.end("Qui ci sono solo /latest.json e l'installer.\n");
  });

  // Solo `127.0.0.1`: un finto server di aggiornamenti che ascolta su tutte le
  // interfacce è una macchina che offre di installare software a chiunque sia
  // sulla stessa rete.
  server.listen(porta, "127.0.0.1", () => {
    const endpoint = `http://127.0.0.1:${porta}/latest.json`;
    console.log(`Aether ${corrente}  →  ${offerta}`);
    console.log(`  ${nome} (${(quantoPesa / 1024 / 1024).toFixed(1)} MB)`);
    console.log(`  firmato con ${dellaChiave}${guasta ? "  ⚠ GUASTATA APPOSTA" : ""}`);
    if (guasta) {
      console.log(
        "\n  L'avviso comparirà lo stesso — la firma si verifica allo\n" +
          "  scaricamento, non al controllo. È «Aggiorna» che deve fallire.",
      );
    }
    console.log(`\nIn ascolto su ${endpoint}\n`);
    console.log("In un altro terminale:");
    console.log("  cd apps\\desktop");
    console.log(`  $env:AETHER_AGGIORNAMENTI_ENDPOINT = "${endpoint}"`);
    console.log("  npm run dev\n");
    console.log("Poi Impostazioni → Aggiornamenti → «Controlla adesso»,");
    console.log("che il primo giro da solo arriva dopo due minuti.\n");
  });

  server.on("error", (err) => {
    if (err.code === "EADDRINUSE") {
      basta(
        `La porta ${porta} è occupata.\n` +
          "  node strumenti/finto-aggiornamento.js --porta 9000",
      );
    }
    basta(`Il server non è partito: ${err.message}`);
  });
}

main();
