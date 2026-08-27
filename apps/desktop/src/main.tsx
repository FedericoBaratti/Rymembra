import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { applicaLingua, scegli } from "./lingue";
import { BarraTitolo } from "./parti/BarraTitolo";
import { Simboli } from "./parti/Icone";
import "./stile.css";

/*
 * La lingua **prima** del primo disegno, e non in un effetto.
 *
 * Qui si può usare soltanto quel che il browser sa già — `navigator.language` —
 * perché la scelta salvata sta nel database e arriva con `avvio()`, che è una
 * chiamata. Questa riga serve a far nascere l'interfaccia nella lingua del
 * sistema invece che nell'inglese di ripiego: chi ha scelto altro la vede
 * corretta un istante dopo, mentre la finestra è ancora nascosta (vedi il
 * commento su `pronto` in `App.tsx`).
 *
 * Farlo dentro un effetto vorrebbe dire un fotogramma nella lingua sbagliata per
 * chiunque non parli inglese, e per chi ha una lingua di sistema senza file
 * vorrebbe dire vederla comparire e sparire.
 */
applicaLingua(scegli(null, navigator.language));

const radice = document.getElementById("radice");
if (radice) {
  createRoot(radice).render(
    <StrictMode>
      {/*
       * I simboli **prima** dell'applicazione, e fuori da essa: `<use href="#…">`
       * cerca nel documento, quindi basta che esistano una volta. Montarli dentro
       * un componente che si smonta — la schermata dello Studio, per dire —
       * lascerebbe le icone di quella dopo senza disegno.
       */}
      <Simboli />
      <App />
      {/*
       * I tre comandi della finestra **fuori** dall'applicazione, e non per
       * simmetria coi simboli: `App` si sostituisce con lo Studio quando si apre
       * l'editor delle skin, e una finestra che perde il tasto di chiusura
       * entrando in una schermata sarebbe un guasto. Qui valgono per tutte.
       */}
      <BarraTitolo />
    </StrictMode>,
  );
}
