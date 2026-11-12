/**
 * Le regole delle righe della coda che si possono dire senza React: in che
 * stato è una riga, quali righe sparite togliere, e quale riga sposta un colpo
 * di `Alt+↑↓` mentre quello di prima è ancora in viaggio.
 *
 * Stanno qui e non in `Coda.tsx` perché `node --test` non carica un `.tsx`, e
 * sono proprio le parti che si sbagliavano in silenzio (R13b-4, R13b-5).
 */
import type { Brano } from "./ipc";
import { idDiLibreria } from "./insieme";

/**
 * Le righe della coda, per identificativo: il brano, oppure `null` per un
 * brano **chiesto e non trovato** — sparito dalla libreria mentre stava in
 * coda. Un identificativo che la mappa non ha è una riga non ancora arrivata.
 */
export type MappaRigheCoda = ReadonlyMap<number, Brano | null>;

/**
 * Lo stato di una riga.
 *
 * «Non ancora arrivata» e «sparita» erano la stessa cosa — `righe.get(id) ===
 * undefined` — e ogni riga in attesa diceva «brano non più in libreria · id
 * 40213»: all'avvio con una coda di diciottomila brani, una colonna intera di
 * brani dati per persi mentre la risposta era in viaggio (R13b-5).
 */
export function statoRiga(
  id: number,
  righe: MappaRigheCoda,
): "presente" | "sparita" | "attesa" {
  const riga = righe.get(id);
  if (riga === undefined) return "attesa";
  return riga === null ? "sparita" : "presente";
}

/**
 * Le righe sparite da togliere dalla coda, dalla più in fondo.
 *
 * Solo i brani della libreria (le voci di «Ascolta insieme» non si chiedono
 * alla libreria, e non spariscono da lei), e solo quelli che una risposta
 * **riuscita** non ha restituito: una lettura fallita non dice niente di
 * nessuno. Dalla più in fondo perché togliere una riga sposta di un posto
 * quelle dopo di lei, non quelle prima: in quest'ordine le posizioni chieste
 * restano vere anche mentre il nucleo le toglie una per una. Ognuna va col suo
 * brano (`codaTogli`), quindi se la coda è cambiata nel frattempo il nucleo non
 * toglie un'altra riga.
 */
export function sparizioniDaTogliere(
  coda: readonly number[],
  righe: MappaRigheCoda,
): { indice: number; id: number }[] {
  const via: { indice: number; id: number }[] = [];
  for (let indice = coda.length - 1; indice >= 0; indice -= 1) {
    const id = coda[indice];
    if (id !== undefined && idDiLibreria(id) && righe.get(id) === null) {
      via.push({ indice, id });
    }
  }
  return via;
}

/** Un riordino partito dalla tastiera e non ancora confermato dallo stato. */
export interface RiordinoInVolo {
  /** Dove il brano sta andando. */
  dove: number;
  /** Il brano che si sta portando. */
  id: number;
}

/**
 * Il riordino che fa un colpo di `Alt+↑↓` sulla riga `indice`.
 *
 * # Mentre il colpo di prima è in viaggio
 *
 * Il fuoco segue la riga spostata subito, prima che lo stato nuovo arrivi; ma
 * la coda in mano è ancora quella vecchia. Col tasto tenuto premuto il secondo
 * colpo partiva dalla riga col fuoco e mandava il brano che la coda **vecchia**
 * ci aveva — un altro — e dal terzo colpo in poi si spostava una canzone
 * diversa da quella presa (R13b-4). Se la riga col fuoco è quella dove sta
 * andando il brano in volo, il colpo continua a portare **lui**: il nucleo, che
 * i riordini li applica in fila, lo trova esattamente lì.
 *
 * `null` se il colpo non porta da nessuna parte: la prima riga in su, l'ultima
 * in giù.
 */
export function prossimoRiordino(
  coda: readonly number[],
  inVolo: RiordinoInVolo | null,
  indice: number,
  verso: "su" | "giu",
): { da: number; a: number; id: number } | null {
  const continua = inVolo !== null && inVolo.dove === indice;
  const id = continua ? inVolo.id : coda[indice];
  const a = indice + (verso === "giu" ? 1 : -1);
  if (id === undefined || a < 0 || a >= coda.length) return null;
  return { da: indice, a, id };
}

/**
 * Per quanto un riordino in volo aspetta lo stato che lo conferma.
 *
 * Oltre, si lascia andare: il riordino non è arrivato (il nucleo l'ha
 * scartato, la coda è cambiata sotto) e un'attesa rimasta appesa portava il
 * fuoco nella coda più tardi, da dovunque fosse — anche dal campo di ricerca —
 * il giorno in cui un altro gesto metteva per caso quel brano in quel posto.
 */
export const ATTESA_RIORDINO_MS = 2_000;
