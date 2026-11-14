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
 *   e offre di rimettere quella di prima. Il tasto lo guarda il nucleo, dalla
 *   comparsa della finestra (`rymembra-barra/src/maiusc.rs`); la pagina chiede
 *   com'è andata.
 *
 * Qui stanno le regole pure, provate da `test/tieniSkin.test.ts`; lo stato
 * sta in `skinAttiva.ts`, la domanda in `ConfermaSkin.tsx`.
 */

/** Quanti secondi ha chi indossa una skin nuova per tenerla. */
export const SECONDI_PER_TENERE = 15;

/** I secondi che restano prima di tornare indietro, mai sotto zero. */
export function secondiRimasti(scadenza: number, ora: number): number {
  return Math.max(0, Math.ceil((scadenza - ora) / 1000));
}

/**
 * Che cosa il nucleo sa di Maiusc all'avvio: ancora in corso (la finestra non
 * si vede da abbastanza tempo per decidere), tenuto, oppure no. `tenuto` si dice
 * una volta sola per processo.
 */
export type EsitoMaiusc = "inCorso" | "tenuto" | "no";

/** Ogni quanto la pagina richiede, finché il nucleo non ha deciso. */
export const PASSO_MAIUSC_MS = 200;

/**
 * Oltre quanto la pagina smette di richiedere. Il nucleo decide entro cinque
 * secondi dalla comparsa della finestra, che compare al più due secondi dopo
 * l'avvio: se dopo dodici secondi non ha detto niente, la finestra non si vede
 * e non c'è niente da aspettare.
 */
export const ATTESA_MAIUSC_MS = 12_000;

/**
 * Maiusc era tenuto mentre Rymembra si apriva?
 *
 * La domanda è del nucleo e non della pagina (F10-2): la finestra compare
 * prima che la skin sia pronta, e un `keydown` arriva solo a una pagina
 * caricata e col fuoco, quindi chi tiene Maiusc fino alla comparsa e poi lo
 * lascia non lo avrebbe mai visto. Il nucleo guarda il tasto dal sistema, dalla
 * comparsa della finestra (`rymembra-barra/src/maiusc.rs`); qui si chiede com'è
 * andata, e si richiede finché risponde «in corso».
 *
 * Gli argomenti sono quel che la pagina ha — la domanda, l'attesa, l'orologio —
 * perché la regola si provi senza aspettare. Un errore del nucleo non si
 * inghiotte: chi chiama lo mostra.
 */
export async function maiuscTenutoAllAvvio(
  chiedi: () => Promise<EsitoMaiusc>,
  attendi: (ms: number) => Promise<void>,
  adesso: () => number,
  tetto: number = ATTESA_MAIUSC_MS,
): Promise<boolean> {
  const inizio = adesso();
  for (;;) {
    const esito = await chiedi();
    if (esito === "tenuto") return true;
    if (esito === "no") return false;
    if (adesso() - inizio >= tetto) return false;
    await attendi(PASSO_MAIUSC_MS);
  }
}

/**
 * Il nome di una skin dall'elenco, o `undefined` se non c'è: l'elenco può non
 * essere ancora arrivato, e chi lo cerca deve saperlo — il nome non si inventa
 * con l'id, che è quel che la notizia di Maiusc scriveva fra virgolette.
 */
export function nomeDellaSkin(
  elenco: readonly { id: string; nome: string }[],
  id: string,
): string | undefined {
  return elenco.find((voce) => voce.id === id)?.nome;
}
