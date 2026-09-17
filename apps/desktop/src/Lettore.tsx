/**
 * La barra di riproduzione.
 *
 * Non tiene nessuno stato che il nucleo già conosca: quel che si vede qui è
 * `StatoRiproduzione` disegnato, e ogni tasto è una chiamata che tornerà come
 * evento. L'unica eccezione era il trascinamento del cursore, che ora sta in
 * `Scrubber` — dove appartiene, perché è stato del controllo e non della barra.
 */
import { useCallback, useEffect, useRef, useState, type CSSProperties } from "react";

import { numero } from "./formato";
import { ipc, type Brano, type StatoRiproduzione } from "./ipc";
import { PannelloEq } from "./parti/Equalizzatore";
import { Icona } from "./parti/Icone";
import { Ora } from "./parti/Ora";
import { Scrubber } from "./parti/Scrubber";
import { Trasporto } from "./parti/Trasporto";
import { t } from "./lingue";
import { Formato } from "./parti/Formato";
import { useVolumeSottoIlDito } from "./volume";

/**
 * Quanto si muove il volume a ogni scatto, di freccia o di rotella.
 *
 * Un ventesimo: venti scatti dal silenzio al massimo, che è il numero in cui si
 * arriva dove si vuole senza tenere premuto e senza superarlo. Il cursore ha
 * `step={0.01}` perché chi trascina punta a un valore; chi scatta cerca una
 * direzione, e un centesimo per scatto sarebbero cento pressioni.
 */
const PASSO_VOLUME = 0.05;

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
  const tastoMuto = useRef<HTMLButtonElement>(null);
  // Il cursore segue il dito, e il nucleo lo sente venti volte al secondo:
  // vedi `volume.ts`. Le frecce e la rotella sul tasto muto restano scatti
  // interi e vanno dritti al nucleo: sono un colpo alla volta.
  const { volume, sposta } = useVolumeSottoIlDito(stato.volume, onErrore);

  /** Sposta il volume di uno scatto, e togliendo il silenziamento. */
  const muovi = useCallback(
    (delta: number) => {
      // Arrotondato al centesimo: `0.05` sommato più volte in virgola mobile dà
      // `0.35000000000000003`, che il nucleo si annoterebbe così com'è.
      const livello = Math.round(Math.min(1, Math.max(0, stato.volume + delta)) * 100) / 100;
      ipc.volume(livello, false).catch(onErrore);
    },
    [stato.volume, onErrore],
  );

  /**
   * La via di ritorno al volume quando il cursore si ritira.
   *
   * Sotto i settecentosessanta pixel di barra il foglio nasconde il cursore del
   * volume e resta il solo tasto muto: fino a qui il livello non si poteva più
   * cambiare in nessun modo — «né scorciatoia da tastiera né rotella
   * sull'icona», dice la regola che lo ritira — cioè un comando spariva invece
   * di ritirarsi. Le frecce sul tasto lo rimettono, e ↑↓ sono libere nella scala
   * globale, dove ←→ valgono «avanti e indietro di cinque secondi».
   *
   * La rotella si iscrive a mano, e non con `onWheel`, perché React iscrive
   * `wheel` sulla radice in modo **passivo**: da lì `preventDefault()` non
   * funziona, e il volume si muoverebbe insieme all'elenco che scorre sotto.
   */
  useEffect(() => {
    const tasto = tastoMuto.current;
    if (tasto === null) return;
    const rotella = (e: WheelEvent) => {
      e.preventDefault();
      muovi(e.deltaY < 0 ? PASSO_VOLUME : -PASSO_VOLUME);
    };
    tasto.addEventListener("wheel", rotella, { passive: false });
    return () => tasto.removeEventListener("wheel", rotella);
  }, [muovi]);

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

      {/* L'ancora del giro guidato: il guscio e non i due pezzi, perché il
          passo racconta trasporto e cursore come una cosa sola — «si comanda
          da qui, e si va dove si vuole». */}
      <div className="comandi" data-giro="trasporto">
        <Trasporto stato={stato} taglia="barra" onErrore={onErrore} />
        <Scrubber stato={stato} onErrore={onErrore} />
        {/* I dati tecnici del file anche qui, sotto il cursore. La colonna e
            lo schermo intero li mostravano, la barra no: e la barra è quel che
            resta sotto i 1100 pixel, quando la colonna si chiude — cioè chi
            teneva la finestra stretta non li vedeva mai. */}
        <Formato formato={stato.formato} classe="formato lettore-formato" />
      </div>

      <div className="suono">
        {/* La porta di ritorno alla colonna. Senza, chiuderla sarebbe una
            scelta che si disfa solo allargando la finestra — cioè non si
            disfa. */}
        <button
          type="button"
          className="tasto icon-btn"
          aria-label={t("player.reopenColumn")}
          title={t("player.nowPlaying")}
          onClick={onColonna}
        >
          <Icona nome="i-expand" dim={17} />
        </button>
        <button
          type="button"
          className="tasto icon-btn"
          aria-pressed={codaAperta}
          aria-label={t("player.queue.aria", { n: stato.coda.length })}
          title={t("player.queue")}
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
            aria-label={t("player.eq")}
            title={t("player.eq")}
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
          ref={tastoMuto}
          type="button"
          className="tasto icon-btn"
          aria-pressed={stato.muto}
          aria-label={stato.muto ? t("player.unmute") : t("player.mute")}
          title={stato.muto ? t("player.unmute.short") : t("player.mute")}
          /* Le due frecce valgono il volume: è l'unica via che resta quando il
             cursore accanto si ritira, e dichiararla è il solo modo perché chi
             ascolta lo schermo sappia che c'è. */
          aria-keyshortcuts="ArrowUp ArrowDown"
          onKeyDown={(e) => {
            const passo =
              e.key === "ArrowUp"
                ? PASSO_VOLUME
                : e.key === "ArrowDown"
                  ? -PASSO_VOLUME
                  : 0;
            if (passo === 0) return;
            e.preventDefault();
            muovi(passo);
          }}
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
          value={volume}
          style={{ "--avanzamento": `${volume * 100}%` } as CSSProperties}
          aria-label={t("player.volume")}
          /* Senza questo uno screen reader legge «0,72», che è il numero con cui
             il volume viaggia e non quello in cui si pensa. La percentuale passa
             da `numero()` come ogni altro numero dell'interfaccia, così la
             virgola o il punto li decide la lingua e non questo file. */
          aria-valuetext={t("player.volume.value", {
            percento: numero(Math.round(volume * 100)),
          })}
          /* Muovere il volume toglie il silenziamento: chi trascina la manopola
             sta chiedendo di sentire, e lasciarla muta gli farebbe credere che
             il comando sia rotto. */
          onChange={(e) => sposta(Number(e.target.value))}
        />
      </div>
    </footer>
  );
}
