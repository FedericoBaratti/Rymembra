import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * iTunes e Deezer spenti di serie, con la nota sui termini accanto
 * all'interruttore (D-10a, D-10b); le somiglianze di ListenBrainz con un
 * interruttore loro (D-94). Il valore viene dal nucleo: qui si controlla che
 * la scheda «Metadati» li disegni, con la nota, e che finché lo stato non è
 * arrivato li mostri spenti.
 */
const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");

const impostazioni = leggi("../src/schermate/Impostazioni.tsx");
const en = JSON.parse(leggi("../src/lingue/en.json")) as Record<string, string>;
const it = JSON.parse(leggi("../src/lingue/it.json")) as Record<string, string>;

for (const [chiave, campo, fonte] of [
  ["settings.enrich.itunes", "itunes", "itunes"],
  ["settings.enrich.deezer", "deezer", "deezer"],
  ["settings.enrich.similar", "vicini", "vicini"],
] as const) {
  test(`«${chiave}»: interruttore con la sua nota, spento finché non si sa`, () => {
    const dove = impostazioni.indexOf(`t("${chiave}")`);
    assert.ok(dove > 0, `manca l'interruttore ${chiave}`);
    const corpo = impostazioni.slice(dove, impostazioni.indexOf("/>", dove));
    assert.match(corpo, new RegExp(`spiegazione=\\{t\\("${chiave}\\.note"\\)\\}`));
    assert.match(corpo, new RegExp(`acceso=\\{arricchimento\\?\\.${campo} \\?\\? false\\}`));
    assert.match(corpo, new RegExp(`onArricchimentoFonte\\?\\.\\("${fonte}"`));
    for (const lingua of [en, it]) {
      assert.ok(lingua[chiave] && lingua[`${chiave}.note`], `testo mancante per ${chiave}`);
    }
  });
}

test("le note dicono che iTunes e Deezer sono spenti di serie, e perché", () => {
  assert.match(en["settings.enrich.itunes.note"] ?? "", /^Off by default\. Apple’s terms/);
  assert.match(en["settings.enrich.deezer.note"] ?? "", /^Off by default\. Deezer’s terms/);
  assert.match(it["settings.enrich.itunes.note"] ?? "", /^Spento di serie\. I termini di Apple/);
  assert.match(it["settings.enrich.deezer.note"] ?? "", /^Spento di serie\. I termini di Deezer/);
});

test("il gesto arriva al nucleo", () => {
  assert.match(leggi("../src/ipc.ts"), /invoke<StatoArricchimento>\("arricchimento_fonte", \{ fonte, attiva \}\)/);
  assert.match(leggi("../src/ImpostazioniFinestra.tsx"), /onArricchimentoFonte=\{\(fonte, attiva\) =>/);
});
