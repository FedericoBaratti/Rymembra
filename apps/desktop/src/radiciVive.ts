/**
 * Le risposte della sonda delle radici, per chi non l'ha chiesta.
 *
 * La sonda (`cartelle_radici_vive`) la fanno le Cartelle — all'apertura, e
 * col loro «Ricontrolla» — e le Impostazioni, nella scheda delle cartelle. Chi
 * l'ha chiesta disegna la risposta; ma c'è chi deve saperla senza averla
 * chiesta: la fascia rossa di «Il file di questo brano non si raggiunge»,
 * nata quando una radice era staccata, restava lì anche dopo che la radice era
 * tornata e «Ricontrolla» lo aveva visto (R18-11).
 *
 * La lettura passa da [`conAnnuncio`] in `DATI_FINESTRA` (`dati.tsx`), cioè
 * per tutti quelli che la fanno con `useDati().ipc`: nessuno deve ricordarsi di
 * annunciarla, e l'anteprima dello Studio, che ha le sue letture finte, non
 * annuncia niente.
 */

/** Chi ascolta: le radici chieste, e per ognuna se risponde. */
export type AscoltoRadici = (radici: readonly string[], vive: readonly boolean[]) => void;

const ascolti = new Set<AscoltoRadici>();

/** Ascolta le risposte della sonda. Restituisce chi smette. */
export function quandoRispondonoLeRadici(fai: AscoltoRadici): () => void {
  ascolti.add(fai);
  return () => {
    ascolti.delete(fai);
  };
}

/** Dice a chi ascolta una risposta della sonda. */
export function annunciaRadici(radici: readonly string[], vive: readonly boolean[]): void {
  // Una copia: chi ascolta può smettere proprio adesso.
  for (const fai of [...ascolti]) fai(radici, vive);
}

/** La lettura della sonda, che annuncia quel che risponde. */
export function conAnnuncio(
  leggi: (radici: string[]) => Promise<boolean[]>,
): (radici: string[]) => Promise<boolean[]> {
  return async (radici) => {
    const vive = await leggi(radici);
    annunciaRadici(radici, vive);
    return vive;
  };
}

/**
 * La fascia d'errore va ritirata, adesso che la sonda ha risposto?
 *
 * Solo per «il file non si raggiunge» (`playback.sourceUnavailable`), che è il
 * guasto di una radice staccata; e solo se **tutte** le radici chieste
 * rispondono, perché la sonda dice delle radici e non del brano: con una
 * ancora spenta, il brano poteva stare proprio lì. Una sonda di niente non
 * dice niente.
 */
export function fasciaDaRitirare(codice: string | null | undefined, vive: readonly boolean[]): boolean {
  return codice === "playback.sourceUnavailable" && vive.length > 0 && vive.every(Boolean);
}

/** Le radici che non rispondono, dalla risposta della sonda. Una che manca nella risposta conta come viva. */
export function radiciSpente(radici: readonly string[], vive: readonly boolean[]): Set<string> {
  return new Set(radici.filter((_, i) => vive[i] === false));
}
