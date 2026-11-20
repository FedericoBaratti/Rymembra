/*
 * La firma minisign, verificata contro i byte.
 *
 * # Perché esiste
 *
 * Fino alla 3.3 la CI guardava del `.sig` solo il **testo**: l'identificativo
 * della chiave (`manifesto.js::identificativo`), la versione e il nome del file
 * nel commento fidato. Nessun passo controllava che la firma fosse valida per i
 * byte dell'installer o dell'APK che si carica. Un `.sig` copiato dalla release
 * sbagliata (stessa chiave, stessa versione), un file riconfezionato dopo la
 * firma, un artefatto sostituito fra due lavori: tutti i cancelli dicevano
 * «ok», `latest.json` era perfetto, e ogni copia installata — a partire dalle
 * 3.2.0, che hanno `requireSignedVersion` — scaricava quindici megabyte e li
 * buttava con «The signature verification failed». Il guasto sta sul computer
 * di altri, e nessuno lo vede prima di averlo spedito.
 *
 * Qui si fa quel che fa il client, con il solo `crypto` di Node, senza
 * dipendenze (gli strumenti non ne hanno): è `minisign-verify` di Rust, che il
 * plugin dell'updater e il telefono usano, riscritto in poche righe e provato
 * sulla firma vera della 3.2.0 (`minisign.test.js`).
 *
 * # Il formato, com'è dentro Tauri
 *
 * La chiave pubblica e la firma sono il **testo** di un file minisign,
 * codificato in base64 (`plugins.updater.pubkey`; il contenuto del `.sig`, che
 * è anche il campo `signature` di `latest.json`).
 *
 *   chiave   untrusted comment: …
 *            base64( "Ed" | id (8) | chiave Ed25519 (32) )                 42 byte
 *   firma    untrusted comment: …
 *            base64( "ED" | id (8) | firma Ed25519 (64) )                  74 byte
 *            trusted comment: <commento>
 *            base64( firma globale Ed25519 (64) )                          64 byte
 *
 * Con l'algoritmo `ED` — quello che `tauri signer sign` scrive — la firma non è
 * sui byte del file ma sul loro BLAKE2b-512 (*prehash*). La firma globale copre
 * i 64 byte della firma più il commento fidato (da dopo `trusted comment: `):
 * è quella che impedisce di cambiare la versione che `requireSignedVersion`
 * confronta.
 *
 * # Dove si discosta da `minisign-verify`, e perché
 *
 * Accetta **solo** `ED`. Il plugin del computer accetta anche `Ed` (la firma
 * diretta, in disuso), ma il telefono no (`verify_stream`), e `tauri signer`
 * non la scrive: una firma `Ed` in una release è un'anomalia da fermare, non un
 * caso da tollerare. Per il resto è identico: lunghezze esatte (Node decodifica
 * il base64 senza guardare, quindi la lunghezza è l'unica guardia), prefisso
 * `trusted comment: ` di diciassette caratteri, stesso identificativo di chiave.
 *
 * # Un'uscita sola
 *
 * `{ ok, motivo }`: la funzione non lancia e non esce dal processo, così chi la
 * chiama (`manifesto.js`, `apk.js`) decide come dirlo.
 */
const crypto = require("crypto");

/** Il prefisso DER di una chiave pubblica Ed25519 (SubjectPublicKeyInfo): 12 byte, poi i 32 della chiave. */
const SPKI = Buffer.from("302a300506032b6570032100", "hex");

const COMMENTO_FIDATO = "trusted comment: ";

/** L'alfabeto base64 con il suo riempimento, e nient'altro: il decodificatore di Rust è severo. */
const BASE64 = /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/;

/** Base64 severo: `null` se non lo è, i byte se lo è. */
function dalBase64(testo) {
  return typeof testo === "string" && testo !== "" && BASE64.test(testo) ? Buffer.from(testo, "base64") : null;
}

/** Il testo di un file minisign tenuto in base64, in righe (a capo `\n` o `\r\n`). */
function righeDi(base64DelFile, cosa) {
  const grezzo = dalBase64(String(base64DelFile).trim());
  if (grezzo === null) throw new Error(`${cosa}: non è base64`);
  return grezzo.toString("utf8").split(/\r?\n/);
}

/** `{ algoritmo, id, chiave }` dalla chiave pubblica di `tauri.conf.json`; lancia se non è una chiave. */
function leggiChiave(base64) {
  const righe = righeDi(base64, "la chiave pubblica");
  const dati = dalBase64(righe[1] ?? "");
  if (dati === null || dati.length !== 42) throw new Error("la chiave pubblica non ha 42 byte di dati");
  const algoritmo = dati.subarray(0, 2).toString("latin1");
  if (algoritmo !== "Ed" && algoritmo !== "ED") throw new Error(`la chiave pubblica ha l'algoritmo «${algoritmo}», che non è minisign`);
  return { algoritmo, id: dati.subarray(2, 10), chiave: dati.subarray(10, 42) };
}

