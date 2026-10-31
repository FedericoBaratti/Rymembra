import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * L'indice delle Impostazioni porta dove promette.
 *
 * Ogni voce di `voci()` nomina la sezione che apre e la scheda in cui la cosa
 * cercata sta davvero. Due voci puntavano alla sezione sbagliata — «Arricchimento
 * dei tag» sotto Riproduzione, disegnato in Cartelle; «Accento dalla copertina»
 * sotto Aspetto, disegnato nella scheda della riproduzione — e nessun controllo
 * se n'era accorto: la ricerca apriva una pagina dove quel che si cercava non
 * c'era. Qui si legge il sorgente e si cerca, per ogni voce, il posto in cui la
 * sua scheda si disegna.
 */

const SORGENTE = readFileSync(
  new URL("../src/schermate/Impostazioni.tsx", import.meta.url),
  "utf8",
);

/** Le voci dell'indice: sezione, nome, chiave del titolo della scheda. */
function vociDellIndice(): { sezione: string; nome: string; scheda: string }[] {
  const corpo = SORGENTE.slice(SORGENTE.indexOf("function voci("));
  return [...corpo.matchAll(/\bv\("([a-z]+)", "([a-zA-Z]+)", "([a-zA-Z.]+)"\)/g)].map(
    ([, sezione, nome, scheda]) => ({ sezione: sezione!, nome: nome!, scheda: scheda! }),
  );
}

/** La funzione di primo livello che contiene `dove`. */
function funzioneIntorno(dove: number): string {
  const prima = SORGENTE.slice(0, dove);
  const trovate = [...prima.matchAll(/\n(?:export )?function ([A-Za-z]+)\(/g)];
  return trovate.at(-1)?.[1] ?? "";
}

/** La sezione del blocco `{sezione === "…" && …}` che precede `dove`. */
function sezioneIntorno(dove: number): string | null {
  const prima = SORGENTE.slice(0, dove);
  const trovate = [...prima.matchAll(/sezione === "([a-z]+)"/g)];
  return trovate.at(-1)?.[1] ?? null;
}

/** Le sezioni in cui si disegna il titolo `chiave`. */
function sezioniDellaScheda(chiave: string): Set<string> {
  const sezioni = new Set<string>();
  for (const trovato of SORGENTE.matchAll(new RegExp(`t\\("${chiave.replaceAll(".", "\\.")}"\\)`, "g"))) {
    const dove = trovato.index;
    const funzione = funzioneIntorno(dove);
    if (funzione === "Impostazioni") {
      const sezione = sezioneIntorno(dove);
      if (sezione) sezioni.add(sezione);
      continue;
    }
    // Una scheda che è un componente suo: conta dove lo si monta.
    for (const montato of SORGENTE.matchAll(new RegExp(`<${funzione}\\b`, "g"))) {
      const sezione = sezioneIntorno(montato.index);
      if (sezione) sezioni.add(sezione);
    }
  }
  return sezioni;
}

test("ogni voce dell'indice apre la sezione in cui la sua scheda si disegna", () => {
  const voci = vociDellIndice();
  assert.ok(voci.length >= 40, `l'indice ha solo ${voci.length} voci leggibili`);
  for (const { sezione, nome, scheda } of voci) {
    const dove = sezioniDellaScheda(scheda);
    assert.ok(
      dove.has(sezione),
      `«${nome}» apre «${sezione}», ma la scheda ${scheda} sta in: ${[...dove].join(", ") || "nessuna sezione"}`,
    );
  }
});

test("i testi e l'arricchimento si trovano cercando, nella sezione delle cartelle", () => {
  const voci = vociDellIndice();
  const di = (nome: string) => voci.find((v) => v.nome === nome)?.sezione;
  assert.equal(di("lyrics"), "cartelle");
  assert.equal(di("enrich"), "cartelle");
  assert.equal(di("accent"), "riproduzione");
});
