/*
 * La chiave di firma, guardata prima di firmare.
 *
 * # Il guasto da incollatura che questo script esiste per dire per nome
 *
 * Fino alla 2.4 `tauri build` la chiave non la toccava fino alla fine: compilava
 * in release, scaricava NSIS, produceva l'installer, e **solo allora** provava
 * a firmarlo. Alla v2.0.1 il segreto era malformato, e la corsa è morta lì,
 * dopo tredici minuti, con un messaggio che parlava di password mentre il
 * guasto era nella chiave:
 *
 *   failed to decode secret key: incorrect updater private key password:
 *   Missing comment in secret key
 *
 * Adesso la firma ha un lavoro suo, dopo la compilazione (`firma` e
 * `firma-apk` in `release.yml`), e in quei due il tempo che questo passo fa
 * risparmiare è poco. Per questo gira **anche** nel lavoro `segreti`, in testa
 * alla release, prima di ogni compilazione: lì un segreto incollato male si
 * scopre dopo due minuti invece che dopo un'ora e mezza. Resta la ragione che
 * conta: `tauri signer sign` un segreto incollato
 * male lo rifiuta con un messaggio che non nomina l'incollatura, e qui lo si
 * dice per nome, prima, senza mai stampare il valore.
 *
 * # Cosa guarda, e perché proprio queste due
 *
 * **Che la chiave sia intera.** `tauri signer generate` scrive un file minisign
 * codificato in base64: una riga sola, senza a capo. Copiata da un terminale
 * che manda a capo a ottanta colonne arriva spezzata, e una base64 spezzata
 * decodifica in spazzatura — che è il modo in cui sparisce la riga
 * `untrusted comment:` che ogni chiave minisign ha in testa. Qui si decodifica
 * e si cerca quella riga: se c'è, il segreto è quantomeno *una chiave*. E
 * senza spazi né a capo ai bordi, come la password: `tauri` passa il valore
 * così com'è al decodificatore base64, che li rifiuta («failed to decode
 * base64 secret key»), e `clip.exe` l'a capo finale lo conserva.
 *
 * **Che la password non porti spazi ai bordi.** `password.txt` finisce con un
 * a capo, come ogni file di testo civile. Incollato com'è dentro il segreto,
 * quell'a capo diventa parte della password, e la chiave non si apre. Non c'è
 * modo di accorgersene guardando il campo su GitHub: sono caratteri invisibili
 * dentro un valore che nessuno può più rileggere. Una password vuota, invece,
 * passa: è quella di una chiave generata senza, e `tauri` la accetta.
 *
 * # Cosa **non** guarda
 *
 * Che la password sia *giusta*, e che la chiave sia *quella* — cioè la metà
 * privata della pubblica compilata dentro l'eseguibile. La prima si scoprirebbe
 * solo provando ad aprire la chiave, la seconda solo dopo aver firmato: è quel
 * che fa `manifesto.js`, confrontando gli identificativi, e resta il suo
 * mestiere. Questo script toglie di mezzo gli errori di *forma*, che sono
 * quelli che si fanno incollando.
 *
 * # Un modo
 *
 *   node strumenti/firma.js
 *
 * Legge `TAURI_SIGNING_PRIVATE_KEY` e `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
 * dall'ambiente (la seconda può mancare, se la chiave non ha password). **Non
 * stampa mai niente che venga da lì dentro**: solo la diagnosi. Un valore di
 * segreto finito in un log è un segreto bruciato, e i log delle corse restano.
 */

/** Il prefisso che minisign mette in testa a ogni chiave, pubblica o privata. */
const COMMENTO = "untrusted comment:";

/** Muore dicendo cosa, e come si rimedia. */
function basta(messaggio) {
  console.error(messaggio);
  process.exit(1);
}

/** L'alfabeto base64, e nient'altro: nemmeno uno spazio. */
const SOLO_BASE64 = /^[A-Za-z0-9+/]+={0,2}$/;

/**
 * Il testo minisign dentro il segreto, se c'è.
 *
 * Ritorna `null` se non è base64 pulita, e il testo decodificato se lo è. La
 * severità sull'alfabeto **non è pedanteria**, ed è la ragione per cui questo
 * controllo esiste in questa forma: `Buffer.from(x, "base64")` di Node ignora
 * in silenzio tutto quel che non appartiene all'alfabeto, a capo compresi,
 * mentre il decodificatore di Rust che sta dentro `tauri` rifiuta. Una chiave
 * spezzata a ottanta colonne — il modo più comune di rompere questo segreto —
 * passerebbe di qui pulita e fallirebbe dopo tredici minuti di compilazione,
 * cioè esattamente il guasto che il passo esiste per anticipare.
 *
 * Il testo minisign decodificato **non** è una forma valida per il segreto,
 * anche se a occhio sembra la cosa giusta da incollare: `tauri` il valore lo
 * tratta come un percorso o come base64, e due righe con dentro spazi e due
 * punti non sono né l'uno né l'altra.
 */
function testoDellaChiave(grezzo) {
  if (!SOLO_BASE64.test(grezzo)) return null;
  try {
    return Buffer.from(grezzo, "base64").toString("utf8");
  } catch {
    return null;
  }
}

