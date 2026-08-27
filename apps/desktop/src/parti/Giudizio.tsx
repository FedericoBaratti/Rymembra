/**
 * Cuore, stelle e volume: cosa penso di quel che sto sentendo, e quanto forte.
 *
 * Le due copie da cui viene differivano per un numero solo — la misura del
 * cuore — il che è precisamente il motivo per cui erano due copie: nessuno
 * estrae trentacinque righe per un `16` che diventa `18`, e così le altre
 * trentaquattro si correggono in un posto e restano sbagliate nell'altro.
 */
import type { CSSProperties } from "react";

import { Stelle } from "../Stelle";
import { ipc, type Brano, type StatoRiproduzione } from "../ipc";
import { Icona } from "./Icone";
import { t } from "../lingue";

/** Dove sta il giudizio, e quindi quanto è grande il cuore. */
export type TagliaGiudizio = "colonna" | "grande";

const CUORE = { colonna: 16, grande: 18 } as const;

export function Giudizio({
  stato,
  brano,
  taglia,
  conStelle = true,
  conVolume = true,
  onPreferito,
  onVoto,
  onErrore,
}: {
  stato: StatoRiproduzione;
  brano: Brano;
  taglia: TagliaGiudizio;
  /** Le cinque stelle. */
  conStelle?: boolean;
  /** Il cursore del volume. */
  conVolume?: boolean;
  onPreferito: (brano: Brano) => void;
  onVoto: (brano: Brano, stelle: number) => void;
  onErrore: (e: unknown) => void;
}) {
  const comanda = (azione: Promise<void>) => {
    azione.catch(onErrore);
  };

  return (
    <div className="giudizio">
      <button
        type="button"
        className="cuore icon-btn"
        aria-pressed={brano.liked}
        aria-label={brano.liked ? t("track.unlike") : t("track.like")}
        onClick={() => onPreferito(brano)}
      >
        <Icona
          nome={brano.liked ? "i-heart-f" : "i-heart"}
          dim={CUORE[taglia]}
        />
      </button>
      {conStelle && (
        <Stelle
          valore={brano.rating}
          onVoto={(stelle) => onVoto(brano, stelle)}
        />
      )}
      <div className="volume" hidden={!conVolume}>
        <button
          type="button"
          className="tasto icon-btn"
          aria-pressed={stato.muto}
          aria-label={stato.muto ? t("player.unmute") : t("player.mute")}
          onClick={() => comanda(ipc.volume(stato.volume, !stato.muto))}
        >
          <Icona
            nome={stato.muto || stato.volume === 0 ? "i-vol-x" : "i-vol"}
            dim={16}
          />
        </button>
        <input
          type="range"
          className="scorrimento range-accent"
          min={0}
          max={1}
          step={0.01}
          value={stato.volume}
          style={{ "--avanzamento": `${stato.volume * 100}%` } as CSSProperties}
          aria-label={t("player.volume")}
          /* Muovere il volume toglie il silenziamento: chi trascina la manopola
             sta chiedendo di sentire, e lasciarla muta gli farebbe credere che
             il comando sia rotto. */
          onChange={(e) => comanda(ipc.volume(Number(e.target.value), false))}
        />
      </div>
    </div>
  );
}
