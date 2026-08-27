/**
 * Un interruttore delle Impostazioni.
 *
 * Stava dentro `Impostazioni.tsx`, che è l'unico posto che lo usava. Adesso lo
 * usa anche la scheda degli aggiornamenti, che è un componente suo per gli
 * stessi motivi di `Scrobbling` — e la scelta era fra spostare l'interruttore
 * qui o passarlo come prop da lì, che è un modo elaborato di dire la stessa
 * cosa con un tipo in più.
 *
 * # `impedito` invece di `disabled`
 *
 * Un interruttore spento senza spiegazione è un difetto: chi lo preme e non
 * vede succedere niente conclude che è rotto. Qui la ragione **è** il modo di
 * spegnerlo — presente vuol dire non toccabile — e prende il posto della
 * spiegazione normale, che descrive cosa farebbe se si potesse.
 */

/** Un interruttore. Spento con la sua ragione quando il comando non c'è. */
export function Interruttore({
  etichetta,
  spiegazione,
  acceso,
  onCambia,
  impedito,
}: {
  etichetta: string;
  spiegazione: string;
  acceso: boolean;
  onCambia?: ((valore: boolean) => void) | undefined;
  /** Perché non si può toccare. Presente ⇒ spento. */
  impedito?: string | undefined;
}) {
  return (
    <div className="riga-opzione">
      <div className="che-cosa">
        <div className="etichetta">{etichetta}</div>
        <div className="spiegazione">{impedito ?? spiegazione}</div>
      </div>
      <button
        type="button"
        className="interruttore switch"
        role="switch"
        aria-checked={acceso}
        aria-label={etichetta}
        disabled={impedito !== undefined}
        title={impedito}
        onClick={() => onCambia?.(!acceso)}
      >
        <span className="pista switch-track" aria-hidden="true">
          <span className="pallina" />
        </span>
      </button>
    </div>
  );
}
