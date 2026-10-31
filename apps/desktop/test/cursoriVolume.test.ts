import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

/*
 * Ogni cursore del volume si annuncia in percentuale.
 *
 * Le copie erano due — la barra del lettore e la fascia del giudizio, che con
 * la terza colonna aperta o a schermo intero è l'unico cursore del volume — e
 * solo la prima aveva `aria-valuetext`: l'altra si annunciava «Volume, 0,72».
 * Qui si cercano nei sorgenti tutti gli `<input type="range">` col nome del
 * volume, e per ognuno si chiede la percentuale.
 */

const SRC = fileURLToPath(new URL("../src", import.meta.url));

function sorgenti(cartella: string): string[] {
  return readdirSync(cartella).flatMap((nome) => {
    const percorso = join(cartella, nome);
    if (statSync(percorso).isDirectory()) return sorgenti(percorso);
    return nome.endsWith(".tsx") ? [percorso] : [];
  });
}

test("ogni cursore del volume si annuncia in percentuale", () => {
  let trovati = 0;
  for (const file of sorgenti(SRC)) {
    const testo = readFileSync(file, "utf8");
    for (const m of testo.matchAll(/<input\b[\s\S]*?\/>/g)) {
      const input = m[0];
      if (!input.includes('aria-label={t("player.volume")}')) continue;
      trovati += 1;
      assert.ok(
        input.includes('aria-valuetext={t("player.volume.value"'),
        `${file}: cursore del volume senza aria-valuetext`,
      );
    }
  }
  assert.ok(trovati >= 2, `cursori trovati: ${trovati}`);
});
