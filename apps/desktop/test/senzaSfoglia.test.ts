/**
 * «Sfoglia» è diventato un filtro (§4.8): di quella pagina non resta niente.
 *
 * La pagina `schermate/Dispositivo.tsx`, la vista `"dispositivo"`, i comandi
 * `lontano_*` con cui la finestra chiedeva gli elenchi dell'altro, le maniglie
 * `Lontana` e la coda `player.queue.remote` sono tolti. Restano i pezzi che
 * servono ancora al server — un dispositivo con una versione di prima chiede ai
 * nostri gli elenchi con le domande `Album`, `Brani`, … — e le maniglie delle
 * sessioni di «Ascolta insieme».
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const RADICE = new URL("..", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const leggi = (percorso: string) => readFileSync(join(RADICE, percorso), "utf8");

function sorgentiTs(cartella: string): string[] {
  const trovati: string[] = [];
  for (const voce of readdirSync(join(RADICE, cartella), { withFileTypes: true })) {
    const percorso = join(cartella, voce.name);
    if (voce.isDirectory()) trovati.push(...sorgentiTs(percorso));
    else if (/\.tsx?$/.test(voce.name)) trovati.push(percorso);
  }
  return trovati;
}

test("la pagina di «Sfoglia» non c'è più", () => {
  assert.ok(!existsSync(join(RADICE, "src/schermate/Dispositivo.tsx")));
});

test("la vista e la pagina `dispositivo` non esistono più", () => {
  for (const file of ["src/pagina.ts", "src/schedeTelefono.ts", "src/parti/Navigazione.tsx", "src/App.tsx"]) {
    assert.doesNotMatch(leggi(file), /["']dispositivo["']\s*[|;,)]|vista === "dispositivo"|tipo: "dispositivo" \}/, file);
  }
  // Il filtro per dispositivo, invece, c'è: `OrigineLibreria` ha il suo tipo `dispositivo`.
  assert.match(leggi("src/ipc/libreria.ts"), /\{ tipo: "dispositivo"; chiave: string \}/);
});

test("la finestra non ha più i comandi con cui sfogliava l'altro dispositivo", () => {
  const ipc = leggi("src/ipc.ts");
  for (const comando of [
    "lontano_album",
    "lontano_album_artista",
    "lontano_artisti",
    "lontano_brani",
    "lontano_brani_album",
    "lontano_cerca",
    "lontano_cerca_conteggio",
    "lontano_suona",
    "lontano_accoda",
    "lontano_dopo",
    "lontano_scarica",
  ]) {
    assert.ok(!ipc.includes(`"${comando}"`), comando);
  }
  assert.doesNotMatch(ipc, /lontanoSuona|lontanoAccoda|lontanoDopo|lontanoBraniAlbum|lontanoAlbumArtista|lontanoCercaConteggio/);
  // «Scarica qui» passa dalle righe della libreria.
  assert.match(ipc, /lontanoScaricaRighe/);
  assert.match(ipc, /invoke<ScaricoLontano>\("lontano_scarica_righe"/);
});

test("nessun sorgente della finestra importa la pagina o i suoi tipi", () => {
  for (const file of sorgentiTs("src")) {
    const testo = leggi(file);
    assert.doesNotMatch(testo, /schermate\/Dispositivo["']|from "\.\/Dispositivo"/, file);
    assert.doesNotMatch(testo, /\b(BranoRemoto|OrdineRemoto|SegmentoRemoto)\b/, file);
  }
});

test("il server risponde ancora alle domande di un dispositivo con una versione di prima", () => {
  // Un APK vecchio sfoglia **questo** dispositivo con `Album`, `Brani`, `Cerca`…:
  // togliere le risposte romperebbe le versioni miste (§12).
  const servizio = leggi("src-tauri/src/lontano/servizio.rs");
  assert.match(servizio, /Domanda::Album/);
  assert.match(servizio, /Domanda::Brani\b/);
});

test("le maniglie e la coda salvata di «Sfoglia» sono tolte dal nucleo", () => {
  const sfoglia = leggi("src-tauri/src/lontano/sfoglia.rs");
  assert.doesNotMatch(sfoglia, /pub async fn lontano_(album|artisti|brani|cerca|suona|accoda|dopo)/);
  const ospiti = leggi("src-tauri/src/riproduzione/ospiti.rs");
  assert.doesNotMatch(ospiti, /registra_lontana|struct Lontana\b/);
  const coda = leggi("src-tauri/src/riproduzione/coda.rs");
  assert.doesNotMatch(coda, /save_queue_remote|riapri_la_coda_intera/);
  // La coda salvata di prima passa alle righe specchio una volta, all'avvio.
  assert.match(coda, /migra_coda_lontana/);
  const lib = leggi("src-tauri/src/lib.rs");
  assert.doesNotMatch(lib, /sfoglia::lontano_/);
});

test("il codice delle copertine di «Sfoglia» è tolto (FR2), e restano i pezzi che servono ancora", () => {
  // Le voci di «Sfoglia» portavano come copertina la chiave `lontano-<dispositivo>-<impronta>`,
  // che si scaricava e si teneva in `copertine-lontane/<dispositivo>/`. Con la libreria unica le
  // copertine di un altro dispositivo stanno nello store (`copertina_di_una_riga`), sotto la loro
  // impronta, e quella chiave non la scrive più nessuno.
  const sfoglia = leggi("src-tauri/src/lontano/sfoglia.rs");
  for (const morto of [
    /PREFISSO_COPERTINA/,
    /fn e_una_copertina/,
    /fn parti_della_copertina/,
    /fn percorso_in_cache/,
    /fn percorso_per_android/,
    /fn file_in_cache/,
    /fn copertina_in\b/,
    /fn pota\b/,
    /static IN_CORSO/,
    /struct Posto\b/,
    /COPERTINE_INSIEME/,
  ]) {
    assert.doesNotMatch(sfoglia, morto, `sfoglia.rs ha ancora ${morto}`);
  }
  // `media/lontane.rs` c'è ancora, ma è un'altra cosa: non chiede più la chiave `lontano-` ma
  // l'impronta di una riga della libreria unica, con `copertina_di_una_riga`, e il file sta nello
  // store (vedi `copertineSchede.test.ts`).
  const lontane = leggi("src-tauri/src/media/lontane.rs");
  assert.doesNotMatch(lontane, /lontano-|sfoglia::|percorso_in_cache|file_in_cache/);
  for (const file of ["media/android.rs", "media/smtc.rs", "copertine.rs", "lontano/mod.rs"]) {
    const testo = leggi(`src-tauri/src/${file}`);
    assert.doesNotMatch(testo, /e_una_copertina|lontane::file_di|sfoglia::percorso_per_android/, file);
  }
  assert.doesNotMatch(leggi("src-tauri/src/copertine.rs"), /risposta_lontana/);

  // Restano: la cartella `copertine-lontane` e la seconda radice di Kotlin servono alle copertine
  // di sessione (`insieme/su_file.rs`), e `togli_le_copertine_di` toglie i file di prima.
  assert.match(sfoglia, /pub\(crate\) fn cartella_copertine/);
  assert.match(sfoglia, /pub\(crate\) fn togli_le_copertine_di/);
  assert.match(leggi("src-tauri/src/insieme/su_file.rs"), /lontane\/\{CARTELLA\}\/\{nome\}/);
  assert.match(
    leggi("src-tauri/gen/android/app/src/main/java/io/github/federicobaratti/rymembra/Copertine.kt"),
    /private const val LONTANE = "lontane"/,
  );
  // E il server: la domanda `Copertina` serve anche l'APK vecchio che sfoglia questo dispositivo.
  assert.match(leggi("src-tauri/src/lontano/servizio.rs"), /Domanda::Copertina/);
});
