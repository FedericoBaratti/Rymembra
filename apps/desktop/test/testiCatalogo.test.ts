import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { formaPlurale } from "../src/lingue/plurali";

/*
 * Le frasi del catalogo che dicono qualcosa sul programma, controllate contro
 * il programma: un testo che promette un servizio spento, che consiglia un
 * sistema che non c'è, o che contraddice la frase accanto è un difetto come un
 * altro, e nessun compilatore lo vede.
 */
const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");
const LINGUE = {
  it: JSON.parse(leggi("../src/lingue/it.json")) as Record<string, string>,
  en: JSON.parse(leggi("../src/lingue/en.json")) as Record<string, string>,
};

test("i testi non promettono servizi spenti", () => {
  // `nuvola_offerti` offre MEGA con `pronto = false`: il dialogo lo mostra «in
  // arrivo» e non lo apre. Finché è così, la frase dell'elenco vuoto non può
  // mandare l'utente a sceglierlo.
  const nucleo = leggi("../src-tauri/src/nuvola_musica.rs");
  const mega = /offerto\(Servizio::Mega, \w+, "\w+", (true|false)\)/.exec(nucleo);
  assert.ok(mega, "non trovo come si offre MEGA: la prova va aggiornata");
  if (mega[1] === "true") return;
  for (const [lingua, voci] of Object.entries(LINGUE)) {
    assert.doesNotMatch(voci["settings.cloudAccounts.empty"] ?? "", /MEGA/, lingua);
  }
});

test("gli errori non danno consigli per Linux a un programma solo per Windows", () => {
  const toolchain = leggi("../../../rust-toolchain.toml");
  const bersagli = /targets = \[([^\]]*)\]/.exec(toolchain)?.[1] ?? "";
  assert.ok(bersagli.includes("windows"), "rust-toolchain.toml non dice i bersagli: la prova va aggiornata");
  if (/linux/.test(bersagli)) return;
  for (const [lingua, voci] of Object.entries(LINGUE)) {
    for (const [chiave, testo] of Object.entries(voci)) {
      if (chiave.startsWith("errors.")) assert.doesNotMatch(testo, /Linux/, `${lingua} ${chiave}`);
    }
  }
});

test("i plurali italiani hanno la forma per uno: «1 segnato», «ne entra 1»", () => {
  for (const [lingua, voci] of Object.entries(LINGUE)) {
    const cerca = (chiave: string) => voci[chiave] ?? null;
    for (const chiave of ["page.library.liked", "rules.fitting"]) {
      assert.ok(voci[`${chiave}.uno`], `${lingua}: manca ${chiave}.uno`);
    }
    const preferiti = formaPlurale(cerca, lingua, "page.library.liked", 1) ?? "";
    const entrano = formaPlurale(cerca, lingua, "rules.fitting", 1) ?? "";
    if (lingua === "it") {
      assert.doesNotMatch(preferiti, /segnati/);
      assert.doesNotMatch(entrano, /entrano/);
    } else {
      assert.doesNotMatch(entrano, /\bfit$/);
    }
  }
});

test("il Cestino che non riesce non parla della rete, che il Cestino non lo usa", () => {
  // Un file di rete non passa mai dal Cestino (`cancellazione.rs`): senza
  // conferma si ferma su `fs.noRecycleBin`, che dice che Rymembra lo cancella
  // dopo averlo chiesto. `fs.trashFailed` che dice «una cartella di rete …
  // Rymembra non lo fa al posto tuo» contraddiceva la frase accanto.
  for (const [lingua, voci] of Object.entries(LINGUE)) {
    assert.ok(voci["errors.fs.noRecycleBin"], lingua);
    assert.doesNotMatch(voci["errors.fs.trashFailed"] ?? "", /rete|network/i, lingua);
  }
});
