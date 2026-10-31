/**
 * Le sorgenti della copertina (`$source`) che un token di colore può usare.
 *
 * Tutte tranne una: `color.accent`. Le sorgenti puntano tutte a `--accent`
 * (`DynamicSource::variabili` nel crate skin), quindi `color.accent: { "$source":
 * … }` è un accento che rimanda a sé stesso e resta senza colore — il
 * validatore lo rifiuta (`sorgente_su_se_stessa` in `document.rs`). Offrire il
 * modo nello Studio voleva dire offrire un errore di validazione. L'accento
 * segue la copertina per un'altra strada: `capabilities.dynamicAccent`, e la
 * descrizione del token lo dice.
 */

/** Il token che non può seguire la copertina con `$source`. */
const ACCENTO = "color.accent";

export function sorgentiPer(token: string | undefined, sorgenti: readonly string[]): readonly string[] {
  return token === ACCENTO ? [] : sorgenti;
}
