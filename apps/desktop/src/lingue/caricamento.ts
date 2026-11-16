/**
 * Caricare i dizionari a richiesta.
 *
 * # Perché
 *
 * I due dizionari erano un terzo del JavaScript principale (≈454 KB su
 * 1 288 KB): caricati e valutati a ogni avvio, anche se se ne usa uno per volta
 * — l'altro serve solo come ripiego per le chiavi che mancano, e oggi non ne
 * manca nessuna. Ogni lingua aggiunta, per la regola di `index.ts` («un file
 * solo»), li avrebbe allungati di un altro pezzo (R21-4).
 *
 * Qui sta la parte che si prova da sola, senza `import.meta.glob`, che solo Vite
 * risolve: caricare un file una volta sola, scordare un guasto, e far vincere
 * l'ultima richiesta quando due cambi di lingua si accavallano.
 *
 * Niente React: `node --test` lo carica da solo (`test/lingueCaricamento.test.ts`).
 */
import { vinceLUltima } from "../ultima";

/** Un dizionario, come sta sul disco: chiave piatta → testo. */
export type Dizionario = Record<string, string>;

/** Chi sa caricare i dizionari, e quali ha già in mano. */
export interface Caricatore {
  /** Il dizionario già caricato, o `undefined`. */
  caricato(codice: string): Dizionario | undefined;
  /**
   * Lo carica, una volta sola: due richieste in volo per la stessa lingua sono
   * una lettura, e una lingua già in mano non si rilegge. Un guasto arriva a chi
   * ha chiesto e **non** si ricorda: la richiesta dopo riprova.
   */
  carica(codice: string): Promise<Dizionario>;
}

/**
 * Un caricatore per questi file: per codice, la funzione che lo legge.
 *
 * L'elenco dei codici è quello dei file, e `codice` si cerca fra le sue chiavi
 * proprie: «constructor» non è una lingua.
 */
export function creaCaricatore(
  file: Readonly<Record<string, () => Promise<Dizionario>>>,
): Caricatore {
  const caricati = new Map<string, Dizionario>();
  const inVolo = new Map<string, Promise<Dizionario>>();
  return {
    caricato: (codice) => caricati.get(codice),
    carica: (codice) => {
      const gia = caricati.get(codice);
      if (gia !== undefined) return Promise.resolve(gia);
      const incorso = inVolo.get(codice);
      if (incorso !== undefined) return incorso;
      const leggi = Object.hasOwn(file, codice) ? file[codice] : undefined;
      if (leggi === undefined) {
        return Promise.reject(new Error(`nessun file per la lingua «${codice}»`));
      }
      const richiesta = leggi()
        .then((dizionario) => {
          caricati.set(codice, dizionario);
          return dizionario;
        })
        .finally(() => {
          inVolo.delete(codice);
        });
      inVolo.set(codice, richiesta);
      return richiesta;
    },
  };
}

/**
 * Il cambio di lingua: prima si carica, poi si applica — e se nel frattempo ne è
 * stata chiesta un'altra, vince quella (`vinceLUltima`).
 *
 * Applicare prima di caricare lascerebbe a schermo le chiavi nude per il tempo
 * della lettura, che è peggio della lingua di prima. Un guasto del caricamento
 * arriva a chi ha chiesto, e la lingua resta com'era.
 */
export function creaCambioLingua(
  caricatore: Caricatore,
  applica: (codice: string) => void,
): (codice: string) => Promise<void> {
  const turno = vinceLUltima();
  return async (codice) => {
    const mio = turno();
    await caricatore.carica(codice);
    if (mio()) applica(codice);
  };
}
