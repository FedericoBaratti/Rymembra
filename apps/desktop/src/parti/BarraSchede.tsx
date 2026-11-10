/**
 * La barra delle schede del telefono: Home, Libreria, Cerca, Altro.
 *
 * Prende il posto della barra con cinque voci e del cassetto che c'era prima,
 * e il perché delle quattro sta in `schedeTelefono.ts`. Qui c'è la forma:
 *
 * - **la pillola dietro l'icona**, e non un colore sull'etichetta: si vede col
 *   pollice sopra, e dice «sei qui» anche a chi non legge le etichette;
 * - **l'etichetta sempre**, anche sulla scheda spenta: quattro icone senza
 *   nome sono quattro indovinelli la prima settimana;
 * - **il tocco sulla scheda accesa** risale alla sua prima pagina, o in cima se
 *   ci si è già — lo decide `App`, che sa cosa c'è aperto;
 * - **un punto su Altro** mentre un'importazione scarica: è la scheda da cui la
 *   si guarda, e senza il punto si saprebbe che sta lavorando solo andandoci.
 *
 * Sotto `memo`, come gli altri widget (vedi «I widget sotto `memo`» in
 * `Impaginazione.tsx`): si ridisegna per la scheda e per la lingua, non per
 * ogni cambio di stato dell'app.
 */
import { memo } from "react";

import { t, useLingua } from "../lingue";
import { SCHEDE, type SchedaTelefono } from "../schedeTelefono";
import { Icona } from "./Icone";
import { ConImportazioni } from "./Importazioni";

function etichettaDi(scheda: SchedaTelefono): string {
  switch (scheda) {
    case "home":
      return t("nav.home");
    case "libreria":
      return t("nav.library");
    case "cerca":
      return t("nav.search");
    case "altro":
      return t("nav.more");
  }
}

export const BarraSchede = memo(function BarraSchede({
  scheda,
  onScheda,
}: {
  scheda: SchedaTelefono;
  onScheda: (scheda: SchedaTelefono) => void;
}) {
  useLingua();
  return (
    <nav className="barra-schede" aria-label={t("nav.aria")}>
      {SCHEDE.map(([chiave, icona]) => {
        const qui = chiave === scheda;
        return (
          <button
            key={chiave}
            type="button"
            className="scheda-telefono"
            aria-current={qui ? "page" : undefined}
            data-accesa={qui || undefined}
            onClick={() => onScheda(chiave)}
          >
            <span className="pillola-scheda">
              <Icona nome={icona} dim={22} />
              {chiave === "altro" && (
                <ConImportazioni>
                  {(importazioni) =>
                    importazioni.brano !== null && (
                      <span className="punto-scheda" aria-hidden="true" />
                    )
                  }
                </ConImportazioni>
              )}
            </span>
            <span className="etichetta-scheda">{etichettaDi(chiave)}</span>
          </button>
        );
      })}
    </nav>
  );
});
