/**
 * Il mini lettore del telefono: la scheda sopra le schede.
 *
 * Sul computer il lettore è una barra con tutto — cursore, formato, coda,
 * equalizzatore, volume — perché lì c'è posto, e la colonna accanto ne è il
 * resto. Sul telefono la barra era la stessa, con mezzi tasti nascosti: niente
 * avanzamento, e nessuna strada per la coda. Qui è una scheda che dice tre cose
 * e ne fa tre:
 *
 * - **cosa suona**: la copertina, il titolo, chi;
 * - **quanto manca**: una linea sottile sul bordo di sotto, che avanza;
 * - **pausa e avanti**, i due tasti che si premono senza guardare. «Indietro»
 *   no: sta a schermo intero, e in una scheda da trecento pixel tre tasti in
 *   fila sono tre bersagli stretti.
 *
 * E risponde ai gesti che la mano prova per primi su una scheda così: un tocco
 * — o un colpo verso l'alto — apre «In riproduzione»; una strisciata di lato
 * cambia brano, verso sinistra il prossimo e verso destra quello prima, con la
 * scheda che segue il dito e una vibrazione quando il cambio parte. Ogni gesto
 * ha il suo tasto, da qui o dallo schermo intero: chi non striscia non perde
 * niente.
 */
import { memo, useLayoutEffect, useRef, useState } from "react";

import { Copertina } from "../Copertina";
import { useDito } from "../dito";
import { nomeArtista } from "../formato";
import { esitoStrisciata } from "../gesti";
import { branoDaMostrare, gestoPermesso } from "../insieme";
import { ipc, type StatoRiproduzione } from "../ipc";
import { t, useLingua } from "../lingue";
import { passoDellaBarra } from "../posizione";
import { usePosizioneAlPasso } from "../riproduzione";
import { statoSuono } from "../segnali";
import { vibra } from "../vibra";
import { Icona } from "./Icone";

/** Quanto in su basta per aprire «In riproduzione» trascinando. */
const SU_PER_APRIRE = 32;

/**
 * La linea dell'avanzamento.
 *
 * Una foglia a parte, e l'unica che si iscrive alla posizione: la scheda
 * intera non deve ridisegnarsi quattro volte al secondo per spostare una
 * linea. Il passo lo sceglie `passoDellaBarra` sulla sua larghezza vera — su
 * una linea di trecento pixel, un brano di quattro minuti avanza di un pixel
 * ogni ottocento millisecondi, e ridisegnarla più spesso non cambia niente di
 * visibile — e il movimento lo fa `scaleX`, che il compositore sposta senza
 * rifare la pagina.
 */
function LineaAvanzamento({ durataMs }: { durataMs: number }) {
  const linea = useRef<HTMLDivElement>(null);
  const [larghezza, setLarghezza] = useState(0);
  useLayoutEffect(() => {
    const nodo = linea.current;
    if (nodo === null) return;
    const misura = () => setLarghezza(nodo.getBoundingClientRect().width);
    misura();
    const osservatore = new ResizeObserver(misura);
    osservatore.observe(nodo);
    return () => osservatore.disconnect();
  }, []);
  const posizione = usePosizioneAlPasso(
    passoDellaBarra(durataMs, larghezza * window.devicePixelRatio),
  );
  const quanto = durataMs > 0 ? Math.min(1, Math.max(0, posizione / durataMs)) : 0;
  return (
    <div ref={linea} className="linea-mini" aria-hidden="true">
      <span className="riempimento-mini" style={{ transform: `scaleX(${quanto})` }} />
    </div>
  );
}

export const MiniLettore = memo(function MiniLettore({
  stato,
  onApri,
  onErrore,
}: {
  stato: StatoRiproduzione;
  /** Apre «In riproduzione» a schermo intero. */
  onApri: () => void;
  onErrore: (e: unknown) => void;
}) {
  useLingua();
  const scheda = useRef<HTMLDivElement>(null);
  const [trascinando, setTrascinando] = useState(false);
  const trasporto = gestoPermesso(stato.insieme, "trasporto");

  /** Riporta la scheda al suo posto, con la transizione del foglio. */
  const rimetti = () => {
    const nodo = scheda.current;
    if (nodo === null) return;
    nodo.style.removeProperty("--spinta-x");
    nodo.style.removeProperty("--spinta-y");
    nodo.style.removeProperty("--spinta-o");
  };

  const dito = useDito({
    assi: "xy",
    onParte: () => setTrascinando(true),
    onMuovi: (dx, dy, asse) => {
      const nodo = scheda.current;
      if (nodo === null) return;
      if (asse === "x") {
        // Di lato solo se si può cambiare brano: in una sessione in cui il
        // trasporto non è concesso la scheda resta ferma, e dice così che il
        // gesto non c'è.
        if (!trasporto) return;
        const larghezza = nodo.offsetWidth || 1;
        nodo.style.setProperty("--spinta-x", `${dx}px`);
        nodo.style.setProperty("--spinta-o", `${1 - Math.min(Math.abs(dx) / larghezza, 0.6)}`);
      } else {
        // In su la scheda sale appena, e in giù non va: il gesto che conta è
        // uno, aprire.
        nodo.style.setProperty("--spinta-y", `${Math.max(Math.min(dy, 0), -40) / 2}px`);
      }
    },
    onFine: ({ asse, dx, dy, vx, vy }) => {
      setTrascinando(false);
      rimetti();
      if (asse === "y") {
        if (dy <= -SU_PER_APRIRE || vy <= -0.5) onApri();
        return;
      }
      if (!trasporto) return;
      const esito = esitoStrisciata({
        dx,
        vx,
        larghezza: scheda.current?.offsetWidth ?? 1,
      });
      if (esito === null) return;
      vibra("scatto");
      (esito === "prossimo" ? ipc.prossimo() : ipc.precedente()).catch(onErrore);
    },
    onAnnulla: () => {
      setTrascinando(false);
      rimetti();
    },
  });

  const brano = stato.brano;
  if (brano === null) return null;
  const mostra = branoDaMostrare(brano, stato.insieme, t);
  const suono = statoSuono(true, stato.inPausa);

  return (
    <div
      ref={scheda}
      className="mini-lettore"
      {...suono}
      data-trascinando={trascinando || undefined}
      onPointerDown={dito.onPointerDown}
    >
      <button
        type="button"
        className="apri-mini"
        aria-label={t("player.open.aria", { titolo: mostra.title })}
        onClick={onApri}
      >
        <Copertina
          hash={mostra.coverArtHash}
          titolo={mostra.album}
          classe="copertina-mini"
          suono={suono}
          cambio
          accanto
        />
        <span className="testi-mini">
          <span className="titolo-mini" dir="auto">
            {mostra.title}
          </span>
          <span className="sotto-mini" dir="auto">
            {nomeArtista(mostra.artist)}
          </span>
        </span>
      </button>
      <button
        type="button"
        className="tasto-mini"
        aria-label={stato.inPausa ? t("player.resume") : t("player.pause")}
        disabled={!trasporto}
        onClick={() => ipc.alterna().catch(onErrore)}
      >
        <Icona nome={stato.inPausa ? "i-play" : "i-pause"} dim={26} />
      </button>
      <button
        type="button"
        className="tasto-mini"
        aria-label={t("player.next")}
        disabled={!trasporto}
        onClick={() => ipc.prossimo().catch(onErrore)}
      >
        <Icona nome="i-next" dim={24} />
      </button>
      <LineaAvanzamento durataMs={stato.durataMs} />
    </div>
  );
});
