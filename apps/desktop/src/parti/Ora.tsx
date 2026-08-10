/**
 * Chi sta suonando: copertina piccola, titolo, autore, cuore.
 *
 * Ne esiste una copia sola, nella barra — e si estrae lo stesso, perché è la
 * risposta a «cosa sto sentendo» nella sua forma più stretta, cioè un widget
 * del registro e non un pezzo della barra. Il giorno in cui una skin lo mette
 * in cima alla colonna, il componente c'è già.
 */
import { Copertina } from "../Copertina";
import type { Brano } from "../ipc";
import { Icona } from "./Icone";

export function Ora({
  brano,
  conCopertina = true,
  conCuore = true,
  onPreferito,
}: {
  brano: Brano;
  /** La copertina in miniatura. */
  conCopertina?: boolean;
  /** Il cuore dei preferiti. */
  conCuore?: boolean;
  onPreferito: (brano: Brano) => void;
}) {
  return (
    <div className="ora">
      {conCopertina && (
        <Copertina hash={brano.coverArtHash} titolo={brano.album} classe="miniatura" />
      )}
      <div className="chi">
        <div className="nome" title={brano.title}>
          {brano.title}
        </div>
        <div className="autore" title={`${brano.artist} · ${brano.album}`}>
          {brano.artist} · {brano.album}
        </div>
      </div>
      {conCuore && (
        <button
          type="button"
          className="cuore icon-btn"
          aria-pressed={brano.liked}
          aria-label={brano.liked ? "Togli dai preferiti" : "Aggiungi ai preferiti"}
          onClick={() => onPreferito(brano)}
        >
          <Icona nome={brano.liked ? "i-heart-f" : "i-heart"} dim={17} />
        </button>
      )}
    </div>
  );
}
