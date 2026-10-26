/**
 * La barra della selezione multipla.
 *
 * # Perché prende il posto del lettore
 *
 * Perché è la stessa domanda in due momenti diversi: «cosa sto per fare adesso».
 * Metterla altrove — in cima, o sopra il lettore — vorrebbe dire due barre
 * insieme, e la seconda coprirebbe comandi che in quel momento non servono a
 * nessuno: chi ha selezionato quaranta brani non sta cercando il tasto pausa.
 *
 * Sparisce da sola quando la selezione si svuota, e si svuota con `Esc`.
 *
 * # Perché i comandi sono gli stessi del menù contestuale
 *
 * Sono le stesse tre cose — riproduci dopo, accoda, aggiungi a playlist — e
 * questo è il punto: **ogni comando ha almeno due porte**. Finché esistevano
 * solo nel menù del tasto destro, erano invisibili a chi non prova il tasto
 * destro, e irraggiungibili per chi non ha un mouse.
 *
 * `apriMenu(e, elenco)` prendeva già un array di identificativi e veniva
 * chiamato sempre con uno solo: la selezione multipla è quel parametro che
 * finalmente serve a qualcosa.
 */
import { memo } from "react";

import { brani_ } from "../formato";
import { gestoPermesso } from "../insieme";
import { Icona } from "./Icone";
import { t, useLingua } from "../lingue";
import { Presenza } from "../presenza";
import { useLettore } from "../riproduzione";

export const BarraSelezione = memo(function BarraSelezione({
  quanti,
  onRiproduci,
  onDopo,
  onAccoda,
  onPlaylist,
  onTuttiOAnnulla,
  tuttiSelezionati,
  onChiudi,
}: {
  quanti: number;
  onRiproduci: () => void;
  onDopo: () => void;
  onAccoda: () => void;
  onPlaylist: () => void;
  onTuttiOAnnulla: () => void;
  tuttiSelezionati: boolean;
  onChiudi: () => void;
}) {
  // Sotto `memo` (vedi «I widget sotto `memo`» in `Impaginazione.tsx`): si
  // ridisegna per le sue prop, e per la lingua, che non è una prop — senza
  // l'iscrizione i testi resterebbero quelli del primo disegno.
  useLingua();
  // In una sessione dove chi ospita non ha dato la coda a questo PC, i tre
  // gesti che la toccano si spengono con la loro ragione, come i tasti del
  // trasporto: un tasto acceso che risponde «non concesso» insegna che il
  // lettore è rotto. Il nucleo si difende comunque da sé.
  const codaLibera = useLettore((s) => gestoPermesso(s.insieme, "coda"));
  const spento = codaLibera ? undefined : t("together.gestures.off.title");
  return (
    <Presenza parte="selection-bar">
      <div
        className="barra-selezione selection-bar"
        role="toolbar"
        aria-label={t("selection.aria")}
      >
        <span className="quanti">{brani_(quanti)}</span>

        <button
          type="button"
          className="bottone minuto btn-ghost"
          onClick={onTuttiOAnnulla}
        >
          {tuttiSelezionati
            ? t("selection.deselectAll")
            : t("selection.selectAll")}
        </button>

        <div className="separatore" aria-hidden="true" />

        <button
          type="button"
          className="bottone minuto btn-accent"
          disabled={!codaLibera}
          title={spento}
          onClick={onRiproduci}
        >
          <Icona nome="i-play" dim={14} />
          {t("action.play")}
        </button>
        <button
          type="button"
          className="bottone minuto btn-ghost"
          disabled={!codaLibera}
          title={spento}
          onClick={onDopo}
        >
          {t("action.playNext")}
        </button>
        <button
          type="button"
          className="bottone minuto btn-ghost"
          disabled={!codaLibera}
          title={spento}
          onClick={onAccoda}
        >
          <Icona nome="i-queue" dim={14} />
          {t("action.enqueue")}
        </button>
        <button
          type="button"
          className="bottone minuto btn-ghost"
          onClick={onPlaylist}
        >
          <Icona nome="i-plus" dim={14} />
          {t("action.addToPlaylist")}
        </button>

        <div className="spinta" />

        <button
          type="button"
          className="tasto icon-btn"
          aria-label={t("selection.clear")}
          title={t("selection.clear.title")}
          onClick={onChiudi}
        >
          <Icona nome="i-x" dim={15} />
        </button>
      </div>
    </Presenza>
  );
});
