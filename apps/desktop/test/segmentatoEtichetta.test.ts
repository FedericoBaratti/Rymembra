import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * Le linguette con un nome a schermo (R18-4).
 *
 * Nella scheda «Riproduzione» c'erano l'interruttore «L'accento segue la
 * copertina» e sotto, senza titolo, «Spenta · Bassa · Normale · Alta» (il
 * ReplayGain) e «Automatico · Disco · Brano»; nella scheda dello spettro
 * «Automatica · Alta · Bassa». Il nome del gruppo era solo un `aria-label`:
 * chi guardava non sapeva che cosa sceglieva. Adesso `Segmentato` lo disegna,
 * e il gruppo prende il nome da quel testo (`aria-labelledby`).
 */
const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");

/** Il `<Segmentato` che ha per etichetta la chiave data, fino alla sua chiusura. */
function segmentato(sorgente: string, chiave: string): string {
  const dove = sorgente.indexOf(`etichetta={t("${chiave}")}`);
  assert.ok(dove > 0, `non trovo ${chiave}: la prova va aggiornata`);
  const da = sorgente.lastIndexOf("<Segmentato", dove);
  return sorgente.slice(da, sorgente.indexOf("/>", dove));
}

test("i gruppi che stanno con altri nella stessa scheda hanno l'etichetta a schermo", () => {
  const impostazioni = leggi("../src/schermate/Impostazioni.tsx");
  for (const chiave of ["settings.replaygain.label", "settings.loudness.mode.label"]) {
    assert.match(segmentato(impostazioni, chiave), /\betichettaVisibile\b/, chiave);
  }
  const spettro = leggi("../src/parti/SchedaSpettro.tsx");
  assert.match(segmentato(spettro, "settings.spectrum.quality.label"), /\betichettaVisibile\b/);
});

test("con l'etichetta a schermo il gruppo prende il nome da lei", () => {
  const sorgente = leggi("../src/parti/Segmentato.tsx");
  assert.match(sorgente, /aria-labelledby=\{etichettaVisibile \? idEtichetta : undefined\}/);
  assert.match(sorgente, /id=\{idEtichetta\}/);
});
