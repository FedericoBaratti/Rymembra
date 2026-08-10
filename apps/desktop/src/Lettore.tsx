/**
 * La barra di riproduzione.
 *
 * Non tiene nessuno stato che il nucleo già conosca: quel che si vede qui è
 * `StatoRiproduzione` disegnato, e ogni tasto è una chiamata che tornerà come
 * evento. L'unica eccezione era il trascinamento del cursore, che ora sta in
 * `Scrubber` — dove appartiene, perché è stato del controllo e non della barra.
 */
import { useState, type CSSProperties } from "react";

import { ipc, type Brano, type StatoRiproduzione } from "./ipc";
import { PannelloEq } from "./parti/Equalizzatore";
import { Icona } from "./parti/Icone";
import { Ora } from "./parti/Ora";
import { Scrubber } from "./parti/Scrubber";
import { Trasporto } from "./parti/Trasporto";

export function Lettore({
  stato,
  codaAperta,
  onCoda,
  onPreferito,
  onErrore,
  onColonna,
}: {
  stato: StatoRiproduzione;
  codaAperta: boolean;
  onCoda: () => void;
  onPreferito: (brano: Brano) => void;
  onErrore: (e: unknown) => void;
  /** Riapre la terza colonna: chiuderla dev'essere reversibile da qui. */
  onColonna: () => void;
}) {
  const [eqAperto, setEqAperto] = useState(false);
  const brano = stato.brano;
  // Niente coda, niente barra: uno spazio vuoto in fondo allo schermo non
  // comunica niente e ruba l'altezza di due righe di elenco.
  if (!brano) return null;

  const comanda = (azione: Promise<void>) => {
    azione.catch(onErrore);
  };

  return (
    <footer className="lettore player-shell">
      <Ora brano={brano} onPreferito={onPreferito} />

      <div className="comandi">
        <Trasporto stato={stato} taglia="barra" onErrore={onErrore} />
        <Scrubber stato={stato} onErrore={onErrore} />
      </div>

      <div className="suono">
        {/* La porta di ritorno alla colonna. Senza, chiuderla sarebbe una
            scelta che si disfa solo allargando la finestra — cioè non si
            disfa. */}
        <button
          type="button"
          className="tasto icon-btn"
          aria-label="Riapri la colonna In riproduzione"
          title="In riproduzione"
          onClick={onColonna}
        >
          <Icona nome="i-expand" dim={17} />
        </button>
        <button
          type="button"
          className="tasto icon-btn"
          aria-pressed={codaAperta}
          aria-label={`Coda di riproduzione, ${stato.coda.length} brani`}
          title="Coda"
          onClick={onCoda}
        >
          <Icona nome="i-queue" dim={17} />
        </button>
        {/* L'equalizzatore accanto al volume: sono la stessa famiglia di
            comandi — quanto forte, e com'è fatto — e chi cerca il secondo lo
            cerca dove ha trovato il primo. */}
        <div className="con-pannello">
          <button
            type="button"
            className="tasto icon-btn"
            aria-pressed={eqAperto}
            aria-expanded={eqAperto}
            aria-label="Equalizzatore"
            title="Equalizzatore"
            data-acceso={stato.eqAttivo || undefined}
            onClick={() => setEqAperto((prima) => !prima)}
          >
            <Icona nome="i-eq" dim={17} />
          </button>
          {eqAperto && (
            <PannelloEq
              attivo={stato.eqAttivo}
              guadagni={stato.eqGuadagni}
              onChiudi={() => setEqAperto(false)}
              onErrore={onErrore}
            />
          )}
        </div>
        <button
          type="button"
          className="tasto icon-btn"
          aria-pressed={stato.muto}
          aria-label={stato.muto ? "Riattiva l'audio" : "Silenzia"}
          title={stato.muto ? "Riattiva" : "Silenzia"}
          onClick={() => comanda(ipc.volume(stato.volume, !stato.muto))}
        >
          {/* Erano due emoji a colori: `🔇` e `🔊` si disegnavano con la
              tavolozza del carattere di sistema, cioè con gli unici due colori
              dell'interfaccia che nessuna skin poteva toccare. */}
          <Icona nome={stato.muto || stato.volume === 0 ? "i-vol-x" : "i-vol"} dim={17} />
        </button>
        <input
          type="range"
          className="scorrimento volume range-accent"
          min={0}
          max={1}
          step={0.01}
          value={stato.volume}
          style={{ "--avanzamento": `${stato.volume * 100}%` } as CSSProperties}
          aria-label="Volume"
          /* Muovere il volume toglie il silenziamento: chi trascina la manopola
             sta chiedendo di sentire, e lasciarla muta gli farebbe credere che
             il comando sia rotto. */
          onChange={(e) => comanda(ipc.volume(Number(e.target.value), false))}
        />
      </div>
    </footer>
  );
}
