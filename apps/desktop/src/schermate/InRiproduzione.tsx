/**
 * In riproduzione, a schermo quasi intero.
 *
 * # Perché «quasi»
 *
 * La barra di navigazione **resta**. Coprire tutta la finestra sembra la scelta
 * ovvia — è quel che fanno quasi tutti i lettori — e costa una cosa che si paga
 * ogni volta: per cambiare vista bisogna prima chiudere. Con la barra al suo
 * posto, guardare la copertina grande e passare agli Album sono due gesti
 * indipendenti, e nessuno dei due annulla l'altro.
 *
 * # Il velo è piatto, e non è un dettaglio
 *
 * Dietro c'è l'ambiente: due sfumature radiali che prendono il colore dalla
 * copertina. Sopra c'è `np-scrim`, che **non** è colorato — è un nero verticale
 * dal 42% all'80%, scritto con `--surface-0-rgb` così che una skin chiara ne
 * ottenga uno bianco.
 *
 * Sono due strati e non uno perché fare del velo una versione più scura della
 * tinta vorrebbe dire che il testo sta sopra un colore imprevedibile: con una
 * copertina gialla il titolo si leggerebbe su giallo scuro, che è ancora giallo.
 * Con due strati il testo sta sopra un velo neutro, e la tinta si vede intorno.
 *
 * # Quel che è spento, e resta visibile
 *
 * I due bottoni «testo» ed «equalizzatore» ci sono, spenti, e dicono perché. Il
 * nucleo non espone né i testi né le bande — `aether-play` non ha né gli uni né
 * le altre — e disegnare uno spettro finto dietro la copertina sarebbe l'unica
 * bugia dell'interfaccia. Toglierli del tutto nasconderebbe che il posto per
 * loro c'è ed è deciso.
 */
import { useEffect, useState } from "react";

import { Copertina, Sfocata } from "../Copertina";
import { RigheCoda, useRigheCoda } from "../Coda";
import { ipc, type Brano, type StatoRiproduzione } from "../ipc";
import { Giudizio } from "../parti/Giudizio";
import { Icona } from "../parti/Icone";
import { Scrubber } from "../parti/Scrubber";
import { Spettro } from "../parti/Spettro";
import { Trasporto } from "../parti/Trasporto";

