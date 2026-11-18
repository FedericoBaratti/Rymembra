/**
 * La proposta del sottofondo, **una volta sola**, in una scheda nella pila
 * delle notifiche: la risposta la ricorda il nucleo (`ui.proposta.sottofondo`),
 * e chi ha già risposto — o ha già tutto acceso, o non può accendere niente —
 * non la vede mai.
 *
 * # Sul computer
 *
 * «Vuoi che Rymembra parta con Windows e resti nel vassoio quando la chiudi?»
 *
 * - **«Sì»** accende l'avvio con Windows **e** la X sul vassoio. Il nucleo
 *   risponde con com'è rimasto tutto, riletto dal sistema: se l'avvio non si è
 *   acceso — un criterio, un registro che non risponde — lo si dice, e le
 *   Impostazioni hanno l'interruttore.
 * - **«No, grazie»** non accende niente, e non si ripropone.
 *
 * # Sul telefono
 *
 * «Vuoi che Rymembra resti attiva in sottofondo, così i tuoi PC la raggiungono
 * sempre?»
 *
 * - **«Sì»** accende il sottofondo con **lo stesso comando dell'interruttore**
 *   delle Impostazioni (`lontanoSottofondo`): è quello che chiede anche le
 *   notifiche, e che avvia il servizio in primo piano. Il sottofondo da solo
 *   non apre niente verso internet — «Raggiungibile da internet» è un sì a
 *   parte —: se è spento lo si dice, invece di lasciare chi ha risposto con la
 *   convinzione che i PC adesso trovino il telefono.
 * - **«No, grazie»** non accende niente, e non si ripropone.
 *
 * La risposta si ricorda **prima** di toccare il sistema, sul telefono come sul
 * computer: se l'accensione fallisce l'errore si dice, e la domanda non si
 * ripete a ogni avvio.
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
 * Una sola chiave per tutti e due (`ui.proposta.sottofondo`): un computer e un
 * telefono sono due installazioni, ognuna con la sua risposta.
 */
import { useEffect, useState } from "react";

import { ipc } from "../ipc";
import { t } from "../lingue";
import { MOBILE } from "../piattaforma";
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
      .then(async (rimasto) => {
        if (MOBILE) {
          // Lo stesso comando dell'interruttore: chiede anche le notifiche. Il
          // nucleo ha già ricordato la risposta, e qui non ha niente da dire
          // sull'avvio con Windows, che sul telefono non esiste.
          if (!accetta) return;
          const stato = await ipc.lontanoSottofondo(true);
          // Il sottofondo da solo non apre niente verso internet.
          if (!stato.attivo) onNotizia(t("background.proposal.phone.needsInternet"));
          return;
        }
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
  const frase = t(MOBILE ? "background.proposal.phone.text" : "background.proposal.desktop.text");
  return (
    <>
      <Annuncio testo={visibile ? frase : null} />
      {visibile && (
        <div className="notizia toast-card" role="group" aria-label={frase}>
          <Icona nome={MOBILE ? "i-telefono" : "i-computer"} dim={16} />
          <span>{frase}</span>
          <button
            type="button"
            className="bottone minuto btn-ghost"
            disabled={rispondendo}
            onClick={() => risponde(true)}
          >
            {t(MOBILE ? "background.proposal.phone.yes" : "background.proposal.desktop.yes")}
          </button>
          <button
            type="button"
            className="bottone minuto btn-ghost"
            disabled={rispondendo}
            onClick={() => risponde(false)}
          >
            {t(MOBILE ? "background.proposal.phone.no" : "background.proposal.desktop.no")}
          </button>
        </div>
      )}
    </>
  );
}
