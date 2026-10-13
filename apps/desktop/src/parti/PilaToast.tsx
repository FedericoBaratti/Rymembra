/**
 * La pila delle notifiche, in basso a destra.
 *
 * Una pila e non due riquadri fissi. Finché il toast era uno, «in basso a
 * destra» bastava a dire dove. Adesso sono due e possono stare in piedi
 * insieme — una scansione parte da sé quando la coda ha finito di scaricare —
 * e due elementi fissi allo stesso angolo si coprono a vicenda. La pila li
 * impila, e ognuno entra ed esce da sé: vedi `Presenza`.
 */
import type { ReactNode } from "react";

import { numero } from "../formato";
import type { Avanzamento } from "../ipc";
import { t } from "../lingue";
import { Presenza } from "../presenza";
import { Icona } from "./Icone";

export function PilaToast({
  scansione,
  onApriScansione,
  children,
}: {
  /** La scansione in corso, o `null` quando non c'è o non va mostrata qui. */
  scansione: Avanzamento | null;
  onApriScansione: () => void;
  /** Gli altri toast, che sanno da sé quando disegnarsi. */
  children?: ReactNode;
}) {
  return (
    <div className="pila-toast toast-stack">
      {/* La scansione segue chi se ne va.
          Dura venti secondi la prima volta, e nessuno resta a guardare una
          barra per venti secondi: si torna alla libreria, e l'avanzamento va
          in basso a destra invece di sparire. Sparire farebbe credere che sia
          finita — o peggio, che sia stata annullata dal cambio di schermata.

          Senza `role="status"`: il conteggio qui dentro cambia quattro volte al
          secondo, e una regione viva lo rileggeva tutto a chi ascolta lo
          schermo. L'inizio e la fine li dice `AnnuncioScansione`; il punto a cui
          si è arrivati lo dice la barra, a chi la interroga. */}
      {scansione !== null && (
        <Presenza parte="toast-card">
          <div className="toast toast-card">
            <Icona nome="i-scan" dim={16} />
            <div className="dentro">
              <div className="cosa">
                {scansione.totale > 0
                  ? t("toast.scan", {
                      fatti: numero(scansione.fatti),
                      totale: numero(scansione.totale),
                    })
                  : t("toast.scan.comparing")}
              </div>
              {/* L'avanzamento esisteva **solo a schermo**: una barra senza
                  ruolo è un rettangolo che si allunga, e a chi ascolta lo
                  schermo non diceva niente — né che c'è una scansione, né a che
                  punto è. `aria-valuenow` manca finché il totale non si sa
                  («confronto col disco…»), ed è il modo in cui si dichiara un
                  avanzamento indeterminato: un `0` lì vorrebbe dire «non è
                  ancora cominciato», che è un'altra cosa. */}
              <div
                className="toast-progress"
                role="progressbar"
                aria-label={t("toast.progress")}
                aria-valuemin={0}
                aria-valuemax={scansione.totale > 0 ? scansione.totale : undefined}
                aria-valuenow={scansione.totale > 0 ? scansione.fatti : undefined}
              >
                <span
                  style={{
                    width:
                      scansione.totale > 0
                        ? `${Math.round((scansione.fatti / scansione.totale) * 100)}%`
                        : "0%",
                  }}
                />
              </div>
            </div>
            <button
              type="button"
              className="bottone minuto btn-ghost"
              onClick={onApriScansione}
            >
              {t("toast.open")}
            </button>
          </div>
        </Presenza>
      )}

      {children}
    </div>
  );
}
