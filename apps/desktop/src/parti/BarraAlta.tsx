/**
 * La barra in cima a una pagina del telefono.
 *
 * Sul computer l'intestazione di una pagina è grande — occhiello, titolo,
 * sottotitolo, i comandi e la ricerca a destra — perché l'aria si spende lì e
 * non fra le righe (`Intestazione.tsx`). Sul telefono quella stessa
 * intestazione andava a capo tre volte e si prendeva un quarto dello schermo
 * prima del primo brano. Qui è una barra: il titolo, a sinistra la freccia
 * quando si è dentro qualcosa, a destra due o tre tasti a icona — cerca,
 * ordina, «di più» — e sotto, quando serve, una seconda fila (le pillole della
 * Libreria, il segmento di Cerca).
 *
 * Non porta le classi delle parti del registro (`page-header`, `page-title`…):
 * una skin decide i colori e i caratteri del telefono, non le sue forme, e una
 * regola scritta per la testata larga del computer qui disegnerebbe la cosa
 * sbagliata. Vedi «Specificità delle skin» nel piano del telefono, e
 * `stile/telefono-pagine.css`.
 */
import type { ReactNode } from "react";

import { t } from "../lingue";
import { Icona, type NomeIcona } from "./Icone";

export function BarraAlta({
  titolo,
  occhiello,
  sottotitolo,
  indietro,
  azioni,
  sotto,
  grande = false,
  ritirato = false,
}: {
  titolo: string;
  /** La parola piccola sopra il titolo, solo nelle barre grandi. */
  occhiello?: string | undefined;
  /** Una riga sotto il titolo piccolo: di chi è il disco, quanti brani. */
  sottotitolo?: string | undefined;
  /** C'è la freccia, e fa questo. */
  indietro?: (() => void) | undefined;
  /** I tasti a destra: `TastoBarra`, uno per azione. */
  azioni?: ReactNode;
  /** La seconda fila, sotto il titolo. */
  sotto?: ReactNode;
  /**
   * Il titolo grande delle prime pagine di una scheda — Home, Libreria, Cerca,
   * Altro — dove dice dove si è. Nelle pagine dentro (un album, le
   * Impostazioni) è piccolo, accanto alla freccia: lì il nome grande lo porta la
   * pagina stessa.
   */
  grande?: boolean | undefined;
  /**
   * La pagina ha la sua testata grande (`TestataDettaglio`), col nome: qui il
   * titolo compare solo quando quello esce di sotto la barra, scorrendo. Lo
   * fa il foglio: vedi «il titolo che si ritira» in `telefono-pagine.css`.
   */
  ritirato?: boolean | undefined;
}) {
  return (
    <header
      className="barra-alta"
      data-grande={grande || undefined}
      data-ritirato={ritirato || undefined}
    >
      <div className="riga-barra">
        {indietro !== undefined && (
          <TastoBarra icona="i-chev-l" etichetta={t("common.back")} onClick={indietro} />
        )}
        <div className="titoli-barra">
          {grande && occhiello !== undefined && (
            <span className="occhiello-barra">{occhiello}</span>
          )}
          {/* `dir="auto"`: spesso è il nome di un album o di un artista, e il
              verso lo decide lui. */}
          <h1 className="titolo-barra" dir="auto">
            {titolo}
          </h1>
          {!grande && sottotitolo !== undefined && (
            <span className="sotto-barra" dir="auto">
              {sottotitolo}
            </span>
          )}
        </div>
        {azioni !== undefined && <div className="azioni-barra">{azioni}</div>}
      </div>
      {sotto}
    </header>
  );
}

/** Un tasto a icona della barra: quarantotto pixel, l'etichetta per chi non vede. */
export function TastoBarra({
  icona,
  etichetta,
  onClick,
  premuto,
}: {
  icona: NomeIcona;
  etichetta: string;
  onClick: (e: React.MouseEvent<HTMLButtonElement>) => void;
  /** Per i tasti che accendono qualcosa. */
  premuto?: boolean | undefined;
}) {
  return (
    <button
      type="button"
      className="tasto-barra"
      aria-label={etichetta}
      aria-pressed={premuto}
      onClick={onClick}
    >
      <Icona nome={icona} dim={22} />
    </button>
  );
}

