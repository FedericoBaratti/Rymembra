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

/**
 * I passi a cui una barra può ridisegnarsi, dal più lungo. Dividono tutti il
 * secondo.
 */
const PASSI_DELLA_BARRA = [1000, 500, 250, 200, 125, 100, 50];

/**
 * Ogni quanto ha senso ridisegnare una barra di avanzamento larga `pixel`
 * pixel del dispositivo, su un brano di `durataMs`.
 *
 * # Perché la barra non segue i venti colpi al secondo
 *
 * Ogni ridisegno della barra è un fotogramma che la WebView compone per intero,
 * e costa. Misurato su `prima.exe` (30/10, Home, finestra normale): venti
 * ridisegni al secondo valevano il 7,5% di un core nel processo della GPU e il
 * 6% nel renderer, cioè quasi tutta la CPU della finestra mentre suona. La
 * sfocatura della tinta non c'entrava: tolta, il conto restava quello. Però su
 * un brano di quattro minuti, in una barra di trecento pixel, 50 ms spostano
 * il riempimento di un quindicesimo di pixel. Su venti ridisegni, diciannove
 * non cambiavano niente di visibile.
 *
 * Il passo è quindi il più lungo fra quelli che **dividono il secondo** e non
 * superano il tempo di un pixel del dispositivo. Che divida il secondo serve ai
 * minuti e secondi scritti ai lati: cambiano esattamente quando cambia il
 * secondo vero, e non un passo dopo. Mai sotto i 50 ms del ciclo che pubblica,
 * che più fitto non ha niente da dire. Con una misura che non c'è ancora
 * (barra non disegnata, durata sconosciuta) vale il passo più fitto, cioè il
 * comportamento di prima.
 */
export function passoDellaBarra(durataMs: number, pixel: number): number {
  const fitto = PASSI_DELLA_BARRA[PASSI_DELLA_BARRA.length - 1] ?? 50;
  if (!(durataMs > 0) || !(pixel > 0)) return fitto;
  const unPixel = durataMs / pixel;
  return PASSI_DELLA_BARRA.find((passo) => passo <= unPixel) ?? fitto;
}
