/**
 * L'avviso dei conti cloud da ricollegare, in cima alla pagina.
 *
 * # Perché
 *
 * Un accesso scaduto — l'app Google lasciata in «Testing» che dura sette
 * giorni, una password cambiata, un permesso revocato — si vedeva solo dentro
 * Impostazioni › Cartelle e scansione, nella scheda dei conti. Chi non ci
 * entrava scopriva il guasto quando un brano di quel conto non partiva, e anche
 * lì l'errore diceva «ricollega» senza dire dove. Qui lo stato dei conti arriva
 * da `FornitoreContiNuvola`, che lo ascolta per tutta la finestra, e il conto
 * da ricollegare si nomina: a differenza di un errore del nucleo, che non porta
 * parametri, qui si sa quale.
 *
 * # Uno alla volta, e si può tacere
 *
 * Il primo dei conti da ricollegare, con «Ricollega…» che apre la stessa
 * finestrella della scheda (`RicollegaConto`). Ricollegato quello, compare il
 * prossimo. La × lo tace fino al prossimo avvio: un conto che non si vuole
 * ricollegare adesso non deve restare rosso sopra ogni pagina, e la scheda
 * dei conti continua a dirlo.
 */
import { useState } from "react";

import { RicollegaConto } from "../AggiungiCartella";
import { NOMI_SERVIZI } from "../cartelleNuvola";
import { useContiNuvolaDellaFinestra } from "../contiNuvola";
import type { ContoNuvola } from "../ipc";
import { t } from "../lingue";
import { Icona } from "./Icone";

export function AvvisoConti({ onNotizia }: { onNotizia: (testo: string) => void }) {
  const { conti } = useContiNuvolaDellaFinestra();
  const [taciuti, setTaciuti] = useState<ReadonlySet<string>>(() => new Set());
  const [ricollega, setRicollega] = useState<ContoNuvola | null>(null);

  const primo = (conti ?? []).find(
    (conto) => conto.stato === "daRicollegare" && !taciuti.has(conto.conto),
  );

  return (
    <>
      {primo !== undefined && (
        <div className="errore toast-card" role="alert" data-avviso="conti">
          <Icona nome="i-nuvola" dim={16} />
          <span>
            {t("cloudAccounts.banner", {
              conto: `${NOMI_SERVIZI[primo.servizio]} · ${primo.nome}`,
            })}
          </span>
          <button
            type="button"
            className="bottone minuto btn-ghost"
            onClick={() => setRicollega(primo)}
          >
            {t("settings.cloudAccounts.reconnect")}
          </button>
          <button
            type="button"
            className="tasto icon-btn"
            aria-label={t("toast.dismiss")}
            onClick={() => setTaciuti((prima) => new Set(prima).add(primo.conto))}
          >
            <Icona nome="i-x" dim={14} />
          </button>
        </div>
      )}
      {ricollega !== null && (
        <RicollegaConto
          conto={ricollega}
          onChiudi={() => setRicollega(null)}
          onFatto={(tornato) =>
            onNotizia(
              t("folders.add.connected", {
                servizio: NOMI_SERVIZI[tornato.servizio],
                nome: tornato.nome,
              }),
            )
          }
        />
      )}
    </>
  );
}
