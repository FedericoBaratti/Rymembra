/**
 * La regione delle date e delle ore (D-42).
 *
 * `Intl` riceveva soltanto il codice della lingua dell'interfaccia — `en` —
 * quindi chi usa l'interfaccia in inglese con il sistema in inglese britannico,
 * australiano o indiano vedeva le date all'americana (`11/10/2026` per il 10
 * novembre) e l'orologio a dodici ore in cronologia, nelle Impostazioni e nei
 * rapporti. Ora le date seguono la regione del sistema quando la sua lingua è
 * la stessa dell'interfaccia: `en-GB` sotto un'interfaccia inglese, `it-CH`
 * sotto una italiana. Con una lingua diversa — un sistema tedesco con
 * l'interfaccia in italiano — resta la lingua dell'interfaccia: una data
 * tedesca in mezzo a frasi italiane si leggerebbe come un errore.
 *
 * Solo le date e le ore: i numeri restano quelli della lingua, perché una
 * regione come `en-IN` raggruppa le cifre a modo suo (`12,34,567`), e un
 * conteggio di brani scritto così sotto un'interfaccia inglese stupirebbe.
 *
 * Puro: `test/regione.test.ts` lo prova con `node --test`.
 */

/** La lingua di un codice: `en-GB` → `en`. */
function radice(codice: string): string {
  return (codice.split("-")[0] ?? "").toLowerCase();
}

/**
 * Il codice da passare a `Intl` per le date: la prima delle lingue del sistema
 * che parla la lingua dell'interfaccia, o la lingua dell'interfaccia.
 */
export function regioneDelleDate(lingua: string, delSistema: readonly string[]): string {
  const voluta = radice(lingua);
  const trovata = delSistema.find((codice) => codice.includes("-") && radice(codice) === voluta);
  return trovata ?? lingua;
}
