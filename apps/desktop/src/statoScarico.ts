/**
 * Come lo stato della coda si legge e si aggiorna in `parti/Importazioni.tsx`.
 *
 * Funzioni pure, fuori dal componente, perché sono le tre regole che lì si
 * sbagliavano in silenzio (R13c-11) e qui si provano senza una finestra:
 *
 * - la lettura d'avvio di `scaricoStato` si ritenta: fallita una volta, lo
 *   stato restava `null` finché un'importazione non lo rileggeva, e con lui
 *   l'interruttore delle registrazioni diverse e il tasto «Riprendi»;
 * - un avanzamento che arriva con lo stato ancora `null` lo fa chiedere: senza,
 *   gli eventi della coda si buttavano tutti;
 * - la fine della coda la dice ferma subito, prima che la rilettura risponda:
 *   se la rilettura cadeva, la coda restava «in corso» per sempre.
 */
import type { StatoScarico } from "./ipc";

/** Quante volte si ritenta la lettura d'avvio dopo la prima. */
export const RITENTATIVI_AVVIO = 2;

/** Quanto si aspetta fra un tentativo e l'altro, in millisecondi. */
export const ATTESA_RITENTATIVO_MS = 1500;

/**
 * Legge con `leggi`, ritentando fino a `ritentativi` volte dopo `attendi`.
 *
 * `fermato` dice se chi ha chiesto non c'è più (il componente smontato): allora
 * non si ritenta, e l'ultimo errore torna com'è.
 */
export async function conRitentativi<T>(
  leggi: () => Promise<T>,
  ritentativi: number,
  attendi: () => Promise<void>,
  fermato: () => boolean = () => false,
): Promise<T> {
  for (let fatti = 0; ; fatti += 1) {
    try {
      return await leggi();
    } catch (errore) {
      if (fatti >= ritentativi || fermato()) throw errore;
      await attendi();
      if (fermato()) throw errore;
    }
  }
}

/** Il carico di `scarico:avanzamento`. */
export interface Avanzamento {
  fatti: number;
  totale: number;
  sorgenti: StatoScarico["sorgenti"];
}

/**
 * Lo stato dopo un avanzamento della coda.
 *
 * Con lo stato ancora `null` non c'è niente da aggiornare: resta `null`, e chi
 * riceve l'evento lo chiede (vedi [`daRileggere`]).
 */
export function conAvanzamento(
  prima: StatoScarico | null,
  carico: Avanzamento,
): StatoScarico | null {
  return prima
    ? {
        ...prima,
        attiva: true,
        fatti: carico.fatti,
        rimasti: carico.totale - carico.fatti,
        sorgenti: carico.sorgenti,
      }
    : prima;
}

/** Un evento della coda è arrivato e lo stato non c'è: va chiesto. */
export function daRileggere(stato: StatoScarico | null): boolean {
  return stato === null;
}

/**
 * Lo stato alla fine della coda, prima che la rilettura risponda: ferma.
 *
 * `scarico:finito` arriva dopo che il posto della coda è stato lasciato (vedi
 * `procura::sul_filo`), quindi dirla ferma è la verità anche se la rilettura
 * che segue non arriva mai.
 */
export function allaFine(prima: StatoScarico | null): StatoScarico | null {
  return prima ? { ...prima, attiva: false } : prima;
}
