import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { AttesaDellaLibreria } from "./AttesaDellaLibreria";
import { spegniIlBrowser } from "./browserSpento";
import { conDove, rigaDelRifiuto } from "./diarioDellaFinestra";
import { ipc } from "./ipc";
import { leggiLaMemoriaCome } from "./cartelleNuvola";
import { impostaLinguaIniziale, t } from "./lingue";
import { RecintoErrori } from "./Recinto";
import { BarraTitolo } from "./parti/BarraTitolo";
import { ANDROID, MOBILE } from "./piattaforma";
import { Simboli } from "./parti/Icone";
import { temaDiPartenza } from "./tema";
import "./stile.css";
import "./stile/navigazione.css";
import "./stile/libreria.css";
import "./stile/ascolto.css";
import "./stile/artisti-cartelle.css";
import "./stile/studio.css";
import "./stile/studio3.css";
import "./stile/impostazioni.css";
import "./stile/lettore.css";
import "./stile/finestrelle.css";
import "./stile/movimento.css";
import "./stile/importazioni.css";
import "./stile/casa.css";
import "./stile/contrasto.css";
import "./stile/esplora-insieme.css";
// Per ultimi: valgono solo con `data-forma="telefono"`, e lì devono vincere.
import "./stile/telefono.css";
import "./stile/telefono-pagine.css";
import "./stile/telefono-fogli.css";
import "./stile/telefono-lettore.css";
// Il tasto Indietro di Android: si aggancia a `window` da sé. Vedi lì.
import "./indietro";
// Le barre di sistema del telefono, come variabili CSS. Vedi lì.
import "./riquadri";
// La vibrazione del tocco lungo, per tutta l'app. Vedi lì.
import "./vibra";
// Le icone delle barre di sistema del telefono sul tema dell'app. Vedi lì.
import "./barreDiSistema";

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
  ipc.diarioAnnota("finestra", conDove(evento));
});
window.addEventListener("unhandledrejection", (evento) => {
  // `reason` è quel che è stato passato a `reject`, e può essere qualunque
  // cosa: un `Error`, un `ErroreIpc` del nucleo, una stringa, `undefined`. Il
  // caso normale è il secondo, e `String(...)` ne faceva «[object Object]»:
  // vedi `rigaDelRifiuto`. Il nucleo taglia a una riga sola prima di scrivere.
  ipc.diarioAnnota("promessa", rigaDelRifiuto(evento.reason));
});

// Il menù di Edge e le scorciatoie del browser, spenti prima del primo disegno
// e fuori dai campi di testo (decisione D-43): un «Aggiorna» o un F5 rifaceva
// tutta l'interfaccia, perdendo dialoghi aperti e testo non salvato. Solo sul
// computer: vedi `browserSpento.ts`.
if (!MOBILE) spegniIlBrowser(window);

// La forma prima del primo disegno, come la lingua: una pagina disegnata da
// computer e poi rifatta da telefono sarebbe un fotogramma con la barra
// laterale su uno schermo da 400 pixel. Vedi `stile/telefono.css`.
document.documentElement.dataset["forma"] = MOBILE ? "telefono" : "computer";
// E sul telefono il tema, per la stessa ragione: la pagina si vede prima che
// arrivi la skin. Vedi `temaDiPartenza` in `tema.ts`.
if (MOBILE) document.documentElement.dataset["theme"] = temaDiPartenza();

// Le cartelle del telefono con i nomi che il telefono dà loro («Memoria
// interna › Music»), non col percorso del sistema. Vedi `percorsoAndroid`.
if (ANDROID) {
  leggiLaMemoriaCome(() => ({
    interna: t("folders.android.internal"),
    scheda: t("folders.android.card"),
  }));
}

/*
 * La lingua **prima** del primo disegno, e non in un effetto.
 *
 * Qui si può usare soltanto quel che il browser sa già — `navigator.language` —
 * perché la scelta salvata sta nel database e arriva con `avvio()`, che è una
 * chiamata. Questa riga serve a far nascere l'interfaccia nella lingua del
 * sistema invece che nell'inglese di ripiego: chi ha scelto altro la vede
 * corretta un istante dopo, mentre la finestra è ancora nascosta (vedi il
 * commento su `pronto` in `aspettoFinestra.ts`).
 *
 * Farlo dentro un effetto vorrebbe dire un fotogramma nella lingua sbagliata per
 * chiunque non parli inglese, e per chi ha una lingua di sistema senza file
 * vorrebbe dire vederla comparire e sparire.
 *
 * I dizionari non stanno nel pacchetto principale (R21-4): si carica quello della
 * lingua del sistema, e si aspetta — è un file dell'applicazione, non la rete —
 * prima di disegnare. Se nemmeno quello si legge, l'interfaccia nasce con le
 * chiavi e il guasto va nel diario: meglio di una finestra che non si apre.
 *
 * Per ultimo, subito prima di disegnare: tutto il resto qui sopra è sincrono, e
 * i due ascoltatori dei guasti (`error`, `unhandledrejection`) devono esserci
 * prima che ci sia un'attesa — un errore nel varco fra il primo `import` e il
 * primo disegno non troverebbe nessuno ad ascoltarlo.
 */
await impostaLinguaIniziale(navigator.language).catch((errore: unknown) => {
  ipc.diarioAnnota("finestra", `lingua: ${errore instanceof Error ? errore.message : String(errore)}`);
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
        {/*
         * L'applicazione si monta quando la libreria è aperta, o quando si sa
         * che non si aprirà; fino ad allora «Apro la libreria…». Vedi
         * `AttesaDellaLibreria.tsx`.
         */}
        <AttesaDellaLibreria>
          <App />
        </AttesaDellaLibreria>
      </RecintoErrori>
      {/*
       * I tre comandi della finestra **fuori** dall'applicazione, e non per
       * simmetria coi simboli: una finestra che perde il tasto di chiusura
       * entrando in una schermata — quella d'errore del recinto qui sopra, lo
       * Studio che copre tutto — sarebbe un guasto. Qui valgono per tutte.
       *
       * Sul telefono non ci sono: la finestra è lo schermo, e la si lascia con
       * i gesti del sistema. Vedi `piattaforma.ts`.
       */}
      {!MOBILE && <BarraTitolo />}
    </StrictMode>,
  );
}
