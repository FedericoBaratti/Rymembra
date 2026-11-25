/*
 * La firma minisign contro i byte (S5-2).
 *
 * Due famiglie di prove. Con una chiave usa e getta (`firma-di-prova.js`) si
 * costruiscono firme nel formato che scrive `tauri signer sign` e si rompono a
 * una a una. Con la firma **vera** della 3.2.0 e la chiave vera del repository si
 * prova che la funzione legge quel che Tauri scrive davvero, non quel che
 * pensiamo che scriva.
 *
 *   node --test "strumenti/*.test.js"
 */
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const { digestDi, leggiChiave, leggiFirma, verificaDigest, verificaFirma } = require("./minisign.js");
const { nuovaChiave, commentoDi, incarta } = require("./firma-di-prova.js");

const NOME = "Rymembra_9.9.9_x64-setup.exe";
const BYTE = Buffer.from("un installer, o quel che ne fa le veci");

/** Il testo di un `.sig` in base64, riletto in righe e rifatto dopo una modifica. */
function ritocca(firma, cambia) {
  const righe = Buffer.from(firma, "base64").toString("utf8").split("\n");
  cambia(righe);
  return Buffer.from(righe.join("\n"), "utf8").toString("base64");
}

test("una firma fatta come Tauri la fa passa, e dice la stessa cosa del client", () => {
  const chiave = nuovaChiave();
  const firma = chiave.firma(BYTE, commentoDi(NOME, "9.9.9"));
  assert.deepEqual(verificaFirma(BYTE, firma, chiave.pubkey), { ok: true });
  // L'algoritmo è `ED`, l'identificativo è quello della chiave, il commento si legge dal diciassettesimo carattere.
  const letta = leggiFirma(firma);
  assert.equal(letta.algoritmo, "ED");
  assert.deepEqual(letta.id, chiave.id);
  assert.equal(letta.commento, commentoDi(NOME, "9.9.9"));
  assert.deepEqual(leggiChiave(chiave.pubkey).id, chiave.id);
});

test("un byte cambiato non passa (S5-2)", () => {
  const chiave = nuovaChiave();
  const firma = chiave.firma(BYTE, commentoDi(NOME, "9.9.9"));
  for (const i of [0, BYTE.length - 1]) {
    const cambiato = Buffer.from(BYTE);
    cambiato[i] ^= 1;
    const esito = verificaFirma(cambiato, firma, chiave.pubkey);
    assert.equal(esito.ok, false, `byte ${i}`);
    assert.match(esito.motivo, /non è valida per i byte/);
  }
  // Un byte in più, o in meno.
  assert.equal(verificaFirma(Buffer.concat([BYTE, Buffer.from("x")]), firma, chiave.pubkey).ok, false);
  assert.equal(verificaFirma(BYTE.subarray(1), firma, chiave.pubkey).ok, false);
  assert.equal(verificaFirma(Buffer.alloc(0), firma, chiave.pubkey).ok, false);
});

test("un commento fidato alterato non passa: lo copre la firma globale (S5-2)", () => {
  const chiave = nuovaChiave();
  const firma = chiave.firma(BYTE, commentoDi(NOME, "9.9.9"));
  // La versione che `requireSignedVersion` confronta sta qui dentro.
  const altraVersione = ritocca(firma, (r) => (r[2] = r[2].replace("version:9.9.9", "version:9.9.8")));
  const esito = verificaFirma(BYTE, altraVersione, chiave.pubkey);
  assert.equal(esito.ok, false);
  assert.match(esito.motivo, /firma globale non copre il commento/);
  // Un carattere solo, in coda.
  assert.equal(verificaFirma(BYTE, ritocca(firma, (r) => (r[2] += "x")), chiave.pubkey).ok, false);
  // Dal digest o dai byte è lo stesso: la firma sui byte era buona, a cadere è il commento.
  assert.equal(verificaDigest(digestDi(BYTE), altraVersione, chiave.pubkey).ok, false);
});