function main() {
  const chiave = process.env.TAURI_SIGNING_PRIVATE_KEY;
  const password = process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD;

  if (chiave === undefined || chiave.trim() === "") {
    basta(
      "TAURI_SIGNING_PRIVATE_KEY non c'è, o è vuoto.\n" +
        "Senza, `createUpdaterArtifacts` non produce nessun `.sig` e nessuna\n" +
        "copia installata potrà aggiornarsi. Il valore è il contenuto del file\n" +
        "che ha scritto `npm run tauri signer generate`, tutto intero.",
    );
  }

  // Prima della forma, e senza `trim()`: `tauri signer sign` riceve il valore
  // intero, e un controllo più largo di lui direbbe «ha la forma giusta» a un
  // segreto che un secondo dopo non si decodifica.
  if (chiave !== chiave.trim()) {
    basta(
      "TAURI_SIGNING_PRIVATE_KEY ha spazi o un a capo ai bordi.\n" +
        "Succede incollando da `clip.exe` o da un editor, che lasciano l'a capo\n" +
        "in fondo: `tauri` passa il valore così com'è al decodificatore base64,\n" +
        "che lo rifiuta. Sul campo di GitHub non si vede. Da PowerShell, negli\n" +
        "appunti tutto intero, in un pezzo solo e senza bordi:\n" +
        '  Set-Clipboard -Value ([IO.File]::ReadAllText("$PWD\\.chiavi\\rymembra.key").Trim())',
    );
  }

  const testo = testoDellaChiave(chiave);
  if (testo === null) {
    basta(
      "TAURI_SIGNING_PRIVATE_KEY non è base64 pulita: dentro c'è un a capo,\n" +
        "uno spazio, o un carattere che non appartiene all'alfabeto.\n" +
        "Il file è **una riga sola**, senza a capo: copiarlo da un terminale\n" +
        "che manda a capo a ottanta colonne lo spezza, e `tauri` rifiuta.\n" +
        "Da PowerShell, negli appunti tutto intero e in un pezzo solo:\n" +
        '  Set-Clipboard -Value ([IO.File]::ReadAllText("$PWD\\.chiavi\\rymembra.key"))',
    );
  }

  if (!testo.startsWith(COMMENTO)) {
    basta(
      "TAURI_SIGNING_PRIVATE_KEY è base64, ma dentro non c'è una chiave\n" +
        `minisign: decodificato non comincia per «${COMMENTO}».\n` +
        "Il valore giusto è il contenuto del file scritto da\n" +
        "`npm run tauri signer generate`, non la chiave pubblica e non il\n" +
        "percorso del file.",
    );
  }

  const righe = testo
    .split("\n")
    .map((r) => r.trim())
    .filter((r) => r !== "");

  // Le due chiavi si somigliano abbastanza da sbagliarsi: stanno nella stessa
  // cartella, hanno lo stesso nome a meno di un `.pub`, e sono tutt'e due
  // base64 di un testo che comincia col commento. A distinguerle è il commento
  // stesso — «minisign public key» contro «rsign encrypted secret key» — ed è
  // uno scambio che senza questa riga si scoprirebbe solo in fondo alla
  // compilazione, con un messaggio che parla d'altro.
  if (/public key/i.test(righe[0] ?? "")) {
    basta(
      "TAURI_SIGNING_PRIVATE_KEY contiene la chiave **pubblica**.\n" +
        "Quella sta già in chiaro in `tauri.conf.json`, dentro ogni eseguibile\n" +
        "che è stato distribuito, e non firma niente. Il segreto vuole l'altro\n" +
        "file, quello senza `.pub`.",
    );
  }

  // Il commento e i dati. Una chiave con la sola prima riga supererebbe i
  // controlli di sopra e fallirebbe identica in fondo alla compilazione, che è
  // precisamente quel che questo script esiste per evitare.
  if (righe.length < 2 || righe[1].length < 40) {
    basta(
      "TAURI_SIGNING_PRIVATE_KEY ha il commento ma non i dati.\n" +
        "Dopo la riga `untrusted comment:` ne serve una seconda, lunga, che è\n" +
        "la chiave vera. Ne è arrivata una sola: il valore è troncato.",
    );
  }

  // Senza password non è un guasto, e fermarsi qui voleva dire che una chiave
  // generata senza password non poteva firmare mai: GitHub un segreto vuoto non
  // lo accetta, e uno che non c'è arriva a questo passo come stringa vuota (o,
  // su Windows, non arriva proprio). `tauri signer sign` (CLI 2.12) le tratta
  // allo stesso modo: una password vuota la usa così com'è, e l'assenza, fuori
  // da un terminale, la prende per una password vuota. Se la chiave invece una
  // password ce l'ha, lo dice lui nel passo dopo, un secondo più tardi: qui non
  // c'è più una compilazione da far perdere. Va detto, però, perché quel suo
  // messaggio non nomina il segreto che manca.
  if (password === undefined || password === "") {
    console.log(
      "TAURI_SIGNING_PRIVATE_KEY_PASSWORD è vuoto o non c'è: si firma con la\n" +
        "password vuota. Va bene se la chiave è stata generata senza password.\n" +
        "Se ne ha una, e `tauri signer sign` risponde «incorrect updater private\n" +
        "key password», il segreto della password manca fra quelli dell'ambiente\n" +
        "`release` (Settings → Environments → release).",
    );
  } else if (password !== password.trim()) {
    basta(
      "TAURI_SIGNING_PRIVATE_KEY_PASSWORD ha spazi o un a capo ai bordi.\n" +
        "Succede incollando il contenuto di un file di testo, che finisce con\n" +
        "un a capo: quell'a capo diventa parte della password e la chiave non\n" +
        "si apre. Sul campo di GitHub non si vede. Da PowerShell:\n" +
        '  Set-Clipboard -Value ([IO.File]::ReadAllText("$PWD\\.chiavi\\password.txt").Trim())',
    );
  }

  console.log("La chiave di firma ha la forma giusta.");
}

main();
