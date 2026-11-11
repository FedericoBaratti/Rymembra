/**
 * L'elenco delle cartelle del primo avvio: quelle che il sistema propone,
 * contate, e in fondo quelle indicate a mano, **senza** conteggio.
 *
 * # Perché senza conteggio, e perché un `null`
 *
 * Contare una cartella scelta col dialogo o trascinata vorrebbe dire
 * camminarci dentro prima di «Continua», cioè una scansione per sapere se fare
 * la scansione. Il commento lo diceva da sempre; il codice però le metteva
 * `brani: 0`, e la riga disegnava «0 brani» accanto alla cartella della musica
 * di chi l'aveva appena scelta — anche dopo che la scansione ne aveva trovati
 * tredici. `null` dice «non contata», e la riga allora non scrive niente, come
 * per le cartelle del cloud.
 *
 * Puro, senza lingue né React: `test/primo.test.ts` lo prova con `node --test`.
 */
import type { CartellaCandidata } from "./ipc";

/** Una riga dell'elenco: come la propone il nucleo, o indicata a mano. */
export type CartellaDelPrimo = Omit<CartellaCandidata, "brani"> & {
  /** Quanti brani ci si sono contati; `null` per una cartella non contata. */
  brani: number | null;
};

/** L'elenco con in fondo le cartelle indicate a mano che non c'erano già. */
export function conAMano(
  elenco: CartellaDelPrimo[],
  percorsi: readonly string[],
): CartellaDelPrimo[] {
  const mancano = percorsi.filter((p) => !elenco.some((c) => c.percorso === p));
  // Lo stesso elenco quando non cambia niente: un elenco nuovo ridisegnerebbe
  // la schermata per nulla.
  if (mancano.length === 0) return elenco;
  return [
    ...elenco,
    ...mancano.map((percorso) => ({
      percorso,
      brani: null,
      troncato: false,
      parziale: false,
    })),
  ];
}
