/**
 * Un archivio della posizione di riproduzione, fuori da React.
 *
 * Il perché dell'archivio esterno sta in `riproduzione.ts`, «La posizione,
 * fuori da React»: venti volte al secondo si ridisegna solo chi la legge.
 *
 * Sta in un modulo suo, senza import, perché di archivi ce n'è più d'uno: quello
 * della finestra, che `useRiproduzione` alimenta dai colpi del nucleo, e quello
 * dell'anteprima dello Studio, che muove un trasporto finto. Chi disegna la
 * posizione non sa quale dei due sta leggendo — lo sceglie il contesto `Dati`.
 */

/** Quel che serve a chi disegna la posizione. */
export interface ArchivioPosizione {
  /** Per `useSyncExternalStore`: identità stabile, per tutta la vita. */
  iscrivi: (avvisa: () => void) => () => void;
  /** La posizione adesso, in millisecondi. */
  leggi: () => number;
}

/** Un archivio che si può anche scrivere: lo tiene chi lo alimenta. */
export interface ArchivioScrivibile extends ArchivioPosizione {
  /** Una posizione nuova. Avvisa solo se è cambiata. */
  pubblica: (ms: number) => void;
}

export function creaArchivioPosizione(iniziale = 0): ArchivioScrivibile {
  let corrente = iniziale;
  const ascoltatori = new Set<() => void>();
  return {
    iscrivi: (avvisa) => {
      ascoltatori.add(avvisa);
      return () => {
        ascoltatori.delete(avvisa);
      };
    },
    leggi: () => corrente,
    pubblica: (ms) => {
      if (ms === corrente) return;
      corrente = ms;
      for (const avvisa of ascoltatori) avvisa();
    },
  };
}

/** L'archivio della finestra: uno, perché la riproduzione è una. */
export const ARCHIVIO_FINESTRA = creaArchivioPosizione();
