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
 * # Dove stanno le cose, e perché dipende
 *
 * I comandi si appoggiano **in fondo**, sempre. Il sommario — copertina, titolo,
 * artista — cambia posto con lo spettro, ed è l'unica cosa qui che si muove.
 *
 * Acceso lo spettro, si ritira **in alto a sinistra** e la copertina si fa
 * piccola: la fascia centrale è quella in cui la scena si legge meglio, e il
 * sommario in mezzo la coprirebbe proprio lì. Spento — cioè all'apertura — quel
 * vuoto non lo guarda nessuno, e tenerlo libero per una scena che non c'è
 * vorrebbe dire mostrare la copertina in un angolo con mezzo schermo intorno.
 * Allora torna **al centro**, e grande: chi apre questa schermata è venuto a
 * guardarla.
 *
 * Una striscia di dieci barre stava sotto il sommario, e non c'è più: diceva in
 * piccolo — dieci barre d'ottava — quel che la scena dice in grande, e copriva
 * la fascia in cui la scena si legge meglio.
 *
 * # Quel che è spento, e resta visibile
 *
 * Il bottone «testo» c'è, spento, e dice perché nel suggerimento: il nucleo non
 * legge ancora i testi, e una schermata vuota che si apre sarebbe peggio di un
 * bottone spento che spiega. Toglierlo del tutto nasconderebbe che il posto per
 * lui c'è ed è deciso.
 */
import { useEffect, useState } from "react";