test("la firma di un'altra chiave non passa, con un identificativo diverso o uguale", () => {
  const chiave = nuovaChiave();
  const altra = nuovaChiave();
  const diversa = verificaFirma(BYTE, altra.firma(BYTE, commentoDi(NOME, "9.9.9")), chiave.pubkey);
  assert.equal(diversa.ok, false);
  assert.match(diversa.motivo, /non di quella dell'updater/);
  // Lo stesso identificativo non basta: è un'etichetta, la crittografia è altrove.
  const gemella = nuovaChiave({ id: chiave.id });
  const uguale = verificaFirma(BYTE, gemella.firma(BYTE, commentoDi(NOME, "9.9.9")), chiave.pubkey);
  assert.equal(uguale.ok, false);
  assert.match(uguale.motivo, /non è valida per i byte/);
});

test("l'algoritmo `Ed` (senza prehash) si rifiuta: Tauri non lo scrive e il telefono non lo legge", () => {
  const chiave = nuovaChiave();
  const esito = verificaFirma(BYTE, chiave.firmaIlDigest(BYTE, commentoDi(NOME, "9.9.9"), { algoritmo: "Ed" }), chiave.pubkey);
  assert.equal(esito.ok, false);
  assert.match(esito.motivo, /«Ed» invece di «ED»/);
});

test("un `.sig` o una chiave che non sono quel che dicono danno un'uscita, non un'eccezione", () => {
  const chiave = nuovaChiave();
  const firma = chiave.firma(BYTE, commentoDi(NOME, "9.9.9"));
  const brutte = {
    "non base64": "questo non è base64!",
    vuota: "",
    "una riga sola": incarta(["untrusted comment: x"]),
    "dati corti": ritocca(firma, (r) => (r[1] = r[1].slice(0, 40))),
    "dati lunghi": ritocca(firma, (r) => (r[1] = `${r[1]}AAAA`)),
    "senza il commento fidato": ritocca(firma, (r) => (r[2] = r[2].replace("trusted comment: ", "commento: "))),
    "firma globale corta": ritocca(firma, (r) => (r[3] = r[3].slice(0, 40))),
    "base64 spezzato": ritocca(firma, (r) => (r[1] = `${r[1].slice(0, 20)} ${r[1].slice(20)}`)),
  };
  for (const [nome, sig] of Object.entries(brutte)) {
    const esito = verificaFirma(BYTE, sig, chiave.pubkey);
    assert.equal(esito.ok, false, nome);
    assert.equal(typeof esito.motivo, "string", nome);
  }
  for (const pubkey of ["", "x", incarta(["untrusted comment: x", "AAAA"]), incarta(["untrusted comment: x", Buffer.alloc(42).toString("base64")])]) {
    assert.equal(verificaFirma(BYTE, firma, pubkey).ok, false, pubkey);
  }
  // Gli a capo di Windows nel file non cambiano niente.
  const conCrlf = Buffer.from(Buffer.from(firma, "base64").toString("utf8").replace(/\n/g, "\r\n"), "utf8").toString("base64");
  assert.equal(verificaFirma(BYTE, conCrlf, chiave.pubkey).ok, true);
});

/* ── la firma vera della 3.2.0 ────────────────────────────────────────── */

/**
 * Il `.sig` di `Rymembra_3.2.0_x64-setup.exe`, identico al campo `signature` di
 * `latest.json` della release pubblica v3.2.0 (un asset pubblico: nessun
 * segreto). Il commento fidato dice `file:Rymembra_3.2.0_x64-setup.exe` e
 * `version:3.2.0`, e la chiave è quella che il repository ha ancora in
 * `plugins.updater.pubkey`.
 */
const SIG_320 =
  "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUTkRiNkxJeUlHLy9iM1g5V05zdys1QzZSTExoY2djNFF1ZHMzZWF4Nm5UUGgvRm9kc3VrMFNwNVFWRUlkQ3poRkVCNWQ1akpiaXpVeGJzQ0VEenIwZWNSeWx4aGJZM2c0PQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkzNDM1Njc5CWZpbGU6UnltZW1icmFfMy4yLjBfeDY0LXNldHVwLmV4ZQl2ZXJzaW9uOjMuMi4wCmdWNk9TS2tNK1h6WDFnOTg1RGxMUDZKK1BrUDlBVzlpWXRPUlVTdnFsbHY5L0RubkMxWlYvNlI5WnFzUkdsb2JaVmpNeEtQd01CQ3A2QVU3b01leER3PT0K";

/**
 * Il BLAKE2b-512 di quell'installer (15 MB, che non sta nel repository): è
 * quello che l'algoritmo `ED` firma, quindi basta a provare la firma vera.
 */
const DIGEST_320 = Buffer.from(
  "0d27edf153c1d0394fab8f2b19871fcb91d28ae3df6c0506dfa474df4a220f63e82d18cdda23411d5b70d40765318f1ba30b0d1159bb80bfed1cc3d4ea1b6ac4",
  "hex",
);

/** La chiave che il repository compila negli eseguibili: un posto solo. */
const PUBKEY = JSON.parse(fs.readFileSync(path.join(__dirname, "../apps/desktop/src-tauri/tauri.conf.json"), "utf8")).plugins.updater.pubkey;

test("la firma vera della 3.2.0 torna con la chiave del repository", () => {
  assert.deepEqual(verificaDigest(DIGEST_320, SIG_320, PUBKEY), { ok: true });
  const firma = leggiFirma(SIG_320);
  assert.equal(firma.algoritmo, "ED");
  assert.equal(firma.commento, "timestamp:1793435679\tfile:Rymembra_3.2.0_x64-setup.exe\tversion:3.2.0");
  assert.equal(firma.id.toString("hex"), leggiChiave(PUBKEY).id.toString("hex"));
});

test("la firma vera della 3.2.0 non passa con un bit cambiato nel file o nel commento", () => {
  const altroFile = Buffer.from(DIGEST_320);
  altroFile[0] ^= 1;
  assert.match(verificaDigest(altroFile, SIG_320, PUBKEY).motivo, /non è valida per i byte/);
  // La versione firmata: è quel che `requireSignedVersion` esige, e non si può spostare.
  const altraVersione = ritocca(SIG_320, (r) => (r[2] = r[2].replace("version:3.2.0", "version:3.3.0")));
  assert.match(verificaDigest(DIGEST_320, altraVersione, PUBKEY).motivo, /firma globale non copre il commento/);
  // Né una firma vera di un'altra chiave (con lo stesso identificativo) prende il suo posto.
  const gemella = nuovaChiave({ id: leggiChiave(PUBKEY).id });
  assert.equal(verificaDigest(DIGEST_320, gemella.firmaIlDigest(DIGEST_320, commentoDi("Rymembra_3.2.0_x64-setup.exe", "3.2.0")), PUBKEY).ok, false);
});

// L'installer vero pesa quindici megabyte e sta fuori dal repository: chi ce
// l'ha (scaricato dalla release v3.2.0) lo indica con questa variabile, e la
// prova passa dal file e non dal digest.
test("l'installer vero della 3.2.0, se c'è, torna per intero", { skip: process.env.RYMEMBRA_INSTALLER_320 ? false : "RYMEMBRA_INSTALLER_320 non è impostata" }, () => {
  assert.deepEqual(verificaFirma(fs.readFileSync(process.env.RYMEMBRA_INSTALLER_320), SIG_320, PUBKEY), { ok: true });
});
