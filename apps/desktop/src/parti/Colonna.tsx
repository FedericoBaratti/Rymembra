/**
 * La terza colonna: cosa sto ascoltando.
 *
 * # La decisione che regge tutta l'impalcatura
 *
 * **Non c'è una barra in fondo.** La colonna *è* il lettore: copertina grande,
 * titolo, cursore, trasporto, e sotto la coda. Una barra alta novanta pixel su
 * tutta la larghezza mostra una miniatura da quaranta e tre righe di testo
 * schiacciate; la stessa area, girata di novanta gradi, mostra una copertina da
 * trecento e la coda intera.
 *
 * Il patto è che la colonna si possa chiudere, e che quando è chiusa il lettore
 * flottante torni **identico a com'era**: chi preferisce la barra non perde
 * niente, e chi lavora su una finestra stretta non deve scegliere fra vedere la
 * musica e vedere la libreria. Sotto i 1100 px la colonna si chiude da sé — con
 * la barra di navigazione aperta, tre colonne su una finestra così lascerebbero
 * all'elenco meno della sua intestazione.
 *
 * # Perché la copertina tinge solo il primo terzo
 *
 * `--hero-rgb` è l'unico colore che viene dalla musica invece che dalla skin, e
 * l'unica regola che lo tiene onesto è che **non tocca mai il testo né una
 * superficie che deve restare leggibile**. Qui è una sfumatura radiale alta 340
 * px al 20% di opacità dietro la copertina, cioè dietro un'immagine: se una
 * copertina è di un rosa acceso, quel rosa sta dietro sé stesso, non sotto il
 * titolo del brano.
 */
import { useState } from "react";

import { Copertina, Sfocata } from "../Copertina";
import { RigheCoda, useRigheCoda } from "../Coda";
import { ipc, type Brano, type StatoRiproduzione } from "../ipc";
import { AvvisoAudio } from "./AvvisoAudio";
import { Cronologia } from "./Cronologia";
import { Giudizio } from "./Giudizio";
import { Icona } from "./Icone";
import { Scrubber } from "./Scrubber";
import { Segmentato } from "./Segmentato";
import { Trasporto } from "./Trasporto";
import { t } from "../lingue";
import { nomeArtista, titoloAlbum } from "../formato";

export function Colonna({
  stato,
  onChiudi,
  onEspandi,
  onPreferito,
  onVoto,
  onErrore,
}: {
  stato: StatoRiproduzione;
  onChiudi: () => void;
  onEspandi: () => void;
  onPreferito: (brano: Brano) => void;
  onVoto: (brano: Brano, stelle: number) => void;
  onErrore: (e: unknown) => void;
}) {
  const [scheda, setScheda] = useState<"coda" | "cronologia">("coda");
  const righe = useRigheCoda(stato.coda, onErrore);
  const brano = stato.brano;

  const comanda = (azione: Promise<void>) => {
    azione.catch(onErrore);
  };

  // Niente `app-shell`: quella classe dice «il contenitore di tutta la
  // finestra», e la porta la zona radice dello scafale.
  return (
    <aside className="colonna" aria-label={t("column.aria")}>
      {/* Lo strato ambientale è un fratello e non uno sfondo: come sfondo
          dovrebbe stare su un elemento che ha anche del testo, e allora la
          tinta della copertina finirebbe sotto delle lettere. */}
      <div className="ambiente ambient-backdrop" aria-hidden="true">
        <Sfocata hash={brano?.coverArtHash ?? null} classe="tinta" />
      </div>

      <header className="testa">
        <span className="occhiello hero-eyebrow">{t("column.aria")}</span>
        <button
          type="button"
          className="tasto icon-btn"
          aria-label={t("column.fullscreen")}
          title={t("column.fullscreen.title")}
          disabled={!brano}
          onClick={onEspandi}
        >
          <Icona nome="i-expand" dim={15} />
        </button>
        <button
          type="button"
          className="tasto icon-btn"
          aria-label={t("column.close")}
          title={t("common.close")}
          onClick={onChiudi}
        >
          <Icona nome="i-x" dim={15} />
        </button>
      </header>

      {/* Sopra tutto il resto, copertina compresa: è la risposta alla domanda
          «perché non si sente niente», e quella domanda viene prima di
          qualunque cosa ci sia da guardare. Prima di questa fascia il nucleo
          alzava un bit che nessuno leggeva, e staccare le cuffie voleva dire
          premere play senza sentire niente, per sempre. */}
      <AvvisoAudio stato={stato} dove="colonna" onErrore={onErrore} />

      {brano === null ? (
        <div className="niente-in-ascolto empty-state">
          <span className="empty-icon" aria-hidden="true">
            <Icona nome="i-album" dim={28} />
          </span>
          <p>{t("nothing.playing")}</p>
          <p className="sotto">{t("nothing.playing.hint")}</p>
        </div>
      ) : (
        <>
          <button
            type="button"
            className="apri-grande"
            aria-label={t("column.fullscreen")}
            onClick={onEspandi}
          >
            <Copertina
              hash={brano.coverArtHash}
              titolo={titoloAlbum(brano.album)}
              classe="hero-art"
              piena
            />
          </button>

          <div className="chi">
            <div className="titolo np-title" title={brano.title}>
              {brano.title}
            </div>
            <div
              className="meta np-meta"
              title={`${nomeArtista(brano.artist)} · ${titoloAlbum(brano.album)}`}
            >
              {nomeArtista(brano.artist)} · {titoloAlbum(brano.album)}
            </div>
          </div>

          <Scrubber stato={stato} onErrore={onErrore} />

          <Trasporto stato={stato} taglia="colonna" onErrore={onErrore} />

          <Giudizio
            stato={stato}
            brano={brano}
            taglia="colonna"
            onPreferito={onPreferito}
            onVoto={onVoto}
            onErrore={onErrore}
          />
        </>
      )}

      <div className="coda-in-colonna">
        <div className="testa-coda">
          <Segmentato
            etichetta={t("column.tabs")}
            scelta={scheda}
            onScegli={setScheda}
            classe="minuto"
            voci={[
              {
                chiave: "coda",
                etichetta: t("column.tab.queue"),
                conteggio: stato.coda.length,
              },
              { chiave: "cronologia", etichetta: t("column.tab.history") },
            ]}
          />
          {/* «Svuota» è della coda, e con la cronologia aperta non avrebbe
              niente da svuotare che sia sotto gli occhi. Sparisce invece di
              spegnersi: un tasto spento accanto a un elenco che non è il suo è
              un tasto che invita a chiedersi cosa cancellerebbe. */}
          {scheda === "coda" && (
            <button
              type="button"
              className="bottone minuto btn-ghost"
              disabled={stato.coda.length === 0}
              onClick={() => comanda(ipc.codaSvuota())}
            >
              {t("queue.clear")}
            </button>
          )}
        </div>
        {scheda === "coda" ? (
          <RigheCoda stato={stato} righe={righe} onErrore={onErrore} compatta />
        ) : (
          <Cronologia stato={stato} onErrore={onErrore} />
        )}
      </div>
    </aside>
  );
}
