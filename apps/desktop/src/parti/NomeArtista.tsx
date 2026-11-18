/**
 * Il nome dell'artista in «In riproduzione»: un pulsante che apre la sua pagina.
 *
 * Era testo semplice, e un tocco non faceva niente. Adesso apre la pagina
 * dell'artista **effettivo** — quello del disco se c'è (`artistaEffettivo`) — e
 * Indietro torna a «In riproduzione» (`App`, `tornaAInRiproduzione`). Vale per
 * le righe della libreria, anche quelle di un altro dispositivo: sono righe
 * specchio con un id positivo.
 *
 * # Una voce di «Ascolta insieme» resta testo
 *
 * Non è nella libreria: l'artista che la sessione dice è un nome che questa
 * libreria può non avere, e cercarlo per nome porterebbe a un elenco vuoto o a
 * un omonimo. Come cuore, stelle e menù (`idDiLibreria`), il gesto c'è solo dove
 * c'è una riga su cui farlo.
 *
 * # Il nome del pulsante, per chi ascolta lo schermo
 *
 * Il testo visibile è l'interprete, la pagina può essere un'altra: su un brano
 * di una raccolta il pulsante dice «Tale» e apre «Vari artisti». Il nome
 * accessibile comincia dal testo che si vede, e dice dove porta.
 */
import { paginaDellArtista } from "../artista";
import { nomeArtista } from "../formato";
import type { Brano } from "../ipc";
import { t } from "../lingue";

export function NomeArtista({
  brano,
  onApri,
}: {
  brano: Brano;
  /** Apre la pagina dell'artista del brano. Assente: il nome è solo testo. */
  onApri?: ((brano: Brano) => void) | undefined;
}) {
  const visibile = nomeArtista(brano.artist);
  const apre = paginaDellArtista(brano);
  if (onApri === undefined || apre === null || visibile === "") {
    return <bdi>{visibile}</bdi>;
  }
  const pagina = nomeArtista(apre);
  return (
    <button
      type="button"
      className="artista-link"
      dir="auto"
      aria-label={
        pagina === visibile
          ? t("artist.open.aria", { artista: visibile })
          : t("artist.openOther.aria", { artista: visibile, pagina })
      }
      onClick={() => onApri(brano)}
    >
      {visibile}
    </button>
  );
}
