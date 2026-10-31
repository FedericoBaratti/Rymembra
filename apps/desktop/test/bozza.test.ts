import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { haUnPacchetto, idDelFormato, scarto } from "../src/studio/bozza";
import { idDa } from "../src/studio/nuovo";

const sorgente = (file: string) => readFileSync(new URL(`../src/studio/${file}`, import.meta.url), "utf8");

test("il campo dell'id segue la regola del formato, non quella delle cartelle", () => {
  // La stessa tabella di `l_id_dichiarato_segue_la_regola_del_formato` in
  // `studio/tests.rs`, che lega la copia del nucleo a `document.rs`. Con la regola
  // di prima del campo (`/^[A-Za-z0-9_-]{1,64}$/`) «Notturno» si scriveva nel
  // documento, il nucleo spostava la bozza e la validazione lo rifiutava.
  const lungo = "a".repeat(48);
  const troppo = "a".repeat(49);
  const tabella: [string, boolean][] = [
    ["a", false],
    ["ab", true],
    ["Ab", false],
    ["a_b", false],
    ["1ab", false],
    ["-ab", false],
    ["a-b", true],
    ["notturno", true],
    ["Notturno", false],
    ["n0tte", true],
    [lungo, true],
    [troppo, false],
    ["àb", false],
    ["ab\n", false],
    ["", false],
  ];
  for (const [candidato, atteso] of tabella) {
    assert.equal(idDelFormato(candidato), atteso, `«${candidato}»`);
  }
});

test("la regola dell'id è ancora quella che il nucleo scrive", () => {
  // Se il formato cambia la regola cambia anche la sua frase, e questa prova
  // ricorda che la copia della finestra va riallineata.
  const documento = readFileSync(
    new URL("../../../core/rymembra-skin/src/document.rs", import.meta.url),
    "utf8",
  );
  assert.match(documento, /l'id ammette da 2 a 48 fra minuscole, cifre e trattini, e comincia con una lettera/);
});

test("gli id che la finestrella del tema nuovo deriva da un nome passano il campo", () => {
  for (const nome of ["Notturno", "2001", "A", "夜曲", "Più notte che giorno", "x".repeat(80)]) {
    const id = idDa(nome, []);
    assert.ok(idDelFormato(id), `«${nome}» → «${id}»`);
  }
});

test("il campo dell'id usa la regola del formato", () => {
  const voci = sorgente("IspezioneVoci.tsx");
  const inizio = voci.indexOf("export function IspezioneDocumento(");
  const documento = voci.slice(inizio, voci.indexOf("\n}\n", inizio));
  assert.match(documento, /const idValido = idDelFormato\(id\);/);
  assert.doesNotMatch(voci, /A-Za-z0-9_-\]\{1,64\}/);
});

test("«Butta la bozza» sa se la bozza ha un pacchetto dall'elenco delle skin", () => {
  const elenco = [{ id: "plain" }, { id: "vinile" }, { id: "rotta" }];
  assert.equal(haUnPacchetto("plain", elenco), true, "di serie");
  assert.equal(haUnPacchetto("rotta", elenco), true, "installata, anche se non passa più la validazione");
  assert.equal(haUnPacchetto("mia", elenco), false, "rinominata: nessuna skin ha il suo id");
  assert.equal(haUnPacchetto("mia", null), null, "l'elenco non è ancora arrivato");
});

test("«Butta la bozza» si spegne su una bozza senza pacchetto", () => {
  // Studio aperto su Plain, id cambiato in «mia»: il testo differisce
  // dall'originale, ma non c'è un pacchetto a cui tornare. Il nucleo adesso
  // rifiuta senza toccare il disco; la finestra non deve offrire il gesto.
  assert.equal(scarto(true, false), "senzaPacchetto");
  // Prima di «nessuna bozza da buttare»: quella frase dice che il documento
  // è quello del pacchetto, e un pacchetto non c'è.
  assert.equal(scarto(false, false), "senzaPacchetto");
  assert.equal(scarto(true, true), "si");
  assert.equal(scarto(false, true), "niente");
  // Finché l'elenco non c'è (o non si è letto) vale la regola di prima: la
  // guardia vera è nel nucleo, e un tasto spento per un elenco che non arriva
  // sarebbe un vicolo cieco.
  assert.equal(scarto(true, null), "si");
  assert.equal(scarto(false, null), "niente");
});

test("il tasto «Butta la bozza» passa da scarto e dice perché è spento", () => {
  const variabili = sorgente("Variabili.tsx");
  const istantanee = variabili.slice(variabili.indexOf("function Istantanee("));
  assert.match(istantanee, /ipc\s*\.skinElenco\(\)/);
  assert.match(istantanee, /scarto\(derivata, haUnPacchetto\(s\.id, elenco[A-Za-z]*\)\)/);
  assert.match(istantanee, /studio\.pkg\.drop\.noPackage/);
});
