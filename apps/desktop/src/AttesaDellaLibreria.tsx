/**
 * La finestra mentre la libreria si apre: «Apro la libreria…», o «Aggiorno la
 * libreria…» quando c'è una migrazione. Poi la finestra vera.
 *
 * # Perché l'applicazione si monta solo dopo
 *
 * Perché ogni comando, ogni stato e ogni filo di Rymembra è scritto per una
 * libreria che, quando la finestra parla, c'è già: era vero quando la libreria
 * si apriva nel `setup`, e resta vero così. Montare `App` prima vorrebbe dire
 * insegnare a decine di schermate un «non ancora» che non hanno mai dovuto
 * sapere. L'attesa sta in un posto solo, qui; i comandi che arrivano da altre
 * strade la trovano nel nucleo (`stato::con_libreria` aspetta l'apertura, il
 * filo della finestra risponde `library.opening`). Vedi `apertura.ts`.
 *
 * # La finestra si mostra subito, se c'è da aspettare
 *
 * Di solito la libreria è aperta prima che la pagina finisca di caricare, e la
 * finestra la mostra `pronto`, a skin applicata, com'è sempre stato. Se dopo
 * un attimo si sta ancora aprendo, la si mostra senza aspettare la skin: un
 * fotogramma coi colori di serie è meglio di una finestra che non c'è.
 *
 * # Ascolto, poi domanda, poi un'occhiata ogni tanto
 *
 * L'evento `libreria:stato` può partire prima che la pagina ascolti: la
 * domanda, fatta dopo che l'ascolto è aperto, lo copre (`poi` di
 * `useAscolto`). E finché si aspetta, ogni secondo si richiede: se un evento
 * si perde, la finestra non resta ferma su «Apro la libreria…».
 */
import { useEffect, useRef, useState, type ReactNode } from "react";

import { copiaDi, finita, prossima } from "./apertura";
import { ipc, type FaseLibreria } from "./ipc";
import { t } from "./lingue";
import { useAscolto } from "./pagine";

/** Dopo quanto, con la libreria ancora in apertura, la finestra si mostra. */
const MOSTRA_DOPO_MS = 300;

/** Ogni quanto si richiede la fase, finché si aspetta. */
const RICHIEDI_OGNI_MS = 1_000;

export function AttesaDellaLibreria({ children }: { children: ReactNode }) {
  const [fase, setFase] = useState<FaseLibreria | null>(null);
  const adotta = (arrivata: FaseLibreria) => setFase((ora) => prossima(ora, arrivata));
  const chiedi = () => {
    ipc
      .libreriaFase()
      .then(adotta)
      // Una domanda che non arriva non decide niente: la prossima occhiata,
      // o l'evento, risponderanno.
      .catch(() => undefined);
  };
  useAscolto<FaseLibreria>("libreria:stato", adotta, true, chiedi);

  const aspetta = fase !== null && !finita(fase);
  const mostrata = useRef(false);
  useEffect(() => {
    if (!aspetta) return;
    const mostra = window.setTimeout(() => {
      if (mostrata.current) return;
      mostrata.current = true;
      ipc.finestraMostra().catch(() => undefined);
    }, MOSTRA_DOPO_MS);
    const occhiata = window.setInterval(chiedi, RICHIEDI_OGNI_MS);
    return () => {
      window.clearTimeout(mostra);
      window.clearInterval(occhiata);
    };
    // `chiedi` cambia a ogni disegno e fa sempre la stessa cosa.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [aspetta]);

  if (finita(fase)) return <>{children}</>;
  if (fase === null) return null;
  return (
    <div className="vuoto" role="status" aria-live="polite">
      {fase.fase === "aggiornamento" ? (
        <>
          <h2>{t("startup.updating")}</h2>
          <p>{t("startup.updating.detail", { copia: copiaDi(fase.da) })}</p>
        </>
      ) : (
        <h2>{t("startup.opening")}</h2>
      )}
    </div>
  );
}