/** `{ algoritmo, id, firma, commento, globale }` da un `.sig`; lancia se non è una firma minisign. */
function leggiFirma(base64) {
  const righe = righeDi(base64, "la firma");
  if (righe.length < 4) throw new Error("la firma non ha le quattro righe di un file minisign");
  const dati = dalBase64(righe[1]);
  if (dati === null || dati.length !== 74) throw new Error("la firma non ha 74 byte di dati");
  if (!righe[2].startsWith(COMMENTO_FIDATO)) throw new Error(`la terza riga non comincia per «${COMMENTO_FIDATO}»`);
  const globale = dalBase64(righe[3]);
  if (globale === null || globale.length !== 64) throw new Error("la firma globale non ha 64 byte");
  return {
    algoritmo: dati.subarray(0, 2).toString("latin1"),
    id: dati.subarray(2, 10),
    firma: dati.subarray(10, 74),
    commento: righe[2].slice(COMMENTO_FIDATO.length),
    globale,
  };
}

/** Ed25519 con il `crypto` di Node: `true` solo se la firma torna. */
function ed25519(messaggio, firma, chiave) {
  const pubblica = crypto.createPublicKey({ key: Buffer.concat([SPKI, chiave]), format: "der", type: "spki" });
  return crypto.verify(null, messaggio, pubblica, firma);
}

/** Il BLAKE2b-512 di `byte`: quel che l'algoritmo `ED` firma. */
function digestDi(byte) {
  return crypto.createHash("blake2b512").update(byte).digest();
}

/**
 * La firma è valida per un digest già calcolato? Separata da `verificaFirma`
 * perché la prova con la firma vera della 3.2.0 (`minisign.test.js`) tiene il
 * digest dell'installer e non i quindici megabyte.
 *
 * @param {Buffer} digest il BLAKE2b-512 dei byte firmati
 * @param {string} firmaBase64 il contenuto del `.sig`
 * @param {string} chiaveBase64 `plugins.updater.pubkey`
 * @returns {{ ok: boolean, motivo?: string }}
 */
function verificaDigest(digest, firmaBase64, chiaveBase64) {
  let chiave;
  let firma;
  try {
    chiave = leggiChiave(chiaveBase64);
    firma = leggiFirma(firmaBase64);
  } catch (e) {
    return { ok: false, motivo: e.message };
  }
  if (firma.algoritmo !== "ED") {
    return {
      ok: false,
      motivo: `la firma ha l'algoritmo «${firma.algoritmo}» invece di «ED» (BLAKE2b-512 del file): ` +
        "`tauri signer sign` scrive «ED», e il telefono rifiuta le altre",
    };
  }
  if (!firma.id.equals(chiave.id)) {
    return {
      ok: false,
      motivo: `la firma è della chiave ${firma.id.toString("hex")}, non di quella dell'updater (${chiave.id.toString("hex")})`,
    };
  }
  try {
    if (!ed25519(digest, firma.firma, chiave.chiave)) {
      return { ok: false, motivo: "la firma non è valida per i byte del file: sono altri da quelli che la chiave ha firmato" };
    }
    const coperto = Buffer.concat([firma.firma, Buffer.from(firma.commento, "utf8")]);
    if (!ed25519(coperto, firma.globale, chiave.chiave)) {
      return { ok: false, motivo: "la firma globale non copre il commento fidato: il commento (file, versione) è stato toccato" };
    }
  } catch (e) {
    return { ok: false, motivo: `la verifica non è riuscita: ${e.message}` };
  }
  return { ok: true };
}

/**
 * La firma `firmaBase64` è valida per `byte`, con la chiave `chiaveBase64`?
 *
 * @param {Buffer} byte il file firmato, per intero
 * @param {string} firmaBase64 il contenuto del `.sig`
 * @param {string} chiaveBase64 `plugins.updater.pubkey`
 * @returns {{ ok: boolean, motivo?: string }}
 */
function verificaFirma(byte, firmaBase64, chiaveBase64) {
  let digest;
  try {
    digest = digestDi(byte);
  } catch (e) {
    // Fail-closed: un Node senza BLAKE2b non ha verificato niente.
    return { ok: false, motivo: `questo Node non calcola il BLAKE2b-512: ${e.message}` };
  }
  return verificaDigest(digest, firmaBase64, chiaveBase64);
}

module.exports = { digestDi, leggiChiave, leggiFirma, verificaDigest, verificaFirma };
