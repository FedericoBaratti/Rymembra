/**
 * Chi sta suonando: copertina piccola, titolo, autore, cuore.
 *
 * Ne esiste una copia sola, nella barra — e si estrae lo stesso, perché è la
 * risposta a «cosa sto sentendo» nella sua forma più stretta, cioè un widget
 * del registro e non un pezzo della barra. Il giorno in cui una skin lo mette
 * in cima alla colonna, il componente c'è già.
 */
import { memo } from "react";

import { Copertina } from "../Copertina";
import { idDiLibreria } from "../insieme";
import type { Brano } from "../ipc";
import { statoSuono } from "../segnali";
import { Icona } from "./Icone";
import { t, useLingua } from "../lingue";
import { nomeArtista, titoloAlbum } from "../formato";

export const Ora = memo(function Ora({
  brano,
  inPausa,
  conCopertina = true,
  conCuore = true,
  onPreferito,
}: {
  brano: Brano;
  inPausa: boolean;
  /** La copertina in miniatura. */
  conCopertina?: boolean;
  /** Il cuore dei preferiti. */
  conCuore?: boolean;
  onPreferito: (brano: Brano) => void;
}) {
  // Sotto `memo` (vedi «I widget sotto `memo`» in `Impaginazione.tsx`): si
  // ridisegna per le sue prop, e per la lingua, che non è una prop — senza
  // l'iscrizione i testi resterebbero quelli del primo disegno.
  useLingua();
  const suono = statoSuono(true, inPausa);
  return (
    <div className="ora now-playing" {...suono}>
      {conCopertina && (
        <Copertina
          hash={brano.coverArtHash}
          titolo={titoloAlbum(brano.album)}
          classe="miniatura np-thumb"
          suono={suono}
          cambio
          /* Titolo e album sono scritti accanto: vedi `Copertina`. */
          accanto
        />
      )}
      <div className="chi">
        <div className="nome" title={brano.title} dir="auto">
          {brano.title}
        </div>
        <div
          className="autore"
          title={`${nomeArtista(brano.artist)} · ${titoloAlbum(brano.album)}`}
        >
          <bdi>{nomeArtista(brano.artist)}</bdi> · <bdi>{titoloAlbum(brano.album)}</bdi>
        </div>
      </div>
      {/* Il cuore scrive sulla riga della libreria, e una voce di «Ascolta
          insieme» su questo PC una riga non ce l'ha: vedi `idDiLibreria`. */}
      {conCuore && idDiLibreria(brano.id) && (
        <button
          type="button"
          className="cuore icon-btn heart"
          aria-pressed={brano.liked}
          aria-label={brano.liked ? t("track.unlike") : t("track.like")}
          onClick={() => onPreferito(brano)}
        >
          <Icona nome={brano.liked ? "i-heart-f" : "i-heart"} dim={17} />
        </button>
      )}
    </div>
  );
});
