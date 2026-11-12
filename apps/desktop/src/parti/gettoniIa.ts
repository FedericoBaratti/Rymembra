/**
 * Il tetto dei gettoni di un profilo IA, come lo si scrive nel modulo (D-64).
 *
 * I bordi sono quelli del nucleo (`rymembra_ia::GETTONI_AMMESSI`, e il tetto
 * di serie `GETTONI_DI_SERIE`): qui servono a dirlo mentre si scrive, invece di
 * scoprirlo da un rifiuto al salvataggio. A decidere resta il nucleo, che
 * rifiuta un numero fuori dai bordi invece di tagliarlo.
 */

/** Il tetto di serie, quando il campo è vuoto. */
export const GETTONI_DI_SERIE = 4096;
/** Il meno che si può chiedere: sotto, non ci sta nemmeno un blocco di modifiche. */
export const GETTONI_MINIMI = 64;
/** Il più che si può chiedere: oltre non è una scelta, è un errore di battitura. */
export const GETTONI_MASSIMI = 262_144;

/**
 * Il numero scritto nel campo.
 *
 * `null` per un campo vuoto (vale il tetto di serie), `undefined` per quel che
 * non è un numero intero dentro i bordi: il modulo non salva, e lo dice.
 */
export function gettoniDalCampo(testo: string): number | null | undefined {
  const pulito = testo.trim();
  if (pulito === "") return null;
  if (!/^\d+$/.test(pulito)) return undefined;
  const numero = Number(pulito);
  return numero >= GETTONI_MINIMI && numero <= GETTONI_MASSIMI ? numero : undefined;
}

/** Il campo di un profilo salvato: vuoto se usa il tetto di serie. */
export function campoDeiGettoni(salvati: number | null | undefined): string {
  return salvati === null || salvati === undefined ? "" : String(salvati);
}
