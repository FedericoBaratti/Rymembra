/**
 * La fascia in cima alla pagina: una notizia riuscita, o un guasto.
 *
 * Due canali e non uno: «5 brani scritti» in rosso, col triangolo d'avviso,
 * sembrava un'esportazione fallita. Il colore di una fascia è metà del suo
 * messaggio.
 *
 * Sta in un componente suo perché la mostrano due posti — sopra la pagina, e
 * sopra lo schermo intero, che copre la pagina e la rende `inert` — e perché
 * lo Studio la disegna con una notizia finta, per chi ridipinge `toast-card`.
 */
import { Icona } from "./Icone";
import { t } from "../lingue";
import { Presenza } from "../presenza";

export function Fascia({
  notizia,
  onChiudiNotizia,
  messaggio,
  dettaglio,
  onRiprova,
  onChiudiErrore,
}: {
  notizia: string | null;
  onChiudiNotizia: () => void;
  /** Il guasto, già in parole. */
  messaggio: string | null;
  /** Quel che il servizio ha scritto con parole sue, se l'ha scritto. */
  dettaglio?: string | null | undefined;
  /** Solo quando riprovare ha senso: senza, il tasto non c'è. */
  onRiprova?: (() => void) | undefined;
  onChiudiErrore: () => void;
}) {
  return (
    <>
      {notizia !== null && (
        <Presenza parte="toast-card">
          <div className="notizia toast-card" role="status">
            <Icona nome="i-check" dim={16} />
            <span>{notizia}</span>
            <button
              type="button"
              className="tasto icon-btn"
              aria-label={t("common.close")}
              onClick={onChiudiNotizia}
            >
              <Icona nome="i-x" dim={14} />
            </button>
          </div>
        </Presenza>
      )}
      {messaggio && (
        <Presenza parte="toast-card">
          <div className="errore toast-card" role="alert">
            <Icona nome="i-alert" dim={16} />
            <span>
              {messaggio}
              {/* Quel che il servizio ha scritto con parole sue: la
                  frase del catalogo dice cosa è successo, questa dice
                  cosa fare. Vedi `dettaglioErrore`. */}
              {dettaglio != null && (
                <span className="dettaglio-errore">{dettaglio}</span>
              )}
            </span>
            {/* «Riprova» compare solo quando il catalogo dice che
                riprovare ha senso, e il caso per cui esiste è la
                cartella di rete che non risponde: lì il gesto non è
                «rifai quel che hai chiesto», è «rimetti la puntina
                dov'era», e il nucleo si è annotato il punto. La stessa
                forma della fascia dell'audio perso qui sopra: un
                `bottone minuto` in linea, senza CSS nuovo. */}
            {onRiprova && (
              <button
                type="button"
                className="bottone minuto btn-ghost"
                onClick={onRiprova}
              >
                {t("common.retry")}
              </button>
            )}
            <button
              type="button"
              className="tasto icon-btn"
              aria-label={t("toast.dismiss")}
              onClick={onChiudiErrore}
            >
              <Icona nome="i-x" dim={14} />
            </button>
          </div>
        </Presenza>
      )}
    </>
  );
}
