import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * Frasi con più numeri che non scrivono «1 brani», «1 bozze», «1 doppioni»
 * (R13c-14).
 *
 * `t` sceglie il plurale su `n` soltanto; queste frasi hanno due o tre numeri
 * con altri nomi, e accordavano il nome a un numero fisso. Si scrivono in una
 * forma che non accorda («bozze: 1»).
 */
const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");
const LINGUE = {
  it: JSON.parse(leggi("../src/lingue/it.json")) as Record<string, string>,
  en: JSON.parse(leggi("../src/lingue/en.json")) as Record<string, string>,
};

const CHIAVI = [
  "restore.playlist.count",
  "restore.skinsAndDrafts",
  "restore.keptAsIs",
  "account.skipped",
];

test("con tutti i numeri a 1 nessun nome resta al plurale", () => {
  for (const [lingua, voci] of Object.entries(LINGUE)) {
    for (const chiave of CHIAVI) {
      const testo = voci[chiave] ?? "";
      assert.ok(testo.length > 0, `${lingua}: manca ${chiave}`);
      const conUno = testo.replace(/\{\w+\}/g, "1");
      assert.doesNotMatch(
        conUno,
        /\b1 (brani|skin|bozze|doppioni|tracks|skins|drafts|duplicates)\b/,
        `${lingua} ${chiave}: «${conUno}»`,
      );
      assert.doesNotMatch(conUno, /\b1 of 1 tracks\b/, `${lingua} ${chiave}`);
    }
  }
});
