/**
 * Quanto c'è in libreria da una certa origine, per la lunghezza degli elenchi.
 *
 * Una lista rada (`rada.ts`) vuole il totale **prima** delle righe: la barra di
 * scorrimento è lunga quanto la vista dal primo istante. Per «Tutti» il totale
 * sta già in `avvio.numeri`; per «Questo dispositivo» e per un dispositivo
 * collegato lo dice `conteggi(origine)`, e si rilegge ogni volta che `avvio`
 * cambia — una scansione, un catalogo arrivato (`lontano:specchio`) — perché i
 * due si rileggono insieme.
 */
import { useEffect, useState } from "react";

import { ipc, type Numeri, type OrigineLibreria } from "./ipc";
import { chiaveDiOrigine, perIlNucleo } from "./origine";

/**
 * I numeri della libreria vista da `origine`, o `undefined` finché non
 * arrivano (e per «Tutti», finché non arriva `avvio`).
 *
 * `versione` è quel che dice che la libreria è stata riletta: l'`avvio`.
 */
export function useNumeriDelFiltro(
  origine: OrigineLibreria,
  numeriDiTutti: Numeri | undefined,
  versione: unknown,
  onErrore: (errore: unknown) => void,
): Numeri | undefined {
  const chiave = chiaveDiOrigine(origine);
  const [letti, setLetti] = useState<{ chiave: string; numeri: Numeri } | null>(null);
  useEffect(() => {
    if (origine.tipo === "tutti") return;
    let vivo = true;
    ipc
      .conteggi(perIlNucleo(origine))
      .then((numeri) => {
        if (vivo) setLetti({ chiave, numeri });
      })
      .catch((errore: unknown) => {
        if (vivo) onErrore(errore);
      });
    return () => {
      vivo = false;
    };
    // `origine` è detta da `chiave`; `onErrore` non cambia mai.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chiave, versione]);
  if (origine.tipo === "tutti") return numeriDiTutti;
  return letti?.chiave === chiave ? letti.numeri : undefined;
}
