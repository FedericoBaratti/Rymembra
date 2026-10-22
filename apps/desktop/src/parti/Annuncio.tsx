/**
 * Una regione viva che c'è sempre, e un testo che le cambia sotto.
 *
 * # Il difetto
 *
 * Le notizie della finestra — «5 brani scritti», «coda sostituita — Annulla»,
 * «versione nuova», l'avanzamento della coda — erano `role="status"` sul
 * riquadro stesso, e il riquadro nasceva **insieme** al suo testo. Una regione
 * viva annuncia i cambiamenti del suo contenuto, e una che compare già piena
 * spesso non la annuncia nessuno: NVDA e l'Assistente vocale la saltano, e chi
 * ascolta lo schermo non sapeva che la coda era stata sostituita — cioè proprio
 * la notizia che chiede un gesto entro dieci secondi.
 *
 * # La regola
 *
 * È quella di `AnnuncioScansione.tsx`: la regione è montata prima che serva,
 * vuota, e il testo arriva dopo, in un effetto — anche quando il componente
 * nasce con la notizia già in mano. Il riquadro che si vede resta com'era,
 * classi comprese, ma non è più lui a parlare: due regioni con la stessa frase
 * la farebbero sentire due volte.
 *
 * Chi la usa passa la frase da dire, o `null` per tacere. Una frase che cambia
 * di continuo — un conteggio — non va passata così com'è: si annuncia il
 * passaggio, non lo stato (vedi `ToastImportazioni.tsx`).
 *
 * Sulla tela dello Studio tace: le notizie che ci si vedono sono finte, messe
 * lì per chi ridipinge `toast-card`, e chi ascolta lo schermo mentre disegna
 * una skin non deve sentirsi dire «5 brani scritti» di niente.
 */
import { useEffect, useState } from "react";

import { useDati } from "../dati";

export function Annuncio({ testo }: { testo: string | null }) {
  const { anteprima } = useDati();
  const [detto, setDetto] = useState("");
  useEffect(() => {
    setDetto(anteprima ? "" : (testo ?? ""));
  }, [testo, anteprima]);
  return (
    <div className="solo-lettori" role="status">
      {detto}
    </div>
  );
}
