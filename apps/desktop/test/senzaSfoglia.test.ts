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
