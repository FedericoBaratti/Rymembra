/*
 * La chiave di firma, guardata prima di firmare.
 *
 * `firma.js` gira una volta sola, nel lavoro `firma` di `release.yml`, con i
 * segreti veri: è il posto peggiore per scoprire che rifiuta una chiave buona.
 * Lo script finisce con `process.exit`, quindi qui gira in un processo suo, con
 * un ambiente costruito a mano e una chiave finta che ha soltanto la forma.
 *
 *   node --test "strumenti/*.test.js"
 */
const test = require("node:test");
const assert = require("node:assert/strict");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const SCRIPT = path.join(__dirname, "firma.js");

/** Una chiave privata come la scrive `tauri signer generate`: base64 di un testo minisign. */
const CHIAVE = Buffer.from(
  "untrusted comment: rsign encrypted secret key\n" + "RWRTY0Iy" + "A".repeat(150) + "\n",
  "utf8",
).toString("base64");

/**
 * Lancia `firma.js` con questi due valori. `undefined` vuol dire «assente»: la
 * variabile non c'è proprio. Le `TAURI_SIGNING_*` di chi lancia le prove si
 * tolgono tutte, e senza badare alle maiuscole, perché su Windows i nomi
 * dell'ambiente non le distinguono.
 */
function lancia(chiave, password) {
  const env = { ...process.env };
  for (const nome of Object.keys(env)) {
    if (nome.toUpperCase().startsWith("TAURI_SIGNING")) delete env[nome];
  }
  if (chiave !== undefined) env.TAURI_SIGNING_PRIVATE_KEY = chiave;
  if (password !== undefined) env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD = password;
  const esito = spawnSync(process.execPath, [SCRIPT], { env, encoding: "utf8" });
  return { codice: esito.status, uscita: `${esito.stdout}${esito.stderr}` };
}

// La chiave generata senza password. GitHub non accetta un segreto vuoto, e un
// segreto che non c'è arriva al passo come stringa vuota (o, su Windows, come
// variabile assente): in nessuno dei due casi la chiave è guasta. `tauri signer
// sign` fuori da un terminale prende l'assenza per una password vuota, e la
// password vuota la usa così com'è.
test("una chiave senza password passa, col segreto vuoto o assente", () => {
  for (const password of ["", undefined]) {
    const { codice, uscita } = lancia(CHIAVE, password);
    assert.equal(codice, 0, `password ${JSON.stringify(password)}: uscita ${codice}\n${uscita}`);
  }
});

test("una password qualunque passa", () => {
  const { codice, uscita } = lancia(CHIAVE, "una password");
  assert.equal(codice, 0, uscita);
});

// L'a capo di `password.txt` incollato dentro il segreto: la chiave non si
// aprirebbe. E la diagnosi non deve ripetere il valore, nemmeno sbagliato.
test("una password con l'a capo dell'incollatura si ferma, senza stamparla", () => {
  const { codice, uscita } = lancia(CHIAVE, "una password\n");
  assert.equal(codice, 1, uscita);
  assert.match(uscita, /a capo ai bordi/);
  assert.ok(!uscita.includes("una password"), uscita);
});

test("una chiave spezzata a ottanta colonne si ferma prima della password", () => {
  const spezzata = CHIAVE.slice(0, 80) + "\n" + CHIAVE.slice(80);
  const { codice, uscita } = lancia(spezzata, "");
  assert.equal(codice, 1, uscita);
  assert.match(uscita, /non è base64 pulita/);
});

// `tauri signer sign` passa il segreto così com'è al decodificatore base64,
// che rifiuta l'a capo che `clip.exe` o un editor lasciano in fondo: il
// controllo non può essere più largo del consumatore che anticipa.
test("una chiave con l'a capo dell'incollatura si ferma, come la password", () => {
  for (const bordo of ["\n", "\r\n", " "]) {
    const { codice, uscita } = lancia(CHIAVE + bordo, "");
    assert.equal(codice, 1, `${JSON.stringify(bordo)}: ${uscita}`);
    assert.match(uscita, /ai bordi/);
    assert.ok(!uscita.includes(CHIAVE.slice(0, 20)), uscita);
  }
});
