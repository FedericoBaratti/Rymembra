/**
 * «Copia», e il modo in cui dice d'aver copiato.
 *
 * # Da dove viene
 *
 * Il primo tasto dell'albero che scriveva negli appunti fu quello
 * dell'indirizzo di questo PC, in «Ascolta insieme» (`IndirizziDiQui`), e lì
 * nacque l'idioma, il più piccolo che bastasse: la scritta diventa «Copiato»
 * per due secondi, una regione viva lo dice a chi ascolta lo schermo, e se gli
 * appunti rifiutano il testo resta selezionabile e una riga dice di copiarlo a
 * mano. Le app dell'utente ne hanno una dozzina — gli indirizzi di ritorno e i
 * permessi da incollare in una console, esatti — e una copia dell'idioma per
 * ciascuna sarebbe una dozzina di timer da tenere d'accordo.
 *
 * # Due pezzi
 *
 * `useCopia` è la parte che non si vede: la chiamata agli appunti, l'esito, i
 * due secondi. `Copia` è il tasto con la sua regione viva e la riga del
 * ripiego, per chi non ha un posto suo dove dirli. «Ascolta insieme» usa il
 * primo e tiene il disegno di prima: la riga del ripiego lì sta sotto
 * l'indirizzo, non accanto al tasto.
 */
import { useEffect, useState } from "react";

import { t } from "../lingue";
import { Annuncio } from "./Annuncio";

/** Quanto resta scritto «Copiato» sul tasto, in millisecondi. */
export const DURATA_COPIATO = 2_000;

/** Com'è andata l'ultima copia: quale testo, e se gli appunti l'hanno preso. */
export interface EsitoCopia {
  testo: string;
  riuscito: boolean;
}

/**
 * Copia negli appunti, e ricorda com'è andata.
 *
 * Un esito riuscito si spegne da sé dopo [`DURATA_COPIATO`]; uno fallito resta
 * finché non si riprova, perché la riga che dice di copiare a mano deve
 * restare lì mentre lo si fa.
 */
export function useCopia(): { esito: EsitoCopia | null; copia: (testo: string) => void } {
  const [esito, setEsito] = useState<EsitoCopia | null>(null);
  useEffect(() => {
    if (esito === null || !esito.riuscito) return;
    const conto = window.setTimeout(() => setEsito(null), DURATA_COPIATO);
    return () => window.clearTimeout(conto);
  }, [esito]);

  const copia = (testo: string) => {
    // Dentro una promessa anche la chiamata: senza appunti (`clipboard`
    // assente) l'errore è sincrono, e deve finire nello stesso ramo.
    Promise.resolve()
      .then(() => navigator.clipboard.writeText(testo))
      .then(
        () => setEsito({ testo, riuscito: true }),
        () => setEsito({ testo, riuscito: false }),
      );
  };

  return { esito, copia };
}

/**
 * Il tasto «Copia» per un testo, con la sua regione viva.
 *
 * Il nome per chi ascolta dice **che cosa** copia (`etichetta`, di serie
 * «Copia …» col testo): sei «Copia» di fila, sentiti senza vederli, sono sei
 * tasti uguali.
 */
export function Copia({
  testo,
  etichetta,
}: {
  testo: string;
  /** Il nome del tasto per chi ascolta. Di serie «Copia {testo}». */
  etichetta?: string | undefined;
}) {
  const { esito, copia } = useCopia();
  const copiato = esito?.riuscito === true && esito.testo === testo;
  const fallito = esito?.riuscito === false && esito.testo === testo;
  return (
    <>
      <Annuncio
        testo={
          copiato
            ? t("copy.done.aria", { testo })
            : fallito
              ? t("copy.failed")
              : null
        }
      />
      <button
        type="button"
        className="bottone minuto btn-ghost"
        aria-label={etichetta ?? t("copy.aria", { testo })}
        onClick={() => copia(testo)}
      >
        {copiato ? t("copy.done") : t("copy.button")}
      </button>
      {fallito && <span className="copia-fallita">{t("copy.failed")}</span>}
    </>
  );
}
