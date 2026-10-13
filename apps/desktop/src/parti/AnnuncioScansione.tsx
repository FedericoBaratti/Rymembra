/**
 * La scansione, detta a chi ascolta lo schermo: quando comincia e quando finisce.
 *
 * # Perché non la barra
 *
 * Il riquadro della scansione in basso a destra era un `role="status"` con
 * dentro il conteggio, e il conteggio cambia quattro volte al secondo: un
 * lettore di schermo rileggeva «1.250 di 18.534 file», poi «1.300 di 18.534»,
 * per tutti i venti secondi della prima scansione — sopra qualunque altra cosa
 * si stesse cercando di ascoltare. Un avanzamento che parla da solo è rumore;
 * chi vuole sapere a che punto è va sulla barra, che ha `role="progressbar"` e
 * risponde quando la si interroga.
 *
 * Due notizie meritano di essere dette senza che nessuno le chieda: che è
 * cominciata — la libreria sta per cambiare sotto le dita — e che è finita.
 *
 * # Perché una regione sempre montata
 *
 * Perché una regione viva annuncia i **cambiamenti** del suo testo, e una che
 * nasce già piena spesso non annuncia niente: il riquadro della scansione
 * nasce con la scansione e muore con lei, cioè proprio nei due istanti da dire.
 * Questa invece c'è sempre, vuota, e il testo le cambia sotto.
 *
 * Riceve la scansione **vera**, non quella che il riquadro mostra: sulla pagina
 * delle cartelle il riquadro si nasconde, perché lì l'avanzamento è già
 * scritto, ma la scansione non è finita — e dirlo sarebbe una bugia.
 */
import { useEffect, useRef, useState } from "react";

import type { Avanzamento } from "../ipc";
import { t } from "../lingue";

export function AnnuncioScansione({ scansione }: { scansione: Avanzamento | null }) {
  const inCorso = scansione !== null;
  const [testo, setTesto] = useState("");
  /** Com'era al disegno prima: si annuncia il passaggio, non lo stato. */
  const prima = useRef(inCorso);

  useEffect(() => {
    if (prima.current === inCorso) return;
    prima.current = inCorso;
    setTesto(inCorso ? t("scan.announce.start") : t("scan.announce.end"));
  }, [inCorso]);

  return (
    <div className="solo-lettori" role="status">
      {testo}
    </div>
  );
}
