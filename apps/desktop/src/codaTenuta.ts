/**
 * La coda che la finestra tiene in mano, e come si tiene d'accordo col nucleo.
 *
 * Il nucleo non manda più la coda intera a ogni `riproduzione:stato` (S7-5): con
 * cinquantamila brani in coda erano ~350 KB di JSON a ogni cambio di brano, a ogni
 * pausa e, con la freccia della barra di avanzamento tenuta premuta, trenta volte
 * al secondo. L'evento porta la **versione** della coda (`versioneCoda`) e gli
 * identificativi (`coda`) solo se sono cambiati dall'ultimo evento spedito.
 *
 * Qui la regola, pura: la finestra tiene l'ultima coda che ha ricevuto con la sua
 * versione. Un evento con la coda la sostituisce; uno senza, con la stessa
 * versione, la lascia dov'è — **con lo stesso array**, perché la lista della coda
 * confronta per identità e un array nuovo a ogni stato la farebbe ridisegnare; uno
 * senza coda e con una versione che la finestra non ha vuol dire che l'evento con
 * la coda non le è arrivato (la finestra non c'era, era in ricarica): la chiede,
 * con `ipc.codaDelLettore`.
 */

import type { CodaDelLettore, StatoRiproduzione, StatoRiproduzioneEvento } from "./ipc";

/** La coda tenuta, e la versione con cui il nucleo l'ha numerata. */
export interface CodaTenuta {
  versione: number;
  coda: number[];
}

/**
 * Da dove si parte: la coda vuota che il nucleo numera zero. Un evento a coda
 * vuota con versione zero non ha niente da dire, e non fa chiedere niente.
 */
export const CODA_VUOTA: CodaTenuta = { versione: 0, coda: [] };

/** Quel che si fa di un evento: la coda da tenere e da mostrare, e se manca. */
export interface EsitoDellaCoda {
  tenuta: CodaTenuta;
  /** La coda da mettere nello stato che la finestra disegna. */
  coda: number[];
  /** Il nucleo ha una coda più nuova di quella in mano, e l'evento non l'ha portata. */
  manca: boolean;
}

/**
 * Il posto della coda in un evento di stato.
 *
 * Mai una coda più vecchia di quella in mano: un evento in ritardo — e uno stato
 * chiesto prima di un evento — non fa tornare indietro la coda. Con la stessa
 * versione e la coda nell'evento si tiene quella in mano, che è già uguale.
 */
export function laCodaDelloStato(tenuta: CodaTenuta, carico: StatoRiproduzioneEvento): EsitoDellaCoda {
  const resta = { tenuta, coda: tenuta.coda, manca: false };
  if (carico.versioneCoda < tenuta.versione) return resta;
  if (carico.versioneCoda === tenuta.versione) return resta;
  if (carico.coda !== undefined) {
    const nuova = { versione: carico.versioneCoda, coda: carico.coda };
    return { tenuta: nuova, coda: nuova.coda, manca: false };
  }
  return { ...resta, manca: true };
}

/**
 * Quel che torna da `codaDelLettore`: la nuova coda da tenere, o `null` se quella
 * in mano è già questa o più nuova (la risposta è arrivata dopo un evento che
 * l'aveva già portata).
 */
export function laCodaChiesta(tenuta: CodaTenuta, risposta: CodaDelLettore): CodaTenuta | null {
  if (risposta.versione <= tenuta.versione) return null;
  return { versione: risposta.versione, coda: risposta.coda };
}

/** Lo stato che la finestra disegna: l'evento, con la sua coda. */
export function statoConLaCoda(carico: StatoRiproduzioneEvento, coda: number[]): StatoRiproduzione {
  return { ...carico, coda };
}
