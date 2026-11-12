/**
 * Il lunedì di chi guarda: quando comincia questa settimana, e quando la
 * prossima.
 *
 * Gli stessi conti che il nucleo fa con lo scostamento della finestra
 * (`rymembra_domain::settimana::lunedi`), qui con l'orologio locale: alla Home
 * servono a riconoscere che la settimana è cambiata e a sapere quando
 * svegliarsi per chiedere quella nuova. Con `new Date(anno, mese, giorno)` la
 * mezzanotte è quella locale anche la settimana del cambio dell'ora, che dura
 * un'ora in più o in meno: sommare sette giorni di millisecondi sbaglierebbe
 * proprio lì.
 */

/** Quanti giorni sono passati da lunedì: 0 il lunedì, 6 la domenica. */
function daLunedi(adesso: Date): number {
  return (adesso.getDay() + 6) % 7;
}

/** La mezzanotte locale del lunedì di questa settimana, in millisecondi. */
export function lunediLocale(adesso: Date): number {
  return new Date(
    adesso.getFullYear(),
    adesso.getMonth(),
    adesso.getDate() - daLunedi(adesso),
  ).getTime();
}

/** La mezzanotte locale del lunedì prossimo, in millisecondi. */
export function prossimoLunedi(adesso: Date): number {
  return new Date(
    adesso.getFullYear(),
    adesso.getMonth(),
    adesso.getDate() - daLunedi(adesso) + 7,
  ).getTime();
}
