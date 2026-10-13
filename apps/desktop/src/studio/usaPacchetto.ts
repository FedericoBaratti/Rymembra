/**
 * I file del pacchetto della bozza: il manifest, la miniatura, le risorse.
 *
 * Una lettura sola per tutto lo Studio, nel contesto: la usano le variabili
 * (per elencarli), l'ispettore (per scegliere l'immagine di un livello o il
 * file di un carattere) e la tela non ne ha bisogno — il compilatore scrive
 * già gli indirizzi. Si rilegge dopo ogni aggiunta o rimozione.
 */
import { useCallback, useEffect, useState } from "react";

import { ipc, type VoceFile } from "../ipc";

export function usePacchetto(id: string, onErrore: (e: unknown) => void) {
  const [voci, setVoci] = useState<VoceFile[]>([]);
  const [giro, setGiro] = useState(0);
  useEffect(() => {
    let vivo = true;
    ipc
      .studioPacchetto(id)
      .then((v) => {
        if (vivo) setVoci(v);
      })
      .catch(onErrore);
    return () => {
      vivo = false;
    };
  }, [id, giro, onErrore]);
  const rileggi = useCallback(() => setGiro((g) => g + 1), []);
  return { voci, rileggi };
}

/** Il nome di una risorsa com'è nel documento: senza `assets/`. */
export function nomeRisorsa(voce: VoceFile): string {
  return voce.nome.replace(/^assets\//, "");
}

/** Le immagini fra i file del pacchetto. */
export function immaginiDi(voci: readonly VoceFile[]): string[] {
  return voci
    .filter((v) => v.genere === "risorsa" && !v.nome.endsWith(".woff2"))
    .map(nomeRisorsa);
}

/** I caratteri fra i file del pacchetto. */
export function caratteriDi(voci: readonly VoceFile[]): string[] {
  return voci
    .filter((v) => v.genere === "risorsa" && v.nome.endsWith(".woff2"))
    .map(nomeRisorsa);
}
