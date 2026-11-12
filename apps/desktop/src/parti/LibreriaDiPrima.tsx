/**
 * La copia del Microsoft Store, al primo avvio: «c'è la libreria che hai già,
 * la copio qui?» (D-97).
 *
 * # Perché una domanda
 *
 * Dalla 3.2.1 la copia del Store tiene una libreria sua, in una cartella che
 * fuori dal pacchetto non esiste (`cartella_di_serie` in `lib.rs`): aprire sul
 * posto quella della copia installata dal sito voleva dire il database da una
 * parte e il suo giornale nella cartella privata del pacchetto. Chi la libreria
 * ce l'ha già — della copia del sito, o della stessa copia del Store prima
 * dell'aggiornamento — non deve ritrovarsi una libreria vuota senza saperlo, e
 * non le si sostituisce niente senza chiedere: la domanda si fa una volta, e
 * solo finché la libreria di qui è vuota. La decide il nucleo
 * (`store_libreria_di_prima`); qui si disegna.
 *
 * Copiare lascia l'originale dov'è, e chiude Rymembra: riaperto, trova la
 * libreria al suo posto.
 */
import { useEffect, useState } from "react";

import { Dialogo } from "../Dialogo";
import { ipc, type LibreriaDiPrima as Libreria } from "../ipc";
import { t } from "../lingue";

export function LibreriaDiPrima({ onErrore }: { onErrore: (e: unknown) => void }) {
  const [libreria, setLibreria] = useState<Libreria | null>(null);
  const [copiando, setCopiando] = useState(false);

  useEffect(() => {
    let vivo = true;
    ipc
      .storeLibreriaDiPrima()
      .then((letta) => {
        if (vivo) setLibreria(letta);
      })
      // Una proposta che non si sa fare non merita la fascia rossa.
      .catch(() => undefined);
    return () => {
      vivo = false;
    };
  }, []);

  if (libreria === null) return null;

  /** «Comincia da capo», e anche Escape: la domanda non torna. */
  const rifiuta = () => {
    setLibreria(null);
    ipc.storeRifiutaLibreriaDiPrima().catch(onErrore);
  };
  const copia = () => {
    setCopiando(true);
    // Se riesce, Rymembra si chiude da sé.
    ipc
      .storeCopiaLibreriaDiPrima()
      .catch((e: unknown) => {
        setCopiando(false);
        onErrore(e);
      });
  };

  return (
    <Dialogo
      titolo={t("store.previous.title")}
      descrizione={t("store.previous.what", { cartella: libreria.cartella })}
      classe="finestrella stretta glass-modal"
      onChiudi={rifiuta}
      chiudibile={!copiando}
      occupato={copiando}
    >
      <div className="tasti-finestrella">
        <button
          type="button"
          className="bottone"
          disabled={copiando}
          onClick={rifiuta}
        >
          {t("store.previous.skip")}
        </button>
        <button
          type="button"
          className="bottone primario"
          disabled={copiando}
          onClick={copia}
          data-fuoco-iniziale
        >
          {copiando ? t("store.previous.copying") : t("store.previous.copy")}
        </button>
      </div>
    </Dialogo>
  );
}
