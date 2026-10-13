/**
 * Da dove una pagina prende quel che non le arriva per prop.
 *
 * # Perché un contesto
 *
 * Perché lo Studio disegna le **pagine vere** — la Home, gli Album, le
 * Cartelle, Esplora, il testo — con dati finti, e le pagine vere non si fanno
 * passare tutto dal genitore: alcune cose le chiedono da sé, in un effetto. Le
 * Cartelle leggono i figli di una cartella quando la si apre, Esplora cerca
 * mentre si scrive, il pannello del testo chiede il testo del brano. Nella
 * finestra quelle domande vanno al nucleo; nell'anteprima devono andare a un
 * archivio finto, altrimenti un tema che si sta disegnando leggerebbe il disco
 * di chi lo disegna — e ci scriverebbe, nel caso di `impostaCartelleUi`.
 *
 * Il valore di serie è la finestra: fuori dallo Studio nessuno fornisce niente,
 * e ogni pagina fa esattamente quel che faceva.
 *
 * # Cosa c'è, e cosa no
 *
 * Ci sono le **letture che partono da un effetto**, e le poche scritture che
 * partono allo stesso modo. Le scritture che partono da un gesto — un clic,
 * un tasto — non ci sono: nell'anteprima i gesti non arrivano alle pagine, li
 * ferma la tela prima (vedi il blocco degli eventi dello Studio).
 */
import { listen } from "@tauri-apps/api/event";
import { createContext, useContext } from "react";

import { urlCopertina } from "./aspetto";
import { ipc } from "./ipc";
import { ARCHIVIO_FINESTRA, type ArchivioPosizione } from "./posizione";

/** Le domande al nucleo che una pagina fa da sé. */
export type LettureDati = Pick<
  typeof ipc,
  | "cartelleFiglie"
  | "cartelleFile"
  | "cartelleBrani"
  | "impostaCartelleUi"
  | "esploraCerca"
  | "testoCerca"
  | "testoCercaDiNuovo"
  | "testiPannello"
  | "spettro"
  | "spettroVisibile"
  | "spettroBande"
  | "spettroQualita"
  | "braniPerId"
  | "eqPresetElenco"
  | "cronologia"
  | "cronologiaConteggio"
  | "cartelleCandidate"
>;

/** Le misure della finestra, o della tela che la finge. */
export interface Misura {
  larghezza: number;
  altezza: number;
}

export interface Dati {
  /** Si sta disegnando dentro l'anteprima dello Studio. */
  anteprima: boolean;
  ipc: LettureDati;
  /** L'indirizzo di una copertina: vedi `urlCopertina`. */
  copertina: (hash: string | null, miniatura?: boolean) => string | null;
  /** La posizione di riproduzione: vedi `posizione.ts`. */
  posizione: ArchivioPosizione;
  /**
   * Ascolta un evento del nucleo. Restituisce come si smette, come `listen`:
   * `useAscolto` in `pagine.ts` passa di qui.
   */
  ascolta: <T>(evento: string, gestore: (carico: T) => void) => Promise<() => void>;
  /**
   * Quanto è grande la finestra, **adesso**.
   *
   * Una funzione e non un numero: chi la chiede lo fa in risposta a un gesto
   * o a un ridimensionamento, e un numero nel contesto ridisegnerebbe ogni
   * pagina a ogni pixel di trascinamento del bordo.
   */
  misura: () => Misura;
}

function ascoltaIlNucleo<T>(evento: string, gestore: (carico: T) => void) {
  return listen<T>(evento, (arrivato) => gestore(arrivato.payload));
}

/** La finestra: il nucleo, il disco, la riproduzione vera. */
export const DATI_FINESTRA: Dati = {
  anteprima: false,
  ipc,
  copertina: urlCopertina,
  posizione: ARCHIVIO_FINESTRA,
  ascolta: ascoltaIlNucleo,
  misura: () => ({ larghezza: window.innerWidth, altezza: window.innerHeight }),
};

export const DatiContesto = createContext<Dati>(DATI_FINESTRA);

export function useDati(): Dati {
  return useContext(DatiContesto);
}
