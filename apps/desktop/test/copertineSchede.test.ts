/**
 * Le copertine dei brani di un altro dispositivo nelle schede del sistema (FR2,
 * richiesta #1 dell'utente).
 *
 * Dopo «Sfoglia» le copertine di un altro dispositivo stanno nello store di questo,
 * e le chiede la **finestra**, quando le mostra. Chi guida col telefono in tasca
 * la finestra non la guarda: la scheda di Android Auto, la notifica, la schermata
 * di blocco e le SMTC di Windows leggono lo store, e di un brano del PC che nessuna
 * finestra ha mai mostrato non trovavano niente. `media/lontane.rs` chiede la
 * copertina una volta sola, in disparte, con la stessa funzione della finestra
 * (`copertina_di_una_riga`), e a copertina arrivata la scheda si ripubblica.
 *
 * Il comportamento (una richiesta sola, la ripubblicazione, il tempo di riprova) lo
 * provano i test Rust di `media/lontane.rs` sulla cucitura; qui si guarda che i
 * pezzi siano collegati dove devono, perché il collegamento sta in codice che solo
 * il telefono e Windows compilano.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const leggi = (percorso: string) =>
  readFileSync(new URL(percorso, import.meta.url), "utf8").replace(/\r\n/g, "\n");
const LONTANE = leggi("../src-tauri/src/media/lontane.rs");
const ANDROID = leggi("../src-tauri/src/media/android.rs");
const SMTC = leggi("../src-tauri/src/media/smtc.rs");

/** Il corpo di una funzione, fino alla prima graffa chiusa a capo di riga. */
function corpo(testo: string, firma: string): string {
  const inizio = testo.indexOf(firma);
  assert.ok(inizio >= 0, `manca ${firma}`);
  return testo.slice(inizio, testo.indexOf("\n}\n", inizio));
}

test("la copertina si chiede con la funzione della finestra e finisce nello store: niente seconda strada", () => {
  const scarica = corpo(LONTANE, "fn scarica(&self, impronta: &str) {");
  assert.match(scarica, /crate::lontano::copertina_di_una_riga\(&self\.0, impronta\)/);
  // Lo store è quello di sempre: la piena o la media, come la guarda l'auto (`auto::copertina`).
  assert.match(
    corpo(LONTANE, "fn nello_store(&self, impronta: &str) -> bool {"),
    /store\.path_for\(impronta\)\.is_file\(\) \|\| store\.percorso_media\(impronta\)\.is_file\(\)/,
  );
  // Nessuna rete, nessuna cache, nessuna chiave `lontano-` in questo modulo.
  assert.doesNotMatch(LONTANE.split("#[cfg(test)]")[0] ?? "", /Domanda::|nodo\(|lontano-|copertine-lontane/);
});

test("Android chiede la copertina di una riga dell'altro dispositivo che lo store non ha, e ricompone scheda e coda", () => {
  const di = corpo(ANDROID, "fn copertina_di(");
  assert.match(di, /nello_store\.is_none\(\) && chiedi && super::lontane::e_di_un_altro_dispositivo\(percorso\)/);
  assert.match(di, /super::lontane::richiedi\(app, hash, copertina_arrivata\)/);
  // La voce di sessione non passa di qui: ha la sua strada, prima.
  assert.ok(di.indexOf("e_una_chiave_di_sessione") < di.indexOf("lontane::richiedi"));
  // All'arrivo il tratto si rifà (il filo della coda lo ricompone, adesso con la copertina) e lo stato si ripubblica.
  const arrivata = corpo(ANDROID, "fn copertina_arrivata(");
  assert.match(arrivata, /controlli\.tratto = None;/);
  assert.match(arrivata, /crate::riproduzione::insieme::aggiorna_finestra\(app\)/);
  // La scheda del brano che suona chiede; la coda, solo per il brano dopo.
  assert.match(ANDROID, /copertina: copertina_di\(\s*app,\s*copertine,\s*brano\.cover_art_hash\.as_deref\(\),\s*brano\.path\.as_deref\(\),\s*true,\s*true,\s*\)/);
  assert.match(ANDROID, /riga\.path\.as_deref\(\),\s*false,\s*Some\(riga\.id\) == prossimo,/);
  assert.match(ANDROID, /struct Tratto \{\s*inizio: usize,\s*brani: Vec<i64>,\s*prossimo: Option<i64>,\s*\}/);
  assert.match(ANDROID, /prossimo: posto\.and_then\(\|dove\| coda\.get\(dove\.saturating_add\(1\)\)\.copied\(\)\)/);
});

test("le SMTC chiedono la copertina mancante e ripubblicano la scheda all'arrivo", () => {
  const file = corpo(SMTC, "fn file_della_copertina(");
  assert.match(file, /super::lontane::e_di_un_altro_dispositivo\(percorso\)/);
  assert.match(file, /super::lontane::richiedi\(app, hash, crate::riproduzione::insieme::aggiorna_finestra\)/);
  assert.match(SMTC, /file_della_copertina\(app, copertine, hash, brano\.path\.as_deref\(\)\)/);
  assert.ok(file.indexOf("e_una_chiave_di_sessione") < file.indexOf("lontane::richiedi"));
});

test("chiede solo la riga di un altro dispositivo, e solo se manca", () => {
  assert.match(
    corpo(LONTANE, "pub(super) fn e_di_un_altro_dispositivo("),
    /percorso\.is_some_and\(rymembra_app::specchio::e_specchio\)/,
  );
  const richiedi = corpo(LONTANE, "pub(super) fn richiedi<A: Ambiente>(");
  assert.match(richiedi, /ambiente\.nello_store\(impronta\) \|\| !self\.comincia\(impronta\)/);
});
