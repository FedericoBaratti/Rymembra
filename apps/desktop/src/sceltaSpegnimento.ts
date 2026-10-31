/**
 * Quale linguetta del timer di spegnimento è accesa quando la scheda si monta.
 *
 * # Perché serve
 *
 * La scelta vive nella scheda e non nel nucleo (il perché è in
 * `SchedaSpegnimento`: quel che il nucleo sa è quanto manca, e quanto manca
 * cala). Ma la scheda si smonta a ogni cambio di sezione delle Impostazioni, e
 * ripartire da «spento» voleva dire una linguetta «Spento» accesa sopra la nota
 * «Mancano 27:12» — e un lettore di schermo che legge «Spento, selezionato» con
 * un timer che corre.
 *
 * Quindi al montaggio, **una volta sola**, la scelta si ricava dal nucleo: un
 * timer che non c'è è «spento», la fine del brano è «brano», e per un timer a
 * minuti vale l'ultima scelta fatta in questa finestra se torna coi minuti
 * rimasti, altrimenti la durata più piccola che li contiene. Una volta sola,
 * perché derivarla a ogni aggiornamento la farebbe scivolare da «30» a «15»
 * mentre la musica suona, che è quel che il commento della scheda vuole evitare.
 */

/** Le durate fra cui si sceglie, in minuti. */
export const DURATE_SPEGNIMENTO = [15, 30, 60] as const;

/**
 * La linguetta da accendere: `"spento"`, `"brano"` o una delle durate in
 * stringa.
 *
 * `ms` è quel che dice il nucleo (`null` nessun timer, `0` fine del brano,
 * altrimenti i millisecondi che mancano); `ricordata` è l'ultima linguetta
 * scelta in questa finestra, o `null`.
 */
export function sceltaIniziale(ms: number | null, ricordata: string | null): string {
  if (ms === null) return "spento";
  if (ms === 0) return "brano";
  const minuti = ms / 60_000;
  const durate: readonly number[] = DURATE_SPEGNIMENTO;
  if (ricordata !== null) {
    const scelti = Number(ricordata);
    // Compatibile: è una delle durate, e quel che manca ci sta dentro.
    if (durate.includes(scelti) && minuti <= scelti) return ricordata;
  }
  const minima = durate.find((d) => minuti <= d);
  return String(minima ?? durate[durate.length - 1]);
}
