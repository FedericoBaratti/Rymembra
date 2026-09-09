/**
 * Il cursore della posizione nel brano.
 *
 * # Perché lo stato del trascinamento sta qui
 *
 * È l'unico stato di questa interfaccia che il nucleo non conosce, ed è giusto
 * che non lo conosca: una posizione *voluta* e non ancora avvenuta. Finché il
 * dito è giù, `aether-play` sta ancora suonando dov'era, e chi trascina deve
 * vedere dove sta andando invece di dove è.
 *
 * Stava scritto in tre componenti — la barra, la colonna e lo schermo intero —
 * insieme alle stesse quattordici righe di `<input type="range">` e alla stessa
 * `rilascia()`. Tre copie di uno stato sono tre occasioni di correggerne due.
 *
 * # Perché la posizione non è una prop
 *
 * Perché questo è **l'unico** componente che la disegna. Scendeva da `App`
 * attraverso `contesto`, `Impaginazione`, `Lettore` e `Colonna` — cinque
 * livelli che non ne facevano niente — e cambiando venti volte al secondo li
 * faceva ridisegnare tutti, elenco dei brani compreso. Letta qui, il suo
 * cambiamento arriva a un componente solo.
 */
import { useState, type CSSProperties } from "react";

import { durata } from "../formato";
import { ipc, type StatoRiproduzione } from "../ipc";
import { usePosizioneMs } from "../riproduzione";
import { t } from "../lingue";

export function Scrubber({
  stato,
  conTempi = true,
  onErrore,
}: {
  stato: StatoRiproduzione;
  /** I due tempi ai lati. Toglierli lascia la sola barra. */
  conTempi?: boolean;
  onErrore: (e: unknown) => void;
}) {
  const posizioneMs = usePosizioneMs();
  const [trascinato, setTrascinato] = useState<number | null>(null);
  // Sopra `rilascia` perché la legge anche lei, e non solo il disegno: il tetto
  // del salto e il tetto di quel che si vede sono lo stesso numero, e tenerli a
  // due righe di distanza è il modo di non correggerne uno solo.
  const durataMs = stato.durataMs;

  const rilascia = async () => {
    if (trascinato === null) return;
    try {
      // `vai_a` manda lo stato prima di rispondere, e con dentro i millisecondi
      // **richiesti**: il motore ci arriva sul filo suo poco dopo, ma quando
      // questa promessa si risolve la posizione nuova è già stata annunciata.
      // Togliere il trascinamento non fa quindi lampeggiare il cursore sul
      // punto di partenza — che è quel che succedeva finché lo stato portava
      // la posizione letta dal motore, cioè quella di prima del salto.
      // Con lo stesso tetto del numero *mostrato* qui sotto, e non con quello
      // grezzo: `max` è `durataMs`, quindi il cursore tirato fino in fondo
      // manda esattamente la durata dichiarata dal database. Su un flusso —
      // dove la durata vera la conosce solo il decodificatore — quel numero è
      // spesso oltre l'ultimo campione, e chiedere un salto oltre la fine è
      // chiedere la fine: il gesto «vai in fondo» diventava «brano successivo».
      await ipc.vaiA(Math.min(trascinato, durataMs));
    } catch (e) {
      onErrore(e);
    }
    setTrascinato(null);
  };

  const dove = trascinato ?? posizioneMs;
  const avanzamento = durataMs > 0 ? (dove / durataMs) * 100 : 0;

  return (
    <div className="cursore player-progress">
      {conTempi && <span className="tempo">{durata(dove)}</span>}
      <input
        type="range"
        className="scorrimento range-accent"
        min={0}
        max={Math.max(durataMs, 1)}
        step={250}
        value={Math.min(dove, durataMs)}
        /* Il riempimento passa da una variabile invece che da un gradiente
           ricomposto a ogni fotogramma: è una proprietà custom sola, e il
           motore di rendering la risolve senza rileggere la regola. */
        style={{ "--avanzamento": `${avanzamento}%` } as CSSProperties}
        aria-label={t("player.position")}
        disabled={durataMs === 0}
        onChange={(e) => setTrascinato(Number(e.target.value))}
        onPointerUp={() => void rilascia()}
        onKeyUp={() => void rilascia()}
        onBlur={() => void rilascia()}
      />
      {conTempi && <span className="tempo">{durata(durataMs)}</span>}
    </div>
  );
}
