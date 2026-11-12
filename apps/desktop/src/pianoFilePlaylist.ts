/**
 * Quando il piano di un file di playlist descrive quel che «Importa» farebbe.
 *
 * Il piano dipende dal **nome**: una playlist con quel nome può esserci già,
 * fatta qui (e allora il file le entra accanto, D-11) o importata prima da un
 * file (e allora si riscrive). Fino alla 3.2.0 il piano si chiedeva una volta,
 * sul nome proposto, e cambiare il campo non lo rifaceva: si scriveva
 * «Preferite», che c'era, e «Importa» la riscriveva senza un avviso; o si
 * scriveva «Rock 2» come l'avviso suggeriva, e l'avviso restava, ora col nome
 * nuovo e falso (R13c-2).
 *
 * Adesso il piano si rifà a ogni nome (con un attimo d'attesa fra un tasto e
 * l'altro), porta il nome per cui è stato fatto, e «Importa» resta spento finché
 * quel nome non è quello nel campo.
 */

/** Quanto aspettare dopo l'ultimo tasto prima di rifare il piano. */
export const ATTESA_PIANO_MS = 300;

/** «Importa» si può premere? */
export function importabile({
  inCorso,
  nome,
  nomeDelPiano,
  ritrovati,
}: {
  /** Si sta leggendo, pianificando o importando. */
  inCorso: boolean;
  /** Quel che c'è nel campo. */
  nome: string;
  /** Il nome per cui è stato fatto il piano a schermo; `null` senza piano. */
  nomeDelPiano: string | null;
  /** Quante voci del file si sono ritrovate: a zero la playlist sarebbe vuota. */
  ritrovati: number;
}): boolean {
  return (
    !inCorso &&
    nomeDelPiano !== null &&
    nome.trim().length > 0 &&
    nomeDelPiano === nome &&
    ritrovati > 0
  );
}
