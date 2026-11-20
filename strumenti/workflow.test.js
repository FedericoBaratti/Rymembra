/*
 * I workflow di GitHub, letti come testo.
 *
 * Una corsa di rilascio parte una volta a versione, e i suoi errori si vedono
 * la sera del tag: qui si fissano le regole che non devono tornare indietro,
 * a ogni push. Senza un lettore YAML (gli strumenti non hanno dipendenze): i
 * lavori si ritagliano per rientro, che in questi file è sempre di due spazi.
 *
 *   node --test "strumenti/*.test.js"
 */
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const REPO = path.dirname(__dirname);
const leggi = (rel) => fs.readFileSync(path.join(REPO, rel), "utf8").replace(/\r\n/g, "\n");
const RELEASE = leggi(".github/workflows/release.yml");
const VERIFY = leggi(".github/workflows/verify.yml");
const MSIX = leggi(".github/workflows/msix.yml");
const SITO = leggi(".github/workflows/sito.yml");

/** I lavori di un workflow: nome → testo, senza i commenti. */
function lavori(yml) {
  const corpo = yml.slice(yml.indexOf("\njobs:\n") + 7);
  const fuori = {};
  let nome = null;
  for (const riga of corpo.split("\n")) {
    const m = /^ {2}([\w-]+):\s*$/.exec(riga);
    if (m) {
      nome = m[1];
      fuori[nome] = "";
    } else if (nome && !/^\s*#/.test(riga)) fuori[nome] += `${riga}\n`;
  }
  return fuori;
}

/** I testi dei blocchi `run:` di un lavoro (in linea o `|`). */
function blocchiRun(testo) {
  const righe = testo.split("\n");
  const fuori = [];
  for (let i = 0; i < righe.length; i++) {
    const m = /^(\s*)(?:- )?run:\s*(.*)$/.exec(righe[i]);
    if (!m) continue;
    if (m[2] !== "|" && m[2] !== "") {
      fuori.push(m[2]);
      continue;
    }
    const rientro = m[1].length;
    const blocco = [];
    while (i + 1 < righe.length && (righe[i + 1].trim() === "" || righe[i + 1].search(/\S/) > rientro)) blocco.push(righe[++i]);
    fuori.push(blocco.join("\n"));
  }
  return fuori;
}

const R = lavori(RELEASE);
const V = lavori(VERIFY);

/* ── R20-6: la stessa toolchain dappertutto, e `--locked` anche in locale ── */

test("la toolchain di Rust è la stessa in tutti i workflow, e verify:ci è quella della CI", () => {
  const versioni = [VERIFY, RELEASE, MSIX].flatMap((yml) => [...yml.matchAll(/RUSTUP_TOOLCHAIN: "([^"]+)"/g)].map((m) => m[1]));
  assert.ok(versioni.length >= 5, versioni.join(" "));
  assert.equal(new Set(versioni).size, 1, `toolchain diverse: ${versioni.join(" ")}`);
  const script = JSON.parse(leggi("apps/desktop/package.json")).scripts;
  // `npm run verify` diceva «tutto quel che controlla la CI», ma senza `--locked`
  // riscriveva il lockfile in silenzio ed era verde dove la CI è rossa.
  assert.match(script.verify, /cargo clippy [^&]*--locked/);
  assert.match(script.verify, /cargo test [^&]*--locked/);
  assert.match(script["verify:ci"] ?? "", /strumenti\/come-la-ci\.js/);
  const { toolchainDellaCi } = require("./come-la-ci.js");
  assert.equal(toolchainDellaCi(VERIFY), versioni[0]);
});

/* ── R20-1: gli avvisi delle dipendenze consultivi nelle due corse di un tag ── */

test("nella release e in msix gli avvisi di sicurezza delle dipendenze non fermano la build, licenze e divieti sì", () => {
  assert.match(VERIFY, /consultivo:\n\s+description: [^\n]+\n\s+type: boolean\n\s+required: false\n\s+default: false/);
  const d = V.dipendenze;
  // Licenze, divieti e sorgenti: sempre bloccanti, dipendono dal codice.
  assert.match(d, /cargo deny [^\n]*check bans licenses sources/);
  // Gli avvisi e npm audit: consultivi solo quando lo chiede chi chiama.
  const passi = d.split(/\n\s+- (?=name:|uses:)/);
  const advisories = passi.find((p) => /check advisories/.test(p));
  const npm = passi.find((p) => /npm audit/.test(p) && /run:/.test(p));
  for (const p of [advisories, npm]) {
    assert.ok(p, d);
    assert.match(p, /continue-on-error: \$\{\{ inputs\.consultivo == true \}\}/);
  }
  assert.ok(!/check bans licenses sources[\s\S]*?continue-on-error/.test(passi.find((p) => /bans licenses sources/.test(p))));
  // L'esito sale a chi chiama, e la release lo scrive nella bozza.
  assert.match(VERIFY, /outputs:\n\s+avvisi:[\s\S]*?value: \$\{\{ jobs\.dipendenze\.outputs\.avvisi \}\}/);
  assert.match(R.verifica, /consultivo: true/);
  assert.match(R.pubblica, /needs\.verifica\.outputs\.avvisi/);
  // `msix.yml` verifica lo stesso tag con lo stesso `deny.toml`, e rilanciarlo non
  // cambia niente: un avviso uscito fra il tag e il lancio lo bloccherebbe fino a
  // un tag nuovo, senza che il pacchetto (nessuna chiave, nessun segreto) ne
  // abbia colpa (S10-12). Come la release, consultivo: l'esito si dice a chi lo
  // carica nel Store. I rami e le pull request restano bloccanti: il default.
  const M = lavori(MSIX);
  assert.match(M.verifica, /consultivo: true/);
  assert.match(M.pacchetto, /needs: \[tag, verifica\]/);
  // L'esito sale a chi carica il pacchetto: un'annotazione in cima al lavoro, e non solo nel registro di `verifica`.
  assert.match(M.pacchetto, /if: needs\.verifica\.outputs\.avvisi == 'trovati'/);
  assert.match(M.pacchetto, /::warning/);
  const avviso = blocchiRun(M.pacchetto).find((r) => /::warning/.test(r));
  assert.ok(avviso, "manca il passo che dice gli avvisi");
  // Il collegamento alla corsa passa dall'ambiente, e non da `${{ }}` dentro `run:`.
  assert.ok(!/\$\{\{/.test(avviso), `un'espressione dentro run:\n${avviso}`);
  // Un backtick dentro un echo fra virgolette è una sostituzione di comando: eseguirebbe il testo.
  assert.ok(!/`/.test(avviso), `un backtick dentro run:\n${avviso}`);
});

/* ── R20-2: tempi ── */

test("i tempi massimi reggono una build a freddo del grafo di oggi, senza staticlib", () => {
  const tetto = (testo) => Number(/timeout-minutes: (\d+)/.exec(testo)[1]);
  assert.ok(tetto(R.costruisci) >= 90, `costruisci: ${tetto(R.costruisci)}`);
  assert.ok(tetto(V.verifica) >= 60, `verifica: ${tetto(V.verifica)}`);
  assert.ok(tetto(R.apk) >= 90, `apk: ${tetto(R.apk)}`);
  const cargo = leggi("apps/desktop/src-tauri/Cargo.toml");
  assert.match(cargo, /^crate-type = \["cdylib", "rlib"\]$/m);
});

/* ── il telefono: il job APK ── */

test("l'APK si costruisce senza segreti, con l'NDK 30 inchiodato, e si controlla prima della firma", () => {
  const a = R.apk;
  assert.ok(a, "manca il lavoro apk");
  assert.match(a, /runs-on: ubuntu-24\.04/);
  assert.ok(!/secrets\./.test(a), "il lavoro che compila non vede segreti");
  assert.ok(!/environment:/.test(a));
  assert.match(a, /NDK_VERSIONE: "30\.0\.16248370"/);
  // Non quello di serie dell'immagine (il 27, che allinea a 4 KB senza errori).
  assert.ok(!/\$ANDROID_NDK_LATEST_HOME|\$\{?ANDROID_NDK_HOME\}?\/|NDK_HOME=\$ANDROID_NDK_HOME/.test(a));
  assert.match(a, /Pkg\.Revision = \$NDK_VERSIONE/);
  assert.match(a, /npx tauri android build --apk --split-per-abi --target aarch64 --ci/);
  assert.match(a, /keystore\.properties/);
  assert.match(a, /node strumenti\/apk\.js controlla [^\n]+--mapping/);
  assert.match(a, /git diff --exit-code -- Cargo\.lock/);
});

test("l'APK si firma nell'ambiente release, con la chiave che non tocca i log e non resta su disco", () => {
  const f = R["firma-apk"];
  assert.ok(f, "manca il lavoro firma-apk");
  assert.match(f, /environment: release/);
  assert.match(f, /needs: \[tag, apk\]/);
  for (const s of ["ANDROID_KEYSTORE_BASE64", "ANDROID_KEYSTORE_PASSWORD", "ANDROID_KEY_ALIAS"]) {
    assert.match(f, new RegExp(`${s}: \\$\\{\\{ secrets\\.${s} \\}\\}`), s);
  }
  for (const run of blocchiRun(f)) {
    assert.ok(!/\$\{\{/.test(run), `un'espressione dentro run:\n${run}`);
    assert.ok(!/set -x/.test(run));
  }
  const firma = blocchiRun(f).find((r) => /apksigner" sign/.test(r));
  assert.ok(firma, "manca apksigner sign");
  assert.match(firma, /umask 077/);
  assert.match(firma, /--ks-pass env:ANDROID_KEYSTORE_PASSWORD/);
  assert.match(firma, /--key-pass env:ANDROID_KEYSTORE_PASSWORD/);
  assert.match(firma, /--v4-signing-enabled false/);
  assert.match(firma, /\$RUNNER_TEMP\//);
  assert.match(f, /if: always\(\)\n\s+(?:shell: bash\n\s+)?run: rm -f "\$RUNNER_TEMP\/[\w.-]+\.jks"/);
  assert.match(f, /npx tauri signer sign --app-version/);
  assert.match(f, /node strumenti\/apk\.js verifica/);
  assert.match(f, /node strumenti\/firma\.js/);
});

test("la bozza porta insieme l'installer e l'APK, e il corpo per tutti e due", () => {
  const p = R.pubblica;
  assert.match(p, /needs: \[tag, verifica, firma, firma-apk\]/);
  assert.match(p, /name: rymembra-android\b/);
  assert.match(p, /name: rymembra-windows\b/);
  assert.match(p, /body_path: corpo\/corpo\.md/);
  assert.ok(!/\n\s+body: \|/.test(p), "il corpo è ancora scritto a mano nel workflow");
  assert.match(R.tag, /node strumenti\/corpo\.js corpo\/corpo\.md/);
  // Il corpo e il mapping di R8 non finiscono fra gli asset (`uscita/*`).
  assert.match(p, /name: rymembra-corpo\n\s+path: corpo\//);
  assert.match(R.apk, /name: rymembra-android-mapping\n\s+path: mapping-r8\//);
  assert.ok(!/mapping/.test(p));
});

/* ── il sito ── */

test("il sito descrive l'APK della release se c'è, e lo dice se non c'è", () => {
  const c = lavori(SITO).costruisci;
  assert.match(c, /_android-arm64\.apk/);
  assert.match(c, /--apk/);
  assert.ok(!/\|\| true/.test(c), "un errore di gh nascosto");
});

/* ── le azioni ── */

test("ogni azione è inchiodata a un hash", () => {
  for (const yml of [RELEASE, VERIFY, MSIX, SITO]) {
    for (const m of yml.matchAll(/uses: ([^\s]+)/g)) {
      if (m[1].startsWith("./")) continue;
      assert.match(m[1], /@[0-9a-f]{40}$/, m[1]);
    }
  }
});

/* ── S5-2: la firma si verifica contro i byte, prima di pubblicare ────── */

test("l'installer e l'APK si verificano contro i byte dopo la firma e prima di caricare l'artefatto", () => {
  const dopo = (testo, prima, poi) => {
    const a = testo.indexOf(prima);
    const b = testo.indexOf(poi);
    assert.ok(a >= 0, `manca «${prima}»`);
    assert.ok(b >= 0, `manca «${poi}»`);
    assert.ok(a < b, `«${prima}» deve venire prima di «${poi}»`);
  };
  // Il computer: firma, poi `manifesto.js` (che verifica le firme), poi l'artefatto che `pubblica` scarica.
  dopo(R.firma, "tauri signer sign", "node strumenti/manifesto.js");
  dopo(R.firma, "node strumenti/manifesto.js", "actions/upload-artifact");
  // Il telefono: firma minisign, poi `apk.js verifica`, poi l'artefatto.
  dopo(R["firma-apk"], "tauri signer sign", "node strumenti/apk.js verifica");
  dopo(R["firma-apk"], "node strumenti/apk.js verifica", "actions/upload-artifact");
  // E `pubblica` non parte senza che tutti e due siano passati.
  assert.match(R.pubblica, /needs: \[tag, verifica, firma, firma-apk\]/);
  // I due script fanno davvero la verifica: la prova è nel codice, non nel nome del passo.
  for (const script of ["manifesto.js", "apk.js"]) {
    assert.match(leggi(`strumenti/${script}`), /require\("\.\/minisign\.js"\)/, `${script} non usa minisign.js`);
  }
  assert.match(leggi("strumenti/manifesto.js"), /verificaFirma\(fs\.readFileSync/);
  assert.match(leggi("strumenti/apk.js"), /verificaFirma\(byte, sig, pubkey\)/);
});

/* ── S5-8: una minor senza Novità ferma il tag ────────────────────────── */

test("il lavoro tag pretende le Novità per una minor o una major, e le lascia libere a una correttiva", () => {
  const corsa = blocchiRun(R.tag).find((r) => /node strumenti\/novita\.js/.test(r));
  assert.ok(corsa, "il lavoro tag non lancia novita.js");
  // `--se-minor` e non il comando nudo, che per una 3.3.0 senza finestra darebbe solo un avviso.
  assert.match(corsa, /node strumenti\/novita\.js --se-minor/);
  assert.ok(!/node strumenti\/novita\.js\s*$/m.test(corsa), "novita.js nudo: la minor senza novità passerebbe con un avviso");
});
