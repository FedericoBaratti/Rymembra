import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * La copia del Microsoft Store e la libreria che c'era (R20-3, D-56/D-97).
 *
 * La cartella separata la prova il nucleo (`lib.rs`,
 * `la_copia_dello_store_non_apre_la_libreria_della_copia_nsis`), la copia della
 * libreria pure (`trasloco::copia_la_libreria_di`). Qui la metà che un DOM
 * nelle prove non c'è per vedere: la proposta è montata nella finestra, fuori
 * dal telefono; sta SOPRA il primo avvio (D-97, collaudo in Windows Sandbox: la
 * finestrella c'era nell'albero di accessibilità e non si vedeva); chiede prima
 * di copiare; Escape e il velo la chiudono «per questa volta», e solo «Comincia
 * da capo» la rifiuta per sempre; e arriva a comandi registrati.
 */

const leggi = (percorso: string) =>
  readFileSync(new URL(percorso, import.meta.url), "utf8");

const APP = leggi("../src/App.tsx");
const PROPOSTA = leggi("../src/parti/LibreriaDiPrima.tsx");
const IPC = leggi("../src/ipc.ts");
const LIB = leggi("../src-tauri/src/lib.rs");
const FINESTRELLE = leggi("../src/stile/finestrelle.css");

/** Il testo di `sorgente` da `da` fino a `fino`, per leggere un solo pezzo. */
function tratto(sorgente: string, da: string, fino: string): string {
  const inizio = sorgente.indexOf(da);
  assert.notEqual(inizio, -1, `non trovo «${da}»`);
  const fine = sorgente.indexOf(fino, inizio + da.length);
  assert.notEqual(fine, -1, `non trovo «${fino}» dopo «${da}»`);
  return sorgente.slice(inizio, fine);
}

test("la proposta è montata, e non sul telefono", () => {
  assert.match(APP, /\{!MOBILE && <LibreriaDiPrima onErrore=\{segnalaErrore\} \/>\}/);
});

test("la proposta non sta dentro la pagina: sta accanto al primo avvio, che la copre", () => {
  // Il primo avvio (`.primo`) copre la finestra a `--strato-primo`, e il velo di
  // una finestrella sta a `--strato-velo`: per stare sopra, la proposta deve
  // essere **fratella** di `.primo` — la regola in `finestrelle.css` guarda i
  // fratelli — e non dentro `.dentro.page-content`, dove oltre allo strato
  // pesano `inert` (a schermo intero) e la pagina che si smonta e si rimonta.
  const primo = APP.indexOf("<Primo");
  const proposta = APP.indexOf("<LibreriaDiPrima");
  const impaginazione = APP.indexOf("<Impaginazione");
  assert.ok(primo !== -1 && proposta !== -1 && impaginazione !== -1);
  assert.ok(primo < proposta, "la proposta viene dopo il primo avvio nel DOM");
  assert.ok(proposta < impaginazione, "la proposta sta fuori dall'impaginazione della pagina");
  assert.ok(
    !tratto(APP, 'className="dentro page-content"', "{corpo()}").includes("<LibreriaDiPrima"),
    "la proposta è ancora dentro page-content",
  );
});

test("a primo avvio aperto ogni finestrella scura sale sopra di lui", () => {
  // Una regola sola per tutte, e non un guscio per ognuna: oltre alla proposta,
  // una skin o una playlist lasciate cadere sul primo avvio aprono la loro
  // finestrella — `ConfermaSkin`, `ImportaPlaylist` — dallo stesso punto.
  const regola = FINESTRELLE.match(
    /:where\(\.finestra-app:has\(> \.primo\)\) > \.velo\.scuro \{([^}]*)\}/,
  );
  assert.ok(regola, "manca la regola del velo scuro accanto al primo avvio");
  assert.match(regola[1] ?? "", /z-index:\s*var\(--strato-sopraprimo\)/);
  // E viene prima di quella del consenso, che a parità di peso vince e sta più su.
  const sopra = FINESTRELLE.indexOf(":where(.finestra-app:has(> .primo))");
  const consenso = FINESTRELLE.indexOf(".velo:has(> [data-consenso])");
  assert.ok(sopra !== -1 && consenso !== -1 && sopra < consenso);
});

test("copiare si chiede, e «Comincia da capo» è l'unico rifiuto per sempre", () => {
  const rifiuta = tratto(PROPOSTA, "const rifiuta", "const copia");
  assert.ok(rifiuta.includes("ipc.storeRifiutaLibreriaDiPrima()"));
  const copia = PROPOSTA.slice(PROPOSTA.indexOf("const copia"));
  assert.ok(copia.includes(".storeCopiaLibreriaDiPrima()"));
  // Mentre copia non si chiude: chiusa a metà, nessuno racconterebbe l'esito.
  assert.match(PROPOSTA, /chiudibile=\{!copiando\}/);
  // Il tasto «Comincia da capo» è quello che rifiuta.
  assert.match(
    tratto(PROPOSTA, "<button", '{t("store.previous.skip")}'),
    /onClick=\{rifiuta\}/,
  );
});

test("Escape e il velo chiudono per questa volta, e la domanda torna al prossimo avvio", () => {
  // Una proposta che si fa una volta sola e che salva una libreria intera non
  // si perde per un clic fuori posto — e fuori posto lo era davvero: nel
  // collaudo il primo clic sulla scheda del primo avvio è caduto sul velo
  // invisibile della finestrella e l'ha chiusa. Il nucleo ricorda `rifiutata`
  // solo se glielo si dice (`store_rifiuta_libreria_di_prima`).
  assert.match(PROPOSTA, /onChiudi=\{chiudiPerOra\}/);
  assert.doesNotMatch(PROPOSTA, /onChiudi=\{rifiuta\}/);
  const perOra = tratto(PROPOSTA, "const chiudiPerOra", "const rifiuta");
  assert.ok(perOra.includes("setLibreria(null)"), "la finestrella si chiude");
  assert.ok(
    !perOra.includes("storeRifiuta"),
    "chiudere per questa volta non deve scrivere il rifiuto",
  );
});

test("i comandi ci sono", () => {
  for (const comando of [
    "store_libreria_di_prima",
    "store_copia_libreria_di_prima",
    "store_rifiuta_libreria_di_prima",
  ]) {
    assert.ok(IPC.includes(`("${comando}")`), `ipc.ts non chiama ${comando}`);
    assert.ok(LIB.includes(`manutenzione::${comando},`), `lib.rs non registra ${comando}`);
  }
});
