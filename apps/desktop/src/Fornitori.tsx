/**
 * I contesti che la finestra dà a tutto quel che disegna: le presenze della
 * skin, la copertina in volo, quel che suona, l'archivio dello stato del
 * motore, la libreria, le importazioni, il backup e la sincronia.
 *
 * Un componente e non sette fornitori annidati in `App`, perché annidati
 * spostavano di sette livelli le quattrocento righe della finestra. Lo Studio
 * fornisce i primi tre, coi suoi valori; l'archivio no, perché il suo scafale
 * lo stato lo porta nel contesto (vedi `ContestoWidget` in `Impaginazione.tsx`).
 *
 * # Chi si ridisegna
 *
 * I figli arrivano già fatti da `App`. Quando qui cambia qualcosa che è suo —
 * il brano o la pausa per «quel che suona», una passata di importazioni o di
 * arricchimento — si ridisegna il fornitore che lo tiene, e i figli restano
 * dove sono: si risvegliano soltanto le schede, le righe e le pagine che
 * leggono quel contesto. È la ragione per cui «quel che suona» si ricava qui
 * dall'archivio, e le importazioni e il backup stanno nei loro fornitori
 * invece che negli stati di `App`.
 *
 * # L'ordine degli effetti
 *
 * `FornitoreImportazioni` e `FornitoreNuvola` registrano le loro letture e i
 * loro ascolti quando si montano, cioè **prima** degli effetti di `App`, che
 * li contiene: stavano a metà del corpo di `App`. Nessuno degli effetti di
 * `App` fa scattare un evento di quei fili, e le loro domande al nucleo sono
 * indipendenti dalle sue: si vede lo stesso.
 */
import { useMemo, type ReactNode } from "react";

import { useArchivio } from "./archivio";
import { LibreriaDellaFinestra, type Libreria } from "./libreria";
import { FornitoreNuvola } from "./nuvola";
import { FornitoreImportazioni } from "./parti/Importazioni";
import { Presenze, Volo, type MappaPresenze } from "./presenza";
import { LettoreDellaFinestra, type ArchivioLettore } from "./riproduzione";
import { Suona, type QuelCheSuona } from "./segnali";

/** Vedi il commento in testa. */
export function Fornitori({
  presenze,
  volo,
  lettore,
  libreria,
  segnalaErrore,
  children,
}: {
  presenze: MappaPresenze;
  volo: string | null;
  lettore: ArchivioLettore;
  libreria: Libreria;
  segnalaErrore: (e: unknown) => void;
  children: ReactNode;
}) {
  const inAscolto = useArchivio(lettore, (s) => s.brano?.id ?? null);
  /** L'album di quel che suona: la sua scheda lo dice alla skin. */
  const albumInAscolto = useArchivio(lettore, (s) => s.brano?.albumKey ?? null);
  const inPausa = useArchivio(lettore, (s) => s.inPausa);
  const suona: QuelCheSuona = useMemo(
    () => ({ brano: inAscolto, album: albumInAscolto, inPausa }),
    [inAscolto, albumInAscolto, inPausa],
  );
  return (
    <Presenze value={presenze}>
      <Volo value={volo}>
        <Suona value={suona}>
          <LettoreDellaFinestra value={lettore}>
            <LibreriaDellaFinestra value={libreria}>
              <FornitoreImportazioni>
                <FornitoreNuvola segnalaErrore={segnalaErrore}>{children}</FornitoreNuvola>
              </FornitoreImportazioni>
            </LibreriaDellaFinestra>
          </LettoreDellaFinestra>
        </Suona>
      </Volo>
    </Presenze>
  );
}
