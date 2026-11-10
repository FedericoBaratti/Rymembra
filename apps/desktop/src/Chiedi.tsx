/**
 * Una domanda con una risposta sola.
 *
 * Esiste perché `window.prompt` non c'è: in un webview Tauri è disabilitata, e
 * anche dove funziona è una finestra del sistema operativo che non conosce i
 * token della skin. Serve a creare e a rinominare una playlist, che sono la
 * stessa domanda fatta due volte.
 */
import { useState } from "react";

import { Dialogo } from "./Dialogo";
import { t } from "./lingue";
import { unaVolta } from "./modale";

export function Chiedi({
  titolo,
  etichetta,
  iniziale = "",
  conferma,
  onRispondi,
  onChiudi,
}: {
  titolo: string;
  etichetta: string;
  iniziale?: string;
  conferma: string;
  onRispondi: (risposta: string) => void;
  onChiudi: () => void;
}) {
  const [testo, setTesto] = useState(iniziale);
  // Il fuoco all'apertura col testo già selezionato, la trappola del Tab,
  // l'Escape, il ritorno del fuoco a chi l'aveva, la pagina dietro inerte e il
  // velo che non chiude a una selezione trascinata fuori: sta tutto in
  // `Dialogo`, che di questi comportamenti porta anche le ragioni.
  const vuoto = testo.trim().length === 0;
  // Una risposta sola, qualunque sia: la finestrella resta montata finché la
  // sua chiusura non è applicata, e Invio tenuto giù su «Nuova playlist» ne
  // creava una a ogni ripetizione. Vedi `unaVolta` in `modale.ts`.
  const [prima] = useState(unaVolta);
  const chiudi = () => {
    if (prima()) onChiudi();
  };

  return (
    <Dialogo
      titolo={titolo}
      classe="finestrella stretta glass-modal"
      onChiudi={chiudi}
      onInvia={() => {
        // Il campo vuoto prima del cancello: un Invio che non risponde non
        // consuma la risposta.
        if (vuoto || !prima()) return;
        onRispondi(testo.trim());
      }}
    >
      <label>
        {etichetta}
        <input
          className="campo field-input"
          value={testo}
          spellCheck={false}
          onChange={(e) => setTesto(e.target.value)}
        />
      </label>
      <div className="tasti-finestrella">
        <button type="button" className="bottone" onClick={chiudi}>
          {t("common.cancel")}
        </button>
        <button type="submit" className="bottone primario" disabled={vuoto}>
          {conferma}
        </button>
      </div>
    </Dialogo>
  );
}