export function InRiproduzione({
  stato,
  onChiudi,
  onPreferito,
  onVoto,
  onErrore,
}: {
  stato: StatoRiproduzione;
  onChiudi: () => void;
  onPreferito: (brano: Brano) => void;
  onVoto: (brano: Brano, stelle: number) => void;
  onErrore: (e: unknown) => void;
}) {
  const [codaVisibile, setCodaVisibile] = useState(true);
  // Spento all'apertura, e di proposito: chi apre questa schermata è venuto a
  // guardare la copertina. Lo spettro è una seconda cosa da guardare, e chi la
  // vuole la accende — accendendola si accende anche la presa nel motore.
  const [spettroVisibile, setSpettroVisibile] = useState(false);
  const righe = useRigheCoda(stato.coda, onErrore);
  const brano = stato.brano;

  // Chi apre questa schermata mentre non suona niente resterebbe davanti a un
  // fondo vuoto senza modo di capire cosa è successo. Non capita dai tre modi
  // previsti per aprirla — sono tutti spenti senza un brano — ma capita se il
  // brano finisce mentre è aperta.
  useEffect(() => {
    if (brano === null) onChiudi();
  }, [brano, onChiudi]);

  if (brano === null) return null;

  const comanda = (azione: Promise<void>) => {
    azione.catch(onErrore);
  };

  return (
    <section className="np-screen in-riproduzione" aria-label="In riproduzione">
      <div className="ambiente ambient-backdrop" aria-hidden="true">
        <Sfocata hash={brano.coverArtHash} classe="tinta" />
      </div>
      <div className="velo np-scrim" aria-hidden="true" />

      <header className="testa">
        <div className="chi-suona">
          <div className="occhiello hero-eyebrow">In riproduzione dall&apos;album</div>
          <div className="disco">
            {brano.album} · {brano.artist}
          </div>
        </div>

        <div className="comandi">
          {/* Spenti, e con la loro ragione nel suggerimento. Il posto c'è ed è
              deciso; quel che manca è nel nucleo, non qui. */}
          <span className="con-suggerimento">
            <button type="button" className="tasto icon-btn" aria-disabled="true" disabled>
              <Icona nome="i-text" dim={16} titolo="Testo" />
            </button>
            <span className="tooltip-pill" role="tooltip">
              Nessun testo per questo brano: il nucleo non li legge ancora
            </span>
          </span>
          {/* Questo invece è acceso, e lo è da quando il motore espone le
              bande: `aether-play::spettro` le prende dai campioni che escono
              davvero. La regola non è cambiata — quel che si disegna viene dal
              suono — è cambiato che adesso il suono si può guardare. */}
          <button
            type="button"
            className="tasto icon-btn"
            aria-pressed={spettroVisibile}
            aria-label="Mostra o nascondi lo spettro"
            onClick={() => setSpettroVisibile((prima) => !prima)}
          >
            <Icona nome="i-eq" dim={16} titolo="Spettro" />
          </button>
          <button
            type="button"
            className="tasto icon-btn"
            aria-pressed={codaVisibile}
            aria-label="Mostra o nascondi la coda"
            onClick={() => setCodaVisibile((prima) => !prima)}
          >
            <Icona nome="i-queue" dim={16} />
          </button>
          <button type="button" className="pillola btn-ghost" onClick={onChiudi}>
            Chiudi
            <kbd className="scorciatoia">Esc</kbd>
            <Icona nome="i-chev-d" dim={13} />
          </button>
        </div>
      </header>

      <div className="corpo" data-con-coda={codaVisibile || undefined}>
        <div className="centro">
          {/* Il quadro tiene insieme due strati: la copertina e, sotto, la
              stessa copertina sfocata che le fa da riflesso. Senza un
              contenitore l'alone dovrebbe essere `position: absolute` rispetto
              al centro, cioè lungo quanto tutta la colonna. */}
          <div className="quadro-np">
            <Sfocata hash={brano.coverArtHash} classe="alone" />
            <Copertina
              hash={brano.coverArtHash}
              titolo={brano.album}
              classe="np-art"
              piena
            />
          </div>

          <h1 className="np-title" title={brano.title}>
            {brano.title}
          </h1>
          <div className="np-meta">
            {brano.artist} · {brano.album}
            {brano.year ? ` · ${brano.year}` : ""}
          </div>

          {/* Fra la copertina e il cursore, non sopra: lo spettro accompagna
              il brano, non lo sostituisce. Montato solo quando serve, così la
              presa nel motore vive esattamente quanto la canvas. */}
          {spettroVisibile && <Spettro onErrore={onErrore} />}

          <Scrubber stato={stato} onErrore={onErrore} />

          <Trasporto stato={stato} taglia="grande" onErrore={onErrore} />

          <Giudizio
            stato={stato}
            brano={brano}
            taglia="grande"
            onPreferito={onPreferito}
            onVoto={onVoto}
            onErrore={onErrore}
          />
        </div>

        {codaVisibile && (
          <aside className="coda-np" aria-label="Coda di riproduzione">
            <header>
              <span className="occhiello hero-eyebrow">In coda · {stato.coda.length}</span>
              <button
                type="button"
                className="bottone minuto btn-ghost"
                disabled={stato.coda.length === 0}
                onClick={() => comanda(ipc.codaSvuota())}
              >
                Svuota
              </button>
            </header>
            <RigheCoda stato={stato} righe={righe} onErrore={onErrore} compatta />
            <p className="come-si-riordina">
              Trascina per riordinare · <kbd>Alt</kbd>+<kbd>↑↓</kbd> da tastiera
            </p>
          </aside>
        )}
      </div>
    </section>
  );
}
