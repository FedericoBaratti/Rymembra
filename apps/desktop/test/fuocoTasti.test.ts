import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * I tasti delle Impostazioni che lavorano non si spengono con `disabled`
 * (R12-6).
 *
 * Un tasto che diventa `disabled` mentre ha il fuoco lo perde, e il fuoco
 * cade sul documento: a lavoro finito il Tab riparte dalla cima della
 * finestra, e chi usa un lettore di schermo non sente più niente della scheda.
 * È la regola che `InRiproduzione.tsx` scrive e applica: il tasto dice
 * `aria-disabled` e `aria-busy`, e il clic in più si ignora. Qui si controlla,
 * tasto per tasto, che la condizione «sta lavorando» non stia in `disabled`.
 * Le condizioni che non cambiano premendo il tasto (nessuna cartella, niente
 * da svuotare) restano in `disabled`: lì il fuoco non c'è.
 */
const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");

/** Il tasto che disegna `chiave`: dall'apertura del `<button` fino alla chiave. */
function tasto(sorgente: string, chiave: string): string {
  const dove = sorgente.indexOf(`t("${chiave}"`);
  assert.ok(dove > 0, `non trovo ${chiave}: la prova va aggiornata`);
  return sorgente.slice(sorgente.lastIndexOf("<button", dove), dove);
}

function controlla(sorgente: string, chiave: string, occupato: string) {
  const corpo = tasto(sorgente, chiave);
  const spento = /\sdisabled=\{([^}]*)\}/.exec(corpo)?.[1] ?? "";
  assert.ok(
    !spento.includes(occupato),
    `«${chiave}»: \`${occupato}\` sta in disabled={${spento.trim()}}`,
  );
  // Il segno «sta lavorando» sta in `aria-disabled`, così chi lo legge sa che
  // adesso non si può premere.
  const dentro = /\saria-disabled=\{([^}]*)\}/.exec(corpo)?.[1] ?? "";
  assert.ok(dentro.includes(occupato), `«${chiave}»: aria-disabled={${dentro.trim()}}`);
}

test("i tasti delle Impostazioni che lavorano restano raggiungibili", () => {
  const impostazioni = leggi("../src/schermate/Impostazioni.tsx");
  const tasti: [string, string][] = [
    ["settings.roots.scanning", "scansione !== null"],
    ["settings.maintenance.cleaning", "pulendo"],
    ["settings.cloudCache.clearing", "svuotando"],
    ["settings.cloud.saving", "nuvola.inCorso"],
    ["settings.cloud.restore", "nuvola.inCorso"],
    ["settings.cloud.disconnect", "nuvola.inCorso"],
    ["settings.sync.syncing", "sincronia?.inCorso"],
    ["settings.enrich.passRunning", "arricchimento?.inCorso"],
  ];
  for (const [chiave, occupato] of tasti) controlla(impostazioni, chiave, occupato);
});

test("chi lavora lo dice con aria-busy, e il clic in più si ignora", () => {
  const impostazioni = leggi("../src/schermate/Impostazioni.tsx");
  for (const chiave of [
    "settings.roots.scanning",
    "settings.maintenance.cleaning",
    "settings.cloudCache.clearing",
    "settings.cloud.saving",
    "settings.sync.syncing",
  ]) {
    assert.match(tasto(impostazioni, chiave), /aria-busy=/, chiave);
  }
});
