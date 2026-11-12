/**
 * Le due vie d'uscita da una skin che rende la finestra inservibile (D-48).
 *
 * Una skin valida può ancora spegnere la finestra in modi che l'esame non
 * vede tutti — un'ombra grande quanto lo schermo, un colore che sparisce su un
 * fondo che non si misura — e le Impostazioni, da cui la si toglierebbe,
 * stanno dentro la finestra che non si vede più. Fino alla 3.2.0 l'unica uscita
 * era cancellare un file nella cartella dei dati. Adesso ce ne sono due, e
 * nessuna dipende dalla skin:
 *
 * - **«Tieni questa skin?»**: chi indossa una skin nuova arrivata da un file
 *   ha quindici secondi per dire di sì; se non risponde — perché non vede
 *   niente, o non trova il tasto — torna da sé quella di prima. È la domanda
 *   che il sistema fa quando si cambia la risoluzione dello schermo, e per la
 *   stessa ragione. La domanda è un consenso, quindi la skin non la ridisegna.
 * - **Maiusc tenuto all'avvio**: la finestra si apre con Plain. La scelta si
 *   scrive, così al riavvio dopo resta Plain; una notizia dice cosa è successo
 *   e offre di rimettere quella di prima.
 *
 * Qui stanno le due regole pure, provate da `test/tieniSkin.test.ts`; lo stato
 * sta in `skinAttiva.ts`, la domanda in `ConfermaSkin.tsx`.
 */

/** Quanti secondi ha chi indossa una skin nuova per tenerla. */
export const SECONDI_PER_TENERE = 15;

/**
 * Per quanti millisecondi dopo che la finestra ha la sua skin un Maiusc tenuto
 * vale «aprila con Plain».
 */
export const FINESTRA_MAIUSC_MS = 5000;

/** I secondi che restano prima di tornare indietro, mai sotto zero. */
export function secondiRimasti(scadenza: number, ora: number): number {
  return Math.max(0, Math.ceil((scadenza - ora) / 1000));
}

/**
 * Un tasto premuto all'avvio è Maiusc **tenuto**?
 *
 * Tenuto, non premuto: chi tiene Maiusc mentre l'app si apre ci arriva con la
 * ripetizione del tasto già in corso, perché la finestra prende il fuoco col
 * tasto giù — e il primo evento che riceve è già una ripetizione. Una pressione
 * breve, per una scorciatoia nei primi secondi, non ripete e non conta.
 */
export function maiuscAllAvvio(
  evento: { key: string; repeat: boolean },
  msDallAvvio: number,
): boolean {
  return evento.key === "Shift" && evento.repeat && msDallAvvio >= 0 && msDallAvvio <= FINESTRA_MAIUSC_MS;
}
