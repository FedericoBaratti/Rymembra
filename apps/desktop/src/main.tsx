import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { ipc } from "./ipc";
import { applicaLingua, scegli } from "./lingue";
import { RecintoErrori } from "./Recinto";
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

/*
 * I due guasti che il recinto non può prendere.
 *
 * `RecintoErrori` intercetta quel che cade **durante il disegno**, e basta:
 * React non fa passare da lì gli errori dei gestori d'evento, quelli dentro un
 * `setTimeout` e le promesse rifiutate senza `catch`. Sono la maggioranza dei
 * guasti veri — un clic che chiama il nucleo e riceve una risposta che non si
 * aspettava è esattamente questa forma — e in rilascio, senza console, non
 * lasciavano niente dietro di sé.
 *
 * A livello di modulo e non dentro un effetto: un errore che succede fra il
 * primo `import` e il primo disegno non troverebbe nessuno ad ascoltarlo, e
 * quello è proprio il momento in cui è più difficile capire cos'è andato
 * storto.
 *
 * Nessuno dei due chiama `preventDefault`: quel che facevano prima —
 * l'`unhandledrejection` che finisce nella console degli strumenti di sviluppo
 * — deve continuare a farlo. Qui si aggiunge una traccia, non si toglie quella
 * che c'era.
 */
window.addEventListener("error", (evento) => {
  ipc.diarioAnnota("finestra", evento.message);
});
window.addEventListener("unhandledrejection", (evento) => {
  // `reason` è quel che è stato passato a `reject`, e può essere qualunque
  // cosa: un `Error`, un `ErroreIpc` del nucleo, una stringa, `undefined`.
  // `String(...)` è l'unica lettura che non fallisce su tutti e quattro, e il
  // nucleo taglia a una riga sola prima di scrivere.
  ipc.diarioAnnota("promessa", String(evento.reason));
});

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
      {/*
       * Il recinto attorno alla sola `App`, e non attorno a tutto: se cadesse
       * l'applicazione, i simboli e i tre comandi della finestra devono restare
       * dove sono. Una finestra che perde il tasto di chiusura entrando in una
       * schermata di guasto sarebbe un guasto peggiore di quello che sta
       * raccontando — è la stessa ragione scritta qui sotto per `BarraTitolo`.
       */}
      <RecintoErrori>
        <App />
      </RecintoErrori>
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
