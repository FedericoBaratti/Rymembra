import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * La voce dell'indice e il titolo della scheda dicono la stessa cosa (R18-10).
 *
 * «Dispositivi» apriva «Collegamento via internet», «Importa da un servizio»
 * apriva «Da dove arriva la musica», «Movimento e accesso» apriva sulle
 * «Scorciatoie da tastiera»: chi cercava una cosa leggeva un altro nome, e si
 * chiedeva se era nel posto giusto.
 */
const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");
const SORGENTE = leggi("../src/schermate/Impostazioni.tsx");
const LINGUE = {
  it: JSON.parse(leggi("../src/lingue/it.json")) as Record<string, string>,
  en: JSON.parse(leggi("../src/lingue/en.json")) as Record<string, string>,
};

/** Le schede disegnate in ogni sezione, nell'ordine del sorgente. */
function schedePerSezione(): Map<string, string[]> {
  const fuori = new Map<string, string[]>();
  let sezione: string | null = null;
  for (const m of SORGENTE.matchAll(/sezione === "([a-z]+)"|titolo=\{t\("([a-zA-Z.]+)"\)\}/g)) {
    if (m[1]) sezione = m[1];
    else if (sezione && m[2]) {
      const elenco = fuori.get(sezione) ?? [];
      if (!elenco.includes(m[2])) elenco.push(m[2]);
      fuori.set(sezione, elenco);
    }
  }
  return fuori;
}

test("una sezione con una scheda sola le dà il suo nome", () => {
  const schede = schedePerSezione();
  let viste = 0;
  for (const [sezione, elenco] of schede) {
    if (elenco.length !== 1) continue;
    viste += 1;
    for (const [lingua, voci] of Object.entries(LINGUE)) {
      assert.equal(
        voci[elenco[0]!],
        voci[`settings.section.${sezione}`],
        `${lingua}: la sezione «${sezione}» e la sua scheda ${elenco[0]}`,
      );
    }
  }
  assert.ok(viste >= 5, `solo ${viste} sezioni a scheda sola: la prova va aggiornata`);
});

test("«Movimento e accesso» apre sulla scheda che le dà il nome", () => {
  assert.equal(schedePerSezione().get("movimento")?.[0], "settings.motion.title");
});
