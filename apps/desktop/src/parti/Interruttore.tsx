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
 *
 * # L'etichetta è collegata, e la riga intera è il bersaglio
 *
 * Due difetti con una causa sola: l'etichetta e la spiegazione stavano in due
 * `div` che il `role="switch"` non nominava e non citava, e l'unica cosa
 * premibile era la pista — quaranta per ventitré pixel in fondo a una riga larga
 * tutto il pannello. Chi ascolta sentiva «interruttore, acceso» senza la frase
 * che dice cosa accende; chi punta doveva colpire il due per cento della riga
 * che stava leggendo.
 *
 * Quindi: `aria-labelledby` sull'etichetta, `aria-describedby` sulla
 * spiegazione — non `aria-label`, che direbbe la stessa parola due volte e
 * perderebbe la frase — e un clic sulla riga che arriva all'interruttore.
 *
 * Il clic sulla riga è un ingrandimento del bersaglio **del puntatore**, non una
 * seconda via da tastiera: la riga non prende il fuoco e non ha ruolo, perché la
 * via da tastiera c'è già e il `role="switch"` è lei. Aggiungere un secondo
 * elemento premibile con lo stesso effetto vorrebbe dire due fermate di
 * tabulazione per un comando. Il bersaglio del dito *attorno* alla pista lo
 * allarga a quarantaquattro per ventotto uno pseudo-elemento nel foglio, che non
 * sposta di un pixel quel che si vede.
 */
import { useId } from "react";

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
  const idBase = useId();
  const idEtichetta = `${idBase}-etichetta`;
  const idSpiegazione = `${idBase}-spiegazione`;
  const spento = impedito !== undefined;

  const alterna = () => {
    if (!spento) onCambia?.(!acceso);
  };

  return (
    <div
      className="riga-opzione riga-interruttore"
      data-impedito={spento || undefined}
      /* Il clic sul bottone arriva fin qui risalendo, e alternerebbe due volte:
         una per il bottone e una per la riga. Il controllo sul bersaglio è quel
         che tiene il gesto uno solo, e lascia selezionabile il testo — un
         trascinamento sulla spiegazione finisce in un `click` con il bersaglio
         dentro `.che-cosa`, quindi il doppio effetto non c'è comunque. */
      onClick={(e) => {
        if (e.target instanceof Element && e.target.closest("button")) return;
        alterna();
      }}
    >
      <div className="che-cosa">
        <div className="etichetta" id={idEtichetta}>
          {etichetta}
        </div>
        <div className="spiegazione" id={idSpiegazione}>
          {impedito ?? spiegazione}
        </div>
      </div>
      <button
        type="button"
        className="interruttore switch"
        role="switch"
        aria-checked={acceso}
        aria-labelledby={idEtichetta}
        aria-describedby={idSpiegazione}
        disabled={spento}
        title={impedito}
        onClick={alterna}
      >
        <span className="pista switch-track" aria-hidden="true">
          <span className="pallina" />
        </span>
      </button>
    </div>
  );
}
