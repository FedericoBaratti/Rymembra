/**
 * Lo Studio per i suoi pannelli: il negozio, il registro, e il modo di
 * scrivere nel documento.
 *
 * Ogni pannello legge da qui invece di ricevere trenta prop: è la ragione per
 * cui il componente dello Studio 2.x era cresciuto fino a quasi duemila righe
 * — ogni valore passava da lui per arrivare a chi lo usava.
 *
 * # Due contesti, e perché
 *
 * Lo stato cambia a ogni battuta, a ogni validazione che torna, a ogni passo
 * di un cursore trascinato. Le azioni — scrivere, mandare un'azione al
 * negozio, il registro — non cambiano mai. Finché stavano in un oggetto solo,
 * ricreato a ogni disegno dello Studio, ogni pannello si ridisegnava a ogni
 * battuta anche se dal contesto leggeva soltanto il registro: un campo
 * dell'ispettore, una riga della timeline, il «vai» di un problema. Adesso le
 * azioni stanno in un contesto che non cambia, e chi legge solo quelle
 * (`useAzioniStudio`) non si ridisegna più per lo stato degli altri.
 * `useStudio` resta, e dà le due metà insieme.
 *
 * # Le scritture hanno un nome
 *
 * `scrivi` e `togli` vogliono un'etichetta — «Raggio di section-card: 14px» —
 * che finisce sul tasto Annulla. Il gruppo dell'accorpamento è il percorso:
 * trascinare il raggio è un passo solo, passare dal raggio al colore ne apre
 * un altro.
 */
import { createContext, useContext, useMemo, type Dispatch } from "react";

import type { OperazioneIa, ParteRegistro, Registro, VoceFile } from "../ipc";
import type { Azione, StatoStudio } from "./negozio";

/** Quel che cambia mentre si lavora. */
export interface StatoDelloStudio {
  s: StatoStudio;
  /** Il documento letto, o `null` se il testo adesso non è JSON. */
  documento: Record<string, unknown> | null;
  /** I file del pacchetto della bozza. */
  file: readonly VoceFile[];
}

/** Quel che non cambia: il registro e il modo di scrivere. */
export interface AzioniDelloStudio {
  manda: Dispatch<Azione>;
  registro: Registro;
  /** Le parti del registro, per nome. */
  parti: ReadonlyMap<string, ParteRegistro>;
  /** Scrive un valore in un percorso; `null` o `undefined` lo toglie. */
  scrivi: (percorso: readonly string[], valore: unknown, etichetta: string) => void;
  /**
   * Più scritture in un passo solo: un preset che tocca dieci token, un colore
   * rinominato insieme a chi lo richiama. Annullare le disfa tutte insieme.
   */
  scriviMolti: (operazioni: readonly OperazioneIa[], etichetta: string, gruppo?: string) => void;
  /** Sostituisce il testo intero: l'editor di codice, la chat, un'istantanea. */
  scriviTesto: (testo: string, etichetta: string, gruppo?: string) => void;
  /** Rilegge i file del pacchetto della bozza. */
  rileggiFile: () => void;
  onErrore: (e: unknown) => void;
}

/** Le due metà insieme: quel che un pannello qualunque chiede. */
export type Studio = StatoDelloStudio & AzioniDelloStudio;

export const ContestoStatoStudio = createContext<StatoDelloStudio | null>(null);
export const ContestoAzioniStudio = createContext<AzioniDelloStudio | null>(null);

/** Le azioni dello Studio, senza lo stato: non ridisegna a ogni battuta. */
export function useAzioniStudio(): AzioniDelloStudio {
  const azioni = useContext(ContestoAzioniStudio);
  if (azioni === null) throw new Error("useAzioniStudio fuori dallo Studio");
  return azioni;
}

/** Lo stato e le azioni dello Studio. */
export function useStudio(): Studio {
  const stato = useContext(ContestoStatoStudio);
  const azioni = useContext(ContestoAzioniStudio);
  const unito = useMemo(
    () => (stato === null || azioni === null ? null : { ...stato, ...azioni }),
    [stato, azioni],
  );
  if (unito === null) throw new Error("useStudio fuori dallo Studio");
  return unito;
}
