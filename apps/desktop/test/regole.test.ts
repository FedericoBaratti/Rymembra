import { test } from "node:test";
import assert from "node:assert/strict";

import {
  creaProva,
  durataDaCampo,
  durataInCampo,
  limiteDaCampo,
  numeroDaCampo,
} from "../src/editorRegole";

/** Una domanda finta, che risponde quando la si risolve a mano. */
function domande<R>() {
  const aperte: { risolvi: (r: R) => void; rifiuta: (e: unknown) => void }[] = [];
  const chiedi = () =>
    new Promise<R>((risolvi, rifiuta) => {
      aperte.push({ risolvi, rifiuta });
    });
  return { chiedi, aperte };
}

const giro = () => new Promise((fatto) => setTimeout(fatto, 0));

test("vince l'ultima anteprima, non l'ultima arrivata", async () => {
  const { chiedi, aperte } = domande<string>();
  const scritte: string[] = [];
  const prova = creaProva(chiedi, { risposta: (r) => scritte.push(r), errore: () => undefined });
  prova("beatl");
  prova("beatles");
  aperte[1]?.risolvi("beatles");
  await giro();
  aperte[0]?.risolvi("beatl");
  await giro();
  assert.deepEqual(scritte, ["beatles"]);
});

test("un'anteprima riuscita toglie l'errore dell'anteprima, e un errore vecchio non torna", async () => {
  const { chiedi, aperte } = domande<string>();
  let errore: unknown = null;
  let anteprima: string | null = null;
  const prova = creaProva(chiedi, {
    risposta: (r) => {
      anteprima = r;
      errore = null;
    },
    errore: (e) => {
      errore = e;
    },
  });
  prova("a");
  aperte[0]?.rifiuta("db.locked");
  await giro();
  assert.equal(errore, "db.locked");
  prova("ab");
  prova("abc");
  aperte[2]?.risolvi("abc");
  await giro();
  aperte[1]?.rifiuta("db.locked");
  await giro();
  assert.equal(anteprima, "abc");
  assert.equal(errore, null);
});

test("un numero in una regola è un intero, e un campo vuoto è incompleto", () => {
  assert.equal(numeroDaCampo("3.5"), 3);
  assert.equal(numeroDaCampo("-2.7"), -2);
  assert.equal(numeroDaCampo("12"), 12);
  assert.equal(numeroDaCampo(""), null);
  assert.equal(numeroDaCampo("  "), null);
  assert.equal(numeroDaCampo("1e20"), Number.MAX_SAFE_INTEGER);
  assert.equal(limiteDaCampo("2.5"), 2);
  assert.equal(limiteDaCampo("1e10"), 4294967295);
  assert.equal(limiteDaCampo("-4"), 0);
  assert.equal(limiteDaCampo(""), null);
});

test("la durata si scrive in minuti e secondi, e si salva in millisecondi", () => {
  assert.equal(durataDaCampo("5:30"), 330_000);
  assert.equal(durataDaCampo("5"), 300_000);
  assert.equal(durataDaCampo("0:45"), 45_000);
  assert.equal(durataDaCampo(" 12:05 "), 725_000);
  assert.equal(durataDaCampo(""), null);
  assert.equal(durataDaCampo("5:"), null);
  assert.equal(durataDaCampo("5:75"), null);
  assert.equal(durataDaCampo("cinque"), null);
  assert.equal(durataInCampo(330_000), "5:30");
  assert.equal(durataInCampo(0), "0:00");
  assert.equal(durataInCampo(null), "");
});

test("una durata salvata che non è a secondi interi torna identica", () => {
  // Regole scritte quando il campo era in millisecondi: mostrate e risalvate
  // senza toccarle, non devono cambiare.
  for (const ms of [5, 330_500, 299_999, 61_010]) {
    const testo = durataInCampo(ms);
    assert.equal(durataDaCampo(testo), ms, `${ms} → «${testo}»`);
  }
  assert.equal(durataInCampo(330_500), "5:30.5");
});

// ── R13b-1: finché le regole non sono in mano, niente si salva ─────────────

import { readFileSync } from "node:fs";
import { entrano, salvabile, TETTO_REGOLE } from "../src/editorRegole";

const sorgente = (file: string) => readFileSync(new URL(file, import.meta.url), "utf8");

test("non si salva prima che le regole siano arrivate, né senza nome, né senza una condizione", () => {
  const pronto = { caricato: true, nome: "Rock anni 70", salvando: false, condizioni: 3 };
  assert.equal(salvabile(pronto), true);
  // Il caso di R13b-1: la lettura non è arrivata (o è fallita) e l'editor
  // mostrerebbe una condizione vuota, cioè «tutta la libreria».
  assert.equal(salvabile({ ...pronto, caricato: false }), false);
  assert.equal(salvabile({ ...pronto, nome: "   " }), false);
  assert.equal(salvabile({ ...pronto, salvando: true }), false);
  // D-22: il nucleo rifiuta una playlist intelligente senza condizioni.
  assert.equal(salvabile({ ...pronto, condizioni: 0 }), false);
});

test("Regole.tsx: il tasto e le condizioni aspettano le regole lette", () => {
  const regole = sorgente("../src/Regole.tsx");
  // Il tasto primario chiede a `salvabile`, che guarda `caricato`.
  const tasto = /className="bottone primario"[\s\S]{0,200}?disabled=\{([^}]*)\}/.exec(regole);
  assert.ok(tasto, "il tasto primario c'è");
  assert.equal((tasto[1] ?? "").trim(), "!siSalva");
  assert.match(regole, /const siSalva = salvabile\(\{[^}]*caricato/);
  // L'elenco delle condizioni si disegna solo con le regole in mano.
  const elenco = regole.indexOf("insieme.regole.map(");
  const cancello = regole.lastIndexOf("caricato ?", elenco);
  assert.ok(elenco > 0 && cancello > 0, "le condizioni stanno dietro `caricato ?`");
  // E una lettura fallita si può rifare.
  assert.match(regole, /common\.retry/);
});

// ── R25-1, D-100: il tetto resta, e si dice ────────────────────────────────

test("il tetto dell'editor è quello del nucleo", () => {
  const smart = sorgente("../../../core/rymembra-app/src/smart.rs");
  const nucleo = /pub const TETTO: u32 = ([\d_]+);/.exec(smart);
  assert.ok(nucleo, "smart::TETTO c'è");
  assert.equal(Number((nucleo[1] ?? "").replaceAll("_", "")), TETTO_REGOLE);
});

test("ne entrano al più il limite, e mai più del tetto", () => {
  assert.equal(entrano(12_300, null), TETTO_REGOLE);
  assert.equal(entrano(12_300, 10_000), TETTO_REGOLE);
  assert.equal(entrano(12_300, 50), 50);
  assert.equal(entrano(40, 50), 40);
  assert.equal(entrano(40, null), 40);
  assert.equal(entrano(40, 0), 0);
});

test("Regole.tsx: «ne entrano» anche senza un limite scritto", () => {
  const regole = sorgente("../src/Regole.tsx");
  assert.match(regole, /entrano\(anteprima\.quanti, insieme\.limite\)/);
  assert.doesNotMatch(regole, /anteprima\.quanti > insieme\.limite/);
});