import { Copertina, Sfocata } from "../Copertina";
import { RigheCoda, useRigheCoda } from "../Coda";
import { ipc, type Brano, type StatoRiproduzione } from "../ipc";
import { DettaglioSpettro } from "../parti/DettaglioSpettro";
import { Giudizio } from "../parti/Giudizio";
import { Icona } from "../parti/Icone";
import { Scrubber } from "../parti/Scrubber";
import { Spettro3D } from "../parti/Spettro3D";
import { Testo } from "../parti/Testo";
import { Trasporto } from "../parti/Trasporto";
import { t } from "../lingue";
import { nomeArtista, titoloAlbum } from "../formato";

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
  // Spento all'apertura come lo spettro, e per la stessa ragione: chi apre
  // questa schermata è venuto a guardare la copertina. Il testo è una seconda
  // cosa da guardare, e chi la vuole la accende.
  const [testoVisibile, setTestoVisibile] = useState(false);
  // Spento all'apertura, e di proposito: chi apre questa schermata è venuto a
  // guardare la copertina. Lo spettro è una seconda cosa da guardare, e chi la
  // vuole la accende — accendendola si accende anche la presa nel motore.
  const [spettroVisibile, setSpettroVisibile] = useState(false);
  // Quante barre disegna la scena. Parte da quel che dice il nucleo — la
  // preferenza sta in `settings`, non qui — e si chiede una volta sola,
  // all'apertura: è una lettura da una tabella di chiavi, non da mezza libreria.
  const [barre, setBarre] = useState(64);
  useEffect(() => {
    ipc.spettroBande().then(setBarre).catch(onErrore);
  }, [onErrore]);
  const scegliBarre = (quante: number) => {
    // Si prende quel che è rimasto e non quel che si è chiesto: fra le potenze
    // di due non c'è niente, e il nucleo porta alla più vicina un numero che
    // non è una di quelle.
    ipc.spettroBandeScegli(quante).then(setBarre).catch(onErrore);
  };
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
    <section
      className="np-screen in-riproduzione"
      aria-label={t("column.aria")}
    >
      <div className="ambiente ambient-backdrop" aria-hidden="true">
        <Sfocata hash={brano.coverArtHash} classe="tinta" />
      </div>
      <div className="velo np-scrim" aria-hidden="true" />
      {/* La scena sta **sopra** il velo e sotto tutto il resto: sotto il velo
          sarebbe schiacciata dal nero verticale che rende leggibile il titolo,
          e sopra il testo lo coprirebbe. Montata solo quando lo spettro è
          acceso: spenta non costa né una texture né un fotogramma, e nemmeno la
          presa nel motore — quella nasce e muore con questa tela. */}
      {spettroVisibile && <Spettro3D barre={barre} onErrore={onErrore} />}

      <header className="testa">
        <div className="chi-suona">
          <div className="occhiello hero-eyebrow">{t("np.fromAlbum")}</div>
        </div>

        <div className="comandi">
          {/* Acceso. Stava spento con la sua ragione nel suggerimento — «il
              nucleo non legge ancora i testi» — e adesso li legge: il sidecar
              accanto al brano, la riga in tabella, il tag. Il posto era deciso
              da prima, e questo è quel che ci è arrivato dentro. */}
          <button
            type="button"
            className="tasto icon-btn"
            aria-pressed={testoVisibile}
            aria-label={t("np.lyrics.toggle")}
            onClick={() => setTestoVisibile((prima) => !prima)}
          >
            <Icona nome="i-text" dim={16} titolo={t("np.lyrics")} />
          </button>
          {/* Questo invece è acceso, e lo è da quando il motore espone le
              bande: `aether-play::spettro` le prende dai campioni che escono
              davvero. La regola non è cambiata — quel che si disegna viene dal
              suono — è cambiato che adesso il suono si può guardare. */}
          <button
            type="button"
            className="tasto icon-btn"
            aria-pressed={spettroVisibile}
            aria-label={t("np.spectrum.toggle")}
            onClick={() => setSpettroVisibile((prima) => !prima)}
          >
            <Icona nome="i-eq" dim={16} titolo={t("np.spectrum")} />
          </button>
          <button
            type="button"
            className="tasto icon-btn"
            aria-pressed={codaVisibile}
            aria-label={t("np.queue.toggle")}
            onClick={() => setCodaVisibile((prima) => !prima)}
          >
            <Icona nome="i-queue" dim={16} />
          </button>
          <button
            type="button"
            className="pillola btn-ghost"
            onClick={onChiudi}
          >
            {t("common.close")}
            <kbd className="scorciatoia">{t("keys.esc")}</kbd>
            <Icona nome="i-chev-d" dim={13} />
          </button>
        </div>
      </header>

      <div
        className="corpo"
        data-con-coda={codaVisibile || undefined}
        data-con-testo={testoVisibile || undefined}
      >
        {/* Lo stato dello spettro arriva al CSS come attributo: quel che cambia
            accendendolo non è cosa c'è nella colonna, ma dove si appoggia — al
            centro o nell'angolo — e dove stanno le cose sta di là. */}
        <div className="centro" data-spettro={spettroVisibile || undefined}>
          {/* Il sommario: la copertina e, accanto o sotto, di chi è. Un
              contenitore suo e non due elementi sciolti nella colonna, perché
              sono una cosa sola che si sposta insieme — e perché il titolo deve
              poter andare a capo accanto alla copertina, non sotto di lei.

              La copertina sta da sola, senz'ombra. Aveva sotto sé stessa,
              sfocata e spostata in giù, a farle da riflesso: erano due
              sfocature della stessa immagine, una addosso all'altra, perché
              l'ambiente dietro è già la copertina sfocata a tutto schermo. La
              piccola sporcava la grande, e sotto la copertina restava una
              macchia del suo colore che nessuna luce di questa stanza
              spiegava. */}
          <div className="sommario-np">
            <Copertina
              hash={brano.coverArtHash}
              titolo={titoloAlbum(brano.album)}
              classe="np-art"
              piena
            />

            <div className="chi">
              <h1 className="np-title" title={brano.title}>
                {brano.title}
              </h1>
              <div className="np-meta">
                {nomeArtista(brano.artist)} · {titoloAlbum(brano.album)}
                {brano.year ? ` · ${brano.year}` : ""}
              </div>
            </div>
          </div>

          {/* In fondo, e tutti insieme: cursore, trasporto e giudizio sono la
              stessa cosa — i comandi — e stando in un contenitore si appoggiano
              al bordo di sotto con una riga di CSS invece di tre. */}
          <div className="comandi-np">
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
        </div>

        {/* Prima della coda: si legge da sinistra, e il testo è quel che si
            guarda mentre la coda è quel che viene dopo. */}
        {testoVisibile && <Testo brano={brano} onErrore={onErrore} />}

        {codaVisibile && (
          <aside className="coda-np" aria-label={t("queue.panel.aria")}>
            <header>
              <span className="occhiello hero-eyebrow">
                {t("np.queued", { n: stato.coda.length })}
              </span>
              <button
                type="button"
                className="bottone minuto btn-ghost"
                disabled={stato.coda.length === 0}
                onClick={() => comanda(ipc.codaSvuota())}
              >
                {t("queue.clear")}
              </button>
            </header>
            <RigheCoda
              stato={stato}
              righe={righe}
              onErrore={onErrore}
              compatta
            />
            <p className="come-si-riordina">
              {t("np.reorderHint.before")}
              <kbd>Alt</kbd>+<kbd>↑↓</kbd>
              {t("np.reorderHint.after")}
            </p>
          </aside>
        )}
      </div>

      {/* Fuori dal corpo, e dopo: il corpo ha `overflow: hidden` e questo si
          appoggia all'angolo della schermata, non della colonna. Fra elementi
          posizionati senza `z-index` decide l'ordine nel DOM, quindi stare qui
          basta a stare sopra tutto — senza aggiungere un quinto `z-index` da
          tenere allineato agli altri quattro. */}
      {spettroVisibile && (
        <DettaglioSpettro barre={barre} onBarre={scegliBarre} />
      )}
    </section>
  );
}
