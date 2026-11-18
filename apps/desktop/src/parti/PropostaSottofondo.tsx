/**
 * «Vuoi che Rymembra parta con Windows e resti nel vassoio quando la chiudi?»
 *
 * Una scheda nella pila delle notifiche, **una volta sola**: la risposta la
 * ricorda il nucleo (`ui.proposta.sottofondo`), e chi ha già risposto — o ha già
 * tutto acceso, o non può accendere niente — non la vede mai.
 *
 * # Cosa fanno le due risposte
 *
 * - **«Sì»** accende l'avvio con Windows **e** la X sul vassoio. Il nucleo
 *   risponde con com'è rimasto tutto, riletto dal sistema: se l'avvio non si è
 *   acceso — un criterio, un registro che non risponde — lo si dice, e le
 *   Impostazioni hanno l'interruttore.
 * - **«No, grazie»** non accende niente, e non si ripropone.
 *
 * Non c'è una terza risposta, né una X per chiuderla: lasciarla lì non è una
 * risposta, e la domanda torna al prossimo avvio finché non ne ha una.
 *
 * # Quando si può mostrare
 *
 * Non lo decide questo componente: lo dice `pronta`. Mai durante `Primo` né
 * durante il giro — chi sta cominciando ha altro da guardare — e, quando F9
 * porterà la finestra delle Novità, dopo di lei. Il punto in cui `pronta` si
 * calcola è uno solo, in `App.tsx` (`propostaPuoMostrarsi`), ed è lì che un
 * nuovo motivo per aspettare si aggiunge: questo componente non conosce né il
 * giro né le Novità, e non deve.
 *
 * # Il computer, e il telefono
 *
 * Questo è il testo **del computer** (`background.proposal.desktop.*`): parla
 * di Windows e del vassoio. La proposta del telefono — il servizio che resta
 * acceso — ha altre parole e altri comandi, e starà nello stesso componente: i
 * due non si assumono a vicenda, e per questo il nucleo risponde «no» a chi non
 * è su un computer. La finestra del telefono non lo monta (`MOBILE`).
 */
import { useEffect, useState } from "react";

import { ipc } from "../ipc";
import { t } from "../lingue";
import { Annuncio } from "./Annuncio";
import { Icona } from "./Icone";

export function PropostaSottofondo({
  pronta,
  onErrore,
  onNotizia,
}: {
  /** Si può mostrare adesso: né `Primo`, né il giro. */
  pronta: boolean;
  onErrore: (e: unknown) => void;
  onNotizia: (testo: string) => void;
}) {
  /** Il nucleo ha detto che c'è da farla. */
  const [daFare, setDaFare] = useState(false);
  /** Una risposta è partita e il nucleo non ha ancora risposto. */
  const [rispondendo, setRispondendo] = useState(false);

  useEffect(() => {
    if (!pronta) return;
    let annullato = false;
    ipc
      .propostaSottofondoDaFare()
      .then((si) => {
        if (!annullato) setDaFare(si);
      })
      .catch(() => {
        /* Una domanda che non si riesce a decidere non si fa: il guasto sta già
           nel diario del nucleo, e una scheda con una domanda che il sistema non
           saprebbe esaudire sarebbe peggio di nessuna. */
      });
    return () => {
      annullato = true;
    };
  }, [pronta]);

  const risponde = (accetta: boolean) => {
    setRispondendo(true);
    ipc
      .propostaSottofondoRispondi(accetta)
      .then((rimasto) => {
        // Il «sì» non è andato fino in fondo: lo si dice, invece di lasciare
        // chi ha risposto con la convinzione che Rymembra parta con Windows.
        if (accetta && !rimasto.avvio.acceso) {
          onNotizia(t("background.proposal.desktop.partial"));
        }
      })
      .catch(onErrore)
      .finally(() => {
        // Qualunque cosa sia successa la domanda è stata fatta: il nucleo ha
        // ricordato la risposta prima di toccare il sistema.
        setDaFare(false);
        setRispondendo(false);
      });
  };

  const visibile = pronta && daFare;
  const frase = t("background.proposal.desktop.text");
  return (
    <>
      <Annuncio testo={visibile ? frase : null} />
      {visibile && (
        <div className="notizia toast-card" role="group" aria-label={frase}>
          <Icona nome="i-computer" dim={16} />
          <span>{frase}</span>
          <button
            type="button"
            className="bottone minuto btn-ghost"
            disabled={rispondendo}
            onClick={() => risponde(true)}
          >
            {t("background.proposal.desktop.yes")}
          </button>
          <button
            type="button"
            className="bottone minuto btn-ghost"
            disabled={rispondendo}
            onClick={() => risponde(false)}
          >
            {t("background.proposal.desktop.no")}
          </button>
        </div>
      )}
    </>
  );
}
