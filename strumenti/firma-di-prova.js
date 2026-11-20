/*
 * Una chiave minisign usa e getta, per le prove.
 *
 * Serve a costruire, dentro le prove, firme **vere** nel formato che scrive
 * `tauri signer sign` (algoritmo `ED`, BLAKE2b-512 del file, firma globale sul
 * commento fidato), senza una chiave privata nel repository: la coppia nasce a
 * ogni chiamata e muore con il processo. Non è uno strumento della CI e non
 * finisce in nessun workflow: lo chiamano `minisign.test.js`,
 * `manifesto.test.js` e `apk.test.js`. Il nome non finisce in `.test.js`, così
 * `node --test` non lo prende per una prova.
 */
const crypto = require("crypto");

/** Il prefisso DER di una chiave pubblica Ed25519 (SubjectPublicKeyInfo). */
const SPKI = Buffer.from("302a300506032b6570032100", "hex");

/** Il BLAKE2b-512 di `byte`: quel che minisign firma con l'algoritmo `ED`. */
function digestDi(byte) {
  return crypto.createHash("blake2b512").update(byte).digest();
}

/** Il testo di un file minisign, in base64: come lo tiene Tauri in `pubkey` e nel `.sig`. */
function incarta(righe) {
  return Buffer.from(`${righe.join("\n")}\n`, "utf8").toString("base64");
}

/**
 * Una coppia di chiavi nuova.
 *
 * @param {{ id?: Buffer }} [opzioni] l'identificativo (otto byte) della chiave
 * @returns {{ pubkey: string, id: Buffer, firma: Function, firmaIlDigest: Function }}
 */
function nuovaChiave({ id = crypto.randomBytes(8) } = {}) {
  const { publicKey, privateKey } = crypto.generateKeyPairSync("ed25519");
  const pub = publicKey.export({ format: "der", type: "spki" }).subarray(SPKI.length);
  const pubkey = incarta([
    `untrusted comment: minisign public key: ${Buffer.from(id).reverse().toString("hex").toUpperCase()}`,
    Buffer.concat([Buffer.from("Ed"), id, pub]).toString("base64"),
  ]);

  /** Il `.sig` di un digest già calcolato, con l'algoritmo che si vuole (`ED` di serie). */
  function firmaIlDigest(digest, commento, { algoritmo = "ED" } = {}) {
    const firma = crypto.sign(null, digest, privateKey);
    const globale = crypto.sign(null, Buffer.concat([firma, Buffer.from(commento, "utf8")]), privateKey);
    return incarta([
      "untrusted comment: signature from tauri secret key",
      Buffer.concat([Buffer.from(algoritmo), id, firma]).toString("base64"),
      `trusted comment: ${commento}`,
      globale.toString("base64"),
    ]);
  }

  /** Il `.sig` di `byte`, come `tauri signer sign --app-version` lo scrive. */
  function firma(byte, commento) {
    return firmaIlDigest(digestDi(byte), commento);
  }

  return { pubkey, id, firma, firmaIlDigest };
}

/** Il commento fidato come lo scrive Tauri: marca, file e versione, separati da tabulazioni. */
function commentoDi(file, versione, marca = "1790000000") {
  return `timestamp:${marca}\tfile:${file}\tversion:${versione}`;
}

module.exports = { nuovaChiave, commentoDi, digestDi, incarta };
